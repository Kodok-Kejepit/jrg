// tests/fmt.rs — tests/fmt.rs — mengunci `jrgm fmt` dan penolakan subperintah yang rumpang.

mod bersama;
use jrgc::fmt::format_sumber;
use jrgc::lexer::{lex, SourceFile};
use std::path::{Path, PathBuf};
use std::process::Command;

fn sf(nama: &str, src: &str) -> SourceFile {
    SourceFile { id: 0, name: nama.to_string(), src: src.to_string() }
}

fn format(src: &str) -> String {
    format_sumber(&sf("uji.jrg", src)).expect("format gagal")
}

/// Aliran token, tanpa span — dua bentuk sebuah program setara kalau dan
/// hanya kalau daftar ini sama persis.
fn token(src: &str) -> Vec<String> {
    lex(&sf("uji.jrg", src))
        .expect("lex gagal")
        .into_iter()
        .map(|t| format!("{:?}", t.kind))
        .collect()
}

fn korpus() -> Vec<PathBuf> {
    let akar = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut v = Vec::new();
    for d in ["examples", "stdlib", "tests/conformance", "examples_llvm"] {
        let Ok(baca) = std::fs::read_dir(akar.join(d)) else { continue };
        for e in baca.flatten() {
            let p = e.path();
            if p.extension().map_or(false, |x| x == "jrg") {
                v.push(p);
            }
        }
    }
    v.sort();
    assert!(v.len() >= 30, "korpus menyusut jadi {} berkas — tesnya jadi tidak berarti", v.len());
    v
}

// ─── Sifat pokok ───────────────────────────────────────────────────────────

/// Sifat yang membuat formatter ini boleh dipercaya sama sekali.
#[test]
fn format_tidak_pernah_mengubah_aliran_token() {
    for p in korpus() {
        let src = std::fs::read_to_string(&p).unwrap();
        let hasil = format_sumber(&sf(&p.display().to_string(), &src))
            .unwrap_or_else(|e| panic!("{} gagal diformat: {}", p.display(), e));

        let sebelum = token(&src);
        let sesudah = token(&hasil);
        assert_eq!(
            sebelum.len(),
            sesudah.len(),
            "{}: jumlah token berubah {} -> {}",
            p.display(),
            sebelum.len(),
            sesudah.len()
        );
        let beda = sebelum.iter().zip(&sesudah).position(|(a, b)| a != b);
        assert!(
            beda.is_none(),
            "{}: token ke-{} berubah dari {} jadi {}",
            p.display(),
            beda.unwrap(),
            sebelum[beda.unwrap()],
            sesudah[beda.unwrap()]
        );
    }
}

/// Memformat hasil format harus tidak mengubah apa pun.
#[test]
fn format_idempoten() {
    for p in korpus() {
        let src = std::fs::read_to_string(&p).unwrap();
        let satu = format_sumber(&sf("x", &src)).unwrap();
        let dua = format_sumber(&sf("x", &satu)).unwrap();
        assert_eq!(satu, dua, "{}: format tidak idempoten", p.display());
    }
}

/// Komentar tidak boleh hilang, dan isinya tidak boleh berubah.
#[test]
fn komentar_utuh_isi_dan_jumlahnya() {
    for p in korpus() {
        let src = std::fs::read_to_string(&p).unwrap();
        let hasil = format_sumber(&sf("x", &src)).unwrap();
        for (i, baris) in src.lines().enumerate() {
            let Some(pos) = baris.find("//") else { continue };
            // Lewati `//` yang ada di dalam string literal — bukan komentar.
            if baris[..pos].matches('"').count() % 2 == 1 {
                continue;
            }
            let isi = baris[pos..].trim_end();
            assert!(
                hasil.contains(isi),
                "{}:{}: komentar hilang atau berubah: {:?}",
                p.display(),
                i + 1,
                isi
            );
        }
    }
}

/// Template literal disalin apa adanya, termasuk spasi yang ada di dalamnya.
#[test]
fn spasi_di_dalam_template_tidak_disentuh() {
    let src = "package main\npub fn main(){ let x=1\nwok(«a  b   ${x}  c») }\n";
    let hasil = format(src);
    assert!(
        hasil.contains("«a  b   ${x}  c»"),
        "spasi di dalam template berubah:\n{}",
        hasil
    );
}

/// Komentar di dalam `${...}` sebuah template.
#[test]
fn komentar_di_dalam_interpolasi_tidak_ganda_dan_tidak_panik() {
    let src = "package main\npub fn main(){ let x=5\nwok(«nilai ${ x /* c */ } habis») }\n";
    let hasil = format(src);
    assert_eq!(
        hasil.matches("/* c */").count(),
        1,
        "komentar di dalam interpolasi muncul {} kali:\n{}",
        hasil.matches("/* c */").count(),
        hasil
    );
    assert_eq!(token(src), token(&hasil), "aliran token berubah");
}

// ─── Keputusan bentuk baku ─────────────────────────────────────────────────

/// Tiap baris di bawah adalah keputusan yang DIUKUR dari korpus, bukan
/// selera. Angkanya ada di komentar `src/fmt/mod.rs`.
#[test]
fn bentuk_baku_sesuai_yang_diukur() {
    let kasus: &[(&str, &str, &str)] = &[
        ("indentasi 4 spasi",
         "package main\npub fn main(){\nwok(1)\n}\n",
         "\n    wok(1)\n"),
        ("spasi sebelum `{`",
         "package main\npub fn main(){ wok(1) }\n",
         "pub fn main() {"),
        ("isi kurawal sebaris berjarak",
         "package main\nstruct T { pub a: int }\npub fn main(){ let t=T{a:1}\nwok(t.a) }\n",
         "T { a: 1 }"),
        ("kurawal kosong tetap rapat",
         "package main\nstruct T { pub a: int }\npub fn main(){ let t=T{}\nwok(t.a) }\n",
         "T {}"),
        ("`:` menempel kiri, berjarak kanan",
         "package main\npub fn main(){ let x:int=1\nwok(x) }\n",
         "let x: int = 1"),
        ("`,` berjarak sesudahnya saja",
         "package main\npub fn main(){ wok([1,2,3]) }\n",
         "[1, 2, 3]"),
        ("operator biner tak-ambigu diberi spasi",
         "package main\npub fn main(){ wok(1+2) }\n",
         "wok(1 + 2)"),
        ("minus awalan TIDAK direnggangkan",
         "package main\npub fn main(){ wok(-5) }\n",
         "wok(-5)"),
        ("kurung generik TIDAK direnggangkan",
         "package main\nstruct K<T> { pub v: T }\npub fn main(){ let k=K{v:1}\nwok(k.v) }\n",
         "struct K<T> {"),
        ("pemanggilan menempel ke namanya",
         "package main\npub fn main(){ wok (1) }\n",
         "wok(1)"),
        ("baris kosong beruntun mengerut jadi satu",
         "package main\n\n\n\n\npub fn main(){ wok(1) }\n",
         "package main\n\npub fn main"),
        // `::nama` (Fase RUANG) diatur seperti nama: dulu spasi di depannya dibuang.
        ("`::` awalan berjarak seperti sebuah nama",
         "package main\npub fn main(){ wok(1, ::len(\"ab\"))\nlet f = ::abs\nwok(f(-1), chan::len) }\n",
         "wok(1, ::len(\"ab\"))\n    let f = ::abs\n    wok(f(-1), chan::len)"),
    ];
    for (nama, masuk, harus_ada) in kasus {
        let hasil = format(masuk);
        assert!(
            hasil.contains(harus_ada),
            "{}: tidak menemukan {:?} di hasil:\n{}",
            nama, harus_ada, hasil
        );
    }
}

/// Berkas selalu berakhir dengan tepat satu newline.
#[test]
fn akhir_berkas_tepat_satu_newline() {
    for masuk in [
        "package main\npub fn main(){ wok(1) }",
        "package main\npub fn main(){ wok(1) }\n\n\n\n",
        "package main\npub fn main(){ wok(1) }\n   \n",
    ] {
        let hasil = format(masuk);
        assert!(hasil.ends_with('\n'), "tidak diakhiri newline");
        assert!(!hasil.ends_with("\n\n"), "diakhiri lebih dari satu newline: {:?}", hasil);
    }
}

/// Sumber yang tidak bisa dilex ditolak, bukan ditulis sebagian.
#[test]
fn sumber_rusak_ditolak_bukan_ditulis() {
    let hasil = format_sumber(&sf("x", "package main\npub fn main(){ let s = \"belum ditutup\n"));
    assert!(hasil.is_err(), "sumber rusak malah diterima");
}

// ─── Subperintah yang menolak ──────────────────────────────────────────────

fn jrgm() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") { p.pop(); }
    p.join("jrgm")
}

/// `jrgm new --kind web` harus menghasilkan aplikasi yang LANGSUNG MELAYANI,
/// bukan kerangka yang masih perlu ditambal.
#[test]
fn rangka_web_langsung_melayani_dan_melolosi_html() {
    let j = jrgm();
    if !j.exists() {
        eprintln!("LEWAT: biner jrgm belum dibangun di {}", j.display());
        return;
    }
    let tmp = crate::bersama::dir_unik("jrg_rangka_web");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let out = Command::new(&j).args(["new", "situs", "--kind", "web"])
        .current_dir(&tmp).output().unwrap();
    assert!(out.status.success(), "jrgm new --kind web gagal: {}",
        String::from_utf8_lossy(&out.stderr));

    let entry = tmp.join("situs").join("src").join("main.jrg");
    assert!(entry.exists(), "src/main.jrg tidak dibuat");

    // Port dipilih dengan MEMBUKA listener lalu menutupnya, supaya dua tes yang
    // jalan bersamaan tidak berebut nomor yang sama.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let isi = std::fs::read_to_string(&entry).unwrap().replace("8080", &port.to_string());
    std::fs::write(&entry, &isi).unwrap();

    let mut anak = Command::new(env!("CARGO_BIN_EXE_jrgc")).arg(&entry).arg("--sopan")
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().expect("jalankan server");

    let ambil = |path: &str| -> Option<String> {
        use std::io::{Read, Write};
        for _ in 0..60 {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let req = format!("GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", path);
                if s.write_all(req.as_bytes()).is_ok() {
                    let mut b = String::new();
                    if s.read_to_string(&mut b).is_ok() && !b.is_empty() { return Some(b); }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        None
    };

    let beranda = ambil("/");
    let api = ambil("/api");
    // `<script>` di URL harus tampil terlolosi — kalau tidak, kerangka bawaan
    // ini memasang lubang XSS di setiap proyek baru.
    let nakal = ambil("/halo/%3Cscript%3E");
    let hilang = ambil("/tidak-ada-sama-sekali");
    let _ = anak.kill();
    let _ = anak.wait();
    let _ = std::fs::remove_dir_all(&tmp);

    let beranda = beranda.expect("server tidak menjawab / sama sekali");
    assert!(beranda.contains("200"), "beranda bukan 200:\n{}", &beranda[..beranda.len().min(200)]);
    assert!(beranda.contains("situs"), "judul proyek tidak muncul di halaman");
    let api = api.expect("/api tidak menjawab");
    assert!(api.contains("\"ok\""), "/api tidak mengembalikan JSON:\n{}", &api[..api.len().min(200)]);
    let nakal = nakal.expect("/halo/... tidak menjawab");
    assert!(nakal.contains("&lt;script&gt;"),
        "nama dari URL TIDAK dilolosi — kerangka bawaan tidak boleh memasang \
         lubang XSS:\n{}", &nakal[..nakal.len().min(300)]);
    assert!(!nakal.contains("<script>"), "tag mentah lolos ke halaman");
    let hilang = hilang.expect("404 tidak menjawab");
    assert!(hilang.contains("404"), "path tak dikenal bukan 404");
}

/// Tiap subperintah yang belum ada isinya harus KELUAR TIDAK NOL dan menyebut
/// alasan yang sebenarnya.
#[test]
fn subperintah_rumpang_menolak_bukan_mengaku_sukses() {
    let j = jrgm();
    if !j.exists() {
        eprintln!("LEWAT: biner jrgm belum dibangun di {}", j.display());
        return;
    }
    let tmp = std::env::temp_dir().join("jrg_fmt_tolak");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Tiap kasus: argumen, dan satu kata yang WAJIB ada di pesannya.
    let kasus: &[(&[&str], &str)] = &[
        (&["add", "serde"], "registry"),
        (&["update"], "jrgm.toml"),
        (&["lint"], "jrgm.toml"),
        (&["publish"], "jrgm.toml"),
    ];
    for (args, wajib) in kasus {
        let out = Command::new(&j).args(*args).current_dir(&tmp).output().unwrap();
        let teks = String::from_utf8_lossy(&out.stderr).to_string()
            + &String::from_utf8_lossy(&out.stdout);
        assert!(
            !out.status.success(),
            "`jrgm {}` masih keluar sukses — inilah bug yang fase PERKAKAS-1 tutup.\n{}",
            args.join(" "), teks
        );
        assert!(
            teks.contains(wajib),
            "`jrgm {}` menolak tanpa menyebut alasannya (dicari {:?}):\n{}",
            args.join(" "), wajib, teks
        );
    }
}

/// `jrgm add` tidak boleh menyentuh `jrgm.toml`.
#[test]
fn add_tidak_menyentuh_manifest() {
    let j = jrgm();
    if !j.exists() { return; }
    let tmp = std::env::temp_dir().join("jrg_fmt_manifest");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let man = tmp.join("jrgm.toml");
    std::fs::write(&man, "nama = \"x\"\nversion = \"0.1.0\"\n[dependencies]\n").unwrap();
    let sebelum = std::fs::read(&man).unwrap();

    let _ = Command::new(&j).args(["add", "serde"]).current_dir(&tmp).output().unwrap();
    assert_eq!(sebelum, std::fs::read(&man).unwrap(), "`jrgm add` mengubah jrgm.toml");
}

/// `jrgm fmt --check` harus keluar TIDAK NOL kalau ada yang belum rapi.
#[test]
fn fmt_check_gagal_kalau_ada_yang_belum_rapi() {
    let j = jrgm();
    if !j.exists() { return; }
    let tmp = std::env::temp_dir().join("jrg_fmt_check");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    let berkas = tmp.join("src").join("main.jrg");
    std::fs::write(&berkas, "package main\npub fn main(){\nwok(1)\n}\n").unwrap();

    let out = Command::new(&j).arg("fmt").arg("--check").current_dir(&tmp).output().unwrap();
    assert!(!out.status.success(), "--check hijau padahal berkasnya belum rapi");
    // --check tidak boleh MENULIS.
    assert_eq!(
        std::fs::read_to_string(&berkas).unwrap(),
        "package main\npub fn main(){\nwok(1)\n}\n",
        "--check mengubah berkas"
    );

    let out = Command::new(&j).arg("fmt").current_dir(&tmp).output().unwrap();
    assert!(out.status.success(), "fmt gagal: {}", String::from_utf8_lossy(&out.stderr));
    let out = Command::new(&j).arg("fmt").arg("--check").current_dir(&tmp).output().unwrap();
    assert!(out.status.success(), "--check masih merah sesudah dirapikan");
}

// ════════════════════════════════════════════════════════════════════════
// `jrgm run --watch` — hot reload …
mod watch {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    fn jrgm() -> &'static str { env!("CARGO_BIN_EXE_jrgm") }
    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    /// Pengawas yang dibunuh saat keluar dari scope. Sebuah pengawas yang
    /// bocor terus menulis ke berkas log dan mengacaukan uji berikutnya.
    struct Watcher(Child, PathBuf);
    impl Drop for Watcher {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    fn ruang(nama: &str) -> PathBuf {
        let d = crate::bersama::dir_unik(&format!("jrg-watch-{}", nama));
        std::fs::create_dir_all(d.join("src")).expect("bikin dir");
        std::fs::write(d.join("jrgm.toml"),
            "[package]\nname = \"uji\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.jrg\"\n")
            .expect("tulis manifest");
        d
    }

    fn tulis(dir: &PathBuf, isi: &str) {
        let p = dir.join("src/main.jrg");
        let mut f = std::fs::File::create(&p).expect("tulis main.jrg");
        f.write_all(isi.as_bytes()).expect("tulis isi");
        f.sync_all().ok();
    }

    fn mulai(dir: &PathBuf) -> (Watcher, PathBuf) {
        let log = dir.join("keluaran.txt");
        let f = std::fs::File::create(&log).expect("bikin log");
        let anak = Command::new(jrgm())
            .arg("run").arg("--watch")
            .current_dir(dir)
            // `jrgm` dan `jrgc` memang bertetangga di `target/`, tapi uji
            // tidak boleh bergantung pada tata letak direktori.
            .env("JRGC_BIN", jrgc())
            .env("JRG_SOPAN", "1")
            .stdout(Stdio::from(f.try_clone().expect("klon")))
            .stderr(Stdio::from(f))
            .stdin(Stdio::null())
            .spawn().expect("jalankan jrgm run --watch");
        (Watcher(anak, dir.clone()), log)
    }

    /// Tunggu sampai `pola` muncul di log, atau menyerah.
    fn tunggu(log: &PathBuf, pola: &str, detik: u64) -> bool {
        let batas = Instant::now() + Duration::from_secs(detik);
        while Instant::now() < batas {
            if let Ok(s) = std::fs::read_to_string(log) {
                if s.contains(pola) { return true; }
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        false
    }

    fn isi(log: &PathBuf) -> String {
        std::fs::read_to_string(log).unwrap_or_default()
    }

    /// Beberapa suntingan berturut-turut, dan SEMUANYA harus terlihat.
    #[test]
    fn menyimpan_berkali_kali_mereload_berkali_kali() {
        let d = ruang("ulang");
        tulis(&d, "package main\npub fn main() { wok(\"VERSI-1\") }\n");
        let (_w, log) = mulai(&d);
        assert!(tunggu(&log, "VERSI-1", 20), "jalan pertama tidak muncul:\n{}", isi(&log));
        for n in 2..=4 {
            tulis(&d, &format!("package main\npub fn main() {{ wok(\"VERSI-{}\") }}\n", n));
            assert!(
                tunggu(&log, &format!("VERSI-{}", n), 20),
                "suntingan ke-{} tidak memicu reload — pengawas berhenti memperhatikan:\n{}",
                n, isi(&log)
            );
        }
    }

    /// Tiap reload harus mulai dari keadaan BERSIH.
    #[test]
    fn tiap_reload_mulai_dari_keadaan_bersih() {
        let d = ruang("bersih");
        let prog = |tanda: &str| format!(
            "package main\npub fn main() {{ let p = jawa::alloc(16)\n wok(\"{}\", jawa::stats()) }}\n",
            tanda);
        tulis(&d, &prog("T1"));
        let (_w, log) = mulai(&d);
        assert!(tunggu(&log, "T1", 20), "jalan pertama tidak muncul:\n{}", isi(&log));
        tulis(&d, &prog("T2"));
        assert!(tunggu(&log, "T2", 20), "reload tidak terjadi:\n{}", isi(&log));
        tulis(&d, &prog("T3"));
        assert!(tunggu(&log, "T3", 20), "reload kedua tidak terjadi:\n{}", isi(&log));

        let teks = isi(&log);
        let angka: Vec<&str> = teks.match_indices("alive_total: ")
            .map(|(i, _)| teks[i + 13..].split(|c: char| !c.is_ascii_digit()).next().unwrap_or(""))
            .collect();
        assert!(angka.len() >= 3, "harusnya ada 3 laporan, dapat {:?}\n{}", angka, teks);
        assert!(
            angka.iter().all(|a| *a == "1"),
            "keadaan global bocor antar-reload: alive_total = {:?} (harusnya 1 terus). \
             Program yang sama menjawab angka berbeda tiap simpan.\n{}",
            angka, teks
        );
    }

    /// Program yang MEMBLOKIR tidak boleh menggantungkan pengawasnya.
    #[test]
    fn program_yang_memblokir_tidak_menggantungkan_pengawas() {
        let d = ruang("blokir");
        tulis(&d, "package main\npub fn main() { wok(\"MEMBLOKIR\")\n let c = make_chan(bool)\n <-c }\n");
        let (_w, log) = mulai(&d);
        assert!(tunggu(&log, "MEMBLOKIR", 20), "jalan pertama tidak muncul:\n{}", isi(&log));
        tulis(&d, "package main\npub fn main() { wok(\"LEPAS\") }\n");
        assert!(
            tunggu(&log, "LEPAS", 20),
            "pengawas ikut menggantung: program pertama tidak pernah selesai, \
             dan suntingan berikutnya tidak pernah dijalankan.\n{}",
            isi(&log)
        );
    }

    /// `std::exit` di program tidak boleh membunuh pengawasnya.
    #[test]
    fn std_exit_tidak_membunuh_pengawas() {
        let d = ruang("exit");
        tulis(&d, "package main\npub fn main() { wok(\"KELUAR\")\n std::exit(0) }\n");
        let (_w, log) = mulai(&d);
        assert!(tunggu(&log, "KELUAR", 20), "jalan pertama tidak muncul:\n{}", isi(&log));
        tulis(&d, "package main\npub fn main() { wok(\"HIDUP-TERUS\") }\n");
        assert!(
            tunggu(&log, "HIDUP-TERUS", 20),
            "`std::exit` di program ikut membunuh pengawasnya.\n{}",
            isi(&log)
        );
    }

    /// Galat sintaks dilaporkan, dan pengawasnya TETAP hidup — memperbaikinya
    /// harus langsung jalan lagi. Sebuah pengawas yang mati pada berkas rusak
    /// akan mati justru di saat orangnya paling sering menyimpan.
    #[test]
    fn berkas_rusak_dilaporkan_tanpa_mematikan_pengawas() {
        let d = ruang("rusak");
        tulis(&d, "package main\npub fn main() { wok(\"SEHAT\") }\n");
        let (_w, log) = mulai(&d);
        assert!(tunggu(&log, "SEHAT", 20), "jalan pertama tidak muncul:\n{}", isi(&log));
        tulis(&d, "package main\npub fn main() { wok(@@@) }\n");
        assert!(tunggu(&log, "error", 20), "galat sintaks tidak dilaporkan:\n{}", isi(&log));
        tulis(&d, "package main\npub fn main() { wok(\"PULIH\") }\n");
        assert!(
            tunggu(&log, "PULIH", 20),
            "pengawas mati pada berkas yang rusak, jadi perbaikannya tidak pernah jalan.\n{}",
            isi(&log)
        );
    }

    /// Perubahan di berkas SELAIN entry ikut memicu reload.
    #[test]
    fn menyunting_modul_lain_ikut_memicu_reload() {
        let d = ruang("modul");
        std::fs::write(d.join("src/bantu.jrg"),
            "package bantu\npub fn pesan() -> string { return \"DARI-MODUL-1\" }\n").unwrap();
        tulis(&d, "package main\nimport \"bantu\"\npub fn main() { wok(bantu::pesan()) }\n");
        let (_w, log) = mulai(&d);
        assert!(tunggu(&log, "DARI-MODUL-1", 20), "jalan pertama tidak muncul:\n{}", isi(&log));
        std::fs::write(d.join("src/bantu.jrg"),
            "package bantu\npub fn pesan() -> string { return \"DARI-MODUL-2\" }\n").unwrap();
        assert!(
            tunggu(&log, "DARI-MODUL-2", 20),
            "menyunting berkas selain entry tidak memicu reload.\n{}",
            isi(&log)
        );
    }
}

/// Dependensi lokal by-path: `bantu = { path = "../bantu" }`.
mod dependensi {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn jrgm() -> &'static str { env!("CARGO_BIN_EXE_jrgm") }

    fn ruang(nama: &str) -> PathBuf {
        let d = crate::bersama::dir_unik(&format!("jrg-dep-{}", nama));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Sebuah paket: `jrgm.toml`, `src/`, dan berkas-berkas modulnya.
    fn paket(akar: &Path, nama: &str, deps: &[(&str, &str)], berkas: &[(&str, &str)]) {
        std::fs::create_dir_all(akar.join("src")).unwrap();
        let mut t = format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.jrg\"\n\n[dependencies]\n", nama);
        for (n, p) in deps { t.push_str(&format!("{} = {{ path = \"{}\" }}\n", n, p)); }
        std::fs::write(akar.join("jrgm.toml"), t).unwrap();
        for (n, isi) in berkas { std::fs::write(akar.join("src").join(n), isi).unwrap(); }
    }

    fn jalankan(args: &[&str], dir: &Path) -> (bool, String) {
        let o = Command::new(jrgm()).args(args).current_dir(dir).output().unwrap();
        (o.status.success(),
         String::from_utf8_lossy(&o.stdout).to_string()
             + &String::from_utf8_lossy(&o.stderr))
    }

    /// Galat lex dan parse lewat `jrgm` dilaporkan seperti `jrgc` — kode, lokasi,
    /// cuplikan, caret. Dulu satu baris polos `berkas:parse: pesan`, tanpa caret.
    #[test]
    fn galat_lex_dan_parse_lewat_jrgm_berlokasi() {
        let d = ruang("diag");
        paket(&d, "a", &[], &[("main.jrg", "package main\npub fn main() {\n    wok(1 + )\n}\n")]);
        for perintah in ["run", "build"] {
            let (ok, teks) = jalankan(&[perintah], &d);
            assert!(!ok, "`jrgm {}` menerima program yang tidak parse:\n{}", perintah, teks);
            assert!(teks.contains("error[E0103]") && teks.contains("main.jrg:3:13")
                    && teks.contains("|             ^") && !teks.contains(":parse:"),
                "`jrgm {}` melaporkan galat parse tanpa lokasi dan caret:\n{}", perintah, teks);
        }
        std::fs::write(d.join("src/main.jrg"), "package main\npub fn main() {\n    wok(\"x)\n}\n").unwrap();
        let (ok, teks) = jalankan(&["run"], &d);
        assert!(!ok && teks.contains("main.jrg:3:9") && !teks.contains(":lex:"),
            "`jrgm run` melaporkan galat lex tanpa lokasi:\n{}", teks);

        std::fs::write(d.join("src/main.jrg"), "package main\npub fn main() { wok(1) }\n").unwrap();
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("tests/rusak.jrg"), "fn test_a() {\n    wok(1 + )\n}\n").unwrap();
        let (ok, teks) = jalankan(&["test"], &d);
        assert!(!ok && teks.contains("rusak.jrg:2:13") && !teks.contains(":parse:"),
            "`jrgm test` melaporkan galat parse berkas uji tanpa lokasi:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Modul milik paket lain harus ditemukan `import`, di KEDUA backend.
    #[test]
    fn modul_paket_lain_ditemukan_di_dua_backend() {
        let d = ruang("pakai");
        paket(&d.join("bantu"), "bantu", &[],
              &[("matematika.jrg", "package matematika\npub fn tambah(a, b) { return a + b }\n")]);
        paket(&d.join("app"), "app", &[("bantu", "../bantu")],
              &[("main.jrg", "package main\nimport \"matematika\"\n\
                              pub fn main() { wok(matematika::tambah(20, 22)) }\n")]);

        for arg in [&[][..], &["--vm"][..]] {
            let mut a = vec!["run"]; a.extend_from_slice(arg);
            let (ok, teks) = jalankan(&a, &d.join("app"));
            assert!(ok && teks.contains("42"),
                "`jrgm run {}` tidak menemukan modul dari paket lain:\n{}",
                arg.join(" "), teks);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Dependensi transitif ikut terbawa, dan `jrgm tree` menggambarnya.
    #[test]
    fn tree_membaca_manifes_bukan_mengarang() {
        let d = ruang("tree");
        paket(&d.join("c"), "c", &[], &[("teks.jrg", "package teks\npub fn s() { return 1 }\n")]);
        paket(&d.join("b"), "b", &[("c", "../c")], &[("m.jrg", "package m\npub fn f() { return 1 }\n")]);
        paket(&d.join("a"), "a", &[("b", "../b"), ("c", "../c")],
              &[("main.jrg", "package main\npub fn main() { wok(1) }\n")]);

        let (ok, teks) = jalankan(&["tree"], &d.join("a"));
        assert!(ok, "jrgm tree gagal:\n{}", teks);
        for n in ["a 0.1.0", "b 0.1.0", "c 0.1.0"] {
            assert!(teks.contains(n), "`jrgm tree` tidak menyebut {:?}:\n{}", n, teks);
        }
        assert!(!teks.contains("std 0.1.0"),
            "`jrgm tree` masih mencetak pohon karangan:\n{}", teks);
        // `c` dipakai dari dua tempat, jadi kemunculan keduanya ditandai.
        assert!(teks.contains("(*)"), "paket berulang tidak ditandai:\n{}", teks);
        // Batang `│` di bawah `b`, karena `b` BUKAN cabang terakhir di sini.
        assert!(teks.contains("│   └── c"), "gambar pohonnya salah:\n{}", teks);

        // Dan arah sebaliknya, yang justru bugnya: kalau `b` adalah cabang TERAKHIR,
        // batang di bawahnya harus hilang — `"│ ".repeat(kedalaman)` menggambar …
        paket(&d.join("e"), "e", &[("b", "../b")],
              &[("main.jrg", "package main\npub fn main() { wok(1) }\n")]);
        let (ok, teks) = jalankan(&["tree"], &d.join("e"));
        assert!(ok, "jrgm tree gagal:\n{}", teks);
        assert!(teks.contains("    └── c") && !teks.contains("│   └── c"),
            "batang menggantung di bawah cabang terakhir:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Tiga cara sebuah graf bisa salah, masing-masing harus DITOLAK dan
    /// menyebut sebabnya. Ketiganya lewat `jrgm build`, yang memang tugasnya
    /// memeriksa — menemukannya saat `run` berarti menemukannya terlambat.
    #[test]
    fn graf_rusak_ditolak_dengan_sebabnya() {
        // (a) siklus
        let d = ruang("siklus");
        paket(&d.join("a"), "a", &[("b", "../b")],
              &[("main.jrg", "package main\npub fn main() { wok(1) }\n")]);
        paket(&d.join("b"), "b", &[("a", "../a")], &[("m.jrg", "package m\npub fn f() { return 1 }\n")]);
        let (ok, teks) = jalankan(&["build"], &d.join("a"));
        assert!(!ok, "siklus dependency diterima:\n{}", teks);
        assert!(teks.contains("melingkar") && teks.contains("a -> b -> a"),
            "siklus ditolak tanpa menyebut rantainya:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);

        // (b) path menunjuk tempat yang bukan proyek
        let d = ruang("kosong");
        paket(&d.join("a"), "a", &[("hantu", "../hantu")],
              &[("main.jrg", "package main\npub fn main() { wok(1) }\n")]);
        let (ok, teks) = jalankan(&["build"], &d.join("a"));
        assert!(!ok, "path yang menunjuk tempat kosong diterima:\n{}", teks);
        assert!(teks.contains("hantu") && teks.contains("jrgm.toml"),
            "path kosong ditolak tanpa menyebut apa yang tidak ada:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);

        // (c) dua dependency menyediakan modul bernama sama.
        let d = ruang("bentrok");
        let isi = "package m\npub fn f() { return 1 }\n";
        paket(&d.join("b"), "b", &[], &[("m.jrg", isi)]);
        paket(&d.join("c"), "c", &[], &[("m.jrg", isi)]);
        paket(&d.join("a"), "a", &[("b", "../b"), ("c", "../c")],
              &[("main.jrg", "package main\npub fn main() { wok(1) }\n")]);
        let (ok, teks) = jalankan(&["build"], &d.join("a"));
        assert!(!ok, "dua modul bernama sama diterima:\n{}", teks);
        assert!(teks.contains("`m`") && teks.contains("`b`") && teks.contains("`c`"),
            "bentrok ditolak tanpa menyebut modul dan kedua paketnya:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `jrgm add --path` menulis, `jrgm remove` menghapus, dan `jrgm add` TANPA
    /// `--path` tetap menolak.
    #[test]
    fn add_dan_remove_mengubah_manifes_sungguhan() {
        let d = ruang("add");
        paket(&d.join("bantu"), "bantu", &[],
              &[("m.jrg", "package m\npub fn f() { return 7 }\n")]);
        paket(&d.join("a"), "a", &[],
              &[("main.jrg", "package main\nimport \"m\"\npub fn main() { wok(m::f()) }\n")]);
        let a = d.join("a");
        let man = a.join("jrgm.toml");

        // Sebelum ditambahkan, modulnya memang tidak ketemu.
        let (ok, _) = jalankan(&["run"], &a);
        assert!(!ok, "modul dari paket lain ketemu sebelum ditambahkan");

        let sebelum = std::fs::read(&man).unwrap();
        let (ok, teks) = jalankan(&["add", "serde"], &a);
        assert!(!ok, "`jrgm add` tanpa --path mengaku sukses:\n{}", teks);
        assert_eq!(sebelum, std::fs::read(&man).unwrap(),
            "`jrgm add` tanpa --path tetap mengubah jrgm.toml");

        // `--version`/`--git` bersama `--path` harus ditolak, bukan diabaikan diam-diam.
        for f in [&["--version", "1.0"][..], &["--git", "u"][..]] {
            let mut ar = vec!["add", "bantu", "--path", "../bantu"];
            ar.extend_from_slice(f);
            let (ok, teks) = jalankan(&ar, &a);
            assert!(!ok && teks.contains(f[0]),
                "`{}` diabaikan diam-diam:\n{}", f[0], teks);
            assert_eq!(sebelum, std::fs::read(&man).unwrap(),
                "`{}` yang ditolak tetap mengubah jrgm.toml", f[0]);
        }

        let (ok, teks) = jalankan(&["add", "hantu", "--path", "../tidak_ada"], &a);
        assert!(!ok, "`--path` ke tempat kosong diterima:\n{}", teks);
        assert_eq!(sebelum, std::fs::read(&man).unwrap(),
            "`--path` yang ditolak tetap mengubah jrgm.toml");

        let (ok, teks) = jalankan(&["add", "bantu", "--path", "../bantu"], &a);
        assert!(ok, "`jrgm add --path` gagal:\n{}", teks);
        let (ok, teks) = jalankan(&["run"], &a);
        assert!(ok && teks.contains("7"), "sesudah add, modulnya masih tidak ketemu:\n{}", teks);

        // Nama yang sama dua kali harus ditolak, bukan ditulis dua baris.
        let (ok, teks) = jalankan(&["add", "bantu", "--path", "../bantu"], &a);
        assert!(!ok && teks.contains("sudah ada"), "add ganda diterima:\n{}", teks);

        let (ok, teks) = jalankan(&["remove", "bantu"], &a);
        assert!(ok, "`jrgm remove` gagal:\n{}", teks);
        assert_eq!(sebelum, std::fs::read(&man).unwrap(),
            "sesudah add lalu remove, jrgm.toml tidak kembali seperti semula");
        let (ok, _) = jalankan(&["run"], &a);
        assert!(!ok, "sesudah remove, modulnya masih ketemu");

        // `--dev` (Fase JRGM-4): masuk `[dev-dependencies]`, yang tidak dilihat `jrgm run`.
        let (ok, teks) = jalankan(&["add", "bantu", "--path", "../bantu", "--dev"], &a);
        assert!(ok && teks.contains("(dev)"), "`--dev` gagal:\n{}", teks);
        assert!(std::fs::read_to_string(&man).unwrap().contains("[dev-dependencies]\nbantu = "));
        let (ok, _) = jalankan(&["run"], &a);
        assert!(!ok, "dependensi dev ikut terlihat oleh `jrgm run`");
        assert!(jalankan(&["remove", "bantu"], &a).0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `jrgm build` harus memeriksa SEMUA berkas proyek, bukan entry point saja.
    #[test]
    fn build_memeriksa_seluruh_proyek_bukan_entry_saja() {
        let d = ruang("build");
        paket(&d.join("bantu"), "bantu", &[],
              &[("m.jrg", "package m\npub fn f() { return 1 }\n")]);
        paket(&d.join("a"), "a", &[("bantu", "../bantu")],
              &[("main.jrg", "package main\npub fn main() { wok(1) }\n"),
                ("rusak.jrg", "package rusak\npub fn f( {\n")]);
        let a = d.join("a");

        let (ok, teks) = jalankan(&["build"], &a);
        assert!(!ok, "berkas saudara yang tidak parse dilaporkan sehat:\n{}", teks);
        assert!(teks.contains("rusak.jrg"), "build gagal tanpa menyebut berkasnya:\n{}", teks);

        // Dua berkas rusak harus dilaporkan dua-duanya: berhenti di yang
        // pertama berarti satu perjalanan bolak-balik per berkas.
        std::fs::write(a.join("src/rusak2.jrg"), "package r2\npub fn g( {\n").unwrap();
        let (_, teks) = jalankan(&["build"], &a);
        assert!(teks.contains("rusak.jrg") && teks.contains("rusak2.jrg"),
            "build berhenti di berkas rusak yang pertama:\n{}", teks);
        std::fs::remove_file(a.join("src/rusak.jrg")).unwrap();
        std::fs::remove_file(a.join("src/rusak2.jrg")).unwrap();

        // Dan berkas milik DEPENDENSI-nya juga.
        std::fs::write(d.join("bantu/src/m.jrg"), "package m\npub fn f( {\n").unwrap();
        let (ok, teks) = jalankan(&["build"], &a);
        assert!(!ok && teks.contains("m.jrg"),
            "berkas dependensi yang tidak parse dilaporkan sehat:\n{}", teks);

        std::fs::write(d.join("bantu/src/m.jrg"), "package m\npub fn f() { return 1 }\n").unwrap();
        let (ok, teks) = jalankan(&["build"], &a);
        assert!(ok, "proyek sehat dilaporkan rusak:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// `jrgm test` — penemuan, isolasi, dan laporan.
mod uji {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn jrgm() -> &'static str { env!("CARGO_BIN_EXE_jrgm") }
    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    fn ruang(nama: &str) -> PathBuf {
        let d = crate::bersama::dir_unik(&format!("jrg-uji-{}", nama));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("jrgm.toml"),
            "[package]\nname = \"uji\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.jrg\"\n").unwrap();
        std::fs::write(d.join("src/main.jrg"),
            "package main\npub fn main() { wok(1) }\n").unwrap();
        d
    }

    fn tulis_uji(d: &Path, nama: &str, isi: &str) {
        std::fs::write(d.join("tests").join(nama), isi).unwrap();
    }

    fn jalankan(bin: &str, args: &[&str], dir: &Path) -> (Option<i32>, String) {
        let o = Command::new(bin).args(args).current_dir(dir).output().unwrap();
        (o.status.code(),
         String::from_utf8_lossy(&o.stdout).to_string()
             + &String::from_utf8_lossy(&o.stderr))
    }

    /// **Ini tes yang paling penting di modul ini.**
    #[test]
    fn kegagalan_keras_tidak_menyembunyikan_test_sesudahnya() {
        let d = ruang("keras");
        tulis_uji(&d, "a.jrg", "package a\n\
            pub fn test_sebelum() { assert_eq(1, 1) }\n\
            pub fn test_meledak() { let x = [1]; wok(x[9]) }\n\
            pub fn test_sesudah() { assert_eq(2, 2) }\n");

        for arg in [&[][..], &["--vm"][..]] {
            let mut a = vec!["--uji", "tests/a.jrg"]; a.extend_from_slice(arg);
            let (kode, teks) = jalankan(jrgc(), &a, &d);
            assert_eq!(kode, Some(1), "berkas dengan test gagal harus keluar 1:\n{}", teks);
            assert!(teks.contains("ok    test_sebelum"), "test pertama hilang:\n{}", teks);
            assert!(teks.contains("GAGAL test_meledak"), "kegagalan tidak dilaporkan:\n{}", teks);
            assert!(teks.contains("ok    test_sesudah"),
                "test SESUDAH kegagalan keras tidak jalan — kegagalannya tidak ditangkap:\n{}",
                teks);
            assert!(teks.contains("3 test: 2 lulus, 1 gagal"),
                "rangkumannya salah atau hilang:\n{}", teks);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Kegagalan menyebut BARIS-nya, dan `assert_eq` menyebut tipenya.
    #[test]
    fn kegagalan_menyebut_baris_dan_tipe() {
        let d = ruang("baris");
        let isi = "package a\n\
            \n\
            pub fn test_lulus() {\n    assert_eq(1, 1)\n}\n\
            \n\
            pub fn test_beda_tipe() {\n    assert_eq(4, \"4\")\n}\n";
        tulis_uji(&d, "a.jrg", isi);
        // Nomornya diturunkan dari sumbernya, bukan dihitung dengan tangan:
        // menghitung baris di dalam sebuah string Rust ber-escape adalah cara
        // yang bagus untuk menuduh implementasi yang benar.
        let n = isi.lines().position(|l| l.contains("assert_eq(4,")).unwrap() + 1;

        for arg in [&[][..], &["--vm"][..]] {
            let mut a = vec!["--uji", "tests/a.jrg"]; a.extend_from_slice(arg);
            let (_, teks) = jalankan(jrgc(), &a, &d);
            assert!(teks.contains(&format!("(baris {})", n)),
                "kegagalan tidak menyebut barisnya:\n{}", teks);
            assert!(teks.contains("(int)") && teks.contains("(string)"),
                "dua nilai yang tercetak sama tidak dibedakan tipenya:\n{}", teks);
            assert!(!teks.contains("ASSERT"),
                "pesan assert lama masih dipakai:\n{}", teks);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Interpreter dan VM harus melaporkan hal yang sama, byte per byte.
    #[test]
    fn laporan_dua_backend_identik() {
        let d = ruang("paritas");
        tulis_uji(&d, "a.jrg", "package a\n\
            import \"std/testing\"\n\
            pub fn test_lulus() { assert_eq(1, 1) }\n\
            pub fn uji_gagal()  { assert_eq(1, 2) }\n\
            pub fn test_bukan_fungsi_pun_jalan() { testing::cek(\"x\", true) }\n");
        let (k1, t1) = jalankan(jrgc(), &["--uji", "tests/a.jrg"], &d);
        let (k2, t2) = jalankan(jrgc(), &["--uji", "tests/a.jrg", "--vm"], &d);
        assert_eq!(k1, k2, "kode keluar beda:\n--- interp:\n{}\n--- vm:\n{}", t1, t2);
        assert_eq!(t1, t2, "laporan mode uji beda antar backend");
        assert!(t1.contains("GAGAL uji_gagal"), "kasusnya tidak menguji apa-apa:\n{}", t1);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Berkas uji harus bisa `import` modul dari `src/` — itulah gunanya.
    #[test]
    fn berkas_uji_bisa_mengimpor_modul_proyek() {
        let d = ruang("impor");
        std::fs::write(d.join("src/bantu.jrg"),
            "package bantu\npub fn tambah(a, b) { return a + b }\n").unwrap();
        tulis_uji(&d, "a.jrg", "package a\nimport \"bantu\"\n\
            pub fn test_tambah() { assert_eq(bantu::tambah(2, 3), 5) }\n");

        // Langsung tanpa -I: modulnya memang tidak ketemu.
        let (kode, _) = jalankan(jrgc(), &["--uji", "tests/a.jrg"], &d);
        assert_ne!(kode, Some(0), "modul `src/` ketemu tanpa -I?");

        for arg in [&[][..], &["--vm"][..]] {
            let mut a = vec!["test"]; a.extend_from_slice(arg);
            let (kode, teks) = jalankan(jrgm(), &a, &d);
            assert_eq!(kode, Some(0), "`jrgm test {}` gagal:\n{}", arg.join(" "), teks);
            assert!(teks.contains("ok    test_tambah"), "testnya tidak jalan:\n{}", teks);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `std::exit` di dalam sebuah test membunuh prosesnya, dan sebuah test yang
    /// memblokir menggantungnya selamanya.
    #[test]
    fn exit_dan_hang_ditahan_batas_proses() {
        let d = ruang("batas");
        tulis_uji(&d, "a_exit.jrg", "package a\n\
            pub fn test_keluar() { std::exit(3) }\n");
        tulis_uji(&d, "b_hang.jrg", "package b\n\
            pub fn test_gantung() { let c = make_chan(int); wok(<-c) }\n");
        tulis_uji(&d, "c_sehat.jrg", "package c\n\
            pub fn test_sehat() { assert_eq(1, 1) }\n");

        let mulai = std::time::Instant::now();
        let (kode, teks) = jalankan(jrgm(), &["test", "--timeout", "3"], &d);
        let lama = mulai.elapsed();

        assert_eq!(kode, Some(1), "gagalnya tidak sampai ke exit code:\n{}", teks);
        assert!(teks.contains("std::exit"),
            "proses yang mati mendadak tidak dijelaskan:\n{}", teks);
        assert!(teks.contains("DIBUNUH") && teks.contains("timeout"),
            "test yang menggantung tidak dilaporkan:\n{}", teks);
        // Yang paling penting: berkas KETIGA tetap dijalankan.
        assert!(teks.contains("ok    test_sehat"),
            "berkas sesudah yang mati/menggantung tidak dijalankan:\n{}", teks);
        // Dan batasnya benar-benar berlaku — tanpa timeout ini tidak selesai.
        assert!(lama.as_secs() < 30, "timeout tidak berlaku, makan {:?}", lama);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Nol test adalah KEGAGALAN, bukan sukses yang sunyi.
    #[test]
    fn nol_test_menolak_bukan_mengaku_sukses() {
        // (a) tidak ada direktori tests/ sama sekali
        let d = ruang("kosong");
        std::fs::remove_dir_all(d.join("tests")).unwrap();
        let (kode, teks) = jalankan(jrgm(), &["test"], &d);
        assert_eq!(kode, Some(1), "proyek tanpa tests/ dilaporkan sukses:\n{}", teks);
        assert!(teks.contains("tests/"), "penolakannya tidak menyebut konvensinya:\n{}", teks);

        // (b) ada tests/, tapi tidak ada fungsi ber-awalan test_/uji_
        std::fs::create_dir_all(d.join("tests")).unwrap();
        tulis_uji(&d, "a.jrg", "package a\npub fn bukan_test() { wok(1) }\n");
        let (kode, teks) = jalankan(jrgm(), &["test"], &d);
        assert_eq!(kode, Some(1), "berkas tanpa fungsi test dilaporkan sukses:\n{}", teks);
        assert!(teks.contains("test_") && teks.contains("uji_"),
            "penolakannya tidak menyebut kedua awalan:\n{}", teks);

        // (c) filter yang tidak cocok apa pun
        tulis_uji(&d, "b.jrg", "package b\npub fn test_ada() { assert_eq(1, 1) }\n");
        let (kode, teks) = jalankan(jrgm(), &["test", "--filter", "hantu"], &d);
        assert_eq!(kode, Some(1), "filter tanpa hasil dilaporkan sukses:\n{}", teks);
        assert!(teks.contains("hantu"), "penolakannya tidak menyebut filternya:\n{}", teks);

        // Dan yang cocok tetap jalan — kalau semuanya ditolak, tes ini pun hijau.
        let (kode, teks) = jalankan(jrgm(), &["test", "--filter", "ada"], &d);
        assert_eq!(kode, Some(0), "filter yang cocok malah gagal:\n{}", teks);
        assert!(teks.contains("ok    test_ada"), "testnya tidak jalan:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Proyek baru harus bisa `jrgm test` sejak menit pertama, dan `jrgm
    /// build` harus ikut memeriksa berkas ujinya.
    #[test]
    fn proyek_baru_langsung_punya_test_yang_lulus() {
        let d = crate::bersama::dir_unik("jrg-uji-new");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (kode, teks) = jalankan(jrgm(), &["new", "segar"], &d);
        assert_eq!(kode, Some(0), "jrgm new gagal:\n{}", teks);
        let p = d.join("segar");

        let (kode, teks) = jalankan(jrgm(), &["test"], &p);
        assert_eq!(kode, Some(0), "proyek baru gagal `jrgm test`:\n{}", teks);

        // `jrgm build` menghitung berkas ujinya juga: sebuah berkas uji yang
        // tidak parse adalah masalah nyata, dan "N berkas diperiksa" dengan N
        // yang lebih kecil dari jumlah berkasnya adalah laporan yang kurang.
        std::fs::write(p.join("tests/rusak.jrg"), "package r\npub fn test_x( {\n").unwrap();
        let (kode, teks) = jalankan(jrgm(), &["build"], &p);
        assert_eq!(kode, Some(1), "berkas uji yang tidak parse lolos `jrgm build`:\n{}", teks);
        assert!(teks.contains("rusak.jrg"), "build gagal tanpa menyebut berkasnya:\n{}", teks);
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// REPL — keadaan yang menetap, galat yang tidak mematikan sesi, dan
/// kesetaraan dengan berkas.
mod repl {
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    /// Jalankan REPL dengan masukan `input`, kembalikan (stdout, stderr).
    fn sesi(input: &str) -> (String, String) {
        let mut anak = Command::new(jrgc())
            .arg("--repl")
            .env("JRG_SOPAN", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("gagal menjalankan jrgc --repl");
        anak.stdin.as_mut().unwrap().write_all(input.as_bytes()).unwrap();
        let out = anak.wait_with_output().unwrap();
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    }

    /// `import` di REPL mencari di direktori kerjanya — dulu daftar tempatnya KOSONG, jadi
    /// modul tepat di sebelahnya dijawab "tidak ketemu. Yang sudah dicari: []" — dan galat
    /// saat jalan dicetak tanpa awalan ganda (`error: runtime error: …`).
    #[test]
    fn import_dari_direktori_kerja_dan_galat_tanpa_awalan_ganda() {
        let dir = crate::bersama::dir_unik("jrg-repl-impor");
        std::fs::create_dir_all(&dir).expect("bikin dir");
        std::fs::write(dir.join("bantu.jrg"), "package bantu\npub fn dua() { return 2 }\n").expect("tulis");
        let mut anak = Command::new(jrgc()).arg("--repl").env("JRG_SOPAN", "1").current_dir(&dir)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().expect("jalankan REPL");
        anak.stdin.as_mut().unwrap().write_all(b"import \"bantu\"\nbantu::dua() + 1\n[1][5]\n").unwrap();
        let out = anak.wait_with_output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(o.lines().any(|b| b.trim() == "3"), "import di REPL gagal:\n{}\n{}", o, e);
        assert!(e.contains("error"), "galat indeks tidak dilaporkan:\n{}", e);
        assert!(!e.contains("runtime error:"), "awalan galat ganda:\n{}", e);
    }

    /// Tipe yang diketik di REPL tidak menimpa tipe bernama sama milik modul yang di-`import`
    /// lebih dulu (Fase REPL-RUANG): dulu `a::buat()` lalu gagal "tidak punya field `x`".
    #[test]
    fn tipe_repl_tidak_menimpa_tipe_modul() {
        let dir = crate::bersama::dir_unik("jrg-repl-ruang");
        std::fs::create_dir_all(&dir).expect("bikin dir");
        std::fs::write(dir.join("a.jrg"), "pub struct Titik { pub x: int }\npub fn buat() { return Titik { x: 1 } }\n").expect("tulis");
        let mut anak = Command::new(jrgc()).arg("--repl").env("JRG_SOPAN", "1").current_dir(&dir)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().expect("jalankan REPL");
        anak.stdin.as_mut().unwrap().write_all(
            b"import \"a\"\nstruct Titik { y: int }\nwok(Titik { y: 2 })\nwok(a::buat())\nwok(a::Titik { x: 9 })\n").unwrap();
        let out = anak.wait_with_output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        for harap in ["Titik {y: 2}", "Titik {x: 1}", "Titik {x: 9}"] {
            assert!(o.lines().any(|b| b.trim() == harap), "REPL tidak mencetak {:?}:\n{}\n{}", harap, o, e);
        }
    }

    #[test]
    fn tanpa_argumen_masuk_repl() {
        // Bentuk TELANJANG, dan ia yang justru rusak.
        let mut anak = Command::new(jrgc())
            .env("JRG_SOPAN", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("gagal menjalankan jrgc tanpa argumen");
        anak.stdin.as_mut().unwrap().write_all(b"1 + 1\n").unwrap();
        let out = anak.wait_with_output().unwrap();
        let o = String::from_utf8_lossy(&out.stdout).to_string();
        let e = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(!e.contains("required arguments"),
            "`jrgc` telanjang masih menjawab pesan clap: {:?}", e);
        assert_eq!(o.trim(), "2",
            "`jrgc` telanjang harusnya masuk REPL dan menjawab 2 — stdout={:?} stderr={:?}", o, e);
    }

    #[test]
    fn keadaan_menetap_antar_masukan() {
        // Inti sebuah REPL. Kalau ini rusak, semua yang lain tidak ada
        // gunanya — tiap baris akan berdiri sendiri dan tidak ada yang bisa
        // dibangun sepotong-sepotong.
        let (o, _) = sesi("let x = 10\nx * 3\nlet y = x + 5\ny\n");
        assert_eq!(o.trim(), "30\n15", "keluaran: {:?}", o);
    }

    #[test]
    fn ekspresi_dicetak_pernyataan_tidak() {
        // `wok` sudah mencetak sendiri; kalau REPL mencetak nilai baliknya
        // juga, tiap `wok` muncul dua kali — sekali isinya, sekali `()`.
        let (o, _) = sesi("wok(\"halo\")\nlet a = 1\n1 + 1\n");
        assert_eq!(o.trim(), "halo\n2", "keluaran: {:?}", o);
    }

    #[test]
    fn galat_tidak_mengakhiri_sesi() {
        // Sebuah REPL yang mati pada galat pertama lebih buruk daripada tidak
        // ada REPL: orang mencoba sesuatu JUSTRU karena belum yakin.
        let (o, e) = sesi("nama_yang_tidak_ada\n1 + 1\n\"masih hidup\"\n");
        assert!(e.contains("error"), "harusnya melapor galat, stderr: {:?}", e);
        assert_eq!(o.trim(), "2\nmasih hidup", "keluaran: {:?}", o);
    }

    #[test]
    fn blok_banyak_baris_dikumpulkan() {
        let (o, _) = sesi("fn faktorial(n) {\n  if n <= 1 { return 1 }\n  return n * faktorial(n - 1)\n}\nfaktorial(10)\n");
        assert_eq!(o.trim(), "3628800", "keluaran: {:?}", o);
    }

    #[test]
    fn impl_di_masukan_terpisah_dari_struct() {
        // Cara paling wajar mengetiknya, dan ia DITOLAK sampai REPL menyusun
        // ulang "berkas"-nya: pemeriksaan nama tipe cuma melihat berkas yang
        // sama, jadi `impl P` sendirian tidak menemukan `P`.
        let (o, e) = sesi(
            "struct P { pub x: int, pub y: int }\n\
             impl P { fn total(self) -> int { return self.x + self.y } }\n\
             P{x:3, y:4}.total()\n");
        assert!(!e.contains("error"), "harusnya tanpa galat, stderr: {:?}", e);
        assert_eq!(o.trim(), "7", "keluaran: {:?}", o);
    }

    #[test]
    fn nama_boleh_didefinisikan_ulang() {
        // Perbedaan yang disengaja dengan berkas, dan ia diuji supaya tidak
        // hilang tanpa sengaja: mengetik ulang sebuah baris adalah kelakuan
        // paling wajar di REPL.
        let (o, _) = sesi("fn f() { return 1 }\nf()\nfn f() { return 2 }\nf()\n");
        assert_eq!(o.trim(), "1\n2", "keluaran: {:?}", o);
    }

    #[test]
    fn simpan_menghasilkan_program_setara() {
        let dir = std::env::temp_dir().join(format!("jrg_repl_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let berkas = dir.join("sesi.jrg");
        // EKSPRESI TELANJANG ikut diuji, dan itu bukan tambahan kosmetik.
        let masukan = format!(
            "struct P {{ pub x: int, pub y: int }}\n\
             impl P {{ fn total(self) -> int {{ return self.x + self.y }} }}\n\
             fn f() {{ return 1 }}\n\
             fn f() {{ return 99 }}\n\
             let p = P{{x:3, y:4}}\n\
             p.total()\n\
             f()\n\
             1 + 2\n\
             wok(\"lewat wok\")\n\
             :simpan {}\n",
            berkas.display());
        let (o_sesi, e) = sesi(&masukan);
        assert!(!e.contains("error"), "sesi harusnya bersih, stderr: {:?}", e);
        assert!(berkas.is_file(), "berkasnya tidak ditulis: {}", o_sesi);

        // Keluaran sesi minus baris konfirmasi `:simpan`.
        let dari_sesi: String = o_sesi
            .lines()
            .filter(|l| !l.starts_with("ditulis ke"))
            .collect::<Vec<_>>()
            .join("\n");

        for vm in [false, true] {
            let mut c = Command::new(jrgc());
            c.arg(&berkas).env("JRG_SOPAN", "1");
            if vm { c.arg("--vm"); }
            let out = c.output().unwrap();
            assert!(out.status.success(),
                "berkas hasil :simpan gagal jalan (vm={}): {}",
                vm, String::from_utf8_lossy(&out.stderr));
            let dari_berkas = String::from_utf8_lossy(&out.stdout).to_string();
            assert_eq!(dari_sesi.trim(), dari_berkas.trim(),
                "sesi dan berkas hasil :simpan menjawab BEDA (vm={})", vm);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn simpan_menolak_sesi_yang_tidak_sah_dengan_sebab_yang_benar() {
        // Versi pertama `:simpan` menaruh SEMUA masukan di dalam `main`, jadi sesi
        // apa pun yang memuat `struct` mustahil disimpan — lalu menolak sambil …
        let dir = std::env::temp_dir().join(format!("jrg_repl2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let berkas = dir.join("x.jrg");
        // `break` di luar loop: ditolak parser, dan tidak ada hubungannya
        // dengan definisi ulang.
        let (o, _) = sesi(&format!("wok(1)\nbreak\n:simpan {}\n", berkas.display()));
        assert!(!o.contains("didefinisikan ulang"),
            "penolakannya tidak boleh menebak sebab: {:?}", o);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Mode skrip: berkas `.jrg` tanpa `package`.
mod skrip {
    use std::process::Command;

    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir()
            .join(format!("jrg_skrip_{}_{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Jalankan sebuah skrip di kedua backend; kembalikan (stdout, stderr, rc).
    fn jalan(isi: &str, vm: bool) -> (String, String, i32) {
        let d = dir();
        let f = d.join("s.jrg");
        std::fs::write(&f, isi).unwrap();
        let mut c = Command::new(jrgc());
        c.arg(&f).env("JRG_SOPAN", "1");
        if vm { c.arg("--vm"); }
        let o = c.output().unwrap();
        let _ = std::fs::remove_dir_all(&d);
        (
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            o.status.code().unwrap_or(-1),
        )
    }

    #[test]
    fn satu_baris_jalan_di_kedua_backend() {
        for vm in [false, true] {
            let (o, e, rc) = jalan("wok(\"halo\")\n", vm);
            assert_eq!(rc, 0, "rc={} stderr={}", rc, e);
            assert_eq!(o.trim(), "halo", "vm={} keluaran={:?}", vm, o);
        }
    }

    #[test]
    fn deklarasi_tetap_ter_hoist() {
        // `fn` di tingkat atas boleh dipakai SEBELUM barisnya — sifat yang
        // sudah dimiliki JRG, dan tidak ada alasan melepasnya di skrip.
        let src = "wok(f())\nfn f() { return 7 }\n";
        for vm in [false, true] {
            let (o, e, rc) = jalan(src, vm);
            assert_eq!(rc, 0, "stderr={}", e);
            assert_eq!(o.trim(), "7", "vm={}", vm);
        }
    }

    #[test]
    fn let_dibaca_berurutan_bukan_ter_hoist() {
        // Kebalikan dari `fn`, dan itu disengaja: di berkas ber-`package` sebuah
        // `let` tingkat atas ter-hoist, jadi `wok(x)` sebelum `let x = 5` akan …
        let (_, e, rc) = jalan("wok(x)\nlet x = 5\n", false);
        assert_ne!(rc, 0, "harusnya ditolak");
        assert!(e.contains("x"), "pesannya harus menyebut `x`: {:?}", e);
    }

    #[test]
    fn struct_dan_impl_jalan_tanpa_package() {
        let src = "struct P { pub x: int, pub y: int }\n\
                   impl P { fn total(self) -> int { return self.x + self.y } }\n\
                   wok(P{x:3, y:4}.total())\n";
        for vm in [false, true] {
            let (o, e, rc) = jalan(src, vm);
            assert_eq!(rc, 0, "stderr={}", e);
            assert_eq!(o.trim(), "7", "vm={}", vm);
        }
    }

    #[test]
    fn main_sendiri_tanpa_package_tetap_jalan() {
        // Tidak ada pernyataan telanjang, jadi tidak ada `main` yang
        // disintesis — `main` yang ditulis sendiri yang dipakai.
        let (o, e, rc) = jalan("fn main() { wok(\"dari main\") }\n", false);
        assert_eq!(rc, 0, "stderr={}", e);
        assert_eq!(o.trim(), "dari main");
    }

    #[test]
    fn main_plus_pernyataan_telanjang_ditolak() {
        // Dua badan untuk satu program. Memilih salah satu diam-diam berarti
        // separuh berkasnya tidak pernah jalan dan tidak ada yang
        // mengatakannya.
        let (_, e, rc) = jalan("fn main() { wok(1) }\nwok(2)\n", false);
        assert_ne!(rc, 0, "harusnya ditolak");
        assert!(e.contains("main"), "pesannya harus menyebut `main`: {:?}", e);
    }

    #[test]
    fn salah_ketik_package_tetap_dituduh_package() {
        // Tanpa penjagaan ini, `pakage main` masuk mode skrip dan gagal jauh
        // belakangan dengan pesan yang tidak menyebut `package` sama sekali.
        let (_, e, rc) = jalan("pakage main\nfn main() { wok(1) }\n", false);
        assert_ne!(rc, 0, "harusnya ditolak");
        // Assertion ini SENGAJA menuntut kalimat persisnya, bukan sekadar kata
        // "package" muncul di suatu tempat.
        assert!(e.contains("harusnya ada `package`"),
            "pesannya harus menuduh `package` yang hilang, bukan gejala lain: {:?}", e);
    }

    #[test]
    fn berkas_ber_package_tidak_berubah_arti() {
        // Setengah lainnya, dan ia yang paling penting: menambah mode skrip
        // tidak boleh menyentuh satu pun berkas yang sudah ada.
        let src = "package main\nvar G = 9\nfn main() { wok(G) }\n";
        for vm in [false, true] {
            let (o, e, rc) = jalan(src, vm);
            assert_eq!(rc, 0, "stderr={}", e);
            assert_eq!(o.trim(), "9", "vm={}", vm);
        }
    }

    #[test]
    fn import_std_jalan_di_skrip() {
        let src = "import \"std/option\"\nwok(1 + 1)\n";
        let (o, e, rc) = jalan(src, false);
        assert_eq!(rc, 0, "stderr={}", e);
        assert_eq!(o.trim(), "2");
    }
}

/// Penyuntingan baris dan editor kode, lewat PTY SUNGGUHAN.
mod sunting {
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }
    fn akar() -> &'static str { env!("CARGO_MANIFEST_DIR") }

    fn ada_python() -> bool {
        Command::new("python3").arg("-c").arg("import pty")
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false)
    }

    /// Jalankan `prog args…` di dalam PTY, kirim `skrip`, kembalikan layarnya.
    fn pty(args: &[&str], skrip: &str) -> String {
        let mut c = Command::new("python3");
        c.arg(format!("{}/perkakas/pty_sesi.py", akar()));
        for a in args { c.arg(a); }
        let mut anak = c.env("JRG_SOPAN", "1")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().expect("gagal menjalankan pty_sesi.py");
        anak.stdin.as_mut().unwrap().write_all(skrip.as_bytes()).unwrap();
        let out = anak.wait_with_output().unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Buang urutan escape supaya yang dibandingkan teksnya, bukan warnanya.
    fn polos(s: &str) -> String {
        let b: Vec<char> = s.chars().collect();
        let (mut i, mut k) = (0usize, String::new());
        while i < b.len() {
            if b[i] == '\u{1b}' {
                i += 1;
                if i < b.len() && (b[i] == '[' || b[i] == 'O') { i += 1; }
                while i < b.len() && !b[i].is_ascii_alphabetic() { i += 1; }
                i += 1;
            } else { k.push(b[i]); i += 1; }
        }
        k
    }

    const ENTER: &str = "0d";
    const ATAS: &str = "1b5b41";
    const BAWAH: &str = "1b5b42";
    const KIRI: &str = "1b5b44";
    const HOME: &str = "1b5b48";
    const END: &str = "1b5b46";

    fn teks(s: &str) -> String {
        s.bytes().map(|b| format!("{:02x}", b)).collect()
    }

    // ---------------------------------------------------------------- REPL

    // Yang ditunggu KEADAAN, bukan waktu: `tunggu 1200` cukup di mesin senggang
    // dan belum tentu cukup saat suite penuh jalan, dan tes yang kadang merah …
    fn repl(langkah: &[&str]) -> String {
        let mut s = String::from("kolom 80\nbaris 24\nsampai 15000 jrg> \n");
        for l in langkah {
            s.push_str(&format!("kirim {}\ndiam 200 8000\n", l));
        }
        s.push_str("kirim 04\ndiam 200 8000\n");
        polos(&pty(&[jrgc(), "--repl", "--sopan"], &s))
    }

    #[test]
    fn repl_riwayat_naik_mengembalikan_baris_sebelumnya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // Ketik satu ekspresi, lalu panah atas + Enter. Kalau panahnya tidak
        // dibaca, yang masuk adalah tiga byte escape mentah dan jawabannya galat
        // parse — bukan `7` untuk kedua kalinya.
        let o = repl(&[&teks("wok(3 + 4)"), ENTER, ATAS, ENTER]);
        assert_eq!(o.matches("\n7").count(), 2,
            "panah atas tidak mengulang baris sebelumnya:\n{}", o);
    }

    #[test]
    fn repl_panah_kiri_menyisip_di_tengah_baris() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // Ketik `wok(12)`, mundur dua, sisipkan `0` -> `wok(102)`.
        let o = repl(&[&teks("wok(12)"), KIRI, KIRI, &teks("0"), ENTER]);
        assert!(o.contains("\n102"), "sisipan di tengah baris tidak sampai:\n{}", o);
    }

    #[test]
    fn repl_home_dan_ctrl_u_bekerja() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // Home lalu ketik: hasilnya harus TERSISIP DI DEPAN.
        let o = repl(&[&teks("8)"), HOME, &teks("wok("), END, ENTER]);
        assert!(o.contains("\n8"), "Home tidak memindahkan kursor ke kolom nol:\n{}", o);
        // Ctrl-U mengosongkan baris; sesudahnya baris baru harus utuh.
        let u = repl(&[&teks("wok(999"), "15", &teks("wok(5)"), ENTER]);
        assert!(u.contains("\n5"), "Ctrl-U tidak mengosongkan baris:\n{}", u);
        assert!(!u.contains("error"), "baris tergabung jadi galat:\n{}", u);
    }

    #[test]
    fn repl_ctrl_c_membatalkan_baris_bukan_sesinya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // Tanpa `-isig` Ctrl-C jadi SINYAL dan sesinya mati; yang benar ia
        // sampai sebagai byte, membatalkan baris, dan sesinya lanjut.
        let o = repl(&[&teks("wok(1"), "03", &teks("wok(6)"), ENTER]);
        assert!(o.contains("\n6"), "sesi tidak selamat dari Ctrl-C:\n{}", o);
    }

    // -------------------------------------------------------------- EDITOR

    const CONTOH: &str = "package main\n// contoh untuk disunting\npub fn main() {\n    let n = 42\n    wok(\"halo\", n)\n}\n";

    fn siapkan(nama: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("jrg_sunting_{}_{}.jrg", std::process::id(), nama));
        std::fs::write(&p, CONTOH).unwrap();
        p
    }

    fn editor(berkas: &std::path::Path, langkah: &[&str]) -> String {
        // Penandanya nama berkasnya sendiri di baris status — muncul hanya
        // sesudah editornya benar-benar menggambar layar pertamanya.
        let tanda = berkas.file_name().unwrap().to_string_lossy().to_string();
        let mut s = format!("kolom 80\nbaris 24\nsampai 25000 {}\n", tanda);
        for l in langkah {
            s.push_str(&format!("kirim {}\ndiam 250 10000\n", l));
        }
        s.push_str("diam 250 10000\n");
        let ed = format!("{}/editor/jrgedit.jrg", akar());
        pty(&[jrgc(), &ed, "--sopan", "--", &berkas.display().to_string()], &s)
    }

    #[test]
    fn kerangka_baru_tidak_memuat_baris_yang_inert() {
        // Berkas pertama yang dilihat pemula tidak boleh memuat baris yang
        // tidak berarti: ia akan menirunya, dan tidak punya cara tahu mana
        // yang berarti.
        let jrgm = env!("CARGO_BIN_EXE_jrgm");
        let dir = std::env::temp_dir().join(format!("jrg_rangka_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let o = Command::new(jrgm).arg("new").arg("contoh")
            .current_dir(&dir).output().expect("jrgm new");
        assert!(o.status.success(), "jrgm new gagal: {:?}", o);

        let main = std::fs::read_to_string(dir.join("contoh/src/main.jrg")).unwrap();
        let toml = std::fs::read_to_string(dir.join("contoh/jrgm.toml")).unwrap();

        // `fmt` bukan modul `std/`, dan `fmt::wok` jalan tanpanya.
        assert!(!main.contains("import \"std/fmt\""),
            "kerangka masih memuat import yang tidak melakukan apa-apa:\n{}", main);
        // Tidak ada registry, jadi `std = {{ version = ... }}` tidak pernah
        // di-resolve apa pun — `jrgm tree` sendiri melaporkannya kosong.
        assert!(!toml.contains("std = {"),
            "manifest masih memuat dependensi yang tidak pernah di-resolve:\n{}", toml);

        // Indentasinya harus benar.
        for baris in main.lines() {
            let spasi = baris.len() - baris.trim_start().len();
            assert!(spasi % 4 == 0 && spasi <= 8,
                "indentasi kerangka tidak wajar ({} spasi):\n{}", spasi, main);
        }
        // Dan ia harus benar-benar jalan, bukan cuma terlihat rapi.
        let o = Command::new(jrgc()).arg(dir.join("contoh/src/main.jrg"))
            .env("JRG_SOPAN", "1").output().expect("jalankan kerangka");
        assert!(o.status.success(), "kerangka tidak jalan: {}",
            String::from_utf8_lossy(&o.stderr));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bantuan_muat_satu_layar_dan_menyebut_yang_nyata() {
        // Bantuan yang panjang berhenti dibaca. 139 baris membuat "bagaimana
        // saya menjalankan berkas ini" tenggelam di antara lima belas tombol
        // optimisasi LLVM.
        let h = String::from_utf8_lossy(
            &Command::new(jrgc()).arg("-h").output().unwrap().stdout).to_string();
        let n = h.lines().count();
        assert!(n <= 60, "`jrgc -h` {} baris — terlalu panjang untuk satu layar", n);
        // Yang paling dibutuhkan harus ADA di layar pertama.
        for w in ["--vm", "--emit-llvm", "--repl", "--sunting", "--uji"] {
            assert!(h.contains(w), "`jrgc -h` tidak menyebut {}:\n{}", w, h);
        }
        // Dan yang jarang dipakai TIDAK boleh ada di sana — kalau semuanya
        // muat, tidak ada yang disembunyikan dan panjangnya kembali.
        for w in ["--pgo-generate", "--no-gc-sections", "--emit-bc"] {
            assert!(!h.contains(w), "`jrgc -h` masih memuat {} — mestinya cuma di --help", w);
        }
        // `--help` tetap lengkap: pendek bukan berarti menyembunyikan.
        let l = String::from_utf8_lossy(
            &Command::new(jrgc()).arg("--help").output().unwrap().stdout).to_string();
        for w in ["--pgo-generate", "--no-gc-sections", "--emit-bc", "--lto"] {
            assert!(l.contains(w), "`jrgc --help` kehilangan {}", w);
        }
    }

    #[test]
    fn bantuan_jrgm_tidak_menyebut_perintah_yang_bekerja_sebagai_kosong() {
        // Teks lamanya mendaftar `test`, `fmt`, `add`, `remove`, `tree` di bawah
        // "BELUM ADA ISINYA".
        let jrgm = env!("CARGO_BIN_EXE_jrgm");
        let h = String::from_utf8_lossy(
            &Command::new(jrgm).arg("-h").output().unwrap().stdout).to_string();
        assert!(!h.contains("BELUM ADA ISINYA"),
            "`jrgm -h` masih memuat daftar 'belum ada isinya' yang basi:\n{}", h);
        let n = h.lines().count();
        assert!(n <= 60, "`jrgm -h` {} baris — terlalu panjang", n);

        // Di luar proyek ketiganya tetap menolak, dengan kode 1.
        for p in ["lint", "publish", "update"] {
            let o = Command::new(jrgm).arg(p)
                .current_dir(std::env::temp_dir())
                .output().unwrap();
            assert!(!o.status.success(),
                "`jrgm {}` keluar 0 — sebuah perintah yang berhasil sambil \
                 tidak mengerjakan apa-apa adalah kebohongan yang paling awet", p);
        }
    }

    #[test]
    fn sunting_tertanam_di_biner_jadi_jalan_dari_direktori_mana_pun() {
        // Dijalankan dari direktori yang BUKAN akar repo, dengan stdin bukan
        // terminal.
        let out = Command::new(jrgc())
            .arg("--sunting").arg("apa_saja.jrg")
            .current_dir(std::env::temp_dir())
            .env("JRG_SOPAN", "1")
            .output().expect("gagal menjalankan jrgc --sunting");
        let t = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(t.contains("butuh terminal sungguhan"),
            "editor tertanam tidak jalan dari direktori lain:\n{}", t);
        assert!(out.status.success(), "keluar tidak nol: {:?}", out.status);
    }

    #[test]
    fn env_exe_menunjuk_biner_yang_sedang_jalan() {
        // `env::args()[0]` adalah nama SKRIP, jadi tanpa `env::exe` sebuah
        // program JRG tidak punya cara menjalankan juru bahasanya sendiri —
        // yang persis dibutuhkan Ctrl-R di editor.
        let p = std::env::temp_dir().join(format!("jrg_exe_{}.jrg", std::process::id()));
        std::fs::write(&p, "package main\npub fn main() { wok(env::exe()) }\n").unwrap();
        for vm in [false, true] {
            let mut c = Command::new(jrgc());
            c.arg(&p).env("JRG_SOPAN", "1");
            if vm { c.arg("--vm"); }
            let o = c.output().expect("gagal menjalankan");
            let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
            assert!(t.ends_with("jrgc"), "env::exe (vm={}) menjawab `{}`", vm, t);
            assert!(std::path::Path::new(&t).exists(),
                "env::exe (vm={}) menunjuk berkas yang tidak ada: {}", vm, t);
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn editor_mengetik_lalu_menyimpan_dan_indentasinya_terbawa() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("ketik");
        // Turun ke badan `main`, ke akhir baris, Enter, ketik, Ctrl-S, Ctrl-Q.
        sunting_berkas(&p, &[BAWAH, BAWAH, BAWAH, END, ENTER, &teks("wok(\"baru\")"), "13", "11"]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(isi.contains("wok(\"baru\")"), "Ctrl-S tidak menyimpan:\n{}", isi);
        // Indentasi baris sebelumnya harus ikut — sebuah editor yang menaruh
        // baris baru di kolom nol memaksa penulisnya membetulkan tiap baris.
        assert!(isi.contains("    wok(\"baru\")"), "indentasi tidak terbawa:\n{}", isi);
    }

    #[test]
    fn editor_hapus_baris_lalu_urungkan_mengembalikannya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("kill");
        sunting_berkas(&p, &[BAWAH, "0b", "13", "11"]);          // Ctrl-K, Ctrl-S, Ctrl-Q
        let isi = std::fs::read_to_string(&p).unwrap();
        assert!(!isi.contains("// contoh"), "Ctrl-K tidak menghapus baris:\n{}", isi);
        assert!(isi.contains("pub fn main"), "Ctrl-K menghapus lebih dari satu baris:\n{}", isi);

        std::fs::write(&p, CONTOH).unwrap();
        sunting_berkas(&p, &[BAWAH, "0b", "1a", "13", "11"]);    // + Ctrl-Z sebelum simpan
        let balik = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(balik.contains("// contoh"), "Ctrl-Z tidak mengembalikan baris:\n{}", balik);
    }

    #[test]
    fn editor_cari_melapor_ketemu_dan_tidak_ketemu() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("cari");
        let ada = polos(&sunting_berkas(&p, &["06", &teks("halo"), ENTER, "11"]));
        assert!(ada.contains("ketemu di baris"), "cari yang ADA tidak melapor:\n{}", ada);
        let nihil = polos(&sunting_berkas(&p, &["06", &teks("zzzz"), ENTER, "11"]));
        let _ = std::fs::remove_file(&p);
        // Arah negatifnya perlu: sebuah pencarian yang selalu bilang "ketemu"
        // lulus separuh pertama dengan sempurna.
        assert!(nihil.contains("tidak ketemu"), "cari yang TIDAK ada tidak melapor:\n{}", nihil);
    }

    #[test]
    fn editor_menolak_keluar_saat_ada_perubahan_belum_tersimpan() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("jaga");
        let o = polos(&sunting_berkas(&p, &[&teks("X"), "11"]));   // ketik, lalu Ctrl-Q
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(o.contains("BELUM TERSIMPAN"), "keluar tanpa peringatan:\n{}", o);
        assert_eq!(isi, CONTOH, "berkasnya berubah padahal tidak disimpan");
    }

    #[test]
    fn editor_gandakan_potong_dan_tempel_baris() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("papan");
        // Ctrl-D di baris komentar -> dua komentar.
        sunting_berkas(&p, &[BAWAH, "04", "13", "11"]);
        let isi = std::fs::read_to_string(&p).unwrap();
        assert_eq!(isi.matches("// contoh").count(), 2,
            "Ctrl-D tidak menggandakan baris:\n{}", isi);

        // Ctrl-X memotong, Ctrl-V menempel kembali -> jumlahnya utuh lagi.
        std::fs::write(&p, CONTOH).unwrap();
        sunting_berkas(&p, &[BAWAH, "18", "16", "13", "11"]);
        let balik = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(balik.matches("// contoh").count(), 1,
            "potong+tempel tidak mengembalikan barisnya:\n{}", balik);
    }

    #[test]
    fn editor_alih_komentar_bolak_balik() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("komen");
        // Ctrl-T di baris `    let n = 42` -> `    // let n = 42`.
        sunting_berkas(&p, &[BAWAH, BAWAH, BAWAH, "14", "13", "11"]);
        let isi = std::fs::read_to_string(&p).unwrap();
        assert!(isi.contains("    // let n = 42"),
            "komentar tidak dipasang sesudah indentasi:\n{}", isi);

        // Dua kali harus kembali seperti semula — sebuah alih yang cuma bisa
        // satu arah adalah "beri komentar", bukan "alih".
        std::fs::write(&p, CONTOH).unwrap();
        sunting_berkas(&p, &[BAWAH, BAWAH, BAWAH, "14", "14", "13", "11"]);
        let balik = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(balik, CONTOH, "alih komentar dua kali tidak kembali semula");
    }

    #[test]
    fn editor_ganti_semua_melapor_jumlahnya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("ganti");
        // Ctrl-P: `n` -> `m`. Muncul di beberapa tempat, jadi angkanya penting.
        // Sesi ganti bertanya per kemunculan, seperti nano: `s` mengganti semuanya.
        let o = polos(&sunting_berkas(&p, &["10", &teks("halo"), ENTER, &teks("hai"), ENTER, "73", "13", "11"]));
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(isi.contains("\"hai\""), "ganti tidak sampai ke berkas:\n{}", isi);
        assert!(o.contains("1 diganti"), "jumlah yang kena tidak dilaporkan:\n{}", o);
    }

    #[test]
    fn editor_kurung_menutup_sendiri_dan_penutup_dilewati() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("kurung");
        // Di akhir baris `}` tekan Enter lalu ketik `f(` -> `f()`, sekali saja.
        // Lalu ketik `)` — yang sudah ada di kanan — dan ia harus DILEWATI,
        // bukan digandakan.
        sunting_berkas(&p, &["1b5b46", ENTER, &teks("f("), &teks(")"), "13", "11"]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(isi.contains("f()"), "kurung tidak menutup sendiri:\n{}", isi);
        assert!(!isi.contains("f())"), "penutup digandakan:\n{}", isi);
    }

    #[test]
    fn editor_kurung_tutup_merapatkan_indentasinya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("indent");
        // Di akhir ` let n = 42`, Enter membawa indentasi empat spasi; mengetik `}`
        // harus merapatkannya ke kolom nol, bukan menaruhnya di kolom empat.
        sunting_berkas(&p, &[BAWAH, BAWAH, BAWAH, END, ENTER, &teks("}"), "13", "11"]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(isi.contains("\n}\n    wok"), "`}}` tidak merapat:\n{}", isi);
    }

    #[test]
    fn editor_esc_menampilkan_daftar_tombol() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("bantu");
        let o = sunting_berkas(&p, &["1b"]);
        // Daftarnya lebih panjang dari satu layar: PgDn membuka halaman berikutnya.
        let lanjut = sunting_berkas(&p, &["1b", "1b5b367e"]);
        let _ = std::fs::remove_file(&p);
        let l = layar(&o, 80, 24).join("\n");
        assert!(l.contains("daftar tombol"), "Esc tidak membuka bantuan:\n{}", l);
        assert!(l.contains("^S") && l.contains("^O") && l.contains("^V"),
            "halaman pertama bantuan tidak menyebut tombol yang ada:\n{}", l);
        let l2 = layar(&lanjut, 80, 24).join("\n");
        assert!(l2.contains("^T") && l2.contains("komentar"),
            "PgDn tidak membuka halaman berikutnya bantuan:\n{}", l2);
    }

    #[test]
    fn editor_menyorot_sintaks_dan_melaporkan_posisi() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("sorot");
        let mentah = sunting_berkas(&p, &["11"]);
        let _ = std::fs::remove_file(&p);
        // Penyorotan berarti jenis token yang berbeda berwarna berbeda: SGR tepat di depan
        // kata kunci `package` tidak sama dengan SGR di depan komentar `//`.
        let sgr_di_depan = |kata: &str| mentah.match_indices(kata).find_map(|(i, _)| {
            let j = mentah[..i].rfind('\u{1b}')?;
            let sgr = &mentah[j..i];
            (sgr.ends_with('m') && sgr.contains("38;5;")).then(|| sgr.to_string())
        });
        let (kunci, komentar) = (sgr_di_depan("package"), sgr_di_depan("// contoh"));
        assert!(kunci.is_some() && komentar.is_some() && kunci != komentar,
            "kata kunci dan komentar tidak dibedakan warnanya — penyorotan tidak jalan: {:?} {:?}",
            kunci, komentar);
        let t = polos(&mentah);
        assert!(t.contains("jrg_sunting_"), "status bar tidak menyebut nama berkas:\n{}", t);
        assert!(t.contains("1:1"), "status bar tidak menyebut posisi kursor:\n{}", t);
    }

    // ------------------------------------------------- EDITOR (Fase SUNTING-2)

    /// Layar AKHIR dari byte keluaran PTY, baris per baris, ujung kanan dipangkas. Cukup
    /// untuk program layar penuh: CSI H/f/G/d/A–D, K/J, layar alternatif, CR/LF/BS/TAB, dan
    /// huruf lebar dua kolom. Warna dibuang — yang diperiksa letak teksnya.
    fn layar(keluaran: &str, kol: usize, bar: usize) -> Vec<String> {
        let kosong = || vec![vec![' '; kol]; bar];
        let mut buf = [kosong(), kosong()];
        let mut a = 0usize;
        let (mut r, mut c) = (0usize, 0usize);
        let mut simpan = (0usize, 0usize);
        let s: Vec<char> = keluaran.chars().collect();
        let mut i = 0;
        while i < s.len() {
            let ch = s[i];
            if ch == '\u{1b}' {
                if s.get(i + 1) == Some(&'[') {
                    let mut j = i + 2;
                    let mut arg = String::new();
                    while j < s.len() && (s[j].is_ascii_digit() || s[j] == ';' || s[j] == '?') {
                        arg.push(s[j]);
                        j += 1;
                    }
                    let perintah = s.get(j).copied().unwrap_or('m');
                    let angka: Vec<usize> = arg.trim_start_matches('?').split(';')
                        .map(|x| x.parse().unwrap_or(0)).collect();
                    let a0 = angka.first().copied().unwrap_or(0);
                    let satu = |v: usize| v.max(1);
                    match perintah {
                        'H' | 'f' => {
                            r = satu(a0).min(bar) - 1;
                            c = satu(angka.get(1).copied().unwrap_or(0)).min(kol) - 1;
                        }
                        'K' => {
                            let ujung = c.min(kol);
                            match a0 {
                                0 => for x in ujung..kol { buf[a][r][x] = ' ' },
                                1 => for x in 0..(ujung + 1).min(kol) { buf[a][r][x] = ' ' },
                                _ => buf[a][r] = vec![' '; kol],
                            }
                        }
                        'J' => {
                            if a0 >= 2 {
                                buf[a] = kosong();
                            } else if a0 == 0 {
                                for x in c.min(kol)..kol { buf[a][r][x] = ' ' }
                                for y in r + 1..bar { buf[a][y] = vec![' '; kol] }
                            }
                        }
                        'A' => r = r.saturating_sub(satu(a0)),
                        'B' => r = (r + satu(a0)).min(bar - 1),
                        'C' => c = (c + satu(a0)).min(kol - 1),
                        'D' => c = c.saturating_sub(satu(a0)),
                        'G' => c = (satu(a0) - 1).min(kol - 1),
                        'd' => r = (satu(a0) - 1).min(bar - 1),
                        'h' | 'l' if arg.starts_with('?')
                            && angka.iter().any(|n| matches!(n, 47 | 1047 | 1049)) =>
                        {
                            if perintah == 'h' {
                                simpan = (r, c);
                                a = 1;
                                buf[1] = kosong();
                                r = 0;
                                c = 0;
                            } else {
                                a = 0;
                                (r, c) = simpan;
                            }
                        }
                        _ => {}
                    }
                    i = j + 1;
                    continue;
                }
                if s.get(i + 1) == Some(&']') {
                    // OSC: sampai BEL atau ESC \.
                    let mut j = i + 2;
                    while j < s.len() && s[j] != '\u{7}' && !(s[j] == '\u{1b}' && s.get(j + 1) == Some(&'\\')) {
                        j += 1;
                    }
                    i = if j < s.len() && s[j] == '\u{1b}' { j + 2 } else { j + 1 };
                    continue;
                }
                i += 2;
                continue;
            }
            match ch {
                '\r' => c = 0,
                '\n' => {
                    r += 1;
                    if r >= bar { buf[a].remove(0); buf[a].push(vec![' '; kol]); r = bar - 1; }
                }
                '\u{8}' => c = c.saturating_sub(1),
                '\t' => c = ((c / 8 + 1) * 8).min(kol - 1),
                _ if ch >= ' ' => {
                    let w = jrgc::lebar::karakter(ch);
                    if w > 0 {
                        // Bungkus TERTUNDA, seperti terminal sungguhan: baru terjadi saat huruf
                        // berikutnya tidak muat lagi.
                        if c + w > kol {
                            c = 0;
                            r += 1;
                            if r >= bar { buf[a].remove(0); buf[a].push(vec![' '; kol]); r = bar - 1; }
                        }
                        buf[a][r][c] = ch;
                        if w == 2 && c + 1 < kol { buf[a][r][c + 1] = '\0'; }
                        c += w;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        buf[a].iter().map(|b| b.iter().filter(|&&x| x != '\0').collect::<String>().trim_end().to_string())
            .collect()
    }

    #[test]
    fn emulator_layar_membaca_posisi_hapus_dan_layar_alternatif() {
        // Penguji yang tidak teruji tidak membuktikan apa pun: tiap urutan yang dipakai tes
        // editor di bawah diperiksa di sini dulu.
        let l = layar("abc\r\nxy\x1b[1;5Hz\x1b[2;2H\x1b[K\x1b[?1049h\x1b[3;1Hdi alt", 10, 4);
        assert_eq!(l, vec!["", "", "di alt", ""]);
        let l = layar("abc\r\nxy\x1b[1;5Hz\x1b[2;2H\x1b[K\x1b[?1049hdi alt\x1b[?1049l", 10, 4);
        assert_eq!(l, vec!["abc z", "x", "", ""]);
        let l = layar("日本x\r\n123456789AB", 10, 3);
        assert_eq!(l, vec!["日本x", "123456789A", "B"]);
    }

    /// Editor lewat `jrgc --sunting` — sumber TERTANAM, di VM — dengan ukuran layar yang
    /// dipilih. Langkah berawalan `!` diteruskan apa adanya sebagai arahan `pty_sesi.py`.
    fn sunting(berkas: &str, tanda: &str, kol: usize, bar: usize, langkah: &[&str]) -> String {
        let mut s = format!("kolom {}\nbaris {}\nsampai 25000 {}\ndiam 250 10000\n", kol, bar, tanda);
        for l in langkah {
            match l.strip_prefix('!') {
                Some(arahan) => s.push_str(&format!("{}\n", arahan)),
                None => s.push_str(&format!("kirim {}\ndiam 250 10000\n", l)),
            }
        }
        s.push_str("diam 250 10000\n");
        pty(&[jrgc(), "--sunting", berkas, "--sopan"], &s)
    }

    fn sunting_berkas(p: &std::path::Path, langkah: &[&str]) -> String {
        let tanda = p.file_name().unwrap().to_string_lossy().to_string();
        sunting(&p.display().to_string(), &tanda, 80, 24, langkah)
    }

    const SIMPAN: &str = "13";
    const KELUAR: &str = "11";

    #[test]
    fn editor_tertanam_di_layar_alternatif_dan_memulihkannya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("alt");
        let o = sunting_berkas(&p, &[KELUAR]);
        let _ = std::fs::remove_file(&p);
        // Layar alternatif: seperti nano/vim, gulungan terminal pemakai tidak ditimpa, dan
        // layar lamanya kembali begitu editornya keluar.
        let masuk = o.find("\x1b[?1049h").expect("editor tidak masuk layar alternatif");
        let keluar = o.rfind("\x1b[?1049l").expect("editor tidak meninggalkan layar alternatif");
        assert!(keluar > masuk, "layar alternatif ditinggalkan sebelum dimasuki");
        let akhir = layar(&o, 80, 24);
        assert!(!akhir.iter().any(|b| b.contains("jrgedit")),
            "sesudah keluar, layar editor masih terpampang:\n{}", akhir.join("\n"));
    }

    #[test]
    fn editor_menggambar_judul_nomor_baris_dan_pintasan() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("tata");
        let o = sunting_berkas(&p, &[]);
        let _ = std::fs::remove_file(&p);
        let l = layar(&o, 80, 24);
        let nama = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(l[0].contains("jrgedit") && l[0].contains(&nama) && l[0].contains("1:1"),
            "baris judul tidak menyebut editor, berkas, dan posisi:\n{}", l.join("\n"));
        // Baris pertama berkas tepat di bawah judul, didahului nomornya.
        assert!(l[1].trim_start().starts_with('1') && l[1].contains("package main"),
            "baris teks pertama tidak di bawah judul:\n{}", l.join("\n"));
        assert!(l[23].contains("^S") || l[22].contains("^S"),
            "baris pintasan tidak menyebut ^S:\n{}", l.join("\n"));
        // Warna nomor baris: dulu 238 (nyaris hitam di latar gelap), sekarang terbaca.
        assert!(o.contains("38;5;246") && !o.contains("38;5;238"),
            "nomor baris memakai warna yang nyaris tak terlihat lagi");
    }

    #[test]
    fn editor_di_terminal_sempit_tidak_melipat_baris_bawah() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("sempit");
        let tanda = p.file_name().unwrap().to_string_lossy().to_string();
        let o = sunting(&p.display().to_string(), &tanda, 40, 12, &[]);
        let _ = std::fs::remove_file(&p);
        let l = layar(&o, 40, 12);
        // Dulu baris bantuan melipat di layar sempit dan seluruh layar bergeser satu baris:
        // baris 1 hilang dan kursor tampil di baris yang salah.
        assert!(l[1].contains("package main"),
            "baris pertama bergeser di layar 40 kolom:\n{}", l.join("\n"));
        assert!(l[11].contains("^S"), "pintasan tidak di baris terakhir:\n{}", l.join("\n"));
    }

    #[test]
    fn editor_menggambar_ulang_saat_jendela_berubah_ukuran() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("ukur");
        let tanda = p.file_name().unwrap().to_string_lossy().to_string();
        let o = sunting(&p.display().to_string(), &tanda, 60, 20, &["!ukuran 100 30", "!diam 600 8000"]);
        let _ = std::fs::remove_file(&p);
        let l = layar(&o, 100, 30);
        // Posisi di ujung kanan judul pindah ke kolom yang hanya ada di jendela baru.
        let kolom = l[0].find("6 baris").unwrap_or(0);
        assert!(kolom > 60, "judul tidak digambar ulang selebar jendela baru:\n{}", l.join("\n"));
        assert!(l[29].contains("^S") || l[28].contains("^S"),
            "pintasan tidak pindah ke dasar jendela baru:\n{}", l.join("\n"));
    }

    #[test]
    fn editor_tempelan_berkurung_masuk_apa_adanya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("tempel");
        // Di akhir `    let n = 42`: Enter (indentasi terbawa), lalu tempelan tiga baris.
        let tempelan = teks("\x1b[200~if x {\n    wok(1)\n}\x1b[201~");
        sunting_berkas(&p, &[BAWAH, BAWAH, BAWAH, END, ENTER, &tempelan, SIMPAN, KELUAR]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        // Dulu tiap baris baru di dalam tempelan memicu indentasi otomatis, dan menumpuk.
        assert!(isi.contains("    let n = 42\n    if x {\n    wok(1)\n}\n    wok(\"halo\", n)"),
            "tempelan tidak masuk apa adanya:\n{}", isi);
    }

    #[test]
    fn editor_membuka_direktori_sebagai_daftar_berkas() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let dir = crate::bersama::dir_unik("jrg-sunting-dir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("alfa.jrg"), "package main\n").unwrap();
        std::fs::write(dir.join("beta.jrg"), "package main\n").unwrap();
        let o = sunting(&dir.display().to_string(), "alfa.jrg", 80, 24, &[]);
        let _ = std::fs::remove_dir_all(&dir);
        let l = layar(&o, 80, 24).join("\n");
        // Dulu `--sunting <direktori>` mati dengan galat internal `fs::read`.
        assert!(l.contains("alfa.jrg") && l.contains("beta.jrg"), "isi direktori tidak didaftar:\n{}", l);
        assert!(!polos(&o).contains("error"), "membuka direktori berakhir galat:\n{}", polos(&o));
    }

    #[test]
    fn editor_berkas_baca_saja_ditandai_dan_editornya_tidak_mati() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        use std::os::unix::fs::PermissionsExt;
        let p = siapkan("bacasaja");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o444)).unwrap();
        if std::fs::OpenOptions::new().write(true).open(&p).is_ok() {
            eprintln!("LEWAT: proses ini bisa menulis berkas 0444 (root?)");
            let _ = std::fs::remove_file(&p);
            return;
        }
        let o = sunting_berkas(&p, &[&teks("X"), SIMPAN, KELUAR]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        let t = polos(&o);
        assert!(t.contains("[baca saja]"), "judul tidak menandai berkas baca saja:\n{}", t);
        assert!(t.contains("cuma bisa dibaca"), "ketikan ke berkas baca saja tidak dijelaskan:\n{}", t);
        assert_eq!(isi, CONTOH, "berkas baca saja berubah");
        // Dulu `fs::write` yang gagal MEMATIKAN editor dan meninggalkan terminal RAW.
        assert!(o.rfind("\x1b[?1049l").is_some(), "editor tidak keluar dengan rapi:\n{}", t);
    }

    #[test]
    fn editor_crlf_ditulis_kembali_apa_adanya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = std::env::temp_dir().join(format!("jrg_sunting_{}_crlf.jrg", std::process::id()));
        std::fs::write(&p, "satu\r\ndua\r\n").unwrap();
        sunting_berkas(&p, &[END, &teks("X"), SIMPAN, KELUAR]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        // Dulu `\r` tertinggal di ujung baris, jadi End berhenti satu huruf terlalu jauh.
        assert_eq!(isi, "satuX\r\ndua\r\n");
    }

    #[test]
    fn editor_urung_lalu_ulangi() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("ulang");
        // Ctrl-Z lalu Ctrl-Y: ketikannya kembali. Dulu tidak ada ulang sama sekali.
        sunting_berkas(&p, &[BAWAH, &teks("X"), "1a", "19", SIMPAN, KELUAR]);
        let isi = std::fs::read_to_string(&p).unwrap();
        assert!(isi.contains("\nX// contoh"), "Ctrl-Y tidak mengulangi:\n{}", isi);
        std::fs::write(&p, CONTOH).unwrap();
        sunting_berkas(&p, &[BAWAH, &teks("X"), "1a", SIMPAN, KELUAR]);
        let urung = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(urung, CONTOH, "Ctrl-Z tidak mengurungkan ketikan");
    }

    #[test]
    fn editor_seleksi_shift_salin_dan_tempel() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = siapkan("pilih");
        let shift_kanan = "1b5b313b3243";
        // Baris 2 `// contoh…`: pilih dua huruf, salin, tempel di ujung baris.
        sunting_berkas(&p, &[BAWAH, HOME, shift_kanan, shift_kanan, "03", END, "16", SIMPAN, KELUAR]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(isi.contains("// contoh untuk disunting//\n"), "seleksi tidak tersalin:\n{}", isi);
    }

    #[test]
    fn editor_jalankan_lalu_lompat_ke_galatnya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = std::env::temp_dir().join(format!("jrg_sunting_{}_jalan.jrg", std::process::id()));
        std::fs::write(&p, "package main\npub fn main() {\n    wok(\"mulai\")\n    let a = [1, 2]\n    wok(a[7])\n}\n").unwrap();
        // Ctrl-R menjalankan di terminal biasa; Enter kembali ke editor.
        let o = sunting_berkas(&p, &["12", "!sampai 15000 Enter untuk kembali", ENTER]);
        let _ = std::fs::remove_file(&p);
        let t = polos(&o);
        assert!(t.contains("mulai"), "keluaran programnya tidak tampil:\n{}", t);
        let l = layar(&o, 80, 24);
        // Galatnya di baris 5 kolom 9: kursor dipindah ke sana dan pesannya tampil.
        assert!(l[0].contains("5:9"), "kursor tidak lompat ke galat:\n{}", l.join("\n"));
        assert!(l.iter().any(|b| b.contains("di luar batas")), "pesan galat tidak tampil:\n{}", l.join("\n"));
    }

    #[test]
    fn editor_periksa_tidak_menjalankan_programnya() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let dir = crate::bersama::dir_unik("jrg-sunting-periksa");
        std::fs::create_dir_all(&dir).unwrap();
        let jejak = dir.join("jejak.txt");
        let p = dir.join("periksa.jrg");
        std::fs::write(&p, format!("package main\npub fn main() {{\n    fs::write(\"{}\", \"jalan\")\n}}\n",
            jejak.display())).unwrap();
        let o = sunting_berkas(&p, &["1b70"]);
        let ada = jejak.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(polos(&o).contains("tidak ada galat sintaks"), "Alt-P tidak melapor:\n{}", polos(&o));
        assert!(!ada, "Alt-P menjalankan programnya");
    }

    #[test]
    fn editor_membuka_banyak_berkas_sekaligus() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let a = siapkan("banyak_a");
        let b = siapkan("banyak_b");
        let tanda = a.file_name().unwrap().to_string_lossy().to_string();
        let skrip = format!("kolom 80\nbaris 24\nsampai 25000 {}\ndiam 250 10000\nkirim 1b5b313b3343\ndiam 250 10000\n", tanda);
        // `jrgc --sunting a.jrg b.jrg`: berkas kedua tiba sebagai sumber, lalu Alt-Kanan pindah.
        let o = pty(&[jrgc(), "--sunting", &a.display().to_string(), &b.display().to_string(), "--sopan"], &skrip);
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        let l = layar(&o, 80, 24);
        let nama_b = b.file_name().unwrap().to_string_lossy().to_string();
        assert!(l[0].contains("[2/2]") && l[0].contains(&nama_b),
            "berkas kedua tidak terbuka atau Alt-Kanan tidak pindah:\n{}", l.join("\n"));
    }

    #[test]
    fn editor_jalan_juga_di_mesin_acuan() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // `jrgc editor/jrgedit.jrg -- berkas`: sumber dari disk, interpreter — dua mesin satu editor.
        let p = siapkan("acuan");
        editor(&p, &[BAWAH, END, &teks(" ok"), SIMPAN, KELUAR]);
        let isi = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(isi.contains("// contoh untuk disunting ok\n"), "editor di interpreter tidak menyunting:\n{}", isi);
    }

    #[test]
    fn uji_editor_lulus_di_dua_mesin() {
        // Aturan editor — penyangga, sorot, cari, layar, perintah — adalah fungsi murni,
        // jadi diuji tanpa terminal lewat `--uji`, di dua mesin.
        let akar_ed = std::path::Path::new(akar()).join("editor");
        let mut berkas: Vec<std::path::PathBuf> = std::fs::read_dir(akar_ed.join("uji")).unwrap()
            .filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jrg"))
            .collect();
        berkas.sort();
        assert!(berkas.len() >= 10, "berkas uji editor tinggal {}", berkas.len());
        let mut jumlah = 0usize;
        for f in &berkas {
            for vm in [false, true] {
                let mut c = Command::new(jrgc());
                c.arg(f).arg("--uji").arg("-I").arg(&akar_ed).arg("--sopan");
                if vm { c.arg("--vm"); }
                let o = c.env("JRG_SOPAN", "1").stdin(Stdio::null()).output().unwrap();
                let t = String::from_utf8_lossy(&o.stdout).to_string();
                assert!(o.status.success() && t.contains(" 0 gagal"),
                    "{} (vm={}) gagal:\n{}{}", f.display(), vm, t, String::from_utf8_lossy(&o.stderr));
                if !vm {
                    // `--- 7 test: 7 lulus, 0 gagal ---`
                    let n = t.lines().filter_map(|b| b.strip_prefix("--- "))
                        .filter_map(|b| b.split(' ').next()?.parse::<usize>().ok()).next().unwrap_or(0);
                    jumlah += n;
                }
            }
        }
        assert!(jumlah >= 78, "tes editor tinggal {} (sebelumnya 78)", jumlah);
    }

    /// Nama yang tidak dideklarasikan dilaporkan `--periksa` — semuanya, dengan barisnya, di
    /// berkas utama maupun modulnya — dan satu operand yang hilang tetap SATU galat.
    #[test]
    fn periksa_melaporkan_nama_tak_dideklarasikan_dan_satu_galat_per_salah_tulis() {
        let dir = crate::bersama::dir_unik("jrg-periksa-nama");
        std::fs::create_dir_all(&dir).unwrap();
        let jejak = dir.join("jejak.txt");
        let galat = dir.join("galat.tsv");
        std::fs::write(dir.join("modul.jrg"),
            "package modul\npub fn f() {\n    return hilang_di_modul + 1\n}\n").unwrap();
        std::fs::write(dir.join("utama.jrg"), format!(
            "package main\nimport \"modul\"\npub fn main() {{\n    fs::write(\"{}\", \"utama\")\n    \
             let jumlah = 3\n    wok(hantu(1), jumlh)\n    wok(str::uper(\"a\"), modul::f(), |x| x + hantu2)\n}}\n",
            jejak.display())).unwrap();
        let periksa = |berkas: &str| {
            let _ = std::fs::remove_file(&galat);
            let o = Command::new(jrgc()).arg(dir.join(berkas)).arg("--periksa")
                .env("JRG_SOPAN", "1").env("JRG_GALAT_KE", &galat).stdin(Stdio::null()).output().unwrap();
            (o.status.success(), std::fs::read_to_string(&galat).unwrap_or_default())
        };
        let (sukses, g) = periksa("utama.jrg");
        assert!(!sukses, "nama tak dideklarasikan lolos `--periksa`");
        assert!(!jejak.exists(), "--periksa menjalankan `main`");
        let baris: Vec<&str> = g.lines().collect();
        assert_eq!(baris.len(), 5, "jumlah galat:\n{}", g);
        for (berkas, lokasi, isi) in [
            ("modul.jrg", "\t3\t12\t", "`hilang_di_modul` belum dikenal"),
            ("utama.jrg", "\t6\t9\t", "`hantu` belum dikenal"),
            ("utama.jrg", "\t6\t19\t", "maksudnya `jumlah`?"),
            ("utama.jrg", "\t7\t9\t", "`str::uper` tidak ditemukan — maksudnya `str::upper`?"),
            ("utama.jrg", "\t7\t45\t", "`hantu2` belum dikenal"),
        ] {
            assert!(baris.iter().any(|b| b.contains(berkas) && b.contains(lokasi) && b.contains(isi)),
                "tidak ada galat {} {:?} {:?}:\n{}", berkas, lokasi, isi, g);
        }

        std::fs::write(dir.join("satu.jrg"),
            "package main\n\nfn f(a) { a + }\n\nfn g() { let y = }\n\npub fn main() { wok(g()) }\n").unwrap();
        let (sukses, g) = periksa("satu.jrg");
        assert!(!sukses);
        let baris: Vec<&str> = g.lines().collect();
        assert_eq!(baris.len(), 2, "pemulihan parser menambah galat ikutan:\n{}", g);
        assert!(baris[0].contains("\t3\t15\t") && baris[1].contains("\t5\t18\t"), "{}", g);

        // Fase PULIH: galat di dalam kurawal milik pernyataannya (lengan `match`) dan deklarasi
        // tingkat atas yang rusak masing-masing satu galat — dulu tiga dan dua.
        std::fs::write(dir.join("pulih.jrg"),
            "package main\nfn f(a) {\n    match a { 1 => wok(1), 2 => }\n    wok(a)\n}\nfnn g(a) { a }\n\
             fn h() { 1 }\npub fn main() { wok(h()) }\n").unwrap();
        let (sukses, g) = periksa("pulih.jrg");
        assert!(!sukses);
        let baris: Vec<&str> = g.lines().collect();
        assert_eq!(baris.len(), 2, "pemulihan parser menambah galat ikutan:\n{}", g);
        assert!(baris[0].contains("\t3\t33\t") && baris[1].contains("\t6\t1\t"), "{}", g);

        // Fase ARGUMEN-PERIKSA: jumlah argumen fungsi tingkat atas, dengan kalimat saat jalan; yang
        // dibayangi closure atau `fn` bersarang bernama sama tidak dituduh.
        std::fs::write(dir.join("arg.jrg"),
            "package main\nfn f(a, b = 2) { return a + b }\nfn g(a, sisa...) { return a }\n\
             fn sepi() { return f(1, 2, 3) }\npub fn main() {\n    wok(f(1), g(1, 2, 3))\n    \
             let h = |x, y| x + y\n    wok(h(1, 2))\n    fn g2() { fn f(x, y, z) { return x }\n return f(1, 2, 3) }\n    \
             wok(g())\n}\n").unwrap();
        let (sukses, g) = periksa("arg.jrg");
        assert!(!sukses);
        let baris: Vec<&str> = g.lines().collect();
        assert_eq!(baris.len(), 2, "jumlah galat:\n{}", g);
        assert!(baris[0].contains("\t4\t20\tfungsi `f` kebanyakan argumen — dia cuma menerima 2, tapi dikasih 3")
            && baris[1].contains("\t11\t9\tfungsi `g`: parameter `a` (posisi 1) wajib diisi"), "{}", g);

        // Sasaran penugasan yang bukan variabel, juga di fungsi yang tidak pernah dipanggil.
        std::fs::write(dir.join("tugas.jrg"),
            "package main\nfn f() { 1 }\nfn sepi() {\n    jumlah = 5\n    f = 2\n    daftar[0] = 1\n    \
             var lokal = 1\n    lokal += 1\n}\npub fn main() { wok(f()) }\n").unwrap();
        let (sukses, g) = periksa("tugas.jrg");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!sukses);
        let baris: Vec<&str> = g.lines().collect();
        assert_eq!(baris.len(), 3, "jumlah galat:\n{}", g);
        assert!(baris[0].contains("\t4\t5\t`jumlah` belum dideklarasikan")
            && baris[1].contains("\t5\t5\t`f` bukan variabel")
            && baris[2].contains("\t6\t5\t`daftar` belum dideklarasikan"), "{}", g);
    }

    #[test]
    fn periksa_diam_kalau_bersih_dan_tidak_menjalankan_apa_pun() {
        let dir = crate::bersama::dir_unik("jrg-periksa");
        std::fs::create_dir_all(&dir).unwrap();
        let jejak = dir.join("jejak.txt");
        std::fs::write(dir.join("modul.jrg"), format!(
            "package modul\nvar X = tulis()\nfn tulis() {{ fs::write(\"{}\", \"modul\"); return 1 }}\npub fn f() {{ return X }}\n",
            jejak.display())).unwrap();
        std::fs::write(dir.join("utama.jrg"), format!(
            "package main\nimport \"modul\"\npub fn main() {{\n    fs::write(\"{}\", \"utama\")\n    wok(modul::f())\n}}\n",
            jejak.display())).unwrap();
        let periksa = || Command::new(jrgc()).arg(dir.join("utama.jrg")).arg("--periksa")
            .env("JRG_SOPAN", "1").stdin(Stdio::null()).output().unwrap();
        let o = periksa();
        assert!(o.status.success() && o.stdout.is_empty() && o.stderr.is_empty(),
            "--periksa atas program yang benar tidak diam: {:?}", o);
        // Inisialisasi tingkat atas sebuah modul adalah KODE: memeriksa tidak boleh menjalankannya.
        assert!(!jejak.exists(), "--periksa menjalankan inisialisasi modul atau `main`");

        std::fs::write(dir.join("modul.jrg"), "package modul\npub fn f( {\n").unwrap();
        let o = periksa();
        let e = String::from_utf8_lossy(&o.stderr).to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!o.status.success(), "galat parse di modul yang di-import tidak ditolak");
        assert!(e.contains("modul.jrg:2"), "galatnya tidak menunjuk berkas modulnya:\n{}", e);
    }

    #[test]
    fn galat_ke_mencatat_lokasi_galat_di_dua_mesin() {
        let dir = crate::bersama::dir_unik("jrg-galat-ke");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("rusak.jrg");
        let jalan = |vm: bool, ke: &std::path::Path| {
            let mut c = Command::new(jrgc());
            c.arg(&p).env("JRG_SOPAN", "1").env("JRG_GALAT_KE", ke).stdin(Stdio::null());
            if vm { c.arg("--vm"); }
            c.output().unwrap()
        };
        std::fs::write(&p, "package main\npub fn main() {\n    let a = [1]\n    wok(a[3])\n}\n").unwrap();
        let mut catatan = Vec::new();
        for vm in [false, true] {
            let ke = dir.join(format!("galat_{}.txt", vm));
            assert!(!jalan(vm, &ke).status.success());
            let isi = std::fs::read_to_string(&ke).unwrap_or_default();
            let k: Vec<&str> = isi.trim_end().split('\t').collect();
            assert!(k.len() == 4 && k[0].ends_with("rusak.jrg") && k[1] == "4" && k[2] == "9",
                "catatan galat (vm={}) bukan `berkas TAB baris TAB kolom TAB pesan` yang benar: {:?}", vm, isi);
            catatan.push(isi);
        }
        assert_eq!(catatan[0], catatan[1], "dua mesin mencatat galat yang berbeda");
        // Galat parse ikut tercatat; program yang benar tidak menulis apa pun.
        std::fs::write(&p, "package main\npub fn main() {\n    wok(1 +)\n}\n").unwrap();
        let ke = dir.join("parse.txt");
        assert!(!jalan(false, &ke).status.success());
        let isi = std::fs::read_to_string(&ke).unwrap_or_default();
        assert_eq!(isi.split('\t').nth(1), Some("3"), "galat parse tidak tercatat: {:?}", isi);
        // Modul yang tidak ketemu menunjuk baris `import`-nya, di dua mesin.
        std::fs::write(&p, "package main\n\nimport \"nggak_ada_modulnya\"\n\npub fn main() { wok(1) }\n").unwrap();
        for vm in [false, true] {
            let ke = dir.join(format!("impor_{}.txt", vm));
            assert!(!jalan(vm, &ke).status.success());
            let isi = std::fs::read_to_string(&ke).unwrap_or_default();
            let k: Vec<&str> = isi.trim_end().split('\t').collect();
            assert!(k.len() == 4 && k[0].ends_with("rusak.jrg") && k[1] == "3" && k[2] == "1"
                && k[3].contains("tidak ketemu"),
                "modul yang tidak ketemu (vm={}) tidak menunjuk baris import-nya: {:?}", vm, isi);
        }
        std::fs::write(&p, "package main\npub fn main() { wok(1) }\n").unwrap();
        let ke = dir.join("bersih.txt");
        assert!(jalan(true, &ke).status.success());
        let ada = ke.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!ada, "program tanpa galat menulis catatan galat");
    }

    // ------------------------------------------------------------ TERMINAL

    fn program(nama: &str, isi: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("jrg_term_{}_{}.jrg", std::process::id(), nama));
        std::fs::write(&p, isi).unwrap();
        p
    }

    /// Jalankan program JRG di dalam PTY, di mesin acuan atau `--vm`.
    fn jalan_pty(p: &std::path::Path, vm: bool, skrip: &str) -> String {
        let berkas = p.display().to_string();
        let mut args = vec![jrgc(), berkas.as_str(), "--sopan"];
        if vm { args.push("--vm"); }
        pty(&args, skrip)
    }

    const BACA_TOMBOL: &str = "package main
pub fn main() {
    if !term::raw(true) { wok(\"bukan terminal\"); return }
    wok(\"SIAP\")
    var n = 0
    loop {
        let t = term::tombol()
        n = n + 1
        wok(\"#\" + str(n) + \"<\" + t + \">\")
        if t == \"q\" || t == \"eof\" { break }
    }
}
";

    #[test]
    fn term_tombol_menyebut_nama_yang_benar_di_terminal_sungguhan() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = program("tombol", BACA_TOMBOL);
        // (byte yang dikirim terminal, nama yang harus dijawab). Tiga yang pertama dulu
        // terbaca Home, hilang, dan MENGETIKKAN byte NUL ke dalam berkas.
        let tombol: &[(&str, &str)] = &[
            ("1b5b31357e", "f5"),
            ("1b5b327e", "insert"),
            ("00", "ctrl-spasi"),
            ("1b78", "alt-x"),
            ("1b5b313b3252", "shift-f3"),
            ("1b5b313b3543", "ctrl-kanan"),
            ("1b7f", "alt-backspace"),
            ("1b", "esc"),
            ("1b5b3c303b31303b354d", "tetikus:kiri:10:5"),
            ("1b5b3c36343b333b344d", "tetikus:roda-atas:3:4"),
            ("e697a5", "\u{65e5}"),
            ("0d", "enter"),
            // Tempelan berkurung dengan CRLF di dalamnya: SATU baris baru, bukan dua.
            ("1b5b3230307e61620d0a63641b5b3230317e", "tempel:ab\ncd"),
            ("71", "q"),
        ];
        // Tombol berikutnya baru dikirim sesudah yang ini DIJAWAB: yang ditunggu keadaan,
        // bukan waktu — Esc yang disusul terlalu cepat memang sah terbaca sebagai Alt.
        let mut skrip = String::from("kolom 80\nbaris 24\nsampai 20000 SIAP\n");
        for (i, (hex, _)) in tombol.iter().enumerate() {
            skrip.push_str(&format!("kirim {}\nsampai 10000 #{}<\n", hex, i + 1));
        }
        skrip.push_str("akhir 10000\nmode\n");
        for vm in [false, true] {
            let o = jalan_pty(&p, vm, &skrip).replace('\r', "");
            for (i, (hex, nama)) in tombol.iter().enumerate() {
                let harap = format!("#{}<{}>", i + 1, nama);
                assert!(o.contains(&harap),
                    "byte {} harus terbaca `{}` (vm={}):\n{}", hex, nama, vm, o);
            }
            assert!(o.contains("[pty keluar=0]"), "program tidak selesai dengan kode 0 (vm={}):\n{}", vm, o);
            // Programnya TIDAK memanggil `term::raw(false)`: pemulihannya tetap terjadi.
            assert!(o.contains("[pty icanon=1 echo=1 isig=1]"),
                "terminal tertinggal raw sesudah program selesai (vm={}):\n{}", vm, o);
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn terminal_pulih_sesudah_galat_dan_sesudah_exit() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // Raw + layar alternatif + tetikus + tempel + kursor tersembunyi, lalu programnya
        // berakhir TANPA membereskan apa pun: lewat galat, dan lewat `exit(3)`.
        let kepala = "package main
pub fn main() {
    if !term::raw(true) { wok(\"bukan terminal\"); return }
    term::mode(\"layar\", true)
    term::mode(\"tetikus\", true)
    term::mode(\"tempel\", true)
    term::mode(\"kursor\", false)
    term::tulis(\"DI DALAM\")
";
        let galat = program("galat", &format!("{}    let a = [1, 2]\n    wok(a[5])\n}}\n", kepala));
        let keluar = program("keluar", &format!("{}    exit(3)\n}}\n", kepala));
        let skrip = "kolom 80\nbaris 24\nsampai 20000 DI DALAM\nakhir 15000\nmode\n";
        for vm in [false, true] {
            for (p, kode, pesan) in [(&galat, "1", true), (&keluar, "3", false)] {
                let o = jalan_pty(p, vm, skrip);
                let ke = |t: &str| o.find(t).unwrap_or_else(||
                    panic!("`{:?}` tidak pernah ditulis (vm={}, kode {}):\n{:?}", t, vm, kode, o));
                // Yang dinyalakan harus dimatikan, masing-masing sesudah dinyalakan.
                for (nyala, mati) in [("\u{1b}[?1049h", "\u{1b}[?1049l"), ("\u{1b}[?1000h", "\u{1b}[?1000l"),
                                      ("\u{1b}[?2004h", "\u{1b}[?2004l"), ("\u{1b}[?25l", "\u{1b}[?25h")] {
                    assert!(ke(nyala) < ke(mati), "{:?} tidak dimatikan sesudah dinyalakan:\n{:?}", nyala, o);
                }
                if pesan {
                    // Pesan galatnya harus tercetak SESUDAH layar alternatif ditinggalkan — di
                    // dalamnya ia ikut hilang begitu layarnya dipulihkan.
                    let g = o.rfind("error").unwrap_or_else(|| panic!("galatnya tidak tercetak:\n{:?}", o));
                    assert!(ke("\u{1b}[?1049l") < g,
                        "pesan galat tercetak di layar alternatif (vm={}):\n{:?}", vm, o);
                }
                assert!(o.contains(&format!("[pty keluar={}]", kode)),
                    "kode keluar harus {} (vm={}):\n{:?}", kode, vm, o);
                assert!(o.contains("[pty icanon=1 echo=1 isig=1]"),
                    "terminal tertinggal raw (vm={}, kode {}):\n{:?}", vm, kode, o);
            }
        }
        let _ = std::fs::remove_file(&galat);
        let _ = std::fs::remove_file(&keluar);
    }

    #[test]
    fn term_tombol_tunggu_menjawab_none_saat_waktunya_habis() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        let p = program("tunggu", "package main
pub fn main() {
    if !term::raw(true) { wok(\"bukan terminal\"); return }
    wok(\"SIAP\")
    wok(\"A=\" + str(term::tombol_tunggu(300)))
    wok(\"LAGI\")
    wok(\"B=\" + str(term::tombol_tunggu(20000)))
    wok(\"C=\" + str(term::tombol_tunggu(0)))
}
");
        // Tidak ada yang diketik sampai `LAGI` muncul, jadi A harus `None`; `x` datang
        // sesudahnya, jadi B harus tombol itu dan C (tanpa menunggu) `None` lagi.
        let skrip = "kolom 80\nbaris 24\nsampai 20000 SIAP\nsampai 10000 LAGI\nkirim 78\nsampai 10000 C=\nakhir 10000\n";
        for vm in [false, true] {
            let o = jalan_pty(&p, vm, skrip).replace('\r', "");
            assert!(o.contains("A=Option::None\n"), "waktu habis harus menjawab None (vm={}):\n{}", vm, o);
            assert!(o.contains("B=Option::Some(x)\n"), "tombol yang diketik tidak sampai (vm={}):\n{}", vm, o);
            assert!(o.contains("C=Option::None\n"), "batas 0 harus menjawab seketika (vm={}):\n{}", vm, o);
            assert!(o.contains("[pty keluar=0]"), "program tidak selesai (vm={}):\n{}", vm, o);
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn terminal_yang_ditutup_mengakhiri_pembacaan_tombol() {
        if !ada_python() { eprintln!("LEWAT: python3 dengan modul pty tidak ada"); return; }
        // Terminal yang ditutup menjawab "siap dibaca" lalu nol byte, selamanya. Program
        // yang selamat dari SIGHUP-nya harus menerima `eof` — bukan berputar di satu inti.
        let satu = program("putus_tombol", "package main
pub fn main() {
    if !term::raw(true) { wok(\"bukan terminal\"); return }
    wok(\"SIAP\")
    loop {
        if term::tombol() == \"eof\" { break }
    }
    fs::write(env::args()[1], \"eof\")
}
");
        let dua = program("putus_tunggu", "package main
pub fn main() {
    if !term::raw(true) { wok(\"bukan terminal\"); return }
    wok(\"SIAP\")
    var selesai = false
    while !selesai {
        match term::tombol_tunggu(200) {
            Option::Some(t) => { if t == \"eof\" { selesai = true } }
            Option::None => {}
        }
    }
    fs::write(env::args()[1], \"eof\")
}
");
        let tanda = std::env::temp_dir().join(format!("jrg_term_{}_putus.txt", std::process::id()));
        for vm in [false, true] {
            for p in [&satu, &dua] {
                let _ = std::fs::remove_file(&tanda);
                let (berkas, t) = (p.display().to_string(), tanda.display().to_string());
                let mut args = vec!["/bin/sh", "-c", "trap '' HUP; exec \"$0\" \"$@\"", jrgc(), berkas.as_str(), "--sopan"];
                if vm { args.push("--vm"); }
                args.push("--");
                args.push(t.as_str());
                let o = pty(&args, "sampai 20000 SIAP\ntutup 8000\n");
                assert!(o.contains("[pty keluar=0]"),
                    "program tidak berhenti sendiri sesudah terminalnya ditutup ({} vm={}):\n{}", berkas, vm, o);
                assert_eq!(std::fs::read_to_string(&tanda).unwrap_or_default(), "eof",
                    "pembacaan tombol tidak menjawab `eof` ({} vm={})", berkas, vm);
            }
        }
        let _ = std::fs::remove_file(&tanda);
        let _ = std::fs::remove_file(&satu);
        let _ = std::fs::remove_file(&dua);
    }
}

/// Aturan proyek: komentar paling banyak 3 baris, penjelasan panjang di
/// `panduan/`. Tanpa penjaga ini utangnya tumbuh lagi.
#[test]
fn tidak_ada_blok_komentar_lebih_dari_tiga_baris() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("LEWAT: python3 tidak ada, pemeriksa komentar tidak bisa dijalankan");
        return;
    }
    let out = Command::new("python3")
        .arg("perkakas/rapikan_komentar.py")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::null())
        .output().expect("jalankan rapikan_komentar.py");
    assert!(out.status.success(),
        "ada blok komentar lebih dari 3 baris — pangkas, dan pindahkan penjelasannya ke \
         `panduan/` (`python3 perkakas/rapikan_komentar.py --tulis`, lalu TINJAU diff-nya):\n{}",
        String::from_utf8_lossy(&out.stdout));
}

/// **Kelas "mekanisme lengkap, kabelnya tidak ada" dijaga pemindai, bukan
/// kebetulan.**
#[test]
fn tidak_ada_field_yang_dibaca_tanpa_pernah_diisi() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("LEWAT: python3 tidak ada, pemindai kabel tidak bisa dijalankan");
        return;
    }
    let out = Command::new("python3")
        .arg("perkakas/kabel.py").arg("--periksa")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::null())
        .output().expect("jalankan kabel.py");
    assert!(out.status.success(), "pemindai kabel memerah:\n{}{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// Alat `jrgc` yang dipakai penyunting di HP, yang cuma membawa `jrgc` (laporan IDE #11, #14).
mod alat_jrgc {
    use std::process::Command;
    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    #[test]
    fn fmt_menulis_hasil_rapi_ke_stdout_tanpa_mengubah_berkas() {
        let dir = crate::bersama::dir_unik("jrg-fmt-jrgc");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.jrg");
        let asli = "package main\npub   fn   main( ){\nlet    angka=[1,2,3]\n  wok(angka)\n}\n";
        std::fs::write(&f, asli).unwrap();
        let out = Command::new(jrgc()).arg(&f).arg("--fmt").env("JRG_SOPAN", "1").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout),
                   "package main\npub fn main() {\n    let angka = [1, 2, 3]\n    wok(angka)\n}\n");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), asli, "berkasnya ikut diubah");
        std::fs::write(&f, "package main\npub fn main( { \"x\n").unwrap();
        let rusak = Command::new(jrgc()).arg(&f).arg("--fmt").env("JRG_SOPAN", "1").output().unwrap();
        assert_eq!(rusak.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&rusak.stderr).contains("tidak bisa dibaca sebagai JRG"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_json_menulis_nama_utuh_di_dua_mesin() {
        let dir = crate::bersama::dir_unik("jrg-profil-json");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p.jrg");
        std::fs::write(&f, "package main\nfn fungsi_dengan_nama_yang_sangat_panjang(n) { return n * 2 }\n\
            pub fn main() {\n var s = 0\n for i in 0..50 { s = s + fungsi_dengan_nama_yang_sangat_panjang(i) }\n wok(s)\n}\n").unwrap();
        for vm in [false, true] {
            let j = dir.join(format!("p{}.json", vm));
            let mut cmd = Command::new(jrgc());
            cmd.arg(&f).arg("--profile-json").arg(&j).env("JRG_SOPAN", "1");
            if vm { cmd.arg("--vm"); }
            let out = cmd.output().unwrap();
            let teks = String::from_utf8_lossy(&out.stdout);
            assert!(teks.contains("2450") && !teks.contains("PROFILER"), "vm={}: stdout {:?}", vm, teks);
            let isi = std::fs::read_to_string(&j).unwrap();
            assert!(isi.contains(r#""nama":"fungsi_dengan_nama_yang_sangat_panjang","panggilan":50"#), "vm={}: {}", vm, isi);
            assert!(isi.starts_with(r#"{"utas":"#) && isi.contains(r#""fungsi":["#), "vm={}: {}", vm, isi);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ════════════════════════════════════════════════════════════════════════
// `[package] bahasa`: jrgm membungkus proyek bahasa lain (src/jrgm/bahasa.rs).
mod bahasa_lain {
    use std::path::Path;
    use std::process::Command;

    fn jrgm(dir: &Path, arg: &[&str]) -> (bool, i32, String) {
        let o = Command::new(env!("CARGO_BIN_EXE_jrgm")).args(arg).current_dir(dir)
            .env("JRG_TANPA_PUTAR", "1").output().expect("jalankan jrgm");
        (o.status.success(), o.status.code().unwrap_or(-1),
         format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
    }

    fn ada(program: &str) -> bool {
        std::env::var_os("PATH").map_or(false, |p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
    }

    /// Rangka tiap bahasa yang perkakasnya ada: jalan, bangun, dan ujinya lulus apa adanya.
    #[test]
    fn rangka_bahasa_lain_jalan_bangun_dan_uji() {
        let dir = crate::bersama::dir_unik("jrgm-bahasa");
        std::fs::create_dir_all(&dir).unwrap();
        let mut diuji = 0;
        for (b, perkakas) in [("python", "python3"), ("c", "cc"), ("sh", "bash"), ("javascript", "node"), ("lua", "lua")] {
            if !ada(perkakas) { eprintln!("dilewati: {} (tanpa {})", b, perkakas); continue; }
            let (ok, _, k) = jrgm(&dir, &["new", &format!("app_{}", b), "--bahasa", b]);
            assert!(ok, "{}: {}", b, k);
            let p = dir.join(format!("app_{}", b));
            let (ok, _, k) = jrgm(&p, &["run"]);
            assert!(ok && k.contains(&format!("Halo dari app_{}!", b)) && k.contains("1 + 2 = 3"), "{} run: {}", b, k);
            let (ok, _, k) = jrgm(&p, &["build"]);
            assert!(ok, "{} build: {}", b, k);
            let (ok, _, k) = jrgm(&p, &["test"]);
            assert!(ok, "{} test: {}", b, k);
            diuji += 1;
        }
        assert!(diuji >= 2, "terlalu sedikit perkakas di mesin ini untuk menguji apa pun");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Kode keluar dan argumen program diteruskan; uji dan kompilasi yang gagal keluar 1, menyebut sebabnya.
    #[test]
    fn kode_keluar_argumen_dan_kegagalan_diteruskan() {
        if !ada("cc") { eprintln!("dilewati: tanpa cc"); return; }
        let dir = crate::bersama::dir_unik("jrgm-bahasa-c");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(jrgm(&dir, &["new", "app", "--bahasa", "c"]).0);
        let p = dir.join("app");
        std::fs::write(p.join("src/main.c"), "#include <stdio.h>\nint main(int c, char **v) { printf(\"%s|%s\\n\", v[1], v[2]); return 7; }\n").unwrap();
        let (_, kode, k) = jrgm(&p, &["run", "--", "a", "b c"]);
        assert_eq!(kode, 7, "{}", k);
        assert!(k.contains("a|b c"), "{}", k);
        std::fs::write(p.join("tests/uji_hitung.c"), "#include \"hitung.h\"\nint main(void) { return tambah(1, 2) == 4 ? 0 : 1; }\n").unwrap();
        let (ok, _, k) = jrgm(&p, &["test"]);
        assert!(!ok && k.contains("uji tests/uji_hitung.c ... GAGAL") && k.contains("0 lulus, 1 gagal"), "{}", k);
        std::fs::write(p.join("src/hitung.c"), "int tambah(int a, int b) { return a + ; }\n").unwrap();
        let (ok, _, k) = jrgm(&p, &["build"]);
        assert!(!ok && k.contains("src/hitung.c") && k.contains("gagal (kode keluar 1)"), "{}", k);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `[perintah]` menimpa resepnya; kunci, bahasa, dan perkakas yang tidak ada dilaporkan dengan jalan keluarnya.
    #[test]
    fn perintah_menimpa_dan_yang_salah_dilaporkan() {
        let dir = crate::bersama::dir_unik("jrgm-bahasa-lain");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("jrgm.toml"), "[package]\nname = \"x\"\nbahasa = \"lain\"\n\n[perintah]\n\
            jalan = [\"sh -c 'echo satu'\", \"sh -c \\\"echo dua {nama}\\\"\"]\n").unwrap();
        let (ok, _, k) = jrgm(&dir, &["run"]);
        assert!(ok && k.contains("satu\ndua x"), "{}", k);
        let (ok, _, k) = jrgm(&dir, &["test"]);
        assert!(!ok && k.contains("belum punya langkah uji") && k.contains("uji = "), "{}", k);
        std::fs::write(dir.join("jrgm.toml"), "[package]\nname = \"x\"\nbahasa = \"lain\"\n[perintah]\njalankan = \"x\"\n").unwrap();
        let (ok, _, k) = jrgm(&dir, &["run"]);
        assert!(!ok && k.contains("kunci `jalankan` di [perintah] tidak dikenal"), "{}", k);
        std::fs::write(dir.join("jrgm.toml"), "[package]\nname = \"x\"\nbahasa = \"cobol\"\n").unwrap();
        let (ok, _, k) = jrgm(&dir, &["run"]);
        assert!(!ok && k.contains("bahasa `cobol` tidak dikenal") && k.contains("python"), "{}", k);
        std::fs::write(dir.join("jrgm.toml"), "[package]\nname = \"x\"\n[perintah]\njalan = \"echo\"\n").unwrap();
        let (ok, _, k) = jrgm(&dir, &["run"]);
        assert!(!ok && k.contains("[perintah] cuma untuk proyek bahasa lain"), "{}", k);
        std::fs::write(dir.join("jrgm.toml"), "[package]\nname = \"x\"\nbahasa = \"lain\"\n[perintah]\njalan = \"perkakas-yang-tidak-ada-xyz\"\n").unwrap();
        let (ok, _, k) = jrgm(&dir, &["run"]);
        assert!(!ok && k.contains("`perkakas-yang-tidak-ada-xyz` (perkakas proyek lain) tidak ditemukan di PATH"), "{}", k);
        std::fs::write(dir.join("jrgm.toml"), "[package]\nname = \"x\"\nbahasa = \"python\"\n").unwrap();
        let (ok, _, k) = jrgm(&dir, &["apk"]);
        assert!(!ok && k.contains("proyek python belum bisa jadi APK"), "{}", k);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `jrgc --jrgm ...` dan biner jrgc lewat nama `jrgm` berperan jrgm (APK dan Ubuntu di HP cuma membawa jrgc);
    /// `jrgm test` di sana menjalankan jrgc yang sama, dan `jrgm new app --c` = `--bahasa c`.
    #[test]
    fn jrgc_berperan_jrgm() {
        let dir = crate::bersama::dir_unik("jrgc-jrgm");
        std::fs::create_dir_all(&dir).unwrap();
        let jrgc = env!("CARGO_BIN_EXE_jrgc");
        let jalan = |arg: &[&str], di: &Path| {
            let o = Command::new(jrgc).args(arg).current_dir(di).env("JRG_TANPA_PUTAR", "1").env_remove("JRGC_BIN").output().unwrap();
            (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
        };
        let (ok, k) = jalan(&["--jrgm", "new", "halo"], &dir);
        assert!(ok && k.contains("Created `halo`"), "{}", k);
        let (ok, k) = jalan(&["--jrgm", "test"], &dir.join("halo"));
        assert!(ok && k.contains("lulus"), "{}", k);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(jrgc, dir.join("jrgm")).unwrap();
            let o = Command::new(dir.join("jrgm")).arg("run").current_dir(dir.join("halo")).output().unwrap();
            assert!(String::from_utf8_lossy(&o.stdout).contains("Halo dari JawaRustGo++!"), "{:?}", o);
        }
        if ada("cc") {
            let (ok, k) = jalan(&["--jrgm", "new", "c1", "--c"], &dir);
            assert!(ok && k.contains("(c) package"), "{}", k);
            assert!(dir.join("c1/src/main.c").is_file());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `jrgm exec` atas berkas bahasa lain: dikompilasi kalau perlu, kode keluarnya diteruskan, sisa sementaranya dibuang.
    #[test]
    fn exec_berkas_bahasa_lain() {
        let dir = crate::bersama::dir_unik("jrgm-bahasa-exec");
        std::fs::create_dir_all(&dir).unwrap();
        if ada("cc") {
            std::fs::write(dir.join("e.c"), "#include <stdio.h>\nint main(int c, char **v) { printf(\"argc %d\\n\", c); return 3; }\n").unwrap();
            let (_, kode, k) = jrgm(&dir, &["exec", "e.c", "--", "x"]);
            assert!(kode == 3 && k.contains("argc 2"), "{}: {}", kode, k);
        }
        if ada("python3") {
            std::fs::write(dir.join("e.py"), "print('dari python')\n").unwrap();
            let (ok, _, k) = jrgm(&dir, &["exec", "e.py"]);
            assert!(ok && k.contains("dari python"), "{}", k);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}


/// Fase JRGM-4: git sebagai registry — `publish` menandai versi di repositori (lokal, tanpa
/// internet), `add --git --version` mengambil dan mengunci commit-nya, `update` memajukannya.
#[test]
fn dependensi_git_publish_add_update() {
    let j = jrgm();
    let akar = std::env::temp_dir().join(format!("jrg_dep_git_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&akar);
    std::fs::create_dir_all(&akar).unwrap();
    let jalan = |dir: &Path, args: &[&str]| {
        // `publish` membuat tag beranotasi, jadi butuh identitas git; runner CI tidak punya.
        let o = Command::new(&j).args(args).current_dir(dir).env("JRGM_TEMBOLOK", akar.join("tembolok"))
            .env("JRG_SOPAN", "1").env("GIT_COMMITTER_NAME", "u").env("GIT_COMMITTER_EMAIL", "u@j")
            .env("GIT_AUTHOR_NAME", "u").env("GIT_AUTHOR_EMAIL", "u@j").output().unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
    };
    let git = |dir: &Path, args: &[&str]| {
        let o = Command::new("git").args(["-c", "user.email=u@j", "-c", "user.name=u"]).args(args)
            .current_dir(dir).output().unwrap();
        assert!(o.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&o.stderr));
    };
    git(&akar, &["init", "-q", "--bare", "pusat.git"]);
    let pusat = akar.join("pusat.git").display().to_string();
    assert!(jalan(&akar, &["new", "bantu"]).0);
    let b = akar.join("bantu");
    std::fs::write(b.join("src/bantu.jrg"), "pub fn sapa(n) { return \"halo \" + n }\n").unwrap();
    git(&b, &["init", "-q"]);
    git(&b, &["add", "-A"]);
    git(&b, &["commit", "-qm", "awal"]);
    git(&b, &["remote", "add", "origin", &pusat]);
    let (ok, t) = jalan(&b, &["publish"]);
    assert!(ok && t.contains("v0.1.0"), "{}", t);
    let (ok, t) = jalan(&b, &["publish"]);
    assert!(!ok && t.contains("sudah ada"), "versi yang sudah terbit tidak boleh ditimpa:\n{}", t);

    assert!(jalan(&akar, &["new", "app"]).0);
    let a = akar.join("app");
    std::fs::write(a.join("src/main.jrg"), "package main\nimport \"bantu\"\npub fn main() { wok(bantu::sapa(\"x\")) }\n").unwrap();
    let (ok, t) = jalan(&a, &["add", "bantu", "--git", &pusat, "--version", "0.1.0"]);
    assert!(ok, "{}", t);
    let (ok, t) = jalan(&a, &["run"]);
    assert!(ok && t.contains("halo x"), "{}", t);
    let kunci = std::fs::read_to_string(a.join("jrgm.lock")).unwrap();
    assert!(kunci.contains("@ v0.1.0"), "{}", kunci);

    // Versi baru terbit; app yang mengunci v0.1.0 tetap di sana sampai manifesnya diubah.
    std::fs::write(b.join("src/bantu.jrg"), "pub fn sapa(n) { return \"hai \" + n }\n").unwrap();
    let m = std::fs::read_to_string(b.join("jrgm.toml")).unwrap().replace("\"0.1.0\"", "\"0.2.0\"");
    std::fs::write(b.join("jrgm.toml"), m).unwrap();
    git(&b, &["commit", "-qam", "dua"]);
    assert!(jalan(&b, &["publish"]).0);
    assert!(jalan(&a, &["run"]).1.contains("halo x"));
    let m = std::fs::read_to_string(a.join("jrgm.toml")).unwrap().replace("version = \"0.1.0\" }", "version = \"0.2.0\" }");
    std::fs::write(a.join("jrgm.toml"), m).unwrap();
    assert!(jalan(&a, &["run"]).1.contains("hai x"));
    let kunci = std::fs::read_to_string(a.join("jrgm.lock")).unwrap();
    assert!(kunci.contains("@ v0.2.0") && !kunci.contains("@ v0.1.0"), "entri basi tidak dibuang:\n{}", kunci);

    // Cabang: `update` memajukan commit yang dikunci.
    git(&b, &["push", "-q", "origin", "HEAD:refs/heads/utama"]);
    assert!(jalan(&a, &["remove", "bantu"]).0);
    assert!(jalan(&a, &["add", "bantu", "--git", &pusat, "--rev", "utama"]).0);
    std::fs::write(b.join("src/bantu.jrg"), "pub fn sapa(n) { return \"yo \" + n }\n").unwrap();
    git(&b, &["commit", "-qam", "tiga"]);
    git(&b, &["push", "-q", "origin", "HEAD:refs/heads/utama"]);
    assert!(jalan(&a, &["run"]).1.contains("hai x"), "commit terkunci tidak boleh bergeser sendiri");
    let (ok, t) = jalan(&a, &["update"]);
    assert!(ok && t.contains("1 dimajukan"), "{}", t);
    assert!(jalan(&a, &["run"]).1.contains("yo x"));
    let (ok, t) = jalan(&a, &["update", "hantu"]);
    assert!(!ok && t.contains("bukan dependensi git"), "{}", t);
    let (ok, t) = jalan(&a, &["add", "lain", "--git", &akar.join("tidak-ada.git").display().to_string()]);
    assert!(!ok && t.contains("tidak bisa mengambil"), "{}", t);
    let _ = std::fs::remove_dir_all(&akar);
}

/// `jrgm lint`: galat yang sama dengan `--periksa` membuat kode keluar 1; peringatannya menyebut
/// import yang tidak dipakai, fungsi yang tidak dipanggil, berkas uji kosong, dan berkas belum rapi.
#[test]
fn lint_melapor_galat_dan_peringatan() {
    let j = jrgm();
    let akar = std::env::temp_dir().join(format!("jrg_lint_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&akar);
    std::fs::create_dir_all(&akar).unwrap();
    let jalan = |dir: &Path, args: &[&str]| {
        let o = Command::new(&j).args(args).current_dir(dir).env("JRG_SOPAN", "1").output().unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
    };
    assert!(jalan(&akar, &["new", "p"]).0);
    let p = akar.join("p");
    let (ok, t) = jalan(&p, &["lint"]);
    assert!(ok && t.contains("0 galat, 0 peringatan"), "rangka baru harus bersih:\n{}", t);
    std::fs::write(p.join("src/util.jrg"), "pub fn dua(x) {\n    return x * 2\n}\n\nfn _rahasia() {\n    return 1\n}\n").unwrap();
    std::fs::write(p.join("src/main.jrg"), "package main\n\nimport \"util\"\nimport \"std/set\"\n\npub fn main() {\n    let x: float = 7\n    wok(util::dua(x))\n}\n").unwrap();
    let (ok, t) = jalan(&p, &["lint"]);
    assert!(ok, "peringatan saja tidak membuat lint gagal:\n{}", t);
    assert!(t.contains("`import \"std/set\"` tidak dipakai") && t.contains("`_rahasia` tidak pernah dipanggil"), "{}", t);
    assert!(!t.contains("`import \"util\"` tidak dipakai"), "{}", t);
    assert!(t.contains("literal bulat `7`") && t.contains("tulis `7.0`"), "{}", t);
    let (ok, _) = jalan(&p, &["lint", "--ketat"]);
    assert!(!ok, "--ketat harus gagal kalau ada peringatan");
    // Lokal yang tidak dibaca (Fase SUBSTR): tangkapan closure, `+=`, bayangan, dan `_x` bukan temuan.
    std::fs::write(p.join("src/main.jrg"), "package main\n\npub fn main() {\n    let a = 1\n    let f = || a + 1\n    var c = 0\n    c += 1\n    let s = 5\n    let s = s + 1\n    let _sengaja = 9\n    let hilang = 10\n    var tulis = [1]\n    tulis[0] = 2\n    let [u, w] = [1, 2]\n    wok(f(), s, u)\n}\n").unwrap();
    let (_, t) = jalan(&p, &["lint"]);
    for n in ["hilang", "tulis", "w"] {
        assert!(t.contains(&format!("`{}` diberi nilai tapi tidak pernah dibaca", n)), "{}:\n{}", n, t);
    }
    assert!(t.contains("main.jrg:11:9"), "lokasinya nama yang dideklarasikan:\n{}", t);
    for n in ["a", "c", "s", "_sengaja", "u", "f"] {
        assert!(!t.contains(&format!("`{}` diberi nilai", n)), "tuduhan palsu atas `{}`:\n{}", n, t);
    }
    std::fs::write(p.join("tests/kosong.jrg"), "fn bantu() {\n    return 1\n}\n").unwrap();
    std::fs::write(p.join("src/main.jrg"), "package main\npub fn main() {  wok(hantu) }\n").unwrap();
    let (ok, t) = jalan(&p, &["lint"]);
    assert!(!ok && t.contains("`hantu` belum dikenal") && t.contains("main.jrg:2:"), "{}", t);
    assert!(t.contains("kosong.jrg tidak punya satu pun fungsi uji") && t.contains("belum rapi"), "{}", t);
    let _ = std::fs::remove_dir_all(&akar);
}
