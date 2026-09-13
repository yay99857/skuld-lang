use super::*;

fn checked(source: &str) -> TypedProgram {
    skuld_compiler::check(source).expect("the fixture checks")
}

/// The help at the byte offset the `|` marks, which is removed first. The
/// text is checked as it stands, so the cursor is placed in a call that is
/// already complete.
fn help_at(marked: &str) -> Option<Help> {
    let offset = marked.find('|').expect("the fixture marks a cursor");
    let source = marked.replace('|', "");
    at(&source, offset, &checked(&source))
}

/// The help in a text that does **not** check, answered from a text that did.
/// This is the real case: a call is asked about while it is half-written.
fn help_while_typing(complete: &str, marked: &str) -> Option<Help> {
    let offset = marked.find('|').expect("the fixture marks a cursor");
    at(&marked.replace('|', ""), offset, &checked(complete))
}

#[test]
fn finds_the_call_the_cursor_is_inside() {
    let call = call_at("f(a, b", 6).expect("a call");
    assert_eq!(call.open, 1);
    assert_eq!(call.argument, 1);
}

#[test]
fn a_comma_inside_a_nested_call_belongs_to_that_call() {
    let call = call_at("outer(inner(1, 2), ", 19).expect("a call");
    assert_eq!(call.open, 5);
    assert_eq!(call.argument, 1, "the nested comma is not the outer one's");
}

#[test]
fn a_bracket_or_a_brace_ends_the_argument_list() {
    // Inside an array literal and inside a lambda body, the cursor is no
    // longer writing an argument.
    assert_eq!(call_at("f([1, 2", 7), None);
    assert_eq!(call_at("sort((a: int, b: int) -> int { return ", 38), None);
}

#[test]
fn a_parenthesis_in_a_string_or_a_comment_opens_nothing() {
    assert_eq!(call_at("let s = \"f(a, b\"", 16), None);
    assert_eq!(call_at("// f(a\n", 7), None);
    // An escaped quote does not end the string early.
    assert_eq!(call_at("let s = \"\\\"(x\"", 14), None);
}

#[test]
fn a_parenthesised_argument_does_not_hide_the_call_it_is_in() {
    // The inner `(` opens no call of its own, so a cursor after it is still
    // writing `print`'s argument. A prelude binding was never declared in a
    // file, so its signature is recovered from the one line that describes it.
    let source = "func main() {\n    print((1 + 2))\n}\n";
    assert_eq!(
        at(source, 31, &checked(source)).map(|help| help.label),
        Some("func print(value) -> void".to_owned())
    );
}

#[test]
fn reports_a_declared_function_with_its_parameter_names() {
    let help = help_at("func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, |2))\n}\n")
        .expect("help inside the call");
    assert_eq!(help.label, "add(a: int, b: int) -> int");
    assert_eq!(help.active, Some(1));
    let (start, end) = help.parameters[1];
    assert_eq!(&help.label[start..end], "b: int");
}

#[test]
fn a_void_function_has_no_arrow() {
    let help = help_at(
        "func shout(text: string) {\n    print(text)\n}\n\nfunc main() {\n    shout(|\"hi\")\n}\n",
    )
    .expect("help inside the call");
    assert_eq!(help.label, "shout(text: string)");
    assert_eq!(help.active, Some(0));
}

#[test]
fn a_method_is_described_the_way_it_was_declared() {
    let help = help_at(
        "class Greeter {\n    name: string\n    greet(times: int) {\n        print(this.name)\n    }\n}\n\nfunc main() {\n    let g = new Greeter(name: \"a\")\n    g.greet(|2)\n}\n",
    )
    .expect("help inside the call");
    assert_eq!(help.label, "greet(times: int)");
    assert_eq!(help.active, Some(0));
}

#[test]
fn a_builtin_method_comes_from_its_description() {
    let help = help_at("func main() {\n    var xs = [1, 2]\n    xs.insert(|0, 5)\n}\n")
        .expect("help inside the call");
    assert_eq!(help.label, "insert(int, int) -> void");
    assert_eq!(help.parameters.len(), 2);
    assert_eq!(help.active, Some(0));
}

#[test]
fn a_construction_lists_the_fields_and_highlights_none() {
    let help = help_at(
        "class User {\n    name: string\n    age: int\n}\n\nfunc main() {\n    let u = new User(|name: \"a\", age: 1)\n    print(u.name)\n}\n",
    )
    .expect("help inside the construction");
    assert_eq!(help.label, "User(name: string, age: int)");
    // Fields are given by name, so no position is the active one.
    assert_eq!(help.active, None);
}

#[test]
fn a_call_with_no_parameters_marks_none_active() {
    let help =
        help_at("func now() -> int {\n    return 1\n}\n\nfunc main() {\n    print(now(|))\n}\n")
            .expect("help inside the call");
    assert_eq!(help.label, "now() -> int");
    assert!(help.parameters.is_empty());
    assert_eq!(help.active, None);
}

#[test]
fn a_half_written_call_is_answered_from_the_last_good_check() {
    let complete = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n}\n";
    let typing = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, |\n}\n";
    let help = help_while_typing(complete, typing).expect("help while typing");
    assert_eq!(help.label, "add(a: int, b: int) -> int");
    assert_eq!(help.active, Some(1));
}

#[test]
fn too_many_arguments_still_point_at_a_parameter() {
    let complete = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n}\n";
    let typing = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2, |\n}\n";
    let help = help_while_typing(complete, typing).expect("help while typing");
    assert_eq!(help.active, Some(1), "the last parameter, not one past it");
}

#[test]
fn a_type_holding_a_comma_is_one_parameter() {
    assert_eq!(
        split_arguments("Result<int, string>, int"),
        ["Result<int, string>", "int"]
    );
    assert!(split_arguments("  ").is_empty());
}

#[test]
fn the_parameter_offsets_are_utf16_units() {
    // The label is measured the way the protocol measures it, which differs
    // from bytes the moment a name is not ASCII.
    let help = assemble("ré", &["x: int".to_owned()], "", Some(0));
    let (start, end) = help.parameters[0];
    assert_eq!(help.label, "ré(x: int)");
    assert_eq!((start, end), (3, 9));
}
