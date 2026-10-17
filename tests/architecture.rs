use execution_evidence_lab::{architecture::*, backup, execution::Recipe, model::*, workspace::*};
use serde_json::json;
use std::collections::BTreeMap;

fn workspace() -> Workspace {
    let mut w = empty_workspace("architecture-test".into(), "合成の対応づけ試験".into());
    let m = openemr();
    w.checks.push(Check {
        id: "openemr".into(),
        version: 1,
        text: "合成の条件".into(),
        origin: "試験".into(),
        source: Some(m.source.clone()),
        observation: None,
        rule: Rule::ManualOnly,
        dependencies: vec![],
        questions: vec![],
    });
    w.architecture_models.push(m);
    w
}
fn link(target: Target) -> Link {
    Link {
        id: "link-1".into(),
        model_id: "openemr-trial".into(),
        model_version: 1,
        object_id: "db".into(),
        target,
    }
}
#[test]
fn rejects_cycles_missing_endpoints_and_reused_model_versions() {
    let original = openemr();
    original.validate().unwrap();
    let mut m = original.clone();
    m.elements[2].parent = Some("system".into());
    assert!(m.validate().is_err());
    m = original.clone();
    m.relations[0].to = "absent".into();
    assert!(m.validate().is_err());
    m = original.clone();
    m.elements[0].id = m.relations[0].id.clone();
    assert!(m.validate().is_err());
    let mut w = workspace();
    w.architecture_models.push(original);
    assert!(execution_evidence_lab::architecture::validate(&w).is_err());
}
#[test]
fn preserves_previous_model_links_and_marks_changed_conditions() {
    let mut w = workspace();
    w.architecture_links.push(link(Target::Check {
        check_id: "openemr".into(),
        check_version: 1,
    }));
    execution_evidence_lab::architecture::validate(&w).unwrap();
    let before = serde_json::to_value(&w.architecture_links).unwrap();
    assert!(links(&w)[0].applicable);
    assert!(links(&w)[0].current_model);
    let mut next = w.architecture_models[0].clone();
    next.version = 2;
    next.elements[0].title = "新版の説明".into();
    w.architecture_models.push(next);
    w.checks[0].version = 2;
    execution_evidence_lab::architecture::validate(&w).unwrap();
    assert!(!links(&w)[0].applicable);
    assert!(!links(&w)[0].current_model);
    assert_eq!(serde_json::to_value(&w.architecture_links).unwrap(), before);
    w.architecture_links[0].object_id = "unknown".into();
    assert!(execution_evidence_lab::architecture::validate(&w).is_err());
}
/// 対応づけの単体試験用の合成記録。OpenEMRを実行した証拠には使わない。
fn synthetic_evidence() -> Evidence {
    let recipe: Recipe =
        serde_json::from_str(include_str!("../examples/openemr-browser/recipe.json")).unwrap();
    let correlation = Correlation {
        run_id: "synthetic-run".into(),
        record_id: "synthetic-record".into(),
        attempt_id: "synthetic-attempt".into(),
    };
    let manifest = execution_evidence_lab::experiment::make_manifest(
        correlation.clone(),
        Condition {
            id: "openemr".into(),
            version: 1,
            text: "合成の条件".into(),
            origin: "試験".into(),
            kind: ConditionKind::Custom,
        },
    )
    .unwrap();
    let execution=serde_json::from_value(json!({"schema_version":1,"correlation":correlation,"recipe":recipe,"tree":"a".repeat(40),"files":[],"runtime_path":"synthetic","runtime_version":"synthetic","exit_code":0,"timed_out":false,"output_limit":false,"snapshot_unchanged":true,"observations":[],"diagnostics":[],"mode":"local","stdout_excerpt":"","stderr_excerpt":"","trial":{"test_value":"synthetic","fault":"database_stopped","fault_applied":"合成","fault_effective":true,"readback":null,"state":null,"results":[{"id":"visit-http","observed":"fail","detail":"HTTP失敗の合成記録"},{"id":"visit-row","observed":"not_observed","detail":"DBの行は未観測"}]}})).unwrap();
    Evidence {
        id: "evidence-1".into(),
        check_id: "openemr".into(),
        check_version: 1,
        rule: Rule::ManualOnly,
        source: Some(recipe.source.clone()),
        observation: None,
        execution: Some(execution),
        reported: None,
        dependencies: BTreeMap::new(),
        dependency_origins: BTreeMap::new(),
        manifest,
        events: vec![],
        adapter: None,
    }
}
#[test]
fn does_not_spread_http_failure_or_old_success_to_missing_database_observation() {
    let mut w = workspace();
    w.evidence.push(synthetic_evidence());
    w.architecture_links.push(link(Target::Evidence {
        evidence_id: "evidence-1".into(),
        observation_id: "visit-row".into(),
    }));
    let mut http = link(Target::Evidence {
        evidence_id: "evidence-1".into(),
        observation_id: "visit-http".into(),
    });
    http.id = "http".into();
    http.object_id = "https".into();
    w.architecture_links.push(http);
    execution_evidence_lab::architecture::validate(&w).unwrap();
    assert_eq!(
        links(&w)[0].observed,
        Some(execution_evidence_lab::trial::Observed::NotObserved)
    );
    assert_eq!(
        links(&w)[0].method,
        Some(execution_evidence_lab::trial::CheckMethod::DatabaseRows)
    );
    assert_eq!(
        links(&w)[1].observed,
        Some(execution_evidence_lab::trial::Observed::Fail)
    );
    w.evidence[0]
        .execution
        .as_mut()
        .unwrap()
        .trial
        .as_mut()
        .unwrap()
        .results
        .retain(|r| r.id != "visit-row");
    assert_eq!(links(&w)[0].observed, None);
    w.evidence[0]
        .execution
        .as_mut()
        .unwrap()
        .recipe
        .containers
        .as_mut()
        .unwrap()
        .services[0]
        .image = format!("mariadb@sha256:{}", "b".repeat(64));
    assert!(execution_evidence_lab::architecture::validate(&w).is_err());
}
#[test]
fn backup_rejects_rewriting_or_removing_previous_architecture() {
    let dir = execution_evidence_lab::recording::new_run_directory(
        &std::env::temp_dir().join("eel-architecture"),
    )
    .unwrap()
    .0;
    let case = dir.join("case");
    initialize(
        &case,
        empty_workspace("case".into(), "構成の履歴".into()),
        "合成",
        "試験",
    )
    .unwrap();
    change(
        &case,
        ChangeRequest {
            expected_revision: 1,
            actor: "合成".into(),
            reason: "構成を追加".into(),
            action: Change::AddArchitectureModel { model: openemr() },
        },
    )
    .unwrap();
    let mut next = openemr();
    next.version = 2;
    change(
        &case,
        ChangeRequest {
            expected_revision: 2,
            actor: "合成".into(),
            reason: "新版を追加".into(),
            action: Change::AddArchitectureModel { model: next },
        },
    )
    .unwrap();
    let saved = backup::snapshot(&case).unwrap();
    backup::validate(&saved).unwrap();
    let mut changed = serde_json::to_value(&saved).unwrap();
    changed["revisions"][2]["architecture_models"][0]["title"] = json!("過去の構成を書換");
    assert!(backup::validate(&serde_json::from_value(changed).unwrap()).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
