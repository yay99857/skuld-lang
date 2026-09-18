//! End-to-end tests require clang. Missing clang must fail this suite, not skip it.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    dir: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new(source: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "skuld-e2e-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).expect("fixture directory");
        let path = dir.join("input with spaces.skuld");
        fs::write(&path, source).expect("fixture source");
        Self { dir, source: path }
    }
    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skuld"));
        command
            .arg(action)
            .arg(&self.source)
            .env("TMPDIR", &self.dir);
        command
    }
    fn run(&self) -> Output {
        self.command("run").output().expect("run CLI")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn assert_run(source: &str, expected: &[u8]) {
    let fixture = Fixture::new(source);
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_dir(&fixture.dir).expect("contents").count(),
        1,
        "native build artifacts must be cleaned"
    );
}
#[test]
fn all_three_native_demos() {
    assert_run(
        include_str!("../../examples/hello.skuld"),
        b"Hello from Skuld!\n",
    );
    assert_run(include_str!("../../examples/functions.skuld"), b"42\n");
    assert_run(
        include_str!("../../examples/conditionals.skuld"),
        b"Adult\n",
    );
}
#[test]
fn recursion_forward_calls_and_return_paths() {
    assert_run(
        "func main() { print(fib(10)) }\nfunc fib(n: int) -> int { if n <= 1 { return n } else { return fib(n - 1) + fib(n - 2) } }",
        b"55\n",
    );
    assert_run(
        "func f(x: int) -> string { if x == 0 { return \"zero\" } else if x == 1 { return \"one\" } else { return \"many\" } }\nfunc main() { print(f(0))\nprint(f(1))\nprint(f(3)) }",
        b"zero\none\nmany\n",
    );
    assert_run(
        "func greeting() { print(\"bye\") } func main() { return greeting() }",
        b"bye\n",
    );
}
#[test]
fn scalar_operations_and_numeric_boundaries() {
    assert_run(
        "func main() { print(-9223372036854775808)\nprint(9223372036854775807)\nprint(-7 / 3)\nprint(-7 % 3)\nprint(-9223372036854775808 % -1)\nprint(1.5 + 2.25)\nprint(10.0 / 4.0)\nprint(3 > 2)\nprint(!true)\nprint(1.0 < 2.0) }",
        b"-9223372036854775808\n9223372036854775807\n-2\n-1\n0\n3.75\n2.5\ntrue\nfalse\ntrue\n",
    );
}
#[test]
fn utf8_nul_strings_and_content_equality() {
    assert_run(
        r#"func echo(s: string) -> string { return s }
func main() {
    let s = echo("olá\0世界")
    print(s)
    print(s == "olá\0世界")
    print(s != "olá")
    print("" == "")
    print("\"\\??/ %s \${literal}")
}"#,
        "olá\0世界\ntrue\ntrue\ntrue\n\"\\??/ %s ${literal}\n".as_bytes(),
    );
}
#[test]
fn left_to_right_and_short_circuit_evaluation() {
    assert_run(
        "func pair(a: int, b: int) -> int { return a * 10 + b }\nfunc side() -> bool { print(\"unexpected\")\nreturn true }\nfunc main() {\nvar x = 1\nprint(pair(x, (x = 2)))\nprint(x + (x = 3))\nx += (x = 4)\nprint(x)\nprint(false && side())\nprint(true || side())\nif false { print(1 / 0) }\nvar y = 0\nx = y = 8\nprint(x + y)\n}",
        b"12\n5\n7\nfalse\ntrue\n16\n",
    );
}
#[test]
fn shadows_have_distinct_c_symbols() {
    assert_run(
        "func main() { let age = 27\n{ let age = age + 3\nprint(age) }\nprint(age)\n{ let print = 2 }\nprint(42) }",
        b"30\n27\n42\n",
    );
    assert_run(
        "func print(a: int, b: int) -> int { return a + b }\nfunc main() { if print(20, 22) != 42 { let bad = 1 / 0 } }",
        b"",
    );
}
#[test]
fn runtime_integer_errors_are_defined() {
    for expression in [
        "9223372036854775807 + 1",
        "-9223372036854775808 - 1",
        "9223372036854775807 * 2",
        "-(-9223372036854775808)",
        "-9223372036854775808 / -1",
        "1 / 0",
        "1 % 0",
    ] {
        let fixture = Fixture::new(&format!("func main() {{ print({expression}) }}"));
        let output = fixture.run();
        assert_eq!(output.status.code(), Some(1), "{expression}: {:?}", output);
        assert!(String::from_utf8_lossy(&output.stderr).contains("runtime error:"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("source byte"));
        assert_eq!(fs::read_dir(&fixture.dir).expect("contents").count(), 1);
    }
}
#[test]
fn check_does_not_require_clang_and_run_reports_its_absence() {
    let fixture = Fixture::new("func main() { print(42) }");
    let output = fixture
        .command("check")
        .env("PATH", "")
        .output()
        .expect("check");
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let output = fixture
        .command("run")
        .env("PATH", "")
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("clang was not found"));
    assert_eq!(fs::read_dir(&fixture.dir).expect("contents").count(), 1);
}
#[test]
fn invalid_source_never_reaches_clang() {
    for (source, code) in [
        ("func main() { let age: int = \"hello\" }", "E0102"),
        ("func main() { let x = 1\nx = 2 }", "E0203"),
        ("func main() { usr.greet() }", "E0201"),
        ("func main() {", "E1001"),
        ("@", "E0001"),
    ] {
        let fixture = Fixture::new(source);
        for action in ["check", "run", "emit-c"] {
            let output = fixture
                .command(action)
                .env("PATH", "")
                .output()
                .expect("CLI");
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains(code), "{error}");
            assert!(!error.contains("clang"));
        }
    }
}
/// Build one program's generated C under the sanitizers and run it.
/// Address and leak detection matter as soon as the runtime allocates: a leak
/// or a double free must fail the suite, not pass quietly.
fn sanitized(source: &str, expected: &[u8]) {
    let fixture = Fixture::new(source);
    let output = fixture.command("emit-c").output().expect("emit C");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    sanitized_c(&output.stdout, expected);
}

/// The same, starting from C that has already been emitted. A fixture made of
/// modules has to be compiled where its sources live, so it cannot be copied
/// into a scratch directory first.
fn sanitized_c(emitted: &[u8], expected: &[u8]) {
    let fixture = Fixture::new("func main() {}");
    let c = fixture.dir.join("generated.c");
    let binary = fixture.dir.join("standalone");
    fs::write(&c, emitted).expect("C source");
    // The platform layer is a translation unit of its own, so C emitted by
    // hand is two files rather than one. Compiling both is what a consumer of
    // `emit-c` actually does, and it is what keeps the layer's headers out of
    // the program, where they would collide with what it declares itself.
    let platform = fixture.dir.join("skuld_platform.c");
    let emitted_platform = fixture
        .command("emit-c")
        .arg("--platform")
        .output()
        .expect("emit the platform layer");
    assert!(
        emitted_platform.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted_platform.stderr)
    );
    fs::write(&platform, &emitted_platform.stdout).expect("platform source");
    let status = Command::new("clang")
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-fsanitize=undefined,address",
            "-fno-sanitize-recover=all",
            "-fno-omit-frame-pointer",
        ])
        .arg(&c)
        .arg(&platform)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("clang is required for native tests");
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    // Leak detection is a Linux guarantee and only a Linux one: LeakSanitizer
    // has no Windows implementation, and asking for it there aborts the run
    // before the program prints anything. Address and undefined-behaviour
    // checking still apply on both, so what is lost is one of the three rather
    // than the harness.
    let output = Command::new(binary)
        .env(
            "ASAN_OPTIONS",
            if cfg!(target_os = "linux") {
                "detect_leaks=1"
            } else {
                "detect_leaks=0"
            },
        )
        .output()
        .expect("standalone program");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn emitted_c_compiles_and_runs_independently() {
    sanitized(include_str!("../../examples/functions.skuld"), b"42\n");
}

/// Fixtures that only run on one kind of system, and why.
///
/// A fixture lands here when the program it holds names something the other
/// system genuinely does not have, rather than something not ported yet. The
/// shape follows `portability.rs`'s `TARGET_SPECIFIC`: an opt-out list with
/// the reason written beside each entry, so that a fixture is never skipped
/// quietly and the list can be read as a statement about the language.
const POSIX_ONLY: &[&str] = &[
    // Declares `pipe` and `close` to demonstrate `ptr()` over a fixed array.
    // `pipe` is POSIX and has no Windows equivalent — `_pipe` takes three
    // arguments and a different contract — so the fixture cannot link there.
    "fixed_arrays",
];

/// Whether this fixture runs on the system the tests are running on.
fn runs_here(source: &Path) -> bool {
    if cfg!(unix) {
        return true;
    }
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    !POSIX_ONLY.contains(&stem.as_ref())
}

#[test]
fn every_language_fixture_is_sanitizer_clean() {
    // The golden fixtures are the broadest sample of real programs; running all
    // of them under the sanitizers is what will catch a memory-management
    // mistake the moment the runtime starts allocating.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("tests")
        .join("pass");
    let mut sources: Vec<_> = fs::read_dir(&root)
        .expect("fixture directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "skuld" || e == "sk"))
        .collect();
    sources.sort();
    assert!(!sources.is_empty(), "no fixtures found");
    for source in sources {
        if !runs_here(&source) {
            continue;
        }
        let expected = fs::read(source.with_extension("out")).expect("expected output");
        // Emitted from the fixture's own directory, so that a program made of
        // modules resolves its imports against the real tree.
        let emitted = Command::new(env!("CARGO_BIN_EXE_skuld"))
            .arg("emit-c")
            .arg(&source)
            .output()
            .expect("emit C");
        assert!(
            emitted.status.success(),
            "{} should emit C:\n{}",
            source.display(),
            String::from_utf8_lossy(&emitted.stderr)
        );
        sanitized_c(&emitted.stdout, &expected);
    }
}
#[cfg(unix)]
#[test]
fn clang_failure_is_reported_and_cleaned() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("func main() {}");
    let fake_clang = fixture.dir.join("clang");
    fs::write(
        &fake_clang,
        "#!/bin/sh\necho 'test compiler failure' >&2\nexit 7\n",
    )
    .expect("fake compiler");
    fs::set_permissions(&fake_clang, fs::Permissions::from_mode(0o700)).expect("executable");
    let output = fixture
        .command("run")
        .env("PATH", &fixture.dir)
        .output()
        .expect("CLI");
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("clang failed"));
    assert!(error.contains("test compiler failure"));
    assert_eq!(fs::read_dir(&fixture.dir).expect("contents").count(), 2);
}

#[test]
fn function_hello_and_empty_print() {
    assert_run(
        "func hello() {\n    print()\n}\nfunc main() { hello()\nprint(\"Hello\") }",
        b"\nHello\n",
    );
}

/// `build` writes into the working directory, so every test below pins it to
/// the fixture directory instead of the crate root.
fn build_in(fixture: &Fixture, name: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_skuld"));
    command
        .arg("build")
        .arg(name)
        .current_dir(&fixture.dir)
        .env("TMPDIR", &fixture.dir);
    command.output().expect("build CLI")
}

#[test]
fn build_produces_a_standalone_executable() {
    let fixture = Fixture::new("func main() {\n    print(7)\n}");
    let source = fixture.dir.join("program.skuld");
    fs::copy(&fixture.source, &source).expect("named source");

    let output = build_in(&fixture, "program.skuld");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The artifact survives the CLI and runs on its own, unlike `run`, which
    // keeps nothing.
    let executable = fixture.dir.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    assert!(executable.is_file(), "executable was not created");
    let executed = Command::new(&executable).output().expect("execute build");
    assert_eq!(executed.stdout, b"7\n");
}

#[test]
fn build_rejects_invalid_source_without_writing_an_executable() {
    let fixture = Fixture::new("func main() {\n    let x: int = \"text\"\n}");
    let source = fixture.dir.join("broken.skuld");
    fs::copy(&fixture.source, &source).expect("named source");

    let output = build_in(&fixture, "broken.skuld");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E0102"));
    assert!(
        !fixture.dir.join("broken").exists(),
        "a rejected program must leave no executable"
    );
}

#[test]
// The collision this guards against needs the executable to be named exactly
// like its source, which only happens where an executable carries no
// extension. On Windows `build` writes `noextension.exe`, so the source is
// never in danger and there is nothing here to refuse.
#[cfg(unix)]
fn build_refuses_to_overwrite_the_source() {
    // Without a `.skuld` extension the stem names the source itself.
    let fixture = Fixture::new("func main() {\n    print(1)\n}");
    let source = fixture.dir.join("noextension");
    fs::copy(&fixture.source, &source).expect("named source");

    let output = build_in(&fixture, "noextension");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("would overwrite"));
    assert_eq!(
        fs::read_to_string(&source).expect("source survives"),
        "func main() {\n    print(1)\n}"
    );
}
