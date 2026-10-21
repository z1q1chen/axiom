// Axiom AST (untyped, as parsed).
#![allow(dead_code)]

#[derive(Clone, Debug)]
pub struct Span {
    pub file: String,
    pub line: u32,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeExpr {
    Int,
    Float,
    Bool,
    Str,
    Void,
    Array(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
    Named(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContractKind {
    Requires,
    Ensures,
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub kind: ContractKind,
    pub expr: Expr,
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Invariant {
    pub expr: Expr,
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, TypeExpr)>,
    pub invariants: Vec<Invariant>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<(String, TypeExpr)>,
    pub ret: TypeExpr,
    pub contracts: Vec<Contract>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Item {
    Struct(StructDef),
    Fn(FnDef),
    Global(String, Option<TypeExpr>, Expr, Span),
}

#[derive(Clone, Debug)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let(String, Option<TypeExpr>, Expr, Span),
    Assign(String, Expr, Span),
    FieldAssign(Expr, String, Expr, Span),
    IndexAssign(Expr, Expr, Expr, Span),
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>, Span),
    While(Expr, Vec<Stmt>, Span),
    For(String, Expr, Vec<Stmt>, Span),
    Return(Option<Expr>, Span),
    Break(Span),
    Continue(Span),
    Check(Expr, String, Span),
    Expr(Expr, Span),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    Str(String, Span),
    Ident(String, Span),
    Array(Vec<Expr>, Span),
    MapLit(Vec<(Expr, Expr)>, Span),
    StructLit(String, Vec<(String, Expr)>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Field(Box<Expr>, String, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Bin(BinOp, Box<Expr>, Box<Expr>, Span),
    Un(UnOp, Box<Expr>, Span),
}

impl Expr {
    pub fn span(&self) -> &Span {
        match self {
            Expr::Int(_, s) | Expr::Float(_, s) | Expr::Bool(_, s) | Expr::Str(_, s)
            | Expr::Ident(_, s) | Expr::Array(_, s) | Expr::MapLit(_, s)
            | Expr::StructLit(_, _, s)
            | Expr::Call(_, _, s) | Expr::Field(_, _, s) | Expr::Index(_, _, s)
            | Expr::Bin(_, _, _, s) | Expr::Un(_, _, s) => s,
        }
    }
}
