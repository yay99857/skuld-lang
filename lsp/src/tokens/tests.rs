use super::*;

/// Each classified name as `(text, type, modifiers)`, which is what a reader
/// of these tests wants to see.
fn classified(source: &str) -> Vec<(String, &'static str, Vec<&'static str>)> {
    let typed = skuld_compiler::check(source).expect("the fixture checks");
    tokens(source, &typed)
        .into_iter()
        .map(|token| {
            let modifiers = MODIFIERS
                .iter()
                .enumerate()
                .filter(|(index, _)| token.modifiers & (1 << index) != 0)
                .map(|(_, name)| *name)
                .collect();
            (
                source[token.start..token.end].to_owned(),
                TYPES[token.kind as usize],
                modifiers,
            )
        })
        .collect()
}

#[test]
fn separates_a_declaration_from_a_use() {
    let found = classified("func main() {\n    print(1)\n}\n");
    assert_eq!(
        found,
        [
            ("main".to_owned(), "function", vec!["declaration"]),
            // A prelude binding is a function of the language itself.
            ("print".to_owned(), "function", vec!["defaultLibrary"]),
        ]
    );
}

#[test]
fn a_let_is_readonly_and_a_var_is_not() {
    let found = classified(
        "func main() {\n    let fixed = 1\n    var moving = 2\n    moving = fixed\n    print(moving)\n}\n",
    );
    assert_eq!(
        found[1],
        (
            "fixed".to_owned(),
            "variable",
            vec!["declaration", "readonly"]
        )
    );
    assert_eq!(
        found[2],
        ("moving".to_owned(), "variable", vec!["declaration"])
    );
    assert_eq!(found[3], ("moving".to_owned(), "variable", vec![]));
    assert_eq!(found[4], ("fixed".to_owned(), "variable", vec!["readonly"]));
}

#[test]
fn a_parameter_is_readonly_because_skuld_has_no_mutable_ones() {
    let found = classified(
        "func twice(value: int) -> int {\n    return value + value\n}\n\nfunc main() {\n    print(twice(2))\n}\n",
    );
    assert_eq!(
        found[1],
        (
            "value".to_owned(),
            "parameter",
            vec!["declaration", "readonly"]
        )
    );
    assert_eq!(found[2].1, "parameter");
    assert!(found[2].2.contains(&"readonly"));
}

#[test]
fn a_class_is_not_a_struct_and_both_are_not_variables() {
    let source = "struct Point {\n    x: int\n}\n\nclass User {\n    name: string\n}\n\nfunc main() {\n    let p = Point { x: 1 }\n    let u = new User(name: \"a\")\n    print(p.x)\n    print(u.name)\n}\n";
    let found = classified(source);
    let kinds: Vec<_> = found
        .iter()
        .filter(|(text, _, _)| text == "Point" || text == "User")
        .map(|(_, kind, _)| *kind)
        .collect();
    assert_eq!(kinds, ["struct", "class", "struct", "class"]);
    // A field read through a receiver is a property, not a variable.
    assert!(
        found
            .iter()
            .any(|(text, kind, _)| text == "x" && *kind == "property")
    );
    assert!(
        found
            .iter()
            .any(|(text, kind, _)| text == "name" && *kind == "property")
    );
}

#[test]
fn a_method_is_a_method_and_a_builtin_one_too() {
    let source = "class Greeter {\n    name: string\n    greet() {\n        print(this.name)\n    }\n}\n\nfunc main() {\n    let g = new Greeter(name: \"a\")\n    g.greet()\n    var xs = [1]\n    print(xs.len())\n}\n";
    let found = classified(source);
    assert!(
        found
            .iter()
            .any(|(text, kind, _)| text == "greet" && *kind == "method")
    );
    assert!(
        found
            .iter()
            .any(|(text, kind, _)| text == "len" && *kind == "method")
    );
}

#[test]
fn an_enum_name_and_an_interface_name_keep_their_own_kinds() {
    let source = "interface Printable {\n    describe() -> string\n}\n\nenum Status {\n    Active,\n    Idle\n}\n\nclass User: Printable {\n    name: string\n    describe() -> string {\n        return this.name\n    }\n}\n\nfunc main() {\n    let s = Status.Active\n    match s {\n        Status.Active: print(1)\n        _: print(0)\n    }\n}\n";
    let found = classified(source);
    assert!(
        found
            .iter()
            .any(|(text, kind, _)| text == "Printable" && *kind == "interface")
    );
    assert!(
        found
            .iter()
            .any(|(text, kind, _)| text == "Status" && *kind == "enum")
    );
}

#[test]
fn a_name_the_checker_cannot_place_is_left_alone() {
    // Field names in a record literal are labels, not names the resolver
    // records; nothing is emitted for them rather than a guess.
    let source = "struct Point {\n    x: int\n}\n\nfunc main() {\n    let p = Point { x: 1 }\n    print(p.x)\n}\n";
    let found = classified(source);
    let labels = found.iter().filter(|(text, _, _)| text == "x").count();
    assert_eq!(labels, 1, "only the one read through the receiver");
}

#[test]
fn the_tokens_come_out_in_source_order() {
    let found = classified("func main() {\n    let a = 1\n    let b = 2\n    print(a + b)\n}\n");
    let texts: Vec<&str> = found.iter().map(|(text, _, _)| text.as_str()).collect();
    assert_eq!(texts, ["main", "a", "b", "print", "a", "b"]);
}
