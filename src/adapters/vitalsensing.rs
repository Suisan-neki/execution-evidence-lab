//! VitalSensingの固定対象とローカル索引の判定。汎用の観測形式へ業務条件を埋め込まない。
use crate::{
    model::*,
    workspace::{AdapterTrace, Evidence},
};
use std::{collections::BTreeMap, io};
fn invalid(text: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, text.into())
}
pub fn validate(manifest: &Manifest, trace: &AdapterTrace) -> io::Result<()> {
    let correlation: Correlation =
        serde_json::from_value(trace.target["correlation"].clone()).map_err(io::Error::other)?;
    if correlation != manifest.correlation
        || trace.target["repository"] != "Medliss-share/vitalsensing"
    {
        return Err(invalid(
            "アダプターの対象・版・試行識別子が対応していません",
        ));
    }
    let expected: BTreeMap<String, String> =
        serde_json::from_value(trace.target["expectedBlobs"].clone()).map_err(io::Error::other)?;
    let targets: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_str(include_str!("../../adapters/vitalsensing-targets.json"))
            .map_err(io::Error::other)?;
    let pinned = trace.target["revision"]
        .as_str()
        .and_then(|revision| targets.get(revision))
        .ok_or_else(|| invalid("アダプターの対象版が固定情報にありません"))?;
    if &expected != pinned
        || trace.target["adapterSha256"]
            .as_str()
            .is_none_or(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(invalid("対象ファイルの固定情報が一致しません"));
    }
    for (i, event) in trace.events.iter().enumerate() {
        if event.sequence != i as u64 + 1 || event.correlation != manifest.correlation {
            return Err(invalid(
                "アダプターの出来事が別の試行に属するか、順序が不正です",
            ));
        }
    }
    Ok(())
}

pub fn assess(evidence: &Evidence, mut result: Assessment) -> Assessment {
    result.unconfirmed = [
        "実S3への保存",
        "実DynamoDBの状態",
        "iOSの表示",
        "管理画面での閲覧",
        "医療業務の利用条件",
    ]
    .map(String::from)
    .to_vec();
    if let Some(trace) = &evidence.adapter {
        let issued: Vec<_> = trace
            .events
            .iter()
            .filter(|e| e.kind == "upload_target_issued" && e.detail["statusCode"] == 200)
            .filter(|e| e.detail["key"].as_str().is_some_and(|key| !key.is_empty()))
            .collect();
        if issued.len() != 1 {
            result.reason = "この試行の対象キーを一件に特定できない".into();
            return result;
        }
        let positive: Vec<_> = trace
            .events
            .iter()
            .filter(|e| {
                e.kind == "index_updated"
                    && e.detail["status"] == "uploaded"
                    && e.detail["key"] == issued[0].detail["key"]
            })
            .collect();
        let negative: Vec<_> = trace
            .events
            .iter()
            .filter(|e| e.kind == "index_failed" && e.detail["key"] == issued[0].detail["key"])
            .collect();
        let mut referenced = vec![issued[0]];
        referenced.extend(positive.iter().chain(&negative).copied());
        referenced.extend(trace.events.iter().filter(|e| {
            e.kind == "index_failed" && e.detail["key"].as_str().is_none_or(str::is_empty)
        }));
        referenced.sort_by_key(|e| e.sequence);
        result.evidence = referenced
            .iter()
            .map(|e| format!("adapter-events.jsonl#{}", e.sequence))
            .collect();
        let ambiguous_failure = trace.events.iter().any(|e| {
            e.kind == "index_failed" && e.detail["key"].as_str().is_none_or(str::is_empty)
        });
        let invalid_order = positive
            .iter()
            .chain(&negative)
            .any(|e| e.sequence <= issued[0].sequence);
        if invalid_order || ambiguous_failure {
            result.reason =
                "対象の発行と更新記録の順序が食い違うか、失敗記録の対象キーを特定できない".into();
        } else if !positive.is_empty() && !negative.is_empty() {
            result.reason = "同じ試行の索引更新の成功・失敗記録が食い違う".into();
        } else if !positive.is_empty() {
            result.verdict = Verdict::Satisfied;
            result.reason = "このローカル試行で模擬索引のuploadedへの更新を観測した".into();
        } else if !negative.is_empty() {
            result.verdict = Verdict::NotSatisfied;
            result.reason = "模擬索引の更新前の人工失敗を観測した".into();
        } else {
            result.reason = "uploadedへの更新を確かめる観測がない。更新失敗とは断定しない".into();
        }
    } else {
        result.reason = "固定版アダプターの対象情報と出来事がない".into();
    }

    result
}
