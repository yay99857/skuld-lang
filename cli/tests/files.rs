//! `std/fs`, end to end: a Skuld program that writes a file, reads it back
//! and reports what it could not open.
//!
//! The test owns every temporary file it touches and removes them, which is
//! why this lives here and not in `tests/pass`: a language fixture that
//! opened a path would stop being hermetic.
use std::{env, fs, process::Command};

struct Scratch {
    directory: std::path::PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-fs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }

    fn program(&self, source: &str) -> std::path::PathBuf {
        let path = self.directory.join("main.skuld");
        fs::write(&path, source).expect("program source");
        path
    }

    fn path(&self, name: &str) -> String {
        self.directory.join(name).to_string_lossy().into_owned()
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

fn run(program: &std::path::Path) -> (String, String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(program)
        .output()
        .expect("run skuld");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

#[test]
fn a_program_writes_a_file_and_reads_it_back() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("roundtrip");
    let target = scratch.path("notes.txt");
    let program = scratch.program(&format!(
        r#"import "std/fs"

func main() {{
    let wrote = fs.write_text("{target}", "one\ntwo\n") else problem {{
        print(fs.describe(problem))
        return
    }}
    print("wrote ${{wrote}}")
    let text = fs.read_text("{target}") else problem {{
        print(fs.describe(problem))
        return
    }}
    print(text.len())
    let bytes = fs.read_file("{target}") else problem {{
        print(fs.describe(problem))
        return
    }}
    print(bytes[0])
}}
"#
    ));
    let (out, err, ok) = run(&program);
    assert!(ok, "the program failed: {err}");
    assert_eq!(out, "wrote 8\n8\n111\n", "{out}");
    // The file is a real one, with exactly what the program wrote in it.
    assert_eq!(
        fs::read_to_string(scratch.directory.join("notes.txt")).expect("the file"),
        "one\ntwo\n"
    );
}

#[test]
fn a_file_larger_than_one_block_is_read_whole() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("blocks");
    let target = scratch.path("big.txt");
    // Three blocks and a bit: reading stops at the end, not at the buffer.
    let content = "0123456789".repeat(1300);
    fs::write(scratch.directory.join("big.txt"), &content).expect("the fixture file");
    let program = scratch.program(&format!(
        r#"import "std/fs"

func main() {{
    let bytes = fs.read_file("{target}") else problem {{
        print(fs.describe(problem))
        return
    }}
    print(bytes.len())
}}
"#
    ));
    let (out, err, ok) = run(&program);
    assert!(ok, "the program failed: {err}");
    assert_eq!(out, format!("{}\n", content.len()));
}

#[test]
fn a_path_that_cannot_be_opened_is_an_error_and_not_a_crash() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("missing");
    let missing = scratch.path("no/such/file.txt");
    let program = scratch.program(&format!(
        r#"import "std/fs"

func main() {{
    match fs.read_text("{missing}") {{
        Ok(text): print(text)
        Err(problem): print(fs.describe(problem))
    }}
}}
"#
    ));
    let (out, err, ok) = run(&program);
    assert!(ok, "the program failed: {err}");
    assert!(out.contains("could not open"), "{out}");
}
