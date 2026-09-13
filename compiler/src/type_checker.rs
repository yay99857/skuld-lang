//! Semantic checking; produces tables for a separate AST-to-HIR lowering pass.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode},
    module::{Errors, FileDiagnostic, FileId, LoadedProgram, ModuleId, ROOT},
    resolver::{Builtin, Resolution, SymbolId, SymbolKind},
    span::Span,
    types::{
        ArrayId, ArrayInfo, EnumId, EnumInfo, FunctionTypeId, FunctionTypeInfo, IntType,
        InterfaceId, InterfaceInfo, InterfaceMethod, OptionId, OptionInfo, Pointee, ResultId,
        ResultInfo, StructId, Type, VariantInfo,
    },
};
use std::collections::BTreeMap;

/// The entry file, which is the only file of the root module today.
const ROOT_FILE: FileId = FileId(0);

#[derive(Debug, Clone)]
pub struct ExternInfo {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub parameters: Vec<Type>,
    pub return_type: Type,
}

/// Only successful checking can construct this object. Tables and syntax must
/// stay together; HIR lowering consumes them without repeating name lookup.
#[derive(Debug)]
pub struct TypedProgram {
    pub(crate) program: LoadedProgram,
    pub(crate) resolution: Resolution,
    pub(crate) expressions: BTreeMap<(FileId, usize, usize), Type>,
    pub(crate) symbol_types: Vec<Type>,
    pub(crate) signatures: BTreeMap<SymbolId, Signature>,
    /// Foreign functions, which have a signature but no body. The C name is the
    /// declared name: the backend calls it verbatim.
    pub(crate) externs: BTreeMap<SymbolId, ExternInfo>,
    pub(crate) entry: SymbolId,
    pub(crate) structs: Vec<StructInfo>,
    pub(crate) enums: Vec<EnumInfo>,
    pub(crate) interfaces: Vec<InterfaceInfo>,
    pub(crate) interface_wraps: BTreeMap<(FileId, usize, usize), Type>,
    /// Enum names by module, since two modules may each declare a `Tag`.
    pub(crate) enum_names: Vec<BTreeMap<String, EnumId>>,
    pub(crate) arrays: Vec<ArrayInfo>,
    pub(crate) options: Vec<OptionInfo>,
    pub(crate) results: Vec<ResultInfo>,
    pub(crate) implicit_wraps: BTreeMap<(FileId, usize, usize), Type>,
    pub(crate) function_signatures: Vec<FunctionTypeInfo>,
}
impl TypedProgram {
    pub fn enums(&self) -> &[EnumInfo] {
        &self.enums
    }
    pub fn options(&self) -> &[OptionInfo] {
        &self.options
    }
    pub fn results(&self) -> &[ResultInfo] {
        &self.results
    }
    pub fn arrays(&self) -> &[ArrayInfo] {
        &self.arrays
    }
    pub fn program(&self) -> &LoadedProgram {
        &self.program
    }
    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }
    /// Declared structs and classes, in declaration order, which is also what
    /// `Type::Struct` indexes. A tool that offers a field or a method needs
    /// this table; nothing inside the compiler needed it exposed before.
    pub fn structs(&self) -> &[StructInfo] {
        &self.structs
    }
    /// Declared interfaces, in declaration order, which is what
    /// `Type::Interface` indexes. Exposed for the same reason `structs` is: a
    /// tool that names a type needs the table, and nothing inside the compiler
    /// needed it exposed before.
    pub fn interfaces(&self) -> &[InterfaceInfo] {
        &self.interfaces
    }
    /// The signatures function values carry, which `Type::Function` indexes.
    /// A function type has no declaration to point at, so this table is the
    /// only place its parameters and result are written down.
    pub fn function_signatures(&self) -> &[FunctionTypeInfo] {
        &self.function_signatures
    }
    /// The checked type of a resolved symbol: a local, a parameter or a
    /// binding. A symbol that names a function has no value type, and reads
    /// as `Type::Error` here; ask `signature` for that instead.
    pub fn symbol_type(&self, id: SymbolId) -> Type {
        self.symbol_types.get(id.0).copied().unwrap_or(Type::Error)
    }
    /// The signature of a function, method or foreign declaration.
    pub fn signature(&self, id: SymbolId) -> Option<&Signature> {
        self.signatures.get(&id)
    }
    pub fn expression_type_in(&self, file: FileId, span: Span) -> Option<Type> {
        self.expressions.get(&(file, span.start, span.end)).copied()
    }
    /// The entry file's expression types. Every caller that walks a whole
    /// program uses `expression_type_in`; this is for single-file callers.
    pub fn expression_type(&self, span: Span) -> Option<Type> {
        self.expression_type_in(ROOT_FILE, span)
    }
}

/// The resolution must belong to this exact parser AST. All source-facing
/// callers should use `check`, which enforces phase ordering and ownership.
pub(crate) fn type_check(
    program: LoadedProgram,
    resolution: Resolution,
) -> Result<TypedProgram, Errors> {
    let mut checker = Checker {
        symbol_types: vec![Type::Error; resolution.symbols.len()],
        resolution: &resolution,
        file: ROOT_FILE,
        module: ROOT,
        expressions: BTreeMap::new(),
        signatures: BTreeMap::new(),
        externs: BTreeMap::new(),
        diagnostics: Vec::new(),
        return_type: Type::Void,
        loops: Vec::new(),
        jumps_escape: false,
        structs: Vec::new(),
        enums: Vec::new(),
        interfaces: Vec::new(),
        module_types: vec![ModuleTypes::default(); program.modules.len()],
        conformances: BTreeMap::new(),
        interface_wraps: BTreeMap::new(),
        arrays: Vec::new(),
        options: Vec::new(),
        option_types: BTreeMap::new(),
        results: Vec::new(),
        result_types: BTreeMap::new(),
        array_types: BTreeMap::new(),
        function_signatures: Vec::new(),
        function_types: BTreeMap::new(),
        expected_context: None,
        implicit_wraps: BTreeMap::new(),
    };
    // Where each type was declared, aligned with the ids handed out below, so
    // that a later pass finds its syntax without searching for it.
    let mut enum_sites: Vec<(FileId, usize)> = Vec::new();
    let mut struct_sites: Vec<(FileId, usize)> = Vec::new();
    for (index, file) in program.files.iter().enumerate() {
        checker.file = FileId(index);
        checker.module = file.module;
        for (position, declaration) in file.program.enums.iter().enumerate() {
            let id = EnumId(checker.enums.len());
            checker.declare_type(&declaration.name, TypeEntry::Enum(id));
            checker.enums.push(EnumInfo {
                name: declaration.name.text.clone(),
                module: file.module,
                visibility: declaration.visibility,
                variants: Vec::new(),
            });
            enum_sites.push((FileId(index), position));
        }
    }
    // Interface names come first so that a field, a parameter or another
    // interface's signature may mention one before it is filled in.
    let mut interface_sites: Vec<(FileId, usize)> = Vec::new();
    for (index, file) in program.files.iter().enumerate() {
        checker.file = FileId(index);
        checker.module = file.module;
        for (position, declaration) in file.program.interfaces.iter().enumerate() {
            let id = InterfaceId(checker.interfaces.len());
            checker.declare_type(&declaration.name, TypeEntry::Interface(id));
            checker.interfaces.push(InterfaceInfo {
                name: declaration.name.text.clone(),
                module: file.module,
                visibility: declaration.visibility,
                methods: Vec::new(),
            });
            interface_sites.push((FileId(index), position));
        }
    }
    // Structs are collected before signatures so functions may use them, and
    // before field types so a struct can refer to one declared later.
    for (index, file) in program.files.iter().enumerate() {
        checker.file = FileId(index);
        checker.module = file.module;
        for (position, declaration) in file.program.structs.iter().enumerate() {
            let id = StructId(checker.structs.len());
            checker.declare_type(&declaration.name, TypeEntry::Struct(id));
            checker.structs.push(StructInfo {
                name: declaration.name.text.clone(),
                module: file.module,
                visibility: declaration.visibility,
                reference: declaration.kind == TypeDeclKind::Reference,
                fields: Vec::new(),
                methods: Vec::new(),
                file: FileId(index),
                span: declaration.span,
            });
            struct_sites.push((FileId(index), position));
        }
    }
    for (index, &(file, position)) in interface_sites.iter().enumerate() {
        checker.file = file;
        checker.module = program.files[file.0].module;
        let declaration = &program.files[file.0].program.interfaces[position];
        let mut methods: Vec<InterfaceMethod> = Vec::new();
        for method in &declaration.methods {
            if methods
                .iter()
                .any(|existing| existing.name == method.name.text)
            {
                checker.error(
                    DiagnosticCode::DuplicateDeclaration,
                    method.name.span,
                    format!(
                        "`{}` is already declared in interface `{}`",
                        method.name.text, declaration.name.text
                    ),
                );
                continue;
            }
            let parameters = method
                .parameters
                .iter()
                .map(|parameter| checker.type_ref(&parameter.type_ref, false))
                .collect();
            let return_type = method
                .return_type
                .as_ref()
                .map(|reference| checker.type_ref(reference, true))
                .unwrap_or(Type::Void);
            methods.push(InterfaceMethod {
                name: method.name.text.clone(),
                parameters,
                return_type,
            });
        }
        checker.interfaces[index].methods = methods;
    }
    for (index, &(file, position)) in struct_sites.iter().enumerate() {
        checker.file = file;
        checker.module = program.files[file.0].module;
        let declaration = &program.files[file.0].program.structs[position];
        let mut fields: Vec<FieldInfo> = Vec::new();
        for field in &declaration.fields {
            let ty = checker.type_ref(&field.type_ref, false);
            checker.reject_stored_function(ty, field.type_ref.span(), "a field");
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
                default: field.default.clone(),
                span: field.span,
            });
        }
        checker.structs[index].fields = fields;
        checker.structs[index].file = file;
    }
    // A value type has no indirection, so containing itself — directly or
    // through other value types — would have no size. A class field is a
    // reference, which breaks any such chain.
    for (index, &(site_file, site_position)) in struct_sites.iter().enumerate() {
        if checker.structs[index].reference {
            continue;
        }
        if checker.contains_by_value(
            StructId(index),
            StructId(index),
            &mut vec![false; checker.structs.len()],
        ) {
            checker.file = site_file;
            let declaration = &program.files[site_file.0].program.structs[site_position];
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
    for (index, &(file, position)) in enum_sites.iter().enumerate() {
        checker.file = file;
        checker.module = program.files[file.0].module;
        let declaration = &program.files[file.0].program.enums[position];
        let mut variants: Vec<VariantInfo> = Vec::new();
        for variant in &declaration.variants {
            let payload = variant.payload.as_ref().map(|ty| {
                let payload = checker.type_ref(ty, false);
                checker.reject_stored_function(payload, ty.span(), "an enum payload");
                payload
            });
            if variants.iter().any(|v| v.name == variant.name.text) {
                checker.error(
                    DiagnosticCode::DuplicateDeclaration,
                    variant.name.span,
                    format!(
                        "variant `{}` is already declared in enum `{}`",
                        variant.name.text, declaration.name.text
                    ),
                );
                continue;
            }
            variants.push(VariantInfo {
                name: variant.name.text.clone(),
                payload,
            });
        }
        checker.enums[index].variants = variants;
    }
    for (index, &(site_file, site_position)) in enum_sites.iter().enumerate() {
        if checker.enum_contains_by_value(
            EnumId(index),
            EnumId(index),
            &mut vec![false; checker.enums.len()],
        ) {
            checker.file = site_file;
            let declaration = &program.files[site_file.0].program.enums[site_position];
            checker.error(
                DiagnosticCode::InvalidValueType,
                declaration.name.span,
                format!(
                    "enum `{}` contains itself by value; a value type has no indirection, so it would have no size",
                    declaration.name.text
                ),
            );
        }
    }
    // Method signatures come after fields so a method can use any field type,
    // and after every struct exists so signatures may mention other structs.
    for (index, &(file, position)) in struct_sites.iter().enumerate() {
        checker.file = file;
        checker.module = program.files[file.0].module;
        let declaration = &program.files[file.0].program.structs[position];
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
                .map(|r| {
                    let ty = checker.type_ref(r, true);
                    checker.reject_stored_function(ty, r.span(), "a return type");
                    ty
                })
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
                .get(&(checker.file, method.body.span.start))
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
    // Conformance, now that every method signature exists. It is checked
    // against the interface rather than inferred from what happens to match.
    for (index, &(file, position)) in struct_sites.iter().enumerate() {
        checker.file = file;
        checker.module = program.files[file.0].module;
        let declaration = &program.files[file.0].program.structs[position];
        if declaration.conforms.is_empty() {
            continue;
        }
        let id = StructId(index);
        if !checker.structs[index].reference {
            checker.error(
                DiagnosticCode::InvalidValueType,
                declaration.conforms[0].span,
                format!(
                    "`{}` is a struct; only a class implements an interface, because an interface value is a counted reference",
                    declaration.name.text
                ),
            );
            continue;
        }
        let mut implemented: Vec<InterfaceId> = Vec::new();
        for conformance in &declaration.conforms {
            let interface = match checker.lookup_type(conformance, "interface") {
                Some(TypeEntry::Interface(interface)) => interface,
                Some(_) => {
                    checker.error(
                        DiagnosticCode::TypeMismatch,
                        conformance.span,
                        format!("`{}` is not an interface", conformance.name.text),
                    );
                    continue;
                }
                None => continue,
            };
            if implemented.contains(&interface) {
                checker.error(
                    DiagnosticCode::DuplicateDeclaration,
                    conformance.span,
                    format!(
                        "`{}` is already implemented by `{}`",
                        conformance.name.text, declaration.name.text
                    ),
                );
                continue;
            }
            checker.check_conformance(id, interface, conformance.span, index);
            implemented.push(interface);
        }
        checker.conformances.insert(id, implemented);
    }
    // Foreign signatures come first: an ordinary function may call one, and
    // nothing about them depends on the rest of the program.
    for (index, file) in program.files.iter().enumerate() {
        checker.file = FileId(index);
        checker.module = file.module;
        for block in &file.program.externs {
            for function in &block.functions {
                let id = checker.declaration(&function.name);
                let mut parameters = Vec::new();
                for parameter in &function.parameters {
                    let ty = checker.type_ref(&parameter.type_ref, false);
                    checker.foreign_type(ty, parameter.type_ref.span(), false);
                    parameters.push(ty);
                }
                let return_type = match &function.return_type {
                    Some(reference) => {
                        let ty = checker.type_ref(reference, true);
                        checker.foreign_type(ty, reference.span(), true);
                        ty
                    }
                    None => Type::Void,
                };
                // The declared name is the linker name, so it must not collide with
                // what the backend emits for the program itself.
                if function.name.text == "main" || function.name.text.starts_with("skuld_") {
                    checker.error(
                    DiagnosticCode::InvalidValueType,
                    function.name.span,
                    format!(
                        "`{}` cannot be declared as a foreign function; the generated program already defines that symbol",
                        function.name.text
                    ),
                );
                }
                checker.externs.insert(
                    id,
                    ExternInfo {
                        name: function.name.text.clone(),
                        span: function.span,
                    },
                );
                checker.signatures.insert(
                    id,
                    Signature {
                        parameters,
                        return_type,
                    },
                );
            }
        }
    }
    for (index, file) in program.files.iter().enumerate() {
        checker.file = FileId(index);
        checker.module = file.module;
        for function in &file.program.functions {
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
                .map(|r| {
                    let ty = checker.type_ref(r, true);
                    checker.reject_stored_function(ty, r.span(), "a return type");
                    ty
                })
                .unwrap_or(Type::Void);
            checker.signatures.insert(
                id,
                Signature {
                    parameters,
                    return_type,
                },
            );
        }
    }
    // The entrypoint belongs to the entry file. A module is a library, so a
    // `main` in one is an ordinary function that nothing calls.
    checker.file = ROOT_FILE;
    checker.module = ROOT;
    let entry_main = program.files[ROOT_FILE.0]
        .program
        .functions
        .iter()
        .find(|f| f.name.text == "main");
    let entry = entry_main.map(|f| checker.declaration(&f.name));
    match entry {
        Some(id) if !checker.signatures[&id].parameters.is_empty() => {
            let function = entry_main.expect("entry exists");
            checker.error(
                DiagnosticCode::InvalidEntrypoint,
                function.name.span,
                "`func main()` cannot take parameters",
            );
        }
        Some(id) if checker.signatures[&id].return_type != Type::Void => {
            let function = entry_main.expect("entry exists");
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
    // Field defaults, now that every signature exists and before any body:
    // the expression is checked once, where it is written, and evaluated at
    // every construction.
    for (index, &(file, position)) in struct_sites.iter().enumerate() {
        checker.file = file;
        checker.module = program.files[file.0].module;
        let declaration = &program.files[file.0].program.structs[position];
        for field in &declaration.fields {
            let Some(default) = &field.default else {
                continue;
            };
            let Some((_, declared)) = checker.structs[index].field(&field.name.text) else {
                continue;
            };
            let expected = declared.ty;
            let previous = checker.expected_context;
            checker.expected_context = Some(expected);
            let found = checker.expression(default);
            checker.expected_context = previous;
            checker.expect_type(expected, found, default.span);
        }
    }
    for (index, file) in program.files.iter().enumerate() {
        checker.file = FileId(index);
        checker.module = file.module;
        for function in file
            .program
            .structs
            .iter()
            .flat_map(|declaration| declaration.methods.iter())
            .chain(file.program.functions.iter())
        {
            checker.return_type =
                checker.signatures[&checker.declaration(&function.name)].return_type;
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
    }
    if !checker.diagnostics.is_empty() {
        return Err(Errors {
            sources: program.sources(),
            diagnostics: checker.diagnostics,
        });
    }
    let Checker {
        structs,
        enums,
        interfaces,
        interface_wraps,
        function_signatures,
        module_types,
        arrays,
        options,
        results,
        expressions,
        symbol_types,
        signatures,
        externs,
        implicit_wraps,
        ..
    } = checker;
    // A missing entry always produces a diagnostic above.
    let entry = entry.expect("internal compiler bug: checked program has no entrypoint");
    let enum_names = module_types.into_iter().map(|types| types.enums).collect();
    Ok(TypedProgram {
        program,
        resolution,
        structs,
        enums,
        interfaces,
        interface_wraps,
        enum_names,
        arrays,
        options,
        results,
        implicit_wraps,
        expressions,
        symbol_types,
        signatures,
        externs,
        function_signatures,
        entry,
    })
}

struct Checker<'a> {
    resolution: &'a Resolution,
    /// The file being checked, and the module it belongs to. Both are part of
    /// every table key, since byte offsets repeat across files and type names
    /// repeat across modules.
    file: FileId,
    module: ModuleId,
    symbol_types: Vec<Type>,
    expressions: BTreeMap<(FileId, usize, usize), Type>,
    signatures: BTreeMap<SymbolId, Signature>,
    externs: BTreeMap<SymbolId, ExternInfo>,
    diagnostics: Vec<FileDiagnostic>,
    return_type: Type,
    /// One frame per enclosing loop, recording whether a `break` can exit it.
    /// Empty means a jump has no loop to bind to.
    loops: Vec<bool>,
    /// Whether a `break` or `continue` counts as leaving the block being
    /// checked. It does inside the escape block of a declaration that unwraps,
    /// where the question is whether control can fall through rather than
    /// whether the function returns.
    jumps_escape: bool,
    /// Declared structs in declaration order; `Type::Struct` indexes this.
    structs: Vec<StructInfo>,
    enums: Vec<EnumInfo>,
    interfaces: Vec<InterfaceInfo>,
    /// Type names by module. A type is reached unqualified from its own
    /// module, or qualified and public from another.
    module_types: Vec<ModuleTypes>,
    /// Which classes implement which interfaces, and in what order, so the
    /// backend can emit one table per pair that is actually used.
    conformances: BTreeMap<StructId, Vec<InterfaceId>>,
    /// The interface a class value was used as, by position, so lowering can
    /// build the pair without re-deriving the context.
    interface_wraps: BTreeMap<(FileId, usize, usize), Type>,
    /// Interned array types; `Type::Array` indexes this.
    arrays: Vec<ArrayInfo>,
    options: Vec<OptionInfo>,
    option_types: BTreeMap<Type, OptionId>,
    /// Interned `Result` types; `Type::Result` indexes this.
    results: Vec<ResultInfo>,
    result_types: BTreeMap<(Type, Type), ResultId>,
    array_types: BTreeMap<Type, ArrayId>,
    /// Interned function types; `Type::Function` indexes this.
    function_signatures: Vec<FunctionTypeInfo>,
    function_types: BTreeMap<FunctionTypeInfo, FunctionTypeId>,
    expected_context: Option<Type>,
    implicit_wraps: BTreeMap<(FileId, usize, usize), Type>,
}

/// One module's type namespace, which is separate from its value scope: a
/// struct and a function may share a name, as they already could.
#[derive(Debug, Default, Clone)]
struct ModuleTypes {
    structs: BTreeMap<String, StructId>,
    enums: BTreeMap<String, EnumId>,
    interfaces: BTreeMap<String, InterfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeEntry {
    Struct(StructId),
    Enum(EnumId),
    Interface(InterfaceId),
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    pub module: ModuleId,
    pub visibility: Visibility,
    /// A class is a reference to a shared object; a struct is a value.
    pub reference: bool,
    /// Field order is declaration order, which the backend layout follows.
    pub fields: Vec<FieldInfo>,
    pub methods: Vec<MethodInfo>,
    /// Where the declaration was written. A field default is an expression in
    /// that file, and its recorded types are keyed by it.
    pub file: FileId,
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
    /// `name: Type = expression`. A field with one may be left out of a
    /// construction; the expression is then evaluated there, once per object,
    /// in the file that declared the field.
    pub default: Option<crate::ast::Expr>,
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
        self.resolution.declarations[&(self.file, name.span.start)]
    }
    fn reference(&self, name: &Name) -> SymbolId {
        self.resolution.references[&(self.file, name.span.start)]
    }
    fn error(&mut self, code: DiagnosticCode, span: Span, message: impl Into<String>) {
        self.diagnostics.push(FileDiagnostic {
            file: self.file,
            diagnostic: Diagnostic {
                code,
                message: message.into(),
                span,
                help: None,
            },
        });
    }
    /// Record a type in the current module's namespace. Types live apart from
    /// value names, so a struct and a function may still share a spelling.
    fn declare_type(&mut self, name: &Name, entry: TypeEntry) {
        if matches!(name.text.as_str(), "Option" | "Result") {
            self.error(
                DiagnosticCode::DuplicateDeclaration,
                name.span,
                format!("`{}` is a builtin type and cannot be redeclared", name.text),
            );
        }
        let types = &self.module_types[self.module.0];
        if types.structs.contains_key(&name.text)
            || types.enums.contains_key(&name.text)
            || types.interfaces.contains_key(&name.text)
        {
            self.error(
                DiagnosticCode::DuplicateDeclaration,
                name.span,
                format!("type `{}` is already declared", name.text),
            );
            return;
        }
        let types = &mut self.module_types[self.module.0];
        match entry {
            TypeEntry::Struct(id) => {
                types.structs.insert(name.text.clone(), id);
            }
            TypeEntry::Enum(id) => {
                types.enums.insert(name.text.clone(), id);
            }
            TypeEntry::Interface(id) => {
                types.interfaces.insert(name.text.clone(), id);
            }
        }
    }
    /// The module a type path names: the current one, or the one an import
    /// qualifier binds in this file.
    fn path_module(&mut self, path: &Path) -> Option<ModuleId> {
        let Some(qualifier) = &path.module else {
            return Some(self.module);
        };
        match self.resolution.module_in_file(self.file, &qualifier.text) {
            Some(id) => Some(id),
            None => {
                self.error(
                    DiagnosticCode::UnknownModule,
                    qualifier.span,
                    format!("no module `{}` is imported in this file", qualifier.text),
                );
                None
            }
        }
    }
    /// Resolve a type name, enforcing that a qualified one is exported. An
    /// unqualified name never crosses a module boundary, so it needs no check.
    fn lookup_type(&mut self, path: &Path, noun: &str) -> Option<TypeEntry> {
        let module = self.path_module(path)?;
        let types = &self.module_types[module.0];
        let entry = types
            .structs
            .get(&path.name.text)
            .copied()
            .map(TypeEntry::Struct)
            .or_else(|| {
                types
                    .enums
                    .get(&path.name.text)
                    .copied()
                    .map(TypeEntry::Enum)
            })
            .or_else(|| {
                types
                    .interfaces
                    .get(&path.name.text)
                    .copied()
                    .map(TypeEntry::Interface)
            });
        let Some(entry) = entry else {
            self.error(
                DiagnosticCode::UnknownType,
                path.name.span,
                match &path.module {
                    Some(qualifier) => format!(
                        "module `{}` declares no {noun} `{}`",
                        qualifier.text, path.name.text
                    ),
                    None => format!("unknown or unsupported {noun} `{}`", path.name.text),
                },
            );
            return None;
        };
        if path.module.is_some() {
            let visibility = match entry {
                TypeEntry::Struct(id) => self.structs[id.0].visibility,
                TypeEntry::Enum(id) => self.enums[id.0].visibility,
                TypeEntry::Interface(id) => self.interfaces[id.0].visibility,
            };
            if visibility != Visibility::Public {
                self.error(
                    DiagnosticCode::PrivateName,
                    path.name.span,
                    format!("type `{}` is private to its module", path.name.text),
                );
                return None;
            }
        }
        Some(entry)
    }
    /// The enum named by the left of `Enum.Variant` or `module.Enum.Variant`.
    fn enum_prefix(&mut self, object: &Expr) -> Option<EnumId> {
        let path = match &object.kind {
            ExprKind::Identifier(name) => Path::bare(name.clone()),
            ExprKind::Member { object, member } => {
                let ExprKind::Identifier(qualifier) = &object.kind else {
                    return None;
                };
                // Only an import qualifier can precede an enum name here; a
                // value of the same name is a member access, not a path.
                self.resolution.module_in_file(self.file, &qualifier.text)?;
                Path {
                    module: Some(qualifier.clone()),
                    name: member.clone(),
                    span: object.span,
                }
            }
            _ => return None,
        };
        let module = match &path.module {
            None => self.module,
            Some(qualifier) => self.resolution.module_in_file(self.file, &qualifier.text)?,
        };
        let id = self.module_types[module.0]
            .enums
            .get(&path.name.text)
            .copied()?;
        // A private enum reached through a qualifier is already reported by the
        // resolver, which sees the same two names as a value path.
        if path.module.is_some() && self.enums[id.0].visibility != Visibility::Public {
            return None;
        }
        Some(id)
    }
    /// `Display` cannot reach the struct table, so every user-facing type name
    /// goes through here instead.
    fn type_name(&self, ty: Type) -> String {
        match ty {
            Type::Struct(id) => self.structs[id.0].name.clone(),
            Type::Enum(id) => self.enums[id.0].name.clone(),
            Type::Array(id) => format!("[]{}", self.type_name(self.arrays[id.0].element)),
            Type::Option(id) => format!("Option<{}>", self.type_name(self.options[id.0].element)),
            Type::Result(id) => format!(
                "Result<{}, {}>",
                self.type_name(self.results[id.0].ok),
                self.type_name(self.results[id.0].err)
            ),
            Type::Weak(id) => format!("weak {}", self.structs[id.0].name),
            Type::Interface(id) => self.interfaces[id.0].name.clone(),
            Type::Function(id) => {
                let info = &self.function_signatures[id.0];
                let parameters: Vec<_> = info
                    .parameters
                    .iter()
                    .map(|ty| self.type_name(*ty))
                    .collect();
                match info.return_type {
                    Type::Void => format!("({})", parameters.join(", ")),
                    other => format!("({}) -> {}", parameters.join(", "), self.type_name(other)),
                }
            }
            other => other.to_string(),
        }
    }
    /// Managed values never cross the foreign boundary: a C function knows
    /// nothing about retain and release, so only scalars, raw pointers and a
    /// `void` return may appear in an `extern` signature.
    fn foreign_type(&mut self, ty: Type, span: Span, is_return: bool) {
        let allowed = matches!(
            ty,
            Type::Int(_) | Type::Float | Type::Bool | Type::Pointer(_) | Type::Error
        ) || (is_return && ty == Type::Void);
        if !allowed {
            self.error(
                DiagnosticCode::InvalidValueType,
                span,
                format!(
                    "`{}` cannot cross the `extern \"C\"` boundary; only scalars and raw pointers can",
                    self.type_name(ty)
                ),
            );
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
    fn result_type(&mut self, ok: Type, err: Type) -> Type {
        if let Some(&id) = self.result_types.get(&(ok, err)) {
            return Type::Result(id);
        }
        let id = ResultId(self.results.len());
        self.results.push(ResultInfo { ok, err });
        self.result_types.insert((ok, err), id);
        Type::Result(id)
    }
    fn function_type(&mut self, parameters: Vec<Type>, return_type: Type) -> Type {
        let info = FunctionTypeInfo {
            parameters,
            return_type,
        };
        if let Some(&id) = self.function_types.get(&info) {
            Type::Function(id)
        } else {
            let id = FunctionTypeId(self.function_signatures.len());
            self.function_signatures.push(info.clone());
            self.function_types.insert(info, id);
            Type::Function(id)
        }
    }
    /// A function value may be a parameter or a local and nothing else. If a
    /// managed value could hold one, a closure that captured that value would
    /// close a cycle the reference counter has no way to collect.
    fn reject_stored_function(&mut self, ty: Type, span: Span, position: &str) {
        if matches!(ty, Type::Function(_)) {
            self.error(
                DiagnosticCode::InvalidValueType,
                span,
                format!(
                    "a function value cannot be {position}; it may only be a parameter or a local"
                ),
            );
        }
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
                let span = element.span();
                let element = self.type_ref(element, false);
                self.reject_stored_function(element, span, "an Option payload");
                if element == Type::Error {
                    Type::Error
                } else {
                    self.option_type(element)
                }
            }
            TypeRef::Result { ok, err, .. } => {
                let ok_type = self.type_ref(ok, false);
                self.reject_stored_function(ok_type, ok.span(), "a Result payload");
                let err_type = self.type_ref(err, false);
                self.reject_stored_function(err_type, err.span(), "a Result payload");
                // `type_ref` already rejects `void` in a payload position.
                if ok_type == Type::Error || err_type == Type::Error {
                    Type::Error
                } else {
                    self.result_type(ok_type, err_type)
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
            TypeRef::Function {
                parameters,
                return_type,
                span,
            } => {
                let parameters: Vec<_> = parameters
                    .iter()
                    .map(|parameter| {
                        let ty = self.type_ref(parameter, false);
                        self.reject_stored_function(
                            ty,
                            parameter.span(),
                            "a parameter of a function type",
                        );
                        ty
                    })
                    .collect();
                let result = match return_type {
                    Some(reference) => {
                        let ty = self.type_ref(reference, true);
                        self.reject_stored_function(
                            ty,
                            reference.span(),
                            "the result of a function type",
                        );
                        ty
                    }
                    None => Type::Void,
                };
                let _ = span;
                self.function_type(parameters, result)
            }
            TypeRef::Named(path) => {
                let name = path.name.clone();
                // A module exports no scalars, so only an unqualified name can
                // be one of the builtin spellings.
                let builtin = path.module.is_none();
                let ty = match name.text.as_str() {
                    // `int` and `i64` are two spellings of one type, not two
                    // types with a conversion between them.
                    "int" if builtin => Type::INT,
                    other if builtin && IntType::ALL.iter().any(|k| k.suffix() == other) => {
                        Type::Int(
                            *IntType::ALL
                                .iter()
                                .find(|k| k.suffix() == other)
                                .expect("matched width"),
                        )
                    }
                    "float" if builtin => Type::Float,
                    "bool" if builtin => Type::Bool,
                    "string" if builtin => Type::String,
                    "void" if builtin => Type::Void,
                    _ => match self.lookup_type(path, "type") {
                        Some(TypeEntry::Struct(id)) => Type::Struct(id),
                        Some(TypeEntry::Enum(id)) => Type::Enum(id),
                        Some(TypeEntry::Interface(id)) => Type::Interface(id),
                        // `lookup_type` reports whichever of unknown module,
                        // unknown name or private name applies.
                        None => Type::Error,
                    },
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
            TypeRef::Pointer { pointee, span } => {
                let inner = self.type_ref(pointee, true);
                match inner {
                    Type::Error => Type::Error,
                    Type::Void => Type::Pointer(Pointee::Void),
                    Type::Int(kind) => Type::Pointer(Pointee::Int(kind)),
                    Type::Float => Type::Pointer(Pointee::Float),
                    Type::Bool => Type::Pointer(Pointee::Bool),
                    other => {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            *span,
                            format!(
                                "a pointer may only point at a scalar or `void`, not `{}`",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            TypeRef::Array { element, span } => {
                // Checked before the array type is built, so the diagnostic
                // names the element rather than the array.
                let element_type = self.type_ref(element, false);
                self.reject_stored_function(element_type, element.span(), "an array element");
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
    fn expect_type(&mut self, expected: Type, found: Type, span: Span) -> bool {
        if expected == Type::Error || found == Type::Error {
            return false;
        }
        if expected == found {
            return true;
        }
        if let Type::Option(id) = expected {
            let element = self.options[id.0].element;
            if element == found {
                self.implicit_wraps
                    .insert((self.file, span.start, span.end), expected);
                return true;
            }
            // A class going into an expected `Option<Interface>` takes both
            // steps: it is seen through the interface, then wrapped.
            if let Type::Interface(interface) = element
                && self.conforms(found, interface)
            {
                self.interface_wraps
                    .insert((self.file, span.start, span.end), element);
                self.implicit_wraps
                    .insert((self.file, span.start, span.end), expected);
                return true;
            }
        }
        // A class widens to an interface it declared, the way a value wraps
        // into an expected Option: the declaration is what makes it safe.
        if let Type::Interface(interface) = expected
            && self.conforms(found, interface)
        {
            self.interface_wraps
                .insert((self.file, span.start, span.end), expected);
            return true;
        }
        self.error(
            DiagnosticCode::TypeMismatch,
            span,
            format!(
                "expected `{}`, found `{}`",
                self.type_name(expected),
                self.type_name(found)
            ),
        );
        false
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
                // With an escape block the annotation describes the unwrapped
                // name, so it cannot steer the initializer.
                self.expected_context = match variable.otherwise {
                    Some(_) => None,
                    None => annotated,
                };
                let inferred = self.expression(&variable.initializer);
                self.expected_context = previous_expected;
                if inferred == Type::Void {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        variable.initializer.span,
                        "cannot store a `void` expression in a variable",
                    );
                }
                let ty = match &variable.otherwise {
                    Some(otherwise) => self.otherwise(variable, otherwise, inferred, annotated),
                    None => {
                        if let Some(annotated) = annotated {
                            self.expect_type(annotated, inferred, variable.initializer.span);
                            annotated
                        } else {
                            inferred
                        }
                    }
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
                pattern,
                binding,
                value,
                then_block,
                else_branch,
            } => {
                let ty = self.expression(value);
                let payload = match (pattern, ty) {
                    (IfLetPattern::Some, Type::Option(id)) => self.options[id.0].element,
                    (IfLetPattern::Ok, Type::Result(id)) => self.results[id.0].ok,
                    (IfLetPattern::Err, Type::Result(id)) => self.results[id.0].err,
                    (_, Type::Error) => Type::Error,
                    (IfLetPattern::Some, other) => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            value.span,
                            format!(
                                "if let Some(...) requires an Option value, found `{}`",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                    (pattern, other) => {
                        let name = if *pattern == IfLetPattern::Ok {
                            "Ok"
                        } else {
                            "Err"
                        };
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            value.span,
                            format!(
                                "if let {name}(...) requires a Result value, found `{}`",
                                self.type_name(other)
                            ),
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
                // A jump never returns, but it does leave the block, which is
                // the question an escape block is asking.
                self.jumps_escape
            }
            StatementKind::Match { value, arms } => {
                let target_ty = self.expression(value);
                if target_ty == Type::Error {
                    for arm in arms {
                        self.block(&arm.body);
                    }
                    return false;
                }
                // A `Result` matches like a two-variant enum, so the arm and
                // exhaustiveness checking below is shared rather than repeated.
                let target_enum = match target_ty {
                    Type::Enum(enum_id) => Some(enum_id),
                    _ => None,
                };
                let enum_info = match target_ty {
                    Type::Enum(enum_id) => self.enums[enum_id.0].clone(),
                    Type::Result(result_id) => result_as_enum(self.results[result_id.0]),
                    other => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            value.span,
                            format!(
                                "match expects an enum or Result value, found `{}`",
                                self.type_name(other)
                            ),
                        );
                        for arm in arms {
                            self.block(&arm.body);
                        }
                        return false;
                    }
                };
                let mut covered = vec![false; enum_info.variants.len()];
                let mut has_wildcard = false;
                let mut all_arms_return = !arms.is_empty();

                for arm in arms {
                    match &arm.pattern {
                        MatchPattern::Wildcard(_) => {
                            has_wildcard = true;
                        }
                        MatchPattern::Variant {
                            enum_name,
                            variant_name,
                            binding,
                            span: _,
                        } => {
                            // A qualified pattern names its enum through
                            // the type namespace, so `a.Tag` and `b.Tag` are
                            // told apart by identity rather than spelling.
                            if let Some(path) = enum_name {
                                let resolved = self.lookup_type(path, "enum");
                                let mismatched = match (resolved, target_enum) {
                                    (Some(TypeEntry::Enum(id)), Some(expected)) => id != expected,
                                    // Unresolved names are already reported.
                                    (None, _) => false,
                                    _ => true,
                                };
                                if mismatched {
                                    self.error(
                                        DiagnosticCode::TypeMismatch,
                                        path.span,
                                        format!(
                                            "pattern belongs to enum `{}`, not `{}`",
                                            path.name.text, enum_info.name
                                        ),
                                    );
                                }
                            }
                            if let Some(variant_index) = enum_info.find_variant(&variant_name.text)
                            {
                                covered[variant_index] = true;
                                let expected_payload = enum_info.variants[variant_index].payload;
                                match (expected_payload, binding) {
                                    (Some(payload_ty), Some(binding_name)) => {
                                        let sym_id = self.declaration(binding_name);
                                        self.symbol_types[sym_id.0] = payload_ty;
                                    }
                                    (Some(_), None) => {
                                        self.error(
                                            DiagnosticCode::ArgumentCount,
                                            variant_name.span,
                                            format!(
                                                "variant `{}` has a payload; pattern must bind it with `(name)`",
                                                variant_name.text
                                            ),
                                        );
                                    }
                                    (None, Some(binding_name)) => {
                                        self.error(
                                            DiagnosticCode::ArgumentCount,
                                            binding_name.span,
                                            format!(
                                                "variant `{}` does not have a payload",
                                                variant_name.text
                                            ),
                                        );
                                    }
                                    (None, None) => {}
                                }
                            } else {
                                self.error(
                                    DiagnosticCode::UnknownName,
                                    variant_name.span,
                                    format!(
                                        "enum `{}` has no variant `{}`",
                                        enum_info.name, variant_name.text
                                    ),
                                );
                            }
                        }
                    }
                    let returns = self.block(&arm.body);
                    all_arms_return &= returns;
                }

                if !has_wildcard {
                    for (i, is_covered) in covered.iter().enumerate() {
                        if !is_covered {
                            self.error(
                                DiagnosticCode::NonExhaustiveMatch,
                                statement.span,
                                format!(
                                    "non-exhaustive match: variant `{}` is not covered",
                                    enum_info.variants[i].name
                                ),
                            );
                        }
                    }
                }

                all_arms_return && (has_wildcard || covered.iter().all(|&c| c))
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                let elem_ty = match iterable {
                    ForIterable::Range { start, end } => {
                        let previous = self.expected_context;
                        self.expected_context = Some(Type::INT);
                        let start_ty = self.expression(start);
                        let end_ty = self.expression(end);
                        self.expected_context = previous;
                        self.expect_type(Type::INT, start_ty, start.span);
                        self.expect_type(Type::INT, end_ty, end.span);
                        Type::INT
                    }
                    ForIterable::Expr(collection) => {
                        let collection_ty = self.expression(collection);
                        match collection_ty {
                            Type::Array(id) => self.arrays[id.0].element,
                            Type::Error => Type::Error,
                            other => {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    collection.span,
                                    format!(
                                        "expected array or range, found `{}`",
                                        self.type_name(other)
                                    ),
                                );
                                Type::Error
                            }
                        }
                    }
                };
                let id = self.resolution.declarations[&(self.file, variable.span.start)];
                self.symbol_types[id.0] = elem_ty;
                self.loops.push(false);
                self.block(body);
                self.loops.pop();
                false
            }
        }
    }
    /// `(a: int, b: int): int { ... }`. A parameter type may be omitted when
    /// the expected type supplies it, which is the same local inference a
    /// `let` already performs.
    fn lambda(&mut self, lambda: &Lambda, expected: Option<Type>) -> Type {
        let signature = match expected {
            Some(Type::Function(id)) => Some(self.function_signatures[id.0].clone()),
            _ => None,
        };
        if let Some(signature) = &signature
            && signature.parameters.len() != lambda.parameters.len()
        {
            self.error(
                DiagnosticCode::ArgumentCount,
                lambda.span,
                format!(
                    "expected a function of {} parameter(s), found one of {}",
                    signature.parameters.len(),
                    lambda.parameters.len()
                ),
            );
        }
        let mut parameters = Vec::new();
        for (index, parameter) in lambda.parameters.iter().enumerate() {
            let ty = match (&parameter.type_ref, signature.as_ref()) {
                (Some(reference), _) => self.type_ref(reference, false),
                (None, Some(signature)) if index < signature.parameters.len() => {
                    signature.parameters[index]
                }
                (None, _) => {
                    self.error(
                        DiagnosticCode::UnknownType,
                        parameter.span,
                        format!(
                            "`{}` needs a type here: nothing in this position says what the function's type is",
                            parameter.name.text
                        ),
                    );
                    Type::Error
                }
            };
            self.reject_stored_function(ty, parameter.span, "a parameter of a function value");
            let id = self.declaration(&parameter.name);
            self.symbol_types[id.0] = ty;
            parameters.push(ty);
        }
        let return_type = match (&lambda.return_type, signature.as_ref()) {
            (Some(reference), _) => self.type_ref(reference, true),
            (None, Some(signature)) => signature.return_type,
            (None, None) => Type::Void,
        };
        self.reject_stored_function(return_type, lambda.span, "the result of a function value");
        // The body returns from the lambda, not from the enclosing function,
        // and a `break` inside it has no enclosing loop to bind to.
        let outer_return = std::mem::replace(&mut self.return_type, return_type);
        let outer_loops = std::mem::take(&mut self.loops);
        // A `return` inside a lambda leaves the lambda, not the block that
        // built it, so it settles nothing about an enclosing escape block.
        let outer_jumps = std::mem::replace(&mut self.jumps_escape, false);
        let outer_expected = self.expected_context.take();
        let returns = self.block(&lambda.body);
        self.expected_context = outer_expected;
        self.jumps_escape = outer_jumps;
        self.loops = outer_loops;
        self.return_type = outer_return;
        if return_type != Type::Void && return_type != Type::Error && !returns {
            self.error(
                DiagnosticCode::MissingReturn,
                lambda.span,
                format!(
                    "this function value must return `{}` on every path",
                    self.type_name(return_type)
                ),
            );
        }
        self.function_type(parameters, return_type)
    }
    /// `let value = fallible() else reason { ... }`. The declaration unwraps,
    /// and the block is what happens when there is nothing to unwrap; it must
    /// not fall through, because the name it guards is in scope afterwards.
    fn otherwise(
        &mut self,
        variable: &VariableDecl,
        otherwise: &Otherwise,
        inferred: Type,
        annotated: Option<Type>,
    ) -> Type {
        let (payload, error) = match inferred {
            Type::Option(id) => (self.options[id.0].element, None),
            Type::Result(id) => {
                let info = self.results[id.0];
                (info.ok, Some(info.err))
            }
            Type::Error => (Type::Error, None),
            other => {
                self.error(
                    DiagnosticCode::TypeMismatch,
                    variable.initializer.span,
                    format!(
                        "`else` unwraps an Option or a Result, found `{}`",
                        self.type_name(other)
                    ),
                );
                (Type::Error, None)
            }
        };
        match (&otherwise.binding, error) {
            (Some(binding), Some(error)) => {
                let id = self.declaration(binding);
                self.symbol_types[id.0] = error;
            }
            (Some(binding), None) => {
                self.error(
                    DiagnosticCode::TypeMismatch,
                    binding.span,
                    "an Option carries no error to name; write `else { ... }`",
                );
            }
            (None, Some(_)) => {
                // Ignoring the error is allowed: the block may not need it.
            }
            (None, None) => {}
        }
        // A `break` or `continue` leaves the block as surely as a `return`
        // does, so both count while this block is being checked.
        let previous = std::mem::replace(&mut self.jumps_escape, true);
        let escapes = self.block(&otherwise.block);
        self.jumps_escape = previous;
        if !escapes {
            self.error(
                DiagnosticCode::MissingReturn,
                otherwise.span,
                format!(
                    "this block must not fall through: `{}` is in scope after it, and there would be nothing to bind",
                    variable.name.text
                ),
            );
        }
        if let Some(annotated) = annotated {
            self.expect_type(annotated, payload, variable.initializer.span);
            return annotated;
        }
        payload
    }
    /// Every signature the interface names must be present on the class, with
    /// exactly the same parameters and result. Nothing is inferred and nothing
    /// is coerced: a near miss is a mistake worth reporting.
    fn check_conformance(
        &mut self,
        id: StructId,
        interface: InterfaceId,
        span: Span,
        struct_index: usize,
    ) {
        let required = self.interfaces[interface.0].methods.clone();
        let interface_name = self.interfaces[interface.0].name.clone();
        let class_name = self.structs[struct_index].name.clone();
        for method in &required {
            let Some(found) = self.structs[id.0]
                .methods
                .iter()
                .find(|candidate| candidate.name == method.name)
                .map(|candidate| candidate.id)
            else {
                self.error(
                    DiagnosticCode::MissingField,
                    span,
                    format!(
                        "`{class_name}` declares it implements `{interface_name}` but has no `{}`",
                        method.name
                    ),
                );
                continue;
            };
            let Some(signature) = self.signatures.get(&found) else {
                continue;
            };
            let (parameters, return_type) = (signature.parameters.clone(), signature.return_type);
            if parameters != method.parameters || return_type != method.return_type {
                let expected = self.signature_name(&method.parameters, method.return_type);
                let actual = self.signature_name(&parameters, return_type);
                self.error(
                    DiagnosticCode::TypeMismatch,
                    span,
                    format!(
                        "`{class_name}.{}` is `{actual}`, and `{interface_name}` requires `{expected}`",
                        method.name
                    ),
                );
            }
        }
    }
    fn signature_name(&self, parameters: &[Type], return_type: Type) -> String {
        let rendered: Vec<_> = parameters.iter().map(|ty| self.type_name(*ty)).collect();
        match return_type {
            Type::Void => format!("({})", rendered.join(", ")),
            other => format!("({}) -> {}", rendered.join(", "), self.type_name(other)),
        }
    }
    /// Whether a class may be seen through an interface it declared.
    fn conforms(&self, ty: Type, interface: InterfaceId) -> bool {
        match ty {
            Type::Struct(id) => self
                .conformances
                .get(&id)
                .is_some_and(|list| list.contains(&interface)),
            _ => false,
        }
    }
    fn construction(&mut self, path: &Path, fields: &[FieldInit], new: bool) -> Type {
        let name = &path.name;
        let noun = if new { "class" } else { "struct" };
        let entry = self.lookup_type(path, noun);
        let Some(TypeEntry::Struct(id)) = entry else {
            if let Some(TypeEntry::Enum(_)) = entry {
                self.error(
                    DiagnosticCode::UnknownType,
                    name.span,
                    format!("`{}` is an enum, not a {noun}", name.text),
                );
            }
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
        // Every field without a default must be given a value: an object is
        // never partially initialized.
        let missing: Vec<_> = self.structs[id.0]
            .fields
            .iter()
            .zip(&initialized)
            .filter(|(field, done)| !**done && field.default.is_none())
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
    /// The type already recorded for an expression this pass has walked.
    fn expression_type_of(&self, expr: &Expr) -> Option<Type> {
        self.expressions
            .get(&(self.file, expr.span.start, expr.span.end))
            .copied()
    }
    fn record(&mut self, expr: &Expr, ty: Type) -> Type {
        self.expressions
            .insert((self.file, expr.span.start, expr.span.end), ty);
        ty
    }
    /// Only unary minus may consume the positive magnitude of a signed type's
    /// most negative value, which is one past what the literal alone accepts.
    fn minimum_magnitude(&mut self, expr: &Expr, kind: IntType) -> bool {
        if !kind.signed() {
            return false;
        }
        let is_min = match &expr.kind {
            ExprKind::Literal(Literal::Integer(value)) => *value == kind.min_magnitude(),
            ExprKind::Group(inner) => self.minimum_magnitude(inner, kind),
            _ => false,
        };
        if is_min {
            self.record(expr, Type::Int(kind));
        }
        is_min
    }
    fn expression(&mut self, expr: &Expr) -> Type {
        let expected = self.expected_context.take();
        let ty = match &expr.kind {
            ExprKind::Literal(literal) => match literal {
                Literal::Integer(value) => {
                    // An integer literal takes the width the context expects,
                    // defaulting to `int`. Nothing converts afterwards.
                    let kind = expected.and_then(Type::int_type).unwrap_or(IntType::I64);
                    if *value > kind.max_magnitude() {
                        self.error(
                            DiagnosticCode::IntegerRange,
                            expr.span,
                            format!("integer literal is outside the `{}` range", kind.name()),
                        );
                        Type::Error
                    } else {
                        Type::Int(kind)
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
                    SymbolKind::Builtin(Builtin::None) => {
                        match expected {
                            Some(ty @ Type::Option(_)) => ty,
                            _ => {
                                self.error(
                                DiagnosticCode::UnknownType,
                                expr.span,
                                format!("{} requires an expected Option type; add a type annotation", name.text),
                            );
                                Type::Error
                            }
                        }
                    }
                    SymbolKind::Builtin(Builtin::Ok | Builtin::Err) => {
                        self.error(
                            DiagnosticCode::ArgumentCount,
                            expr.span,
                            format!(
                                "`{}` is a Result constructor; call it with a value",
                                name.text
                            ),
                        );
                        Type::Error
                    }
                    SymbolKind::Builtin(Builtin::BytesToString) => {
                        self.error(
                            DiagnosticCode::ArgumentCount,
                            expr.span,
                            "`bytes_to_string` is a function; call it with a `[]u8`",
                        );
                        Type::Error
                    }
                    SymbolKind::Builtin(Builtin::Ptr) => {
                        self.error(
                            DiagnosticCode::ArgumentCount,
                            expr.span,
                            "`ptr` is a function; call it with a string or an array",
                        );
                        Type::Error
                    }
                    SymbolKind::Builtin(Builtin::IntConvert(_)) => {
                        self.error(
                            DiagnosticCode::ArgumentCount,
                            expr.span,
                            format!("`{}` is a conversion; call it with a value", name.text),
                        );
                        Type::Error
                    }
                    SymbolKind::Enum => {
                        self.error(
                            DiagnosticCode::UnsupportedFeature,
                            expr.span,
                            format!(
                                "`{}` is an enum type and cannot be used as a value",
                                name.text
                            ),
                        );
                        Type::Error
                    }
                    SymbolKind::Function => match self.signatures.get(&id) {
                        // A declared function is a function value like any
                        // lambda; it simply captures nothing.
                        Some(signature) => {
                            let (parameters, return_type) =
                                (signature.parameters.clone(), signature.return_type);
                            self.function_type(parameters, return_type)
                        }
                        None => Type::Error,
                    },
                    _ => {
                        self.error(
                            DiagnosticCode::UnsupportedFeature,
                            expr.span,
                            format!("`{}` is a module, not a value", name.text),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::Group(inner) => {
                self.expected_context = expected;
                self.expression(inner)
            }
            ExprKind::Try(inner) => {
                let ty = self.expression(inner);
                match (ty, self.return_type) {
                    (Type::Error, _) => Type::Error,
                    (Type::Result(value_id), Type::Result(return_id)) => {
                        let value = self.results[value_id.0];
                        let returned = self.results[return_id.0];
                        if value.err == returned.err {
                            value.ok
                        } else {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                expr.span,
                                format!(
                                    "`?` propagates `{}`, but this function returns errors of type `{}`",
                                    self.type_name(value.err),
                                    self.type_name(returned.err)
                                ),
                            );
                            Type::Error
                        }
                    }
                    (Type::Result(_), _) => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            expr.span,
                            "`?` is only valid inside a function returning `Result`",
                        );
                        Type::Error
                    }
                    (other, _) => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            inner.span,
                            format!(
                                "`?` requires a Result value, found `{}`",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::Unary {
                op,
                operand,
                op_span,
            } => {
                let width = expected.and_then(Type::int_type).unwrap_or(IntType::I64);
                if *op == UnaryOp::Negative && self.minimum_magnitude(operand, width) {
                    Type::Int(width)
                } else {
                    if *op != UnaryOp::Not {
                        self.expected_context = expected;
                    }
                    let ty = self.expression(operand);
                    let valid = match op {
                        UnaryOp::Not => ty == Type::Bool,
                        // Negating an unsigned value has a result only for
                        // zero, so it is rejected rather than trapped.
                        UnaryOp::Negative => {
                            ty == Type::Float || ty.int_type().is_some_and(IntType::signed)
                        }
                        UnaryOp::Positive => ty.is_numeric(),
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
                // The left operand takes the surrounding expectation and then
                // supplies it to the right, so `byte * 2` types the literal as
                // the left operand's width instead of defaulting to `int`.
                self.expected_context = expected;
                let left = self.expression(left);
                self.expected_context = Some(left);
                let right = self.expression(right);
                self.expected_context = None;
                self.binary(*op, left, right, *op_span)
            }
            ExprKind::Assignment {
                target,
                op,
                value,
                op_span,
            } => {
                let target_type = self.expression(target);
                if let ExprKind::Index { object, .. } = &strip_groups_ref(target).kind
                    && self.expression_type_of(object) == Some(Type::String)
                {
                    self.error(
                        DiagnosticCode::InvalidAssignment,
                        target.span,
                        "strings are immutable; build a `[]u8` and convert it instead",
                    );
                }
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
                            self.diagnostics.push(FileDiagnostic {
                                file: self.file,
                                diagnostic,
                            });
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
                if let Some(enum_id) = self.enum_prefix(object) {
                    let enum_info = &self.enums[enum_id.0];
                    if let Some(variant_index) = enum_info.find_variant(&member.text) {
                        let variant = &enum_info.variants[variant_index];
                        if variant.payload.is_some() {
                            self.error(
                                DiagnosticCode::ArgumentCount,
                                member.span,
                                format!(
                                    "variant `{}` expects a payload; call it with `(...)`",
                                    member.text
                                ),
                            );
                            Type::Error
                        } else {
                            Type::Enum(enum_id)
                        }
                    } else {
                        self.error(
                            DiagnosticCode::UnknownName,
                            member.span,
                            format!(
                                "enum `{}` has no variant `{}`",
                                self.enums[enum_id.0].name, member.text
                            ),
                        );
                        Type::Error
                    }
                } else {
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
                                self.diagnostics.push(FileDiagnostic {
                                    file: self.file,
                                    diagnostic,
                                });
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
                        } else if matches!(found, Type::Function(_)) {
                            // An array is a heap value; a function value's
                            // environment is on the stack. One must never hold
                            // the other, whether the element type was written
                            // or inferred from the elements themselves.
                            self.reject_stored_function(found, elem.span, "an array element");
                            has_error = true;
                        } else {
                            match elem_ty {
                                Some(expected) => {
                                    if !self.expect_type(expected, found, elem.span) {
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
            ExprKind::Slice { object, start, end } => {
                let object_type = self.expression(object);
                let previous_expected = self.expected_context;
                self.expected_context = Some(Type::INT);
                let start_type = self.expression(start);
                self.expected_context = Some(Type::INT);
                let end_type = self.expression(end);
                self.expected_context = previous_expected;
                let mut usable = true;
                for (ty, span) in [(start_type, start.span), (end_type, end.span)] {
                    if ty == Type::Error || !self.expect_type(Type::INT, ty, span) {
                        usable = false;
                    }
                }
                match object_type {
                    // A slice copies, so it never keeps a larger buffer alive
                    // through a short view of it.
                    Type::String if usable => Type::String,
                    Type::Array(id) if usable => Type::Array(id),
                    Type::String | Type::Array(_) | Type::Error => Type::Error,
                    other => {
                        self.error(
                            DiagnosticCode::InvalidOperator,
                            expr.span,
                            format!(
                                "cannot slice `{}`; only arrays and strings support slicing",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::Index { object, index } => {
                let object_type = self.expression(object);
                let previous_expected = self.expected_context;
                self.expected_context = Some(Type::INT);
                let index_type = self.expression(index);
                self.expected_context = previous_expected;
                if index_type != Type::Error {
                    self.expect_type(Type::INT, index_type, index.span);
                }
                let usable = index_type == Type::INT;
                match object_type {
                    Type::Array(id) if usable => self.arrays[id.0].element,
                    // Indexing a string reads one byte, not one character:
                    // Skuld strings are byte sequences and this milestone adds
                    // no code point type.
                    Type::String if usable => Type::Int(IntType::U8),
                    Type::Array(_) | Type::String | Type::Error => Type::Error,
                    other => {
                        self.error(
                            DiagnosticCode::InvalidOperator,
                            expr.span,
                            format!(
                                "cannot index into `{}`; only arrays and strings support indexing",
                                self.type_name(other)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::Lambda(lambda) => self.lambda(lambda, expected),
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
                        Type::Int(_) | Type::Float | Type::Bool | Type::String | Type::Error
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
        if left != right && !self.expect_type(left, right, span) {
            return Type::Error;
        }
        use BinaryOp::*;
        let valid = match op {
            // `+` also concatenates; the result is a new string.
            Add => left.is_numeric() || left == Type::String,
            Subtract | Multiply | Divide | Less | Greater | LessEqual | GreaterEqual => {
                left.is_numeric()
            }
            Modulo => left.int_type().is_some(),
            And | Or => left == Type::Bool,
            Equal | NotEqual => {
                matches!(left, Type::Int(_) | Type::Float | Type::Bool | Type::String)
            }
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
        if let Type::Array(id) = receiver {
            let element = self.arrays[id.0].element;
            let parameters = match member.text.as_str() {
                "push" => Some(vec![element]),
                "insert" => Some(vec![Type::INT, element]),
                "pop" => Some(vec![]),
                "remove" => Some(vec![Type::INT]),
                // A comparator returns a negative, zero or positive `int`,
                // the ordering convention the C library already uses.
                "sort" => {
                    let comparator = self.function_type(vec![element, element], Type::INT);
                    Some(vec![comparator])
                }
                _ => None,
            };
            if let Some(parameters) = parameters {
                if arguments.len() != parameters.len() {
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        format!(
                            "method `{}` expects {} arguments, found {}",
                            member.text,
                            parameters.len(),
                            arguments.len()
                        ),
                    );
                }
                let previous = self.expected_context;
                for (index, argument) in arguments.iter().enumerate() {
                    self.expected_context = parameters.get(index).copied();
                    let found = self.expression(argument);
                    if let Some(expected) = parameters.get(index) {
                        self.expect_type(*expected, found, argument.span);
                    }
                }
                self.expected_context = previous;
                return if matches!(member.text.as_str(), "pop" | "remove") {
                    self.option_type(element)
                } else {
                    Type::Void
                };
            }
        }
        let builtin = match (receiver, member.text.as_str()) {
            (Type::Array(_), "len") => Some(Type::INT),
            // A string's length is its byte count, matching what indexing and
            // slicing address.
            (Type::String, "len") => Some(Type::INT),
            (Type::String, "bytes") => {
                let byte = Type::Int(IntType::U8);
                Some(self.array_type(byte))
            }
            (Type::Weak(_), "alive") => Some(Type::Bool),
            (Type::Weak(id), "get") => Some(Type::Struct(id)),
            (Type::Weak(id), "upgrade") => Some(self.option_type(Type::Struct(id))),
            (Type::Option(_), "is_some" | "is_none") => Some(Type::Bool),
            (Type::Result(_), "is_ok" | "is_err") => Some(Type::Bool),
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
        // Dispatch through an interface: the method is looked up in the
        // interface's own table, not in whatever class happens to be inside.
        if let Type::Interface(interface) = receiver {
            let Some(method) = self.interfaces[interface.0]
                .methods
                .iter()
                .find(|candidate| candidate.name == member.text)
                .cloned()
            else {
                self.error(
                    DiagnosticCode::NotCallable,
                    member.span,
                    format!(
                        "interface `{}` has no method `{}`",
                        self.interfaces[interface.0].name, member.text
                    ),
                );
                for argument in arguments {
                    self.expression(argument);
                }
                return Type::Error;
            };
            if arguments.len() != method.parameters.len() {
                self.error(
                    DiagnosticCode::ArgumentCount,
                    span,
                    format!(
                        "method `{}` expects {} arguments, found {}",
                        member.text,
                        method.parameters.len(),
                        arguments.len()
                    ),
                );
            }
            let previous = self.expected_context;
            for (argument, expected) in arguments.iter().zip(&method.parameters) {
                self.expected_context = Some(*expected);
                let found = self.expression(argument);
                self.expect_type(*expected, found, argument.span);
            }
            self.expected_context = previous;
            for argument in arguments.iter().skip(method.parameters.len()) {
                self.expression(argument);
            }
            return method.return_type;
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
            if let Some(enum_id) = self.enum_prefix(object) {
                let enum_info = &self.enums[enum_id.0];
                return if let Some(variant_index) = enum_info.find_variant(&member.text) {
                    let variant = &enum_info.variants[variant_index];
                    match variant.payload {
                        None => {
                            self.error(
                                DiagnosticCode::ArgumentCount,
                                span,
                                format!("variant `{}` does not take arguments", member.text),
                            );
                            for arg in arguments {
                                self.expression(arg);
                            }
                            Type::Error
                        }
                        Some(expected_payload) => {
                            if arguments.len() != 1 {
                                self.error(
                                    DiagnosticCode::ArgumentCount,
                                    span,
                                    format!(
                                        "variant `{}` expects 1 argument, found {}",
                                        member.text,
                                        arguments.len()
                                    ),
                                );
                                for arg in arguments {
                                    self.expression(arg);
                                }
                                Type::Error
                            } else {
                                let previous = self.expected_context;
                                self.expected_context = Some(expected_payload);
                                let found = self.expression(&arguments[0]);
                                self.expected_context = previous;
                                self.expect_type(expected_payload, found, arguments[0].span);
                                Type::Enum(enum_id)
                            }
                        }
                    }
                } else {
                    self.error(
                        DiagnosticCode::UnknownName,
                        member.span,
                        format!(
                            "enum `{}` has no variant `{}`",
                            self.enums[enum_id.0].name, member.text
                        ),
                    );
                    for arg in arguments {
                        self.expression(arg);
                    }
                    Type::Error
                };
            }
            // `geometry.area(p)` is one name in two halves: a direct call to
            // an exported function, not a method on a value called `geometry`.
            let module_call = matches!(&object.kind, ExprKind::Identifier(qualifier)
                if self
                    .resolution
                    .module_in_file(self.file, &qualifier.text)
                    .is_some())
                && self
                    .resolution
                    .references
                    .contains_key(&(self.file, member.span.start));
            if !module_call {
                return self.method_call(object, member, arguments, span);
            }
        }
        let id = match &direct.kind {
            ExprKind::Identifier(name) => Some(self.reference(name)),
            // Only a module-qualified call reaches here as a member.
            ExprKind::Member { member, .. } => Some(self.reference(member)),
            _ => None,
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
                } else if matches!(element, Type::Function(_)) {
                    self.reject_stored_function(element, arguments[0].span, "an Option payload");
                    Type::Error
                } else if element == Type::Error {
                    Type::Error
                } else {
                    self.option_type(element)
                }
            }
            Some((_, SymbolKind::Builtin(Builtin::Ptr))) => {
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        "`ptr` expects exactly one argument",
                    );
                    return Type::Error;
                }
                let previous = self.expected_context;
                self.expected_context = None;
                let found = self.expression(&arguments[0]);
                self.expected_context = previous;
                let pointee = match found {
                    Type::Error => return Type::Error,
                    Type::String => Some(Pointee::Int(IntType::U8)),
                    Type::Array(id) => match self.arrays[id.0].element {
                        Type::Int(kind) => Some(Pointee::Int(kind)),
                        Type::Float => Some(Pointee::Float),
                        Type::Bool => Some(Pointee::Bool),
                        _ => None,
                    },
                    _ => None,
                };
                match pointee {
                    Some(pointee) => Type::Pointer(pointee),
                    None => {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            arguments[0].span,
                            format!(
                                "`ptr` borrows the bytes of a string or an array of scalars, not `{}`",
                                self.type_name(found)
                            ),
                        );
                        Type::Error
                    }
                }
            }
            Some((_, SymbolKind::Builtin(Builtin::BytesToString))) => {
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        "`bytes_to_string` expects exactly one argument",
                    );
                    return Type::Error;
                }
                let bytes = self.array_type(Type::Int(IntType::U8));
                self.expected_context = Some(bytes);
                let found = self.expression(&arguments[0]);
                self.expected_context = None;
                if found == Type::Error {
                    Type::Error
                } else if found != bytes {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        arguments[0].span,
                        format!(
                            "`bytes_to_string` expects `[]u8`, found `{}`",
                            self.type_name(found)
                        ),
                    );
                    Type::Error
                } else {
                    // The error side is a message rather than a dedicated type:
                    // no error enum belongs in the language before a standard
                    // library exists to own one.
                    self.result_type(Type::String, Type::String)
                }
            }
            Some((_, SymbolKind::Builtin(Builtin::IntConvert(kind)))) => {
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        format!("`{}` converts exactly one value", kind.name()),
                    );
                    return Type::Error;
                }
                // Expecting the target width lets a literal argument be
                // range-checked here instead of trapping at run time. It
                // reaches a literal only: imposing the width on a whole
                // expression would make `u8(128 + n % 64)` a type error, since
                // `n` is an `int` and widths never mix. A computed argument
                // types itself and is checked when it is converted.
                let previous = self.expected_context;
                self.expected_context = literal_int(&arguments[0]).then_some(Type::Int(kind));
                let found = self.expression(&arguments[0]);
                self.expected_context = previous;
                if found == Type::Error {
                    Type::Error
                } else if found.int_type().is_none() {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        arguments[0].span,
                        format!(
                            "`{}` converts an integer, found `{}`",
                            kind.name(),
                            self.type_name(found)
                        ),
                    );
                    Type::Error
                } else {
                    Type::Int(kind)
                }
            }
            Some((_, SymbolKind::Builtin(builtin @ (Builtin::Ok | Builtin::Err)))) => {
                let label = if builtin == Builtin::Ok { "Ok" } else { "Err" };
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        format!("{label} expects exactly one argument"),
                    );
                    return Type::Error;
                }
                // Only one side of a `Result` is written at a construction, so
                // the other has to come from the context. Inferring it would
                // mean guessing an error type from a success value.
                let Some(Type::Result(id)) = expected else {
                    self.expression(&arguments[0]);
                    self.error(
                        DiagnosticCode::UnknownType,
                        span,
                        format!(
                            "{label} requires an expected Result type; annotate the binding or the return type"
                        ),
                    );
                    return Type::Error;
                };
                let payload = if builtin == Builtin::Ok {
                    self.results[id.0].ok
                } else {
                    self.results[id.0].err
                };
                self.expected_context = Some(payload);
                let found = self.expression(&arguments[0]);
                self.expected_context = None;
                self.expect_type(payload, found, arguments[0].span);
                Type::Result(id)
            }
            Some((_, SymbolKind::Builtin(Builtin::None))) => {
                for argument in arguments {
                    self.expression(argument);
                }
                let name = if let ExprKind::Identifier(name) = &direct.kind {
                    name.text.as_str()
                } else {
                    "None"
                };
                self.error(
                    DiagnosticCode::NotCallable,
                    callee.span,
                    format!("{name} is a value; use {name} without parentheses"),
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
                        Type::Int(_) | Type::Float | Type::Bool | Type::String | Type::Error
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
                // The callee is evaluated first, as it is at run time, so a
                // function value held in a local or a parameter can be called.
                let ty = self.expression(callee);
                if let Type::Function(function) = ty {
                    let signature = self.function_signatures[function.0].clone();
                    if signature.parameters.len() != arguments.len() {
                        self.error(
                            DiagnosticCode::ArgumentCount,
                            span,
                            format!(
                                "this function value expects {} argument(s), found {}",
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
                    return signature.return_type;
                }
                for arg in arguments {
                    self.expression(arg);
                }
                if ty != Type::Error {
                    self.error(
                        DiagnosticCode::NotCallable,
                        callee.span,
                        format!("value of type `{}` is not callable", self.type_name(ty)),
                    );
                }
                Type::Error
            }
        }
    }
}

impl Checker<'_> {
    /// Every value stored inline in `ty`: the type itself, or what an `Option`
    /// or `Result` keeps inside it. Sizing and cycle detection have to see
    /// through both, because neither adds indirection.
    fn inline_payloads(&self, ty: Type) -> Vec<Type> {
        match ty {
            Type::Option(id) => self.inline_payloads(self.options[id.0].element),
            Type::Result(id) => {
                let info = self.results[id.0];
                let mut payloads = self.inline_payloads(info.ok);
                payloads.extend(self.inline_payloads(info.err));
                payloads
            }
            other => vec![other],
        }
    }
    /// Whether `target` is reachable from `from` through value-typed fields.
    fn contains_by_value(&self, from: StructId, target: StructId, seen: &mut Vec<bool>) -> bool {
        if seen[from.0] {
            return false;
        }
        seen[from.0] = true;
        self.structs[from.0].fields.iter().any(|field| {
            self.inline_payloads(field.ty)
                .into_iter()
                .any(|ty| match ty {
                    Type::Struct(id) if !self.structs[id.0].reference => {
                        id == target || self.contains_by_value(id, target, seen)
                    }
                    Type::Enum(id) => self.enum_contains_struct_by_value(id, target),
                    _ => false,
                })
        })
    }
    fn enum_contains_struct_by_value(&self, from: EnumId, target: StructId) -> bool {
        self.enums[from.0].variants.iter().any(|v| {
            v.payload.is_some_and(|payload| {
                self.inline_payloads(payload)
                    .into_iter()
                    .any(|ty| match ty {
                        Type::Struct(id) if !self.structs[id.0].reference => id == target,
                        Type::Enum(id) => self.enum_contains_struct_by_value(id, target),
                        _ => false,
                    })
            })
        })
    }
    fn enum_contains_by_value(&self, from: EnumId, target: EnumId, seen: &mut Vec<bool>) -> bool {
        if seen[from.0] {
            return false;
        }
        seen[from.0] = true;
        self.enums[from.0].variants.iter().any(|v| {
            v.payload.is_some_and(|payload| {
                self.inline_payloads(payload)
                    .into_iter()
                    .any(|ty| match ty {
                        Type::Enum(id) => {
                            id == target || self.enum_contains_by_value(id, target, seen)
                        }
                        Type::Struct(id) if !self.structs[id.0].reference => {
                            self.struct_contains_enum_by_value(id, target)
                        }
                        _ => false,
                    })
            })
        })
    }
    fn struct_contains_enum_by_value(&self, from: StructId, target: EnumId) -> bool {
        self.structs[from.0].fields.iter().any(|field| {
            self.inline_payloads(field.ty)
                .into_iter()
                .any(|ty| match ty {
                    Type::Enum(id) => id == target,
                    Type::Struct(id) if !self.structs[id.0].reference => {
                        self.struct_contains_enum_by_value(id, target)
                    }
                    _ => false,
                })
        })
    }
    /// True when the assignment writes into an object reached by reference.
    fn through_reference(&self, target: &Expr) -> bool {
        match &target.kind {
            ExprKind::Member { object, .. } => {
                let ty = self
                    .expressions
                    .get(&(self.file, object.span.start, object.span.end))
                    .copied();
                matches!(ty, Some(Type::Struct(id)) if self.structs[id.0].reference)
                    || self.through_reference(object)
            }
            ExprKind::Index { object, .. } => {
                let ty = self
                    .expressions
                    .get(&(self.file, object.span.start, object.span.end))
                    .copied();
                matches!(ty, Some(Type::Array(_))) || self.through_reference(object)
            }
            ExprKind::Group(inner) => self.through_reference(inner),
            _ => false,
        }
    }
}

/// A `Result` seen as the two-variant enum it behaves like. `Ok` is variant 0
/// and `Err` variant 1, the same order the backend gives its tag.
fn result_as_enum(info: ResultInfo) -> EnumInfo {
    EnumInfo {
        name: "Result".into(),
        // A builtin belongs to no module and is visible everywhere.
        module: ROOT,
        visibility: Visibility::Public,
        variants: vec![
            VariantInfo {
                name: "Ok".into(),
                payload: Some(info.ok),
            },
            VariantInfo {
                name: "Err".into(),
                payload: Some(info.err),
            },
        ],
    }
}

/// Looks past redundant parentheses without consuming the expression.
/// Whether an argument is an integer literal, possibly signed or parenthesised.
/// Only such an argument can carry an expected width, because only it has no
/// type of its own.
fn literal_int(expr: &Expr) -> bool {
    match &strip_groups_ref(expr).kind {
        ExprKind::Literal(Literal::Integer(_)) => true,
        ExprKind::Unary {
            op: UnaryOp::Negative | UnaryOp::Positive,
            operand,
            ..
        } => literal_int(operand),
        _ => false,
    }
}

fn strip_groups_ref(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Group(inner) => strip_groups_ref(inner),
        _ => expr,
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
