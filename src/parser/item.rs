// src/parser/item.rs — deklarasi tingkat atas: fn, struct, enum, trait,
// impl, mod, extern

use super::*;

impl Parser {
    // ── Items ─────────────────────────────────────────────────────────────

    pub(crate) fn parse_item(&mut self) -> Result<Item, ParseError> {
        let vis = self.parse_visibility();
        // Bentuk dari bahasa lain ditolak lewat definisi yang SAMA dengan posisi
        // pernyataan: sebuah berkas ber-`package` menaruh `static G = 5` di posisi …
        if let Some(e) = self.tolak_bentuk_asing() { return Err(e); }
        match &self.current().kind {
            TokenKind::KwAsync | TokenKind::KwFn => {
                {
                    let d = self.parse_fn_decl(vis)?;
                    self.tolak_self_bukan_method(&d)?;
                    Ok(Item::Fn(d))
                }
            }
            TokenKind::KwStruct => self.parse_struct_decl(vis).map(Item::Struct),
            TokenKind::KwEnum   => self.parse_enum_decl(vis).map(Item::Enum),
            TokenKind::KwTrait  => self.parse_trait_decl(vis).map(Item::Trait),
            TokenKind::KwImpl   => self.parse_impl_decl().map(Item::Impl),
            // `type Alias = T` dan `mod nama { … }` DITOLAK, dan itu keputusan sadar —
            // bukan "belum sempat".
            TokenKind::KwType   => {
                let ta = self.parse_type_alias(vis)?;
                Err(ParseError {
                    msg: format!(
                        "alias tipe `{}` belum punya arti di JRG\n\
                         bantuan: pakai tipe yang di sebelah kanannya langsung\n\
                         catatan: anotasi tipe di JRG TIDAK ditegakkan — `let x: int = \"teks\"` \
                         diterima kedua backend acuan — jadi sebuah alias tidak bisa \
                         memeriksa apa pun, dan satu-satunya tempat ia akan terasa \
                         (`nilai as {}`) justru yang selama ini ditolak",
                        ta.name, ta.name),
                    span: ta.span,
                })
            }
            TokenKind::KwConst  => self.parse_const_decl(vis).map(Item::Const),
            TokenKind::KwVar    => self.parse_var_decl_item(vis).map(Item::Var),
            TokenKind::KwLet    => self.parse_let_item(vis),
            TokenKind::KwMod    => {
                let md = self.parse_mod_decl(vis)?;
                Err(ParseError {
                    msg: format!(
                        "modul sebaris `mod {}` belum didukung\n\
                         bantuan: taruh isinya di berkas `{}.jrg` lalu `import \"{}\"` — \
                         itu bentuk modul yang memang bekerja, lengkap dengan \
                         `nama::item` dan aturan privat `_`\n\
                         catatan: sebelum ini `mod` diurai lalu dibuang tanpa pesan, jadi \
                         isinya tidak pernah bisa dipanggil sama sekali",
                        md.name, md.name, md.name),
                    span: md.span,
                })
            }
            TokenKind::KwExtern => self.parse_extern_block().map(Item::Extern),
            TokenKind::KwUse    => self.parse_use_decl().map(Item::Use),
            other => Err(self.error(format!("{} tidak bisa berdiri di tingkat atas berkas — yang boleh di sini cuma deklarasi (`fn`, `struct`, `enum`, `trait`, `impl`, `let`, `var`, `const`, `import`, `extern`)", other.tulisan()))),
        }
    }

    pub(crate) fn parse_visibility(&mut self) -> Visibility {
        if self.eat(&TokenKind::KwPub) { Visibility::Public } else { Visibility::Private }
    }

    // ── Function declaration ──────────────────────────────────────────────

    pub(crate) fn parse_fn_decl(&mut self, vis: Visibility) -> Result<FnDecl, ParseError> {
        self.parse_fn_decl_inner(vis, false, false)
    }

    /// Method di dalam `impl`: namanya boleh sebuah kata kunci.
    pub(crate) fn parse_fn_decl_metode(&mut self, vis: Visibility) -> Result<FnDecl, ParseError> {
        self.parse_fn_decl_inner(vis, false, true)
    }

    /// Tolak parameter `self` pada fungsi yang bukan method.
    pub(crate) fn tolak_self_bukan_method(&self, d: &FnDecl) -> Result<(), ParseError> {
        for p in &d.params {
            if let Pattern::Ident { name, span, .. } = &p.pattern {
                if name == "self" {
                    return Err(ParseError {
                        span: *span,
                        msg: format!(
                            "`self` cuma berarti di dalam `impl` atau `trait` — `{}` \
                             bukan method, jadi ia tidak punya penerima.\n  \
                             bantuan: kalau ini memang method (termasuk hook tampilan \
                             `to_string`), taruh di dalam `impl <Tipe> {{ ... }}`; kalau \
                             bukan, ganti nama parameternya",
                            d.name
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// Sama dengan `parse_fn_decl`, tapi badan boleh TIDAK ADA — bentuk
    /// yang cuma sah di dalam `trait`, dan artinya method itu wajib
    /// disediakan tiap `impl`.
    pub(crate) fn parse_fn_decl_dalam_trait(&mut self, vis: Visibility) -> Result<FnDecl, ParseError> {
        self.parse_fn_decl_inner(vis, true, true)
    }

    pub(crate) fn parse_fn_decl_inner(
        &mut self,
        vis: Visibility,
        boleh_tanpa_badan: bool,
        nama_boleh_kata_kunci: bool,
    ) -> Result<FnDecl, ParseError> {
        let lo = self.span_lo();
        let is_async = self.eat(&TokenKind::KwAsync);
        self.expect(&TokenKind::KwFn)?;
        let name = if nama_boleh_kata_kunci {
            self.expect_ident_or_kw()?
        } else {
            self.expect_ident()?
        };
        let generics = self.parse_generic_params()?;
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_fn_params()?;
        self.expect(&TokenKind::RParen)?;
        let ret_type = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let where_clause = self.parse_where_clause()?;
        let body = if self.eat(&TokenKind::Eq) {
            let e = self.parse_expr()?;
            self.expect_semi_or_closer()?;
            FnBody::Expr(Box::new(e))
        } else if boleh_tanpa_badan && !self.at_exact(&TokenKind::LBrace) {
            // Tanda tangan tanpa badan di dalam `trait`.
            self.eat(&TokenKind::Semi);
            FnBody::Abstrak
        } else {
            FnBody::Block(self.parse_block()?)
        };
        let (params, body) = bongkar_pola_parameter(params, body);
        Ok(FnDecl { vis, is_async, name, generics, params, ret_type, where_clause, body: body.into(),
                    span: self.merge_span(lo) })
    }

    pub(crate) fn parse_fn_params(&mut self) -> Result<Vec<FnParam>, ParseError> {
        let mut params = Vec::new();
        while !self.at_exact(&TokenKind::RParen) {
            params.push(self.parse_fn_param()?);
            if !self.eat(&TokenKind::Comma) { break; }
        }
        periksa_urutan_default(&params)?;
        Ok(params)
    }

    pub(crate) fn parse_fn_param(&mut self) -> Result<FnParam, ParseError> {
        let lo = self.span_lo();
        let is_mut = self.eat(&TokenKind::KwMut);

        // Handle `self`, `&self`, `&mut self` as receiver parameter
        let borrow_for_self = if self.at_exact(&TokenKind::Amp) {
            // peek: is next `self` or `mut self`?
            let next = &self.peek().kind;
            if matches!(next, TokenKind::KwSelf)
                || matches!(next, TokenKind::KwMut) {
                self.advance(); // consume &
                if self.at_exact(&TokenKind::KwMut) {
                    self.advance(); // consume mut
                    Some(BorrowKind::RefMut)
                } else {
                    Some(BorrowKind::Ref)
                }
            } else { None }
        } else { None };

        if let Some(borrow) = borrow_for_self {
            // Must be followed by `self`
            if self.at_exact(&TokenKind::KwSelf) {
                self.advance(); // consume `self`
                let pattern = Pattern::Ident {
                    name: "self".to_string(),
                    is_mut: false,
                    span: self.merge_span(lo),
                };
                // Optional `: Type` after &self
                let ty = if self.eat(&TokenKind::Colon) {
                    self.parse_type()?
                } else {
                    TypeExpr::Inferred
                };
                return Ok(FnParam { is_variadic: false, is_mut: false, borrow, pattern, ty,
                    default: None, lifetime: None, span: self.merge_span(lo) });
            }
        }

        // Handle bare `self` (owned receiver)
        if self.at_exact(&TokenKind::KwSelf) {
            self.advance();
            let pattern = Pattern::Ident {
                name: "self".to_string(),
                is_mut,
                span: self.merge_span(lo),
            };
            let ty = if self.eat(&TokenKind::Colon) {
                self.parse_type()?
            } else {
                TypeExpr::Inferred
            };
            return Ok(FnParam { is_variadic: false, is_mut, borrow: BorrowKind::None, pattern, ty,
                default: None, lifetime: None, span: self.merge_span(lo) });
        }

        // Normal borrow prefix (non-self)
        let borrow = if self.eat(&TokenKind::Amp) {
            if self.eat(&TokenKind::KwMut) { BorrowKind::RefMut }
            else { BorrowKind::Ref }
        } else if self.at_exact(&TokenKind::AmpMut) {
            self.advance(); BorrowKind::RefMut
        } else {
            BorrowKind::None
        };
        // `parse_single_pattern` (BUKAN `parse_pattern`) sengaja: parameter secara
        // semantik tidak bisa jadi or-pattern (`a | b`) seperti match arm, dan pakai …
        let pattern = self.parse_single_pattern()?;
        // `sisa...` — parameter pengumpul. Diperiksa TEPAT setelah polanya,
        // sebelum anotasi tipe, karena `sisa...: int` tidak punya arti:
        // yang dikumpulkan selalu sebuah array.
        let is_variadic = self.eat(&TokenKind::DotDotDot);
        // Anotasi tipe sekarang OPSIONAL (dulu wajib pakai `: Type`) — sama seperti
        // `self`/`&self` yang sudah lama boleh tanpa tipe (lihat …
        let ty = if self.eat(&TokenKind::Colon) {
            self.parse_type()?
        } else {
            TypeExpr::Inferred
        };
        // Nilai default: `fn greet(nama = "dunia")` atau `fn f(x: int = 1)`.
        let default = if self.eat(&TokenKind::Eq) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        let lifetime = if self.at_ident_starting_with_tick() {
            Some(self.advance_as_string())
        } else {
            None
        };
        if is_variadic && default.is_some() {
            return Err(self.error(
                "parameter pengumpul (`sisa...`) tidak boleh punya nilai default — \
                 kalau tidak ada argumen tersisa ia sudah berisi array kosong"
                    .to_string()));
        }
        Ok(FnParam { is_variadic, is_mut, borrow, pattern, ty, default, lifetime, span: self.merge_span(lo) })
    }

    pub(crate) fn at_ident_starting_with_tick(&self) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(s) if s.starts_with('\''))
    }

    pub(crate) fn advance_as_string(&mut self) -> String {
        match self.advance().kind.clone() {
            TokenKind::Ident(s) => s,
            _ => String::new(),
        }
    }

    // ── Struct declaration ────────────────────────────────────────────────

    pub(crate) fn parse_struct_decl(&mut self, vis: Visibility) -> Result<StructDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwStruct)?;
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params()?;
        // Pewarisan opsional: `struct Dog: Animal { ...
        let parent = if self.at_exact(&TokenKind::Colon) {
            self.advance();
            Some(self.expect_ident()?)
        } else { None };
        let where_clause = self.parse_where_clause()?;

        // ── Struct tuple: `struct P(int, int)` ────────────────────────────
        if self.at_exact(&TokenKind::LParen) {
            self.advance();
            let mut fields = Vec::new();
            let mut n = 0usize;
            while !self.at_exact(&TokenKind::RParen) {
                let f_lo = self.span_lo();
                let f_vis = self.parse_visibility();
                let ty = self.parse_type()?;
                fields.push(StructField {
                    // Field struct tuple selalu `pub`: ia diakses lewat
                    // posisinya (`p._0`), dan sebuah posisi yang privat
                    // tidak bisa dipakai siapa pun termasuk pembuatnya.
                    vis: if matches!(f_vis, Visibility::Private) { Visibility::Public } else { f_vis },
                    name: format!("_{}", n),
                    ty,
                    default: None,
                    span: self.merge_span(f_lo),
                });
                n += 1;
                if !self.eat(&TokenKind::Comma) { break; }
            }
            self.expect(&TokenKind::RParen)?;
            let span = self.merge_span(lo);
            self.tambahan_item.push(Item::Fn(konstruktor_tuple(&name, &fields, vis.clone(), span)));
            return Ok(StructDecl { vis, name, generics, parent, where_clause, fields, span });
        }

        self.expect(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) {
            fields.push(self.parse_struct_field()?);
            self.eat(&TokenKind::Comma);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(StructDecl { vis, name, generics, parent, where_clause, fields, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_struct_field(&mut self) -> Result<StructField, ParseError> {
        let lo = self.span_lo();
        let vis = self.parse_visibility();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        let default = if self.eat(&TokenKind::Eq) {
            Some(Box::new(self.parse_expr()?))
        } else { None };
        Ok(StructField { vis, name, ty, default, span: self.merge_span(lo) })
    }

    // ── Enum declaration ──────────────────────────────────────────────────

    pub(crate) fn parse_enum_decl(&mut self, vis: Visibility) -> Result<EnumDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwEnum)?;
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params()?;
        let where_clause = self.parse_where_clause()?;
        self.expect(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) {
            variants.push(self.parse_enum_variant()?);
            self.eat(&TokenKind::Comma);
            // A struct-variant (`Name { field: Type, ...
            self.skip_semis();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(EnumDecl { vis, name, generics, where_clause, variants, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_enum_variant(&mut self) -> Result<EnumVariant, ParseError> {
        let lo = self.span_lo();
        let name = self.expect_ident()?;
        if self.at_exact(&TokenKind::Eq) {
            // Sebelum ini pesannya "harusnya ada nama di sini, yang ketemu malah `=`" —
            // benar tentang tokennya dan diam soal apa yang sedang dicoba.
            return Err(self.error(format!(
                "diskriminan enum (`{} = ...`) belum ada di JRG. Petakan variannya ke \
                 angka dengan `match`: `fn kode(s) {{ return match s {{ {}::{} => 5, ... }} }}` \
                 — lebih panjang satu baris, dan pemetaannya jadi terbaca di satu tempat",
                name, "E", name)));
        }
        let kind = if self.at_exact(&TokenKind::LParen) {
            self.advance();
            let mut types = Vec::new();
            while !self.at_exact(&TokenKind::RParen) {
                types.push(self.parse_type()?);
                if !self.eat(&TokenKind::Comma) { break; }
            }
            self.expect(&TokenKind::RParen)?;
            EnumVariantKind::Tuple(types)
        } else if self.at_exact(&TokenKind::LBrace) {
            self.advance();
            let mut fields = Vec::new();
            while !self.at_exact(&TokenKind::RBrace) {
                fields.push(self.parse_struct_field()?);
                self.eat(&TokenKind::Comma);
            }
            self.expect(&TokenKind::RBrace)?;
            EnumVariantKind::Struct(fields)
        } else {
            EnumVariantKind::Unit
        };
        Ok(EnumVariant { name, kind, span: self.merge_span(lo) })
    }

    // ── Trait declaration ─────────────────────────────────────────────────

    pub(crate) fn parse_trait_decl(&mut self, vis: Visibility) -> Result<TraitDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwTrait)?;
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params()?;
        let bounds = if self.eat(&TokenKind::Colon) {
            self.parse_trait_bounds()?
        } else { Vec::new() };
        let where_clause = self.parse_where_clause()?;
        self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) {
            let v = self.parse_visibility();
            let fd = self.parse_fn_decl_dalam_trait(v)?;
            // "Punya badan?", bukan "badannya blok?".
            let punya_badan = !matches!(*fd.body, FnBody::Abstrak);
            items.push(if punya_badan { TraitItem::DefaultMethod(fd) } else { TraitItem::Method(fd) });
            self.skip_semis();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(TraitDecl { vis, name, generics, bounds, where_clause, items, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_trait_bounds(&mut self) -> Result<Vec<TraitBound>, ParseError> {
        let mut bounds = vec![self.parse_trait_bound()?];
        while self.eat(&TokenKind::Plus) {
            bounds.push(self.parse_trait_bound()?);
        }
        Ok(bounds)
    }

    /// Satu bound pada parameter tipe (`T: Bound`).
    pub(crate) fn parse_trait_bound(&mut self) -> Result<TraitBound, ParseError> {
        // `??` ikut, bukan cuma `?`.
        if self.at_exact(&TokenKind::Question) || self.at_exact(&TokenKind::QuestionQuestion) {
            return Err(self.error(
                "bound negatif (`?Bound`) belum didukung — JRG belum menegakkan \
                 trait bound sama sekali, jadi menuliskannya cuma akan terlihat \
                 seperti batasan yang tidak pernah diperiksa. Hapus `?`-nya, atau \
                 hapus seluruh bound-nya kalau memang tidak ada yang mau dibatasi"
                    .to_string(),
            ));
        }
        if self.at_ident_starting_with_tick() {
            Ok(TraitBound::Lifetime(self.advance_as_string()))
        } else {
            Ok(TraitBound::Trait(self.parse_type()?))
        }
    }

    // ── Impl declaration ──────────────────────────────────────────────────

    pub(crate) fn parse_impl_decl(&mut self) -> Result<ImplDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwImpl)?;
        let generics = self.parse_generic_params()?;
        let first_type = self.parse_type()?;
        let (self_type, trait_ref) = if self.eat(&TokenKind::KwFor) {
            (self.parse_type()?, Some(first_type))
        } else {
            (first_type, None)
        };
        let where_clause = self.parse_where_clause()?;
        // Method atas tipe bawaan tidak pernah bisa dipanggil: method penerima bawaan selalu
        // bawaan (Fase RUANG). Dulu diterima lalu `5.ganda()` dijawab "tidak punya method".
        let bawaan = match &self_type {
            TypeExpr::Named(nt) => nt.package.is_none() && matches!(nt.name.as_str(),
                "array" | "tuple" | "unit" | "function" | "closure" | "native_fn"
                | "canvas" | "channel"),
            TypeExpr::Primitive(_) | TypeExpr::Array(_) | TypeExpr::Slice(_)
            | TypeExpr::Chan(_) | TypeExpr::Fn(_) | TypeExpr::Tuple(_) | TypeExpr::Pointer(_) => true,
            _ => false,
        };
        if bawaan {
            let t = super::nama_tipe(&self_type);
            return Err(ParseError {
                msg: format!(
                    "`impl {}` tidak berpengaruh — method tidak bisa ditambahkan ke tipe bawaan `{}`\n  \
                     bantuan: tulis fungsi biasa, mis. `fn ganda(x: {}) {{ … }}`, lalu panggil `ganda(nilai)`\n  \
                     catatan: dulu bentuk ini diterima, lalu method-nya tidak pernah bisa dipanggil",
                    t, t, t),
                span: self.merge_span(lo),
            });
        }
        self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) {
            let v = self.parse_visibility();
            let item = match &self.current().kind {
                TokenKind::KwConst => ImplItem::Const(self.parse_const_decl(v)?),
                // Sama alasannya dengan `type` tingkat atas: diurai lalu dibuang tanpa
                // pesan.
                TokenKind::KwType  => {
                    let ta = self.parse_type_alias(v)?;
                    return Err(ParseError {
                        msg: format!(
                            "tipe terkait `type {}` di dalam `impl` belum punya arti di JRG\n\
                             bantuan: pakai tipe yang di sebelah kanannya langsung\n\
                             catatan: sebelum ini ia diurai lalu dibuang tanpa pesan",
                            ta.name),
                        span: ta.span,
                    });
                }
                _ => ImplItem::Method(self.parse_fn_decl_metode(v)?),
            };
            items.push(item);
            self.skip_semis();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(ImplDecl { generics, self_type, trait_ref, where_clause, items,
                      span: self.merge_span(lo) })
    }

    // ── Type alias, const, var ────────────────────────────────────────────

    pub(crate) fn parse_type_alias(&mut self, vis: Visibility) -> Result<TypeAlias, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwType)?;
        let name = self.expect_ident()?;
        let generics = self.parse_generic_params()?;
        self.expect(&TokenKind::Eq)?;
        let ty = self.parse_type()?;
        self.expect_semi_or_closer()?;
        Ok(TypeAlias { vis, name, generics, ty, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_const_decl(&mut self, vis: Visibility) -> Result<ConstDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwConst)?;
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        self.expect(&TokenKind::Eq)?;
        let value = Box::new(self.parse_expr()?);
        self.eat(&TokenKind::Semi);
        Ok(ConstDecl { vis, name, ty, value, kata: "const", span: self.merge_span(lo) })
    }

    /// `let NAMA = expr` di TINGKAT MODUL.
    pub(crate) fn parse_let_item(&mut self, vis: Visibility) -> Result<Item, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwLet)?;
        let bisa_ubah = self.eat(&TokenKind::KwMut);
        // Pola (`let [a, b] = …`) sengaja tidak didukung di sini: bentuk item
        // tingkat-atas cuma memuat satu nama, dan menerima pola berarti sebagian …
        if !matches!(self.current().kind, TokenKind::Ident(_)) {
            return Err(self.error(
                "`let` di tingkat modul cuma menerima satu nama, bukan pola — \
                 deklarasikan tiap namanya sendiri-sendiri, atau pindahkan \
                 destructuring-nya ke dalam sebuah fungsi".to_string(),
            ));
        }
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        self.expect(&TokenKind::Eq)?;
        let value = Box::new(self.parse_expr()?);
        self.eat(&TokenKind::Semi);
        let span = self.merge_span(lo);
        Ok(if bisa_ubah {
            Item::Var(VarDeclItem { name, ty, value: Some(value), span })
        } else {
            Item::Const(ConstDecl { vis, name, ty, value, kata: "let", span })
        })
    }

    pub(crate) fn parse_var_decl_item(&mut self, _vis: Visibility) -> Result<VarDeclItem, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwVar)?;
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        let value = if self.eat(&TokenKind::Eq) { Some(Box::new(self.parse_expr()?)) } else { None };
        self.eat(&TokenKind::Semi);
        Ok(VarDeclItem { name, ty, value, span: self.merge_span(lo) })
    }

    // ── Mod, extern, use ─────────────────────────────────────────────────

    pub(crate) fn parse_mod_decl(&mut self, vis: Visibility) -> Result<ModDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwMod)?;
        let name = self.expect_ident()?;
        let items = if self.eat(&TokenKind::LBrace) {
            let mut its = Vec::new();
            while !self.at_exact(&TokenKind::RBrace) {
                match self.parse_item() {
                    Ok(i) => its.push(i),
                    Err(e) => { self.errors.push(e); self.synchronize(); }
                }
                self.skip_semis();
            }
            self.expect(&TokenKind::RBrace)?;
            Some(its)
        } else {
            self.eat(&TokenKind::Semi);
            None
        };
        Ok(ModDecl { vis, name, items, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_extern_block(&mut self) -> Result<ExternBlock, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwExtern)?;
        let abi = match self.current().kind.clone() {
            TokenKind::StringLit(s) => { self.advance(); s }
            _ => "C".to_string(),
        };
        // Optional second string literal: the shared library to dlopen at runtime,
        // e.g.
        let lib = match self.current().kind.clone() {
            TokenKind::StringLit(s) => { self.advance(); Some(s) }
            _ => None,
        };
        self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();
        while !self.at_exact(&TokenKind::RBrace) {
            items.push(self.parse_extern_item()?);
            self.skip_semis();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(ExternBlock { abi, lib, items, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_extern_item(&mut self) -> Result<ExternItem, ParseError> {
        let lo = self.span_lo();
        let vis = self.parse_visibility();
        match &self.current().kind {
            TokenKind::KwFn => {
                self.advance();
                // Nama boleh berbentuk path (`math::mul`): sebuah fungsi C++ hampir selalu
                // ada di dalam namespace, dan tanpa ini blok `extern "C++"` cuma bisa …
                let mut name = self.expect_ident()?;
                while self.at_exact(&TokenKind::ColonColon) {
                    self.advance();
                    let ruas = self.expect_ident()?;
                    name = format!("{}::{}", name, ruas);
                }
                self.expect(&TokenKind::LParen)?;
                let mut params = Vec::new();
                while !self.at_exact(&TokenKind::RParen) {
                    let pname = if matches!(self.current().kind, TokenKind::Ident(_))
                                && self.peek().kind == TokenKind::Colon {
                        let n = self.expect_ident()?;
                        self.advance(); // :
                        Some(n)
                    } else { None };
                    let ty = self.parse_type()?;
                    params.push(ExternParam { name: pname, ty });
                    if !self.eat(&TokenKind::Comma) { break; }
                }
                self.expect(&TokenKind::RParen)?;
                let ret = if self.eat(&TokenKind::Arrow) { Some(self.parse_type()?) } else { None };
                self.expect_semi_or_closer()?;
                Ok(ExternItem::Fn { vis, name, params, ret, span: self.merge_span(lo) })
            }
            TokenKind::KwStatic => {
                self.advance();
                let is_mut = self.eat(&TokenKind::KwMut);
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let ty = self.parse_type()?;
                self.expect_semi_or_closer()?;
                Ok(ExternItem::Static { vis, is_mut, name, ty, span: self.merge_span(lo) })
            }
            other => Err(self.error(format!("{} tidak bisa berdiri di dalam blok `extern` — yang boleh cuma deklarasi `fn`", other.tulisan()))),
        }
    }

    pub(crate) fn parse_use_decl(&mut self) -> Result<UseDecl, ParseError> {
        let lo = self.span_lo();
        self.expect(&TokenKind::KwUse)?;
        let path = self.parse_use_path()?;
        self.eat(&TokenKind::Semi);
        Ok(UseDecl { path, span: self.merge_span(lo) })
    }

    pub(crate) fn parse_use_path(&mut self) -> Result<UsePath, ParseError> {
        let mut segments = Vec::new();
        segments.push(self.expect_ident()?);
        while self.at_exact(&TokenKind::ColonColon) {
            self.advance();
            match &self.current().kind {
                TokenKind::Star => {
                    self.advance();
                    return Ok(UsePath::Glob(segments));
                }
                TokenKind::LBrace => {
                    self.advance();
                    let mut children = Vec::new();
                    while !self.at_exact(&TokenKind::RBrace) {
                        children.push(self.parse_use_path()?);
                        if !self.eat(&TokenKind::Comma) { break; }
                    }
                    self.expect(&TokenKind::RBrace)?;
                    return Ok(UsePath::Group(segments, children));
                }
                TokenKind::Ident(_) => {
                    segments.push(self.expect_ident()?);
                }
                _ => break,
            }
        }
        if self.eat(&TokenKind::KwAs) {
            let alias = self.expect_ident()?;
            Ok(UsePath::Aliased(segments, alias))
        } else {
            Ok(UsePath::Simple(segments))
        }
    }

    // ── Generics & where clause ───────────────────────────────────────────

    pub(crate) fn parse_generic_params(&mut self) -> Result<GenericParams, ParseError> {
        if !self.at_exact(&TokenKind::Lt) {
            return Ok(GenericParams::default());
        }
        let lo = self.span_lo();
        self.advance(); // <
        let mut params = Vec::new();
        while !self.at_exact(&TokenKind::Gt) {
            let plo = self.span_lo();
            if self.at_exact(&TokenKind::KwConst) {
                return Err(self.error(concat!(
                    "generik const (`<const N: int>`) belum ada di JRG, dan tidak akan ",
                    "berarti banyak selama anotasi tipe di sini tidak ditegakkan: ",
                    "`let x: int = \"teks\"` diterima. Serahkan angkanya sebagai ",
                    "parameter biasa").to_string()));
            }
            let name = self.expect_ident()?;
            let bounds = if self.eat(&TokenKind::Colon) {
                self.parse_trait_bounds()?
            } else { Vec::new() };
            params.push(GenericParam { name, bounds, span: self.merge_span(plo) });
            if !self.eat(&TokenKind::Comma) { break; }
        }
        self.expect(&TokenKind::Gt)?;
        Ok(GenericParams { params, span: Some(self.merge_span(lo)) })
    }

    pub(crate) fn parse_where_clause(&mut self) -> Result<Vec<WhereItem>, ParseError> {
        if !self.at_exact(&TokenKind::KwWhere) { return Ok(Vec::new()); }
        self.advance();
        let mut items = Vec::new();
        loop {
            if matches!(&self.current().kind, TokenKind::LBrace | TokenKind::Semi | TokenKind::Eof) {
                break;
            }
            let lo = self.span_lo();
            let ty = self.parse_type()?;
            self.expect(&TokenKind::Colon)?;
            let bounds = self.parse_trait_bounds()?;
            items.push(WhereItem { ty, bounds, span: self.merge_span(lo) });
            if !self.eat(&TokenKind::Comma) { break; }
        }
        Ok(items)
    }
}

/// Parameter wajib SESUDAH parameter ber-default membuat default itu tak pernah
/// terpakai (argumen diisi berurutan; argumen bernama tidak bisa melompat), dan
/// `--vm` dulu menjawab `nil` untuk parameter wajib yang tidak diisi.
fn periksa_urutan_default(params: &[FnParam]) -> Result<(), ParseError> {
    let nama = |p: &FnParam, i: usize| match &p.pattern {
        Pattern::Ident { name, .. } => format!("`{}`", name),
        _ => format!("ke-{}", i + 1),
    };
    let mut ber_default: Option<(usize, &FnParam)> = None;
    for (i, p) in params.iter().enumerate() {
        if p.is_variadic { continue; }
        match (&p.default, ber_default) {
            (Some(_), None) => ber_default = Some((i, p)),
            (None, Some((j, d))) => {
                let (wajib, bawaan) = (nama(p, i), nama(d, j));
                return Err(ParseError {
                    msg: format!(
                        "parameter {} wajib diisi tapi ditulis sesudah {} yang punya nilai default — \
                         default {} tidak akan pernah terpakai: argumen diisi berurutan, dan \
                         argumen bernama tidak bisa melompati parameter\n\
                         bantuan: taruh {} sebelum {}, atau beri {} nilai default juga",
                        wajib, bawaan, bawaan, wajib, bawaan, wajib),
                    span: p.span,
                });
            }
            _ => {}
        }
    }
    Ok(())
}
