use super::*;
use TokenKind::*;

fn kinds(source: &str) -> Vec<TokenKind> {
    let output = lex(source);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    output.tokens.into_iter().map(|t| t.kind).collect()
}

#[test]
fn keywords_and_identifiers() {
    assert_eq!(
        kinds(
            "func let var return if else while loop new class struct impl interface enum match import pub for in static extern unsafe true false main _x x2 fnx"
        ),
        vec![
            Function,
            Let,
            Var,
            Return,
            If,
            Else,
            While,
            Loop,
            New,
            Class,
            Struct,
            Impl,
            Interface,
            Enum,
            Match,
            Import,
            Pub,
            For,
            In,
            Static,
            Extern,
            Unsafe,
            Boolean(true),
            Boolean(false),
            Identifier("main".into()),
            Identifier("_x".into()),
            Identifier("x2".into()),
            Identifier("fnx".into()),
            Eof
        ]
    );
}

#[test]
fn operators_and_delimiters() {
    assert_eq!(
        kinds("( ) { } [ ] , . : -> + - * / % = == != < > <= >= ! ? && || += -= *= /="),
        vec![
            LeftParen,
            RightParen,
            LeftBrace,
            RightBrace,
            LeftBracket,
            RightBracket,
            Comma,
            Dot,
            Colon,
            Arrow,
            Plus,
            Minus,
            Star,
            Slash,
            Percent,
            Equal,
            EqualEqual,
            BangEqual,
            Less,
            Greater,
            LessEqual,
            GreaterEqual,
            Bang,
            Question,
            AndAnd,
            OrOr,
            PlusEqual,
            MinusEqual,
            StarEqual,
            SlashEqual,
            Eof
        ]
    );
    assert_eq!(
        kinds("===!==+++="),
        vec![
            EqualEqual, Equal, BangEqual, Equal, Plus, Plus, PlusEqual, Eof
        ]
    );
    // `?` never pairs with a neighbour. `>>` is a shift token, which the type
    // parser splits when closing nested generic arguments.
    assert_eq!(
        kinds("a?? >> >>="),
        vec![
            Identifier("a".into()),
            Question,
            Question,
            GreaterGreater,
            GreaterGreaterEqual,
            Eof
        ]
    );
}

#[test]
fn numbers_and_separate_signs() {
    assert_eq!(
        kinds("0 27 001 10.5 -9223372036854775808 18446744073709551615 1.foo .5 1."),
        vec![
            Integer(0),
            Integer(27),
            Integer(1),
            Float(10.5),
            Minus,
            Integer(9223372036854775808),
            Integer(u64::MAX),
            Integer(1),
            Dot,
            Identifier("foo".into()),
            Dot,
            Integer(5),
            Integer(1),
            Dot,
            Eof
        ]
    );
}

#[test]
fn strings_chars_and_escapes() {
    assert_eq!(
        kinds(r#""Hello, 世界" "" "\n\r\t\0\\\"\'" 'é' '\n' '\'' '🦀'"#),
        vec![
            String("Hello, 世界".into()),
            String("".into()),
            String("\n\r\t\0\\\"'".into()),
            Char('é'),
            Char('\n'),
            Char('\''),
            Char('🦀'),
            Eof
        ]
    );
}

#[test]
fn comments_whitespace_and_eof() {
    assert_eq!(kinds("// hello\r\n func // end"), vec![Function, Eof]);
    assert_eq!(kinds(" \t\r\n"), vec![Eof]);
    assert_eq!(
        lex("").tokens,
        vec![Token {
            kind: Eof,
            span: Span::new(0, 0)
        }]
    );
    assert_eq!(
        kinds("\"// text\" /"),
        vec![String("// text".into()), Slash, Eof]
    );
}

#[test]
fn spans_are_utf8_byte_ranges() {
    let output = lex("\"é\"\r\nlet x");
    let spans: Vec<_> = output.tokens.iter().map(|t| t.span).collect();
    assert_eq!(
        spans,
        vec![
            Span::new(0, 4),
            Span::new(6, 9),
            Span::new(10, 11),
            Span::new(11, 11)
        ]
    );
}

#[test]
fn invalid_characters_recover() {
    let output = lex("@ é ` $ ; let");
    assert_eq!(output.diagnostics.len(), 5);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::InvalidCharacter)
    );
    assert_eq!(output.diagnostics[1].span, Span::new(2, 4));
    assert_eq!(output.tokens[0].kind, Let);
    assert_eq!(output.tokens.last().map(|t| &t.kind), Some(&Eof));
}

#[test]
fn malformed_literals_recover() {
    for (source, code) in [
        ("\"abc", DiagnosticCode::UnterminatedLiteral),
        ("'x", DiagnosticCode::UnterminatedLiteral),
        ("\"abc\\", DiagnosticCode::UnterminatedLiteral),
        (r#""\q""#, DiagnosticCode::InvalidEscape),
        ("''", DiagnosticCode::InvalidChar),
        ("'ab'", DiagnosticCode::InvalidChar),
        ("'é'", DiagnosticCode::InvalidChar),
        ("18446744073709551616", DiagnosticCode::InvalidNumber),
    ] {
        let output = lex(source);
        assert_eq!(output.diagnostics[0].code, code, "{source}");
        assert_eq!(output.tokens.len(), 1, "{source}");
        assert_eq!(output.tokens[0].kind, Eof);
    }
    let output = lex("\"broken\nlet x");
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(output.tokens[0].kind, Let);
    let output = lex(&format!("{}.0", "9".repeat(400)));
    assert_eq!(output.diagnostics[0].code, DiagnosticCode::InvalidNumber);
}

#[test]
fn hello_tokens() {
    assert_eq!(
        kinds(include_str!("../../../examples/hello.skuld")),
        vec![
            Function,
            Identifier("main".into()),
            LeftParen,
            RightParen,
            LeftBrace,
            Identifier("print".into()),
            LeftParen,
            String("Hello from Skuld!".into()),
            RightParen,
            RightBrace,
            Eof
        ]
    );
}

#[test]
fn arbitrary_small_inputs_preserve_span_invariants() {
    let alphabet = [
        'a', '0', '.', '"', '\'', '\\', '\n', '\r', 'é', '🦀', '@', '/', '=',
    ];
    for a in alphabet {
        for b in alphabet {
            for c in alphabet {
                let source: std::string::String = [a, b, c].into_iter().collect();
                let output = lex(&source);
                assert_eq!(output.tokens.iter().filter(|t| t.kind == Eof).count(), 1);
                for span in output
                    .tokens
                    .iter()
                    .map(|t| t.span)
                    .chain(output.diagnostics.iter().map(|d| d.span))
                {
                    assert!(span.start <= span.end && span.end <= source.len());
                    assert!(
                        source.is_char_boundary(span.start) && source.is_char_boundary(span.end)
                    );
                }
            }
        }
    }
}

#[test]
fn func_is_keyword_print_and_old_spellings_are_identifiers() {
    assert_eq!(
        kinds("func print fn function println func_name"),
        vec![
            Function,
            Identifier("print".into()),
            Identifier("fn".into()),
            Identifier("function".into()),
            Identifier("println".into()),
            Identifier("func_name".into()),
            Eof
        ]
    );
}

#[test]
fn break_and_continue_are_keywords() {
    assert_eq!(
        kinds("break continue breaks continued"),
        vec![
            Break,
            Continue,
            Identifier("breaks".into()),
            Identifier("continued".into()),
            Eof
        ]
    );
}

#[test]
fn interpolation_splits_text_from_expressions() {
    assert_eq!(
        kinds("\"a ${x} b\""),
        vec![
            InterpolationBegin("a ".into()),
            Identifier("x".into()),
            InterpolationEnd(" b".into()),
            Eof
        ]
    );
    assert_eq!(
        kinds("\"${a}-${b}\""),
        vec![
            InterpolationBegin("".into()),
            Identifier("a".into()),
            InterpolationPart("-".into()),
            Identifier("b".into()),
            InterpolationEnd("".into()),
            Eof
        ]
    );
    // Braces inside the expression are counted, so a record literal does not
    // end the interpolation early.
    assert_eq!(
        kinds("\"${P { n: 1 }}\""),
        vec![
            InterpolationBegin("".into()),
            Identifier("P".into()),
            LeftBrace,
            Identifier("n".into()),
            Colon,
            Integer(1),
            RightBrace,
            InterpolationEnd("".into()),
            Eof
        ]
    );
    // A string with no `${` is still one plain literal.
    assert_eq!(kinds("\"plain\""), vec![String("plain".into()), Eof]);
    // `\$` writes a literal `${`.
    assert_eq!(
        kinds("\"\\${literal}\""),
        vec![String("${literal}".into()), Eof]
    );
    // A lone `$` is ordinary text.
    assert_eq!(kinds("\"5$\""), vec![String("5$".into()), Eof]);
}

#[test]
fn dot_dot_range_tokens() {
    use TokenKind::*;
    assert_eq!(kinds(".."), vec![DotDot, Eof]);
    assert_eq!(kinds("0..10"), vec![Integer(0), DotDot, Integer(10), Eof]);
    assert_eq!(
        kinds("for i in 0..len(bytes)"),
        vec![
            For,
            Identifier("i".into()),
            In,
            Integer(0),
            DotDot,
            Identifier("len".into()),
            LeftParen,
            Identifier("bytes".into()),
            RightParen,
            Eof
        ]
    );
}

#[test]
fn bitwise_and_shift_tokens() {
    use TokenKind::*;
    assert_eq!(
        kinds("& | ^ ~ << >> &= |= ^= <<= >>="),
        vec![
            Ampersand,
            Pipe,
            Caret,
            Tilde,
            LessLess,
            GreaterGreater,
            AmpersandEqual,
            PipeEqual,
            CaretEqual,
            LessLessEqual,
            GreaterGreaterEqual,
            Eof,
        ]
    );
}

#[test]
fn base_integer_literals_and_separators() {
    use TokenKind::*;
    assert_eq!(
        kinds("0x1A_2F 0b1010_0101 0o755 1_000_000 1_2.5_0 0XFF 0B11 0O77"),
        vec![
            Integer(0x1A2F),
            Integer(0b10100101),
            Integer(0o755),
            Integer(1000000),
            Float(12.5),
            Integer(255),
            Integer(3),
            Integer(63),
            Eof,
        ]
    );

    for (source, expected_msg) in [
        ("0x", "hexadecimal literal has no digits"),
        ("0x_", "hexadecimal literal has no digits"),
        ("0b", "binary literal has no digits"),
        ("0o", "octal literal has no digits"),
        ("0x12G", "invalid digit `G` in hexadecimal literal"),
        ("0b102", "invalid digit `2` in binary literal"),
        ("0o78", "invalid digit `8` in octal literal"),
    ] {
        let output = lex(source);
        assert_eq!(output.diagnostics.len(), 1, "{source}");
        assert_eq!(
            output.diagnostics[0].code,
            DiagnosticCode::InvalidNumber,
            "{source}"
        );
        assert_eq!(output.diagnostics[0].message, expected_msg, "{source}");
    }
}
