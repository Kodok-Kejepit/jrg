// src/parser/ast.rs — bentuk pohon sintaksnya.

use crate::lexer::{Span, TokenKind};

// ─── AST Node Types ────────────────────────────────────────────────────────

// ast.rs inlined:

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub package: String,
    pub imports: Vec<ImportDecl>,
    pub items:   Vec<Item>,
    pub span:    Span,
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub alias: Option<String>,
    pub path:  String,
    pub span:  Span,
}

// ── Top-level items ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    TypeAlias(TypeAlias),
    Const(ConstDecl),
    Var(VarDeclItem),
    Mod(ModDecl),
    Extern(ExternBlock),
    Use(UseDecl),
    Error(Span),
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub vis:        Visibility,
    pub is_async:   bool,
    pub name:       Ident,
    pub generics:   GenericParams,
    pub params:     Vec<FnParam>,
    pub ret_type:   Option<TypeExpr>,
    pub where_clause: Vec<WhereItem>,
    /// `Arc`: nilai fungsi di interpreter memegang badannya, dan menyalinnya harus O(1).
    pub body:       std::sync::Arc<FnBody>,
    pub span:       Span,
}

#[derive(Debug, Clone)]
pub enum FnBody {
    Block(Block),
    Expr(Box<Expr>),   // fn foo() = expr;
    /// Tanda tangan tanpa badan — hanya sah **di dalam `trait`**, dan artinya
    /// "tiap `impl` wajib menyediakan method ini".
    Abstrak,
}

#[derive(Debug, Clone)]
pub struct FnParam {
    /// `fn f(a, sisa...)` — parameter TERAKHIR mengumpulkan argumen yang tersisa
    /// jadi sebuah array.
    pub is_variadic: bool,
    pub is_mut:    bool,
    pub borrow:    BorrowKind,
    pub pattern:   Pattern,
    pub ty:        TypeExpr,
    /// Nilai default (`fn greet(nama = "dunia")` atau `fn f(x: int = 1)`).
    /// `None` = parameter wajib diisi, sama seperti sebelum fitur ini ada.
    /// Meniru persis pola `StructField.default` yang sudah lebih dulu ada.
    pub default:   Option<Box<Expr>>,
    pub lifetime:  Option<String>,
    pub span:      Span,
}

#[derive(Debug, Clone)]
pub enum BorrowKind {
    None,           // owned / moved
    Ref,            // &T
    RefMut,         // &mut T
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub vis:      Visibility,
    pub name:     Ident,
    pub generics: GenericParams,
    /// Induk pewarisan tunggal, dari sintaks opsional `struct Dog: Animal { ...
    pub parent:   Option<Ident>,
    pub where_clause: Vec<WhereItem>,
    pub fields:   Vec<StructField>,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub vis:      Visibility,
    pub name:     Ident,
    pub ty:       TypeExpr,
    pub default:  Option<Box<Expr>>,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub vis:      Visibility,
    pub name:     Ident,
    pub generics: GenericParams,
    pub where_clause: Vec<WhereItem>,
    pub variants: Vec<EnumVariant>,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub enum EnumVariantKind {
    Unit,
    Tuple(Vec<TypeExpr>),
    Struct(Vec<StructField>),
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Ident,
    pub kind: EnumVariantKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub vis:      Visibility,
    pub name:     Ident,
    pub generics: GenericParams,
    pub bounds:   Vec<TraitBound>,
    pub where_clause: Vec<WhereItem>,
    pub items:    Vec<TraitItem>,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub enum TraitItem {
    Method(FnDecl),
    DefaultMethod(FnDecl),
}

#[derive(Debug, Clone)]
pub struct ImplDecl {
    pub generics:  GenericParams,
    pub self_type: TypeExpr,
    pub trait_ref: Option<TypeExpr>,
    pub where_clause: Vec<WhereItem>,
    pub items:     Vec<ImplItem>,
    pub span:      Span,
}

#[derive(Debug, Clone)]
pub enum ImplItem {
    Method(FnDecl),
    Const(ConstDecl),
    TypeAlias(TypeAlias),
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub vis:      Visibility,
    pub name:     Ident,
    pub generics: GenericParams,
    pub ty:       TypeExpr,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub vis:   Visibility,
    pub name:  Ident,
    pub ty:    Option<TypeExpr>,
    pub value: Box<Expr>,
    /// Kata kunci yang benar-benar ditulis: `"const"` atau `"let"`.
    pub kata:  &'static str,
    pub span:  Span,
}

#[derive(Debug, Clone)]
pub struct VarDeclItem {
    pub name:  Ident,
    pub ty:    Option<TypeExpr>,
    pub value: Option<Box<Expr>>,
    pub span:  Span,
}

#[derive(Debug, Clone)]
pub struct ModDecl {
    pub vis:   Visibility,
    pub name:  Ident,
    pub items: Option<Vec<Item>>, // None means external file
    pub span:  Span,
}

#[derive(Debug, Clone)]
pub struct ExternBlock {
    pub abi: String, // "C", "C++", "Go", "system"
    /// Optional path to the shared library this block's symbols should be
    /// resolved from at runtime, written as a second string literal right after …
    pub lib:   Option<String>,
    pub items: Vec<ExternItem>,
    pub span:  Span,
}

#[derive(Debug, Clone)]
pub enum ExternItem {
    Fn  { vis: Visibility, name: Ident, params: Vec<ExternParam>, ret: Option<TypeExpr>, span: Span },
    Static { vis: Visibility, is_mut: bool, name: Ident, ty: TypeExpr, span: Span },
}

#[derive(Debug, Clone)]
pub struct ExternParam {
    pub name: Option<Ident>,
    pub ty:   TypeExpr,
}

#[derive(Debug, Clone)]
pub struct UseDecl {
    pub path: UsePath,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum UsePath {
    Simple(Vec<String>),
    Aliased(Vec<String>, String),
    Glob(Vec<String>),
    Group(Vec<String>, Vec<UsePath>),
}

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Primitive(PrimTy),
    Named(NamedType),
    Ownership(OwnershipTy),
    Pointer(PointerTy),
    Array(ArrayTy),
    Slice(SliceTy),
    Map(MapTy),
    Chan(ChanTy),
    Fn(FnTy),
    Tuple(Vec<TypeExpr>),
    Never,
    Inferred,
    Error(Span),
}

#[derive(Debug, Clone)]
pub enum PrimTy {
    I8, I16, I32, I64, I128, Isize,
    U8, U16, U32, U64, U128, Usize,
    Int, Uint,
    F32, F64, F128,
    C64, C128,
    Bool, Byte, Rune, Str, String_, Unit,
}

#[derive(Debug, Clone)]
pub struct NamedType {
    pub package: Option<String>,
    /// Alias modul di pola tiga segmen (`b::Warna::Merah`); cuma dibaca `ruang_tipe`.
    pub modul:   Option<String>,
    pub name:    String,
    pub args:    Vec<TypeExpr>,
    pub span:    Span,
}

#[derive(Debug, Clone)]
pub struct OwnershipTy {
    pub qualifier: OwnershipQual,
    pub inner:     Box<TypeExpr>,
    pub span:      Span,
}

#[derive(Debug, Clone)]
pub enum OwnershipQual { Owned, Shared, Gc, Raw }

#[derive(Debug, Clone)]
pub struct PointerTy { pub is_mut: bool, pub inner: Box<TypeExpr>, pub span: Span }
#[derive(Debug, Clone)]
pub struct ArrayTy   { pub len: Box<Expr>, pub elem: Box<TypeExpr>, pub span: Span }
#[derive(Debug, Clone)]
pub struct SliceTy   { pub elem: Box<TypeExpr>, pub span: Span }
#[derive(Debug, Clone)]
pub struct MapTy     { pub key: Box<TypeExpr>, pub val: Box<TypeExpr>, pub span: Span }
#[derive(Debug, Clone)]
pub struct ChanTy    { pub dir: ChanDir, pub elem: Box<TypeExpr>, pub span: Span }
#[derive(Debug, Clone)]
pub enum ChanDir { Bidir, SendOnly, RecvOnly }
#[derive(Debug, Clone)]
pub struct FnTy {
    pub generics: GenericParams,
    pub params: Vec<TypeExpr>,
    pub ret: Option<Box<TypeExpr>>,
    pub span: Span,
}

// ── Generics ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GenericParams {
    pub params: Vec<GenericParam>,
    pub span:   Option<Span>,
}

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name:   String,
    pub bounds: Vec<TraitBound>,
    pub span:   Span,
}

#[derive(Debug, Clone)]
pub enum TraitBound {
    Trait(TypeExpr),
    Lifetime(String),
    Negative(Box<TraitBound>),  // ?Sized
}

#[derive(Debug, Clone)]
pub struct WhereItem {
    pub ty:     TypeExpr,
    pub bounds: Vec<TraitBound>,
    pub span:   Span,
}

// ── Patterns ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Pattern {
    Ident   { name: String, is_mut: bool, span: Span },
    Tuple   { elems: Vec<Pattern>, span: Span },
    Struct  { ty: TypeExpr, fields: Vec<FieldPat>, span: Span },
    Enum    { ty: TypeExpr, variant: String, inner: Option<Box<Pattern>>, span: Span },
    Literal { lit: LitKind, span: Span },
    /// `lo..hi` / `lo..=hi`, with either bound omittable (`..hi`, `lo..`, or even
    /// a bare `..`/`..=`, matching anything — though a plain `_` reads better for …
    Range   { lo: Option<Box<Pattern>>, hi: Option<Box<Pattern>>, inclusive: bool, span: Span },
    Ref     { inner: Box<Pattern>, is_mut: bool, span: Span },
    Or      { pats: Vec<Pattern>, span: Span },
    /// Pola array: `[a, b, c]`, `[]`, `[first, ...sisa]`, `[a, ..., z]`.
    Array   { before: Vec<Pattern>, rest: Option<Option<String>>, after: Vec<Pattern>, span: Span },
    /// `nama @ pola` — ikat nilainya ke `nama` DAN uji `pola` atasnya.
    Binding { name: String, inner: Box<Pattern>, span: Span },
    Wildcard(Span),
}

#[derive(Debug, Clone)]
pub struct FieldPat { pub name: String, pub pat: Pattern, pub span: Span }

// ── Statements ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(LetStmt),
    Var(VarStmt),
    Const(ConstDecl),
    Expr(Box<Expr>),
    Assign(AssignStmt),
    ShortDecl(ShortDeclStmt),
    If(IfStmt),
    For(ForStmt),
    While(WhileStmt),
    Loop(LoopStmt),
    Match(MatchStmt),
    Return(ReturnStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Spawn(SpawnStmt),
    Select(SelectStmt),
    Defer(DeferStmt),
    Block(Block),
    /// Deklarasi fungsi bernama DI DALAM badan fungsi lain: `fn bantu(x: int) ->
    /// int { ...
    Fn(FnDecl),
    Error(Span),
}

impl Stmt {
    /// Rentang sumber statement ini — pasangan dari [`Expr::span`].
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let(s) => s.span,
            Stmt::Var(s) => s.span,
            Stmt::Const(s) => s.span,
            Stmt::Expr(e) => e.span(),
            Stmt::Assign(s) => s.span,
            Stmt::ShortDecl(s) => s.span,
            Stmt::If(s) => s.span,
            Stmt::For(s) => s.span(),
            Stmt::While(s) => s.span,
            Stmt::Loop(s) => s.span,
            Stmt::Match(s) => s.span,
            Stmt::Return(s) => s.span,
            Stmt::Break(s) => s.span,
            Stmt::Continue(s) => s.span,
            Stmt::Spawn(s) => s.span,
            Stmt::Select(s) => s.span,
            Stmt::Defer(s) => s.span,
            Stmt::Block(b) => b.span,
            Stmt::Fn(f) => f.span,
            Stmt::Error(sp) => *sp,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub is_mut:    bool,
    pub ownership: Option<OwnershipQual>,
    pub pattern:   Pattern,
    pub ty:        Option<TypeExpr>,
    pub init:      Box<Expr>,
    pub lifetime:  Option<String>,
    pub span:      Span,
}

#[derive(Debug, Clone)]
pub struct VarStmt {
    pub name:  Ident,
    pub ty:    Option<TypeExpr>,
    pub init:  Option<Box<Expr>>,
    pub span:  Span,
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub targets: Vec<Expr>,
    pub op:      AssignOp,
    pub values:  Vec<Expr>,
    pub span:    Span,
}

#[derive(Debug, Clone)]
pub enum AssignOp {
    Eq, PlusEq, MinusEq, StarEq, SlashEq, PercentEq,
    AndEq, OrEq, CaretEq, ShlEq, ShrEq,
    /// `**=`. Ada karena ketiadaannya cuma lubang di keluarga yang sudah
    /// lengkap: `**` ada, `*=` ada, `**=` tidak.
    PowEq,
}

#[derive(Debug, Clone)]
pub struct ShortDeclStmt {
    pub names:  Vec<Ident>,
    pub values: Vec<Expr>,
    pub span:   Span,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub init:     Option<Box<Stmt>>,   // optional `let x = ...; ` before condition
    pub cond:     Box<Expr>,
    pub then:     Block,
    pub else_ifs: Vec<(Option<Box<Stmt>>, Box<Expr>, Block)>,
    pub else_:    Option<Block>,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub enum ForStmt {
    Classic { init: Option<Box<Stmt>>, cond: Option<Box<Expr>>, post: Option<Box<Stmt>>, body: Block, label: Option<String>, span: Span },
    Range   { vars: Vec<Ident>, is_decl: bool, iter: Box<Expr>, body: Block, label: Option<String>, span: Span },
}

impl ForStmt {
    pub(crate) fn set_label(&mut self, label: String) {
        match self {
            ForStmt::Classic { label: l, .. } | ForStmt::Range { label: l, .. } => *l = Some(label),
        }
    }

    /// Rentang sumber statement `for` ini.
    pub fn span(&self) -> Span {
        match self {
            ForStmt::Classic { span, .. } | ForStmt::Range { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WhileStmt { pub cond: Box<Expr>, pub body: Block, pub label: Option<String>, pub span: Span }
#[derive(Debug, Clone)]
pub struct LoopStmt  { pub body: Block, pub label: Option<String>, pub span: Span }

#[derive(Debug, Clone)]
pub struct MatchStmt {
    pub scrutinee: Box<Expr>,
    pub arms:      Vec<MatchArm>,
    pub span:      Span,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub patterns: Vec<Pattern>,
    pub guard:    Option<Box<Expr>>,
    pub body:     MatchBody,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub enum MatchBody { Expr(Box<Expr>), Block(Block) }

#[derive(Debug, Clone)]
pub struct ReturnStmt  { pub values: Vec<Expr>, pub span: Span }
#[derive(Debug, Clone)]
pub struct BreakStmt   { pub label: Option<String>, pub span: Span }
#[derive(Debug, Clone)]
pub struct ContinueStmt { pub label: Option<String>, pub span: Span }

#[derive(Debug, Clone)]
pub struct SpawnStmt {
    pub params:   Option<Vec<FnParam>>,  // |params|
    pub is_async: bool,
    pub is_move:  bool,
    pub body:     Block,
    pub span:     Span,
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub cases:   Vec<SelectCase>,
    pub default: Option<Block>,
    pub span:    Span,
}

#[derive(Debug, Clone)]
pub enum SelectCase {
    Send { chan: Box<Expr>, val: Box<Expr>, body: Block, span: Span },
    Recv { names: Vec<Ident>, is_decl: bool, chan: Box<Expr>, body: Block, span: Span },
    Let  { stmt: LetStmt, body: Block, span: Span },
}

#[derive(Debug, Clone)]
pub struct DeferStmt { pub expr: Box<Expr>, pub span: Span }

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail:  Option<Box<Expr>>,   // last expression (block value)
    pub span:  Span,
}

// ── Expressions ───────────────────────────────────────────────────────────

impl Expr {
    /// Rentang sumber ekspresi ini.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, s)
            | Expr::Ident(_, s)
            | Expr::Unary { span: s, .. }
            | Expr::Binary { span: s, .. }
            | Expr::Ternary { span: s, .. }
            | Expr::Pipe { span: s, .. }
            | Expr::Cast { span: s, .. }
            | Expr::Field { span: s, .. }
            | Expr::Method { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::Slice { span: s, .. }
            | Expr::Call { span: s, .. }
            | Expr::Await { span: s, .. }
            | Expr::Propagate { span: s, .. }
            | Expr::Closure { span: s, .. }
            | Expr::Struct { span: s, .. }
            | Expr::Array { span: s, .. }
            | Expr::ArrayRepeat { span: s, .. }
            | Expr::Tuple { span: s, .. }
            | Expr::Range { span: s, .. }
            | Expr::MakeChan { span: s, .. }
            | Expr::Send { span: s, .. }
            | Expr::Recv { span: s, .. }
            | Expr::PathExpr { span: s, .. }
            | Expr::Match { span: s, .. }
            | Expr::Template { span: s, .. }
            | Expr::Error(s) => *s,
            Expr::Block(b) => b.span,
            Expr::If(s) => s.span,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(LitKind, Span),
    Ident(String, Span),
    Unary  { op: UnaryOp, expr: Box<Expr>, span: Span },
    Binary { op: BinaryOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    Ternary { cond: Box<Expr>, then: Box<Expr>, else_: Box<Expr>, span: Span },
    Pipe   { lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    Cast   { expr: Box<Expr>, ty: Box<TypeExpr>, span: Span },
    Field  { obj: Box<Expr>, field: String, span: Span },
    Method { obj: Box<Expr>, method: String, args: Vec<Expr>, span: Span },
    Index  { obj: Box<Expr>, idx: Box<Expr>, span: Span },
    Slice  { obj: Box<Expr>, lo: Option<Box<Expr>>, hi: Option<Box<Expr>>, span: Span },
    Call   { func: Box<Expr>, args: Vec<Expr>, span: Span },
    Await  { expr: Box<Expr>, span: Span },
    Propagate { expr: Box<Expr>, span: Span }, // `expr?`
    /// `ret` adalah anotasi `-> T` yang boleh ditulis sesudah parameter.
    /// Seperti tiap anotasi tipe di JRG ia DIPERIKSA NAMANYA dan tidak
    /// ditegakkan; yang memakainya cuma inferensi backend LLVM.
    Closure { params: Vec<FnParam>, is_move: bool, ret: Option<TypeExpr>,
              body: std::sync::Arc<Expr>, span: Span },
    Block(Block),
    Struct { ty: TypeExpr, fields: Vec<FieldInit>, spread: Option<Box<Expr>>, span: Span },
    Array  { elems: Vec<Expr>, span: Span },
    ArrayRepeat { elem: Box<Expr>, count: Box<Expr>, span: Span },
    Tuple  { elems: Vec<Expr>, span: Span },
    Range  { lo: Option<Box<Expr>>, hi: Option<Box<Expr>>, inclusive: bool, span: Span },
    MakeChan { ty: Box<TypeExpr>, cap: Option<Box<Expr>>, span: Span },
    Send   { chan: Box<Expr>, val: Box<Expr>, span: Span },
    Recv   { chan: Box<Expr>, span: Span },
    PathExpr { segments: Vec<String>, span: Span },
    /// `match scrutinee { pat => body, ...
    Match  { scrutinee: Box<Expr>, arms: Vec<MatchArm>, span: Span },
    /// `if cond { ..
    If(Box<IfStmt>),
    /// Template literal `« teks ${expr} teks »` — potongan teks dan ekspresi yang
    /// berselang-seling, disimpan berurutan persis seperti urutan di sumbernya.
    Template { parts: Vec<TemplatePart>, span: Span },
    Error(Span),
}

#[derive(Debug, Clone)]
pub struct FieldInit { pub name: String, pub value: Expr, pub span: Span }

/// Satu potongan di dalam template literal: teks apa adanya, atau sebuah
/// ekspresi yang nanti diformat jadi teks.
#[derive(Debug, Clone)]
pub enum TemplatePart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum LitKind {
    Int(u128), Float(f64), Imag(f64), Rune(char),
    Str(String), RawStr(String), Bool(bool), Nil,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp { Neg, Not, BitNot, Deref, Ref, RefMut, Move }

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Rem, Pow,
    BitAnd, BitOr, BitXor, Shl, Shr, AndNot,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    Chan,   // <-
}

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility { Public, Private }

pub type Ident = String;
