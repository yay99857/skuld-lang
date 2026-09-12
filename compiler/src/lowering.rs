//! AST-to-HIR lowering after resolution and checking, with no code generation.
use crate::{ast, hir as h, resolver::SymbolKind, type_checker::TypedProgram};

pub fn lower(typed: TypedProgram) -> h::Program {
    let functions = typed
        .syntax
        .functions
        .iter()
        .map(|function| {
            let id = typed.resolution.declarations[&function.name.span.start];
            h::Function {
                id,
                name: function.name.text.clone(),
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let id = typed.resolution.declarations[&parameter.name.span.start];
                        h::Parameter {
                            id,
                            ty: typed.symbol_types[id.0],
                            span: parameter.span,
                        }
                    })
                    .collect(),
                return_type: typed.signatures[&id].return_type,
                body: block(&function.body, &typed),
                span: function.span,
            }
        })
        .collect();
    h::Program {
        functions,
        entry: typed.entry,
        span: typed.syntax.span,
    }
}
fn block(source: &ast::Block, typed: &TypedProgram) -> h::Block {
    h::Block {
        statements: source
            .statements
            .iter()
            .map(|s| statement(s, typed))
            .collect(),
        span: source.span,
    }
}
fn statement(source: &ast::Statement, typed: &TypedProgram) -> h::Statement {
    let kind = match &source.kind {
        ast::StatementKind::Variable(variable) => {
            let id = typed.resolution.declarations[&variable.name.span.start];
            h::StatementKind::Variable {
                id,
                ty: typed.symbol_types[id.0],
                initializer: expression(&variable.initializer, typed),
            }
        }
        ast::StatementKind::Expression(expr) => {
            h::StatementKind::Expression(expression(expr, typed))
        }
        ast::StatementKind::Return(value) => {
            h::StatementKind::Return(value.as_ref().map(|e| expression(e, typed)))
        }
        ast::StatementKind::Block(source) => h::StatementKind::Block(block(source, typed)),
        ast::StatementKind::If {
            condition,
            then_block,
            else_branch,
        } => h::StatementKind::If {
            condition: expression(condition, typed),
            then_block: block(then_block, typed),
            else_branch: else_branch.as_ref().map(|s| Box::new(statement(s, typed))),
        },
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
fn expression(source: &ast::Expr, typed: &TypedProgram) -> h::Expr {
    let kind = match &source.kind {
        ast::ExprKind::Literal(literal) => match literal {
            ast::Literal::Integer(value) => h::ExprKind::Int(*value as i64),
            ast::Literal::Float(value) => h::ExprKind::Float(*value),
            ast::Literal::Boolean(value) => h::ExprKind::Bool(*value),
            ast::Literal::String(value) => h::ExprKind::String(value.clone()),
            ast::Literal::Char(_) => unreachable!("internal compiler bug: checked char literal"),
        },
        ast::ExprKind::Identifier(name) => {
            h::ExprKind::Local(typed.resolution.references[&name.span.start])
        }
        ast::ExprKind::Group(inner) => expression(inner, typed).kind,
        ast::ExprKind::Unary {
            op,
            operand,
            op_span,
        } => {
            if *op == ast::UnaryOp::Negative
                && matches!(strip_groups(operand).kind, ast::ExprKind::Literal(ast::Literal::Integer(value)) if value == (1_u64 << 63))
            {
                h::ExprKind::Int(i64::MIN)
            } else {
                h::ExprKind::Unary {
                    op: match op {
                        ast::UnaryOp::Positive => h::UnaryOp::Positive,
                        ast::UnaryOp::Negative => h::UnaryOp::Negative,
                        ast::UnaryOp::Not => h::UnaryOp::Not,
                    },
                    op_span: *op_span,
                    operand: Box::new(expression(operand, typed)),
                }
            }
        }
        ast::ExprKind::Binary {
            left,
            op,
            right,
            op_span,
        } => h::ExprKind::Binary {
            left: Box::new(expression(left, typed)),
            op: binary(*op),
            op_span: *op_span,
            right: Box::new(expression(right, typed)),
        },
        ast::ExprKind::Assignment {
            target,
            op,
            value,
            op_span,
        } => {
            let ast::ExprKind::Identifier(name) = &target.kind else {
                unreachable!("internal compiler bug: checked member assignment")
            };
            h::ExprKind::Assignment {
                target: typed.resolution.references[&name.span.start],
                op: match op {
                    ast::AssignmentOp::Assign => h::AssignmentOp::Assign,
                    ast::AssignmentOp::Add => h::AssignmentOp::Add,
                    ast::AssignmentOp::Subtract => h::AssignmentOp::Subtract,
                    ast::AssignmentOp::Multiply => h::AssignmentOp::Multiply,
                    ast::AssignmentOp::Divide => h::AssignmentOp::Divide,
                },
                op_span: *op_span,
                value: Box::new(expression(value, typed)),
            }
        }
        ast::ExprKind::Call { callee, arguments } => {
            let ast::ExprKind::Identifier(name) = &strip_groups(callee).kind else {
                unreachable!("internal compiler bug: indirect checked call")
            };
            let id = typed.resolution.references[&name.span.start];
            let target = match typed.resolution.symbols[id.0].kind {
                SymbolKind::Builtin(_) => h::CallTarget::Print,
                SymbolKind::Function => h::CallTarget::Function(id),
                _ => unreachable!("internal compiler bug: non-callable checked symbol"),
            };
            h::ExprKind::Call {
                target,
                arguments: arguments.iter().map(|e| expression(e, typed)).collect(),
            }
        }
        ast::ExprKind::Member { .. } => {
            unreachable!("internal compiler bug: checked member access")
        }
    };
    h::Expr {
        kind,
        ty: typed.expressions[&(source.span.start, source.span.end)],
        span: source.span,
    }
}
fn binary(op: ast::BinaryOp) -> h::BinaryOp {
    match op {
        ast::BinaryOp::Or => h::BinaryOp::Or,
        ast::BinaryOp::And => h::BinaryOp::And,
        ast::BinaryOp::Equal => h::BinaryOp::Equal,
        ast::BinaryOp::NotEqual => h::BinaryOp::NotEqual,
        ast::BinaryOp::Less => h::BinaryOp::Less,
        ast::BinaryOp::Greater => h::BinaryOp::Greater,
        ast::BinaryOp::LessEqual => h::BinaryOp::LessEqual,
        ast::BinaryOp::GreaterEqual => h::BinaryOp::GreaterEqual,
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
        assert_eq!(initializer.ty, crate::types::Type::Int);
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
        assert!(matches!(arguments[0].kind, h::ExprKind::Local(symbol) if symbol == *id));
    }
}
