use std::{path::PathBuf, process::Command};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_skuld"))
}
fn example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/hello.skuld")
}

#[test]
fn prints_hello_tokens() {
    let output = cli().arg("lex").arg(example()).output().expect("start CLI");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "Function\nIdentifier(\"main\")\nLeftParen\nRightParen\nLeftBrace\nIdentifier(\"print\")\nLeftParen\nString(\"Hello from Skuld!\")\nRightParen\nRightBrace\nEof\n"
    );
}

#[test]
fn missing_file_is_friendly() {
    let output = cli()
        .args(["lex", "nonexistent-skuld-test-directory/input.skuld"])
        .output()
        .expect("start CLI");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}

#[test]
fn invalid_arguments() {
    for args in [
        vec![],
        vec!["run"],
        vec!["lex"],
        vec!["unknown-command", "x.skuld"],
        vec!["build"],
        vec!["lex", "x", "y"],
        // Linker arguments belong to `build` and `run`, and only name libraries.
        vec!["lex", "x.skuld", "-lm"],
        vec!["run", "x.skuld", "-O2"],
        vec!["run", "x.skuld", "-l"],
        // `-o` chooses where `build` writes, and nothing else writes a file.
        vec!["run", "x.skuld", "-o", "program"],
        vec!["check", "x.skuld", "-o", "program"],
        vec!["build", "x.skuld", "-o"],
    ] {
        let output = cli().args(&args).output().expect("start CLI");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("usage:"), "{args:?}: {stderr}");
        // A misuse names what to read next rather than dumping the whole help.
        assert!(stderr.contains("--help"), "{args:?}: {stderr}");
    }
    // A near miss is worth a suggestion; a wild guess is not.
    let typo = cli()
        .args(["buidl", "x.skuld"])
        .output()
        .expect("start CLI");
    assert!(
        String::from_utf8_lossy(&typo.stderr).contains("did you mean `build`?"),
        "{}",
        String::from_utf8_lossy(&typo.stderr)
    );
    for flag in ["--help", "-h"] {
        let help = cli().arg(flag).output().expect("start CLI");
        assert!(help.status.success());
        // Every accepted command and option must be discoverable from the help,
        // which goes to stdout: it is an answer, not an error.
        let text = String::from_utf8_lossy(&help.stdout);
        assert!(help.stderr.is_empty());
        for item in [
            "lex",
            "parse",
            "resolve",
            "check",
            "fmt",
            "emit-c",
            "build",
            "run",
            "-o",
            "-l",
            "-L",
            "--check",
            "--",
            "exit codes",
        ] {
            assert!(text.contains(item), "`{item}` missing from: {text}");
        }
    }
    for flag in ["--version", "-V"] {
        let version = cli().arg(flag).output().expect("start CLI");
        assert!(version.status.success());
        let text = String::from_utf8_lossy(&version.stdout);
        assert!(text.starts_with("skuld "), "{text}");
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
    }
    // Help answers even when the rest of the command line is nonsense, so a
    // reader who is lost can always get it.
    let rescued = cli()
        .args(["nonsense", "--help", "-o"])
        .output()
        .expect("start CLI");
    assert!(rescued.status.success());
    assert!(String::from_utf8_lossy(&rescued.stdout).contains("commands:"));
}

#[test]
fn options_may_come_before_the_file_and_after_a_separator() {
    let fixture = std::env::temp_dir().join(format!("skuld-order-{}.skuld", std::process::id()));
    std::fs::write(&fixture, "func main() { print(7) }").expect("write fixture");
    // A flag before the file is the same command as a flag after it. The
    // linker flag is `-L`, not `-lm`: this is about where an option may sit,
    // not about linking, and `libm` is a Unix arrangement — the MSVC toolchain
    // keeps the maths in the C runtime and has no `m.lib` to find. Naming a
    // search directory asks nothing of either system.
    for args in [
        vec!["run".into(), fixture.display().to_string()],
        vec!["run".into(), "-L.".into(), fixture.display().to_string()],
        vec!["run".into(), "--".into(), fixture.display().to_string()],
    ] {
        let output = cli().args(&args).output().expect("start CLI");
        assert!(output.status.success(), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n", "{args:?}");
    }
    std::fs::remove_file(fixture).expect("remove fixture");
}

#[test]
fn invalid_source_has_diagnostic_without_partial_tokens() {
    let path = std::env::temp_dir().join(format!("skuld-invalid-{}.skuld", std::process::id()));
    std::fs::write(&path, "function @").expect("write fixture");
    let output = cli().arg("lex").arg(&path).output().expect("start CLI");
    std::fs::remove_file(path).expect("remove fixture");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[E0001]: invalid character `@`"));
    assert!(stderr.contains(":1:10"));
}

#[test]
fn parse_prints_ast_for_examples() {
    for filename in ["hello.skuld", "functions.skuld"] {
        let output = cli()
            .arg("parse")
            .arg(example().with_file_name(filename))
            .output()
            .expect("start CLI");
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.starts_with("Program {"));
        assert!(stdout.contains("FunctionDecl"));
    }
}

#[test]
fn parse_reports_syntax_location_without_partial_ast() {
    let path =
        std::env::temp_dir().join(format!("skuld-parse-invalid-{}.skuld", std::process::id()));
    std::fs::write(&path, "func main() {\n    let x =\n}\n").expect("write fixture");
    let output = cli().arg("parse").arg(&path).output().expect("start CLI");
    std::fs::remove_file(path).expect("remove fixture");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[E1001]: expected an expression, found `}`"));
    assert!(stderr.contains(":3:1"));
}

#[test]
fn resolve_prints_symbols_for_examples() {
    let output = cli()
        .arg("resolve")
        .arg(example())
        .output()
        .expect("start CLI");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("Resolution {"));
    assert!(text.contains("Print"));
}

#[test]
fn resolve_errors_suppress_partial_output() {
    let path = std::env::temp_dir().join(format!(
        "skuld-resolve-invalid-{}.skuld",
        std::process::id()
    ));
    for (source, code) in [
        ("func main() { missing() }", "E0201"),
        ("func main( {}", "E1001"),
        ("@", "E0001"),
    ] {
        std::fs::write(&path, source).expect("fixture");
        let output = cli().arg("resolve").arg(&path).output().expect("start CLI");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(code));
    }
    std::fs::remove_file(path).expect("cleanup");
}

/// A build reads only what is under its own root. Path syntax is checked by
/// the compiler, but a symlink is resolved by the filesystem and can point
/// anywhere, so the loader is what has to hold the line.
#[cfg(unix)]
#[test]
fn a_module_linked_out_of_the_program_root_is_not_read() {
    use std::{env, fs, os::unix::fs::symlink};

    let base = env::temp_dir().join(format!("skuld-root-{}", std::process::id()));
    let root = base.join("root");
    let outside = base.join("outside").join("secret");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&root).expect("root");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(
        outside.join("s.skuld"),
        "pub func leaked() -> int {\n    return 42\n}\n",
    )
    .expect("module source");
    symlink(&outside, root.join("lib")).expect("symlink");
    let entry = root.join("main.skuld");
    fs::write(
        &entry,
        "import \"lib\"\nfunc main() { print(lib.leaked()) }\n",
    )
    .expect("entry source");

    let output = cli().arg("check").arg(&entry).output().expect("start CLI");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a linked-out module must not compile, got:\n{stderr}"
    );
    assert!(
        stderr.contains("outside the program root"),
        "expected a containment diagnostic, got:\n{stderr}"
    );
    let _ = fs::remove_dir_all(&base);
}

/// An entry named without a directory still has a program root, and it is the
/// working directory. This is the shape every `cd` into a project produces —
/// `skuld check main.skuld` — and it used to fail for any program with an
/// import while the same file named `./main.skuld` compiled.
#[test]
fn a_bare_entry_name_still_resolves_its_imports() {
    use std::{env, fs};

    let root = env::temp_dir().join(format!("skuld-bare-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("greeting")).expect("module directory");
    fs::write(
        root.join("greeting").join("greeting.skuld"),
        "pub func hello() -> string {
    return \"hi\"
}
",
    )
    .expect("module source");
    fs::write(
        root.join("main.skuld"),
        "import \"greeting\"
func main() { print(greeting.hello()) }
",
    )
    .expect("entry source");

    let output = cli()
        .current_dir(&root)
        .args(["check", "main.skuld"])
        .output()
        .expect("start CLI");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "a bare entry name must resolve its imports, got:
{stderr}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn fmt_formats_in_place_and_check_detects_drift() {
    let path = std::env::temp_dir().join(format!("skuld-fmt-{}.skuld", std::process::id()));
    let unformatted = "func add(a: int, b: int): int {\nreturn a + b\n}\n";
    std::fs::write(&path, unformatted).expect("write unformatted");

    // --check fails when file needs formatting
    let check_fail = cli()
        .args(["fmt", "--check"])
        .arg(&path)
        .output()
        .expect("start CLI");
    assert_eq!(check_fail.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&path).expect("read"), unformatted);

    // fmt without --check formats in place
    let fmt_run = cli().arg("fmt").arg(&path).output().expect("start CLI");
    assert!(fmt_run.status.success());
    let formatted = std::fs::read_to_string(&path).expect("read formatted");
    assert_eq!(
        formatted,
        "func add(a: int, b: int) -> int {\n    return a + b\n}\n"
    );

    // --check now passes
    let check_ok = cli()
        .args(["fmt", "--check"])
        .arg(&path)
        .output()
        .expect("start CLI");
    assert!(check_ok.status.success());

    // formatting invalid code fails and does not overwrite
    let invalid = "func main() {\n    let =\n}\n";
    std::fs::write(&path, invalid).expect("write invalid");
    let fmt_invalid = cli().arg("fmt").arg(&path).output().expect("start CLI");
    assert_eq!(fmt_invalid.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&fmt_invalid.stderr);
    assert!(
        stderr.contains("error[E1001]"),
        "expected diagnostic: {stderr}"
    );
    assert_eq!(std::fs::read_to_string(&path).expect("read"), invalid);

    std::fs::remove_file(path).expect("cleanup");
}

#[test]
fn sk_extension_is_supported_for_source_and_modules() {
    let dir = std::env::temp_dir().join(format!("skuld-sk-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("math")).expect("create math module dir");

    let mod_file = dir.join("math").join("ops.sk");
    std::fs::write(
        &mod_file,
        "pub func double(x: int) -> int { return x * 2 }\n",
    )
    .expect("write ops.sk");

    let main_file = dir.join("main.sk");
    std::fs::write(
        &main_file,
        "import \"math\"\nfunc main() {\n    print(math.double(21))\n}\n",
    )
    .expect("write main.sk");

    let check = cli()
        .arg("check")
        .arg(&main_file)
        .output()
        .expect("run check");
    assert!(
        check.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let run = cli()
        .arg("run")
        .arg(&main_file)
        .output()
        .expect("run program");
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");

    let _ = std::fs::remove_dir_all(dir);
}
