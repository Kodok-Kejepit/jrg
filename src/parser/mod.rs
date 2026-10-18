// src/parser/mod.rs — Compiler — src/parser/mod.rs
// Complete Recursive-Descent AST Parser

#![allow(dead_code, unused_variables, unused_imports, irrefutable_let_patterns)]

pub mod mutabilitas;

use crate::lexer::{Token, TokenKind, Span};

mod ast;
mod item;
mod tipe;
mod stmt;
mod ekspresi;
mod pola;
pub use ast::*;
pub use ekspresi::{lengan_dari_koalesensi, asal_let};
pub use tipe::nilai_nol;

// ─── Parser ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg:  String,
    pub span: Span,
}

use crate::lexer::is_keyword_token;

pub struct Parser {
    tokens:  Vec<Token>,
    pos:     usize,
    errors:  Vec<ParseError>,
    /// Penanda `...` per argumen posisional, diisi `parse_arg_penuh` dan
    /// dikuras situs pemanggilnya.
    arg_sebar: Vec<bool>,
    /// Item yang DITURUNKAN dari sebuah deklarasi, bukan ditulis pemakainya
    /// — sejauh ini konstruktor struct tuple. Dikuras oleh loop item begitu
    /// deklarasi asalnya selesai, jadi urutannya tetap seperti yang dibaca.
    tambahan_item: Vec<Item>,
    recover: bool,         // if true, accumulate errors instead of panicking
    /// Sedang mengurai HEADER `if`/`while`/`for`/`match`, tempat `{` selalu
    /// berarti badan — bukan awal literal struct.
    larang_struct_literal: bool,
    /// Token berikutnya operand LANGSUNG sebuah `-` prefiks: cuma di sana literal
    /// 2^127 sah, karena `-170141183460469231731687303715884105728` adalah `i128::MIN`.
    minus_baru: bool,
    /// Literal 2^127 yang diizinkan di bawah `-`, diperiksa lagi oleh `-`-nya.
    literal_min: Option<Span>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, recover: bool) -> Self {
        Parser { tokens, pos: 0, errors: Vec::new(),
            tambahan_item: Vec::new(), arg_sebar: Vec::new(), recover,
                 larang_struct_literal: false, minus_baru: false, literal_min: None }
    }

    /// Urai ekspresi sebagai HEADER kontrol: `{` yang menyusulnya membuka
    /// badan, bukan literal struct.
    pub(crate) fn parse_header_expr(&mut self) -> Result<Expr, ParseError> {
        let dulu = self.larang_struct_literal;
        self.larang_struct_literal = true;
        let hasil = self.parse_expr();
        self.larang_struct_literal = dulu;
        hasil
    }

    /// Jalankan `f` dengan larangan literal struct DILEPAS — dipakai di
    /// dalam kurung, di mana `{` tidak lagi bisa disalahartikan sebagai
    /// badan blok milik header.
    pub(crate) fn dgn_struct_literal_boleh<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let dulu = self.larang_struct_literal;
        self.larang_struct_literal = false;
        let hasil = f(self);
        self.larang_struct_literal = dulu;
        hasil
    }

    // ── Primitive helpers ─────────────────────────────────────────────────

    pub(crate) fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    pub(crate) fn peek(&self) -> &Token {
        let i = (self.pos + 1).min(self.tokens.len() - 1);
        &self.tokens[i]
    }

    pub(crate) fn peek2(&self) -> &Token {
        let i = (self.pos + 2).min(self.tokens.len() - 1);
        &self.tokens[i]
    }

    pub(crate) fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 { self.pos += 1; }
        tok
    }

    pub(crate) fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind)
    }

    pub(crate) fn at_exact(&self, kind: &TokenKind) -> bool {
        &self.current().kind == kind
    }

    pub(crate) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) { self.advance(); true }
        else { false }
    }

    pub(crate) fn expect(&mut self, kind: &TokenKind) -> Result<&Token, ParseError> {
        if self.at(kind) {
            return Ok(self.advance());
        }
        // Satu situs, banyak kasus: hampir setiap kata operator dari bahasa
        // lain (`and`, `or`, `not`) mendarat di sini sebagai "harusnya ada `)`",
        // yang benar tentang tokennya dan diam soal apa yang sedang ditulis.
        if let Some(pesan) = self.kata_asing_di_sini() {
            return Err(self.error(pesan));
        }
        // `if a = 1 { ... }` — satu sama dengan di kepala syarat. Pesannya
        // dulu "harusnya ada `{`", yang benar tentang tokennya dan menyuruh
        // pembacanya mencari kurawal yang memang belum waktunya.
        if matches!(kind, TokenKind::LBrace) && matches!(self.current().kind, TokenKind::Eq) {
            return Err(self.error(
                "`=` itu MENUGASKAN nilai, bukan membandingkan — di kepala `if`/`while` \
                 maksudnya hampir pasti `==`. (JRG memang tidak mengizinkan penugasan \
                 di dalam syarat, justru supaya salah ketik ini tidak pernah diam.)"
                    .to_string()));
        }
        Err(self.error(format!(
            "harusnya ada {} di sini, yang ketemu malah {}",
            kind.tulisan(), self.current().kind.tulisan()
        )))
    }

    /// Kata dari bahasa lain yang muncul di posisi operator atau penutup.
    pub(crate) fn kata_asing_di_sini(&self) -> Option<String> {
        // `not true` sudah terlanjur membaca `not` sebagai NAMA VARIABEL, jadi yang
        // terlihat di sini `true`.
        if let Some(i) = self.pos.checked_sub(1) {
            // `f"halo ${n}"` — awalan f-string Python. Ia terbaca sebagai nama
            // `f` diikuti sebuah string, jadi yang terlihat di sini string-nya.
            if matches!(self.current().kind, TokenKind::StringLit(_)) {
                if let Some(tok) = self.tokens.get(i) {
                    if matches!(&tok.kind, TokenKind::Ident(n) if n == "f" || n == "rb" || n == "b")
                        && tok.span.hi == self.current().span.lo
                    {
                        return Some("awalan string seperti `f\"...\"` bukan bentuk JRG. \
                            Untuk menyisipkan nilai ke dalam teks pakai template: \
                            `\"\"\"halo ${nama}\"\"\"` — tanda `${...}` diisi saat jalan"
                            .to_string());
                    }
                }
            }
            if let Some(TokenKind::Ident(sebelum)) = self.tokens.get(i).map(|t| &t.kind) {
                if sebelum == "not" {
                    return Some("`not` bukan operator JRG — pakai `!` di depan nilainya, \
                                 mis. `!selesai`".to_string());
                }
            }
        }
        let TokenKind::Ident(n) = &self.current().kind else { return None };
        Some(match n.as_str() {
            "and" => "`and` bukan operator JRG — pakai `&&`".to_string(),
            "or" => "`or` bukan operator JRG — pakai `||`".to_string(),
            "not" => "`not` bukan operator JRG — pakai `!` di depan nilainya, mis. `!selesai`"
                .to_string(),
            "is" => "`is` bukan operator JRG — pakai `==` untuk membandingkan nilai".to_string(),
            "in" if !matches!(self.peek_kind(1), Some(TokenKind::Ident(_))) =>
                "`in` di JRG cuma dipakai di dalam `for x in ...`".to_string(),
            "elif" => "di JRG ditulis `else if`, dua kata".to_string(),
            "then" => "`then` bukan bentuk JRG — badan `if` langsung dibuka `{`".to_string(),
            "endif" | "endfor" | "endwhile" | "end" =>
                "di JRG sebuah blok ditutup `}`, bukan kata `end`".to_string(),
            _ => return None,
        })
    }

    pub(crate) fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.current().kind.clone() {
            TokenKind::Ident(s) => { self.advance(); Ok(s) }
            other => Err(self.error(format!("harusnya ada nama di sini, yang ketemu malah {}", other.tulisan()))),
        }
    }
    /// Like expect_ident but also accepts keywords-used-as-identifiers (after ::).
    /// Needed for paths like arr::map, arr::filter, str::split, etc.
    pub(crate) fn expect_ident_or_kw(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            // Index tuple: `t.0`, `t.1`.
            TokenKind::IntLit(n) => {
                let name = n.to_string();
                self.advance();
                Ok(name)
            }
            // Terima keyword apa pun sebagai representasi string-nya, supaya
            // method yang namanya bertabrakan dengan keyword (`.map(..)`,
            // `.filter(..)`) tetap ke-parse sebagai nama method.
            other if is_keyword_token(other) => {
                // Ejaan token diambil dari `tulisan()`, bukan dari `format!("{:?}")` lalu
                // membuang awalan "kw".
                let name = other.tulisan().trim_matches('`').to_string();
                self.advance();
                Ok(name)
            }
            // Token lain (operator, literal non-integer, delimiter) bukan nama field
            // yang sah.
            other => {
                let found = other.tulisan();
                Err(self.error(format!(
                    "setelah `.` harusnya ada nama field/method (atau angka untuk index tuple), \
                     yang ketemu malah {}",
                    found
                )))
            }
        }
    }



    /// Jenis token pada offset `n` dari posisi sekarang (`0` = token
    /// sekarang). Dipakai untuk keputusan yang butuh lookahead, mis.
    /// membedakan `fn nama(..)` (deklarasi) dari `fn(..)` (closure).
    pub(crate) fn peek_kind(&self, n: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + n).map(|t| &t.kind)
    }

    pub(crate) fn error(&self, msg: String) -> ParseError {
        ParseError { msg, span: self.current().span }
    }

    pub(crate) fn span_lo(&self) -> Span { self.current().span }

    pub(crate) fn merge_span(&self, lo: Span) -> Span {
        let prev_span = if self.pos > 0 { self.tokens[self.pos - 1].span } else { lo };
        Span::new(lo.lo, prev_span.hi, lo.file_id)
    }

    pub(crate) fn skip_semis(&mut self) {
        while self.at_exact(&TokenKind::Semi) { self.advance(); }
    }

    /// Like `self.expect(&TokenKind::Semi)`, but also accepts ending right at a
    /// closing delimiter (`}`, `)`, `]`) with *no* semicolon at all, consuming …
    pub(crate) fn expect_semi_or_closer(&mut self) -> Result<(), ParseError> {
        if self.eat(&TokenKind::Semi) {
            return Ok(());
        }
        if matches!(self.current().kind, TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket) {
            return Ok(());
        }
        self.expect(&TokenKind::Semi).map(|_| ())
    }

    // Synchronize to next declaration/statement boundary after an error.
    pub(crate) fn synchronize(&mut self) {
        // Dulu: kalau token SAAT INI (tempat error terjadi) sudah salah satu
        // stop-keyword (mis. `pub`), loop `break` TANPA advance sama sekali.
        self.advance();
        loop {
            match &self.current().kind {
                TokenKind::Eof
                | TokenKind::KwFn
                | TokenKind::KwStruct
                | TokenKind::KwEnum
                | TokenKind::KwTrait
                | TokenKind::KwImpl
                | TokenKind::KwLet
                | TokenKind::KwVar
                | TokenKind::KwConst
                | TokenKind::KwPub
                | TokenKind::KwMod
                | TokenKind::KwExtern => break,
                // `}` TIDAK dikonsumsi (beda dari yang lain, yang break SETELAH landing di
                // situ): kalau recovery dipanggil dari dalam blok (`parse_block`), `}` ini …
                TokenKind::RBrace => break,
                TokenKind::Semi => { self.advance(); break; }
                _ => { self.advance(); }
            }
        }
    }

    /// Kurung kurawal yang dibuka sejak token ke-`mulai` dan belum ditutup di posisi kini.
    pub(crate) fn kurawal_terbuka(&self, mulai: usize) -> usize {
        let mut dalam = 0usize;
        for t in &self.tokens[mulai.min(self.pos)..self.pos] {
            match t.kind {
                TokenKind::LBrace => dalam += 1,
                TokenKind::RBrace => dalam = dalam.saturating_sub(1),
                _ => {}
            }
        }
        dalam
    }

    /// Lompati sampai `dalam` kurung kurawal yang masih terbuka tertutup semua.
    pub(crate) fn tutup_kurawal(&mut self, mut dalam: usize) {
        while dalam > 0 && !self.at_exact(&TokenKind::Eof) {
            match self.current().kind {
                TokenKind::LBrace => dalam += 1,
                TokenKind::RBrace => dalam -= 1,
                _ => {}
            }
            self.advance();
        }
    }

    /// Pemulihan di tingkat atas berkas: deklarasi yang rusak dilompati UTUH (badannya
    /// ikut), sampai kata kunci deklarasi berikutnya — dulu `}` penutupnya jadi galat kedua.
    pub(crate) fn synchronize_atas(&mut self, mulai: usize) {
        let dalam = self.kurawal_terbuka(mulai);
        self.tutup_kurawal(dalam);
        if self.pos == mulai { self.advance(); }
        loop {
            match &self.current().kind {
                TokenKind::Eof
                | TokenKind::KwFn | TokenKind::KwStruct | TokenKind::KwEnum | TokenKind::KwTrait
                | TokenKind::KwImpl | TokenKind::KwLet | TokenKind::KwVar | TokenKind::KwConst
                | TokenKind::KwPub | TokenKind::KwMod | TokenKind::KwExtern => break,
                TokenKind::LBrace => { self.advance(); self.tutup_kurawal(1); }
                _ => { self.advance(); }
            }
        }
    }

    // ── Source file ───────────────────────────────────────────────────────

    pub fn parse_file(&mut self) -> ParsedFile {
        let lo = self.span_lo();
        self.skip_semis();
        // ── Mode skrip ────────────────────────────────────────────────────
        if !self.at_exact(&TokenKind::KwPackage) && !self.mirip_package() {
            return self.parse_file_skrip(lo);
        }
        let package = self.parse_package_decl().unwrap_or_else(|e| {
            self.errors.push(e); "unknown".to_string()
        });
        self.skip_semis();

        let mut imports = Vec::new();
        while self.at_exact(&TokenKind::KwImport) {
            match self.parse_import() {
                Ok(i)  => imports.push(i),
                Err(e) => { self.errors.push(e); self.synchronize(); }
            }
            self.skip_semis();
        }

        let mut items = Vec::new();
        while !self.at_exact(&TokenKind::Eof) {
            let mulai = self.pos;
            match self.parse_item() {
                Ok(item)  => { items.push(item); items.append(&mut self.tambahan_item); }
                Err(e) => {
                    self.errors.push(e);
                    items.push(Item::Error(self.current().span));
                    self.synchronize_atas(mulai);
                }
            }
            self.skip_semis();
        }

        periksa_ekor_main(&mut items);
        self.sisipkan_warna(&imports, &mut items);
        let file = ParsedFile { package, imports, items, span: self.merge_span(lo) };
        self.periksa_impl_lengkap(&file);
        self.periksa_nama_bound(&file);
        self.periksa_nama_tipe(&file);
        self.periksa_mutabilitas(&file);
        file
    }

    /// `Warna` pustaka (hasil `canvas::get_pixel`) sebagai struct berbentuk tetap, bentuk yang sama
    /// dengan `std/mesin/warna`: dulu rekaman bebas (`Warna { x: 1 }` diterima) dan LLVM tidak
    /// mengenalnya. Berkas ber-`import` dilewati — `Warna` di sana bisa milik modulnya.
    fn sisipkan_warna(&self, imports: &[ImportDecl], items: &mut Vec<Item>) {
        if !imports.is_empty() { return }
        if !self.tokens.iter().any(|t| matches!(&t.kind, TokenKind::Ident(n) if n == "Warna")) { return }
        let sudah = items.iter().any(|it| matches!(it,
            Item::Struct(d) if d.name == "Warna") || matches!(it, Item::Enum(d) if d.name == "Warna")
            || matches!(it, Item::Trait(d) if d.name == "Warna"));
        if sudah { return }
        let span = Span::dummy();
        let field = |nama: &str, bawaan: Option<u128>| StructField {
            vis: Visibility::Public, name: nama.to_string(), ty: TypeExpr::Primitive(PrimTy::Int),
            default: bawaan.map(|n| Box::new(Expr::Literal(LitKind::Int(n), span))), span };
        items.push(Item::Struct(StructDecl {
            vis: Visibility::Public, name: "Warna".to_string(),
            generics: GenericParams { params: Vec::new(), span: None }, parent: None, where_clause: Vec::new(),
            fields: vec![field("r", None), field("g", None), field("b", None), field("a", Some(255))], span }));
    }

    /// Apakah token pertamanya SALAH KETIK dari `package`?
    pub(crate) fn mirip_package(&self) -> bool {
        match &self.current().kind {
            TokenKind::Ident(n) => {
                let d = crate::diag::jarak_sunting(n, "package");
                d > 0 && d <= 2
            }
            _ => false,
        }
    }

    /// Berkas tanpa `package`: pernyataan telanjang jadi badan `main`.
    pub(crate) fn parse_file_skrip(&mut self, lo: Span) -> ParsedFile {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        let mut stmts: Vec<Stmt> = Vec::new();
        let mut span_stmt: Option<Span> = None;

        while !self.at_exact(&TokenKind::Eof) {
            if self.at_exact(&TokenKind::KwImport) {
                match self.parse_import() {
                    Ok(i) => imports.push(i),
                    Err(e) => { self.errors.push(e); self.synchronize(); }
                }
                self.skip_semis();
                continue;
            }
            if self.mulai_deklarasi() {
                match self.parse_item() {
                    Ok(item) => { items.push(item); items.append(&mut self.tambahan_item); }
                    Err(e) => {
                        self.errors.push(e);
                        items.push(Item::Error(self.current().span));
                        self.synchronize();
                    }
                }
                self.skip_semis();
                continue;
            }
            let s_lo = self.span_lo();
            match self.parse_stmt() {
                Ok(st) => {
                    span_stmt = Some(match span_stmt {
                        None => self.merge_span(s_lo),
                        Some(sp) => Span { lo: sp.lo, hi: self.merge_span(s_lo).hi, file_id: sp.file_id },
                    });
                    stmts.push(st);
                }
                Err(e) => { self.errors.push(e); self.synchronize(); }
            }
            self.skip_semis();
        }

        if !stmts.is_empty() {
            // `main` yang ditulis sendiri DAN pernyataan telanjang: dua badan untuk satu
            // program.
            let ada_main = items.iter().any(|i| matches!(i, Item::Fn(d) if d.name == "main"));
            if ada_main {
                let sp = span_stmt.unwrap_or_else(|| self.current().span);
                self.errors.push(ParseError {
                    msg: "berkas ini punya `fn main()` DAN pernyataan di luarnya. \
                          Tanpa `package`, pernyataan telanjang jadi isi `main` \
                          yang dibuatkan — jadi ada dua. Pilih salah satu: \
                          pindahkan pernyataannya ke dalam `main`, atau hapus \
                          `fn main()`".to_string(),
                    span: sp,
                });
            } else {
                let sp = span_stmt.unwrap_or_else(|| self.current().span);
                items.push(Item::Fn(FnDecl {
                    vis: Visibility::Public,
                    is_async: false,
                    name: "main".to_string(),
                    generics: GenericParams { params: Vec::new(), span: None },
                    params: Vec::new(),
                    ret_type: None,
                    where_clause: Vec::new(),
                    body: FnBody::Block(Block { stmts, tail: None, span: sp }).into(),
                    span: sp,
                }));
            }
        }

        self.sisipkan_warna(&imports, &mut items);
        let file = ParsedFile {
            package: "main".to_string(),
            imports,
            items,
            span: self.merge_span(lo),
        };
        self.periksa_impl_lengkap(&file);
        self.periksa_nama_bound(&file);
        self.periksa_nama_tipe(&file);
        self.periksa_mutabilitas(&file);
        file
    }

    /// Apakah posisi sekarang mengawali sebuah DEKLARASI tingkat atas?
    pub(crate) fn mulai_deklarasi(&self) -> bool {
        let mut i = 0;
        if matches!(self.peek_kind(0), Some(TokenKind::KwPub)) { i = 1; }
        // `type` dan `mod` ikut di sini walau keduanya DITOLAK: penolakannya ditulis
        // di `parse_item` dan menyebutkan alasannya.
        matches!(self.peek_kind(i), Some(
            TokenKind::KwFn | TokenKind::KwAsync | TokenKind::KwStruct
            | TokenKind::KwEnum | TokenKind::KwTrait | TokenKind::KwImpl
            | TokenKind::KwExtern | TokenKind::KwType | TokenKind::KwMod
            | TokenKind::KwUse))
    }

    /// Kata kunci yang DIURAI LENGKAP lalu tidak pernah berpengaruh.

    /// `let` mengikat, `var` bisa diubah — dan sekarang itu ditegakkan.
    pub(crate) fn periksa_mutabilitas(&mut self, file: &ParsedFile) {
        let pm = Self::telusuri_lingkup(file);
        self.periksa_nama_ganda(file);
        // Satu penelusuran menghasilkan DUA jenis temuan: penugasan ke nama yang
        // mengikat, dan kata kunci yang tidak pernah berpengaruh.
        for (msg, span) in pm.galat.into_iter().chain(pm.belum_ada).chain(pm.arity) {
            self.errors.push(ParseError { msg, span });
        }
    }

    fn telusuri_lingkup(file: &ParsedFile) -> mutabilitas::Pemeriksa {
        let mut pm = mutabilitas::Pemeriksa::baru();
        // Pemeriksaan arity builtin ikut penelusuran INI, bukan yang kedua — persis
        // alasan yang sudah tertulis di bawah.
        pm.periksa_arity = file.imports.is_empty();
        for item in &file.items {
            match item {
                Item::Struct(d) => { pm.tipe_sendiri.insert(d.name.clone()); }
                Item::Enum(d)   => {
                    pm.tipe_sendiri.insert(d.name.clone());
                    for v in &d.variants {
                        if matches!(v.kind, crate::parser::EnumVariantKind::Unit) {
                            pm.varian_unit.insert(v.name.clone());
                        }
                    }
                }
                Item::Trait(d)  => { pm.tipe_sendiri.insert(d.name.clone()); }
                Item::Fn(f) => {
                    let nama = |p: &crate::parser::FnParam| match &p.pattern {
                        crate::parser::Pattern::Ident { name, .. } => name.clone(), _ => String::new() };
                    let min = f.params.iter().filter(|p| p.default.is_none() && !p.is_variadic).count();
                    let maks = (!f.params.iter().any(|p| p.is_variadic)).then_some(f.params.len());
                    pm.fungsi_atas.insert(f.name.clone(), (min, maks, f.params.iter().map(nama).collect()));
                }
                _ => {}
            }
        }
        let mut pemilik: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for item in &file.items {
            if let Item::Enum(d) = item {
                for v in &d.variants { pemilik.entry(v.name.clone()).or_default().push(d.name.clone()); }
            }
        }
        pm.varian_ganda = pemilik.into_iter().filter(|(_, e)| e.len() > 1).collect();
        pm.berkas(&file.items);
        pm
    }

    /// Sebuah nama tidak boleh dideklarasikan dua kali di tingkat atas berkas
    /// yang sama.
    pub(crate) fn periksa_nama_ganda(&mut self, file: &ParsedFile) {
        use std::collections::HashMap;
        // (nama -> (span pertama, kata kunci pertama)), per ruang nama.
        let mut nilai: HashMap<&str, (Span, &str)> = HashMap::new();
        let mut tipe:  HashMap<&str, (Span, &str)> = HashMap::new();
        let mut temuan: Vec<(String, Span)> = Vec::new();

        {
            fn catat<'n>(ruang: &mut HashMap<&'n str, (Span, &'static str)>,
                         nama: &'n str, kw: &'static str, span: Span,
                         temuan: &mut Vec<(String, Span)>) {
                match ruang.get(nama) {
                    Some((_, kw_pertama)) => {
                        let sama = kw == *kw_pertama;
                        temuan.push((format!(
                            "`{}` sudah dideklarasikan di berkas ini sebagai `{}`{}\n \
                             bantuan: deklarasi kedua akan menimpa yang pertama tanpa pesan \
                             apa pun, jadi yang pertama tidak akan pernah bisa dipanggil — \
                             ganti salah satu namanya",
                            nama, kw_pertama,
                            if sama { String::new() }
                            else { format!(", sekarang sebagai `{}`", kw) },
                        ), span));
                    }
                    None => { ruang.insert(nama, (span, kw)); }
                }
            }

            // Varian telanjang ikut ruang nama NILAI: `enum E { A }` lalu `fn A` membuat `A`
            // selalu berarti variannya di dua mesin, dan fungsinya tak pernah terjangkau.
            // Dua enum boleh berbagi nama varian (`X::A`, `Y::A`), jadi varian tidak dicatat.
            let mut varian: HashMap<&str, &str> = HashMap::new();
            for item in &file.items {
                if let Item::Enum(ed) = item {
                    for v in &ed.variants { varian.entry(v.name.as_str()).or_insert(ed.name.as_str()); }
                }
            }
            let tabrak_varian = |nama: &str, kw: &str, span: Span, temuan: &mut Vec<(String, Span)>| {
                if let Some(tipe) = varian.get(nama) {
                    temuan.push((format!(
                        "`{}` sudah jadi nama varian `{}::{}` di berkas ini, sekarang sebagai `{}`\n \
                         bantuan: nama `{}` telanjang selalu berarti variannya, jadi deklarasi ini \
                         tidak akan pernah terjangkau — ganti salah satu namanya",
                        nama, tipe, nama, kw, nama), span));
                }
            };
            for item in &file.items {
                match item {
                    Item::Fn(d) => tabrak_varian(&d.name, "fn", d.span, &mut temuan),
                    Item::Const(d) => tabrak_varian(&d.name, d.kata, d.span, &mut temuan),
                    Item::Var(d) => tabrak_varian(&d.name, "var", d.span, &mut temuan),
                    _ => {}
                }
            }

            for item in &file.items {
                match item {
                    Item::Fn(d) => catat(&mut nilai, &d.name, "fn", d.span, &mut temuan),
                    Item::Const(d)  => catat(&mut nilai, &d.name, d.kata, d.span, &mut temuan),
                    Item::Var(d) => catat(&mut nilai, &d.name, "var", d.span, &mut temuan),
                    Item::Struct(d) => catat(&mut tipe, &d.name, "struct", d.span, &mut temuan),
                    Item::Enum(d) => catat(&mut tipe, &d.name, "enum", d.span, &mut temuan),
                    Item::Trait(d) => catat(&mut tipe, &d.name, "trait", d.span, &mut temuan),
                    Item::Extern(b) => {
                        for ei in &b.items {
                            match ei {
                                ExternItem::Fn { name, span, .. } =>
                                    catat(&mut nilai, name, "extern fn", *span, &mut temuan),
                                ExternItem::Static { name, span, .. } =>
                                    catat(&mut nilai, name, "extern static", *span, &mut temuan),
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for (msg, span) in temuan {
            self.errors.push(ParseError { msg, span });
        }
    }

    /// Nama trait yang disebut sebuah bound (`<T: Bound>`) harus benar-benar ada.
    pub(crate) fn periksa_nama_bound(&mut self, file: &ParsedFile) {
        if !file.imports.is_empty() { return; }

        let mut dikenal: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for item in &file.items {
            if let Item::Trait(td) = item { dikenal.insert(td.name.as_str()); }
        }

        // Kumpulkan semua bound di berkas ini, dari tiap tempat yang bisa punya
        // generics atau `where`.
        let mut semua: Vec<(&Vec<TraitBound>, Span)> = Vec::new();
        for item in &file.items {
            let (generics, wheres): (&GenericParams, Option<&Vec<WhereItem>>) = match item {
                Item::Fn(fd)     => (&fd.generics,  Some(&fd.where_clause)),
                Item::Struct(sd) => (&sd.generics,  Some(&sd.where_clause)),
                Item::Trait(td)  => (&td.generics,  Some(&td.where_clause)),
                Item::Impl(idl)  => (&idl.generics, Some(&idl.where_clause)),
                Item::Enum(ed)   => (&ed.generics,  None),
                _ => continue,
            };
            for p in &generics.params { semua.push((&p.bounds, p.span)); }
            if let Some(w) = wheres {
                for it in w { semua.push((&it.bounds, it.span)); }
            }
        }

        for (bounds, span) in semua {
            for b in bounds {
                // Lifetime bukan trait, jadi tidak ikut diperiksa. Bound
                // negatif sudah ditolak lebih dulu di `parse_trait_bound`.
                let TraitBound::Trait(ty) = b else { continue };
                let nama = nama_tipe(ty);
                if dikenal.contains(nama.as_str()) { continue; }
                let mut urut: Vec<&str> = dikenal.iter().copied().collect();
                urut.sort_unstable();
                let saran = saran_nama(&nama, urut.into_iter());
                let daftar = if dikenal.is_empty() {
                    " (berkas ini belum mendeklarasikan trait apa pun)".to_string()
                } else {
                    let mut v: Vec<&str> = dikenal.iter().copied().collect();
                    v.sort_unstable();
                    format!(" (yang ada di berkas ini: {})", v.join(", "))
                };
                self.errors.push(ParseError {
                    msg: format!(
                        "trait `{}` tidak ada{}{}. Perlu diketahui: JRG belum MENEGAKKAN \
                         trait bound sama sekali — bound cuma dicek namanya. Kalau memang \
                         tidak ada yang mau dibatasi, hapus saja bound-nya",
                        nama, saran, daftar
                    ),
                    span,
                });
            }
        }
    }

    /// Nama tipe yang disebut sebuah ANOTASI harus benar-benar ada.
    pub(crate) fn periksa_nama_tipe(&mut self, file: &ParsedFile) {
        if !file.imports.is_empty() { return; }

        let mut dikenal: std::collections::HashSet<String> =
            TIPE_BAWAAN.iter().map(|s| s.to_string()).collect();
        for item in &file.items {
            match item {
                Item::Struct(sd) => { dikenal.insert(sd.name.clone()); }
                Item::Enum(ed)   => { dikenal.insert(ed.name.clone()); }
                Item::Trait(td)  => { dikenal.insert(td.name.clone()); }
                _ => {}
            }
        }

        let mut temuan: Vec<(String, Span)> = Vec::new();
        // Induk warisan dikumpulkan TERPISAH karena kalimatnya berbeda: catatan "JRG
        // tidak menegakkan anotasi tipe" benar untuk anotasi dan menyesatkan di …
        let mut induk_hilang: Vec<(String, String, Span)> = Vec::new();
        for item in &file.items {
            match item {
                Item::Fn(fd) => {
                    let g = nama_generik(&fd.generics);
                    Self::tipe_di_fn(fd, &g, &mut temuan);
                }
                Item::Struct(sd) => {
                    let g = nama_generik(&sd.generics);
                    // `struct Anjing: Hewann` — salah ketik satu huruf dari sebuah struct yang
                    // ada beberapa baris di atasnya — diterima ketiga mesin dan tidak mewariskan …
                    if let Some(ind) = &sd.parent {
                        if !g.iter().any(|x| x == ind) && !dikenal.contains(ind) {
                            induk_hilang.push((sd.name.clone(), ind.clone(), sd.span));
                        }
                    }
                    for f in &sd.fields { kumpul_nama_tipe(&f.ty, &g, &mut temuan); }
                }
                Item::Const(cd) => {
                    if let Some(t) = &cd.ty { kumpul_nama_tipe(t, &[], &mut temuan); }
                }
                Item::Var(vd) => {
                    if let Some(t) = &vd.ty { kumpul_nama_tipe(t, &[], &mut temuan); }
                }
                Item::Enum(ed) => {
                    let g = nama_generik(&ed.generics);
                    for v in &ed.variants {
                        match &v.kind {
                            EnumVariantKind::Tuple(ts) => {
                                for t in ts { kumpul_nama_tipe(t, &g, &mut temuan); }
                            }
                            EnumVariantKind::Struct(fs) => {
                                for f in fs { kumpul_nama_tipe(&f.ty, &g, &mut temuan); }
                            }
                            EnumVariantKind::Unit => {}
                        }
                    }
                }
                Item::Impl(idl) => {
                    let mut g = nama_generik(&idl.generics);
                    // `Self` selalu sah di dalam `impl`, dan tipe yang
                    // di-`impl` juga — termasuk kalau ia generik.
                    g.push("Self".to_string());
                    // Kepala `impl` ikut diperiksa: `impl Hantu for S` dan `impl T for Hantu`
                    // sama-sama salah ketik yang benar- benar terjadi, dan keduanya diterima …
                    kumpul_nama_tipe(&idl.self_type, &g, &mut temuan);
                    if let Some(tr) = &idl.trait_ref {
                        kumpul_nama_tipe(tr, &g, &mut temuan);
                    }
                    for it in &idl.items {
                        if let ImplItem::Method(fd) = it {
                            let mut gm = g.clone();
                            gm.extend(nama_generik(&fd.generics));
                            Self::tipe_di_fn(fd, &gm, &mut temuan);
                        }
                    }
                }
                Item::Trait(td) => {
                    let mut g = nama_generik(&td.generics);
                    g.push("Self".to_string());
                    for it in &td.items {
                        let fd = match it {
                            TraitItem::Method(fd) | TraitItem::DefaultMethod(fd) => fd,
                            _ => continue,
                        };
                        let mut gm = g.clone();
                        gm.extend(nama_generik(&fd.generics));
                        Self::tipe_di_fn(fd, &gm, &mut temuan);
                    }
                }
                _ => {}
            }
        }

        for (anak, induk, span) in induk_hilang {
            let mut urut: Vec<&str> = dikenal.iter().map(|s| s.as_str()).collect();
            urut.sort_unstable();
            let saran = saran_nama(&induk, urut.into_iter());
            self.errors.push(ParseError {
                msg: format!(
                    "struct `{}` mewarisi `{}`, tapi tipe `{}` tidak ada{}\n\
                     catatan: induk yang tidak ada tidak mewariskan apa pun \u{2014} field, \
                     nilai default dan method milik induknya tidak akan berlaku, dan yang \
                     gagal biasanya baris yang lain",
                    anak, induk, induk, saran
                ),
                span,
            });
        }

        for (nama, span) in temuan {
            if dikenal.contains(&nama) { continue; }
            let mut urut: Vec<&str> = dikenal.iter().map(|s| s.as_str()).collect();
            urut.sort_unstable();
            let saran = saran_nama(&nama, urut.into_iter());
            // Kalau sarannya sudah ada, `bantuan:` tidak boleh menyuruh mendeklarasikan
            // nama yang SALAH KETIK itu — dua kalimat yang saling bertentangan di satu …
            let bantuan = if saran.is_empty() {
                format!("bantuan: deklarasikan `struct {}`/`enum {}`, atau pakai tipe yang ada",
                        nama, nama)
            } else {
                "bantuan: perbaiki ejaannya".to_string()
            };
            self.errors.push(ParseError {
                msg: format!(
                    "tipe `{}` tidak ada{}\n\
                     {}\n\
                     catatan: JRG TIDAK menegakkan anotasi tipe — `let x: int = \"teks\"` \
                     diterima — jadi yang diperiksa di sini cuma ADA-tidaknya namanya, \
                     bukan cocok-tidaknya nilainya",
                    nama, saran, bantuan
                ),
                span,
            });
        }
    }

    pub(crate) fn tipe_di_fn(fd: &FnDecl, generik: &[String], keluar: &mut Vec<(String, Span)>) {
        for p in &fd.params { kumpul_nama_tipe(&p.ty, generik, keluar); }
        if let Some(r) = &fd.ret_type { kumpul_nama_tipe(r, generik, keluar); }
        if let FnBody::Block(b) = &*fd.body { kumpul_tipe_blok(b, generik, keluar); }
    }

    /// Tiap `impl Trait for Tipe` wajib menyediakan semua method ABSTRAK milik
    /// traitnya.
    pub(crate) fn periksa_impl_lengkap(&mut self, file: &ParsedFile) {
        use std::collections::HashMap;

        // nama trait -> nama method yang WAJIB disediakan
        let mut wajib: HashMap<&str, Vec<&str>> = HashMap::new();
        for item in &file.items {
            if let Item::Trait(td) = item {
                let abstrak: Vec<&str> = td.items.iter().filter_map(|it| match it {
                    TraitItem::Method(fd) => Some(fd.name.as_str()),
                    _ => None,
                }).collect();
                if !abstrak.is_empty() {
                    wajib.insert(td.name.as_str(), abstrak);
                }
            }
        }
        if wajib.is_empty() { return; }

        for item in &file.items {
            let Item::Impl(imp) = item else { continue };
            let Some(tr) = &imp.trait_ref else { continue };
            let nama_trait = nama_tipe(tr);
            let Some(perlu) = wajib.get(nama_trait.as_str()) else { continue };

            let disediakan: Vec<&str> = imp.items.iter().filter_map(|it| match it {
                ImplItem::Method(fd) => Some(fd.name.as_str()),
                _ => None,
            }).collect();

            let kurang: Vec<&str> = perlu.iter().copied()
                .filter(|m| !disediakan.contains(m)).collect();
            if kurang.is_empty() { continue; }

            let tipe = nama_tipe(&imp.self_type);
            let daftar = kurang.iter().map(|m| format!("`{}`", m))
                .collect::<Vec<_>>().join(", ");
            self.errors.push(ParseError { msg: pesan_impl_kurang(&nama_trait, &tipe, &daftar), span: imp.span });
        }
    }

    pub(crate) fn parse_package_decl(&mut self) -> Result<String, ParseError> {
        self.expect(&TokenKind::KwPackage)?;
        self.expect_ident()
    }

    pub(crate) fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwImport)?;
        if self.at_exact(&TokenKind::LParen) {
            // grouped imports — parse first one and return; caller loops
            self.advance(); // (
            let imp = self.parse_import_spec(lo)?;
            self.skip_semis();
            self.expect(&TokenKind::RParen)?;
            Ok(imp)
        } else {
            self.parse_import_spec(lo)
        }
    }

    pub(crate) fn parse_import_spec(&mut self, lo: Span) -> Result<ImportDecl, ParseError> {
        let alias = match &self.current().kind {
            TokenKind::Ident(_) if !matches!(self.peek().kind, TokenKind::StringLit(_)) => {
                // next token after ident is not a string → this ident IS the path
                None
            }
            TokenKind::Ident(s) => {
                let a = s.clone(); self.advance(); Some(a)
            }
            _ => None,
        };
        let path = match self.current().kind.clone() {
            TokenKind::StringLit(s) => { self.advance(); s }
            _ => return Err(self.error(
                "setelah `import` harusnya ada path modul dalam tanda kutip, mis. `import \"utils/matek\"`".to_string(),
            )),
        };
        // Bentuk kedua penulisan alias: `import "path" as nama`.
        let alias = if self.eat(&TokenKind::KwAs) {
            let a = self.expect_ident_or_kw()?;
            if alias.is_some() {
                return Err(self.error(format!(
                    "alias import ditulis dua kali — pilih salah satu: `import {} \"{}\"` atau `import \"{}\" as {}`",
                    a, path, path, a
                )));
            }
            Some(a)
        } else {
            alias
        };
        Ok(ImportDecl { alias, path, span: self.merge_span(lo) })
    }

    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Nama yang bisa dibaca manusia untuk sebuah `TypeExpr` — dipakai diagnostik
/// `periksa_impl_lengkap` dan penolakan sasaran `as`.
pub(crate) fn nama_tipe(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(nt)         => nt.name.clone(),
        TypeExpr::Ownership(o)      => nama_tipe(&o.inner),
        // Dipetakan eksplisit, BUKAN `format!("{:?}").to_lowercase()`.
        TypeExpr::Primitive(p) => {
            use PrimTy::*;
            match p {
                I8 => "i8", I16 => "i16", I32 => "i32", I64 => "i64",
                I128 => "i128", Isize => "isize",
                U8 => "u8", U16 => "u16", U32 => "u32", U64 => "u64",
                U128 => "u128", Usize => "usize",
                Int => "int", Uint => "uint",
                F32 => "f32", F64 => "f64", F128 => "f128",
                C64 => "c64", C128 => "c128",
                Bool => "bool", Byte => "byte", Rune => "rune",
                Str => "str", String_ => "string", Unit => "unit",
            }
            .to_string()
        }
        TypeExpr::Slice(_) => "slice".to_string(),
        TypeExpr::Array(_) => "array".to_string(),
        TypeExpr::Map(_) => "map".to_string(),
        TypeExpr::Tuple(_) => "tuple".to_string(),
        TypeExpr::Fn(_) => "fn".to_string(),
        TypeExpr::Chan(_) => "chan".to_string(),
        TypeExpr::Pointer(_) => "pointer".to_string(),
        TypeExpr::Never => "never".to_string(),
        TypeExpr::Inferred => "_".to_string(),
        TypeExpr::Error(_) => "<error>".to_string(),
    }
}

pub fn parse(tokens: Vec<Token>, recover: bool) -> (ParsedFile, Vec<ParseError>) {
    let mut p = Parser::new(tokens, recover);
    let file  = p.parse_file();
    let errs  = p.errors().to_vec();
    (file, errs)
}

// End of §3 — Parser

/// Nama tipe yang SELALU sah walau tidak dideklarasikan berkasnya.
fn bongkar_pola_closure(params: Vec<FnParam>, body: Box<Expr>) -> (Vec<FnParam>, Box<Expr>) {
    let (params, fb) = bongkar_pola_parameter(params, FnBody::Expr(body));
    match fb {
        FnBody::Expr(e) => (params, e),
        FnBody::Block(b) => { let sp = b.span; (params, Box::new(Expr::Block(b))) }
        FnBody::Abstrak => unreachable!("closure selalu punya badan"),
    }
}

/// Ubah parameter berpola (`fn f((a, b))`) jadi parameter biasa plus `let`.
fn bongkar_pola_parameter(
    params: Vec<FnParam>, body: FnBody,
) -> (Vec<FnParam>, FnBody) {
    let jamak = |pat: &Pattern| matches!(
        pat, Pattern::Tuple { .. } | Pattern::Array { .. } | Pattern::Struct { .. });
    if !params.iter().any(|p| jamak(&p.pattern)) {
        return (params, body);
    }
    let mut baru = Vec::with_capacity(params.len());
    let mut ikat: Vec<Stmt> = Vec::new();
    for (i, mut p) in params.into_iter().enumerate() {
        if !jamak(&p.pattern) { baru.push(p); continue; }
        let sementara = format!("__pola{}", i);
        let span = p.span;
        let pola = std::mem::replace(
            &mut p.pattern,
            Pattern::Ident { name: sementara.clone(), is_mut: false, span },
        );
        ikat.push(Stmt::Let(LetStmt {
            is_mut: false,
            ownership: None,
            pattern: pola,
            ty: None,
            init: Box::new(Expr::Ident(sementara, span)),
            lifetime: None,
            span,
        }));
        baru.push(p);
    }
    let body = match body {
        FnBody::Block(mut b) => {
            ikat.append(&mut b.stmts);
            b.stmts = ikat;
            FnBody::Block(b)
        }
        FnBody::Expr(e) => {
            let span = e.span();
            FnBody::Block(Block { stmts: ikat, tail: Some(e), span })
        }
        // Tanda tangan tanpa badan tidak punya tempat untuk `let`-nya, dan
        // sebuah pola di sana tidak mengikat apa pun bagi siapa pun.
        FnBody::Abstrak => FnBody::Abstrak,
    };
    (baru, body)
}

/// `fn P(a0, a1) { return P { _0: a0, _1: a1 } }` untuk sebuah struct tuple.
fn konstruktor_tuple(
    nama: &str, fields: &[StructField], vis: Visibility, span: Span,
) -> FnDecl {
    let params: Vec<FnParam> = fields.iter().enumerate().map(|(i, f)| FnParam {
        is_variadic: false,
        is_mut: false,
        borrow: BorrowKind::None,
        pattern: Pattern::Ident { name: format!("a{}", i), is_mut: false, span },
        ty: f.ty.clone(),
        default: None,
        lifetime: None,
        span,
    }).collect();
    let init: Vec<FieldInit> = fields.iter().enumerate().map(|(i, f)| FieldInit {
        name: f.name.clone(),
        value: Expr::Ident(format!("a{}", i), span),
        span,
    }).collect();
    let tipe_diri = || TypeExpr::Named(NamedType {
        package: None, modul: None, name: nama.to_string(), args: Vec::new(), span,
    });
    let badan = Expr::Struct {
        ty: tipe_diri(),
        fields: init,
        spread: None,
        span,
    };
    FnDecl {
        vis,
        is_async: false,
        name: nama.to_string(),
        generics: GenericParams::default(),
        params,
        // Tipe kembaliannya DISEBUT, bukan dibiarkan disimpulkan: backend LLVM
        // menyimpulkan tipe return dari badan fungsi dan menyerah pada literal …
        ret_type: Some(tipe_diri()),
        where_clause: Vec::new(),
        body: FnBody::Expr(Box::new(badan)).into(),
        span,
    }
}

pub fn prim_dari_nama(n: &str) -> Option<PrimTy> {
    Some(match n {
        "i8" => PrimTy::I8, "i16" => PrimTy::I16, "i32" => PrimTy::I32,
        "i64" => PrimTy::I64, "i128" => PrimTy::I128, "isize" => PrimTy::Isize,
        "u8" => PrimTy::U8, "u16" => PrimTy::U16, "u32" => PrimTy::U32,
        "u64" => PrimTy::U64, "u128" => PrimTy::U128, "usize" => PrimTy::Usize,
        "int" => PrimTy::Int, "uint" => PrimTy::Uint,
        "f32" => PrimTy::F32, "f64" => PrimTy::F64, "f128" => PrimTy::F128,
        // `float` adalah nama generik untuk pecahan, sepadan dengan `int` untuk
        // bilangan bulat — dan itu pula nama yang dijawab `typeof(1.0)`.
        "float" => PrimTy::F64,
        "bool" => PrimTy::Bool, "byte" => PrimTy::Byte, "rune" => PrimTy::Rune,
        "str" => PrimTy::Str, "string" => PrimTy::String_, "unit" => PrimTy::Unit,
        _ => return None,
    })
}

pub const TIPE_BAWAAN: &[&str] = &[
    // Dibuat interpreter sendiri (`interpreter/tugas.rs`).
    "Future", "Task",
    // Bawaan bahasa.
    "Option", "Result", "Self",
    // `array` telanjang: tipe parameter yang tipe elemennya disimpulkan di
    // situs pemanggilan (lihat `is_bare_array_type`).
    "array",
    // Nama KIND yang dijawab `typeof`.
    "dict", "tuple", "function", "closure", "native_fn", "canvas", "channel",
    // Handle berbasis Arc yang bisa dipegang sebuah nilai.
    "Canvas", "Channel", "PyObj", "JsObj", "PhpObj", "Nd", "Atomic", "Mutex",
    // Struct yang dibuat interpreter sendiri.
    "DokDb", "DokHttpResp", "DokLinReg", "DokRequest", "DokResponse", "DokWs",
    "HeapStats", "JsonObject", "MousePos", "Ptr", "UdpPacket",
    // Handle modul `.wasm` yang sedang dimuat (`wasm::muat`).
    "WasmModul",
    // Handle pointer fungsi .NET (`dotnet::fungsi`).
    "DotnetFn",
    // Handle objek Java/Kotlin (`jvm::baru`, `kotlin::objek`).
    "JavaObj",
    // Hasil `proses::keluaran`/`proses::baca`: stdout, stderr, kode keluar, sukses.
    "ProsesBagian", "ProsesHasil",
    // Rekaman data pustaka yang boleh ditulis sebagai literal (`STRUCT_DATA_PUSTAKA`).
    "Warna",
];

/// Saran " — maksudnya `X`?" untuk sebuah nama yang tidak dikenal.
fn saran_nama<'a>(nama: &str, calon: impl Iterator<Item = &'a str> + Clone) -> String {
    crate::diag::saran_nama(nama, calon)
}

/// Nama parameter generik sebuah deklarasi — semuanya sah sebagai tipe di
/// dalam deklarasi itu.
fn nama_generik(g: &GenericParams) -> Vec<String> {
    g.params.iter().map(|p| p.name.clone()).collect()
}

/// Kalimat untuk `impl` yang tidak mengisi method abstrak trait-nya.
pub fn pesan_impl_kurang(nama_trait: &str, tipe: &str, daftar: &str) -> String {
    format!(
        "`impl {} for {}` belum menyediakan method yang diwajibkan trait-nya: {} \
         — trait `{}` mendeklarasikannya tanpa badan, jadi tiap impl harus \
         mengisinya sendiri. Kalau yang dimaksud sebenarnya perilaku bawaan, \
         beri badan di traitnya",
        nama_trait, tipe, daftar, nama_trait)
}

/// `impl Trait for Tipe` terhadap trait yang TERLIHAT (berkas ini dan modul yang
/// diimpornya): trait yang tidak ada, atau method abstraknya belum diisi. Dipanggil dua
/// mesin acuan, karena parser tidak menyelesaikan modul.
pub fn periksa_impl(imp: &ImplDecl, wajib: impl Fn(&str) -> Option<Vec<String>>) -> Option<String> {
    let Some(TypeExpr::Named(tr)) = &imp.trait_ref else { return None };
    let tipe = nama_tipe(&imp.self_type);
    let Some(perlu) = wajib(&tr.name) else {
        return Some(format!(
            "`impl {} for {}`: tidak ada trait bernama `{}` — bukan di berkas ini, bukan di \
             modul yang diimpornya. Deklarasikan `trait {} {{ .. }}`, atau impor modul pemiliknya",
            tr.name, tipe, tr.name, tr.name));
    };
    let kurang: Vec<String> = perlu.iter()
        .filter(|m| !imp.items.iter().any(|it| matches!(it, ImplItem::Method(fd) if &fd.name == *m)))
        .map(|m| format!("`{}`", m)).collect();
    (!kurang.is_empty()).then(|| pesan_impl_kurang(&tr.name, &tipe, &kurang.join(", ")))
}

/// Method abstrak sebuah trait, urut tulis.
pub fn metode_abstrak(td: &TraitDecl) -> Vec<String> {
    td.items.iter().filter_map(|it| match it {
        TraitItem::Method(fd) => Some(fd.name.clone()),
        _ => None,
    }).collect()
}

/// Kumpulkan tiap nama tipe yang disebut `ty`, termasuk yang bersarang.
fn kumpul_nama_tipe(ty: &TypeExpr, generik: &[String], keluar: &mut Vec<(String, Span)>) {
    match ty {
        TypeExpr::Named(nt) => {
            // `paket::Tipe` menyebut modul lain; parser tidak
            // menyelesaikan modul, jadi ia tidak diperiksa.
            if nt.package.is_none() && !generik.iter().any(|g| g == &nt.name) {
                keluar.push((nt.name.clone(), nt.span));
            }
            for a in &nt.args { kumpul_nama_tipe(a, generik, keluar); }
        }
        TypeExpr::Array(a)     => kumpul_nama_tipe(&a.elem, generik, keluar),
        TypeExpr::Slice(s)     => kumpul_nama_tipe(&s.elem, generik, keluar),
        TypeExpr::Pointer(p)   => kumpul_nama_tipe(&p.inner, generik, keluar),
        TypeExpr::Ownership(o) => kumpul_nama_tipe(&o.inner, generik, keluar),
        TypeExpr::Chan(c)      => kumpul_nama_tipe(&c.elem, generik, keluar),
        TypeExpr::Map(m) => {
            kumpul_nama_tipe(&m.key, generik, keluar);
            kumpul_nama_tipe(&m.val, generik, keluar);
        }
        TypeExpr::Tuple(ts) => {
            for t in ts { kumpul_nama_tipe(t, generik, keluar); }
        }
        TypeExpr::Fn(f) => {
            let mut g = generik.to_vec();
            g.extend(nama_generik(&f.generics));
            for t in &f.params { kumpul_nama_tipe(t, &g, keluar); }
            if let Some(r) = &f.ret { kumpul_nama_tipe(r, &g, keluar); }
        }
        _ => {}
    }
}

/// Anotasi `let`/`var`/`const` di dalam sebuah blok, termasuk blok bersarang.
fn kumpul_tipe_blok(b: &Block, generik: &[String], keluar: &mut Vec<(String, Span)>) {
    for st in &b.stmts { kumpul_tipe_stmt(st, generik, keluar); }
    if let Some(t) = &b.tail { kumpul_tipe_ekspr(t, generik, keluar); }
}

fn kumpul_tipe_stmt(st: &Stmt, generik: &[String], keluar: &mut Vec<(String, Span)>) {
    match st {
        // Anotasinya sendiri, LALU nilainya — sebuah closure di sisi kanan
        // (`let f = |x: Hantu| …`) membawa anotasinya sendiri, dan itu tidak
        // akan pernah terlihat kalau cuma `ty` yang dibaca.
        Stmt::Let(l) => {
            if let Some(t) = &l.ty { kumpul_nama_tipe(t, generik, keluar); }
            kumpul_tipe_ekspr(&l.init, generik, keluar);
        }
        Stmt::Var(v) => {
            if let Some(t) = &v.ty { kumpul_nama_tipe(t, generik, keluar); }
            if let Some(e) = &v.init { kumpul_tipe_ekspr(e, generik, keluar); }
        }
        Stmt::Const(c) => { if let Some(t) = &c.ty { kumpul_nama_tipe(t, generik, keluar); } }
        Stmt::Assign(a) => { for v in &a.values { kumpul_tipe_ekspr(v, generik, keluar); } }
        Stmt::Return(r) => { for v in &r.values { kumpul_tipe_ekspr(v, generik, keluar); } }
        Stmt::Block(b) => kumpul_tipe_blok(b, generik, keluar),
        Stmt::If(i) => {
            kumpul_tipe_blok(&i.then, generik, keluar);
            for (_, _, bb) in &i.else_ifs { kumpul_tipe_blok(bb, generik, keluar); }
            if let Some(e) = &i.else_ { kumpul_tipe_blok(e, generik, keluar); }
        }
        Stmt::While(w) => kumpul_tipe_blok(&w.body, generik, keluar),
        Stmt::Loop(l)  => kumpul_tipe_blok(&l.body, generik, keluar),
        Stmt::For(ForStmt::Classic { body, .. })
        | Stmt::For(ForStmt::Range { body, .. }) => kumpul_tipe_blok(body, generik, keluar),
        Stmt::Match(m) => {
            for a in &m.arms {
                match &a.body {
                    MatchBody::Block(bb) => kumpul_tipe_blok(bb, generik, keluar),
                    MatchBody::Expr(e)   => kumpul_tipe_ekspr(e, generik, keluar),
                }
            }
        }
        Stmt::Expr(e) => kumpul_tipe_ekspr(e, generik, keluar),
        _ => {}
    }
}

fn kumpul_tipe_ekspr(e: &Expr, generik: &[String], keluar: &mut Vec<(String, Span)>) {
    match e {
        // Badan closure adalah EKSPRESI, jadi `let x: Hantu` di dalamnya
        // tidak akan pernah terlihat oleh penelusur statement.
        Expr::Closure { params, ret, body, .. } => {
            for p in params { kumpul_nama_tipe(&p.ty, generik, keluar); }
            if let Some(r) = ret { kumpul_nama_tipe(r, generik, keluar); }
            kumpul_tipe_ekspr(body, generik, keluar);
        }
        Expr::Block(b) => kumpul_tipe_blok(b, generik, keluar),
        // Tempat sebuah closure realistis muncul.
        Expr::Call { args, .. } => {
            for a in args { kumpul_tipe_ekspr(a, generik, keluar); }
        }
        Expr::Method { obj, args, .. } => {
            kumpul_tipe_ekspr(obj, generik, keluar);
            for a in args { kumpul_tipe_ekspr(a, generik, keluar); }
        }
        Expr::Binary { lhs, rhs, .. } => {
            kumpul_tipe_ekspr(lhs, generik, keluar);
            kumpul_tipe_ekspr(rhs, generik, keluar);
        }
        Expr::Unary { expr, .. } => kumpul_tipe_ekspr(expr, generik, keluar),
        Expr::Cast { expr, ty, .. } => {
            kumpul_tipe_ekspr(expr, generik, keluar);
            kumpul_nama_tipe(ty, generik, keluar);
        }
        Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => {
            for x in elems { kumpul_tipe_ekspr(x, generik, keluar); }
        }
        Expr::If(i) => {
            kumpul_tipe_blok(&i.then, generik, keluar);
            for (_, _, bb) in &i.else_ifs { kumpul_tipe_blok(bb, generik, keluar); }
            if let Some(x) = &i.else_ { kumpul_tipe_blok(x, generik, keluar); }
        }
        Expr::Match { arms, .. } => {
            for a in arms {
                match &a.body {
                    MatchBody::Block(bb) => kumpul_tipe_blok(bb, generik, keluar),
                    MatchBody::Expr(x)   => kumpul_tipe_ekspr(x, generik, keluar),
                }
            }
        }
        _ => {}
    }
}

/// Nama yang diikat sebuah pola, dalam URUTAN KEMUNCULAN.
fn kumpul_nama_pola(p: &Pattern, keluar: &mut Vec<String>) {
    match p {
        Pattern::Ident { name, .. } => {
            // `_` tidak mengikat, dan nama varian unit MENGUJI alih-alih mengikat
            // (`None`, atau varian tanpa payload yang disebut telanjang) — memasukkannya …
            if name != "_" && !name.starts_with(char::is_uppercase) {
                keluar.push(name.clone());
            }
        }
        Pattern::Binding { name, inner, .. } => {
            keluar.push(name.clone());
            kumpul_nama_pola(inner, keluar);
        }
        Pattern::Tuple { elems, .. } => { for e in elems { kumpul_nama_pola(e, keluar) } }
        Pattern::Struct { fields, .. } => { for f in fields { kumpul_nama_pola(&f.pat, keluar) } }
        Pattern::Enum { inner, .. } => { if let Some(i) = inner { kumpul_nama_pola(i, keluar) } }
        Pattern::Ref { inner, .. } => kumpul_nama_pola(inner, keluar),
        Pattern::Array { before, rest, after, .. } => {
            for e in before { kumpul_nama_pola(e, keluar) }
            if let Some(Some(n)) = rest { keluar.push(n.clone()) }
            for e in after { kumpul_nama_pola(e, keluar) }
        }
        // Alternasi tidak boleh menyumbang nama: tiap cabangnya bisa
        // mengikat himpunan yang berbeda, dan `let … else` butuh satu
        // himpunan yang pasti.
        Pattern::Or { .. } => {}
        Pattern::Literal { .. } | Pattern::Range { .. } | Pattern::Wildcard(_) => {}
    }
}

/// Apakah blok ini pasti MENYIMPANG — tidak pernah jatuh sampai ujungnya?
fn blok_menyimpang(b: &Block) -> bool {
    if b.tail.is_some() { return false; }
    match b.stmts.last() {
        Some(Stmt::Return(_)) | Some(Stmt::Break(_)) | Some(Stmt::Continue(_)) => true,
        Some(Stmt::Block(inner)) => blok_menyimpang(inner),
        Some(Stmt::If(i)) => {
            blok_menyimpang(&i.then)
                && i.else_ifs.iter().all(|(_, _, bb)| blok_menyimpang(bb))
                && i.else_.as_ref().map_or(false, blok_menyimpang)
        }
        Some(Stmt::Match(m)) => m.arms.iter().all(|a| match &a.body {
            MatchBody::Block(bb) => blok_menyimpang(bb),
            MatchBody::Expr(_) => false,
        }),
        _ => false,
    }
}

/// Nilai ekor `main` dibuang (kecuali `Err`/`None`), jadi ekor yang berupa NAMA
/// telanjang juga diperiksa sebagai pernyataan — `pub fn main() { f }` dulu diam.
/// Membaca nama tidak berefek samping, jadi membacanya dua kali aman.
fn periksa_ekor_main(items: &mut [Item]) {
    fn nama_murni(e: &Expr) -> bool {
        match e {
            Expr::Ident(..) | Expr::PathExpr { .. } => true,
            Expr::Field { obj, .. } => nama_murni(obj),
            _ => false,
        }
    }
    for item in items.iter_mut() {
        let Item::Fn(fd) = item else { continue };
        if fd.name != "main" { continue; }
        match std::sync::Arc::make_mut(&mut fd.body) {
            FnBody::Block(b) => {
                if let Some(t) = b.tail.as_deref().filter(|t| nama_murni(t)) {
                    b.stmts.push(Stmt::Expr(Box::new(t.clone())));
                }
            }
            FnBody::Expr(e) if nama_murni(e) => {
                let span = e.span();
                let blok = Block { stmts: vec![Stmt::Expr(e.clone())], tail: Some(e.clone()), span };
                fd.body = FnBody::Block(blok).into();
            }
            _ => {}
        }
    }
}

/// Panggilan fungsi tingkat atas berkas ini dengan jumlah argumen di luar batas (`--periksa`).
pub fn aritas_fungsi_pemakai(file: &ParsedFile) -> Vec<(String, Span)> {
    Parser::telusuri_lingkup(file).aritas_pemakai
}

/// Lokal `let`/`var`/`const`/`:=` yang tidak pernah dibaca, dari penelusuran lingkup yang sama
/// dengan aturan mutabilitas (dipakai `jrgm lint`); urut menurut letaknya.
pub fn lokal_tak_dibaca(file: &ParsedFile) -> Vec<(String, Span)> {
    let mut v = Parser::telusuri_lingkup(file).tak_dibaca;
    v.sort_by_key(|(_, sp)| sp.lo.0);
    v
}
