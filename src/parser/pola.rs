// src/parser/pola.rs — pola: `match`, `let`, parameter

use super::*;

impl Parser {
    // ── Pattern parsing ───────────────────────────────────────────────────

    pub(crate) fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let lo = self.span_lo();
        if self.at_exact(&TokenKind::LBrace) {
            return Err(self.error(concat!(
                "destrukturisasi dict (`let {x, y} = d`) belum ada di JRG. Ambil per ",
                "kunci: `let x = d[\"x\"]` — dan itu juga yang membuat kunci yang ",
                "hilang punya pesannya sendiri, bukan satu kegagalan untuk semuanya")
                .to_string()));
        }
        // `nama @ pola`.
        if let TokenKind::Ident(nama) = &self.current().kind {
            if matches!(self.peek_kind(1), Some(TokenKind::At)) {
                let nama = nama.clone();
                self.advance(); // nama
                self.advance(); // @
                let inner = Box::new(self.parse_pattern()?);
                return Ok(Pattern::Binding { name: nama, inner, span: self.merge_span(lo) });
            }
        }
        let first = self.parse_single_pattern()?;
        // Check for Or-pattern: `pat | pat`
        if self.at_exact(&TokenKind::Or) {
            let mut pats = vec![first];
            while self.eat(&TokenKind::Or) {
                pats.push(self.parse_single_pattern()?);
            }
            return Ok(Pattern::Or { pats, span: self.merge_span(lo) });
        }
        Ok(first)
    }

    pub(crate) fn parse_single_pattern(&mut self) -> Result<Pattern, ParseError> {
        let lo = self.span_lo();
        match &self.current().kind {
            // `self` as pattern (receiver parameter)
            TokenKind::KwSelf => {
                let span = self.merge_span(lo);
                self.advance();
                Ok(Pattern::Ident { name: "self".to_string(), is_mut: false, span })
            }
            TokenKind::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pattern::Wildcard(self.merge_span(lo)))
            }
            TokenKind::KwMut => {
                self.advance();
                let name = self.expect_ident()?;
                Ok(Pattern::Ident { name, is_mut: true, span: self.merge_span(lo) })
            }
            TokenKind::Amp => {
                self.advance();
                let is_mut = self.eat(&TokenKind::KwMut);
                let inner = Box::new(self.parse_pattern()?);
                Ok(Pattern::Ref { inner, is_mut, span: self.merge_span(lo) })
            }
            TokenKind::LParen => {
                self.advance();
                let mut elems = Vec::new();
                let mut koma = false;
                while !self.at_exact(&TokenKind::RParen) {
                    elems.push(self.parse_pattern()?);
                    if !self.eat(&TokenKind::Comma) { break; }
                    koma = true;
                }
                self.expect(&TokenKind::RParen)?;
                // `(p)` tanpa koma mengelompokkan, seperti di ekspresi: dulu ia tuple satu
                // elemen, jadi `match 3 { (3) => … }` dan `n @ (1 | 3)` tidak pernah cocok.
                if elems.len() == 1 && !koma {
                    return Ok(elems.pop().unwrap());
                }
                Ok(Pattern::Tuple { elems, span: self.merge_span(lo) })
            }
            // Pola array: `[a, b]`, `[]`, `[kepala, ...ekor]`, `[a, ..., z]`.
            TokenKind::LBracket => {
                self.advance();
                let mut before: Vec<Pattern> = Vec::new();
                let mut after:  Vec<Pattern> = Vec::new();
                let mut rest: Option<Option<String>> = None;
                while !self.at_exact(&TokenKind::RBracket) {
                    if self.at_exact(&TokenKind::DotDotDot) {
                        if rest.is_some() {
                            return Err(self.error(
                                "pola array cuma boleh punya satu `...` — dengan dua atau lebih, \
                                 tidak ada cara menentukan elemen mana masuk yang mana".to_string(),
                            ));
                        }
                        self.advance();
                        // `...nama` mengikat sisanya, `...` saja membuangnya.
                        let nama = match &self.current().kind {
                            TokenKind::Ident(n) if n != "_" => {
                                let n = n.clone();
                                self.advance();
                                Some(n)
                            }
                            TokenKind::Ident(_) => { self.advance(); None }
                            _ => None,
                        };
                        rest = Some(nama);
                    } else {
                        let p = self.parse_pattern()?;
                        if rest.is_some() { after.push(p); } else { before.push(p); }
                    }
                    if !self.eat(&TokenKind::Comma) { break; }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Pattern::Array { before, rest, after, span: self.merge_span(lo) })
            }
            // Open-ended range with no lower bound: `..5`, `..=5`, or a bare `..`
            // (matches absolutely anything — `_` reads better for that, but this is …
            TokenKind::DotDot | TokenKind::DotDotEq => {
                let inclusive = matches!(self.current().kind, TokenKind::DotDotEq);
                self.advance();
                let hi = if self.at_pattern_bound_start() {
                    Some(Box::new(self.parse_literal_pattern_bound()?))
                } else { None };
                Ok(Pattern::Range { lo: None, hi, inclusive, span: self.merge_span(lo) })
            }
            // Negative numeric literal pattern, e.g. `-1`, `-3.5`, and
            // possibly the lower bound of a range: `-5..10`.
            TokenKind::Minus => {
                self.advance();
                let lit = match self.current().kind.clone() {
                    TokenKind::IntLit(v) => { let sp = self.span_lo(); self.advance(); self.lit_pola_int(v, true, sp)? }
                    TokenKind::FloatLit(v) => { self.advance(); LitKind::Float(-v) }
                    other => return Err(self.error(
                        format!("sesudah `-` di dalam pola harusnya ada angka, yang ketemu malah {}", other.tulisan()))),
                };
                let first = Pattern::Literal { lit, span: self.merge_span(lo) };
                self.parse_pattern_range_tail(first, lo)
            }
            TokenKind::IntLit(v) => {
                let v = *v;
                self.advance();
                let lit = self.lit_pola_int(v, false, self.merge_span(lo))?;
                let first = Pattern::Literal { lit, span: self.merge_span(lo) };
                self.parse_pattern_range_tail(first, lo)
            }
            TokenKind::FloatLit(v) => {
                let v = *v;
                self.advance();
                let first = Pattern::Literal { lit: LitKind::Float(v), span: self.merge_span(lo) };
                self.parse_pattern_range_tail(first, lo)
            }
            TokenKind::RuneLit(c) => {
                let c = *c;
                self.advance();
                let first = Pattern::Literal { lit: LitKind::Rune(c), span: self.merge_span(lo) };
                self.parse_pattern_range_tail(first, lo)
            }
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Pattern::Literal { lit: LitKind::Str(s), span: self.merge_span(lo) })
            }
            TokenKind::BoolLit(b) => {
                let b = *b;
                self.advance();
                Ok(Pattern::Literal { lit: LitKind::Bool(b), span: self.merge_span(lo) })
            }
            TokenKind::NilLit => {
                self.advance();
                Ok(Pattern::Literal { lit: LitKind::Nil, span: self.merge_span(lo) })
            }
            TokenKind::Ident(_) => {
                let name = self.expect_ident()?;
                // Could be: bare ident pattern, a qualified enum variant (`Type::Variant`,
                // optionally with a payload pattern), a *naked* tuple-style variant pattern …
                if self.at_exact(&TokenKind::ColonColon) {
                    self.advance();
                    let mut paket = name;
                    let mut modul: Option<String> = None;
                    let mut variant = self.expect_ident()?;
                    // Tiga segmen sah di posisi EKSPRESI, jadi sah di POLA
                    // juga. Segmen modulnya digeser keluar: tipenya sendiri
                    // memang global sesudah `import`.
                    while self.at_exact(&TokenKind::ColonColon) {
                        self.advance();
                        modul = Some(std::mem::replace(&mut paket, variant));
                        variant = self.expect_ident()?;
                    }
                    let ty = TypeExpr::Named(NamedType {
                        package: Some(paket), modul, name: variant.clone(), args: Vec::new(), span: lo });
                    if self.at_exact(&TokenKind::LParen) {
                        self.advance();
                        let mut elems = Vec::new();
                        while !self.at_exact(&TokenKind::RParen) {
                            elems.push(self.parse_pattern()?);
                            if !self.eat(&TokenKind::Comma) { break; }
                        }
                        self.expect(&TokenKind::RParen)?;
                        let inner = match elems.len() {
                            0 => None,
                            1 => Some(Box::new(elems.into_iter().next().unwrap())),
                            _ => {
                                let tspan = self.merge_span(lo);
                                Some(Box::new(Pattern::Tuple { elems, span: tspan }))
                            }
                        };
                        Ok(Pattern::Enum { ty, variant, inner, span: self.merge_span(lo) })
                    } else if self.at_exact(&TokenKind::LBrace) {
                        // Qualified struct-variant pattern, e.g.
                        self.advance();
                        let mut fields = Vec::new();
                        while !self.at_exact(&TokenKind::RBrace) {
                            // `..` ("ignore the rest of the fields") is accepted for idiomatic-looking
                            // patterns, though it's actually a no-op: a struct/ struct-variant pattern …
                            if self.at_exact(&TokenKind::DotDot) {
                                self.advance();
                                break;
                            }
                            let flo = self.span_lo();
                            let fname = self.expect_ident()?;
                            let fpat = if self.eat(&TokenKind::Colon) {
                                self.parse_pattern()?
                            } else {
                                Pattern::Ident { name: fname.clone(), is_mut: false, span: self.merge_span(flo) }
                            };
                            fields.push(FieldPat { name: fname, pat: fpat, span: self.merge_span(flo) });
                            if !self.eat(&TokenKind::Comma) { break; }
                        }
                        self.expect(&TokenKind::RBrace)?;
                        Ok(Pattern::Struct { ty, fields, span: self.merge_span(lo) })
                    } else {
                        Ok(Pattern::Enum { ty, variant, inner: None, span: self.merge_span(lo) })
                    }
                } else if self.at_exact(&TokenKind::LParen) {
                    self.advance();
                    let mut elems = Vec::new();
                    while !self.at_exact(&TokenKind::RParen) {
                        elems.push(self.parse_pattern()?);
                        if !self.eat(&TokenKind::Comma) { break; }
                    }
                    self.expect(&TokenKind::RParen)?;
                    let ty = TypeExpr::Named(NamedType {
                        package: None, modul: None, name: name.clone(), args: Vec::new(), span: lo });
                    let inner = match elems.len() {
                        0 => None,
                        1 => Some(Box::new(elems.into_iter().next().unwrap())),
                        _ => {
                            let tspan = self.merge_span(lo);
                            Some(Box::new(Pattern::Tuple { elems, span: tspan }))
                        }
                    };
                    Ok(Pattern::Enum { ty, variant: name, inner, span: self.merge_span(lo) })
                } else if self.at_exact(&TokenKind::LBrace) {
                    self.advance();
                    let ty = TypeExpr::Named(NamedType {
                        package: None, modul: None, name: name.clone(), args: Vec::new(), span: lo });
                    let mut fields = Vec::new();
                    while !self.at_exact(&TokenKind::RBrace) {
                        // See the comment on the qualified struct-variant
                        // pattern's identical `..` handling above.
                        if self.at_exact(&TokenKind::DotDot) {
                            self.advance();
                            break;
                        }
                        let flo = self.span_lo();
                        let fname = self.expect_ident()?;
                        let fpat = if self.eat(&TokenKind::Colon) {
                            self.parse_pattern()?
                        } else {
                            Pattern::Ident { name: fname.clone(), is_mut: false, span: self.merge_span(flo) }
                        };
                        fields.push(FieldPat { name: fname, pat: fpat, span: self.merge_span(flo) });
                        if !self.eat(&TokenKind::Comma) { break; }
                    }
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Pattern::Struct { ty, fields, span: self.merge_span(lo) })
                } else {
                    Ok(Pattern::Ident { name, is_mut: false, span: self.merge_span(lo) })
                }
            }
            other => Err(self.error(format!("harusnya ada pola di sini, yang ketemu malah {}", other.tulisan()))),
        }
    }

    /// After parsing a literal pattern that could serve as a range's lower bound,
    /// checks for `..` / `..=`; if present, parses the upper bound (itself …
    pub(crate) fn parse_pattern_range_tail(&mut self, first: Pattern, lo: Span) -> Result<Pattern, ParseError> {
        let inclusive = if self.at_exact(&TokenKind::DotDotEq) {
            self.advance();
            true
        } else if self.at_exact(&TokenKind::DotDot) {
            self.advance();
            false
        } else {
            return Ok(first);
        };
        let hi = if self.at_pattern_bound_start() {
            Some(Box::new(self.parse_literal_pattern_bound()?))
        } else { None };
        Ok(Pattern::Range { lo: Some(Box::new(first)), hi, inclusive, span: self.merge_span(lo) })
    }

    /// Whether the current token can start a literal range bound: a
    /// number (optionally negative) or a rune.
    pub(crate) fn at_pattern_bound_start(&self) -> bool {
        matches!(self.current().kind,
            TokenKind::IntLit(_) | TokenKind::FloatLit(_) | TokenKind::RuneLit(_) | TokenKind::Minus)
    }

    /// Literal integer di pola: positif sampai `i128::MAX`, negatif sampai `-2^127` (disimpan
    /// sebagai pola bit-nya). Di luar itu dulu membungkus: pola `2^127` cocok dengan `i128::MIN`.
    fn lit_pola_int(&self, v: u128, negatif: bool, sp: Span) -> Result<LitKind, ParseError> {
        let batas = if negatif { i128::MAX as u128 + 1 } else { i128::MAX as u128 };
        if v > batas {
            let tulisan = if negatif { format!("-{}", v) } else { v.to_string() };
            return Err(ParseError { msg: crate::interpreter::pesan_literal_luber(&tulisan), span: sp });
        }
        Ok(LitKind::Int(if negatif { (v as i128).wrapping_neg() as u128 } else { v }))
    }

    /// Parses exactly one literal pattern usable as a range bound. Bounds
    /// never themselves continue into a further range or any enum/struct
    /// pattern — they are always a plain (possibly negative) literal.
    pub(crate) fn parse_literal_pattern_bound(&mut self) -> Result<Pattern, ParseError> {
        let lo = self.span_lo();
        match self.current().kind.clone() {
            TokenKind::Minus => {
                self.advance();
                let lit = match self.current().kind.clone() {
                    TokenKind::IntLit(v) => { let sp = self.span_lo(); self.advance(); self.lit_pola_int(v, true, sp)? }
                    TokenKind::FloatLit(v) => { self.advance(); LitKind::Float(-v) }
                    other => return Err(self.error(
                        format!("sesudah `-` di dalam pola jangkauan harusnya ada angka, yang ketemu malah {}", other.tulisan()))),
                };
                Ok(Pattern::Literal { lit, span: self.merge_span(lo) })
            }
            TokenKind::IntLit(v) => {
                self.advance();
                let lit = self.lit_pola_int(v, false, self.merge_span(lo))?;
                Ok(Pattern::Literal { lit, span: self.merge_span(lo) })
            }
            TokenKind::FloatLit(v) => {
                self.advance();
                Ok(Pattern::Literal { lit: LitKind::Float(v), span: self.merge_span(lo) })
            }
            TokenKind::RuneLit(c) => {
                self.advance();
                Ok(Pattern::Literal { lit: LitKind::Rune(c), span: self.merge_span(lo) })
            }
            other => Err(self.error(format!("batas jangkauan harusnya sebuah literal, yang ketemu malah {}", other.tulisan()))),
        }
    }
}
