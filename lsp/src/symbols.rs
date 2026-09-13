//! The outline of a document: `textDocument/documentSymbol`.
//!
//! This is the one answer that comes from the syntax alone. Nothing here asks
//! the resolver or the checker anything, because an outline is a list of what
//! was written, not of what it means — a class whose field has an unknown type
//! still belongs in the breadcrumb bar.

use skuld_compiler::ast::{
    self, ExternBlock, FunctionDecl, MethodSignature, Parameter, Program, StructDecl, TypeDeclKind,
    TypeRef,
};
use skuld_compiler::span::Span;

/// The LSP `SymbolKind` values this server produces. They are numbers on the
/// wire; naming them here keeps the mapping in one readable place.
pub mod kind {
    pub const MODULE: f64 = 2.0;
    pub const CLASS: f64 = 5.0;
    pub const METHOD: f64 = 6.0;
    pub const FIELD: f64 = 8.0;
    pub const ENUM: f64 = 10.0;
    pub const INTERFACE: f64 = 11.0;
    pub const FUNCTION: f64 = 12.0;
    pub const ENUM_MEMBER: f64 = 22.0;
    pub const STRUCT: f64 = 23.0;
}

/// One entry of the outline. `range` covers the whole declaration and
/// `selection` only its name, which is what a client reveals when the entry is
/// picked; the specification requires the second to sit inside the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: f64,
    pub range: Span,
    pub selection: Span,
    pub children: Vec<Symbol>,
}

/// The outline of a parsed program, in source order.
///
/// The AST keeps each kind of declaration in its own list, so an interface
/// written between two functions would otherwise be reported after both. A
/// client does not re-sort what it is given, and an outline that disagrees
/// with the file is worse than no outline.
pub fn outline(program: &Program) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for declaration in &program.interfaces {
        symbols.push(Symbol {
            name: declaration.name.text.clone(),
            detail: None,
            kind: kind::INTERFACE,
            range: declaration.span,
            selection: declaration.name.span,
            children: declaration.methods.iter().map(signature_symbol).collect(),
        });
    }
    for declaration in &program.structs {
        symbols.push(type_symbol(declaration));
    }
    for declaration in &program.enums {
        symbols.push(Symbol {
            name: declaration.name.text.clone(),
            detail: None,
            kind: kind::ENUM,
            range: declaration.span,
            selection: declaration.name.span,
            children: declaration
                .variants
                .iter()
                .map(|variant| Symbol {
                    name: variant.name.text.clone(),
                    detail: variant.payload.as_ref().map(type_text),
                    kind: kind::ENUM_MEMBER,
                    range: variant.span,
                    selection: variant.name.span,
                    children: Vec::new(),
                })
                .collect(),
        });
    }
    for declaration in &program.functions {
        symbols.push(function_symbol(declaration, kind::FUNCTION));
    }
    for block in &program.externs {
        symbols.push(extern_symbol(block));
    }
    sort_by_position(&mut symbols);
    symbols
}

/// The outline flattened to one entry per symbol, each paired with the name
/// of the declaration that holds it. A workspace search wants a flat list —
/// there is no tree to expand in a list of results from many files — but the
/// container is what tells two `name` fields of two classes apart.
pub fn flatten(symbols: &[Symbol]) -> Vec<(&Symbol, Option<&str>)> {
    let mut flat = Vec::new();
    for symbol in symbols {
        flat.push((symbol, None));
        for child in &symbol.children {
            flat.push((child, Some(symbol.name.as_str())));
        }
    }
    flat
}

fn type_symbol(declaration: &StructDecl) -> Symbol {
    let mut children: Vec<Symbol> = declaration
        .fields
        .iter()
        .map(|field| Symbol {
            name: field.name.text.clone(),
            detail: Some(type_text(&field.type_ref)),
            kind: kind::FIELD,
            range: field.span,
            selection: field.name.span,
            children: Vec::new(),
        })
        .collect();
    children.extend(
        declaration
            .methods
            .iter()
            .map(|method| function_symbol(method, kind::METHOD)),
    );
    sort_by_position(&mut children);
    Symbol {
        name: declaration.name.text.clone(),
        // What the class conforms to is the fact a reader most often wants
        // from the outline, since the declaration itself is off screen.
        detail: conformance(declaration),
        kind: match declaration.kind {
            TypeDeclKind::Value => kind::STRUCT,
            TypeDeclKind::Reference => kind::CLASS,
        },
        range: declaration.span,
        selection: declaration.name.span,
        children,
    }
}

/// An `unsafe extern "C"` block is a namespace in the outline rather than a
/// flat run of functions: the block is the unsafe assertion, and collapsing it
/// hides every foreign name at once.
fn extern_symbol(block: &ExternBlock) -> Symbol {
    Symbol {
        name: format!("extern {:?}", block.abi),
        detail: None,
        kind: kind::MODULE,
        range: block.span,
        selection: block.abi_span,
        children: block
            .functions
            .iter()
            .map(|function| Symbol {
                name: function.name.text.clone(),
                detail: Some(signature_text(
                    &function.parameters,
                    function.return_type.as_ref(),
                )),
                kind: kind::FUNCTION,
                range: function.span,
                selection: function.name.span,
                children: Vec::new(),
            })
            .collect(),
    }
}

fn function_symbol(declaration: &FunctionDecl, kind: f64) -> Symbol {
    Symbol {
        name: declaration.name.text.clone(),
        detail: Some(signature_text(
            &declaration.parameters,
            declaration.return_type.as_ref(),
        )),
        kind,
        range: declaration.span,
        selection: declaration.name.span,
        children: Vec::new(),
    }
}

fn signature_symbol(signature: &MethodSignature) -> Symbol {
    Symbol {
        name: signature.name.text.clone(),
        detail: Some(signature_text(
            &signature.parameters,
            signature.return_type.as_ref(),
        )),
        kind: kind::METHOD,
        range: signature.span,
        selection: signature.name.span,
        children: Vec::new(),
    }
}

fn conformance(declaration: &StructDecl) -> Option<String> {
    if declaration.conforms.is_empty() {
        return None;
    }
    Some(format!(
        ": {}",
        declaration
            .conforms
            .iter()
            .map(path_text)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// `(a: int, b: int) -> int`. A void return is left implicit, as it is written.
fn signature_text(parameters: &[Parameter], return_type: Option<&TypeRef>) -> String {
    let mut text = String::from("(");
    for (index, parameter) in parameters.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&parameter.name.text);
        text.push_str(": ");
        text.push_str(&type_text(&parameter.type_ref));
    }
    text.push(')');
    if let Some(return_type) = return_type {
        text.push_str(" -> ");
        text.push_str(&type_text(return_type));
    }
    text
}

/// A source type written back out. This is the syntax, not the checker's
/// `Type`: an outline is drawn for a file that may not check at all.
pub fn type_text(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Named(path) => path_text(path),
        TypeRef::Option { element, .. } => format!("Option<{}>", type_text(element)),
        TypeRef::Result { ok, err, .. } => {
            format!("Result<{}, {}>", type_text(ok), type_text(err))
        }
        TypeRef::Weak { class, .. } => format!("weak {}", path_text(class)),
        TypeRef::Array { element, .. } => format!("[]{}", type_text(element)),
        TypeRef::Function {
            parameters,
            return_type,
            ..
        } => {
            let inside = parameters
                .iter()
                .map(type_text)
                .collect::<Vec<_>>()
                .join(", ");
            match return_type {
                Some(result) => format!("({inside}) -> {}", type_text(result)),
                None => format!("({inside})"),
            }
        }
        TypeRef::Pointer { pointee, .. } => format!("*{}", type_text(pointee)),
    }
}

fn path_text(path: &ast::Path) -> String {
    match &path.module {
        Some(module) => format!("{}.{}", module.text, path.name.text),
        None => path.name.text.clone(),
    }
}

fn sort_by_position(symbols: &mut [Symbol]) {
    symbols.sort_by_key(|symbol| symbol.range.start);
}

#[cfg(test)]
mod tests;
