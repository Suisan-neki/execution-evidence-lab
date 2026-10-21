//! Dify Chatflowの返答と会話変数をサーバーから取得する。秘密はUIへ返さない。
use super::*;
use std::time::Duration;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    base_url: String,
    api_key: String,
}
impl Config {
    pub fn for_app(directory: &Path) -> io::Result<Option<Self>> {
        let path = directory.join("dify.json");
        if !path.exists() {
            return Ok(None);
        }
        if fs::metadata(&path)?.len() > 16384 {
            return Err(invalid("Dify設定のサイズを確認してください"));
        }
        let bytes = fs::read(path).map_err(|_| invalid("運営者のDify設定を読めません"))?;
        let mut config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("運営者のDify設定のJSON形式を確認してください"))?;
        config.base_url = config.base_url.trim_end_matches('/').into();
        let url = url::Url::parse(&config.base_url)
            .map_err(|_| invalid("Dify接続先のURLを確認してください"))?;
        if url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https"
                || url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            || config.base_url.len() > 2048
            || config.api_key.is_empty()
            || config.api_key.len() > 4096
            || !config.api_key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(invalid(
                "Difyの接続先にはHTTPS（localhostはHTTP可）とアプリAPIキーを指定してください",
            ));
        }
        Ok(Some(config))
    }
    pub(super) fn identity(&self) -> String {
        format!(
            "dify:{}",
            digest(format!("{}:{}", self.base_url, self.api_key).as_bytes())
        )
    }
    fn response(mut response: ureq::http::Response<ureq::Body>) -> io::Result<Value> {
        if !response.status().is_success() {
            return Err(invalid(format!(
                "DifyがHTTP {}を返しました。運営者が公開状態・認証・利用枠を確認してください",
                response.status().as_u16()
            )));
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map_err(|_| invalid("Difyの応答を上限内で読めません"))?;
        serde_json::from_str(&text).map_err(|_| invalid("Difyの応答をJSONとして読めません"))
    }
    pub(super) fn chat(
        &self,
        user: &str,
        conversation: Option<&str>,
        context: &str,
        query: &Value,
    ) -> io::Result<(String, String, String)> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        let body = serde_json::to_vec(&json!({"inputs":{"audience":query["audience"],"system_context":context},"query":query.to_string(),"response_mode":"blocking","conversation_id":conversation.unwrap_or_default(),"user":user,"auto_generate_name":false})).map_err(io::Error::other)?;
        let response = agent
            .post(format!("{}/chat-messages", self.base_url))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send(body.as_slice())
            .map_err(|_| invalid("Difyへ接続できませんでした。対話は更新していません"))?;
        let data = Self::response(response)?;
        let id = data["conversation_id"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 80
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .ok_or_else(|| invalid("Difyの会話IDが不正です"))?;
        let answer = data["answer"]
            .as_str()
            .ok_or_else(|| invalid("Difyの返答がありません"))?;
        let response = agent
            .get(format!("{}/conversations/{id}/variables", self.base_url))
            .query("user", user)
            .query("limit", "100")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .call()
            .map_err(|_| {
                invalid("Difyの整理案を取得できませんでした。検証内容は更新していません")
            })?;
        let variables = Self::response(response)?;
        let value = variables["data"]
            .as_array()
            .and_then(|v| v.iter().find(|v| v["name"] == "case_record"))
            .ok_or_else(|| {
                invalid("Difyにcase_recordがありません。フローと公開する版を確認してください")
            })?;
        let record = value["value"]
            .as_str()
            .ok_or_else(|| invalid("Difyのcase_recordはJSONを格納した文字列で返してください"))?;
        Ok((answer.into(), record.into(), id.into()))
    }
}
