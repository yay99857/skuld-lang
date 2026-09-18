//! The editor files describe what the compiler accepts, and this is what
//! makes that true rather than intended.
//!
//! `editors/nvim/syntax/skuld.vim` says at the top that every keyword in it is
//! taken from the lexer, and by the time it was checked it had missed seven
//! milestones: it still called the integer literals decimal-only and knew
//! nothing of `defer`, `usize` or the pointer builtins. A promise a file makes
//! about itself is worth as much as the test that keeps it.
//!
//! So the lists are read from the compiler's own source — the lexer's keyword
//! table and the resolver's prelude — and every name in them has to appear in
//! both the Vim syntax file and the TextMate grammar.
use std::{fs, path::PathBuf};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repository().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

/// Every `"word" => Variant,` in the lexer's keyword table.
fn keywords() -> Vec<String> {
    let lexer = read("compiler/src/lexer.rs");
    let table = lexer
        .split_once("\"func\" => Function,")
        .map(|(_, rest)| format!("\"func\" => Function,{rest}"))
        .expect("the lexer has a keyword table");
    let mut found = Vec::new();
    for line in table.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some((word, tail)) = rest.split_once('"') else {
            continue;
        };
        if !tail.trim_start().starts_with("=>") {
            continue;
        }
        if word.chars().all(|c| c.is_ascii_lowercase()) {
            found.push(word.to_owned());
        }
        // The table ends at the booleans, which are the last two entries.
        if word == "false" {
            break;
        }
    }
    assert!(
        found.len() > 20,
        "the keyword table was not found where it was expected: {found:?}"
    );
    found
}

/// Every name the resolver inserts into the root scope by a literal, which is
/// every prelude binding except the width conversions, and those are listed
/// with the types instead.
fn prelude() -> Vec<String> {
    let resolver = read("compiler/src/resolver.rs");
    let mut found = Vec::new();
    for (index, _) in resolver.match_indices("resolver.insert(") {
        let rest = &resolver[index + "resolver.insert(".len()..];
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            // `resolver.insert(kind.name(), ...)` is a width conversion, whose
            // spelling comes from the type rather than from a literal.
            continue;
        };
        let Some((name, _)) = rest.split_once('"') else {
            continue;
        };
        found.push(name.to_owned());
    }
    found.sort();
    found.dedup();
    assert!(
        found.len() > 10,
        "the prelude was not found where it was expected: {found:?}"
    );
    found
}

/// The widths, which are both type names and conversion functions.
const WIDTHS: [&str; 15] = [
    "int", "float", "bool", "string", "char", "void", "i8", "i16", "i32", "i64", "isize", "u8",
    "u16", "u32", "u64",
];

fn assert_mentions(file: &str, contents: &str, names: &[String], what: &str) {
    let missing: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !contents.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "{file} does not mention {what}: {missing:?}\n\
         it describes what the compiler accepts, so a name the compiler knows belongs in it"
    );
}

#[test]
fn the_editor_files_know_every_keyword_and_prelude_binding() {
    let keywords = keywords();
    let prelude = prelude();
    let widths: Vec<String> = WIDTHS.iter().map(|name| (*name).to_owned()).collect();

    for file in [
        "editors/nvim/syntax/skuld.vim",
        "editors/vscode/syntaxes/skuld.tmLanguage.json",
    ] {
        let contents = read(file);
        assert_mentions(file, &contents, &keywords, "every keyword");
        assert_mentions(file, &contents, &prelude, "every prelude binding");
        assert_mentions(file, &contents, &widths, "every type name");
    }
}

#[test]
fn the_grammar_is_well_formed_and_claims_both_extensions() {
    let grammar = read("editors/vscode/syntaxes/skuld.tmLanguage.json");
    // The scope name is what Linguist's entry points at, and what any other
    // editor embedding this grammar refers to.
    assert!(grammar.contains("\"scopeName\": \"source.skuld\""));
    // A TextMate grammar lists its file types without the dot, which is the
    // convention every editor that reads one expects.
    assert!(grammar.contains("\"fileTypes\": [\"skuld\", \"sk\"]"));

    let package = read("editors/vscode/package.json");
    assert!(package.contains("\"source.skuld\""));
    assert!(package.contains("\".sk\""));

    // The proposed Linguist entry has to agree with the grammar it names.
    let proposed = read(".github/linguist/skuld.yml");
    assert!(proposed.contains("tm_scope: source.skuld"));
    assert!(proposed.contains("#7a1515"));
}

/// The extension says it provides a language server. That sentence was false
/// for as long as the extension existed: it registered a grammar and started
/// nothing, so a reader who believed the description got highlighting and
/// wondered why completion never came.
///
/// What is checked here is only what can rot without anyone noticing — a file
/// renamed out from under `main`, a discovery module that stops being
/// required. Whether the server then answers is the language server's own 191
/// tests, and whether Visual Studio Code loads the result is something only
/// Visual Studio Code can say.
#[test]
fn the_extension_starts_the_language_server_it_advertises() {
    let package = read("editors/vscode/package.json");
    assert!(
        package.contains("\"main\": \"./extension.js\""),
        "the extension declares no entry point, so nothing runs"
    );
    assert!(
        package.contains("\"vscode-languageclient\""),
        "the entry point needs a client to speak the protocol with"
    );
    assert!(
        package.contains("\"skuld.server.path\""),
        "a person whose server is somewhere unusual needs a way to say so"
    );

    let extension = read("editors/vscode/extension.js");
    assert!(
        extension.contains("require(\"./server.js\")"),
        "the entry point no longer uses the discovery it was split out of"
    );
    assert!(
        extension.contains("LanguageClient"),
        "the entry point does not start a client"
    );

    // The discovery is its own file precisely so it can run outside an editor.
    // If that stops being true, the only part of this that is testable stops
    // being testable.
    let server = read("editors/vscode/server.js");
    assert!(
        !server.contains("require(\"vscode\")"),
        "the discovery reaches for the editor, so it can no longer be tested"
    );
    assert!(
        server.contains("skuld-lsp.exe"),
        "the discovery does not look for a Windows server"
    );
    // Release before debug: someone who built for release meant to use it.
    let release = server.find("\"release\"").expect("a release profile");
    let debug = server.find("\"debug\"").expect("a debug profile");
    assert!(release < debug, "debug would be preferred over release");
}
