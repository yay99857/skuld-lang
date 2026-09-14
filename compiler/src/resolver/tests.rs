use super::*;
use crate::types::IntType;
fn output(source: &str) -> ResolveOutput {
    let program = crate::module::load("<test>", source, &mut crate::module::NoModules)
        .expect("a single-file program parses");
    resolve(&program)
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
            result.diagnostics[0].diagnostic.code,
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
        assert_eq!(
            result.diagnostics[0].diagnostic.code,
            DiagnosticCode::UnknownName
        );
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
    let names: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| &d.diagnostic.message)
        .collect();
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
fn result_constructors_are_prelude_bindings() {
    let result = valid("func f() { Ok(1)\nErr(2) }");
    let kinds: Vec<_> = result
        .references
        .values()
        .map(|id| result.symbols[id.0].kind)
        .collect();
    assert!(kinds.contains(&SymbolKind::Builtin(Builtin::Ok)));
    assert!(kinds.contains(&SymbolKind::Builtin(Builtin::Err)));
    // Like the rest of the prelude, they are ordinary bindings a user can
    // shadow; the checker must identify builtins by symbol, not by spelling.
    let result = valid("func f() { let Ok = 1\nOk }");
    let id = result.references.values().next().expect("use");
    assert_eq!(
        result.symbols[id.0].kind,
        SymbolKind::Variable(Mutability::Immutable)
    );
}

#[test]
fn try_operand_and_result_if_let_bindings_resolve() {
    let result = output(
        "func read() -> Result<int, string> { return Ok(1) }\nfunc main() {\n    if let Ok(value) = read() { print(value) }\n    if let Err(reason) = read() { print(reason) }\n}",
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    // The binding belongs to the then-block, not to the enclosing scope.
    let result = output(
        "func read() -> Result<int, string> { return Ok(1) }\nfunc main() {\n    if let Ok(value) = read() {}\n    print(value)\n}",
    );
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].diagnostic.code,
        DiagnosticCode::UnknownName
    );
}

#[test]
fn width_conversions_and_decoding_are_prelude_bindings() {
    let result = valid("func f() { u8(1)\nint(2)\ni64(3)\nbytes_to_string(4) }");
    let kinds: Vec<_> = result
        .references
        .values()
        .map(|id| result.symbols[id.0].kind)
        .collect();
    assert!(kinds.contains(&SymbolKind::Builtin(Builtin::IntConvert(IntType::U8))));
    // `int` and `i64` are two spellings of one width, so both resolve to it.
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SymbolKind::Builtin(Builtin::IntConvert(IntType::I64)))
            .count(),
        2
    );
    assert!(kinds.contains(&SymbolKind::Builtin(Builtin::BytesToString)));

    // They are ordinary prelude bindings, so a user may shadow them.
    let result = valid("func f() { let u8 = 1\nu8 }");
    let id = result.references.values().next().expect("use");
    assert_eq!(
        result.symbols[id.0].kind,
        SymbolKind::Variable(Mutability::Immutable)
    );
}

#[test]
fn slice_endpoints_resolve() {
    let result = output(
        "func main() {\n    let xs = [1, 2, 3]\n    let a = 0\n    let b = 2\n    print(xs[a..b].len())\n}",
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let result = output("func main() {\n    let xs = [1, 2, 3]\n    print(xs[lo..hi].len())\n}");
    assert_eq!(result.diagnostics.len(), 2);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.diagnostic.code == DiagnosticCode::UnknownName)
    );
}

#[test]
fn diagnostics_point_to_name_and_do_not_hide_following_errors() {
    let source = "func main() {\n    usr.greet()\n    other()\n}";
    let result = output(source);
    assert_eq!(result.diagnostics.len(), 2);
    let d = &result.diagnostics[0];
    assert_eq!(
        &source[d.diagnostic.span.start..d.diagnostic.span.end],
        "usr"
    );
    let rendered = d
        .diagnostic
        .render(&crate::span::SourceFile::new("main.skuld", source));
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
            .any(|d| d.diagnostic.code == DiagnosticCode::UnknownName)
    );
    // Methods are equally invisible from a plain function.
    let result = output(
        "struct R {\n    a() -> int {\n        return 1\n    }\n}\nfunc main() { print(a()) }",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == DiagnosticCode::UnknownName)
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
    assert_eq!(
        result.diagnostics[0].diagnostic.code,
        DiagnosticCode::UnknownName
    );
}

#[test]
fn extern_functions_are_predeclared_value_names() {
    let resolution = valid(
        "func main() { let n = abs(-1) }\nunsafe extern \"C\" { func abs(value: i32) -> i32 }",
    );
    // Declared like any function, and callable before its declaration.
    let id = resolution
        .references
        .values()
        .find(|id| resolution.symbols[id.0].name == "abs")
        .copied()
        .expect("reference to the foreign function");
    assert_eq!(resolution.symbols[id.0].kind, SymbolKind::Function);
    // Parameter names inside an extern block are documentation, not bindings.
    assert!(!resolution.symbols.iter().any(|s| s.name == "value"));
}

#[test]
fn extern_names_collide_with_ordinary_functions() {
    let result = output(
        "unsafe extern \"C\" { func abs(value: i32) -> i32 }\nfunc abs() {}\nfunc main() {}",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == DiagnosticCode::DuplicateDeclaration)
    );
}

#[test]
fn ptr_is_a_shadowable_prelude_binding() {
    let resolution = valid("func main() { let ptr = 1\nprint(ptr) }");
    let id = resolution
        .references
        .values()
        .find(|id| resolution.symbols[id.0].name == "ptr")
        .copied()
        .expect("reference to the shadowing binding");
    assert!(matches!(
        resolution.symbols[id.0].kind,
        SymbolKind::Variable(_)
    ));
}

/// A loader for the module rules below; the graph itself is tested in
/// `module::tests`.
struct Fake(&'static [(&'static str, &'static str, &'static str)]);

impl crate::module::ModuleLoader for Fake {
    fn load(&mut self, path: &str) -> Result<Vec<(String, String)>, String> {
        let files: Vec<_> = self
            .0
            .iter()
            .filter(|(module, ..)| *module == path)
            .map(|(_, name, source)| ((*name).to_owned(), (*source).to_owned()))
            .collect();
        if files.is_empty() {
            return Err("no such module".into());
        }
        Ok(files)
    }
}

fn program(
    entry: &str,
    modules: &'static [(&'static str, &'static str, &'static str)],
) -> ResolveOutput {
    let loaded =
        crate::module::load("main.skuld", entry, &mut Fake(modules)).expect("a program that loads");
    resolve(&loaded)
}

#[test]
fn a_qualified_name_resolves_to_the_exported_declaration() {
    let result = program(
        "import \"lib\"\nfunc main() { print(lib.exported()) }",
        &[(
            "lib",
            "lib/l.skuld",
            "pub func exported() -> int { return 1 }",
        )],
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let resolution = result.resolution.expect("resolution");
    // The right half of `lib.exported` is a value name, resolved in the
    // module's own scope rather than an enclosing one.
    let symbol = resolution
        .references
        .iter()
        .find(|((file, _), _)| *file == FileId(0))
        .map(|(_, id)| &resolution.symbols[id.0]);
    assert!(symbol.is_some());
    assert!(
        resolution
            .symbols
            .iter()
            .any(|s| s.name == "exported" && s.visibility == Visibility::Public)
    );
}

#[test]
fn a_private_name_does_not_leave_its_module() {
    let result = program(
        "import \"lib\"\nfunc main() { print(lib.hidden()) }",
        &[("lib", "lib/l.skuld", "func hidden() -> int { return 1 }")],
    );
    assert_eq!(
        result.diagnostics[0].diagnostic.code,
        DiagnosticCode::PrivateName
    );
}

#[test]
fn imports_belong_to_a_file_rather_than_to_its_module() {
    // `b.skuld` shares a namespace with `a.skuld`, but not its imports: a
    // qualifier is bound where it is written.
    let result = program(
        "import \"pair\"\nfunc main() { print(pair.a()) }",
        &[
            (
                "pair",
                "pair/a.skuld",
                "import \"lib\"\npub func a() -> int { return lib.value() }",
            ),
            (
                "pair",
                "pair/b.skuld",
                "pub func b() -> int { return lib.value() }",
            ),
            ("lib", "lib/l.skuld", "pub func value() -> int { return 1 }"),
        ],
    );
    let codes: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect();
    assert_eq!(codes, vec![DiagnosticCode::UnknownName]);
}

#[test]
fn a_module_qualifier_is_shadowed_by_a_local_binding() {
    let result = program(
        "import \"lib\"\nfunc main() { let lib = 1\n print(lib) }",
        &[("lib", "lib/l.skuld", "pub func value() -> int { return 1 }")],
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn a_misspelt_name_suggests_the_one_in_scope_and_carries_the_edit() {
    let source = "func main() {\n    let total = 1\n    print(totla)\n}";
    let result = output(source);
    let diagnostic = &result.diagnostics[0].diagnostic;
    assert_eq!(diagnostic.code, DiagnosticCode::UnknownName);
    assert_eq!(diagnostic.help.as_deref(), Some("did you mean `total`?"));
    let fix = diagnostic.fix.as_ref().expect("the whole edit is known");
    let mut fixed = source.to_string();
    fixed.replace_range(fix.span.start..fix.span.end, &fix.replacement);
    assert_eq!(
        fixed,
        "func main() {\n    let total = 1\n    print(total)\n}"
    );
    assert!(output(&fixed).diagnostics.is_empty());
}

#[test]
fn a_name_nothing_in_scope_resembles_is_not_guessed_at() {
    // Two mistakes is the budget, and `elephant` is further than that from
    // every name here, prelude included.
    let result = output("func main() {\n    let total = 1\n    print(elephant)\n}");
    let diagnostic = &result.diagnostics[0].diagnostic;
    assert_eq!(diagnostic.code, DiagnosticCode::UnknownName);
    assert!(diagnostic.fix.is_none());
    assert!(
        diagnostic
            .help
            .as_deref()
            .is_some_and(|help| help.contains("check the spelling"))
    );
}

#[test]
fn a_misspelt_export_suggests_a_name_the_module_actually_exports() {
    let result = program(
        "import \"lib\"\nfunc main() { print(lib.exportd()) }",
        &[(
            "lib",
            "lib/l.skuld",
            "pub func exported() -> int { return 1 }\nfunc hidden() -> int { return 2 }",
        )],
    );
    let diagnostic = &result.diagnostics[0].diagnostic;
    assert_eq!(diagnostic.code, DiagnosticCode::UnknownName);
    assert_eq!(
        diagnostic.help.as_deref(),
        Some("did you mean `lib.exported`?")
    );
    let fix = diagnostic.fix.as_ref().expect("the whole edit is known");
    assert_eq!(fix.replacement, "exported");
}

#[test]
fn a_private_name_is_not_offered_as_a_suggestion() {
    // `hiddn` is one mistake from `hidden`, which the module does not export:
    // suggesting it would send the reader to a name they cannot write.
    let result = program(
        "import \"lib\"\nfunc main() { print(lib.hiddn()) }",
        &[("lib", "lib/l.skuld", "func hidden() -> int { return 1 }")],
    );
    let diagnostic = &result.diagnostics[0].diagnostic;
    assert_eq!(diagnostic.code, DiagnosticCode::UnknownName);
    assert!(diagnostic.fix.is_none());
}
