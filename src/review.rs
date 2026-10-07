//! localhostだけで動く共同レビュー。任意のコマンドやファイルはHTTPで受け付けない。
use crate::{
    model::*,
    workspace::{self, *},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
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
    if length > MAX_BODY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "入力は1MiB以内です",
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
    let phrase = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {phrase}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; form-action 'none'; frame-ancestors 'none'; base-uri 'none'\r\nConnection: close\r\n\r\n",
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
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            return json(
                &mut stream,
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
            &mut stream,
            403,
            &serde_json::json!({"error": "localhostのHostだけを受け付けます"}),
        );
    }
    let origin = format!("http://{host}");
    if request.method == "POST"
        && (request.headers.get("origin") != Some(&origin)
            || request.headers.get("x-review-token").map(String::as_str) != Some(token)
            || request.headers.get("content-type").map(String::as_str) != Some("application/json"))
    {
        return json(
            &mut stream,
            403,
            &serde_json::json!({"error": "画面のOrigin・操作トークン・JSON形式が必要です"}),
        );
    }
    if request.method == "GET" && !request.body.is_empty() {
        return json(
            &mut stream,
            400,
            &serde_json::json!({"error": "GETに本文は指定できません"}),
        );
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => respond(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            html(token).as_bytes(),
        ),
        ("GET", "/api/case") => match workspace::load(directory) {
            Ok(case) => json(&mut stream, 200, &view(case)),
            Err(error) => json(
                &mut stream,
                400,
                &serde_json::json!({"error": error.to_string()}),
            ),
        },
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
            result_response(&mut stream, result)
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
            result_response(&mut stream, result)
        }
        _ => json(
            &mut stream,
            404,
            &serde_json::json!({"error": "この操作はありません"}),
        ),
    }
}
fn result_response(stream: &mut TcpStream, result: io::Result<WorkspaceView>) -> io::Result<()> {
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
    let mut bytes = [0; 24];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!(
        "共同レビュー: http://{}\n終了: Ctrl+C。事例は{}へ保存します。",
        listener.local_addr()?,
        directory.display()
    );
    for stream in listener.incoming() {
        match stream.and_then(|stream| handle_connection(stream, &directory, &token)) {
            Ok(()) => (),
            Err(error) => eprintln!("レビューの接続エラー: {error}"),
        }
    }
    Ok(())
}
