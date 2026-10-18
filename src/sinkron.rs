// src/sinkron.rs — `Atomic` dan `Mutex`: handle ber-`Arc`, satu definisi untuk dua mesin.

use std::cell::RefCell;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;

/// Isi `Value::Sinkron`/`VmValue::Sinkron`. Yang disalin pegangannya, bukan
/// keadaannya — sama seperti channel, jadi menyeberang jembatan nilai O(1).
#[derive(Clone, Debug)]
pub enum Pegangan {
    Atom(Arc<Mutex<i128>>),
    Kunci(Arc<Kunci>),
    /// Isi dalam sebuah handle bawaan (saluran hasil `Future`/`Task`): bisa
    /// dipegang dan disalin, tapi tidak bisa dibuka dari program.
    Buram(Buram),
}

/// Isi buram. Aman-unwind dinyatakan sendiri: isinya saluran (`Mutex` + `Condvar`),
/// dan `dyn Any` tidak membawa sifat itu — tanpanya jembatan LLVM tidak terkompilasi.
#[derive(Clone, Debug)]
pub struct Buram(Arc<dyn std::any::Any + Send + Sync>);

impl std::panic::UnwindSafe for Buram {}
impl std::panic::RefUnwindSafe for Buram {}

/// Kunci murni berpemilik: yang mengunci harus yang melepas.
#[derive(Debug, Default)]
pub struct Kunci {
    st: Mutex<Keadaan>,
    cv: Condvar,
}

#[derive(Debug, Default)]
struct Keadaan {
    pemilik: Option<ThreadId>,
    /// Pemiliknya selesai tanpa melepas; kunci ini tidak bisa dipakai lagi.
    ditinggal: bool,
}

/// Kunci yang sedang dipegang thread ini. Saat thread-nya selesai, yang masih
/// tertinggal ditandai `ditinggal` supaya penunggunya gagal, bukan menggantung.
struct Dipegang(RefCell<Vec<Arc<Kunci>>>);

impl Drop for Dipegang {
    fn drop(&mut self) {
        for k in self.0.borrow_mut().drain(..) {
            let mut st = k.st.lock().unwrap_or_else(|p| p.into_inner());
            st.pemilik = None;
            st.ditinggal = true;
            k.cv.notify_all();
        }
    }
}

thread_local! {
    static DIPEGANG: Dipegang = const { Dipegang(RefCell::new(Vec::new())) };
}

impl Pegangan {
    pub fn atom(awal: i128) -> Self { Pegangan::Atom(Arc::new(Mutex::new(awal))) }
    pub fn kunci() -> Self { Pegangan::Kunci(Arc::new(Kunci::default())) }

    pub fn buram<T: Send + Sync + 'static>(isi: T) -> Self { Pegangan::Buram(Buram(Arc::new(isi))) }

    /// Isi sebuah pegangan buram, kalau memang bertipe `T`.
    pub fn isi_buram<T: 'static>(&self) -> Option<&T> {
        match self { Pegangan::Buram(a) => a.0.downcast_ref::<T>(), _ => None }
    }

    /// Nama yang dijawab `typeof`.
    pub fn nama_tipe(&self) -> &'static str {
        match self {
            Pegangan::Atom(_) => "atomic",
            Pegangan::Kunci(_) => "mutex",
            Pegangan::Buram(_) => "handle",
        }
    }

    /// Namespace tempat method pegangan ini dicari: `a.add(2)` adalah `atomic::add(a, 2)`.
    pub fn ns_metode(&self) -> Option<&'static str> {
        match self {
            Pegangan::Atom(_) => Some("atomic"),
            Pegangan::Kunci(_) => Some("mutex"),
            Pegangan::Buram(_) => None,
        }
    }

    /// Tampilannya tidak memuat isinya: nilainya bisa berubah di tengah mencetak.
    pub fn tampil(&self) -> &'static str {
        match self {
            Pegangan::Atom(_) => "<atomic>",
            Pegangan::Kunci(_) => "<mutex>",
            Pegangan::Buram(_) => "<handle>",
        }
    }

    /// Dua pegangan sama kalau menunjuk keadaan yang sama, seperti channel.
    pub fn sama(&self, lain: &Pegangan) -> bool {
        match (self, lain) {
            (Pegangan::Atom(a), Pegangan::Atom(b)) => Arc::ptr_eq(a, b),
            (Pegangan::Kunci(a), Pegangan::Kunci(b)) => Arc::ptr_eq(a, b),
            (Pegangan::Buram(a), Pegangan::Buram(b)) => Arc::ptr_eq(&a.0, &b.0),
            _ => false,
        }
    }
}

/// Kalimat untuk argumen yang bukan pegangan yang diminta.
/// Method yang dimiliki pegangan ber-namespace `ns` — satu daftar untuk tiga mesin.
pub fn metode_ada(ns: &str, method: &str) -> bool {
    match ns {
        "atomic" => matches!(method, "get" | "set" | "add" | "sub" | "cas"),
        "mutex" => matches!(method, "lock" | "unlock" | "with_lock"),
        _ => false,
    }
}

pub fn pesan_bukan(siapa: &str, apa: &str, tipe: &str) -> String {
    format!("`{}` butuh sebuah {} (buat dengan `{}::new`), dapat `{}`", siapa, apa, apa, tipe)
}

/// Kalimat untuk argumen angka yang bukan bilangan bulat.
pub fn pesan_bukan_int(siapa: &str, tipe: &str) -> String {
    format!("`{}` butuh bilangan bulat, dapat `{}` — Atomic cuma menyimpan `int`", siapa, tipe)
}

fn isi(a: &Arc<Mutex<i128>>) -> std::sync::MutexGuard<'_, i128> {
    a.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn atom_baca(a: &Arc<Mutex<i128>>) -> i128 { *isi(a) }

pub fn atom_tulis(a: &Arc<Mutex<i128>>, n: i128) { *isi(a) = n; }

/// Tambah (atau kurang) lalu jawab nilai barunya; luapan ditolak dan isinya tetap.
pub fn atom_tambah(a: &Arc<Mutex<i128>>, n: i128, kurang: bool, siapa: &str) -> Result<i128, String> {
    let mut g = isi(a);
    let (baru, op) = if kurang { (g.checked_sub(n), "-") } else { (g.checked_add(n), "+") };
    let baru = baru.ok_or_else(|| format!("{}: {}", siapa, crate::interpreter::pesan_luber(op, *g, n)))?;
    *g = baru;
    Ok(baru)
}

/// Tukar ke `baru` hanya kalau isinya masih `harap`.
pub fn atom_tukar(a: &Arc<Mutex<i128>>, harap: i128, baru: i128) -> bool {
    let mut g = isi(a);
    if *g != harap { return false; }
    *g = baru;
    true
}

fn pesan_ditinggal(siapa: &str) -> String {
    format!("{}: kunci ini ditinggal terkunci oleh goroutine yang sudah selesai — \
             yang mengunci harus melepasnya (`mutex::unlock`), atau pakai \
             `mutex::with_lock(m, f)` yang melepas sendiri", siapa)
}

/// Kunci, menunggu kalau dipegang thread lain. Mengunci lagi dari thread yang
/// sama ditolak: ia menunggu dirinya sendiri dan tidak akan pernah selesai.
pub fn kunci(k: &Arc<Kunci>, siapa: &str) -> Result<(), String> {
    let saya = std::thread::current().id();
    let mut st = k.st.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if st.ditinggal { return Err(pesan_ditinggal(siapa)); }
        match st.pemilik {
            None => break,
            Some(p) if p == saya => return Err(format!(
                "{}: kunci ini sudah kamu pegang di goroutine yang sama — mengunci lagi \
                 berarti menunggu dirimu sendiri selamanya. Lepas dulu dengan `mutex::unlock`", siapa)),
            Some(_) => st = k.cv.wait(st).unwrap_or_else(|p| p.into_inner()),
        }
    }
    st.pemilik = Some(saya);
    drop(st);
    // Thread yang sedang dibongkar tidak punya daftar lagi; kuncinya tetap sah.
    let _ = DIPEGANG.try_with(|d| d.0.borrow_mut().push(Arc::clone(k)));
    Ok(())
}

/// Lepas kunci. Yang tidak terkunci, atau dipegang goroutine lain, ditolak.
pub fn lepas(k: &Arc<Kunci>, siapa: &str) -> Result<(), String> {
    let saya = std::thread::current().id();
    let mut st = k.st.lock().unwrap_or_else(|p| p.into_inner());
    if st.ditinggal { return Err(pesan_ditinggal(siapa)); }
    match st.pemilik {
        None => return Err(format!(
            "{}: kunci ini tidak sedang terkunci — tiap `mutex::unlock` butuh satu \
             `mutex::lock` sebelumnya", siapa)),
        Some(p) if p != saya => return Err(format!(
            "{}: kunci ini dipegang goroutine lain — cuma yang mengunci yang boleh melepas", siapa)),
        Some(_) => {}
    }
    st.pemilik = None;
    drop(st);
    k.cv.notify_one();
    let _ = DIPEGANG.try_with(|d| {
        let mut v = d.0.borrow_mut();
        if let Some(i) = v.iter().position(|x| Arc::ptr_eq(x, k)) { v.swap_remove(i); }
    });
    Ok(())
}

/// Melepas kunci saat keluar dari cakupan, juga kalau fungsi di dalamnya gagal.
pub struct Terkunci<'a>(&'a Arc<Kunci>);

impl<'a> Terkunci<'a> {
    pub fn baru(k: &'a Arc<Kunci>, siapa: &str) -> Result<Self, String> {
        kunci(k, siapa)?;
        Ok(Terkunci(k))
    }
}

impl Drop for Terkunci<'_> {
    fn drop(&mut self) { let _ = lepas(self.0, "mutex::with_lock"); }
}
