//! The example application, end to end: a multi-module Skuld program that
//! reads a JSON file, selects part of it and writes the result.
//!
//! Every temporary file belongs to this test, including the ones the program
//! writes, and the scratch directory goes away whether the test passes or not.
use std::{env, fs, path::PathBuf, process::Command};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-tool-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.directory.join(name);
        fs::write(&path, content).expect("fixture file");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn clang_available() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn repository(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join(relative)
}

/// Build `examples/jsontool.skuld` into the scratch directory.
fn build(scratch: &Scratch) -> PathBuf {
    let binary = scratch.path("jsontool");
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("build")
        .arg(repository("examples/jsontool.skuld"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("build jsontool");
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

const DOCUMENT: &str = r#"{"user": {"name": "Ada", "tags": ["native", "fast"]}, "ok": true}"#;

#[test]
fn the_tool_selects_part_of_a_document_and_writes_it_out() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("select");
    let tool = build(&scratch);
    let input = scratch.write("document.json", DOCUMENT);

    // To standard output, by a path that walks an object and an array.
    let output = Command::new(&tool)
        .arg(&input)
        .arg("user.tags.1")
        .output()
        .expect("run jsontool");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "\"fast\"\n");

    // And to a file, which the tool creates itself.
    let destination = scratch.path("name.json");
    let output = Command::new(&tool)
        .arg(&input)
        .arg("user.name")
        .arg("-o")
        .arg(&destination)
        .output()
        .expect("run jsontool");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    assert_eq!(
        fs::read_to_string(&destination).expect("the written file"),
        "\"Ada\"\n"
    );
}

#[test]
fn counting_the_member_names_of_a_document_uses_the_map() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("keys");
    let tool = build(&scratch);
    let input = scratch.write(
        "repeated.json",
        r#"{"items": [{"id": 1, "name": "a"}, {"id": 2}], "id": 0}"#,
    );
    let output = Command::new(&tool)
        .arg(&input)
        .arg("--keys")
        .output()
        .expect("run jsontool");
    assert!(output.status.success());
    // Counted across the whole document, in the order the names first appear.
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1 items\n3 id\n1 name\n"
    );
}

#[test]
fn a_missing_file_is_reported_on_stderr_and_writes_nothing() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("missing");
    let tool = build(&scratch);
    let destination = scratch.path("out.json");
    let output = Command::new(&tool)
        .arg(scratch.path("nothing.json"))
        .arg("")
        .arg("-o")
        .arg(&destination)
        .output()
        .expect("run jsontool");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("could not open"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // A failure before the output exists leaves nothing behind.
    assert!(!destination.exists());
}

#[test]
fn malformed_input_and_a_path_that_leads_nowhere_are_told_apart() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("malformed");
    let tool = build(&scratch);

    let broken = scratch.write("broken.json", "{\"user\": }");
    let output = Command::new(&tool).arg(&broken).output().expect("run");
    assert_eq!(output.status.code(), Some(1));
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("broken.json"), "{message}");

    let good = scratch.write("document.json", DOCUMENT);
    let output = Command::new(&tool)
        .arg(&good)
        .arg("user.missing")
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no member named `missing`"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A command line that makes no sense is the other kind of failure, and
    // has its own status.
    let output = Command::new(&tool).output().expect("run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}
