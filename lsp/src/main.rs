//! `skuld-lsp` — a language server for Skuld, speaking LSP over stdio.
//!
//! Stage 0 reports diagnostics and nothing else. The protocol is implemented
//! here rather than taken from a crate, which keeps the project's rule of no
//! external dependencies: rust-analyzer, clangd and gopls each do the same.

mod json;
mod rpc;
mod server;
mod text;

use std::io::{self, BufReader};

const USAGE: &str = "\
skuld-lsp — a language server for Skuld

Usage:
  skuld-lsp            speak LSP over stdin and stdout
  skuld-lsp --help     print this message
  skuld-lsp --version  print the version

The server is started by an editor, not by hand. Running it in a terminal
leaves it waiting for a framed LSP message that will never come.
";

/// What the command line asked for. Kept separate from doing it so the
/// decision is a pure function with its own tests, as the CLI's is.
#[derive(Debug, PartialEq, Eq)]
enum Startup {
    Serve,
    Print(String),
    Misuse(String),
}

fn decide(arguments: &[String]) -> Startup {
    // An option is answered before anything else, so `--help` works even where
    // stdin is a terminal and no client is attached.
    match arguments {
        [] => Startup::Serve,
        [only] => match only.as_str() {
            "-h" | "--help" => Startup::Print(USAGE.to_string()),
            "-V" | "--version" => {
                Startup::Print(format!("skuld-lsp {}\n", env!("CARGO_PKG_VERSION")))
            }
            other => Startup::Misuse(format!("unexpected argument `{other}`")),
        },
        // Every option this server takes answers on its own, so a second one
        // is a misunderstanding worth reporting rather than quietly ignoring.
        [first, ..] => Startup::Misuse(format!(
            "`{first}` takes no other arguments, and {} followed it",
            arguments.len() - 1
        )),
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match decide(&arguments) {
        Startup::Print(message) => print!("{message}"),
        Startup::Misuse(message) => {
            eprintln!("skuld-lsp: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
        Startup::Serve => {
            let stdin = io::stdin();
            let stdout = io::stdout();
            let mut input = BufReader::new(stdin.lock());
            let mut output = stdout.lock();
            let code = server::Server::new().run(&mut input, &mut output);
            std::process::exit(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide_on(arguments: &[&str]) -> Startup {
        decide(&arguments.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn no_arguments_starts_the_server() {
        assert_eq!(decide_on(&[]), Startup::Serve);
    }

    #[test]
    fn answers_help_and_version() {
        assert!(matches!(decide_on(&["-h"]), Startup::Print(_)));
        assert!(matches!(decide_on(&["--help"]), Startup::Print(_)));
        let Startup::Print(version) = decide_on(&["--version"]) else {
            panic!("--version should print");
        };
        assert!(version.starts_with("skuld-lsp "));
        assert!(version.ends_with('\n'));
    }

    #[test]
    fn rejects_an_unknown_argument() {
        let Startup::Misuse(message) = decide_on(&["--stdio"]) else {
            panic!("an unknown argument is a misuse");
        };
        assert!(message.contains("--stdio"), "{message}");
    }

    #[test]
    fn rejects_a_trailing_argument_after_an_option() {
        // The earlier loop form answered the first option and dropped the
        // rest, which hides a mistake in an editor's configured command.
        assert!(matches!(
            decide_on(&["--help", "extra"]),
            Startup::Misuse(_)
        ));
    }
}
