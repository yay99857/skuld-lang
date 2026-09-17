use super::format_source;

fn fmt(source: &str) -> String {
    format_source(source).expect("source should parse")
}

#[test]
fn format_basic_indentation() {
    let input = "func main(){\nprint(\"hello\")\n}\n";
    let expected = "func main() {\n    print(\"hello\")\n}\n";
    assert_eq!(fmt(input), expected);
}

#[test]
fn normalize_return_type() {
    // `: int` normalizes to `-> int` for top-level functions.
    let input = "func add(a: int, b: int): int {\n    return a + b\n}\n";
    let expected = "func add(a: int, b: int) -> int {\n    return a + b\n}\n";
    assert_eq!(fmt(input), expected);
}

#[test]
fn top_level_blank_lines() {
    let input = "func a() {}\nfunc b() {}\n";
    let output = fmt(input);
    assert!(
        output.contains("}\n\nfunc b"),
        "blank line between decls: {output}"
    );
}

#[test]
fn empty_block() {
    let input = "func noop() {}\n";
    assert_eq!(fmt(input), "func noop() {}\n");
}

#[test]
fn format_comments() {
    let input = "// A greeting.\nfunc main() {\n    print(\"hi\") // inline\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("// A greeting."),
        "leading comment: {output}"
    );
    assert!(output.contains("// inline"), "trailing comment: {output}");
}

#[test]
fn literals_preserved() {
    let input =
        "func main() {\n    let x = 0042\n    let y = 3.14\n    let s = \"hello\\tworld\"\n}\n";
    let output = fmt(input);
    assert!(output.contains("0042"), "integer preserved: {output}");
    assert!(output.contains("3.14"), "float preserved: {output}");
    assert!(
        output.contains("\"hello\\tworld\""),
        "string preserved: {output}"
    );
}

#[test]
fn normalize_match_arms() {
    let input = "func main() {\n    match x {\n        _: print(\"ok\")\n    }\n}\n";
    let output = fmt(input);
    assert!(output.contains("_: print(\"ok\")"), "match arm: {output}");
}

#[test]
fn methods_have_no_func_prefix() {
    let input =
        "class Counter {\n    value: int\n\n    increment() {\n        this.value += 1\n    }\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("    increment()"),
        "method has no func: {output}"
    );
    assert!(
        !output.contains("func increment"),
        "no func prefix: {output}"
    );
}

#[test]
fn new_uses_parentheses() {
    let input = "func main() {\n    let c = new Counter(value: 10)\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("new Counter(value: 10)"),
        "new with parens: {output}"
    );
}

#[test]
fn try_is_postfix() {
    let input =
        "func parse() -> Result<int, string> {\n    let x = lookup()?\n    return Ok(x)\n}\n";
    let output = fmt(input);
    assert!(output.contains("lookup()?"), "postfix ?: {output}");
}

#[test]
fn weak_uses_parentheses() {
    let input = "func main() {\n    let w = weak(obj)\n    let e = weak()\n}\n";
    let output = fmt(input);
    assert!(output.contains("weak(obj)"), "weak with value: {output}");
    assert!(output.contains("weak()"), "weak empty: {output}");
}

#[test]
fn lambda_syntax() {
    let input =
        "func main() {\n    let f = (a: int, b: int): int {\n        return a + b\n    }\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("(a: int, b: int) -> int {"),
        "lambda form: {output}"
    );
    assert!(
        !output.contains("func("),
        "no func prefix on lambda: {output}"
    );

    let expr_lambda = "func main() {\n    let f = (a, b) => a - b\n}\n";
    let output2 = fmt(expr_lambda);
    assert!(
        output2.contains("(a, b) => a - b"),
        "expression lambda form: {output2}"
    );
}

#[test]
fn unsafe_extern() {
    let input =
        "unsafe extern \"C\" {\n    func write(fd: i32, buffer: *u8, count: u64) -> i64\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("unsafe extern \"C\""),
        "unsafe prefix: {output}"
    );
}

#[test]
fn if_let_bare_binding() {
    let input = "func main() {\n    if let answer = find() {\n        print(answer)\n    }\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("if let answer = find()"),
        "bare if let: {output}"
    );
    assert!(!output.contains("Some("), "no Some wrapper: {output}");
}

#[test]
fn if_let_ok_pattern() {
    let input = "func main() {\n    if let Ok(value) = result {\n        print(value)\n    }\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("if let Ok(value) = result"),
        "Ok pattern: {output}"
    );
}

#[test]
fn if_else_chain() {
    let input = "func main() {\n    if x {\n        print(\"a\")\n    } else {\n        print(\"b\")\n    }\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("} else {"),
        "else on same line as closing brace: {output}"
    );
}

#[test]
fn idempotent() {
    let input = "// Header comment.\n\nfunc add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    let x = add(1, 2)\n    print(x)\n}\n";
    let first = fmt(input);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting must be idempotent");
}

#[test]
fn struct_literal_uses_braces() {
    let input = "struct Point {\n    x: int\n    y: int\n}\n\nfunc main() {\n    let p = Point { x: 1, y: 2 }\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("Point { x: 1, y: 2 }"),
        "struct literal: {output}"
    );
}

#[test]
fn enum_trailing_commas() {
    let input = "enum Color {\n    Red\n    Green\n    Blue\n}\n";
    let output = fmt(input);
    assert!(output.contains("Red,"), "trailing comma: {output}");
    assert!(output.contains("Green,"), "trailing comma: {output}");
    assert!(output.contains("Blue,"), "trailing comma: {output}");
}

#[test]
fn function_type_no_func_prefix() {
    let input = "func apply(f: (int) -> int, x: int) -> int {\n    return f(x)\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("f: (int) -> int"),
        "function type: {output}"
    );
}

#[test]
fn interface_methods_no_func() {
    let input = "interface Renderer {\n    render(value: int) -> string\n}\n";
    let output = fmt(input);
    assert!(
        output.contains("    render(value: int)"),
        "method no func: {output}"
    );
    assert!(!output.contains("func render"), "no func prefix: {output}");
}

#[test]
fn a_comment_stays_with_the_declaration_it_documents() {
    // The blank line that separates two declarations belongs before the
    // second one's comment, not between the comment and what it describes.
    let input = "func a() {}\n\n// What b does.\nfunc b() {}\n";
    assert_eq!(fmt(input), "func a() {}\n\n// What b does.\nfunc b() {}\n");
}

#[test]
fn a_comment_the_source_kept_apart_stays_apart() {
    // A comment with a blank line on both sides documents neither neighbour,
    // and formatting must not attach it to the one that follows.
    let input = "func a() {}\n\n// A note about nothing in particular.\n\nfunc b() {}\n";
    assert_eq!(
        fmt(input),
        "func a() {}\n\n// A note about nothing in particular.\n\nfunc b() {}\n"
    );
}

#[test]
fn a_comment_after_a_run_of_code_does_not_inherit_an_earlier_gap() {
    // The distance is measured from the last thing emitted. Measuring it from
    // the previous comment instead put a blank line in front of every comment
    // that followed a few lines of code.
    let input = "enum E {\n    // The first.\n    A\n    B\n    C\n    // The last.\n    D\n}\n";
    assert_eq!(
        fmt(input),
        "enum E {\n    // The first.\n    A,\n    B,\n    C,\n    // The last.\n    D,\n}\n"
    );
}

#[test]
fn a_blank_line_between_statements_is_how_a_reader_groups_them() {
    let input = "func main() {\n    let a = 1\n\n    let b = 2\n    let c = 3\n}\n";
    assert_eq!(fmt(input), input);
}

#[test]
fn an_empty_struct_literal_has_nothing_to_space_out() {
    // Every field defaulted: `Point {}` is the whole expression.
    let input = "struct Point {\n    x: int = 0\n}\n\nfunc main() {\n    let p = Point {}\n}\n";
    assert_eq!(fmt(input), input);
}

#[test]
fn an_unsafe_block_keeps_its_keyword_on_the_brace() {
    let input = "func main() {\n    var cell: int = 1\n\n    unsafe {\n        store(ptr(cell), 2)\n    }\n}\n";
    assert_eq!(fmt(input), input);
    // A block written tight is opened up like any other.
    assert_eq!(
        fmt("func main() {\n    unsafe{let x = 1}\n}\n"),
        "func main() {\n    unsafe {\n        let x = 1\n    }\n}\n"
    );
}
