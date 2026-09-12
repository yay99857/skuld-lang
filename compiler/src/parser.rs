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
        Ok(TypeRef::Named(self.name("a type name")?))
    }

    fn program(mut self) -> ParseOutput {
        let mut functions = Vec::new();
        let mut structs = Vec::new();
        while !self.at(&TokenKind::Eof) {
            let start = self.position;
            if self.at(&TokenKind::Struct) {
                match self.struct_declaration() {
                    Ok(declaration) => structs.push(declaration),
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
            && !self.at(&TokenKind::Eof)
        {
            self.bump();
        }
    }
    /// One field per line, matching the statement-boundary rule elsewhere.
    fn struct_declaration(&mut self) -> Parsed<StructDecl> {
        let start = self.expect(&TokenKind::Struct, "`struct`")?.span.start;
        let name = self.name("a struct name")?;
        self.expect(&TokenKind::LeftBrace, "`{` to begin the struct body")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            let field_start = self.current().span.start;
            let name = self.name("a field name")?;
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
            .expect(&TokenKind::RightBrace, "`}` to close the struct body")?
            .span
            .end;
        Ok(StructDecl {
            name,
            fields,
            span: Span::new(start, end),
        })
    }
    fn function(&mut self) -> Parsed<FunctionDecl> {
        let start = self.expect(&TokenKind::Function, "`func`")?.span.start;
        let name = self.name("a function name")?;
        self.expect(&TokenKind::LeftParen, "`(` after the function name")?;
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
        let return_type = if self.take(&TokenKind::Arrow).is_some() {
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
            Class | Impl | Interface | Enum | Match | Import | For | Static | Extern => {
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
    fn struct_literal(&mut self, name: Name) -> Parsed<ExprKind> {
        self.expect(&TokenKind::LeftBrace, "`{` to begin the fields")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
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
        self.expect(&TokenKind::RightBrace, "`}` after the fields")?;
        Ok(ExprKind::StructLiteral { name, fields })
    }
    fn if_statement(&mut self) -> Parsed<Statement> {
        let start = self.expect(&TokenKind::If, "`if`")?.span.start;
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
            kind: StatementKind::If {
                condition,
                then_block,
                else_branch,
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
                && !matches!(left.kind, ExprKind::Identifier(_) | ExprKind::Member { .. })
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
