// tests/span_vm.rs — tests/span_vm.rs — mengunci LOKASI error backend VM.

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

struct Ruang(std::path::PathBuf);
impl Ruang {
    fn baru() -> Ruang {
        let d = crate::bersama::dir_unik("jrg-span");
        std::fs::create_dir_all(&d).expect("bikin dir");
        Ruang(d)
    }
}
impl Drop for Ruang {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

/// `(baris, kolom, lebar_caret)` dari sebuah diagnostik, atau `None` kalau
/// diagnostiknya tidak punya lokasi.
fn lokasi(berkas: &std::path::Path, vm: bool) -> Option<(u32, u32, usize)> {
    let mut c = Command::new(jrgc());
    c.arg(berkas).arg("--sopan");
    if vm { c.arg("--vm"); }
    let out = c.output().expect("jalankan jrgc");
    let teks = format!("{}{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // `--> path:baris:kolom`
    let baris_lokasi = teks.lines().find(|b| b.trim_start().starts_with("-->"))?;
    let bagian: Vec<&str> = baris_lokasi.trim().rsplitn(3, ':').collect();
    let kolom: u32 = bagian.first()?.trim().parse().ok()?;
    let baris: u32 = bagian.get(1)?.trim().parse().ok()?;
    // Baris caret: `  | <spasi>^^^^`
    let lebar = teks.lines()
        .filter_map(|b| {
            let sisa = b.trim_start().strip_prefix('|')?;
            let t = sisa.trim_start();
            if t.starts_with('^') { Some(t.chars().take_while(|c| *c == '^').count()) } else { None }
        })
        .next()?;
    Some((baris, kolom, lebar))
}

fn nama_berkas(teks: &str, berkas: &std::path::Path, vm: bool) -> String {
    let mut c = Command::new(jrgc());
    c.arg(berkas).arg("--sopan");
    if vm { c.arg("--vm"); }
    let out = c.output().expect("jalankan jrgc");
    let _ = teks;
    let semua = format!("{}{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    semua.lines()
        .find(|b| b.trim_start().starts_with("-->"))
        .map(|b| b.trim().trim_start_matches("-->").trim().to_string())
        .unwrap_or_default()
}

/// Program-program yang bentuk kesalahannya berbeda-beda, dipilih supaya
/// span yang salah TETAP terlihat masuk akal — itu satu-satunya cara
/// membedakan "tepat" dari "kebetulan mendekati".
const KASUS: &[(&str, &str)] = &[
    ("index luar batas di tengah ekspresi",
     "package main\npub fn main() {\n let a=[1,2]\n let b=9\n wok(a[b] + a[0] + 1)\n}\n"),
    ("variabel tak dikenal di tengah ekspresi",
     "package main\npub fn main() {\n let x = 1\n wok(x + wokwokwo + 2)\n}\n"),
    ("bagi nol di tengah ekspresi",
     "package main\npub fn main() {\n let n = 0\n wok(1 + 10 / n + 3)\n}\n"),
    ("field yang tidak ada",
     "package main\nstruct P { pub x: int = 1 }\npub fn main() {\n let p = P{}\n wok(p.x + p.zzz)\n}\n"),
    ("index negatif di luar batas",
     "package main\npub fn main() {\n let a=[1,2,3]\n wok(a[-9])\n}\n"),
    // ── JALUR TULIS ─────────────────────────────────────────────────────
    ("let: pola yang tidak cocok",
     "package main\npub fn main() {\n let (a, 5) = (1, 6)\n wok(a)\n}\n"),
    ("tulis: operator gabungan atas variabel telanjang",
     "package main\npub fn main() {\n var n = 7\n n %= \"s\"\n}\n"),
    ("tulis: index luar batas",
     "package main\npub fn main() {\n var a=[1,2]\n a[9] = 5\n}\n"),
    ("tulis: index negatif luar batas",
     "package main\npub fn main() {\n var a=[1,2]\n a[-9] = 5\n}\n"),
    ("tulis: key antara belum ada",
     "package main\npub fn main() {\n var d = {\"a\": 1}\n d[\"b\"][\"c\"] = 1\n}\n"),
    ("tulis: langkah antara di array",
     "package main\npub fn main() {\n var a=[[1,2]]\n a[7][0] = 5\n}\n"),

    // Chunk BERBEDA: fungsi, closure, dan method masing-masing punya kolam
    // span sendiri. Kalau pemetaan indeks->span salah chunk, ketiganya
    // akan menunjuk tempat yang ngawur.
    ("di dalam fungsi",
     "package main\nfn f(a) { return a[7] }\npub fn main() { wok(f([1,2])) }\n"),
    ("di dalam closure",
     "package main\npub fn main() {\n let g = |a| a[7]\n wok(g([1,2]))\n}\n"),
    ("di dalam method",
     "package main\nstruct S { pub v: []int = [] }\n\
      impl S { fn amb(self) { return self.v[5] } }\npub fn main() { wok(S{v:[1]}.amb()) }\n"),
    ("geser bit tak sah",
     "package main\npub fn main() {\n wok(1 + (1 << 200))\n}\n"),
    ("cast nil",
     "package main\npub fn main() {\n let n = nil\n wok(1 + (n as int))\n}\n"),
    ("potongan terbalik",
     "package main\npub fn main() {\n let a=[1,2,3]\n wok(a[2:1])\n}\n"),
    ("memanggil yang bukan fungsi",
     "package main\npub fn main() {\n let x = 5\n wok(x(1))\n}\n"),
    // HOF memanggil balik lewat jalur yang dulu tidak membawa lokasi sama sekali di `--vm`.
    ("HOF namespace dengan argumen bukan fungsi",
     "package main\npub fn main() {\n let a = [1]\n wok(arr::map(a, 5))\n}\n"),
    ("method HOF dengan argumen bukan fungsi",
     "package main\npub fn main() {\n let a = [1]\n wok(a.map(5))\n}\n"),
    ("aritas method HOF",
     "package main\npub fn main() {\n let a = [1]\n wok(a.map())\n}\n"),
    ("argumen kurang",
     "package main\nfn f(a, b) { return a }\npub fn main() { wok(f(1)) }\n"),
    ("luapan integer",
     "package main\npub fn main() {\n wok(170141183460469231731687303715884105727 + 1)\n}\n"),
    ("error sesudah keluaran program",
     "package main\npub fn main() {\n wok(\"satu\")\n wok(\"dua\")\n let a=[1]\n wok(a[5])\n}\n"),
];

#[test]
fn lokasi_error_vm_sama_persis_dengan_interpreter() {
    let r = Ruang::baru();
    let mut gagal = Vec::new();
    for (nama, src) in KASUS {
        let f = r.0.join("u.jrg");
        std::fs::write(&f, src).expect("tulis sumber");
        let i = lokasi(&f, false);
        let v = lokasi(&f, true);
        assert!(
            i.is_some(),
            "{}: interpreter sendiri tidak memberi lokasi — kasus ujinya yang salah, \
             bukan VM-nya", nama
        );
        if i != v {
            gagal.push(format!(
                "  {}\n     interp: {:?}\n     vm    : {:?}", nama, i, v
            ));
        }
    }
    assert!(
        gagal.is_empty(),
        "{} dari {} kasus punya lokasi yang berbeda antar-backend.\n\
         Lokasi yang HAMPIR benar lebih menyesatkan daripada tidak ada lokasi:\n\
         yang membacanya akan mencari di tempat yang ditunjuk dan tidak menemukan apa-apa.\n{}",
        gagal.len(), KASUS.len(), gagal.join("\n")
    );
}

/// Error di dalam modul harus menunjuk BERKAS MODULNYA, bukan berkas utama.
#[test]
fn error_di_dalam_modul_menunjuk_berkas_modulnya() {
    let r = Ruang::baru();
    std::fs::create_dir_all(r.0.join("src")).expect("bikin src");
    std::fs::write(r.0.join("jrgm.toml"),
        "[package]\nname = \"uji\"\nversion = \"0.1.0\"\n").expect("tulis toml");
    std::fs::write(r.0.join("src/lib.jrg"),
        "package lib\npub fn ambil(a) {\n    return a[7]\n}\n").expect("tulis lib");
    let utama = r.0.join("src/main.jrg");
    std::fs::write(&utama,
        "package main\nimport \"lib\"\npub fn main() {\n    wok(\"output dulu\")\n    \
         wok(lib::ambil([1, 2]))\n}\n").expect("tulis main");

    let i = lokasi(&utama, false);
    let v = lokasi(&utama, true);
    assert!(i.is_some(), "interpreter tidak memberi lokasi untuk error di modul");
    assert_eq!(
        i, v,
        "lokasi error lintas-modul berbeda antar-backend: interp {:?}, vm {:?}", i, v
    );
    let nama_i = nama_berkas("", &utama, false);
    let nama_v = nama_berkas("", &utama, true);
    assert!(
        nama_v.contains("lib.jrg"),
        "VM menunjuk berkas yang SALAH: `{}` — errornya ada di lib.jrg.\n\
         Kalau `file_id` modul tidak unik, span-nya mengaku milik berkas utama \
         dan cuplikan sumbernya jadi baris yang tidak ada hubungannya.", nama_v
    );
    assert_eq!(nama_i, nama_v, "nama berkas berbeda antar-backend");
}

/// Galat LEX/PARSE di modul yang di-`import` menunjuk berkas modulnya, dengan
/// kalimat yang sama di dua mesin. Dulu tanpa lokasi, dua kalimat berbeda.
#[test]
fn galat_parse_di_modul_punya_lokasi() {
    for (isi, baris, kata) in [
        ("package bantu\npub fn dua() {\n    let x = (1 +\n}\n", 4, "harusnya ada ekspresi"),
        ("package bantu\npub fn dua() {\n    let s = \"abc\n}\n", 3, "string belum ditutup"),
    ] {
        let r = Ruang::baru();
        std::fs::write(r.0.join("bantu.jrg"), isi).expect("tulis modul");
        let utama = r.0.join("utama.jrg");
        std::fs::write(&utama, "package main\nimport \"bantu\"\npub fn main() { wok(\"a\") }\n")
            .expect("tulis utama");
        let (i, v) = (lokasi(&utama, false), lokasi(&utama, true));
        assert!(i.is_some(), "galat di modul tanpa lokasi: {}", kata);
        assert_eq!(i, v, "lokasi berbeda antar-mesin untuk `{}`", kata);
        assert_eq!(i.map(|l| l.0), Some(baris), "baris salah untuk `{}`", kata);
        let (ni, nv) = (nama_berkas("", &utama, false), nama_berkas("", &utama, true));
        assert!(ni.contains("bantu.jrg") && ni == nv, "berkasnya: interp `{}`, vm `{}`", ni, nv);
        let pesan = |vm: bool| {
            let mut c = Command::new(jrgc());
            c.arg(&utama).arg("--sopan");
            if vm { c.arg("--vm"); }
            let out = c.output().expect("jalankan jrgc");
            String::from_utf8_lossy(&out.stderr).lines()
                .find(|l| l.contains("error")).unwrap_or("").to_string()
        };
        let (pi, pv) = (pesan(false), pesan(true));
        assert!(pi.contains(kata) && pi == pv, "kalimat: interp `{}`, vm `{}`", pi, pv);
    }
}

/// Galat dari modul yang diimpor modul LAIN, dan galat saat modul diinisialisasi,
/// juga menunjuk berkasnya — dulu interpreter membungkusnya jadi teks tanpa lokasi.
#[test]
fn galat_modul_transitif_dan_inisialisasi_punya_lokasi() {
    for (dalam, baris, kata) in [
        ("package dalam\npub fn f() {\n    let x = (1 +\n}\n", 4, "harusnya ada ekspresi"),
        ("package dalam\npub let A = 1 / 0\n", 2, "bagi dengan nol"),
    ] {
        let r = Ruang::baru();
        std::fs::write(r.0.join("dalam.jrg"), dalam).expect("tulis dalam");
        std::fs::write(r.0.join("bantu.jrg"), "package bantu\nimport \"dalam\"\npub fn dua() { 2 }\n")
            .expect("tulis bantu");
        let utama = r.0.join("utama.jrg");
        std::fs::write(&utama, "package main\nimport \"bantu\"\npub fn main() { wok(\"a\") }\n")
            .expect("tulis utama");
        let (i, v) = (lokasi(&utama, false), lokasi(&utama, true));
        assert!(i.is_some(), "tanpa lokasi: {}", kata);
        assert_eq!(i, v, "lokasi berbeda antar-mesin untuk `{}`", kata);
        assert_eq!(i.map(|l| l.0), Some(baris), "baris salah untuk `{}`", kata);
        let (ni, nv) = (nama_berkas("", &utama, false), nama_berkas("", &utama, true));
        assert!(ni.contains("dalam.jrg") && ni == nv, "berkasnya: interp `{}`, vm `{}`", ni, nv);
    }
}

/// Diagnostik VM harus punya cuplikan sumber dan caret — bukan cuma pesan.
#[test]
fn diagnostik_vm_punya_cuplikan_dan_caret() {
    let r = Ruang::baru();
    let f = r.0.join("u.jrg");
    std::fs::write(&f, "package main\npub fn main() {\n let a=[1,2,3]\n wok(a[9])\n}\n")
        .expect("tulis sumber");
    let out = Command::new(jrgc()).arg(&f).arg("--vm").arg("--sopan")
        .output().expect("jalankan");
    let teks = format!("{}{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    for wajib in ["-->", "wok(a[9])", "^^^^"] {
        assert!(
            teks.contains(wajib),
            "diagnostik VM kehilangan `{}` — ini yang hilang selama `Instruction` \
             menyimpan nomor baris yang selalu nol:\n{}", wajib, teks
        );
    }
    // Dan pesannya tidak boleh lagi menempelkan `[line N]`: lokasinya
    // sekarang ditampilkan diagnostik, jadi menempelkannya lagi ke pesan
    // berarti satu informasi di dua tempat yang bisa berbeda.
    assert!(
        !teks.contains("[line "),
        "pesan masih menempelkan `[line N]` padahal lokasinya sudah punya barisnya \
         sendiri:\n{}", teks
    );
}
