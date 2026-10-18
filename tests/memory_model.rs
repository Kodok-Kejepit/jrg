// tests/memory_model.rs

mod bersama;

use std::time::Instant;

use jrgc::interpreter::interpret_with_paths;
use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;

const STACK_SIZE: usize = 32 * 1024 * 1024;

fn jalankan(src: &'static str) -> Result<(), String> {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let sf = SourceFile { id: 0, name: "<memory>".to_string(), src: src.to_string() };
            let tokens = lex(&sf).map_err(|e| format!("lex: {}", e.msg))?;
            let (ast, errors) = parse(tokens, false);
            if !errors.is_empty() {
                return Err(format!("parse: {:?}", errors.iter().map(|e| &e.msg).collect::<Vec<_>>()));
            }
            interpret_with_paths(&ast, Vec::new()).map_err(|e| format!("runtime: {}", e.message))
        })
        .expect("spawn")
        .join()
        .expect("thread panik")
}

/// Waktu menjalankan `src`, diambil yang tercepat dari beberapa kali coba
/// supaya derau penjadwalan tidak bikin test ini rewel.
fn tercepat(src: &'static str, kali: usize) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..kali {
        let t = Instant::now();
        jalankan(src).expect("program harus jalan tanpa error");
        best = best.min(t.elapsed().as_secs_f64());
    }
    best
}

/// `a[i] = v` harus berbiaya tetap, bukan sebanding ukuran array.
#[test]
fn assignment_lewat_index_tidak_menyalin_seluruh_array() {
    const KECIL: &str = r#"
package main
pub fn main() {
    var a = []
    var i = 0
    while i < 4000 { a = a + [0]; i = i + 1 }
    var j = 0
    while j < 4000 { a[j] = j; j = j + 1 }
    assert_eq(a[3999], 3999)
}
"#;
    const BESAR: &str = r#"
package main
pub fn main() {
    var a = []
    var i = 0
    while i < 8000 { a = a + [0]; i = i + 1 }
    var j = 0
    while j < 8000 { a[j] = j; j = j + 1 }
    assert_eq(a[7999], 7999)
}
"#;
    // Kedua program membangun arraynya dengan cara yang sama, jadi yang
    // dibandingkan adalah rasio total.
    let kecil = tercepat(KECIL, 2);
    let besar = tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 12.0,
        "menulis lewat index sepertinya kembali menyalin seluruh array: \
         n=4000 {:.3}s, n=8000 {:.3}s (rasio {:.1}x, ambang 12x)",
        kecil, besar, rasio
    );
}

/// Menyerahkan agregat TIDAK boleh ikut ukurannya.
#[test]
fn menyerahkan_agregat_tidak_ikut_ukurannya() {
    const KECIL: &str = r#"
package main
fn balik(x) { return x }
pub fn main() {
    let a = range(500)
    var s = 0
    var i = 0
    while i < 20000 {
        s = s + len(a) + a.len() + a[0]
        var b = a
        let c = balik(a)
        i = i + 1
    }
    assert(s > 0)
}
"#;
    const BESAR: &str = r#"
package main
fn balik(x) { return x }
pub fn main() {
    let a = range(4000)
    var s = 0
    var i = 0
    while i < 20000 {
        s = s + len(a) + a.len() + a[0]
        var b = a
        let c = balik(a)
        i = i + 1
    }
    assert(s > 0)
}
"#;
    let kecil = tercepat(KECIL, 2);
    let besar = tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 3.0,
        "menyerahkan agregat mulai ikut ukurannya lagi: \
         ukuran 500 {:.3}s, ukuran 4000 {:.3}s (rasio {:.1}x untuk 8x ukuran, \
         ambang 3x). O(1) ~1x, O(ukuran) ~8x — penyebab paling mungkin: nilai \
         agregat kembali disalin saat berpindah, bukan saat ditulis.",
        kecil, besar, rasio
    );
}

/// Menumbuhkan koleksi lewat `a = a + [x]` harus LINEAR terhadap n.
#[test]
fn menumbuhkan_koleksi_tetap_linear() {
    const KECIL: &str = r#"
package main
pub fn main() {
    var a = []
    var i = 0
    while i < 4000 { a = a + [i]; i = i + 1 }
    assert_eq(len(a), 4000)
    assert_eq(a[3999], 3999)
}
"#;
    const BESAR: &str = r#"
package main
pub fn main() {
    var a = []
    var i = 0
    while i < 32000 { a = a + [i]; i = i + 1 }
    assert_eq(len(a), 32000)
    assert_eq(a[31999], 31999)
}
"#;
    let kecil = tercepat(KECIL, 2);
    let besar = tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 24.0,
        "menumbuhkan koleksi sepertinya kembali kuadratik: \
         n=4000 {:.3}s, n=32000 {:.3}s (rasio {:.1}x untuk 8x n, ambang 24x). \
         Linear ~8x, kuadratik ~64x.",
        kecil, besar, rasio
    );
}

/// Biaya MEMANGGIL closure tidak boleh bergantung pada besar scope di sekitar
/// tempat closure itu dibuat.
#[test]
fn biaya_panggil_closure_tidak_bergantung_besar_scope() {
    const SEMPIT: &str = r#"
package main
pub fn main() {
    let f = |x| x + 1
    var acc = 0
    var i = 0
    while i < 120000 { acc = acc + f(i); i = i + 1 }
    assert(acc > 0)
}
"#;
    const LEBAR: &str = r#"
package main
pub fn main() {
    var p0 = [1,2,3,4,5,6,7,8,9,10]
    var p1 = [1,2,3,4,5,6,7,8,9,10]
    var p2 = [1,2,3,4,5,6,7,8,9,10]
    var p3 = [1,2,3,4,5,6,7,8,9,10]
    var p4 = [1,2,3,4,5,6,7,8,9,10]
    var p5 = [1,2,3,4,5,6,7,8,9,10]
    var p6 = [1,2,3,4,5,6,7,8,9,10]
    var p7 = [1,2,3,4,5,6,7,8,9,10]
    let f = |x| x + 1
    var acc = 0
    var i = 0
    while i < 120000 { acc = acc + f(i); i = i + 1 }
    assert(acc > 0)
}
"#;
    let sempit = tercepat(SEMPIT, 2);
    let lebar = tercepat(LEBAR, 2);
    let rasio = lebar / sempit.max(1e-6);
    assert!(
        rasio < 2.0,
        "biaya panggil closure ikut membengkak karena scope sekitarnya: \
         sempit {:.3}s, lebar {:.3}s (rasio {:.1}x, ambang 2x)",
        sempit, lebar, rasio
    );
}

/// Nilai biasa TIDAK pernah beralias — ini sifat inti model memori JRG,
/// dan yang membuat lifetime-nya sepenuhnya lexical tanpa GC.
#[test]
fn nilai_agregat_tidak_pernah_beralias() {
    const SRC: &str = r#"
package main
struct Q { pub v: int }
struct P { pub q: Q }
fn ubah(a) { a[0] = 999; return a }
pub fn main() {
    var a = [1,2,3]
    var b = a
    b[0] = 99
    assert_eq(a[0], 1)

    var p = P { q: Q { v: 1 } }
    var p2 = p
    p2.q.v = 77
    assert_eq(p.q.v, 1)

    var c = [1,2,3]
    ubah(c)
    assert_eq(c[0], 1)

    var outer = [[1,2],[3,4]]
    var inner = outer[0]
    inner[0] = 55
    assert_eq(outer[0][0], 1)

    var d = { "k": [1,2] }
    var dv = d["k"]
    dv[0] = 88
    assert_eq(d["k"][0], 1)
}
"#;
    jalankan(SRC).expect("value semantics harus utuh");
}

/// Satu-satunya alias yang disengaja di JRG: binding yang ditangkap
/// closure. Dikunci di sini supaya perubahan model memori tidak
/// diam-diam menghapus atau memperluasnya.
#[test]
fn satu_satunya_alias_adalah_binding_tertangkap() {
    const SRC: &str = r#"
package main
pub fn main() {
    var x = [1,2,3]
    let ubah = || { x[0] = 77 }
    ubah()
    assert_eq(x[0], 77)

    var n = 0
    let a = || { n = n + 1 }
    let b = || n
    a()
    assert_eq(b(), 1)
    assert_eq(n, 1)
}
"#;
    jalankan(SRC).expect("alias binding tertangkap harus tetap bekerja");
}

/// `arr::push`/`pop`/`concat`/`reverse` di dalam loop harus LINEAR.
#[test]
fn transformer_array_di_loop_tetap_linear() {
    for (nama, badan) in [
        ("arr::push(a, i)",  "a = arr::push(a, i)"),
        ("a.push(i)",        "a = a.push(i)"),
        ("arr::concat",      "a = arr::concat(a, [i])"),
    ] {
        let prog = |n: usize| format!(
            "package main\npub fn main() {{\n  var a = []\n  var i = 0\n  \
             while i < {} {{ {}; i = i + 1 }}\n  assert_eq(len(a), {})\n}}\n",
            n, badan, n
        );
        let (pk, pb) = (prog(4000), prog(32000));
        let kecil = tercepat(Box::leak(pk.into_boxed_str()), 2);
        let besar = tercepat(Box::leak(pb.into_boxed_str()), 2);
        let rasio = besar / kecil.max(1e-6);
        assert!(
            rasio < 24.0,
            "`{}` sepertinya kembali kuadratik: n=4000 {:.3}s, n=32000 {:.3}s \
             (rasio {:.1}x untuk 8x n, ambang 24x). Linear ~8x, kuadratik ~64x.",
            nama, kecil, besar, rasio
        );
    }

    // `pop` diuji terpisah karena bentuknya beda: ia menjawab
    // `(array_baru, nilai)`, jadi pemakaian yang wajar selalu terbungkus
    // indeks — dan bungkus itulah yang harus ikut dikenali.
    let prog_pop = |n: usize| format!(
        "package main\npub fn main() {{\n  var a = arr::fill(0, {})\n  var i = 0\n  \
         while i < {} {{ a = arr::pop(a)[0]; i = i + 1 }}\n  assert_eq(len(a), 0)\n}}\n",
        n, n
    );
    let (pk, pb) = (prog_pop(4000), prog_pop(32000));
    let kecil = tercepat(Box::leak(pk.into_boxed_str()), 2);
    let besar = tercepat(Box::leak(pb.into_boxed_str()), 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 24.0,
        "`arr::pop(a)[0]` sepertinya kembali kuadratik: n=4000 {:.3}s, \
         n=32000 {:.3}s (rasio {:.1}x untuk 8x n, ambang 24x).",
        kecil, besar, rasio
    );
}

/// **Menyebut nama builtin yang dilayani lewat jembatan tidak membocorkan memori per
/// panggilan**. Programnya membaca puncak RSS-nya sendiri;
#[test]
fn nama_lewat_jembatan_tidak_bocor_per_panggilan() {
    let dir = bersama::dir_unik("jrg-bocor-jembatan");
    std::fs::create_dir_all(&dir).expect("bikin dir");
    let puncak = |n: u32| -> Option<i64> {
        let src = format!(
            "let d = {{\"a\": 1}}\nvar i = 0\nwhile i < {} {{\n    let k = json::keys(d)\n    i = i + 1\n}}\n\
             for _, l in str::split(file::read(\"/proc/self/status\").unwrap_or(\"\"), \"\\n\") {{\n\
             \x20   if str::starts_with(l, \"VmHWM\") {{ wok(l) }}\n}}\n", n);
        let f = dir.join(format!("bocor{}.jrg", n));
        std::fs::write(&f, src).expect("tulis sumber");
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_jrgc")).arg(&f)
            .stdin(std::process::Stdio::null()).output().expect("jalankan jrgc");
        let teks = String::from_utf8_lossy(&out.stdout);
        teks.split_whitespace().nth(1).and_then(|k| k.parse().ok())
    };
    let hasil = (puncak(100_000), puncak(800_000));
    let _ = std::fs::remove_dir_all(&dir);
    let (Some(kecil), Some(besar)) = hasil else {
        eprintln!("dilewati: /proc/self/status tidak terbaca di sistem ini");
        return;
    };
    // Kebocoran lama menambah ±21 MB di sini; tanpa kebocoran selisihnya nyaris nol.
    assert!(besar - kecil < 8_000,
        "puncak memori tumbuh {} kB untuk 700 ribu panggilan tambahan ({} kB -> {} kB) — \
         ada yang dialokasikan per panggilan dan tidak pernah dilepas", besar - kecil, kecil, besar);
}

/// **Memanggil fungsi tidak menyalin badannya** (Fase PANGGIL-SALIN). Badan 16x lebih besar —
/// di cabang yang tidak pernah jalan — dulu membuat tiap panggilan ±15x lebih mahal.
#[test]
fn biaya_panggil_tidak_ikut_besar_badan() {
    fn sumber(bentuk: &str, n: usize) -> &'static str {
        let mati: String = (0..n).map(|i| format!("        let v{} = x + {}\n", i, i)).collect();
        let badan = format!("    if x < 0 {{\n{}    }}\n    return x + 1\n", mati);
        let ulang = "    var s = 0\n    for i := 0; i < 3000; i = i + 1 {";
        let src = match bentuk {
            "fungsi" => format!("package main\nfn f(x) {{\n{}}}\npub fn main() {{\n{} s = s + f(i) }}\n}}\n",
                badan, ulang),
            "bersarang" => format!("package main\nfn luar(x) {{\n fn f(x) {{\n{} }}\n return f(x)\n}}\n\
                pub fn main() {{\n{} s = s + luar(i) }}\n}}\n", badan, ulang),
            "closure" => format!("package main\npub fn main() {{\n let f = |x| {{\n{} }}\n{} s = s + f(i) }}\n}}\n",
                badan, ulang),
            _ => format!("package main\npub fn main() {{\n{}\n let f = |x| {{\n{} }}\n s = s + f(i) }}\n}}\n",
                ulang, badan),
        };
        Box::leak(src.into_boxed_str())
    }
    for bentuk in ["fungsi", "bersarang", "closure", "closure dibuat di loop"] {
        let kecil = tercepat(sumber(bentuk, 25), 3);
        let besar = tercepat(sumber(bentuk, 400), 3);
        let rasio = besar / kecil;
        assert!(rasio < 4.0,
            "{}: badan 16x lebih besar membuat panggilannya {:.1}x lebih mahal ({:.1} → {:.1} ms) — \
             badan fungsi disalin lagi tiap panggilan?", bentuk, rasio, kecil * 1e3, besar * 1e3);
    }
}

/// **Menumbuhkan koleksi lewat field tetap linear** (Fase TUMBUH-2): `p.xs = p.xs + [x]` dan
/// `p.xs += [x]` dulu menyalin seluruh array tiap putaran.
#[test]
fn menumbuhkan_field_tetap_linear() {
    fn sumber(bentuk: &str, n: usize) -> &'static str {
        Box::leak(format!(
            "package main\nstruct P {{ pub xs: []int }}\npub fn main() {{ var p = P {{ xs: [] }}\n \
             for i := 0; i < {}; i = i + 1 {{ {} }}\n assert_eq(len(p.xs), {}) }}\n", n, bentuk, n
        ).into_boxed_str())
    }
    for bentuk in ["p.xs = p.xs + [i]", "p.xs += [i]"] {
        let kecil = tercepat(sumber(bentuk, 4000), 3);
        let besar = tercepat(sumber(bentuk, 32000), 3);
        let rasio = besar / kecil;
        assert!(rasio < 24.0,
            "`{}`: 8x elemen membuat waktunya {:.1}x ({:.1} → {:.1} ms) — linear ±8x, kuadratik ±64x",
            bentuk, rasio, kecil * 1e3, besar * 1e3);
    }
}

/// `x = x.m(..)` atas method yang menulis ke `self` LINEAR: penerimanya dipindahkan, jadi
/// `self.xs += [v]` menyambung di tempat. Dulu tiap panggilan menyalin seluruh isi
/// (`std/queue`: 20 ribu `push` = 8 detik).
#[test]
fn penerima_yang_dikonsumsi_tetap_linear() {
    fn sumber(n: usize) -> &'static str {
        Box::leak(format!(
            "package main\nstruct Q {{ pub xs: []int, pub k: int }}\nimpl Q {{\n \
             pub fn push(self, x: int) -> Q {{ self.xs += [x]\n return self }}\n \
             pub fn rest(self) -> Q {{ self.k += 1\n return self }} }}\n\
             pub fn main() {{ var q = Q {{ xs: [], k: 0 }}\n \
             for i := 0; i < {n}; i = i + 1 {{ q = q.push(i) }}\n \
             for i := 0; i < {n}; i = i + 1 {{ q = q.rest() }}\n assert_eq(len(q.xs), {n}) }}\n", n = n
        ).into_boxed_str())
    }
    let kecil = tercepat(sumber(4000), 3);
    let besar = tercepat(sumber(32000), 3);
    let rasio = besar / kecil;
    assert!(rasio < 24.0,
        "`q = q.push(i)`: 8x panggilan membuat waktunya {:.1}x ({:.1} → {:.1} ms) — linear ±8x, \
         kuadratik ±64x. Penyebab paling mungkin: `coba_konsumsi_penerima` tidak lagi kena.",
        rasio, kecil * 1e3, besar * 1e3);
}

/// Bentuk menumbuhkan array yang bindingnya TIDAK bisa dikosongkan dulu (`var` modul, tangkapan
/// closure) atau yang lewat jalur field/elemen. `{n}` = jumlah putaran.
const BENTUK_TUMBUH_LAIN: &[(&str, &str)] = &[
    ("var modul, `A = A + [i]`",
     "package main\nvar A = []\npub fn main() { for i in {n} { A = A + [i] }\n assert_eq(len(A), {n}) }\n"),
    ("var modul, `A += [i]`",
     "package main\nvar A = []\npub fn main() { for i in {n} { A += [i] }\n assert_eq(len(A), {n}) }\n"),
    ("var modul, `A = A.push(i)`",
     "package main\nvar A = []\npub fn main() { for i in {n} { A = A.push(i) }\n assert_eq(len(A), {n}) }\n"),
    ("var modul, `G.xs = G.xs + [i]`",
     "package main\nstruct P { pub xs: array }\nvar G = P { xs: [] }\n\
      pub fn main() { for i in {n} { G.xs = G.xs + [i] }\n assert_eq(len(G.xs), {n}) }\n"),
    ("tangkapan closure, `h = h + [x]`",
     "package main\npub fn main() { var h = []\n let f = |x| { h = h + [x] }\n for i in {n} { f(i) }\n assert_eq(len(h), {n}) }\n"),
    ("tangkapan closure, `h = h.push(x)`",
     "package main\npub fn main() { var h = []\n let f = |x| { h = h.push(x) }\n for i in {n} { f(i) }\n assert_eq(len(h), {n}) }\n"),
    ("tangkapan closure, `h += [x]`",
     "package main\npub fn main() { var h = []\n let f = |x| { h += [x] }\n for i in {n} { f(i) }\n assert_eq(len(h), {n}) }\n"),
    ("lokal milik pembuat closure, `h = h + [i]`",
     "package main\npub fn main() { var h = []\n let f = || len(h)\n for i in {n} { h = h + [i] }\n assert_eq(f(), {n}) }\n"),
    ("lokal milik pembuat closure, `h = h.push(i)`",
     "package main\npub fn main() { var h = []\n let f = || len(h)\n for i in {n} { h = h.push(i) }\n assert_eq(f(), {n}) }\n"),
    ("field lokal, `p.xs = p.xs + [i]`",
     "package main\nstruct P { pub xs: array }\npub fn main() { var p = P { xs: [] }\n \
      for i in {n} { p.xs = p.xs + [i] }\n assert_eq(len(p.xs), {n}) }\n"),
    ("field lokal, `p.xs = p.xs.push(i)`",
     "package main\nstruct P { pub xs: array }\npub fn main() { var p = P { xs: [] }\n \
      for i in {n} { p.xs = p.xs.push(i) }\n assert_eq(len(p.xs), {n}) }\n"),
    ("elemen, `m[1] = m[1] + [i]`",
     "package main\npub fn main() { var m = [[], []]\n for i in {n} { m[1] = m[1] + [i] }\n assert_eq(len(m[1]), {n}) }\n"),
    ("isi dict, `d[\"a\"] = d[\"a\"] + [i]`",
     "package main\npub fn main() { var d = {\"a\": []}\n for i in {n} { d[\"a\"] = d[\"a\"] + [i] }\n assert_eq(len(d[\"a\"]), {n}) }\n"),
    ("field privat di method, `self.xs = self.xs + [x]`",
     "package main\nstruct P { xs: array }\nimpl P { fn baru() { return P { xs: [] } }\n \
      fn tambah(self, x) { self.xs = self.xs + [x]\n return self }\n fn n(self) { return len(self.xs) } }\n\
      pub fn main() { var p = P::baru()\n for i in {n} { p = p.tambah(i) }\n assert_eq(p.n(), {n}) }\n"),
    ("field privat di method, `self.xs += [x]`",
     "package main\nstruct P { xs: array }\nimpl P { fn baru() { return P { xs: [] } }\n \
      fn tambah(self, x) { self.xs += [x]\n return self }\n fn n(self) { return len(self.xs) } }\n\
      pub fn main() { var p = P::baru()\n for i in {n} { p = p.tambah(i) }\n assert_eq(p.n(), {n}) }\n"),
    ("field privat di method, `self.xs = self.xs.push(x)`",
     "package main\nstruct P { xs: array }\nimpl P { fn baru() { return P { xs: [] } }\n \
      fn tambah(self, x) { self.xs = self.xs.push(x)\n return self }\n fn n(self) { return len(self.xs) } }\n\
      pub fn main() { var p = P::baru()\n for i in {n} { p = p.tambah(i) }\n assert_eq(p.n(), {n}) }\n"),
];

/// Menumbuhkan array lewat binding yang tidak bisa dikosongkan atau lewat jalur tetap LINEAR:
/// tempatnya disambung di tempat selama masih memegang penyimpanan yang tadi dibaca. Dulu
/// `A = A + [i]` atas `var` modul: 32 ribu putaran = 20 detik.
#[test]
fn menumbuhkan_lewat_binding_lain_tetap_linear() {
    for (nama, pola) in BENTUK_TUMBUH_LAIN {
        let sumber = |n: usize| -> &'static str { Box::leak(pola.replace("{n}", &n.to_string()).into_boxed_str()) };
        let kecil = tercepat(sumber(4000), 2);
        let besar = tercepat(sumber(32000), 2);
        let rasio = besar / kecil;
        assert!(rasio < 24.0,
            "{}: 8x putaran membuat waktunya {:.1}x ({:.1} → {:.1} ms) — linear ±8x, kuadratik ±64x. \
             Penyebab paling mungkin: `tumbuh_bukan_lokal`/`coba_tumbuh_jalur` tidak lagi kena.",
            nama, rasio, kecil * 1e3, besar * 1e3);
    }
}

/// Membaca SATU nilai dari koleksi tidak boleh sebanding ukurannya: `dict::has`, `dict::get`,
/// `dict::len`, `arr::get`, `arr::first`, `arr::last` dulu menyalin seluruh koleksinya tiap
/// dipanggil (3000 `dict::has` atas 32 ribu kunci = 21 detik).
#[test]
fn membaca_satu_nilai_tidak_ikut_ukuran_koleksi() {
    fn sumber(baca: &str, n: usize) -> &'static str {
        Box::leak(format!(
            "package main\npub fn main() {{ var a = []\n var d = {{\"k0\": 0}}\n \
             for i := 0; i < {n}; i = i + 1 {{ a = arr::push(a, i)\n d[\"k\" + str(i)] = i }}\n var s = 0\n \
             for i := 0; i < 20000; i = i + 1 {{ s = s + {baca} }}\n assert(s >= 0) }}\n", n = n, baca = baca
        ).into_boxed_str())
    }
    for baca in ["(dict::has(d, \"k1\") $ 1 : 0)", "(dict::get(d, \"k1\") ?? 0)", "dict::len(d)",
                 "(arr::get(a, 0) ?? 0)", "(arr::first(a) ?? 0)", "(arr::last(a) ?? 0)", "arr::get_or(a, 0, 1)"] {
        let kecil = tercepat(sumber(baca, 1000), 2);
        let besar = tercepat(sumber(baca, 16000), 2);
        let rasio = besar / kecil;
        assert!(rasio < 12.0,
            "`{}`: koleksi 16x lebih besar membuat 20 ribu pembacaan {:.1}x lebih lama ({:.1} → {:.1} ms) — \
             pembacaannya ikut menyalin koleksinya", baca, rasio, kecil * 1e3, besar * 1e3);
    }
}

/// `q, v = q.pop()` LINEAR: `q` dipindahkan ke `pop`, dan `self = self.rest()` di dalamnya
/// mengubah di tempat. Bentuk dua pernyataan (`let r = q.pop(); q = r[0]`) tidak bisa begitu.
#[test]
fn bongkar_yang_mengonsumsi_tetap_linear() {
    fn sumber(n: usize) -> &'static str {
        Box::leak(format!(r#"package main
struct Q {{ pub xs: []int, pub k: int }}
impl Q {{
    pub fn push(self, x: int) -> Q {{ self.xs += [x]
      return self }}
    pub fn rest(self) -> Q {{ self.k += 1
      return self }}
    pub fn pop(self) -> (Q, int) {{ let v = self.xs[self.k]
      self = self.rest()
      return (self, v) }}
}}
pub fn main() {{ var q = Q {{ xs: [], k: 0 }}
  var s = 0
  var v = 0
  for i := 0; i < {n}; i = i + 1 {{ q = q.push(i) }}
  for i := 0; i < {n}; i = i + 1 {{ q, v = q.pop()
    s = s + v }}
  assert_eq(q.k, {n}) }}
"#, n = n).into_boxed_str())
    }
    let kecil = tercepat(sumber(4000), 3);
    let besar = tercepat(sumber(32000), 3);
    let rasio = besar / kecil;
    assert!(rasio < 24.0,
        "`q, v = q.pop()`: 8x panggilan membuat waktunya {:.1}x ({:.1} → {:.1} ms) — linear ±8x, kuadratik ±64x",
        rasio, kecil * 1e3, besar * 1e3);
}

