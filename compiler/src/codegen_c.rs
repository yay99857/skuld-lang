//! Readable C11 generation from typed HIR only. No source syntax or name lookup.
use crate::{hir::*, type_checker::StructInfo, types::Type};

pub fn emit_c(program: &Program) -> String {
    let mut emitter = Emitter {
        output: format!("{PRELUDE_HEAD}{RUNTIME}{PRELUDE_TAIL}"),
        indent: 0,
        next_temp: 0,
        structs: program.structs.clone(),
        arrays: program.arrays.clone(),
        options: program.options.clone(),
    };
    for index in 0..program.arrays.len() {
        emitter.line(&format!("typedef struct skuld_a{index} skuld_a{index};"));
    }
    // A class is a pointer, so it only needs a name before its body; this also
    // lets a class refer to itself.
    for (index, declaration) in program.structs.iter().enumerate() {
        if declaration.reference {
            emitter.line(&format!("typedef struct skuld_s{index} skuld_s{index};"));
        }
    }
    // Inline structs and Options must be complete before embedding them.
    let types: Vec<_> = (0..program.structs.len())
        .map(|i| Type::Struct(crate::types::StructId(i)))
        .chain((0..program.options.len()).map(|i| Type::Option(crate::types::OptionId(i))))
        .collect();
    let mut order = Vec::new();
    while order.len() < types.len() {
        let before = order.len();
        for &ty in &types {
            if order.contains(&ty) {
                continue;
            }
            let complete = |field: Type| match field {
                Type::Struct(id) => program.structs[id.0].reference || order.contains(&field),
                Type::Option(_) => order.contains(&field),
                _ => true,
            };
            let ready = match ty {
                Type::Struct(id) => program.structs[id.0]
                    .fields
                    .iter()
                    .all(|field| complete(field.ty)),
                Type::Option(id) => complete(program.options[id.0].element),
                _ => unreachable!(),
            };
            if ready {
                order.push(ty);
            }
        }
        if order.len() == before {
            unreachable!("internal compiler bug: value type cycle reached backend");
        }
    }
    for ty in order {
        let index = match ty {
            Type::Option(id) => {
                emitter.line(&format!(
                    "typedef struct {{ bool some; {} value; }} skuld_o{};",
                    emitter.c_type(program.options[id.0].element),
                    id.0
                ));
                continue;
            }
            Type::Struct(id) => id.0,
            _ => unreachable!(),
        };
        let declaration = &program.structs[index];
        emitter.line("");
        emitter.line(&format!(
            "/* {} {}: source bytes {}..{} */",
            if declaration.reference {
                "class"
            } else {
                "struct"
            },
            declaration.name,
            declaration.span.start,
            declaration.span.end
        ));
        if declaration.reference {
            emitter.line(&format!("struct skuld_s{index} {{"));
        } else {
            emitter.line("typedef struct {");
        }
        emitter.indent += 1;
        if declaration.reference {
            emitter.line("skuld_object header;");
        }
        for (position, field) in declaration.fields.iter().enumerate() {
            emitter.line(&format!(
                "{} f{position}; /* {} */",
                emitter.c_type(field.ty),
                field.name
            ));
        }
        emitter.indent -= 1;
        if declaration.reference {
            emitter.line("};");
        } else {
            emitter.line(&format!("}} skuld_s{index};"));
        }
    }
    // Complete array layouts after value types; arrays themselves are pointers.
    for (index, array) in program.arrays.iter().enumerate() {
        emitter.line(&format!(
            "struct skuld_a{index} {{ skuld_object header; size_t len; {} data[]; }};",
            emitter.c_type(array.element)
        ));
    }
    let managed: Vec<_> = (0..program.structs.len())
        .map(|i| Type::Struct(crate::types::StructId(i)))
        .chain((0..program.arrays.len()).map(|i| Type::Array(crate::types::ArrayId(i))))
        .chain((0..program.options.len()).map(|i| Type::Option(crate::types::OptionId(i))))
        .filter(|ty| emitter.managed(*ty))
        .collect();
    // Prototypes permit forward references and mutually referring classes.
    for ty in &managed {
        let name = emitter.c_type(*ty);
        let prefix = emitter.aggregate_prefix(*ty);
        emitter.line(&format!(
            "static inline {name} {prefix}_retain({name} value);"
        ));
        emitter.line(&format!(
            "static inline void {prefix}_release({name} *slot);"
        ));
        emitter.line(&format!(
            "static inline void {prefix}_assign({name} *slot, {name} value);"
        ));
    }
    for ty in managed {
        emitter.aggregate_helpers(ty);
    }
    if !program.structs.is_empty() {
        emitter.line("");
    }
    for function in &program.functions {
        emitter.line(&format!("{};", emitter.signature(function)));
    }
    for function in &program.functions {
        emitter.line("");
        emitter.line(&format!(
            "/* {}: source bytes {}..{} */",
            function.name, function.span.start, function.span.end
        ));
        emitter.line(&format!("{} {{", emitter.signature(function)));
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
fn string_literal(value: &str) -> String {
    // Fixed-width octal for every byte: no injection, NUL truncation, trigraphs
    // or dependence on the C source character set.
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
fn signature_of(structs: &[StructInfo], function: &Function) -> String {
    let params = if function.parameters.is_empty() {
        "void".into()
    } else {
        function
            .parameters
            .iter()
            .map(|p| format!("{} skuld_v{}", type_name(structs, p.ty), p.id.0))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{} skuld_f{}({params})",
        type_name(structs, function.return_type),
        function.id.0
    )
}
struct Emitter {
    output: String,
    indent: usize,
    next_temp: usize,
    /// Needed to decide which types own a reference and must be released.
    structs: Vec<StructInfo>,
    arrays: Vec<crate::types::ArrayInfo>,
    options: Vec<crate::types::OptionInfo>,
}
fn type_name(structs: &[StructInfo], ty: Type) -> String {
    match ty {
        Type::Int => "int64_t".into(),
        Type::Float => "double".into(),
        Type::Bool => "bool".into(),
        Type::String => "skuld_string".into(),
        Type::Void => "void".into(),
        Type::Weak(_) => "skuld_weak".into(),
        Type::Option(id) => format!("skuld_o{}", id.0),
        Type::Array(id) => format!("skuld_a{} *", id.0),
        // A class value is a pointer to a shared object; a struct is the
        // object itself, and C assignment copies it, which is value semantics.
        Type::Struct(id) if structs[id.0].reference => format!("skuld_s{} *", id.0),
        Type::Struct(id) => format!("skuld_s{}", id.0),
        Type::Error => unreachable!("internal compiler bug: error type in HIR"),
    }
}
impl Emitter {
    fn option_helpers(&mut self, id: crate::types::OptionId) {
        let name = format!("skuld_o{}", id.0);
        let element = self.options[id.0].element;
        self.line(&format!(
            "static inline {name} {name}_retain({name} value) {{"
        ));
        self.indent += 1;
        self.line(&format!(
            "if (value.some) value.value = {};",
            self.retained(element, "value.value")
        ));
        self.line("return value;");
        self.indent -= 1;
        self.line("}");
        self.line(&format!(
            "static inline void {name}_release({name} *slot) {{"
        ));
        self.indent += 1;
        self.line(&format!(
            "if (slot->some) {}(&slot->value);",
            self.release_function(element)
                .expect("managed Option payload")
        ));
        self.line("slot->some = false;");
        self.indent -= 1;
        self.line("}");
        self.line(&format!(
            "static inline void {name}_assign({name} *slot, {name} value) {{"
        ));
        self.indent += 1;
        self.line(&format!("{name} previous = *slot;"));
        self.line(&format!("*slot = {name}_retain(value);"));
        self.line(&format!("{name}_release(&previous);"));
        self.indent -= 1;
        self.line("}");
    }
    fn aggregate_prefix(&self, ty: Type) -> String {
        match ty {
            Type::Struct(id) => format!("skuld_s{}", id.0),
            Type::Array(id) => format!("skuld_a{}", id.0),
            Type::Option(id) => format!("skuld_o{}", id.0),
            _ => unreachable!("aggregate type"),
        }
    }
    fn aggregate_helpers(&mut self, ty: Type) {
        if let Type::Option(id) = ty {
            self.option_helpers(id);
            return;
        }
        let name = self.c_type(ty);
        let prefix = self.aggregate_prefix(ty);
        let reference = match ty {
            Type::Array(_) => true,
            Type::Struct(id) => self.structs[id.0].reference,
            _ => unreachable!(),
        };
        let fields: Vec<_> = match ty {
            Type::Struct(id) => self.structs[id.0]
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| self.managed(f.ty))
                .map(|(i, f)| (i, f.ty))
                .collect(),
            _ => Vec::new(),
        };
        if reference {
            self.line(&format!(
                "static void {prefix}_destroy(skuld_object *object) {{"
            ));
            self.indent += 1;
            self.line(&format!("{name} value = ({name})object;"));
            self.line("(void)value;");
            for (index, field) in &fields {
                self.line(&format!(
                    "{}(&value->f{index});",
                    self.release_function(*field).expect("managed field")
                ));
            }
            if let Type::Array(id) = ty
                && let Some(release) = self.release_function(self.arrays[id.0].element)
            {
                self.line(&format!(
                    "for (size_t i = 0; i < value->len; ++i) {release}(&value->data[i]);"
                ));
            }
            self.indent -= 1;
            self.line("}");
        }
        self.line(&format!(
            "static inline {name} {prefix}_retain({name} value) {{"
        ));
        self.indent += 1;
        if reference {
            self.line("skuld_object_retain(&value->header);");
        } else {
            for (index, field) in &fields {
                self.line(&format!(
                    "value.f{index} = {};",
                    self.retained(*field, &format!("value.f{index}"))
                ));
            }
        }
        self.line("return value;");
        self.indent -= 1;
        self.line("}");
        self.line(&format!(
            "static inline void {prefix}_release({name} *slot) {{"
        ));
        self.indent += 1;
        if reference {
            self.line("skuld_object_release(&(*slot)->header);");
        } else {
            for (index, field) in &fields {
                self.line(&format!(
                    "{}(&slot->f{index});",
                    self.release_function(*field).expect("managed field")
                ));
            }
        }
        self.indent -= 1;
        self.line("}");
        self.line(&format!(
            "static inline void {prefix}_assign({name} *slot, {name} value) {{"
        ));
        self.indent += 1;
        self.line(&format!("{name} previous = *slot;"));
        self.line(&format!("*slot = {prefix}_retain(value);"));
        self.line(&format!("{prefix}_release(&previous);"));
        self.indent -= 1;
        self.line("}");
    }
    fn allocate(&mut self, ty: Type, count: usize, byte: usize) -> String {
        let prefix = self.aggregate_prefix(ty);
        let element = match ty {
            Type::Array(id) => format!("sizeof({})", self.c_type(self.arrays[id.0].element)),
            _ => "0".into(),
        };
        let name = self.store(
            ty,
            &format!("skuld_allocate(sizeof({prefix}), {count}, {element}, {byte})"),
            true,
        );
        self.line(&format!(
            "skuld_object_init(&{name}->header, {prefix}_destroy);"
        ));
        if matches!(ty, Type::Array(_)) {
            self.line(&format!("{name}->len = {count};"));
        }
        name
    }
    fn index_place(&mut self, object: &Expr, index: &Expr) -> String {
        let value = self.expression(object);
        let subscript = self.expression(index);
        let checked = self.temporary(
            Type::Int,
            &format!(
                "(int64_t)skuld_index({subscript}, {value}->len, {})",
                index.span.start
            ),
        );
        format!("{value}->data[{checked}]")
    }
    fn place(&mut self, place: &Place) -> String {
        match place {
            Place::Local(id) => format!("skuld_v{}", id.0),
            Place::Field { base, index } => {
                let base = self.place(base);
                format!("{base}.f{index}")
            }
            Place::ReferenceField { object, index } => {
                let object = self.expression(object);
                format!("{object}->f{index}")
            }
            Place::Index { object, index } => self.index_place(object, index),
        }
    }

    fn c_type(&self, ty: Type) -> String {
        type_name(&self.structs, ty)
    }
    fn signature(&self, function: &Function) -> String {
        signature_of(&self.structs, function)
    }
    /// A type owns references when it is a string or holds one, directly or
    /// through another struct. Unmanaged values need no retain, release or
    /// cleanup, so they cost exactly what they did before.
    fn managed(&self, ty: Type) -> bool {
        match ty {
            Type::String | Type::Array(_) | Type::Weak(_) => true,
            Type::Option(id) => self.managed(self.options[id.0].element),
            // A class always owns a reference; a struct owns one only if a
            // field does.
            Type::Struct(id) => {
                self.structs[id.0].reference
                    || self.structs[id.0]
                        .fields
                        .iter()
                        .any(|field| self.managed(field.ty))
            }
            _ => false,
        }
    }
    fn retained(&self, ty: Type, value: &str) -> String {
        match ty {
            Type::String => format!("skuld_string_retain({value})"),
            Type::Option(id) if self.managed(ty) => format!("skuld_o{}_retain({value})", id.0),
            Type::Weak(_) => format!("skuld_weak_retain({value})"),
            Type::Array(id) => format!("skuld_a{}_retain({value})", id.0),
            Type::Struct(id) if self.managed(ty) => format!("skuld_s{}_retain({value})", id.0),
            _ => value.into(),
        }
    }
    fn release_function(&self, ty: Type) -> Option<String> {
        match ty {
            Type::String => Some("skuld_string_release".into()),
            Type::Option(id) if self.managed(ty) => Some(format!("skuld_o{}_release", id.0)),
            Type::Weak(_) => Some("skuld_weak_release".into()),
            Type::Array(id) => Some(format!("skuld_a{}_release", id.0)),
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
            Type::Option(id) if self.managed(ty) => Some(format!("skuld_o{}_assign", id.0)),
            Type::Weak(_) => Some("skuld_weak_assign".into()),
            Type::Array(id) => Some(format!("skuld_a{}_assign", id.0)),
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
        self.line(&format!(
            "{cleanup}{} {name} = {initializer};",
            self.c_type(ty)
        ));
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
                    self.c_type(*ty),
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
            StatementKind::IfLet {
                binding,
                value,
                then_block,
                else_branch,
            } => {
                let Type::Option(id) = value.ty else {
                    unreachable!("checked if let")
                };
                let payload = self.options[id.0].element;
                let value = self.expression(value);
                self.line(&format!("if ({value}.some) {{"));
                self.indent += 1;
                self.line(&format!(
                    "{}{} skuld_v{} = {};",
                    self.cleanup(payload),
                    self.c_type(payload),
                    binding.0,
                    self.retained(payload, &format!("{value}.value"))
                ));
                self.line(&format!("(void)skuld_v{};", binding.0));
                self.block_contents(then_block);
                self.indent -= 1;
                self.line("}");
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
                let values: Vec<_> = fields
                    .iter()
                    .map(|(index, field)| {
                        let value = self.expression(field);
                        (*index, self.retained(field.ty, &value))
                    })
                    .collect();
                if !self.structs[id.0].reference {
                    let initializers: Vec<_> = values
                        .iter()
                        .map(|(index, value)| format!(".f{index} = {value}"))
                        .collect();
                    return self.store(
                        expr.ty,
                        &format!("(skuld_s{}){{{}}}", id.0, initializers.join(", ")),
                        true,
                    );
                }
                let name = self.allocate(expr.ty, 0, expr.span.start);
                for (position, value) in values {
                    self.line(&format!("{name}->f{position} = {value};"));
                }
                name
            }
            ExprKind::Array(elements) => {
                let Type::Array(id) = expr.ty else {
                    unreachable!("checked array")
                };
                let element_type = self.arrays[id.0].element;
                let values: Vec<_> = elements
                    .iter()
                    .map(|element| self.expression(element))
                    .collect();
                let name = self.allocate(expr.ty, elements.len(), expr.span.start);
                for (position, value) in values.iter().enumerate() {
                    self.line(&format!(
                        "{name}->data[{position}] = {};",
                        self.retained(element_type, value)
                    ));
                }
                name
            }
            ExprKind::Index { object, index } => {
                let place = self.index_place(object, index);
                self.temporary(expr.ty, &place)
            }
            ExprKind::ArrayLen(object) => {
                let value = self.expression(object);
                self.temporary(Type::Int, &format!("(int64_t){value}->len"))
            }
            ExprKind::Weak(value) => {
                let value = match value {
                    Some(value) => {
                        let value = self.expression(value);
                        format!("skuld_weak_retain(&{value}->header)")
                    }
                    None => "NULL".into(),
                };
                self.store(expr.ty, &value, true)
            }
            ExprKind::WeakAlive(object) => {
                let value = self.expression(object);
                self.temporary(Type::Bool, &format!("skuld_weak_alive({value})"))
            }
            ExprKind::Some(value) => {
                let rendered = self.expression(value);
                self.store(
                    expr.ty,
                    &format!(
                        "({}){{.some = true, .value = {}}}",
                        self.c_type(expr.ty),
                        self.retained(value.ty, &rendered)
                    ),
                    true,
                )
            }
            ExprKind::None => {
                self.store(expr.ty, &format!("({}){{0}}", self.c_type(expr.ty)), true)
            }
            ExprKind::IsSome(value) | ExprKind::IsNone(value) => {
                let value = self.expression(value);
                let not = if matches!(expr.kind, ExprKind::IsNone(_)) {
                    "!"
                } else {
                    ""
                };
                self.temporary(Type::Bool, &format!("{not}{value}.some"))
            }
            ExprKind::WeakUpgrade(object) => {
                let value = self.expression(object);
                let result = self.store(expr.ty, &format!("({}){{0}}", self.c_type(expr.ty)), true);
                // The runtime returns a retained target or an internal null.
                // Adopt it directly into Some; no nullable class value enters HIR.
                let pointer = format!("skuld_t{}", self.next_temp);
                self.next_temp += 1;
                self.line(&format!("void *{pointer} = skuld_weak_upgrade({value});"));
                self.line(&format!(
                    "if ({pointer} != NULL) {{ {result}.some = true; {result}.value = {pointer}; }}"
                ));
                result
            }
            ExprKind::WeakGet(object) => {
                let value = self.expression(object);
                self.store(
                    expr.ty,
                    &format!("skuld_weak_get({value}, {})", expr.span.start),
                    true,
                )
            }
            ExprKind::Interpolation(parts) => {
                // Folded left into concatenations. Each piece becomes a string
                // that owns itself, and each intermediate result is released by
                // its own slot.
                let mut result: Option<String> = None;
                for part in parts {
                    let piece = match part {
                        InterpolationPart::Text(text) => {
                            if text.is_empty() {
                                continue;
                            }
                            self.store(Type::String, &string_literal(text), true)
                        }
                        InterpolationPart::Value(value) => {
                            let rendered = self.expression(value);
                            match value.ty {
                                Type::String => rendered,
                                Type::Int => self.store(
                                    Type::String,
                                    &format!("skuld_string_from_int({rendered})"),
                                    true,
                                ),
                                Type::Float => self.store(
                                    Type::String,
                                    &format!("skuld_string_from_float({rendered})"),
                                    true,
                                ),
                                Type::Bool => self.store(
                                    Type::String,
                                    &format!("skuld_string_from_bool({rendered})"),
                                    true,
                                ),
                                _ => unreachable!(
                                    "internal compiler bug: uncheckable interpolation part"
                                ),
                            }
                        }
                    };
                    result = Some(match result {
                        None => {
                            self.store(Type::String, &self.retained(Type::String, &piece), true)
                        }
                        Some(left) => self.store(
                            Type::String,
                            &format!("skuld_string_concat({left}, {piece}, {})", expr.span.start),
                            true,
                        ),
                    });
                }
                // An interpolation with no pieces at all is the empty string.
                match result {
                    Some(value) => value,
                    None => self.store(Type::String, &string_literal(""), true),
                }
            }
            ExprKind::Field { object, index } => {
                let value = self.expression(object);
                let arrow = match object.ty {
                    Type::Struct(id) if self.structs[id.0].reference => "->",
                    _ => ".",
                };
                self.temporary(expr.ty, &format!("{value}{arrow}f{index}"))
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
                let place = self.place(target);
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
