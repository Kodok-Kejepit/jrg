// tests/jit_parity.rs — mesin keempat (`--jit`, `dokumentasi/jit/07-kebenaran-dan-uji.md`).
// Fase J1: `--jit` = VM berprofil; tabelnya sudah dipasang sekarang supaya tiap fase sesudahnya
// (J2 kode mesin, J3 deopt) langsung diukur terhadap seluruh `PARITAS`.

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

include!("bersama/paritas.rs");

struct Hasil { stdout: String, stderr: String, sukses: bool }

fn jalankan(src: &str, arg: &[&str], env: &[(&str, &str)]) -> Hasil {
    let dir = bersama::dir_unik("jrg-jit");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let mut cmd = Command::new(jrgc());
    // `TMPDIR` sendiri: tiga tes tabel berjalan bersamaan, dan kasus `fs:` memakai jalur tetap di bawah
    // `fs::temp_dir()`.
    cmd.arg(&file).args(arg).env("JRG_SOPAN", "1").env_remove("JRG_JIT_LOG").env("TMPDIR", &dir)
        .stdin(std::process::Stdio::null());
    for (k, v) in env { cmd.env(k, v); }
    let out = cmd.output().expect("jalankan jrgc");
    let _ = std::fs::remove_dir_all(&dir);
    // Tiap jalan punya direktori sementara sendiri; jalurnya bukan bagian yang dibandingkan.
    let (jalur, sementara) = (file.display().to_string(), dir.display().to_string());
    let normal = |b: &[u8]| String::from_utf8_lossy(b).replace(&jalur, "<uji>").replace(&sementara, "<tmp>");
    Hasil {
        stdout: normal(&out.stdout).trim().to_string(),
        stderr: normal(&out.stderr),
        sukses: out.status.success(),
    }
}

#[test]
fn paritas_lewat_jit() {
    let mut gagal = Vec::new();
    for (nama, src) in PARITAS {
        let i = jalankan(src, &[], &[]);
        let j = jit_sama(src, &i, &[]);
        if i.sukses != j.sukses || i.stdout != j.stdout {
            gagal.push(format!("  {nama}\n     interpreter ({}): {:?}\n     jit         ({}): {:?}",
                i.sukses, i.stdout, j.sukses, j.stdout));
        }
    }
    assert!(gagal.is_empty(), "{} kasus PARITAS beda di --jit:\n{}", gagal.len(), gagal.join("\n"));
}

const FIB: &str = "fn fib(n) {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\nfn main() {\n  var t = 0\n  for i in 0..10 { t = t + i }\n  wok(fib(15), t)\n}\n";

/// Sama dengan `FIB`, tapi menyentuh teks: tidak layak dikompilasi J2, jadi tiap panggilan lewat VM.
const FIB_VM: &str = "fn fib(n) {\n  let s = \"x\"\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\nfn main() {\n  var t = 0\n  for i in 0..10 { t = t + i }\n  wok(fib(15), t)\n}\n";

#[test]
fn profil_t0_melihat_fungsi_panas() {
    let h = jalankan(FIB_VM, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    assert!(h.sukses, "{}", h.stderr);
    assert_eq!(h.stdout, "610 45");
    // fib(15) memanggil fib tepat 1973 kali; angka lain berarti perekamnya salah hitung.
    let baris = h.stderr.lines().find(|b| b.contains(" fib: panggilan")).unwrap_or_else(|| panic!("fib tidak dilaporkan:\n{}", h.stderr));
    assert!(baris.contains("panggilan 1973,"), "{baris}");
    let utama = h.stderr.lines().find(|b| b.contains(" main: panggilan")).unwrap_or_else(|| panic!("main tidak dilaporkan:\n{}", h.stderr));
    assert!(utama.contains("putaran 10,"), "{utama}");
}

#[test]
fn profil_tidak_menyentuh_stdout() {
    let tanpa = jalankan(FIB, &["--jit"], &[]);
    let dengan = jalankan(FIB, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    assert_eq!(tanpa.stdout, dengan.stdout);
    assert!(tanpa.stderr.is_empty(), "tanpa JRG_JIT_LOG tidak boleh ada laporan: {}", tanpa.stderr);
}

// ── J2: fungsi bulat murni sebagai kode mesin (`dokumentasi/jit/j2-rencana.md`) ──────────────

/// Kasus berwaktu (`sleep` bersama goroutine/`Future`/`Task`): JIT tidak menjanjikan waktu, dan kompilasi mode
/// stres di runner sibuk pernah membalik urutan dua baris. Yang beda diulang paling banyak dua kali (dicetak);
/// cacat yang konsisten tetap merah.
fn jit_sama(src: &str, acuan: &Hasil, env: &[(&str, &str)]) -> Hasil {
    let berwaktu = src.contains("sleep") && ["spawn", "Future", "Task", "go "].iter().any(|k| src.contains(k));
    let mut j = jalankan(src, &["--jit"], env);
    for ulang in 1..=2 {
        if !berwaktu || (j.sukses == acuan.sukses && j.stdout == acuan.stdout) { break; }
        eprintln!("catatan: kasus berwaktu diulang ({ulang}) di --jit {env:?}");
        j = jalankan(src, &["--jit"], env);
    }
    j
}

const STRES: (&str, &str) = ("JRG_JIT_AMBANG", "0");

#[test]
fn paritas_lewat_jit_stres() {
    let mut gagal = Vec::new();
    for (nama, src) in PARITAS {
        let i = jalankan(src, &[], &[]);
        let j = jit_sama(src, &i, &[STRES]);
        if i.sukses != j.sukses || i.stdout != j.stdout {
            gagal.push(format!("  {nama}\n     interpreter ({}): {:?}\n     jit stres   ({}): {:?}",
                i.sukses, i.stdout, j.sukses, j.stdout));
        }
    }
    assert!(gagal.is_empty(), "{} kasus PARITAS beda di --jit (stres):\n{}", gagal.len(), gagal.join("\n"));
}

#[test]
fn fungsi_bulat_dikompilasi() {
    let h = jalankan(FIB, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    assert!(h.sukses, "{}", h.stderr);
    assert_eq!(h.stdout, "610 45");
    assert!(h.stderr.lines().any(|b| b.contains("dikompilasi:") && b.contains("fib")), "fib tidak dikompilasi:\n{}", h.stderr);
}

/// Program yang hasilnya sama di interpreter dan `--jit` (stres): stdout, stderr dan status.
fn sama_dengan_interpreter(src: &str) {
    let i = jalankan(src, &[], &[]);
    let j = jalankan(src, &["--jit"], &[STRES]);
    assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr));
}

#[test]
fn luapan_mundur_ke_128_bit() {
    sama_dengan_interpreter("fn kali(a, b) { return a * b }\nfn tambah(a, b) { return a + b }\nfn main() {\n  wok(kali(3, 4))\n  wok(kali(kali(3037000500, 3037000500), 4))\n  wok(tambah(9223372036854775807, 1))\n}\n");
}

#[test]
fn bagi_nol_berkalimat_sama() {
    sama_dengan_interpreter("fn sisa(a, b) { return a % b }\nfn main() {\n  wok(sisa(7, 3), sisa(-7, 3))\n  wok(sisa(7, 0))\n}\n");
}

#[test]
fn rekursi_dalam_mundur_ke_vm() {
    let src = "fn turun(n) {\n  if n == 0 { return 0 }\n  return turun(n - 1) + 1\n}\nfn main() { wok(turun(20000)) }\n";
    let v = jalankan(src, &["--vm"], &[]);
    let j = jalankan(src, &["--jit"], &[STRES]);
    assert_eq!((v.sukses, v.stdout.as_str()), (true, "20000"));
    assert_eq!((j.sukses, j.stdout.as_str()), (true, "20000"));
}

// ── J3: deopt di tengah fungsi, bingkai native disusun ulang jadi bingkai VM (`j3-rencana.md`) ───

const ACAK: (&str, &str) = ("JRG_JIT_STRES", "deopt-acak");

#[test]
fn paritas_lewat_jit_deopt_acak() {
    let mut gagal = Vec::new();
    for (nama, src) in PARITAS {
        let i = jalankan(src, &[], &[]);
        let j = jit_sama(src, &i, &[STRES, ACAK]);
        if i.sukses != j.sukses || i.stdout != j.stdout {
            gagal.push(format!("  {nama}\n     interpreter ({}): {:?}\n     deopt acak  ({}): {:?}",
                i.sukses, i.stdout, j.sukses, j.stdout));
        }
    }
    assert!(gagal.is_empty(), "{} kasus PARITAS beda di --jit (deopt acak):\n{}", gagal.len(), gagal.join("\n"));
}

/// Luapan di callee terdalam: seluruh rantai native lanjut di VM dari titik itu, hasilnya 128 bit.
const RANTAI: &str = "fn g(x) { return x * x }\nfn f(n) {\n  if n == 0 { return g(4000000000) }\n  return f(n - 1) + 1\n}\nfn main() { wok(f(5), f(30)) }\n";

#[test]
fn deopt_di_tengah_rantai_native() {
    sama_dengan_interpreter(RANTAI);
    let h = jalankan(RANTAI, &["--jit"], &[STRES, ("JRG_JIT_LOG", "ringkas")]);
    assert_eq!(h.stdout, "16000000000000000005 16000000000000000030");
    assert!(h.stderr.lines().any(|b| b == "[jit] deopt: 2"), "tiap luapan tepat satu deopt:\n{}", h.stderr);
}

#[test]
fn deopt_acak_sama_dengan_interpreter() {
    let src = "fn sisa(a, b) { return a % b }\nfn fib(n) {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\nfn main() {\n  wok(fib(18), sisa(-7, 3))\n  wok(sisa(7, 0))\n}\n";
    let i = jalankan(src, &[], &[]);
    let j = jalankan(src, &["--jit"], &[STRES, ACAK]);
    assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr));
}

// ── J4: titik keluar, nilai milik VM di bingkai, OSR (`j4-rencana.md`) ─────────────────────────────

include!("bersama/paritas_llvm.rs");
include!("bersama/gagal_saat_jalan.rs");

/// Dua mode yang memaksa kode native: semua dikompilasi seketika, dan ditambah deopt acak.
const MODE_PAKSA: [&[(&str, &str)]; 2] = [&[STRES], &[STRES, ACAK]];

#[test]
fn paritas_llvm_lewat_jit() {
    let mut gagal = Vec::new();
    for (nama, src) in PARITAS_LLVM {
        let i = jalankan(src, &[], &[]);
        for env in MODE_PAKSA {
            let j = jalankan(src, &["--jit"], env);
            if i.sukses != j.sukses || i.stdout != j.stdout {
                gagal.push(format!("  {nama} {env:?}\n     interpreter: {:?}\n     jit:         {:?}", i.stdout, j.stdout));
            }
        }
    }
    assert!(gagal.is_empty(), "{} kasus PARITAS_LLVM beda di --jit:\n{}", gagal.len(), gagal.join("\n"));
}

/// Galatnya identik dengan VM tempat kode native berjalan (termasuk jejaknya, yang kedalamannya milik
/// VM), dan sama dengan acuan pada keluaran, status dan kalimat yang dicatat tabelnya.
#[test]
fn gagal_saat_jalan_sama_di_jit() {
    let mut gagal = Vec::new();
    for (nama, src, _, memuat) in GAGAL_SAAT_JALAN {
        let i = jalankan(src, &[], &[]);
        let v = jalankan(src, &["--vm"], &[]);
        for env in MODE_PAKSA {
            let j = jalankan(src, &["--jit"], env);
            if (v.sukses, &v.stdout, &v.stderr) != (j.sukses, &j.stdout, &j.stderr)
                || (i.sukses, &i.stdout) != (j.sukses, &j.stdout) || !j.stderr.contains(memuat) {
                gagal.push(format!("  {nama} {env:?}\n     interpreter: {:?} {:?}\n     vm:          {:?}\n     jit:         {:?} {:?}",
                    i.stdout, i.stderr, v.stderr, j.stdout, j.stderr));
            }
        }
    }
    assert!(gagal.is_empty(), "{} kasus GAGAL_SAAT_JALAN beda di --jit:\n{}", gagal.len(), gagal.join("\n"));
}

/// Loop di `main` (dipanggil sekali) naik lewat OSR sambil membawa teks yang tidak pernah disentuh native.
const OSR: &str = "fn main() {\n  let label = \"jumlah\"\n  var s = 0\n  for i in 0..300000 { s = s + (i * 3) % 7 }\n  var j = 0\n  while j < 5000 { j += 1 }\n  wok(label, s, j)\n}\n";

/// `Return` bool dari VM, dan fungsi dengan parameter teks yang keluar ke VM sesudah loop-nya.
const KELUAR: &str = "fn genap(x) { return x % 2 == 0 }\nfn tempel(s, n) {\n  var t = 0\n  for i in 0..n { t = t + i }\n  return s + str(t)\n}\nfn main() {\n  var c = 0\n  for i in 0..3000 { if genap(i) { c += 1 } }\n  wok(c, tempel(\"t=\", 100))\n  wok(tempel(\"u=\", 7))\n}\n";

#[test]
fn osr_dan_titik_keluar() {
    for (src, harus, nama) in [(OSR, "jumlah 899997 5000", &["main"][..]), (KELUAR, "1500 t=4950\nu=21", &["genap", "tempel"][..])] {
        sama_dengan_interpreter(src);
        let i = jalankan(src, &[], &[]);
        assert_eq!(i.stdout, harus);
        let h = jalankan(src, &["--jit"], &[ACAK, ("JRG_JIT_LOG", "ringkas")]);
        assert_eq!(h.stdout, harus, "{}", h.stderr);
        let h = jalankan(src, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
        assert_eq!(h.stdout, harus);
        let jadi = h.stderr.lines().find(|b| b.contains("dikompilasi:")).unwrap_or_else(|| panic!("tidak ada yang dikompilasi:\n{}", h.stderr));
        for n in nama { assert!(jadi.contains(n), "{n} tidak dikompilasi: {jadi}"); }
    }
}

/// J4 langkah 2: pecahan (NaN, tak hingga, `-0.0`, campur bulat), `/` bulat memotong, `5 / 0` lewat VM.
const PECAHAN: &str = "fn ukur(x, y) {\n  var s = 0.0\n  var i = 0\n  while i < 2000 {\n    s = s + x * 1.5 - y / 3\n    if s > 1000.0 { s = s - 999.5 }\n    i += 1\n  }\n  return s\n}\nfn bagi(a, b) { return a / b }\nfn banding(a, b) { return a < b }\nfn main() {\n  let nan = 0.0 / 0.0\n  wok(ukur(2.5, 7), ukur(1, 2.0), ukur(-0.0, 0))\n  wok(bagi(7, 2), bagi(-7, 2), bagi(7, -2), bagi(7.0, 2), bagi(1.0, 0.0), bagi(-1, 0.0))\n  wok(banding(nan, 1.0), banding(1.0, nan), nan == nan, nan != nan, banding(2, 2.5))\n  var t = 0.0\n  for i in 0..100000 { t = t + i * 0.5 }\n  wok(t, -t, 0.0 * -1.0)\n  wok(bagi(5, 0))\n}\n";

#[test]
fn pecahan_dan_bagi() {
    sama_dengan_interpreter(PECAHAN);
    let i = jalankan(PECAHAN, &[], &[]);
    let j = jalankan(PECAHAN, &["--jit"], &[STRES, ACAK]);
    assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr));
    let h = jalankan(PECAHAN, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    let jadi = h.stderr.lines().find(|b| b.contains("dikompilasi:")).unwrap_or_default();
    for n in ["ukur", "main"] { assert!(jadi.contains(n), "{n} tidak dikompilasi:\n{}", h.stderr); }
}

/// J4 langkah 3: larik milik VM dibaca dan ditulis lewat jalur lambat — salin-saat-tulis, elemen campur,
/// indeks negatif, dan indeks di luar batas (kalimat VM).
const LARIK: &str = "fn isi(a, n) {\n  for i in 0..n { a[i] = a[i] * 2 + i }\n  return a\n}\nfn main() {\n  var a = [0; 2000]\n  for k in 0..30 {\n    for i in 0..2000 { a[i] = a[i] + i % 13 + k }\n  }\n  wok(a[1999], a[-1], a[0], len(a))\n  var b = a\n  b[0] = 77\n  wok(a[0], b[0])\n  var c = [1, 2.5, \"x\"]\n  for i in 0..2 { c[i] = c[i] }\n  wok(c)\n  let d = isi([1, 2, 3], 3)\n  wok(d)\n  var e = [5, 6]\n  for i in 0..3 { e[i] = i }\n}\n";

#[test]
fn larik_lewat_jalur_lambat() {
    sama_dengan_interpreter(LARIK);
    let i = jalankan(LARIK, &[], &[]);
    let j = jalankan(LARIK, &["--jit"], &[STRES, ACAK]);
    assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr));
    assert_eq!(i.stdout, "735 735 435 2000\n435 77\n[1, 2.5, x]\n[2, 5, 8]");
    let h = jalankan(LARIK, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    assert!(h.stderr.lines().any(|b| b.contains("dikompilasi:") && b.contains("main")), "{}", h.stderr);
}

// ── J4 langkah 4: jalur lambat umum (`lambat::langkah`, `dokumentasi/jit/j4-rencana.md`) ────────

const UMUM: &str = r#"fn gabung(xs) {
    var s = ""
    var n = 0
    for i in 0..len(xs) {
        let v = xs[i]
        if v { n = n + v % 7 }
        s = s + str(v % 3)
    }
    return s + ":" + str(n)
}
fn label(k) { return "k" + str(k) }
fn diam(n) { var t = 0; for i in 0..n { t = t + i } }
fn ambil(d, k) { return d[k] * 2 }
fn main() {
    var d = dict::new()
    for i in 0..400 { d[str(i % 13)] = i }
    var tot = 0
    for i in 0..400 { tot = tot + d[str(i % 13)] % 5 }
    wok(len(dict::keys(d)), tot)
    var xs = []
    for i in 0..40 { xs = xs + [i * 3] }
    for k in 0..6 { wok_(gabung(xs), " ") }
    wok("")
    var p = 0
    for i in 0..200 { p = p + len(label(i)) }
    wok(p, diam(50))
    var f = 0.5
    let a = [1.5, 2.5]
    for i in 0..300 { f = f + a[i % 2] * 2.0 }
    wok(f)
    var hit = 0
    let tambah = |x| { hit = hit + x }
    for i in 0..300 { tambah(i); hit = hit + 1 }
    wok(hit)
    var campur = [1, "a", 2.5, nil, [3]]
    var teks = []
    for i in 0..50 { let c = campur[i % 5]; if c { teks = teks + [typeof(c)] } }
    wok(len(teks), teks[0], teks[3])
    var u = 0
    for i in 0..300 { u = u + ambil(d, str(i % 13)) }
    wok(u)
    var g = 0
    for i in 0..300 { g = g + ambil(d, str(i % 20)) }
    wok(g)
}
"#;

#[test]
fn jalur_lambat_umum() {
    // Teks, dict, panggilan bawaan, truthiness nilai VM, sel tangkapan, `Return` nilai VM dan unit,
    // pecahan dibuka dari larik, dan galat dari instruksi yang dijalankan VM di dalam kode native.
    sama_dengan_interpreter(UMUM);
    let i = jalankan(UMUM, &[], &[]);
    let j = jalankan(UMUM, &["--jit"], &[STRES, ACAK]);
    assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr));
    let biasa = jalankan(UMUM, &["--jit"], &[]);
    assert_eq!((i.sukses, &i.stdout, &i.stderr), (biasa.sukses, &biasa.stdout, &biasa.stderr));
    assert!(!i.sukses && i.stderr.contains("jejak: `ambil` dipanggil"), "{}", i.stderr);
    assert!(i.stdout.starts_with("13 890\n") && i.stdout.ends_with("1200.5\n45150\n40 int array\n235794"), "{}", i.stdout);
    let h = jalankan(UMUM, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    assert!(h.stderr.lines().any(|b| b.contains("dikompilasi:") && b.contains("gabung")), "{}", h.stderr);
    // `ambil` terlalu pendek untuk untung, tapi mode stres tetap menjalankannya native (galat dari langkah).
    let h = jalankan(UMUM, &["--jit"], &[STRES, ("JRG_JIT_LOG", "ringkas")]);
    assert!(h.stderr.lines().any(|b| b.contains("dikompilasi:") && b.contains("ambil")), "{}", h.stderr);
}

// ── J4 langkah 6: semua opcode punya jalur (`dokumentasi/jit/j4-rencana.md`) ───────────────────────

const CAKUPAN: &str = r#"struct Kotak { pub xs: []int }
fn bagi(a, b) -> Result<int, string> {
    if b == 0 { return Err("nol") }
    Ok(a / b)
}
fn jumlah_bagi(xs) -> Result<int, string> {
    var t = 0
    for _, x in xs { t = t + bagi(100, x)? }
    Ok(t)
}
fn tunda(n) {
    defer wok("tunda", n)
    var s = 0
    for i in 0..n { s = s + i }
    s
}
fn tumbuh(n) {
    var a = []
    let berapa = || len(a)
    for i in 0..n { a = a + [i] }
    var b = [[1], [2]]
    for i in 0..3 { b[0] = b[0] + [i] }
    var k = Kotak { xs: [] }
    for i in 0..3 { k.xs = k.xs + [i] }
    wok(berapa(), b, k.xs)
}
fn pilih() {
    let c = make_chan(int, 3)
    let k = make_chan(int, 1)
    c <- 1
    // Satu lengan siap per putaran: interpreter dan VM memilih acak di antara yang siap.
    k <- 0
    var n = 0
    for i in 0..3 {
        select {
            case v := <-c: { n = n + v }
            case k <- i: { n = n + 10 }
            default: { n = n + 100 }
        }
    }
    n
}
fn koleksi() {
    var t = 0
    for i, x in [3, 4, 5] { t = t + i * x }
    for k, v in {"a": 1, "b": 2} { wok_(k, v, "") }
    for c in "abc" { t = t + c }
    let tutup = |x| x + t
    let m = [1, 2, 3].map(tutup).filter(|x| x % 2 == 0)
    let r = arr::reduce(m, |a, b| a + b, 0)
    p, q := (r, "dua")
    wok(t, m, r, p, q)
}
fn gagal() {
    defer wok("defer goroutine")
    let a = [1]
    wok(a[5])
}
pub fn main() {
    wok(jumlah_bagi([1, 2, 5]), jumlah_bagi([1, 0, 5]))
    wok(tunda(5))
    tumbuh(4)
    wok(pilih())
    koleksi()
    spawn { gagal() }
    time::sleep(0.05)
    wok("selesai")
}
"#;

#[test]
fn semua_opcode_punya_jalur() {
    // `?`, `defer`, tumbuh di tempat (`CobaTumbuh*`, `TumbuhIndexLokal`, `SamaTempat`), `select`, iterator
    // larik/dict/teks, closure yang menangkap lokal native, HOF, bongkar tuple, galat goroutine ber-`defer`.
    let i = jalankan(CAKUPAN, &[], &[]);
    assert!(i.sukses && i.stdout.contains("38 38 dua") && i.stderr.contains("goroutine error"), "{} {}", i.stdout, i.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(CAKUPAN, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    // Di mode stres tidak ada titik keluar selain `Return` fungsi ber-`defer` (VM yang menjalankan `defer`-nya).
    let h = jalankan(CAKUPAN, &["--jit"], &[STRES, ("JRG_JIT_LOG", "ringkas")]);
    let jadi = h.stderr.lines().find(|b| b.contains("dikompilasi:")).unwrap_or_default().to_string();
    for f in ["jumlah_bagi", "tunda", "tumbuh", "pilih", "koleksi"] { assert!(jadi.contains(f), "{f}: {}", h.stderr); }
    assert!(h.stderr.lines().filter(|b| b.starts_with("[jit]   keluar ")).all(|b| b.contains("keluar Return")), "{}", h.stderr);
}

// ── J4 langkah 7: panggilan langsung dan spekulasi hasil (`dokumentasi/jit/j4-rencana.md`) ─────────

const LANGSUNG: &str = r#"enum Op { Tambah(int), Kali(int), Nol }
struct Kotak { pub isi: int }
fn terap(op, v) {
    return match op {
        Op::Tambah(n) => v + n,
        Op::Kali(n) => (v * n) % 1000003,
        Op::Nol => 0,
    }
}
fn kadang_teks(i) {
    if i > 3000 { return "teks" }
    return i * 2
}
fn ambil(k) { return k.isi }
pub fn main() {
    let ops = [Op::Tambah(3), Op::Kali(7), Op::Tambah(11), Op::Nol]
    var v = 1
    for i in 0..5000 {
        v = terap(ops[i % 4], v)
        if v == 0 { v = 1 }
    }
    wok(v)
    var t = 0
    var terakhir = ""
    for i in 0..4000 {
        let h = kadang_teks(i)
        if typeof(h) == "int" { t = t + h } else { terakhir = h }
    }
    wok(t, terakhir)
    let ks = [Kotak { isi: 4 }, Kotak { isi: 5 }]
    var s = 0
    for i in 0..3000 { s = s + ambil(ks[i % 2]) + ks[i % 2].isi }
    wok(s)
    var campur = [1, 2, 3, 4.5]
    var c = 0.0
    for i in 0..3000 { c = c + campur[i % 4] }
    wok(c)
}
"#;

#[test]
fn panggilan_langsung_dan_spekulasi_hasil() {
    // Argumen enum/struct ke fungsi native (`Cara::Langsung`), hasil bulat dari umpan profil, hasil yang
    // berganti jenis sesudah pemanasan (spekulasinya gagal sekali lalu tidak diulang), larik campur.
    let i = jalankan(LANGSUNG, &[], &[]);
    assert!(i.sukses && i.stdout == "1\n9003000 teks\n27000\n7875.0", "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(LANGSUNG, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let h = jalankan(LANGSUNG, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    let jadi = h.stderr.lines().find(|b| b.contains("dikompilasi:")).unwrap_or_default().to_string();
    for f in ["main", "terap", "ambil"] { assert!(jadi.contains(f), "{f}: {}", h.stderr); }
    // Deopt cuma di titik yang memang belajar, bukan tiap putaran.
    let deopt: u64 = h.stderr.lines().find_map(|b| b.strip_prefix("[jit] deopt: ")).map_or(0, |n| n.trim().parse().unwrap());
    assert!(deopt < 10, "{}", h.stderr);
}

// ── J4 langkah 9: operasi bit native dan upvalue (`dokumentasi/jit/j4-rencana.md`) ─────────────────

const BIT: &str = r#"fn hash(s) {
    var h = 2166136261
    for i in 0..len(s) {
        h = (h ^ 7) * 16777619
        h = h & 4294967295
    }
    h
}
fn geser(x, k) { return x << k }
fn kanan(x, k) { return x >> k }
pub fn main() {
    var t = 0
    for i in 0..2000 { t = t ^ (i << 3) | (i >> 1) & 255 }
    wok(t, hash("abcdef"), ~5, 12 &^ 10)
    var g = 0
    for i in 0..100 { g = g + geser(i, 2) + kanan(-i, 70) + kanan(i * 1000, 63) }
    wok(g)
    wok(geser(1, 63), geser(3, 62), geser(-1, 63), geser(1, 100), kanan(-8, 127))
    wok(geser(1, 128))
}
"#;

#[test]
fn operasi_bit_native() {
    // `& | ^ &^ ~` atas bulat 64 bit, `<<` yang hasilnya tidak muat dan geseran di luar batas jatuh ke VM
    // (128 bit, kalimat galatnya milik VM).
    let i = jalankan(BIT, &[], &[]);
    assert!(!i.sukses && i.stdout.contains("9223372036854775808 13835058055282163712") && i.stderr.contains("1 << 128"), "{} {}", i.stdout, i.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BIT, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

// ── J4 langkah 10: titik temu dilebarkan (`dokumentasi/jit/j4-rencana.md`) ─────────────────────────

const TITIK_TEMU: &str = r#"fn kelas(v: int) -> int {
    if typeof(v) == "int" { return v % 7 }
    return 0
}
pub fn main() {
    var t = 0
    var u = 0
    for i in 0..3000 {
        var x = i
        if i % 3 == 0 { x = "tiga" }
        if typeof(x) == "int" { t = t + kelas(x) } else { u = u + len(x) }
    }
    wok(t, u)
}
"#;

#[test]
fn titik_temu_dilebarkan() {
    // Lokal yang bulat di satu cabang dan teks di cabang lain bertemu sebagai `Nilai` (tepinya menulis angkanya
    // ke stack VM), dan callee yang punya `typeof` dipanggil langsung, bukan callee bulat yang deopt tiap kali.
    let i = jalankan(TITIK_TEMU, &[], &[]);
    assert!(i.sukses && i.stdout == "5995 4000", "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(TITIK_TEMU, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let h = jalankan(TITIK_TEMU, &["--jit"], &[("JRG_JIT_LOG", "ringkas")]);
    assert!(h.stderr.lines().filter(|b| b.starts_with("[jit]   keluar ")).all(|b| b.contains("keluar Return")), "{}", h.stderr);
}

// ── J5: bulat 128 bit native, `math::wrap_*` intrinsik (`dokumentasi/jit/j5-rencana.md`) ───────────

const LEBAR: &str = r#"fn fnv(h, s) {
    var acc = h
    for i in 0..len(s) {
        acc = math::wrap_mul(acc ^ (str::char_at(s, i) as uint), 1099511628211)
    }
    return acc
}
fn campur(x) {
    var a = x
    for i in 0..50 {
        a = math::wrap_add(a, 18446744073709551557) ^ (a >> 7)
        a = math::wrap_sub(a, i * 3) | (a & 255)
        if a > 9223372036854775807 { a = a >> 1 }
    }
    a
}
pub fn main() {
    var h = 14695981039346656037
    for r in 0..300 { h = fnv(h, "item-" + str(r)) }
    wok(h)
    wok(campur(1), campur(-5), campur(170141183460469231731687303715884105727))
    let besar = 170141183460469231731687303715884105727
    wok(besar >> 127, besar >> 64, (besar ^ 1) & 3, ~besar, besar == besar, besar < 1)
    wok(math::wrap_mul(-1, -1), math::wrap_add(-1, 1), math::wrap_sub(0, 1))
    var g = 18446744073709551615
    for i in 0..3 { g = g << 1 }
    wok(g)
    wok(besar << 128)
}
"#;

/// Hash 64 bit tak bertanda dalam loop: lokal yang di satu putaran bulat 64 bit dan di putaran lain i128.
/// Dulu sasaran titik temu yang cuma didatangi jatuh-terus tidak menerima konversi tepinya: jawaban salah.
const LEBAR_LOOP: &str = r#"fn f(a) {
    var acc = a
    for i in 0..2 { acc = math::wrap_mul(acc ^ 97, 3) }
    return acc
}
pub fn main() {
    var h = 14695981039346656037
    for r in 0..30 { h = f(h); wok_(h % 1000, "") }
    wok("")
}
"#;

/// Jumlah/selisih yang melewati 2^63 di loop (jaga luapan 64 bit belajar ke 128 bit), perkalian, dan luapan
/// 128 bit yang harus berakhir dengan kalimat galat VM.
const LEBAR_ARITMETIKA: &str = r#"fn jumlah(n) {
    var t = 0
    for i in 0..n { t = t + 4611686018427387904 }
    t
}
fn kurang(n) {
    var t = 0
    for i in 0..n { t = t - 4611686018427387904 - i }
    t
}
fn kali(n) {
    var p = 1
    for i in 1..n { p = p * 3 }
    p
}
fn campur(n) {
    var a = 9223372036854775807
    var b = 0
    for i in 0..n { a = a + i; b = b + a * 2 - 1; if b > 100000000000000000000 { b = b - 99999999999999999999 } }
    wok(a, b)
}
fn meledak(n) {
    var t = 85070591730234615865843651857942052864
    for i in 0..n { t = t + t }
    t
}
pub fn main() {
    wok(jumlah(20), kurang(20), kali(60))
    campur(3000)
    wok(meledak(5))
}
"#;

#[test]
fn bulat_lebar_native() {
    for src in [LEBAR, LEBAR_LOOP, LEBAR_ARITMETIKA] {
        let i = jalankan(src, &[], &[]);
        for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
            let j = jalankan(src, &["--jit"], env);
            assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
        }
    }
    let i = jalankan(LEBAR, &[], &[]);
    assert!(i.stdout.starts_with("7476383727222582879\n") && i.stderr.contains("<< 128"), "{} {}", i.stdout, i.stderr);
}

/// `LoadLocal` + `GetField` digabung (`lambat::field_lokal`): angka langsung ke register, jenis field
/// yang berganti, lokal tertangkap closure, dict, penerima bukan struct, field privat.
const FIELD_LOKAL: &str = r#"struct T { pub a: int, pub b: string, c: float }
impl T {
    fn jumlah(self) -> float { self.c + self.a as float }
}
fn baca(t) { t.a + 1 }
fn campur(t) { t.b }
fn kunci(d) { d.k * 2 }
pub fn main() {
    var s = 0
    for i in 0..3000 { s = s + baca(T { a: i, b: i, c: 0.5 }) }
    wok(s)
    var u = []
    for i in 0..3000 { u = arr::push(u, campur(T { a: 1, b: i % 7 == 0 $ "teks" : (i % 5 == 0 $ 2.5 : i), c: 1.0 })) }
    wok(len(u), u[0], u[5], u[7], u[3])
    var f = 0.0
    for i in 0..3000 { f = f + T { a: i, b: 0, c: 0.25 }.jumlah() }
    wok(f)
    var k = 0
    for i in 0..3000 { k = k + kunci({"k": i}) }
    wok(k)
    let t = T { a: 5, b: 6, c: 1.0 }
    let g = || t.a + t.b
    var z = 0
    for i in 0..3000 { z = z + g() }
    wok(z)
    var n = 0
    for i in 0..3000 { n = n + baca(T { a: i, b: 0, c: 0.0 }) + len("ab") }
    wok(n)
    for i in 0..3000 { if i == 2999 { wok(baca("bukan struct")) } else { baca(T { a: 1, b: 1, c: 1.0 }) } }
}
"#;

const FIELD_PRIVAT: &str = r#"struct R { rahasia: int, pub x: int }
fn intip(r) { r.rahasia }
pub fn main() {
    var s = 0
    for i in 0..3000 { s = s + R { rahasia: 1, x: i }.x }
    wok(s)
    wok(intip(R { rahasia: 7, x: 1 }))
}
"#;

/// Satu situs `o.x` atas bentuk berbeda (urutan field lain, bentuk yang bertambah nama, dict): tembolok
/// bentuknya monomorfik, yang lain dicari ulang.
const FIELD_POLIMORF: &str = r#"struct A { pub x: int, pub y: int }
struct B { pub y: int, pub z: int, pub x: int }
fn ambil(o) { o.x * 10 + o.y }
pub fn main() {
    var t = 0
    for i in 0..4000 {
        t = t + ambil(A { x: i, y: 1 })
        if i % 3 == 0 { t = t + ambil(B { y: 2, z: 9, x: i }) }
        if i % 7 == 0 { t = t + ambil(json::merge(dict { w: 5 }, A { x: 1, y: i })) }
        if i % 11 == 0 { t = t + ambil({"x": 3, "y": 4}) }
    }
    wok(t)
    let c = B { x: 7, y: 8, z: 9 }
    let f = || ambil(c)
    var u = 0
    for i in 0..3000 { u = u + f() }
    wok(u)
}
"#;

#[test]
fn field_lokal_digabung() {
    for src in [FIELD_LOKAL, FIELD_PRIVAT, FIELD_POLIMORF] {
        let i = jalankan(src, &[], &[]);
        for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
            let j = jalankan(src, &["--jit"], env);
            assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
        }
    }
    let i = jalankan(FIELD_LOKAL, &[], &[]);
    assert!(i.stdout.starts_with("4501500\n3000 teks 2.5 teks 3\n") && i.stderr.contains("bernama `a`"), "{}", i.stdout);
}

/// Pemeriksaan jenis pola (`Typeof; Const; Eq`) atas angka/bool native dilipat (`Cara::TipeLipat`); pola
/// literal dan rentang atas bulat, pecahan, bool, rune dan teks di satu `match`.
const TIPE_LIPAT: &str = r#"fn kelas(v) {
    match v {
        0 => 5,
        1 | 2 | 3 => 7,
        4..=8 => 11,
        1.5..=2.5 => 17,
        true => 19,
        'a'..='z' => 23,
        _ => 13,
    }
}
fn bilang(x) { match x % 11 { 0 => 1, 4..=8 => 2, _ => 3 } }
fn pecah(x) { match x * 0.5 { 0.0..=2.0 => 1, _ => 2 } }
pub fn main() {
    var t = 0
    for i in 0..5000 { t = t + bilang(i) + pecah(i as float) }
    wok(t)
    var u = 0
    for i in 0..3000 { u = u + kelas(i % 12) }
    wok(u)
    for _, v in [0, 3, 5, 9, 2.0, 7.0, true, false, 'q', "s", 1.0] { wok_(kelas(v), " ") }
    wok("")
    var w = 0
    for i in 0..4000 { let v = i % 4 == 0 $ 2.0 : (i % 4 == 1 $ true : (i % 4 == 2 $ 5 : "x")); w = w + campur(v) }
    wok(w)
}
fn campur(v) { match v { 4..=8 => 1, 1.5..=2.5 => 2, true => 3, _ => 4 } }
"#;

#[test]
fn tipe_pola_dilipat() {
    let i = jalankan(TIPE_LIPAT, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(TIPE_LIPAT, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.contains("5  7  11  13  7  11  19  13  23  13  7"), "{}", i.stdout);
}

/// Spekulasi yang gagal di callee yang dipanggil langsung: pemanggil (loop di `main`, tidak pernah masuk
/// ulang) dulu terus memakai salinan callee yang basi — 2 946 deopt, satu per panggilan (`string_scan` kecil).
const CALLEE_BASI: &str = r#"
package main

// Microbenchmark for the by-index string scan path (`str::char_at` in a `for
// i in 0..len(s)` loop) — the only way to visit characters by position …

fn fnv(h: uint, s: str) -> uint {
    var acc: uint = h
    for i in 0..len(s) {
        acc = math::wrap_mul(acc ^ (str::char_at(s, i) as uint), 1099511628211)
    }
    return acc
}

pub fn main() {
    // Three lengths, ~4M characters scanned each. A quadratic char_at makes
    // the long-string pass dominate; a linear one keeps all three close.
    let short: str = "abcdefghijklmnopqrst"
    let medium: str = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzab"
    // Multi-byte on purpose: guards against anyone "optimizing" this into
    // byte indexing, which would silently corrupt non-ASCII text.
    let uni: str = "héllo wörld ☃ 𝔘 çà mixed widths here ☃☃ 𝔘𝔘 ééé"

    var hash: uint = 1469598103934665603

    for r in 0..4000 {
        hash = fnv(hash, short)
    }
    for r in 0..1000 {
        hash = fnv(hash, medium)
    }
    for r in 0..1800 {
        hash = fnv(hash, uni)
    }

    wok(hash)
}
"#;

#[test]
fn callee_basi_tidak_dipakai_lagi() {
    let i = jalankan(CALLEE_BASI, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(CALLEE_BASI, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(CALLEE_BASI, &["--jit"], &[("JRG_JIT_LOG", "1")]);
    let deopt: usize = j.stderr.lines().find_map(|l| l.strip_prefix("[jit] deopt: ")).and_then(|n| n.trim().parse().ok()).unwrap_or(0);
    assert!(deopt < 20, "deopt {deopt} kali (dulu satu per panggilan):\n{}", j.stderr);
}

/// `Jenis::Rune` dari intrinsik `str::char_at`: disimpan, dibandingkan, dicetak, dikonversi `as int`/`uint`,
/// diserahkan dan dikembalikan, masuk larik, bertemu bulat di titik temu; indeks negatif dan di luar batas
/// lewat jalur umum.
const RUNE_NATIVE: &str = r#"fn huruf(s, i) { str::char_at(s, i) }
fn sama(a, b) { a == b }
fn kode(s, i) { str::char_at(s, i) as int }
pub fn main() {
    let t = "aé☃𝔘bc"
    var n = 0
    var akhir = ' '
    var kumpul = []
    for r in 0..2000 {
        for i in 0..len(t) {
            let c = str::char_at(t, i)
            n = n + (c as uint) + kode(t, i)
            if c == 'b' { n = n + 1 }
            akhir = c
            if r == 0 { kumpul = arr::push(kumpul, c) }
        }
        let x = r % 2 == 0 $ str::char_at(t, 2) : 7
        n = n + typeof(x).len()
    }
    wok(n, akhir, kumpul, huruf(t, -1), sama(huruf(t, 1), 'é'))
    wok(str::char_at("xyz", -2), typeof(str::char_at("a", 0)))
    var m = 0
    for i in 0..3000 { m = m + (str::char_at(i % 2 == 0 $ "ab" : "☃☃", i % 2) as int) }
    wok(m)
    wok(str::char_at(t, 99))
}
"#;

#[test]
fn rune_native() {
    let i = jalankan(RUNE_NATIVE, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(RUNE_NATIVE, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.starts_with("521393000 c [a, é, ☃, 𝔘, b, c] c true\n") && i.stderr.contains("index 99"), "{}", i.stdout);
}

/// `Cara::Bangun`: struct/larik/tuple/dict literal dari angka native (bulat, pecahan, `Lebar`) dan nilai VM.
const BANGUN: &str = r#"struct P { pub x: float, pub n: int, pub s: string, pub h: uint }
pub fn main() {
    var t = 0
    var akhir = P { x: 0.0, n: 0, s: "", h: 0 }
    var kumpul = []
    for i in 0..3000 {
        let h: uint = math::wrap_mul(i as uint, 11400714819323198485)
        let p = P { x: i as float * 0.5, n: i, s: «k${i % 3}», h: h }
        let a = [i, i * 2, p.x, p.s]
        let u = (i, p.h, "t")
        t = t + p.n + a[1] + len(a) + u.0
        akhir = p
        if i % 1000 == 0 { kumpul = arr::push(kumpul, (a, u)) }
        let d = dict { a: i, b: p.s }
        t = t + d.a
    }
    wok(t, akhir, kumpul)
}
"#;

#[test]
fn bangun_dari_angka_native() {
    let i = jalankan(BANGUN, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BANGUN, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.starts_with("22504500 P {h: 8926974566473112067"), "{}", i.stdout);
}

/// Fungsi bawaan sebagai konstanta maya (tidak ditulis ke stack VM): disimpan, diserahkan, dikembalikan, masuk
/// larik, bertemu fungsi lain di titik temu, dipanggil lewat variabel, dibandingkan (galat), intrinsik.
const KONSTANTA_MAYA: &str = r#"fn terap(f, x) { f(x) }
fn pilih(i) { i % 2 == 0 $ str::upper : str::lower }
pub fn main() {
    var n = 0
    var akhir = len
    var fs = []
    for i in 0..3000 {
        let f = i % 3 == 0 $ str::upper : str::reverse
        let g = len
        n = n + g(f("abc")) + len(str::upper("xy")) + (str::char_at("hé", i % 2) as int)
        n = n + terap(len, [1, 2, 3]) + len(pilih(i)("Ab"))
        if i % 1000 == 0 { fs = arr::push(fs, f) }
        akhir = i % 2 == 0 $ len : typeof
        n = n + (math::wrap_mul(i as uint, 3) as int) + (math::abs(-i) % 7)
        let k = [str::upper, str::lower][i % 2]
        n = n + len(k("q"))
    }
    wok(n, fs, akhir, akhir([1]), typeof(str::upper))
    wok(len == len, arr::map(["a", "b"], str::upper))
}
"#;

#[test]
fn konstanta_maya() {
    let i = jalankan(KONSTANTA_MAYA, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(KONSTANTA_MAYA, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.starts_with("14042994 [<native fn>") && i.stderr.contains("native_fn"), "{}", i.stdout);
}

/// Modul `std/` lewat JIT (mode stres) sama dengan `--vm`: korpus inilah yang menangkap salinan maya di tepi
/// titik temu (panik "tepi menulis angka native") yang tidak dimiliki tabel PARITAS.
#[test]
fn uji_std_lewat_jit_sama_dengan_vm() {
    let akar = env!("CARGO_MANIFEST_DIR");
    let mut berkas: Vec<std::path::PathBuf> = std::fs::read_dir(std::path::Path::new(akar).join("stdlib/uji")).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jrg"))
        .collect();
    berkas.sort();
    assert!(berkas.len() >= 23, "berkas uji std tinggal {}", berkas.len());
    let jalan = |f: &std::path::Path, mode: &[(&str, &str)]| {
        let mut c = Command::new(jrgc());
        c.arg(f).arg("--uji").arg("--sopan").arg(if mode.is_empty() { "--vm" } else { "--jit" });
        for (k, v) in mode { c.env(k, v); }
        let o = c.env("JRG_SOPAN", "1").stdin(std::process::Stdio::null()).output().unwrap();
        (o.status.code(), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
    };
    for f in &berkas {
        let v = jalan(f, &[]);
        // Deopt acak menemukan `BUKA_SUDAH` yang terbawa ke rekaman `ip + 1` (`uji_tanggal`: jawaban salah).
        for mode in [&[STRES][..], &[STRES, ACAK][..]] {
            assert_eq!(v, jalan(f, mode), "{} beda antara --vm dan --jit {mode:?}", f.display());
        }
    }
}

/// Callee native yang deopt di dalam panggilan berargumen bukaan (`Call` + `Buka`), dengan `Buka` lagi di
/// instruksi sesudahnya: rekaman pemanggil di `ip + 1` dulu mewarisi `BUKA_SUDAH` dan deopt membaca posisi
/// milik VM sebagai angka dari slot yang tak pernah ditulis (nilai mirip alamat memori).
const BUKA_TERBAWA: &str = r#"struct W { pub tahun: int, pub bulan: int, pub hari: int }
fn sipil(t0, b, h) {
    let t = b <= 2 $ t0 - 1 : t0
    let era = t / 400
    let yoe = t - era * 400
    let doy = (153 * (b > 2 $ b - 3 : b + 9) + 2) / 5 + h - 1
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
    return era * 146097 + doe - 719468
}
fn hari_ke(w) { return sipil(w.tahun, w.bulan, w.hari) - sipil(w.tahun, 1, 1) + 1 }
pub fn main() {
    for i in 0..6 { wok_(hari_ke(W { tahun: 2023, bulan: 1, hari: 1 }), " ") }
    wok("")
}
"#;

#[test]
fn bendera_bukaan_tidak_terbawa_ke_rekaman_sesudahnya() {
    let i = jalankan(BUKA_TERBAWA, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BUKA_TERBAWA, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert_eq!(i.stdout.trim_end(), "1  1  1  1  1  1");
}

/// Elemen larik pecahan lewat `IndeksLokal` (umpan VM, `lambat::baca_pecahan`), larik yang elemennya
/// berganti jenis di tengah jalan (spekulasi meleset → VM).
const ELEMEN_PECAHAN: &str = r#"pub fn main() {
    var s: [16]f64
    var c: [16]int
    for i in 0..16 { s[i] = 0.0; c[i] = 0 }
    var total = 0.0
    for r in 0..5000 {
        let b = r % 16
        s[b] = s[b] + (r as float) * 0.5
        c[b] = c[b] + 1
        total = total + s[(b + 1) % 16]
    }
    wok(total, s[3], c[3])
    var m = [1.5, 2.5, 3.5]
    var t = 0.0
    for r in 0..3000 {
        if r == 2000 { m[1] = 7 }
        let v = m[r % 3]
        t = t + v
    }
    wok(t, m)
}
"#;

#[test]
fn elemen_larik_pecahan() {
    let i = jalankan(ELEMEN_PECAHAN, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(ELEMEN_PECAHAN, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.contains("8998.5 [1.5, 7, 3.5]"), "{}", i.stdout);
}

/// `a[i] = a[i] + x` (`TumbuhIndexLokal`) atas angka native langsung jatuh ke `Add` biasa; teks dan larik
/// tetap lewat sambungan di tempat milik VM; elemen di atas 2^63.
const TUMBUH_ANGKA: &str = r#"pub fn main() {
    var c = [0, 0, 0]
    var f = [0.5, 0.5]
    var s = ["a", "b"]
    var l = [[1], [2]]
    for i in 0..3000 {
        c[i % 3] = c[i % 3] + 1
        f[i % 2] = f[i % 2] + 0.25
        if i % 500 == 0 {
            s[i % 2] = s[i % 2] + "x"
            s[1] = s[1] + 7
            l[i % 2] = l[i % 2] + [i]
        }
    }
    wok(c, f, s, l)
    var m = [1, 2]
    for i in 0..2000 { m[i % 2] = m[i % 2] + (i % 2 == 0 $ 1 : 2) }
    wok(m)
    var big = [9223372036854775000]
    for i in 0..2000 { big[0] = big[0] + 1 }
    wok(big)
}
"#;

#[test]
fn tumbuh_index_atas_angka() {
    let i = jalankan(TUMBUH_ANGKA, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(TUMBUH_ANGKA, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.contains("[1000, 1000, 1000] [375.5, 375.5] [axxxxxx, b777777]"), "{}", i.stdout);
}

/// Tembolok pola enum per situs (`lambat::SitusPola`): varian bernama sama dari dua enum, varian unit lawan
/// berpayload, `Option`/`Result` bawaan, nilai enum lain di situs yang tidak mengenalnya.
const POLA_ENUM: &str = r#"enum A { X(int), Y, Z(int, int) }
enum B { X(int), Y }
fn a(v) { match v { A::X(n) => n, A::Y => 100, A::Z(p, q) => p * q } }
fn b(v) { match v { B::X(n) => n + 1, B::Y => 7 } }
fn o(v) { match v { Some(n) => n, None => 0 } }
fn r(v) { match v { Ok(n) => n, Err(e) => len(e) } }
pub fn main() {
    var t = 0
    for i in 0..3000 {
        let va = i % 3 == 0 $ A::X(i) : (i % 3 == 1 $ A::Y : A::Z(i, 2))
        t = t + a(va) + b(i % 2 == 0 $ B::X(i) : B::Y)
        t = t + o(i % 4 == 0 $ None : Some(i)) + r(i % 5 == 0 $ Err("ab") : Ok(i))
    }
    wok(t)
    wok(a(B::X(1)))
}
"#;

#[test]
fn tembolok_pola_enum() {
    let i = jalankan(POLA_ENUM, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(POLA_ENUM, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.starts_with("13836200") && i.stderr.contains("B::X(1)"), "{}", i.stdout);
}

/// Pemeriksaan jenis pola atas nilai milik VM (`Cara::TipeSama`) dan `len` atas nilai milik VM
/// (`Cara::Panjang`): payload tuple/teks/struct, teks berhuruf lebar, larik, dict, galat `len` atas bulat.
const JENIS_PANJANG_VM: &str = r#"struct T { pub a: int }
enum E { P(int, int), Q(string), R(T), S }
fn f(v) {
    match v {
        E::P(x, y) => x + y,
        E::Q(s) => len(s),
        E::R(t) => t.a,
        E::S => 0,
    }
}
fn ukur(x) { len(x) }
pub fn main() {
    var n = 0
    for i in 0..3000 {
        let v = i % 4 == 0 $ E::P(i, 1) : (i % 4 == 1 $ E::Q("héllo") : (i % 4 == 2 $ E::R(T { a: i }) : E::S))
        n = n + f(v) + ukur(i % 3 == 0 $ "ab☃" : (i % 3 == 1 $ [1, 2, 3, 4] : dict { a: 1 }))
    }
    wok(n)
    wok(ukur(5))
}
"#;

#[test]
fn jenis_dan_panjang_nilai_vm() {
    let i = jalankan(JENIS_PANJANG_VM, &[], &[]);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(JENIS_PANJANG_VM, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    assert!(i.stdout.starts_with("2261000") && i.stderr.contains("tidak bisa ukur `int`"), "{}", i.stdout);
}

/// Deopt dari bukaan `Lebar` yang gagal (`PUNCAK_VM`): puncaknya nilai milik VM dan tidak boleh ditimpa
/// isi 128 bit native. Dulu `b.uji(2)` menjawab `0` di ambang bawaan, bukan `bb` (jawaban salah tanpa suara).
const DEOPT_LEBAR_PUNCAK: &str = r#"struct H { pub isi: []int }
impl H {
    fn bobot(self, x) { return x }
    fn uji(self, i) { return self.bobot(self.isi[i]) }
}
pub fn main() {
    let a = H { isi: [5, 3, 8] }
    var t = 0
    for j in 0..50 { t = t + a.uji(j % 3) }
    wok(t)
    let b = H { isi: ["ccc", "a", "bb"] }
    wok(b.uji(1), b.uji(2), b.uji(0), b.uji(2))
}
"#;

#[test]
fn deopt_lebar_tidak_menimpa_puncak_vm() {
    let i = jalankan(DEOPT_LEBAR_PUNCAK, &[], &[]);
    assert_eq!(i.stdout, "264\na bb ccc bb");
    for env in [&[][..], &[STRES][..], &[STRES, ACAK][..]] {
        let j = jalankan(DEOPT_LEBAR_PUNCAK, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout), (j.sukses, &j.stdout), "{env:?}");
    }
}

/// Cabang `match` yang belum dijalankan VM sebelum kompilasi (tanpa umpan): payload-nya dicoba bulat sekali,
/// dan kalau ternyata bukan bulat dideopt lalu tidak diulang.
const CABANG_TANPA_UMPAN: &str = r#"enum Op { P(int), A(int) }
fn ex(o: Op, acc: int) -> int {
    match o {
        Op::P(k) => k,
        Op::A(k) => acc + k,
    }
}
pub fn main() {
    var acc = 1
    for i in 0..5000 {
        let o = i % 2 == 0 $ Op::P(i % 97) : Op::A(i % 31)
        acc = ex(o, acc) % 1000
    }
    wok(acc)
}
"#;

#[test]
fn cabang_tanpa_umpan_dispekulasikan_bulat() {
    let i = jalankan(CABANG_TANPA_UMPAN, &[], &[]);
    let v = jalankan(&CABANG_TANPA_UMPAN.replace("Op::A(i % 31)", "Op::A(i % 31)\n        if i == 4000 { wok(ex(Op::A(\"x\"), 1)) }"), &[], &[]);
    assert_eq!(v.stdout, "1x\n59", "payload teks di cabang yang dispekulasikan bulat");
    for env in [&[][..], &[STRES][..], &[STRES, ACAK][..]] {
        let j = jalankan(CABANG_TANPA_UMPAN, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout), (j.sukses, &j.stdout), "{env:?}");
        let w = jalankan(&CABANG_TANPA_UMPAN.replace("Op::A(i % 31)", "Op::A(i % 31)\n        if i == 4000 { wok(ex(Op::A(\"x\"), 1)) }"), &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (w.sukses, &w.stdout, &w.stderr), "{env:?}");
    }
}

/// `BuildEnum` lewat `lambat::bangun` (`SitusBangun::Varian`, `VmValue::varian_berisi` sama dengan VM): payload
/// bulat, pecahan, bool, teks, tuple, rune, larik, bulat di atas 2^63.
const BANGUN_VARIAN: &str = r#"enum V { I(int), F(float), B(bool), S(string), T((int, int)), R(rune), A([]int) }
fn buat(i) {
    let k = i % 7
    if k == 0 { return V::I(i * 3) }
    if k == 1 { return V::F(i as float / 2.0) }
    if k == 2 { return V::B(i % 2 == 0) }
    if k == 3 { return V::S("s" + str(i)) }
    if k == 4 { return V::T((i, i + 1)) }
    if k == 5 { return V::R(str::char_at("héllo", i % 5)) }
    return V::A([i, i])
}
fn besar(i) { return V::I(9223372036854775807 + i) }
fn nilai(v) {
    match v {
        V::I(n) => n % 1000,
        V::F(x) => (x as int) % 1000,
        V::B(b) => b $ 1 : 2,
        V::S(s) => len(s),
        V::T(p) => p.0 + p.1,
        V::R(r) => int(r) % 100,
        V::A(a) => len(a),
    }
}
pub fn main() {
    var t = 0
    for i in 0..3000 { t = t + nilai(buat(i)) }
    wok(t)
    wok(besar(5), nilai(besar(2)))
    wok(buat(3), buat(4), buat(5), buat(1))
}
"#;

#[test]
fn bangun_varian_dari_angka_native() {
    let i = jalankan(BANGUN_VARIAN, &[], &[]);
    assert!(i.stdout.starts_with("1684447\nV::I(9223372036854775812) 809"), "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BANGUN_VARIAN, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Bacaan field lokal yang sama di satu blok dipakai ulang (`turun::cse_aman`): lokal yang ditulis lewat
/// field, ditugasi ulang, diubah closure lewat panggilan, atau di loop harus dibaca lagi.
const CSE_FIELD: &str = r#"struct P { pub x: int, pub y: float, z: int }
impl P {
    fn dua(self) -> int { return self.x + self.x + self.z + self.z }
    fn geser(self) -> P { return P { x: self.x + 1, y: self.y * 2.0, z: self.z } }
}
fn ubah_tulis(p: P) -> int {
    var q = p
    let a = q.x
    q.x = q.x + 10
    let b = q.x
    return a * 1000 + b + q.x
}
fn ganti(p: P) -> int {
    var q = p
    let a = q.x + q.x
    q = P { x: 7, y: 1.0, z: 0 }
    return a + q.x + q.x
}
fn lewat_closure(p: P) -> int {
    var q = p
    let f = || { q = P { x: 100, y: 0.0, z: 0 } }
    let a = q.x
    f()
    return a + q.x
}
fn dalam_loop(p: P) -> int {
    var q = p
    var t = 0
    for i in 0..5 { t = t + q.x; q.x = q.x + i }
    return t + q.x
}
pub fn main() {
    var t = 0
    var u = 0.0
    for i in 0..2000 {
        let p = P { x: i, y: i as float, z: 3 }
        t = t + p.dua() + ubah_tulis(p) + ganti(p) + lewat_closure(p) + dalam_loop(p)
        u = u + p.geser().y + p.y + p.y
    }
    wok(t, u)
    wok(ubah_tulis(P { x: 5, y: 0.0, z: 1 }), ganti(P { x: 5, y: 0.0, z: 1 }), lewat_closure(P { x: 5, y: 0.0, z: 1 }), dalam_loop(P { x: 5, y: 0.0, z: 1 }))
}
"#;

#[test]
fn bacaan_field_dipakai_ulang_cuma_kalau_lokalnya_tetap() {
    let i = jalankan(CSE_FIELD, &[], &[]);
    assert_eq!(i.stdout, "2025307000 7996000.0\n5030 24 105 50");
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(CSE_FIELD, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Field angka lain lokal yang sama dibaca sekali kunci (`lambat::field_lokal_banyak`): field yang bukan
/// angka atau berganti jenis antar panggilan lewat jalur biasa, field privat, dict, field yang hilang.
const FIELD_BANYAK: &str = r#"struct Q<T> { pub a: int, pub b: T, pub c: float, pub d: int, k: int }
impl<T> Q<T> {
    fn jumlah(self) -> int {
        let x = self.a + self.d
        let y = self.c * 2.0
        return x + self.d + (y as int) + self.a + self.k
    }
    fn campur(self) -> string { return str(self.a) + str(self.b) + str(self.d) }
}
fn pakai(q) { return q.a * 3 + q.d + q.a }
pub fn main() {
    var t = 0
    var s = ""
    for i in 0..3000 {
        let q = Q { a: i, b: i % 5, c: 1.5, d: 2, k: 1 }
        t = t + q.jumlah() + pakai(q)
        if i % 1000 == 999 { s = s + q.campur() }
    }
    let r = Q { a: 1, b: "teks", c: 2.5, d: 4, k: 0 }
    wok(t, s, r.jumlah(), r.campur(), pakai(r))
    let w = dict { a: 5, d: "x" }
    wok(pakai(dict { a: 5, d: 7 }))
    wok(pakai(w))
}
"#;

#[test]
fn field_lokal_dibaca_bersama() {
    let i = jalankan(FIELD_BANYAK, &[], &[]);
    assert!(i.stdout.starts_with("27021000 99942199942299942 15 1teks4 8\n27\n15x5"), "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(FIELD_BANYAK, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// `Dup; GetMethod; Swap` atas penerima milik VM lewat satu pembantu (`lambat::metode_tukar`): situs yang
/// berganti tipe penerima (deopt di `GetMethod`), dan penerima tanpa method itu (galat dari VM).
const METODE_TUKAR: &str = r#"struct A { pub n: int }
struct B { pub n: int }
impl A { fn m(self, k) -> A { return A { n: self.n + k } } fn v(self) -> int { return self.n } }
impl B { fn m(self, k) -> B { return B { n: self.n * 2 + k } } fn v(self) -> int { return self.n + 1 } }
fn jalan(x, k) { var y = x
    for i in 0..k { y = y.m(i) }
    return y.v() }
pub fn main() {
    wok(jalan(A { n: 1 }, 100), jalan(A { n: 2 }, 50))
    wok(jalan(B { n: 1 }, 20), jalan(A { n: 3 }, 10))
    var a = A { n: 0 }
    for i in 0..2000 { a = a.m(1) }
    wok(a.v())
    let d = dict { n: 4 }
    wok(jalan(d, 3))
}
"#;

#[test]
fn metode_dan_penerima_dalam_satu_pembantu() {
    let i = jalankan(METODE_TUKAR, &[], &[]);
    assert!(i.stdout.starts_with("4951 1227\n2097132 48\n2000") && i.stderr.contains("bernama `m`"), "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(METODE_TUKAR, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// `LoadLocal`/`AmbilLokal` + `Dup; GetMethod; Swap` (`lambat::metode_lokal`): lokal yang dipindah ke
/// argumen, dibaca, dan yang tertangkap closure (sel tidak boleh dikosongkan).
const METODE_LOKAL: &str = r#"struct A { pub n: int }
struct B { pub n: int }
impl A { fn m(self, k) -> A { return A { n: self.n + k } } fn v(self) -> int { return self.n } }
impl B { fn m(self, k) -> B { return B { n: self.n * 2 + k } } fn v(self) -> int { return self.n + 1 } }
fn tangkap() {
    var a = A { n: 1 }
    let f = || { a = A { n: a.n * 10 } }
    for i in 0..5 { a = a.m(i); f() }
    return a.v()
}
pub fn main() {
    var x = A { n: 0 }
    var t = 0
    for i in 0..3000 {
        x = x.m(1)
        t = t + x.v()
        if i == 2000 { wok(t) }
    }
    wok(t, tangkap())
    var y = B { n: 1 }
    for i in 0..10 { y = y.m(i) }
    wok(y.v())
}
"#;

#[test]
fn penerima_lokal_langsung_ke_argumen() {
    let i = jalankan(METODE_LOKAL, &[], &[]);
    assert_eq!(i.stdout, "2003001\n4501500 112340\n2038");
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(METODE_LOKAL, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// `int`/`uint`/`float` atas bulat native tanpa panggilan, `%`/`/` dengan `Lebar` yang isinya muat 64 bit:
/// `uint` negatif ditolak VM, `Lebar` di atas 2^63 (spekulasi gagal), pecahan yang tidak eksak.
const KONVERSI_LEBAR: &str = r#"fn sisa(s: uint, m: int) -> int { return ((s >> 3) % m) as int }
fn bagi(s: uint, m: int) -> int { return ((s >> 3) / m) as int }
fn besar(s: uint) -> uint { return s % 1000 }
fn pecah(n: int) -> f64 { return n as f64 * 0.5 + (n as float) }
fn tak(n: int) -> uint { return n as uint }
pub fn main() {
    var seed: uint = 88172645463325252
    var t = 0
    var u = 0.0
    for i in 0..3000 {
        seed = math::wrap_add(math::wrap_mul(seed, 6364136223846793005), 1442695040888963407)
        t = t + sisa(seed >> 30, 97) + bagi(seed >> 33, 13) + (besar(seed) as int)
        u = u + pecah(i - 1500) + pecah(9007199254740993)
    }
    wok(t, u)
    wok(besar(18446744073709551615), sisa(18446744073709551615, 7), bagi(18446744073709551615, 3))
    wok(tak(5), pecah(-3))
    wok(tak(-1))
}
"#;

#[test]
fn konversi_dan_sisa_lebar_native() {
    let i = jalankan(KONVERSI_LEBAR, &[], &[]);
    assert!(i.stdout.starts_with("30786593624 40532396646333005824.0\n615 1 768614336404564650\n5 -4.5"), "{}", i.stdout);
    assert!(i.stderr.contains("tidak muat di `uint`"), "{}", i.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(KONVERSI_LEBAR, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Panggilan satu argumen (`int(..)`) tepat di atas `math::wrap_*` bukan `wrap_*` itu sendiri: dulu lengan
/// `wrap` di `turun_lebar` cuma melihat `t - 3` dan menghitung `wrap` atas operand yang salah.
const BUNGKUS_BERSARANG: &str = r#"fn h(s: string) -> uint {
    var a: uint = 7
    for i in 0..len(s) { a = math::wrap_mul(int(str::char_at(s, i)), 31) + a % 1000 }
    return a
}
pub fn main() {
    var t: uint = 0
    for i in 0..2000 {
        t = math::wrap_add(t, math::wrap_mul(i as uint, 11400714819323198485))
        t = math::wrap_add(t, math::wrap_mul(int(i % 50), 3))
    }
    wok(t, h("héllo dunia"), h("abc"))
}
"#;

#[test]
fn intrinsik_di_dalam_argumen_wrap() {
    let i = jalankan(BUNGKUS_BERSARANG, &[], &[]);
    assert_eq!(i.stdout, "17404706681937226416 3982 3121");
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BUNGKUS_BERSARANG, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Struct yang baru dibangun dari angka native lalu jadi lokal (`let`) atau disimpan: bacaan field-nya memakai
/// angka itu. Struct sementara yang langsung dibaca (`R { .. }.id`), struct di dalam struct, tulis field,
/// struct dari panggilan, dan field privat dari luar (galat VM).
const BENIH_STRUCT: &str = r#"struct R { pub id: int, pub v: float, pub s: string }
struct S { pub id: int, pub w: R }
fn ambil(i) { return R { id: i * 2, v: 0.5, s: "x" } }
pub fn main() {
    var t = 0
    var u = 0.0
    for i in 0..3000 {
        let r = R { id: i, v: i as float * 0.25, s: "a" }
        let a = R { id: i + 5, v: 1.0, s: "b" }.id
        let q = ambil(i)
        let w = S { id: 9, w: r }
        t = t + r.id + a + q.id + w.id + w.w.id + r.id
        u = u + r.v + q.v
        let z = R { id: 1, v: 2.0, s: "c" }
        let y = z.id
        t = t + y + z.id
    }
    wok(t, u)
}
"#;
const BENIH_STRUCT_PRIVAT: &str = r#"struct R { pub id: int, pub v: float, pub s: string, k: int }
impl R { fn kk(self) -> int { return self.k * 2 } }
fn ubah(r: R) -> R { var q = r
    q.id = q.id + 100
    return q }
pub fn main() {
    var t = 0
    var u = 0.0
    for i in 0..3000 {
        var r = R { id: i, v: i as float * 0.25, s: "a", k: 3 }
        t = t + r.id + r.kk()
        u = u + r.v
        r.id = r.id + 1
        t = t + r.id
        r = ubah(r)
        t = t + r.id
        let q = R { id: 7, v: 1.5, s: "b", k: 1 }
        t = t + q.id + q.k * 0
    }
    wok(t, u)
}
"#;

#[test]
fn field_struct_baru_dari_angka_pembangunnya() {
    for (src, awal) in [(BENIH_STRUCT, "27039000 1126125.0"), (BENIH_STRUCT_PRIVAT, "")] {
        let i = jalankan(src, &[], &[]);
        assert!(i.stdout.starts_with(awal), "{}", i.stdout);
        for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
            let j = jalankan(src, &["--jit"], env);
            assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
        }
    }
}

/// `str::char_at` atas teks lokal tanpa menyalinnya (salinan maya, `lambat::karakter_lokal`): huruf lebar,
/// lokal tertangkap closure, bukan teks dan di luar batas (jalur bawaan umum, galatnya dari VM).
const KARAKTER_LOKAL: &str = r#"fn hitung(s: string) -> int {
    var n = 0
    for i in 0..len(s) { n = n + (str::char_at(s, i) as int) % 7 }
    return n
}
fn luar(s: string) -> int { return int(str::char_at(s, 99)) }
fn tangkap(s: string) -> int {
    var t = s
    let f = || { t = "zz" }
    var n = 0
    for i in 0..2 { n = n + int(str::char_at(t, i)); f() }
    return n
}
fn bukan(x) { return str::char_at(x, 0) }
pub fn main() {
    var t = 0
    for i in 0..2000 { t = t + hitung("héllo wörld " + str(i)) }
    wok(t, tangkap("ab"), bukan("q"))
    wok(bukan([1, 2]))
    wok(luar("abc"))
}
"#;

#[test]
fn karakter_dari_teks_lokal() {
    let i = jalankan(KARAKTER_LOKAL, &[], &[]);
    assert!(i.stdout.starts_with("88140 219 q") && i.stderr.contains("argumen pertama harus string"), "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(KARAKTER_LOKAL, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// `str(bulat)` dan `len(teks)` dijawab di tempat oleh `lambat::bawaan_cepat`: bulat negatif dan terkecil,
/// huruf lebar, teks kosong, yang lain lewat jalur umum (`len(5)` galat dari VM).
const STR_LEN: &str = r#"pub fn main() {
    var t = 0
    var s = ""
    for i in 0..3000 {
        let a = str(i - 1500) + str(i * 7919)
        t = t + len(a) + len("héllo" + str(i % 3)) + len([1, 2, i])
        if i % 1000 == 0 { s = s + a + "|" }
    }
    wok(t, s, str(-9223372036854775807 - 1), len(""))
    let d = dict { a: 1 }
    wok(len(d), str(1.5), str(true))
    wok(len(5))
}
"#;

#[test]
fn str_dan_len_di_tempat() {
    let i = jalankan(STR_LEN, &[], &[]);
    assert!(i.stdout.starts_with("60875 -15000|") && i.stderr.contains("tidak bisa ukur `int`"), "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(STR_LEN, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// `str::upper`/`lower`/`trim`/`reverse`/`replace`/`contains`/`starts_with`/`ends_with` atas teks lewat inti
/// yang sama dengan interpreter (`str_ops::inti`), tanpa jembatan nilai; rune dan bukan teks lewat jembatan.
const INTI_TEKS: &str = r#"pub fn main() {
    let r = str::char_at("ab", 0)
    var t = ""
    for i in 0..500 {
        let s = "Straße İz ünï " + str(i % 7) + "  "
        t = str::upper(s) + str::lower(s) + str::trim(s) + str::reverse(s) + str::replace(s, "ß", "ss")
            + str::replace(s, "", "-") + str(str::contains(s, "ünï")) + str(str::starts_with(s, "Str"))
            + str(str::ends_with(s, " ")) + str(str::contains(s, ""))
    }
    wok(t)
    wok(str::upper(r), str::replace("aXa", "X", r), str::contains("abc", r), str::replace("a1", "1", 2))
    wok(str::upper(5))
}
"#;

#[test]
fn inti_teks_tanpa_jembatan() {
    let i = jalankan(INTI_TEKS, &[], &[]);
    assert!(i.stderr.contains("str::upper"), "{}", i.stderr);
    for (m, env) in [("--vm", &[][..]), ("--jit", &[STRES][..]), ("--jit", &[STRES, ACAK][..]), ("--jit", &[][..])] {
        let j = jalankan(INTI_TEKS, &[m], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{m} {env:?}");
    }
}

/// `math::` pecahan satu argumen atas angka lewat tabel `Interpreter::unary_f64` yang sama dengan acuan,
/// tanpa jembatan nilai di VM: NaN, `-inf`, pembulatan .5, bulat sebagai argumen, nama telanjang, bukan angka.
const MATH_PECAHAN: &str = r#"pub fn main() {
    var t = 0.0
    for i in 0..20000 {
        let x = i as float * 0.001 + 0.5
        t = t + math::sqrt(x) + math::floor(x) + math::sin(x) + math::ln(x) + math::exp(x * 0.01)
            + math::round(x) + math::trunc(-x) + math::ceil(x) + math::atan(x) + math::log2(x) + math::sqrt(i)
    }
    wok(t, math::sqrt(-1.0), math::ln(0), math::round(2.5), math::round(-2.5), sqrt(16), math::floor(7))
    wok(math::sqrt(true))
}
"#;

#[test]
fn math_pecahan_tanpa_jembatan() {
    let i = jalankan(MATH_PECAHAN, &[], &[]);
    assert!(i.stdout.contains("NaN -inf 3.0 -3.0 4.0 7.0") && i.stderr.contains("math::sqrt: butuh angka"), "{}", i.stdout);
    for (m, env) in [("--vm", &[][..]), ("--jit", &[STRES][..]), ("--jit", &[STRES, ACAK][..]), ("--jit", &[][..])] {
        let j = jalankan(MATH_PECAHAN, &[m], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{m} {env:?}");
    }
}

/// Indeks larik lewat `lambat::norm_indeks` (64 bit): negatif dari belakang, di luar batas ke dua arah, larik
/// kosong, baca dan tulis, elemen pecahan.
const INDEKS_LARIK: &str = r#"fn baca(a, i) { return a[i] }
fn tulis(a, i, v) { var b = a
    b[i] = v
    return b }
pub fn main() {
    var a = [1, 2, 3, 4, 5]
    var t = 0
    for k in 0..3000 {
        let i = k % 10 - 5
        t = t + a[i] * (k % 3)
        a[i] = a[i] + 1
    }
    wok(t, a, baca(a, -1), baca(a, -5), tulis(a, -2, 9))
    var f = [1.5, 2.5]
    f[-1] = f[-2] + f[1]
    wok(f)
    wok(baca([], 0))
}
"#;

#[test]
fn indeks_larik_negatif_dan_luar_batas() {
    let i = jalankan(INDEKS_LARIK, &[], &[]);
    assert!(i.stdout.starts_with("907900 [601, 602, 603, 604, 605]") && i.stderr.contains("array ini kosong"), "{}", i.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(INDEKS_LARIK, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Callee bulat kecil disisipkan Cranelift di tempat panggilannya (`modul::Sisip`): hasil, bersarang, dan
/// luapan 64 bit di dalam badan yang disisipkan (deopt-nya lewat status yang sama dengan panggilan biasa).
const SISIP_BULAT: &str = r#"fn kuad(x: int) -> int { return x * x + 1 }
fn campur(a: int, b: int) -> int { return kuad(a) % 1000 + kuad(b) % 7 }
fn luap(x: int) -> int { return x * 3037000500 }
pub fn main() {
    var t = 0
    for i in 0..300000 { t = (t + campur(i, i + 3)) % 1000000007 }
    wok(t)
    var u = 0
    for i in 0..10 { u = u + luap(i * 1000000000) }
    wok(u)
}
"#;

#[test]
fn callee_bulat_disisipkan() {
    let i = jalankan(SISIP_BULAT, &[], &[]);
    assert_eq!(i.stdout, "139650000\n136665022500000000000");
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(SISIP_BULAT, &["--jit"], env);
        assert_eq!((i.sukses, &i.stdout, &i.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Callee `layak::ringan` dipanggil tanpa bingkai VM didorong lebih dulu (`lambat::masuk_ringan`): deopt,
/// galat dengan jejaknya (indeks di luar batas, `match` tanpa lengan cocok), dan spekulasi gagal di dalamnya —
/// bingkainya diwujudkan `keluar_ringan` sebelum VM melanjutkan, jadi stderr sama persis dengan VM.
const BINGKAI_TUNDA: &str = r#"enum Op { A(int), B((int, int)), C(string) }
struct P { pub x: int, pub y: float }
impl P {
    fn geser(self, k) -> P { return P { x: self.x + k, y: self.y * 0.5 } }
    fn ambil(self, i) -> int { let t = (self.x, 2, 3); return t[i] }
}
fn ex(o, acc) {
    match o {
        Op::A(k) => acc + k,
        Op::B(p) => acc + p.0 * p.1,
    }
}
pub fn main() {
    var t = 0
    var p = P { x: 0, y: 1.0 }
    for i in 0..3000 {
        let o = i % 2 == 0 $ Op::A(i) : Op::B((i, 2))
        t = ex(o, t) % 1000003
        p = p.geser(1)
        t = t + p.ambil(i % 3)
    }
    wok(t, p.x, p.y)
    wok(ex(Op::A(1.5), 2))
    wok(p.ambil(1))
    wok(p.ambil(7))
}
"#;
const BINGKAI_TUNDA_MATCH: &str = r#"enum Op { A(int), C(string) }
fn ex(o, acc) { match o { Op::A(k) => acc + k } }
pub fn main() {
    var t = 0
    for i in 0..3000 { t = ex(Op::A(i), t) % 1000 }
    wok(t)
    wok(ex(Op::C("z"), 1))
}
"#;

#[test]
fn callee_ringan_tanpa_bingkai_didorong() {
    for (src, galat) in [(BINGKAI_TUNDA, "dipanggil di"), (BINGKAI_TUNDA_MATCH, "tidak exhaustive")] {
        let v = jalankan(src, &["--vm"], &[]);
        assert!(v.stderr.contains(galat), "{}", v.stderr);
        for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
            let j = jalankan(src, &["--jit"], env);
            assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
        }
    }
}

/// Pemeriksa `match` sebagai salinan maya (J6 langkah 41): closure menimpa pemeriksanya di tengah lengan (match
/// luar memegang nilai saat masuk, match dalam sesudah panggilan membaca yang baru), pemeriksa ditulis ulang
/// di lengannya lalu dibaca sesudah match, dan `let t = a` yang dibaca lewat lokal sementaranya.
const SALINAN_PEMERIKSA: &str = r#"
package main
enum Op { A(int), B(int), C }
fn exec(o: Op, acc: int) -> int {
    match o {
        Op::A(k) => acc + k,
        Op::B(k) => acc * k % 1000003,
        Op::C => acc ^ 77,
    }
}
pub fn main() {
    var o: Op = Op::A(1)
    let ganti = || { o = Op::B(2) }
    var hasil = []
    for i in 0..300 {
        o = Op::A(i)
        let r = match o {
            Op::C => -1,
            Op::A(k) => {
                ganti()
                let dalam = match o { Op::B(m) => m * 10, Op::A(m) => m, Op::C => -2 }
                k * 1000 + dalam
            }
            Op::B(k) => k,
        }
        if (i % 100 == 0) { hasil = arr::push(hasil, r) }
    }
    wok(hasil, o)
    var p: Op = Op::A(7)
    var acc = 1
    for j in 0..300 {
        let q = match p {
            Op::A(k) => { p = Op::B(k + 1); k }
            Op::B(k) => { p = Op::A(k + 1); k * 2 }
            Op::C => 0,
        }
        acc = exec(p, acc + q)
        if (j % 100 == 0) { wok(j, q, p, acc) }
    }
    var arr = [1, 2, 3]
    var tot = 0
    for i in 0..300 {
        let t = arr
        tot = tot + t[i % 3] + len(t)
        arr[i % 3] = arr[i % 3] + 1
    }
    wok(p, tot, arr)
}
"#;

/// Deopt di dalam lengan: luapan 64 bit atas payload sesudah pemeriksanya cuma maya — VM yang melanjutkan
/// harus melihat salinan pemeriksa yang diwujudkan dari lokalnya.
const SALINAN_PEMERIKSA_DEOPT: &str = r#"
package main
enum Op { Kecil(int), Besar(int) }
fn nilai(o: Op) -> int {
    match o {
        Op::Kecil(k) => k * 2,
        Op::Besar(k) => k * 9223372036854775807,
    }
}
pub fn main() {
    var total: int = 0
    for i in 0..400 {
        let o = if (i % 2 == 0) { Op::Kecil(i) } else { Op::Besar(i % 3) }
        total = total + nilai(o)
    }
    wok(total)
    wok(nilai(Op::Besar(170141183460469231731687303715884105727)))
}
"#;

#[test]
fn pemeriksa_match_tanpa_salinan() {
    for (src, galat) in [(SALINAN_PEMERIKSA, ""), (SALINAN_PEMERIKSA_DEOPT, "kelewat besar")] {
        let v = jalankan(src, &["--vm"], &[]);
        assert!(v.stderr.contains(galat), "{}", v.stderr);
        for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
            let j = jalankan(src, &["--jit"], env);
            assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
        }
    }
    // Rencananya memang memakai salinan maya untuk pemeriksa `exec`: `LoadLocal 0` jadi `Salinan`.
    let j = jalankan(SALINAN_PEMERIKSA, &["--jit"], &[STRES, ("JRG_JIT_LOG", "rencana")]);
    let rencana = j.stderr.split("rencana exec").nth(1).expect("rencana exec dicetak");
    assert!(rencana.lines().take(3).any(|l| l.contains("LoadLocal 0 Salinan")), "{rencana}");
}

/// Jalur sebaris `a[i]` (J8, `tata_letak`): tiap bentuk yang tidak pasti jatuh ke pembantu dengan jawaban yang
/// sama — larik dibagi variabel lain, larik di sel tangkapan closure, elemen di atas 64 bit, pecahan dan teks di
/// larik bulat, indeks negatif, larik yang ditulis dan tumbuh di tengah loop, dan indeks di luar batas di akhir.
const LARIK_SEBARIS: &str = r#"
package main
fn jumlah(a, n: int) -> int {
    var s = 0
    for i in 0..n { s = s + a[i] + a[-1 - (i % 3)] }
    return s
}
pub fn main() {
    var a = [3, 1, 4, 1, 5, 9, 2, 6]
    var tot = 0
    for r in 0..200 {
        for i in 0..8 { tot = tot + a[i] * (r % 7) }
        if (r == 50) { let b = a; tot = tot + b[2] }
        if (r == 120) { a[3] = 1180591620717411303424 }
        if (r == 121) { a[3] = 1 }
        if (r == 150) { a = a + [7] }
    }
    wok(tot, jumlah(a, 9))
    var c = [1, 2, 3]
    let ubah = || { c[0] = c[0] + 10 }
    var u = 0
    for i in 0..300 {
        u = u + c[i % 3]
        if (i % 100 == 0) { ubah() }
    }
    wok(u, c)
    var campur = [1, 2, 3, 4]
    var v = 0
    for i in 0..400 {
        v = v + campur[i % 4]
        if (i == 200) { campur[1] = 2.5 }
    }
    wok(v)
    var bersama = [5, 6, 7]
    let simpan = bersama
    var w = 0
    for i in 0..300 { w = w + bersama[i % 3] }
    wok(w, simpan)
    var teks = ["a", "b", 3]
    var z = [[1], 2, 3]
    var dulu = [1, 2, 3]
    // Indeks bernama: bentuk yang diturunkan VM jadi `TulisIndexLokal` (jalur tulis sebaris).
    for i in 0..300 {
        let k = i % 3
        let q = i % 4
        teks[k] = i
        z[k] = i * 2
        if (i == 150) { let lama = dulu; dulu[0] = 99; wok(lama, dulu) }
        dulu[k] = dulu[k] + 1
        campur[q] = 0.5 * i
    }
    var nyala = [true, false, true]
    for i in 0..300 { let k = i % 3; nyala[k] = i % 2 == 0 }
    wok(teks, z, dulu, campur, nyala)
    let x = [10, 20, 30]
    var y = 0
    for i in 0..300 { y = y + x[i % 3] }
    wok(y, x[3])
}
"#;

#[test]
fn larik_sebaris_jatuh_ke_pembantu_bila_tak_pasti() {
    let v = jalankan(LARIK_SEBARIS, &["--vm"], &[]);
    assert!(v.stderr.contains("di luar"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(LARIK_SEBARIS, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    // IR yang ditolak verifier mematikan JIT diam-diam — jawabannya tetap benar lewat VM, jadi paritas saja tidak
    // menangkapnya: `main` harus benar-benar dikompilasi. Verifier cuma menyala kalau `JRG_JIT_STRES` diset.
    let j = jalankan(LARIK_SEBARIS, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    let baris = j.stderr.lines().find(|l| l.contains("dikompilasi:")).unwrap_or("");
    assert!(baris.contains("main"), "main tidak dikompilasi:\n{}", j.stderr);
}

/// `()` sebagai `Jenis::Satuan` (J8): tidak ditulis ke stack VM sampai ada yang butuh isinya. Nilai `if` tanpa
/// `else` dipakai, disimpan, masuk larik, dikembalikan, ditanya `typeof`, dibandingkan, dan hidup di stack saat
/// deopt (luapan 64 bit di putaran yang sama).
const SATUAN_MAYA: &str = r#"
package main
fn kosong(n: int) { if (n > 5) { wok("besar") } }
fn bare() { return }
pub fn main() {
    var simpan = []
    var hitung = 0
    var besar = 1
    for i in 0..400 {
        let u = if (i % 7 == 0) { hitung = hitung + 1 }
        if (i % 100 == 0) { simpan = arr::push(simpan, u) }
        let v = kosong(i % 3)
        if (typeof(v) != "unit") { wok("salah") }
        if (u != ()) { wok("beda") }
        besar = (besar * 3 + i) % 1000000000000000000000
        if (i == 390) { besar = 9223372036854775000 }
    }
    wok(hitung, simpan, typeof(bare()), besar + 1000, [(), 1])
}
"#;

#[test]
fn unit_tidak_ditulis_ke_stack_vm() {
    let v = jalankan(SATUAN_MAYA, &["--vm"], &[]);
    assert!(v.sukses, "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(SATUAN_MAYA, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(SATUAN_MAYA, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    let baris = j.stderr.lines().find(|l| l.contains("dikompilasi:")).unwrap_or("");
    assert!(baris.contains("main"), "main tidak dikompilasi:\n{}", j.stderr);
}

/// Bawaan murni dipanggil langsung atas argumennya di stack VM (J6 langkah 45): sesudah ratusan panggilan yang
/// berhasil (terkompilasi), argumen berjenis salah harus gagal dengan kalimat, lokasi dan jejak yang sama.
const BAWAAN_LANGSUNG: &str = r#"
package main
fn olah(x, i: int) -> string {
    let a = str::upper(x)
    let b = str::replace(a, "A", "b")
    return str::reverse(b) + str(i)
}
pub fn main() {
    var n = 0
    for i in 0..400 {
        let x = if (i == 350) { 42 } else { "aku-" + str(i % 7) }
        n = n + len(olah(x, i))
    }
    wok(n)
}
"#;

#[test]
fn bawaan_langsung_gagal_sama() {
    let v = jalankan(BAWAAN_LANGSUNG, &["--vm"], &[]);
    assert!(!v.sukses && v.stderr.contains("str::upper"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BAWAAN_LANGSUNG, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(BAWAAN_LANGSUNG, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    let baris = j.stderr.lines().find(|l| l.contains("dikompilasi:")).unwrap_or("");
    assert!(baris.contains("olah"), "olah tidak dikompilasi:\n{}", j.stderr);
}

/// Bacaan field lewat `field_lokal`/`field_lokal_banyak` di bentuk-bentuk yang tidak biasa — struct dibagi, di sel
/// tangkapan, dict bernama field sama, struct di atas 16 field (tabel hash), dua bentuk di satu situs, field
/// privat, field berisi jenis lain, dan deopt (luapan) di tengah bacaan. (Dari J8 tahap 5, yang dicabut.)
const FIELD_SEBARIS: &str = r#"
package main
struct P { pub x: int, pub y: int, pub z: f64 }
struct Q { pub y: int, pub x: int, pub z: f64, pub w: int }
struct R { a: int, pub b: int }
struct Besar { pub f0: int, pub f1: int, pub f2: int, pub f3: int, pub f4: int, pub f5: int, pub f6: int, pub f7: int,
    pub f8: int, pub f9: int, pub f10: int, pub f11: int, pub f12: int, pub f13: int, pub f14: int, pub f15: int, pub f16: int }
impl R { fn jumlah(self) -> int { self.a + self.b } }
fn baca(p) -> f64 { (p.x + p.y) as f64 + p.z }
pub fn main() {
    var t = 0.0
    for i in 0..400 {
        var p = P { x: i, y: i * 2, z: 0.5 }
        if (i % 3 == 0) { let salin = p; t = t + baca(salin) + (salin.x as f64) }
        t = t + baca(p)
        if (i % 5 == 0) { t = t + baca(Q { y: 1, x: 2, z: 0.25, w: 9 }) }
        if (i == 300) { p.x = 9223372036854775000 }
        t = t + (p.x % 1000) as f64
    }
    wok(t)
    var c = P { x: 1, y: 2, z: 3.0 }
    let ubah = || { c.x = c.x + 1 }
    var u = 0
    for i in 0..300 { u = u + c.x + c.y; if (i % 50 == 0) { ubah() } }
    wok(u, c.x)
    var d = dict { x: 1, y: 2, z: 0.5 }
    var v = 0.0
    for i in 0..300 { v = v + baca(d) }
    wok(v)
    let b = Besar { f0: 0, f1: 1, f2: 2, f3: 3, f4: 4, f5: 5, f6: 6, f7: 7, f8: 8, f9: 9, f10: 10, f11: 11, f12: 12,
        f13: 13, f14: 14, f15: 15, f16: 16 }
    var w = 0
    for i in 0..300 { w = w + b.f3 + b.f16 + b.f0 }
    wok(w)
    var r = R { a: 3, b: 4 }
    var x = 0
    for i in 0..300 { x = x + r.jumlah() + r.b }
    wok(x)
    var campur = P { x: 1, y: 2, z: 3.0 }
    var y = 0
    for i in 0..300 { y = y + campur.x; if (i == 200) { campur = P { x: 5, y: 6, z: 7.0 } } }
    wok(y, typeof(baca(P { x: 1, y: 2, z: 0.5 })))
}
"#;

#[test]
fn field_sebaris_jatuh_ke_pembantu_bila_tak_pasti() {
    let v = jalankan(FIELD_SEBARIS, &["--vm"], &[]);
    assert!(v.sukses, "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(FIELD_SEBARIS, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(FIELD_SEBARIS, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    let baris = j.stderr.lines().find(|l| l.contains("dikompilasi:")).unwrap_or("");
    assert!(baris.contains("main") && baris.contains("baca"), "main/baca tidak dikompilasi:\n{}", j.stderr);
}

/// Struct yang langsung dikembalikan memakai ulang lokal sebentuk milik tunggal (J6 langkah 46). Yang tidak boleh
/// teramati: salinan penerima yang masih dipegang pemanggil tetap utuh, tipe lain bernama field sama tidak ikut,
/// struct yang memuat `self` tidak menimpa `self`, sel tangkapan tidak dipakai, dan deopt di tengahnya.
const PAKAI_ULANG: &str = r#"
package main
struct P { pub x: int, pub y: f64 }
struct Q { pub x: int, pub y: f64 }
struct N { pub isi: P, pub n: int }
impl P {
    fn maju(self, d: f64) -> P { return P { x: self.x + 1, y: self.y + d } }
    fn jadi_q(self) -> Q { return Q { x: self.x, y: self.y } }
    fn bungkus(self) -> N { return N { isi: self, n: self.x } }
}
fn gabung(a: P, b: P) -> P { return P { x: a.x + b.x, y: a.y * b.y } }
pub fn main() {
    var p = P { x: 0, y: 0.5 }
    var simpan = []
    for i in 0..400 {
        if (i % 100 == 0) { let lama = p; p = p.maju(0.25); simpan = arr::push(simpan, lama) } else { p = p.maju(0.25) }
        if (i == 300) { p = P { x: 9223372036854775000, y: p.y } }
    }
    wok(p, simpan)
    var q = Q { x: 0, y: 0.0 }
    var n = N { isi: P { x: 0, y: 0.0 }, n: 0 }
    var g = P { x: 1, y: 1.0 }
    for i in 0..300 {
        q = p.jadi_q()
        n = P { x: i, y: 2.0 }.bungkus()
        g = gabung(P { x: i, y: 1.5 }, P { x: 2, y: 2.0 })
    }
    wok(q, n, g)
    var c = P { x: 5, y: 1.0 }
    let lihat = || c.x
    var s = 0
    for i in 0..300 { c = c.maju(1.0); s = s + lihat() }
    wok(c, s)
}
"#;

#[test]
fn struct_dipakai_ulang_tidak_teramati() {
    let v = jalankan(PAKAI_ULANG, &["--vm"], &[]);
    assert!(v.sukses, "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(PAKAI_ULANG, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(PAKAI_ULANG, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    let baris = j.stderr.lines().find(|l| l.contains("dikompilasi:")).unwrap_or("");
    assert!(baris.contains("P::maju") && baris.contains("gabung"), "maju/gabung tidak dikompilasi:\n{}", j.stderr);
}

/// Pola berkualifikasi (`SitusPola`): alamat nama varian yang lain = tidak cocok tanpa `cocok_enum`. Dua enum
/// dengan varian bernama sama, pemeriksa yang bukan enum (struct, dict, bulat, `nil`), lokal di sel tangkapan
/// closure, dan galat jumlah sub-pola — jawaban dan waktu galatnya sama dengan `--vm`.
const POLA_SEBARIS: &str = r#"
package main
enum A { X(int), Y, Z(int, int) }
enum B { X(int), Y }
struct S { x: int }
fn nilai_a(o: A) -> int {
    match o {
        A::X(k) => k,
        A::Y => 100,
        A::Z(p, q) => p * q,
    }
}
fn nilai_b(o: B) -> int {
    match o {
        B::X(k) => k + 1000,
        B::Y => 2000,
    }
}
fn apa(v) -> int {
    match v {
        A::X(k) => k,
        B::X(k) => k + 7,
        A::Y => 1,
        B::Y => 2,
        _ => 3,
    }
}
pub fn main() {
    var tot = 0
    for i in 0..3000 {
        let a = if (i % 3 == 0) { A::X(i) } else if (i % 3 == 1) { A::Y } else { A::Z(i, 2) }
        let b = if (i % 2 == 0) { B::X(i) } else { B::Y }
        tot = tot + nilai_a(a) + nilai_b(b)
        tot = tot + apa(a) + apa(b) + apa(S { x: i }) + apa(dict { x: i }) + apa(i) + apa(nil)
    }
    var c = A::Y
    let ubah = || { c = A::X(5) }
    var u = 0
    for i in 0..500 {
        u = u + match c { A::X(k) => k, A::Y => 1, A::Z(p, q) => p + q }
        if (i == 300) { ubah() }
    }
    wok(tot, u)
    var g = 0
    for i in 0..400 {
        let w = if (i < 399) { A::Y } else { A::X(4) }
        g = g + match w { A::Z(p) => 9, A::Y => 1, A::X(p, q) => p }
    }
    wok(g)
}
"#;

#[test]
fn pola_berkualifikasi_sama_dengan_vm() {
    let v = jalankan(POLA_SEBARIS, &["--vm"], &[]);
    assert!(v.stderr.contains("butuh 1 sub-pola, dapat 2"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(POLA_SEBARIS, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    // Keempat fungsinya benar-benar dikompilasi (JIT yang mati diam-diam juga menjawab sama).
    let j = jalankan(POLA_SEBARIS, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: apa, main, nilai_a, nilai_b"), "{}", j.stderr);
}

/// Lokal yang mati sesudah dibaca dipindah, bukan disalin (`mati_sesudah`): yang dibaca lagi sesudah panggilan,
/// di putaran berikutnya, lewat sel tangkapan closure, atau di cabang tetap utuh; callee yang deopt atau
/// menerima jenis lain di tengah loop — jawabannya sama dengan `--vm`.
const PINDAH_LOKAL: &str = r#"
package main
enum Op { A(int), B }
struct P { pub x: int }
fn nilai(o: Op) -> int {
    match o { Op::A(k) => k, Op::B => 1 }
}
fn ukur(p: P, k: int) -> int { p.x * k }
fn sisa(o: Op, n: int) -> int {
    if (n > 2990) { return 1180591620717411303424 % 1000 + nilai(o) }
    nilai(o)
}
pub fn main() {
    var tot = 0
    var tetap = Op::A(3)
    for i in 0..3000 {
        let o = if (i % 2 == 0) { Op::A(i) } else { Op::B }
        tot = tot + nilai(o)
        tot = tot + nilai(o)
        let p = P { x: i }
        tot = tot + ukur(p, 2)
        if (i % 7 == 0) { tot = tot + p.x }
        tot = tot + nilai(tetap)
        let q = Op::A(i % 5)
        tot = tot + sisa(q, i)
    }
    wok(tot, nilai(tetap))
    var c = Op::B
    let lihat = || nilai(c)
    var u = 0
    for i in 0..500 {
        let d = c
        u = u + nilai(d) + nilai(c)
        if (i == 250) { c = Op::A(7) }
    }
    wok(u, lihat())
    var w = 0
    for i in 0..400 {
        let r = if (i < 399) { Op::B } else { Op::A(1) }
        w = w + nilai(r)
        let z = P { x: i }
        w = w + ukur(z, if (i == 399) { "dua" } else { 2 })
    }
    wok(w)
}
"#;

#[test]
fn lokal_mati_dipindah_tidak_teramati() {
    let v = jalankan(PINDAH_LOKAL, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.starts_with("14158458 3\n3988 7\n"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(PINDAH_LOKAL, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(PINDAH_LOKAL, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("main, nilai, sisa, ukur"), "{}", j.stderr);
}

/// Struct dipakai ulang dengan field yang ditulis di tempat (angka atas angka sejenis): field yang berganti
/// jenis (bulat, pecahan, bulat di atas 64 bit, teks) dan salinan bersama yang harus tetap utuh — sama dengan `--vm`.
const ISI_DI_TEMPAT: &str = r#"
package main
struct Q { pub a: int, pub b: int, pub c: int }
impl Q {
    fn maju(self, k: int) -> Q {
        return Q { a: self.b, b: self.a + k, c: self.c }
    }
    fn ganti(self, i: int) -> Q {
        return Q { a: if (i % 3 == 0) { 1.5 } else { i }, b: if (i == 150) { 1180591620717411303424 } else { self.b }, c: if (i == 160) { "teks" } else { self.c } }
    }
}
pub fn main() {
    var q = Q { a: 1, b: 2.5, c: 0 }
    for i in 0..300 {
        q = q.maju(i)
        if (i % 50 == 0) { wok(q.a, q.b, q.c) }
    }
    var r = Q { a: 0, b: 0, c: 0 }
    for i in 0..300 {
        r = r.ganti(i)
        if (i % 40 == 0 || i == 150 || i == 151 || i == 160) { wok(r.a, r.b, r.c) }
    }
    let s = r
    r = r.ganti(7)
    wok(s.a, r.a)
}
"#;

#[test]
fn isi_struct_di_tempat_tidak_teramati() {
    let v = jalankan(ISI_DI_TEMPAT, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.ends_with("299 7"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(ISI_DI_TEMPAT, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(ISI_DI_TEMPAT, &["--jit"], &[STRES, ACAK, ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("Q::ganti, Q::maju, main"), "{}", j.stderr);
}

/// Field pertama ikut diambil bersama field sesudahnya (`field_lokal_banyak`, bit 0) dan label method diintern:
/// field pertama atau lain yang berganti jenis di tengah loop, bulat di atas 64 bit, teks, method atas dua
/// struct sebentuk dan atas enum (label teks) — jawaban dan galatnya sama dengan `--vm`.
const FIELD_GABUNG: &str = r#"
package main
struct V { pub x: float, pub y: float, pub z: float }
struct W { pub x: float, pub y: float, pub z: float }
enum E { A(int), B }
impl V {
    fn total(self) -> float { self.x * 2.0 + self.y + self.z }
}
impl W {
    fn total(self) -> float { self.x + self.y * 3.0 + self.z }
}
impl E {
    fn nilai(self) -> int { match self { E::A(k) => k, E::B => 0 } }
}
fn campur(v: V) -> float { v.x + v.y - v.z }
pub fn main() {
    var t = 0.0
    var n = 0
    for i in 0..600 {
        let fi = i as float
        let v = V { x: if (i == 300) { 1.0 } else { fi }, y: fi * 0.5, z: if (i == 450) { 2.0 } else { 1.5 } }
        t = t + v.total() + campur(v)
        let w = W { x: fi, y: 1.0, z: fi }
        t = t + w.total()
        let e = if (i % 3 == 0) { E::A(i) } else { E::B }
        n = n + e.nilai()
    }
    wok(t, n)
    var u = 0.0
    for i in 0..400 {
        let v = V { x: if (i == 399) { 7 } else { 1.5 }, y: 2.5, z: if (i == 398) { 1180591620717411303424 } else { 0.5 } }
        u = u + campur(v)
    }
    wok(u)
    for i in 0..300 {
        let v = V { x: if (i == 299) { "s" } else { 1.0 }, y: 1.0, z: 1.0 }
        u = u + campur(v)
    }
    wok(u)
}
"#;

#[test]
fn field_gabung_dan_label_metode_sama_dengan_vm() {
    let v = jalankan(FIELD_GABUNG, &["--vm"], &[]);
    assert!(v.stdout.starts_with("1079103.0 59700") && v.stderr.contains("`string` dan `float`"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(FIELD_GABUNG, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(FIELD_GABUNG, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: E::nilai, V::total, W::total, campur, main"), "{}", j.stderr);
}

/// `SitusBanyak` (satu tembolok bentuk untuk daftar field): struct dengan field sama berurutan lain, dict, angka
/// berganti jenis, dan struct tanpa salah satu field lewat situs yang sama — jawaban dan galatnya sama dengan `--vm`.
const FIELD_BANYAK_POLIMORF: &str = r#"
package main
struct A { pub x: float, pub y: float, pub z: float }
struct B { pub z: float, pub y: float, pub x: float }
struct C { pub x: float, pub y: float }
fn jumlah(p) -> float { p.x * 2.0 + p.y * 3.0 + p.z }
pub fn main() {
    var t = 0.0
    for i in 0..600 {
        let fi = i as float
        t = t + jumlah(A { x: fi, y: 1.0, z: 2.0 })
        if (i % 5 == 0) { t = t + jumlah(B { z: fi, y: 2.0, x: 3.0 }) }
        if (i % 7 == 0) { t = t + jumlah(dict { x: 1.0, y: fi, z: 0.5 }) }
        if (i == 400) { t = t + jumlah(A { x: 1, y: 2, z: 3 }) }
    }
    wok(t)
    for i in 0..300 {
        let p = if (i == 299) { C { x: 1.0, y: 2.0 } } else { A { x: 1.0, y: 2.0, z: 3.0 } }
        t = t + jumlah(p)
    }
    wok(t)
}
"#;

#[test]
fn field_banyak_polimorf_sama_dengan_vm() {
    let v = jalankan(FIELD_BANYAK_POLIMORF, &["--vm"], &[]);
    assert!(v.stdout.starts_with("476521.0") && v.stderr.contains("bernama `z`"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(FIELD_BANYAK_POLIMORF, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(FIELD_BANYAK_POLIMORF, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: jumlah, main"), "{}", j.stderr);
}

/// Cadangan struct (`buang` -> `bangun`): salinan yang disimpan di larik sebelum dilepas tetap utuh, field teks,
/// tipe lain sebentuk, dict sebentuk, dan struct yang diubah sesudah dipakai ulang — sama dengan `--vm`.
const CADANGAN_STRUCT: &str = r#"
package main
struct R { pub a: int, pub b: float }
struct S { pub a: int, pub b: float }
struct T { pub a: int, pub b: string }
fn nilai(r: R) -> float { (r.a as float) + r.b }
pub fn main() {
    var simpan = []
    var tot = 0.0
    for i in 0..800 {
        let r = R { a: i, b: 0.5 }
        tot = tot + nilai(r)
        if (i % 100 == 0) { simpan = simpan + [r] }
        let s = S { a: i * 2, b: 1.5 }
        tot = tot + (s.a as float) + s.b
        let t = T { a: i, b: "x" + str(i % 3) }
        if (i % 200 == 0) { wok(t.b) }
        let d = dict { a: i, b: 2.5 }
        tot = tot + d["b"]
        var m = R { a: 1, b: 1.0 }
        m.a = m.a + i
        tot = tot + nilai(m)
    }
    wok(tot)
    for k, r in simpan { wok(k, r.a, r.b) }
}
"#;

#[test]
fn cadangan_struct_tidak_teramati() {
    let v = jalankan(CADANGAN_STRUCT, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.ends_with("7 700 0.5"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(CADANGAN_STRUCT, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(CADANGAN_STRUCT, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: main, nilai"), "{}", j.stderr);
}

/// Tembolok penyimpanan larik per wilayah (`TemboLarik`): tukar elemen, salinan `let b = a` lalu tulis, larik
/// diserahkan ke fungsi, tumbuh, bulat di atas 64 bit, sel tangkapan closure, teks, dan indeks di luar batas.
const LARIK_TEMBOLOK: &str = r#"
package main
fn jumlah(x) -> int {
    var s = 0
    for i in 0..len(x) { s = s + x[i] }
    return s
}
pub fn main() {
    var a = [5, 3, 8, 1, 9, 2, 7, 4]
    var tot = 0
    for r in 0..300 {
        let i = r % 8
        let j = (r * 3) % 8
        if (a[i] > a[j]) { let t = a[i]; a[i] = a[j]; a[j] = t }
        tot = tot + a[i] + a[j] + a[-1]
        let b = a
        a[0] = a[0] + 1
        tot = tot + b[0] + a[0]
        a[1] = a[1] + jumlah(a) % 3
        if (r == 100) { a = a + [6] }
        if (r == 200) { a[2] = 1180591620717411303424 }
        if (r == 201) { a[2] = 3 }
    }
    wok(tot, a)
    var c = [1, 2, 3]
    let tambah = || { c[0] = c[0] + 100 }
    var u = 0
    for k in 0..200 {
        u = u + c[k % 3] + c[0]
        c[1] = c[1] + 1
        if (k % 50 == 0) { tambah() }
    }
    wok(u, c)
    var d = [1, 2, 3, 4]
    for k in 0..100 {
        d[k % 4] = d[(k + 1) % 4] + d[(k + 2) % 4]
        if (k == 99) { d[0] = "teks" }
    }
    wok(d)
    var e = [1, 2, 3]
    var v = 0
    for k in 0..10 { v = v + e[k] }
    wok(v)
}
"#;

#[test]
fn larik_tembolok_sama_dengan_vm() {
    let v = jalankan(LARIK_TEMBOLOK, &["--vm"], &[]);
    assert!(v.stdout.starts_with("114284 [305, 115, 3, 41, 9, 4, 7, 2, 6]") && v.stderr.contains("di luar batas"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(LARIK_TEMBOLOK, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(LARIK_TEMBOLOK, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: jumlah, main"), "{}", j.stderr);
}

/// `payload_angka_lokal` (payload enum dibuka ke register tanpa ditulis ke stack VM): payload yang berganti
/// jenis, bulat di atas 64 bit, `Option<float>`, varian tanpa payload, enum di sel tangkapan, payload teks.
const PAYLOAD_ANGKA: &str = r#"
package main
enum K { A(int), B, C(int, int) }
fn ambil(k: K) -> int {
    match k {
        K::A(x) => x + 1,
        K::B => 0,
        K::C(p, q) => p - q,
    }
}
fn pecahan(o: Option<float>) -> float {
    match o { Some(v) => v * 2.0, None => -1.0 }
}
pub fn main() {
    var t = 0
    for i in 0..900 {
        let k = if (i % 3 == 0) { K::A(i) } else if (i % 3 == 1) { K::B } else { K::C(i, 1) }
        t = t + ambil(k)
        if (i == 600) { t = t + ambil(K::A(1180591620717411303424 % 7)) }
    }
    wok(t)
    var f = 0.0
    for i in 0..500 {
        let o = if (i % 4 == 0) { None } else { Some((i as float) * 0.5) }
        f = f + pecahan(o)
    }
    wok(f)
    var c = K::A(5)
    let ganti = || { c = K::C(9, 4) }
    var u = 0
    for i in 0..300 {
        u = u + match c { K::A(x) => x, K::B => 0, K::C(p, q) => p * q }
        if (i == 150) { ganti() }
    }
    wok(u)
    var g = 0
    for i in 0..400 {
        let k = if (i == 399) { K::A("teks") } else { K::A(i) }
        g = g + ambil(k)
    }
    wok(g)
}
"#;

#[test]
fn payload_angka_sama_dengan_vm() {
    let v = jalankan(PAYLOAD_ANGKA, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.ends_with("79800teks1"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(PAYLOAD_ANGKA, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(PAYLOAD_ANGKA, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: ambil, main, pecahan"), "{}", j.stderr);
}

/// Rantai lengan `match` (`rantai_pola`/`pola_pilih`): lengan berpenjaga dan pola alternatif (rantai putus di
/// titik berpendahulu dua), payload tuple diikat satu nama, dan galat jumlah sub-pola di lengan kelima sesudah
/// empat lengan tidak cocok — jawaban dan tempat galatnya sama dengan `--vm`.
const RANTAI_POLA: &str = r#"
package main
enum Op { Push(int), Add(int), Neg, Swap, Stop(int, int) }
fn langkah(o: Op, acc: int) -> int {
    match o {
        Op::Push(k) if k > 50 => k * 2,
        Op::Push(k) => k,
        Op::Add(k) if k % 2 == 0 => acc + k,
        Op::Add(k) => acc - k,
        Op::Neg | Op::Swap => -acc,
        Op::Stop(a, b) => a * b,
    }
}
fn salah(o: Op) -> int {
    match o {
        Op::Neg => 1,
        Op::Swap => 2,
        Op::Push(k) => k,
        Op::Stop(a) => 7,
        Op::Add(x, y) => x,
    }
}
pub fn main() {
    var acc = 1
    for i in 0..1200 {
        let o = if (i % 5 == 0) { Op::Push(i % 100) } else if (i % 5 == 1) { Op::Add(i % 9) } else if (i % 5 == 2) { Op::Neg } else if (i % 5 == 3) { Op::Swap } else { Op::Stop(i % 7, 3) }
        acc = (langkah(o, acc) + acc) % 100003
    }
    wok(acc)
    var s = 0
    for i in 0..600 {
        let o = if (i % 4 == 0) { Op::Neg } else if (i % 4 == 1) { Op::Swap } else if (i % 4 == 2) { Op::Push(i) } else { Op::Stop(i, 2) }
        s = s + salah(o)
    }
    wok(s)
    s = s + salah(Op::Add(3))
    wok(s)
}
"#;

#[test]
fn rantai_pola_sama_dengan_vm() {
    let v = jalankan(RANTAI_POLA, &["--vm"], &[]);
    assert!(v.stdout == "6\n46500" && v.stderr.contains("butuh 1 sub-pola, dapat 2"), "{} {}", v.stdout, v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(RANTAI_POLA, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(RANTAI_POLA, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: langkah, main, salah"), "{}", j.stderr);
}

/// `arr::pop` yang memindah isi larik milik tunggal (sesi antrean, #1015) lewat jalur JIT: salinan yang masih
/// dibaca, lokal mati yang dipindah (`ambil`), tembolok penyimpanan larik, callee, dan pop berulang.
const POP_PINDAH: &str = r#"
package main
fn buang_akhir(x) -> int {
    let r = arr::pop(x)
    return len(r[0])
}
fn jumlah(x) -> int {
    var s = 0
    for i in 0..len(x) { s = s + x[i] }
    return s
}
pub fn main() {
    var tot = 0
    for i in 0..400 {
        var a = [i, i + 1, i + 2, i + 3]
        let b = a
        tot = tot + a[0] + a[3]
        let r = a.pop()
        a = r[0]
        tot = tot + len(a) + len(b) + b[3] + jumlah(a) + jumlah(b)
        var c = [1, 2, 3]
        tot = tot + buang_akhir(c) + len(c) + c[2]
        var d = [7, 8, 9]
        let e = arr::pop(d)
        tot = tot + len(d) + d[2] + len(e[0])
    }
    wok(tot)
    var q = [1, 2, 3, 4, 5, 6, 7, 8]
    var k = 0
    while (len(q) > 0) {
        k = k + q[len(q) - 1] * len(q)
        let r = q.pop()
        q = r[0]
    }
    wok(k, q)
}
"#;

#[test]
fn pop_pindah_tidak_teramati() {
    let v = jalankan(POP_PINDAH, &["--vm"], &[]);
    assert!(v.sukses && v.stdout == "815600\n204 []", "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(POP_PINDAH, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(POP_PINDAH, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: buang_akhir, jumlah, main"), "{}", j.stderr);
}

/// `len(lokal)` dibaca dari lokal asalnya (`Cara::PanjangLokal`) dan `char_at` atas teks lokal: teks ASCII,
/// non-ASCII di awal dan di ujung, kosong, larik, tuple, dict, sel tangkapan, lokal berganti jenis, dan galat
/// `len(5)` yang dilaporkan VM di tempat yang sama.
const LEN_LOKAL: &str = r#"package main

fn hitung(s) -> int {
    var n = 0
    for i in 0..len(s) {
        n = n + 1
    }
    return n + len(s)
}

fn kar(s: str) -> int {
    var acc = 0
    for i in 0..len(s) {
        acc = acc * 31 + (str::char_at(s, i) as int)
        acc = acc % 1000003
    }
    return acc
}

pub fn main() {
    var tot = 0
    let teks = ["abc", "", "héllo wörld", "日本語テキスト", "abcdefghijklmnopqrstuvwxyz0123456789", "aé", "1234567é"]
    for r in 0..3000 {
        let s = teks[r % 7]
        tot = tot + hitung(s) + kar(s)
        tot = tot + hitung([1, 2, r]) + hitung((1, 2)) + hitung(dict { a: 1, b: r })
    }
    wok(tot)
    var c = "tangkap"
    let f = || len(c)
    for r in 0..2000 {
        if r == 1000 { c = "tangkapé!" }
        tot = tot + f() + len(c)
    }
    wok(tot)
    var x = "awal"
    for r in 0..3000 {
        if r == 2500 { x = [1, 2, 3, 4] }
        tot = tot + len(x)
    }
    wok(tot)
    var y = "ok"
    for r in 0..3000 {
        if r == 2900 { y = 5 }
        tot = tot + len(y)
    }
    wok(tot)
}
"#;

#[test]
fn len_lokal_sama_dengan_vm() {
    let v = jalankan(LEN_LOKAL, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout == "762344621\n762376621\n762388621", "{}", v.stdout);
    assert!(v.stderr.contains("tidak bisa ukur `int`"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(LEN_LOKAL, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(LEN_LOKAL, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: <closure>, hitung, kar, main"), "{}", j.stderr);
}

/// `char_at` atas teks lokal di callee ringan (`karakter_lokal`, galat = deopt): indeks negatif, non-ASCII,
/// sel tangkapan, dan galat di luar batas yang jejaknya menyebut pemanggil seperti VM.
const CHAR_AT_RINGAN: &str = r#"package main

fn jumlah(s: str, d: int) -> int {
    var acc = 0
    for i in 0..len(s) {
        acc = acc * 7 + (str::char_at(s, i) as int) + (str::char_at(s, -1 - (i % len(s))) as int) * d
        acc = acc % 1000003
    }
    return acc
}

fn ambil(s: str, i: int) -> int {
    return str::char_at(s, i) as int
}

pub fn main() {
    var tot = 0
    let teks = ["abc", "héllo wörld", "日本語テキスト", "abcdefghijklmnopqrstuvwxyz0123456789", "aé", "1234567é", "x"]
    for r in 0..4000 {
        let s = teks[r % 7]
        tot = (tot + jumlah(s, r % 3) + ambil(s, r % len(s)) + ambil(s, -1)) % 1000000007
    }
    wok(tot)
    var c = "sél"
    let f = |i| str::char_at(c, i)
    for r in 0..2000 {
        tot = tot + (f(r % 3) as int) + (f(-(r % 3) - 1) as int)
    }
    wok(tot)
    for r in 0..3000 {
        let s = teks[r % 7]
        tot = tot + ambil(s, r % 40)
    }
    wok(tot)
}
"#;

#[test]
fn char_at_ringan_sama_dengan_vm() {
    let v = jalankan(CHAR_AT_RINGAN, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout == "388542044\n389150125", "{}", v.stdout);
    assert!(v.stderr.contains("index 4 di luar batas") && v.stderr.contains("`ambil` dipanggil"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(CHAR_AT_RINGAN, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(CHAR_AT_RINGAN, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("ambil, jumlah, main"), "{}", j.stderr);
}

/// `len` teks lewat `vm::value::jumlah_karakter` (teks pendek ASCII lewat `prefiks_ascii`): karakter non-ASCII di
/// tiap posisi teks 0 sampai 40 byte, lewat `len`, `s.len()`, `str::len` dan `char_at`, sama dengan interpreter.
const JUMLAH_KARAKTER: &str = r#"package main

fn cek(s: str) -> int {
    var h = len(s) * 1000 + s.len()
    for i in 0..len(s) {
        h = (h * 31 + (str::char_at(s, i) as int)) % 1000003
    }
    return h
}

pub fn main() {
    var tot = 0
    for n in 0..41 {
        var dasar = ""
        for k in 0..n { dasar = dasar + "a" }
        tot = (tot + cek(dasar)) % 1000000007
        for p in 0..n {
            var s = ""
            for k in 0..n { s = s + (k == p $ "é" : "b") }
            tot = (tot * 7 + cek(s) + str::len(s)) % 1000000007
        }
    }
    wok(tot)
}
"#;

#[test]
fn jumlah_karakter_sama_dengan_interpreter() {
    let i = jalankan(JUMLAH_KARAKTER, &[], &[]);
    assert!(i.sukses && i.stdout == "908523536", "{}", i.stdout);
    for (arg, env) in [("--vm", &[][..]), ("--jit", &[STRES][..]), ("--jit", &[STRES, ACAK][..]), ("--jit", &[][..])] {
        let j = jalankan(JUMLAH_KARAKTER, &[arg], env);
        assert_eq!((i.sukses, &i.stdout), (j.sukses, &j.stdout), "{arg} {env:?}");
    }
}

/// Elemen tuple lewat jalur larik (`lambat::baca`): payload varian berfield banyak, indeks negatif, tuple di
/// sel tangkapan, elemen bukan bulat (spekulasi gagal), dan galat di luar batas seperti VM.
const TUPLE_INDEKS: &str = r#"package main

enum E {
    Dua(int, int),
    Tiga(int, float, int),
    Campur(int, string),
}

fn nilai(e: E, k: int) -> int {
    match e {
        E::Dua(a, b) => a * 3 + b + k,
        E::Tiga(a, x, c) => a + (x as int) + c,
        E::Campur(a, s) => a + len(s),
    }
}

fn pilih(t, i: int) -> int {
    return t[i] + t[-1]
}

pub fn main() {
    var tot = 0
    for r in 0..5000 {
        let e = r % 3 == 0 $ E::Dua(r, r % 7) : (r % 3 == 1 $ E::Tiga(r, 2.5, 4) : E::Campur(r, "abc"))
        tot = (tot + nilai(e, r)) % 1000000007
        let t = (r, r * 2, r % 5)
        tot = tot + pilih(t, r % 3) + pilih(t, -(r % 3) - 1)
    }
    wok(tot)
    var c = (1, 2, 3)
    let f = |i| c[i]
    for r in 0..2000 {
        if r == 1000 { c = (4, 5, 6) }
        tot = tot + f(r % 3) + c[r % 3]
    }
    wok(tot)
    var m = (1, "x", 3)
    for r in 0..3000 {
        if r == 1500 { m = (1, 2, 3) }
        tot = tot + pilih(m, 2) + (r % 2 == 0 $ 1 : 0)
    }
    wok(tot)
    for r in 0..3000 {
        let t = (r, r)
        tot = tot + pilih(t, r % 4)
    }
    wok(tot)
}
"#;

#[test]
fn tuple_indeks_sama_dengan_vm() {
    let v = jalankan(TUPLE_INDEKS, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout == "50041662\n50055660\n50075160", "{}", v.stdout);
    assert!(v.stderr.contains("tuple ini isinya 2 elemen"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(TUPLE_INDEKS, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(TUPLE_INDEKS, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("main, nilai, pilih"), "{}", j.stderr);
}

/// Pemeriksaan jenis dan panjang payload langsung dari lokal (`Cara::PayloadSama`/`PayloadPanjang`): varian
/// berfield satu sampai tiga, payload tuple, teks, varian tanpa payload, `Option` atas tuple, enum di sel tangkapan.
const PAYLOAD_GABUNG: &str = r#"package main

enum B {
    Satu(int),
    Dua(int, int),
    Tiga(int, int, int),
    Tup((int, int)),
    Teks(string, int),
    Kosong,
}

fn nilai(b: B) -> int {
    match b {
        B::Satu(a) => a,
        B::Dua(a, c) => a * 2 + c,
        B::Tiga(a, c, d) => a + c * 3 + d,
        B::Tup(t) => t[0] - t[1],
        B::Teks(s, k) => len(s) * k,
        B::Kosong => 7,
    }
}

fn opsi(o) -> int {
    match o {
        Some(x) => typeof(x) == "tuple" $ x[0] + x[1] : x,
        None => -1,
    }
}

pub fn main() {
    var tot = 0
    for r in 0..6000 {
        let k = r % 6
        let b = k == 0 $ B::Satu(r) : (k == 1 $ B::Dua(r, 3) : (k == 2 $ B::Tiga(r, 1, 2) : (k == 3 $ B::Tup((r, 5)) : (k == 4 $ B::Teks("abé", r) : B::Kosong))))
        tot = (tot + nilai(b)) % 1000000007
        tot = tot + opsi(r % 3 == 0 $ Some((r, 1)) : (r % 3 == 1 $ Some(r) : None))
    }
    wok(tot)
    var c = B::Dua(1, 2)
    let f = || nilai(c)
    for r in 0..2000 {
        if r == 1000 { c = B::Tiga(1, 2, 3) }
        tot = tot + f()
    }
    wok(tot)
}
"#;

#[test]
fn payload_gabung_sama_dengan_vm() {
    let v = jalankan(PAYLOAD_GABUNG, &["--vm"], &[]);
    assert!(v.sukses && v.stdout == "36001000\n36015000", "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(PAYLOAD_GABUNG, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(PAYLOAD_GABUNG, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: <closure>, main, nilai, opsi"), "{}", j.stderr);
}

/// Hasil angka callee ringan dipakai dari register (`keluar_ringan_angka`): bulat, pecahan, hasil yang kadang
/// berganti jenis (jalur `keluar_ringan` biasa), dan luapan 64 bit di callee (deopt).
const RINGAN_ANGKA: &str = r#"package main

struct P {
    pub x: float,
    pub y: float,
    pub z: float,
}

fn f(a: int, b: int) -> int {
    return a * 3 + b
}

fn g(p: P, k: float) -> float {
    return p.x * k + p.y - p.z
}

fn campur(a: int) {
    if a % 97 == 0 { return 0.5 }
    if a % 89 == 0 { return "t" }
    return a + 1
}

fn besar(a: int) -> int {
    return a * 4611686018427387
}

pub fn main() {
    var tot = 0
    var ft = 0.0
    let p = P { x: 1.5, y: 2.25, z: 0.125 }
    for i in 0..20000 {
        tot = (tot + f(i, 7)) % 1000000007
        ft = ft + g(p, i as float * 0.001)
        let c = campur(i)
        if typeof(c) == "int" { tot = tot + c } else { wok("lain", i, c) }
        tot = (tot + besar(i % 3000) % 1000) % 1000000007
    }
    wok(tot, ft)
}
"#;

#[test]
fn ringan_angka_sama_dengan_vm() {
    let v = jalankan(RINGAN_ANGKA, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.ends_with("805824533 342484.99999999994"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(RINGAN_ANGKA, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(RINGAN_ANGKA, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: besar, campur, f, g, main"), "{}", j.stderr);
}

/// `int`/`uint` atas bulat 128 bit di register: hasil yang muat 64 bit (spekulasi), nilai di luar 64 bit (tetap
/// `Lebar`), dan `uint` atas bilangan negatif yang dilaporkan VM di tempat yang sama.
const UBAH_LEBAR: &str = r#"package main

fn putar(n: int) -> int {
    var seed: uint = 88172645463325252
    var tot: int = 0
    for i in 0..n {
        seed = math::wrap_add(math::wrap_mul(seed, 6364136223846793005), 1442695040888963407)
        let a = (seed >> 1) as int
        let b = int(seed)
        let c = (seed >> 60) as uint
        tot = (tot + a % 1000 + b % 777 + (c as int)) % 1000000007
    }
    return tot
}

fn lebar(n: int) -> int {
    var x = 170141183460469231731687303715884105
    var tot = 0
    for i in 0..n {
        x = x + i
        let y = int(x) % 1000
        tot = tot + y + (uint(i) as int)
    }
    return tot
}

pub fn main() {
    wok(putar(5000), lebar(3000))
    var z = 5
    for i in 0..3000 {
        z = z - 1
        if i > 2990 { wok(uint(z)) }
    }
}
"#;

#[test]
fn ubah_lebar_sama_dengan_vm() {
    let v = jalankan(UBAH_LEBAR, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout == "4484277 5991000", "{}", v.stdout);
    assert!(v.stderr.contains("tidak muat di `uint`"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(UBAH_LEBAR, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(UBAH_LEBAR, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: lebar, main, putar"), "{}", j.stderr);
}

/// Varian enum tanpa payload sebagai konstanta maya (`Jenis::Konst`): disimpan ke lokal, dibandingkan, jadi
/// argumen, dikembalikan, masuk struct dan larik, ditangkap closure, berganti di cabang dan loop.
const KONST_ENUM: &str = r#"package main

enum W {
    Merah,
    Hijau,
    Biru,
    Isi(int),
}

struct K {
    pub w: W,
    pub n: int,
}

fn kode(w: W) -> int {
    match w {
        W::Merah => 1,
        W::Hijau => 2,
        W::Biru => 3,
        W::Isi(k) => k,
    }
}

fn pilih(i: int) -> W {
    if i % 3 == 0 { return W::Merah }
    var w = W::Hijau
    if i % 5 == 0 { w = W::Biru }
    if i % 7 == 0 { w = W::Isi(i) }
    return w
}

pub fn main() {
    var tot = 0
    var c = W::Merah
    let f = || kode(c)
    var daftar = []
    for i in 0..6000 {
        var w = W::Biru
        var u = W::Merah
        if i % 2 == 0 { w = W::Hijau } else if i % 3 == 0 { w = W::Isi(i % 50) }
        if w == W::Hijau { tot = tot + 1 }
        if u != w { tot = tot + 2 }
        let v = w
        tot = tot + kode(v) + kode(pilih(i)) + kode(u)
        let k = K { w: W::Biru, n: i }
        tot = tot + kode(k.w)
        if i % 1000 == 0 { daftar = daftar + [W::Merah, w]; c = W::Biru }
        tot = (tot + f()) % 1000000007
        u = w
        tot = tot + kode(u)
    }
    wok(tot, len(daftar), daftar[3], c)
}
"#;

#[test]
fn konst_enum_sama_dengan_vm() {
    let v = jalankan(KONST_ENUM, &["--vm"], &[]);
    assert!(v.sukses && v.stdout == "1858258 12 W::Hijau W::Biru", "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(KONST_ENUM, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(KONST_ENUM, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: <closure>, kode, main, pilih"), "{}", j.stderr);
}

/// Callee bulat berkonvensi register (`turun::tanda_tangan_bulat`): rekursi, tiga argumen, luapan ke 128 bit di
/// callee (deopt), rekursi melewati batas kedalaman native, dan galat `% 0` di callee yang dilaporkan VM.
const REGISTER_BULAT: &str = r#"package main

fn fib(n: int) -> int {
    if n < 2 { return n }
    return fib(n - 1) + fib(n - 2)
}

fn kali(a: int, b: int, c: int) -> int {
    return a * b + c
}

fn besar(n: int) -> int {
    if n == 0 { return 1 }
    return besar(n - 1) * 3
}

fn dalam(n: int) -> int {
    if n == 0 { return 0 }
    return dalam(n - 1) + 1
}

fn bagi(a: int, b: int) -> int {
    return a % b + kali(a, b, 1)
}

pub fn main() {
    var t = 0
    for i in 0..300 {
        t = t + fib(i % 18) + kali(i, i + 1, i + 2) + besar(i % 50) % 1000003
    }
    wok(t, besar(45), besar(75))
    wok(dalam(50000))
    var u = 0
    for i in 0..2000 { u = u + bagi(i + 7, 5 - i % 6) }
    wok(u)
}
"#;

#[test]
fn register_bulat_sama_dengan_vm() {
    let v = jalankan(REGISTER_BULAT, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout == "128268382 2954312706550833698643 608266787713357709119683992618861307\n50000", "{}", v.stdout);
    assert!(v.stderr.contains("`12 % 0`"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(REGISTER_BULAT, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(REGISTER_BULAT, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: bagi, besar, dalam, fib, kali"), "{}", j.stderr);
}

/// `x = f(..)` lewat callee ringan berhasil milik VM (`keluar_ringan_simpan`): struct yang dipakai ulang, lokal yang
/// jadi sel tangkapan, hasil yang kadang teks atau bulat (jalur `keluar_ringan` biasa), dan luapan di callee.
const RINGAN_SIMPAN: &str = r#"package main

struct P {
    pub x: float,
    pub y: float,
}

impl P {
    fn maju(self, d: float) -> P {
        return P { x: self.x + d, y: self.y * 0.5 + d }
    }
}

fn ganti(p: P, i: int) {
    if i % 333 == 0 { return "teks" }
    if i % 250 == 0 { return i }
    return P { x: p.y, y: p.x }
}

fn besar(n: int) {
    return n * 4611686018427387
}

pub fn main() {
    var p = P { x: 1.0, y: 2.0 }
    var q = P { x: 0.5, y: 0.25 }
    var lain = P { x: 0.0, y: 0.0 }
    let f = || q.x + q.y
    var tot = 0.0
    var b = 0
    for i in 0..5000 {
        p = p.maju(0.001)
        q = q.maju(0.002)
        tot = tot + f()
        lain = ganti(p, i)
        if typeof(lain) == "string" || typeof(lain) == "int" { wok(i, lain); lain = p }
        b = besar(i % 5000)
        tot = tot + lain.x
    }
    wok(p.x, p.y, q.x, tot, b)
}
"#;

#[test]
fn ringan_simpan_sama_dengan_vm() {
    let v = jalankan(RINGAN_SIMPAN, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.ends_with("27658.66999999996 23053818406118507613"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(RINGAN_SIMPAN, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(RINGAN_SIMPAN, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("P::maju, besar, ganti, main"), "{}", j.stderr);
}

/// `str(n)` atas bulat native (`Cara::TeksBulat`) dan situs bawaan yang polosnya diputuskan saat kompilasi
/// (`lambat::bawaan_polos`): bulat negatif dan `i64::MIN`, bawaan teks dan larik, dan galat bawaan seperti VM.
const BAWAAN_SITUS: &str = r#"package main

fn label(n: int) -> string {
    return "n" + str(n) + ":" + str(n % 7 - 3)
}

pub fn main() {
    var h = 0
    var s = ""
    for i in 0..4000 {
        let k = i * 7919 - 20000
        s = label(k)
        h = (h * 31 + len(s) + len(str::upper(s)) + len(str::replace(s, "-", "_"))) % 1000003
        if i % 999 == 0 { wok(s, str(-9223372036854775807 - 1), str(i), str::reverse(s)) }
        h = h + len(str::repeat("ab", i % 3)) + len(arr::join(["a", s], "-"))
    }
    wok(h)
    for i in 0..3000 {
        if i == 2999 { wok(str::repeat("x", -1)) }
        h = h + len(str::trim("  a  "))
    }
}
"#;

#[test]
fn bawaan_situs_sama_dengan_vm() {
    let v = jalankan(BAWAAN_SITUS, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout.ends_with("16069"), "{}", v.stdout);
    assert!(v.stderr.contains("sebanyak -1 kali"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(BAWAAN_SITUS, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    let j = jalankan(BAWAAN_SITUS, &["--jit"], &[STRES, ("JRG_JIT_STRES", "1"), ("JRG_JIT_LOG", "ringkas")]);
    assert!(j.stderr.contains("dikompilasi: label, main"), "{}", j.stderr);
}

/// `+` atas dua teks milik VM (`lambat::tambah_nilai`): sambungan berantai, teks yang tumbuh di tempat dan yang
/// dipegang dua nama, lalu yang lewat `biner` — teks + bulat, teks + struct ber-`to_string`, operator `add`.
const TAMBAH_TEKS: &str = r#"package main

struct V {
    pub x: int,
}

impl V {
    fn to_string(self) -> string { return "V" + str(self.x) }
    fn add(self, o: V) -> V { return V { x: self.x + o.x } }
}

pub fn main() {
    var s = ""
    var h = 0
    let v = V { x: 3 }
    var w = V { x: 0 }
    for i in 0..3000 {
        let a = "k" + str(i % 13)
        let b = a + "-" + a
        let salin = s
        s = s + b
        if i % 500 == 0 { s = "" }
        let c = "v=" + v
        let d = "n:" + (i % 5)
        w = w + v
        h = (h * 31 + len(b) + len(c) + len(d) + len(salin)) % 1000003
    }
    wok(h, len(s), w.x, "akhir" + w)
}
"#;

#[test]
fn tambah_teks_sama_dengan_vm() {
    let v = jalankan(TAMBAH_TEKS, &["--vm"], &[]);
    assert!(v.sukses && v.stdout == "393208 2723 9000 akhirV9000", "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(TAMBAH_TEKS, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Geser konstan atas bulat 128 bit di register (dua geser 64 bit): `>>` dan `<<` 1 sampai 63, bilangan negatif besar,
/// dan `<<` yang membuang bit sampai `-1` — sama dengan VM.
const GESER_LEBAR: &str = r#"package main

fn geser(n: int) -> int {
    var x: uint = 305419896
    var y = -170141183460469231731687303715884105
    var tot = 0
    for i in 0..n {
        x = math::wrap_add(math::wrap_mul(x, 1664525), 1013904223) ^ (x >> 13)
        let a = (x >> 1) ^ (x >> 63) ^ (x << 3)
        let b = (y >> 7) + (y >> 63) + (y >> 1)
        y = y + 1
        let c = (x << 63) >> 60
        tot = (tot + (a % 1000) + (b % 1000) + (c % 1000)) % 1000000007
    }
    return tot
}

pub fn main() {
    wok(geser(20000))
    var z = 1
    for i in 0..200 {
        z = (z << 1) + 1
        if i % 37 == 0 { wok(i, z >> 5) }
    }
}
"#;

#[test]
fn geser_lebar_sama_dengan_vm() {
    let v = jalankan(GESER_LEBAR, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.starts_with("9685561\n0 0\n37 17179869183"), "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(GESER_LEBAR, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Pintu VM ke kode native (`panggil::jalankan`): bingkai lebih dari 32 posisi (penyangga argumen di heap), hasil
/// teks milik VM, luapan 64 bit di dalamnya (deopt), dan closure yang dipanggil `arr::map`.
const PINTU_VM: &str = r#"package main

fn banyak(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31, a32, a33, a34, a35) -> int {
    var t = 0
    for i in 0..3 { t = t + a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15 + a16 + a17 + a18 + a19 + a20 + a21 + a22 + a23 + a24 + a25 + a26 + a27 + a28 + a29 + a30 + a31 + a32 + a33 + a34 + a35 }
    return t
}

fn teks(n: int) {
    var s = ""
    for i in 0..n { s = s + "x" }
    return s
}

fn luap(n: int) -> int {
    var x = n
    for i in 0..2 { x = x * 4611686018427387 }
    return x % 1000003
}

pub fn main() {
    var tot = 0
    for r in 0..3000 {
        tot = (tot + banyak(r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r) + len(teks(r % 9)) + luap(r)) % 1000000007
    }
    let f = |x| x * 2 + tot % 5
    wok(tot, arr::sum(arr::map(range(0, 2000), f)))
}
"#;

#[test]
fn pintu_vm_sama_dengan_vm() {
    let v = jalankan(PINTU_VM, &["--vm"], &[]);
    assert!(v.sukses && v.stdout == "987727185 3998000", "{}", v.stdout);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(PINTU_VM, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Closure pendek yang cuma membaca tangkapan (`MuatUpval`) ikut dikompilasi: tangkapan bulat, pecahan dan teks yang
/// berubah di antara panggilan, `map`/`filter`/`reduce`, dan tangkapan yang berganti jenis (galat yang sama dengan VM).
const UPVAL_PENDEK: &str = r#"package main

pub fn main() {
    var k = 3
    var f = 0.5
    var nama = "ab"
    let tambah = |x| x * 2 + k
    let kali = |x| x * f
    let panjang = |x| x + len(nama)
    let xs = range(0, 5000)
    let ys = xs.map(|x| x * 2 + k).filter(|x| x % 3 == 0)
    wok(ys.reduce(|a, b| a + b, 0))
    var tot = 0
    var tf = 0.0
    for i in 0..30000 {
        tot = (tot + tambah(i) + panjang(i)) % 1000000007
        tf = tf + kali(i as float)
        if i % 7000 == 0 { k = k + 1; f = f * 2.0; nama = nama + "c" }
        if i == 20000 { k = "teks" }
        if i == 20001 { k = 4 }
    }
    wok(tot, tf)
}
"#;

#[test]
fn upval_pendek_sama_dengan_vm() {
    let v = jalankan(UPVAL_PENDEK, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout == "8336667", "{}", v.stdout);
    assert!(v.stderr.contains("tidak bisa `%` antara `string` dan `int`"), "{}", v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(UPVAL_PENDEK, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}

/// Method berumus sebaris (`Cara::UbahLokal`/`Ubah`/`RumusLokal`, langkah 81–82): pembaruan biasa, salinan yang
/// dibagi, field bulat yang dibaca rumus (deopt) atau cuma ditulis, lokal yang ditangkap closure, hasil ke lokal
/// lain, rumus nilai dengan field bulat dan field yang jenisnya berganti.
const UBAH: &str = r#"struct P { pub x: f64, pub v: f64, pub nama: string, pub n: int }

impl P {
    fn maju(self, dt: f64) -> P {
        return P { x: self.x + self.v * dt, v: self.v - self.x * 0.5, nama: self.nama, n: self.n }
    }
    fn setel(self, k: f64) -> P {
        return P { x: -self.x, v: k / 4.0, nama: self.nama, n: self.n }
    }
}

// Pembaruan biasa di loop panas.
fn biasa() {
    var p = P { x: 1.0, v: 0.5, nama: "a", n: 3 }
    for i in 0..3000 { p = p.maju(0.01) }
    wok(p.x, p.v, p.nama, p.n)
}

// Salinan yang dibagi tidak ikut berubah.
fn dibagi() {
    var q = P { x: 2.0, v: 1.0, nama: "b", n: 4 }
    var simpan = []
    for i in 0..3000 {
        if i % 1000 == 0 { simpan = simpan + [q] }
        q = q.maju(0.02)
    }
    wok(q.x, simpan[0].x, simpan[1].x, simpan[2].x)
}

// Field bulat yang dibaca rumus: spekulasinya gagal di tengah jalan.
fn bulat(x0) {
    var r = P { x: 1.0, v: 2.0, nama: "c", n: 5 }
    for i in 0..3000 {
        if i == 2000 { r = P { x: x0, v: 2.0, nama: "c", n: 5 } }
        r = r.maju(0.001)
    }
    wok(r.x, r.v)
}

// Field bulat yang cuma ditulis: diganti pecahan.
fn tulis_saja() {
    var w = P { x: 1.5, v: 7.0, nama: "d", n: 6 }
    for i in 0..3000 {
        if i == 2000 { w = P { x: 1.5, v: 7, nama: "d", n: 6 } }
        w = w.setel(1.0)
    }
    wok(w.x, w.v, typeof(w.v))
}

// Lokal yang ditangkap closure: lewat sel, closure melihat pembaruannya.
fn tangkap() {
    var c = P { x: 3.0, v: 1.0, nama: "e", n: 7 }
    let lihat = || c.x
    for i in 0..3000 { c = c.maju(0.0001) }
    wok(lihat(), c.x)
}

// Hasil ke lokal lain: penerimanya disalin, asalnya tetap.
fn lain() {
    let a = P { x: 1.0, v: 1.0, nama: "f", n: 8 }
    var t = 0.0
    for i in 0..3000 { let b = a.maju(0.1); t = t + b.x }
    wok(a.x, t)
}

// Rumus nilai (`Cara::RumusLokal`): field bulat lewat `as f64`; field berjenis lain di tengah jalan.
struct R { pub a: f64, pub w: int }

impl R {
    fn skor(self) -> f64 { return self.a * (1.0 + (self.w as f64) * 0.01) - 0.5 }
}

fn nilai_rumus(x) {
    var t = 0.0
    for i in 0..3000 {
        var r = R { a: (i as f64) * 0.5, w: i % 7 }
        if i == 2000 { r = R { a: x, w: 3 } }
        t = t + r.skor()
    }
    wok(t)
}

fn main() {
    biasa()
    dibagi()
    bulat(1)
    tulis_saja()
    tangkap()
    lain()
    nilai_rumus(2)
}
"#;

#[test]
fn ubah_sama_dengan_vm() {
    let v = jalankan(UBAH, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.lines().count() == 7, "{}{}", v.stdout, v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(UBAH, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    // Kelima pembaruan di tempat dan satu ke lokal lain memang lewat rumus (bukan panggilan biasa).
    let r = jalankan(UBAH, &["--jit"], &[("JRG_JIT_LOG", "rencana")]);
    assert!(r.stderr.matches("UbahLokal").count() >= 5 && r.stderr.contains(" Ubah(") && r.stderr.contains("RumusLokal"),
        "{}", r.stderr);
}

/// Situs `Call` yang belum dijalankan VM (loop sesudah titik OSR) meminjam jenis hasil situs lain ke callee yang
/// sama (langkah 83): `hash` tetap bilangan native di loop kedua; pinjaman yang salah (hasil di luar 64 bit) deopt.
const PINJAM_SPEK: &str = r#"fn fnv(h: uint, s: str) -> uint {
    var acc: uint = h
    for i in 0..len(s) {
        acc = math::wrap_mul(acc ^ (str::char_at(s, i) as uint), 1099511628211)
    }
    return acc
}

fn kali(x, k) { return x * k }

fn main() {
    var hash: uint = 1469598103934665603
    for r in 0..3000 { hash = fnv(hash, "abcdefgh") }
    for r in 0..3000 { hash = fnv(hash, "héllo ☃ 𝔘") }
    wok(hash)
    // Situs kedua meminjam jenis hasil bulat dari situs pertama, lalu hasilnya di luar 64 bit.
    var a = 0
    for r in 0..3000 { a = a + kali(r, 3) }
    var b = 0
    for r in 0..3000 { b = b + kali(r, 9223372036854775807) }
    wok(a, b)
}
"#;

#[test]
fn pinjam_spek_sama_dengan_vm() {
    let v = jalankan(PINJAM_SPEK, &["--vm"], &[]);
    assert!(v.sukses && v.stdout.lines().count() == 2, "{}{}", v.stdout, v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(PINJAM_SPEK, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
    // Tanpa pinjaman, `fnv` dari loop kedua dikompilasi dengan `h` milik VM.
    let r = jalankan(PINJAM_SPEK, &["--jit"], &[("JRG_JIT_LOG", "rencana")]);
    assert!(r.stderr.contains("rencana fnv") && !r.stderr.contains("LoadLocal 0 Muat [Nilai, Nilai]"), "{}", r.stderr);
}

/// Sesi panggilan HOF (`jit::panggil::SesiHof`, langkah 84): jenis elemen yang berganti di tengah larik, closure yang
/// menulis tangkapannya, HOF bersarang, callback rekursif dan berhasil struct, bentuk method dan namespace, galat di
/// tengah larik (kalimat dan jejaknya).
const SESI_HOF: &str = r#"struct T { pub a: int }

fn fakt(n) { if n < 2 { return 1 } return n * fakt(n - 1) }

fn main() {
    let xs = range(0, 3000)
    // Jenis elemen berganti di tengah: sesinya ditutup, panggilan biasa, lalu dibuka lagi.
    var campur = []
    for i in 0..3000 { campur = campur + [(i % 500 == 0) $ (i as float) : i] }
    wok(campur.map(|x| x * 2).reduce(|a, b| a + b, 0))
    // Closure yang menulis tangkapannya.
    var n = 0
    xs.each(|x| { n = n + x })
    wok(n)
    // HOF bersarang di dalam callback.
    wok(range(0, 300).map(|i| range(0, i % 7).map(|j| j * i).reduce(|a, b| a + b, 0)).reduce(|a, b| a + b, 0))
    // Callback rekursif dan hasil struct.
    wok(range(0, 2000).map(|i| fakt(i % 10)).reduce(|a, b| a + b, 0))
    wok(xs.map(|i| T { a: i }).filter(|t| t.a % 999 == 0).map(|t| t.a))
    // any/all/count/fold/find.
    wok(xs.any(|x| x == 2999), xs.all(|x| x < 2999), xs.count_if(|x| x % 7 == 0), xs.fold(1, |a, b| (a + b) % 1000003))
    wok(arr::map(xs, |x| x + 1)[2999], arr::filter(xs, |x| x % 1000 == 0), arr::reduce(xs, |a, b| a + b, 0))
    // Galat di tengah larik: keluaran sebelumnya tetap, kalimat dan jejaknya sama.
    wok(xs.map(|x| 10 / (2500 - x)).reduce(|a, b| a + b, 0))
}
"#;

#[test]
fn sesi_hof_sama_dengan_vm() {
    let v = jalankan(SESI_HOF, &["--vm"], &[]);
    assert!(!v.sukses && v.stdout.lines().count() == 7 && v.stderr.contains("jejak"), "{}{}", v.stdout, v.stderr);
    for env in [&[STRES][..], &[STRES, ACAK][..], &[][..]] {
        let j = jalankan(SESI_HOF, &["--jit"], env);
        assert_eq!((v.sukses, &v.stdout, &v.stderr), (j.sukses, &j.stdout, &j.stderr), "{env:?}");
    }
}
