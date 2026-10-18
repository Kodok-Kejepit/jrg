// src/term.rs Kendali terminal: raw mode, tombol, ukuran layar.

use std::io::{Read, Write};
use std::sync::Mutex;

/// Tombol yang sudah diurai dari byte mentah. Urutan escape berbeda antar
/// terminal, jadi penguraiannya ada di SINI, bukan di tiap pemakai.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tombol {
    Karakter(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Kiri,
    Kanan,
    Atas,
    Bawah,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    /// Tombol fungsi F1–F12.
    F(u8),
    Ctrl(char),
    Alt(char),
    Esc,
    /// Tombol navigasi dengan pengubah: bit 1 = Shift, 2 = Alt, 4 = Ctrl.
    Ubah(u8, Box<Tombol>),
    /// Teks yang DITEMPEL (bracketed paste) datang utuh, bukan sebagai ketikan — jadi
    /// indentasi otomatis sebuah penyunting tidak menumpuk di tiap barisnya. `terpotong`
    /// berarti isinya melewati batas simpan; sisanya dibuang, dan itu harus terlihat.
    Tempel { teks: String, terpotong: bool },
    /// Tetikus (laporan SGR): kolom dan baris dihitung dari 1.
    Tetikus { aksi: AksiTetikus, x: u16, y: u16 },
    /// Byte yang tidak dikenali — dilaporkan apa adanya, bukan dibuang diam-diam.
    TakDikenal(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AksiTetikus { Kiri, Tengah, Kanan, Lepas, Seret, RodaAtas, RodaBawah, RodaKiri, RodaKanan }

#[cfg(unix)]
mod platform {
    use std::process::Command;

    /// `stty` ada di setiap Unix — Linux, macOS, dan Termux — jadi ia dipakai
    /// alih-alih `termios` yang tata letak structnya berbeda per platform.
    fn stty(args: &[&str]) -> bool {
        Command::new("stty")
            .args(args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn simpan() -> Option<String> {
        let o = Command::new("stty").arg("-g")
            .stdin(std::process::Stdio::inherit())
            .output().ok()?;
        if !o.status.success() { return None; }
        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    pub fn raw_masuk() -> bool {
        // `-isig`: Ctrl-C datang sebagai BYTE, bukan sinyal. `-icrnl`: CRLF di dalam
        // tempelan tetap SATU baris baru, bukan dua. `-iexten`: di macOS/BSD Ctrl-V dan
        // Ctrl-O ditelan terminalnya selama IEXTEN menyala.
        stty(&["-icanon", "-echo", "-isig", "-ixon", "-icrnl", "-inlcr", "-igncr", "-istrip",
               "-iexten", "min", "0", "time", "1"])
    }

    pub fn pulihkan(simpanan: Option<&str>) -> bool {
        match simpanan {
            Some(s) => stty(&[s]),
            None => stty(&["icanon", "echo", "isig", "icrnl", "iexten"]),
        }
    }

    // `TIOCGWINSZ` = 0x5413 menurut `asm-generic/ioctls.h`. mips, powerpc dan sparc
    // memakai angka lain, dan angka yang salah bukan galat melainkan ioctl LAIN — jadi
    // jalur ini cuma untuk arsitektur yang memakai header generik itu.
    #[cfg(all(any(target_os = "linux", target_os = "android"),
              any(target_arch = "x86", target_arch = "x86_64", target_arch = "arm",
                  target_arch = "aarch64", target_arch = "riscv64")))]
    pub fn ukuran() -> (u16, u16) {
        #[repr(C)]
        #[derive(Default)]
        struct Winsize { baris: u16, kolom: u16, x: u16, y: u16 }
        extern "C" { fn ioctl(fd: i32, permintaan: std::ffi::c_ulong, ...) -> i32; }
        // Layar adalah tempat MENGGAMBAR, jadi stdout ditanya lebih dulu.
        for fd in [1, 0, 2] {
            let mut w = Winsize::default();
            let ok = unsafe { ioctl(fd, 0x5413, &mut w as *mut Winsize) } == 0;
            if ok && w.baris > 0 && w.kolom > 0 { return (w.kolom, w.baris); }
        }
        (80, 24)
    }

    #[cfg(not(all(any(target_os = "linux", target_os = "android"),
                  any(target_arch = "x86", target_arch = "x86_64", target_arch = "arm",
                      target_arch = "aarch64", target_arch = "riscv64"))))]
    pub fn ukuran() -> (u16, u16) {
        let o = Command::new("stty").arg("size")
            .stdin(std::process::Stdio::inherit())
            .output();
        if let Ok(o) = o {
            let t = String::from_utf8_lossy(&o.stdout);
            let mut b = t.split_whitespace();
            if let (Some(r), Some(k)) = (b.next(), b.next()) {
                if let (Ok(r), Ok(k)) = (r.parse::<u16>(), k.parse::<u16>()) {
                    if r > 0 && k > 0 { return (k, r); }
                }
            }
        }
        (80, 24)
    }

    #[repr(C)]
    struct PollFd { fd: i32, events: i16, revents: i16 }
    // `nfds_t`: `unsigned long` di Linux/Android, `unsigned int` di macOS dan BSD.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    type Nfds = std::ffi::c_ulong;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    type Nfds = std::ffi::c_uint;
    extern "C" {
        fn poll(fds: *mut PollFd, n: Nfds, tunggu_ms: i32) -> i32;
        fn isatty(fd: i32) -> i32;
    }
    const POLLIN: i16 = 0x0001;

    /// Tunggu stdin bisa dibaca, paling lama `ms`. `poll` tidak bergantung pada mode
    /// terminal, jadi batas waktunya berlaku juga di luar raw mode.
    pub fn tunggu_masukan(ms: u64) -> super::Siap {
        let batas = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        loop {
            let sisa = batas.saturating_duration_since(std::time::Instant::now()).as_millis();
            let mut p = PollFd { fd: 0, events: POLLIN, revents: 0 };
            let n = unsafe { poll(&mut p, 1, sisa.min(i32::MAX as u128) as i32) };
            if n == 0 { return super::Siap::Waktu; }
            if n > 0 {
                // Tanpa POLLIN yang tersisa cuma HUP/ERR/NVAL: sumbernya sudah putus.
                return if p.revents & POLLIN != 0 { super::Siap::Ada } else { super::Siap::Putus };
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return super::Siap::Putus;
            }
        }
    }

    pub fn stdout_terminal() -> bool { unsafe { isatty(1) == 1 } }
}

#[cfg(windows)]
mod platform {
    // Konsol Windows tidak punya `stty`. Mode diubah lewat `SetConsoleMode`,
    // dan VT dinyalakan supaya urutan ANSI yang sama bekerja di sini.
    type Handle = *mut std::ffi::c_void;
    const STD_INPUT: u32 = 0xFFFF_FFF6;
    const STD_OUTPUT: u32 = 0xFFFF_FFF5;
    const ENABLE_LINE_INPUT: u32 = 0x0002;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    extern "system" {
        fn GetStdHandle(n: u32) -> Handle;
        fn GetConsoleMode(h: Handle, m: *mut u32) -> i32;
        fn SetConsoleMode(h: Handle, m: u32) -> i32;
        fn GetConsoleScreenBufferInfo(h: Handle, i: *mut Info) -> i32;
        fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    }

    #[repr(C)]
    #[derive(Default)]
    struct Koord { x: i16, y: i16 }
    #[repr(C)]
    #[derive(Default)]
    struct Persegi { kiri: i16, atas: i16, kanan: i16, bawah: i16 }
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        ukuran: Koord, kursor: Koord, atribut: u16,
        jendela: Persegi, maks: Koord,
    }

    fn mode(h: u32) -> Option<u32> {
        unsafe {
            let handle = GetStdHandle(h);
            let mut m = 0u32;
            if GetConsoleMode(handle, &mut m) == 0 { None } else { Some(m) }
        }
    }

    pub fn simpan() -> Option<String> { mode(STD_INPUT).map(|m| m.to_string()) }

    pub fn raw_masuk() -> bool {
        unsafe {
            // VT di sisi KELUARAN dulu, supaya urutan ANSI yang ditulis editor
            // benar-benar ditafsirkan alih-alih tercetak apa adanya.
            if let Some(m) = mode(STD_OUTPUT) {
                SetConsoleMode(GetStdHandle(STD_OUTPUT),
                               m | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
            match mode(STD_INPUT) {
                None => false,
                Some(m) => {
                    let baru = m & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT
                                     | ENABLE_PROCESSED_INPUT);
                    SetConsoleMode(GetStdHandle(STD_INPUT), baru) != 0
                }
            }
        }
    }

    pub fn pulihkan(simpanan: Option<&str>) -> bool {
        let m = simpanan.and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
        unsafe { SetConsoleMode(GetStdHandle(STD_INPUT), m) != 0 }
    }

    pub fn ukuran() -> (u16, u16) {
        unsafe {
            let mut i = Info::default();
            if GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT), &mut i) == 0 {
                return (80, 24);
            }
            let k = (i.jendela.kanan - i.jendela.kiri + 1).max(1) as u16;
            let b = (i.jendela.bawah - i.jendela.atas + 1).max(1) as u16;
            (k, b)
        }
    }

    /// `WAIT_OBJECT_0` (0) berarti ada peristiwa masukan, `WAIT_TIMEOUT` (0x102) waktunya habis.
    pub fn tunggu_masukan(ms: u64) -> super::Siap {
        let hasil = unsafe { WaitForSingleObject(GetStdHandle(STD_INPUT), ms.min(0x7fff_ffff) as u32) };
        match hasil { 0 => super::Siap::Ada, 0x102 => super::Siap::Waktu, _ => super::Siap::Putus }
    }

    pub fn stdout_terminal() -> bool { mode(STD_OUTPUT).is_some() }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    pub fn simpan() -> Option<String> { None }
    pub fn raw_masuk() -> bool { false }
    pub fn pulihkan(_: Option<&str>) -> bool { false }
    pub fn ukuran() -> (u16, u16) { (80, 24) }
    pub fn tunggu_masukan(_: u64) -> super::Siap { super::Siap::Ada }
    pub fn stdout_terminal() -> bool { false }
}

/// Hasil menunggu masukan: ada yang bisa dibaca, waktunya habis, atau sumbernya putus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Siap { Ada, Waktu, Putus }

/// Penjaga raw mode. Mode dipulihkan lewat `Drop`, jadi sebuah panic atau
/// `return` di tengah tidak meninggalkan terminal tanpa echo.
pub struct Raw {
    simpanan: Option<String>,
    aktif: bool,
}

impl Raw {
    pub fn masuk() -> Self {
        let simpanan = platform::simpan();
        let aktif = platform::raw_masuk();
        Raw { simpanan, aktif }
    }
    pub fn aktif(&self) -> bool { self.aktif }
}

impl Drop for Raw {
    fn drop(&mut self) {
        if self.aktif { platform::pulihkan(self.simpanan.as_deref()); }
    }
}

/// Mode layar yang sedang menyala. Dicatat supaya jalan keluar mana pun — galat,
/// `exit`, panik — mematikan TEPAT yang dinyalakan programnya.
#[derive(Default, Clone, Copy)]
struct Mode { layar: bool, tetikus: bool, tempel: bool, kursor_sembunyi: bool }

static MODE: Mutex<Mode> = Mutex::new(Mode { layar: false, tetikus: false, tempel: false, kursor_sembunyi: false });

/// Raw mode milik PROGRAM (`term::raw`). `static` tidak pernah di-drop saat
/// `process::exit`, jadi pemulihannya harus dipanggil: lihat [`pulihkan`].
static RAW: Mutex<Option<Raw>> = Mutex::new(None);

/// Nama mode yang dikenal [`mode`].
pub const NAMA_MODE: &[&str] = &["kursor", "layar", "tempel", "tetikus"];

/// Nyalakan atau matikan raw mode milik program. `false` kalau terminalnya menolak.
pub fn raw_program(nyala: bool) -> bool {
    let mut g = RAW.lock().unwrap_or_else(|p| p.into_inner());
    if !nyala {
        *g = None;
        return true;
    }
    if g.is_none() {
        let r = Raw::masuk();
        if !r.aktif() { return false; }
        *g = Some(r);
    }
    true
}

/// Ubah satu mode layar. `None`: namanya tidak dikenal. `Some(false)`: stdout bukan
/// terminal, jadi tidak ada yang ditulis — urutan escape di dalam pipa cuma sampah.
pub fn mode(nama: &str, nyala: bool) -> Option<bool> {
    if !NAMA_MODE.contains(&nama) { return None; }
    if !platform::stdout_terminal() { return Some(false); }
    let mut g = MODE.lock().unwrap_or_else(|p| p.into_inner());
    let urutan = match (nama, nyala) {
        // Layar alternatif: isi terminal sebelum program jalan kembali utuh saat keluar.
        ("layar", true) => { g.layar = true; "\x1b[?1049h" }
        ("layar", false) => { g.layar = false; "\x1b[?1049l" }
        // 1000 tekan/lepas, 1002 gerak selagi ditekan (seret), 1006 koordinat SGR.
        ("tetikus", true) => { g.tetikus = true; "\x1b[?1000h\x1b[?1002h\x1b[?1006h" }
        ("tetikus", false) => { g.tetikus = false; "\x1b[?1006l\x1b[?1002l\x1b[?1000l" }
        ("tempel", true) => { g.tempel = true; "\x1b[?2004h" }
        ("tempel", false) => { g.tempel = false; "\x1b[?2004l" }
        ("kursor", true) => { g.kursor_sembunyi = false; "\x1b[?25h" }
        (_, _) => { g.kursor_sembunyi = true; "\x1b[?25l" }
    };
    drop(g);
    tulis(urutan);
    Some(true)
}

/// Kembalikan terminal ke keadaan sebelum program menyentuhnya. Aman dipanggil kapan
/// pun dan berkali-kali: tanpa mode yang menyala ia tidak menulis apa-apa.
pub fn pulihkan() {
    // `try_lock`: dipanggil juga dari pengait panik, yang bisa terjadi selagi kunci dipegang.
    let m = match MODE.try_lock() {
        Ok(mut g) => std::mem::take(&mut *g),
        Err(std::sync::TryLockError::Poisoned(p)) => std::mem::take(&mut *p.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => Mode::default(),
    };
    let mut s = String::new();
    if m.tetikus { s.push_str("\x1b[?1006l\x1b[?1002l\x1b[?1000l"); }
    if m.tempel { s.push_str("\x1b[?2004l"); }
    if m.layar { s.push_str("\x1b[0m\x1b[?1049l"); }
    if m.kursor_sembunyi { s.push_str("\x1b[?25h"); }
    if !s.is_empty() { tulis(&s); }
    match RAW.try_lock() {
        Ok(mut g) => *g = None,
        Err(std::sync::TryLockError::Poisoned(p)) => *p.into_inner() = None,
        Err(std::sync::TryLockError::WouldBlock) => {}
    }
}

/// Pasang pengait panik yang memulihkan terminal SEBELUM pesan paniknya dicetak —
/// di layar alternatif pesan itu ikut hilang, dan yang tersisa terminal tanpa echo.
pub fn pasang_pengait_panik() { pasang_pengait_panik_dengan(|| {}) }

/// Sama, plus `juga`: pemulih salinan modul ini milik crate lain (`jrgc` memuat
/// pustakanya dua kali). Panik cetak std atas pipa tertutup bukan bug: ia berakhir diam.
pub fn pasang_pengait_panik_dengan(juga: fn()) {
    let lama = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        pulihkan();
        juga();
        if crate::host::panik_pipa_putus(info.payload()) { crate::host::akhiri_pipa_putus() }
        lama(info);
    }));
}

pub fn ukuran() -> (u16, u16) { platform::ukuran() }

/// Apakah stdin sebuah terminal. Dipakai untuk memutuskan apakah penyuntingan
/// baris masuk akal — di dalam pipa, ia tidak.
pub fn interaktif() -> bool {
    #[cfg(unix)]
    {
        extern "C" { fn isatty(fd: i32) -> i32; }
        unsafe { isatty(0) == 1 }
    }
    #[cfg(not(unix))]
    { platform::simpan().is_some() }
}

/// Baca satu tombol. Urutan escape dibaca sampai selesai di sini, jadi
/// pemanggilnya tidak pernah melihat byte mentah.
pub fn baca_tombol(masuk: &mut impl Read) -> Option<Tombol> {
    let mut b = [0u8; 1];
    if masuk.read(&mut b).ok()? == 0 { return None; }
    Some(urai_tombol(b[0], masuk))
}

/// Stdin lewat antrean masukan proses (`host`): byte yang sudah ditarik tidak pernah
/// tertinggal di penyangga lain, jadi `poll` atas fd-nya menjawab apa adanya.
struct Masukan;

impl Read for Masukan {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { crate::host::baca_masukan(buf) }
}

/// Baca satu tombol dari terminal sungguhan.
pub fn baca_tombol_terminal() -> Option<Tombol> {
    let mut masuk = Masukan;
    let mut b = [0u8; 1];
    // `Ok(0)` punya DUA arti, dan yang membedakannya cuma di mana ia dibaca: di
    // terminal ber-`time 1` ia berarti "belum ada", di `/dev/null` atau pipa …
    let ulangi = interaktif();
    loop {
        match masuk.read(&mut b) {
            // Terminal yang sudah DITUTUP juga menjawab `Ok(0)`, selamanya: tanpa
            // pemeriksaan ini putarannya memakan satu inti penuh menunggu yang tak akan datang.
            Ok(0) if ulangi => {
                if !interaktif() || platform::tunggu_masukan(0) == Siap::Putus { return None; }
            }
            Ok(0) => return None,
            Ok(_) => return Some(urai_tombol(b[0], &mut masuk)),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
}

/// Hasil membaca tombol dengan batas waktu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bacaan {
    Tombol(Tombol),
    /// Tidak ada tombol sampai waktunya habis.
    Kosong,
    /// Masukannya habis atau terminalnya ditutup.
    Habis,
}

/// Baca satu tombol, menunggu paling lama `ms` milidetik.
#[cfg(target_arch = "wasm32")]
pub fn baca_tombol_tunggu(_: u64) -> Bacaan {
    // Di peramban tidak ada terminal, dan `Instant::now()` di sana panik.
    Bacaan::Habis
}

/// Baca satu tombol, menunggu paling lama `ms` milidetik.
#[cfg(not(target_arch = "wasm32"))]
pub fn baca_tombol_tunggu(ms: u64) -> Bacaan {
    let batas = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    let terminal = interaktif();
    let mut masuk = Masukan;
    let mut b = [0u8; 1];
    loop {
        // Yang sudah ada di antrean dibaca tanpa bertanya ke sistem: `poll` cuma tahu
        // isi fd, bukan byte yang sudah ditarik bersama tombol sebelumnya.
        if !crate::host::ada_masukan() {
            let sisa = batas.saturating_duration_since(std::time::Instant::now()).as_millis() as u64;
            match platform::tunggu_masukan(sisa) {
                Siap::Waktu => return Bacaan::Kosong,
                Siap::Putus => return Bacaan::Habis,
                Siap::Ada => {}
            }
        }
        match masuk.read(&mut b) {
            // Di terminal `Ok(0)` berarti belum ada apa-apa; di pipa berarti habis. Terminal
            // yang ditutup menjawab siap-baca lalu nol byte, dan sudah bukan terminal lagi.
            Ok(0) if terminal => {
                if !interaktif() { return Bacaan::Habis; }
                if std::time::Instant::now() >= batas { return Bacaan::Kosong; }
            }
            Ok(0) => return Bacaan::Habis,
            Ok(_) => return Bacaan::Tombol(urai_tombol(b[0], &mut masuk)),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Bacaan::Habis,
        }
    }
}

fn urai_tombol(pertama: u8, masuk: &mut impl Read) -> Tombol {
    match pertama {
        b'\r' | b'\n' => Tombol::Enter,
        b'\t' => Tombol::Tab,
        0x7f | 0x08 => Tombol::Backspace,
        0x1b => baca_escape(masuk),
        // Ctrl-Space dan Ctrl-\ ] ^ _ dulu jatuh ke `Karakter`, jadi sebuah penyunting
        // MENGETIKKAN byte kendali itu ke dalam berkas.
        0x00 => Tombol::Ctrl(' '),
        c @ 0x01..=0x1a => Tombol::Ctrl((c + b'a' - 1) as char),
        c @ 0x1c..=0x1f => Tombol::Ctrl((c + 0x40) as char),
        c if c < 0x80 => Tombol::Karakter(c as char),
        // UTF-8 multi-byte: panjangnya ditentukan byte pertama. Byte lanjutan yang
        // datang sendirian bukan awal karakter, jadi tidak boleh menelan byte berikutnya.
        c => {
            let n = match c { 0xc0..=0xdf => 1, 0xe0..=0xef => 2, 0xf0..=0xf7 => 3, _ => 0 };
            let mut buf = vec![c];
            for _ in 0..n {
                match satu(masuk) { Some(x) => buf.push(x), None => break }
            }
            match std::str::from_utf8(&buf) {
                Ok(s) => s.chars().next().map(Tombol::Karakter)
                    .unwrap_or(Tombol::TakDikenal(buf.clone())),
                Err(_) => Tombol::TakDikenal(buf),
            }
        }
    }
}

fn satu(masuk: &mut impl Read) -> Option<u8> {
    let mut b = [0u8; 1];
    match masuk.read(&mut b) { Ok(n) if n > 0 => Some(b[0]), _ => None }
}

/// Tambahkan pengubah `m` (bit 1 Shift, 2 Alt, 4 Ctrl) ke tombol `t`.
fn tambah_ubah(m: u8, t: Tombol) -> Tombol {
    if m == 0 { return t; }
    match t {
        Tombol::Ubah(m0, d) => Tombol::Ubah(m0 | m, d),
        Tombol::TakDikenal(b) => Tombol::TakDikenal(b),
        lain => Tombol::Ubah(m, Box::new(lain)),
    }
}

fn baca_escape(masuk: &mut impl Read) -> Tombol {
    let Some(b) = satu(masuk) else { return Tombol::Esc };
    match b {
        b'[' => baca_csi(masuk),
        b'O' => baca_ss3(masuk),
        // `ESC ESC [ A`: Alt+panah di sebagian terminal. Dulu `[A`-nya bocor jadi ketikan.
        0x1b => match baca_escape(masuk) {
            Tombol::Esc => Tombol::Esc,
            t => tambah_ubah(2, t),
        },
        // ESC + tombol apa pun = Alt + tombol itu: huruf, Enter, Backspace, Ctrl-huruf.
        lain => match urai_tombol(lain, masuk) {
            Tombol::Karakter(c) => Tombol::Alt(c),
            Tombol::TakDikenal(mut sisa) => {
                sisa.insert(0, 0x1b);
                Tombol::TakDikenal(sisa)
            }
            t => tambah_ubah(2, t),
        },
    }
}

fn panah(c: u8) -> Option<Tombol> {
    Some(match c.to_ascii_uppercase() {
        b'A' => Tombol::Atas,
        b'B' => Tombol::Bawah,
        b'C' => Tombol::Kanan,
        b'D' => Tombol::Kiri,
        _ => return None,
    })
}

/// SS3 (`ESC O x`): sebagian terminal mengirimnya untuk Home/End, panah, dan F1–F4.
fn baca_ss3(masuk: &mut impl Read) -> Tombol {
    // `ESC O` tanpa lanjutan adalah Alt+O yang diketik, bukan urutan yang terpotong.
    let Some(c) = satu(masuk) else { return Tombol::Alt('O') };
    match c {
        b'H' => Tombol::Home,
        b'F' => Tombol::End,
        b'A'..=b'D' => panah(c).unwrap_or(Tombol::Esc),
        // rxvt: huruf kecil berarti panah bersama Ctrl.
        b'a'..=b'd' => tambah_ubah(4, panah(c).unwrap_or(Tombol::Esc)),
        b'P'..=b'S' => Tombol::F(1 + c - b'P'),
        lain => Tombol::TakDikenal(vec![0x1b, b'O', lain]),
    }
}

/// Paling banyak byte sebuah tempelan yang disimpan; sisanya tetap dibaca sampai
/// penutupnya supaya tidak bocor jadi ketikan, dan pemotongannya dilaporkan.
const MAKS_TEMPEL: usize = 16 * 1024 * 1024;

/// Isi tempelan sampai `ESC [ 201 ~`. Pembacaan kosong diulang beberapa kali: tempelan
/// besar datang berpotong-potong, dan berhenti di jeda pertama membocorkan sisanya.
fn baca_tempel(masuk: &mut impl Read) -> Tombol {
    const AKHIR: &[u8] = b"\x1b[201~";
    let mut isi: Vec<u8> = Vec::new();
    let mut ekor: Vec<u8> = Vec::new();
    let mut terpotong = false;
    let mut kosong = 0;
    while kosong < 20 {
        let Some(c) = satu(masuk) else { kosong += 1; continue };
        kosong = 0;
        ekor.push(c);
        if ekor.ends_with(AKHIR) {
            ekor.truncate(ekor.len() - AKHIR.len());
            break;
        }
        // Yang sudah pasti bukan bagian penutup dipindah ke isi.
        if ekor.len() > AKHIR.len() {
            let lebih = ekor.len() - AKHIR.len();
            if isi.len() + lebih <= MAKS_TEMPEL { isi.extend_from_slice(&ekor[..lebih]); }
            else { terpotong = true; }
            ekor.drain(..lebih);
        }
    }
    if isi.len() + ekor.len() <= MAKS_TEMPEL && !terpotong { isi.extend_from_slice(&ekor); }
    else if !ekor.is_empty() { terpotong = true; }
    // Terminal mengirim baris baru tempelan sebagai CR; di dalam teks ia adalah LF.
    let teks = String::from_utf8_lossy(&isi).replace("\r\n", "\n").replace('\r', "\n");
    Tombol::Tempel { teks, terpotong }
}

/// `ESC [ < b ; x ; y M|m` — laporan tetikus SGR.
fn urai_tetikus(param: &[u8], tekan: bool, mentah: Vec<u8>) -> Tombol {
    let teks = String::from_utf8_lossy(param);
    let v: Vec<u32> = teks.split(';').filter_map(|x| x.parse().ok()).collect();
    if v.len() != 3 { return Tombol::TakDikenal(mentah); }
    let b = v[0];
    let aksi = if b & 64 != 0 {
        match b & 3 {
            0 => AksiTetikus::RodaAtas,
            1 => AksiTetikus::RodaBawah,
            2 => AksiTetikus::RodaKiri,
            _ => AksiTetikus::RodaKanan,
        }
    } else if b & 32 != 0 {
        AksiTetikus::Seret
    } else if !tekan {
        AksiTetikus::Lepas
    } else {
        match b & 3 {
            0 => AksiTetikus::Kiri,
            1 => AksiTetikus::Tengah,
            2 => AksiTetikus::Kanan,
            _ => AksiTetikus::Lepas,
        }
    };
    let batas = |n: u32| n.min(u16::MAX as u32) as u16;
    let t = Tombol::Tetikus { aksi, x: batas(v[1]), y: batas(v[2]) };
    // Bit 4 Shift, 8 Alt, 16 Ctrl — digeser jadi bitmask pengubah yang sama dengan tombol.
    tambah_ubah(((b >> 2) & 0x7) as u8, t)
}

/// Tombol yang dilaporkan sebagai KODE karakter plus pengubah (`CSI kode ; ubah u`,
/// dan `CSI 27 ; ubah ; kode ~` milik xterm) — dikirim terminal yang dipasangi begitu.
fn dari_kode(kode: u32, ubah: u8) -> Option<Tombol> {
    let dasar = match kode {
        13 => Tombol::Enter,
        9 => Tombol::Tab,
        27 => Tombol::Esc,
        8 | 127 => Tombol::Backspace,
        _ => Tombol::Karakter(char::from_u32(kode).filter(|c| !c.is_control())?),
    };
    Some(match (dasar, ubah) {
        (Tombol::Karakter(c), 4) if c.is_ascii_alphabetic() => Tombol::Ctrl(c.to_ascii_lowercase()),
        (Tombol::Karakter(' '), 4) => Tombol::Ctrl(' '),
        (Tombol::Karakter(c), 2) => Tombol::Alt(c),
        (Tombol::Tab, 1) => Tombol::BackTab,
        (t, m) => tambah_ubah(m, t),
    })
}

fn baca_csi(masuk: &mut impl Read) -> Tombol {
    // Tata bahasa CSI: byte parameter 0x30–0x3F, byte antara 0x20–0x2F, lalu SATU byte
    // penutup 0x40–0x7E. Dulu cuma angka dan `;` yang dianggap parameter, jadi `ESC [ <`
    // (tetikus) berhenti di `<` dan sisanya bocor jadi ketikan.
    let mut mentah: Vec<u8> = vec![0x1b, b'['];
    let mut param: Vec<u8> = Vec::new();
    let mut antara = false;
    let akhir = loop {
        let Some(c) = satu(masuk) else {
            // `ESC [` tanpa lanjutan adalah Alt+[ yang diketik.
            return if mentah.len() == 2 { Tombol::Alt('[') } else { Tombol::TakDikenal(mentah) };
        };
        mentah.push(c);
        match c {
            0x30..=0x3f => param.push(c),
            // rxvt menutup urutannya dengan `$` untuk Shift (`ESC [ 5 $`).
            b'$' if !param.is_empty() && !antara => break c,
            0x20..=0x2f => antara = true,
            0x40..=0x7e => break c,
            _ => return Tombol::TakDikenal(mentah),
        }
        if mentah.len() > 64 { return Tombol::TakDikenal(mentah); }
    };
    if param.first() == Some(&b'<') && (akhir == b'M' || akhir == b'm') {
        return urai_tetikus(&param[1..], akhir == b'M', mentah);
    }
    // Konsol Linux: F1–F5 datang sebagai `ESC [ [ A` sampai `ESC [ [ E`.
    if akhir == b'[' && param.is_empty() && !antara {
        return match satu(masuk) {
            Some(c @ b'A'..=b'E') => Tombol::F(1 + c - b'A'),
            Some(c) => { mentah.push(c); Tombol::TakDikenal(mentah) }
            None => Tombol::TakDikenal(mentah),
        };
    }
    if antara { return Tombol::TakDikenal(mentah); }
    let teks = String::from_utf8_lossy(&param).to_string();
    let angka: Vec<Option<u32>> = teks.split(';').map(|x| x.parse::<u32>().ok()).collect();
    let p = |i: usize| angka.get(i).copied().flatten();
    // Parameter pengubah adalah 1 + bitmask (1=Shift, 2=Alt, 4=Ctrl); tanpa membacanya
    // Ctrl+Kanan terbaca sebagai Kanan biasa dan informasinya hilang tanpa suara.
    let bit = |i: usize| p(i).map(|m| (m.saturating_sub(1) & 0x7) as u8).unwrap_or(0);
    let dasar = match akhir {
        b'A'..=b'D' => panah(akhir).unwrap_or(Tombol::Esc),
        // rxvt: huruf kecil berarti panah bersama Shift.
        b'a'..=b'd' => return tambah_ubah(1, panah(akhir).unwrap_or(Tombol::Esc)),
        b'H' => Tombol::Home,
        b'F' => Tombol::End,
        b'Z' => return Tombol::BackTab,
        b'P'..=b'S' if p(0).is_none() || p(0) == Some(1) => Tombol::F(1 + akhir - b'P'),
        b'u' => return match p(0).and_then(|k| dari_kode(k, bit(1))) {
            Some(t) => t,
            None => Tombol::TakDikenal(mentah),
        },
        b'~' if p(0) == Some(27) => return match p(2).and_then(|k| dari_kode(k, bit(1))) {
            Some(t) => t,
            None => Tombol::TakDikenal(mentah),
        },
        b'~' if p(0) == Some(200) => return baca_tempel(masuk),
        // Angkanya dibaca UTUH: dulu cuma byte pertamanya, jadi F5 (`15~`) terbaca Home.
        // Penutup `$`, `^`, `@` adalah cara rxvt menyebut Shift, Ctrl, dan keduanya.
        b'~' | b'$' | b'^' | b'@' => {
            let t = match p(0) {
                Some(1) | Some(7) => Tombol::Home,
                Some(2) => Tombol::Insert,
                Some(3) => Tombol::Delete,
                Some(4) | Some(8) => Tombol::End,
                Some(5) => Tombol::PageUp,
                Some(6) => Tombol::PageDown,
                Some(n @ 11..=15) => Tombol::F((n - 10) as u8),
                Some(n @ 17..=21) => Tombol::F((n - 11) as u8),
                Some(n @ 23..=24) => Tombol::F((n - 12) as u8),
                _ => return Tombol::TakDikenal(mentah),
            };
            let rxvt = match akhir { b'$' => 1, b'^' => 4, b'@' => 5, _ => 0 };
            return tambah_ubah(bit(1) | rxvt, t);
        }
        _ => return Tombol::TakDikenal(mentah),
    };
    tambah_ubah(bit(1), dasar)
}

/// Tulis lalu paksa keluar. Tanpa flush, keluaran editor tertahan di buffer
/// dan layar terlihat membeku.
pub fn tulis(s: &str) {
    let mut o = std::io::stdout();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urai(b: &[u8]) -> Vec<Tombol> {
        let mut c = std::io::Cursor::new(b.to_vec());
        let mut v = Vec::new();
        while let Some(t) = baca_tombol(&mut c) { v.push(t); }
        v
    }

    #[test]
    fn pembacaan_habis_berarti_selesai_bukan_berputar() {
        // `Ok(0)` dari sumber yang HABIS harus berarti selesai.
        let mut kosong = std::io::Cursor::new(Vec::new());
        assert_eq!(baca_tombol(&mut kosong), None);
        let mut habis = std::io::Cursor::new(b"a".to_vec());
        assert_eq!(baca_tombol(&mut habis), Some(Tombol::Karakter('a')));
        assert_eq!(baca_tombol(&mut habis), None);
    }

    #[test]
    fn esc_sendirian_jadi_tombol_bukan_menggantung() {
        // Mode raw dipasang `min 0 time 1`, jadi pembacaan lanjutan yang kosong
        // menjawab `Ok(0)` — dan `baca_escape` membacanya sebagai "urutannya habis".
        assert_eq!(urai(b"\x1b"), vec![Tombol::Esc]);
        // Alt+huruf datang sebagai satu semburan, jadi ia TIDAK boleh ikut
        // terbaca sebagai Esc.
        assert_eq!(urai(b"\x1bx"), vec![Tombol::Alt('x')]);
    }

    #[test]
    fn pengubah_pada_panah_terbaca_bukan_dibuang() {
        // `ESC [ 1 ; 5 C` = Ctrl+Kanan. Sebelum ini parameternya dikumpulkan
        // lalu diabaikan, jadi Ctrl+Kanan terbaca sebagai Kanan biasa —
        // jawaban yang masuk akal dan salah, tanpa satu pun tanda.
        assert_eq!(urai(b"\x1b[1;5C"), vec![Tombol::Ubah(4, Box::new(Tombol::Kanan))]);
        assert_eq!(urai(b"\x1b[1;3A"), vec![Tombol::Ubah(2, Box::new(Tombol::Atas))]);
        assert_eq!(urai(b"\x1b[1;2D"), vec![Tombol::Ubah(1, Box::new(Tombol::Kiri))]);
        assert_eq!(urai(b"\x1b[1;5H"), vec![Tombol::Ubah(4, Box::new(Tombol::Home))]);
        // Tanpa pengubah bentuknya TIDAK boleh berubah: seluruh pemakai lama
        // membandingkan dengan `Tombol::Kanan`, bukan `Ubah(0, ...)`.
        assert_eq!(urai(b"\x1b[C"), vec![Tombol::Kanan]);
        assert_eq!(urai(b"\x1b[3~"), vec![Tombol::Delete]);
    }

    #[test]
    fn panah_dan_navigasi_diurai() {
        assert_eq!(urai(b"\x1b[A"), vec![Tombol::Atas]);
        assert_eq!(urai(b"\x1b[B"), vec![Tombol::Bawah]);
        assert_eq!(urai(b"\x1b[C"), vec![Tombol::Kanan]);
        assert_eq!(urai(b"\x1b[D"), vec![Tombol::Kiri]);
        assert_eq!(urai(b"\x1b[H"), vec![Tombol::Home]);
        assert_eq!(urai(b"\x1b[F"), vec![Tombol::End]);
        assert_eq!(urai(b"\x1b[3~"), vec![Tombol::Delete]);
        assert_eq!(urai(b"\x1b[5~"), vec![Tombol::PageUp]);
        assert_eq!(urai(b"\x1b[6~"), vec![Tombol::PageDown]);
        // Bentuk SS3, dipakai sebagian terminal untuk tombol yang sama.
        assert_eq!(urai(b"\x1bOH"), vec![Tombol::Home]);
        assert_eq!(urai(b"\x1bOA"), vec![Tombol::Atas]);
        // Bentuk lain untuk Home/End yang dipakai xterm/tmux.
        assert_eq!(urai(b"\x1b[1~"), vec![Tombol::Home]);
        assert_eq!(urai(b"\x1b[4~"), vec![Tombol::End]);
    }

    #[test]
    fn ctrl_dan_karakter_biasa_diurai() {
        assert_eq!(urai(b"a"), vec![Tombol::Karakter('a')]);
        assert_eq!(urai(b"\x01"), vec![Tombol::Ctrl('a')]);
        assert_eq!(urai(b"\x03"), vec![Tombol::Ctrl('c')]);
        assert_eq!(urai(b"\r"), vec![Tombol::Enter]);
        assert_eq!(urai(b"\t"), vec![Tombol::Tab]);
        assert_eq!(urai(&[0x7f]), vec![Tombol::Backspace]);
    }

    /// Sebuah editor yang memotong UTF-8 di tengah akan merusak teks pemakainya,
    /// dan kerusakan itu tidak melapor.
    #[test]
    fn utf8_multibyte_utuh() {
        assert_eq!(urai("é".as_bytes()), vec![Tombol::Karakter('é')]);
        assert_eq!(urai("日".as_bytes()), vec![Tombol::Karakter('日')]);
        assert_eq!(urai("aé日".as_bytes()),
                   vec![Tombol::Karakter('a'), Tombol::Karakter('é'), Tombol::Karakter('日')]);
    }

    /// Urutan yang tidak dikenal dilaporkan APA ADANYA. Membuangnya diam-diam
    /// membuat tombol yang tidak jalan terlihat seperti tombol yang mati.
    #[test]
    fn urutan_asing_tidak_dibuang_diam_diam() {
        match urai(b"\x1b[99q").first() {
            Some(Tombol::TakDikenal(_)) => {}
            lain => panic!("urutan asing harusnya dilaporkan, dapat {:?}", lain),
        }
    }

    /// F5–F8 (`15~`..`19~`) dulu terbaca HOME: cuma byte pertama parameternya yang
    /// dicocokkan. Tombol yang salah arti lebih buruk daripada tombol yang mati.
    #[test]
    fn tombol_fungsi_dibaca_utuh() {
        let kode = [(11, 1), (12, 2), (13, 3), (14, 4), (15, 5), (17, 6), (18, 7),
                    (19, 8), (20, 9), (21, 10), (23, 11), (24, 12)];
        for (n, f) in kode {
            assert_eq!(urai(format!("\x1b[{}~", n).as_bytes()), vec![Tombol::F(f)], "kode {}", n);
        }
        assert_eq!(urai(b"\x1bOP"), vec![Tombol::F(1)]);
        assert_eq!(urai(b"\x1bOS"), vec![Tombol::F(4)]);
        assert_eq!(urai(b"\x1b[[A"), vec![Tombol::F(1)]);
        assert_eq!(urai(b"\x1b[[E"), vec![Tombol::F(5)]);
        assert_eq!(urai(b"\x1b[15;2~"), vec![Tombol::Ubah(1, Box::new(Tombol::F(5)))]);
        assert_eq!(urai(b"\x1b[1;5P"), vec![Tombol::Ubah(4, Box::new(Tombol::F(1)))]);
        assert_eq!(urai(b"\x1b[2~"), vec![Tombol::Insert]);
        assert_eq!(urai(b"\x1b[7~"), vec![Tombol::Home]);
        assert_eq!(urai(b"\x1b[8~"), vec![Tombol::End]);
    }

    /// Byte kendali yang bukan huruf dulu jadi `Karakter` — sebuah penyunting lalu
    /// mengetikkannya ke dalam berkas.
    #[test]
    fn byte_kendali_tidak_pernah_jadi_ketikan() {
        assert_eq!(urai(&[0x00]), vec![Tombol::Ctrl(' ')]);
        assert_eq!(urai(&[0x1c]), vec![Tombol::Ctrl('\\')]);
        assert_eq!(urai(&[0x1d]), vec![Tombol::Ctrl(']')]);
        assert_eq!(urai(&[0x1e]), vec![Tombol::Ctrl('^')]);
        assert_eq!(urai(&[0x1f]), vec![Tombol::Ctrl('_')]);
        for b in 0u8..0x20 {
            for t in urai(&[b]) {
                assert!(!matches!(t, Tombol::Karakter(_)), "byte {:#04x} jadi ketikan: {:?}", b, t);
            }
        }
    }

    /// Alt sebagai awalan ESC: tidak ada sisa urutan yang boleh bocor jadi ketikan.
    #[test]
    fn alt_di_depan_tombol_apa_pun() {
        assert_eq!(urai(b"\x1b\x1b[A"), vec![Tombol::Ubah(2, Box::new(Tombol::Atas))]);
        assert_eq!(urai(b"\x1b\x1b[1;5C"), vec![Tombol::Ubah(6, Box::new(Tombol::Kanan))]);
        assert_eq!(urai(&[0x1b, 0x7f]), vec![Tombol::Ubah(2, Box::new(Tombol::Backspace))]);
        assert_eq!(urai(b"\x1b\r"), vec![Tombol::Ubah(2, Box::new(Tombol::Enter))]);
        assert_eq!(urai(&[0x1b, 0x13]), vec![Tombol::Ubah(2, Box::new(Tombol::Ctrl('s')))]);
        assert_eq!(urai(b"\x1b/"), vec![Tombol::Alt('/')]);
        assert_eq!(urai(b"\x1b]"), vec![Tombol::Alt(']')]);
        assert_eq!(urai("\x1bé".as_bytes()), vec![Tombol::Alt('é')]);
        // `ESC [` dan `ESC O` tanpa lanjutan adalah Alt+[ dan Alt+O yang diketik.
        assert_eq!(urai(b"\x1b["), vec![Tombol::Alt('[')]);
        assert_eq!(urai(b"\x1bO"), vec![Tombol::Alt('O')]);
        assert_eq!(urai(b"\x1b\x1b"), vec![Tombol::Esc]);
    }

    #[test]
    fn bentuk_rxvt_dan_kode_karakter() {
        assert_eq!(urai(b"\x1b[a"), vec![Tombol::Ubah(1, Box::new(Tombol::Atas))]);
        assert_eq!(urai(b"\x1bOd"), vec![Tombol::Ubah(4, Box::new(Tombol::Kiri))]);
        assert_eq!(urai(b"\x1b[5^"), vec![Tombol::Ubah(4, Box::new(Tombol::PageUp))]);
        assert_eq!(urai(b"\x1b[6$"), vec![Tombol::Ubah(1, Box::new(Tombol::PageDown))]);
        assert_eq!(urai(b"\x1b[7@"), vec![Tombol::Ubah(5, Box::new(Tombol::Home))]);
        // `CSI kode ; ubah u` dan bentuk xterm `CSI 27 ; ubah ; kode ~`.
        assert_eq!(urai(b"\x1b[13;5u"), vec![Tombol::Ubah(4, Box::new(Tombol::Enter))]);
        assert_eq!(urai(b"\x1b[115;5u"), vec![Tombol::Ctrl('s')]);
        assert_eq!(urai(b"\x1b[32;5u"), vec![Tombol::Ctrl(' ')]);
        assert_eq!(urai(b"\x1b[97;3u"), vec![Tombol::Alt('a')]);
        assert_eq!(urai(b"\x1b[9;2u"), vec![Tombol::BackTab]);
        assert_eq!(urai(b"\x1b[27;5;13~"), vec![Tombol::Ubah(4, Box::new(Tombol::Enter))]);
        assert_eq!(urai(b"\x1b[97u"), vec![Tombol::Karakter('a')]);
    }

    /// Tempelan datang sebagai SATU tombol berisi teksnya; CR terminal jadi baris baru,
    /// dan tombol sesudah penutupnya tetap terbaca sebagai tombol.
    #[test]
    fn tempel_berkurung_utuh() {
        let t = urai(b"\x1b[200~fn f() {\r    wok(1)\r}\x1b[201~x");
        assert_eq!(t, vec![
            Tombol::Tempel { teks: "fn f() {\n    wok(1)\n}".to_string(), terpotong: false },
            Tombol::Karakter('x'),
        ]);
        assert_eq!(urai(b"\x1b[200~\x1b[201~"),
            vec![Tombol::Tempel { teks: String::new(), terpotong: false }]);
        // Isi yang MEMUAT urutan escape lain (mis. salinan dari terminal) tetap isi.
        assert_eq!(urai(b"\x1b[200~a\x1b[Ab\x1b[201~"),
            vec![Tombol::Tempel { teks: "a\x1b[Ab".to_string(), terpotong: false }]);
        assert_eq!(urai("\x1b[200~日本\r\n語\x1b[201~".as_bytes()),
            vec![Tombol::Tempel { teks: "日本\n語".to_string(), terpotong: false }]);
        // Penutup yang tidak pernah datang: isinya tetap dikembalikan, bukan menggantung.
        assert_eq!(urai(b"\x1b[200~abc"),
            vec![Tombol::Tempel { teks: "abc".to_string(), terpotong: false }]);
    }

    /// Laporan tetikus SGR dulu bocor seluruhnya jadi ketikan (`<0;12;5M`).
    #[test]
    fn tetikus_sgr_diurai() {
        let t = |aksi, x, y| Tombol::Tetikus { aksi, x, y };
        assert_eq!(urai(b"\x1b[<0;12;5M"), vec![t(AksiTetikus::Kiri, 12, 5)]);
        assert_eq!(urai(b"\x1b[<0;12;5m"), vec![t(AksiTetikus::Lepas, 12, 5)]);
        assert_eq!(urai(b"\x1b[<1;1;1M"), vec![t(AksiTetikus::Tengah, 1, 1)]);
        assert_eq!(urai(b"\x1b[<2;300;200M"), vec![t(AksiTetikus::Kanan, 300, 200)]);
        assert_eq!(urai(b"\x1b[<32;13;5M"), vec![t(AksiTetikus::Seret, 13, 5)]);
        assert_eq!(urai(b"\x1b[<64;3;4M"), vec![t(AksiTetikus::RodaAtas, 3, 4)]);
        assert_eq!(urai(b"\x1b[<65;3;4M"), vec![t(AksiTetikus::RodaBawah, 3, 4)]);
        assert_eq!(urai(b"\x1b[<66;3;4M"), vec![t(AksiTetikus::RodaKiri, 3, 4)]);
        assert_eq!(urai(b"\x1b[<16;7;9M"),
            vec![Tombol::Ubah(4, Box::new(t(AksiTetikus::Kiri, 7, 9)))]);
        assert_eq!(urai(b"\x1b[<4;7;9M"),
            vec![Tombol::Ubah(1, Box::new(t(AksiTetikus::Kiri, 7, 9)))]);
        // Laporan yang cacat dilaporkan apa adanya, dan tidak menelan tombol berikutnya.
        match urai(b"\x1b[<0;12Mz").as_slice() {
            [Tombol::TakDikenal(_), Tombol::Karakter('z')] => {}
            lain => panic!("laporan cacat: {:?}", lain),
        }
    }

    /// Byte lanjutan UTF-8 yang datang sendirian bukan awal karakter: ia dilaporkan,
    /// dan TIDAK menelan tombol yang diketik sesudahnya.
    #[test]
    fn utf8_cacat_tidak_menelan_tombol_berikutnya() {
        match urai(&[0x80, b'a']).as_slice() {
            [Tombol::TakDikenal(b), Tombol::Karakter('a')] => assert_eq!(b, &vec![0x80]),
            lain => panic!("byte lanjutan sendirian: {:?}", lain),
        }
        match urai(&[0xff, b'b']).as_slice() {
            [Tombol::TakDikenal(_), Tombol::Karakter('b')] => {}
            lain => panic!("byte 0xff: {:?}", lain),
        }
        assert_eq!(urai("😀".as_bytes()), vec![Tombol::Karakter('😀')]);
    }

    /// Urutan CSI yang panjangnya tak masuk akal dipotong, bukan dibaca selamanya.
    #[test]
    fn csi_tanpa_penutup_dibatasi() {
        let mut b = b"\x1b[".to_vec();
        b.extend(std::iter::repeat(b'1').take(200));
        let t = urai(&b);
        assert!(matches!(t.first(), Some(Tombol::TakDikenal(_))), "{:?}", t.first());
    }

    /// Tanpa mode yang menyala, pemulihan tidak menulis apa pun dan boleh diulang.
    #[test]
    fn pulihkan_tanpa_mode_tidak_berbuat_apa_apa() {
        pulihkan();
        pulihkan();
        assert_eq!(mode("hantu", true), None);
        // Di bawah `cargo test` stdout bukan terminal: tidak ada yang ditulis.
        if !platform::stdout_terminal() {
            assert_eq!(mode("layar", true), Some(false));
            assert_eq!(mode("tetikus", true), Some(false));
        }
    }

    #[test]
    fn escape_sendirian_tetap_escape() {
        assert_eq!(urai(b"\x1b"), vec![Tombol::Esc]);
    }
}
