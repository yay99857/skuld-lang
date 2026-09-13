//! Signature help: which call the cursor is inside, and which argument it is
//! on.
//!
//! Finding the call is the hard half, and it is done over the text rather than
//! the syntax tree for the same reason completion is: the moment a signature
//! is wanted is the moment the call is half-written and the file does not
//! parse. What the text gives is the innermost unclosed `(` before the cursor
//! and the number of top-level commas since it; the name in front of that
//! parenthesis is then looked up in the last check that succeeded.

use crate::query::{self, Target, Word};
use crate::symbols::type_text;
use skuld_compiler::{resolver::SymbolId, type_checker::TypedProgram, types::Type};

/// A call the cursor is inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Byte offset of the `(` that opened the argument list.
    pub open: usize,
    /// Top-level commas between that `(` and the cursor, which is the index of
    /// the argument being written.
    pub argument: usize,
}

/// What to show above a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Help {
    /// The whole signature on one line, as a reader would write it.
    pub label: String,
    /// Where each parameter sits inside `label`, in **UTF-16 code units**,
    /// which is what the protocol measures a parameter label in.
    pub parameters: Vec<(usize, usize)>,
    /// Which parameter the cursor is on, when that question has an answer.
    /// A construction names its fields, so position means nothing there.
    pub active: Option<usize>,
}

/// The signature of the call the cursor is inside, if there is one.
pub fn at(source: &str, offset: usize, typed: &TypedProgram) -> Option<Help> {
    let call = call_at(source, offset)?;
    let before = source[..call.open].trim_end();
    let callee = query::word_at(before, before.len())?;
    // The name has to be looked up where it is written, not where it was
    // trimmed to, so that the tables agree with the offsets they recorded.
    let target = query::target_at(source, callee.start, typed)?;
    let mut help = describe(typed, &target, &callee)?;
    if help.parameters.is_empty() {
        help.active = None;
    } else if help.active.is_some() {
        // A client is free to ask about an argument that does not exist, and
        // an out-of-range index would highlight nothing at all; the last
        // parameter is the one still being written.
        help.active = Some(call.argument.min(help.parameters.len() - 1));
    }
    Some(help)
}

/// The innermost call the cursor sits in the arguments of.
///
/// Strings, characters and comments are skipped, because a `(` inside one
/// opens nothing. A `[` or a `{` is tracked too: if the innermost thing still
/// open is a lambda body or an array literal, the cursor is not in an argument
/// list any more, and answering with the enclosing call's signature would
/// point at the wrong thing.
pub fn call_at(source: &str, offset: usize) -> Option<Call> {
    struct Open {
        bracket: u8,
        start: usize,
        commas: usize,
    }
    let offset = offset.min(source.len());
    let bytes = source.as_bytes();
    let mut stack: Vec<Open> = Vec::new();
    let mut index = 0;
    while index < offset {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < offset && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            quote @ (b'"' | b'\'') => {
                index += 1;
                while index < offset {
                    if bytes[index] == b'\\' {
                        index += 2;
                        continue;
                    }
                    if bytes[index] == quote {
                        break;
                    }
                    index += 1;
                }
            }
            bracket @ (b'(' | b'[' | b'{') => stack.push(Open {
                bracket,
                start: index,
                commas: 0,
            }),
            b')' | b']' | b'}' => {
                stack.pop();
            }
            b',' => {
                if let Some(open) = stack.last_mut()
                    && open.bracket == b'('
                {
                    open.commas += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let open = stack.last()?;
    (open.bracket == b'(').then_some(Call {
        open: open.start,
        argument: open.commas,
    })
}

/// The signature of whatever the name in front of the parenthesis turned out
/// to be.
fn describe(typed: &TypedProgram, target: &Target, callee: &Word) -> Option<Help> {
    match target {
        Target::Symbol(symbol, word) => declared(typed, *symbol, &word.text)
            .or_else(|| from_description(&query::describe(typed, target)?)),
        // `new User(...)`: the parameters are the fields, in declaration
        // order, and a field with a default may be left out.
        Target::Type(Type::Struct(id), _) => {
            let info = typed.structs().get(id.0)?;
            let parts: Vec<String> = info
                .fields
                .iter()
                .map(|field| {
                    let written = format!(
                        "{}: {}",
                        field.name,
                        crate::complete::type_name(typed, field.ty)
                    );
                    match field.default {
                        Some(_) => format!("{written} = ..."),
                        None => written,
                    }
                })
                .collect();
            // Fields are given by name at a construction, so no position is
            // the active one.
            Some(assemble(&info.name, &parts, "", None))
        }
        // A method on a declared type has a declaration like any function, and
        // that is the only place its parameter names are written.
        Target::Member {
            receiver: Type::Struct(id),
            word,
        } => typed
            .structs()
            .get(id.0)
            .and_then(|info| info.methods.iter().find(|method| method.name == word.text))
            .and_then(|method| declared(typed, method.id, &word.text))
            .or_else(|| from_description(&query::describe(typed, target)?)),
        // A builtin method belongs to the language; its shape is written once,
        // for completion, and read back here.
        Target::Member { .. } => from_description(&query::describe(typed, target)?),
        Target::Type(_, _) => {
            let _ = callee;
            None
        }
    }
}

/// A signature built from the declaration as it was written, which is the only
/// source that has parameter names.
fn declared(typed: &TypedProgram, symbol: SymbolId, name: &str) -> Option<Help> {
    let (file, offset) = crate::rename::declaration_key(typed, symbol)?;
    let program = &typed.program().files.get(file.0)?.program;
    let parameters = program
        .functions
        .iter()
        .chain(
            program
                .structs
                .iter()
                .flat_map(|declaration| &declaration.methods),
        )
        .find(|function| function.name.span.start == offset)
        .map(|function| &function.parameters)?;
    let parts: Vec<String> = parameters
        .iter()
        .map(|parameter| {
            format!(
                "{}: {}",
                parameter.name.text,
                type_text(&parameter.type_ref)
            )
        })
        .collect();
    let returns = match typed
        .signature(symbol)
        .map(|signature| signature.return_type)
    {
        Some(Type::Void) | None => String::new(),
        Some(ty) => format!(" -> {}", crate::complete::type_name(typed, ty)),
    };
    Some(assemble(name, &parts, &returns, Some(0)))
}

/// A signature recovered from a one-line description, which is all a prelude
/// binding or a builtin method has: neither was declared in a file.
fn from_description(description: &str) -> Option<Help> {
    let open = description.find('(')?;
    let close = description.rfind(')')?;
    if close < open {
        return None;
    }
    let inside = &description[open + 1..close];
    let parts: Vec<String> = split_arguments(inside)
        .into_iter()
        .map(str::to_owned)
        .collect();
    Some(assemble(
        &description[..open],
        &parts,
        &description[close + 1..],
        Some(0),
    ))
}

/// Split a parameter list on its top-level commas. A type may hold one of its
/// own — `Result<int, string>` is a single parameter — so the angle brackets
/// and parentheses are counted.
fn split_arguments(inside: &str) -> Vec<&str> {
    if inside.trim().is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (index, character) in inside.char_indices() {
        match character {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(inside[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(inside[start..].trim());
    parts
}

/// Put a signature together and record where each parameter landed in it.
fn assemble(name: &str, parts: &[String], suffix: &str, active: Option<usize>) -> Help {
    let mut label = format!("{name}(");
    let mut parameters = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            label.push_str(", ");
        }
        let start = label.chars().map(char::len_utf16).sum();
        label.push_str(part);
        let end = label.chars().map(char::len_utf16).sum();
        parameters.push((start, end));
    }
    label.push(')');
    label.push_str(suffix);
    Help {
        label,
        parameters,
        active,
    }
}

#[cfg(test)]
mod tests;
