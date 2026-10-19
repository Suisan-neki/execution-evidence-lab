//! AIのJSONを型と上限で検査し、引用を実際に渡したコード抜粋へ照合する。
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io};

pub(crate) type ProvidedLines = BTreeSet<(String, u64)>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    path: String,
    line: u64,
    #[serde(default)]
    end_line: Option<u64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    text: String,
    refs: Vec<Reference>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    expectation: String,
    reason: String,
    refs: Vec<Reference>,
    unknowns: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Response {
    answer: String,
    claims: Vec<Claim>,
    unknowns: Vec<String>,
    #[serde(default)]
    test_candidates: Vec<Candidate>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "AIの応答が決められた形式や上限を満たしていません",
    )
}
fn text_ok(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= limit
}
fn strings_ok(values: &[String]) -> bool {
    values.len() <= 16 && values.iter().all(|s| text_ok(s, 2000))
}
fn refs_ok(values: &[Reference]) -> bool {
    values.len() <= 8 && values.iter().all(|r| text_ok(&r.path, 1024))
}

pub(crate) fn assess(
    content: &str,
    snapshot: &Value,
    provided: &ProvidedLines,
) -> io::Result<Value> {
    let response: Response = serde_json::from_str(content).map_err(|_| invalid())?;
    if !text_ok(&response.answer, 8000)
        || response.claims.len() > 64
        || !strings_ok(&response.unknowns)
        || response.test_candidates.len() > 8
        || response
            .claims
            .iter()
            .any(|c| !text_ok(&c.text, 2000) || !refs_ok(&c.refs))
        || response.test_candidates.iter().any(|c| {
            !text_ok(&c.expectation, 2000)
                || !text_ok(&c.reason, 2000)
                || !refs_ok(&c.refs)
                || !strings_ok(&c.unknowns)
        })
    {
        return Err(invalid());
    }
    let mut result = serde_json::to_value(response).map_err(io::Error::other)?;
    // 欠けたend_lineは開始行と同じ。モデル由来のverified等の判定欄は受け付けない。
    for key in ["claims", "test_candidates"] {
        for item in result[key].as_array_mut().ok_or_else(invalid)? {
            for r in item["refs"].as_array_mut().ok_or_else(invalid)? {
                if r["end_line"].is_null() {
                    r["end_line"] = r["line"].clone();
                }
            }
        }
    }
    let mut unverified = 0;
    let mut outside_context = 0;
    for key in ["claims", "test_candidates"] {
        let items = result[key].as_array_mut().ok_or_else(invalid)?;
        unverified += crate::understanding::verify_claims(items, snapshot);
        for item in items {
            let refs = item["refs"].as_array_mut().ok_or_else(invalid)?;
            let mut usable = !refs.is_empty();
            for r in refs {
                let path = r["path"].as_str().unwrap_or_default();
                let start = r["line"].as_u64().unwrap_or(0);
                let end = r["end_line"].as_u64().unwrap_or(start);
                // 固定版に存在しても、今回送っていない行を根拠にはしない。
                let included = r["verified"] == true
                    && (start..=end).all(|n| provided.contains(&(path.to_owned(), n)));
                if !included {
                    outside_context += 1;
                }
                r["in_context"] = json!(included);
                usable &= included;
            }
            item["references_in_context"] = json!(usable);
            if key == "test_candidates" {
                item["can_use"] = json!(usable);
            }
        }
    }
    result["unverified_references"] = json!(unverified);
    result["references_outside_context"] = json!(outside_context);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Value {
        json!({"fullName":"example/notes", "commit":"a".repeat(40), "files":{"app.py":"save(value)\nprint(value)\nreturn value\n"}})
    }
    fn response(refs: Value) -> Value {
        json!({"answer":"保存処理がある", "claims":[], "unknowns":["閲覧権限は不明"], "test_candidates":[{"expectation":"保存後に同じ値を読み出せる", "reason":"表示と保存を分けて確認する", "refs":refs, "unknowns":["実際の保存先は未確認"]}]})
    }
    #[test]
    fn only_supplied_fixed_lines_allow_a_candidate_and_unknowns_are_preserved() {
        let provided = [("app.py".into(), 1), ("app.py".into(), 2)].into();
        let value = response(json!([{"path":"app.py", "line":1, "end_line":2}]));
        let result = assess(&value.to_string(), &snapshot(), &provided).unwrap();
        assert_eq!(result["test_candidates"][0]["can_use"], true);
        assert_eq!(
            result["test_candidates"][0]["unknowns"],
            value["test_candidates"][0]["unknowns"]
        );
        assert_eq!(result["unknowns"], value["unknowns"]);
        assert!(result.get("verdict").is_none());
        for refs in [
            json!([]),
            json!([{"path":"app.py","line":3}]),
            json!([{"path":"missing.py","line":1}]),
            json!([{"path":"app.py","line":0}]),
            json!([{"path":"app.py","line":1,"end_line":99}]),
            json!([{"path":"app.py","line":1},{"path":"app.py","line":3}]),
        ] {
            let result = assess(&response(refs).to_string(), &snapshot(), &provided).unwrap();
            assert_eq!(result["test_candidates"][0]["can_use"], false);
        }
    }
    #[test]
    fn malformed_and_forged_responses_fail_without_panicking() {
        let provided = ProvidedLines::new();
        for value in [
            json!(null),
            json!([]),
            json!({"answer":42}),
            json!({"answer":"説明","claims":[null],"unknowns":[]}),
            response(json!([{"path":"app.py","line":1,"verified":true}])),
            response(json!([{"path":"app.py","line":-1}])),
        ] {
            assert!(assess(&value.to_string(), &snapshot(), &provided).is_err());
        }
        let mut value = response(json!([]));
        value["test_candidates"] = json!(vec![value["test_candidates"][0].clone(); 9]);
        assert!(assess(&value.to_string(), &snapshot(), &provided).is_err());
    }
    #[test]
    fn explanation_without_candidates_remains_readable() {
        let result = assess(
            r#"{"answer":"説明","claims":[],"unknowns":[]}"#,
            &snapshot(),
            &ProvidedLines::new(),
        )
        .unwrap();
        assert_eq!(result["test_candidates"], json!([]));
    }
}
