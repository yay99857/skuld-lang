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
fn run_forwards_what_follows_args_and_returns_the_program_status() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("forward");
    let program = scratch.directory.join("main.skuld");
    fs::write(&program, PROGRAM).expect("program source");
    // `--args` ends the compiler's command line: `-o` after it belongs to the
    // program, and would otherwise be read as where to write an executable.
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&program)
        .arg("--args")
        .arg("-o")
        .arg("two three")
        .output()
        .expect("run skuld");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "count 2\n[-o]\n[two three]\n"
    );

    // No arguments, and the program's own exit status reaches the shell.
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&program)
        .output()
        .expect("run skuld");
    assert_eq!(output.status.code(), Some(3));
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

/// Running another program and reading what it wrote. The programs used here
/// are the ones every POSIX system has, and none of them is a shell: `run`
/// does no quoting, splitting or expansion, which is what the last case
/// checks.
// The program the Unix-only `run` test compiles. It goes with that test: with
// `-D warnings` in CI an unused constant is an error rather than a warning.
#[cfg(unix)]
const RUNNER: &str = r#"
import "std/os"

func main() {
    let hello = os.run("echo", ["one", "two three"]) else problem {
        print(os.describe_run(problem))
        os.exit(1)
        return
    }
    print("status ${hello.status}")
    print("[${hello.text}]")
    print("truncated ${hello.truncated}")

    // A status other than zero comes back as itself rather than as a failure:
    // a program that answers "no" has still run.
    let refused = os.run("false", []) else problem {
        print(os.describe_run(problem))
        os.exit(1)
        return
    }
    print("false says ${refused.status}")

    // A program that does not exist is the shell's 127, not a crash.
    let missing = os.run("skuld-no-such-program", []) else problem {
        print(os.describe_run(problem))
        os.exit(1)
        return
    }
    print("missing says ${missing.status}")

    // No shell means no expansion: the asterisk is a character, not a glob.
    let literal = os.run("echo", ["*"]) else problem {
        print(os.describe_run(problem))
        os.exit(1)
        return
    }
    print("[${literal.text}]")

    if let home = os.environment("SKULD_TEST_VARIABLE") {
        print("variable ${home}")
    } else {
        print("variable missing")
    }
    if let absent = os.environment("SKULD_TEST_ABSENT") {
        print("unexpected ${absent}")
    } else {
        print("absent is nothing")
    }
}
"#;

#[test]
// Unlike the rest of this suite's Unix gates, this one marks work that is
// owed rather than a difference that is permanent. `os.run` is still a
// `fork`/`execvp`/`waitpid` pipeline, and none of those exists on Windows;
// moving it into the platform layer, onto `CreateProcess`, is what removes
// this attribute. The program the test runs is POSIX too — `echo` is a `cmd`
// builtin rather than an executable there, and 127 is not how that system
// reports a missing program — so the fixture needs its own expectations on
// the other side, not just a ported library.
#[cfg(unix)]
fn a_program_runs_another_and_reads_what_it_wrote() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("run");
    let binary = scratch.build(RUNNER);
    let output = Command::new(&binary)
        .env("SKULD_TEST_VARIABLE", "a value with spaces")
        .env_remove("SKULD_TEST_ABSENT")
        .output()
        .expect("run the program");
    assert!(
        output.status.success(),
        "the program failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "status 0\n[one two three\n]\ntruncated false\nfalse says 1\nmissing says 127\n[*\n]\nvariable a value with spaces\nabsent is nothing\n"
    );
}
