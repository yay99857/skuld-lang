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

/// Index into the checked program's interface table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceId(pub usize);

/// A named abstraction over classes. It carries signatures only; the bodies
/// belong to the classes that declare they implement it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub name: String,
    pub module: crate::module::ModuleId,
    pub visibility: crate::ast::Visibility,
    pub methods: Vec<InterfaceMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceMethod {
    pub name: String,
    pub parameters: Vec<Type>,
    pub return_type: Type,
}

/// Index into the checked program's function-type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionTypeId(pub usize);

/// The signature a function value carries. It is a type, not a declaration:
/// two lambdas with the same parameters and result have the same type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionTypeInfo {
    pub parameters: Vec<Type>,
    pub return_type: Type,
}

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
    /// The module that declares it, and whether it leaves that module.
    pub module: crate::module::ModuleId,
    pub visibility: crate::ast::Visibility,
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

/// A machine integer width and signedness. `int` is a spelling of `I64`, so
/// both name the same type rather than one converting to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

impl IntType {
    pub const ALL: [Self; 8] = [
        Self::I8,
        Self::I16,
        Self::I32,
        Self::I64,
        Self::U8,
        Self::U16,
        Self::U32,
        Self::U64,
    ];
    /// The canonical name used in diagnostics. `I64` renders as `int`, the
    /// spelling the language leads with.
    pub fn name(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "int",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }
    /// The suffix used to build generated C helper names.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            other => other.name(),
        }
    }
    pub fn c_type(self) -> &'static str {
        match self {
            Self::I8 => "int8_t",
            Self::I16 => "int16_t",
            Self::I32 => "int32_t",
            Self::I64 => "int64_t",
            Self::U8 => "uint8_t",
            Self::U16 => "uint16_t",
            Self::U32 => "uint32_t",
            Self::U64 => "uint64_t",
        }
    }
    pub fn signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }
    pub fn bits(self) -> u32 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 => 64,
        }
    }
    /// The largest literal magnitude this type accepts without a leading `-`.
    pub fn max_magnitude(self) -> u64 {
        if self.signed() {
            (1_u64 << (self.bits() - 1)) - 1
        } else if self.bits() == 64 {
            u64::MAX
        } else {
            (1_u64 << self.bits()) - 1
        }
    }
    /// The magnitude of the most negative value, which only a unary minus
    /// applied directly to a literal may name.
    pub fn min_magnitude(self) -> u64 {
        if self.signed() {
            1_u64 << (self.bits() - 1)
        } else {
            0
        }
    }
}

/// What a raw pointer points at. Only unmanaged, C-representable values are
/// possible: a pointer never carries a reference count across the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pointee {
    /// `*void`: an opaque handle, which Skuld can only pass back to C.
    Void,
    Int(IntType),
    Float,
    Bool,
}

impl Pointee {
    pub fn name(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Int(kind) => kind.name(),
            Self::Float => "float",
            Self::Bool => "bool",
        }
    }
    pub fn c_type(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Int(kind) => kind.c_type(),
            Self::Float => "double",
            Self::Bool => "bool",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Int(IntType),
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
    /// A raw, unmanaged pointer. It exists for the `extern "C"` boundary and
    /// keeps nothing alive; Skuld cannot read or write through it.
    Pointer(Pointee),
    /// A function value: a parameter or a local, never stored anywhere a
    /// managed value could reach it, so it never allocates and never retains.
    Function(FunctionTypeId),
    /// A class reference seen through an interface: the object and the table
    /// of methods to call on it. Counted like the class it holds.
    Interface(InterfaceId),
    /// Recovery only; never present in a successfully checked program.
    Error,
}
impl Type {
    /// The platform-independent default integer, `int`, which is `i64`.
    pub const INT: Self = Self::Int(IntType::I64);
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Int(_) | Self::Float)
    }
    pub fn int_type(self) -> Option<IntType> {
        match self {
            Self::Int(kind) => Some(kind),
            _ => None,
        }
    }
}
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Pointer(pointee) = self {
            return write!(f, "*{}", pointee.name());
        }
        f.write_str(match self {
            Self::Int(kind) => kind.name(),
            Self::Float => "float",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Void => "void",
            // Only the checker knows struct names; it renders them itself.
            Self::Struct(_) => "<struct>",
            Self::Enum(_) => "<enum>",
            Self::Array(_) => "<array>",
            // A signature lives in the checker's table too, which `Display`
            // cannot reach; `type_name` renders it in full.
            Self::Function(_) => "<function>",
            Self::Interface(_) => "<interface>",
            Self::Option(_) => "<option>",
            Self::Result(_) => "<result>",
            Self::Weak(_) => "<weak>",
            Self::Pointer(_) => unreachable!("rendered above"),
            Self::Error => "<error>",
        })
    }
}
