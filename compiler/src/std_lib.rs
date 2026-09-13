//! The standard library, embedded in the compiler binary.
//!
//! `std` is a reserved import prefix, not a directory: `import "std/utf8"`
//! resolves here and never reaches the caller's [`ModuleLoader`], so a program
//! compiles the same from any working directory, with no installation step and
//! nothing on disk that could shadow it. A user directory named `std` is
//! therefore unreachable, which is a language rule rather than a filesystem
//! accident and is why the reservation lives in the compiler.
//!
//! The cost of embedding is that changing the library means rebuilding the
//! compiler. That is the right trade while the library is small and moves with
//! the language.
//!
//! [`ModuleLoader`]: crate::module::ModuleLoader

/// The reserved first path segment.
pub const PREFIX: &str = "std";

/// Every module, with its files in the order they are compiled. A module's
/// files share one namespace, so the order is a filing decision only.
const MODULES: &[(&str, &[(&str, &str)])] = &[
    (
        "std/cstring",
        &[(
            "std/cstring/cstring.skuld",
            include_str!("../../std/cstring/cstring.skuld"),
        )],
    ),
    (
        "std/fs",
        &[("std/fs/fs.skuld", include_str!("../../std/fs/fs.skuld"))],
    ),
    (
        "std/http",
        &[(
            "std/http/client.skuld",
            include_str!("../../std/http/client.skuld"),
        )],
    ),
    (
        "std/json",
        &[(
            "std/json/json.skuld",
            include_str!("../../std/json/json.skuld"),
        )],
    ),
    (
        "std/net",
        &[(
            "std/net/socket.skuld",
            include_str!("../../std/net/socket.skuld"),
        )],
    ),
    (
        "std/strings",
        &[(
            "std/strings/strings.skuld",
            include_str!("../../std/strings/strings.skuld"),
        )],
    ),
    (
        "std/utf8",
        &[(
            "std/utf8/decode.skuld",
            include_str!("../../std/utf8/decode.skuld"),
        )],
    ),
];

/// Whether a path is reserved, whether or not it names a real module. A
/// reserved path that names nothing is an error, never a fallback to disk.
pub fn is_reserved(path: &str) -> bool {
    path == PREFIX || path.starts_with("std/")
}

/// The sources of one standard library module.
pub fn module(path: &str) -> Option<Vec<(String, String)>> {
    MODULES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, files)| {
            files
                .iter()
                .map(|(name, source)| ((*name).to_owned(), (*source).to_owned()))
                .collect()
        })
}

/// Every module path, for a diagnostic that has to say what does exist.
pub fn paths() -> Vec<&'static str> {
    MODULES.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests;
