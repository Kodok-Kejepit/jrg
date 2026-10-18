// src/interpreter/builtin/aritas.rs — pintu jumlah argumen builtin di interpreter.
// Batasnya dari `interpreter::arity`, dibaca lewat IDENTITAS fungsinya (jenisnya),
// jadi alias seperti `parse_int` mewarisi batas `str::to_int` tanpa entri kedua.

use std::sync::OnceLock;

use super::NativeFnKind;
use crate::interpreter::arity::{maks, minimal, pesan_kebanyakan, pesan_kurang, tanda_tangan, PetaFnv};
use crate::interpreter::{Env, Value};
use crate::parser::Expr;

/// `tanda`: tanda tangan `--doc` salah satu nama kelompoknya, untuk baris bantuan.
struct Info { maks: Option<usize>, min: Option<usize>, kanonik: String, tanda: Option<&'static str> }

struct Indeks {
    per_jenis: PetaFnv<NativeFnKind, Info>,
    per_nama: PetaFnv<String, NativeFnKind>,
    /// Nama kanonik SETIAP jenis, berbatas atau tidak — untuk jembatan nilai.
    nama: PetaFnv<NativeFnKind, String>,
}

/// Dibangun SEKALI per proses dari registrasi builtin yang sesungguhnya.
fn indeks() -> &'static Indeks {
    static I: OnceLock<Indeks> = OnceLock::new();
    I.get_or_init(|| {
        let mut e = Env::new();
        e.register_builtins();
        let g = e.globals.lock().unwrap_or_else(|p| p.into_inner());
        let mut kelompok: PetaFnv<NativeFnKind, Vec<String>> = PetaFnv::default();
        let mut per_nama: PetaFnv<String, NativeFnKind> = PetaFnv::default();
        for (n, v) in g.iter() {
            if let Value::NativeFn(k) = v {
                kelompok.entry(k.clone()).or_default().push(n.clone());
                per_nama.insert(n.clone(), k.clone());
            }
        }
        let nama_jenis = kelompok.iter().map(|(k, n)| {
            let kanonik = n.iter().min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            (k.clone(), kanonik.cloned().unwrap_or_default())
        }).collect();
        let per_jenis = kelompok.into_iter().filter_map(|(k, mut nama)| {
            // Batas atas diambil TERBESAR dan batas bawah TERKECIL di kelompoknya:
            // batas yang terlalu ketat menolak program yang benar.
            let m = nama.iter().filter_map(|n| maks(n)).max();
            let b = nama.iter().filter_map(|n| minimal(n)).min();
            if m.is_none() && b.is_none() { return None; }
            nama.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            let tanda = nama.iter().find_map(|n| tanda_tangan(n));
            Some((k, Info { maks: m, min: b, kanonik: nama.swap_remove(0), tanda }))
        }).collect();
        Indeks { per_jenis, per_nama, nama: nama_jenis }
    })
}

/// Jenis builtin interpreter di balik sebuah nama.
pub(crate) fn jenis_nama(n: &str) -> Option<NativeFnKind> {
    indeks().per_nama.get(n).cloned()
}

/// Nama kanonik builtin interpreter `k` (terpendek, lalu abjad), kalau terdaftar.
pub(crate) fn nama_jenis(k: &NativeFnKind) -> Option<&'static str> {
    indeks().nama.get(k).map(|s| s.as_str())
}

/// Nama yang DITULIS di situs panggilan, kalau calleenya memang sebuah nama.
fn nama_callee(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n, _) => Some(n.clone()),
        // `::len` adalah `len` yang sama; awalannya cuma menutup pembayangan.
        Expr::PathExpr { segments, .. } if segments.first().is_some_and(|s| s.is_empty()) =>
            Some(segments[1..].join("::")),
        Expr::PathExpr { segments, .. } => Some(segments.join("::")),
        _ => None,
    }
}

/// Argumen lebih DAN kurang ditolak SEBELUM implementasinya jalan, jadi tak ada badan
/// builtin yang sempat mengarang argumen. Namanya yang DITULIS kalau memang builtin ini;
/// kalau tidak (alias lewat variabel, `terap`, callback HOF), nama kanoniknya.
pub(crate) fn periksa(kind: &NativeFnKind, diberi: usize, callee: Option<&Expr>) -> Result<(), String> {
    // Builtin stdlib VM (lewat jembatan) dibaca dari indeks VM: satu pengelompokan alias.
    let (maks, min, kanonik, tanda) = match kind {
        NativeFnKind::DariStdlibVm(n) => match crate::vm::info_native::info_nama(n) {
            Some(i) => (i.maks, i.min, i.kanonik.as_str(), i.tanda),
            None => return Ok(()),
        },
        k => match indeks().per_jenis.get(k) {
            Some(i) => (i.maks, i.min, i.kanonik.as_str(), i.tanda),
            None => return Ok(()),
        },
    };
    let (lebih, kurang) = (maks.filter(|m| diberi > *m), min.filter(|m| diberi < *m));
    if lebih.is_none() && kurang.is_none() { return Ok(()) }
    let ditulis = callee.and_then(nama_callee).filter(|n| nama_milik(n, kind));
    let nama = ditulis.as_deref().unwrap_or(kanonik);
    Err(match (lebih, kurang) {
        (Some(m), _) => pesan_kebanyakan(nama, m, diberi),
        (None, Some(b)) => pesan_kurang(nama, b, maks, diberi, tanda_tangan(nama).or(tanda)),
        (None, None) => return Ok(()),
    })
}

/// Apakah nama yang ditulis memang merujuk builtin `kind` — bukan variabel pembayang.
fn nama_milik(n: &str, kind: &NativeFnKind) -> bool {
    match kind {
        NativeFnKind::DariStdlibVm(asal) => {
            let a = crate::vm::info_native::alamat_nama(n);
            a.is_some() && a == crate::vm::info_native::alamat_nama(asal)
        }
        k => indeks().per_nama.get(n) == Some(k),
    }
}

/// Batas tiap jenis beserta semua namanya — untuk tes kesepakatan alias.
#[cfg(test)]
pub(crate) fn kelompok_untuk_tes() -> Vec<(Vec<String>, usize)> {
    let ix = indeks();
    let mut v: Vec<(Vec<String>, usize)> = ix.per_jenis.iter().filter_map(|(k, i)| {
        let mut n: Vec<String> = ix.per_nama.iter()
            .filter(|(_, kk)| *kk == k).map(|(n, _)| n.clone()).collect();
        n.sort();
        Some((n, i.maks?))
    }).collect();
    v.sort();
    v
}

/// Batas BAWAH tiap jenis beserta semua namanya — untuk tes kesepakatan alias.
#[cfg(test)]
pub(crate) fn kelompok_bawah_untuk_tes() -> Vec<(Vec<String>, usize)> {
    let ix = indeks();
    let mut v: Vec<(Vec<String>, usize)> = ix.per_jenis.iter().filter_map(|(k, i)| {
        let mut n: Vec<String> = ix.per_nama.iter()
            .filter(|(_, kk)| *kk == k).map(|(n, _)| n.clone()).collect();
        n.sort();
        Some((n, i.min?))
    }).collect();
    v.sort();
    v
}

/// SEMUA nama dikelompokkan per jenis (juga yang tanpa batas) — untuk tes
/// "satu fungsi di interpreter = satu fungsi di VM".
#[cfg(test)]
pub(crate) fn semua_kelompok_untuk_tes() -> Vec<Vec<String>> {
    let mut g: PetaFnv<NativeFnKind, Vec<String>> = PetaFnv::default();
    for (n, k) in &indeks().per_nama { g.entry(k.clone()).or_default().push(n.clone()); }
    g.into_values().map(|mut v| { v.sort(); v }).collect()
}
