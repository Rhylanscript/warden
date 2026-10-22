use std::ops::Range;

pub type Span = Range<usize>;

/// An AST node of type T plus where it came from in source
#[derive(Debug, Clone, PartialEq)]
pub struct Node<T> {
    pub kind: T,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Node<Item>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Fn(FnDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub is_pub: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Node<TypeExpr>>,
    pub effects: Vec<Node<Effect>>,
    pub body: Node<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Node<TypeExpr>,
}

/// e.g. `io.print` is Effect { path: ["io", "print"] }
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Node<Stmt>>,
    /// The optional trailing expression, which is the block's value.
    pub tail: Option<Box<Node<Expr>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        is_mut: bool,
        ty: Option<Node<TypeExpr>>,
        value: Node<Expr>,
    },
    Assign {
        name: String,
        value: Node<Expr>,
    },
    While {
        cond: Node<Expr>,
        body: Node<Block>,
    },
    Return(Option<Node<Expr>>),
    Expr(Node<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Str(String),
    Bool(bool),
    Ident(String),
    Unary {
        op: UnaryOp,
        operand: Box<Node<Expr>>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Node<Expr>>,
        rhs: Box<Node<Expr>>,
    },
    Call {
        callee: Box<Node<Expr>>,
        args: Vec<Node<Expr>>,
    },
    If {
        cond: Box<Node<Expr>>,
        then_block: Node<Block>,
        /// Either another `If` expression or a `Block` expression.
        else_branch: Option<Box<Node<Expr>>>,
    },
    Block(Block),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}
