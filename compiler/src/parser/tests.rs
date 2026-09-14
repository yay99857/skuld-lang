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
        ("low | high << 8", "(low BitOr (high ShiftLeft 8))"),
        ("flags & mask >> 16", "(flags BitAnd (mask ShiftRight 16))"),
        (
            "a | b ^ c & d << e + f * -g",
            "(a BitOr (b BitXor (c BitAnd (d ShiftLeft (e Add (f Multiply (Negative g)))))))",
        ),
        ("a & b == 0", "((a BitAnd b) Equal 0)"),
        ("~a & b", "((BitNot a) BitAnd b)"),
        (
            "a &= b |= c ^= d <<= e >>= 1",
            "(a BitAnd (b BitOr (c BitXor (d ShiftLeft (e ShiftRight 1)))))",
        ),
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
    assert_eq!(name.name.text, "int");
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
            "func f() { import math }",
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
    assert_eq!(name.name.text, "Vec2");
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

#[test]
fn class_bodies_and_new_expressions() {
    let program = program(
        "class User {\n    name: string\n    hello() {\n        print(this.name)\n    }\n}\nfunc main() {\n    let u = new User(name: \"Ada\")\n}",
    );
    assert_eq!(program.structs.len(), 1);
    let class_decl = &program.structs[0];
    assert_eq!(class_decl.kind, TypeDeclKind::Reference);
    assert_eq!(class_decl.name.text, "User");
    assert_eq!(class_decl.fields.len(), 1);
    assert_eq!(class_decl.fields[0].name.text, "name");
    assert_eq!(class_decl.methods.len(), 1);
    assert_eq!(class_decl.methods[0].name.text, "hello");

    let statement = &program.functions[0].body.statements[0];
    let StatementKind::Variable(var) = &statement.kind else {
        panic!("variable statement");
    };
    let ExprKind::New { name, fields } = &var.initializer.kind else {
        panic!("new expression");
    };
    assert_eq!(name.name.text, "User");
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name.text, "name");
}

#[test]
fn arrays_weak_types_spans_and_multiline_indexing() {
    let source = "class User {}\nfunc main() {\nlet refs: []weak User = [weak(new User())]\nlet value = [1, 2,]\n[0] + 3\n}";
    let parsed = program(source);
    let StatementKind::Variable(refs) = &parsed.functions[0].body.statements[0].kind else {
        panic!("variable")
    };
    let reference = refs.type_ref.as_ref().expect("annotation");
    assert_eq!(
        &source[reference.span().start..reference.span().end],
        "[]weak User"
    );
    let StatementKind::Variable(value) = &parsed.functions[0].body.statements[1].kind else {
        panic!("variable")
    };
    let ExprKind::Binary {
        left,
        op: BinaryOp::Add,
        ..
    } = &value.initializer.kind
    else {
        panic!("addition")
    };
    assert!(matches!(left.kind, ExprKind::Index { .. }));
    assert_eq!(&source[left.span.start..left.span.end], "[1, 2,]\n[0]");
}

#[test]
fn nested_array_types_are_bounded_and_malformed_arrays_recover() {
    let source = format!("func main() {{ let x: {}int = [] }}", "[]".repeat(1000));
    let output = parse(&source);
    assert!(output.program.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::SyntaxLimit)
    );
    for source in [
        "func main() { let x = [1,,2] }",
        "func main() { let x = [1,2 }",
        "func main() { let x: [int = [] }",
        "func main() { let x = weak(1, 2) }",
    ] {
        let output = parse(source);
        assert!(output.program.is_none(), "{source}");
        assert!(!output.diagnostics.is_empty());
        assert!(
            output
                .diagnostics
                .iter()
                .all(|d| d.span.start <= d.span.end && d.span.end <= source.len())
        );
    }
}

#[test]
fn option_type_spans_and_if_let_else_if() {
    let source = "func main() { let x: Option<Option<int>>=Some(None)\nif let Some(value) = x {} else if let Some(other) = x {} }";
    let ast = program(source);
    let StatementKind::Variable(variable) = &ast.functions[0].body.statements[0].kind else {
        panic!("variable")
    };
    let ty = variable.type_ref.as_ref().expect("type");
    assert_eq!(
        &source[ty.span().start..ty.span().end],
        "Option<Option<int>>"
    );
    let StatementKind::IfLet {
        binding,
        else_branch,
        ..
    } = &ast.functions[0].body.statements[1].kind
    else {
        panic!("if let")
    };
    assert_eq!(&source[binding.span.start..binding.span.end], "value");
    assert!(matches!(
        else_branch.as_ref().expect("else").kind,
        StatementKind::IfLet { .. }
    ));
}

#[test]
fn option_syntax_errors_and_nesting_are_diagnosed() {
    for source in [
        "func main() { let x: Option<> = None }",
        "func main() { let x: Option<int = None }",
        "func main() { if let None = None {} }",
        "func main() { if let Some() = None {} }",
        "func main() { if let Some(x) Some(1) {} }",
        "func main() { let x: Option<int, bool> = None }",
    ] {
        let result = parse(source);
        assert!(result.program.is_none(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.span.start <= d.span.end && d.span.end <= source.len())
        );
    }
    let source = format!(
        "func main() {{ let x: {}int{} = None }}",
        "Option<".repeat(1000),
        ">".repeat(1000)
    );
    let result = parse(&source);
    assert!(result.program.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::SyntaxLimit)
    );
}

#[test]
fn result_types_patterns_and_try_parse() {
    // `Result<T, E>` closes like `Option<T>`, including the `>=` that the lexer
    // hands over as one token in an annotation followed by an initializer.
    let source = "func read(): Result<int, string> { return Ok(1) }\nfunc main() {\n    let a: Result<int, string>= read()\n    if let Ok(value) = a { print(value) }\n    if let Err(reason) = a { print(reason) }\n}";
    let result = parse(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let program = result.program.expect("parsed");
    let TypeRef::Result { ok, err, .. } = program.functions[0]
        .return_type
        .as_ref()
        .expect("return type")
    else {
        panic!("Result return type")
    };
    assert!(matches!(**ok, TypeRef::Named(ref name) if name.name.text == "int"));
    assert!(matches!(**err, TypeRef::Named(ref name) if name.name.text == "string"));
    let patterns: Vec<IfLetPattern> = program.functions[1]
        .body
        .statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            StatementKind::IfLet { pattern, .. } => Some(*pattern),
            _ => None,
        })
        .collect();
    assert_eq!(patterns, vec![IfLetPattern::Ok, IfLetPattern::Err]);

    // `?` is postfix and binds tighter than any operator, so it applies to the
    // call it follows rather than to the surrounding expression.
    let expr = expr("read()? + read()?");
    let ExprKind::Binary { left, right, .. } = expr.kind else {
        panic!("binary")
    };
    assert!(matches!(left.kind, ExprKind::Try(_)));
    assert!(matches!(right.kind, ExprKind::Try(_)));

    for source in [
        "func main() { let x: Result<int> = None }",
        "func main() { let x: Result<int, = None }",
        "func main() { let x: Result<, string> = None }",
        "func main() { if let Ok() = None {} }",
    ] {
        let result = parse(source);
        assert!(result.program.is_none(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.span.start <= d.span.end && d.span.end <= source.len())
        );
    }
}

#[test]
fn slices_and_indexes_are_distinguished() {
    // The same brackets index or slice depending on whether `..` appears.
    let program = program("func main() { let a = xs[1]\nlet b = xs[1..2] }");
    let statements = &program.functions[0].body.statements;
    let StatementKind::Variable(first) = &statements[0].kind else {
        panic!("variable")
    };
    assert!(matches!(first.initializer.kind, ExprKind::Index { .. }));
    let StatementKind::Variable(second) = &statements[1].kind else {
        panic!("variable")
    };
    let ExprKind::Slice { object, start, end } = &second.initializer.kind else {
        panic!("slice")
    };
    assert!(matches!(object.kind, ExprKind::Identifier(_)));
    assert!(matches!(start.kind, ExprKind::Literal(Literal::Integer(1))));
    assert!(matches!(end.kind, ExprKind::Literal(Literal::Integer(2))));

    // A slice is a postfix step like any other, so it chains and nests.
    let expr = expr("a[i..j][0].field");
    let ExprKind::Member { object, .. } = &expr.kind else {
        panic!("member")
    };
    let ExprKind::Index { object, .. } = &object.kind else {
        panic!("index")
    };
    assert!(matches!(object.kind, ExprKind::Slice { .. }));

    for source in [
        "func main() { let a = xs[1..] }",
        "func main() { let a = xs[..2] }",
        "func main() { let a = xs[1..2 }",
    ] {
        let result = parse(source);
        assert!(result.program.is_none(), "{source}");
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.span.start <= d.span.end && d.span.end <= source.len())
        );
    }
}

#[test]
fn colon_return_and_direct_if_let_parse() {
    let source = "struct Calc { compute(x: int): int { return x } }\nfunc add(a: int, b: int): int { return a + b }\nfunc main() { if let ans = None {} }";
    let result = parse(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let program = result.program.expect("parsed");
    assert_eq!(program.functions.len(), 2);
    assert_eq!(program.structs.len(), 1);
}

#[test]
fn enum_declarations_and_match_statements_parse() {
    let source = "enum Status {\n    Pending,\n    Active(int),\n    Cancelled\n}\nfunc main() {\n    match s {\n        Status.Pending: return\n        Status.Active(code): { print(code) }\n        _: {}\n    }\n}";
    let result = parse(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let program = result.program.expect("parsed");
    assert_eq!(program.enums.len(), 1);
    assert_eq!(program.enums[0].name.text, "Status");
    assert_eq!(program.enums[0].variants.len(), 3);
    assert_eq!(program.enums[0].variants[1].name.text, "Active");
    assert!(program.enums[0].variants[1].payload.is_some());
}

#[test]
fn for_loop_statements_parse() {
    let source = "func main() {\n    for i in 0..10 {\n        print(i)\n    }\n    for item in items {\n        print(item)\n    }\n}";
    let result = parse(source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let program = result.program.expect("parsed");
    assert_eq!(program.functions.len(), 1);
    let body = &program.functions[0].body;
    assert_eq!(body.statements.len(), 2);
    assert!(matches!(
        body.statements[0].kind,
        StatementKind::For {
            iterable: ForIterable::Range { .. },
            ..
        }
    ));
    assert!(matches!(
        body.statements[1].kind,
        StatementKind::For {
            iterable: ForIterable::Expr(_),
            ..
        }
    ));
}

#[test]
fn extern_blocks_declare_foreign_signatures() {
    let program = program(
        "unsafe extern \"C\" {\n    func write(fd: i32, buffer: *u8, count: u64) -> i64\n    func flush()\n}\nfunc main() {}",
    );
    assert_eq!(program.externs.len(), 1);
    let block = &program.externs[0];
    assert_eq!(block.abi, "C");
    assert_eq!(block.functions.len(), 2);
    assert_eq!(block.functions[0].name.text, "write");
    assert!(matches!(
        block.functions[0].parameters[1].type_ref,
        TypeRef::Pointer { .. }
    ));
    // An omitted return type is void, exactly as for an ordinary function.
    assert!(block.functions[1].return_type.is_none());
    // The block spans from `unsafe` to its closing brace.
    assert_eq!(block.span.start, 0);
}

#[test]
fn extern_blocks_reject_bodies_other_abis_and_a_missing_marker() {
    for source in [
        "unsafe extern \"C\" { func abs(value: i32) -> i32 { return value } }\nfunc main() {}",
        "unsafe extern \"Rust\" { func abs(value: i32) -> i32 }\nfunc main() {}",
        "extern \"C\" { func abs(value: i32) -> i32 }\nfunc main() {}",
        "unsafe func main() {}",
    ] {
        let output = parse(source);
        assert!(output.program.is_none(), "{source}");
        assert!(
            output.diagnostics.iter().any(|d| matches!(
                d.code,
                DiagnosticCode::UnsupportedSyntax | DiagnosticCode::ExpectedSyntax
            )),
            "{source}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn pointer_types_nest_and_keep_spans() {
    let program = program("unsafe extern \"C\" { func f(a: *void, b: *u8) }\nfunc main() {}");
    let parameters = &program.externs[0].functions[0].parameters;
    for parameter in parameters {
        let TypeRef::Pointer { pointee, span } = &parameter.type_ref else {
            panic!("pointer type")
        };
        assert!(matches!(**pointee, TypeRef::Named(_)));
        assert_eq!(*span, parameter.type_ref.span());
    }
}

#[test]
fn imports_are_collected_before_declarations() {
    let program = program("import \"json\"\nimport \"net/socket\"\nfunc main() { }");
    let paths: Vec<_> = program
        .imports
        .iter()
        .map(|import| (import.path.as_str(), import.qualifier.text.as_str()))
        .collect();
    // The qualifier is the last segment, so two different paths can still
    // collide on the name they bind.
    assert_eq!(paths, vec![("json", "json"), ("net/socket", "socket")]);
}

#[test]
fn an_import_after_a_declaration_is_rejected() {
    let output = parse("func main() { }\nimport \"json\"");
    assert!(output.program.is_none());
    assert_eq!(
        output.diagnostics[0].code,
        DiagnosticCode::MisplacedImport,
        "{:#?}",
        output.diagnostics
    );
}

#[test]
fn module_paths_reject_traversal_and_empty_segments() {
    for source in [
        "import \"../json\"",
        "import \"/json\"",
        "import \"json/\"",
        "import \"\"",
        "import \"2json\"",
    ] {
        let output = parse(source);
        assert_eq!(
            output.diagnostics[0].code,
            DiagnosticCode::InvalidModulePath,
            "{source}"
        );
    }
}

#[test]
fn pub_marks_functions_types_and_enums() {
    let program = program(
        "pub func exported() { }\nfunc hidden() { }\npub class Open { x: int }\nstruct Closed { x: int }\npub enum Tag { A }",
    );
    assert_eq!(program.functions[0].visibility, Visibility::Public);
    assert_eq!(program.functions[1].visibility, Visibility::Private);
    assert_eq!(program.structs[0].visibility, Visibility::Public);
    assert_eq!(program.structs[1].visibility, Visibility::Private);
    assert_eq!(program.enums[0].visibility, Visibility::Public);
}

#[test]
fn a_method_is_public_with_the_type_that_owns_it() {
    let program = program("struct Point { x: int\n  show() -> int { return this.x } }");
    assert_eq!(program.structs[0].visibility, Visibility::Private);
    assert_eq!(program.structs[0].methods[0].visibility, Visibility::Public);
}

#[test]
fn type_positions_accept_a_module_qualifier() {
    let program = program(
        "func f(a: json.Value, b: []json.Value, c: Option<json.Value>, d: weak json.Node) -> json.Value { return a }",
    );
    let parameters = &program.functions[0].parameters;
    let TypeRef::Named(path) = &parameters[0].type_ref else {
        panic!("named type")
    };
    assert_eq!(path.module.as_ref().map(|m| m.text.as_str()), Some("json"));
    assert_eq!(path.name.text, "Value");
    let TypeRef::Array { element, .. } = &parameters[1].type_ref else {
        panic!("array type")
    };
    assert!(matches!(&**element, TypeRef::Named(path) if path.module.is_some()));
    let TypeRef::Option { element, .. } = &parameters[2].type_ref else {
        panic!("option type")
    };
    assert!(matches!(&**element, TypeRef::Named(path) if path.module.is_some()));
    assert!(
        matches!(&parameters[3].type_ref, TypeRef::Weak { class, .. } if class.module.is_some())
    );
}

#[test]
fn construction_and_patterns_accept_a_module_qualifier() {
    let program = program(
        "func main() { let a = new json.Node(value: 1)\n let b = json.Point { x: 1 }\n match a { json.Tag.One(v): print(v)\n Tag.Two: print(2)\n _: print(3) } }",
    );
    let statements = &program.functions[0].body.statements;
    let StatementKind::Variable(first) = &statements[0].kind else {
        panic!("variable")
    };
    assert!(matches!(&first.initializer.kind, ExprKind::New { name, .. } if name.module.is_some()));
    let StatementKind::Variable(second) = &statements[1].kind else {
        panic!("variable")
    };
    assert!(
        matches!(&second.initializer.kind, ExprKind::StructLiteral { name, .. } if name.module.is_some())
    );
    let StatementKind::Match { arms, .. } = &statements[2].kind else {
        panic!("match")
    };
    // Three names are `module.Enum.Variant`; two are `Enum.Variant`.
    let MatchPattern::Variant {
        enum_name,
        variant_name,
        ..
    } = &arms[0].pattern
    else {
        panic!("variant pattern")
    };
    let path = enum_name.as_ref().expect("qualified enum");
    assert_eq!(path.module.as_ref().map(|m| m.text.as_str()), Some("json"));
    assert_eq!(path.name.text, "Tag");
    assert_eq!(variant_name.text, "One");
    let MatchPattern::Variant { enum_name, .. } = &arms[1].pattern else {
        panic!("variant pattern")
    };
    assert!(enum_name.as_ref().expect("enum name").module.is_none());
}

#[test]
fn a_member_access_is_not_a_qualified_struct_literal() {
    // `value.field` followed by a block must stay a member access; only a
    // `name.Name {` sequence reads as construction.
    let program = program("func main() { let x = point.field\n if flag { print(1) } }");
    let StatementKind::Variable(declaration) = &program.functions[0].body.statements[0].kind else {
        panic!("variable")
    };
    assert!(matches!(
        &declaration.initializer.kind,
        ExprKind::Member { .. }
    ));
}

#[test]
fn a_declaration_may_carry_an_escape_block() {
    let program = program(
        "func main() { let a = f() else reason { return }\n let b = g() else { return }\n let c = h() }",
    );
    let statements = &program.functions[0].body.statements;
    let named = match &statements[0].kind {
        StatementKind::Variable(declaration) => declaration,
        _ => panic!("variable"),
    };
    assert_eq!(
        named
            .otherwise
            .as_ref()
            .expect("an escape block")
            .binding
            .as_ref()
            .map(|name| name.text.as_str()),
        Some("reason")
    );
    let anonymous = match &statements[1].kind {
        StatementKind::Variable(declaration) => declaration,
        _ => panic!("variable"),
    };
    assert!(
        anonymous
            .otherwise
            .as_ref()
            .expect("a block")
            .binding
            .is_none()
    );
    let plain = match &statements[2].kind {
        StatementKind::Variable(declaration) => declaration,
        _ => panic!("variable"),
    };
    assert!(plain.otherwise.is_none());
}

#[test]
fn an_else_after_an_if_still_belongs_to_the_if() {
    // `else` binds to a declaration only where a declaration is being parsed.
    let program = program("func main() { if flag { print(1) } else { print(2) } }");
    let StatementKind::If { else_branch, .. } = &program.functions[0].body.statements[0].kind
    else {
        panic!("if statement")
    };
    assert!(else_branch.is_some());
}

#[test]
fn an_interface_declares_signatures_without_bodies() {
    let program = program(
        "pub interface Renderer {\n  render(value: int) -> string\n  reset()\n}\nfunc main() { }",
    );
    let interface = &program.interfaces[0];
    assert_eq!(interface.visibility, Visibility::Public);
    assert_eq!(interface.name.text, "Renderer");
    assert_eq!(interface.methods.len(), 2);
    assert_eq!(interface.methods[0].parameters[0].name.text, "value");
    assert!(interface.methods[0].return_type.is_some());
    // A signature with no result is void, as a declaration with none is.
    assert!(interface.methods[1].return_type.is_none());
}

#[test]
fn a_class_declares_the_interfaces_it_implements() {
    let program = program(
        "class User: Printable, other.Comparable { name: string }\nstruct Point { x: int }\nfunc main() { }",
    );
    let conforms = &program.structs[0].conforms;
    assert_eq!(conforms.len(), 2);
    assert_eq!(conforms[0].name.text, "Printable");
    // A conformance may name an imported interface like any other type.
    assert_eq!(
        conforms[1].module.as_ref().map(|m| m.text.as_str()),
        Some("other")
    );
    assert!(program.structs[1].conforms.is_empty());
}

#[test]
fn a_field_may_carry_a_default() {
    let parsed = program("class User {\n    name: string = \"anonymous\"\n    age: int\n}\n");
    let fields = &parsed.structs[0].fields;
    assert_eq!(fields.len(), 2);
    let default = fields[0].default.as_ref().expect("a default expression");
    assert!(matches!(
        &default.kind,
        ExprKind::Literal(Literal::String(text)) if text == "anonymous"
    ));
    // A field without one is unchanged, and the default is not confused with
    // the next field.
    assert!(fields[1].default.is_none());
    assert_eq!(fields[1].name.text, "age");
}

#[test]
fn a_missing_unsafe_carries_the_edit_that_adds_it() {
    let source = "extern \"C\" { func abs(value: i32) -> i32 }\nfunc main() {}";
    let output = parse(source);
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnsupportedSyntax)
        .expect("the missing marker is reported");
    let fix = diagnostic.fix.as_ref().expect("with an edit");
    assert_eq!(fix.title, "add `unsafe`");
    // The edit is judged by what it produces, not by its coordinates: applying
    // it has to leave source the parser accepts.
    let mut fixed = source.to_string();
    fixed.replace_range(fix.span.start..fix.span.end, &fix.replacement);
    assert_eq!(
        fixed,
        "unsafe extern \"C\" { func abs(value: i32) -> i32 }\nfunc main() {}"
    );
    assert!(parse(&fixed).diagnostics.is_empty());
}

#[test]
fn nested_generics_close_with_greater_greater() {
    let parsed = program(
        "func main() {\n    var x: Option<Result<int, string>> = None\n    var y: Option<Option<Option<int>>> = None\n    var z: Option<Option<int>>=None\n}",
    );
    assert_eq!(parsed.functions[0].body.statements.len(), 3);
}
