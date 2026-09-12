use super::*;

fn program(source: &str) -> Program {
    let output = parse(source);
    assert!(output.diagnostics.is_empty(), "{:#?}", output.diagnostics);
    output.program.expect("valid AST")
}
fn expr(source: &str) -> Expr {
    let mut program = program(&format!("func main() {{ {source} }}"));
    let statement = program.functions.remove(0).body.statements.remove(0);
    let StatementKind::Expression(expr) = statement.kind else {
        panic!("expression statement")
    };
    expr
}
fn shape(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Identifier(name) => name.text.clone(),
        ExprKind::Literal(Literal::Integer(value)) => value.to_string(),
        ExprKind::Binary {
            left, op, right, ..
        } => format!("({} {op:?} {})", shape(left), shape(right)),
        ExprKind::Assignment {
            target, op, value, ..
        } => format!("({} {op:?} {})", shape(target), shape(value)),
        ExprKind::Unary { op, operand, .. } => format!("({op:?} {})", shape(operand)),
        ExprKind::Group(value) => format!("(group {})", shape(value)),
        ExprKind::Call { callee, arguments } => format!(
            "{}({})",
            shape(callee),
            arguments.iter().map(shape).collect::<Vec<_>>().join(",")
        ),
        ExprKind::Member { object, member } => format!("{}.{}", shape(object), member.text),
        _ => panic!("unexpected shape: {expr:?}"),
    }
}

#[test]
fn precedence_and_associativity() {
    for (source, expected) in [
        ("1 + 2 * 3", "(1 Add (2 Multiply 3))"),
        ("1 - 2 - 3", "((1 Subtract 2) Subtract 3)"),
        ("a = b = 3", "(a Assign (b Assign 3))"),
        ("a += b *= 3", "(a Add (b Multiply 3))"),
        (
            "a = b || c && d == e < f + g * -h",
            "(a Assign (b Or (c And (d Equal (e Less (f Add (g Multiply (Negative h))))))))",
        ),
        ("(1 + 2) * 3", "((group (1 Add 2)) Multiply 3)"),
        ("!user.greet(1, 2).ready", "(Not user.greet(1,2).ready)"),
        ("+a / b % c", "(((Positive a) Divide b) Modulo c)"),
        ("a != b >= c", "(a NotEqual (b GreaterEqual c))"),
        ("a <= b", "(a LessEqual b)"),
        ("a > b", "(a Greater b)"),
        ("a -= b /= 2", "(a Subtract (b Divide 2))"),
    ] {
        assert_eq!(shape(&expr(source)), expected, "{source}");
    }
}

#[test]
fn parses_existing_examples_and_signatures() {
    let hello = program(include_str!("../../../examples/hello.skuld"));
    assert_eq!(hello.functions[0].name.text, "main");
    assert!(hello.functions[0].return_type.is_none());
    let functions = program(include_str!("../../../examples/functions.skuld"));
    assert_eq!(functions.functions.len(), 2);
    let add = &functions.functions[0];
    assert_eq!(add.parameters.len(), 2);
    let Some(TypeRef::Named(name)) = &add.return_type else {
        panic!("return type")
    };
    assert_eq!(name.text, "int");
    assert!(matches!(
        add.body.statements[0].kind,
        StatementKind::Return(Some(_))
    ));
    program("func f(a: int, b: bool,) -> void {} func main() { f(1, true,) }");
}

#[test]
fn variables_preserve_mutability_and_source_types_without_type_checking() {
    let p = program("func main() {\nlet age: int = \"hello\"\nvar x = 1\nage = 28\nunknown(x)\n}");
    let statements = &p.functions[0].body.statements;
    let StatementKind::Variable(age) = &statements[0].kind else {
        panic!("variable")
    };
    assert_eq!(age.mutability, Mutability::Immutable);
    assert!(age.type_ref.is_some());
    let StatementKind::Variable(x) = &statements[1].kind else {
        panic!("variable")
    };
    assert_eq!(x.mutability, Mutability::Mutable);
    assert!(x.type_ref.is_none());
    program("func f() -> Unknown { return \"wrong\" }");
}

#[test]
fn blocks_and_else_if() {
    let p = program(
        "func main() {\nvar age = 17\nage += 10\nif age >= 18 { print(\"Adult\") } else if age == 0 { return } else { print(\"Minor\") }\n{ let age = 30 }\n}",
    );
    let statements = &p.functions[0].body.statements;
    let StatementKind::If {
        else_branch: Some(branch),
        ..
    } = &statements[2].kind
    else {
        panic!("if")
    };
    assert!(matches!(branch.kind, StatementKind::If { .. }));
    assert!(matches!(statements[3].kind, StatementKind::Block(_)));
}

#[test]
fn multiline_expressions_comments_and_return_boundaries() {
    let p = program(
        "func main() {\r\nlet result =\r\n foo(\n1,\n2\n) // keep adding\n + bar()\nprint(result)\nreturn // bare\nfoo()\n}",
    );
    let statements = &p.functions[0].body.statements;
    assert_eq!(statements.len(), 4);
    let StatementKind::Variable(variable) = &statements[0].kind else {
        panic!("variable")
    };
    assert_eq!(shape(&variable.initializer), "(foo(1,2) Add bar())");
    assert!(matches!(statements[2].kind, StatementKind::Return(None)));
    assert_eq!(shape(&expr("user\n .greet\n (1)")), "user.greet(1)");
    program("func main() { return (\n1 +\n2\n) }");
    assert_eq!(shape(&expr("foo\n(bar)")), "foo(bar)");
    assert_eq!(shape(&expr("a\n-b")), "(a Subtract b)");
}

#[test]
fn spans_cover_names_types_operators_and_groups() {
    let source = "func add(a: int) -> int { return (a + 2) }";
    let p = program(source);
    assert_eq!(p.span, Span::new(0, source.len()));
    let f = &p.functions[0];
    assert_eq!(&source[f.name.span.start..f.name.span.end], "add");
    assert_eq!(
        &source[f.parameters[0].span.start..f.parameters[0].span.end],
        "a: int"
    );
    let StatementKind::Return(Some(group)) = &f.body.statements[0].kind else {
        panic!("return")
    };
    assert_eq!(&source[group.span.start..group.span.end], "(a + 2)");
    let ExprKind::Group(inner) = &group.kind else {
        panic!("group")
    };
    let ExprKind::Binary { op_span, .. } = inner.kind else {
        panic!("binary")
    };
    assert_eq!(&source[op_span.start..op_span.end], "+");
    let p = program("func main() { let text = \"é\" }");
    let StatementKind::Variable(v) = &p.functions[0].body.statements[0].kind else {
        panic!("variable")
    };
    assert_eq!(v.initializer.span.end - v.initializer.span.start, 4);
}

#[test]
fn literals_are_ast_owned_variants() {
    for source in ["42", "3.5", "true", "false", "'é'", "\"hello\""] {
        assert!(matches!(expr(source).kind, ExprKind::Literal(_)));
    }
}

#[test]
fn invalid_sources_produce_specific_diagnostics() {
    for (source, code, text) in [
        (
            "let x = 1",
            DiagnosticCode::ExpectedDeclaration,
            "function declaration",
        ),
        (
            "func f(a) {}",
            DiagnosticCode::ExpectedSyntax,
            "explicit parameter type",
        ),
        (
            "func f(a: ) {}",
            DiagnosticCode::ExpectedSyntax,
            "type name",
        ),
        (
            "func f( {}",
            DiagnosticCode::ExpectedSyntax,
            "parameter name",
        ),
        (
            "func f() { let x }",
            DiagnosticCode::ExpectedSyntax,
            "initializer",
        ),
        (
            "func f() { foo(1 2) }",
            DiagnosticCode::ExpectedSyntax,
            "call arguments",
        ),
        (
            "func f() { user. }",
            DiagnosticCode::ExpectedSyntax,
            "member name",
        ),
        (
            "func f() { (1 + 2 }",
            DiagnosticCode::ExpectedSyntax,
            "grouped expression",
        ),
        (
            "func f() { 1 = 2 }",
            DiagnosticCode::InvalidAssignmentTarget,
            "assignment target",
        ),
        (
            "func f() { foo() = 2 }",
            DiagnosticCode::InvalidAssignmentTarget,
            "assignment target",
        ),
        (
            "func f() { let x = 1 let y = 2 }",
            DiagnosticCode::ExpectedSyntax,
            "newline",
        ),
        (
            "func f() { let x = 1e3 }",
            DiagnosticCode::ExpectedSyntax,
            "newline",
        ),
        (
            "func f() { if true {} else 1 }",
            DiagnosticCode::ExpectedSyntax,
            "begin a block",
        ),
        (
            "func f() { for i in xs {} }",
            DiagnosticCode::UnsupportedSyntax,
            "later milestone",
        ),
        (
            "func f() {",
            DiagnosticCode::ExpectedSyntax,
            "close the block",
        ),
        (
            "func f() { @ }",
            DiagnosticCode::InvalidCharacter,
            "invalid character",
        ),
    ] {
        let output = parse(source);
        assert!(output.program.is_none(), "{source}");
        assert_eq!(
            output.diagnostics[0].code, code,
            "{source}: {:?}",
            output.diagnostics
        );
        assert!(
            output.diagnostics[0].message.contains(text),
            "{source}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn recovery_reports_independent_errors() {
    let output = parse("func bad() {\nlet = 1\nlet x =\n}\nfunc other(a) {}\nfunc valid() {}\n");
    assert!(output.program.is_none());
    assert_eq!(output.diagnostics.len(), 3, "{:?}", output.diagnostics);
    let output = parse("func first() {\nreturn 1\nfunc second(a) {}\n");
    assert_eq!(output.diagnostics.len(), 2, "{:?}", output.diagnostics);
    assert!(output.diagnostics[0].message.contains("next function"));
}

#[test]
fn empty_source_is_syntactically_valid_without_entrypoint_checking() {
    assert!(program("").functions.is_empty());
}

#[test]
fn deeply_nested_and_long_expressions_are_diagnosed() {
    for body in [
        format!("{}1{}", "(".repeat(200), ")".repeat(200)),
        format!("{}true", "!".repeat(200)),
        format!("{}1", "1+".repeat(400)),
        format!("{}{}", "{".repeat(100), "}".repeat(100)),
        format!("{}{{}}", "if true {} else ".repeat(100)),
    ] {
        let output = parse(&format!("func main() {{ {body} }}"));
        assert!(output.program.is_none());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::SyntaxLimit),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn malformed_token_combinations_terminate_and_keep_valid_spans() {
    let fragments = [
        "function", "x", "(", ")", "{", "}", "let", "=", "1", "return", "if", "else", ",", ".",
        "+", "\n",
    ];
    for a in fragments {
        for b in fragments {
            for c in fragments {
                let source = format!("func main() {{ {a} {b} {c} }}");
                let output = parse(&source);
                for diagnostic in output.diagnostics {
                    assert!(diagnostic.span.start <= diagnostic.span.end);
                    assert!(diagnostic.span.end <= source.len());
                }
            }
        }
    }
}

#[test]
fn while_statement_takes_a_condition_and_body() {
    let mut program = program("func main() {\n    while a < 2 {\n        b = 1\n    }\n}");
    let statement = program.functions.remove(0).body.statements.remove(0);
    let StatementKind::While { condition, body } = statement.kind else {
        panic!("while statement")
    };
    // The condition is a plain expression: no parentheses are required and none
    // are consumed as a call.
    assert!(matches!(condition.kind, ExprKind::Binary { .. }));
    assert_eq!(body.statements.len(), 1);
}

#[test]
fn while_recovers_without_swallowing_the_next_statement() {
    let output = parse("func main() {\n    while {\n    }\n    let x = 1\n}");
    assert!(!output.diagnostics.is_empty());
    assert!(output.program.is_none());
}

#[test]
fn loop_break_and_continue_parse_as_statements() {
    let mut program =
        program("func main() {\n    loop {\n        break\n        continue\n    }\n}");
    let statement = program.functions.remove(0).body.statements.remove(0);
    let StatementKind::Loop { body } = statement.kind else {
        panic!("loop statement")
    };
    assert!(matches!(
        body.statements.as_slice(),
        [
            Statement {
                kind: StatementKind::Break,
                ..
            },
            Statement {
                kind: StatementKind::Continue,
                ..
            }
        ]
    ));
}

#[test]
fn struct_declaration_takes_one_field_per_line() {
    let program = program("struct Vec2 {\n    x: float\n    y: float\n}\nfunc main() {}");
    assert_eq!(program.structs.len(), 1);
    let declaration = &program.structs[0];
    assert_eq!(declaration.name.text, "Vec2");
    let names: Vec<_> = declaration
        .fields
        .iter()
        .map(|field| field.name.text.as_str())
        .collect();
    assert_eq!(names, ["x", "y"]);
}

#[test]
fn struct_fields_on_one_line_need_a_separator() {
    let output = parse("struct V {\n    x: int y: int\n}\nfunc main() {}");
    assert!(!output.diagnostics.is_empty());
    assert_eq!(output.diagnostics[0].code, DiagnosticCode::ExpectedSyntax);
}

#[test]
fn record_construction_parses_as_an_expression() {
    let expression = expr("Vec2 { x: 1.0, y: 2.0 }");
    let ExprKind::StructLiteral { name, fields } = expression.kind else {
        panic!("struct literal")
    };
    assert_eq!(name.text, "Vec2");
    assert_eq!(fields.len(), 2);
}

#[test]
fn conditions_do_not_read_a_block_as_record_construction() {
    // `if value { }` must stay an if with a block; a literal there would
    // swallow the body. Parentheses make construction available again.
    let mut parsed = program("func main() {\n    if value {\n        print(1)\n    }\n}");
    let statement = parsed.functions.remove(0).body.statements.remove(0);
    let StatementKind::If { condition, .. } = statement.kind else {
        panic!("if statement")
    };
    assert!(matches!(condition.kind, ExprKind::Identifier(_)));

    let mut parsed = program("func main() {\n    while ok {\n        print(1)\n    }\n}");
    let statement = parsed.functions.remove(0).body.statements.remove(0);
    let StatementKind::While { condition, .. } = statement.kind else {
        panic!("while statement")
    };
    assert!(matches!(condition.kind, ExprKind::Identifier(_)));

    let expression = expr("(Point { x: 1 })");
    let ExprKind::Group(inner) = expression.kind else {
        panic!("group")
    };
    assert!(matches!(inner.kind, ExprKind::StructLiteral { .. }));
}

#[test]
fn struct_bodies_mix_fields_and_methods() {
    let program = program(
        "struct R {\n    w: int\n    area() -> int {\n        return this.w\n    }\n    h: int\n}\nfunc main() {}",
    );
    let declaration = &program.structs[0];
    let fields: Vec<_> = declaration
        .fields
        .iter()
        .map(|field| field.name.text.as_str())
        .collect();
    assert_eq!(fields, ["w", "h"]);
    assert_eq!(declaration.methods.len(), 1);
    // Methods carry no `func` keyword and no receiver parameter.
    assert_eq!(declaration.methods[0].name.text, "area");
    assert!(declaration.methods[0].parameters.is_empty());
}
