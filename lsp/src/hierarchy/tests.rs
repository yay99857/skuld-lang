use super::*;

const ENTRY: FileId = FileId(0);

fn checked(source: &str) -> TypedProgram {
    skuld_compiler::check(source).expect("the fixture checks")
}

/// The symbol and item of the function declared with this name.
fn function(typed: &TypedProgram, source: &str, name: &str) -> (SymbolId, Item) {
    let offset = source
        .find(&format!("func {name}("))
        .map(|start| start + "func ".len())
        .or_else(|| source.find(&format!("{name}(")))
        .expect("the fixture declares it");
    declared_at(typed, ENTRY, offset).expect("a declaration at that offset")
}

#[test]
fn finds_the_function_a_use_sits_inside() {
    let source = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n}\n";
    let typed = checked(source);
    let call = source.rfind("add(1").expect("the call");
    let caller = enclosing(&typed, ENTRY, call).expect("a caller");
    assert_eq!(caller.name, "main");
    assert_eq!(caller.kind, FUNCTION);
}

#[test]
fn a_method_is_told_apart_from_a_function_and_names_its_class() {
    let source = "class Greeter {\n    name: string\n    greet() {\n        print(this.name)\n    }\n}\n\nfunc main() {\n    let g = new Greeter(name: \"a\")\n    g.greet()\n}\n";
    let typed = checked(source);
    let inside = source.find("print(this.name)").expect("the body");
    let owner = enclosing(&typed, ENTRY, inside).expect("a method");
    assert_eq!(owner.name, "greet");
    assert_eq!(owner.kind, METHOD);
    assert_eq!(owner.detail.as_deref(), Some("Greeter"));
}

#[test]
fn reports_every_use_but_not_the_declaration() {
    let source = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n    print(add(3, 4))\n}\n";
    let typed = checked(source);
    let (symbol, item) = function(&typed, source, "add");
    let uses = references_to(&typed, symbol);
    assert_eq!(uses.len(), 2);
    assert!(
        uses.iter()
            .all(|(_, span)| span.start != item.selection.start),
        "the declaration is not a use"
    );
}

#[test]
fn collects_what_a_body_calls() {
    let source = "func one() -> int {\n    return 1\n}\n\nfunc two() -> int {\n    return one() + one()\n}\n\nfunc main() {\n    print(two())\n}\n";
    let typed = checked(source);
    let (_, two) = function(&typed, source, "two");
    let called = calls_within(&typed, ENTRY, two.range);
    assert_eq!(called.len(), 2, "both calls, each at its own place");
    let names: Vec<String> = called
        .iter()
        .map(|(symbol, _)| item_of(&typed, *symbol).expect("an item").name)
        .collect();
    assert_eq!(names, ["one", "one"]);
}

#[test]
fn the_prelude_is_not_part_of_the_graph() {
    // `print` has no declaration to hand back as the other end of a call.
    let source = "func main() {\n    print(1)\n}\n";
    let typed = checked(source);
    let (_, main) = function(&typed, source, "main");
    assert!(calls_within(&typed, ENTRY, main.range).is_empty());
}

#[test]
fn a_function_named_as_a_value_is_a_call_too() {
    // Skuld turns a declared function into a function value where one is
    // expected, so the body does reach it.
    let source = "func by_size(a: int, b: int) -> int {\n    return a - b\n}\n\nfunc main() {\n    var xs = [2, 1]\n    xs.sort(by_size)\n    print(xs[0])\n}\n";
    let typed = checked(source);
    let (_, main) = function(&typed, source, "main");
    let called = calls_within(&typed, ENTRY, main.range);
    assert_eq!(called.len(), 1);
    assert_eq!(
        item_of(&typed, called[0].0).expect("an item").name,
        "by_size"
    );
}
