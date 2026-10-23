use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correlation {
    pub run_id: String,
    pub record_id: String,
    pub attempt_id: String,
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
    /// 既存の保存内容を保持するための情報。現在の故障条件はRecipeに記録する。
    pub fault: Option<serde_json::Value>,
    /// 既存の保存内容を保持するための情報。現在の入力はRecipeに記録する。
    pub input: Option<serde_json::Value>,
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
#[derive(Debug)]
pub struct RunResult {
    pub directory: PathBuf,
    pub manifest: Manifest,
    pub assessment: Assessment,
}
