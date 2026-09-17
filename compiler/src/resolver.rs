//! Lexical value-name resolution over an unchanged parser AST.
//!
//! Scopes nest prelude → module → file → bodies. A module scope holds every
//! declaration of every file in that module, public or not; a file scope holds
//! only that file's import qualifiers, so two files of one module can import
//! different things without seeing each other's imports.
use crate::{
    ast::*,
    diagnostic::{Diagnostic, DiagnosticCode, Fix, nearest},
    module::{FileDiagnostic, FileId, LoadedProgram, ModuleId},
    span::Span,
    types::IntType,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymbolId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Print,
    Some,
    None,
    Ok,
    Err,
    /// `u8(value)` and friends. The width travels with the symbol so the
    /// checker never has to read the identifier's spelling back.
    IntConvert(IntType),
    /// Explicit conversion to `float`.
    FloatConvert,
    /// Explicit conversion to `char`.
    CharConvert,
    /// `bytes_to_string(bytes)`, which validates UTF-8 and can fail.
    BytesToString,
    /// `ptr(value)`, which borrows the bytes of a string or `[]u8` as a raw
    /// pointer for the foreign boundary. It keeps nothing alive.
    Ptr,
    /// `load(pointer)`, which reads the value a pointer points at. Its type is
    /// the pointer's own pointee, so nothing spells it at the call.
    Load,
    /// `store(pointer, value)`, which writes through a pointer.
    Store,
    /// `volatile_load(pointer)`: a read the backend may neither elide nor
    /// reorder against other volatile accesses.
    VolatileLoad,
    /// `volatile_store(pointer, value)`: the same for a write.
    VolatileStore,
    /// `offset(pointer, count)`, which moves a pointer in element units.
    Offset,
    /// `addr(pointer)`, which reads a pointer as a `usize` address.
    Addr,
    /// `ptr_from(address)`, which turns a `usize` back into a pointer. The
    /// type it becomes comes from the context that receives it.
    PtrFrom,
    /// `size_of(Type)`: the size in bytes of a type whose layout is declared.
    SizeOf,
    /// `offset_of(Type, field)`: where a field sits inside one.
    OffsetOf,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Builtin(Builtin),
    Function,
    Parameter,
    Variable(Mutability),
    Constant,
    Enum,
    /// An import qualifier. It names a module, never a value, so it is only
    /// ever the left half of a qualified name.
    Module(ModuleId),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Builtins have no source declaration.
    pub span: Option<Span>,
    pub scope: ScopeId,
    /// Whether another module may name this symbol. Everything below module
    /// level is private by construction, since no other module can reach it.
    pub visibility: Visibility,
    /// The module that declares this symbol, absent for the prelude.
    pub module: Option<ModuleId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub span: Option<Span>,
    pub symbols: BTreeMap<String, SymbolId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    /// Keys are a file and a declaration-name byte start in that file's
    /// unchanged AST. Offsets repeat across files, so the file is part of the
    /// key rather than a property of the span.
    pub declarations: BTreeMap<(FileId, usize), SymbolId>,
    /// Keys are identifier-use positions. Member labels are not value names,
    /// except the right half of a module-qualified name, which is one.
    pub references: BTreeMap<(FileId, usize), SymbolId>,
    /// The import scope of each file, by `FileId`.
    pub file_scopes: Vec<ScopeId>,
    /// The declaration scope of each module, by `ModuleId`.
    pub module_scopes: Vec<ScopeId>,
    /// What each lambda captures, keyed by its file and body-brace offset, in
    /// declaration order. A capture is a name the body used and did not bind.
    pub captures: BTreeMap<(FileId, usize), Vec<SymbolId>>,
}

impl Resolution {
    /// The module an import qualifier names in one file. Type names live in
    /// the checker's own namespace, so it resolves its qualifiers through
    /// this rather than through the value scopes.
    pub fn module_in_file(&self, file: FileId, qualifier: &str) -> Option<ModuleId> {
        let scope = *self.file_scopes.get(file.0)?;
        let symbol = self.scopes[scope.0].symbols.get(qualifier)?;
        match self.symbols[symbol.0].kind {
            SymbolKind::Module(id) => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ResolveOutput {
    pub resolution: Option<Resolution>,
    pub diagnostics: Vec<FileDiagnostic>,
}

/// Resolve a whole program. Tables belong only to this revision of these
/// sources; no type names, member labels, call signatures or entrypoints are
/// checked here.
pub fn resolve(program: &LoadedProgram) -> ResolveOutput {
    let mut resolver = Resolver {
        result: Resolution {
            symbols: Vec::new(),
            scopes: Vec::new(),
            declarations: BTreeMap::new(),
            references: BTreeMap::new(),
            file_scopes: vec![ScopeId(0); program.files.len()],
            module_scopes: Vec::new(),
            captures: BTreeMap::new(),
        },
        lambdas: Vec::new(),
        in_field_default: false,
        diagnostics: Vec::new(),
        current: ScopeId(0),
        file: FileId(0),
        module: None,
    };
    resolver.result.scopes.push(Scope {
        parent: None,
        span: None,
        symbols: BTreeMap::new(),
    });
    resolver.insert("print", SymbolKind::Builtin(Builtin::Print), None);
    resolver.insert("Some", SymbolKind::Builtin(Builtin::Some), None);
    resolver.insert("None", SymbolKind::Builtin(Builtin::None), None);
    resolver.insert("null", SymbolKind::Builtin(Builtin::None), None);
    resolver.insert("Ok", SymbolKind::Builtin(Builtin::Ok), None);
    resolver.insert("Err", SymbolKind::Builtin(Builtin::Err), None);
    resolver.insert(
        "bytes_to_string",
        SymbolKind::Builtin(Builtin::BytesToString),
        None,
    );
    resolver.insert("ptr", SymbolKind::Builtin(Builtin::Ptr), None);
    // The pointer builtins. They are prelude bindings like `print` and `u8()`,
    // shadowable like them, and the checker refuses them outside `unsafe`.
    resolver.insert("load", SymbolKind::Builtin(Builtin::Load), None);
    resolver.insert("store", SymbolKind::Builtin(Builtin::Store), None);
    resolver.insert(
        "volatile_load",
        SymbolKind::Builtin(Builtin::VolatileLoad),
        None,
    );
    resolver.insert(
        "volatile_store",
        SymbolKind::Builtin(Builtin::VolatileStore),
        None,
    );
    resolver.insert("offset", SymbolKind::Builtin(Builtin::Offset), None);
    resolver.insert("addr", SymbolKind::Builtin(Builtin::Addr), None);
    resolver.insert("ptr_from", SymbolKind::Builtin(Builtin::PtrFrom), None);
    resolver.insert("size_of", SymbolKind::Builtin(Builtin::SizeOf), None);
    resolver.insert("offset_of", SymbolKind::Builtin(Builtin::OffsetOf), None);
    // `int` and `i64` name one type, so both spellings convert to it.
    for kind in IntType::ALL {
        resolver.insert(
            kind.name(),
            SymbolKind::Builtin(Builtin::IntConvert(kind)),
            None,
        );
        resolver.insert(
            kind.suffix(),
            SymbolKind::Builtin(Builtin::IntConvert(kind)),
            None,
        );
    }
    resolver.insert("float", SymbolKind::Builtin(Builtin::FloatConvert), None);
    resolver.insert("char", SymbolKind::Builtin(Builtin::CharConvert), None);
    let prelude = resolver.current;

    // One scope per module, holding the declarations of all its files. They
    // are created first so that a module can be imported before it is walked.
    for _ in &program.modules {
        resolver.current = prelude;
        resolver.enter_scope(None);
        resolver.result.module_scopes.push(resolver.current);
    }
    for (index, module) in program.modules.iter().enumerate() {
        let id = ModuleId(index);
        resolver.module = Some(id);
        resolver.current = resolver.result.module_scopes[index];
        for file in &module.files {
            resolver.file = *file;
            let syntax = &program.files[file.0].program;
            for declaration in &syntax.enums {
                resolver.declare(&declaration.name, SymbolKind::Enum, declaration.visibility);
            }
            for declaration in &syntax.constants {
                resolver.declare(
                    &declaration.name,
                    SymbolKind::Constant,
                    declaration.visibility,
                );
            }
            // Foreign functions are ordinary value names: only the backend
            // knows they are calls into another object file. Their parameter
            // names are documentation, so they get no symbols of their own.
            for block in &syntax.externs {
                for function in &block.functions {
                    // An `extern` block cannot be exported, so its names stay
                    // inside the module that asserted them.
                    resolver.declare(&function.name, SymbolKind::Function, Visibility::Private);
                }
            }
            for function in &syntax.functions {
                resolver.declare(&function.name, SymbolKind::Function, function.visibility);
            }
        }
    }

    // One scope per file, holding only that file's imports.
    for (index, file) in program.files.iter().enumerate() {
        let id = FileId(index);
        resolver.file = id;
        resolver.module = Some(file.module);
        resolver.current = resolver.result.module_scopes[file.module.0];
        resolver.enter_scope(Some(file.program.span));
        resolver.result.file_scopes[index] = resolver.current;
        for import in &file.program.imports {
            let Some(target) = program
                .modules
                .iter()
                .position(|module| module.path == import.path)
            else {
                continue;
            };
            // A qualifier may shadow a prelude binding, like any other name,
            // but never another import: the loader rejected that already.
            resolver.insert(
                &import.qualifier.text,
                SymbolKind::Module(ModuleId(target)),
                None,
            );
        }
    }

    // Bodies last, so every declaration in the program is already visible.
    for (index, file) in program.files.iter().enumerate() {
        let id = FileId(index);
        resolver.file = id;
        resolver.module = Some(file.module);
        let file_scope = resolver.result.file_scopes[index];
        let syntax = &file.program;
        for block in &syntax.externs {
            resolver.current = file_scope;
            for function in &block.functions {
                for parameter in &function.parameters {
                    resolver.type_ref(&parameter.type_ref);
                }
                if let Some(t) = &function.return_type {
                    resolver.type_ref(t);
                }
            }
        }
        // Methods get symbols, but in a scope of their own so they never
        // resolve as bare identifiers: a method is reached through `this` or a
        // value.
        for declaration in &syntax.structs {
            // A field default is written in the type's declaration but runs at
            // every construction, where there is no object yet. It resolves in
            // the file scope for that reason: `this` is not in scope, and
            // neither is another field.
            for field in &declaration.fields {
                resolver.current = file_scope;
                resolver.type_ref(&field.type_ref);
                if let Some(default) = &field.default {
                    resolver.in_field_default = true;
                    resolver.expression(default);
                    resolver.in_field_default = false;
                }
            }
            resolver.current = file_scope;
            resolver.enter_scope(Some(declaration.span));
            for method in &declaration.methods {
                resolver.declare(&method.name, SymbolKind::Function, Visibility::Public);
            }
            let method_scope = resolver.current;
            for method in &declaration.methods {
                // Bodies resolve from the file scope, so a sibling method is
                // not visible without a receiver.
                resolver.current = file_scope;
                for parameter in &method.parameters {
                    resolver.type_ref(&parameter.type_ref);
                }
                if let Some(t) = &method.return_type {
                    resolver.type_ref(t);
                }
                resolver.enter_scope(Some(method.body.span));
                resolver.insert(
                    "this",
                    SymbolKind::Parameter,
                    Some(Span::new(method.body.span.start, method.body.span.start)),
                );
                for parameter in &method.parameters {
                    resolver.declare(&parameter.name, SymbolKind::Parameter, Visibility::Private);
                }
                resolver.statements(&method.body);
                resolver.leave();
                resolver.current = method_scope;
            }
            resolver.leave();
        }
        for function in &syntax.functions {
            resolver.current = file_scope;
            for parameter in &function.parameters {
                resolver.type_ref(&parameter.type_ref);
            }
            if let Some(t) = &function.return_type {
                resolver.type_ref(t);
            }
            resolver.enter_scope(Some(function.body.span));
            for parameter in &function.parameters {
                resolver.declare(&parameter.name, SymbolKind::Parameter, Visibility::Private);
            }
            // Parameters and the outermost function body share one lexical scope.
            resolver.statements(&function.body);
            resolver.leave();
        }
        for constant in &syntax.constants {
            resolver.current = file_scope;
            if let Some(t) = &constant.type_ref {
                resolver.type_ref(t);
            }
            resolver.expression(&constant.value);
        }
    }
    ResolveOutput {
        resolution: resolver.diagnostics.is_empty().then_some(resolver.result),
        diagnostics: resolver.diagnostics,
    }
}

struct Resolver {
    result: Resolution,
    diagnostics: Vec<FileDiagnostic>,
    current: ScopeId,
    /// The file being walked. Declaration and reference keys carry it, since
    /// byte offsets alone no longer identify a position in the program.
    file: FileId,
    module: Option<ModuleId>,
    /// The body scope and body offset of each lambda being walked, innermost
    /// last. A name resolved outside one of these crossed its boundary.
    lambdas: Vec<(ScopeId, usize)>,
    /// Whether the expression being walked is a field default, which changes
    /// only what an unknown name is told.
    in_field_default: bool,
}
impl Resolver {
    /// Whether `scope` lies inside `outer`, which is what decides whether a
    /// name a lambda used is its own or one it captured.
    fn within(&self, outer: ScopeId, scope: ScopeId) -> bool {
        let mut current = Some(scope);
        while let Some(id) = current {
            if id == outer {
                return true;
            }
            current = self.result.scopes[id.0].parent;
        }
        false
    }
    /// Record a name a lambda body used but did not declare. Nested lambdas
    /// each capture it in turn, since an inner one can only read what the
    /// outer one already carries.
    fn capture(&mut self, symbol: SymbolId, span: Span) {
        if self.lambdas.is_empty() {
            return;
        }
        let kind = self.result.symbols[symbol.0].kind;
        if !matches!(kind, SymbolKind::Parameter | SymbolKind::Variable(_)) {
            // Functions, enums, modules and the prelude are reachable from
            // anywhere; only a binding can be captured.
            return;
        }
        let scope = self.result.symbols[symbol.0].scope;
        for index in 0..self.lambdas.len() {
            let (lambda_scope, offset) = self.lambdas[index];
            if self.within(lambda_scope, scope) {
                continue;
            }
            if kind == SymbolKind::Variable(Mutability::Mutable) {
                self.error(
                    DiagnosticCode::ImmutableAssignment,
                    span,
                    format!(
                        "`{}` is declared with `var`, and a function value captures values rather than variables",
                        self.result.symbols[symbol.0].name
                    ),
                    "copy it into a `let` before the function value, or pass it as a parameter"
                        .into(),
                );
                return;
            }
            let captures = self.result.captures.entry((self.file, offset)).or_default();
            if !captures.contains(&symbol) {
                captures.push(symbol);
            }
        }
    }
    fn error(&mut self, code: DiagnosticCode, span: Span, message: String, help: String) {
        self.diagnostics.push(FileDiagnostic {
            file: self.file,
            diagnostic: Diagnostic {
                code,
                message,
                span,
                help: Some(help),
                fix: None,
            },
        });
    }
    fn enter(&mut self, span: Span) {
        self.enter_scope(Some(span));
    }
    fn enter_scope(&mut self, span: Option<Span>) {
        let id = ScopeId(self.result.scopes.len());
        self.result.scopes.push(Scope {
            parent: Some(self.current),
            span,
            symbols: BTreeMap::new(),
        });
        self.current = id;
    }
    fn leave(&mut self) {
        if let Some(parent) = self.result.scopes[self.current.0].parent {
            self.current = parent;
        }
    }
    fn insert(&mut self, name: &str, kind: SymbolKind, span: Option<Span>) -> SymbolId {
        self.insert_with(name, kind, span, Visibility::Private)
    }
    fn insert_with(
        &mut self,
        name: &str,
        kind: SymbolKind,
        span: Option<Span>,
        visibility: Visibility,
    ) -> SymbolId {
        let id = SymbolId(self.result.symbols.len());
        self.result.symbols.push(Symbol {
            name: name.into(),
            kind,
            span,
            scope: self.current,
            visibility,
            module: self.module,
        });
        self.result.scopes[self.current.0]
            .symbols
            .insert(name.into(), id);
        if let Some(span) = span {
            self.result.declarations.insert((self.file, span.start), id);
        }
        id
    }
    fn declare(&mut self, name: &Name, kind: SymbolKind, visibility: Visibility) {
        if self.result.scopes[self.current.0]
            .symbols
            .contains_key(&name.text)
        {
            self.error(
                DiagnosticCode::DuplicateDeclaration,
                name.span,
                format!("duplicate declaration of `{}` in the same scope", name.text),
                "rename this declaration or introduce a child block to shadow the existing binding"
                    .into(),
            );
            // Keep the first binding so further diagnostics remain predictable.
        } else {
            self.insert_with(&name.text, kind, Some(name.span), visibility);
        }
    }
    fn lookup(&self, name: &str) -> Option<SymbolId> {
        let mut scope = Some(self.current);
        while let Some(id) = scope {
            let current = &self.result.scopes[id.0];
            if let Some(symbol) = current.symbols.get(name) {
                return Some(*symbol);
            }
            scope = current.parent;
        }
        None
    }
    fn reference(&mut self, name: &Name) {
        if let Some(id) = self.lookup(&name.text) {
            self.result
                .references
                .insert((self.file, name.span.start), id);
            self.capture(id, name.span);
            return;
        }
        // A name one slip away from one that is in scope is a typo, and the
        // whole edit is known: replace what was written with what was meant.
        if let Some(meant) = self.nearest_in_scope(&name.text) {
            let diagnostic = Diagnostic {
                code: DiagnosticCode::UnknownName,
                span: name.span,
                message: format!("unknown identifier `{}`", name.text),
                help: Some(format!("did you mean `{meant}`?")),
                fix: None,
            }
            .with_fix(Fix::new(
                format!("change to `{meant}`"),
                name.span,
                meant.clone(),
            ));
            self.diagnostics.push(FileDiagnostic {
                file: self.file,
                diagnostic,
            });
            return;
        }
        // A field default is written inside a type but runs where the object
        // is being made, so the two names a reader reaches for first — `this`
        // and a sibling field — are exactly the ones that are not there.
        let help = if self.in_field_default {
            "a field default is evaluated at every construction, before there is an object, so `this` and the other fields are not in scope"
        } else {
            "check the spelling or declare this name in an enclosing scope before using it"
        };
        self.error(
            DiagnosticCode::UnknownName,
            name.span,
            format!("unknown identifier `{}`", name.text),
            help.into(),
        );
    }
    /// The name a misspelling most likely meant, over every scope from here
    /// outwards — which is the same set `lookup` searches, so a suggestion is
    /// never a name the use could not have reached.
    fn nearest_in_scope(&self, written: &str) -> Option<String> {
        let mut visible = Vec::new();
        let mut scope = Some(self.current);
        while let Some(id) = scope {
            let current = &self.result.scopes[id.0];
            visible.extend(current.symbols.keys().map(String::as_str));
            scope = current.parent;
        }
        nearest(written, visible.into_iter())
    }
    /// The right half of `module.name`. A module's scope is not an enclosing
    /// scope of the importing file, so this looks in exactly one place rather
    /// than walking parents, and the name has to be exported to be found.
    /// Whether a callee is `size_of` or `offset_of` as the prelude defines
    /// them — a shadowing binding of either name is an ordinary call again.
    fn names_layout_builtin(&self, callee: &Expr) -> bool {
        let ExprKind::Identifier(name) = &callee.kind else {
            return false;
        };
        matches!(
            self.lookup(&name.text)
                .map(|symbol| self.result.symbols[symbol.0].kind),
            Some(SymbolKind::Builtin(Builtin::SizeOf | Builtin::OffsetOf))
        )
    }
    fn module_member(&mut self, module: ModuleId, qualifier: &Name, name: &Name) {
        let scope = self.result.module_scopes[module.0];
        let Some(symbol) = self.result.scopes[scope.0].symbols.get(&name.text).copied() else {
            // A module's scope is one place rather than a chain, so the
            // candidates are exactly its public names.
            let exported = self.result.scopes[scope.0]
                .symbols
                .iter()
                .filter(|(_, symbol)| {
                    self.result.symbols[symbol.0].visibility == Visibility::Public
                })
                .map(|(name, _)| name.as_str());
            if let Some(meant) = nearest(&name.text, exported) {
                let diagnostic = Diagnostic {
                    code: DiagnosticCode::UnknownName,
                    span: name.span,
                    message: format!("module `{}` declares no `{}`", qualifier.text, name.text),
                    help: Some(format!("did you mean `{}.{meant}`?", qualifier.text)),
                    fix: None,
                }
                .with_fix(Fix::new(
                    format!("change to `{meant}`"),
                    name.span,
                    meant.clone(),
                ));
                self.diagnostics.push(FileDiagnostic {
                    file: self.file,
                    diagnostic,
                });
                return;
            }
            self.error(
                DiagnosticCode::UnknownName,
                name.span,
                format!("module `{}` declares no `{}`", qualifier.text, name.text),
                "check the spelling, or the module's own declarations".into(),
            );
            return;
        };
        if self.result.symbols[symbol.0].visibility != Visibility::Public {
            self.error(
                DiagnosticCode::PrivateName,
                name.span,
                format!("`{}` is private to module `{}`", name.text, qualifier.text),
                format!(
                    "write `pub` on its declaration in `{}` to export it",
                    qualifier.text
                ),
            );
            return;
        }
        self.result
            .references
            .insert((self.file, name.span.start), symbol);
    }
    fn statements(&mut self, block: &Block) {
        for statement in &block.statements {
            self.statement(statement);
        }
    }
    fn block(&mut self, block: &Block) {
        self.enter(block.span);
        self.statements(block);
        self.leave();
    }
    fn statement(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Variable(variable) => {
                if let Some(type_ref) = &variable.type_ref {
                    self.type_ref(type_ref);
                }
                // A binding becomes visible only after its initializer.
                self.expression(&variable.initializer);
                // The escape block runs when there is no value, so the name
                // being declared is not in scope inside it; the error it names
                // is, and nowhere else.
                if let Some(otherwise) = &variable.otherwise {
                    self.enter(otherwise.block.span);
                    if let Some(binding) = &otherwise.binding {
                        self.declare(
                            binding,
                            SymbolKind::Variable(Mutability::Immutable),
                            Visibility::Private,
                        );
                    }
                    self.statements(&otherwise.block);
                    self.leave();
                }
                self.declare(
                    &variable.name,
                    SymbolKind::Variable(variable.mutability),
                    Visibility::Private,
                );
            }
            StatementKind::Constant(constant) => {
                if let Some(type_ref) = &constant.type_ref {
                    self.type_ref(type_ref);
                }
                self.expression(&constant.value);
                self.declare(&constant.name, SymbolKind::Constant, Visibility::Private);
            }
            StatementKind::Expression(expr) => self.expression(expr),
            StatementKind::Return(value) => {
                if let Some(expr) = value {
                    self.expression(expr);
                }
            }
            StatementKind::Block(block) | StatementKind::Unsafe(block) => self.block(block),
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                self.expression(condition);
                self.block(then_block);
                if let Some(branch) = else_branch {
                    self.statement(branch);
                }
            }
            StatementKind::IfLet {
                binding,
                value,
                pattern: _,
                then_block,
                else_branch,
            } => {
                self.expression(value);
                self.enter(then_block.span);
                self.declare(
                    binding,
                    SymbolKind::Variable(Mutability::Immutable),
                    Visibility::Private,
                );
                self.statements(then_block);
                self.leave();
                if let Some(branch) = else_branch {
                    self.statement(branch);
                }
            }
            StatementKind::While { condition, body } => {
                // The condition is evaluated in the enclosing scope; the body
                // gets its own child scope, like any other block.
                self.expression(condition);
                self.block(body);
            }
            StatementKind::Loop { body } => self.block(body),
            // Jumps bind to the innermost loop; they introduce no names.
            StatementKind::Break | StatementKind::Continue => {}
            StatementKind::Match { value, arms } => {
                self.expression(value);
                for arm in arms {
                    self.enter(arm.body.span);
                    match &arm.pattern {
                        MatchPattern::Variant {
                            binding: Some(binding),
                            ..
                        } => {
                            self.declare(
                                binding,
                                SymbolKind::Variable(Mutability::Immutable),
                                Visibility::Private,
                            );
                        }
                        MatchPattern::Variant {
                            enum_name,
                            variant_name,
                            binding: None,
                            ..
                        } => {
                            if let Some(path) = enum_name {
                                if path.module.is_none()
                                    && let Some(sym) = self.lookup(&path.name.text)
                                    && let SymbolKind::Module(mod_id) =
                                        self.result.symbols[sym.0].kind
                                {
                                    self.result
                                        .references
                                        .insert((self.file, path.name.span.start), sym);
                                    self.module_member(mod_id, &path.name, variant_name);
                                }
                            } else if let Some(sym) = self.lookup(&variant_name.text)
                                && matches!(self.result.symbols[sym.0].kind, SymbolKind::Constant)
                            {
                                self.result
                                    .references
                                    .insert((self.file, variant_name.span.start), sym);
                            }
                        }
                        MatchPattern::Constant(expr) => {
                            self.expression(expr);
                        }
                        MatchPattern::Range { start, end, .. } => {
                            self.expression(start);
                            self.expression(end);
                        }
                        MatchPattern::Wildcard(_) => {}
                    }
                    self.statements(&arm.body);
                    self.leave();
                }
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                match iterable {
                    ForIterable::Range { start, end } => {
                        self.expression(start);
                        self.expression(end);
                    }
                    ForIterable::Expr(collection) => {
                        self.expression(collection);
                    }
                }
                self.enter(body.span);
                self.declare(
                    variable,
                    SymbolKind::Variable(Mutability::Immutable),
                    Visibility::Private,
                );
                self.statements(body);
                self.leave();
            }
        }
    }
    fn expression(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Identifier(name) => self.reference(name),
            ExprKind::Group(inner)
            | ExprKind::Try(inner)
            | ExprKind::Unary { operand: inner, .. } => self.expression(inner),
            ExprKind::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            ExprKind::Assignment { target, value, .. } => {
                self.expression(target);
                self.expression(value);
            }
            ExprKind::Call { callee, arguments } => {
                self.expression(callee);
                // `size_of(Type)` and `offset_of(Type, field)` are asked about
                // a type and one of its fields, neither of which is a value
                // name. Resolving them here would report every one of them as
                // unknown, so the checker reads the syntax instead.
                if self.names_layout_builtin(callee) {
                    return;
                }
                for argument in arguments {
                    self.expression(argument);
                }
            }
            ExprKind::Member { object, member } => {
                // `json.parse` is one name in two halves, not a member of a
                // value called `json`. A local binding of the same name wins,
                // because `lookup` finds the innermost scope first.
                if let ExprKind::Identifier(qualifier) = &object.kind
                    && let Some(symbol) = self.lookup(&qualifier.text)
                    && let SymbolKind::Module(module) = self.result.symbols[symbol.0].kind
                {
                    self.result
                        .references
                        .insert((self.file, qualifier.span.start), symbol);
                    self.module_member(module, qualifier, member);
                    return;
                }
                self.expression(object)
            }
            ExprKind::Interpolation(parts) => {
                for part in parts {
                    if let InterpolationPart::Value(value) = part {
                        self.expression(value);
                    }
                }
            }
            // The type name and the field labels are not value names; only the
            // field values are resolved here.
            ExprKind::StructLiteral { fields, .. } | ExprKind::New { fields, .. } => {
                for field in fields {
                    self.expression(&field.value);
                }
            }
            ExprKind::Lambda(lambda) => {
                for parameter in &lambda.parameters {
                    if let Some(t) = &parameter.type_ref {
                        self.type_ref(t);
                    }
                }
                if let Some(t) = &lambda.return_type {
                    self.type_ref(t);
                }
                // Parameters and the body share one scope, as they do in a
                // declared function. Names the body does not bind resolve
                // outward, which is what makes a capture a capture.
                self.enter(lambda.body.span);
                for parameter in &lambda.parameters {
                    self.declare(&parameter.name, SymbolKind::Parameter, Visibility::Private);
                }
                self.lambdas.push((self.current, lambda.body.span.start));
                self.statements(&lambda.body);
                self.lambdas.pop();
                self.leave();
            }
            ExprKind::Weak(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            ExprKind::Array(elements) => {
                for element in elements {
                    self.expression(element);
                }
            }
            ExprKind::ArrayRepeat { element, count } => {
                self.expression(element);
                self.expression(count);
            }
            ExprKind::Index { object, index } => {
                self.expression(object);
                self.expression(index);
            }
            ExprKind::Slice { object, start, end } => {
                self.expression(object);
                self.expression(start);
                self.expression(end);
            }
        }
    }
    fn type_ref(&mut self, type_ref: &TypeRef) {
        match type_ref {
            TypeRef::FixedArray { element, size, .. } => {
                self.expression(size);
                self.type_ref(element);
            }
            TypeRef::Array { element, .. }
            | TypeRef::Option { element, .. }
            | TypeRef::Pointer {
                pointee: element, ..
            } => {
                self.type_ref(element);
            }
            TypeRef::Result { ok, err, .. } => {
                self.type_ref(ok);
                self.type_ref(err);
            }
            TypeRef::Function {
                parameters,
                return_type,
                ..
            } => {
                for p in parameters {
                    self.type_ref(p);
                }
                if let Some(ret) = return_type {
                    self.type_ref(ret);
                }
            }
            TypeRef::Named(_) | TypeRef::Weak { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests;
