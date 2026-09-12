//! Skuld's compiler foundation. Lexing is independent of parsing and semantics.
pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod span;
pub mod token;

pub use lexer::{LexOutput, lex};

pub use parser::{ParseOutput, parse};

pub use resolver::{ResolveOutput, resolve};

pub mod type_checker;
pub mod types;

/// Parse, resolve and type-check a complete single-file executable.
pub fn check(source: &str) -> Result<type_checker::TypedProgram, Vec<diagnostic::Diagnostic>> {
    let parsed = parse(source);
    let Some(program) = parsed.program else {
        return Err(parsed.diagnostics);
    };
    let resolved = resolve(&program);
    let Some(resolution) = resolved.resolution else {
        return Err(resolved.diagnostics);
    };
    type_checker::type_check(program, resolution)
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
