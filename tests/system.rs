use execution_evidence_lab::{
    assessment::{assess, compare_runs},
    capture::{CaptureConfig, capture},
    experiment::{run_trial, verify_storage},
    model::*,
    protocol::{MAX_FRAME_BYTES, read_frame, write_frame},
    recording::*,
};
use std::{
    fs,
    io::{self, Cursor},
    path::PathBuf,
    process::Command,
};

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        Self(
            new_run_directory(&std::env::temp_dir().join("execution-evidence-lab-tests"))
                .expect("temporary directory")
                .0,
        )
    }
    fn trial(&self, fault: Fault) -> RunResult {
        run_trial(TrialConfig {
            output: self.0.clone(),
            data: sample(),
            fault,
            previous_run: None,
        })
        .expect("complete trial")
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn sample() -> Data {
    Data {
        name: "架空の利用者\"改行\n".into(),
        age: 30,
    }
}
fn has(events: &[Event], kind: EventKind) -> bool {
    events.iter().any(|event| event.kind == kind)
}

#[test]
fn normal_tcp_trial_correlates_every_event_and_matches_stored_content() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::Normal);
    let events = load_events(&run.directory).unwrap();
    assert_eq!(run.assessment.verdict, Verdict::Satisfied);
    assert_eq!(
        run.truth,
        Truth::Saved {
            data: sample(),
            matches_input: true
        }
    );
    for kind in [
        EventKind::OperationStarted,
        EventKind::RequestSent,
        EventKind::RequestReceived,
        EventKind::SaveCommitted,
        EventKind::ReplyReceived,
        EventKind::DisplaySaved,
    ] {
        assert!(has(&events, kind), "{kind:?}");
    }
    assert!(
        events
            .iter()
            .all(|event| event.correlation == run.manifest.correlation)
    );
    let proof = events
        .iter()
        .find(|event| event.kind == EventKind::SaveCommitted)
        .unwrap();
    assert_eq!(
        run.assessment.evidence,
        vec![format!("events.jsonl#{}", proof.sequence)]
    );
    assert!(run.directory.join("complete.json").exists());
}

#[test]
fn disconnect_gate_blocks_real_request_before_server_receives_it() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::DisconnectBeforeReceive);
    let events = load_events(&run.directory).unwrap();
    assert_eq!(run.assessment.verdict, Verdict::NotSatisfied);
    assert_eq!(run.truth, Truth::NotSaved);
    assert!(has(&events, EventKind::RequestSent));
    assert!(has(&events, EventKind::RequestBlocked));
    assert!(has(&events, EventKind::TransportFailed));
    assert!(has(&events, EventKind::DisplayFailed));
    assert!(!has(&events, EventKind::RequestReceived));
    assert!(!has(&events, EventKind::SaveCommitted));
    assert!(!has(&events, EventKind::DisplaySaved));
}

#[test]
fn save_failure_receives_request_but_does_not_save_or_display_success() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::FailBeforeSave);
    let events = load_events(&run.directory).unwrap();
    assert_eq!(run.assessment.verdict, Verdict::NotSatisfied);
    assert_eq!(run.truth, Truth::NotSaved);
    assert!(has(&events, EventKind::RequestReceived));
    assert!(has(&events, EventKind::SaveFailed));
    assert!(has(&events, EventKind::ReplyReceived));
    assert!(!has(&events, EventKind::SaveCommitted));
    assert!(!has(&events, EventKind::DisplaySaved));
}

#[test]
fn missing_save_observation_remains_unknown_despite_real_saved_data_and_success_display() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::OmitSaveObservation);
    let events = load_events(&run.directory).unwrap();
    assert_eq!(
        run.truth,
        Truth::Saved {
            data: sample(),
            matches_input: true
        }
    );
    assert!(has(&events, EventKind::DisplaySaved));
    assert!(!has(&events, EventKind::SaveCommitted));
    assert_eq!(run.assessment.verdict, Verdict::Unknown);
    assert!(run.assessment.evidence.is_empty());
    // 評価結果を書き換えても観測側の再判定には使われない。
    fs::write(run.directory.join("truth.json"), b"not even json").unwrap();
    assert_eq!(
        assess(
            &load_manifest(&run.directory).unwrap(),
            &load_events(&run.directory).unwrap()
        )
        .verdict,
        Verdict::Unknown
    );
}

#[test]
fn another_run_record_or_attempt_cannot_supply_positive_evidence() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::Normal);
    let events = load_events(&run.directory).unwrap();
    let proof = events
        .into_iter()
        .find(|event| event.kind == EventKind::SaveCommitted)
        .unwrap();
    for field in 0..3 {
        let mut other = proof.clone();
        match field {
            0 => other.correlation.run_id.push_str("-other"),
            1 => other.correlation.record_id.push_str("-other"),
            _ => other.correlation.attempt_id.push_str("-other"),
        }
        assert_eq!(assess(&run.manifest, &[other]).verdict, Verdict::Unknown);
    }
    let mut client_proof = proof;
    client_proof.source = Source::Client;
    assert_eq!(
        assess(&run.manifest, &[client_proof]).verdict,
        Verdict::Unknown
    );
}

#[test]
fn conflicting_observations_and_changed_condition_do_not_produce_success() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::Normal);
    let events = load_events(&run.directory).unwrap();
    let proof = events
        .into_iter()
        .find(|event| event.kind == EventKind::SaveCommitted)
        .unwrap();
    let mut failed = proof.clone();
    failed.kind = EventKind::SaveFailed;
    failed.sequence += 1;
    assert_eq!(
        assess(&run.manifest, &[proof.clone(), failed]).verdict,
        Verdict::Unknown
    );
    let mut changed = run.manifest;
    changed.condition.text = "管理画面から明日も閲覧できる".into();
    changed.condition.version += 1;
    assert_eq!(assess(&changed, &[proof]).verdict, Verdict::Unknown);
}

#[test]
fn saved_file_corruption_and_wrong_payload_are_distinguished_from_absence() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::Normal);
    fs::write(run.directory.join("stored.json"), b"broken").unwrap();
    assert!(matches!(
        verify_storage(&run.directory, &sample()),
        Truth::Unreadable { .. }
    ));
    fs::write(
        run.directory.join("stored.json"),
        serde_json::to_vec(&Data {
            name: "別のデータ".into(),
            age: 5,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        verify_storage(&run.directory, &sample()),
        Truth::Saved {
            matches_input: false,
            ..
        }
    ));
}

#[test]
fn retest_links_history_without_overwriting_it_and_records_configuration_changes() {
    let sandbox = Sandbox::new();
    let before = sandbox.trial(Fault::FailBeforeSave);
    let original = fs::read(before.directory.join("manifest.json")).unwrap();
    let after = run_trial(TrialConfig {
        output: sandbox.0.clone(),
        data: sample(),
        fault: Fault::Normal,
        previous_run: Some(before.manifest.correlation.run_id.clone()),
    })
    .unwrap();
    let comparison = compare_runs(&before.directory, &after.directory).unwrap();
    assert_ne!(before.directory, after.directory);
    assert_eq!(
        fs::read(before.directory.join("manifest.json")).unwrap(),
        original
    );
    assert_eq!(
        after.manifest.previous_run,
        Some(before.manifest.correlation.run_id)
    );
    assert!(comparison.same_question);
    assert!(!comparison.prior_evidence_matches_current_configuration);
    assert_eq!(comparison.before, Verdict::NotSatisfied);
    assert_eq!(comparison.after, Verdict::Satisfied);
    assert!(
        comparison
            .changed
            .iter()
            .any(|change| change.contains("故障条件"))
    );
}

#[test]
fn code_and_condition_changes_require_new_evidence() {
    let sandbox = Sandbox::new();
    let before = sandbox.trial(Fault::Normal);
    let after = sandbox.trial(Fault::Normal);
    let mut manifest = after.manifest;
    manifest.source_fingerprint.push_str("-changed");
    manifest.condition.version += 1;
    fs::write(
        after.directory.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let comparison = compare_runs(&before.directory, &after.directory).unwrap();
    assert!(!comparison.same_question);
    assert!(!comparison.prior_evidence_matches_current_configuration);
    assert_eq!(comparison.changed.len(), 2);
    assert_eq!(comparison.after, Verdict::Unknown);
}

#[test]
fn frames_round_trip_utf8_and_reject_truncation_or_oversized_lengths() {
    let mut bytes = vec![];
    write_frame(&mut bytes, &sample()).unwrap();
    let recovered: Data = read_frame(&mut Cursor::new(&bytes)).unwrap();
    assert_eq!(recovered, sample());
    bytes.pop();
    assert_eq!(
        read_frame::<Data>(&mut Cursor::new(bytes))
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
    let prefix = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes();
    assert_eq!(
        read_frame::<Data>(&mut Cursor::new(prefix))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        write_frame(&mut vec![], &"x".repeat(MAX_FRAME_BYTES))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn exclusive_file_creation_prevents_replacing_previous_results() {
    let sandbox = Sandbox::new();
    let file = sandbox.0.join("state.json");
    write_json_new(&file, &sample()).unwrap();
    let original = fs::read(&file).unwrap();
    assert_eq!(
        write_json_new(&file, &0).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(file).unwrap(), original);
}

#[test]
fn malformed_or_reordered_event_logs_and_unknown_schema_are_rejected() {
    let sandbox = Sandbox::new();
    let run = sandbox.trial(Fault::Normal);
    let mut events = load_events(&run.directory).unwrap();
    events[1].sequence = events[0].sequence;
    let content = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run.directory.join("events.jsonl"), content).unwrap();
    assert!(load_events(&run.directory).is_err());
    fs::write(run.directory.join("events.jsonl"), b"{\n").unwrap();
    assert!(load_events(&run.directory).is_err());
    let mut manifest = run.manifest;
    manifest.schema_version = 999;
    fs::write(
        run.directory.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(load_manifest(&run.directory).is_err());
}

#[test]
fn invalid_input_does_not_create_a_run() {
    let sandbox = Sandbox::new();
    for data in [
        Data {
            name: String::new(),
            age: 30,
        },
        Data {
            name: "架空".into(),
            age: 151,
        },
    ] {
        assert!(
            run_trial(TrialConfig {
                output: sandbox.0.clone(),
                data,
                fault: Fault::Normal,
                previous_run: None
            })
            .is_err()
        );
    }
    assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 0);
}

#[test]
fn external_command_records_output_and_exit_but_cannot_verify_business_condition() {
    let sandbox = Sandbox::new();
    let config = || CaptureConfig {
        output: sandbox.0.clone(),
        condition: "管理者が保存したデータを閲覧できる".into(),
        command: vec![
            env!("CARGO_BIN_EXE_execution-evidence-lab").into(),
            "help".into(),
        ],
        timeout_ms: 5000,
    };
    let run = capture(config()).unwrap();
    let events = load_events(&run.directory).unwrap();
    assert!(has(&events, EventKind::ProcessExited));
    assert_eq!(run.assessment.verdict, Verdict::Unknown);
    assert_eq!(run.truth, Truth::NotInspected);
    assert!(!run.assessment.evidence.is_empty());
    assert!(
        fs::read_to_string(run.directory.join("stdout.txt"))
            .unwrap()
            .contains("suite")
    );
    let other = capture(config()).unwrap();
    assert!(
        !compare_runs(&run.directory, &other.directory)
            .unwrap()
            .prior_evidence_matches_current_configuration
    );
}

#[test]
fn process_start_failure_is_recorded_without_claiming_it_started() {
    let sandbox = Sandbox::new();
    let run = capture(CaptureConfig {
        output: sandbox.0.clone(),
        condition: "保存済み".into(),
        command: vec![
            sandbox
                .0
                .join("does-not-exist")
                .to_string_lossy()
                .into_owned(),
        ],
        timeout_ms: 100,
    })
    .unwrap();
    let events = load_events(&run.directory).unwrap();
    assert!(has(&events, EventKind::ProcessStartFailed));
    assert!(!has(&events, EventKind::ProcessStarted));
    assert_eq!(run.assessment.verdict, Verdict::Unknown);
}

#[cfg(unix)]
#[test]
fn process_timeout_terminates_and_reaps_the_direct_child() {
    let sandbox = Sandbox::new();
    let run = capture(CaptureConfig {
        output: sandbox.0.clone(),
        condition: "待機後に保存される".into(),
        command: vec!["/bin/sleep".into(), "2".into()],
        timeout_ms: 10,
    })
    .unwrap();
    assert!(has(
        &load_events(&run.directory).unwrap(),
        EventKind::ProcessTimedOut
    ));
    assert_eq!(run.assessment.verdict, Verdict::Unknown);
}

#[test]
fn cli_rejects_invalid_unknown_and_duplicate_options() {
    for args in [
        vec!["demo", "--age", "151"],
        vec!["demo", "--unknown", "x"],
        vec!["demo", "--age"],
        vec!["demo", "--age", "30", "--age", "40"],
        vec![
            "capture",
            "--condition",
            "保存",
            "--timeout-ms",
            "0",
            "--",
            "help",
        ],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_execution-evidence-lab"))
            .args(args)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("エラー"));
    }
}
