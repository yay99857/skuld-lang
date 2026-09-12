//! Readable C11 generation from typed HIR only. No source syntax or name lookup.
use crate::{hir::*, types::Type};

pub fn emit_c(program: &Program) -> String {
    let mut emitter = Emitter {
        output: PRELUDE.into(),
        indent: 0,
        next_temp: 0,
    };
    for function in &program.functions {
        emitter.line(&format!("{};", signature(function)));
    }
    for function in &program.functions {
        emitter.line("");
        emitter.line(&format!(
            "/* {}: source bytes {}..{} */",
            function.name, function.span.start, function.span.end
        ));
        emitter.line(&format!("{} {{", signature(function)));
        emitter.indent += 1;
        for parameter in &function.parameters {
            emitter.line(&format!(
                "(void)skuld_v{}; /* parameter bytes {}..{} */",
                parameter.id.0, parameter.span.start, parameter.span.end
            ));
        }
        emitter.block_contents(&function.body);
        emitter.indent -= 1;
        emitter.line("}");
    }
    emitter.line("");
    emitter.line("int main(void) {");
    emitter.indent += 1;
    emitter.line(&format!("skuld_f{}();", program.entry.0));
    emitter.line("return fflush(stdout) == 0 ? 0 : 1;");
    emitter.indent -= 1;
    emitter.line("}");
    emitter.output
}
fn c_type(ty: Type) -> &'static str {
    match ty {
        Type::Int => "int64_t",
        Type::Float => "double",
        Type::Bool => "bool",
        Type::String => "skuld_string",
        Type::Void => "void",
        Type::Error => unreachable!("internal compiler bug: error type in HIR"),
    }
}
fn signature(function: &Function) -> String {
    let params = if function.parameters.is_empty() {
        "void".into()
    } else {
        function
            .parameters
            .iter()
            .map(|p| format!("{} skuld_v{}", c_type(p.ty), p.id.0))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{} skuld_f{}({params})",
        c_type(function.return_type),
        function.id.0
    )
}
struct Emitter {
    output: String,
    indent: usize,
    next_temp: usize,
}
impl Emitter {
    fn line(&mut self, line: &str) {
        self.output.push_str(&"    ".repeat(self.indent));
        self.output.push_str(line);
        self.output.push('\n');
    }
    fn temporary(&mut self, ty: Type, value: &str) -> String {
        let name = format!("skuld_t{}", self.next_temp);
        self.next_temp += 1;
        self.line(&format!("{} {name} = {value};", c_type(ty)));
        name
    }
    fn block_contents(&mut self, block: &Block) {
        self.line(&format!(
            "/* block bytes {}..{} */",
            block.span.start, block.span.end
        ));
        for statement in &block.statements {
            self.statement(statement);
        }
    }
    fn block(&mut self, block: &Block) {
        self.line("{");
        self.indent += 1;
        self.block_contents(block);
        self.indent -= 1;
        self.line("}");
    }
    fn statement(&mut self, statement: &Statement) {
        self.line(&format!(
            "/* statement bytes {}..{} */",
            statement.span.start, statement.span.end
        ));
        match &statement.kind {
            StatementKind::Variable {
                id,
                ty,
                initializer,
            } => {
                let value = self.expression(initializer);
                self.line(&format!("{} skuld_v{} = {value};", c_type(*ty), id.0));
                self.line(&format!("(void)skuld_v{};", id.0));
            }
            StatementKind::Expression(expr) => {
                let value = self.expression(expr);
                if expr.ty != Type::Void {
                    self.line(&format!("(void){value};"));
                }
            }
            StatementKind::Return(value) => {
                let value = value
                    .as_ref()
                    .map(|e| self.expression(e))
                    .unwrap_or_default();
                self.line(&format!("return {value};"));
            }
            StatementKind::Block(block) => self.block(block),
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                let condition = self.expression(condition);
                self.line(&format!("if ({condition})"));
                self.block(then_block);
                if let Some(branch) = else_branch {
                    self.line("else {");
                    self.indent += 1;
                    self.statement(branch);
                    self.indent -= 1;
                    self.line("}");
                }
            }
            StatementKind::While { condition, body } => {
                // Evaluating the condition can emit temporaries, so it cannot be
                // hoisted the way `if` does: emit it inside the loop and exit
                // with a break. `continue` then re-tests the condition, which is
                // what a `while` must do.
                self.line("for (;;) {");
                self.indent += 1;
                let condition = self.expression(condition);
                self.line(&format!("if (!({condition})) break;"));
                self.block_contents(body);
                self.indent -= 1;
                self.line("}");
            }
            StatementKind::Loop { body } => {
                self.line("for (;;) {");
                self.indent += 1;
                self.block_contents(body);
                self.indent -= 1;
                self.line("}");
            }
            // C binds these to the innermost enclosing loop, which is exactly
            // how they are checked. In a while, `continue` reaches the emitted
            // condition test at the top of the loop.
            StatementKind::Break => self.line("break;"),
            StatementKind::Continue => self.line("continue;"),
        }
    }
    fn expression(&mut self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(value) => {
                if *value == i64::MIN {
                    "INT64_MIN".into()
                } else if *value < 0 {
                    format!("(-INT64_C({}))", value.unsigned_abs())
                } else {
                    format!("INT64_C({value})")
                }
            }
            ExprKind::Float(value) => format!("{value:.17e}"),
            ExprKind::Bool(value) => value.to_string(),
            ExprKind::String(value) => {
                // All bytes use fixed-width octal escapes: no injection, NUL
                // truncation, trigraphs or dependence on the C source charset.
                let escaped: String = value
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("\\{byte:03o}"))
                    .collect();
                format!(
                    "((skuld_string){{(const unsigned char *)\"{escaped}\", {}}})",
                    value.len()
                )
            }
            ExprKind::Local(id) => self.temporary(expr.ty, &format!("skuld_v{}", id.0)),
            ExprKind::Unary {
                op,
                operand,
                op_span,
            } => {
                let value = self.expression(operand);
                let result = match op {
                    UnaryOp::Positive => value,
                    UnaryOp::Negative if expr.ty == Type::Int => {
                        format!("skuld_neg({value}, {})", op_span.start)
                    }
                    UnaryOp::Negative => format!("(-{value})"),
                    UnaryOp::Not => format!("(!{value})"),
                };
                self.temporary(expr.ty, &result)
            }
            ExprKind::Binary {
                left,
                op,
                right,
                op_span,
            } => {
                let left_value = self.expression(left);
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let result = self.temporary(Type::Bool, &left_value);
                    let condition = if *op == BinaryOp::And {
                        result.clone()
                    } else {
                        format!("!{result}")
                    };
                    self.line(&format!("if ({condition}) {{"));
                    self.indent += 1;
                    let right = self.expression(right);
                    self.line(&format!("{result} = {right};"));
                    self.indent -= 1;
                    self.line("}");
                    result
                } else {
                    let right_value = self.expression(right);
                    let result =
                        binary_value(*op, left.ty, &left_value, &right_value, op_span.start);
                    self.temporary(expr.ty, &result)
                }
            }
            ExprKind::Assignment {
                target,
                op,
                value,
                op_span,
            } => {
                // Compound assignment snapshots the old value before its RHS.
                let old = if *op != AssignmentOp::Assign {
                    Some(self.temporary(expr.ty, &format!("skuld_v{}", target.0)))
                } else {
                    None
                };
                let value = self.expression(value);
                let result = if let Some(old) = old {
                    let op = match op {
                        AssignmentOp::Add => BinaryOp::Add,
                        AssignmentOp::Subtract => BinaryOp::Subtract,
                        AssignmentOp::Multiply => BinaryOp::Multiply,
                        AssignmentOp::Divide => BinaryOp::Divide,
                        AssignmentOp::Assign => unreachable!(),
                    };
                    binary_value(op, expr.ty, &old, &value, op_span.start)
                } else {
                    value
                };
                let result = self.temporary(expr.ty, &result);
                self.line(&format!("skuld_v{} = {result};", target.0));
                result
            }
            ExprKind::Call { target, arguments } => {
                let values: Vec<_> = arguments.iter().map(|arg| self.expression(arg)).collect();
                let call = match target {
                    CallTarget::Function(id) => format!("skuld_f{}({})", id.0, values.join(", ")),
                    CallTarget::Print if arguments.is_empty() => format!(
                        "skuld_print_string((skuld_string){{(const unsigned char *)\"\", 0}}, {})",
                        expr.span.start
                    ),
                    CallTarget::Print => {
                        let suffix = match arguments[0].ty {
                            Type::Int => "int",
                            Type::Float => "float",
                            Type::Bool => "bool",
                            Type::String => "string",
                            _ => unreachable!("internal compiler bug: non-printable HIR argument"),
                        };
                        format!("skuld_print_{suffix}({}, {})", values[0], expr.span.start)
                    }
                };
                if expr.ty == Type::Void {
                    self.line(&format!("{call};"));
                    String::new()
                } else {
                    self.temporary(expr.ty, &call)
                }
            }
        }
    }
}
fn binary_value(op: BinaryOp, ty: Type, left: &str, right: &str, byte: usize) -> String {
    use BinaryOp::*;
    let helper = if ty == Type::Int {
        match op {
            Add => Some("add"),
            Subtract => Some("sub"),
            Multiply => Some("mul"),
            Divide => Some("div"),
            Modulo => Some("rem"),
            _ => None,
        }
    } else {
        None
    };
    if let Some(helper) = helper {
        return format!("skuld_{helper}({left}, {right}, {byte})");
    }
    if ty == Type::String {
        let negate = if op == NotEqual { "!" } else { "" };
        return format!("{negate}skuld_string_equal({left}, {right})");
    }
    let operator = match op {
        Add => "+",
        Subtract => "-",
        Multiply => "*",
        Divide => "/",
        Modulo => "%",
        Equal => "==",
        NotEqual => "!=",
        Less => "<",
        Greater => ">",
        LessEqual => "<=",
        GreaterEqual => ">=",
        And => "&&",
        Or => "||",
    };
    format!("({left} {operator} {right})")
}

const PRELUDE: &str = r#"/* Generated by Skuld. C11, compiled with clang. */
#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <float.h>
_Static_assert(DBL_MANT_DIG == 53 && DBL_MAX_EXP == 1024, "Skuld requires binary64 double");

typedef struct { const unsigned char *data; size_t len; } skuld_string;

static inline void skuld_fail(const char *message, size_t byte) {
    fprintf(stderr, "runtime error: %s (source byte %zu)\n", message, byte);
    exit(1);
}
static inline int64_t skuld_add(int64_t a, int64_t b, size_t byte) {
    int64_t result;
    if (__builtin_add_overflow(a, b, &result)) skuld_fail("integer overflow", byte);
    return result;
}
static inline int64_t skuld_sub(int64_t a, int64_t b, size_t byte) {
    int64_t result;
    if (__builtin_sub_overflow(a, b, &result)) skuld_fail("integer overflow", byte);
    return result;
}
static inline int64_t skuld_mul(int64_t a, int64_t b, size_t byte) {
    int64_t result;
    if (__builtin_mul_overflow(a, b, &result)) skuld_fail("integer overflow", byte);
    return result;
}
static inline int64_t skuld_div(int64_t a, int64_t b, size_t byte) {
    if (b == 0) skuld_fail("integer division by zero", byte);
    if (a == INT64_MIN && b == -1) skuld_fail("integer overflow", byte);
    return a / b;
}
static inline int64_t skuld_rem(int64_t a, int64_t b, size_t byte) {
    if (b == 0) skuld_fail("integer remainder by zero", byte);
    if (a == INT64_MIN && b == -1) return 0;
    return a % b;
}
static inline int64_t skuld_neg(int64_t a, size_t byte) {
    if (a == INT64_MIN) skuld_fail("integer overflow", byte);
    return -a;
}
static inline bool skuld_string_equal(skuld_string a, skuld_string b) {
    return a.len == b.len && memcmp(a.data, b.data, a.len) == 0;
}
static inline void skuld_print_int(int64_t value, size_t byte) {
    if (printf("%" PRId64 "\n", value) < 0) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_float(double value, size_t byte) {
    if (printf("%.17g\n", value) < 0) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_bool(bool value, size_t byte) {
    if (puts(value ? "true" : "false") == EOF) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_string(skuld_string value, size_t byte) {
    if (fwrite(value.data, 1, value.len, stdout) != value.len || fputc('\n', stdout) == EOF)
        skuld_fail("stdout write failed", byte);
}

"#;
