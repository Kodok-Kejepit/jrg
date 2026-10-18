// src/host.rs — yang disediakan lingkungan: keluaran, masukan, jam, keacakan,
// tidur, dan keluar. Di mesin biasa ini std apa adanya; di peramban (wasm) std
// diam atau panik untuk semuanya, jadi tiap-tiapnya diminta ke JavaScript.

use std::io;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::collections::VecDeque;
    use std::io::{self, BufRead, Write};
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// Byte stdin yang sudah ditarik dan belum dipakai. `Stdin` milik Rust menyangga
    /// sendiri; kalau sisanya tinggal di sana, `poll` atas fd-nya menjawab "kosong"
    /// sementara masih ada tombol yang menunggu.
    static MASUKAN: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());

    fn antre() -> std::sync::MutexGuard<'static, VecDeque<u8>> {
        MASUKAN.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Tarik SEMUA yang tersedia dari stdin ke antrean. `Ok(0)`: tidak ada byte baru —
    /// habis, atau batas waktu baca terminal raw. Kunci stdin dipegang PEMANGGIL, supaya
    /// satu pembacaan utuh tidak terselip pembaca lain (dua goroutine yang sama-sama `tok()`).
    fn tarik(kunci: &mut io::StdinLock<'static>) -> io::Result<usize> {
        loop {
            match kunci.fill_buf() {
                Ok(b) => {
                    let n = b.len();
                    antre().extend(b);
                    kunci.consume(n);
                    return Ok(n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn ada_masukan() -> bool { !antre().is_empty() }

    #[cfg(test)]
    pub fn isi_antrean_uji(b: &[u8]) { antre().extend(b); }

    pub fn baca_masukan(buf: &mut [u8]) -> io::Result<usize> {
        let mut kunci = io::stdin().lock();
        if antre().is_empty() { tarik(&mut kunci)?; }
        let mut q = antre();
        let n = buf.len().min(q.len());
        for (i, b) in q.drain(..n).enumerate() { buf[i] = b; }
        Ok(n)
    }

    pub fn tulis(s: &str) { tulis_ke(&mut io::stdout().lock(), "stdout", &[s.as_bytes()]); }
    pub fn tulis_baris(s: &str) { tulis_ke(&mut io::stdout().lock(), "stdout", &[s.as_bytes(), b"\n"]); }
    pub fn galat(s: &str) { tulis_ke(&mut io::stderr().lock(), "stderr", &[s.as_bytes()]); }
    pub fn galat_baris(s: &str) { tulis_ke(&mut io::stderr().lock(), "stderr", &[s.as_bytes(), b"\n"]); }
    pub fn siram() {
        if let Err(e) = io::stdout().flush() { if e.kind() == io::ErrorKind::BrokenPipe { super::pipa_putus() } }
    }

    /// Seperti `print!`, kecuali pipa yang ditutup pembacanya: itu bukan galat program
    /// melainkan pembaca yang sudah pergi (`super::pipa_putus`).
    fn tulis_ke(w: &mut impl Write, nama: &str, potong: &[&[u8]]) {
        for p in potong {
            if let Err(e) = w.write_all(p) {
                if e.kind() == io::ErrorKind::BrokenPipe { super::pipa_putus() }
                panic!("failed printing to {}: {}", nama, e);
            }
        }
    }
    pub fn baca_baris(buf: &mut String) -> io::Result<usize> {
        let mut kunci = io::stdin().lock();
        let mut baris: Vec<u8> = Vec::new();
        loop {
            {
                let mut q = antre();
                if let Some(pos) = q.iter().position(|b| *b == b'\n') {
                    baris.extend(q.drain(..=pos));
                    break;
                }
                baris.extend(q.drain(..));
            }
            match tarik(&mut kunci) {
                Ok(0) => break,
                Ok(_) => {}
                // Galat baca tidak boleh menelan yang sudah terambil: dikembalikan ke depan.
                Err(e) => {
                    let mut q = antre();
                    for b in baris.into_iter().rev() { q.push_front(b); }
                    return Err(e);
                }
            }
        }
        // Sama dengan `BufRead::read_line`: byte yang bukan UTF-8 adalah galat, dan
        // `buf` tidak disentuh.
        let teks = String::from_utf8(baris).map_err(|_| io::Error::new(
            io::ErrorKind::InvalidData, "stream did not contain valid UTF-8"))?;
        buf.push_str(&teks);
        Ok(teks.len())
    }
    pub fn sejak_epoch() -> Option<Duration> { SystemTime::now().duration_since(UNIX_EPOCH).ok() }
    pub fn acak(_: usize) -> Option<Vec<u8>> { None }
    pub fn tidur(d: Duration) { std::thread::sleep(d) }
    pub fn keluar(kode: i32) -> ! {
        // `process::exit` tidak menjalankan `Drop` milik `static`, jadi terminal yang
        // dibuat raw oleh programnya harus dipulihkan DI SINI, bukan diandaikan.
        crate::term::pulihkan();
        std::process::exit(kode)
    }
    pub fn akhiri_pipa_putus() -> ! {
        // SIGPIPE dengan tindakan bawaannya, seperti `cat`: pemanggil melihat proses yang
        // dihentikan sinyal 13 (141 di shell). Rust mengabaikan SIGPIPE sejak awal proses.
        #[cfg(unix)]
        unsafe {
            extern "C" { fn signal(sinyal: i32, tindakan: usize) -> usize; fn raise(sinyal: i32) -> i32; }
            signal(13, 0);
            raise(13);
        }
        std::process::exit(141)
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::io;
    use std::time::Duration;

    #[link(wasm_import_module = "jrg")]
    extern "C" {
        #[link_name = "tulis"]
        fn js_tulis(fd: i32, p: *const u8, n: usize);
        #[link_name = "sekarang_ms"]
        fn js_sekarang_ms() -> f64;
        #[link_name = "isi_acak"]
        fn js_isi_acak(p: *mut u8, n: usize);
        // Sisi JS MELEMPAR di sini, jadi pemanggilannya tidak pernah kembali.
        #[link_name = "keluar"]
        fn js_keluar(kode: i32) -> !;
    }

    fn kirim(fd: i32, s: &str) { unsafe { js_tulis(fd, s.as_ptr(), s.len()) } }

    pub fn tulis(s: &str) { kirim(1, s); }
    // Satu pesan per baris, bukan dua: tiap potongan jadi satu `postMessage`.
    fn dengan_baris(s: &str) -> String { let mut t = String::with_capacity(s.len() + 1); t.push_str(s); t.push('\n'); t }
    pub fn tulis_baris(s: &str) { kirim(1, &dengan_baris(s)); }
    pub fn galat(s: &str) { kirim(2, s); }
    pub fn galat_baris(s: &str) { kirim(2, &dengan_baris(s)); }
    pub fn siram() {}
    pub fn baca_baris(_: &mut String) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::Unsupported,
            "di peramban tidak ada masukan dari keyboard — jalankan dengan `jrgc` di mesinmu"))
    }
    pub fn ada_masukan() -> bool { false }
    pub fn baca_masukan(_: &mut [u8]) -> io::Result<usize> { baca_baris(&mut String::new()) }
    pub fn sejak_epoch() -> Option<Duration> {
        Some(Duration::from_secs_f64(unsafe { js_sekarang_ms() } / 1000.0))
    }
    pub fn acak(n: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; n];
        // `crypto.getRandomValues` menolak lebih dari 65536 byte sekaligus.
        for potong in buf.chunks_mut(65536) {
            unsafe { js_isi_acak(potong.as_mut_ptr(), potong.len()) }
        }
        Some(buf)
    }
    pub fn tidur(d: Duration) {
        // Program berjalan di Web Worker, jadi menunggu di sini tidak membekukan halaman.
        let akhir = sejak_epoch().unwrap_or_default() + d;
        while sejak_epoch().unwrap_or_default() < akhir {}
    }
    pub fn keluar(kode: i32) -> ! { unsafe { js_keluar(kode) } }
    pub fn akhiri_pipa_putus() -> ! { keluar(141) }
}

/// Keluaran program, tanpa ganti baris — `print!`.
/// Sebab sebuah galat I/O dalam kalimat JRG — tanpa ini kalimat OS (`No such file or
/// directory (os error 2)`) bocor ke pesan. Tiap pintu yang melaporkan `io::Error` lewat sini.
pub fn galat_io(e: &io::Error) -> String {
    use io::ErrorKind as K;
    let teks = match e.kind() {
        K::NotFound => "berkas atau direktorinya tidak ada",
        K::PermissionDenied => "tidak diizinkan (izin aksesnya kurang)",
        K::AlreadyExists => "sudah ada",
        K::IsADirectory => "itu direktori, bukan berkas",
        K::NotADirectory => "itu bukan direktori",
        K::DirectoryNotEmpty => "direktorinya tidak kosong",
        K::ReadOnlyFilesystem => "sistem berkasnya cuma bisa dibaca",
        K::StorageFull => "penyimpanannya penuh",
        K::FileTooLarge => "berkasnya terlalu besar",
        K::InvalidFilename => "nama berkasnya tidak sah",
        K::InvalidData => "isinya bukan teks UTF-8 yang sah",
        K::UnexpectedEof => "isinya habis sebelum selesai dibaca",
        K::WriteZero => "tidak ada yang bisa ditulis lagi ke sana",
        K::ConnectionRefused => "sambungan ditolak (tidak ada yang mendengarkan di sana)",
        K::ConnectionReset => "sambungan diputus pihak seberang",
        K::ConnectionAborted => "sambungan dibatalkan",
        K::NotConnected => "belum tersambung",
        K::AddrInUse => "alamatnya sedang dipakai",
        K::AddrNotAvailable => "alamat itu tidak ada di mesin ini",
        K::HostUnreachable => "mesin tujuannya tidak terjangkau",
        K::NetworkUnreachable => "jaringannya tidak terjangkau",
        K::NetworkDown => "jaringannya mati",
        K::BrokenPipe => "pipanya sudah putus",
        K::TimedOut => "waktunya habis",
        K::WouldBlock => "belum ada yang siap (waktunya habis)",
        K::Interrupted => "terpotong sinyal",
        K::OutOfMemory => "memorinya habis",
        K::ResourceBusy => "sedang dipakai",
        K::CrossesDevices => "asal dan tujuannya di perangkat berbeda",
        K::ArgumentListTooLong => "daftar argumennya terlalu panjang",
        K::Unsupported => "tidak didukung di sistem ini",
        _ => "",
    };
    if !teks.is_empty() { return teks.to_string(); }
    match (e.raw_os_error(), e.get_ref()) {
        (Some(n), _) => format!("ditolak sistem operasi (galat nomor {})", n),
        (None, Some(d)) => d.to_string(),
        (None, None) => "galat I/O yang tidak dikenal".to_string(),
    }
}

pub fn tulis(s: &str) { imp::tulis(s) }
/// Keluaran program plus ganti baris — `println!`.
pub fn tulis_baris(s: &str) { imp::tulis_baris(s) }
/// Ke stderr, tanpa ganti baris — `eprint!`.
pub fn galat(s: &str) { imp::galat(s) }
/// Ke stderr plus ganti baris — `eprintln!`.
pub fn galat_baris(s: &str) { imp::galat_baris(s) }
/// Dorong stdout keluar sekarang.
pub fn siram() { imp::siram() }
/// Satu baris dari stdin, seperti `BufRead::read_line`.
pub fn baca_baris(buf: &mut String) -> io::Result<usize> { imp::baca_baris(buf) }
/// Byte mentah dari stdin (untuk tombol), lewat antrean yang SAMA dengan [`baca_baris`].
/// `Ok(0)`: tidak ada byte — masukannya habis, atau terminal raw belum menerima apa-apa.
pub fn baca_masukan(buf: &mut [u8]) -> io::Result<usize> { imp::baca_masukan(buf) }
/// Apakah masih ada byte yang sudah ditarik dari stdin tapi belum dibaca.
pub fn ada_masukan() -> bool { imp::ada_masukan() }
/// Waktu sejak epoch Unix; `None` kalau jam mesinnya sebelum 1970.
pub fn sejak_epoch() -> Option<Duration> { imp::sejak_epoch() }
/// `n` byte dari sumber acak lingkungan; `None` kalau lingkungan ini tidak
/// menyediakannya sendiri (di mesin biasa: pakai `/dev/urandom`).
pub fn acak(n: usize) -> Option<Vec<u8>> { imp::acak(n) }
/// Tunggu sekurang-kurangnya `d`.
pub fn tidur(d: Duration) { imp::tidur(d) }
/// Akhiri program dengan kode `kode`.
pub fn keluar(kode: i32) -> ! { imp::keluar(kode) }

/// Pembaca keluarannya sudah pergi (`jrgc p.jrg | head -2`): program berakhir diam-diam
/// seperti `cat` dan Go, terminalnya dipulihkan lebih dulu.
pub fn pipa_putus() -> ! {
    crate::term::pulihkan();
    imp::akhiri_pipa_putus()
}

/// Sesudah terminal dipulihkan pemanggilnya — untuk pengait panik.
pub fn akhiri_pipa_putus() -> ! { imp::akhiri_pipa_putus() }

/// Apakah panik ini milik `print!`/`println!` std yang menulis ke pipa tertutup
/// (EPIPE 32; di Windows ERROR_NO_DATA 232) — kalimatnya dari `std::io::stdio`.
pub fn panik_pipa_putus(isi: &(dyn std::any::Any + Send)) -> bool {
    let s = isi.downcast_ref::<String>().map(|s| s.as_str())
        .or_else(|| isi.downcast_ref::<&str>().copied()).unwrap_or("");
    s.starts_with("failed printing to std") && (s.contains("(os error 32)") || s.contains("(os error 232)"))
}

/// Kode keluar yang sah untuk `exit`: 0 sampai 255. Di Unix cuma 8 bit terbawahnya
/// yang sampai ke pemanggil, jadi `exit(256)` akan terbaca 0 — sukses — tanpa suara.
pub fn kode_keluar(n: i128) -> Result<i32, String> {
    if (0..=255).contains(&n) { return Ok(n as i32); }
    Err(pesan_kode_keluar_luar(&n.to_string()))
}

/// Kalimat untuk kode keluar di luar 0–255; `nilai` ditulis apa adanya (backend LLVM
/// mengisinya dengan penanda, lalu menyisipkan angkanya saat jalan).
pub fn pesan_kode_keluar_luar(nilai: &str) -> String {
    format!("`exit`: kode keluar harus 0 sampai 255, dapat {} — cuma 8 bit terbawahnya yang \
             sampai ke pemanggil, jadi 256 pun akan terbaca 0 (sukses)", nilai)
}

/// Kalimat untuk `exit(x)` dengan `x` bukan bilangan bulat — dulu diam-diam keluar 0.
pub fn pesan_kode_keluar(tipe: &str) -> String {
    format!("`exit`: kode keluar harus bilangan bulat 0 sampai 255, dapat `{}`", tipe)
}

/// Kalimat kalau thread gagal dibuat karena lingkungannya memang TANPA thread
/// (peramban) — `None` untuk kegagalan OS biasa, yang kalimatnya milik pemanggil.
pub fn tanpa_thread(apa: &str, e: &io::Error) -> Option<String> {
    (e.kind() == io::ErrorKind::Unsupported).then(|| format!(
        "{} butuh thread, dan lingkungan ini tidak menyediakannya — di peramban `spawn`, \
         `go`, `Future` dan `Task` tidak bisa jalan; jalankan dengan `jrgc` di mesinmu", apa))
}

#[cfg(test)]
mod tes {
    /// `baca_baris` dan pembaca tombol memakai SATU antrean: baris diambil utuh sampai
    /// `\n`, sisanya tetap menunggu pembaca berikutnya, dan yang bukan UTF-8 ditolak.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn antrean_masukan_dibagi_baris_dan_byte() {
        // Tiap pembacaan di bawah terpenuhi dari antrean, jadi stdin sungguhan tidak disentuh.
        super::imp::isi_antrean_uji(b"satu\ndua\nx\xff\ny");
        let mut b = String::new();
        assert_eq!(super::baca_baris(&mut b).unwrap(), 5);
        assert_eq!(b, "satu\n");
        let mut dua = [0u8; 2];
        assert_eq!(super::baca_masukan(&mut dua).unwrap(), 2);
        assert_eq!(&dua, b"du");
        let mut b = String::new();
        assert_eq!(super::baca_baris(&mut b).unwrap(), 2);
        assert_eq!(b, "a\n");
        let mut b = String::from("utuh");
        assert!(super::baca_baris(&mut b).is_err());
        assert_eq!(b, "utuh");
        assert!(super::ada_masukan());
        let mut sisa = [0u8; 8];
        assert_eq!(super::baca_masukan(&mut sisa).unwrap(), 1);
        assert_eq!(sisa[0], b'y');
        assert!(!super::ada_masukan());
    }

    #[test]
    fn kode_keluar_cuma_0_sampai_255() {
        assert_eq!(super::kode_keluar(0), Ok(0));
        assert_eq!(super::kode_keluar(255), Ok(255));
        // 256 dan 2^32 dulu dipotong jadi 0: program yang melapor gagal terbaca sukses.
        assert!(super::kode_keluar(256).unwrap_err().contains("dapat 256 —"));
        assert!(super::kode_keluar(4294967296).unwrap_err().contains("dapat 4294967296 —"));
        assert!(super::kode_keluar(-1).unwrap_err().contains("dapat -1 —"));
        assert!(super::kode_keluar(i128::MIN).is_err());
    }

    /// Kalimat "tanpa thread" cuma untuk lingkungan yang memang tanpa thread;
    /// kegagalan OS biasa tetap memakai kalimat pemanggilnya.
    #[test]
    fn kalimat_tanpa_thread_cuma_untuk_lingkungan_tanpa_thread() {
        let e = std::io::Error::new(std::io::ErrorKind::Unsupported, "x");
        assert!(super::tanpa_thread("goroutine", &e).unwrap().contains("butuh thread"));
        let e = std::io::Error::new(std::io::ErrorKind::OutOfMemory, "x");
        assert!(super::tanpa_thread("goroutine", &e).is_none());
    }
}
