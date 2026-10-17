//! 一件の確認事項と、その版に対する根拠・人の判断を保存する。
//! 試行は取り込み時に複製し、答え合わせ用の保存物は読み込まない。
use crate::{
    assessment::assess,
    execution::{Criterion, ExecutionTrace},
    model::*,
    recording::*,
};
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
    /// 人が自分の言葉で書いた期待・発言。
    Statement,
    CodeFact,
    /// コード・テスト・解析からの提案。人の決定ではない。
    Proposal,
    Unanswered,
    /// 技術者による、期待と実装・確認方法の対応づけの説明。
    Implementation,
    /// 期待・説明・結果の食い違いの指摘。
    Discrepancy,
    /// 結果や説明への質問・異論・補足。
    Comment,
}

/// 記録を読んだ人が、その時点の条件の版で内容を確認したこと。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirmation {
    pub actor: String,
    pub role: Role,
    pub revision: u64,
    pub check_versions: BTreeMap<String, u32>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub actor: String,
    pub revision: u64,
    pub text: String,
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
    /// 実装の対応づけで示すコード。固定コミットの行を指す。
    #[serde(default)]
    pub source: Option<CodeReference>,
    /// 結果への質問・異論の対象となる根拠。
    #[serde(default)]
    pub evidence_id: Option<String>,
    /// 以下は保存時に基盤が記入する。送られた値は使わない。
    #[serde(default)]
    pub recorded_by: String,
    #[serde(default)]
    pub recorded_revision: u64,
    #[serde(default)]
    pub check_versions: BTreeMap<String, u32>,
    #[serde(default)]
    pub confirmations: Vec<Confirmation>,
    #[serde(default)]
    pub resolution: Option<Resolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    DemoSave,
    LocalIndexUploaded,
    ManualOnly,
    ObservedAssertion,
    ReportedAssertion,
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
    #[serde(default)]
    pub source: Option<CodeReference>,
    #[serde(default)]
    pub observation: Option<Criterion>,
    pub rule: Rule,
    pub dependencies: Vec<String>,
    pub questions: Vec<Question>,
}

/// 確認事項が参照するコード。実行したコードとの一致を証明する情報ではない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeReference {
    pub repository: String,
    pub commit: String,
    pub path: String,
    pub line: u32,
    pub end_line: u32,
}

pub fn validate_source(source: &CodeReference) -> io::Result<()> {
    let parts: Vec<_> = source.repository.split('/').collect();
    let valid_part = |value: &str| {
        !value.is_empty()
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if parts.len() != 2
        || !parts.iter().all(|v| valid_part(v))
        || !parts[0]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || source.repository.len() > 200
        || source.commit.len() != 40
        || !source
            .commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || source.path.len() > 1024
        || source
            .path
            .split('/')
            .any(|v| v.is_empty() || v == "." || v == "..")
        || source.path.chars().any(|c| c.is_control() || c == '\\')
        || source.line == 0
        || source.end_line < source.line
        || source.end_line > 1_000_000
    {
        return Err(invalid(
            "リポジトリ・固定コミット・相対パス・行範囲を確認してください",
        ));
    }
    Ok(())
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
    #[serde(default)]
    pub source: Option<CodeReference>,
    #[serde(default)]
    pub observation: Option<Criterion>,
    #[serde(default)]
    pub execution: Option<ExecutionTrace>,
    #[serde(default)]
    pub reported: Option<crate::reported::ReportedTrace>,
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
    /// 試験案を使った試行の要約。旧形式の試行にはない。
    #[serde(default)]
    pub summary: Option<crate::trial::Summary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationStatus {
    /// 誰も確認していない。
    Unconfirmed,
    /// 一方の役割だけが現在の条件の版で確認した。
    PartlyConfirmed,
    /// 運用担当と開発者の両方が、現在の条件の版で確認した。
    BothConfirmed,
    /// 確認後に条件が変わり、再確認が必要。
    Outdated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatementView {
    pub id: String,
    pub status: ConfirmationStatus,
    pub status_label: String,
    pub current_roles: Vec<Role>,
    pub open: bool,
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
    #[serde(default)]
    pub statements: Vec<StatementView>,
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
    ConfigureReportedObservation {
        check_id: String,
        criterion: Criterion,
    },
    ImportReported {
        condition: Condition,
        trace: crate::reported::ReportedTrace,
    },
    AddStatement {
        statement: Statement,
    },
    /// 記録を読んだ人が、現在の条件の版で内容を確認する。
    ConfirmStatement {
        statement_id: String,
        role: Role,
        note: String,
    },
    /// 指摘・質問への対応を残す。元の記録は消さない。
    ResolveStatement {
        statement_id: String,
        text: String,
    },
    AddCheck {
        check: Check,
    },
    ConfigureObservation {
        check_id: String,
        criterion: Criterion,
    },
    BindSource {
        check_id: String,
        source: CodeReference,
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
fn context_value(value: &str) -> io::Result<()> {
    if value.trim().is_empty() || value.len() > 256 * 1024 {
        return Err(invalid("実行条件の値は空でない256KiB以内です"));
    }
    Ok(())
}
fn identifier(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(invalid("識別子は英数字・ハイフン・下線の1〜128文字です"));
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
    if let Some(source) = &check.source {
        validate_source(source)?;
    }
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
    if matches!(
        check.rule,
        Rule::ObservedAssertion | Rule::ReportedAssertion
    ) {
        let criterion = check
            .observation
            .as_ref()
            .ok_or_else(|| invalid("判定する観測名と対象が必要です"))?;
        crate::execution::validate_criterion(criterion)?;
        if check.rule == Rule::ObservedAssertion && check.source.is_none() {
            return Err(invalid("固定版のコード参照が必要です"));
        }
    }
    if check.rule == Rule::ReportedAssertion
        && !check.dependencies.contains(&format!("report-{}", check.id))
    {
        return Err(invalid("報告された構成を照合する依存先が必要です"));
    }
    if check.rule == Rule::ObservedAssertion
        && !check
            .dependencies
            .contains(&format!("execution-{}", check.id))
    {
        return Err(invalid(
            "固定版の入力・起動方法・ランタイムを照合する依存先が必要です",
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
        Rule::ManualOnly | Rule::ObservedAssertion | Rule::ReportedAssertion => &[],
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
        if let Some(source) = &s.source {
            validate_source(source)?;
        }
        if s.evidence_id
            .as_ref()
            .is_some_and(|id| !workspace.evidence.iter().any(|e| &e.id == id))
        {
            return Err(invalid("記録が参照する試行の根拠がありません"));
        }
        if matches!(s.kind, StatementKind::Discrepancy | StatementKind::Comment)
            && s.check_ids.is_empty()
            && s.evidence_id.is_none()
        {
            return Err(invalid("指摘・質問には対象の確認事項か試行結果が必要です"));
        }
        for c in &s.confirmations {
            nonempty(&c.actor, "確認者")?;
        }
        if let Some(r) = &s.resolution {
            nonempty(&r.actor, "対応者")?;
            nonempty(&r.text, "対応")?;
        }
    }
    for c in &workspace.checks {
        validate_check(c)?;
    }
    for (key, value) in &workspace.context {
        identifier(key)?;
        context_value(value)?;
    }
    for e in &workspace.evidence {
        if let Some(source) = &e.source {
            validate_source(source)?;
        }
        if !workspace.checks.iter().any(|c| c.id == e.check_id) || e.check_version == 0 {
            return Err(invalid("根拠が参照する確認事項がありません"));
        }
        validate_events(&e.manifest, &e.events)?;
        if let Some(trace) = &e.execution {
            crate::execution::validate_trace(trace, &e.manifest)?;
        }
        if let Some(adapter) = &e.adapter {
            crate::adapters::vitalsensing::validate(&e.manifest, adapter)?;
        }
        if let Some(trace) = &e.reported {
            crate::reported::validate(trace)?;
            if e.rule != Rule::ReportedAssertion
                || e.execution.is_some()
                || e.adapter.is_some()
                || trace.correlation != e.manifest.correlation
                || trace.source != e.source
                || e.manifest.mode != "reported_observation"
                || e.manifest.condition.id != e.check_id
                || e.manifest.condition.version != e.check_version
                || e.observation.is_none()
            {
                return Err(invalid("報告観測と根拠の条件・版・出所が一致しません"));
            }
        } else if e.rule == Rule::ReportedAssertion {
            return Err(invalid("報告観測の記録がありません"));
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
            source: None,
            evidence_id: None,
            recorded_by: String::new(),
            recorded_revision: 0,
            check_versions: BTreeMap::new(),
            confirmations: vec![],
            resolution: None,
        }],
        context: BTreeMap::new(),
        checks: vec![
            Check {
                id: "save".into(),
                source: None,
                observation: None,
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
                source: None,
                observation: None,
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
pub(crate) fn save_revision(directory: &Path, workspace: &Workspace) -> io::Result<()> {
    validate(workspace)?;
    if serde_json::to_vec_pretty(workspace)
        .map_err(io::Error::other)?
        .len() as u64
        + 1
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
    if directory.join("restore.pending").exists() {
        return Err(invalid(
            "復元が完了していません。復元元と新しい保存先を確認してください",
        ));
    }
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
    crate::adapters::vitalsensing::validate(manifest, &trace)?;
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
        Rule::ReportedAssertion => {
            if let (Some(trace), Some(criterion)) = (&evidence.reported, &evidence.observation) {
                return crate::reported::assess(trace, &evidence.manifest.condition, criterion);
            }
            result.reason = "報告観測と判定する観測名・対象がありません".into();
        }
        Rule::DemoSave => {
            result = assess(&evidence.manifest, &evidence.events);
            result.condition_id = evidence.check_id.clone();
            result.condition_version = evidence.check_version;
        }
        Rule::ObservedAssertion => {
            if let (Some(trace), Some(criterion)) = (&evidence.execution, &evidence.observation)
                && evidence.source.as_ref() == Some(&trace.recipe.source)
            {
                return crate::execution::assess_trace(&evidence.manifest, trace, criterion);
            }
            result.reason = "固定版の実行記録と、条件のコード参照・観測規則を照合できない".into();
        }
        Rule::ManualOnly => {
            result.reason =
                "この条件に対応する判定規則は未定義。プロセス終了や別条件の成立から判断しない"
                    .into();
        }
        Rule::LocalIndexUploaded => return crate::adapters::vitalsensing::assess(evidence, result),
    }
    result
}

fn external_suggestion(evidence: &Evidence) -> Option<SuggestedAction> {
    use crate::execution::ObservationRead;
    let trace = evidence.execution.as_ref()?;
    let criterion = evidence.observation.as_ref()?;
    // 試験案では0以外の終了コードも期待になりうる。判定条件ごとの結果から次の確認を決める。
    if trace.recipe.trial.is_some() {
        return None;
    }
    let (kind, reason, text) = if trace.timed_out
        || trace.output_limit
        || trace.exit_code != Some(0)
    {
        (
            ActionKind::InspectFailure,
            "対象の実行が完了していないため、観測を追加する前に起動・終了を確認する".into(),
            format!(
                "{}方式の起動ファイル {}、ランタイム、待機上限、標準エラーを確認する。方式を自動で変更しない",
                trace.mode, trace.recipe.entrypoint
            ),
        )
    } else if !trace.snapshot_unchanged {
        (
            ActionKind::InspectConflict,
            "試行用の固定版が開始時と終了時で異なり、どのコードの結果か確認できない".into(),
            "sourceの書き換え・追加ファイルと出所を調べ、固定版を維持して再試験する".into(),
        )
    } else if trace.observation_read == ObservationRead::Invalid {
        (
            ActionKind::InspectConflict,
            "観測の識別子・順序・参照先または形式を採用できない".into(),
            "観測の診断記録を読み、試行識別子・順序番号・JSONL・固定版のパスと行を照合する".into(),
        )
    } else {
        let matching: Vec<_> = trace
            .observations
            .iter()
            .filter(|o| o.assertion == criterion.assertion && o.subject == criterion.subject)
            .collect();
        if matching.is_empty() {
            (
                ActionKind::AddObservation,
                "試行は完了したが、選択した観測名・対象の根拠がない".into(),
                format!(
                    "観測名 {}・対象 {} の出力箇所と分岐を固定版で確認し、観測を追加して再試験する。記録の欠落を処理失敗と決めない",
                    criterion.assertion, criterion.subject
                ),
            )
        } else {
            return None;
        }
    };
    Some(SuggestedAction { kind, reason, text })
}

fn check_versions(workspace: &Workspace, ids: &[String]) -> BTreeMap<String, u32> {
    workspace
        .checks
        .iter()
        .filter(|c| ids.contains(&c.id))
        .map(|c| (c.id.clone(), c.version))
        .collect()
}
fn find_statement<'a>(workspace: &'a Workspace, id: &str) -> io::Result<&'a Statement> {
    workspace
        .statements
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| invalid("記録がありません"))
}

/// 確認の状態は、人が記入した確認だけから求める。解析や試行の結果から補わない。
fn statement_view(workspace: &Workspace, statement: &Statement) -> StatementView {
    let current = check_versions(workspace, &statement.check_ids);
    let current_roles: Vec<Role> = [Role::Operator, Role::Developer, Role::Other]
        .into_iter()
        .filter(|role| {
            statement
                .confirmations
                .iter()
                .any(|c| &c.role == role && c.check_versions == current)
        })
        .collect();
    let both = current_roles.contains(&Role::Operator) && current_roles.contains(&Role::Developer);
    let (status, status_label) = if both {
        (
            ConfirmationStatus::BothConfirmed,
            "運用担当と開発者の両方が、現在の条件の版で確認した".to_owned(),
        )
    } else if !current_roles.is_empty() {
        (
            ConfirmationStatus::PartlyConfirmed,
            "一方の担当だけが確認した。もう一方の確認は未記入".to_owned(),
        )
    } else if !statement.confirmations.is_empty() {
        (
            ConfirmationStatus::Outdated,
            "確認した後に条件が変わった。現在の版では未確認".to_owned(),
        )
    } else {
        (
            ConfirmationStatus::Unconfirmed,
            "まだ誰も確認していない".to_owned(),
        )
    };
    StatementView {
        id: statement.id.clone(),
        status,
        status_label,
        current_roles,
        open: matches!(
            statement.kind,
            StatementKind::Discrepancy | StatementKind::Comment | StatementKind::Unanswered
        ) && statement.resolution.is_none(),
    }
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
                    if e.observation != check.observation {
                        invalidated_by.push("判定する観測名・対象が変わった".into());
                    }
                    if e.source != check.source {
                        invalidated_by.push("確認事項が参照するコードの版・場所が変わった".into());
                    }
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
                        summary: e
                            .execution
                            .as_ref()
                            .and_then(|t| crate::trial::summarize(t, &e.manifest.command)),
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
            } else if let Some(next) = workspace
                .evidence
                .iter()
                .rev()
                .find(|e| e.check_id == check.id)
                .and_then(external_suggestion)
            {
                next
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
    let statements = workspace
        .statements
        .iter()
        .map(|s| statement_view(&workspace, s))
        .collect();
    WorkspaceView {
        workspace,
        checks,
        statements,
    }
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
        Change::ConfigureReportedObservation {
            check_id,
            criterion,
        } => {
            crate::execution::validate_criterion(&criterion)?;
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &mut workspace.checks[i];
            if check.rule == Rule::ReportedAssertion
                && check.observation.as_ref() == Some(&criterion)
            {
                return Err(invalid("同じ報告観測規則が既に記録されています"));
            }
            check.version = check
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("条件の版の上限です"))?;
            check.rule = Rule::ReportedAssertion;
            check.observation = Some(criterion);
            let key = format!("report-{}", check.id);
            if !check.dependencies.contains(&key) {
                check.dependencies.push(key);
            }
            "configure_reported_observation"
        }
        Change::ImportReported { condition, trace } => {
            crate::reported::validate(&trace)?;
            let i = find_check(&workspace.checks, &condition.id)?;
            let check = &workspace.checks[i];
            if check.rule != Rule::ReportedAssertion
                || check.version != condition.version
                || check.text != condition.text
                || check.origin != condition.origin
                || condition.kind != ConditionKind::Custom
                || check.source != trace.source
            {
                return Err(invalid(
                    "報告を取り込む条件・版・出所・参照コードが現在と一致しません",
                ));
            }
            if workspace.evidence.iter().any(|e| {
                e.check_id == check.id && e.manifest.correlation.run_id == trace.correlation.run_id
            }) {
                return Err(invalid("同じ試行がこの条件へ既に取り込まれています"));
            }
            let key = format!("report-{}", check.id);
            let value = serde_json::to_string(&serde_json::json!({"producer": trace.producer, "origin": trace.origin, "source": trace.source, "context": trace.context})).map_err(io::Error::other)?;
            context_value(&value)?;
            let mut dependencies: BTreeMap<String, String> = check
                .dependencies
                .iter()
                .filter_map(|k| workspace.context.get(k).map(|v| (k.clone(), v.clone())))
                .collect();
            dependencies.insert(key.clone(), value.clone());
            let dependency_origins = dependencies
                .keys()
                .map(|k| {
                    (
                        k.clone(),
                        if k == &key {
                            "外部からの報告。基盤が実行・照合した値ではない".into()
                        } else {
                            "記入者が事例に宣言した値".into()
                        },
                    )
                })
                .collect();
            let mut manifest =
                crate::experiment::make_manifest(trace.correlation.clone(), condition)?;
            manifest.mode = "reported_observation".into();
            manifest.initial_storage =
                "外部からの報告。基盤での初期化・隔離は実施していない".into();
            manifest.observation_scope =
                "報告された観測名・対象・結果のみ。実行経路・実環境の状態は未確認".into();
            manifest.previous_run = workspace
                .evidence
                .iter()
                .rfind(|e| e.check_id == check.id)
                .map(|e| e.manifest.correlation.run_id.clone());
            workspace.evidence.push(Evidence {
                id: format!("evidence-{}", workspace.evidence.len() + 1),
                check_id: check.id.clone(),
                check_version: check.version,
                rule: check.rule.clone(),
                source: check.source.clone(),
                observation: check.observation.clone(),
                execution: None,
                reported: Some(trace),
                dependencies,
                dependency_origins,
                manifest,
                events: vec![],
                adapter: None,
            });
            workspace.context.insert(key, value);
            "import_reported"
        }
        Change::AddStatement { mut statement } => {
            if !statement.confirmations.is_empty() || statement.resolution.is_some() {
                return Err(invalid(
                    "確認・対応は記録の追加後に、それぞれの人が記入します",
                ));
            }
            let next = workspace.revision + 1;
            statement.recorded_by = request.actor.clone();
            statement.recorded_revision = next;
            statement.check_versions = check_versions(&workspace, &statement.check_ids);
            workspace.statements.push(statement);
            "add_statement"
        }
        Change::ConfirmStatement {
            statement_id,
            role,
            note,
        } => {
            let versions = {
                let s = find_statement(&workspace, &statement_id)?;
                check_versions(&workspace, &s.check_ids)
            };
            let next = workspace.revision + 1;
            let actor = request.actor.clone();
            let i = workspace
                .statements
                .iter()
                .position(|s| s.id == statement_id)
                .ok_or_else(|| invalid("記録がありません"))?;
            let statement = &mut workspace.statements[i];
            if statement
                .confirmations
                .iter()
                .any(|c| c.actor == actor && c.role == role && c.check_versions == versions)
            {
                return Err(invalid("同じ人が同じ版で既に確認しています"));
            }
            statement.confirmations.push(Confirmation {
                actor,
                role,
                revision: next,
                check_versions: versions,
                note,
            });
            "confirm_statement"
        }
        Change::ResolveStatement { statement_id, text } => {
            nonempty(&text, "対応")?;
            find_statement(&workspace, &statement_id)?;
            let next = workspace.revision + 1;
            let statement = workspace
                .statements
                .iter_mut()
                .find(|s| s.id == statement_id)
                .ok_or_else(|| invalid("記録がありません"))?;
            if !matches!(
                statement.kind,
                StatementKind::Discrepancy | StatementKind::Comment | StatementKind::Unanswered
            ) {
                return Err(invalid("対応を残せるのは指摘・質問・未回答の記録です"));
            }
            statement.resolution = Some(Resolution {
                actor: request.actor.clone(),
                revision: next,
                text,
            });
            "resolve_statement"
        }
        Change::AddCheck { check } => {
            if check.version != 1 {
                return Err(invalid("新しい条件の版は1です"));
            }
            workspace.checks.push(check);
            "add_check"
        }
        Change::ConfigureObservation {
            check_id,
            criterion,
        } => {
            crate::execution::validate_criterion(&criterion)?;
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &mut workspace.checks[i];
            if check.observation.as_ref() == Some(&criterion)
                && check.rule == Rule::ObservedAssertion
            {
                return Err(invalid("同じ観測規則が既に記録されています"));
            }
            check.version = check
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("条件の版の上限です"))?;
            check.rule = Rule::ObservedAssertion;
            check.observation = Some(criterion);
            let key = format!("execution-{}", check.id);
            if !check.dependencies.contains(&key) {
                check.dependencies.push(key);
            }
            "configure_observation"
        }
        Change::BindSource { check_id, source } => {
            validate_source(&source)?;
            let i = find_check(&workspace.checks, &check_id)?;
            let check = &mut workspace.checks[i];
            if check.source.as_ref() == Some(&source) {
                return Err(invalid("同じコード参照が既に記録されています"));
            }
            check.version = check
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("条件の版の上限です"))?;
            check.source = Some(source);
            "bind_source"
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
            context_value(&value)?;
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
            let execution: Option<ExecutionTrace> = if manifest.mode == "pinned_execution" {
                let trace: ExecutionTrace = document(&directory.join("execution.json"))?;
                crate::execution::validate_trace(&trace, &manifest)?;
                if manifest.condition.id != check.id
                    || manifest.condition.version != check.version
                    || manifest.condition.text != check.text
                    || manifest.condition.origin != check.origin
                    || trace.recipe.check_id != check.id
                    || check.source.as_ref() != Some(&trace.recipe.source)
                    || check.observation.as_ref() != Some(&trace.recipe.criterion)
                {
                    return Err(invalid(
                        "試行の確認事項・コード参照・観測規則が現在の条件と一致しません",
                    ));
                }
                Some(trace)
            } else {
                None
            };
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
            let mut values = dependency_values(&manifest, adapter.as_ref())?;
            if let Some(trace) = &execution {
                values = BTreeMap::from([(format!("execution-{}", check.id), serde_json::to_string(&serde_json::json!({
                    "recipe": trace.recipe, "engine": manifest.source_fingerprint,
                    "runtime_path": trace.runtime_path, "runtime_version": trace.runtime_version,
                    "os": manifest.os, "architecture": manifest.architecture, "mode": trace.mode
                })).map_err(io::Error::other)?)]);
            }
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
                source: check.source.clone(),
                observation: check.observation.clone(),
                execution,
                reported: None,
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

/// 明示的に指定したリポジトリを実行する。
pub fn run_external_mode(
    directory: &Path,
    repository_root: &Path,
    recipe: crate::execution::Recipe,
    mode: crate::execution::ExecutionMode,
    actor: String,
    reason: String,
) -> io::Result<WorkspaceView> {
    run_external_at_revision(
        directory,
        repository_root,
        recipe,
        mode,
        actor,
        reason,
        None,
    )
}

/// 起動時と取り込み時の版を照合し、実行中の変更を上書きしない。
pub fn run_external_at_revision(
    directory: &Path,
    repository_root: &Path,
    recipe: crate::execution::Recipe,
    mode: crate::execution::ExecutionMode,
    actor: String,
    reason: String,
    expected_revision: Option<u64>,
) -> io::Result<WorkspaceView> {
    run_external_with_dependencies(
        directory,
        repository_root,
        None,
        recipe,
        mode,
        actor,
        reason,
        expected_revision,
    )
}

/// 依存先を `dependency_base/owner/repo` に取得済みの固定コミットから読み込んで実行し、取り込む。
#[allow(clippy::too_many_arguments)]
pub fn run_external_with_dependencies(
    directory: &Path,
    repository_root: &Path,
    dependency_base: Option<&Path>,
    recipe: crate::execution::Recipe,
    mode: crate::execution::ExecutionMode,
    actor: String,
    reason: String,
    expected_revision: Option<u64>,
) -> io::Result<WorkspaceView> {
    nonempty(&actor, "記入者")?;
    nonempty(&reason, "理由")?;
    let workspace = load(directory)?;
    if expected_revision.is_some_and(|revision| revision != workspace.revision) {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "事例が更新されています。最新版を読み直してください",
        ));
    }
    let check = workspace
        .checks
        .iter()
        .find(|c| c.id == recipe.check_id)
        .ok_or_else(|| invalid("試験設定の確認事項がありません"))?;
    if check.source.as_ref() != Some(&recipe.source)
        || check.observation.as_ref() != Some(&recipe.criterion)
        || check.rule != Rule::ObservedAssertion
    {
        return Err(invalid(
            "コード参照と観測規則を画面かcase applyで設定してください",
        ));
    }
    let previous = workspace
        .evidence
        .iter()
        .rfind(|e| e.check_id == check.id)
        .map(|e| e.manifest.correlation.run_id.clone());
    let condition = Condition {
        id: check.id.clone(),
        version: check.version,
        text: check.text.clone(),
        origin: check.origin.clone(),
        kind: ConditionKind::Custom,
    };
    let result = crate::execution::execute_with_dependencies(
        repository_root,
        dependency_base,
        &directory.join("runs"),
        recipe,
        condition,
        previous,
        mode,
    )?;
    eprintln!("外部コードの試行: {}", result.directory.display());
    // 実行中に条件が変わった場合は取り込まず、試行をそのまま残す。
    change(
        directory,
        ChangeRequest {
            expected_revision: workspace.revision,
            actor,
            reason,
            action: Change::Import {
                check_id: check.id.clone(),
                run_directory: result.directory.to_string_lossy().into_owned(),
                use_run_context: true,
            },
        },
    )
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}。試行記録: {}", result.directory.display()),
        )
    })
}

pub fn run_external(
    directory: &Path,
    repository_root: &Path,
    recipe: crate::execution::Recipe,
    actor: String,
    reason: String,
) -> io::Result<WorkspaceView> {
    run_external_mode(
        directory,
        repository_root,
        recipe,
        crate::execution::ExecutionMode::Local,
        actor,
        reason,
    )
}
