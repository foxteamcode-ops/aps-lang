use crate::compiler::diagnostics::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Value, // Default owned value or Copy value
    Read,  // `read T`
    Write, // `write T`
    Take,  // `take T`
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::Value => "",
            Capability::Read => "read",
            Capability::Write => "write",
            Capability::Take => "take",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAst {
    Named {
        name: String,
        args: Vec<TypeAst>,
    },
    Capability {
        cap: Capability,
        inner: Box<TypeAst>,
    },
    Array {
        elem: Box<TypeAst>,
        size: Option<usize>,
    },
    Function {
        params: Vec<TypeAst>,
        ret: Box<TypeAst>,
    },
    Shared(Box<TypeAst>),
    Pointer(Box<TypeAst>),
    Void,
    Inferred,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub name: String,
    pub bound: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub capability: Capability,
    pub ty: TypeAst,
    pub is_mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub is_pub: bool,
    pub name: String,
    pub ty: TypeAst,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub is_pub: bool,
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub fields: Vec<StructField>,
    pub attributes: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnumVariantKind {
    Unit,
    Tuple(Vec<TypeAst>),
    Struct(Vec<StructField>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub kind: EnumVariantKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub is_pub: bool,
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub is_pub: bool,
    pub is_async: bool,
    pub is_const: bool,
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret_type: TypeAst,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret_type: TypeAst,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub generics: Vec<GenericParam>,
    pub target_name: String,
    pub trait_name: Option<String>,
    pub methods: Vec<FnDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub is_pub: bool,
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub methods: Vec<FnSig>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    pub abi: String,
    pub functions: Vec<FnSig>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub is_pub: bool,
    pub name: String,
    pub ty: TypeAst,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Const(ConstDecl),
    Impl(ImplBlock),
    Trait(TraitDecl),
    Extern(ExternBlock),
    TypeAlias {
        is_pub: bool,
        name: String,
        ty: TypeAst,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub module_name: Option<Vec<String>>,
    pub imports: Vec<ImportDecl>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    RemAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        ty: TypeAst,
        init: Expr,
        span: Span,
    },
    Var {
        name: String,
        ty: TypeAst,
        init: Expr,
        span: Span,
    },
    Const(ConstDecl),
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Defer {
        body: Block,
        span: Span,
    },
    Region {
        name: String,
        body: Block,
        span: Span,
    },
    Unsafe {
        body: Block,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    For {
        var: String,
        iter: Expr,
        body: Block,
        span: Span,
    },
    Loop {
        body: Block,
        span: Span,
    },
    Expr {
        expr: Expr,
        has_semi: bool,
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> &Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Var { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Break { span }
            | Stmt::Continue { span }
            | Stmt::Defer { span, .. }
            | Stmt::Region { span, .. }
            | Stmt::Unsafe { span, .. }
            | Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::Expr { span, .. } => span,
            Stmt::Const(c) => &c.span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Read,
    Write,
    Take,
    Share,
    Await,
    Spawn,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallArg {
    pub capability: Capability,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Literal(Expr),
    Ident(String),
    Variant {
        enum_name: Option<String>,
        variant_name: String,
        bindings: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    IntLit(i64, Option<String>),
    FloatLit(f64, Option<String>),
    StringLit(String),
    CharLit(char),
    BoolLit(bool),
    Ident(String),
    Path(Vec<String>),
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: TypeAst,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<CallArg>,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    ArrayLit(Vec<Expr>),
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    If {
        cond: Box<Expr>,
        then_block: Block,
        else_branch: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Closure {
        params: Vec<Param>,
        ret_type: TypeAst,
        body: Block,
    },
    Block(Block),
    TryProp(Box<Expr>), // `expr?`
}
