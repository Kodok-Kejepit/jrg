// src/parser/stmt.rs — blok dan pernyataan

use super::*;

impl Parser {
    // ── Block ─────────────────────────────────────────────────────────────

    pub(crate) fn parse_block(&mut self) -> Result<Block, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        let mut tail  = None;
        while !self.at_exact(&TokenKind::RBrace) && !self.at_exact(&TokenKind::Eof) {
            self.skip_semis();
            if self.at_exact(&TokenKind::RBrace) { break; }
            // If the next construct is an expression and there's no `;` after it,
            // it might be the tail expression (block value).
            let mulai = self.pos;
            match self.parse_stmt_or_tail() {
                Ok((stmt, is_tail)) => {
                    if is_tail {
                        tail = Some(Box::new(match stmt {
                            Stmt::Expr(e) => *e,
                            _ => unreachable!(),
                        }));
                        break;
                    } else {
                        stmts.push(stmt);
                    }
                }
                Err(e) => {
                    self.errors.push(e);
                    stmts.push(Stmt::Error(self.current().span));
                    // Galat di dalam kurawal milik pernyataan ini (lengan `match`, closure): kurawal
                    // ITU ditutup dulu. Galat tepat di `}` penutup blok: jangan dilompati, atau
                    // sisa berkas dibaca sebagai isi blok dan satu galat jadi tiga.
                    let dalam = self.kurawal_terbuka(mulai);
                    if dalam > 0 { self.tutup_kurawal(dalam); }
                    else if !self.at_exact(&TokenKind::RBrace) { self.synchronize(); }
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Block { stmts, tail, span: self.merge_span(lo) })
    }

    // Returns (stmt, is_tail_expr)
    pub(crate) fn parse_stmt_or_tail(&mut self) -> Result<(Stmt, bool), ParseError> {
        let mut stmt = self.parse_stmt()?;
        // A trailing bare `{ ..
        if matches!(stmt, Stmt::Block(_)) && !self.at_exact(&TokenKind::Semi) && self.at_exact(&TokenKind::RBrace) {
            if let Stmt::Block(b) = stmt {
                let span = b.span;
                stmt = Stmt::Expr(Box::new(Expr::Block(b)));
                let _ = span;
            }
        }
        // After parsing an Expr stmt, if there's no semicolon and next is `}`,
        // it's the tail expression.
        if matches!(stmt, Stmt::Expr(_)) && !self.at_exact(&TokenKind::Semi) {
            if self.at_exact(&TokenKind::RBrace) {
                return Ok((stmt, true));
            }
        }
        self.eat(&TokenKind::Semi);
        Ok((stmt, false))
    }

    // ── Statements ────────────────────────────────────────────────────────

    /// Bentuk dari bahasa LAIN yang orang wajar coba, ditolak sambil menyebut
    /// padanannya di sini.
    pub(crate) fn tolak_bentuk_asing(&mut self) -> Option<ParseError> {
        let pesan: Option<String> = match &self.current().kind {
            TokenKind::KwDo => Some(concat!(
                "`do { ... } while c` belum ada di JRG. Bentuk yang setara: ",
                "`loop { ... ; if !c { break } }` — badannya tetap jalan sekali ",
                "sebelum syaratnya diperiksa").to_string()),
            TokenKind::KwConst if matches!(self.peek_kind(1), Some(TokenKind::KwFn)) =>
                Some(concat!(
                    "`const fn` belum punya arti di JRG: tidak ada yang dihitung saat ",
                    "kompilasi, jadi sebuah `const fn` akan sama persis dengan `fn` biasa ",
                    "sambil terlihat menjanjikan sesuatu yang lain. Tulis `fn`").to_string()),
            TokenKind::KwStatic => Some(concat!(
                "`static` bukan bentuk JRG. Di tingkat modul pakai `let` (tidak ",
                "bisa ditugasi ulang), `const` (sama, dan biasanya untuk tetapan), ",
                "atau `var` kalau memang mau bisa berubah — ketiganya sudah ",
                "berlaku di tingkat modul dan ikut ter-hoist").to_string()),
            // `else` yang sampai ke sini pasti MENGGANTUNG: milik `if` sudah
            // dikonsumsi `parse_if`, termasuk yang ditulis di baris sendiri.
            TokenKind::KwElse => Some(concat!(
                "`else` di sini tidak menempel pada `if` mana pun. Kalau maksudnya ",
                "`for`/`while` … `else` seperti di Python: JRG tidak punya itu. ",
                "Pakai penanda — `var ketemu = false` di atas loop, `ketemu = true` ",
                "sebelum `break`, lalu `if !ketemu { … }` sesudahnya").to_string()),
            TokenKind::KwCase | TokenKind::KwDefault => Some(concat!(
                "`case`/`default` di sini bukan bentuk JRG — keduanya cuma dipakai ",
                "di dalam `select`. Untuk memilih berdasarkan nilai pakai `match`: ",
                "`match x { 1 => ..., _ => ... }`").to_string()),
            TokenKind::Ident(n)
                if n == "try" && matches!(self.peek_kind(1), Some(TokenKind::LBrace)) =>
                Some(concat!(
                    "JRG tidak punya `try`/`catch`, dan itu keputusan: kegagalan ",
                    "diwakili nilai (`Result`), bukan aliran kendali yang tak terlihat ",
                    "di tanda tangan. Pakai `?` untuk meneruskan kegagalan, atau ",
                    "`match` untuk menanganinya").to_string()),
            TokenKind::Ident(n)
                if n == "switch" && matches!(self.peek_kind(1),
                    Some(TokenKind::IntLit(_)) | Some(TokenKind::Ident(_))) =>
                Some(concat!(
                    "`switch` bukan bentuk JRG — pakai `match`, yang juga menerima pola ",
                    "dan penjaga: `match x { 1 | 2 => ..., n if n > 9 => ..., _ => ... }`")
                    .to_string()),
            _ => None,
        };
        pesan.map(|p| self.error(p))
    }

    pub(crate) fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        if let Some(e) = self.tolak_bentuk_asing() { return Err(e); }
        match &self.current().kind {
            TokenKind::KwLet    => self.parse_let_atau_else(),
            TokenKind::KwVar    => self.parse_var_stmt().map(Stmt::Var),
            TokenKind::KwConst  => self.parse_const_decl(Visibility::Private).map(Stmt::Const),
            // `if` is intentionally *not* special-cased here, for exactly the same
            // reason `match` is not (see the comment below): it now parses through the …
            TokenKind::KwFn if matches!(self.peek_kind(1), Some(TokenKind::Ident(_))) => {
                {
                    // Sebuah `fn` bersarang juga bukan method: ia bahkan tidak
                    // bisa melihat lokal di sekitarnya (lihat `JrgFunction::group`).
                    let d = self.parse_fn_decl(Visibility::Private)?;
                    self.tolak_self_bukan_method(&d)?;
                    Ok(Stmt::Fn(d))
                }
            }
            TokenKind::KwFor    => self.parse_for_stmt().map(Stmt::For),
            TokenKind::KwWhile if matches!(self.peek_kind(1), Some(TokenKind::KwLet)) =>
                self.parse_while_let(None),
            TokenKind::KwWhile  => self.parse_while_stmt().map(Stmt::While),
            TokenKind::KwLoop   => self.parse_loop_stmt().map(Stmt::Loop),
            // Loop berlabel: `'nama: for ...`, `'nama: while ...`, `'nama: loop { ...
            TokenKind::Ident(s) if s.starts_with('\'') && matches!(self.peek().kind, TokenKind::Colon) => {
                let label = s.clone();
                self.advance(); // label
                self.advance(); // ':'
                match &self.current().kind {
                    TokenKind::KwFor => {
                        let mut f = self.parse_for_stmt()?;
                        f.set_label(label);
                        Ok(Stmt::For(f))
                    }
                    TokenKind::KwWhile if matches!(self.peek_kind(1), Some(TokenKind::KwLet)) => {
                        // Labelnya diteruskan ke penurunannya, supaya `break
                        // 'nama` menembus loop hasil turunan itu — bukan
                        // menempel di `loop` tanpa nama dan tersesat.
                        self.parse_while_let(Some(label))
                    }
                    TokenKind::KwWhile => {
                        let mut w = self.parse_while_stmt()?;
                        w.label = Some(label);
                        Ok(Stmt::While(w))
                    }
                    TokenKind::KwLoop => {
                        let mut l = self.parse_loop_stmt()?;
                        l.label = Some(label);
                        Ok(Stmt::Loop(l))
                    }
                    lain => Err(self.error(format!(
                        "label `{}` cuma bisa dipasang tepat di depan `for`, `while`, atau `loop` \
                         — yang ketemu malah {}",
                        label, lain.tulisan()
                    ))),
                }
            }
            // `match` is intentionally *not* special-cased here anymore.
            TokenKind::KwReturn => self.parse_return_stmt().map(Stmt::Return),
            TokenKind::KwBreak  => self.parse_break_stmt().map(Stmt::Break),
            TokenKind::KwContinue => self.parse_continue_stmt().map(Stmt::Continue),
            TokenKind::KwSpawn  => self.parse_spawn_stmt().map(Stmt::Spawn),
            // `go expr` / `go { block }` — Go-flavoured sugar for `spawn`,
            // desugared straight into the same `SpawnStmt` AST node so the
            // interpreter needs no extra code path to support it.
            TokenKind::KwGo     => self.parse_go_stmt().map(Stmt::Spawn),
            TokenKind::KwSelect => self.parse_select_stmt().map(Stmt::Select),
            TokenKind::KwDefer  => self.parse_defer_stmt().map(Stmt::Defer),
            TokenKind::LBrace   => self.parse_block().map(Stmt::Block),
            _ => {
                // Try expression statement (including short decl `x, y := expr`)
                let expr = self.parse_expr()?;
                // Sasaran JAMAK: `a, b = b, a` dan `a, b := 1, 2`.
                if self.at_exact(&TokenKind::Comma) {
                    return self.parse_assign_jamak(expr);
                }
                // Check for short decl: `ident, ident := expr`
                // (already parsed first ident as expr; check for `:=`)
                if self.at_exact(&TokenKind::ColonEq) {
                    if let Expr::Ident(name, _) = &expr {
                        let lo_span = self.span_lo();
                        let names = vec![name.clone()];
                        self.advance(); // :=
                        // More names before :=?  Actually, we handle multi-assign differently:
                        // `a, b := expr` is tokenized as: Ident, Comma, Ident, ColonEq
                        // We already consumed `a`; handle remaining names if `,` before `:=`
                        let values = vec![self.parse_expr()?];
                        return Ok(Stmt::ShortDecl(ShortDeclStmt { names, values,
                            span: self.merge_span(lo_span) }));
                    }
                }
                // Channel send statement: `chan <- value` (outside of a
                // `select` block, where this same shape is handled by
                // `parse_select_stmt`'s dedicated case parsing instead).
                if self.at_exact(&TokenKind::LArrow) {
                    let lo_span = self.span_lo();
                    self.advance(); // <-
                    let val = Box::new(self.parse_expr()?);
                    let send = Expr::Send { chan: Box::new(expr), val, span: self.merge_span(lo_span) };
                    return Ok(Stmt::Expr(Box::new(send)));
                }
                // Check for assignment: `expr = expr` or `expr += expr` etc.
                if let Some(op) = self.try_parse_assign_op() {
                    // Awalnya diambil dari TARGET, bukan dari token saat ini.
                    let awal = expr.span();
                    let targets = vec![expr];
                    self.periksa_sasaran(&targets)?;
                    let values  = self.parse_expr_list()?;
                    return Ok(Stmt::Assign(AssignStmt { targets, op, values,
                        span: self.merge_span(awal) }));
                }
                // `wok "halo"` — pemanggilan tanpa kurung. Lihat
                // `recover_parenless_call`.
                if let Some(call) = self.recover_parenless_call(&expr)? {
                    return Ok(Stmt::Expr(Box::new(call)));
                }
                Ok(Stmt::Expr(Box::new(expr)))
            }
        }
    }

    /// Pemulihan untuk pemanggilan fungsi yang lupa tanda kurung: `wok "halo"`
    /// dimaksudkan sebagai `wok("halo")`.
    pub(crate) fn looks_like_dict_literal(&self) -> bool {
        // self.current() adalah `{`
        let k1 = self.tokens.get(self.pos + 1).map(|t| &t.kind);
        let k2 = self.tokens.get(self.pos + 2).map(|t| &t.kind);
        matches!(
            (k1, k2),
            (Some(TokenKind::StringLit(_)), Some(TokenKind::Colon))
                | (Some(TokenKind::RawStringLit(_)), Some(TokenKind::Colon))
                | (Some(TokenKind::Ident(_)), Some(TokenKind::Colon))
                | (Some(TokenKind::IntLit(_)), Some(TokenKind::Colon))
        )
    }

    /// `{"nama": "budi", "umur": 17}` — literal dict.
    pub(crate) fn parse_dict_literal(&mut self) -> Result<Expr, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::LBrace)?;
        let mut pairs = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) && !self.at_exact(&TokenKind::Eof) {
            self.skip_semis();
            if self.at_exact(&TokenKind::RBrace) { break; }
            let key_span = self.span_lo();
            let key = match self.current().kind.clone() {
                TokenKind::StringLit(s) | TokenKind::RawStringLit(s) => { self.advance(); s }
                TokenKind::Ident(s) => { self.advance(); s }
                // Kunci dict adalah bentuk Display-nya, jadi `{1: "a"}` sama dengan `d[1]`.
                TokenKind::IntLit(n) => { self.advance(); n.to_string() }
                other => {
                    return Err(self.error(format!(
                        "key dict harus string, nama, atau bilangan bulat, dapat {}", other.tulisan()
                    )));
                }
            };
            self.expect(&TokenKind::Colon)?;
            let val = self.parse_expr()?;
            let kspan = self.merge_span(key_span);
            pairs.push(Expr::Array {
                elems: vec![Expr::Literal(LitKind::Str(key), kspan), val],
                span: kspan,
            });
            if !self.eat(&TokenKind::Comma) {
                self.skip_semis();
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        let span = self.merge_span(lo);
        Ok(Expr::Call {
            func: Box::new(super::ekspresi::jalur_bawaan(&["dict", "from_pairs"], span)),
            args: vec![Expr::Array { elems: pairs, span }],
            span,
        })
    }

    pub(crate) fn parse_expr_consuming(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_expr()?;
        match self.recover_parenless_call(&expr)? {
            Some(call) => Ok(call),
            None => Ok(expr),
        }
    }

    pub(crate) fn recover_parenless_call(&mut self, callee: &Expr) -> Result<Option<Expr>, ParseError> {
        // Hanya bentuk yang jelas-jelas "nama sesuatu" yang boleh
        // dipulihkan. `1 "x"` atau `(a+b) "x"` bukan salah ketik kurung,
        // itu kesalahan lain — biarkan jalur error biasa yang menangani.
        if !matches!(callee, Expr::Ident(..) | Expr::PathExpr { .. } | Expr::Field { .. }) {
            return Ok(None);
        }
        if !self.starts_unambiguous_argument() {
            return Ok(None);
        }
        let lo = self.span_lo();
        let mut args = vec![self.parse_expr()?];
        while self.eat(&TokenKind::Comma) {
            if self.at_exact(&TokenKind::RParen) || self.at_exact(&TokenKind::RBrace) {
                break;
            }
            args.push(self.parse_expr()?);
        }
        Ok(Some(Expr::Call {
            func: Box::new(callee.clone()),
            args,
            span: self.merge_span(lo),
        }))
    }

    /// Token yang boleh memulai argumen dalam pemulihan tanpa-kurung: hanya yang
    /// TIDAK bisa menjadi lanjutan dari ekspresi sebelumnya, sehingga tafsirannya …
    pub(crate) fn starts_unambiguous_argument(&self) -> bool {
        matches!(
            &self.current().kind,
            TokenKind::StringLit(_)
                | TokenKind::RawStringLit(_)
                | TokenKind::IntLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::RuneLit(_)
                | TokenKind::BoolLit(_)
                | TokenKind::Ident(_)
        )
    }

    /// `a, b = b, a` dan `a, b := 1, 2`.
    pub(crate) fn parse_assign_jamak(&mut self, pertama: Expr) -> Result<Stmt, ParseError> {
        let awal = pertama.span();
        let mut targets = vec![pertama];
        while self.eat(&TokenKind::Comma) {
            targets.push(self.parse_expr()?);
        }

        // `:=` mendeklarasikan, `=` menugaskan. Keduanya sah dengan sasaran
        // jamak; bedanya cuma sasaran `:=` harus nama telanjang.
        if self.at_exact(&TokenKind::ColonEq) {
            self.advance();
            let values = self.parse_expr_list()?;
            let mut names = Vec::with_capacity(targets.len());
            for t in &targets {
                match t {
                    Expr::Ident(n, _) => names.push(n.clone()),
                    lain => return Err(ParseError {
                        msg: "sisi kiri `:=` harus nama variabel — ia MENDEKLARASIKAN, \
                              jadi tidak ada tempat yang bisa ditulisi\n\
                              bantuan: pakai `=` kalau tempatnya memang sudah ada"
                             .to_string(),
                        span: lain.span(),
                    }),
                }
            }
            // `a, b := e`: satu nilai untuk banyak nama membongkar tuple — diturunkan ke
            // `let mut (a, b) = e`, jadi ketiga mesin memakai aturan pola yang sudah ada.
            if names.len() > 1 && values.len() == 1 {
                let span = self.merge_span(awal);
                let elems = targets.iter().map(|t| match t {
                    Expr::Ident(n, s) if n == "_" => Pattern::Wildcard(*s),
                    Expr::Ident(n, s) => Pattern::Ident { name: n.clone(), is_mut: true, span: *s },
                    lain => Pattern::Wildcard(lain.span()),
                }).collect();
                return Ok(Stmt::Let(LetStmt {
                    is_mut: true, ownership: None, pattern: Pattern::Tuple { elems, span }, ty: None,
                    init: Box::new(values.into_iter().next().expect("panjangnya satu")),
                    lifetime: Some(super::ekspresi::PENANDA_BONGKAR_PENDEK.to_string()), span,
                }));
            }
            self.periksa_jumlah_sasaran(names.len(), values.len(), awal, ":=")?;
            return Ok(Stmt::ShortDecl(ShortDeclStmt {
                names, values, span: self.merge_span(awal) }));
        }

        let Some(op) = self.try_parse_assign_op() else {
            return Err(ParseError {
                msg: "daftar dipisah koma di sini cuma sah sebagai sisi kiri penugasan\n\
                      bantuan: tambahkan `= ...` atau `:= ...`, atau pisahkan jadi \
                      beberapa statement"
                     .to_string(),
                span: self.merge_span(awal),
            });
        };
        self.periksa_sasaran(&targets)?;
        let values = self.parse_expr_list()?;
        // `a, b = e`: satu nilai untuk banyak sasaran membongkar tuple (cuma untuk `=` polos).
        let bongkar = targets.len() > 1 && values.len() == 1 && matches!(op, AssignOp::Eq);
        if !bongkar { self.periksa_jumlah_sasaran(targets.len(), values.len(), awal, "=")?; }
        Ok(Stmt::Assign(AssignStmt { targets, op, values, span: self.merge_span(awal) }))
    }

    /// Sisi kiri `=` harus sebuah TEMPAT: nama, atau `x[...]`/`x.nama` yang berakar pada nama.
    /// Dulu `5 = 3` gagal saat jalan di acuan dan jadi `compile error: invalid assignment
    /// target` — Inggris, tanpa lokasi — di `--vm`; bentuknya sudah salah sebelum jalan.
    pub(crate) fn periksa_sasaran(&self, targets: &[Expr]) -> Result<(), ParseError> {
        for t in targets {
            let mut kini = t;
            loop {
                match kini {
                    Expr::Ident(..) => break,
                    Expr::Index { obj, .. } | Expr::Field { obj, .. } => kini = obj,
                    lain => return Err(ParseError {
                        msg: crate::interpreter::pesan_bukan_sasaran(lain),
                        span: lain.span(),
                    }),
                }
            }
        }
        Ok(())
    }

    pub(crate) fn periksa_jumlah_sasaran(
        &self, kiri: usize, kanan: usize, awal: Span, op: &str,
    ) -> Result<(), ParseError> {
        if kiri == kanan { return Ok(()); }
        // Satu nilai untuk banyak sasaran hampir selalu berarti pemanggilan yang
        // mengembalikan tuple, dan untuk itu JRG sudah punya bentuk yang bekerja — …
        let bantuan = if kanan == 1 {
            "\nbantuan: kalau sisi kanannya mengembalikan tuple, `a, b = ...` (dengan `=` polos) \
             atau `let (a, b) = ...` membongkarnya"
        } else {
            ""
        };
        Err(ParseError {
            msg: format!(
                "`{}` dengan {} sasaran tapi {} nilai — jumlahnya harus sama{}",
                op, kiri, kanan, bantuan),
            span: self.merge_span(awal),
        })
    }

    pub(crate) fn try_parse_assign_op(&mut self) -> Option<AssignOp> {
        let op = match &self.current().kind {
            TokenKind::Eq       => AssignOp::Eq,
            TokenKind::PlusEq   => AssignOp::PlusEq,
            TokenKind::MinusEq  => AssignOp::MinusEq,
            TokenKind::StarEq   => AssignOp::StarEq,
            TokenKind::SlashEq  => AssignOp::SlashEq,
            TokenKind::PercentEq => AssignOp::PercentEq,
            TokenKind::AndEq    => AssignOp::AndEq,
            TokenKind::OrEq     => AssignOp::OrEq,
            TokenKind::CaretEq  => AssignOp::CaretEq,
            TokenKind::ShlEq    => AssignOp::ShlEq,
            TokenKind::ShrEq    => AssignOp::ShrEq,
            TokenKind::StarStarEq => AssignOp::PowEq,
            _ => return None,
        };
        self.advance();
        Some(op)
    }

    pub(crate) fn parse_let_stmt(&mut self) -> Result<LetStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwLet)?;
        let is_mut = self.eat(&TokenKind::KwMut);
        // Check for @ownership annotation
        let ownership = if self.eat(&TokenKind::At) {
            Some(match self.current().kind.clone() {
                TokenKind::KwOwned  => { self.advance(); OwnershipQual::Owned }
                TokenKind::KwShared => { self.advance(); OwnershipQual::Shared }
                TokenKind::KwGc     => { self.advance(); OwnershipQual::Gc }
                TokenKind::KwRaw    => { self.advance(); OwnershipQual::Raw }
                other => return Err(self.error(format!("harusnya ada penanda kepemilikan sesudah `@`, yang ketemu malah {}", other.tulisan()))),
            })
        } else { None };
        let pattern = self.parse_pattern()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        self.expect(&TokenKind::Eq)?;
        let init = Box::new(self.parse_expr_consuming()?);
        let lifetime = if self.at_ident_starting_with_tick() {
            Some(self.advance_as_string())
        } else { None };
        Ok(LetStmt { is_mut, ownership, pattern, ty, init, lifetime, span: self.merge_span(lo) })
    }

    /// `let POLA = EKSPR else { … }` — DITURUNKAN jadi `match`, di sini.
    pub(crate) fn parse_let_atau_else(&mut self) -> Result<Stmt, ParseError> {
        let lo = self.span_lo();
        let l = self.parse_let_stmt()?;
        if !self.at_exact(&TokenKind::KwElse) {
            return Ok(Stmt::Let(l));
        }
        self.advance(); // else
        let kalau_gagal = self.parse_block()?;
        let span = self.merge_span(lo);

        if !blok_menyimpang(&kalau_gagal) {
            return Err(ParseError {
                msg: "blok `else` pada `let … else` harus MENYIMPANG — `return`, \
                      `break`, atau `continue`\n\
                      bantuan: kalau memang mau memberi nilai cadangan, pakai \
                      `match` biasa atau operator `??`\n\
                      catatan: kalau blok ini boleh jatuh terus, nama yang diikat polanya \
                      akan terpakai walau polanya tidak cocok — dan seluruh gunanya hilang"
                     .to_string(),
                span: kalau_gagal.span,
            });
        }

        let mut nama = Vec::new();
        kumpul_nama_pola(&l.pattern, &mut nama);

        // Tanpa nama terikat, tidak ada yang perlu keluar dari `match` —
        // jadi ia cukup jadi statement.
        if nama.is_empty() {
            return Ok(Stmt::Match(MatchStmt {
                scrutinee: l.init,
                arms: vec![
                    MatchArm { patterns: vec![l.pattern], guard: None,
                               body: MatchBody::Block(Block { stmts: Vec::new(), tail: None, span }),
                               span },
                    MatchArm { patterns: vec![Pattern::Wildcard(span)], guard: None,
                               body: MatchBody::Block(kalau_gagal), span },
                ],
                span,
            }));
        }

        // Satu nama keluar apa adanya; beberapa nama dibungkus tuple, karena
        // `let (a, b) = …` memang sudah bisa membongkarnya.
        let (pola_kiri, nilai_arm) = if nama.len() == 1 {
            (Pattern::Ident { name: nama[0].clone(), is_mut: false, span },
             Expr::Ident(nama[0].clone(), span))
        } else {
            (Pattern::Tuple {
                elems: nama.iter().map(|n| Pattern::Ident {
                    name: n.clone(), is_mut: false, span }).collect(),
                span,
             },
             Expr::Tuple {
                elems: nama.iter().map(|n| Expr::Ident(n.clone(), span)).collect(),
                span,
             })
        };

        let cocok = Expr::Match {
            scrutinee: l.init,
            arms: vec![
                MatchArm { patterns: vec![l.pattern], guard: None,
                           body: MatchBody::Expr(Box::new(nilai_arm)), span },
                MatchArm { patterns: vec![Pattern::Wildcard(span)], guard: None,
                           body: MatchBody::Block(kalau_gagal), span },
            ],
            span,
        };
        Ok(Stmt::Let(LetStmt {
            is_mut: l.is_mut,
            ownership: l.ownership,
            pattern: pola_kiri,
            ty: None,
            init: Box::new(cocok),
            lifetime: l.lifetime,
            span,
        }))
    }

    pub(crate) fn parse_var_stmt(&mut self) -> Result<VarStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwVar)?;
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        let init = if self.eat(&TokenKind::Eq) { Some(Box::new(self.parse_expr_consuming()?)) } else { None };
        Ok(VarStmt { name, ty, init, span: self.merge_span(lo) })
    }

    /// `if let POLA = EKSPR { ...
    pub(crate) fn parse_if_let(&mut self) -> Result<Expr, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwIf)?;
        self.expect(&TokenKind::KwLet)?;
        let pola = self.parse_pattern()?;
        self.expect(&TokenKind::Eq)?;
        // `parse_header_expr`, bukan `parse_expr`: di posisi header sebuah
        // `{` SELALU membuka badan, tidak pernah literal struct.
        let scrutinee = Box::new(self.parse_header_expr()?);
        let then = self.parse_block()?;
        let span_then = then.span;

        let mut arms = vec![MatchArm {
            patterns: vec![pola],
            guard: None,
            body: MatchBody::Block(then),
            span: span_then,
        }];

        // Lengan penampung. Tanpa `else` ia blok kosong — sama artinya
        // dengan `if` biasa tanpa `else`.
        let sisa: MatchBody = if self.eat(&TokenKind::KwElse) {
            if self.at_exact(&TokenKind::KwIf) {
                let lanjut = if matches!(self.peek_kind(1), Some(TokenKind::KwLet)) {
                    self.parse_if_let()?
                } else {
                    Expr::If(Box::new(self.parse_if_stmt()?))
                };
                MatchBody::Expr(Box::new(lanjut))
            } else {
                MatchBody::Block(self.parse_block()?)
            }
        } else {
            MatchBody::Block(Block { stmts: Vec::new(), tail: None, span: self.merge_span(lo) })
        };

        let span = self.merge_span(lo);
        arms.push(MatchArm {
            patterns: vec![Pattern::Wildcard(span)],
            guard: None,
            body: sisa,
            span,
        });
        Ok(Expr::Match { scrutinee, arms, span })
    }

    /// `while let POLA = EKSPR { ...
    pub(crate) fn parse_while_let(&mut self, label: Option<String>) -> Result<Stmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwWhile)?;
        self.expect(&TokenKind::KwLet)?;
        let pola = self.parse_pattern()?;
        self.expect(&TokenKind::Eq)?;
        let scrutinee = Box::new(self.parse_header_expr()?);
        let body = self.parse_block()?;
        let span_body = body.span;
        let span = self.merge_span(lo);

        let keluar = Block {
            stmts: vec![Stmt::Break(BreakStmt { label: label.clone(), span })],
            tail: None,
            span,
        };
        let cocok = Stmt::Match(MatchStmt {
            scrutinee,
            arms: vec![
                MatchArm { patterns: vec![pola], guard: None,
                           body: MatchBody::Block(body), span: span_body },
                MatchArm { patterns: vec![Pattern::Wildcard(span)], guard: None,
                           body: MatchBody::Block(keluar), span },
            ],
            span,
        });
        Ok(Stmt::Loop(LoopStmt {
            body: Block { stmts: vec![cocok], tail: None, span },
            label,
            span,
        }))
    }

    pub(crate) fn parse_if_stmt(&mut self) -> Result<IfStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwIf)?;
        let (init, cond) = self.parse_if_init_and_cond()?;
        let then = self.parse_block()?;
        let mut else_ifs = Vec::new();
        let mut else_ = None;
        while self.eat(&TokenKind::KwElse) {
            if self.at_exact(&TokenKind::KwIf) {
                self.advance();
                let (ei_init, ei_cond) = self.parse_if_init_and_cond()?;
                let ei_block = self.parse_block()?;
                else_ifs.push((ei_init.map(Box::new), Box::new(ei_cond), ei_block));
            } else {
                else_ = Some(self.parse_block()?);
                break;
            }
        }
        Ok(IfStmt { init: init.map(Box::new), cond: Box::new(cond), then, else_ifs, else_,
                    span: self.merge_span(lo) })
    }

    pub(crate) fn parse_if_init_and_cond(&mut self) -> Result<(Option<Stmt>, Expr), ParseError> {
        // Disambiguate: `if let x = e; cond` from `if cond`
        // We parse an expression; if followed by `;`, it's the init stmt.
        let expr = self.parse_header_expr()?;
        if self.eat(&TokenKind::Semi) {
            let init = Stmt::Expr(Box::new(expr));
            let cond = self.parse_header_expr()?;
            Ok((Some(init), cond))
        } else {
            Ok((None, expr))
        }
    }

    pub(crate) fn parse_for_stmt(&mut self) -> Result<ForStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwFor)?;
        // Range form: `for x in expr` or `for x, y in expr`
        if self.is_for_range() {
            let mut vars = vec![self.expect_ident()?];
            // Pola cuma sah di posisi NILAI, dan itu bukan pilihan gaya:
            // posisi PERTAMA mengikat indeks (`for i in arr` memberi indeks),
            // dan sebuah indeks tidak punya bagian untuk dibongkar.
            let mut pola: Option<Pattern> = None;
            while self.eat(&TokenKind::Comma) {
                if matches!(self.current().kind, TokenKind::LParen | TokenKind::LBracket) {
                    pola = Some(self.parse_pattern()?);
                    vars.push("__nilai".to_string());
                } else {
                    vars.push(self.expect_ident()?);
                }
            }
            let is_decl = self.eat(&TokenKind::ColonEq);
            if !is_decl { self.expect(&TokenKind::KwIn)?; }
            let iter = Box::new(self.parse_header_expr()?);
            let mut body = self.parse_block()?;
            if let Some(pat) = pola {
                let span = self.merge_span(lo);
                let mut stmts = vec![Stmt::Let(LetStmt {
                    is_mut: false, ownership: None, pattern: pat, ty: None,
                    init: Box::new(Expr::Ident("__nilai".to_string(), span)),
                    lifetime: None, span,
                })];
                stmts.append(&mut body.stmts);
                body.stmts = stmts;
            }
            return Ok(ForStmt::Range { vars, is_decl, iter, body, label: None, span: self.merge_span(lo) });
        }
        // `for (a, b) in ...`: posisi pertama mengikat INDEKS, jadi sebuah pola di
        // sana tidak punya apa pun untuk dibongkar.
        if matches!(self.current().kind, TokenKind::LParen | TokenKind::LBracket)
            && self.ada_in_sebelum_badan(self.pos)
        {
            return Err(self.error(concat!(
                "`for (a, b) in ...` tidak bisa: posisi pertama sebuah `for` mengikat ",
                "INDEKS, bukan nilainya. Tulis `for _, (a, b) in ...` — bentuk dua-nama ",
                "itu yang memberi nilainya").to_string()));
        }
        // Classic C-style: `for init; cond; post { }`
        let init = if !self.at_exact(&TokenKind::Semi) {
            Some(Box::new(self.parse_stmt()?))
        } else { None };
        self.eat(&TokenKind::Semi);
        let cond = if !self.at_exact(&TokenKind::Semi) {
            Some(Box::new(self.parse_header_expr()?))
        } else { None };
        self.eat(&TokenKind::Semi);
        let post = if !self.at_exact(&TokenKind::LBrace) {
            Some(Box::new(self.parse_stmt()?))
        } else { None };
        let body = self.parse_block()?;
        Ok(ForStmt::Classic { init, cond, post, body, label: None, span: self.merge_span(lo) })
    }

    pub(crate) fn is_for_range(&self) -> bool {
        // Look for `ident in` or `ident, ident in` pattern
        if !matches!(&self.current().kind, TokenKind::Ident(_)) { return false; }
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::KwIn => return true,
                // `:=` TIDAK cukup untuk memutuskan.
                TokenKind::ColonEq => return !self.ada_semi_sebelum_badan(i + 1),
                TokenKind::Comma => { i += 1; }
                TokenKind::Ident(_) => { i += 1; }
                // Pola di posisi NILAI (`for _, (a, b) in ...`): kurungnya
                // dilewati utuh, jadi `in` di belakangnya tetap ketemu.
                TokenKind::LParen | TokenKind::LBracket => {
                    let buka = self.tokens[i].kind.clone();
                    let tutup = if matches!(buka, TokenKind::LParen) {
                        TokenKind::RParen } else { TokenKind::RBracket };
                    let mut dalam = 0i32;
                    while i < self.tokens.len() {
                        if self.tokens[i].kind == buka { dalam += 1; }
                        else if self.tokens[i].kind == tutup {
                            dalam -= 1;
                            if dalam == 0 { i += 1; break; }
                        } else if matches!(self.tokens[i].kind, TokenKind::Eof) { return false; }
                        i += 1;
                    }
                }
                _ => return false,
            }
        }
        false
    }

    /// `a[::2]` dan `a[1:2:3]` — slice berlangkah, yang belum ada.
    pub(crate) fn tolak_slice_berlangkah(&mut self) -> Option<ParseError> {
        // `::` adalah SATU token di lexer ini, jadi `a[::2]` terbaca
        // `ColonColon` — bukan dua `Colon`. Versi pertama penjaga ini
        // memeriksa dua `Colon` dan karena itu tidak pernah kena.
        if !self.at_exact(&TokenKind::ColonColon) { return None; }
        self.pesan_slice_berlangkah()
    }

    pub(crate) fn pesan_slice_berlangkah(&self) -> Option<ParseError> {
        Some(self.error(concat!(
            "slice berlangkah (`a[::2]`) belum ada di JRG. Bentuk yang setara: ",
            "`for i, x in a { if i % 2 == 0 { ... } }`, atau `arr::filter` atas ",
            "hasil `arr::enumerate`").to_string()))
    }

    /// Adakah `in` di kedalaman nol sebelum `{` yang membuka badan loop?
    pub(crate) fn ada_in_sebelum_badan(&self, mut i: usize) -> bool {
        let mut dalam = 0i32;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen | TokenKind::LBracket => dalam += 1,
                TokenKind::RParen | TokenKind::RBracket => dalam -= 1,
                TokenKind::KwIn if dalam == 0 => return true,
                TokenKind::LBrace if dalam == 0 => return false,
                TokenKind::Semi if dalam == 0 => return false,
                TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Adakah `;` di kedalaman nol sebelum `{` yang membuka badan loop?
    pub(crate) fn ada_semi_sebelum_badan(&self, mut i: usize) -> bool {
        let mut dalam = 0i32;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen | TokenKind::LBracket => dalam += 1,
                TokenKind::RParen | TokenKind::RBracket => dalam -= 1,
                TokenKind::Semi if dalam == 0 => return true,
                TokenKind::LBrace if dalam == 0 => return false,
                TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    pub(crate) fn parse_while_stmt(&mut self) -> Result<WhileStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwWhile)?;
        let cond = Box::new(self.parse_header_expr()?);
        let body = self.parse_block()?;
        Ok(WhileStmt { cond, body, label: None, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_loop_stmt(&mut self) -> Result<LoopStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwLoop)?;
        let body = self.parse_block()?;
        Ok(LoopStmt { body, label: None, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_match_stmt(&mut self) -> Result<MatchStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwMatch)?;
        let scrutinee = Box::new(self.parse_header_expr()?);
        let arms = self.parse_match_arms_block()?;
        Ok(MatchStmt { scrutinee, arms, span: self.merge_span(lo) })
    }

    /// Parses the brace-delimited arm list shared by both the legacy
    /// statement-position `match` (`MatchStmt`, kept intact for AST/API …
    pub(crate) fn parse_match_arms_block(&mut self) -> Result<Vec<MatchArm>, ParseError> {
        self.expect(&TokenKind::LBrace)?;
        let mut arms = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) {
            arms.push(self.parse_match_arm()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(arms)
    }

    pub(crate) fn parse_match_arm(&mut self) -> Result<MatchArm, ParseError> {
        let lo = self.span_lo();
        let mut patterns = vec![self.parse_pattern()?];
        while self.eat(&TokenKind::Or) {
            patterns.push(self.parse_pattern()?);
        }
        let guard = if self.eat(&TokenKind::KwIf) {
            Some(Box::new(self.parse_expr()?))
        } else { None };
        self.expect(&TokenKind::FatArrow)?;
        let body = if self.at_exact(&TokenKind::LBrace) {
            MatchBody::Block(self.parse_block()?)
        } else if matches!(self.current().kind, TokenKind::KwReturn | TokenKind::KwBreak | TokenKind::KwContinue) {
            // `pola => return x` / `=> break` / `=> continue` tanpa kurung kurawal.
            let lo_stmt = self.span_lo();
            let stmt = match self.current().kind {
                // BUKAN `self.parse_return_stmt()` biasa: itu mem-parse daftar nilai balik
                // dipisah koma (`return a, b`), yang di posisi ini akan melahap koma PEMISAH …
                TokenKind::KwReturn   => Stmt::Return(self.parse_bare_return_in_match_arm()?),
                TokenKind::KwBreak    => Stmt::Break(self.parse_break_stmt()?),
                TokenKind::KwContinue => Stmt::Continue(self.parse_continue_stmt()?),
                _ => unreachable!("disaring oleh guard match di atas"),
            };
            MatchBody::Block(Block { stmts: vec![stmt], tail: None, span: self.merge_span(lo_stmt) })
        } else {
            MatchBody::Expr(Box::new(self.parse_expr()?))
        };
        // Arms may be separated by a comma, and/or by one or more semicolons —
        // explicit, or automatically inserted by the lexer right after an …
        self.eat(&TokenKind::Comma);
        self.skip_semis();
        Ok(MatchArm { patterns, guard, body, span: self.merge_span(lo) })
    }

    /// `return` di posisi match-arm tanpa kurung kurawal (`pola => return x,`).
    pub(crate) fn parse_bare_return_in_match_arm(&mut self) -> Result<ReturnStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwReturn)?;
        let values = if matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::Semi | TokenKind::RBrace
        ) {
            Vec::new()
        } else {
            vec![self.parse_expr()?]
        };
        Ok(ReturnStmt { values, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_return_stmt(&mut self) -> Result<ReturnStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwReturn)?;
        let values = if self.at_exact(&TokenKind::Semi) || self.at_exact(&TokenKind::RBrace) {
            Vec::new()
        } else {
            self.parse_expr_list()?
        };
        Ok(ReturnStmt { values, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_continue_stmt(&mut self) -> Result<ContinueStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwContinue)?;
        let label = if self.at_ident_starting_with_tick() { Some(self.advance_as_string()) } else { None };
        Ok(ContinueStmt { label, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_break_stmt(&mut self) -> Result<BreakStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwBreak)?;
        let label = if self.at_ident_starting_with_tick() { Some(self.advance_as_string()) } else { None };
        Ok(BreakStmt { label, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_spawn_stmt(&mut self) -> Result<SpawnStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwSpawn)?;
        // Optional |params|
        let params = if self.at_exact(&TokenKind::Or) {
            self.advance();
            let mut ps = Vec::new();
            while !self.at_exact(&TokenKind::Or) {
                ps.push(self.parse_fn_param()?);
                if !self.eat(&TokenKind::Comma) { break; }
            }
            self.expect(&TokenKind::Or)?;
            Some(ps)
        } else { None };
        let is_async = self.eat(&TokenKind::KwAsync);
        let is_move  = self.eat(&TokenKind::KwMove);
        let body = self.parse_block()?;
        Ok(SpawnStmt { params, is_async, is_move, body, span: self.merge_span(lo) })
    }

    /// `go expr` / `go { block }` — the classic Go goroutine idiom (`go
    /// worker(id)`, `go func() { ...
    pub(crate) fn parse_go_stmt(&mut self) -> Result<SpawnStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwGo)?;
        if self.at_exact(&TokenKind::LBrace) {
            let body = self.parse_block()?;
            return Ok(SpawnStmt { params: None, is_async: false, is_move: false, body,
                                   span: self.merge_span(lo) });
        }
        let expr = self.parse_expr()?;
        let span = self.merge_span(lo);
        let body = Block { stmts: Vec::new(), tail: Some(Box::new(expr)), span };
        Ok(SpawnStmt { params: None, is_async: false, is_move: false, body, span })
    }

    pub(crate) fn parse_select_stmt(&mut self) -> Result<SelectStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwSelect)?;
        self.expect(&TokenKind::LBrace)?;
        let mut cases = Vec::new();
        let mut default = None;
        loop {
            // Each case/default body is itself a brace-delimited block, and `}` triggers
            // auto-semicolon insertion on the newline that follows it — so a semicolon …
            self.skip_semis();
            if self.at_exact(&TokenKind::RBrace) { break; }
            let clo = self.span_lo();
            if self.eat(&TokenKind::KwDefault) {
                self.expect(&TokenKind::Colon)?;
                default = Some(self.parse_block()?);
            } else {
                self.expect(&TokenKind::KwCase)?;
                // Determine case type: send, recv, or let
                if self.at_exact(&TokenKind::KwLet) {
                    let stmt = self.parse_let_stmt()?;
                    self.expect(&TokenKind::Colon)?;
                    let body = self.parse_block()?;
                    cases.push(SelectCase::Let { stmt, body, span: self.merge_span(clo) });
                } else if self.at_exact(&TokenKind::LArrow) {
                    // recv case: <-chan
                    self.advance();
                    let chan = Box::new(self.parse_expr()?);
                    self.expect(&TokenKind::Colon)?;
                    let body = self.parse_block()?;
                    cases.push(SelectCase::Recv { names: Vec::new(), is_decl: false, chan, body,
                                                   span: self.merge_span(clo) });
                } else {
                    // send case: chan <- val
                    let chan_or_name = self.parse_expr()?;
                    if self.at_exact(&TokenKind::LArrow) {
                        self.advance();
                        let val = Box::new(self.parse_expr()?);
                        self.expect(&TokenKind::Colon)?;
                        let body = self.parse_block()?;
                        cases.push(SelectCase::Send { chan: Box::new(chan_or_name), val, body,
                                                       span: self.merge_span(clo) });
                    } else {
                        // recv with assignment: `name := <-chan`
                        let is_decl = self.eat(&TokenKind::ColonEq);
                        if !is_decl { self.expect(&TokenKind::Eq)?; }
                        self.expect(&TokenKind::LArrow)?;
                        let chan = Box::new(self.parse_expr()?);
                        self.expect(&TokenKind::Colon)?;
                        let body = self.parse_block()?;
                        let name = match chan_or_name {
                            Expr::Ident(n, _) => n,
                            _ => return Err(self.error("expected identifier in select recv case".to_string())),
                        };
                        cases.push(SelectCase::Recv { names: vec![name], is_decl, chan, body,
                                                       span: self.merge_span(clo) });
                    }
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(SelectStmt { cases, default, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_defer_stmt(&mut self) -> Result<DeferStmt, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwDefer)?;
        let expr = Box::new(self.parse_expr()?);
        Ok(DeferStmt { expr, span: self.merge_span(lo) })
    }
}
