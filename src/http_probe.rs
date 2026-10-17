//! ブラウザーを経由せず、試験専用サーバーから画像などを直接読み出す。
use serde::{Deserialize, Serialize};
use std::{io, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Payload {
    Raw,
    DicomFrame,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub id: String,
    pub service: String,
    pub path: String,
    pub payload: Payload,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRead {
    pub id: String,
    pub performed: bool,
    pub status: Option<u16>,
    pub content_type: String,
    pub bytes: Option<usize>,
    pub sha256: Option<String>,
    pub detail: String,
}
pub fn read(probe: &Probe, containers: &crate::container::Session) -> HttpRead {
    let mut read = HttpRead {
        id: probe.id.clone(),
        performed: false,
        status: None,
        content_type: String::new(),
        bytes: None,
        sha256: None,
        detail: String::new(),
    };
    let result = (|| -> io::Result<()> {
        let port = containers.port_of(&probe.service)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        let request = agent.get(&format!("http://127.0.0.1:{port}{}", probe.path));
        let request = if probe.payload == Payload::DicomFrame {
            request.header(
                "Accept",
                "multipart/related; type=\"application/octet-stream\"; transfer-syntax=*",
            )
        } else {
            request
        };
        let mut response = request.call().map_err(io::Error::other)?;
        read.performed = true;
        read.status = Some(response.status().as_u16());
        read.content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_vec()
            .map_err(io::Error::other)?;
        let body = match probe.payload {
            Payload::Raw => bytes.as_slice(),
            Payload::DicomFrame => single_frame(&read.content_type, &bytes)?,
        };
        read.bytes = Some(body.len());
        read.sha256 = Some(crate::environment::sha256(body));
        read.detail =
            "基盤が試験専用サーバーへ直接GETし、読み出したバイト列のSHA-256を計算した".into();
        Ok(())
    })();
    if let Err(error) = result {
        read.detail = format!("直接読取の全部または一部が未確認: {error}");
    }
    read
}
/// 一つのframeだけを要求したDICOMweb multipart/relatedに限定する。
pub fn single_frame<'a>(kind: &str, bytes: &'a [u8]) -> io::Result<&'a [u8]> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "単一フレームのmultipart応答を照合できません",
        )
    };
    if !kind.to_ascii_lowercase().starts_with("multipart/related") {
        return Err(invalid());
    }
    let boundary = kind
        .split(';')
        .skip(1)
        .find_map(|p| {
            p.trim()
                .split_once('=')
                .filter(|(k, _)| k.eq_ignore_ascii_case("boundary"))
                .map(|(_, v)| v.trim().trim_matches('"'))
        })
        .ok_or_else(invalid)?;
    if boundary.is_empty() || boundary.len() > 200 || boundary.chars().any(char::is_control) {
        return Err(invalid());
    }
    let opening = format!("--{boundary}\r\n");
    let bytes = bytes.strip_prefix(b"\r\n").unwrap_or(bytes);
    let body = bytes.strip_prefix(opening.as_bytes()).ok_or_else(invalid)?;
    let split = body
        .windows(4)
        .position(|p| p == b"\r\n\r\n")
        .ok_or_else(invalid)?;
    if split > 8192 {
        return Err(invalid());
    }
    let headers = std::str::from_utf8(&body[..split]).map_err(|_| invalid())?;
    if !headers.lines().any(|l| {
        l.to_ascii_lowercase()
            .starts_with("content-type: application/octet-stream")
    }) {
        return Err(invalid());
    }
    let payload = &body[split + 4..];
    let end = format!("\r\n--{boundary}--");
    let index = payload
        .windows(end.len())
        .position(|p| p == end.as_bytes())
        .ok_or_else(invalid)?;
    if ![b"".as_slice(), b"\r\n".as_slice()].contains(&&payload[index + end.len()..]) {
        return Err(invalid());
    }
    let additional = format!("\r\n--{boundary}\r\n");
    if payload[..index]
        .windows(additional.len())
        .any(|p| p == additional.as_bytes())
    {
        return Err(invalid());
    }
    Ok(&payload[..index])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_one_frame_without_boundary_bytes_and_rejects_partial_or_multiple_parts() {
        let body =
            b"--test\r\nContent-Type: application/octet-stream\r\n\r\n\0\xff\r\n--test--\r\n";
        assert_eq!(
            single_frame(
                "multipart/related; type=\"application/octet-stream\"; boundary=\"test\"",
                body
            )
            .unwrap(),
            b"\0\xff"
        );
        assert!(single_frame("text/html", body).is_err());
        assert!(single_frame("multipart/related; boundary=test", &body[..body.len() - 4]).is_err());
        assert!(single_frame("multipart/related; boundary=test",b"--test\r\nContent-Type: application/octet-stream\r\n\r\nfirst\r\n--test\r\nContent-Type: application/octet-stream\r\n\r\nsecond\r\n--test--").is_err());
    }
}
