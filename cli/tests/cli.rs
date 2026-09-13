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
    ] {
        let output = cli().args(args).output().expect("start CLI");
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
    let help = cli().arg("--help").output().expect("start CLI");
    assert!(help.status.success());
    // Every accepted action must be discoverable from the usage line.
    let text = String::from_utf8_lossy(&help.stdout);
    for action in ["lex", "parse", "resolve", "check", "emit-c", "build", "run"] {
        assert!(text.contains(action), "`{action}` missing from: {text}");
    }
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
