//! 起動時に指定した一つのリポジトリを、localhostの画面から試す。
use crate::{
    execution::{self, ExecutionMode, Recipe},
    recording::unix_ms,
    workspace::{self, Rule},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteRequest {
    pub expected_revision: u64,
    pub actor: String,
    pub reason: String,
    pub recipe: Recipe,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub check_id: String,
    pub status: String,
    pub started_unix_ms: u64,
    pub finished_unix_ms: Option<u64>,
    pub resulting_revision: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecutionStatus {
    pub enabled: bool,
    pub repository: Option<String>,
    pub repository_root: Option<PathBuf>,
    pub mode: Option<String>,
    pub job: Option<Job>,
}
impl ExecutionStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            repository: None,
            repository_root: None,
            mode: None,
            job: None,
        }
    }
}
#[derive(Default)]
struct State {
    sequence: u64,
    job: Option<Job>,
}
#[derive(Clone)]
pub struct Controller {
    directory: PathBuf,
    repository_root: PathBuf,
    repository: String,
    mode: ExecutionMode,
    state: Arc<Mutex<State>>,
}
impl Controller {
    pub fn new(
        directory: PathBuf,
        repository_root: PathBuf,
        mode: ExecutionMode,
    ) -> io::Result<Self> {
        workspace::load(&directory)?;
        let repository_root = std::fs::canonicalize(repository_root)?;
        let repository = execution::repository_name(&repository_root)?;
        Ok(Self {
            directory: std::fs::canonicalize(directory)?,
            repository_root,
            repository,
            mode,
            state: Arc::new(Mutex::new(State::default())),
        })
    }
    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("実行状態を読み取れません"))
    }
    pub fn status(&self) -> io::Result<ExecutionStatus> {
        Ok(ExecutionStatus {
            enabled: true,
            repository: Some(self.repository.clone()),
            repository_root: Some(self.repository_root.clone()),
            mode: Some(self.mode.label().into()),
            job: self.lock()?.job.clone(),
        })
    }
    pub fn start(&self, request: ExecuteRequest) -> io::Result<Job> {
        execution::validate_recipe(&request.recipe)?;
        for value in [&request.actor, &request.reason] {
            if value.trim().is_empty() || value.len() > 16384 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "記入者と理由は空でない16384バイト以内の値が必要です",
                ));
            }
        }
        if request.recipe.source.repository != self.repository {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "起動時に指定したリポジトリだけを実行できます",
            ));
        }
        let workspace = workspace::load(&self.directory)?;
        if workspace.revision != request.expected_revision {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "事例が更新されています。最新版を読み直してください",
            ));
        }
        if !workspace.checks.iter().any(|check| {
            check.id == request.recipe.check_id
                && check.rule == Rule::ObservedAssertion
                && check.source.as_ref() == Some(&request.recipe.source)
                && check.observation.as_ref() == Some(&request.recipe.criterion)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "現在のコード参照と観測規則に対応する試験設定が必要です",
            ));
        }
        let job = {
            let mut state = self.lock()?;
            if state
                .job
                .as_ref()
                .is_some_and(|job| job.status == "running")
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "別の試験を実行中です。終了してから再試験してください",
                ));
            }
            state.sequence = state
                .sequence
                .checked_add(1)
                .ok_or_else(|| io::Error::other("試験番号が上限に達しました"))?;
            let job = Job {
                id: format!("job-{}", state.sequence),
                check_id: request.recipe.check_id.clone(),
                status: "running".into(),
                started_unix_ms: unix_ms()?,
                finished_unix_ms: None,
                resulting_revision: None,
                error: None,
            };
            state.job = Some(job.clone());
            job
        };
        let controller = self.clone();
        let spawn = thread::Builder::new()
            .name("review-execution".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    workspace::run_external_at_revision(
                        &controller.directory,
                        &controller.repository_root,
                        request.recipe,
                        controller.mode,
                        request.actor,
                        request.reason,
                        Some(request.expected_revision),
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(io::Error::other(
                        "試験処理が中断しました。試行の記録と最新版を確認してください",
                    ))
                });
                if let Ok(mut state) = controller.lock()
                    && let Some(job) = state.job.as_mut()
                {
                    job.finished_unix_ms = unix_ms().ok();
                    match result {
                        Ok(view) => {
                            job.status = "completed".into();
                            job.resulting_revision = Some(view.workspace.revision);
                        }
                        Err(error) => {
                            job.status = "failed".into();
                            job.error = Some(error.to_string());
                        }
                    }
                }
            });
        if let Err(error) = spawn {
            if let Some(job) = self.lock()?.job.as_mut() {
                job.status = "failed".into();
                job.error = Some(error.to_string());
                job.finished_unix_ms = unix_ms().ok();
            }
            return Err(error);
        }
        Ok(job)
    }
}
