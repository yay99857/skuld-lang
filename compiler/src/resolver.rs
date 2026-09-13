//! Lexical value-name resolution over an unchanged, single-source parser AST.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode},
    span::Span,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymbolId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Print,
    Some,
    None,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Builtin(Builtin),
    Function,
    Parameter,
    Variable(Mutability),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Builtins have no source declaration.
    pub span: Option<Span>,
    pub scope: ScopeId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub span: Option<Span>,
    pub symbols: BTreeMap<String, SymbolId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    /// Keys are declaration-name byte starts in the original, unchanged AST.
    pub declarations: BTreeMap<usize, SymbolId>,
    /// Keys are identifier-use byte starts. Member labels are not value names.
    pub references: BTreeMap<usize, SymbolId>,
}
#[derive(Debug)]
pub struct ResolveOutput {
    pub resolution: Option<Resolution>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolve a parser-produced AST. Tables belong only to this source revision;
/// no type names, member labels, call signatures or entrypoints are checked.
pub fn resolve(program: &Program) -> ResolveOutput {
    let mut resolver = Resolver {
        result: Resolution {
            symbols: Vec::new(),
            scopes: Vec::new(),
            declarations: BTreeMap::new(),
            references: BTreeMap::new(),
        },
        diagnostics: Vec::new(),
        current: ScopeId(0),
    };
    resolver.result.scopes.push(Scope {
        parent: None,
        span: None,
        symbols: BTreeMap::new(),
    });
    resolver.insert("print", SymbolKind::Builtin(Builtin::Print), None);
    resolver.insert("Some", SymbolKind::Builtin(Builtin::Some), None);
    resolver.insert("None", SymbolKind::Builtin(Builtin::None), None);
    resolver.enter(program.span);
    for function in &program.functions {
        resolver.declare(&function.name, SymbolKind::Function);
    }
    // Methods get symbols, but in a scope of their own so they never resolve as
    // bare identifiers: a method is reached through `this` or a value.
    let program_scope = resolver.current;
    for declaration in &program.structs {
        resolver.enter(declaration.span);
        for method in &declaration.methods {
            resolver.declare(&method.name, SymbolKind::Function);
        }
        let method_scope = resolver.current;
        for method in &declaration.methods {
            // Bodies resolve from the program scope, so a sibling method is not
            // visible without a receiver.
            resolver.current = program_scope;
            resolver.enter(method.body.span);
            resolver.insert(
                "this",
                SymbolKind::Parameter,
                Some(Span::new(method.body.span.start, method.body.span.start)),
            );
            for parameter in &method.parameters {
                resolver.declare(&parameter.name, SymbolKind::Parameter);
            }
            resolver.statements(&method.body);
            resolver.leave();
            resolver.current = method_scope;
        }
        resolver.leave();
    }
    for function in &program.functions {
        resolver.enter(function.body.span);
        for parameter in &function.parameters {
            resolver.declare(&parameter.name, SymbolKind::Parameter);
        }
        // Parameters and the outermost function body share one lexical scope.
        resolver.statements(&function.body);
        resolver.leave();
    }
    ResolveOutput {
        resolution: resolver.diagnostics.is_empty().then_some(resolver.result),
        diagnostics: resolver.diagnostics,
    }
}

struct Resolver {
    result: Resolution,
    diagnostics: Vec<Diagnostic>,
    current: ScopeId,
}
impl Resolver {
    fn enter(&mut self, span: Span) {
        let id = ScopeId(self.result.scopes.len());
        self.result.scopes.push(Scope {
            parent: Some(self.current),
            span: Some(span),
            symbols: BTreeMap::new(),
        });
        self.current = id;
    }
    fn leave(&mut self) {
        if let Some(parent) = self.result.scopes[self.current.0].parent {
            self.current = parent;
        }
    }
    fn insert(&mut self, name: &str, kind: SymbolKind, span: Option<Span>) -> SymbolId {
        let id = SymbolId(self.result.symbols.len());
        self.result.symbols.push(Symbol {
            name: name.into(),
            kind,
            span,
            scope: self.current,
        });
        self.result.scopes[self.current.0]
            .symbols
            .insert(name.into(), id);
        if let Some(span) = span {
            self.result.declarations.insert(span.start, id);
        }
        id
    }
    fn declare(&mut self, name: &Name, kind: SymbolKind) {
        if self.result.scopes[self.current.0]
            .symbols
            .contains_key(&name.text)
        {
            self.diagnostics.push(Diagnostic {
                code: DiagnosticCode::DuplicateDeclaration,
                message: format!("duplicate declaration of `{}` in the same scope", name.text),
                span: name.span,
                help: Some("rename this declaration or introduce a child block to shadow the existing binding".into()),
            });
            // Keep the first binding so further diagnostics remain predictable.
        } else {
            self.insert(&name.text, kind, Some(name.span));
        }
    }
    fn lookup(&self, name: &str) -> Option<SymbolId> {
        let mut scope = Some(self.current);
        while let Some(id) = scope {
            let current = &self.result.scopes[id.0];
            if let Some(symbol) = current.symbols.get(name) {
                return Some(*symbol);
            }
            scope = current.parent;
        }
        None
    }
    fn reference(&mut self, name: &Name) {
        if let Some(id) = self.lookup(&name.text) {
            self.result.references.insert(name.span.start, id);
        } else {
            self.diagnostics.push(Diagnostic {
                code: DiagnosticCode::UnknownName,
                message: format!("unknown identifier `{}`", name.text),
                span: name.span,
                help: Some(
                    "check the spelling or declare this name in an enclosing scope before using it"
                        .into(),
                ),
            });
        }
    }
    fn statements(&mut self, block: &Block) {
        for statement in &block.statements {
            self.statement(statement);
        }
    }
    fn block(&mut self, block: &Block) {
        self.enter(block.span);
        self.statements(block);
        self.leave();
    }
    fn statement(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Variable(variable) => {
                // A binding becomes visible only after its initializer.
                self.expression(&variable.initializer);
                self.declare(&variable.name, SymbolKind::Variable(variable.mutability));
            }
            StatementKind::Expression(expr) => self.expression(expr),
            StatementKind::Return(value) => {
                if let Some(expr) = value {
                    self.expression(expr);
                }
            }
            StatementKind::Block(block) => self.block(block),
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                self.expression(condition);
                self.block(then_block);
                if let Some(branch) = else_branch {
                    self.statement(branch);
                }
            }
            StatementKind::IfLet {
                binding,
                value,
                then_block,
                else_branch,
            } => {
                self.expression(value);
                self.enter(then_block.span);
                self.declare(binding, SymbolKind::Variable(Mutability::Immutable));
                self.statements(then_block);
                self.leave();
                if let Some(branch) = else_branch {
                    self.statement(branch);
                }
            }
            StatementKind::While { condition, body } => {
                // The condition is evaluated in the enclosing scope; the body
                // gets its own child scope, like any other block.
                self.expression(condition);
                self.block(body);
            }
            StatementKind::Loop { body } => self.block(body),
            // Jumps bind to the innermost loop; they introduce no names.
            StatementKind::Break | StatementKind::Continue => {}
        }
    }
    fn expression(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Identifier(name) => self.reference(name),
            ExprKind::Group(inner) | ExprKind::Unary { operand: inner, .. } => {
                self.expression(inner)
            }
            ExprKind::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            ExprKind::Assignment { target, value, .. } => {
                self.expression(target);
                self.expression(value);
            }
            ExprKind::Call { callee, arguments } => {
                self.expression(callee);
                for argument in arguments {
                    self.expression(argument);
                }
            }
            ExprKind::Member { object, .. } => self.expression(object),
            ExprKind::Interpolation(parts) => {
                for part in parts {
                    if let InterpolationPart::Value(value) = part {
                        self.expression(value);
                    }
                }
            }
            // The type name and the field labels are not value names; only the
            // field values are resolved here.
            ExprKind::StructLiteral { fields, .. } | ExprKind::New { fields, .. } => {
                for field in fields {
                    self.expression(&field.value);
                }
            }
            ExprKind::Weak(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            ExprKind::Array(elements) => {
                for element in elements {
                    self.expression(element);
                }
            }
            ExprKind::Index { object, index } => {
                self.expression(object);
                self.expression(index);
            }
        }
    }
}

#[cfg(test)]
mod tests;
