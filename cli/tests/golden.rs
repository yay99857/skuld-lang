//! Language golden fixtures from the repository root `tests/` directory.
//!
//! `pass/` programs must run and match their `.out` stdout exactly. `fail/`
//! programs must be rejected by `check` with every diagnostic code in `.err`.
//! `trap/` programs must compile but abort at runtime with the `.err` message.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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

fn skuld() -> Command {
    Command::new(env!("CARGO_BIN_EXE_skuld"))
}

fn fixtures(category: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("tests")
        .join(category);
    let mut sources: Vec<_> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", root.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "skuld" || e == "sk"))
        .collect();
    sources.sort();
    assert!(!sources.is_empty(), "no fixtures in {}", root.display());
    sources
}

fn expectation(source: &Path, extension: &str) -> String {
    let path = source.with_extension(extension);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// `run` and `trap` fixtures need clang; checking does not.
fn clang_available() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn pass_fixtures_run_and_match_expected_stdout() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    for source in fixtures("pass") {
        if !runs_here(&source) {
            continue;
        }
        let output = skuld().arg("run").arg(&source).output().expect("run skuld");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{} should run cleanly, got {}:\n{stderr}",
            source.display(),
            output.status
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expectation(&source, "out"),
            "unexpected stdout for {}",
            source.display()
        );
    }
}

#[test]
fn fail_fixtures_are_rejected_with_expected_codes() {
    for source in fixtures("fail") {
        let output = skuld()
            .arg("check")
            .arg(&source)
            .output()
            .expect("check skuld");
        assert!(
            !output.status.success(),
            "{} should be rejected",
            source.display()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        for code in expectation(&source, "err").split_whitespace() {
            assert!(
                stderr.contains(code),
                "{} should report {code}, got:\n{stderr}",
                source.display()
            );
        }
        assert!(
            output.stdout.is_empty(),
            "{} must not emit partial output",
            source.display()
        );
    }
}

#[test]
fn trap_fixtures_compile_but_abort_at_runtime() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    for source in fixtures("trap") {
        let checked = skuld()
            .arg("check")
            .arg(&source)
            .output()
            .expect("check skuld");
        assert!(
            checked.status.success(),
            "{} should pass static checking",
            source.display()
        );
        let output = skuld().arg("run").arg(&source).output().expect("run skuld");
        assert!(
            !output.status.success(),
            "{} should abort at runtime",
            source.display()
        );
        let message = expectation(&source, "err");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(message.trim()),
            "{} should report `{}`, got:\n{stderr}",
            source.display(),
            message.trim()
        );
    }
}
