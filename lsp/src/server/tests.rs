use super::*;
use crate::json;
use std::io::BufReader;

/// Drive the server with a script of messages and collect what it wrote back.
fn converse(messages: &[Json]) -> (Vec<Json>, i32) {
    let mut input = Vec::new();
    for message in messages {
        rpc::write(&mut input, message).expect("writing to a Vec cannot fail");
    }
    let mut output = Vec::new();
    let code = Server::new().run(&mut BufReader::new(&input[..]), &mut output);
    (read_all(&output), code)
}

fn read_all(bytes: &[u8]) -> Vec<Json> {
    let mut reader = BufReader::new(bytes);
    let mut messages = Vec::new();
    while let Ok(message) = rpc::read(&mut reader) {
        messages.push(message);
    }
    messages
}

fn request(id: i64, method: &str) -> Json {
    Json::Object(
        [
            ("jsonrpc".to_string(), Json::string("2.0")),
            ("id".to_string(), Json::number(id as f64)),
            ("method".to_string(), Json::string(method)),
        ]
        .into_iter()
        .collect(),
    )
}

fn did_open(path: &str, text: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("textDocument/didOpen")),
        (
            "params",
            Json::object([(
                "textDocument",
                Json::object([
                    ("uri", Json::string(path_to_uri(path))),
                    ("text", Json::string(text)),
                ]),
            )]),
        ),
    ])
}

/// The diagnostics published for `path`, from the last report about it.
fn diagnostics_for<'a>(messages: &'a [Json], path: &str) -> &'a [Json] {
    let uri = path_to_uri(path);
    messages
        .iter()
        .rfind(|message| {
            message.get("method").and_then(Json::as_str) == Some("textDocument/publishDiagnostics")
                && message.path(&["params", "uri"]).and_then(Json::as_str) == Some(uri.as_str())
        })
        .and_then(|message| message.path(&["params", "diagnostics"]))
        .and_then(Json::as_array)
        .expect("a report for this file")
}

#[test]
fn answers_initialize_with_its_capabilities() {
    let (out, _) = converse(&[request(1, "initialize")]);
    let result = out[0]
        .path(&["result", "capabilities"])
        .expect("capabilities");
    assert_eq!(out[0].get("id").unwrap().as_i64(), Some(1));
    assert_eq!(result.get("textDocumentSync").unwrap().as_i64(), Some(1));
    // Declared because `Positions` counts UTF-16; a mismatch here misplaces
    // every diagnostic on a line with an astral character.
    assert_eq!(
        result.get("positionEncoding").and_then(Json::as_str),
        Some("utf-16")
    );
}

#[test]
fn shutdown_then_exit_is_a_clean_end() {
    let (out, code) = converse(&[request(1, "shutdown"), request(2, "exit")]);
    assert_eq!(code, 0);
    assert_eq!(out[0].get("result"), Some(&Json::Null));
}

#[test]
fn exit_without_shutdown_reports_an_abnormal_end() {
    let (_, code) = converse(&[request(1, "exit")]);
    assert_eq!(code, 1);
}

#[test]
fn a_closed_stream_without_shutdown_is_abnormal() {
    let (_, code) = converse(&[request(1, "initialize")]);
    assert_eq!(code, 1);
}

#[test]
fn a_valid_file_publishes_an_empty_list() {
    // Not "no message": an empty list is how LSP clears an editor's gutter.
    let (out, _) = converse(&[did_open(
        "/tmp/ok.skuld",
        "func main() {\n    print(1)\n}\n",
    )]);
    assert!(diagnostics_for(&out, "/tmp/ok.skuld").is_empty());
}

#[test]
fn reports_a_type_error_with_its_code_and_range() {
    let source = "func main() {\n    let x: int = \"text\"\n}\n";
    let (out, _) = converse(&[did_open("/tmp/bad.skuld", source)]);
    let reported = diagnostics_for(&out, "/tmp/bad.skuld");
    assert_eq!(reported.len(), 1, "expected one diagnostic: {reported:?}");
    let first = &reported[0];
    assert_eq!(first.get("source").and_then(Json::as_str), Some("skuld"));
    assert_eq!(first.get("severity").unwrap().as_i64(), Some(1));
    let code = first.get("code").and_then(Json::as_str).expect("a code");
    assert!(code.starts_with('E'), "expected an E-code, got {code}");
    // The error is on the second line, where the string literal is.
    assert_eq!(
        first.path(&["range", "start", "line"]).unwrap().as_i64(),
        Some(1)
    );
}

#[test]
fn a_range_is_never_empty() {
    // A zero-width range underlines nothing in the editor.
    let (out, _) = converse(&[did_open("/tmp/empty.skuld", "func main() {\n")]);
    for reported in diagnostics_for(&out, "/tmp/empty.skuld") {
        let start = reported.path(&["range", "start"]).unwrap();
        let end = reported.path(&["range", "end"]).unwrap();
        assert_ne!(start, end, "zero-width range in {reported:?}");
    }
}

#[test]
fn a_change_replaces_the_previous_report() {
    let broken = did_open(
        "/tmp/fix.skuld",
        "func main() {\n    let x: int = \"s\"\n}\n",
    );
    let fixed = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("textDocument/didChange")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri("/tmp/fix.skuld")))]),
                ),
                (
                    "contentChanges",
                    Json::Array(vec![Json::object([(
                        "text",
                        Json::string("func main() {\n    let x: int = 1\n    print(x)\n}\n"),
                    )])]),
                ),
            ]),
        ),
    ]);
    let (out, _) = converse(&[broken, fixed]);
    assert!(
        diagnostics_for(&out, "/tmp/fix.skuld").is_empty(),
        "the corrected file should clear its diagnostics"
    );
}

#[test]
fn closing_a_document_clears_its_diagnostics() {
    let open = did_open(
        "/tmp/close.skuld",
        "func main() {\n    let x: int = \"s\"\n}\n",
    );
    let close = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("textDocument/didClose")),
        (
            "params",
            Json::object([(
                "textDocument",
                Json::object([("uri", Json::string(path_to_uri("/tmp/close.skuld")))]),
            )]),
        ),
    ]);
    let (out, _) = converse(&[open, close]);
    assert!(diagnostics_for(&out, "/tmp/close.skuld").is_empty());
}

#[test]
fn an_unknown_request_is_refused_but_an_unknown_notification_is_not() {
    let notification = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("textDocument/inventedNotification")),
    ]);
    // `hover`, `references`, `rename`, the outline and formatting are all
    // answered now, so the refused request has to be one the server genuinely
    // does not implement.
    let (out, _) = converse(&[request(7, "textDocument/inventedRequest"), notification]);
    assert_eq!(out.len(), 1, "a notification must not be answered");
    assert_eq!(out[0].get("id").unwrap().as_i64(), Some(7));
    assert_eq!(
        out[0].path(&["error", "code"]).unwrap().as_i64(),
        Some(METHOD_NOT_FOUND)
    );
}

#[test]
fn a_response_from_the_client_is_ignored() {
    // A message with an id and no method is a response; answering it would
    // start a loop.
    let response = Json::Object(
        [
            ("jsonrpc".to_string(), Json::string("2.0")),
            ("id".to_string(), Json::number(1.0)),
            ("result".to_string(), Json::Null),
        ]
        .into_iter()
        .collect(),
    );
    let (out, _) = converse(&[response]);
    assert!(out.is_empty());
}

#[test]
fn a_document_that_is_not_a_local_file_is_ignored() {
    // An `untitled:` buffer has no path to compile or root to import from.
    let open = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("textDocument/didOpen")),
        (
            "params",
            Json::object([(
                "textDocument",
                Json::object([
                    ("uri", Json::string("untitled:Untitled-1")),
                    ("text", Json::string("func main() {}")),
                ]),
            )]),
        ),
    ]);
    let (out, _) = converse(&[open]);
    assert!(out.is_empty());
}

#[test]
fn a_malformed_message_ends_the_session_rather_than_guessing() {
    // The stream position is lost after a bad frame; continuing would report
    // nonsense against the wrong file.
    let mut input = Vec::new();
    input.extend_from_slice(b"Content-Length: 9\r\n\r\n{not json");
    let mut output = Vec::new();
    let code = Server::new().run(&mut BufReader::new(&input[..]), &mut output);
    assert_eq!(code, 1);
    assert!(output.is_empty());
}

#[test]
fn a_diagnostic_message_carries_the_help_text() {
    // The compiler's `help` is the most useful half of several diagnostics,
    // and LSP has nowhere else to put it.
    // A misplaced `import` is one of the diagnostics that carries help; the
    // invalid-character error, for instance, does not.
    let source = "func main() {}\nimport \"json\"\n";
    let (out, _) = converse(&[did_open("/tmp/help.skuld", source)]);
    let reported = diagnostics_for(&out, "/tmp/help.skuld");
    assert!(!reported.is_empty());
    let has_help = reported.iter().any(|d| {
        d.get("message")
            .and_then(Json::as_str)
            .is_some_and(|m| m.contains("help:"))
    });
    assert!(has_help, "expected help text in {reported:?}");
}

#[test]
fn json_survives_the_full_round_trip() {
    // Guards the framing against a body length counted in characters.
    let (out, _) = converse(&[did_open(
        "/tmp/uni.skuld",
        "func main() {\n    print(\"olá 😀\")\n}\n",
    )]);
    assert!(!out.is_empty());
    assert!(json::parse(&out[0].to_text()).is_ok());
}

#[test]
fn a_file_that_declares_no_main_is_not_judged_as_a_program() {
    // Every module file, and every file under `std/`, is a file the editor
    // shows and no program's entry. Reporting a missing entrypoint against it
    // would leave the whole library permanently red.
    let (out, _) = converse(&[did_open(
        "/tmp/skuld-lsp-test/geometry/point.skuld",
        "pub struct Point {\n    x: int\n    y: int\n}\n",
    )]);
    assert!(
        diagnostics_for(&out, "/tmp/skuld-lsp-test/geometry/point.skuld").is_empty(),
        "{out:?}"
    );
}

#[test]
fn a_broken_main_is_still_reported() {
    // Suppression applies to a file that declares no `main` at all; one that
    // declares a wrong `main` is a program with a real mistake in it.
    let (out, _) = converse(&[did_open(
        "/tmp/skuld-lsp-test/main.skuld",
        "func main(n: int) {\n}\n",
    )]);
    assert_eq!(
        diagnostics_for(&out, "/tmp/skuld-lsp-test/main.skuld").len(),
        1,
        "{out:?}"
    );
}

/// A path inside the repository's own fixtures, so a module on disk can be
/// imported without inventing a temporary tree.
fn fixture_path(name: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("tests")
        .join("pass");
    root.join(name).to_string_lossy().into_owned()
}

#[test]
fn editing_a_module_refreshes_the_files_that_import_it() {
    // The editor shows one program; a file whose dependency changed under it
    // must not keep reporting what was true before the change.
    let entry = fixture_path("lsp_entry.skuld");
    let module = fixture_path("modules/geometry/point.skuld");
    let (out, _) = converse(&[
        did_open(
            &entry,
            "import \"modules/geometry\"\nfunc main() { print(geometry.origin().sum()) }\n",
        ),
        // The same module, opened unsaved with `pub` taken off `origin`, which
        // is the name the entry file calls.
        did_open(
            &module,
            "pub struct Point {\n    x: int\n    y: int\n\n    sum() -> int {\n        return this.x + this.y\n    }\n}\n\npub enum Shape {\n    Dot\n    Box(Point)\n}\n\nfunc origin() -> Point {\n    return Point { x: 0, y: 0 }\n}\n\nfunc scale(value: int) -> int {\n    return value * 2\n}\n",
        ),
    ]);
    assert!(
        !diagnostics_for(&out, &entry).is_empty(),
        "the importing file was never re-checked: {out:?}"
    );
}

fn did_change(path: &str, text: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("textDocument/didChange")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "contentChanges",
                    Json::Array(vec![Json::object([("text", Json::string(text))])]),
                ),
            ]),
        ),
    ])
}

fn completion_at(path: &str, line: i64, character: i64) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(7.0)),
        ("method", Json::string("textDocument/completion")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "position",
                    Json::object([
                        ("line", Json::number(line as f64)),
                        ("character", Json::number(character as f64)),
                    ]),
                ),
            ]),
        ),
    ])
}

/// The labels of the completion response, in the order they were sent.
fn completion_labels(messages: &[Json]) -> Vec<String> {
    messages
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(7))
        .and_then(|message| message.get("result"))
        .and_then(Json::as_array)
        .expect("a completion response")
        .iter()
        .filter_map(|item| item.get("label").and_then(Json::as_str))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_server_advertises_and_answers_completion() {
    let path = "/tmp/skuld-lsp-test/enum.skuld";
    // The file as it last checked, then the same file with `Command.` being
    // typed into it — which does not parse, as a half-written line rarely does.
    let checked = "enum Command {\n    Quit\n    Echo(string)\n}\nfunc main() {\n}\n";
    let typing = "enum Command {\n    Quit\n    Echo(string)\n}\nfunc main() {\n    Command.\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, checked),
        did_change(path, typing),
        // Line 5 is `    Command.`; the cursor sits after the dot.
        completion_at(path, 5, 12),
    ]);
    let capabilities = out[0].path(&["result", "capabilities"]).expect("result");
    assert!(
        capabilities
            .path(&["completionProvider", "triggerCharacters"])
            .and_then(Json::as_array)
            .is_some_and(|characters| characters.iter().any(|c| c.as_str() == Some("."))),
        "a client only asks after a `.` if the server says to: {capabilities:?}"
    );
    assert_eq!(completion_labels(&out), ["Quit", "Echo"]);
}

#[test]
fn completion_answers_from_the_last_good_check_while_the_file_is_broken() {
    // The moment a user wants a suggestion is the moment the file does not
    // parse, so an unparseable buffer must not empty the list.
    let path = "/tmp/skuld-lsp-test/broken.skuld";
    let good = "class User {\n    name: string\n}\nfunc main() {\n    let user = new User(name: \"Ada\")\n    print(user.name)\n}\n";
    let broken = format!("{good}func (");
    let (out, _) = converse(&[
        did_open(path, good),
        did_change(path, &broken),
        // Still on line 5, `    print(user.name)`, after the dot.
        completion_at(path, 5, 15),
    ]);
    assert_eq!(completion_labels(&out), ["name"]);
}

fn hover_at(path: &str, line: i64, character: i64) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(9.0)),
        ("method", Json::string("textDocument/hover")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "position",
                    Json::object([
                        ("line", Json::number(line as f64)),
                        ("character", Json::number(character as f64)),
                    ]),
                ),
            ]),
        ),
    ])
}

#[test]
fn the_server_advertises_and_answers_hover() {
    let path = "/tmp/skuld-lsp-test/hover.skuld";
    let source = "func area(w: int, h: int) -> int {\n    return w * h\n}\nfunc main() {\n    print(area(2, 3))\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        // Line 4 is `    print(area(2, 3))`; the cursor rests inside `area`.
        hover_at(path, 4, 12),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "hoverProvider"]),
        Some(&Json::Bool(true))
    );
    let hover = out
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(9))
        .and_then(|message| message.get("result"))
        .expect("a hover response");
    assert_eq!(
        hover.path(&["contents", "value"]).and_then(Json::as_str),
        Some("```skuld\nfunc area(int, int) -> int\n```")
    );
    // The range covers the word asked about, so a client can highlight it.
    assert_eq!(
        hover
            .path(&["range", "start", "character"])
            .and_then(Json::as_i64),
        Some(10)
    );
    assert_eq!(
        hover
            .path(&["range", "end", "character"])
            .and_then(Json::as_i64),
        Some(14)
    );
}

#[test]
fn hover_over_nothing_answers_null_rather_than_an_error() {
    let path = "/tmp/skuld-lsp-test/blank.skuld";
    let (out, _) = converse(&[did_open(path, "func main() {\n}\n"), hover_at(path, 1, 0)]);
    let hover = out
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(9))
        .and_then(|message| message.get("result"))
        .expect("a hover response");
    assert_eq!(*hover, Json::Null);
}

fn definition_at(path: &str, line: i64, character: i64) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(11.0)),
        ("method", Json::string("textDocument/definition")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "position",
                    Json::object([
                        ("line", Json::number(line as f64)),
                        ("character", Json::number(character as f64)),
                    ]),
                ),
            ]),
        ),
    ])
}

#[test]
fn definition_crosses_into_the_module_that_declared_the_name() {
    // The point of a module system, from an editor: following a qualified name
    // into a file the editor never opened.
    let entry = fixture_path("lsp_definition.skuld");
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(
            &entry,
            "import \"modules/geometry\"\nfunc main() {\n    print(geometry.origin().sum())\n}\n",
        ),
        // Line 2, inside `origin`.
        definition_at(&entry, 2, 22),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "definitionProvider"]),
        Some(&Json::Bool(true))
    );
    let location = out
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(11))
        .and_then(|message| message.get("result"))
        .expect("a definition response");
    let uri = location.get("uri").and_then(Json::as_str).expect("a uri");
    assert!(
        uri.ends_with("/tests/pass/modules/geometry/point.skuld"),
        "expected the module file, got {uri}"
    );
    // `pub func origin()` is on line 17 of that file, zero-based.
    assert_eq!(
        location
            .path(&["range", "start", "line"])
            .and_then(Json::as_i64),
        Some(17)
    );
}

/// A position request with a method and an id of its own, which is how the
/// reference and rename tests find their answer in the stream.
fn position_request(id: i64, method: &str, path: &str, line: i64, character: i64) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(id as f64)),
        ("method", Json::string(method)),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "position",
                    Json::object([
                        ("line", Json::number(line as f64)),
                        ("character", Json::number(character as f64)),
                    ]),
                ),
            ]),
        ),
    ])
}

fn references_at(id: i64, path: &str, line: i64, character: i64, declaration: bool) -> Json {
    let mut message = position_request(id, "textDocument/references", path, line, character);
    if let Json::Object(members) = &mut message
        && let Some(Json::Object(params)) = members.get_mut("params")
    {
        params.insert(
            "context".to_string(),
            Json::object([("includeDeclaration", Json::Bool(declaration))]),
        );
    }
    message
}

fn rename_at(id: i64, path: &str, line: i64, character: i64, new_name: &str) -> Json {
    let mut message = position_request(id, "textDocument/rename", path, line, character);
    if let Json::Object(members) = &mut message
        && let Some(Json::Object(params)) = members.get_mut("params")
    {
        params.insert("newName".to_string(), Json::string(new_name));
    }
    message
}

/// The result of the response with that id.
fn result(messages: &[Json], id: i64) -> &Json {
    answer(messages, id)
        .get("result")
        .expect("a response with a result")
}

fn answer(messages: &[Json], id: i64) -> &Json {
    messages
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(id))
        .expect("a response with that id")
}

/// The edits of a workspace edit, as `(path suffix, line, character, text)`,
/// so an assertion does not depend on the temporary root.
fn edits(result: &Json) -> Vec<(String, i64, i64, String)> {
    let Some(Json::Object(changes)) = result.get("changes") else {
        panic!("a workspace edit with changes, got {result:?}");
    };
    let mut all = Vec::new();
    for (uri, list) in changes {
        for edit in list.as_array().expect("a list of edits") {
            all.push((
                uri.clone(),
                edit.path(&["range", "start", "line"])
                    .and_then(Json::as_i64)
                    .expect("a line"),
                edit.path(&["range", "start", "character"])
                    .and_then(Json::as_i64)
                    .expect("a character"),
                edit.get("newText")
                    .and_then(Json::as_str)
                    .expect("the new text")
                    .to_string(),
            ));
        }
    }
    all.sort();
    all
}

const CALLS: &str = "\
func area(width: int, height: int) -> int {
    return width * height
}

func main() {
    print(area(2, 3))
    print(area(4, 5))
}
";

#[test]
fn the_server_advertises_references_and_rename() {
    let (out, _) = converse(&[request(1, "initialize")]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "referencesProvider"]),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        out[0].path(&[
            "result",
            "capabilities",
            "renameProvider",
            "prepareProvider"
        ]),
        Some(&Json::Bool(true))
    );
}

#[test]
fn references_report_the_declaration_and_every_use() {
    let path = fixture_path("lsp_references.skuld");
    // Line 5, inside the first call to `area`.
    let (out, _) = converse(&[
        did_open(&path, CALLS),
        references_at(20, &path, 5, 11, true),
        references_at(21, &path, 5, 11, false),
    ]);
    let with = result(&out, 20).as_array().expect("locations").to_vec();
    let lines: Vec<i64> = with
        .iter()
        .map(|location| {
            location
                .path(&["range", "start", "line"])
                .and_then(Json::as_i64)
                .expect("a line")
        })
        .collect();
    assert_eq!(lines, vec![0, 5, 6]);
    let without = result(&out, 21).as_array().expect("locations").len();
    assert_eq!(without, 2, "the declaration should have been left out");
}

#[test]
fn renaming_a_local_rewrites_its_declaration_and_its_uses() {
    let path = fixture_path("lsp_rename_local.skuld");
    let source = "func main() {\n    let total = 2\n    print(total + total)\n}\n";
    let (out, _) = converse(&[
        did_open(&path, source),
        // Line 1, inside `total`.
        rename_at(22, &path, 1, 10, "sum"),
    ]);
    let found = edits(result(&out, 22));
    assert_eq!(
        found
            .iter()
            .map(|(_, line, character, text)| (*line, *character, text.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, 8, "sum"), (2, 10, "sum"), (2, 18, "sum")]
    );
}

#[test]
fn a_rename_refuses_a_name_that_is_not_an_identifier() {
    let path = fixture_path("lsp_rename_invalid.skuld");
    let (out, _) = converse(&[did_open(&path, CALLS), rename_at(23, &path, 0, 6, "func")]);
    let message = answer(&out, 23)
        .path(&["error", "message"])
        .and_then(Json::as_str)
        .expect("a refusal");
    assert!(message.contains("not an identifier"), "{message}");
}

#[test]
fn a_rename_refuses_a_collision_the_checker_would_report() {
    let path = fixture_path("lsp_rename_collision.skuld");
    let source = "func main() {\n    let total = 2\n    let sum = 3\n    print(total + sum)\n}\n";
    let (out, _) = converse(&[did_open(&path, source), rename_at(24, &path, 1, 10, "sum")]);
    assert!(
        answer(&out, 24).get("error").is_some(),
        "a duplicate declaration should have been refused: {:?}",
        answer(&out, 24)
    );
}

#[test]
fn a_rename_refuses_a_capture_that_would_still_compile() {
    // Renaming `outer` to `inner` leaves a program that checks perfectly well
    // and means something else: the print inside the block would read the
    // block's own binding. Nothing but comparing where each name resolves
    // catches this.
    let path = fixture_path("lsp_rename_capture.skuld");
    let source = "\
func main() {
    let outer = 1
    if true {
        let inner = 2
        print(outer + inner)
    }
}
";
    let (out, _) = converse(&[did_open(&path, source), rename_at(25, &path, 1, 9, "inner")]);
    let message = answer(&out, 25)
        .path(&["error", "message"])
        .and_then(Json::as_str)
        .expect("a refusal");
    assert!(
        message.contains("which declaration a name reaches"),
        "{message}"
    );
}

#[test]
fn prepare_rename_offers_the_word_and_refuses_the_prelude() {
    let path = fixture_path("lsp_prepare_rename.skuld");
    let (out, _) = converse(&[
        did_open(&path, CALLS),
        // Line 5, inside `area`.
        position_request(26, "textDocument/prepareRename", &path, 5, 11),
        // Line 5, inside `print`.
        position_request(27, "textDocument/prepareRename", &path, 5, 6),
    ]);
    let range = result(&out, 26);
    assert_eq!(
        range.get("placeholder").and_then(Json::as_str),
        Some("area")
    );
    assert_eq!(
        range
            .path(&["range", "start", "character"])
            .and_then(Json::as_i64),
        Some(10)
    );
    let message = answer(&out, 27)
        .path(&["error", "message"])
        .and_then(Json::as_str)
        .expect("a refusal");
    assert!(message.contains("prelude"), "{message}");
}

#[test]
fn renaming_an_exported_name_crosses_every_file_of_the_program() {
    // The milestone's marker: a name declared in a module, used from the file
    // that imports it, renamed in both at once.
    let entry = fixture_path("lsp_rename_module.skuld");
    let module = fixture_path("modules/geometry/point.skuld");
    let entry_source =
        "import \"modules/geometry\"\nfunc main() {\n    print(geometry.origin().sum())\n}\n";
    let (out, _) = converse(&[
        did_open(&entry, entry_source),
        // Line 2, inside the qualified `origin`.
        rename_at(28, &entry, 2, 22, "start"),
    ]);
    let found = edits(result(&out, 28));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(
        |(uri, line, _, text)| uri.ends_with("/lsp_rename_module.skuld")
            && *line == 2
            && text == "start"
    ));
    assert!(
        found.iter().any(
            |(uri, line, _, text)| uri.ends_with("/modules/geometry/point.skuld")
                && *line == 17
                && text == "start"
        ),
        "{found:?}"
    );
    // Nothing was written: a workspace edit is the client's to apply.
    let on_disk = std::fs::read_to_string(&module).expect("the fixture is still there");
    assert!(on_disk.contains("pub func origin()"));
}

#[test]
fn a_rename_refuses_a_buffer_that_has_not_checked_since_it_changed() {
    let path = fixture_path("lsp_rename_stale.skuld");
    let (out, _) = converse(&[
        did_open(&path, CALLS),
        // Break the file: the last good check is now a text nobody has.
        // The same lines, with an unfinished one added: the cursor still
        // rests on `area`, and the last good check is now a text nobody has.
        did_change(&path, &format!("{CALLS}func ")),
        rename_at(29, &path, 5, 11, "size"),
    ]);
    let message = answer(&out, 29)
        .path(&["error", "message"])
        .and_then(Json::as_str)
        .expect("a refusal");
    assert!(message.contains("have not checked"), "{message}");
}

#[test]
fn a_name_from_the_standard_library_is_not_renameable() {
    let path = fixture_path("lsp_rename_std.skuld");
    let source =
        "import \"std/strings\"\nfunc main() {\n    print(strings.contains(\"ab\", \"a\"))\n}\n";
    let (out, _) = converse(&[
        did_open(&path, source),
        // Line 2, inside `contains`.
        position_request(30, "textDocument/prepareRename", &path, 2, 22),
    ]);
    let message = answer(&out, 30)
        .path(&["error", "message"])
        .and_then(Json::as_str)
        .expect("a refusal");
    assert!(message.contains("standard library"), "{message}");
}

#[test]
fn an_edit_is_positioned_in_utf16_units_like_every_other_answer() {
    // The line holds an astral character, which is one scalar and two UTF-16
    // units: a client counting the compiler's bytes would edit the wrong span.
    let path = fixture_path("lsp_rename_unicode.skuld");
    let source = "func main() {\n    let total = 1 // 🌍 comment\n    print(total)\n}\n";
    let (out, _) = converse(&[did_open(&path, source), rename_at(31, &path, 1, 10, "sum")]);
    let found = edits(result(&out, 31));
    assert_eq!(
        found
            .iter()
            .map(|(_, line, character, _)| (*line, *character))
            .collect::<Vec<_>>(),
        vec![(1, 8), (2, 10)]
    );
}

#[test]
fn a_document_that_never_checked_is_refused_rather_than_guessed_at() {
    let path = fixture_path("lsp_rename_broken.skuld");
    let (out, _) = converse(&[
        did_open(&path, "func main( {\n    let total = 1\n}\n"),
        rename_at(32, &path, 1, 10, "sum"),
    ]);
    let message = answer(&out, 32)
        .path(&["error", "message"])
        .and_then(Json::as_str)
        .expect("a refusal");
    assert!(message.contains("no successful check"), "{message}");
}

/// A request naming a document and nothing else, which is the shape of both
/// `documentSymbol` and `formatting`.
fn document_request(id: i64, method: &str, path: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(id as f64)),
        ("method", Json::string(method)),
        (
            "params",
            Json::object([(
                "textDocument",
                Json::object([("uri", Json::string(path_to_uri(path)))]),
            )]),
        ),
    ])
}

fn result_of(messages: &[Json], id: i64) -> &Json {
    messages
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(id))
        .and_then(|message| message.get("result"))
        .expect("a response for this request")
}

#[test]
fn the_server_advertises_and_answers_the_outline() {
    let path = "/tmp/skuld-lsp-test/outline.skuld";
    let source = "class User {\n    name: string\n    hello() {\n        print(this.name)\n    }\n}\n\nfunc main() {\n    let u = new User(name: \"a\")\n    u.hello()\n}\n";
    let (out, _) = converse(&[
        initialize_with_hierarchical_symbols(1),
        did_open(path, source),
        document_request(2, "textDocument/documentSymbol", path),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "documentSymbolProvider"]),
        Some(&Json::Bool(true))
    );
    let symbols = result_of(&out, 2).as_array().expect("an outline");
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].get("name").and_then(Json::as_str), Some("User"));
    assert_eq!(symbols[0].get("kind").and_then(Json::as_i64), Some(5));
    // The class starts on line 0 and the name sits inside that range.
    assert_eq!(
        symbols[0]
            .path(&["selectionRange", "start", "character"])
            .and_then(Json::as_i64),
        Some(6)
    );
    let children = symbols[0]
        .get("children")
        .and_then(Json::as_array)
        .expect("a field and a method");
    assert_eq!(children.len(), 2);
    assert_eq!(
        children[1].get("name").and_then(Json::as_str),
        Some("hello")
    );
    assert_eq!(symbols[1].get("name").and_then(Json::as_str), Some("main"));
}

#[test]
fn the_outline_survives_a_document_that_stopped_parsing() {
    // The outline is what a client draws while the user types, so it falls
    // back to the last text that parsed rather than emptying itself.
    let path = "/tmp/skuld-lsp-test/half-typed.skuld";
    let (out, _) = converse(&[
        initialize_with_hierarchical_symbols(1),
        did_open(path, "func main() {\n}\n"),
        did_change(path, "func main() {\n}\nfunc half("),
        document_request(2, "textDocument/documentSymbol", path),
    ]);
    let symbols = result_of(&out, 2).as_array().expect("an outline");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].get("name").and_then(Json::as_str), Some("main"));
}

#[test]
fn an_outline_of_an_unopened_document_is_empty_rather_than_an_error() {
    let (out, _) = converse(&[document_request(
        2,
        "textDocument/documentSymbol",
        "/tmp/skuld-lsp-test/never-opened.skuld",
    )]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

#[test]
fn the_server_advertises_and_answers_formatting() {
    let path = "/tmp/skuld-lsp-test/unformatted.skuld";
    let source = "func main(){\nprint(\"hi\")\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        document_request(2, "textDocument/formatting", path),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "documentFormattingProvider"]),
        Some(&Json::Bool(true))
    );
    let edits = result_of(&out, 2).as_array().expect("an edit list");
    assert_eq!(edits.len(), 1, "the formatter replaces the whole document");
    let text = edits[0]
        .get("newText")
        .and_then(Json::as_str)
        .expect("the formatted text");
    assert_eq!(text, skuld_compiler::format_source(source).unwrap());
    assert_ne!(text, source);
    // The range starts at the top of the file and ends past its last line.
    assert_eq!(
        edits[0]
            .path(&["range", "start", "line"])
            .and_then(Json::as_i64),
        Some(0)
    );
    assert_eq!(
        edits[0]
            .path(&["range", "end", "line"])
            .and_then(Json::as_i64),
        Some(3)
    );
}

#[test]
fn formatting_an_already_formatted_document_edits_nothing() {
    let path = "/tmp/skuld-lsp-test/formatted.skuld";
    let source = skuld_compiler::format_source("func main() {\n    print(\"hi\")\n}\n").unwrap();
    let (out, _) = converse(&[
        did_open(path, &source),
        document_request(2, "textDocument/formatting", path),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

#[test]
fn formatting_a_document_that_does_not_parse_edits_nothing() {
    // Format-on-save must not raise a dialog about a syntax error the editor
    // is already underlining.
    let path = "/tmp/skuld-lsp-test/broken.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main() {\n    print(\n"),
        document_request(2, "textDocument/formatting", path),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

/// The `(line, character)` each range in a response starts at.
fn starts_of(result: &Json) -> Vec<(i64, i64)> {
    result
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| {
            (
                entry
                    .path(&["range", "start", "line"])
                    .and_then(Json::as_i64)
                    .expect("a line"),
                entry
                    .path(&["range", "start", "character"])
                    .and_then(Json::as_i64)
                    .expect("a character"),
            )
        })
        .collect()
}

#[test]
fn the_server_advertises_and_answers_document_highlights() {
    let path = "/tmp/skuld-lsp-test/highlight.skuld";
    let source = "func main() {\n    var total = 1\n    total = total + 2\n    print(total)\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        // The cursor rests on `total` in the declaration.
        position_request(2, "textDocument/documentHighlight", path, 1, 9),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "documentHighlightProvider"]),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        starts_of(result_of(&out, 2)),
        [(1, 8), (2, 4), (2, 12), (3, 10)],
        "the declaration and every use, in source order"
    );
}

#[test]
fn a_prelude_binding_is_highlighted_even_though_it_cannot_be_renamed() {
    let path = "/tmp/skuld-lsp-test/prelude-highlight.skuld";
    let source = "func main() {\n    print(1)\n    print(2)\n}\n";
    let (out, _) = converse(&[
        did_open(path, source),
        position_request(2, "textDocument/documentHighlight", path, 1, 5),
    ]);
    assert_eq!(starts_of(result_of(&out, 2)), [(1, 4), (2, 4)]);
}

#[test]
fn highlighting_nothing_answers_an_empty_list() {
    let path = "/tmp/skuld-lsp-test/blank-highlight.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main() {\n}\n"),
        position_request(2, "textDocument/documentHighlight", path, 1, 0),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

/// An inlay hint request over an explicit line range.
fn inlay_hints(id: i64, path: &str, first: i64, last: i64) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(id as f64)),
        ("method", Json::string("textDocument/inlayHint")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "range",
                    Json::object([
                        (
                            "start",
                            Json::object([
                                ("line", Json::number(first as f64)),
                                ("character", Json::number(0.0)),
                            ]),
                        ),
                        (
                            "end",
                            Json::object([
                                ("line", Json::number(last as f64)),
                                ("character", Json::number(0.0)),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
    ])
}

/// The label of each hint in a response.
fn labels_of(result: &Json) -> Vec<&str> {
    result
        .as_array()
        .expect("a list")
        .iter()
        .map(|hint| hint.get("label").and_then(Json::as_str).expect("a label"))
        .collect()
}

#[test]
fn the_server_advertises_and_answers_inlay_hints() {
    let path = "/tmp/skuld-lsp-test/hints.skuld";
    let source = "func main() {\n    let count = 1\n    let name: string = \"a\"\n    print(count)\n    print(name)\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        inlay_hints(2, path, 0, 6),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "inlayHintProvider"]),
        Some(&Json::Bool(true))
    );
    let hints = result_of(&out, 2);
    assert_eq!(
        labels_of(hints),
        [": int"],
        "the written type is not repeated"
    );
    // The hint is drawn just after the name, at the end of `    let count`.
    assert_eq!(
        hints.as_array().unwrap()[0]
            .path(&["position", "character"])
            .and_then(Json::as_i64),
        Some(13)
    );
}

#[test]
fn inlay_hints_answer_only_the_range_asked_about() {
    let path = "/tmp/skuld-lsp-test/hints-range.skuld";
    let source = "func main() {\n    let first = 1\n    let second = 2.0\n    print(first)\n    print(second)\n}\n";
    let (out, _) = converse(&[
        did_open(path, source),
        inlay_hints(2, path, 0, 2),
        inlay_hints(3, path, 2, 3),
    ]);
    assert_eq!(labels_of(result_of(&out, 2)), [": int"]);
    assert_eq!(labels_of(result_of(&out, 3)), [": float"]);
}

#[test]
fn a_document_that_never_checked_has_no_hints() {
    let path = "/tmp/skuld-lsp-test/hints-broken.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main() {\n    let x =\n}\n"),
        inlay_hints(2, path, 0, 3),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

#[test]
fn the_server_advertises_and_answers_signature_help() {
    let path = "/tmp/skuld-lsp-test/signature.skuld";
    let source = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        // Line 5 is `    print(add(1, 2))`; the cursor rests on the second
        // argument.
        position_request(2, "textDocument/signatureHelp", path, 5, 17),
    ]);
    assert_eq!(
        out[0]
            .path(&[
                "result",
                "capabilities",
                "signatureHelpProvider",
                "triggerCharacters"
            ])
            .and_then(Json::as_array)
            .map(|characters| characters.len()),
        Some(2)
    );
    let help = result_of(&out, 2);
    let signatures = help
        .get("signatures")
        .and_then(Json::as_array)
        .expect("one signature");
    assert_eq!(signatures.len(), 1);
    assert_eq!(
        signatures[0].get("label").and_then(Json::as_str),
        Some("add(a: int, b: int) -> int")
    );
    assert_eq!(
        help.get("activeParameter").and_then(Json::as_i64),
        Some(1),
        "the cursor is on the second argument"
    );
    // Each parameter is a pair of offsets into the label, not a repeated
    // string the client would have to match.
    let parameters = signatures[0]
        .get("parameters")
        .and_then(Json::as_array)
        .expect("two parameters");
    assert_eq!(
        parameters[1]
            .get("label")
            .and_then(Json::as_array)
            .map(|pair| pair.iter().filter_map(Json::as_i64).collect::<Vec<_>>()),
        Some(vec![12, 18])
    );
}

#[test]
fn signature_help_survives_a_call_that_is_still_being_typed() {
    let path = "/tmp/skuld-lsp-test/signature-typing.skuld";
    let complete = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n}\n";
    let typing = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, \n}\n";
    let (out, _) = converse(&[
        did_open(path, complete),
        did_change(path, typing),
        position_request(2, "textDocument/signatureHelp", path, 5, 17),
    ]);
    assert_eq!(
        result_of(&out, 2)
            .get("signatures")
            .and_then(Json::as_array)
            .and_then(<[Json]>::first)
            .and_then(|signature| signature.get("label"))
            .and_then(Json::as_str),
        Some("add(a: int, b: int) -> int")
    );
}

#[test]
fn signature_help_outside_a_call_is_null() {
    let path = "/tmp/skuld-lsp-test/signature-none.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main() {\n    print(1)\n}\n"),
        position_request(2, "textDocument/signatureHelp", path, 0, 0),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Null);
}

#[test]
fn the_server_advertises_and_answers_semantic_tokens() {
    let path = "/tmp/skuld-lsp-test/tokens.skuld";
    let source = "func main() {\n    let count = 1\n    print(count)\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        document_request(2, "textDocument/semanticTokens/full", path),
    ]);
    let legend = out[0]
        .path(&["result", "capabilities", "semanticTokensProvider", "legend"])
        .expect("a legend");
    assert_eq!(
        legend
            .get("tokenTypes")
            .and_then(Json::as_array)
            .map(|types| types.len()),
        Some(tokens::TYPES.len())
    );
    let data: Vec<i64> = result_of(&out, 2)
        .get("data")
        .and_then(Json::as_array)
        .expect("a token stream")
        .iter()
        .filter_map(Json::as_i64)
        .collect();
    assert_eq!(data.len() % 5, 0, "five numbers per token");
    // Five numbers per token, each position relative to the one before:
    // delta line, delta column, length, type, modifier bits.
    #[rustfmt::skip]
    let expected: Vec<i64> = vec![
        // `main`: line 0, column 5, a function being declared.
        0, 5, 4, 5, 0b001,
        // `count`: one line down, column 8, a binding declared and readonly.
        1, 8, 5, 9, 0b011,
        // `print`: one line down again, column 4, the language's own.
        1, 4, 5, 5, 0b100,
        // `count`: same line, six columns on, a readonly binding.
        0, 6, 5, 9, 0b010,
    ];
    assert_eq!(data, expected);
}

#[test]
fn a_document_that_never_checked_has_no_semantic_tokens() {
    let path = "/tmp/skuld-lsp-test/tokens-broken.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main( {\n"),
        document_request(2, "textDocument/semanticTokens/full", path),
    ]);
    assert_eq!(
        result_of(&out, 2).get("data"),
        Some(&Json::Array(Vec::new()))
    );
}

/// A `workspace/symbol` request for one query.
fn workspace_symbol(id: i64, query: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(id as f64)),
        ("method", Json::string("workspace/symbol")),
        ("params", Json::object([("query", Json::string(query))])),
    ])
}

/// Each result as `(name, containerName)`.
fn named(result: &Json) -> Vec<(&str, Option<&str>)> {
    result
        .as_array()
        .expect("a list")
        .iter()
        .map(|symbol| {
            (
                symbol.get("name").and_then(Json::as_str).expect("a name"),
                symbol.get("containerName").and_then(Json::as_str),
            )
        })
        .collect()
}

#[test]
fn the_server_advertises_and_answers_workspace_symbols() {
    let path = "/tmp/skuld-lsp-test/workspace.skuld";
    let source = "class User {\n    name: string\n    rename(to: string) {\n        print(to)\n    }\n}\n\nfunc main() {\n    let u = new User(name: \"a\")\n    u.rename(\"b\")\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        workspace_symbol(2, "name"),
        workspace_symbol(3, ""),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "workspaceSymbolProvider"]),
        Some(&Json::Bool(true))
    );
    // A field and a method both hold the query, and each says what holds it.
    assert_eq!(
        named(result_of(&out, 2)),
        [("name", Some("User")), ("rename", Some("User"))]
    );
    // An empty query matches everything the outline knows.
    assert_eq!(
        named(result_of(&out, 3)),
        [
            ("User", None),
            ("name", Some("User")),
            ("rename", Some("User")),
            ("main", None),
        ]
    );
}

#[test]
fn workspace_symbols_reach_a_module_through_the_file_that_imports_it() {
    let entry = fixture_path("lsp_entry.skuld");
    let (out, _) = converse(&[
        did_open(
            &entry,
            "import \"modules/geometry\"\nfunc main() { print(geometry.origin().sum()) }\n",
        ),
        workspace_symbol(2, "point"),
    ]);
    let found = named(result_of(&out, 2));
    assert!(
        found.iter().any(|(name, _)| *name == "Point"),
        "a declaration in an imported module is part of the workspace: {found:?}"
    );
    // It is reported in the module's own file, not in the entry document.
    let uri = result_of(&out, 2).as_array().unwrap()[0]
        .path(&["location", "uri"])
        .and_then(Json::as_str)
        .expect("a location");
    assert!(uri.ends_with("modules/geometry/point.skuld"), "{uri}");
}

#[test]
fn the_standard_library_is_not_part_of_the_workspace() {
    // Its files are embedded in the compiler, so their paths name nothing an
    // editor could open.
    let path = "/tmp/skuld-lsp-test/uses-std.skuld";
    let (out, _) = converse(&[
        did_open(
            path,
            "import \"std/strings\"\nfunc main() {\n    print(strings.trim(\" a \"))\n}\n",
        ),
        workspace_symbol(2, "trim"),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

/// The `(line, character)` a location response starts at, and its uri.
fn located(result: &Json) -> (String, i64, i64) {
    (
        result
            .get("uri")
            .and_then(Json::as_str)
            .expect("a uri")
            .to_owned(),
        result
            .path(&["range", "start", "line"])
            .and_then(Json::as_i64)
            .expect("a line"),
        result
            .path(&["range", "start", "character"])
            .and_then(Json::as_i64)
            .expect("a character"),
    )
}

#[test]
fn the_server_advertises_and_answers_type_definition() {
    let path = "/tmp/skuld-lsp-test/type-definition.skuld";
    let source = "class User {\n    name: string\n}\n\nfunc main() {\n    let u = new User(name: \"a\")\n    print(u.name)\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        // Line 6 is `    print(u.name)`; the cursor rests on `u`.
        position_request(2, "textDocument/typeDefinition", path, 6, 10),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "typeDefinitionProvider"]),
        Some(&Json::Bool(true))
    );
    let (uri, line, _) = located(result_of(&out, 2));
    assert_eq!(uri, path_to_uri(path));
    assert_eq!(line, 0, "the class declaration, not the binding");
}

#[test]
fn type_definition_looks_through_an_array_and_an_option() {
    let path = "/tmp/skuld-lsp-test/type-definition-container.skuld";
    let source = "class User {\n    name: string\n}\n\nfunc main() {\n    var all = [new User(name: \"a\")]\n    let first: Option<User> = all[0]\n    if let one = first {\n        print(one.name)\n    }\n    print(all.len())\n}\n";
    let (out, _) = converse(&[
        did_open(path, source),
        // `all`, an array of users.
        position_request(2, "textDocument/typeDefinition", path, 10, 10),
        // `one`, an Option payload.
        position_request(3, "textDocument/typeDefinition", path, 8, 14),
    ]);
    assert_eq!(located(result_of(&out, 2)).1, 0);
    assert_eq!(located(result_of(&out, 3)).1, 0);
}

#[test]
fn type_definition_of_something_with_no_declaration_is_null() {
    let path = "/tmp/skuld-lsp-test/type-definition-int.skuld";
    let (out, _) = converse(&[
        did_open(
            path,
            "func main() {\n    let count = 1\n    print(count)\n}\n",
        ),
        position_request(2, "textDocument/typeDefinition", path, 2, 10),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Null);
}

#[test]
fn the_server_advertises_and_answers_implementation() {
    let path = "/tmp/skuld-lsp-test/implementation.skuld";
    let source = "interface Printable {\n    describe() -> string\n}\n\nclass User: Printable {\n    name: string\n    describe() -> string {\n        return this.name\n    }\n}\n\nclass Tag: Printable {\n    text: string\n    describe() -> string {\n        return this.text\n    }\n}\n\nfunc show(item: Printable) {\n    print(item.describe())\n}\n\nfunc main() {\n    show(new User(name: \"a\"))\n    show(new Tag(text: \"b\"))\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        // The interface name in the signature of `show`, on line 18.
        position_request(2, "textDocument/implementation", path, 18, 18),
        // The method called through the interface, on line 19.
        position_request(3, "textDocument/implementation", path, 19, 16),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "implementationProvider"]),
        Some(&Json::Bool(true))
    );
    let classes: Vec<i64> = result_of(&out, 2)
        .as_array()
        .expect("a list")
        .iter()
        .map(|location| located(location).1)
        .collect();
    assert_eq!(classes, [4, 11], "both classes that declare conformance");
    let methods: Vec<i64> = result_of(&out, 3)
        .as_array()
        .expect("a list")
        .iter()
        .map(|location| located(location).1)
        .collect();
    assert_eq!(methods, [6, 13], "the bodies, not the classes");
}

#[test]
fn implementation_of_a_name_that_is_not_an_interface_is_empty() {
    let path = "/tmp/skuld-lsp-test/implementation-none.skuld";
    let (out, _) = converse(&[
        did_open(
            path,
            "func main() {\n    let count = 1\n    print(count)\n}\n",
        ),
        position_request(2, "textDocument/implementation", path, 2, 10),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
}

#[test]
fn the_server_advertises_and_answers_folding_ranges() {
    let path = "/tmp/skuld-lsp-test/folding.skuld";
    let source = "import \"a\"\nimport \"b\"\n\nfunc main() {\n    print(1)\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        document_request(2, "textDocument/foldingRange", path),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "foldingRangeProvider"]),
        Some(&Json::Bool(true))
    );
    let ranges: Vec<(i64, i64, Option<&str>)> = result_of(&out, 2)
        .as_array()
        .expect("a list")
        .iter()
        .map(|range| {
            (
                range.get("startLine").and_then(Json::as_i64).unwrap(),
                range.get("endLine").and_then(Json::as_i64).unwrap(),
                range.get("kind").and_then(Json::as_str),
            )
        })
        .collect();
    assert_eq!(ranges, [(0, 1, Some("imports")), (3, 5, None)]);
}

#[test]
fn folding_answers_a_document_that_does_not_parse() {
    // It reads tokens, so the half-typed text on screen still folds.
    let path = "/tmp/skuld-lsp-test/folding-broken.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main() {\n    print(\n"),
        document_request(2, "textDocument/foldingRange", path),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Array(Vec::new()));
    // And a block that is closed folds even when the file as a whole is not.
    let (out, _) = converse(&[
        did_open(path, "func main() {\n    print(1)\n}\nfunc half(\n"),
        document_request(3, "textDocument/foldingRange", path),
    ]);
    assert_eq!(result_of(&out, 3).as_array().map(<[Json]>::len), Some(1));
}

#[test]
fn the_server_advertises_and_answers_selection_ranges() {
    let path = "/tmp/skuld-lsp-test/selection.skuld";
    let source = "func main() {\n    print(count)\n}\n";
    let request_with_positions = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(2.0)),
        ("method", Json::string("textDocument/selectionRange")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "positions",
                    Json::Array(vec![Json::object([
                        ("line", Json::number(1.0)),
                        ("character", Json::number(12.0)),
                    ])]),
                ),
            ]),
        ),
    ]);
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        request_with_positions,
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "selectionRangeProvider"]),
        Some(&Json::Bool(true))
    );
    let chains = result_of(&out, 2)
        .as_array()
        .expect("one chain per position");
    assert_eq!(chains.len(), 1);
    // The innermost range is the word, and each parent contains it.
    assert_eq!(
        chains[0]
            .path(&["range", "start", "character"])
            .and_then(Json::as_i64),
        Some(10)
    );
    assert_eq!(
        chains[0]
            .path(&["range", "end", "character"])
            .and_then(Json::as_i64),
        Some(15)
    );
    let parent = chains[0].get("parent").expect("a parent");
    assert_eq!(
        parent
            .path(&["range", "start", "character"])
            .and_then(Json::as_i64),
        Some(9),
        "the parentheses of the call"
    );
    // The chain ends at the whole document, which has no parent of its own.
    let mut outermost = parent;
    while let Some(next) = outermost.get("parent") {
        outermost = next;
    }
    assert_eq!(
        outermost
            .path(&["range", "start", "line"])
            .and_then(Json::as_i64),
        Some(0)
    );
}

/// A call-hierarchy query about an item the server produced.
fn call_hierarchy(id: i64, method: &str, item: &Json) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(id as f64)),
        ("method", Json::string(method)),
        ("params", Json::object([("item", item.clone())])),
    ])
}

#[test]
fn the_server_advertises_and_walks_the_call_hierarchy() {
    let path = "/tmp/skuld-lsp-test/hierarchy.skuld";
    let source = "func one() -> int {\n    return 1\n}\n\nfunc two() -> int {\n    return one() + one()\n}\n\nfunc main() {\n    print(two())\n    print(one())\n}\n";
    // `two` is declared on line 4, at column 5.
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        position_request(2, "textDocument/prepareCallHierarchy", path, 4, 6),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "callHierarchyProvider"]),
        Some(&Json::Bool(true))
    );
    let items = result_of(&out, 2).as_array().expect("one item").to_vec();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("name").and_then(Json::as_str), Some("two"));

    // The same session, asked both ways about that item.
    let (out, _) = converse(&[
        did_open(path, source),
        call_hierarchy(3, "callHierarchy/incomingCalls", &items[0]),
        call_hierarchy(4, "callHierarchy/outgoingCalls", &items[0]),
    ]);
    let incoming = result_of(&out, 3).as_array().expect("a list");
    assert_eq!(incoming.len(), 1);
    assert_eq!(
        incoming[0].path(&["from", "name"]).and_then(Json::as_str),
        Some("main")
    );
    assert_eq!(
        incoming[0]
            .get("fromRanges")
            .and_then(Json::as_array)
            .map(<[Json]>::len),
        Some(1),
        "main calls two once"
    );
    let outgoing = result_of(&out, 4).as_array().expect("a list");
    assert_eq!(outgoing.len(), 1);
    assert_eq!(
        outgoing[0].path(&["to", "name"]).and_then(Json::as_str),
        Some("one")
    );
    assert_eq!(
        outgoing[0]
            .get("fromRanges")
            .and_then(Json::as_array)
            .map(<[Json]>::len),
        Some(2),
        "two calls one twice, and each place is reported"
    );
}

#[test]
fn a_method_can_start_a_call_hierarchy_and_says_which_class() {
    let path = "/tmp/skuld-lsp-test/hierarchy-method.skuld";
    let source = "class Greeter {\n    name: string\n    greet() {\n        print(this.name)\n    }\n}\n\nfunc main() {\n    let g = new Greeter(name: \"a\")\n    g.greet()\n}\n";
    let (out, _) = converse(&[
        did_open(path, source),
        // The method's declaration, on line 2.
        position_request(2, "textDocument/prepareCallHierarchy", path, 2, 5),
    ]);
    let items = result_of(&out, 2).as_array().expect("one item");
    assert_eq!(items[0].get("name").and_then(Json::as_str), Some("greet"));
    assert_eq!(
        items[0].get("detail").and_then(Json::as_str),
        Some("Greeter")
    );
    assert_eq!(items[0].get("kind").and_then(Json::as_i64), Some(6));
}

#[test]
fn a_call_hierarchy_on_something_that_is_not_a_function_is_null() {
    let path = "/tmp/skuld-lsp-test/hierarchy-none.skuld";
    let (out, _) = converse(&[
        did_open(
            path,
            "func main() {\n    let count = 1\n    print(count)\n}\n",
        ),
        position_request(2, "textDocument/prepareCallHierarchy", path, 1, 9),
    ]);
    assert_eq!(result_of(&out, 2), &Json::Null);
}

/// An `initialize` from a client that reads a nested outline, which is what
/// every editor in use does and what the protocol nonetheless makes optional.
fn initialize_with_hierarchical_symbols(id: i64) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(id as f64)),
        ("method", Json::string("initialize")),
        (
            "params",
            Json::object([(
                "capabilities",
                Json::object([(
                    "textDocument",
                    Json::object([(
                        "documentSymbol",
                        Json::object([("hierarchicalDocumentSymbolSupport", Json::Bool(true))]),
                    )]),
                )]),
            )]),
        ),
    ])
}

#[test]
fn a_client_that_never_claimed_nesting_gets_the_flat_outline() {
    // The protocol's default is that a client cannot read the nested form,
    // and one that cannot shows nothing at all when it is sent.
    let path = "/tmp/skuld-lsp-test/flat-outline.skuld";
    let source = "class User {\n    name: string\n}\n\nfunc main() {\n    let u = new User(name: \"a\")\n    print(u.name)\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        document_request(2, "textDocument/documentSymbol", path),
    ]);
    let symbols = result_of(&out, 2).as_array().expect("an outline");
    assert_eq!(
        symbols
            .iter()
            .map(|symbol| (
                symbol.get("name").and_then(Json::as_str).unwrap(),
                symbol.get("containerName").and_then(Json::as_str),
            ))
            .collect::<Vec<_>>(),
        [("User", None), ("name", Some("User")), ("main", None)]
    );
    // Flat entries carry a location instead of nesting.
    assert!(symbols[1].get("children").is_none());
    assert_eq!(
        symbols[1].path(&["location", "uri"]).and_then(Json::as_str),
        Some(path_to_uri(path).as_str())
    );
}

#[test]
fn semantic_tokens_answer_a_window_when_one_is_asked_for() {
    let path = "/tmp/skuld-lsp-test/tokens-range.skuld";
    let source = "func first() {\n    print(1)\n}\n\nfunc second() {\n    print(2)\n}\n\nfunc main() {\n    first()\n    second()\n}\n";
    let ranged = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::number(2.0)),
        ("method", Json::string("textDocument/semanticTokens/range")),
        (
            "params",
            Json::object([
                (
                    "textDocument",
                    Json::object([("uri", Json::string(path_to_uri(path)))]),
                ),
                (
                    "range",
                    Json::object([
                        (
                            "start",
                            Json::object([
                                ("line", Json::number(4.0)),
                                ("character", Json::number(0.0)),
                            ]),
                        ),
                        (
                            "end",
                            Json::object([
                                ("line", Json::number(6.0)),
                                ("character", Json::number(0.0)),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);
    let (out, _) = converse(&[request(1, "initialize"), did_open(path, source), ranged]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "semanticTokensProvider", "range"]),
        Some(&Json::Bool(true))
    );
    let data: Vec<i64> = result_of(&out, 2)
        .get("data")
        .and_then(Json::as_array)
        .expect("a token stream")
        .iter()
        .filter_map(Json::as_i64)
        .collect();
    // Only `second` and the `print` under it, and the first delta is measured
    // from the top of the document, not from a token that was not sent.
    #[rustfmt::skip]
    let expected: Vec<i64> = vec![
        4, 5, 6, 5, 0b001,
        1, 4, 5, 5, 0b100,
    ];
    assert_eq!(data, expected);
}

#[test]
fn a_file_with_no_entrypoint_is_a_document_like_any_other() {
    // A module is a library, and an editor showing one cannot know which
    // program it belongs to. Requiring a `main` left it unchecked, and every
    // answer that reads the last successful check was dead in it.
    let path = "/tmp/skuld-lsp-test/library.skuld";
    let source =
        "pub func twice(value: int) -> int {\n    let doubled = value * 2\n    return doubled\n}\n";
    let (out, _) = converse(&[
        did_open(path, source),
        hover_at(path, 2, 12),
        document_request(3, "textDocument/semanticTokens/full", path),
        inlay_hints(4, path, 0, 4),
    ]);
    assert_eq!(
        diagnostics_for(&out, path),
        &[] as &[Json],
        "a library file reports nothing about a `main` it is not missing"
    );
    let hover = out
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(9))
        .and_then(|message| message.get("result"))
        .expect("a hover response");
    assert_eq!(
        hover.path(&["contents", "value"]).and_then(Json::as_str),
        Some("```skuld\nlet doubled: int\n```")
    );
    assert!(
        !result_of(&out, 3)
            .get("data")
            .and_then(Json::as_array)
            .expect("a token stream")
            .is_empty()
    );
    assert_eq!(labels_of(result_of(&out, 4)), [": int"]);
}

#[test]
fn declaration_answers_the_same_place_as_definition() {
    // Skuld has no forward declarations, so the two questions have one answer;
    // answering both keeps a client's `gD` from reporting an unsupported
    // method.
    let path = "/tmp/skuld-lsp-test/declaration.skuld";
    let source = "func add(a: int, b: int) -> int {\n    return a + b\n}\n\nfunc main() {\n    print(add(1, 2))\n}\n";
    let (out, _) = converse(&[
        request(1, "initialize"),
        did_open(path, source),
        position_request(2, "textDocument/definition", path, 5, 11),
        position_request(3, "textDocument/declaration", path, 5, 11),
    ]);
    assert_eq!(
        out[0].path(&["result", "capabilities", "declarationProvider"]),
        Some(&Json::Bool(true))
    );
    assert_eq!(result_of(&out, 2), result_of(&out, 3));
    assert_eq!(located(result_of(&out, 3)).1, 0);
}

#[test]
fn the_server_asks_to_be_told_about_files_changing_on_disk() {
    let path = "/tmp/skuld-lsp-test/watched.skuld";
    let initialized = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("initialized")),
        ("params", Json::object([])),
    ]);
    let changed = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("workspace/didChangeWatchedFiles")),
        (
            "params",
            Json::object([("changes", Json::Array(Vec::new()))]),
        ),
    ]);
    let (out, _) = converse(&[
        request(1, "initialize"),
        initialized,
        did_open(path, "func main() {\n    print(1)\n}\n"),
        changed,
    ]);
    let registration = out
        .iter()
        .find(|message| {
            message.get("method").and_then(Json::as_str) == Some("client/registerCapability")
        })
        .expect("a registration request");
    assert_eq!(
        registration
            .path(&["params", "registrations"])
            .and_then(Json::as_array)
            .and_then(<[Json]>::first)
            .and_then(|entry| entry.get("method"))
            .and_then(Json::as_str),
        Some("workspace/didChangeWatchedFiles")
    );
    // The change republished the open document rather than being dropped.
    let reports = out
        .iter()
        .filter(|message| {
            message.get("method").and_then(Json::as_str) == Some("textDocument/publishDiagnostics")
        })
        .count();
    assert_eq!(reports, 2, "one for the open, one for the change");
}
