//! 宣言済みの試行・手順の画面写真だけを読む。任意のパスはHTTPで受け付けない。
use crate::{browser::BrowserReport, execution::ExecutionTrace};
use std::{
    fs,
    io::{self, Read},
    path::Path,
};
const MAX_PNG: u64 = 4 * 1024 * 1024;
fn missing() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "この事例内に対応する画面写真がありません。取り込み元の試行や復元時の有無を確認してください",
    )
}
fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
/// バックアップに含めない実行フォルダーを参照する。写真の欠落で判定は変えない。
pub fn screenshot(directory: &Path, trace: &ExecutionTrace, step_id: &str) -> io::Result<Vec<u8>> {
    let report = trace
        .containers
        .as_ref()
        .and_then(|c| c.browser.as_ref())
        .ok_or_else(missing)?;
    read(directory, &trace.correlation.run_id, report, step_id)
}
fn read(
    directory: &Path,
    run_id: &str,
    report: &BrowserReport,
    step_id: &str,
) -> io::Result<Vec<u8>> {
    if !component(run_id) || !component(step_id) || report.run_id != run_id {
        return Err(missing());
    }
    let step = report
        .steps
        .iter()
        .find(|s| s.id == step_id)
        .ok_or_else(missing)?;
    if !["preparation", "action"].contains(&step.phase.as_str()) {
        return Err(missing());
    }
    let file = format!("{}-{}.png", step.phase, step.id);
    if step.screenshot.as_deref() != Some(format!("browser/{file}").as_str()) {
        return Err(missing());
    }
    let mut path = directory.to_path_buf();
    for part in ["runs", run_id, "browser", &file] {
        path.push(part);
        if fs::symlink_metadata(&path)
            .map_err(|_| missing())?
            .file_type()
            .is_symlink()
        {
            return Err(missing());
        }
    }
    let meta = fs::metadata(&path).map_err(|_| missing())?;
    if !meta.is_file() || meta.len() > MAX_PNG {
        return Err(missing());
    }
    let mut bytes = vec![];
    fs::File::open(&path)?
        .take(MAX_PNG + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PNG || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(missing());
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::StepRecord;
    #[test]
    fn only_declared_local_pngs_are_read_and_missing_or_redirected_paths_are_rejected() {
        let (dir, correlation) =
            crate::recording::new_run_directory(&std::env::temp_dir().join("eel-browser-evidence"))
                .unwrap();
        let mut report = BrowserReport {
            schema_version: 1,
            run_id: correlation.run_id.clone(),
            version: "test".into(),
            prepared: true,
            completed: true,
            steps: vec![StepRecord {
                id: "saved".into(),
                phase: "action".into(),
                completed: true,
                text: None,
                screenshot: Some("browser/action-saved.png".into()),
                detail: String::new(),
            }],
            responses: vec![],
            blocked_requests: 0,
            detail: String::new(),
        };
        let path = dir.join("runs").join(&report.run_id).join("browser");
        fs::create_dir_all(&path).unwrap();
        let png = b"\x89PNG\r\n\x1a\nsynthetic";
        fs::write(path.join("action-saved.png"), png).unwrap();
        assert_eq!(read(&dir, &report.run_id, &report, "saved").unwrap(), png);
        assert!(read(&dir, &report.run_id, &report, "../saved").is_err());
        assert!(read(&dir, "other-run", &report, "saved").is_err());
        report.steps[0].screenshot = Some("../../outside.png".into());
        assert!(read(&dir, &report.run_id, &report, "saved").is_err());
        report.steps[0].screenshot = Some("browser/action-saved.png".into());
        fs::remove_file(path.join("action-saved.png")).unwrap();
        assert!(read(&dir, &report.run_id, &report, "saved").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", path.join("action-saved.png")).unwrap();
            assert!(read(&dir, &report.run_id, &report, "saved").is_err());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
