// tests/bersama/paritas.rs — program yang WAJIB menjawab sama di interpreter dan `--vm`;
// dibaca `backend_parity.rs` dan `llvm_parity.rs` lewat `include!`, jadi satu tabel dua tes.

const PARITAS: &[(&str, &str)] = &[
    ("pangkat mengikat lebih erat dari minus dan tilde di kirinya (Fase KEPUTUSAN-1)",
     r#"package main
pub fn main() {
    let x = 3
    wok(-2 ** 2, -x ** 2, (-2) ** 2, -(2) ** 2, ~2 ** 2, 2 ** -1.0, -2 ** 3 ** 2)
    wok(-128 as i8, -3 * 2, - 2 ** 2 + 1, 0 - 2 ** 2, -2.0 ** 2)
}
"#),
    ("indeks negatif di builtin dihitung dari belakang seperti a[i] (Fase KEPUTUSAN-1)",
     r#"package main
pub fn main() {
    let a = [10, 20, 30]
    wok(a.get(-1), a.get(-3), a.get(-4), a.get_or(-1, 0), a.get_or(-9, 0), arr::get(a, -2), arr::get_or(a, -2, 7))
    wok(a.remove(-1), arr::remove(a, -3), a.insert(-1, 9), arr::insert(a, -3, 5), a.insert(3, 1))
    wok(a.swap(-1, 0), arr::swap(a, -2, -1), "abc".char_at(-1), str::char_at("héé", -2))
}
"#),
    ("str::substr: awal negatif dihitung dari belakang (Fase SUBSTR)",
     r#"package main
pub fn main() {
    let s = "halo dunia"
    wok(str::substr(s, -5, 3), s.substr(-5), str::substr(s, -10, 4), "[" + str::substr(s, 10) + "]", str::substr("héllo", -4, 2))
    wok(2 ** -1, 3 ** -2, typeof(2 ** -1), (-2) ** -3, 10 ** -3 * 1000.0)
}
"#),
    ("http: gagal sambung dan URL tak sah jadi Err (dulu galat saat jalan, seperti dok::http_get tidak)",
     r#"package main
pub fn main() {
    match http::get("http://127.0.0.1:1/x") { Ok(j) => wok("ok"), Err(e) => wok("err", str::contains(e, "sambungan ditolak")) }
    wok(http::get("http://127.0.0.1:abc/"))
    wok(http::post("http://127.0.0.1:1/", "x").is_err(), http::request("PUT", "http://127.0.0.1:1/").is_err())
}
"#),
    ("self milik var modul tidak ikut tulisan fungsi lain, operan kiri dibaca lebih dulu",
     r#"package main
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
    ("fs: try_mkdir/try_copy/try_rename/try_rmdir/try_delete menyebut sebabnya, db_open berkalimat Indonesia",
     r#"package main
pub fn main() {
    let d = fs::join(fs::temp_dir(), "jrg-sebab")
    fs::try_delete(d)
    wok(fs::try_mkdir(fs::join(d, "a/b")))
    wok(fs::try_mkdir("/proc/tidakbisa/x"))
    fs::write(fs::join(d, "f.txt"), "hai")
    wok(fs::try_copy(fs::join(d, "f.txt"), fs::join(d, "g.txt")))
    wok(fs::try_copy("/tidak/ada", fs::join(d, "h.txt")))
    wok(fs::try_rename(fs::join(d, "g.txt"), fs::join(d, "a/g.txt")))
    wok(fs::try_rename("/tidak/ada", fs::join(d, "z")))
    wok(fs::try_rmdir(fs::join(d, "a")))
    wok(arr::map(fs::walk(d), |p| str::replace(p, d, "")))
    wok(fs::try_delete(d), fs::exists(d))
    wok(fs::try_delete("/tidak/ada"))
    wok(dok::db_open("/proc/tidak/bisa.db"))
}
"#),
    ("crypto: array byte di hash/encode/aes_encrypt, aes_decrypt_bytes",
     r#"package main
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
    ("fs: try_read, read_base64, write_base64, append_base64 bolak-balik",
     r#"package main
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
    ("dok: byte dikirim lewat dok::binary, parameter BLOB dan UDP; udp_local_addr menyebut port pilihan OS",
     r#"package main
fn port(a) {
    let p = str::split(a, ":")
    return str::to_int(p[p.len() - 1])
}
pub fn main() {
    let u = dok::udp_bind("127.0.0.1", 0).unwrap()
    let v = dok::udp_bind("127.0.0.1", 0).unwrap()
    wok(str::starts_with(dok::udp_local_addr(u), "127.0.0.1:"))
    dok::udp_send_to(v, "127.0.0.1", port(dok::udp_local_addr(u)), [0, 255]).unwrap()
    wok(dok::udp_recv_bytes_from(u, 8).unwrap().data)
    dok::udp_close(u)
    if dok::db_tersedia() {
        let db = dok::db_open(":memory:").unwrap()
        dok::db_exec(db, "CREATE TABLE t (b BLOB)", []).unwrap()
        wok(dok::db_exec(db, "INSERT INTO t VALUES (?)", [[0, 255, 65]]))
        wok(dok::db_query(db, "SELECT b, typeof(b) AS j, length(b) AS n FROM t", []))
        wok(dok::db_exec(db, "INSERT INTO t VALUES (?)", [[0, 256]]))
        dok::db_close(db)
    }
    let l = dok::tcp_listen("127.0.0.1", 0).unwrap()
    let n = port(dok::tcp_local_addr(l))
    dok::tcp_close_listener(l)
    let r = dok::router_new()
    dok::router_get(r, "/b", |req| dok::binary([0, 255, 65]))
    dok::router_get(r, "/p", |req| dok::binary([137, 80], "image/png"))
    dok::router_get(r, "/t", |req| dok::binary([104, 195, 169]))
    dok::router_get(r, "/x", |req| dok::binary([1, 300]))
    spawn { dok::serve(r, n) }
    var siap = false
    while !siap {
        match dok::tcp_connect("127.0.0.1", n, 200) { Ok(k) => { dok::tcp_close(k)  siap = true }  Err(_) => { time::sleep_ms(20) } }
    }
    let u0 = "http://127.0.0.1:" + str(n)
    let a = dok::http_get(u0 + "/b").unwrap()
    wok(a.status, a.body, a.body_bytes, dok::header_get(a.headers, "content-type"))
    let p = dok::http_get(u0 + "/p").unwrap()
    wok(p.body_bytes, dok::header_get(p.headers, "content-type"))
    let t = dok::http_get(u0 + "/t").unwrap()
    wok(t.body, t.body_bytes)
    let x = dok::http_get(u0 + "/x").unwrap()
    wok(x.status, x.body)
    exit(0)
}
"#),
    ("dok: badan HTTP bukan teks sampai di body_bytes (body nil), %XX bukan teks dijawab 400",
     r#"package main
fn port(l) {
    let p = str::split(dok::tcp_local_addr(l), ":")
    return str::to_int(p[p.len() - 1])
}
fn minta(n, data) {
    let k = dok::tcp_connect("127.0.0.1", n, 2000).unwrap()
    dok::tcp_send(k, data).unwrap()
    let j = dok::tcp_recv_all(k).unwrap()
    dok::tcp_close(k)
    return str::split(j, "\r\n")[0]
}
pub fn main() {
    let l = dok::tcp_listen("127.0.0.1", 0).unwrap()
    let n = port(l)
    dok::tcp_close_listener(l)
    let dir = fs::join(fs::temp_dir(), "jrg-badan-" + str(n))
    fs::mkdir(dir)
    fs::write_base64(fs::join(dir, "b.bin"), "AP9B")
    let r = dok::router_new()
    dok::router_post(r, "/up", |req| { wok("server", req.body, req.body_bytes)  return dok::text("ok") })
    dok::router_get(r, "/q", |req| dok::text("q"))
    dok::router_static(r, "/f", dir)
    spawn { dok::serve(r, n) }
    var siap = false
    while !siap {
        match dok::tcp_connect("127.0.0.1", n, 200) { Ok(k) => { dok::tcp_close(k)  siap = true }  Err(_) => { time::sleep_ms(20) } }
    }
    let kepala = "POST /up HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nConnection: close\r\n\r\n"
    wok(minta(n, str::bytes(kepala) + [97, 255, 98]))
    wok(minta(n, kepala + "hé"))
    wok(minta(n, "GET /q?k=%ff HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"))
    let a = dok::http_get("http://127.0.0.1:" + str(n) + "/f/b.bin").unwrap()
    wok("klien", a.body, a.body_bytes)
    let c = http::get("http://127.0.0.1:" + str(n) + "/f/b.bin").unwrap()
    wok("klien", c["body"], c["body_bytes"])
    fs::delete(dir)
    exit(0)
}
"#),
    ("dok: tcp_recv tidak membelah karakter, byte bukan teks jadi Err, pintu byte mengambilnya",
     r#"package main
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
    ("dok: db_query menjawab BLOB sebagai array byte dan menolak TEXT bukan UTF-8",
     r#"package main
pub fn main() {
    if !dok::db_tersedia() { wok("tanpa sqlite")  return }
    let db = dok::db_open(":memory:").unwrap()
    wok(dok::db_query(db, "SELECT x'00ff41' AS b, 'é' AS t, 3 AS n"))
    wok(dok::db_query(db, "SELECT CAST(x'ff41' AS TEXT) AS rusak"))
    wok(dok::db_query(db, "SELECT CAST(CAST(x'ff41' AS TEXT) AS BLOB) AS rusak"))
    dok::db_close(db)
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
    // Fase PULIH: `A { ...a }` dulu ditolak parser di tiga mesin.
    // Fase BINER: dekode yang hasilnya bukan UTF-8 dijawab `Err` (dulu byte rusaknya jadi U+FFFD
    // tanpa suara), byte-nya lewat bentuk `_bytes`, dan `fs::try_read` membaca tanpa menghentikan.
    ("dekode byte yang bukan teks: Err, bentuk _bytes, fs::try_read", r#"package main
pub fn main() {
    wok(crypto::base64_decode("YWL/Y2Q="))
    wok(crypto::hex_decode("6162ff6364"))
    wok(crypto::base64_decode("YWJj"), crypto::hex_decode("616263"))
    wok(crypto::base64_decode_bytes("YWL/Y2Q="))
    wok(crypto::hex_decode_bytes("6162ff"), crypto::hex_decode_bytes("zz").is_err())
    let f = fs::temp_dir() + "/jrg_paritas_biner.dat"
    fs::write_base64(f, "YWL/Y2Q=")
    wok(fs::try_read(f).is_err(), fs::try_read(f + ".tidak-ada").is_err())
    fs::write(f, "halo")
    wok(fs::try_read(f))
    fs::delete(f)
}"#),
    ("literal struct yang isinya cuma basis sebar",
     "package main\nstruct A { pub x: int, pub y: int }\npub fn main() {\n let a = A { x: 1, y: 2 }\n wok(A { ...a })\n let b = A { ...a }\n wok(b.x + b.y)\n}"),
    // Fase WAJIB: default cuma untuk field yang tidak diisi, sesudah field tertulis, di tempat
    // deklarasi pertamanya dengan isi dari deklarasi terdekat. Interpreter dulu mengevaluasi SEMUA.
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
    ("varian ber-field dan struct lengkap lewat basis sebar",
     "package main\nstruct A { pub x: int, pub y: int }\nenum E { V { a: int, b: int }, W }\npub fn main() {\n let a = A { x: 1, y: 2 }\n wok(A { x: 5, ...a })\n let v = E::V { a: 1, b: 2 }\n wok(E::V { b: 9, ...v }, V { b: 5, a: 4 })\n}"),
    // ── Fase LET-POLA: pola yang COCOK tetap jalan ───────────────────────
    ("chunks dan windows: ukuran di atas 64 bit jenuh, tidak dipotong",
     "wok(arr::chunks([1, 2, 3], 18446744073709551617), arr::windows([1, 2], 18446744073709551616))\nwok([1, 2].chunks(18446744073709551616))\n"),
    ("or-pattern yang mengikat nama, di tingkat lengan dan bersarang",
     "for _, t in [(2, 5), (1, 7), (9, 9)] { wok(match t { (1, a) | (a, 5) => a, _ => 0 }) }\nwok(match (1, (2, 3)) { (x, (y, 3) | (3, y)) => x + y, _ => 0 })\nwok(match [1, 9] { [a, 9] | [9, a] => a, _ => 0 })\nwok(match (2, 5) { (1, a) | (a, 5) if a > 1 => a * 10, _ => 0 })\n"),
    ("pola: kurung tanpa koma mengelompokkan, dengan koma tuple satu elemen",
     "wok(match 3 { (3) => \"a\", _ => \"b\" }, match 3 { (1 | 3) => \"a\", _ => \"b\" }, match (3,) { (3,) => \"t\", _ => \"b\" })\n"),
    ("or-pattern pengikat: enum, struct, @, dan sisa array",
     "enum E { A(int), B(int), C }\nstruct P { pub x: int, pub y: int }\nfor _, e in [E::A(1), E::B(2), E::C] { wok(match e { E::A(n) | E::B(n) => n, E::C => -1 }) }\nlet p = P { x: 1, y: 4 }\nwok(match p { P { x: 1, y: v } | P { x: v, y: 1 } => v, _ => 0 })\nwok(match 3 { n @ (1 | 3) => n * 2, _ => 0 })\nwok(match [0, 1, 2] { [0, ...r] | [9, ...r] => r, _ => [] })\n"),
    ("pola rentang: nilai yang tidak sebanding tidak cocok, bukan galat",
     "fn h(v) { return match v { 'a'..='z' => \"huruf\", 1..=9 => \"satu\", 0.5..1.5 => \"dekat\", _ => \"lain\" } }\nfor _, v in [100, 'd', 'D', 5, 5.5, 1.0, \"s\", nil, true, [1], (1, 2), Option::Some(1), 9, 10] { wok(v, h(v)) }\n"),
    ("pola rentang: int lawan batas pecahan dibandingkan sebagai angka",
     "wok(match 5 { 1.0..=9.0 => \"f\", _ => \"lain\" }, match 9 { 1..9 => \"i\", _ => \"lain\" }, match 1 { ..1.5 => \"bawah\", _ => \"lain\" })\n"),
    ("let: literal yang cocok mengikat sisanya",
     "let (a, 5) = (1, 5)\nlet None = Option::None\nlet [x, 2] = [1, 2]\nlet 7 = 7\nwok(a, x)\n"),
    ("let: varian unit milik pemakai sebagai pola let",
     "enum E { A, B }\nlet A = E::A\nwok(\"ok\")\n"),
    ("parameter dan for: pola cocok tetap jalan",
     "fn f((a, 5)) { return a }\nwok(f((9, 5)))\nfor _, (p, 1) in [(3, 1), (4, 1)] { wok(p) }\n"),
    // ── Fase TERM: lebar tampil, mode tanpa terminal, dan `exit` ─────────
    ("term: lebar tampil menghitung kolom layar, bukan karakter",
     "wok(term::lebar(\"halo\"), term::lebar(\"日本語\"), term::lebar(\"e\\u0301\"), term::lebar(\"a\\tb\"), term::lebar(\"\"))\nwok(term::lebar_tiap(\"a日\\t😀\"), term::lebar_tiap(\"\"))\n"),
    ("term: tanpa terminal mode menjawab false dan tombol menjawab eof",
     "wok(term::mode(\"layar\", true), term::mode(\"kursor\", false), term::raw(true), term::tombol_tunggu(0), term::tombol())\n"),
    ("exit: kode 0 keluar sesudah keluaran sebelumnya, yang di bawahnya tidak jalan",
     "wok(\"sebelum\")\nexit(0)\nwok(\"tidak sampai\")\n"),
    // ── Fase LITERAL: jangkauan literal integer, negasi, dan luapan ──────
    ("literal: -2^127 adalah i128::MIN, ditulis menempel maupun berspasi",
     "wok(-170141183460469231731687303715884105728, - 170141183460469231731687303715884105728)\nlet m = -170141183460469231731687303715884105728\nwok(m + 1, typeof(m))\n"),
    ("literal: pola integer di luar 64 bit jalan di dua mesin",
     "let x = 9223372036854775808\nmatch x { 9223372036854775808 => wok(\"2^63\"), _ => wok(\"lain\") }\nmatch 0 - x - 1 { -9223372036854775809 => wok(\"-2^63-1\"), _ => wok(\"lain\") }\nlet m = 0 - 170141183460469231731687303715884105727 - 1\nmatch m { -170141183460469231731687303715884105728 => wok(\"min\"), _ => wok(\"lain\") }\nmatch 170141183460469231731687303715884105727 { 170141183460469231731687303715884105727 => wok(\"max\"), _ => wok(\"lain\") }\n"),
    ("literal: pola negatif dan jangkauan negatif",
     "for _, x in [-5, 3, -10, -2] { wok(match x { -5 => \"neg5\", -10..=-6 => \"jauh\", -4..=-1 => \"dekat\", _ => \"lain\" }) }\n"),
    ("negasi: lawan sebuah lawan, nol, dan lawan i128::MAX",
     "let a = 5\nwok(-(-a), - -a, -0, -(0 - 170141183460469231731687303715884105727))\n"),
    ("arr::product: dilipat kiri ke kanan seperti `*`",
     "wok(arr::product([170141183460469231731687303715884105727, 0.5, 4]), arr::product([2, 1.5, 3]), arr::product([]), arr::product([2, 3]), [2, 0.5].product())\n"),
    ("unit: `()` yang ditulis ADALAH unit yang dijawab fungsi tanpa nilai",
     "import \"std/iterator\"\nfn f() { }\nwok(typeof(()), f() == (), () == (), typeof(iterator::each([1], |x| x)))\n"),
    ("dict: literal berkunci bilangan bulat sama dengan d[n]",
     "let d = {1: \"a\", 20: \"b\", \"x\": 3}\nwok(d[1], d[\"20\"], d[\"x\"], dict::keys(d))\n"),
    ("kanal: has_value atas channel tertutup yang kosong menjawab false",
     "let c = Chan::new(1)\nc <- 1\nc.close()\nwok(c.has_value(), chan::has_value(c))\nwok(<-c)\nwok(c.has_value(), chan::has_value(c))\n"),
    ("select: channel tertutup yang MASIH berisi tetap bisa diterima",
     "let c = Chan::new(2)\nc <- 4\nc.close()\nselect {\n    case v := <-c: { wok(\"dapat\", v) }\n}\n"),
    // ── Bentuk namespace `chan::` (Fase CHAN-NS) ─────────────────────────
    ("chan: bentuk namespace sama dengan bentuk method",
     "let c = chan::make(2)\nchan::send(c, 7)\nwok(chan::len(c), chan::cap(c), chan::has_value(c), c.len())\nwok(chan::recv(c), chan::try_recv(c).is_err(), chan::try_send(c, 1))\nchan::close(c)\nwok(chan::is_closed(c), c.is_closed(), chan::recv(c))\n"),
    ("chan: try_recv atas channel tertutup menyebut TERTUTUP, bukan kosong",
     "let c = Chan::new(1)\nc.close()\nwok(c.try_recv(), chan::try_recv(c))\n"),
    // ── Pintu konversi (Fase KONVERSI) ────────────────────────────────────
    ("konversi: byte bilangan 8 bit tak bertanda, bukan karakter",
     "package main\npub fn main() { wok(65 as byte); wok(typeof(65 as byte)); wok((65 as byte) == 65); wok(255 as byte, 'A' as byte) }"),
    ("konversi: lebar sempit yang muat tetap jalan",
     "package main\npub fn main() { wok(255 as u8, 127 as i8, 65535 as u16, 4294967295 as u32, 9223372036854775807 as i64, 18446744073709551615 as u64) }"),
    ("konversi: minus unary diikat sebelum `as`",
     "package main\npub fn main() { var x = 5\n wok(-x as float); wok(-128 as i8); wok(-5 as string); wok(!true as int) }"),
    ("konversi: int() dan kawannya sama dengan `as`",
     "package main\npub fn main() { wok(int(5.9), int(-5.9), int(true), int(\"12\"), int('A')); wok(uint(7), float(2), rune(65), rune(1114111) as int) }"),
    ("konversi: pintu parse menerima teks dan bilangan bulat",
     "package main\npub fn main() { wok(str::to_int(\" 7 \"), parse_int(8), str::to_float(\"1.5\"), parse_float(2), \"9\".to_int(), \"2.5\".to_float()) }"),
    ("konversi: ke tipe pointer",
     "package main\npub fn main() { wok(5 as *const int); wok(0 as *mut int); wok(typeof(5 as raw<int>)) }"),
    // ── Bentuk JSON sebuah enum (Fase JSON-ENUM) ──────────────────────────
    (
        "json: enum memakai varian sebagai kunci, bukan nama representasi",
        "package main\nenum Warna { Merah }\nenum B { Kotak { p: int, l: int } }\n\
         pub fn main() {\n\
         wok(json::stringify(Option::Some(3)))\n\
         wok(json::stringify(Option::None))\n\
         wok(json::stringify(Option::Some(nil)))\n\
         wok(json::stringify(Result::Ok(1)))\n\
         wok(json::stringify(Warna::Merah))\n\
         wok(json::stringify(B::Kotak { p: 1, l: 2 }))\n\
         wok(json::stringify([Option::Some(1), Option::None]))\n\
         wok(json::stringify({\"a\": Option::Some(1)}))\n\
         wok(json::pretty({\"b\": B::Kotak { p: 1, l: 2 }}))\n}",
    ),
    // Sisi POSITIF penjaga field (Fase FIELD).
    (
        "field: yang dideklarasikan dan yang diwarisi tetap bisa ditulis",
        "package main\nstruct P { pub a: int }\nstruct Q: P { pub b: int }\n\
         pub fn main() {\n\
         var q = Q { a: 1, b: 2 }\n q.a = 9\n q.b = 8\n wok(q.a, q.b)\n\
         var d = {\"x\": 1}\n d.y = 2\n wok(d)\n}",
    ),
    // Sisi POSITIF penjaga pola `str::` (Fase POLA-STR).
    (
        "str:: pola: rune sah, str(x) jalan, teks yang dirender tidak dijaga",
        "package main\npub fn main() {\n\
         wok(str::contains(\"abc\", str::char_at(\"abc\", 1)))\n\
         wok(str::contains(\"a[1, 2]b\", str([1, 2])))\n\
         wok(str::replace(\"x1y\", \"1\", 9))\n\
         wok(str::pad_left(\"42\", 5, 0))\n\
         wok(str::split(\"a,b\", \",\"))\n}",
    ),
    // ── Channel: menutup BUKAN membuang ──────────────────────────────── Atas
    // biner sebelum Fase KANAL ini menjawab "diterima 1 dari 10, jumlah 0": …
    ("kanal: tutup tidak membuang isi yang sudah terkirim",
     "package main\npub fn main() {\n let c = Chan::new(100)\n spawn { for i in range(0, 10) { c <- i }  c.close() }\n var j = 0\n var n = 0\n while !c.is_closed() || c.len() > 0 {\n  if c.has_value() {\n   let r = c.try_recv()\n   if r.is_ok() { j = j + r.unwrap()  n = n + 1 }\n  }\n }\n wok(n, j)\n}"),
    ("kanal: len dan is_closed",
     "package main\npub fn main() {\n let c = Chan::new(2)\n c <- 1\n wok(c.len(), c.is_closed())\n c.close()\n wok(c.len(), c.is_closed())\n wok(<-c)\n wok(c.len())\n}"),
    ("kanal: has_value tidak mengambil",
     "package main\npub fn main() {\n let c = Chan::new(2)\n c <- 9\n wok(c.has_value(), c.has_value(), c.len())\n wok(<-c)\n}"),

    // ── `proses::`/`fs::` yang baru — diuji lewat PROPERTI, karena jalur
    // dan pid berbeda tiap mesin. Sebuah tes yang mengunci nilainya akan
    // merah di mesin orang lain tanpa ada yang rusak.
    ("proses::pid positif",
     "package main\npub fn main() { wok(proses::pid() > 0) }"),
    ("fs::temp_dir tidak kosong",
     "package main\npub fn main() { wok(len(fs::temp_dir()) > 0) }"),
    ("fs::abs meratakan `..`",
     "package main\npub fn main() { wok(str::ends_with(fs::abs(\"x/../y\"), \"y\"))\n  wok(str::contains(fs::abs(\"x/../y\"), \"x\")) }"),
    ("fs::is_symlink atas jalur yang tidak ada",
     "package main\npub fn main() { wok(fs::is_symlink(\"/pasti/tidak/ada/xyz\")) }"),
    // ── Rentang acak: rentang `i128` penuh pernah membagi dengan nol ────────
    ("acak: rand::int rentang i128 penuh",
     "package main\npub fn main() { wok(typeof(rand::int(-170141183460469231731687303715884105728, 170141183460469231731687303715884105727))) }"),
    ("acak: crypto::random_int rentang i128 penuh",
     "package main\npub fn main() { wok(typeof(crypto::random_int(-170141183460469231731687303715884105728, 170141183460469231731687303715884105727))) }"),
    ("acak: rentang satu nilai",
     "package main\npub fn main() { wok(rand::int(5, 5))\n  wok(crypto::random_int(-3, -3)) }"),
    ("acak: random_hex panjangnya dua kali byte",
     "package main\npub fn main() { wok(len(crypto::random_hex(4))) }"),
    // ── Sebar di literal array (`[...a, 3]`) ────────────────────────────
    // Sebar sudah ada di literal struct dan di argumen pemanggilan; literal
    // array adalah posisi ketiga, dan justru yang paling sering ditulis.
    ("sebar array: di tengah",
     "package main\npub fn main() { let a = [2,3]\n  wok([1, ...a, 4]) }"),
    ("sebar array: sendirian menyalin",
     "package main\npub fn main() { let a = [1,2]\n  var b = [...a]\n  b[0] = 9\n  wok(a, b) }"),
    ("sebar array: dua sebar",
     "package main\npub fn main() { let a = [1]\n  let b = [2,3]\n  wok([...a, ...b]) }"),
    ("sebar array: sebar dari kosong",
     "package main\npub fn main() { let a = []\n  wok([...a, 1]) }"),
    ("sebar array: tanpa sebar tidak berubah",
     "package main\npub fn main() { wok([1,2,3])\n  wok([0; 3]) }"),
    // ── Anotasi tipe kembalian closure ──────────────────────────────────
    ("closure: anotasi return diterima",
     "package main\npub fn main() { let f = |x| -> int { return x * 2 }\n  wok(f(3)) }"),
    ("closure: anotasi return tidak ditegakkan",
     "package main\npub fn main() { let f = |x| -> int { return \"teks\" }\n  wok(f(1)) }"),
    // ── Bentuk path atas method ber-`self` ──────────────────────────────
    ("path method: penerima benar",
     "package main\nstruct P { pub n: int }\nimpl P { pub fn tambah(self, k) -> int { return self.n + k } }\npub fn main() { wok(P::tambah(P{n:1}, 4)) }"),
    ("path method: lewat warisan",
     "package main\nstruct Hewan { pub n: int }\nimpl Hewan { pub fn suara(self) -> string { return \"guk\" } }\nstruct Anjing: Hewan { }\npub fn main() { wok(Hewan::suara(Anjing{n:1}))\n  wok(Anjing::suara(Anjing{n:1})) }"),
    ("path method: sebagai NILAI",
     "package main\nstruct P { pub n: int }\nimpl P { pub fn dua(self) -> int { return self.n * 2 } }\npub fn main() { let f = P::dua\n  wok(f(P{n:3}))\n  wok(arr::map([P{n:1}, P{n:2}], P::dua)) }"),
    ("path method: fungsi terasosiasi tetap jalan",
     "package main\nstruct P { pub n: int }\nimpl P { pub fn nol() -> P { return P{n:0} } }\npub fn main() { wok(P::nol()) }"),
    // Sisi POSITIF penjaga `??=`: bentuk-bentuk yang harus tetap sah.
    ("koalesensi biasa tidak tersentuh penjaga `??=`",
     "package main\npub fn main() {\n  \
      let a = Option::None\n  \
      let b = Option::Some(7)\n  \
      wok(a ?? 1)\n  \
      wok(b ?? 1)\n  \
      wok(a ?? b ?? 2)\n  \
      wok((a ?? 3) == 3)\n\
      }"),
    // ── `str::format` dengan jumlah desimal tetap ────────────────────────
    ("format: dua desimal",        "package main\npub fn main() { wok(str::format(\"{:.2}\", 3.14159)) }"),
    ("format: pembulatan naik",    "package main\npub fn main() { wok(str::format(\"{:.1}\", 0.25)) }"),
    ("format: nol desimal",        "package main\npub fn main() { wok(str::format(\"{:.0}\", 2.7)) }"),
    // `int` diberi desimal tanpa lewat `f64` — sebuah bilangan di atas
    // 2^53 akan berubah kalau dikonversi dulu.
    ("format: int dapat desimal",  "package main\npub fn main() { wok(str::format(\"{:.2}\", 5)) }"),
    ("format: int besar utuh",     "package main\npub fn main() { wok(str::format(\"{:.2}\", 9007199254740993)) }"),
    ("format: campur dengan {}",   "package main\npub fn main() { wok(str::format(\"{} = {:.3}\", \"pi\", 3.14159265)) }"),
    // Bukan-spec tetap teks biasa, seperti sebelumnya.
    ("format: {x} tetap teks",     "package main\npub fn main() { wok(str::format(\"{x} {}\", 1)) }"),
    ("format: persen tetap teks",  "package main\npub fn main() { wok(str::format(\"naik 100% jadi {}\", 2)) }"),
    // Nilai tak hingga tidak punya desimal — dirender lewat jalur tampil yang
    // sama dengan `wok`, supaya satu nilai tidak punya dua nama.
    ("format: inf lewat spec",     "package main\npub fn main() { wok(str::format(\"{:.2}\", 1.0/0.0)) }"),

    // ── Nama ganda: sisi yang harus tetap DITERIMA ───────────────────────
    ("nama: struct dan fn boleh senama",
     "package main\nstruct P { pub x: int }\nfn P() { return 9 }\npub fn main() { wok(P()); wok(P{x:1}.x) }"),
    // Nama yang sama di SCOPE yang berbeda tidak bertabrakan: yang diperiksa
    // cuma tingkat atas berkas.
    ("nama: lokal boleh menutupi global",
     "package main\nconst K = 1\nfn f() { let K = 2; return K }\npub fn main() { wok(K); wok(f()) }"),
    ("nama: fn bersarang senama di dua fungsi",
     "package main\nfn a() { fn bantu() { return 1 } return bantu() }\nfn b() { fn bantu() { return 2 } return bantu() }\npub fn main() { wok(a()); wok(b()) }"),
    // Method boleh senama dengan fungsi tingkat atas — ruang yang berbeda lagi.
    ("nama: method senama dengan fn tingkat atas",
     "package main\nstruct T { pub v: int }\nimpl T { fn nilai(self) { return self.v } }\nfn nilai() { return 7 }\npub fn main() { wok(nilai()); wok(T{v:3}.nilai()) }"),

    // ── `dok::` di KEDUA backend ─────────────────────────────────────────
    ("dok: json encode",      "package main\npub fn main() { wok(dok::json_encode({\"a\": 1, \"b\": [1,2]})) }"),
    ("dok: mat identitas",    "package main\npub fn main() { let i = dok::mat_identity(3)\n wok(dok::mat_get(i, 1, 1))\n wok(dok::mat_get(i, 0, 1)) }"),
    ("dok: mat kali + det",   "package main\npub fn main() { let a = dok::mat_identity(2)\n wok(dok::mat_det(dok::mat_mul(a, a))) }"),
    ("dok: vec dot & norm",   "package main\npub fn main() { wok(dok::vec_dot([1.0,2.0],[3.0,4.0]))\n wok(dok::vec_norm([3.0,4.0])) }"),
    ("dok: stat linreg",      "package main\npub fn main() { wok(dok::stat_linreg([1.0,2.0,3.0],[2.0,4.0,6.0])) }"),
    ("dok: num prima",        "package main\npub fn main() { wok(dok::num_is_prime(97))\n wok(dok::num_prime_factors(84)) }"),
    ("dok: num base",         "package main\npub fn main() { wok(dok::num_to_base(255, 16)) }"),
    ("dok: url encode/decode","package main\npub fn main() { let e = dok::url_encode(\"a b&c\")\n wok(e)\n wok(dok::url_decode(e)) }"),
    ("dok: log berjenjang",   "package main\npub fn main() { dok::log_set_name(\"uji\")\n dok::log_info(\"halo\")\n wok(\"ok\") }"),
    // Handle `dok::` berbentuk struct ber-id — sebuah NOMOR, bukan penunjuk
    // — jadi ia menyeberang tanpa perlakuan khusus dan kedua backend
    // menunjuk entri registri yang sama.
    ("dok: handle router",    "package main\npub fn main() { let r = dok::router_new()\n wok(typeof(r)) }"),
    ("dok: handle listener",  "package main\npub fn main() { match dok::tcp_listen(\"127.0.0.1\", 0) { Ok(l) => { wok(typeof(l))\n dok::tcp_close_listener(l) }, Err(e) => wok(e) } }"),
    // `num_solve` memanggil BALIK fungsi JRG, jadi ia satu-satunya `dok::`
    // yang tidak bisa lewat jembatan biasa: VM mencegatnya lewat alamat
    // fungsinya, lalu menjalankan implementasi yang sama.
    ("dok: num_solve closure","package main\npub fn main() { wok(dok::num_solve(|x| x*x - 2.0, 1.0, 2.0)) }"),
    ("dok: num_solve bernama","package main\nfn f(x) { return x*x - 2.0 }\npub fn main() { wok(dok::num_solve(f, 1.0, 2.0)) }"),
    ("dok: num_solve tak konvergen","package main\npub fn main() { wok(dok::num_solve(|x| 1.0, 1.0, 2.0)) }"),
    // Bentuk TAK LANGSUNG: itu alasan pencegatannya lewat alamat dan bukan
    // lewat nama di compiler.
    ("dok: num_solve tak langsung","package main\npub fn main() { let g = dok::num_solve\n wok(g(|x| x - 3.0, 1.0, 5.0)) }"),

    // ── `Future::`/`Task::` di KEDUA backend ─────────────────────────────
    ("future: new + await",      "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { let f = Future::new(|| 7)\n wok(future::await(f)) }"),
    ("future: fungsi pengguna",  "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { wok(future::await(Future::new(kerja))) }"),
    ("future: dengan argumen",   "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { wok(future::await(Future::new(tambah, 3, 4))) }"),
    ("future: dua paralel",      "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { let a = Future::new(|| 1)\n let b = Future::new(|| 2)\n wok(future::await(a) + future::await(b)) }"),
    ("future: typeof",           "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { let f = Future::new(|| 1)\n wok(typeof(f))\n wok(future::await(f)) }"),
    // `is_done` TIDAK boleh mengonsumsi hasilnya — kalau ia mengambil dan
    // tidak mengembalikan, `await` sesudahnya menggantung selamanya, dan
    // gejalanya adalah program yang diam bukan program yang gagal.
    ("future: is_done tidak mengonsumsi", "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { let f = Future::new(|| 5)\n future::is_done(f)\n wok(future::await(f)) }"),
    ("task: spawn + join",       "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { let t = Task::spawn(|| 9)\n wok(task::join(t)) }"),
    ("task: fungsi pengguna",    "package main\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\npub fn main() { wok(task::join(Task::spawn(kerja))) }"),

    // Task/Future di VM dulu berjalan di `Vm::new()` KOSONG.
    ("pointer: perkalian tetap jalan",  "package main\npub fn main() { let a = 3\n let b = 4\n wok(a * b) }"),
    ("pointer: bitwise and tetap jalan","package main\npub fn main() { wok(12 & 10) }"),
    ("pointer: && tetap jalan",         "package main\npub fn main() { wok(true && false) }"),
    ("pointer: closure pakai *",        "package main\npub fn main() { let f = |x| x * 2\n wok(f(5)) }"),
    ("pointer: string * n",             "package main\npub fn main() { wok(\"ab\" * 3) }"),
    // Yang SUNGGUH berbagi, dan itu jawaban yang ditunjuk pesan penolakannya.
    ("ptr: berbagi lintas nama",        "package main\npub fn main() { let p = ptr::new(1)\n let q = p\n ptr::set(q, 7)\n wok(ptr::get(p)) }"),
    ("ptr: berbagi lintas fungsi",      "package main\nfn naik(r) { ptr::set(r, ptr::get(r) + 1) }\npub fn main() { let p = ptr::new(41)\n naik(p)\n wok(ptr::get(p)) }"),

    // ── `let` di TINGKAT MODUL ──────────────────────────────────────────
    ("modul: let dasar",            "package main\nlet K = 5\npub fn main() { wok(K) }"),
    ("modul: let dibaca fungsi",    "package main\nlet K = 5\nfn baca() -> int { return K }\npub fn main() { wok(baca()) }"),
    // Binding tingkat-modul dihoist: fungsi yang dideklarasikan LEBIH DULU
    // tetap melihatnya, sama seperti `var`.
    ("modul: let dipakai sebelum barisnya", "package main\nfn baca() -> int { return K }\nlet K = 7\npub fn main() { wok(baca()) }"),
    ("modul: let beranotasi tipe",  "package main\nlet K: int = 5\npub fn main() { wok(K) }"),
    ("modul: let dari let lain",    "package main\nlet A = 2\nlet B = A * 3\npub fn main() { wok(B) }"),
    ("modul: pub let",              "package main\npub let K = 5\npub fn main() { wok(K) }"),
    ("modul: let memanggil fungsi", "package main\nfn dua() -> int { return 2 }\nlet K = dua() * 3\npub fn main() { wok(K) }"),
    // `let mut` berarti bisa diubah, jadi ia diturunkan ke bentuk `var`.
    ("modul: let mut bisa diubah",  "package main\nlet mut G = 5\npub fn main() { G = 6\n wok(G) }"),

    // ── str::format: SATU definisi ──────────────────────────────────────
    ("format: dasar",              "package main\npub fn main() { wok(str::format(\"{} dan {}\", \"a\", \"b\")) }"),
    // Nilai yang KEBETULAN berisi `{}`.
    ("format: nilai berisi {}",    "package main\npub fn main() { wok(str::format(\"{}\", \"{}\")) }"),
    ("format: nilai {} di tengah", "package main\npub fn main() { wok(str::format(\"a{}b\", \"{}\")) }"),
    // Kurung kurawal yang BUKAN placeholder tetap teks biasa — sebuah string
    // memang boleh berisi `{`.
    ("format: brace bukan placeholder", "package main\npub fn main() { wok(str::format(\"{1,2} {}\", 9)) }"),
    ("format: brace tak berpenutup",    "package main\npub fn main() { wok(str::format(\"a { b\")) }"),
    ("format: persen lewat apa adanya",  "package main\npub fn main() { wok(str::format(\"100% dan {}\", 5)) }"),
    ("format: non-ASCII",          "package main\npub fn main() { wok(str::format(\"«ünïcödé» {}\", \"ok\")) }"),
    ("format: tanpa placeholder",  "package main\npub fn main() { wok(str::format(\"apa adanya\")) }"),

    // ── Sebuah tugas menyerahkan hasilnya SEKALI, dan bilang begitu ─────
    ("tugas: try_get belum siap lalu await", "package main\npub fn main() { let a = Future::new(|| { time::sleep_ms(400)\n  return 7 })\n wok(future::try_get(a))\n wok(future::await(a)) }"),
    ("tugas: timeout habis lalu await",      "package main\npub fn main() { let a = Future::new(|| { time::sleep_ms(500)\n  return 7 })\n wok(future::timeout(a, 50))\n wok(future::await(a)) }"),
    // Future-nya sengaja LAMBAT, dan itu bukan hiasan.
    ("hof ns: count_if",   "package main\npub fn main() { wok(arr::count_if([1,2,2,3], |x| x == 2)) }"),
    // `_` membuang nilainya, tapi sisi kanannya tetap dievaluasi — dulu `_ = f()` ditolak
    // acuan ("`_` belum dideklarasikan") dan diterima `--vm`.
    ("pembuang: `_ = f()`, `a, _ = …`, `var _` dan `let _` mengevaluasi lalu membuang",
     "fn f() { wok(\"f\"); return 5 }\n_ = f()\nvar a = 1\na, _ = 7, f()\nwok(a)\n\
      var _ = f()\nlet _ = f()\nfor _, x in [3] { wok(x) }\nwok(arr::map([1, 2], |_| 0))\n"),
    ("sasaran: jalur tulis yang sah tetap diterima parser",
     "struct P { pub x: int = 0 }\nimpl P { fn set(self) { self.x = 4; return self } }\n\
      var a = [[1]]\nvar d = {\"k\": [1]}\na[0][0] = 5\nd[\"k\"][0] = 6\nd.k[0] += 1\n\
      var p = P {}\np = p.set()\nwok(a, d, p.x)\n"),
    // HOF bawaan sebagai NILAI — dulu `--vm` menjawab "`arr::map` tidak ditemukan".
    ("hof: sebagai nilai lewat variabel, terap, fungsi pemakai, dan ::",
     "fn pakai(h, xs) { return h(xs, |x| x * 10) }\n\
      wok(pakai(arr::map, [1, 2]), pakai(arr::filter, [1, 0, 2]))\n\
      wok(terap(arr::fold, [[1, 2, 3], 0, |a, b| a + b]))\n\
      let g = ::arr::map\nwok(g([1], |x| x + 1), typeof(g), g)\n\
      let c = arr::count\nwok(c([1, 2, 3], |x| x > 1), arr::count([1, 2, 3], |x| x > 1))\n\
      let u = ptr::update\nlet p = ptr::new(5)\nwok(u(p, |x| x + 1), ptr::get(p))\n"),
    // ── Fase LOGIKA: operator menjawab sama di dua mesin ─────────────────
    ("logika: && dan || selalu menjawab bool, dan tetap hubung-singkat",
     "wok(1 && 2, 0 || \"x\", nil && true, false || 3, 0 && 5, 5 || 9)\n\
      wok(typeof(1 && 2), (1 && 2) == true, [] || 1, [1] && \"a\")\n\
      let x = 7 || 0\nwok(x, typeof(x))\n\
      fn f() { wok(\"f\"); return 0 }\nwok(f() || f(), true || f(), false && f())\n"),
    ("logika: rune selalu truthy",
     "let r = 0 as rune\nif r { wok(\"ya\") } else { wok(\"tidak\") }\n\
      wok(r $ 1 : 2, r && true, 'a' || false)\n"),
    ("operator: ~ ! dan - lewat method milik pemakai",
     "struct P { pub n: int }\n\
      impl P { fn bit_not(self) -> int { return 0 - self.n }\n\
      fn not(self) -> string { return \"bukan\" }\n\
      fn neg(self) -> P { return P { n: self.n * 2 } } }\n\
      let p = P { n: 3 }\nwok(~p, !p, (-p).n)\n"),
    ("banding: agregat memakai urutan pengurutan, bool ikut",
     "wok([true] < [false], [false] < [true], [true] <= [true], (true, 1) < (true, 2))\n\
      wok([1, 2] > [1], [] >= [], [1] > [1, 0], [2] > [1.5], [\"a\"] < [\"b\"])\n"),
    ("kesamaan: penyimpanan yang sama tetap dibandingkan elemen per elemen",
     "let x = [math::NAN]\nlet y = x\nwok(x == x, x != x, x == y)\n\
      wok(arr::contains([x], x), arr::index_of([x], x), len(arr::unique([x, x])))\n\
      let d = {\"a\": math::NAN}\nstruct P { pub v: float }\nlet p = P { v: math::NAN }\n\
      let o = Option::Some(math::NAN)\nwok(d == d, p == p, o == o)\n\
      let a = [1, 2]\nlet b = a\nwok(a == b, a == a, a <= b, a < b)\n"),
    ("pangkat: eksponen di luar i32 tidak dipotong",
     "wok(2 ** -4294967297, 2 ** -4294967296, 2.0 ** 4294967297, 2.0 ** -4294967297)\n\
      wok(1 ** 4294967296, (-1) ** 4294967297, (-1) ** 4294967296, 0 ** 4294967296)\n\
      wok(2 ** -1, 2.0 ** 3, 2 ** 10, 3 ** -2)\n"),
    ("callback: fungsi bernama tanpa nilai kembalian tidak menggeser lokal pemanggil",
     "package main\nvar total = 0\nfn diam(x) { total = total + x }\nfn kosong() { let y = 1 }\n\
      fn f() { let a = 1\n let b = 2\n arr::each([1, 2], diam)\n let h = [1, 2].map(diam)\n\
      let t = terap(kosong, [])\n defer kosong()\n wok(a, b, h, t, total) }\npub fn main() { f() }\n"),
    ("hof ns: max_by",     "package main\npub fn main() { wok(arr::max_by([\"aa\",\"b\",\"cccc\"], |s| str::len(s))) }"),
    ("hof ns: min_by",     "package main\npub fn main() { wok(arr::min_by([\"aa\",\"b\",\"cccc\"], |s| str::len(s))) }"),
    ("hof ns: sort_by_key", "package main\npub fn main() { wok(arr::sort_by_key([\"ccc\",\"a\",\"bb\"], |s| str::len(s))) }"),
    ("hof ns: fold, nilai awal di depan",
     "package main\npub fn main() { wok(arr::fold([1,2,3], 10, |a, b| a + b))\n wok([1,2,3].fold(10, |a, b| a + b))\n wok([].fold(5, |a, b| a + b)) }"),
    // Bentuk method dan bentuk namespace harus menjawab hal yang SAMA —
    // kalau tidak, satu bahasa punya dua arti untuk satu operasi.
    ("hof ns: setara dengan bentuk method",
     "package main\npub fn main() {\n let a = [5,1,9,3]\n \
      wok(arr::max_by(a, |x| x) == a.max_by(|x| x))\n \
      wok(arr::min_by(a, |x| x) == a.min_by(|x| x))\n \
      wok(arr::sort_by_key(a, |x| x) == a.sort_by_key(|x| x))\n \
      wok(arr::count_if(a, |x| x > 2) == a.count_if(|x| x > 2)) }"),
    // ── Urutan leksikografis atas array dan tuple ────────────────────────
    ("urut: array leksikografis",
     "package main\npub fn main() {\n \
      wok([1,2] < [1,3], [1,3] < [1,2])\n \
      wok([1,2] <= [1,2], [2] > [1])\n \
      wok([1,2] < [1], [1] < [1,2]) }"),
    ("urut: tuple leksikografis",
     "package main\npub fn main() {\n \
      wok((1,2) < (1,3), (1,\"a\") < (1,\"b\"))\n \
      wok((2,1) >= (2,1)) }"),
    ("urut: bersarang",
     "package main\npub fn main() { wok([[1,2],[0]] > [[1,1],[9]]) }"),
    // Yang MEMAKAI urutan itu harus ikut bekerja — dan inilah enam asimetri
    // yang tertutup.
    ("urut: sort/min/max atas agregat",
     "package main\npub fn main() {\n \
      wok(arr::sort([[2],[1]]))\n \
      wok(arr::sort([(2,1),(1,2)]))\n \
      wok(arr::min([[2],[1]]), arr::max([[2],[1]])) }"),
    // ── Goroutine ditunggu sebelum program berakhir ──────────────────────
    ("goroutine: kerjanya selesai sebelum program berakhir",
     "package main\npub fn main() {\n \
      let ch = make_chan(int)\n \
      spawn { ch <- 1\n \
              var i = 0\n \
              while i < 200000 { i = i + 1 }\n \
              wok(\"goroutine selesai\") }\n \
      wok(<-ch) }"),
    // Tanpa channel sama sekali: goroutine yang cuma mencetak tetap tercetak.
    ("goroutine: tanpa channel pun ditunggu",
     "package main\npub fn main() {\n \
      spawn { var i = 0\n \
              while i < 200000 { i = i + 1 }\n \
              wok(\"selesai tanpa channel\") }\n \
      wok(\"utama\") }"),
    ("tugas: is_done berkali lalu await",    "package main\npub fn main() { let a = Future::new(|| { time::sleep_ms(400)\n  return 7 })\n wok(future::is_done(a))\n wok(future::is_done(a))\n wok(future::await(a)) }"),
    ("tugas: race, yang kalah masih utuh",   "package main\npub fn main() { let a = Future::new(|| { time::sleep_ms(600)\n  return 1 })\n let b = Future::new(|| 2)\n wok(future::race([a, b]))\n wok(future::await(a)) }"),
    // `is_done` SESUDAH diserahkan menjawab `true`. Dulu `false` — jawaban
    // yang salah, bukan sekadar kurang: `while !is_done(f) { … }` berputar
    // selamanya justru karena kerjanya sudah selesai.
    ("tugas: is_done sesudah await",         "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::await(a))\n wok(future::is_done(a)) }"),

    // ── future::all / race / timeout / try_get, di KEDUA backend ────────
    ("future: all",             "package main\npub fn main() { let a = Future::new(|| 1)\n let b = Future::new(|| 2)\n wok(future::all([a, b])) }"),
    ("future: all kosong",      "package main\npub fn main() { wok(future::all([])) }"),
    ("future: all atas Task",   "package main\npub fn main() { let t = Task::spawn(|| 3)\n wok(future::all([t])) }"),
    ("future: race",            "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::race([a])) }"),
    // `Option`, bukan `nil`. Dua kasus di bawah ini yang mengunci alasannya:
    // sebelumnya keduanya menjawab `nil`, jadi "hasilnya memang nil" dan
    // "belum siap"/"lewat waktu" tidak bisa dibedakan sama sekali.
    ("future: try_get siap",    "package main\npub fn main() { let a = Future::new(|| 7)\n time::sleep_ms(200)\n wok(future::try_get(a)) }"),
    ("future: try_get hasil nil","package main\npub fn main() { let a = Future::new(|| nil)\n time::sleep_ms(200)\n wok(future::try_get(a)) }"),
    ("future: try_get belum siap","package main\npub fn main() { let a = Future::new(|| { time::sleep_ms(4000)\n  return 7 })\n wok(future::try_get(a)) }"),
    ("future: timeout dapat",   "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::timeout(a, 2000)) }"),
    ("future: timeout hasil nil","package main\npub fn main() { let a = Future::new(|| nil)\n wok(future::timeout(a, 2000)) }"),
    ("future: timeout habis",   "package main\npub fn main() { let a = Future::new(|| { time::sleep_ms(4000)\n  return 7 })\n wok(future::timeout(a, 100)) }"),
    ("future: try_get dipakai match", "package main\npub fn main() { let a = Future::new(|| 7)\n time::sleep_ms(200)\n match future::try_get(a) { Some(v) => wok(\"dapat\", v), None => wok(\"belum\") } }"),
    // `is_done` TIDAK mengonsumsi — sifat dari fase ASYNC yang harus tetap
    // utuh sesudah `try_get` (yang MEMANG mengonsumsi) berdiri di sebelahnya.
    ("future: is_done lalu await", "package main\npub fn main() { let a = Future::new(|| 7)\n time::sleep_ms(200)\n wok(future::is_done(a))\n wok(future::await(a)) }"),

    // ── Index: dict berkunci apa pun ────────────────────────────────────
    ("index: dict kunci int",       "package main\npub fn main() { let d = dict::from_pairs([[0, \"x\"]])\n wok(d[0])\n wok(dict::get(d, 0))\n wok(d[\"0\"]) }"),
    ("index: dict kunci array",     "package main\npub fn main() { let d = dict::from_pairs([[[1,2], \"x\"]])\n wok(d[[1,2]]) }"),
    ("index: dict kunci nil",       "package main\npub fn main() { let d = dict::from_pairs([[nil, \"x\"]])\n wok(d[nil]) }"),
    // Jalur TULIS punya dua langkahnya sendiri (antara dan terakhir), dan
    // keduanya dulu menuntut kunci string.
    ("index: tulis dict kunci int", "package main\npub fn main() { var d = dict::from_pairs([[0, \"x\"]])\n d[0] = \"y\"\n wok(dict::get(d, 0)) }"),
    ("index: tulis dict bersarang", "package main\npub fn main() { var d = dict::from_pairs([[0, dict::from_pairs([[\"q\", 1]])]])\n d[0][\"q\"] = 5\n wok(d[0][\"q\"]) }"),
    ("index: struct via string",    "package main\nstruct P { x: int, y: int }\npub fn main() { let p = P{x:1,y:2}\n wok(p[\"x\"]) }"),

    // `<-c` sebagai PERNYATAAN, didahului pernyataan lain.
    ("chan: <-c sbg pernyataan sesudah pernyataan lain", "package main\npub fn main() { let c = make_chan(bool)\n spawn { c <- true }\n wok(\"a\")\n <-c\n wok(\"b\") }"),
    ("chan: <-c sesudah tutup kurung",                   "package main\npub fn main() { let c = make_chan(bool)\n spawn { c <- true }\n if true { wok(\"a\") }\n <-c\n wok(\"b\") }"),
    ("chan: <-c sbg pernyataan pertama",                 "package main\npub fn main() { let c = make_chan(bool)\n spawn { c <- true }\n <-c\n wok(\"b\") }"),
    // Sifat dari fase LANJUTAN yang harus tetap utuh: `<` TUNGGAL boleh
    // memimpin baris lanjutan. Perbaikan di atas hanya mengecualikan `<-`.
    ("chan: `<` tunggal masih menyambung baris",         "package main\npub fn main() { let a = 1\n let x = a\n   < 5\n wok(x) }"),

    // PRESEDENSI `<-`.
    ("chan: <-c + <-c",             "package main\npub fn main() { let c = make_chan(int, 2)\n c <- 3\n c <- 4\n wok(<-c + <-c) }"),
    ("chan: <-c + literal",         "package main\npub fn main() { let c = make_chan(int, 2)\n c <- 3\n wok(<-c + 1) }"),
    ("chan: <-c == literal",        "package main\npub fn main() { let c = make_chan(int, 2)\n c <- 3\n wok(<-c == 3) }"),
    ("chan: <-a && <-b",            "package main\npub fn main() { let c = make_chan(bool, 2)\n c <- true\n c <- true\n wok(<-c && <-c) }"),
    ("chan: <-c * literal",         "package main\npub fn main() { let c = make_chan(int, 2)\n c <- 3\n wok(<-c * 2) }"),
    ("chan: !<-c",                  "package main\npub fn main() { let c = make_chan(bool, 2)\n c <- true\n wok(!<-c) }"),
    ("chan: <-arr[0] (postfix lebih erat)", "package main\npub fn main() { let c = make_chan(int, 2)\n c <- 7\n let arr = [c]\n wok(<-arr[0]) }"),
    ("chan: <-f(c) (panggilan lebih erat)", "package main\nfn ambil(c) { return c }\npub fn main() { let c = make_chan(int, 2)\n c <- 8\n wok(<-ambil(c)) }"),

    // MODUL `std/...` DI KEDUA MESIN.
    ("std: import testing",  "package main\nimport \"std/testing\"\npub fn main() { testing::mulai(\"a\")\n testing::cek_sama(\"tambah\", 1 + 1, 2)\n testing::selesai() }"),
    ("std: import option",   "package main\nimport \"std/option\"\npub fn main() { wok(option::unwrap_or_else(None, || 9))\n wok(option::to_array(Some(3))) }"),
    ("std: import result",   "package main\nimport \"std/result\"\npub fn main() { wok(result::map_or(Ok(4), 0, |v| v * 2)) }"),
    ("std: import iterator", "package main\nimport \"std/iterator\"\npub fn main() { wok(iterator::chunk([1, 2, 3, 4, 5], 2))\n wok(iterator::enumerate([\"a\", \"b\"])) }"),
    ("std: empat modul sekaligus", "package main\nimport \"std/option\"\nimport \"std/result\"\nimport \"std/iterator\"\nimport \"std/testing\"\npub fn main() { wok(option::to_array(Some(1)), result::contains(Ok(2), 2), iterator::zip([1], [2])) }"),

    // ── Posisi dihitung per KARAKTER, bukan per byte ─────────────────────
    ("posisi: find per karakter",    "package main\npub fn main() { wok(str::find(\"héllo\", \"l\")) }"),
    ("posisi: find di luar ASCII",   "package main\npub fn main() { wok(str::find(\"日本語kode\", \"k\")) }"),
    ("posisi: find tidak ketemu",    "package main\npub fn main() { wok(str::find(\"halo\", \"z\")) }"),
    // Bentuk pulang-perginya sekarang lewat `Option`. Kasus lamanya berbunyi
    // `s[str::find(s, "k")]` — yang persis jebakan yang ditutup: `-1` adalah
    // indeks yang SAH, jadi pencarian yang gagal membaca karakter terakhir.
    ("posisi: pulang-pergi s[find]",
     "package main\npub fn main() { let s = \"日本語kode\"\n       match str::find(s, \"k\") { Some(i) => wok(s[i]), None => wok(\"tidak ada\") }\n       match str::find(s, \"z\") { Some(i) => wok(s[i]), None => wok(\"tidak ada\") } }"),
    ("posisi: method find/index_of", "package main\npub fn main() { wok(\"héllo\".find(\"l\"), \"héllo\".index_of(\"l\")) }"),

    // ── Cast string ke angka, dan bentuk method yang cuma hidup di VM ────
    ("arity: argumen opsional tetap sah", "package main\npub fn main() { wok(jawa::array(3) != nil, jawa::array(3, 7) != nil) }"),
    ("arity: batas pas tetap sah",        "package main\npub fn main() { wok(str::replace(\"aa\", \"a\", \"b\"), dict::get_or({\"a\": 1}, \"z\", 0), arr::join([1, 2], \"-\")) }"),
    ("cast: string ke float",  "package main\npub fn main() { wok(\"12\" as float, \"12.5\" as float) }"),
    ("cast: string ke uint",   "package main\npub fn main() { wok(\"12\" as uint, \"12\" as u32) }"),
    ("method: upper/lower",    "package main\npub fn main() { wok(\"halo\".upper(), \"HALO\".lower()) }"),


    ("task: panggil fn tingkat-atas", "package main\nfn bantu(n) -> int { return n * 5 }\nfn kerja2() -> int { return bantu(21) }\npub fn main() { wok(task::join(Task::spawn(kerja2))) }"),
    ("task: baca var modul",          "package main\nvar GLOBAL = 8\nfn baca() -> int { return GLOBAL }\npub fn main() { wok(task::join(Task::spawn(baca))) }"),
    ("task: rantai fn dua tingkat",   "package main\nfn c(n) -> int { return n + 1 }\nfn b(n) -> int { return c(n) * 2 }\nfn a() -> int { return b(3) }\npub fn main() { wok(task::join(Task::spawn(a))) }"),
    ("future: panggil fn tingkat-atas","package main\nfn bantu(n) -> int { return n * 5 }\nfn kerja2() -> int { return bantu(21) }\npub fn main() { wok(future::await(Future::new(kerja2))) }"),
    ("future: baca var modul",        "package main\nvar GLOBAL = 8\nfn baca() -> int { return GLOBAL }\npub fn main() { wok(future::await(Future::new(baca))) }"),

    // ── closure `move` ───────────────────────────────────────────────────
    ("move: berbagi (biasa)",       "package main\npub fn main() { var n = 1\n let f = || n\n n = 9\n wok(f()) }"),
    ("move: snapshot luar->dalam",  "package main\npub fn main() { var n = 1\n let f = move || n\n n = 9\n wok(f()) }"),
    ("move: biasa dalam->luar",     "package main\npub fn main() { var n = 1\n let f = || { n = 5\n return n }\n wok(f())\n wok(n) }"),
    ("move: dalam->luar terputus",  "package main\npub fn main() { var n = 1\n let f = move || { n = 5\n return n }\n wok(f())\n wok(n) }"),
    // Salinannya DIMILIKI closure, jadi mutasi bertahan antar pemanggilan —
    // kalau tidak, `move` cuma jadi pembekuan sekali pakai.
    ("move: mutasi bertahan",       "package main\npub fn main() { var n = 0\n let f = move || { n = n + 1\n return n }\n wok(f())\n wok(f())\n wok(n) }"),
    ("move: agregat disalin",       "package main\npub fn main() { var a = [1,2]\n let f = move || a\n a[0] = 9\n wok(f())\n wok(a) }"),
    ("move: dua nama",              "package main\npub fn main() { var a = 1\n var b = 2\n let f = move || a + b\n a = 10\n b = 20\n wok(f()) }"),
    ("move: closure bersarang",     "package main\npub fn main() { var n = 1\n let f = move || { let g = || n\n return g() }\n n = 9\n wok(f()) }"),
    ("move: tanpa tangkapan",       "package main\npub fn main() { let f = move || 7\n wok(f()) }"),
    ("move: lolos keluar fungsi",   "package main\nfn buat() { var n = 5\n return move || n }\npub fn main() { let f = buat()\n wok(f()) }"),
    // `spawn` SUDAH menangkap by-value, jadi `move` di sana akurat tapi
    // mubazir. Diuji dengan jabat tangan channel supaya deterministik —
    // tanpa itu hasilnya balapan dan tesnya akan berkedip.
    ("move: spawn sudah by-value",  "package main\npub fn main() { var n = 1\n let siap = make_chan(int)\n let hasil = make_chan(int)\n spawn move { let _ = <-siap\n hasil <- n }\n n = 9\n siap <- 1\n wok(<-hasil) }"),

    // Bentuk NORMAL dari ketiga kata kunci yang ditolak harus tetap bekerja.
    // Tanpa pasangan positif ini, menolak `spawn` dan closure sepenuhnya juga
    // akan membuat tabel negatifnya hijau.
    ("tanpa move: closure berbagi sel", "package main\npub fn main() { var n = 1\n let f = || n\n n = 9\n wok(f()) }"),
    ("tanpa move: spawn biasa",         "package main\npub fn main() { let c = make_chan(int)\n spawn { c <- 1 }\n wok(<-c) }"),
    ("tanpa async: fn biasa",           "package main\nfn f() -> int { return 1 }\npub fn main() { wok(f()) }"),

    ("rekursi: tepat 10000 panggilan bersarang masih boleh",
     "package main\nfn d(n: int) -> int { if n == 0 { return 0 }\n return d(n - 1) + 1 }\npub fn main() { wok(d(9999)) }"),
    ("math::wrap_*: membungkus di 64 bit tak bertanda, argumen negatif dan raksasa",
     "package main\npub fn main() { var x: uint = 305419896\n for i in 0..5 { x = math::wrap_add(math::wrap_mul(x, 1664525), 1013904223) ^ (x >> 13) }\n wok(x, math::wrap_sub(0, 1), math::wrap_add(18446744073709551615, 2), math::wrap_mul(-1, 2), typeof(math::wrap_add(1, 2))) }"),
    ("int: tepi 128 bit lewat jalur cepat sama dengan jalur biasa",
     "package main\npub fn main() { let m = 170141183460469231731687303715884105727\n let k = 0 - m\n wok(m - 1 + 1, k - 1, k + m, m == m, m != m, m > m - 1, k <= k, 3 * -4, 1 == 1.0, 2 < 2.5) }"),
    ("struct: field privat warisan dibaca dari method anak, default dari leluhur",
     "package main\nstruct A { rahasia: int = 7, pub buka: int = 1 }\nstruct B: A { pub z: int = 2 }\nimpl A { fn baca(self) -> int { return self.rahasia } }\nimpl B { fn intip(self) -> int { return self.rahasia + self.z } }\npub fn main() { let b = B { z: 9 }\n wok(b.baca(), b.intip(), b.buka, b) }"),
    ("rekursi: 9999 panggilan bersarang lewat closure masih boleh",
     "package main\npub fn main() { var c = |n| 0\n c = |n| n == 0 $ 0 : c(n - 1) + 1\n wok(c(9998)) }"),
    // ── operator yang didefinisikan pengguna ─────────────────────────────
    ("operator: += lewat method milik pemakai, juga ke elemen array",
     "package main\nstruct V { pub x: int, pub y: int }\nimpl V { fn add(self, o: V) -> V { return V { x: self.x + o.x, y: self.y + o.y } } }\npub fn main() {\n var p = V { x: 1, y: 2 }\n let q = V { x: 10, y: 20 }\n p += q\n var a = [V { x: 1, y: 1 }]\n a[0] += q\n wok(p, a) }"),
    ("operator: a + b",         "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} + V{x:2}) }"),
    ("operator: a - b",         "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:5} - V{x:2}) }"),
    ("operator: a * b",         "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:3} * V{x:4}) }"),
    ("operator: -a",            "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(-V{x:7}) }"),
    // `==`/`!=` lewat SATU metode `eq`, dan `< <= > >=` lewat SATU `cmp`.
    // Empat metode terpisah memungkinkan sebuah tipe bilang `a < b` dan
    // `a >= b` dua-duanya benar; satu metode membuat itu mustahil.
    ("operator: a == b",        "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} == V{x:1}); wok(V{x:1} == V{x:2}) }"),
    ("operator: a != b",        "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} != V{x:2}); wok(V{x:1} != V{x:1}) }"),
    ("operator: a < b",         "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} < V{x:2}); wok(V{x:3} < V{x:2}) }"),
    ("operator: a <= b",        "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:2} <= V{x:2}) }"),
    ("operator: a > b",         "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:3} > V{x:2}) }"),
    ("operator: a >= b",        "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:3} >= V{x:3}) }"),
    // `(a+b)+c` — sisi kiri sebuah `Binary`. Backend LLVM tidak bisa membaca
    // tipe bentuk ini lewat `peek_expr_type`, jadi pemeriksaannya harus
    // memakai tipe NYATA sesudah operand diturunkan.
    ("operator: rantai",        "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} + V{x:2} + V{x:3}) }"),
    // Tipe tanpa metode operator tidak berubah perilakunya sama sekali.
    ("operator: tipe bawaan tak tersentuh", "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(1 + 2); wok(-5); wok(1 < 2); wok(\"a\" + \"b\") }"),
    // `&&` dan `||` sengaja TIDAK bisa dioverload: keduanya hubung-singkat,
    // sementara metode selalu mengevaluasi argumennya. Sebuah metode `and`
    // harus diabaikan, bukan dipakai diam-diam.
    ("operator: && tetap hubung-singkat", "package main\nstruct V { pub x: int }\nimpl V { fn and(self, o) -> V { return self } }\npub fn main() { wok(true && false) }"),

    // ── mutabilitas ──────────────────────────────────────────────────────
    ("mut: var ditugasi ulang", "package main\npub fn main() { var x = 1\n x = 2\n wok(x) }"),
    ("mut: let mut",            "package main\npub fn main() { let mut x = 1\n x = 2\n wok(x) }"),
    ("mut: := ditugasi ulang",  "package main\npub fn main() { x := 1\n x = 2\n wok(x) }"),
    ("mut: var lewat indeks",   "package main\npub fn main() { var a = [1,2]\n a[0] = 9\n wok(a) }"),
    ("mut: parameter bisa diubah", "package main\nfn ubah(a) { a[0] = 7\n return a }\npub fn main() { wok(ubah([1,2])) }"),
    ("mut: shadowing let",      "package main\npub fn main() { let x = 1\n let x = x + 1\n wok(x) }"),
    ("mut: let dalam blok",     "package main\npub fn main() { let x = 1\n { var x = 2\n x = 3\n wok(x) }\n wok(x) }"),

    // ── hook tampilan `to_string` ────────────────────────────────────────
    ("hook: wok",              "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1}) }"),
    ("hook: wok_",             "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok_(P{n:1}); wok(\"\") }"),
    ("hook: fmt::sprint",      "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(fmt::sprint(P{n:1})) }"),
    ("hook: template literal", "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { let p = P{n:1}; wok(«${p}») }"),
    ("hook: \"x\" + p",          "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(\"x\" + P{n:1}) }"),
    ("hook: p + \"x\"",          "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1} + \"x\") }"),
    ("hook: s += p",           "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { var s = \"a\"; s += P{n:1}; wok(s) }"),
    // Bersarang: nilai yang sama harus tampil sama entah berdiri sendiri
    // atau di dalam koleksi.
    ("hook: di dalam array",   "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok([P{n:1}, P{n:2}]) }"),
    ("hook: di dalam tuple",   "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok((P{n:1}, 7)) }"),
    ("hook: di dalam dict",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok({\"a\": P{n:1}}) }"),
    ("hook: di field struct",  "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\nstruct Q { pub p: P }\npub fn main() { wok(Q{p: P{n:1}}) }"),
    ("hook: array bersarang",  "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok([[P{n:1}]]) }"),
    // Yang TIDAK boleh berubah.
    ("hook: tipe tanpa hook",  "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\nstruct R { pub m: int }\npub fn main() { wok(R{m:2}) }"),
    ("hook: typeof tetap",     "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(typeof(P{n:1})) }"),
    ("hook: panggil eksplisit","package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1}.to_string()) }"),
    // Dipungut lewat pewarisan dan lewat default trait, karena hook memakai
    // penelusuran metode yang SAMA dengan pemanggilan biasa.
    ("hook: warisan induk",    "package main\nstruct A { pub n: int }\nimpl A { fn to_string(self) -> string { return \"AA\" } }\nstruct B: A { }\npub fn main() { wok(B{n:1}) }"),
    ("hook: default trait",    "package main\ntrait T { fn to_string(self) -> string { return \"TT\" } }\nstruct S { pub n: int }\nimpl T for S { }\npub fn main() { wok(S{n:1}) }"),
    ("hook: enum",             "package main\nenum E { A, B }\nimpl E { fn to_string(self) -> string { return \"EE\" } }\npub fn main() { wok(E::A); wok([E::B]) }"),
    // Pemanggilan TAK LANGSUNG. Ini yang memaksa VM menandai builtin cetak
    // lewat alamat fungsinya alih-alih lewat nama di compiler.
    ("hook: wok tak langsung", "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { let f = wok; f(P{n:1}) }"),
    // Kontainer dalam TANPA hook harus tetap seperti dulu: penjaganya
    // menghitung pemanggilan hook, bukan kedalaman kontainer.
    ("hook: array dalam 200",  "package main\npub fn main() { var a = [1]\n for i in 0..200 { a = [a] }\n wok(len(a)) }"),

    // ── `let … else` ─────────────────────────────────────────────────────
    ("let else: satu nama, banyak nama, tanpa nama", r#"package main
fn satu(o: Option<int>) -> int {
    let Some(x) = o else { return -1 }
    let y = x + 1
    return y * 10
}
fn banyak(o: Option<(int, int)>) -> int {
    let Some((a, b)) = o else { return -1 }
    return a + b
}
fn kosong(o: Option<int>) -> int {
    let Some(_) = o else { return -1 }
    return 7
}
fn larik(a: []int) -> int {
    let [x, y] = a else { return -1 }
    return x + y
}
pub fn main() {
    wok(satu(Some(3)), satu(None))
    wok(banyak(Some((3, 4))), banyak(None))
    wok(kosong(Some(1)), kosong(None))
    wok(larik([3, 4]), larik([1, 2, 3]))
}"#),

    // ── parameter pengumpul `sisa...` ────────────────────────────────────
    ("parameter pengumpul", r#"package main
fn f(a, sisa...) { wok(a, sisa, len(sisa)) }
fn g(semua...) { wok(semua, len(semua)) }
fn jumlah(xs...) -> int {
    var t = 0
    for i in xs { t = t + xs[i] }
    return t
}
fn gabung(sep: string, bagian...) -> string {
    var r = ""
    for i in bagian { if i > 0 { r = r + sep }; r = r + bagian[i] }
    return r
}
pub fn main() {
    f(1, 2, 3)
    f(1)
    g()
    g(7, 8, 9)
    wok(jumlah(1, 2, 3), jumlah())
    wok(gabung("-", "a", "b", "c"))
}"#),

    // ── pola pengikat `nama @ pola` ──────────────────────────────────────
    ("pola pengikat @ mengikat DAN menguji", r#"package main
pub fn main() {
    wok(match 5 { n @ 1..=9 => n * 10, _ => 0 })
    wok(match 50 { n @ 1..=9 => n * 10, _ => 0 })
    wok(match 7 { n @ 7 => n, _ => 0 })
    let o = Some(3)
    wok(match o { n @ Some(x) => x + 1, None => 0 })
    if let n @ 1..=9 = 5 { wok(n) }
}"#),

    // ── operator koalesensi `??` ─────────────────────────────────────────
    ("koalesensi: nilai, cadangan, dan rantai", r#"package main
pub fn main() {
    let a: Option<int> = Some(3)
    let k: Option<int> = None
    wok(a ?? 9, k ?? 9)
    let d = {"a": 1}
    wok(dict::get(d, "z") ?? 9, dict::get(d, "a") ?? 9)
    let b: Option<int> = None
    let c: Option<int> = Some(7)
    wok(k ?? b ?? c ?? 0)
}"#),
    ("koalesensi hubung-singkat, unwrap_or tidak", r#"package main
var n = 0
fn mahal() -> int { n = n + 1; return 9 }
pub fn main() {
    let a: Option<int> = Some(3)
    wok(a ?? mahal(), n)
    let k: Option<int> = None
    wok(k ?? mahal(), n)
    // `unwrap_or` sebuah METHOD, dan pemanggilan mengevaluasi argumennya
    // tanpa kecuali — backend LLVM dulu malas di sini, jadi efek sampingnya
    // hilang tanpa mengubah nilai yang dikembalikan.
    let b: Option<int> = Some(3)
    wok(b.unwrap_or(mahal()), n)
    let r: Result<int, string> = Ok(3)
    wok(r.unwrap_or(mahal()), n)
}"#),

    // ── varian enum tanpa payload disebut TELANJANG ──────────────────────
    ("varian unit telanjang menguji, bukan mengikat", r#"package main
enum E { A(int), B }
pub fn main() {
    let o: Option<int> = Some(3)
    wok(match o { None => 0, Some(v) => v })
    let k: Option<int> = None
    wok(match k { None => 7, Some(v) => v })
    let e = E::A(3)
    wok(match e { B => 0, E::A(v) => v })
    wok(match E::B { B => 7, E::A(v) => v })
    wok(B, A(9))
}"#),

    // ── builtin tanpa nilai mengembalikan `unit`, bukan `nil` ────────────
    ("cetak dan assert mengembalikan unit", r#"package main
fn f() { wok("x") }
pub fn main() {
    wok(f(), typeof(f()))
    wok(typeof(assert(true)), typeof(assert_eq(1, 1)))
    let a = wok_("")
    let b = eprint("")
    wok(typeof(a), typeof(b))
}"#),

    // Yang harus TETAP DITERIMA sesudah nama tipe diperiksa. Sisi positif
    // ini yang menahan pemeriksaannya dari jadi terlalu galak: menolak
    // segalanya juga membuat tabel penolakan hijau.
    ("nama tipe yang sah tetap diterima", r#"package main
struct Titik { pub x: int }
enum Warna { Merah, Biru }
trait Bentuk { fn n(self) -> string }
fn id<T>(x: T) -> T { return x }
fn pakai(t: Titik, w: Warna, a: array) -> Option<Titik> { return Some(t) }
impl Titik { fn diri(self) -> Self { return self } }
pub fn main() {
    let t: Titik = Titik { x: 1 }
    let o: Option<int> = Some(2)
    let r: Result<int, string> = Ok(3)
    let p: Ptr = jawa::alloc(9)
    let xs: []Titik = [t]
    wok(id(1), pakai(t, Warna::Merah, [1]).unwrap().x, o.unwrap(), r.unwrap())
    wok(jawa::get(p), len(xs), t.diri().x)
}"#),

    // ── sintaks yang mesinnya sudah ada tapi kabelnya tidak ──────────────
    ("penugasan jamak: menukar tanpa perantara", r#"package main
pub fn main() {
    var a = 1
    var b = 2
    a, b = b, a
    wok(a, b)
    var c = 3
    a, b, c = c, a, b
    wok(a, b, c)
}"#),
    ("penugasan jamak: elemen dan field", r#"package main
struct P { pub x: int, pub y: int }
pub fn main() {
    var a = [1, 2]
    a[0], a[1] = a[1], a[0]
    wok(a)
    var p = P { x: 1, y: 2 }
    p.x, p.y = p.y, p.x
    wok(p.x, p.y)
}"#),
    ("penugasan jamak: := dan operator gabungan", r#"package main
pub fn main() {
    a, b := 1, 2
    wok(a, b)
    a, b += 10, 20
    wok(a, b)
}"#),
    // `for i := 0; …` dulu selalu dibaca sebagai bentuk range, karena
    // penentunya cuma dua token (`ident :=`). Yang membedakan keduanya
    // adalah ada-tidaknya `;` sebelum badan loop.
    ("for klasik dengan := tetap klasik", r#"package main
fn f(a, b) { return a }
pub fn main() {
    for i := 0; i < 3; i = i + 1 { wok(i) }
    for i := f(0, 1); i < 2; i = i + 1 { wok("f", i) }
    for i := [7, 8] { wok("range", i) }
    for var j = 0; j < 2; j = j + 1 { wok("var", j) }
}"#),
    // `if let` diturunkan jadi `match`, jadi perilakunya harus SAMA PERSIS
    // dengan bentuk panjangnya — termasuk pengikatan yang tidak bocor keluar.
    ("if let: bentuk dasar, else, dan sebagai ekspresi", r#"package main
pub fn main() {
    let o = Some(3)
    if let Some(x) = o { wok("ada", x) }
    let k: Option<int> = None
    if let Some(x) = k { wok(x) } else { wok("kosong") }
    wok(if let Some(v) = o { v } else { 0 })
    if let None = k { wok("varian unit ikut cocok") }
    if let x = 5 { wok("pola tak terbantahkan", x) }
}"#),
    ("if let: rantai else if, campur dengan if biasa", r#"package main
pub fn main() {
    let a: Option<int> = None
    let b = Some(9)
    if let Some(x) = a { wok("a", x) }
    else if let Some(y) = b { wok("b", y) }
    else { wok("tidak ada") }

    if let Some(x) = a { wok(x) } else if 1 > 0 { wok("biasa") } else { wok("tidak") }
}"#),
    ("if let: pola array, Result, dan bersarang", r#"package main
pub fn main() {
    if let [a, b] = [1, 2] { wok(a, b) }
    let r: Result<int, string> = Ok(4)
    if let Ok(v) = r { wok(v) }
    let dua = Some(Some(5))
    if let Some(dalam) = dua { if let Some(v) = dalam { wok(v) } }
}"#),
    ("while let: break, continue, dan label", r#"package main
pub fn main() {
    var sisa = [1, 2, 3]
    while let Some(x) = arr::first(sisa) { wok(x); sisa = arr::drop(sisa, 1) }

    var n = 0
    while let Some(x) = Some(n) {
        n = n + 1
        if x == 1 { continue }
        if x > 3 { break }
        wok("n", x)
    }

    var m = 0
    'luar: while let Some(x) = Some(m) {
        if x > 1 { break 'luar }
        wok("label", x)
        m = m + 1
    }
}"#),

    // ── method array: satu definisi, dua backend ──────────────────────────
    // Seluruh permukaan `iterator::` dalam satu program: 43 fungsi, dan yang …
    ("iterator: seluruh permukaannya", r#"package main
import "std/iterator"
pub fn main() {
    let a = [3,1,2]
    wok(iterator::chunk(a,2), iterator::window(a,2), iterator::zip(a,[9,8,7]))
    wok(iterator::unzip([(1,2),(3,4)]), iterator::enumerate(a))
    wok(iterator::take_while(a, |x| x>2), iterator::drop_while(a, |x| x>2))
    wok(iterator::group_by(a, |x| x%2), iterator::tally([1,1,2]))
    wok(iterator::scan(a, |s,x| s+x, 0), iterator::intersperse(a, 0))
    wok(iterator::find(a, |x| x>1), iterator::position(a, |x| x==2))
    wok(iterator::all(a, |x| x>0), iterator::any(a, |x| x>2))
    wok(iterator::flatten([[1],[2]]), iterator::flat_map(a, |x| [x,x]))
    wok(iterator::map(a,|x| x), iterator::filter(a,|x| x>1))
    wok(iterator::filter_map(a, |x| x>1 $ Option::Some(x) : Option::None))
    wok(iterator::reduce(a,|s,x| s+x,0), iterator::fold(a,|s,x| s*x,1))
    wok(iterator::step_by(a,2), iterator::chain([1],[2]), iterator::chain_all([[1],[2]]))
    wok(iterator::count(a,|x| x>1), iterator::find_last(a,|x| x>1))
    wok(iterator::sum_by(a,|x| x), iterator::take(a,2), iterator::skip(a,2))
    wok(iterator::tabulate(3,|i| i), iterator::partition(a,|x| x>1))
    wok(iterator::unique([1,1,2]), iterator::reverse(a))
    wok(iterator::min_by(a,|x| x), iterator::max_by(a,|x| x))
    wok(iterator::first(a), iterator::last(a), iterator::is_empty([]), iterator::count_all(a))
    wok(iterator::chunk_by([1,1,2], |x| x))
    iterator::each(a, |x| x)
    wok("lengkap")
}"#),
    // Penjaga argumen modul `std/` menolak DI PINTUNYA, dan jejaknya menyebut
    // baris milik pemakai — bukan baris di dalam modulnya.
    ("std: penjaga argumen menolak di pintunya", r#"package main
import "std/iterator"
pub fn main() {
    wok(iterator::map([1,2], |x| x * 2))
    wok(iterator::filter([1,2], |x| x > 1))
}"#),
    ("map sebagai nama biasa, dan map[K]V tetap tipe", r#"package main
fn map(a, f) {
    var keluar = []
    for _, x in a { keluar = arr::push(keluar, f(x)) }
    return keluar
}
fn hitung(d: map[string]int) -> int { return dict::len(d) }
pub fn main() {
    wok(map([1,2,3], |x| x * 2))
    wok(hitung({"a": 1, "b": 2}))
    let map = 5
    wok(map + 1)
}"#),
    ("method array tingkat-tinggi", r#"package main
pub fn main() {
    let a = [3, 1, 2]
    a.each(|x| wok("e", x))
    wok(a.count_if(|x| x > 1))
    wok(a.find_idx(|x| x == 2), a.find_idx(|x| x == 9))
    wok([[1,2],[3]].flat_map(|x| x))
    wok(a.min_by(|x| x), a.max_by(|x| x))
    wok(a.sort_by(|p, q| p - q), a.sort_by(|p, q| q - p))
    wok(a.sort_by_key(|x| 0 - x))
    wok(a)
}"#),
    // Satu nama, satu kontrak.
    ("kontrak each dan group_by: satu nama, satu hasil", r#"package main
pub fn main() {
    let a = [1, 2, 3, 4]
    wok(typeof(arr::each(a, |x| x)), arr::each(a, |x| x))
    wok(typeof(a.each(|x| x)), a.each(|x| x))
    let g = arr::group_by(a, |x| x % 2)
    let h = a.group_by(|x| x % 2)
    wok(typeof(g), g)
    wok(typeof(h), h)
    wok(g == h)
    // Kuncinya bentuk Display-nya, seperti tiap kunci dict di bahasa ini.
    wok(h["0"], h[1], dict::get(h, 0))
    // `each` tidak menyentuh asalnya, dan efek sampingnya tetap jalan.
    var n = 0
    a.each(|x| { n = n + x })
    wok(n, a)
}"#),
    ("method array yang dulu cuma ada di interpreter", r#"package main
pub fn main() {
    wok([1,2,3].binary_search(2), [1,2,3].binary_search(9))
    wok([1,1,2,2,3].dedup())
    wok([1,2,3,4].drop(2), [1,2,3,4].skip(1))
    wok([1,2,3].rotate_left(1), [1,2,3].rotate_right(1))
    wok([1,2,3].to_string())
}"#),
    // Sisi POSITIF dari aturan yang diperbaiki Fase SAPU-METODE: dengan
    // argumennya DIBERIKAN, kedelapan nama ini tetap jalan dan tetap sepakat.
    ("bentuk method str yang dulu cuma ada di VM", r#"package main
pub fn main() {
    wok("halo dunia".capitalize())
    wok("Halo Dunia".swap_case())
    wok("halo".is_lower(), "HALO".is_upper())
    wok("halo dunia".slice(0, 4))
    wok(str::format("{} {}", 1, 2), "{} {}".format(1, 2))
}"#),
    // ── `"""` — pembatas ASCII untuk template ────────────────────────────
    ("tiga kutip: multi-baris dan interpolasi", r#"package main
pub fn main() {
    let n = 7
    let s = """halo ${n}
baris dua
  dengan "kutip" tunggal"""
    wok(s)
    wok(len(str::lines(s)))
    wok("""satu baris""")
    wok(len(""""""))
}"#),
    // Sisi yang paling gampang rusak: string BIASA tidak boleh berubah arti.
    ("tiga kutip: string biasa tidak berubah", r#"package main
pub fn main() {
    wok("halo")
    wok("", "")
    wok("" + "x")
    wok("dia bilang \"halo\"")
    wok("a\"\"\"b")
    wok(«template lama ${1 + 1}»)
}"#),
    ("argumen wajib diberikan, dan hasilnya tetap sepakat", r#"package main
pub fn main() {
    wok([1,2,3].chunks(2), [1,2,3].windows(2))
    wok([1,2,3].take(2), [1,2,3].skip(1))
    wok([1,2,3].repeat(2), [1,2].join("-"))
    wok([1,2,3].get(0), [1,2,3].get_or(9, -1))
}"#),
    ("fill sebagai method bukan fungsi namespace", r#"package main
pub fn main() {
    wok([1,2,3].fill(9))
    wok(arr::fill(7, 3))
    wok(len([1,2,3].fill(|x| x)))
}"#),
    ("shuffle dan sample punya BENTUK yang sama", r#"package main
pub fn main() {
    let a = [1, 2, 3, 4, 5, 6, 7, 8]
    wok(typeof(a.sample()), len(a.sample()), len(a.sample(3)))
    wok(typeof(a.shuffle()), len(a.shuffle()))
    var beda = 0
    for i in 0..20 { if a.shuffle() != a { beda = beda + 1 } }
    wok(beda > 10)
}"#),

    // ── method string: satu definisi, dua backend ─────────────────────────
    ("str::substr argumen ketiga adalah PANJANG", r#"package main
pub fn main() {
    wok(str::substr("abcdef", 1, 2))
    wok(str::substr("abcdef", 2, 3))
    wok(str::substr("abcdef", 0))
}"#),
    ("str::slice argumen ketiga adalah AKHIR", r#"package main
pub fn main() {
    wok(str::slice("abcdef", 1, 2))
    wok(str::slice("abcdef", 2, 3))
}"#),
    ("method string yang dulu cuma ada di interpreter", r#"package main
pub fn main() {
    wok("  x  ".trim_start(), "|")
    wok("  x  ".trim_end(), "|")
    wok("hé".byte_len(), "hé".len())
    wok("hé".to_string())
    wok("abcdef".sub(1, 2), "abcdef".substring(2, 3))
}"#),
    ("find pada string bukan HOF", r#"package main
pub fn main() {
    wok("abc".find("b"), "abc".find("z"))
    wok([1, 2, 3].find(|x| x > 1))
}"#),

    // ── truthiness ────────────────────────────────────────────────────────
    ("truthiness: int, float, string",
     "package main\nfn cek(b: bool) -> string { return b $ \"T\" : \"F\" }\n\
      pub fn main() { wok(cek(0), cek(1), cek(-1), cek(0.0), cek(1.5), cek(\"\"), cek(\"x\"), cek(\"0\")) }"),
    ("truthiness: NaN falsy, inf truthy, -0.0 falsy",
     "package main\nfn cek(b: bool) -> string { return b $ \"T\" : \"F\" }\n\
      pub fn main() { var n: f64 = math::NAN; var i: f64 = math::INF; var z: f64 = -0.0;\n\
      wok(cek(n), cek(i), cek(z)); if n { wok(\"NaN truthy\") } else { wok(\"NaN falsy\") } }"),
    ("truthiness: konversi bool() eksplisit",
     "package main\npub fn main() { wok(bool(0), bool(1), bool(-5), bool(0.0), bool(1.5), bool(\"\"), bool(\"x\"), bool(true)) }"),

    ("aritmetika dan presedensi",
     "package main\npub fn main() { wok(2 + 3 * 4); wok((2 + 3) * 4); wok(7 / 2); wok(-7 / 2); wok(7 % 3) }"),
    ("perbandingan dan logika",
     "package main\npub fn main() { wok(1 < 2); wok(2 <= 2); wok(3 > 4); wok(1 == 1); wok(1 != 2) }"),
    ("alur kendali",
     "package main\npub fn main() { var t = 0; var i = 0; while i < 5 { if i % 2 == 0 { t = t + i }; i = i + 1 }; wok(t) }"),
    ("fungsi dan rekursi",
     "package main\nfn fib(n: int) -> int { if n < 2 { return n }; return fib(n-1) + fib(n-2) }\npub fn main() { wok(fib(15)) }"),
    ("string: method dasar",
     "package main\npub fn main() { let s = \"halo dunia\"; wok(s.contains(\"dunia\")); wok(s.starts_with(\"halo\")) }"),
    ("closure tanpa tangkapan",
     "package main\npub fn main() { let f = |x| x * 2; wok(f(21)) }"),

    // ── Tangkapan closure (Phase 5F-C) ───────────────────────────────────
    ("closure: tangkap satu lokal",
     "package main\npub fn main() { let x = 42; let f = || x; wok(f()) }"),
    ("closure: mutasi lewat closure bertahan",
     "package main\npub fn main() { var n = 0; let inc = || { n = n + 1; n }; wok(inc()); wok(inc()); wok(inc()) }"),
    ("closure: dua closure berbagi satu binding",
     "package main\npub fn main() { var n = 0; let inc = || { n = n + 1 }; let get = || n; inc(); inc(); wok(get()) }"),
    // Kalau tangkapan berupa potret nilai, ini menjawab 1.
    ("closure: mutasi langsung terlihat dari closure",
     "package main\npub fn main() { var n = 1; let get = || n; n = 11; wok(get()) }"),
    ("closure: tulisan closure terlihat di scope asal",
     "package main\npub fn main() { var n = 1; let set = || { n = 2 }; set(); wok(n) }"),
    ("closure: instance berbeda saling bebas",
     "package main\nfn mk() { var n = 0; return || { n = n + 1; n } }\npub fn main() { let p = mk(); let q = mk(); p(); p(); wok(p()); wok(q()) }"),
    ("closure: lolos dari scope pembuat",
     "package main\nfn mk() { var n = 100; return || { n = n + 1; n } }\npub fn main() { let c = mk(); c(); wok(c()) }"),
    ("closure: bersarang menangkap dari luar",
     "package main\npub fn main() { let x = 10; let outer = || { let inner = || x + 1; inner() }; wok(outer()) }"),
    ("closure: bersarang tiga tingkat",
     "package main\npub fn main() { let x = 5; let f = || { let g = || { let h = || x * 2; h() }; g() }; wok(f()) }"),
    ("closure: bersarang menulis binding terluar",
     "package main\npub fn main() { var n = 0; let outer = || { let inner = || { n = n + 7 }; inner() }; outer(); wok(n) }"),
    // Tangkapan mengikuti IDENTITAS binding, bukan namanya saja.
    ("closure: shadowing memilih binding terdalam",
     "package main\npub fn main() { let x = 1; let x = 2; let f = || x; wok(f()) }"),
    ("closure: parameter membayangi tangkapan",
     "package main\npub fn main() { let x = 1; let f = |x| x + 1; wok(f(10)); wok(x) }"),
    ("closure: let di badan membayangi tangkapan",
     "package main\npub fn main() { let x = 1; let f = || { let x = 9; x }; wok(f()); wok(x) }"),
    // Tiap iterasi `for-in` memasang binding baru. Implementasi yang
    // berbagi satu slot menjawab `2 2 2`.
    ("closure: tangkapan di for-in per iterasi",
     "package main\npub fn main() { var fs = []; for k in 0..3 { fs = fs + [|| k] }; wok(fs[0]()); wok(fs[1]()); wok(fs[2]()) }"),
    // Sebaliknya `while` cuma punya SATU binding — dipasangkan dengan kasus
    // di atas supaya "per iterasi" tidak ditambal jadi selalu-baru.
    ("closure: tangkapan di while satu binding",
     "package main\npub fn main() { var fs = []; var i = 0; while i < 3 { fs = fs + [|| i]; i = i + 1 }; wok(fs[0]()); wok(fs[2]()) }"),
    ("closure: satu binding luar ditangkap berkali-kali di loop",
     "package main\npub fn main() { var fs = []; var n = 0; for _i in 0..3 { fs = fs + [|| n] }; n = 5; wok(fs[0]()); wok(fs[2]()) }"),
    // Dua blok berurutan memakai slot stack yang sama; bindingnya tetap beda.
    ("closure: slot stack dipakai ulang blok lain",
     "package main\npub fn main() { var fs = []; if true { let a = 1; fs = fs + [|| a] }; if true { let b = 2; fs = fs + [|| b] }; wok(fs[0]()); wok(fs[1]()) }"),
    ("closure: banyak tangkapan sekaligus",
     "package main\npub fn main() { let a = 1; let b = 2; let c = 3; let f = || a + b + c; wok(f()) }"),
    ("closure: menangkap parameter fungsi pembuat",
     "package main\nfn adder(base) { return |v| v + base }\npub fn main() { let a5 = adder(5); let a9 = adder(9); wok(a5(1)); wok(a9(1)) }"),
    ("closure: mengembalikan closure (currying)",
     "package main\npub fn main() { let mk = |a| |b| a + b; wok(mk(3)(4)) }"),
    // Konsekuensi langsung dari menangkap binding, bukan menyalin nilainya.
    ("closure: agregat tertangkap melihat mutasi di tempat",
     "package main\npub fn main() { var a = [1,2,3]; let get = || a[0]; a[0] = 99; wok(get()) }"),
    ("closure: menulis agregat yang ditangkap",
     "package main\npub fn main() { var a = [1,2,3]; let bump = || { a[0] = a[0] + 100 }; bump(); bump(); wok(a) }"),
    ("closure: dipakai sebagai argumen hof",
     "package main\npub fn main() { let f = 3; wok(arr::map([1,2,3], |v| v * f)) }"),
    ("closure: rekursi lewat binding yang ditangkapnya sendiri",
     "package main\npub fn main() { var fak = nil; fak = |n| if n <= 1 { 1 } else { n * fak(n - 1) }; wok(fak(5)) }"),
    ("closure: rekursi mutual",
     "package main\npub fn main() { var genap = nil; var ganjil = nil; genap = |n| if n == 0 { true } else { ganjil(n - 1) }; ganjil = |n| if n == 0 { false } else { genap(n - 1) }; wok(genap(10)) }"),
    ("closure: typeof",
     "package main\npub fn main() { let x = 1; let a = || x; let b = || 1; wok(typeof(a)); wok(typeof(b)) }"),

    // ── `match` sebagai SUB-EKSPRESI (Phase 5F-F) ────────────────────────
    ("match: sebagai argumen",
     "package main\npub fn main() { let v = Some(5); wok(match v { Some(n) => n, None => 0 }) }"),
    ("match: argumen kedua",
     "package main\npub fn main() { let v = Some(5); wok(1, match v { Some(n) => n, None => 0 }) }"),
    ("match: operand kanan binop",
     "package main\npub fn main() { let v = Some(5); wok(1 + match v { Some(n) => n, None => 0 }) }"),
    ("match: operand kiri binop",
     "package main\npub fn main() { let v = Some(5); wok(match v { Some(n) => n, None => 0 } + 1) }"),
    ("match: elemen array",
     "package main\npub fn main() { let v = Some(5); wok([9, match v { Some(n) => n, None => 0 }]) }"),
    ("match: elemen tuple",
     "package main\npub fn main() { let v = Some(5); wok((9, match v { Some(n) => n, None => 0 })) }"),
    ("match: nilai field struct",
     "package main\nstruct K { pub a: int = 0, pub b: int = 0 }\npub fn main() { let v = Some(5); wok(K { a: 9, b: match v { Some(n) => n, None => 0 } }) }"),
    ("match: argumen fungsi user",
     "package main\nfn dua(x, y) { return x * 100 + y }\npub fn main() { let v = Some(5); wok(dua(9, match v { Some(n) => n, None => 0 })) }"),
    ("match: sebagai index",
     "package main\npub fn main() { let v = Some(5); wok([10,20,30][match v { Some(n) => n - 4, None => 0 }]) }"),
    ("match: bersarang di dalam arm",
     "package main\npub fn main() { let v = Some(5); wok(match v { Some(n) => match Some(n) { Some(m) => m * 2, None => 0 }, None => 0 }) }"),
    ("match: di dalam kondisi if",
     "package main\npub fn main() { let v = Some(5); wok(if match v { Some(n) => n > 3, None => false } { \"ya\" } else { \"tidak\" }) }"),

    // ── `match` di posisi ekspresi yang MENUMPUK stack (Phase 5F-I) ──────
    ("match: batas bawah slice",
     "package main\npub fn main() { let v = Some(5); let a = [10,20,30,40]; wok(a[match v { Some(n) => n - 5, None => 0 } : 3]) }"),
    ("match: batas atas slice",
     "package main\npub fn main() { let v = Some(5); let a = [10,20,30,40]; wok(a[1 : match v { Some(n) => n - 2, None => 0 }]) }"),
    ("match: payload konstruktor enum",
     "package main\npub fn main() { let v = Some(5); wok(Some(match v { Some(n) => n * 2, None => 0 })); wok(Ok(match v { Some(n) => n + 1, None => 0 })) }"),
    ("match: payload enum banyak argumen",
     "package main\nenum P { Dua(int, int) }\npub fn main() { let v = Some(5); wok(P::Dua(match v { Some(n) => n, None => 0 }, 9)) }"),
    ("match: nilai balik multi",
     "package main\nfn multi() { let v = Some(3); return (match v { Some(n) => n, None => 0 }, 7) }\npub fn main() { wok(multi()) }"),
    ("match: jumlah pengulangan array",
     "package main\npub fn main() { let v = Some(3); wok([match v { Some(n) => n, None => 0 }; 3]) }"),
    ("match: nilai yang dikirim ke channel",
     "package main\npub fn main() { let v = Some(5); let ch = make_chan(int, 2); ch <- match v { Some(n) => n, None => 0 }; wok(<-ch) }"),
    ("match: di dalam badan spawn",
     "package main\npub fn main() { let v = Some(5); let ch = make_chan(int, 2); spawn { ch <- match v { Some(n) => n + 100, None => 0 } }; wok(<-ch) }"),
    ("match: di dalam template",
     "package main\npub fn main() { let v = Some(5); wok(«t ${match v { Some(n) => n, None => 0 }}») }"),

    // ── Namespace sistem (Phase 5F-J) ────────────────────────────────────
    ("sistem: time:: bentuk dan rentang",
     "package main\npub fn main() { wok(time::year() > 2000); wok(time::month() >= 1 && time::month() <= 12); wok(time::hour() >= 0 && time::hour() <= 23); wok(typeof(time::weekday())); wok(time::now() > 0); wok(time::now_ms() > 0); wok(time::format(0)) }"),
    ("sistem: env::",
     "package main\npub fn main() { wok(env::get_or(\"VAR_TIDAK_ADA_XYZ\", \"cadangan\")); wok(env::get(\"VAR_TIDAK_ADA_XYZ\")); wok(typeof(env::args())) }"),
    ("sistem: rand:: tipe dan rentang",
     "package main\npub fn main() { wok(typeof(rand::int(1,10))); wok(rand::int(5,5)); wok(typeof(rand::float())); wok(typeof(rand::bool())); wok(rand::choice([7])); wok(typeof(rand::shuffle([1,2,3]))); wok(len(rand::sample([1,2,3], 2))) }"),
    ("sistem: crypto:: hex/base64/hash",
     "package main\npub fn main() { wok(crypto::hex_encode(\"hi\")); wok(crypto::hex_decode(\"6869\")); wok(crypto::base64_encode(\"hi\")); wok(crypto::base64_decode(\"aGk=\")); wok(crypto::hash(\"halo\")); wok(len(crypto::random_hex(4))) }"),
    ("sistem: file:: pemeriksaan yang aman",
     "package main\npub fn main() { wok(file::exists(\"/tidak/ada/xyz\")); wok(typeof(file::read(\"/tidak/ada/xyz\"))) }"),
    ("sistem: time::sleep_ms menjepit negatif",
     "package main\npub fn main() { wok(typeof(time::sleep_ms(-5))); wok(typeof(time::sleep_ms(0))) }"),

    // ── Ternary `$ :` ────────────────────────────────────────────────────
    ("ternary: cabang berupa match / if / template",
     "package main\npub fn main() { let v = Some(5); let n = 7; wok(n > 3 $ match v { Some(x) => x, None => 0 } : 0); wok(n > 3 $ if n > 5 { \"a\" } else { \"b\" } : \"c\"); wok(n > 3 $ «t${n}» : \"x\") }"),
    ("ternary: cabang berupa literal majemuk",
     "package main\nstruct K { pub a: int = 0 }\npub fn main() { let n = 7; let d = {\"a\": 1}; let a = [1,2,3,4]; wok(n > 3 $ d[\"a\"] : 0); wok(n > 3 $ a[1:3] : []); wok(n > 3 $ K{a:1} : K{}); wok(n > 3 $ (1,2) : (3,4)); wok(n > 3 $ Some(1) : None) }"),
    ("ternary: bersarang, dengan dan tanpa kurung",
     "package main\npub fn main() { let n = 7; wok(n > 3 $ (n > 5 $ \"aa\" : \"ab\") : \"b\"); wok(n > 3 $ n > 5 $ \"aa\" : \"ab\" : \"b\") }"),
    ("ternary: cabang berawalan operator prefix",
     "package main\npub fn main() { let n = 7; wok(n > 3 $ -1 : 1); wok(n > 3 $ !false : true); wok(n > 3 $ ~1 : 0); wok(n > 3 $ 'z' : 'y') }"),

    // ── Ternary: bentuk yang DULU gagal parse ────────────────────────────
    ("ternary: propagasi `?` di dalam cabang dan kondisi",
     "package main\nfn g(n) { if n > 0 { return Ok(n) }; return Err(\"x\") }\nfn f(n) { let a = true $ g(n)? : 0; let b = g(n)? > 0 $ \"p\" : \"m\"; let c = true $ g(n)? : g(n)?; return Ok((a, b, c)) }\npub fn main() { wok(f(7)) }"),
    ("ternary: dipecah antar-baris",
     "package main\npub fn main() { let x = true $\n    1 : 2\n  let y = false $\n    3 : 4\n  wok(x); wok(y) }"),
    ("ternary: cabang sangat panjang (dulu kena batas pindai)",
     // >4.096 token antara pembuka dan `:` — dulu membuat pemindaian
     // menyerah dan ternary yang sah ditolak.
     "package main\npub fn main() { let v = true $ 1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1 : 0; wok(v) }"),

    // ── Header kontrol: `{` selalu membuka BADAN, bukan literal struct ───
    ("header: if/while/for/match dengan nama telanjang lalu `:` di badan",
     "package main\npub fn main() { let v = true; let a = [1,2,3]\n  if v { let x: int = 1; wok(x) }\n  if !v { wok(\"tidak\") } else { let y: int = 2; wok(y) }\n  var w = true\n  while w { let z: int = 3; wok(z); w = false }\n  for i in a { let k: int = i; wok(k) }\n  wok(match v { true => a[0:2], _ => a }) }"),
    // ── Identitas handle sumber daya ─────────────────────────────────────
    ("identitas: channel sama dan channel beda",
     "package main\npub fn main() { let c = make_chan(int); let d = c; let e = make_chan(int); wok(c == d); wok(c == e); wok(d != e) }"),

    // ── Trait: method abstrak dan default ────────────────────────────────
    ("trait: tipe anak mewarisi default trait induknya",
     "package main\ntrait T { fn s(self) -> string\n fn sapa(self) { \"halo \" + self.s() } }\nstruct K { n: int = 1 }\nimpl T for K { fn s(self) -> string { \"k\" } }\nstruct A: K { m: int = 0 }\nstruct B: A { }\nimpl T for B { fn s(self) -> string { \"b\" } }\npub fn main() { wok(A {}.sapa(), B {}.sapa(), K {}.sapa()) }"),
    ("trait: default berbadan ekspresi tetap jadi default",
     "package main\ntrait S { fn m(self) -> string = \"bawaan\" }\nstruct K { pub n: int }\nimpl S for K { }\npub fn main() { let k = K { n: 1 }; wok(k.m()) }"),
    ("trait: abstrak dan default bisa bercampur",
     "package main\ntrait S { fn wajib(self) -> string\n  fn opsional(self) -> string { return \"bawaan\" } }\nstruct K { pub n: int }\nimpl S for K { fn wajib(self) -> string { return \"diisi\" } }\npub fn main() { let k = K { n: 1 }; wok(k.wajib()); wok(k.opsional()) }"),
    ("trait: abstrak boleh diakhiri titik koma",
     "package main\ntrait S { fn m(self) -> string; }\nstruct K { pub n: int }\nimpl S for K { fn m(self) -> string { return \"punya\" } }\npub fn main() { let k = K { n: 1 }; wok(k.m()) }"),

    ("header: literal struct tetap sah kalau dikurung",
     "package main\nstruct K { pub a: int = 7 }\nfn punya(k) { return k.a > 0 }\npub fn main() { if (K { a: 1 }).a > 0 { wok(\"kurung\") }\n  if punya(K { a: 2 }) { wok(\"argumen\") }\n  wok(match (K { a: 3 }).a { 3 => \"tiga\", _ => \"lain\" })\n  for i in [K { a: 4 }] { wok(i) } }"),
    (
        // Arah sebaliknya: `?` yang MEMANG propagasi, diikuti operator
        // infix. Ini yang dulu gagal parse sama sekali.
        "ternary: propagasi diikuti operator infix",
        "package main\nfn g(n) { if n > 0 { return Ok(n) }; return Err(\"x\") }\nfn f(n) { let v = g(n)? - 1; let w = g(n)? | 8; let x = g(n)? * 2; return Ok((v, w, x)) }\npub fn main() { wok(f(7)) }",
    ),

    // ── Integer raksasa tidak boleh terpotong diam-diam (Phase 5F-I) ─────
    ("numerik: index raksasa lewat .get() tidak dibungkus",
     "package main\npub fn main() { let a = [1,2,3]; wok(a.get(18446744073709551617)) }"),
    ("numerik: cacah raksasa berarti seluruhnya",
     "package main\npub fn main() { let a = [1,2,3]; wok(arr::take(a, 18446744073709551617)); wok(arr::skip(a, 18446744073709551617)) }"),

    // ── Pola `None` bukan pengikat (Phase 5F-F) ──────────────────────────
    ("pola: None tidak menelan Some",
     "package main\npub fn main() { wok(match Some(1) { None => \"SALAH\", Some(x) => \"benar\" }) }"),
    ("pola: None tetap cocok dengan None",
     "package main\npub fn main() { wok(match None { None => \"benar\", Some(x) => \"SALAH\" }) }"),

    // ── Pola array (Phase 5F-F) ──────────────────────────────────────────
    ("pola array: panjang persis",
     "package main\npub fn main() { wok(match [1,2] { [a,b] => a + b, _ => -1 }); wok(match [1,2,3] { [a,b] => 0, _ => -1 }) }"),
    ("pola array: kepala dan sisa",
     "package main\npub fn main() { wok(match [1,2,3] { [h, ...t] => t, _ => [] }); wok(match [9] { [h, ...t] => t, _ => [-1] }) }"),
    ("pola array: depan dan belakang",
     "package main\npub fn main() { wok(match [1,2,3,4,5] { [a, ..., z] => a * 100 + z, _ => -1 }) }"),
    ("pola array: kosong dan bersarang",
     "package main\npub fn main() { wok(match [] { [] => \"kosong\", _ => \"isi\" }); wok(match [[1,2],[3,4]] { [[a,b],[c,d]] => a+b+c+d, _ => -1 }) }"),
    ("pola array: literal di dalam pola",
     "package main\npub fn main() { wok(match [1,2,3] { [1, x, 3] => x, _ => -1 }); wok(match [1,2,3] { [9, x, 3] => x, _ => -1 }) }"),

    // ── Destructuring di `let` (Phase 5F-F) ──────────────────────────────
    ("destructuring: tuple",
     "package main\nfn p() { return (1, 2) }\npub fn main() { let (a, b) = p(); wok(a); wok(b) }"),
    ("destructuring: array dan sisa",
     "package main\npub fn main() { let [x, y, z] = [10,20,30]; wok(x+y+z); let [h, ...t] = [1,2,3,4]; wok(h); wok(t) }"),

    // ── Template literal (Phase 5F-F) ────────────────────────────────────
    ("template: teks dan ekspresi",
     "package main\npub fn main() { let n = 5; let s = \"dunia\"; wok(«halo ${s}, n=${n}»); wok(«hitung: ${n * 2 + 1}») }"),
    ("template: bersarang dan nilai majemuk",
     "package main\nstruct P { pub x: int = 1 }\npub fn main() { let n = 5; wok(«luar ${«dalam ${n}»}»); wok(«${[1,2]} ${(1,2)} ${P{}} ${Some(3)} ${1.5} ${nil}») }"),
    ("template: selalu menghasilkan string",
     "package main\npub fn main() { let n = 5; wok(typeof(«${n}»)) }"),

    // ── Dict (Phase 5F-F) ────────────────────────────────────────────────
    ("dict: literal, index, len",
     "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}; wok(d); wok(d[\"a\"]); wok(len(d)); wok(d.len()) }"),
    ("dict: keys/values/items terurut",
     "package main\npub fn main() { let d = {\"zebra\": 1, \"apel\": 2, \"mangga\": 3}; wok(dict::keys(d)); wok(dict::values(d)); wok(dict::items(d)) }"),
    ("dict: get mengembalikan Option, set murni",
     "package main\npub fn main() { let d = {\"a\": 1}; wok(dict::get(d, \"a\")); wok(dict::get(d, \"zz\")); wok(dict::set(d, \"b\", 2)); wok(d) }"),
    ("dict: has/remove/merge/is_empty/is_dict",
     "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}; wok(dict::has(d,\"a\")); wok(dict::remove(d,\"a\")); wok(dict::merge({\"a\":1},{\"b\":2})); wok(dict::is_empty(dict::new())); wok(dict::is_dict(d)); wok(dict::is_dict(1)) }"),
    // Sisi POSITIF pintu dict dan jalur tulis (Fase KUNCI): yang benar tetap jalan.
    ("dict: get_or bercadangan, from_pairs menerima tuple, is_dict atas struct dan enum",
     "package main\nstruct P { pub a: int }\npub fn main() { let d = {\"a\": 1}\n wok(dict::get_or(d, \"z\", 7), dict::get_or(d, \"a\", 7))\n wok(dict::from_pairs([(\"x\", 1), [\"y\", 2]]))\n wok(dict::is_dict(P { a: 1 }), dict::is_dict({}), dict::is_dict(Option::Some(1))) }"),
    ("tulis: jalur bersarang yang ADA tetap jalan di tiap bentuk",
     "package main\nstruct Q { pub x: int }\nstruct P { pub a: int, pub q: Q }\npub fn main() { var p = P { a: 1, q: Q { x: 2 } }\n p.q.x = 5\n var d = {\"a\": {\"b\": 1}}\n d.a.b = 7\n d[\"a\"][\"b\"] += 1\n var e = [p]\n e[0].q.x = 9\n e[-1].a = 4\n wok(p, d, e) }"),
    ("for: isi tuple tetap bisa diiterasi lewat index-nya",
     "package main\npub fn main() { let t = ([1, 2], 3)\n for i, x in t[0] { wok(i, x) }\n wok(t[1], len(t)) }"),

    // ── Rune adalah tipe tersendiri (Phase 5F-F) ─────────────────────────
    ("rune: tipe, banding, dan cast",
     "package main\npub fn main() { let r = 'o'; wok(r); wok(typeof(r)); wok(r as int); wok(r == 'o'); wok(r == \"o\"); wok(r == 111) }"),
    ("rune: indexing string menghasilkan rune",
     "package main\npub fn main() { let s = \"halo\"; wok(s[3]); wok(typeof(s[3])); wok(s[3] == 'o'); wok(\"halo\"[-1] == 'o') }"),

    // ── Slice (Phase 5F-F) ───────────────────────────────────────────────
    ("slice: batas negatif dan kosong",
     "package main\npub fn main() { let a=[1,2,3,4,5]; wok(a[1:3]); wok(a[1:-1]); wok(a[-2:]); wok(a[:2]); wok(a[2:]); wok(a[2:2]); wok(a[-5:-1]) }"),
    ("slice: string",
     "package main\npub fn main() { let s=\"halo\"; wok(s[1:3]); wok(s[-2:]) }"),

    // ── Nilai unit (Phase 5F-F) ──────────────────────────────────────────
    ("unit: fungsi tanpa return",
     "package main\nfn kosong() { let x = 1 }\npub fn main() { let u = kosong(); wok(u); wok(typeof(u)); wok(u == nil) }"),
    ("unit: if tanpa else sebagai nilai",
     "package main\npub fn main() { let n = 1; let r = if n > 3 { \"a\" }; wok(r); wok(typeof(r)) }"),
    ("unit: blok tanpa ekor dan return telanjang",
     "package main\nfn bare() { return }\npub fn main() { wok(typeof(bare())); let b = { let x = 1 }; wok(typeof(b)) }"),

    // ── typeof untuk nilai berbasis Map (Phase 5F-F) ─────────────────────
    ("typeof: struct, enum, dict",
     "package main\nstruct P { pub x: int = 1 }\nenum E { A, B(int) }\npub fn main() { wok(typeof(P{})); wok(typeof(Some(1))); wok(typeof(Ok(1))); wok(typeof(E::A)); wok(typeof({\"a\":1})) }"),

    // ── Cast (Phase 5F-F) ────────────────────────────────────────────────
    ("cast: as float benar-benar mengubah tipe",
     "package main\npub fn main() { let a = 3 as float; wok(a); wok(typeof(a)); wok(a / 2) }"),

    // ── Operator (Phase 5F-F) ────────────────────────────────────────────
    ("operator: ! hanya untuk bool, ~ untuk bit",
     "package main\npub fn main() { wok(!true); wok(!false); wok(~12) }"),
    ("operator: pembagian float mengikuti IEEE",
     "package main\npub fn main() { wok(1.0/0.0); wok(-1.0/0.0); wok(0.0/0.0 == 0.0/0.0) }"),

    // ── Semantik pemanggilan method (Phase 5F-E) ─────────────────────────
    ("method: penerima disalin, mutasi tidak bocor",
     "package main\nstruct Akun { pub saldo: int = 0 }\nimpl Akun { fn setor(self, n) { self.saldo = self.saldo + n } }\npub fn main() { var a = Akun { saldo: 10 }; a.setor(5); wok(a.saldo) }"),
    ("method: mutasi field array lewat method tidak bocor",
     "package main\nstruct K { pub d: []int = [] }\nimpl K { fn set(self) { self.d[0] = 77 } }\npub fn main() { var k = K { d: [1,2] }; k.set(); wok(k.d[0]) }"),
    // Penerima dievaluasi SEKALI. VM dulu mengompilasi ekspresinya dua kali.
    ("method: penerima dievaluasi sekali",
     "package main\nvar n = 0\nstruct K { pub v: int = 0 }\nimpl K { fn ambil(self) { return self.v } }\nfn bikin() { n = n + 1; return K { v: 5 } }\npub fn main() { let r = bikin().ambil(); wok(r); wok(n) }"),

    // ── math:: dan arr:: (Phase 5F-F) ────────────────────────────────────
    ("math::abs mempertahankan tipe",
     "package main\npub fn main() { wok(math::abs(-5)); wok(typeof(math::abs(-5))); wok(math::abs(-5.5)) }"),
    ("math:: kelengkapan",
     "package main\npub fn main() { wok(math::gcd(12,18)); wok(math::lcm(4,6)); wok(math::sign(-3)); wok(math::trunc(1.7)); wok(math::hypot(3.0,4.0)); wok(math::degrees(3.141592653589793)); wok(math::is_nan(0.0/0.0)) }"),
    ("arr:: kelengkapan tanpa closure",
     "package main\npub fn main() { let a=[3,1,2,1]; wok(arr::first(a)); wok(arr::last(a)); wok(arr::min(a)); wok(arr::max(a)); wok(arr::unique(a)); wok(arr::take(a,2)); wok(arr::chunks([1,2,3,4,5],2)); wok(arr::zip([1,2],[\"a\",\"b\"])); wok(arr::enumerate([\"a\",\"b\"])); wok(arr::median([1,2,3])) }"),
    ("arr:: kelengkapan dengan closure",
     "package main\npub fn main() { let a=[3,1,2,1]; wok(arr::count(a, |x| x == 1)); wok(arr::find_idx(a, |x| x == 2)); wok(arr::sort_by(a, |x,y| y - x)); wok(arr::partition([1,2,3,4], |x| x % 2 == 0)); wok(arr::zip_with([1,2],[10,20], |x,y| x+y)); wok(arr::flat_map([1,2], |x| [x,x])) }"),
    // ── Label pada break/continue (Phase 5F-G) ───────────────────────────
    ("label: break menembus beberapa loop",
     "package main\npub fn main() { var s = 0; 'luar: for i in 0..3 { for j in 0..3 { if i == 2 { break 'luar }; if j == 1 { continue 'luar }; s = s + 1 } }; wok(s) }"),
    ("label: pada while dan loop",
     "package main\npub fn main() { var t = 0; 'w: while true { for k in 0..5 { if k == 2 { break 'w }; t = t + 1 } }; wok(t); var u = 0; 'l: loop { u = u + 1; if u > 2 { break 'l } }; wok(u) }"),
    ("label: continue berlabel melanjutkan loop luar",
     "package main\npub fn main() { var s = 0; 'a: for i in 0..3 { for j in 0..3 { if j == 1 { continue 'a }; s = s + 10 } }; wok(s) }"),

    // ── Fungsi bersarang (Phase 5F-G) ────────────────────────────────────
    ("fn bersarang: dasar dan rekursi",
     "package main\npub fn main() { fn lipat(n) { return n * 3 }; wok(lipat(5)); fn fakt(n) { if n <= 1 { return 1 }; return n * fakt(n - 1) }; wok(fakt(5)) }"),
    ("fn bersarang: hoisted (dipanggil sebelum dideklarasikan)",
     "package main\nfn f() { let r = kedua(1); fn kedua(x) { return x + 100 }; return r }\npub fn main() { wok(f()) }"),
    ("fn bersarang: rekursi mutual",
     "package main\npub fn main() { fn genap(n) { if n == 0 { return true }; return ganjil(n - 1) }; fn ganjil(n) { if n == 0 { return false }; return genap(n - 1) }; wok(genap(10)); wok(ganjil(7)) }"),
    ("fn bersarang: nama sama di fungsi berbeda tetap terpisah",
     "package main\nfn a() { fn f() { return 1 }; return f() }\nfn b() { fn f() { return 2 }; return f() }\npub fn main() { wok(a()); wok(b()) }"),
    ("fn bersarang: bisa jadi nilai dan lihat fungsi top-level",
     "package main\nfn dua(x) { return x * 2 }\npub fn main() { fn tiga(n) { return n * 3 }; let f = tiga; wok(f(4)); wok([1,2,3].map(tiga)); fn pakai(n) { return dua(n) + 1 }; wok(pakai(10)) }"),

    // ── Nilai default parameter (Phase 5F-G) ─────────────────────────────
    ("default: dipakai dan ditimpa",
     "package main\nfn f(a, b = 10) { return a + b }\nfn g(a, b = 1, c = 2) { return a * 100 + b * 10 + c }\npub fn main() { wok(f(1)); wok(f(1,2)); wok(g(1)); wok(g(1,5)); wok(g(1,5,9)) }"),
    ("default: boleh merujuk parameter sebelumnya dan global",
     "package main\nvar G = 7\nfn h(a, b = a + 1) { return b }\nfn k(a = G) { return a }\npub fn main() { wok(h(3)); wok(k()); wok(k(99)) }"),
    ("default: nil eksplisit berbeda dari tidak diisi",
     "package main\nfn f(a = 5) { return a }\npub fn main() { wok(f()); wok(f(nil)) }"),

    // ── defer membekukan argumennya (Phase 5F-G) ─────────────────────────
    ("defer: argumen dibekukan di baris defer",
     "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\nfn beku() { var n = 1; defer catat(n); n = 99 }\npub fn main() { beku(); wok(jejak) }"),
    ("defer: urutan LIFO",
     "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\nfn f() { defer catat(\"A\"); defer catat(\"B\"); catat(\"body\") }\npub fn main() { f(); wok(jejak) }"),

    // ── jawa::/jawir:: (Phase 5F-G) ──────────────────────────────────────
    ("jawa: alloc/get/set/is_valid",
     "package main\npub fn main() { let p = jawa::alloc(42); wok(jawa::get(p)); let _ = jawa::set(p, 99); wok(jawa::get(p)); wok(jawa::is_valid(p)); let _ = jawir::free(p); wok(jawa::is_valid(p)) }"),
    ("jawa: array",
     "package main\npub fn main() { let a = jawa::array(3, 0); let _ = jawa::array_set(a, 1, 7); let _ = jawa::array_push(a, 9); wok(jawa::array_len(a)); wok(jawa::array_to_list(a)); wok(jawa::array_pop(a)) }"),
    ("jawa: buffer",
     "package main\npub fn main() { let b = jawa::buffer(4); let _ = jawa::buffer_set(b, 0, 255); wok(jawa::buffer_to_hex(b)); let _ = jawa::buffer_fill(b, 1); wok(jawa::buffer_to_hex(b)); wok(jawa::buffer_to_hex(jawa::buffer_from_hex(\"dead\"))) }"),
    ("jawa: bentuk telanjang",
     "package main\npub fn main() { let p = jawa(7); wok(jawa::get(p)); let _ = jawir(p); wok(jawa::is_valid(p)) }"),

    // ── FFI `extern "C"` (Phase 5F-H) ────────────────────────────────────
    ("ffi: panggilan libc dan libm",
     "package main\nextern \"C\" { fn abs(n: i32) -> i32\n fn strlen(s: string) -> i64 }\nextern \"C\" \"libm.so.6\" { fn sqrt(x: f64) -> f64\n fn pow(b: f64, e: f64) -> f64 }\npub fn main() { wok(abs(-42)); wok(strlen(\"halo\")); wok(sqrt(16.0)); wok(pow(2.0, 10.0)) }"),
    ("ffi: buffer native malloc/memset/free",
     "package main\nextern \"C\" { fn malloc(n: u64) -> *mut u8\n fn free(p: *mut u8) -> unit\n fn memset(p: *mut u8, c: i32, n: u64) -> *mut u8\n fn strlen(s: *const u8) -> i64 }\npub fn main() { match malloc(16) { Result::Ok(b) => { let _ = memset(b, 65, 15); wok(strlen(b)); let _ = free(b) } Result::Err(e) => wok(e) } }"),
    ("ffi: simbol tidak ada jadi Err, bukan crash",
     "package main\nextern \"C\" { fn simbol_yang_tidak_pernah_ada(n: i32) -> i32 }\npub fn main() { match simbol_yang_tidak_pernah_ada(1) { Result::Ok(v) => wok(\"ok\", v), Result::Err(_) => wok(\"err seperti seharusnya\") } }"),
    ("ffi: agregat ditolak dengan pesan yang sama",
     "package main\nextern \"C\" { fn strlen(s: []int) -> i64 }\npub fn main() { wok(strlen([1,2,3])) }"),
    ("ffi: function pointer ditolak dengan pesan yang sama",
     "package main\nextern \"C\" { fn atexit(cb: fn() -> unit) -> i32 }\npub fn main() { let f = || 1; wok(atexit(f)) }"),
    ("ffi: batas 3 parameter ditolak dengan pesan yang sama",
     "package main\nextern \"C\" { fn foo(a: i32, b: i32, c: i32, d: i32) -> i32 }\npub fn main() { wok(foo(1,2,3,4)) }"),
    ("ffi: handle ptr:: bukan alamat native",
     "package main\nextern \"C\" { fn strlen(s: *const u8) -> i64 }\npub fn main() { let p = ptr::new(42); wok(strlen(p)) }"),

    // ── Presisi numerik (Phase 5F-G) ─────────────────────────────────────
    ("numerik: melewati batas 64 bit",
     "package main\npub fn main() { var n = 9223372036854775807; wok(n + 1); wok(4000000000 * 4000000000); wok(2 ** 100) }"),
    ("numerik: literal 128 bit",
     "package main\npub fn main() { wok(170141183460469231731687303715884105727) }"),
    ("numerik: pergeseran bit lebar penuh",
     "package main\npub fn main() { wok(1 << 100); wok((1 << 100) >> 50) }"),
    ("numerik: float presisi dan pembulatan",
     "package main\npub fn main() { wok(0.1 + 0.2); wok(1.0 / 3.0); wok(2.0 ** 0.5); wok(1e308 * 10) }"),

    ("str:: kelengkapan",
     "package main\npub fn main() { wok(str::chars(\"abc\")); wok(str::index_of(\"halo\",\"lo\")); wok(str::join([\"a\",\"b\"],\"-\")); wok(str::lines(\"a\\nb\")); wok(str::capitalize(\"halo\")); wok(str::snake_case(\"HaloDunia\")); wok(str::pad_left(\"7\",3,\"0\")); wok(str::is_numeric(\"12\")) }"),

    // ── Copy-on-write ────────────────────────────────────────────────────
    ("cow: bersarang dua lapis",
     "package main\npub fn main() { var a = [[1,2],[3,4]]; var b = a; b[0][0] = 99; wok(a[0][0]); wok(b[0][0]) }"),
    ("cow: bersarang tiga lapis",
     "package main\npub fn main() { var a = [[[1]]]; var b = a; b[0][0][0] = 42; wok(a[0][0][0]); wok(b[0][0][0]) }"),
    ("cow: elemen dalam diambil keluar",
     "package main\npub fn main() { var a = [[1,2],[3,4]]; var d = a[0]; d[0] = 55; wok(a[0][0]); wok(d[0]) }"),
    ("cow: array di dalam struct",
     "package main\nstruct K { pub isi: []int }\npub fn main() { var k = K{isi:[1,2]}; var j = k; j.isi[0] = 9; wok(k.isi[0]); wok(j.isi[0]) }"),
    ("cow: dict berbagi lalu ditulis",
     "package main\npub fn main() { var d = {\"k\":[1,2]}; var e = d; e[\"k\"][0] = 33; wok(d[\"k\"][0]); wok(e[\"k\"][0]) }"),
    ("cow: banyak pemegang satu penulis",
     "package main\npub fn main() { var a = [1,2,3]; var p = a; var q = a; q[0] = 100; wok(a[0]); wok(p[0]); wok(q[0]) }"),
    ("cow: penerima method tidak ikut berubah",
     "package main\nstruct A { pub n: int }\nimpl A { fn naik(self, k) { self.n = self.n + k; return self.n } }\npub fn main() { var a = A{n:1}; wok(a.naik(5)); wok(a.n) }"),
    // Jalur cepat `a = a + [x]` mengosongkan binding sementara. Empat kasus
    // di bawah adalah cara lubang itu bisa terlihat kalau penjagaannya salah.
    ("cow: tumbuh dengan diri sendiri",
     "package main\npub fn main() { var a = [1,2]; a = a + a; wok(a); a += a; wok(len(a)) }"),
    // `var` modul dan tangkapan closure tidak bisa dikosongkan: mereka disambung di tempat hanya
    // kalau masih memegang penyimpanan yang tadi dibaca. Tiap kasus = satu cara itu bisa meleset.
    ("tumbuh: kanan mengganti var modul",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\nfn tulis() { A = [100]; return [7] }\npub fn main() { A = A + tulis(); wok(A); A += tulis(); wok(A); A = A.push(tulis()); wok(A) }"),
    ("tumbuh: kanan menulis elemen var modul",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\nfn ubah() { A[0] = 55; return [8] }\npub fn main() { A = A + ubah(); wok(A); A = A.push(ubah()); wok(A); A = A.concat(ubah()); wok(A) }"),
    ("tumbuh: kanan membaca var modul",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\nfn baca() { return [len(A)] }\npub fn main() { A = A + baca(); A += baca(); A = A.push(baca()); A = A + [2] + baca(); A = A + A; wok(A) }"),
    ("tumbuh: var modul berganti jenis di tengah rantai",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\npub fn main() { A = A + [1] + \"x\"; wok(A, typeof(A)); A = [1]; A += \"y\"; wok(A) }"),
    ("tumbuh: pemegang lama var modul tidak terseret",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\npub fn main() { let b = A; A = A + [9]; let c = A; A += [8]; let d = A; A = A.push(7); wok(A, b, c, d) }"),
    ("tumbuh: argumen push var modul dihitung sekali",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\nfn f() { N = N + 1; return N }\npub fn main() { A = A.push(f()); A = arr::push(A, f()); wok(A, N); var h = []; let g = || { h = h.push(f()) }; g(); wok(h, N) }"),
    ("tumbuh: pop dan reverse atas var modul",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\npub fn main() { A = A + [2, 3]; A = arr::pop(A)[0]; wok(A); A = A.reverse(); wok(A); let b = A; A = A.reverse(); wok(A, b) }"),
    ("tumbuh: tangkapan closure, tiga bentuk",
     "package main\npub fn main() { var h = [1]; let g = |x| { h = h + [x]; h = h.push(x * 2); h += [x * 3] }; g(1); let k = h; g(2); wok(h, k) }"),
    ("tumbuh: lokal milik pembuat closure",
     "package main\npub fn main() { var h = [1]; let f = || { h = [0]; return [7] }; let g = || h; h = h + [2]; let k = h; h = h.push(3); h = h + [4] + [5]; wok(h, k, g()); h = h + f(); wok(h); h = h.push(f()); wok(h); h = h.concat(g()); wok(h) }"),
    ("tumbuh: field var modul, kanan mengganti dan menulisnya",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\nfn ganti() { G.xs = [9, 9]; return [3] }\nfn ubah() { G.xs[0] = 4; return [5] }\npub fn main() { G.xs = G.xs + [1]; G.xs = G.xs + ganti(); wok(G.xs); G.xs = G.xs + ubah(); wok(G.xs); G.xs = G.xs.push(ganti()); wok(G.xs); let c = G.xs; G.xs = G.xs + [0]; wok(G.xs, c) }"),
    ("tumbuh: teks di field var modul",
     "package main\nstruct P { pub xs: array, pub s: string }\nvar A = [1]\nvar G = P { xs: [], s: \"\" }\nvar N = 0\nfn f() { G.s = \"z\"; return \"b\" }\npub fn main() { G.s = G.s + \"a\"; G.s = G.s + f(); wok(G.s); G.s = G.s + 1; wok(G.s) }"),
    ("tumbuh: index jalur berubah selagi kanan dihitung",
     "package main\npub fn main() { var m = [[1], [1]]; var i = 0; let naik = || { i = 1; return [5] }; m[i] = m[i] + naik(); wok(m); i = 0; m[i] = m[i].push(naik()); wok(m) }"),
    ("tumbuh: push lewat jalur",
     "package main\nstruct P { pub xs: array }\npub fn main() { var p = P { xs: [1] }; p.xs = p.xs.push(2); p.xs = p.xs.push(p.xs); wok(p.xs); var d = {\"a\": [1]}; d[\"a\"] = d[\"a\"].push(2); d[\"a\"] = d[\"a\"] + [3]; d[\"a\"] += [4]; wok(d) }"),
    ("tumbuh: push milik pemakai lewat jalur tetap dipanggil",
     "package main\nstruct K { pub n: int }\nimpl K { fn push(self, x) { self.n = self.n + x; return self } }\nstruct W { pub k: K, pub t: string }\npub fn main() { var w = W { k: K { n: 1 }, t: \"a\" }; w.k = w.k.push(5); wok(w.k.n); w.t = w.t + \"b\"; wok(w.t) }"),
    ("tumbuh: field privat di dalam method",
     "package main\nstruct P { xs: array }\nimpl P { fn baru() { return P { xs: [] } }\n fn a(self, x) { self.xs = self.xs + [x]; return self }\n fn b(self, x) { self.xs += [x]; return self }\n fn c(self, x) { self.xs = self.xs.push(x); return self }\n fn isi(self) { return self.xs } }\npub fn main() { var p = P::baru(); p = p.a(1); let q = p; p = p.b(2); p = p.c(3); wok(p.isi(), q.isi()) }"),
    // Fase PACU-4: jalur cepat VM (bilangan bulat di tempat, `SimpanBuang`, index atas lokal).
    ("pacu: bilangan bulat di tempat, tepi",
     "package main\npub fn main() { let b = 9223372036854775807; wok(b * b, b + b, -b - b, 7 % -1, -7 % 3, 7 % -3, (b + 1) % 10, b * -1 % 7); wok(1 < 2, 2 <= 2, 3 > 4, 4 >= 5, 5 == 5, 5 != 5); var x = 170141183460469231731687303715884105727; x = x - 1; wok(x % 1000, x == x, x > 0) }"),
    ("pacu: penugasan lokal langsung",
     "package main\npub fn main() { var a = [1, 2]; var b = a; b = [9]; wok(a, b); var n = 1; let f = || { n = n + 1; return n }; n = f() + n; wok(n); var s = \"a\"; s = s; wok(s); var t = (1, 2); t = t; wok(t); for i in 3 { var k = i; k = k * 2; wok(k) } }"),
    ("pacu: index lokal baca dan tulis",
     "package main\npub fn main() { var a = [1, 2, 3]; let b = a; var i = 1; a[i] = a[i] + 10; a[0] = a[-1]; a[-1] = 7; wok(a, b); var d = {\"k\": 1}; let e = d; d[\"k\"] = d[\"k\"] + 1; d[\"z\"] = 5; wok(d, e); var t = (1, 2); t[0] = 9; wok(t, t[1]); let s = \"halo\"; wok(s[0], s[-1]) }"),
    ("pacu: index lokal tertangkap closure",
     "package main\npub fn main() { var a = [1, 2, 3]; let f = || a[0]; let g = |v| { a[1] = v }; a[0] = 5; g(8); let c = a; a[2] = a[0] + a[1]; wok(a, c, f()) }"),
    ("pacu: index lokal bersarang dan salinan",
     "package main\npub fn main() { var m = [[1, 2], [3, 4]]; let k = m; var r = m[0]; r[0] = 99; m[1] = r; wok(m, k, r); var i = 0; m[i] = m[1]; wok(m) }"),
    ("pacu: a[i] = a[i] + x atas lokal",
     "package main\nstruct V { pub n: int }\nimpl V { fn add(self, o) { return V { n: self.n + o.n } } }\npub fn main() { var a = [1, 2]; var i = 0; a[i] = a[i] + 5; a[1] = a[1] + a[0]; wok(a); var m = [[1], [2]]; let k = m; let e = m[0]; m[i] = m[i] + [9]; m[1] = m[1] + m[1]; m[0] = m[0] + m[1]; wok(m, k, e); var d = {\"a\": [1], \"s\": \"x\"}; let j = \"a\"; d[j] = d[j] + [2]; d[\"s\"] = d[\"s\"] + 1; wok(d); var v = [V { n: 1 }]; v[0] = v[0] + V { n: 4 }; wok(v[0].n); var t = [\"a\", 1.5]; t[0] = t[0] + t; t[1] = t[1] + 1; wok(t) }"),
    ("pacu: a[i] = a[i] + f() yang menulis a",
     "package main\npub fn main() { var m = [[1], [2]]; let f = || { m[0] = [7]; return [3] }; let g = || { m = [[0]]; return [4] }; m[0] = m[0] + f(); wok(m); m[0] = m[0] + g(); wok(m); var c = [[1]]; let h = || c; c[0] = c[0] + h()[0]; wok(c, h()) }"),
    // Fase TUNGGU-LLVM: `import "std/fmt"` (namespace bawaan) dilewati — dulu LLVM menolaknya.
    ("import: namespace bawaan lewat std/",
     "package main\nimport \"std/fmt\"\npub fn main() { fmt::wok(\"a\", 1)\n wok(fmt::sprint(\"x\", 2)) }\n"),
    // Fase TUNGGU-LLVM: `main` ber-`defer` tetap menunggu goroutine — dulu LLVM membuang keluarannya.
    ("goroutine: main ber-defer menunggu goroutine sesudah defer-nya",
     "package main\npub fn main() {\n    defer wok(\"defer main\")\n    spawn { time::sleep_ms(100)\n wok(\"goroutine\") }\n    wok(\"akhir main\")\n}\n"),
    // Fase WARNA: `Warna` pustaka berbentuk tetap (r, g, b wajib, a = 255) di tiga mesin.
    ("warna: struct pustaka berbentuk tetap",
     "package main\nfn hijau(w: Warna) -> int { return w.g }\npub fn main() {\n    let w = Warna { r: 1, g: 2, b: 3 }\n    wok(w, typeof(w), hijau(w), w.a)\n    let v = Warna { r: 1, g: 2, b: 3, a: 4 }\n    wok(v, v == w, Warna { r: 1, g: 2, b: 3, a: 255 } == w)\n}\n"),
    // Fase URUTAN-LLVM: sisi kanan penugasan dievaluasi sebelum indeks dan field sasarannya.
    ("urutan: sisi kanan sebelum tempat sasaran",
     "package main\nstruct P { pub xs: []int }\nfn i() -> int { wok(\"indeks\"); return 0 }\nfn v() -> int { wok(\"nilai\"); return 9 }\nfn k() -> string { wok(\"kunci\"); return \"a\" }\npub fn main() {\n    var a = [1, 2]\n    a[i()] = v()\n    a[i()] += v()\n    var b = [[1], [2]]\n    b[i()][i()] = v()\n    wok(a, b)\n    var d = {\"a\": [1]}\n    let g = || { d = {\"a\": [0]}; return [5] }\n    d[\"a\"] = d[\"a\"] + g()\n    d[k()] = [9]\n    wok(d)\n    var p = P { xs: [1] }\n    let h = || { p = P { xs: [] }; return 2 }\n    p.xs = [h()]\n    wok(p)\n}\n"),
    // Fase SINKRON: `Atomic`/`Mutex` sebagai pegangan sungguhan di dua mesin.
    ("sinkron: atomic dasar",
     "let a = Atomic::new(5)\nwok(a, typeof(a), atomic::get(a))\nwok(atomic::set(a, 7), atomic::add(a, 3), atomic::sub(a, 1), atomic::cas(a, 9, 1), atomic::cas(a, 9, 2), atomic::get(a))\nlet b = a\natomic::add(b, 10)\nwok(atomic::get(a), a == b, a == Atomic::new(11), atomic::get(Atomic::new()))\nlet xs = [a, a]\natomic::add(xs[1], 1)\nwok(atomic::get(xs[0]), [a] == [a], {\"a\": a})\n"),
    ("sinkron: atomic dari empat goroutine",
     "let a = Atomic::new(0)\nlet c = Chan::new(4)\nfor i in 0..4 { spawn { for j in 0..500 { atomic::add(a, 1) }\n c <- 1 } }\nfor i in 0..4 { <-c }\nwok(atomic::get(a))\n"),
    ("sinkron: mutex menjaga baca-lalu-tulis",
     "let m = Mutex::new()\nwok(m, typeof(m), mutex::lock(m), mutex::unlock(m))\nlet p = ptr::new(0)\nlet c = Chan::new(4)\nfor i in 0..4 { spawn { for j in 0..300 { mutex::lock(m)\n ptr::set(p, ptr::get(p) + 1)\n mutex::unlock(m) }\n c <- 1 } }\nfor i in 0..4 { <-c }\nwok(ptr::get(p), m == m, m == Mutex::new())\n"),
    ("sinkron: with_lock menjawab hasil dan melepas",
     "let m = Mutex::new()\nwok(mutex::with_lock(m, || 42), mutex::with_lock(m, || mutex::with_lock(Mutex::new(), || \"dalam\")))\nmutex::lock(m)\nmutex::unlock(m)\nlet f = mutex::with_lock\nwok(f(m, || 3), terap(mutex::with_lock, [m, || 4]))\nlet a: Atomic = Atomic::new(1)\nlet k: Mutex = m\nwok(atomic::get(a), k == m)\n"),
    ("sinkron: with_lock melepas juga kalau fungsinya gagal",
     "package main\nfn gagal(m) { mutex::with_lock(m, || arr::get([1], 5).unwrap()) }\npub fn main() { let m = Mutex::new(); let t = Task::spawn(gagal, m); time::sleep_ms(150); wok(mutex::with_lock(m, || \"bebas lagi\")) }\n"),
    ("sinkron: Atomic dan Mutex lewat bentuk method",
     "package main\nstruct K { pub a: Atomic, pub m: Mutex }\npub fn main() { let a = Atomic::new(5)\n wok(a.get(), a.add(2), a.sub(1), a.cas(6, 10), a.cas(6, 11), a.get(), a.to_string())\n a.set(3)\n let m = Mutex::new()\n m.lock()\n a.add(1)\n m.unlock()\n wok(m.with_lock(|| a.add(10)), a.get())\n let k = K { a: Atomic::new(1), m: Mutex::new() }\n k.m.lock()\n wok(k.a.add(4))\n k.m.unlock()\n wok(k.m.with_lock(|| k.a.get() * 2)) }\n"),
    ("anotasi: Warna adalah nama tipe pustaka",
     "package main\nfn terang(w: Warna) -> Warna { Warna { r: w.r + 1, g: w.g, b: w.b, a: w.a } }\npub fn main() { let w: Warna = Warna { r: 1, g: 2, b: 3, a: 255 }\n wok(w.r, terang(w).r) }\n"),
    ("handle: struct program bernama sama dengan handle pustaka tetap bisa ditulisi",
     "package main\nstruct Task { pub id: int, pub nama: string }\npub fn main() { var t = Task { id: 1, nama: \"a\" }\n t.id = 5\n t.nama += \"b\"\n wok(t) }\n"),
    ("struct: Self di dalam method adalah tipe pemiliknya",
     "package main\nstruct P { pub x: int }\nimpl P { fn baru() -> P { return Self { x: 3 } }\n fn ganda(self) -> int { return self.x * 2 } }\npub fn main() { let p = P::baru(); wok(p, typeof(p), p.ganda()) }\n"),
    ("struct: bentuk literal yang sah tetap jalan",
     "package main\nstruct Dua(int, int)\nenum E { V { a: int }, W }\nstruct Kotak<T> { pub v: T }\npub fn main() { wok(Dua(1, 2), E::V { a: 1 }, V { a: 2 }, Kotak { v: \"a\" }, dict { }, Warna { r: 1, g: 2, b: 3 }) }\n"),
    ("tugas: handle disalin dan diserahkan sekali, saluran dalamnya buram",
     "let t = Task::spawn(|| 5)\nwok(typeof(t._ch), t)\nwok(task::join(t))\nlet f = Future::new(|| 6)\nlet g = f\nwok(future::await(g), future::is_done(f))\n"),
    // Fase PACU-5: tepi `dok::num_*`, `ptr::update`, port HTTP, JSON, dan putaran `for`.
    ("angka: dok::num_* di tepi 128 bit",
     "wok(dok::num_mod_pow(1267650600228229401496703205376, 2, 1329227995783706947084195916825640961), dok::num_mod_pow(3, 170141183460469231731687303715884105726, 170141183460469231731687303715884105727))\nwok(dok::num_is_prime(170141183460469231731687303715884105727), dok::num_is_prime(170141183460469231731687303715884105725), dok::num_is_prime(18446744073709551557))\nwok(dok::num_next_prime(170141183460469231731687303715884105000), dok::num_prime_factors(170141183460469231731687303715884105727), dok::num_prime_factors(2 ** 100 + 1))\nwok(dok::num_to_base(-170141183460469231731687303715884105728, 10), dok::num_from_base(\"-170141183460469231731687303715884105728\", 10))\n"),
    ("angka: dok::num_* kecil dan bertanda",
     "wok(dok::num_mod_pow(2, 10, 1000), dok::num_mod_pow(-2, 3, 5), dok::num_mod_pow(2, 3, -5), dok::num_mod_pow(7, 0, 1))\nwok(dok::num_next_prime(-5), dok::num_next_prime(13), dok::num_next_prime(1000000000000000000))\nwok(dok::num_is_prime(2), dok::num_is_prime(1), dok::num_is_prime(561), dok::num_is_prime(3215031751))\nwok(dok::num_prime_factors(360), dok::num_prime_factors(1000000007 * 998244353), dok::num_prime_factors(97), dok::num_prime_factors(1))\nwok(dok::num_to_base(255, 16), dok::num_to_base(-255, 2), dok::num_to_base(0, 7), dok::num_from_base(\"ff\", 16), dok::num_from_base(\"-zz\", 36))\n"),
    ("ptr: update memakai pointer LAIN di dalam fungsinya",
     "let p = ptr::new(1)\nlet q = ptr::new(5)\nwok(ptr::update(p, |x| ptr::get(q) + x))\nwok(ptr::update(p, |x| ptr::update(q, |y| y + x)))\nwok(ptr::get(p), ptr::get(q), ptr::dec(p), ptr::inc(p), ptr::inc(q))\n"),
    ("http: port tidak sah dijawab Err, bukan diam-diam 80",
     "wok(dok::http_get(\"http://127.0.0.1:99999/\"))\nwok(dok::http_get(\"http://127.0.0.1:0/\"))\nwok(dok::http_get(\"http://127.0.0.1:abc/x\"))\n"),
    ("json: pecahan besar yang masih muat",
     "wok(json::parse(\"[1.5, 2e3, -1e-300]\"), dok::json_decode(\"-1e999\"))\n"),
    ("pacu: for rentang dengan continue, break dan bersarang",
     "var s = 0\nfor i in 0..10 { if i % 2 == 0 { continue }\n if i > 7 { break }\n for j in 0..i { s = s + j } }\nwok(s)\nfor i, x in [5, 6, 7] { if x == 6 { continue }\n wok(i, x) }\nfor k, v in {\"a\": 1, \"b\": 2} { wok(k, v) }\nvar n = 0\nfor i in 5..=8 { n = n + i }\nwok(n)\nfor i in 3 { wok(i) }\n"),
    ("pacu: banding langsung diikuti lompatan",
     "var a = 3\nvar b = 5\nif a < b { wok(\"lt\") } else { wok(\"ge\") }\nif a >= b { wok(\"ge\") } else if a != b { wok(\"ne\") }\nwhile a <= b { a = a + 1 }\nwok(a, a == b, a > b && b > 0, a < b || b == 5)\nlet f = 1.5\nif f < 2 { wok(\"f\") }\nif \"a\" < \"b\" { wok(\"s\") }\nlet t = a == 6 $ \"enam\" : \"lain\"\nwok(t)\n"),
    ("cow: tumbuh lewat closure penangkap",
     "package main\npub fn main() { var a = [1]; let f = || a; a = a + f(); wok(a); var b = [7]; b = b + (|| b)(); wok(b) }"),
    ("cow: tumbuh pada binding tertangkap",
     "package main\npub fn main() { var a = [1]; let t = || { a = a + [9] }; t(); t(); wok(a) }"),
    ("cow: tumbuh tidak menyeret pemegang lama",
     "package main\npub fn main() { var a = [1,2]; var p = a; a = a + [3]; wok(a); wok(p) }"),
    ("cow: sisi kanan berefek samping tepat sekali",
     "package main\npub fn main() { var j = []; var a = [0]; let f = || { j = j + [\"x\"]; return [1] }; a = a + f(); wok(a); wok(j) }"),

    // ── Sisi POSITIF dari audit pasca-COW ───────────────────────────────
    ("audit: geser yang sah tetap jalan",
     "package main\npub fn main() { wok(1 << 0); wok(1 << 127); wok(-8 >> 2); wok(255 >> 4) }"),
    ("audit: factorial di bawah batas tetap jalan",
     "package main\npub fn main() { wok(math::factorial(0)); wok(math::factorial(20)); wok(math::factorial(33)) }"),
    ("audit: sort nilai sebanding tetap jalan",
     "package main\npub fn main() { wok(arr::sort([3,1,2])); wok(arr::sort([\"b\",\"a\"])); wok(arr::sort([3, 1.5, 2])); wok(arr::sort([])) }"),
    ("audit: cast uint tak-negatif tetap jalan",
     "package main\npub fn main() { wok(0 as uint); wok(5 as uint); wok(255 as u8); wok(1.9 as uint); wok(true as uint) }"),
    ("audit: default menoleh ke KIRI tetap jalan",
     "package main\nfn f(a, b = a * 2, c = b + 1) { return (a, b, c) }\npub fn main() { wok(f(3)); wok(f(3, 9)); wok(f(3, 9, 0)) }"),
    // Literal paling negatif tidak bisa ditulis sebagai negasi literal yang sah
    // — literalnya sendiri sudah kelewat satu.
    ("audit: literal integer paling negatif",
     "package main\npub fn main() { wok(-170141183460469231731687303715884105728); \
      wok(170141183460469231731687303715884105727) }"),
    // Float yang nilainya bulat tetap dicetak dengan `.0`.
    ("konsumsi: push tidak menyeret pemegang lama",
     "package main\npub fn main() { var a = [1,2]; var p = a; a = arr::push(a, 3); wok(a); wok(p); \
      var b = [1,2]; var q = b; b = b.push(9); wok(b); wok(q) }"),
    ("konsumsi: pop lewat indeks dan tuple penuh",
     "package main\npub fn main() { var a = [1,2,3]; a = arr::pop(a)[0]; wok(a); \
      var d = [1,2,3]; let r = arr::pop(d); wok(r); wok(d); \
      var e = []; e = arr::pop(e)[0]; wok(e); var f = [1]; wok(f.pop()) }"),
    ("konsumsi: concat, reverse, dan diri sendiri",
     "package main\npub fn main() { var a = [1]; a = arr::concat(a, [2,3]); wok(a); \
      var b = [1,2,3]; b = arr::reverse(b); wok(b); \
      var c = [1,2]; c = c.concat(c); wok(c); var d = [1,2]; d = arr::concat(d, d); wok(d) }"),
    // Bentuk yang sengaja TIDAK dilayani jalur cepat: penerima bukan target,
    // ada operasi lain di sekelilingnya, atau argumennya menyebut `a` —
    // semuanya harus jatuh ke jalur biasa dan tetap benar.
    ("konsumsi: bentuk di luar jalur cepat tetap benar",
     "package main\npub fn main() { var a = [1]; var b = arr::push(a, 2); wok(a); wok(b); \
      var c = [1]; c = arr::push(arr::push(c, 2), 3); wok(c); \
      var d = [1]; d = arr::push(d, len(d)); wok(d); \
      var e = [[1]]; e[0] = arr::push(e[0], 2); wok(e) }"),
    ("konsumsi: bersarang tidak bocor",
     "package main\npub fn main() { var k = [[1],[2]]; var m = k[0]; k = arr::push(k, [3]); \
      m = arr::push(m, 99); wok(k); wok(m) }"),
    // ── Stdlib VM yang kini juga jalan di interpreter (Fase JEMBATAN) ──
    ("jembatan: fs:: (murni-jalur, tanpa menyentuh berkas)",
     "package main\npub fn main() { wok(fs::basename(\"/a/b/c.txt\")); wok(fs::dirname(\"/a/b/c.txt\")); \
      wok(fs::join(\"a\", \"b\")); wok(fs::exists(\"/tentu/tidak/ada\")) }"),
    ("jembatan: json:: bolak-balik",
     "package main\npub fn main() { let d = json::parse(\"{\\\"b\\\":1,\\\"a\\\":[2,3]}\"); \
      wok(d); wok(json::stringify(d)); wok(json::keys(d)); wok(json::values(d)); \
      wok(json::has(d, \"a\")); wok(json::pretty(d)) }"),
    ("jembatan: re::",
     "package main\npub fn main() { wok(re::test(\"^a\", \"abc\")); wok(re::test(\"^b\", \"abc\")); \
      wok(re::replace_all(\"[0-9]\", \"a1b2\", \"#\")); wok(re::split(\",\", \"a,b,c\")); \
      wok(re::find_all(\"[0-9]\", \"a1b2\")); wok(re::match_str(\"[0-9]+\", \"x42\")) }"),
    ("jembatan: arr:: dan str:: yang dulu cuma di VM",
     "package main\npub fn main() { wok(arr::union([1,2],[2,3])); wok(arr::intersect([1,2],[2,3])); \
      wok(arr::difference([1,2],[2,3])); wok(arr::sort_desc([1,3,2])); \
      wok(arr::get([1,2,3], 0)); wok(arr::get([1,2,3], 9)); wok(arr::get_or([1,2,3], 9, -1)); \
      wok(str::len(\"abc\")); wok(str::to_uppercase(\"ab\")); wok(str::to_lowercase(\"AB\")) }"),
    ("jembatan: http:: yang tidak menyentuh jaringan",
     "package main\npub fn main() { wok(http::url_encode(\"a b&c=d\")); wok(http::url_decode(\"a%20b%26c\")) }"),
    // Keluaran JSON dulu mengikuti urutan iterasi HashMap, yang Rust acak per
    // proses — jadi `json::stringify` pada objek yang sama menjawab string …
    ("jembatan: keluaran json terurut, bukan urutan hash",
     "package main\npub fn main() { let d = json::parse(\"{\\\"z\\\":1,\\\"a\\\":2,\\\"m\\\":3}\"); \
      wok(json::stringify(d)); wok(json::keys(d)); wok(json::values(d)) }"),
    // Sisi POSITIF visibilitas field.
    ("lanjutan: operator boleh memimpin baris",
     "package main\npub fn main() { let a = [3,1,2]\n  let n = a\n    .len()\n  wok(n)\n        let t = 1 > 0\n    $ \"ya\"\n    : \"tidak\"\n  wok(t)\n        let b = true\n    && 1 < 2\n    && 3 > 2\n  wok(b)\n        let j = 1\n    + 2\n    + 3\n  wok(j)\n        let m = a\n    .map(|v| v * 2)\n    .len()\n  wok(m) }"),
    // Token yang bisa muncul di posisi PREFIX sengaja TIDAK menahan titik-koma:
    // baris yang dimulai dengannya memang bisa berdiri sendiri sebagai …
    ("lanjutan: token prefix TETAP berdiri sendiri",
     "package main\npub fn main() { var a = 1\n  -1\n  wok(a)\n        var b = 2\n  !true\n  wok(b)\n        var c = 3\n  ~1\n  wok(c)\n        var d = 4\n  (5)\n  wok(d)\n        var e = 5\n  [6]\n  wok(e)\n        var f = 6\n  || true\n  wok(f) }"),
    // `self` dan `continue` di akhir baris dulu tidak memicu titik-koma, jadi
    // `var g = self` lalu `g.a = 9` diurai jadi satu pernyataan dan ditolak.
    ("lanjutan: baris yang berakhir `self` atau `continue` selesai di situ",
     "package main\nstruct P { pub a: int }\nimpl P {\n  fn ubah(self) {\n    var g = self\n    g.a = 9\n    return g\n  }\n  fn salin(self) {\n    let x = self\n    x\n  }\n}\npub fn main() {\n  wok(P { a: 1 }.ubah(), P { a: 2 }.salin())\n  for _, i in [1, 2, 3] {\n    if i == 2 {\n      continue\n    }\n    wok(i)\n  }\n}"),
    // Kanari untuk `wokwok::`.
    ("wokwok: array numerik menjawab sama di dua backend",
     "package main\npub fn main() { let m = wokwok::reshape(wokwok::arange(6), [2,3]); \
      wok(m); wok(wokwok::sum(m, 0)); wok(wokwok::transpose(m)); \
      wok(wokwok::div(m, 2)); wok(wokwok::dot(wokwok::array([1,2]), wokwok::array([3,4]))); \
      wok(wokwok::det(wokwok::array([[1.0,2.0],[3.0,4.0]]))); \
      wok(typeof(m)); wok(wokwok::sum(wokwok::full([1000000], 0.1))) }"),
    ("visibilitas: akses sah dari dalam tipe sendiri",
     "package main\nstruct S { rahasia: int = 5, pub terbuka: int = 1 }\n\
      trait T { fn via_trait(self) -> int { return self.rahasia } }\n\
      impl T for S { }\n\
      struct Anak: S { }\n\
      impl S {\n\
        fn langsung(self) -> int { return self.rahasia }\n\
        fn via_closure(self) -> int { let f = || self.rahasia; return f() }\n\
        fn via_tulis(self) -> int { var k = self; k.rahasia = 9; return k.rahasia }\n\
      }\n\
      pub fn main() { let s = S{}; wok(s.terbuka); wok(s.langsung()); \
      wok(s.via_closure()); wok(s.via_tulis()); wok(s.via_trait()); \
      let a = Anak{}; wok(a.langsung()) }"),
    ("jembatan: ptr::inc/dec kini ada di dua backend",
     "package main\npub fn main() { let p = ptr::new(5); wok(ptr::inc(p)); wok(ptr::inc(p)); \
      wok(ptr::dec(p)); wok(ptr::get(p)) }"),
    ("audit: float bulat tetap tercetak sebagai float",
     "package main\npub fn main() { wok(1.0); wok(1e15); wok(1e16); wok(1e17); wok([1.0, 1e16]); \
      wok(-0.0); wok(1.0/3.0); wok(0.0/0.0); wok(1.0/0.0) }"),

    // ── Pustaka standar tingkat-JRG: wadah yang belum ada sebagai builtin ─
    ("std/set: keanggotaan, himpunan, dan HOF",
     "package main\nimport \"std/set\"\npub fn main() { \
      let a = set::from([3,1,2,3,1]); wok(a.len()); wok(arr::sort(a.to_array())); \
      wok(a.has(2)); wok(a.has(9)); \
      let b = set::from([2,3,4]); \
      wok(arr::sort(a.union(b).to_array())); wok(arr::sort(a.intersect(b).to_array())); \
      wok(arr::sort(a.difference(b).to_array())); \
      wok(set::from([1,2]).is_subset(a)); wok(a.equals(set::from([3,2,1]))); \
      wok(arr::sort(a.map(|x| x * 2).to_array())); wok(arr::sort(a.filter(|x| x > 1).to_array())); \
      wok(set::new().is_empty()) }"),
    // Nilai ASLI-nya yang disimpan, bukan kuncinya: kunci dict adalah bentuk
    // Display, jadi tanpa itu himpunan berisi int menjawab string.
    ("std/set: tipe elemennya bertahan, bukan jadi string",
     "package main\nimport \"std/set\"\npub fn main() { \
      wok(typeof(arr::first(set::from([7]).to_array()))) }"),
    // Fase KONSUMSI-2: `x = x.m(..)` memindahkan penerimanya. Yang dikunci: tidak ada pemegang
    // lain yang ikut berubah (salinan, `let`, closure, elemen array, parameter).
    ("konsumsi penerima: x = x.m(..) tidak membocorkan alias ke pemegang lain",
     "package main\nstruct K { pub xs: []int, pub n: int, pub baris: [][]int }\nimpl K {\n    pub fn tambah(self, x: int) -> K { self.xs += [x]\n      self.n += 1\n      return self }\n    pub fn kadang(self, x: int) -> K { if x < 0 { return K { xs: [], n: 0, baris: [] } }\n      self.xs += [x]\n      return self }\n    pub fn baris_baru(self, r: []int) -> K { self.baris += [r]\n      return self }\n    pub fn tulis(self, i: int, v: int) -> K { self.xs[i] = v\n      return self }\n}\nfn lewat(k: K) -> K { var m = k\n  m = m.tambah(99)\n  return m }\npub fn main() {\n    var a = K { xs: [1], n: 0, baris: [] }\n    var b = a\n    let c = a\n    a = a.tambah(2)\n    a = a.tambah(3).tambah(4)\n    wok(a); wok(b); wok(c)\n    b = b.tambah(7)\n    wok(a.xs); wok(b.xs); wok(c.xs)\n    let d = a.tambah(5)\n    wok(a.xs); wok(d.xs)\n    a = a.kadang(6)\n    wok(a.xs)\n    a = a.kadang(-1)\n    wok(a)\n    var r = [1, 2]\n    a = a.baris_baru(r)\n    r[0] = 9\n    a = a.baris_baru(r)\n    wok(a.baris); wok(r)\n    var p = K { xs: [1, 2], n: 0, baris: [[0]] }\n    var q = p\n    p.xs += [3]\n    q.xs += [4]\n    p.xs = p.xs + [5]\n    q.xs = arr::push(q.xs, 6)\n    p.baris[0] += [1]\n    p.baris += [p.xs]\n    p.xs[0] = 100\n    wok(p); wok(q)\n    let f = || { p = p.tambah(8) }\n    f()\n    wok(p.xs)\n    let t = lewat(p)\n    wok(p.xs); wok(t.xs)\n    var e = K { xs: [5, 6], n: 0, baris: [] }\n    let e2 = e\n    e = e.tulis(0, 50)\n    wok(e.xs); wok(e2.xs)\n    var ks = [K { xs: [1], n: 0, baris: [] }, K { xs: [2], n: 0, baris: [] }]\n    ks[0].xs += [10]\n    let k0 = ks[0]\n    ks[0].xs += [11]\n    wok(ks[0].xs); wok(k0.xs); wok(ks[1].xs)\n}\n"),
    // Fase TUTUP: `x = f(x, ..)` memindahkan argumen pertamanya (`d = dict::set(d, k, v)`,
    // fungsi pemakai). Pemegang lain tidak boleh ikut berubah.
    ("konsumsi argumen: x = f(x, ..) tidak membocorkan alias",
     "package main\nstruct K { pub xs: []int }\nfn tambah(k, x) { k.xs += [x]\n  return k }\nfn isi(d, k, v) { d[k] = v\n  return d }\nfn dorong(a, x) { a = arr::push(a, x)\n  return a }\nfn rek(d, n) { if n == 0 { return d }\n  d = isi(d, n, n * n)\n  d = rek(d, n - 1)\n  return d }\nfn banyak(a, xs...) { for _, x in xs { a = a + [x] }\n  return a }\nfn bawaan(a, x = 7) { a = a + [x]\n  return a }\nfn gagal(d, i) { let a = [1]\n  d[\"x\"] = a[i]\n  return d }\npub fn main() {\n    var k = K { xs: [1] }\n    let k0 = k\n    var k1 = k\n    k = tambah(k, 2)\n    k = tambah(tambah(k, 3), 4)\n    wok(k); wok(k0); wok(k1)\n    var d = {\"a\": 1}\n    let d0 = d\n    d = dict::set(d, \"b\", 2)\n    d = dict::set(d, 3, \"tiga\")\n    d = isi(d, \"c\", 3)\n    d = dict::remove(d, \"a\")\n    d = dict::remove(d, \"tak ada\")\n    wok(d); wok(d0)\n    let f = || { d = dict::set(d, \"f\", 0) }\n    f()\n    wok(d)\n    var a = [1]\n    let a0 = a\n    a = dorong(a, 2)\n    a = banyak(a, 3, 4)\n    a = bawaan(a)\n    a = bawaan(a, 9)\n    wok(a); wok(a0)\n    wok(rek({\"0\": 0}, 4))\n    var e = {\"z\": 1}\n    e = rek(e, 2)\n    wok(e)\n    var s = \"ab\"\n    s = str::upper(s)\n    var o = Some(1)\n    o = Some(o)\n    wok(s); wok(o)\n    var t = {\"k\": 1}\n    wok(\"sebelum\")\n}\n"),
    // Fase BONGKAR: `a, b = e` membongkar tuple; `q, v = q.pop()` memindahkan `q`.
    ("bongkar tuple: a, b = f(), sasaran berjalur, `_`, dan q, v = q.pop()",
     "package main\nimport \"std/queue\"\nstruct P { pub x: int, pub ys: []int }\nfn dua() { return (1, \"x\") }\nfn tiga(n) { return (n, n * 2, [n]) }\npub fn main() {\n    var a = 0\n    var b = \"\"\n    a, b = dua()\n    wok(a); wok(b)\n    var c = [0]\n    var p = P { x: 0, ys: [9, 9] }\n    p.x, p.ys[1], c = tiga(7)\n    wok(p); wok(c)\n    var t = (5, 6)\n    a, _ = t\n    wok(a)\n    var q = queue::from([1, 2, 3])\n    var v = None\n    q, v = q.pop()\n    wok(q); wok(v)\n    let q0 = q\n    q, v = q.pop()\n    q, v = q.pop()\n    q, v = q.pop()\n    wok(q); wok(v); wok(q0)\n    var x = 1\n    var y = 2\n    x, y = y, x\n    wok(x); wok(y)\n    x, y = (y, x)\n    wok(x); wok(y)\n}\n"),
    // Fase SISA: `a, b := e` mendeklarasikan sambil membongkar, sama dengan `a, b = e`.
    ("bongkar tuple: a, b := f(), `_`, dan q2, v := q.pop()",
     "package main\nimport \"std/queue\"\nfn dua() { return (1, \"x\") }\npub fn main() {\n    a, b := dua()\n    wok(a); wok(b)\n    a = 5; b = \"y\"\n    wok(a); wok(b)\n    x, _, z := (1, 2, 3)\n    wok(x + z)\n    q := queue::from([7, 8])\n    q2, v := q.pop()\n    wok(q2); wok(v); wok(q)\n    m, n := 1, 2\n    wok(m + n)\n}\n"),
    // Fase SISA: nilai fungsi tampil sama di tiga mesin — `<fn nama>` dan `<closure>`.
    ("tampilan nilai fungsi: wok, str, templat, di dalam koleksi dan struct",
     "package main\nfn tambah(a: int, b: int) -> int { a + b }\nfn kali(a: int, b: int) -> int { a * b }\nstruct K { f: fn(int, int) -> int, n: int }\npub fn main() {\n    let f = tambah\n    let k = 3\n    let g = |x: int| x + k\n    let h = |x: int| x * 2\n    wok(f)\n    wok(g)\n    wok(h)\n    wok(kali, f)\n    wok(\"nilai: \" + str(f))\n    wok(\"\"\"f=${f} g=${g}\"\"\")\n    wok([tambah, kali])\n    wok((tambah, 1))\n    wok(typeof(f), typeof(g))\n    let s = K { f: kali, n: 2 }\n    wok(s)\n    wok(Some(tambah))\n}\n"),
    ("tampilan nilai fungsi: parameter, kembalian, fn bersarang, dict",
     "package main\nimport \"std/functional\" as fx\nstruct T { n: int }\nimpl T { fn dua(self) -> int { self.n * 2 } }\nfn tampil(f: fn(int) -> int) { wok(\"dapat\", f) }\nfn naik(x: int) -> int { x + 1 }\nfn bikin() -> fn(int) -> int { naik }\nfn bikin2(k: int) -> fn(int) -> int { |x: int| x + k }\npub fn main() {\n    fn dalam(x: int) -> int { x - 1 }\n    tampil(naik)\n    tampil(dalam)\n    tampil(|x: int| x)\n    wok(bikin())\n    wok(bikin2(2))\n    let d = {\"a\": naik}\n    wok(d)\n    wok(str(naik) == \"<fn naik>\")\n    wok(str::len(str(naik)))\n}\n"),
    // Fase SISA: `std/csv` utuh, termasuk lewat LLVM (lokal bertumbuh ber-elemen array dan dict).
    ("std/csv: parse, format, format_with, to_dicts, from_dicts",
     "package main\nimport \"std/csv\"\npub fn main() {\n    let r = csv::parse(\"nama,umur\\nani,\\\"3,5\\\"\\n\\\"b \\\"\\\"x\\\"\\\"\\\",7\\n\")\n    wok(r)\n    wok(csv::format([[\"a\", \"b\"], [\"1\", \"x,y\"], [\"k\\\"q\", \"baris\\ndua\"]]))\n    wok(csv::format_with([[\"a\", \"b\"]], ';'))\n    let d = csv::to_dicts(r)\n    wok(d)\n    wok(csv::from_dicts([\"nama\", \"umur\"], d))\n}\n"),
    // Fase ISI: array dan dict di dalam nilai bertag — dicetak, dibuka, diukur, diiterasi.
    ("nilai bertag: array/dict di dalam kotak tercetak dan terbaca",
     "package main\npub fn main() {\n    var x = nil\n    x = [1, 2, 3]\n    wok(x)\n    let d = {\"a\": 1, \"b\": \"teks\", \"c\": [1, 2, 3], \"d\": {\"x\": 1.5}, \"e\": [[1, 2], [3, 4]], \"f\": [\"p\", \"q\"]}\n    wok(d)\n    wok(d[\"c\"])\n    wok(d[\"e\"])\n    let k = \"c\"\n    wok(d[k])\n    let a: []int = d[\"c\"]\n    wok(a[1] + 10)\n    let campur = [1, \"dua\", [3, 4], {\"k\": true}, 2.5, nil]\n    wok(campur)\n    for _, v in campur { wok(typeof(v), v) }\n    var y = nil\n    y = {\"p\": [1.5, 2.5]}\n    wok(y)\n    wok(arr::push(x, 4))\n    wok(arr::reverse(x))\n    wok(dict::keys(y))\n}\n"),
    ("nilai bertag: len, indeks, for, struct di dalam array, literal bersarang campur",
     "package main\nstruct P { pub x: int, pub n: string }\npub fn main() {\n    var x = nil\n    x = [10, 20, 30]\n    wok(len(x))\n    wok(x[1])\n    for i, v in x { wok(i, v) }\n    var e = nil\n    e = []\n    let es: []string = e\n    wok(len(es), es)\n    var ps = nil\n    ps = [P { x: 1, n: \"a\" }, P { x: 2, n: \"b\" }]\n    let qs: []P = ps\n    wok(qs[1].n)\n    let campur = [1, 2, 3.5]\n    wok(campur)\n    let d = {\"a\": [1, 2], \"b\": \"x\"}\n    let da: []int = d[\"a\"]\n    wok(arr::sum(da))\n    var m = nil\n    m = {\"k\": 1, \"j\": 2}\n    let mm: dict<int> = m\n    wok(mm[\"k\"] + mm[\"j\"])\n    wok(m)\n    let bersarang = [[1, \"a\"], [2.5, nil], []]\n    wok(bersarang)\n    wok({\"x\": {\"y\": {\"z\": [true, false]}}, \"w\": 0})\n}\n"),
    ("literal array bergerigi: [[1, 2], [3]]",
     "package main\npub fn main() {\n    let r = [[1, 2], [3]]\n    wok(r)\n    wok(len(r[1]))\n}\n"),
    ("std/iterator: group_by",
     "package main\nimport \"std/iterator\" as it\npub fn main() {\n    let g = it::group_by([1, 2, 3, 4, 5], |x| x % 2)\n    wok(g)\n}\n"),
    // Fase LAPIS: builtin `arr::`/`str::` yang di LLVM ditulis dalam JRG (`src/llvm/lapis.jrg`).
    ("lapis: arr:: — hitung, cari, potong, himpunan, geser, statistik, urut",
     "package main\nstruct P { pub n: string, pub u: int }\npub fn main() {\n    let a = [3, 1, 4, 1, 5, 9, 2, 6]\n    wok(arr::count_if(a, |x| x > 2))\n    wok(arr::count(a, |x| x > 2))\n    wok(arr::index_of(a, 4))\n    wok(arr::index_of([\"a\", \"b\"], \"z\"))\n    wok(arr::find_idx(a, |x| x > 4))\n    wok(arr::is_empty(a))\n    wok(arr::chunks(a, 3))\n    wok(arr::windows(a, 6))\n    wok(arr::windows(a, 9))\n    wok(arr::dedup([1, 1, 2, 2, 1]))\n    wok(arr::unique(a))\n    wok(arr::union([1, 2, 2], [2, 3]))\n    wok(arr::intersect([1, 2, 2, 3], [2, 3, 4]))\n    wok(arr::difference([1, 2, 2, 3], [2]))\n    wok(arr::enumerate([\"a\", \"b\"]))\n    wok(arr::flatten([[1], [2, 3], [4]]))\n    wok(arr::flat_map([1, 2], |x| [x, x * 10]))\n    wok(arr::insert([1, 2, 3], 3, 9))\n    wok(arr::insert([1, 2, 3], 0, 9))\n    wok(arr::remove([1, 2, 3], 1))\n    wok(arr::repeat([1, 2], 3))\n    wok(arr::fill(\"x\", 3))\n    wok(arr::rotate([1, 2, 3, 4], 5))\n    wok(arr::rotate([1, 2, 3, 4], -1))\n    wok(arr::rotate_left([1, 2, 3]))\n    wok(arr::rotate_right([1, 2, 3, 4], 1))\n    wok(arr::rotate_right([1, 2, 3, 4]))\n    wok(arr::join([1, 2, 3], \"-\"))\n    wok(arr::join([1.5, 2.0], \", \"))\n    let ps = [P { n: \"a\", u: 3 }, P { n: \"b\", u: 7 }, P { n: \"c\", u: 7 }, P { n: \"d\", u: 1 }, P { n: \"e\", u: 1 }]\n    wok(arr::max_by(ps, |p| p.u).n)\n    wok(arr::min_by(ps, |p| p.u).n)\n    wok(arr::partition(a, |x| x % 2 == 0))\n    wok(arr::zip([1, 2, 3], [\"a\", \"b\"]))\n    wok(arr::zip_with([1, 2, 3], [10, 20], |x, y| x + y))\n    wok(arr::mean(a))\n    wok(arr::mean([1.5, 2.5]))\n    wok(arr::variance([1, 2, 3, 4]))\n    wok(arr::std_dev([1, 2, 3, 4]))\n    wok(arr::median([3, 1, 2]))\n    wok(arr::median([4.5, 1.0, 2.0, 3.0]))\n    wok(arr::sort_desc(a))\n    wok(arr::binary_search([1, 3, 5, 7], 5))\n    wok(arr::binary_search([1, 3, 5, 7], 4))\n    wok(arr::group_by(a, |x| x % 3))\n    let urut = arr::sort_by_key(ps, |p| p.u)\n    for _, p in urut { wok(p.n) }\n}\n"),
    ("lapis: kunci NaN tunggal, sort_by, dict::items",
     "package main\npub fn main() {\n    let n = math::NAN\n    wok(arr::max_by([n], |x| x))\n    wok(arr::sort_by_key([n], |x| x))\n    wok(arr::sort_by_key([2.5, 1.0, 9.0, 1.0], |x| 0.0 - x))\n    wok(arr::index_of([1.0, n], n))\n    wok(arr::unique([n, n, 1.0]))\n    wok(arr::dedup([n, n]))\n    wok(arr::sort_by([3, 1, 2, 1], |a, b| a - b))\n    wok(arr::sort_by([\"bb\", \"a\", \"ccc\"], |a, b| len(b) - len(a)))\n    wok(dict::items({\"b\": 1, \"a\": 2}))\n    wok(arr::sort_desc([\"b\", \"a\", \"c\"]))\n    wok(arr::median([7]))\n}\n"),
    ("lapis: str:: — len, strip, chars, count, lines",
     "package main\npub fn main() {\n    wok(str::len(\"héllo\"))\n    wok(str::is_empty(\"\"))\n    wok(str::is_empty(\" \"))\n    wok(str::to_lowercase(\"HaLo\"))\n    wok(str::to_uppercase(\"HaLo\"))\n    wok(str::strip_prefix(\"halo dunia\", \"halo \"))\n    wok(str::strip_prefix(\"héllo\", \"hé\"))\n    wok(str::strip_prefix(\"halo\", \"x\"))\n    wok(str::strip_suffix(\"berkas.jrg\", \".jrg\"))\n    wok(str::strip_suffix(\"naïve\", \"ïve\"))\n    wok(str::strip_suffix(\"a\", \"\"))\n    wok(str::chars(\"hé!\"))\n    wok(str::count(\"banana\", \"an\"))\n    wok(str::count(\"aaaa\", \"aa\"))\n    wok(str::count(\"abc\", \"\"))\n    wok(str::count(\"héé\", \"é\"))\n    wok(str::lines(\"a\\nb\\r\\nc\\n\"))\n    wok(str::lines(\"\"))\n    wok(str::lines(\"\\n\\n\"))\n    wok(str::lines(\"satu\"))\n    wok(str::lines(\"a\\r\"))\n}\n"),
    ("lapis: bentuk method, penerimanya dievaluasi sekali",
     "package main\nfn buat() -> []int { wok(\"dievaluasi sekali\"); return [5, 3, 8, 3] }\npub fn main() {\n    let a = [3, 1, 4, 1, 5]\n    wok(a.count_if(|x| x > 2))\n    wok(a.chunks(2))\n    wok(a.unique())\n    wok(a.index_of(4))\n    wok(a.rotate(2))\n    wok(a.sort_desc())\n    wok(a.join(\"+\"))\n    wok(a.windows(4))\n    wok(a.max_by(|x| 0 - x))\n    wok(buat().unique())\n    wok(\"Halo\".to_uppercase())\n    wok(\"a,b\".count(\",\"))\n    wok(\"satu\\ndua\".lines())\n    wok(\"héllo\".chars())\n    wok(\"berkas.jrg\".strip_suffix(\".jrg\"))\n    wok(a.flat_map(|x| [x, x]))\n    wok(a.zip([\"a\", \"b\"]))\n    wok(a.partition(|x| x > 2))\n    wok(a.mean(), a.median(), a.variance())\n}\n"),
    // `NaN != x` dulu `false` di LLVM (`fcmp one`); dan program tanpa keluaran gagal ditautkan.
    ("pecahan: != atas NaN selalu benar, juga lewat fungsi generik",
     "package main\nfn beda<T>(a: T, b: T) -> bool { return a != b }\npub fn main() {\n    let n = math::NAN\n    let x = 1.0\n    wok(n != n, n == n, n != x, x != n, n < x, n >= x)\n    wok(beda(n, n), beda(x, x), beda(n, x))\n    let a = [n, 1.0]\n    wok(a[0] != a[0], a[0] == a[0])\n    var k = n\n    if k != k { wok(\"nan terdeteksi\") } else { wok(\"tidak\") }\n    wok([n] == [n], [n] != [n], (n, 1) == (n, 1), (n, 1) != (n, 1))\n}\n"),
    ("program tanpa keluaran", "package main\npub fn main() {\n    let r = arr::get_or([1, 2, 3], 1, 2)\n}\n"),
    // Fase EKOR: ekor `str::`/`math::`, `arr::product`, dan `str(x)` atas agregat — juga lewat LLVM.
    ("ekor: str:: berbasis Unicode, pad, math::trunc/hypot/log, product, str() atas agregat",
     "package main\nstruct P { pub x: int }\npub fn main() {\n    wok(str::capitalize(\"halo dunia\"), str::capitalize(\"\"), str::capitalize(\"élan\"))\n    wok(str::title(\"halo  dunia besar\"), str::snake_case(\"HaloDunia Besar\"), str::camel_case(\"halo_dunia besar\"))\n    wok(str::swap_case(\"HaLo é\"), \"[\" + str::trim_start(\"  a b  \") + \"]\", \"[\" + str::trim_end(\"  a b  \") + \"]\")\n    wok(str::is_alpha(\"abcé\"), str::is_alpha(\"ab1\"), str::is_alpha(\"\"), str::is_alphanumeric(\"ab1\"), str::is_alphanumeric(\"a b\"))\n    wok(str::is_lower(\"abc d1\"), str::is_lower(\"aBc\"), str::is_lower(\"123\"), str::is_upper(\"ABC 1\"), str::is_upper(\"AbC\"))\n    wok(str::is_numeric(\"42\"), str::is_numeric(\" -3.5 \"), str::is_numeric(\"1.2.3\"), str::is_numeric(\"-\"), str::is_numeric(\".\"), str::is_numeric(\"\"))\n    wok(str::words(\"  halo   dunia \\t x\\n\"), str::words(\"\"), str::bytes(\"hé\"), str::byte_len(\"héllo\"))\n    wok(\"halo\".capitalize(), \"a b\".title(), \"AbC\".swap_case(), \" x \".trim_start(), \"ab\".is_alpha(), \"a b c\".words(), \"é\".bytes(), \"é\".byte_len())\n    wok(math::trunc(2.7), math::trunc(-2.7), math::trunc(5), math::trunc(-0.5))\n    wok(math::copysign(3.0, -1.0), math::copysign(3, -2), math::copysign(-3.0, 0.0), math::hypot(3, 4), math::hypot(1.5, 2.5))\n    wok(math::log(8, 2), math::log(100.0, 10.0), math::log(2, 1), math::log(7.3, 2.1))\n    wok(math::degrees(1), math::degrees(0.3), math::radians(180), math::radians(33.3))\n    wok(math::is_finite(1.5), math::is_finite(math::INF), math::is_finite(math::NAN), math::is_finite(3))\n    wok(arr::product([2, 3, 4]), arr::product([1.5, 2.0]), [2, 5].product())\n    wok(str::pad_left(42, 5), str::pad_left(\"ab\", 5, \"*\"), str::pad_left(\"ab\", 5, '*'), str::pad_left(\"abcdef\", 3))\n    wok(str::pad_right(1.5, 6, \"0\"), str::center(\"ab\", 7, \"-\"), \"[\" + str::center(\"ab\", 6) + \"]\", str::ljust(\"ab\", 4, \".\"), str::rjust(\"ab\", 4, \".\"))\n    wok(str::pad_left(\"é\", 3, \"ü\"), str::pad_left(true, 6), str::pad_left([1, 2], 8))\n    wok(str([1, 2]), str((1, \"a\")), str(P { x: 3 }), str(Some(2)), str({\"a\": 1}), \"x\" + str([1.5]))\n}\n"),
    ("method chunks(0)/windows(0): satu kalimat dengan bentuk namespace-nya", "package main\npub fn main() { wok([1, 2, 3].chunks(2)); wok([1, 2, 3].windows(2)) }"),
    // Fase KECIL: method sebagai nilai, `rand::`, `arr::shuffle`/`sample`, `str::substr`, `dict::is_dict`.
    ("method sebagai nilai (T::m), rand:: dalam batasnya, shuffle/sample menjaga isi, substr, is_dict",
     "package main\nstruct T { pub n: int }\nimpl T {\n    fn dua(self) -> int { self.n * 2 }\n    fn tambah(self, k: int) -> int { self.n + k }\n    fn baru(n: int) -> T { T { n: n } }\n}\nfn pakai(f: fn(T) -> int, t: T) -> int { return f(t) }\npub fn main() {\n    wok(arr::map([T { n: 1 }, T { n: 4 }], T::dua))\n    wok(arr::map([1, 2], T::baru))\n    let f = T::dua\n    wok(f(T { n: 21 }), typeof(f), f)\n    let g = T::tambah\n    wok(g(T { n: 1 }, 5), pakai(T::dua, T { n: 3 }))\n    wok(str::substr(\"halo dunia\", 5, 3), str::substr(\"halo\", 2), \"[\" + str::substr(\"halo\", 4) + \"]\", str::substr(\"halo\", 1, 0) == \"\", str::substr(\"halo\", 1, 99), str::substr(\"héllo\", 1, 2), \"halo dunia\".substr(5, 3))\n    wok(dict::is_dict({\"a\": 1}), dict::is_dict([1]), dict::is_dict(5))\n    var x = nil\n    x = {\"k\": 1}\n    wok(dict::is_dict(x))\n    let a = [1, 2, 3, 4, 5, 6, 7, 8]\n    let s = arr::shuffle(a)\n    wok(len(s), arr::sum(s), arr::sort(s) == a, a)\n    wok(len(arr::sample(a, 3)), len(arr::sample(a)), len(arr::sample(a, 99)), len(rand::sample(a, 2)), arr::sort(rand::shuffle(a)) == a)\n    var baik = true\n    for i in 0..2000 {\n        let r = rand::int(3, 7)\n        if r < 3 || r > 7 { baik = false }\n        let q = rand::float()\n        if q < 0.0 || q >= 1.0 { baik = false }\n        if !arr::contains(a, rand::choice(a)) { baik = false }\n    }\n    wok(baik, rand::int(5, 5), typeof(rand::bool()), typeof(rand::float()))\n    var lihat = [false, false, false]\n    for i in 0..300 { lihat[rand::int(0, 2)] = true }\n    wok(lihat)\n}\n"),
    ("method array lewat pintu namespace: concat, drop, skip, take, get, get_or, swap",
     "package main\npub fn main() {\n    let a = [1, 2, 3, 4]\n    wok(a.concat([9]), a.drop(1), a.skip(2), a.take(2), a.get(1), a.get(9), a.get_or(9, 0), a.swap(0, 3))\n}\n"),
    ("parameter tipe menang atas struct bernama sama (struct T lawan fn f<T>)",
     "package main\nstruct T { pub n: int }\nstruct K { pub s: string }\nfn pertama<T>(a: []T) -> T { return a[0] }\nfn pasang<K, T>(k: K, t: T) -> (K, T) { return (k, t) }\npub fn main() {\n    wok(pertama([5, 6]), pertama([\"a\"]), pertama([T { n: 9 }]))\n    wok(pasang(1, \"x\"), pasang(K { s: \"k\" }, T { n: 2 }))\n    wok(arr::unique([3, 3, 1]), arr::chunks([\"a\", \"b\", \"c\"], 2), arr::max_by([T { n: 1 }, T { n: 5 }], |t| t.n))\n}\n"),
    // Fase BERKAS: `fs::`/`file::`/`env::`/`time::` — juga lewat LLVM (pintu C `jrg_os` + lapis).
    ("berkas: tulis, baca, tambah, salin, pindah, hapus, isi folder terurut, Result milik file::",
     "package main\npub fn main() {\n    let akar = fs::join(fs::temp_dir(), \"jrg_berkas_\" + str(time::now_ms()) + \"_\" + str(rand::int(0, 999999999)))\n    wok(fs::mkdir(fs::join(akar, \"d\", \"sub\")), fs::is_dir(akar), fs::is_file(akar), fs::exists(fs::join(akar, \"x\")))\n    let a = fs::join(akar, \"a.txt\")\n    fs::write(a, \"baris1\\nbaris2\\n\")\n    wok(fs::read(a), fs::read_lines(a), fs::size(a), fs::is_file(a), file::exists(a), fs::is_symlink(a))\n    wok(file::read(a), file::read_lines(a))\n    fs::append(a, \"x\")\n    wok(file::append(a, 5), fs::read(a))\n    wok(file::write(fs::join(akar, \"d\", \"b.txt\"), [1, 2]), fs::read(fs::join(akar, \"d\", \"b.txt\")))\n    fs::write_lines(fs::join(akar, \"d\", \"c.txt\"), [\"p\", \"q\"])\n    fs::write_lines(fs::join(akar, \"d\", \"a.txt\"), [1.5, 2.5])\n    wok(fs::ls(fs::join(akar, \"d\")), file::list_dir(fs::join(akar, \"d\")), fs::ls(fs::join(akar, \"d\", \"sub\")))\n    wok(fs::read(fs::join(akar, \"d\", \"c.txt\")), fs::read(fs::join(akar, \"d\", \"a.txt\")))\n    wok(fs::copy(a, fs::join(akar, \"salin.txt\")), fs::copy(fs::join(akar, \"zz\"), fs::join(akar, \"yy\")), fs::read(fs::join(akar, \"salin.txt\")) == fs::read(a))\n    wok(fs::rename(fs::join(akar, \"salin.txt\"), fs::join(akar, \"pindah.txt\")), fs::rename(fs::join(akar, \"zz\"), fs::join(akar, \"yy\")), fs::exists(fs::join(akar, \"salin.txt\")), fs::exists(fs::join(akar, \"pindah.txt\")))\n    let hilang = fs::join(akar, \"tidak_ada\")\n    wok(str::replace(str(file::read(hilang)), akar, \"<akar>\"))\n    wok(str::replace(str(file::read_lines(hilang)), akar, \"<akar>\"))\n    wok(str::replace(str(file::list_dir(hilang)), akar, \"<akar>\"))\n    wok(str::replace(str(file::delete(hilang)), akar, \"<akar>\"))\n    wok(str::replace(str(file::write(fs::join(hilang, \"x\", \"y\"), \"x\")), akar, \"<akar>\"))\n    wok(str::replace(str(file::append(fs::join(hilang, \"x\", \"y\"), \"x\")), akar, \"<akar>\"))\n    wok(fs::delete(fs::join(akar, \"pindah.txt\")), fs::delete(fs::join(akar, \"pindah.txt\")), file::delete(a), fs::exists(a))\n    for _, n in [\"a.txt\", \"b.txt\", \"c.txt\"] { fs::delete(fs::join(akar, \"d\", n)) }\n    wok(fs::ls(fs::join(akar, \"d\")))\n}\n"),
    ("berkas: fs::delete membuang pohon, fs::rmdir cuma yang kosong, file::delete cuma berkas",
     "package main\npub fn main() {\n    let akar = fs::join(fs::temp_dir(), \"jrg_hapus_\" + str(time::now_ms()) + \"_\" + str(rand::int(0, 999999999)))\n    let d = fs::join(akar, \"d\")\n    let x = fs::join(d, \"x.txt\")\n    fs::mkdir(d)\n    fs::write(x, \"1\")\n    wok(fs::rmdir(d), fs::rmdir(x), fs::rmdir(fs::join(akar, \"hilang\")))\n    wok(str::replace(str(file::delete(d)), akar, \"AKAR\"))\n    wok(fs::delete(x), fs::rmdir(d), fs::exists(d))\n    fs::mkdir(fs::join(akar, \"p\", \"q\"))\n    fs::write(fs::join(akar, \"p\", \"y\"), \"2\")\n    wok(fs::delete(akar), fs::exists(akar), fs::delete(akar))\n}\n"),
    ("jalur, lingkungan, dan waktu: basename/dirname/join, env::, time:: dari cap waktu tetap",
     "package main\npub fn main() {\n    for _, p in [\"/a/b/c.txt\", \"c\", \"/a/b/\", \"\", \"/\", \"a//b\", \"a/\", \"/a\"] { wok(\"[\" + fs::basename(p) + \"]\", \"[\" + fs::dirname(p) + \"]\", \"[\" + fs::join(p, \"x\") + \"]\", \"[\" + fs::join(\"x\", p) + \"]\") }\n    wok(fs::temp_dir(), fs::cwd() == fs::cwd(), len(fs::cwd()) > 1)\n    wok(env::get(\"TIDAK_ADA_XYZ\"), env::get_or(\"TIDAK_ADA_XYZ\", \"c\"), env::get_or(\"TIDAK_ADA_XYZ\", 5), env::get(\"HOME\") == Some(env::get_or(\"HOME\", \"\")))\n    env::set(\"JRG_X\", \"1\")\n    wok(env::get(\"JRG_X\"))\n    for _, ms in [0, 1700000000123, -1, -86400000, 951782400000, 4102444799999] {\n        wok(time::year(ms), time::month(ms), time::day(ms), time::hour(ms), time::minute(ms), time::second(ms), time::weekday(ms), time::format(ms), time::format(ms, \"%Y/%y %m-%d %H:%M:%S.%L %A %B %%\"))\n    }\n    let t0 = time::now_ms()\n    time::sleep_ms(30)\n    time::sleep(0.02)\n    let t1 = time::now_ms()\n    wok(t1 - t0 >= 50, t1 - t0 < 2000, typeof(time::now()), time::now() > 1700000000.0, time::year() >= 2024, len(time::format()) == 19)\n    time::sleep(-1)\n    time::sleep_ms(0)\n}\n"),
    ("fs::join: semua potongan ikut", "package main\npub fn main() { wok(fs::join(\"a\", \"b\", \"c\"), fs::join(\"a/\", \"b\", \"c\", \"d\"), fs::join(\"a\", \"/b\", \"c\")) }"),
    // Fase JSON: satu pengurai ketat (`json_inti`) untuk `json::parse` dan `dok::json_decode`.
    // Dulu teks non-ASCII rusak (`é` jadi `Ã©`) dan pasangan surrogate hilang.
    ("json::parse: teks non-ASCII utuh, surrogate, angka, kunci kembar, bolak-balik stringify",
     "package main\npub fn main() {\n    wok(json::parse(\"{\\\"a\\\": 1, \\\"b\\\": [true, null, 2.5, \\\"x\\\"], \\\"é\\\": \\\"naïve 😀\\\"}\"))\n    wok(json::parse(\"\\\"é\\\"\") == \"é\", len(json::parse(\"\\\"é\\\"\")), json::parse(\"\\\"\\\\ud83d\\\\ude00\\\\u00e9\\\\n\\\\b\\\\f\\\"\"))\n    wok(json::parse(\" [1, -0, 1e3, 123456789012345678901234567890] \"), typeof(json::parse(\"3\")), typeof(json::parse(\"3.0\")), typeof(json::parse(\"null\")))\n    wok(json::parse(\"{\\\"a\\\": 1, \\\"a\\\": 2}\"), json::parse(\"{}\"), json::parse(\"[]\"), json::parse(\"true\"), json::parse(\"false\"))\n    wok(json::parse(json::stringify({\"k\": [\"é\", 1.5, nil, {\"z\": true}]})))\n    wok(dok::json_decode(\"{\\\"a\\\": [1, 2.5, null], \\\"é\\\": \\\"ü\\\"}\"), dok::json_decode(\"tidak\"), dok::json_decode(\"1 2\"))\n}\n"),
    // Fase JSON-LLVM: `json::` lewat LLVM, dan karakter kendali di-escape oleh penyandinya.
    ("json:: urai, sandi rapat dan rapi, get/has/keys/values, bolak-balik, iterasi hasil urai",
     "package main\npub fn main() {\n    let j = json::parse(\"{\\\"a\\\": 1, \\\"b\\\": [true, null, 2.5, \\\"x\\\"], \\\"é\\\": \\\"naïve 😀\\\", \\\"o\\\": {\\\"z\\\": []}}\")\n    wok(j)\n    wok(json::stringify(j))\n    wok(json::pretty(j))\n    wok(json::pretty([1, 2], 0), json::pretty({\"a\": 1}, 4))\n    wok(json::stringify({\"a\": 1, \"b\": [true, nil, 2.5, \"x\\\"y\\n\"]}), json::stringify([1, 2.0, \"é\"]), json::stringify(1.0), json::stringify(\"teks\"), json::stringify(nil), json::stringify([[1, 2], [3]]))\n    wok(json::get(j, \"a\"), json::get(j, \"tak ada\"), json::has(j, \"b\"), json::has(j, \"q\"), json::keys(j), json::values(json::parse(\"{\\\"b\\\": 1, \\\"a\\\": 2}\")))\n    wok(json::parse(\"[1, -0, 1e3]\"), typeof(json::parse(\"3\")), typeof(json::parse(\"3.0\")), typeof(json::parse(\"null\")), typeof(json::parse(\"\\\"s\\\"\")), typeof(json::parse(\"[]\")), typeof(json::parse(\"{}\")))\n    wok(json::parse(json::stringify({\"k\": [\"é\", 1.5, nil, {\"z\": true}]})))\n    let a = json::parse(\"[10, 20, 30]\")\n    wok(len(a), a[1])\n    for _, x in a { wok(x) }\n}\n"),
    ("json::stringify meng-escape karakter kendali", "package main\npub fn main() { wok(json::stringify(\"a\\u0001b\\u001f\\n\")) }"),
    // Fase BERKAS-2: `fs::walk`/`abs`/`is_writable`/`chmod` dan `env::args` — juga lewat LLVM.
    ("fs::walk terurut per folder, fs::abs leksikal, is_writable mengikuti chmod",
     "package main\npub fn main() {\n    let akar = fs::join(fs::temp_dir(), \"jrg_walk_\" + str(time::now_ms()) + \"_\" + str(rand::int(0, 999999999)))\n    fs::mkdir(fs::join(akar, \"sub\"))\n    fs::mkdir(fs::join(akar, \"sub.d\"))\n    for _, n in [\"b.txt\", \"a.txt\", \"sub.txt\", \"sub/c.txt\", \"sub.d/z\"] { fs::write(fs::join(akar, n), n) }\n    wok(arr::map(fs::walk(akar), |p| str::replace(p, akar, \"<akar>\")), fs::exists(fs::join(akar, \"tidak_ada\")))\n    let c = fs::cwd()\n    wok(fs::abs(\"x/../y\") == c + \"/y\", fs::abs(\"/a/./b/../c\"), fs::abs(\"/..\"), fs::abs(\"\") == c, fs::abs(\"a//b/\") == c + \"/a/b\")\n    let f = fs::join(akar, \"a.txt\")\n    wok(fs::is_writable(f), fs::is_writable(akar), fs::is_writable(fs::join(akar, \"zz\")))\n    fs::chmod(f, 292)\n    wok(fs::is_writable(f))\n    fs::chmod(f, 420)\n    wok(fs::is_writable(f), len(env::args()) >= 1)\n    for _, n in [\"b.txt\", \"a.txt\", \"sub.txt\", \"sub/c.txt\", \"sub.d/z\"] { fs::delete(fs::join(akar, n)) }\n}\n"),
    // Fase KORUTIN: konkurensi lewat LLVM (korutin satu utas) menjawab sama dengan dua mesin ber-utas.
    ("konkurensi: channel berpenyangga dan tidak, spawn menangkap salinan, try_send/try_recv, Chan::new",
     "package main\npub fn main() {\n    let ch = make_chan(int)\n    spawn { ch <- 1 }\n    wok(<-ch)\n    let c = make_chan(bool)\n    spawn { c <- true }\n    wok(\"a\")\n    <-c\n    wok(\"b\")\n    let b = make_chan(string, 2)\n    b <- \"satu\"\n    b <- \"dua\"\n    wok(chan::len(b), chan::cap(b), b.len(), <-b, <-b, chan::has_value(b))\n    let hasil = make_chan(int, 10)\n    for i in 0..5 {\n        let k = i\n        spawn { hasil <- k * k }\n    }\n    var jumlah = 0\n    for _ in 0..5 { jumlah += <-hasil }\n    wok(jumlah)\n    let d = Chan::new(2)\n    d <- 1\n    d <- \"dua\"\n    wok(<-d, <-d, typeof(d), d)\n    d.close()\n    wok(chan::is_closed(d), d.is_closed())\n    var a = [1, 2, 3]\n    let s = make_chan(int)\n    spawn { s <- len(a) + a[0] }\n    a[0] = 100\n    wok(<-s)\n    let t = make_chan(int, 1)\n    wok(chan::try_recv(t), chan::try_send(t, 7), chan::try_send(t, 8), chan::try_recv(t))\n    t.close()\n    wok(chan::try_recv(t), chan::try_send(t, 1))\n}\n"),
    ("konkurensi: tidur menentukan urutan, select dua channel dan default, fungsi ber-parameter channel",
     "package main\nfn hitung(c: chan int, n: int) { for i in 0..n { c <- i } }\npub fn main() {\n    let urut = make_chan(string, 10)\n    spawn { time::sleep_ms(60)\n        urut <- \"lambat\" }\n    spawn { time::sleep_ms(20)\n        urut <- \"cepat\" }\n    wok(<-urut, <-urut)\n    let a = make_chan(int)\n    let b = make_chan(string)\n    spawn { a <- 1 }\n    spawn { time::sleep_ms(30)\n        b <- \"dua\" }\n    for _ in 0..2 {\n        select {\n            case x := <-a: { wok(\"a\", x) }\n            case y := <-b: { wok(\"b\", y) }\n        }\n    }\n    select {\n        case x := <-a: { wok(\"tak mungkin\", x) }\n        default: { wok(\"kosong\") }\n    }\n    let c = make_chan(int)\n    spawn { hitung(c, 4) }\n    var total = 0\n    for _ in 0..4 { total += <-c }\n    wok(total)\n    let giliran = make_chan(int)\n    spawn { <-giliran\n        wok(\"selesai belakangan\") }\n    wok(\"akhir main\")\n    giliran <- 1\n}\n"),
    // `select` atas channel TANPA penyangga dulu menggantung di `--vm`: armnya bertanya `has_value`,
    // dan pengirim yang menunggu baru menaruh nilainya sesudah melihat penerima.
    ("select: menerima dari channel tanpa penyangga yang pengirimnya goroutine",
     "package main\npub fn main() {\n    let a = make_chan(int)\n    spawn { a <- 1 }\n    select {\n        case x := <-a: { wok(\"a\", x) }\n    }\n    wok(\"selesai\")\n}\n"),
    ("Future/Task: await, join, try_get, timeout, all, race, hasil berbagai tipe",
     "package main\nstruct P { pub x: int }\nfn kerja() -> int { return 42 }\nfn tambah(a, b) -> int { return a + b }\nfn nama(n: int) -> string { return \"n\" + str(n) }\nfn buat(x: int) -> P { return P { x: x * 2 } }\nfn diam() { wok(\"jalan\") }\npub fn main() {\n    let f = Future::new(|| 7)\n    wok(future::await(f))\n    let g = Future::new(kerja)\n    let t = Task::spawn(tambah, 1, 2)\n    wok(typeof(g), typeof(t), g, t)\n    wok(task::join(t), future::await(g))\n    wok(future::await(Future::new(nama, 5)), future::await(Future::new(buat, 4)), future::await(Future::new(|a, b| [a, b], 1, 2)))\n    let d = Future::new(diam)\n    wok(future::await(d))\n    let l = Future::new(|| { time::sleep_ms(40); 5 })\n    wok(future::try_get(l), future::is_done(l))\n    time::sleep_ms(120)\n    wok(future::is_done(l), future::try_get(l))\n    let w = Future::new(|| { time::sleep_ms(80); 9 })\n    wok(future::timeout(w, 10))\n    wok(future::timeout(w, 900))\n    wok(future::all([Future::new(|| 1), Future::new(|| 2), Future::new(tambah, 3, 4)]), future::all([]))\n    wok(future::race([Future::new(|| { time::sleep_ms(60); 1 }), Future::new(|| { time::sleep_ms(10); 2 })]))\n    var fs = [Future::new(kerja)]\n    for i in 0..3 { fs = fs + [Future::new(tambah, i, 10)] }\n    var jumlah = 0\n    for _, x in fs { jumlah += future::await(x) }\n    wok(jumlah)\n}\n"),
    // Fase JEMBATAN: builtin milik `jrgc` dipanggil biner LLVM lewat pustaka dinamis.
    ("jembatan: crypto::, re::, dok:: (url, json, numerik), ptr:: sebagai handle, Result dari builtin",
     "package main\npub fn main() {\n    wok(crypto::hex_encode(\"é\"), crypto::hex_decode(\"c3a9\"), crypto::hex_decode(\"zz\"))\n    wok(crypto::base64_encode(\"é😀\"), crypto::base64_decode(\"w6nwn5iA\"), crypto::sha256(\"abc\"))\n    wok(re::test(\"^é+$\", \"ééé\"), re::test(\"^a\", \"ba\"))\n    wok(dok::url_encode(\"é a/b?c=d\"), dok::url_decode(\"%C3%A9+a\"), dok::escape_html(\"<é&>\"))\n    wok(dok::num_is_prime(97), dok::num_is_prime(98), dok::num_to_base(255, 16))\n    wok(dok::json_encode({\"a\": [1, 2.5, \"x\"]}), dok::json_decode(\"[1, {\\\"k\\\": null}]\"))\n    let p = ptr::new(5)\n    ptr::set(p, 9)\n    wok(ptr::get(p), typeof(p))\n    wok(dok::vec_dot([1.0, 2.0], [3.0, 4.0]), dok::mat_identity(2))\n    match crypto::hex_decode(\"6869\") {\n        Ok(v) => wok(\"ok\", v),\n        Err(e) => wok(\"gagal\", e),\n    }\n}\n"),
    // Fase UMUM: nilai bertag LLVM memuat tipe apa pun, dan array/tuple punya urutan di sana.
    ("nilai bertag: struct, tuple, enum, Option, fungsi — dicetak, typeof, dibuka kembali",
     "package main\nstruct P { pub x: int, pub n: string }\nenum W { Merah, Rgb(int, int, int) }\nfn dobel(x: int) -> int { return x * 2 }\npub fn main() {\n    var x = nil\n    x = P { x: 1, n: \"a\" }\n    wok(x, typeof(x))\n    x = (1, \"dua\")\n    wok(x, typeof(x))\n    x = W::Rgb(1, 2, 3)\n    wok(x, typeof(x))\n    x = Some(5)\n    wok(x)\n    x = dobel\n    wok(x)\n    let campur = [1, P { x: 2, n: \"b\" }, (3, 4), W::Merah, \"teks\", [P { x: 9, n: \"z\" }], Ok(1)]\n    wok(campur)\n    for _, v in campur { wok(typeof(v)) }\n    let d = {\"p\": P { x: 5, n: \"q\" }, \"t\": (1, 2), \"n\": 3}\n    wok(d)\n    let p: P = d[\"p\"]\n    wok(p.x + 1, p.n)\n    var y = nil\n    y = P { x: 7, n: \"y\" }\n    let q: P = y\n    wok(q.x)\n}\n"),
    ("urutan leksikografis array dan tuple: <, <=, >, >=, beda panjang, bersarang",
     "package main\npub fn main() {\n    wok([1,2] < [1,3], [1,3] < [1,2], [1,2] <= [1,2], [2] > [1], [1,2] < [1], [1] < [1,2])\n    wok((1,2) < (1,3), (1,\"a\") < (1,\"b\"), (2,1) >= (2,1))\n    wok([[1,2],[0]] > [[1,1],[9]], [\"b\", \"a\"] >= [\"b\"], ['a'] < ['b'], [true] < [false], [false] < [true], [true] <= [true], (true, 1) < (true, 2))\n    wok([1, 2] > [1], [1] > [1, 0], [\"a\"] < [\"b\"], [1.5, 2.0] < [1.5, 3.0])\n    let a = [3, 1]\n    let b = [3, 2]\n    var k: []int = []\n    wok(a < b, b < a, k < a, a >= k)\n}\n"),
    ("arr::sort/min/max atas array dan tuple",
     "package main\npub fn main() {\n    wok(arr::sort([[2],[1]]), arr::sort([(2,1),(1,2),(1,1)]), arr::sort([[1,2],[1],[0,9],[1,1]]))\n    wok(arr::min([[2],[1]]), arr::max([[2],[1]]), arr::sort([(\"b\", 2), (\"a\", 9), (\"b\", 1)]))\n    var xs: [][]int = []\n    for i in 0..6 { xs = xs + [[(i * 7) % 5, i]] }\n    wok(arr::sort(xs))\n}\n"),
    ("nilai bertag berisi struct/tuple/enum: == membandingkan ISI; typeof closure dalam kotak",
     "package main\nstruct P { x: int, n: string }\nenum W { Merah, Rgb(int, int, int) }\npub fn main() {\n    var u = nil\n    var v = nil\n    u = P { x: 1, n: \"a\" }\n    v = P { x: 1, n: \"a\" }\n    wok(u == v, u != v)\n    v = P { x: 2, n: \"a\" }\n    wok(u == v, u != v)\n    v = (1, \"dua\")\n    wok(u == v)\n    u = (1, \"dua\")\n    wok(u == v)\n    u = W::Rgb(1, 2, 3)\n    v = W::Rgb(1, 2, 3)\n    wok(u == v)\n    v = W::Merah\n    wok(u == v)\n    u = Some(4)\n    v = Some(4)\n    wok(u == v)\n    v = 4\n    wok(u == v)\n    let xs = [1, P { x: 1, n: \"a\" }, \"t\"]\n    wok(xs[1] == P { x: 1, n: \"a\" })\n    var f = nil\n    f = |x| x + 1\n    wok(typeof(f))\n    f = main\n    wok(typeof(f))\n}\n"),
    ("nilai bertag: == bulat lawan pecahan, NaN, lintas tipe",
     "package main\npub fn main() {\n    var u = nil\n    var v = nil\n    u = 1\n    v = 1.0\n    wok(u == v, v == u, u != v)\n    v = 1.5\n    wok(u == v, u != v)\n    v = 1\n    wok(u == v)\n    v = \"1\"\n    wok(u == v)\n    u = 2.5\n    v = 2.5\n    wok(u == v)\n    v = math::NAN\n    u = math::NAN\n    wok(u == v)\n    u = true\n    v = 1\n    wok(u == v)\n    u = nil\n    wok(u == v, u == nil)\n    let xs = [1, 2.0, \"a\"]\n    wok(xs[0] == xs[1], xs[1] == 2)\n}\n"),
    ("nilai bertag: == nil dibaca dari tag; bool, rune, unit",
     "package main\npub fn main() {\n    var u = nil\n    var v = nil\n    u = 5\n    u = nil\n    v = 1\n    v = nil\n    wok(u == nil, nil == u, u != nil)\n    wok(u == v)\n    u = true\n    v = true\n    wok(u == v)\n    u = 'a'\n    v = 'a'\n    wok(u == v)\n    u = ()\n    v = ()\n    wok(u == v)\n    u = 3\n    wok(u == nil, u == 3, 3 == u, u == 3.0, u == \"3\")\n}\n"),
    ("cow: salinan dict berisi array/dict tidak berbagi isi; array bersarang dan struct juga",
     "package main\nstruct S { pub xs: []int }\npub fn main() {\n    var d = {\"k\": [1, 2], \"j\": [1]}\n    var e = d\n    e[\"k\"][0] = 33\n    wok(d[\"k\"][0], e[\"k\"][0])\n    var d2 = {\"k\": [1, 2]}\n    var e2 = d2\n    e2[\"k\"][0] = 33\n    wok(d2, e2)\n    var dd = {\"a\": {\"x\": [1]}}\n    var ee = dd\n    ee[\"a\"][\"x\"][0] = 9\n    wok(dd, ee)\n    var a = [[1, 2], [3]]\n    var b = a\n    b[0][0] = 44\n    wok(a[0][0], b[0][0])\n    var s = S { xs: [1, 2] }\n    var t = s\n    t.xs[0] = 55\n    wok(s.xs[0], t.xs[0])\n}\n"),
    ("cow: salinan dict campur, array campur dan nilai bertag tidak berbagi isi",
     "package main\npub fn main() {\n    var d = {\"k\": [1, 2], \"n\": 5}\n    var e = d\n    e[\"k\"] = [9, 9]\n    wok(d, e)\n    var xs = [1, [2, 3], \"a\"]\n    var ys = xs\n    ys[1] = [7]\n    wok(xs, ys)\n    var u = nil\n    u = [1, 2]\n    var v = u\n    v = arr::push(v, 3)\n    wok(u, v)\n    var m = {\"a\": {\"x\": 1}, \"b\": 2}\n    var n = m\n    n[\"b\"] = 3\n    wok(m, n)\n    var c = {\"k\": [1, 2], \"n\": 5}\n    var c2 = c\n    c2[\"k\"] = arr::push([0], 77)\n    c2[\"n\"] = 6\n    wok(c, c2)\n    var x = nil\n    x = [[1], [2]]\n    var y = x\n    y = arr::push(y, [5])\n    wok(x, y)\n    var bs = [1, [2, 3], \"a\"]\n    var cs = bs\n    cs[1] = [8, 3]\n    cs[0] = 4\n    wok(bs, cs)\n    var kk = [{\"a\": [1], \"b\": 2}]\n    var ll = kk\n    ll[0][\"b\"] = 3\n    wok(kk, ll)\n    let hasil = ubah(c)\n    wok(c, hasil)\n}\nfn ubah(d: dict) -> dict { d[\"z\"] = 1\n return d }\n"),
    ("== lintas golongan skalar tidak pernah sama; .get_or, .to_string, .format sebagai method",
     "package main\npub fn main() {\n    let r = 'o'\n    wok(r == \"o\", r == 111, \"o\" == r, r != \"ob\", 'a' == \"b\", r == 'o')\n    wok(1 == \"1\", \"1\" != 1, true == \"true\", 1.5 == \"1.5\", true == 'a', 1 == 1.0, 2 == 2)\n    let d = {\"a\": 1}\n    wok(d.get_or(\"a\", 0), d.get_or(\"z\", 9))\n    wok([1,2,3].to_string(), \"hé\".to_string(), \"{} {}\".format(1, 2), d.to_string())\n    var hit = 0\n    let f = || { hit = hit + 1\n        return \"x\" }\n    wok(f() == 1, hit)\n    lain()\n}\nfn lain() { let r = 'o'\n let n = 111\n let b = true\n wok(r == n, n == r, b == 1, 1 == b, b == n, r != n, n == 111, 1.0 == n, n == 111.0, r == 'o', b == true) }\n"),
    ("pola rentang berbatas pecahan atas bilangan bulat, termasuk tak bertanda dan negatif",
     "package main\npub fn main() {\n    let u: uint = 3\n    wok(match u { 2.5..=3.5 => \"ya\", _ => \"tidak\" }, match 4 { 4.0..4.5 => \"a\", _ => \"b\" }, match 4 { 3.0..4.0 => \"a\", _ => \"b\" })\n    wok(match -2 { -2.5..-1.5 => \"neg\", _ => \"x\" }, match 7 { 1.5.. => \"atas\", _ => \"x\" })\n}\n"),
    ("fungsi bernama tanpa anotasi parameter sebagai nilai: variabel, callback, array",
     "package main\nvar total = 0\nfn diam(x) { total = total + x }\nfn dobel(x) { return x * 2 }\nfn gabung(a, b) { return str(a) + \"-\" + str(b) }\nfn besar(x) { return x > 2 }\nfn nama(x) { return typeof(x) }\npub fn main() {\n    let f = dobel\n    wok(f(4), f(2.5), f(\"ab\"), typeof(f))\n    let g = gabung\n    wok(g(1, \"x\"), g([1, 2], nil))\n    arr::each([1, 2, 3], diam)\n    wok(total)\n    wok([1, 2, 3].map(dobel), [1, 2, 3, 4].filter(besar), arr::map([\"a\", 1, 2.5], nama))\n    let fs = [dobel, dobel]\n    wok(fs[0](21), fs[1](\"z\"))\n    let h = diam\n    wok(h(5), total)\n}\n"),
    ("literal varian enum ber-field: urutan evaluasi, match, ==, tampilan",
     "package main\nenum E { V { a: int, b: int }, W { nama: string, xs: []int }, U, T(int) }\nfn catat(s: string, n: int) -> int { wok(s)\n return n }\npub fn main() {\n    let e = E::V { b: catat(\"b dulu\", 20), a: catat(\"a kemudian\", 1) }\n    wok(e, typeof(e))\n    wok(match e { E::V { a, b } => a + b, _ => 0 })\n    let w = E::W { nama: \"w\", xs: [1, 2] }\n    wok(w)\n    match w { E::W { nama, xs } => wok(nama, len(xs)), _ => wok(\"lain\") }\n    wok(e == E::V { a: 1, b: 20 }, e == E::V { a: 2, b: 20 }, [e, E::U, E::T(3)])\n}\n"),
    // Operasi acak atas queue/deque/heap dibandingkan dengan model array yang naif.
    ("std/queue, deque, heap: operasi acak sama dengan model array",
     "package main\nimport \"std/queue\"\nimport \"std/deque\"\nimport \"std/heap\"\nfn lcg(s: int) -> int { return (s * 1103515245 + 12345) % 2147483648 }\nfn uji_deque(benih: int, langkah: int) -> int {\n    var d: deque::Deque<int> = deque::new()\n    var m: []int = []\n    var s = benih\n    var salah = 0\n    for i in 0..langkah {\n        s = lcg(s)\n        let op = (s / 65536) % 7\n        if op == 0 || op == 1 { d = d.push_back(i)\n            m = arr::push(m, i) }\n        if op == 2 { d = d.push_front(i)\n            m = arr::concat([i], m) }\n        if op == 3 { if d.peek_front() != arr::first(m) { salah = salah + 1 }\n            d = d.rest_front()\n            if len(m) > 0 { m = arr::skip(m, 1) } }\n        if op == 4 { if d.peek_back() != arr::last(m) { salah = salah + 1 }\n            d = d.rest_back()\n            if len(m) > 0 { m = arr::take(m, len(m) - 1) } }\n        if op == 5 { let r = d.pop_front()\n            if r[1] != arr::first(m) { salah = salah + 1 }\n            d = r[0]\n            if len(m) > 0 { m = arr::skip(m, 1) } }\n        if op == 6 { let r = d.pop_back()\n            if r[1] != arr::last(m) { salah = salah + 1 }\n            d = r[0]\n            if len(m) > 0 { m = arr::take(m, len(m) - 1) } }\n        if d.len() != len(m) { salah = salah + 1 }\n        if i % 97 == 0 { if d.to_array() != m { salah = salah + 1 } }\n    }\n    if d.to_array() != m { salah = salah + 1 }\n    return salah\n}\nfn uji_queue(benih: int, langkah: int) -> int {\n    var q: queue::Queue<int> = queue::new()\n    var m: []int = []\n    var s = benih\n    var salah = 0\n    for i in 0..langkah {\n        s = lcg(s)\n        let op = (s / 65536) % 5\n        if op <= 1 { q = q.push(i)\n            m = arr::push(m, i) }\n        if op == 2 || op == 3 { if q.peek() != arr::first(m) { salah = salah + 1 }\n            q = q.rest()\n            if len(m) > 0 { m = arr::skip(m, 1) } }\n        if op == 4 { let r = q.pop()\n            if r[1] != arr::first(m) { salah = salah + 1 }\n            q = r[0]\n            if len(m) > 0 { m = arr::skip(m, 1) } }\n        if q.len() != len(m) { salah = salah + 1 }\n        if i % 89 == 0 { if q.to_array() != m { salah = salah + 1 } }\n    }\n    return salah\n}\nfn uji_heap(benih: int, langkah: int) -> int {\n    var h: heap::Heap<int, int> = heap::new()\n    var m: []int = []\n    var s = benih\n    var salah = 0\n    for i in 0..langkah {\n        s = lcg(s)\n        let op = (s / 65536) % 5\n        let nilai = (s / 256) % 1000\n        if op <= 2 { h = h.push(nilai)\n            m = arr::sort(arr::push(m, nilai)) }\n        if op == 3 { if h.peek() != arr::first(m) { salah = salah + 1 }\n            h = h.rest()\n            if len(m) > 0 { m = arr::skip(m, 1) } }\n        if op == 4 { let r = h.pop()\n            if r[1] != arr::first(m) { salah = salah + 1 }\n            h = r[0]\n            if len(m) > 0 { m = arr::skip(m, 1) } }\n        if h.len() != len(m) { salah = salah + 1 }\n    }\n    if h.to_sorted_array() != m { salah = salah + 1 }\n    let negatif = heap::from_by([3, 9, 1, 7], |x| 0 - x)\n    if negatif.to_sorted_array() != [9, 7, 3, 1] { salah = salah + 1 }\n    return salah\n}\npub fn main() {\n    for b in [1, 7, 42, 2024] { wok(uji_deque(b, 1500), uji_queue(b, 1500), uji_heap(b, 600)) }\n}\n"),
    ("std/queue: FIFO dan antrean kosong menjawab None",
     "package main\nimport \"std/queue\"\npub fn main() { \
      var q = queue::new(); q = q.push(1); q = q.push(2); q = q.push(3); \
      wok(q.len()); wok(q.peek()); wok(q.to_array()); \
      var r = q.pop(); wok(r[1]); q = r[0]; wok(q.to_array()); \
      q = q.push(4); r = q.pop(); wok(r[1]); wok(r[0].to_array()); \
      wok(queue::new().pop()[1]); wok(queue::new().peek()); wok(q) }"),
    ("std/deque: dua ujung, dan pengisian ulang setengah",
     "package main\nimport \"std/deque\"\npub fn main() { \
      var d = deque::new(); d = d.push_back(2); d = d.push_back(3); d = d.push_front(1); \
      wok(d.to_array()); wok(d.len()); \
      var s = d.pop_front(); wok(s[1]); d = s[0]; \
      s = d.pop_back(); wok(s[1]); d = s[0]; wok(d.to_array()); \
      wok(deque::new().pop_front()[1]); wok(deque::new().pop_back()[1]); \
      wok(deque::from([1,2,3,4,5]).pop_front()[1]); wok(deque::from([1,2,3,4,5]).pop_back()[1]) }"),
    // Yang dikunci di sini bukan bentuknya melainkan URUTANNYA: menguras
    // sebuah heap harus keluar terurut, dan itu satu-satunya pemeriksaan
    // murah bahwa sift-up/sift-down-nya benar.
    ("std/heap: kuras selalu terurut, dengan dan tanpa fungsi kunci",
     "package main\nimport \"std/heap\"\npub fn main() { \
      let h = heap::from([5,3,8,1,9,2]); wok(h.len()); wok(h.peek()); \
      wok(h.to_sorted_array()); \
      let r = h.pop(); wok(r[1]); wok(r[0].to_sorted_array()); \
      wok(heap::new().pop()[1]); wok(heap::new().peek()); \
      wok(heap::from_by([\"ccc\",\"a\",\"bb\"], |s| len(s)).to_sorted_array()) }"),
    ("std/csv: koma dalam kutip, kutip ganda, baris baru dalam sel",
     "package main\nimport \"std/csv\"\npub fn main() { \
      wok(csv::parse(\"a,b,c\\n1,2,3\")); \
      wok(csv::parse(\"a,\\\"b,x\\\",c\")); \
      wok(csv::parse(\"a,\\\"say \\\"\\\"hi\\\"\\\"\\\",c\")); \
      wok(csv::parse(\"a,\\\"dua\\nbaris\\\",c\")); \
      wok(csv::parse(\"a,b\\n1,2\\n\")); wok(csv::parse(\"\")); wok(csv::parse(\"a,,c\")); \
      wok(csv::format([[\"a\",\"b\"],[\"1\",\"2\"]])); \
      wok(csv::format([[\"a,x\",\"b\\\"y\"]])); \
      wok(csv::to_dicts(csv::parse(\"nama,umur\\nadi,7\\nbudi,9\"))); \
      wok(csv::to_dicts(csv::parse(\"nama,umur\\nadi\"))) }"),
    // Nama method yang sama dengan operasi array, ditugaskan kembali ke
    // variabelnya sendiri.
    ("method pemakai bernama seperti operasi array, ditugaskan ke diri sendiri",
     "package main\npub struct K { pub isi: array }\n\
      impl K {\n\
      pub fn push(self, x) { return K { isi: arr::push(self.isi, x) } }\n\
      pub fn pop(self) { return K { isi: arr::take(self.isi, len(self.isi) - 1) } }\n\
      pub fn reverse(self) { return K { isi: arr::reverse(self.isi) } }\n\
      pub fn concat(self, o) { return K { isi: arr::concat(self.isi, o) } }\n\
      }\n\
      pub fn main() { var k = K { isi: [] }\n\
      k = k.push(1)\n k = k.push(2)\n wok(k.isi)\n\
      k = k.reverse()\n wok(k.isi)\n k = k.concat([9])\n wok(k.isi)\n\
      k = k.pop()\n wok(k.isi) }"),
    // Sisi positifnya: array biasa HARUS tetap lewat jalur cepat itu.
    // Tanpa kasus ini, mematikan optimasinya seluruhnya juga hijau.
    ("array biasa tetap memakai jalur ubah-milik",
     "package main\npub fn main() { var a = []\n a = a.push(1)\n a = a.push(2)\n\
      a = a.reverse()\n a = a.concat([9])\n wok(a)\n var b = [1,2,3]\n b = b.pop()[0]\n wok(b) }"),
    // Nama method yang bertabrakan dengan kata kunci. `.map(f)` sudah lama
    // bisa DIPANGGIL; mendeklarasikannya dulu gagal, jadi `map` adalah nama
    // yang bisa dipakai dan tidak bisa dibuat.
    ("nama method boleh kata kunci",
     "package main\npub struct K { pub isi: array }\n\
      impl K {\n pub fn map(self, f) { return K { isi: arr::map(self.isi, f) } }\n\
      pub fn type(self) { return \"K\" }\n\
      pub fn default(self) { return K { isi: [] } }\n }\n\
      pub fn main() { let k = K { isi: [1,2,3] }\n\
      wok(k.map(|x| x * 2).isi)\n wok(k.type())\n wok(k.default().isi) }"),
    ("std/path: ekstensi, stem, dan normalisasi . dan ..",
     "package main\nimport \"std/path\"\npub fn main() { \
      wok(path::split(\"/a/b/c\")); wok(path::split(\"\")); \
      wok(path::basename(\"/a/b/c.txt\")); wok(path::parent(\"/a/b/c\")); \
      wok(path::parent(\"a\")); wok(path::parent(\"/a\")); \
      wok(path::extension(\"a.tar.gz\")); wok(path::extension(\".gitignore\")); \
      wok(path::extension(\"abc\")); wok(path::extension(\"a.\")); \
      wok(path::stem(\"a.tar.gz\")); wok(path::stem(\".gitignore\")); \
      wok(path::with_extension(\"/a/b.txt\", \"md\")); wok(path::with_extension(\"b.txt\", \"\")); \
      wok(path::join(\"/a\", \"b\")); wok(path::join(\"/a/\", \"b\")); wok(path::join(\"a\", \"/b\")); \
      wok(path::join_all([\"a\",\"b\",\"c\"])); \
      wok(path::normalize(\"/a/../b\")); wok(path::normalize(\"a/../../b\")); \
      wok(path::normalize(\"/..\")); wok(path::normalize(\"\")); wok(path::normalize(\"./a/./b\")); \
      wok(path::is_absolute(\"/a\")); wok(path::is_absolute(\"a\")) }"),
    ("std/functional: gabungan, kari, dan penerapan berulang",
     "package main\nimport \"std/functional\"\npub fn main() { \
      wok(functional::compose(|x| x + 1, |x| x * 2)(5)); \
      wok(functional::pipe([|x| x * 2, |x| x + 1])(5)); \
      wok(functional::flip(|a, b| a - b)(1, 10)); \
      wok(functional::partial(|a, b| a - b, 10)(3)); \
      wok(functional::curry2(|a, b| a - b)(10)(3)); \
      wok(functional::identity(7)); wok(functional::constant(9)(1)); \
      wok(functional::apply_n(|x| x * 2, 3, 1)); \
      wok(functional::until(|x| x * 2, |x| x > 50, 1, 20)); \
      wok(functional::until(|x| x * 2, |x| x > 50, 1, 2)) }"),
    // Yang dikunci di sini BUKAN jawabannya melainkan berapa kali fungsi aslinya
    // benar-benar dipanggil.
    ("std/functional: memoize dan once MENAHAN panggilan, terukur",
     "package main\nimport \"std/functional\"\npub fn main() { \
      let c = functional::counted(|x| x * x)\n\
      let m = functional::memoize(c[0])\n\
      wok(m(4)); wok(m(4)); wok(m(4)); wok(m(5))\n\
      wok(\"panggilan nyata: \" + fmt::sprint(c[1]()))\n\
      let c2 = functional::counted(|x| x + 1)\n\
      let o = functional::once(c2[0])\n\
      wok(o(1)); wok(o(100)); wok(o(999))\n\
      wok(\"once memanggil: \" + fmt::sprint(c2[1]())) }"),
    // `**=` melengkapi keluarga operator gabungan.
    ("operator **= termasuk pada sasaran berindeks",
     "package main\npub fn main() { var a = 2\n a **= 10\n wok(a)\n\
      var b = 3.0\n b **= 2\n wok(b)\n var c = [2, 3]\n c[0] **= 5\n wok(c) }"),
    // Struct tuple diturunkan di parser jadi struct berfield `_0`, `_1`, … plus
    // fungsi konstruktor bernama sama.
    ("struct tuple: konstruksi, field, method, hook tampilan",
     "package main\nstruct Titik(int, int)\npub struct Bungkus(string)\n\
      impl Titik {\n\
      pub fn jumlah(self) { return self._0 + self._1 }\n\
      pub fn to_string(self) { return \"Titik(\" + fmt::sprint(self._0) + \", \" + \
        fmt::sprint(self._1) + \")\" }\n }\n\
      pub fn main() { let t = Titik(3, 4)\n\
      wok(t)\n wok(t._0)\n wok(t._1)\n wok(t.jumlah())\n\
      wok(Bungkus(\"halo\")._0)\n wok(typeof(t))\n\
      let u = t\n wok(u == t) }"),
    // Spread di pemanggilan (`f(...v)`) — pasangan dari parameter pengumpul yang
    // sudah ada.
    ("spread di pemanggilan, termasuk campuran dan kosong",
     "package main\nfn tambah(a, b) { return a + b }\n\
      fn banyak(a, sisa...) { return a + arr::sum(sisa) }\n\
      pub fn main() { let v = [2, 3]\n\
      wok(tambah(...v))\n wok(banyak(1, ...v))\n wok(banyak(...[1, 2, 3, 4]))\n\
      let f = |x, y| x * y\n wok(f(...v))\n\
      wok(tambah(1, ...[2]))\n wok(banyak(...[], 5)) }"),
    // `terap` adalah mesinnya, dan ia harus bisa dipanggil langsung maupun
    // lewat sebuah nama — bentuk tak-langsung itu yang membuktikan ia
    // dicegat lewat ALAMAT, bukan lewat nama di situs pemanggilan.
    ("terap langsung dan lewat nama",
     "package main\nfn tambah(a, b) { return a + b }\n\
      pub fn main() { wok(terap(tambah, [10, 20]))\n\
      let g = terap\n wok(g(tambah, [1, 1]))\n\
      wok(terap(|x| x * 3, [7]))\n\
      fn nol() { return 99 }\n wok(terap(nol, [])) }"),
    // Argumen bernama.
    ("argumen bernama, termasuk urutan terbalik dan closure",
     "package main\nfn bagi(pembilang, penyebut) { return pembilang / penyebut }\n\
      fn sapa(nama, salam = \"halo\", tanda = \"!\") { return salam + \" \" + nama + tanda }\n\
      pub fn main() { wok(bagi(pembilang: 10, penyebut: 2))\n\
      wok(bagi(penyebut: 2, pembilang: 10))\n wok(bagi(10, penyebut: 2))\n\
      wok(sapa(\"dunia\"))\n wok(sapa(\"dunia\", salam: \"hai\"))\n\
      wok(sapa(nama: \"dunia\", salam: \"hai\", tanda: \"?\"))\n\
      let f = |a, b| a - b\n wok(f(b: 1, a: 9)) }"),
    // Pola di POSISI PARAMETER.
    ("pola di parameter fn dan closure",
     "package main\n\
      fn jarak((x1, y1), (x2, y2)) { return math::abs(x2 - x1) + math::abs(y2 - y1) }\n\
      fn kepala([a, ...sisa]) { return fmt::sprint(a) + \"/\" + fmt::sprint(sisa) }\n\
      fn campur(n, (a, b)) { return n * (a + b) }\n\
      fn ekspr((a, b)) = a * b\n\
      pub fn main() { wok(jarak((0, 0), (3, 4)))\n\
      wok(kepala([1, 2, 3]))\n wok(campur(2, (3, 4)))\n wok(ekspr((6, 7)))\n\
      let f = |(a, b)| a + b\n wok(f((1, 2)))\n\
      let g = |(a, b)| { let s = a * b\n return s + 1 }\n wok(g((3, 4)))\n\
      wok(arr::map([[1,2],[3,4]], |[a, b]| a * b)) }"),
    // Penjaga bentuk-asing di atas tidak boleh menyambar nama biasa: `switch`
    // dan `try` bukan kata kunci di JRG, jadi keduanya sah sebagai variabel.
    ("switch dan try tetap sah sebagai nama biasa",
     "package main\npub fn main() { var switch = 5\n var try = 7\n\
      wok(switch + try) }"),
    // Pola di posisi NILAI sebuah `for`.
    ("pola di posisi nilai for, dan bentuk lama tak berubah",
     "package main\npub fn main() {\n\
      for _, (a, b) in [(1,2),(3,4)] { wok(a + b) }\n\
      for _, [a, b] in [[1,2],[3,4]] { wok(a * b) }\n\
      for i, (a, b) in [(10,20)] { wok(fmt::sprint(i) + \":\" + fmt::sprint(a + b)) }\n\
      for i, x in [10, 20] { wok(fmt::sprint(i) + \"=\" + fmt::sprint(x)) }\n\
      for i in [7, 8] { wok(i) }\n\
      for i := 0; i < 2; i = i + 1 { wok(i) } }"),
    // Sisi POSITIF ketiga penjaga baru: bentuk yang sah tidak boleh ikut
    // tersambar.
    ("slice, pola struct, dan path tetap sah",
     "package main\nstruct T { pub x: int }\npub fn main() {\n\
      wok([1,2,3,4][1:3])\n wok([1,2,3,4][:2])\n wok([1,2,3,4][2:])\n\
      let T { x } = T{x:5}\n wok(x)\n\
      let d = {\"a\": 1}\n wok(d[\"a\"])\n wok(arr::sum([1,2])) }"),
    // Nama KIND yang dijawab `typeof` harus bisa ditulis sebagai anotasi.
    // Sebuah dict dulu tidak bisa dianotasi dalam bentuk apa pun.
    ("nama typeof bisa ditulis sebagai anotasi",
     "package main\npub fn main() { let a: dict = dict::new()\n let b: tuple = (1,2)\n\
      let c: function = |x| x\n let d: closure = |x| x\n let e: native_fn = wok\n\
      wok(typeof(a)); wok(typeof(b)); wok(typeof(c)); wok(typeof(e)) }"),

    // ── Fase LABEL: data tidak bisa memalsukan jenis sebuah nilai ────────
    // `--vm` dulu mencetak struct ber-field `variant` sebagai `V::None` dengan
    // `typeof` `enum:V`, dan struct ber-field `__type__` sebagai tipe lain.
    ("label: field struct bernama variant dan __type__ tetap field",
     "package main\nstruct V { pub variant: string }\nstruct T { pub __type__: string, pub isi: int }\n\
      pub fn main() { var v = V { variant: \"a\" }\n v.variant = \"None\"\n\
      wok(v); wok(typeof(v)); wok(v.variant)\n\
      wok(match v { None => \"enum\", _ => \"struct\" })\n\
      let t = T { __type__: \"Option\", isi: 1 }\n wok(t); wok(typeof(t)); wok(t.__type__) }"),
    // `--vm` dulu menyembunyikan key dict pemakai yang berawalan `__`.
    ("label: key dict berawalan __ tetap terlihat",
     "package main\npub fn main() { let d = {\"__x\": 1, \"a\": 2, \"variant\": \"Some\", \"__payload__\": 3}\n\
      wok(d); wok(typeof(d)); wok(dict::keys(d)); wok(d.variant)\n\
      wok(match d { Some(n) => n, _ => -1 }) }"),
    // JSON adalah DATA: objek berkunci `variant` dulu jadi ENUM di interpreter lewat
    // jembatan nilai, dan tiga kunci label memalsukan `Option::Some(5)` di dua backend.
    ("label: JSON tidak bisa memalsukan enum",
     "package main\npub fn main() {\n\
      let a = json::parse(\"{\\\"variant\\\":\\\"x\\\",\\\"n\\\":1}\")\n wok(typeof(a)); wok(a)\n\
      let b = json::parse(\"{\\\"__type__\\\":\\\"Option\\\",\\\"variant\\\":\\\"Some\\\",\\\"__payload__\\\":5}\")\n\
      wok(typeof(b)); wok(match b { Some(n) => n, _ => -1 })\n\
      wok(json::stringify({\"__type__\": 1, \"a\": 2})) }"),
    // `--vm` dulu menjawab `E::V(V {a: __payload__1})`: lima konstanta label di bawah
    // literal payload tidak ikut hitungan sementara, jadi slot lokal di dalamnya meleset.
    ("varian struct enum dengan ekspresi yang punya lokal",
     "package main\nenum E { V { a: int, b: int } }\n\
      pub fn main() { let e = E::V { a: match 3 { n => n + 1 }, b: if true { let k = 5\n k * 2 } else { 0 } }\n\
      wok(e)\n wok(match e { E::V { a, b } => a + b }) }"),
    // Spread: `--vm` dulu tidak mengisi default untuk field yang tidak dibawa basisnya,
    // dan membuang field varian enum yang dipakai sebagai basis.
    ("spread: default diisi untuk field yang tidak dibawa basis",
     "package main\nstruct R { pub a: int }\nstruct P { pub a: int, pub b: int = 2 }\n\
      struct A { pub x: int = 7 }\nstruct B: A { pub y: int = 8 }\n\
      struct S { pub a: int, rahasia: int = 3 }\nimpl S { fn r(self) { return self.rahasia } }\n\
      pub fn main() { wok(P { a: 1, ...R { a: 5 } })\n wok(P { a: 1, ...{\"a\": 9} })\n\
      wok(B { y: 1, ...{\"y\": 0} })\n let s = S { a: 1, ...{\"a\": 2} }\n wok(s.r()) }"),
    ("spread: varian enum ber-field sebagai basis",
     "package main\nenum E { V { a: int, b: int } }\nstruct P { pub a: int, pub b: int = 2, pub c: int = 3 }\n\
      pub fn main() { let e = E::V { a: 1, b: 20 }\n wok(E::V { a: 9, ...e })\n wok(P { a: 5, ...e }) }"),
    // ── Pintu method HOF (MESIN-2) ─────────────────────────────────────────
    // Aritas diputuskan SAAT JALAN: method pemakai bernama `map`/`filter`/`zip_with` dengan aritas
    // lain dulu ditolak `--vm` saat kompilasi, tanpa lokasi.
    ("hof: method pemakai bernama seperti HOF, aritas bebas",
     "package main\nstruct P { pub a: int }\nimpl P {\n\
      fn map(self, f, g) { return f(self.a) + g(self.a) }\n\
      fn filter(self) { return self.a * 7 }\n\
      fn zip_with(self, n, f) { return f(self.a, n) }\n\
      fn reduce(self) { return 99 }\n\
      fn each(self, a, b, c) { return a + b + c }\n}\n\
      pub fn main() { let p = P { a: 1 }\n wok(p.map(|x| x + 1, |x| x * 10))\n wok(p.filter())\n\
      wok(p.zip_with(2, |a, b| a + b))\n wok(p.reduce())\n wok(p.each(1, 2, 3)) }"),
    ("hof: warisan dan default trait bernama HOF",
     "package main\nstruct Induk { pub a: int }\nimpl Induk { fn map(self, k) { return self.a + k } }\n\
      struct Anak: Induk { }\ntrait Hitung { fn fold(self) = 42 }\nimpl Hitung for Anak { }\n\
      pub fn main() { let c = Anak { a: 5 }\n wok(c.map(10))\n wok(c.fold()) }"),
    ("hof: method enum pemakai bernama map",
     "package main\nenum W { A, B }\nimpl W { fn map(self, n) { return match self { A => n, B => n * 2 } } }\n\
      pub fn main() { wok(W::B.map(21)) }"),
    // `--vm` dulu melanjutkan bingkai PEMANGGIL dari tengah HOF begitu callback-nya native:
    // program ini mencetak `di f 1` dan `hasil 2` sebelum gagal.
    ("hof: fungsi native sebagai callback",
     "package main\nfn f() {\n let b = [-1, -2].map(abs)\n wok(\"di f\", b)\n return 7\n}\n\
      pub fn main() { wok(\"hasil\", f())\n wok(arr::map([\"a\"], str::upper))\n\
      wok([\"1\", \"a\"].filter(str::is_numeric))\n wok([-3, 1, -2].sort_by_key(abs))\n\
      wok(terap(abs, [-5]))\n wok(\"sebelum\")\n [1, 2].each(wok)\n wok(\"sesudah\") }"),
    // Sebar lewat `arr::__sebar`: isi yang disebar sendirian dipakai bersama, jadi yang dikunci
    // bahwa menulis ke hasilnya tidak bocor ke asalnya, termasuk di tingkat yang lebih dalam.
    ("sebar: hasil tidak beralias dengan asalnya",
     "package main\nfn f(a, b) { return a + b }\n\
      pub fn main() { let a = [[1], [2]]\n var b = [...a]\n b[0][0] = 9\n wok(a, b)\n\
      var c = [...a, ...a]\n c[1][0] = 7\n wok(a, c)\n wok(f(...[1, 2]))\n wok(f(1, ...[2])) }"),

    // Penugasan gabungan memakai operator dan hook milik pemakai, seperti `a = a op b`;
    // dulu interpreter menolaknya ("tidak bisa `+` antara `struct:V` dan `struct:V`").
    ("operator pemakai: penugasan gabungan di tiap bentuk sasaran, index dihitung sekali",
     "package main\nstruct V { pub x: int }\n\
      impl V { fn add(self, o) { return V { x: self.x + o.x } }\n fn mul(self, o) { return V { x: self.x * o.x } }\n\
      fn to_string(self) -> string { return \"<\" + str(self.x) + \">\" } }\n\
      struct K { pub v: V }\nvar G = V { x: 100 }\nfn hitung() { wok(\"idx\")\n return 0 }\n\
      pub fn main() { var p = V { x: 1 }\n let q = V { x: 5 }\n p = p + q\n p += q\n p *= q\n\
      var a = [V { x: 1 }]\n a[hitung()] += V { x: 2 }\n var k = K { v: V { x: 3 } }\n k.v *= V { x: 4 }\n\
      var t = (V { x: 1 }, 2)\n t.0 += V { x: 1 }\n var d = {\"k\": V { x: 7 }}\n d[\"k\"] += V { x: 1 }\n\
      G += V { x: 1 }\n let f = || { a[0] += V { x: 10 } }\n f()\n wok(p, a, k, t, d, G)\n\
      var s = \"n=\"\n s += V { x: 9 }\n var u = [\"p\"]\n u[0] += V { x: 1 }\n wok(s, u)\n\
      var n = [1, 2.5]\n n[0] += 2\n n[1] *= 2\n n[0] **= 2\n wok(n) }"),

    // `jalur = jalur + x` dan `jalur += x` tumbuh di tempat; salinan lain tidak boleh ikut berubah.
    ("tumbuh di tempat lewat field, index, dict dan tuple: tanpa alias, kanan dihitung sekali",
     "package main\nstruct P { pub xs: []int, pub s: string, rahasia: []int }\nimpl P { fn isi(self, v) { self.rahasia = self.rahasia + [v]; self.rahasia += [v]; return self } }\nstruct Q { pub p: P }\nfn bunyi() { wok(\"kanan\"); return [7] }\npub fn main() {\n    var p = P { xs: [], s: \"\", rahasia: [] }\n    let salin = p\n    for i := 0; i < 3; i = i + 1 { p.xs = p.xs + [i]; p.xs += [i * 10]; p.s = p.s + \"a\"; p.s += \"b\" }\n    wok(p, salin)\n    var q = Q { p: p }\n    let q2 = q\n    q.p.xs = q.p.xs + bunyi()\n    var a = [[1], [2]]\n    let b = a\n    var i = 1\n    a[i] = a[i] + [9]\n    a[0] += [8]\n    var d = {\"k\": [1]}\n    d[\"k\"] = d[\"k\"] + [2]\n    var t = ([1], \"x\")\n    t.0 = t.0 + [5]\n    t[1] = t[1] + \"y\"\n    wok(q, q2.p.xs, a, b, d, t, p.isi(4))\n    p.s = p.s + 5\n    p.xs = p.xs + [1.5] + [2]\n    wok(p)\n}\n"),

    // Fase TULIS-TUPLE: elemen tuple bisa ditulis (dulu cuma di backend LLVM), tanpa alias.
    ("tuple: elemen ditulis lewat index dan .N, salinan tidak ikut berubah",
     "package main\nstruct K { pub t: (int, []int) }\n\
      fn ubah(t: (int, []int)) -> int { t[0] = 50\n t[1][0] = 60\n return t[0] + t[1][0] }\n\
      pub fn main() { var t = (1, [2, 3])\n var u = t\n u[0] = 7\n u[1][1] = 9\n wok(t, u, ubah(u), u)\n\
      u[0] += 1\n u.1 = [0]\n u[-1] = [4]\n wok(u, typeof(u), t == (1, [2, 3]), t < t)\n\
      var k = K { t: t }\n var k2 = k\n k2.t[1][0] = 5\n k2.t.0 = 8\n wok(k, k2)\n\
      var b = [(1, 2), (3, 4)]\n let s = b\n b[1][0] = 30\n wok(b, s)\n\
      let f = || { t[0] = 100\n return t }\n wok(f(), t) }"),

    // Rentang di kepala `for` tidak dijadikan array: dulu lebih dari 16 juta putaran ditolak
    // di dua mesin acuan (LLVM sudah menjalankannya), dan `for i in n` di `--vm` membuat n elemen.
    ("for atas rentang besar: malas, artinya tetap indeks",
     "for i in 0..100000000000 { if i == 3 { break }\n wok(i) }\n\
      for i in 100000000000 { if i == 2 { break }\n wok(\"n\", i) }\n\
      var s = 0\nfor i in 3..1 { s += 1 }\nfor i in 2..=2 { s += 10 }\nwok(s)\n"),
    // `var x: T` tanpa `=` mendapat nilai nol tipenya, seperti di LLVM — cuma untuk tipe
    // yang nolnya jelas (angka, bool, teks, rune, `[N]T`, `[]T`, tuple).
    ("var bertipe tanpa nilai awal: nilai nol tipenya",
     "package main\npub fn main() { var a: [3]int\n a[1] = 7\n var b = a\n b[0] = 9\n\
      var x: int\n var f: f64\n var s: string\n var t: (int, bool)\n var m: [2][2]bool\n\
      var k: []int\n wok(a, b, x, f, s == \"\", t, m, k) }"),
    ("var tingkat modul tanpa nilai awal: nilai nol tipenya",
     "package main\nvar G: [2]int\nvar H: f64\npub fn main() { G[0] = 4\n wok(G, H) }"),    // Satu nama di `for` selalu INDEKS, juga untuk rentang yang tidak mulai dari 0 — LLVM dulu
    // menjawab nilainya (`5 6 7`), dan bentuk dua nama `5 5`.
    // Tumbuh di tempat juga untuk rantai `s = s + a + b` dan sasaran berjalur (`b.s += x`,
    // `a[i] += x`) — hasilnya wajib sama dengan jalur biasa, termasuk hook dan salinan.
    ("tumbuh di tempat: rantai dan sasaran berjalur, hasil sama dengan jalur biasa",
     "package main\nstruct P { pub v: int }\nimpl P { fn to_string(self) { return \"P\" + str(self.v) } }\n\
      struct B { pub s: string, pub xs: []int }\n\
      pub fn main() { var s = \"x\"\n s = s + 1 + P { v: 2 } + true + [3, 4]\n wok(s)\n\
      var a = [1]\n a = a + [2] + [3, 4]\n var u = \"u\"\n u = u + u + \"!\"\n var c = [1]\n c = c + [2] + \"z\"\n wok(a, u, c)\n\
      var b = B { s: \"a\", xs: [1] }\n let salinan = b\n b.s += \"b\"\n b.s += 1\n b.s += P { v: 7 }\n b.xs += [2, 3]\n b.s += [9]\n\
      wok(b.s, b.xs, salinan.s, salinan.xs)\n\
      var t = [\"x\", \"y\"]\n let t2 = t\n t[-1] += \"z\"\n t[0] += P { v: 1 }\n var m = [[1], [2]]\n m[1] += [5]\n\
      var d = {\"k\": \"v\"}\n d[\"k\"] += \"w\"\n wok(t, t2, m, d) }"),
    // Closure di dalam method masih "di dalam tipe itu": dulu interpreter menolaknya kalau
    // closure tidak menyebut `self`, karena aksesnya diputuskan dari variabel `self`.
    ("field privat: closure di dalam method, dengan atau tanpa menyebut self",
     "struct Base { rahasia: int = 7 }\n\
      impl Base { fn intip(self, o) { let g = || o.rahasia\n return g() }\n\
      fn lewat(self, o) { let f = |x| x.rahasia\n return f(o) }\n\
      fn buka(self) { return |o| o.rahasia } }\n\
      let b = Base {}\nwok(b.intip(Base { rahasia: 3 }), b.lewat(Base { rahasia: 5 }))\n\
      let g = b.buka()\nwok(g(Base { rahasia: 4 }))\n"),
    ("field privat: deklarasi terdekat yang menentukan",
     "struct Base { rahasia: int = 7 }\nstruct T: Base { pub rahasia: int = 9 }\nlet t = T {}\nwok(t.rahasia)\n"),
    ("method default trait tidak membayangi fungsi global bernama sama",
     "fn sapa(x) { return \"global\" }\ntrait T { fn sapa(self) { return sapa(1) } }\n\
      struct S { pub a: int }\nimpl T for S { }\nlet s = S { a: 1 }\nwok(s.sapa())\n\
      struct R { pub a: int }\nimpl R { fn ulang(self, n) { if n == 0 { return 0 } return 1 + R::ulang(self, n - 1) } }\n\
      wok(R { a: 1 }.ulang(5))\n"),
    // `a op= b` = `a = a op b` dengan aturan `Expr::Binary`: interpreter dulu menolak
    // operator `add` milik pemakai dan melewatkan hook tampilan di semua bentuk sasaran.
    ("operator gabungan: operator pemakai dan hook tampilan di tiap bentuk sasaran",
     "package main\nstruct V { pub x: int }\n\
      impl V { fn add(self, o) { return V { x: self.x + o.x } }  fn to_string(self) { return \"V\" + str(self.x) } }\n\
      struct K { pub v: V, pub s: string }\nvar G = V { x: 1 }\nvar GS = \"g\"\n\
      pub fn main() { var p = V { x: 1 }\n p = p + V { x: 2 }\n p += V { x: 10 }\n wok(p.x)\n\
      G += V { x: 5 }\n GS += V { x: 7 }\n wok(G.x, GS)\n\
      var k = K { v: V { x: 1 }, s: \"k\" }\n k.v += V { x: 2 }\n k.s += V { x: 3 }\n wok(k.v.x, k.s)\n\
      var a = [V { x: 1 }]\n a[0] += V { x: 4 }\n var t = \"t\"\n t += V { x: 9 }\n wok(a[0].x, t) }"),
    ("for atas rentang tak berawal 0: indeks, lalu (indeks, nilai)",
     "for i in 5..8 { wok(i) }\nfor i, x in 5..=7 { wok(i, x) }\nlet a = 3\nfor i, x in a..a + 2 { wok(i, x) }\n"),
];
