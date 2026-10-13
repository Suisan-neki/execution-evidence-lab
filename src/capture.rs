//! 外部コマンドの出力を記録する入口。サンドボックスや業務条件の判定器ではない。
use crate::{
    experiment::{finish_run, make_manifest},
    model::*,
    recording::*,
};
use std::{
    fs::OpenOptions,
    io,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct CaptureConfig {
    pub output: PathBuf,
    pub condition: String,
    pub command: Vec<String>,
    pub timeout_ms: u64,
}

pub fn capture(config: CaptureConfig) -> io::Result<RunResult> {
    if config.command.is_empty()
        || config.condition.trim().is_empty()
        || !(1..=300_000).contains(&config.timeout_ms)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "条件、コマンド、1〜300000msの待機上限が必要",
        ));
    }
    let (directory, correlation) = new_run_directory(&config.output)?;
    let condition = Condition {
        id: "custom".into(),
        version: 1,
        text: config.condition,
        origin: "実行者がコマンドに指定した問い。自動判定規則は未定義".into(),
        kind: ConditionKind::Custom,
    };
    let mut manifest = make_manifest(correlation.clone(), condition)?;
    manifest.mode = "capture".into();
    manifest.command = config.command.clone();
    manifest.timeout_ms = config.timeout_ms;
    manifest.initial_storage =
        "作業ディレクトリのみ新規。外部ファイル・DB・環境変数は初期化していない".into();
    manifest.observation_scope = "直接起動したプロセスのstdout/stderr/終了だけ。対象のコード版、子孫プロセス、通信・保存は未観測".into();
    write_json_new(&directory.join("manifest.json"), &manifest)?;
    let recorder = Recorder::new(&directory, correlation)?;
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("stdout.txt"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("stderr.txt"))?;
    // シェルを挟まない。環境は継承するため、未信頼のプログラムの隔離には使わない。
    let child = Command::new(&config.command[0])
        .args(&config.command[1..])
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            recorder.record(
                Source::Process,
                EventKind::ProcessStartFailed,
                error.to_string(),
            )?;
            return finish_run(directory, manifest, Truth::NotInspected);
        }
    };
    let observed = (|| -> io::Result<()> {
        recorder.record(
            Source::Process,
            EventKind::ProcessStarted,
            format!("pid={}", child.id()),
        )?;
        let deadline = Instant::now() + Duration::from_millis(config.timeout_ms);
        loop {
            if let Some(status) = child.try_wait()? {
                recorder.record(
                    Source::Process,
                    EventKind::ProcessExited,
                    status.to_string(),
                )?;
                break;
            }
            if Instant::now() >= deadline {
                // 終了対象は直接の子のみ。プロセスツリー全体の停止は保証しない。
                match child.kill() {
                    Ok(()) => (),
                    Err(error) => {
                        if child.try_wait()?.is_none() {
                            return Err(error);
                        }
                    }
                }
                let status = child.wait()?;
                recorder.record(
                    Source::Process,
                    EventKind::ProcessTimedOut,
                    format!("待機上限超過、直接の子を回収: {status}"),
                )?;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    if let Err(error) = observed {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    finish_run(directory, manifest, Truth::NotInspected)
}
