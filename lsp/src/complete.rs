//! What to offer at a cursor.
//!
//! Completion has to answer while the file is mid-edit, which is exactly when
//! it does not parse. So it answers from the last check that succeeded: the
//! tables are a moment stale, and the text before the cursor is almost always
//! unchanged since, which is the part they are consulted about.
//!
//! Nothing here filters by what has been typed. A client filters and ranks the
//! list itself, and doing it twice only makes the two disagree.

use skuld_compiler::{
    module::{FileId, ModuleId},
    resolver::{SymbolId, SymbolKind},
    type_checker::TypedProgram,
    types::Type,
};

/// LSP `CompletionItemKind`, which travels as a number.
pub mod kind {
    pub const METHOD: f64 = 2.0;
    pub const FUNCTION: f64 = 3.0;
    pub const FIELD: f64 = 5.0;
    pub const VARIABLE: f64 = 6.0;
    pub const CLASS: f64 = 7.0;
    pub const MODULE: f64 = 9.0;
    pub const ENUM: f64 = 13.0;
    pub const KEYWORD: f64 = 14.0;
    pub const ENUM_MEMBER: f64 = 20.0;
    pub const CONSTANT: f64 = 21.0;
    pub const STRUCT: f64 = 22.0;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    pub kind: f64,
    /// The signature or type, shown beside the label.
    pub detail: Option<String>,
}

impl Item {
    fn new(label: impl Into<String>, kind: f64, detail: Option<String>) -> Self {
        Self {
            label: label.into(),
            kind,
            detail,
        }
    }
}

/// Every keyword the lexer produces, minus those that can never begin
/// something a user types at a fresh cursor.
const KEYWORDS: [&str; 22] = [
    "func",
    "let",
    "var",
    "return",
    "if",
    "else",
    "while",
    "loop",
    "break",
    "continue",
    "new",
    "weak",
    "class",
    "struct",
    "impl",
    "interface",
    "enum",
    "match",
    "import",
    "for",
    "in",
    "unsafe",
];

/// The entry file of a checked program is always its first file.
const ENTRY: FileId = FileId(0);

/// The completions for `source` at `offset`, using the last successful check
/// of this document if there is one. Without a check, only what needs no
/// semantic knowledge is offered, which is better than nothing and never wrong.
pub fn at(source: &str, offset: usize, checked: Option<&TypedProgram>) -> Vec<Item> {
    let offset = offset.min(source.len());
    let prefix_start = identifier_start(source, offset);
    let before = source[..prefix_start].trim_end();
    match before.strip_suffix('.') {
        Some(receiver) => members(source, receiver.trim_end(), checked),
        None => names(checked),
    }
}

/// Where the identifier ending at `offset` begins. An offset that is not
/// inside an identifier yields itself, which makes the prefix empty.
fn identifier_start(source: &str, offset: usize) -> usize {
    let mut start = offset;
    while start > 0 {
        let candidate = source[..start]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        let character = source[candidate..start].chars().next().unwrap_or(' ');
        if character.is_alphanumeric() || character == '_' {
            start = candidate;
        } else {
            break;
        }
    }
    start
}

/// What follows a `.`: the members of whatever is on its left.
fn members(source: &str, receiver: &str, checked: Option<&TypedProgram>) -> Vec<Item> {
    let start = identifier_start(receiver, receiver.len());
    let name = &receiver[start..];
    let Some(typed) = checked else {
        return Vec::new();
    };
    if name.is_empty() {
        return Vec::new();
    }
    // `Command.` names an enum, not a value: the variants are the members.
    if let Some(info) = typed.enums().iter().find(|info| info.name == name) {
        return info
            .variants
            .iter()
            .map(|variant| {
                let detail = variant
                    .payload
                    .map(|payload| format!("{}({})", variant.name, type_name(typed, payload)));
                Item::new(&variant.name, kind::ENUM_MEMBER, detail)
            })
            .collect();
    }
    // Otherwise the receiver is a value, and its type says what it has. The
    // offset is the one the last good check saw, which is why the text before
    // the cursor being unchanged is what makes this work.
    // `receiver` is a prefix of `source`, so an offset inside it is that same
    // offset in the whole document — which is the key the tables use.
    let _ = source;
    let resolution = typed.resolution();
    let Some(&symbol) = resolution.references.get(&(ENTRY, start)) else {
        return Vec::new();
    };
    if let SymbolKind::Module(module) = resolution.symbols[symbol.0].kind {
        return exports(typed, module);
    }
    members_of(typed, typed.symbol_type(symbol))
}

/// The members a value of this type has. Hover reads the same table, so a
/// builtin method's shape is written once.
pub fn members_of(typed: &TypedProgram, ty: Type) -> Vec<Item> {
    let mut items = Vec::new();
    match ty {
        Type::Struct(id) => {
            let Some(info) = typed.structs().get(id.0) else {
                return items;
            };
            for field in &info.fields {
                items.push(Item::new(
                    &field.name,
                    kind::FIELD,
                    Some(type_name(typed, field.ty)),
                ));
            }
            for method in &info.methods {
                // A method's signature holds its declared parameters; the
                // implicit receiver is added at lowering, not here.
                let detail = typed.signature(method.id).map(|signature| {
                    format!(
                        "({}) -> {}",
                        signature
                            .parameters
                            .iter()
                            .map(|parameter| type_name(typed, *parameter))
                            .collect::<Vec<_>>()
                            .join(", "),
                        type_name(typed, signature.return_type)
                    )
                });
                items.push(Item::new(&method.name, kind::METHOD, detail));
            }
        }
        // The builtin methods, which are the checker's table read the other
        // way round. A name added there has to be added here.
        Type::Array(id) => {
            let element = typed
                .arrays()
                .get(id.0)
                .map(|info| type_name(typed, info.element))
                .unwrap_or_else(|| "T".to_owned());
            items.push(Item::new("len", kind::METHOD, Some("() -> int".into())));
            items.push(Item::new(
                "push",
                kind::METHOD,
                Some(format!("({element}) -> void")),
            ));
            items.push(Item::new(
                "insert",
                kind::METHOD,
                Some(format!("(int, {element}) -> void")),
            ));
            items.push(Item::new(
                "pop",
                kind::METHOD,
                Some(format!("() -> Option<{element}>")),
            ));
            items.push(Item::new(
                "remove",
                kind::METHOD,
                Some(format!("(int) -> Option<{element}>")),
            ));
            items.push(Item::new(
                "sort",
                kind::METHOD,
                Some(format!("(({element}, {element}) -> int) -> void")),
            ));
        }
        Type::String => {
            items.push(Item::new("len", kind::METHOD, Some("() -> int".into())));
            items.push(Item::new("bytes", kind::METHOD, Some("() -> []u8".into())));
        }
        Type::Option(_) => {
            items.push(Item::new(
                "is_some",
                kind::METHOD,
                Some("() -> bool".into()),
            ));
            items.push(Item::new(
                "is_none",
                kind::METHOD,
                Some("() -> bool".into()),
            ));
        }
        Type::Result(_) => {
            items.push(Item::new("is_ok", kind::METHOD, Some("() -> bool".into())));
            items.push(Item::new("is_err", kind::METHOD, Some("() -> bool".into())));
        }
        Type::Weak(id) => {
            let class = typed
                .structs()
                .get(id.0)
                .map(|info| info.name.clone())
                .unwrap_or_else(|| "Class".to_owned());
            items.push(Item::new("alive", kind::METHOD, Some("() -> bool".into())));
            items.push(Item::new(
                "get",
                kind::METHOD,
                Some(format!("() -> {class}")),
            ));
            items.push(Item::new(
                "upgrade",
                kind::METHOD,
                Some(format!("() -> Option<{class}>")),
            ));
        }
        _ => {}
    }
    items
}

/// What an imported module offers, which is only what it exported.
fn exports(typed: &TypedProgram, module: ModuleId) -> Vec<Item> {
    let resolution = typed.resolution();
    let Some(&scope) = resolution.module_scopes.get(module.0) else {
        return Vec::new();
    };
    let mut items: Vec<Item> = resolution.scopes[scope.0]
        .symbols
        .iter()
        .map(|(name, id)| item_for(typed, name, *id))
        .collect();
    // A type is not a value name, so it lives in the checker's tables instead.
    for info in typed.structs().iter().filter(|info| info.module == module) {
        let kind = if info.reference {
            kind::CLASS
        } else {
            kind::STRUCT
        };
        items.push(Item::new(&info.name, kind, None));
    }
    items
}

/// Everything nameable at a bare cursor: keywords, the prelude, this module's
/// declarations, the file's import qualifiers and its own bindings.
fn names(checked: Option<&TypedProgram>) -> Vec<Item> {
    let mut items: Vec<Item> = KEYWORDS
        .iter()
        .map(|keyword| Item::new(*keyword, kind::KEYWORD, None))
        .collect();
    let Some(typed) = checked else {
        return items;
    };
    let resolution = typed.resolution();
    let mut scopes = vec![0];
    if let Some(scope) = resolution.file_scopes.first() {
        scopes.push(scope.0);
    }
    if let Some(scope) = resolution.module_scopes.first() {
        scopes.push(scope.0);
    }
    for scope in scopes {
        let Some(scope) = resolution.scopes.get(scope) else {
            continue;
        };
        for (name, id) in &scope.symbols {
            items.push(item_for(typed, name, *id));
        }
    }
    // Bindings declared in this file. Scope is not consulted: a name from a
    // sibling function is a worse answer than a missing one, and the client
    // ranks by what has been typed anyway.
    for (&(file, _), &id) in &resolution.declarations {
        if file != ENTRY {
            continue;
        }
        let symbol = &resolution.symbols[id.0];
        if matches!(
            symbol.kind,
            SymbolKind::Variable(_) | SymbolKind::Parameter | SymbolKind::Function
        ) {
            items.push(item_for(typed, &symbol.name, id));
        }
    }
    for info in typed.structs() {
        let kind = if info.reference {
            kind::CLASS
        } else {
            kind::STRUCT
        };
        items.push(Item::new(&info.name, kind, None));
    }
    for info in typed.enums() {
        items.push(Item::new(&info.name, kind::ENUM, None));
    }
    items.sort_by(|left, right| left.label.cmp(&right.label));
    items.dedup_by(|left, right| left.label == right.label && left.kind == right.kind);
    items
}

fn item_for(typed: &TypedProgram, name: &str, id: SymbolId) -> Item {
    let symbol = &typed.resolution().symbols[id.0];
    match symbol.kind {
        SymbolKind::Function => {
            let detail = typed.signature(id).map(|signature| {
                format!(
                    "({}) -> {}",
                    signature
                        .parameters
                        .iter()
                        .map(|parameter| type_name(typed, *parameter))
                        .collect::<Vec<_>>()
                        .join(", "),
                    type_name(typed, signature.return_type)
                )
            });
            Item::new(name, kind::FUNCTION, detail)
        }
        SymbolKind::Enum => Item::new(name, kind::ENUM, None),
        SymbolKind::Module(_) => Item::new(name, kind::MODULE, None),
        SymbolKind::Builtin(_) => Item::new(name, kind::FUNCTION, None),
        SymbolKind::Constant => Item::new(
            name,
            kind::CONSTANT,
            Some(type_name(typed, typed.symbol_type(id))),
        ),
        SymbolKind::Parameter | SymbolKind::Variable(_) => Item::new(
            name,
            kind::VARIABLE,
            Some(type_name(typed, typed.symbol_type(id))),
        ),
    }
}

/// A type as a user writes it. `Display` cannot reach the struct table, so the
/// names come from the checked program the way the compiler's own diagnostics
/// render them.
pub fn type_name(typed: &TypedProgram, ty: Type) -> String {
    match ty {
        Type::Struct(id) => typed
            .structs()
            .get(id.0)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| "<struct>".to_owned()),
        Type::Enum(id) => typed
            .enums()
            .get(id.0)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| "<enum>".to_owned()),
        Type::Array(id) => typed
            .arrays()
            .get(id.0)
            .map(|info| format!("[]{}", type_name(typed, info.element)))
            .unwrap_or_else(|| "[]".to_owned()),
        Type::Option(id) => typed
            .options()
            .get(id.0)
            .map(|info| format!("Option<{}>", type_name(typed, info.element)))
            .unwrap_or_else(|| "Option".to_owned()),
        Type::Result(id) => typed
            .results()
            .get(id.0)
            .map(|info| {
                format!(
                    "Result<{}, {}>",
                    type_name(typed, info.ok),
                    type_name(typed, info.err)
                )
            })
            .unwrap_or_else(|| "Result".to_owned()),
        Type::Weak(id) => typed
            .structs()
            .get(id.0)
            .map(|info| format!("weak {}", info.name))
            .unwrap_or_else(|| "weak".to_owned()),
        Type::Interface(id) => typed
            .interfaces()
            .get(id.0)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| "<interface>".to_owned()),
        // A function type has no declaration to name it, so it is written the
        // way the language writes one: `(int, int) -> bool`.
        Type::Function(id) => typed
            .function_signatures()
            .get(id.0)
            .map(|info| {
                format!(
                    "({}) -> {}",
                    info.parameters
                        .iter()
                        .map(|parameter| type_name(typed, *parameter))
                        .collect::<Vec<_>>()
                        .join(", "),
                    type_name(typed, info.return_type)
                )
            })
            .unwrap_or_else(|| "<function>".to_owned()),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
