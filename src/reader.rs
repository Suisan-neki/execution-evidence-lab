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
    /// 読取と同時に概要を作る。
    Open {
        url: String,
        reference: String,
    },
    Overview {
        snapshot: Value,
    },
    /// 質問・選んだ関数に関係する箇所を集める。必要な未読ファイルを最大8件読む。
    Inquire {
        snapshot: Value,
        question: String,
        path: Option<String>,
        line: Option<u32>,
    },
    Function {
        snapshot: Value,
        path: String,
        line: u32,
    },
    Plan {
        snapshot: Value,
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
import {overview} from './overview.js';
import {inquire,functionDetail} from './inquiry.js';
import {plan} from './trial-plan.js';
try {
const request=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
let snapshot;
if(request.operation==='read'||request.operation==='open') snapshot=await scanPublicRepository(request.url,request.reference);
else snapshot=validateSnapshot(request.snapshot);
snapshot=validateSnapshot(snapshot);
if(request.operation==='file') await readSource(snapshot,request.path);
let output;
const limits='作者の説明と簡易解析。実行経路・業務上の意味は未確認。未読の範囲と候補を確認してください。';
if(['read','analyze','file'].includes(request.operation)) {
  const model=analyze(snapshot);
  const guide=request.path && request.line ? explainFunction(model,request.path,request.line) : null;
  const callers=guide ? callersOf(model,request.path,guide.range.start) : null;
  output={schema_version:1,snapshot,model,guide,callers,limits};
} else if(request.operation==='inquire') {
  const focus=request.path && request.line ? {path:request.path,line:request.line} : null;
  let result=inquire(snapshot,analyze(snapshot),request.question,focus), loaded=[], failed=[];
  for(let round=0;round<2 && result.needed_files.length && loaded.length<8;round++) {
    const before=loaded.length;
    for(const path of result.needed_files.filter(p=>!(p in snapshot.files))) {
      if(loaded.length>=8) break;
      try { await readSource(snapshot,path); loaded.push(path); } catch(e) { failed.push({path,reason:e.message}); }
    }
    if(loaded.length===before) break;
    result=inquire(snapshot,analyze(snapshot),request.question,focus);
  }
  output={schema_version:1,snapshot,result:{...result,loaded_files:loaded,failed_files:failed}};
} else if(request.operation==='function') {
  let detail=functionDetail(analyze(snapshot),request.path,request.line);
  const loaded=[];
  if(!detail.found && detail.needed_files?.length) { for(const p of detail.needed_files.slice(0,2)) { await readSource(snapshot,p); loaded.push(p); } detail=functionDetail(analyze(snapshot),request.path,request.line); }
  output={schema_version:1,snapshot,result:{...detail,loaded_files:loaded}};
} else if(request.operation==='plan') {
  output={schema_version:1,snapshot,result:plan(snapshot,analyze(snapshot))};
} else {
  output={schema_version:1,snapshot,result:overview(snapshot,analyze(snapshot))};
}
const result=JSON.stringify(output);
if(Buffer.byteLength(result)>16*1024*1024) throw Error('解析結果は16MiB以内です。対象を分けてください。');
process.stdout.write(result);
} catch(error) { process.stderr.write(String(error.message)); process.exitCode=1; }
"#;
/// GitHubのAPIの利用上限を広げるための認証情報。サーバーの起動者が指定したファイルだけから読む。
fn github_token() -> io::Result<Option<String>> {
    let Some(path) = std::env::var_os("EEL_GITHUB_TOKEN_FILE") else {
        return Ok(None);
    };
    let text = fs::read_to_string(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            "EEL_GITHUB_TOKEN_FILE に指定したファイルを読めません",
        )
    })?;
    let token = text.trim();
    if token.is_empty() || token.len() > 256 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "EEL_GITHUB_TOKEN_FILE の内容を確認してください",
        ));
    }
    Ok(Some(token.to_owned()))
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
        "読取APIにはNode.js 22以上が必要です",
    ))
}

const MODULES: [(&str, &str); 6] = [
    ("scanner.js", include_str!("../ui/scanner.js")),
    ("comprehension.js", include_str!("../ui/comprehension.js")),
    (
        "call-navigation.js",
        include_str!("../ui/call-navigation.js"),
    ),
    ("overview.js", include_str!("../ui/overview.js")),
    ("inquiry.js", include_str!("../ui/inquiry.js")),
    ("trial-plan.js", include_str!("../ui/trial-plan.js")),
];

/// 基盤に組み込んだスクリプトだけをNode.jsで動かす。対象リポジトリのコードは渡さない。
pub(crate) fn run_script(
    script: &str,
    request: &[u8],
    timeout: Duration,
    output_limit: u64,
) -> io::Result<Value> {
    let (path, _) = new_run_directory(&std::env::temp_dir().join("eel-reader"))?;
    let _directory = Temporary(path.clone());
    for (name, contents) in MODULES {
        fs::write(path.join(name), contents)?;
    }
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
    if let Some(token) = github_token()? {
        command.env("EEL_GITHUB_TOKEN", token);
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

pub fn read(request: ReaderRequest) -> io::Result<Value> {
    let bytes = serde_json::to_vec(&request).map_err(io::Error::other)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "読取APIの入力は8MiB以内です",
        ));
    }
    let limit = match request {
        ReaderRequest::Read { .. } | ReaderRequest::Analyze { .. } | ReaderRequest::File { .. } => {
            8 * 1024 * 1024
        }
        _ => 16 * 1024 * 1024,
    };
    run_script(SCRIPT, &bytes, Duration::from_secs(30), limit)
}
