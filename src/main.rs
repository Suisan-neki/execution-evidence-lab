use execution_evidence_lab::{
    assessment::compare_runs,
    capture::{CaptureConfig, capture},
    evaluation,
    experiment::{run_trial, verify_storage},
    model::*,
    recording::{load_events, load_manifest},
    review,
    workspace::{self, Change, ChangeRequest},
};
use std::{collections::BTreeMap, io, path::PathBuf};

const HELP: &str = "execution-evidence-lab
  demo [--fault normal|disconnect|save-failure|missing-observation] [--name TEXT] [--age 0..150] [--output DIR]
  suite [--output DIR]
  show RUN_DIR
  verify RUN_DIR
  retest RUN_DIR [--fault CONDITION] [--output DIR]
  compare BEFORE_DIR AFTER_DIR
  capture --condition TEXT [--timeout-ms 5000] [--output DIR] -- PROGRAM [ARGS...]
  repo prepare NEW_DIR RECIPE_JSON
  environment inspect RECIPE_JSON
  environment run RECIPE_JSON --repository-root DIR [--output DIR]
  environment show RUN_DIR
  environment compare BEFORE_DIR AFTER_DIR
  case init CASE_DIR [--file SPEC_JSON] [--actor TEXT] [--reason TEXT]
  case init CASE_DIR --template general --id CASE_ID --title TEXT [--actor TEXT] [--reason TEXT]
  case show CASE_DIR
  case apply CASE_DIR CHANGE_JSON
  case import CASE_DIR RUN_DIR --condition CHECK_ID --actor TEXT --reason TEXT [--use-run-context true|false]
  case execute CASE_DIR RECIPE_JSON --repository-root DIR --mode local|bubblewrap --actor TEXT --reason TEXT
  case run CASE_DIR --actor TEXT --reason TEXT [--condition save] [--fault CONDITION]
  case export CASE_DIR --output NEW_DIR
  case report CASE_DIR --output NEW_JSON [--format json|html]
  case backup CASE_DIR --output NEW_JSON
  case restore NEW_CASE_DIR BACKUP_JSON
  case compare CASE_DIR --before EVIDENCE_ID --after EVIDENCE_ID --condition CHECK_ID
  api CASE_DIR --token-file NEW_FILE [--port 8765] [--repository-root DIR --execution-mode local|bubblewrap]
  app DATA_DIR [--port 8765] [--execution-mode local|bubblewrap]
  explore [--port 8765]
  review CASE_DIR [--port 8765] [--repository-root DIR --execution-mode local|bubblewrap]

外部コマンドのcaptureは隔離環境ではありません。業務条件は終了コードだけで判定しません。";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn options(args: &[String], allowed: &[&str]) -> io::Result<BTreeMap<String, String>> {
    let mut parsed = BTreeMap::new();
    let (pairs, remainder) = args.as_chunks::<2>();
    for pair in pairs {
        if !allowed.contains(&pair[0].as_str()) {
            return Err(invalid(format!("不明なオプション: {}", pair[0])));
        }
        if parsed.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(invalid(format!("重複した指定: {}", pair[0])));
        }
    }
    if !remainder.is_empty() {
        return Err(invalid("オプションには値が必要です"));
    }
    Ok(parsed)
}
fn output(options: &BTreeMap<String, String>) -> PathBuf {
    options
        .get("--output")
        .map_or_else(|| PathBuf::from("runs"), PathBuf::from)
}
fn fault(options: &BTreeMap<String, String>, default: Fault) -> io::Result<Fault> {
    options
        .get("--fault")
        .map_or(Ok(default), |value| value.parse().map_err(invalid))
}
fn print_result(result: &RunResult) {
    println!(
        "試行: {}\n条件: {}\n観測からの判定: {}\n理由: {}\n根拠: {}\n次の確認: {}",
        result.directory.display(),
        result.manifest.condition.text,
        result.assessment.verdict.label(),
        result.assessment.reason,
        result.assessment.evidence.join(", "),
        result.assessment.next_action
    );
    println!(
        "評価側の独立した読み出し（判定には使用しない）: {:?}",
        result.truth
    );
}
fn run(args: &[String]) -> io::Result<()> {
    let Some(command) = args.first() else {
        return review::serve_app(
            PathBuf::from("data/app"),
            8765,
            execution_evidence_lab::execution::ExecutionMode::Local,
        );
    };
    match command.as_str() {
        "help" | "--help" | "-h" if args.len() == 1 => println!("{HELP}"),
        "case" => case_command(&args[1..])?,
        "environment" => environment_command(&args[1..])?,
        "api" if args.len() >= 2 => {
            let opts = options(
                &args[2..],
                &[
                    "--port",
                    "--token-file",
                    "--repository-root",
                    "--execution-mode",
                ],
            )?;
            let port = opts.get("--port").map_or(Ok(8765), |v| {
                v.parse::<u16>()
                    .map_err(|_| invalid("ポートは0〜65535です"))
            })?;
            let token_file = opts
                .get("--token-file")
                .ok_or_else(|| invalid("新規の--token-fileが必要です"))?;
            let execution = match (opts.get("--repository-root"), opts.get("--execution-mode")) {
                (None, None) => None,
                (Some(root), Some(mode)) => Some((PathBuf::from(root), mode.parse()?)),
                _ => return Err(invalid("実行にはリポジトリと方式の両方が必要です")),
            };
            review::serve_api(
                PathBuf::from(&args[1]),
                port,
                PathBuf::from(token_file),
                execution,
            )?;
        }
        "repo" if args.len() == 4 && args[1] == "prepare" => {
            let recipe = execution_evidence_lab::recording::read_json(&PathBuf::from(&args[3]))?;
            execution_evidence_lab::execution::prepare_repository(
                &PathBuf::from(&args[2]),
                &recipe,
            )?;
            println!(
                "固定コミットを取得しました: {}。checkoutと対象コードの実行は行っていません。",
                args[2]
            );
        }
        "app" if args.len() >= 2 => {
            let opts = options(&args[2..], &["--port", "--execution-mode"])?;
            let port = opts.get("--port").map_or(Ok(8765), |value| {
                value
                    .parse::<u16>()
                    .map_err(|_| invalid("ポートは0〜65535です"))
            })?;
            let mode = opts.get("--execution-mode").map_or(
                Ok(execution_evidence_lab::execution::ExecutionMode::Local),
                |m| m.parse(),
            )?;
            review::serve_app(PathBuf::from(&args[1]), port, mode)?;
        }
        "explore" => {
            let opts = options(&args[1..], &["--port"])?;
            let port = opts.get("--port").map_or(Ok(8765), |value| {
                value
                    .parse::<u16>()
                    .map_err(|_| invalid("ポートは0〜65535です"))
            })?;
            review::serve_explorer(port)?;
        }
        "review" if args.len() >= 2 => {
            let opts = options(
                &args[2..],
                &["--port", "--repository-root", "--execution-mode"],
            )?;
            let port = opts.get("--port").map_or(Ok(8765), |value| {
                value
                    .parse::<u16>()
                    .map_err(|_| invalid("ポートは0〜65535です"))
            })?;
            match (opts.get("--repository-root"), opts.get("--execution-mode")) {
                (None, None) => review::serve(PathBuf::from(&args[1]), port)?,
                (Some(root), Some(mode)) => review::serve_with_execution(
                    PathBuf::from(&args[1]),
                    port,
                    PathBuf::from(root),
                    mode.parse()?,
                )?,
                _ => {
                    return Err(invalid(
                        "画面から実行するには--repository-rootと--execution-modeの両方を指定してください",
                    ));
                }
            }
        }
        "demo" => {
            let opts = options(&args[1..], &["--fault", "--name", "--age", "--output"])?;
            let age = opts.get("--age").map_or(Ok(30), |value| {
                value
                    .parse::<u16>()
                    .map_err(|_| invalid("年齢は整数0〜150"))
            })?;
            let data = Data {
                name: opts
                    .get("--name")
                    .cloned()
                    .unwrap_or_else(|| "架空の利用者".into()),
                age,
            };
            print_result(&run_trial(TrialConfig {
                output: output(&opts),
                data,
                fault: fault(&opts, Fault::Normal)?,
                previous_run: None,
            })?);
        }
        "suite" => {
            let opts = options(&args[1..], &["--output"])?;
            for fault in Fault::ALL {
                let result = run_trial(TrialConfig {
                    output: output(&opts),
                    data: Data {
                        name: "架空の利用者".into(),
                        age: 30,
                    },
                    fault,
                    previous_run: None,
                })?;
                println!(
                    "{}\t{}\t{:?}\t{}",
                    fault.label(),
                    result.assessment.verdict.label(),
                    result.truth,
                    result.directory.display()
                );
            }
        }
        "show" if args.len() == 2 => {
            let directory = PathBuf::from(&args[1]);
            let manifest = load_manifest(&directory)?;
            let events = load_events(&directory)?;
            println!("条件: {}", manifest.condition.text);
            println!(
                "{}",
                serde_json::to_string_pretty(&execution_evidence_lab::execution::assess_run(
                    &directory, &manifest, &events
                )?)
                .map_err(io::Error::other)?
            );
            for event in events {
                println!(
                    "#{} +{}us {:?}/{:?}: {}",
                    event.sequence, event.elapsed_us, event.source, event.kind, event.detail
                );
            }
        }
        "verify" if args.len() == 2 => {
            let directory = PathBuf::from(&args[1]);
            let manifest = load_manifest(&directory)?;
            if manifest.mode != "demo" {
                return Err(invalid("外部アプリの保存先と読み出し方法は未定義です"));
            }
            let input = manifest
                .input
                .ok_or_else(|| invalid("入力が記録されていません"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&verify_storage(&directory, &input))
                    .map_err(io::Error::other)?
            );
        }
        "retest" if args.len() >= 2 => {
            let old_directory = PathBuf::from(&args[1]);
            let old = load_manifest(&old_directory)?;
            if old.mode != "demo" {
                return Err(invalid("captureの再実行にはコマンドを明示してください"));
            }
            let opts = options(&args[2..], &["--fault", "--output"])?;
            let result = run_trial(TrialConfig {
                output: output(&opts),
                data: old.input.ok_or_else(|| invalid("入力がありません"))?,
                fault: fault(&opts, old.fault.unwrap_or(Fault::Normal))?,
                previous_run: Some(old.correlation.run_id),
            })?;
            print_result(&result);
            println!(
                "比較: {}",
                serde_json::to_string_pretty(&compare_runs(&old_directory, &result.directory)?)
                    .map_err(io::Error::other)?
            );
        }
        "compare" if args.len() == 3 => println!(
            "{}",
            serde_json::to_string_pretty(&compare_runs(
                &PathBuf::from(&args[1]),
                &PathBuf::from(&args[2])
            )?)
            .map_err(io::Error::other)?
        ),
        "capture" => {
            let separator = args
                .iter()
                .position(|arg| arg == "--")
                .ok_or_else(|| invalid("--の後に起動コマンドが必要です"))?;
            let opts = options(
                &args[1..separator],
                &["--condition", "--timeout-ms", "--output"],
            )?;
            let condition = opts
                .get("--condition")
                .cloned()
                .ok_or_else(|| invalid("--conditionが必要です"))?;
            let timeout_ms = opts.get("--timeout-ms").map_or(Ok(5000), |value| {
                value.parse().map_err(|_| invalid("待機上限は整数です"))
            })?;
            print_result(&capture(CaptureConfig {
                output: output(&opts),
                condition,
                command: args[separator + 1..].to_vec(),
                timeout_ms,
            })?);
        }
        _ => return Err(invalid(format!("引数を確認してください\n{HELP}"))),
    }
    Ok(())
}
fn case_command(args: &[String]) -> io::Result<()> {
    let (Some(command), Some(directory)) = (args.first(), args.get(1)) else {
        return Err(invalid(HELP));
    };
    let directory = PathBuf::from(directory);
    let print_view = |view: workspace::WorkspaceView| -> io::Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&view).map_err(io::Error::other)?
        );
        Ok(())
    };
    let required = |opts: &BTreeMap<String, String>, name: &str| {
        opts.get(name)
            .cloned()
            .ok_or_else(|| invalid(format!("{name}が必要です")))
    };
    match command.as_str() {
        "init" => {
            let opts = options(
                &args[2..],
                &[
                    "--file",
                    "--actor",
                    "--reason",
                    "--template",
                    "--id",
                    "--title",
                ],
            )?;
            if opts.contains_key("--file")
                && ["--template", "--id", "--title"]
                    .iter()
                    .any(|k| opts.contains_key(*k))
            {
                return Err(invalid(
                    "--fileと--template・--id・--titleは同時に指定できません",
                ));
            }
            let case = match opts.get("--file") {
                Some(file) => execution_evidence_lab::recording::read_json(&PathBuf::from(file))?,
                None => match opts.get("--template").map(String::as_str).unwrap_or("demo") {
                    "general" => workspace::empty_workspace(
                        required(&opts, "--id")?,
                        required(&opts, "--title")?,
                    ),
                    "demo" if !opts.contains_key("--id") && !opts.contains_key("--title") => {
                        workspace::demo_workspace()
                    }
                    _ => {
                        return Err(invalid(
                            "--template generalには--idと--titleが必要です。demoでは指定できません",
                        ));
                    }
                },
            };
            print_view(workspace::initialize(
                &directory,
                case,
                opts.get("--actor").map(String::as_str).unwrap_or("Codex"),
                opts.get("--reason")
                    .map(String::as_str)
                    .unwrap_or("事例を初期化する。条件・発言・根拠は入力された範囲だけを扱う"),
            )?)
        }
        "show" if args.len() == 2 => print_view(workspace::view(workspace::load(&directory)?)),
        "report" => {
            let opts = options(&args[2..], &["--output", "--format"])?;
            let report =
                execution_evidence_lab::review_report::build(workspace::load(&directory)?)?;
            let content = match opts.get("--format").map(String::as_str).unwrap_or("json") {
                "json" => serde_json::to_vec_pretty(&report).map_err(io::Error::other)?,
                "html" => execution_evidence_lab::review_report::html(&report)?.into_bytes(),
                _ => return Err(invalid("確認資料の形式はjsonかhtmlです")),
            };
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(required(&opts, "--output")?)?;
            file.write_all(&content)?;
            file.sync_all()
        }
        "backup" => {
            let opts = options(&args[2..], &["--output"])?;
            execution_evidence_lab::recording::write_json_new(
                &PathBuf::from(required(&opts, "--output")?),
                &execution_evidence_lab::backup::snapshot(&directory)?,
            )
        }
        "restore" if args.len() == 3 => print_view(execution_evidence_lab::backup::restore(
            &directory,
            &execution_evidence_lab::backup::read(&PathBuf::from(&args[2]))?,
        )?),
        "compare" => {
            let opts = options(&args[2..], &["--before", "--after", "--condition"])?;
            let w = workspace::load(&directory)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&execution_evidence_lab::comparison::compare(
                    w.clone(),
                    execution_evidence_lab::comparison::CompareRequest {
                        expected_revision: w.revision,
                        check_id: required(&opts, "--condition")?,
                        before_id: required(&opts, "--before")?,
                        after_id: required(&opts, "--after")?
                    }
                )?)
                .map_err(io::Error::other)?
            );
            Ok(())
        }
        "apply" if args.len() == 3 => {
            let request: ChangeRequest =
                execution_evidence_lab::recording::read_json(&PathBuf::from(&args[2]))?;
            print_view(workspace::change(&directory, request)?)
        }
        "import" if args.len() >= 3 => {
            let opts = options(
                &args[3..],
                &["--condition", "--actor", "--reason", "--use-run-context"],
            )?;
            let use_run_context = opts.get("--use-run-context").map_or(Ok(false), |value| {
                value
                    .parse::<bool>()
                    .map_err(|_| invalid("--use-run-contextにはtrueかfalseを指定します"))
            })?;
            print_view(workspace::change(
                &directory,
                ChangeRequest {
                    expected_revision: workspace::load(&directory)?.revision,
                    actor: required(&opts, "--actor")?,
                    reason: required(&opts, "--reason")?,
                    action: Change::Import {
                        check_id: required(&opts, "--condition")?,
                        run_directory: args[2].clone(),
                        use_run_context,
                    },
                },
            )?)
        }
        "execute" if args.len() >= 3 => {
            let opts = options(
                &args[3..],
                &["--repository-root", "--mode", "--actor", "--reason"],
            )?;
            let mode = required(&opts, "--mode")?
                .parse::<execution_evidence_lab::execution::ExecutionMode>()?;
            let recipe = execution_evidence_lab::recording::read_json(&PathBuf::from(&args[2]))?;
            print_view(workspace::run_external_mode(
                &directory,
                &PathBuf::from(required(&opts, "--repository-root")?),
                recipe,
                mode,
                required(&opts, "--actor")?,
                required(&opts, "--reason")?,
            )?)
        }
        "run" => {
            let opts = options(
                &args[2..],
                &["--actor", "--reason", "--condition", "--fault"],
            )?;
            print_view(workspace::run_demo(
                &directory,
                workspace::load(&directory)?.revision,
                required(&opts, "--actor")?,
                required(&opts, "--reason")?,
                opts.get("--condition")
                    .cloned()
                    .unwrap_or_else(|| "save".into()),
                fault(&opts, Fault::Normal)?,
            )?)
        }
        "export" => {
            let opts = options(&args[2..], &["--output"])?;
            let output = PathBuf::from(required(&opts, "--output")?);
            evaluation::export(&directory, &output)?;
            println!("比較資料: {}（利用者評価は未実施）", output.display());
            Ok(())
        }
        _ => Err(invalid(HELP)),
    }
}
fn environment_command(args: &[String]) -> io::Result<()> {
    use execution_evidence_lab::environment;
    let value = match args.first().map(String::as_str) {
        Some("inspect") if args.len() == 2 => environment::inspect(&PathBuf::from(&args[1]))?,
        Some("show") if args.len() == 2 => {
            serde_json::to_value(environment::load(&PathBuf::from(&args[1]))?)
                .map_err(io::Error::other)?
        }
        Some("compare") if args.len() == 3 => environment::compare(
            &environment::load(&PathBuf::from(&args[1]))?,
            &environment::load(&PathBuf::from(&args[2]))?,
        ),
        Some("run") if args.len() >= 4 => {
            let opts = options(&args[2..], &["--repository-root", "--output"])?;
            let root = opts
                .get("--repository-root")
                .ok_or_else(|| invalid("--repository-rootが必要です"))?;
            let path = PathBuf::from(&args[1]);
            println!(
                "{}",
                serde_json::to_string_pretty(&environment::inspect(&path)?)
                    .map_err(io::Error::other)?
            );
            let directory = environment::run(&path, &PathBuf::from(root), &output(&opts))?;
            let record = environment::load(&directory)?;
            println!(
                "記録: {}\n準備: {:?}\n対象処理: {:?}",
                directory.display(),
                record.report.preparation,
                record.report.target
            );
            if record.report.preparation != environment::Status::Succeeded
                || (record.recipe.command_phase == "target"
                    && record.report.target != environment::Status::Succeeded)
                || !record.container_removed
                || !record.diagnostics.is_empty()
            {
                return Err(io::Error::other(format!(
                    "準備または指定コマンドが完了しませんでした。詳細: {}/environment.json",
                    directory.display()
                )));
            }
            return Ok(());
        }
        _ => return Err(invalid(HELP)),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(io::Error::other)?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("エラー: {error}");
        std::process::exit(1);
    }
}
