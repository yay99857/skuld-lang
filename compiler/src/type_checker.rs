//! Semantic checking; produces tables for a separate AST-to-HIR lowering pass.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode, Fix, nearest},
    module::{Errors, FileDiagnostic, FileId, LoadedProgram, ModuleId, ROOT},
    resolver::{Builtin, Resolution, SymbolId, SymbolKind},
    span::Span,
    types::{
        ArrayId, ArrayInfo, ConstValue, EnumId, EnumInfo, FixedArrayId, FixedArrayInfo,
        FunctionTypeId, FunctionTypeInfo, IntType, InterfaceId, InterfaceInfo, InterfaceMethod,
        OptionId, OptionInfo, Pointee, ResultId, ResultInfo, StructId, Type, VariantInfo,
        int_type_fits, int_type_min, sign_extend,
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
    /// The program's `main`, when it has one. A program checked for a tool may
    /// not: see `Entrypoint`.
    pub(crate) entry: Option<SymbolId>,
    pub(crate) structs: Vec<StructInfo>,
    pub(crate) enums: Vec<EnumInfo>,
    pub(crate) interfaces: Vec<InterfaceInfo>,
    pub(crate) interface_wraps: BTreeMap<(FileId, usize, usize), Type>,
    /// Enum names by module, since two modules may each declare a `Tag`.
    pub(crate) enum_names: Vec<BTreeMap<String, EnumId>>,
    pub(crate) arrays: Vec<ArrayInfo>,
    pub(crate) fixed_arrays: Vec<FixedArrayInfo>,
    pub(crate) options: Vec<OptionInfo>,
    pub(crate) results: Vec<ResultInfo>,
    pub(crate) implicit_wraps: BTreeMap<(FileId, usize, usize), Type>,
    pub(crate) slice_coercions: BTreeMap<(FileId, usize, usize), Type>,
    /// The type, and field where there is one, that a `size_of` or `offset_of`
    /// call asked about, by call position.
    pub(crate) layout_queries: BTreeMap<(FileId, usize, usize), (StructId, Option<usize>)>,
    pub(crate) function_signatures: Vec<FunctionTypeInfo>,
    pub(crate) constants: BTreeMap<SymbolId, ConstValue>,
}
impl TypedProgram {
    pub fn constants(&self) -> &BTreeMap<SymbolId, ConstValue> {
        &self.constants
    }
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
    pub fn fixed_arrays(&self) -> &[FixedArrayInfo] {
        &self.fixed_arrays
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
    /// The program's entrypoint, which a program checked with
    /// `Entrypoint::Optional` may not have.
    pub fn entry(&self) -> Option<SymbolId> {
        self.entry
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

/// Whether a program must have an entrypoint.
///
/// A compiler always requires one. A tool showing one file of a program — an
/// editor — must not: a module is a library, and the file on screen may not be
/// a program at all. Nothing else about the check changes, which is the point:
/// the tool sees exactly what the compiler sees, minus a rule that is about
/// building rather than about meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entrypoint {
    Required,
    Optional,
}

/// The resolution must belong to this exact parser AST. All source-facing
/// callers should use `check`, which enforces phase ordering and ownership.
pub(crate) fn type_check(
    program: LoadedProgram,
    resolution: Resolution,
    entrypoint: Entrypoint,
) -> Result<TypedProgram, Errors> {
    let mut checker = Checker {
        symbol_types: vec![Type::Error; resolution.symbols.len()],
        resolution: &resolution,
        files: &program.files,
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
        fixed_arrays: Vec::new(),
        fixed_array_types: BTreeMap::new(),
        slice_coercions: BTreeMap::new(),
        options: Vec::new(),
        option_types: BTreeMap::new(),
        results: Vec::new(),
        result_types: BTreeMap::new(),
        array_types: BTreeMap::new(),
        function_signatures: Vec::new(),
        function_types: BTreeMap::new(),
        expected_context: None,
        implicit_wraps: BTreeMap::new(),
        constants: BTreeMap::new(),
        evaluating_constants: Vec::new(),
        constant_decls: BTreeMap::new(),
        place_writes: 0,
        layout_queries: BTreeMap::new(),
        unsafe_depth: 0,
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
                underlying: None,
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
                union: declaration.kind == TypeDeclKind::Union,
                layout: declaration.layout.clone(),
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
        // A type that describes memory somebody else defined may hold only
        // what that other language can see. A managed field would put a
        // reference count inside a layout C decides, where nothing would
        // retain or release it.
        if declaration.layout.is_foreign() {
            let field_types: Vec<Type> = checker.structs[index]
                .fields
                .iter()
                .map(|field| field.ty)
                .collect();
            let noun = match declaration.kind {
                TypeDeclKind::Union => "an `extern union`",
                _ => "an `extern struct`",
            };
            for (field, ty) in declaration.fields.iter().zip(field_types) {
                if !checker.foreign_layout_type(ty) {
                    let rendered = checker.type_name(ty);
                    checker.error(
                        DiagnosticCode::InvalidValueType,
                        field.type_ref.span(),
                        format!(
                            "`{rendered}` cannot be a member of {noun}; its layout is the C compiler's, so every member must be one C can describe"
                        ),
                    );
                }
            }
            if let crate::ast::Layout::Foreign {
                align: Some((value, span)),
                ..
            } = declaration.layout
                && (value == 0 || !value.is_power_of_two() || value > 4096)
            {
                checker.error(
                    DiagnosticCode::IntegerRange,
                    span,
                    "`align` takes a power of two between 1 and 4096",
                );
            }
        }
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
        // `enum Protocol: u8`. The values below are then worth something
        // outside the program, which is the whole reason to write one.
        let underlying = declaration.underlying.as_ref().and_then(|reference| {
            let ty = checker.type_ref(reference, false);
            match ty {
                Type::Int(kind) => Some(kind),
                Type::Error => None,
                other => {
                    let rendered = checker.type_name(other);
                    checker.error(
                        DiagnosticCode::InvalidValueType,
                        reference.span(),
                        format!("an enum is numbered by an integer type, not by `{rendered}`"),
                    );
                    None
                }
            }
        });
        checker.enums[index].underlying = underlying;
        // Where a variant writes no value it continues from the one before,
        // starting at zero, the way C numbers an enumeration.
        let mut next_value: i128 = 0;
        let mut variants: Vec<VariantInfo> = Vec::new();
        for variant in &declaration.variants {
            let payload = variant.payload.as_ref().map(|ty| {
                let payload = checker.type_ref(ty, false);
                checker.reject_stored_function(payload, ty.span(), "an enum payload");
                payload
            });
            let position_value = variants.len() as i128;
            let value = match (&variant.value, underlying) {
                (Some(expression), Some(kind)) => {
                    match checker.eval_constant_expr(expression, Some(Type::Int(kind))) {
                        Some(ConstValue::Int(value, _)) => value,
                        Some(_) => {
                            checker.error(
                                DiagnosticCode::TypeMismatch,
                                expression.span,
                                "a variant's value is an integer",
                            );
                            next_value
                        }
                        None => next_value,
                    }
                }
                (Some(expression), None) => {
                    // An underlying type that was written but did not resolve
                    // is already reported; saying it is missing too would be
                    // the same mistake twice.
                    if declaration.underlying.is_none() {
                        checker.error(
                            DiagnosticCode::UnsupportedSyntax,
                            expression.span,
                            format!(
                                "`{}` gives its variants values, so it needs an integer type: `enum {}: i32`",
                                declaration.name.text, declaration.name.text
                            ),
                        );
                    }
                    position_value
                }
                (None, Some(_)) => next_value,
                (None, None) => position_value,
            };
            if underlying.is_some() {
                if payload.is_some() {
                    checker.error(
                        DiagnosticCode::InvalidValueType,
                        variant.span,
                        "a variant with a payload has no integer value, so its enum cannot name one",
                    );
                }
                if let Some(kind) = underlying
                    && !fits_int_type(value, kind)
                {
                    checker.error(
                        DiagnosticCode::IntegerRange,
                        variant.span,
                        format!("value `{value}` does not fit in `{}`", kind.name()),
                    );
                }
                // Two variants worth the same integer would make the
                // conversion back ambiguous and one of them unreachable.
                if let Some(existing) = variants.iter().find(|other| other.value == value) {
                    checker.error(
                        DiagnosticCode::DuplicateDeclaration,
                        variant.span,
                        format!(
                            "`{}` is already worth {value}, so `{}` would be unreachable",
                            existing.name, variant.name.text
                        ),
                    );
                }
                next_value = value.saturating_add(1);
            }
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
                value,
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
        None if entrypoint == Entrypoint::Required => checker.error(
            DiagnosticCode::InvalidEntrypoint,
            Span::new(0, 0),
            "missing entrypoint `func main()`",
        ),
        None => {}
    }
    // Constants, collected across all files and evaluated.
    for (file_idx, file) in program.files.iter().enumerate() {
        let file_id = FileId(file_idx);
        for constant in &file.program.constants {
            let sym_id = checker.resolution.declarations[&(file_id, constant.name.span.start)];
            checker
                .constant_decls
                .insert(sym_id, (file_id, constant.clone()));
        }
    }
    for (file_idx, file) in program.files.iter().enumerate() {
        let file_id = FileId(file_idx);
        for constant in &file.program.constants {
            let sym_id = checker.resolution.declarations[&(file_id, constant.name.span.start)];
            checker.ensure_constant_evaluated(sym_id);
        }
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
        fixed_arrays,
        options,
        results,
        expressions,
        symbol_types,
        signatures,
        externs,
        implicit_wraps,
        slice_coercions,
        layout_queries,
        constants,
        ..
    } = checker;
    // A missing entry is a diagnostic above unless the caller allowed one, and
    // then there is no entrypoint to record.
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
        fixed_arrays,
        options,
        results,
        implicit_wraps,
        slice_coercions,
        layout_queries,
        expressions,
        symbol_types,
        signatures,
        externs,
        function_signatures,
        entry,
        constants,
    })
}

struct Checker<'a> {
    resolution: &'a Resolution,
    /// The program's files, for the rare diagnostic whose fix is written in
    /// terms of the source text rather than of the syntax tree: an arm added
    /// to a `match` has to land at the indentation the rest of the block uses.
    files: &'a [crate::module::LoadedFile],
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
    fixed_arrays: Vec<FixedArrayInfo>,
    fixed_array_types: BTreeMap<(Type, usize), FixedArrayId>,
    slice_coercions: BTreeMap<(FileId, usize, usize), Type>,
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
    constants: BTreeMap<SymbolId, ConstValue>,
    evaluating_constants: Vec<SymbolId>,
    constant_decls: BTreeMap<SymbolId, (FileId, ConstantDecl)>,
    /// Whether the expression being checked is the target of a plain
    /// assignment, where a union member is being written rather than read.
    place_writes: usize,
    /// What `size_of` and `offset_of` were asked about, by call position, so
    /// lowering finds the type and field without re-reading the syntax.
    layout_queries: BTreeMap<(FileId, usize, usize), (StructId, Option<usize>)>,
    /// How many `unsafe` blocks enclose what is being checked. The pointer
    /// builtins read and write memory the compiler cannot vouch for, so they
    /// are refused wherever this is zero.
    unsafe_depth: usize,
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
    /// One piece of memory read as one of several types. Which member is live
    /// is the program's claim, which is why reading one is written inside
    /// `unsafe`.
    pub union: bool,
    /// The compiler's own layout, or the platform C compiler's for a type that
    /// describes memory somebody else defined.
    pub layout: crate::ast::Layout,
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
                fix: None,
            },
        });
    }
    /// Report a member that is not there, naming the one a typo probably meant
    /// and carrying the edit that writes it. The candidates are the members
    /// the receiver actually has, so a suggestion is never a name that would
    /// fail for a second reason.
    fn unknown_member(
        &mut self,
        code: DiagnosticCode,
        written: &Name,
        message: String,
        candidates: Vec<String>,
    ) {
        let mut diagnostic = Diagnostic {
            code,
            span: written.span,
            message,
            help: None,
            fix: None,
        };
        if let Some(meant) = nearest(&written.text, candidates.iter().map(String::as_str)) {
            diagnostic.help = Some(format!("did you mean `{meant}`?"));
            diagnostic = diagnostic.with_fix(Fix::new(
                format!("change to `{meant}`"),
                written.span,
                meant,
            ));
        }
        self.diagnostics.push(FileDiagnostic {
            file: self.file,
            diagnostic,
        });
    }
    /// The `let` in front of a declared name, turned into a `var`.
    ///
    /// The resolver records where the name was written and not where its
    /// keyword was, so the keyword is read back out of the source: the fix is
    /// offered only when the bytes before the name really are `let`, which a
    /// binding introduced any other way — an `if let`, a `for`, an arm
    /// pattern, an escape block — never is.
    fn let_into_var(&self, declared: Span) -> Option<Fix> {
        let text = &self.files[self.file.0].source;
        let before = text.get(..declared.start)?.trim_end();
        let keyword = before.strip_suffix("let")?;
        // `let` has to be a word of its own: `varlet x` is not a declaration.
        if keyword
            .chars()
            .next_back()
            .is_some_and(|last| last.is_alphanumeric() || last == '_')
        {
            return None;
        }
        Some(Fix::new(
            "declare it with `var`",
            Span::new(keyword.len(), keyword.len() + 3),
            "var",
        ))
    }
    /// The arm that would cover a variant nothing matches, written where the
    /// closing brace of the `match` is and indented one step past it.
    ///
    /// It is offered only where the pattern can be spelled from inside this
    /// file: an enum from another module is named through the qualifier that
    /// file imported it under, which is a fact about the file rather than
    /// about the type, so nothing is offered there rather than a name that
    /// does not resolve.
    fn missing_arm(
        &self,
        statement: Span,
        target: Type,
        enum_info: &EnumInfo,
        variant: &VariantInfo,
    ) -> Option<Fix> {
        // A `Result` is matched as `Ok(x)` and `Err(e)`, with no type name in
        // front; every other enum names itself.
        let pattern = match (target, &variant.payload) {
            (Type::Result(_), Some(_)) => format!("{}(value)", variant.name),
            (Type::Result(_), None) => variant.name.clone(),
            _ if enum_info.module != self.module => return None,
            (_, Some(_)) => format!("{}.{}(value)", enum_info.name, variant.name),
            (_, None) => format!("{}.{}", enum_info.name, variant.name),
        };
        let text = &self.files[self.file.0].source;
        // The statement ends just past its closing brace, which is what the
        // new arm goes in front of.
        let brace = statement.end.checked_sub(1)?;
        if text.as_bytes().get(brace) != Some(&b'}') {
            return None;
        }
        let line_start = text[..brace].rfind('\n').map_or(0, |index| index + 1);
        // Where the closing brace opens its own line, its indentation is the
        // block's and the arm goes one step further in. A `match` written on
        // one line has no such line to copy, so the arm takes the statement's
        // own indentation and opens a line of its own.
        if text[line_start..brace].trim().is_empty() {
            // The brace opens its own line: the arm becomes a whole line of
            // its own in front of it, indented one step past the block.
            let indent = &text[line_start..brace];
            return Some(Fix::new(
                format!("add an arm for `{pattern}`"),
                Span::new(line_start, line_start),
                format!("{indent}    {pattern}: {{}}\n"),
            ));
        }
        // A `match` written on one line has no such line to copy, so the arm
        // opens one, indented past the statement, and the brace follows on a
        // line of its own.
        let statement_line = text[..statement.start].rfind('\n').map_or(0, |i| i + 1);
        let indent: String = text[statement_line..statement.start]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        Some(Fix::new(
            format!("add an arm for `{pattern}`"),
            Span::new(brace, brace),
            format!("\n{indent}    {pattern}: {{}}\n{indent}"),
        ))
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
    /// The enum a resolved symbol declares. The resolver records an enum name
    /// as a symbol so that shadowing works on it; the type itself lives in the
    /// checker's own namespace, which is what this crosses back to.
    fn enum_of_symbol(&self, id: SymbolId) -> Option<EnumId> {
        let symbol = &self.resolution.symbols[id.0];
        let module = symbol.module.unwrap_or(self.module);
        self.module_types[module.0].enums.get(&symbol.name).copied()
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
            Type::FixedArray(id) => format!(
                "[{}]{}",
                self.fixed_arrays[id.0].size,
                self.type_name(self.fixed_arrays[id.0].element)
            ),
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
    /// nothing about retain and release, so only scalars, raw pointers, a type
    /// whose layout is C's own and a `void` return may appear in an `extern`
    /// signature.
    fn foreign_type(&mut self, ty: Type, span: Span, is_return: bool) {
        let allowed = self.foreign_layout_type(ty) || (is_return && ty == Type::Void);
        if !allowed {
            let extra = match ty {
                Type::Struct(id) if !self.structs[id.0].reference => {
                    "; declare it `extern struct` to give it the layout C expects"
                }
                _ => "",
            };
            self.error(
                DiagnosticCode::InvalidValueType,
                span,
                format!(
                    "`{}` cannot cross the `extern \"C\"` boundary; only scalars, raw pointers and `extern struct` types can{extra}",
                    self.type_name(ty)
                ),
            );
        }
    }
    /// Whether a value of this type has a layout C can describe: a scalar, a
    /// raw pointer, a fixed array of such, or an `extern struct` of them.
    ///
    /// The ordinary struct layout is the compiler's own and unspecified on
    /// purpose, which is exactly why it is not in this list.
    fn foreign_layout_type(&self, ty: Type) -> bool {
        match ty {
            Type::Int(_) | Type::Float | Type::Bool | Type::Char | Type::Pointer(_) => true,
            // Recovery: a second diagnostic about a type that is already wrong
            // would be noise.
            Type::Error => true,
            Type::FixedArray(id) => self.foreign_layout_type(self.fixed_arrays[id.0].element),
            Type::Struct(id) => {
                !self.structs[id.0].reference && self.structs[id.0].layout.is_foreign()
            }
            _ => false,
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
    fn fixed_array_type(&mut self, element: Type, size: usize) -> Type {
        if let Some(&id) = self.fixed_array_types.get(&(element, size)) {
            Type::FixedArray(id)
        } else {
            let id = FixedArrayId(self.fixed_arrays.len());
            self.fixed_arrays.push(FixedArrayInfo { element, size });
            self.fixed_array_types.insert((element, size), id);
            Type::FixedArray(id)
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
                    "char" if builtin => Type::Char,
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
                    Type::Char => Type::Pointer(Pointee::Char),
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
            TypeRef::FixedArray {
                element,
                size,
                span,
            } => {
                let size_val = self.eval_constant_expr(size, Some(Type::Int(IntType::USize)));
                let count = match size_val {
                    Some(ConstValue::Int(v, _)) => {
                        if v <= 0 {
                            self.error(
                                DiagnosticCode::IntegerRange,
                                size.span,
                                "fixed array size must be greater than zero",
                            );
                            None
                        } else if v > (i32::MAX as i128) {
                            self.error(
                                DiagnosticCode::IntegerRange,
                                size.span,
                                "fixed array size is too large",
                            );
                            None
                        } else {
                            Some(v as usize)
                        }
                    }
                    Some(_) => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            size.span,
                            "fixed array size must be an integer",
                        );
                        None
                    }
                    None => None,
                };
                let element_type = self.type_ref(element, false);
                self.reject_stored_function(element_type, element.span(), "an array element");
                match count {
                    Some(count) if element_type != Type::Error => {
                        if element_type == Type::Void {
                            self.error(
                                DiagnosticCode::InvalidValueType,
                                *span,
                                "array element type cannot be `void`",
                            );
                            Type::Error
                        } else {
                            self.fixed_array_type(element_type, count)
                        }
                    }
                    _ => Type::Error,
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
        if let Type::Array(expected_array) = expected {
            let expected_elem = self.arrays[expected_array.0].element;
            if let Type::FixedArray(found_fixed) = found {
                let fixed_info = self.fixed_arrays[found_fixed.0];
                if fixed_info.element == expected_elem {
                    self.slice_coercions
                        .insert((self.file, span.start, span.end), expected);
                    return true;
                }
            }
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
        // Any pointer is also an opaque one. C makes this conversion
        // implicitly, and it is the one pointer conversion that cannot be
        // wrong: `*void` points at no particular type, so nothing can be read
        // through it and nothing about the pointee is being claimed. Without
        // it every call taking a `void *` would be written
        // `ptr_from(addr(p))` inside an `unsafe` block, which is a block that
        // suspends real guarantees to express a conversion that suspends
        // none.
        if expected == Type::Pointer(Pointee::Void) && matches!(found, Type::Pointer(_)) {
            return true;
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
                    let mut help = None;
                    if let ExprKind::Call { callee, .. } = &variable.initializer.kind
                        && let ExprKind::Member { member, .. } = &callee.kind
                        && member.text == "sort"
                    {
                        help = Some("method `sort` mutates the array in-place and returns `void`; use `to_sorted` to obtain a sorted copy, or call `sort` as a separate statement".to_string());
                    }
                    self.diagnostics.push(FileDiagnostic {
                        file: self.file,
                        diagnostic: Diagnostic {
                            code: DiagnosticCode::InvalidValueType,
                            message: "cannot store a `void` expression in a variable".to_string(),
                            span: variable.initializer.span,
                            help,
                            fix: None,
                        },
                    });
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
            StatementKind::Constant(constant) => {
                let id = self.declaration(&constant.name);
                self.check_constant_decl(constant, id);
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
                let span = value.as_ref().map(|e| e.span).unwrap_or(statement.span);
                if let (Type::Array(_), Type::FixedArray(_)) = (self.return_type, found) {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        span,
                        "cannot return a fixed-size array as a dynamic array slice; slice explicitly",
                    );
                    return true;
                }
                self.expect_type(self.return_type, found, span);
                true
            }
            StatementKind::Block(block) => self.block(block),
            StatementKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let returns = self.block(block);
                self.unsafe_depth -= 1;
                returns
            }
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
                if !matches!(target_ty, Type::Enum(_) | Type::Result(_)) {
                    let is_scalar = matches!(
                        target_ty,
                        Type::Int(_) | Type::Float | Type::Bool | Type::String | Type::Char
                    );
                    if !is_scalar {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            value.span,
                            format!(
                                "match expects an enum, Result, integer, float, bool, char, or string, found `{}`",
                                self.type_name(target_ty)
                            ),
                        );
                        for arm in arms {
                            self.block(&arm.body);
                        }
                        return false;
                    }
                    let mut has_wildcard = false;
                    let mut all_arms_return = !arms.is_empty();
                    for arm in arms {
                        match &arm.pattern {
                            MatchPattern::Wildcard(_) => {
                                has_wildcard = true;
                            }
                            MatchPattern::Constant(c_expr) => {
                                self.expected_context = Some(target_ty);
                                let const_ty = self.expression(c_expr);
                                self.expected_context = None;
                                self.expect_type(target_ty, const_ty, c_expr.span);
                            }
                            MatchPattern::Range {
                                start,
                                end,
                                inclusive: _,
                                span,
                            } => {
                                if target_ty.int_type().is_none() && target_ty != Type::Char {
                                    self.error(
                                        DiagnosticCode::InvalidOperator,
                                        *span,
                                        format!(
                                            "range patterns are only supported for integer and char types, found `{target_ty}`"
                                        ),
                                    );
                                }
                                self.expected_context = Some(target_ty);
                                let start_ty = self.expression(start);
                                let end_ty = self.expression(end);
                                self.expected_context = None;
                                self.expect_type(target_ty, start_ty, start.span);
                                self.expect_type(target_ty, end_ty, end.span);
                            }
                            MatchPattern::Variant {
                                enum_name: _,
                                variant_name,
                                binding,
                                span: _,
                            } => {
                                if binding.is_some() {
                                    self.error(
                                        DiagnosticCode::ArgumentCount,
                                        variant_name.span,
                                        "scalar match pattern does not support payload bindings",
                                    );
                                }
                                if let Some(&sym_id) = self
                                    .resolution
                                    .references
                                    .get(&(self.file, variant_name.span.start))
                                {
                                    if matches!(
                                        self.resolution.symbols[sym_id.0].kind,
                                        SymbolKind::Constant
                                    ) {
                                        let sym_ty = self.symbol_types[sym_id.0];
                                        let const_ty = if sym_ty == Type::Error {
                                            self.ensure_constant_evaluated(sym_id)
                                                .map(|v| v.ty())
                                                .unwrap_or(Type::Error)
                                        } else {
                                            sym_ty
                                        };
                                        self.expect_type(target_ty, const_ty, variant_name.span);
                                    } else {
                                        self.error(
                                            DiagnosticCode::TypeMismatch,
                                            variant_name.span,
                                            format!("`{}` is not a constant", variant_name.text),
                                        );
                                    }
                                } else {
                                    self.error(
                                        DiagnosticCode::UnknownName,
                                        variant_name.span,
                                        format!(
                                            "unknown constant `{}` in pattern",
                                            variant_name.text
                                        ),
                                    );
                                }
                            }
                        }
                        let returns = self.block(&arm.body);
                        all_arms_return &= returns;
                    }
                    if !has_wildcard {
                        self.error(
                            DiagnosticCode::NonExhaustiveMatch,
                            statement.span,
                            "non-exhaustive match: value matches require a wildcard `_` arm",
                        );
                    }
                    return all_arms_return && has_wildcard;
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
                    _ => unreachable!(),
                };
                let mut covered = vec![false; enum_info.variants.len()];
                let mut has_wildcard = false;
                let mut all_arms_return = !arms.is_empty();

                for arm in arms {
                    match &arm.pattern {
                        MatchPattern::Wildcard(_) => {
                            has_wildcard = true;
                        }
                        MatchPattern::Constant(c_expr) => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                c_expr.span,
                                "constant pattern is not valid when matching an enum or Result",
                            );
                        }
                        MatchPattern::Range { span, .. } => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *span,
                                "range pattern is not valid when matching an enum or Result",
                            );
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
                                let message = format!(
                                    "enum `{}` has no variant `{}`",
                                    enum_info.name, variant_name.text
                                );
                                let variants = enum_info
                                    .variants
                                    .iter()
                                    .map(|variant| variant.name.clone())
                                    .collect();
                                self.unknown_member(
                                    DiagnosticCode::UnknownName,
                                    variant_name,
                                    message,
                                    variants,
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
                            let variant = &enum_info.variants[i];
                            let mut diagnostic = Diagnostic {
                                code: DiagnosticCode::NonExhaustiveMatch,
                                span: statement.span,
                                message: format!(
                                    "non-exhaustive match: variant `{}` is not covered",
                                    variant.name
                                ),
                                help: None,
                                fix: None,
                            };
                            if let Some(fix) =
                                self.missing_arm(statement.span, target_ty, &enum_info, variant)
                            {
                                diagnostic = diagnostic.with_fix(fix);
                            }
                            self.diagnostics.push(FileDiagnostic {
                                file: self.file,
                                diagnostic,
                            });
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
                            Type::FixedArray(id) => self.fixed_arrays[id.0].element,
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
            (None, None) if lambda.is_expression => {
                if let Some(Statement {
                    kind: StatementKind::Return(Some(expr)),
                    ..
                }) = lambda.body.statements.first()
                {
                    let previous_expected = self.expected_context.take();
                    let inferred = self.expression(expr);
                    self.expected_context = previous_expected;
                    inferred
                } else {
                    Type::Void
                }
            }
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
                let message = format!(
                    "`{}` has no field `{}`",
                    self.structs[id.0].name, field.name.text
                );
                let names = self.structs[id.0]
                    .fields
                    .iter()
                    .map(|declared| declared.name.clone())
                    .collect();
                self.unknown_member(DiagnosticCode::UnknownName, &field.name, message, names);
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
            if let (Type::Array(_), Type::FixedArray(_)) = (declared.ty, found) {
                self.error(
                    DiagnosticCode::InvalidValueType,
                    field.value.span,
                    "cannot initialize a field expecting a dynamic array with a fixed-size array; slice explicitly",
                );
                continue;
            }
            self.expect_type(declared.ty, found, field.value.span);
        }
        // A union is one piece of memory, so exactly one member is written and
        // that member is the one that is live.
        if self.structs[id.0].union {
            let written = initialized.iter().filter(|done| **done).count();
            if written != 1 {
                self.error(
                    DiagnosticCode::MissingField,
                    name.span,
                    format!(
                        "a union is written one member at a time; `{}` was given {written}",
                        self.structs[id.0].name
                    ),
                );
            }
            return Type::Struct(id);
        }
        // Every field without a default must be given a value: an object is
        // never partially initialized.
        let missing: Vec<_> = self.structs[id.0]
            .fields
            .iter()
            .zip(&initialized)
            .filter(|(field, done)| !**done && field.default.is_none())
            .map(|(field, _)| (field.name.clone(), field.ty))
            .collect();
        if !missing.is_empty() {
            let names: Vec<String> = missing
                .iter()
                .map(|(field, _)| format!("`{field}`"))
                .collect();
            // The types come with the names, because the next thing the
            // reader does is write a value of each. There is no fix here and
            // there should not be: what the values are is the one thing the
            // compiler does not know, and Skuld has no zero value to put in
            // their place.
            let shape: Vec<String> = missing
                .iter()
                .map(|(field, ty)| format!("`{field}: {}`", self.type_name(*ty)))
                .collect();
            let mut diagnostic = Diagnostic {
                code: DiagnosticCode::MissingField,
                span: name.span,
                message: format!(
                    "`{}` is missing {}",
                    self.structs[id.0].name,
                    names.join(", ")
                ),
                help: None,
                fix: None,
            };
            diagnostic.help = Some(format!("give it {}", shape.join(", ")));
            self.diagnostics.push(FileDiagnostic {
                file: self.file,
                diagnostic,
            });
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
                Literal::Char(_) => Type::Char,
            },
            ExprKind::Identifier(name) => {
                let id = self.reference(name);
                match self.resolution.symbols[id.0].kind {
                    SymbolKind::Variable(_) | SymbolKind::Parameter => self.symbol_types[id.0],
                    SymbolKind::Constant => {
                        let ty = self.symbol_types[id.0];
                        if ty == Type::Error {
                            if let Some(val) = self.ensure_constant_evaluated(id) {
                                val.ty()
                            } else {
                                Type::Error
                            }
                        } else {
                            ty
                        }
                    }
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
                        UnaryOp::BitNot => ty.int_type().is_some(),
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
                // Writing a union member is the program deciding which member
                // is live, which needs no claim; only reading one does. A
                // compound assignment reads first, so it is not a plain write.
                let plain_write = *op == AssignmentOp::Assign;
                self.place_writes += usize::from(plain_write);
                let target_type = self.expression(target);
                self.place_writes -= usize::from(plain_write);
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
                                fix: None,
                            };
                            diagnostic.help = Some(if symbol.kind == SymbolKind::Parameter {
                                "parameters are immutable; copy the value into a local `var`".into()
                            } else {
                                "declare the variable with `var` to allow assignment".into()
                            });
                            // A parameter has no keyword to change: making it
                            // assignable is a different edit, in a different
                            // place, and the reader decides where.
                            if symbol.kind != SymbolKind::Parameter
                                && let Some(declared) = symbol.span
                                && let Some(fix) = self.let_into_var(declared)
                            {
                                diagnostic = diagnostic.with_fix(fix);
                            }
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
                if let (Type::Array(_), Type::FixedArray(_)) = (target_type, value_type)
                    && self.through_reference(target)
                {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        value.span,
                        "cannot assign a fixed-size array to a class field expecting a dynamic array; slice explicitly",
                    );
                    return target_type;
                }
                self.expect_type(target_type, value_type, value.span);
                if *op != AssignmentOp::Assign {
                    let binary = match op {
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
                    self.binary(binary, target_type, value_type, *op_span);
                }
                target_type
            }
            ExprKind::Call { callee, arguments } => {
                self.call(callee, arguments, expr.span, expected)
            }
            ExprKind::Member { object, member } => {
                if matches!(&object.kind, ExprKind::Identifier(qualifier) if self.resolution.module_in_file(self.file, &qualifier.text).is_some())
                    && let Some(&sym_id) = self
                        .resolution
                        .references
                        .get(&(self.file, member.span.start))
                    && matches!(self.resolution.symbols[sym_id.0].kind, SymbolKind::Constant)
                {
                    let ty = self.symbol_types[sym_id.0];
                    return if ty == Type::Error {
                        if let Some(val) = self.ensure_constant_evaluated(sym_id) {
                            val.ty()
                        } else {
                            Type::Error
                        }
                    } else {
                        ty
                    };
                }
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
                        let message = format!(
                            "enum `{}` has no variant `{}`",
                            self.enums[enum_id.0].name, member.text
                        );
                        let variants = self.enums[enum_id.0]
                            .variants
                            .iter()
                            .map(|variant| variant.name.clone())
                            .collect();
                        self.unknown_member(DiagnosticCode::UnknownName, member, message, variants);
                        Type::Error
                    }
                } else {
                    let object_type = self.expression(object);
                    match object_type {
                        Type::Struct(id) => match self.structs[id.0]
                            .field(&member.text)
                            .map(|(_, field)| field.ty)
                        {
                            Some(field_type) => {
                                // Which member of a union is live is the
                                // program's claim and not the compiler's
                                // knowledge, so reading one is written where a
                                // reader can see the claim being made.
                                if self.structs[id.0].union && self.place_writes == 0 {
                                    let what = format!(
                                        "reading `{}` of union `{}`",
                                        member.text, self.structs[id.0].name
                                    );
                                    self.require_unsafe_claim(
                                        &what,
                                        member.span,
                                        "a union says nothing about which member was last written, so reading one is a claim the program makes inside `unsafe { ... }`",
                                    );
                                }
                                field_type
                            }
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
                                    fix: None,
                                };
                                if is_method {
                                    diagnostic.help =
                                        Some("call it with `()`; methods are not values".into());
                                } else if let Some(meant) = nearest(
                                    &member.text,
                                    self.structs[id.0]
                                        .fields
                                        .iter()
                                        .map(|field| field.name.as_str()),
                                ) {
                                    diagnostic.help = Some(format!("did you mean `{meant}`?"));
                                    diagnostic = diagnostic.with_fix(Fix::new(
                                        format!("change to `{meant}`"),
                                        member.span,
                                        meant,
                                    ));
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
                let (expected_elem, expected_fixed_size) = match expected {
                    Some(Type::Array(id)) => (Some(self.arrays[id.0].element), None),
                    Some(Type::FixedArray(id)) => (
                        Some(self.fixed_arrays[id.0].element),
                        Some(self.fixed_arrays[id.0].size),
                    ),
                    _ => (None, None),
                };
                if let Some(expected_size) = expected_fixed_size
                    && elements.len() != expected_size
                {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        expr.span,
                        format!(
                            "fixed array size mismatch: expected {} elements, found {}",
                            expected_size,
                            elements.len()
                        ),
                    );
                }
                if elements.is_empty() {
                    match expected_elem {
                        Some(elem) => {
                            if let Some(size) = expected_fixed_size {
                                self.fixed_array_type(elem, size)
                            } else {
                                self.array_type(elem)
                            }
                        }
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
                            Some(elem) => {
                                if let Some(size) = expected_fixed_size {
                                    self.fixed_array_type(elem, size)
                                } else {
                                    self.array_type(elem)
                                }
                            }
                            None => Type::Error,
                        }
                    }
                }
            }
            ExprKind::ArrayRepeat { element, count } => {
                let count_val = self.eval_constant_expr(count, Some(Type::Int(IntType::USize)));
                let size = match count_val {
                    Some(ConstValue::Int(v, _)) => {
                        if v <= 0 {
                            self.error(
                                DiagnosticCode::IntegerRange,
                                count.span,
                                "fixed array size must be greater than zero",
                            );
                            None
                        } else if v > (i32::MAX as i128) {
                            self.error(
                                DiagnosticCode::IntegerRange,
                                count.span,
                                "fixed array size is too large",
                            );
                            None
                        } else {
                            Some(v as usize)
                        }
                    }
                    Some(_) => {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            count.span,
                            "fixed array size must be an integer",
                        );
                        None
                    }
                    None => None,
                };
                let expected_elem = match expected {
                    Some(Type::FixedArray(id)) => Some(self.fixed_arrays[id.0].element),
                    Some(Type::Array(id)) => Some(self.arrays[id.0].element),
                    _ => None,
                };
                let previous_expected = self.expected_context;
                self.expected_context = expected_elem;
                let elem_ty = self.expression(element);
                self.expected_context = previous_expected;
                if let Some(expected_ty) = expected_elem {
                    self.expect_type(expected_ty, elem_ty, element.span);
                }
                if elem_ty == Type::Void {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        element.span,
                        "array element cannot be `void`",
                    );
                } else if matches!(elem_ty, Type::Function(_)) {
                    self.reject_stored_function(elem_ty, element.span, "an array element");
                }
                if let Some(sz) = size.filter(|_| elem_ty != Type::Error) {
                    if let Some(Type::FixedArray(id)) = expected {
                        let expected_sz = self.fixed_arrays[id.0].size;
                        if sz != expected_sz {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                expr.span,
                                format!(
                                    "fixed array size mismatch: expected {} elements, found {}",
                                    expected_sz, sz
                                ),
                            );
                        }
                    }
                    self.fixed_array_type(elem_ty, sz)
                } else {
                    Type::Error
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
                    Type::FixedArray(id) if usable => {
                        let elem = self.fixed_arrays[id.0].element;
                        self.array_type(elem)
                    }
                    Type::String | Type::Array(_) | Type::FixedArray(_) | Type::Error => {
                        Type::Error
                    }
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
                    Type::FixedArray(id) if usable => {
                        let fixed = self.fixed_arrays[id.0];
                        if let Some(idx) = self.const_int_value(index)
                            && (idx < 0 || idx >= fixed.size as i128)
                        {
                            self.error(
                                DiagnosticCode::IntegerRange,
                                index.span,
                                format!(
                                    "index `{idx}` out of bounds for array of length {}",
                                    fixed.size
                                ),
                            );
                        }
                        fixed.element
                    }
                    // Indexing a string reads one byte, not one character:
                    // Skuld strings are byte sequences and this milestone adds
                    // no code point type.
                    Type::String if usable => Type::Int(IntType::U8),
                    Type::Array(_) | Type::FixedArray(_) | Type::String | Type::Error => {
                        Type::Error
                    }
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
                        Type::Int(_)
                            | Type::Float
                            | Type::Bool
                            | Type::Char
                            | Type::String
                            | Type::Error
                    ) {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            value.span,
                            format!(
                                "cannot interpolate `{}`; only int, float, bool, char and string have a textual form",
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
        use BinaryOp::*;
        let valid = match op {
            // `+` also concatenates; the result is a new string.
            Add => left.is_numeric() || left == Type::String,
            Subtract | Multiply | Divide => left.is_numeric(),
            Less | Greater | LessEqual | GreaterEqual => left.is_numeric() || left == Type::Char,
            Modulo | BitAnd | BitOr | BitXor | ShiftLeft | ShiftRight => left.int_type().is_some(),
            And | Or => left == Type::Bool,
            Equal | NotEqual => {
                matches!(
                    left,
                    Type::Int(_) | Type::Float | Type::Bool | Type::String | Type::Char
                )
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
        if left != right && !self.expect_type(left, right, span) {
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
                "sort" | "to_sorted" => {
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
                } else if member.text == "to_sorted" {
                    Type::Array(id)
                } else {
                    Type::Void
                };
            }
        }
        let builtin = match (receiver, member.text.as_str()) {
            (Type::Array(_) | Type::FixedArray(_), "len") => Some(Type::INT),
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
            if is_field {
                self.error(
                    DiagnosticCode::NotCallable,
                    member.span,
                    format!("field `{}` is not callable", member.text),
                );
                return Type::Error;
            }
            let message = format!(
                "`{}` has no method `{}`",
                self.structs[id.0].name, member.text
            );
            let methods = self.structs[id.0]
                .methods
                .iter()
                .map(|method| method.name.clone())
                .collect();
            self.unknown_member(DiagnosticCode::NotCallable, member, message, methods);
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
            // `Protocol(6)`: the conversion back from the integer, which is
            // spelled the way `u8(...)` already spells a conversion. It traps
            // on a value no variant is worth, since the enum's values are
            // exactly the ones it declared.
            Some((id, SymbolKind::Enum)) => {
                let Some(enum_id) = self.enum_of_symbol(id) else {
                    return Type::Error;
                };
                let Some(kind) = self.enums[enum_id.0].underlying else {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::UnsupportedFeature,
                        span,
                        format!(
                            "`{}` is not numbered, so there is no integer to convert; declare it `enum {}: i32`",
                            self.enums[enum_id.0].name, self.enums[enum_id.0].name
                        ),
                    );
                    return Type::Error;
                };
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        format!(
                            "`{}` converts exactly one value",
                            self.enums[enum_id.0].name
                        ),
                    );
                    return Type::Error;
                }
                let previous = self.expected_context;
                self.expected_context = Some(Type::Int(kind));
                let found = self.expression(&arguments[0]);
                self.expected_context = previous;
                if found != Type::Error {
                    self.expect_type(Type::Int(kind), found, arguments[0].span);
                }
                Type::Enum(enum_id)
            }
            Some((_, SymbolKind::Builtin(builtin @ (Builtin::SizeOf | Builtin::OffsetOf)))) => {
                self.layout_builtin(builtin, arguments, span)
            }
            Some((
                _,
                SymbolKind::Builtin(
                    builtin @ (Builtin::Load
                    | Builtin::Store
                    | Builtin::VolatileLoad
                    | Builtin::VolatileStore
                    | Builtin::Offset
                    | Builtin::Addr
                    | Builtin::PtrFrom),
                ),
            )) => self.pointer_builtin(builtin, arguments, span, expected),
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
                    Type::FixedArray(id) => match self.fixed_arrays[id.0].element {
                        Type::Int(kind) => Some(Pointee::Int(kind)),
                        Type::Float => Some(Pointee::Float),
                        Type::Bool => Some(Pointee::Bool),
                        _ => None,
                    },
                    // An `extern struct` is bytes C already knows the shape
                    // of, so its address is what a foreign call takes. It is
                    // `*void`: there is no pointer to a struct type, and
                    // nothing in Skuld can read through it anyway.
                    Type::Struct(id)
                        if !self.structs[id.0].reference
                            && self.structs[id.0].layout.is_foreign() =>
                    {
                        Some(Pointee::Void)
                    }
                    // The address of a scalar local. It points into the frame
                    // it was taken in, which nothing tracks, so it is written
                    // inside `unsafe` like every other pointer operation.
                    scalar
                        if self.addressable_local(&arguments[0]).is_some()
                            && Pointee::of(scalar).is_some() =>
                    {
                        self.require_unsafe("ptr", span);
                        // A pointer can always write, so handing one out for an
                        // immutable binding would undo what `let` promises.
                        if self.addressable_local(&arguments[0]) == Some(Mutability::Immutable) {
                            self.error(
                                DiagnosticCode::ImmutableAssignment,
                                arguments[0].span,
                                "cannot take the address of an immutable binding, since a pointer can write through it",
                            );
                        }
                        Pointee::of(scalar)
                    }
                    _ => None,
                };
                match pointee {
                    Some(pointee) => Type::Pointer(pointee),
                    None => {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            arguments[0].span,
                            format!(
                                "`ptr` borrows the bytes of a string, an array of scalars or a scalar local, not `{}`",
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
                if found == Type::Error || !self.expect_type(bytes, found, arguments[0].span) {
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
                // An enum that names an integer type converts to it: what
                // travels is the variant's declared value, so the enum has to
                // be one whose values mean something.
                if let Type::Enum(id) = found {
                    if self.enums[id.0].underlying.is_none() {
                        self.error(
                            DiagnosticCode::TypeMismatch,
                            arguments[0].span,
                            format!(
                                "`{}` is not numbered, so it has no integer value; declare it `enum {}: {}`",
                                self.enums[id.0].name,
                                self.enums[id.0].name,
                                kind.name()
                            ),
                        );
                        return Type::Error;
                    }
                    return Type::Int(kind);
                }
                if found == Type::Error {
                    Type::Error
                } else if found.int_type().is_none() && found != Type::Float && found != Type::Char
                {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        arguments[0].span,
                        format!(
                            "`{}` converts an integer, float or char, found `{}`",
                            kind.name(),
                            self.type_name(found)
                        ),
                    );
                    Type::Error
                } else {
                    Type::Int(kind)
                }
            }
            Some((_, SymbolKind::Builtin(Builtin::FloatConvert))) => {
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        "`float` converts exactly one value".to_string(),
                    );
                    return Type::Error;
                }
                let found = self.expression(&arguments[0]);
                if found == Type::Error {
                    Type::Error
                } else if found.int_type().is_none() && found != Type::Float {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        arguments[0].span,
                        format!(
                            "`float` converts an integer or float, found `{}`",
                            self.type_name(found)
                        ),
                    );
                    Type::Error
                } else {
                    Type::Float
                }
            }
            Some((_, SymbolKind::Builtin(Builtin::CharConvert))) => {
                if arguments.len() != 1 {
                    for argument in arguments {
                        self.expression(argument);
                    }
                    self.error(
                        DiagnosticCode::ArgumentCount,
                        span,
                        "`char` converts exactly one value".to_string(),
                    );
                    return Type::Error;
                }
                let found = self.expression(&arguments[0]);
                if found == Type::Error {
                    Type::Error
                } else if found.int_type().is_none() && found != Type::Char {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        arguments[0].span,
                        format!(
                            "`char` converts an integer or char, found `{}`",
                            self.type_name(found)
                        ),
                    );
                    Type::Error
                } else {
                    Type::Char
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
                        Type::Int(_)
                            | Type::Float
                            | Type::Bool
                            | Type::Char
                            | Type::String
                            | Type::Error
                    ) {
                        self.error(
                            DiagnosticCode::InvalidValueType,
                            arg.span,
                            format!(
                                "`print` cannot print `{}`; only int, float, bool, char and string have a textual form",
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
    /// `load`, `store`, `offset`, `addr` and the rest.
    ///
    /// Every one of them either reads memory the compiler cannot vouch for or
    /// hands out an address that outlives what it points at, so every one of
    /// them is refused outside an `unsafe` block. That check lives here
    /// because it is the only meaning `unsafe` has: nothing about the
    /// generated code changes, and everything outside such a block keeps the
    /// rules it had before this milestone.
    fn pointer_builtin(
        &mut self,
        builtin: Builtin,
        arguments: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let name = match builtin {
            Builtin::Load => "load",
            Builtin::Store => "store",
            Builtin::VolatileLoad => "volatile_load",
            Builtin::VolatileStore => "volatile_store",
            Builtin::Offset => "offset",
            Builtin::Addr => "addr",
            Builtin::PtrFrom => "ptr_from",
            _ => unreachable!("internal compiler bug: not a pointer builtin"),
        };
        let wanted = match builtin {
            Builtin::Store | Builtin::VolatileStore | Builtin::Offset => 2,
            _ => 1,
        };
        if arguments.len() != wanted {
            let previous = self.expected_context;
            self.expected_context = None;
            for argument in arguments {
                self.expression(argument);
            }
            self.expected_context = previous;
            let plural = if wanted == 1 { "argument" } else { "arguments" };
            self.error(
                DiagnosticCode::ArgumentCount,
                span,
                format!("`{name}` expects exactly {wanted} {plural}"),
            );
            return Type::Error;
        }
        self.require_unsafe(name, span);
        // `ptr_from` takes an address rather than a pointer, so it is the one
        // that reads its type from the context instead of from its argument.
        if builtin == Builtin::PtrFrom {
            let previous = self.expected_context;
            self.expected_context = Some(Type::Int(IntType::USize));
            let found = self.expression(&arguments[0]);
            self.expected_context = previous;
            if found != Type::Error {
                self.expect_type(Type::Int(IntType::USize), found, arguments[0].span);
            }
            return match expected {
                Some(Type::Pointer(pointee)) => Type::Pointer(pointee),
                _ => {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        span,
                        "`ptr_from` needs the pointer type it becomes to be known here",
                    );
                    Type::Error
                }
            };
        }
        let previous = self.expected_context;
        self.expected_context = None;
        let pointer_type = self.expression(&arguments[0]);
        self.expected_context = previous;
        let pointee = match pointer_type {
            Type::Error => return Type::Error,
            Type::Pointer(pointee) => pointee,
            other => {
                self.error(
                    DiagnosticCode::TypeMismatch,
                    arguments[0].span,
                    format!(
                        "`{name}` expects a pointer, not `{}`",
                        self.type_name(other)
                    ),
                );
                for argument in &arguments[1..] {
                    self.expression(argument);
                }
                return Type::Error;
            }
        };
        if builtin == Builtin::Addr {
            return Type::Int(IntType::USize);
        }
        // `*void` points at no particular value, so there is nothing to read,
        // nothing to write, and no element to step over.
        let Some(value_type) = pointee.value_type() else {
            self.error(
                DiagnosticCode::InvalidValueType,
                arguments[0].span,
                format!("`{name}` cannot work through `*void`, which points at no particular type"),
            );
            for argument in &arguments[1..] {
                self.expression(argument);
            }
            return Type::Error;
        };
        match builtin {
            Builtin::Load | Builtin::VolatileLoad => value_type,
            Builtin::Offset => {
                let previous = self.expected_context;
                self.expected_context = Some(Type::INT);
                let count = self.expression(&arguments[1]);
                self.expected_context = previous;
                if count != Type::Error {
                    self.expect_type(Type::INT, count, arguments[1].span);
                }
                Type::Pointer(pointee)
            }
            Builtin::Store | Builtin::VolatileStore => {
                let previous = self.expected_context;
                self.expected_context = Some(value_type);
                let written = self.expression(&arguments[1]);
                self.expected_context = previous;
                if written != Type::Error {
                    self.expect_type(value_type, written, arguments[1].span);
                }
                Type::Void
            }
            _ => unreachable!("internal compiler bug: pointer builtin handled above"),
        }
    }

    /// Whether an expression names a local variable, which is the one thing
    /// whose address `ptr` hands out. A parameter is left out on purpose: its
    /// address is the address of a copy, which is never what a caller wants.
    fn addressable_local(&self, expr: &Expr) -> Option<Mutability> {
        match &expr.kind {
            ExprKind::Identifier(name) => {
                match self
                    .resolution
                    .references
                    .get(&(self.file, name.span.start))
                    .map(|id| self.resolution.symbols[id.0].kind)
                {
                    Some(SymbolKind::Variable(mutability)) => Some(mutability),
                    _ => None,
                }
            }
            ExprKind::Group(inner) => self.addressable_local(inner),
            _ => None,
        }
    }

    /// `size_of(Type)` and `offset_of(Type, field)`.
    ///
    /// Both answer a question about a layout, so both are refused for a type
    /// whose layout is the compiler's own and deliberately unspecified: only
    /// an `extern struct` has an answer a program is allowed to depend on.
    /// Neither argument is a value — one names a type and the other a field —
    /// which is why the resolver leaves them alone and they are read here from
    /// the syntax.
    fn layout_builtin(&mut self, builtin: Builtin, arguments: &[Expr], span: Span) -> Type {
        let (name, wanted) = match builtin {
            Builtin::SizeOf => ("size_of", 1),
            _ => ("offset_of", 2),
        };
        if arguments.len() != wanted {
            self.error(
                DiagnosticCode::ArgumentCount,
                span,
                match builtin {
                    Builtin::SizeOf => "`size_of` expects exactly one type".to_owned(),
                    _ => "`offset_of` expects a type and a field name".to_owned(),
                },
            );
            return Type::Error;
        }
        let Some(path) = self.type_path(&arguments[0]) else {
            self.error(
                DiagnosticCode::ExpectedSyntax,
                arguments[0].span,
                format!("`{name}` takes the name of a type, not an expression"),
            );
            return Type::Error;
        };
        let Some(TypeEntry::Struct(id)) = self.lookup_type(&path, "type") else {
            return Type::Error;
        };
        if self.structs[id.0].reference || !self.structs[id.0].layout.is_foreign() {
            self.error(
                DiagnosticCode::InvalidValueType,
                arguments[0].span,
                format!(
                    "`{name}` needs a declared layout; `{}` is laid out by the compiler, which makes no promise about where its fields sit",
                    self.structs[id.0].name
                ),
            );
            return Type::Error;
        }
        let field = match builtin {
            Builtin::SizeOf => None,
            _ => {
                let ExprKind::Identifier(written) = &arguments[1].kind else {
                    self.error(
                        DiagnosticCode::ExpectedSyntax,
                        arguments[1].span,
                        "`offset_of` takes a field name".to_owned(),
                    );
                    return Type::Error;
                };
                let Some((index, _)) = self.structs[id.0].field(&written.text) else {
                    let candidates = self.structs[id.0]
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
                        .collect();
                    let message = format!(
                        "`{}` has no field `{}`",
                        self.structs[id.0].name, written.text
                    );
                    self.unknown_member(DiagnosticCode::MissingField, written, message, candidates);
                    return Type::Error;
                };
                Some(index)
            }
        };
        self.layout_queries
            .insert((self.file, span.start, span.end), (id, field));
        Type::Int(IntType::USize)
    }
    /// The type name an argument spells: `Header` or `headers.Header`.
    fn type_path(&self, expr: &Expr) -> Option<Path> {
        match &expr.kind {
            ExprKind::Identifier(name) => Some(Path::bare(name.clone())),
            ExprKind::Member { object, member } => {
                let ExprKind::Identifier(qualifier) = &object.kind else {
                    return None;
                };
                self.resolution.module_in_file(self.file, &qualifier.text)?;
                Some(Path {
                    module: Some(qualifier.clone()),
                    name: member.clone(),
                    span: expr.span,
                })
            }
            _ => None,
        }
    }

    /// Refuse an operation that only an `unsafe` block allows, once, with the
    /// reason that operation is refused.
    fn require_unsafe_claim(&mut self, what: &str, span: Span, why: &str) -> bool {
        if self.unsafe_depth > 0 {
            return true;
        }
        let diagnostic = Diagnostic {
            code: DiagnosticCode::RequiresUnsafe,
            span,
            message: format!("{what} is only allowed inside an `unsafe` block"),
            help: Some(why.to_owned()),
            fix: None,
        };
        self.diagnostics.push(FileDiagnostic {
            file: self.file,
            diagnostic,
        });
        false
    }
    /// Refuse an operation that only an `unsafe` block allows, once.
    fn require_unsafe(&mut self, name: &str, span: Span) -> bool {
        if self.unsafe_depth > 0 {
            return true;
        }
        let mut diagnostic = Diagnostic {
            code: DiagnosticCode::RequiresUnsafe,
            span,
            message: format!("`{name}` is only allowed inside an `unsafe` block"),
            help: None,
            fix: None,
        };
        diagnostic.help = Some(
            "the compiler cannot check what a pointer points at, so reading or writing through one is written inside `unsafe { ... }`"
                .to_string(),
        );
        self.diagnostics.push(FileDiagnostic {
            file: self.file,
            diagnostic,
        });
        false
    }

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

    fn const_int_value(&self, expr: &Expr) -> Option<i128> {
        match &expr.kind {
            ExprKind::Literal(Literal::Integer(v)) => Some(*v as i128),
            ExprKind::Unary {
                op: UnaryOp::Negative,
                operand,
                ..
            } => {
                if let ExprKind::Literal(Literal::Integer(v)) = &operand.kind {
                    Some(-(*v as i128))
                } else {
                    None
                }
            }
            ExprKind::Identifier(name) => {
                if let Some(&sym_id) = self
                    .resolution
                    .references
                    .get(&(self.file, name.span.start))
                    && let Some(ConstValue::Int(v, _)) = self.constants.get(&sym_id)
                {
                    return Some(*v);
                }
                None
            }
            _ => None,
        }
    }

    fn ensure_constant_evaluated(&mut self, id: SymbolId) -> Option<ConstValue> {
        if let Some(val) = self.constants.get(&id) {
            return Some(val.clone());
        }
        if self.evaluating_constants.contains(&id) {
            let sym_name = &self.resolution.symbols[id.0].name;
            let sym_span = self.resolution.symbols[id.0]
                .span
                .unwrap_or(Span::new(0, 0));
            self.error(
                DiagnosticCode::UnsupportedFeature,
                sym_span,
                format!("circular constant definition involving `{sym_name}`"),
            );
            return None;
        }
        let (decl_file, decl) = self.constant_decls.get(&id).cloned()?;
        let prev_file = self.file;
        let prev_module = self.module;
        self.file = decl_file;
        self.module = self.resolution.symbols[id.0].module.unwrap_or(self.module);
        let res = self.check_constant_decl(&decl, id);
        self.file = prev_file;
        self.module = prev_module;
        res
    }

    fn check_constant_decl(&mut self, decl: &ConstantDecl, id: SymbolId) -> Option<ConstValue> {
        self.evaluating_constants.push(id);
        let annotated_ty = decl.type_ref.as_ref().map(|tr| self.type_ref(tr, false));
        let val = self.eval_constant_expr(&decl.value, annotated_ty);
        self.evaluating_constants.pop();

        let val = match val {
            Some(v) => v,
            None => {
                self.symbol_types[id.0] = Type::Error;
                return None;
            }
        };

        let val_ty = val.ty();
        let final_val = if let Some(expected) = annotated_ty {
            if expected != val_ty && expected != Type::Error && val_ty != Type::Error {
                if let (Type::Int(exp_it), ConstValue::Int(v, _)) = (expected, &val) {
                    if int_type_fits(*v, exp_it) {
                        ConstValue::Int(*v, exp_it)
                    } else {
                        self.error(
                            DiagnosticCode::IntegerRange,
                            decl.value.span,
                            format!("constant value `{v}` does not fit type `{expected}`"),
                        );
                        self.symbol_types[id.0] = Type::Error;
                        return None;
                    }
                } else {
                    self.error(
                        DiagnosticCode::TypeMismatch,
                        decl.value.span,
                        format!("constant expects `{expected}`, found `{val_ty}`"),
                    );
                    self.symbol_types[id.0] = Type::Error;
                    return None;
                }
            } else {
                val
            }
        } else {
            val
        };

        let final_ty = final_val.ty();
        match final_ty {
            Type::Int(_) | Type::Float | Type::Bool | Type::Char | Type::String => {}
            Type::Error => {}
            other => {
                self.error(
                    DiagnosticCode::InvalidValueType,
                    decl.span,
                    format!("type `{other}` is not allowed for a constant; constants must be scalars or strings"),
                );
                self.symbol_types[id.0] = Type::Error;
                return None;
            }
        }

        self.constants.insert(id, final_val.clone());
        self.symbol_types[id.0] = final_ty;
        self.record(&decl.value, final_ty);
        Some(final_val)
    }

    fn eval_constant_expr(&mut self, expr: &Expr, expected: Option<Type>) -> Option<ConstValue> {
        match &expr.kind {
            ExprKind::Literal(literal) => match literal {
                Literal::Integer(value) => {
                    let it = expected.and_then(Type::int_type).unwrap_or(IntType::I64);
                    let val = *value as i128;
                    if !int_type_fits(val, it) {
                        self.error(
                            DiagnosticCode::IntegerRange,
                            expr.span,
                            format!("integer literal `{val}` out of range for `{it}`"),
                        );
                        return None;
                    }
                    self.record(expr, Type::Int(it));
                    Some(ConstValue::Int(val, it))
                }
                Literal::Float(value) => {
                    self.record(expr, Type::Float);
                    Some(ConstValue::Float(*value))
                }
                Literal::Boolean(value) => {
                    self.record(expr, Type::Bool);
                    Some(ConstValue::Bool(*value))
                }
                Literal::String(value) => {
                    self.record(expr, Type::String);
                    Some(ConstValue::String(value.clone()))
                }
                Literal::Char(c) => {
                    self.record(expr, Type::Char);
                    Some(ConstValue::Char(*c))
                }
            },
            ExprKind::Group(inner) => {
                let val = self.eval_constant_expr(inner, expected)?;
                self.record(expr, val.ty());
                Some(val)
            }
            ExprKind::Unary {
                op,
                operand,
                op_span,
            } => {
                let width = expected.and_then(Type::int_type).unwrap_or(IntType::I64);
                if *op == UnaryOp::Negative && width.signed() {
                    let stripped = strip_groups_ref(operand);
                    if let ExprKind::Literal(Literal::Integer(mag)) = &stripped.kind {
                        let neg = -(*mag as i128);
                        if int_type_fits(neg, width) {
                            self.record(operand, Type::Int(width));
                            self.record(expr, Type::Int(width));
                            return Some(ConstValue::Int(neg, width));
                        }
                    }
                }
                let op_val = self.eval_constant_expr(operand, expected)?;
                match op {
                    UnaryOp::Negative => match op_val {
                        ConstValue::Int(v, it) => {
                            if !it.signed() {
                                self.error(
                                    DiagnosticCode::InvalidOperator,
                                    *op_span,
                                    "cannot negate unsigned integer type",
                                );
                                return None;
                            }
                            if v == int_type_min(it) {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    expr.span,
                                    "constant negation overflow",
                                );
                                return None;
                            }
                            let res = ConstValue::Int(-v, it);
                            self.record(expr, res.ty());
                            Some(res)
                        }
                        ConstValue::Float(f) => {
                            let res = ConstValue::Float(-f);
                            self.record(expr, res.ty());
                            Some(res)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::InvalidOperator,
                                *op_span,
                                format!("unary `-` does not accept `{}`", op_val.ty()),
                            );
                            None
                        }
                    },
                    UnaryOp::Not => match op_val {
                        ConstValue::Bool(b) => {
                            let res = ConstValue::Bool(!b);
                            self.record(expr, res.ty());
                            Some(res)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::InvalidOperator,
                                *op_span,
                                format!("unary `!` does not accept `{}`", op_val.ty()),
                            );
                            None
                        }
                    },
                    UnaryOp::BitNot => match op_val {
                        ConstValue::Int(v, it) => {
                            let mask = if it.bits() == 64 {
                                u64::MAX as u128
                            } else {
                                (1_u128 << it.bits()) - 1
                            };
                            let raw = (!(v as u128)) & mask;
                            let res_val = if it.signed() {
                                sign_extend(raw, it)
                            } else {
                                raw as i128
                            };
                            let res = ConstValue::Int(res_val, it);
                            self.record(expr, res.ty());
                            Some(res)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::InvalidOperator,
                                *op_span,
                                format!("unary `~` does not accept `{}`", op_val.ty()),
                            );
                            None
                        }
                    },
                    UnaryOp::Positive => match op_val {
                        ConstValue::Int(..) | ConstValue::Float(..) => {
                            self.record(expr, op_val.ty());
                            Some(op_val)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::InvalidOperator,
                                *op_span,
                                format!("unary `+` does not accept `{}`", op_val.ty()),
                            );
                            None
                        }
                    },
                }
            }
            ExprKind::Binary {
                left,
                op,
                right,
                op_span,
            } => {
                let left_val = self.eval_constant_expr(left, expected)?;
                let right_expected = match op {
                    BinaryOp::ShiftLeft | BinaryOp::ShiftRight => None,
                    _ => Some(left_val.ty()),
                };
                let right_val = self.eval_constant_expr(right, right_expected)?;
                match op {
                    BinaryOp::Add => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b)) => {
                            if it_a != it_b {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    format!("mismatched integer widths `{it_a}` and `{it_b}`"),
                                );
                                return None;
                            }
                            let res = a.checked_add(*b);
                            if res.is_none() || !int_type_fits(res.unwrap(), *it_a) {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    *op_span,
                                    "constant arithmetic overflow",
                                );
                                return None;
                            }
                            let v = ConstValue::Int(res.unwrap(), *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        (ConstValue::Float(a), ConstValue::Float(b)) => {
                            let v = ConstValue::Float(a + b);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        (ConstValue::String(a), ConstValue::String(b)) => {
                            let v = ConstValue::String(format!("{a}{b}"));
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                format!(
                                    "operator `+` does not accept `{}` and `{}`",
                                    left_val.ty(),
                                    right_val.ty()
                                ),
                            );
                            None
                        }
                    },
                    BinaryOp::Subtract => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b)) => {
                            if it_a != it_b {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    format!("mismatched integer widths `{it_a}` and `{it_b}`"),
                                );
                                return None;
                            }
                            let res = a.checked_sub(*b);
                            if res.is_none() || !int_type_fits(res.unwrap(), *it_a) {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    *op_span,
                                    "constant arithmetic overflow",
                                );
                                return None;
                            }
                            let v = ConstValue::Int(res.unwrap(), *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        (ConstValue::Float(a), ConstValue::Float(b)) => {
                            let v = ConstValue::Float(a - b);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                format!(
                                    "operator `-` does not accept `{}` and `{}`",
                                    left_val.ty(),
                                    right_val.ty()
                                ),
                            );
                            None
                        }
                    },
                    BinaryOp::Multiply => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b)) => {
                            if it_a != it_b {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    format!("mismatched integer widths `{it_a}` and `{it_b}`"),
                                );
                                return None;
                            }
                            let res = a.checked_mul(*b);
                            if res.is_none() || !int_type_fits(res.unwrap(), *it_a) {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    *op_span,
                                    "constant arithmetic overflow",
                                );
                                return None;
                            }
                            let v = ConstValue::Int(res.unwrap(), *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        (ConstValue::Float(a), ConstValue::Float(b)) => {
                            let v = ConstValue::Float(a * b);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                format!(
                                    "operator `*` does not accept `{}` and `{}`",
                                    left_val.ty(),
                                    right_val.ty()
                                ),
                            );
                            None
                        }
                    },
                    BinaryOp::Divide => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b)) => {
                            if it_a != it_b {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    format!("mismatched integer widths `{it_a}` and `{it_b}`"),
                                );
                                return None;
                            }
                            if *b == 0 {
                                self.error(
                                    DiagnosticCode::InvalidOperator,
                                    *op_span,
                                    "division by zero in constant expression",
                                );
                                return None;
                            }
                            if it_a.signed() && *a == int_type_min(*it_a) && *b == -1 {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    *op_span,
                                    "constant division overflow",
                                );
                                return None;
                            }
                            let v = ConstValue::Int(a / b, *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        (ConstValue::Float(a), ConstValue::Float(b)) => {
                            let v = ConstValue::Float(a / b);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                format!(
                                    "operator `/` does not accept `{}` and `{}`",
                                    left_val.ty(),
                                    right_val.ty()
                                ),
                            );
                            None
                        }
                    },
                    BinaryOp::Modulo => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b)) => {
                            if it_a != it_b {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    format!("mismatched integer widths `{it_a}` and `{it_b}`"),
                                );
                                return None;
                            }
                            if *b == 0 {
                                self.error(
                                    DiagnosticCode::InvalidOperator,
                                    *op_span,
                                    "modulo by zero in constant expression",
                                );
                                return None;
                            }
                            let v = ConstValue::Int(a % b, *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                format!(
                                    "operator `%` does not accept `{}` and `{}`",
                                    left_val.ty(),
                                    right_val.ty()
                                ),
                            );
                            None
                        }
                    },
                    BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                        match (&left_val, &right_val) {
                            (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b)) => {
                                if it_a != it_b {
                                    self.error(
                                        DiagnosticCode::TypeMismatch,
                                        *op_span,
                                        format!("mismatched integer widths `{it_a}` and `{it_b}`"),
                                    );
                                    return None;
                                }
                                let res = match op {
                                    BinaryOp::BitAnd => a & b,
                                    BinaryOp::BitOr => a | b,
                                    BinaryOp::BitXor => a ^ b,
                                    _ => unreachable!(),
                                };
                                let v = ConstValue::Int(res, *it_a);
                                self.record(expr, v.ty());
                                Some(v)
                            }
                            _ => {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    "bitwise operators require integer operands",
                                );
                                None
                            }
                        }
                    }
                    BinaryOp::ShiftLeft => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, _)) => {
                            if *b < 0 || *b >= it_a.bits() as i128 {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    *op_span,
                                    format!("shift count `{b}` out of range for `{it_a}`"),
                                );
                                return None;
                            }
                            let mask = if it_a.bits() == 64 {
                                u64::MAX as u128
                            } else {
                                (1_u128 << it_a.bits()) - 1
                            };
                            let shifted = ((*a as u128) << (*b as u32)) & mask;
                            let res = if it_a.signed() {
                                sign_extend(shifted, *it_a)
                            } else {
                                shifted as i128
                            };
                            let v = ConstValue::Int(res, *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                "shift operators require integer operands",
                            );
                            None
                        }
                    },
                    BinaryOp::ShiftRight => match (&left_val, &right_val) {
                        (ConstValue::Int(a, it_a), ConstValue::Int(b, _)) => {
                            if *b < 0 || *b >= it_a.bits() as i128 {
                                self.error(
                                    DiagnosticCode::IntegerRange,
                                    *op_span,
                                    format!("shift count `{b}` out of range for `{it_a}`"),
                                );
                                return None;
                            }
                            let res = if it_a.signed() {
                                a >> (*b as u32)
                            } else {
                                ((*a as u128) >> (*b as u32)) as i128
                            };
                            let v = ConstValue::Int(res, *it_a);
                            self.record(expr, v.ty());
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                "shift operators require integer operands",
                            );
                            None
                        }
                    },
                    BinaryOp::Equal
                    | BinaryOp::NotEqual
                    | BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual => {
                        let is_cmp = match (&left_val, &right_val) {
                            (ConstValue::Int(a, it_a), ConstValue::Int(b, it_b))
                                if it_a == it_b =>
                            {
                                match op {
                                    BinaryOp::Equal => a == b,
                                    BinaryOp::NotEqual => a != b,
                                    BinaryOp::Less => a < b,
                                    BinaryOp::LessEqual => a <= b,
                                    BinaryOp::Greater => a > b,
                                    BinaryOp::GreaterEqual => a >= b,
                                    _ => unreachable!(),
                                }
                            }
                            (ConstValue::Float(a), ConstValue::Float(b)) => match op {
                                BinaryOp::Equal => a == b,
                                BinaryOp::NotEqual => a != b,
                                BinaryOp::Less => a < b,
                                BinaryOp::LessEqual => a <= b,
                                BinaryOp::Greater => a > b,
                                BinaryOp::GreaterEqual => a >= b,
                                _ => unreachable!(),
                            },
                            (ConstValue::Bool(a), ConstValue::Bool(b)) => match op {
                                BinaryOp::Equal => a == b,
                                BinaryOp::NotEqual => a != b,
                                _ => {
                                    self.error(
                                        DiagnosticCode::InvalidOperator,
                                        *op_span,
                                        "ordered comparisons are not supported on booleans",
                                    );
                                    return None;
                                }
                            },
                            (ConstValue::Char(a), ConstValue::Char(b)) => match op {
                                BinaryOp::Equal => a == b,
                                BinaryOp::NotEqual => a != b,
                                BinaryOp::Less => a < b,
                                BinaryOp::LessEqual => a <= b,
                                BinaryOp::Greater => a > b,
                                BinaryOp::GreaterEqual => a >= b,
                                _ => unreachable!(),
                            },
                            (ConstValue::String(a), ConstValue::String(b)) => match op {
                                BinaryOp::Equal => a == b,
                                BinaryOp::NotEqual => a != b,
                                _ => {
                                    self.error(
                                        DiagnosticCode::InvalidOperator,
                                        *op_span,
                                        "ordered comparisons are not supported on strings",
                                    );
                                    return None;
                                }
                            },
                            _ => {
                                self.error(
                                    DiagnosticCode::TypeMismatch,
                                    *op_span,
                                    format!(
                                        "cannot compare `{}` with `{}`",
                                        left_val.ty(),
                                        right_val.ty()
                                    ),
                                );
                                return None;
                            }
                        };
                        let v = ConstValue::Bool(is_cmp);
                        self.record(expr, Type::Bool);
                        Some(v)
                    }
                    BinaryOp::And | BinaryOp::Or => match (&left_val, &right_val) {
                        (ConstValue::Bool(a), ConstValue::Bool(b)) => {
                            let res = match op {
                                BinaryOp::And => *a && *b,
                                BinaryOp::Or => *a || *b,
                                _ => unreachable!(),
                            };
                            let v = ConstValue::Bool(res);
                            self.record(expr, Type::Bool);
                            Some(v)
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::TypeMismatch,
                                *op_span,
                                "logical operators require boolean operands",
                            );
                            None
                        }
                    },
                }
            }
            ExprKind::Identifier(name) => {
                let id = self.reference(name);
                if matches!(self.resolution.symbols[id.0].kind, SymbolKind::Constant) {
                    let val = self.ensure_constant_evaluated(id)?;
                    self.record(expr, val.ty());
                    Some(val)
                } else {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        expr.span,
                        format!("`{}` is not a constant", name.text),
                    );
                    None
                }
            }
            ExprKind::Member { object, member } => {
                if matches!(&object.kind, ExprKind::Identifier(qualifier) if self.resolution.module_in_file(self.file, &qualifier.text).is_some())
                    && let Some(&sym_id) = self
                        .resolution
                        .references
                        .get(&(self.file, member.span.start))
                    && matches!(self.resolution.symbols[sym_id.0].kind, SymbolKind::Constant)
                {
                    let val = self.ensure_constant_evaluated(sym_id)?;
                    self.record(expr, val.ty());
                    Some(val)
                } else {
                    self.error(
                        DiagnosticCode::InvalidValueType,
                        expr.span,
                        "member access is not valid in a constant expression",
                    );
                    None
                }
            }
            ExprKind::Call { callee, arguments } => {
                let id = match &callee.kind {
                    ExprKind::Identifier(name) => Some(self.reference(name)),
                    ExprKind::Member { member, .. } => Some(self.reference(member)),
                    _ => None,
                };
                if let Some(sym_id) = id {
                    match self.resolution.symbols[sym_id.0].kind {
                        SymbolKind::Builtin(Builtin::IntConvert(target_it)) => {
                            if arguments.len() != 1 {
                                self.error(
                                    DiagnosticCode::ArgumentCount,
                                    expr.span,
                                    format!("conversion `{}` expects 1 argument", target_it.name()),
                                );
                                return None;
                            }
                            let arg_val = self.eval_constant_expr(&arguments[0], None)?;
                            match arg_val {
                                ConstValue::Int(val, _) => {
                                    if !int_type_fits(val, target_it) {
                                        self.error(
                                            DiagnosticCode::IntegerRange,
                                            expr.span,
                                            format!(
                                                "constant value `{val}` does not fit target type `{}`",
                                                target_it.name()
                                            ),
                                        );
                                        return None;
                                    }
                                    let v = ConstValue::Int(val, target_it);
                                    self.record(expr, Type::Int(target_it));
                                    Some(v)
                                }
                                ConstValue::Float(f) => {
                                    if f.is_nan() {
                                        self.error(
                                            DiagnosticCode::IntegerRange,
                                            expr.span,
                                            "constant float value is NaN",
                                        );
                                        return None;
                                    }
                                    let (min_f, max_f) = match target_it {
                                        IntType::I8 => (-129.0, 128.0),
                                        IntType::I16 => (-32769.0, 32768.0),
                                        IntType::I32 => (-2147483649.0, 2147483648.0),
                                        IntType::I64 | IntType::ISize => {
                                            (-9223372036854775808.0, 9223372036854775808.0)
                                        }
                                        IntType::U8 => (-1.0, 256.0),
                                        IntType::U16 => (-1.0, 65536.0),
                                        IntType::U32 => (-1.0, 4294967296.0),
                                        IntType::U64 | IntType::USize => {
                                            (-1.0, 18446744073709551616.0)
                                        }
                                    };
                                    let out_of_bounds = match target_it {
                                        IntType::I64 | IntType::ISize => f < min_f || f >= max_f,
                                        _ => f <= min_f || f >= max_f,
                                    };
                                    if out_of_bounds {
                                        self.error(
                                            DiagnosticCode::IntegerRange,
                                            expr.span,
                                            format!(
                                                "constant float value `{f}` does not fit target type `{}`",
                                                target_it.name()
                                            ),
                                        );
                                        return None;
                                    }
                                    let val = f.trunc() as i128;
                                    if !int_type_fits(val, target_it) {
                                        self.error(
                                            DiagnosticCode::IntegerRange,
                                            expr.span,
                                            format!(
                                                "constant float value `{f}` does not fit target type `{}`",
                                                target_it.name()
                                            ),
                                        );
                                        return None;
                                    }
                                    let v = ConstValue::Int(val, target_it);
                                    self.record(expr, Type::Int(target_it));
                                    Some(v)
                                }
                                ConstValue::Char(c) => {
                                    let val = (c as u32) as i128;
                                    if !int_type_fits(val, target_it) {
                                        self.error(
                                            DiagnosticCode::IntegerRange,
                                            expr.span,
                                            format!(
                                                "constant char code point `{val}` does not fit target type `{}`",
                                                target_it.name()
                                            ),
                                        );
                                        return None;
                                    }
                                    let v = ConstValue::Int(val, target_it);
                                    self.record(expr, Type::Int(target_it));
                                    Some(v)
                                }
                                _ => {
                                    self.error(
                                        DiagnosticCode::TypeMismatch,
                                        arguments[0].span,
                                        format!(
                                            "conversion `{}` expects integer, float, or char argument, found `{}`",
                                            target_it.name(),
                                            arg_val.ty()
                                        ),
                                    );
                                    None
                                }
                            }
                        }
                        SymbolKind::Builtin(Builtin::FloatConvert) => {
                            if arguments.len() != 1 {
                                self.error(
                                    DiagnosticCode::ArgumentCount,
                                    expr.span,
                                    "conversion `float` expects 1 argument",
                                );
                                return None;
                            }
                            let arg_val = self.eval_constant_expr(&arguments[0], None)?;
                            match arg_val {
                                ConstValue::Int(val, _) => {
                                    let v = ConstValue::Float(val as f64);
                                    self.record(expr, Type::Float);
                                    Some(v)
                                }
                                ConstValue::Float(f) => {
                                    let v = ConstValue::Float(f);
                                    self.record(expr, Type::Float);
                                    Some(v)
                                }
                                _ => {
                                    self.error(
                                        DiagnosticCode::TypeMismatch,
                                        arguments[0].span,
                                        format!(
                                            "conversion `float` expects integer or float argument, found `{}`",
                                            arg_val.ty()
                                        ),
                                    );
                                    None
                                }
                            }
                        }
                        SymbolKind::Builtin(Builtin::CharConvert) => {
                            if arguments.len() != 1 {
                                self.error(
                                    DiagnosticCode::ArgumentCount,
                                    expr.span,
                                    "conversion `char` expects 1 argument",
                                );
                                return None;
                            }
                            let arg_val = self.eval_constant_expr(&arguments[0], None)?;
                            match arg_val {
                                ConstValue::Int(val, _) => {
                                    if !(0..=0x10FFFF).contains(&val)
                                        || (0xD800..=0xDFFF).contains(&val)
                                    {
                                        self.error(
                                            DiagnosticCode::IntegerRange,
                                            expr.span,
                                            format!(
                                                "constant value `{val}` is not a valid Unicode scalar value"
                                            ),
                                        );
                                        return None;
                                    }
                                    let c = char::from_u32(val as u32).expect("valid unicode");
                                    let v = ConstValue::Char(c);
                                    self.record(expr, Type::Char);
                                    Some(v)
                                }
                                ConstValue::Char(c) => {
                                    let v = ConstValue::Char(c);
                                    self.record(expr, Type::Char);
                                    Some(v)
                                }
                                _ => {
                                    self.error(
                                        DiagnosticCode::TypeMismatch,
                                        arguments[0].span,
                                        format!(
                                            "conversion `char` expects integer or char argument, found `{}`",
                                            arg_val.ty()
                                        ),
                                    );
                                    None
                                }
                            }
                        }
                        _ => {
                            self.error(
                                DiagnosticCode::UnsupportedFeature,
                                expr.span,
                                "function calls are not supported in constant expressions",
                            );
                            None
                        }
                    }
                } else {
                    self.error(
                        DiagnosticCode::UnsupportedFeature,
                        expr.span,
                        "function calls are not supported in constant expressions",
                    );
                    None
                }
            }
            _ => {
                self.error(
                    DiagnosticCode::UnsupportedFeature,
                    expr.span,
                    "expression is not valid in a constant definition",
                );
                None
            }
        }
    }
}

/// Whether an integer value is representable in a width. Both ends are read
/// from the width itself, so a new one needs nothing here.
fn fits_int_type(value: i128, kind: IntType) -> bool {
    let low = -(kind.min_magnitude() as i128);
    let high = kind.max_magnitude() as i128;
    value >= low && value <= high
}

/// A `Result` seen as the two-variant enum it behaves like. `Ok` is variant 0
/// and `Err` variant 1, the same order the backend gives its tag.
fn result_as_enum(info: ResultInfo) -> EnumInfo {
    EnumInfo {
        name: "Result".into(),
        // A builtin belongs to no module and is visible everywhere.
        module: ROOT,
        visibility: Visibility::Public,
        underlying: None,
        variants: vec![
            VariantInfo {
                name: "Ok".into(),
                payload: Some(info.ok),
                value: ResultInfo::OK as i128,
            },
            VariantInfo {
                name: "Err".into(),
                payload: Some(info.err),
                value: ResultInfo::ERR as i128,
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
