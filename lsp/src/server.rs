//! Stage 0 of the language server: diagnostics, and the protocol needed to
//! deliver them.
//!
//! What it does is compile the file in the editor and report what the checker
//! says. What it deliberately does not do is hover, go-to-definition or
//! completion — those want the resolver's declaration and use tables, which is
//! a later stage, not a bigger version of this one.

use crate::complete;
use crate::folding;
use crate::hierarchy;
use crate::hints;
use crate::json::Json;
use crate::query;
use crate::rename::{self, Refusal};
use crate::rpc::{self, ReadError};
use crate::selection;
use crate::signature;
use crate::symbols::{self, Symbol};
use crate::text::{Positions, path_to_uri, uri_to_path};
use crate::tokens;
use skuld_compiler::module::{Errors, FileId, ModuleLoader};
use skuld_compiler::resolver::SymbolId;
use skuld_compiler::span::Span;
use skuld_compiler::type_checker::TypedProgram;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// JSON-RPC error codes this server can return.
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_REQUEST: i64 = -32600;

/// LSP severity: this server reports only errors, since the compiler has no
/// warning level of its own.
const SEVERITY_ERROR: f64 = 1.0;

/// The document a request names is always the entry file of the program it
/// was checked as, which is that program's first file.
const ENTRY: FileId = FileId(0);

/// How many workspace symbols one answer carries. An empty query asks for
/// everything, and a client that renders a list does not want every name in
/// every open program at once.
const WORKSPACE_SYMBOL_LIMIT: usize = 256;

/// LSP `InlayHintKind::Type`. The other kind is a parameter name at a call
/// site, which this server does not produce.
const INLAY_TYPE: f64 = 1.0;

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
    /// Ids for the requests this server sends the client. They live in the
    /// server's own numbering, which never meets the client's.
    next_request: u32,
    /// Whether the client said it understands a nested outline. The protocol's
    /// default is that it does not, and a client that cannot parse the nested
    /// form shows nothing at all, so the flat one is what silence buys.
    hierarchical_symbols: bool,
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
            next_request: 0,
            hierarchical_symbols: false,
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

            // Skuld has no forward declarations: where a function is declared
            // is where it is defined, so the two questions have one answer.
            // Answering both is what keeps a client's `gD` from reporting that
            // the server cannot do it.
            (Some("textDocument/definition" | "textDocument/declaration"), Some(id)) => {
                let location = self.definition(message);
                respond(output, id.clone(), location);
                None
            }

            (Some("textDocument/prepareCallHierarchy"), Some(id)) => {
                let items = self.prepare_call_hierarchy(message);
                respond(output, id.clone(), items);
                None
            }

            (Some("callHierarchy/incomingCalls"), Some(id)) => {
                let calls = self.call_hierarchy(message, Direction::Incoming);
                respond(output, id.clone(), calls);
                None
            }

            (Some("callHierarchy/outgoingCalls"), Some(id)) => {
                let calls = self.call_hierarchy(message, Direction::Outgoing);
                respond(output, id.clone(), calls);
                None
            }

            (Some("textDocument/typeDefinition"), Some(id)) => {
                let location = self.type_definition(message);
                respond(output, id.clone(), location);
                None
            }

            (Some("textDocument/implementation"), Some(id)) => {
                let locations = self.implementations(message);
                respond(output, id.clone(), locations);
                None
            }

            (Some("textDocument/references"), Some(id)) => {
                let locations = self.references(message);
                respond(output, id.clone(), locations);
                None
            }

            (Some("textDocument/prepareRename"), Some(id)) => {
                match self.prepare_rename(message) {
                    Ok(range) => respond(output, id.clone(), range),
                    // A refusal is the answer, not a protocol failure: the
                    // client shows it instead of offering an edit box.
                    Err(reason) => respond_error(output, id.clone(), INVALID_REQUEST, &reason),
                }
                None
            }

            (Some("textDocument/rename"), Some(id)) => {
                match self.rename(message) {
                    Ok(edit) => respond(output, id.clone(), edit),
                    Err(reason) => respond_error(output, id.clone(), INVALID_REQUEST, &reason),
                }
                None
            }

            (Some("textDocument/documentHighlight"), Some(id)) => {
                let highlights = self.document_highlights(message);
                respond(output, id.clone(), highlights);
                None
            }

            (Some("textDocument/codeAction"), Some(id)) => {
                let actions = self.code_actions(message);
                respond(output, id.clone(), actions);
                None
            }

            (Some("textDocument/inlayHint"), Some(id)) => {
                let hints = self.inlay_hints(message);
                respond(output, id.clone(), hints);
                None
            }

            (Some("textDocument/semanticTokens/full"), Some(id)) => {
                let tokens = self.semantic_tokens(message, false);
                respond(output, id.clone(), tokens);
                None
            }

            (Some("textDocument/semanticTokens/range"), Some(id)) => {
                let tokens = self.semantic_tokens(message, true);
                respond(output, id.clone(), tokens);
                None
            }

            (Some("textDocument/selectionRange"), Some(id)) => {
                let ranges = self.selection_ranges(message);
                respond(output, id.clone(), ranges);
                None
            }

            (Some("textDocument/foldingRange"), Some(id)) => {
                let ranges = self.folding_ranges(message);
                respond(output, id.clone(), ranges);
                None
            }

            (Some("textDocument/documentSymbol"), Some(id)) => {
                let symbols = self.document_symbols(message);
                respond(output, id.clone(), symbols);
                None
            }

            (Some("textDocument/formatting"), Some(id)) => {
                let edits = self.formatting(message);
                respond(output, id.clone(), edits);
                None
            }

            (Some("textDocument/hover"), Some(id)) => {
                let hover = self.hover(message);
                respond(output, id.clone(), hover);
                None
            }

            (Some("textDocument/signatureHelp"), Some(id)) => {
                let help = self.signature_help(message);
                respond(output, id.clone(), help);
                None
            }

            (Some("textDocument/completion"), Some(id)) => {
                let items = self.completions(message);
                respond(output, id.clone(), items);
                None
            }

            (Some("workspace/symbol"), Some(id)) => {
                let symbols = self.workspace_symbols(message);
                respond(output, id.clone(), symbols);
                None
            }

            (Some("initialize"), Some(id)) => {
                // What the client can read decides what some answers look
                // like, so it is recorded before the first of them is sent.
                self.hierarchical_symbols = matches!(
                    message.path(&[
                        "params",
                        "capabilities",
                        "textDocument",
                        "documentSymbol",
                        "hierarchicalDocumentSymbolSupport",
                    ]),
                    Some(Json::Bool(true))
                );
                respond(output, id.clone(), initialize_result());
                None
            }

            (Some("initialized"), _) => {
                // A watcher has to be registered; there is no way to ask for
                // one in the `initialize` result. A client that does not
                // support the registration ignores it, and the server is no
                // worse off than before.
                self.watch_skuld_files(output);
                None
            }

            (Some("workspace/didChangeWatchedFiles"), _) => {
                // Something changed on disk that the editor is not showing —
                // another tool, a branch switch, a second agent. Which file it
                // was does not matter: every open document is re-checked on
                // any change already, and this is that same refresh.
                self.publish_all(output);
                None
            }

            // Notifications that need no reply and no work.
            (Some("$/setTrace"), _) => None,

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
                    // The check goes with it: a closed document's tables
                    // describe a text nobody is looking at any more, and a
                    // reference search over every checked program would keep
                    // finding names in it.
                    self.checked.remove(&path);
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

    /// Ask the client to tell us when a `.skuld` file changes on disk.
    ///
    /// The server reads imported modules through the loader on every check, so
    /// what is missing without this is not fresh content but the trigger: a
    /// module edited outside the editor would go unnoticed until the user
    /// typed in a file that imports it.
    fn watch_skuld_files(&mut self, output: &mut impl Write) {
        self.next_request += 1;
        send(
            output,
            Json::object([
                ("jsonrpc", Json::string("2.0")),
                ("id", Json::number(self.next_request as f64)),
                ("method", Json::string("client/registerCapability")),
                (
                    "params",
                    Json::object([(
                        "registrations",
                        Json::Array(vec![Json::object([
                            ("id", Json::string("skuld-watched-files")),
                            ("method", Json::string("workspace/didChangeWatchedFiles")),
                            (
                                "registerOptions",
                                Json::object([(
                                    "watchers",
                                    Json::Array(vec![Json::object([(
                                        "globPattern",
                                        Json::string("**/*.skuld"),
                                    )])]),
                                )]),
                            ),
                        ])]),
                    )]),
                ),
            ]),
        );
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
        // The entrypoint is optional here, and that is what makes a module
        // file a first-class document: whether a program has a `main` is a
        // property of the program, and an editor showing one file cannot know
        // which program that file belongs to. Requiring one left every module
        // — and everything under `std/` — permanently unchecked, which is not
        // only a red underline: hover, completion, hints and tokens all read
        // the last successful check, and there never was one.
        let result = skuld_compiler::check_program_with(
            &name,
            &source,
            &mut loader,
            skuld_compiler::type_checker::Entrypoint::Optional,
        );
        // A failed check leaves the previous good one in place: that is what
        // completion answers from while the file is mid-edit.
        let result = match result {
            Ok(typed) => {
                self.checked.insert(path.to_string(), typed);
                Ok(())
            }
            Err(errors) => Err(errors),
        };

        // Every file the program touched gets a report, so fixing the last
        // error in an imported module actually clears its underline. A file
        // with no diagnostics is published as an empty list, which is how LSP
        // says "nothing wrong here".
        let mut by_file: BTreeMap<String, Vec<Json>> = BTreeMap::new();
        by_file.insert(path.to_string(), Vec::new());

        if let Err(errors) = result {
            for entry in &errors.diagnostics {
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
        match query::definition(source, offset, typed) {
            Some((file, span)) => self.location(&path, typed, file, span),
            None => Json::Null,
        }
    }

    /// Answer `textDocument/prepareCallHierarchy` with the function under the
    /// cursor, which is the handle the two call queries are asked about.
    fn prepare_call_hierarchy(&self, message: &Json) -> Json {
        let Some(path) = document_path(message) else {
            return Json::Null;
        };
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Json::Null;
        };
        // A call hierarchy starts at a function, whether the cursor rests on
        // its declaration or on a call to it.
        let Some(query::Target::Symbol(symbol, _)) = query::target_at(source, offset, typed) else {
            return Json::Null;
        };
        let Some((file, declared)) = rename::declaration_key(typed, symbol) else {
            return Json::Null;
        };
        let Some((_, item)) = hierarchy::declared_at(typed, file, declared) else {
            return Json::Null;
        };
        match self.hierarchy_item(&path, typed, &item) {
            Some(json) => Json::Array(vec![json]),
            None => Json::Null,
        }
    }

    /// Answer `callHierarchy/incomingCalls` or `callHierarchy/outgoingCalls`.
    ///
    /// Both walk every checked program that holds the declaring file, for the
    /// reason references does: a caller in another open program is still a
    /// caller, and a program nothing open reaches cannot be searched at all.
    fn call_hierarchy(&self, message: &Json, direction: Direction) -> Json {
        let empty = Json::Array(Vec::new());
        let Some((path, offset)) = self.call_hierarchy_key(message) else {
            return empty;
        };
        // Grouped by the declaration each call reaches, so the same function
        // found through two open programs is one entry with one set of ranges.
        let mut grouped: BTreeMap<(String, usize), CallGroup> = BTreeMap::new();
        for (document, typed) in &self.checked {
            let Some(file) = file_in(document, typed, &path) else {
                continue;
            };
            let Some(symbol) = rename::symbol_declared_at(typed, file, offset) else {
                continue;
            };
            let found = match direction {
                Direction::Incoming => hierarchy::references_to(typed, symbol)
                    .into_iter()
                    .filter_map(|(used_in, span)| {
                        hierarchy::enclosing(typed, used_in, span.start).map(|item| (item, span))
                    })
                    .collect::<Vec<_>>(),
                Direction::Outgoing => {
                    let Some((_, item)) = hierarchy::declared_at(typed, file, offset) else {
                        continue;
                    };
                    hierarchy::calls_within(typed, file, item.range)
                        .into_iter()
                        .filter_map(|(callee, span)| {
                            hierarchy::item_of(typed, callee).map(|item| (item, span))
                        })
                        .collect::<Vec<_>>()
                }
            };
            for (item, span) in found {
                let Some(json) = self.hierarchy_item(document, typed, &item) else {
                    continue;
                };
                let Some(other) = file_path(document, typed, item.file) else {
                    continue;
                };
                // A call's range is read in the file the call is written in,
                // which is the other end's file in both directions: incoming,
                // the caller's body; outgoing, the body being asked about.
                let calling = match direction {
                    Direction::Incoming => other.clone(),
                    Direction::Outgoing => path.clone(),
                };
                grouped
                    .entry((other, item.selection.start))
                    .or_insert_with(|| CallGroup {
                        item: json,
                        calling,
                        spans: BTreeSet::new(),
                    })
                    .spans
                    .insert((span.start, span.end));
            }
        }
        Json::Array(
            grouped
                .into_values()
                .filter_map(|group| {
                    let text = self.text_of(&group.calling).ok()?;
                    let positions = Positions::new(text);
                    let ranges = group
                        .spans
                        .into_iter()
                        .map(|(start, end)| range_json(&positions, Span::new(start, end)))
                        .collect();
                    Some(Json::object([
                        (direction.end(), group.item),
                        ("fromRanges", Json::Array(ranges)),
                    ]))
                })
                .collect(),
        )
    }

    /// The function a call-hierarchy request is about: the path it was found
    /// in, and the offset its name was declared at.
    ///
    /// It is recovered from the item's own uri and selection range rather than
    /// from the `data` field, so nothing depends on a client carrying an
    /// opaque value back unchanged.
    fn call_hierarchy_key(&self, message: &Json) -> Option<(String, usize)> {
        let path = message
            .path(&["params", "item", "uri"])
            .and_then(Json::as_str)
            .and_then(uri_to_path)?;
        let line = message
            .path(&["params", "item", "selectionRange", "start", "line"])
            .and_then(Json::as_i64)?
            .max(0) as usize;
        let character = message
            .path(&["params", "item", "selectionRange", "start", "character"])
            .and_then(Json::as_i64)?
            .max(0) as usize;
        let text = self.text_of(&path).ok()?;
        let offset = Positions::new(text).offset(crate::text::Position { line, character });
        Some((path, offset))
    }

    /// One end of a call, as the protocol carries it.
    fn hierarchy_item(
        &self,
        document: &str,
        typed: &TypedProgram,
        item: &hierarchy::Item,
    ) -> Option<Json> {
        let path = file_path(document, typed, item.file)?;
        let text = typed.program().files.get(item.file.0)?.source.clone();
        let positions = Positions::new(text);
        let mut fields = vec![
            ("name", Json::string(&item.name)),
            ("kind", Json::number(item.kind)),
            ("uri", Json::string(path_to_uri(&path))),
            ("range", range_json(&positions, item.range)),
            ("selectionRange", range_json(&positions, item.selection)),
        ];
        if let Some(detail) = &item.detail {
            fields.push(("detail", Json::string(detail)));
        }
        Some(Json::object(fields))
    }

    /// Answer `textDocument/typeDefinition` with where the type of the thing
    /// under the cursor was declared, which is the question definition cannot
    /// answer: on `u` in `let u = new User(...)`, definition leads to the
    /// binding and this leads to `class User`.
    fn type_definition(&self, message: &Json) -> Json {
        let Some(path) = document_path(message) else {
            return Json::Null;
        };
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Json::Null;
        };
        match query::type_definition(source, offset, typed) {
            Some((file, span)) => self.location(&path, typed, file, span),
            None => Json::Null,
        }
    }

    /// Answer `textDocument/implementation` with the classes that declare they
    /// implement the interface under the cursor — or, on one of its methods,
    /// with the bodies that implement that method.
    fn implementations(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let Some((source, offset, typed)) = self.position_context(message) else {
            return empty;
        };
        Json::Array(
            query::implementations(source, offset, typed)
                .into_iter()
                .map(|(file, span)| self.location(&path, typed, file, span))
                .filter(|location| *location != Json::Null)
                .collect(),
        )
    }

    /// A location in a file of a program, as the protocol carries one. The
    /// span is measured against the text that file was checked with, which is
    /// the text the span came from.
    fn location(&self, document: &str, typed: &TypedProgram, file: FileId, span: Span) -> Json {
        let Some(declaring) = typed.program().files.get(file.0) else {
            return Json::Null;
        };
        let Some(target) = file_path(document, typed, file) else {
            return Json::Null;
        };
        let positions = Positions::new(declaring.source.clone());
        Json::object([
            ("uri", Json::string(path_to_uri(&target))),
            ("range", range_json(&positions, span)),
        ])
    }

    /// Answer `textDocument/references` with every place a name is written,
    /// in every program the editor has checked — which is what makes it a
    /// workspace answer rather than a file one.
    fn references(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let Some((source, offset, typed)) = self.position_context(message) else {
            return empty;
        };
        let Ok((symbol, _)) = self.renameable(source, offset, typed) else {
            return empty;
        };
        // The default is to include the declaration; a client that wants only
        // the uses says so.
        let with_declaration = !matches!(
            message.path(&["params", "context", "includeDeclaration"]),
            Some(Json::Bool(false))
        );
        let Some(found) = self.all_occurrences(&path, typed, symbol) else {
            return empty;
        };
        let mut locations = Vec::new();
        for (file, spans) in &found.by_file {
            let Ok(text) = self.text_of(file) else {
                continue;
            };
            let positions = Positions::new(text);
            for span in spans {
                if !with_declaration && *file == found.declaring && span.start == found.offset {
                    continue;
                }
                locations.push(Json::object([
                    ("uri", Json::string(path_to_uri(file))),
                    ("range", range_json(&positions, *span)),
                ]));
            }
        }
        Json::Array(locations)
    }

    /// Answer `textDocument/prepareRename`: which range the client should
    /// offer to edit, or why this name will not move.
    fn prepare_rename(&self, message: &Json) -> Result<Json, String> {
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Err("this document has no successful check to rename from".to_string());
        };
        let (_, word) = self.renameable(source, offset, typed)?;
        let positions = Positions::new(source.clone());
        Ok(Json::object([
            (
                "range",
                range_json(&positions, Span::new(word.start, word.end)),
            ),
            ("placeholder", Json::string(&word.text)),
        ]))
    }

    /// Answer `textDocument/rename` with a workspace edit, or refuse.
    ///
    /// The refusal is the point of the method. Replacing the text at every
    /// recorded use is the easy half; the other half is proving that the
    /// result still means what it meant, which is done by rechecking every
    /// program the edit touches and comparing where each name resolves.
    fn rename(&self, message: &Json) -> Result<Json, String> {
        let path = document_path(message).ok_or("the request names no document")?;
        let new_name = message
            .path(&["params", "newName"])
            .and_then(Json::as_str)
            .ok_or("the request carries no new name")?
            .to_string();
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Err("this document has no successful check to rename from".to_string());
        };
        if !rename::is_identifier(&new_name) {
            return Err(format!("`{new_name}` is not an identifier"));
        }
        let (symbol, word) = self.renameable(source, offset, typed)?;
        if word.text == new_name {
            // Nothing to do, and an empty edit says so without an error.
            return Ok(Json::object([("changes", Json::Object(BTreeMap::new()))]));
        }
        let found = self
            .all_occurrences(&path, typed, symbol)
            .ok_or("this name has no declaration to rename")?;

        // The edited text of every file the rename touches, over the text
        // that was actually checked.
        let mut edited: BTreeMap<String, String> = BTreeMap::new();
        for (file, spans) in &found.by_file {
            let text = self.text_of(file)?;
            edited.insert(
                file.clone(),
                replace_all(&text, spans, &word.text, &new_name)?,
            );
        }
        let mut overlay = self.open.clone();
        for (file, text) in &edited {
            overlay.insert(file.clone(), text.clone());
        }

        // Where an offset in the old text sits in the new one: every edit
        // before it in the same file moves it by the difference in lengths.
        let delta = new_name.len() as isize - word.text.len() as isize;
        let shift = |file: &str, offset: usize| -> usize {
            let before = found.by_file.get(file).map_or(0, |spans| {
                spans.iter().filter(|span| span.start < offset).count()
            });
            (offset as isize + before as isize * delta).max(0) as usize
        };

        for (document, before) in &self.checked {
            if !found
                .by_file
                .keys()
                .any(|file| file_in(document, before, file).is_some())
            {
                continue;
            }
            let after = self
                .recheck(document, &overlay)
                .map_err(|reason| format!("renaming to `{new_name}` would not check: {reason}"))?;
            let old = rename::shape(before, &|file| file_path(document, before, file));
            let new = rename::shape(&after, &|file| file_path(document, &after, file));
            if rename::shifted(&old, &shift) != new {
                return Err(format!(
                    "renaming to `{new_name}` would change which declaration a name reaches"
                ));
            }
        }

        let mut changes = BTreeMap::new();
        for (file, spans) in &found.by_file {
            let positions = Positions::new(self.text_of(file)?);
            changes.insert(
                path_to_uri(file),
                Json::Array(
                    spans
                        .iter()
                        .map(|span| {
                            Json::object([
                                ("range", range_json(&positions, *span)),
                                ("newText", Json::string(&new_name)),
                            ])
                        })
                        .collect(),
                ),
            );
        }
        Ok(Json::object([("changes", Json::Object(changes))]))
    }

    /// The symbol at a position, when it is one this server will move. The
    /// standard library is refused here rather than in `rename`, since a name
    /// the server cannot rewrite should not be offered an edit box either.
    fn renameable(
        &self,
        source: &str,
        offset: usize,
        typed: &TypedProgram,
    ) -> Result<(SymbolId, crate::query::Word), String> {
        let (symbol, word) = rename::nameable(source, offset, typed).map_err(Refusal::message)?;
        if let Some((file, _)) = rename::declaration_key(typed, symbol)
            && library_file(typed, file)
        {
            return Err(
                "this name is declared in the standard library, which is part of the compiler"
                    .to_string(),
            );
        }
        Ok((symbol, word))
    }

    /// Every occurrence of one declaration, across every checked program that
    /// includes the file it was declared in.
    ///
    /// A program is what the editor has open: each document is compiled as
    /// the entry file of its own program, so a module's uses are found through
    /// whichever entry file reaches it. A program nothing open reaches is not
    /// searched, and cannot be: the server is told about documents, not about
    /// a directory tree.
    fn all_occurrences(&self, path: &str, typed: &TypedProgram, symbol: SymbolId) -> Option<Found> {
        let (file, offset) = rename::declaration_key(typed, symbol)?;
        let declaring = file_path(path, typed, file)?;
        let mut by_file: BTreeMap<String, Vec<Span>> = BTreeMap::new();
        for (document, other) in &self.checked {
            let Some(id) = file_in(document, other, &declaring) else {
                continue;
            };
            // The same declaration in another program is the one written at
            // the same place in the same file; symbol numbers are private to
            // each check.
            let Some(same) = rename::symbol_declared_at(other, id, offset) else {
                continue;
            };
            for occurrence in rename::occurrences(other, same) {
                if let Some(file) = file_path(document, other, occurrence.file) {
                    by_file.entry(file).or_default().push(occurrence.span);
                }
            }
        }
        for spans in by_file.values_mut() {
            spans.sort_unstable_by_key(|span| span.start);
            spans.dedup();
        }
        Some(Found {
            declaring,
            offset,
            by_file,
        })
    }

    /// The text a file was checked with, which is what the recorded offsets
    /// are offsets into. A buffer that has changed since is refused: editing
    /// it from stale positions would corrupt it.
    fn text_of(&self, file: &str) -> Result<String, String> {
        let mut checked: Option<&str> = None;
        for (document, typed) in &self.checked {
            let Some(id) = file_in(document, typed, file) else {
                continue;
            };
            let source = typed.program().files[id.0].source.as_str();
            if checked.is_some_and(|text| text != source) {
                return Err(format!(
                    "`{file}` was checked with two different texts; save it and try again"
                ));
            }
            checked = Some(source);
        }
        let checked =
            checked.ok_or_else(|| format!("`{file}` is not part of a checked program"))?;
        if self.open.get(file).is_some_and(|open| open != checked) {
            return Err(format!(
                "`{file}` has changes that have not checked; fix the errors in it and try again"
            ));
        }
        Ok(checked.to_string())
    }

    /// Check one document again over the edited texts, the way `publish`
    /// checks it over the open ones.
    fn recheck(
        &self,
        document: &str,
        overlay: &BTreeMap<String, String>,
    ) -> Result<TypedProgram, String> {
        let source = overlay
            .get(document)
            .ok_or_else(|| format!("`{document}` is not open"))?
            .clone();
        let root = Path::new(document)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let name = Path::new(document)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| document.to_string());
        let mut loader = OpenFirst {
            root,
            open: overlay,
        };
        skuld_compiler::check_program_with(
            &name,
            &source,
            &mut loader,
            skuld_compiler::type_checker::Entrypoint::Optional,
        )
        .map_err(|errors| first_message(&errors))
    }

    /// Answer `workspace/symbol` with every declaration whose name matches,
    /// across every program the editor has checked.
    ///
    /// The workspace is what is open, as it is for references: a program no
    /// open document reaches is not searched, and cannot be — the server is
    /// told about documents, not about a directory tree. The standard library
    /// is left out for a sharper reason: its files are embedded in the
    /// compiler, so their paths name nothing the editor could open.
    fn workspace_symbols(&self, message: &Json) -> Json {
        let query = message
            .path(&["params", "query"])
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_lowercase();
        let mut seen: std::collections::BTreeSet<(String, usize)> =
            std::collections::BTreeSet::new();
        let mut found = Vec::new();
        for (document, typed) in &self.checked {
            for (index, file) in typed.program().files.iter().enumerate() {
                let id = FileId(index);
                if library_file(typed, id) {
                    continue;
                }
                let Some(path) = file_path(document, typed, id) else {
                    continue;
                };
                let positions = Positions::new(file.source.clone());
                let outline = symbols::outline(&file.program);
                for (symbol, container) in symbols::flatten(&outline) {
                    // A plain case-insensitive substring, which is what a
                    // reader predicts. Clients filter and rank the result
                    // again anyway, and a fuzzy match here would only disagree
                    // with the one they apply.
                    if !symbol.name.to_lowercase().contains(&query) {
                        continue;
                    }
                    // The same file is reached through every open document
                    // that imports it, and its declarations are the same ones.
                    if !seen.insert((path.clone(), symbol.selection.start)) {
                        continue;
                    }
                    let mut fields = vec![
                        ("name", Json::string(&symbol.name)),
                        ("kind", Json::number(symbol.kind)),
                        (
                            "location",
                            Json::object([
                                ("uri", Json::string(path_to_uri(&path))),
                                ("range", range_json(&positions, symbol.range)),
                            ]),
                        ),
                    ];
                    if let Some(container) = container {
                        fields.push(("containerName", Json::string(container)));
                    }
                    found.push(Json::object(fields));
                    if found.len() >= WORKSPACE_SYMBOL_LIMIT {
                        return Json::Array(found);
                    }
                }
            }
        }
        Json::Array(found)
    }

    /// Answer `textDocument/documentHighlight` with every place this file
    /// writes the name under the cursor.
    ///
    /// It is `references` narrowed to one file and widened in what it accepts:
    /// a prelude binding and an import qualifier are highlighted even though
    /// neither can be renamed, because showing where `print` is used costs
    /// nothing and refusing it would be a surprise.
    ///
    /// No `kind` is sent. The tables record where a name is written, not
    /// whether that writing reads or assigns, and marking every use `Read`
    /// would colour `count = count + 1` wrongly on both sides.
    fn document_highlights(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some((source, offset, typed)) = self.position_context(message) else {
            return empty;
        };
        let Some(query::Target::Symbol(symbol, _)) = query::target_at(source, offset, typed) else {
            return empty;
        };
        let positions = Positions::new(source.clone());
        Json::Array(
            rename::occurrences(typed, symbol)
                .into_iter()
                // The document asked about is the entry file of its own
                // program; an occurrence in an imported module belongs to
                // another document's highlights, not to this one's.
                .filter(|occurrence| occurrence.file == ENTRY)
                .map(|occurrence| {
                    Json::object([("range", range_json(&positions, occurrence.span))])
                })
                .collect(),
        )
    }

    /// Answer `textDocument/codeAction`.
    ///
    /// One action so far: write out the type a binding leaves inferred, which
    /// is the hint made permanent. It exists as a refactor rather than as a
    /// fix because nothing is wrong with the code — a reader sometimes wants
    /// the type on the page.
    ///
    /// There are no quick fixes yet, and that is not an oversight to work
    /// around here: a fix has to come from the diagnostic that knows what went
    /// wrong. The compiler's diagnostics carry a message and a help line,
    /// both prose, and turning prose into an edit would be guessing. Giving a
    /// diagnostic a machine-applicable fix is a compiler change, and it is the
    /// right one.
    fn code_actions(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let (Some(source), Some(typed)) = (self.open.get(&path), self.checked.get(&path)) else {
            return empty;
        };
        let positions = Positions::new(source.clone());
        let window = requested_range(message, &positions, source.len());
        Json::Array(
            hints::type_hints(source, typed)
                .into_iter()
                .filter(|hint| window.contains(&hint.offset))
                .filter_map(|hint| {
                    let name = query::word_at(source, hint.offset)?;
                    let at = positions.position(hint.offset);
                    Some(Json::object([
                        (
                            "title",
                            Json::string(format!(
                                "Annotate `{}` as `{}`",
                                name.text,
                                hint.label.trim_start_matches([':', ' '])
                            )),
                        ),
                        ("kind", Json::string("refactor.rewrite")),
                        (
                            "edit",
                            Json::object([(
                                "changes",
                                Json::Object(
                                    [(
                                        path_to_uri(&path),
                                        Json::Array(vec![Json::object([
                                            (
                                                // An insertion is an edit whose
                                                // range is empty, at the point
                                                // the text goes in.
                                                "range",
                                                Json::object([
                                                    ("start", position_json(at)),
                                                    ("end", position_json(at)),
                                                ]),
                                            ),
                                            ("newText", Json::string(&hint.label)),
                                        ])]),
                                    )]
                                    .into_iter()
                                    .collect(),
                                ),
                            )]),
                        ),
                    ]))
                })
                .collect(),
        )
    }

    /// Answer `textDocument/inlayHint` with the type of every binding that
    /// does not write one.
    ///
    /// The answer comes from the last check that succeeded, like completion
    /// and hover, so a hint is a moment stale while a line is being typed
    /// rather than gone. The client asks about a range and only that range is
    /// answered: a hint outside the window would be work nobody sees.
    fn inlay_hints(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let (Some(source), Some(typed)) = (self.open.get(&path), self.checked.get(&path)) else {
            return empty;
        };
        let positions = Positions::new(source.clone());
        let window = requested_range(message, &positions, source.len());
        Json::Array(
            hints::type_hints(source, typed)
                .into_iter()
                .filter(|hint| window.contains(&hint.offset))
                .map(|hint| {
                    Json::object([
                        ("position", position_json(positions.position(hint.offset))),
                        ("label", Json::string(&hint.label)),
                        // `Type`, which is what a client dims differently from
                        // a parameter name.
                        ("kind", Json::number(INLAY_TYPE)),
                        ("paddingLeft", Json::Bool(false)),
                        ("paddingRight", Json::Bool(false)),
                    ])
                })
                .collect(),
        )
    }

    /// Answer `textDocument/semanticTokens/full` with every name the checker
    /// can classify.
    ///
    /// The editor's own highlighting stays underneath: keywords, literals and
    /// comments are the lexer's shape and a client layers these over them.
    /// What is added is the part a syntax file can only guess — that `User` is
    /// a class, `count` a binding that cannot be assigned again, `len` a
    /// method of the language rather than a name in this file.
    fn semantic_tokens(&self, message: &Json, windowed: bool) -> Json {
        let empty = Json::object([("data", Json::Array(Vec::new()))]);
        let Some(path) = document_path(message) else {
            return empty;
        };
        let (Some(source), Some(typed)) = (self.open.get(&path), self.checked.get(&path)) else {
            return empty;
        };
        let positions = Positions::new(source.clone());
        // A ranged request asks about the part of the document on screen. The
        // deltas are relative to the tokens actually sent, so the window is
        // applied before any of them is encoded.
        let window = if windowed {
            requested_range(message, &positions, source.len())
        } else {
            0..source.len() + 1
        };
        let mut data = Vec::new();
        let mut line = 0;
        let mut character = 0;
        for token in tokens::tokens(source, typed) {
            if !window.contains(&token.start) {
                continue;
            }
            let start = positions.position(token.start);
            let end = positions.position(token.end);
            // A name never holds a newline, so a token that appears to span
            // lines is a span that no longer matches the text.
            if end.line != start.line || end.character < start.character {
                continue;
            }
            let delta_line = start.line.saturating_sub(line);
            let delta_start = if delta_line == 0 {
                start.character.saturating_sub(character)
            } else {
                start.character
            };
            data.extend([
                Json::number(delta_line as f64),
                Json::number(delta_start as f64),
                Json::number((end.character - start.character) as f64),
                Json::number(f64::from(token.kind)),
                Json::number(f64::from(token.modifiers)),
            ]);
            line = start.line;
            character = start.character;
        }
        Json::object([("data", Json::Array(data))])
    }

    /// Answer `textDocument/selectionRange`: what each cursor should reach as
    /// the selection is expanded, as a chain from the word outwards.
    ///
    /// The request carries a list of positions and the answer carries one
    /// chain each, in the same order — a client with several cursors expands
    /// them together.
    fn selection_ranges(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let Some(source) = self.open.get(&path) else {
            return empty;
        };
        let Some(asked) = message
            .path(&["params", "positions"])
            .and_then(Json::as_array)
        else {
            return empty;
        };
        let positions = Positions::new(source.clone());
        Json::Array(
            asked
                .iter()
                .map(|position| {
                    let line = position
                        .get("line")
                        .and_then(Json::as_i64)
                        .unwrap_or(0)
                        .max(0);
                    let character = position
                        .get("character")
                        .and_then(Json::as_i64)
                        .unwrap_or(0)
                        .max(0);
                    let offset = positions.offset(crate::text::Position {
                        line: line as usize,
                        character: character as usize,
                    });
                    // The chain is built innermost first and nests outwards,
                    // so it is assembled from the outside in.
                    let mut built = Json::Null;
                    for span in selection::chain(source, offset).into_iter().rev() {
                        let mut fields = vec![("range", range_json(&positions, span))];
                        if built != Json::Null {
                            fields.push(("parent", built));
                        }
                        built = Json::object(fields);
                    }
                    built
                })
                .collect(),
        )
    }

    /// Answer `textDocument/foldingRange` with the runs of lines an editor may
    /// collapse.
    ///
    /// This is the one answer that needs no check at all — not even a parse.
    /// It reads the token stream, so it is right about the text on screen
    /// however broken that text is, which is what folding has to be.
    fn folding_ranges(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let Some(source) = self.open.get(&path) else {
            return empty;
        };
        let positions = Positions::new(source.clone());
        Json::Array(
            folding::folds(source, &positions)
                .into_iter()
                .map(|fold| {
                    let mut fields = vec![
                        ("startLine", Json::number(fold.start_line as f64)),
                        ("endLine", Json::number(fold.end_line as f64)),
                    ];
                    if let Some(kind) = fold.kind {
                        fields.push(("kind", Json::string(kind)));
                    }
                    Json::object(fields)
                })
                .collect(),
        )
    }

    /// Answer `textDocument/documentSymbol` with the outline of the file.
    ///
    /// The outline comes from the syntax, so it is the one answer that needs
    /// no check at all. When the text on screen does not parse, the last text
    /// that did is used instead: an outline that empties itself on every
    /// half-typed declaration is worse than one a keystroke behind.
    fn document_symbols(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let Some(source) = self.open.get(&path) else {
            return empty;
        };
        let fallback = self
            .checked
            .get(&path)
            .and_then(|typed| typed.program().files.first())
            .map(|file| file.source.clone());
        let (text, program) = match skuld_compiler::parse(source).program {
            Some(program) => (source.clone(), program),
            None => match fallback.and_then(|text| {
                skuld_compiler::parse(&text)
                    .program
                    .map(|program| (text, program))
            }) {
                Some(pair) => pair,
                None => return empty,
            },
        };
        let positions = Positions::new(text);
        let outline = symbols::outline(&program);
        if self.hierarchical_symbols {
            return Json::Array(
                outline
                    .iter()
                    .map(|symbol| symbol_json(&positions, symbol))
                    .collect(),
            );
        }
        // The flat form: every entry carries a location and names what holds
        // it, since there is no nesting left to say so.
        Json::Array(
            symbols::flatten(&outline)
                .into_iter()
                .map(|(symbol, container)| {
                    let mut fields = vec![
                        ("name", Json::string(&symbol.name)),
                        ("kind", Json::number(symbol.kind)),
                        (
                            "location",
                            Json::object([
                                ("uri", Json::string(path_to_uri(&path))),
                                ("range", range_json(&positions, symbol.range)),
                            ]),
                        ),
                    ];
                    if let Some(container) = container {
                        fields.push(("containerName", Json::string(container)));
                    }
                    Json::object(fields)
                })
                .collect(),
        )
    }

    /// Answer `textDocument/formatting` with the official formatter's output,
    /// as one edit over the whole document.
    ///
    /// A document that does not parse is answered with no edits rather than
    /// an error: the usual caller is format-on-save, and a dialog about a
    /// syntax error the editor is already underlining helps nobody. A document
    /// already formatted is answered the same way, which saves the client a
    /// no-op undo entry.
    fn formatting(&self, message: &Json) -> Json {
        let empty = Json::Array(Vec::new());
        let Some(path) = document_path(message) else {
            return empty;
        };
        let Some(source) = self.open.get(&path) else {
            return empty;
        };
        let Ok(formatted) = skuld_compiler::format_source(source) else {
            return empty;
        };
        if formatted == *source {
            return empty;
        }
        let positions = Positions::new(source.clone());
        Json::Array(vec![Json::object([
            ("range", range_json(&positions, Span::new(0, source.len()))),
            ("newText", Json::string(formatted)),
        ])])
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

    /// Answer `textDocument/signatureHelp` with the signature of the call the
    /// cursor is inside.
    ///
    /// Like completion, it reads the last check that succeeded and the text as
    /// it stands now: a call is asked about exactly while it is half-written,
    /// so the current text never parses and the tables are a moment stale.
    fn signature_help(&self, message: &Json) -> Json {
        let Some((source, offset, typed)) = self.position_context(message) else {
            return Json::Null;
        };
        let Some(help) = signature::at(source, offset, typed) else {
            return Json::Null;
        };
        let parameters = help
            .parameters
            .iter()
            .map(|&(start, end)| {
                Json::object([(
                    // A pair of offsets into the label rather than a repeated
                    // string: the client then highlights the exact run, and
                    // cannot mismatch a parameter that reads like another.
                    "label",
                    Json::Array(vec![Json::number(start as f64), Json::number(end as f64)]),
                )])
            })
            .collect();
        let mut signature = vec![
            ("label", Json::string(&help.label)),
            ("parameters", Json::Array(parameters)),
        ];
        if let Some(active) = help.active {
            signature.push(("activeParameter", Json::number(active as f64)));
        }
        Json::object([
            ("signatures", Json::Array(vec![Json::object(signature)])),
            ("activeSignature", Json::number(0.0)),
            // Skuld has no overloading, so the one signature is the active
            // one; a missing `activeParameter` means nothing is highlighted,
            // which is the honest answer for a construction by field name.
            (
                "activeParameter",
                match help.active {
                    Some(active) => Json::number(active as f64),
                    None => Json::Null,
                },
            ),
        ])
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

/// One end of a call hierarchy while it is being collected: the item itself,
/// the file the calls are written in, and where each of them is.
struct CallGroup {
    item: Json,
    calling: String,
    spans: BTreeSet<(usize, usize)>,
}

/// Which way a call hierarchy is being read. The two questions share every
/// step but the one that finds the other end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Incoming,
    Outgoing,
}

impl Direction {
    /// The field the protocol names the other end with.
    fn end(self) -> &'static str {
        match self {
            Self::Incoming => "from",
            Self::Outgoing => "to",
        }
    }
}

/// The occurrences of one declaration, and where the declaration itself is.
struct Found {
    declaring: String,
    offset: usize,
    by_file: BTreeMap<String, Vec<Span>>,
}

/// The path of a file of a program whose entry document is `document`. The
/// entry file is named as the editor opened it; every other file is named
/// relative to the program root, which is the entry file's directory.
fn file_path(document: &str, typed: &TypedProgram, file: FileId) -> Option<String> {
    let loaded = typed.program().files.get(file.0)?;
    if file.0 == 0 {
        return Some(document.to_string());
    }
    Some(
        Path::new(document)
            .parent()
            .unwrap_or(Path::new("."))
            .join(&loaded.name)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Which file of a program is the one at `path`, if it has it at all.
fn file_in(document: &str, typed: &TypedProgram, path: &str) -> Option<FileId> {
    (0..typed.program().files.len())
        .map(FileId)
        .find(|&file| file_path(document, typed, file).is_some_and(|found| found == path))
}

/// Whether a file came from the embedded standard library rather than from
/// the program root. Its path would name a directory that does not exist, and
/// its text belongs to the compiler.
fn library_file(typed: &TypedProgram, file: FileId) -> bool {
    let program = typed.program();
    program.files.get(file.0).is_some_and(|loaded| {
        program
            .modules
            .get(loaded.module.0)
            .is_some_and(|module| module.path == "std" || module.path.starts_with("std/"))
    })
}

/// Replace each span with a new name, refusing if a span does not hold the
/// old one — the last check that the offsets and the text still agree.
fn replace_all(text: &str, spans: &[Span], old: &str, new: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor
            || span.end > text.len()
            || text.get(span.start..span.end) != Some(old)
        {
            return Err("the text no longer holds the name being renamed".to_string());
        }
        out.push_str(&text[cursor..span.start]);
        out.push_str(new);
        cursor = span.end;
    }
    out.push_str(&text[cursor..]);
    Ok(out)
}

/// The first thing the compiler said, which is what a one-line refusal has
/// room for.
fn first_message(errors: &Errors) -> String {
    match errors.diagnostics.first() {
        Some(entry) => entry.diagnostic.message.clone(),
        None => errors.render().trim_end().to_string(),
    }
}

/// One outline entry, with its children. The recursion is here rather than in
/// `symbols` so that module stays free of the protocol.
fn symbol_json(positions: &Positions, symbol: &Symbol) -> Json {
    let mut fields = vec![
        ("name", Json::string(&symbol.name)),
        ("kind", Json::number(symbol.kind)),
        ("range", range_json(positions, symbol.range)),
        ("selectionRange", range_json(positions, symbol.selection)),
    ];
    if let Some(detail) = &symbol.detail {
        fields.push(("detail", Json::string(detail)));
    }
    if !symbol.children.is_empty() {
        fields.push((
            "children",
            Json::Array(
                symbol
                    .children
                    .iter()
                    .map(|child| symbol_json(positions, child))
                    .collect(),
            ),
        ));
    }
    Json::object(fields)
}

fn range_json(positions: &Positions, span: Span) -> Json {
    Json::object([
        ("start", position_json(positions.position(span.start))),
        ("end", position_json(positions.position(span.end))),
    ])
}

/// The byte range a ranged request asks about. A request that names none, or
/// names one this text cannot hold, is answered over the whole document.
fn requested_range(message: &Json, positions: &Positions, length: usize) -> std::ops::Range<usize> {
    let offset = |end: &str| -> Option<usize> {
        let line = message
            .path(&["params", "range", end, "line"])
            .and_then(Json::as_i64)?
            .max(0) as usize;
        let character = message
            .path(&["params", "range", end, "character"])
            .and_then(Json::as_i64)?
            .max(0) as usize;
        Some(positions.offset(crate::text::Position { line, character }))
    };
    match (offset("start"), offset("end")) {
        // A hint sits after a name, so the end is inclusive: a range ending
        // exactly where the hint is drawn still wants it.
        (Some(start), Some(end)) if start <= end => start..end + 1,
        _ => 0..length + 1,
    }
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
            ("documentHighlightProvider", Json::Bool(true)),
            ("workspaceSymbolProvider", Json::Bool(true)),
            // No `resolveProvider`: a hint is a short string the server
            // already had, and there is nothing to fill in on a second call.
            ("inlayHintProvider", Json::Bool(true)),
            (
                "codeActionProvider",
                Json::object([(
                    "codeActionKinds",
                    Json::Array(vec![Json::string("refactor.rewrite")]),
                )]),
            ),
            (
                "semanticTokensProvider",
                Json::object([
                    (
                        "legend",
                        Json::object([
                            (
                                "tokenTypes",
                                Json::Array(
                                    tokens::TYPES.iter().copied().map(Json::string).collect(),
                                ),
                            ),
                            (
                                "tokenModifiers",
                                Json::Array(
                                    tokens::MODIFIERS
                                        .iter()
                                        .copied()
                                        .map(Json::string)
                                        .collect(),
                                ),
                            ),
                        ]),
                    ),
                    // The whole document each time. A delta would mean keeping
                    // the previous stream per document to diff against, which
                    // is state to go stale for a file this size.
                    ("full", Json::Bool(true)),
                    // A window is answered too, since a client asks for one
                    // while a large file is still opening.
                    ("range", Json::Bool(true)),
                ]),
            ),
            ("documentSymbolProvider", Json::Bool(true)),
            ("foldingRangeProvider", Json::Bool(true)),
            ("selectionRangeProvider", Json::Bool(true)),
            // Whole-document only: the formatter reads a program, not a
            // fragment, so there is no honest answer for a range.
            ("documentFormattingProvider", Json::Bool(true)),
            ("definitionProvider", Json::Bool(true)),
            ("declarationProvider", Json::Bool(true)),
            ("typeDefinitionProvider", Json::Bool(true)),
            ("callHierarchyProvider", Json::Bool(true)),
            // Conformance is declared in Skuld, never inferred, so this
            // answers from the declarations rather than from a search for
            // classes that happen to have the methods.
            ("implementationProvider", Json::Bool(true)),
            ("referencesProvider", Json::Bool(true)),
            (
                "renameProvider",
                // Prepare first: a name this server will not move should be
                // refused before the user types a replacement, not after.
                Json::object([("prepareProvider", Json::Bool(true))]),
            ),
            (
                "signatureHelpProvider",
                Json::object([
                    // `(` opens a signature and `,` moves to the next
                    // parameter; without these the client never asks.
                    (
                        "triggerCharacters",
                        Json::Array(vec![Json::string("("), Json::string(",")]),
                    ),
                ]),
            ),
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
