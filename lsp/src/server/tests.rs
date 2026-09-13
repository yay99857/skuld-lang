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
    // `hover` is answered now, so the refused request has to be one the
    // server genuinely does not implement.
    let (out, _) = converse(&[request(7, "textDocument/references"), notification]);
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
        hover.path(&["range", "start", "character"]).and_then(Json::as_i64),
        Some(10)
    );
    assert_eq!(
        hover.path(&["range", "end", "character"]).and_then(Json::as_i64),
        Some(14)
    );
}

#[test]
fn hover_over_nothing_answers_null_rather_than_an_error() {
    let path = "/tmp/skuld-lsp-test/blank.skuld";
    let (out, _) = converse(&[
        did_open(path, "func main() {\n}\n"),
        hover_at(path, 1, 0),
    ]);
    let hover = out
        .iter()
        .rfind(|message| message.get("id").and_then(Json::as_i64) == Some(9))
        .and_then(|message| message.get("result"))
        .expect("a hover response");
    assert_eq!(*hover, Json::Null);
}
