// Tabel bersama `tests/llvm_parity.rs` dan `tests/jit_parity.rs`; disisipkan lewat `include!`.

const PARITAS_LLVM: &[(&str, &str)] = &[
    // Fase BAWAAN-HOF (#868): builtin langsung sebagai callback HOF dan di `terap` dulu ditolak LLVM.
    ("builtin sebagai callback: map, filter, sort_by_key, each(wok), terap", r#"package main
fn f() {
 let b = [-1, -2].map(abs)
 wok("di f", b)
 return 7
}
pub fn main() { wok("hasil", f())
 wok(arr::map(["a"], str::upper))
wok(["1", "a"].filter(str::is_numeric))
 wok([-3, 1, -2].sort_by_key(abs))
wok(terap(abs, [-5]))
 wok("sebelum")
 [1, 2].each(wok)
 wok("sesudah") }"#),
    // Fase TERNARY-DUGA + POLA-ATAU (#866–#867): dulu "butuh tipe yang bisa disimpulkan" dan
    // "binding tidak didukung di dalam alternasi" di LLVM.
    ("ternary: tipe dari cabang pertama yang bernilai", r#"package main
pub fn main() { let n = 7; wok(n > 3 $ -1 : 1); wok(n > 3 $ !false : true); wok(n > 3 $ ~1 : 0); wok(n > 3 $ 'z' : 'y') }"#),
    ("pola alternatif yang mengikat: tuple, bersarang, array, guard", r#"for _, t in [(2, 5), (1, 7), (9, 9)] { wok(match t { (1, a) | (a, 5) => a, _ => 0 }) }
wok(match (1, (2, 3)) { (x, (y, 3) | (3, y)) => x + y, _ => 0 })
wok(match [1, 9] { [a, 9] | [9, a] => a, _ => 0 })
wok(match (2, 5) { (1, a) | (a, 5) if a > 1 => a * 10, _ => 0 })"#),
    ("pola alternatif yang mengikat: enum, struct, @, sisa array", r#"enum E { A(int), B(int), C }
struct P { pub x: int, pub y: int }
for _, e in [E::A(1), E::B(2), E::C] { wok(match e { E::A(n) | E::B(n) => n, E::C => -1 }) }
let p = P { x: 1, y: 4 }
wok(match p { P { x: 1, y: v } | P { x: v, y: 1 } => v, _ => 0 })
wok(match 3 { n @ (1 | 3) => n * 2, _ => 0 })
wok(match [0, 1, 2] { [0, ...r] | [9, ...r] => r, _ => [] })"#),
    // Fase POLA-LET (#864): pola literal, struct, `@` dan tuple yang bisa gagal di `let`/`for` dulu ditolak LLVM.
    ("let: pola yang bisa gagal lewat pencocok match", r#"package main
struct T { pub x: int, pub y: int = 2 }
pub fn main() {
 let (a, 5) = (1, 5)
 wok(a)
 let T { x, y } = T { x: 5 }
 wok(x, y)
 let n @ 1..=9 = 4
 wok(n)
 for _, (p, 1) in [(3, 1), (4, 1)] { wok(p) }
}"#),
    // Fase SAMBUNG-CAMPUR (#865): `[]bertag + []int` dulu ditolak "tipe elemennya beda".
    ("sambung array berjenis elemen berbeda", r#"package main
pub fn main() { var h = [1]
 let f = || { h = [0]; return [7] }
 h = h + [2]
 h = h.push(3)
 h = h + [4] + [5]
 wok(h)
 var g = len(env::args()) > 99 $ [1] : [2.5]
 wok(g + [3], [4] + g) }"#),
    // Fase JOIN-METODE (#863): `[1, 2].join("-")` dulu ditolak LLVM; objeknya dievaluasi sekali.
    ("join sebagai method atas array bukan teks", r#"package main
fn xs() -> []int { wok("sekali")
 return [1, 2] }
pub fn main() { wok([1, 2].join("-"), xs().join(", "), [1.5, 2.0].join("|")) }"#),
    // Fase RUNE-TEKS (#862): `rune` di argumen teks `str::` dulu ditolak LLVM ("rune sebagai string").
    ("str: rune sebagai teks satu karakter di semua pintu", r#"package main
pub fn main() {
    let r = "abc".char_at(1)
    wok(str::contains("abc", r), str::starts_with("bc", r), str::ends_with("ab", r))
    wok(str::replace("abc", r, "X"), str::split("abcbd", r), str::index_of("abc", r))
    wok(str::repeat(r, 3), str::upper(r), str::count("bbb", r))
    wok(str::pad_left("7", 3, r), str::join(["x", "y"], r))
}"#),
    // Fase SEBAR-DICT (#861): tuple kosong dan sebar struct/dict berjenis lain ke `dict { .. }` dulu ditolak LLVM.
    ("potong: tuple kosong", r#"package main
pub fn main() { let t = (1, 2)
 let k = t[1:1]
 wok(typeof(k), k, len(k), k == t[0:0])
 wok(t[2:], t[:0]) }"#),
    ("sebar ke literal dict: basis struct dan dict berjenis lain", r#"package main
struct Q { a: int, b: int = 3 }
pub fn main() { wok(dict { a: 9, ...Q { a: 1 } }, dict { z: "t", ...Q { a: 1 } })
 let d = dict { a: 1 }
 wok(dict { s: "x", ...d }, dict::merge(d, {"a": "y"}))
 wok(dict { ...Q { a: 2 } }) }"#),
    // Fase MINMAX-CAMPUR (#860): bulat lawan pecahan dulu ditolak LLVM; jawabannya argumen aslinya.
    ("math::max/min bulat lawan pecahan: seri, NaN, typeof", r#"package main
pub fn main() {
 wok(math::max(1, 2.5), math::max(3, 2.5), math::max(2, 2.0), math::max(2.0, 2))
 wok(math::min(1, 2.5), math::min(3, 2.5), math::min(2, 2.0), math::min(2.0, 2))
 let a = math::max(1, 2.5)
 wok(typeof(a), typeof(math::max(3, 2.5)))
 wok(math::max(1, math::NAN), math::min(math::NAN, 1))
}"#),
    // Fase KUNCI-VARIABEL (#859): `p[k]` dengan kunci dari variabel dulu ditolak LLVM saat kompilasi.
    ("struct: indeks teks dari variabel, campur dan setipe, privat dari dalam method", r#"package main
struct P { pub x: int, pub nama: string, rahasia: int = 7 }
struct Q { pub a: int, pub b: int }
impl P { fn baca(self, k) { return self[k] } }
pub fn main() {
    let p = P { x: 1, nama: "a" }
    for _, k in ["x", "nama"] { wok(p[k], typeof(p[k])) }
    wok(p.baca("rahasia"))
    let q = Q { a: 3, b: 4 }
    var t = 0
    for _, k in ["a", "b"] { t = t + q[k] }
    wok(t)
}"#),
    // Fase TUPLE-INDEKS (#856): `t[i]` dengan `i` dari variabel dulu ditolak LLVM saat kompilasi.
    ("tuple: indeks dari variabel, setipe (baca/tulis) dan campur tipe", r#"package main
pub fn main() {
    var t: (int, int) = (1, 2)
    var i: int = 0
    wok(t[i])
    i = -1
    wok(t[i])
    t[i] = 9
    wok(t)
    let c = (1, "dua", 3.5)
    for k in 0..3 { wok(c[k], typeof(c[k])) }
    var j = -3
    wok(c[j])
}"#),
    // Fase BAWAAN-NILAI (#855): fungsi pustaka sebagai nilai dulu ditolak LLVM ("dipakai sebagai NILAI").
    ("fungsi bawaan sebagai nilai", r#"package main
pub fn main() {
    wok([1.0, 4.0].map(math::sqrt))
    let f = str::upper
    wok(f("ab"), typeof(f), f)
    wok(arr::map(["a", "bc"], len))
    let g = math::max
    wok(g(1, 5))
}"#),
    // Fase KTOR-GENERIK (#854): dulu "belum mendukung konstruktor sebagai nilai" di LLVM.
    ("konstruktor generik, Some, Ok, Err sebagai nilai", r#"package main
enum G<T> { V(T), C }
enum P<A, B> { Dua(A, B) }
pub fn main() {
    let f = G::V
    wok(f(3), f("x"), typeof(f), f)
    let s = Some
    let o = Ok
    let e = Err
    wok(s(1), o("a"), e(2.5), typeof(s), s)
    wok(arr::map([1, 2], Some))
    let d = P::Dua
    wok(d(1, "b"))
}"#),
    // Fase WARIS-GENERIK (#853): mewarisi struct generik dulu ditolak LLVM ("field warisannya tidak bisa diratakan").
    ("warisan: induk generik, method dan default warisan", r#"package main
struct Dasar<T> { pub isi: T, pub n: int = 1 }
struct Anak: Dasar { pub tambah: int }
struct Cucu: Anak { pub z: bool = true }
impl<T> Dasar<T> { fn ambil(self) -> T { self.isi } fn dua(self) -> int { self.n * 2 } }
pub fn main() {
    let a = Anak { isi: "x", tambah: 2 }
    wok(a, a.isi, a.n, a.tambah)
    wok(a.ambil(), a.dua())
    let b = Anak { isi: 5, tambah: 1, n: 7 }
    wok(b, b.dua(), b == b)
    let c = Cucu { isi: [1, 2], tambah: 0 }
    wok(c, c.ambil())
}"#),
    // Fase ENUM-GENERIK (#852): enum generik milik program dulu "enum `E` tidak dikenal" di LLVM.
    ("enum generik: konstruktor, anotasi, fungsi dan impl generik, rekursif, varian ber-field", r#"package main
enum Pilih<T> { Ada(T), Kosong }
enum Daftar<T> { Isi(T, Daftar<T>), Habis }
enum Dua<A, B> { Kiri(A), Kanan(B), Titik { x: A, y: B } }
fn buka<T>(p: Pilih<T>, d: T) -> T { match p { Pilih::Ada(x) => x, Pilih::Kosong => d } }
fn panjang<T>(l: Daftar<T>) -> int { match l { Daftar::Isi(_, s) => 1 + panjang(s), Daftar::Habis => 0 } }
impl<T> Pilih<T> {
    fn ada(self) -> bool { match self { Pilih::Ada(_) => true, Pilih::Kosong => false } }
}
pub fn main() {
    let a = Pilih::Ada(5)
    let b: Pilih<string> = Pilih::Kosong
    wok(a, b, typeof(a))
    wok(buka(a, 0), buka(b, "x"), buka(Pilih::Ada("y"), "z"))
    wok(a.ada(), b.ada())
    var l: Daftar<int> = Daftar::Habis
    for i in 0..4 { l = Daftar::Isi(i, l) }
    wok(l, panjang(l))
    let d: Dua<int, string> = Dua::Kanan("k")
    let e: Dua<int, string> = Dua::Titik { x: 1, y: "s" }
    wok(d, e, Dua::Kiri(2.5))
    wok(a == Pilih::Ada(5), a == Pilih::Ada(6))
    match e { Dua::Titik { x, y } => wok(x, y), _ => wok("lain") }
    let xs = [Pilih::Ada(1), Pilih::Kosong, Pilih::Ada(3)]
    wok(xs)
}"#),
    ("enum generik: varian unit tanpa konteks dan varian telanjang", r#"package main
enum G<T> { V(T), C }
pub fn main() {
    let k = G::C
    wok(k)
    let m = V(3)
    wok(m, C)
}"#),
    // Fase JSON-JEMBATAN (#851): dulu "belum bisa disandikan jadi JSON di backend `--emit-llvm`".
    ("json: struct, enum, tuple dan wadahnya", r#"package main
struct N { pub v: int, pub kiri: Option<N> }
enum E { A, B(int) }
pub fn main() { let n = N { v: 1, kiri: Some(N { v: 2, kiri: None }) }
 wok(json::stringify(n), json::stringify([E::A, E::B(3)]), json::stringify((1, "x")))
 wok(json::pretty(n))
 wok(json::pretty({"k": E::B(1)}, 4), json::stringify([1, 2])) }"#),
    // Fase ENUM-JEMBATAN (#850): enum milik program dulu tidak bisa menyeberang jembatan, jadi struct
    // apa pun yang memuat `Option` ditolak; argumen struct/enum langsung ditolak saat kompilasi.
    ("jembatan: struct berisi enum, enum dan struct rekursif", r#"package main
struct Q { pub v: int, pub o: Option<int> }
struct N { pub v: int, pub kiri: Option<N> }
enum Bentuk { Titik, Lingkar(float), Kotak { w: int, h: int }, Garis(int, int) }
pub fn main() { let c = Chan::new(1)
 c <- Q { v: 1, o: Some(5) }
 let r = <-c
 wok(r.v, r.o)
 wok(dok::json_encode(N { v: 1, kiri: Some(N { v: 2, kiri: None }) }))
 wok(dok::json_encode([Bentuk::Titik, Bentuk::Lingkar(1.5), Bentuk::Kotak { w: 2, h: 3 }, Bentuk::Garis(4, 5)]))
 wok(ptr::get(ptr::new(Bentuk::Garis(1, 2))), dok::json_encode((1, "a"))) }"#),
    // Fase KOTAK-LLVM (#849): tipe yang memuat dirinya sebaris dulu ditolak LLVM (#836); field yang
    // menutup siklusnya sekarang kotak di heap — salinan tidak berbagi, tampil/`==`/pola lewat isinya.
    ("tipe rekursif: enum memuat dirinya", r#"package main
enum L { Kosong, Isi(int, L) }
fn jumlah(l: L) -> int { match l { L::Kosong => 0, L::Isi(x, sisa) => x + jumlah(sisa) } }
pub fn main() { var l = L::Kosong
 for i in 0..100 { l = L::Isi(i, l) }
 wok(jumlah(l))
 var k = L::Kosong
 for i in 0..3 { k = L::Isi(i, k) }
 let m = k
 wok(k, m == k, L::Isi(1, L::Kosong) == L::Isi(2, L::Kosong)) }"#),
    ("tipe rekursif: struct lewat Option, salinan tidak berbagi", r#"package main
struct N { pub v: int, pub kiri: Option<N> }
impl N { fn jumlah(self) -> int { match self.kiri { Some(k) => self.v + k.jumlah(), None => self.v } } }
pub fn main() { var a = N { v: 1, kiri: Some(N { v: 2, kiri: Some(N { v: 3, kiri: None }) }) }
 var c = a
 c.v = 9
 var m = N { v: 7, kiri: None }
 let n = N { v: 8, kiri: Some(m) }
 m.v = 70
 wok(a, c.v, a.jumlah(), a == c)
 wok(n, m)
 a.kiri = None
 wok(a, c.jumlah())
 match n.kiri { Some(N { v, kiri }) => wok(v, kiri), None => wok("kosong") } }"#),
    ("tipe rekursif: struct generik, varian ber-field, tuple", r#"package main
struct T<X> { pub v: X, pub a: Option<T<X>> }
enum Pohon { Daun(int), Cabang { kiri: Pohon, kanan: Pohon } }
struct P { pub x: (int, Option<P>) }
fn total(p: Pohon) -> int { match p { Pohon::Daun(x) => x, Pohon::Cabang { kiri, kanan } => total(kiri) + total(kanan) } }
pub fn main() { let t = T { v: 1, a: Some(T { v: 2, a: None }) }
 wok(t)
 let p = Pohon::Cabang { kiri: Pohon::Daun(1), kanan: Pohon::Cabang { kiri: Pohon::Daun(2), kanan: Pohon::Daun(3) } }
 wok(p, total(p))
 var q = P { x: (1, Some(P { x: (2, None) })) }
 var r = q
 r.x.0 = 9
 wok(q, r)
 let v = len(env::args()) > 99 $ 1 : t
 wok(v, [t, t][1].v) }"#),
    // Fase POTONG-BERTAG (#848): potongan atas nilai bertag dulu ditolak saat kompilasi.
    ("potong: nilai bertag berisi teks", "package main\npub fn main() { let v = len(env::args()) > 99 $ 5 : \"halo dunia\"\n wok(v[0:4], v[5:len(v)], v[:2])\n let s = crypto::hex_encode(\"ab\")\n wok(s[1:3]) }"),
    // Fase BAGI-LLVM: `gcd` atas MIN dulu menjawab bertanda salah (`-2`).
    ("math::gcd dan lcm atas bilangan negatif", "package main\npub fn main() { let m = -9223372036854775807 - 1\n wok(math::gcd(m, 6))\n wok(math::gcd(-12, 18))\n wok(math::lcm(-4, 6))\n wok(math::lcm(0, 5))\n wok(math::gcd(0, 0)) }"),
    // Fase BAGI-LLVM: `akhir - awal` di luar 64 bit dulu memberi panjang karangan (`1`, bukan `18`).
    ("range: selisih di luar 64 bit", "package main\npub fn main() { var a = 9000000000000000000\n var b = -9000000000000000000\n wok(len(range(a, b, -1000000000000000000)))\n var t = 0\n for i in range(9223372036854775800, 9223372036854775807) { t = t + 1 }\n wok(t)\n wok(range_inclusive(6, 3))\n wok(range(10, 0, -4)) }"),
    // Fase BAGI-LLVM: `srem` atas MIN dan -1 tak terdefinisi; dulu menjawab `1`.
    // Fase HOF-LLVM (#832): bukan fungsi atas koleksi kosong tidak pernah dipanggil, jadi tidak gagal.
    ("hof: bukan fungsi atas koleksi kosong", "package main\nfn nol() -> int { 1 }\npub fn main() { var a: []int = []\n wok(a.map(5), a.reduce(0, 1), a.filter(true))\n wok(a.all(|| 1), a.find(nol), a.fold(2, |s| s)) }"),
    // Fase GESER-LLVM (#831): geseran yang muat di 64 bit, juga tak bertanda dan sampai MIN.
    ("geser: hasil yang muat di 64 bit", "package main\npub fn main() { var k = 63\n wok(-1 << k)\n var j = 62\n wok(1 << j, -1 << j)\n var c: u64 = 1\n var m: u64 = 63\n wok(c << m)\n wok(-8 >> k) }"),
    ("bagi: sisa MIN % -1 nol", "package main\npub fn main() { var m = -9223372036854775807 - 1\n var n = -1\n wok(m % n)\n m %= -1\n wok(m)\n wok(-7 % n)\n wok(-7 / n) }"),
    // Fase POTONG: dulu ditolak saat kompilasi ("tidak bisa slicing pada tipe").
    ("potong: tuple berbatas literal", "package main\npub fn main() { let t = (1, \"x\", 2.5)\n let u = t[1:]\n wok(u)\n wok(u.1 + 1.0)\n wok(t[-1:])\n wok(t[:2]) }"),
    // Fase KUNCI-LLVM (#824): `len`/`is_empty` sebagai field atas teks dan array; method skalar tanpa `()`.
    ("indeks teks atas struct", r#"package main
struct P { pub x: int, pub n: string, y: int = 3 }
impl P { fn baca(self) -> int { self["y"] * 2 } }
pub fn main() {
  var p = P { x: 1, n: "a" }
  wok(p["x"] + 1, p["n"], typeof(p["n"]))
  p["x"] = 5
  p["x"] += 2
  wok(p.x, p.baca())
}
"#),
    ("len dan is_empty sebagai field", r#"package main
pub fn main() {
  let s = "éa"
  wok(s.len, s.is_empty, "".is_empty)
  let a = [1, 2]
  wok(a.is_empty, a.len)
  let f = 2.5
  wok(f.floor)
}"#),
    // Fase KUNCI-LLVM (#823): method skalar acuan lewat `math::`, `to_string` atas skalar.
    ("method skalar int dan float", r#"package main
pub fn main() {
  let f = 2.7
  let i = -3
  let r: i32 = -4
  let u: u8 = 9
  wok(f.ceil(), f.round(), (-1.5).abs(), f.pow(2), f.ln(), f.sqrt(), f.is_finite(), f.is_inf())
  wok(i.abs(), r.abs(), u.abs(), i.to_string() + "!", f.to_string(), true.to_string(), 'q'.to_string())
}"#),
    // Fase KUNCI-LLVM (#822): method acuan yang dulu cuma ada di bentuk namespace.
    ("method repeat atas teks dan set atas dict", r#"package main
pub fn main() {
  let s = "ab"
  wok(s.repeat(3), "x".repeat(0))
  let d = {"a": 1}
  wok(d.set("b", 2), d)
}"#),
    // Fase KUNCI-LLVM (#821): penerima method yang bukan variabel dibaca sebagai nilai.
    ("method atas None, const dan varian telanjang", r#"package main
enum W { Merah, Biru }
impl W { fn nama(self) -> string = match self { W::Merah => "m", W::Biru => "b" } }
const K = "ab"
pub fn main() {
  wok(None.unwrap_or(7))
  wok(K.upper())
  wok(Merah.nama())
  wok(None.is_none())
}"#),
    // Fase SEBAR-TIPE: field yang ditulis literalnya tidak dibaca dari basis; field bertag dibuka.
    ("sebar dari struct lain berbeda tipe field", r#"package main
struct A { pub x: int, pub y: string }
struct B { pub x: int, pub y: int }
struct D { pub x: tuple, pub y: tuple }
struct E { pub x: int, pub y: tuple }
pub fn main() {
    let b = B { x: 1, y: 2 }
    let a = A { y: "teks", ...b }
    wok(a.x, a.y)
    let d = D { x: 3, y: "dy" }
    let a4 = A { ...d }
    wok(a4.x, a4.y)
    let e = E { ...a }
    wok(e.x, e.y, typeof(e.y))
}
"#),
    // Fase PTR-LLVM: `Ptr` sebagai tipe field, parameter dan anotasi; pegangan `jawa::` di dalamnya.
    ("Ptr sebagai tipe field dan anotasi", r#"package main
struct Kotak { pub p: Ptr, pub n: int }
struct Sel { pub h: Ptr }
fn ambil() -> Ptr { ptr::new(3) }
fn baca(h: Ptr) -> int { jawa::get(h) }
pub fn main() {
    let k = Kotak { p: ptr::new(5), n: 1 }
    ptr::set(k.p, 9)
    var q = k
    ptr::set(q.p, 11)
    wok(ptr::get(k.p), k.n)
    let p: Ptr = ambil()
    let xs: []Ptr = [p, ptr::new(4)]
    wok(ptr::get(p), typeof(p), ptr::get(xs[1]))
    let a: Ptr = jawa::alloc(7)
    wok(baca(a), typeof(a))
    let s = Sel { h: jawa::alloc(8) }
    jawa::set(s.h, 9)
    wok(jawa::get(s.h))
    jawir::free(a)
    wok(jawa::is_valid(a))
}
"#),
    // Fase PEGANGAN-BERTAG: `Atomic`/`Mutex` di dalam nilai bertag, bentuk namespace dan method.
    ("pegangan sinkron di dalam nilai bertag", r#"package main
fn pilih(n: int) { if n == 0 { return Mutex::new() } if n == 1 { return [7, 8] } return Atomic::new(2) }
pub fn main() {
    let m = pilih(0)
    mutex::lock(m)
    wok("kunci", typeof(m))
    mutex::unlock(m)
    wok(mutex::with_lock(m, || 5), m.with_lock(|| 6))
    let b = pilih(2)
    atomic::add(b, 3)
    wok(atomic::cas(b, 5, 10), atomic::get(b))
    let c = b
    atomic::set(c, 1)
    wok(b.get(), b.add(1), b.cas(2, 9), b.get())
    wok(pilih(1).get(1))
}
"#),
    // Fase KEPUTUSAN-1: `-2 ** 2` = -4, indeks negatif di builtin dari belakang.
    ("pangkat lawan minus, indeks negatif di builtin", r#"package main
pub fn main() {
    let x = 3
    wok(-2 ** 2, -x ** 2, (-2) ** 2, ~2 ** 2, -2 ** 3 ** 2, -128 as i8, - 2 ** 2 + 1, -2.0 ** 2)
    let a = [10, 20, 30]
    wok(a.get(-1), a.get(-4), a.get_or(-1, 0), a.get_or(-9, 0))
    wok(a.remove(-1), a.insert(-1, 9), arr::insert(a, -3, 5), a.swap(-1, 0), "abc".char_at(-1))
}
"#),
    // Fase JAWA-LLVM: `jawa::array(n, isi)` dengan isi awal.
    ("jawa::array dengan isi awal", r#"package main
pub fn main() {
    let a = jawa::array(3, 7)
    jawa::array_set(a, 1, 9)
    wok(jawa::array_get(a, 0), jawa::array_get(a, 1), jawa::array_len(a), jawa::array_to_list(a))
    jawir::free(a)
    wok(jawa::is_valid(a))
}
"#),
    // Fase PETA-OPSI: `map`/`map_err`/`and_then` atas Option dan Result, termasuk yang mengganti tipe isinya.
    ("peta atas Option dan Result", r##"package main
fn bagi(a: float, b: float) -> Result<float, string> {
    if b == 0.0 { return Err("nol") }
    return Ok(a / b)
}
fn cari(xs: []int, x: int) -> Option<int> {
    for i, v in xs { if v == x { return Some(i) } }
    return None
}
fn ke_teks(n: int) -> string { return "#" + str(n) }
pub fn main() {
    wok(Some(2).map(|x| x + 1), cari([1, 2], 9).map(|i| i * 10))
    wok(bagi(20.0, 4.0).map(fn(v) => v * 2.0), bagi(1.0, 0.0).map(|v| v * 2.0))
    wok(bagi(1.0, 0.0).map_err(|s| s + "!"), bagi(2.0, 1.0).map_err(|s| s + "!"))
    wok(cari([5, 6], 6).and_then(|i| cari([0, 1, 2], i)), cari([5], 6).and_then(|i| Some(i)))
    wok(bagi(8.0, 2.0).and_then(|v| bagi(v, 2.0)), bagi(8.0, 0.0).and_then(|v| bagi(v, 2.0)))
    wok(cari([3, 4], 4).map(ke_teks), Some(1).map(|x| x + 1).map(|x| x * 5))
    let urutan = Some(1).map(|x| { wok("di dalam f"); x })
    wok(urutan)
}
"##),
    // Fase KTOR-LLVM: konstruktor enum non-generik sebagai nilai — tampilan, `typeof`, HOF, `|>`.
    ("konstruktor sebagai nilai", r#"package main
enum E { A(int), B, C(int, string) }
enum W { K(string), L }
fn terap(f: fn(string) -> W, x: string) -> W { return f(x) }
fn dua(x: int) -> int { return x * 2 }
pub fn main() {
    let f = E::A
    wok(f, typeof(f), [f], str(f))
    wok(f(3), arr::map([1, 2], E::A))
    let g = E::C
    wok(g(1, "x"))
    let k = K
    wok(k("a"), "b" |> W::K, terap(W::K, "c"))
    let fs = [W::K, W::K]
    wok(fs[1]("d"))
    wok(dua, typeof(dua), typeof(|x| x))
}
"#),
    // Fase SUBSTR: awal `str::substr` negatif, dan eksponen negatif TERTULIS menjawab pecahan.
    ("substr awal negatif, pangkat negatif tertulis", r#"package main
fn separuh(x: int) { return x ** -1 }
pub fn main() {
    let s = "halo dunia"
    wok(str::substr(s, -5, 3), s.substr(-5), str::substr(s, -10, 4), "[" + str::substr(s, 10) + "]", str::substr("héllo", -4, 2))
    wok(2 ** -1, 3 ** -2, typeof(2 ** -1), (-2) ** -3, 10 ** -3 * 1000.0, separuh(4))
    let x = 5
    let h = x ** -2
    wok(h, typeof(h))
}
"#),
    // Fase JRGM-2: `http::` menjawab `Result` (dulu galat saat jalan di tiga mesin).
    ("http: gagal sambung dan URL tak sah jadi Err", r#"package main
pub fn main() {
    match http::get("http://127.0.0.1:1/x") { Ok(j) => wok("ok"), Err(e) => wok("err", str::contains(e, "sambungan ditolak")) }
    wok(http::get("http://127.0.0.1:abc/"))
}
"#),
    // Fase PEGANGAN-LLVM: dulu entri `BELUM_DIDUKUNG_LLVM` ("`i64` sebagai `Atomic`").
    ("pegangan dibandingkan dengan jenis lain menjawab false", r#"package main
pub fn main() {
 let a = Atomic::new(1)
 let m = Mutex::new()
 let c = make_chan(int)
 wok(a == 1, a != 1, m == "x", c == nil, 1 == a, a == m)
}
"#),
    // Fase KALIMAT: `jawa::sizeof` atas nilai biasa menaksir ukurannya, dulu ditolak LLVM.
    ("jawa::sizeof atas nilai yang bukan pegangan", r#"package main
pub fn main() {
 wok(jawa::sizeof(5), jawa::sizeof("abc"), jawa::sizeof([1,2]), jawa::sizeof(2.5), jawa::sizeof(true))
 let b = jawa::buffer(4)
 wok(jawa::sizeof(b))
 wok(jawa::sizeof({"a": 1}))
}
"#),
    // Fase HEKS: buffer dari teks heksa lahir di runtime LLVM, bukan di registri jembatan.
    ("jawa::buffer_from_hex dibaca buffer_get dan buffer_to_hex", r#"package main
pub fn main() {
 let b = jawa::buffer_from_hex("00ff41")
 wok(jawa::buffer_get(b, 1))
 wok(jawa::buffer_len(b))
 let c = jawa::buffer(3)
 wok(jawa::buffer_get(c, 0))
}
"#),
    ("jawa::buffer_to_hex atas buffer dari teks heksa", r#"package main
pub fn main() {
 let c = jawa::buffer(2)
 jawa::buffer_set(c, 0, 171)
 wok(jawa::buffer_to_hex(c))
 wok(jawa::buffer_to_hex(jawa::buffer_from_hex("ABcd")))
 wok(jawa::buffer_len(jawa::buffer_from_hex("")))
}
"#),
    // Fase SALIN-MODUL: nilai milik `var` modul yang dibaca lewat penunjuk disalin kalau yang
    // dipanggil sesudahnya bisa menulis `var` modul. Yang pertama dulu entri `DIVERGENSI_LLVM`.
    ("operan array var modul ditulis operan berikutnya",
     "package main\nvar A = [1, 2]\nfn ubah() -> []int { A[0] = 4; return [5] }\npub fn main() { let x = A + ubah()\n wok(x) }"),
    ("self milik var modul tidak ikut tulisan fungsi lain, operan kiri dibaca lebih dulu", r#"package main
struct K { pub n: int, pub xs: []int }
var G = K { n: 1, xs: [1] }
fn ubah() { G = K { n: 99, xs: [9, 9] } }
impl K {
    fn f(self) -> int {
        let a = self.n
        ubah()
        return a + self.n + len(self.xs)
    }
    fn baca(self) -> int { return self.n }
}
var A = [1, 2, 3]
fn tulis() -> int { A[0] = 4; return 5 }
fn ubah2() -> []int { A[0] = 7; return [5] }
pub fn main() {
    wok(G.f())
    wok(G.baca())
    wok(A + [tulis()])
    wok(A + ubah2())
    let x = A + [1]
    wok(x)
}
"#),
    // Fase RUANG-LLVM: pola varian atas nilai yang pasti bukan enum tidak cocok, bukan ditolak.
    ("pola varian telanjang dan berawalan atas struct jatuh ke lengan lain", r#"package main
enum E { X(int), Y }
struct P { a: int }
pub fn main() {
 let p = P { a: 1 }
 wok(match p { Y => "y", _ => "lain" })
 wok(match p { E::Y => "y", _ => "lain" })
}
"#),
    // Fase GENAP: pintu byte dan `fs::` yang dulu tidak ada di sini, lewat jembatan.
    ("crypto: array byte dan aes_decrypt_bytes", r#"package main
pub fn main() {
    let k = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
    let iv = "000102030405060708090a0b0c0d0e0f"
    let s = crypto::aes_encrypt([0, 255, 65], k, iv).unwrap()
    wok(crypto::aes_decrypt(s, k, iv))
    wok(crypto::aes_decrypt_bytes(s, k, iv))
    wok(crypto::aes_decrypt_bytes(crypto::aes_encrypt("hé", k, iv).unwrap(), k, iv))
    wok(crypto::aes_decrypt_bytes(s, "00", iv))
    wok(crypto::base64_encode([0, 255, 65]), crypto::base64_encode("hé"))
    wok(crypto::hex_encode([0, 255]), crypto::hex_encode("A"))
    wok(crypto::sha256([104, 105]) == crypto::sha256("hi"), crypto::md5([]) == crypto::md5(""))
    wok(crypto::crc32([97]), crypto::crc32("a"), crypto::hmac_sha256([104], "x") == crypto::hmac_sha256("h", "x"))
    wok(crypto::base64_encode(crypto::base64_decode_bytes("AP9B").unwrap()))
}
"#),
    ("fs: try_read dan pintu base64 lewat jembatan", r#"package main
pub fn main() {
    let d = fs::join(fs::temp_dir(), "jrg-l3")
    fs::mkdir(d)
    let f = fs::join(d, "b.bin")
    wok(fs::write_base64(f, "AP9B"))
    wok(fs::read_base64(f))
    wok(fs::try_read(f))
    wok(fs::try_read(fs::join(d, "hilang")))
    wok(fs::append_base64(f, "Qg=="))
    wok(fs::size(f))
    fs::write(fs::join(d, "t.txt"), "hé")
    wok(fs::try_read(fs::join(d, "t.txt")))
    fs::delete(d)
    wok(fs::exists(d))
}
"#),
    // Fase UTF8-LLVM: berkas bukan UTF-8 dibaca sebagai teks dijawab `Err`, bukan teks kosong; `env::exe` ada.
    ("fs: berkas bukan UTF-8 lewat file::, env::exe", r#"package main
pub fn main() {
    let f = fs::join(fs::temp_dir(), "jrg-l3-utf8.bin")
    fs::write_base64(f, "//4K")
    wok(file::read(f))
    wok(file::read_lines(f))
    wok(fs::try_read(f))
    fs::copy(f, f + ".2")
    wok(fs::size(f + ".2"))
    fs::delete(f + ".2")
    fs::write(f, "hé\nya")
    wok(file::read_lines(f), fs::read(f))
    fs::delete(f)
    wok(env::exe().len() > 0, fs::exists(env::exe()))
}
"#),
    ("dok: BLOB SQLite jadi array byte, TEXT bukan UTF-8 jadi Err", r#"package main
pub fn main() {
    if !dok::db_tersedia() { wok("tanpa sqlite")  return }
    let db = dok::db_open(":memory:").unwrap()
    wok(dok::db_query(db, "SELECT x'00ff41' AS b, 'é' AS t, 3 AS n"))
    wok(dok::db_query(db, "SELECT CAST(x'ff41' AS TEXT) AS rusak"))
    wok(dok::db_query(db, "SELECT CAST(CAST(x'ff41' AS TEXT) AS BLOB) AS rusak"))
    dok::db_close(db)
}
"#),
    ("dok: tcp_recv tidak membelah karakter, pintu byte TCP dan UDP", r#"package main
fn port(l) {
    let a = dok::tcp_local_addr(l)
    let p = str::split(a, ":")
    return str::to_int(p[p.len() - 1])
}
fn port_udp(u) {
    let p = str::split(dok::udp_local_addr(u), ":")
    return str::to_int(p[p.len() - 1])
}
pub fn main() {
    let l = dok::tcp_listen("127.0.0.1", 0).unwrap()
    let k = dok::tcp_connect("127.0.0.1", port(l), 2000).unwrap()
    let c = dok::tcp_accept(l).unwrap()
    dok::tcp_send(k, "aaaé€b").unwrap()
    wok(dok::tcp_recv(c, 4))
    wok(dok::tcp_recv(c, 4))
    wok(dok::tcp_recv(c, 1))
    wok(dok::tcp_recv(c, 4))
    dok::tcp_send(k, [97, 255, 98]).unwrap()
    wok(dok::tcp_recv(c, 8))
    wok(dok::tcp_recv(c, 8))
    wok(dok::tcp_recv_bytes(c, 8))
    dok::tcp_send(k, [0, 200, 10]).unwrap()
    wok(dok::tcp_recv_line(c))
    dok::tcp_send(k, [226, 130]).unwrap()
    dok::tcp_close(k)
    wok(dok::tcp_recv(c, 8))
    wok(dok::tcp_recv(c, 8))
    dok::tcp_close(c)
    dok::tcp_close_listener(l)
    let u = dok::udp_bind("127.0.0.1", 0).unwrap()
    let v = dok::udp_bind("127.0.0.1", 0).unwrap()
    let pu = port_udp(u)
    dok::udp_send_to(v, "127.0.0.1", pu, "héé").unwrap()
    wok(dok::udp_recv_from(u, 64).unwrap().data)
    dok::udp_send_to(v, "127.0.0.1", pu, "héé").unwrap()
    wok(dok::udp_recv_from(u, 2).unwrap().data)
    dok::udp_send_to(v, "127.0.0.1", pu, [1, 255]).unwrap()
    wok(dok::udp_recv_from(u, 64))
    wok(dok::udp_recv_bytes_from(u, 64).unwrap().data)
}
"#),
    ("crypto: dekode byte bukan teks", r#"package main
pub fn main() {
    wok(crypto::base64_decode_bytes("AP9B"))
    wok(crypto::base64_decode("AP9B"))
    wok(crypto::hex_decode_bytes("00ff"))
}
"#),
    // Fase SEBAR: basis bertipe lain sah selama field-nya bagian dari sasaran.
    ("basis sebar bertipe struct lain yang field-nya bagian dari sasaran", r#"package main
struct B { pub x: int, pub y: int = 7 }
struct H { pub x: int }
struct K: H { pub k: int }
struct N { pub nama: string, pub x: int }
struct M { pub x: int, pub nama: string, pub z: int = 3 }
pub fn main() {
    let h = H { x: 4 }
    wok(K { k: 1, ...h })
    wok(B { ...h })
    let n = N { nama: "nn", x: 8 }
    wok(M { ...n })
    wok(M { x: 1, ...n })
    let b = B { x: 1, y: 2 }
    wok(B { x: 5, ...b })
}"#),
    // Fase BINER: LLVM dulu menjawab "tipe `V` tidak dikenal" untuk varian yang ditulis telanjang.
    ("literal varian ber-field yang ditulis telanjang", r#"package main
enum E { V { a: int, b: int }, K }
fn lihat(e: E) -> int {
    match e { E::V { a, b } => a + b, E::K => 0 }
}
pub fn main() {
    let e = V { a: 1, b: 2 }
    wok(lihat(e))
    wok(lihat(V { b: 5, a: 4 }))
    wok(V { a: 7, b: 8 })
}"#),
    // Fase PULIH: `A { ...a }` dulu ditolak parser di tiga mesin.
    ("literal struct yang isinya cuma basis sebar",
     "package main\nstruct A { pub x: int, pub y: int }\npub fn main() {\n let a = A { x: 1, y: 2 }\n wok(A { ...a })\n let b = A { ...a }\n wok(b.x + b.y)\n}"),
    // Fase WAJIB: urutan evaluasi default field sama di tiga mesin.
    ("default field struct: hanya yang tidak diisi, sesudah field tertulis", r#"package main
fn catat(s: string) -> int { wok(s); return 1 }
struct A { pub x: int = catat("default A.x"), pub y: int = catat("default A.y"), pub z: int = catat("default A.z") }
struct B: A { pub w: int = catat("default B.w"), pub y: int = catat("default B.y") }
pub fn main() {
    wok(A { y: catat("tulis y") })
    let a = A { x: 5, y: 6, z: 7 }
    wok(A { y: catat("tulis y2"), ...a })
    wok(B { z: catat("tulis z") })
    wok(B {})
}
"#),
    // Fase BONGKAR: `q, v = q.pop()` di biner berjalan di tempat `q`. 200 ribu elemen —
    // bentuk dua pernyataan kehabisan memori di ukuran ini.
    ("std/queue, deque, heap: q, v = q.pop() atas 200 ribu elemen", r#"package main
import "std/queue"
import "std/deque"
import "std/heap"
pub fn main() {
    var q: queue::Queue<int> = queue::new()
    var d: deque::Deque<int> = deque::new()
    var h: heap::Heap<int, int> = heap::new()
    var s = 0
    var v: Option<int> = None
    for i in 0..200000 { q = q.push(i)
      d = d.push_front(i) }
    for i in 0..20000 { h = h.push((i * 7919) % 10007) }
    for i in 0..200000 { q, v = q.pop()
      s = s + (v ?? 0)
      d, v = d.pop_back()
      s = s + (v ?? 0) }
    for i in 0..20000 { h, v = h.pop()
      s = s + (v ?? 0) }
    wok(s); wok(q.len()); wok(d.len()); wok(h.len()); wok(q.pop()[1])
}
"#),
    // Fase TUTUP: array literal yang meluruh jadi slice DISALIN ke heap. Dulu slice-nya
    // menunjuk stack fungsinya, dan `return [v, v + 1]` menjawab sampah.
    ("array literal jadi slice lalu keluar dari fungsi: return, field, Some", r#"package main
fn f(v: int) -> []int { return [v, v + 1] }
fn g(v: int) -> []int { let a = [v, v * 2]
  return a }
fn kotor(n: int) -> int { var t = [9, 9, 9, 9, 9, 9, 9, 9]
  return t[n] + n }
struct K { pub xs: []int }
fn h(v: int) -> K { return K { xs: [v, 7] } }
fn o(v: int) -> Option<[]int> { return Some([v]) }
pub fn main() {
    let a = f(3)
    let b = g(5)
    let c = h(4)
    let d = o(6)
    wok(kotor(1))
    wok(a); wok(b); wok(c); wok(d)
}
"#),
    ("match di ekor fungsi dengan lengan array beda panjang; return [v] lawan return []", r#"package main
fn ta(o) {
    match o {
        Some(v) => [v],
        None    => [],
    }
}
fn tb(o) {
    match o {
        Some(v) => return [v, v],
        None => return [],
    }
}
pub fn main() { wok(ta(Some(3))); wok(typeof(ta(Some(3)))); let k: Option<int> = None
  wok(ta(k)); wok(tb(Some(4))); wok(tb(k)) }
"#),
    ("separuh tipe tak terlihat jadi nilai bertag: None, Ok, Err, var x = nil", r#"package main
pub fn main() {
    var kosong = nil
    kosong = 5
    wok(kosong); kosong = "teks"; wok(kosong)
    var x = None
    wok(x); x = Some(3); wok(x); x = Some("teks"); wok(x)
    var r = Ok(4)
    wok(r); r = Err("rusak"); wok(r)
    let e = Err(5)
    wok(e); wok(typeof(e)); wok(e == Err(5)); wok(None == None)
    match x { Some(v) => wok(v), None => wok("kosong") }
    let daftar = [None, Some(1)]
    wok(daftar); wok(len(daftar))
    wok(x ?? "bawaan"); let y = None
    wok(y ?? 7)
}
"#),
    ("x = f(x, ..) dan nama tertangkap: closure melihat nilai LAMA selagi fungsinya jalan", r#"package main
struct K { pub xs: []int }
impl K {
    pub fn terap(self, f: fn() -> int) -> K { self.xs += [1]
      self.xs += [f()]
      return self }
}
fn pakai(a: []int, f: fn() -> int) -> []int { a = arr::push(a, 1)
  a = arr::push(a, f())
  return a }
pub fn main() {
    var p = K { xs: [5] }
    let g = || len(p.xs)
    p = p.terap(g)
    wok(p.xs)
    var a = [5]
    let h = || len(a)
    a = pakai(a, h)
    wok(a)
    var b = [1]
    let b0 = b
    b = pakai(b, || 0)
    wok(b); wok(b0)
}
"#),
    ("x = f(x, ..): dict::set, dict::remove, fungsi pemakai — tanpa alias", r#"package main
struct K { pub xs: []int }
fn tambah(k, x) { k.xs += [x]
  return k }
fn isi(d, k, v) { d[k] = v
  return d }
fn dorong(a, x) { a = arr::push(a, x)
  return a }
fn rek(d, n) { if n == 0 { return d }
  d = isi(d, n, n * n)
  d = rek(d, n - 1)
  return d }
fn banyak(a, xs...) { for _, x in xs { a = a + [x] }
  return a }
fn bawaan(a, x = 7) { a = a + [x]
  return a }
fn gagal(d, i) { let a = [1]
  d["x"] = a[i]
  return d }
pub fn main() {
    var k = K { xs: [1] }
    let k0 = k
    var k1 = k
    k = tambah(k, 2)
    k = tambah(tambah(k, 3), 4)
    wok(k); wok(k0); wok(k1)
    var d = {"a": 1}
    let d0 = d
    d = dict::set(d, "b", 2)
    d = isi(d, "c", 3)
    d = dict::remove(d, "a")
    d = dict::remove(d, "tak ada")
    wok(d); wok(d0)
    let f = || { d = dict::set(d, "f", 0) }
    f()
    wok(d)
    var a = [1]
    let a0 = a
    a = dorong(a, 2)
    a = banyak(a, 3, 4)
    a = bawaan(a)
    a = bawaan(a, 9)
    wok(a); wok(a0)
    wok(rek({"0": 0}, 4))
    var e = {"z": 1}
    e = rek(e, 2)
    wok(e)
    var s = "ab"
    s = str::upper(s)
    wok(s)
    var t = {"k": 1}
    wok("sebelum")
}
"#),
    // Fase BERTIPE: penyalin-dalam menembus struct bersarang dan payload enum; nilai yang
    // dikembalikan dari `self` pinjaman, dari pengikat pola, dan dari `var` modul disalin.
    // Dua puluh dari 34 baris ini dulu dijawab salah tanpa pesan.
    ("alias: kembalian dari self, struct bersarang, enum, pola, var modul", r#"package main
struct K { pub a: []int, pub b: []int }
impl K {
    pub fn sama(self) -> K { return K { a: self.a, b: self.b } }
    pub fn dorong(self, x: int) -> K { return K { a: arr::push(self.a, x), b: self.b } }
    pub fn isi(self) -> []int { return self.a }
}
fn lewat(k: K) -> K { return k }
fn ambil(k: K) -> []int { return k.b }
pub fn main() {
    var k = K { a: [1, 2], b: [7, 8] }
    var m = k.sama()
    m.a[0] = 9; m.b[1] = 0
    wok(k); wok(m)
    var n = k.dorong(3)
    n.b[0] = 100; n.a[0] = 50
    wok(k); wok(n)
    var x = k.isi()
    x[1] = 77
    wok(k); wok(x)
    var l = lewat(k)
    l.a[0] = -1
    var y = ambil(k)
    y[0] = -2
    wok(k); wok(l); wok(y)
    let p = k.dorong(4)
    let r = k.dorong(5)
    wok(p.a); wok(r.a)
    var t = (k.a, k.b)
    t[0][0] = 31
    wok(k); wok(t)
    kedua()
    ketiga()
}
struct D { pub d: dict<int>, pub n: K, pub o: Option<[]int> }
impl D {
    pub fn diri(self) -> D { return self }
    pub fn dik(self) -> dict<int> { self.d }
    pub fn dalam(self) -> []int = self.n.a
    pub fn pasang(self) -> (K, []int) { return (self.n, self.n.b) }
    pub fn pilih(self, c: bool) -> []int { if c { return self.n.a }
      return self.n.b }
    pub fn opsi(self) -> []int { match self.o { Some(v) => return v, None => return [] } }
}
pub fn kedua() {
    var d = D { d: {"a": 1}, n: K { a: [1, 2], b: [3] }, o: Some([5, 6]) }
    var e = d.diri()
    e.n.a[0] = 9; e.d["a"] = 2
    var f = d.dik()
    f["a"] = 3; f["z"] = 0
    var g = d.dalam()
    g[1] = 8
    var h = d.pasang()
    h[0].a[0] = 7; h[1][0] = 6
    var i = d.pilih(true)
    i[0] = 5
    var j = d.opsi()
    j[0] = 4
    wok(d); wok(e); wok(f); wok(g); wok(h); wok(i); wok(j)
}
var G = [1, 2]
enum Bentuk { Daftar([]int), Kosong }
fn ambil_g() { return G }
fn isi_bentuk(b: Bentuk) -> []int { match b { Bentuk::Daftar(xs) => return xs, Bentuk::Kosong => return [] } }
pub fn ketiga() {
    var x = ambil_g()
    x[0] = 9
    wok(G); wok(x)
    var o = Some([1, 2])
    var p = o
    match p { Some(v) => { v[0] = 7
        wok(v) } None => {} }
    wok(o); wok(p)
    var b = Bentuk::Daftar([3, 4])
    var c = b
    var z = isi_bentuk(c)
    z[0] = 0
    wok(isi_bentuk(b)); wok(isi_bentuk(c)); wok(z)
    var d1 = D { d: {"a": 1}, n: K { a: [1, 2], b: [3] }, o: Some([5, 6]) }
    var d2 = d1
    d2.n.a[0] = 99
    let ds = [d1, d2]
    var d3 = ds[0]
    d3.n.b[0] = 55
    wok(d1.n); wok(d2.n); wok(ds[0].n); wok(d3.n)
    let tp = (K { a: [1], b: [2] }, Some([3]))
    var tq = tp
    tq[0].a[0] = 4
    wok(tp); wok(tq)
}
"#),
    ("std/queue, deque, heap bertipe; parameter tipe tak terlihat jadi nilai bertag", r#"package main
import "std/queue"
import "std/deque"
import "std/heap"
struct Titik { pub x: int }
pub fn main() {
    var q: queue::Queue<Titik> = queue::new()
    q = q.push(Titik { x: 1 }).push(Titik { x: 2 })
    wok(q.len()); wok(q.pop()[1]); wok(q.peek())
    let s = queue::from(["a", "b"]).push("c")
    wok(s); wok(s.to_array()); s.each(|x| wok(x + "!"))
    var d: deque::Deque<float> = deque::new()
    d = d.push_back(1.5).push_front(0.5)
    wok(d); wok(d.pop_back()[1]); wok(d.peek_front())
    var h = heap::new()
    h = h.push(3).push(1).push(2)
    wok(h); wok(h.pop()[1]); wok(h.to_sorted_array())
    var bebas = queue::new()
    bebas = bebas.push(1).push("dua").push(3.5)
    wok(bebas); wok(bebas.pop()[1])
}
"#),
    ("method tanpa anotasi atas struct generik: closure tanpa hasil, struct baru dari penduga", r#"package main
struct S<T> { pub d: []T }
impl<T> S<T> {
    pub fn ubah(self, f) {
        var k = []
        for _, v in self.d { k = arr::push(k, f(v)) }
        return S { d: k }
    }
    pub fn tiap(self, f) { for _, v in self.d { f(v) } }
}
struct S2<T> { pub d: dict<T> }
impl<T> S2<T> {
    pub fn ubah(self, f) {
        var d = dict::new()
        for _, v in dict::values(self.d) {
            let y = f(v)
            d = dict::set(d, y, y)
        }
        return S2 { d: d }
    }
}
pub fn main() { let a = S { d: [1, 2] }
  a.tiap(|x| wok(x))
  wok(a.ubah(|x| x * 2).d)
  let b = S2 { d: {"1": 1, "2": 2} }
  wok(b.ubah(|x| x * 2).d) }
"#),
    // Fase GENERIK: struct generik dimonomorfisasi per susunan argumen tipe.
    ("struct generik: literal, method, fungsi generik, tipe dari tempat hasil", r#"package main
struct Kotak<T> { pub isi: []T, pub nama: string }
impl<T> Kotak<T> {
    pub fn tambah(self, x: T) -> Kotak<T> { return Kotak { isi: arr::push(self.isi, x), nama: self.nama } }
    pub fn banyak(self) -> int { return len(self.isi) }
    pub fn pertama(self) -> Option<T> { return arr::first(self.isi) }
}
fn baru<T>(nama: string) -> Kotak<T> { return Kotak { isi: [], nama: nama } }
fn id<T>(x: T) -> T { return x }
pub fn main() {
    var k: Kotak<int> = baru("angka")
    k = k.tambah(3)
    k = k.tambah(4)
    wok(k.banyak()); wok(k.pertama()); wok(k); wok(typeof(k))
    var t: Kotak<string> = baru("teks")
    t = t.tambah("a")
    wok(t); wok(id(5)); wok(id("x"))
    let l = Kotak { isi: [1.5], nama: "pecahan" }
    wok(l.tambah(2.5))
}
"#),
    ("struct generik: dua parameter, impl<U>, operator, hook, field fn, bersarang", r#"package main
struct Pasang<A, B> { pub kiri: A, pub kanan: B }
struct Tumpuk<T> { isi: []T, pub nama: string = "tumpukan" }
struct Titik { pub x: int, pub y: int }
struct Pilih<T> { pub f: fn(T) -> int, pub bawaan: Option<T> }
impl<A, B> Pasang<A, B> {
    pub fn tukar(self) -> Pasang<B, A> { return Pasang { kiri: self.kanan, kanan: self.kiri } }
    pub fn to_string(self) -> string { return "<" + str(self.kiri) + " | " + str(self.kanan) + ">" }
}
impl<U> Tumpuk<U> {
    pub fn dorong(self, x: U) -> Tumpuk<U> { return Tumpuk { isi: arr::push(self.isi, x), nama: self.nama } }
    pub fn ambil(self) -> (Tumpuk<U>, Option<U>) {
        let h = arr::pop(self.isi)
        return (Tumpuk { isi: h[0], nama: self.nama }, h[1])
    }
    pub fn tinggi(self) -> int { return len(self.isi) }
    pub fn eq(self, lain: Tumpuk<U>) -> bool { return self.isi == lain.isi }
}
impl<T> Pilih<T> {
    pub fn nilai(self, x: T) -> int { let f = self.f
      return f(x) }
}
fn kosong<T>() -> Tumpuk<T> { return Tumpuk { isi: [] } }
fn pasang<A, B>(a: A, b: B) -> Pasang<A, B> { return Pasang { kiri: a, kanan: b } }
fn pertama<T>(xs: []T) -> Option<T> { return arr::first(xs) }
fn jumlah(t: Tumpuk<int>) -> int { var s = 0
  var k = t
  for i in 0..t.tinggi() { let r = k.ambil()
    k = r[0]
    s = s + (r[1] ?? 0) }
  return s }
pub fn main() {
    let p = pasang(1, "satu")
    wok(p); wok(p.tukar()); wok(p.tukar().kiri); wok(typeof(p))
    var t: Tumpuk<Titik> = kosong()
    t = t.dorong(Titik { x: 1, y: 2 }).dorong(Titik { x: 3, y: 4 })
    wok(t.tinggi()); wok(t.nama)
    let r = t.ambil()
    wok(r[1]); wok(r[0].tinggi())
    var a: Tumpuk<int> = kosong()
    for i in 0..5 { a = a.dorong(i * i) }
    wok(jumlah(a)); wok(a == a.dorong(1)); wok(a == a)
    let daftar = [pasang(1, 2.5), pasang(3, 4.5)]
    wok(daftar[1].kanan); wok(len(daftar)); wok(daftar)
    let pl = Pilih { f: |s: string| len(s), bawaan: Some("xy") }
    wok(pl.nilai("halo")); wok(pl.bawaan)
    wok(pertama([7, 8])); wok(pertama(["a"]))
    let dalam = pasang(pasang(1, 2), [pasang("a", true)])
    wok(dalam.kiri.kanan); wok(dalam.kanan[0].kiri)
}
"#),
    ("std/heap bertipe: Heap<T, K> lewat tiga mesin", r#"package main
import "std/heap"
pub fn main() { let h = heap::from([5,3,8,1,9,2]); wok(h.len()); wok(h.peek()); wok(h.to_sorted_array()); let r = h.pop(); wok(r[1]); wok(r[0].to_sorted_array()); wok(heap::from_by(["ccc","a","bb"], |s| len(s)).to_sorted_array())
  var k: heap::Heap<string, string> = heap::new()
  k = k.push("b").push("a")
  wok(k); wok(k.pop()[1]); wok(typeof(k))
  let kosong: heap::Heap<int, int> = heap::new()
  wok(kosong.pop()[1]); wok(kosong.peek()) }
"#),
    ("std/mesin/tween: closure dan fungsi bernama ke field fn(float) -> float", r#"package main
import "std/mesin/tween"
pub fn main() {
    var t = tween::baru(0, 10, 2, tween::masuk_kuad)
    t = t.maju(1); wok(t.nilai())
    let u = tween::baru(1.5, 3, 4.0, tween::pantul).maju(3)
    wok(u.nilai()); wok(tween::antara(0, 8, 0.25, tween::keluar_kubik)); wok(tween::linear(2)); wok(tween::sinus(0.5))
}
"#),
    ("math::sign: NaN tetap NaN, tanda mengikuti jenis argumennya", r#"package main
pub fn main() { wok(math::sign(math::NAN), math::sign(-2.5), math::sign(0.0), math::sign(-3), math::sign(7), math::sign(0)) }
"#),
    // Fase SERPIH-3: tiga bentuk yang dulu ditolak atau mematikan program di LLVM.
    ("bilangan bulat beda lebar dan tanda: dibandingkan dan dihitung menurut nilainya", r#"package main
pub fn main() {
    let x: i32 = 5
    let y: i64 = 5
    wok(x == y, x < y, x + y, typeof(x + y))
    let a: u8 = 200
    let b: i8 = -100
    wok(a + b, a - b, a * b, a > b, a == b)
    let c: u64 = 18446744073709551615
    let d: i64 = -1
    wok(c == d, c > d, d < c, d != c)
    let e: i16 = 300
    let f: i64 = 9000000000
    wok(e * f, f / e, f % e)
}
"#),
    ("defer di badan closure: jalan saat closure pulang, lewat return maupun ekor", r#"package main
fn pakai(f: fn(int) -> int) -> int { return f(3) }
pub fn main() {
    var jml = 0
    let g = |a: int| -> int {
        defer wok("keluar", a)
        if a > 2 { return a * 10 }
        jml += a
        a
    }
    wok(g(1), g(5), jml)
    wok(pakai(|x| { defer wok("dalam"); x + 1 }))
}
"#),
    ("goroutine yang gagal berhenti sendirian: defer-nya jalan, main lanjut", r#"package main
fn gagal() {
    defer wok("defer goroutine")
    let a = [1]
    wok(a[5])
}
pub fn main() {
    spawn { gagal() }
    time::sleep(0.05)
    wok("main lanjut")
}
"#),
    // Fase SAPU-29: temuan dua sapuan builtin (contoh benar, dan argumen angka berganti jenis).
    ("env::set tidak menjawab apa-apa; handle jawa:: tampil dengan kind-nya", r#"package main
pub fn main() {
    wok(typeof(env::set("JRG_SAPU29", 1)), env::get("JRG_SAPU29"))
    let p = jawa::alloc(5)
    let a = jawa::array(2)
    let b = jawa::buffer(2)
    wok(p, a, b)
    wok(p.id, a.kind, typeof(b))
    jawa::array_set(a, 0, 9)
    jawa::array_push(a, 4)
    wok(jawa::array_get(a, 0), jawa::array_get(a, 2), jawa::array_len(a))
    wok(math::degrees(180), math::radians(3.5), math::is_finite(1), math::log(8, 2))
}
"#),
    // Fase PINJAM-LLVM: argumen ke parameter yang cuma dibaca tidak disalin lagi, jadi yang
    // DIKEMBALIKAN dari pinjaman (self, parameter, elemen `for`) harus nilai sendiri.
    ("tugas yang gagal dan tidak pernah diambil hasilnya tidak menghentikan program", r#"package main
fn bagi(a: int, b: int) -> int { return a / b }
pub fn main() {
    let f = Future::new(bagi, 6, 0)
    let g = Task::spawn(bagi, 6, 2)
    wok("jalan terus")
    wok(task::join(g))
    time::sleep(0.02)
    wok(future::is_done(f))
}
"#),
    ("`Self` di tanda tangan dan badan method adalah tipe penerimanya", r#"package main
struct T { pub x: int }
impl T { fn baru(x: int) -> Self { return Self { x: x } }
 fn tambah(self, n: int) -> Self { return Self { x: self.x + n } }
 fn sama(self, o: Self) -> bool { return self.x == o.x }
 fn daftar(self) -> []Self { return [self, Self { x: 0 }] } }
struct A { pub x: int }
struct B: A { pub y: int }
impl A { fn buat(self) { return Self { x: self.x + 1 } } }
struct K<T> { pub v: T }
impl K<T> { fn lagi(self) -> Self { return Self { v: self.v } } }
pub fn main() { let t = T::baru(3)
 wok(t.tambah(4).x, t.sama(T::baru(3)), t.daftar())
 let a = A { x: 1 }
 wok(a.buat(), typeof(a.buat()))
 let k = K { v: 2 }
 wok(k.lagi())
 }
"#),
    ("Option::ok, Result::ok dan Result::err menjawab Option; isinya nilai sendiri", r#"package main
pub fn main() { let o: Option<int> = Some(3)
 let n: Option<int> = None
 let r: Result<int, string> = Ok(4)
 let e: Result<int, string> = Err("x")
 wok(Option::ok(o), Option::ok(n), o.ok(), n.ok())
 wok(r.ok(), r.err(), e.ok(), e.err(), typeof(e.err()))
 wok(e.err().unwrap() + "y", r.ok().unwrap() + 1)
 wok(Result::ok(r), Result::err(e))
 let ra: Result<[]int, string> = Ok([1, 2])
 var x = ra.ok().unwrap()
 x[0] = 9
 wok(x, ra) }
"#),
    ("predikat HOF memakai truthiness; enumerate/zip/list telanjang; str::sub satu argumen", r#"package main
pub fn main() { wok(str::sub("a,b"), str::slice("a,b"), str::substring("a,b"))
 wok(list("ab"), list([1, 2]), typeof(list("ab")))
 wok(arr::count([0, 1, 2], |x| x), arr::count_if([0, 1, 2], |x| x), arr::find([0, 1, 2], |x| x), arr::find_idx([0, 3], |x| x), arr::partition([0, 1, 2], |x| x))
 wok(arr::filter([0, 1, 2, 0, 3], |x| x), arr::filter(["", "a"], |s| s), [0.0, 1.5].filter(|x| x))
 wok(arr::any(["", "a"], |s| s), arr::all(["", "a"], |s| s), [1, 2].any(|x| x - 1), [[], [1]].filter(|a| a))
 wok(enumerate([10, 20]), zip([1, 2], ["a", "b"]))
 wok(arr::count([1, 2, 3], |x| x > 1), arr::partition([1, 2, 3], |x| x % 2 == 0)) }
"#),
    ("jawa::array_pop, array_to_list, buffer_fill, buffer_to_hex, sizeof", r#"package main
pub fn main() { let a = jawa::array(2)
 jawa::array_set(a, 0, 5)
 jawa::array_set(a, 1, 6)
 jawa::array_push(a, 7)
 wok(jawa::array_to_list(a), typeof(jawa::array_to_list(a)))
 let p = jawa::array_pop(a)
 wok(p, typeof(p), jawa::array_len(a))
 wok(jawa::array_pop(a), jawa::array_pop(a))
 let b = jawa::buffer(3)
 let r = jawa::buffer_fill(b, 171)
 wok(r, typeof(r), jawa::buffer_to_hex(b), jawa::buffer_get(b, 2))
 wok(jawa::sizeof(a), jawa::sizeof(b), jawa::sizeof(jawa::alloc(1)), typeof(jawa::sizeof(b)))
 }
"#),
    ("json::set dan json::merge menjawab objek baru", r#"package main
pub fn main() { let o = {"a": 1, "b": 2}
 let p = json::set(o, "c", 3)
 wok(p, o, json::set(o, "a", 9))
 wok(json::merge(o, {"b": 7, "z": 1}), json::merge({"a": 1}, {"a": 1}))
 let j = json::parse("{\"k\": [1, 2], \"s\": \"x\"}")
 wok(json::set(j, "n", 5))
 wok(json::stringify(json::merge(j, json::parse("{\"s\": 2}"))))
 }
"#),
    ("pinjaman: yang dikembalikan dari self atau parameter yang cuma dibaca adalah nilai sendiri", r#"package main
struct Q { pub xs: []int, pub ys: [][]int }
struct K { pub xs: []int }
impl Q {
    pub fn a(self) -> Option<[]int> { return Some(self.xs) }
    pub fn b(self) -> ([]int, int) { return (self.xs, 1) }
    pub fn c(self, k: bool) -> []int { return k $ self.xs : self.xs }
    pub fn d(self) -> []int { self.xs }
    pub fn e(self) -> [][]int { return [self.xs] }
    pub fn g(self) -> []int { return match 1 { _ => self.xs } }
    pub fn h(self) -> int { for i, y in self.ys { y[0] = 5 }
        return 0 }
    pub fn k(self) -> fn() -> int { return || self.xs[0] }
    pub fn m(self) -> ([]int, []int) { return self.xs, self.ys[0] }
}
fn pa(p: []int) -> Option<[]int> { return Some(p) }
fn pb(p: []int) -> K { return K { xs: p } }
fn pc(p: [][]int) -> []int { return p[0] }
fn pd(p: []int) -> []int { p }
fn pe(p: [][]int, i: int) -> []int { if i > 0 { return p[1] }
    return arr::first(p).unwrap() }
fn pf(p: [][]int) -> []int { for _, y in p { return y }
    return [] }
fn pg(p: []int) -> int { return p[0] + len(p) }
fn ph(k: K) -> []int { return k.xs }
fn baca(p: [][]int) -> int { return pg(p[1]) + pg(pc(p)) }
pub fn main() {
    var q = Q { xs: [1, 2], ys: [[1]] }
    var a = q.a().unwrap()
    a[0] = 9
    var b = q.b()
    b.0[0] = 9
    var c = q.c(true)
    c[0] = 9
    var d = q.d()
    d[0] = 9
    var e = q.e()
    e[0][0] = 9
    var g = q.g()
    g[0] = 9
    let _ = q.h()
    let k = q.k()
    q.xs[0] = 40
    var m = q.m()
    m.0[1] = 9
    m.1[0] = 9
    wok(q.xs, q.ys, k())
    var x: []int = [1, 2]
    var xx: [][]int = [[1, 2], [3]]
    var ra = pa(x).unwrap()
    ra[0] = 9
    var rb = pb(x)
    rb.xs[0] = 9
    var rc = pc(xx)
    rc[0] = 9
    var rd = pd(x)
    rd[0] = 9
    var re = pe(xx, 0)
    re[0] = 9
    var re2 = pe(xx, 1)
    re2[0] = 9
    var rf = pf(xx)
    rf[0] = 9
    var kk = K { xs: [7, 8] }
    var rh = ph(kk)
    rh[0] = 9
    wok(x, xx, kk.xs, pg(x), baca(xx))
}
"#),
    ("elemen yang diambil bawaan dan elemen for adalah salinan; tumbuh lewat tangkapan closure", r#"package main
struct K { pub xs: []int }
pub fn main() {
    var ys: [][]int = [[1], [2]]
    var a = ys.first().unwrap()
    a[0] = 10
    var c = arr::get(ys, 1).unwrap()
    c[0] = 12
    var d = arr::last(ys).unwrap()
    d[0] = 13
    var e = arr::find(ys, |v: []int| true).unwrap()
    e[0] = 14
    var g = arr::get_or(ys, 0, [0])
    g[0] = 16
    var dflt: []int = [0]
    var g2 = arr::get_or(ys, 9, dflt)
    g2[0] = 16
    var o = Some(ys[0])
    var u = o.unwrap()
    u[0] = 17
    var u2 = o.unwrap_or([0])
    u2[0] = 18
    var kosong: Option<[]int> = None
    var u3 = kosong.unwrap_or(dflt)
    u3[0] = 18
    var dd: dict<[]int> = dict::new()
    dd["a"] = [1]
    var v = dict::get(dd, "a").unwrap()
    v[0] = 19
    for i, y in ys { y[0] = 5 }
    for _, y in ys { var z = y
        z[0] = 6 }
    var jumlah = 0
    for _, y in ys { jumlah += y[0] }
    wok(ys, o, dd, dflt, jumlah)
    var a2: []int = []
    let tambah = |x: int| { a2 = a2 + [x] }
    for i in 0..5 { tambah(i) }
    a2 += [9]
    tambah(7)
    let beku = move || len(a2)
    let awal = move || a2[0]
    let tulis = move |v: int| { a2[0] = v
        return a2[0] }
    a2 = a2 + [1]
    a2[0] = 44
    wok(a2, beku(), awal(), tulis(3), a2[0])
}
"#),
    ("arr::swap: salinan dengan dua elemen ditukar", r#"package main
pub fn main() { let a = [1, 2, 3]
  wok(arr::swap(a, 0, 2)); wok(arr::swap(a, 1, 1)); wok(a); wok(arr::swap(["x", "y"], 1, 0)) }
"#),
    // Fase PETA: dict LLVM jadi tabel hash (primitif C). Yang dikunci: kepemilikan (salinan,
    // parameter, elemen array, iterasi sambil menyisip), urutan tampil, dan 60 ribu kunci acak —
    // ukuran yang dulu menghabiskan memori.
    ("dict tabel hash: alias, urutan, hapus, gabung, banding, 60 ribu kunci", r#"package main
fn isi(d, n) { for i in 0..n { d[str(i * 3 % 17)] = i }
  return d }
fn tambah(d) { d["zz"] = 1
  return len(d) }
pub fn main() {
    var d = {"m": 0}
    for i in 0..30 { d[str((i * 7) % 31)] = i }
    wok(len(d)); wok(d["21"]); wok(dict::has(d, "30")); wok(dict::has(d, "31")); wok(dict::get(d, "7")); wok(dict::get(d, "x"))
    wok(dict::keys(d))
    d["0"] = 100; d["a"] = 5
    wok(d)
    var e = d
    e["baru"] = 1
    for i in 0..40 { e[str(i + 100)] = i }
    wok(len(d)); wok(len(e)); wok(dict::has(d, "baru")); wok(dict::has(e, "139")); wok(dict::has(d, "139"))
    wok(tambah(d)); wok(len(d)); wok(dict::has(d, "zz"))
    let f = isi(d, 20)
    wok(len(f)); wok(len(d)); wok(f["16"]); wok(dict::has(d, "16"))
    var n = 0
    for k, v in d { d[k + "!"] = v
      n = n + 1 }
    wok(n); wok(len(d))
    for k in d { if len(k) > 2 { d = dict::remove(d, k) } }
    wok(dict::keys(d))
    var g = dict::merge(d, {"q": 9, "0": 1})
    g["k"] = 2
    wok(g == d); wok(len(g)); wok(dict::values(g)[0])
    var h = {"x": 1}
    var i2 = {"x": 1}
    h["b"] = 2; h["a"] = 3
    i2["a"] = 3; i2["b"] = 2
    wok(h == i2); wok(h); wok(i2)
    let xs = [h, i2]
    var y = xs[1]
    y["c"] = 0
    wok(xs); wok(y)
    var kosong = dict::remove({"a": 1}, "a")
    kosong["p"] = 1
    wok(kosong)
    var besar = {"s": 0}
    for i in 0..60000 { besar[str(i * 7919 % 100003)] = i }
    var s = 0
    for i in 0..60000 { s = s + besar[str(i * 7919 % 100003)] }
    wok(len(besar)); wok(s); wok(dict::keys(besar)[1]); wok(besar[dict::keys(besar)[4000]])
    besar["zzz"] = 1
    wok(dict::has(besar, "zzz")); wok(dict::has(besar, "99999x")); wok(besar[str(59999 * 7919 % 100003)])
}
"#),
    ("dict: alias lewat fungsi, closure, iterasi, salinan", r#"package main

fn ubah(d) { d["a"] = 99
  d["z"] = 5
  return d["a"] }
fn buat() { var d = {"a": 1}
  return d }
pub fn main() {
    var d = {"a": 1, "b": 2}
    wok(ubah(d)); wok(d)
    var e = d
    e["a"] = 7; e["c"] = 3
    wok(d); wok(e)
    let xs = [d, d]
    var y = xs[0]
    y["b"] = 50
    wok(xs); wok(y)
    var f = buat()
    f["q"] = 1
    wok(f); wok(buat())
    let g = || { d["a"] = 1000 }
    g()
    wok(d)
    for k, v in d { d[k] = v + 1 }
    wok(d)
    var h = dict::remove(d, "a")
    h["a"] = 0
    wok(d); wok(h); wok(dict::merge(d, h)); wok(dict::keys(h)); wok(dict::values(h)); wok(d == e); wok(dict::has(h, "b")); wok(dict::get(h, "zz"))
}
"#),
    ("dict: anotasi `dict`/`dict<V>`, kunci bukan teks, dict::set", r#"package main
pub fn main() { var d: dict<string, int> = {"a": 1}
 var e: dict<int> = {"b": 2}
 var f: dict = {"c": 3}
 wok(d, e, f)
 var g = {1: "x", 20: "y"}
 wok(g); wok(g[1]); wok(g["20"]); wok(dict::has(g, 1)); g[2.5] = "p"; g[true] = "q"
 let r = 'k'
 g[r] = "rune"
 wok(g); wok(dict::get(g, 2.5)); g = dict::set(g, 7, "tujuh"); wok(g); wok(dict::keys(g)) }
"#),
    ("dict telanjang di field struct, dict::set di tempat, dict::get_or", r#"package main
struct Set { pub d: dict }
fn dari(a) { var d = dict::new()
  for _, x in a { d = dict::set(d, x, x) }
  return Set { d: d } }
fn hitung(teks) { var c = dict::new()
  for _, k in str::split(teks, " ") { c = dict::set(c, k, dict::get_or(c, k, 0) + 1) }
  return c }
pub fn main() {
    let s = dari([3, 1, 2, 3, 1])
    wok(dict::len(s.d)); wok(dict::values(s.d)); wok(dict::has(s.d, 2)); wok(dict::has(s.d, 9))
    let t = dari(["b", "a"])
    wok(dict::keys(t.d)); wok(t.d)
    var d = {"a": 1}
    let e = dict::set(d, "b", 2)
    wok(d); wok(e)
    wok(hitung("a b a c a b"))
    var besar = dict::new()
    for i in 0..3000 { besar = dict::set(besar, i * 7 % 3001, i) }
    wok(len(besar)); wok(besar[7])
}
"#),
    ("argumen method berparameter tanpa anotasi dievaluasi SEKALI", r#"package main
struct P { pub n: int }
impl P { pub fn m(self, x) { return x + self.n } }
fn efek() { wok("efek")
 return 1 }
pub fn main() { let p = P { n: 1 }
 wok(p.m(efek())) }
"#),
    ("arr::sort atas elemen bertag: angka campur, teks, stabil", r#"package main
struct K { pub isi: array }
pub fn main() {
    wok(arr::sort(K { isi: [3, 1.5, 2, 2.0, 1] }.isi)); wok(arr::sort(K { isi: ["b", "a", "c"] }.isi))
    wok(arr::sort(K { isi: [true, false] }.isi)); let e: []int = []
    wok(arr::sort(K { isi: e }.isi))
}
"#),
    // Fase BUNGKUS: `array<T>` yang masuk field `array` telanjang dikotak per elemen, dan
    // nilai bertag yang menuju variabel bertipe dibuka dengan pemeriksaan tag.
    ("array bertipe masuk field `array` telanjang, lalu dibaca kembali", r#"package main
struct Kotak { pub isi: array }
fn jumlah(k) { var s = 0
  for _, x in k.isi { s = s + x }
  return s }
pub fn main() {
    let a = [1, 2, 3]
    let k = Kotak { isi: a }
    wok(k.isi); wok(len(k.isi)); wok(k.isi[1]); wok(jumlah(k))
    let t = Kotak { isi: ["a", "b"] }
    wok(t.isi); wok(typeof(t.isi[0])); wok(typeof(k.isi[0]))
    var b = [4.5]
    b = arr::push(b, 2.5)
    let f = Kotak { isi: b }
    wok(f.isi); wok(a)
    var s = 0
    s = k.isi[2]
    wok(s + 1)
    var u = 7
    u = t.isi[0]
    wok(u)
    let e: []int = []
    wok(Kotak { isi: e }.isi)
}
"#),
    // Fase SEPARUH: `Ok`/`Err`/`None` yang separuh tipenya datang dari `return` LAIN di fungsi
    // tanpa anotasi, termasuk di dalam tuple.
    ("separuh tipe: Ok dan Err, Some dan None, tuple berisi None", r#"package main
fn bagi(a, b) {
    if b == 0 { return Err("bagi nol") }
    return Ok(a / b)
}
fn cari(xs, x) {
    if len(xs) == 0 { return None }
    for i, v in xs { if v == x { return Some(i) } }
    return None
}
fn pasang(n) {
    if n < 0 { return (n, Option::None) }
    return (n, Some("ada"))
}
fn ekst(s) {
    match str::index_of(s, ".") { None => return Option::None, Some(i) => if i < 1 { return Option::None }, }
    return Option::Some(str::sub(s, 1, len(s)))
}
pub fn main() {
    wok(bagi(6, 3)); wok(bagi(1, 0))
    match bagi(9, 3) { Ok(v) => wok(v), Err(e) => wok(e) }
    wok(cari([4, 5, 6], 6)); wok(cari([1], 7)); wok(cari([], 1))
    wok(pasang(-1)); wok(pasang(2)); wok(ekst("a.b")); wok(ekst("ab"))
}
"#),
    ("arr:: bentuk namespace: pop, first, last, get, get_or, take, skip", r#"package main
fn pecah(daftar, n) {
    var keluar = []
    var bagian = []
    for i in daftar {
        bagian = arr::push(bagian, daftar[i])
        if bagian.len() == n { keluar = arr::push(keluar, bagian)
            bagian = [] }
    }
    return keluar
}
fn kiri(ps) { var k = []
    for i in ps { let p = ps[i]
        k = arr::push(k, p.0) }
    return k }
pub fn main() {
    let a = [3, 4, 5]
    wok(arr::pop(a)); wok(arr::first(a)); wok(arr::last(a)); wok(arr::get(a, 1)); wok(arr::get(a, 9)); wok(arr::get_or(a, 9, 7))
    let e: []int = []
    wok(arr::pop(e)); wok(arr::last(e))
    wok(arr::take(a, 2)); wok(arr::take(a, 9)); wok(arr::take(a, 0)); wok(arr::skip(a, 1)); wok(arr::skip(a, 9))
    wok(pecah([1, 2, 3, 4], 2)); wok(kiri([(1, "a"), (2, "b")]))
}
"#),
    ("str::join, str::sub, dan `for j, c in teks`", r#"package main
fn potong(p) { var k = []
    for _, b in str::split(p, "/") { if len(b) > 0 { k = arr::push(k, b) } }
    return k }
pub fn main() {
    let n = "abé.cd"
    wok(str::join(["a", "b"], "-")); wok(str::join([], "-")); wok(["x", "y"].join("+")); wok(str::join(potong("/usr//lib/"), "/"))
    wok(str::sub(n, 1, 3)); wok(n.sub(4, len(n))); wok(str::sub(n, 2, 2)); wok(str::substring(n, 0, 2))
    for j, c in n { wok_(j); wok_(" "); wok_(c); wok_(" "); wok(c == '.') }
}
"#),
    // Fase SIMPUL-1: closure tanpa anotasi yang diserahkan ke fungsi tanpa anotasi — tipe
    // parameternya dibaca dari cara BADAN fungsi itu memakainya, termasuk di modul `std/`.
    ("closure tanpa anotasi ke fungsi pemakai: map, filter, lipat, panggil dua kali", r#"package main
fn petakan(daftar, f) { var keluar = []
  for _, x in daftar { keluar = arr::push(keluar, f(x)) }
  return keluar }
fn saring(daftar, pred) { var keluar = []
  for _, x in daftar { if pred(x) { keluar = arr::push(keluar, x) } }
  return keluar }
fn lipat(daftar, awal, f) { var a = awal
  for _, x in daftar { a = f(a, x) }
  return a }
fn dua_kali(f, x) { return f(f(x)) }
pub fn main() {
  let k = 10
  wok(petakan([1, 2, 3], |x| x * 2 + k))
  wok(petakan(["a", "b"], |s| s + "!"))
  wok(saring([1, 2, 3, 4], |x| x % 2 == 0))
  wok(lipat([1, 2, 3], 0, |a, x| a + x))
  wok(dua_kali(|x| x + 3, 4))
}"#),
    // Fase TUMBUH-LLVM: pertumbuhan di tempat tidak boleh membocorkan alias ke nama lain,
    // ke pemanggil, ke global yang sudah disalin, atau antar-putaran loop.
    ("tumbuh di tempat: salinan, parameter, global, loop, closure, a + a", r#"package main
var G = []
fn tumbuh(d, x) { d = d + [x]
  d = arr::push(d, x * 2)
  return d }
fn isi_global(n) { for i in 0..n { G = arr::push(G, i) } }
fn bikin(n) { var k = []
  for i in 0..n { k = arr::push(k, i * i) }
  return k }
pub fn main() {
  var a = [1, 2]
  var b = a
  a = a + [3]
  a += [4, 5]
  b = arr::push(b, 99)
  wok(a, b)
  a = a + a
  wok(a, len(a))
  let c = tumbuh(a, 7)
  wok(len(a), len(c), c[len(c) - 1])
  var d = bikin(5)
  var e = d
  d = arr::push(d, -1)
  e[0] = 42
  wok(d, e)
  for r in 0..3 { var k = []
    for i in 0..r + 2 { k = arr::push(k, r * 10 + i) }
    k = arr::concat(k, [r])
    wok(k) }
  isi_global(4)
  let h = G
  isi_global(2)
  wok(G, h)
  var t = ["a"]
  for i in 0..3 { t = arr::push(t, "s" + str(i)) }
  let tambah = |x| { a = arr::push(a, x) }
  tambah(1000)
  wok(t, a[len(a) - 1], len(a))
  var kosong = []
  kosong = kosong + []
  kosong = arr::push(kosong, true)
  wok(kosong)
}"#),
    // Fase SIMPUL-3: nama yang diikat pola `match` punya tipe di penyimpul (`Some(v) => v`), dan
    // field `array` tanpa tipe elemen disimpan bertag — satu tata letak untuk isi apa pun.
    ("pola match berpengikat di penyimpul, iterator::filter_map, field array telanjang", r#"package main
import "std/iterator"
struct Kotak { pub isi: array, pub n: int = 0 }
impl Kotak {
  fn tambah(self, x) { return Kotak { isi: arr::push(self.isi, x), n: self.n + 1 } }
  fn panjang(self) { return len(self.isi) }
}
fn pilih(d, f) { var k = []
  for _, x in d { match f(x) { Some(v) => { k = arr::push(k, v) }
    None => {} } }
  return k }
fn atau_nol(d, f) { var k = []
  for _, x in d { let r = f(x)
    k = arr::push(k, r ?? 0) }
  return k }
pub fn main() {
  let a = [3, 1, 2]
  wok(pilih(a, |x| x > 1 $ Some(x * 10) : None), atau_nol(a, |x| x > 1 $ Some(x) : None))
  wok(iterator::filter_map(a, |x| x > 1 $ Option::Some(x * 10) : Option::None))
  var k = Kotak { isi: [] }
  k = k.tambah(1)
  k = k.tambah(2)
  wok(k.panjang(), k.n, k.isi, k.isi[0], k)
}"#),
    // Fase SIMPUL-2: fungsi yang mengembalikan `Some`/`None` tanpa anotasi (tata letaknya dibawa
    // pulang dari salinan penduga), `for … in range`, dan parameter-fungsi yang DITERUSKAN.
    ("closure tanpa anotasi: Option dari penduga, range, penerusan, std/iterator fold/find", r#"package main
import "std/iterator"
fn cari(daftar, pred) { for i in daftar { if pred(daftar[i]) { return Some(daftar[i]) } }
  return None }
fn isi(n, f) { var keluar = []
  for _, i in range(0, n) { keluar = arr::push(keluar, f(i)) }
  return keluar }
fn lipat(d, f, awal) { var a = awal
  for _, x in d { a = f(a, x) }
  return a }
fn lipat2(d, f, awal) { return lipat(d, f, awal) }
pub fn main() {
  let a = [3, 1, 2]
  wok(cari(a, |x| x < 3), cari(a, |x| x > 9))
  wok(isi(4, |i| i * i))
  wok(lipat2(a, |s, x| s + x, 0), iterator::fold(a, |s, x| s * x, 1), iterator::reduce(a, |s, x| s + x, 0))
  wok(iterator::find(a, |x| x > 1), iterator::any(a, |x| x == 2), iterator::all(a, |x| x > 0))
}"#),
    ("std/iterator lewat LLVM: map, filter, each dengan closure tanpa anotasi", r#"package main
import "std/iterator"
pub fn main() {
    wok(iterator::map([1, 2], |x| x * 2))
    wok(iterator::filter([1, 2], |x| x > 1))
    wok(typeof(iterator::each([1], |x| x)))
}"#),
    ("panik: pesan dirakit, keluaran sebelumnya utuh — lihat GAGAL_SAAT_JALAN", r#"package main
fn aman(x) { if x < 0 { panik("negatif: " + str::to_string(x)) }
  return x * 2 }
pub fn main() { wok(aman(4)) }"#),
    // Fase LEBAR-LLVM: tepat di batas masih jalan, dan yang sengaja membungkus ditulis.
    ("int 64 bit: tepi yang muat, dan math::wrap_*", r#"package main
pub fn main() {
    var x = 9223372036854775806
    x += 1
    var m = -9223372036854775807
    m = m - 1
    wok(x, m, -x, math::abs(-x), [9223372036854775806, 1].sum())
    var h: uint = 1469598103934665603
    for i in 0..5 { h = math::wrap_mul(h ^ (i as uint), 1099511628211) }
    wok(h, math::wrap_sub(0, 1), math::wrap_add(18446744073709551615, 2))
}"#),
    // Fase DUGA: tipe kembalian fungsi tanpa anotasi dibaca dari penurunan badannya.
    ("kembalian tanpa anotasi: match berpola pengikat, ??, fungsi pustaka", r#"package main
enum E { A(int), B }
fn g(e) { return match e { E::A(n) => n, E::B => -1 } }
fn m(o: Option<int>) { return o ?? 5 }
fn p(x: int) { return math::abs(x) }
pub fn main() { wok(g(E::A(3)), g(E::B), m(Option::Some(1)), m(Option::None), p(-5), typeof(p(-5))) }"#),
    ("kembalian tanpa anotasi: fn bersarang, method, default, tuple, rekursi", r#"package main
var G = 7
struct K { pub v: int = 0 }
impl K {
    fn ambil(self) { return self.v }
    fn pasangan(self) { return (self.v, "k") }
}
fn h(a, b = a + 1) { return b }
fn k(a = G) { return a }
fn luar() { let r = kedua(1); fn kedua(x) { return x + 100 }; return r }
fn fakt(n) { if n < 2 { return 1 }
    return n * fakt(n - 1) }
pub fn main() {
    wok(K { v: 5 }.ambil(), K { v: 2 }.pasangan(), h(3), k(), k(99), luar(), fakt(10))
}"#),
    // Fase PACU-3: literal diperiksa lebarnya, dan `-2^63` tetap sah sebagai operand `-`.
    ("literal tepi 64 bit", "package main\npub fn main() { let x: u64 = 18446744073709551615\n let y: i8 = -128\n wok(-9223372036854775808, 9223372036854775807, x, y) }"),
    // Fase LLVM-17: `self` dulu penunjuk ke milik pemanggil, `wok` mencetak di tengah evaluasi
    // argumennya, tiap nilai fungsi `function`, dan geseran >= lebar adalah poison.
    ("penerima disalin, urutan cetak, typeof closure, geseran", r#"package main
struct P { pub n: int, pub xs: []int }
impl P {
    fn naik(self) -> int { self.n += 1; return self.n }
    fn dua(self) -> int { return self.naik() + self.n }
    fn isi(self) -> P { self.xs[0] = 7; self.n = 3; return self }
    fn baca(self) -> int { return self.n + self.xs[0] }
    fn lewat_closure(self) -> int { let f = || { self.n = 50; return self.n }; return f() + self.n }
}
fn tambah(a: int, b: int) -> int { return a + b }
fn f(n: int) -> int { wok_("[", n, "]"); return n }
pub fn main() {
    var p = P { n: 1, xs: [1, 2] }
    wok(p.naik(), p.n, p.dua(), p.n)
    let q = p.isi()
    wok(p, q, p.baca(), q.baca())
    wok(p.lewat_closure(), p.n)
    wok(f(1), f(2))
    let g = |x: int| x + 1
    let k = 5
    let h = |x: int| x + k
    let t = tambah
    wok(typeof(g), typeof(h), typeof(tambah), typeof(t), g(1), h(1), t(1, 2))
    let n = 70
    let z = 0
    wok(-8 >> n, 8 >> n, z << n, 1 << 3, 256 >> 4, -256 >> 4, 5 << 0)
}"#),
    ("method default trait, hook tampilan darinya, dan teks + agregat", r#"package main
struct B { pub n: int }
impl Bentuk for B { fn luas(self) -> int { return self.n * self.n } }
trait Bentuk {
    fn luas(self) -> int
    fn sapa(self) -> string = "dari trait"
    fn to_string(self) -> string { return "Bentuk luas " + str(self.luas()) }
    fn dobel(self) -> int { return self.luas() * 2 }
}
struct A { pub n: int }
impl Bentuk for A {
    fn luas(self) -> int { return self.n }
    fn sapa(self) -> string { return "milik A" }
}
struct P { pub a: int }
enum E { X(int), Y }
struct V { pub variant: string }
pub fn main() {
    let a = A { n: 3 }
    let b = B { n: 4 }
    wok(a.sapa(), b.sapa(), a.dobel(), b.dobel())
    wok(a, b, [a], "x" + b, a.to_string())
    let p = P { a: 1 }
    wok("x" + p, "x" + [1, 2], "x" + (1, "a"), "x" + E::X(2), "x" + E::Y, [1] + "y", p + "z")
    let v = V { variant: "None" }
    wok(match v { None => "none", _ => "lain" }, match p { Y => "y", _ => "lain" })
}"#),
    // Fase LLVM-16: tanpa anotasi, kembalian ternary/`if … else`/`match` dulu ditebak `()`
    // dan nilainya dibuang tanpa kata — `wok(f(0), f(1))` mencetak `() ()`.
    ("kembalian ternary, if, dan match tanpa anotasi", r#"package main
fn f(n) { return n == 0 $ true : false }
fn g(e) { return match e { 1 => 10, _ => 20 } }
fn h(n) { if n > 0 { "pos" } else { "neg" } }
fn k(n) { return if n > 0 { 1.5 } else { 2.5 } }
pub fn main() {
    wok(f(0), f(1), g(1), g(2), h(1), h(-1), k(1))
}"#),
    // `math::pow` selalu pecahan, dan min/max memakai rumus acuan `a < b $ a : b`.
    ("math: pow selalu pecahan, min dan max seperti acuan", r#"package main
pub fn main() {
    wok(math::pow(2, 10), pow(2, 3), math::pow(2, -1), math::pow(2.0, 0.5))
    wok(math::min(math::NAN, 1.0), math::min(1.0, math::NAN), math::max(math::NAN, 1.0))
    wok(math::min(-0.0, 0.0), math::min(0.0, -0.0), math::max(3, 2), math::min(2.5, 1.5))
}"#),
    // Fase SERPIH: `a, _ = …` dulu ditolak LLVM ("`_` bukan variabel yang bisa dialamati").
    ("pembuang: `_ = f()` dan `a, _ = …` membuang nilainya", r#"package main
fn f() -> int {
    wok("f")
    return 5
}
pub fn main() {
    _ = f()
    var a = 1
    a, _ = 7, f()
    wok(a)
}"#),
    // Fase RUANG: fungsi milik program menang atas bawaan bernama sama, di tiga mesin —
    // dulu LLVM menjawab `abs(-5)` dengan bawaannya dan menolak `len(1, 2, 3)`.
    (
        "fungsi milik program menang atas bawaan bernama sama",
        "package main\nfn len(a, b, c) { return a + b + c }\nfn abs(x) { return 7 }\npub fn main() {\n    wok(len(1, 2, 3), abs(-5), ::len(\"ab\"), ::abs(-5))\n}\n",
    ),
    // Fase TERM: `exit` satu aturan di tiga mesin.
    (
        "exit: kode 0 sesudah keluaran, sisanya tidak jalan",
        "package main\npub fn main() {\n    wok(\"sebelum\")\n    exit(0)\n    wok(\"tidak sampai\")\n}\n",
    ),
    // `()` yang ditulis adalah unit, di tiga mesin (dulu tuple kosong).
    (
        "unit: () yang ditulis sama dengan unit yang dijawab fungsi",
        "package main\nfn f() { }\npub fn main() {\n    wok(typeof(()), f() == (), () == ())\n}\n",
    ),
    // Fase KEMBALIAN: yang tidak menjawab apa-apa menjawab `unit` — LLVM sudah
    // begitu sejak dulu, dua mesin acuan menjawab `nil`.
    (
        "jawa::set dan jawir::free menjawab unit",
        r#"package main
pub fn main() {
    let p = jawa::alloc(1)
    wok(typeof(jawa::set(p, 2)), jawa::get(p))
    let a = jawa::array(2)
    wok(typeof(jawa::array_set(a, 0, 7)), typeof(jawir::free(p)))
    let u = jawa::array_push(a, 1)
    wok(1, u, typeof(u))
}
"#,
    ),
    // Fase KURUNG: penjaganya menyentuh pintu yang dilewati SETIAP pemanggilan
    // method, jadi sisi positifnya dikunci berdampingan.
    (
        "pemanggilan method dan pseudo-field tetap jalan",
        r#"package main
struct P { pub a: int }
impl P { fn dua(self) { return self.a * 2 } }
pub fn main() {
    let p = P { a: 5 }
    wok(p.dua())
    let s = "halo"
    wok(s.len, s.upper())
}
"#,
    ),
    // Fase POLA-FIELD: sisi POSITIF penjaga pola field. Penjaga yang menolak
    // semuanya juga menghijaukan tabel penolakan, jadi batasnya dikunci di sini.
    (
        "pola sebagian field tetap cocok",
        r#"package main
struct P { pub a: int, pub b: int }
pub fn main() {
    let p = P { a: 7, b: 2 }
    match p { P { a } => wok(a), _ => wok("lain") }
    let q = P { a: 8, b: 3 }
    match q { P { a, .. } => wok(a), _ => wok("lain") }
}
"#,
    ),
    // `byte` bilangan 8 bit tak bertanda di ketiga mesin (Fase KONVERSI) —
    // kasus ini dulu satu-satunya isi `DIVERGENSI_LLVM`.
    (
        "byte: bilangan 8 bit, bukan karakter",
        r#"package main
pub fn main() {
    wok(65 as byte)
    wok((65 as byte) == 65)
}"#,
    ),
    // Tipe hasil dari ANOTASI method struct, di fungsi tanpa anotasi dan di
    // closure. Dulu `f` dipancarkan `void`: nilainya dibuang dan `typeof`
    // menjawab `unit` — hasil salah yang diam.
    (
        "method struct: tipe hasil dari anotasinya",
        r#"package main
struct P { pub n: int }
impl P {
    fn dua(self) -> int { return self.n * 2 }
    fn besar(self) -> bool { return self.n > 1 }
}
fn f(t: P) { return t.dua() }
fn g(t: P) { return P::dua(t) }
pub fn main() {
    let xs = [P { n: 1 }, P { n: 2 }]
    wok(f(P { n: 3 }), typeof(f(P { n: 3 })), g(P { n: 4 }))
    wok(xs.map(|t| t.dua()), xs.map(|t: P| t.dua()), xs.filter(|t| t.besar()))
    wok(arr::map(xs, |t| t.dua()), xs.map(|t| P::dua(t)))
}"#,
    ),
    // Nilai FUNGSI di parameter: tipe hasilnya dari tipenya. Dulu `jalankan`
    // ditebak `()` dan `assert_eq(jalankan(..), 3)` di `closures.jrg` LULUS
    // karena `() == 3` dikompilasi jadi `true` — dua cacat saling menutupi.
    (
        "nilai fungsi di parameter: tipe hasil dari tipenya",
        r#"package main
fn jalankan(f) { return f() }
pub fn main() {
    var arg = 1
    wok(jalankan(|| 3), typeof(jalankan(|| 3)))
    assert_eq(jalankan(|| { arg = arg * 3; return arg }), 3)
    wok(arg)
}"#,
    ),
    // `()` lawan nilai lain: `false`, seperti acuan. Dulu literal `6` menyesuaikan
    // diri ke `()` dan `kosong() == 6` jadi konstanta `true`.
    (
        "unit lawan nilai lain: == lintas tipe",
        r#"package main
fn kosong() { }
pub fn main() {
    let u = kosong()
    wok(u == 6, u != 6, kosong() == 6, kosong() == "teks", kosong() != 0)
}"#,
    ),
    // Bentuk path atas method ber-`self`. Di sini penerimanya diketahui saat
    // KOMPILASI, jadi pemeriksaannya gratis dan tepat.
    (
        "path method di llvm",
        r#"package main
struct P { pub n: int }
impl P { pub fn tambah(self, k: int) -> int { return self.n + k } }
pub fn main() {
    let p = P{n: 1}
    wok(P::tambah(p, 4))
    wok(p.tambah(4))
}
"#,
    ),
    (
        "sebar array di llvm",
        r#"package main
pub fn main() {
    let a = [2, 3]
    let b = [1, ...a, 4]
    wok(b)
    wok(len(b))
}
"#,
    ),
    // `"""` — pembatas ASCII untuk template, di backend ketiga.
    (
        "tiga kutip di llvm",
        r#"package main
pub fn main() {
    let n = 7
    wok("""nilai ${n}
baris dua""")
    wok(«lama ${n * 2}»)
    wok("biasa", "")
}"#,
    ),
    // ── `str::format`: aturan template yang sama dengan interpreter ──────
    ("format: dua desimal",       "package main\npub fn main() { wok(str::format(\"{:.2}\", 3.14159)) }"),
    ("format: nol desimal",       "package main\npub fn main() { wok(str::format(\"{:.0}\", 2.7)) }"),
    ("format: int dapat desimal", "package main\npub fn main() { wok(str::format(\"{:.2}\", 5)) }"),
    ("format: campur",           "package main\npub fn main() { wok(str::format(\"{} = {:.3}\", \"pi\", 3.14159265)) }"),
    ("format: {x} tetap teks",   "package main\npub fn main() { wok(str::format(\"{x} {}\", 1)) }"),
    // Closure `move`. Di backend ini tangkapan berupa kotak heap dan `env`
    // menyimpan PENUNJUK ke tiap kotak; `move` menyimpan penunjuk ke kotak
    // BARU berisi salinan. Kedua arah diuji terpisah.
    ("move llvm: berbagi (biasa)",  "package main\npub fn main() { var n = 1\n let f = || n\n n = 9\n wok(f()) }"),
    ("move llvm: snapshot",         "package main\npub fn main() { var n = 1\n let f = move || n\n n = 9\n wok(f()) }"),
    ("move llvm: dalam->luar putus","package main\npub fn main() { var n = 1\n let f = move || { n = 5\n return n }\n wok(f())\n wok(n) }"),
    ("move llvm: mutasi bertahan",  "package main\npub fn main() { var n = 0\n let f = move || { n = n + 1\n return n }\n wok(f())\n wok(f())\n wok(n) }"),
    ("move llvm: dua nama",         "package main\npub fn main() { var a = 1\n var b = 2\n let f = move || a + b\n a = 10\n b = 20\n wok(f()) }"),
    ("move llvm: tanpa tangkapan",  "package main\npub fn main() { let f = move || 7\n wok(f()) }"),

    // Operator yang didefinisikan pengguna.
    ("operator llvm: a + b",   "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} + V{x:2}) }"),
    ("operator llvm: a - b",   "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:5} - V{x:2}) }"),
    ("operator llvm: -a",      "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(-V{x:7}) }"),
    ("operator llvm: a == b",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} == V{x:1}); wok(V{x:1} == V{x:2}) }"),
    ("operator llvm: a != b",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} != V{x:2}) }"),
    ("operator llvm: a < b",   "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} < V{x:2}); wok(V{x:3} < V{x:2}) }"),
    ("operator llvm: a >= b",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:3} >= V{x:3}) }"),
    ("operator llvm: rantai",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} + V{x:2} + V{x:3}) }"),
    ("operator llvm: bawaan tak tersentuh", "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(1 + 2); wok(-5); wok(1 < 2) }"),
    ("mut llvm: var",          "package main\npub fn main() { var x = 1\n x = 2\n wok(x) }"),
    ("mut llvm: let mut",      "package main\npub fn main() { let mut x = 1\n x = 2\n wok(x) }"),

    // Hook tampilan `to_string`.
    ("hook llvm: wok",            "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1}) }"),
    ("hook llvm: wok_",           "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok_(P{n:1}); wok(\"\") }"),
    ("hook llvm: fmt::sprint",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(fmt::sprint(P{n:1})) }"),
    ("hook llvm: dalam array",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok([P{n:1}, P{n:2}]) }"),
    ("hook llvm: dalam tuple",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok((P{n:1}, 7)) }"),
    ("hook llvm: di field",       "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\nstruct Q { pub p: P }\npub fn main() { wok(Q{p: P{n:1}}) }"),
    ("hook llvm: eksplisit",      "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1}.to_string()) }"),
    ("hook llvm: tanpa hook",     "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\nstruct R { pub m: int }\npub fn main() { wok(R{m:2}) }"),
    ("hook llvm: typeof tetap",   "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(typeof(P{n:1})) }"),

    // `let … else` dan parameter pengumpul.
    ("let else dan parameter pengumpul", r#"package main
fn satu(o: Option<int>) -> int {
    let Some(x) = o else { return -1 }
    return x * 10
}
fn banyak(o: Option<(int, int)>) -> int {
    let Some((a, b)) = o else { return -1 }
    return a + b
}
fn jumlah(xs...) -> int {
    var t = 0
    for i in xs { t = t + xs[i] }
    return t
}
pub fn main() {
    wok(satu(Some(3)), satu(None))
    wok(banyak(Some((3, 4))), banyak(None))
    wok(jumlah(1, 2, 3), jumlah())
    wok(match 5 { x => x }, match 9 { n @ _ => n })
}"#),

    // ── pola pengikat, koalesensi, dan varian unit telanjang ────────────
    ("pola pengikat dan koalesensi", r#"package main
enum E { A(int), B }
var n = 0
fn mahal() -> int { n = n + 1; return 9 }
pub fn main() {
    wok(match 5 { m @ 1..=9 => m * 10, _ => 0 })
    wok(match 50 { m @ 1..=9 => m * 10, _ => 0 })
    let o: Option<int> = Some(3)
    wok(match o { None => 0, Some(v) => v })
    let e = E::A(3)
    wok(match e { B => 0, E::A(v) => v })

    let k: Option<int> = None
    let b: Option<int> = None
    let c: Option<int> = Some(7)
    wok(o ?? 9, k ?? 9, k ?? b ?? c ?? 0)

    wok(o ?? mahal(), n)
    wok(o.unwrap_or(mahal()), n)
}"#),

    // ── sintaks yang diturunkan di parser ───────────────────────────────
    ("if let dan while let", r#"package main
pub fn main() {
    let o = Some(3)
    if let Some(x) = o { wok("ada", x) }
    let k: Option<int> = None
    if let Some(x) = k { wok(x) } else { wok("kosong") }
    wok(if let Some(v) = o { v } else { 0 })
    if let [a, b] = [1, 2] { wok(a, b) }

    var n = 0
    while let Some(x) = Some(n) {
        n = n + 1
        if x > 2 { break }
        wok("n", x)
    }
}"#),
    ("penugasan jamak dan for klasik `:=`", r#"package main
pub fn main() {
    var a = 1
    var b = 2
    a, b = b, a
    wok(a, b)
    c, d := 5, 6
    wok(c, d)
    for i := 0; i < 3; i = i + 1 { wok(i) }
}"#),
    ("else boleh memimpin barisnya sendiri", r#"package main
pub fn main() {
    if 1 > 2 { wok("a") }
    else if 1 > 0 { wok("b") }
    else { wok("c") }
}"#),

    // ── closure ─────────────────────────────────────────────────────────
    ("closure: tangkapan bermutasi terlihat dua arah", r#"package main
pub fn main() {
    var x = 1
    let ubah = || { x = 2 }
    wok(x)
    ubah()
    wok(x)
}"#),
    ("closure: dua closure berbagi satu pengikatan", r#"package main
pub fn main() {
    var n = 0
    let naik = || { n = n + 1 }
    let lihat = || n
    naik(); naik(); naik()
    wok(n, lihat())
}"#),
    ("closure: lolos keluar dari bingkai asalnya", r#"package main
fn bikin() -> fn() -> int {
    var n = 0
    return || { n = n + 1; return n }
}
pub fn main() {
    let c = bikin()
    wok(c(), c(), c())
    let d = bikin()
    wok(d())
}"#),
    ("closure: tiap putaran loop mengikat ulang", r#"package main
pub fn main() {
    var fs = []
    for i in 0..3 { fs = fs + [|| i] }
    wok(fs[0](), fs[1](), fs[2]())
}"#),
    ("closure: tipe parameter dibaca dari situs pemanggilan", r#"package main
pub fn main() {
    var p = 1
    let byparam = |p| p
    wok(byparam(5), p)
    let dua = |a, b| a + b
    wok(dua(3, 4))
}"#),
    ("closure: menangkap var tingkat modul", r#"package main
var g = 0
pub fn main() {
    let gn = || { g = g + 1; return g }
    wok(gn(), gn(), g)
}"#),
    // Fungsi BERSARANG bukan closure: ia tidak melihat lokal fungsi luarnya
    // (diukur — acuannya menolak dengan "variabel belum dikenal") dan di-hoist …
    ("fungsi bersarang: hoist per blok dan saling panggil", r#"package main
fn luar() -> int {
    wok(a(2))
    fn a(x: int) -> int { return b(x) + 1 }
    fn b(x: int) -> int { return x * 10 }
    return a(3)
}
pub fn main() {
    wok(luar())
    fn dalam(x: int) -> int { return x * 3 }
    wok(dalam(2))
    if true {
        fn blok(x: int) -> int { return x + 100 }
        wok(blok(1))
    }
}"#),
    ("fungsi bersarang: dimonomorfisasi per tipe argumen", r#"package main
pub fn main() {
    fn ganda(x) { return x + x }
    wok(ganda(3), ganda("ab"))
}"#),

    // ── jawa:: / jawir:: ────────────────────────────────────────────────
    ("jawa: sel tunggal, array, buffer, dan handle salinan", r#"package main
pub fn main() {
    let p = jawa::alloc(42)
    wok(jawa::get(p), jawa::is_valid(p), typeof(p))
    jawa::set(p, 99)
    wok(jawa::get(p))
    jawir::free(p)
    wok(jawa::is_valid(p))

    let a = jawa::array(3)
    jawa::array_set(a, 0, 10)
    let a2 = jawa::array_push(a, 30)
    wok(jawa::array_len(a), jawa::array_get(a, 0), jawa::array_get(a, 3), a2)

    let b = jawa::buffer(4)
    jawa::buffer_set(b, 0, 255)
    wok(jawa::buffer_len(b), jawa::buffer_get(b, 0))

    let asli = jawa::alloc(1)
    let salinan = asli
    jawa::set(salinan, 123)
    wok(jawa::get(asli))
    jawir::free(salinan)
    wok(jawa::is_valid(asli), jawa::is_valid(salinan))
}"#),

    ("fungsi bernama sebagai nilai", r#"package main
fn tambah(a: int, b: int) -> int { return a + b }
fn kali(a: int, b: int) -> int { return a * b }
fn pakai(f: fn(int, int) -> int, x: int, y: int) -> int { return f(x, y) }
pub fn main() {
    let f = tambah
    var g = tambah
    g = kali
    wok(f(2, 3), g(2, 3))
    wok(pakai(tambah, 4, 5), pakai(kali, 4, 5))
}"#),

    // ── defer ───────────────────────────────────────────────────────────
    ("defer: urutan LIFO dan pembekuan argumen", r#"package main
fn urutan() {
    defer wok("A")
    defer wok("B")
    defer wok("C")
    wok("badan")
}
fn beku() {
    var x = 1
    defer wok("beku:", x)
    x = 99
    wok("akhir:", x)
}
pub fn main() { urutan(); beku() }"#),
    ("defer: lingkup fungsi, bukan blok", r#"package main
fn dalam_blok() {
    if true {
        defer wok("dalam if")
        wok("di if")
    }
    wok("sesudah if")
}
fn dalam_loop() {
    for i in 0..3 { defer wok("loop", i) }
    wok("sesudah loop")
}
pub fn main() { dalam_blok(); dalam_loop() }"#),
    ("defer: fungsi yang dipanggil dibekukan saat didaftarkan",
     "package main\nfn a() { wok(\"a\") }\nfn b() { wok(\"b\") }\npub fn main() { var f = a\n defer f()\n f = b\n defer f()\n wok(\"badan\") }"),
    ("defer: tetap jalan lewat return lebih awal", r#"package main
fn dg_return() -> int {
    defer wok("defer jalan")
    return 7
}
fn kondisional(c: bool) {
    if c { defer wok("hanya kalau true") }
    wok("selesai", c)
}
pub fn main() {
    wok(dg_return())
    kondisional(true)
    kondisional(false)
}"#),

    // ── method tingkat-tinggi ───────────────────────────────────────────
    ("hof: map filter reduce find any all", r#"package main
pub fn main() {
    let a = [1, 2, 3]
    wok(a.map(|x| x * 2))
    wok(a.filter(|x| x > 1))
    wok(a.reduce(|acc, x| acc + x, 0))
    wok(a.any(|x| x > 2), a.all(|x| x > 0))
    wok(a.find(|x| x > 1))
    wok(a.find(|x| x > 9))
    wok(a.map(|x| x * 2).filter(|x| x > 2))
    let s = ["a", "bb"]
    wok(s.map(|x| x.len()))
}"#),

    // ── parameter default ───────────────────────────────────────────────
    ("parameter default dievaluasi di bingkai callee", r#"package main
fn a(x: int, y: int = 10) -> int { return x + y }
fn b(x: int, y: int = x * 2) -> int { return x + y }
var hit = 0
fn efek() -> int { hit = hit + 1; return 100 }
fn c(x: int, y: int = efek()) -> int { return x + y }
pub fn main() {
    wok(a(5), a(5, 1))
    wok(b(3), b(3, 1))
    wok(c(1), c(1, 2), c(1))
    wok(hit)
}"#),

    // ── var tingkat modul ───────────────────────────────────────────────
    ("var tingkat modul: urutan inisialisasi dan mutasi", r#"package main
var n = 0
var pesan = "halo"
var xs = [1, 2, 3]
var turunan = n + 5
fn naik() { n = n + 1 }
pub fn main() {
    wok(n, pesan, xs, turunan)
    naik(); naik()
    wok(n)
    pesan = pesan + " dunia"
    xs = xs + [4]
    wok(pesan, xs)
}"#),

    // ── range ───────────────────────────────────────────────────────────
    ("range: satu, dua, dan tiga argumen", r#"package main
pub fn main() {
    wok(range(3))
    wok(range(1, 4))
    wok(range(0, 10, 3))
    wok(range(0))
    wok(typeof(range(3)))
}"#),

    // ── aliasing: penunjuk DI DALAM agregat ─────────────────────────────
    ("aliasing: slice di dalam struct ikut disalin", r#"package main
struct Kotak { pub isi: []int }
pub fn main() {
    var k1 = Kotak { isi: [9, 9] }
    var k2 = k1
    k2.isi[0] = 88
    wok(k1.isi[0], k2.isi[0])
}"#),
    ("aliasing: array di dalam array ikut disalin", r#"package main
pub fn main() {
    var a1 = [[1], [2]]
    var a2 = a1
    a2[0] = a2[0] + [3]
    wok(a1[0], a2[0])
}"#),

    // ── method string ───────────────────────────────────────────────────
    ("method string: split, contains, ubah huruf", r#"package main
pub fn main() {
    wok("a,b,c".split(","))
    wok("abc".split(""))
    wok("a,,b".split(","))
    wok("a,b,c".split(",").len())
    wok("halo".to_uppercase(), "HALO".to_lowercase())
    wok("halo".contains("al"), "halo".contains("z"))
    wok("halo".starts_with("ha"), "halo".ends_with("lo"))
    wok("a-b".replace("-", "+"))
    wok("  x  ".trim())
}"#),

    // ── nama internal tidak boleh bocor ─────────────────────────────────
    ("nama enum termangel tidak bocor ke keluaran", r#"package main
enum Warna { Merah, Biru }
pub fn main() {
    let o: Option<int> = Some(3)
    wok(o, typeof(o))
    let w = Warna::Merah
    wok(w, typeof(w))
    let r: Result<int, string> = Ok(1)
    wok(r, typeof(r))
}"#),

    // ── dict ────────────────────────────────────────────────────────────
    ("dict: literal, baca, dan cetak", r#"package main
pub fn main() {
    let d = {"b": 2, "a": 1, "c": 3}
    wok(d)
    wok(d["a"], d["b"], d["c"])
    wok(typeof(d), len(d))
}"#),
    ("dict: kunci kembar, yang terakhir menang", r#"package main
pub fn main() {
    wok({"a": 1, "b": 2, "a": 9})
}"#),
    ("dict: namespace dict::", r#"package main
pub fn main() {
    let d = {"a": 1, "b": 2}
    wok(dict::len(d), dict::is_empty(d))
    wok(dict::has(d, "a"), dict::has(d, "z"))
    wok(dict::keys(d), dict::values(d))
    wok(dict::get(d, "a").unwrap())
    wok(dict::remove(d, "a"))
    wok(dict::merge(d, {"z": 9}))
}"#),
    ("dict: tulis lewat indeks, tumbuh dan menimpa", r#"package main
pub fn main() {
    var d = {"a": 1}
    d["b"] = 2
    d["a"] = 9
    wok(d, len(d))
}"#),
    ("dict: iterasi menyebut KUNCI, bukan indeks", r#"package main
pub fn main() {
    let d = {"x": 10, "y": 20}
    for k in d { wok(k, d[k]) }
    for k, v in d { wok(k, v) }
}"#),
    ("dict: kesamaan tidak bergantung urutan penulisan", r#"package main
pub fn main() {
    wok({"a": 1, "b": 2} == {"b": 2, "a": 1})
    wok({"a": 1} == {"a": 2}, {"a": 1} == {"a": 1, "b": 2})
    wok({"a": 1} != {"a": 2})
}"#),
    ("dict: nilai array, panjang boleh beda", r#"package main
pub fn main() {
    let d = {"a": [1, 2], "b": [3]}
    wok(d)
    wok(d["a"], d["b"])
}"#),
    ("dict: penyerahan menyalin, bukan berbagi", r#"package main
pub fn main() {
    var a = {"k": 1}
    var b = a
    b["k"] = 99
    b["baru"] = 5
    wok(a, b)
}"#),
    ("aritmatika int", r#"package main
pub fn main() {
    var a: int = 17
    var b: int = 5
    wok(a + b, a - b, a * b, a / b, a % b)
}"#),

    ("presedensi dan tanda", r#"package main
pub fn main() {
    wok(2 + 3 * 4, (2 + 3) * 4, -7 / 2, -7 % 2)
}"#),

    ("float dibagi dan dicetak", r#"package main
pub fn main() {
    var x: f64 = 7.0
    var y: f64 = 2.0
    wok(x / y, x * y, x - y)
}"#),

    ("bool dan operator logika", r#"package main
pub fn main() {
    var t: bool = true
    var f: bool = false
    wok(t && f, t || f, !t)
}"#),

    ("perbandingan", r#"package main
pub fn main() {
    var a: int = 3
    var b: int = 9
    wok(a < b, a > b, a <= b, a >= b, a == b, a != b)
}"#),

    ("if else berantai", r#"package main
fn kelas(n: int) -> int {
    if n < 10 { return 1 }
    if n < 100 { return 2 }
    return 3
}
pub fn main() {
    wok(kelas(5), kelas(50), kelas(500))
}"#),

    ("while dan akumulator", r#"package main
pub fn main() {
    var i: int = 0
    var s: int = 0
    while i < 10 { s = s + i; i = i + 1 }
    wok(s)
}"#),

    ("for range", r#"package main
pub fn main() {
    var s: int = 0
    for i in 0..5 { s = s + i }
    wok(s)
}"#),

    ("rekursi fib", r#"package main
fn fib(n: int) -> int {
    if n < 2 { return n }
    return fib(n - 1) + fib(n - 2)
}
pub fn main() { wok(fib(15)) }"#),

    ("break dan continue", r#"package main
pub fn main() {
    var s: int = 0
    var i: int = 0
    while i < 20 {
        i = i + 1
        if i % 2 == 0 { continue }
        if i > 11 { break }
        s = s + i
    }
    wok(s, i)
}"#),

    ("array indeks dan len", r#"package main
pub fn main() {
    var a: [4]int = [10, 20, 30, 40]
    wok(a[0], a[3], len(a))
}"#),

    ("array ditulis di tempat", r#"package main
pub fn main() {
    var a: [3]int = [1, 2, 3]
    a[1] = 99
    wok(a[0], a[1], a[2])
}"#),

    ("string dan cetak", r#"package main
pub fn main() {
    var s: string = "halo"
    wok(s, len(s))
}"#),

    ("struct dan field", r#"package main
struct Titik { pub x: int, pub y: int }
pub fn main() {
    var p: Titik = Titik { x: 3, y: 4 }
    wok(p.x, p.y)
}"#),

    ("method dengan self", r#"package main
struct Kotak { pub w: int, pub h: int }
impl Kotak {
    fn luas(self) -> int { return self.w * self.h }
}
pub fn main() {
    var k: Kotak = Kotak { w: 3, h: 5 }
    wok(k.luas())
}"#),

    ("enum dan match", r#"package main
enum Warna { Merah, Hijau, Biru }
fn kode(w: Warna) -> int {
    match w {
        Warna::Merah => { return 1 }
        Warna::Hijau => { return 2 }
        Warna::Biru  => { return 3 }
    }
}
pub fn main() {
    wok(kode(Warna::Merah), kode(Warna::Hijau), kode(Warna::Biru))
}"#),

    ("match pada int dengan default", r#"package main
fn nama(n: int) -> int {
    match n {
        0 => { return 100 }
        1 => { return 200 }
        _ => { return 900 }
    }
}
pub fn main() { wok(nama(0), nama(1), nama(7)) }"#),

    ("fungsi banyak argumen", r#"package main
fn campur(a: int, b: int, c: int) -> int { return a * 100 + b * 10 + c }
pub fn main() { wok(campur(1, 2, 3)) }"#),

    ("cast int ke float dan balik", r#"package main
pub fn main() {
    var i: int = 7
    var f: f64 = i as f64 / 2.0
    wok(f, f as int)
}"#),

    ("operator bit", r#"package main
pub fn main() {
    var a: int = 12
    var b: int = 10
    wok(a & b, a | b, a ^ b, a << 2, a >> 2)
}"#),

    ("ternary dolar", r#"package main
pub fn main() {
    var n: int = 7
    wok(n > 5 $ 111 : 222)
}"#),

    ("loop bersarang", r#"package main
pub fn main() {
    var s: int = 0
    var i: int = 0
    while i < 4 {
        var j: int = 0
        while j < 4 { s = s + i * j; j = j + 1 }
        i = i + 1
    }
    wok(s)
}"#),

    // ── truthiness ─────────────────────────────────────────────────────
    ("truthiness sebagai kondisi", r#"package main
pub fn main() {
    if 2 { wok("dua") } else { wok("SALAH") }
    var n: int = 5
    if n { wok("n") } else { wok("SALAH") }
    var z: int = 0
    if z { wok("SALAH") } else { wok("nol") }
    if "x" { wok("isi") } else { wok("SALAH") }
    if "" { wok("SALAH") } else { wok("kosong") }
}"#),

    ("konversi bool eksplisit", r#"package main
pub fn main() {
    wok(bool(0), bool(1), bool(-5), bool(0.0), bool(1.5))
    wok(bool(""), bool("x"), bool("0"), bool(true), bool(false))
}"#),

    // NaN falsy — `fcmp one`, bukan `une`. Perbedaan satu huruf yang cuma
    // kelihatan kalau NaN benar-benar diuji, dan sudah pernah menyimpang
    // antara interpreter dan VM.
    ("NaN falsy, inf truthy", r#"package main
pub fn main() {
    var n: f64 = math::NAN
    var i: f64 = math::INF
    wok(bool(n), bool(i))
    if n { wok("SALAH") } else { wok("NaN falsy") }
}"#),

    ("konstanta math lengkap", r#"package main
pub fn main() {
    wok(math::PI, math::E, math::TAU, math::INF, math::NAN)
}"#),

    // ── aturan indeks ──────────────────────────────────────────────────
    ("indeks negatif pada array, baca dan tulis", r#"package main
pub fn main() {
    var a: [4]int = [10, 20, 30, 40]
    wok(a[-1], a[-4], a[0], a[3])
    a[-1] = 99
    a[-4] = 11
    wok(a[3], a[0])
}"#),

    ("indeks negatif pada slice", r#"package main
fn ambil(a: array, i: int) -> int { return a[i] }
pub fn main() {
    var a: [5]int = [1, 2, 3, 4, 5]
    wok(ambil(a[0:4], -1), ambil(a[1:4], -2))
}"#),

    // Elemen tuple bertipe beda-beda, jadi indeksnya wajib konstan di
    // backend statis. Yang diuji di sini bahwa yang konstan BEKERJA dan
    // jawabannya sama.
    ("indeks tuple termasuk negatif", r#"package main
pub fn main() {
    var t: (int, string, bool) = (1, "dua", true)
    wok(t[0], t[1], t[2], t[-1], t[-3])
}"#),

    // Indeks string menghitung KARAKTER, bukan byte. Kalau per byte, `u[1]`
    // akan mengembalikan separuh karakter — sebuah rune yang bukan karakter
    // apa pun, tanpa error.
    ("indeks string per karakter, bukan byte", r#"package main
pub fn main() {
    var s: string = "halo"
    wok(s[0], s[3], s[-1])
    var u: string = "aé漢z"
    wok(len(u), u[0], u[1], u[2], u[3], u[-1])
}"#),

    // ── rune sebagai tipe tersendiri ───────────────────────────────────
    ("rune dicetak sebagai karakter", r#"package main
pub fn main() {
    var c: rune = 'h'
    wok(c, typeof(c), c as int)
    wok(65 as rune, typeof(65 as rune))
    wok('a' < 'b', 'a' == 'a', 'b' > 'a')
    var u: rune = '漢'
    wok(u, u as int)
}"#),

    // ── string: konkatenasi, pengulangan, slicing ──────────────────────
    ("konkatenasi string dua arah", r#"package main
pub fn main() {
    wok("n=" + 5, "f=" + 1.5, "b=" + true, "r=" + 'h')
    wok(5 + "=n", "a" + "b")
}"#),

    ("pengulangan string dua arah", r#"package main
pub fn main() {
    wok("ab" * 3, "ab" * 0, 3 * "ab")
}"#),

    // Slicing string menghitung KARAKTER. Kalau per byte, `u[1:3]` akan
    // memotong di tengah karakter dan menghasilkan byte yang bukan UTF-8
    // sah — string rusak, tanpa error.
    ("slicing string per karakter", r#"package main
pub fn main() {
    var s: string = "halo dunia"
    wok(s[0:4], s[5:], s[:4], s[-5:])
    var u: string = "aé漢z"
    wok(u[1:3], len(u[1:3]), u[:2], u[2:])
}"#),

    // Batas slice negatif — aturan yang sama dengan indeks, dan sebelumnya
    // dipanikkan di backend ini persis seperti indeks dulu.
    ("batas slice negatif pada array", r#"package main
fn cetak(a: array) { for i in 0..len(a) { wok_(a[i]) }; wok("") }
pub fn main() {
    var a: [5]int = [1, 2, 3, 4, 5]
    cetak(a[-3:])
    cetak(a[:-2])
    cetak(a[1:-1])
}"#),

    ("konkatenasi array", r#"package main
pub fn main() {
    var a: [2]int = [1, 2]
    var b: [2]int = [3, 4]
    var c = a + b
    wok(len(c), c[0], c[1], c[2], c[3])
    var s1: [2]string = ["a", "b"]
    var s2: [1]string = ["c"]
    var s3 = s1 + s2
    wok(len(s3), s3[0], s3[2])
}"#),

    // ── inferensi tipe ─────────────────────────────────────────────────
    ("parameter tanpa anotasi, banyak tipe", r#"package main
fn cetak(v) { wok(v) }
fn gandakan(x) -> int { return x * 2 }
pub fn main() {
    cetak(7)
    cetak("teks")
    cetak(1.5)
    cetak(true)
    wok(gandakan(21))
}"#),

    ("tipe return disimpulkan dari badan", r#"package main
fn bikin_array() {
    var lokal = [1, 2, 3]
    return lokal
}
fn jumlah(a: int, b: int) { return a + b }
fn nama() { return "jrg" }
fn ubah_dalam(a) { a[0][0] = 777; return a }
pub fn main() {
    var u = bikin_array()
    u[0] = 999
    wok(u[0], u[1], jumlah(2, 3), nama())
    var c: [2][2]int = [[1, 2], [3, 4]]
    let hasil = ubah_dalam(c)
    wok(hasil[0][0], c[0][0])
}"#),

    ("parameter method tanpa anotasi", r#"package main
struct K { pub n: int }
impl K {
    fn tambah(self, x) -> int { return self.n + x }
    fn gabung(self, s) -> string { return s + "!" }
}
pub fn main() {
    var k: K = K { n: 1 }
    wok(k.tambah(2), k.gabung("hai"))
}"#),

    // `match` tanpa konteks tipe: sebelumnya dianggap tak bernilai, jadi
    // `let z = if c { match .. } else { 5 }` ditolak dengan menuduh `if`-nya.
    ("match menyimpulkan tipe dari arm-nya", r#"package main
pub fn main() {
    let z = if true { match 1 { 1 => 111, _ => 0 } } else { 5 }
    wok(z)
    let w = match 2 { 1 => "satu", _ => "lain" }
    wok(w)
}"#),

    // ── pola array ─────────────────────────────────────────────────────
    ("pola array di match", r#"package main
fn klas(a: array) -> string {
    return match a {
        []           => "kosong",
        [x]          => "satu:" + x,
        [x, y]       => "dua:" + x + "," + y,
        [k, ...sisa] => "kepala:" + k + " sisa:" + len(sisa),
    }
}
pub fn main() {
    var e: [0]int = []
    var s: [1]int = [7]
    var d: [2]int = [1, 2]
    var b: [4]int = [1, 2, 3, 4]
    wok(klas(e), klas(s), klas(d), klas(b))
    match b { [p, ..., z] => wok("ujung:", p, z), _ => wok("?") }
    wok(match [1, 2, 3] { [k, ...] => k, _ => -1 })
    wok(match [[1, 2], [3, 4]] { [[a, b], [c, d]] => a + b + c + d, _ => -1 })
    wok(match [1, 99, 3] { [1, _, 3] => "cocok", _ => "tidak" })
    wok(match [2, 99, 3] { [1, _, 3] => "cocok", _ => "tidak" })
}"#),

    ("pola array di let", r#"package main
pub fn main() {
    let [m, n] = [10, 20]
    wok(m, n)
    let [h, ...t] = [1, 2, 3]
    wok(h, len(t), t)
}"#),

    // ── `nil` ──────────────────────────────────────────────────────────
    ("nil: cetak, banding, truthiness", r#"package main
pub fn main() {
    wok(typeof(nil), «${nil}», sprint(nil), nil)
    wok(nil == nil, nil != nil, nil == 1, 1 == nil)
    wok(bool(nil))
}"#),

    // ── template literal ───────────────────────────────────────────────
    ("template literal untuk semua tipe", r#"package main
struct P { pub x: int, pub y: string }
enum W { A, B }
pub fn main() {
    var n: int = 42
    var f: f64 = 1.5
    var b: bool = true
    var s: string = "dunia"
    var r: rune = 'z'
    var a: [3]int = [1, 2, 3]
    var t: (int, string) = (7, "x")
    var p: P = P { x: 1, y: "q" }
    wok(«halo ${s}, n=${n}»)
    wok(«float ${f} bool ${b} rune ${r}»)
    wok(«array ${a} slice ${a[0:2]} tuple ${t}»)
    wok(«struct ${p} enum ${W::A}»)
    wok(«hitung ${n * 2 + 1} dan ${n > 10}»)
    wok(«bersarang ${«dalam ${n}»}»)
    wok(«»)
}"#),

    // ── namespace `arr::` ──────────────────────────────────────────────
    ("arr:: mengembalikan nilai baru", r#"package main
pub fn main() {
    var a: [5]int = [3, 1, 4, 1, 5]
    wok(arr::push(a, 9))
    wok(arr::sum(a), arr::min(a), arr::max(a))
    wok(arr::reverse(a), arr::sort(a))
    wok(arr::contains(a, 4), arr::contains(a, 7))
    wok("asli:", a)
    var k: [0]int = []
    wok(arr::sum(k), arr::reverse(k), arr::contains(k, 1))
}"#),

    ("arr:: pada float dan string", r#"package main
pub fn main() {
    var f: [3]f64 = [2.5, 1.5, 3.0]
    wok(arr::sum(f), arr::min(f), arr::max(f), arr::sort(f))
    var s: [3]string = ["c", "a", "b"]
    wok(arr::sort(s), arr::contains(s, "a"), arr::reverse(s))
}"#),

    ("arr::push menumbuhkan di dalam loop", r#"package main
pub fn main() {
    var t = []
    for i in 0..5 { t = arr::push(t, i * i) }
    wok(len(t), t, arr::sum(t))
}"#),

    // ── array bertumbuh ────────────────────────────────────────────────
    ("kumpulkan hasil di dalam loop", r#"package main
pub fn main() {
    var t = []
    for i in 0..5 { t = t + [i] }
    wok(len(t), t.len(), t[0], t[4], t == [0, 1, 2, 3, 4])
}"#),

    ("kumpulkan tuple, dan di dalam if/match", r#"package main
pub fn main() {
    var a = []
    for i in 0..3 { a = a + [(i, i * 2)] }
    var b = []
    for i in 0..5 { if i > 1 { b = b + [i] } }
    var c = []
    for i in 0..5 { match i { 3 => break, _ => { c = c + [i] } } }
    wok(len(a), len(b), len(c))
}"#),

    // Lokal BERANOTASI tidak ikut berubah — kalau seseorang menulis
    // `var a: [2000]int`, ia meminta array berukuran tetap, dan mengubahnya
    // diam-diam jadi heap akan memperlambat kode yang sudah benar.
    ("array beranotasi tetap berukuran tetap", r#"package main
pub fn main() {
    var a: [3]int = [1, 2, 3]
    wok(len(a), a.len(), a[0])
}"#),

    // ── break/continue berlabel ────────────────────────────────────────
    ("break dan continue berlabel", r#"package main
pub fn main() {
    var a = []
    'luar: for i in 0..3 {
        for j in 0..3 {
            if i == 1 && j == 1 { break 'luar }
            a = a + [(i, j)]
        }
    }
    var b = []
    'lain: for i in 0..3 {
        for j in 0..3 {
            if j == 1 { continue 'lain }
            b = b + [(i, j)]
        }
    }
    var n = 0
    'w: while true { n = n + 1; if n > 2 { break 'w } }
    var m = 0
    'l: loop { m = m + 1; if m > 4 { break 'l } }
    wok(len(a), len(b), n, m)
}"#),

    // ── `typeof` menjawab nama JRG, bukan nama internal ────────────────
    ("typeof memakai nama JRG", r#"package main
struct P { pub x: int }
enum W { A }
fn kosong() {}
pub fn main() {
    var i: int = 1
    var f: f64 = 1.5
    var r: rune = 'a'
    var a: [2]int = [1, 2]
    var t: (int, string) = (1, "a")
    var p: P = P { x: 1 }
    var u: u32 = 7
    wok(typeof(i), typeof(f), typeof(r), typeof(u))
    wok(typeof(a), typeof(a[0:1]), typeof(t), typeof(p))
    wok(typeof(W::A), typeof(kosong()), typeof("x"), typeof(true))
}"#),

    // ── memindahkan slice juga menyalin ────────────────────────────────
    ("memindahkan slice ke nama lain menyalin", r#"package main
fn ambil(a: array) -> array { return a }
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    var s = a[0:3]
    var t = s
    t[0] = 99
    wok(s[0], t[0])
    var g = []
    for i in 0..3 { g = g + [i] }
    var h = g
    h[0] = 77
    wok(g[0], h[0])
    var u = ambil(s)
    u[1] = 55
    wok(s[1], u[1])
}"#),

    // ── slice adalah NILAI, bukan pandangan ────────────────────────────
    ("menulis lewat slice tidak menyentuh asalnya", r#"package main
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    var s = a[0:2]
    s[0] = 99
    wok(a[0], a[1], s[0], s[1])
}"#),

    ("slice dari slice juga terpisah", r#"package main
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    var s = a[0:3]
    var t = s[0:2]
    t[0] = 55
    wok(a[0], s[0], t[0])
}"#),

    ("fungsi yang menulis slice kiriman tidak menyentuh asalnya", r#"package main
fn ubah(s: array) { s[0] = 900 }
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    ubah(a[0:2])
    ubah(a)
    wok(a[0], a[1])
}"#),

    // Sebuah slice bisa MELEWATI hidup frame yang membuatnya.
    ("slice dikembalikan dari fungsi tetap utuh", r#"package main
fn bikin() -> array {
    var lokal: [4]int = [11, 22, 33, 44]
    return lokal[0:3]
}
fn ganggu() -> int {
    var sampah: [8]int = [7, 7, 7, 7, 7, 7, 7, 7]
    return sampah[0]
}
pub fn main() {
    var s = bikin()
    var _ = ganggu()
    wok(len(s), s[0], s[1], s[2])
}"#),

    // ── `a += b` berarti `a = a + b` ───────────────────────────────────
    ("penugasan majemuk pada string", r#"package main
pub fn main() {
    var s: string = "a"
    s += "b"
    s += 5
    wok(s)
    var t: string = "ab"
    t *= 3
    wok(t)
}"#),

    // ── array kosong sebagai literal lentur ────────────────────────────
    ("array kosong di kedua sisi penyambungan", r#"package main
pub fn main() {
    wok(len([1] + []), len([] + [1]))
    var s: [2]string = ["a", "b"]
    wok(len(s + []), len([] + s))
}"#),

    ("array kosong sebagai argumen dan nilai", r#"package main
fn ambil(a: array) -> int { return len(a) }
pub fn main() {
    wok(ambil([]))
    var t = []
    wok(len(t))
}"#),

    // Array berukuran tetap dibandingkan dengan slice: yang dibandingkan ISINYA.
    ("array tetap dibandingkan dengan slice", r#"package main
pub fn main() {
    var xs: [4]int = [1, 2, 3, 4]
    wok(xs[2:2] == [], xs[0:2] == [])
    wok(xs == xs[0:4], xs[0:2] == xs[0:2])
    wok([] == [])
}"#),

    // ── operasi string di level byte ───────────────────────────────────
    ("operasi string dengan karakter multi-byte", r#"package main
pub fn main() {
    var s: string = "Halo Dunia aé漢z"
    wok(str::upper(s))
    wok(str::lower(s))
    wok(str::reverse(s))
    wok(str::replace(s, "a", "_"))
    wok(str::replace(s, "漢", "X"))
    wok(str::contains(s, "Dunia"), str::contains(s, "漢"), str::contains(s, "zz"))
    wok(str::starts_with(s, "Halo"), str::ends_with(s, "z"))
    wok(len(s), str::trim("  x  "))
}"#),

    // Pola kosong menyisipkan penggantinya di setiap BATAS KARAKTER,
    // termasuk kedua ujung. Runtime LLVM dulu mengembalikan stringnya apa
    // adanya — divergensi yang sudah ada sebelum pekerjaan level-byte.
    ("replace dengan pola kosong", r#"package main
pub fn main() {
    wok(str::replace("aé漢", "", "-"))
    wok(str::replace("", "", "-"))
    wok(str::replace("abc", "", "."))
}"#),

    // ── kesetaraan struktural agregat ──────────────────────────────────
    ("eq array tetap", r#"package main
pub fn main() {
    var a: [2]int = [1, 2]
    var b: [2]int = [1, 2]
    var c: [2]int = [1, 3]
    wok(a == b, a == c, a != c, a != b)
}"#),

    ("eq tuple campur tipe", r#"package main
pub fn main() {
    var a: (int, string) = (1, "a")
    var b: (int, string) = (1, "a")
    var c: (int, string) = (2, "a")
    wok(a == b, a == c)
}"#),

    ("eq unit", r#"package main
fn kosong() {}
pub fn main() {
    wok(kosong() == kosong())
}"#),

    ("eq struct field demi field", r#"package main
struct P { pub x: int, pub y: int }
pub fn main() {
    wok(P { x: 1, y: 2 } == P { x: 1, y: 2 }, P { x: 1, y: 2 } == P { x: 9, y: 2 })
}"#),

    ("eq array bersarang", r#"package main
pub fn main() {
    var a: [2][2]int = [[1, 2], [3, 4]]
    var b: [2][2]int = [[1, 2], [3, 4]]
    var c: [2][2]int = [[1, 2], [3, 9]]
    wok(a == b, a == c)
}"#),

    ("eq float dan string di dalam array", r#"package main
pub fn main() {
    var f1: [2]f64 = [1.5, 2.5]
    var f2: [2]f64 = [1.5, 2.5]
    var s1: [2]string = ["a", "b"]
    var s2: [2]string = ["a", "c"]
    wok(f1 == f2, s1 == s2)
}"#),

    // Slice: panjang harus dibandingkan LEBIH DULU. Tanpa itu loopnya
    // membaca melewati ujung yang lebih pendek.
    ("eq slice termasuk beda panjang", r#"package main
fn sama(a: array, b: array) -> bool { return a == b }
pub fn main() {
    var x: [5]int = [1, 2, 3, 4, 5]
    var y: [5]int = [1, 2, 9, 4, 5]
    wok(sama(x[0:3], y[0:2]), sama(x[0:3], y[0:3]), sama(x[0:2], y[0:2]))
}"#),

    // Jalur LOOP, bukan unroll: interpreter menerima array besar, jadi
    // meng-unroll perbandingannya akan meledakkan IR. Kasus "beda di elemen
    // 0" ikut menguji keluar-lebih-awal.
    ("eq array besar lewat loop", r#"package main
pub fn main() {
    var a: [2000]int = [7; 2000]
    var b: [2000]int = [7; 2000]
    wok(a == b)
    b[1999] = 8
    wok(a == b)
    b[1999] = 7
    b[0] = 8
    wok(a == b)
}"#),

    // Sisi POSITIF dari penegakan visibilitas: field privat harus tetap bisa
    // diakses dari dalam method tipenya sendiri, baca maupun tulis.
    ("field privat boleh diakses dari method sendiri", r#"package main
struct Akun { saldo: int = 100, pub nama: string = "a" }
impl Akun {
    fn lihat(self) -> int { return self.saldo }
    fn tambah(self) -> int {
        self.saldo = self.saldo + 5
        return self.saldo
    }
}
pub fn main() {
    var a: Akun = Akun {}
    wok(a.lihat(), a.tambah(), a.nama)
}"#),

    ("shadowing di blok", r#"package main
pub fn main() {
    var x: int = 1
    { var x: int = 2; wok(x) }
    wok(x)
}"#),

    // ── Closure yang dipanggil langsung, dan `move` di posisi HOF ────────
    ("closure langsung dipanggil",        "package main\npub fn main() { wok((|x| x * 2)(4)) }"),
    ("closure langsung, dua parameter",   "package main\npub fn main() { wok((|a, b| a + b)(3, 5)) }"),
    ("closure langsung, argumen string",  "package main\npub fn main() { wok((|s| s + \"!\")(\"halo\")) }"),
    ("closure langsung, tanpa parameter", "package main\npub fn main() { wok((|| 7)()) }"),
    ("closure langsung, anotasi menang",  "package main\npub fn main() { wok((|x: f64| x * 2.0)(1.5)) }"),
    ("closure langsung, move",            "package main\npub fn main() { var n = 1\n wok((move |x| x + n)(10)) }"),
    ("move di HOF memutus tautan",        "package main\npub fn main() { var n = 0\n let b = [1, 2, 3].map(move |x| { n = n + 1\n return x })\n wok(n, b) }"),

    // ── Tangkapan atas PARAMETER closure yang melingkupi ─────────────────
    ("tangkap: fn -> closure atas param fn",     "package main\nfn bikin(a: int) -> fn(int) -> int { return |b: int| { return a + b } }\npub fn main() { wok(bikin(3)(4)) }"),
    ("tangkap: closure atas lokal luar",         "package main\npub fn main() { let n = 3\n let c = |b: int| { return n + b }\n wok(c(4)) }"),
    ("tangkap: closure->closure atas lokal",     "package main\npub fn main() { let d = |a: int| { let m = a\n return |b: int| { return m + b } }\n wok(d(3)(4)) }"),
    ("tangkap: closure->closure atas PARAMETER", "package main\npub fn main() { let e = |a: int| { return |b: int| { return a + b } }\n wok(e(3)(4)) }"),

    // ── Closure berantai (currying) ──────────────────────────────────────
    ("currying: dua tingkat",  "package main\npub fn main() { let luar = |a| { return |b| { return a + b } }\n wok(luar(3)(4)) }"),
    ("currying: tiga tingkat", "package main\npub fn main() { let t = |a| { return |b| { return |c| { return a + b + c } } }\n wok(t(1)(2)(3)) }"),
    ("currying: tipe campur",  "package main\npub fn main() { let c = |s| { return |n| { return s + (n as string) } }\n wok(c(\"x\")(5)) }"),

    // ── Paruh `Result` dari situs panggilan LAIN ─────────────────────────
    ("result: paruh dari situs panggilan lain",
     "package main\nfn lewat(r) { let v = r?\n return Ok(v * 2) }\npub fn main() { wok(lewat(Ok(21)))\n wok(lewat(Err(\"gagal\"))) }"),

    // ── Kesetaraan enum ──────────────────────────────────────────────────
    ("enum ==: Result sama dan beda",
     "package main\npub fn main() { let a: Result<int, string> = Ok(1)\n let b: Result<int, string> = Ok(1)\n let c: Result<int, string> = Ok(2)\n wok(a == b, a == c, a != c) }"),
    ("enum ==: lintas variant",
     "package main\npub fn main() { let a: Result<int, string> = Ok(1)\n let d: Result<int, string> = Err(\"x\")\n wok(a == d, d == d) }"),
    ("enum ==: Option dengan variant unit",
     "package main\npub fn main() { let s: Option<int> = Some(3)\n let t: Option<int> = Some(4)\n let u: Option<int> = None\n wok(s == s, s == t, s == u, u == u) }"),
    ("enum ==: enum pengguna, payload jamak",
     "package main\nenum B { Lingkaran(float), Kotak(float, float), Titik }\npub fn main() { wok(B::Titik == B::Titik, B::Lingkaran(2.0) == B::Lingkaran(2.0), B::Lingkaran(2.0) == B::Lingkaran(3.0)) }"),
    ("enum ==: payload dua field",
     "package main\nenum B { Lingkaran(float), Kotak(float, float), Titik }\npub fn main() { wok(B::Kotak(1.0, 2.0) == B::Kotak(1.0, 2.0), B::Kotak(1.0, 2.0) == B::Kotak(1.0, 9.0), B::Lingkaran(2.0) == B::Titik) }"),

    // ── `d.kunci` pada sebuah dict ───────────────────────────────────────
    ("dict: baca lewat .kunci",  "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d[\"a\"], d.a, d.b) }"),
    ("dict: tulis lewat .kunci", "package main\npub fn main() { var d = {\"a\": 1}\n d.a = 9\n wok(d.a, d[\"a\"]) }"),
    ("dict: .kunci baru menumbuhkan", "package main\npub fn main() { var d = {\"a\": 1}\n d.c = 30\n wok(d.c, dict::len(d), d) }"),
    ("dict: .kunci dengan +=",   "package main\npub fn main() { var d = {\"a\": 1}\n d.a += 5\n wok(d.a) }"),
    // ── `dict::new()` dan method gaya `d.len()` ──────────────────────────
    ("dict::new lalu diisi",     "package main\npub fn main() { var b = dict::new()\n b[\"x\"] = \"halo\"\n wok(b, dict::len(b)) }"),
    ("dict::new tak pernah diisi","package main\npub fn main() { let k = dict::new()\n wok(dict::len(k), dict::is_empty(k), k) }"),
    ("dict method: len/is_empty/has", "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d.len(), d.is_empty(), d.has(\"a\"), d.has(\"z\")) }"),
    ("dict method: keys/values",  "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d.keys(), d.values()) }"),
    ("dict method: remove/merge", "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d.remove(\"a\"), d.merge({\"c\": 3})) }"),
    ("dict method: get",          "package main\npub fn main() { let d = {\"a\": 1}\n wok(d.get(\"a\"), d.get(\"z\")) }"),

    // ── Nilai bertag: dict bernilai CAMPUR TIPE ──────────────────────────
    ("any: dict campur, baca kunci tertulis",
     "package main\npub fn main() { let d = {\"nama\": \"budi\", \"umur\": 17}\n wok(d[\"nama\"], d[\"umur\"], dict::len(d)) }"),
    ("any: dict campur lewat .kunci",
     "package main\npub fn main() { let d = {nama: \"siti\", umur: 20}\n wok(d[\"nama\"], d.umur) }"),
    ("any: dict campur bersarang",
     "package main\npub fn main() { let b = {\"angka\": [1, 2], \"dalam\": {\"k\": \"v\"}}\n wok(b[\"angka\"][1], b[\"dalam\"][\"k\"]) }"),
    ("any: dict campur dengan nil",
     "package main\npub fn main() { let g = {\"ada\": 1, \"kosong\": nil}\n wok(g.get(\"ada\"), g.get(\"kosong\"), g.get(\"tak_ada\")) }"),
    ("any: Option berpayload nol",
     "package main\npub fn main() { let g = {\"ada\": 1, \"kosong\": nil}\n wok(g.get(\"kosong\") == Some(nil), g.get(\"tak_ada\").is_none()) }"),

    // ── Nilai bertag: `var` yang BERGANTI tipe elemen ────────────────────
    ("any: var berganti tipe elemen",
     "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\npub fn main() { jejak = []\n catat(\"a\")\n wok(jejak)\n jejak = []\n catat(1)\n wok(jejak) }"),
    ("any: konflik lewat defer",
     "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\nfn pakai() { defer catat(7)\n catat(\"x\") }\npub fn main() { pakai()\n wok(jejak) }"),

    // Yang TIDAK boleh ikut berubah: koleksi bertipe seragam tetap telanjang.
    // Kalau kotak bocor ke sini, keluarannya masih benar tapi jalannya sudah
    // beda — jadi yang dijaga di sini bukan sekadar jawabannya.
    ("any: dict seragam tetap seragam",
     "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d[\"a\"] + d[\"b\"], dict::len(d)) }"),
    ("any: var tipe elemen konsisten",
     "package main\nvar t = []\nfn tambah(x) { t = arr::push(t, x) }\npub fn main() { tambah(1)\n tambah(2)\n wok(t) }"),

    // ── Method koleksi yang mengembalikan `Option` ───────────────────────
    ("koleksi: first/last/get",
     "package main\npub fn main() { let a = [10, 20, 30]\n wok(a.first(), a.last(), a.get(1), a.get(99)) }"),
    ("koleksi: first pada kosong",
     "package main\npub fn main() { let k = []\n wok(k.first(), k.last()) }"),
    ("koleksi: Some(nil) beda dari None",
     "package main\npub fn main() { let a = [nil]\n wok(a.first(), a.first().is_some()) }"),
    ("koleksi: get_or",
     "package main\npub fn main() { let a = [10, 20]\n wok(a.get_or(1, -1), a.get_or(9, -1)) }"),
    ("koleksi: pop",
     "package main\npub fn main() { let a = [10, 20, 30]\n let (s, d) = a.pop()\n wok(s, d, a) }"),
    ("koleksi: pop pada kosong",
     "package main\npub fn main() { let k = []\n let (s, d) = k.pop()\n wok(s, d) }"),
    ("koleksi: min/max sbg method",
     "package main\npub fn main() { wok([3, 1, 2].min(), [3, 1, 2].max(), [1, 2].sum()) }"),

    // ── Namespace `str::` ────────────────────────────────────────────────
    ("str:: bentuk namespace",
     "package main\npub fn main() { wok(str::contains(\"halo\", \"al\"), str::upper(\"ab\"), str::trim(\"  x  \")) }"),
    ("str:: is_empty/char_at/repeat",
     "package main\npub fn main() { wok(str::is_empty(\"\"), str::is_empty(\"a\"), str::char_at(\"halo\", 1), str::repeat(\"ab\", 3)) }"),

    // ── Iterasi dict memberi KUNCI ───────────────────────────────────────
    ("dict: iterasi memberi kunci",
     "package main\npub fn main() { let d = {\"b\": 2, \"a\": 1}\n var t = []\n for k in d { t = t + [k] }\n wok(t) }"),
    ("dict: iterasi dua variabel",
     "package main\npub fn main() { let d = {\"b\": 2, \"a\": 1}\n var t = []\n for k, v in d { t = t + [«${k}=${v}»] }\n wok(t) }"),

    // ── Lokal SKALAR yang berganti tipe ──────────────────────────────────
    ("dinamis: int lalu float",   "package main\npub fn main() { var x = 1\n x = 2.5\n wok(x, typeof(x)) }"),
    ("dinamis: int lalu bool",    "package main\npub fn main() { var y = 1\n y = true\n wok(y, typeof(y)) }"),
    ("dinamis: float lalu int",   "package main\npub fn main() { var z = 3.9\n z = 1\n wok(z, typeof(z)) }"),
    ("dinamis: berganti di dalam if", "package main\npub fn main() { var x = 1\n if true { x = \"teks\" }\n wok(x, typeof(x)) }"),
    ("dinamis: int lalu string",  "package main\npub fn main() { var x = 1\n x = \"teks\"\n wok(x, typeof(x)) }"),
    ("dinamis: dibaca sebelum berganti", "package main\npub fn main() { var x = 1\n wok(x, typeof(x))\n x = \"teks\"\n wok(x, typeof(x)) }"),
    ("dinamis: global skalar",    "package main\nvar g = 1\npub fn main() { g = \"teks\"\n wok(g, typeof(g)) }"),

    // Aritmetika dan perbandingan atas nilai yang golongannya baru diketahui
    // saat jalan.
    ("dinamis: aritmetika setelah berganti", "package main\npub fn main() { var x = 1\n x = \"a\"\n x = 2\n wok(x + 1) }"),
    ("dinamis: int + float bertag", "package main\npub fn main() { var y = 1\n y = 2.5\n wok(y + 1, y * 2) }"),
    ("dinamis: gabung string dua arah", "package main\npub fn main() { var s = 1\n s = \"n=\"\n var t = 1\n t = \"x\"\n wok(s + 5, 5 + t) }"),
    ("dinamis: kesetaraan",       "package main\npub fn main() { var x = 1\n x = 2\n var y = 1\n y = \"teks\"\n wok(x == 2, x != 2, y == \"teks\", y == \"lain\") }"),
    ("dinamis: urutan angka",     "package main\npub fn main() { var z = 1\n z = 2.5\n wok(z > 2.0, z < 2.0, z >= 2.5) }"),
    ("dinamis: typeof dari tag",  "package main\npub fn main() { let d = {\"a\": 1, \"b\": \"t\"}\n let k = \"a\"\n wok(typeof(d[k])) }"),

    // Yang TIDAK boleh ikut berubah: lokal yang tipenya konsisten tetap
    // telanjang. Kalau kotaknya bocor ke sini, jawabannya masih benar tapi
    // jalannya sudah beda — jadi yang dijaga bukan sekadar keluarannya.
    ("dinamis: lokal konsisten tetap telanjang", "package main\npub fn main() { var x = 1\n x = 2\n x = 3\n wok(x + 1, typeof(x)) }"),
    ("dinamis: float konsisten tetap telanjang", "package main\npub fn main() { var f = 1.5\n f = 2.5\n wok(f * 2.0, typeof(f)) }"),
    // Posisi per KARAKTER, sama seperti dua backend lain — dan `find`/
    // `index_of` sebelumnya tidak ada di sini sama sekali.
    ("posisi: find per karakter",    "package main\npub fn main() { wok(str::find(\"héllo\", \"l\"), \"héllo\".index_of(\"l\")) }"),
    // Bentuk pulang-perginya lewat `Option` sejak Fase MUDAH. Kasus lamanya
    // berbunyi `s[str::find(s, "k")]` — yang persis jebakan yang ditutup: `-1`
    // adalah indeks yang SAH, jadi pencarian gagal membaca karakter terakhir.
    ("posisi: pulang-pergi s[find]",
     "package main\npub fn main() { let s = \"日本語kode\"\n       match str::find(s, \"k\") { Some(i) => wok(s[i]), None => wok(\"tidak ada\") }\n       match str::find(s, \"z\") { Some(i) => wok(s[i]), None => wok(\"tidak ada\") } }"),
    ("posisi: find tidak ketemu",    "package main\npub fn main() { wok(str::find(\"halo\", \"z\")) }"),

    // `**=` dan struct tuple keduanya diturunkan di PARSER, jadi backend ini
    // mewarisinya tanpa disentuh — dan itu justru yang perlu dibuktikan.
    ("operator **=",
     "package main\npub fn main() { var a = 2\n a **= 10\n wok(a) }"),
    ("struct tuple: konstruksi dan field",
     "package main\nstruct Titik(int, int)\n\
      pub fn main() { let t = Titik(3, 4)\n wok(t._0 + t._1) }"),

    // Pola di posisi parameter, diturunkan di parser jadi parameter biasa plus
    // `let` — jadi backend ini mewarisinya tanpa disentuh.
    ("pola di parameter",
     "package main\nfn jumlah((a, b)) -> int { return a + b }\n\
      pub fn main() { wok(jumlah((3, 4))) }"),

    // Bentuk namespace HOF dialihkan ke bentuk method yang sudah ada di backend ini, dan `fold`
    // memakai mesin `reduce` dengan letak nilai awal yang bertukar.
    ("hof namespace dan fold",
     "package main\npub fn main() { wok(arr::map([1, 2, 3], |x| x * 2))\n\
      wok(arr::filter([1, 2, 3], |x| x > 1))\n wok(arr::reduce([1, 2, 3], |a, b| a + b, 0))\n\
      wok(arr::any([1, 2], |x| x > 1))\n wok(arr::find([1, 2], |x| x > 1))\n\
      wok([1, 2, 3].fold(10, |a, b| a + b))\n wok(arr::fold([1, 2, 3], 10, |a, b| a + b)) }"),

    // ── Warisan struct (Fase LLVM-14) ──────────────────────────────────────
    (
        "warisan: field induk terbaca, dan tampilannya sama",
        r#"package main
struct Hewan { pub nama: string }
struct Anjing: Hewan { pub ras: string }
pub fn main() {
    let a = Anjing { nama: "Bo", ras: "beagle" }
    wok(a.nama, a.ras)
    wok(a)
}"#),
    (
        "warisan: nilai default milik induk ikut berlaku",
        r#"package main
struct Hewan { pub nama: string = "tanpa nama", pub kaki: int = 4 }
struct Anjing: Hewan { pub ras: string = "campuran" }
pub fn main() { let a = Anjing {}  wok(a.nama, a.kaki, a.ras) }"#),
    (
        "warisan: method induk dipanggil atas anak",
        r#"package main
struct Hewan { pub nama: string }
impl Hewan { pub fn sapa(self) -> string { return "halo " + self.nama } }
struct Anjing: Hewan { pub ras: string }
pub fn main() { wok(Anjing { nama: "Bo", ras: "beagle" }.sapa()) }"#),
    (
        "warisan: rantai tiga tingkat",
        r#"package main
struct A { pub a: int }
struct B: A { pub b: int }
struct C: B { pub c: int }
pub fn main() { let x = C { a: 1, b: 2, c: 3 }  wok(x.a + x.b + x.c) }"#),
    // Bayangan yang MENGGANTI TIPE menentukan rancangannya.
    (
        "warisan: anak membayangi field induk, tipenya ikut berganti",
        r#"package main
struct P { pub n: int }
impl P { pub fn lihat(self) -> string { return "n=" + self.n } }
struct Q: P { pub n: string }
pub fn main() { let q = Q { n: "teks" }  wok(q.n)  wok(q.lihat()) }"#),
    (
        "warisan: field privat induk lewat method warisan",
        r#"package main
struct P { rahasia: int = 5 }
impl P { pub fn baca(self) -> int { return self.rahasia } }
struct Q: P { pub x: int }
pub fn main() { wok(Q { x: 1 }.baca()) }"#),
    (
        "warisan: pola, sebar, dan hook to_string",
        r#"package main
struct Hewan { pub nama: string = "x", pub kaki: int = 4 }
impl Hewan { pub fn to_string(self) -> string { return "<" + self.nama + ">" } }
struct Anjing: Hewan { pub ras: string = "y" }
pub fn main() {
    let a = Anjing { nama: "Bo", ras: "beagle" }
    match a { Anjing { nama, ras } => wok(nama, ras) }
    let b = Anjing { nama: "Rex", ...a }
    wok(b.nama, b.kaki, b.ras)
    wok(a)
}"#),

    // Fase SINKRON-LLVM: `Atomic` dan `Mutex` adalah pegangan di backend ini juga.
    ("Atomic: baca, tulis, tambah, cas, dan salinan yang menunjuk keadaan yang sama", r#"package main
pub fn main() {
    let a = Atomic::new(5)
    wok(a, typeof(a))
    wok(atomic::add(a, 3), atomic::sub(a, 1), atomic::get(a))
    wok(atomic::set(a, 10), atomic::get(a))
    wok(atomic::cas(a, 10, 20), atomic::cas(a, 10, 30), atomic::get(a))
    let b = a
    atomic::add(b, 1)
    wok(atomic::get(a), a == b, a == Atomic::new(21), a != b)
    wok([a, b], (a, 1), "x" + str(a))
    let z = Atomic::new()
    wok(atomic::get(z))
}"#),
    ("Atomic dan Mutex sebagai field, lewat spawn dan closure", r#"package main
struct K { pub hit: Atomic, pub m: Mutex }
fn naik(a: Atomic, n: int) { for i in 0..n { atomic::add(a, 1) } }
pub fn main() {
    let k = K { hit: Atomic::new(0), m: Mutex::new() }
    let ch = make_chan(int, 4)
    for i in 0..4 {
        spawn { naik(k.hit, 100); ch <- 1 }
    }
    for i in 0..4 { <-ch }
    wok(atomic::get(k.hit), k)
    let f = |x| { atomic::add(k.hit, x) }
    wok(f(5))
    var total = 0
    let m = k.m
    wok(m, typeof(m), m == k.m, m == Mutex::new())
    wok(mutex::lock(m))
    wok(mutex::unlock(m))
    let r = mutex::with_lock(m, || { 7 * 6 })
    wok(r)
    wok(mutex::with_lock(m, || "teks"))
    wok("beres")
}"#),
    ("mutex::with_lock menjaga baca-tidur-tulis antar goroutine", r#"package main
pub fn main() {
    let m = Mutex::new()
    let p = Atomic::new(0)
    let ch = make_chan(int, 8)
    for i in 0..8 {
        spawn {
            mutex::with_lock(m, || {
                let v = atomic::get(p)
                time::sleep(0.001)
                atomic::set(p, v + 1)
            })
            ch <- i
        }
    }
    for i in 0..8 { <-ch }
    wok(atomic::get(p))
}"#),
    ("ekor badan perulangan bukan nilai kembalian fungsi tanpa anotasi", r#"package main
fn hitung(n: int) { for i in 0..n { math::abs(i) } }
fn putar(n: int) { var i = 0
    while i < n { i += 1; str::upper("a") } }
fn ulang(xs: []int) { for _, x in xs { x * 2 } }
fn cari(xs: []int) { for _, x in xs { if x > 1 { return x } }
    return -1 }
pub fn main() {
    wok(hitung(3), putar(2), ulang([1, 2]), cari([1, 5]))
}"#),
    ("Atomic dan Mutex lewat method, fungsi tanpa anotasi, Future, Task, Option dan dict", r#"package main
struct Pencacah { pub n: Atomic, pub m: Mutex }
impl Pencacah {
    fn baru() -> Pencacah { Pencacah { n: Atomic::new(0), m: Mutex::new() } }
    fn naik(self) { atomic::add(self.n, 1) }
    fn baca(self) -> int { atomic::get(self.n) }
}
fn buat() { Atomic::new(7) }
fn kunci_baru() { Mutex::new() }
fn tambah(a, n) { atomic::add(a, n) }
fn tukar(a: Atomic) { atomic::cas(a, 7, 70) }
pub fn main() {
    let p = Pencacah::baru()
    p.naik(); p.naik()
    let q = p
    q.naik()
    wok(p.baca(), q.baca(), p == q)
    let a = buat()
    wok(tambah(a, 3), tukar(a), tukar(buat()), atomic::get(a))
    let m = kunci_baru()
    let hasil = mutex::with_lock(m, || { atomic::add(a, 1); [atomic::get(a), 2] })
    wok(hasil, str(a), str(m), """${a} ${m}""")
    let f = Future::new(|| { mutex::with_lock(m, || atomic::add(a, 5)) })
    wok(future::await(f))
    var hit = Atomic::new(0)
    let t = Task::spawn(|| { for i in 0..1000 { atomic::add(hit, 2) }; atomic::get(hit) })
    wok(task::join(t), atomic::get(hit))
    hit = Atomic::new(-4)
    wok(atomic::get(hit), atomic::sub(hit, -9223372036854775807))
    let o: Option<Atomic> = Some(a)
    wok(o, typeof(o), [m] == [m], (a, m) == (a, m), (a, m) == (buat(), m))
    let d = {"x": a, "y": buat()}
    wok(d, atomic::add(d["x"], 1), atomic::get(a))
    let g = Future::new(|| { mutex::with_lock(m, || { let z = [1]; z[4] }) })
    time::sleep(0.02)
    mutex::lock(m); wok("kunci bebas lagi"); mutex::unlock(m)
}"#),

    // Fase CAMPUR: dulu ditolak (`BELUM_DIDUKUNG_LLVM`); elemennya sekarang bertag, jadi `2` tetap bulat.
    ("literal bulat ditambahkan ke array pecahan",
     "package main\npub fn main() { var f = [1.5]\n f = arr::push(f, 2)\n wok(f)\n wok(typeof(f[1]), f[1] / 4, f[0] + f[1]) }"),
    // Fase FFI-LLVM: tipe kembalian dari `return Ok(..)` saja, atau dari `?` plus `return Ok(..)`.
    ("tanpa anotasi: return Ok saja", "package main\nfn f() { return Ok(0) }\npub fn main() { match f() { Ok(v) => wok(\"ok\", v), Err(e) => wok(\"err\", e) } }"),
    ("tanpa anotasi: ? lalu return Ok", "package main\nfn cek(n: int) -> Result<int, string> { if n > 0 { return Ok(n) }\n return Err(\"negatif\") }\nfn f(n) {\n let x = cek(n)?\n return Ok(x * 2) }\npub fn main() { wok(f(3), f(-1)) }"),
    ("? di main yang berhasil", "package main\npub fn main() { let x = Result::Ok(5)?\n let y = [7].first()?\n wok(x + y) }"),
    // Fase MAIN-HASIL (#834): `main` beranotasi `Result`/`Option` dulu ditolak saat kompilasi.
    ("main beranotasi Result yang berhasil", "package main\nfn f(x: int) -> Result<int, string> { Ok(x * 2) }\npub fn main() -> Result<int, string> { wok(f(3)?)\n Ok(5) }"),
    // Fase AMAN-1 (#840): NUL dulu memotong jalur di runtime C — `mkdir("d\0x")` membuat `d`.
    ("jalur ber-NUL tidak menyentuh berkas lain", r#"package main
pub fn main() {
    let z = str(rune(0))
    wok(fs::exists("Cargo.toml" + z + "x"), fs::is_file("Cargo.toml" + z), fs::mkdir("jrg_uji_nul" + z + "x"))
    wok(fs::exists("jrg_uji_nul"), env::get("PATH" + z), fs::delete("jrg_uji_nul" + z))
}
"#),
    // Fase AMAN-21 (#899): gzip:: — CRC32, ukuran, bit cadangan, ekor liar dan data terpotong jadi Err.
    ("gzip:: dan gzip jahat", r#"package main
pub fn main() {
    let z = gzip::compress("halo gzip\n")
    wok(len(z), z[0], z[1], z[2])
    wok(gzip::decompress_text(z).unwrap() == "halo gzip\n")
    wok(gzip::decompress(gzip::compress([0, 255, 7])))
    let h = [31, 139, 8, 8, 133, 111, 229, 106, 0, 3, 104, 97, 108, 111, 46, 116, 120, 116, 0, 203, 72, 204, 201, 87, 72, 175, 202, 44, 224, 2, 0, 57, 190, 200, 48, 10, 0, 0, 0]
    wok(gzip::decompress_text(h + gzip::compress("lagi")))
    var rusak = h
    rusak[31] = 0
    wok(gzip::decompress(rusak))
    wok(gzip::decompress_text(gzip::compress([255, 254])))
    wok(gzip::decompress(h + [0]))
    wok(gzip::decompress([80, 75, 3, 4]))
    var cadangan = h
    cadangan[3] = 40
    wok(gzip::decompress(cadangan))
    wok(gzip::decompress(h[0:20]))
}
"#),
    // Fase AMAN-29 (#908): masukan di pesan galat dipotong dan kendalinya di-escape — pesan tidak tumbuh mengikuti masukan.
    ("pesan galat toml::/url:: tidak mengutip masukan utuh", r#"package main
fn main() {
    let x = str::repeat("9", 2000)
    for _, r in [toml::parse("a = " + x), toml::parse("a = " + x + "z"), toml::parse("[" + str::repeat("k", 500) + "]\n[" + str::repeat("k", 500) + "]")] {
        match r {
            Ok(_) => wok("ok"),
            Err(e) => wok(len(e), e),
        }
    }
    for _, r in [url::parse("http://h:" + x), url::parse("http://" + str::repeat("a", 300) + "%41/"), url::parse("http://h:8a")] {
        match r {
            Ok(_) => wok("ok"),
            Err(e) => wok(len(e), e),
        }
    }
}
"#),
    // Fase SEBAR-VARIAN (#975): sebar di literal varian ber-field dari struct atau varian enum yang sama.
    ("sebar di literal varian: dari struct dan varian enum", r#"package main
enum E { V { a: int, b: int }, W { a: int }, U, T(int), X { z: string } }
struct P { a: int, b: int }
struct Q { a: int }
fn ambil(e: E) -> E { E::V { a: 7, ...e } }
pub fn main() {
    let x = E::V { a: 1, b: 2 }
    wok(E::V { a: 5, ...x })
    wok(E::V { ...x })
    wok(E::V { b: 9, ...E::W { a: 3 } })
    wok(E::V { b: 1, ...P { a: 4, b: 6 } })
    wok(E::V { b: 1, ...Q { a: 8 } })
    wok(ambil(x))
    wok(E::W { ...E::W { a: 2 } })
}
"#),
    // Fase OPSI-GANTI (#973–#974): lokal `Option` berganti isi jadi nilai bertag; method `Option` atas nilai bertag.
    ("opsi: lokal Option berganti isi", r#"package main
pub fn main() {
    var o = Some(1)
    o = Some(o)
    o = Some(o)
    wok(o)
    match o { Some(v) => wok(v), None => wok("n") }
    var p = Some(1)
    p = None
    p = Some("t")
    wok(p, p.is_some())
    var q = Some(3)
    q = None
    wok(q, q.is_none())
    var w = Some(1)
    for i in 0..3 { if i == 1 { w = Some([i]) } }
    wok(w)
    var x = nil
    x = Some(4)
    wok(x.is_some(), x.unwrap(), x.unwrap_or(0))
    x = Ok("a")
    wok(x.is_ok(), x.is_err(), x.unwrap())
    x = None
    wok(x.is_none(), x.unwrap_or(9))
}
"#),
    // Fase PANGKAT-TEPAT (#970–#971): `float ** int` dibulatkan tepat di tiga mesin; `**` atas nilai bertag.
    ("pangkat: pecahan dibulatkan tepat, nilai bertag", r#"package main
fn f(x: float, n: int) -> float { return x ** n }
pub fn main() {
    let xs = [1.1, 1.0001, 0.7, 9.87654321, 1.01]
    let ns = [10, 37, 123, 15, 1000]
    for i, x in xs { wok(f(x, ns[i])) }
    wok(1.1 ** 10, 2 ** -3, 3 ** 0.5)
    var x = nil
    x = 2
    wok(x ** 10, x ** -2, x ** 0.5)
    x = 1.1
    wok(x ** 10, x ** 2.5)
    var n = [1, 2.5]
    n[0] **= 2
    n[1] **= 2
    wok(n)
}
"#),
    // Fase TAMBAH-TEKS (#967–#968): `+` dengan teks menghasilkan teks, dua array bertag tersambung, `dict::set` berjenis lain.
    ("tambah: teks lawan apa pun, array bertag, dict::set berjenis lain", r#"package main
pub fn main() {
    var c = [1]
    c = c + "z"
    wok(c, typeof(c))
    var e = [1]
    e = e + [2] + "z"
    wok(e)
    var n = 1
    n = n + "x"
    wok(n, len(1 + "ab"))
    var s = "x"
    s = s + 1 + true + [3, 4]
    wok(s)
    var x = nil
    x = [1, 2]
    var z = nil
    z = ["q"]
    let w = x + z + [3.5]
    wok(w, len(w), w[1])
    var d = {"a": 1}
    d = dict::set(d, "b", 2)
    d = dict::set(d, 3, "tiga")
    wok(d, d["a"] + 1)
}
"#),
    // Fase ISI-ARRAY (#965–#966): elemen `var` array ditulis berjenis lain; literal wadah bersama bukan-wadah.
    ("array: elemen ditulis berjenis lain, wadah campur bukan wadah", r#"package main
pub fn main() {
    var a = [1, 2]
    a[1] = "q"
    wok(a, a[0] + 1)
    var b = [1, 2]
    b = arr::push(b, 3)
    b[0] = "q"
    wok(b)
    var c = [[1], [2]]
    c[0][0] = "q"
    wok(c)
    var e = [[1, 2], "z"]
    e[1] = [1, 1]
    wok(e, arr::max(e), arr::min(e))
    var f = [[1], {"a": 1}]
    f[0] = [3]
    wok(f)
    var g = ["a"]
    g[0] = 5
    g = arr::push(g, [1])
    wok(g)
    var h = [1, 2]
    for i in 0..2 { h[i] = 1.5 }
    wok(h)
}
"#),
    // Fase AMAN-39 (#962): literal array berisi array berbeda panjang atas variabel/tuple/Option jadi slice.
    ("array bersarang berbeda panjang atas tuple dan Option", r#"package main
pub fn main() {
    let x = 1
    let t = (1, "b")
    let p = Some(2)
    var a = [[t], [t, t], []]
    a[2] = [t]
    wok([[x], [x, x]], a, [[p], [p, p]])
    for _, r in [[("a", "b")], [("c", "d"), ("e", "f")]] { wok(len(r), r) }
}
"#),
    // Fase LABEL-LLVM (#952–#953): `str(x)` di rantai sambungan ditulis langsung kalau `x` ternyata bulat.
    ("teks: str(aritmetika/field) di rantai sambungan", r#"package main
struct R { pub id: int, pub skor: float }
pub fn main() {
    let x: int = -9223372036854775807 - 1
    let r = R { id: 7, skor: 1.5 }
    let f = 2.5
    let a = [3, 40, -5]
    wok("a" + str(x % 10) + "b" + str(r.id / 2) + str(f * 2.0) + "|" + str(x))
    wok("n" + str(r.id - 10) + "|" + str(3 * 4) + "|" + str(r.skor) + str(a[1] % 7) + str(a[2]))
    let lebar = 7
    wok("x" + str(lebar * 1000000000000) + "/" + str(0) + str(-0))
}
"#),
    // Fase POLA-BERTAG (#948–#951): pola varian enum generik, varian unit telanjang, dan `for` atas nilai bertag.
    ("bertag: pola Option/Result/enum generik", r#"package main
enum Pohon<T> { Daun(T), Cabang(T, int), Kosong }
fn cek(x) {
    match x {
        Some(1) => "satu",
        Some(v) => "some " + str(v),
        None => "none",
        Ok(v) => "ok " + str(v),
        Err(e) => "err " + e,
        Pohon::Daun(d) => "daun " + str(d),
        Cabang(a, b) => "cabang " + str(a) + " " + str(b),
        Kosong => "kosong",
        _ => "lain",
    }
}
fn belakangan() {
    let a = [Some([1, 2]), 5]
    wok(cek(a[0]))
    let b = [Some(Some(3)), "x"]
    match b[0] {
        Some(Some(n)) => wok("dalam", n),
        _ => wok("tidak"),
    }
}
pub fn main() {
    let xs = [Some(1), Some("dua"), None, 4]
    for _, x in xs { wok(cek(x)) }
    let r: Result<int, string> = Ok(7)
    let e: Result<int, string> = Err("rusak")
    let ys = [r, e, Pohon::Daun(2.5), Pohon::Cabang("c", 3), "s"]
    for _, y in ys { wok(cek(y)) }
    let k: Pohon<int> = Pohon::Kosong
    let zs = [k, 1]
    wok(cek(zs[0]))
    belakangan()
    let campur = [Some(9), "q"]
    let Some(q) = campur[0] else { return }
    wok("q", q + 1)
    wok("akhir")
}
"#),
    ("bertag: varian unit telanjang", r#"package main
enum Arah { Kiri, Kanan }
pub fn main() {
    let a = [Arah::Kiri, 1, None, Arah::Kanan]
    for _, x in a {
        match x {
            Kiri => wok("kiri"),
            Arah::Kanan => wok("kanan"),
            None => wok("none"),
            n => wok("lain", n),
        }
    }
}
"#),
    ("bertag: for atas dict, array, teks, bulat", r#"package main
pub fn main() {
    var d = dict::new()
    d["m"] = {"b": 2, "a": [1, "x"]}
    d["n"] = [5, "enam", 7.5]
    d["s"] = "héllo"
    d["k"] = 3
    for k in d["m"] { wok("kunci", k) }
    for k, v in d["m"] { wok(k, v) }
    for i, v in d["n"] {
        if i == 1 { continue }
        wok(i, v)
    }
    for i, c in d["s"] {
        if c == 'l' { break }
        wok(i, c)
    }
    for i in d["k"] { wok("n", i) }
    var e = d["n"]
    for i, v in e { e[0] = 99; wok(v) }
    wok(e)
    var jumlah = 0
    for _, v in [1, "a", 2.5] { jumlah = jumlah + 1 }
    wok(jumlah)
}
"#),
    // Fase CELAH-BERTAG (#945–#947): tulis lewat indeks ke array/dict bertag (tanpa alias), `t.0` atas tuple bertag.
    ("bertag: tulis lewat indeks, field tuple", r#"package main
pub fn main() {
    var a = []
    a = arr::push(a, ("x", 1))
    a = arr::push(a, 5)
    let t = a[0]
    wok(t.0, t.1)
    var d = dict::new()
    d["k"] = [1, "dua"]
    d["m"] = {"a": 1}
    d["n"] = 3
    var e = d["k"]
    e[1] = "tiga"
    e[-1] = [9]
    var f = d["m"]
    f["b"] = "x"
    f["a"] = 2.5
    wok(d, e, f)
    d["k"] = e
    var g = e
    g[0] = 100
    wok(d, e, g)
}
"#),
    ("bertag: tulis lewat jalur bersarang", r#"package main
pub fn main() {
    var d = dict::new()
    d["k"] = [1, "dua"]
    d["n"] = 3
    d["m"] = {"a": 1}
    d["k"][1] = "x"
    d["k"][-2] = [7]
    d["m"]["b"] = "baru"
    var a = []
    a = arr::push(a, [1, 2])
    a = arr::push(a, "s")
    a[0][1] = "y"
    let salin = d["k"]
    d["k"][0] = 0
    wok(d, a, salin)
}
"#),
    // Fase TEKS-LLVM (#942–#944): sambungan teks n-ari (bulat, `str(bulat)`, negatif, MIN), `s[i]`/`len` lewat
    // memo di IR, `reverse`/`replace` jalur cepat — atas teks ASCII dan multibyte.
    ("teks: sambungan n-ari, indeks dan len lewat memo, reverse/replace", r#"package main
fn nama() { return "fn" }
pub fn main() {
    let n = -42
    let m = -9223372036854775807 - 1
    var s = "a"
    s = s + "b" + "c"
    wok("x" + str(1) + ":" + str(-7) + str(n) + "|" + 0 + "|" + m)
    wok("" + "" + "")
    wok("p" + str(2.5) + "q" + 1.5 + 'r' + true)
    wok("t" + nama() + str("u") + str([1, 2]) + s)
    var t = ""
    for i in 0..5 { t = t + str(i) + "," + i * i + ";" }
    wok(t, len(t))
    for _, w in ["", "abc", "aé日z", "日本", "x"] {
        var o = str(len(w)) + ":"
        for i in 0..len(w) { o = o + str(w[i]) + str(str::char_at(w, i)) + str(w[-1 - i]) }
        wok(o, str::reverse(w), str::replace(w, "a", "_"), str::replace("aXbXXc", "XX", "-"))
    }
}
"#),
    // Fase AMAN-38 (#930–#936): cabang `typeof` pasti, `map_err` di penduga, `Ok(None)` lalu `Ok(Some(..))`,
    // bulat+pecahan di satu literal, `rune` di `str::`, hasil `map` berukuran tetap, dict campur di `for`.
    ("typeof pasti, map_err, separuh Result, literal bulat+pecahan, map meluruh", r#"package main
struct W { pub t: int }
fn _byte(x) {
    if typeof(x) == "string" { return str::bytes(x) }
    if typeof(x) != "array" { return [] }
    return x
}
fn _cek(w) { if w.t < 0 { return Err("negatif") } return Ok(w) }
fn _tiga(s) {
    if s == "" { return Err("kosong") }
    return Ok((1, 2, 3))
}
fn urai(s) {
    if len(s) > 10 { return Err(«panjang ${s}») }
    let (a, b, c) = _tiga(s).map_err(|e| «x ${s}: ${e}»)?
    return _cek(W { t: a + b + c - len(s) }).map_err(|e| «y ${s}: ${e}»)
}
fn cari(n) {
    if n < 0 { return Err("negatif") }
    if n == 0 { return Ok(None) }
    return Ok(Some((n * 2, ["a", "b"])))
}
fn bikin() { return Ok({"A": "1", "B": "2"}) }
fn kunci() {
    let d = bikin().map_err(|e| «x: ${e}»)?
    var k = []
    for n, _ in d { k = arr::push(k, n) }
    return Ok(k)
}
fn isi(e, kini) { return [math::min(3.0, e[0] + 0.5), kini] }
pub fn main() {
    wok(len(_byte("ab")), len(_byte([1, 2, 3])), len(_byte(7)))
    wok(urai("ab"), urai(""), urai("abcdefghijk"))
    wok(cari(2).unwrap(), cari(0).unwrap(), cari(-1))
    wok(kunci())
    let kini = 5
    let d = {"a": [1.0, 2]}
    var e = dict::has(d, "a") $ isi(d["a"], kini) : [3 as float, kini]
    wok(e, isi([2.5, 1], 9))
    wok(str::is_alphanumeric('a'), str::is_alphanumeric('-'))
    var ops = arr::map(["p", "q"], |x| ["=", x])
    ops = arr::concat(ops, arr::push([], ["+", "r"]))
    wok(ops)
    for k, v in {"x": 1, "y": "dua"} { wok(k, v) }
}
"#),
    // Fase AMAN-38 (#938): `Option<any>` ke `Option<T>`, lokal yang berganti jenis di ekor blok, dict berisi dict,
    // `arr::min`/`max` atas slice, argumen dari fungsi ber-closure.
    ("None tanpa petunjuk ke field Option, v = v[k] di ekor blok, dict berisi dict", r#"package main
struct O { pub nama: string, pub bawaan: Option<string> = None }
fn buat(n, b) { return O { nama: n, bawaan: b } }
fn ambil(data, j) {
    var v = data
    for _, k in str::split(j, ".") {
        if typeof(v) == "dict" {
            if !dict::has(v, k) { return Err("tidak ada") }
            v = v[k]
        } else {
            return Err("bukan dict")
        }
    }
    return Ok(v)
}
fn cari(cs, dari, a) {
    var i = dari
    while i < len(cs) {
        if cs[i] == a { return i }
        i += 1
    }
    return -1
}
pub fn main() {
    wok(buat("a", None), buat("b", Some("x")))
    let d = {"a": {"b": "x"}, "c": "y"}
    wok(ambil(d, "a.b"), ambil(d, "c"), ambil(d, "c.z"))
    let cs = arr::map(str::chars("bbab"), |r| str(r))
    wok(cari(cs, 0, "a"))
    var xs: []float = []
    xs = arr::push(xs, 4.5)
    xs = arr::push(xs, 2.0)
    wok(arr::min(xs), arr::max(xs))
    let ini = {"": {"nama": "jrg"}, "s": {"port": 8080, "host": "h"}}
    for b, isi in ini { for k, v in isi { wok(b, k, v) } }
}
"#),
    // Fase AMAN-37 (#926–#929): urutan atas array bertag, `sort_desc` stabil, literal campur, konstanta modul.
    ("urutan array bertag, sort_desc stabil, literal campur dari variabel", r#"package main
const NAMA = ["nol", "satu", "dua"]
pub fn main() {
    var a = []
    a = arr::push(a, 2)
    a = arr::push(a, 2.0)
    a = arr::push(a, 1)
    wok(arr::sort_desc(a), arr::sort(a))
    var h = []
    for _, k in ["pisang", "apel", "jeruk", "anggur"] {
        let n = len(k)
        let i = 0
        h = arr::push(h, [n, i, k])
    }
    wok(arr::sort(h), arr::sort_desc(h))
    var x = []
    x = [x, 1]
    var y = []
    y = [y, 2]
    var t = []
    t = [t, true]
    var f = []
    f = [f, false]
    wok(x < y, y < x, x <= x, f < t)
    wok(NAMA[1], NAMA[-1], len(NAMA))
    var s = ""
    for i, c in "aé😀中z" { s = s + str(i) + c }
    wok(s)
}
"#),
    // Fase AMAN-34 (#919): array/dict di nilai bertag ditampilkan dan dibandingkan penjelajah C tanpa rekursi.
    ("tampil array/dict bertag berisi ragam nilai", r#"package main
struct P { pub x: int, pub y: string }
enum W { Merah, Isi(int) }
pub fn main() {
    var a = []
    a = [a, 1, 2.5, "teks", 'r', true, nil]
    wok(a)
    var d = dict::new()
    d["k"] = 1
    d["s"] = 2
    var x = []
    x = [x, d, [d], P { x: 1, y: "b" }, W::Isi(3), W::Merah, Some(4), (1, "t")]
    wok(x)
    let kosong: dict<int> = dict::new()
    var y = []
    y = [y, kosong, [[]], [1.0, 2.0], -0.5]
    wok(y, str(y), len(str(y)))
    var z = []
    z = [z, [[1, 2], [3]], ["a", "b"]]
    wok(z)
}
"#),
    ("== atas array/dict bertag: urutan sisip dict, NaN, bulat lawan pecahan", r#"package main
fn f(x) { return x }
pub fn main() {
    var a = []
    var b = []
    a = [a, 1, "x"]
    b = [b, 1, "x"]
    var c = []
    c = [c, 1, "y"]
    var d = dict::new()
    d["b"] = 1
    d["a"] = 2
    var e = dict::new()
    e["a"] = 2
    e["b"] = 1
    var p = []
    p = [p, d]
    var q = []
    q = [q, e]
    var r = []
    r = [r, 2.0, math::NAN]
    var s = []
    s = [s, 2, math::NAN]
    wok(a == b, a == c, a != c, p == q, r == s, [a] == [b], a == [[], 1, "x"])
    var m = []
    m = [m, 1.0]
    var n = []
    n = [n, 1]
    wok(m == n, m == p, [] == a)
}
"#),
    // Fase AMAN-34 (#918): `x = [.., x, ..]` memindahkan `x`; salinan yang diambil sebelumnya tidak ikut berubah.
    ("x = [.., x, ..] memindahkan x, salinan lama utuh", r#"package main
pub fn main() {
    var a = []
    a = [a]
    var c = a
    a = [a, [3]]
    wok(a, c)
    var z = []
    var simpan = []
    var i = 0
    while i < 3 {
        simpan = arr::push(simpan, z)
        z = [z, i]
        i += 1
    }
    wok(z, simpan)
    var d = [1]
    let f = || len(d)
    d = [d, [2]]
    var e = [1]
    e = [e, e]
    wok(f(), d, e)
}
"#),
    // Fase AMAN-30 (#909–#910): base64 satu ejaan per isi (kelas CVE-2026-3446), `crypto::base64url_*` ketat.
    ("base64 dan base64url: satu isi satu ejaan", r#"package main
pub fn main() {
    for _, s in ["aGk=", " aGk= ", "aGk=aGk=", "aGl=", "aG k=", "a===", "ab=c", "YQ==", "YR==", "aGk"] {
        match crypto::base64_decode_bytes(s) {
            Ok(b) => wok("ok", b),
            Err(e) => wok(e),
        }
    }
    wok(crypto::base64url_encode([251, 255]), crypto::base64url_encode("hi"))
    wok(crypto::base64url_decode_bytes("-_8"), crypto::base64url_decode("aGk"))
    for _, s in ["aGk=", "a+b", "aGl", "a"] { wok(crypto::base64url_decode_bytes(s)) }
}
"#),
    // `env::os`/`env::arch`: satu daftar nama di tiga mesin (Rust `std::env::consts`, C praprosesor di runtime LLVM).
    ("env::os dan env::arch dari daftar nama yang sama", r#"package main
pub fn main() {
    let o = env::os()
    let a = env::arch()
    wok(arr::contains(["linux", "android", "macos", "windows", "freebsd", "tak-dikenal"], o))
    wok(arr::contains(["x86_64", "aarch64", "arm", "x86", "riscv64", "tak-dikenal"], a))
    wok(o, a, typeof(o))
}
"#),
    // Fase AMAN-29 (#907): `fs::safe_join` — `..`, jalur absolut, `\`, huruf drive, kendali jadi `Err`.
    ("fs::safe_join dan jalur yang keluar dari dasarnya", r#"package main
fn main() {
    let d = fs::join(fs::temp_dir(), "jrg-gabung-aman")
    fs::mkdir(fs::join(d, "statis/css"))
    fs::write(fs::join(d, "berkas.txt"), "x")
    wok(str::replace(fs::safe_join(d, "statis/css/a.css").unwrap(), d, "<d>"))
    wok(str::replace(fs::safe_join(d, "baru/belum/ada.txt").unwrap(), d, "<d>"))
    wok(fs::safe_join(d, "").unwrap() == d, fs::safe_join(d, "statis/").is_ok())
    wok(fs::join(d, "/etc/passwd"))
    for _, r in ["../rahasia", "statis/../../x", "/etc/passwd", "a\\b", "C:/w", "a//b", "./a", "a\nb"] {
        match fs::safe_join(d, r) {
            Ok(p) => wok("lolos", p),
            Err(e) => wok(str::replace(e, d, "<d>")),
        }
    }
    match fs::safe_join(fs::join(d, "berkas.txt"), "a") {
        Ok(p) => wok("lolos", p),
        Err(e) => wok(str::replace(e, d, "<d>")),
    }
    wok(fs::safe_join(fs::join(d, "tidak-ada"), "a").is_err())
}
"#),
    // Fase AMAN-28 (#906): `semver::` — awalan `v`, nol di depan, versi telanjang dan spasi di rentang jadi `Err`.
    ("semver::parse/compare/satisfies dan bentuk ambigunya", r#"package main
fn main() {
    let v = semver::parse("1.2.3-rc.10+linux.01").unwrap()
    wok(v)
    wok(v.pra[1] + 1, typeof(v.pra[0]))
    for _, p in [["1.0.0-alpha", "1.0.0-alpha.1"], ["1.0.0-beta.11", "1.0.0-beta.2"], ["1.0.0", "1.0.0-rc.1"], ["1.0.0+a", "1.0.0+b"]] {
        wok(p[0], p[1], semver::compare(p[0], p[1]).unwrap())
    }
    for _, r in [["1.9.0", "^1.2.3"], ["2.0.0", "^1.2.3"], ["0.3.0", "^0.2.3"], ["1.2.9", "~1.2.3"], ["1.3.0", ">1.2"],
                 ["2.0.0-rc.1", "<2.0.0"], ["1.3.0-beta", "^1.2.3"], ["1.2.3-beta.2", "^1.2.3-beta.1"], ["3.1.0", "<1 || >=3, <4"]] {
        wok(r[0], r[1], semver::satisfies(r[0], r[1]).unwrap())
    }
    for _, s in ["v1.2.3", "01.2.3", "1.2", "1.2.3-01", "1.2.3-a..b", "9223372036854775808.0.0", " 1.2.3"] {
        wok(semver::parse(s).unwrap_err())
    }
    for _, r in ["1.2.3", ">= 1.2 <2", "1.2.3 - 2.0.0", "1.x", ">=1,", ""] {
        wok(semver::satisfies("1.2.3", r).unwrap_err())
    }
    wok(semver::compare("1.0.0", "v1").unwrap_err())
}"#),
    // Fase AMAN-26 (#904): `toml::` — kunci/tabel kembar, tabel diperluas, bilangan di luar 64 bit jadi `Err`.
    ("toml::parse/stringify dan bentuk ambigunya", r#"package main
fn main() {
    let d = toml::parse("judul = \"jrg\"\nversi.utama = 1\n[paket]\nnama = 'x'\nangka = [1, 2.5, 0xff, -inf]\nw = 2024-02-29T10:00:00+07:00\n[[dep]]\nn = \"a\"\n[[dep]]\nn = \"b\"\n").unwrap()
    wok(d)
    wok(d.paket.angka, d.dep[1].n, typeof(d.paket.w))
    wok(toml::stringify(d).unwrap())
    for _, s in ["a = 1\na = 2", "[t]\n[t]", "a.b = 1\n[a]", "a = {b = 1}\na.c = 2", "a = [1]\n[[a]]",
                 "a = 9223372036854775808", "a = 012", "a = 1__0", "a = \"\\x41\"", "a = \"\\uD800\"",
                 "a = 1\rb = 2", "a = 2023-02-29", "a = {b = 1,}", "a = abc"] {
        wok(toml::parse(s).unwrap_err())
    }
    wok(toml::stringify(dict { a: (1, 2) }).unwrap_err())
    wok(toml::stringify([1]).unwrap_err())
    wok(toml::stringify({"a b": "\"\n", "c": [dict { d: 1 }], "e": dict {}}).unwrap())
}"#),
    // Fase AMAN-25 (#903): kepala `Host` dicocokkan dengan daftar izin; `Host` cacat dan kembar jadi `Err`.
    ("dok::host_check", r#"package main
fn req(hs) {
    return dict { headers: hs, body: "" }
}
fn main() {
    let izin = ["contoh.id", ".jrg.web.id", "127.0.0.1:8080", "[::1]:8080"]
    for _, h in ["contoh.id", "CONTOH.id.", "a.jrg.web.id:443", "jrg.web.id", "evil-jrg.web.id", "127.0.0.1:8080",
                 "127.0.0.1:9090", "localhost", "[0::1]:8080", "0x7f.0.0.1:8080", "contoh.id@evil", "contoh.id:"] {
        wok(h, dok::host_check(req([("Host", h)]), izin))
    }
    wok(dok::host_check(req([]), izin))
    wok(dok::host_check(req([("host", "contoh.id"), ("Host", "evil")]), izin))
    wok(dok::host_check(req([("X-Forwarded-Host", "contoh.id"), ("Host", "evil")]), izin))
}
"#),
    // Fase AMAN-24 (#902): badan formulir dan multipart diurai ketat, pengalihan cuma ke jalur lokal.
    ("dok::form, dok::multipart, dok::safe_redirect", r##"package main
fn req(ct, body) {
    return dict { headers: [("content-type", ct)], body: body }
}
fn main() {
    let f = dok::form(req("application/x-www-form-urlencoded; charset=UTF-8", "a=1&b=x+y&a=%C3%A9&;c=2"))
    wok(f)
    wok(dok::form(req("text/plain", "a=1")).unwrap_err())
    wok(dok::form(req("multipart/form-data; boundary=x", "")).unwrap_err())
    wok(dok::form(req("application/x-www-form-urlencoded", "a=%zz")).is_err())
    let b = "--B\r\nContent-Disposition: form-data; name=\"teks\"\r\n\r\nhalo\r\n--B\r\nContent-Disposition: form-data; name=\"f\"; filename=\"../../etc/passwd\"\r\nContent-Type: text/plain\r\n\r\nisi\r\n--B--\r\n"
    let m = dok::multipart(req("multipart/form-data; boundary=B", b)).unwrap()
    wok(len(m), m[0]["nama"], m[0]["isi"], m[1]["berkas"], m[1]["tipe"])
    wok(dok::berkas_aman(m[1]["berkas"]), dok::berkas_aman(".."), dok::berkas_aman("NUL.txt"))
    let dua = "--B\r\nContent-Disposition: form-data; name=\"a\"; filename=\"x\"; filename*=UTF-8''y\r\n\r\nz\r\n--B--"
    wok(dok::multipart(req("multipart/form-data; boundary=B", dua)).unwrap_err())
    wok(dok::multipart(req("multipart/form-data; boundary=B", "--B\r\nContent-Disposition: form-data; name=a\r\n\r\nz")).unwrap_err())
    wok(dok::multipart(req("multipart/form-data", "")).unwrap_err())
    for _, t in ["/akun", "//evil.com", "/\\evil.com", "https://evil.com", "/..//evil", "/a/../b?x=1"] {
        let r = dok::safe_redirect(t, "/")
        wok(t, dok::header_get(r.headers, "Location"))
    }
}
"##),
    // Fase AMAN-23 (#901): satu pengurai URL; URL yang dibaca lain oleh pengurai lain jadi `Err`.
    ("url:: dan URL ambigu", r##"package main
pub fn main() {
    let u = url::parse("HTTPS://Pak:rahasia@Contoh.ID/a/./b/../c?x=1&x=2#f").unwrap()
    wok(u.skema, u.host, u.port, u.jalur, u.query, u.fragmen, u.pengguna, u.sandi)
    wok(dict::has(url::parse("http://h/").unwrap(), "query"), url::parse("http://[0::1]:8080/%2e%2E/x").unwrap().host)
    for _, r in ["g", "../g", "/g", "//q/r", "?y", "#s", "", "../../../g", "HTTPS://X"] {
        wok(url::join("http://a/b/c/d;p?q", r).unwrap())
    }
    for _, s in [" http://a/", "http://a/\tb", "http://a\\@b/", "http://a@b@c/", "http:a", "/rel", "http://0x7f.1/",
                 "http://2130706433/", "http://0177.0.0.1/", "http://%31.2.3.4/", "http://ä.id/", "http://a:0/",
                 "http://[fe80::1%25e]/", "http://a/%zz", "http:///x"] {
        wok(url::parse(s).unwrap_err())
    }
    wok(url::build(dict { skema: "https", host: "a.id", port: 8443, jalur: "/x", query: "q=1" }).unwrap())
    wok(url::build(dict { skema: "https", host: "a.id/@evil" }).unwrap_err())
    wok(url::build(dict { skema: "https", host: "a.id", jalur: "x" }).unwrap_err())
    wok(url::query_parse("?a=1&a=2;b=3&&c&d=x+y%2B%C3%A9").unwrap())
    wok(url::query_parse("a=%ff").unwrap_err())
    wok(url::query_build([["a b", "x&y=z"], ["k", "é+"]]), url::query_build(dict { b: 2, a: "1" }))
    let p = url::query_build([["a", "1&admin=1"]])
    wok(p, url::query_parse(p).unwrap())
    wok(str::contains(http::get("http://u:p@127.0.0.1:1/").unwrap_err(), "kredensial"))
}
"##),
    // Fase AMAN-20 (#895–#898): penjaga SSRF menilai alamat hasil DNS, juga ejaan angka dan IPv6 pembungkus.
    ("http::hanya_publik menolak alamat internal", r#"package main
pub fn main() {
    wok(str::contains(http::get("http://127.0.0.1:1/").unwrap_err(), "sambungan ditolak"))
    http::hanya_publik(true)
    for _, u in ["http://127.0.0.1:1/", "http://localhost:1/", "http://[::1]:1/", "http://[::ffff:169.254.169.254]/",
                 "http://169.254.169.254/latest", "http://10.0.0.1/"] {
        wok(str::contains(http::get(u).unwrap_err(), "hanya_publik(true)"))
    }
    // Ejaan angka IPv4 tak baku ditolak pengurai `url::` sebelum sampai ke DNS (Fase AMAN-23).
    wok(str::contains(http::get("http://0x7f.1:1/").unwrap_err(), "tak baku"), str::contains(http::get("http://2130706433:1/").unwrap_err(), "tak baku"))
    wok(dok::http_get("http://127.0.0.1:1/").unwrap_err())
    wok(http::unduh("http://192.168.0.1/x", "x.bin").is_err())
    http::hanya_publik(false)
    wok(str::contains(http::get("http://[::1]:1/").unwrap_err(), "sambungan ditolak"))
}
"#),
    // Fase AMAN-19 (#893–#894): token CSRF terikat sesi; sandi panjang tidak dihash tiap putaran.
    ("dok::csrf_token terikat sesi, sandi panjang", r#"package main

fn main() {
    let r = "rahasia-yang-panjangnya-32-byte!"
    let t = dok::csrf_token(r, "sesi-a")
    wok(len(str::split(t, ".")), dok::csrf_verify(t, r, "sesi-a", 60))
    wok(dok::csrf_verify(t, r, "sesi-b"), dok::csrf_verify(t + "x", r, "sesi-a"))
    wok(dok::csrf_verify(t, "rahasia-lain-yang-juga-32-byte!!", "sesi-a"), dok::csrf_verify("", r, "sesi-a"))
    wok(dok::csrf_verify("1.AAAAAAAAAAAAAAAAAAAAAA.x", r, "sesi-a"))
    let h = dok::password_hash("a" * 100000, 1000)
    wok(dok::password_verify("a" * 100000, h), dok::password_verify("a" * 99999, h))
}
"#),
    // Fase AMAN-18 (#892): `dok::render` menempatkan nilai menurut konteks HTML — URL berskema berbahaya
    // diganti penanda, nilai di `<script>`, atribut `on*` dan atribut tanpa kutip ditolak.
    ("dok::render menurut konteks HTML", r#"package main
pub fn main() {
    let d = {"u": "javascript:alert(1)", "ok": "/cari?q=a&b=1", "t": "x onclick=y"}
    wok(dok::render("<a href=\"{{u}}\">{{t}}</a><a href='{{ok}}'>", d)?)
    wok(dok::render("<a href=\"{{u}}\">", {"u": "JaVa\tScRiPt:x"})?)
    wok(dok::render("<script>let s = '{{t}}'</script>", d))
    wok(dok::render("<p title={{t}}>", d))
    wok(dok::render("<b onclick=\"f('{{t}}')\">", d))
    wok(dok::render("<textarea>{{t}}</textarea><script>{{{ t }}}</script>", d)?)
}
"#),
    // Fase AMAN-17 (#891): JWT HS256 dipatok — `alg: none` dan rahasia lain ditolak, `exp` diperiksa.
    ("jwt::sign/verify dan alg none", r#"package main
pub fn main() {
    let k = "rahasia-yang-panjangnya-32-byte!"
    let t = jwt::sign({"sub": "ani", "peran": "admin", "exp": 4102444800}, k)?
    wok(t)
    let klaim = jwt::verify(t, k)?
    wok(klaim.sub, klaim.peran, klaim.exp)
    wok(jwt::verify(t, "rahasia-lain-yang-juga-32-byte!!"))
    match jwt::verify(jwt::sign({"exp": 1000}, k)?, k) { Ok(_) => wok("BAHAYA"), Err(e) => wok(str::contains(e, "kedaluwarsa")) }
    wok(jwt::verify("eyJhbGciOiJub25lIn0.eyJhZG1pbiI6dHJ1ZX0.", k))
    wok(jwt::verify("a.b", k))
}
"#),
    // Fase AMAN-16 (#889–#890): `xml::` — DTD ditolak (XXE, ledakan entitas), atribut ganda, penulis tak bisa
    // disisipi; di LLVM juga akses bersarang atas nilai bertag (`doc.atribut.id`, `parse(..)?.atribut.t`).
    ("xml::parse/stringify/escape dan serangan klasiknya", r#"package main
fn teks_semua(n) -> string {
    var hasil = ""
    for _, a in n.anak { hasil += typeof(a) == "string" $ a : teks_semua(a) }
    return hasil
}
pub fn main() {
    let doc = xml::parse("<?xml version='1.0'?><buku id='7' judul=\"Jawa &amp; Rust\"><bab n='1'>Halo <b>dunia</b></bab><![CDATA[<mentah>]]></buku>")?
    wok(doc.nama, doc.atribut.id, doc.atribut.judul, len(doc.anak))
    wok(doc.anak[0].nama, doc.anak[0].atribut.n, teks_semua(doc))
    wok(xml::stringify(doc)?)
    wok(xml::stringify({"nama": "a", "atribut": {"t": "x\"y<z>&\n"}, "anak": ["1 < 2", {"nama": "b"}]})?)
    wok(xml::parse(xml::stringify({"nama": "a", "atribut": {"t": "x\"y\nz"}})?)?.atribut.t == "x\"y\nz")
    wok(xml::escape("<a href=\"x\">&</a>"))
    wok(xml::parse("<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><r>&x;</r>"))
    wok(xml::parse("<a>\n  <b x='1' x='2'/>\n</a>"))
    wok(xml::parse("<a><b></a>"))
    wok(xml::stringify({"nama": "a onload=alert(1)"}))
    wok(xml::stringify({"nama": "a", "atribut": {"x><script": "1"}}))
    wok(xml::stringify({"nama": "a", "anak": ["\u0001"]}))
}
"#),
    // Fase AMAN-14 (#884–#885): pembanding waktu-tetap lewat jembatan; initializer modul yang membuat lokal.
    ("crypto::compare_digest dan initializer modul berlokal", r#"package main
let KUNCI = env::get("JRG_TIDAK_PERNAH_DISET_XYZ") ?? "bawaan"
let N = match Some(3) { Some(v) => v * 2, None => 0 }
const B = { let a = 2; a * 3 }
var C = if true { let z = 1; z + 1 } else { 0 }
pub fn main() { C += 1
    wok(KUNCI, N, B, C)
    let tanda = crypto::hmac_sha256("kunci", "pesan")
    wok(crypto::compare_digest(tanda, crypto::hmac_sha256("kunci", "pesan")))
    wok(crypto::compare_digest(tanda, crypto::hmac_sha256("kunci", "pesan!")))
    wok(crypto::compare_digest("abc", "abcd"), crypto::compare_digest("", ""))
    wok(crypto::compare_digest([1, 2, 255], [1, 2, 255]), crypto::compare_digest([1, 2], "\u0001\u0002"))
}
"#),
    // Fase AMAN-12 (#879): nama non-ASCII yang sah jadi simbol native seperti nama ASCII.
    ("nama non-ASCII yang sah", r#"package main
fn luas(Δx, Δy) { Δx * Δy }
struct Titik { pub 名前: string, pub café: int }
pub fn main() {
    let имя = "Иван"
    let 日本語 = 3
    let a日b = 4
    let σ_x = 0.5
    let t = Titik { 名前: "x", café: 7 }
    wok(имя, 日本語 + a日b, σ_x, luas(2, 3), t.名前, t.café)
    wok("pаypal", len("café"), "аdmin" == "admin")
}
"#),
    // Fase AMAN-11 (#877): `len(c)` atas channel dulu ditolak LLVM ("tidak punya panjang").
    ("len atas channel", r#"package main
fn isi(c: chan int) -> int { len(c) }
pub fn main() {
    let c = make_chan(int, 4)
    wok(len(c), c.len())
    c <- 1
    c <- 2
    wok(len(c), isi(c), chan::len(c))
    let _ = <-c
    wok(len(c) + 10)
}
"#),
    // Fase AMAN-10 (#875): `kap * ukuran` dulu membungkus jadi 0 dan kiriman menulisi penunjuk kosong (SIGSEGV).
    ("channel berkapasitas raksasa tumbuh bertahap", r#"package main
pub fn main() {
    let c = make_chan(int, 2305843009213693952)
    for i in 0..5 { c <- i }
    wok(<-c, <-c, <-c)
    var i = 5
    while i < 40 { c <- i
        i = i + 1 }
    var s = 0
    for i in 0..37 { s = s + <-c }
    wok(s, chan::len(c))
    let k = make_chan(string, 3)
    k <- "a"
    k <- "b"
    wok(<-k)
    k <- "c"
    k <- "d"
    wok(<-k, <-k, <-k)
}
"#),
    // Fase REGEX-LINEAR (#841): mesin regex lama tidak pernah mundur ke dalam pengulangan.
    ("regex: mundur, malas, escape, kecocokan kosong", r#"package main
pub fn main() {
    wok(re::test("^.*x$", "abx"), re::test("a*a", "aaa"), re::test("^(a|ab)c$", "abc"), re::test("^[\\d]+0$", "100"))
    wok(re::find_all("a+?", "aaa"), re::find("\\bab\\b", "xab ab"))
    wok(re::replace_all("x*", "abc", "-"), re::split("\\s*,\\s*", "a , b,c"), re::test("a\nb", "a\nb"), re::test("a\\nb", "a\nb"))
}
"#),
    // Fase DICT-LITERAL (#839): `dict { .. }` dulu ditolak LLVM ("butuh tipe struct"), dan dict
    // kosong (`dict {}`, `dict::from_pairs([])`) ditolak walau tempatnya menyebut tipenya.
    ("literal dict berfield, sebar, dan dict kosong", r#"package main
fn f(s: string, v: int) -> int { wok(s)
 v }
pub fn main() {
    let d = dict { a: 1, b: 2 }
    wok(d, d["a"], d == {"a": 1, "b": 2})
    let g = dict { b: f("b", 20), a: f("a", 9), ...d }
    wok(g, dict { ...d })
    var e: dict<string, int> = dict {}
    e["k"] = 4
    var h: dict<string, int> = dict::from_pairs([])
    h["z"] = 1
    var m = dict {}
    m["x"] = "teks"
    m["y"] = 2
    wok(e, h, m, len(dict {}))
    let c = dict { n: 1, s: "dua" }
    wok(c, c["s"])
}
"#),
    // Fase REKURSIF-LLVM (#837): tampil, `==` dan salin tipe yang memuat dirinya lewat slice dulu
    // diturunkan sebaris tanpa ujung — `jrgc` sendiri kehabisan stack saat kompilasi.
    ("tipe berulang lewat slice: tampil, sama, salin", r#"package main
struct N { pub v: int, pub anak: []N }
enum E { Daun(int), Cabang([]E) }
fn total(n: N) -> int { var s = n.v
 for _, a in n.anak { s += total(a) }
 s }
pub fn main() {
    let n = N { v: 1, anak: [N { v: 2, anak: [] }, N { v: 3, anak: [N { v: 4, anak: [] }] }] }
    wok(n, total(n))
    var m = n
    m.anak[0].v = 99
    wok(n.anak[0].v, m.anak[0].v, n == n, n == m)
    let e = E::Cabang([E::Daun(1), E::Cabang([E::Daun(2)])])
    let f = e
    wok(e, e == f, [e] == [f], str(n), «${e}»)
}"#),
    ("spec desimal atas nilai bertag", "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2.5, \"c\": \"t\"}\n for _, k in [\"a\", \"b\"] { wok(str::format(\"{:.2}\", d[k])) } }"),
    // Fase POLIMORF: struct berbeda di satu tempat, method-nya dipilih saat jalan menurut tipe isinya —
    // default trait, method berargumen, dan method warisan.
    ("polimorfisme: method atas struct bertag", r#"package main
trait Bentuk {
    fn luas(self) -> float { return 0.0 }
    fn nama(self) -> string { return "bentuk" }
}
struct Lingkaran { r: float }
struct Persegi { s: float }
struct Hewan { pub nama_h: string = "x" }
struct Anjing: Hewan { pub ras: string = "kampung" }
impl Bentuk for Lingkaran {
    fn luas(self) -> float { return 3.0 * self.r * self.r }
    fn nama(self) -> string { return "lingkaran" }
}
impl Bentuk for Persegi {
    fn luas(self) -> float { return self.s * self.s }
}
impl Lingkaran { fn skala(self, k: float) -> float { return self.r * k } }
impl Persegi { fn skala(self, k: float) -> float { return self.s * k } }
impl Hewan { fn sapa(self) -> string { return "halo " + self.nama_h } }
fn cetak(b) { wok("-", b.nama(), b.luas(), b.skala(2.0)) }
pub fn main() {
    let xs = [Lingkaran { r: 1.0 }, Persegi { s: 3.0 }]
    for _, b in xs { cetak(b) }
    for i in xs { wok(xs[i].nama()) }
    let hs = [Anjing { nama_h: "rex" }, Hewan { nama_h: "kucing" }]
    for _, h in hs { wok(h.sapa()) }
}
"#),
    // Fase FUNGSI-JEMBATAN: fungsi JRG diserahkan ke builtin milik `jrgc` dan dipanggil balik.
    ("jembatan: closure ke ptr::update dan dok::num_solve", "package main\npub fn main() { let p = ptr::new(10)\n ptr::update(p, |n: int| n + 1)\n ptr::update(p, |n| n * 3)\n wok(ptr::get(p))\n wok(dok::num_solve(|x: float| x * x - 2.0, 0.0, 2.0)) }"),
    ("bertag: method bawaan teks dan array dipilih saat jalan", "package main\npub fn main() { let d = {\"a\": \"xyz\", \"b\": [3, 1, 2]}\n for _, k in [\"a\", \"b\"] { let v = d[k]\n wok(len(v), v.len(), v.contains(\"y\") || v.contains(1)) }\n let s = d[\"a\"]\n wok(str::len(s), s.upper()) }"),
    // Registry `jawa::` di LLVM milik runtime-nya; lewat jembatan ia dulu menjawab 0 tanpa pesan.
    ("extern C: fungsi sebagai argumen dijawab Err dari lapisan FFI", "package main\nextern \"C\" { fn atexit(f: fn()) -> i32 }\npub fn main() { let f = || 1; wok(atexit(f)) }"),
    ("ternary dan if bercabang dua separuh Result/Option tanpa anotasi", "package main\npub fn main() {\n    let r = 5 > 2 $ Result::Ok(1) : Result::Err(\"y\")\n    let s = 1 > 2 $ Result::Err(\"z\") : Result::Ok(2.5)\n    let o = 1 > 2 $ Option::Some(4) : Option::None\n    let p = 3 > 2 $ Option::None : Option::Some(\"a\")\n    wok(r, s, o, p)\n    match s { Ok(v) => wok(v * 2.0), Err(e) => wok(e + \"!\") }\n    wok(if 2 > 1 { Result::Ok([1, 2]) } else { Result::Err(5) })\n}"),
    ("dict::set dan json::set dengan nilai berjenis lain dari isinya", "package main\npub fn main() {\n    let d = {\"a\": 1}\n    wok(json::set(d, \"b\", \"teks\"), d)\n    let f = dict::set(d, \"c\", [1, 2])\n    wok(f, f[\"a\"], f[\"c\"], dict::set(d, \"z\", 9))\n}"),
    ("d[k] = v dan d.f = v berjenis lain atas dict bertipe", "package main\npub fn main() {\n    var d = {\"a\": 1}\n    d[\"b\"] = \"x\"\n    d.c = 2.5\n    wok(d, d[\"a\"] + 1)\n}"),
    ("menulis field atas nilai titipan jrgc: salinannya, bukan aslinya", "package main\npub fn main() {\n    let d = dok::json_decode(\"{\\\"a\\\": 1}\").unwrap()\n    var e = d\n    e.b = \"dua\"\n    e.a = 3\n    wok(d, e, e.b, d.a)\n}"),
    ("spread dan argumen bernama ke fungsi bernama, flatten datar, replace berpengganti bukan teks", r#"package main
fn tambah(a: int, b: int) -> int { return a + b }
fn sapa(nama: string, salam: string = "halo", tanda: string = "!") -> string { return salam + " " + nama + tanda }
fn n(x: int) -> int { wok("hitung", x); return x }
fn baca() -> []int { return [5, 6] }
pub fn main() {
    let xs = [3, 4]
    wok(tambah(...xs))
    wok(tambah(...baca()))
    wok(tambah(1, ...[2]))
    wok(terap(tambah, [1, 2]))
    var ys: []int = []
    ys = ys + [7]
    ys = ys + [8]
    wok(tambah(...ys))
    wok(sapa(...["ani"]))
    wok(sapa(...["ani", "hai"]))
    wok(sapa("budi", salam: "pagi"))
    wok(sapa("budi", tanda: "?", salam: "malam"))
    wok(sapa(nama: "cici"))
    wok(tambah(b: n(1), a: n(2)))
    wok(arr::flatten([1, 2, 3]), arr::flatten([[1], 2, [3, 4]]), arr::flatten([[1, 2], [3]]))
    wok([1, 2].flatten(), arr::flat_map([1, 2], |x| x * 10), [1, 2].flat_map(|x| [x, x]))
    wok(str::replace("a-b-c", "-", 1), "x.y".replace(".", true), "a.b".replace(".", [1, 2]))
}"#),
    ("goroutine tetap mendapat giliran selama main menunggu proses::", r#"package main
pub fn main() {
    let p = "/tmp/jrg-kor-" + str(rand::int(0, 1000000000))
    spawn { time::sleep_ms(30); fs::write(p, "x"); wok("ditulis") }
    let r = proses::keluaran("sh", ["-c", "for i in $(seq 500); do [ -f \"$1\" ] && break; sleep 0.01; done; [ -f \"$1\" ] && echo ada || echo tidak", "_", p])
    wok(r.unwrap().keluaran.trim())
    fs::delete(p)
}"#),
    ("method atas field nilai bertag", r#"package main
pub fn main() {
    let u = proses::keluaran("echo", ["hai"]).unwrap()
    wok(u.keluaran.trim(), u.keluaran.trim().len())
    let d = dok::json_decode("{\"a\": \" x \", \"b\": [1, 2]}").unwrap()
    wok(d.a.trim(), d.b.len(), dok::json_decode("{\"c\": \"q\"}").unwrap().c.upper())
}"#),
    ("d[k] = v berjenis lain atas var modul dan dict bersarang", r#"package main
var D = {"a": 1}
var E = {"n": 5}
fn isi() { D["b"] = [true] }
fn bayang() { var E = {"z": 1}; E["z"] = 2; wok(E) }
pub fn main() {
    isi()
    wok(D, D["a"])
    E["n"] = E["n"] + 1
    wok(E["n"] * 2)
    bayang()
    var e = {"p": {"q": 1}}
    e["p"]["r"] = "t"
    wok(e["p"]["q"], e["p"]["r"], len(e["p"]))
    for k, v in e["p"] { wok(k, v) }
}"#),
    ("jawa::stats dari registry biner sendiri", "package main\npub fn main() { let a = jawa::alloc(1)\n let b = jawa::array(3, 0)\n wok(jawa::stats().alive_total, jawa::stats())\n jawir::free(a)\n wok(jawa::stats().alive_total, typeof(jawa::stats()))\n jawir::free(b) }"),
    ("struct pengguna bernama Ptr/Task/DokRouter terpisah dari handle pustaka", r#"package main
struct Ptr { pub id: int, pub kind: string }
struct Task { pub id: int }
struct DokRouter { pub id: int }
pub fn main() {
    var p = Ptr { id: 7, kind: "single" }
    p.id = 8
    let asli = jawa::alloc(99)
    wok(p, p.id, typeof(p), jawa::get(asli), typeof(asli))
    jawir::free(asli)
    let t = Task { id: 3 }
    let h = Task::spawn(|| 42)
    wok(t.id, task::join(h))
    let r = DokRouter { id: 5 }
    wok(r.id, typeof(dok::router_new()))
}"#),
    ("var modul array didorong elemen berjenis lain dan penerima method dievaluasi sekali", r#"package main
var A = [1]
var H = 0
fn ubah() { A[0] = 55; return [8] }
fn buat() -> []int { H = H + 1; return [10, 20] }
pub fn main() {
    A = A + ubah()
    wok(A)
    A = A.push(ubah())
    wok(A)
    A = A.concat(ubah())
    wok(A)
    wok(buat().push(30), H)
}"#),
    ("lokal Result berganti jenis muatan Ok/Err jadi nilai bertag", r#"package main
pub fn main() {
    var r = Ok(1)
    r = Ok("s")
    wok(r, r.is_ok(), r.unwrap())
    var s = Ok(1)
    s = Ok(s)
    wok(s)
    match s { Ok(v) => wok(v), Err(e) => wok(e) }
    var e = Err(1)
    e = Err("gagal")
    wok(e, e.is_err(), e.unwrap_or(9))
}"#),
    ("kunci dict bertipe nil, array, tuple, struct, enum, dan dict", r#"package main
struct P { pub x: int }
enum E { A, B(int) }
pub fn main() {
    var d = {"awal": 0}
    d[nil] = 10
    d[[1, 2]] = 20
    d[(3, 4)] = 30
    d[P { x: 5 }] = 40
    d[E::B(6)] = 50
    wok(d[nil], d[[1, 2]], d[(3, 4)], d[P { x: 5 }], d[E::B(6)])
    wok(dict::has(d, [1, 2]), dict::get_or(d, nil, 0))
}"#),
    ("pola literal berjenis lain dan NaN mengikuti aturan ==", r#"package main
fn pecah_atas_bulat(v: int) -> string {
    match v {
        -5.0 => "min5",
        0.0 => "nol",
        2.0 => "dua",
        3 => "tiga",
        _ => "lain",
    }
}
fn bulat_atas_pecah(v: float) -> string {
    match v {
        -5 => "min5",
        0 => "nol",
        2 => "dua",
        2.5 => "setengah",
        _ => "lain",
    }
}
fn bulat_atas_f32(v: f32) -> string {
    match v {
        -5 => "min5",
        2 => "dua",
        _ => "lain",
    }
}
fn kelas(v) -> string {
    match v {
        -5.0 => "min5f",
        0 => "nol",
        2.0 => "dua",
        "a" => "str_a",
        'a' => "rune_a",
        true => "bool_t",
        nil => "nil",
        _ => "lain",
    }
}
fn bertag_campur(v) -> string {
    match v {
        -5 => "min5i",
        0.0 => "nol_f",
        2 => "dua_i",
        97 => "int_97",
        1 => "int_1",
        _ => "lain",
    }
}
fn u8_pola(x: u8) -> string {
    match x {
        -5 => "neg",
        300 => "besar",
        2.0 => "dua",
        5 => "lima",
        _ => "lain",
    }
}
pub fn main() {
    wok(pecah_atas_bulat(-5), pecah_atas_bulat(0), pecah_atas_bulat(2), pecah_atas_bulat(3), pecah_atas_bulat(5))
    wok(bulat_atas_pecah(-5.0), bulat_atas_pecah(-0.0), bulat_atas_pecah(2.0), bulat_atas_pecah(2.5), bulat_atas_pecah(math::NAN))
    wok(bulat_atas_f32(-5.0 as f32), bulat_atas_f32(2.0 as f32), bulat_atas_f32(1.5 as f32))
    wok(kelas(-5), kelas(-5.0), kelas(0.0), kelas(2), kelas(2.0), kelas("a"), kelas('a'), kelas(true), kelas(nil))
    wok(kelas(97), kelas(1), kelas(false), kelas(math::NAN))
    wok(bertag_campur(-5.0), bertag_campur(0), bertag_campur(2.0), bertag_campur('a'), bertag_campur(true), bertag_campur(math::NAN))
    wok(u8_pola(2 as u8), u8_pola(5 as u8), u8_pola(9 as u8))
    let m = (2, "x")
    wok(match m { (2.0, "x") => "ok", _ => "no" })
    let t = (1, 2)
    wok(match t { 2 => "a", _ => "b" })
    let x = 2
    wok(match x { 1.0 | 2.0 | 3.0 => "ya", _ => "tdk" })
    let nan = math::NAN
    wok(match nan { 0 | 0.0 | 2 | 2.0 => "cocok", _ => "tdk" })
}"#),
    // `const` lokal dan fungsi bernama di ekor blok: LLVM dulu membuang `const` lokal (pakai
    // `const` modul yang bernama sama), VM membaca fungsi modul dari ekor blok `for`/`if`/`{ }`.
    ("const lokal: membayangi const modul, cakupan blok, closure, fungsi bersarang, anotasi",
     "package main\nconst KOTA = \"jakarta\"\nconst K = 1\nfn tanpa_anotasi() { const K = 7; K * 2 }\nfn dalam() -> int { fn bantu() -> int { const Q = 3; Q + 1 }  bantu() }\npub fn main() {\n  const KOTA = \"bogor\"\n  wok(KOTA)\n  let n = 1\n  { const n = 2; wok(n) }\n  wok(n)\n  let f = |x| { const T = 10; x + T + K }\n  wok(f(5))\n  const S = \"luar\"\n  let g = || S\n  wok(g(), tanpa_anotasi(), dalam())\n  for i in 0..2 { const L = i * 10; wok(L) }\n  let m = match 2 { 2 => { const Q = \"lengan\"; Q }, _ => \"x\" }\n  const A: float = 2.5\n  wok(m, A + 1.0, K)\n}\n"),
    ("fungsi bernama di blok dipanggil dari ekor blok: blok polos, for, if, if bernilai, lengan match",
     "package main\nfn sapa() -> string { \"modul\" }\npub fn main() {\n  { fn a() -> int { b() + 1 }  fn b() -> int { 40 }  wok(a()) }\n  { fn sapa() -> string { \"blok\" }  wok(sapa()) }\n  for i in 0..1 { fn sapa() -> string { \"for\" } wok(sapa()) }\n  if true { fn sapa() -> string { \"if\" } wok(sapa()) }\n  while true { fn sapa() -> string { \"while\" }  wok(sapa()); break }\n  let v = if true { fn h() -> int { 9 } h() } else { 0 }\n  let m = match 2 { 2 => { fn j() -> string { \"lengan\" } j() }, _ => \"x\" }\n  wok(v, m, sapa())\n}\n"),
    ("fungsi bernama di blok polos sebagai ekor main",
     "package main\nfn sapa() -> string { \"modul\" }\npub fn main() { { fn sapa() -> string { \"blok\" }  wok(sapa()) } }\n"),
    // Pola varian atas nilai yang pasti bukan enum tidak pernah cocok — LLVM dulu menurunkan badannya dan
    // menolak "`n` tidak ditemukan"; `json::` baca atas struct dulu gagal di dalam `<lapis>`.
    ("pola varian atas dict/bulat/teks/array: lengannya dilewati, juga if let, alternatif, @",
     "fn cek(v) { if let Some(n) = v { return n } else { return 0 } }\nfn bagian(x) { match x { Some(a) | Ok(a) => a, n @ Some(_) => 1, _ => 9 } }\npub fn main() {\n  let d = {\"a\": 2}\n  wok(match d { Some(n) => n, _ => -1 })\n  let x = 5\n  wok(match x { Some(n) => n, Ok(k) => k, _ => -2 })\n  wok(match \"t\" { Some(n) => n + \"!\", _ => \"lain\" })\n  wok(match [1] { None => \"kosong\", _ => \"isi\" })\n  wok(cek(\"t\"), cek(3), bagian([1]))\n}\n"),
    ("json::keys/values/has/get atas struct membaca field-nya",
     "struct P { pub z: int, pub a: int, pub m: string = \"s\" }\npub fn main() {\n  let p = P { z: 1, a: 2 }\n  wok(json::keys(p), json::keys({\"b\": 1, \"a\": 2}))\n  wok(json::values(p), json::has(p, \"a\"), json::has(p, \"q\"), json::get(p, \"z\"), json::get(p, \"q\"))\n}\n"),
];
