use super::*;

fn folds_of(source: &str) -> Vec<(usize, usize, Option<&'static str>)> {
    let positions = Positions::new(source);
    folds(source, &positions)
        .into_iter()
        .map(|fold| (fold.start_line, fold.end_line, fold.kind))
        .collect()
}

#[test]
fn folds_a_body_from_its_brace_to_its_close() {
    assert_eq!(folds_of("func main() {\n    print(1)\n}\n"), [(0, 2, None)]);
}

#[test]
fn a_one_line_block_is_not_a_fold() {
    assert_eq!(folds_of("func main() { print(1) }\n"), []);
}

#[test]
fn nested_blocks_each_fold() {
    let source = "func main() {\n    if true {\n        print(1)\n    }\n}\n";
    assert_eq!(folds_of(source), [(0, 4, None), (1, 3, None)]);
}

#[test]
fn an_array_literal_over_several_lines_folds() {
    let source =
        "func main() {\n    var xs = [\n        1,\n        2\n    ]\n    print(xs.len())\n}\n";
    assert_eq!(folds_of(source), [(0, 6, None), (1, 4, None)]);
}

#[test]
fn a_run_of_imports_folds_as_one() {
    let source = "import \"a\"\nimport \"b\"\nimport \"c\"\n\nfunc main() {\n}\n";
    let found = folds_of(source);
    assert!(found.contains(&(0, 2, Some("imports"))), "{found:?}");
}

#[test]
fn a_single_import_is_not_a_fold() {
    let source = "import \"a\"\n\nfunc main() {\n}\n";
    assert_eq!(folds_of(source), [(2, 3, None)]);
}

#[test]
fn a_run_of_whole_line_comments_folds() {
    let source = "// one\n// two\nfunc main() {\n    let x = 1 // not a run\n    print(x)\n}\n";
    let found = folds_of(source);
    assert!(found.contains(&(0, 1, Some("comment"))), "{found:?}");
    // The trailing comment is on a line with code, so it folds nothing.
    assert_eq!(
        found
            .iter()
            .filter(|(_, _, kind)| *kind == Some("comment"))
            .count(),
        1
    );
}

#[test]
fn unbalanced_text_folds_what_it_can() {
    // The editor holds text like this on every other keystroke.
    let source = "func main() {\n    if true {\n        print(1)\n}\n";
    assert_eq!(folds_of(source), [(1, 3, None)]);
}
