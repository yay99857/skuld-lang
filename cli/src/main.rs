mod native;
use skuld_compiler::{
    check_program, compile_program_to_c,
    diagnostic::Diagnostic,
    lex,
    module::{self, ModuleLoader},
    parse,
    span::SourceFile,
};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const USAGE: &str = "Usage: skuld <lex|parse|resolve|check|emit-c|build|run> <file.skuld> [-l<library> | -L<directory>]...";
#[derive(Clone, Copy)]
enum Action {
    Lex,
    Parse,
    Resolve,
    Check,
    EmitC,
    Build,
    Run,
}
fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return write_output(&format!("{USAGE}\n"));
    }
    if args.len() < 2 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let action = match args[0].to_str() {
        Some("lex") => Action::Lex,
        Some("parse") => Action::Parse,
        Some("resolve") => Action::Resolve,
        Some("check") => Action::Check,
        Some("emit-c") => Action::EmitC,
        Some("build") => Action::Build,
        Some("run") => Action::Run,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Linker arguments are restricted to library selection: nothing here may
    // redirect clang's output or change how the program itself is compiled.
    let mut link_flags = Vec::new();
    for argument in &args[2..] {
        let Some(flag) = argument.to_str() else {
            eprintln!("error: linker arguments must be valid UTF-8");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        if !matches!(action, Action::Build | Action::Run) {
            eprintln!("error: linker arguments are only meaningful for `build` and `run`");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        if !((flag.starts_with("-l") || flag.starts_with("-L")) && flag.len() > 2) {
            eprintln!(
                "error: unsupported linker argument `{flag}`; only `-l<library>` and `-L<directory>` are accepted"
            );
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        link_flags.push(flag.to_owned());
    }
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
    let entry = PathBuf::from(&args[1]);
    // Import paths are relative to the directory the entry file lives in.
    // That directory is the program root; a module is a directory under it.
    let mut loader = Directories {
        root: entry.parent().unwrap_or(Path::new(".")).to_path_buf(),
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
                Err(one_file(&source, output.diagnostics))
            }
        }
        Action::Parse => {
            let parsed = parse(&source.text);
            match parsed.program {
                // `parse` inspects one file, so it never follows an import.
                Some(program) => Ok(format!("{program:#?}\n")),
                None => Err(one_file(&source, parsed.diagnostics)),
            }
        }
        // `resolve` reports the whole program's tables, since a module's
        // declarations are part of what the entry file resolves against.
        Action::Resolve => check_program(&source.name, &source.text, &mut loader)
            .map(|typed| format!("{:#?}\n", typed.resolution())),
        Action::Check => {
            check_program(&source.name, &source.text, &mut loader).map(|_| String::new())
        }
        Action::EmitC | Action::Build | Action::Run => {
            compile_program_to_c(&source.name, &source.text, &mut loader)
        }
    };
    match result {
        Err(errors) => {
            eprint!("{}", errors.render());
            ExitCode::FAILURE
        }
        Ok(output) if matches!(action, Action::Run) => match native::run(&output, &link_flags) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(output) if matches!(action, Action::Build) => {
            let executable = match executable_path(Path::new(&args[1])) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match native::build(&output, &executable, &link_flags) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Ok(output) => write_output(&output),
    }
}
/// The executable lands in the working directory under the source file's stem,
/// so building never writes next to the source or into a directory the user did
/// not choose.
fn executable_path(source: &Path) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .ok_or_else(|| format!("`{}` has no file name to build from", source.display()))?;
    if stem.is_empty() {
        return Err(format!("`{}` has an empty file name", source.display()));
    }
    let mut name = stem.to_os_string();
    if cfg!(windows) {
        name.push(".exe");
    }
    let executable = PathBuf::from(&name);
    // Without an extension the stem is the source itself; refuse rather than
    // overwrite the program being compiled.
    if same_file(&executable, source) {
        return Err(format!(
            "building `{}` would overwrite it; rename the source to end in `.skuld`",
            source.display()
        ));
    }
    Ok(executable)
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        // A missing output cannot be the source that was just read.
        _ => false,
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

/// Diagnostics from a stage that looks at the entry file alone.
fn one_file(source: &SourceFile, diagnostics: Vec<Diagnostic>) -> module::Errors {
    module::Errors {
        sources: vec![source.clone()],
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| module::FileDiagnostic {
                file: module::FileId(0),
                diagnostic,
            })
            .collect(),
    }
}

/// Import paths resolved against the program root on disk. A module is a
/// directory; its `.skuld` files are read in name order, so the set a module
/// is made of never depends on how the filesystem happens to enumerate it.
struct Directories {
    root: PathBuf,
}

impl ModuleLoader for Directories {
    fn load(&mut self, path: &str) -> Result<Vec<(String, String)>, String> {
        // The compiler already rejected `..` and absolute paths, but that is a
        // rule about how a path is written. A symlink is resolved by the
        // filesystem and can still point anywhere, so containment in the
        // program root is verified after resolution, where it can be enforced.
        let root = fs::canonicalize(&self.root)
            .map_err(|error| format!("cannot resolve the program root: {error}"))?;
        let directory =
            fs::canonicalize(self.root.join(path)).map_err(|error| error.to_string())?;
        if !directory.starts_with(&root) {
            return Err(outside(path, &root));
        }
        let entries = fs::read_dir(&directory).map_err(|error| error.to_string())?;
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
            let file = match fs::canonicalize(&file) {
                Ok(file) => file,
                // A broken link is not a source file; skip it rather than
                // failing a module that is otherwise well formed.
                Err(_) => continue,
            };
            if !file.is_file() {
                continue;
            }
            if !file.starts_with(&root) {
                return Err(outside(path, &root));
            }
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("cannot read `{}`: {error}", file.display()))?;
            files.push((
                format!("{path}/{}", entry.file_name().to_string_lossy()),
                text,
            ));
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(files)
    }
}

fn outside(path: &str, root: &Path) -> String {
    format!(
        "`{path}` resolves outside the program root at `{}`; a build reads only \
         what is under its own root, and a link out of it is not followed",
        root.display()
    )
}
