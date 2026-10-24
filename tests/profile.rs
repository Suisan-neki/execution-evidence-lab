mod support;
use mugiurushi::{
    backup,
    model::*,
    profile::{self, Domain},
    recording::new_run_directory,
    review_report,
    workspace::*,
};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Case(PathBuf);
impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn case() -> Case {
    let root = new_run_directory(&std::env::temp_dir().join("eel-profile-tests"))
        .unwrap()
        .0;
    let c = Case(root);
    initialize(
        &c.0.join("case"),
        support::workspace(),
        "合成の作成者",
        "開始",
    )
    .unwrap();
    c
}
fn apply(c: &Case, action: Change) -> WorkspaceView {
    let path = c.0.join("case");
    change(
        &path,
        ChangeRequest {
            expected_revision: load(&path).unwrap().revision,
            actor: "合成の作成者".into(),
            reason: "確認モードの試験".into(),
            action,
        },
    )
    .unwrap()
}

#[test]
fn changing_domain_keeps_success_and_history_but_new_candidates_are_unknown() {
    let c = case();
    let path = c.0.join("case");
    support::import_pass(&path);
    let before = load(&path).unwrap();
    let result = apply(
        &c,
        Change::SelectProfile {
            domain: Domain::Medical,
        },
    );
    assert_eq!(result.workspace.schema_version, 2);
    let mut downgraded = result.workspace.clone();
    downgraded.schema_version = 1;
    assert!(validate(&downgraded).is_err());
    assert_eq!(result.checks[0].verdict, Verdict::Satisfied);
    assert_eq!(
        serde_json::to_value(&before.evidence).unwrap(),
        serde_json::to_value(&result.workspace.evidence).unwrap()
    );
    let result = apply(
        &c,
        Change::AddProfileCheck {
            template_id: "medical-access".into(),
            selection_version: 1,
        },
    );
    assert_eq!(result.checks.last().unwrap().verdict, Verdict::Unknown);
    assert_eq!(
        result.workspace.checks.last().unwrap().rule,
        Rule::ManualOnly
    );
    let selected = apply(
        &c,
        Change::SelectProfile {
            domain: Domain::General,
        },
    );
    assert_eq!(selected.checks[0].verdict, Verdict::Satisfied);
    assert_eq!(selected.checks.last().unwrap().verdict, Verdict::Unknown);
    assert_eq!(selected.workspace.profile_history.len(), 2);
    let before_invalid = serde_json::to_value(&selected.workspace).unwrap();
    let invalid = change(
        &path,
        ChangeRequest {
            expected_revision: selected.workspace.revision,
            actor: "合成".into(),
            reason: "古い設定".into(),
            action: Change::AddProfileCheck {
                template_id: "general-data".into(),
                selection_version: 1,
            },
        },
    );
    assert!(invalid.is_err());
    assert_eq!(
        serde_json::to_value(load(&path).unwrap()).unwrap(),
        before_invalid
    );
    assert!(profile::proposed_check(&selected.workspace, "medical-retention", 2).is_err());
}

#[test]
fn selections_and_evidence_survive_backup_and_cannot_rewrite_earlier_selections() {
    let c = case();
    let path = c.0.join("case");
    apply(
        &c,
        Change::SelectProfile {
            domain: Domain::Medical,
        },
    );
    support::import_pass(&path);
    let original = load(&path).unwrap();
    assert_eq!(
        original.evidence[0].review_profile.as_ref().unwrap().domain,
        Domain::Medical
    );
    apply(
        &c,
        Change::SelectProfile {
            domain: Domain::General,
        },
    );
    let snapshot = backup::snapshot(&path).unwrap();
    let restored = backup::restore(&c.0.join("restored"), &snapshot).unwrap();
    assert_eq!(
        restored.workspace.evidence[0].review_profile,
        original.evidence[0].review_profile
    );
    let mut tampered = snapshot;
    tampered.revisions.last_mut().unwrap().profile_history[0].domain = Domain::General;
    assert!(backup::validate(&tampered).is_err());
}

#[test]
fn legacy_records_load_without_inventing_an_explicit_profile() {
    let mut value = serde_json::to_value(support::workspace()).unwrap();
    value.as_object_mut().unwrap().remove("profile_history");
    let old: Workspace = serde_json::from_value(value).unwrap();
    validate(&old).unwrap();
    assert_eq!(profile::current(&old), (Domain::General, 0));
    assert!(old.profile_history.is_empty());
}

#[test]
fn report_keeps_unknown_stale_results_and_escapes_user_text_without_raw_logs() {
    let c = case();
    let path = c.0.join("case");
    let v = support::import_pass(&path);
    change(
        &path,
        ChangeRequest {
            expected_revision: v.workspace.revision,
            actor: "合成".into(),
            reason: "状態変更".into(),
            action: Change::SetContext {
                key: "environment".into(),
                value: "changed".into(),
            },
        },
    )
    .unwrap();
    let mut w = load(&path).unwrap();
    w.title = "<script>alert('x')</script>".into();
    let report = review_report::build(w).unwrap();
    assert_eq!(report.checks[0].current.verdict, Verdict::Unknown);
    assert!(!report.checks[0].current.evidence[0].applicable);
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(
        value["checks"][0]["current"]["evidence"][0]["observed"]["verdict"],
        "satisfied"
    );
    assert!(
        !value["checks"][0]["current"]["evidence"][0]["invalidated_by"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let html = review_report::html(&report).unwrap();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("再確認が必要"));
    assert!(value.get("events").is_none());
    assert!(value["checks"][0]["references"][0].get("events").is_none());
    assert_eq!(value["selection_version"], json!(0));
}
