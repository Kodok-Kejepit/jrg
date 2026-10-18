// tests/nada.rs — tests/nada.rs — mengunci warna, pembatas, dan pilihan nada dari CLI.

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }
fn jrgm() -> &'static str { env!("CARGO_BIN_EXE_jrgm") }

struct Berkas(std::path::PathBuf);
impl Berkas {
    fn baru(isi: &str) -> Berkas {
        let d = crate::bersama::dir_unik("jrg-nada");
        std::fs::create_dir_all(d.join("src")).expect("bikin dir");
        std::fs::write(d.join("jrgm.toml"),
            "[package]\nname = \"uji\"\nversion = \"0.1.0\"\n").expect("tulis toml");
        std::fs::write(d.join("src/main.jrg"), isi).expect("tulis sumber");
        Berkas(d)
    }
    fn jrgc(&self, arg: &[&str]) -> (String, bool) {
        let out = Command::new(jrgc()).arg("src/main.jrg").args(arg)
            .current_dir(&self.0).output().expect("jalankan jrgc");
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)), out.status.success())
    }
    fn jrgm(&self, arg: &[&str]) -> (String, bool) {
        let out = Command::new(jrgm()).args(arg)
            .current_dir(&self.0).output().expect("jalankan jrgm");
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)), out.status.success())
    }
}
impl Drop for Berkas {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

const SALAH: &str = "package main\npub fn main() { let a = [1,2,3]; wok(a[9]) }\n";

/// Enam nada semuanya bisa dipilih, dan ikut berpengaruh.
#[test]
fn semua_nada_bisa_dipilih() {
    let f = Berkas::baru(SALAH);
    let mut sapaan = Vec::new();
    for n in ["sopan", "galak", "judes", "hangat", "cool", "lawak"] {
        let (keluaran, sukses) = f.jrgc(&[&format!("--nada={}", n)]);
        assert!(!sukses, "program salah seharusnya gagal ({})", n);
        assert!(
            keluaran.contains("di luar batas"),
            "nada `{}` menghilangkan pesan intinya:\n{}", n, keluaran
        );
        let baris1 = keluaran.lines().next().unwrap_or("").to_string();
        if n == "sopan" {
            assert!(
                baris1.starts_with("error:"),
                "`sopan` seharusnya tanpa sapaan:\n{}", keluaran
            );
        } else {
            assert!(
                !baris1.starts_with("error:"),
                "nada `{}` seharusnya menambahkan sapaan:\n{}", n, keluaran
            );
            sapaan.push(baris1);
        }
    }
    // Kelimanya harus benar-benar berbeda — bukan lima nama untuk satu
    // kalimat. Kalau ada dua yang sama, salah satu tabelnya tidak dipakai.
    for (i, a) in sapaan.iter().enumerate() {
        for b in sapaan.iter().skip(i + 1) {
            assert_ne!(a, b, "dua nada memberi sapaan identik:\n{}\n{}", a, b);
        }
    }
}

/// Nama nada yang salah ditolak, bukan diam-diam jatuh ke bawaan.
#[test]
fn nama_nada_salah_ditolak() {
    let f = Berkas::baru("package main\npub fn main() { wok(1) }\n");
    for (alat, keluaran) in [
        ("jrgc", f.jrgc(&["--nada=hanget"]).0),
        ("jrgm", f.jrgm(&["--nada=hanget", "run"]).0),
    ] {
        assert!(
            keluaran.contains("tidak ada") && keluaran.contains("hangat"),
            "{}: nama nada salah harus ditolak DAN menyebutkan yang tersedia:\n{}",
            alat, keluaran
        );
    }
}

/// `--nada` berlaku di `jrgm` juga, untuk apa pun subperintahnya.
#[test]
fn nada_berlaku_di_jrgm() {
    let f = Berkas::baru(SALAH);
    let (sopan, _) = f.jrgm(&["--nada=sopan", "run"]);
    assert!(sopan.starts_with("error:"), "jrgm --nada=sopan:\n{}", sopan);
    let (hangat, _) = f.jrgm(&["--nada=hangat", "run"]);
    assert!(!hangat.starts_with("error:"), "jrgm --nada=hangat:\n{}", hangat);
    // Badan diagnostiknya tetap lengkap di kedua nada.
    for (n, k) in [("sopan", &sopan), ("hangat", &hangat)] {
        for wajib in ["di luar batas", "-->", "^"] {
            assert!(k.contains(wajib), "jrgm --nada={} kehilangan `{}`:\n{}", n, wajib, k);
        }
    }
}

/// Tanpa terminal: TIDAK ada kode warna dan TIDAK ada pembatas.
#[test]
fn tanpa_terminal_tidak_berwarna_dan_tanpa_pembatas() {
    let f = Berkas::baru(SALAH);
    for arg in [vec!["--nada=galak"], vec!["--nada=sopan"], vec!["--nada=lawak"]] {
        let (keluaran, _) = f.jrgc(&arg);
        assert!(
            !keluaran.contains('\x1b'),
            "ada kode escape ANSI padahal stderr bukan terminal ({:?}):\n{:?}", arg, keluaran
        );
        for hiasan in ["ada yang salah", "───", "┈┈┈"] {
            assert!(
                !keluaran.contains(hiasan),
                "pembatas `{}` muncul padahal bukan terminal ({:?}):\n{}",
                hiasan, arg, keluaran
            );
        }
    }
}

/// `NO_COLOR` dihormati bahkan kalau stderr terminal.
#[test]
fn no_color_tidak_bikin_pesan_hilang() {
    let f = Berkas::baru(SALAH);
    let out = Command::new(jrgc()).arg("src/main.jrg").arg("--nada=galak")
        .env("NO_COLOR", "1").current_dir(&f.0).output().expect("jalankan");
    let keluaran = format!("{}{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!keluaran.contains('\x1b'), "NO_COLOR diabaikan:\n{:?}", keluaran);
    assert!(keluaran.contains("di luar batas"), "pesan intinya hilang:\n{}", keluaran);
}

/// `JRG_NADA` di lingkungan berlaku, dan CLI mengalahkannya.
#[test]
fn variabel_lingkungan_berlaku_dan_cli_mengalahkannya() {
    let f = Berkas::baru(SALAH);
    let jalan = |env: &[(&str, &str)], arg: &[&str]| -> String {
        let mut c = Command::new(jrgc());
        c.arg("src/main.jrg").args(arg).current_dir(&f.0);
        for (k, v) in env { c.env(k, v); }
        let out = c.output().expect("jalankan");
        format!("{}{}", String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr))
    };
    // Lewat lingkungan saja: sapaannya muncul.
    let via_env = jalan(&[("JRG_NADA", "judes")], &[]);
    assert!(!via_env.starts_with("error:"), "JRG_NADA=judes tidak berlaku:\n{}", via_env);
    // CLI menang: `--nada=sopan` mematikan sapaan walau lingkungannya judes.
    let cli_menang = jalan(&[("JRG_NADA", "judes")], &["--nada=sopan"]);
    assert!(
        cli_menang.starts_with("error:"),
        "`--nada` di CLI seharusnya mengalahkan `JRG_NADA`:\n{}", cli_menang
    );
    // Nama tak dikenal di lingkungan DIABAIKAN (jatuh ke bawaan), bukan membuat
    // program gagal — variabel lingkungan sering warisan dari sesi lain, dan …
    let env_ngawur = jalan(&[("JRG_NADA", "hanget")], &[]);
    assert!(
        env_ngawur.contains("di luar batas"),
        "JRG_NADA yang tidak dikenal seharusnya diabaikan, bukan menggagalkan:\n{}",
        env_ngawur
    );
}

/// Baris `bantuan:`/`catatan:` milik sebuah galat parse dicetak SESUDAH lokasi
/// galat itu sendiri. Dulu dicetak sebelumnya, jadi di layar ia terbaca sebagai
/// milik galat di atasnya — lewat `jrgc` maupun `jrgm`.
#[test]
fn bantuan_galat_parse_menempel_pada_galatnya_sendiri() {
    let b = Berkas::baru("package main\npub fn main() {\n  wok(1 2)\n  let y: Hntu = 5\n}\n");
    for (keluar, sukses) in [b.jrgc(&["--sopan"]), b.jrgm(&["--sopan", "run"])] {
        assert!(!sukses, "program yang salah harus ditolak:\n{}", keluar);
        let posisi = |p: &str| keluar.find(p).unwrap_or_else(|| panic!("tidak ada `{}`:\n{}", p, keluar));
        let tipe = posisi("tipe `Hntu` tidak ada");
        let lokasi = tipe + keluar[tipe..].find("-->").expect("lokasi galat tipe");
        assert!(posisi("bantuan: deklarasikan") > lokasi && posisi("catatan: JRG TIDAK") > lokasi,
            "bantuan/catatan tercetak sebelum lokasi galatnya:\n{}", keluar);
    }
}
