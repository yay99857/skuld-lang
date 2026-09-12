//! Semantic checking; produces tables for a separate AST-to-HIR lowering pass.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode},
    resolver::{Resolution, SymbolId, SymbolKind},
    span::Span,
    types::{StructId, Type},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Signature {
    pub parameters: Vec<Type>,
    pub return_type: Type,
}

/// Only successful checking can construct this object. Tables and syntax must
/// stay together; HIR lowering consumes them without repeating name lookup.
#[derive(Debug)]
pub struct TypedProgram {
    pub(crate) syntax: Program,
    pub(crate) resolution: Resolution,
    pub(crate) expressions: BTreeMap<(usize, usize), Type>,
    pub(crate) symbol_types: Vec<Type>,
    pub(crate) signatures: BTreeMap<SymbolId, Signature>,
    pub(crate) entry: SymbolId,
    pub(crate) structs: Vec<StructInfo>,
}
impl TypedProgram {
    pub fn syntax(&self) -> &Program {
        &self.syntax
    }
    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }
    pub fn expression_type(&self, span: Span) -> Option<Type> {
        self.expressions.get(&(span.start, span.end)).copied()
    }
}

/// The resolution must belong to this exact parser AST. All source-facing
/// callers should use `check`, which enforces phase ordering and ownership.
pub(crate) fn type_check(
    syntax: Program,
    resolution: Resolution,
) -> Result<TypedProgram, Vec<Diagnostic>> {
    let mut checker = Checker {
        symbol_types: vec![Type::Error; resolution.symbols.len()],
        resolution: &resolution,
        expressions: BTreeMap::new(),
        signatures: BTreeMap::new(),
        diagnostics: Vec::new(),
        return_type: Type::Void,
        loops: Vec::new(),
        structs: Vec::new(),
        struct_names: BTreeMap::new(),
    };
    // Structs are collected before signatures so functions may use them, and
    // before field types so a struct can refer to one declared later.
    for declaration in &syntax.structs {
        let id = StructId(checker.structs.len());
        if checker
            .struct_names
            .insert(declaration.name.text.clone(), id)
            .is_some()
        {
            checker.error(
                DiagnosticCode::DuplicateDeclaration,
                declaration.name.span,
                format!("struct `{}` is already declared", declaration.name.text),
            );
        }
        checker.structs.push(StructInfo {
            name: declaration.name.text.clone(),
            fields: Vec::new(),
            methods: Vec::new(),
            span: declaration.span,
        });
    }
    for (index, declaration) in syntax.structs.iter().enumerate() {
        let mut fields: Vec<FieldInfo> = Vec::new();
        for field in &declaration.fields {
            let ty = checker.type_ref(&field.type_ref, false);
            if fields
                .iter()
                .any(|existing| existing.name == field.name.text)
            {
                checker.error(
                    DiagnosticCode::DuplicateDeclaration,
                    field.name.span,
                    format!(
                        "field `{}` is already declared in `{}`",
                        field.name.text, declaration.name.text
                    ),
                );
                continue;
            }
            if ty == Type::Struct(StructId(index)) {
                checker.error(
                    DiagnosticCode::InvalidValueType,
                    field.type_ref_span(),
                    format!(
                        "struct `{}` cannot contain itself; a value type has no indirection",
                        declaration.name.text
                    ),
                );
                continue;
            }
            fields.push(FieldInfo {
                name: field.name.text.clone(),
                ty,
                span: field.span,
            });
        }
        checker.structs[index].fields = fields;
    }
    // Method signatures come after fields so a method can use any field type,
    // and after every struct exists so signatures may mention other structs.
    for (index, declaration) in syntax.structs.iter().enumerate() {
        let receiver = Type::Struct(StructId(index));
        let mut methods: Vec<MethodInfo> = Vec::new();
        for method in &declaration.methods {
            let id = checker.declaration(&method.name);
            if methods
                .iter()
                .any(|existing| existing.name == method.name.text)
            {
                checker.error(
                    DiagnosticCode::DuplicateDeclaration,
                    method.name.span,
                    format!(
                        "method `{}` is already declared in `{}`",
                        method.name.text, declaration.name.text
                    ),
                );
            }
            if checker.structs[index]
                .fields
                .iter()
                .any(|field| field.name == method.name.text)
            {
                checker.error(
                    DiagnosticCode::DuplicateDeclaration,
                    method.name.span,
                    format!(
                        "`{}` already has a field named `{}`",
                        declaration.name.text, method.name.text
                    ),
                );
            }
            let parameters: Vec<_> = method
                .parameters
                .iter()
                .map(|p| {
                    let ty = checker.type_ref(&p.type_ref, false);
                    let parameter = checker.declaration(&p.name);
                    checker.symbol_types[parameter.0] = ty;
                    ty
                })
                .collect();
            let return_type = method
                .return_type
                .as_ref()
                .map(|r| checker.type_ref(r, true))
                .unwrap_or(Type::Void);
            checker.signatures.insert(
                id,
                Signature {
                    parameters,
                    return_type,
                },
            );
            // `this` is an immutable parameter, like every other parameter.
            let this = checker.resolution.declarations[&method.body.span.start];
            checker.symbol_types[this.0] = receiver;
            methods.push(MethodInfo {
                name: method.name.text.clone(),
                id,
            });
        }
        checker.structs[index].methods = methods;
    }
    for function in &syntax.functions {
        let parameters: Vec<_> = function
            .parameters
            .iter()
            .map(|p| {
                let ty = checker.type_ref(&p.type_ref, false);
                let id = checker.declaration(&p.name);
                checker.symbol_types[id.0] = ty;
                ty
            })
            .collect();
        let return_type = function
            .return_type
            .as_ref()
            .map(|t| checker.type_ref(t, true))
            .unwrap_or(Type::Void);
        let id = checker.declaration(&function.name);
        checker.signatures.insert(
            id,
            Signature {
                parameters,
                return_type,
            },
        );
    }
    let entry = syntax
        .functions
        .iter()
        .find(|f| f.name.text == "main")
        .map(|f| checker.declaration(&f.name));
    match entry {
        Some(id) => {
            let signature = &checker.signatures[&id];
            if !signature.parameters.is_empty() || signature.return_type != Type::Void {
                checker.error(
                    DiagnosticCode::InvalidEntrypoint,
                    resolution.symbols[id.0].span.unwrap_or(syntax.span),
                    "entrypoint must have signature `func main()` (void return, no parameters)",
                );
            }
        }
        None => checker.error(
            DiagnosticCode::InvalidEntrypoint,
            Span::new(syntax.span.end, syntax.span.end),
            "missing entrypoint `func main()`",
        ),
    }
    for function in syntax
        .structs
        .iter()
        .flat_map(|declaration| declaration.methods.iter())
        .chain(syntax.functions.iter())
    {
        checker.return_type = checker.signatures[&checker.declaration(&function.name)].return_type;
        let returns = checker.block(&function.body);
        if checker.return_type != Type::Void && checker.return_type != Type::Error && !returns {
            checker.error(
                DiagnosticCode::MissingReturn,
                function.name.span,
                format!(
                    "function `{}` must return `{}` on every path",
                    function.name.text, checker.return_type
                ),
            );
        }
    }
    if !checker.diagnostics.is_empty() {
        return Err(checker.diagnostics);
    }
    let Checker {
        structs,
        expressions,
        symbol_types,
        signatures,
        ..
    } = checker;
    // A missing entry always produces a diagnostic above.
    let entry = entry.expect("internal compiler bug: checked program has no entrypoint");
    Ok(TypedProgram {
        syntax,
        resolution,
        structs,
        expressions,
        symbol_types,
        signatures,
        entry,
    })
}

struct Checker<'a> {
    resolution: &'a Resolution,
    symbol_types: Vec<Type>,
    expressions: BTreeMap<(usize, usize), Type>,
    signatures: BTreeMap<SymbolId, Signature>,
    diagnostics: Vec<Diagnostic>,
    return_type: Type,
    /// One frame per enclosing loop, recording whether a `break` can exit it.
    /// Empty means a jump has no loop to bind to.
    loops: Vec<bool>,
    /// Declared structs in declaration order; `Type::Struct` indexes this.
    structs: Vec<StructInfo>,
    /// Struct name to table index, for resolving type names and constructions.
    struct_names: BTreeMap<String, StructId>,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    /// Field order is declaration order, which the backend layout follows.
    pub fields: Vec<FieldInfo>,
    pub methods: Vec<MethodInfo>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub name: String,
    /// Methods are ordinary functions with an implicit leading receiver.
    pub id: SymbolId,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

impl StructInfo {
    fn field(&self, name: &str) -> Option<(usize, &FieldInfo)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == name)
    }
}
impl Checker<'_> {
    fn declaration(&self, name: &Name) -> SymbolId {
        self.resolution.declarations[&name.span.start]
    }
    fn reference(&self, name: &Name) -> SymbolId {
        self.resolution.references[&name.span.start]
    }
    fn error(&mut self, code: DiagnosticCode, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            code,
            message: message.into(),
            span,
            help: None,
        });
    }
    /// `Display` cannot reach the struct table, so every user-facing type name
    /// goes through here instead.
    fn type_name(&self, ty: Type) -> String {
        match ty {
            Type::Struct(id) => self.structs[id.0].name.clone(),
            other => other.to_string(),
        }
    }
    fn type_ref(&mut self, reference: &TypeRef, allow_void: bool) -> Type {
        let TypeRef::Named(name) = reference;
        let ty = match name.text.as_str() {
            "int" => Type::Int,
            "float" => Type::Float,
            "bool" => Type::Bool,
            "string" => Type::String,
            "void" => Type::Void,
            other if self.struct_names.contains_key(other) => {
                Type::Struct(self.struct_names[other])
            }
            _ => {
                self.error(
                    DiagnosticCode::UnknownType,
                    name.span,
                    format!("unknown or unsupported type `{}`", name.text),
                );
                Type::Error
            }
        };
        if ty == Type::Void && !allow_void {
            self.error(
                DiagnosticCode::InvalidValueType,
                name.span,
                "`void` is only allowed as a function return type",
            );
            Type::Error
        } else {
            ty
        }
    }
    fn expect_type(&mut self, expected: Type, found: Type, span: Span) {
        if expected != Type::Error && found != Type::Error && expected != found {
            self.error(
                DiagnosticCode::TypeMismatch,
                span,
                format!(
                    "expected `{}`, found `{}`",
                    self.type_name(expected),
                    self.type_name(found)
                ),
            );
        }
    }
    fn block(&mut self, block: &Block) -> bool {
        let mut returns = false;
        for statement in &block.statements {
            returns |= self.statement(statement);
        }
        returns
    }
    fn statement(&mut self, statement: &Statement) -> bool {
        match &statement.kind {
            StatementKind::Variable(variable) => {
                let inferred = self.expression(&variable.initializer);
                if inferred == Type::Void {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        variable.initializer.span,
                        "cannot store a `void` expression in a variable",
                    );
                }
                let ty = if let Some(reference) = &variable.type_ref {
                    let annotated = self.type_ref(reference, false);
                    self.expect_type(annotated, inferred, variable.initializer.span);
                    annotated
                } else {
                    inferred
                };
                let id = self.declaration(&variable.name);
                self.symbol_types[id.0] = ty;
                false
            }
            StatementKind::Expression(expr) => {
                self.expression(expr);
                false
            }
            StatementKind::Return(value) => {
                let found = value
                    .as_ref()
                    .map(|e| self.expression(e))
                    .unwrap_or(Type::Void);
                self.expect_type(
                    self.return_type,
                    found,
                    value.as_ref().map(|e| e.span).unwrap_or(statement.span),
                );
                true
            }
            StatementKind::Block(block) => self.block(block),
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                let ty = self.expression(condition);
                self.expect_type(Type::Bool, ty, condition.span);
                let then_returns = self.block(then_block);
                let else_returns = else_branch
                    .as_ref()
                    .is_some_and(|branch| self.statement(branch));
                then_returns && else_returns
            }
            StatementKind::While { condition, body } => {
                let ty = self.expression(condition);
                self.expect_type(Type::Bool, ty, condition.span);
                self.loops.push(false);
                self.block(body);
                self.loops.pop();
                // The condition may be false on entry, so a `while` never
                // guarantees that its body runs, let alone that it returns.
                false
            }
            StatementKind::Loop { body } => {
                self.loops.push(false);
                self.block(body);
                let escapes = self.loops.pop().unwrap_or(true);
                // A loop nobody breaks out of never falls through, so the code
                // after it is unreachable and the function needs no further
                // return. `break` reintroduces the fall-through path.
                !escapes
            }
            StatementKind::Break | StatementKind::Continue => {
                let keyword = if matches!(statement.kind, StatementKind::Break) {
                    "break"
                } else {
                    "continue"
                };
                match self.loops.last_mut() {
                    Some(escapes) => {
                        if keyword == "break" {
                            *escapes = true;
                        }
                    }
                    None => self.error(
                        DiagnosticCode::JumpOutsideLoop,
                        statement.span,
                        format!("`{keyword}` is only valid inside a loop"),
                    ),
                }
                false
            }
        }
    }
    fn struct_literal(&mut self, name: &Name, fields: &[FieldInit]) -> Type {
        let Some(id) = self.struct_names.get(&name.text).copied() else {
            self.error(
                DiagnosticCode::UnknownType,
                name.span,
                format!("unknown struct `{}`", name.text),
            );
            // Still check the values so their own errors are reported.
            for field in fields {
                self.expression(&field.value);
            }
            return Type::Error;
        };
        let mut initialized = vec![false; self.structs[id.0].fields.len()];
        for field in fields {
            let found = self.expression(&field.value);
            let Some((index, declared)) = self.structs[id.0]
                .field(&field.name.text)
                .map(|(index, declared)| (index, declared.clone()))
            else {
                self.error(
                    DiagnosticCode::UnknownName,
                    field.name.span,
                    format!(
                        "`{}` has no field `{}`",
                        self.structs[id.0].name, field.name.text
                    ),
                );
                continue;
            };
            if initialized[index] {
                self.error(
                    DiagnosticCode::DuplicateDeclaration,
                    field.name.span,
                    format!("field `{}` is initialized twice", field.name.text),
                );
            }
            initialized[index] = true;
            self.expect_type(declared.ty, found, field.value.span);
        }
        // Every field must be given a value: there are no defaults and no
        // partially initialized values.
        let missing: Vec<_> = self.structs[id.0]
            .fields
            .iter()
            .zip(&initialized)
            .filter(|(_, done)| !**done)
            .map(|(field, _)| format!("`{}`", field.name))
            .collect();
        if !missing.is_empty() {
            self.error(
                DiagnosticCode::MissingField,
                name.span,
                format!(
                    "`{}` is missing {}",
                    self.structs[id.0].name,
                    missing.join(", ")
                ),
            );
        }
        Type::Struct(id)
    }
    fn record(&mut self, expr: &Expr, ty: Type) -> Type {
        self.expressions
            .insert((expr.span.start, expr.span.end), ty);
        ty
    }
    // Only unary minus may consume the positive magnitude of i64::MIN.
    fn minimum_magnitude(&mut self, expr: &Expr) -> bool {
        let is_min = match &expr.kind {
            ExprKind::Literal(Literal::Integer(value)) => *value == (1_u64 << 63),
            ExprKind::Group(inner) => self.minimum_magnitude(inner),
            _ => false,
        };
        if is_min {
            self.record(expr, Type::Int);
        }
        is_min
    }
    fn expression(&mut self, expr: &Expr) -> Type {
        let ty = match &expr.kind {
            ExprKind::Literal(literal) => match literal {
                Literal::Integer(value) => {
                    if *value > i64::MAX as u64 {
                        self.error(
                            DiagnosticCode::IntegerRange,
                            expr.span,
                            "integer literal is outside the signed 64-bit `int` range",
                        );
                        Type::Error
                    } else {
                        Type::Int
                    }
                }
                Literal::Float(_) => Type::Float,
                Literal::Boolean(_) => Type::Bool,
                Literal::String(_) => Type::String,
                Literal::Char(_) => {
                    self.error(
                        DiagnosticCode::UnsupportedFeature,
                        expr.span,
                        "char values are not supported in this milestone",
                    );
                    Type::Error
                }
            },
            ExprKind::Identifier(name) => {
                let id = self.reference(name);
                match self.resolution.symbols[id.0].kind {
                    SymbolKind::Variable(_) | SymbolKind::Parameter => self.symbol_types[id.0],
                    _ => {
                        self.error(DiagnosticCode::UnsupportedFeature, expr.span, "functions can only be used as direct call targets; function values are not supported");
                        Type::Error
                    }
                }
            }
            ExprKind::Group(inner) => self.expression(inner),
            ExprKind::Unary {
                op,
                operand,
                op_span,
            } => {
                if *op == UnaryOp::Negative && self.minimum_magnitude(operand) {
                    Type::Int
                } else {
                    let ty = self.expression(operand);
                    let valid = if *op == UnaryOp::Not {
                        ty == Type::Bool
                    } else {
                        ty.is_numeric()
                    };
                    if !valid && ty != Type::Error {
                        self.error(
                            DiagnosticCode::InvalidOperator,
                            *op_span,
                            format!("unary operator `{op:?}` does not accept `{ty}`"),
                        );
                        Type::Error
                    } else {
                        ty
                    }
                }
            }
            ExprKind::Binary {
                left,
                op,
                right,
                op_span,
            } => {
                let left = self.expression(left);
                let right = self.expression(right);
                self.binary(*op, left, right, *op_span)
            }
            ExprKind::Assignment {
                target,
                op,
                value,
                op_span,
            } => {
                let target_type = self.expression(target);
                // Assigning to `v.x` needs the mutability of `v`: a field of an
                // immutable binding is immutable too.
                if let Some(name) = assignment_root(target) {
                    let id = self.reference(name);
                    let symbol = &self.resolution.symbols[id.0];
                    match symbol.kind {
                        SymbolKind::Variable(Mutability::Mutable) => {}
                        SymbolKind::Variable(Mutability::Immutable) | SymbolKind::Parameter => {
                            let mut diagnostic = Diagnostic {
                                code: DiagnosticCode::ImmutableAssignment,
                                span: name.span,
                                message: format!(
                                    "cannot assign to immutable variable `{}`",
                                    name.text
                                ),
                                help: None,
                            };
                            diagnostic.help = Some(if symbol.kind == SymbolKind::Parameter {
                                "parameters are immutable; copy the value into a local `var`".into()
                            } else {
                                "declare the variable with `var` to allow assignment".into()
                            });
                            self.diagnostics.push(diagnostic);
                        }
                        _ => self.error(
                            DiagnosticCode::InvalidAssignment,
                            name.span,
                            "cannot assign to a function or builtin",
                        ),
                    }
                }
                let value_type = self.expression(value);
                self.expect_type(target_type, value_type, value.span);
                if *op != AssignmentOp::Assign {
                    let binary = match op {
                        AssignmentOp::Add => BinaryOp::Add,
                        AssignmentOp::Subtract => BinaryOp::Subtract,
                        AssignmentOp::Multiply => BinaryOp::Multiply,
                        AssignmentOp::Divide => BinaryOp::Divide,
                        AssignmentOp::Assign => unreachable!(),
                    };
                    self.binary(binary, target_type, value_type, *op_span);
                }
                target_type
            }
            ExprKind::Call { callee, arguments } => self.call(callee, arguments, expr.span),
            ExprKind::Member { object, member } => {
                let object_type = self.expression(object);
                match object_type {
                    Type::Struct(id) => match self.structs[id.0].field(&member.text) {
                        Some((_, field)) => field.ty,
                        None => {
                            let is_method = self.structs[id.0]
                                .methods
                                .iter()
                                .any(|method| method.name == member.text);
                            let mut diagnostic = Diagnostic {
                                code: DiagnosticCode::UnknownName,
                                span: member.span,
                                message: if is_method {
                                    format!("`{}` is a method, not a field", member.text)
                                } else {
                                    format!(
                                        "`{}` has no field `{}`",
                                        self.structs[id.0].name, member.text
                                    )
                                },
                                help: None,
                            };
                            if is_method {
                                diagnostic.help =
                                    Some("call it with `()`; methods are not values".into());
                            }
                            self.diagnostics.push(diagnostic);
                            Type::Error
                        }
                    },
                    // A failed subexpression already reported the reason.
                    Type::Error => Type::Error,
                    other => {
                        self.error(
                            DiagnosticCode::UnsupportedFeature,
                            member.span,
                            format!(
                                "`{}` has no fields; only structs support field access",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::StructLiteral { name, fields } => self.struct_literal(name, fields),
        };
        self.record(expr, ty)
    }
    fn binary(&mut self, op: BinaryOp, left: Type, right: Type, span: Span) -> Type {
        if left == Type::Error || right == Type::Error {
            return Type::Error;
        }
        if left != right {
            self.expect_type(left, right, span);
            return Type::Error;
        }
        use BinaryOp::*;
        let valid = match op {
            // `+` also concatenates; the result is a new string.
            Add => left.is_numeric() || left == Type::String,
            Subtract | Multiply | Divide | Less | Greater | LessEqual | GreaterEqual => {
                left.is_numeric()
            }
            Modulo => left == Type::Int,
            And | Or => left == Type::Bool,
            Equal | NotEqual => matches!(left, Type::Int | Type::Float | Type::Bool | Type::String),
        };
        if !valid {
            self.error(
                DiagnosticCode::InvalidOperator,
                span,
                format!("operator `{op:?}` does not accept `{left}` operands"),
            );
            return Type::Error;
        }
        match op {
            And | Or | Equal | NotEqual | Less | Greater | LessEqual | GreaterEqual => Type::Bool,
            _ => left,
        }
    }
    fn method_call(
        &mut self,
        object: &Expr,
        member: &Name,
        arguments: &[Expr],
        span: Span,
    ) -> Type {
        let receiver = self.expression(object);
        let arg_types: Vec<_> = arguments.iter().map(|arg| self.expression(arg)).collect();
        let Type::Struct(id) = receiver else {
            if receiver != Type::Error {
                self.error(
                    DiagnosticCode::UnsupportedFeature,
                    member.span,
                    format!(
                        "`{}` has no methods; only structs support method calls",
                        self.type_name(receiver)
                    ),
                );
            }
            return Type::Error;
        };
        let Some(method) = self.structs[id.0]
            .methods
            .iter()
            .find(|method| method.name == member.text)
            .cloned()
        else {
            let is_field = self.structs[id.0].field(&member.text).is_some();
            self.error(
                DiagnosticCode::NotCallable,
                member.span,
                if is_field {
                    format!("field `{}` is not callable", member.text)
                } else {
                    format!(
                        "`{}` has no method `{}`",
                        self.structs[id.0].name, member.text
                    )
                },
            );
            return Type::Error;
        };
        let signature = self.signatures[&method.id].clone();
        if signature.parameters.len() != arguments.len() {
            self.error(
                DiagnosticCode::ArgumentCount,
                span,
                format!(
                    "method `{}` expects {} arguments, found {}",
                    member.text,
                    signature.parameters.len(),
                    arguments.len()
                ),
            );
            return signature.return_type;
        }
        for ((expected, found), argument) in
            signature.parameters.iter().zip(arg_types).zip(arguments)
        {
            self.expect_type(*expected, found, argument.span);
        }
        signature.return_type
    }
    fn call(&mut self, callee: &Expr, arguments: &[Expr], span: Span) -> Type {
        let mut direct = callee;
        while let ExprKind::Group(inner) = &direct.kind {
            direct = inner;
        }
        if let ExprKind::Member { object, member } = &direct.kind {
            return self.method_call(object, member, arguments, span);
        }
        let id = if let ExprKind::Identifier(name) = &direct.kind {
            Some(self.reference(name))
        } else {
            None
        };
        let arg_types: Vec<_> = arguments.iter().map(|arg| self.expression(arg)).collect();
        match id.map(|id| (id, self.resolution.symbols[id.0].kind)) {
            Some((_, SymbolKind::Builtin(_))) => {
                if arguments.len() > 1 {
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        format!(
                            "`print` expects 0 or 1 arguments, found {}",
                            arguments.len()
                        ),
                    );
                }
                for (arg, ty) in arguments.iter().zip(arg_types) {
                    if ty == Type::Void {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            arg.span,
                            "`print` cannot print `void`",
                        );
                    }
                }
                Type::Void
            }
            Some((id, SymbolKind::Function)) => {
                let signature = self.signatures[&id].clone();
                if arguments.len() != signature.parameters.len() {
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        format!(
                            "function expects {} arguments, found {}",
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    );
                }
                for ((arg, found), expected) in
                    arguments.iter().zip(arg_types).zip(signature.parameters)
                {
                    self.expect_type(expected, found, arg.span);
                }
                signature.return_type
            }
            _ => {
                let ty = self.expression(callee);
                if ty != Type::Error {
                    self.error(
                        DiagnosticCode::NotCallable,
                        callee.span,
                        format!("value of type `{ty}` is not callable"),
                    );
                }
                Type::Error
            }
        }
    }
}

/// The local an assignment ultimately writes through, looking past field steps.
fn assignment_root(target: &Expr) -> Option<&Name> {
    match &target.kind {
        ExprKind::Identifier(name) => Some(name),
        ExprKind::Member { object, .. } => assignment_root(object),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
