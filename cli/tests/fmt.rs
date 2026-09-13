use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn skuld() -> Command {
    Command::new(env!("CARGO_BIN_EXE_skuld"))
}

fn fixtures(category: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join(category);
    let mut sources: Vec<_> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", root.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension() == Some(OsStr::new("skuld")))
        .collect();
    sources.sort();
    assert!(!sources.is_empty(), "no fixtures in {}", root.display());
    sources
}

#[test]
fn formatting_examples_is_idempotent() {
    for source in fixtures("examples") {
        let text = fs::read_to_string(&source).expect("read example");
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!(
            "skuld-fmt-test-{}-{}",
            std::process::id(),
            source.file_name().unwrap().to_string_lossy()
        ));
        fs::write(&path, &text).expect("write temp file");

        // Format once
        let output = skuld().arg("fmt").arg(&path).output().expect("format");
        assert!(
            output.status.success(),
            "fmt failed on {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let first_pass = fs::read_to_string(&path).expect("read first pass");

        // Check mode on formatted code must succeed
        let check = skuld()
            .args(["fmt", "--check"])
            .arg(&path)
            .output()
            .expect("fmt --check");
        assert!(
            check.status.success(),
            "check mode reported drift on already formatted file {}: {}",
            source.display(),
            String::from_utf8_lossy(&check.stderr)
        );

        // Format second time must produce identical output (idempotency)
        let output2 = skuld().arg("fmt").arg(&path).output().expect("format 2nd");
        assert!(output2.status.success());
        let second_pass = fs::read_to_string(&path).expect("read second pass");
        assert_eq!(
            first_pass,
            second_pass,
            "formatting not idempotent on {}",
            source.display()
        );

        fs::remove_file(&path).expect("cleanup");
    }
}
