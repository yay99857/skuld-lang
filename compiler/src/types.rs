//! Semantic types, independent of source spellings and backend representations.
use std::fmt;

/// Index into the checked program's struct table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StructId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Void,
    /// A value-semantics record; copied on assignment and argument passing.
    Struct(StructId),
    /// Recovery only; never present in a successfully checked program.
    Error,
}
impl Type {
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::Float)
    }
}
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Void => "void",
            // Only the checker knows struct names; it renders them itself.
            Self::Struct(_) => "<struct>",
            Self::Error => "<error>",
        })
    }
}
