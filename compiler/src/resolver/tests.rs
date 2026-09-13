use super::*;
use crate::parse;
fn output(source: &str) -> ResolveOutput {
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    resolve(&parsed.program.expect("AST"))
}
fn valid(source: &str) -> Resolution {
    let result = output(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    result.resolution.expect("resolution")
}
#[test]
fn examples_resolve_builtin_and_functions() {
    for source in [
        include_str!("../../../examples/hello.skuld"),
        include_str!("../../../examples/functions.skuld"),
    ] {
        let resolution = valid(source);
        assert!(
            resolution
                .references
                .values()
                .any(|id| resolution.symbols[id.0].kind == SymbolKind::Builtin(Builtin::Print))
        );
    }
}
#[test]
fn forward_calls_and_mutual_recursion() {
    let source = "func first() { second() } func second() { first() }";
    let result = valid(source);
    assert_eq!(result.references.len(), 2);
    for id in result.references.values() {
        assert_eq!(result.symbols[id.0].kind, SymbolKind::Function);
    }
}
#[test]
fn shadowing_and_initializer_use_outer_binding() {
    let source = "func main() {\nlet age = 27\n{ let age = age + 1\nprint(age) }\nprint(age)\n}";
    let result = valid(source);
    let ids: Vec<_> = result
        .declarations
        .values()
        .copied()
        .filter(|id| result.symbols[id.0].name == "age")
        .collect();
    assert_eq!(ids.len(), 2);
    let uses: Vec<_> = result
        .references
        .values()
        .copied()
        .filter(|id| result.symbols[id.0].name == "age")
        .collect();
    assert_eq!(uses, vec![ids[0], ids[1], ids[0]]);
    assert_ne!(
        result.symbols[ids[0].0].scope,
        result.symbols[ids[1].0].scope
    );
}
#[test]
fn duplicate_functions_parameters_and_locals() {
    for source in [
        "func f() {} func f() {}",
        "func f(x: int, x: int) {}",
        "func f(x: int) { let x = 1 }",
        "func f() { let x = 1\nvar x = 2 }",
    ] {
        let result = output(source);
        assert!(result.resolution.is_none());
        assert_eq!(result.diagnostics.len(), 1, "{source}");
        assert_eq!(
            result.diagnostics[0].code,
            DiagnosticCode::DuplicateDeclaration
        );
    }
    valid("func f(x: int) { { let x = 1 } } func g(x: int) {}");
}
#[test]
fn locals_are_not_hoisted_or_visible_in_other_functions() {
    for source in [
        "func f() { print(x)\nlet x = 1 }",
        "func f() { let x = x }",
        "func f() { let x = 1 } func g() { print(x) }",
        "func f() { { let x = 1 }\nprint(x) }",
    ] {
        let result = output(source);
        assert_eq!(result.diagnostics.len(), 1, "{source}");
        assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnknownName);
    }
}
#[test]
fn branch_scopes_do_not_leak() {
    valid(
        "func f(flag: bool) { if flag { let x = 1 } else if flag { let x = 2 } else { let x = 3 } }",
    );
    let result = output("func f() { if true { let x = 1 } else { print(x) }\nprint(x) }");
    assert_eq!(result.diagnostics.len(), 2);
}
#[test]
fn all_expression_positions_resolve_and_member_labels_are_deferred() {
    valid(
        "func f(user: Unknown, x: int) {\nuser.field = -x\nx += +x\nif !(x == x) { return user.greet((x + x)) }\n}",
    );
    let result = output("func f() {\na = b\nc(d)\nif e { return !f_missing }\nusr.greet()\n}");
    let names: Vec<_> = result.diagnostics.iter().map(|d| &d.message).collect();
    assert_eq!(names.len(), 7, "{names:?}");
    assert!(names.last().expect("unknown usr").contains("`usr`"));
    assert!(!names.iter().any(|name| name.contains("`greet`")));
}
#[test]
fn retains_mutability_but_does_not_type_check() {
    let result = valid(
        "func f(p: Unknown) -> Missing {\nlet x: int = \"wrong\"\nvar y = 1\nx = 2\np = 3\ny()\nreturn true\n}",
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Variable(Mutability::Immutable))
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Variable(Mutability::Mutable))
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Parameter)
    );
}
#[test]
fn prelude_can_be_shadowed_by_user_declarations() {
    let result = valid("func print() {} func f() { print() }");
    let id = result.references.values().next().expect("call");
    assert_eq!(result.symbols[id.0].kind, SymbolKind::Function);
    let result = valid("func f() { let print = 1\nprint }");
    let id = result.references.values().next().expect("use");
    assert_eq!(
        result.symbols[id.0].kind,
        SymbolKind::Variable(Mutability::Immutable)
    );
}
#[test]
fn diagnostics_point_to_name_and_do_not_hide_following_errors() {
    let source = "func main() {\n    usr.greet()\n    other()\n}";
    let result = output(source);
    assert_eq!(result.diagnostics.len(), 2);
    let d = &result.diagnostics[0];
    assert_eq!(&source[d.span.start..d.span.end], "usr");
    let rendered = d.render(&crate::span::SourceFile::new("main.skuld", source));
    assert!(rendered.contains("error[E0201]: unknown identifier `usr`"));
    assert!(rendered.contains("main.skuld:2:5"));
    assert!(rendered.contains("    ^^^"));
}
#[test]
fn results_are_deterministic_and_do_not_require_main() {
    let source = "func f(x: int) { let y = x\nprint(y) }";
    assert_eq!(valid(source), valid(source));
    valid("");
}

#[test]
fn while_body_is_a_child_scope() {
    // A binding declared in the body may shadow an outer one without leaking,
    // exactly like a plain block.
    let resolution = valid(
        "func main() {\n    let x = 1\n    while x < 0 {\n        let x = 2\n        print(x)\n    }\n    print(x)\n}",
    );
    let declarations = resolution.declarations.len();
    assert_eq!(declarations, 3, "main, outer x and shadowing x");
}

#[test]
fn methods_bind_this_and_do_not_leak_as_bare_names() {
    valid(
        "struct R {\n    n: int\n    get() -> int {\n        return this.n\n    }\n}\nfunc main() {}",
    );
    // A sibling method needs a receiver: method names are not in scope as
    // ordinary identifiers.
    let result = output(
        "struct R {\n    a() -> int {\n        return 1\n    }\n    b() -> int {\n        return a()\n    }\n}\nfunc main() {}",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownName)
    );
    // Methods are equally invisible from a plain function.
    let result = output(
        "struct R {\n    a() -> int {\n        return 1\n    }\n}\nfunc main() { print(a()) }",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownName)
    );
}

#[test]
fn match_arm_bindings_resolve_in_arm_scope() {
    let result = output(
        "enum E { V(int) }\nfunc main() {\n    let e = E.V(10)\n    match e {\n        E.V(val): { print(val) }\n    }\n}",
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn for_loop_variable_scoped_to_body() {
    let result =
        output("func main() {\n    for i in 0..5 {\n        print(i)\n    }\n    print(i)\n}");
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnknownName);
}
