//! Typed, resolved high-level IR. Constructed only from successful checking.
//! Groups and source-level call/member syntax do not reach the C backend.
use crate::{
    resolver::SymbolId,
    span::Span,
    type_checker::StructInfo,
    types::{StructId, Type},
};

#[derive(Debug)]
pub struct Program {
    /// Declaration order, which is also the emitted field layout.
    pub(crate) structs: Vec<StructInfo>,
    pub(crate) enums: Vec<crate::types::EnumInfo>,
    pub(crate) arrays: Vec<crate::types::ArrayInfo>,
    pub(crate) options: Vec<crate::types::OptionInfo>,
    pub(crate) results: Vec<crate::types::ResultInfo>,
    pub(crate) functions: Vec<Function>,
    /// Foreign functions: a signature and a linker name, with no body.
    pub(crate) externs: Vec<ExternFunction>,
    pub(crate) entry: SymbolId,
    pub(crate) span: Span,
}
impl Program {
    pub fn span(&self) -> Span {
        self.span
    }
}
#[derive(Debug)]
pub(crate) struct Function {
    pub id: SymbolId,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
    pub body: Block,
    pub span: Span,
}
/// A function defined in another object file. Its name is emitted verbatim,
/// unlike every generated name, because the linker has to find it.
#[derive(Debug)]
pub(crate) struct ExternFunction {
    pub id: SymbolId,
    pub name: String,
    pub parameters: Vec<Type>,
    pub return_type: Type,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) struct Parameter {
    pub id: SymbolId,
    pub ty: Type,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) enum StatementKind {
    Variable {
        id: SymbolId,
        ty: Type,
        initializer: Expr,
    },
    Expression(Expr),
    Return(Option<Expr>),
    Block(Block),
    If {
        condition: Expr,
        then_block: Block,
        else_branch: Option<Box<Statement>>,
    },
    IfLet {
        pattern: IfLetPattern,
        binding: SymbolId,
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
        variable: SymbolId,
        iterable: ForIterable,
        body: Block,
    },
}
/// Which inline payload an `if let` reads. The value's own type says whether
/// that is an `Option` or a `Result`; this only picks the side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IfLetPattern {
    Some,
    Ok,
    Err,
}
#[derive(Debug)]
pub(crate) enum ForIterable {
    Range { start: Expr, end: Expr },
    Array(Expr),
}
#[derive(Debug)]
pub(crate) struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Block,
}
#[derive(Debug)]
pub(crate) enum MatchPattern {
    Variant {
        variant_index: usize,
        binding: Option<SymbolId>,
    },
    Wildcard,
}
#[derive(Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Local(SymbolId),
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
        target: Place,
        op: AssignmentOp,
        op_span: Span,
        value: Box<Expr>,
    },
    Call {
        target: CallTarget,
        arguments: Vec<Expr>,
    },
    /// Field values in source order, each paired with its layout index.
    StructLiteral {
        id: StructId,
        fields: Vec<(usize, Expr)>,
    },
    Field {
        object: Box<Expr>,
        index: usize,
    },
    Interpolation(Vec<InterpolationPart>),
    Array(Vec<Expr>),
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    /// `value[start..end]` over a string or an array. Always copies.
    Slice {
        object: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
    },
    /// `ptr(value)`. Borrows the bytes of a string or array as a raw pointer,
    /// valid only while the borrowed value is alive.
    Ptr(Box<Expr>),
    StringLen(Box<Expr>),
    StringBytes(Box<Expr>),
    /// `bytes_to_string(bytes)`. Yields `Result<string, string>`.
    BytesToString(Box<Expr>),
    Weak(Option<Box<Expr>>),
    ArrayLen(Box<Expr>),
    ArrayCall {
        object: Box<Expr>,
        method: ArrayMethod,
        arguments: Vec<Expr>,
    },
    WeakAlive(Box<Expr>),
    WeakGet(Box<Expr>),
    WeakUpgrade(Box<Expr>),
    Some(Box<Expr>),
    None,
    IsSome(Box<Expr>),
    IsNone(Box<Expr>),
    Ok(Box<Expr>),
    Err(Box<Expr>),
    /// `u8(value)`. Traps when the value does not fit the target width.
    IntConvert {
        value: Box<Expr>,
        target: crate::types::IntType,
    },
    IsOk(Box<Expr>),
    IsErr(Box<Expr>),
    /// `value?`. Yields the success payload, or returns the error from the
    /// enclosing function, whose `Result` type the backend already knows.
    Try(Box<Expr>),
    EnumVariant {
        variant_index: usize,
        payload: Option<Box<Expr>>,
    },
}

#[derive(Debug)]
pub(crate) enum InterpolationPart {
    Text(String),
    Value(Expr),
}
/// A checked place. Reference receivers are evaluated and kept alive before the RHS.
#[derive(Debug)]
pub(crate) enum Place {
    Local(SymbolId),
    Field { base: Box<Place>, index: usize },
    ReferenceField { object: Box<Expr>, index: usize },
    Index { object: Box<Expr>, index: Box<Expr> },
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArrayMethod {
    Push,
    Insert,
    Pop,
    Remove,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CallTarget {
    Function(SymbolId),
    /// A call that leaves the program: no retain, no release, no trapping.
    Extern(SymbolId),
    Print,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Positive,
    Negative,
    Not,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
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
pub(crate) enum AssignmentOp {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
}
