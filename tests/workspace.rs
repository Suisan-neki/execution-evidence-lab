use execution_evidence_lab::{
    evaluation, experiment::run_trial, model::*, recording::*, review, workspace::*,
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    thread,
};

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        Self(
            new_run_directory(&std::env::temp_dir().join("evidence-workspace-tests"))
                .unwrap()
                .0,
        )
    }
    fn case(&self) -> PathBuf {
        let path = self.0.join("case");
        initialize(&path, demo_workspace(), "Codex", "合成データでの検証用").unwrap();
        path
    }
    fn trial(&self, fault: Fault) -> RunResult {
        run_trial(TrialConfig {
            output: self.0.join("runs"),
            data: Data {
                name: "架空の利用者".into(),
                age: 30,
            },
            fault,
            previous_run: None,
        })
        .unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn apply(path: &std::path::Path, action: Change) -> WorkspaceView {
    change(
        path,
        ChangeRequest {
            expected_revision: load(path).unwrap().revision,
            actor: "試験用の記入者".into(),
            reason: "合成条件の確認".into(),
            action,
        },
    )
    .unwrap()
}
fn import(path: &std::path::Path, run: &RunResult, check_id: &str) -> WorkspaceView {
    apply(
        path,
        Change::Import {
            check_id: check_id.into(),
            run_directory: run.directory.to_string_lossy().into_owned(),
            use_run_context: true,
        },
    )
}

#[test]
fn adding_questions_and_changing_meaning_requires_new_answers() {
    let s = Sandbox::new();
    let case = s.case();
    import(&case, &s.trial(Fault::Normal), "save");
    let v = apply(
        &case,
        Change::AddQuestion {
            check_id: "save".into(),
            question: Question {
                id: "scope".into(),
                text: "どの範囲か".into(),
                answer: None,
                origin: None,
            },
        },
    );
    assert_eq!(v.checks[0].verdict, Verdict::Unknown);
    assert_eq!(v.checks[0].unanswered.len(), 1);
    assert_eq!(v.checks[0].unanswered[0].text, "どの範囲か");
    apply(
        &case,
        Change::Answer {
            check_id: "save".into(),
            question_id: "scope".into(),
            answer: "ローカル試行のみ".into(),
            origin: "試験用の回答".into(),
        },
    );
    assert_eq!(
        import(&case, &s.trial(Fault::Normal), "save").checks[0].verdict,
        Verdict::Satisfied
    );
    let v = apply(
        &case,
        Change::ReviseCheck {
            check_id: "save".into(),
            text: "翌日に閲覧できる".into(),
            origin: "試験用の別条件".into(),
            rule: Rule::ManualOnly,
            dependencies: vec![],
        },
    );
    assert_eq!(v.checks[0].verdict, Verdict::Unknown);
    assert!(v.workspace.checks[0].questions[0].answer.is_none());
    assert!(v.workspace.checks[0].questions[0].origin.is_none());
}

// 判定器の入力契約を調べる合成記録。実アダプター実行の証拠とは扱わない。
fn adapter_fixture(s: &Sandbox, revision: &str, kinds: &[(&str, serde_json::Value)]) -> RunResult {
    let mut run = s.trial(Fault::Normal);
    run.manifest.mode = "capture".into();
    run.manifest.command = vec!["synthetic-adapter-fixture".into()];
    run.manifest.condition.kind = ConditionKind::Custom;
    fs::write(
        run.directory.join("manifest.json"),
        serde_json::to_vec(&run.manifest).unwrap(),
    )
    .unwrap();
    let registry: serde_json::Value =
        serde_json::from_str(include_str!("../adapters/vitalsensing-targets.json")).unwrap();
    let target = serde_json::json!({ "correlation": run.manifest.correlation, "repository": "Medliss-share/vitalsensing", "revision": revision, "expectedBlobs": registry[revision], "adapterSha256": "a".repeat(64), "node": "test-only", "fault": "synthetic" });
    fs::write(
        run.directory.join("target.json"),
        serde_json::to_vec(&target).unwrap(),
    )
    .unwrap();
    let events = kinds
        .iter()
        .enumerate()
        .map(|(i, (kind, detail))| {
            serde_json::to_string(&AdapterEvent {
                sequence: i as u64 + 1,
                correlation: run.manifest.correlation.clone(),
                kind: (*kind).into(),
                detail: detail.clone(),
            })
            .unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(run.directory.join("adapter-events.jsonl"), events).unwrap();
    run
}

fn add_local_check(case: &std::path::Path) {
    apply(
        case,
        Change::AddCheck {
            check: Check {
                id: "local-index".into(),
                version: 1,
                text: LOCAL_INDEX_CONDITION.into(),
                origin: "合成契約試験".into(),
                rule: Rule::LocalIndexUploaded,
                dependencies: [
                    "engine",
                    "environment",
                    "target",
                    "adapter",
                    "fault",
                    "command",
                ]
                .map(String::from)
                .to_vec(),
                questions: vec![],
            },
        },
    );
}

#[test]
fn adapter_requires_matching_key_and_pinned_target_and_keeps_conflicts_unknown() {
    let s = Sandbox::new();
    let case = s.case();
    add_local_check(&case);
    let old = "218329b7a1409539c6c581990bbbb4ed9dd4175c";
    let current = "2bc818b2a8e7002154ab5c29524bb03315f00b42";
    let issued = (
        "upload_target_issued",
        serde_json::json!({"key":"expected-key", "statusCode":200}),
    );
    let positive = (
        "index_updated",
        serde_json::json!({"key":"expected-key", "status":"uploaded"}),
    );
    let foreign = (
        "index_updated",
        serde_json::json!({"key":"other-key", "status":"uploaded"}),
    );
    let negative = (
        "index_failed",
        serde_json::json!({"key":"expected-key", "reason":"synthetic failure"}),
    );
    for revision in [old, current] {
        let run = adapter_fixture(&s, revision, &[issued.clone(), positive.clone()]);
        let v = import(&case, &run, "local-index");
        assert_eq!(v.checks[2].verdict, Verdict::Satisfied);
        assert_eq!(v.checks[1].verdict, Verdict::Unknown);
    }
    for kinds in [
        vec![issued.clone(), foreign],
        vec![positive.clone()],
        vec![issued.clone(), positive, negative],
    ] {
        let run = adapter_fixture(&s, current, &kinds);
        assert_eq!(
            import(&case, &run, "local-index").checks[2].verdict,
            Verdict::Unknown
        );
    }
    let run = adapter_fixture(&s, current, &[issued]);
    let mut target: serde_json::Value = read_json(&run.directory.join("target.json")).unwrap();
    target["expectedBlobs"]["src/handler.mjs"] = "wrong-version".into();
    fs::write(
        run.directory.join("target.json"),
        serde_json::to_vec(&target).unwrap(),
    )
    .unwrap();
    assert!(
        change(
            &case,
            ChangeRequest {
                expected_revision: load(&case).unwrap().revision,
                actor: "test".into(),
                reason: "bad pinned file".into(),
                action: Change::Import {
                    check_id: "local-index".into(),
                    run_directory: run.directory.to_string_lossy().into_owned(),
                    use_run_context: true
                }
            }
        )
        .is_err()
    );
}

#[test]
fn unasked_business_conditions_are_unknown_and_do_not_inherit_storage_success() {
    let s = Sandbox::new();
    let case = s.case();
    let run = s.trial(Fault::Normal);
    fs::write(run.directory.join("truth.json"), "THIS MUST NOT BE READ").unwrap();
    let v = import(&case, &run, "save");
    assert_eq!(v.checks[0].verdict, Verdict::Satisfied);
    assert_eq!(v.checks[1].verdict, Verdict::Unknown);
    assert_eq!(v.checks[1].unanswered.len(), 3);
    assert!(matches!(v.checks[1].next.kind, ActionKind::Ask));
    let v = import(&case, &run, "later-view");
    assert_eq!(v.checks[1].verdict, Verdict::Unknown);
    assert!(
        !serde_json::to_string(&v)
            .unwrap()
            .contains("THIS MUST NOT BE READ")
    );
    for id in ["viewer", "record", "timing"] {
        apply(
            &case,
            Change::Answer {
                check_id: "later-view".into(),
                question_id: id.into(),
                answer: "合成回答".into(),
                origin: "テスト用、実際の運用担当の発言ではない".into(),
            },
        );
    }
    let v = view(load(&case).unwrap());
    assert_eq!(v.checks[1].verdict, Verdict::Unknown);
    assert!(v.checks[1].unanswered.is_empty());
}

#[test]
fn latest_missing_observation_does_not_fall_back_to_an_older_success() {
    let s = Sandbox::new();
    let case = s.case();
    import(&case, &s.trial(Fault::Normal), "save");
    let v = import(&case, &s.trial(Fault::OmitSaveObservation), "save");
    assert_eq!(v.checks[0].verdict, Verdict::Unknown);
    assert_eq!(v.checks[0].selected_evidence.as_deref(), Some("evidence-2"));
    assert_eq!(v.checks[0].evidence[0].observed.verdict, Verdict::Satisfied);
    assert!(!v.checks[0].evidence[0].applicable);
    assert!(matches!(v.checks[0].next.kind, ActionKind::AddObservation));
}

#[test]
fn context_change_invalidates_only_declared_dependents_and_retest_restores_them() {
    let s = Sandbox::new();
    let case = s.case();
    let run = s.trial(Fault::Normal);
    apply(
        &case,
        Change::SetContext {
            key: "configuration".into(),
            value: "v1".into(),
        },
    );
    let mut extra = demo_workspace().checks[0].clone();
    extra.id = "configured-save".into();
    extra.dependencies.push("configuration".into());
    apply(&case, Change::AddCheck { check: extra });
    import(&case, &run, "save");
    import(&case, &run, "configured-save");
    let before = view(load(&case).unwrap());
    assert_eq!(before.checks[2].verdict, Verdict::Satisfied);
    assert!(before.workspace.evidence[1].dependency_origins["configuration"].contains("記入者"));
    let v = apply(
        &case,
        Change::SetContext {
            key: "configuration".into(),
            value: "v2".into(),
        },
    );
    assert_eq!(v.checks[0].verdict, Verdict::Satisfied);
    assert_eq!(v.checks[2].verdict, Verdict::Unknown);
    assert!(v.checks[2].reason.contains("configuration"));
    assert!(matches!(v.checks[2].next.kind, ActionKind::Retest));
    let v = import(&case, &s.trial(Fault::Normal), "configured-save");
    assert_eq!(v.checks[2].verdict, Verdict::Satisfied);
    assert!(!v.checks[2].evidence[0].applicable);
}

#[test]
fn rule_cannot_claim_business_readability_from_demo_save_or_exit_status() {
    let s = Sandbox::new();
    let case = s.case();
    let original = load(&case).unwrap();
    let request = ChangeRequest {
        expected_revision: 1,
        actor: "テスト".into(),
        reason: "異なる意味の条件への変更".into(),
        action: Change::ReviseCheck {
            check_id: "save".into(),
            text: "管理者が後で閲覧できる".into(),
            origin: "仮説".into(),
            rule: Rule::DemoSave,
            dependencies: original.checks[0].dependencies.clone(),
        },
    };
    assert!(change(&case, request).is_err());
    assert_eq!(load(&case).unwrap().revision, 1);
    let mut invalid = original.checks[0].clone();
    invalid.id = "missing-dependencies".into();
    invalid.dependencies.clear();
    assert!(
        change(
            &case,
            ChangeRequest {
                expected_revision: 1,
                actor: "テスト".into(),
                reason: "依存先なし".into(),
                action: Change::AddCheck { check: invalid }
            }
        )
        .is_err()
    );
}

#[test]
fn imported_snapshot_is_immutable_and_foreign_events_cannot_be_evidence() {
    let s = Sandbox::new();
    let case = s.case();
    let run = s.trial(Fault::Normal);
    import(&case, &run, "save");
    let mut events = load_events(&run.directory).unwrap();
    for event in &mut events {
        event.correlation.record_id = "different-record".into();
    }
    let lines = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(run.directory.join("events.jsonl"), lines).unwrap();
    assert_eq!(
        view(load(&case).unwrap()).checks[0].verdict,
        Verdict::Satisfied
    );
    let v = import(&case, &run, "save");
    assert_eq!(v.checks[0].verdict, Verdict::Unknown);
    assert_eq!(v.workspace.evidence.len(), 2);
}

#[test]
fn conditions_revision_and_decisions_preserve_the_basis_at_that_time() {
    let s = Sandbox::new();
    let case = s.case();
    import(&case, &s.trial(Fault::Normal), "save");
    let v = apply(
        &case,
        Change::Decide {
            role: Role::Developer,
            action: DecisionKind::Hold,
            check_ids: vec!["save".into(), "later-view".into()],
            change_reference: Some("PR #110のレビュー待ち".into()),
            confidence: Some(2),
            elapsed_ms: Some(12_000),
        },
    );
    assert_eq!(
        v.workspace.decisions[0].basis[0].verdict,
        Verdict::Satisfied
    );
    assert_eq!(v.workspace.decisions[0].basis[1].verdict, Verdict::Unknown);
    let old_revision = v.workspace.decisions[0].based_on_revision;
    let c = v.workspace.checks[0].clone();
    let v = apply(
        &case,
        Change::ReviseCheck {
            check_id: c.id,
            text: c.text,
            origin: "条件の出所を修正した合成例".into(),
            rule: c.rule,
            dependencies: c.dependencies,
        },
    );
    assert_eq!(v.checks[0].verdict, Verdict::Unknown);
    assert!(v.checks[0].reason.contains("版"));
    assert_eq!(v.workspace.decisions[0].based_on_revision, old_revision);
    assert_eq!(
        v.workspace.decisions[0].basis[0].verdict,
        Verdict::Satisfied
    );
    assert!(case.join("revisions/00000000000000000001.json").exists());
}

#[test]
fn stale_write_duplicate_id_incomplete_run_and_partial_revision_are_rejected() {
    let s = Sandbox::new();
    let case = s.case();
    let run = s.trial(Fault::Normal);
    let action = || Change::SetContext {
        key: "configuration".into(),
        value: "v1".into(),
    };
    apply(&case, action());
    assert_eq!(
        change(
            &case,
            ChangeRequest {
                expected_revision: 1,
                actor: "テスト".into(),
                reason: "古い画面".into(),
                action: action()
            }
        )
        .unwrap_err()
        .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    fs::write(
        case.join("revisions/00000000000000000003.pending"),
        "partial JSON",
    )
    .unwrap();
    assert_eq!(load(&case).unwrap().revision, 2);
    fs::remove_file(run.directory.join("complete.json")).unwrap();
    assert!(
        change(
            &case,
            ChangeRequest {
                expected_revision: 2,
                actor: "テスト".into(),
                reason: "未完了の試行".into(),
                action: Change::Import {
                    check_id: "save".into(),
                    run_directory: run.directory.to_string_lossy().into_owned(),
                    use_run_context: true
                }
            }
        )
        .is_err()
    );
    assert_eq!(load(&case).unwrap().revision, 2);
    let c = demo_workspace().checks[0].clone();
    assert!(
        change(
            &case,
            ChangeRequest {
                expected_revision: 2,
                actor: "テスト".into(),
                reason: "重複ID".into(),
                action: Change::AddCheck { check: c }
            }
        )
        .is_err()
    );
    assert!(!case.join("write.lock").exists());
}

#[test]
fn comparison_variants_have_identical_raw_information_and_no_answer_key() {
    let s = Sandbox::new();
    let case = s.case();
    let run = s.trial(Fault::OmitSaveObservation);
    fs::write(run.directory.join("truth.json"), "SECRET ANSWER KEY").unwrap();
    fs::write(run.directory.join("stored.json"), "SECRET SAVED CONTENT").unwrap();
    import(&case, &run, "save");
    let output = s.0.join("comparison");
    evaluation::export(&case, &output).unwrap();
    let sources: serde_json::Value = read_json(&output.join("input.json")).unwrap();
    let marker = serde_json::to_string_pretty(&sources).unwrap();
    for name in ["baseline.md", "linked.md", "without-links.md"] {
        let text = fs::read_to_string(output.join(name)).unwrap();
        assert!(text.contains(&marker));
        assert!(!text.contains("SECRET"));
    }
    let result: serde_json::Value = read_json(&output.join("export.json")).unwrap();
    assert_eq!(result["human_evaluation"], "not_conducted");
    assert_eq!(
        read_json::<serde_json::Value>(&output.join("response-template.json")).unwrap()["participant_id"],
        serde_json::Value::Null
    );
    assert!(evaluation::export(&case, &output).is_err());
}

fn http(
    path: &std::path::Path,
    method: &str,
    route: &str,
    body: &str,
    origin: Option<&str>,
    token: Option<&str>,
    host_override: Option<&str>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let case = path.to_owned();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        review::handle_connection(stream, &case, "test-token").unwrap();
    });
    let mut stream = TcpStream::connect(address).unwrap();
    let host = host_override
        .map(String::from)
        .unwrap_or_else(|| address.to_string());
    let origin = origin.map(|v| {
        if v == "same" {
            format!("http://{address}")
        } else {
            v.into()
        }
    });
    write!(stream,"{method} {route} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",body.len()).unwrap();
    if let Some(origin) = origin {
        write!(stream, "Origin: {origin}\r\n").unwrap();
    }
    if let Some(token) = token {
        write!(stream, "X-Review-Token: {token}\r\n").unwrap();
    }
    write!(stream, "\r\n{body}").unwrap();
    let mut result = String::new();
    stream.read_to_string(&mut result).unwrap();
    server.join().unwrap();
    result
}

#[test]
fn review_http_rejects_cross_origin_missing_token_import_and_stale_update() {
    let s = Sandbox::new();
    let case = s.case();
    let body = serde_json::to_string(&ChangeRequest {
        expected_revision: 1,
        actor: "テスト".into(),
        reason: "画面の動作確認".into(),
        action: Change::SetContext {
            key: "configuration".into(),
            value: "v1".into(),
        },
    })
    .unwrap();
    for (origin, token) in [
        (Some("https://unrelated.invalid"), Some("test-token")),
        (Some("same"), None),
        (None, Some("test-token")),
    ] {
        assert!(
            http(&case, "POST", "/api/change", &body, origin, token, None)
                .starts_with("HTTP/1.1 403")
        );
    }
    assert!(
        http(
            &case,
            "GET",
            "/api/case",
            "",
            None,
            None,
            Some("unrelated.invalid")
        )
        .starts_with("HTTP/1.1 403")
    );
    assert_eq!(load(&case).unwrap().revision, 1);
    assert!(
        http(
            &case,
            "POST",
            "/api/change",
            &body,
            Some("same"),
            Some("test-token"),
            None
        )
        .starts_with("HTTP/1.1 200")
    );
    assert!(
        http(
            &case,
            "POST",
            "/api/change",
            &body,
            Some("same"),
            Some("test-token"),
            None
        )
        .starts_with("HTTP/1.1 409")
    );
    let importing = serde_json::to_string(&ChangeRequest {
        expected_revision: 2,
        actor: "テスト".into(),
        reason: "任意ファイル取り込みの防止".into(),
        action: Change::Import {
            check_id: "save".into(),
            run_directory: "/not-readable-over-http".into(),
            use_run_context: true,
        },
    })
    .unwrap();
    assert!(
        http(
            &case,
            "POST",
            "/api/change",
            &importing,
            Some("same"),
            Some("test-token"),
            None
        )
        .starts_with("HTTP/1.1 403")
    );
    assert_eq!(load(&case).unwrap().revision, 2);
    assert!(http(&case, "GET", "/api/case", "", None, None, None).contains("事例"));
}
