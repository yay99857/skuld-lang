mod native;
mod test_runner;
use skuld_compiler::{
    check_program, compile_program_to_c,
    diagnostic::Diagnostic,
    lex,
    module::{self, ModuleLoader},
    parse,
    span::SourceFile,
};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const USAGE: &str =
    "usage: skuld <command> <file.skuld> [options]   (`skuld --help` for the full list)";

const HELP: &str = "\
skuld — the Skuld compiler

usage:
    skuld <command> <file.skuld> [options]

commands:
    run        check, compile and execute; keeps no artifacts
    build      check and compile to an executable in the working directory
    test       run the `test_...` functions in the file and report them
    check      static checking only; needs no clang and prints nothing on success
    fmt        format the file; writes back unless --check is passed
    emit-c     print the generated C to stdout
    lex        print the token stream of the one file named
    parse      print the syntax tree of the one file named
    resolve    print the resolution tables of the whole program

options:
    -o <path>        where `build` writes the executable
    --args ...       hand every later argument to the program        (run)
    -l<library>      link a library, e.g. -lm            (build and run)
    -L<directory>    add a library search directory      (build and run)
    --check          only check if formatting would change the file (fmt)
    --               read every later argument as a path, never a flag
    -h, --help       print this help
    -V, --version    print the version

exit codes:
    0    success; for `run`, the compiled program's own exit code
    1    the program was rejected, or a tool failed
    2    the command line was invalid

examples:
    skuld run examples/hello.skuld
    skuld run tool.skuld --args document.json user.name
    skuld check src/main.skuld
    skuld test src/main_tests.skuld
    skuld build program.skuld -o bin/program -lm
    skuld emit-c program.skuld > generated.c
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Lex,
    Parse,
    Resolve,
    Check,
    Test,
    Fmt,
    EmitC,
    Build,
    Run,
}

impl Action {
    const NAMES: [(&'static str, Self); 9] = [
        ("lex", Self::Lex),
        ("parse", Self::Parse),
        ("resolve", Self::Resolve),
        ("check", Self::Check),
        ("test", Self::Test),
        ("fmt", Self::Fmt),
        ("emit-c", Self::EmitC),
        ("build", Self::Build),
        ("run", Self::Run),
    ];
    fn parse(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find(|(spelling, _)| *spelling == name)
            .map(|(_, action)| *action)
    }
    /// Whether this stage ever reaches clang, which is what makes a linker
    /// argument meaningful.
    fn links(self) -> bool {
        matches!(self, Self::Build | Self::Run | Self::Test)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    action: Action,
    file: PathBuf,
    link_flags: Vec<String>,
    /// Only `build` writes a file, and only `-o` chooses where.
    output: Option<PathBuf>,
    check_only: bool,
    /// `--freestanding`: no runtime, no libc, no entry point, and an object
    /// file rather than an executable.
    freestanding: bool,
    /// Everything after `--args`, handed to the program `run` executes.
    program_arguments: Vec<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// Print to stdout and exit successfully: help and version are answers,
    /// not errors, so they never go to stderr.
    Print(String),
    Invoke(Box<Invocation>),
    /// Print to stderr and exit 2: the command line itself was wrong.
    Misuse(String),
}

/// Flags may appear before or after the file, `--` ends them, and nothing is
/// positional but the command and the file.
fn parse_arguments(arguments: &[OsString]) -> Command {
    let mut positional: Vec<&OsStr> = Vec::new();
    let mut link_flags = Vec::new();
    let mut output: Option<PathBuf> = None;
    let mut check_only = false;
    let mut freestanding = false;
    let mut flags_over = false;
    let mut pending_output = false;
    let mut program_arguments: Vec<OsString> = Vec::new();
    let mut forwarding = false;
    for argument in arguments {
        // `--args` ends this command line and starts the program's. Nothing
        // after it is read here, not even `--`: a program's own arguments are
        // none of the compiler's business.
        if forwarding {
            program_arguments.push(argument.clone());
            continue;
        }
        if pending_output {
            output = Some(PathBuf::from(argument));
            pending_output = false;
            continue;
        }
        let text = argument.to_str();
        if flags_over || !text.is_some_and(|text| text.starts_with('-') && text.len() > 1) {
            positional.push(argument);
            continue;
        }
        // `text` is Some here: only a valid UTF-8 argument reaches this point.
        let flag = text.unwrap_or_default();
        match flag {
            "--" => flags_over = true,
            "--args" => forwarding = true,
            "-h" | "--help" => return Command::Print(HELP.to_owned()),
            "-V" | "--version" => {
                return Command::Print(format!("skuld {}\n", env!("CARGO_PKG_VERSION")));
            }
            "--check" => check_only = true,
            "--freestanding" => freestanding = true,
            "-o" | "--output" => pending_output = true,
            _ if flag.starts_with("-o") => output = Some(PathBuf::from(&flag[2..])),
            _ if (flag.starts_with("-l") || flag.starts_with("-L")) && flag.len() > 2 => {
                link_flags.push(flag.to_owned());
            }
            _ => {
                return Command::Misuse(format!(
                    "unknown option `{flag}`\n{USAGE}\nonly `-o`, `-l<library>`, `-L<directory>`, `--check` and `--freestanding` are accepted"
                ));
            }
        }
    }
    if pending_output {
        return Command::Misuse(format!(
            "`-o` needs a path to write the executable to\n{USAGE}"
        ));
    }
    let Some(name) = positional.first() else {
        return Command::Misuse(format!("no command given\n{USAGE}"));
    };
    let Some(action) = name.to_str().and_then(Action::parse) else {
        let name = name.to_string_lossy();
        let mut message = format!("unknown command `{name}`");
        if let Some(guess) = nearest_command(&name) {
            message.push_str(&format!("; did you mean `{guess}`?"));
        }
        message.push('\n');
        message.push_str(USAGE);
        return Command::Misuse(message);
    };
    let Some(file) = positional.get(1) else {
        let mut message = format!("`{}` needs a file to work on", name.to_string_lossy());
        // `-osomething.skuld` is `-o` with a joined value, the way a C compiler
        // reads it, so a source whose name begins with a dash disappears into
        // an option. Say so, rather than leaving the reader to work it out.
        if let Some(taken) = output
            .as_ref()
            .filter(|path| path.extension().is_some_and(|e| e == "skuld" || e == "sk"))
        {
            message.push_str(&format!(
                "\nnote: `-o` took `{}` as the path to write to; pass a file whose name begins with `-` after `--`",
                taken.display()
            ));
        }
        message.push('\n');
        message.push_str(USAGE);
        return Command::Misuse(message);
    };
    if let Some(extra) = positional.get(2) {
        return Command::Misuse(format!(
            "unexpected argument `{}`; one file at a time\n{USAGE}",
            extra.to_string_lossy()
        ));
    }
    // Restricting linker arguments to library selection keeps a build from
    // being redirected or recompiled differently through this door.
    if !link_flags.is_empty() && !action.links() {
        return Command::Misuse(format!(
            "linker arguments are only meaningful for `build` and `run`\n{USAGE}"
        ));
    }
    if output.is_some() && action != Action::Build {
        return Command::Misuse(format!(
            "`-o` chooses where `build` writes its executable, and applies to nothing else\n{USAGE}"
        ));
    }
    if forwarding && action != Action::Run {
        return Command::Misuse(format!(
            "`--args` passes arguments to the program `run` executes, and applies to nothing else\n{USAGE}"
        ));
    }
    if check_only && action != Action::Fmt {
        return Command::Misuse(format!("`--check` is only meaningful for `fmt`\n{USAGE}"));
    }
    // A freestanding build produces an object file for something else to link,
    // so there is nothing for `run` to start and nothing for `test` to run.
    if freestanding && !matches!(action, Action::Check | Action::EmitC | Action::Build) {
        return Command::Misuse(format!(
            "`--freestanding` applies to `check`, `emit-c` and `build`; there is no program to run\n{USAGE}"
        ));
    }
    if freestanding && !link_flags.is_empty() {
        return Command::Misuse(format!(
            "a freestanding build links nothing; it writes an object file for your own linker\n{USAGE}"
        ));
    }
    Command::Invoke(Box::new(Invocation {
        action,
        file: PathBuf::from(file),
        link_flags,
        output,
        check_only,
        freestanding,
        program_arguments,
    }))
}

/// A close typo gets a suggestion; anything further apart gets the usage line
/// rather than a guess that would send the reader somewhere else.
fn nearest_command(given: &str) -> Option<&'static str> {
    let allowed = if given.chars().count() <= 3 { 1 } else { 2 };
    Action::NAMES
        .iter()
        .map(|(name, _)| *name)
        .map(|name| (skuld_compiler::diagnostic::edit_distance(given, name), name))
        .filter(|(distance, _)| *distance <= allowed)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name)
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let invocation = match parse_arguments(&arguments) {
        Command::Print(text) => return write_output(&text),
        Command::Misuse(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
        Command::Invoke(invocation) => *invocation,
    };
    let action = invocation.action;
    let link_flags = invocation.link_flags;
    let entry = invocation.file;
    let text = match fs::read_to_string(&entry) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: cannot read `{}`: {error}", entry.display());
            return ExitCode::FAILURE;
        }
    };
    // Import paths are relative to the directory the entry file lives in.
    // That directory is the program root; a module is a directory under it.
    let mut loader = Directories {
        root: entry.parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    let source = SourceFile::new(entry.to_string_lossy(), text);
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
        Action::Check if invocation.freestanding => skuld_compiler::check_program_with(
            &source.name,
            &source.text,
            &mut loader,
            skuld_compiler::type_checker::Entrypoint::Optional,
            skuld_compiler::type_checker::Mode::Freestanding,
        )
        .map(|_| String::new()),
        Action::Check => {
            check_program(&source.name, &source.text, &mut loader).map(|_| String::new())
        }
        Action::Fmt => skuld_compiler::formatter::format_source(&source.text)
            .map_err(|diagnostics| one_file(&source, diagnostics)),
        Action::EmitC | Action::Build | Action::Run => {
            let mode = if invocation.freestanding {
                skuld_compiler::type_checker::Mode::Freestanding
            } else {
                skuld_compiler::type_checker::Mode::Hosted
            };
            skuld_compiler::compile_program_to_c_in(&source.name, &source.text, &mut loader, mode)
        }
        // A test file is not a program until the runner writes its entry
        // point, so this arm compiles a source the user never wrote.
        Action::Test => {
            let parsed = parse(&source.text);
            match parsed.program {
                None => Err(one_file(&source, parsed.diagnostics)),
                Some(program) => match test_runner::discover(&program, &source.text) {
                    Err(reason) => {
                        eprintln!("error: {}: {reason}", entry.display());
                        return ExitCode::FAILURE;
                    }
                    Ok(suite) => {
                        match compile_program_to_c(&source.name, &suite.source, &mut loader) {
                            Err(errors) => Err(errors),
                            Ok(c_source) => {
                                return run_suite(&suite, &c_source, &link_flags);
                            }
                        }
                    }
                },
            }
        }
    };
    match result {
        Err(errors) => {
            eprint!("{}", errors.render());
            ExitCode::FAILURE
        }
        Ok(output) if matches!(action, Action::Fmt) => {
            if output == source.text {
                ExitCode::SUCCESS
            } else if invocation.check_only {
                ExitCode::FAILURE
            } else {
                if let Err(error) = fs::write(&entry, output) {
                    eprintln!("error: cannot write formatted output: {error}");
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
        }
        Ok(output) if matches!(action, Action::Run) => {
            match native::run(&output, &link_flags, &invocation.program_arguments) {
                Ok(code) => ExitCode::from(code),
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        // A freestanding build stops at an object file: there is no entry
        // point to link and nothing to run, and the linker script and the
        // target belong to whoever is assembling the thing this is part of.
        Ok(output) if matches!(action, Action::Build) && invocation.freestanding => {
            let object = match object_path(&entry, invocation.output.as_deref()) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match native::build_object(&output, &object) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Ok(output) if matches!(action, Action::Build) => {
            let executable = match executable_path(&entry, invocation.output.as_deref()) {
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
/// Build the suite, run it, and print the report. The status is the report's:
/// a failing test is a failing command, which is what a build script reads.
fn run_suite(suite: &test_runner::Suite, c_source: &str, link_flags: &[String]) -> ExitCode {
    match native::capture(c_source, link_flags) {
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
        Ok((code, out, err)) => {
            let outcome = test_runner::report(&suite.names, &out, code);
            print!("{}", outcome.report);
            if !err.is_empty() {
                eprint!("{err}");
            }
            if outcome.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

/// Without `-o` the executable lands in the working directory under the source
/// file's stem, so building never writes next to the source or into a directory
/// the user did not choose. With `-o` the user chose it, and the only rule kept
/// is that a build must not consume its own source.
fn executable_path(source: &Path, requested: Option<&Path>) -> Result<PathBuf, String> {
    let executable = match requested {
        Some(path) => {
            if path.as_os_str().is_empty() {
                return Err("`-o` needs a path to write the executable to".to_owned());
            }
            if path.is_dir() {
                return Err(format!(
                    "`{}` is a directory; `-o` names the executable itself",
                    path.display()
                ));
            }
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
                && !parent.is_dir()
            {
                return Err(format!(
                    "`{}` does not exist; create it before writing an executable into it",
                    parent.display()
                ));
            }
            path.to_path_buf()
        }
        None => {
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
            PathBuf::from(&name)
        }
    };
    // Without an extension the stem is the source itself; refuse rather than
    // overwrite the program being compiled.
    if same_file(&executable, source) {
        return Err(format!(
            "building `{}` would overwrite it; choose another path with `-o`, or rename the source to end in `.skuld`",
            source.display()
        ));
    }
    Ok(executable)
}

/// Where a freestanding build writes its object file: where `-o` says, or the
/// source's own name with `.o`.
fn object_path(source: &Path, requested: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = requested {
        return executable_path(source, Some(path));
    }
    let stem = source
        .file_stem()
        .ok_or_else(|| format!("`{}` has no file name to build from", source.display()))?;
    if stem.is_empty() {
        return Err(format!("`{}` has an empty file name", source.display()));
    }
    let mut name = stem.to_os_string();
    name.push(".o");
    let object = PathBuf::from(&name);
    if same_file(&object, source) {
        return Err(format!(
            "building `{}` would overwrite it; choose another path with `-o`",
            source.display()
        ));
    }
    Ok(object)
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
                .is_none_or(|extension| extension != "skuld" && extension != "sk")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Command {
        let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
        parse_arguments(&arguments)
    }
    fn invocation(arguments: &[&str]) -> Invocation {
        match parse(arguments) {
            Command::Invoke(invocation) => *invocation,
            other => panic!("{arguments:?} should be an invocation, got {other:?}"),
        }
    }
    fn misuse(arguments: &[&str]) -> String {
        match parse(arguments) {
            Command::Misuse(message) => message,
            other => panic!("{arguments:?} should be a misuse, got {other:?}"),
        }
    }

    #[test]
    fn a_flag_may_sit_on_either_side_of_the_file() {
        let expected = Invocation {
            action: Action::Run,
            file: PathBuf::from("program.skuld"),
            link_flags: vec!["-lm".to_owned()],
            output: None,
            check_only: false,
            freestanding: false,
            program_arguments: Vec::new(),
        };
        assert_eq!(invocation(&["run", "program.skuld", "-lm"]), expected);
        assert_eq!(invocation(&["run", "-lm", "program.skuld"]), expected);
        assert_eq!(invocation(&["-lm", "run", "program.skuld"]), expected);
    }

    #[test]
    fn freestanding_applies_to_the_commands_that_compile() {
        let expected = Invocation {
            action: Action::Build,
            file: PathBuf::from("kernel.skuld"),
            link_flags: Vec::new(),
            output: Some(PathBuf::from("kernel.o")),
            check_only: false,
            freestanding: true,
            program_arguments: Vec::new(),
        };
        assert_eq!(
            invocation(&["build", "--freestanding", "kernel.skuld", "-o", "kernel.o"]),
            expected
        );
        // There is no program to start and nothing to link.
        assert!(misuse(&["run", "--freestanding", "kernel.skuld"]).contains("no program to run"));
        assert!(
            misuse(&["build", "--freestanding", "kernel.skuld", "-lm"]).contains("links nothing")
        );
    }

    #[test]
    fn fmt_invocation_supports_check_flag() {
        let expected = Invocation {
            action: Action::Fmt,
            file: PathBuf::from("program.skuld"),
            link_flags: Vec::new(),
            output: None,
            check_only: true,
            freestanding: false,
            program_arguments: Vec::new(),
        };
        assert_eq!(invocation(&["fmt", "--check", "program.skuld"]), expected);
        assert_eq!(invocation(&["fmt", "program.skuld", "--check"]), expected);

        let uncheck = Invocation {
            action: Action::Fmt,
            file: PathBuf::from("program.skuld"),
            link_flags: Vec::new(),
            output: None,
            check_only: false,
            freestanding: false,
            program_arguments: Vec::new(),
        };
        assert_eq!(invocation(&["fmt", "program.skuld"]), uncheck);
        assert!(misuse(&["run", "program.skuld", "--check"]).contains("`--check`"));
    }

    #[test]
    fn args_hands_the_rest_to_the_program() {
        // Everything after `--args` belongs to the program, including things
        // this command line would otherwise read as its own.
        let parsed = invocation(&["run", "program.skuld", "--args", "-o", "--", "file.json"]);
        assert_eq!(parsed.action, Action::Run);
        assert_eq!(parsed.file, PathBuf::from("program.skuld"));
        assert!(parsed.output.is_none());
        assert_eq!(
            parsed.program_arguments,
            vec![
                OsString::from("-o"),
                OsString::from("--"),
                OsString::from("file.json")
            ]
        );
        // An empty list is still a forwarded list, not an error.
        assert!(
            invocation(&["run", "program.skuld", "--args"])
                .program_arguments
                .is_empty()
        );
        // Nothing else runs a program, so nothing else may forward to one.
        assert!(misuse(&["build", "program.skuld", "--args", "x"]).contains("`--args`"));
        // After `--`, it is a path like anything else — and a second one.
        assert!(misuse(&["run", "--", "program.skuld", "--args"]).contains("one file at a time"));
    }

    #[test]
    fn a_separator_ends_the_flags() {
        // Without `--`, a file whose name begins with a dash is read as an
        // option: `-odd-name.skuld` is `-o` with a joined value, the way every
        // C compiler reads it. That ambiguity is exactly what `--` resolves.
        let parsed = invocation(&["check", "--", "-odd-name.skuld"]);
        assert_eq!(parsed.file, PathBuf::from("-odd-name.skuld"));
        assert!(parsed.link_flags.is_empty());
        let swallowed = misuse(&["check", "-odd-name.skuld"]);
        assert!(
            swallowed.contains("`-o` took `dd-name.skuld`"),
            "{swallowed}"
        );
        assert!(swallowed.contains("after `--`"), "{swallowed}");
        assert!(misuse(&["check", "-weird.skuld"]).contains("unknown option"));
    }

    #[test]
    fn an_output_path_is_taken_joined_or_separate() {
        for arguments in [
            &["build", "program.skuld", "-o", "bin/program"][..],
            &["build", "program.skuld", "-obin/program"][..],
            &["build", "-o", "bin/program", "program.skuld"][..],
            &["build", "program.skuld", "--output", "bin/program"][..],
        ] {
            let parsed = invocation(arguments);
            assert_eq!(parsed.output, Some(PathBuf::from("bin/program")));
            assert_eq!(parsed.file, PathBuf::from("program.skuld"));
        }
        assert!(misuse(&["build", "program.skuld", "-o"]).contains("needs a path"));
    }

    #[test]
    fn an_option_is_refused_where_it_would_mean_nothing() {
        // Silently ignoring one would hide a mistake the user is about to repeat.
        assert!(misuse(&["check", "program.skuld", "-lm"]).contains("`build` and `run`"));
        assert!(misuse(&["lex", "program.skuld", "-L/opt/lib"]).contains("`build` and `run`"));
        assert!(misuse(&["run", "program.skuld", "-o", "program"]).contains("`-o`"));
        assert!(misuse(&["run", "program.skuld", "-O2"]).contains("unknown option"));
        // A bare `-l` names no library.
        assert!(misuse(&["run", "program.skuld", "-l"]).contains("unknown option"));
    }

    #[test]
    fn help_and_version_answer_before_anything_else_is_judged() {
        for arguments in [
            &["--help"][..],
            &["-h"][..],
            &["nonsense", "--help"][..],
            &["build", "--help", "-o"][..],
        ] {
            let Command::Print(text) = parse(arguments) else {
                panic!("{arguments:?} should print help");
            };
            assert!(text.contains("commands:"), "{arguments:?}");
        }
        for arguments in [
            &["--version"][..],
            &["-V"][..],
            &["run", "-V", "x.skuld"][..],
        ] {
            let Command::Print(text) = parse(arguments) else {
                panic!("{arguments:?} should print the version");
            };
            assert!(text.starts_with("skuld "), "{arguments:?}");
        }
    }

    #[test]
    fn a_missing_or_mistyped_command_says_what_to_do() {
        assert!(misuse(&[]).contains("no command given"));
        assert!(misuse(&["run"]).contains("needs a file"));
        assert!(misuse(&["run", "a.skuld", "b.skuld"]).contains("one file at a time"));
        assert!(misuse(&["buidl", "x.skuld"]).contains("did you mean `build`?"));
        assert!(misuse(&["chekc", "x.skuld"]).contains("did you mean `check`?"));
        // A word that resembles nothing gets no guess.
        let far = misuse(&["frobnicate", "x.skuld"]);
        assert!(far.contains("unknown command"), "{far}");
        assert!(!far.contains("did you mean"), "{far}");
    }

    #[test]
    fn a_swap_of_two_letters_is_one_mistake() {
        assert_eq!(
            skuld_compiler::diagnostic::edit_distance("build", "build"),
            0
        );
        assert_eq!(
            skuld_compiler::diagnostic::edit_distance("buidl", "build"),
            1
        );
        assert_eq!(
            skuld_compiler::diagnostic::edit_distance("biuld", "build"),
            1
        );
        assert_eq!(
            skuld_compiler::diagnostic::edit_distance("bild", "build"),
            1
        );
        assert_eq!(skuld_compiler::diagnostic::edit_distance("", "run"), 3);
        assert_eq!(
            skuld_compiler::diagnostic::edit_distance("emit-c", "emit-c"),
            0
        );
    }

    #[test]
    fn an_output_path_is_the_users_to_choose_but_never_the_source() {
        let source = Path::new("program.skuld");
        assert_eq!(
            executable_path(source, None).expect("default"),
            PathBuf::from(if cfg!(windows) {
                "program.exe"
            } else {
                "program"
            })
        );
        // `-o` may point anywhere the user can write: unlike a module path, it
        // carries no code into the build and was typed on purpose.
        assert_eq!(
            executable_path(source, Some(Path::new("../program"))).expect("explicit"),
            PathBuf::from("../program")
        );
        let directory = executable_path(source, Some(Path::new(".")));
        assert!(
            directory.is_err_and(|message| message.contains("is a directory")),
            "a directory is not an executable"
        );
        let missing = executable_path(source, Some(Path::new("no/such/place/program")));
        assert!(
            missing.is_err_and(|message| message.contains("does not exist")),
            "a missing parent is reported before clang is launched"
        );
    }
}
