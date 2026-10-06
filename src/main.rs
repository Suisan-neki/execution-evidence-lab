use execution_evidence_lab::{
    assessment::{assess, compare_runs},
    capture::{CaptureConfig, capture},
    experiment::{run_trial, verify_storage},
    model::*,
    recording::{load_events, load_manifest},
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
        println!("{HELP}");
        return Ok(());
    };
    match command.as_str() {
        "help" | "--help" | "-h" if args.len() == 1 => println!("{HELP}"),
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
                serde_json::to_string_pretty(&assess(&manifest, &events))
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
fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("エラー: {error}");
        std::process::exit(1);
    }
}
