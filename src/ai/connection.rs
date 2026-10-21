//! 運営者の設定を起動時に読み、秘密をレスポンスや通信エラーへ含めない。
use super::*;
use std::time::Duration;
use std::{fs, io::Write, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionSettings {
    base_url: String,
    model: String,
    api_key: String,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
impl Config {
    pub fn test_connection(&self) -> io::Result<Value> {
        let response = self.complete(json!([
            {"role":"system", "content":"EEL_CONNECTION_TEST: Return only the JSON object {\"connected\":true}."},
            {"role":"user", "content":"接続確認です。指定されたJSONだけを返してください。"}
        ]))?;
        let content: Value = serde_json::from_str(response["content"].as_str().unwrap_or_default())
            .map_err(|_| invalid("接続先からJSON形式の応答を得られませんでした"))?;
        if content["connected"] != true {
            return Err(invalid("接続先から想定した形式の応答を得られませんでした"));
        }
        Ok(
            json!({"status":"ok", "model":response["model"], "message":"指定したモデルから応答を受け取りました。構成図を生成できます。"}),
        )
    }
    pub(super) fn validate_connection(&self) -> io::Result<()> {
        let url = url::Url::parse(&self.base_url)
            .map_err(|_| invalid("AI接続先のURLを確認してください"))?;
        if url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https"
                || url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            || self.base_url.len() > 2048
            || self.model.trim().is_empty()
            || self.model.len() > 160
            || self.model.chars().any(char::is_control)
        {
            return Err(invalid(
                "AI接続先にはHTTPS、ローカル接続にはlocalhostのHTTPを指定し、モデル名を入力してください",
            ));
        }
        Ok(())
    }
    pub(super) fn key(&self) -> io::Result<String> {
        let key = match &self.inline_key {
            Some(key) => key.clone(),
            None => fs::read_to_string(&self.key_file)
                .map_err(|_| invalid("AIの認証情報を読めません"))?,
        };
        let key = key.trim();
        if key.is_empty() || key.len() > 4096 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(invalid("AIのAPIキーを確認してください"));
        }
        Ok(key.into())
    }
    pub fn load_connection(path: &Path) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        if fs::metadata(path)?.len() > 16384 {
            return Err(invalid("AI接続設定のサイズを確認してください"));
        }
        let bytes = fs::read(path).map_err(|_| invalid("運営者のAI設定ファイルを読めません"))?;
        let settings: ConnectionSettings = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("運営者のAI設定ファイルのJSON形式を確認してください"))?;
        let config = Self {
            base_url: settings.base_url.trim_end_matches('/').into(),
            model: settings.model.trim().into(),
            key_file: PathBuf::new(),
            inline_key: Some(settings.api_key),
        };
        config.validate_connection()?;
        config.key()?;
        Ok(Some(config))
    }
    /// 生成ではNode.jsを必要としない。JSONの構造と参照の検査は呼び出し側が行う。
    pub(crate) fn complete(&self, messages: Value) -> io::Result<Value> {
        self.validate_connection()?;
        let body = serde_json::to_vec(&json!({"model": self.model, "messages":messages, "response_format":{"type":"json_object"}})).map_err(io::Error::other)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(90)))
            .build()
            .into();
        let mut response = agent
            .post(format!("{}/chat/completions", self.base_url))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.key()?))
            .send(body.as_slice())
            .map_err(|_| {
                invalid("AI接続先へ接続できませんでした。接続設定とネットワークを確認してください")
            })?;
        if !response.status().is_success() {
            return Err(invalid(&format!(
                "AI接続先がHTTP {}を返しました。認証・モデル名・利用枠を確認してください",
                response.status().as_u16()
            )));
        }
        let text = response.body_mut().with_config().limit(2 * 1024 * 1024).read_to_string().map_err(|_| invalid("AI応答を読み取れませんでした。待機時間またはサイズの上限に達した可能性があります"))?;
        let data: Value = serde_json::from_str(&text)
            .map_err(|_| invalid("AI接続先の応答をJSONとして読めませんでした"))?;
        let content = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| invalid("AIの応答に本文がありません"))?;
        if content.len() > 512 * 1024 {
            return Err(invalid("AIの生成結果が512KiBを超えています"));
        }
        Ok(json!({"content":content,"model":data["model"].as_str().unwrap_or(&self.model)}))
    }
}

/// 開発者が編集するひな形を作る。既存のキーは上書きしない。
pub fn initialize_connection(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(include_bytes!("../../ai.example.json"))?;
    file.sync_all()
}
