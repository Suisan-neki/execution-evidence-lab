//! 一件の確認事項と、その版に対する根拠・人の判断を保存する。
//! 試行は取り込み時に複製し、答え合わせ用の保存物は読み込まない。
use crate::{assessment::assess, model::*, recording::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

const MAX_DOCUMENT: u64 = 4 * 1024 * 1024;
pub const LOCAL_INDEX_CONDITION: &str =
    "このローカル試行で、合成S3通知による模擬索引のuploadedへの更新を観測した";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Developer,
    Operator,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    Statement,
    CodeFact,
    Proposal,
    Unanswered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statement {
    pub id: String,
    pub kind: StatementKind,
    pub role: Option<Role>,
    pub text: String,
    pub origin: String,
    #[serde(default)]
    pub check_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    DemoSave,
    LocalIndexUploaded,
    ManualOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub answer: Option<String>,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub version: u32,
    pub text: String,
    pub origin: String,
    pub rule: Rule,
    pub dependencies: Vec<String>,
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterEvent {
    pub sequence: u64,
    pub correlation: Correlation,
    pub kind: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterTrace {
    pub target: serde_json::Value,
    pub events: Vec<AdapterEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub check_id: String,
    pub check_version: u32,
    pub rule: Rule,
    pub dependencies: BTreeMap<String, String>,
    pub dependency_origins: BTreeMap<String, String>,
    pub manifest: Manifest,
    pub events: Vec<Event>,
    pub adapter: Option<AdapterTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Release,
    Hold,
    Modify,
    Verify,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub based_on_revision: u64,
    pub actor: String,
    pub role: Role,
    pub action: DecisionKind,
    pub reason: String,
    pub check_ids: Vec<String>,
    pub change_reference: Option<String>,
    pub confidence: Option<u8>,
    pub elapsed_ms: Option<u64>,
    pub basis: Vec<CheckView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Audit {
    pub revision: u64,
    pub unix_ms: u64,
    pub actor: String,
    pub reason: String,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub schema_version: u32,
    pub revision: u64,
    pub id: String,
    pub title: String,
    pub statements: Vec<Statement>,
    pub context: BTreeMap<String, String>,
    pub checks: Vec<Check>,
    pub evidence: Vec<Evidence>,
    pub decisions: Vec<Decision>,
    pub history: Vec<Audit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceView {
    pub id: String,
    pub run_id: String,
    pub observed: Assessment,
    pub applicable: bool,
    pub invalidated_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Ask,
    RunTest,
    Retest,
    DefineObservation,
    AddObservation,
    InspectConflict,
    InspectFailure,
    ReviewLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    pub kind: ActionKind,
    pub reason: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckView {
    pub check_id: String,
    pub check_version: u32,
    pub verdict: Verdict,
    pub reason: String,
    pub selected_evidence: Option<String>,
    pub evidence: Vec<EvidenceView>,
    pub unanswered: Vec<Question>,
    pub next: SuggestedAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceView {
    pub workspace: Workspace,
    pub checks: Vec<CheckView>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRequest {
    pub expected_revision: u64,
    pub actor: String,
    pub reason: String,
    pub action: Change,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    AddStatement {
        statement: Statement,
    },
    AddCheck {
        check: Check,
    },
    AddQuestion {
        check_id: String,
        question: Question,
    },
    ReviseCheck {
        check_id: String,
        text: String,
        origin: String,
        rule: Rule,
        dependencies: Vec<String>,
    },
    Answer {
        check_id: String,
        question_id: String,
        answer: String,
        origin: String,
    },
    SetContext {
        key: String,
        value: String,
    },
    Import {
        check_id: String,
        run_directory: String,
        use_run_context: bool,
    },
    Decide {
        role: Role,
        action: DecisionKind,
        check_ids: Vec<String>,
        change_reference: Option<String>,
        confidence: Option<u8>,
        elapsed_ms: Option<u64>,
    },
}

fn invalid(text: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, text.into())
}
fn nonempty(value: &str, name: &str) -> io::Result<()> {
    if value.trim().is_empty() || value.len() > 16_384 {
        return Err(invalid(format!(
            "{name}には空でない16384バイト以内の値が必要です"
        )));
    }
    Ok(())
}
fn identifier(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(invalid("識別子は英数字・ハイフン・下線の1〜80文字です"));
    }
    Ok(())
}
fn unique<'a>(values: impl Iterator<Item = &'a str>) -> io::Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        identifier(value)?;
        if !seen.insert(value) {
            return Err(invalid(format!("識別子が重複しています: {value}")));
        }
    }
    Ok(())
}
fn validate_check(check: &Check) -> io::Result<()> {
    identifier(&check.id)?;
    nonempty(&check.text, "条件")?;
    nonempty(&check.origin, "条件の出所")?;
    if check.version == 0 {
        return Err(invalid("条件の版は1以上です"));
    }
    if (check.rule == Rule::DemoSave && check.text != Condition::demo().text)
        || (check.rule == Rule::LocalIndexUploaded && check.text != LOCAL_INDEX_CONDITION)
    {
        return Err(invalid(
            "この判定規則に対応する条件文と一致しません。別の業務条件にはmanual_onlyを使ってください",
        ));
    }
    unique(check.dependencies.iter().map(String::as_str))?;
    unique(check.questions.iter().map(|q| q.id.as_str()))?;
    for q in &check.questions {
        nonempty(&q.text, "質問")?;
        match (&q.answer, &q.origin) {
            (Some(answer), Some(origin)) => {
                nonempty(answer, "回答")?;
                nonempty(origin, "回答の出所")?;
            }
            (None, None) => (),
            _ => return Err(invalid("回答と出所は一緒に記録してください")),
        }
    }
    let required: &[&str] = match check.rule {
        Rule::DemoSave => &["engine", "input", "environment", "fault"],
        Rule::LocalIndexUploaded => &[
            "engine",
            "environment",
            "target",
            "adapter",
            "fault",
            "command",
        ],
        Rule::ManualOnly => &[],
    };
    if required
        .iter()
        .any(|key| !check.dependencies.iter().any(|v| v == key))
    {
        return Err(invalid(
            "判定規則に必要なコード・入力・環境等の依存先が不足しています",
        ));
    }
    Ok(())
}

pub fn validate(workspace: &Workspace) -> io::Result<()> {
    if workspace.schema_version != 1 || workspace.revision == 0 {
        return Err(invalid("未対応の事例形式です"));
    }
    identifier(&workspace.id)?;
    nonempty(&workspace.title, "事例名")?;
    unique(workspace.statements.iter().map(|s| s.id.as_str()))?;
    unique(workspace.checks.iter().map(|c| c.id.as_str()))?;
    unique(workspace.evidence.iter().map(|e| e.id.as_str()))?;
    unique(workspace.decisions.iter().map(|d| d.id.as_str()))?;
    for s in &workspace.statements {
        nonempty(&s.text, "記録")?;
        nonempty(&s.origin, "出所")?;
        unique(s.check_ids.iter().map(String::as_str))?;
        if s.check_ids
            .iter()
            .any(|id| !workspace.checks.iter().any(|c| &c.id == id))
        {
            return Err(invalid("記録が参照する確認事項がありません"));
        }
    }
    for c in &workspace.checks {
        validate_check(c)?;
    }
    for (key, value) in &workspace.context {
        identifier(key)?;
        nonempty(value, "実行条件の値")?;
    }
    for e in &workspace.evidence {
        if !workspace.checks.iter().any(|c| c.id == e.check_id) || e.check_version == 0 {
            return Err(invalid("根拠が参照する確認事項がありません"));
        }
        validate_events(&e.manifest, &e.events)?;
        if let Some(adapter) = &e.adapter {
            validate_adapter(&e.manifest, adapter)?;
        }
    }
    Ok(())
}

/// 条件・発言・回答を補わずに、汎用の事例を始める。
pub fn empty_workspace(id: String, title: String) -> Workspace {
    Workspace {
        schema_version: 1,
        revision: 1,
        id,
        title,
        statements: vec![],
        context: BTreeMap::new(),
        checks: vec![],
        evidence: vec![],
        decisions: vec![],
        history: vec![],
    }
}

/// 合成データだけを使う練習用。実際の担当者の発言や採用要件は含めない。
pub fn demo_workspace() -> Workspace {
    Workspace {
        schema_version: 1,
        revision: 1,
        id: "synthetic-save-review".into(),
        title: "保存の確認範囲と、後の閲覧で必要な条件を分ける練習用事例".into(),
        statements: vec![Statement {
            id: "scope".into(),
            kind: StatementKind::Proposal,
            role: None,
            text: "保存処理の記録と、誰が後で閲覧できるかを別の確認事項にする".into(),
            origin: "Codexが作った練習用の案。CASE-001の本人・運用担当の発言ではない".into(),
            check_ids: vec!["save".into(), "later-view".into()],
        }],
        context: BTreeMap::new(),
        checks: vec![
            Check {
                id: "save".into(),
                version: 1,
                text: Condition::demo().text,
                origin: Condition::demo().origin,
                rule: Rule::DemoSave,
                dependencies: ["engine", "input", "environment", "fault"]
                    .map(String::from)
                    .to_vec(),
                questions: vec![],
            },
            Check {
                id: "later-view".into(),
                version: 1,
                text: "必要な担当者が、必要な記録を必要な時点で閲覧できる".into(),
                origin: "Codexによる未採用の条件候補。具体的な利用条件は未回答".into(),
                rule: Rule::ManualOnly,
                dependencies: vec![
                    "target".into(),
                    "environment".into(),
                    "authorization".into(),
                ],
                questions: vec![
                    Question {
                        id: "viewer".into(),
                        text: "誰が記録を閲覧する必要がありますか".into(),
                        answer: None,
                        origin: None,
                    },
                    Question {
                        id: "record".into(),
                        text: "どの記録を閲覧する必要がありますか".into(),
                        answer: None,
                        origin: None,
                    },
                    Question {
                        id: "timing".into(),
                        text: "どの時点までに閲覧する必要がありますか".into(),
                        answer: None,
                        origin: None,
                    },
                ],
            },
        ],
        evidence: vec![],
        decisions: vec![],
        history: vec![],
    }
}

struct WriteLock(std::path::PathBuf);
impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn lock(directory: &Path) -> io::Result<WriteLock> {
    let path = directory.join("write.lock");
    let _ = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "別の書き込みが進行中です。停止後に残ったwrite.lockは手動確認してください",
                )
            } else {
                error
            }
        })?;
    Ok(WriteLock(path))
}
fn save_revision(directory: &Path, workspace: &Workspace) -> io::Result<()> {
    validate(workspace)?;
    if serde_json::to_vec(workspace)
        .map_err(io::Error::other)?
        .len() as u64
        > MAX_DOCUMENT
    {
        return Err(invalid(
            "事例が4MiBを超えています。別の事例に分けてください",
        ));
    }
    let path = directory
        .join("revisions")
        .join(format!("{:020}.json", workspace.revision));
    let temporary = path.with_extension("pending");
    write_json_new(&temporary, workspace)?;
    // 完成したファイルだけを公開し、既存の版は置き換えない。
    let result = fs::hard_link(&temporary, &path);
    let _ = fs::remove_file(&temporary);
    result
}
pub fn initialize(
    directory: &Path,
    mut workspace: Workspace,
    actor: &str,
    reason: &str,
) -> io::Result<WorkspaceView> {
    nonempty(actor, "記入者")?;
    nonempty(reason, "理由")?;
    if !workspace.evidence.is_empty()
        || !workspace.decisions.is_empty()
        || !workspace.history.is_empty()
    {
        return Err(invalid("新しい事例へ既存の根拠・判断履歴は混入できません"));
    }
    workspace.revision = 1;
    validate(&workspace)?;
    if let Some(parent) = directory.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(directory)?;
    fs::create_dir(directory.join("revisions"))?;
    workspace.history.push(Audit {
        revision: 1,
        unix_ms: unix_ms()?,
        actor: actor.into(),
        reason: reason.into(),
        operation: "initialize".into(),
    });
    save_revision(directory, &workspace)?;
    Ok(view(workspace))
}
pub fn load(directory: &Path) -> io::Result<Workspace> {
    let mut latest = None;
    for entry in fs::read_dir(directory.join("revisions"))? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.len() == 25
            && name.ends_with(".json")
            && name[..20].bytes().all(|b| b.is_ascii_digit())
        {
            let version = name[..20].parse::<u64>().map_err(io::Error::other)?;
            if latest.as_ref().is_none_or(|(v, _)| version > *v) {
                latest = Some((version, entry.path()));
            }
        }
    }
    let (version, path) = latest.ok_or_else(|| invalid("事例の保存済みの版がありません"))?;
    if fs::metadata(&path)?.len() > MAX_DOCUMENT {
        return Err(invalid("事例が4MiBを超えています"));
    }
    let workspace: Workspace = read_json(&path)?;
    if workspace.revision != version {
        return Err(invalid("事例の版とファイル名が一致しません"));
    }
    validate(&workspace)?;
    Ok(workspace)
}

fn validate_events(manifest: &Manifest, events: &[Event]) -> io::Result<()> {
    if manifest.schema_version != 1 {
        return Err(invalid("未対応の試行形式です"));
    }
    let mut previous = 0;
    for (index, event) in events.iter().enumerate() {
        if event.sequence != index as u64 + 1 || event.elapsed_us < previous {
            return Err(invalid("出来事の順序が不正です"));
        }
        previous = event.elapsed_us;
    }
    Ok(())
}
fn validate_adapter(manifest: &Manifest, trace: &AdapterTrace) -> io::Result<()> {
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
        serde_json::from_str(include_str!("../adapters/vitalsensing-targets.json"))
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
fn document<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    if fs::metadata(path)?.len() > MAX_DOCUMENT {
        return Err(invalid("取り込むファイルが4MiBを超えています"));
    }
    read_json(path)
}
fn read_trace(directory: &Path, manifest: &Manifest) -> io::Result<Option<AdapterTrace>> {
    if !directory.join("target.json").exists() && !directory.join("adapter-events.jsonl").exists() {
        return Ok(None);
    }
    let target = document(&directory.join("target.json"))?;
    let path = directory.join("adapter-events.jsonl");
    if fs::metadata(&path)?.len() > MAX_DOCUMENT {
        return Err(invalid("アダプターの記録が大きすぎます"));
    }
    let events = fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<AdapterEvent>, _>>()
        .map_err(io::Error::other)?;
    let trace = AdapterTrace { target, events };
    validate_adapter(manifest, &trace)?;
    Ok(Some(trace))
}
fn dependency_values(
    manifest: &Manifest,
    adapter: Option<&AdapterTrace>,
) -> io::Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::from([
        ("engine".into(), manifest.source_fingerprint.clone()),
        (
            "environment".into(),
            format!(
                "{}/{}; timeout_ms={}",
                manifest.os, manifest.architecture, manifest.timeout_ms
            ),
        ),
        (
            "input".into(),
            serde_json::to_string(&manifest.input).map_err(io::Error::other)?,
        ),
        (
            "fault".into(),
            serde_json::to_string(&manifest.fault).map_err(io::Error::other)?,
        ),
        (
            "command".into(),
            serde_json::to_string(&manifest.command).map_err(io::Error::other)?,
        ),
    ]);
    if let Some(trace) = adapter {
        values.insert(
            "target".into(),
            format!(
                "{}@{}",
                trace.target["repository"].as_str().unwrap_or("unknown"),
                trace.target["revision"].as_str().unwrap_or("unknown")
            ),
        );
        values.insert(
            "adapter".into(),
            trace.target["adapterSha256"]
                .as_str()
                .unwrap_or("unknown")
                .into(),
        );
        values.insert(
            "fault".into(),
            trace.target["fault"].as_str().unwrap_or("unknown").into(),
        );
        values.insert(
            "environment".into(),
            format!(
                "{}; node={}",
                values["environment"],
                trace.target["node"].as_str().unwrap_or("unknown")
            ),
        );
    }
    Ok(values)
}

fn observed(evidence: &Evidence) -> Assessment {
    let mut result = Assessment {
        condition_id: evidence.check_id.clone(),
        condition_version: evidence.check_version,
        verdict: Verdict::Unknown,
        evidence: vec![],
        reason: String::new(),
        unconfirmed: vec![],
        next_action: String::new(),
    };
    match evidence.rule {
        Rule::DemoSave => {
            result = assess(&evidence.manifest, &evidence.events);
            result.condition_id = evidence.check_id.clone();
            result.condition_version = evidence.check_version;
        }
        Rule::ManualOnly => {
            result.reason =
                "この条件に対応する判定規則は未定義。プロセス終了や別条件の成立から判断しない"
                    .into();
        }
        Rule::LocalIndexUploaded => {
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
                    .filter(|e| {
                        e.kind == "index_failed" && e.detail["key"] == issued[0].detail["key"]
                    })
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
                        "対象の発行と更新記録の順序が食い違うか、失敗記録の対象キーを特定できない"
                            .into();
                } else if !positive.is_empty() && !negative.is_empty() {
                    result.reason = "同じ試行の索引更新の成功・失敗記録が食い違う".into();
                } else if !positive.is_empty() {
                    result.verdict = Verdict::Satisfied;
                    result.reason = "このローカル試行で模擬索引のuploadedへの更新を観測した".into();
                } else if !negative.is_empty() {
                    result.verdict = Verdict::NotSatisfied;
                    result.reason = "模擬索引の更新前の人工失敗を観測した".into();
                } else {
                    result.reason =
                        "uploadedへの更新を確かめる観測がない。更新失敗とは断定しない".into();
                }
            } else {
                result.reason = "固定版アダプターの対象情報と出来事がない".into();
            }
        }
    }
    result
}

pub fn view(workspace: Workspace) -> WorkspaceView {
    let checks = workspace
        .checks
        .iter()
        .map(|check| {
            let evidence: Vec<_> = workspace
                .evidence
                .iter()
                .filter(|e| e.check_id == check.id)
                .map(|e| {
                    let mut invalidated_by = vec![];
                    if e.check_version != check.version || e.rule != check.rule {
                        invalidated_by.push(format!(
                            "条件の版または判定規則が変わった（根拠の版{}、現在{}）",
                            e.check_version, check.version
                        ));
                    }
                    for key in &check.dependencies {
                        match (e.dependencies.get(key), workspace.context.get(key)) {
                            (Some(old), Some(new)) if old == new => (),
                            (Some(_), Some(_)) => {
                                invalidated_by.push(format!("依存先 {key} が変わった"))
                            }
                            _ => invalidated_by.push(format!("依存先 {key} の値を確認できない")),
                        }
                    }
                    EvidenceView {
                        id: e.id.clone(),
                        run_id: e.manifest.correlation.run_id.clone(),
                        observed: observed(e),
                        applicable: invalidated_by.is_empty(),
                        invalidated_by,
                    }
                })
                .collect();
            // 最新の取り込みを表示する。新しい試行が不明でも、古い成功へ戻って埋めない。
            let last = evidence.last();
            let unanswered: Vec<_> = check
                .questions
                .iter()
                .filter(|q| q.answer.is_none())
                .cloned()
                .collect();
            let (verdict, reason) = if !unanswered.is_empty() {
                (
                    Verdict::Unknown,
                    "期待する動作を決める利用条件に未回答がある".into(),
                )
            } else if let Some(last) = last {
                if last.applicable {
                    (last.observed.verdict.clone(), last.observed.reason.clone())
                } else {
                    (
                        Verdict::Unknown,
                        format!("再確認待ち: {}", last.invalidated_by.join("、")),
                    )
                }
            } else {
                (
                    Verdict::Unknown,
                    "この確認事項に対応する試行の根拠はまだない".into(),
                )
            };
            let next = if let Some(question) = unanswered.first() {
                SuggestedAction {
                    kind: ActionKind::Ask,
                    reason: "利用条件が未回答で、試験の対象を決められない".into(),
                    text: question.text.clone(),
                }
            } else if last.is_some_and(|e| !e.applicable) {
                SuggestedAction {
                    kind: ActionKind::Retest,
                    reason: reason.clone(),
                    text: "変更した条件・依存先を固定し、この確認事項を新しい試行で確かめる".into(),
                }
            } else if check.rule == Rule::ManualOnly {
                SuggestedAction {
                    kind: ActionKind::DefineObservation,
                    reason: "この条件を判定する観測方法が未定義".into(),
                    text: "必要な状態と観測箇所、別経路での確認方法を決める".into(),
                }
            } else if last.is_none() {
                SuggestedAction {
                    kind: ActionKind::RunTest,
                    reason: "対応する試行がない".into(),
                    text: "条件と実行環境を固定して試行する".into(),
                }
            } else if reason.contains("食い違") {
                SuggestedAction {
                    kind: ActionKind::InspectConflict,
                    reason: reason.clone(),
                    text: "成功と失敗の記録の出所・試行識別子を照合する".into(),
                }
            } else if verdict == Verdict::NotSatisfied {
                SuggestedAction {
                    kind: ActionKind::InspectFailure,
                    reason: reason.clone(),
                    text: "失敗した段階を確認し、対応理由と改修への参照を残す".into(),
                }
            } else if verdict == Verdict::Unknown {
                SuggestedAction {
                    kind: ActionKind::AddObservation,
                    reason: reason.clone(),
                    text: "不足する観測箇所を確認し、追加の観測を伴う新しい試行を行う".into(),
                }
            } else {
                SuggestedAction {
                    kind: ActionKind::ReviewLimits,
                    reason: "確認できた範囲以外の利用条件は未確認".into(),
                    text: "後の閲覧など、別の確認事項と残る不確実性を人が確認する".into(),
                }
            };
            CheckView {
                check_id: check.id.clone(),
                check_version: check.version,
                verdict,
                reason,
                selected_evidence: last.map(|e| e.id.clone()),
                evidence,
                unanswered,
                next,
            }
        })
        .collect();
    WorkspaceView { workspace, checks }
}

pub fn change(directory: &Path, request: ChangeRequest) -> io::Result<WorkspaceView> {
    nonempty(&request.actor, "記入者")?;
    nonempty(&request.reason, "理由")?;
    let _guard = lock(directory)?;
    let mut workspace = load(directory)?;
    if workspace.revision != request.expected_revision {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "別の更新があります。最新版を読み直してください",
        ));
    }
    let find_check = |checks: &[Check], id: &str| {
        checks
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| invalid("確認事項がありません"))
    };
    let operation = match request.action {
        Change::AddStatement { statement } => {
            workspace.statements.push(statement);
            "add_statement"
        }
        Change::AddCheck { check } => {
            if check.version != 1 {
                return Err(invalid("新しい条件の版は1です"));
            }
            workspace.checks.push(check);
            "add_check"
        }
        Change::AddQuestion { check_id, question } => {
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &mut workspace.checks[i];
            check.version = check
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("条件の版の上限です"))?;
            check.questions.push(question);
            "add_question"
        }
        Change::ReviseCheck {
            check_id,
            text,
            origin,
            rule,
            dependencies,
        } => {
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &mut workspace.checks[i];
            if check.text != text || check.rule != rule {
                for question in &mut check.questions {
                    question.answer = None;
                    question.origin = None;
                }
            }
            check.version = check
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("条件の版の上限です"))?;
            check.text = text;
            check.origin = origin;
            check.rule = rule;
            check.dependencies = dependencies;
            "revise_check"
        }
        Change::Answer {
            check_id,
            question_id,
            answer,
            origin,
        } => {
            nonempty(&answer, "回答")?;
            nonempty(&origin, "回答の出所")?;
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &mut workspace.checks[i];
            let question = check
                .questions
                .iter_mut()
                .find(|q| q.id == question_id)
                .ok_or_else(|| invalid("質問がありません"))?;
            question.answer = Some(answer);
            question.origin = Some(origin);
            check.version = check
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("条件の版の上限です"))?;
            "answer"
        }
        Change::SetContext { key, value } => {
            identifier(&key)?;
            nonempty(&value, "実行条件の値")?;
            workspace.context.insert(key, value);
            "set_context"
        }
        Change::Import {
            check_id,
            run_directory,
            use_run_context,
        } => {
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &workspace.checks[i];
            let directory = Path::new(&run_directory);
            let complete: serde_json::Value = document(&directory.join("complete.json"))?;
            let manifest: Manifest = document(&directory.join("manifest.json"))?;
            if complete["schema_version"] != 1
                || complete["run_id"].as_str() != Some(manifest.correlation.run_id.as_str())
            {
                return Err(invalid("出力完了の記録が試行に対応していません"));
            }
            if fs::metadata(directory.join("events.jsonl"))?.len() > MAX_DOCUMENT {
                return Err(invalid("出来事の記録が大きすぎます"));
            }
            let events = load_events(directory)?;
            validate_events(&manifest, &events)?;
            if check.rule == Rule::DemoSave
                && (manifest.mode != "demo" || manifest.condition != Condition::demo())
            {
                return Err(invalid("この規則は元のデモの保存条件だけに適用できます"));
            }
            let adapter = read_trace(directory, &manifest)?;
            if check.rule == Rule::LocalIndexUploaded
                && (adapter.is_none() || manifest.mode != "capture")
            {
                return Err(invalid(
                    "この規則にはcaptureで取得した固定版ローカルアダプターの記録が必要です",
                ));
            }
            let values = dependency_values(&manifest, adapter.as_ref())?;
            let dependencies = check
                .dependencies
                .iter()
                .filter_map(|key| {
                    values
                        .get(key)
                        .or_else(|| workspace.context.get(key))
                        .map(|value| (key.clone(), value.clone()))
                })
                .collect();
            let dependency_origins = check
                .dependencies
                .iter()
                .map(|key| {
                    (
                        key.clone(),
                        if values.contains_key(key) {
                            "実行記録または固定版アダプターの対象情報".into()
                        } else {
                            "記入者が事例に宣言した値。実環境との一致は別途確認が必要".into()
                        },
                    )
                })
                .collect();
            if use_run_context {
                workspace.context.extend(values);
            }
            workspace.evidence.push(Evidence {
                id: format!("evidence-{}", workspace.evidence.len() + 1),
                check_id,
                check_version: check.version,
                rule: check.rule.clone(),
                dependencies,
                dependency_origins,
                manifest,
                events,
                adapter,
            });
            "import"
        }
        Change::Decide {
            role,
            action,
            check_ids,
            change_reference,
            confidence,
            elapsed_ms,
        } => {
            if check_ids.is_empty()
                || confidence.is_some_and(|c| c > 5)
                || elapsed_ms.is_some_and(|t| t > 86_400_000)
            {
                return Err(invalid(
                    "確認事項と、0〜5の安心感・24時間以内の記録時間が必要です",
                ));
            }
            unique(check_ids.iter().map(String::as_str))?;
            for id in &check_ids {
                find_check(&workspace.checks, id)?;
            }
            if let Some(reference) = &change_reference {
                nonempty(reference, "改修への参照")?;
            }
            let basis = view(workspace.clone())
                .checks
                .into_iter()
                .filter(|v| check_ids.contains(&v.check_id))
                .collect();
            workspace.decisions.push(Decision {
                id: format!("decision-{}", workspace.decisions.len() + 1),
                based_on_revision: workspace.revision,
                actor: request.actor.clone(),
                role,
                action,
                reason: request.reason.clone(),
                check_ids,
                change_reference,
                confidence,
                elapsed_ms,
                basis,
            });
            "decide"
        }
    };
    workspace.revision = workspace
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("事例の版の上限です"))?;
    workspace.history.push(Audit {
        revision: workspace.revision,
        unix_ms: unix_ms()?,
        actor: request.actor,
        reason: request.reason,
        operation: operation.into(),
    });
    save_revision(directory, &workspace)?;
    Ok(view(workspace))
}

/// デモの再試行。既存の試行を上書きせず、前の試行への参照を残す。
pub fn run_demo(
    directory: &Path,
    expected_revision: u64,
    actor: String,
    reason: String,
    check_id: String,
    fault: Fault,
) -> io::Result<WorkspaceView> {
    nonempty(&actor, "記入者")?;
    nonempty(&reason, "理由")?;
    let case = load(directory)?;
    if case.revision != expected_revision {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "別の更新があります。最新版を読み直してください",
        ));
    }
    let check = case
        .checks
        .iter()
        .find(|c| c.id == check_id)
        .ok_or_else(|| invalid("確認事項がありません"))?;
    if check.rule != Rule::DemoSave {
        return Err(invalid("この試行はデモの保存条件だけを確認します"));
    }
    let previous_run = case
        .evidence
        .iter()
        .rev()
        .find(|e| e.check_id == check_id)
        .map(|e| e.manifest.correlation.run_id.clone());
    let run = crate::experiment::run_trial(TrialConfig {
        output: directory.join("runs"),
        data: Data {
            name: "架空の利用者".into(),
            age: 30,
        },
        fault,
        previous_run,
    })?;
    change(
        directory,
        ChangeRequest {
            expected_revision,
            actor,
            reason,
            action: Change::Import {
                check_id,
                run_directory: run.directory.to_string_lossy().into_owned(),
                use_run_context: true,
            },
        },
    )
}
