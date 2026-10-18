// src/interpreter/builtin/jembatan_stdlib.rs — interpreter/builtin/jembatan_stdlib.rs

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult};
use crate::vm::value::{VmValue, VmNativeFn};
use crate::runtime_err;

/// Namespace yang bergantung pada keadaan runtime VM. Lihat catatan di atas.
const BERGANTUNG_RUNTIME_VM: &[&str] = &["future::", "task::"];

/// Peta nama → fungsi, dipanen sekali dari registrasi stdlib VM. Namanya
/// dibocorkan SEKALI di sini, jadi `cari` bisa menyerahkan `&'static str`.
fn peta() -> &'static HashMap<&'static str, VmNativeFn> {
    static PETA: OnceLock<HashMap<&'static str, VmNativeFn>> = OnceLock::new();
    PETA.get_or_init(|| {
        crate::vm::bawaan::tabel_bawaan()
            .iter()
            .filter_map(|(k, v)| match v {
                crate::vm::bawaan::Bawaan::Fungsi(f) =>
                    Some((&*Box::leak(k.clone().into_boxed_str()), *f)),
                _ => None,
            })
            .collect()
    })
}

/// Nama kanonik (terpendek, lalu abjad — aturan Fase TELANJANG) sebuah native VM.
pub(crate) fn nama_native(f: VmNativeFn) -> Option<&'static str> {
    static NAMA: OnceLock<HashMap<usize, &'static str>> = OnceLock::new();
    NAMA.get_or_init(|| {
        let mut m: HashMap<usize, &'static str> = HashMap::new();
        for (n, g) in peta() {
            let e = m.entry(*g as usize).or_insert(*n);
            if (n.len(), *n) < (e.len(), *e) { *e = *n; }
        }
        m
    })
    .get(&(f as usize))
    .copied()
}

/// Native VM yang melayani builtin interpreter `k` — lewat NAMA-nya, karena
/// dua mesin mengelompokkan nama builtin persis sama.
pub(crate) fn native_vm_untuk(k: &super::NativeFnKind) -> Option<VmNativeFn> {
    let nama = match k {
        super::NativeFnKind::DariStdlibVm(n) => *n,
        k => super::aritas::nama_jenis(k)?,
    };
    peta().get(nama).copied()
}

/// Apakah `nama` dilayani stdlib VM dan layak dijembatani?
pub fn ada_di_stdlib(nama: &str) -> bool {
    peta().contains_key(nama)
}

pub(crate) fn cari(nama: &str) -> Option<&'static str> {
    if BERGANTUNG_RUNTIME_VM.iter().any(|p| nama.starts_with(p)) {
        return None;
    }
    // Kuncinya sudah `&'static str` sejak peta dibangun; dulu dibocorkan
    // ULANG di tiap evaluasi nama (±28 byte per panggilan, tanpa batas).
    peta().get_key_value(nama).map(|(k, _)| *k)
}

/// Panggil fungsi stdlib VM dari interpreter.
pub(crate) fn panggil(nama: &str, args: &[Value]) -> EvalResult {
    let Some(f) = peta().get(nama) else {
        return runtime_err!(
            "`{}` terdaftar sebagai builtin stdlib tapi implementasinya tidak ketemu — \
             ini bug di JRG sendiri, bukan di programmu",
            nama
        );
    };
    // Nilai FUNGSI dititipkan: yang menyeberang penanda bertipe dan bertampilan
    // sama, jadi badan VM memutuskan dengan kalimatnya sendiri, di kedalaman mana pun.
    let mut titipan = crate::vm::ffi_jembatan::TitipanInterp::default();
    let mut vm_args = Vec::with_capacity(args.len());
    for a in args {
        match titipan.ke_vm(a) {
            Ok(v) => vm_args.push(v),
            // Nilai yang tidak punya padanan di sisi VM ditolak dengan
            // menyebut tipenya — bukan diam-diam jadi `nil`.
            // Yang ditanyakan `json::` adalah JSON: kalimatnya kalimat penyandinya, bukan jembatan.
            Err(_) if nama.starts_with("json::") => return runtime_err!("{}",
                crate::interpreter::galat_index::tidak_bisa_jadi_json(&a.type_name(),
                    if matches!(a, Value::Channel(_)) { crate::json_inti::JALAN_CHANNEL }
                    else { "kirim nilai biasa — angka, teks, array, dict, struct atau enum" })),
            Err(e) => return runtime_err!("{}: {}", nama, e),
        }
    }
    match f(&vm_args) {
        Ok(v) => match titipan.ke_interp(&v) {
            Ok(v) => Ok(v),
            Err(e) => runtime_err!("{}: hasilnya tidak bisa dibawa balik: {}", nama, e),
        },
        // Sebuah pesan yang isinya cuma tanda tangan bukan kalimat.
        // Lihat `vm::value::perjelas_pesan_native`.
        Err(e) => runtime_err!("{}",
            crate::vm::value::perjelas_pesan_native(e, &vm_args)),
    }
}
