//! 固定したGitのコードをコピーし、明示されたローカル試験を実行する。
//! 計測対象は構造化された自己申告の観測。OSの隔離や実行経路の証明ではない。
use crate::{
    experiment::{finish_run, make_manifest},
    model::*,
    recording::*,
    workspace::CodeReference,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_FILES: usize = 512;
const MAX_FILE: u64 = 4 * 1024 * 1024;
const MAX_TOTAL: u64 = 32 * 1024 * 1024;
const MAX_TRACE: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Local,
    Bubblewrap,
}
impl ExecutionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Bubblewrap => "bubblewrap",
        }
    }
}
impl std::str::FromStr for ExecutionMode {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "local" => Ok(Self::Local),
            "bubblewrap" => Ok(Self::Bubblewrap),
            _ => Err(invalid("実行方式はlocalかbubblewrapです")),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub assertion: String,
    pub subject: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Node,
    Python3,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema_version: u32,
    pub check_id: String,
    pub source: CodeReference,
    pub runtime: Runtime,
    pub entrypoint: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub input: serde_json::Value,
    pub criterion: Criterion,
    pub timeout_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOutcome {
    Pass,
    Fail,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub schema_version: u32,
    pub sequence: u64,
    pub correlation: Correlation,
    pub assertion: String,
    pub subject: String,
    pub outcome: ObservationOutcome,
    pub detail: String,
    pub path: String,
    pub line: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFile {
    pub path: String,
    pub blob: String,
    pub mode: String,
    pub bytes: u64,
    pub lines: u32,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationRead {
    Valid,
    Missing,
    Invalid,
    #[default]
    Unspecified,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub schema_version: u32,
    pub correlation: Correlation,
    pub recipe: Recipe,
    pub tree: String,
    pub files: Vec<SnapshotFile>,
    pub runtime_path: String,
    pub runtime_version: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output_limit: bool,
    pub snapshot_unchanged: bool,
    pub observations: Vec<Observation>,
    #[serde(default)]
    pub observation_read: ObservationRead,
    pub diagnostics: Vec<String>,
    pub mode: String,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn invalid(text: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, text.into())
}
fn relative(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 1024
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || !Path::new(value)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p == ".git")
    {
        return Err(invalid("対象内の相対パスが必要です"));
    }
    Ok(())
}
fn text(value: &str) -> io::Result<()> {
    if value.trim().is_empty() || value.len() > 16384 {
        return Err(invalid("空でない16384バイト以内の値が必要です"));
    }
    Ok(())
}
pub fn validate_criterion(criterion: &Criterion) -> io::Result<()> {
    text(&criterion.assertion)?;
    text(&criterion.subject)
}
pub fn validate_recipe(recipe: &Recipe) -> io::Result<()> {
    crate::workspace::validate_source(&recipe.source)?;
    relative(&recipe.entrypoint)?;
    relative(&recipe.source.path)?;
    validate_criterion(&recipe.criterion)?;
    text(&recipe.check_id)?;
    if recipe.schema_version != 1
        || !(1..=300000).contains(&recipe.timeout_ms)
        || recipe.args.len() > 64
        || recipe
            .args
            .iter()
            .any(|a| a.len() > 16384 || a.contains('\0'))
        || serde_json::to_vec(recipe).map_err(io::Error::other)?.len() > 192 * 1024
        || serde_json::to_vec(&recipe.input)
            .map_err(io::Error::other)?
            .len()
            > 128 * 1024
    {
        return Err(invalid(
            "試験設定の形式・入力サイズ・待機上限を確認してください",
        ));
    }
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .args([
            "--no-replace-objects",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(root)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    // 取得に必要なネットワーク設定だけをGitへ渡す。対象アプリへは渡さない。
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "GIT_SSL_CAINFO",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(invalid(format!(
            "Gitの固定版を読み取れません: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}
fn git_text(root: &Path, args: &[&str]) -> io::Result<String> {
    Ok(String::from_utf8(git(root, args)?)
        .map_err(io::Error::other)?
        .trim()
        .into())
}
fn expected_origin(repository: &str, origin: &str) -> bool {
    origin == format!("https://github.com/{repository}")
        || origin == format!("https://github.com/{repository}.git")
        || origin == format!("git@github.com:{repository}.git")
}
fn inventory(root: &Path, recipe: &Recipe) -> io::Result<(String, Vec<SnapshotFile>)> {
    let commit = git_text(
        root,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", recipe.source.commit),
        ],
    )?;
    if commit != recipe.source.commit
        || !expected_origin(
            &recipe.source.repository,
            &git_text(root, &["remote", "get-url", "origin"])?,
        )
    {
        return Err(invalid(
            "ローカルのコミットまたはoriginがコード参照と一致しません",
        ));
    }
    let tree = git_text(
        root,
        &["rev-parse", &format!("{}^{{tree}}", recipe.source.commit)],
    )?;
    let entries = git(
        root,
        &["ls-tree", "-rz", "--full-tree", &recipe.source.commit],
    )?;
    let mut files = vec![];
    let mut total = 0;
    for entry in entries.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let entry = std::str::from_utf8(entry).map_err(io::Error::other)?;
        let (header, path) = entry
            .split_once('\t')
            .ok_or_else(|| invalid("Gitのファイル情報が不正です"))?;
        relative(path)?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[1] != "blob" || !["100644", "100755"].contains(&fields[0]) {
            return Err(invalid(
                "symlink・submodule・特殊ファイルはこの実行方式の対象外です",
            ));
        }
        let bytes = git_text(root, &["cat-file", "-s", fields[2]])?
            .parse::<u64>()
            .map_err(io::Error::other)?;
        total += bytes;
        if bytes > MAX_FILE || total > MAX_TOTAL || files.len() >= MAX_FILES {
            return Err(invalid("固定版は512ファイル・各4MiB・合計32MiB以内です"));
        }
        let blob = git(root, &["cat-file", "blob", fields[2]])?;
        let lines = std::str::from_utf8(&blob)
            .map(|s| s.lines().count() as u32)
            .unwrap_or(0);
        files.push(SnapshotFile {
            path: path.into(),
            blob: fields[2].into(),
            mode: fields[0].into(),
            bytes,
            lines,
        });
    }
    if !files.iter().any(|f| f.path == recipe.source.path)
        || !files.iter().any(|f| f.path == recipe.entrypoint)
    {
        return Err(invalid(
            "参照ファイルまたは起動ファイルが固定コミットにありません",
        ));
    }
    Ok((tree, files))
}
fn copy_snapshot(root: &Path, destination: &Path, files: &[SnapshotFile]) -> io::Result<()> {
    fs::create_dir(destination)?;
    for file in files {
        let bytes = git(root, &["cat-file", "blob", &file.blob])?;
        if bytes.len() as u64 != file.bytes {
            return Err(invalid("Git blobの長さが一致しません"));
        }
        let path = destination.join(&file.path);
        fs::create_dir_all(path.parent().ok_or_else(|| invalid("出力先がありません"))?)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(if file.mode == "100755" { 0o555 } else { 0o444 }),
            )?;
        }
    }
    Ok(())
}
fn snapshot_unchanged(root: &Path, snapshot: &Path, files: &[SnapshotFile]) -> bool {
    fn paths(directory: &Path, base: &Path, found: &mut BTreeSet<String>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.is_dir() {
                paths(&entry.path(), base, found)?;
            } else if metadata.is_file() {
                found.insert(
                    entry
                        .path()
                        .strip_prefix(base)
                        .map_err(io::Error::other)?
                        .to_string_lossy()
                        .into_owned(),
                );
                if found.len() > MAX_FILES {
                    return Err(invalid("試行中にファイルが増えました"));
                }
            } else {
                return Err(invalid("固定版に特殊ファイルが追加されました"));
            }
        }
        Ok(())
    }
    let mut found = BTreeSet::new();
    if paths(snapshot, snapshot, &mut found).is_err()
        || found != files.iter().map(|f| f.path.clone()).collect()
    {
        return false;
    }
    files.iter().all(|file| {
        let path = snapshot.join(&file.path);
        fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
            && git(root, &["cat-file", "blob", &file.blob])
                .ok()
                .zip(fs::read(&path).ok())
                .is_some_and(|(a, b)| a == b)
    })
}

fn runtime_path(runtime: &Runtime) -> io::Result<PathBuf> {
    let name = match runtime {
        Runtime::Node => "node",
        Runtime::Python3 => "python3",
    };
    executable(name)
}
fn executable(name: &str) -> io::Result<PathBuf> {
    let paths = std::env::var_os("PATH").ok_or_else(|| invalid("PATHを確認してください"))?;
    for directory in std::env::split_paths(&paths) {
        let path = directory.join(name);
        if path.is_file() {
            return fs::canonicalize(path);
        }
    }
    Err(invalid(format!("{name}を実行できません")))
}
fn load_observations(
    path: &Path,
    correlation: &Correlation,
    files: &[SnapshotFile],
) -> (Vec<Observation>, Vec<String>, ObservationRead) {
    let read = (|| -> io::Result<Vec<Observation>> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_TRACE {
            return Err(invalid("観測ファイルは通常ファイル・1MiB以内です"));
        }
        let mut input = String::new();
        File::open(path)?
            .take(MAX_TRACE + 1)
            .read_to_string(&mut input)?;
        if input.len() as u64 > MAX_TRACE {
            return Err(invalid("観測ファイルが1MiBを超えています"));
        }
        let mut observations = vec![];
        for (i, line) in input.lines().enumerate() {
            let o: Observation = serde_json::from_str(line).map_err(io::Error::other)?;
            if o.schema_version != 1
                || o.sequence != i as u64 + 1
                || &o.correlation != correlation
                || !files.iter().any(|f| f.path == o.path)
                || o.line == 0
                || files
                    .iter()
                    .find(|f| f.path == o.path)
                    .is_none_or(|f| o.line > f.lines)
                || observations.len() >= 2000
            {
                return Err(invalid("観測の順序・試行識別子・コード参照が不正です"));
            }
            text(&o.assertion)?;
            text(&o.subject)?;
            text(&o.detail)?;
            relative(&o.path)?;
            observations.push(o);
        }
        Ok(observations)
    })();
    match read {
        Ok(o) => (o, vec![], ObservationRead::Valid),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (
            vec![],
            vec!["観測ファイルを取得できませんでした。条件の失敗とは断定しません".into()],
            ObservationRead::Missing,
        ),
        Err(e) => (
            vec![],
            vec![format!("観測を採用できません: {e}")],
            ObservationRead::Invalid,
        ),
    }
}

pub fn validate_trace(trace: &ExecutionTrace, manifest: &Manifest) -> io::Result<()> {
    validate_recipe(&trace.recipe)?;
    let hash = |value: &str| {
        value.len() == 40
            && value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    };
    if trace.schema_version != 1
        || trace.correlation != manifest.correlation
        || !["local", "bubblewrap"].contains(&trace.mode.as_str())
        || manifest.mode != "pinned_execution"
        || manifest.condition.id != trace.recipe.check_id
        || manifest.timeout_ms != trace.recipe.timeout_ms
        || !hash(&trace.tree)
        || trace.files.len() > MAX_FILES
        || !trace
            .files
            .iter()
            .any(|f| f.path == trace.recipe.source.path)
        || !trace
            .files
            .iter()
            .any(|f| f.path == trace.recipe.entrypoint)
        || (trace.observation_read == ObservationRead::Missing
            || trace.observation_read == ObservationRead::Invalid)
            && !trace.observations.is_empty()
        || trace.observations.len() > 2000
        || trace.diagnostics.len() > 100
        || trace.diagnostics.iter().any(|d| d.len() > 16384)
    {
        return Err(invalid("実行対象・観測・試行の対応が不正です"));
    }
    let mut seen = BTreeSet::new();
    let mut total = 0;
    for file in &trace.files {
        relative(&file.path)?;
        total += file.bytes;
        if !seen.insert(&file.path)
            || file.bytes > MAX_FILE
            || total > MAX_TOTAL
            || !hash(&file.blob)
            || !["100644", "100755"].contains(&file.mode.as_str())
        {
            return Err(invalid("固定版のファイル情報が不正です"));
        }
    }
    for (i, o) in trace.observations.iter().enumerate() {
        if o.schema_version != 1
            || o.sequence != i as u64 + 1
            || o.correlation != trace.correlation
            || o.line == 0
            || trace
                .files
                .iter()
                .find(|f| f.path == o.path)
                .is_none_or(|f| o.line > f.lines)
        {
            return Err(invalid("観測の参照先・順序・識別子が一致しません"));
        }
        text(&o.assertion)?;
        text(&o.subject)?;
        text(&o.detail)?;
    }
    Ok(())
}

pub fn assess_trace(
    manifest: &Manifest,
    trace: &ExecutionTrace,
    criterion: &Criterion,
) -> Assessment {
    let mut result = Assessment {
        condition_id: manifest.condition.id.clone(), condition_version: manifest.condition.version,
        verdict: Verdict::Unknown, evidence: vec![], reason: String::new(),
        unconfirmed: vec!["対象プログラムが出力した観測です。独立した読み出し、参照した行への到達、外部依存先・医療業務の利用可否は確認していない".into()],
        next_action: "不足・食い違いの出所を確認し、新しい試行で観測する".into(),
    };
    result.unconfirmed.extend(trace.diagnostics.clone());
    if validate_trace(trace, manifest).is_err() {
        result.reason = "試行・観測・固定版の対応を照合できない".into();
        return result;
    }
    if !trace.snapshot_unchanged {
        result.reason = "試行用の固定版が開始時と終了時で異なり、結果の適用先を確認できない".into();
        return result;
    }
    if trace.timed_out || trace.output_limit || trace.exit_code != Some(0) {
        result.reason =
            "対象の起動・実行・終了を確認できない。選択した方式と標準エラーを確認する".into();
        return result;
    }
    if trace.observation_read == ObservationRead::Invalid {
        result.reason = "試行は終了したが、観測の識別子・順序・参照先・形式を採用できない".into();
        return result;
    }
    if trace.observation_read == ObservationRead::Missing {
        result.reason =
            "試行は正常に終了したが、観測ファイルがない。条件の失敗とは断定しない".into();
        return result;
    }
    if !trace.diagnostics.is_empty() {
        result.reason = "観測の採用条件を確認できない".into();
        return result;
    }
    let matching: Vec<_> = trace
        .observations
        .iter()
        .filter(|o| o.assertion == criterion.assertion && o.subject == criterion.subject)
        .collect();
    let pass = matching
        .iter()
        .any(|o| o.outcome == ObservationOutcome::Pass);
    let fail = matching
        .iter()
        .any(|o| o.outcome == ObservationOutcome::Fail);
    result.evidence = matching
        .iter()
        .map(|o| format!("observations.jsonl#{}", o.sequence))
        .collect();
    if pass && fail {
        result.reason = "同じ試行・観測名・対象の成功と失敗が食い違う".into();
    } else if pass {
        result.verdict = Verdict::Satisfied;
        result.reason = "選択した観測名・対象について、固定版の実行から成功の観測がある".into();
        result.next_action = "観測が条件の意味を十分に確かめるか、別経路の状態と照合する".into();
    } else if fail {
        result.verdict = Verdict::NotSatisfied;
        result.reason =
            "選択した観測名・対象について、固定版の実行から明示的な失敗の観測がある".into();
    } else {
        result.reason = "選択した観測名・対象に対応する記録がない。条件の失敗とは断定しない".into();
    }
    result
}

pub fn execute_with_mode(
    repository_root: &Path,
    output: &Path,
    recipe: Recipe,
    condition: Condition,
    previous_run: Option<String>,
    mode: ExecutionMode,
) -> io::Result<RunResult> {
    validate_recipe(&recipe)?;
    let root = fs::canonicalize(repository_root)?;
    let (tree, files) = inventory(&root, &recipe)?;
    let program = runtime_path(&recipe.runtime)?;
    let runtime_version = String::from_utf8_lossy(
        &Command::new(&program)
            .arg("--version")
            .env_clear()
            .output()?
            .stdout,
    )
    .trim()
    .to_owned();
    let (directory, correlation) = new_run_directory(output)?;
    let directory = fs::canonicalize(directory)?;
    let snapshot = directory.join("source");
    let work = directory.join("work");
    copy_snapshot(&root, &snapshot, &files)?;
    fs::create_dir(&work)?;
    let source_text = fs::read_to_string(snapshot.join(&recipe.source.path))?;
    if recipe.source.end_line as usize > source_text.lines().count() {
        return Err(invalid("コード参照の行範囲が固定版を超えています"));
    }
    write_json_new(&work.join("input.json"), &recipe.input)?;
    let mut manifest = make_manifest(correlation.clone(), condition)?;
    manifest.mode = "pinned_execution".into();
    manifest.timeout_ms = recipe.timeout_ms;
    manifest.previous_run = previous_run;
    manifest.initial_storage = if mode == ExecutionMode::Local {
        "固定コミットからsourceへコピー。workと入力は試行ごとに新規。外部ファイル・通信はOSで隔離していない"
    } else {
        "Linuxのnamespaceで外部ネットワークを分離する方式を要求。固定版は読み取り専用、workだけを書き込み共有。起動失敗時はローカルへ切り替えない"
    }.into();
    manifest.observation_scope =
        "固定版と構造化された自己申告の観測、stdout/stderr・終了。実行経路と外部依存先は未確認"
            .into();
    manifest.command = [
        vec![
            program.to_string_lossy().into_owned(),
            recipe.entrypoint.clone(),
        ],
        recipe.args.clone(),
    ]
    .concat();
    write_json_new(&directory.join("manifest.json"), &manifest)?;
    write_json_new(&directory.join("recipe.json"), &recipe)?;
    let recorder = Recorder::new(&directory, correlation.clone())?;
    let mut command;
    let (observation_path, input_path, work_path);
    if mode == ExecutionMode::Bubblewrap {
        if !cfg!(target_os = "linux") {
            return Err(invalid(
                "bubblewrapはLinuxだけに対応します。ローカルへ自動で切り替えません",
            ));
        }
        command = Command::new(executable("bwrap")?);
        command.args([
            "--unshare-all",
            "--new-session",
            "--die-with-parent",
            "--cap-drop",
            "ALL",
        ]);
        for system in ["/usr", "/lib", "/lib64"] {
            if Path::new(system).exists() {
                command.args(["--ro-bind", system, system]);
            }
        }
        let runtime_root = program
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("ランタイムの場所を確認してください"))?;
        if !program.starts_with("/usr") {
            command.arg("--ro-bind").arg(runtime_root).arg(runtime_root);
        }
        command
            .args([
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--tmpfs",
                "/tmp",
                "--ro-bind",
            ])
            .arg(&snapshot)
            .arg("/source")
            .arg("--bind")
            .arg(&work)
            .args(["/work", "--chdir", "/work", "--"])
            .arg(&program)
            .arg(Path::new("/source").join(&recipe.entrypoint));
        observation_path = PathBuf::from("/work/observations.jsonl");
        input_path = PathBuf::from("/work/input.json");
        work_path = PathBuf::from("/work");
        // 固定版は読み取り専用。workだけを共有し、外部ネットワークを分離する。
    } else {
        command = Command::new(&program);
        command.arg(snapshot.join(&recipe.entrypoint));
        observation_path = work.join("observations.jsonl");
        input_path = work.join("input.json");
        work_path = work.clone();
    }
    command
        .args(&recipe.args)
        .current_dir(&work)
        .env_clear()
        .env("PATH", program.parent().unwrap_or(Path::new("/usr/bin")))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("HOME", &work_path)
        .env(
            "TMPDIR",
            if mode == ExecutionMode::Bubblewrap {
                Path::new("/tmp")
            } else {
                &work
            },
        )
        .env("EEL_RUN_ID", &correlation.run_id)
        .env("EEL_RECORD_ID", &correlation.record_id)
        .env("EEL_ATTEMPT_ID", &correlation.attempt_id)
        .env("EEL_OBSERVATIONS", &observation_path)
        .env("EEL_INPUT", &input_path)
        .env("EEL_WORK_DIR", &work_path)
        .env(
            "EEL_SOURCE_ROOT",
            if mode == ExecutionMode::Bubblewrap {
                Path::new("/source")
            } else {
                &snapshot
            },
        )
        .stdin(Stdio::null())
        .stdout(File::create(directory.join("stdout.txt"))?)
        .stderr(File::create(directory.join("stderr.txt"))?);
    let mut child = ChildGuard(command.spawn()?);
    recorder.record(
        Source::Process,
        EventKind::ProcessStarted,
        format!(
            "pid={}、固定コミット{}、起動ファイル{}",
            child.0.id(),
            recipe.source.commit,
            recipe.entrypoint
        ),
    )?;
    let started = Instant::now();
    let mut timed_out = false;
    let mut output_limit = false;
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        output_limit = ["stdout.txt", "stderr.txt"]
            .iter()
            .any(|p| fs::metadata(directory.join(p)).is_ok_and(|m| m.len() > MAX_TRACE));
        if started.elapsed() >= Duration::from_millis(recipe.timeout_ms) || output_limit {
            timed_out = !output_limit;
            child.0.kill()?;
            break child.0.wait()?;
        }
        thread::sleep(Duration::from_millis(5));
    };
    output_limit |= ["stdout.txt", "stderr.txt"]
        .iter()
        .any(|p| fs::metadata(directory.join(p)).is_ok_and(|m| m.len() > MAX_TRACE));
    recorder.record(
        Source::Process,
        if timed_out {
            EventKind::ProcessTimedOut
        } else {
            EventKind::ProcessExited
        },
        format!("{status}、出力上限超過={output_limit}"),
    )?;
    let (observations, mut diagnostics, observation_read) =
        load_observations(&work.join("observations.jsonl"), &correlation, &files);
    let excerpt = |path: &Path| -> io::Result<String> {
        let mut bytes = vec![];
        File::open(path)?.take(8192).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    let stdout_excerpt = excerpt(&directory.join("stdout.txt"))?;
    let stderr_excerpt = excerpt(&directory.join("stderr.txt"))?;
    if !status.success() {
        diagnostics.push(format!(
            "起動・実行・終了を確認できませんでした: {}",
            stderr_excerpt.chars().take(2000).collect::<String>()
        ));
    }
    let unchanged = snapshot_unchanged(&root, &snapshot, &files);
    let trace = ExecutionTrace {
        schema_version: 1,
        correlation,
        recipe,
        tree,
        files,
        runtime_path: program.to_string_lossy().into_owned(),
        runtime_version,
        exit_code: status.code(),
        timed_out,
        output_limit,
        snapshot_unchanged: unchanged,
        observations,
        observation_read,
        diagnostics,
        mode: mode.label().into(),
        stdout_excerpt,
        stderr_excerpt,
    };
    write_json_new(&directory.join("execution.json"), &trace)?;
    // 観測は複製して取り込める。状態の答え合わせはこの判定へ混ぜない。
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("observations.jsonl"))?;
    for observation in &trace.observations {
        serde_json::to_writer(&mut out, observation).map_err(io::Error::other)?;
        out.write_all(b"\n")?;
    }
    out.sync_all()?;
    finish_run(directory, manifest, Truth::NotInspected)
}

pub fn execute(
    repository_root: &Path,
    output: &Path,
    recipe: Recipe,
    condition: Condition,
    previous_run: Option<String>,
) -> io::Result<RunResult> {
    execute_with_mode(
        repository_root,
        output,
        recipe,
        condition,
        previous_run,
        ExecutionMode::Local,
    )
}

pub fn assess_run(
    directory: &Path,
    manifest: &Manifest,
    events: &[Event],
) -> io::Result<Assessment> {
    if manifest.mode == "pinned_execution" {
        let trace: ExecutionTrace = read_json(&directory.join("execution.json"))?;
        Ok(assess_trace(manifest, &trace, &trace.recipe.criterion))
    } else {
        Ok(crate::assessment::assess(manifest, events))
    }
}

/// 公開GitHubの固定コミットを取得する。checkoutや対象コードの起動は行わない。
pub fn prepare_repository(destination: &Path, recipe: &Recipe) -> io::Result<()> {
    validate_recipe(recipe)?;
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(destination)?;
    git(destination, &["init", "-q"])?;
    let origin = format!("https://github.com/{}.git", recipe.source.repository);
    git(destination, &["remote", "add", "origin", &origin])?;
    git(
        destination,
        &[
            "-c",
            "credential.helper=",
            "-c",
            "credential.interactive=false",
            "fetch",
            "--depth=1",
            "--no-tags",
            "origin",
            &recipe.source.commit,
        ],
    )?;
    if git_text(
        destination,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", recipe.source.commit),
        ],
    )? != recipe.source.commit
    {
        return Err(invalid("取得したコミットが試験設定と一致しません"));
    }
    git(
        destination,
        &["update-ref", "refs/heads/prepared", &recipe.source.commit],
    )?;
    git(
        destination,
        &["symbolic-ref", "HEAD", "refs/heads/prepared"],
    )?;
    write_json_new(
        &destination.join("prepared.json"),
        &serde_json::json!({"schema_version":1,"repository":recipe.source.repository,"commit":recipe.source.commit,"checkout":false,"executed":false}),
    )
}
