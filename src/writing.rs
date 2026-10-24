//! 本家yomiyasuの固定版から取り出した規則を、日本語の生成に使う。
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;

const RULES: &str = include_str!("../third_party/yomiyasu/runtime-rules.md");
const UPSTREAM: &str = include_str!("../third_party/yomiyasu/UPSTREAM.json");
const CONTRACT: &str = "以下は運営者が組み込んだyomiyasuの文章規則です。利用者に見せる日本語に適用してください。JSONの項目・構造、ID、参照行、コード、正式名称、数値、否定、条件、未確認の状態は変更しません。指定された出力形式を守り、校正の報告や内部手順を出力に追加しません。利用者が選んだ非技術者はgeneral、技術者はexpertです。非技術者には、まず誰が何を入力し誰に結果が返るかを説明します。図に技術的な名前がある場合は、担う役割を先に述べ、必要なら元の名前を括弧で対応づけます。コードのパスと行番号は根拠として添え、説明の主語にしません。資料で分からない役割や動作を推測で補いません。構成図を両方の立場へ生成する場合は、それぞれの説明欄で使い分けます。上流の検査ツールを実行していないのに、検査済み・意味の一致を確認済みとは書きません。";

pub(crate) fn policy() -> String {
    format!("{CONTRACT}\n\n{RULES}")
}
pub(crate) fn metadata() -> Value {
    let source: Value = serde_json::from_str(UPSTREAM).expect("同梱したyomiyasuの出所");
    json!({"name":"yomiyasu", "version":source["version"], "commit":source["commit"],
        "rules_sha256":format!("{:x}", Sha256::digest(RULES.as_bytes())),
        "policy_sha256":format!("{:x}", Sha256::digest(policy().as_bytes())), "method":"generation_instructions", "runtime_lint":false})
}
pub(crate) fn apply(messages: &mut Value) -> io::Result<()> {
    let system = messages
        .as_array_mut()
        .and_then(|v| v.first_mut())
        .filter(|v| v["role"] == "system")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "AIへのシステム指示がありません",
            )
        })?;
    let original = system["content"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "AIへのシステム指示が文字列ではありません",
        )
    })?;
    system["content"] = json!(format!("{original}\n\n{}", policy()));
    Ok(())
}
