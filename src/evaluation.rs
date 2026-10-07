//! 同じ入力情報を使う比較資料。利用者の回答や効果は生成しない。
use crate::{
    recording::write_json_new,
    workspace::{self, WorkspaceView},
};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

fn write_new(path: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}
fn fingerprint(bytes: &[u8]) -> String {
    let value = bytes.iter().fold(0xcbf29ce484222325u64, |value, byte| {
        (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{value:016x}")
}
#[derive(Serialize)]
struct ExportSummary<'a> {
    schema_version: u32,
    case_id: &'a str,
    revision: u64,
    common_input_fingerprint: String,
    methods: Vec<&'static str>,
    human_evaluation: &'static str,
}

/// 関係づけを外しても、元の条件・出所・出来事を削らない。
pub fn common_inputs(view: &WorkspaceView) -> serde_json::Value {
    let case = &view.workspace;
    serde_json::json!({"schema_version": 1, "case_id": case.id, "revision": case.revision, "title": case.title,
        "statements": case.statements, "checks": case.checks, "context": case.context,
        "runs": case.evidence.iter().map(|e| serde_json::json!({"id": e.id, "manifest": e.manifest, "events": e.events, "adapter": e.adapter})).collect::<Vec<_>>(),
        "declared_mapping": case.evidence.iter().map(|e| serde_json::json!({"evidence_id":e.id,"check_id":e.check_id,"check_version":e.check_version,"dependencies":e.dependencies,"dependency_origins":e.dependency_origins})).collect::<Vec<_>>(),
        "decisions": case.decisions, "history": case.history})
}

pub fn export(directory: &Path, output: &Path) -> io::Result<()> {
    let view = workspace::view(workspace::load(directory)?);
    let inputs = common_inputs(&view);
    let bytes = serde_json::to_vec_pretty(&inputs).map_err(io::Error::other)?;
    let hash = fingerprint(&bytes);
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(output)?;
    write_json_new(&output.join("input.json"), &inputs)?;
    let common = format!(
        "# 共通の入力情報\n\n事例: {}、版: {}\n\n同一情報の識別値: {}（変更確認用で、暗号学的な署名ではない）\n\n次の情報は3条件で同一。保存物・truth.json・adapter-truth.json・独立した答え合わせは含まない。\n\n```json\n{}\n```\n",
        view.workspace.id,
        view.workspace.revision,
        hash,
        String::from_utf8(bytes).map_err(io::Error::other)?
    );
    write_new(
        &output.join("baseline.md"),
        &format!(
            "# 通常の資料と実行記録\n\n条件・出所・実行記録をそのまま確認する。自動生成した対応づけや次の確認の説明は付けない。必要なら同じ入力から手作業・既存ツールで整理できる。\n\n{common}"
        ),
    )?;
    let linked = serde_json::to_string_pretty(&view.checks).map_err(io::Error::other)?;
    write_new(
        &output.join("linked.md"),
        &format!(
            "# 条件と根拠を対応づける表示\n\n入力はbaselineと同じ。次の説明は共通入力から決定的な規則で生成したもので、新しい観測ではない。\n\n```json\n{linked}\n```\n\n{common}"
        ),
    )?;
    let without_links = view.checks.iter().map(|v| serde_json::json!({"check_id":v.check_id,"check_version":v.check_version,"verdict":v.verdict})).collect::<Vec<_>>();
    write_new(
        &output.join("without-links.md"),
        &format!(
            "# 根拠へのリンクと行動提案を外す表示\n\n条件ごとの判定ラベルは残す。根拠への参照、失効の理由、次の確認の提案を表示しない。入力情報は削らず、元の対応を手作業で調べることはできる。判定器そのものを変更する実験ではない。\n\n```json\n{}\n```\n\n{common}",
            serde_json::to_string_pretty(&without_links).map_err(io::Error::other)?
        ),
    )?;
    write_json_new(
        &output.join("export.json"),
        &ExportSummary {
            schema_version: 1,
            case_id: &view.workspace.id,
            revision: view.workspace.revision,
            common_input_fingerprint: hash,
            methods: vec!["baseline", "linked", "without_links"],
            human_evaluation: "not_conducted",
        },
    )?;
    write_json_new(
        &output.join("response-template.json"),
        &serde_json::json!({"schema_version":1,"status":"blank_template_not_results","participant_id":null,"role":null,"method":null,
        "case_id":view.workspace.id,"case_revision":view.workspace.revision,"expected_behavior":null,"observed_state":null,"evidence":null,"unconfirmed":null,
        "different_understandings":null,"next_confirmation":null,"decision":null,"decision_reason":null,"confidence_0_to_5":null,"elapsed_ms":null,
        "reference_criteria_version":null,"assessment_by_independent_reviewer":null}),
    )?;
    write_new(
        &output.join("evaluation-plan.md"),
        "# 実施前に確定すること\n\nこの出力は比較資料と未記入の回答欄であり、利用者評価の結果ではない。\n\n- #51・#85・#108で、参加者、立場、資料の版、条件の順序、確認時間、事前の説明、独立した参照項目を決める。\n- 同じ入力情報から、通常の方法でも整理する時間とツールを用意する。baselineを単独の弱いログ画面として固定しない。\n- 条件・観測した状態・根拠・未確認事項・次の確認・判断理由を回答してもらう。両者の一致と正しい理解を分ける。\n- 公開することを一律の正解にしない。安心感、根拠不足への気付き、誤った判断、確認時間を分ける。\n- 回答内容、順序効果、条件差、手作業の負担と限界を記録する。\n- 観測欠落時の答え合わせは別に保管する。開発用の既知4条件を未知の見落としの発見や独立した評価として数えない。\n- 既存ツールの実装・設定の再現とgoat/YAGIの比較は別途必要。この出力だけで#86・#87・#108を完了にしない。\n",
    )
}
