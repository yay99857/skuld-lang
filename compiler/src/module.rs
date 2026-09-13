//! Turning an entry file into the set of modules that make up one program.
//!
//! A module is a directory: every `.skuld` file in it shares one namespace, so
//! splitting a module across files is a filing decision, not a semantic one.
//! The root module is the entry file named on the command line, and every
//! import path is relative to the directory that file lives in.
//!
//! Nothing here touches the filesystem. The caller supplies a [`ModuleLoader`],
//! which keeps the compiler library free of I/O while leaving path syntax,
//! discovery order, duplicate qualifiers and import cycles — all language
//! rules, all wanting spans — on this side of the boundary.
use crate::{
    ast::Program,
    diagnostic::{Diagnostic, DiagnosticCode},
    parser::parse,
    span::{SourceFile, Span},
    std_lib,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleId(pub usize);

/// The root module, which holds the entry file.
pub const ROOT: ModuleId = ModuleId(0);

/// A diagnostic and the file it belongs to. Spans stay byte offsets into their
/// own file, so only this pairing says which source to render against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiagnostic {
    pub file: FileId,
    pub diagnostic: Diagnostic,
}

#[derive(Debug)]
pub struct LoadedFile {
    /// How the file is named in diagnostics, e.g. `json/parser.skuld`.
    pub name: String,
    pub source: String,
    pub module: ModuleId,
    pub program: Program,
}

#[derive(Debug)]
pub struct Module {
    /// The import path, empty for the root module.
    pub path: String,
    /// The last path segment, which is how importers spell this module.
    /// Empty for the root, which nothing can import.
    pub qualifier: String,
    pub files: Vec<FileId>,
}

#[derive(Debug)]
pub struct LoadedProgram {
    pub files: Vec<LoadedFile>,
    pub modules: Vec<Module>,
}

impl LoadedProgram {
    pub fn file(&self, id: FileId) -> &LoadedFile {
        &self.files[id.0]
    }
    /// One renderable source per file, in `FileId` order.
    pub fn sources(&self) -> Vec<SourceFile> {
        self.files
            .iter()
            .map(|file| SourceFile::new(file.name.clone(), file.source.clone()))
            .collect()
    }
}

/// Everything needed to report a failed compilation: the diagnostics, and the
/// sources they point into. A span is an offset in its own file, so the two
/// only mean something together.
#[derive(Debug)]
pub struct Errors {
    pub sources: Vec<SourceFile>,
    pub diagnostics: Vec<FileDiagnostic>,
}

impl Errors {
    pub fn render(&self) -> String {
        let mut out = String::new();
        for entry in &self.diagnostics {
            match self.sources.get(entry.file.0) {
                Some(source) => out.push_str(&entry.diagnostic.render(source)),
                // A file with no source would be an internal bug; report the
                // diagnostic rather than dropping it.
                None => out.push_str(&format!(
                    "{}: {}\n",
                    entry.diagnostic.code.as_str(),
                    entry.diagnostic.message
                )),
            }
        }
        out
    }
}

/// Where module sources come from. The compiler asks for a path; the caller
/// answers with every `.skuld` file of that module, or a reason it cannot.
pub trait ModuleLoader {
    fn load(&mut self, path: &str) -> Result<Vec<(String, String)>, String>;
}

/// A loader for a program that is exactly one file. Any import is an error,
/// since there is no root directory to resolve it against.
pub struct NoModules;

impl ModuleLoader for NoModules {
    fn load(&mut self, path: &str) -> Result<Vec<(String, String)>, String> {
        Err(format!(
            "`{path}` cannot be resolved: this program is a single source with no root directory"
        ))
    }
}

/// Parse the entry file, then every module it reaches, and report the language
/// rules that only a whole program can break.
pub fn load(
    entry_name: &str,
    entry_source: &str,
    loader: &mut dyn ModuleLoader,
) -> Result<LoadedProgram, Errors> {
    let mut state = Loader {
        files: Vec::new(),
        modules: vec![Module {
            path: String::new(),
            qualifier: String::new(),
            files: Vec::new(),
        }],
        by_path: BTreeMap::new(),
        edges: vec![Vec::new()],
        diagnostics: Vec::new(),
    };
    state.add_file(ROOT, entry_name.to_owned(), entry_source.to_owned());
    // Breadth-first, so a module is loaded once however many files import it.
    let mut next = 0;
    while next < state.modules.len() {
        let module = ModuleId(next);
        state.resolve_imports(module, loader);
        next += 1;
    }
    state.detect_cycles();
    let program = LoadedProgram {
        files: state.files,
        modules: state.modules,
    };
    if state.diagnostics.is_empty() {
        Ok(program)
    } else {
        Err(Errors {
            sources: program.sources(),
            diagnostics: state.diagnostics,
        })
    }
}

struct Loader {
    files: Vec<LoadedFile>,
    modules: Vec<Module>,
    by_path: BTreeMap<String, ModuleId>,
    /// One entry per module: the modules it imports, with the import that
    /// created the edge, so a cycle can be reported where it is written.
    edges: Vec<Vec<Edge>>,
    diagnostics: Vec<FileDiagnostic>,
}

struct Edge {
    to: ModuleId,
    file: FileId,
    span: Span,
}

impl Loader {
    fn add_file(&mut self, module: ModuleId, name: String, source: String) {
        let id = FileId(self.files.len());
        let output = parse(&source);
        for diagnostic in output.diagnostics {
            self.diagnostics.push(FileDiagnostic {
                file: id,
                diagnostic,
            });
        }
        // A file that did not parse still occupies an id, so that diagnostics
        // already recorded against it keep pointing at the right source.
        let program = output.program.unwrap_or_else(|| Program {
            imports: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            functions: Vec::new(),
            externs: Vec::new(),
            span: Span::new(0, source.len()),
        });
        self.files.push(LoadedFile {
            name,
            source,
            module,
            program,
        });
        self.modules[module.0].files.push(id);
    }

    fn resolve_imports(&mut self, module: ModuleId, loader: &mut dyn ModuleLoader) {
        let files = self.modules[module.0].files.clone();
        for file in files {
            // Two paths whose last segments agree would bind one qualifier
            // twice, and the second use would silently win.
            let mut qualifiers: BTreeMap<String, String> = BTreeMap::new();
            let imports = self.files[file.0].program.imports.clone();
            for import in imports {
                if let Some(first) = qualifiers.get(&import.qualifier.text) {
                    let first = first.clone();
                    self.diagnostics.push(FileDiagnostic {
                        file,
                        diagnostic: Diagnostic {
                            code: DiagnosticCode::DuplicateDeclaration,
                            message: format!(
                                "`{}` is already the qualifier of another import in this file",
                                import.qualifier.text
                            ),
                            span: import.path_span,
                            help: Some(format!(
                                "`{first}` ends in the same segment, and a module is \
                                 spelled by its last one; import only one of them here"
                            )),
                        },
                    });
                    continue;
                }
                qualifiers.insert(import.qualifier.text.clone(), import.path.clone());
                let target = self.module_for(&import.path, file, import.path_span, loader);
                if let Some(target) = target {
                    self.edges[module.0].push(Edge {
                        to: target,
                        file,
                        span: import.path_span,
                    });
                }
            }
        }
    }

    fn module_for(
        &mut self,
        path: &str,
        file: FileId,
        span: Span,
        loader: &mut dyn ModuleLoader,
    ) -> Option<ModuleId> {
        if let Some(id) = self.by_path.get(path) {
            return Some(*id);
        }
        // The standard library is reserved: it is served from the compiler
        // binary and the loader never sees the path, so no directory on disk
        // can shadow or provide it.
        let loaded = if std_lib::is_reserved(path) {
            std_lib::module(path).ok_or_else(|| {
                format!(
                    "`{}` is a reserved prefix for the standard library, which has no module `{path}`",
                    std_lib::PREFIX
                )
            })
        } else {
            loader.load(path)
        };
        let sources = match loaded {
            Ok(sources) if sources.is_empty() => {
                self.diagnostics.push(FileDiagnostic {
                    file,
                    diagnostic: Diagnostic {
                        code: DiagnosticCode::UnknownModule,
                        message: format!("module `{path}` contains no `.skuld` files"),
                        span,
                        help: Some("a module is a directory of source files".into()),
                    },
                });
                return None;
            }
            Ok(sources) => sources,
            Err(reason) => {
                let help = if std_lib::is_reserved(path) {
                    format!(
                        "the standard library ships with the compiler; its modules are {}",
                        std_lib::paths().join(", ")
                    )
                } else {
                    "an import path names a directory relative to the program root".to_owned()
                };
                self.diagnostics.push(FileDiagnostic {
                    file,
                    diagnostic: Diagnostic {
                        code: DiagnosticCode::UnknownModule,
                        message: format!("cannot import `{path}`: {reason}"),
                        span,
                        help: Some(help),
                    },
                });
                return None;
            }
        };
        let id = ModuleId(self.modules.len());
        let qualifier = path.rsplit('/').next().unwrap_or(path).to_owned();
        self.modules.push(Module {
            path: path.to_owned(),
            qualifier,
            files: Vec::new(),
        });
        self.edges.push(Vec::new());
        self.by_path.insert(path.to_owned(), id);
        for (name, source) in sources {
            self.add_file(id, name, source);
        }
        Some(id)
    }

    /// A cycle would leave no order in which modules could be checked, and
    /// nothing in the language needs one, so it is rejected where it is written
    /// rather than worked around.
    fn detect_cycles(&mut self) {
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            New,
            Active,
            Done,
        }
        let mut states = vec![State::New; self.modules.len()];
        let mut reported: Vec<(FileId, usize)> = Vec::new();
        // An explicit stack, so a deep import graph cannot overflow the real
        // one the way a recursive walk could.
        let mut stack: Vec<(ModuleId, usize)> = vec![(ROOT, 0)];
        states[ROOT.0] = State::Active;
        while let Some((module, index)) = stack.pop() {
            if index >= self.edges[module.0].len() {
                states[module.0] = State::Done;
                continue;
            }
            stack.push((module, index + 1));
            let edge = &self.edges[module.0][index];
            let (to, file, span) = (edge.to, edge.file, edge.span);
            match states[to.0] {
                State::Active => {
                    if !reported.contains(&(file, span.start)) {
                        reported.push((file, span.start));
                        self.diagnostics.push(FileDiagnostic {
                            file,
                            diagnostic: Diagnostic {
                                code: DiagnosticCode::ImportCycle,
                                message: format!(
                                    "importing `{}` closes a cycle back to `{}`",
                                    self.modules[to.0].path,
                                    match self.modules[module.0].path.as_str() {
                                        "" => "the program root",
                                        path => path,
                                    }
                                ),
                                span,
                                help: Some(
                                    "move the shared declarations into a module both can import"
                                        .into(),
                                ),
                            },
                        });
                    }
                }
                State::New => {
                    states[to.0] = State::Active;
                    stack.push((to, 0));
                }
                State::Done => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
