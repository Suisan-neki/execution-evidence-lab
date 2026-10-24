use mugiurushi::{environment::*, prepared_web::*, recording::*};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fixture() -> (PathBuf, WebBuild, Record) {
    let root = new_run_directory(&std::env::temp_dir().join("eel-web-assets-test"))
        .unwrap()
        .0;
    let run = root.join("run-synthetic");
    fs::create_dir_all(run.join("result/dist/scripts")).unwrap();
    fs::write(run.join("result/dist/index.html"), b"synthetic HTML").unwrap();
    fs::write(run.join("result/dist/scripts/app.js"), b"synthetic JS").unwrap();
    let files = vec![
        BuiltFile {
            path: "index.html".into(),
            bytes: 14,
            sha256: sha256(b"synthetic HTML"),
        },
        BuiltFile {
            path: "scripts/app.js".into(),
            bytes: 12,
            sha256: sha256(b"synthetic JS"),
        },
    ];
    let manifest = serde_json::to_vec(&files).unwrap();
    fs::write(run.join("result/build-files.json"), &manifest).unwrap();
    let recipe = read_json(Path::new("examples/ohif-build/recipe.json")).unwrap();
    let record = Record {
        schema_version: 1,
        recipe,
        tree: "synthetic-tree".into(),
        source_archive_sha256: "synthetic-archive".into(),
        engine: Some("synthetic-engine".into()),
        image_id: None,
        runner_sha256: "synthetic-runner".into(),
        exit_code: Some(0),
        timed_out: false,
        output_limit: false,
        container_removed: true,
        diagnostics: vec![],
        report: Report {
            preparation: Status::Succeeded,
            packages: vec![Package {
                name: "synthetic-package".into(),
                version: "1".into(),
                hash: "synthetic-hash".into(),
                hash_scope: "synthetic-test".into(),
                lock_integrity: None,
            }],
            ..Default::default()
        },
    };
    fs::write(
        run.join("environment.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    fs::write(
        run.join("result/packages.json"),
        serde_json::to_vec(&record.report.packages).unwrap(),
    )
    .unwrap();
    let build = WebBuild {
        run_id: "run-synthetic".into(),
        manifest_sha256: sha256(&manifest),
        source: record.recipe.source.clone(),
    };
    (root, build, record)
}
#[test]
fn copies_only_verified_bytes_and_preserves_the_build_basis() {
    let (root, build, record) = fixture();
    fs::write(
        root.join("run-synthetic/result/dist/unlisted.txt"),
        "not included",
    )
    .unwrap();
    let copy = snapshot(&root, &build).unwrap();
    assert_eq!(copy.basis.files, 2);
    assert_eq!(copy.basis.preparation_image, record.recipe.image);
    assert_eq!(
        fs::read(copy.directory.join("scripts/app.js")).unwrap(),
        b"synthetic JS"
    );
    assert!(!copy.directory.join("unlisted.txt").exists());
    fs::write(
        root.join("run-synthetic/result/dist/index.html"),
        "modified later",
    )
    .unwrap();
    assert_eq!(
        fs::read(copy.directory.join("index.html")).unwrap(),
        b"synthetic HTML"
    );
    assert!(snapshot(&root, &build).is_err());
}
#[test]
fn rejects_failed_preparation_or_a_different_source_and_manifest() {
    let (root, mut build, mut record) = fixture();
    build.manifest_sha256 = "0".repeat(64);
    assert!(snapshot(&root, &build).is_err());
    let manifest = fs::read(root.join("run-synthetic/result/build-files.json")).unwrap();
    build.manifest_sha256 = sha256(&manifest);
    record.report.preparation = Status::Failed;
    fs::write(
        root.join("run-synthetic/environment.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert!(snapshot(&root, &build).is_err());
    record.report.preparation = Status::Succeeded;
    fs::write(
        root.join("run-synthetic/environment.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    build.source.commit = "a".repeat(40);
    assert!(snapshot(&root, &build).is_err());
}
#[test]
fn rejects_escape_paths_duplicate_entries_and_symlinked_files() {
    let (root, mut build, _) = fixture();
    for entries in [
        vec![
            BuiltFile {
                path: "index.html".into(),
                bytes: 14,
                sha256: sha256(b"synthetic HTML"),
            },
            BuiltFile {
                path: "../index.html".into(),
                bytes: 0,
                sha256: sha256(b""),
            },
        ],
        vec![
            BuiltFile {
                path: "index.html".into(),
                bytes: 14,
                sha256: sha256(b"synthetic HTML"),
            },
            BuiltFile {
                path: "index.html".into(),
                bytes: 14,
                sha256: sha256(b"synthetic HTML"),
            },
        ],
    ] {
        let manifest = serde_json::to_vec(&entries).unwrap();
        build.manifest_sha256 = sha256(&manifest);
        fs::write(root.join("run-synthetic/result/build-files.json"), manifest).unwrap();
        assert!(snapshot(&root, &build).is_err());
    }
    #[cfg(unix)]
    {
        let (root, build, _) = fixture();
        let file = root.join("run-synthetic/result/dist/scripts/app.js");
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink("../index.html", file).unwrap();
        assert!(snapshot(&root, &build).is_err());
    }
}
