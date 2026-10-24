use super::{HELP, invalid, options};
use execution_evidence_lab::workspace::{self, Change, ChangeRequest};
use std::{collections::BTreeMap, io, path::PathBuf};

pub(super) fn run(args: &[String]) -> io::Result<()> {
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
                None => match opts
                    .get("--template")
                    .map(String::as_str)
                    .unwrap_or("general")
                {
                    "general" => workspace::empty_workspace(
                        opts.get("--id").cloned().unwrap_or_else(|| "case".into()),
                        opts.get("--title")
                            .cloned()
                            .unwrap_or_else(|| "確認する動作".into()),
                    ),
                    _ => {
                        return Err(invalid("--templateにはgeneralだけを指定できます"));
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
        _ => Err(invalid(HELP)),
    }
}
