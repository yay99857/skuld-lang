//! `skuld test`: finding the tests in a file, running them, and saying what
//! happened.
//!
//! Three decisions are worth knowing before reading the code.
//!
//! **A test is a function, not an annotation.** A top-level `func test_*()`
//! taking nothing and returning nothing is a test. Skuld has no annotations
//! and this milestone is not the place to introduce them, and a naming rule
//! needs nothing from the compiler.
//!
//! **The suite is one program.** The file is compiled once, with a `main`
//! written here that calls each test in source order. Skuld has no
//! recoverable panic — a failed assertion or a trap ends the process — so the
//! first failure stops the run, and the report says which tests never got to
//! start. Compiling once per test would make each one independent and would
//! also mean a clang invocation per test; that trade can be revisited when a
//! suite is large enough to care.
//!
//! **The runner reads the program's output.** Each test that returns prints a
//! line through `std/testing`, flushed, so the record survives even a trap
//! that kills the process. Everything else the program printed is the test's
//! own output and is shown with the failure.

use skuld_compiler::ast::Program;

/// The marker `std/testing` prints, and this module reads.
const OK: &str = "skuld-test: ok ";
const FAIL: &str = "skuld-test: fail ";

/// What a test file turned out to hold.
#[derive(Debug)]
pub struct Suite {
    pub names: Vec<String>,
    /// The source with the synthetic entry point, ready to compile.
    pub source: String,
}

/// The tests in a parsed file, in source order.
///
/// A name that looks like a test but cannot be called as one is an error
/// rather than something skipped quietly: a test that never runs because of
/// its signature is the worst possible outcome.
pub fn discover(program: &Program, source: &str) -> Result<Suite, String> {
    if program.functions.iter().any(|f| f.name.text == "main") {
        return Err(
            "a test file declares tests, not `main`; the runner writes the entry point".to_owned(),
        );
    }
    let mut names = Vec::new();
    for function in &program.functions {
        if !function.name.text.starts_with("test_") {
            continue;
        }
        if !function.parameters.is_empty() {
            return Err(format!(
                "`{}` takes parameters, so it cannot be run as a test",
                function.name.text
            ));
        }
        if function.return_type.is_some() {
            return Err(format!(
                "`{}` returns a value, so it cannot be run as a test",
                function.name.text
            ));
        }
        names.push(function.name.text.clone());
    }
    if names.is_empty() {
        return Err("no tests here: a test is a top-level `func test_...()`".to_owned());
    }
    let imported = program
        .imports
        .iter()
        .any(|import| import.path == "std/testing");
    Ok(Suite {
        source: harness(source, &names, imported),
        names,
    })
}

/// The file, plus the entry point that runs its tests.
///
/// The import goes on the first line when the file does not already have it:
/// an import must precede every declaration, and a comment is not one, so the
/// top of the file is always a legal place for it.
fn harness(source: &str, names: &[String], imported: bool) -> String {
    let mut out = String::new();
    if !imported {
        out.push_str("import \"std/testing\"\n");
    }
    out.push_str(source);
    if !source.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\nfunc main() {\n");
    for name in names {
        out.push_str(&format!("    {name}()\n"));
        out.push_str(&format!("    testing.passed(\"{name}\")\n"));
    }
    out.push_str("}\n");
    out
}

/// What the run amounted to: a report to print, and whether it passed.
pub struct Outcome {
    pub report: String,
    pub passed: bool,
}

/// Read the program's output back into a report.
///
/// Tests that printed their marker passed. The first one that did not is the
/// failure, and everything the program printed that is not a marker belongs
/// to it. A suite where every test passed and the program still failed is
/// reported too, since something after the last test went wrong.
pub fn report(names: &[String], output: &str, code: u8) -> Outcome {
    let mut passed = Vec::new();
    let mut failure: Option<String> = None;
    let mut printed = Vec::new();
    for line in output.lines() {
        if let Some(name) = line.strip_prefix(OK) {
            passed.push(name.to_owned());
        } else if let Some(reason) = line.strip_prefix(FAIL) {
            failure = Some(reason.to_owned());
        } else {
            printed.push(line);
        }
    }
    let mut report = String::new();
    // The first test without a marker is the one that stopped the run; every
    // test after it never started.
    let mut stopped = false;
    for name in names {
        if passed.iter().any(|done| done == name) {
            report.push_str(&format!("ok   {name}\n"));
        } else if !stopped && (failure.is_some() || code != 0) {
            report.push_str(&format!("FAIL {name}\n"));
            stopped = true;
        } else {
            report.push_str(&format!("     {name} (not run)\n"));
        }
    }
    if let Some(reason) = &failure {
        report.push_str(&format!("\n{reason}\n"));
    }
    if !printed.is_empty() {
        report.push_str(&format!("\noutput:\n{}\n", printed.join("\n")));
    }
    let all = passed.len() == names.len() && code == 0;
    report.push_str(&format!(
        "\n{} of {} tests passed\n",
        passed.len(),
        names.len()
    ));
    if !all && failure.is_none() && code != 0 {
        report.push_str(&format!("the program ended with status {code}\n"));
    }
    Outcome {
        report,
        passed: all,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skuld_compiler::parse;

    fn parsed(source: &str) -> Program {
        parse(source).program.expect("the fixture must parse")
    }

    const FILE: &str = "import \"std/testing\"\n\nfunc helper() -> int {\n    return 1\n}\n\nfunc test_one() {\n    testing.check(true, \"yes\")\n}\n\nfunc test_two() {\n}\n";

    #[test]
    fn tests_are_found_in_source_order_and_helpers_are_left_alone() {
        let suite = discover(&parsed(FILE), FILE).expect("a suite");
        assert_eq!(suite.names, vec!["test_one", "test_two"]);
        assert!(
            suite.source.ends_with(
                "\nfunc main() {\n    test_one()\n    testing.passed(\"test_one\")\n    test_two()\n    testing.passed(\"test_two\")\n}\n"
            ),
            "{}",
            suite.source
        );
        // The file already imports the library, so nothing was added.
        assert!(
            suite
                .source
                .starts_with("import \"std/testing\"\n\nfunc helper")
        );
    }

    #[test]
    fn the_import_is_added_only_when_it_is_missing() {
        let source = "// a comment first\nfunc test_one() {\n}\n";
        let suite = discover(&parsed(source), source).expect("a suite");
        assert!(
            suite
                .source
                .starts_with("import \"std/testing\"\n// a comment first\n")
        );
    }

    #[test]
    fn a_name_that_cannot_be_run_as_a_test_is_an_error() {
        let with_parameter = "func test_one(value: int) {\n}\n";
        assert!(
            discover(&parsed(with_parameter), with_parameter)
                .unwrap_err()
                .contains("takes parameters")
        );
        let with_return = "func test_one() -> int {\n    return 1\n}\n";
        assert!(
            discover(&parsed(with_return), with_return)
                .unwrap_err()
                .contains("returns a value")
        );
        let with_main = "func main() {\n}\nfunc test_one() {\n}\n";
        assert!(
            discover(&parsed(with_main), with_main)
                .unwrap_err()
                .contains("not `main`")
        );
        let empty = "func helper() {\n}\n";
        assert!(
            discover(&parsed(empty), empty)
                .unwrap_err()
                .contains("no tests here")
        );
    }

    #[test]
    fn a_clean_run_reports_every_test() {
        let names = vec!["test_one".to_owned(), "test_two".to_owned()];
        let outcome = report(
            &names,
            "skuld-test: ok test_one\nskuld-test: ok test_two\n",
            0,
        );
        assert!(outcome.passed);
        assert_eq!(
            outcome.report,
            "ok   test_one\nok   test_two\n\n2 of 2 tests passed\n"
        );
    }

    #[test]
    fn a_failure_names_the_test_and_keeps_what_it_printed() {
        let names = vec![
            "test_one".to_owned(),
            "test_two".to_owned(),
            "test_three".to_owned(),
        ];
        let outcome = report(
            &names,
            "skuld-test: ok test_one\nhalfway\nskuld-test: fail sum: expected 3, got 4\n",
            1,
        );
        assert!(!outcome.passed);
        assert!(
            outcome.report.contains("ok   test_one\n"),
            "{}",
            outcome.report
        );
        assert!(
            outcome.report.contains("FAIL test_two\n"),
            "{}",
            outcome.report
        );
        assert!(
            outcome.report.contains("test_three (not run)"),
            "{}",
            outcome.report
        );
        assert!(outcome.report.contains("sum: expected 3, got 4"));
        assert!(outcome.report.contains("halfway"));
        assert!(outcome.report.contains("1 of 3 tests passed"));
    }

    #[test]
    fn a_trap_with_no_marker_is_still_a_failure() {
        let names = vec!["test_one".to_owned()];
        // A runtime trap kills the process without printing a marker.
        let outcome = report(&names, "", 134);
        assert!(!outcome.passed);
        assert!(outcome.report.contains("FAIL test_one"));
        assert!(outcome.report.contains("status 134"));
    }
}
