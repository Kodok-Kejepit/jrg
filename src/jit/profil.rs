//! Profil per fungsi untuk tingkat T0 (`dokumentasi/jit/02-arsitektur.md`). Hanya diisi di mode
//! `--jit`; di `--vm` biasa `Chunk::profil` kosong dan perekamnya tidak pernah dipanggil.

use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::vm::value::VmValue;

/// Bit tipe yang teramati di satu situs (beberapa bit = situs polimorfik).
pub mod bit {
    pub const BULAT: u16 = 1;
    pub const PECAHAN: u16 = 2;
    pub const TEKS: u16 = 4;
    pub const BOOL: u16 = 8;
    pub const LARIK: u16 = 16;
    pub const PETA: u16 = 32;
    pub const NIL: u16 = 64;
    pub const LAIN: u16 = 128;
    /// Bulat di luar 64 bit (hash `uint`): spekulasinya langsung `Jenis::Lebar`.
    pub const BESAR: u16 = 256;
}

pub fn bit_tipe(v: &VmValue) -> u16 {
    match v {
        VmValue::Int(n) => if i64::try_from(*n).is_ok() { bit::BULAT } else { bit::BESAR },
        VmValue::Float(_) => bit::PECAHAN,
        VmValue::Str(_) => bit::TEKS,
        VmValue::Bool(_) => bit::BOOL,
        VmValue::Array(_) | VmValue::Tuple(_) => bit::LARIK,
        VmValue::Map(_) => bit::PETA,
        VmValue::Nil | VmValue::Unit => bit::NIL,
        _ => bit::LAIN,
    }
}

#[derive(Debug)]
pub struct ProfilFungsi {
    pub nama: String,
    pub panggilan: AtomicU32,
    /// Lompatan mundur (badan loop) yang dijalankan di fungsi ini.
    pub putaran: AtomicU32,
    /// Panggilan yang masuk kode native, dan yang keluar lewat deopt.
    pub jit_masuk: AtomicU32,
    pub jit_deopt: AtomicU32,
    /// Satu bitmask tipe per instruksi; hanya situs operator yang terisi.
    pub umpan: Box<[AtomicU16]>,
}

impl ProfilFungsi {
    /// Hitungan kasar tanpa operasi atomik tulis-baca (`lock xadd` di tiap panggilan dan putaran membuat
    /// `--jit` ±9% lebih lambat dari `--vm` di kode yang tetap di VM); balapan antar-thread cuma kehilangan hitungan.
    #[inline]
    pub fn catat_panggilan(&self) { self.panggilan.store(self.panggilan.load(Ordering::Relaxed).wrapping_add(1), Ordering::Relaxed); }
    #[inline]
    pub fn catat_putaran(&self) -> u32 {
        let n = self.putaran.load(Ordering::Relaxed).wrapping_add(1);
        self.putaran.store(n, Ordering::Relaxed);
        n
    }
    /// Bit baru saja yang ditulis: sesudah situsnya stabil cuma satu baca, tanpa operasi atomik tulis.
    #[inline]
    pub fn catat_tipe(&self, ip: usize, b: u16) {
        if let Some(s) = self.umpan.get(ip) {
            if s.load(Ordering::Relaxed) & b != b { s.fetch_or(b, Ordering::Relaxed); }
        }
    }
}

/// Semua profil yang dibuat di proses ini, untuk laporan akhir.
static DAFTAR: Mutex<Vec<Arc<ProfilFungsi>>> = Mutex::new(Vec::new());

/// Profil baru untuk chunk ber-`n` instruksi; `None` di luar mode `--jit`.
pub fn baru(nama: &str, n: usize) -> Option<Arc<ProfilFungsi>> {
    if !super::aktif() { return None; }
    let p = Arc::new(ProfilFungsi {
        nama: nama.to_string(),
        panggilan: AtomicU32::new(0),
        putaran: AtomicU32::new(0),
        jit_masuk: AtomicU32::new(0),
        jit_deopt: AtomicU32::new(0),
        umpan: (0..n).map(|_| AtomicU16::new(0)).collect(),
    });
    DAFTAR.lock().unwrap_or_else(|p| p.into_inner()).push(Arc::clone(&p));
    Some(p)
}

pub fn semua() -> Vec<Arc<ProfilFungsi>> {
    DAFTAR.lock().unwrap_or_else(|p| p.into_inner()).clone()
}
