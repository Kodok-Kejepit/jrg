// src/parser/tipe.rs — posisi TIPE: `int`, `[]T`, `map[K]V`, `chan T`,
// generik

use super::*;

impl Parser {
    // ── Type parsing ──────────────────────────────────────────────────────

    pub(crate) fn parse_type(&mut self) -> Result<TypeExpr, ParseError> {
        match self.current().kind.clone() {
            TokenKind::Bang => { self.advance(); Ok(TypeExpr::Never) }
            TokenKind::Star => {
                // `awal` HARUS diambil sebelum apa pun dikonsumsi.
                let awal = self.span_lo();
                self.advance();
                let is_mut = self.eat(&TokenKind::KwMut);
                if !is_mut { self.eat(&TokenKind::KwConst); }
                let inner = Box::new(self.parse_type()?);
                Ok(TypeExpr::Pointer(PointerTy { is_mut, inner, span: self.merge_span(awal) }))
            }
            TokenKind::LBracket => {
                self.advance();
                if self.eat(&TokenKind::RBracket) {
                    let elem = Box::new(self.parse_type()?);
                    let lo = self.span_lo();
                    return Ok(TypeExpr::Slice(SliceTy { elem, span: self.merge_span(lo) }));
                }
                let len = Box::new(self.parse_expr()?);
                self.expect(&TokenKind::RBracket)?;
                let elem = Box::new(self.parse_type()?);
                let lo = self.span_lo();
                Ok(TypeExpr::Array(ArrayTy { len, elem, span: self.merge_span(lo) }))
            }
            // `map[K]V` dikenali dari BENTUKNYA, bukan dari sebuah kata kunci:
            // `map` yang tidak diikuti `[` adalah nama biasa, jadi `fn map`
            // dan `let map = ...` tetap bisa ditulis.
            TokenKind::Ident(n) if n == "map"
                && matches!(self.peek_kind(1), Some(TokenKind::LBracket)) => {
                let lo = self.span_lo();
                self.advance();
                self.expect(&TokenKind::LBracket)?;
                let key = Box::new(self.parse_type()?);
                self.expect(&TokenKind::RBracket)?;
                let val = Box::new(self.parse_type()?);
                Ok(TypeExpr::Map(MapTy { key, val, span: self.merge_span(lo) }))
            }
            TokenKind::KwChan => {
                let lo = self.span_lo();
                self.advance();
                let dir = if self.eat(&TokenKind::LArrow) { ChanDir::SendOnly } else { ChanDir::Bidir };
                let elem = Box::new(self.parse_type()?);
                Ok(TypeExpr::Chan(ChanTy { dir, elem, span: self.merge_span(lo) }))
            }
            TokenKind::LArrow => {
                let lo = self.span_lo();
                self.advance();
                self.expect(&TokenKind::KwChan)?;
                let elem = Box::new(self.parse_type()?);
                Ok(TypeExpr::Chan(ChanTy { dir: ChanDir::RecvOnly, elem, span: self.merge_span(lo) }))
            }
            TokenKind::KwFn => {
                let lo = self.span_lo();
                self.advance();
                let generics = self.parse_generic_params()?;
                self.expect(&TokenKind::LParen)?;
                let mut params = Vec::new();
                while !self.at_exact(&TokenKind::RParen) {
                    params.push(self.parse_type()?);
                    if !self.eat(&TokenKind::Comma) { break; }
                }
                self.expect(&TokenKind::RParen)?;
                let ret = if self.eat(&TokenKind::Arrow) {
                    Some(Box::new(self.parse_type()?))
                } else { None };
                Ok(TypeExpr::Fn(FnTy { generics, params, ret, span: self.merge_span(lo) }))
            }
            TokenKind::LParen => {
                self.advance();
                if self.eat(&TokenKind::RParen) { return Ok(TypeExpr::Primitive(PrimTy::Unit)); }
                let mut types = vec![self.parse_type()?];
                while self.eat(&TokenKind::Comma) {
                    if self.at_exact(&TokenKind::RParen) { break; }
                    types.push(self.parse_type()?);
                }
                self.expect(&TokenKind::RParen)?;
                Ok(TypeExpr::Tuple(types))
            }
            // Ownership qualifiers: owned<T>, shared<T>, gc<T>, raw<T>
            TokenKind::KwOwned | TokenKind::KwShared | TokenKind::KwGc | TokenKind::KwRaw => {
                let lo = self.span_lo();
                let qual = match self.advance().kind {
                    TokenKind::KwOwned  => OwnershipQual::Owned,
                    TokenKind::KwShared => OwnershipQual::Shared,
                    TokenKind::KwGc     => OwnershipQual::Gc,
                    _                   => OwnershipQual::Raw,
                };
                self.expect(&TokenKind::Lt)?;
                let inner = Box::new(self.parse_type()?);
                self.expect(&TokenKind::Gt)?;
                Ok(TypeExpr::Ownership(OwnershipTy { qualifier: qual, inner, span: self.merge_span(lo) }))
            }
            TokenKind::Ident(name) => {
                let lo = self.span_lo();
                let prim = match name.as_str() {
                    "_" => { self.advance(); return Ok(TypeExpr::Inferred); }
                    lain => prim_dari_nama(lain),
                };
                if let Some(p) = prim {
                    self.advance();
                    return Ok(TypeExpr::Primitive(p));
                }
                let _tak_terpakai = match "" {
                    _ => (),
                };
                // Named type, possibly with package qualifier and generic args
                self.advance();
                let mut pkg = None;
                let mut tname = name;
                if self.eat(&TokenKind::ColonColon) {
                    pkg = Some(tname);
                    tname = self.expect_ident()?;
                }
                let args = if self.at_exact(&TokenKind::Lt) {
                    self.advance();
                    let mut a = Vec::new();
                    while !self.at_exact(&TokenKind::Gt) {
                        a.push(self.parse_type()?);
                        if !self.eat(&TokenKind::Comma) { break; }
                    }
                    self.expect(&TokenKind::Gt)?;
                    a
                } else { Vec::new() };
                Ok(TypeExpr::Named(NamedType { package: pkg, modul: None, name: tname, args, span: self.merge_span(lo) }))
            }
            other => Err(self.error(format!("harusnya ada tipe di sini, yang ketemu malah {}", other.tulisan()))),
        }
    }
}

/// Nilai awal `var x: T` yang ditulis tanpa `=`, sebagai ekspresi biasa
/// (`[N]T` jadi `[nol; N]`, jadi batas ukurannya ikut aturan yang sama).
/// `None` untuk tipe yang nilai nolnya tidak jelas (struct, enum, Option…).
pub fn nilai_nol(ty: &TypeExpr, span: Span) -> Option<Expr> {
    let lit = |k: LitKind| Some(Expr::Literal(k, span));
    match ty {
        TypeExpr::Primitive(p) => match p {
            PrimTy::I8 | PrimTy::I16 | PrimTy::I32 | PrimTy::I64 | PrimTy::I128 | PrimTy::Isize
            | PrimTy::U8 | PrimTy::U16 | PrimTy::U32 | PrimTy::U64 | PrimTy::U128 | PrimTy::Usize
            | PrimTy::Int | PrimTy::Uint | PrimTy::Byte => lit(LitKind::Int(0)),
            PrimTy::F32 | PrimTy::F64 | PrimTy::F128 => lit(LitKind::Float(0.0)),
            PrimTy::Bool => lit(LitKind::Bool(false)),
            PrimTy::Str | PrimTy::String_ => lit(LitKind::Str(String::new())),
            PrimTy::Rune => lit(LitKind::Rune('\0')),
            PrimTy::C64 | PrimTy::C128 | PrimTy::Unit => None,
        },
        TypeExpr::Array(a) => Some(Expr::ArrayRepeat {
            elem: Box::new(nilai_nol(&a.elem, span)?), count: a.len.clone(), span,
        }),
        TypeExpr::Slice(_) => Some(Expr::Array { elems: Vec::new(), span }),
        TypeExpr::Tuple(ts) if !ts.is_empty() => Some(Expr::Tuple {
            elems: ts.iter().map(|t| nilai_nol(t, span)).collect::<Option<Vec<_>>>()?, span,
        }),
        _ => None,
    }
}
