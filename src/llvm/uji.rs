// src/llvm/uji.rs — `--uji` di biner `--emit-llvm`: `main` buatan memanggil tiap `test_*`/`uji_*` lewat
// `jrg_masa_lindung` (galatnya ditangkap, `defer` tetap jalan), lalu melapor dengan kalimat `crate::uji` —
// templatnya dirakit di sini saat kompilasi, runtime cuma menyisipkan sebab, baris, dan hitungan.

use crate::lexer::{BytePos, Span};
use crate::parser::*;

/// Nama intrinsik yang dipancarkan `main` buatan (`::__uji_jalan("nama", fungsi)`, `::__uji_akhir()`).
pub const JALAN: &str = "__uji_jalan";
pub const AKHIR: &str = "__uji_akhir";

/// Salinan `file` yang `main`-nya menjalankan tes (sesudah filter); `Err` = kalimat `uji::tidak_ada_uji`.
pub fn siapkan(file: &ParsedFile, filter: Option<&str>) -> Result<ParsedFile, String> {
    let mut daftar = crate::uji::daftar_uji(file);
    if let Some(f) = filter { daftar.retain(|n| n.contains(f)); }
    if daftar.is_empty() { return Err(crate::uji::tidak_ada_uji()); }
    let sp = Span { lo: BytePos(0), hi: BytePos(0), file_id: file.span.file_id };
    let bawaan = |nama: &str| Box::new(Expr::PathExpr { segments: vec![String::new(), nama.to_string()], span: sp });
    let mut stmts: Vec<Stmt> = daftar.iter().map(|n| Stmt::Expr(Box::new(Expr::Call {
        func: bawaan(JALAN),
        args: vec![Expr::Literal(LitKind::Str(n.clone()), sp), Expr::Ident(n.clone(), sp)],
        span: sp,
    }))).collect();
    stmts.push(Stmt::Return(ReturnStmt {
        values: vec![Expr::Call { func: bawaan(AKHIR), args: Vec::new(), span: sp }], span: sp }));
    let main = FnDecl {
        vis: Visibility::Public, is_async: false, name: "main".to_string(), generics: Default::default(),
        params: Vec::new(),
        ret_type: Some(TypeExpr::Named(NamedType { package: None, modul: None, name: "int".to_string(), args: Vec::new(), span: sp })),
        where_clause: Vec::new(),
        body: std::sync::Arc::new(FnBody::Block(Block { stmts, tail: None, span: sp })),
        span: sp,
    };
    let mut salinan = file.clone();
    salinan.items.retain(|it| !matches!(it, Item::Fn(f) if f.name == "main"));
    salinan.items.push(Item::Fn(main));
    Ok(salinan)
}

/// Penanda di templat: `\u{1}` sebab, `\u{2}` nomor baris (templat berbaris), `\u{3}`/`\u{4}`/`\u{5}` hitungan.
const BARIS_PENANDA: usize = 987_654_321;

/// `(lulus, gagal tanpa baris, gagal berbaris)` untuk satu tes, dari `uji::baris_hasil`.
pub fn templat_hasil(nama: &str) -> (String, String, String) {
    let gagal = Err("\u{1}".to_string());
    (crate::uji::baris_hasil(nama, &Ok(()), None),
     crate::uji::baris_hasil(nama, &gagal, None),
     crate::uji::baris_hasil(nama, &gagal, Some(BARIS_PENANDA)).replacen(&BARIS_PENANDA.to_string(), "\u{2}", 1))
}

/// Rangkuman dengan penanda total `\u{3}`, lulus `\u{4}`, gagal `\u{5}`.
pub fn templat_rangkuman() -> String {
    let (l, g) = (100_000_007usize, 200_000_011usize);
    crate::uji::rangkuman(l, g).replacen(&(l + g).to_string(), "\u{3}", 1)
        .replacen(&l.to_string(), "\u{4}", 1).replacen(&g.to_string(), "\u{5}", 1)
}

use super::codegen::{Codegen, KerjaFn, Val};
use super::types::JrgType;

impl Codegen {
    /// `::__uji_jalan("nama", fungsi)`: jalankan satu tes lewat `jrg_masa_lindung`, lalu laporkan.
    pub(crate) fn uji_jalan(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (Some(Expr::Literal(LitKind::Str(nama), _)), Some(Expr::Ident(tes, _))) = (args.first(), args.get(1)) else {
            return Err(super::galat::tandai(span, "`__uji_jalan` cuma dipancarkan mode `--uji`".to_string()));
        };
        self.pakai_uji = true;
        let badan = format!("jrg.uji.{}", self.uji_ke);
        self.uji_ke += 1;
        self.antre_fn(KerjaFn::Uji { badan: badan.clone(), tes: tes.clone(), span });
        self.ensure_extern("declare i32 @jrg_masa_lindung(ptr, ptr, ptr, ptr)", "jrg_masa_lindung");
        let (pp, pn) = (self.alloca_raw("ujp.", "ptr", Some(8)), self.alloca_raw("ujn.", "i64", Some(8)));
        self.emit(format!("store ptr null, ptr {}", pp));
        self.emit(format!("store i64 0, ptr {}", pn));
        let rc = self.fresh("ujrc.");
        self.emit(format!("{} = call i32 @jrg_masa_lindung(ptr @{}, ptr null, ptr {}, ptr {})",
            rc, super::mangle::llvm_quoted_ident(&format!("{}.badan", badan)), pp, pn));
        let (p, l) = (self.fresh("ujp."), self.fresh("ujl."));
        self.emit(format!("{} = load ptr, ptr {}", p, pp));
        self.emit(format!("{} = load i64, ptr {}", l, pn));
        let (ok, g, gb) = templat_hasil(nama);
        let mut teks = Vec::new();
        for t in [ok, g, gb] {
            let v = self.intern_string(&t);
            let (a, b) = self.split_str(&v);
            teks.push(format!("ptr {}, i64 {}", a, b));
        }
        self.ensure_extern("declare void @jrg_uji_lapor(i32, ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr, ptr)", "jrg_uji_lapor");
        self.emit(format!("call void @jrg_uji_lapor(i32 {}, ptr {}, i64 {}, {}, ptr @jrg_uji_baris, ptr @jrg_situs_n)",
            rc, p, l, teks.join(", ")));
        Ok(Val::unit())
    }

    /// `::__uji_akhir()`: rangkuman; nilainya kode keluar (1 kalau ada yang gagal).
    pub(crate) fn uji_akhir(&mut self) -> Result<Val, String> {
        let v = self.intern_string(&templat_rangkuman());
        let (a, b) = self.split_str(&v);
        self.ensure_extern("declare i64 @jrg_uji_akhir(ptr, i64)", "jrg_uji_akhir");
        let r = self.fresh("ujak.");
        self.emit(format!("{} = call i64 @jrg_uji_akhir(ptr {}, i64 {})", r, a, b));
        Ok(Val { text: r, ty: JrgType::I64 })
    }

    /// `@<badan>.badan(ptr %paket)`: badan tugas yang memanggil tes `tes` (milik berkas utama), lalu
    /// mengirim tanda selesai ke channel `paket[1]` — bentuk yang sama dengan badan adaptor jembatan.
    pub(crate) fn pancarkan_badan_uji(&mut self, badan: &str, tes: &str, span: Span) -> Result<(), String> {
        self.mulai_fungsi_kosong(JrgType::Unit);
        self.unit_kini = 0;
        self.emit("%pc = getelementptr ptr, ptr %paket, i64 1".to_string());
        self.emit("%c = load ptr, ptr %pc".to_string());
        self.call_named_function(tes, &[], None, span)?;
        if !self.terminated {
            self.ensure_extern("declare void @jrg_chan_send(ptr, ptr, i64)", "jrg_chan_send");
            self.emit("call void @jrg_chan_send(ptr %c, ptr null, i64 0)".to_string());
            self.terminate("ret void".to_string());
        }
        let isi = self.take_function_body();
        self.tambah_definisi(format!("define internal void @{}(ptr %paket) {{\n{}}}\n",
            super::mangle::llvm_quoted_ident(&format!("{}.badan", badan)), isi));
        self.selesai_fungsi_kosong();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn templat_memuat_semua_penanda() {
        let (ok, g, gb) = super::templat_hasil("test_a");
        assert_eq!(ok, crate::uji::baris_hasil("test_a", &Ok(()), None));
        assert!(g.contains('\u{1}') && gb.contains('\u{1}') && gb.contains('\u{2}'));
        let r = super::templat_rangkuman();
        assert!(r.contains('\u{3}') && r.contains('\u{4}') && r.contains('\u{5}'), "{}", r);
    }
}
