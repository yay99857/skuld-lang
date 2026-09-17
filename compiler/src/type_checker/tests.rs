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
        Some(Type::INT)
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
    valid("func main() { print('a') }");
    fails(
        "func main() { let x = 1\nx.greet() }",
        DiagnosticCode::UnsupportedFeature,
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

#[test]
fn jumps_require_an_enclosing_loop() {
    valid("func main() { loop { break } }");
    valid("func main() { var i = 0\nwhile i < 1 { i += 1\ncontinue } }");
    fails("func main() { break }", DiagnosticCode::JumpOutsideLoop);
    fails(
        "func main() { if true { continue } }",
        DiagnosticCode::JumpOutsideLoop,
    );
}

#[test]
fn a_loop_without_break_diverges() {
    // Nothing follows an unbroken loop, so no further return is required.
    valid(
        "func answer() -> int {\n    loop {\n        return 1\n    }\n}\nfunc main() { print(answer()) }",
    );
    // A break restores the fall-through path, so the return is required again.
    fails(
        "func answer() -> int {\n    loop {\n        break\n    }\n}\nfunc main() { print(answer()) }",
        DiagnosticCode::MissingReturn,
    );
    // The break belongs to the inner while, so the outer loop still diverges.
    valid(
        "func answer() -> int {\n    var i = 0\n    loop {\n        while i < 1 {\n            break\n        }\n        return 1\n    }\n}\nfunc main() { print(answer()) }",
    );
}

#[test]
fn structs_construct_with_every_field_exactly_once() {
    valid(
        "struct P { a: int\nb: string }\nfunc main() { let p = P { a: 1, b: \"x\" }\nprint(p.a) }",
    );
    fails(
        "struct P { a: int\nb: int }\nfunc main() { let p = P { a: 1 } }",
        DiagnosticCode::MissingField,
    );
    fails(
        "struct P { a: int }\nfunc main() { let p = P { a: 1, z: 2 } }",
        DiagnosticCode::UnknownName,
    );
    fails(
        "struct P { a: int }\nfunc main() { let p = P { a: 1, a: 2 } }",
        DiagnosticCode::DuplicateDeclaration,
    );
    fails(
        "struct P { a: int }\nfunc main() { let p = P { a: \"text\" } }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { let p = Missing { a: 1 } }",
        DiagnosticCode::UnknownType,
    );
}

#[test]
fn struct_declarations_reject_duplicates_and_self_containment() {
    fails(
        "struct P { a: int }\nstruct P { b: int }\nfunc main() {}",
        DiagnosticCode::DuplicateDeclaration,
    );
    fails(
        "struct P { a: int\na: int }\nfunc main() {}",
        DiagnosticCode::DuplicateDeclaration,
    );
    // A value type has no indirection, so containing itself has no size.
    fails(
        "struct P { inner: P }\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn field_access_requires_a_struct_and_an_existing_field() {
    valid(
        "struct I { v: int }\nstruct O { i: I }\nfunc main() { let o = O { i: I { v: 1 } }\nprint(o.i.v) }",
    );
    fails(
        "struct P { a: int }\nfunc main() { let p = P { a: 1 }\nprint(p.missing) }",
        DiagnosticCode::UnknownName,
    );
    fails(
        "func main() { let x = 1\nprint(x.field) }",
        DiagnosticCode::UnsupportedFeature,
    );
}

#[test]
fn field_assignment_follows_the_binding_mutability() {
    valid("struct P { n: int }\nfunc main() { var p = P { n: 1 }\np.n = 2\np.n += 3\nprint(p.n) }");
    // The field of an immutable binding is immutable too.
    fails(
        "struct P { n: int }\nfunc main() { let p = P { n: 1 }\np.n = 2 }",
        DiagnosticCode::ImmutableAssignment,
    );
    fails(
        "struct P { n: int }\nfunc f(p: P) { p.n = 2 }\nfunc main() {}",
        DiagnosticCode::ImmutableAssignment,
    );
}

#[test]
fn methods_take_an_implicit_immutable_receiver() {
    valid(
        "struct R {\n    w: int\n    h: int\n    area() -> int {\n        return this.w * this.h\n    }\n}\nfunc main() { let r = R { w: 3, h: 4 }\nprint(r.area()) }",
    );
    valid(
        "struct R {\n    w: int\n    scaled(by: int) -> int {\n        return this.w * by\n    }\n}\nfunc main() { let r = R { w: 2 }\nprint(r.scaled(3)) }",
    );
    // `this` is a parameter, and parameters are immutable.
    fails(
        "struct R {\n    n: int\n    bump() {\n        this.n = 1\n    }\n}\nfunc main() {}",
        DiagnosticCode::ImmutableAssignment,
    );
}

#[test]
fn method_calls_are_checked_like_calls() {
    fails(
        "struct R { n: int }\nfunc main() { let r = R { n: 1 }\nprint(r.missing()) }",
        DiagnosticCode::NotCallable,
    );
    fails(
        "struct R { n: int }\nfunc main() { let r = R { n: 1 }\nprint(r.n()) }",
        DiagnosticCode::NotCallable,
    );
    fails(
        "struct R {\n    n: int\n    add(a: int) -> int {\n        return this.n + a\n    }\n}\nfunc main() { let r = R { n: 1 }\nprint(r.add(1, 2)) }",
        DiagnosticCode::ArgumentCount,
    );
    // A method is not a value.
    fails(
        "struct R {\n    n: int\n    get() -> int {\n        return this.n\n    }\n}\nfunc main() { let r = R { n: 1 }\nprint(r.get) }",
        DiagnosticCode::UnknownName,
    );
    fails(
        "func main() { let x = 1\nprint(x.method()) }",
        DiagnosticCode::UnsupportedFeature,
    );
}

#[test]
fn method_names_cannot_collide() {
    fails(
        "struct R {\n    n: int\n    n() -> int {\n        return 1\n    }\n}\nfunc main() {}",
        DiagnosticCode::DuplicateDeclaration,
    );
}

#[test]
fn strings_concatenate_with_plus_only() {
    valid("func main() { print(\"a\" + \"b\") }");
    valid(
        "func join(a: string, b: string) -> string { return a + b }\nfunc main() { print(join(\"x\", \"y\")) }",
    );
    // Only `+` is defined on strings; the rest stay arithmetic.
    for source in [
        "func main() { print(\"a\" - \"b\") }",
        "func main() { print(\"a\" * \"b\") }",
        "func main() { print(\"a\" < \"b\") }",
    ] {
        fails(source, DiagnosticCode::InvalidOperator);
    }
    // No implicit conversion joins a string to a number.
    fails(
        "func main() { print(\"a\" + 1) }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn interpolation_accepts_what_print_accepts() {
    valid("func main() { let n = 1\nprint(\"a ${n} b ${n > 0} c ${1.5} d ${\"s\"}\") }");
    valid("func main() { print(\"${1 + 2}\") }");
    // A struct has no textual form.
    fails(
        "struct P { n: int }\nfunc main() { let p = P { n: 1 }\nprint(\"${p}\") }",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "func main() { print(\"${print(1)}\") }",
        DiagnosticCode::InvalidValueType,
    );
    // An interpolation is a string, so it type checks as one.
    fails(
        "func main() { let x: int = \"${1}\" }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn classes_construct_with_new_and_have_reference_semantics() {
    valid("class User { name: string }\nfunc main() { let u = new User(name: \"Ada\") }");
    fails(
        "class User { name: string }\nfunc main() { let u = User { name: \"Ada\" } }",
        DiagnosticCode::InvalidAssignment,
    );
    fails(
        "struct S { n: int }\nfunc main() { let s = new S(n: 1) }",
        DiagnosticCode::InvalidAssignment,
    );
    valid("class Box { val: int }\nfunc main() { let b = new Box(val: 1)\nb.val = 2 }");
    valid("class Box {\n    val: int\n    inc() {\n        this.val = 2\n    }\n}\nfunc main() {}");
    fails(
        "class Box {\n    val: int\n    reset() {\n        this = new Box(val: 0)\n    }\n}\nfunc main() {}",
        DiagnosticCode::ImmutableAssignment,
    );
    valid("class Node { next: Node }\nfunc main() {}");
    fails(
        "class Box { val: int }\nfunc main() { let b = new Box(val: 1)\nprint(b) }",
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn arrays_and_weak_references_use_local_expected_types() {
    valid(
        "class User {}\nfunc use(values: [][]int, ref: weak User) {}\nfunc empty() -> []int { return ([]) }\nfunc main() {\nlet nested: [][]int = [[], [1]]\nuse([[]], weak())\nvar ref: weak User = (weak())\nref = weak(new User())\nlet refs: []weak User = [weak(), weak(new User())]\n}",
    );
    fails(
        "func main() { let a: []int = [][0] }",
        DiagnosticCode::UnknownType,
    );
    fails(
        "func main() { let a: []int = [[]].len() }",
        DiagnosticCode::UnknownType,
    );
    fails(
        "class A {}\nfunc main() { let a: weak A = weak().get() }",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "func main() { let a = [[1], [true]] }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn invalid_method_arguments_are_checked_even_with_wrong_arity() {
    let source = "class A { f() {} }\nfunc main() { new A().f(1 + true) }";
    let errors = check(source).expect_err("invalid call");
    assert!(
        errors
            .iter()
            .any(|d| d.code == DiagnosticCode::ArgumentCount)
    );
    assert!(
        errors
            .iter()
            .any(|d| d.code == DiagnosticCode::TypeMismatch)
    );
}

#[test]
fn weak_types_do_not_resolve_as_value_names() {
    valid("class A {}\nfunc main() { let A = 1\nlet ref: weak A = weak(new A())\nprint(A) }");
    fails(
        "struct A {}\nfunc main() { let ref: weak A = weak() }",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "class A {}\nclass B {}\nfunc main() { let refs: []weak A = [weak(new B())] }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn options_infer_only_from_local_expected_types() {
    valid(
        "func f(x: Option<[]int>) -> Option<Option<int>> { return Some(None) }\nfunc main() { let x: Option<[]int> = Some([])\nf(Some([])) }",
    );
    fails(
        "func main() { let x = Some(None) }",
        DiagnosticCode::UnknownType,
    );
    fails(
        "func main() { let x: Option<int> = None.is_some() }",
        DiagnosticCode::UnknownType,
    );
    fails(
        "func main() { let x: Option<int> = Some(true) }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "struct A { next: Option<B> }\nstruct B { next: Option<A> }\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
    valid("class A { next: Option<A> }\nfunc main() { let a = new A(next: None) }");
}

#[test]
fn if_let_bindings_obey_scope_and_mutability_rules() {
    valid(
        "func main() { let x = Some(1)\nif let Some(x) = x { print(x) } else { print(x.is_some()) } }",
    );
    fails(
        "func main() { if let Some(x) = Some(1) { let x = 2 } }",
        DiagnosticCode::DuplicateDeclaration,
    );
    fails(
        "func main() { if let Some(x) = Some(1) { x = 2 } }",
        DiagnosticCode::ImmutableAssignment,
    );
    fails(
        "func main() { if let Some(x) = Some(1) {}\nprint(x) }",
        DiagnosticCode::UnknownName,
    );
    valid(
        "func f(x: Option<int>) -> int { if let Some(n) = x { return n } else { return 0 } }\nfunc main() {}",
    );
}

#[test]
fn option_constructor_identity_comes_from_resolution() {
    valid("func Some(x: int) -> int { return x }\nfunc main() { print(Some(1)) }");
    valid("func None() -> int { return 2 }\nfunc main() { print(None()) }");
    fails(
        "func main() { let Some = 1\nSome(2) }",
        DiagnosticCode::NotCallable,
    );
    fails(
        "func main() { let None = \"shadowed\"\nlet x: Option<int> = None }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn option_null_implicit_wrap_and_direct_if_let() {
    valid(
        "func find(ok: bool): Option<int> {\n    if ok { return 42 }\n    return null\n}\nfunc main() {\n    let opt: Option<int> = 10\n    let empty: Option<int> = null\n    if let val = find(true) { print(val) }\n}",
    );
    fails("func main() { let x = null }", DiagnosticCode::UnknownType);
    fails("func main() { null() }", DiagnosticCode::NotCallable);
}

#[test]
fn enum_and_match_type_checking() {
    valid(
        "enum Color { Red, Green, Blue }\nfunc main() {\n    let c = Color.Red\n    match c {\n        Color.Red: { print(1) }\n        Color.Green: { print(2) }\n        Color.Blue: { print(3) }\n    }\n}",
    );
    valid(
        "enum Outcome { Good(int), Bad(string) }\nfunc eval(r: Outcome) -> int {\n    match r {\n        Outcome.Good(val): return val\n        Outcome.Bad(msg): return 0\n    }\n}\nfunc main() {}",
    );
    // `Option` and `Result` name builtin types and cannot be declared.
    fails(
        "enum Result { Good, Bad }\nfunc main() {}",
        DiagnosticCode::DuplicateDeclaration,
    );
    fails(
        "enum E { A, B }\nfunc main() {\n    let e = E.A\n    match e {\n        E.A: {}\n    }\n}",
        DiagnosticCode::NonExhaustiveMatch,
    );
    fails(
        "enum List { Cons(List), Nil }\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn result_type_checking() {
    valid(
        "func read(n: int) -> Result<int, string> {\n    if n < 0 { return Err(\"negative\") }\n    return Ok(n)\n}\nfunc twice(n: int) -> Result<int, string> {\n    let value = read(n)?\n    return Ok(value * 2)\n}\nfunc main() {\n    match twice(2) {\n        Ok(v): print(v)\n        Err(e): print(e)\n    }\n    if let Ok(v) = twice(2) { print(v) }\n    if let Err(e) = twice(-1) { print(e) }\n    print(twice(2).is_ok())\n    print(twice(2).is_err())\n}",
    );
    // Interned by payload pair: the same `Result<T, E>` is the same type, and a
    // different pair is not.
    let typed = check(
        "func a() -> Result<int, string> { return Ok(1) }\nfunc b() -> Result<int, string> { return Ok(2) }\nfunc c() -> Result<string, int> { return Err(3) }\nfunc main() {}",
    )
    .expect("checked");
    assert_eq!(typed.results().len(), 2);

    // Neither side can be inferred from the other, so a constructor needs an
    // expected type; `?` needs an enclosing `Result` with the same error type.
    fails("func main() { let x = Ok(1) }", DiagnosticCode::UnknownType);
    fails(
        "func main() { let x = Err(\"boom\") }",
        DiagnosticCode::UnknownType,
    );
    fails(
        "func f() -> Result<int, string> { return Ok(\"text\") }\nfunc main() {}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func f() -> Result<int, string> { return Ok(1, 2) }\nfunc main() {}",
        DiagnosticCode::ArgumentCount,
    );
    fails("func main() { let f = Ok }", DiagnosticCode::ArgumentCount);
    fails(
        "func f() -> Result<int, string> { return Ok(1) }\nfunc main() { let x = f()? }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "enum E { Bad }\nfunc f() -> Result<int, string> { return Ok(1) }\nfunc g() -> Result<int, E> { return Ok(f()?) }\nfunc main() {}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func f() -> Result<int, string> { let x = 1? \n return Ok(x) }\nfunc main() {}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func f() -> Result<int, string> { return Ok(1) }\nfunc main() { match f() { Ok(v): print(v) } }",
        DiagnosticCode::NonExhaustiveMatch,
    );
    fails(
        "func main() { let x: Option<int> = 1\n if let Ok(v) = x { print(v) } }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "struct Result { x: int }\nfunc main() {}",
        DiagnosticCode::DuplicateDeclaration,
    );
    // A value type cannot reach itself through a `Result`, which stores both
    // payloads inline and so adds no indirection.
    fails(
        "struct Node { next: Result<Node, string> }\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn sized_integer_type_checking() {
    valid(
        "func main() {\n    let a: i8 = -128\n    let b: u8 = 255\n    let c: i16 = -32768\n    let d: u16 = 65535\n    let e: i32 = -2147483648\n    let f: u32 = 4294967295\n    let g: i64 = -9223372036854775808\n    let h: u64 = 18446744073709551615\n    print(a)\n    print(b)\n    print(c)\n    print(d)\n    print(e)\n    print(f)\n    print(g)\n    print(h)\n}",
    );
    // `int` and `i64` name one type, so neither needs converting to the other.
    valid("func f(x: i64) -> int { return x }\nfunc main() { print(f(1)) }");
    let typed = check("func main() { let x: u8 = 1 }").expect("checked");
    assert_eq!(
        typed.expression_type(crate::span::Span::new(26, 27)),
        Some(Type::Int(crate::types::IntType::U8))
    );

    // A literal takes the width the context expects and is range-checked there.
    for source in [
        "func main() { let x: u8 = 256 }",
        "func main() { let x: i8 = 128 }",
        "func main() { let x: i32 = 2147483648 }",
        "func main() { let x: u16 = 65536 }",
        "func main() { let x: int = 9223372036854775808 }",
    ] {
        fails(source, DiagnosticCode::IntegerRange);
    }
    valid("func main() { let x: i8 = -128\nprint(x) }");

    // Widths never mix implicitly, in either direction.
    fails(
        "func main() { let a: u8 = 1\nlet b: int = 2\nprint(a + b) }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { let a: u8 = 1\nlet b: i32 = a }",
        DiagnosticCode::TypeMismatch,
    );
    // Negating an unsigned value has a result only for zero.
    fails(
        "func main() { let a: u8 = 5\nprint(-a) }",
        DiagnosticCode::InvalidOperator,
    );

    // Conversions are explicit, take one integer and yield the named width.
    valid("func main() { let a: u8 = 200\nprint(int(a) + 100) }");
    fails(
        "func main() { print(u8(\"text\")) }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { print(u8(1, 2)) }",
        DiagnosticCode::ArgumentCount,
    );
    fails("func main() { let f = u8 }", DiagnosticCode::ArgumentCount);
    // A literal argument is range-checked where it is written, not at run time.
    fails(
        "func main() { print(u8(256)) }",
        DiagnosticCode::IntegerRange,
    );
}

#[test]
fn string_bytes_and_slice_type_checking() {
    valid(
        "func main() {\n    let t = \"abc\"\n    print(t.len())\n    print(t[0])\n    print(t[0..2])\n    let b = t.bytes()\n    print(b.len())\n    print(b[0])\n    print(b[0..1].len())\n}",
    );
    let typed = check("func main() { let t = \"abc\"\nlet b = t[0] }").expect("checked");
    assert_eq!(
        typed.expression_type(crate::span::Span::new(36, 40)),
        Some(Type::Int(crate::types::IntType::U8))
    );

    // Indexing a string reads a byte, and slicing one yields a string.
    fails(
        "func main() { let t = \"abc\"\nlet v: int = t[0] }",
        DiagnosticCode::TypeMismatch,
    );
    // Endpoints and indexes are `int`, not some other width.
    fails(
        "func main() { let t = \"abc\"\nlet i: u8 = 1\nprint(t[i..2]) }",
        DiagnosticCode::TypeMismatch,
    );
    // Only arrays and strings index or slice.
    fails(
        "func main() { print((42)[0..1]) }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { print((42)[0]) }",
        DiagnosticCode::InvalidOperator,
    );
    // Strings are immutable, so an indexed write into one has no meaning.
    fails(
        "func main() { var t = \"abc\"\nt[0] = u8(65) }",
        DiagnosticCode::InvalidAssignment,
    );

    // Decoding bytes can fail, so it hands back a Result.
    valid(
        "func main() {\n    let b = \"abc\".bytes()\n    match bytes_to_string(b) {\n        Ok(t): print(t)\n        Err(e): print(e)\n    }\n}",
    );
    fails(
        "func main() { let n = [1, 2]\nmatch bytes_to_string(n) { Ok(t): print(t)\n Err(e): print(e) } }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { let f = bytes_to_string }",
        DiagnosticCode::ArgumentCount,
    );
}

#[test]
fn for_loop_type_checking() {
    valid(
        "func main() {\n    for i in 0..10 {\n        print(i)\n    }\n    for item in [\"a\", \"b\"] {\n        print(item)\n    }\n}",
    );
    fails(
        "func main() {\n    for i in 1.5..3.5 {}\n}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() {\n    for x in 42 {}\n}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() {\n    for i in 0..5 {\n        i = 1\n    }\n}",
        DiagnosticCode::ImmutableAssignment,
    );
}

#[test]
fn extern_signatures_accept_only_c_representable_types() {
    valid(
        "unsafe extern \"C\" { func write(fd: i32, buffer: *u8, count: u64) -> i64\n    func flush() }\nfunc main() { let n = write(1, ptr(\"hi\"), 2) }",
    );
    for source in [
        "unsafe extern \"C\" { func f(text: string) }\nfunc main() {}",
        "unsafe extern \"C\" { func f(bytes: []u8) }\nfunc main() {}",
        "unsafe extern \"C\" { func f(value: Option<int>) }\nfunc main() {}",
        "unsafe extern \"C\" { func f() -> string }\nfunc main() {}",
        "unsafe extern \"C\" { func f(p: *string) }\nfunc main() {}",
        // `void` is a return type, never a parameter or a payload.
        "unsafe extern \"C\" { func f(nothing: void) }\nfunc main() {}",
    ] {
        fails(source, DiagnosticCode::InvalidValueType);
    }
}

#[test]
fn extern_names_may_not_shadow_generated_symbols() {
    for source in [
        "unsafe extern \"C\" { func main() }\nfunc start() {}",
        "unsafe extern \"C\" { func skuld_fail() }\nfunc main() {}",
    ] {
        fails(source, DiagnosticCode::InvalidValueType);
    }
}

#[test]
fn foreign_calls_check_arguments_like_any_other_call() {
    let source =
        "unsafe extern \"C\" { func abs(value: i32) -> i32 }\nfunc main() { let n = abs(-1) }";
    let typed = check(source).expect("checked");
    let start = source.find("abs(-1)").expect("call site");
    assert_eq!(
        typed.expression_type(crate::span::Span::new(start, start + "abs(-1)".len())),
        Some(Type::Int(crate::types::IntType::I32))
    );
    // Widths never mix implicitly, at the boundary as anywhere else. A literal
    // still takes the width its context expects, so only a typed value clashes.
    fails(
        "unsafe extern \"C\" { func abs(value: i32) -> i32 }\nfunc main() { let x = 1\nlet n = abs(1) + x }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "unsafe extern \"C\" { func abs(value: i32) -> i32 }\nfunc main() { abs() }",
        DiagnosticCode::ArgumentCount,
    );
}

#[test]
fn ptr_borrows_strings_and_scalar_arrays() {
    valid(
        "func main() { let a = ptr(\"hi\")\nvar bytes: []u8 = []\nlet b = ptr(bytes)\nvar reals: []float = []\nlet c = ptr(reals) }",
    );
    for source in [
        "func main() { let p = ptr(1) }",
        "func main() { var names: []string = []\nlet p = ptr(names) }",
        "func main() { let p = ptr }",
        "func main() { let p = ptr(\"a\", \"b\") }",
    ] {
        let errors = check(source).expect_err("must fail");
        assert!(
            errors.iter().any(|error| matches!(
                error.code,
                DiagnosticCode::InvalidValueType | DiagnosticCode::ArgumentCount
            )),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn a_conversion_expects_a_width_of_a_literal_only() {
    // The expected width reaches a literal, which has no type of its own, and
    // stops there: a computed argument keeps its own type and is converted.
    valid("func main() { let n = 3\nprint(u8(128 + n % 64)) }");
    valid("func main() { var bytes: []u8 = []\nlet n = 200\nbytes.push(u8(n)) }");
    valid("func main() { let n = 3\nprint(i8(-n)) }");
    // A literal is still range-checked where it is written, signed or not.
    fails(
        "func main() { print(u8(256)) }",
        DiagnosticCode::IntegerRange,
    );
    fails(
        "func main() { print(i8(-129)) }",
        DiagnosticCode::IntegerRange,
    );
    fails(
        "func main() { print(u8((300))) }",
        DiagnosticCode::IntegerRange,
    );
    // And a width still never mixes with another on its own.
    fails(
        "func main() { let n: i32 = 3\nlet m: int = 4\nprint(n + m) }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn a_declared_function_is_a_value_of_its_own_signature() {
    // Naming a function without calling it used to be an error; M8 makes it a
    // function value that happens to capture nothing.
    let source = "func twice(n: int) -> int { return n * 2 }\nfunc main() { let f = twice }";
    let typed = crate::check(source).expect("a declared function is a value");
    let start = source.find("twice }").expect("the use");
    assert!(matches!(
        typed.expression_type(crate::span::Span::new(start, start + "twice".len())),
        Some(Type::Function(_))
    ));
}

#[test]
fn a_function_value_may_not_be_stored_where_a_managed_value_could_reach_it() {
    // The escape rule is the whole safety argument: a closure nothing managed
    // can reach cannot be half of a cycle.
    for source in [
        "class Holder { action: (int) -> int }\nfunc main() { }",
        "enum Wrap { V((int) -> int) }\nfunc main() { }",
        "func give() -> (int) -> int { return (n: int): int { return n } }\nfunc main() { }",
        "func main() { let a: [](int) -> int = [] }",
        "func main() { let o: Option<(int) -> int> = null }",
    ] {
        fails(source, DiagnosticCode::InvalidValueType);
    }
}

#[test]
fn a_lambda_parameter_needs_a_type_when_nothing_supplies_one() {
    fails(
        "func main() { let f = (n): int { return n } }",
        DiagnosticCode::UnknownType,
    );
}

#[test]
fn a_function_value_is_called_with_its_own_signature() {
    fails(
        "func apply(f: (int) -> int) -> int { return f(1, 2) }\nfunc main() { }",
        DiagnosticCode::ArgumentCount,
    );
    fails(
        "func apply(f: (int) -> int) -> int { return f(\"x\") }\nfunc main() { }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { let n = 1\n n() }",
        DiagnosticCode::NotCallable,
    );
}

#[test]
fn the_checked_tables_are_reachable_from_outside() {
    // A tool that offers a field, a method or a signature reads these; the
    // language server is the first caller and must not rebuild them.
    let typed = check(
        "class User {\n    name: string\n\n    greet() -> string { return this.name }\n}\nfunc twice(n: int) -> int { return n * 2 }\nfunc main() { let u = new User(name: \"Ada\")\nprint(u.greet()) }",
    )
    .expect("checked");
    let user = typed
        .structs()
        .iter()
        .find(|s| s.name == "User")
        .expect("the declared class");
    assert_eq!(
        user.fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["name"]
    );
    assert_eq!(
        user.methods
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["greet"]
    );
    // A binding's type is reachable through the symbol its use resolves to.
    let source_of = |needle: &str| {
        let text = "class User {\n    name: string\n\n    greet() -> string { return this.name }\n}\nfunc twice(n: int) -> int { return n * 2 }\nfunc main() { let u = new User(name: \"Ada\")\nprint(u.greet()) }";
        text.rfind(needle).expect("occurrence")
    };
    let use_of_u = source_of("u.greet");
    let symbol = typed.resolution().references[&(crate::module::FileId(0), use_of_u)];
    assert!(matches!(typed.symbol_type(symbol), Type::Struct(_)));
    let twice =
        typed.resolution().declarations[&(crate::module::FileId(0), source_of("twice(n: int)"))];
    let signature = typed.signature(twice).expect("a declared function");
    assert_eq!(signature.parameters, vec![Type::INT]);
    assert_eq!(signature.return_type, Type::INT);
}

#[test]
fn an_escape_block_must_not_fall_through() {
    let prelude = "func fallible() -> Result<int, string> { return Ok(1) }\n";
    // The name is in scope after the statement, so falling past the block
    // would leave it unbound.
    fails(
        &format!("{prelude}func main() {{ let v = fallible() else r {{ print(r) }}\n print(v) }}"),
        DiagnosticCode::MissingReturn,
    );
    fails(
        &format!(
            "{prelude}func main() {{ let v = fallible() else r {{ if r.len() > 0 {{ return }} }}\n print(v) }}"
        ),
        DiagnosticCode::MissingReturn,
    );
    valid(&format!(
        "{prelude}func main() {{ let v = fallible() else r {{ print(r)\n return }}\n print(v) }}"
    ));
    // A jump leaves the block as surely as a return does.
    valid(&format!(
        "{prelude}func main() {{ for i in 0..2 {{ let v = fallible() else r {{ continue }}\n print(v) }} }}"
    ));
}

#[test]
fn an_escape_block_unwraps_an_option_or_a_result() {
    fails(
        "func main() { let v = 42 else { return }\n print(v) }",
        DiagnosticCode::TypeMismatch,
    );
    // An Option carries no error, so there is nothing to name.
    fails(
        "func maybe() -> Option<int> { return 1 }\nfunc main() { let v = maybe() else r { return }\n print(v) }",
        DiagnosticCode::TypeMismatch,
    );
    valid(
        "func maybe() -> Option<int> { return 1 }\nfunc main() { let v = maybe() else { return }\n print(v) }",
    );
}

#[test]
fn an_unwrapped_name_has_the_payload_type() {
    let source = "func fallible() -> Result<int, string> { return Ok(1) }\nfunc main() { let v = fallible() else r { return }\n print(v) }";
    let typed = check(source).expect("a valid escape binding");
    let start = source.rfind("v) }").expect("the use");
    assert_eq!(
        typed.expression_type(crate::span::Span::new(start, start + 1)),
        Some(Type::INT)
    );
}

#[test]
fn conformance_is_declared_and_checked_method_by_method() {
    let interface = "interface Printable { show() -> string }\n";
    valid(&format!(
        "{interface}class P: Printable {{ x: int\n show() -> string {{ return \"x\" }} }}\nfunc main() {{ }}"
    ));
    // Having the methods is not enough; saying so is what counts.
    fails(
        &format!(
            "{interface}class P {{ show() -> string {{ return \"x\" }} }}\nfunc r(v: Printable) {{ print(v.show()) }}\nfunc main() {{ r(new P()) }}"
        ),
        DiagnosticCode::TypeMismatch,
    );
    fails(
        &format!("{interface}class P: Printable {{ x: int }}\nfunc main() {{ }}"),
        DiagnosticCode::MissingField,
    );
    fails(
        &format!(
            "{interface}class P: Printable {{ show() -> int {{ return 1 }} }}\nfunc main() {{ }}"
        ),
        DiagnosticCode::TypeMismatch,
    );
    // A struct has no identity to put behind an interface.
    fails(
        &format!(
            "{interface}struct P: Printable {{ x: int\n show() -> string {{ return \"x\" }} }}\nfunc main() {{ }}"
        ),
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn an_interface_value_answers_only_what_the_interface_declares() {
    let program = "interface Printable { show() -> string }\nclass P: Printable { show() -> string { return \"x\" }\n hidden() -> int { return 1 } }\n";
    valid(&format!(
        "{program}func r(v: Printable) {{ print(v.show()) }}\nfunc main() {{ r(new P()) }}"
    ));
    fails(
        &format!(
            "{program}func r(v: Printable) {{ print(v.hidden()) }}\nfunc main() {{ r(new P()) }}"
        ),
        DiagnosticCode::NotCallable,
    );
}

#[test]
fn a_class_widens_into_an_expected_option_of_an_interface() {
    // Two implicit steps compose here, and only here: seen through the
    // interface, then wrapped.
    valid(
        "interface P { show() -> string }\nclass C: P { show() -> string { return \"x\" } }\nfunc main() { let v: Option<P> = new C()\n if let p = v { print(p.show()) } }",
    );
}

#[test]
fn a_field_with_a_default_may_be_left_out_of_a_construction() {
    let class = "class User {\n    name: string = \"anonymous\"\n    age: int = 0\n}\n";
    valid(&format!(
        "{class}func main() {{ let u = new User()\nprint(u.name) }}"
    ));
    valid(&format!(
        "{class}func main() {{ let u = new User(age: 7)\nprint(u.name) }}"
    ));
    // A field without a default is still required.
    fails(
        "class User {\n    name: string\n    age: int = 0\n}\nfunc main() { let u = new User()\nprint(u.name) }",
        DiagnosticCode::MissingField,
    );
    // The default has to be the field's type.
    fails(
        "class User {\n    age: int = \"old\"\n}\nfunc main() { let u = new User()\nprint(u.age) }",
        DiagnosticCode::TypeMismatch,
    );
    // A struct takes defaults the same way.
    valid(
        "struct Point {\n    x: int = 0\n    y: int = 0\n}\nfunc main() { let p = Point { y: 2 }\nprint(p.x) }",
    );
}

#[test]
fn an_entrypoint_is_required_of_a_program_and_optional_for_a_tool() {
    // A library file is not a program, and a tool showing one must be able to
    // check it; the compiler must still refuse to build it.
    let library = "pub func twice(value: int) -> int {\n    return value * 2\n}\n";
    let errors = check(library).expect_err("a program needs an entrypoint");
    assert!(
        errors
            .iter()
            .any(|error| error.code == DiagnosticCode::InvalidEntrypoint)
    );

    let typed = crate::check_program_with(
        "library.skuld",
        library,
        &mut crate::module::NoModules,
        crate::type_checker::Entrypoint::Optional,
        crate::type_checker::Mode::Hosted,
    )
    .expect("a library checks when the entrypoint is optional");
    assert_eq!(typed.entry(), None);
    // Everything else is checked exactly as it would have been.
    assert!(typed.resolution().symbols.iter().any(|s| s.name == "twice"));

    // A mistake in the file is still a mistake, entrypoint or not.
    crate::check_program_with(
        "library.skuld",
        "pub func twice(value: int) -> int {\n    return \"two\"\n}\n",
        &mut crate::module::NoModules,
        crate::type_checker::Entrypoint::Optional,
        crate::type_checker::Mode::Hosted,
    )
    .expect_err("a wrong return type is still wrong");
}

/// Apply one offered fix at a time, re-checking in between, the way an editor
/// does: two fixes reported together are alternatives at the same point, and
/// each one is written as if it were applied to the text that was checked.
fn apply_fixes(source: &str) -> String {
    let mut text = source.to_string();
    for _ in 0..10 {
        let Err(errors) = check(&text) else { break };
        let Some(fix) = errors.iter().find_map(|error| error.fix.as_deref()) else {
            break;
        };
        text.replace_range(fix.span.start..fix.span.end, &fix.replacement);
    }
    text
}

#[test]
fn a_non_exhaustive_match_offers_the_arms_it_is_missing() {
    let source = "enum Status {\n    Pending,\n    Active,\n    Cancelled(string)\n}\nfunc report(s: Status) {\n    match s {\n        Status.Pending: print(\"pending\")\n    }\n}\nfunc main() { report(Status.Active) }";
    let errors = check(source).expect_err("must fail");
    assert_eq!(errors.len(), 2, "one per uncovered variant: {errors:?}");
    let titles: Vec<&str> = errors
        .iter()
        .filter_map(|error| error.fix.as_deref())
        .map(|fix| fix.title.as_str())
        .collect();
    assert_eq!(
        titles,
        vec![
            "add an arm for `Status.Active`",
            // A variant with a payload binds it, since a pattern must say
            // where the payload goes.
            "add an arm for `Status.Cancelled(value)`"
        ]
    );
    // Each arm lands inside the block, at the indentation its arms use.
    let fixed = apply_fixes(source);
    assert!(
        fixed.contains(
            "        Status.Pending: print(\"pending\")\n        Status.Active: {}\n        Status.Cancelled(value): {}\n    }"
        ),
        "{fixed}"
    );
    assert!(check(&fixed).is_ok(), "{:?}", check(&fixed));
}

#[test]
fn a_match_on_a_result_is_offered_the_patterns_a_result_uses() {
    let source = "func read() -> Result<int, string> { return Ok(1) }\nfunc main() {\n    match read() {\n        Ok(value): print(value)\n    }\n}";
    let errors = check(source).expect_err("must fail");
    let fix = errors[0].fix.as_deref().expect("an edit");
    // `Err(e)`, not `Result.Err(e)`: a `Result` pattern names no type.
    assert_eq!(fix.title, "add an arm for `Err(value)`");
    let fixed = apply_fixes(source);
    assert!(check(&fixed).is_ok(), "{fixed}\n{:?}", check(&fixed));
}

#[test]
fn a_match_written_on_one_line_still_places_its_arm() {
    let source = "enum Flag { On, Off }\nfunc main() {\n    match Flag.On { Flag.On: print(1) }\n}";
    let fixed = apply_fixes(source);
    assert!(check(&fixed).is_ok(), "{fixed}\n{:?}", check(&fixed));
}

#[test]
fn assigning_to_a_let_offers_to_make_it_a_var() {
    let source = "func main() {\n    let total = 1\n    total = 2\n    print(total)\n}";
    let errors = check(source).expect_err("must fail");
    let fix = errors[0].fix.as_deref().expect("an edit");
    assert_eq!(fix.title, "declare it with `var`");
    let fixed = apply_fixes(source);
    assert_eq!(
        fixed,
        "func main() {\n    var total = 1\n    total = 2\n    print(total)\n}"
    );
    assert!(check(&fixed).is_ok(), "{:?}", check(&fixed));
}

#[test]
fn a_declaration_that_unwraps_takes_the_same_edit() {
    // `var name = value else { ... }` is as valid as the `let` form, so the
    // keyword is the whole difference here too.
    let source = "func maybe() -> Option<int> { return 3 }
func main() {
    let value = maybe() else {
        return
    }
    value = 4
    print(value)
}";
    let fixed = apply_fixes(source);
    assert!(fixed.contains("    var value = maybe() else {"), "{fixed}");
    assert!(check(&fixed).is_ok(), "{:?}", check(&fixed));
}

#[test]
fn a_binding_with_no_let_to_change_is_offered_nothing() {
    // A parameter is immutable by design, and an arm binding has no keyword
    // of its own: neither has a `let` to rewrite.
    for source in [
        "func f(value: int) { value = 1 }\nfunc main() { f(1) }",
        "enum Tag { One(int) }\nfunc main() {\n    match Tag.One(1) {\n        Tag.One(value): value = 2\n    }\n}",
        "func main() {\n    for step in 0..3 {\n        step = 1\n    }\n}",
    ] {
        let errors = check(source).expect_err("must fail");
        let assignment = errors
            .iter()
            .find(|error| error.code == DiagnosticCode::ImmutableAssignment)
            .unwrap_or_else(|| panic!("{source}: {errors:?}"));
        assert!(assignment.fix.is_none(), "{source}: {assignment:?}");
    }
}

#[test]
fn a_misspelt_member_names_the_one_the_type_has() {
    // A field in a construction, a field being read, a method being called,
    // and an enum variant: each is searched among the members that exist.
    for (source, title) in [
        (
            "class User { name: string }\nfunc main() { let u = new User(nmae: \"a\")\nprint(u.name) }",
            "change to `name`",
        ),
        (
            "class User { name: string }\nfunc main() { let u = new User(name: \"a\")\nprint(u.naem) }",
            "change to `name`",
        ),
        (
            "class User { name: string\n    hello() { print(this.name) } }\nfunc main() { let u = new User(name: \"a\")\nu.helo() }",
            "change to `hello`",
        ),
        (
            "enum Status { Pending, Active }\nfunc main() { let s = Status.Actve\nmatch s { _: print(1) } }",
            "change to `Active`",
        ),
    ] {
        let errors = check(source).expect_err("must fail");
        let fix = errors
            .iter()
            .find_map(|error| error.fix.as_deref())
            .unwrap_or_else(|| panic!("{source}: {errors:?}"));
        assert_eq!(fix.title, title, "{source}");
        let fixed = apply_fixes(source);
        assert!(check(&fixed).is_ok(), "{fixed}\n{:?}", check(&fixed));
    }
}

#[test]
fn a_member_nothing_resembles_is_not_guessed_at() {
    let errors = check("class User { name: string }\nfunc main() { let u = new User(name: \"a\")\nprint(u.elephant) }")
        .expect_err("must fail");
    assert!(errors.iter().all(|error| error.fix.is_none()), "{errors:?}");
}

#[test]
fn a_missing_field_is_named_with_the_type_it_wants() {
    let errors = check(
        "class User {\n    name: string\n    age: int\n    active: bool = true\n}\nfunc main() {\n    let u = new User(name: \"a\")\n    print(u.name)\n}",
    )
    .expect_err("must fail");
    let diagnostic = &errors[0];
    assert_eq!(diagnostic.code, DiagnosticCode::MissingField);
    // `active` has a default and is not missing; `age` has none.
    assert_eq!(diagnostic.message, "`User` is missing `age`");
    assert_eq!(diagnostic.help.as_deref(), Some("give it `age: int`"));
    // No fix: which value goes there is the one thing the compiler does not
    // know, and Skuld has no zero value to stand in for it.
    assert!(diagnostic.fix.is_none());
}

#[test]
fn bitwise_and_shift_type_checking() {
    valid(
        "func main() {\n    let a: u32 = 0xFF00\n    let b: u32 = 0x00FF\n    let c = (a | b) & ~b ^ 0x10\n    let d = c << 2\n    let e = d >> 1\n    var v: int = 10\n    v &= 3\n    v |= 4\n    v ^= 1\n    v <<= 2\n    v >>= 1\n}",
    );

    // Bitwise operators do not accept bools or floats
    fails(
        "func main() { let x = true & false }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { let x = true | false }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { let x = true ^ false }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { let x = ~true }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { let x = 1.0 & 2.0 }",
        DiagnosticCode::InvalidOperator,
    );
    fails(
        "func main() { let x = 1.0 << 2 }",
        DiagnosticCode::InvalidOperator,
    );

    // Bitwise operators never mix widths implicitly
    fails(
        "func main() { let a: u8 = 1\nlet b: u16 = 2\nlet c = a & b }",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "func main() { let a: u8 = 1\nlet b: int = 2\nlet c = a << b }",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn expression_lambdas_and_to_sorted() {
    valid(
        "func main() {\n    let add = (a: int, b: int) => a + b\n    let numbers = [5, 2, 8, 1]\n    let sorted_nums = numbers.to_sorted((a, b) => a - b)\n    numbers.sort((a, b) => a - b)\n}",
    );

    // Assigning the result of `sort` (which returns void) suggests `to_sorted`
    let errors = check(
        "func main() {\n    let numbers = [1, 2]\n    var s = numbers.sort((a, b) => a - b)\n}",
    )
    .expect_err("must fail");
    let diagnostic = &errors[0];
    assert_eq!(diagnostic.code, DiagnosticCode::InvalidValueType);
    assert!(
        diagnostic
            .help
            .as_deref()
            .unwrap()
            .contains("use `to_sorted`")
    );
}

#[test]
fn pointer_reads_and_writes_need_an_unsafe_block() {
    valid(
        "func main() {\n    var cell: int = 1\n    unsafe {\n        let p = ptr(cell)\n        store(p, load(p) + 1)\n        let stepped = offset(p, 1)\n        let address = addr(stepped)\n        let back: *i64 = ptr_from(address)\n        print(load(back))\n    }\n}",
    );
    // The block is what suspends the rule, so leaving it puts the rule back.
    fails(
        "func main() {\n    var cell: int = 1\n    unsafe {\n        let p = ptr(cell)\n    }\n    print(load(ptr(cell)))\n}",
        DiagnosticCode::RequiresUnsafe,
    );
    // An inner function is not inside the block that encloses its call.
    fails(
        "func read(p: *i64) -> int {\n    return load(p)\n}\nfunc main() {\n    var cell: int = 1\n    unsafe {\n        print(read(ptr(cell)))\n    }\n}",
        DiagnosticCode::RequiresUnsafe,
    );
}

#[test]
fn a_pointer_operation_keeps_the_type_its_pointee_names() {
    // The value written has to be the type the pointer points at; nothing
    // widens on the way through.
    fails(
        "func main() {\n    var cell: u8 = 1\n    unsafe {\n        let p = ptr(cell)\n        store(p, 300)\n    }\n}",
        DiagnosticCode::IntegerRange,
    );
    fails(
        "func main() {\n    var cell: int = 1\n    unsafe {\n        store(ptr(cell), true)\n    }\n}",
        DiagnosticCode::TypeMismatch,
    );
    // `*void` points at no particular value, so there is nothing to read.
    fails(
        "unsafe extern \"C\" {\n    func opaque() -> *void\n}\nfunc main() {\n    unsafe {\n        print(load(opaque()))\n    }\n}",
        DiagnosticCode::InvalidValueType,
    );
    // An address is a `usize` in both directions.
    fails(
        "func main() {\n    var cell: int = 1\n    unsafe {\n        let address: int = addr(ptr(cell))\n    }\n}",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn an_address_is_only_taken_of_a_mutable_local() {
    fails(
        "func main() {\n    let cell: int = 1\n    unsafe {\n        let p = ptr(cell)\n    }\n}",
        DiagnosticCode::ImmutableAssignment,
    );
    // A parameter is a copy the caller cannot see, so its address is refused
    // along with everything else that is not a local.
    fails(
        "func write(value: int) {\n    unsafe {\n        let p = ptr(value)\n    }\n}\nfunc main() {\n    write(1)\n}",
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn a_declared_layout_is_what_crosses_the_boundary_and_what_can_be_measured() {
    valid(
        "extern struct Point {\n    x: i32,\n    y: i32,\n}\n\nunsafe extern \"C\" {\n    func take(p: Point) -> i32\n    func make() -> Point\n}\n\nfunc main() {\n    let p = Point { x: 1, y: 2 }\n    print(take(p))\n    print(int(size_of(Point)))\n    print(int(offset_of(Point, y)))\n}",
    );
    // A managed field would put a reference count inside a layout C decides.
    fails(
        "extern struct Bad {\n    name: string,\n}\n\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
    // So would a struct whose own layout is unspecified.
    fails(
        "struct Inner {\n    x: int,\n}\n\nextern struct Outer {\n    inner: Inner,\n}\n\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
    // The compiler's layout is unspecified on purpose, so it cannot be asked
    // about.
    fails(
        "struct Point {\n    x: int,\n}\n\nfunc main() {\n    print(int(size_of(Point)))\n}",
        DiagnosticCode::InvalidValueType,
    );
    fails(
        "extern struct Point {\n    x: i32,\n}\n\nfunc main() {\n    print(int(offset_of(Point, y)))\n}",
        DiagnosticCode::MissingField,
    );
    // Neither argument is a value, and neither is an expression.
    fails(
        "extern struct Point {\n    x: i32,\n}\n\nfunc main() {\n    print(int(size_of(1 + 1)))\n}",
        DiagnosticCode::ExpectedSyntax,
    );
    // `align` describes an alignment, so it has to be one.
    fails(
        "extern struct Bad align 6 {\n    x: u8,\n}\n\nfunc main() {}",
        DiagnosticCode::IntegerRange,
    );
}

#[test]
fn a_numbered_enum_converts_to_its_integer_and_back() {
    valid(
        "enum Protocol: u8 {\n    Tcp = 6,\n    Udp = 17,\n}\n\nfunc main() {\n    print(int(u8(Protocol.Tcp)))\n    let back = Protocol(u8(17))\n    print(i32(back))\n}",
    );
    // A value continues from the one before it, so a plain list numbers itself.
    valid(
        "enum Level: i32 {\n    Low,\n    High,\n}\n\nfunc main() {\n    print(i32(Level.High))\n}",
    );
    // An enum without an underlying type has no value to convert either way.
    fails(
        "enum Status {\n    On,\n}\n\nfunc main() {\n    print(int(u8(Status.On)))\n}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "enum Status {\n    On,\n}\n\nfunc main() {\n    let s = Status(0)\n}",
        DiagnosticCode::UnsupportedFeature,
    );
    // Two variants worth the same number would make the conversion back
    // ambiguous.
    fails(
        "enum Protocol: u8 {\n    Tcp = 6,\n    Other = 6,\n}\n\nfunc main() {}",
        DiagnosticCode::DuplicateDeclaration,
    );
    // A payload has no integer value.
    fails(
        "enum Mixed: u8 {\n    Carrying(int) = 1,\n}\n\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
    // The conversion back takes the underlying width, not any integer.
    fails(
        "enum Protocol: u8 {\n    Tcp = 6,\n}\n\nfunc main() {\n    let p = Protocol(6.5)\n}",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn a_union_is_written_one_member_at_a_time_and_read_under_a_claim() {
    valid(
        "extern union Word {\n    whole: u32,\n    bytes: [4]u8,\n}\n\nfunc main() {\n    var w = Word { bytes: [0; 4] }\n    w.whole = u32(1)\n    unsafe {\n        print(int(w.bytes[0]))\n    }\n}",
    );
    // Reading a member is the claim, so it needs the block.
    fails(
        "extern union Word {\n    whole: u32,\n    bytes: [4]u8,\n}\n\nfunc main() {\n    let w = Word { whole: u32(1) }\n    print(int(w.whole))\n}",
        DiagnosticCode::RequiresUnsafe,
    );
    // A compound assignment reads before it writes, so it is not a plain
    // write.
    fails(
        "extern union Word {\n    whole: u32,\n    bytes: [4]u8,\n}\n\nfunc main() {\n    var w = Word { whole: u32(1) }\n    w.whole += u32(1)\n}",
        DiagnosticCode::RequiresUnsafe,
    );
    // One member, not none and not two.
    fails(
        "extern union Word {\n    whole: u32,\n    bytes: [4]u8,\n}\n\nfunc main() {\n    let w = Word { whole: u32(1), bytes: [0; 4] }\n}",
        DiagnosticCode::MissingField,
    );
    fails(
        "extern union Word {\n    whole: u32,\n}\n\nfunc main() {\n    let w = Word {}\n}",
        DiagnosticCode::MissingField,
    );
    // And its members are still what C can describe.
    fails(
        "extern union Bad {\n    text: string,\n}\n\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
}

#[test]
fn any_pointer_is_also_an_opaque_one() {
    valid(
        "unsafe extern \"C\" {\n    func take(value: *void) -> i32\n}\n\nextern struct Point {\n    x: i32,\n}\n\nfunc main() {\n    let text = \"hi\"\n    print(take(ptr(text)))\n    var bytes: [4]u8 = [0; 4]\n    print(take(ptr(bytes)))\n    let point = Point { x: 1 }\n    print(take(ptr(point)))\n}",
    );
    // It goes one way only: an opaque pointer claims nothing about what it
    // points at, so it cannot become a pointer that does.
    fails(
        "unsafe extern \"C\" {\n    func take(value: *u8) -> i32\n    func give() -> *void\n}\n\nfunc main() {\n    print(take(give()))\n}",
        DiagnosticCode::TypeMismatch,
    );
}

#[test]
fn a_deferred_statement_may_not_leave_its_block() {
    valid(
        "func main() {\n    defer print(1)\n    defer {\n        print(2)\n    }\n    print(3)\n}",
    );
    // A `defer` runs while the block is already being left, so leaving again
    // has nothing to mean.
    fails(
        "func main() {\n    defer {\n        return\n    }\n}",
        DiagnosticCode::UnsupportedSyntax,
    );
    fails(
        "func main() {\n    while true {\n        defer break\n    }\n}",
        DiagnosticCode::UnsupportedSyntax,
    );
    fails(
        "func inner() -> Result<int, string> {\n    return Ok(1)\n}\nfunc run() -> Result<int, string> {\n    defer {\n        let value = inner()?\n    }\n    return Ok(0)\n}\nfunc main() {}",
        DiagnosticCode::UnsupportedSyntax,
    );
    // A deferred declaration binds a name nothing can read.
    fails(
        "func main() {\n    defer let value = 1\n}",
        DiagnosticCode::UnsupportedSyntax,
    );
    // The deferred statement is checked like any other.
    fails(
        "func main() {\n    defer print(missing)\n}",
        DiagnosticCode::UnknownName,
    );
}

#[test]
fn a_static_is_storage_that_outlives_every_call() {
    valid(
        "static counter: int = 0\n\nfunc bump() -> int {\n    counter = counter + 1\n    return counter\n}\n\nfunc main() {\n    print(bump())\n}",
    );
    // A fixed array of scalars, which is the storage a program with no heap
    // has: it starts at zero and is filled while the program runs.
    valid(
        "static bytes: [4]u8 = [0; 4]\n\nfunc main() {\n    bytes[0] = u8(1)\n    print(int(bytes[0]))\n}",
    );
    // Nothing would retain or release a managed value that outlives every
    // call, so there is none.
    fails(
        "static name: string = \"hello\"\n\nfunc main() {}",
        DiagnosticCode::InvalidValueType,
    );
    // The initialiser runs at compile time, because there is no moment before
    // the program starts at which it could run.
    fails(
        "func compute() -> int {\n    return 1\n}\n\nstatic value: int = compute()\n\nfunc main() {}",
        DiagnosticCode::UnsupportedFeature,
    );
    fails(
        "static bytes: [4]u8 = [1; 4]\n\nfunc main() {}",
        DiagnosticCode::UnsupportedSyntax,
    );
    fails(
        "static flag: bool = 1\n\nfunc main() {}",
        DiagnosticCode::TypeMismatch,
    );
    fails(
        "static level: u8 = 300\n\nfunc main() {}",
        DiagnosticCode::IntegerRange,
    );
}

/// The freestanding subset: one language, two build modes, and the checker
/// saying which one a file is being compiled in.
fn freestanding(source: &str) -> Result<(), Vec<DiagnosticCode>> {
    crate::check_program_with(
        "kernel.skuld",
        source,
        &mut crate::module::NoModules,
        crate::type_checker::Entrypoint::Optional,
        crate::type_checker::Mode::Freestanding,
    )
    .map(|_| ())
    .map_err(|errors| {
        errors
            .diagnostics
            .into_iter()
            .map(|error| error.diagnostic.code)
            .collect()
    })
}

#[test]
fn a_freestanding_program_holds_what_it_can_count_for_itself() {
    // Scalars, fixed arrays, structs, enums and pointers — and no `main`,
    // since something else starts it.
    freestanding(
        "extern struct Register {\n    value: u32,\n}\n\nenum State: u8 {\n    Off = 0,\n    On = 1,\n}\n\nstatic seen: [4]u8 = [0; 4]\n\npub func tick(state: State) -> u32 {\n    seen[0] = u8(state)\n    let register = Register { value: u32(1) }\n    unsafe {\n        let cell: *u32 = ptr_from(usize(753664))\n        volatile_store(cell, register.value)\n    }\n    return register.value\n}",
    )
    .expect("the subset checks");

    // Everything that would need the runtime is refused where it is written.
    for source in [
        "pub func boot() {\n    let message = \"hello\"\n}",
        "pub func boot() {\n    var numbers: []int = []\n}",
        "class Thing {\n    value: int,\n}\n\npub func boot() {\n    let thing = new Thing(value: 1)\n}",
        "pub func boot() -> string {\n    return \"no\"\n}",
    ] {
        let codes = freestanding(source).expect_err("a managed value is refused");
        assert!(
            codes.contains(&DiagnosticCode::UnsupportedFeature),
            "{source}: {codes:?}"
        );
    }

    // `print` has nowhere to print to.
    let codes = freestanding("pub func boot() {\n    print(1)\n}").expect_err("no stdout");
    assert!(codes.contains(&DiagnosticCode::UnsupportedFeature));

    // And the same program is ordinary in a hosted build, which is the point
    // of there being one language.
    valid("pub func boot() {\n    print(1)\n}\n\nfunc main() {\n    boot()\n}");
}
