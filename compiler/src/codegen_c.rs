//! Readable C11 generation from typed HIR only. No source syntax or name lookup.
use crate::{hir::*, type_checker::StructInfo, types::Type};

pub fn emit_c(program: &Program, mode: crate::type_checker::Mode) -> String {
    let freestanding = mode.is_freestanding();
    let mut emitter = Emitter {
        output: if freestanding {
            format!("{FREESTANDING_HEAD}{SHARED_CHECKS}{PRELUDE_TAIL}")
        } else {
            format!(
                "{PRELUDE_HEAD}{SHARED_CHECKS}{RUNTIME}{PLATFORM}{PRELUDE_TAIL}{PRELUDE_HOSTED}"
            )
        },
        indent: 0,
        next_temp: 0,
        structs: program.structs.clone(),
        enums: program.enums.clone(),
        interfaces: program.interfaces.clone(),
        arrays: program.arrays.clone(),
        fixed_arrays: program.fixed_arrays.clone(),
        options: program.options.clone(),
        results: program.results.clone(),
        current_return: Type::Void,
        extern_names: program
            .externs
            .iter()
            .map(|function| (function.id, function.name.clone()))
            .collect(),
        captures: std::collections::BTreeSet::new(),
        lambda_captures: program
            .lambdas
            .iter()
            .map(|lambda| lambda.captures.iter().map(|c| c.id).collect())
            .collect(),
        exports: if freestanding {
            program.exports.iter().cloned().collect()
        } else {
            std::collections::BTreeMap::new()
        },
        statics: program.statics.iter().map(|(id, _)| *id).collect(),
        defers: Vec::new(),
    };
    // An interface value is a pair: the object, and the table of methods to
    // call on it. The pair is declared before the aggregates, because an array
    // or a field may hold one; the table itself stays incomplete until every
    // type it mentions is defined.
    for (index, _) in program.interfaces.iter().enumerate() {
        emitter.line(&format!("struct skuld_ivt{index};"));
        emitter.line(&format!(
            "typedef struct {{ skuld_object *object; const struct skuld_ivt{index} *vtable; }} skuld_i{index};"
        ));
        // The allocation header already carries the destructor, so counting an
        // interface value needs nothing the runtime does not already have.
        emitter.line(&format!(
            "static inline skuld_i{index} skuld_i{index}_retain(skuld_i{index} value) {{ if (value.object) skuld_object_retain(value.object); return value; }}"
        ));
        emitter.line(&format!(
            "static inline void skuld_i{index}_release(skuld_i{index} *slot) {{ if (slot->object) skuld_object_release(slot->object); slot->object = NULL; }}"
        ));
        emitter.line(&format!(
            "static inline void skuld_i{index}_assign(skuld_i{index} *slot, skuld_i{index} value) {{ skuld_i{index} previous = *slot; *slot = value; if (previous.object) skuld_object_release(previous.object); }}"
        ));
    }
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
    // Inline structs, enums, fixed arrays and Options must be complete before embedding them.
    let types: Vec<_> = (0..program.structs.len())
        .map(|i| Type::Struct(crate::types::StructId(i)))
        .chain(
            (0..program.fixed_arrays.len())
                .map(|i| Type::FixedArray(crate::types::FixedArrayId(i))),
        )
        .chain((0..program.options.len()).map(|i| Type::Option(crate::types::OptionId(i))))
        .chain((0..program.results.len()).map(|i| Type::Result(crate::types::ResultId(i))))
        .chain((0..program.enums.len()).map(|i| Type::Enum(crate::types::EnumId(i))))
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
                Type::Option(_) | Type::Result(_) | Type::FixedArray(_) => order.contains(&field),
                Type::Enum(_) => order.contains(&field),
                _ => true,
            };
            let ready = match ty {
                Type::FixedArray(id) => complete(program.fixed_arrays[id.0].element),
                Type::Struct(id) => program.structs[id.0]
                    .fields
                    .iter()
                    .all(|field| complete(field.ty)),
                Type::Option(id) => complete(program.options[id.0].element),
                Type::Result(id) => {
                    complete(program.results[id.0].ok) && complete(program.results[id.0].err)
                }
                Type::Enum(id) => program.enums[id.0]
                    .variants
                    .iter()
                    .all(|v| v.payload.is_none_or(complete)),
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
            Type::FixedArray(id) => {
                let info = &program.fixed_arrays[id.0];
                emitter.line(&format!(
                    "typedef struct {{ {} data[{}]; }} skuld_fa{};",
                    emitter.c_type(info.element),
                    info.size,
                    id.0
                ));
                continue;
            }
            Type::Option(id) => {
                emitter.line(&format!(
                    "typedef struct {{ bool some; {} value; }} skuld_o{};",
                    emitter.c_type(program.options[id.0].element),
                    id.0
                ));
                continue;
            }
            Type::Result(id) => {
                let info = program.results[id.0];
                emitter.line(&format!(
                    "typedef struct {{ int64_t tag; union {{ {} v0; {} v1; }} payload; }} skuld_r{};",
                    emitter.c_type(info.ok),
                    emitter.c_type(info.err),
                    id.0
                ));
                continue;
            }
            Type::Enum(id) => {
                let declaration = &program.enums[id.0];
                emitter.line("");
                emitter.line(&format!(
                    "/* enum {}: source id {} */",
                    declaration.name, id.0
                ));
                emitter.line("typedef struct {");
                emitter.indent += 1;
                emitter.line("int64_t tag;");
                let has_payload = declaration.variants.iter().any(|v| v.payload.is_some());
                if has_payload {
                    emitter.line("union {");
                    emitter.indent += 1;
                    for (v_index, variant) in declaration.variants.iter().enumerate() {
                        if let Some(payload_ty) = variant.payload {
                            emitter.line(&format!(
                                "{} v{v_index}; /* {} */",
                                emitter.c_type(payload_ty),
                                variant.name
                            ));
                        }
                    }
                    emitter.indent -= 1;
                    emitter.line("} payload;");
                }
                emitter.indent -= 1;
                emitter.line(&format!("}} skuld_e{};", id.0));
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
            } else if declaration.union {
                "union"
            } else {
                "struct"
            },
            declaration.name,
            declaration.span.start,
            declaration.span.end
        ));
        // An `extern struct` is laid out the way the platform's C compiler
        // lays out the same fields, which is what it is for; `packed` and
        // `align` are passed straight through to it.
        let attributes = match &declaration.layout {
            crate::ast::Layout::Foreign { packed, align } => {
                let mut parts = Vec::new();
                if *packed {
                    parts.push("packed".to_owned());
                }
                if let Some((value, _)) = align {
                    parts.push(format!("aligned({value})"));
                }
                if parts.is_empty() {
                    String::new()
                } else {
                    format!("__attribute__(({})) ", parts.join(", "))
                }
            }
            crate::ast::Layout::Skuld => String::new(),
        };
        if declaration.reference {
            emitter.line(&format!("struct skuld_s{index} {{"));
        } else if declaration.union {
            emitter.line(&format!("typedef union {attributes}{{"));
        } else {
            emitter.line(&format!("typedef struct {attributes}{{"));
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
    // Module-level storage. It is emitted after the types it may be shaped by
    // and before any function that reads it, and it is `static` in the C sense
    // too: a program is one translation unit, so nothing outside it has a name
    // to reach these by.
    if !program.statics.is_empty() {
        emitter.line("");
    }
    for (id, info) in &program.statics {
        let start = match &info.start {
            Some(value) => format!(" = {}", constant_literal(value)),
            // A fixed array starts at zero, which is what `{0}` means in C
            // however long it is.
            None => " = {0}".to_owned(),
        };
        emitter.line(&format!(
            "static {} skuld_g{}{start};",
            emitter.c_type(info.ty),
            id.0
        ));
    }
    // Foreign declarations come after the aggregates rather than first: a
    // signature may name an `extern struct`, which has to be complete before
    // it is passed or returned by value.
    for function in &program.externs {
        let parameters = if function.parameters.is_empty() {
            "void".to_owned()
        } else {
            function
                .parameters
                .iter()
                .map(|ty| type_name(&program.structs, *ty))
                .collect::<Vec<_>>()
                .join(", ")
        };
        emitter.line(&format!(
            "extern {} {}({parameters}); /* extern \"C\": source bytes {}..{} */",
            type_name(&program.structs, function.return_type),
            function.name,
            function.span.start,
            function.span.end
        ));
    }
    // A numbered enum carries what each variant is worth, and the conversion
    // back from a value to a variant. Both live next to the type rather than
    // at each call: the values are a property of the declaration.
    for (index, declaration) in program.enums.iter().enumerate() {
        let Some(underlying) = declaration.underlying else {
            continue;
        };
        let cell = underlying.c_type();
        let values = declaration
            .variants
            .iter()
            .map(|variant| format!("({cell}){}", variant.value))
            .collect::<Vec<_>>()
            .join(", ");
        emitter.line(&format!(
            "static const {cell} skuld_e{index}_values[] = {{ {values} }};"
        ));
        emitter.line(&format!(
            "static skuld_e{index} skuld_e{index}_from({cell} value, size_t byte) {{"
        ));
        emitter.indent += 1;
        emitter.line(&format!(
            "for (size_t i = 0; i < {}; i++) {{",
            declaration.variants.len()
        ));
        emitter.indent += 1;
        emitter.line(&format!(
            "if (skuld_e{index}_values[i] == value) return (skuld_e{index}){{.tag = (int64_t)i}};"
        ));
        emitter.indent -= 1;
        emitter.line("}");
        emitter.line(&format!(
            "skuld_fail(\"no `{}` variant has that value\", byte);",
            declaration.name
        ));
        // `skuld_fail` does not return, but C does not know that here.
        emitter.line(&format!("return (skuld_e{index}){{.tag = 0}};"));
        emitter.indent -= 1;
        emitter.line("}");
    }
    // Complete array layouts after value types; arrays themselves are pointers.
    for (index, array) in program.arrays.iter().enumerate() {
        emitter.line(&format!(
            "struct skuld_a{index} {{ skuld_object header; size_t len; size_t capacity; {} *data; }};",
            emitter.c_type(array.element)
        ));
    }
    let managed: Vec<_> = (0..program.structs.len())
        .map(|i| Type::Struct(crate::types::StructId(i)))
        .chain((0..program.arrays.len()).map(|i| Type::Array(crate::types::ArrayId(i))))
        .chain(
            (0..program.fixed_arrays.len())
                .map(|i| Type::FixedArray(crate::types::FixedArrayId(i))),
        )
        .chain((0..program.options.len()).map(|i| Type::Option(crate::types::OptionId(i))))
        .chain((0..program.results.len()).map(|i| Type::Result(crate::types::ResultId(i))))
        .chain((0..program.enums.len()).map(|i| Type::Enum(crate::types::EnumId(i))))
        .filter(|ty| emitter.managed(*ty))
        .collect();
    for (index, interface) in program.interfaces.iter().enumerate() {
        emitter.line(&format!("struct skuld_ivt{index} {{"));
        emitter.indent += 1;
        for method in &interface.methods {
            let parameters: Vec<String> = std::iter::once("skuld_object *".to_owned())
                .chain(
                    method
                        .parameters
                        .iter()
                        .map(|ty| type_name(&program.structs, *ty)),
                )
                .collect();
            emitter.line(&format!(
                "{} (*{})({});",
                type_name(&program.structs, method.return_type),
                method.name,
                parameters.join(", ")
            ));
        }
        emitter.indent -= 1;
        emitter.line("};");
    }
    // A function value is a pair: the code to run, and the environment the
    // code reads its captures from. Both halves are needed at once, so the
    // pair is a value rather than a bare pointer.
    for (index, info) in program.function_types.iter().enumerate() {
        let parameters: Vec<String> = std::iter::once("void *".to_owned())
            .chain(
                info.parameters
                    .iter()
                    .map(|ty| type_name(&program.structs, *ty)),
            )
            .collect();
        emitter.line(&format!(
            "typedef struct {{ {} (*code)({}); void *env; }} skuld_ft{index};",
            type_name(&program.structs, info.return_type),
            parameters.join(", ")
        ));
    }
    // One environment per lambda. C has no empty struct, so a lambda that
    // captures nothing still carries a byte.
    for lambda in &program.lambdas {
        emitter.line(&format!("struct skuld_env{} {{", lambda.index));
        emitter.indent += 1;
        if lambda.captures.is_empty() {
            emitter.line("char skuld_nothing;");
        }
        for capture in &lambda.captures {
            emitter.line(&format!(
                "{} skuld_v{};",
                type_name(&program.structs, capture.ty),
                capture.id.0
            ));
        }
        emitter.indent -= 1;
        emitter.line("};");
    }
    // Calling through a value must evaluate the pair once, so it goes through
    // a helper rather than being written twice at every call site.
    for (index, info) in program.function_types.iter().enumerate() {
        let mut parameters = vec![format!("skuld_ft{index} skuld_fn")];
        let mut arguments = vec!["skuld_fn.env".to_owned()];
        for (position, ty) in info.parameters.iter().enumerate() {
            parameters.push(format!(
                "{} skuld_p{position}",
                type_name(&program.structs, *ty)
            ));
            arguments.push(format!("skuld_p{position}"));
        }
        emitter.line(&format!(
            "static inline {} skuld_ftcall{index}({}) {{",
            type_name(&program.structs, info.return_type),
            parameters.join(", ")
        ));
        emitter.indent += 1;
        let call = format!("skuld_fn.code({})", arguments.join(", "));
        if info.return_type == Type::Void {
            emitter.line(&format!("{call};"));
        } else {
            emitter.line(&format!("return {call};"));
        }
        emitter.indent -= 1;
        emitter.line("}");
    }
    // One sort per array type that is sorted. It is stable, and it works on a
    // snapshot: a comparator that mutated the array while it ran would
    // otherwise leave the merge reading freed memory.
    for (array, comparator) in &program.sorts {
        let element = type_name(&program.structs, program.arrays[array.0].element);
        emitter.line(&format!(
            "static void skuld_sort_a{}_{}(skuld_a{} *array, skuld_ft{} cmp, size_t byte) {{",
            array.0, comparator.0, array.0, comparator.0
        ));
        emitter.indent += 1;
        for line in [
            "size_t n = array->len;".to_owned(),
            "if (n < 2) return;".to_owned(),
            format!("{element} *original = array->data;"),
            format!("{element} *src = malloc(n * sizeof({element}));"),
            format!("{element} *dst = malloc(n * sizeof({element}));"),
            "if (src == NULL || dst == NULL) skuld_fail(\"out of memory while sorting\", byte);"
                .to_owned(),
            format!("memcpy(src, array->data, n * sizeof({element}));"),
            "for (size_t width = 1; width < n; width *= 2) {".to_owned(),
            "    for (size_t start = 0; start < n; start += 2 * width) {".to_owned(),
            "        size_t middle = start + width < n ? start + width : n;".to_owned(),
            "        size_t end = start + 2 * width < n ? start + 2 * width : n;".to_owned(),
            "        size_t left = start, right = middle, out = start;".to_owned(),
            "        while (left < middle && right < end) {".to_owned(),
            // `<= 0` is what makes the sort stable: equal elements keep the
            // order they were written in.
            format!(
                "            dst[out++] = skuld_ftcall{}(cmp, src[left], src[right]) <= 0 ? src[left++] : src[right++];",
                comparator.0
            ),
            "        }".to_owned(),
            "        while (left < middle) dst[out++] = src[left++];".to_owned(),
            "        while (right < end) dst[out++] = src[right++];".to_owned(),
            "    }".to_owned(),
            format!("    {element} *swap = src; src = dst; dst = swap;"),
            "}".to_owned(),
            // A comparator that pushed or removed would have invalidated the
            // snapshot; that is a mistake, not something to paper over.
            "if (array->len != n || array->data != original) skuld_fail(\"the array changed while it was being sorted\", byte);"
                .to_owned(),
            format!("memcpy(array->data, src, n * sizeof({element}));"),
            "free(src);".to_owned(),
            "free(dst);".to_owned(),
        ] {
            emitter.line(&line);
        }
        emitter.indent -= 1;
        emitter.line("}");
    }
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
    // A lambda becomes an ordinary function whose first parameter is the
    // environment its captures were copied into.
    for lambda in &program.lambdas {
        emitter.line(&format!("{};", lambda_signature(&program.structs, lambda)));
    }
    // A declared function used as a value is reached through a thunk, so that
    // every function value has the same shape whatever it came from.
    for (id, ty) in &program.function_values {
        let info = &program.function_types[ty.0];
        let mut parameters = vec!["void *skuld_env".to_owned()];
        let mut arguments = Vec::new();
        for (position, parameter) in info.parameters.iter().enumerate() {
            parameters.push(format!(
                "{} skuld_p{position}",
                type_name(&program.structs, *parameter)
            ));
            arguments.push(format!("skuld_p{position}"));
        }
        emitter.line(&format!(
            "static {} skuld_thunk{}_{}({}) {{",
            type_name(&program.structs, info.return_type),
            id.0,
            ty.0,
            parameters.join(", ")
        ));
        emitter.indent += 1;
        emitter.line("(void)skuld_env;");
        let call = format!("{}({})", emitter.function_name(*id), arguments.join(", "));
        if info.return_type == Type::Void {
            emitter.line(&format!("{call};"));
        } else {
            emitter.line(&format!("return {call};"));
        }
        emitter.indent -= 1;
        emitter.line("}");
    }
    // One table per (class, interface) pair, reached through thunks so that
    // no call is ever made through a mismatched function pointer type.
    for (class, interface) in &program.vtables {
        let methods = &program.interfaces[interface.0].methods;
        for method in methods {
            let target = program.structs[class.0]
                .methods
                .iter()
                .find(|candidate| candidate.name == method.name)
                .expect("checked conformance");
            let mut parameters = vec!["skuld_object *skuld_self".to_owned()];
            let mut arguments = vec![format!("(skuld_s{} *)skuld_self", class.0)];
            for (position, ty) in method.parameters.iter().enumerate() {
                parameters.push(format!(
                    "{} skuld_p{position}",
                    type_name(&program.structs, *ty)
                ));
                arguments.push(format!("skuld_p{position}"));
            }
            emitter.line(&format!(
                "static {} skuld_ithunk{}_{}_{}({}) {{",
                type_name(&program.structs, method.return_type),
                interface.0,
                class.0,
                method.name,
                parameters.join(", ")
            ));
            emitter.indent += 1;
            let call = format!(
                "{}({})",
                emitter.function_name(target.id),
                arguments.join(", ")
            );
            if method.return_type == Type::Void {
                emitter.line(&format!("{call};"));
            } else {
                emitter.line(&format!("return {call};"));
            }
            emitter.indent -= 1;
            emitter.line("}");
        }
        let entries: Vec<String> = methods
            .iter()
            .map(|method| {
                format!(
                    ".{} = skuld_ithunk{}_{}_{}",
                    method.name, interface.0, class.0, method.name
                )
            })
            .collect();
        emitter.line(&format!(
            "static const struct skuld_ivt{} skuld_ivtable{}_{} = {{ {} }};",
            interface.0,
            interface.0,
            class.0,
            entries.join(", ")
        ));
    }
    for lambda in &program.lambdas {
        emitter.line("");
        emitter.line(&format!(
            "/* function value: source bytes {}..{} */",
            lambda.span.start, lambda.span.end
        ));
        emitter.line(&format!(
            "{} {{",
            lambda_signature(&program.structs, lambda)
        ));
        emitter.current_return = lambda.return_type;
        emitter.captures = lambda.captures.iter().map(|c| c.id).collect();
        emitter.indent += 1;
        emitter.line(&format!(
            "struct skuld_env{} *skuld_env = skuld_envp;",
            lambda.index
        ));
        emitter.line("(void)skuld_env;");
        for parameter in &lambda.parameters {
            emitter.line(&format!("(void)skuld_v{};", parameter.id.0));
        }
        emitter.block_contents(&lambda.body);
        emitter.indent -= 1;
        emitter.line("}");
        emitter.captures.clear();
    }
    for function in &program.functions {
        emitter.line("");
        emitter.line(&format!(
            "/* {}: source bytes {}..{} */",
            function.name, function.span.start, function.span.end
        ));
        emitter.line(&format!("{} {{", emitter.signature(function)));
        emitter.current_return = function.return_type;
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
    // A freestanding program is started by something this compiler did not
    // write — an assembly stub, a bootloader, another object file — so it is
    // given no entry point of its own, and the functions it exports are what
    // that something calls.
    if let Some(entry) = program.entry.filter(|_| !freestanding) {
        emitter.line("");
        // The arguments are taken here and nowhere else: a Skuld program reaches
        // them through the platform layer, since following `argv` is a pointer
        // read the foreign boundary does not do. The same call is where the
        // platform is put into the state the language assumes — on Windows,
        // the standard streams in binary mode, so `\n` stays one byte.
        emitter.line("int main(int argc, char **argv) {");
        emitter.indent += 1;
        emitter.line("skuld_start(argc, argv);");
        emitter.line(&format!("{}();", emitter.function_name(entry)));
        emitter.line("return fflush(stdout) == 0 ? 0 : 1;");
        emitter.indent -= 1;
        emitter.line("}");
    }
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
fn lambda_signature(structs: &[StructInfo], lambda: &Lambda) -> String {
    let mut params = vec!["void *skuld_envp".to_owned()];
    params.extend(
        lambda
            .parameters
            .iter()
            .map(|p| format!("{} skuld_v{}", type_name(structs, p.ty), p.id.0)),
    );
    format!(
        "static {} skuld_lam{}({})",
        type_name(structs, lambda.return_type),
        lambda.index,
        params.join(", ")
    )
}
fn signature_of(structs: &[StructInfo], function: &Function, name: &str) -> String {
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
        "{} {name}({params})",
        type_name(structs, function.return_type)
    )
}
struct Emitter<'a> {
    output: String,
    indent: usize,
    next_temp: usize,
    /// Needed to decide which types own a reference and must be released.
    structs: Vec<StructInfo>,
    enums: Vec<crate::types::EnumInfo>,
    interfaces: Vec<crate::types::InterfaceInfo>,
    arrays: Vec<crate::types::ArrayInfo>,
    fixed_arrays: Vec<crate::types::FixedArrayInfo>,
    options: Vec<crate::types::OptionInfo>,
    results: Vec<crate::types::ResultInfo>,
    /// The enclosing function's return type, which `?` needs to build its
    /// early `Err` return without carrying it through the whole HIR.
    current_return: Type,
    /// Linker names for foreign functions, which are emitted verbatim.
    extern_names: std::collections::BTreeMap<crate::resolver::SymbolId, String>,
    /// While a lambda body is being emitted, the symbols that live in its
    /// environment rather than in a local of their own.
    captures: std::collections::BTreeSet<crate::resolver::SymbolId>,
    /// What each lambda captures, by index, so that building one can fill its
    /// environment without reaching back into the HIR.
    lambda_captures: Vec<Vec<crate::resolver::SymbolId>>,
    /// The functions and statics a freestanding program exports under the
    /// names they were written with, so that whatever starts the program — an
    /// assembly stub, another object file — has something to call. It is empty
    /// in a hosted program, where every generated name stays generated.
    exports: std::collections::BTreeMap<crate::resolver::SymbolId, String>,
    /// Module-level storage, by symbol. A read or a write of one of these
    /// names is an ordinary read or write of a file-scope variable, which is
    /// why it only has to be known here.
    statics: std::collections::BTreeSet<crate::resolver::SymbolId>,
    /// One frame per block being emitted, holding that block's `defer`red
    /// statements in the order they were registered.
    ///
    /// A block emits its own frame in reverse as it ends; a `return` emits
    /// every frame, innermost first; a `break` or `continue` emits the frames
    /// down to the loop it leaves. The statements are emitted again at each
    /// exit rather than jumped to, which is what keeps them able to read the
    /// locals they were written next to.
    defers: Vec<DeferScope<'a>>,
}

/// The `defer`red statements of one block, and whether leaving that block is
/// what `break` and `continue` do.
struct DeferScope<'a> {
    statements: Vec<&'a Statement>,
    loop_body: bool,
}
/// The C spelling of a value a static starts at. It is a constant expression
/// in C too, since a static is initialised before anything runs.
fn constant_literal(value: &crate::types::ConstValue) -> String {
    match value {
        crate::types::ConstValue::Int(written, kind) if !kind.signed() => {
            format!("UINT64_C({})", *written as u64)
        }
        crate::types::ConstValue::Int(written, _) if *written < 0 => {
            format!("(-INT64_C({}))", written.unsigned_abs())
        }
        crate::types::ConstValue::Int(written, _) => format!("INT64_C({written})"),
        crate::types::ConstValue::Float(written) => format!("{written:.17e}"),
        crate::types::ConstValue::Bool(written) => written.to_string(),
        crate::types::ConstValue::Char(written) => format!("((uint32_t){})", *written as u32),
        // A string is managed, so the checker never lets one reach here.
        crate::types::ConstValue::String(_) => {
            unreachable!("internal compiler bug: a static holding a string")
        }
    }
}

fn type_name(structs: &[StructInfo], ty: Type) -> String {
    match ty {
        Type::Int(kind) => kind.c_type().into(),
        Type::Float => "double".into(),
        Type::Bool => "bool".into(),
        Type::Char => "uint32_t".into(),
        Type::String => "skuld_string".into(),
        Type::Void => "void".into(),
        Type::Weak(_) => "skuld_weak".into(),
        Type::Pointer(pointee) => format!("{} *", pointee.c_type()),
        Type::Option(id) => format!("skuld_o{}", id.0),
        Type::Result(id) => format!("skuld_r{}", id.0),
        Type::Enum(id) => format!("skuld_e{}", id.0),
        Type::Array(id) => format!("skuld_a{} *", id.0),
        Type::FixedArray(id) => format!("skuld_fa{}", id.0),
        // A class value is a pointer to a shared object; a struct is the
        // object itself, and C assignment copies it, which is value semantics.
        Type::Function(id) => format!("skuld_ft{}", id.0),
        Type::Interface(id) => format!("skuld_i{}", id.0),
        Type::Struct(id) if structs[id.0].reference => format!("skuld_s{} *", id.0),
        Type::Struct(id) => format!("skuld_s{}", id.0),
        Type::Error => unreachable!("internal compiler bug: error type in HIR"),
    }
}
impl<'a> Emitter<'a> {
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
            Type::FixedArray(id) => format!("skuld_fa{}", id.0),
            Type::Option(id) => format!("skuld_o{}", id.0),
            Type::Result(id) => format!("skuld_r{}", id.0),
            Type::Enum(id) => format!("skuld_e{}", id.0),
            _ => unreachable!("aggregate type"),
        }
    }
    fn enum_helpers(&mut self, id: crate::types::EnumId) {
        let managed_variants: Vec<(usize, Type)> = self.enums[id.0]
            .variants
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.payload.filter(|ty| self.managed(*ty)).map(|ty| (i, ty)))
            .collect();
        self.tagged_helpers(format!("skuld_e{}", id.0), &managed_variants);
    }
    fn result_helpers(&mut self, id: crate::types::ResultId) {
        let info = self.results[id.0];
        let managed_variants: Vec<(usize, Type)> = [
            (crate::types::ResultInfo::OK, info.ok),
            (crate::types::ResultInfo::ERR, info.err),
        ]
        .into_iter()
        .filter(|(_, ty)| self.managed(*ty))
        .collect();
        self.tagged_helpers(format!("skuld_r{}", id.0), &managed_variants);
    }
    /// Retain, release and assign for an inline tag-plus-payload value. Enums
    /// and `Result` share one layout, so they share one implementation.
    fn tagged_helpers(&mut self, name: String, managed_variants: &[(usize, Type)]) {
        self.line(&format!(
            "static inline {name} {name}_retain({name} value) {{"
        ));
        self.indent += 1;
        if !managed_variants.is_empty() {
            self.line("switch (value.tag) {");
            self.indent += 1;
            for &(index, payload) in managed_variants {
                self.line(&format!("case {index}:"));
                self.indent += 1;
                let retained = self.retained(payload, &format!("value.payload.v{index}"));
                self.line(&format!("value.payload.v{index} = {retained};"));
                self.line("break;");
                self.indent -= 1;
            }
            self.line("default: break;");
            self.indent -= 1;
            self.line("}");
        }
        self.line("return value;");
        self.indent -= 1;
        self.line("}");

        self.line(&format!(
            "static inline void {name}_release({name} *slot) {{"
        ));
        self.indent += 1;
        if !managed_variants.is_empty() {
            self.line("switch (slot->tag) {");
            self.indent += 1;
            for &(index, payload) in managed_variants {
                let release_fn = self.release_function(payload).unwrap();
                self.line(&format!("case {index}:"));
                self.indent += 1;
                self.line(&format!("{release_fn}(&slot->payload.v{index});"));
                self.line("break;");
                self.indent -= 1;
            }
            self.line("default: break;");
            self.indent -= 1;
            self.line("}");
        }
        self.line("slot->tag = -1;");
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
    fn fixed_array_helpers(&mut self, id: crate::types::FixedArrayId) {
        let name = format!("skuld_fa{}", id.0);
        let info = &self.fixed_arrays[id.0];
        let element = info.element;
        let size = info.size;
        let element_managed = self.managed(element);

        self.line(&format!(
            "static inline {name} {name}_retain({name} value) {{"
        ));
        self.indent += 1;
        if element_managed {
            self.line(&format!(
                "for (size_t i = 0; i < {size}; ++i) value.data[i] = {};",
                self.retained(element, "value.data[i]")
            ));
        }
        self.line("return value;");
        self.indent -= 1;
        self.line("}");

        self.line(&format!(
            "static inline void {name}_release({name} *slot) {{"
        ));
        self.indent += 1;
        if element_managed {
            let release = self.release_function(element).expect("managed element");
            self.line(&format!(
                "for (size_t i = 0; i < {size}; ++i) {release}(&slot->data[i]);"
            ));
        }
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
    fn aggregate_helpers(&mut self, ty: Type) {
        if let Type::FixedArray(id) = ty {
            self.fixed_array_helpers(id);
            return;
        }
        if let Type::Option(id) = ty {
            self.option_helpers(id);
            return;
        }
        if let Type::Result(id) = ty {
            self.result_helpers(id);
            return;
        }
        if let Type::Enum(id) = ty {
            self.enum_helpers(id);
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
            if matches!(ty, Type::Array(_)) {
                self.line("free(value->data);");
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
    fn allocate(&mut self, ty: Type, count: &str, byte: usize) -> String {
        let prefix = self.aggregate_prefix(ty);
        let name = self.store(
            ty,
            &format!("skuld_allocate(sizeof({prefix}), 0, 0, {byte})"),
            true,
        );
        self.line(&format!(
            "skuld_object_init(&{name}->header, {prefix}_destroy);"
        ));
        if let Type::Array(id) = ty {
            self.line(&format!(
                "{name}->len = 0; {name}->capacity = 0; {name}->data = NULL;"
            ));
            self.line(&format!("{name}->data = skuld_array_reserve({name}->data, &{name}->capacity, {count}, sizeof({}), {byte});", self.c_type(self.arrays[id.0].element)));
            self.line(&format!("{name}->len = {count};"));
        }
        name
    }
    fn array_call(
        &mut self,
        object: &Expr,
        method: ArrayMethod,
        arguments: &[Expr],
        expr: &Expr,
    ) -> String {
        let Type::Array(id) = object.ty else {
            unreachable!("checked array method")
        };
        let element = self.arrays[id.0].element;
        let array = self.expression(object);
        // Kept before the arguments become text, since a sort needs the
        // comparator's signature to name the helper it calls.
        let first_type = arguments.first().map(|argument| argument.ty);
        let arguments: Vec<_> = arguments.iter().map(|arg| self.expression(arg)).collect();
        let byte = expr.span.start;
        match method {
            ArrayMethod::Sort => {
                let Some(Type::Function(comparator)) = first_type else {
                    unreachable!("checked comparator")
                };
                self.line(&format!(
                    "skuld_sort_a{}_{}({array}, {}, {byte});",
                    id.0, comparator.0, arguments[0]
                ));
                String::new()
            }
            ArrayMethod::ToSorted => {
                let Some(Type::Function(comparator)) = first_type else {
                    unreachable!("checked comparator")
                };
                let result = self.allocate(expr.ty, &format!("{array}->len"), byte);
                self.line(&format!("for (size_t i = 0; i < {result}->len; ++i) {{"));
                self.indent += 1;
                let source = format!("{array}->data[i]");
                self.line(&format!(
                    "{result}->data[i] = {};",
                    self.retained(element, &source)
                ));
                self.indent -= 1;
                self.line("}");
                self.line(&format!(
                    "skuld_sort_a{}_{}({result}, {}, {byte});",
                    id.0, comparator.0, arguments[0]
                ));
                result
            }
            // Appending is its own emission rather than an insertion at the
            // end. Sharing one made the hot operation carry a move whose
            // condition is always false, and left the reader to work out that
            // it never runs.
            ArrayMethod::Push => {
                self.line(&format!("{array}->data = skuld_array_reserve({array}->data, &{array}->capacity, skuld_array_next_length({array}->len, {byte}), sizeof({}), {byte});", self.c_type(element)));
                self.line(&format!(
                    "{array}->data[{array}->len] = {};",
                    self.retained(element, &arguments[0])
                ));
                self.line(&format!("{array}->len += 1;"));
                String::new()
            }
            ArrayMethod::Insert => {
                let index = self.temporary(
                    Type::INT,
                    &format!(
                        "(int64_t)skuld_insert_index({}, {array}->len, {byte})",
                        arguments[0]
                    ),
                );
                self.line(&format!("{array}->data = skuld_array_reserve({array}->data, &{array}->capacity, skuld_array_next_length({array}->len, {byte}), sizeof({}), {byte});", self.c_type(element)));
                self.line(&format!("if ((size_t){index} < {array}->len) memmove(&{array}->data[{index} + 1], &{array}->data[{index}], ({array}->len - (size_t){index}) * sizeof({}));", self.c_type(element)));
                self.line(&format!(
                    "{array}->data[{index}] = {};",
                    self.retained(element, &arguments[1])
                ));
                self.line(&format!("{array}->len += 1;"));
                String::new()
            }
            ArrayMethod::Pop | ArrayMethod::Remove => {
                let result = self.store(expr.ty, &format!("({}){{0}}", self.c_type(expr.ty)), true);
                let (condition, index) = if matches!(method, ArrayMethod::Pop) {
                    (format!("{array}->len != 0"), format!("{array}->len - 1"))
                } else {
                    (
                        format!(
                            "{} >= 0 && (uint64_t){} < {array}->len",
                            arguments[0], arguments[0]
                        ),
                        arguments[0].clone(),
                    )
                };
                self.line(&format!("if ({condition}) {{"));
                self.indent += 1;
                let index = self.temporary(Type::INT, &format!("(int64_t)({index})"));
                // Move ownership out; the removed slot no longer owns it.
                self.line(&format!(
                    "{result}.some = true; {result}.value = {array}->data[{index}];"
                ));
                self.line(&format!("{array}->len -= 1;"));
                self.line(&format!("if ((size_t){index} < {array}->len) memmove(&{array}->data[{index}], &{array}->data[{index} + 1], ({array}->len - (size_t){index}) * sizeof({}));", self.c_type(element)));
                self.line(&format!(
                    "memset(&{array}->data[{array}->len], 0, sizeof({}));",
                    self.c_type(element)
                ));
                self.indent -= 1;
                self.line("}");
                result
            }
        }
    }
    /// The C expression for storage reached only through bindings nothing can
    /// reassign, and fields of them: `name`, `this->f0`, `this->f0->f1`.
    ///
    /// Reading through one of these borrows rather than counts, because the
    /// bindings on the way are themselves what keep the value alive. It is
    /// only safe where nothing runs between the read and the use, which is why
    /// every caller pairs it with [`Self::runs_no_code`] on the other operand.
    fn settled_storage(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Local { id, settled } => settled.then(|| self.local_name(*id)),
            ExprKind::Field { object, index } => {
                let base = self.settled_storage(object)?;
                let arrow = match object.ty {
                    Type::Struct(id) if self.structs[id.0].reference => "->",
                    _ => ".",
                };
                Some(format!("{base}{arrow}f{index}"))
            }
            _ => None,
        }
    }

    /// Whether evaluating this expression can run code that frees something —
    /// a call, an allocation, a concatenation. Arithmetic and reads cannot: a
    /// trap ends the process rather than releasing anything, so a bounds check
    /// on the way is not a reason to take a count.
    fn runs_no_code(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Local { .. } => true,
            ExprKind::FloatConvert(operand) | ExprKind::CharConvert(operand) => {
                self.runs_no_code(operand)
            }
            ExprKind::Field { object, .. }
            | ExprKind::StringLen(object)
            | ExprKind::ArrayLen(object) => self.runs_no_code(object),
            ExprKind::Unary { operand, .. } => self.runs_no_code(operand),
            ExprKind::Binary { left, right, .. } => {
                self.runs_no_code(left) && self.runs_no_code(right)
            }
            ExprKind::Index { object, index } => {
                self.runs_no_code(object) && self.runs_no_code(index)
            }
            _ => false,
        }
    }

    /// Reading one element, which checks the index once. A place has to check
    /// again where it is used; see [`Self::index_place`].
    fn index_read(&mut self, object: &Expr, index: &Expr) -> String {
        let value = self.expression(object);
        let subscript = self.expression(index);
        format!(
            "{value}->data[skuld_index({subscript}, {value}->len, {})]",
            index.span.start
        )
    }

    fn index_place(&mut self, object: &Expr, index: &Expr) -> String {
        let value = self.expression(object);
        let subscript = self.expression(index);
        let checked = self.temporary(
            Type::INT,
            &format!(
                "(int64_t)skuld_index({subscript}, {value}->len, {})",
                index.span.start
            ),
        );
        // Re-read storage and recheck bounds whenever the place is used: an
        // assignment RHS can grow, shrink or reorder this shared array.
        format!(
            "{value}->data[skuld_index({checked}, {value}->len, {})]",
            index.span.start
        )
    }
    fn place(&mut self, place: &Place) -> String {
        match place {
            Place::Local(id) => self.local_name(*id),
            Place::Field { base, index } => {
                let base = self.place(base);
                format!("{base}.f{index}")
            }
            Place::ReferenceField { object, index } => {
                let object = self.expression(object);
                format!("{object}->f{index}")
            }
            Place::Index { object, index } => self.index_place(object, index),
            Place::FixedIndex { base, index, size } => {
                let base = self.place(base);
                let subscript = self.expression(index);
                format!(
                    "{base}.data[skuld_index({subscript}, {size}, {})]",
                    index.span.start
                )
            }
        }
    }

    fn lvalue(&mut self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Local { id, .. } => Some(self.local_name(*id)),
            ExprKind::Field { object, index } => {
                if let Type::Struct(id) = object.ty
                    && self.structs[id.0].reference
                {
                    let obj = self.expression(object);
                    Some(format!("{obj}->f{index}"))
                } else {
                    let base = self.lvalue(object)?;
                    Some(format!("{base}.f{index}"))
                }
            }
            ExprKind::Index { object, index } => {
                if let Type::FixedArray(fa_id) = object.ty {
                    let base = self.lvalue(object)?;
                    let size = self.fixed_arrays[fa_id.0].size;
                    let subscript = self.expression(index);
                    Some(format!(
                        "{base}.data[skuld_index({subscript}, {size}, {})]",
                        index.span.start
                    ))
                } else if let Type::Array(_) = object.ty {
                    let obj = self.expression(object);
                    let subscript = self.expression(index);
                    Some(format!(
                        "{obj}->data[skuld_index({subscript}, {obj}->len, {})]",
                        index.span.start
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Where a name lives: a local of its own, or a field of the environment
    /// the enclosing lambda was handed.
    /// What a function is called in the generated C: its own name where the
    /// program exports it, and a generated one everywhere else.
    fn function_name(&self, id: crate::resolver::SymbolId) -> String {
        match self.exports.get(&id) {
            Some(name) => name.clone(),
            None => format!("skuld_f{}", id.0),
        }
    }
    fn local_name(&self, id: crate::resolver::SymbolId) -> String {
        if self.statics.contains(&id) {
            return format!("skuld_g{}", id.0);
        }
        if self.captures.contains(&id) {
            format!("skuld_env->skuld_v{}", id.0)
        } else {
            format!("skuld_v{}", id.0)
        }
    }
    fn c_type(&self, ty: Type) -> String {
        type_name(&self.structs, ty)
    }
    fn signature(&self, function: &Function) -> String {
        let signature = signature_of(&self.structs, function, &self.function_name(function.id));
        // A whole program is emitted as one translation unit, so nothing
        // outside it can name a generated function, and external linkage
        // only obliges the C compiler to keep every one of them: a function
        // it may not discard is a function it must emit, and a call inside
        // that function is a symbol the linker must then resolve. One
        // `import` therefore drags in every function of that module, the
        // unused ones included, along with everything they call. Internal
        // linkage lets the C compiler drop what the program never reaches.
        // A freestanding program is the exception and says so through its
        // exports: it has no `main`, and what calls it is linked separately.
        if self.exports.contains_key(&function.id) {
            signature
        } else {
            format!("static {signature}")
        }
    }
    /// A type owns references when it is a string or holds one, directly or
    /// through another struct. Unmanaged values need no retain, release or
    /// cleanup, so they cost exactly what they did before.
    fn managed(&self, ty: Type) -> bool {
        match ty {
            Type::String | Type::Array(_) | Type::Weak(_) | Type::Interface(_) => true,
            Type::FixedArray(id) => self.managed(self.fixed_arrays[id.0].element),
            Type::Option(id) => self.managed(self.options[id.0].element),
            Type::Result(id) => {
                self.managed(self.results[id.0].ok) || self.managed(self.results[id.0].err)
            }
            Type::Enum(id) => self.enums[id.0]
                .variants
                .iter()
                .any(|v| v.payload.is_some_and(|p| self.managed(p))),
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
            Type::Result(id) if self.managed(ty) => format!("skuld_r{}_retain({value})", id.0),
            Type::Weak(_) => format!("skuld_weak_retain({value})"),
            Type::Interface(id) => format!("skuld_i{}_retain({value})", id.0),
            Type::Array(id) => format!("skuld_a{}_retain({value})", id.0),
            Type::FixedArray(id) if self.managed(ty) => format!("skuld_fa{}_retain({value})", id.0),
            Type::Struct(id) if self.managed(ty) => format!("skuld_s{}_retain({value})", id.0),
            Type::Enum(id) if self.managed(ty) => format!("skuld_e{}_retain({value})", id.0),
            _ => value.into(),
        }
    }
    fn release_function(&self, ty: Type) -> Option<String> {
        match ty {
            Type::String => Some("skuld_string_release".into()),
            Type::Option(id) if self.managed(ty) => Some(format!("skuld_o{}_release", id.0)),
            Type::Result(id) if self.managed(ty) => Some(format!("skuld_r{}_release", id.0)),
            Type::Weak(_) => Some("skuld_weak_release".into()),
            Type::Interface(id) => Some(format!("skuld_i{}_release", id.0)),
            Type::Array(id) => Some(format!("skuld_a{}_release", id.0)),
            Type::FixedArray(id) if self.managed(ty) => Some(format!("skuld_fa{}_release", id.0)),
            Type::Struct(id) if self.managed(ty) => Some(format!("skuld_s{}_release", id.0)),
            Type::Enum(id) if self.managed(ty) => Some(format!("skuld_e{}_release", id.0)),
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
            Type::Result(id) if self.managed(ty) => Some(format!("skuld_r{}_assign", id.0)),
            Type::Weak(_) => Some("skuld_weak_assign".into()),
            Type::Interface(id) => Some(format!("skuld_i{}_assign", id.0)),
            Type::Array(id) => Some(format!("skuld_a{}_assign", id.0)),
            Type::FixedArray(id) if self.managed(ty) => Some(format!("skuld_fa{}_assign", id.0)),
            Type::Struct(id) if self.managed(ty) => Some(format!("skuld_s{}_assign", id.0)),
            Type::Enum(id) if self.managed(ty) => Some(format!("skuld_e{}_assign", id.0)),
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
    /// The value a `return` is about to hand over, settled into a local of its
    /// own before the deferred statements run.
    ///
    /// It carries no cleanup attribute on purpose: the reference in it belongs
    /// to the caller from here on, and releasing it as this frame ends would
    /// be releasing what was just returned.
    fn settled_return(&mut self, ty: Type, value: &str) -> String {
        let name = format!("skuld_t{}", self.next_temp);
        self.next_temp += 1;
        self.line(&format!("{} {name} = {value};", self.c_type(ty)));
        name
    }
    fn block_contents(&mut self, block: &'a Block) {
        self.block_contents_in(block, false)
    }
    /// A block, with its own frame of deferred statements. `loop_body` marks
    /// the frame `break` and `continue` unwind to.
    fn block_contents_in(&mut self, block: &'a Block, loop_body: bool) {
        self.line(&format!(
            "/* block bytes {}..{} */",
            block.span.start, block.span.end
        ));
        self.defers.push(DeferScope {
            statements: Vec::new(),
            loop_body,
        });
        let mut left = false;
        for statement in &block.statements {
            self.statement(statement);
            // Everything after a jump is unreachable, and emitting the
            // deferred statements after it would be unreachable too.
            if matches!(
                statement.kind,
                StatementKind::Return(_) | StatementKind::Break | StatementKind::Continue
            ) {
                left = true;
                break;
            }
        }
        let frame = self.defers.pop().expect("a frame per block");
        if !left {
            self.emit_defers(&frame);
        }
    }
    /// One frame's deferred statements, in reverse: the last one written is
    /// the first one run.
    fn emit_defers(&mut self, frame: &DeferScope<'a>) {
        for deferred in frame.statements.iter().rev() {
            self.line(&format!(
                "/* defer bytes {}..{} */",
                deferred.span.start, deferred.span.end
            ));
            self.statement(deferred);
        }
    }
    /// Every frame a jump passes through, innermost first. A `return` unwinds
    /// the whole function; a `break` or `continue` stops at the loop body.
    fn emit_defers_through(&mut self, to_loop: bool) {
        let frames: Vec<Vec<&'a Statement>> = {
            let mut collected = Vec::new();
            for frame in self.defers.iter().rev() {
                collected.push(frame.statements.clone());
                if to_loop && frame.loop_body {
                    break;
                }
            }
            collected
        };
        for statements in frames {
            self.emit_defers(&DeferScope {
                statements,
                loop_body: false,
            });
        }
    }
    fn block(&mut self, block: &'a Block) {
        self.block_in(block, false)
    }
    fn block_in(&mut self, block: &'a Block, loop_body: bool) {
        self.line("{");
        self.indent += 1;
        self.block_contents_in(block, loop_body);
        self.indent -= 1;
        self.line("}");
    }
    fn statement(&mut self, statement: &'a Statement) {
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
            StatementKind::GuardVariable {
                id,
                ty,
                pattern,
                value,
                error,
                error_ty,
                otherwise,
            } => {
                let (present, slot) = match (pattern, value.ty) {
                    (IfLetPattern::Some, Type::Option(_)) => {
                        (".some".to_owned(), ".value".to_owned())
                    }
                    (IfLetPattern::Ok, Type::Result(_)) => (
                        format!(".tag == {}", crate::types::ResultInfo::OK),
                        format!(".payload.v{}", crate::types::ResultInfo::OK),
                    ),
                    _ => unreachable!("checked escape binding"),
                };
                let rendered = self.expression(value);
                // The escape block never falls through, so the declaration
                // below it is reached only when there was a value.
                self.line(&format!("if (!({rendered}{present})) {{"));
                self.indent += 1;
                if let Some(error) = error {
                    let payload = format!("{rendered}.payload.v{}", crate::types::ResultInfo::ERR);
                    self.line(&format!(
                        "{}{} skuld_v{} = {};",
                        self.cleanup(*error_ty),
                        self.c_type(*error_ty),
                        error.0,
                        self.retained(*error_ty, &payload)
                    ));
                    self.line(&format!("(void)skuld_v{};", error.0));
                }
                self.block_contents(otherwise);
                self.indent -= 1;
                self.line("}");
                let initial = self.retained(*ty, &format!("{rendered}{slot}"));
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
                // The returned value is computed first and the deferred
                // statements run after it, so a `defer` never changes what is
                // being returned — it only runs before the caller sees it.
                let rendered = match value {
                    // The caller receives a reference of its own, because every
                    // local here is released as this function returns.
                    Some(expr) => {
                        let value = self.expression(expr);
                        let retained = self.retained(expr.ty, &value);
                        if self.defers.iter().any(|frame| !frame.statements.is_empty()) {
                            self.settled_return(expr.ty, &retained)
                        } else {
                            retained
                        }
                    }
                    None => String::new(),
                };
                self.emit_defers_through(false);
                self.line(&format!("return {rendered};"));
            }
            StatementKind::Block(block) => self.block(block),
            StatementKind::Defer(deferred) => {
                self.defers
                    .last_mut()
                    .expect("a frame per block")
                    .statements
                    .push(deferred);
            }
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
                pattern,
                binding,
                value,
                then_block,
                else_branch,
            } => {
                let (payload, slot) = match (pattern, value.ty) {
                    (IfLetPattern::Some, Type::Option(id)) => {
                        (self.options[id.0].element, ".value".to_string())
                    }
                    (IfLetPattern::Ok, Type::Result(id)) => (
                        self.results[id.0].ok,
                        format!(".payload.v{}", crate::types::ResultInfo::OK),
                    ),
                    (IfLetPattern::Err, Type::Result(id)) => (
                        self.results[id.0].err,
                        format!(".payload.v{}", crate::types::ResultInfo::ERR),
                    ),
                    _ => unreachable!("checked if let"),
                };
                let present = match pattern {
                    IfLetPattern::Some => ".some".to_string(),
                    IfLetPattern::Ok => {
                        format!(".tag == {}", crate::types::ResultInfo::OK)
                    }
                    IfLetPattern::Err => {
                        format!(".tag == {}", crate::types::ResultInfo::ERR)
                    }
                };
                let value = self.expression(value);
                self.line(&format!("if ({value}{present}) {{"));
                self.indent += 1;
                self.line(&format!(
                    "{}{} skuld_v{} = {};",
                    self.cleanup(payload),
                    self.c_type(payload),
                    binding.0,
                    self.retained(payload, &format!("{value}{slot}"))
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
                self.block_contents_in(body, true);
                self.indent -= 1;
                self.line("}");
            }
            StatementKind::Loop { body } => {
                self.line("for (;;) {");
                self.indent += 1;
                self.block_contents_in(body, true);
                self.indent -= 1;
                self.line("}");
            }
            // C binds these to the innermost enclosing loop, which is exactly
            // how they are checked. In a while, `continue` reaches the emitted
            StatementKind::Break => {
                self.emit_defers_through(true);
                self.line("break;");
            }
            StatementKind::Continue => {
                self.emit_defers_through(true);
                self.line("continue;");
            }
            StatementKind::Match { value, arms } => {
                // Enums and `Result` share the tag-plus-payload layout, so the
                let is_scalar_match = !matches!(value.ty, Type::Enum(_) | Type::Result(_));
                if is_scalar_match {
                    let (target_expr, _has_temp) = if matches!(value.kind, ExprKind::Local { .. }) {
                        (self.expression(value), false)
                    } else {
                        let temp_name = format!("skuld_t{}", self.next_temp);
                        self.next_temp += 1;
                        let val_expr = self.expression(value);
                        let cleanup_str = self.cleanup(value.ty);
                        let ty_str = self.c_type(value.ty);
                        self.line(&format!("{cleanup_str}{ty_str} {temp_name} = {val_expr};"));
                        (temp_name, true)
                    };
                    let mut first = true;
                    for arm in arms {
                        match &arm.pattern {
                            MatchPattern::Constant(c_expr) => {
                                let c_val = self.expression(c_expr);
                                let comparison = if value.ty == Type::String {
                                    format!("skuld_string_equal({target_expr}, {c_val})")
                                } else {
                                    format!("{target_expr} == {c_val}")
                                };
                                let cond = if first {
                                    first = false;
                                    format!("if ({comparison}) {{")
                                } else {
                                    format!("else if ({comparison}) {{")
                                };
                                self.line(&cond);
                                self.indent += 1;
                                self.block_contents(&arm.body);
                                self.indent -= 1;
                                self.line("}");
                            }
                            MatchPattern::Range {
                                start,
                                end,
                                inclusive,
                            } => {
                                let s_val = self.expression(start);
                                let e_val = self.expression(end);
                                let op = if *inclusive { "<=" } else { "<" };
                                let comparison = format!(
                                    "{target_expr} >= {s_val} && {target_expr} {op} {e_val}"
                                );
                                let cond = if first {
                                    first = false;
                                    format!("if ({comparison}) {{")
                                } else {
                                    format!("else if ({comparison}) {{")
                                };
                                self.line(&cond);
                                self.indent += 1;
                                self.block_contents(&arm.body);
                                self.indent -= 1;
                                self.line("}");
                            }
                            MatchPattern::Wildcard => {
                                let cond = if first {
                                    first = false;
                                    "if (true) {".to_string()
                                } else {
                                    "else {".to_string()
                                };
                                self.line(&cond);
                                self.indent += 1;
                                self.block_contents(&arm.body);
                                self.indent -= 1;
                                self.line("}");
                            }
                            MatchPattern::Variant { .. } => {
                                unreachable!("scalar match pattern was lowered to constant")
                            }
                        }
                    }
                } else {
                    // A `Result` matches like a two-variant enum, so the arm
                    // code generation is shared rather than repeated; the
                    // only difference here is where a variant's payload type lives.
                    let payloads: Vec<Option<Type>> = match value.ty {
                        Type::Enum(id) => self.enums[id.0]
                            .variants
                            .iter()
                            .map(|variant| variant.payload)
                            .collect(),
                        Type::Result(id) => {
                            let info = self.results[id.0];
                            vec![Some(info.ok), Some(info.err)]
                        }
                        _ => unreachable!("checked match"),
                    };
                    let target = self.expression(value);
                    let mut first = true;
                    for arm in arms {
                        match &arm.pattern {
                            MatchPattern::Variant {
                                variant_index,
                                binding,
                            } => {
                                let cond = if first {
                                    first = false;
                                    format!("if ({target}.tag == {variant_index}) {{")
                                } else {
                                    format!("else if ({target}.tag == {variant_index}) {{")
                                };
                                self.line(&cond);
                                self.indent += 1;
                                if let Some(binding_id) = binding {
                                    let payload_ty =
                                        payloads[*variant_index].expect("checked variant payload");
                                    self.line(&format!(
                                        "{}{} skuld_v{} = {};",
                                        self.cleanup(payload_ty),
                                        self.c_type(payload_ty),
                                        binding_id.0,
                                        self.retained(
                                            payload_ty,
                                            &format!("{target}.payload.v{variant_index}")
                                        )
                                    ));
                                    self.line(&format!("(void)skuld_v{};", binding_id.0));
                                }
                                self.block_contents(&arm.body);
                                self.indent -= 1;
                                self.line("}");
                            }
                            MatchPattern::Wildcard => {
                                let cond = if first {
                                    first = false;
                                    "if (true) {".to_string()
                                } else {
                                    "else {".to_string()
                                };
                                self.line(&cond);
                                self.indent += 1;
                                self.block_contents(&arm.body);
                                self.indent -= 1;
                                self.line("}");
                            }
                            _ => unreachable!("enum match pattern must be variant or wildcard"),
                        }
                    }
                }
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                self.line("{");
                self.indent += 1;
                match iterable {
                    ForIterable::Range { start, end } => {
                        let start_val = self.expression(start);
                        let end_val = self.expression(end);
                        let start_temp = self.next_temp;
                        self.next_temp += 1;
                        let end_temp = self.next_temp;
                        self.next_temp += 1;
                        self.line(&format!("int64_t skuld_t{start_temp} = {start_val};"));
                        self.line(&format!("int64_t skuld_t{end_temp} = {end_val};"));
                        self.line(&format!(
                            "for (int64_t skuld_v{} = skuld_t{start_temp}; skuld_v{} < skuld_t{end_temp}; skuld_v{}++) {{",
                            variable.0, variable.0, variable.0
                        ));
                        self.indent += 1;
                        self.line(&format!("(void)skuld_v{};", variable.0));
                        self.block_contents_in(body, true);
                        self.indent -= 1;
                        self.line("}");
                    }
                    ForIterable::Array(collection) => {
                        let arr_val = self.expression(collection);
                        let arr_temp = self.next_temp;
                        self.next_temp += 1;
                        let arr_c_ty = self.c_type(collection.ty);
                        self.line(&format!("{arr_c_ty} skuld_t{arr_temp} = {arr_val};"));
                        let idx_temp = self.next_temp;
                        self.next_temp += 1;
                        self.line(&format!(
                            "for (int64_t skuld_t{idx_temp} = 0; skuld_t{idx_temp} < skuld_t{arr_temp}->len; skuld_t{idx_temp}++) {{"
                        ));
                        self.indent += 1;
                        let Type::Array(array_id) = collection.ty else {
                            unreachable!("checked array for loop")
                        };
                        let elem_ty = self.arrays[array_id.0].element;
                        let cleanup = self.cleanup(elem_ty);
                        let elem_c_ty = self.c_type(elem_ty);
                        let retained_elem = self.retained(
                            elem_ty,
                            &format!("skuld_t{arr_temp}->data[skuld_t{idx_temp}]"),
                        );
                        self.line(&format!(
                            "{cleanup}{elem_c_ty} skuld_v{} = {retained_elem};",
                            variable.0
                        ));
                        self.line(&format!("(void)skuld_v{};", variable.0));
                        self.block_contents_in(body, true);
                        self.indent -= 1;
                        self.line("}");
                    }
                    ForIterable::FixedArray { collection, size } => {
                        let arr_val = self.expression(collection);
                        let arr_temp = self.next_temp;
                        self.next_temp += 1;
                        let arr_c_ty = self.c_type(collection.ty);
                        self.line(&format!("{arr_c_ty} skuld_t{arr_temp} = {arr_val};"));
                        let idx_temp = self.next_temp;
                        self.next_temp += 1;
                        self.line(&format!(
                            "for (int64_t skuld_t{idx_temp} = 0; skuld_t{idx_temp} < {size}; skuld_t{idx_temp}++) {{"
                        ));
                        self.indent += 1;
                        let Type::FixedArray(array_id) = collection.ty else {
                            unreachable!("checked fixed array for loop")
                        };
                        let elem_ty = self.fixed_arrays[array_id.0].element;
                        let cleanup = self.cleanup(elem_ty);
                        let elem_c_ty = self.c_type(elem_ty);
                        let retained_elem = self.retained(
                            elem_ty,
                            &format!("skuld_t{arr_temp}.data[skuld_t{idx_temp}]"),
                        );
                        self.line(&format!(
                            "{cleanup}{elem_c_ty} skuld_v{} = {retained_elem};",
                            variable.0
                        ));
                        self.line(&format!("(void)skuld_v{};", variable.0));
                        self.block_contents_in(body, true);
                        self.indent -= 1;
                        self.line("}");
                    }
                    // The same loop over a string's own bytes: no array is
                    // built, and a `u8` needs no retain or release.
                    ForIterable::StringBytes(text) => {
                        let value = self.expression(text);
                        let text_temp = self.next_temp;
                        self.next_temp += 1;
                        self.line(&format!("skuld_string skuld_t{text_temp} = {value};"));
                        let idx_temp = self.next_temp;
                        self.next_temp += 1;
                        self.line(&format!(
                            "for (size_t skuld_t{idx_temp} = 0; skuld_t{idx_temp} < skuld_t{text_temp}.len; skuld_t{idx_temp}++) {{"
                        ));
                        self.indent += 1;
                        self.line(&format!(
                            "uint8_t skuld_v{} = skuld_t{text_temp}.data[skuld_t{idx_temp}];",
                            variable.0
                        ));
                        self.line(&format!("(void)skuld_v{};", variable.0));
                        self.block_contents_in(body, true);
                        self.indent -= 1;
                        self.line("}");
                    }
                }
                self.indent -= 1;
                self.line("}");
            }
        }
    }
    fn expression(&mut self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(value) => {
                // An unsigned literal is emitted through its own family, so a
                // u64 above INT64_MAX never has to round-trip through a
                // negative constant to reach its own value.
                if expr.ty.int_type().is_some_and(|kind| !kind.signed()) {
                    format!("UINT64_C({})", *value as u64)
                } else if *value == i64::MIN {
                    "INT64_MIN".into()
                } else if *value < 0 {
                    format!("(-INT64_C({}))", value.unsigned_abs())
                } else {
                    format!("INT64_C({value})")
                }
            }
            ExprKind::Float(value) => format!("{value:.17e}"),
            ExprKind::Bool(value) => value.to_string(),
            ExprKind::Char(value) => format!("((uint32_t){})", *value as u32),
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
            ExprKind::Local { id, settled } => {
                let name = self.local_name(*id);
                // A binding nothing can reassign outlives the expression that
                // reads it, so the read borrows it: no retain, no release, no
                // temporary. Everything that stores the value retains it
                // itself, which is where ownership was always decided.
                if *settled {
                    return name;
                }
                self.temporary(expr.ty, &name)
            }
            ExprKind::Lambda { index } => {
                let Type::Function(ty) = expr.ty else {
                    unreachable!("internal compiler bug: lambda without a function type")
                };
                // The environment is a local of the enclosing block, which the
                // value cannot outlive: a function value is never stored where
                // something could keep it alive for longer.
                let environment = self.next_temp;
                self.next_temp += 1;
                let captures = self.lambda_captures[*index].clone();
                let initialisers: Vec<String> = captures
                    .iter()
                    .map(|id| format!(".skuld_v{} = {}", id.0, self.local_name(*id)))
                    .collect();
                let body = if initialisers.is_empty() {
                    "{ 0 }".to_owned()
                } else {
                    format!("{{ {} }}", initialisers.join(", "))
                };
                self.line(&format!(
                    "struct skuld_env{index} skuld_e{environment} = {body};"
                ));
                self.temporary(
                    expr.ty,
                    &format!(
                        "(skuld_ft{}){{ skuld_lam{index}, &skuld_e{environment} }}",
                        ty.0
                    ),
                )
            }
            ExprKind::InterfaceValue {
                object,
                class,
                interface,
            } => {
                let rendered = self.expression(object);
                // The header is the first member of a class, so its address is
                // the object's address; the table says what the methods are.
                self.store(
                    expr.ty,
                    &format!(
                        "(skuld_i{}){{ &({}) ->header, &skuld_ivtable{}_{} }}",
                        interface.0,
                        self.retained(object.ty, &rendered),
                        interface.0,
                        class.0
                    ),
                    true,
                )
            }
            ExprKind::InterfaceCall {
                object,
                interface,
                index,
                arguments,
            } => {
                let receiver = self.expression(object);
                let mut values = vec![format!("{receiver}.object")];
                values.extend(arguments.iter().map(|argument| self.expression(argument)));
                let name = &self.interfaces[interface.0].methods[*index].name;
                let call = format!("{receiver}.vtable->{name}({})", values.join(", "));
                if expr.ty == Type::Void {
                    self.line(&format!("{call};"));
                    String::new()
                } else {
                    self.store(expr.ty, &call, true)
                }
            }
            ExprKind::FunctionValue { id, ty } => self.temporary(
                expr.ty,
                &format!("(skuld_ft{}){{ skuld_thunk{}_{}, NULL }}", ty.0, id.0, ty.0),
            ),
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
                let name = self.allocate(expr.ty, "0", expr.span.start);
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
                let name = self.allocate(expr.ty, &elements.len().to_string(), expr.span.start);
                for (position, value) in values.iter().enumerate() {
                    self.line(&format!(
                        "{name}->data[{position}] = {};",
                        self.retained(element_type, value)
                    ));
                }
                name
            }
            ExprKind::FixedArray(elements) => {
                let Type::FixedArray(id) = expr.ty else {
                    unreachable!("checked fixed array")
                };
                let element_type = self.fixed_arrays[id.0].element;
                let values: Vec<_> = elements
                    .iter()
                    .map(|element| self.expression(element))
                    .collect();
                let type_str = self.c_type(expr.ty);
                let name = self.store(expr.ty, &format!("({type_str}){{0}}"), false);
                for (position, value) in values.iter().enumerate() {
                    self.line(&format!(
                        "{name}.data[{position}] = {};",
                        self.retained(element_type, value)
                    ));
                }
                name
            }
            ExprKind::FixedArrayRepeat { element, size } => {
                let Type::FixedArray(id) = expr.ty else {
                    unreachable!("checked fixed array")
                };
                let element_type = self.fixed_arrays[id.0].element;
                let val = self.expression(element);
                let type_str = self.c_type(expr.ty);
                let name = self.store(expr.ty, &format!("({type_str}){{0}}"), false);
                self.line(&format!(
                    "for (size_t skuld_i = 0; skuld_i < {size}; ++skuld_i) {{ {name}.data[skuld_i] = {}; }}",
                    self.retained(element_type, &val)
                ));
                name
            }
            ExprKind::FixedArrayToSlice { object, array_id } => {
                let val = if let Some(lv) = self.lvalue(object) {
                    lv
                } else {
                    self.expression(object)
                };
                let Type::FixedArray(fa_id) = object.ty else {
                    unreachable!("checked fixed array to slice")
                };
                let size = self.fixed_arrays[fa_id.0].size;
                let slice_type = format!("skuld_a{}", array_id.0);
                self.store(
                    expr.ty,
                    &format!("(&( {slice_type} ){{ .header = {{ .strong = SIZE_MAX / 2, .weak = SIZE_MAX / 2, .destroy = NULL }}, .len = {size}, .capacity = 0, .data = {val}.data }})"),
                    true,
                )
            }
            ExprKind::Index { object, index } if object.ty == Type::String => {
                // A byte read out of settled storage borrows it: nothing
                // between the read and the use can release the string.
                if let Some(storage) = self.settled_storage(object)
                    && self.runs_no_code(index)
                {
                    let subscript = self.expression(index);
                    return self.temporary(
                        expr.ty,
                        &format!(
                            "skuld_string_byte({storage}, {subscript}, {})",
                            index.span.start
                        ),
                    );
                }
                let value = self.expression(object);
                let subscript = self.expression(index);
                self.temporary(
                    expr.ty,
                    &format!(
                        "skuld_string_byte({value}, {subscript}, {})",
                        index.span.start
                    ),
                )
            }
            ExprKind::Slice { object, start, end } => {
                let value = self.expression(object);
                let from = self.expression(start);
                let to = self.expression(end);
                let byte = expr.span.start;
                if object.ty == Type::String {
                    return self.store(
                        expr.ty,
                        &format!("skuld_string_slice({value}, {from}, {to}, {byte})"),
                        true,
                    );
                }
                if let Type::FixedArray(fa_id) = object.ty {
                    let element = self.fixed_arrays[fa_id.0].element;
                    let size = self.fixed_arrays[fa_id.0].size;
                    let low = self.temporary(Type::INT, &format!("(int64_t)({from})"));
                    let high = self.temporary(Type::INT, &format!("(int64_t)({to})"));
                    self.line(&format!(
                        "skuld_slice_range({low}, {high}, {size}, {byte});"
                    ));
                    let result = self.allocate(expr.ty, &format!("(size_t)({high} - {low})"), byte);
                    self.line(&format!("for (size_t i = 0; i < {result}->len; ++i) {{"));
                    self.indent += 1;
                    let val = if let Some(lv) = self.lvalue(object) {
                        lv
                    } else {
                        value
                    };
                    let source = format!("{val}.data[(size_t){low} + i]");
                    self.line(&format!(
                        "{result}->data[i] = {};",
                        self.retained(element, &source)
                    ));
                    self.indent -= 1;
                    self.line("}");
                    return result;
                }
                let Type::Array(id) = object.ty else {
                    unreachable!("checked slice")
                };
                let element = self.arrays[id.0].element;
                let low = self.temporary(Type::INT, &format!("(int64_t)({from})"));
                let high = self.temporary(Type::INT, &format!("(int64_t)({to})"));
                self.line(&format!(
                    "skuld_slice_range({low}, {high}, {value}->len, {byte});"
                ));
                let result = self.allocate(expr.ty, &format!("(size_t)({high} - {low})"), byte);
                self.line(&format!("for (size_t i = 0; i < {result}->len; ++i) {{"));
                self.indent += 1;
                let source = format!("{value}->data[(size_t){low} + i]");
                self.line(&format!(
                    "{result}->data[i] = {};",
                    self.retained(element, &source)
                ));
                self.indent -= 1;
                self.line("}");
                result
            }
            ExprKind::BytesToString(value) => {
                let bytes = self.expression(value);
                let byte = expr.span.start;
                let result = self.store(expr.ty, &format!("({}){{0}}", self.c_type(expr.ty)), true);
                let offset = format!("skuld_t{}", self.next_temp);
                self.next_temp += 1;
                self.line(&format!("size_t {offset} = 0;"));
                self.line(&format!(
                    "if (skuld_utf8_valid({bytes}->data, {bytes}->len, &{offset})) {{"
                ));
                self.indent += 1;
                self.line(&format!("{result}.tag = {};", crate::types::ResultInfo::OK));
                self.line(&format!(
                    "{result}.payload.v{} = skuld_string_from_bytes((const char *){bytes}->data, {bytes}->len);",
                    crate::types::ResultInfo::OK
                ));
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                let position = self.store(
                    Type::String,
                    &format!("skuld_string_from_uint({offset})"),
                    true,
                );
                self.line(&format!(
                    "{result}.tag = {};",
                    crate::types::ResultInfo::ERR
                ));
                self.line(&format!(
                    "{result}.payload.v{} = skuld_string_concat({}, {position}, {byte});",
                    crate::types::ResultInfo::ERR,
                    string_literal("invalid UTF-8 at byte ")
                ));
                self.indent -= 1;
                self.line("}");
                result
            }
            // A borrowed pointer into bytes the program still owns. Nothing is
            // retained: the borrow is only valid while the operand is alive,
            // which the temporary holding it guarantees for this statement.
            ExprKind::Ptr(value) => {
                let ty = value.ty;
                let cast = self.c_type(expr.ty);
                let bytes = match ty {
                    Type::String => {
                        let value = self.expression(value);
                        format!("({cast}){value}.data")
                    }
                    Type::Array(_) => {
                        let value = self.expression(value);
                        format!("({cast}){value}->data")
                    }
                    Type::FixedArray(_) => {
                        let val = if let Some(lv) = self.lvalue(value) {
                            lv
                        } else {
                            self.expression(value)
                        };
                        format!("({cast}){val}.data")
                    }
                    _ => unreachable!("internal compiler bug: unchecked `ptr` operand"),
                };
                self.temporary(expr.ty, &bytes)
            }
            // The address of a scalar local. It is a C local, so its address
            // is its address; nothing is copied and nothing is counted.
            ExprKind::AddressOf(value) => {
                // A local is already a place; anything else is first settled
                // into a temporary, whose address is then good for as long as
                // every other borrow this expression takes.
                let place = match self.lvalue(value) {
                    Some(place) => place,
                    None => {
                        let ty = value.ty;
                        let computed = self.expression(value);
                        self.store(ty, &computed, true)
                    }
                };
                let cast = self.c_type(expr.ty);
                self.temporary(expr.ty, &format!("({cast})&{place}"))
            }
            ExprKind::Load { pointer, volatile } => {
                let pointer = self.expression(pointer);
                let read = if *volatile {
                    let cell = self.c_type(expr.ty);
                    format!("(*(volatile {cell} *){pointer})")
                } else {
                    format!("(*{pointer})")
                };
                self.temporary(expr.ty, &read)
            }
            ExprKind::Store {
                pointer,
                value,
                volatile,
            } => {
                let target = self.expression(pointer);
                let written = self.expression(value);
                if *volatile {
                    let cell = self.c_type(value.ty);
                    self.line(&format!("*(volatile {cell} *){target} = {written};"));
                } else {
                    self.line(&format!("*{target} = {written};"));
                }
                String::new()
            }
            ExprKind::PointerOffset { pointer, count } => {
                let pointer = self.expression(pointer);
                let count = self.expression(count);
                self.temporary(expr.ty, &format!("({pointer} + {count})"))
            }
            ExprKind::PointerAddr(pointer) => {
                let pointer = self.expression(pointer);
                self.temporary(expr.ty, &format!("((size_t){pointer})"))
            }
            ExprKind::EnumValue { value, id } => {
                let rendered = self.expression(value);
                self.temporary(expr.ty, &format!("skuld_e{}_values[{rendered}.tag]", id.0))
            }
            ExprKind::EnumFromValue { value, id } => {
                let rendered = self.expression(value);
                self.temporary(
                    expr.ty,
                    &format!("skuld_e{}_from({rendered}, {})", id.0, expr.span.start),
                )
            }
            ExprKind::LayoutOf { id, field } => {
                let query = match field {
                    Some(index) => format!("offsetof(skuld_s{}, f{index})", id.0),
                    None => format!("sizeof(skuld_s{})", id.0),
                };
                self.temporary(expr.ty, &format!("((size_t){query})"))
            }
            ExprKind::PointerFrom(address) => {
                let address = self.expression(address);
                let cast = self.c_type(expr.ty);
                self.temporary(expr.ty, &format!("(({cast})(uintptr_t){address})"))
            }
            ExprKind::StringLen(value) => {
                if let Some(storage) = self.settled_storage(value) {
                    return self.temporary(Type::INT, &format!("(int64_t){storage}.len"));
                }
                let value = self.expression(value);
                self.temporary(Type::INT, &format!("(int64_t){value}.len"))
            }
            ExprKind::StringBytes(value) => {
                let value = self.expression(value);
                let byte = expr.span.start;
                let result = self.allocate(expr.ty, &format!("{value}.len"), byte);
                self.line(&format!(
                    "if ({value}.len != 0) memcpy({result}->data, {value}.data, {value}.len);"
                ));
                result
            }
            ExprKind::Index { object, index } => {
                if let Type::FixedArray(id) = object.ty {
                    let size = self.fixed_arrays[id.0].size;
                    if self.runs_no_code(index)
                        && let Some(storage) = self.lvalue(object)
                    {
                        let subscript = self.expression(index);
                        let read = format!(
                            "{storage}.data[skuld_index({subscript}, {size}, {})]",
                            index.span.start
                        );
                        return self.temporary(expr.ty, &read);
                    }
                    let value = self.expression(object);
                    let subscript = self.expression(index);
                    let read = format!(
                        "{value}.data[skuld_index({subscript}, {size}, {})]",
                        index.span.start
                    );
                    return self.temporary(expr.ty, &read);
                }
                if let Some(storage) = self.settled_storage(object)
                    && self.runs_no_code(index)
                {
                    let subscript = self.expression(index);
                    let read = format!(
                        "{storage}->data[skuld_index({subscript}, {storage}->len, {})]",
                        index.span.start
                    );
                    return self.temporary(expr.ty, &read);
                }
                let read = self.index_read(object, index);
                self.temporary(expr.ty, &read)
            }
            ExprKind::ArrayCall {
                object,
                method,
                arguments,
            } => self.array_call(object, *method, arguments, expr),
            ExprKind::ArrayLen(object) => {
                if let Some(storage) = self.settled_storage(object) {
                    return self.temporary(Type::INT, &format!("(int64_t){storage}->len"));
                }
                let value = self.expression(object);
                self.temporary(Type::INT, &format!("(int64_t){value}->len"))
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
            ExprKind::Ok(value) | ExprKind::Err(value) => {
                let tag = if matches!(expr.kind, ExprKind::Ok(_)) {
                    crate::types::ResultInfo::OK
                } else {
                    crate::types::ResultInfo::ERR
                };
                let rendered = self.expression(value);
                let retained = self.retained(value.ty, &rendered);
                let c_ty = self.c_type(expr.ty);
                self.store(
                    expr.ty,
                    &format!("({c_ty}){{.tag = {tag}, .payload = {{.v{tag} = {retained}}}}}"),
                    true,
                )
            }
            ExprKind::IntConvert { value, target } => {
                let rendered = self.expression(value);
                if value.ty == Type::Float {
                    return self.temporary(
                        expr.ty,
                        &format!(
                            "skuld_f_to_{}({rendered}, {})",
                            target.suffix(),
                            expr.span.start
                        ),
                    );
                }
                if value.ty == Type::Char {
                    return self.temporary(
                        expr.ty,
                        &format!(
                            "skuld_u_to_{}((uint64_t)({rendered}), {})",
                            target.suffix(),
                            expr.span.start
                        ),
                    );
                }
                let source = value.ty.int_type().expect("checked integer conversion");
                if source == *target {
                    return self.temporary(expr.ty, &rendered);
                }
                // A signed source widens to int64_t and an unsigned one to
                // uint64_t, so one helper per pair of families covers all.
                let family = if source.signed() { "i" } else { "u" };
                self.temporary(
                    expr.ty,
                    &format!(
                        "skuld_{family}_to_{}({rendered}, {})",
                        target.suffix(),
                        expr.span.start
                    ),
                )
            }
            ExprKind::FloatConvert(value) => {
                let rendered = self.expression(value);
                if value.ty == Type::Float {
                    self.temporary(expr.ty, &rendered)
                } else {
                    self.temporary(expr.ty, &format!("((double)({rendered}))"))
                }
            }
            ExprKind::CharConvert(value) => {
                let rendered = self.expression(value);
                if value.ty == Type::Char {
                    self.temporary(expr.ty, &rendered)
                } else if value.ty.int_type().is_some_and(|it| it.signed()) {
                    self.temporary(
                        expr.ty,
                        &format!(
                            "skuld_i_to_char((int64_t)({rendered}), {})",
                            expr.span.start
                        ),
                    )
                } else {
                    self.temporary(
                        expr.ty,
                        &format!(
                            "skuld_u_to_char((uint64_t)({rendered}), {})",
                            expr.span.start
                        ),
                    )
                }
            }
            ExprKind::IsOk(value) | ExprKind::IsErr(value) => {
                let tag = if matches!(expr.kind, ExprKind::IsOk(_)) {
                    crate::types::ResultInfo::OK
                } else {
                    crate::types::ResultInfo::ERR
                };
                let value = self.expression(value);
                self.temporary(Type::Bool, &format!("{value}.tag == {tag}"))
            }
            ExprKind::Try(value) => {
                let Type::Result(id) = value.ty else {
                    unreachable!("checked `?`")
                };
                let ok = self.results[id.0].ok;
                let err = self.results[id.0].err;
                let rendered = self.expression(value);
                // The operand is kept in an owning slot so that the early
                // return below releases it like any other local.
                let slot = self.temporary(value.ty, &rendered);
                let error = crate::types::ResultInfo::ERR;
                let returned = self.c_type(self.current_return);
                let payload = self.retained(err, &format!("{slot}.payload.v{error}"));
                self.line(&format!("if ({slot}.tag == {error}) {{"));
                self.indent += 1;
                // `?` is a return, so every deferred statement runs here too.
                // The error is settled first, for the same reason a returned
                // value is: what leaves must not depend on what runs on the
                // way out.
                let escaping = self.settled_return(
                    self.current_return,
                    &format!(
                        "({returned}){{.tag = {error}, .payload = {{.v{error} = {payload}}}}}"
                    ),
                );
                self.emit_defers_through(false);
                self.line(&format!("return {escaping};"));
                self.indent -= 1;
                self.line("}");
                let success = crate::types::ResultInfo::OK;
                self.temporary(ok, &format!("{slot}.payload.v{success}"))
            }
            ExprKind::EnumVariant {
                variant_index,
                payload,
            } => {
                let c_ty = self.c_type(expr.ty);
                let value = if let Some(payload_expr) = payload {
                    let rendered = self.expression(payload_expr);
                    let retained = self.retained(payload_expr.ty, &rendered);
                    format!(
                        "({c_ty}){{.tag = {variant_index}, .payload = {{.v{variant_index} = {retained}}}}}"
                    )
                } else {
                    format!("({c_ty}){{.tag = {variant_index}}}")
                };
                self.store(expr.ty, &value, true)
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
                                Type::Int(kind) => {
                                    let from = if kind.signed() { "int" } else { "uint" };
                                    self.store(
                                        Type::String,
                                        &format!("skuld_string_from_{from}({rendered})"),
                                        true,
                                    )
                                }
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
                                Type::Char => self.store(
                                    Type::String,
                                    &format!("skuld_string_from_char({rendered})"),
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
                    // Unsigned types never reach here: the checker rejects
                    // negating one, since only zero would have a result.
                    UnaryOp::Negative if let Type::Int(kind) = expr.ty => {
                        format!("skuld_neg_{}({value}, {})", kind.suffix(), op_span.start)
                    }
                    UnaryOp::Negative => format!("(-{value})"),
                    UnaryOp::Not => format!("(!{value})"),
                    UnaryOp::BitNot if let Type::Int(kind) = expr.ty => {
                        format!("(({})(~{value}))", kind.c_type())
                    }
                    UnaryOp::BitNot => format!("(~{value})"),
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
                        AssignmentOp::BitAnd => BinaryOp::BitAnd,
                        AssignmentOp::BitOr => BinaryOp::BitOr,
                        AssignmentOp::BitXor => BinaryOp::BitXor,
                        AssignmentOp::ShiftLeft => BinaryOp::ShiftLeft,
                        AssignmentOp::ShiftRight => BinaryOp::ShiftRight,
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
                    CallTarget::Function(id) => {
                        format!("{}({})", self.function_name(*id), values.join(", "))
                    }
                    // The pair is evaluated once, into the helper, so a callee
                    // with side effects runs exactly as often as it is written.
                    CallTarget::Value(callee) => {
                        let Type::Function(ty) = callee.ty else {
                            unreachable!("internal compiler bug: call through a non-function")
                        };
                        let mut arguments = vec![self.expression(callee)];
                        arguments.extend(values.iter().cloned());
                        format!("skuld_ftcall{}({})", ty.0, arguments.join(", "))
                    }
                    // The foreign name is the linker's, not the generator's.
                    CallTarget::Extern(id) => {
                        format!("{}({})", self.extern_names[id], values.join(", "))
                    }
                    CallTarget::Print if arguments.is_empty() => format!(
                        "skuld_print_string((skuld_string){{(const unsigned char *)\"\", 0, NULL}}, {})",
                        expr.span.start
                    ),
                    CallTarget::Print => {
                        let suffix = match arguments[0].ty {
                            Type::Int(kind) if kind.signed() => "int",
                            Type::Int(_) => "uint",
                            Type::Float => "float",
                            Type::Bool => "bool",
                            Type::String => "string",
                            Type::Char => "char",
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
    // Every integer width traps on overflow and on invalid division rather
    // than wrapping, so arithmetic goes through a per-width helper.
    let helper = match (ty.int_type(), op) {
        (Some(kind), Add) => Some(("add", kind)),
        (Some(kind), Subtract) => Some(("sub", kind)),
        (Some(kind), Multiply) => Some(("mul", kind)),
        (Some(kind), Divide) => Some(("div", kind)),
        (Some(kind), Modulo) => Some(("rem", kind)),
        (Some(kind), ShiftLeft) => Some(("shl", kind)),
        (Some(kind), ShiftRight) => Some(("shr", kind)),
        _ => None,
    };
    if let Some((helper, kind)) = helper {
        return format!("skuld_{helper}_{}({left}, {right}, {byte})", kind.suffix());
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
        BitAnd => "&",
        BitOr => "|",
        BitXor => "^",
        Equal => "==",
        NotEqual => "!=",
        Less => "<",
        Greater => ">",
        LessEqual => "<=",
        GreaterEqual => ">=",
        And => "&&",
        Or => "||",
        ShiftLeft | ShiftRight => unreachable!(),
    };
    if let Some(kind) = ty.int_type()
        && matches!(op, BitAnd | BitOr | BitXor)
    {
        return format!("(({})({left} {operator} {right}))", kind.c_type());
    }
    format!("({left} {operator} {right})")
}

/// Checks the language makes in both build modes.
///
/// Bounds checking is part of the language, not part of managing memory: a
/// freestanding program has no runtime and still checks every index, so this
/// is emitted in both preludes rather than living with retain and release.
const SHARED_CHECKS: &str = r#"
/* One comparison covers both ends: a negative index becomes an enormous
   unsigned value, which is already past any length. Clang was folding the two
   into one anyway — measuring showed no difference — so this is for the reader
   and for compilers that do not. */
static inline size_t skuld_index(int64_t index, size_t length, size_t byte) {
    if ((uint64_t)index >= length) skuld_fail("array index out of bounds", byte);
    return (size_t)index;
}
"#;

const PRELUDE_HEAD: &str = r#"/* Generated by Skuld. C11, compiled with clang. */
#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <float.h>
#include <math.h>
_Static_assert(DBL_MANT_DIG == 53 && DBL_MAX_EXP == 1024, "Skuld requires binary64 double");

/* Every bounds check, overflow check and trap in a generated program ends
   here. `_Noreturn` says so to a C compiler that does not infer it from the
   `exit` below; clang already did, and measuring showed no difference, so it
   is here for the readers — human and otherwise — rather than for speed. */
_Noreturn static inline void skuld_fail(const char *message, size_t byte) {
    fprintf(stderr, "runtime error: %s (source byte %zu)\n", message, byte);
    exit(1);
}
"#;

/// The managed-memory runtime is real C in `runtime/`, embedded verbatim so
/// there is one source of truth for retain and release.
const RUNTIME: &str = include_str!("../../runtime/strings.c");

/// The platform layer, also real C, and separate from the memory runtime
/// because the two answer different questions: `strings.c` is about what a
/// value costs, this is about what the operating system is called. Keeping
/// them apart is what lets a second operating system arrive without the
/// memory runtime learning it exists.
const PLATFORM: &str = include_str!("../../runtime/platform.c");

/// What a freestanding program starts with: the three headers C guarantees a
/// freestanding implementation provides, and a trap that faults instead of
/// printing.
///
/// There is no `stdio.h` to say why, and nothing to say it to — a kernel that
/// divides by zero has no standard error. `__builtin_trap` is an instruction
/// the processor refuses (`ud2` on x86), which is the most a program with no
/// operating system under it can do about a bug in itself.
const FREESTANDING_HEAD: &str = r#"/* Generated by Skuld (freestanding). No runtime, no libc. */
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <float.h>
_Static_assert(DBL_MANT_DIG == 53 && DBL_MAX_EXP == 1024, "Skuld requires binary64 double");

/* `isnan` lives in <math.h>, which a freestanding implementation does not
 * provide; the comparison it stands for is the definition of a NaN. */
#define isnan(v) ((v) != (v))

_Noreturn static inline void skuld_fail(const char *message, size_t byte) {
    (void)message;
    (void)byte;
    __builtin_trap();
}
"#;

const PRELUDE_TAIL: &str = r#"
/* Arithmetic traps on overflow and on invalid division at every width, rather
 * than wrapping or leaving signed overflow to C's undefined behavior. The two
 * macros differ only where signedness does: an unsigned type has no negation
 * and no INT_MIN / -1 case. */
#define SKULD_INT_OPS(S, T)                                                        \
    static inline T skuld_add_##S(T a, T b, size_t byte) {                         \
        T r;                                                                       \
        if (__builtin_add_overflow(a, b, &r)) skuld_fail("integer overflow", byte); \
        return r;                                                                  \
    }                                                                              \
    static inline T skuld_sub_##S(T a, T b, size_t byte) {                         \
        T r;                                                                       \
        if (__builtin_sub_overflow(a, b, &r)) skuld_fail("integer overflow", byte); \
        return r;                                                                  \
    }                                                                              \
    static inline T skuld_mul_##S(T a, T b, size_t byte) {                         \
        T r;                                                                       \
        if (__builtin_mul_overflow(a, b, &r)) skuld_fail("integer overflow", byte); \
        return r;                                                                  \
    }

#define SKULD_INT_OPS_SIGNED(S, T, MIN, UT, BITS)                                  \
    SKULD_INT_OPS(S, T)                                                            \
    static inline T skuld_div_##S(T a, T b, size_t byte) {                         \
        if (b == 0) skuld_fail("integer division by zero", byte);                  \
        if (a == MIN && b == -1) skuld_fail("integer overflow", byte);             \
        return (T)(a / b);                                                         \
    }                                                                              \
    static inline T skuld_rem_##S(T a, T b, size_t byte) {                         \
        if (b == 0) skuld_fail("integer remainder by zero", byte);                 \
        if (a == MIN && b == -1) return 0;                                         \
        return (T)(a % b);                                                         \
    }                                                                              \
    static inline T skuld_neg_##S(T a, size_t byte) {                              \
        if (a == MIN) skuld_fail("integer overflow", byte);                        \
        return (T)(-a);                                                            \
    }                                                                              \
    static inline T skuld_shl_##S(T a, T b, size_t byte) {                         \
        if (b < 0 || b >= BITS) skuld_fail("shift amount out of range", byte);    \
        return (T)((UT)a << b);                                                    \
    }                                                                              \
    static inline T skuld_shr_##S(T a, T b, size_t byte) {                         \
        if (b < 0 || b >= BITS) skuld_fail("shift amount out of range", byte);    \
        return (T)(a >> b);                                                        \
    }

#define SKULD_INT_OPS_UNSIGNED(S, T, BITS)                                         \
    SKULD_INT_OPS(S, T)                                                            \
    static inline T skuld_div_##S(T a, T b, size_t byte) {                         \
        if (b == 0) skuld_fail("integer division by zero", byte);                  \
        return (T)(a / b);                                                         \
    }                                                                              \
    static inline T skuld_rem_##S(T a, T b, size_t byte) {                         \
        if (b == 0) skuld_fail("integer remainder by zero", byte);                 \
        return (T)(a % b);                                                         \
    }                                                                              \
    static inline T skuld_shl_##S(T a, T b, size_t byte) {                         \
        if (b >= BITS) skuld_fail("shift amount out of range", byte);              \
        return (T)(a << b);                                                        \
    }                                                                              \
    static inline T skuld_shr_##S(T a, T b, size_t byte) {                         \
        if (b >= BITS) skuld_fail("shift amount out of range", byte);              \
        return (T)(a >> b);                                                        \
    }

SKULD_INT_OPS_SIGNED(i8, int8_t, INT8_MIN, uint8_t, 8)
SKULD_INT_OPS_SIGNED(i16, int16_t, INT16_MIN, uint16_t, 16)
SKULD_INT_OPS_SIGNED(i32, int32_t, INT32_MIN, uint32_t, 32)
SKULD_INT_OPS_SIGNED(i64, int64_t, INT64_MIN, uint64_t, 64)
SKULD_INT_OPS_SIGNED(isize, ptrdiff_t, PTRDIFF_MIN, size_t, ((ptrdiff_t)(sizeof(ptrdiff_t) * 8)))
SKULD_INT_OPS_UNSIGNED(u8, uint8_t, 8)
SKULD_INT_OPS_UNSIGNED(u16, uint16_t, 16)
SKULD_INT_OPS_UNSIGNED(u32, uint32_t, 32)
SKULD_INT_OPS_UNSIGNED(u64, uint64_t, 64)
SKULD_INT_OPS_UNSIGNED(usize, size_t, (sizeof(size_t) * 8))

/* Conversions between widths are explicit in Skuld and trap when the value
 * does not fit. A signed source widens to int64_t and an unsigned one to
 * uint64_t, so two helpers per target cover every source. */
#define SKULD_INT_CONVERT(S, T, LO, HI)                                            \
    static inline T skuld_i_to_##S(int64_t v, size_t byte) {                       \
        if (v < (int64_t)(LO) || (v > 0 && (uint64_t)v > (uint64_t)(HI)))          \
            skuld_fail("integer conversion out of range", byte);                   \
        return (T)v;                                                               \
    }                                                                              \
    static inline T skuld_u_to_##S(uint64_t v, size_t byte) {                      \
        if (v > (uint64_t)(HI)) skuld_fail("integer conversion out of range", byte); \
        return (T)v;                                                               \
    }

SKULD_INT_CONVERT(i8, int8_t, INT8_MIN, INT8_MAX)
SKULD_INT_CONVERT(i16, int16_t, INT16_MIN, INT16_MAX)
SKULD_INT_CONVERT(i32, int32_t, INT32_MIN, INT32_MAX)
SKULD_INT_CONVERT(i64, int64_t, INT64_MIN, INT64_MAX)
SKULD_INT_CONVERT(isize, ptrdiff_t, PTRDIFF_MIN, PTRDIFF_MAX)
SKULD_INT_CONVERT(u8, uint8_t, 0, UINT8_MAX)
SKULD_INT_CONVERT(u16, uint16_t, 0, UINT16_MAX)
SKULD_INT_CONVERT(u32, uint32_t, 0, UINT32_MAX)
SKULD_INT_CONVERT(u64, uint64_t, 0, UINT64_MAX)
SKULD_INT_CONVERT(usize, size_t, 0, SIZE_MAX)

static inline int8_t skuld_f_to_i8(double v, size_t byte) {
    if (isnan(v) || v <= -129.0 || v >= 128.0) skuld_fail("float conversion out of range", byte);
    return (int8_t)v;
}
static inline int16_t skuld_f_to_i16(double v, size_t byte) {
    if (isnan(v) || v <= -32769.0 || v >= 32768.0) skuld_fail("float conversion out of range", byte);
    return (int16_t)v;
}
static inline int32_t skuld_f_to_i32(double v, size_t byte) {
    if (isnan(v) || v <= -2147483649.0 || v >= 2147483648.0) skuld_fail("float conversion out of range", byte);
    return (int32_t)v;
}
static inline int64_t skuld_f_to_i64(double v, size_t byte) {
    if (isnan(v) || v < -9223372036854775808.0 || v >= 9223372036854775808.0) skuld_fail("float conversion out of range", byte);
    return (int64_t)v;
}
static inline ptrdiff_t skuld_f_to_isize(double v, size_t byte) {
    if (sizeof(ptrdiff_t) == 4) {
        if (isnan(v) || v <= -2147483649.0 || v >= 2147483648.0) skuld_fail("float conversion out of range", byte);
    } else {
        if (isnan(v) || v < -9223372036854775808.0 || v >= 9223372036854775808.0) skuld_fail("float conversion out of range", byte);
    }
    return (ptrdiff_t)v;
}
static inline uint8_t skuld_f_to_u8(double v, size_t byte) {
    if (isnan(v) || v <= -1.0 || v >= 256.0) skuld_fail("float conversion out of range", byte);
    return (uint8_t)v;
}
static inline uint16_t skuld_f_to_u16(double v, size_t byte) {
    if (isnan(v) || v <= -1.0 || v >= 65536.0) skuld_fail("float conversion out of range", byte);
    return (uint16_t)v;
}
static inline uint32_t skuld_f_to_u32(double v, size_t byte) {
    if (isnan(v) || v <= -1.0 || v >= 4294967296.0) skuld_fail("float conversion out of range", byte);
    return (uint32_t)v;
}
static inline uint64_t skuld_f_to_u64(double v, size_t byte) {
    if (isnan(v) || v <= -1.0 || v >= 18446744073709551616.0) skuld_fail("float conversion out of range", byte);
    return (uint64_t)v;
}
static inline size_t skuld_f_to_usize(double v, size_t byte) {
    if (sizeof(size_t) == 4) {
        if (isnan(v) || v <= -1.0 || v >= 4294967296.0) skuld_fail("float conversion out of range", byte);
    } else {
        if (isnan(v) || v <= -1.0 || v >= 18446744073709551616.0) skuld_fail("float conversion out of range", byte);
    }
    return (size_t)v;
}

static inline uint32_t skuld_i_to_char(int64_t v, size_t byte) {
    if (v < 0 || v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF)) {
        skuld_fail("invalid Unicode code point", byte);
    }
    return (uint32_t)v;
}
static inline uint32_t skuld_u_to_char(uint64_t v, size_t byte) {
    if (v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF)) {
        skuld_fail("invalid Unicode code point", byte);
    }
    return (uint32_t)v;
}

"#;

/// The half of the prelude that needs a runtime and a libc behind it: string
/// comparison and every `print`. A freestanding program has neither, and the
/// checker has already refused the types that would reach these.
const PRELUDE_HOSTED: &str = r#"
static inline bool skuld_string_equal(skuld_string a, skuld_string b) {
    return a.len == b.len && memcmp(a.data, b.data, a.len) == 0;
}
static inline void skuld_print_int(int64_t value, size_t byte) {
    if (printf("%" PRId64 "\n", value) < 0) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_uint(uint64_t value, size_t byte) {
    if (printf("%" PRIu64 "\n", value) < 0) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_float(double value, size_t byte) {
    if (printf("%.17g\n", value) < 0) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_bool(bool value, size_t byte) {
    if (puts(value ? "true" : "false") == EOF) skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_char(uint32_t cp, size_t byte) {
    char bytes[4];
    size_t len = 0;
    if (cp <= 0x7F) {
        bytes[0] = (char)cp;
        len = 1;
    } else if (cp <= 0x7FF) {
        bytes[0] = (char)(0xC0 | (cp >> 6));
        bytes[1] = (char)(0x80 | (cp & 0x3F));
        len = 2;
    } else if (cp <= 0xFFFF) {
        bytes[0] = (char)(0xE0 | (cp >> 12));
        bytes[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        bytes[2] = (char)(0x80 | (cp & 0x3F));
        len = 3;
    } else if (cp <= 0x10FFFF) {
        bytes[0] = (char)(0xF0 | (cp >> 18));
        bytes[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
        bytes[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
        bytes[3] = (char)(0x80 | (cp & 0x3F));
        len = 4;
    } else {
        skuld_fail("invalid Unicode code point", byte);
    }
    if (fwrite(bytes, 1, len, stdout) != len || fputc('\n', stdout) == EOF)
        skuld_fail("stdout write failed", byte);
}
static inline void skuld_print_string(skuld_string value, size_t byte) {
    if (fwrite(value.data, 1, value.len, stdout) != value.len || fputc('\n', stdout) == EOF)
        skuld_fail("stdout write failed", byte);
}

"#;
