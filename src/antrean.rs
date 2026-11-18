// src/antrean.rs — antrean channel, SATU definisi untuk dua mesin

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// Isi bersama sebuah antrean. `closed` adalah BENDERA, bukan tebakan dari
/// hitungan rujukan — itu bedanya yang paling penting dengan `mpsc`.
#[derive(Debug)]
pub struct Isi<T> {
    pub buf: VecDeque<T>,
    pub kapasitas: usize,
    pub tertutup: bool,
    // Untuk `kapasitas == 0`: berapa yang sedang menunggu di tiap sisi.
    pub penunggu_terima: usize,
    pub penunggu_kirim: usize,
}

/// Kenapa `recv` menjawab `Option` dan bukan `T`: sebuah antrean yang sudah
/// DITUTUP dan KOSONG tidak punya nilai untuk diberikan, dan mengarang satu
/// adalah persis hasil salah yang diam yang proyek ini larang.
#[derive(Debug)]
pub struct Antrean<T> {
    isi: Arc<Mutex<Isi<T>>>,
    tak_penuh: Arc<Condvar>,
    tak_kosong: Arc<Condvar>,
    temu: Arc<Condvar>,
}

// `derive(Clone)` akan menuntut `T: Clone`, padahal yang disalin cuma `Arc`.
impl<T> Clone for Antrean<T> {
    fn clone(&self) -> Self {
        Antrean {
            isi: Arc::clone(&self.isi),
            tak_penuh: Arc::clone(&self.tak_penuh),
            tak_kosong: Arc::clone(&self.tak_kosong),
            temu: Arc::clone(&self.temu),
        }
    }
}

impl<T> Antrean<T> {
    pub fn new(kapasitas: usize) -> Self {
        Antrean {
            isi: Arc::new(Mutex::new(Isi {
                buf: VecDeque::new(),
                kapasitas,
                tertutup: false,
                penunggu_terima: 0,
                penunggu_kirim: 0,
            })),
            tak_penuh: Arc::new(Condvar::new()),
            tak_kosong: Arc::new(Condvar::new()),
            temu: Arc::new(Condvar::new()),
        }
    }

    /// Dua antrean yang menunjuk penyimpanan yang SAMA. Dipakai `select` dan
    /// kontrak "sebuah future menyerahkan nilainya sekali".
    pub fn sama(&self, lain: &Antrean<T>) -> bool {
        Arc::ptr_eq(&self.isi, &lain.isi)
    }

    // ── Kirim ────────────────────────────────────────────────────────────

    pub fn kirim(&self, nilai: T) -> Result<(), String> {
        buntu::geser();
        let mut jaga = None;
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        if g.tertutup {
            return Err(PESAN_KIRIM_TERTUTUP.to_string());
        }
        if g.kapasitas == 0 {
            g.penunggu_kirim += 1;
            // Tunggu sampai ada penerima, lalu taruh dan tunggu ia mengambil.
            while g.buf.is_empty() && g.penunggu_terima == 0 && !g.tertutup {
                g = match self.tunggu(&self.temu, g, &mut jaga) { Ok(g) => g, Err(e) => return Err(e) };
            }
            if g.tertutup {
                g.penunggu_kirim -= 1;
                return Err(PESAN_KIRIM_TERTUTUP.to_string());
            }
            g.buf.push_back(nilai);
            // Berhenti dihitung SEKARANG, bukan sesudah nilainya diambil: `penunggu_kirim`
            // berarti "masih menunggu penerima muncul", dan `coba_terima` bergantung pada itu.
            g.penunggu_kirim -= 1;
            self.temu.notify_all();
            self.tak_kosong.notify_all();
            while !g.buf.is_empty() && !g.tertutup {
                g = match self.tunggu(&self.temu, g, &mut jaga) { Ok(g) => g, Err(e) => return Err(e) };
            }
            return Ok(());
        }
        while g.buf.len() >= g.kapasitas && !g.tertutup {
            g = self.tunggu(&self.tak_penuh, g, &mut jaga)?;
        }
        if g.tertutup {
            return Err(PESAN_KIRIM_TERTUTUP.to_string());
        }
        g.buf.push_back(nilai);
        self.tak_kosong.notify_one();
        Ok(())
    }

    /// `Ok(true)` terkirim, `Ok(false)` akan memblokir, `Err` sudah ditutup.
    pub fn coba_kirim(&self, nilai: T) -> Result<bool, String> {
        buntu::geser();
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        if g.tertutup {
            return Err(PESAN_KIRIM_TERTUTUP.to_string());
        }
        if g.kapasitas == 0 {
            if g.penunggu_terima == 0 {
                return Ok(false);
            }
            g.buf.push_back(nilai);
            self.temu.notify_all();
            self.tak_kosong.notify_all();
            return Ok(true);
        }
        if g.buf.len() >= g.kapasitas {
            return Ok(false);
        }
        g.buf.push_back(nilai);
        self.tak_kosong.notify_one();
        Ok(true)
    }

    // ── Terima ───────────────────────────────────────────────────────────

    /// `None` berarti DITUTUP DAN KOSONG — tidak akan pernah ada nilai lagi.
    /// Isi yang sudah terkirim tetap dikuras lebih dulu: menutup berarti tidak
    /// ada lagi yang DIKIRIM, bukan membuang yang sudah terkirim.
    pub fn terima(&self) -> Option<T> {
        match self.terima_cek() { Ok(v) => v, Err(_) => None }
    }

    /// `terima` yang di thread utama mesin berdeteksi bisa gagal dengan `PESAN_BUNTU`.
    pub fn terima_cek(&self) -> Result<Option<T>, String> {
        buntu::geser();
        let mut jaga = None;
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        if g.kapasitas == 0 {
            g.penunggu_terima += 1;
            self.temu.notify_all();
            while g.buf.is_empty() && !g.tertutup {
                g = self.tunggu(&self.temu, g, &mut jaga)?;
            }
            g.penunggu_terima -= 1;
            let keluar = g.buf.pop_front();
            self.temu.notify_all();
            self.tak_penuh.notify_one();
            return Ok(keluar);
        }
        loop {
            if let Some(v) = g.buf.pop_front() {
                self.tak_penuh.notify_one();
                return Ok(Some(v));
            }
            if g.tertutup {
                return Ok(None);
            }
            g = self.tunggu(&self.tak_kosong, g, &mut jaga)?;
        }
    }

    /// `Ok(v)` dapat, `Err(true)` belum ada isi, `Err(false)` ditutup & kosong.
    /// Dua kegagalan itu dipisah karena artinya berbeda bagi pemanggilnya:
    /// yang satu "coba lagi nanti", yang satu "jangan coba lagi".
    pub fn coba_terima(&self) -> Result<T, bool> {
        buntu::geser();
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(v) = g.buf.pop_front() {
            self.temu.notify_all();
            self.tak_penuh.notify_one();
            return Ok(v);
        }
        if g.tertutup {
            return Err(false);
        }
        // Unbuffered: daftarkan diri sebentar supaya pengirim yang menunggu
        // tahu ada penerima, lalu lihat sekali lagi TANPA memblokir.
        if g.kapasitas == 0 && g.penunggu_kirim > 0 {
            g.penunggu_terima += 1;
            self.temu.notify_all();
            // Selama masih ada pengirim yang belum menaruh nilainya, ia PASTI akan menaruh
            // (ia melihat kita): tunggu itu, bukan sembarang kabar yang mungkin tak pernah datang.
            while g.buf.is_empty() && !g.tertutup && g.penunggu_kirim > 0 {
                g = self.temu.wait(g).unwrap_or_else(|p| p.into_inner());
            }
            g.penunggu_terima -= 1;
            if let Some(v) = g.buf.pop_front() {
                self.temu.notify_all();
                return Ok(v);
            }
        }
        Err(true)
    }

    // ── Keadaan ──────────────────────────────────────────────────────────

    /// Menandai tertutup dan membangunkan semua yang menunggu. Isinya TIDAK
    /// dibuang — versi sebelumnya mengosongkan buffer di sini, dan sepuluh
    /// nilai yang sudah terkirim jadi satu yang diterima, tanpa satu pun galat.
    pub fn tutup(&self) {
        buntu::geser();
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        g.tertutup = true;
        drop(g);
        self.tak_kosong.notify_all();
        self.tak_penuh.notify_all();
        self.temu.notify_all();
    }

    pub fn tertutup(&self) -> bool {
        self.isi.lock().unwrap_or_else(|p| p.into_inner()).tertutup
    }

    pub fn panjang(&self) -> usize {
        self.isi.lock().unwrap_or_else(|p| p.into_inner()).buf.len()
    }

    pub fn kapasitas(&self) -> usize {
        self.isi.lock().unwrap_or_else(|p| p.into_inner()).kapasitas
    }

    /// Melihat, bukan mengambil.
    pub fn ada_isi(&self) -> bool {
        let g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        !g.buf.is_empty() || g.tertutup
    }
}

/// Satu kalimat, dipakai dua mesin.
pub const PESAN_KIRIM_TERTUTUP: &str = "channel sudah ditutup: tidak bisa kirim lagi";
pub const PESAN_TERIMA_TERTUTUP: &str =
    "channel sudah ditutup dan kosong: tidak akan ada nilai lagi";

/// Kalimat kebuntuan, satu definisi untuk tiga mesin (LLVM: `korutin.c::jrg_ko_buntu`).
pub const PESAN_BUNTU: &str =
    "kebuntuan: semua goroutine sedang menunggu channel atau kunci dan tidak ada yang bisa melanjutkan";

/// Deteksi kebuntuan untuk mesin ber-thread (`dokumentasi/antrean-fase.md`, "Rancangan: kebuntuan").
/// Mati sampai sebuah mesin memanggil `aktifkan()`; aktor = thread yang menjalankan kode JRG dan dihitung.
pub mod buntu {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::SeqCst};
    use std::sync::OnceLock;
    use std::time::Duration;

    static AKTIF: AtomicBool = AtomicBool::new(false);
    static HIDUP: AtomicUsize = AtomicUsize::new(0);
    static TERBLOKIR: AtomicUsize = AtomicUsize::new(0);
    static GENERASI: AtomicU64 = AtomicU64::new(0);
    static UTAMA: OnceLock<std::thread::ThreadId> = OnceLock::new();
    /// Tenggang sebelum dinyatakan buntu: dua pemeriksaan berturut tanpa perubahan apa pun.
    pub const TIAP: Duration = Duration::from_millis(100);

    /// Dipanggil sekali oleh thread utama sebuah mesin sebelum `main` jalan.
    pub fn aktifkan() {
        if UTAMA.set(std::thread::current().id()).is_ok() {
            HIDUP.store(1, SeqCst);
            AKTIF.store(true, SeqCst);
        }
    }
    pub fn aktif() -> bool { AKTIF.load(SeqCst) }
    pub fn geser() { GENERASI.fetch_add(1, SeqCst); }
    pub fn utama() -> bool { UTAMA.get() == Some(&std::thread::current().id()) }

    /// Aktor baru — dibuat PEMBUAT thread sebelum `spawn` dan dipindah ke dalamnya; lepas = aktor selesai.
    pub struct Aktor(bool);
    pub fn lahir() -> Aktor {
        let a = aktif();
        if a { HIDUP.fetch_add(1, SeqCst); geser(); }
        Aktor(a)
    }
    impl Drop for Aktor {
        fn drop(&mut self) { if self.0 { HIDUP.fetch_sub(1, SeqCst); geser(); } }
    }

    /// Satu kali menunggu tanpa tenggat sedang berlangsung.
    pub struct Blokir { gen_awal: u64, stabil: u32 }
    impl Blokir {
        pub fn mulai() -> Blokir { TERBLOKIR.fetch_add(1, SeqCst); geser(); Blokir { gen_awal: GENERASI.load(SeqCst), stabil: 0 } }
        /// Dipanggil tiap tenggat `TIAP` habis tanpa dibangunkan: benar kalau programnya pasti buntu.
        pub fn buntu(&mut self) -> bool {
            let g = GENERASI.load(SeqCst);
            if g != self.gen_awal || TERBLOKIR.load(SeqCst) < HIDUP.load(SeqCst) {
                self.gen_awal = g;
                self.stabil = 0;
                return false;
            }
            self.stabil += 1;
            self.stabil >= 2
        }
    }
    impl Drop for Blokir {
        fn drop(&mut self) { TERBLOKIR.fetch_sub(1, SeqCst); geser(); }
    }

    /// `join` milik main atas aktor lain: buntu kalau SEMUA aktor lain terblokir dan tidak ada perubahan.
    pub fn sisanya_buntu(gen_awal: &mut u64, stabil: &mut u32) -> bool {
        let g = GENERASI.load(SeqCst);
        let lain = HIDUP.load(SeqCst).saturating_sub(1);
        if g != *gen_awal || lain == 0 || TERBLOKIR.load(SeqCst) < lain { *gen_awal = g; *stabil = 0; return false; }
        *stabil += 1;
        *stabil >= 2
    }
    pub fn generasi() -> u64 { GENERASI.load(SeqCst) }
}

impl<T> Antrean<T> {
    /// `cv.wait(g)`, yang di thread utama mesin berdeteksi berhenti dengan `PESAN_BUNTU` kalau programnya buntu.
    fn tunggu<'a>(&self, cv: &Condvar, g: MutexGuard<'a, Isi<T>>, jaga: &mut Option<buntu::Blokir>)
        -> Result<MutexGuard<'a, Isi<T>>, String>
    {
        if !buntu::aktif() { return Ok(cv.wait(g).unwrap_or_else(|p| p.into_inner())); }
        let b = jaga.get_or_insert_with(buntu::Blokir::mulai);
        let (g, habis) = cv.wait_timeout(g, buntu::TIAP).unwrap_or_else(|p| p.into_inner());
        if habis.timed_out() && b.buntu() && buntu::utama() { return Err(PESAN_BUNTU.to_string()); }
        Ok(g)
    }
}

#[cfg(test)]
mod uji_penjaga {
    /// Thread lepas yang tidak menjalankan kode JRG; alasannya di sampingnya.
    const BUKAN_AKTOR: &[(&str, &str)] = &[
        ("src/stdlib/regex_inti.rs", "mengompilasi regex di stack besar, menunggu hasilnya"),
        ("src/stdlib/proses.rs", "pipa stdin/stdout program lain"),
    ];
    /// Situs VM yang belum menghitung aktornya; dikerjakan Agus. Entri yang sudah ber-`lahir` membuat tes ini merah.
    const MENUNGGU_VM: &[&str] = &[];

    fn berkas(dir: &std::path::Path, keluar: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { berkas(&p, keluar) } else if p.extension().is_some_and(|x| x == "rs") { keluar.push(p) }
        }
    }

    /// Thread lepas yang menjalankan kode JRG wajib `buntu::lahir()` di pembuatnya, atau kebuntuan dituduh palsu.
    #[test]
    fn tiap_thread_jrg_dihitung_aktor() {
        let akar = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut semua = Vec::new();
        for d in ["src/vm", "src/interpreter", "src/stdlib"] { berkas(&akar.join(d), &mut semua) }
        let (mut tanpa, mut basi) = (Vec::new(), Vec::new());
        for p in semua {
            let rel = p.strip_prefix(akar).unwrap().to_string_lossy().replace('\\', "/");
            let teks = std::fs::read_to_string(&p).unwrap();
            let baris: Vec<&str> = teks.lines().collect();
            let mut ada_lahir = false;
            for (i, b) in baris.iter().enumerate() {
                let t = b.trim_start();
                if t.starts_with("//") || !(t.contains("thread::Builder::new()") || t.contains("thread::spawn(")) {
                    continue;
                }
                let sesudah = baris[i..(i + 4).min(baris.len())].join("\n");
                if sesudah.contains("spawn_scoped") { continue }
                let sebelum = baris[i.saturating_sub(15)..i].join("\n");
                if sebelum.contains("buntu::lahir") { ada_lahir = true; continue }
                if BUKAN_AKTOR.iter().any(|(f, _)| *f == rel) || MENUNGGU_VM.contains(&rel.as_str()) { continue }
                tanpa.push(format!("{}:{}", rel, i + 1));
            }
            if ada_lahir && MENUNGGU_VM.contains(&rel.as_str()) { basi.push(rel) }
        }
        assert!(tanpa.is_empty(), "thread lepas tanpa `buntu::lahir()` di pembuatnya: {:?}", tanpa);
        assert!(basi.is_empty(), "KABAR BAIK: sudah menghitung aktor, hapus dari MENUNGGU_VM: {:?}", basi);
    }
}
