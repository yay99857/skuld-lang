//! Semantic types, independent of source spellings and backend representations.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Void,
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
            Self::Error => "<error>",
        })
    }
}
