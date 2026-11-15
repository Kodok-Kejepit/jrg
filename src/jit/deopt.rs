//! Deopt penuh J3 (`dokumentasi/jit/j3-rencana.md`): kode native yang jaganya gagal mencatat keadaan
//! bingkainya (dan bingkai native pemanggilnya), lalu VM menyusun ulang bingkai-bingkai itu dan
//! melanjutkan dari instruksi yang sama — tidak ada yang diulang dari awal.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::layak::{Cara, Jenis};
use super::turun::{BUKA_SUDAH, GAGAL_SPEK, PUNCAK_VM};

/// Bit penanda di `ip` rekaman deopt (`turun.rs`).
const BENDERA: i64 = BUKA_SUDAH | GAGAL_SPEK | PUNCAK_VM;
use crate::vm::value::VmValue;
use crate::vm::vm::{CallFrame, Vm};

/// Satu bingkai native yang keluar: fungsi ke-`fid`, lanjut di `ip`, nilai stack-nya dari dasar.
pub struct Rekaman {
    pub fid: u32,
    pub ip: u32,
    pub nilai: Vec<i64>,
}

/// Rekaman satu panggilan native, paling dalam lebih dulu.
pub type Catatan = Vec<Rekaman>;

static JUMLAH: AtomicU64 = AtomicU64::new(0);

/// Berapa kali kode native keluar lewat deopt, untuk `JRG_JIT_LOG`.
pub fn jumlah() -> u64 { JUMLAH.load(Ordering::Relaxed) }

thread_local! {
    /// Per thread, bukan parameter native: parameter keempat makan satu register di tiap panggilan.
    static CATATAN: RefCell<Catatan> = const { RefCell::new(Vec::new()) };
}

/// Dipanggil kode native di blok deopt (jalur dingin).
pub extern "C" fn catat(fid: i32, ip: i32, nilai: *const i64, n: i32) {
    // `nilai` = slot stack bingkai native yang memanggil, berisi `n` bilangan.
    let nilai = unsafe { std::slice::from_raw_parts(nilai, n as usize) };
    CATATAN.with(|c| c.borrow_mut().push(Rekaman { fid: fid as u32, ip: ip as u32, nilai: nilai.to_vec() }));
}

thread_local! {
    /// Sebuah spekulasi gagal sejak terakhir ditanya (`lambat::keluar`: kode pemanggil sudah basi).
    static SPEK_BARU: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn ambil_spek_baru() -> bool { SPEK_BARU.with(|s| s.replace(false)) }

/// Rekaman panggilan native yang baru keluar (dikosongkan).
pub fn ambil() -> Catatan { CATATAN.with(|c| std::mem::take(&mut *c.borrow_mut())) }

/// `JRG_JIT_STRES=deopt-acak`: tiap instruksi yang dikompilasi bisa keluar ke VM (07-kebenaran-dan-uji.md).
pub fn acak_aktif() -> bool {
    static A: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *A.get_or_init(|| std::env::var("JRG_JIT_STRES").map(|s| s.split(',').any(|m| m == "deopt-acak")).unwrap_or(false))
}

/// Undian deterministik per thread (xorshift, benih tetap): 1 kira-kira sekali tiap 16 instruksi.
pub extern "C" fn undi() -> i32 {
    thread_local! { static X: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) }; }
    X.with(|x| {
        let mut v = x.get();
        v ^= v << 13; v ^= v >> 7; v ^= v << 17;
        x.set(v);
        (v >> 60 == 0) as i32
    })
}

/// Susun ulang bingkai VM dari rekaman. Rekaman terluar milik bingkai VM teratas (yang menjalankan kode
/// native): isinya diperbarui di tempat, posisi `Nilai` tetap nilai VM aslinya. Sisanya bingkai baru.
pub(crate) fn pulihkan(vm: &mut Vm, catatan: Catatan) {
    JUMLAH.fetch_add(1, Ordering::Relaxed);
    if super::laporan::diminta() { catat_sebab(&catatan); }
    let mut rekaman = catatan.into_iter().rev();
    if let Some(r) = rekaman.next() {
        let (f, rencana) = super::modul::info(r.fid);
        let (ip, bendera) = ((r.ip as i64 & !BENDERA) as usize, r.ip as i64 & BENDERA);
        // Bukaan gabungan dijaga di `LoadLocal`-nya; spekulasinya milik instruksi sesudahnya. Hasil yang
        // dispekulasikan dijaga di instruksi sesudahnya.
        if bendera & GAGAL_SPEK != 0 {
            let situs = if bendera & PUNCAK_VM != 0 { ip - 1 } else if rencana.cara[ip] == Cara::Muat { ip + 1 } else { ip };
            f.chunk.kode_jit.tandai_spek_gagal(situs);
            SPEK_BARU.with(|s| s.set(true));
        }
        let mut jenis = rencana.keadaan[ip].clone().expect("ip lanjut selalu terjangkau");
        // `keadaan[ip]` sudah sesudah `wujud[ip]`; rekaman yang dipasang instruksi SEBELUMNYA untuk `ip` tiba
        // sebelum `muat`-nya berjalan, jadi salinan mayanya dibaca dari lokal asalnya (sama isinya bila sudah).
        for &(p, l) in &rencana.wujud[ip] { jenis[p] = Jenis::Salinan(l); }
        // Bukaan yang sudah berhasil: angkanya di bingkai native (lokal gabungan tidak pernah di stack VM).
        if let (true, Cara::Buka(daftar)) = (bendera & BUKA_SUDAH != 0, &rencana.cara[ip]) {
            for &(p, j) in daftar { jenis[p] = j; }
        }
        if bendera & PUNCAK_VM != 0 { if let Some(p) = jenis.last_mut() { *p = Jenis::Nilai; } }
        let fr = vm.frames.last_mut().expect("bingkai yang menjalankan kode native");
        fr.ip = ip;
        let basis = fr.slot + 1;
        let mut lama = vm.stack.split_off(basis);
        // Salinan maya: lokal asalnya belum berubah sejak dimuat; dibaca sebelum posisi-posisinya dipindah.
        let mut salinan: Vec<Option<VmValue>> = jenis.iter().map(|j| match j {
            Jenis::Salinan(l) => Some(match &lama[*l as usize] {
                VmValue::Cell(c) => c.lock().unwrap_or_else(|p| p.into_inner()).clone(),
                v => v.clone(),
            }),
            _ => None,
        }).collect();
        for (i, v) in r.nilai.into_iter().enumerate() {
            // `Lebar` sudah ditulis ke stack VM oleh blok deopt-nya (`turun::keluar_deopt`).
            let nilai = match jenis[i] {
                Jenis::Salinan(_) => salinan[i].take().expect("dibaca di atas"),
                j if j.di_vm() || j == Jenis::Lebar => std::mem::replace(&mut lama[i], VmValue::Nil),
                j => nilai_dari(j, v, &rencana.callee, &f.chunk.constants),
            };
            vm.stack.push(nilai);
        }
    }
    for r in rekaman {
        let (f, rencana) = super::modul::info(r.fid);
        vm.stack.push(VmValue::Function(Arc::clone(&f)));
        let slot = vm.stack.len() - 1;
        let jenis = rencana.keadaan[r.ip as usize].as_ref().expect("ip lanjut selalu terjangkau");
        for (i, v) in r.nilai.into_iter().enumerate() {
            vm.stack.push(nilai_dari(jenis[i], v, &rencana.callee, &f.chunk.constants));
        }
        let argc = f.arity as u8;
        vm.dorong_bingkai(CallFrame { func: f, ip: r.ip as usize, slot, defers: Vec::new(), argc, closure: None });
    }
}

/// Deopt per opcode (`titik keluar` atau `jaga`) untuk `JRG_JIT_LOG`: penentu opcode mana yang diturunkan berikutnya.
static SEBAB: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());

fn catat_sebab(catatan: &Catatan) {
    let Some(r) = catatan.first() else { return };
    let (f, rc) = super::modul::info(r.fid);
    let mut ip = (r.ip as i64 & !BENDERA) as usize;
    let jenis = if rc.keluar[ip] { "keluar" } else if r.ip as i64 & GAGAL_SPEK != 0 { "spekulasi" } else { "jaga" };
    // Hasil yang dispekulasikan dijaga di instruksi sesudahnya; yang disebut instruksi penghasilnya.
    if r.ip as i64 & PUNCAK_VM != 0 { ip -= 1; }
    let kunci = format!("{jenis} {:?}", f.chunk.code[ip].op);
    let mut g = SEBAB.lock().unwrap_or_else(|p| p.into_inner());
    match g.iter_mut().find(|(k, _)| *k == kunci) {
        Some((_, n)) => *n += 1,
        None => g.push((kunci, 1)),
    }
}

/// Sebab deopt terbanyak lebih dulu.
pub fn sebab() -> Vec<(String, u64)> {
    let mut v = SEBAB.lock().unwrap_or_else(|p| p.into_inner()).clone();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    v
}

fn nilai_dari(j: Jenis, v: i64, callee: &[Arc<crate::vm::value::VmFunction>], konstanta: &[VmValue]) -> VmValue {
    match j {
        // Fungsi bawaan maya: tidak pernah ditulis ke stack VM, dibangun dari konstantanya.
        Jenis::Bawaan(i) | Jenis::Bungkus(_, i) => konstanta[i as usize].clone(),
        Jenis::Bulat => VmValue::Int(v as i128),
        Jenis::Bool => VmValue::Bool(v != 0),
        Jenis::Pecahan => VmValue::Float(f64::from_bits(v as u64)),
        Jenis::Rune => super::lambat::skalar_kode(7, v).expect("kode rune"),
        Jenis::Fungsi(c) => VmValue::Function(Arc::clone(&callee[c])),
        Jenis::Kosong => VmValue::Nil,
        Jenis::Satuan => VmValue::Unit,
        // Bingkai callee native tidak pernah memegang nilai milik VM (argumennya selalu bulat).
        Jenis::Nilai | Jenis::Lebar | Jenis::Salinan(_) => unreachable!("posisi Nilai cuma ada di bingkai terluar"),
    }
}
