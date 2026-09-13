use super::*;
use skuld_compiler::{check_program, module::NoModules};

/// The last good check of a single-file document, which is what the server
/// caches and completion answers from.
fn checked(source: &str) -> TypedProgram {
    check_program("main.skuld", source, &mut NoModules).unwrap_or_else(|errors| {
        panic!("the fixture must check:\n{}", errors.render());
    })
}

fn labels(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

fn detail_of<'a>(items: &'a [Item], label: &str) -> Option<&'a str> {
    items
        .iter()
        .find(|item| item.label == label)
        .and_then(|item| item.detail.as_deref())
}

/// Complete at the end of `source`, as if the cursor sat there.
fn at_end(program: &str, typed: &TypedProgram) -> Vec<Item> {
    at(program, program.len(), Some(typed))
}

#[test]
fn a_dot_after_an_enum_offers_its_variants() {
    let program = "enum Command {\n    Quit\n    Echo(string)\n    Move(int)\n}\nfunc main() { }\n";
    let typed = checked(program);
    let typing = format!("{program}// Command.");
    let items = at(&typing, typing.len(), Some(&typed));
    assert_eq!(labels(&items), ["Quit", "Echo", "Move"]);
    // A variant that carries a payload says so, since `Echo` alone would not
    // tell the reader a string is expected.
    assert_eq!(detail_of(&items, "Echo"), Some("Echo(string)"));
    assert_eq!(detail_of(&items, "Quit"), None);
    assert!(items.iter().all(|item| item.kind == kind::ENUM_MEMBER));
}

#[test]
fn a_dot_after_a_value_offers_fields_and_methods() {
    let program = "class User {\n    name: string\n    age: int\n\n    greet(loud: bool) -> string {\n        return this.name\n    }\n}\nfunc main() {\n    let user = new User(name: \"Ada\", age: 36)\n    print(user.name)\n}\n";
    let typed = checked(program);
    // The cursor sits right after the `.` of the existing `user.name`.
    let dot = program.find("user.name").expect("the use") + "user.".len();
    let items = at(program, dot, Some(&typed));
    assert_eq!(labels(&items), ["name", "age", "greet"]);
    assert_eq!(detail_of(&items, "name"), Some("string"));
    assert_eq!(detail_of(&items, "greet"), Some("(bool) -> string"));
    // The receiver is not offered as one of its own members.
    assert!(!labels(&items).contains(&"user"));
}

#[test]
fn a_dot_after_a_builtin_offers_what_the_checker_accepts() {
    let program = "func main() {\n    var numbers: []int = []\n    print(numbers.len())\n    let text = \"hi\"\n    print(text.len())\n}\n";
    let typed = checked(program);
    let array = program.find("numbers.len").expect("array use") + "numbers.".len();
    let items = at(program, array, Some(&typed));
    assert_eq!(
        labels(&items),
        ["len", "push", "insert", "pop", "remove", "sort"]
    );
    assert_eq!(detail_of(&items, "push"), Some("(int) -> void"));
    assert_eq!(detail_of(&items, "pop"), Some("() -> Option<int>"));

    let string = program.find("text.len").expect("string use") + "text.".len();
    assert_eq!(labels(&at(program, string, Some(&typed))), ["len", "bytes"]);
}

#[test]
fn a_bare_cursor_offers_keywords_the_prelude_and_what_is_declared() {
    let program = "struct Point {\n    x: int\n}\nenum Shape {\n    Dot\n}\nfunc helper(n: int) -> int {\n    return n\n}\nfunc main() {\n    let total = 1\n}\n";
    let typed = checked(program);
    let items = at_end(program, &typed);
    let labels = labels(&items);
    for expected in [
        "func",
        "let",
        "var",
        "match", // keywords
        "print",
        "Some",
        "Ok",
        "bytes_to_string", // the prelude
        "helper",
        "main",
        "total", // what this file declares
        "Point",
        "Shape", // types, which are not value names
    ] {
        assert!(
            labels.contains(&expected),
            "`{expected}` missing: {labels:?}"
        );
    }
    assert_eq!(detail_of(&items, "helper"), Some("(int) -> int"));
    assert_eq!(detail_of(&items, "total"), Some("int"));
    // One entry per name and kind, however many tables it appears in.
    let mut sorted = labels.clone();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(sorted.len(), before, "duplicated labels in {labels:?}");
}

#[test]
fn completion_without_a_check_still_offers_what_needs_no_types() {
    // The first keystroke in a new file, before anything has ever checked.
    let items = at("fun", 3, None);
    assert!(labels(&items).contains(&"func"));
    // And a member list is empty rather than wrong, since nothing is known.
    assert!(at("value.", 6, None).is_empty());
}

#[test]
fn an_unknown_receiver_offers_nothing_rather_than_everything() {
    let program = "func main() { }\n";
    let typed = checked(program);
    let typing = format!("{program}// mystery.");
    assert!(at(&typing, typing.len(), Some(&typed)).is_empty());
}

#[test]
fn names_an_interface_and_a_function_type_instead_of_a_placeholder() {
    // Both live in the checker's own tables, so `Display` alone renders them
    // `<interface>` and `<function>` — which is what a hover used to show.
    let source = "interface Printable {\n    describe() -> string\n}\n\nclass User: Printable {\n    name: string\n    describe() -> string {\n        return this.name\n    }\n}\n\nfunc run(shown: Printable, pick: (int, int) -> int) {\n    print(shown.describe())\n    print(pick(1, 2))\n}\n\nfunc main() {\n    run(new User(name: \"a\"), (a: int, b: int) -> int { return a - b })\n}\n";
    let typed = skuld_compiler::check(source).expect("the fixture checks");
    let resolution = typed.resolution();
    let named = |name: &str| {
        let symbol = resolution
            .symbols
            .iter()
            .position(|info| info.name == name)
            .map(skuld_compiler::resolver::SymbolId)
            .expect("a symbol with this name");
        type_name(&typed, typed.symbol_type(symbol))
    };
    assert_eq!(named("shown"), "Printable");
    assert_eq!(named("pick"), "(int, int) -> int");
}
