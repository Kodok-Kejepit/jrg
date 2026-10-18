// src/vm/info_native.rs — SATU tabel per alamat native: batas argumen, nama
// kanonik, layanan khusus, dan apakah ia builtin cetak. Dibangun sekali per proses,
// jadi pintu `call_value` membayar satu pencarian FNV, bukan dua pencarian SipHash.

use std::sync::OnceLock;

use super::tampil::BUILTIN_CETAK;
use super::value::VmNativeFn;
use super::vm::NativeKhusus;
use crate::interpreter::arity::{maks, minimal, tanda_tangan, PetaFnv};

pub(crate) struct InfoNative {
    pub(crate) maks: Option<usize>,
    pub(crate) min: Option<usize>,
    pub(crate) kanonik: String,
    /// Tanda tangan `--doc` salah satu nama kelompoknya, untuk baris bantuan.
    pub(crate) tanda: Option<&'static str>,
    pub(crate) khusus: Option<NativeKhusus>,
    pub(crate) cetak: bool,
}

/// Builtin yang VM layani SENDIRI: perlu memanggil balik fungsi JRG atau
/// membaca `globals`, sesuatu yang penunjuk fungsi dari `def` tidak bisa.
const KHUSUS: &[(&str, NativeKhusus)] = &[
    ("dok::num_solve", NativeKhusus::NumSolve),
    ("Future::new",    NativeKhusus::Tugas { task: false }),
    ("Task::spawn",    NativeKhusus::Tugas { task: true }),
    ("dok::serve",              NativeKhusus::Serve),
    ("dok::router_get",         NativeKhusus::Router { metode: "GET" }),
    ("dok::router_post",        NativeKhusus::Router { metode: "POST" }),
    ("dok::router_put",         NativeKhusus::Router { metode: "PUT" }),
    ("dok::router_delete",      NativeKhusus::Router { metode: "DELETE" }),
    ("dok::router_patch",       NativeKhusus::Router { metode: "PATCH" }),
    ("dok::router_any",         NativeKhusus::Router { metode: "*" }),
    ("dok::router_not_found",   NativeKhusus::Router { metode: "404" }),
    ("dok::router_use",         NativeKhusus::Router { metode: "USE" }),
    ("dok::router_after",       NativeKhusus::Router { metode: "AFTER" }),
    ("dok::router_ws",          NativeKhusus::Router { metode: "WS" }),
    ("terap",                   NativeKhusus::Terap),
    ("terap_bernama",           NativeKhusus::TerapBernama),
];

struct Indeks {
    per_alamat: PetaFnv<usize, InfoNative>,
    per_nama: PetaFnv<String, usize>,
}

fn indeks() -> &'static Indeks {
    static I: OnceLock<Indeks> = OnceLock::new();
    I.get_or_init(|| {
        let mut kelompok: PetaFnv<usize, Vec<String>> = PetaFnv::default();
        let mut per_nama: PetaFnv<String, usize> = PetaFnv::default();
        for (n, v) in super::bawaan::tabel_bawaan().iter() {
            if let super::bawaan::Bawaan::Fungsi(f) = v {
                kelompok.entry(*f as usize).or_default().push(n.clone());
                per_nama.insert(n.clone(), *f as usize);
            }
        }
        let per_alamat = kelompok.into_iter().map(|(a, mut nama)| {
            // Atas TERBESAR, bawah TERKECIL: batas yang terlalu ketat menolak program benar.
            let m = nama.iter().filter_map(|n| maks(n)).max();
            let b = nama.iter().filter_map(|n| minimal(n)).min();
            let cetak = nama.iter().any(|n| BUILTIN_CETAK.contains(&n.as_str()));
            let khusus = KHUSUS.iter().find(|(k, _)| nama.iter().any(|n| n == k)).map(|(_, j)| *j)
                .or_else(|| nama.iter().find_map(|n| super::hof::layanan_hof(n))
                    .map(|h| NativeKhusus::Hof { nama: h }))
                .or_else(|| nama.iter().find_map(|n| super::ffi_asing::nomor(n))
                    .map(|no| NativeKhusus::Ffi { nomor: no }));
            nama.sort_by(|x, y| x.len().cmp(&y.len()).then_with(|| x.cmp(y)));
            let tanda = nama.iter().find_map(|n| tanda_tangan(n));
            (a, InfoNative { maks: m, min: b, kanonik: nama.swap_remove(0), khusus, cetak, tanda })
        }).collect();
        Indeks { per_alamat, per_nama }
    })
}

pub(crate) fn info(f: VmNativeFn) -> Option<&'static InfoNative> {
    indeks().per_alamat.get(&(f as usize))
}

/// Seperti [`info`], lewat NAMA — untuk builtin stdlib VM yang interpreter capai
/// lewat jembatan, supaya kedua mesin membaca batas dari kelompok yang sama.
pub(crate) fn info_nama(n: &str) -> Option<&'static InfoNative> {
    let ix = indeks();
    ix.per_alamat.get(ix.per_nama.get(n)?)
}

/// Alamat builtin bernama `n` — untuk memastikan nama yang ditulis pemakai
/// memang merujuk native yang sedang dipanggil (bukan variabel pembayang).
pub(crate) fn alamat_nama(n: &str) -> Option<usize> {
    indeks().per_nama.get(n).copied()
}

/// Batas tiap alamat beserta semua namanya — untuk tes kesepakatan alias.
#[cfg(test)]
pub(crate) fn kelompok_untuk_tes() -> Vec<(Vec<String>, usize)> {
    let ix = indeks();
    let mut v: Vec<(Vec<String>, usize)> = ix.per_alamat.iter().filter_map(|(a, i)| {
        let m = i.maks?;
        let mut n: Vec<String> = ix.per_nama.iter().filter(|(_, aa)| *aa == a)
            .map(|(n, _)| n.clone()).collect();
        n.sort();
        Some((n, m))
    }).collect();
    v.sort();
    v
}

/// Batas BAWAH tiap alamat beserta semua namanya.
#[cfg(test)]
pub(crate) fn kelompok_bawah_untuk_tes() -> Vec<(Vec<String>, usize)> {
    let ix = indeks();
    let mut v: Vec<(Vec<String>, usize)> = ix.per_alamat.iter().filter_map(|(a, i)| {
        let b = i.min?;
        let mut n: Vec<String> = ix.per_nama.iter().filter(|(_, aa)| *aa == a)
            .map(|(n, _)| n.clone()).collect();
        n.sort();
        Some((n, b))
    }).collect();
    v.sort();
    v
}

/// SEMUA nama dikelompokkan per alamat (juga yang tanpa batas).
#[cfg(test)]
pub(crate) fn semua_kelompok_untuk_tes() -> Vec<Vec<String>> {
    let mut g: PetaFnv<usize, Vec<String>> = PetaFnv::default();
    for (n, a) in &indeks().per_nama { g.entry(*a).or_default().push(n.clone()); }
    g.into_values().map(|mut v| { v.sort(); v }).collect()
}

#[cfg(test)]
mod tes {
    use std::collections::HashMap;

    /// Tiap nama dalam kelompok berbatas punya entri SENDIRI yang sama, dan dua
    /// mesin mengelompokkan nama bersama persis sama — kalau tidak, pemanggilan
    /// tak langsung (`terap(str::rjust, …)`) menyebut nama kanonik yang berbeda.
    #[test]
    fn alias_sepakat_batas_dan_nama_di_dua_mesin() {
        let vm = super::kelompok_untuk_tes();
        let interp = crate::interpreter::builtin::aritas::kelompok_untuk_tes();
        let mut salah = Vec::new();
        for (nama, m) in vm.iter().chain(interp.iter()) {
            for n in nama {
                if crate::interpreter::arity::maks(n) != Some(*m) {
                    salah.push(format!("`{}` di kelompok {:?} (batas {}) tanpa entri yang sama", n, nama, m));
                }
            }
        }
        // Pengelompokan nama yang ada di DUA mesin (py::/js::/atomic:: cuma di satu).
        let peta = |g: &[(Vec<String>, usize)]| -> HashMap<String, (Vec<String>, usize)> {
            g.iter().flat_map(|(ns, m)| ns.iter().map(move |n| (n.clone(), (ns.clone(), *m)))).collect()
        };
        let (pv, pi) = (peta(&vm), peta(&interp));
        let bersama = |ns: &[String]| -> Vec<String> {
            ns.iter().filter(|n| pv.contains_key(*n) && pi.contains_key(*n)).cloned().collect()
        };
        for (n, (gv, mv)) in &pv {
            let Some((gi, mi)) = pi.get(n) else { continue };
            if mv != mi || bersama(gv) != bersama(gi) {
                salah.push(format!("`{}`: vm {:?}/{} lawan interpreter {:?}/{}", n, gv, mv, gi, mi));
            }
        }
        assert!(vm.len() > 50 && interp.len() > 50, "cuma {}/{} kelompok berbatas", vm.len(), interp.len());
        // SEMUA kelompok, termasuk yang tanpa batas: `fmt::eprint` pernah jadi salinan
        // kedua di VM (menjawab `nil`) sementara interpreter memberinya satu jenis.
        let rata = |g: Vec<Vec<String>>| -> HashMap<String, Vec<String>> {
            g.into_iter().flat_map(|ns| ns.clone().into_iter().map(move |n| (n, ns.clone()))).collect()
        };
        let (sv, si) = (rata(super::semua_kelompok_untuk_tes()),
                        rata(crate::interpreter::builtin::aritas::semua_kelompok_untuk_tes()));
        let potong = |ns: &[String]| -> Vec<String> {
            ns.iter().filter(|n| sv.contains_key(*n) && si.contains_key(*n)).cloned().collect()
        };
        for (n, gv) in &sv {
            let Some(gi) = si.get(n) else { continue };
            if potong(gv) != potong(gi) {
                salah.push(format!("`{}`: satu fungsi beda kelompok — vm {:?} lawan interpreter {:?}",
                    n, potong(gv), potong(gi)));
            }
        }
        assert!(sv.len() > 500, "cuma {} nama VM yang terbaca", sv.len());
        // Batas BAWAH: tiap anggota kelompok punya entri yang sama, dan dua mesin
        // sepakat — kalau tidak, satu alias menolak yang diterima alias lainnya.
        let (bv, bi) = (super::kelompok_bawah_untuk_tes(),
                        crate::interpreter::builtin::aritas::kelompok_bawah_untuk_tes());
        for (nama, b) in bv.iter().chain(bi.iter()) {
            for n in nama {
                if crate::interpreter::arity::minimal(n) != Some(*b) {
                    salah.push(format!("`{}` di kelompok {:?} (bawah {}) tanpa entri yang sama", n, nama, b));
                }
            }
        }
        let (pbv, pbi) = (peta(&bv), peta(&bi));
        for (n, (_, b)) in &pbv {
            if let Some((_, c)) = pbi.get(n) {
                if b != c { salah.push(format!("`{}`: batas bawah vm {} lawan interpreter {}", n, b, c)); }
            }
        }
        assert!(bv.len() > 50 && bi.len() > 50, "cuma {}/{} kelompok berbatas bawah", bv.len(), bi.len());
        salah.sort();
        salah.dedup();
        assert!(salah.is_empty(), "{} masalah:\n{}", salah.len(), salah.join("\n"));
    }
}
