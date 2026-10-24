use mugiurushi::{environment::*, recording::*};
use std::{fs, path::Path};

fn recipe() -> Recipe {
    read_json(Path::new("examples/monai-cpu/recipe.json")).unwrap()
}
#[test]
fn rejects_unpinned_images_and_dependencies() {
    let mut r = recipe();
    validate(&r).unwrap();
    r.image = "python:latest".into();
    assert!(validate(&r).is_err());
    r = recipe();
    if let Setup::PythonWheels { wheels } = &mut r.setup {
        wheels[0].sha256.clear();
    }
    assert!(validate(&r).is_err());
    r = recipe();
    r.locks[0].path = "../pyproject.toml".into();
    assert!(validate(&r).is_err());
    r = recipe();
    r.env.insert("PYTHONPATH".into(), "/host".into());
    assert!(validate(&r).is_err());
}
#[test]
fn refuses_changed_support_before_any_execution() {
    let dir = new_run_directory(&std::env::temp_dir().join("eel-environment-test"))
        .unwrap()
        .0;
    let mut r = recipe();
    r.support[0].path = "probe.py".into();
    fs::write(dir.join("probe.py"), "raise RuntimeError('changed')\n").unwrap();
    write_json_new(&dir.join("recipe.json"), &r).unwrap();
    assert!(inspect(&dir.join("recipe.json")).is_err());
    assert!(inspect(Path::new("examples/ohif-build/recipe.json")).is_ok());
}
fn record() -> Record {
    Record {
        schema_version: 1,
        recipe: recipe(),
        tree: "test-tree".into(),
        source_archive_sha256: "test-archive".into(),
        engine: Some("test-engine".into()),
        image_id: None,
        runner_sha256: "runner".into(),
        exit_code: Some(0),
        timed_out: false,
        output_limit: false,
        container_removed: true,
        diagnostics: vec![],
        report: Report {
            preparation: Status::Succeeded,
            target: Status::Succeeded,
            packages: vec![Package {
                name: "synthetic".into(),
                version: "1.0.0".into(),
                hash: "same-manifest".into(),
                hash_scope: "installed_package_json".into(),
                lock_integrity: Some("first-tarball".into()),
            }],
            ..Default::default()
        },
    }
}
#[test]
fn comparison_includes_distribution_integrity_and_incomplete_preparation() {
    let before = record();
    let mut after = before.clone();
    after.report.packages[0].lock_integrity = Some("changed-tarball".into());
    let diff = compare(&before, &after);
    assert_eq!(diff["removed_or_changed"].as_array().unwrap().len(), 1);
    assert_eq!(diff["added_or_changed"].as_array().unwrap().len(), 1);
    after.report.preparation = Status::Failed;
    after.report.target = Status::NotObserved;
    let diff = compare(&before, &after);
    assert_eq!(diff["packages_comparable"], false);
    assert_eq!(diff["target_after"], "not_observed");
}
