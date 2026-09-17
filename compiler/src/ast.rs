//! Source-level syntax only: no token kinds, resolved symbols or inferred types.
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// `import "net/socket"`, in source order, always before any declaration.
    pub imports: Vec<ImportDecl>,
    pub interfaces: Vec<InterfaceDecl>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub constants: Vec<ConstantDecl>,
    pub functions: Vec<FunctionDecl>,
    /// Foreign declarations, which have signatures but no bodies.
    pub externs: Vec<ExternBlock>,
    pub span: Span,
}

/// `import "net/socket"`. The path names a directory of `.skuld` files
/// relative to the program root; its last segment is the qualifier the
/// importing file uses, so `net/socket` is spelled `socket.connect(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub path: String,
    /// The string literal, for diagnostics about the path itself.
    pub path_span: Span,
    /// The last path segment, which is the name bound in this file.
    pub qualifier: Name,
    pub span: Span,
}

/// Whether a declaration leaves its module. Visibility is written, never
/// inferred from spelling, and it applies to the whole declaration: a public
/// type carries its fields and methods with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

/// A name that may be qualified by an imported module: `parse` or
/// `json.parse`. An unqualified path names something in the current module or
/// the prelude; there is no unqualified access to another module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub module: Option<Name>,
    pub name: Name,
    pub span: Span,
}

impl Path {
    pub fn bare(name: Name) -> Self {
        let span = name.span;
        Self {
            module: None,
            name,
            span,
        }
    }
}

/// `unsafe extern "C" { ... }`. The `unsafe` marker is the source-level record
/// that the declared signatures are asserted, not checked: nothing in Skuld can
/// verify them against the library that is eventually linked.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    pub abi: String,
    pub abi_span: Span,
    pub functions: Vec<ExternFunctionDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternFunctionDecl {
    pub name: Name,
    pub parameters: Vec<Parameter>,
    /// None means an implicit void return type.
    pub return_type: Option<TypeRef>,
    pub span: Span,
}

/// `interface Renderer { render(value: int) -> string }`. Signatures only: a
/// method body belongs to the class that declares it implements this.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    pub visibility: Visibility,
    pub name: Name,
    pub methods: Vec<MethodSignature>,
    pub span: Span,
}

/// A method signature with no body. `this` is implicit, as it is on a class.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodSignature {
    pub name: Name,
    pub parameters: Vec<Parameter>,
    /// None means an implicit void return type.
    pub return_type: Option<TypeRef>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub visibility: Visibility,
    pub name: Name,
    pub variants: Vec<VariantDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantDecl {
    pub name: Name,
    pub payload: Option<TypeRef>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDeclKind {
    /// `struct`: copied on assignment, no identity.
    Value,
    /// `class`: a reference to a shared, reference-counted object.
    Reference,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub visibility: Visibility,
    pub kind: TypeDeclKind,
    pub name: Name,
    /// `class User: Printable, Comparable`. Conformance is declared here
    /// rather than inferred from the methods that happen to be present.
    pub conforms: Vec<Path>,
    pub fields: Vec<FieldDecl>,
    /// Declared without `func` and without an explicit receiver; `this` is
    /// bound implicitly inside the body.
    pub methods: Vec<FunctionDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: Name,
    pub type_ref: TypeRef,
    /// `name: Type = expression`. A field with a default may be left out of a
    /// construction, and the expression is evaluated there, once per object.
    pub default: Option<Expr>,
    pub span: Span,
}

impl FieldDecl {
    pub fn type_ref_span(&self) -> Span {
        self.type_ref.span()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

/// A source type name, to be resolved to a semantic type in a later phase.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    Named(Path),
    Option {
        element: Box<TypeRef>,
        span: Span,
    },
    Result {
        ok: Box<TypeRef>,
        err: Box<TypeRef>,
        span: Span,
    },
    Weak {
        class: Path,
        span: Span,
    },
    Array {
        element: Box<TypeRef>,
        span: Span,
    },
    /// `[N]T`. A fixed-size array with value semantics.
    FixedArray {
        element: Box<TypeRef>,
        size: Box<Expr>,
        span: Span,
    },
    /// `func(int, int) -> int`. A function value: an argument or a local, never
    /// something a managed value can hold.
    Function {
        parameters: Vec<TypeRef>,
        /// None means an implicit void return type.
        return_type: Option<Box<TypeRef>>,
        span: Span,
    },
    /// `*u8`, `*void`. Raw, unmanaged, and only valid at the foreign boundary.
    Pointer {
        pointee: Box<TypeRef>,
        span: Span,
    },
}

impl TypeRef {
    pub fn span(&self) -> Span {
        match self {
            Self::Named(path) => path.span,
            Self::Array { span, .. }
            | Self::FixedArray { span, .. }
            | Self::Function { span, .. }
            | Self::Pointer { span, .. }
            | Self::Weak { span, .. }
            | Self::Option { span, .. }
            | Self::Result { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstantDecl {
    pub visibility: Visibility,
    pub name: Name,
    pub type_ref: Option<TypeRef>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub visibility: Visibility,
    pub name: Name,
    pub parameters: Vec<Parameter>,
    /// None means an implicit void return type.
    pub return_type: Option<TypeRef>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: Name,
    pub type_ref: TypeRef,
    pub span: Span,
}

/// `func(a: int, b: int) -> int { ... }`: a function declaration without a
/// name. It is an expression, so it has no visibility and no symbol of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct Lambda {
    pub parameters: Vec<LambdaParameter>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
    pub is_expression: bool,
    pub span: Span,
}

/// A lambda parameter, whose type the expected function type may supply.
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaParameter {
    pub name: Name,
    pub type_ref: Option<TypeRef>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Variable(VariableDecl),
    Constant(ConstantDecl),
    Expression(Expr),
    Return(Option<Expr>),
    Block(Block),
    /// `unsafe { ... }`. An ordinary block that also says, in the source, that
    /// the guarantees the compiler makes everywhere else are suspended inside
    /// it. It is the only place a pointer may be read or written.
    Unsafe(Block),
    If {
        condition: Expr,
        then_block: Block,
        /// Either a block or another if statement.
        else_branch: Option<Box<Statement>>,
    },
    IfLet {
        pattern: IfLetPattern,
        binding: Name,
        value: Expr,
        then_block: Block,
        else_branch: Option<Box<Statement>>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    Loop {
        body: Block,
    },
    Break,
    Continue,
    Match {
        value: Expr,
        arms: Vec<MatchArm>,
    },
    For {
        variable: Name,
        iterable: ForIterable,
        body: Block,
    },
}

/// Which builtin payload an `if let` destructures. A bare binding and
/// `Some(name)` are the same pattern; `Ok`/`Err` select a `Result` side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfLetPattern {
    Some,
    Ok,
    Err,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForIterable {
    Range { start: Expr, end: Expr },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    Variant {
        enum_name: Option<Path>,
        variant_name: Name,
        binding: Option<Name>,
        span: Span,
    },
    Wildcard(Span),
    Constant(Expr),
    Range {
        start: Expr,
        end: Expr,
        inclusive: bool,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    Immutable,
    Mutable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariableDecl {
    pub name: Name,
    pub mutability: Mutability,
    pub type_ref: Option<TypeRef>,
    pub initializer: Expr,
    /// `let value = fallible() else reason { ... }`: the binding takes the
    /// payload, and the block runs instead when there is none. Present only on
    /// a declaration whose initializer is an `Option` or a `Result`.
    pub otherwise: Option<Otherwise>,
    pub span: Span,
}

/// The escape half of a declaration that unwraps. It must not fall through:
/// the name it guards is in scope after the statement, so reaching past the
/// block would leave it unbound.
#[derive(Debug, Clone, PartialEq)]
pub struct Otherwise {
    /// The error a `Result` carried, named here and visible only in `block`.
    /// An `Option` has nothing to name.
    pub binding: Option<Name>,
    pub block: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Literal(Literal),
    Identifier(Name),
    Group(Box<Expr>),
    Unary {
        op: UnaryOp,
        op_span: Span,
        operand: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        op_span: Span,
        right: Box<Expr>,
    },
    Assignment {
        target: Box<Expr>,
        op: AssignmentOp,
        op_span: Span,
        value: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
    },
    Member {
        object: Box<Expr>,
        member: Name,
    },
    /// Record construction, e.g. `Vec2 { x: 1.0, y: 2.0 }`. The name is a type,
    /// not a value, so it is never resolved as one.
    StructLiteral {
        name: Path,
        fields: Vec<FieldInit>,
    },
    /// `"text ${value} more"`. Always yields a string.
    Interpolation(Vec<InterpolationPart>),
    /// `new User(name: "Ada")`. Allocates a reference-counted object.
    New {
        name: Path,
        fields: Vec<FieldInit>,
    },
    /// `[1, 2, 3]`. Allocates a reference-counted array.
    Array(Vec<Expr>),
    /// `[expr; count]`. A fixed-size array repeat literal.
    ArrayRepeat {
        element: Box<Expr>,
        count: Box<Expr>,
    },
    /// `weak(value)` or a contextually typed empty `weak()`.
    Weak(Option<Box<Expr>>),
    /// `func(a: int) -> int { ... }`. Evaluates to a function value.
    Lambda(Box<Lambda>),
    /// `value?`. Unwraps a `Result`, returning its `Err` from the function.
    Try(Box<Expr>),
    /// `arr[index]`. Reads an element from an array, or a byte from a string.
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    /// `value[start..end]`. Both endpoints are required, and the range is
    /// half-open like every other range in the language.
    Slice {
        object: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum InterpolationPart {
    Text(String),
    Value(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: Name,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(u64),
    Float(f64),
    Boolean(bool),
    String(String),
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Positive,
    Negative,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    ShiftLeft,
    ShiftRight,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOp {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
}
