use super::*;
fn fixture() -> Snapshot {
    serde_json::from_str::<Snapshot>(include_str!("../../../tests/fixtures/reader/system.json"))
        .unwrap()
        .validate()
        .unwrap()
}
fn nodes(v: &Value) -> &[Value] {
    v["nodes"].as_array().unwrap()
}
fn node<'a>(v: &'a Value, role: &str) -> &'a Value {
    nodes(v).iter().find(|n| n["role"] == role).unwrap()
}
fn edge<'a>(v: &'a Value, from: &Value, to: &Value) -> &'a Value {
    v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["from"] == from["id"] && e["to"] == to["id"])
        .unwrap()
}
#[test]
fn architecture_represents_apps_stores_and_auth_instead_of_files() {
    let s = fixture();
    let graph = build(&s).unwrap();
    assert_eq!(nodes(&graph).len(), 6);
    let api = node(&graph, "api");
    let ui = node(&graph, "ui");
    let db = node(&graph, "database");
    let cache = node(&graph, "cache");
    assert_eq!(edge(&graph, ui, api)["relation_type"], "request");
    assert_eq!(edge(&graph, api, db)["relation_type"], "data");
    assert_eq!(edge(&graph, api, cache)["relation_type"], "startup");
    assert_eq!(node(&graph, "auth")["technology"][0], "Cognito");
    assert_eq!(node(&graph, "storage")["technology"][0], "S3");
    assert_eq!(
        nodes(&graph).iter().filter(|n| n["role"] == "api").count(),
        1
    );
    assert!(!graph.to_string().contains("synthetic-password"));
    assert!(
        !nodes(&graph)
            .iter()
            .any(|n| n["title"].as_str().unwrap().ends_with(".py"))
    );
    assert!(
        api["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["text"].as_str().unwrap().contains("/api/records"))
    );
    let snapshot = serde_json::to_value(&s).unwrap();
    let mut checked = graph.clone();
    assert_eq!(
        crate::understanding::verify_refs(&mut checked, &snapshot)["unverified"],
        json!([])
    );
}
#[test]
fn no_service_is_invented_from_dependencies_comments_or_imports() {
    let mut s = fixture();
    s.files
        .retain(|p, _| p == "server/main.py" || p == "ui/package.json");
    s.files.insert("server/main.py".into(),json!("# app = FastAPI()\ntext = 'boto3.client(\"s3\")'\nimport requests\nfrom other import storage\n"));
    let graph = build(&s).unwrap();
    // 明示されたviteスクリプトだけ。SDKの依存やコメントを外部サービスにしない。
    assert_eq!(nodes(&graph).len(), 1);
    assert_eq!(node(&graph, "ui")["technology"][0], "Web UI");
    assert!(graph["edges"].as_array().unwrap().is_empty());
    s.files.clear();
    assert!(nodes(&build(&s).unwrap()).is_empty());
}
#[test]
fn a_single_program_is_not_split_into_a_box_per_module() {
    let mut s = fixture();
    s.files.clear();
    for (p, t) in [
        ("src/main.rs", "mod storage; fn main() { storage::save(); }"),
        (
            "src/storage.rs",
            "pub fn save() { std::fs::write(\"a.txt\",\"test\"); }",
        ),
        (
            "src/read.rs",
            "pub fn read() { std::fs::read_to_string(\"a.txt\"); }",
        ),
    ] {
        s.inventory.push(snapshot::Entry {
            path: p.into(),
            mode: "100644".into(),
            size: None,
            sha: None,
        });
        s.files.insert(p.into(), json!(t));
    }
    let graph = build(&s).unwrap();
    assert_eq!(nodes(&graph).len(), 2);
    let app = node(&graph, "application");
    let store = node(&graph, "storage");
    assert_eq!(edge(&graph, app, store)["protocol"], "ファイルI/O");
    assert_eq!(store["refs"].as_array().unwrap().len(), 2);
}
#[test]
fn alternate_environments_and_sqlite_fallback_are_not_merged_as_live_services() {
    let mut s = fixture();
    s.inventory.push(snapshot::Entry {
        path: "compose.production.yaml".into(),
        mode: "100644".into(),
        size: None,
        sha: None,
    });
    s.files.insert(
        "compose.production.yaml".into(),
        json!("services:\n  production:\n    image: mongo:8\n"),
    );
    let graph = build(&s).unwrap();
    assert!(graph["scope"].as_str().unwrap().starts_with("compose.yaml"));
    assert_eq!(
        nodes(&graph)
            .iter()
            .filter(|n| n["role"] == "database")
            .count(),
        1
    );
    assert!(!graph.to_string().contains("MongoDB"));
    assert!(
        graph["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("混ぜていません"))
    );
}
#[test]
fn identities_are_stable_across_commits_and_internal_file_moves() {
    let mut s = fixture();
    let before = build(&s).unwrap();
    s.commit = "b".repeat(40);
    let text = s.files.remove("server/files.py").unwrap();
    s.files.insert("server/blob.py".into(), text);
    s.inventory
        .iter_mut()
        .find(|e| e.path == "server/files.py")
        .unwrap()
        .path = "server/blob.py".into();
    let after = build(&s).unwrap();
    let ids = |v: &Value| {
        let mut n = nodes(v).iter().map(|n| n["id"].clone()).collect::<Vec<_>>();
        n.sort_by_key(Value::to_string);
        n
    };
    assert_eq!(ids(&before), ids(&after));
}
#[test]
fn compose_maps_and_lists_and_unknown_connection_variables_are_handled() {
    let mut s = fixture();
    s.files.insert("compose.yaml".into(),json!("services:\n  api:\n    command: uvicorn main:app\n    environment:\n      - DATABASE_URL=postgresql://db/records\n      - SECRET=${NEVER_EXPAND}\n    depends_on:\n      db:\n        condition: service_healthy\n  db:\n    image: postgres:16\n"));
    let graph = build(&s).unwrap();
    assert_eq!(
        edge(&graph, node(&graph, "api"), node(&graph, "database"))["relation_type"],
        "data"
    );
    assert!(!graph.to_string().contains("NEVER_EXPAND"));
    assert!(snapshot::architecture_priority(".env").is_none());
    assert!(snapshot::architecture_priority("compose.yaml").is_some());
    assert!(snapshot::architecture_priority("docs/architecture.md").is_some());
}

fn from_files(files: &[(&str, &str)]) -> Snapshot {
    let mut s = fixture();
    s.files.clear();
    s.inventory.clear();
    for (path, text) in files {
        s.files.insert((*path).into(), json!(text));
        s.inventory.push(snapshot::Entry {
            path: (*path).into(),
            mode: "100644".into(),
            size: Some(text.len()),
            sha: None,
        });
    }
    s
}
#[test]
fn server_rendered_pages_belong_to_the_same_app_and_helper_scripts_do_not_steal_ownership() {
    let s = from_files(&[
        (
            "compose.yaml",
            "services:\n  api:\n    build: .\n    command: uvicorn project.app.main:app\n",
        ),
        (
            "project/app/main.py",
            "app = FastAPI()\n@app.get('/view')\ndef view(): pass\n",
        ),
        ("project/app/infra/storage.py", "target.write_bytes(data)\n"),
        (
            "project/app/templates/view.html",
            "<!doctype html>\n<html></html>\n",
        ),
        (
            "scripts/check.py",
            "#!/usr/bin/env python\nif __name__ == '__main__':\n    print('helper')\n",
        ),
    ]);
    let graph = build(&s).unwrap();
    assert_eq!(nodes(&graph).len(), 3);
    assert_eq!(
        edge(&graph, node(&graph, "ui"), node(&graph, "api"))["relation_type"],
        "request"
    );
    assert_eq!(
        edge(&graph, node(&graph, "api"), node(&graph, "storage"))["relation_type"],
        "data"
    );
}
#[test]
fn independent_apps_are_not_collapsed_and_token_getter_names_do_not_create_external_auth() {
    let s = from_files(&[
        ("services/a/main.py", "app = FastAPI()\n"),
        ("services/b/main.py", "app = Flask(__name__)\n"),
        (
            "ui/main.tsx",
            "createRoot(element);\nfunction getCognitoIdToken() { return localStorage.getItem('token'); }\n",
        ),
        (
            "README.md",
            "# Example\n`src/` は旧構成のプロトタイプです。\n",
        ),
        (
            "src/main.py",
            "if __name__ == '__main__':\n    print('prototype')\n",
        ),
    ]);
    let graph = build(&s).unwrap();
    assert_eq!(
        nodes(&graph).iter().filter(|n| n["role"] == "api").count(),
        2
    );
    assert!(!nodes(&graph).iter().any(|n| n["role"] == "auth"));
    let prototype = node(&graph, "application");
    assert_eq!(prototype["parent"], "reference");
    assert!(
        prototype["scope_note"]
            .as_str()
            .unwrap()
            .contains("別の試作")
    );
}
#[test]
fn mentioning_a_prototype_on_the_same_line_does_not_reclassify_another_app() {
    let s = from_files(&[
        (
            "README.md",
            "# Example\n- `server/` と `src/` が共存しています。`src/` はプロトタイプです。\n- `src/`: ローカルのプロトタイプ\n",
        ),
        ("server/main.py", "app = FastAPI()\n"),
        (
            "src/main.py",
            "if __name__ == '__main__':\n    print('prototype')\n",
        ),
    ]);
    let graph = build(&s).unwrap();
    assert_eq!(node(&graph, "api")["parent"], "application");
    assert_eq!(node(&graph, "application")["parent"], "reference");
}
#[test]
fn a_generic_worker_does_not_take_the_source_of_an_explicit_api() {
    let s = from_files(&[
        (
            "compose.yaml",
            "services:\n  api:\n    build: .\n    command: uvicorn main:app\n  worker:\n    build: .\n    image: example/worker:1\n",
        ),
        ("main.py", "app = FastAPI()\n"),
    ]);
    let graph = build(&s).unwrap();
    assert_eq!(nodes(&graph).len(), 2);
    assert_eq!(node(&graph, "api")["technical_title"], "api");
    assert_eq!(node(&graph, "application")["technical_title"], "worker");
}
