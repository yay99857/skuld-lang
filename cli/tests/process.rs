//! `std/os`, end to end: a built program reads the arguments it was started
//! with and chooses its own exit status.
//!
//! It has to be `build` rather than `run`: `run` owns the command line it
//! passes to the program, so arguments are only observable from an executable
//! the test starts itself.
use std::{env, fs, process::Command};

struct Scratch {
    directory: std::path::PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-os-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }

    /// Build a program and answer with the executable's path.
    fn build(&self, source: &str) -> std::path::PathBuf {
        let program = self.directory.join("main.skuld");
        fs::write(&program, source).expect("program source");
        let binary = self.directory.join("main");
        let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
            .arg("build")
            .arg(&program)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("build the program");
        assert!(
            output.status.success(),
            "build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
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

const PROGRAM: &str = r#"import "std/os"

func main() {
    let given = os.parameters() else problem {
        print(os.describe(problem))
        os.exit(2)
        return
    }
    print("count ${given.len()}")
    for argument in given {
        print("[${argument}]")
    }
    if given.len() == 0 {
        os.exit(3)
    }
}
"#;

#[test]
fn a_program_reads_its_arguments_and_picks_its_status() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("arguments");
    let binary = scratch.build(PROGRAM);

    // An argument with a space in it stays one argument, and the program
    // itself is not among them.
    let output = Command::new(&binary)
        .arg("one")
        .arg("two three")
        .output()
        .expect("run the program");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "count 2\n[one]\n[two three]\n"
    );

    // No arguments at all, and a status the program chose.
    let output = Command::new(&binary).output().expect("run the program");
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "count 0\n");
}

#[test]
fn everything_after_a_double_dash_reaches_the_program_unchanged() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("dashes");
    let binary = scratch.build(PROGRAM);
    // A shell hands these through verbatim; nothing in Skuld interprets them.
    let output = Command::new(&binary)
        .args(["--", "-o", "--help"])
        .output()
        .expect("run the program");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "count 3\n[--]\n[-o]\n[--help]\n"
    );
}
