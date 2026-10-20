//! 読んだ対象の構成候補。ファイル間の参照を実行時の通信と取り違えない。
use super::{
    analysis::Model,
    overview,
    snapshot::{self, Snapshot},
};
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn build(snapshot: &Snapshot, model: &Model<'_>) -> Value {
    let mut files: Vec<_> = model
        .files
        .iter()
        .filter(|f| snapshot::source(&f.path) && !snapshot::test_file(&f.path))
        .collect();
    files.sort_by_key(|f| {
        (
            !model.entries.iter().any(|e| e["path"] == f.path),
            f.path.clone(),
        )
    });
    let total = files.len();
    files.truncate(18);
    let selected: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let nodes: Vec<Value> = files.iter().map(|f| {
        let effects = overview::effects(f);
        let has = |kind: &str| effects.iter().any(|e| e["kind"] == kind);
        let mut roles = Vec::new();
        if has("input") || has("env") { roles.push("入力"); }
        if has("read") || has("write") || has("delete") { roles.push("データの読み書き"); }
        if has("send") { roles.push("外部との通信"); }
        if has("output") { roles.push("結果の表示"); }
        let entry = model.entries.iter().find(|e| e["path"] == f.path);
        let title = if roles.is_empty() {
            if entry.is_some() { "処理の入口".to_owned() } else { "アプリ内の処理".to_owned() }
        } else { roles.join("・") };
        let mut summaries = Vec::new();
        for effect in &effects {
            if let Some(label) = effect["label"].as_str()
                && !summaries.iter().any(|s: &String| s == label)
            { summaries.push(label.to_owned()); }
        }
        let line = entry.and_then(|e| e["line"].as_u64()).unwrap_or(1);
        json!({"id":f.path,"path":f.path,"title":title,
            "description":if summaries.is_empty() { "このファイルの役割は、内容を読んで確認する必要があります。".to_owned() } else { format!("{}に当たる記述があります。", summaries.join("、")) },
            "entry":entry.is_some(),"kind":"inference",
            "refs":[overview::reference(&f.path,line as usize,line as usize)],
            "effects":effects.iter().take(20).collect::<Vec<_>>()})
    }).collect();
    let mut seen = HashSet::new();
    let edges: Vec<Value> = files
        .iter()
        .flat_map(|f| &f.imports)
        .filter(|e| e.from != e.to && selected.contains(e.to.as_str()))
        .filter(|e| seen.insert((e.from.clone(), e.to.clone())))
        .map(|e| {
            json!({"from":e.from,"to":e.to,"label":"参照宣言","kind":"code",
            "refs":[overview::line_ref(&e.from,e.line)]})
        })
        .collect();
    let unread = snapshot
        .inventory
        .iter()
        .filter(|f| !snapshot.files.contains_key(&f.path))
        .count();
    json!({"repository":snapshot.full_name,"commit":snapshot.commit,"nodes":nodes,"edges":edges,
        "read_files":snapshot.files.len(),"unread_files":unread,"omitted_nodes":total.saturating_sub(18),
        "inventory_truncated":snapshot.truncated,
        "basis":"読んだコードの入出力と参照宣言から作った構成の候補です。線はコード間の参照で、通信や実行順序を表しません。",
        "unknown":"未読のコード、実際のサーバー配置、外部サービスの構成は含まれないことがあります。実行時の動作は検証して確認します。"})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_uses_only_the_selected_repository_and_read_references() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/reader/navigation.json"))
                .unwrap();
        let snapshot: Snapshot = serde_json::from_value(fixture["snapshot"].clone()).unwrap();
        let budget = super::super::budget::Budget::new();
        let model = super::super::analysis::analyze(&snapshot, &budget);
        let map = build(&snapshot, &model);
        assert_eq!(map["repository"], "example/project");
        let nodes = map["nodes"].as_array().unwrap();
        assert!(nodes.iter().any(|n| n["id"] == "src/main.rs"));
        for node in nodes {
            assert!(snapshot.files.contains_key(node["path"].as_str().unwrap()));
            assert!(!node.to_string().contains("MONAI"));
        }
        assert!(
            map["edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["from"] == "src/main.rs" && e["to"] == "src/storage.rs")
        );
        assert!(
            map["basis"]
                .as_str()
                .unwrap()
                .contains("通信や実行順序を表しません")
        );
    }
}
