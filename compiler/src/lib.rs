//! Skuld's compiler foundation. Lexing is independent of parsing and semantics.
pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod module;
pub mod parser;
pub mod resolver;
pub mod span;
pub mod std_lib;
pub mod token;

pub use lexer::{LexOutput, lex};

pub use parser::{ParseOutput, parse};

pub use resolver::{ResolveOutput, resolve};

pub mod formatter;
pub use formatter::format_source;

pub mod type_checker;
pub mod types;

/// Load, resolve and type-check a whole program: the entry file, and every
/// module it imports, through the caller's loader.
pub fn check_program(
    entry_name: &str,
    entry_source: &str,
    loader: &mut dyn module::ModuleLoader,
) -> Result<type_checker::TypedProgram, module::Errors> {
    let program = module::load(entry_name, entry_source, loader)?;
    let resolved = resolve(&program);
    let Some(resolution) = resolved.resolution else {
        return Err(module::Errors {
            sources: program.sources(),
            diagnostics: resolved.diagnostics,
        });
    };
    type_checker::type_check(program, resolution)
}

/// Parse, resolve and type-check a program that is exactly one source, with
/// no root directory and so no imports.
pub fn check(source: &str) -> Result<type_checker::TypedProgram, Vec<diagnostic::Diagnostic>> {
    check_program("<source>", source, &mut module::NoModules).map_err(|errors| {
        errors
            .diagnostics
            .into_iter()
            .map(|d| d.diagnostic)
            .collect()
    })
}

pub mod hir;
pub mod lowering;

pub mod codegen_c;

/// Run every compiler stage and return C without invoking external tools.
pub fn compile_to_c(source: &str) -> Result<String, Vec<diagnostic::Diagnostic>> {
    let typed = check(source)?;
    let hir = lowering::lower(typed);
    Ok(codegen_c::emit_c(&hir))
}

/// The same, for a program made of modules.
pub fn compile_program_to_c(
    entry_name: &str,
    entry_source: &str,
    loader: &mut dyn module::ModuleLoader,
) -> Result<String, module::Errors> {
    let typed = check_program(entry_name, entry_source, loader)?;
    let hir = lowering::lower(typed);
    Ok(codegen_c::emit_c(&hir))
}
