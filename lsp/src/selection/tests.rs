use super::*;

/// The chain at the offset the `|` marks, as the text each step selects.
fn chain_at(marked: &str) -> Vec<String> {
    let offset = marked.find('|').expect("the fixture marks a cursor");
    let source = marked.replace('|', "");
    chain(&source, offset)
        .into_iter()
        .map(|span| source[span.start..span.end].to_owned())
        .collect()
}

#[test]
fn expands_from_a_word_through_its_brackets_to_the_file() {
    let steps = chain_at("func main() {\n    print(co|unt)\n}\n");
    assert_eq!(
        steps,
        [
            "count",
            // The call's contents are the word itself, so that step is not
            // offered twice: the next one adds the parentheses.
            "(count)",
            "\n    print(count)\n",
            "{\n    print(count)\n}",
            "func main() {\n    print(count)\n}\n",
        ]
    );
}

#[test]
fn every_step_contains_the_one_before_it() {
    let source = "func main() {\n    var xs = [1, 2]\n    print(xs.len())\n}\n";
    let offset = source.find("len").expect("a cursor") + 1;
    let steps = chain(source, offset);
    for pair in steps.windows(2) {
        assert!(
            pair[0].start >= pair[1].start && pair[0].end <= pair[1].end,
            "{pair:?} does not grow"
        );
    }
    assert_eq!(steps.last().map(|span| span.end), Some(source.len()));
}

#[test]
fn a_string_expands_to_its_contents_before_its_quotes() {
    let steps = chain_at("func main() {\n    print(\"he|llo\")\n}\n");
    assert_eq!(steps[0], "hello");
    assert_eq!(steps[1], "\"hello\"");
}

#[test]
fn a_position_in_empty_space_still_reaches_the_file() {
    let steps = chain_at("func main() {\n|\n}\n");
    assert_eq!(steps.last().unwrap(), &"func main() {\n\n}\n");
}

#[test]
fn unbalanced_text_still_answers() {
    let steps = chain_at("func main() {\n    print(1|\n");
    assert_eq!(steps[0], "1");
    assert_eq!(steps.last().unwrap(), &"func main() {\n    print(1\n");
}
