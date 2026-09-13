//! `skuld test`, end to end: discovery, the report, and the exit status a
//! build script reads.
use std::{env, fs, path::PathBuf, process::Command};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }

    fn file(&self, source: &str) -> PathBuf {
        let path = self.directory.join("suite.skuld");
        fs::write(&path, source).expect("test source");
        path
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

fn skuld_test(path: &std::path::Path) -> (String, String, Option<i32>) {
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("test")
        .arg(path)
        .output()
        .expect("run skuld test");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

fn repository(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join(relative)
}

#[test]
fn the_example_application_has_its_own_passing_suite() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    // The milestone's marker: a multi-module program with tests of its own,
    // run by the language's own command.
    let (out, err, code) = skuld_test(&repository("examples/jsontool_tests.skuld"));
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(
        out.contains("ok   test_selecting_walks_objects_and_arrays"),
        "{out}"
    );
    assert!(out.contains("7 of 7 tests passed"), "{out}");
}

#[test]
fn a_failing_test_stops_the_suite_and_fails_the_command() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("failing");
    let path = scratch.file(
        "import \"std/testing\"\n\
         \n\
         func test_first() {\n\
         }\n\
         \n\
         func test_second() {\n\
             print(\"halfway\")\n\
             testing.equal_int(4, 5, \"sums\")\n\
         }\n\
         \n\
         func test_third() {\n\
         }\n",
    );
    let (out, _, code) = skuld_test(&path);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("ok   test_first"), "{out}");
    assert!(out.contains("FAIL test_second"), "{out}");
    assert!(out.contains("test_third (not run)"), "{out}");
    // The failure's own message, and what the test printed before it.
    assert!(out.contains("sums: expected 5, got 4"), "{out}");
    assert!(out.contains("halfway"), "{out}");
    assert!(out.contains("1 of 3 tests passed"), "{out}");
}

#[test]
fn a_trap_is_a_failure_and_the_tests_before_it_are_still_reported() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("trap");
    let path = scratch.file(
        "func test_first() {\n\
         }\n\
         \n\
         func test_second() {\n\
             let items = [1, 2]\n\
             print(items[5])\n\
         }\n",
    );
    let (out, err, code) = skuld_test(&path);
    assert_eq!(code, Some(1), "{out}{err}");
    // The marker for the first test survived a process that was killed: that
    // is what the flush in `std/testing` is for.
    assert!(out.contains("ok   test_first"), "{out}");
    assert!(out.contains("FAIL test_second"), "{out}");
    assert!(err.contains("array index out of bounds"), "{err}");
}

#[test]
fn a_file_that_holds_no_tests_says_so_instead_of_passing() {
    let scratch = Scratch::new("empty");
    let path = scratch.file("func helper() -> int {\n    return 1\n}\n");
    let (_, err, code) = skuld_test(&path);
    assert_eq!(code, Some(1));
    assert!(err.contains("no tests here"), "{err}");
}

#[test]
fn a_test_file_may_not_declare_its_own_entry_point() {
    let scratch = Scratch::new("main");
    let path = scratch.file("func main() {\n}\n\nfunc test_one() {\n}\n");
    let (_, err, code) = skuld_test(&path);
    assert_eq!(code, Some(1));
    assert!(err.contains("not `main`"), "{err}");
}

#[test]
fn a_test_file_that_does_not_compile_is_reported_like_any_other_program() {
    let scratch = Scratch::new("broken");
    let path = scratch.file("func test_one() {\n    let x: int = \"text\"\n}\n");
    let (_, err, code) = skuld_test(&path);
    assert_eq!(code, Some(1));
    assert!(err.contains("E01"), "{err}");
}
