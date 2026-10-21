use super::*;
fn snapshot() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/reader/system.json")).unwrap()
}
fn graph() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/reader/system-ai.json")).unwrap()
}
#[test]
fn semantic_graph_preserves_domain_roles_actors_subsystems_and_source_evidence() {
    let snapshot = snapshot();
    let context = context::build(&snapshot).unwrap();
    let mut compiled = model::compile(&graph().to_string(), &snapshot, &context).unwrap();
    assert_eq!(compiled["nodes"].as_array().unwrap().len(), 7);
    assert_eq!(compiled["nodes"][0]["role"], "actor");
    assert!(compiled["nodes"][0]["parent"].is_null());
    assert_eq!(compiled["nodes"][2]["title"], "記録管理API");
    assert!(
        compiled["nodes"][2]["technical_description"]
            .as_str()
            .unwrap()
            .contains("空の配列")
    );
    assert_eq!(compiled["edges"][3]["evidence_kind"], "inference");
    assert_eq!(
        crate::understanding::verify_refs(&mut compiled, &snapshot)["unverified"],
        json!([])
    );
}
#[test]
fn unseen_lines_duplicate_ids_wrong_paths_and_disconnected_references_are_rejected() {
    let snapshot = snapshot();
    let context = context::build(&snapshot).unwrap();
    for mutate in 0..6 {
        let mut g = graph();
        match mutate {
            0 => g["nodes"][1]["refs"][0]["line"] = json!(999),
            1 => g["nodes"][1]["id"] = g["nodes"][0]["id"].clone(),
            2 => g["nodes"][1]["path"] = json!("not-present.py"),
            3 => g["edges"][0]["to"] = json!("not_present"),
            4 => g["nodes"][1]["refs"] = json!([]),
            _ => g["nodes"][1]["group_id"] = json!("not_present"),
        }
        assert!(
            model::compile(&g.to_string(), &snapshot, &context).is_err(),
            "case {mutate}"
        );
    }
}
#[test]
fn context_distributes_long_file_excerpts_and_never_authorizes_omitted_lines_or_env() {
    let mut snapshot = snapshot();
    snapshot["files"]["server/main.py"] = json!(
        (0..1000)
            .map(|n| format!("def operation_{n}(): return {n}\n"))
            .collect::<String>()
    );
    snapshot["files"][".env"] = json!("DO_NOT_SEND=synthetic-secret");
    let context = context::build(&snapshot).unwrap();
    assert!(!context.text.contains("synthetic-secret"));
    let lines = &context.provided["server/main.py"];
    assert!(lines.iter().any(|l| *l > 900));
    let missing = (1..=1000).find(|n| !lines.contains(n)).unwrap();
    let mut g = graph();
    g["nodes"][2]["refs"][0] = json!({"path":"server/main.py","line":missing,"end_line":missing});
    assert!(model::compile(&g.to_string(), &snapshot, &context).is_err());
    assert!(context.text.len() <= 160 * 1024);
}
#[test]
fn changes_to_meaning_change_revision_but_keep_shared_component_identity() {
    let snapshot = snapshot();
    let context = context::build(&snapshot).unwrap();
    let mut g = graph();
    let before = model::compile(&g.to_string(), &snapshot, &context).unwrap();
    g["nodes"][2]["operator_description"] =
        json!("保存した記録を一覧で返します。実行は未確認です。");
    let after = model::compile(&g.to_string(), &snapshot, &context).unwrap();
    assert_eq!(before["nodes"][2]["id"], after["nodes"][2]["id"]);
    assert_ne!(before["model_revision"], after["model_revision"]);
}
