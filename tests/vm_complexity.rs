// tests/vm_complexity.rs

mod bersama;
use std::process::Command;
use std::time::Instant;

fn jrgc() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target");
    p.push(if cfg!(debug_assertions) { "debug" } else { "release" });
    p.push("jrgc");
    p
}

/// Waktu tercepat dari beberapa percobaan, lewat backend `--vm`.
fn vm_tercepat(src: &str, kali: usize) -> f64 {
    let dir = crate::bersama::dir_unik("jrg-kompleksitas");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");

    let mut best = f64::MAX;
    for _ in 0..kali {
        let t = Instant::now();
        let out = Command::new(jrgc())
            .arg(&file)
            .arg("--vm")
            .output()
            .expect("jalankan jrgc");
        assert!(
            out.status.success(),
            "program harus jalan tanpa error:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        best = best.min(t.elapsed().as_secs_f64());
    }
    let _ = std::fs::remove_dir_all(&dir);
    best
}

/// Waktu tercepat `--periksa` (urai + kompilasi VM, tanpa menjalankan) atas `src`.
fn periksa_tercepat(src: &str, kali: usize) -> f64 {
    let dir = crate::bersama::dir_unik("jrg-kompilasi");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let mut best = f64::MAX;
    for _ in 0..kali {
        let t = Instant::now();
        let out = Command::new(jrgc()).arg(&file).arg("--periksa").output().expect("jalankan jrgc");
        assert!(out.status.success(), "harus terkompilasi:\n{}", String::from_utf8_lossy(&out.stderr));
        best = best.min(t.elapsed().as_secs_f64());
    }
    let _ = std::fs::remove_dir_all(&dir);
    best
}

/// **Kompilasi `--vm` linear dalam jumlah ekspresi** — dulu KUADRAT: kolam span, nama dan
/// konstanta sebuah chunk dicari linear, dan tiap ekspresi punya span unik, jadi 40 ribu baris
/// `wok(i)` memakan 16 detik. Linear ≈8x untuk 8x baris, kuadrat ≈64x.
#[test]
fn kompilasi_vm_linear_dalam_jumlah_ekspresi() {
    let program = |n: usize| (0..n).map(|i| format!("wok({}, \"t{}\", {}.5)\n", i, i, i)).collect::<String>();
    let kecil = periksa_tercepat(&program(3000), 2);
    let besar = periksa_tercepat(&program(24000), 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 24.0,
        "kompilasi tidak lagi linear: 3000 baris {:.3}s, 24000 baris {:.3}s (rasio {:.1}x, \
         ambang 24x; linear ≈8x, kuadrat ≈64x)",
        kecil, besar, rasio
    );
}

/// `a[i] = v` di VM harus berbiaya tetap terhadap ukuran array.
#[test]
fn penulisan_terindeks_tidak_ikut_ukuran_array() {
    const KECIL: &str = r#"
package main
pub fn main() {
    var a = range(500)
    var i = 0
    while i < 20000 { a[i % 500] = i; i = i + 1 }
    assert_eq(a[0], 19500)
}
"#;
    const BESAR: &str = r#"
package main
pub fn main() {
    var a = range(4000)
    var i = 0
    while i < 20000 { a[i % 4000] = i; i = i + 1 }
    assert_eq(a[0], 16000)
}
"#;
    let kecil = vm_tercepat(KECIL, 2);
    let besar = vm_tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 3.0,
        "penulisan lewat index sepertinya mulai menyalin seluruh array: \
         array 500 {:.3}s, array 4000 {:.3}s (rasio {:.1}x, ambang 3x untuk \
         jumlah penulisan yang SAMA)",
        kecil, besar, rasio
    );
}

/// Menyerahkan agregat ke fungsi TIDAK boleh ikut ukurannya.
#[test]
fn penyerahan_agregat_ke_fungsi_tidak_ikut_ukuran() {
    const KECIL: &str = r#"
package main
pub fn main() {
    let a = range(500)
    var s = 0
    var i = 0
    while i < 20000 { s = s + len(a); i = i + 1 }
    assert_eq(s, 10000000)
}
"#;
    const BESAR: &str = r#"
package main
pub fn main() {
    let a = range(4000)
    var s = 0
    var i = 0
    while i < 20000 { s = s + len(a); i = i + 1 }
    assert_eq(s, 80000000)
}
"#;
    let kecil = vm_tercepat(KECIL, 2);
    let besar = vm_tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 3.0,
        "penyerahan agregat ke fungsi mulai ikut ukurannya lagi: \
         array 500 {:.3}s, array 4000 {:.3}s (rasio {:.1}x untuk 8x ukuran, \
         ambang 3x). O(1) ~1x, O(ukuran) ~8x — kalau ini muncul, kemungkinan \
         besar penyerahan nilai kembali menyalin di muka alih-alih menunggu \
         ada yang menulis.",
        kecil, besar, rasio
    );
}

/// Penerima method mengikuti aturan yang sama dengan argumen biasa, jadi
/// `a.len()` juga tidak boleh ikut ukurannya.
#[test]
fn penyerahan_penerima_method_tidak_ikut_ukuran() {
    const KECIL: &str = r#"
package main
pub fn main() {
    let a = range(500)
    var s = 0
    var i = 0
    while i < 20000 { s = s + a.len(); i = i + 1 }
    assert_eq(s, 10000000)
}
"#;
    const BESAR: &str = r#"
package main
pub fn main() {
    let a = range(4000)
    var s = 0
    var i = 0
    while i < 20000 { s = s + a.len(); i = i + 1 }
    assert_eq(s, 80000000)
}
"#;
    let kecil = vm_tercepat(KECIL, 2);
    let besar = vm_tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 3.0,
        "penerima method mulai ikut ukurannya lagi: \
         array 500 {:.3}s, array 4000 {:.3}s (rasio {:.1}x untuk 8x ukuran, \
         ambang 3x). O(1) ~1x, O(ukuran) ~8x.",
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
    let kecil = vm_tercepat(KECIL, 2);
    let besar = vm_tercepat(BESAR, 2);
    let rasio = besar / kecil.max(1e-6);
    assert!(
        rasio < 24.0,
        "menumbuhkan koleksi sepertinya kembali kuadratik: \
         n=4000 {:.3}s, n=32000 {:.3}s (rasio {:.1}x untuk 8x n, ambang 24x). \
         Linear ~8x, kuadratik ~64x. Penyebab paling mungkin: jalur cepat \
         `a = a + [x]` tidak lagi kena, jadi binding masih ikut memegang \
         nilainya saat penggabungan.",
        kecil, besar, rasio
    );
}

/// Menumbuhkan TEKS di `--vm` linear — dulu tiap `s = s + str(..)` menyalin seluruh `s`
/// (memcpy 49% instruksi, 3,5x lebih lambat dari interpreter). Waktu program kosong
/// dikurangkan, dan n cukup besar: pada n kecil waktu start menutupi kuadratnya.
#[test]
fn menumbuhkan_teks_tetap_linear_di_vm() {
    if cfg!(debug_assertions) {
        eprintln!("lewati: di build debug dispatch VM jauh lebih lambat dari memcpy, jadi rasionya menipu");
        return;
    }
    let program = |n: usize| format!(r#"
package main
pub fn main() {{
    var s = ""
    var t = ""
    for i in 0..{n} {{
        s = s + str(i % 10)
        t += "ab"
    }}
    assert_eq(len(s), {n})
    assert_eq(len(t), {n} * 2)
}}
"#);
    let kosong = vm_tercepat("package main\npub fn main() { }\n", 3);
    let kecil = (vm_tercepat(&program(25_000), 3) - kosong).max(1e-6);
    let besar = (vm_tercepat(&program(200_000), 2) - kosong).max(1e-6);
    let rasio = besar / kecil;
    assert!(
        rasio < 24.0,
        "menumbuhkan teks di --vm sepertinya kembali kuadratik: n=25000 {:.3}s, n=200000 {:.3}s \
         (rasio {:.1}x untuk 8x n, ambang 24x; linear ~8x, kuadratik ~64x). Penyebab paling \
         mungkin: cabang `Str` di `VmValue::tambah_milik` tidak lagi menyambung di tempat.",
        kecil, besar, rasio
    );
}

/// Rantai `s = s + a + b` dan sasaran berjalur (`b.s += x`, `a[0] += x`) linear di `--vm` —
/// dulu ketiganya kuadratik di DUA mesin (8x n → 38–110x waktu).
#[test]
fn rantai_dan_sasaran_berjalur_tetap_linear_di_vm() {
    if cfg!(debug_assertions) {
        eprintln!("lewati: di build debug dispatch VM jauh lebih lambat dari memcpy, jadi rasionya menipu");
        return;
    }
    let program = |n: usize| format!(r#"
package main
struct B {{ pub s: string }}
pub fn main() {{
    var s = ""
    var b = B {{ s: "" }}
    var a = [""]
    for i in 0..{n} {{
        s = s + str(i % 10) + ","
        b.s += "ab"
        a[0] += "cd"
    }}
    assert_eq(len(s) + len(b.s) + len(a[0]), {n} * 6)
}}
"#);
    let kosong = vm_tercepat("package main\npub fn main() { }\n", 3);
    let kecil = (vm_tercepat(&program(20_000), 3) - kosong).max(1e-6);
    let besar = (vm_tercepat(&program(160_000), 2) - kosong).max(1e-6);
    let rasio = besar / kecil;
    assert!(rasio < 24.0,
        "rantai/sasaran berjalur di --vm sepertinya kembali kuadratik: {:.3}s lawan {:.3}s (rasio \
         {:.1}x untuk 8x n, ambang 24x). Penyebab paling mungkin: `rantai_tambah` atau opcode \
         `CobaTambahField`/`CobaTambahIndex` tidak lagi kena.", kecil, besar, rasio);
}

/// Padanan VM dari `transformer_array_di_loop_tetap_linear`.
#[test]
fn transformer_array_di_loop_tetap_linear_di_vm() {
    for (nama, badan, awal, akhir) in [
        ("arr::push(a, i)", "a = arr::push(a, i)",   "[]",                  "n"),
        ("a.push(i)",       "a = a.push(i)",          "[]",                  "n"),
        ("arr::concat",     "a = arr::concat(a, [i])","[]",                  "n"),
        ("arr::pop(a)[0]",  "a = arr::pop(a)[0]",     "arr::fill(0, {n})",   "0"),
    ] {
        let prog = |n: usize| format!(
            "package main\npub fn main() {{\n  var a = {}\n  var i = 0\n  \
             while i < {} {{ {}; i = i + 1 }}\n  assert_eq(len(a), {})\n}}\n",
            awal.replace("{n}", &n.to_string()), n, badan,
            if akhir == "n" { n.to_string() } else { akhir.to_string() }
        );
        let kecil = vm_tercepat(&prog(4000), 2);
        let besar = vm_tercepat(&prog(32000), 2);
        let rasio = besar / kecil.max(1e-6);
        assert!(
            rasio < 24.0,
            "VM: `{}` sepertinya kembali kuadratik: n=4000 {:.3}s, n=32000 {:.3}s \
             (rasio {:.1}x untuk 8x n, ambang 24x). Linear ~8x, kuadratik ~64x.",
            nama, kecil, besar, rasio
        );
    }
}

/// `x = x.m(..)` di `--vm`: penerimanya dipindahkan (`AmbilLokal`), jadi method yang menulis ke
/// `self` tidak menyalin seluruh isi tiap panggilan. Dulu 20 ribu `q = q.push(i)` = 6 detik.
#[test]
fn penerima_yang_dikonsumsi_tetap_linear_di_vm() {
    let sumber = |n: usize| format!(
        "package main\nstruct Q {{ pub xs: []int, pub k: int }}\nimpl Q {{\n \
         pub fn push(self, x: int) -> Q {{ self.xs += [x]\n return self }}\n \
         pub fn rest(self) -> Q {{ self.k += 1\n return self }} }}\n\
         pub fn main() {{ var q = Q {{ xs: [], k: 0 }}\n \
         for i := 0; i < {n}; i = i + 1 {{ q = q.push(i) }}\n \
         for i := 0; i < {n}; i = i + 1 {{ q = q.rest() }}\n assert_eq(len(q.xs), {n}) }}\n", n = n);
    let kecil = vm_tercepat(&sumber(4000), 3);
    let besar = vm_tercepat(&sumber(32000), 3);
    let rasio = besar / kecil.max(1e-6);
    assert!(rasio < 24.0,
        "`q = q.push(i)` di --vm: n=4000 {:.3}s, n=32000 {:.3}s (rasio {:.1}x, ambang 24x; linear ~8x, \
         kuadratik ~64x). Penyebab paling mungkin: `coba_konsumsi_penerima` tidak lagi kena.",
        kecil, besar, rasio);
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

/// Bentuk yang sama di `--vm`: sel binding (`CobaTumbuhUpval`/`CobaTumbuhGlobal`) dan jalur
/// (`SamaTempat` lalu `+=` di tempat) menyambung tanpa menyalin. Dulu 32 ribu putaran = 16 detik.
#[test]
fn menumbuhkan_lewat_binding_lain_tetap_linear_di_vm() {
    for (nama, pola) in BENTUK_TUMBUH_LAIN {
        let sumber = |n: usize| pola.replace("{n}", &n.to_string());
        let kecil = vm_tercepat(&sumber(4000), 2);
        let besar = vm_tercepat(&sumber(32000), 2);
        let rasio = besar / kecil.max(1e-6);
        assert!(rasio < 24.0,
            "{} di --vm: n=4000 {:.3}s, n=32000 {:.3}s (rasio {:.1}x, ambang 24x; linear ~8x, kuadratik ~64x). \
             Penyebab paling mungkin: opcode coba-tumbuh atau `SamaTempat` tidak lagi dipancarkan.",
            nama, kecil, besar, rasio);
    }
}

/// Pembaca satu nilai di `--vm` tidak menyalin koleksinya (`arr_pinjam`, `pinjam_isi`): dulu
/// 3000 `dict::has` atas 32 ribu kunci = 13 detik.
#[test]
fn membaca_satu_nilai_tidak_ikut_ukuran_koleksi_di_vm() {
    let sumber = |baca: &str, n: usize| format!(
        "package main\npub fn main() {{ var a = []\n var d = {{\"k0\": 0}}\n \
         for i := 0; i < {n}; i = i + 1 {{ a = arr::push(a, i)\n d[\"k\" + str(i)] = i }}\n var s = 0\n \
         for i := 0; i < 20000; i = i + 1 {{ s = s + {baca} }}\n assert(s >= 0) }}\n", n = n, baca = baca);
    for baca in ["(dict::has(d, \"k1\") $ 1 : 0)", "(dict::get(d, \"k1\") ?? 0)", "dict::len(d)",
                 "(arr::get(a, 0) ?? 0)", "(arr::first(a) ?? 0)", "(arr::last(a) ?? 0)", "arr::get_or(a, 0, 1)",
                 "(a.is_empty() $ 1 : 0)"] {
        let kecil = vm_tercepat(&sumber(baca, 1000), 2);
        let besar = vm_tercepat(&sumber(baca, 16000), 2);
        let rasio = besar / kecil.max(1e-6);
        assert!(rasio < 12.0,
            "`{}` di --vm: koleksi 16x lebih besar membuat 20 ribu pembacaan {:.1}x lebih lama \
             ({:.3}s → {:.3}s) — pembacaannya ikut menyalin koleksinya", baca, rasio, kecil, besar);
    }
}

/// `q, v = q.pop()` di `--vm` linear: penerimanya dipindahkan, tuplenya dibongkar `BongkarTuple`.
#[test]
fn bongkar_yang_mengonsumsi_tetap_linear_di_vm() {
    let sumber = |n: usize| format!(r#"package main
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
"#, n = n);
    let kecil = vm_tercepat(&sumber(4000), 3);
    let besar = vm_tercepat(&sumber(32000), 3);
    let rasio = besar / kecil.max(1e-6);
    assert!(rasio < 24.0,
        "`q, v = q.pop()` di --vm: n=4000 {:.3}s, n=32000 {:.3}s (rasio {:.1}x, ambang 24x)", kecil, besar, rasio);
}

/// Bentuk paling sering di putaran panas dikompilasi jadi instruksi gabungan (Fase PACU-4):
/// `x = e` atas lokal, `a[i]`, `a[i] = e`, `a[i] = a[i] + e`. Kalau salah satunya hilang dari
/// bytecode, jalur cepatnya tidak lagi kena dan yang terlihat cuma kernel yang melambat.
#[test]
fn bentuk_panas_dikompilasi_jadi_instruksi_gabungan() {
    let dir = crate::bersama::dir_unik("jrg-bytecode");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, "package main\nfn f(n) { var a = [0; 4]\n var b = [0; 4]\n var i = 0\n var k = 0\n \
        while i < n { a[k] = a[k] + i % 3\n b[k] = a[k]\n k = i % 4\n i = i + 1 }\n for j in 0..2 { k = j }\n return a }\n\
        pub fn main() { wok(f(5)) }\n").expect("tulis sumber");
    let out = Command::new(jrgc()).arg(&file).args(["--vm", "--emit-bc"]).output().expect("jalankan jrgc");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let teks = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let awal = teks.find("name: \"f\", arity: 1, code: [").expect("bytecode `f` tidak tercetak");
    let badan = &teks[awal..awal + teks[awal..].find(']').expect("akhir daftar instruksi")];
    for op in ["SimpanBuang", "IndeksLokal", "TulisIndexLokal", "TumbuhIndexLokal", "IterMaju"] {
        assert!(badan.contains(&format!("op: {},", op)), "`{}` tidak dipancarkan lagi:\n{}", op, badan);
    }
    let banyak = badan.matches("Instruction {").count();
    assert!(banyak <= 56, "fungsi uji sekarang {} instruksi (52 sesudah PACU-5, 100-an sebelum PACU-4):\n{}", banyak, badan);
}
