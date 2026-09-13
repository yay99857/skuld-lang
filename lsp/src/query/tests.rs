use super::*;
use skuld_compiler::{check_program, module::NoModules};

fn checked(source: &str) -> TypedProgram {
    check_program("main.skuld", source, &mut NoModules).unwrap_or_else(|errors| {
        panic!("the fixture must check:\n{}", errors.render());
    })
}

/// Hover over the first occurrence of `needle`, at its second byte, so the
/// test never depends on the cursor resting exactly at a word's start.
fn hover_over(source: &str, needle: &str, typed: &TypedProgram) -> Option<String> {
    let offset = source.find(needle).expect("the occurrence") + 1;
    hover(source, offset, typed).map(|(text, _)| text)
}

const PROGRAM: &str = "class User {\n    name: string\n\n    greet(loud: bool) -> string {\n        return this.name\n    }\n}\n\nenum Shape {\n    Dot\n    Box(int)\n}\n\nfunc area(width: int, height: int) -> int {\n    return width * height\n}\n\nfunc main() {\n    var total = 0\n    let user = new User(name: \"Ada\")\n    print(user.greet(false))\n    print(area(2, 3))\n    var bytes: []u8 = []\n    print(bytes.len())\n}\n";

#[test]
fn hover_reports_a_declaration_the_way_it_is_written() {
    let typed = checked(PROGRAM);
    assert_eq!(
        hover_over(PROGRAM, "area(2, 3)", &typed).as_deref(),
        Some("func area(int, int) -> int")
    );
    assert_eq!(
        hover_over(PROGRAM, "total = 0", &typed).as_deref(),
        Some("var total: int")
    );
    assert_eq!(
        hover_over(PROGRAM, "user = new", &typed).as_deref(),
        Some("let user: User")
    );
    assert_eq!(
        hover_over(PROGRAM, "width * height", &typed).as_deref(),
        Some("width: int")
    );
}

#[test]
fn hover_answers_at_a_declaration_as_well_as_at_a_use() {
    let typed = checked(PROGRAM);
    // The name in `func area(...)` itself, not a call to it.
    let offset = PROGRAM.find("area(width").expect("the declaration") + 1;
    assert_eq!(
        hover(PROGRAM, offset, &typed).map(|(text, _)| text).as_deref(),
        Some("func area(int, int) -> int")
    );
}

#[test]
fn hover_reports_members_and_types() {
    let typed = checked(PROGRAM);
    assert_eq!(
        hover_over(PROGRAM, "greet(false)", &typed).as_deref(),
        Some("greet(bool) -> string")
    );
    assert_eq!(
        hover_over(PROGRAM, "len())", &typed).as_deref(),
        Some("len() -> int")
    );
    // A type name resolves to no symbol, and is still worth answering.
    assert_eq!(
        hover_over(PROGRAM, "User {", &typed).as_deref(),
        Some("class User")
    );
    assert_eq!(
        hover_over(PROGRAM, "Shape {", &typed).as_deref(),
        Some("enum Shape")
    );
}

#[test]
fn hover_describes_the_prelude_which_has_no_declaration_to_point_at() {
    let typed = checked(PROGRAM);
    assert_eq!(
        hover_over(PROGRAM, "print(area", &typed).as_deref(),
        Some("func print(value) -> void")
    );
}

#[test]
fn hover_is_silent_where_there_is_nothing_to_say() {
    let typed = checked(PROGRAM);
    // Whitespace, punctuation and a keyword are not names.
    let space = PROGRAM.find("    var total").expect("indent");
    assert!(hover(PROGRAM, space, &typed).is_none());
    assert!(hover_over(PROGRAM, "return width", &typed).is_none());
}

#[test]
fn a_word_is_found_from_either_end_and_through_multibyte_text() {
    let source = "let olá = 1\n";
    let start = source.find("olá").expect("the name");
    // At its first byte, inside it, and just past its last.
    for offset in [start, start + 1, start + "olá".len()] {
        assert_eq!(
            word_at(source, offset).map(|word| word.text),
            Some("olá".to_owned()),
            "offset {offset}"
        );
    }
    // A cursor between two non-word characters names nothing. At offset 3 it
    // still names `let`, since resting just past a word asks about it.
    let equals = source.find('=').expect("the operator");
    assert!(word_at(source, equals).is_none(), "between the spaces");
    assert_eq!(word_at(source, 3).map(|w| w.text), Some("let".to_owned()));
}

/// Go to the definition of the first occurrence of `needle`.
fn definition_of(source: &str, needle: &str, typed: &TypedProgram) -> Option<(FileId, Span)> {
    let offset = source.find(needle).expect("the occurrence") + 1;
    definition(source, offset, typed)
}

#[test]
fn definition_points_at_the_name_that_was_declared() {
    let typed = checked(PROGRAM);
    let (file, span) = definition_of(PROGRAM, "area(2, 3)", &typed).expect("a function");
    assert_eq!(file, FileId(0));
    assert_eq!(&PROGRAM[span.start..span.end], "area");
    // The declaration, not the call.
    assert_eq!(span.start, PROGRAM.find("area(width").expect("declaration"));

    let (_, span) = definition_of(PROGRAM, "user.greet", &typed).expect("a binding");
    assert_eq!(span.start, PROGRAM.find("user = new").expect("the `let`"));

    let (_, span) = definition_of(PROGRAM, "width * height", &typed).expect("a parameter");
    assert_eq!(span.start, PROGRAM.find("width: int").expect("the parameter"));
}

#[test]
fn definition_reaches_a_type_a_field_and_a_method() {
    let typed = checked(PROGRAM);
    let (_, span) = definition_of(PROGRAM, "User(name:", &typed).expect("the class");
    assert_eq!(span.start, PROGRAM.find("class User").expect("declaration"));

    let (_, span) = definition_of(PROGRAM, "greet(false)", &typed).expect("the method");
    assert_eq!(span.start, PROGRAM.find("greet(loud").expect("declaration"));

    let program = "struct Point {\n    x: int\n}\nfunc main() {\n    let p = Point { x: 1 }\n    print(p.x)\n}\n";
    let typed = checked(program);
    let offset = program.find("p.x").expect("the use") + 2;
    let (_, span) = definition(program, offset, &typed).expect("the field");
    assert_eq!(span.start, program.find("x: int").expect("the field"));
}

#[test]
fn a_builtin_has_nowhere_to_go() {
    let typed = checked(PROGRAM);
    // `print` and a builtin method belong to the language, not to a file.
    assert!(definition_of(PROGRAM, "print(area", &typed).is_none());
    assert!(definition_of(PROGRAM, "len())", &typed).is_none());
}
