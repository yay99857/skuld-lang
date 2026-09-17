//! AST-to-HIR lowering after resolution and checking, with no code generation.
use crate::{
    ast, hir as h,
    module::FileId,
    resolver::{Builtin, SymbolId, SymbolKind},
    span::Span,
    type_checker::TypedProgram,
    types::{ConstValue, EnumId, Type},
};
use std::cell::RefCell;

/// The checked program, plus the file being walked. Byte offsets repeat
/// across files, so every table lookup needs both.
struct Lowering<'a> {
    typed: &'a TypedProgram,
    /// The file whose tables the spans below belong to. It is a cell because
    /// lowering a field default steps into the file that declared the field
    /// and back, and the context is shared by reference.
    file: std::cell::Cell<FileId>,
    /// Every lambda in the program, shared by all files so that one index
    /// names one lambda.
    lambdas: &'a RefCell<Vec<h::Lambda>>,
    /// Declared functions used as values, collected here rather than found
    /// again by walking the finished HIR.
    function_values: &'a RefCell<Vec<(crate::resolver::SymbolId, crate::types::FunctionTypeId)>>,
    /// Array types the program sorts, collected the same way.
    sorts: &'a RefCell<Vec<(crate::types::ArrayId, crate::types::FunctionTypeId)>>,
    /// (class, interface) pairs the program builds a value for.
    vtables: &'a RefCell<Vec<(crate::types::StructId, crate::types::InterfaceId)>>,
}

impl Lowering<'_> {
    fn decl(&self, span: Span) -> SymbolId {
        self.typed.resolution.declarations[&(self.file.get(), span.start)]
    }
    fn reference(&self, span: Span) -> SymbolId {
        self.typed.resolution.references[&(self.file.get(), span.start)]
    }
    fn ty(&self, span: Span) -> Option<Type> {
        self.typed.expression_type_in(self.file.get(), span)
    }
    /// The enum named by the left of `Enum.Variant` or `module.Enum.Variant`.
    /// Checking already accepted it, so this only has to find it again.
    fn enum_prefix(&self, object: &ast::Expr) -> Option<EnumId> {
        let (module, name) = match &object.kind {
            ast::ExprKind::Identifier(name) => (
                self.typed.program.files[self.file.get().0].module,
                &name.text,
            ),
            ast::ExprKind::Member { object, member } => {
                let ast::ExprKind::Identifier(qualifier) = &object.kind else {
                    return None;
                };
                let module = self
                    .typed
                    .resolution
                    .module_in_file(self.file.get(), &qualifier.text)?;
                (module, &member.text)
            }
            _ => return None,
        };
        self.typed.enum_names[module.0].get(name).copied()
    }
}

pub fn lower(typed: TypedProgram) -> h::Program {
    let mut functions = Vec::new();
    let mut externs = Vec::new();
    // Every file of every module, in load order. Symbol ids are already
    // unique across the program, so nothing here has to disambiguate them.
    let lambdas = RefCell::new(Vec::new());
    let function_values = RefCell::new(Vec::new());
    let sorts = RefCell::new(Vec::new());
    let vtables = RefCell::new(Vec::new());
    let files: Vec<Lowering<'_>> = (0..typed.program.files.len())
        .map(|index| Lowering {
            typed: &typed,
            file: std::cell::Cell::new(FileId(index)),
            lambdas: &lambdas,
            function_values: &function_values,
            sorts: &sorts,
            vtables: &vtables,
        })
        .collect();
    for cx in &files {
        let syntax = &typed.program.files[cx.file.get().0].program;
        for block in &syntax.externs {
            for function in &block.functions {
                let id = cx.decl(function.name.span);
                let signature = &typed.signatures[&id];
                externs.push(h::ExternFunction {
                    id,
                    name: typed.externs[&id].name.clone(),
                    parameters: signature.parameters.clone(),
                    return_type: signature.return_type,
                    span: function.span,
                });
            }
        }
        for declaration in &syntax.structs {
            for method in &declaration.methods {
                let id = cx.decl(method.name.span);
                let this = cx.decl(method.body.span);
                let mut parameters = vec![h::Parameter {
                    id: this,
                    ty: typed.symbol_types[this.0],
                    span: Span::new(method.body.span.start, method.body.span.start),
                }];
                parameters.extend(method.parameters.iter().map(|parameter| {
                    let id = cx.decl(parameter.name.span);
                    h::Parameter {
                        id,
                        ty: typed.symbol_types[id.0],
                        span: parameter.span,
                    }
                }));
                functions.push(h::Function {
                    id,
                    name: format!("{}.{}", declaration.name.text, method.name.text),
                    parameters,
                    return_type: typed.signatures[&id].return_type,
                    body: block(&method.body, cx),
                    span: method.span,
                });
            }
        }
        for function in &syntax.functions {
            {
                let id = cx.decl(function.name.span);
                functions.push(h::Function {
                    id,
                    name: function.name.text.clone(),
                    parameters: function
                        .parameters
                        .iter()
                        .map(|parameter| {
                            let id = cx.decl(parameter.name.span);
                            h::Parameter {
                                id,
                                ty: typed.symbol_types[id.0],
                                span: parameter.span,
                            }
                        })
                        .collect(),
                    return_type: typed.signatures[&id].return_type,
                    body: block(&function.body, cx),
                    span: function.span,
                });
            }
        }
    }
    let lambdas = lambdas.into_inner();
    let mut function_values = function_values.into_inner();
    function_values.sort();
    function_values.dedup();
    let mut sorts = sorts.into_inner();
    sorts.sort();
    sorts.dedup();
    let mut vtables = vtables.into_inner();
    vtables.sort();
    vtables.dedup();
    h::Program {
        externs,
        lambdas,
        function_types: typed.function_signatures.clone(),
        function_values,
        sorts,
        vtables,
        interfaces: typed.interfaces.clone(),
        structs: typed.structs.clone(),
        enums: typed.enums.clone(),
        arrays: typed.arrays.clone(),
        fixed_arrays: typed.fixed_arrays.clone(),
        options: typed.options.clone(),
        results: typed.results.clone(),
        functions,
        // Only a program checked with `Entrypoint::Required` is lowered, and
        // that check refuses a program with no `main`.
        entry: typed
            .entry()
            .expect("internal compiler bug: lowering a program with no entrypoint"),
        span: typed.program.files[0].program.span,
    }
}
/// Walk a chain of field accesses down to the local it is rooted in.
fn place(target: &ast::Expr, cx: &Lowering<'_>) -> h::Place {
    match &target.kind {
        ast::ExprKind::Group(inner) => place(inner, cx),
        ast::ExprKind::Identifier(name) => h::Place::Local(cx.reference(name.span)),
        ast::ExprKind::Member { object, member } => {
            let Some(Type::Struct(id)) = cx.ty(object.span) else {
                unreachable!("internal compiler bug: unchecked field assignment")
            };
            let index = cx.typed.structs[id.0]
                .fields
                .iter()
                .position(|f| f.name == member.text)
                .expect("checked field");
            if cx.typed.structs[id.0].reference {
                h::Place::ReferenceField {
                    object: Box::new(expression(object, cx)),
                    index,
                }
            } else {
                h::Place::Field {
                    base: Box::new(place(object, cx)),
                    index,
                }
            }
        }
        ast::ExprKind::Index { object, index } => {
            if let Some(Type::FixedArray(id)) = cx.ty(object.span) {
                let size = cx.typed.fixed_arrays[id.0].size;
                h::Place::FixedIndex {
                    base: Box::new(place(object, cx)),
                    index: Box::new(expression(index, cx)),
                    size,
                }
            } else {
                h::Place::Index {
                    object: Box::new(expression(object, cx)),
                    index: Box::new(expression(index, cx)),
                }
            }
        }
        _ => unreachable!("internal compiler bug: unchecked assignment target"),
    }
}
fn block(source: &ast::Block, cx: &Lowering<'_>) -> h::Block {
    h::Block {
        statements: source.statements.iter().map(|s| statement(s, cx)).collect(),
        span: source.span,
    }
}
fn statement(source: &ast::Statement, cx: &Lowering<'_>) -> h::Statement {
    let kind = match &source.kind {
        ast::StatementKind::Variable(variable) => {
            let id = cx.decl(variable.name.span);
            match &variable.otherwise {
                Some(otherwise) => {
                    let value = expression(&variable.initializer, cx);
                    let (pattern, error_ty) = match value.ty {
                        Type::Option(_) => (h::IfLetPattern::Some, Type::Void),
                        Type::Result(result) => {
                            (h::IfLetPattern::Ok, cx.typed.results[result.0].err)
                        }
                        _ => unreachable!("internal compiler bug: unchecked escape binding"),
                    };
                    let error = otherwise
                        .binding
                        .as_ref()
                        .map(|binding| cx.decl(binding.span));
                    h::StatementKind::GuardVariable {
                        id,
                        ty: cx.typed.symbol_types[id.0],
                        pattern,
                        value,
                        error,
                        error_ty,
                        otherwise: block(&otherwise.block, cx),
                    }
                }
                None => h::StatementKind::Variable {
                    id,
                    ty: cx.typed.symbol_types[id.0],
                    initializer: expression(&variable.initializer, cx),
                },
            }
        }
        ast::StatementKind::Constant(constant) => h::StatementKind::Block(h::Block {
            statements: Vec::new(),
            span: constant.span,
        }),
        ast::StatementKind::Expression(expr) => h::StatementKind::Expression(expression(expr, cx)),
        ast::StatementKind::Return(value) => {
            h::StatementKind::Return(value.as_ref().map(|e| expression(e, cx)))
        }
        // `unsafe` is a promise the checker already made the caller keep. It
        // changes nothing about the code that runs, so it lowers to the block
        // it wraps.
        ast::StatementKind::Block(source) | ast::StatementKind::Unsafe(source) => {
            h::StatementKind::Block(block(source, cx))
        }
        ast::StatementKind::If {
            condition,
            then_block,
            else_branch,
        } => h::StatementKind::If {
            condition: expression(condition, cx),
            then_block: block(then_block, cx),
            else_branch: else_branch.as_ref().map(|s| Box::new(statement(s, cx))),
        },
        ast::StatementKind::IfLet {
            pattern,
            binding,
            value,
            then_block,
            else_branch,
        } => h::StatementKind::IfLet {
            pattern: match pattern {
                ast::IfLetPattern::Some => h::IfLetPattern::Some,
                ast::IfLetPattern::Ok => h::IfLetPattern::Ok,
                ast::IfLetPattern::Err => h::IfLetPattern::Err,
            },
            binding: cx.decl(binding.span),
            value: expression(value, cx),
            then_block: block(then_block, cx),
            else_branch: else_branch.as_ref().map(|s| Box::new(statement(s, cx))),
        },
        ast::StatementKind::While { condition, body } => h::StatementKind::While {
            condition: expression(condition, cx),
            body: block(body, cx),
        },
        ast::StatementKind::Loop { body } => h::StatementKind::Loop {
            body: block(body, cx),
        },
        ast::StatementKind::Break => h::StatementKind::Break,
        ast::StatementKind::Continue => h::StatementKind::Continue,
        ast::StatementKind::Match { value, arms } => {
            let lowered_value = expression(value, cx);
            let is_scalar = !matches!(lowered_value.ty, Type::Enum(_) | Type::Result(_));
            let lowered_arms = arms
                .iter()
                .map(|arm| {
                    let pattern = match &arm.pattern {
                        ast::MatchPattern::Wildcard(_) => h::MatchPattern::Wildcard,
                        ast::MatchPattern::Constant(c_expr) => {
                            h::MatchPattern::Constant(expression(c_expr, cx))
                        }
                        ast::MatchPattern::Range {
                            start,
                            end,
                            inclusive,
                            ..
                        } => h::MatchPattern::Range {
                            start: expression(start, cx),
                            end: expression(end, cx),
                            inclusive: *inclusive,
                        },
                        ast::MatchPattern::Variant {
                            variant_name,
                            binding,
                            ..
                        } => {
                            if is_scalar {
                                let sym_id = cx.reference(variant_name.span);
                                let const_val = &cx.typed.constants[&sym_id];
                                let kind = match const_val {
                                    ConstValue::Int(val, _) => h::ExprKind::Int(*val as i64),
                                    ConstValue::Float(val) => h::ExprKind::Float(*val),
                                    ConstValue::Bool(val) => h::ExprKind::Bool(*val),
                                    ConstValue::Char(val) => h::ExprKind::Char(*val),
                                    ConstValue::String(val) => h::ExprKind::String(val.clone()),
                                };
                                h::MatchPattern::Constant(h::Expr {
                                    kind,
                                    ty: const_val.ty(),
                                    span: variant_name.span,
                                })
                            } else {
                                let variant_index = match lowered_value.ty {
                                    Type::Enum(id) => cx.typed.enums[id.0]
                                        .find_variant(&variant_name.text)
                                        .expect("checked variant"),
                                    Type::Result(_) if variant_name.text == "Ok" => {
                                        crate::types::ResultInfo::OK
                                    }
                                    Type::Result(_) => crate::types::ResultInfo::ERR,
                                    _ => unreachable!("checked match target"),
                                };
                                let binding_id = binding.as_ref().map(|name| cx.decl(name.span));
                                h::MatchPattern::Variant {
                                    variant_index,
                                    binding: binding_id,
                                }
                            }
                        }
                    };
                    h::MatchArm {
                        pattern,
                        body: block(&arm.body, cx),
                    }
                })
                .collect();
            h::StatementKind::Match {
                value: lowered_value,
                arms: lowered_arms,
            }
        }
        ast::StatementKind::For {
            variable,
            iterable,
            body,
        } => {
            let symbol_id = cx.decl(variable.span);
            let lowered_iterable = match iterable {
                ast::ForIterable::Range { start, end } => h::ForIterable::Range {
                    start: expression(start, cx),
                    end: expression(end, cx),
                },
                ast::ForIterable::Expr(collection) => {
                    // `for byte in text.bytes()` is the one place the array
                    // `bytes()` would answer cannot be reached from the body,
                    // so it is not built: the loop reads the string's own
                    // bytes. Everything else about the loop is unchanged —
                    // the string is still evaluated exactly once, before the
                    // first iteration.
                    let lowered = expression(collection, cx);
                    if let h::Expr {
                        kind: h::ExprKind::StringBytes(text),
                        ..
                    } = lowered
                    {
                        h::ForIterable::StringBytes(*text)
                    } else if let Some(Type::FixedArray(id)) = cx.ty(collection.span) {
                        let size = cx.typed.fixed_arrays[id.0].size;
                        h::ForIterable::FixedArray {
                            collection: lowered,
                            size,
                        }
                    } else {
                        h::ForIterable::Array(lowered)
                    }
                }
            };
            h::StatementKind::For {
                variable: symbol_id,
                iterable: lowered_iterable,
                body: block(body, cx),
            }
        }
    };
    h::Statement {
        kind,
        span: source.span,
    }
}
fn strip_groups(mut expr: &ast::Expr) -> &ast::Expr {
    while let ast::ExprKind::Group(inner) = &expr.kind {
        expr = inner;
    }
    expr
}
fn expression(source: &ast::Expr, cx: &Lowering<'_>) -> h::Expr {
    let kind = match &source.kind {
        ast::ExprKind::Lambda(lambda) => {
            let Some(Type::Function(ty)) = cx.ty(source.span) else {
                unreachable!("internal compiler bug: unchecked function value")
            };
            let parameters = lambda
                .parameters
                .iter()
                .map(|parameter| {
                    let id = cx.decl(parameter.name.span);
                    h::Parameter {
                        id,
                        ty: cx.typed.symbol_types[id.0],
                        span: parameter.span,
                    }
                })
                .collect();
            // The resolver already worked out which names crossed the body's
            // boundary; each is copied in, and each is immutable, so the copy
            // can never disagree with the original.
            let captures = cx
                .typed
                .resolution
                .captures
                .get(&(cx.file.get(), lambda.body.span.start))
                .map(|symbols| {
                    symbols
                        .iter()
                        .map(|id| h::Parameter {
                            id: *id,
                            ty: cx.typed.symbol_types[id.0],
                            span: lambda.span,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let index = cx.lambdas.borrow().len();
            // Reserved before the body is lowered, so a nested lambda cannot
            // take this one's index.
            cx.lambdas.borrow_mut().push(h::Lambda {
                index,
                parameters,
                captures,
                return_type: cx.typed.function_signatures[ty.0].return_type,
                body: h::Block {
                    statements: Vec::new(),
                    span: lambda.body.span,
                },
                span: lambda.span,
            });
            let body = block(&lambda.body, cx);
            cx.lambdas.borrow_mut()[index].body = body;
            h::ExprKind::Lambda { index }
        }
        ast::ExprKind::Array(elements) => {
            if matches!(cx.ty(source.span), Some(Type::FixedArray(_))) {
                h::ExprKind::FixedArray(elements.iter().map(|e| expression(e, cx)).collect())
            } else {
                h::ExprKind::Array(elements.iter().map(|e| expression(e, cx)).collect())
            }
        }
        ast::ExprKind::ArrayRepeat { element, .. } => {
            let Some(Type::FixedArray(id)) = cx.ty(source.span) else {
                unreachable!("internal compiler bug: unchecked array repeat")
            };
            let size = cx.typed.fixed_arrays[id.0].size;
            h::ExprKind::FixedArrayRepeat {
                element: Box::new(expression(element, cx)),
                size,
            }
        }
        ast::ExprKind::Index { object, index } => h::ExprKind::Index {
            object: Box::new(expression(object, cx)),
            index: Box::new(expression(index, cx)),
        },
        ast::ExprKind::Slice { object, start, end } => h::ExprKind::Slice {
            object: Box::new(expression(object, cx)),
            start: Box::new(expression(start, cx)),
            end: Box::new(expression(end, cx)),
        },
        ast::ExprKind::Weak(value) => {
            h::ExprKind::Weak(value.as_ref().map(|v| Box::new(expression(v, cx))))
        }
        ast::ExprKind::Try(inner) => h::ExprKind::Try(Box::new(expression(inner, cx))),
        ast::ExprKind::Interpolation(parts) => h::ExprKind::Interpolation(
            parts
                .iter()
                .map(|part| match part {
                    ast::InterpolationPart::Text(text) => h::InterpolationPart::Text(text.clone()),
                    ast::InterpolationPart::Value(value) => {
                        h::InterpolationPart::Value(expression(value, cx))
                    }
                })
                .collect(),
        ),
        ast::ExprKind::StructLiteral { fields, .. } | ast::ExprKind::New { fields, .. } => {
            let Some(Type::Struct(id)) = cx.ty(source.span) else {
                unreachable!("internal compiler bug: unchecked struct literal")
            };
            let mut values: Vec<(usize, h::Expr)> = fields
                .iter()
                .map(|field| {
                    let index = cx.typed.structs[id.0]
                        .fields
                        .iter()
                        .position(|f| f.name == field.name.text)
                        .expect("checked field");
                    (index, expression(&field.value, cx))
                })
                .collect();
            // Then the fields nobody wrote, in declaration order. The written
            // arguments are evaluated first, in the order they were written,
            // which is the rule everywhere else in the language; a default is
            // an expression of the declaring file, so it is lowered with that
            // file's recorded types.
            let declaring = cx.typed.structs[id.0].file;
            // A union has exactly one member written and no others to fill:
            // the rest are the same bytes read another way.
            for index in
                (0..cx.typed.structs[id.0].fields.len()).filter(|_| !cx.typed.structs[id.0].union)
            {
                if values.iter().any(|(written, _)| *written == index) {
                    continue;
                }
                let default = cx.typed.structs[id.0].fields[index]
                    .default
                    .clone()
                    .expect("checked construction leaves no field unset");
                let current = cx.file.replace(declaring);
                let lowered = expression(&default, cx);
                cx.file.set(current);
                values.push((index, lowered));
            }
            h::ExprKind::StructLiteral { id, fields: values }
        }
        ast::ExprKind::Member { object, member } => {
            if let Some(enum_id) = cx.enum_prefix(object) {
                let variant_index = cx.typed.enums[enum_id.0]
                    .find_variant(&member.text)
                    .expect("checked variant");
                h::ExprKind::EnumVariant {
                    variant_index,
                    payload: None,
                }
            } else if let Some(&sym_id) = cx
                .typed
                .resolution
                .references
                .get(&(cx.file.get(), member.span.start))
                && matches!(
                    cx.typed.resolution.symbols[sym_id.0].kind,
                    SymbolKind::Constant
                )
            {
                let const_val = &cx.typed.constants[&sym_id];
                match const_val {
                    ConstValue::Int(val, _) => h::ExprKind::Int(*val as i64),
                    ConstValue::Float(val) => h::ExprKind::Float(*val),
                    ConstValue::Bool(val) => h::ExprKind::Bool(*val),
                    ConstValue::Char(val) => h::ExprKind::Char(*val),
                    ConstValue::String(val) => h::ExprKind::String(val.clone()),
                }
            } else {
                let Some(Type::Struct(id)) = cx.ty(object.span) else {
                    unreachable!("internal compiler bug: unchecked field access")
                };
                let index = cx.typed.structs[id.0]
                    .fields
                    .iter()
                    .position(|field| field.name == member.text)
                    .expect("internal compiler bug: checked access to a missing field");
                h::ExprKind::Field {
                    object: Box::new(expression(object, cx)),
                    index,
                }
            }
        }
        ast::ExprKind::Literal(literal) => match literal {
            ast::Literal::Integer(value) => h::ExprKind::Int(*value as i64),
            ast::Literal::Float(value) => h::ExprKind::Float(*value),
            ast::Literal::Boolean(value) => h::ExprKind::Bool(*value),
            ast::Literal::String(value) => h::ExprKind::String(value.clone()),
            ast::Literal::Char(value) => h::ExprKind::Char(*value),
        },
        ast::ExprKind::Identifier(name) => {
            let id = cx.reference(name.span);
            match cx.typed.resolution.symbols[id.0].kind {
                SymbolKind::Constant => {
                    let const_val = &cx.typed.constants[&id];
                    match const_val {
                        ConstValue::Int(val, _) => h::ExprKind::Int(*val as i64),
                        ConstValue::Float(val) => h::ExprKind::Float(*val),
                        ConstValue::Bool(val) => h::ExprKind::Bool(*val),
                        ConstValue::Char(val) => h::ExprKind::Char(*val),
                        ConstValue::String(val) => h::ExprKind::String(val.clone()),
                    }
                }
                SymbolKind::Builtin(Builtin::None) => h::ExprKind::None,
                // A declared function named where a value is expected becomes
                // a function value with nothing captured.
                SymbolKind::Function => {
                    let Some(Type::Function(ty)) = cx.ty(source.span) else {
                        unreachable!("internal compiler bug: unchecked function value")
                    };
                    cx.function_values.borrow_mut().push((id, ty));
                    h::ExprKind::FunctionValue { id, ty }
                }
                // A `var` can be assigned to, and an assignment is an
                // expression, so only an immutable binding is safe to read in
                // place.
                kind => h::ExprKind::Local {
                    id,
                    settled: !matches!(kind, SymbolKind::Variable(ast::Mutability::Mutable)),
                },
            }
        }
        ast::ExprKind::Group(inner) => expression(inner, cx).kind,
        ast::ExprKind::Unary {
            op,
            operand,
            op_span,
        } => {
            // A signed type's most negative value has no positive literal, so
            // the minus and its operand fold into one constant at every width.
            let minimum = cx
                .ty(source.span)
                .and_then(Type::int_type)
                .filter(|kind| *op == ast::UnaryOp::Negative && kind.signed())
                .filter(|kind| {
                    matches!(
                        strip_groups(operand).kind,
                        ast::ExprKind::Literal(ast::Literal::Integer(value))
                            if value == kind.min_magnitude()
                    )
                });
            if let Some(kind) = minimum {
                h::ExprKind::Int(-(kind.min_magnitude() as i128) as i64)
            } else {
                h::ExprKind::Unary {
                    op: match op {
                        ast::UnaryOp::Positive => h::UnaryOp::Positive,
                        ast::UnaryOp::Negative => h::UnaryOp::Negative,
                        ast::UnaryOp::Not => h::UnaryOp::Not,
                        ast::UnaryOp::BitNot => h::UnaryOp::BitNot,
                    },
                    op_span: *op_span,
                    operand: Box::new(expression(operand, cx)),
                }
            }
        }
        ast::ExprKind::Binary {
            left,
            op,
            right,
            op_span,
        } => h::ExprKind::Binary {
            left: Box::new(expression(left, cx)),
            op: binary(*op),
            op_span: *op_span,
            right: Box::new(expression(right, cx)),
        },
        ast::ExprKind::Assignment {
            target,
            op,
            value,
            op_span,
        } => h::ExprKind::Assignment {
            target: place(target, cx),
            op: match op {
                ast::AssignmentOp::Assign => h::AssignmentOp::Assign,
                ast::AssignmentOp::Add => h::AssignmentOp::Add,
                ast::AssignmentOp::Subtract => h::AssignmentOp::Subtract,
                ast::AssignmentOp::Multiply => h::AssignmentOp::Multiply,
                ast::AssignmentOp::Divide => h::AssignmentOp::Divide,
                ast::AssignmentOp::BitAnd => h::AssignmentOp::BitAnd,
                ast::AssignmentOp::BitOr => h::AssignmentOp::BitOr,
                ast::AssignmentOp::BitXor => h::AssignmentOp::BitXor,
                ast::AssignmentOp::ShiftLeft => h::AssignmentOp::ShiftLeft,
                ast::AssignmentOp::ShiftRight => h::AssignmentOp::ShiftRight,
            },
            op_span: *op_span,
            value: Box::new(expression(value, cx)),
        },
        ast::ExprKind::Call { callee, arguments } => {
            if let ast::ExprKind::Member { object, member } = &strip_groups(callee).kind {
                if let Some(enum_id) = cx.enum_prefix(object) {
                    let variant_index = cx.typed.enums[enum_id.0]
                        .find_variant(&member.text)
                        .expect("checked enum variant");
                    let payload = Some(Box::new(expression(&arguments[0], cx)));
                    return wrap_expression(
                        h::ExprKind::EnumVariant {
                            variant_index,
                            payload,
                        },
                        source,
                        cx,
                    );
                }
                let object_type = cx.ty(object.span);
                let array_method = match (object_type, member.text.as_str()) {
                    (Some(Type::Array(_)), "push") => Some(h::ArrayMethod::Push),
                    (Some(Type::Array(_)), "insert") => Some(h::ArrayMethod::Insert),
                    (Some(Type::Array(_)), "pop") => Some(h::ArrayMethod::Pop),
                    (Some(Type::Array(_)), "remove") => Some(h::ArrayMethod::Remove),
                    (Some(Type::Array(id)), "sort") => {
                        let Some(Type::Function(comparator)) = cx.ty(arguments[0].span) else {
                            unreachable!("internal compiler bug: unchecked comparator")
                        };
                        cx.sorts.borrow_mut().push((id, comparator));
                        Some(h::ArrayMethod::Sort)
                    }
                    (Some(Type::Array(id)), "to_sorted") => {
                        let Some(Type::Function(comparator)) = cx.ty(arguments[0].span) else {
                            unreachable!("internal compiler bug: unchecked comparator")
                        };
                        cx.sorts.borrow_mut().push((id, comparator));
                        Some(h::ArrayMethod::ToSorted)
                    }
                    _ => None,
                };
                if let Some(method) = array_method {
                    return h::Expr {
                        kind: h::ExprKind::ArrayCall {
                            object: Box::new(expression(object, cx)),
                            method,
                            arguments: arguments.iter().map(|arg| expression(arg, cx)).collect(),
                        },
                        ty: cx.ty(source.span).expect("checked expression"),
                        span: source.span,
                    };
                }
                let special = match (object_type, member.text.as_str()) {
                    (Some(Type::Weak(_)), "upgrade") => {
                        Some(h::ExprKind::WeakUpgrade(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Option(_)), "is_some") => {
                        Some(h::ExprKind::IsSome(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Option(_)), "is_none") => {
                        Some(h::ExprKind::IsNone(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Result(_)), "is_ok") => {
                        Some(h::ExprKind::IsOk(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Result(_)), "is_err") => {
                        Some(h::ExprKind::IsErr(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Array(_)), "len") => {
                        Some(h::ExprKind::ArrayLen(Box::new(expression(object, cx))))
                    }
                    (Some(Type::FixedArray(id)), "len") => {
                        let size = cx.typed.fixed_arrays[id.0].size as i64;
                        Some(h::ExprKind::Int(size))
                    }
                    (Some(Type::String), "len") => {
                        Some(h::ExprKind::StringLen(Box::new(expression(object, cx))))
                    }
                    (Some(Type::String), "bytes") => {
                        Some(h::ExprKind::StringBytes(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Weak(_)), "alive") => {
                        Some(h::ExprKind::WeakAlive(Box::new(expression(object, cx))))
                    }
                    (Some(Type::Weak(_)), "get") => {
                        Some(h::ExprKind::WeakGet(Box::new(expression(object, cx))))
                    }
                    _ => None,
                };
                if let Some(kind) = special {
                    return h::Expr {
                        kind,
                        ty: cx.ty(source.span).expect("checked expression"),
                        span: source.span,
                    };
                }
                // A module-qualified call: the resolver bound the right half
                // to the exported function, so this lowers as a direct call
                // and the qualifier itself produces no code.
                if let ast::ExprKind::Identifier(qualifier) = &object.kind
                    && cx
                        .typed
                        .resolution
                        .module_in_file(cx.file.get(), &qualifier.text)
                        .is_some()
                {
                    return h::Expr {
                        kind: h::ExprKind::Call {
                            target: h::CallTarget::Function(cx.reference(member.span)),
                            arguments: arguments.iter().map(|e| expression(e, cx)).collect(),
                        },
                        ty: cx.ty(source.span).expect("checked expression"),
                        span: source.span,
                    };
                }
                if let Some(Type::Interface(interface)) = cx.ty(object.span) {
                    let index = cx.typed.interfaces[interface.0]
                        .methods
                        .iter()
                        .position(|method| method.name == member.text)
                        .expect("internal compiler bug: checked interface method");
                    return h::Expr {
                        kind: h::ExprKind::InterfaceCall {
                            object: Box::new(expression(object, cx)),
                            interface,
                            index,
                            arguments: arguments.iter().map(|e| expression(e, cx)).collect(),
                        },
                        ty: cx.ty(source.span).expect("checked expression"),
                        span: source.span,
                    };
                }
                let Some(Type::Struct(id)) = cx.ty(object.span) else {
                    unreachable!("internal compiler bug: unchecked method call")
                };
                let method = cx.typed.structs[id.0]
                    .methods
                    .iter()
                    .find(|method| method.name == member.text)
                    .expect("internal compiler bug: checked call to a missing method");
                // The receiver is an ordinary leading argument, copied like any
                // other value-typed argument.
                let mut values = vec![expression(object, cx)];
                values.extend(arguments.iter().map(|e| expression(e, cx)));
                return h::Expr {
                    kind: h::ExprKind::Call {
                        target: h::CallTarget::Function(method.id),
                        arguments: values,
                    },
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            // A call through a value: the callee is an expression, not a name.
            if !matches!(strip_groups(callee).kind, ast::ExprKind::Identifier(_))
                || matches!(cx.ty(callee.span), Some(Type::Function(_)))
            {
                return wrap_expression(
                    h::ExprKind::Call {
                        target: h::CallTarget::Value(Box::new(expression(callee, cx))),
                        arguments: arguments.iter().map(|e| expression(e, cx)).collect(),
                    },
                    source,
                    cx,
                );
            }
            let ast::ExprKind::Identifier(name) = &strip_groups(callee).kind else {
                unreachable!("internal compiler bug: indirect checked call")
            };
            let id = cx.reference(name.span);
            if cx.typed.resolution.symbols[id.0].kind == SymbolKind::Builtin(Builtin::BytesToString)
            {
                return h::Expr {
                    kind: h::ExprKind::BytesToString(Box::new(expression(&arguments[0], cx))),
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            if let SymbolKind::Builtin(Builtin::IntConvert(target)) =
                cx.typed.resolution.symbols[id.0].kind
            {
                let mut value = expression(&arguments[0], cx);
                // A numbered enum reaches the width conversion as the integer
                // it is worth, so nothing below here knows about enums.
                if let Type::Enum(enum_id) = value.ty {
                    let underlying = cx.typed.enums[enum_id.0]
                        .underlying
                        .expect("checked numbered enum");
                    let span = value.span;
                    value = h::Expr {
                        kind: h::ExprKind::EnumValue {
                            value: Box::new(value),
                            id: enum_id,
                        },
                        ty: Type::Int(underlying),
                        span,
                    };
                }
                return h::Expr {
                    kind: h::ExprKind::IntConvert {
                        value: Box::new(value),
                        target,
                    },
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            // `Protocol(6)`: the conversion back, which traps on a value no
            // variant is worth.
            if cx.typed.resolution.symbols[id.0].kind == SymbolKind::Enum
                && let Some(Type::Enum(enum_id)) = cx.ty(source.span)
            {
                return h::Expr {
                    kind: h::ExprKind::EnumFromValue {
                        value: Box::new(expression(&arguments[0], cx)),
                        id: enum_id,
                    },
                    ty: Type::Enum(enum_id),
                    span: source.span,
                };
            }
            if cx.typed.resolution.symbols[id.0].kind == SymbolKind::Builtin(Builtin::FloatConvert)
            {
                return h::Expr {
                    kind: h::ExprKind::FloatConvert(Box::new(expression(&arguments[0], cx))),
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            if cx.typed.resolution.symbols[id.0].kind == SymbolKind::Builtin(Builtin::CharConvert) {
                return h::Expr {
                    kind: h::ExprKind::CharConvert(Box::new(expression(&arguments[0], cx))),
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            let constructor = match cx.typed.resolution.symbols[id.0].kind {
                SymbolKind::Builtin(Builtin::Some) => Some(h::ExprKind::Some as fn(_) -> _),
                SymbolKind::Builtin(Builtin::Ok) => Some(h::ExprKind::Ok as fn(_) -> _),
                SymbolKind::Builtin(Builtin::Err) => Some(h::ExprKind::Err as fn(_) -> _),
                _ => None,
            };
            if let Some(constructor) = constructor {
                return h::Expr {
                    kind: constructor(Box::new(expression(&arguments[0], cx))),
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            if cx.typed.resolution.symbols[id.0].kind == SymbolKind::Builtin(Builtin::Ptr) {
                let operand = expression(&arguments[0], cx);
                // `ptr` over a string or an array borrows the bytes it already
                // owns; over a scalar local it takes that local's address.
                let kind = match operand.ty {
                    Type::String | Type::Array(_) | Type::FixedArray(_) => {
                        h::ExprKind::Ptr(Box::new(operand))
                    }
                    _ => h::ExprKind::AddressOf(Box::new(operand)),
                };
                return h::Expr {
                    kind,
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            if let Some(&(id, field)) =
                cx.typed
                    .layout_queries
                    .get(&(cx.file.get(), source.span.start, source.span.end))
            {
                return h::Expr {
                    kind: h::ExprKind::LayoutOf { id, field },
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            if let SymbolKind::Builtin(
                builtin @ (Builtin::Load
                | Builtin::Store
                | Builtin::VolatileLoad
                | Builtin::VolatileStore
                | Builtin::Offset
                | Builtin::Addr
                | Builtin::PtrFrom),
            ) = cx.typed.resolution.symbols[id.0].kind
            {
                let pointer = Box::new(expression(&arguments[0], cx));
                let kind = match builtin {
                    Builtin::Load => h::ExprKind::Load {
                        pointer,
                        volatile: false,
                    },
                    Builtin::VolatileLoad => h::ExprKind::Load {
                        pointer,
                        volatile: true,
                    },
                    Builtin::Store => h::ExprKind::Store {
                        pointer,
                        value: Box::new(expression(&arguments[1], cx)),
                        volatile: false,
                    },
                    Builtin::VolatileStore => h::ExprKind::Store {
                        pointer,
                        value: Box::new(expression(&arguments[1], cx)),
                        volatile: true,
                    },
                    Builtin::Offset => h::ExprKind::PointerOffset {
                        pointer,
                        count: Box::new(expression(&arguments[1], cx)),
                    },
                    Builtin::Addr => h::ExprKind::PointerAddr(pointer),
                    _ => h::ExprKind::PointerFrom(pointer),
                };
                return h::Expr {
                    kind,
                    ty: cx.ty(source.span).expect("checked expression"),
                    span: source.span,
                };
            }
            let target = match cx.typed.resolution.symbols[id.0].kind {
                SymbolKind::Builtin(Builtin::Print) => h::CallTarget::Print,
                SymbolKind::Function if cx.typed.externs.contains_key(&id) => {
                    h::CallTarget::Extern(id)
                }
                SymbolKind::Function => h::CallTarget::Function(id),
                _ => unreachable!("internal compiler bug: non-callable checked symbol"),
            };
            h::ExprKind::Call {
                target,
                arguments: arguments.iter().map(|e| expression(e, cx)).collect(),
            }
        }
    };
    wrap_expression(kind, source, cx)
}
fn wrap_expression(kind: h::ExprKind, source: &ast::Expr, cx: &Lowering<'_>) -> h::Expr {
    let mut lowered = h::Expr {
        kind,
        ty: cx.ty(source.span).expect("checked expression"),
        span: source.span,
    };
    // A class used where an interface was expected becomes the pair of the
    // object and that interface's table. It happens first, so that a class
    // going into an `Option<Interface>` is widened and then wrapped.
    if let Some(Type::Interface(interface)) = cx
        .typed
        .interface_wraps
        .get(&(cx.file.get(), source.span.start, source.span.end))
        .copied()
        && let Type::Struct(class) = lowered.ty
    {
        cx.vtables.borrow_mut().push((class, interface));
        lowered = h::Expr {
            kind: h::ExprKind::InterfaceValue {
                object: Box::new(lowered),
                class,
                interface,
            },
            ty: Type::Interface(interface),
            span: source.span,
        };
    }
    if let Some(target_type) = cx
        .typed
        .slice_coercions
        .get(&(cx.file.get(), source.span.start, source.span.end))
        .copied()
        && let Type::Array(array_id) = target_type
    {
        lowered = h::Expr {
            kind: h::ExprKind::FixedArrayToSlice {
                object: Box::new(lowered),
                array_id,
            },
            ty: target_type,
            span: source.span,
        };
    }
    if let Some(target_type) =
        cx.typed
            .implicit_wraps
            .get(&(cx.file.get(), source.span.start, source.span.end))
    {
        h::Expr {
            kind: h::ExprKind::Some(Box::new(lowered)),
            ty: *target_type,
            span: source.span,
        }
    } else {
        lowered
    }
}
fn binary(op: ast::BinaryOp) -> h::BinaryOp {
    match op {
        ast::BinaryOp::Or => h::BinaryOp::Or,
        ast::BinaryOp::And => h::BinaryOp::And,
        ast::BinaryOp::BitOr => h::BinaryOp::BitOr,
        ast::BinaryOp::BitXor => h::BinaryOp::BitXor,
        ast::BinaryOp::BitAnd => h::BinaryOp::BitAnd,
        ast::BinaryOp::Equal => h::BinaryOp::Equal,
        ast::BinaryOp::NotEqual => h::BinaryOp::NotEqual,
        ast::BinaryOp::Less => h::BinaryOp::Less,
        ast::BinaryOp::Greater => h::BinaryOp::Greater,
        ast::BinaryOp::LessEqual => h::BinaryOp::LessEqual,
        ast::BinaryOp::GreaterEqual => h::BinaryOp::GreaterEqual,
        ast::BinaryOp::ShiftLeft => h::BinaryOp::ShiftLeft,
        ast::BinaryOp::ShiftRight => h::BinaryOp::ShiftRight,
        ast::BinaryOp::Add => h::BinaryOp::Add,
        ast::BinaryOp::Subtract => h::BinaryOp::Subtract,
        ast::BinaryOp::Multiply => h::BinaryOp::Multiply,
        ast::BinaryOp::Divide => h::BinaryOp::Divide,
        ast::BinaryOp::Modulo => h::BinaryOp::Modulo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lowers_typed_symbols_and_signed_minimum() {
        let source = "func main() { let x = -(9223372036854775808)\nprint(x) }";
        let hir = lower(crate::check(source).expect("checked"));
        assert_eq!(hir.span(), crate::span::Span::new(0, source.len()));
        let h::StatementKind::Variable {
            initializer, id, ..
        } = &hir.functions[0].body.statements[0].kind
        else {
            panic!("variable")
        };
        assert!(matches!(initializer.kind, h::ExprKind::Int(i64::MIN)));
        assert_eq!(initializer.ty, crate::types::Type::INT);
        let h::StatementKind::Expression(h::Expr {
            kind:
                h::ExprKind::Call {
                    target: h::CallTarget::Print,
                    arguments,
                },
            ..
        }) = &hir.functions[0].body.statements[1].kind
        else {
            panic!("builtin")
        };
        assert!(
            matches!(arguments[0].kind, h::ExprKind::Local { id: symbol, .. } if symbol == *id)
        );
    }
}
