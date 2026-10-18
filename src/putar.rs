// src/putar.rs — tanda hidup untuk langkah yang lama (cargo, unduhan, penanda tangan): tanpa ini
// `jrgm apk` diam belasan menit dan terlihat seperti macet. Di terminal sebuah baris berputar
// dengan waktu berjalan; bukan terminal (IDE, pipa, CI) cuma satu baris pembuka per langkah.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BINGKAI: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// `12 dtk`, `3 mnt 05 dtk`.
pub fn lama(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 { format!("{} dtk", s) } else { format!("{} mnt {:02} dtk", s / 60, s % 60) }
}

/// Satu langkah yang sedang berjalan. Dibuang = baris putarnya dihapus.
pub struct Putaran {
    judul: String,
    mulai: Instant,
    rincian: Arc<Mutex<String>>,
    henti: Arc<AtomicBool>,
    utas: Option<std::thread::JoinHandle<()>>,
}

impl Putaran {
    /// Mulai menampilkan `judul` di stderr.
    pub fn mulai(judul: &str) -> Putaran {
        let rincian = Arc::new(Mutex::new(String::new()));
        let henti = Arc::new(AtomicBool::new(false));
        let mulai = Instant::now();
        let terminal = std::io::stderr().is_terminal() && std::env::var_os("JRG_TANPA_PUTAR").is_none();
        let utas = if terminal {
            let (j, r, h) = (judul.to_string(), rincian.clone(), henti.clone());
            Some(std::thread::spawn(move || {
                let lebar = (crate::term::ukuran().0 as usize).max(20);
                let mut i = 0;
                while !h.load(Ordering::Relaxed) {
                    let r = r.lock().map(|r| r.clone()).unwrap_or_default();
                    let mut baris = format!("{} {} ({})", BINGKAI[i % BINGKAI.len()], j, lama(mulai.elapsed()));
                    if !r.is_empty() { baris.push_str(" — "); baris.push_str(&r); }
                    let baris: String = baris.chars().take(lebar - 1).collect();
                    let _ = write!(std::io::stderr(), "\r\x1b[2K{}", baris);
                    let _ = std::io::stderr().flush();
                    i += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = write!(std::io::stderr(), "\r\x1b[2K");
            }))
        } else {
            let _ = writeln!(std::io::stderr(), "{} ...", judul);
            None
        };
        Putaran { judul: judul.to_string(), mulai, rincian, henti, utas }
    }

    /// Ganti keterangan kecil di belakang judul (mis. baris `Compiling …` terakhir dari cargo).
    pub fn rinci(&self, teks: &str) {
        if let Ok(mut r) = self.rincian.lock() { *r = teks.trim().to_string(); }
    }

    /// Hentikan dan tulis satu baris hasil kalau langkahnya cukup lama untuk diingat.
    pub fn selesai(mut self, berhasil: bool) {
        let ada_terminal = self.utas.is_some();
        self.hentikan();
        let d = self.mulai.elapsed();
        if ada_terminal && (d >= Duration::from_secs(2) || !berhasil) {
            let _ = writeln!(std::io::stderr(), "{} {} ({})", if berhasil { "selesai:" } else { "gagal:" }, self.judul, lama(d));
        }
    }

    fn hentikan(&mut self) {
        self.henti.store(true, Ordering::Relaxed);
        if let Some(u) = self.utas.take() { let _ = u.join(); }
    }
}

impl Drop for Putaran {
    fn drop(&mut self) { self.hentikan(); }
}

/// Jalankan `cmd` sampai selesai di bawah sebuah `Putaran`, keluarannya ditampung untuk kalimat
/// gagal. Baris stderr terakhir yang berbunyi (`Compiling x`, `Downloading y`) jadi rinciannya.
pub fn jalankan(cmd: &mut Command, judul: &str) -> std::io::Result<std::process::Output> {
    let p = Putaran::mulai(judul);
    let mut anak = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = anak.stdout.take();
    let pembaca_out = std::thread::spawn(move || {
        let mut v = Vec::new();
        if let Some(mut s) = stdout { let _ = std::io::Read::read_to_end(&mut s, &mut v); }
        v
    });
    let mut galat = Vec::new();
    if let Some(s) = anak.stderr.take() {
        let mut r = BufReader::new(s);
        let mut baris = Vec::new();
        while r.read_until(b'\n', &mut baris).map_or(false, |n| n > 0) {
            let teks = String::from_utf8_lossy(&baris);
            let t = teks.trim();
            if !t.is_empty() { p.rinci(t); }
            galat.extend_from_slice(&baris);
            baris.clear();
        }
    }
    let status = anak.wait()?;
    let stdout = pembaca_out.join().unwrap_or_default();
    p.selesai(status.success());
    Ok(std::process::Output { status, stdout, stderr: galat })
}
