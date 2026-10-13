use execution_evidence_lab::{execution::*, model::*, recording::*, workspace::*};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    commit: String,
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
impl Fixture {
    fn new() -> Self {
        let directory = new_run_directory(&std::env::temp_dir().join("evidence-execution-tests"))
            .unwrap()
            .0;
        let root = directory.join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.name", "Synthetic test"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/project.git",
            ],
        );
        fs::create_dir_all(root.join("examples/observable-store")).unwrap();
        fs::create_dir_all(root.join("adapters/observation")).unwrap();
        fs::write(
            root.join("adapters/observation/observe.mjs"),
            include_str!("../adapters/observation/observe.mjs"),
        )
        .unwrap();
        fs::write(
            root.join("adapters/observation/observe.py"),
            include_str!("../adapters/observation/observe.py"),
        )
        .unwrap();
        fs::write(
            root.join("examples/observable-store/app.mjs"),
            include_str!("../examples/observable-store/app.mjs"),
        )
        .unwrap();
        fs::write(
            root.join("examples/observable-store/app.py"),
            include_str!("../examples/observable-store/app.py"),
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "synthetic app"]);
        let commit = git(&root, &["rev-parse", "HEAD"]);
        Self {
            directory,
            root,
            commit,
        }
    }
    fn recipe(&self, runtime: Runtime, scenario: &str) -> Recipe {
        let path = if runtime == Runtime::Node {
            "examples/observable-store/app.mjs"
        } else {
            "examples/observable-store/app.py"
        };
        Recipe {
            schema_version: 1,
            check_id: "stored".into(),
            source: CodeReference {
                repository: "example/project".into(),
                commit: self.commit.clone(),
                path: path.into(),
                line: 1,
                end_line: 1,
            },
            runtime,
            entrypoint: path.into(),
            args: vec![scenario.into()],
            input: serde_json::json!({"message":"架空データ"}),
            criterion: Criterion {
                assertion: "stored_matches_input".into(),
                subject: "record-1".into(),
            },
            timeout_ms: 1500,
        }
    }
    fn run(&self, runtime: Runtime, scenario: &str) -> RunResult {
        execute(
            &self.root,
            &self.directory.join("runs"),
            self.recipe(runtime, scenario),
            self.condition(),
            None,
        )
        .unwrap()
    }
    fn condition(&self) -> Condition {
        Condition {
            id: "stored".into(),
            version: 1,
            text: "試行用の保存物を読み出し、入力と一致する".into(),
            origin: "合成の接続試験".into(),
            kind: ConditionKind::Custom,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn both_languages_run_pinned_code_and_distinguish_failure_missing_and_conflict() {
    let fixture = Fixture::new();
    for runtime in [Runtime::Node, Runtime::Python3] {
        for (scenario, expected) in [
            ("normal", Verdict::Satisfied),
            ("failure", Verdict::NotSatisfied),
            ("missing", Verdict::Unknown),
            ("conflict", Verdict::Unknown),
        ] {
            let result = fixture.run(runtime.clone(), scenario);
            assert_eq!(
                result.assessment.verdict, expected,
                "{scenario}: {}",
                result.assessment.reason
            );
            let trace: ExecutionTrace =
                read_json(&result.directory.join("execution.json")).unwrap();
            assert_eq!(trace.recipe.source.commit, fixture.commit);
            assert!(trace.snapshot_unchanged);
            assert_eq!(trace.exit_code, Some(0));
            assert!(result.directory.join("complete.json").exists());
        }
    }
}

#[test]
fn zero_exit_foreign_trace_and_timeout_never_supply_positive_evidence() {
    let fixture = Fixture::new();
    for scenario in [
        "exit-zero",
        "foreign",
        "bad-line",
        "sequence-gap",
        "mutate",
        "too-much-output",
    ] {
        let result = fixture.run(Runtime::Node, scenario);
        assert_eq!(result.assessment.verdict, Verdict::Unknown);
    }
    let mut recipe = fixture.recipe(Runtime::Node, "timeout");
    recipe.timeout_ms = 80;
    let result = execute(
        &fixture.root,
        &fixture.directory.join("runs"),
        recipe,
        fixture.condition(),
        None,
    )
    .unwrap();
    assert_eq!(result.assessment.verdict, Verdict::Unknown);
    let trace: ExecutionTrace = read_json(&result.directory.join("execution.json")).unwrap();
    assert!(trace.timed_out);
}

#[test]
fn dirty_worktree_is_not_executed_and_wrong_origin_and_symlinks_are_rejected() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("examples/observable-store/app.mjs"),
        "throw Error('dirty code must not run')",
    )
    .unwrap();
    assert_eq!(
        fixture.run(Runtime::Node, "normal").assessment.verdict,
        Verdict::Satisfied
    );
    git(
        &fixture.root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/other/project.git",
        ],
    );
    assert!(
        execute(
            &fixture.root,
            &fixture.directory.join("runs"),
            fixture.recipe(Runtime::Node, "normal"),
            fixture.condition(),
            None
        )
        .is_err()
    );
    git(
        &fixture.root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/example/project.git",
        ],
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", fixture.root.join("outside")).unwrap();
        git(&fixture.root, &["add", "outside"]);
        git(&fixture.root, &["commit", "-qm", "symlink"]);
        let mut recipe = fixture.recipe(Runtime::Node, "normal");
        recipe.source.commit = git(&fixture.root, &["rev-parse", "HEAD"]);
        assert!(
            execute(
                &fixture.root,
                &fixture.directory.join("runs"),
                recipe,
                fixture.condition(),
                None
            )
            .is_err()
        );
    }
}

#[test]
fn external_trial_returns_to_its_code_and_retest_preserves_old_evidence() {
    let fixture = Fixture::new();
    let case = fixture.directory.join("case");
    let recipe = fixture.recipe(Runtime::Node, "failure");
    let mut workspace = empty_workspace("external".into(), "外部コードの合成試験".into());
    workspace.checks.push(Check {
        id: "stored".into(),
        version: 1,
        text: fixture.condition().text,
        origin: "合成の仕様".into(),
        source: Some(recipe.source.clone()),
        observation: None,
        rule: Rule::ManualOnly,
        dependencies: vec![],
        questions: vec![],
    });
    initialize(&case, workspace, "Synthetic", "外部試験を始める").unwrap();
    change(
        &case,
        ChangeRequest {
            expected_revision: 1,
            actor: "Synthetic".into(),
            reason: "観測名と対象を対応づける".into(),
            action: Change::ConfigureObservation {
                check_id: "stored".into(),
                criterion: recipe.criterion.clone(),
            },
        },
    )
    .unwrap();
    let failure = run_external(
        &case,
        &fixture.root,
        recipe.clone(),
        "Synthetic".into(),
        "保存前の人工失敗".into(),
    )
    .unwrap();
    assert_eq!(failure.checks[0].verdict, Verdict::NotSatisfied);
    let mut normal = recipe;
    normal.args = vec!["normal".into()];
    let success = run_external(
        &case,
        &fixture.root,
        normal,
        "Synthetic".into(),
        "正常に戻して再試験".into(),
    )
    .unwrap();
    assert_eq!(success.checks[0].verdict, Verdict::Satisfied);
    assert_eq!(success.workspace.evidence.len(), 2);
    assert!(
        success.workspace.evidence[1]
            .manifest
            .previous_run
            .is_some()
    );
    assert_eq!(
        success.workspace.evidence[0]
            .execution
            .as_ref()
            .unwrap()
            .recipe
            .source
            .commit,
        fixture.commit
    );
    assert!(!success.checks[0].evidence[0].applicable); // 起動引数が変わった結果を現在へ補わない。
    let mut changed = success.workspace.evidence[1]
        .execution
        .as_ref()
        .unwrap()
        .recipe
        .source
        .clone();
    changed.commit = "b".repeat(40);
    let stale = change(
        &case,
        ChangeRequest {
            expected_revision: success.workspace.revision,
            actor: "Synthetic".into(),
            reason: "別のコード版を参照する".into(),
            action: Change::BindSource {
                check_id: "stored".into(),
                source: changed,
            },
        },
    )
    .unwrap();
    assert_eq!(stale.checks[0].verdict, Verdict::Unknown);
}

#[cfg(target_os = "linux")]
#[test]
fn sandbox_unavailability_never_falls_back_to_local_execution() {
    let fixture = Fixture::new();
    let result = execute_with_mode(
        &fixture.root,
        &fixture.directory.join("runs"),
        fixture.recipe(Runtime::Node, "normal"),
        fixture.condition(),
        None,
        ExecutionMode::Bubblewrap,
    );
    match result {
        Ok(result) => {
            let trace: ExecutionTrace =
                read_json(&result.directory.join("execution.json")).unwrap();
            assert_eq!(trace.mode, "bubblewrap");
            if trace.exit_code == Some(0) {
                assert_eq!(result.assessment.verdict, Verdict::Satisfied);
            } else {
                assert_eq!(result.assessment.verdict, Verdict::Unknown);
                assert!(!result.directory.join("work/stored.json").exists());
            }
            eprintln!(
                "bubblewrap: mode={}, exit={:?}, verdict={:?}",
                trace.mode, trace.exit_code, result.assessment.verdict
            );
        }
        Err(error) => assert!(error.to_string().contains("bwrap")),
    }
}
