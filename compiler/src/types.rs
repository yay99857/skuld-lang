//! Semantic types, independent of source spellings and backend representations.
use std::fmt;

/// Index into the checked program's struct table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StructId(pub usize);

/// Index into the checked program's array table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArrayId(pub usize);

/// Index into the checked program's option table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OptionId(pub usize);

/// Index into the checked program's result table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResultId(pub usize);

/// Index into the checked program's enum table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnumId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionInfo {
    pub element: Type,
}

/// A builtin `Result<T, E>`. `Ok` is tag 0 and `Err` is tag 1 everywhere,
/// which is what lets matching reuse the enum machinery unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultInfo {
    pub ok: Type,
    pub err: Type,
}

impl ResultInfo {
    pub const OK: usize = 0;
    pub const ERR: usize = 1;
    pub fn payload(&self, variant_index: usize) -> Type {
        if variant_index == Self::OK {
            self.ok
        } else {
            self.err
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayInfo {
    pub element: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumInfo {
    pub name: String,
    pub variants: Vec<VariantInfo>,
}

impl EnumInfo {
    pub fn find_variant(&self, name: &str) -> Option<usize> {
        self.variants.iter().position(|v| v.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    pub name: String,
    pub payload: Option<Type>,
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
    /// A user-declared discriminated union / enum.
    Enum(EnumId),
    /// A reference-counted heap array.
    Array(ArrayId),
    /// An inline discriminated optional value.
    Option(OptionId),
    /// An inline discriminated success-or-error value.
    Result(ResultId),
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
            Self::Enum(_) => "<enum>",
            Self::Array(_) => "<array>",
            Self::Option(_) => "<option>",
            Self::Result(_) => "<result>",
            Self::Weak(_) => "<weak>",
            Self::Error => "<error>",
        })
    }
}
