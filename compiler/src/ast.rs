//! Source-level syntax only: no token kinds, resolved symbols or inferred types.
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub functions: Vec<FunctionDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
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
    pub kind: TypeDeclKind,
    pub name: Name,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    Named(Name),
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
        class: Name,
        span: Span,
    },
    Array {
        element: Box<TypeRef>,
        span: Span,
    },
}

impl TypeRef {
    pub fn span(&self) -> Span {
        match self {
            Self::Named(name) => name.span,
            Self::Array { span, .. }
            | Self::Weak { span, .. }
            | Self::Option { span, .. }
            | Self::Result { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
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
    Expression(Expr),
    Return(Option<Expr>),
    Block(Block),
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
        enum_name: Option<Name>,
        variant_name: Name,
        binding: Option<Name>,
        span: Span,
    },
    Wildcard(Span),
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
        name: Name,
        fields: Vec<FieldInit>,
    },
    /// `"text ${value} more"`. Always yields a string.
    Interpolation(Vec<InterpolationPart>),
    /// `new User(name: "Ada")`. Allocates a reference-counted object.
    New {
        name: Name,
        fields: Vec<FieldInit>,
    },
    /// `[1, 2, 3]`. Allocates a reference-counted array.
    Array(Vec<Expr>),
    /// `weak(value)` or a contextually typed empty `weak()`.
    Weak(Option<Box<Expr>>),
    /// `value?`. Unwraps a `Result`, returning its `Err` from the function.
    Try(Box<Expr>),
    /// `arr[index]`. Reads an element from an array.
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
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
}
