//! Semantic types, independent of source spellings and backend representations.
use std::fmt;

/// Index into the checked program's struct table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StructId(pub usize);

/// Index into the checked program's array table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArrayId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OptionId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionInfo {
    pub element: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayInfo {
    pub element: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Void,
    /// A declared struct or class; its table entry determines value/reference semantics.
    Struct(StructId),
    /// A reference-counted heap array.
    Array(ArrayId),
    /// An inline discriminated optional value.
    Option(OptionId),
    /// A non-owning class reference, which may be empty or expired.
    Weak(StructId),
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
            Self::Array(_) => "<array>",
            Self::Option(_) => "<option>",
            Self::Weak(_) => "<weak>",
            Self::Error => "<error>",
        })
    }
}
