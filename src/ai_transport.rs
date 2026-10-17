//! AI説明のHTTP接続に残るNode.js用の実行処理。コード解析には使わない。
use crate::recording::{new_run_directory, write_json_new};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub(crate) fn node() -> io::Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = directory.join("node");
        if path.is_file() {
            return fs::canonicalize(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "AI説明の接続にはNode.js 22以上が必要です",
    ))
}

/// 基盤に組み込んだスクリプトだけをNode.jsで動かす。対象リポジトリのコードは渡さない。
pub(crate) fn run_script(
    script: &str,
    request: &[u8],
    timeout: Duration,
    output_limit: u64,
) -> io::Result<Value> {
    let (path, _) = new_run_directory(&std::env::temp_dir().join("eel-reader"))?;
    let _directory = Temporary(path.clone());
    fs::write(path.join("reader.mjs"), script)?;
    write_json_new(
        &path.join("package.json"),
        &serde_json::json!({"type":"module"}),
    )?;
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // 認証情報を含む場合があるため、本人だけが読める権限で書く。
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path.join("request.json"))?;
        file.write_all(request)?;
    }
    let stdout = fs::File::create(path.join("output.json"))?;
    let stderr = fs::File::create(path.join("error.txt"))?;
    let mut command = Command::new(node()?);
    command.env_clear();
    // AI接続の処理に、ホストが設定した接続経路とCAだけを渡す。
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "NODE_EXTRA_CA_CERTS",
        "NODE_USE_ENV_PROXY",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = Child(
        command
            .args([Path::new("reader.mjs"), Path::new("request.json")])
            .current_dir(&path)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?,
    );
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("コード読取が{}秒の待機上限に達しました", timeout.as_secs()),
            ));
        }
        if fs::metadata(path.join("output.json"))?.len() > output_limit
            || fs::metadata(path.join("error.txt"))?.len() > 128 * 1024
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "読取結果の上限に達しました",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            fs::read_to_string(path.join("error.txt"))?
                .chars()
                .take(16384)
                .collect::<String>(),
        ));
    }
    if fs::metadata(path.join("output.json"))?.len() > output_limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "読取結果の上限に達しました",
        ));
    }
    crate::recording::read_json(&path.join("output.json"))
}
