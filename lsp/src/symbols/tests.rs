use super::*;

fn outline_of(source: &str) -> Vec<Symbol> {
    let program = skuld_compiler::parse(source)
        .program
        .expect("the fixture parses");
    outline(&program)
}

fn names(symbols: &[Symbol]) -> Vec<&str> {
    symbols.iter().map(|symbol| symbol.name.as_str()).collect()
}

#[test]
fn reports_a_function_with_its_signature() {
    let outline = outline_of("func add(a: int, b: int) -> int { return a + b }");
    assert_eq!(names(&outline), ["add"]);
    assert_eq!(outline[0].kind, kind::FUNCTION);
    assert_eq!(
        outline[0].detail.as_deref(),
        Some("(a: int, b: int) -> int")
    );
    // The selection must sit inside the range, or a client refuses the entry.
    assert!(outline[0].selection.start >= outline[0].range.start);
    assert!(outline[0].selection.end <= outline[0].range.end);
    assert_eq!(outline[0].selection, skuld_compiler::span::Span::new(5, 8));
}

#[test]
fn a_void_function_has_no_arrow() {
    let outline = outline_of("func main() { }");
    assert_eq!(outline[0].detail.as_deref(), Some("()"));
}

#[test]
fn separates_a_struct_from_a_class() {
    let outline = outline_of(
        "struct Point {\n  x: int\n  y: int\n}\nclass User {\n  name: string\n  hello() { }\n}\n",
    );
    assert_eq!(names(&outline), ["Point", "User"]);
    assert_eq!(outline[0].kind, kind::STRUCT);
    assert_eq!(outline[1].kind, kind::CLASS);
    assert_eq!(names(&outline[0].children), ["x", "y"]);
    assert_eq!(outline[0].children[0].detail.as_deref(), Some("int"));
    assert_eq!(names(&outline[1].children), ["name", "hello"]);
    assert_eq!(outline[1].children[1].kind, kind::METHOD);
}

#[test]
fn a_class_shows_what_it_conforms_to() {
    let outline = outline_of(
        "interface Printable { describe() -> string }\nclass User: Printable { name: string\n  describe() -> string { return this.name } }\n",
    );
    assert_eq!(outline[0].kind, kind::INTERFACE);
    assert_eq!(names(&outline[0].children), ["describe"]);
    assert_eq!(
        outline[0].children[0].detail.as_deref(),
        Some("() -> string")
    );
    assert_eq!(outline[1].detail.as_deref(), Some(": Printable"));
}

#[test]
fn reports_enum_variants_with_their_payloads() {
    let outline = outline_of("enum Shape { Dot, Circle(float) }");
    assert_eq!(outline[0].kind, kind::ENUM);
    assert_eq!(names(&outline[0].children), ["Dot", "Circle"]);
    assert_eq!(outline[0].children[0].detail, None);
    assert_eq!(outline[0].children[1].detail.as_deref(), Some("float"));
    assert_eq!(outline[0].children[1].kind, kind::ENUM_MEMBER);
}

#[test]
fn an_extern_block_nests_its_functions() {
    let outline = outline_of(
        "unsafe extern \"C\" {\n  func write(fd: i32, buffer: *u8, count: i64) -> i64\n}\n",
    );
    assert_eq!(names(&outline), ["extern \"C\""]);
    assert_eq!(outline[0].kind, kind::MODULE);
    assert_eq!(names(&outline[0].children), ["write"]);
    assert_eq!(
        outline[0].children[0].detail.as_deref(),
        Some("(fd: i32, buffer: *u8, count: i64) -> i64")
    );
}

#[test]
fn keeps_declarations_in_source_order() {
    // The AST holds each kind in its own list, so without the sort this
    // reports the struct first and the outline disagrees with the file.
    let outline = outline_of("func first() { }\nstruct Middle { x: int }\nfunc last() { }\n");
    assert_eq!(names(&outline), ["first", "Middle", "last"]);
}

#[test]
fn writes_the_compound_types_the_way_the_formatter_does() {
    let outline = outline_of(
        "func f(a: []u8, b: Option<int>, c: Result<int, string>, d: weak User, e: (int) -> bool) { }",
    );
    assert_eq!(
        outline[0].detail.as_deref(),
        Some("(a: []u8, b: Option<int>, c: Result<int, string>, d: weak User, e: (int) -> bool)")
    );
}
