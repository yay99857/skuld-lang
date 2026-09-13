//! Recursive descent statements/declarations and Pratt expressions.
//! Newline information is read from gaps between token spans, not lexer tokens.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode},
    lexer::lex,
    span::Span,
    token::{Token, TokenKind},
};

#[derive(Debug)]
pub struct ParseOutput {
    /// No partial AST is exposed after lexical or syntax errors.
    pub program: Option<Program>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Runs lexing followed by parsing. Resolution and type checking are separate.
pub fn parse(source: &str) -> ParseOutput {
    let lexed = lex(source);
    if !lexed.diagnostics.is_empty() {
        return ParseOutput {
            program: None,
            diagnostics: lexed.diagnostics,
        };
    }
    Parser {
        source,
        tokens: lexed.tokens,
        position: 0,
        diagnostics: Vec::new(),
        depth: 0,
        expression_start: None,
        struct_literals: true,
    }
    .program()
}

// Bound recursive syntax and expression size before exhausting the host stack.
const MAX_DEPTH: usize = 64;
const MAX_EXPRESSION_TOKENS: usize = 256;
type Parsed<T> = Result<T, Diagnostic>;

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    position: usize,
    diagnostics: Vec<Diagnostic>,
    depth: usize,
    expression_start: Option<usize>,
    /// False while parsing an `if`/`while` condition, where `name {` would be
    /// ambiguous with the block that follows.
    struct_literals: bool,
}

#[derive(Clone, Copy)]
enum Infix {
    Binary(BinaryOp),
    Assignment(AssignmentOp),
}

impl Parser<'_> {
    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }
    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind)
    }
    fn bump(&mut self) -> Token {
        let token = self.current().clone();
        if token.kind != TokenKind::Eof {
            self.position += 1;
        }
        token
    }
    fn take(&mut self, kind: &TokenKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            None
        }
    }
    fn error(&self, code: DiagnosticCode, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            message: message.into(),
            span: self.current().span,
            help: None,
        }
    }
    fn expected(&self, description: &str) -> Diagnostic {
        let found = if self.at(&TokenKind::Eof) {
            "end of file".to_owned()
        } else {
            format!(
                "`{}`",
                self.source[self.current().span.start..self.current().span.end].escape_debug()
            )
        };
        self.error(
            DiagnosticCode::ExpectedSyntax,
            format!("expected {description}, found {found}"),
        )
    }
    fn expect(&mut self, kind: &TokenKind, description: &str) -> Parsed<Token> {
        self.take(kind).ok_or_else(|| self.expected(description))
    }
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Parsed<T>) -> Parsed<T> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error(
                DiagnosticCode::SyntaxLimit,
                "syntax nesting limit exceeded (64 levels)",
            ));
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }
    fn previous_end(&self) -> usize {
        if self.position == 0 {
            0
        } else {
            self.tokens[self.position - 1].span.end
        }
    }
    fn newline_before(&self) -> bool {
        self.source[self.previous_end()..self.current().span.start].contains(['\n', '\r'])
    }
    fn name(&mut self, description: &str) -> Parsed<Name> {
        if let TokenKind::Identifier(text) = &self.current().kind {
            let name = Name {
                text: text.clone(),
                span: self.current().span,
            };
            self.bump();
            Ok(name)
        } else {
            Err(self.expected(description))
        }
    }
    /// `if p { }` and `while p { }` would otherwise read `p { ... }` as record
    /// construction. Conditions forbid a bare struct literal; parentheses make
    /// one available again, as does any nested expression context.
    fn with_struct_literals<T>(
        &mut self,
        allowed: bool,
        parse: impl FnOnce(&mut Self) -> Parsed<T>,
    ) -> Parsed<T> {
        let previous = std::mem::replace(&mut self.struct_literals, allowed);
        let result = parse(self);
        self.struct_literals = previous;
        result
    }
    fn type_ref(&mut self) -> Parsed<TypeRef> {
        self.nested(|p| p.type_ref_inner())
    }
    /// The byte that closes a generic argument list. In a type annotation
    /// `Option<T>=value` the lexer produced one `>=`, whose first byte closes
    /// the type; the assignment token is left behind for its normal parser.
    fn close_generic(&mut self, message: &str) -> Parsed<usize> {
        if self.at(&TokenKind::GreaterEqual) {
            let span = self.current().span;
            self.tokens[self.position] = Token {
                kind: TokenKind::Equal,
                span: Span::new(span.start + 1, span.end),
            };
            Ok(span.start + 1)
        } else {
            Ok(self.expect(&TokenKind::Greater, message)?.span.end)
        }
    }
    fn type_ref_inner(&mut self) -> Parsed<TypeRef> {
        if matches!(&self.current().kind, TokenKind::Identifier(name) if name == "Option") {
            let start = self.bump().span.start;
            self.expect(&TokenKind::Less, "`<` and the Option payload type")?;
            let element = Box::new(self.type_ref()?);
            let end = self.close_generic("`>` after the Option payload type")?;
            return Ok(TypeRef::Option {
                element,
                span: Span::new(start, end),
            });
        }
        if matches!(&self.current().kind, TokenKind::Identifier(name) if name == "Result") {
            let start = self.bump().span.start;
            self.expect(&TokenKind::Less, "`<` and the Result success type")?;
            let ok = Box::new(self.type_ref()?);
            self.expect(&TokenKind::Comma, "`,` and the Result error type")?;
            let err = Box::new(self.type_ref()?);
            let end = self.close_generic("`>` after the Result error type")?;
            return Ok(TypeRef::Result {
                ok,
                err,
                span: Span::new(start, end),
            });
        }
        if self.at(&TokenKind::Weak) {
            let start = self.bump().span.start;
            let class = self.name("a class name after `weak`")?;
            let span = Span::new(start, class.span.end);
            return Ok(TypeRef::Weak { class, span });
        }
        if self.at(&TokenKind::LeftBracket) {
            let start = self.bump().span.start;
            self.expect(&TokenKind::RightBracket, "`]` after `[` in an array type")?;
            let element = self.type_ref()?;
            let end = element.span().end;
            Ok(TypeRef::Array {
                element: Box::new(element),
                span: Span::new(start, end),
            })
        } else {
            Ok(TypeRef::Named(self.name("a type name")?))
        }
    }

    fn program(mut self) -> ParseOutput {
        let mut functions = Vec::new();
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        while !self.at(&TokenKind::Eof) {
            let start = self.position;
            if self.at(&TokenKind::Struct) || self.at(&TokenKind::Class) {
                match self.struct_declaration() {
                    Ok(declaration) => structs.push(declaration),
                    Err(diagnostic) => {
                        self.diagnostics.push(diagnostic);
                        self.recover_declaration(start);
                    }
                }
                continue;
            }
            if self.at(&TokenKind::Enum) {
                match self.enum_declaration() {
                    Ok(declaration) => enums.push(declaration),
                    Err(diagnostic) => {
                        self.diagnostics.push(diagnostic);
                        self.recover_declaration(start);
                    }
                }
                continue;
            }
            let result = if self.at(&TokenKind::Function) {
                self.function()
            } else {
                Err(self.error(DiagnosticCode::ExpectedDeclaration, "expected a function declaration (`func`); executable global statements and other declarations are not supported"))
            };
            match result {
                Ok(function) => functions.push(function),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    self.recover_declaration(start);
                }
            }
        }
        let program = self.diagnostics.is_empty().then_some(Program {
            structs,
            enums,
            functions,
            span: Span::new(0, self.source.len()),
        });
        ParseOutput {
            program,
            diagnostics: self.diagnostics,
        }
    }
    fn recover_declaration(&mut self, start: usize) {
        if self.position == start {
            self.bump();
        }
        while !self.at(&TokenKind::Function)
            && !self.at(&TokenKind::Struct)
            && !self.at(&TokenKind::Class)
            && !self.at(&TokenKind::Enum)
            && !self.at(&TokenKind::Eof)
        {
            self.bump();
        }
    }
    fn enum_declaration(&mut self) -> Parsed<EnumDecl> {
        let start = self.expect(&TokenKind::Enum, "`enum`")?.span.start;
        let name = self.name("an enum name")?;
        self.expect(&TokenKind::LeftBrace, "`{` to begin the enum body")?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            let variant_name = self.name("a variant name")?;
            let payload = if self.take(&TokenKind::LeftParen).is_some() {
                let ty = self.type_ref()?;
                self.expect(&TokenKind::RightParen, "`)` after payload type")?;
                Some(ty)
            } else {
                None
            };
            let span = Span::new(variant_name.span.start, self.previous_end());
            variants.push(VariantDecl {
                name: variant_name,
                payload,
                span,
            });
            let has_comma = self.take(&TokenKind::Comma).is_some();
            if !has_comma
                && !self.at(&TokenKind::RightBrace)
                && !self.at(&TokenKind::Eof)
                && !self.newline_before()
            {
                return Err(self.expected("a newline, `,` or `}` after the variant"));
            }
        }
        let end = self
            .expect(&TokenKind::RightBrace, "`}` to close the enum body")?
            .span
            .end;
        Ok(EnumDecl {
            name,
            variants,
            span: Span::new(start, end),
        })
    }
    /// One field per line, matching the statement-boundary rule elsewhere.
    fn struct_declaration(&mut self) -> Parsed<StructDecl> {
        let (kind, noun) = if self.at(&TokenKind::Class) {
            (TypeDeclKind::Reference, "class")
        } else {
            (TypeDeclKind::Value, "struct")
        };
        let start = self.bump().span.start;
        let name = self.name(&format!("a {noun} name"))?;
        self.expect(&TokenKind::LeftBrace, "`{` to begin the body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            let field_start = self.current().span.start;
            let name = self.name("a field or method name")?;
            // `name(` is a method; `name:` is a field.
            if self.at(&TokenKind::LeftParen) {
                methods.push(self.nested(|parser| parser.method(name.clone(), field_start))?);
                continue;
            }
            self.expect(&TokenKind::Colon, "`:` and a field type")?;
            let type_ref = self.type_ref()?;
            fields.push(FieldDecl {
                name,
                type_ref,
                span: Span::new(field_start, self.previous_end()),
            });
            self.take(&TokenKind::Comma);
            if !self.at(&TokenKind::RightBrace)
                && !self.at(&TokenKind::Eof)
                && !self.newline_before()
            {
                return Err(self.expected("a newline or `}` after the field"));
            }
        }
        let end = self
            .expect(&TokenKind::RightBrace, "`}` to close the body")?
            .span
            .end;
        Ok(StructDecl {
            kind,
            name,
            fields,
            methods,
            span: Span::new(start, end),
        })
    }
    /// The name and `(` are already known; methods carry no `func` keyword.
    fn method(&mut self, name: Name, start: usize) -> Parsed<FunctionDecl> {
        let parameters = self.parameter_list()?;
        let return_type =
            if self.take(&TokenKind::Colon).is_some() || self.take(&TokenKind::Arrow).is_some() {
                Some(self.type_ref()?)
            } else {
                None
            };
        let body = self.block()?;
        let span = Span::new(start, body.span.end);
        Ok(FunctionDecl {
            name,
            parameters,
            return_type,
            body,
            span,
        })
    }
    fn parameter_list(&mut self) -> Parsed<Vec<Parameter>> {
        self.expect(&TokenKind::LeftParen, "`(` after the name")?;
        let mut parameters = Vec::new();
        if !self.at(&TokenKind::RightParen) {
            loop {
                let name = self.name("a parameter name")?;
                let start = name.span.start;
                self.expect(&TokenKind::Colon, "`:` and an explicit parameter type")?;
                let type_ref = self.type_ref()?;
                parameters.push(Parameter {
                    name,
                    type_ref,
                    span: Span::new(start, self.previous_end()),
                });
                if self.take(&TokenKind::Comma).is_none() || self.at(&TokenKind::RightParen) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RightParen, "`)` after parameters")?;
        Ok(parameters)
    }
    fn function(&mut self) -> Parsed<FunctionDecl> {
        let start = self.expect(&TokenKind::Function, "`func`")?.span.start;
        let name = self.name("a function name")?;
        let parameters = self.parameter_list()?;
        let return_type =
            if self.take(&TokenKind::Colon).is_some() || self.take(&TokenKind::Arrow).is_some() {
                Some(self.type_ref()?)
            } else {
                None
            };
        let body = self.block()?;
        let span = Span::new(start, body.span.end);
        Ok(FunctionDecl {
            name,
            parameters,
            return_type,
            body,
            span,
        })
    }
    fn block(&mut self) -> Parsed<Block> {
        self.nested(|parser| parser.block_inner())
    }
    fn block_inner(&mut self) -> Parsed<Block> {
        let start = self
            .expect(&TokenKind::LeftBrace, "`{` to begin a block")?
            .span
            .start;
        let mut statements = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            // A new function can indicate a missing closing brace. Preserve it for
            // top-level recovery instead of consuming the next declaration.
            if self.at(&TokenKind::Function) {
                return Err(self.expected("`}` before the next function"));
            }
            let before = self.position;
            match self.statement() {
                Ok(statement) => statements.push(statement),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    self.synchronize_statement(before);
                }
            }
        }
        let end = self
            .expect(&TokenKind::RightBrace, "`}` to close the block")?
            .span
            .end;
        Ok(Block {
            statements,
            span: Span::new(start, end),
        })
    }
    fn synchronize_statement(&mut self, before: usize) {
        if self.position == before && !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof)
        {
            self.bump();
        }
        while !self.at(&TokenKind::RightBrace)
            && !self.at(&TokenKind::Eof)
            && !self.at(&TokenKind::Function)
        {
            if matches!(
                self.current().kind,
                TokenKind::Let
                    | TokenKind::Var
                    | TokenKind::Return
                    | TokenKind::If
                    | TokenKind::While
                    | TokenKind::Loop
                    | TokenKind::Break
                    | TokenKind::Continue
                    | TokenKind::LeftBrace
            ) || self.newline_before()
            {
                break;
            }
            self.bump();
        }
    }
    fn statement(&mut self) -> Parsed<Statement> {
        self.nested(|parser| parser.statement_inner())
    }
    fn statement_inner(&mut self) -> Parsed<Statement> {
        use TokenKind::*;
        let start = self.current().span.start;
        let kind = match self.current().kind {
            Let | Var => {
                let mutability = if self.bump().kind == Let {
                    Mutability::Immutable
                } else {
                    Mutability::Mutable
                };
                let name = self.name("a variable name")?;
                let type_ref = if self.take(&Colon).is_some() {
                    Some(self.type_ref()?)
                } else {
                    None
                };
                self.expect(&Equal, "`=` and a variable initializer")?;
                let initializer = self.expression()?;
                StatementKind::Variable(VariableDecl {
                    name,
                    mutability,
                    type_ref,
                    initializer,
                    span: Span::new(start, self.previous_end()),
                })
            }
            Return => {
                self.bump();
                let value = if self.at(&RightBrace) || self.at(&Eof) || self.newline_before() {
                    None
                } else {
                    Some(self.expression()?)
                };
                StatementKind::Return(value)
            }
            LeftBrace => {
                let block = self.block()?;
                return Ok(Statement {
                    span: block.span,
                    kind: StatementKind::Block(block),
                });
            }
            If => return self.if_statement(),
            While => return self.while_statement(),
            Loop => {
                let start = self.bump().span.start;
                let body = self.block()?;
                return Ok(Statement {
                    kind: StatementKind::Loop { body },
                    span: Span::new(start, self.previous_end()),
                });
            }
            Break => {
                self.bump();
                StatementKind::Break
            }
            Continue => {
                self.bump();
                StatementKind::Continue
            }
            Match => return self.match_statement(),
            For => return self.for_statement(),
            Enum => {
                return Err(self.error(
                    DiagnosticCode::ExpectedDeclaration,
                    "enums can only be declared at the top level",
                ));
            }
            Impl | Interface | Import | Static | Extern => {
                return Err(self.error(
                    DiagnosticCode::UnsupportedSyntax,
                    "this syntax is reserved for a later milestone",
                ));
            }
            _ => StatementKind::Expression(self.expression()?),
        };
        let span = Span::new(start, self.previous_end());
        self.statement_end()?;
        Ok(Statement { kind, span })
    }
    fn statement_end(&self) -> Parsed<()> {
        if self.at(&TokenKind::RightBrace) || self.at(&TokenKind::Eof) || self.newline_before() {
            Ok(())
        } else {
            let mut diagnostic = self.expected("a newline or `}` after the statement");
            diagnostic.help = Some("put separate statements on separate lines; expressions may continue across lines with operators, calls or member access".into());
            Err(diagnostic)
        }
    }
    fn while_statement(&mut self) -> Parsed<Statement> {
        let start = self.expect(&TokenKind::While, "`while`")?.span.start;
        let condition = self.with_struct_literals(false, |parser| parser.expression())?;
        let body = self.block()?;
        Ok(Statement {
            kind: StatementKind::While { condition, body },
            span: Span::new(start, self.previous_end()),
        })
    }
    fn match_statement(&mut self) -> Parsed<Statement> {
        let start = self.expect(&TokenKind::Match, "`match`")?.span.start;
        let value = self.with_struct_literals(false, |parser| parser.expression())?;
        self.expect(&TokenKind::LeftBrace, "`{` to begin match arms")?;
        let mut arms = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            let arm_start = self.current().span.start;
            let pattern = if matches!(&self.current().kind, TokenKind::Identifier(name) if name == "_")
            {
                let token = self.bump();
                MatchPattern::Wildcard(token.span)
            } else {
                let first = self.name("a pattern or `_`")?;
                let (enum_name, variant_name) = if self.take(&TokenKind::Dot).is_some() {
                    let variant = self.name("a variant name")?;
                    (Some(first), variant)
                } else {
                    (None, first)
                };
                let binding = if self.take(&TokenKind::LeftParen).is_some() {
                    let binding = self.name("a variable name for the pattern payload")?;
                    self.expect(&TokenKind::RightParen, "`)` after pattern binding")?;
                    Some(binding)
                } else {
                    None
                };
                let span = Span::new(arm_start, self.previous_end());
                MatchPattern::Variant {
                    enum_name,
                    variant_name,
                    binding,
                    span,
                }
            };
            if self.take(&TokenKind::Colon).is_none() && self.take(&TokenKind::Arrow).is_none() {
                return Err(self.expected("`:` or `->` after match pattern"));
            }
            let body = if self.at(&TokenKind::LeftBrace) {
                self.block()?
            } else {
                let stmt = self.statement()?;
                let span = stmt.span;
                Block {
                    statements: vec![stmt],
                    span,
                }
            };
            self.take(&TokenKind::Comma);
            let arm_span = Span::new(arm_start, body.span.end);
            arms.push(MatchArm {
                pattern,
                body,
                span: arm_span,
            });
        }
        let end = self
            .expect(&TokenKind::RightBrace, "`}` to close match arms")?
            .span
            .end;
        Ok(Statement {
            kind: StatementKind::Match { value, arms },
            span: Span::new(start, end),
        })
    }
    fn for_statement(&mut self) -> Parsed<Statement> {
        let start = self.expect(&TokenKind::For, "`for`")?.span.start;
        let variable = self.name("a loop variable name")?;
        self.expect(&TokenKind::In, "`in` after the loop variable")?;
        let first = self.with_struct_literals(false, |parser| parser.expression())?;
        let iterable = if self.take(&TokenKind::DotDot).is_some() {
            let second = self.with_struct_literals(false, |parser| parser.expression())?;
            ForIterable::Range {
                start: first,
                end: second,
            }
        } else {
            ForIterable::Expr(first)
        };
        let body = self.block()?;
        let span = Span::new(start, body.span.end);
        Ok(Statement {
            kind: StatementKind::For {
                variable,
                iterable,
                body,
            },
            span,
        })
    }
    /// The lexer already split the text; only the expressions are parsed here.
    fn interpolation(&mut self, first: String) -> Parsed<ExprKind> {
        let mut parts = vec![InterpolationPart::Text(first)];
        loop {
            let value = self.with_struct_literals(true, |parser| parser.expression())?;
            parts.push(InterpolationPart::Value(value));
            match self.current().kind.clone() {
                TokenKind::InterpolationPart(text) => {
                    self.bump();
                    parts.push(InterpolationPart::Text(text));
                }
                TokenKind::InterpolationEnd(text) => {
                    self.bump();
                    parts.push(InterpolationPart::Text(text));
                    return Ok(ExprKind::Interpolation(parts));
                }
                _ => return Err(self.expected("`}` to close the interpolation")),
            }
        }
    }
    fn field_initializers(
        &mut self,
        open: &TokenKind,
        close: &TokenKind,
        open_message: &str,
        close_message: &str,
    ) -> Parsed<Vec<FieldInit>> {
        self.expect(open, open_message)?;
        let mut fields = Vec::new();
        while !self.at(close) && !self.at(&TokenKind::Eof) {
            let start = self.current().span.start;
            let field = self.name("a field name")?;
            self.expect(&TokenKind::Colon, "`:` and a field value")?;
            let value = self.with_struct_literals(true, |parser| parser.expression())?;
            fields.push(FieldInit {
                name: field,
                value,
                span: Span::new(start, self.previous_end()),
            });
            if self.take(&TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect(close, close_message)?;
        Ok(fields)
    }
    fn struct_literal(&mut self, name: Name) -> Parsed<ExprKind> {
        let fields = self.field_initializers(
            &TokenKind::LeftBrace,
            &TokenKind::RightBrace,
            "`{` to begin the fields",
            "`}` after the fields",
        )?;
        Ok(ExprKind::StructLiteral { name, fields })
    }
    fn if_statement(&mut self) -> Parsed<Statement> {
        let start = self.expect(&TokenKind::If, "`if`")?.span.start;
        let binding = if self.take(&TokenKind::Let).is_some() {
            let constructor = match &self.current().kind {
                TokenKind::Identifier(text) if text == "Some" => Some(IfLetPattern::Some),
                TokenKind::Identifier(text) if text == "Ok" => Some(IfLetPattern::Ok),
                TokenKind::Identifier(text) if text == "Err" => Some(IfLetPattern::Err),
                _ => None,
            }
            .filter(|_| {
                self.tokens
                    .get(self.position + 1)
                    .is_some_and(|t| t.kind == TokenKind::LeftParen)
            });
            let (pattern, name) = if let Some(pattern) = constructor {
                self.bump();
                self.expect(&TokenKind::LeftParen, "`(` after the pattern name")?;
                let name = self.name("a binding name")?;
                self.expect(&TokenKind::RightParen, "`)` after the binding")?;
                (pattern, name)
            } else {
                let name = self.name("a binding name")?;
                if name.text == "None" || name.text == "null" {
                    return Err(Diagnostic {
                        code: DiagnosticCode::ExpectedSyntax,
                        span: name.span,
                        message: "cannot bind to `null` or `None` in an if-let pattern".into(),
                        help: None,
                    });
                }
                (IfLetPattern::Some, name)
            };
            self.expect(&TokenKind::Equal, "`=` after the pattern")?;
            Some((pattern, name))
        } else {
            None
        };
        let condition = self.with_struct_literals(false, |parser| parser.expression())?;
        let then_block = self.block()?;
        let else_branch = if self.take(&TokenKind::Else).is_some() {
            if self.at(&TokenKind::If) {
                Some(Box::new(self.nested(|parser| parser.if_statement())?))
            } else {
                let block = self.block()?;
                Some(Box::new(Statement {
                    span: block.span,
                    kind: StatementKind::Block(block),
                }))
            }
        } else {
            None
        };
        Ok(Statement {
            kind: if let Some((pattern, binding)) = binding {
                StatementKind::IfLet {
                    pattern,
                    binding,
                    value: condition,
                    then_block,
                    else_branch,
                }
            } else {
                StatementKind::If {
                    condition,
                    then_block,
                    else_branch,
                }
            },
            span: Span::new(start, self.previous_end()),
        })
    }
    fn expression(&mut self) -> Parsed<Expr> {
        let root = self.expression_start.is_none();
        if root {
            self.expression_start = Some(self.position);
        }
        let result = self.expression_bp(0);
        if root {
            self.expression_start = None;
        }
        result
    }
    fn expression_limit(&self) -> Parsed<()> {
        if self
            .position
            .saturating_sub(self.expression_start.unwrap_or(self.position))
            >= MAX_EXPRESSION_TOKENS
        {
            Err(self.error(
                DiagnosticCode::SyntaxLimit,
                "expression complexity limit exceeded (256 tokens)",
            ))
        } else {
            Ok(())
        }
    }
    fn expression_bp(&mut self, minimum: u8) -> Parsed<Expr> {
        self.nested(|parser| parser.expression_inner(minimum))
    }
    fn expression_inner(&mut self, minimum: u8) -> Parsed<Expr> {
        self.expression_limit()?;
        let token = self.current().clone();
        let kind = match token.kind {
            TokenKind::Integer(value) => {
                self.bump();
                ExprKind::Literal(Literal::Integer(value))
            }
            TokenKind::Float(value) => {
                self.bump();
                ExprKind::Literal(Literal::Float(value))
            }
            TokenKind::String(value) => {
                self.bump();
                ExprKind::Literal(Literal::String(value))
            }
            TokenKind::Char(value) => {
                self.bump();
                ExprKind::Literal(Literal::Char(value))
            }
            TokenKind::Boolean(value) => {
                self.bump();
                ExprKind::Literal(Literal::Boolean(value))
            }
            TokenKind::Identifier(text) => {
                self.bump();
                let name = Name {
                    text,
                    span: token.span,
                };
                if self.struct_literals && self.at(&TokenKind::LeftBrace) {
                    self.struct_literal(name)?
                } else {
                    ExprKind::Identifier(name)
                }
            }
            TokenKind::Weak => {
                self.bump();
                self.expect(&TokenKind::LeftParen, "`(` after `weak`")?;
                let value = if self.at(&TokenKind::RightParen) {
                    None
                } else {
                    Some(Box::new(
                        self.with_struct_literals(true, |p| p.expression())?,
                    ))
                };
                self.expect(&TokenKind::RightParen, "`)` after weak reference")?;
                ExprKind::Weak(value)
            }
            TokenKind::New => {
                self.bump();
                let name = self.name("a class name after `new`")?;
                let fields = self.field_initializers(
                    &TokenKind::LeftParen,
                    &TokenKind::RightParen,
                    "`(` after the class name",
                    "`)` after the fields",
                )?;
                ExprKind::New { name, fields }
            }
            TokenKind::InterpolationBegin(text) => {
                self.bump();
                self.interpolation(text)?
            }
            TokenKind::LeftBracket => {
                self.bump();
                let mut elements = Vec::new();
                if !self.at(&TokenKind::RightBracket) {
                    loop {
                        elements.push(self.with_struct_literals(true, |p| p.expression())?);
                        if self.take(&TokenKind::Comma).is_none()
                            || self.at(&TokenKind::RightBracket)
                        {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RightBracket, "`]` after array elements")?;
                ExprKind::Array(elements)
            }
            TokenKind::LeftParen => {
                self.bump();
                let value = self.with_struct_literals(true, |parser| parser.expression())?;
                self.expect(&TokenKind::RightParen, "`)` after the grouped expression")?;
                ExprKind::Group(Box::new(value))
            }
            TokenKind::Plus | TokenKind::Minus | TokenKind::Bang => {
                self.bump();
                let op = match token.kind {
                    TokenKind::Plus => UnaryOp::Positive,
                    TokenKind::Minus => UnaryOp::Negative,
                    _ => UnaryOp::Not,
                };
                let operand = self.expression_bp(14)?;
                ExprKind::Unary {
                    op,
                    op_span: token.span,
                    operand: Box::new(operand),
                }
            }
            _ => return Err(self.expected("an expression")),
        };
        let mut left = Expr {
            kind,
            span: Span::new(token.span.start, self.previous_end()),
        };
        loop {
            self.expression_limit()?;
            if self.at(&TokenKind::Question) {
                let end = self.bump().span.end;
                let span = Span::new(left.span.start, end);
                left = Expr {
                    kind: ExprKind::Try(Box::new(left)),
                    span,
                };
                continue;
            }
            if self.at(&TokenKind::LeftBracket) {
                self.bump();
                let index = self.with_struct_literals(true, |p| p.expression())?;
                // `[a..b]` slices; `[a]` indexes. The `..` is the same token
                // a `for` range uses.
                let slice_end = if self.take(&TokenKind::DotDot).is_some() {
                    Some(self.with_struct_literals(true, |p| p.expression())?)
                } else {
                    None
                };
                let end = self
                    .expect(
                        &TokenKind::RightBracket,
                        if slice_end.is_some() {
                            "`]` after the slice range"
                        } else {
                            "`]` after array index"
                        },
                    )?
                    .span
                    .end;
                let span = Span::new(left.span.start, end);
                left = Expr {
                    kind: match slice_end {
                        Some(slice_end) => ExprKind::Slice {
                            object: Box::new(left),
                            start: Box::new(index),
                            end: Box::new(slice_end),
                        },
                        None => ExprKind::Index {
                            object: Box::new(left),
                            index: Box::new(index),
                        },
                    },
                    span,
                };
                continue;
            }
            if self.at(&TokenKind::LeftParen) {
                self.bump();
                let mut arguments = Vec::new();
                if !self.at(&TokenKind::RightParen) {
                    loop {
                        arguments.push(self.with_struct_literals(true, |p| p.expression())?);
                        if self.take(&TokenKind::Comma).is_none() || self.at(&TokenKind::RightParen)
                        {
                            break;
                        }
                    }
                }
                let end = self
                    .expect(&TokenKind::RightParen, "`)` after call arguments")?
                    .span
                    .end;
                let span = Span::new(left.span.start, end);
                left = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(left),
                        arguments,
                    },
                    span,
                };
                continue;
            }
            if self.take(&TokenKind::Dot).is_some() {
                let member = self.name("a member name after `.`")?;
                let span = Span::new(left.span.start, member.span.end);
                left = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(left),
                        member,
                    },
                    span,
                };
                continue;
            }
            let Some((left_bp, right_bp, op)) = infix(&self.current().kind) else {
                break;
            };
            if left_bp < minimum {
                break;
            }
            let op_span = self.bump().span;
            if matches!(op, Infix::Assignment(_))
                && !matches!(
                    left.kind,
                    ExprKind::Identifier(_) | ExprKind::Member { .. } | ExprKind::Index { .. }
                )
            {
                return Err(Diagnostic {
                    code: DiagnosticCode::InvalidAssignmentTarget,
                    message: "assignment target must be a variable or member".into(),
                    span: left.span,
                    help: None,
                });
            }
            let right = self.expression_bp(right_bp)?;
            let span = Span::new(left.span.start, right.span.end);
            let kind = match op {
                Infix::Binary(op) => ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    op_span,
                    right: Box::new(right),
                },
                Infix::Assignment(op) => ExprKind::Assignment {
                    target: Box::new(left),
                    op,
                    op_span,
                    value: Box::new(right),
                },
            };
            left = Expr { kind, span };
        }
        Ok(left)
    }
}

fn infix(token: &TokenKind) -> Option<(u8, u8, Infix)> {
    use TokenKind::*;
    let (precedence, op) = match token {
        Equal => (1, Infix::Assignment(AssignmentOp::Assign)),
        PlusEqual => (1, Infix::Assignment(AssignmentOp::Add)),
        MinusEqual => (1, Infix::Assignment(AssignmentOp::Subtract)),
        StarEqual => (1, Infix::Assignment(AssignmentOp::Multiply)),
        SlashEqual => (1, Infix::Assignment(AssignmentOp::Divide)),
        OrOr => (2, Infix::Binary(BinaryOp::Or)),
        AndAnd => (4, Infix::Binary(BinaryOp::And)),
        EqualEqual => (6, Infix::Binary(BinaryOp::Equal)),
        BangEqual => (6, Infix::Binary(BinaryOp::NotEqual)),
        Less => (8, Infix::Binary(BinaryOp::Less)),
        Greater => (8, Infix::Binary(BinaryOp::Greater)),
        LessEqual => (8, Infix::Binary(BinaryOp::LessEqual)),
        GreaterEqual => (8, Infix::Binary(BinaryOp::GreaterEqual)),
        Plus => (10, Infix::Binary(BinaryOp::Add)),
        Minus => (10, Infix::Binary(BinaryOp::Subtract)),
        Star => (12, Infix::Binary(BinaryOp::Multiply)),
        Slash => (12, Infix::Binary(BinaryOp::Divide)),
        Percent => (12, Infix::Binary(BinaryOp::Modulo)),
        _ => return None,
    };
    Some((
        precedence,
        if matches!(op, Infix::Assignment(_)) {
            precedence
        } else {
            precedence + 1
        },
        op,
    ))
}

#[cfg(test)]
mod tests;
