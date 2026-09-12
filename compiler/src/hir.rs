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
    pub(crate) functions: Vec<Function>,
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
    While {
        condition: Expr,
        body: Block,
    },
    Loop {
        body: Block,
    },
    Break,
    Continue,
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
    /// Field values in declaration order, not source order.
    StructLiteral {
        id: StructId,
        fields: Vec<Expr>,
    },
    Field {
        object: Box<Expr>,
        index: usize,
    },
    Interpolation(Vec<InterpolationPart>),
}

#[derive(Debug)]
pub(crate) enum InterpolationPart {
    Text(String),
    Value(Expr),
}
/// An assignable location: a local, optionally followed by field steps.
#[derive(Debug)]
pub(crate) struct Place {
    pub base: SymbolId,
    pub fields: Vec<usize>,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum CallTarget {
    Function(SymbolId),
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
