use std::{env, fs, path::Path, process::Command};
fn collect(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read source directory") {
            collect(&entry.expect("source entry").path(), files);
        }
    } else if path.extension().is_some_and(|ext| ext == "rs") {
        files.push(path.to_owned());
    }
}
fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").expect("manifest directory");
    let root = Path::new(&root);
    let mut files = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("build.rs"),
    ];
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=.git/index");
    collect(&root.join("src"), &mut files);
    files.sort();
    // 変更検知用。暗号学的な署名や改ざん検知ではない。
    let mut hash = 0xcbf29ce484222325_u64;
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        for byte in file
            .strip_prefix(root)
            .expect("source path")
            .to_string_lossy()
            .bytes()
            .chain([0])
            .chain(fs::read(&file).expect("read source"))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    println!("cargo:rustc-env=SOURCE_FINGERPRINT=fnv1a64:{hash:016x}");
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()
    };
    let revision = git(&["rev-parse", "HEAD"])
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|out| !out.stdout.is_empty());
    println!(
        "cargo:rustc-env=SOURCE_REVISION={revision}{}",
        if dirty { "+dirty" } else { "" }
    );
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Some(out) = git(&["symbolic-ref", "HEAD"])
        && out.status.success()
    {
        println!(
            "cargo:rerun-if-changed=.git/{}",
            String::from_utf8_lossy(&out.stdout).trim()
        );
    }
}
