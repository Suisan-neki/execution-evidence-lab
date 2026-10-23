//! 実行方式から独立した試行の識別情報と、記録出力の完了処理。
use crate::{model::*, recording::*};
use std::{
    fs::OpenOptions,
    io::{self, Write},
};

pub fn make_manifest(correlation: Correlation, condition: Condition) -> io::Result<Manifest> {
    Ok(Manifest {
        schema_version: 1,
        correlation,
        previous_run: None,
        source_revision: env!("SOURCE_REVISION").into(),
        source_fingerprint: env!("SOURCE_FINGERPRINT").into(),
        program_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        started_unix_ms: unix_ms()?,
        mode: "unclassified".into(),
        fault: None,
        input: None,
        condition,
        timeout_ms: 5000,
        initial_storage: "新規の試行ディレクトリ。対象の初期状態は実行側で記録する".into(),
        observation_scope: "観測範囲は実行側で記録する".into(),
        command: vec![],
    })
}

pub fn finish_run(directory: std::path::PathBuf, manifest: Manifest) -> io::Result<RunResult> {
    let assessment =
        crate::execution::assess_run(&directory, &manifest, &load_events(&directory)?)?;
    write_json_new(&directory.join("assessment.json"), &assessment)?;
    let mut report = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("report.md"))?;
    writeln!(
        report,
        "# 試行 {}\n\n条件：{}\n\n判定：{}\n\n理由：{}\n\n根拠：{}\n\n次の確認：{}\n\n## 未確認事項\n",
        manifest.correlation.run_id,
        manifest.condition.text,
        assessment.verdict.label(),
        assessment.reason,
        assessment.evidence.join(", "),
        assessment.next_action
    )?;
    for item in &assessment.unconfirmed {
        writeln!(report, "- {item}")?;
    }
    report.sync_all()?;
    write_json_new(
        &directory.join("complete.json"),
        &serde_json::json!({"schema_version": 1, "run_id": manifest.correlation.run_id}),
    )?;
    Ok(RunResult {
        directory,
        manifest,
        assessment,
    })
}
