use crate::{check, diagnostic::DiagnosticCode, types::Type};
fn valid(source: &str) {
    assert!(check(source).is_ok(), "{:?}", check(source));
}
fn fails(source: &str, code: DiagnosticCode) {
    let errors = check(source).expect_err("must fail");
    assert!(
        errors.iter().any(|error| error.code == code),
        "{source}: {errors:?}"
    );
}
#[test]
fn checks_demos_and_infers_types() {
    valid(include_str!("../../../examples/hello.skuld"));
    valid(include_str!("../../../examples/functions.skuld"));
    valid(
        "func main() { var age = 17\nage += 10\nif age >= 18 { print(\"Adult\") } else { print(\"Minor\") } }",
    );
    let source = "func main() { let x = 1 + 2 }";
    let typed = check(source).expect("checked");
    assert_eq!(
        typed.expression_type(crate::span::Span::new(22, 27)),
        Some(Type::Int)
    );
}
#[test]
fn no_implicit_coercions_or_truthiness() {
    for source in [
        "func main() { let x: int = 10.5 }",
        "func main() { let x: int = \"hello\" }",
        "func main() { print(1 + 2.0) }",
        "func main() { if 1 {} }",
        "func f(x: bool) {} func main() { f(1) }",
    ] {
        fails(source, DiagnosticCode::TypeMismatch);
    }
    valid("func main() { print(1.5 + 2.5)\nprint(true && !false)\nprint(\"a\" == \"b\") }");
}
#[test]
fn assignment_checks_mutability_and_type() {
    for source in [
        "func main() { let x = 1\nx = 2 }",
        "func main() { let x = 1\nx += 2 }",
        "func f(x: int) { x = 2 } func main() {}",
    ] {
        fails(source, DiagnosticCode::ImmutableAssignment);
    }
    fails(
        "func main() { var x = 1\nx = false }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { main = 1 }",
        DiagnosticCode::InvalidAssignment,
    );
    valid("func main() { var x = 1\nvar y = 2\nx = y = 3\nx *= y\nprint(x) }");
}
#[test]
fn call_signatures_and_builtin_shadowing() {
    valid("func main() { print() }");
    fails("func main() { print(1, 2) }", DiagnosticCode::ArgumentCount);
    fails(
        "func f(x: int) {} func main() { f() }",
        DiagnosticCode::ArgumentCount,
    );
    fails(
        "func main() { let print = 1\nprint(2) }",
        DiagnosticCode::NotCallable,
    );
    valid("func print(x: int, y: int) -> int { return x + y } func main() { print(1, 2) }");
    valid(
        "func main() { print(later(4)) } func later(x: int) -> int { if x == 0 { return 0 } else { return later(x - 1) } }",
    );
}
#[test]
fn return_completeness_and_signatures() {
    fails(
        "func f() -> int { return \"wrong\" } func main() {}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func f() -> int { return } func main() {}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func f(x: bool) -> int { if x { return 1 } } func main() {}",
        DiagnosticCode::MissingReturn,
    );
    fails(
        "func f() -> int {} func main() {}",
        DiagnosticCode::MissingReturn,
    );
    fails("func main() { return 1 }", DiagnosticCode::TypeMismatch);
    valid(
        "func f(x: bool) -> int { if x { return 1 } else { { return 2 } } } func main() { print(f(true)) }",
    );
    valid("func f() {} func main() { return f() }");
}
#[test]
fn void_and_unsupported_features() {
    fails(
        "func main() { let x = print(1) }",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "func f(x: void) {} func main() {}",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "func main() { print(print(1)) }",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "func main() { let x: User = 1 }",
        DiagnosticCode::UnknownType,
    );
    fails(
        "func main() { print('a') }",
        DiagnosticCode::UnsupportedFeature,
    );
    fails(
        "func main() { let x = 1\nx.greet() }",
        DiagnosticCode::UnsupportedFeature,
    );
    fails(
        "func main() { let f = main }",
        DiagnosticCode::UnsupportedFeature,
    );
    fails(
        "func main() { print(\"a\" + \"b\") }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { print(1.0 % 2.0) }",
        DiagnosticCode::InvalidOperator,
    );
}
#[test]
fn signed_literal_boundaries() {
    valid(
        "func main() { print(9223372036854775807)\nprint(-9223372036854775808)\nprint(-(9223372036854775808)) }",
    );
    fails(
        "func main() { print(9223372036854775808) }",
        DiagnosticCode::IntegerRange,
    );
    fails(
        "func main() { print(-9223372036854775809) }",
        DiagnosticCode::IntegerRange,
    );
}
#[test]
fn entrypoint_contract() {
    fails("", DiagnosticCode::InvalidEntrypoint);
    fails("func main(x: int) {}", DiagnosticCode::InvalidEntrypoint);
    fails(
        "func main() -> int { return 0 }",
        DiagnosticCode::InvalidEntrypoint,
    );
    valid("func main() -> void {}");
}
#[test]
fn errors_are_accumulated_and_earlier_stages_still_run() {
    fails("func main() { unknown() }", DiagnosticCode::UnknownName);
    fails("func main() { @ }", DiagnosticCode::InvalidCharacter);
    let errors =
        check("func main() { let x: int = \"bad\"\nlet y: bool = 1 }").expect_err("two errors");
    assert_eq!(errors.len(), 2);
}

#[test]
fn old_spellings_are_not_implicit_aliases() {
    fails("fn main() {}", DiagnosticCode::ExpectedDeclaration);
    fails("func main() { println() }", DiagnosticCode::UnknownName);
}

#[test]
fn while_requires_a_bool_condition() {
    valid("func main() { var i = 0\nwhile i < 3 { i += 1 } }");
    fails(
        "func main() { var i = 0\nwhile i { i += 1 } }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { while \"yes\" { } }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn while_never_satisfies_a_return_type() {
    // The condition can be false on entry, so the body is not guaranteed to run.
    fails(
        "func answer() -> int {\n    while true {\n        return 1\n    }\n}\nfunc main() { print(answer()) }",
        DiagnosticCode::MissingReturn,
    );
}
