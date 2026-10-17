use execution_evidence_lab::{
    browser::{BrowserSpec, DatabaseRead, ResponseRecord, evaluate, query, validate},
    container::{ContainerPhase, ContainerSpec},
    trial::{CheckMethod, Expectation, Observed, TrialCheck},
};
use serde_json::json;

fn spec() -> BrowserSpec {
    serde_json::from_value(json!({
        "service":"web","tls":false,"start_path":"/","expected_version":"151.0.7922.34",
        "preparation":[{"id":"username","action":"fill","target":{"locator":{"by":"label","text":"利用者"}},"value":{"source":"secret","key":"username"}}],
        "actions":[{"id":"message","action":"read_text","target":{"locator":{"by":"role","role":"heading","name":"完了"}}}],
        "responses":[{"id":"save","method":"POST","path":"/save","contains":"saved"}],
        "database":[{"id":"row","service":"db","database":"trial","table":"patient","match_column":"name","columns":["name"]}]
    })).unwrap()
}
fn containers() -> ContainerSpec {
    serde_json::from_value(json!({"services":(["web","db"].map(|name| json!({"name":name,"image":format!("example@sha256:{}","a".repeat(64)),"role":if name=="db"{"database"}else{"app"},"expose":80,"env":{},"volumes":[],"command":[],"ready":"tcp","ready_timeout_ms":1000})))})).unwrap()
}
fn phase() -> ContainerPhase {
    serde_json::from_value(json!({"engine":"test","network":"test","volumes":[],"services":[],"leftovers":[],"database_query":null,"http":null,"detail":"合成の記録",
        "browser":{"schema_version":1,"run_id":"test","version":"151.0.7922.34","prepared":true,"completed":false,"steps":[{"id":"message","phase":"action","completed":true,"text":"完了","screenshot":null,"detail":"合成"}],"responses":[],"blocked_requests":0,"detail":"合成"},
        "database_reads":[]
    })).unwrap()
}
fn check(method: CheckMethod, expect: Expectation, target: &str) -> TrialCheck {
    TrialCheck {
        id: "test".into(),
        scope: "保存".into(),
        method,
        expect,
        text: Some("test".into()),
        target: Some(target.into()),
    }
}
#[test]
fn validates_declarative_steps_and_rejects_remote_navigation_and_sql_identifiers() {
    let original = spec();
    let containers = containers();
    validate(&original, &containers).unwrap();
    let mut invalid = original.clone();
    invalid.start_path = "https://example.invalid/".into();
    assert!(validate(&invalid, &containers).is_err());
    invalid = original.clone();
    invalid.actions[0].id = "username".into();
    assert!(validate(&invalid, &containers).is_err());
    invalid = original.clone();
    invalid.database[0].table = "patient`; DROP TABLE patient;--".into();
    assert!(validate(&invalid, &containers).is_err());
    let sql = query(&original.database[0], "x'; DELETE FROM patient; --");
    assert!(!sql.contains("DELETE"));
    assert!(sql.contains("CONVERT(0x"));
}
#[test]
fn absent_and_failed_reads_do_not_pass_negative_expectations() {
    let mut p = phase();
    for method in [
        CheckMethod::BrowserText,
        CheckMethod::BrowserResponse,
        CheckMethod::DatabaseRows,
    ] {
        assert_eq!(
            evaluate(
                &check(method, Expectation::NotContains, "missing"),
                Some(&p),
                "test"
            )
            .0,
            Observed::NotObserved
        );
    }
    p.database_reads.push(DatabaseRead {
        id: "row".into(),
        performed: true,
        exit_code: Some(1),
        text: "".into(),
        detail: "読取失敗".into(),
    });
    assert_eq!(
        evaluate(
            &check(CheckMethod::DatabaseRows, Expectation::NotContains, "row"),
            Some(&p),
            "test"
        )
        .0,
        Observed::NotObserved
    );
    p.database_reads[0].exit_code = Some(0);
    assert_eq!(
        evaluate(
            &check(CheckMethod::DatabaseRows, Expectation::NotContains, "row"),
            Some(&p),
            "test"
        )
        .0,
        Observed::Pass
    );
}
#[test]
fn display_does_not_establish_database_success_and_mixed_http_responses_are_unknown() {
    let mut p = phase();
    assert_eq!(
        evaluate(
            &check(CheckMethod::BrowserText, Expectation::Contains, "message"),
            Some(&p),
            "完了"
        )
        .0,
        Observed::Pass
    );
    assert_eq!(
        evaluate(
            &check(CheckMethod::DatabaseRows, Expectation::Contains, "row"),
            Some(&p),
            "完了"
        )
        .0,
        Observed::NotObserved
    );
    let browser = p.browser.as_mut().unwrap();
    browser.responses = vec![
        ResponseRecord {
            watch: "save".into(),
            method: "POST".into(),
            path: "/save".into(),
            status: 200,
            body_match: Some(true),
            request_step: Some("message".into()),
        },
        ResponseRecord {
            watch: "save".into(),
            method: "POST".into(),
            path: "/save".into(),
            status: 500,
            body_match: None,
            request_step: Some("message".into()),
        },
    ];
    assert_eq!(
        evaluate(
            &check(CheckMethod::BrowserResponse, Expectation::Contains, "save"),
            Some(&p),
            "status=200"
        )
        .0,
        Observed::NotObserved
    );
    p.browser.as_mut().unwrap().responses.truncate(1);
    assert_eq!(
        evaluate(
            &check(CheckMethod::BrowserResponse, Expectation::Contains, "save"),
            Some(&p),
            "status=200"
        )
        .0,
        Observed::Pass
    );
    p.browser.as_mut().unwrap().responses[0].request_step = Some("username".into());
    assert_eq!(
        evaluate(
            &check(CheckMethod::BrowserResponse, Expectation::Contains, "save"),
            Some(&p),
            "status=200"
        )
        .0,
        Observed::NotObserved
    );
}

#[test]
fn validates_the_pinned_openemr_recipe_without_running_it() {
    let recipe: execution_evidence_lab::execution::Recipe =
        serde_json::from_str(include_str!("../examples/openemr-browser/recipe.json")).unwrap();
    execution_evidence_lab::execution::validate_recipe(&recipe).unwrap();
    assert_eq!(recipe.browser.as_ref().unwrap().database.len(), 2);
}

#[test]
fn unknown_response_body_never_passes_negative_body_check_and_expressions_are_validated() {
    let mut p = phase();
    p.browser.as_mut().unwrap().responses.push(ResponseRecord {
        watch: "save".into(),
        method: "POST".into(),
        path: "/save".into(),
        status: 200,
        body_match: None,
        request_step: Some("message".into()),
    });
    assert_eq!(
        evaluate(
            &check(
                CheckMethod::BrowserResponse,
                Expectation::NotContains,
                "save"
            ),
            Some(&p),
            "body_match=true"
        )
        .0,
        Observed::NotObserved
    );
    let mut recipe: execution_evidence_lab::execution::Recipe =
        serde_json::from_str(include_str!("../examples/openemr-browser/recipe.json")).unwrap();
    recipe.trial.as_mut().unwrap().checks[1].text = Some("false".into());
    assert!(execution_evidence_lab::execution::validate_recipe(&recipe).is_err());
}
