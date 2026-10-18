// src/jrgm/lint.rs — `jrgm lint`: tiap berkas JRG proyek diperiksa tanpa dijalankan (galat yang
// sama dengan `jrgc --periksa`), lalu aturan peringatan: import yang tidak dipakai, fungsi `_privat`
// yang tidak dipanggil, lokal yang tidak dibaca, berkas uji tanpa fungsi uji, berkas belum rapi.

use std::path::{Path, PathBuf};

use jrgc::diag::{Diagnostic, SourceMap};
use jrgc::lexer::{SourceFile, Span, Token, TokenKind};
use jrgc::parser::{Item, parse};

use super::dependensi::{graf_dependensi_dev, kumpul_jrg, path_dependensi};

/// Hasil satu berkas: jumlah galat dan peringatan yang sudah dicetak.
#[derive(Default)]
struct Hitung { galat: usize, peringatan: usize }

pub(crate) fn cmd_lint(ketat: bool) -> Result<(), String> {
    if let Some((m, r, akar)) = super::bahasa::proyek_kini()? { return super::bahasa::lint(&m, r, &akar); }
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    if !akar.join("jrgm.toml").is_file() {
        return Err("jrgm lint: tidak ada `jrgm.toml` di sini — jalankan dari akar proyek".into());
    }
    let m = super::manifes::Manifest::load(&akar)?;
    let mut jalur = vec![akar.join("src"), akar.clone()];
    jalur.extend(path_dependensi(&graf_dependensi_dev(&akar)?)?);
    let mut berkas = Vec::new();
    kumpul_jrg(&akar.join("src"), &mut berkas);
    let uji_awal = berkas.len();
    kumpul_jrg(&akar.join("tests"), &mut berkas);
    if !berkas.contains(&m.entry) && m.entry.is_file() { berkas.insert(0, m.entry.clone()); }
    if berkas.is_empty() { return Err("jrgm lint: tidak ada berkas .jrg di `src/` maupun `tests/`".into()); }
    let mut total = Hitung::default();
    for (i, f) in berkas.iter().enumerate() {
        let h = periksa_berkas(f, &jalur, f == &m.entry, i >= uji_awal && f.starts_with(akar.join("tests")))?;
        total.galat += h.galat;
        total.peringatan += h.peringatan;
    }
    eprintln!("[jrgm] lint: {} berkas, {} galat, {} peringatan", berkas.len(), total.galat, total.peringatan);
    if total.galat > 0 { return Err(String::new()); }
    if ketat && total.peringatan > 0 {
        return Err(format!("{} peringatan, dan `--ketat` memperlakukannya sebagai galat", total.peringatan));
    }
    Ok(())
}

fn periksa_berkas(f: &Path, jalur: &[PathBuf], entri: bool, berkas_uji: bool) -> Result<Hitung, String> {
    let src = std::fs::read_to_string(f)
        .map_err(|e| format!("jrgm lint: tidak bisa baca `{}`: {}", f.display(), jrgc::host::galat_io(&e)))?;
    let sf = SourceFile { id: 0, name: f.display().to_string(), src: src.clone() };
    let mut peta = SourceMap::with_file(sf.clone());
    let mut h = Hitung::default();
    let Some(token) = jrgc::pipa::lex_dilaporkan(&sf, &peta) else { h.galat += 1; return Ok(h) };
    let (ast, galat) = parse(token.clone(), true);
    if !galat.is_empty() {
        jrgc::pipa::laporkan_galat_parse(&galat, true, &peta);
        h.galat += galat.len();
        return Ok(h);
    }
    let mut cari = vec![f.parent().map(Path::to_path_buf).unwrap_or_default()];
    cari.extend(jalur.iter().cloned());
    if let Err(gs) = jrgc::vm::periksa_dengan_paths(&ast, cari) {
        // Galat di modul lain dilaporkan saat modul itu sendiri diperiksa, bukan dua kali.
        for g in gs.iter().filter(|g| g.span.map_or(true, |s| s.file_id == 0)) {
            jrgc::diag::laporkan_vm(&mut peta, g);
            h.galat += 1;
        }
    }
    let mut peringatan = |pesan: String, span: Option<Span>, bantuan: &str| {
        Diagnostic::warning(pesan).with_span_opt(span).with_help(bantuan).emit_tanpa_sapaan(&peta);
        h.peringatan += 1;
    };
    for imp in &ast.imports {
        let alias = imp.alias.clone().unwrap_or_else(|| jrgc::module::ModuleResolver::module_name(&imp.path));
        if !dipakai_sebagai_awalan(&token, &alias) {
            peringatan(format!("`import \"{}\"` tidak dipakai — tidak ada `{}::` di berkas ini", imp.path, alias),
                Some(imp.span), "hapus baris import-nya");
        }
    }
    for it in &ast.items {
        let Item::Fn(d) = it else { continue };
        let privat = d.name.starts_with('_');
        let bebas = d.name == "main" || jrgc::uji::adalah_uji(&d.name);
        if bebas || !(privat || entri) { continue; }
        if sebutan(&token, &d.name) <= 1 {
            peringatan(format!("fungsi `{}` tidak pernah dipanggil", d.name), span_deklarasi(&token, &d.name),
                if privat { "hapus, atau pakai" } else { "hapus, atau pindahkan ke modul yang memakainya" });
        }
    }
    for (nama, span) in jrgc::parser::lokal_tak_dibaca(&ast) {
        peringatan(format!("`{}` diberi nilai tapi tidak pernah dibaca", nama), Some(span),
            &format!("hapus, atau beri awalan `_` (`_{}`) kalau memang sengaja", nama));
    }
    for (span, n) in bulat_di_tempat_pecahan(&token) {
        peringatan(format!("literal bulat `{}` di tempat bertipe pecahan tetap `int` di interpreter dan VM \
            (anotasi tidak mengubah nilai), dan ditolak `--emit-llvm`", n), Some(span),
            &format!("tulis `{}.0`", n));
    }
    if berkas_uji && jrgc::uji::daftar_uji(&ast).is_empty() {
        peringatan(format!("{} tidak punya satu pun fungsi uji", f.display()), None,
            "`jrgm test` cuma menjalankan fungsi yang namanya diawali `test_` atau `uji_`");
    }
    if matches!(jrgc::fmt::format_sumber(&sf), Ok(rapi) if rapi != src) {
        peringatan(format!("{} belum rapi", f.display()), None, "jalankan `jrgm fmt`");
    }
    Ok(h)
}

fn dipakai_sebagai_awalan(t: &[Token], alias: &str) -> bool {
    t.windows(2).any(|w| matches!(&w[0].kind, TokenKind::Ident(n) if n == alias) && w[1].kind == TokenKind::ColonColon)
}

/// `x: float = 7`, `fn f(k: f64 = 2)`, `y: f32 = -1`: literal bulat yang ditulis langsung di
/// tempat beranotasi pecahan (`let`/`var`/`const`, default field dan parameter).
fn bulat_di_tempat_pecahan(t: &[Token]) -> Vec<(Span, String)> {
    let mut hasil = Vec::new();
    for i in 0..t.len().saturating_sub(3) {
        let pecahan = matches!(&t[i + 1].kind, TokenKind::Ident(n) if n == "float" || n == "f64" || n == "f32");
        if t[i].kind != TokenKind::Colon || !pecahan || t[i + 2].kind != TokenKind::Eq { continue; }
        let (j, minus) = if t[i + 3].kind == TokenKind::Minus { (i + 4, "-") } else { (i + 3, "") };
        if let Some(TokenKind::IntLit(n)) = t.get(j).map(|x| &x.kind) {
            hasil.push((t[j].span, format!("{}{}", minus, n)));
        }
    }
    hasil
}

fn sebutan(t: &[Token], nama: &str) -> usize {
    t.iter().filter(|x| matches!(&x.kind, TokenKind::Ident(n) if n == nama)).count()
}

fn span_deklarasi(t: &[Token], nama: &str) -> Option<Span> {
    t.windows(2).find(|w| w[0].kind == TokenKind::KwFn && matches!(&w[1].kind, TokenKind::Ident(n) if n == nama))
        .map(|w| w[1].span)
}
