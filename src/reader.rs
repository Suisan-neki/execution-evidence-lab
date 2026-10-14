//! 画面と同じ静的読取をJSON APIで提供する。対象リポジトリのコードは実行しない。
use crate::recording::{new_run_directory, write_json_new};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReaderRequest {
    Read {
        url: String,
        reference: String,
    },
    Analyze {
        snapshot: Value,
        path: Option<String>,
        line: Option<u32>,
    },
    File {
        snapshot: Value,
        path: String,
    },
}
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
const SCRIPT: &str = r#"
import fs from 'node:fs';
import {scanPublicRepository,validateSnapshot,analyze,readSource} from './scanner.js';
import {explainFunction,callersOf} from './comprehension.js';
try {
const request=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
let snapshot;
if(request.operation==='read') snapshot=await scanPublicRepository(request.url,request.reference);
else snapshot=validateSnapshot(request.snapshot);
snapshot=validateSnapshot(snapshot);
if(request.operation==='file') await readSource(snapshot,request.path);
const model=analyze(snapshot);
const guide=request.path && request.line ? explainFunction(model,request.path,request.line) : null;
const callers=guide ? callersOf(model,request.path,guide.range.start) : null;
const result=JSON.stringify({schema_version:1,snapshot,model,guide,callers,limits:'作者の説明と簡易解析。実行経路・業務上の意味は未確認。未読の範囲と候補を確認してください。'});
if(Buffer.byteLength(result)>8*1024*1024) throw Error('解析結果は8MiB以内です。対象を分けてください。');
process.stdout.write(result);
} catch(error) { process.stderr.write(String(error.message)); process.exitCode=1; }
"#;
fn node() -> io::Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = directory.join("node");
        if path.is_file() {
            return fs::canonicalize(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "読取APIにはNode.js 22以上が必要です",
    ))
}
pub fn read(request: ReaderRequest) -> io::Result<Value> {
    let bytes = serde_json::to_vec(&request).map_err(io::Error::other)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "読取APIの入力は8MiB以内です",
        ));
    }
    let (path, _) = new_run_directory(&std::env::temp_dir().join("eel-reader"))?;
    let _directory = Temporary(path.clone());
    for (name, contents) in [
        ("scanner.js", include_str!("../ui/scanner.js")),
        ("comprehension.js", include_str!("../ui/comprehension.js")),
        (
            "call-navigation.js",
            include_str!("../ui/call-navigation.js"),
        ),
        ("reader.mjs", SCRIPT),
    ] {
        fs::write(path.join(name), contents)?;
    }
    write_json_new(
        &path.join("package.json"),
        &serde_json::json!({"type":"module"}),
    )?;
    fs::write(path.join("request.json"), bytes)?;
    let stdout = fs::File::create(path.join("output.json"))?;
    let stderr = fs::File::create(path.join("error.txt"))?;
    let mut command = Command::new(node()?);
    command.env_clear();
    // 信頼する解析処理に、ホストが設定した接続経路とCAだけを渡す。
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
        if started.elapsed() > Duration::from_secs(30) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "コード読取が30秒の待機上限に達しました",
            ));
        }
        if fs::metadata(path.join("output.json"))?.len() > 8 * 1024 * 1024
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
    if fs::metadata(path.join("output.json"))?.len() > 8 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "読取結果は8MiB以内です",
        ));
    }
    crate::recording::read_json(&path.join("output.json"))
}
