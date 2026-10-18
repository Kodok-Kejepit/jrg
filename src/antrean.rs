// src/antrean.rs — antrean channel, SATU definisi untuk dua mesin

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

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
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        if g.tertutup {
            return Err(PESAN_KIRIM_TERTUTUP.to_string());
        }
        if g.kapasitas == 0 {
            g.penunggu_kirim += 1;
            // Tunggu sampai ada penerima, lalu taruh dan tunggu ia mengambil.
            while g.buf.is_empty() && g.penunggu_terima == 0 && !g.tertutup {
                g = self.temu.wait(g).unwrap_or_else(|p| p.into_inner());
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
                g = self.temu.wait(g).unwrap_or_else(|p| p.into_inner());
            }
            return Ok(());
        }
        while g.buf.len() >= g.kapasitas && !g.tertutup {
            g = self.tak_penuh.wait(g).unwrap_or_else(|p| p.into_inner());
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
        let mut g = self.isi.lock().unwrap_or_else(|p| p.into_inner());
        if g.kapasitas == 0 {
            g.penunggu_terima += 1;
            self.temu.notify_all();
            while g.buf.is_empty() && !g.tertutup {
                g = self.temu.wait(g).unwrap_or_else(|p| p.into_inner());
            }
            g.penunggu_terima -= 1;
            let keluar = g.buf.pop_front();
            self.temu.notify_all();
            self.tak_penuh.notify_one();
            return keluar;
        }
        loop {
            if let Some(v) = g.buf.pop_front() {
                self.tak_penuh.notify_one();
                return Some(v);
            }
            if g.tertutup {
                return None;
            }
            g = self.tak_kosong.wait(g).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// `Ok(v)` dapat, `Err(true)` belum ada isi, `Err(false)` ditutup & kosong.
    /// Dua kegagalan itu dipisah karena artinya berbeda bagi pemanggilnya:
    /// yang satu "coba lagi nanti", yang satu "jangan coba lagi".
    pub fn coba_terima(&self) -> Result<T, bool> {
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
