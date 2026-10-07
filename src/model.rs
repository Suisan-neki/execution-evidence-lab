use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data {
    pub name: String,
    pub age: u16,
}
pub fn is_valid(data: &Data) -> bool {
    !data.name.is_empty() && data.age <= 150
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    Normal,
    DisconnectBeforeReceive,
    FailBeforeSave,
    OmitSaveObservation,
}
impl Fault {
    pub const ALL: [Self; 4] = [
        Self::Normal,
        Self::DisconnectBeforeReceive,
        Self::FailBeforeSave,
        Self::OmitSaveObservation,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::DisconnectBeforeReceive => "disconnect",
            Self::FailBeforeSave => "save-failure",
            Self::OmitSaveObservation => "missing-observation",
        }
    }
}
impl std::str::FromStr for Fault {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|fault| fault.label() == value)
            .ok_or_else(|| format!("不明な条件: {value}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correlation {
    pub run_id: String,
    pub record_id: String,
    pub attempt_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub correlation: Correlation,
    pub data: Data,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyStatus {
    Saved,
    SaveFailed,
    InvalidInput,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub correlation: Correlation,
    pub status: ReplyStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Client,
    Server,
    FaultGate,
    Process,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    OperationStarted,
    RequestSent,
    RequestReceived,
    RequestBlocked,
    InputRejected,
    SaveCommitted,
    SaveFailed,
    ReplyReceived,
    DisplaySaved,
    DisplayFailed,
    TransportFailed,
    ProcessStartFailed,
    ProcessStarted,
    ProcessExited,
    ProcessTimedOut,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub elapsed_us: u64,
    pub correlation: Correlation,
    pub source: Source,
    pub kind: EventKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    SaveCompleted,
    Custom,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub id: String,
    pub version: u32,
    pub text: String,
    pub origin: String,
    pub kind: ConditionKind,
}
impl Condition {
    pub fn demo() -> Self {
        Self {
            id: "demo-save".into(),
            version: 1,
            text: "この試行のデータについて、サーバーのファイル書き込みと同期処理が成功した".into(),
            origin: "実験用にCodexが選んだ条件。医療機関の採用済み要件ではない".into(),
            kind: ConditionKind::SaveCompleted,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub correlation: Correlation,
    pub previous_run: Option<String>,
    pub source_revision: String,
    pub source_fingerprint: String,
    pub program_version: String,
    pub os: String,
    pub architecture: String,
    pub started_unix_ms: u64,
    pub mode: String,
    pub fault: Option<Fault>,
    pub input: Option<Data>,
    pub condition: Condition,
    pub timeout_ms: u64,
    pub initial_storage: String,
    pub observation_scope: String,
    pub command: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Satisfied,
    NotSatisfied,
    Unknown,
}
impl Verdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Satisfied => "条件を満たす",
            Self::NotSatisfied => "条件を満たさない",
            Self::Unknown => "判定できない",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub condition_id: String,
    pub condition_version: u32,
    pub verdict: Verdict,
    pub evidence: Vec<String>,
    pub reason: String,
    pub unconfirmed: Vec<String>,
    pub next_action: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Truth {
    Saved { data: Data, matches_input: bool },
    NotSaved,
    Unreadable { reason: String },
    NotInspected,
}
#[derive(Debug)]
pub struct RunResult {
    pub directory: PathBuf,
    pub manifest: Manifest,
    pub assessment: Assessment,
    pub truth: Truth,
}
pub struct TrialConfig {
    pub output: PathBuf,
    pub data: Data,
    pub fault: Fault,
    pub previous_run: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Comparison {
    pub before_run: String,
    pub after_run: String,
    pub same_question: bool,
    pub prior_evidence_matches_current_configuration: bool,
    pub changed: Vec<String>,
    pub before: Verdict,
    pub after: Verdict,
}
