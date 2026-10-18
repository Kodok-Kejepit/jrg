// src/profiler/mod.rs
// Built-in Profiler: flag --profile

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::time::Instant;

// ─── Data Structures ─────────────────────────────────────────────────────────

/// Statistik untuk satu fungsi JRG.
#[derive(Debug, Clone)]
pub struct FnStat {
    /// Nama fungsi.
    pub name: String,
    /// Berapa kali dipanggil.
    pub calls: u64,
    /// Total waktu termasuk callees (wall time).
    pub total_ns: u64,
    /// Waktu murni fungsi ini (total − waktu yang dihabiskan di callees).
    pub self_ns: u64,
    /// Panggilan TERCEPAT dan TERLAMBAT (self-time), dalam nanodetik.
    pub min_ns: u64,
    pub max_ns: u64,
    /// Kedalaman rekursi terdalam yang pernah tercapai fungsi ini.
    /// `1` berarti tidak pernah rekursif.
    pub kedalaman: u32,
}

/// Frame per-fungsi di call stack profiler.
struct Frame {
    name:     String,
    start:    Instant,
    child_ns: u64,   // akumulasi waktu callees selama frame ini
}

/// Profiler utama — disimpan di thread-local.
#[derive(Default)]
pub struct Profiler {
    pub enabled: bool,
    stats:       HashMap<String, FnStat>,
    stack:       Vec<Frame>,
}

impl Profiler {
    pub fn new(enabled: bool) -> Self {
        Profiler { enabled, ..Default::default() }
    }

    /// Dipanggil saat interpreter masuk ke fungsi JRG.
    pub fn enter(&mut self, name: &str) {
        if !self.enabled { return; }
        // Tambah call count
        let e = self.stats.entry(name.to_string()).or_insert_with(|| FnStat {
            name: name.to_string(),
            calls: 0,
            total_ns: 0,
            self_ns: 0,
            min_ns: u64::MAX,
            max_ns: 0,
            kedalaman: 0,
        });
        e.calls += 1;
        // Kedalaman = berapa frame fungsi INI yang sedang aktif sekaligus.
        let aktif = self.stack.iter().filter(|f| f.name == name).count() as u32 + 1;
        if aktif > e.kedalaman { e.kedalaman = aktif; }
        // Push frame ke stack
        self.stack.push(Frame {
            name:     name.to_string(),
            start:    Instant::now(),
            child_ns: 0,
        });
    }

    /// Dipanggil saat interpreter keluar dari fungsi JRG (termasuk via error/return).
    pub fn exit(&mut self) {
        if !self.enabled { return; }
        if let Some(frame) = self.stack.pop() {
            let elapsed_ns = frame.start.elapsed().as_nanos() as u64;
            let self_ns    = elapsed_ns.saturating_sub(frame.child_ns);

            if let Some(stat) = self.stats.get_mut(&frame.name) {
                stat.total_ns += elapsed_ns;
                stat.self_ns  += self_ns;
                if self_ns < stat.min_ns { stat.min_ns = self_ns; }
                if self_ns > stat.max_ns { stat.max_ns = self_ns; }
            }

            // Propagate elapsed ke parent frame sebagai child time
            if let Some(parent) = self.stack.last_mut() {
                parent.child_ns += elapsed_ns;
            }
        }
    }

    /// Cetak laporan profiling ke stdout.
    pub fn report(&self) {
        if !self.enabled || self.stats.is_empty() {
            return;
        }

        let use_color = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        let c = |code: &'static str| ansi(code, use_color);
        let reset = c("\x1b[0m");
        let dim   = c("\x1b[2m");
        let bold  = c("\x1b[1m");
        let cyan  = c("\x1b[1;36m");
        let amber = c("\x1b[1;33m");

        let mut rows: Vec<&FnStat> = self.stats.values().collect();
        rows.sort_by(|a, b| b.self_ns.cmp(&a.self_ns));

        let total_self_ns: u64 = rows.iter().map(|r| r.self_ns).sum();
        let total_wall_ns: u64 = rows.iter().map(|r| r.total_ns).max().unwrap_or(0);
        let total_calls:   u64 = rows.iter().map(|r| r.calls).sum();
        let accounted_pct = if total_wall_ns > 0 {
            total_self_ns as f64 / total_wall_ns as f64 * 100.0
        } else {
            0.0
        };

        // ── Header ──
        println!();
        println!("  {cyan}░▒▓{reset} {bold}JRG PROFILER REPORT{reset} {cyan}▓▒░{reset}");
        println!(
            "  {dim}{} fungsi · {} calls · self-time {}{reset}",
            rows.len(), fmt_calls(total_calls), fmt_dur(total_self_ns)
        );
        println!();

        // ── Table ──
        println!(
            "  {dim}{:<22}  {:>8}  {:>10}  {:>10}  {:>9}   {}{reset}",
            "Function", "Calls", "Cumulative", "Self", "Avg/call", "Self %"
        );
        let rule = "─".repeat(92);
        println!("  {dim}{rule}{reset}");

        for row in &rows {
            let pct = if total_self_ns > 0 {
                row.self_ns as f64 / total_self_ns as f64 * 100.0
            } else {
                0.0
            };
            let avg_ns = row.self_ns / row.calls.max(1);
            let sev = severity_color(pct, use_color);

            println!(
                "  {sev}{}{reset}  {:>8}  {:>10}  {:>10}  {:>9}   {} {sev}{:>5.1}%{reset}",
                truncate_name(&row.name, 22),
                fmt_calls(row.calls),
                fmt_dur(row.total_ns),
                fmt_dur(row.self_ns),
                fmt_dur(avg_ns),
                bar(pct, sev, dim, reset),
                pct,
            );
        }

        // ── Footer ──
        println!("  {dim}{rule}{reset}");
        println!(
            "  {bold}{:<22}  {:>8}  {:>10}  {:>10}{reset}",
            "TOTAL", fmt_calls(total_calls), fmt_dur(total_wall_ns), fmt_dur(total_self_ns)
        );

        // "Cumulative"/TOTAL bisa jauh lebih besar dari waktu proses sesungguhnya
        // untuk fungsi rekursif dalam — itu BUKAN salah baca atau bug tampilan: …
        if accounted_pct < 50.0 {
            println!(
                "  {dim}Catatan: `Cumulative` besar untuk fungsi rekursif dalam itu wajar (akumulasi\n  \
                 berlapis tiap level rekursi, bukan waktu dinding berurutan) — pakai `Self` buat analisis.{reset}"
            );
        }
        println!();

        // ── Bottleneck insight ──
        if let Some(hottest) = rows.first() {
            let hot_pct = hottest.self_ns as f64 / total_self_ns.max(1) as f64 * 100.0;
            if hot_pct > 50.0 {
                let avg = hottest.self_ns / hottest.calls.max(1);
                println!(
                    "  {amber}⚠  BOTTLENECK{reset}  {bold}{}{reset} menyumbang {bold}{:.1}%{reset} dari total self-time \
                     — dipanggil {}× (rata-rata {}/call)",
                    hottest.name, hot_pct, fmt_calls(hottest.calls), fmt_dur(avg)
                );
                if hottest.calls > 500 && avg > 10_000 {
                    println!(
                        "  {dim}   Banyak panggilan kecil ke fungsi rekursif — kandidat kuat untuk memoization \
                         atau iterative rewrite.{reset}"
                    );
                }
                println!();
            }
        }
    }
}

/// Cetak laporan lengkap.
fn cetak_laporan(stats: &[FnStat], dinding_ns: u64, utas: u32) {
    if stats.is_empty() {
        // Diam kalau memang tidak ada yang tercatat: mencetak tabel kosong
        // lebih membingungkan daripada tidak mencetak apa pun.
        return;
    }
    let use_color = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
    let c = |code: &'static str| ansi(code, use_color);
    let reset = c("\x1b[0m");
    let dim   = c("\x1b[2m");
    let bold  = c("\x1b[1m");
    let cyan  = c("\x1b[1;36m");
    let amber = c("\x1b[1;33m");

    let mut rows: Vec<&FnStat> = stats.iter().collect();
    rows.sort_by(|a, b| b.self_ns.cmp(&a.self_ns));

    let total_self_ns: u64 = rows.iter().map(|r| r.self_ns).sum();
    let total_cum_ns:  u64 = rows.iter().map(|r| r.total_ns).max().unwrap_or(0);
    let total_calls:   u64 = rows.iter().map(|r| r.calls).sum();
    let ada_rekursi = rows.iter().any(|r| r.kedalaman > 1);

    // ── Header ──
    println!();
    println!("  {cyan}░▒▓{reset} {bold}JRG PROFILER REPORT{reset} {cyan}▓▒░{reset}");
    // Dengan beberapa utas, jumlah self-time BISA melebihi waktu dinding — empat
    // utas yang bekerja satu detik menghabiskan empat detik CPU dalam satu detik …
    let paralel = utas > 1 && total_self_ns > dinding_ns;
    let bagian_dinding = if dinding_ns > 0 {
        if paralel {
            format!(
                " · dinding {} · paralel {:.1}×",
                fmt_dur(dinding_ns),
                total_self_ns as f64 / dinding_ns as f64
            )
        } else {
            format!(
                " · dinding {} · terukur {:.1}%",
                fmt_dur(dinding_ns),
                total_self_ns as f64 / dinding_ns as f64 * 100.0
            )
        }
    } else {
        String::new()
    };
    println!(
        "  {dim}{} fungsi · {} panggilan · {} utas · self-time {}{}{reset}",
        rows.len(), fmt_calls(total_calls), utas.max(1), fmt_dur(total_self_ns), bagian_dinding
    );
    println!();

    // ── Tabel ──
    const RULE: usize = 108;
    println!(
        "  {dim}{:<20}  {:>6}  {:>9}  {:>9}  {:>9}  {:>9}  {:>9}  {:<14} {}{reset}",
        "Fungsi", "Calls", "Cumulative", "Self", "Avg/call", "Min", "Max", "Sebaran", "Self %"
    );
    let rule = "─".repeat(RULE);
    println!("  {dim}{rule}{reset}");

    for row in &rows {
        let pct = if total_self_ns > 0 {
            row.self_ns as f64 / total_self_ns as f64 * 100.0
        } else { 0.0 };
        let avg_ns = row.self_ns / row.calls.max(1);
        let sev = severity_color(pct, use_color);
        // `min_ns` masih `u64::MAX` berarti fungsinya masuk tapi tidak
        // pernah keluar — mungkin karena program berhenti di dalamnya.
        // Ditampilkan sebagai `-`, bukan angka raksasa yang tampak nyata.
        let min_teks = if row.min_ns == u64::MAX { "-".to_string() } else { fmt_dur(row.min_ns) };
        let max_teks = if row.max_ns == 0 && row.min_ns == u64::MAX { "-".to_string() } else { fmt_dur(row.max_ns) };
        let nama = if row.kedalaman > 1 {
            format!("{} {}↻{}{}", truncate_name(&row.name, 17).trim_end(), dim, row.kedalaman, reset)
        } else {
            truncate_name(&row.name, 20)
        };
        // Lebar nama dihitung dari KARAKTER tampak, bukan panjang string —
        // kode warna ANSI tidak memakan kolom tapi tetap menambah byte.
        let tampak = if row.kedalaman > 1 {
            truncate_name(&row.name, 17).trim_end().chars().count() + 2 + row.kedalaman.to_string().len()
        } else { 20 };
        let padding = " ".repeat(20usize.saturating_sub(tampak));
        println!(
            "  {sev}{}{reset}{padding}  {:>6}  {:>9}  {:>9}  {:>9}  {:>9}  {:>9}  {} {sev}{:>5.1}%{reset}",
            nama,
            fmt_calls(row.calls),
            fmt_dur(row.total_ns),
            fmt_dur(row.self_ns),
            fmt_dur(avg_ns),
            min_teks,
            max_teks,
            bar(pct, sev, dim, reset),
            pct,
        );
    }

    println!("  {dim}{rule}{reset}");
    println!(
        "  {bold}{:<20}  {:>6}  {:>9}  {:>9}{reset}",
        "TOTAL", fmt_calls(total_calls), fmt_dur(total_cum_ns), fmt_dur(total_self_ns)
    );
    println!();

    // ── Bottleneck ──
    if let Some(hottest) = rows.first() {
        let hot_pct = hottest.self_ns as f64 / total_self_ns.max(1) as f64 * 100.0;
        if hot_pct > 50.0 {
            let avg = hottest.self_ns / hottest.calls.max(1);
            println!(
                "  {amber}⚠  BOTTLENECK{reset}  {bold}{}{reset} menyumbang {bold}{:.1}%{reset} dari total \
                 self-time — dipanggil {}× (rata-rata {}/call)",
                hottest.name, hot_pct, fmt_calls(hottest.calls), fmt_dur(avg)
            );
            if hottest.calls > 500 && avg > 10_000 {
                println!(
                    "  {dim}   Banyak panggilan dengan biaya besar — kandidat kuat untuk memoization \
                     atau penulisan ulang secara iteratif.{reset}"
                );
            }
            // Sebaran yang lebar jauh lebih menolong daripada rata-rata
            // waktu mencari SEBAB, jadi ia disebut kalau memang lebar.
            if hottest.min_ns != u64::MAX && hottest.max_ns > hottest.min_ns.saturating_mul(10) {
                println!(
                    "  {dim}   Sebarannya lebar ({} … {}) — biayanya tidak seragam, jadi lihat dulu \
                     masukan mana yang memicu yang lambat sebelum mengoptimalkan.{reset}",
                    fmt_dur(hottest.min_ns), fmt_dur(hottest.max_ns)
                );
            }
            println!();
        }
    }

    // ── Cara baca ──
    println!("  {bold}Cara baca{reset}");
    println!("  {dim}{}{reset}", "─".repeat(RULE));
    let baris = |k: &str, v: &str| println!("  {bold}{:<11}{reset}{}", k, v);
    baris("Self", "waktu DI DALAM fungsi itu sendiri, tidak termasuk fungsi yang dipanggilnya.");
    println!("             {dim}Ini angka yang dipakai mencari bottleneck.{reset}");
    baris("Cumulative", "Self ditambah seluruh waktu callee-nya.");
    if ada_rekursi {
        println!(
            "             {dim}Untuk fungsi rekursif (ditandai ↻ beserta kedalamannya), angkanya bisa\n             \
             melebihi waktu dinding: tiap level rekursi menambahkan rentangnya sendiri\n             \
             ke jumlah yang sama. Itu bukan salah baca — pakai `Self` untuk analisis.{reset}"
        );
    }
    baris("Min / Max", "panggilan tercepat dan terlambat (self-time).");
    println!(
        "             {dim}Jarak yang jauh berarti biayanya tidak seragam — sering lebih penting\n             \
         daripada rata-ratanya, karena rata-rata menyembunyikan yang sesekali mahal.{reset}"
    );
    baris("Sebaran", "proporsi Self fungsi ini terhadap total self-time semua fungsi.");
    baris("dinding", "waktu nyata dari profiler dinyalakan sampai program selesai.");
    println!(
        "             {dim}`terukur` = berapa persennya yang jatuh di dalam fungsi JRG. Sisanya\n             \
         dipakai hal yang tidak diprofilkan: startup, parsing, dan builtin.{reset}"
    );
    if paralel {
        println!(
            "  {bold}paralel{reset}    total self-time DIBAGI waktu dinding, saat programnya memakai\n             \
             {dim}beberapa utas. `{:.1}×` berarti kerjanya setara {:.1} kali waktu dinding —\n             \
             itu memang bisa, karena {} utas bekerja bersamaan. Angka ini menggantikan\n             \
             `terukur`, yang untuk program berutas banyak akan melebihi 100% dan\n             \
             terbaca seperti kesalahan.{reset}",
            total_self_ns as f64 / dinding_ns.max(1) as f64,
            total_self_ns as f64 / dinding_ns.max(1) as f64,
            utas
        );
    }
    baris("Satuan", "1s = 1.000ms · 1ms = 1.000µs · 1µs = 1.000ns.");
    println!(
        "             {dim}µs (mikrodetik) = sepersejuta detik; ns (nanodetik) = sepermiliar detik.\n             \
         Patokan di mesin ini: satu panggilan fungsi JRG kosong sekitar 1µs, jadi\n             \
         angka di bawah beberapa µs biasanya lebih banyak berisi biaya pengukuran\n             \
         itu sendiri daripada kerja yang sesungguhnya.{reset}"
    );
    println!();
}

/// Kembalikan `code` apa adanya kalau warna aktif, string kosong kalau tidak
/// — dipisah jadi fungsi (bukan closure) khusus supaya lifetime `'static`-nya …
fn ansi(code: &'static str, use_color: bool) -> &'static str {
    if use_color { code } else { "" }
}

/// Format angka dengan pemisah ribuan (`1234` → `1,234`).
fn fmt_calls(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format durasi dengan unit yang otomatis menyesuaikan skala (µs/ms/s) —
/// jauh lebih gampang dibaca sekilas daripada selalu memaksa milidetik
/// dengan banyak angka nol di depan atau di belakang koma.
fn fmt_dur(ns: u64) -> String {
    let ms = ns as f64 / 1_000_000.0;
    if ms >= 1000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else if ms >= 1.0 {
        format!("{:.3}ms", ms)
    } else {
        format!("{:.1}µs", ns as f64 / 1000.0)
    }
}

/// Potong nama fungsi ke `width` KARAKTER (bukan byte!) supaya tabel tetap
/// rapi — pakai `.chars()` karena nama fungsi JRG boleh mengandung identifier …
fn truncate_name(name: &str, width: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width {
        format!("{:<width$}", name, width = width)
    } else {
        let truncated: String = chars[..width.saturating_sub(1)].iter().collect();
        format!("{}…", truncated)
    }
}

/// Warna sesuai seberapa besar kontribusi fungsi ini ke total self-time:
/// merah (≥50%) = dominan, kuning (≥20%) = perlu perhatian, hijau (≥5%) = …
fn severity_color(pct: f64, use_color: bool) -> &'static str {
    if !use_color { return ""; }
    if pct >= 50.0 { "\x1b[1;31m" }
    else if pct >= 20.0 { "\x1b[1;33m" }
    else if pct >= 5.0 { "\x1b[1;32m" }
    else { "\x1b[2m" }
}

/// Bar dua-tona lebar tetap (20 karakter): bagian terisi (warna sesuai
/// severity) proporsional terhadap `pct`, sisanya redup — supaya proporsi …
fn bar(pct: f64, sev_color: &str, dim: &str, reset: &str) -> String {
    const WIDTH: usize = 20;
    let filled = ((pct / 100.0) * WIDTH as f64).round() as usize;
    let filled = filled.min(WIDTH);
    format!(
        "{sev_color}{}{reset}{dim}{}{reset}",
        "█".repeat(filled),
        "░".repeat(WIDTH - filled)
    )
}

// ─── API lintas-thread ───────────────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Saklar global. Dibaca tiap thread baru saat profiler thread-local-nya
/// pertama kali disentuh.
static AKTIF: AtomicBool = AtomicBool::new(false);

/// Kapan profiling dimulai — dipakai menghitung waktu dinding sesungguhnya.
static MULAI: Mutex<Option<Instant>> = Mutex::new(None);

/// Berapa thread yang sudah menyetor hasilnya.
static THREAD_TERHITUNG: Mutex<u32> = Mutex::new(0);

/// Berapa utas yang DIHARAPKAN menyetor hasilnya.
static THREAD_DIHARAPKAN: Mutex<u32> = Mutex::new(0);

/// Dipanggil setiap kali sebuah goroutine dilahirkan.
pub fn utas_baru() {
    if !AKTIF.load(Ordering::Relaxed) { return; }
    *THREAD_DIHARAPKAN.lock().unwrap_or_else(|e| e.into_inner()) += 1;
}

fn tabel_global() -> &'static Mutex<HashMap<String, FnStat>> {
    static T: OnceLock<Mutex<HashMap<String, FnStat>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    static PROF: RefCell<Profiler> =
        RefCell::new(Profiler::new(AKTIF.load(Ordering::Relaxed)));
}

/// Aktifkan atau nonaktifkan profiler. Panggil sebelum program dijalankan.
pub fn init(enabled: bool) {
    AKTIF.store(enabled, Ordering::Relaxed);
    PROF.with(|p| p.borrow_mut().enabled = enabled);
    if enabled {
        *MULAI.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}

/// Catat masuk ke fungsi `name`.
#[inline]
pub fn enter(name: &str) {
    PROF.with(|p| {
        if p.borrow().enabled {
            p.borrow_mut().enter(name);
        }
    });
}

/// Catat keluar dari fungsi.
#[inline]
pub fn exit() {
    PROF.with(|p| {
        if p.borrow().enabled {
            p.borrow_mut().exit();
        }
    });
}

/// Apakah profiler sedang menyala? Dibaca thread baru (goroutine) yang
/// harus memutuskan sendiri apakah ia perlu mencatat.
pub fn sedang_aktif() -> bool {
    AKTIF.load(Ordering::Relaxed)
}

/// Setorkan hasil thread ini ke tabel global, lalu kosongkan miliknya.
pub fn flush() {
    if !AKTIF.load(Ordering::Relaxed) { return; }
    let punya = PROF.with(|p| {
        let mut g = p.borrow_mut();
        if g.stats.is_empty() { return None; }
        Some(std::mem::take(&mut g.stats))
    });
    let Some(punya) = punya else { return };
    let mut global = tabel_global().lock().unwrap_or_else(|e| e.into_inner());
    for (nama, s) in punya {
        let e = global.entry(nama.clone()).or_insert_with(|| FnStat {
            name: nama, calls: 0, total_ns: 0, self_ns: 0,
            min_ns: u64::MAX, max_ns: 0, kedalaman: 0,
        });
        e.calls += s.calls;
        e.total_ns += s.total_ns;
        e.self_ns += s.self_ns;
        if s.min_ns < e.min_ns { e.min_ns = s.min_ns; }
        if s.max_ns > e.max_ns { e.max_ns = s.max_ns; }
        if s.kedalaman > e.kedalaman { e.kedalaman = s.kedalaman; }
    }
    *THREAD_TERHITUNG.lock().unwrap_or_else(|e| e.into_inner()) += 1;
}

/// Berkas tujuan `--profile-json`; `None` = tabel di stdout.
static TUJUAN_JSON: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// Laporan berikutnya ditulis ke `p` sebagai JSON, bukan dicetak sebagai tabel.
pub fn tulis_json_ke(p: std::path::PathBuf) {
    *TUJUAN_JSON.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

/// Baris yang sama dengan tabelnya, untuk mesin: nama tidak dipotong, waktu dalam nanodetik,
/// urut self-time terbesar dulu.
pub fn laporan_json(stats: &[FnStat], dinding_ns: u64, utas: u32) -> String {
    let mut rows: Vec<&FnStat> = stats.iter().collect();
    rows.sort_by(|a, b| b.self_ns.cmp(&a.self_ns).then_with(|| a.name.cmp(&b.name)));
    let terukur: u64 = rows.iter().map(|s| s.self_ns).sum();
    let fungsi: Vec<String> = rows.iter().map(|s| format!(
        "{{\"nama\":{},\"panggilan\":{},\"kumulatif_ns\":{},\"self_ns\":{},\"rata_ns\":{},\"min_ns\":{},\"maks_ns\":{},\"kedalaman\":{}}}",
        crate::json_inti::sandi_teks(&s.name), s.calls, s.total_ns, s.self_ns,
        if s.calls == 0 { 0 } else { s.self_ns / s.calls }, s.min_ns, s.max_ns, s.kedalaman)).collect();
    format!("{{\"utas\":{},\"dinding_ns\":{},\"terukur_ns\":{},\"fungsi\":[{}]}}\n",
            utas, dinding_ns, terukur, fungsi.join(","))
}

/// Cetak laporan. Panggil setelah program selesai.
pub fn report() {
    if !AKTIF.load(Ordering::Relaxed) { return; }
    // Thread pemanggil ikut disetor dulu — kalau program berjalan di
    // thread ini, hasilnya ada di sini.
    flush();
    // Lalu TUNGGU setoran goroutine yang masih di jalan, dengan BATAS.
    {
        let harap = *THREAD_DIHARAPKAN.lock().unwrap_or_else(|e| e.into_inner()) + 1;
        let batas = std::time::Instant::now() + std::time::Duration::from_millis(250);
        while std::time::Instant::now() < batas {
            if *THREAD_TERHITUNG.lock().unwrap_or_else(|e| e.into_inner()) >= harap { break; }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let dinding = MULAI.lock().unwrap_or_else(|e| e.into_inner())
        .map(|t| t.elapsed().as_nanos() as u64).unwrap_or(0);
    let utas = *THREAD_TERHITUNG.lock().unwrap_or_else(|e| e.into_inner());
    let global = tabel_global().lock().unwrap_or_else(|e| e.into_inner());
    let stats: Vec<FnStat> = global.values().cloned().collect();
    drop(global);
    let tujuan = TUJUAN_JSON.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match tujuan {
        Some(p) => if let Err(e) = std::fs::write(&p, laporan_json(&stats, dinding, utas)) {
            crate::host::galat_baris(&format!("profil JSON '{}': {}", p.display(), crate::host::galat_io(&e)));
        },
        None => cetak_laporan(&stats, dinding, utas),
    }
}

/// Reset semua data (dipakai testing dan `jrgm run --watch`).
pub fn reset() {
    PROF.with(|p| {
        let enabled = p.borrow().enabled;
        *p.borrow_mut() = Profiler::new(enabled);
    });
    tabel_global().lock().unwrap_or_else(|e| e.into_inner()).clear();
    *THREAD_TERHITUNG.lock().unwrap_or_else(|e| e.into_inner()) = 0;
    *THREAD_DIHARAPKAN.lock().unwrap_or_else(|e| e.into_inner()) = 0;
    if AKTIF.load(Ordering::Relaxed) {
        *MULAI.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}

/// Ambil snapshot data statistik (untuk unit test atau export JSON).
pub fn snapshot() -> Vec<FnStat> {
    flush();
    let mut v: Vec<FnStat> =
        tabel_global().lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect();
    v.sort_by(|a, b| b.self_ns.cmp(&a.self_ns));
    v
}
