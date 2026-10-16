//! 設定したOpenAI互換の接続先に、固定コミットのコード抜粋だけを根拠として質問する。
//! 認証情報はサーバー側のファイルから読み、画面へは返さない。未設定・失敗はそのまま返す。
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    base_url: String,
    model: String,
    key_file: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiRequest {
    pub repository: String,
    pub commit: String,
    pub question: String,
    pub path: Option<String>,
    pub line: Option<u32>,
}

const SCRIPT: &str = r#"
import fs from 'node:fs';
try {
const r=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
let response;
try { response=await fetch(r.url,{method:'POST',headers:{'content-type':'application/json',authorization:'Bearer '+r.key},body:JSON.stringify({model:r.model,messages:r.messages,response_format:{type:'json_object'},temperature:0}),signal:AbortSignal.timeout(r.timeout_ms)}); }
catch(e) { throw Error('AIの接続先に接続できませんでした: '+e.message); }
const text=await response.text();
if(!response.ok) throw Error(`AIの接続先がHTTP ${response.status}を返しました: ${text.slice(0,300)}`);
let data; try { data=JSON.parse(text); } catch { throw Error('AIの接続先の応答をJSONとして読めませんでした'); }
const content=data.choices?.[0]?.message?.content;
if(typeof content!=='string') throw Error('AIの応答に本文がありません');
process.stdout.write(JSON.stringify({model:data.model||r.model,content}));
} catch(error) { process.stderr.write(String(error.message)); process.exitCode=1; }
"#;

const INSTRUCTIONS: &str = "あなたは、固定したコミットのコード抜粋だけを根拠に、ソフトウェアの動作を日本語で説明します。\
抜粋にない処理・設定・外部サービスを補ってはいけません。抜粋から分からないことは unknowns に書きます。\
コードに記述があることと、実行時にその処理が行われることを区別します。\
次のJSONだけを返します: {\"answer\": 文字列, \"claims\": [{\"text\": 文字列, \"refs\": [{\"path\": 文字列, \"line\": 整数, \"end_line\": 整数}]}], \"unknowns\": [文字列]}。\
claims の各文には、根拠にした抜粋のファイルと行番号を refs に必ず入れます。行番号は抜粋の各行の先頭に書かれた番号です。";

impl Config {
    pub fn new(base_url: &str, model: &str, key_file: PathBuf) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
            key_file,
        }
    }
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("EEL_AI_BASE_URL").ok()?;
        let model = std::env::var("EEL_AI_MODEL").ok()?;
        let key_file = std::env::var_os("EEL_AI_API_KEY_FILE").map(PathBuf::from)?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            model,
            key_file,
        })
    }
}

pub fn status(config: Option<&Config>) -> Value {
    match config {
        None => json!({
            "status": "not_configured",
            "message": "AIによる説明は未設定です。静的解析の結果だけを表示しています。使う場合は、サーバーの起動時に EEL_AI_BASE_URL・EEL_AI_MODEL・EEL_AI_API_KEY_FILE を指定してください",
        }),
        Some(c) => {
            let host = c
                .base_url
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or_default()
                .to_owned();
            let key = c.key_file.is_file();
            json!({
                "status": if key { "configured" } else { "failed" },
                "model": c.model,
                "host": host,
                "message": if key { "AIの接続先が設定されています。説明の根拠となる行は、基盤が固定コミットの読取記録と照合します" } else { "AIの認証情報のファイルを読めません。静的解析の結果だけを表示しています" },
            })
        }
    }
}

/// 質問に関係する行の前後を、行番号付きで抜き出す。
fn verified_ranges(v: &Value, out: &mut Vec<(String, u64, u64)>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::Array(refs)) = map.get("refs") {
                for r in refs {
                    if r["verified"] == true
                        && let (Some(p), Some(l)) = (r["path"].as_str(), r["line"].as_u64())
                    {
                        let end = r["end_line"].as_u64().unwrap_or(l);
                        out.push((p.to_owned(), l, end.min(l + 80)));
                    }
                }
            }
            for child in map.values() {
                verified_ranges(child, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| verified_ranges(i, out)),
        _ => {}
    }
}
fn excerpts(snapshot: &Value, inquiry: &Value, limit: usize) -> String {
    let mut ranges = vec![];
    verified_ranges(inquiry, &mut ranges);
    let mut text = String::new();
    let mut seen: Vec<(String, u64)> = vec![];
    for (path, start, end) in ranges {
        let Some(source) = snapshot["files"][&path].as_str() else {
            continue;
        };
        let lines: Vec<&str> = source.lines().collect();
        let from = start.saturating_sub(8).max(1);
        let to = (end + 8).min(lines.len() as u64);
        let mut block = String::new();
        for n in from..=to {
            if seen.iter().any(|(p, l)| p == &path && *l == n) {
                continue;
            }
            seen.push((path.clone(), n));
            block.push_str(&format!("{n}: {}\n", lines[n as usize - 1]));
        }
        if block.is_empty() {
            continue;
        }
        let chunk = format!("--- {path}\n{block}");
        if text.len() + chunk.len() > limit {
            break;
        }
        text.push_str(&chunk);
    }
    text
}

pub fn explain(
    config: Option<&Config>,
    store: &crate::understanding::Store,
    request: AiRequest,
) -> io::Result<Value> {
    let Some(config) = config else {
        return Ok(status(None));
    };
    let inquiry = store.handle(crate::understanding::UnderstandRequest::Inquire {
        repository: request.repository.clone(),
        commit: request.commit.clone(),
        question: request.question.clone(),
        path: request.path.clone(),
        line: request.line,
    })?;
    let snapshot = store.load(&request.repository, &request.commit)?;
    let context = excerpts(&snapshot, &inquiry["result"], 48 * 1024);
    if context.is_empty() {
        return Ok(json!({
            "status": "failed",
            "message": "質問に関係するコードの行を見つけられなかったため、AIへは問い合わせていません",
        }));
    }
    let key = match std::fs::read_to_string(&config.key_file) {
        Ok(key) if !key.trim().is_empty() => key.trim().to_owned(),
        _ => {
            return Ok(
                json!({"status": "failed", "message": "AIの認証情報のファイルを読めません"}),
            );
        }
    };
    let body = json!({
        "url": format!("{}/chat/completions", config.base_url),
        "key": key,
        "model": config.model,
        "timeout_ms": 60000,
        "messages": [
            {"role": "system", "content": INSTRUCTIONS},
            {"role": "user", "content": format!(
                "リポジトリ: {}\nコミット: {}\n質問: {}\n\nコード抜粋（行番号: 内容）:\n{context}",
                request.repository, request.commit, request.question
            )},
        ],
    });
    let bytes = serde_json::to_vec(&body).map_err(io::Error::other)?;
    let response = match crate::reader::run_script(
        SCRIPT,
        &bytes,
        Duration::from_secs(75),
        2 * 1024 * 1024,
    ) {
        Ok(value) => value,
        Err(error) => {
            return Ok(
                json!({"status": "failed", "message": format!("AIによる説明を取得できませんでした: {error}")}),
            );
        }
    };
    let content = response["content"].as_str().unwrap_or_default();
    let mut parsed: Value = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(_) => {
            return Ok(
                json!({"status": "failed", "message": "AIの応答を決められた形式（JSON）として読めませんでした"}),
            );
        }
    };
    let mut claims = parsed["claims"].as_array().cloned().unwrap_or_default();
    let unverified = crate::understanding::verify_claims(&mut claims, &snapshot);
    parsed["claims"] = Value::Array(claims);
    Ok(json!({
        "status": "ok",
        "model": response["model"],
        "answer": parsed["answer"],
        "claims": parsed["claims"],
        "unknowns": parsed["unknowns"],
        "unverified_references": unverified,
        "kind": "ai",
        "meaning": "AIによる説明。示した行が固定コミットに存在することは基盤が確かめたが、説明の正しさは確かめていない。行を開いて確認してください",
        "source": inquiry["source"],
    }))
}
