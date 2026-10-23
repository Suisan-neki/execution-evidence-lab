use super::{HELP, invalid, options, output};
use std::{io, path::PathBuf};

pub(super) fn run(args: &[String]) -> io::Result<()> {
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
