//! Stage 0 of the language server: diagnostics, and the protocol needed to
//! deliver them.
//!
//! What it does is compile the file in the editor and report what the checker
//! says. What it deliberately does not do is hover, go-to-definition or
//! completion — those want the resolver's declaration and use tables, which is
//! a later stage, not a bigger version of this one.

use crate::complete;
use crate::json::Json;
use crate::query;
use crate::rpc::{self, ReadError};
use crate::text::{Positions, path_to_uri, uri_to_path};
use skuld_compiler::module::{Errors, ModuleLoader};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// JSON-RPC error codes this server can return.
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_REQUEST: i64 = -32600;

/// LSP severity: this server reports only errors, since the compiler has no
/// warning level of its own.
const SEVERITY_ERROR: f64 = 1.0;

/// Full document sync. The incremental form would mean applying ranges to a
/// buffer, which is a source of drift bugs for no gain at this size: a Skuld
/// file is small and the compiler re-reads it in microseconds.
const SYNC_FULL: f64 = 1.0;

pub struct Server {
    /// Open buffers by path, which may differ from what is on disk.
    open: BTreeMap<String, String>,
    /// The last check of each document that succeeded. Completion answers
    /// from it, because the moment a user wants a suggestion is the moment
    /// the file does not parse.
    checked: BTreeMap<String, skuld_compiler::type_checker::TypedProgram>,
    /// Set once `shutdown` arrives, so `exit` can report the right code.
    shutting_down: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            open: BTreeMap::new(),
            checked: BTreeMap::new(),
            shutting_down: false,
        }
    }

    /// Read messages until the client closes the stream or sends `exit`.
    /// Returns the process exit code.
    pub fn run(&mut self, input: &mut impl BufRead, output: &mut impl Write) -> i32 {
        loop {
            let message = match rpc::read(input) {
                Ok(message) => message,
                // A closed stream without `exit` is an abnormal end per the
                // specification, but there is nobody left to tell.
                Err(ReadError::Closed) => return if self.shutting_down { 0 } else { 1 },
                Err(ReadError::Malformed(reason)) => {
                    // The stream position is lost, so there is no resynchronising
                    // and no id to answer. Report it and stop.
                    eprintln!("skuld-lsp: {reason}");
                    return 1;
                }
            };
            if let Some(code) = self.handle(&message, output) {
                return code;
            }
        }
    }

    /// Handle one message. `Some(code)` means the server should exit.
    fn handle(&mut self, message: &Json, output: &mut impl Write) -> Option<i32> {
        let method = message.get("method").and_then(Json::as_str);
        let id = message.get("id").cloned();

        match (method, &id) {
            // A response to a request this server never sends: ignore it
            // rather than answering it.
            (None, _) => None,

            (Some("exit"), _) => Some(if self.shutting_down { 0 } else { 1 }),

            (Some("shutdown"), Some(id)) => {
                self.shutting_down = true;
                respond(output, id.clone(), Json::Null);
                None
            }

            (Some("textDocument/definition"), Some(id)) => {
                let location = self.definition(message);
                respond(output, id.clone(), location);
                None
            }

            (Some("textDocument/hover"), Some(id)) => {
                let hover = self.hover(message);
                respond(output, id.clone(), hover);
                None
            }

            (Some("textDocument/completion"), Some(id)) => {
                let items = self.completions(message);
                respond(output, id.clone(), items);
                None
            }

            (Some("initialize"), Some(id)) => {
                respond(output, id.clone(), initialize_result());
                None
            }

            // Notifications that need no reply and no work.
            (Some("initialized" | "$/setTrace"), _) => None,

            (Some("textDocument/didOpen"), _) => {
                let text = message.path(&["params", "textDocument", "text"]);
                self.update(message, text.and_then(Json::as_str), output);
                None
            }

            (Some("textDocument/didChange"), _) => {
                // Full sync: the last change carries the whole document.
                let text = message
                    .path(&["params", "contentChanges"])
                    .and_then(Json::as_array)
                    .and_then(<[Json]>::last)
                    .and_then(|change| change.get("text"))
                    .and_then(Json::as_str);
                self.update(message, text, output);
                None
            }

            (Some("textDocument/didSave"), _) => {
                // The buffer is already current; recheck so a save reflects
                // any change made to a file this one imports.
                if let Some(path) = document_path(message) {
                    self.publish(&path, output);
                }
                None
            }

            (Some("textDocument/didClose"), _) => {
                if let Some(path) = document_path(message) {
                    self.open.remove(&path);
                    // Clear what was published, or the editor keeps showing
                    // diagnostics for a file nobody has open.
                    publish_empty(output, &path);
                }
                None
            }

            // A request must be answered, even to say no; a notification with
            // an unknown method is dropped, as the specification requires.
            (Some(_), Some(id)) => {
                respond_error(
                    output,
                    id.clone(),
                    METHOD_NOT_FOUND,
                    &format!(
                        "`{}` is not implemented; this server reports diagnostics only",
                        method.unwrap_or("")
                    ),
                );
                None
            }
            (Some(_), None) => None,
        }
    }

    /// Record a document's new text and republish its diagnostics.
    fn update(&mut self, message: &Json, text: Option<&str>, output: &mut impl Write) {
        let (Some(path), Some(text)) = (document_path(message), text) else {
            return;
        };
        self.open.insert(path.clone(), text.to_string());
        // Every open document is re-checked, not only this one: a file whose
        // imported module just changed under it must not keep reporting what
        // was true before the change. A program this size checks in
        // microseconds, so the cost is not worth a dependency graph.
        self.publish_all(output);
    }

    /// Re-check every open document. Reports are sent in path order so that a
    /// file appearing in two programs settles on one answer deterministically.
    fn publish_all(&mut self, output: &mut impl Write) {
        for path in self.open.keys().cloned().collect::<Vec<_>>() {
            self.publish(&path, output);
        }
    }

    /// Compile the document and send what the checker reports.
    fn publish(&mut self, path: &str, output: &mut impl Write) {
        let Some(source) = self.open.get(path).cloned() else {
            return;
        };
        let root = Path::new(path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let name = Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());

        let mut loader = OpenFirst {
            root: root.clone(),
            open: &self.open,
        };
        let result = skuld_compiler::check_program(&name, &source, &mut loader);
        // A failed check leaves the previous good one in place: that is what
        // completion answers from while the file is mid-edit.
        let result = match result {
            Ok(typed) => {
                self.checked.insert(path.to_string(), typed);
                Ok(())
            }
            Err(errors) => Err(errors),
        };

        // Whether a program has an entrypoint is a property of the program,
        // and an editor showing one file cannot know which program that file
        // belongs to. A module file, or anything under `std/`, would otherwise
        // be permanently red for a mistake it is not making. A file that does
        // declare `main` is a program, and keeps every diagnostic about it.
        let declares_main = declares_main(&source);

        // Every file the program touched gets a report, so fixing the last
        // error in an imported module actually clears its underline. A file
        // with no diagnostics is published as an empty list, which is how LSP
        // says "nothing wrong here".
        let mut by_file: BTreeMap<String, Vec<Json>> = BTreeMap::new();
        by_file.insert(path.to_string(), Vec::new());

        if let Err(errors) = result {
            for entry in &errors.diagnostics {
                if !declares_main
                    && entry.diagnostic.code
                        == skuld_compiler::diagnostic::DiagnosticCode::InvalidEntrypoint
                {
                    continue;
                }
                let Some(file) = errors.sources.get(entry.file.0) else {
                    continue;
                };
                // `file.name` is the entry name for the root and a path
                // relative to the root directory for every other module.
                let full = if file.name == name {
                    path.to_string()
                } else {
                    root.join(&file.name).to_string_lossy().into_owned()
                };
                let positions = Positions::new(file.text.clone());
                by_file
                    .entry(full)
                    .or_default()
                    .push(diagnostic_json(&positions, &entry.diagnostic));
            }
            // A load failure with no per-file diagnostic would otherwise be
            // silent in the editor.
            if errors.diagnostics.is_empty() {
                by_file
                    .entry(path.to_string())
                    .or_default()
                    .push(whole_file_error(&errors));
            }
        }

        for (file, diagnostics) in by_file {
            publish(output, &file, Json::Array(diagnostics));
        }
    }

    /// Answer `textDocument/definition` with where a name was declared, which
    /// may be a file the editor does not have open.
    fn definition(&self, message: &Json) -> Json {
        let Some(path) = document_path(message) else {
            return Json::Null;
        };
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Json::Null;
        };
        let Some((file, span)) = query::definition(source, offset, typed) else {
            return Json::Null;
        };
        let Some(declaring) = typed.program().files.get(file.0) else {
            return Json::Null;
        };
        // A module file is named relative to the program root; the entry file
        // is named as the editor opened it.
        let target = if file.0 == 0 {
            path
        } else {
            Path::new(&path)
                .parent()
                .unwrap_or(Path::new("."))
                .join(&declaring.name)
                .to_string_lossy()
                .into_owned()
        };
        let positions = Positions::new(declaring.source.clone());
        let start = positions.position(span.start);
        let end = positions.position(span.end);
        Json::object([
            ("uri", Json::string(path_to_uri(&target))),
            (
                "range",
                Json::object([("start", position_json(start)), ("end", position_json(end))]),
            ),
        ])
    }

    /// Answer `textDocument/hover` with the declaration a reader would
    /// otherwise have to go and find.
    fn hover(&self, message: &Json) -> Json {
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Json::Null;
        };
        let Some((text, span)) = query::hover(source, offset, typed) else {
            return Json::Null;
        };
        let positions = Positions::new(source.clone());
        let start = positions.position(span.start);
        let end = positions.position(span.end);
        Json::object([
            (
                "contents",
                Json::object([
                    ("kind", Json::string("markdown")),
                    ("value", Json::string(format!("```skuld\n{text}\n```"))),
                ]),
            ),
            (
                "range",
                Json::object([("start", position_json(start)), ("end", position_json(end))]),
            ),
        ])
    }

    /// The document, the byte offset asked about, and the last good check of
    /// it — the three things every position request needs.
    fn position_context(
        &self,
        message: &Json,
    ) -> Option<(&String, usize, &skuld_compiler::type_checker::TypedProgram)> {
        let path = document_path(message)?;
        let source = self.open.get(&path)?;
        let typed = self.checked.get(&path)?;
        // A negative line or character would be a client bug; clamping to zero
        // answers at the start of the file instead of refusing.
        let line = message
            .path(&["params", "position", "line"])
            .and_then(Json::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let character = message
            .path(&["params", "position", "character"])
            .and_then(Json::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let offset =
            Positions::new(source.clone()).offset(crate::text::Position { line, character });
        Some((source, offset, typed))
    }

    /// Answer `textDocument/completion` from the last good check of the
    /// document, which may be a moment behind the text on screen.
    fn completions(&self, message: &Json) -> Json {
        let Some(path) = document_path(message) else {
            return Json::Array(Vec::new());
        };
        let Some(source) = self.open.get(&path) else {
            return Json::Array(Vec::new());
        };
        // A negative line or character would be a client bug; clamping to zero
        // answers at the start of the file instead of refusing.
        let line = message
            .path(&["params", "position", "line"])
            .and_then(Json::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let character = message
            .path(&["params", "position", "character"])
            .and_then(Json::as_i64)
            .unwrap_or(0)
            .max(0) as usize;
        let offset =
            Positions::new(source.clone()).offset(crate::text::Position { line, character });
        let items = complete::at(source, offset, self.checked.get(&path));
        Json::Array(
            items
                .into_iter()
                .map(|item| {
                    let mut fields = vec![
                        ("label", Json::string(&item.label)),
                        ("kind", Json::number(item.kind)),
                    ];
                    if let Some(detail) = &item.detail {
                        fields.push(("detail", Json::string(detail)));
                    }
                    Json::object(fields)
                })
                .collect(),
        )
    }
}

/// Where the server looks for an imported module: open buffers first, then
/// disk. An editor shows unsaved text, and diagnostics that disagreed with
/// what is on screen would be worse than none.
struct OpenFirst<'a> {
    root: PathBuf,
    open: &'a BTreeMap<String, String>,
}

impl ModuleLoader for OpenFirst<'_> {
    fn load(&mut self, path: &str) -> Result<Vec<(String, String)>, String> {
        let directory = self.root.join(path);
        let entries = std::fs::read_dir(&directory).map_err(|error| {
            format!(
                "cannot read module `{path}` at `{}`: {error}",
                directory.display()
            )
        })?;
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let file = entry.path();
            if file
                .extension()
                .is_none_or(|extension| extension != "skuld")
            {
                continue;
            }
            let name = format!("{path}/{}", entry.file_name().to_string_lossy());
            let key = file.to_string_lossy().into_owned();
            let text = match self.open.get(&key) {
                Some(open) => open.clone(),
                None => std::fs::read_to_string(&file)
                    .map_err(|error| format!("cannot read `{}`: {error}", file.display()))?,
            };
            files.push((name, text));
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(files)
    }
}

/// Whether the text declares a top-level `main`, which is what makes it an
/// entry file rather than a module the editor happens to be showing.
fn declares_main(source: &str) -> bool {
    skuld_compiler::parse(source)
        .program
        .is_some_and(|program| program.functions.iter().any(|f| f.name.text == "main"))
}

fn document_path(message: &Json) -> Option<String> {
    message
        .path(&["params", "textDocument", "uri"])
        .and_then(Json::as_str)
        .and_then(uri_to_path)
}

fn initialize_result() -> Json {
    Json::object([(
        "capabilities",
        Json::object([
            ("textDocumentSync", Json::number(SYNC_FULL)),
            // Declared explicitly: the default is utf-16 and that is what
            // `Positions` produces, so saying so keeps the two in step even
            // if a client would have preferred something else.
            ("positionEncoding", Json::string("utf-16")),
            ("hoverProvider", Json::Bool(true)),
            ("definitionProvider", Json::Bool(true)),
            (
                "completionProvider",
                Json::object([
                    // Without this the client never asks after a `.`, which is
                    // where a member list is the whole point.
                    ("triggerCharacters", Json::Array(vec![Json::string(".")])),
                    // Every item is complete when it is sent; there is nothing
                    // expensive to fill in on a second request.
                    ("resolveProvider", Json::Bool(false)),
                ]),
            ),
        ]),
    )])
}

fn diagnostic_json(
    positions: &Positions,
    diagnostic: &skuld_compiler::diagnostic::Diagnostic,
) -> Json {
    let mut start = positions.position(diagnostic.span.start);
    // A zero-width span would underline nothing, so it is widened by one unit.
    let mut end = positions.position(diagnostic.span.end.max(diagnostic.span.start + 1));
    if start == end {
        // Both offsets clamped to the same place, which happens for a span at
        // end of file. Cover the character before it where there is one, and
        // otherwise reach one unit past the end; a client clamps that to the
        // line, while an empty range highlights nothing at all. The compiler's
        // own renderer forces the same minimum width.
        if start.character > 0 {
            start.character -= 1;
        } else {
            end.character += 1;
        }
    }
    let message = match &diagnostic.help {
        Some(help) => format!("{}\n\nhelp: {help}", diagnostic.message),
        None => diagnostic.message.clone(),
    };
    Json::object([
        (
            "range",
            Json::object([("start", position_json(start)), ("end", position_json(end))]),
        ),
        ("severity", Json::number(SEVERITY_ERROR)),
        ("code", Json::string(diagnostic.code.as_str())),
        ("source", Json::string("skuld")),
        ("message", Json::string(message)),
    ])
}

/// A failure with no span of its own, reported at the start of the file.
fn whole_file_error(errors: &Errors) -> Json {
    let zero = Json::object([
        ("line", Json::number(0.0)),
        ("character", Json::number(0.0)),
    ]);
    Json::object([
        (
            "range",
            Json::object([("start", zero.clone()), ("end", zero)]),
        ),
        ("severity", Json::number(SEVERITY_ERROR)),
        ("source", Json::string("skuld")),
        ("message", Json::string(errors.render().trim_end())),
    ])
}

fn position_json(position: crate::text::Position) -> Json {
    Json::object([
        ("line", Json::number(position.line as f64)),
        ("character", Json::number(position.character as f64)),
    ])
}

fn publish(output: &mut impl Write, path: &str, diagnostics: Json) {
    notify(
        output,
        "textDocument/publishDiagnostics",
        Json::object([
            ("uri", Json::string(path_to_uri(path))),
            ("diagnostics", diagnostics),
        ]),
    );
}

fn publish_empty(output: &mut impl Write, path: &str) {
    publish(output, path, Json::Array(Vec::new()));
}

fn notify(output: &mut impl Write, method: &'static str, params: Json) {
    send(
        output,
        Json::object([
            ("jsonrpc", Json::string("2.0")),
            ("method", Json::string(method)),
            ("params", params),
        ]),
    );
}

fn respond(output: &mut impl Write, id: Json, result: Json) {
    send(
        output,
        Json::Object(
            [
                ("jsonrpc".to_string(), Json::string("2.0")),
                ("id".to_string(), id),
                ("result".to_string(), result),
            ]
            .into_iter()
            .collect(),
        ),
    );
}

fn respond_error(output: &mut impl Write, id: Json, code: i64, message: &str) {
    debug_assert!(code == METHOD_NOT_FOUND || code == INVALID_REQUEST);
    send(
        output,
        Json::Object(
            [
                ("jsonrpc".to_string(), Json::string("2.0")),
                ("id".to_string(), id),
                (
                    "error".to_string(),
                    Json::object([
                        ("code", Json::number(code as f64)),
                        ("message", Json::string(message)),
                    ]),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );
}

/// A write that fails means the client is gone; there is no channel left to
/// report that on, so it is recorded on stderr and the loop ends naturally at
/// the next read.
fn send(output: &mut impl Write, message: Json) {
    if let Err(error) = rpc::write(output, &message) {
        eprintln!("skuld-lsp: cannot write to the client: {error}");
    }
}

#[cfg(test)]
mod tests;
