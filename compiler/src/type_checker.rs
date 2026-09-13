//! Semantic checking; produces tables for a separate AST-to-HIR lowering pass.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode},
    resolver::{Builtin, Resolution, SymbolId, SymbolKind},
    span::Span,
    types::{ArrayId, ArrayInfo, OptionId, OptionInfo, StructId, Type},
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
    pub(crate) arrays: Vec<ArrayInfo>,
    pub(crate) options: Vec<OptionInfo>,
}
impl TypedProgram {
    pub fn options(&self) -> &[OptionInfo] {
        &self.options
    }
    pub fn arrays(&self) -> &[ArrayInfo] {
        &self.arrays
    }
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
        arrays: Vec::new(),
        options: Vec::new(),
        option_types: BTreeMap::new(),
        array_types: BTreeMap::new(),
        expected_context: None,
    };
    // Structs are collected before signatures so functions may use them, and
    // before field types so a struct can refer to one declared later.
    for declaration in &syntax.structs {
        let id = StructId(checker.structs.len());
        if declaration.name.text == "Option" {
            checker.error(
                DiagnosticCode::DuplicateDeclaration,
                declaration.name.span,
                "`Option` is a builtin type and cannot be redeclared",
            );
        }
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
            reference: declaration.kind == TypeDeclKind::Reference,
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
            fields.push(FieldInfo {
                name: field.name.text.clone(),
                ty,
                span: field.span,
            });
        }
        checker.structs[index].fields = fields;
    }
    // A value type has no indirection, so containing itself — directly or
    // through other value types — would have no size. A class field is a
    // reference, which breaks any such chain.
    for index in 0..checker.structs.len() {
        if checker.structs[index].reference {
            continue;
        }
        if checker.contains_by_value(
            StructId(index),
            StructId(index),
            &mut vec![false; checker.structs.len()],
        ) {
            let declaration = &syntax.structs[index];
            checker.error(
                DiagnosticCode::InvalidValueType,
                declaration.name.span,
                format!(
                    "struct `{}` contains itself by value; a value type has no indirection, so it would have no size",
                    declaration.name.text
                ),
            );
        }
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
            let mut parameters = Vec::new();
            for parameter in &method.parameters {
                let ty = checker.type_ref(&parameter.type_ref, false);
                parameters.push(ty);
                let param_id = checker.declaration(&parameter.name);
                checker.symbol_types[param_id.0] = ty;
            }
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
            let this = checker
                .resolution
                .declarations
                .get(&method.body.span.start)
                .copied()
                .expect("internal compiler bug: method body has no receiver symbol");
            checker.symbol_types[this.0] = receiver;
            methods.push(MethodInfo {
                name: method.name.text.clone(),
                id,
            });
        }
        checker.structs[index].methods = methods;
    }
    for function in &syntax.functions {
        let id = checker.declaration(&function.name);
        let mut parameters = Vec::new();
        for parameter in &function.parameters {
            let ty = checker.type_ref(&parameter.type_ref, false);
            parameters.push(ty);
            let param_id = checker.declaration(&parameter.name);
            checker.symbol_types[param_id.0] = ty;
        }
        let return_type = function
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
    }
    let entry = syntax
        .functions
        .iter()
        .find(|f| f.name.text == "main")
        .map(|f| checker.declaration(&f.name));
    match entry {
        Some(id) if !checker.signatures[&id].parameters.is_empty() => {
            let function = syntax
                .functions
                .iter()
                .find(|f| f.name.text == "main")
                .unwrap();
            checker.error(
                DiagnosticCode::InvalidEntrypoint,
                function.name.span,
                "`func main()` cannot take parameters",
            );
        }
        Some(id) if checker.signatures[&id].return_type != Type::Void => {
            let function = syntax
                .functions
                .iter()
                .find(|f| f.name.text == "main")
                .unwrap();
            checker.error(
                DiagnosticCode::InvalidEntrypoint,
                function.name.span,
                "`func main()` must return `void`",
            );
        }
        Some(_) => {}
        None => checker.error(
            DiagnosticCode::InvalidEntrypoint,
            Span::new(0, 0),
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
        arrays,
        options,
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
        arrays,
        options,
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
    /// Interned array types; `Type::Array` indexes this.
    arrays: Vec<ArrayInfo>,
    options: Vec<OptionInfo>,
    option_types: BTreeMap<Type, OptionId>,
    array_types: BTreeMap<Type, ArrayId>,
    expected_context: Option<Type>,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    /// A class is a reference to a shared object; a struct is a value.
    pub reference: bool,
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
            Type::Array(id) => format!("[]{}", self.type_name(self.arrays[id.0].element)),
            Type::Option(id) => format!("Option<{}>", self.type_name(self.options[id.0].element)),
            Type::Weak(id) => format!("weak {}", self.structs[id.0].name),
            other => other.to_string(),
        }
    }
    fn option_type(&mut self, element: Type) -> Type {
        if let Some(&id) = self.option_types.get(&element) {
            return Type::Option(id);
        }
        let id = OptionId(self.options.len());
        self.options.push(OptionInfo { element });
        self.option_types.insert(element, id);
        Type::Option(id)
    }
    fn array_type(&mut self, element: Type) -> Type {
        if let Some(&id) = self.array_types.get(&element) {
            Type::Array(id)
        } else {
            let id = ArrayId(self.arrays.len());
            self.arrays.push(ArrayInfo { element });
            self.array_types.insert(element, id);
            Type::Array(id)
        }
    }
    fn type_ref(&mut self, reference: &TypeRef, allow_void: bool) -> Type {
        match reference {
            TypeRef::Option { element, .. } => {
                let element = self.type_ref(element, false);
                if element == Type::Error {
                    Type::Error
                } else {
                    self.option_type(element)
                }
            }
            TypeRef::Weak { class, span } => {
                let ty = self.type_ref(&TypeRef::Named(class.clone()), false);
                match ty {
                    Type::Struct(id) if self.structs[id.0].reference => Type::Weak(id),
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            *span,
                            "weak references require a class type",
                        );
                        Type::Error
                    }
                }
            }
            TypeRef::Named(name) => {
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
            TypeRef::Array { element, span } => {
                let element_type = self.type_ref(element, false);
                if element_type == Type::Error {
                    Type::Error
                } else if element_type == Type::Void {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        *span,
                        "array element type cannot be `void`",
                    );
                    Type::Error
                } else {
                    self.array_type(element_type)
                }
            }
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
                let annotated = variable
                    .type_ref
                    .as_ref()
                    .map(|reference| self.type_ref(reference, false));
                let previous_expected = self.expected_context;
                self.expected_context = annotated;
                let inferred = self.expression(&variable.initializer);
                self.expected_context = previous_expected;
                if inferred == Type::Void {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        variable.initializer.span,
                        "cannot store a `void` expression in a variable",
                    );
                }
                let ty = if let Some(annotated) = annotated {
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
                let previous_expected = self.expected_context;
                self.expected_context = Some(self.return_type);
                let found = value
                    .as_ref()
                    .map(|e| self.expression(e))
                    .unwrap_or(Type::Void);
                self.expected_context = previous_expected;
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
            StatementKind::IfLet {
                binding,
                value,
                then_block,
                else_branch,
            } => {
                let ty = self.expression(value);
                let payload = match ty {
                    Type::Option(id) => self.options[id.0].element,
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            value.span,
                            "if let Some(...) requires an Option value",
                        );
                        Type::Error
                    }
                };
                let id = self.declaration(binding);
                self.symbol_types[id.0] = payload;
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
    fn construction(&mut self, name: &Name, fields: &[FieldInit], new: bool) -> Type {
        let Some(id) = self.struct_names.get(&name.text).copied() else {
            self.error(
                DiagnosticCode::UnknownType,
                name.span,
                format!(
                    "unknown {} `{}`",
                    if new { "class" } else { "struct" },
                    name.text
                ),
            );
            // Still check the values so their own errors are reported.
            for field in fields {
                self.expression(&field.value);
            }
            return Type::Error;
        };
        if self.structs[id.0].reference != new {
            let (found, expected) = if new {
                ("struct", format!("`{} {{ ... }}`", name.text))
            } else {
                ("class", format!("`new {}(...)`", name.text))
            };
            self.error(
                DiagnosticCode::InvalidAssignment,
                name.span,
                format!("`{}` is a {found}; construct it with {expected}", name.text),
            );
        }
        let mut initialized = vec![false; self.structs[id.0].fields.len()];
        for field in fields {
            let declared_field = self.structs[id.0]
                .field(&field.name.text)
                .map(|(index, declared)| (index, declared.clone()));
            let previous = self.expected_context;
            self.expected_context = declared_field.as_ref().map(|(_, d)| d.ty);
            let found = self.expression(&field.value);
            self.expected_context = previous;
            let Some((index, declared)) = declared_field else {
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
        let expected = self.expected_context.take();
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
                    SymbolKind::Builtin(Builtin::None) => match expected {
                        Some(ty @ Type::Option(_)) => ty,
                        _ => {
                            self.error(
                                DiagnosticCode::UnknownType,
                                expr.span,
                                "None requires an expected Option type; add a type annotation",
                            );
                            Type::Error
                        }
                    },
                    _ => {
                        self.error(DiagnosticCode::UnsupportedFeature, expr.span, "functions can only be used as direct call targets; function values are not supported");
                        Type::Error
                    }
                }
            }
            ExprKind::Group(inner) => {
                self.expected_context = expected;
                self.expression(inner)
            }
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
                if assignment_root(target).is_none()
                    && !self.through_reference(target)
                    && target_type != Type::Error
                {
                    self.error(DiagnosticCode::InvalidAssignment, target.span, "assignment requires a variable or a field/index reached through a reference");
                }
                // Assigning to `v.x` needs the mutability of `v`: a field of an
                // immutable binding is immutable too. A class is different: the
                // binding holds a reference, and the object it refers to is
                // shared and mutable, so only rebinding the reference is
                // governed by `let`.
                if let Some(name) =
                    assignment_root(target).filter(|_| !self.through_reference(target))
                {
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
                let previous_expected = self.expected_context;
                self.expected_context = Some(target_type);
                let value_type = self.expression(value);
                self.expected_context = previous_expected;
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
            ExprKind::Call { callee, arguments } => {
                self.call(callee, arguments, expr.span, expected)
            }
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
            ExprKind::Weak(value) => {
                match value {
                    Some(value) => {
                        let ty = self.expression(value);
                        match ty {
                            Type::Struct(id) if self.structs[id.0].reference => Type::Weak(id),
                            Type::Error => Type::Error,
                            _ => {
                                self.error(
                                    DiagnosticCode::InvalidValueType,
                                    value.span,
                                    "weak() requires a class reference",
                                );
                                Type::Error
                            }
                        }
                    }
                    None => match expected {
                        Some(ty @ Type::Weak(_)) => ty,
                        _ => {
                            self.error(DiagnosticCode::InvalidValueType, expr.span, "empty weak() requires a weak class type annotation or expected type");
                            Type::Error
                        }
                    },
                }
            }
            ExprKind::Array(elements) => {
                let previous_expected = self.expected_context;
                let expected_elem = match expected {
                    Some(Type::Array(id)) => Some(self.arrays[id.0].element),
                    _ => None,
                };
                if elements.is_empty() {
                    match expected_elem {
                        Some(elem) => self.array_type(elem),
                        None => {
                            self.error(
                                DiagnosticCode::UnknownType,
                                expr.span,
                                "cannot infer element type for empty array literal; explicit type annotation required",
                            );
                            Type::Error
                        }
                    }
                } else {
                    let mut elem_ty = expected_elem;
                    let mut has_error = false;
                    for elem in elements {
                        self.expected_context = elem_ty;
                        let found = self.expression(elem);
                        self.expected_context = previous_expected;
                        if found == Type::Error {
                            has_error = true;
                        } else if found == Type::Void {
                            self.error(
                                DiagnosticCode::InvalidValueType,
                                elem.span,
                                "array element cannot be `void`",
                            );
                            has_error = true;
                        } else {
                            match elem_ty {
                                Some(expected) => {
                                    self.expect_type(expected, found, elem.span);
                                    if expected != found {
                                        has_error = true;
                                    }
                                }
                                None => {
                                    elem_ty = Some(found);
                                }
                            }
                        }
                    }
                    if has_error {
                        Type::Error
                    } else {
                        match elem_ty {
                            Some(elem) => self.array_type(elem),
                            None => Type::Error,
                        }
                    }
                }
            }
            ExprKind::Index { object, index } => {
                let object_type = self.expression(object);
                let previous_expected = self.expected_context;
                self.expected_context = Some(Type::Int);
                let index_type = self.expression(index);
                self.expected_context = previous_expected;
                if index_type != Type::Error {
                    self.expect_type(Type::Int, index_type, index.span);
                }
                match object_type {
                    Type::Array(id) => {
                        if index_type == Type::Error || index_type != Type::Int {
                            Type::Error
                        } else {
                            self.arrays[id.0].element
                        }
                    }
                    Type::Error => Type::Error,
                    other => {
                        self.error(
                            DiagnosticCode::InvalidOperator,
                            expr.span,
                            format!(
                                "cannot index into `{}`; only arrays support indexing",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::StructLiteral { name, fields } => self.construction(name, fields, false),
            ExprKind::New { name, fields } => self.construction(name, fields, true),
            ExprKind::Interpolation(parts) => {
                for part in parts {
                    let InterpolationPart::Value(value) = part else {
                        continue;
                    };
                    let ty = self.expression(value);
                    // The same set `print` accepts: anything with an obvious
                    // textual form, and no implicit conversion beyond that.
                    if !matches!(
                        ty,
                        Type::Int | Type::Float | Type::Bool | Type::String | Type::Error
                    ) {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            value.span,
                            format!(
                                "cannot interpolate `{}`; only int, float, bool and string have a textual form",
                                self.type_name(ty)
                            ),
                        );
                    }
                }
                Type::String
            }
        };
        self.expected_context = expected;
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
        let builtin = match (receiver, member.text.as_str()) {
            (Type::Array(_), "len") => Some(Type::Int),
            (Type::Weak(_), "alive") => Some(Type::Bool),
            (Type::Weak(id), "get") => Some(Type::Struct(id)),
            (Type::Weak(id), "upgrade") => Some(self.option_type(Type::Struct(id))),
            (Type::Option(_), "is_some" | "is_none") => Some(Type::Bool),
            _ => None,
        };
        if let Some(ty) = builtin {
            for argument in arguments {
                self.expression(argument);
            }
            if !arguments.is_empty() {
                self.error(
                    DiagnosticCode::ArgumentCount,
                    span,
                    format!(
                        "method `{}` expects 0 arguments, found {}",
                        member.text,
                        arguments.len()
                    ),
                );
            }
            return ty;
        }
        let Type::Struct(id) = receiver else {
            if receiver != Type::Error {
                self.error(
                    DiagnosticCode::UnsupportedFeature,
                    member.span,
                    format!(
                        "`{}` has no method `{}`",
                        self.type_name(receiver),
                        member.text
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
            for argument in arguments {
                self.expression(argument);
            }
            return signature.return_type;
        }
        let previous = self.expected_context;
        for (expected, argument) in signature.parameters.iter().zip(arguments) {
            self.expected_context = Some(*expected);
            let found = self.expression(argument);
            self.expect_type(*expected, found, argument.span);
        }
        self.expected_context = previous;
        signature.return_type
    }
    fn call(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
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
        match id.map(|id| (id, self.resolution.symbols[id.0].kind)) {
            Some((_, SymbolKind::Builtin(Builtin::Some))) => {
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        "Some expects exactly one argument",
                    );
                    return Type::Error;
                }
                self.expected_context = match expected {
                    Some(Type::Option(id)) => Some(self.options[id.0].element),
                    _ => None,
                };
                let element = self.expression(&arguments[0]);
                self.expected_context = None;
                if element == Type::Void {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        arguments[0].span,
                        "Some cannot contain void",
                    );
                    Type::Error
                } else if element == Type::Error {
                    Type::Error
                } else {
                    self.option_type(element)
                }
            }
            Some((_, SymbolKind::Builtin(Builtin::None))) => {
                for argument in arguments {
                    self.expression(argument);
                }
                self.error(
                    DiagnosticCode::NotCallable,
                    callee.span,
                    "None is a value; use None without parentheses",
                );
                Type::Error
            }
            Some((_, SymbolKind::Builtin(Builtin::Print))) => {
                let arg_types: Vec<_> = arguments.iter().map(|arg| self.expression(arg)).collect();
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
                    } else if !matches!(
                        ty,
                        Type::Int | Type::Float | Type::Bool | Type::String | Type::Error
                    ) {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            arg.span,
                            format!(
                                "`print` cannot print `{}`; only int, float, bool and string have a textual form",
                                self.type_name(ty)
                            ),
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
                let previous = self.expected_context;
                for (arg, expected) in arguments.iter().zip(&signature.parameters) {
                    self.expected_context = Some(*expected);
                    let found = self.expression(arg);
                    self.expect_type(*expected, found, arg.span);
                }
                self.expected_context = previous;
                for arg in arguments.iter().skip(signature.parameters.len()) {
                    self.expression(arg);
                }
                signature.return_type
            }
            _ => {
                for arg in arguments {
                    self.expression(arg);
                }
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

impl Checker<'_> {
    /// Whether `target` is reachable from `from` through value-typed fields.
    fn contains_by_value(&self, from: StructId, target: StructId, seen: &mut Vec<bool>) -> bool {
        if seen[from.0] {
            return false;
        }
        seen[from.0] = true;
        self.structs[from.0].fields.iter().any(|field| {
            let mut ty = field.ty;
            while let Type::Option(id) = ty {
                ty = self.options[id.0].element;
            }
            let Type::Struct(id) = ty else {
                return false;
            };
            // A reference field stops the chain: it is a pointer.
            !self.structs[id.0].reference
                && (id == target || self.contains_by_value(id, target, seen))
        })
    }
    /// True when the assignment writes into an object reached by reference.
    fn through_reference(&self, target: &Expr) -> bool {
        match &target.kind {
            ExprKind::Member { object, .. } => {
                let ty = self
                    .expressions
                    .get(&(object.span.start, object.span.end))
                    .copied();
                matches!(ty, Some(Type::Struct(id)) if self.structs[id.0].reference)
                    || self.through_reference(object)
            }
            ExprKind::Index { object, .. } => {
                let ty = self
                    .expressions
                    .get(&(object.span.start, object.span.end))
                    .copied();
                matches!(ty, Some(Type::Array(_))) || self.through_reference(object)
            }
            ExprKind::Group(inner) => self.through_reference(inner),
            _ => false,
        }
    }
}

/// The local an assignment ultimately writes through, looking past field steps.
fn assignment_root(target: &Expr) -> Option<&Name> {
    match &target.kind {
        ExprKind::Identifier(name) => Some(name),
        ExprKind::Group(inner) => assignment_root(inner),
        ExprKind::Member { object, .. } => assignment_root(object),
        ExprKind::Index { object, .. } => assignment_root(object),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
