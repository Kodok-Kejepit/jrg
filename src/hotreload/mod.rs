// src/hotreload/mod.rs
// Hot Reloading Engine

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

// ─── FNV-1a hash ─────────────────────────────────────────────────────────

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); }
    h
}

// ─── FileState ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct FileState { mtime: u64, checksum: u64 }

impl FileState {
    fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta.modified().ok()?
            .duration_since(UNIX_EPOCH).ok()?.as_secs();
        let content = std::fs::read(path).ok()?;
        Some(FileState { mtime, checksum: fnv1a(&content) })
    }
    fn changed(&self, other: &FileState) -> bool {
        self.checksum != other.checksum
    }
}

// ─── FileWatcher ──────────────────────────────────────────────────────────

struct FileWatcher {
    states:   HashMap<PathBuf, FileState>,
    interval: Duration,
}

impl FileWatcher {
    fn new() -> Self {
        FileWatcher { states: HashMap::new(), interval: Duration::from_millis(150) }
    }

    fn watch_dir(&mut self, dir: &Path) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && !hidden(&p) { self.watch_dir(&p); }
                else if p.extension().and_then(|x| x.to_str()) == Some("jrg") {
                    if let Some(s) = FileState::read(&p) { self.states.insert(p, s); }
                }
            }
        }
    }

    fn watch_single(&mut self, p: &Path) {
        if let Some(s) = FileState::read(p) { self.states.insert(p.to_path_buf(), s); }
    }

    fn refresh(&mut self, dir: &Path) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && !hidden(&p) { self.refresh(&p); }
                else if p.extension().and_then(|x| x.to_str()) == Some("jrg") {
                    if !self.states.contains_key(&p) {
                        if let Some(s) = FileState::read(&p) { self.states.insert(p, s); }
                    }
                }
            }
        }
    }

    /// Return list of changed paths
    fn poll(&mut self) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        for (path, old) in &mut self.states {
            if let Some(new) = FileState::read(path) {
                if new.changed(old) { *old = new; changed.push(path.clone()); }
            }
        }
        changed
    }

    fn count(&self) -> usize { self.states.len() }
}

fn hidden(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(false)
}

// ─── ReloadConfig ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ReloadConfig {
    pub entry:        PathBuf,
    pub watch_dir:    PathBuf,
    pub use_vm:       bool,
    pub profiling:    bool,
    pub emit_bc:      bool,
    pub clear_screen: bool,
    pub debounce_ms:  u64,
    pub search_paths: Vec<PathBuf>,
}

impl ReloadConfig {
    pub fn new(entry: PathBuf) -> Self {
        let dir = entry.parent().map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        ReloadConfig {
            watch_dir:    dir.clone(),
            search_paths: vec![dir],
            entry,
            use_vm:       false,
            profiling:    false,
            emit_bc:      false,
            clear_screen: false,
            debounce_ms:  150,
        }
    }
}

// ─── HotReloader ──────────────────────────────────────────────────────────

/// Biner `jrgc` yang dipakai menjalankan program.
pub fn biner_jrgc() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("JRGC_BIN") {
        if !p.is_empty() { return std::path::PathBuf::from(p); }
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            let c = dir.join(if cfg!(windows) { "jrgc.exe" } else { "jrgc" });
            if c.is_file() { return c; }
        }
    }
    std::path::PathBuf::from("jrgc")
}

pub struct HotReloader {
    cfg: ReloadConfig,
    reloads: u32,
    errors: u32,
    /// Proses program yang sedang berjalan.
    anak: Option<std::process::Child>,
    mulai: Option<Instant>,
}

impl HotReloader {
    pub fn new(cfg: ReloadConfig) -> Self {
        HotReloader { cfg, reloads: 0, errors: 0, anak: None, mulai: None }
    }

    pub fn run(&mut self) {
        let mut watcher = FileWatcher::new();
        watcher.watch_dir(&self.cfg.watch_dir.clone());
        if watcher.count() == 0 { watcher.watch_single(&self.cfg.entry.clone()); }

        print_banner(&self.cfg.entry, &self.cfg.watch_dir, watcher.count());

        // First run
        self.reload();

        let debounce = Duration::from_millis(self.cfg.debounce_ms);
        let mut last_change: Option<Instant> = None;

        loop {
            std::thread::sleep(watcher.interval);
            watcher.refresh(&self.cfg.watch_dir.clone());

            let changed = watcher.poll();
            if !changed.is_empty() {
                for p in &changed {
                    let rel = p.strip_prefix(&self.cfg.watch_dir).unwrap_or(p);
                    println!("\n{} {}",
                        bold_yellow("[jrg:watch] ✦"), bold(&rel.display().to_string()));
                }
                last_change = Some(Instant::now());
            }

            if let Some(t) = last_change {
                if t.elapsed() >= debounce {
                    last_change = None;
                    self.reload();
                }
            }

            // DIPERIKSA, bukan ditunggu.
            if self.periksa_anak() {
                println!("\n{}", dim("[jrg:watch] Menunggu perubahan... (Ctrl+C untuk keluar)"));
            }
        }
    }

    fn reload(&mut self) {
        // Proses sebelumnya DIHENTIKAN dulu. Tanpa ini sebuah program yang
        // memblokir (server) menumpuk satu proses per simpan, dan yang kedua
        // gagal bind ke port yang masih dipegang yang pertama.
        self.hentikan_anak();

        self.reloads += 1;
        if self.cfg.clear_screen { print!("\x1b[2J\x1b[H"); }
        println!("{} #{}", bold_cyan("[jrg:watch] ↺ Reload"), self.reloads);

        let mut cmd = std::process::Command::new(biner_jrgc());
        cmd.arg(&self.cfg.entry);
        if self.cfg.use_vm    { cmd.arg("--vm"); }
        if self.cfg.profiling { cmd.arg("--profile"); }
        if self.cfg.emit_bc   { cmd.arg("--emit-bc"); }

        self.mulai = Some(Instant::now());
        match cmd.spawn() {
            Ok(anak) => self.anak = Some(anak),
            Err(e) => {
                self.errors += 1;
                self.mulai = None;
                println!("{}", bold_red("[jrg:watch] ✘ Tidak bisa menjalankan program:"));
                println!("  {} ({})", e, biner_jrgc().display());
                println!("{}", dim("[jrg:watch]   → set `JRGC_BIN` kalau `jrgc` ada di tempat lain"));
            }
        }
    }

    /// Hentikan program yang sedang berjalan, kalau ada.
    fn hentikan_anak(&mut self) {
        if let Some(mut a) = self.anak.take() {
            if matches!(a.try_wait(), Ok(None)) {
                println!("{}", dim("[jrg:watch]   → menghentikan proses sebelumnya"));
                let _ = a.kill();
            }
            let _ = a.wait();
        }
        self.mulai = None;
    }

    /// Laporkan sekali kalau programnya sudah selesai sendiri.
    fn periksa_anak(&mut self) -> bool {
        let Some(a) = self.anak.as_mut() else { return false };
        match a.try_wait() {
            Ok(Some(st)) => {
                let ms = self.mulai.map(|t| t.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0);
                self.anak = None;
                self.mulai = None;
                if st.success() {
                    println!("{} dalam {:.2}ms", bold_green("[jrg:watch] ✔ Selesai"), ms);
                } else {
                    self.errors += 1;
                    // Diagnostiknya sudah dicetak `jrgc` sendiri, lengkap dengan kutipan sumber
                    // dan caret — jauh lebih berguna daripada `format!("parse: {}", e.msg)` yang …
                    println!("{} (kode keluar {})",
                        bold_red("[jrg:watch] ✘ Berhenti dengan error"),
                        st.code().map(|c| c.to_string()).unwrap_or_else(|| "sinyal".to_string()));
                    println!("{}", dim("[jrg:watch]   → perbaiki, simpan untuk reload..."));
                }
                true
            }
            _ => false,
        }
    }
}

impl Drop for HotReloader {
    /// Pengawas yang berhenti tidak boleh meninggalkan programnya berjalan —
    /// sebuah server yang bocor menahan portnya dan membuat jalan berikutnya
    /// gagal karena alasan yang tidak ada hubungannya.
    fn drop(&mut self) {
        self.hentikan_anak();
    }
}

// ─── Public API ───────────────────────────────────────────────────────────

pub fn watch(cfg: ReloadConfig) {
    HotReloader::new(cfg).run();
}

// ─── Terminal colors ──────────────────────────────────────────────────────

fn bold(s: &str)        -> String { format!("\x1b[1m{}\x1b[0m", s) }
fn dim(s: &str)         -> String { format!("\x1b[2m{}\x1b[0m", s) }
fn bold_green(s: &str)  -> String { format!("\x1b[1;32m{}\x1b[0m", s) }
fn bold_red(s: &str)    -> String { format!("\x1b[1;31m{}\x1b[0m", s) }
fn bold_yellow(s: &str) -> String { format!("\x1b[1;33m{}\x1b[0m", s) }
fn bold_cyan(s: &str)   -> String { format!("\x1b[1;36m{}\x1b[0m", s) }

fn print_banner(entry: &Path, dir: &Path, n: usize) {
    println!("{}", bold_cyan("╔══════════════════════════════════════════════╗"));
    println!("{}", bold_cyan("║       JawaRustGo++ — Hot Reload Mode         ║"));
    println!("{}", bold_cyan("╚══════════════════════════════════════════════╝"));
    println!("  Entry : {}", bold(&entry.display().to_string()));
    println!("  Watch : {} ({} files)", dir.display(), n);
}

#[cfg(test)]
mod tes {
    use super::*;

    fn ruang(nama: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jrg-hr-{}-{}-{}", nama, std::process::id(),
            std::time::SystemTime::now().duration_since(UNIX_EPOCH)
                .map(|x| x.as_nanos()).unwrap_or(0)));
        std::fs::create_dir_all(&d).expect("bikin dir");
        d
    }

    /// Perubahan dikenali dari ISI, bukan dari `mtime`.
    #[test]
    fn perubahan_dikenali_dari_isi_bukan_waktu() {
        let d = ruang("isi");
        let f = d.join("a.jrg");
        std::fs::write(&f, "package main\n").unwrap();

        let mut w = FileWatcher::new();
        w.watch_dir(&d);
        assert_eq!(w.count(), 1, "berkas .jrg tidak terdaftar");
        assert!(w.poll().is_empty(), "belum ada perubahan, tapi sudah dilaporkan");

        // Tulis ulang isi yang SAMA: `mtime` bergerak, isinya tidak.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&f, "package main\n").unwrap();
        assert!(
            w.poll().is_empty(),
            "isi tidak berubah tapi dilaporkan berubah — tiap `touch` akan memicu reload"
        );

        std::fs::write(&f, "package main\n// beda\n").unwrap();
        assert_eq!(w.poll().len(), 1, "isi berubah tapi tidak dilaporkan");

        let _ = std::fs::remove_dir_all(&d);
    }

    /// Berkas yang BARU muncul ikut diawasi. Menambah berkas ke sebuah proyek
    /// adalah hal biasa, dan pengawas yang cuma melihat daftar awal akan
    /// mengabaikannya selamanya.
    #[test]
    fn berkas_baru_ikut_diawasi() {
        let d = ruang("baru");
        std::fs::write(d.join("a.jrg"), "package main\n").unwrap();
        let mut w = FileWatcher::new();
        w.watch_dir(&d);
        assert_eq!(w.count(), 1);

        std::fs::write(d.join("b.jrg"), "package b\n").unwrap();
        w.refresh(&d);
        assert_eq!(w.count(), 2, "berkas baru tidak ikut terdaftar");

        std::fs::write(d.join("b.jrg"), "package b\n// beda\n").unwrap();
        assert_eq!(w.poll().len(), 1, "perubahan di berkas baru tidak terlihat");

        let _ = std::fs::remove_dir_all(&d);
    }

    /// Subdirektori diawasi, direktori tersembunyi tidak. `.git/` berisi
    /// ribuan berkas yang berubah terus dan tidak satu pun milik programnya.
    #[test]
    fn subdirektori_diawasi_yang_tersembunyi_tidak() {
        let d = ruang("sub");
        std::fs::create_dir_all(d.join("dalam")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("a.jrg"), "package main\n").unwrap();
        std::fs::write(d.join("dalam/b.jrg"), "package b\n").unwrap();
        std::fs::write(d.join(".git/c.jrg"), "package c\n").unwrap();
        // Bukan `.jrg`: tidak diawasi sama sekali.
        std::fs::write(d.join("catatan.txt"), "halo\n").unwrap();

        let mut w = FileWatcher::new();
        w.watch_dir(&d);
        assert_eq!(
            w.count(), 2,
            "harusnya `a.jrg` dan `dalam/b.jrg` saja — `.git/` dan `.txt` tidak ikut"
        );

        let _ = std::fs::remove_dir_all(&d);
    }
}
