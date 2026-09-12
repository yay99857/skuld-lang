//! Readable C11 generation from typed HIR only. No source syntax or name lookup.
use crate::{hir::*, type_checker::StructInfo, types::Type};

pub fn emit_c(program: &Program) -> String {
    let mut emitter = Emitter {
        output: format!("{PRELUDE_HEAD}{RUNTIME}{PRELUDE_TAIL}"),
        indent: 0,
        next_temp: 0,
        structs: program.structs.clone(),
    };
    // Declaration order is a valid definition order: a value type cannot
    // contain itself, and the checker rejects any cycle.
    for (index, declaration) in program.structs.iter().enumerate() {
        emitter.line("");
        emitter.line(&format!(
            "/* struct {}: source bytes {}..{} */",
            declaration.name, declaration.span.start, declaration.span.end
        ));
        emitter.line("typedef struct {");
        emitter.indent += 1;
        for (position, field) in declaration.fields.iter().enumerate() {
            emitter.line(&format!(
                "{} f{position}; /* {} */",
                c_type(field.ty),
                field.name
            ));
        }
        emitter.indent -= 1;
        emitter.line(&format!("}} skuld_s{index};"));
    }
    for index in 0..program.structs.len() {
        if !emitter.managed(Type::Struct(crate::types::StructId(index))) {
            continue;
        }
        let fields: Vec<_> = emitter.structs[index]
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| emitter.managed(field.ty))
            .map(|(position, field)| (position, field.ty))
            .collect();
        emitter.line("");
        emitter.line(&format!(
            "static inline skuld_s{index} skuld_s{index}_retain(skuld_s{index} value) {{"
        ));
        emitter.indent += 1;
        for (position, ty) in &fields {
            let retained = emitter.retained(*ty, &format!("value.f{position}"));
            emitter.line(&format!("value.f{position} = {retained};"));
        }
        emitter.line("return value;");
        emitter.indent -= 1;
        emitter.line("}");
        emitter.line(&format!(
            "static inline void skuld_s{index}_release(skuld_s{index} *slot) {{"
        ));
        emitter.indent += 1;
        for (position, ty) in &fields {
            let release = emitter
                .release_function(*ty)
                .expect("managed field has a release");
            emitter.line(&format!("{release}(&slot->f{position});"));
        }
        emitter.indent -= 1;
        emitter.line("}");
        emitter.line(&format!(
            "static inline void skuld_s{index}_assign(skuld_s{index} *slot, skuld_s{index} value) {{"
        ));
        emitter.indent += 1;
        emitter.line(&format!("skuld_s{index} previous = *slot;"));
        emitter.line(&format!("*slot = skuld_s{index}_retain(value);"));
        emitter.line(&format!("skuld_s{index}_release(&previous);"));
        emitter.indent -= 1;
        emitter.line("}");
    }
    if !program.structs.is_empty() {
        emitter.line("");
    }
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
fn place_expression(place: &Place) -> String {
    let mut rendered = format!("skuld_v{}", place.base.0);
    for index in &place.fields {
        rendered.push_str(&format!(".f{index}"));
    }
    rendered
}
fn c_type(ty: Type) -> String {
    match ty {
        Type::Int => "int64_t".into(),
        Type::Float => "double".into(),
        Type::Bool => "bool".into(),
        Type::String => "skuld_string".into(),
        Type::Void => "void".into(),
        // C struct assignment copies, which is exactly value semantics.
        Type::Struct(id) => format!("skuld_s{}", id.0),
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
    /// Needed to decide which types own a reference and must be released.
    structs: Vec<StructInfo>,
}
impl Emitter {
    /// A type owns references when it is a string or holds one, directly or
    /// through another struct. Unmanaged values need no retain, release or
    /// cleanup, so they cost exactly what they did before.
    fn managed(&self, ty: Type) -> bool {
        match ty {
            Type::String => true,
            Type::Struct(id) => self.structs[id.0]
                .fields
                .iter()
                .any(|field| self.managed(field.ty)),
            _ => false,
        }
    }
    fn retained(&self, ty: Type, value: &str) -> String {
        match ty {
            Type::String => format!("skuld_string_retain({value})"),
            Type::Struct(id) if self.managed(ty) => format!("skuld_s{}_retain({value})", id.0),
            _ => value.into(),
        }
    }
    fn release_function(&self, ty: Type) -> Option<String> {
        match ty {
            Type::String => Some("skuld_string_release".into()),
            Type::Struct(id) if self.managed(ty) => Some(format!("skuld_s{}_release", id.0)),
            _ => None,
        }
    }
    /// Every owning slot releases on every exit path, including `return`,
    /// `break` and `continue`, which C cannot express without this attribute.
    fn cleanup(&self, ty: Type) -> String {
        match self.release_function(ty) {
            Some(function) => format!("__attribute__((cleanup({function}))) "),
            None => String::new(),
        }
    }
    fn assign_function(&self, ty: Type) -> Option<String> {
        match ty {
            Type::String => Some("skuld_string_assign".into()),
            Type::Struct(id) if self.managed(ty) => Some(format!("skuld_s{}_assign", id.0)),
            _ => None,
        }
    }
    fn line(&mut self, line: &str) {
        self.output.push_str(&"    ".repeat(self.indent));
        self.output.push_str(line);
        self.output.push('\n');
    }
    /// `owned` marks a value that already carries a reference of its own, such
    /// as a fresh concatenation or a function result. A borrowed value, like
    /// reading a variable or a field, is retained as it enters the slot.
    fn store(&mut self, ty: Type, value: &str, owned: bool) -> String {
        let name = format!("skuld_t{}", self.next_temp);
        self.next_temp += 1;
        let initializer = if owned {
            value.to_string()
        } else {
            self.retained(ty, value)
        };
        let cleanup = self.cleanup(ty);
        self.line(&format!("{cleanup}{} {name} = {initializer};", c_type(ty)));
        name
    }
    fn temporary(&mut self, ty: Type, value: &str) -> String {
        self.store(ty, value, false)
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
                let initial = self.retained(*ty, &value);
                let cleanup = self.cleanup(*ty);
                self.line(&format!(
                    "{cleanup}{} skuld_v{} = {initial};",
                    c_type(*ty),
                    id.0
                ));
                self.line(&format!("(void)skuld_v{};", id.0));
            }
            StatementKind::Expression(expr) => {
                let value = self.expression(expr);
                if expr.ty != Type::Void {
                    self.line(&format!("(void){value};"));
                }
            }
            StatementKind::Return(value) => {
                let rendered = match value {
                    // The caller receives a reference of its own, because every
                    // local here is released as this function returns.
                    Some(expr) => {
                        let value = self.expression(expr);
                        self.retained(expr.ty, &value)
                    }
                    None => String::new(),
                };
                self.line(&format!("return {rendered};"));
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
                    "((skuld_string){{(const unsigned char *)\"{escaped}\", {}, NULL}})",
                    value.len()
                )
            }
            ExprKind::Local(id) => self.temporary(expr.ty, &format!("skuld_v{}", id.0)),
            ExprKind::StructLiteral { id, fields } => {
                // Fields are evaluated in declaration order into temporaries
                // first, so the initializer itself contains no side effects.
                let values: Vec<_> = fields
                    .iter()
                    .map(|field| {
                        let value = self.expression(field);
                        self.retained(field.ty, &value)
                    })
                    .collect();
                self.store(
                    expr.ty,
                    &format!("(skuld_s{}){{{}}}", id.0, values.join(", ")),
                    true,
                )
            }
            ExprKind::Field { object, index } => {
                let value = self.expression(object);
                self.temporary(expr.ty, &format!("{value}.f{index}"))
            }
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
                    // Concatenation is the only binary result that owns memory,
                    // and it is freshly allocated.
                    let fresh = expr.ty == Type::String;
                    self.store(expr.ty, &result, fresh)
                }
            }
            ExprKind::Assignment {
                target,
                op,
                value,
                op_span,
            } => {
                let place = place_expression(target);
                // Compound assignment snapshots the old value before its RHS.
                let old = if *op != AssignmentOp::Assign {
                    Some(self.temporary(expr.ty, &place))
                } else {
                    None
                };
                let value = self.expression(value);
                // A compound assignment computes a fresh value; a plain one
                // copies an existing value and must retain it.
                let (result, fresh) = if let Some(old) = old {
                    let op = match op {
                        AssignmentOp::Add => BinaryOp::Add,
                        AssignmentOp::Subtract => BinaryOp::Subtract,
                        AssignmentOp::Multiply => BinaryOp::Multiply,
                        AssignmentOp::Divide => BinaryOp::Divide,
                        AssignmentOp::Assign => unreachable!(),
                    };
                    (binary_value(op, expr.ty, &old, &value, op_span.start), true)
                } else {
                    (value, false)
                };
                // A plain assignment already holds the value in a slot of its
                // own; only a fresh result needs one.
                let result = if fresh {
                    self.store(expr.ty, &result, true)
                } else {
                    result
                };
                match self.assign_function(expr.ty) {
                    Some(assign) => self.line(&format!("{assign}(&{place}, {result});")),
                    None => self.line(&format!("{place} = {result};")),
                }
                result
            }
            ExprKind::Call { target, arguments } => {
                let values: Vec<_> = arguments.iter().map(|arg| self.expression(arg)).collect();
                let call = match target {
                    CallTarget::Function(id) => format!("skuld_f{}({})", id.0, values.join(", ")),
                    CallTarget::Print if arguments.is_empty() => format!(
                        "skuld_print_string((skuld_string){{(const unsigned char *)\"\", 0, NULL}}, {})",
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
                    // A callee returns a reference of its own; adopt it.
                    self.store(expr.ty, &call, true)
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
    if ty == Type::String && op == Add {
        return format!("skuld_string_concat({left}, {right}, {byte})");
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

const PRELUDE_HEAD: &str = r#"/* Generated by Skuld. C11, compiled with clang. */
#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <float.h>
_Static_assert(DBL_MANT_DIG == 53 && DBL_MAX_EXP == 1024, "Skuld requires binary64 double");

static inline void skuld_fail(const char *message, size_t byte) {
    fprintf(stderr, "runtime error: %s (source byte %zu)\n", message, byte);
    exit(1);
}
"#;

/// The managed-memory runtime is real C in `runtime/`, embedded verbatim so
/// there is one source of truth for retain and release.
const RUNTIME: &str = include_str!("../../runtime/strings.c");

const PRELUDE_TAIL: &str = r#"
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
