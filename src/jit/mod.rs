//! JRGJIT, mesin eksekusi keempat (rancangan: `dokumentasi/jit/`). J1: VM berprofil; J2: kode mesin lewat
//! Cranelift (`j2-rencana.md`); J3: deopt di instruksi yang sama (`j3-rencana.md`); J4: titik keluar, nilai
//! milik VM di bingkai, OSR di kepala loop (`j4-rencana.md`).

pub mod profil;
pub mod laporan;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
pub mod layak;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
mod turun;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
pub(crate) mod modul;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
pub(crate) mod panggil;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
pub(crate) mod deopt;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
pub(crate) mod lambat;
#[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
pub(crate) mod tata_letak;

use std::sync::atomic::{AtomicBool, Ordering};

static AKTIF: AtomicBool = AtomicBool::new(false);

/// Dinyalakan sekali oleh `jrgc --jit` sebelum program dikompilasi.
pub fn aktifkan() { AKTIF.store(true, Ordering::Relaxed); }

#[inline]
pub fn aktif() -> bool { AKTIF.load(Ordering::Relaxed) }

/// Pintu masuk kode native sebuah fungsi: `(args, kedalaman, keluar) -> status`.
#[derive(Clone, Copy)]
pub struct KodeJit {
    pub masuk: unsafe extern "C" fn(*const i64, i32, *mut i64) -> i32,
    /// Memuat panggilan langsung (salinan kode callee): basi begitu spekulasi mana pun gagal (`EPOCH_SPEK`).
    pub langsung: bool,
}

/// Naik tiap spekulasi gagal; kode berpanggilan langsung dari epoch lama tidak dipakai lagi.
pub static EPOCH_SPEK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl std::fmt::Debug for KodeJit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("<kode jit>") }
}

/// Panggilan ke-berapa sebuah fungsi dikompilasi (`JRG_JIT_AMBANG`, bawaan 2; 0 = mode stres).
pub fn ambang() -> u32 {
    static A: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *A.get_or_init(|| std::env::var("JRG_JIT_AMBANG").ok().and_then(|s| s.parse().ok()).unwrap_or(2))
}

/// Satu titik masuk kode native milik sebuah chunk; `kode: None` = sudah dicoba dan tidak layak.
pub struct Masuk {
    pub ip: u32,
    pub kembali: u8,
    /// `PintuJit::versi` saat dikompilasi; entri versi lama tidak dipakai lagi.
    pub versi: u32,
    /// Jenis tiap posisi stack bingkai di titik masuk (`layak::Jenis` sebagai kode).
    pub jenis: Box<[u8]>,
    /// `EPOCH_SPEK` saat dikompilasi (dipakai cuma untuk kode berpanggilan langsung).
    pub epoch: u32,
    pub kode: Option<KodeJit>,
}

/// Titik masuk native per chunk (dibagi goroutine, jadi di balik kunci).
#[derive(Default)]
pub struct PintuJit {
    /// Entri dibuat sekali dan tidak pernah dibebaskan (paling banyak `MAKS_MASUK` per versi), jadi
    /// `cepat` boleh menunjuknya tanpa kunci.
    pub masuk: std::sync::Mutex<Vec<&'static Masuk>>,
    /// Entri yang terakhir cocok: dicocokkan tanpa kunci di tiap panggilan dari VM.
    pub cepat: std::sync::atomic::AtomicPtr<Masuk>,
    /// Masuk lewat panggilan pernah gagal dikompilasi: panggilan berikutnya langsung VM, tanpa kunci.
    pub panggilan_mati: std::sync::atomic::AtomicBool,
    /// Naik tiap kali kodenya dibuang (spekulasi gagal); bagian kunci tembolok `modul`.
    pub versi: std::sync::atomic::AtomicU32,
    /// Instruksi yang spekulasinya (nilai VM sebagai angka, larik berelemen angka) pernah gagal, dan berapa
    /// kali: sekali = bulat 64 bit gagal (coba `Lebar`), dua kali = tidak berspekulasi lagi.
    pub spek_gagal: std::sync::Mutex<Vec<(u32, u8)>>,
    /// Umpan `GetMethod`: situs, label tipe penerima pertama yang teramati, dan method pemakai-nya.
    pub metode: std::sync::Mutex<Vec<(u32, String, std::sync::Arc<crate::vm::value::VmFunction>)>>,
}

/// Spekulasi yang gagal lebih dari ini membuat chunk-nya berhenti dikompilasi ulang.
const MAKS_SPEK_GAGAL: usize = 16;

impl PintuJit {
    pub fn catat_metode(&self, ip: usize, label: String, f: std::sync::Arc<crate::vm::value::VmFunction>) {
        let mut g = self.metode.lock().unwrap_or_else(|p| p.into_inner());
        if !g.iter().any(|(i, ..)| *i == ip as u32) { g.push((ip as u32, label, f)); }
    }

    pub fn metode_di(&self, ip: usize) -> Option<(String, std::sync::Arc<crate::vm::value::VmFunction>)> {
        let g = self.metode.lock().unwrap_or_else(|p| p.into_inner());
        g.iter().find(|(i, ..)| *i == ip as u32).map(|(_, l, f)| (l.clone(), std::sync::Arc::clone(f)))
    }

    pub fn spek_gagal_di(&self, ip: usize) -> bool { self.spek_tingkat(ip) > 0 }

    /// Berapa kali spekulasi di `ip` gagal (0, 1, 2).
    pub fn spek_tingkat(&self, ip: usize) -> u8 {
        let g = self.spek_gagal.lock().unwrap_or_else(|p| p.into_inner());
        g.iter().find(|(i, _)| *i == ip as u32).map_or(0, |(_, n)| *n)
    }

    /// Spekulasi di `ip` gagal: kodenya dibuang, kompilasi berikutnya berspekulasi lebih lebar (`Lebar`)
    /// atau tidak sama sekali.
    pub fn tandai_spek_gagal(&self, ip: usize) {
        let mut g = self.spek_gagal.lock().unwrap_or_else(|p| p.into_inner());
        match g.iter_mut().find(|(i, _)| *i == ip as u32) {
            Some((_, n)) if *n >= 2 => return,
            Some((_, n)) => *n += 1,
            None => g.push((ip as u32, 1)),
        }
        if g.len() > MAKS_SPEK_GAGAL { self.panggilan_mati.store(true, Ordering::Relaxed); }
        drop(g);
        self.versi.fetch_add(1, Ordering::Relaxed);
        EPOCH_SPEK.fetch_add(1, Ordering::Relaxed);
        self.cepat.store(std::ptr::null_mut(), Ordering::Release);
        self.masuk.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

// Salinan chunk (goroutine, closure) mulai tanpa kode; kodenya dikompilasi ulang bila perlu.
impl Clone for PintuJit {
    fn clone(&self) -> Self { PintuJit::default() }
}

impl std::fmt::Debug for PintuJit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("<pintu jit>") }
}

/// Berapa putaran sekali VM mencoba OSR di lompatan mundur (mode stres lebih sering).
pub fn osr_tiap() -> u32 { if ambang() == 0 { 4 } else { 1024 } }
