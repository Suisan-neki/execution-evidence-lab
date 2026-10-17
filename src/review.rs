//! localhostだけで動く共同レビュー。任意のコマンドやファイルはHTTPで受け付けない。
use crate::{
    model::*,
    review_execution::{Controller, ExecuteRequest, ExecutionStatus, Job},
    workspace::{self, *},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;

pub fn html(token: &str) -> String {
    include_str!("../ui/review.html")
        .replace("__REVIEW_STYLE__", include_str!("../ui/review.css"))
        .replace("__REVIEW_SCRIPT__", include_str!("../ui/review.js"))
        .replace("__REVIEW_TOKEN__", token)
}

pub fn explorer_html() -> String {
    include_str!("../ui/explorer.html")
        .replace("__EXPLORER_STYLE__", include_str!("../ui/explorer.css"))
}

/// 理解と試験の画面。デジタル庁デザインシステムのCSSを同じ応答に含める。
pub fn app_html(token: &str) -> String {
    include_str!("../ui/app.html")
        .replace(
            "__APP_STYLE__",
            &[
                include_str!("../ui/dads/tokens.css"),
                include_str!("../ui/dads/components.css"),
                include_str!("../ui/app.css"),
            ]
            .concat(),
        )
        .replace("__REVIEW_TOKEN__", token)
}

pub fn architecture_html(token: &str) -> String {
    include_str!("../ui/architecture.html")
        .replace(
            "__ARCHITECTURE_STYLE__",
            &[
                include_str!("../ui/dads/tokens.css"),
                include_str!("../ui/dads/components.css"),
                include_str!("../ui/architecture.css"),
            ]
            .concat(),
        )
        .replace("__REVIEW_TOKEN__", token)
}

/// 画面版で使う、読取記録の保存先とAIの設定。
#[derive(Clone)]
pub struct App {
    pub store: crate::understanding::Store,
    pub ai: Option<crate::ai::Config>,
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}
fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut bytes = vec![];
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() >= MAX_HEADER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HTTPヘッダーが大きすぎます",
            ));
        }
        let mut part = [0; 1024];
        let count = stream.read(&mut part)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTPヘッダーが途中で終わっています",
            ));
        }
        bytes.extend_from_slice(&part[..count]);
    };
    if header_end > MAX_HEADER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTPヘッダーが大きすぎます",
        ));
    }
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(io::Error::other)?;
    let mut lines = header.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP/1.1のリクエストが必要です",
        ));
    }
    let method = parts[0].to_owned();
    let path = parts[1].to_owned();
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "HTTPヘッダーの形式が不正です")
        })?;
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HTTPヘッダーが重複しています",
            ));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chunked形式は未対応です",
        ));
    }
    let length = headers
        .get("content-length")
        .map_or(Ok(0), |s| s.parse::<usize>().map_err(io::Error::other))?;
    let limit = if method == "POST" && path == "/api/v1/repository" {
        8 * 1024 * 1024
    } else {
        MAX_BODY
    };
    if length > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("入力は{limit}バイト以内です"),
        ));
    }
    let mut body = bytes[header_end..].to_vec();
    if body.len() > length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "本文の長さが一致しません",
        ));
    }
    while body.len() < length {
        let mut part = [0; 4096];
        let count = stream.read(&mut part[..(length - body.len()).min(4096)])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "本文が途中で終わっています",
            ));
        }
        body.extend_from_slice(&part[..count]);
    }
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) -> io::Result<()> {
    respond_encoded(stream, status, content_type, body, "")
}
fn respond_encoded(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    encoding: &str,
) -> io::Result<()> {
    let phrase = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {phrase}\r\nContent-Type: {content_type}\r\n{encoding}Content-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self' https://api.github.com https://raw.githubusercontent.com; img-src 'self' blob:; form-action 'none'; frame-ancestors 'none'; base-uri 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}
fn json(stream: &mut TcpStream, status: u16, value: &impl serde::Serialize) -> io::Result<()> {
    respond(
        stream,
        status,
        "application/json; charset=utf-8",
        &serde_json::to_vec(value).map_err(io::Error::other)?,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoRequest {
    expected_revision: u64,
    actor: String,
    reason: String,
    check_id: String,
    fault: Fault,
}

/// 任意ファイルへのアクセスや任意コマンド実行は公開しない。変更JSONのimportはCLI専用。
pub fn handle_connection(mut stream: TcpStream, directory: &Path, token: &str) -> io::Result<()> {
    handle_connection_mode(&mut stream, Some(directory), token, None, None)
}

pub fn handle_connection_with_execution(
    mut stream: TcpStream,
    directory: &Path,
    token: &str,
    controller: &Controller,
) -> io::Result<()> {
    handle_connection_mode(&mut stream, Some(directory), token, Some(controller), None)
}

pub fn handle_connection_app(
    mut stream: TcpStream,
    directory: &Path,
    token: &str,
    controller: &Controller,
    app: &App,
) -> io::Result<()> {
    handle_connection_mode(
        &mut stream,
        Some(directory),
        token,
        Some(controller),
        Some(app),
    )
}

fn handle_connection_mode(
    stream: &mut TcpStream,
    directory: Option<&Path>,
    token: &str,
    controller: Option<&Controller>,
    app: Option<&App>,
) -> io::Result<()> {
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(error) => {
            return json(
                stream,
                400,
                &serde_json::json!({"error": error.to_string()}),
            );
        }
    };
    let port = stream.local_addr()?.port();
    let host = request
        .headers
        .get("host")
        .map(String::as_str)
        .unwrap_or("");
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return json(
            stream,
            403,
            &serde_json::json!({"error": "localhostのHostだけを受け付けます"}),
        );
    }
    let origin = format!("http://{host}");
    let versioned = request.path.starts_with("/api/v1/");
    if versioned
        && request.path != "/api/v1/capabilities"
        && request.headers.get("x-review-token").map(String::as_str) != Some(token)
    {
        return json(
            stream,
            403,
            &serde_json::json!({"error":"APIの操作トークンが必要です"}),
        );
    }
    if request.method == "POST"
        && ((if versioned {
            request.headers.get("origin").is_some_and(|v| v != &origin)
        } else {
            request.headers.get("origin") != Some(&origin)
        }) || request.headers.get("x-review-token").map(String::as_str) != Some(token)
            || request.headers.get("content-type").map(String::as_str) != Some("application/json"))
    {
        return json(
            stream,
            403,
            &serde_json::json!({"error": "画面のOrigin・操作トークン・JSON形式が必要です"}),
        );
    }
    if request.method == "GET" && !request.body.is_empty() {
        return json(
            stream,
            400,
            &serde_json::json!({"error": "GETに本文は指定できません"}),
        );
    }
    if request.method == "GET" && request.path == "/api/v1/capabilities" {
        return json(
            stream,
            200,
            &serde_json::json!({"schema_version":1,"api_version":"v1","scope":"localhost","routes":if app.is_some() {serde_json::json!(["case","change","compare","backup","execution","execute","browser-screenshot","architecture","repository","understand","prepare","ai"])} else {serde_json::json!(["case","change","compare","backup","execution","execute","browser-screenshot","architecture","repository"])},"reported_observations":true,"limits":{"request_bytes":MAX_BODY,"backup_bytes":crate::backup::MAX_BACKUP,"reader_request_bytes":8*1024*1024,"reader_seconds":30,"reader_output_bytes":8*1024*1024},"understanding":"作者の説明と静的解析。実行経路・業務上の意味は未確認"}),
        );
    }
    if let Some(app) = app {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/") => {
                return respond(
                    stream,
                    200,
                    "text/html; charset=utf-8",
                    app_html(token).as_bytes(),
                );
            }
            ("GET", "/review") => {
                return respond(
                    stream,
                    200,
                    "text/html; charset=utf-8",
                    "<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\"><title>検証用の例</title><body><p>保存の練習用デモ（検証用の例）は、この画面とは別の事例で動きます。次のコマンドで別に起動してください。</p><pre>cargo run -- case init demo-case\ncargo run -- review demo-case --port 8766</pre><p><a href=\"/\">理解と試験の画面へ戻る</a></p></body></html>".as_bytes(),
                );
            }
            ("GET", "/ui/app.js") => {
                return respond(
                    stream,
                    200,
                    "text/javascript; charset=utf-8",
                    include_str!("../ui/app.js").as_bytes(),
                );
            }
            ("POST", "/api/v1/understand") => {
                let result = serde_json::from_slice::<crate::understanding::UnderstandRequest>(
                    &request.body,
                )
                .map_err(io::Error::other)
                .and_then(|r| app.store.handle(r));
                return result_response(stream, result);
            }
            ("GET", "/api/v1/ai") => {
                return json(stream, 200, &crate::ai::status(app.ai.as_ref()));
            }
            ("POST", "/api/v1/ai") => {
                let result = serde_json::from_slice::<crate::ai::AiRequest>(&request.body)
                    .map_err(io::Error::other)
                    .and_then(|r| crate::ai::explain(app.ai.as_ref(), &app.store, r));
                return result_response(stream, result);
            }
            ("POST", "/api/v1/prepare") => {
                let result = controller
                    .ok_or_else(|| io::Error::other("実行の設定がありません"))
                    .and_then(|c| {
                        serde_json::from_slice::<crate::review_execution::PrepareRequest>(
                            &request.body,
                        )
                        .map_err(io::Error::other)
                        .and_then(|r| c.prepare(r))
                    });
                return result_response(stream, result);
            }
            _ => {}
        }
    }
    if request.method == "POST" && request.path == "/api/v1/repository" {
        let result = serde_json::from_slice::<crate::reader::ReaderRequest>(&request.body)
            .map_err(io::Error::other)
            .and_then(crate::reader::read);
        return result_response(stream, result);
    }
    if request.method == "GET" && request.path == "/ui/architecture-likec4.js" {
        return respond_encoded(
            stream,
            200,
            "text/javascript; charset=utf-8",
            include_bytes!("../ui/architecture-generated/likec4.js.gz"),
            "Content-Encoding: gzip\r\n",
        );
    }
    if request.method == "GET" {
        let resource = match request.path.as_str() {
            "/architecture" => Some(("text/html; charset=utf-8", architecture_html(token))),
            "/ui/architecture.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/architecture.js").to_owned(),
            )),
            "/ui/architecture-ids.json" => Some((
                "application/json; charset=utf-8",
                include_str!("../ui/architecture-generated/ids.json").to_owned(),
            )),
            "/" | "/explore" => Some((
                "text/html; charset=utf-8",
                explorer_html().replace("__REVIEW_TOKEN__", token),
            )),
            "/ui/browser-evidence.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/browser-evidence.js").to_owned(),
            )),
            "/ui/explorer.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/explorer.js").to_owned(),
            )),
            "/ui/comprehension.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/comprehension.js").to_owned(),
            )),
            "/ui/call-navigation.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/call-navigation.js").to_owned(),
            )),
            "/ui/trial-comparison.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/trial-comparison.js").to_owned(),
            )),
            "/ui/scanner.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../ui/scanner.js").to_owned(),
            )),
            _ => None,
        };
        if let Some((content_type, body)) = resource {
            return respond(stream, 200, content_type, body.as_bytes());
        }
    }
    let Some(directory) = directory else {
        return json(
            stream,
            404,
            &serde_json::json!({"error": "テストの事例を開いていません"}),
        );
    };
    if request.method == "GET" && request.path == "/api/v1/architecture" {
        let result = workspace::load(directory).map(|w| serde_json::json!({"revision":w.revision,"models":w.architecture_models,"links":crate::architecture::links(&w),"builtin":crate::architecture::openemr(),"builtins":crate::architecture::builtins()}));
        return result_response(stream, result);
    }
    let api_path = if versioned {
        request.path.replacen("/api/v1/", "/api/", 1)
    } else {
        request.path.clone()
    };
    if request.method == "GET" && api_path.starts_with("/api/browser-screenshot/") {
        if !versioned {
            return json(
                stream,
                403,
                &serde_json::json!({"error":"画面写真は認証付きのv1 APIで読み出してください"}),
            );
        }
        let parts = api_path
            .trim_start_matches("/api/browser-screenshot/")
            .split('/')
            .collect::<Vec<_>>();
        let result = (|| {
            if parts.len() != 2 {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "画面写真の識別子が不正です",
                ));
            }
            let case = workspace::load(directory)?;
            let evidence = case
                .evidence
                .iter()
                .find(|e| e.id == parts[0])
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "対応する試行がありません")
                })?;
            let trace = evidence.execution.as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "ブラウザの試行がありません")
            })?;
            crate::browser_evidence::screenshot(directory, trace, parts[1])
        })();
        return match result {
            Ok(bytes) => respond(stream, 200, "image/png", &bytes),
            Err(error) => json(stream, 404, &serde_json::json!({"error":error.to_string()})),
        };
    }
    match (request.method.as_str(), api_path.as_str()) {
        ("GET", "/api/backup") => result_response(stream, crate::backup::snapshot(directory)),
        ("POST", "/api/compare") => {
            let result = serde_json::from_slice::<crate::comparison::CompareRequest>(&request.body)
                .map_err(io::Error::other)
                .and_then(|request| {
                    crate::comparison::compare(workspace::load(directory)?, request)
                });
            result_response(stream, result)
        }
        ("GET", "/review") => respond(
            stream,
            200,
            "text/html; charset=utf-8",
            html(token).as_bytes(),
        ),
        ("GET", "/api/case") => match workspace::load(directory) {
            Ok(case) => json(stream, 200, &view(case)),
            Err(error) => json(
                stream,
                400,
                &serde_json::json!({"error": error.to_string()}),
            ),
        },
        ("GET", "/api/execution") => {
            let result =
                controller.map_or_else(|| Ok(ExecutionStatus::disabled()), Controller::status);
            result_response(stream, result)
        }
        ("POST", "/api/execute") => {
            let result = controller
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "画面からの実行は起動時にリポジトリと方式を指定した場合だけ利用できます",
                    )
                })
                .and_then(|controller| {
                    serde_json::from_slice::<ExecuteRequest>(&request.body)
                        .map_err(io::Error::other)
                        .and_then(|request| controller.start(request))
                });
            match result {
                Ok(job) => json(stream, 202, &job),
                Err(error) => result_response(stream, Err::<Job, _>(error)),
            }
        }
        ("POST", "/api/change") => {
            let result = serde_json::from_slice::<ChangeRequest>(&request.body)
                .map_err(io::Error::other)
                .and_then(|change| {
                    if matches!(change.action, Change::Import { .. }) {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "試行の取り込みはCLIから行ってください",
                        ));
                    }
                    workspace::change(directory, change)
                });
            result_response(stream, result)
        }
        ("POST", "/api/demo") => {
            let result = serde_json::from_slice::<DemoRequest>(&request.body)
                .map_err(io::Error::other)
                .and_then(|request| {
                    workspace::run_demo(
                        directory,
                        request.expected_revision,
                        request.actor,
                        request.reason,
                        request.check_id,
                        request.fault,
                    )
                });
            result_response(stream, result)
        }
        _ => json(
            stream,
            404,
            &serde_json::json!({"error": "この操作はありません"}),
        ),
    }
}
fn result_response<T: serde::Serialize>(
    stream: &mut TcpStream,
    result: io::Result<T>,
) -> io::Result<()> {
    match result {
        Ok(value) => json(stream, 200, &value),
        Err(error) => {
            let status = match error.kind() {
                io::ErrorKind::AlreadyExists | io::ErrorKind::WouldBlock => 409,
                io::ErrorKind::PermissionDenied => 403,
                _ => 400,
            };
            json(
                stream,
                status,
                &serde_json::json!({"error": error.to_string()}),
            )
        }
    }
}
pub fn serve(directory: PathBuf, port: u16) -> io::Result<()> {
    workspace::load(&directory)?;
    serve_mode(Some(directory), port, None, None, None)
}

pub fn serve_explorer(port: u16) -> io::Result<()> {
    serve_mode(None, port, None, None, None)
}

/// 理解と試験の画面を起動する。データの保存先に、事例・読取記録・取得したリポジトリ・試行を置く。
pub fn serve_app(
    data: PathBuf,
    port: u16,
    mode: crate::execution::ExecutionMode,
) -> io::Result<()> {
    std::fs::create_dir_all(&data)?;
    let case = data.join("case");
    if !case.exists() {
        workspace::initialize(
            &case,
            workspace::empty_workspace("app".into(), "期待する動作と試験の記録".into()),
            "基盤",
            "画面の初回起動で、空の事例を作成した",
        )?;
    }
    workspace::load(&case)?;
    let controller = Controller::managed(case.clone(), data.join("repositories"), mode)?;
    let app = App {
        store: crate::understanding::Store::new(data.join("snapshots"))?,
        ai: crate::ai::Config::from_env(),
    };
    println!(
        "データの保存先: {}\nAIによる説明: {}",
        data.display(),
        crate::ai::status(app.ai.as_ref())["message"]
            .as_str()
            .unwrap_or_default()
    );
    serve_mode(Some(case), port, Some(controller), None, Some(app))
}

pub fn serve_with_execution(
    directory: PathBuf,
    port: u16,
    repository_root: PathBuf,
    mode: crate::execution::ExecutionMode,
) -> io::Result<()> {
    let controller = Controller::new(directory.clone(), repository_root, mode)?;
    serve_mode(Some(directory), port, Some(controller), None, None)
}
pub fn serve_api(
    directory: PathBuf,
    port: u16,
    token_file: PathBuf,
    execution: Option<(PathBuf, crate::execution::ExecutionMode)>,
) -> io::Result<()> {
    workspace::load(&directory)?;
    let controller = execution
        .map(|(root, mode)| Controller::new(directory.clone(), root, mode))
        .transpose()?;
    serve_mode(Some(directory), port, controller, Some(token_file), None)
}

fn serve_mode(
    directory: Option<PathBuf>,
    port: u16,
    controller: Option<Controller>,
    token_file: Option<PathBuf>,
    app: Option<App>,
) -> io::Result<()> {
    let mut bytes = [0; 24];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    if let Some(path) = token_file {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
    }
    if app.is_some() {
        println!(
            "理解と試験の画面: http://{}\n期待・試行・判断の記録の保存先: {}\n終了: Ctrl+C。",
            listener.local_addr()?,
            directory
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_default()
        );
    } else {
        println!("理解: http://{}\n終了: Ctrl+C。", listener.local_addr()?);
    }
    if let (Some(directory), None) = (&directory, &app) {
        println!(
            "テストと判断の記録: http://{}/review\n事例は{}へ保存します。",
            listener.local_addr()?,
            directory.display()
        );
    }
    struct Active(Arc<AtomicUsize>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("接続エラー: {e}");
                continue;
            }
        };
        if active.fetch_add(1, Ordering::SeqCst) >= 16 {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = json(
                &mut stream,
                503,
                &serde_json::json!({"error":"接続の上限です。後で再試行してください"}),
            );
            continue;
        }
        let guard = Active(active.clone());
        let directory = directory.clone();
        let token = token.clone();
        let controller = controller.clone();
        let app = app.clone();
        if let Err(e) = thread::Builder::new()
            .name("backend-request".into())
            .spawn(move || {
                let _guard = guard;
                if let Err(e) = handle_connection_mode(
                    &mut stream,
                    directory.as_deref(),
                    &token,
                    controller.as_ref(),
                    app.as_ref(),
                ) {
                    eprintln!("接続エラー: {e}");
                }
            })
        {
            eprintln!("接続の受付エラー: {e}");
        }
    }
    Ok(())
}
