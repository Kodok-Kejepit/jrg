// src/parser/ekspresi.rs — ekspresi — Pratt parser

use super::*;

/// Satu kalimat untuk `++` dan `--`, dipakai jalur prefix maupun postfix.
const PESAN_TAMBAH: &str = "JRG tidak punya `++`. Untuk menambah satu tulis \
    `i += 1` (atau `i = i + 1`) — dan itu sebuah PERNYATAAN, bukan ekspresi, \
    jadi nilainya tidak bisa dipakai di tengah perhitungan";
const PESAN_KURANG: &str = "JRG tidak punya `--`. Untuk mengurangi satu tulis \
    `i -= 1` (atau `i = i - 1`) — dan itu sebuah PERNYATAAN, bukan ekspresi. \
    Kalau yang dimaksud negasi ganda, beri spasi: `- -x`";

/// Jalur ke sebuah BAWAAN bahasa (`::terap`, `::dict::from_pairs`): segmen pertamanya
/// kosong, jadi `fn terap` milik pemakai atau modul bernama `dict` tidak bisa membajak
/// sintaks yang diturunkan ke sana.
pub(crate) fn jalur_bawaan(nama: &[&str], span: Span) -> Expr {
    let mut segments = vec![String::new()];
    segments.extend(nama.iter().map(|s| s.to_string()));
    Expr::PathExpr { segments, span }
}

/// Awalan nama sementara hasil penurunan `a ?? b` jadi `match`.
pub(crate) const AWALAN_KOALESENSI: &str = "$koalesensi";

/// Apakah lengan-lengan ini hasil penurunan `a ?? b`? Dipakai dua mesin supaya `7 ?? 0`
/// dijawab tentang `??`, bukan tentang `match` yang tidak pernah ditulis pemakainya.
pub fn lengan_dari_koalesensi(arms: &[MatchArm]) -> bool {
    matches!(arms, [_, MatchArm { patterns, .. }] if matches!(patterns.as_slice(),
        [Pattern::Enum { inner: Some(p), .. }] if matches!(&**p,
            Pattern::Ident { name, .. } if name.starts_with(AWALAN_KOALESENSI))))
}

/// Penanda `let` hasil penurunan `a, b := e` (di field `lifetime`, yang tidak dipakai di sana).
pub const PENANDA_BONGKAR_PENDEK: &str = ":=";

/// Asal sebuah `let`: 0 ditulis pemakai, 1 pola parameter, 2 pola nilai `for`, 3 `a, b := e` —
/// dua yang terakhir hasil penurunan parser, jadi pesannya tidak boleh menyebut `let`.
pub fn asal_let(ls: &LetStmt) -> u8 {
    if ls.lifetime.as_deref() == Some(PENANDA_BONGKAR_PENDEK) { return 3; }
    match &*ls.init {
        Expr::Ident(n, _) if n.starts_with("__pola") => 1,
        Expr::Ident(n, _) if n == "__nilai" => 2,
        _ => 0,
    }
}

impl Parser {
    // ── Expressions (Pratt parser) ────────────────────────────────────────

    pub(crate) fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_with_bp(0)
    }

    pub(crate) fn parse_expr_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut exprs = vec![self.parse_expr()?];
        while self.eat(&TokenKind::Comma) {
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    // Binding powers for Pratt parsing
    pub(crate) fn prefix_bp(kind: &TokenKind) -> Option<u8> {
        match kind {
            TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde
            | TokenKind::Star | TokenKind::Amp | TokenKind::AmpMut
            | TokenKind::KwMove => Some(90),
            _ => None,
        }
    }

    pub(crate) fn infix_bp(kind: &TokenKind) -> Option<(u8, u8)> {
        match kind {
            TokenKind::DotDot | TokenKind::DotDotEq => Some((25, 26)),
            TokenKind::Pipe         => Some((5, 6)),    // |>
            TokenKind::OrOr         => Some((10, 11)),
            // `??` sedikit lebih erat dari `||` dan lebih longgar dari `&&`, dan
            // ASOSIATIF KANAN — `a ?? b ?? c` berarti `a ?? (b ?? c)`, yaitu rantai …
            TokenKind::QuestionQuestion => Some((15, 14)),
            TokenKind::AndAnd       => Some((20, 21)),
            TokenKind::EqEq | TokenKind::BangEq
            | TokenKind::Lt | TokenKind::LtEq
            | TokenKind::Gt | TokenKind::GtEq => Some((30, 31)),
            TokenKind::Or           => Some((40, 41)),
            TokenKind::Caret        => Some((50, 51)),
            TokenKind::And | TokenKind::Amp | TokenKind::AndCaret => Some((60, 61)),
            TokenKind::Shl | TokenKind::Shr => Some((70, 71)),
            TokenKind::Plus | TokenKind::Minus => Some((80, 81)),
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some((85, 86)),
            TokenKind::StarStar      => Some((88, 87)), // right-associative
            _ => None,
        }
    }

    /// `**` mengikat lebih erat dari `-`/`~` di kirinya (`-2 ** 2` = `-(2 ** 2)` = -4, seperti Python
    /// dan matematika), tapi `as` tetap lebih longgar dari prefix (`-128 as i8` = `(-128) as i8`).
    fn pangkat_di_bawah_prefix(&mut self, operand: Expr) -> Result<Expr, ParseError> {
        if !self.at_exact(&TokenKind::StarStar) { return Ok(operand); }
        let lo = operand.span();
        self.advance();
        let rhs = Box::new(self.parse_expr_with_bp(87)?);
        Ok(Expr::Binary { op: BinaryOp::Pow, lhs: Box::new(operand), rhs, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_expr_with_bp(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let lo = self.span_lo();
        let mut lhs = self.parse_prefix()?;

        loop {
            // Check for infix operators first (before postfix, to respect bp)
            {
                let kind = &self.current().kind;
                if let Some((l_bp, r_bp)) = Self::infix_bp(kind) {
                    if l_bp < min_bp { break; }
                    let op = self.token_to_binary_op();
                    self.advance();

                    // `a ?? b` — DITURUNKAN di sini jadi `match`.
                    if matches!(&self.tokens[self.pos - 1].kind, TokenKind::QuestionQuestion) {
                        // `a ??= b` — ditolak sebagai KEPUTUSAN, bukan celah.
                        if self.at_exact(&TokenKind::Eq) {
                            return Err(self.error(concat!(
                                "`??=` tidak ada di JRG, dan itu keputusan. `??` MEMBUKA Option-nya, ",
                                "jadi `a ??= b` cuma bisa berarti `a = a ?? b` — dan itu mengubah tipe ",
                                "`a` dari `Option<T>` jadi `T` bahkan waktu isinya ada, kebalikan dari ",
                                "arti `??=` di bahasa lain. Tulis maksudnya: `a = a ?? b` kalau memang ",
                                "mau dibuka, atau `if a.is_none() { a = Option::Some(b) }` kalau mau ",
                                "diisi-kalau-kosong. Untuk dict ada `d.get_or(kunci, bawaan)`").to_string()));
                        }
                        let rhs = self.parse_expr_with_bp(r_bp)?;
                        let span = self.merge_span(lo);
                        // Nama sementara yang tidak bisa ditulis programmer:
                        // sebuah `$` tidak pernah sah di tengah identifier.
                        let sementara = format!("{}{}", AWALAN_KOALESENSI, self.pos);
                        lhs = Expr::Match {
                            scrutinee: Box::new(lhs),
                            arms: vec![
                                MatchArm {
                                    patterns: vec![Pattern::Ident {
                                        name: "None".to_string(), is_mut: false, span }],
                                    guard: None,
                                    body: MatchBody::Expr(Box::new(rhs)),
                                    span,
                                },
                                MatchArm {
                                    patterns: vec![Pattern::Enum {
                                        ty: TypeExpr::Named(NamedType {
                                            package: None, modul: None, name: "Some".to_string(),
                                            args: Vec::new(), span }),
                                        variant: "Some".to_string(),
                                        inner: Some(Box::new(Pattern::Ident {
                                            name: sementara.clone(), is_mut: false, span })),
                                        span,
                                    }],
                                    guard: None,
                                    body: MatchBody::Expr(Box::new(Expr::Ident(sementara, span))),
                                    span,
                                },
                            ],
                            span,
                        };
                        continue;
                    }

                    // Pipe: lhs |> rhs_call
                    if matches!(&self.tokens[self.pos - 1].kind, TokenKind::Pipe) {
                        let rhs = Box::new(self.parse_expr_with_bp(r_bp)?);
                        lhs = Expr::Pipe { lhs: Box::new(lhs), rhs, span: self.merge_span(lo) };
                        continue;
                    }

                    // Range operators: `lo..hi` and `lo..=hi`
                    let prev_tok = &self.tokens[self.pos - 1].kind;
                    if matches!(prev_tok, TokenKind::DotDot | TokenKind::DotDotEq) {
                        let inclusive = matches!(prev_tok, TokenKind::DotDotEq);
                        let hi_expr = Box::new(self.parse_expr_with_bp(r_bp)?);
                        lhs = Expr::Range {
                            lo: Some(Box::new(lhs)),
                            hi: Some(hi_expr),
                            inclusive,
                            span: self.merge_span(lo),
                        };
                        continue;
                    }
                    let rhs = Box::new(self.parse_expr_with_bp(r_bp)?);
                    lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs, span: self.merge_span(lo) };
                    continue;
                }
            }

            // Postfix operations: ?, .await, [], ., (args), as
            lhs = match &self.current().kind {
                // `?` sekarang SELALU propagasi error — tidak ada lagi yang perlu dibedakan.
                TokenKind::Question => {
                    self.advance();
                    Expr::Propagate { expr: Box::new(lhs), span: self.merge_span(lo) }
                }
                TokenKind::Dot => {
                    self.advance();
                    if self.at_exact(&TokenKind::KwAwait) {
                        self.advance();
                        Expr::Await { expr: Box::new(lhs), span: self.merge_span(lo) }
                    } else {
                        // `expect_ident_or_kw` (bukan `expect_ident`) supaya method yang namanya
                        // collide dengan keyword (`.map(...)`, `.filter(...)`, dst — persis seperti …
                        let field = self.expect_ident_or_kw()?;
                        if self.at_exact(&TokenKind::LParen) {
                            self.advance();
                            let args = self.parse_call_args()?;
                            self.expect(&TokenKind::RParen)?;
                            Expr::Method { obj: Box::new(lhs), method: field, args, span: self.merge_span(lo) }
                        } else {
                            Expr::Field { obj: Box::new(lhs), field, span: self.merge_span(lo) }
                        }
                    }
                }
                TokenKind::LBracket => {
                    self.advance();
                    // `a[..]`/`a[..3]` — sintaks Rust, bukan JRG.
                    if self.at_exact(&TokenKind::DotDot) || self.at_exact(&TokenKind::DotDotEq) {
                        return Err(self.error(concat!(
                            "slice di JRG memakai `:`, bukan `..`. Seluruh isinya `a[:]`, ",
                            "dari depan `a[:3]`, sampai belakang `a[2:]`, sepotong ",
                            "`a[1:3]` — dan `..` tetap berarti rentang di dalam `match` ",
                            "(`1..=9`) dan di `range`").to_string()));
                    }
                    if let Some(e) = self.tolak_slice_berlangkah() { return Err(e); }
                    if self.eat(&TokenKind::Colon) {
                        let hi = if !self.at_exact(&TokenKind::RBracket) { Some(Box::new(self.parse_expr()?)) } else { None };
                        self.expect(&TokenKind::RBracket)?;
                        Expr::Slice { obj: Box::new(lhs), lo: None, hi, span: self.merge_span(lo) }
                    } else {
                        let idx = self.parse_expr()?;
                        if self.eat(&TokenKind::Colon) {
                            let hi = if !self.at_exact(&TokenKind::RBracket) { Some(Box::new(self.parse_expr()?)) } else { None };
                            if self.at_exact(&TokenKind::Colon) || self.at_exact(&TokenKind::ColonColon) {
                                if let Some(e) = self.pesan_slice_berlangkah() { return Err(e); }
                            }
                            self.expect(&TokenKind::RBracket)?;
                            Expr::Slice { obj: Box::new(lhs), lo: Some(Box::new(idx)), hi, span: self.merge_span(lo) }
                        } else {
                            self.expect(&TokenKind::RBracket)?;
                            Expr::Index { obj: Box::new(lhs), idx: Box::new(idx), span: self.merge_span(lo) }
                        }
                    }
                }
                TokenKind::LParen => {
                    self.advance();
                    let (args, bernama) = self.parse_arg_penuh()?;
                    let sebar = self.arg_sebar.split_off(0);
                    self.expect(&TokenKind::RParen)?;
                    let sp = self.merge_span(lo);
                    self.bangun_panggilan_penuh(lhs, args, sebar, bernama, sp)
                }
                // `as` lebih longgar dari operator prefix (90) dan lebih erat dari
                // `**` (88), seperti Rust: `-128 as i8` berarti `(-128) as i8`. Dulu
                // `as` ikut mengikat operand-nya, jadi `-5 as uint` menjawab `-5`.
                TokenKind::KwAs if min_bp > 89 => break,
                TokenKind::KwAs => {
                    self.advance();
                    let lo_ty = self.span_lo();
                    let ty = Box::new(self.parse_type()?);
                    // Ditolak di sini supaya ketiga mesin berbagi satu kalimat dan
                    // satu saat: dulu interpreter menolak waktu JALAN (atau, untuk
                    // `as unit`, diam-diam tidak melakukan apa pun) dan VM saat kompilasi.
                    if !crate::interpreter::konversi::sasaran_cast_sah(&ty) {
                        return Err(ParseError {
                            msg: crate::interpreter::konversi::pesan_sasaran_cast_tak_sah(
                                &super::nama_tipe(&ty)),
                            span: self.merge_span(lo_ty),
                        });
                    }
                    Expr::Cast { expr: Box::new(lhs), ty, span: self.merge_span(lo) }
                }
                _ => break,
            };
        }

        // Operator ternary: `cond $ then : else`.
        if min_bp == 0 && self.at_exact(&TokenKind::Dollar) {
            self.advance(); // `$`
            let then = Box::new(self.parse_expr_with_bp(0)?);
            self.expect(&TokenKind::Colon)?;
            let else_ = Box::new(self.parse_expr_with_bp(0)?);
            return Ok(Expr::Ternary { cond: Box::new(lhs), then, else_, span: self.merge_span(lo) });
        }

        Ok(lhs)
    }

    pub(crate) fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        let lo = self.span_lo();
        // Diambil di SINI, oleh token apa pun: cuma prefiks yang langsung menyusul `-`
        // yang boleh melihatnya.
        let di_bawah_minus = std::mem::take(&mut self.minus_baru);
        match &self.current().kind {
            // Unary operators
            TokenKind::Minus => {
                // `--i` DITERIMA dan diam-diam menjawab `i`: dua minus unary saling
                // meniadakan.
                if self.dua_tanda_menempel() || self.tanda_kembar_di_belakang() {
                    return Err(self.error(PESAN_KURANG.to_string()));
                }
                self.advance();
                self.literal_min = None;
                self.minus_baru = true;
                let expr = self.parse_expr_with_bp(90)?;
                let expr = self.pangkat_di_bawah_prefix(expr)?;
                self.minus_baru = false;
                // Literal 2^127 sah cuma sebagai operand LANGSUNG: `-1701…728.abs()` berarti
                // `-(1701…728.abs())`, dan di sana literalnya berdiri sendiri.
                if let Some(sp) = self.literal_min.take() {
                    if !matches!(&expr, Expr::Literal(LitKind::Int(v), s)
                                 if *v == i128::MAX as u128 + 1 && *s == sp) {
                        return Err(ParseError {
                            msg: crate::interpreter::pesan_literal_luber(
                                &(i128::MAX as u128 + 1).to_string()),
                            span: sp,
                        });
                    }
                }
                Ok(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr), span: self.merge_span(lo) })
            }
            TokenKind::Bang => {
                self.advance();
                let expr = Box::new(self.parse_expr_with_bp(90)?);
                Ok(Expr::Unary { op: UnaryOp::Not, expr, span: self.merge_span(lo) })
            }
            TokenKind::Tilde => {
                self.advance();
                let expr = self.parse_expr_with_bp(90)?;
                let expr = Box::new(self.pangkat_di_bawah_prefix(expr)?);
                Ok(Expr::Unary { op: UnaryOp::BitNot, expr, span: self.merge_span(lo) })
            }
            // `*p`, `&x`, `&mut x` DITOLAK, dan itu bukan fitur yang belum sempat
            // ditulis.
            TokenKind::Star | TokenKind::Amp | TokenKind::AmpMut => {
                let simbol = match self.current().kind {
                    TokenKind::Star => "*",
                    TokenKind::AmpMut => "&mut",
                    _ => "&",
                };
                Err(self.error(format!(
                    "`{}` sebagai operator pointer tidak ada di JRG — nilai di sini tidak pernah beralias, jadi `&x` tidak bisa merujuk `x` dan `*p` tidak bisa menulis balik ke sana\n bantuan: kalau butuh sesuatu yang benar-benar dibagi, pakai `ptr::new(v)` dengan `ptr::get`/`ptr::set` — itu berbagi lintas nama maupun lintas fungsi\n bantuan: kalau butuh alokasi eksplisit berikut deteksi use-after-free, pakai `jawa::alloc`/`jawir::free`\n catatan: sebelum ini `{}` diterima dan TIDAK berbuat apa-apa, jadi programnya jalan sambil diam-diam memakai salinan",
                    simbol, simbol
                )))
            }
            TokenKind::KwMove => {
                self.advance();
                // move |params| expr  → closure
                if self.at_exact(&TokenKind::Or) || self.at_exact(&TokenKind::OrOr) {
                    return self.parse_closure(true);
                }
                let expr = Box::new(self.parse_expr_with_bp(90)?);
                Ok(Expr::Unary { op: UnaryOp::Move, expr, span: self.merge_span(lo) })
            }
            // await expr (prefix form)
            TokenKind::KwAwait => {
                self.advance();
                let expr = Box::new(self.parse_expr_with_bp(1)?);
                Ok(Expr::Await { expr, span: self.merge_span(lo) })
            }
            // Channel receive: <-chan
            TokenKind::LArrow => {
                self.advance();
                let chan = Box::new(self.parse_expr_with_bp(90)?);
                Ok(Expr::Recv { chan, span: self.merge_span(lo) })
            }
            // match scrutinee { pat => body, ... }  — as an expression.
            TokenKind::KwMatch => {
                self.advance();
                let scrutinee = Box::new(self.parse_header_expr()?);
                let arms = self.parse_match_arms_block()?;
                Ok(Expr::Match { scrutinee, arms, span: self.merge_span(lo) })
            }
            // if cond { .. } else { .. }  — as an expression. Delegates to
            // the statement parser so `else if` chains and `if init; cond`
            // headers are parsed by exactly one piece of code.
            TokenKind::KwIf if matches!(self.peek_kind(1), Some(TokenKind::KwLet)) =>
                self.parse_if_let(),
            TokenKind::KwIf => {
                let stmt = self.parse_if_stmt()?;
                Ok(Expr::If(Box::new(stmt)))
            }
            // JRG-style closure: fn(params) => expr  or  fn(params) { block }
            // params can be bare identifiers (no type annotation needed)
            TokenKind::KwFn => {
                self.advance(); // consume `fn`
                self.expect(&TokenKind::LParen)?;
                let mut params = Vec::new();
                while !self.at_exact(&TokenKind::RParen) && !self.at_exact(&TokenKind::Eof) {
                    let plo = self.span_lo();
                    let is_mut = self.eat(&TokenKind::KwMut);
                    let name = self.expect_ident_or_kw()?;
                    let pattern = Pattern::Ident {
                        name,
                        is_mut,
                        span: self.merge_span(plo),
                    };
                    // Optional type annotation
                    let ty = if self.eat(&TokenKind::Colon) {
                        self.parse_type()?
                    } else {
                        TypeExpr::Inferred
                    };
                    params.push(FnParam {
                        is_variadic: false,
                        is_mut,
                        borrow: BorrowKind::None,
                        pattern,
                        ty,
                        default: None, // closure params: belum didukung di iterasi ini
                        lifetime: None,
                        span: self.merge_span(plo),
                    });
                    if !self.eat(&TokenKind::Comma) { break; }
                }
                self.expect(&TokenKind::RParen)?;
                // optional return type annotation: `-> Type`
                if self.at_exact(&TokenKind::Arrow) {
                    self.advance();
                    let _ = self.parse_type();
                }
                let body = Box::new(if self.at_exact(&TokenKind::LBrace) {
                    Expr::Block(self.parse_block()?)
                } else {
                    // `=>` (FatArrow) then expression
                    if self.at_exact(&TokenKind::FatArrow) { self.advance(); }
                    self.parse_expr_with_bp(0)?
                });
                Ok(Expr::Closure { params, is_move: false, ret: None, body: body.into(), span: self.merge_span(lo) })
            }
            // Rust-style closure: |params| expr  or  move |params| expr
            TokenKind::Or | TokenKind::OrOr => {
                self.parse_closure(false)
            }
            // Literals
            TokenKind::IntLit(v)  => {
                let v = *v;
                self.advance();
                let sp = self.merge_span(lo);
                // Di atas `i128::MAX` literalnya dulu MEMBUNGKUS di interpreter
                // (`2^127` dicetak negatif) dan ditolak tanpa lokasi di `--vm`.
                if v > i128::MAX as u128 {
                    if v == i128::MAX as u128 + 1 && di_bawah_minus {
                        self.literal_min = Some(sp);
                    } else {
                        return Err(ParseError {
                            msg: crate::interpreter::pesan_literal_luber(&v.to_string()),
                            span: sp,
                        });
                    }
                }
                Ok(Expr::Literal(LitKind::Int(v), sp))
            }
            TokenKind::FloatLit(v) => { let v = *v; self.advance(); Ok(Expr::Literal(LitKind::Float(v), self.merge_span(lo))) }
            TokenKind::ImaginaryLit(v) => { let v = *v; self.advance(); Ok(Expr::Literal(LitKind::Imag(v), self.merge_span(lo))) }
            TokenKind::RuneLit(c) => { let c = *c; self.advance(); Ok(Expr::Literal(LitKind::Rune(c), self.merge_span(lo))) }
            TokenKind::StringLit(s) => { let s = s.clone(); self.advance(); Ok(Expr::Literal(LitKind::Str(s), self.merge_span(lo))) }
            TokenKind::RawStringLit(s) => { let s = s.clone(); self.advance(); Ok(Expr::Literal(LitKind::RawStr(s), self.merge_span(lo))) }
            TokenKind::TemplateLitStart => self.parse_template_lit(lo),
            TokenKind::BoolLit(b) => { let b = *b; self.advance(); Ok(Expr::Literal(LitKind::Bool(b), self.merge_span(lo))) }
            TokenKind::NilLit     => { self.advance(); Ok(Expr::Literal(LitKind::Nil, self.merge_span(lo))) }
            // Grouped expression or tuple
            TokenKind::LParen => {
                self.advance();
                if self.eat(&TokenKind::RParen) {
                    return Ok(Expr::Tuple { elems: Vec::new(), span: self.merge_span(lo) });
                }
                // Di dalam kurung, batas header sudah jelas — jadi literal
                // struct boleh lagi: `if (K { a: 1 }).a > 0 { ... }` sah.
                let first = self.dgn_struct_literal_boleh(|p| p.parse_expr())?;
                if self.eat(&TokenKind::Comma) {
                    let mut elems = vec![first];
                    while !self.at_exact(&TokenKind::RParen) {
                        elems.push(self.dgn_struct_literal_boleh(|p| p.parse_expr())?);
                        if !self.eat(&TokenKind::Comma) { break; }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Tuple { elems, span: self.merge_span(lo) })
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }
            // Array literal or repeat
            TokenKind::LBracket => {
                self.advance();
                if self.eat(&TokenKind::RBracket) {
                    return Ok(Expr::Array { elems: Vec::new(), span: self.merge_span(lo) });
                }
                let sebar0 = self.eat(&TokenKind::DotDotDot);
                let first = self.dgn_struct_literal_boleh(|p| p.parse_expr())?;
                if !sebar0 && self.eat(&TokenKind::Semi) {
                    let count = Box::new(self.parse_expr()?);
                    self.expect(&TokenKind::RBracket)?;
                    Ok(Expr::ArrayRepeat { elem: Box::new(first), count, span: self.merge_span(lo) })
                } else {
                    let mut elems = vec![first];
                    let mut sebar = vec![sebar0];
                    while self.eat(&TokenKind::Comma) {
                        if self.at_exact(&TokenKind::RBracket) { break; }
                        sebar.push(self.eat(&TokenKind::DotDotDot));
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RBracket)?;
                    let span = self.merge_span(lo);
                    Ok(self.gabung_sebar_array(elems, sebar, span))
                }
            }
            // Block expression
            TokenKind::LBrace => {
                // `{` di posisi ekspresi bisa berarti dua hal: blok (`{ stmt; stmt }`) atau
                // literal dict (`{"a": 1}`).
                if self.looks_like_dict_literal() {
                    self.parse_dict_literal()
                } else {
                    Ok(Expr::Block(self.parse_block()?))
                }
            }
            // make_chan
            TokenKind::Ident(name) if name == "make_chan" => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let ty = Box::new(self.parse_type()?);
                let cap = if self.eat(&TokenKind::Comma) { Some(Box::new(self.parse_expr()?)) } else { None };
                self.expect(&TokenKind::RParen)?;
                Ok(Expr::MakeChan { ty, cap, span: self.merge_span(lo) })
            }
            // `chan` kata kunci tipe (`chan int`), tapi `chan::len(c)` sebuah path —
            // namespace yang didaftarkan dua mesin dan dulu tidak bisa ditulis.
            TokenKind::KwChan if matches!(self.peek_kind(1), Some(TokenKind::ColonColon)) => {
                self.advance();
                self.advance();
                let name = self.expect_ident_or_kw()?;
                Ok(Expr::PathExpr { segments: vec!["chan".to_string(), name], span: self.merge_span(lo) })
            }
            // Identifier / path / struct literal
            TokenKind::Ident(_) => {
                self.parse_ident_or_struct(lo)
            }
            // `::nama`, `::ns::nama` — bawaan bahasa, tanpa peduli deklarasi program. Segmen
            // pertamanya kosong; penurunan parser (sebar, argumen bernama, literal dict)
            // memakai bentuk yang sama.
            TokenKind::ColonColon => {
                let mut segments = vec![String::new()];
                while self.at_exact(&TokenKind::ColonColon) {
                    self.advance();
                    segments.push(self.expect_ident_or_kw()?);
                }
                Ok(Expr::PathExpr { segments, span: self.merge_span(lo) })
            }
            // `self` as expression (receiver in method body)
            TokenKind::KwSelf => {
                let span = self.merge_span(lo);
                self.advance();
                Ok(Expr::Ident("self".to_string(), span))
            }
            other => {
                // Bentuk dari bahasa lain dijawab padanannya, bukan "harusnya
                // ada ekspresi di sini" — yang benar tentang tokennya dan diam
                // soal apa yang sedang dicoba.
                if let Some(pesan) = self.bentuk_asing_di_ekspresi() {
                    return Err(self.error(pesan));
                }
                Err(self.error(format!("harusnya ada ekspresi di sini, yang ketemu malah {}", other.tulisan())))
            }
        }
    }

    /// Bentuk yang dibawa dari bahasa lain dan ditemui di posisi EKSPRESI.
    pub(crate) fn dua_tanda_menempel(&self) -> bool {
        let a = self.current();
        let Some(b) = self.tokens.get(self.pos + 1) else { return false };
        if a.span.hi != b.span.lo { return false; }
        matches!((&a.kind, &b.kind),
            (TokenKind::Plus, TokenKind::Plus) | (TokenKind::Minus, TokenKind::Minus))
    }

    /// Benar kalau tanda SEBELUM yang sekarang sama dan menempel padanya —
    /// bentuk `i--`, di mana tanda pertamanya sudah dimakan sebagai operator
    /// biner sebelum yang kedua sampai ke sini.
    pub(crate) fn tanda_kembar_di_belakang(&self) -> bool {
        let kini = self.current();
        let Some(i) = self.pos.checked_sub(1) else { return false };
        let Some(sebelum) = self.tokens.get(i) else { return false };
        if sebelum.span.hi != kini.span.lo { return false; }
        matches!((&sebelum.kind, &kini.kind),
            (TokenKind::Plus, TokenKind::Plus) | (TokenKind::Minus, TokenKind::Minus))
    }

    pub(crate) fn bentuk_asing_di_ekspresi(&self) -> Option<String> {
        // Dilihat MUNDUR juga, bukan cuma maju.
        let menempel_mundur = |n: usize| -> bool {
            let (Some(a), Some(b)) = (
                self.pos.checked_sub(n).and_then(|i| self.tokens.get(i)),
                self.pos.checked_sub(n.saturating_sub(1)).and_then(|i| self.tokens.get(i)),
            ) else { return false };
            a.span.hi == b.span.lo
        };
        let kb = |n: usize| self.pos.checked_sub(n).and_then(|i| self.tokens.get(i)).map(|t| &t.kind);

        // `++i` di awal: dua tanda menempel, dan yang sekarang tanda pertamanya.
        if self.dua_tanda_menempel() {
            return Some(match self.current().kind {
                TokenKind::Plus => PESAN_TAMBAH.to_string(),
                _ => PESAN_KURANG.to_string(),
            });
        }
        // `i++`: `+` pertama sudah dimakan operator biner.
        if matches!(self.current().kind, TokenKind::Plus)
            && matches!(kb(1), Some(TokenKind::Plus))
            && menempel_mundur(1)
        {
            return Some(PESAN_TAMBAH.to_string());
        }
        None
    }

    pub(crate) fn parse_ident_or_struct(&mut self, lo: Span) -> Result<Expr, ParseError> {
        let mut segments = vec![self.expect_ident()?];
        while self.at_exact(&TokenKind::ColonColon) && !matches!(self.peek().kind, TokenKind::Lt) {
            self.advance();
            // After `::`, accept keywords too as identifiers (e.g. arr::map, arr::filter)
            segments.push(self.expect_ident_or_kw()?);
        }
        // Struct literal: TypeName { field: expr, ... }
        // We check: next is `{` AND this is a type-like name context
        if self.at_exact(&TokenKind::LBrace) && self.looks_like_struct_literal() {
            let ty = TypeExpr::Named(NamedType {
                package: if segments.len() > 1 { Some(segments[0].clone()) } else { None },
                modul: None,
                name: segments.last().unwrap().clone(),
                args: Vec::new(),
                span: lo,
            });
            self.advance(); // {
            let mut fields = Vec::new();
            let mut spread = None;
            while !self.at_exact(&TokenKind::RBrace) {
                if self.at_exact(&TokenKind::DotDotDot) {
                    self.advance();
                    spread = Some(Box::new(self.parse_expr()?));
                    self.eat(&TokenKind::Comma);
                    break;
                }
                let flo = self.span_lo();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let value = self.parse_expr()?;
                fields.push(FieldInit { name, value, span: self.merge_span(flo) });
                if !self.eat(&TokenKind::Comma) { break; }
            }
            self.expect(&TokenKind::RBrace)?;
            return Ok(Expr::Struct { ty, fields, spread, span: self.merge_span(lo) });
        }
        if segments.len() == 1 {
            Ok(Expr::Ident(segments.into_iter().next().unwrap(), self.merge_span(lo)))
        } else {
            Ok(Expr::PathExpr { segments, span: self.merge_span(lo) })
        }
    }

    /// Apakah `{` sesudah sebuah nama membuka LITERAL STRUCT, bukan blok?
    pub(crate) fn looks_like_struct_literal(&self) -> bool {
        // Di header `if`/`while`/`for`/`match`, `{` SELALU membuka badan.
        if self.larang_struct_literal { return false; }
        // `A { ...a }`: blok tidak pernah diawali `...`, jadi ini pasti literal.
        if matches!(self.peek().kind, TokenKind::RBrace | TokenKind::DotDotDot) { return true; }
        let mut i = self.pos + 1;
        let mut depth = 1;
        while i < self.tokens.len() && depth > 0 {
            match &self.tokens[i].kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => { depth -= 1; }
                TokenKind::Colon if depth == 1 => return true,
                TokenKind::Semi  if depth == 1 => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// `« teks ${ekspresi} teks »`.
    pub(crate) fn parse_template_lit(&mut self, lo: Span) -> Result<Expr, ParseError> {
        self.advance(); // `«`
        let mut parts: Vec<TemplatePart> = Vec::new();
        loop {
            match self.current().kind.clone() {
                TokenKind::TemplateLitEnd => {
                    self.advance();
                    return Ok(Expr::Template { parts, span: self.merge_span(lo) });
                }
                TokenKind::StringLit(text) => {
                    self.advance();
                    parts.push(TemplatePart::Text(text));
                }
                TokenKind::TemplateInterpStart => {
                    self.advance();
                    // `${}` kosong hampir pasti salah ketik, dan kalau
                    // dibiarkan akan jadi teks kosong tanpa peringatan —
                    // persis jenis "hasil diam-diam salah" yang dilarang.
                    if self.at_exact(&TokenKind::TemplateInterpEnd) {
                        return Err(self.error(
                            "`${}` di dalam template kosong — isi dengan ekspresi yang mau ditempel, \
                             atau tulis `$` biasa kalau memang mau karakter dolar".to_string(),
                        ));
                    }
                    let e = self.parse_expr()?;
                    self.skip_semis();
                    if !self.eat(&TokenKind::TemplateInterpEnd) {
                        return Err(self.error(
                            "`${` di template ini belum ditutup `}`".to_string(),
                        ));
                    }
                    parts.push(TemplatePart::Expr(e));
                }
                // Lexer selalu mengakhiri template dengan `TemplateLitEnd`
                // atau gagal duluan, jadi ini cuma jaring pengaman.
                _ => {
                    return Err(self.error(
                        "template literal `«...»` belum ditutup `»`".to_string(),
                    ));
                }
            }
        }
    }

    pub(crate) fn parse_closure(&mut self, is_move: bool) -> Result<Expr, ParseError> {
        let lo = self.span_lo();
        let params = if self.eat(&TokenKind::OrOr) {
            Vec::new()  // || means no params
        } else {
            self.expect(&TokenKind::Or)?;
            let mut ps = Vec::new();
            while !self.at_exact(&TokenKind::Or) {
                ps.push(self.parse_fn_param()?);
                if !self.eat(&TokenKind::Comma) { break; }
            }
            self.expect(&TokenKind::Or)?;
            ps
        };
        // `|x| -> int { … }` — sejajar dengan `fn f(x) -> int`.
        let ret = if self.eat(&TokenKind::Arrow) { Some(self.parse_type()?) } else { None };
        let body = Box::new(if self.at_exact(&TokenKind::LBrace) {
            Expr::Block(self.parse_block()?)
        } else {
            self.parse_expr()?
        });
        let (params, body) = bongkar_pola_closure(params, body);
        Ok(Expr::Closure { params, is_move, ret, body: body.into(), span: self.merge_span(lo) })
    }

    pub(crate) fn parse_call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        Ok(self.parse_call_args_sebar()?.0)
    }

    /// Argumen pemanggilan, plus penanda mana yang di-`...`-kan.
    pub(crate) fn parse_call_args_sebar(&mut self) -> Result<(Vec<Expr>, Vec<bool>), ParseError> {
        Ok((self.parse_arg_penuh()?.0, self.arg_sebar.split_off(0)))
    }

    /// Argumen pemanggilan lengkap: posisional, `...`-kan, dan BERNAMA.
    pub(crate) fn parse_arg_penuh(&mut self)
        -> Result<(Vec<Expr>, Vec<(String, Expr)>), ParseError>
    {
        let mut args = Vec::new();
        let mut bernama: Vec<(String, Expr)> = Vec::new();
        self.arg_sebar.clear();
        while !self.at_exact(&TokenKind::RParen) {
            let ini_sebar = self.eat(&TokenKind::DotDotDot);
            let nama = if !ini_sebar { self.nama_argumen() } else { None };
            // Argumen berada di dalam `(...)`, jadi larangan literal struct
            // milik header kontrol tidak berlaku lagi di sini:
            // `if punya(K { a: 1 }) { ... }` sah.
            let e = self.dgn_struct_literal_boleh(|p| p.parse_expr())?;
            match nama {
                Some(n) => bernama.push((n, e)),
                None => {
                    if !bernama.is_empty() {
                        // Argumen posisional SESUDAH yang bernama ditolak: posisinya jadi bergantung
                        // pada nama yang mendahului, dan itu aturan yang tidak bisa dibaca dari …
                        return Err(self.error(
                            "argumen posisional tidak boleh sesudah argumen bernama"
                                .to_string()));
                    }
                    args.push(e);
                    self.arg_sebar.push(ini_sebar);
                }
            }
            if !self.eat(&TokenKind::Comma) { break; }
        }
        Ok((args, bernama))
    }

    /// `nama:` di awal sebuah argumen — dikonsumsi kalau ada.
    pub(crate) fn nama_argumen(&mut self) -> Option<String> {
        let TokenKind::Ident(n) = &self.current().kind else { return None };
        if !matches!(self.peek_kind(1), Some(TokenKind::Colon)) { return None; }
        let n = n.clone();
        self.advance();
        self.advance();
        Some(n)
    }

    /// Bangun ekspresi pemanggilan, menurunkan spread kalau ada.
    pub(crate) fn bangun_panggilan_penuh(
        &mut self, func: Expr, args: Vec<Expr>, sebar: Vec<bool>,
        bernama: Vec<(String, Expr)>, span: Span,
    ) -> Expr {
        if bernama.is_empty() {
            return self.bangun_panggilan(func, args, sebar, span);
        }
        let posisional = if sebar.iter().any(|x| *x) {
            // `f(...v, b: 2)`: posisionalnya sendiri sudah berupa array.
            match self.bangun_panggilan(Expr::Ident("__x".to_string(), span),
                                        args, sebar, span) {
                Expr::Call { args: mut a, .. } => a.remove(1),
                lain => lain,
            }
        } else {
            Expr::Array { elems: args, span }
        };
        let (nama, nilai): (Vec<Expr>, Vec<Expr>) = bernama.into_iter()
            .map(|(n, e)| (Expr::Literal(LitKind::Str(n), span), e))
            .unzip();
        Expr::Call {
            func: Box::new(jalur_bawaan(&["terap_bernama"], span)),
            args: vec![
                func,
                posisional,
                Expr::Array { elems: nama, span },
                Expr::Array { elems: nilai, span },
            ],
            span,
        }
    }

    pub(crate) fn bangun_panggilan(
        &mut self, func: Expr, args: Vec<Expr>, sebar: Vec<bool>, span: Span,
    ) -> Expr {
        if !sebar.iter().any(|x| *x) {
            return Expr::Call { func: Box::new(func), args, span };
        }
        // Potongan sebar PERTAMA juga lewat `arr::__sebar`, supaya `f(...5)` ditolak dengan
        // kalimat tentang `...`, bukan tentang `terap` yang tidak pernah ditulis pemakainya.
        let benih_kosong = sebar.first().copied().unwrap_or(false);
        let semua = self.rantai_sebar(args, sebar, span, benih_kosong);
        Expr::Call {
            func: Box::new(jalur_bawaan(&["terap"], span)),
            args: vec![func, semua],
            span,
        }
    }

    /// `[a, ...b, c]` — literal array dengan sebar, diturunkan jadi rantai
    /// `arr::__sebar`. Tanpa pembungkus `terap`: yang keluar SUDAH array.
    pub(crate) fn gabung_sebar_array(
        &mut self, elems: Vec<Expr>, sebar: Vec<bool>, span: Span,
    ) -> Expr {
        if !sebar.iter().any(|x| *x) {
            return Expr::Array { elems, span };
        }
        // Kalau potongan PERTAMA yang disebar, benihnya harus array kosong.
        // `[...a]` yang diturunkan jadi `a` begitu saja membuat `[...5]`
        // menjawab `5` — bukan array, tanpa satu pun galat.
        let benih_kosong = sebar.first().copied().unwrap_or(false);
        self.rantai_sebar(elems, sebar, span, benih_kosong)
    }

    /// Rantai `arr::__sebar` atas potongan sebar (diperiksa array di sana, dengan kalimat tentang
    /// `...`) dan non-sebar (dibungkus jadi array satu isi).
    fn rantai_sebar(
        &mut self, bagian: Vec<Expr>, sebar: Vec<bool>, span: Span, benih_kosong: bool,
    ) -> Expr {
        let bungkus = |e: Expr, s: bool| -> Expr {
            if s { e } else { Expr::Array { elems: vec![e], span } }
        };
        let sambung = |a: Expr, b: Expr| -> Expr {
            Expr::Call {
                func: Box::new(jalur_bawaan(&["arr", "__sebar"], span)),
                args: vec![a, b],
                span,
            }
        };
        let mut iter = bagian.into_iter().zip(sebar);
        let mut semua = if benih_kosong {
            Expr::Array { elems: Vec::new(), span }
        } else {
            let (e0, s0) = iter.next().expect("sebar menuntut minimal satu potongan");
            bungkus(e0, s0)
        };
        for (e, s) in iter {
            semua = sambung(semua, bungkus(e, s));
        }
        semua
    }

    pub(crate) fn token_to_binary_op(&self) -> BinaryOp {
        match &self.current().kind {
            TokenKind::Plus     => BinaryOp::Add,
            TokenKind::Minus    => BinaryOp::Sub,
            TokenKind::Star     => BinaryOp::Mul,
            TokenKind::Slash    => BinaryOp::Div,
            TokenKind::Percent  => BinaryOp::Rem,
            TokenKind::StarStar => BinaryOp::Pow,
            TokenKind::And | TokenKind::Amp => BinaryOp::BitAnd,
            // Range tokens — actual Expr::Range built in parse_expr_with_bp; dummy here
            TokenKind::DotDot | TokenKind::DotDotEq => BinaryOp::Add, // never actually used
            TokenKind::Or       => BinaryOp::BitOr,
            TokenKind::Caret    => BinaryOp::BitXor,
            TokenKind::Shl      => BinaryOp::Shl,
            TokenKind::Shr      => BinaryOp::Shr,
            TokenKind::AndCaret => BinaryOp::AndNot,
            TokenKind::EqEq     => BinaryOp::Eq,
            TokenKind::BangEq   => BinaryOp::Ne,
            TokenKind::Lt       => BinaryOp::Lt,
            TokenKind::LtEq     => BinaryOp::Le,
            TokenKind::Gt       => BinaryOp::Gt,
            TokenKind::GtEq     => BinaryOp::Ge,
            TokenKind::AndAnd   => BinaryOp::And,
            TokenKind::OrOr     => BinaryOp::Or,
            TokenKind::LArrow   => BinaryOp::Chan,
            _                   => BinaryOp::Add, // unreachable in practice
        }
    }
}
