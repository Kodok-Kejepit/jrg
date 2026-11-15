// src/llvm/situs.rs — lokasi galat saat jalan di biner `--emit-llvm`: panik dan panggilan runtime didahului
// `store i32 N, ptr @jrg_situs`; runtime mencetak blok `-->` situs N yang dirender saat kompilasi oleh
// perender acuan. Tanpa `isi_situs` tabelnya kosong (IR tetap sah, galat tanpa lokasi).

use crate::diag::{Diagnostic, Nada, SourceMap};
use crate::lexer::{BytePos, Span};

const AWALAN: &str = "; jrg-situs";

/// Global tabel situs: sah apa adanya (semua blok kosong); `isi_situs` mengganti isinya.
pub(crate) fn definisi(situs: &[(u32, u32, u32)]) -> String {
    let daftar: Vec<String> = situs.iter().map(|(lo, hi, b)| format!("{}:{}:{}", lo, hi, b)).collect();
    format!(
        "{} {}\n@jrg_situs = external global i32\n@jrg_situs_n = private constant i32 {}\n\
         @jrg_situs_isi = private constant [{} x [3 x i32]] zeroinitializer\n\
         @jrg_situs_teks = private constant [1 x i8] zeroinitializer\n",
        AWALAN, daftar.join(" "), situs.len(), situs.len().max(1))
}

/// Panik di teks yang dirakit tanpa `emit` (pembantu) tidak boleh mewarisi situs lama: diberi nol.
/// Pembantu dispatch method (`md.*`) dikecualikan — pemanggilnya sudah memasang situs panggilan method.
pub(crate) fn nolkan_panik_tanpa_situs<'a>(def: &'a str, lapis: Option<&str>) -> std::borrow::Cow<'a, str> {
    if !def.contains("call void @jrg_panic") { return def.into(); }
    let mut out = String::with_capacity(def.len());
    let mut warisi = false;
    let mut sebelum = "";
    for baris in def.split_inclusive('\n') {
        // Fungsi lapis juga memakai situs pemanggilnya (`m<N>$…`).
        if baris.starts_with("define ") {
            // Daun penjelajah C dan pembanding buram dipanggil dari panggilan `@jrg_*` yang sudah memasang situs.
            warisi = baris.contains("@md.") || baris.contains("@\"md.") || lapis.map_or(false, |l| baris.contains(l))
                || baris.contains("@jrg_sama_daun") || baris.contains("@jrg_tampil_daun") || baris.contains("@sama.buram.");
        }
        if !warisi && baris.trim_start().starts_with("call void @jrg_panic") && !sebelum.contains("ptr @jrg_situs") {
            out.push_str("  store i32 0, ptr @jrg_situs\n");
        }
        out.push_str(baris);
        sebelum = baris;
    }
    out.into()
}

/// Blok lokasi satu situs (sesudah baris judul, sampai `  |` terakhir) dan lebar gutternya.
fn blok(map: &mut SourceMap, lo: u32, hi: u32, berkas: u32) -> (String, usize) {
    map.ensure_module(berkas);
    let d = Diagnostic::error("\u{1}").with_span(Span::new(BytePos(lo), BytePos(hi), berkas)).with_help("\u{2}");
    let t = d.render_dengan_nada(map, Nada::Sopan);
    let (Some(a), Some(k)) = (t.find('\u{1}'), t.find("= bantuan: \u{2}")) else { return (String::new(), 0) };
    let Some(awal_baris) = t[..k].rfind('\n') else { return (String::new(), 0) };
    // Tanpa lokasi perendernya tidak menulis `-->`: blok kosong, bukan lokasi karangan.
    let isi = &t[a + '\u{1}'.len_utf8()..awal_baris];
    if !isi.contains("-->") { return (String::new(), 0); }
    (isi.to_string(), k.saturating_sub(awal_baris + 2))
}

/// Isi tabel situs IR dengan blok lokasi yang dirender dari `map`.
pub fn isi_situs(ir: String, map: &mut SourceMap) -> String {
    let Some(baris) = ir.lines().find(|l| l.starts_with(AWALAN)) else { return ir };
    let situs: Vec<(u32, u32, u32)> = baris[AWALAN.len()..].split_whitespace().filter_map(|s| {
        let mut p = s.split(':').map(|x| x.parse::<u32>().ok());
        Some((p.next()??, p.next()??, p.next()??))
    }).collect();
    if situs.is_empty() { return ir; }
    let mut teks = Vec::new();
    let mut isi = Vec::new();
    for (lo, hi, b) in &situs {
        let (s, lebar) = blok(map, *lo, *hi, *b);
        isi.push(format!("[3 x i32] [i32 {}, i32 {}, i32 {}]", teks.len(), s.len(), lebar));
        teks.extend_from_slice(s.as_bytes());
    }
    teks.push(0);
    let lama_isi = format!("@jrg_situs_isi = private constant [{} x [3 x i32]] zeroinitializer", situs.len());
    let baru_isi = format!("@jrg_situs_isi = private constant [{} x [3 x i32]] [{}]", situs.len(), isi.join(", "));
    let lama_teks = "@jrg_situs_teks = private constant [1 x i8] zeroinitializer";
    let baru_teks = format!("@jrg_situs_teks = private constant [{} x i8] c\"{}\"", teks.len(),
        teks.iter().map(|b| format!("\\{:02X}", b)).collect::<String>());
    let ir = ir.replacen(&lama_isi, &baru_isi, 1).replacen(lama_teks, &baru_teks, 1);
    // Mode `--uji`: nomor baris tiap situs di berkas utama (0 = berkas lain, laporannya tanpa baris).
    let lama_baris = format!("@jrg_uji_baris = private constant [{} x i32] zeroinitializer", situs.len());
    if !ir.contains(&lama_baris) { return ir; }
    let baris: Vec<String> = situs.iter().map(|(lo, hi, b)| {
        let n = if *b == 0 { map.baris(Span::new(BytePos(*lo), BytePos(*hi), 0)).unwrap_or(0) } else { 0 };
        format!("i32 {}", n)
    }).collect();
    ir.replacen(&lama_baris, &format!("@jrg_uji_baris = private constant [{} x i32] [{}]", situs.len(), baris.join(", ")), 1)
}

/// Global tabel baris situs untuk mode `--uji`, diisi `isi_situs`.
pub(crate) fn definisi_baris(n: usize) -> String {
    format!("@jrg_uji_baris = private constant [{} x i32] zeroinitializer\n", n.max(1))
}
