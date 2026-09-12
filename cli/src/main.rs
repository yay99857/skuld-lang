mod native;
use skuld_compiler::{check, compile_to_c, lex, parse, resolve, span::SourceFile};
use std::{
    env, fs,
    io::{self, Write},
    process::ExitCode,
};

const USAGE: &str = "Usage: skuld <lex|parse|resolve|check|emit-c|run> <file.skuld>";
#[derive(Clone, Copy)]
enum Action {
    Lex,
    Parse,
    Resolve,
    Check,
    EmitC,
    Run,
}
fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return write_output(&format!("{USAGE}\n"));
    }
    if args.len() != 2 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let action = match args[0].to_str() {
        Some("lex") => Action::Lex,
        Some("parse") => Action::Parse,
        Some("resolve") => Action::Resolve,
        Some("check") => Action::Check,
        Some("emit-c") => Action::EmitC,
        Some("run") => Action::Run,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!(
                "error: cannot read `{}`: {error}",
                args[1].to_string_lossy()
            );
            return ExitCode::FAILURE;
        }
    };
    let source = SourceFile::new(args[1].to_string_lossy(), text);
    let result = match action {
        Action::Lex => {
            let output = lex(&source.text);
            if output.diagnostics.is_empty() {
                Ok(output
                    .tokens
                    .iter()
                    .map(|t| format!("{:?}\n", t.kind))
                    .collect())
            } else {
                Err(output.diagnostics)
            }
        }
        Action::Parse | Action::Resolve => {
            let parsed = parse(&source.text);
            if let Some(program) = parsed.program {
                if matches!(action, Action::Parse) {
                    Ok(format!("{program:#?}\n"))
                } else {
                    let resolved = resolve(&program);
                    if let Some(resolution) = resolved.resolution {
                        Ok(format!("{resolution:#?}\n"))
                    } else {
                        Err(resolved.diagnostics)
                    }
                }
            } else {
                Err(parsed.diagnostics)
            }
        }
        Action::Check => check(&source.text).map(|_| String::new()),
        Action::EmitC | Action::Run => compile_to_c(&source.text),
    };
    match result {
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                eprint!("{}", diagnostic.render(&source));
            }
            ExitCode::FAILURE
        }
        Ok(output) if matches!(action, Action::Run) => match native::run(&output) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(output) => write_output(&output),
    }
}
fn write_output(output: &str) -> ExitCode {
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    if let Err(error) = stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.flush())
    {
        eprintln!("error: cannot write output: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
