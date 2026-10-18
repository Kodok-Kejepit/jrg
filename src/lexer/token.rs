// src/lexer/token.rs — definisi token dan kata kunci

#![allow(dead_code, unused_variables, unused_imports)]

use super::*;

// ─── Token definitions ─────────────────────────────────────────────────────

// token.rs is inlined here for single-file legibility:

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Literals ──────────────────────────────────────────────────────────
    IntLit(u128),
    FloatLit(f64),
    ImaginaryLit(f64),
    RuneLit(char),
    StringLit(String),        // interpreted "..." string
    RawStringLit(String),     // raw `...` string
    TemplateLitStart,         // «
    TemplateLitEnd,           // »
    TemplateInterpStart,      // ${
    TemplateInterpEnd,        // }  (only inside template)
    BoolLit(bool),
    NilLit,

    // ── Identifiers & Keywords ────────────────────────────────────────────
    Ident(String),
    // Keywords:
    KwAsync, KwAwait, KwBreak, KwChan, KwConst, KwDefer, KwDo, KwElse,
    KwEnum, KwExtern, KwFalse, KwFn, KwFor, KwGo, KwIf, KwImpl,
    KwImport, KwIn, KwLet, KwLoop, KwMatch, KwMod, KwMove, KwMut,
    KwOwned, KwPackage, KwPub, KwRaw, KwRef, KwReturn, KwSelect,
    KwSelf, KwShared, KwGc, KwSpawn, KwStatic, KwStruct, KwTrait,
    KwTrue, KwType, KwUnsafe, KwUse, KwVar, KwWhere, KwWhile, KwYield,
    // Additional keywords used in parser and type system:
    KwAs,       // `as` cast operator, use-alias
    KwCase,     // `case` in select statements
    KwDefault,  // `default` in select statements
    KwContinue, // `continue` statement

    // ── Operators ─────────────────────────────────────────────────────────
    Plus, Minus, Star, Slash, Percent, StarStar, StarStarEq,  // + - * / % ** **=
    And, Or, Caret, Tilde, Shl, Shr, AndCaret,   // & | ^ ~ << >> &^
    EqEq, BangEq, Lt, LtEq, Gt, GtEq,            // == != < <= > >=
    AndAnd, OrOr, Bang,                            // && || !
    Arrow, FatArrow,                               // -> =>
    LArrow,                                        // <- (channel)
    Pipe,                                          // |> (pipe operator)

    // ── Assignment ────────────────────────────────────────────────────────
    Eq, PlusEq, MinusEq, StarEq, SlashEq, PercentEq,
    AndEq, OrEq, CaretEq, ShlEq, ShrEq,
    ColonEq,                                       // :=

    // ── Punctuation ───────────────────────────────────────────────────────
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Dot, DotDot, DotDotEq, Comma, Semi, Colon, ColonColon, DotDotDot,
    At, Hash, Question, QuestionQuestion, Amp, AmpMut,
    /// Pembuka ternary (`cond $ a : b`). Lihat catatan di tempat lexer
    /// menghasilkannya untuk alasan kenapa ternary tidak lagi memakai `?`.
    Dollar,

    // ── Special ───────────────────────────────────────────────────────────
    Newline,    // significant in some positions (auto-semicolon)
    Eof,
    Error(String),
}

impl TokenKind {
    /// Bagaimana token ini DITULIS oleh pemakainya.
    pub fn tulisan(&self) -> String {
        use TokenKind::*;
        let t = |s: &str| format!("`{}`", s);
        match self {
            // Literal: yang ditampilkan NILAINYA, bukan nama jenisnya.
            IntLit(n)        => t(&n.to_string()),
            FloatLit(f)      => t(&f.to_string()),
            ImaginaryLit(f)  => t(&format!("{}i", f)),
            RuneLit(c)       => t(&format!("'{}'", c)),
            StringLit(s)     => t(&format!("\"{}\"", potong(s))),
            RawStringLit(s)  => t(&format!("`{}`", potong(s))),
            BoolLit(b)       => t(if *b { "true" } else { "false" }),
            NilLit           => t("nil"),
            Ident(n)         => t(n),

            TemplateLitStart   => t("«"),
            TemplateLitEnd     => t("»"),
            TemplateInterpStart => t("${"),
            TemplateInterpEnd  => t("}"),

            KwAsync => t("async"), KwAwait => t("await"), KwBreak => t("break"),
            KwChan => t("chan"), KwConst => t("const"), KwDefer => t("defer"),
            KwDo => t("do"), KwElse => t("else"), KwEnum => t("enum"),
            KwExtern => t("extern"), KwFalse => t("false"), KwFn => t("fn"),
            KwFor => t("for"), KwGo => t("go"), KwIf => t("if"),
            KwImpl => t("impl"), KwImport => t("import"), KwIn => t("in"),
            KwLet => t("let"), KwLoop => t("loop"), KwMatch => t("match"),
            KwMod => t("mod"), KwMove => t("move"), KwMut => t("mut"),
            KwOwned => t("owned"), KwPackage => t("package"), KwPub => t("pub"),
            KwRaw => t("raw"), KwRef => t("ref"), KwReturn => t("return"),
            KwSelect => t("select"), KwSelf => t("self"), KwShared => t("shared"),
            KwGc => t("gc"), KwSpawn => t("spawn"), KwStatic => t("static"),
            KwStruct => t("struct"), KwTrait => t("trait"), KwTrue => t("true"),
            KwType => t("type"), KwUnsafe => t("unsafe"), KwUse => t("use"),
            KwVar => t("var"), KwWhere => t("where"), KwWhile => t("while"),
            KwYield => t("yield"), KwAs => t("as"),
            KwCase => t("case"), KwDefault => t("default"), KwContinue => t("continue"),

            Plus => t("+"), Minus => t("-"), Star => t("*"), Slash => t("/"),
            Percent => t("%"), StarStar => t("**"), StarStarEq => t("**="),
            And => t("&"), Or => t("|"), Caret => t("^"), Tilde => t("~"),
            Shl => t("<<"), Shr => t(">>"), AndCaret => t("&^"),
            EqEq => t("=="), BangEq => t("!="), Lt => t("<"), LtEq => t("<="),
            Gt => t(">"), GtEq => t(">="),
            AndAnd => t("&&"), OrOr => t("||"), Bang => t("!"),
            Arrow => t("->"), FatArrow => t("=>"), LArrow => t("<-"), Pipe => t("|>"),

            Eq => t("="), PlusEq => t("+="), MinusEq => t("-="), StarEq => t("*="),
            SlashEq => t("/="), PercentEq => t("%="), AndEq => t("&="),
            OrEq => t("|="), CaretEq => t("^="), ShlEq => t("<<="), ShrEq => t(">>="),
            ColonEq => t(":="),

            LParen => t("("), RParen => t(")"), LBrace => t("{"), RBrace => t("}"),
            LBracket => t("["), RBracket => t("]"),
            Dot => t("."), DotDot => t(".."), DotDotEq => t("..="), DotDotDot => t("..."),
            Comma => t(","), Semi => t(";"), Colon => t(":"), ColonColon => t("::"),
            At => t("@"), Hash => t("#"), Question => t("?"),
            QuestionQuestion => t("??"), Amp => t("&"), AmpMut => t("&mut"),
            Dollar => t("$"),

            // Bukan sesuatu yang diketik, jadi tidak di-backtick.
            Newline => "akhir baris".to_string(),
            Eof     => "akhir file".to_string(),
            Error(e) => format!("token rusak ({})", potong(e)),
        }
    }
}

/// Literal panjang dipendekkan: sebuah pesan galat yang memuat string 4 KB
/// berhenti berguna jauh sebelum barisnya habis.
pub(crate) fn potong(s: &str) -> String {
    const MAKS: usize = 32;
    if s.chars().count() <= MAKS { return s.to_string(); }
    let potongan: String = s.chars().take(MAKS).collect();
    format!("{}…", potongan)
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }
}

// Keyword map — phf would be faster at scale but HashMap is clearer here.
pub(crate) fn keyword_or_ident(s: &str) -> TokenKind {
    match s {
        "async"   => TokenKind::KwAsync,
        "as"      => TokenKind::KwAs,
        "await"   => TokenKind::KwAwait,
        "break"   => TokenKind::KwBreak,
        "chan"     => TokenKind::KwChan,
        // `map` BUKAN kata kunci.
        "case"     => TokenKind::KwCase,
        "default"  => TokenKind::KwDefault,
        "continue" => TokenKind::KwContinue,
        "const"   => TokenKind::KwConst,
        "defer"   => TokenKind::KwDefer,
        "do"      => TokenKind::KwDo,
        "else"    => TokenKind::KwElse,
        "enum"    => TokenKind::KwEnum,
        "extern"  => TokenKind::KwExtern,
        "false"   => TokenKind::BoolLit(false),
        "fn"      => TokenKind::KwFn,
        "for"     => TokenKind::KwFor,
        "go"      => TokenKind::KwGo,
        "gc"      => TokenKind::KwGc,
        "if"      => TokenKind::KwIf,
        "impl"    => TokenKind::KwImpl,
        "import"  => TokenKind::KwImport,
        "in"      => TokenKind::KwIn,
        "let"     => TokenKind::KwLet,
        "loop"    => TokenKind::KwLoop,
        "match"   => TokenKind::KwMatch,
        "mod"     => TokenKind::KwMod,
        "move"    => TokenKind::KwMove,
        "mut"     => TokenKind::KwMut,
        "nil"     => TokenKind::NilLit,
        "owned"   => TokenKind::KwOwned,
        "package" => TokenKind::KwPackage,
        "pub"     => TokenKind::KwPub,
        "raw"     => TokenKind::KwRaw,
        "ref"     => TokenKind::KwRef,
        "return"  => TokenKind::KwReturn,
        "select"  => TokenKind::KwSelect,
        "self"    => TokenKind::KwSelf,
        "shared"  => TokenKind::KwShared,
        "spawn"   => TokenKind::KwSpawn,
        "static"  => TokenKind::KwStatic,
        "struct"  => TokenKind::KwStruct,
        "trait"   => TokenKind::KwTrait,
        "true"    => TokenKind::BoolLit(true),
        "type"    => TokenKind::KwType,
        "unsafe"  => TokenKind::KwUnsafe,
        "use"     => TokenKind::KwUse,
        "var"     => TokenKind::KwVar,
        "where"   => TokenKind::KwWhere,
        "while"   => TokenKind::KwWhile,
        "yield"   => TokenKind::KwYield,
        other     => TokenKind::Ident(other.to_owned()),
    }
}
