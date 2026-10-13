use crate::{
    model::*,
    recording::{load_events, load_manifest},
};
use std::{io, path::Path};

// Truthを受け取らない。観測できなかった情報で判定を補わない。
pub fn assess(manifest: &Manifest, events: &[Event]) -> Assessment {
    let relevant: Vec<_> = events
        .iter()
        .filter(|event| event.correlation == manifest.correlation)
        .collect();
    let saved: Vec<_> = relevant
        .iter()
        .copied()
        .filter(|event| event.source == Source::Server && event.kind == EventKind::SaveCommitted)
        .collect();
    let failed: Vec<_> = relevant
        .iter()
        .copied()
        .filter(|event| {
            (event.source == Source::Server
                && matches!(event.kind, EventKind::SaveFailed | EventKind::InputRejected))
                || (event.source == Source::FaultGate && event.kind == EventKind::RequestBlocked)
        })
        .collect();
    let mut result = Assessment {
        condition_id: manifest.condition.id.clone(),
        condition_version: manifest.condition.version,
        verdict: Verdict::Unknown,
        evidence: vec![],
        reason: String::new(),
        unconfirmed: vec![
            "後の時点での閲覧可能性、クラッシュ復旧、医療業務の導入可否は評価していない".into(),
        ],
        next_action: String::new(),
    };
    let reference = |event: &&Event| format!("events.jsonl#{}", event.sequence);
    if manifest.condition.kind == ConditionKind::Custom {
        result.evidence = relevant
            .iter()
            .copied()
            .filter(|event| event.source == Source::Process)
            .collect::<Vec<_>>()
            .iter()
            .map(reference)
            .collect();
        result.reason = "プロセスの出力・終了だけでは、指定された条件を判定できない".into();
        result.next_action = "条件に対応する観測箇所と、別経路で確かめる状態を決める".into();
    } else if manifest.mode != "demo" || manifest.condition != Condition::demo() {
        result.reason = "条件または実行方法が変わり、この判定規則をそのまま適用できない".into();
        result.next_action = "変更した条件の意味に対応する判定規則を定義して再試行する".into();
    } else if !saved.is_empty() && !failed.is_empty() {
        result.evidence = saved.iter().chain(&failed).map(reference).collect();
        result.reason = "同じ試行・データ・送信について、保存成功と失敗の証拠が食い違う".into();
        result.next_action = "成功と失敗の記録の出所を照合し、条件を固定して再試行する".into();
    } else if !saved.is_empty() {
        result.verdict = Verdict::Satisfied;
        result.evidence = saved.iter().map(reference).collect();
        result.reason = "この試行に対応するサーバーの書き込み・同期処理の成功記録がある".into();
        result.next_action = "別経路の読み出しと内容の照合を、評価側で確認する".into();
    } else if !failed.is_empty() {
        result.verdict = Verdict::NotSatisfied;
        result.evidence = failed.iter().map(reference).collect();
        result.reason = "保存処理の失敗、入力拒否、または受信前の遮断を明示する記録がある".into();
        result.next_action = "失敗した段階と設定を確認し、変更理由を残して再試行する".into();
    } else {
        result.reason = "保存成功・保存失敗を確かめる記録がない。保存失敗とは断定できない".into();
        result
            .unconfirmed
            .push("この試行の保存処理が完了したかは観測記録から確認できない".into());
        result.next_action =
            "保存の観測箇所を確認し、別経路の読み出しを追加してから再試行する".into();
    }
    result
}

pub fn compare_runs(before: &Path, after: &Path) -> io::Result<Comparison> {
    let old = load_manifest(before)?;
    let new = load_manifest(after)?;
    let mut changed = vec![];
    if old.source_fingerprint != new.source_fingerprint {
        changed.push("基盤のコード・依存関係が変わった".into());
    }
    if old.condition != new.condition {
        changed.push("条件の版・内容が変わった".into());
    }
    if old.input != new.input {
        changed.push("入力データが変わった".into());
    }
    if old.fault != new.fault {
        changed.push("故障条件が変わった".into());
    }
    if old.mode != new.mode || old.command != new.command {
        changed.push("実行対象または実行方法が変わった".into());
    }
    if old.os != new.os || old.architecture != new.architecture || old.timeout_ms != new.timeout_ms
    {
        changed.push("実行環境または待機上限が変わった".into());
    }
    Ok(Comparison {
        before_run: old.correlation.run_id.clone(),
        after_run: new.correlation.run_id.clone(),
        same_question: old.condition == new.condition
            && old.input == new.input
            && old.mode == new.mode
            && old.command == new.command,
        prior_evidence_matches_current_configuration: changed.is_empty()
            && old.mode == "demo"
            && new.mode == "demo",
        changed,
        before: assess(&old, &load_events(before)?).verdict,
        after: assess(&new, &load_events(after)?).verdict,
    })
}
