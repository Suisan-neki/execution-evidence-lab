//! 読取済みファイルの行番号を保持して、上限内で複数の実装箇所をモデルへ渡す。
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub(super) struct Context {
    pub text: String,
    pub provided: BTreeMap<String, BTreeSet<u32>>,
    pub hash: String,
    pub read_files: usize,
    pub excerpted_files: usize,
    pub inventory_files: usize,
    pub truncated: bool,
}
fn priority(path: &str) -> usize {
    let lower = path.to_lowercase();
    if lower.split('/').any(|s| {
        matches!(
            s,
            "tests"
                | "test"
                | "fixtures"
                | "node_modules"
                | "vendor"
                | "target"
                | "dist"
                | "build"
                | ".git"
        )
    }) || lower.ends_with(".env")
        || lower
            .rsplit('/')
            .next()
            .is_some_and(|s| s.starts_with(".env."))
    {
        return 9;
    }
    if lower
        .rsplit('/')
        .next()
        .is_some_and(|s| s.starts_with("readme") || s.starts_with("architecture"))
    {
        0
    } else if lower.ends_with(".html")
        || lower.ends_with(".tsx")
        || lower.ends_with(".jsx")
        || [
            "main.",
            "app.",
            "server.",
            "routes",
            "api/",
            "auth",
            "database",
            "storage",
            "security",
            "audit",
            "encrypt",
            "signature",
            "integrat",
        ]
        .iter()
        .any(|s| lower.contains(s))
    {
        1
    } else if lower.ends_with(".rs")
        || lower.ends_with(".py")
        || lower.ends_with(".ts")
        || lower.ends_with(".js")
    {
        2
    } else {
        3
    }
}
pub(super) fn build(snapshot: &Value) -> io::Result<Context> {
    let files = snapshot["files"]
        .as_object()
        .ok_or_else(|| invalid("読取記録に本文がありません"))?;
    let inventory = snapshot["inventory"]
        .as_array()
        .ok_or_else(|| invalid("読取記録にファイル一覧がありません"))?;
    let mut text = format!(
        "repository: {}\ncommit: {}\nFILE TREE (本文未読を含む。名前だけから接続を判断しない):\n",
        snapshot["fullName"].as_str().unwrap_or_default(),
        snapshot["commit"].as_str().unwrap_or_default()
    );
    let mut listed = 0;
    for entry in inventory {
        if let Some(path) = entry["path"].as_str() {
            if text.len() + path.len() > 24 * 1024 {
                break;
            }
            text.push_str(path);
            text.push('\n');
            listed += 1;
        }
    }
    text.push_str(&format!(
        "END TREE ({listed}/{} paths shown)\nSOURCE EXCERPTS (行番号は元のファイルの番号):\n",
        inventory.len()
    ));
    let mut selected: Vec<_> = files.iter().filter(|(p, _)| priority(p) < 9).collect();
    // 説明資料と実装を優先し、各ファイルの抜粋量を制限する。
    selected.sort_by_key(|(p, _)| (priority(p), p.matches('/').count(), p.as_str()));
    let mut provided = BTreeMap::new();
    let mut documents = 0;
    let mut processed = 0;
    for (path, raw) in selected {
        if priority(path) == 0 {
            documents += 1;
            if documents > 6 {
                continue;
            }
        }
        if processed == 64 {
            break;
        }
        processed += 1;
        let Some(source) = raw.as_str() else { continue };
        let lines: Vec<_> = source.lines().collect();
        let limit = if priority(path) == 0 { 8000 } else { 3600 };
        let mut block = format!("--- {path} ({} lines; excerpt)\n", lines.len());
        let mut included = BTreeSet::new();
        // 冒頭と、長い実装の中ほど・末尾から分散して抜粋する。
        let starts = if lines.len() <= 100 {
            vec![0]
        } else {
            vec![
                0,
                lines.len() / 4,
                lines.len() / 2,
                lines.len() * 3 / 4,
                lines.len().saturating_sub(20),
            ]
        };
        for start in &starts {
            let mut region_bytes = 0;
            let end = if starts.len() == 1 {
                lines.len()
            } else {
                (start + 25).min(lines.len())
            };
            for (index, value) in lines.iter().enumerate().take(end).skip(*start) {
                let line = format!("{}: {}\n", index + 1, value);
                if region_bytes + line.len() > limit / starts.len()
                    || block.len() + line.len() > limit
                {
                    continue;
                }
                if included.insert((index + 1) as u32) {
                    block.push_str(&line);
                    region_bytes += line.len();
                }
            }
        }
        if included.is_empty() || text.len() + block.len() > 160 * 1024 {
            continue;
        }
        text.push_str(&block);
        provided.insert(path.clone(), included);
    }
    if provided.is_empty() {
        return Err(invalid("説明の根拠にできる本文を読み取れませんでした"));
    }
    let hash = digest(text.as_bytes());
    Ok(Context {
        text,
        hash,
        read_files: files.len(),
        excerpted_files: provided.len(),
        provided,
        inventory_files: inventory.len(),
        truncated: snapshot["truncated"] == true,
    })
}
