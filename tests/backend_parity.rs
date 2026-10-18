// tests/backend_parity.rs

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

struct Hasil {
    keluaran: String,
    sukses: bool,
}

/// Keluaran sebuah proses, plus apakah ia SELESAI sendiri.
struct Keluaran {
    stdout: Vec<u8>,
    /// Dikuras terpisah dan DISIMPAN — pesan galat keluar lewat sini, dan
    /// kasus yang menguncinya (mis. jejak pemanggilan) perlu membacanya.
    stderr: Vec<u8>,
    status_sukses: bool,
    /// Kode keluar apa adanya; `None` kalau dibunuh (batas waktu atau sinyal).
    kode: Option<i32>,
}

/// Batas waktu untuk satu program uji.
const BATAS_DETIK: u64 = 30;

/// Jalankan `jrgc` dengan BATAS WAKTU, bukan `Command::output()`.
fn jalankan_berbatas(mut cmd: Command) -> Keluaran {
    use std::io::Read;
    use std::process::Stdio;

    // Stdin kosong: tanpa ini program uji mewarisi terminal `cargo test`, dan
    // `fmt::tok` di sebuah sapuan menunggu ketikan sampai batas waktunya.
    let mut anak = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("jalankan jrgc");

    let mut so = anak.stdout.take().expect("stdout");
    let mut se = anak.stderr.take().expect("stderr");
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });

    let batas = std::time::Instant::now() + std::time::Duration::from_secs(BATAS_DETIK);
    let status = loop {
        match anak.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(_) => break None,
        }
        if std::time::Instant::now() >= batas {
            let _ = anak.kill();
            let _ = anak.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    Keluaran {
        stdout,
        stderr,
        // Dibunuh karena kehabisan waktu dihitung GAGAL, dan itu memang
        // jawaban yang benar: program yang tidak berhenti tidak menghasilkan
        // apa pun yang bisa dibandingkan.
        status_sukses: status.map(|s| s.success()).unwrap_or(false),
        kode: status.and_then(|s| s.code()),
    }
}

fn jalankan(src: &str, pakai_vm: bool) -> Hasil {
    let dir = crate::bersama::dir_unik("jrg-paritas");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");

    let mut cmd = Command::new(jrgc());
    cmd.arg(&file);
    if pakai_vm { cmd.arg("--vm"); }
    let out = jalankan_berbatas(cmd);

    let _ = std::fs::remove_dir_all(&dir);
    Hasil {
        keluaran: String::from_utf8_lossy(&out.stdout).trim().to_string(),
        sukses: out.status_sukses,
    }
}

/// Keluaran lengkap (stdout + stderr) dari backend yang dipilih.
fn jalankan_keluaran_penuh(src: &str, pakai_vm: bool) -> String {
    let dir = crate::bersama::dir_unik("jrg-paritas-err");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let mut cmd = Command::new(jrgc());
    cmd.arg(&file);
    if pakai_vm { cmd.arg("--vm"); }
    // BERBATAS, seperti `jalankan`.
    let out = jalankan_berbatas(cmd);
    let _ = std::fs::remove_dir_all(&dir);
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Program yang arti dan keluarannya WAJIB sama di kedua backend — dan di LLVM, kalau
/// LLVM menjalankannya (`tests/llvm_parity.rs`).
include!("bersama/paritas.rs");

/// Konstruksi yang MEMANG masih berbeda arti antar-backend, dan sudah
/// didokumentasikan. Kosong berarti tidak ada program yang jalan di dua
/// backend lalu menjawab beda.
const DIVERGENSI_DIKETAHUI: &[(&str, &str, &str)] = &[];

/// Program yang DITERIMA VM tapi ditolak interpreter.
const VM_LEBIH_PERMISIF: &[(&str, &str, &str)] = &[
    // KOSONG — dan itu artinya dua backend sekarang setara persis: tidak ada
    // satu pun program yang diterima salah satu tapi ditolak yang lain.
];


/// Konstruksi yang BELUM didukung VM.
const BELUM_DIDUKUNG_VM: &[(&str, &str, &str)] = &[
    // KOSONG.
];

/// Namespace yang SENGAJA cuma ada di interpreter (`dok::`, `py::`) — beda
/// dari `BELUM_DIDUKUNG_VM` di atas.
const NAMESPACE_INTERPRETER_SAJA: &[(&str, &str, &str)] = &[
    // KOSONG sejak Fase FFI-VM: `py::`/`js::`/`php::`/`lua::` jalan di dua mesin lewat satu
    // pintu (`interpreter::builtin::ffi_pintu`). Tabelnya dipertahankan untuk celah berikutnya.
];

/// Berkas konformans yang sengaja ditulis untuk jalan di DUA backend.
const KONFORMANS_DUA_BACKEND: &[&str] = &[
    "semantik_nilai.jrg",
    // Ditambahkan di fase copy-on-write.
    "semantik_cow.jrg",
];

#[test]
fn konformans_semantik_nilai_jalan_di_dua_backend() {
    for nama in KONFORMANS_DUA_BACKEND {
        let berkas = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests").join("conformance").join(nama);
        let src = std::fs::read_to_string(&berkas)
            .unwrap_or_else(|e| panic!("baca {}: {}", berkas.display(), e));

        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert!(i.sukses, "{}: interpreter gagal:\n{}", nama, i.keluaran);
        assert!(
            v.sukses,
            "{}: VM gagal padahal berkas ini memang ditulis untuk dua backend.\n\
             Kalau ini karena fitur baru yang VM belum dukung, pindahkan fitur itu \
             ke berkas konformans lain — jangan biarkan berkas ini gagal.\nVM: {}",
            nama, v.keluaran
        );
        assert_eq!(i.keluaran, v.keluaran, "{}: keluaran dua backend berbeda", nama);
    }
}

/// Program yang WAJIB ditolak oleh KEDUA backend, dengan alasan yang sama.
const DITOLAK_DUA_BACKEND: &[(&str, &str, &str, &str)] = &[
    // Fase KALIMAT: acuan dulu menjawab kalimat jembatan ("nilai balik `channel` tidak punya padanan di VM").
    ("json::stringify atas channel", "package main\npub fn main() {\n let a = make_chan(int)\n wok(json::stringify(a))\n}",
     "`channel` tidak punya padanan di JSON, jadi ia tidak bisa disandikan", "`channel` tidak punya padanan di JSON, jadi ia tidak bisa disandikan"),
    // Fase SEBAB: dulu `[]` di dua mesin, sama dengan direktori kosong (kasus PARITAS lama).
    ("fs::walk atas jalur yang tidak ada", "package main\npub fn main() { wok(fs::walk(\"/pasti/tidak/ada/xyz\")) }",
     "fs::walk '/pasti/tidak/ada/xyz': berkas atau direktorinya tidak ada", "fs::walk '/pasti/tidak/ada/xyz': berkas atau direktorinya tidak ada"),
    // Fase SEBAR: basis `...` tidak menyelundupkan field di luar deklarasi sasarannya, dan varian
    // telanjang milik dua enum ditolak parser (dulu dua mesin memilih satu diam-diam).
    ("basis sebar membawa field liar ke struct", "package main\nstruct A { pub x: int, pub w: int }\nstruct B { pub x: int, pub y: int = 7 }\nfn catat(s: string) -> int { wok(s); return 1 }\npub fn main() {\n let a = A { x: 1, w: 2 }\n wok(B { y: catat(\"tidak sampai\"), ...a })\n}", "basis `...` membawa field `w` yang tidak dideklarasikan struct `B`", "basis `...` membawa field `w` yang tidak dideklarasikan struct `B`"),
    ("basis sebar membawa field liar ke varian", "package main\nstruct A { pub a: int, pub zz: int }\nenum E { V { a: int, b: int }, K }\npub fn main() {\n let s = A { a: 1, zz: 2 }\n wok(E::V { b: 3, ...s })\n}", "basis `...` membawa field `zz` yang tidak dideklarasikan varian `E::V`", "basis `...` membawa field `zz` yang tidak dideklarasikan varian `E::V`"),
    ("varian unit telanjang milik dua enum", "package main\nenum E { A(int), K, V { a: int } }\nenum F { A(int), K, V { a: int } }\npub fn main() {\n wok(K)\n}",
     "`K` ambigu — varian bernama itu ada di E dan F", "`K` ambigu — varian bernama itu ada di E dan F"),
    ("konstruktor telanjang milik dua enum", "package main\nenum E { A(int), K, V { a: int } }\nenum F { A(int), K, V { a: int } }\npub fn main() {\n wok(A(1))\n}",
     "`A` ambigu — varian bernama itu ada di E dan F", "`A` ambigu — varian bernama itu ada di E dan F"),
    ("literal varian telanjang milik dua enum", "package main\nenum E { A(int), K, V { a: int } }\nenum F { A(int), K, V { a: int } }\npub fn main() {\n wok(V { a: 1 })\n}",
     "`V` ambigu — varian bernama itu ada di E dan F", "`V` ambigu — varian bernama itu ada di E dan F"),
    ("pola varian telanjang milik dua enum", "package main\nenum E { A(int), K, V { a: int } }\nenum F { A(int), K, V { a: int } }\npub fn main() {\n match F::K { K => wok(1), _ => wok(2) }\n}",
     "`K` ambigu — varian bernama itu ada di E dan F", "`K` ambigu — varian bernama itu ada di E dan F"),
    // Fase WAJIB: field tanpa default yang tidak diisi — dulu nilainya lahir tanpa field itu.
    ("literal struct tanpa field wajib",
     "package main\nstruct B { x: int, pub y: int }\npub fn main() { wok(1)\n let b = B { x: 1 }\n wok(b) }",
     "struct `B`: field `y` wajib diisi (tidak ada nilai default)", "struct `B`: field `y` wajib diisi (tidak ada nilai default)"),
    ("literal struct kosong atas struct ber-field wajib",
     "package main\nstruct B { x: int = 1, pub y: int }\npub fn main() { wok(B {}) }",
     "struct `B`: field `y` wajib diisi (tidak ada nilai default)", "struct `B`: field `y` wajib diisi (tidak ada nilai default)"),
    ("literal struct tanpa field wajib milik induk",
     "package main\nstruct A { pub x: int }\nstruct B: A { pub y: int }\npub fn main() { wok(B { y: 2 }) }",
     "struct `B`: field `x` wajib diisi (tidak ada nilai default)", "struct `B`: field `x` wajib diisi (tidak ada nilai default)"),
    ("Self { .. } di method warisan tanpa field milik anak",
     "package main\nstruct A { pub x: int }\nstruct B: A { pub y: int }\nimpl A { fn baru(self) -> Self { Self { x: self.x + 1 } } }\npub fn main() { let b = B { x: 1, y: 2 }\n wok(b.baru()) }",
     "struct `B`: field `y` wajib diisi (tidak ada nilai default)", "struct `B`: field `y` wajib diisi (tidak ada nilai default)"),
    ("anak mendeklarasikan ulang field tanpa default: default induk tidak dipakai",
     "package main\nstruct A { pub x: int = 1 }\nstruct B: A { pub x: int }\npub fn main() { wok(B {}) }",
     "struct `B`: field `x` wajib diisi (tidak ada nilai default)", "struct `B`: field `x` wajib diisi (tidak ada nilai default)"),
    ("basis sebar yang tidak membawa field wajib",
     "package main\nstruct A { pub x: int, pub y: int }\nstruct H { pub x: int }\npub fn main() { let h = H { x: 1 }\n wok(A { x: 5, ...h }) }",
     "struct `A`: field `y` wajib diisi (tidak ada nilai default)", "struct `A`: field `y` wajib diisi (tidak ada nilai default)"),
    ("varian ber-field tanpa salah satu field",
     "package main\nenum E { V { a: int, b: int }, W }\npub fn main() { wok(E::V { a: 1 }) }",
     "varian `E::V` butuh field `b`", "varian `E::V` butuh field `b`"),
    ("varian ber-field telanjang tanpa salah satu field",
     "package main\nenum E { V { a: int, b: int }, W }\npub fn main() { wok(V { b: 1 }) }",
     "varian `E::V` butuh field `a`", "varian `E::V` butuh field `a`"),
    // Fase SINKRON: `Atomic`/`Mutex` sebagai pegangan sungguhan di dua mesin.
    ("atomic: add di batas 128 bit",
     "let a = Atomic::new(170141183460469231731687303715884105727)\nwok(\"x\")\natomic::add(a, 1)\n", "kelewat besar", "kelewat besar"),
    ("atomic: sub di batas 128 bit",
     "let a = Atomic::new(-170141183460469231731687303715884105727)\natomic::sub(a, 2)\n", "kelewat besar", "kelewat besar"),
    ("atomic: nilai awal pecahan", "wok(Atomic::new(2.5))\n", "butuh bilangan bulat, dapat `float`", "butuh bilangan bulat, dapat `float`"),
    ("atomic: dict menyamar Atomic", "wok(atomic::get({\"_val\": [42]}))\n", "butuh sebuah Atomic", "butuh sebuah Atomic"),
    ("atomic: set dengan bool", "let a = Atomic::new(1)\natomic::set(a, true)\n", "butuh bilangan bulat, dapat `bool`", "butuh bilangan bulat, dapat `bool`"),
    ("atomic: Mutex bukan Atomic", "wok(atomic::get(Mutex::new()))\n", "dapat `mutex`", "dapat `mutex`"),
    ("atomic: tidak punya urutan", "let a = Atomic::new(1)\nwok(a < a)\n", "tidak bisa membandingkan `atomic`", "tidak bisa membandingkan `atomic`"),
    ("atomic: tidak punya padanan JSON", "wok(json::stringify(Atomic::new(1)))\n", "tidak punya padanan di JSON", "tidak punya padanan di JSON"),
    ("mutex: mengunci dua kali dari goroutine yang sama",
     "let m = Mutex::new()\nmutex::lock(m)\nwok(\"satu\")\nmutex::lock(m)\n", "sudah kamu pegang", "sudah kamu pegang"),
    ("mutex: melepas yang tidak terkunci", "let m = Mutex::new()\nmutex::unlock(m)\n", "tidak sedang terkunci", "tidak sedang terkunci"),
    ("mutex: dilepas goroutine lain",
     "let m = Mutex::new()\nmutex::lock(m)\nlet t = Task::spawn(|k| mutex::unlock(k), m)\ntask::join(t)\n", "dipegang goroutine lain", "dipegang goroutine lain"),
    ("mutex: ditinggal terkunci goroutine yang selesai",
     "let m = Mutex::new()\nlet t = Task::spawn(|k| mutex::lock(k), m)\ntask::join(t)\ntime::sleep_ms(100)\nmutex::lock(m)\n", "ditinggal terkunci", "ditinggal terkunci"),
    ("mutex: with_lock bersarang atas kunci yang sama",
     "let m = Mutex::new()\nmutex::with_lock(m, || mutex::with_lock(m, || 1))\n", "sudah kamu pegang", "sudah kamu pegang"),
    ("mutex: tidak menyimpan nilai", "wok(Mutex::new(5))\n", "kebanyakan argumen", "kebanyakan argumen"),
    // Fase PEGANGAN: bentuk method `Atomic`/`Mutex`, dan handle pustaka yang tidak bisa ditulisi.
    ("atomic: method kurang argumen", "let a = Atomic::new(1)\nwok(\"x\")\na.add()\n", "method `add` butuh 1 argumen (nilai), dapat 0", "method `add` butuh 1 argumen (nilai), dapat 0"),
    ("atomic: method cas kurang argumen", "let a = Atomic::new(1)\na.cas(1)\n", "method `cas` butuh 2 argumen (lama, baru), dapat 1", "method `cas` butuh 2 argumen (lama, baru), dapat 1"),
    ("atomic: konstruktor bukan method", "let a = Atomic::new(1)\na.new()\n", "`atomic` tidak punya method atau field bernama `new`", "`atomic` tidak punya method atau field bernama `new`"),
    ("atomic: method ditulis tanpa kurung", "let a = Atomic::new(1)\nwok(a.get)\n", "`get` itu method milik `atomic`", "`get` itu method milik `atomic`"),
    ("mutex: lock tanpa kurung tidak mengunci", "let m = Mutex::new()\nwok(m.lock)\n", "`lock` itu method milik `mutex`", "`lock` itu method milik `mutex`"),
    ("mutex: method lock berargumen", "let m = Mutex::new()\nm.lock(1)\n", "method `lock` butuh 0 argumen, dapat 1", "method `lock` butuh 0 argumen, dapat 1"),
    ("mutex: method milik Atomic", "let m = Mutex::new()\nm.get()\n", "`mutex` tidak punya method atau field bernama `get`", "`mutex` tidak punya method atau field bernama `get`"),
    ("handle: menulis id sebuah Ptr", "var h = ptr::new(1)\nwok(\"x\")\nh.id = 7\n", "`Ptr` adalah handle buatan pustaka — field `id` tidak bisa ditulis", "`Ptr` adalah handle buatan pustaka — field `id` tidak bisa ditulis"),
    ("handle: menambah field ke sebuah Ptr", "var h = ptr::new(1)\nh.baru = 7\n", "`Ptr` adalah handle buatan pustaka — field `baru`", "`Ptr` adalah handle buatan pustaka — field `baru`"),
    ("handle: penugasan gabungan ke id router", "var h = dok::router_new()\nh.id += 1\n", "`DokRouter` adalah handle buatan pustaka", "`DokRouter` adalah handle buatan pustaka"),
    ("handle: saluran sebuah Future", "var h = Future::new(|| 1)\nh._ch = 3\n", "`Future` adalah handle buatan pustaka", "`Future` adalah handle buatan pustaka"),
    ("handle: Ptr di dalam array", "var h = [ptr::new(1)]\nh[0].id = 4\n", "`Ptr` adalah handle buatan pustaka", "`Ptr` adalah handle buatan pustaka"),
    ("mutex: with_lock dengan bukan fungsi", "let m = Mutex::new()\nmutex::with_lock(m, 5)\n", "bukan fungsi", "bukan fungsi"),
    ("struct: literal yang namanya tidak dideklarasikan",
     "package main\nstruct Titik { pub x: int }\npub fn main() { wok(\"awal\")\n let t = Titk { x: 1 }\n wok(t) }\n",
     "struct `Titk` belum dideklarasikan — maksudnya `Titik`?", "struct `Titk` belum dideklarasikan — maksudnya `Titik`?"),
    ("struct: handle bawaan ditulis sebagai literal",
     "let p = Ptr { id: 1, kind: \"single\" }\nwok(ptr::get(p))\n", "dibuat oleh pustaka", "dibuat oleh pustaka"),
    ("tugas: hasil tidak bisa dicuri lewat saluran dalamnya",
     "let t = Task::spawn(|| 5)\nwok(<-t._ch)\n", "butuh channel, dapat `handle`", "butuh channel, dapat `handle`"),
    ("tugas: saluran dalam tidak terbuka lewat pola",
     "package main\npub fn main() { let t = Task::spawn(|| 5)\n match t { Task { _ch } => wok(<-_ch), _ => wok(0) } }\n",
     "butuh channel, dapat `handle`", "butuh channel, dapat `handle`"),
    // Fase PACU-5: yang dulu menggantung, membungkus, atau menjawab angka karangan.
    ("ptr: fungsi update membaca pointernya sendiri",
     "let p = ptr::new(1)\nwok(\"x\")\nptr::update(p, |x| ptr::get(p) + x)\n", "sedang diperbarui", "sedang diperbarui"),
    ("ptr: fungsi update menulis pointernya sendiri",
     "let p = ptr::new(1)\nptr::update(p, |x| { ptr::set(p, 9)\n return x })\n", "sedang diperbarui", "sedang diperbarui"),
    ("ptr: fungsi update menaikkan pointernya sendiri",
     "let p = ptr::new(1)\nptr::update(p, |x| ptr::inc(p))\n", "sedang diperbarui", "sedang diperbarui"),
    ("ptr: update bersarang atas pointer yang sama",
     "let p = ptr::new(1)\nptr::update(p, |x| ptr::update(p, |y| y + 1))\n", "sedang diperbarui", "sedang diperbarui"),
    ("ptr: inc di batas 128 bit",
     "let p = ptr::new(170141183460469231731687303715884105727)\nptr::inc(p)\n", "kelewat besar", "kelewat besar"),
    ("ptr: dec di batas bawah 128 bit",
     "let p = ptr::new(-170141183460469231731687303715884105727 - 1)\nptr::dec(p)\n", "kelewat besar", "kelewat besar"),
    ("angka: tidak ada prima sesudah 2^127 - 1",
     "wok(dok::num_next_prime(170141183460469231731687303715884105727))\n", "tidak ada bilangan prima", "tidak ada bilangan prima"),
    ("angka: num_from_base di luar 128 bit",
     "wok(dok::num_from_base(\"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz\", 36))\n", "di luar jangkauan integer 128 bit", "di luar jangkauan integer 128 bit"),
    ("angka: num_to_base atas pecahan raksasa",
     "wok(dok::num_to_base(1e300, 10))\n", "di luar jangkauan 128 bit", "di luar jangkauan 128 bit"),
    ("waktu: cap waktu di luar jangkauan",
     "wok(time::year(170141183460469231731687303715884105727))\n", "di luar jangkauan", "di luar jangkauan"),
    ("waktu: time::format di luar jangkauan",
     "wok(time::format(-8640000000000001, \"%Y\"))\n", "di luar jangkauan", "di luar jangkauan"),
    ("json: pecahan di luar 64 bit",
     "wok(json::parse(\"1e400\"))\n", "di luar jangkauan 64 bit", "di luar jangkauan 64 bit"),
    // Fase PACU-4: galat dari jalur cepat VM tetap kalimat jalur biasa.
    ("pacu: tulis index lokal di luar batas",
     "var a = [1, 2]\nlet i = 2\na[i] = 1\n", "di luar batas", "di luar batas"),
    ("pacu: baca index lokal di luar batas",
     "var a = [1, 2]\nlet i = 5\nwok(a[i])\n", "di luar batas", "di luar batas"),
    ("pacu: tulis index lokal bukan wadah",
     "var n = 5\nn[0] = 1\n", "tidak bisa ditulis lewat `[...]`", "tidak bisa ditulis lewat `[...]`"),
    ("pacu: `a[i] = a[i] + x` salah tipe",
     "var a = [[1]]\na[0] = a[0] + 5\n", "array", "array"),
    ("pacu: sisa bagi nol atas lokal",
     "var a = 5\nlet z = 0\na = a % z\n", "sisa bagi dengan nol", "sisa bagi dengan nol"),
    ("pacu: luapan kali di atas 128 bit",
     "var x = 170141183460469231731687303715884105727\nx = x * 2\n", "kelewat besar", "kelewat besar"),
    // Field privat: yang menentukan tipe pemilik method yang LEKSIKAL, bukan variabel `self`
    // — dulu `let self = Base {}` di luar method membuka field privat di interpreter.
    ("field privat: `self` palsu di tingkat atas tidak membuka akses",
     "struct Base { rahasia: int = 7 }\nlet self = Base {}\nlet lain = Base { rahasia: 2 }\nwok(lain.rahasia)\n",
     "bersifat private", "bersifat private"),
    ("field privat: `self` palsu di dalam fungsi biasa tidak membuka akses",
     "struct Base { rahasia: int = 7 }\nfn curi(o) { let self = o\n return o.rahasia }\nwok(curi(Base {}))\n",
     "bersifat private", "bersifat private"),
    // Varian ber-field menolak field yang tidak dideklarasikannya, dengan satu kalimat.
    ("varian ber-field: field liar ditolak",
     "enum E { V { a: int }, W }\nlet v = E::V { a: 1, zz: 2 }\nwok(v)\n",
     "field `zz` tidak ada pada variant E::V", "field `zz` tidak ada pada variant E::V"),
    ("varian ber-field telanjang: field liar ditolak",
     "enum E { V { a: int }, W }\nlet w = V { a: 1, zz: 3 }\nwok(w)\n",
     "field `zz` tidak ada pada variant E::V", "field `zz` tidak ada pada variant E::V"),
    // Method tidak melihat namanya sendiri di scope: `P::f` di dalamnya tetap diperiksa
    // penerimanya, dan nama telanjang default trait tidak dikenal (sama dengan `impl`).
    ("method: `P::f` dari dalam method tetap memeriksa penerimanya",
     "struct P { pub x: int }\nstruct Q { pub x: int }\n\
      impl P { fn f(self, n) { if n == 0 { return Q { x: 9 } } return P::f(Q { x: 1 }, 0) } }\n\
      let p = P { x: 1 }\nwok(p.f(1))\n",
     "argumen pertamanya harus `P`", "argumen pertamanya harus `P`"),
    ("method default trait: nama telanjangnya tidak dikenal, sama dengan impl",
     "trait T { fn hitung(self, n) { if n == 0 { return 0 } return 1 + hitung(self, n - 1) } }\n\
      struct S { pub a: int }\nimpl T for S { }\nlet s = S { a: 1 }\nwok(s.hitung(3))\n",
     "`hitung` belum dikenal", "`hitung` belum dikenal"),
    // `[x; n]` dulu mematikan dua mesin lewat OOM untuk n raksasa, dan `--vm` menjawab `[]`
    // untuk n negatif atau bukan bilangan bulat (`interpreter::jumlah_ulang`).
    ("[x; n]: ukuran raksasa ditolak, bukan mematikan proses",
     "wok(\"mulai\")\nwok(len([0; 99999999999]))\n",
     "minta 99999999999 elemen sekaligus", "minta 99999999999 elemen sekaligus"),
    ("[x; n]: n negatif ditolak",
     "wok([0; -1])\n", "n tidak boleh negatif", "n tidak boleh negatif"),
    ("[x; n]: n pecahan ditolak, bukan jadi array kosong",
     "wok([0; 2.5])\n", "n harus bilangan bulat", "n harus bilangan bulat"),
    ("[x; n]: n bukan angka ditolak",
     "wok([0; \"a\"])\n", "n harus bilangan bulat, dapat `string`", "n harus bilangan bulat, dapat `string`"),
    ("var [N]T raksasa ditolak lewat batas yang sama",
     "package main\npub fn main() { var a: [99999999999]int\n wok(1) }",
     "minta 99999999999 elemen sekaligus", "minta 99999999999 elemen sekaligus"),
    // ── Fase LET-POLA ─────────────────────────────────────────────────────
    ("pad bentuk method: lebar raksasa ditolak, bukan mematikan proses",
     "wok(\"mulai\")\nwok(\"a\".pad_left(99999999999))\n",
     "minta 99999999999 elemen sekaligus", "minta 99999999999 elemen sekaligus"),
    ("center bentuk method: lebar raksasa ditolak",
     "wok(\"a\".center(99999999999))\n",
     "minta 99999999999 elemen sekaligus", "minta 99999999999 elemen sekaligus"),
    ("repeat: batasnya atas TOTAL elemen",
     "wok(len(arr::repeat(range(0, 1000), 16000000)))\n",
     "minta 16000000000 elemen sekaligus", "minta 16000000000 elemen sekaligus"),
    ("repeat bentuk method: satu batas dengan namespace",
     "wok(len([1].repeat(20000000)))\n",
     "arr::repeat: minta 20000000 elemen", "arr::repeat: minta 20000000 elemen"),
    ("pad: isi lebih dari satu karakter ditolak",
     "wok(str::pad_left(\"ab\", 5, \"xy\"))\n",
     "isinya harus tepat satu karakter, dapat 2 karakter",
     "isinya harus tepat satu karakter, dapat 2 karakter"),
    ("pad bentuk method: isi kosong ditolak",
     "wok(\"ab\".pad_right(5, \"\"))\n",
     "isinya harus tepat satu karakter, dapat 0 karakter",
     "isinya harus tepat satu karakter, dapat 0 karakter"),
    ("wokwok::outer: ukuran raksasa ditolak",
     "wok(wokwok::outer(wokwok::ones([100000]), wokwok::ones([100000])))\n",
     "wokwok::outer: minta 10000000000 elemen", "wokwok::outer: minta 10000000000 elemen"),
    ("wokwok::linspace: jumlah raksasa ditolak",
     "wok(wokwok::linspace(0, 1, 99999999999))\n",
     "wokwok::linspace: minta 99999999999 elemen", "wokwok::linspace: minta 99999999999 elemen"),
    ("impl atas tipe bawaan ditolak, bukan diterima lalu tak terpanggil",
     "impl int { fn ganda(self) { return self * 2 } }\nwok(5.ganda())\n",
     "`impl int` tidak berpengaruh", "`impl int` tidak berpengaruh"),
    ("impl atas array ditolak",
     "impl []int { fn jumlah(self) { return 0 } }\nwok(1)\n",
     "tidak berpengaruh — method tidak bisa ditambahkan ke tipe bawaan",
     "tidak berpengaruh — method tidak bisa ditambahkan ke tipe bawaan"),
    ("or-pattern: alternatif yang mengikat nama berbeda ditolak parser",
     "wok(\"mulai\")\nwok(match (2, 5) { (a, 1) | (2, b) => b, _ => 0 })\n",
     "`a` diikat di alternatif ke-1 tapi tidak di alternatif ke-2",
     "`a` diikat di alternatif ke-1 tapi tidak di alternatif ke-2"),
    ("kurung kosong: tulis index ke `{}` menyebut dict::new",
     "var d = {}\nd[\"x\"] = 1\n",
     "dict kosong ditulis `dict::new()`", "dict kosong ditulis `dict::new()`"),
    ("kurung kosong: method atas `{}` menyebut dict::new",
     "var d = {}\nwok(d.len())\n",
     "dict kosong ditulis `dict::new()`", "dict kosong ditulis `dict::new()`"),
    ("kurung kosong: dict:: atas `{}` menyebut dict::new",
     "var d = {}\nwok(dict::keys(d))\n",
     "dict kosong ditulis `dict::new()`", "dict kosong ditulis `dict::new()`"),
    ("kurung kosong: field ke `{}` menyebut dict::new",
     "var d = {}\nd.x = 1\n",
     "dict kosong ditulis `dict::new()`", "dict kosong ditulis `dict::new()`"),
    ("let: literal di dalam tuple diuji, bukan dilewati",
     "wok(\"a\")\nlet (a, 5) = (1, 6)\nwok(a)\n",
     "nilai `let` bertipe `tuple` tidak cocok dengan polanya",
     "nilai `let` bertipe `tuple` tidak cocok dengan polanya"),
    ("let: varian unit telanjang menguji, bukan mengikat",
     "let None = Option::Some(3)\nwok(\"lolos\")\n",
     "nilai `let` bertipe `enum:Option` tidak cocok dengan polanya",
     "nilai `let` bertipe `enum:Option` tidak cocok dengan polanya"),
    ("let: literal angka di sisi kiri",
     "let s = 7\nlet 8 = s\nwok(\"lolos\")\n",
     "nilai `let` bertipe `int` tidak cocok dengan polanya",
     "nilai `let` bertipe `int` tidak cocok dengan polanya"),
    ("let: literal teks di sisi kiri",
     "let \"x\" = \"y\"\nwok(\"lolos\")\n",
     "nilai `let` bertipe `string` tidak cocok dengan polanya",
     "nilai `let` bertipe `string` tidak cocok dengan polanya"),
    ("let: literal di field struct",
     "struct P { pub a: int }\nlet P { a: 3 } = P { a: 4 }\nwok(\"lolos\")\n",
     "tidak cocok dengan polanya",
     "tidak cocok dengan polanya"),
    ("let: panjang array salah",
     "let [a, b] = [1, 2, 3]\nwok(a)\n",
     "nilai `let` bertipe `array` tidak cocok dengan polanya",
     "nilai `let` bertipe `array` tidak cocok dengan polanya"),
    ("parameter: pola tuple dengan literal menyebut argumen, bukan let",
     "fn f((a, 5)) { return a }\nwok(f((1, 6)))\n",
     "argumen bertipe `tuple` tidak cocok dengan polanya",
     "argumen bertipe `tuple` tidak cocok dengan polanya"),
    ("for: pola nilai yang tidak cocok menyebut elemen for",
     "for _, (a, 5) in [(1, 6)] { wok(a) }\n",
     "elemen `for` bertipe `tuple` tidak cocok dengan polanya",
     "elemen `for` bertipe `tuple` tidak cocok dengan polanya"),
    // ── Fase LOGIKA ───────────────────────────────────────────────────────
    ("banding: elemen NaN membatalkan pembandingan agregat",
     "wok([1.0] < [math::NAN])\n",
     "tidak bisa membandingkan `float` dengan `float`",
     "tidak bisa membandingkan `float` dengan `float`"),
    ("kesamaan: fungsi di dalam array yang SAMA tetap ditolak",
     "fn f() { return 1 }\nlet x = [f]\nwok(x == x)\n",
     "tidak bisa membandingkan `function` dengan `function`",
     "tidak bisa membandingkan `function` dengan `function`"),
    ("koalesensi: sisi kiri bukan Option",
     "wok(7 ?? 0)\n",
     "`??` cuma untuk `Option`, dapat `int`",
     "`??` cuma untuk `Option`, dapat `int`"),
    ("banding: > menyebut operand urut tulis",
     "wok(7 > true)\n",
     "tidak bisa membandingkan `int` dengan `bool`",
     "tidak bisa membandingkan `int` dengan `bool`"),
    ("sisa bagi: tipe diperiksa sebelum nol",
     "wok(\"s\" % 0)\n",
     "tidak bisa `%` antara `string` dan `int`",
     "tidak bisa `%` antara `string` dan `int`"),
    ("match: tak ada lengan yang cocok menyebut nilainya",
     "wok(match 7 { 1 => 2 })\n",
     "tidak ada arm yang cocok untuk nilai `7`",
     "tidak ada arm yang cocok untuk nilai `7`"),
    ("pangkat: bilangan bulat tetap meluap",
     "wok(2 ** 4294967296)\n",
     "kelewat besar buat integer",
     "kelewat besar buat integer"),
    // ── Fase TERM ─────────────────────────────────────────────────────────
    ("exit: kode bukan bilangan bulat ditolak, bukan keluar 0",
     "exit(2.5)\n",
     "kode keluar harus bilangan bulat 0 sampai 255, dapat `float`",
     "kode keluar harus bilangan bulat 0 sampai 255, dapat `float`"),
    ("exit: teks bukan kode keluar",
     "exit(\"gagal\")\n",
     "kode keluar harus bilangan bulat 0 sampai 255, dapat `string`",
     "kode keluar harus bilangan bulat 0 sampai 255, dapat `string`"),
    ("exit: kode di luar 0-255 ditolak, bukan dipotong jadi 0",
     "exit(256)\n",
     "kode keluar harus 0 sampai 255, dapat 256", "kode keluar harus 0 sampai 255, dapat 256"),
    ("exit: kode negatif ditolak",
     "std::exit(0 - 1)\n",
     "kode keluar harus 0 sampai 255, dapat -1", "kode keluar harus 0 sampai 255, dapat -1"),
    ("term::mode: nama mode salah ketik dapat saran",
     "term::mode(\"layr\", true)\n",
     "mode `layr` tidak ada — maksudnya `layar`?", "mode `layr` tidak ada — maksudnya `layar`?"),
    ("term::tombol_tunggu: lama tunggu negatif ditolak",
     "term::tombol_tunggu(0 - 1)\n",
     "lama tunggu harus 0 sampai 86400000 milidetik", "lama tunggu harus 0 sampai 86400000 milidetik"),
    ("term::lebar: yang diukur harus teks",
     "wok(term::lebar([1, 2]))\n",
     "term::lebar: argumen ke-1 harus teks, dapat `array`", "term::lebar: argumen ke-1 harus teks, dapat `array`"),
    ("kanal: kapasitas negatif menyebut angkanya",
     "let c = Chan::new(-1)\n",
     "kapasitas tidak boleh negatif, dapat -1", "kapasitas tidak boleh negatif, dapat -1"),
    ("select: terima dari channel tertutup dan kosong ditolak, bukan menggantung/`nil`",
     "let c = Chan::new(1)\nc.close()\nwok(\"mulai\")\nselect {\n    case v := <-c: { wok(\"dapat\", v) }\n}\n",
     "sudah ditutup dan kosong", "sudah ditutup dan kosong"),
    ("select: kirim ke channel tertutup ditolak, bukan menggantung",
     "let c = Chan::new(1)\nc.close()\nselect {\n    case c <- 1: { wok(\"kirim\") }\n}\n",
     "tidak bisa kirim lagi", "tidak bisa kirim lagi"),
    ("select: channel tertutup ditolak walau ada default",
     "let c = Chan::new(1)\nc.close()\nselect {\n    case v := <-c: { wok(\"dapat\", v) }\n    default: { wok(\"def\") }\n}\n",
     "sudah ditutup dan kosong", "sudah ditutup dan kosong"),
    ("select: operand bukan channel ditolak, bukan menggantung",
     "let a = [1]\nselect {\n    case v := <-a: { wok(v) }\n}\n",
     "`case` di `select` butuh channel, dapat `array`", "`case` di `select` butuh channel, dapat `array`"),
    ("select: operand bukan channel ditolak, bukan diam-diam ke default",
     "let a = [1]\nselect {\n    case a <- 1: { wok(1) }\n    default: { wok(2) }\n}\n",
     "`case` di `select` butuh channel, dapat `array`", "`case` di `select` butuh channel, dapat `array`"),
    ("select: operand bukan channel di cabang KEDUA ditolak walau cabang pertama siap",
     "let c = Chan::new(1)\nc <- 1\nlet a = [1]\nselect {\n    case v := <-c: { wok(v) }\n    case w := <-a: { wok(w) }\n}\n",
     "`case` di `select` butuh channel, dapat `array`", "`case` di `select` butuh channel, dapat `array`"),
    ("<-: terima dari bukan-channel menyebut tipe, bukan isinya",
     "let a = range(0, 3000)\nwok(<-a)\n",
     "`<-c` (terima) butuh channel, dapat `array`", "`<-c` (terima) butuh channel, dapat `array`"),
    ("<-: kirim ke bukan-channel menyebut tipe, bukan isinya",
     "let a = [1, 2]\na <- 5\n",
     "`c <- v` (kirim) butuh channel, dapat `array`", "`c <- v` (kirim) butuh channel, dapat `array`"),
    ("chan: recv atas channel tertutup dan kosong ditolak, bukan `nil`",
     "let c = Chan::new(1)\nc.close()\nwok(\"mulai\")\nwok(c.recv())\n",
     "sudah ditutup dan kosong", "sudah ditutup dan kosong"),
    ("chan: argumen pertama bukan channel",
     "wok(chan::len([1, 2]))\n",
     "chan::len: argumen ke-1 harus channel, dapat `array`", "chan::len: argumen ke-1 harus channel, dapat `array`"),
    // ── Pintu konversi (Fase KONVERSI) ────────────────────────────────────
    ("konversi: int(NaN) bukan 0",
     "package main\npub fn main() { wok(int(math::NAN)) }",
     "NaN memang bukan angka", "NaN memang bukan angka"),
    ("konversi: uint(tak hingga) bukan batas atas",
     "package main\npub fn main() { wok(uint(math::INF)) }",
     "tak hingga tidak muat", "tak hingga tidak muat"),
    ("konversi: parse_int(NaN) bukan 0",
     "package main\npub fn main() { wok(parse_int(math::NAN)) }",
     "butuh teks, dapat `float`", "butuh teks, dapat `float`"),
    ("konversi: str::to_int tidak memotong pecahan diam-diam",
     "package main\npub fn main() { wok(str::to_int(5.9)) }",
     "butuh teks, dapat `float`", "butuh teks, dapat `float`"),
    ("konversi: pintu parse menyebut nama kanoniknya",
     "package main\npub fn main() { wok(\"abc\".to_int()) }",
     "str::to_int: teks `abc` bukan bilangan bulat", "str::to_int: teks `abc` bukan bilangan bulat"),
    ("konversi: kode rune negatif bukan karakter NUL",
     "package main\npub fn main() { wok(rune(-1)) }",
     "bukan kode karakter Unicode yang sah", "bukan kode karakter Unicode yang sah"),
    ("konversi: surrogate bukan karakter",
     "package main\npub fn main() { wok(55296 as rune) }",
     "bukan kode karakter Unicode yang sah", "bukan kode karakter Unicode yang sah"),
    ("konversi: kode di atas 2^32 tidak dipotong jadi karakter lain",
     "package main\npub fn main() { wok(4294967361 as rune) }",
     "bukan kode karakter Unicode yang sah", "bukan kode karakter Unicode yang sah"),
    ("konversi: 300 tidak muat di u8",
     "package main\npub fn main() { wok(300 as u8) }",
     "tidak muat di `u8`", "tidak muat di `u8`"),
    ("konversi: 256 tidak muat di byte",
     "package main\npub fn main() { wok(256 as byte) }",
     "tidak muat di `byte`", "tidak muat di `byte`"),
    ("konversi: minus unary tidak melewati pemeriksaan tanda",
     "package main\npub fn main() { var x = 5\n wok(-x as uint) }",
     "angka negatif tidak punya padanan", "angka negatif tidak punya padanan"),
    ("konversi: pecahan di luar 128 bit tidak dijepit ke batas",
     "package main\npub fn main() { wok(170141183460469231731687303715884105728.0 as int) }",
     "di luar jangkauan integer 128 bit", "di luar jangkauan integer 128 bit"),
    ("konversi: `as unit` ditolak, bukan diterima diam-diam",
     "package main\npub fn main() { wok(5 as unit) }",
     "cast ke `unit` tidak didukung", "cast ke `unit` tidak didukung"),
    ("konversi: sasaran struct ditolak dengan kalimat yang sama",
     "package main\nstruct T { pub x: int }\npub fn main() { wok(5 as T) }",
     "cast ke `T` tidak didukung", "cast ke `T` tidak didukung"),
    ("konversi: alamat pointer tidak negatif",
     "package main\npub fn main() { wok((-1) as *const int) }",
     "tidak muat di `pointer`", "tidak muat di `pointer`"),
    // ── Field yang tidak dideklarasikan (Fase FIELD) ──────────────────────
    (
        "field salah ketik tidak menambah field baru",
        "package main\nstruct P { pub a: int }\n\
         pub fn main() { var p = P { a: 1 }  p.A = 9  wok(p.a) }",
        "tidak punya field `A`", "tidak punya field `A`",
    ),
    (
        "field yang tidak dideklarasikan ditolak di jalur tulis",
        "package main\nstruct P { pub a: int }\n\
         pub fn main() { var p = P { a: 1 }  p.zz = 5  wok(p) }",
        "tidak punya field `zz`", "tidak punya field `zz`",
    ),
    // Literalnya DULU asimetri: interpreter menerima, VM menolak saat kompilasi.
    (
        "field yang tidak dideklarasikan ditolak di literal struct",
        "package main\nstruct P { pub a: int }\n\
         pub fn main() { let p = P { a: 1, zz: 5 }  wok(p) }",
        "tidak punya field `zz`", "tidak punya field `zz`",
    ),
    (
        "field yang tidak dideklarasikan ditolak lewat warisan",
        "package main\nstruct P { pub a: int }\nstruct Q: P { pub b: int }\n\
         pub fn main() { var q = Q { a: 1, b: 2 }  q.zz = 9  wok(q) }",
        "tidak punya field `zz`", "tidak punya field `zz`",
    ),
    // ── `str::` berhenti men-Display argumen POLA-nya (Fase POLA-STR) ──────
    (
        "str::contains: pola bukan-string ditolak",
        "package main\npub fn main() { wok(str::contains(\"a[1, 2]b\", [1, 2])) }",
        "yang dicari harus string", "yang dicari harus string",
    ),
    (
        "str::split: pemisah bukan-string ditolak",
        "package main\npub fn main() { wok(str::split(\"a1b\", [1, 2])) }",
        "pemisahnya harus string", "pemisahnya harus string",
    ),
    (
        "str::replace: yang diganti harus string",
        "package main\npub fn main() { wok(str::replace(\"x1y\", 1, \"Z\")) }",
        "yang diganti harus string", "yang diganti harus string",
    ),
    (
        "str::index_of: pola bukan-string ditolak",
        "package main\npub fn main() { wok(str::index_of(\"q5w\", 5)) }",
        "yang dicari harus string", "yang dicari harus string",
    ),
    (
        "str::strip_prefix: awalan bukan-string ditolak",
        "package main\npub fn main() { wok(str::strip_prefix(\"[1]x\", [1])) }",
        "awalannya harus string", "awalannya harus string",
    ),
    // Nama induk warisan yang tidak ada.
    (
        "induk warisan yang namanya tidak ada",
        "package main\nstruct Hewan { pub nama: string }\n\
         struct Anjing: Hewann { pub ras: string }\n\
         pub fn main() { wok(Anjing { nama: \"Bo\", ras: \"b\" }.ras) }",
        "mewarisi `Hewann`", "mewarisi `Hewann`",
    ),
    (
        "proses::kill menolak pid nol",
        "package main\npub fn main() { wok(proses::kill(0)) }",
        "pid harus positif", "pid harus positif",
    ),
    (
        "proses::kill menolak pid negatif",
        "package main\npub fn main() { wok(proses::kill(-1)) }",
        "pid harus positif", "pid harus positif",
    ),
    // Kalau penjaganya hilang, angka ini terpotong ke pid yang TIDAK MUNGKIN ada.
    // Jangan ganti dengan i64 terbesar: potongannya `kill -1`, semua proses.
    (
        "proses::kill menolak pid di atas jangkauan 32 bit",
        "package main\npub fn main() { wok(proses::kill(4299161601)) }",
        "di luar jangkauan", "di luar jangkauan",
    ),
    (
        "proses::kill menolak pid tepat sesudah i32 terbesar",
        "package main\npub fn main() { wok(proses::kill(2147483648)) }",
        "di luar jangkauan", "di luar jangkauan",
    ),
    // `as usize` dulu memotong 2^64+5 jadi 5, jadi yang ini menjawab 10 karakter.
    (
        "crypto::random_hex menolak ukuran raksasa, bukan memotongnya",
        "package main\npub fn main() { wok(crypto::random_hex(18446744073709551621)) }",
        "elemen sekaligus", "elemen sekaligus",
    ),
    // Method milik satu tipe TIDAK boleh jalan atas nilai tipe lain. Sebelum
    // ini `--vm` menjawab 13 — hasil salah yang diam, dan tidak satu pun tabel
    // bisa melihatnya karena interpreter menolak seluruh bentuknya.
    (
        "path method: penerima tipe lain",
        "package main\nstruct P { pub n: int }\nstruct Q { pub n: int }\nimpl P { pub fn tambah(self, k) -> int { return self.n + k } }\npub fn main() { wok(P::tambah(Q{n:9}, 4)) }",
        "method milik", "method milik",
    ),
    (
        "path method: penerima bukan struct",
        "package main\nstruct P { pub n: int }\nimpl P { pub fn tambah(self, k) -> int { return self.n + k } }\npub fn main() { wok(P::tambah(5, 4)) }",
        "method milik", "method milik",
    ),
    (
        "path method: induk tidak menerima anak-nya sendiri terbalik",
        "package main\nstruct Hewan { pub n: int }\nstruct Anjing: Hewan { }\nimpl Anjing { pub fn gonggong(self) -> string { return \"guk\" } }\npub fn main() { wok(Anjing::gonggong(Hewan{n:1})) }",
        "method milik", "method milik",
    ),
    // ── Bentuk yang ditolak sambil MENYEBUT padanannya ──────────────────
    (
        "`a[..]` menyebut `:`",
        "package main\npub fn main() { let a = [1,2,3]\n  wok(a[..]) }",
        "memakai `:`", "memakai `:`",
    ),
    (
        "`for ... else` menyebut padanannya",
        "package main\npub fn main() { for i in [1] { wok(i) } else { wok(0) } }",
        "tidak menempel", "tidak menempel",
    ),
    (
        "`const fn` menyebut sebabnya",
        "package main\nconst fn f() -> int { return 1 }\npub fn main() { wok(f()) }",
        "const fn", "const fn",
    ),
    (
        "`static` menyebut padanannya",
        "package main\nstatic G = 5\npub fn main() { wok(G) }",
        "bukan bentuk JRG", "bukan bentuk JRG",
    ),
    (
        "sebar array: yang disebar harus array",
        "package main\npub fn main() { wok([...5]) }",
        "cuma bisa menyebar array — dapat `int`", "cuma bisa menyebar array — dapat `int`",
    ),
    // Urutan agregat TIDAK berarti "apa pun bisa diurutkan": elemen yang tidak
    // sebanding tetap membuat seluruh pembandingan gagal, bukan "dianggap sama".
    ("`??=` ditolak sambil menyebut kedua bentuk yang benar",
     "package main\npub fn main() { var a = Option::None; a ??= 5; wok(a) }",
     "`??=` tidak ada di JRG", "`??=` tidak ada di JRG"),
    ("`??=` menyebut bahwa `??` membuka Option-nya",
     "package main\npub fn main() { var a = Option::Some(1); a ??= 5; wok(a) }",
     "a = a ?? b", "a = a ?? b"),
    ("elemen tak sebanding membatalkan urutan array",
     "package main\npub fn main() { wok([1] < [\"a\"]) }",
     "`int` dengan `string`", "`int` dengan `string`"),
    ("elemen tak sebanding membatalkan urutan tuple",
     "package main\npub fn main() { wok((1,2) < (1,\"a\")) }",
     "`int` dengan `string`", "`int` dengan `string`"),
    // ── Array kosong tidak punya "yang terkecil"/"yang terbesar" ──────────
    ("min_by atas array kosong ditolak",
     "package main\npub fn main() { wok([].min_by(|x| x)) }",
     "array kosong", "array kosong"),
    ("max_by atas array kosong ditolak",
     "package main\npub fn main() { wok([].max_by(|x| x)) }",
     "array kosong", "array kosong"),
    ("arr::max_by ns atas array kosong ditolak",
     "package main\npub fn main() { wok(arr::max_by([], |x| x)) }",
     "array kosong", "array kosong"),
    // HOF namespace yang kekurangan argumen: satu kalimat, dua backend — sejak Fase
    // KURANG kalimat pintu native, dengan tanda tangan `--doc` di baris bantuan.
    ("arr::map kurang argumen",
     "package main\npub fn main() { wok(arr::map([1])) }",
     "`arr::map` butuh 2 argumen, dapat 1", "`arr::map` butuh 2 argumen, dapat 1"),
    ("arr::reduce kurang argumen",
     "package main\npub fn main() { wok(arr::reduce([1,2])) }",
     "`arr::reduce` butuh 3 argumen, dapat 1", "`arr::reduce` butuh 3 argumen, dapat 1"),
    // ── Fase SAPU-METODE: bentuk method tidak boleh mengarang argumen wajib ──
    (
        "method: chunks tanpa ukuran",
        "package main\npub fn main() { wok([1,2,3].chunks()) }",
        "chunks", "chunks",
    ),
    (
        "method: windows tanpa lebar",
        "package main\npub fn main() { wok([1,2,3].windows()) }",
        "windows", "windows",
    ),
    (
        "method: index_of tanpa yang dicari",
        "package main\npub fn main() { wok([1,2,3].index_of()) }",
        "index_of", "index_of",
    ),
    (
        "method: join tanpa pemisah",
        "package main\npub fn main() { wok([1,2,3].join()) }",
        "join", "join",
    ),
    (
        "method: take tanpa jumlah",
        "package main\npub fn main() { wok([1,2,3].take()) }",
        "take", "take",
    ),
    (
        "method: skip tanpa jumlah",
        "package main\npub fn main() { wok([1,2,3].skip()) }",
        "skip", "skip",
    ),
    (
        "method: repeat tanpa jumlah",
        "package main\npub fn main() { wok([1,2,3].repeat()) }",
        "repeat", "repeat",
    ),
    (
        "method: get tanpa indeks",
        "package main\npub fn main() { wok([1,2,3].get()) }",
        "get", "get",
    ),
    (
        "method: get_or tanpa indeks dan cadangan",
        "package main\npub fn main() { wok([1,2,3].get_or()) }",
        "get_or", "get_or",
    ),
    // Enam nama yang bentuk method-nya dulu HILANG di interpreter dan hidup di
    // VM.
    (
        "method: slice dengan batas terbalik",
        "package main\npub fn main() { wok(\"halo\".slice(3, 1)) }",
        "terbalik", "terbalik",
    ),
    // ── `str::format`: spec yang tidak dikenal ───────────────────────────
    (
        "format: spec lebar belum ada",
        "package main\npub fn main() { wok(str::format(\"{:>8}\", 1)) }",
        "tidak dikenal",
        "tidak dikenal",
    ),
    (
        "format: titik tanpa angka",
        "package main\npub fn main() { wok(str::format(\"{:.}\", 1)) }",
        "tidak dikenal",
        "tidak dikenal",
    ),
    (
        "format: desimal kebanyakan",
        "package main\npub fn main() { wok(str::format(\"{:.99}\", 1.5)) }",
        "maksimum",
        "maksimum",
    ),
    (
        "format: spec di nilai bukan angka",
        "package main\npub fn main() { wok(str::format(\"{:.2}\", \"teks\")) }",
        "cuma untuk angka",
        "cuma untuk angka",
    ),
    (
        "format: spec ikut dihitung saat argumen kurang",
        "package main\npub fn main() { wok(str::format(\"{} {:.2}\", 1)) }",
        "argumen",
        "argumen",
    ),

    // ── Nama tingkat-atas yang ganda ─────────────────────────────────────
    (
        "nama ganda: fn dua kali",
        "package main\nfn f() { return 1 }\nfn f() { return 2 }\npub fn main() { wok(f()) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),
    (
        "nama ganda: const dua kali",
        "package main\nconst K = 1\nconst K = 2\npub fn main() { wok(K) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),
    (
        "nama ganda: let lalu const",
        "package main\nlet K = 1\nconst K = 2\npub fn main() { wok(K) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),
    (
        "nama ganda: fn lalu const (satu ruang nama)",
        "package main\nfn n() { return 1 }\nconst n = 2\npub fn main() { wok(n) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),
    (
        "nama ganda: struct dua kali",
        "package main\nstruct P { pub x: int }\nstruct P { pub y: int }\npub fn main() { wok(1) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),
    (
        "nama ganda: enum lalu struct (satu ruang nama)",
        "package main\nenum E { A }\nstruct E { pub x: int }\npub fn main() { wok(1) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),
    (
        "nama ganda: var dua kali",
        "package main\nvar G = 1\nvar G = 2\npub fn main() { wok(G) }",
        "sudah dideklarasikan",
        "sudah dideklarasikan",
    ),

    // ── Builtin kebanyakan argumen ───────────────────────────────────────
    (
        "arity: str::upper kebanyakan",
        "package main\npub fn main() { wok(str::upper(\"a\", 9)) }",
        "kebanyakan argumen",
        "kebanyakan argumen",
    ),
    (
        "arity: str::replace kebanyakan",
        "package main\npub fn main() { wok(str::replace(\"a\", \"b\", \"c\", 9)) }",
        "kebanyakan argumen",
        "kebanyakan argumen",
    ),
    (
        "arity: jawa::set kebanyakan",
        "package main\npub fn main() { let p = jawa::alloc(8)\n jawa::set(p, 1, 99) }",
        "kebanyakan argumen",
        "kebanyakan argumen",
    ),
    // `"-5" as uint` menjawab `-5` di VM: sebuah tak-bertanda yang NEGATIF.
    (
        "cast: string negatif ke uint",
        "package main\npub fn main() { wok(\"-5\" as uint) }",
        "angka negatif tidak punya padanan",
        "angka negatif tidak punya padanan",
    ),
    (
        "cast: string bukan angka ke float",
        "package main\npub fn main() { wok(\"abc\" as float) }",
        "teks `abc` bukan angka",
        "teks `abc` bukan angka",
    ),
    // Nama modul `std/` yang tidak ada.
    (
        "std: modul yang tidak ada",
        "package main\nimport \"std/hantu\"\npub fn main() { wok(1) }",
        "modul standar 'std/hantu' tidak ada",
        "modul standar 'std/hantu' tidak ada",
    ),
    // ── Operator pointer semu ───────────────────────────────────────────
    (
        "pointer semu: &x",
        "package main\npub fn main() { var x = 42\n let p = &x\n wok(p) }",
        "sebagai operator pointer tidak ada di JRG",
        "sebagai operator pointer tidak ada di JRG",
    ),
    (
        "pointer semu: &mut x",
        "package main\npub fn main() { var x = 42\n let p = &mut x\n wok(p) }",
        "sebagai operator pointer tidak ada di JRG",
        "sebagai operator pointer tidak ada di JRG",
    ),
    (
        "pointer semu: *p",
        "package main\npub fn main() { var x = 42\n wok(*x) }",
        "sebagai operator pointer tidak ada di JRG",
        "sebagai operator pointer tidak ada di JRG",
    ),
    // `let` tingkat modul mewarisi SEMUA aturan `let` biasa: tidak bisa ditulis
    // ulang, tidak bisa ditulis MELALUI (nilai tidak pernah beralias, jadi `A` …
    (
        "modul: let tidak bisa ditulis ulang",
        "package main\nlet K = 5\npub fn main() { K = 6\n wok(K) }",
        "`K` dideklarasikan dengan `let`",
        "`K` dideklarasikan dengan `let`",
    ),
    (
        "modul: tulis MELALUI let",
        "package main\nlet A = [1,2]\npub fn main() { A[0] = 9\n wok(A) }",
        "`A` dideklarasikan dengan `let`",
        "`A` dideklarasikan dengan `let`",
    ),
    (
        "modul: let dengan pola ditolak beralasan",
        "package main\nlet [a, b] = [1, 2]\npub fn main() { wok(a) }",
        "cuma menerima satu nama, bukan pola",
        "cuma menerima satu nama, bukan pola",
    ),
    (
        "modul: let dengan tipe tak dikenal",
        "package main\nlet K: Hantu = 5\npub fn main() { wok(K) }",
        "tipe `Hantu` tidak ada",
        "tipe `Hantu` tidak ada",
    ),
    // ── str::format dan `self` bukan-method ─────────────────────────────
    (
        "format: placeholder kurang argumen",
        "package main\npub fn main() { wok(str::format(\"{} {} {}\", 1)) }",
        "punya 3 placeholder",
        "punya 3 placeholder",
    ),
    (
        "format: argumen berlebih dibuang diam-diam",
        "package main\npub fn main() { wok(str::format(\"{}\", 1, 2, 3)) }",
        "argumen terakhir tidak terpakai",
        "argumen terakhir tidak terpakai",
    ),
    (
        "format: placeholder berposisi tidak didukung",
        "package main\npub fn main() { wok(str::format(\"{0} {1}\", \"a\", \"b\")) }",
        "`{0}` tidak didukung",
        "`{0}` tidak didukung",
    ),
    (
        "format: tanpa argumen sama sekali",
        "package main\npub fn main() { wok(str::format()) }",
        "`str::format` butuh paling sedikit 1 argumen, dapat 0",
        "`str::format` butuh paling sedikit 1 argumen, dapat 0",
    ),
    (
        "self di fungsi tingkat-atas",
        "package main\nstruct P { pub x: int }\nfn to_string(self) -> string { return \"khusus\" }\npub fn main() { wok(P{x:1}) }",
        "`self` cuma berarti di dalam `impl`",
        "`self` cuma berarti di dalam `impl`",
    ),
    (
        "self di fungsi bersarang",
        "package main\npub fn main() { fn g(self) -> int { return 1 }\n wok(2) }",
        "`self` cuma berarti di dalam `impl`",
        "`self` cuma berarti di dalam `impl`",
    ),
    // ── Penyerahan kedua: GAGAL KERAS, bukan menggantung ────────────────
    (
        "tugas: await kedua gagal keras, bukan menggantung",
        "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::await(a))\n wok(future::await(a)) }",
        "hasilnya sudah diambil sebelumnya",
        "hasilnya sudah diambil sebelumnya",
    ),
    (
        "tugas: join kedua",
        "package main\npub fn main() { let t = Task::spawn(|| 7)\n wok(task::join(t))\n wok(task::join(t)) }",
        "task::join: hasilnya sudah diambil",
        "task::join: hasilnya sudah diambil",
    ),
    (
        "tugas: await lewat SALINAN future yang sama",
        "package main\npub fn main() { let a = Future::new(|| 7)\n let g = a\n wok(future::await(a))\n wok(future::await(g)) }",
        "hasilnya sudah diambil sebelumnya",
        "hasilnya sudah diambil sebelumnya",
    ),
    (
        "tugas: try_get sesudah diserahkan",
        "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::await(a))\n wok(future::try_get(a)) }",
        "future::try_get: hasilnya sudah diambil",
        "future::try_get: hasilnya sudah diambil",
    ),
    (
        "tugas: timeout sesudah diserahkan",
        "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::await(a))\n wok(future::timeout(a, 200)) }",
        "future::timeout: hasilnya sudah diambil",
        "future::timeout: hasilnya sudah diambil",
    ),
    (
        "tugas: all sesudah diserahkan",
        "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::await(a))\n wok(future::all([a])) }",
        "future::all: elemen ke-0: hasilnya sudah diambil",
        "future::all: elemen ke-0: hasilnya sudah diambil",
    ),
    (
        "tugas: race sesudah diserahkan",
        "package main\npub fn main() { let a = Future::new(|| 7)\n wok(future::await(a))\n wok(future::race([a])) }",
        "future::race: elemen ke-0: hasilnya sudah diambil",
        "future::race: elemen ke-0: hasilnya sudah diambil",
    ),
    // ── future::/task::: bentuk salah dan kegagalan ─────────────────────
    (
        "future: kegagalan tugas jadi galat, bukan string",
        "package main\npub fn main() { let a = Future::new(|| arr::get([1], 99).unwrap())\n wok(future::await(a) + 1) }",
        "Future gagal: unwrap() pada Option::None",
        "Future gagal: unwrap() pada Option::None",
    ),
    (
        "future: task gagal lewat join",
        "package main\npub fn main() { let t = Task::spawn(|| arr::get([1], 99).unwrap())\n wok(task::join(t)) }",
        "Task gagal: unwrap() pada Option::None",
        "Task gagal: unwrap() pada Option::None",
    ),
    (
        "future: kegagalan lewat all",
        "package main\npub fn main() { let a = Future::new(|| arr::get([1], 99).unwrap())\n wok(future::all([a])) }",
        "Future gagal:",
        "Future gagal:",
    ),
    (
        "future: race atas array kosong",
        "package main\npub fn main() { wok(future::race([])) }",
        "arraynya kosong",
        "arraynya kosong",
    ),
    (
        "future: all dengan elemen bukan Future",
        "package main\npub fn main() { wok(future::all([1, 2])) }",
        "elemen ke-0: harus `Future`/`Task`, dapat `int`",
        "elemen ke-0: harus `Future`/`Task`, dapat `int`",
    ),
    (
        "future: all bukan array",
        "package main\npub fn main() { wok(future::all(5)) }",
        "argumen 1 harus array berisi Future/Task",
        "argumen 1 harus array berisi Future/Task",
    ),
    (
        "future: timeout ms negatif",
        "package main\npub fn main() { let a = Future::new(|| 1)\n wok(future::timeout(a, -5)) }",
        "argumen 2 harus int >= 0",
        "argumen 2 harus int >= 0",
    ),
    (
        "future: try_get bukan Future",
        "package main\npub fn main() { wok(future::try_get(5)) }",
        "harus `Future`/`Task`, dapat `int`",
        "harus `Future`/`Task`, dapat `int`",
    ),
    // ── Pesan galat index: SATU kalimat, dibaca kedua backend ───────────
    (
        "index: array dengan string",
        "package main\npub fn main() { let a = [1,2]\n wok(a[\"q\"]) }",
        "tidak bisa index `array` dengan `string`",
        "tidak bisa index `array` dengan `string`",
    ),
    (
        "index: array dengan float",
        "package main\npub fn main() { let a = [1,2]\n wok(a[1.5]) }",
        "tidak bisa index `array` dengan `float`",
        "tidak bisa index `array` dengan `float`",
    ),
    (
        "index: string dengan string",
        "package main\npub fn main() { let s = \"abc\"\n wok(s[\"q\"]) }",
        "tidak bisa index `string` dengan `string`",
        "tidak bisa index `string` dengan `string`",
    ),
    (
        "index: tuple dengan string",
        "package main\npub fn main() { let t = (1,2)\n wok(t[\"q\"]) }",
        "tidak bisa index `tuple` dengan `string`",
        "tidak bisa index `tuple` dengan `string`",
    ),
    (
        "index: int bukan wadah",
        "package main\npub fn main() { let n = 5\n wok(n[0]) }",
        "tidak bisa index `int` — yang bisa di-index",
        "tidak bisa index `int` — yang bisa di-index",
    ),
    (
        "index: nil bukan wadah",
        "package main\npub fn main() { let z = nil\n wok(z[0]) }",
        "tidak bisa index `nil` — yang bisa di-index",
        "tidak bisa index `nil` — yang bisa di-index",
    ),
    (
        "index: range dikira potongan",
        "package main\npub fn main() { let a = [1,2,3]\n wok(a[0..2]) }",
        "sintaksnya titik dua",
        "sintaksnya titik dua",
    ),
    (
        "index: key dict tidak ada",
        "package main\npub fn main() { let d = {\"a\": 1, \"z\": 2}\n wok(d[\"b\"]) }",
        "key `b` tidak ada di dict ini — key yang ada: a, z",
        "key `b` tidak ada di dict ini — key yang ada: a, z",
    ),
    (
        "index: dict kosong",
        "package main\npub fn main() { let d = dict::new()\n wok(d[\"a\"]) }",
        "belum ada key sama sekali",
        "belum ada key sama sekali",
    ),
    (
        "index: field struct tidak ada",
        "package main\nstruct P { x: int, y: int }\npub fn main() { let p = P{x:1,y:2}\n wok(p[\"z\"]) }",
        "field `z` tidak ditemukan di struct `P` — field yang ada: x, y",
        "field `z` tidak ditemukan di struct `P` — field yang ada: x, y",
    ),
    (
        "index: struct dengan int",
        "package main\nstruct P { x: int, y: int }\npub fn main() { let p = P{x:1,y:2}\n wok(p[0]) }",
        "field `0` tidak ditemukan di struct `P`",
        "field `0` tidak ditemukan di struct `P`",
    ),
    (
        "index: oob baca array",
        "package main\npub fn main() { let a = [1,2]\n wok(a[9]) }",
        "array ini isinya 2 elemen",
        "array ini isinya 2 elemen",
    ),
    (
        "index: oob baca string sebut karakter",
        "package main\npub fn main() { let s = \"ab\"\n wok(s[9]) }",
        "string ini isinya 2 karakter",
        "string ini isinya 2 karakter",
    ),
    (
        "index: oob array kosong",
        "package main\npub fn main() { let a = []\n wok(a[0]) }",
        "array ini kosong (0 elemen), jadi belum ada index yang valid",
        "array ini kosong (0 elemen), jadi belum ada index yang valid",
    ),
    (
        "index: oob tulis array",
        "package main\npub fn main() { var a = [1,2]\n a[9] = 5 }",
        "array ini isinya 2 elemen",
        "array ini isinya 2 elemen",
    ),
    (
        "index: tulis lewat key yang belum ada",
        "package main\npub fn main() { var d = {\"a\": 1}\n d[\"b\"][\"c\"] = 1 }",
        "key `b` belum ada — bikin dulu isinya",
        "key `b` belum ada — bikin dulu isinya",
    ),
    // `task::cancel` DIUKUR: task-nya tetap jalan sampai selesai dan tetap
    // menulis efek sampingnya (Atomic terisi 7 sesudah cancel).
    (
        "task::cancel tidak pernah membatalkan",
        "package main\npub fn main() { let t = Task::spawn(|| 1)\n wok(task::cancel(t)) }",
        "tidak pernah benar-benar membatalkan",
        "tidak pernah benar-benar membatalkan",
    ),
    (
        "task::id tidak menyimpan nomor berarti",
        "package main\npub fn main() { let t = Task::spawn(|| 1)\n wok(task::id(t)) }",
        "tidak menyimpan nomor yang berarti",
        "tidak menyimpan nomor yang berarti",
    ),
    // Bentuk salah `num_solve` ditolak dengan kalimat yang SAMA di kedua
    // backend.
    (
        "num_solve dengan argumen bukan fungsi",
        "package main\npub fn main() { wok(dok::num_solve(5, 1.0, 2.0)) }",
        "argumen 1 harus function/closure",
        "argumen 1 harus function/closure",
    ),

    // ── kata kunci yang diurai tapi tidak pernah berpengaruh ─────────────
    (
        "async fn tidak berpengaruh",
        "package main\nasync fn f() -> int { return 1 }\npub fn main() { wok(1) }",
        "`async` tidak ada di JRG, dan ini keputusan",
        "`async` tidak ada di JRG, dan ini keputusan",
    ),
    (
        "await tidak berpengaruh",
        "package main\npub fn main() { wok(await 5) }",
        "`await` tidak ada di JRG, dan ini keputusan",
        "`await` tidak ada di JRG, dan ini keputusan",
    ),

    // ── mutabilitas ──────────────────────────────────────────────────────
    (
        "const ditugasi ulang",
        "package main\nconst K = 1\npub fn main() { K = 2\n wok(K) }",
        "tidak bisa ditugasi ulang",
        "tidak bisa ditugasi ulang",
    ),
    (
        "let ditugasi ulang",
        "package main\npub fn main() { let x = 1\n x = 2\n wok(x) }",
        "tidak bisa ditugasi ulang",
        "tidak bisa ditugasi ulang",
    ),
    // Menulis LEWAT jalur juga ditolak.
    (
        "let ditulis lewat indeks",
        "package main\npub fn main() { let a = [1,2]\n a[0] = 9\n wok(a) }",
        "tidak bisa ditugasi ulang",
        "tidak bisa ditugasi ulang",
    ),
    (
        "let ditulis lewat field",
        "package main\nstruct P { pub x: int }\npub fn main() { let p = P{x:1}\n p.x = 9\n wok(p.x) }",
        "tidak bisa ditugasi ulang",
        "tidak bisa ditugasi ulang",
    ),
    (
        "let ditugasi lewat +=",
        "package main\npub fn main() { let x = 1\n x += 2\n wok(x) }",
        "tidak bisa ditugasi ulang",
        "tidak bisa ditugasi ulang",
    ),
    // ── bentuk metode operator ───────────────────────────────────────────
    (
        "operator add dengan arity salah",
        "package main\nstruct V { pub x: int }\nimpl V { fn add(self, a, b) -> V { return self } }\npub fn main() { wok((V{x:1} + V{x:2}).x) }",
        "harus menerima `self` dan tepat satu operand",
        "harus menerima `self` dan tepat satu operand",
    ),
    (
        "operator eq mengembalikan non-bool",
        "package main\nstruct V { pub x: int }\nimpl V { fn eq(self, o) -> int { return 1 } }\npub fn main() { wok(V{x:1} == V{x:1}) }",
        "harus mengembalikan `bool`",
        "harus mengembalikan `bool`",
    ),
    (
        "operator cmp mengembalikan non-int",
        "package main\nstruct V { pub x: int }\nimpl V { fn cmp(self, o) -> bool { return true } }\npub fn main() { wok(V{x:1} < V{x:2}) }",
        "harus mengembalikan `int`",
        "harus mengembalikan `int`",
    ),

    // Hook yang BENTUKNYA salah ditolak, tidak diabaikan.
    (
        "hook to_string mengembalikan non-string",
        "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return 7 } }\npub fn main() { wok(P{n:1}) }",
        "harus mengembalikan string",
        "harus mengembalikan string",
    ),
    (
        "hook to_string minta parameter tambahan",
        "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self, x) -> string { return \"a\" } }\npub fn main() { wok(P{n:1}) }",
        "harus menerima `self` saja",
        "harus menerima `self` saja",
    ),
    (
        "hook to_string menampilkan dirinya sendiri",
        "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return fmt::sprint(self) } }\npub fn main() { wok(P{n:1}) }",
        "tidak pernah selesai",
        "tidak pernah selesai",
    ),
    // Rekursi biasa.
    (
        "rekursi tak berujung",
        "package main\nfn f(n) { return f(n+1) }\npub fn main() { wok(f(0)) }",
        "rekursi terlalu dalam",
        "rekursi terlalu dalam",
    ),
    // Lewat closure: dulu meluapkan stack interpreter (proses mati), `--vm` menolak.
    (
        "rekursi tak berujung lewat closure",
        "package main\npub fn main() { var c = |n| 0\n c = |n| c(n + 1) + 1\n wok(c(0)) }",
        "rekursi terlalu dalam di fungsi `<closure>`",
        "rekursi terlalu dalam di fungsi `<closure>`",
    ),

    // Blok `else` yang JATUH TERUS akan membuat nama yang diikat polanya
    // terpakai walau polanya tidak cocok — seluruh guna `let … else` hilang.
    (
        "let else yang tidak menyimpang",
        "package main\nfn f(o: Option<int>) -> int {\n  let Some(x) = o else { wok(\"oops\") }\n  return x\n}\npub fn main() { wok(f(Some(1))) }",
        "harus MENYIMPANG",
        "harus MENYIMPANG",
    ),
    (
        "parameter pengumpul dengan nilai default",
        "package main\nfn f(sisa... = 1) { wok(sisa) }\npub fn main() { f() }",
        "tidak boleh punya nilai default",
        "tidak boleh punya nilai default",
    ),

    // Nama tipe di ANOTASI sekarang diperiksa keberadaannya — perlakuan yang
    // sama persis dengan trait bound di fase NADA, dengan alasan yang sama: …
    (
        "nama tipe yang tidak ada di anotasi",
        "package main\npub fn main() { let x: Hantu = 5; wok(x) }",
        "tipe `Hantu` tidak ada",
        "tipe `Hantu` tidak ada",
    ),
    (
        "nama tipe salah ketik disarankan",
        "package main\nstruct Titik { pub x: int }\nfn f(p: Titk) -> int { return p.x }\npub fn main() { wok(f(Titik{x:1})) }",
        "maksudnya `Titik`?",
        "maksudnya `Titik`?",
    ),
    (
        "nama tipe tak ada di dalam tipe majemuk",
        "package main\nfn f() -> Option<Hantu> { return None }\npub fn main() { wok(1) }",
        "tipe `Hantu` tidak ada",
        "tipe `Hantu` tidak ada",
    ),

    // `type` dan `mod` DIURAI LENGKAP lalu dibuang diam-diam di
    // `hoist_top_level` (lengan `_ => {}`).
    (
        "alias tipe ditolak dengan alasannya",
        "package main\ntype Meter = int\npub fn main() { wok(1) }",
        "belum punya arti di JRG",
        "belum punya arti di JRG",
    ),
    (
        "tipe terkait di dalam impl ditolak",
        "package main\nstruct S {}\nimpl S { type T = int\n fn f(self) { return 1 } }\npub fn main() { wok(1) }",
        "belum punya arti di JRG",
        "belum punya arti di JRG",
    ),
    (
        "modul sebaris ditolak, menunjuk yang bekerja",
        "package main\nmod dalam { pub fn f() { return 7 } }\npub fn main() { wok(1) }",
        "modul sebaris",
        "modul sebaris",
    ),

    // Jumlah sasaran dan nilai HARUS sama — kecuali SATU nilai, yang berarti membongkar tuple
    // (Fase BONGKAR) dan karenanya harus tuple berisi persis sebanyak sasarannya.
    (
        "penugasan jamak: jumlah tidak cocok",
        "package main\npub fn main() { var a = 1; var b = 2; var c = 3; a, b, c = 1, 2; wok(a, b) }",
        "jumlahnya harus sama",
        "jumlahnya harus sama",
    ),
    (
        "bongkar tuple: sisi kanan bukan tuple",
        "package main\npub fn main() { var a = 1; var b = 2; a, b = 1; wok(a, b) }",
        "butuh nilai berupa tuple berisi 2 nilai di sisi kanan, dapat `int`",
        "butuh nilai berupa tuple berisi 2 nilai di sisi kanan, dapat `int`",
    ),
    (
        "bongkar tuple: isi tuple tidak sebanyak sasarannya",
        "package main\npub fn main() { var a = 1; var b = 2; a, b = (1, 2, 3); wok(a, b) }",
        "tuple di sisi kanan berisi 3 nilai — jumlahnya harus sama",
        "tuple di sisi kanan berisi 3 nilai — jumlahnya harus sama",
    ),
    (
        "bongkar tuple: `a, b := 5` bukan tuple",
        "package main\npub fn main() { a, b := 5; wok(a, b) }",
        "`:=` dengan beberapa nama butuh tuple berisi sebanyak namanya di sisi kanan, dapat `int`",
        "`:=` dengan beberapa nama butuh tuple berisi sebanyak namanya di sisi kanan, dapat `int`",
    ),
    (
        "bongkar tuple: `a, b := (1, 2, 3)` jumlahnya beda",
        "package main\npub fn main() { a, b := (1, 2, 3); wok(a, b) }",
        "`:=` dengan beberapa nama butuh tuple berisi sebanyak namanya di sisi kanan, dapat `tuple`",
        "`:=` dengan beberapa nama butuh tuple berisi sebanyak namanya di sisi kanan, dapat `tuple`",
    ),
    (
        "json::parse: `tidak` bukan `true`",
        "package main\npub fn main() { wok(json::parse(\"tidak\")) }",
        "harusnya `true` di posisi 0, yang tertulis `tidak`",
        "harusnya `true` di posisi 0, yang tertulis `tidak`",
    ),
    (
        "json::parse: teks berlebih sesudah nilai",
        "package main\npub fn main() { wok(json::parse(\"1 2\")) }",
        "ada teks berlebih sesudah nilainya, mulai posisi 2",
        "ada teks berlebih sesudah nilainya, mulai posisi 2",
    ),
    (
        "json::parse: objek tak tertutup (dulu panik)",
        "package main\npub fn main() { wok(json::parse(\"{\")) }",
        "objek tidak ditutup dengan `}`",
        "objek tidak ditutup dengan `}`",
    ),
    (
        "json::parse: separuh pasangan surrogate",
        "package main\npub fn main() { wok(json::parse(\"\\\"\\\\ud83d\\\"\")) }",
        "separuh pasangan surrogate tanpa pasangannya",
        "separuh pasangan surrogate tanpa pasangannya",
    ),
    (
        "json::parse: angka berawalan nol",
        "package main\npub fn main() { wok(json::parse(\"01\")) }",
        "angka JSON tidak boleh berawalan nol",
        "angka JSON tidak boleh berawalan nol",
    ),
    (
        "json::parse: escape tak dikenal",
        "package main\npub fn main() { wok(json::parse(\"\\\"\\\\x\\\"\")) }",
        "tidak dikenal JSON",
        "tidak dikenal JSON",
    ),
    (
        "bongkar tuple: operator gabungan tidak membongkar",
        "package main\npub fn main() { var a = 1; var b = 2; a, b += (1, 2); wok(a, b) }",
        "jumlahnya harus sama",
        "jumlahnya harus sama",
    ),
    (
        "penugasan jamak: sasaran := bukan nama",
        "package main\npub fn main() { var a = [1]; a[0], b := 1, 2; wok(a) }",
        "sisi kiri `:=` harus nama variabel",
        "sisi kiri `:=` harus nama variabel",
    ),
    (
        "daftar berkoma tanpa penugasan",
        "package main\npub fn main() { var a = 1; var b = 2; a, b; wok(a) }",
        "cuma sah sebagai sisi kiri penugasan",
        "cuma sah sebagai sisi kiri penugasan",
    ),

    // Channel tertutup: dua kesalahan yang DULU dijawab diam-diam.
    // `<-c` atas channel tertutup+kosong menjawab `nil` telanjang di `--vm`,
    // yang tidak bisa dibedakan dari channel yang memang membawa `nil`.
    (
        "kirim ke channel yang sudah ditutup",
        "package main\npub fn main() { let c = Chan::new(1)  c.close()  c <- 1 }",
        "tidak bisa kirim lagi",
        "tidak bisa kirim lagi",
    ),
    (
        "terima dari channel tertutup dan kosong",
        "package main\npub fn main() { let c = Chan::new(1)  c.close()  wok(<-c) }",
        "tidak akan ada nilai lagi",
        "tidak akan ada nilai lagi",
    ),

    // Argumen yang ketiadaannya membuat operasinya TAK BERMAKNA.
    (
        "contains tanpa argumen",
        "package main\npub fn main() { wok([1,2,3].contains()) }",
        "butuh 1 argumen (nilai)",
        "butuh 1 argumen (nilai)",
    ),
    (
        "fill tanpa argumen",
        "package main\npub fn main() { wok([1,2,3].fill()) }",
        "butuh 1 argumen (nilai)",
        "butuh 1 argumen (nilai)",
    ),
    (
        "sort_by dengan pembanding bool",
        "package main\npub fn main() { wok([3,1,2].sort_by(|p, q| p < q)) }",
        "harus mengembalikan angka",
        "harus mengembalikan angka",
    ),

    // Aturan indeks JRG: di luar jangkauan SELALU error, tidak pernah dijepit.
    (
        "str::substr awal di luar jangkauan",
        "package main\npub fn main() { wok(str::substr(\"abc\", 99, 1)) }",
        "di luar jangkauan",
        "di luar jangkauan",
    ),
    // Fase KEPUTUSAN-1: negatif dihitung dari belakang, tapi melewati awal tetap galat yang menyebut
    // indeks yang DITULIS, tidak dijepit.
    (
        "indeks negatif builtin melewati awal",
        "package main\npub fn main() { let a = [1, 2, 3]\n wok(a.insert(-1, 9))\n wok(a.insert(-4, 9)) }",
        "arr::insert: index -4 di luar batas (panjang 3)",
        "arr::insert: index -4 di luar batas (panjang 3)",
    ),
    // Fase SUBSTR: awal negatif dihitung dari belakang; yang melewati awal menyebut angka tulisannya.
    (
        "str::substr awal negatif melewati awal",
        "package main\npub fn main() { wok(str::substr(\"abc\", -1, 3))\n wok(str::substr(\"abc\", -4, 3)) }",
        "str::substr: posisi awal -4 di luar jangkauan — string ini cuma 3 karakter",
        "str::substr: posisi awal -4 di luar jangkauan — string ini cuma 3 karakter",
    ),
    // Fase WARNA: `Warna` dulu rekaman bebas — `Warna { x: 1 }` diterima, gagal jauh di `canvas::`.
    (
        "Warna pustaka menolak field liar",
        "package main\npub fn main() { wok(Warna { r: 1, g: 2, b: 3 })\n wok(Warna { x: 1 }) }",
        "struct `Warna` tidak punya field `x`",
        "struct `Warna` tidak punya field `x`",
    ),
    // Fase JSON-SATU: dulu handle-nya mendapat field baru tanpa kabar, dan sesi yang
    // memuat fungsi menyalahkan `dict` pembungkusnya.
    (
        "dok::json_set atas handle pustaka",
        "package main\npub fn main() { wok(dok::json_set({\"a\": 1}, \"b\", 2))\n wok(dok::json_set(dok::router_new(), \"b\", 2)) }",
        "`DokRouter` adalah handle buatan pustaka — field `b` tidak bisa ditulis",
        "`DokRouter` adalah handle buatan pustaka — field `b` tidak bisa ditulis",
    ),
    (
        "dok::session_encode menyebut isi yang bukan JSON, bukan wadahnya",
        "package main\npub fn main() { wok(dok::session_encode({\"f\": |x| x}, \"rahasia-yang-panjang-sekali\")) }",
        "`fungsi` tidak punya padanan di JSON",
        "`fungsi` tidak punya padanan di JSON",
    ),
    (
        "str::slice batas di luar jangkauan",
        "package main\npub fn main() { wok(str::slice(\"abc\", 5, 6)) }",
        "di luar jangkauan",
        "di luar jangkauan",
    ),

    // Ditutup di fase copy-on-write; sebelumnya entri `VM_LEBIH_PERMISIF`.
    (
        "tuple: .N di luar batas di jalur tulis",
        "package main\npub fn main() { var t = ([1,2], 0); t.7[0] = 99; wok(t) }",
        "index [7] di luar batas — tuple ini isinya 2 elemen",
        "index [7] di luar batas — tuple ini isinya 2 elemen",
    ),

    // ── Fase KUNCI: pintu struct/dict di VM ─────────────────────────────
    // Tiga yang pertama MENGGANTUNG di `--vm` sebelum fase ini: Mutex yang sama
    // dikunci dua kali. Batas waktu pelari tes yang membuat regresinya merah.
    (
        "for-in atas struct ditolak, bukan menggantung",
        "package main\nstruct P { pub a: int }\npub fn main() { let s = P { a: 1 }\n for k in s { wok(k) } }",
        "`P` itu struct, bukan koleksi", "`P` itu struct, bukan koleksi",
    ),
    (
        "for-in atas enum ditolak, bukan menggantung",
        "package main\npub fn main() { for k, v in Option::Some(1) { wok(k, v) } }",
        "tidak bisa iterate `enum:Option`", "tidak bisa iterate `enum:Option`",
    ),
    (
        "tulis lewat field yang belum ada ditolak, bukan menggantung",
        "package main\nstruct Q { pub x: int }\nstruct P { pub a: int, pub q: Q }\n\
         pub fn main() { var p = P { a: 1, q: Q { x: 2 } }\n p.zz.x = 5\n wok(p) }",
        "field `zz` belum ada", "field `zz` belum ada",
    ),
    (
        "tulis lewat key yang belum ada, ditulis dengan titik",
        "package main\npub fn main() { var d = {\"a\": {\"b\": 1}}\n d.zz.b = 5\n wok(d) }",
        "key `zz` belum ada", "key `zz` belum ada",
    ),
    (
        "tuple: index di luar batas di jalur tulis",
        "package main\npub fn main() { var t = (1, 2)\n wok(\"mulai\")\n t[5] = 1 }",
        "index [5] di luar batas — tuple ini isinya 2 elemen", "index [5] di luar batas — tuple ini isinya 2 elemen",
    ),
    (
        "for-in atas tuple ditolak di dua backend",
        "package main\npub fn main() { for x in (7, 8) { wok(x) } }",
        "tidak bisa iterate `tuple`", "tidak bisa iterate `tuple`",
    ),
    // Interpreter dulu memotong: `a[1.5] = 9` dan `a[true] = 9` sama-sama menulis ke `a[1]`.
    (
        "index float di jalur tulis ditolak seperti di jalur baca",
        "package main\npub fn main() { var a = [1, 2]\n a[1.5] = 9\n wok(a) }",
        "tidak bisa index `array` dengan `float`", "tidak bisa index `array` dengan `float`",
    ),
    (
        "index bool di langkah tengah jalur tulis ditolak",
        "package main\npub fn main() { var a = [[1], [2]]\n a[true][0] = 9\n wok(a) }",
        "tidak bisa index `array` dengan `bool`", "tidak bisa index `array` dengan `bool`",
    ),
    (
        "dict:: menolak enum, bukan membocorkan kunci internalnya",
        "package main\npub fn main() { wok(dict::keys(Option::Some(1))) }",
        "argumen ke-1 harus dict, dapat `enum:Option`", "argumen ke-1 harus dict, dapat `enum:Option`",
    ),
    (
        "dict::merge menyebut posisi argumen yang salah",
        "package main\nstruct P { pub a: int }\npub fn main() { wok(dict::merge({\"b\": 2}, P { a: 1 })) }",
        "argumen ke-2 harus dict, dapat `struct:P`", "argumen ke-2 harus dict, dapat `struct:P`",
    ),
    // VM dulu mengarang `nil` untuk argumen yang tidak pernah ditulis.
    (
        "dict::get_or tanpa cadangan ditolak",
        "package main\npub fn main() { wok(dict::get_or({\"a\": 1}, \"a\")) }",
        "`dict::get_or` butuh 3 argumen, dapat 2", "`dict::get_or` butuh 3 argumen, dapat 2",
    ),
    (
        "dict::set tanpa nilai ditolak",
        "package main\npub fn main() { wok(dict::set({\"a\": 1}, \"b\")) }",
        "`dict::set` butuh 3 argumen, dapat 2", "`dict::set` butuh 3 argumen, dapat 2",
    ),
    (
        "dict::from_pairs tanpa argumen ditolak",
        "package main\npub fn main() { wok(dict::from_pairs()) }",
        "`dict::from_pairs` butuh 1 argumen, dapat 0", "`dict::from_pairs` butuh 1 argumen, dapat 0",
    ),
    (
        "dict::from_pairs menyebut panjang pasangan, bukan isinya",
        "package main\npub fn main() { wok(dict::from_pairs([[1, 2, 3]])) }",
        "elemen di index 0 harus pasangan [kunci, nilai], dapat 3 elemen",
        "elemen di index 0 harus pasangan [kunci, nilai], dapat 3 elemen",
    ),
    (
        "dict::new dengan argumen ditolak",
        "package main\npub fn main() { wok(dict::new({\"a\": 1})) }",
        "kebanyakan argumen", "kebanyakan argumen",
    ),

    // ── Fase LABEL: jenis nilai VM di label, bukan di kunci peta ────────
    // `--vm` dulu membaca dan menulis kunci label enum sebagai data biasa:
    // `o.variant` menjawab "Some", dan `o.variant = "None"` mengganti variannya.
    (
        "enum tidak punya field untuk dibaca",
        "package main\npub fn main() { let o = Option::Some(1)\n wok(o.variant) }",
        "`enum:Option` tidak punya method atau field bernama `variant`",
        "`enum:Option` tidak punya method atau field bernama `variant`",
    ),
    (
        "enum tidak bisa di-index dengan string",
        "package main\npub fn main() { wok(Option::Some(1)[\"variant\"]) }",
        "tidak bisa index `enum:Option`", "tidak bisa index `enum:Option`",
    ),
    (
        "enum tidak bisa di-index dengan angka",
        "package main\npub fn main() { wok(Option::Some(1)[0]) }",
        "tidak bisa index `enum:Option`", "tidak bisa index `enum:Option`",
    ),
    (
        "varian enum tidak bisa diganti lewat field",
        "package main\npub fn main() { var o = Option::Some(1)\n o.variant = \"None\"\n wok(o) }",
        "`enum:Option` tidak punya field", "`enum:Option` tidak punya field",
    ),
    (
        "enum tidak bisa ditulis lewat index",
        "package main\npub fn main() { var o = Option::Some(1)\n o[\"x\"] = 1\n wok(o) }",
        "`enum:Option` tidak bisa ditulis lewat `[...]`", "`enum:Option` tidak bisa ditulis lewat `[...]`",
    ),
    (
        "field varian struct tidak dibaca lewat enumnya",
        "package main\nenum E { V { a: int } }\npub fn main() { let e = E::V { a: 1 }\n wok(e.a) }",
        "`enum:E` tidak punya method atau field bernama `a`",
        "`enum:E` tidak punya method atau field bernama `a`",
    ),
    // Interpreter dulu mengabaikan basis tanpa field diam-diam; VM menolaknya sambil menyebut `map`.
    (
        "spread dari angka ditolak",
        "package main\nstruct P { pub a: int, pub b: int = 2 }\npub fn main() { wok(P { a: 1, ...5 }) }",
        "cuma bisa menyalin dari struct, dict, atau varian enum ber-field — dapat `int`",
        "cuma bisa menyalin dari struct, dict, atau varian enum ber-field — dapat `int`",
    ),
    (
        "spread dari enum tanpa field ditolak",
        "package main\nstruct P { pub a: int, pub b: int = 2 }\npub fn main() { wok(P { a: 1, ...Option::None }) }",
        "dapat `enum:Option`", "dapat `enum:Option`",
    ),
    (
        "spread dari array ditolak",
        "package main\nstruct P { pub a: int }\npub fn main() { wok(P { a: 1, ...[1, 2] }) }",
        "dapat `array`", "dapat `array`",
    ),
    // ── Pintu method HOF, bukan-fungsi, dan sebar (MESIN-2) ─────────────────
    (
        "hof: argumen berlebih di method bawaan ditolak",
        "package main\npub fn main() { wok([1, 2].map(|x| x, 5)) }",
        "method `map` butuh 1 argumen (fn), dapat 2", "method `map` butuh 1 argumen (fn), dapat 2",
    ),
    (
        "hof: Option::map tanpa fungsi ditolak",
        "package main\npub fn main() { wok(Option::Some(1).map()) }",
        "method `map` butuh 1 argumen (fn), dapat 0", "method `map` butuh 1 argumen (fn), dapat 0",
    ),
    (
        "hof: method HOF atas int ditolak dengan kalimat bersama",
        "package main\npub fn main() { let n = 5\n wok(n.map(|x| x)) }",
        "`int` tidak punya method atau field bernama `map`",
        "`int` tidak punya method atau field bernama `map`",
    ),
    (
        "bukan fungsi: diserahkan ke HOF namespace",
        "package main\npub fn main() { wok(arr::map([1], 5)) }",
        "nilai bertipe `int` bukan fungsi", "nilai bertipe `int` bukan fungsi",
    ),
    (
        "fold: urutan argumen terbalik",
        "package main\npub fn main() { wok([1].fold(|a, b| a, 0)) }",
        "nilai bertipe `int` bukan fungsi", "nilai bertipe `int` bukan fungsi",
    ),
    (
        "arr::fold: argumen pertama bukan array",
        "package main\npub fn main() { wok(arr::fold(5, 0, |a, b| a)) }",
        "arr::fold: argumen ke-1 harus array, dapat `int`",
        "arr::fold: argumen ke-1 harus array, dapat `int`",
    ),
    // `f(...5)` dulu menyebut `terap` dan `[1, ...5]` menyebut `arr::concat` — dua nama hasil
    // penurunan yang tidak pernah ditulis pemakainya.
    (
        "sebar di pemanggilan: yang disebar harus array",
        "package main\nfn f(a) { return a }\npub fn main() { wok(f(...5)) }",
        "cuma bisa menyebar array — dapat `int`", "cuma bisa menyebar array — dapat `int`",
    ),
    (
        "sebar campuran di pemanggilan",
        "package main\nfn f(a, b) { return a }\npub fn main() { wok(f(1, ...5)) }",
        "cuma bisa menyebar array — dapat `int`", "cuma bisa menyebar array — dapat `int`",
    ),
    (
        "sebar string menyebut str::chars",
        "package main\npub fn main() { wok([...\"ab\"]) }",
        "`...str::chars(s)`", "`...str::chars(s)`",
    ),
    (
        "memanggil bukan fungsi lewat sebar",
        "package main\npub fn main() { let x = 5\n wok(x(...[1])) }",
        "nilai bertipe `int` bukan fungsi", "nilai bertipe `int` bukan fungsi",
    ),

    // ── Audit pasca-COW: tujuh divergensi yang ditemukan sekaligus ──────

    // Fase SERPIH: sisi kiri `=` yang bukan tempat ditolak PARSER — dulu `5 = 3` gagal saat
    // jalan di acuan dan jadi `compile error: invalid assignment target` di `--vm`.
    ("sasaran: literal", "package main\npub fn main() { wok(\"mulai\")\n 5 = 3 }",
     "nilai literal bukan sesuatu yang bisa di-assign", "nilai literal bukan sesuatu yang bisa di-assign"),
    ("sasaran: hasil operasi", "package main\npub fn main() { var a = 1\n var b = 2\n a + b = 3 }",
     "hasil operasi bukan sesuatu yang bisa di-assign", "hasil operasi bukan sesuatu yang bisa di-assign"),
    ("sasaran: nama ber-:: cuma bisa dibaca", "package main\npub fn main() { math::PI = 3 }",
     "`math::PI` bukan sesuatu yang bisa di-assign — nama ber-`::`",
     "`math::PI` bukan sesuatu yang bisa di-assign — nama ber-`::`"),
    ("sasaran: potongan adalah salinan", "package main\npub fn main() { var a = [1, 2, 3]\n a[0:2] = [9, 9] }",
     "potongan (`x[a:b]`) bukan sesuatu yang bisa di-assign", "potongan (`x[a:b]`) bukan sesuatu yang bisa di-assign"),
    ("sasaran: tuple ditulis tanpa kurung", "package main\npub fn main() { var a = 1\n var b = 2\n (a, b) = (b, a) }",
     "tulis tanpa kurung: `a, b = b, a`", "tulis tanpa kurung: `a, b = b, a`"),
    ("main bukan fungsi", "package main\nconst main = 5",
     "`main` di berkas ini bukan fungsi", "`main` di berkas ini bukan fungsi"),
    // `_` tidak pernah jadi variabel. Dulu `var _ = 1; _ = 2; wok(_)` menjawab `2` di acuan
    // dan LLVM tapi `1` di `--vm`, dan `let _ = 7; wok(_)` ditolak acuan saja.
    ("pembuang dibaca", "package main\npub fn main() { var _ = 1\n wok(_) }",
     "`_` cuma untuk membuang nilai", "`_` cuma untuk membuang nilai"),
    ("pembuang dibaca sesudah let", "package main\npub fn main() { let _ = 7\n wok(_ + 1) }",
     "`_` cuma untuk membuang nilai", "`_` cuma untuk membuang nilai"),
    ("pembuang: penugasan gabungan membacanya", "package main\npub fn main() { var _ = 1\n _ += 1 }",
     "`_` cuma untuk membuang nilai", "`_` cuma untuk membuang nilai"),
    ("pembuang: indeks lewat `_`", "package main\npub fn main() { var _ = [1]\n _[0] = 2 }",
     "`_` cuma untuk membuang nilai", "`_` cuma untuk membuang nilai"),
    ("pembuang di nilai awal tingkat atas", "package main\nconst A = _\npub fn main() { wok(1) }",
     "`_` cuma untuk membuang nilai", "`_` cuma untuk membuang nilai"),
    // Yang paling parah dari ketujuhnya, dan satu-satunya yang bukan sekadar
    // "lebih permisif": sejak transfer nilai jadi O(1), nilai balik sebuah …
    (
        "hasil panggilan bukan target penugasan",
        "package main\nstruct P { pub x: int = 0 }\nfn lewat(p) { return p }\n\
         pub fn main() { var a = P{x:1}; lewat(a).x = 99; wok(a.x) }",
        "bukan sesuatu yang bisa di-assign",
        "bukan sesuatu yang bisa di-assign",
    ),
    // VM me-mask jumlah geseran (`b & 127`), jadi tiga permintaan yang
    // mustahil dijawab tiga angka yang tidak berhubungan: `1 << 128` -> 1,
    // `1 << 200` -> 1<<72, `1 << -1` -> i128::MIN.
    (
        "geser >= 128 ditolak",
        "package main\npub fn main() { wok(1 << 128) }",
        "cuma 128 bit",
        "cuma 128 bit",
    ),
    (
        "geser negatif ditolak",
        "package main\npub fn main() { wok(1 << -1) }",
        "tidak boleh negatif",
        "tidak boleh negatif",
    ),
    // VM mengalikan dalam u64 lalu melebarkan hasilnya, jadi factorial(40)
    // keluar terbalik dalam 64 bit sebagai 18376134811363311616.
    (
        "factorial yang meluap ditolak",
        "package main\npub fn main() { wok(math::factorial(40)) }",
        "kelewat besar",
        "kelewat besar",
    ),
    // Pembanding VM jatuh ke `Ordering::Equal` untuk pasangan tipe yang tak
    // dikenalinya — persis "dianggap sama" yang dilarang aturan keras.
    (
        // Kedua kolomnya SENGAJA identik sekarang: sampai Fase MUDAH interpreter
        // menyebut NILAI-nya ("`a` (tipe string) dan `1` …") sementara VM menyebut …
        "sort nilai tak sebanding ditolak",
        "package main\npub fn main() { wok(arr::sort([1, \"a\"])) }",
        "tidak bisa membandingkan",
        "tidak bisa membandingkan",
    ),
    // Semua slot parameter di VM sudah ada dan berisi Nil sejak frame
    // dibuat, jadi default yang menyebut parameter di KANANNYA membaca slot
    // kosong dan menjawab `nil` alih-alih mengeluh.
    (
        "default parameter tidak boleh menoleh ke kanan",
        "package main\nfn f(a, b = c, c = 5) { return b }\npub fn main() { wok(f(3)) }",
        "belum dikenal",
        "posisinya ada di KANANNYA",
    ),
    // Ditutup di Fase VISIBILITAS.
    (
        "field privat tidak bisa diakses dari luar, termasuk lewat turunan",
        "package main\nstruct Base { rahasia: int = 7 }\nstruct Turunan: Base { }\n\
         pub fn main() { let d = Turunan{}; wok(d.rahasia) }",
        "bersifat private",
        "bersifat private",
    ),
    // VM memetakan `uint` ke builtin `int`, jadi tandanya tidak pernah
    // diperiksa — dan salahnya bahkan tidak konsisten: `(-1) as uint`
    // menjawab `-1`, tapi lewat variabel menjawab `0`.
    (
        "cast negatif ke uint ditolak",
        "package main\npub fn main() { var n = -1; wok(n as uint) }",
        "tidak punya padanan di tipe tak bertanda",
        "tidak punya padanan di tipe tak bertanda",
    ),
    // Salah ketik nama modul `std/` tetap DITOLAK, sesudah namespace bawaan
    // dilewati diam-diam. Tanpa kasus ini, melonggarkan pemeriksaannya
    // supaya `std/json` lolos juga akan meloloskan `std/hantu`.
    ("import std/ yang tidak ada ditolak",
     "package main\nimport \"std/hantu\"\npub fn main() { wok(1) }",
     "tidak ada", "tidak ada"),
    // Nama method boleh kata kunci di dalam `impl` — dan HANYA di sana.

    // Konstruktor struct tuple menuntut arity-nya seperti fungsi lain —
    // sebuah `Titik(1)` yang diterima akan mengarang field kedua.
    ("konstruktor struct tuple menuntut arity",
     "package main\nstruct Titik(int, int)\npub fn main() { wok(Titik(1)) }",
     "wajib diisi", "wajib diisi"),

    // `terap` menolak yang bukan fungsi dan yang bukan array dengan sebabnya
    // — sebuah "argumen" yang bukan array tidak bisa disebarkan, dan
    // memperlakukannya sebagai satu argumen adalah tebakan.
    ("terap menolak argumen pertama yang bukan fungsi",
     "package main\npub fn main() { wok(terap(5, [1])) }",
     "nilai bertipe `int` bukan fungsi", "nilai bertipe `int` bukan fungsi"),
    ("terap menolak argumen kedua yang bukan array",
     "package main\nfn f(a) { return a }\npub fn main() { wok(terap(f, 5)) }",
     "harus array", "harus array"),

    // Empat cara salah memakai argumen bernama, masing-masing dengan
    // kalimatnya sendiri. "Salah" saja tidak memberi tahu yang mana.
    ("argumen bernama: nama yang tidak ada",
     "package main\nfn bagi(a, b) { return a / b }\npub fn main() { wok(bagi(a: 1, salah: 2)) }",
     "tidak ada parameter bernama", "tidak ada parameter bernama"),
    ("argumen bernama: mengisi dua kali lewat posisi",
     "package main\nfn bagi(a, b) { return a / b }\npub fn main() { wok(bagi(10, a: 1)) }",
     "sudah diisi lewat posisi", "sudah diisi lewat posisi"),
    ("argumen bernama: melompati parameter",
     "package main\nfn f(a, b = 1, c = 2) { return a }\npub fn main() { wok(f(a: 1, c: 3)) }",
     "dilewati", "dilewati"),
    ("argumen bernama: builtin tidak menyimpan nama parameternya",
     "package main\npub fn main() { wok(len(x: 1)) }",
     "ditulis di JRG", "ditulis di JRG"),

    // Bentuk dari bahasa lain yang orang wajar coba: ditolak sambil MENYEBUT
    // padanannya di sini.
    ("diskriminan enum ditolak sambil menyebut match",
     "package main\nenum E { A = 5 }\npub fn main() { wok(1) }",
     "match", "match"),
    ("do-while ditolak sambil menyebut loop",
     "package main\npub fn main() { var i = 0\n do { i = i + 1 } while i < 3 }",
     "loop", "loop"),
    ("switch ditolak sambil menyebut match",
     "package main\npub fn main() { switch 2 { case 1: wok(\"a\") } }",
     "match", "match"),
    ("try/catch ditolak sambil menyebut Result dan ?",
     "package main\npub fn main() { try { wok(1) } catch e { wok(e) } }",
     "Result", "Result"),

    // `for (a, b) in ...` ditolak sambil menyebut bentuk yang benar.
    ("pola di posisi indeks for ditolak sambil menyebut bentuk dua-nama",
     "package main\npub fn main() { for (a, b) in [(1,2)] { wok(a) } }",
     "for _, (a, b)", "for _, (a, b)"),

    // Tiga bentuk terakhir yang masih ditolak, masing-masing menyebut jalan
    // keluarnya. Sebelumnya ketiganya galat parse generik.
    ("slice berlangkah ditolak sambil menyebut for berindeks",
     "package main\npub fn main() { wok([1,2,3,4][::2]) }",
     "slice berlangkah", "slice berlangkah"),
    ("destrukturisasi dict ditolak sambil menyebut d[\"x\"]",
     "package main\npub fn main() { let {x, y} = {\"x\": 1}\n wok(x) }",
     "destrukturisasi dict", "destrukturisasi dict"),
    ("generik const ditolak sambil menyebut sebabnya",
     "package main\nfn f<const N: int>() -> int { return N }\npub fn main() { wok(1) }",
     "generik const", "generik const"),

];

#[test]
fn program_tak_sah_ditolak_dua_backend() {
    let mut gagal = Vec::new();
    for (nama, src, kunci_interp, kunci_vm) in DITOLAK_DUA_BACKEND {
        let i = jalankan(src, false);
        let v = jalankan(src, true);
        if i.sukses {
            gagal.push(format!(
                "  {} -> interpreter sekarang MENERIMA program ini. Kalau itu \
                 memang disengaja, pindahkan kasusnya ke `PARITAS`.",
                nama
            ));
        } else {
            // Pesan error keluar lewat stderr, bukan stdout — `Hasil.keluaran`
            // sengaja cuma memuat stdout karena itu yang dibandingkan `PARITAS`.
            let pesan = jalankan_keluaran_penuh(src, false).to_lowercase();
            if !pesan.contains(&kunci_interp.to_lowercase()) {
                gagal.push(format!(
                    "  {} -> interpreter menolak, tapi pesannya tidak menyebut `{}`:\n     {}",
                    nama, kunci_interp, pesan.trim()
                ));
            }
        }
        if v.sukses {
            gagal.push(format!(
                "  {} -> VM sekarang MENERIMA program ini padahal interpreter \
                 menolak. Itu berarti satu program punya dua arti lagi — \
                 perbaiki VM, jangan tambah entri divergensi.",
                nama
            ));
        } else {
            let pesan = jalankan_keluaran_penuh(src, true).to_lowercase();
            if !pesan.contains(&kunci_vm.to_lowercase()) {
                gagal.push(format!(
                    "  {} -> VM menolak, tapi pesannya tidak menyebut `{}`:\n     {}",
                    nama, kunci_vm, pesan.trim()
                ));
            }
        }
    }
    assert!(
        gagal.is_empty(),
        "{} kasus `DITOLAK_DUA_BACKEND` tidak lagi cocok:\n{}",
        gagal.len(),
        gagal.join("\n")
    );
}

#[test]
fn yang_lebih_permisif_di_vm_masih_akurat() {
    let mut kabar_baik = Vec::new();
    for (nama, src, kenapa) in VM_LEBIH_PERMISIF {
        let i = jalankan(src, false);
        let v = jalankan(src, true);
        if i.sukses {
            kabar_baik.push(format!(
                "  {} -> interpreter sekarang MENERIMA program ini, jadi kasusnya \
                 tidak lagi menunjukkan VM lebih permisif.\n     catatan lama: {}",
                nama, kenapa
            ));
        } else if !v.sukses {
            kabar_baik.push(format!(
                "  {} -> VM sekarang ikut menolak. Kabar baik: hapus entri ini.\n     \
                 catatan lama: {}",
                nama, kenapa
            ));
        }
    }
    assert!(
        kabar_baik.is_empty(),
        "{} entri `VM_LEBIH_PERMISIF` sudah basi:\n{}",
        kabar_baik.len(),
        kabar_baik.join("\n")
    );
}

#[test]
fn yang_belum_didukung_vm_gagal_dengan_jujur() {
    let mut gagal = Vec::new();
    for (nama, src, kata_kunci) in BELUM_DIDUKUNG_VM {
        let i = jalankan(src, false);
        let v = jalankan(src, true);
        if !i.sukses {
            gagal.push(format!("  {} -> interpreter sendiri gagal; kasusnya tidak sahih", nama));
            continue;
        }
        if v.sukses {
            gagal.push(format!(
                "  {} -> VM ternyata SUDAH jalan. Kabar baik: pindahkan ke `PARITAS` \
                 dan perbarui tabel divergensi di catatan proyek.",
                nama
            ));
            continue;
        }
        let pesan = jalankan_keluaran_penuh(src, true);
        if !pesan.to_lowercase().contains(&kata_kunci.to_lowercase()) {
            gagal.push(format!(
                "  {} -> ditolak, tapi pesannya tidak menyebut {:?}\n     pesan: {}",
                nama, kata_kunci, pesan.trim()
            ));
        }
    }
    assert!(gagal.is_empty(), "{} kasus:\n{}", gagal.len(), gagal.join("\n"));
}

#[test]
fn namespace_interpreter_saja_dijelaskan_bukan_dituduh_typo() {
    let mut gagal = Vec::new();
    for (nama, src, awalan) in NAMESPACE_INTERPRETER_SAJA {
        let i = jalankan(src, false);
        if !i.sukses {
            gagal.push(format!("  {} -> interpreter sendiri gagal; kasusnya tidak sahih", nama));
            continue;
        }
        let v = jalankan(src, true);
        if v.sukses {
            gagal.push(format!(
                "  {} -> VM ternyata SUDAH mendukung `{}`. Kabar baik, tapi berarti \
                 tabel ini dan catatan proyek perlu diperbarui.",
                nama, awalan
            ));
            continue;
        }
        let pesan = jalankan_keluaran_penuh(src, true).to_lowercase();
        if !pesan.contains(&awalan.to_lowercase()) {
            gagal.push(format!(
                "  {} -> ditolak, tapi pesannya tidak menyebut `{}`\n     pesan: {}",
                nama, awalan, pesan.trim()
            ));
        }
        // Ini yang tadinya salah: pesan generik "belum didefinisikan" tidak menyebut
        // SEBAB (namespace interpreter-saja), cuma menuduh nama itu tidak pernah …
        if pesan.contains("belum didefinisikan") {
            gagal.push(format!(
                "  {} -> pesannya masih pakai template generik \"belum didefinisikan\", \
                 padahal alasannya sudah diketahui (namespace interpreter-saja)\n     pesan: {}",
                nama, pesan.trim()
            ));
        }
    }
    assert!(gagal.is_empty(), "{} kasus:\n{}", gagal.len(), gagal.join("\n"));
}

/// Kebalikan dari test di atas: variabel yang BENAR-BENAR tidak pernah
/// dideklarasikan harus tetap dapat pesan generik lama.
#[test]
fn variabel_asal_ketik_tetap_pakai_pesan_generik() {
    let pesan = jalankan_keluaran_penuh(
        "package main\npub fn main() { wok(nama_yang_sungguhan_tidak_pernah_ada) }",
        true,
    ).to_lowercase();
    // "belum dikenal", bukan "belum didefinisikan": kalimatnya sekarang satu
    // definisi yang dipakai kedua backend.
    assert!(
        pesan.contains("nama_yang_sungguhan_tidak_pernah_ada") && pesan.contains("belum dikenal"),
        "variabel asal-ketik biasa harus tetap dapat pesan generik; pesan: {}",
        pesan.trim()
    );
    assert!(
        !pesan.contains("--vm") && !pesan.contains("interpreter"),
        "salah ketik biasa malah dijelaskan sebagai namespace interpreter-saja: {}",
        pesan.trim()
    );
}

/// Nama yang tidak dikenal harus dijelaskan dengan kalimat yang SAMA, dan
/// dibantu dengan saran yang SAMA, di kedua backend.
#[test]
fn nama_tak_dikenal_dijelaskan_sama_di_dua_backend() {
    // (nama yang ditulis, potongan yang wajib ada di KEDUA pesan)
    let kasus: &[(&str, &str)] = &[
        // Path yang salah namespace-nya: `jawir::free` yang benar.
        ("jawa::free",                    "maksudnya `jawir::free`?"),
        // Nama telanjang yang beda satu huruf dari builtin.
        ("wokk",                          "maksudnya `wok`?"),
        // Yang tidak mirip apa pun: tidak boleh dikarang saran.
        ("zzz_tidak_mirip_apa_pun_zzz",   "belum dikenal"),
    ];
    for (nama, wajib) in kasus {
        let src = format!("package main\npub fn main() {{ wok({}) }}", nama);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        for (mesin, pesan) in [("interp", &i), ("vm", &v)] {
            assert!(pesan.contains(wajib),
                "{}: pesan untuk `{}` tidak memuat {:?}\n{}", mesin, nama, wajib, pesan.trim());
        }
        // Dan tidak boleh ada saran yang dikarang untuk nama yang jauh.
        if nama.starts_with("zzz") {
            for (mesin, pesan) in [("interp", &i), ("vm", &v)] {
                assert!(!pesan.contains("maksudnya"),
                    "{}: nama yang tidak mirip apa pun malah dikasih saran\n{}",
                    mesin, pesan.trim());
            }
        }
    }
}

/// `str::is_numeric(" ")` dulu menjawab `true` — spasi dianggap angka —
/// karena `trim()` dipanggil SEBELUM memeriksa kosong, dan …
#[test]
fn pemeriksa_bentuk_teks_tidak_menerima_spasi_kosong() {
    let kasus: &[(&str, &str)] = &[
        // (ekspresi, jawaban yang benar)
        (r#"str::is_numeric(" ")"#, "false"),
        (r#"str::is_numeric("")"#, "false"),
        (r#"str::is_numeric("\t")"#, "false"),
        (r#"str::is_alpha(" ")"#, "false"),
        (r#"str::is_alphanumeric(" ")"#, "false"),
        // Sisi POSITIF: yang memang angka harus tetap diterima, termasuk
        // yang berspasi di tepinya — kalau tidak, perbaikannya menukar satu
        // kesalahan dengan kesalahan lain.
        (r#"str::is_numeric("42")"#, "true"),
        (r#"str::is_numeric(" 42 ")"#, "true"),
        (r#"str::is_numeric("-3.5")"#, "true"),
        (r#"str::is_numeric(".5")"#, "true"),
        // Bukan angka utuh: dulu `true`, karena tiap karakternya digit, `.` atau `-`.
        (r#"str::is_numeric("-")"#, "false"),
        (r#"str::is_numeric(".")"#, "false"),
        (r#"str::is_numeric("1.2.3")"#, "false"),
        (r#"str::is_numeric("3-")"#, "false"),
        (r#"str::is_numeric("--1")"#, "false"),
        (r#"str::is_alpha("abc")"#, "true"),
        (r#"str::is_alphanumeric("a1")"#, "true"),
    ];
    for (e, mau) in kasus {
        let src = format!("package main\npub fn main() {{ wok({}) }}", e);
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, v.keluaran, "`{}` beda antar-backend", e);
        assert_eq!(i.keluaran.trim(), *mau, "`{}` harusnya {}", e, mau);
    }
}

/// Tanda tangan di `bantu::DOKUMEN` adalah KLAIM, dan klaim yang salah lebih
/// menyesatkan daripada tidak ada penjelasan sama sekali.
#[test]
fn tanda_tangan_terdokumentasi_cocok_dengan_tipe_sebenarnya() {
    // (ekspresi pemanggilan, nama yang didokumentasikan). Hanya fungsi yang
    // aman dipanggil tanpa efek samping — tidak ada berkas, soket, atau JVM.
    let contoh: &[(&str, &str)] = &[
        ("dok::vec_add([1.0],[2.0])", "dok::vec_add"),
        ("dok::vec_dot([1.0],[2.0])", "dok::vec_dot"),
        ("dok::vec_norm([3.0,4.0])", "dok::vec_norm"),
        ("dok::vec_lerp([0.0],[1.0],0.5)", "dok::vec_lerp"),
        ("dok::mat_identity(2)", "dok::mat_identity"),
        ("dok::mat_det(dok::mat_identity(2))", "dok::mat_det"),
        ("dok::mat_inverse(dok::mat_identity(2))", "dok::mat_inverse"),
        ("dok::stat_correlation([1.0,2.0,3.0],[2.0,4.0,6.0])", "dok::stat_correlation"),
        ("dok::stat_linreg([1.0,2.0,3.0],[2.0,4.0,6.0])", "dok::stat_linreg"),
        ("dok::num_is_prime(97)", "dok::num_is_prime"),
        ("dok::num_prime_factors(12)", "dok::num_prime_factors"),
        ("dok::num_to_base(255,16)", "dok::num_to_base"),
        ("dok::num_from_base(\"ff\",16)", "dok::num_from_base"),
        ("dok::url_encode(\"a b\")", "dok::url_encode"),
        ("dok::json_encode({\"a\":1})", "dok::json_encode"),
        ("dok::json_decode(\"{}\")", "dok::json_decode"),
        ("dok::json_get({\"a\":1},\"a\")", "dok::json_get"),
        ("dok::escape_html(\"<a>\")", "dok::escape_html"),
        ("dok::response(200,\"x\")", "dok::response"),
        ("dok::json_response({\"a\":1})", "dok::json_response"),
        ("dok::html(\"x\")", "dok::html"),
        ("dok::router_new()", "dok::router_new"),
        ("dok::db_tersedia()", "dok::db_tersedia"),
        ("dok::password_hash(\"a\")", "dok::password_hash"),
        ("dok::render(\"{{x}}\",{\"x\":1})", "dok::render"),
        ("jvm::tersedia()", "jvm::tersedia"),
        ("kotlin::tersedia()", "kotlin::tersedia"),
        ("jvm::objek_hidup()", "jvm::objek_hidup"),
        ("php::tersedia()", "php::tersedia"),
    ];
    // `typeof` menjawab nama JRG; tanda tangan memakai nama yang sama, kecuali
    // handle bertipe struct yang ditulis sebagai nama tipenya.
    let samakan = |t: &str| -> String {
        match t {
            "enum:Result" => "Result".to_string(),
            "enum:Option" => "Option".to_string(),
            lain => lain.strip_prefix("struct:").unwrap_or(lain).to_string(),
        }
    };
    let baris: Vec<String> = contoh.iter()
        .map(|(e, n)| format!("wok(\"{}\", typeof({}))", n, e))
        .collect();
    let src = format!("package main\npub fn main() {{\n{}\n}}\n", baris.join("\n"));
    let hasil = jalankan_keluaran_penuh(&src, false);
    let mut nyata: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    for l in hasil.lines() {
        let mut k = l.split_whitespace();
        if let (Some(n), Some(t)) = (k.next(), k.next()) {
            if let Some((_, nama)) = contoh.iter().find(|(_, x)| *x == n) {
                nyata.insert(nama, samakan(t));
            }
        }
    }
    assert_eq!(nyata.len(), contoh.len(),
        "tidak semua contoh berhasil dijalankan — keluaran:\n{}", hasil.trim());
    let mut salah = Vec::new();
    for (nama, asli) in &nyata {
        let Some(info) = jrgc::bantu::DOKUMEN.iter().find(|i| &i.nama == nama) else {
            salah.push(format!("  {} belum punya entri di DOKUMEN", nama));
            continue;
        };
        let ditulis = info.tanda_tangan.rsplit("->").next().unwrap_or("").trim();
        if ditulis != asli {
            salah.push(format!("  {:26} ditulis `{}`, sebenarnya `{}`", nama, ditulis, asli));
        }
    }
    assert!(salah.is_empty(),
        "tanda tangan yang didokumentasikan tidak cocok dengan tipe kembalian \
         sebenarnya:\n{}", salah.join("\n"));
}

/// Nama LOKAL yang salah ketik harus disarankan di kedua backend.
#[test]
fn nama_lokal_salah_ketik_disarankan_di_dua_backend() {
    let kasus: &[(&str, &str)] = &[
        // let biasa
        ("package main\npub fn main() { let jumlah_nilai = 1\n wok(jumlah_nilaii) }",
         "maksudnya `jumlah_nilai`?"),
        // parameter fungsi
        ("package main\nfn f(panjang: int) -> int { return panjan }\npub fn main() { wok(f(1)) }",
         "maksudnya `panjang`?"),
        // var, dan yang salah ketik dipakai di dalam blok bersarang
        ("package main\npub fn main() { var penghitung = 0\n if true { wok(penghitun) } }",
         "maksudnya `penghitung`?"),
    ];
    for (src, wajib) in kasus {
        let i = jalankan_keluaran_penuh(src, false);
        let v = jalankan_keluaran_penuh(src, true);
        for (mesin, pesan) in [("interp", &i), ("vm", &v)] {
            assert!(pesan.contains(wajib),
                "{}: saran untuk nama lokal hilang, harusnya {:?}\n{}",
                mesin, wajib, pesan.trim());
        }
    }
    // Sisi NEGATIF: lokal yang tidak mirip apa pun tidak boleh dikarang saran,
    // karena saran yang salah menyesatkan lebih jauh daripada tidak ada.
    let jauh = "package main\npub fn main() { let abc = 1\n wok(zzz_jauh_sekali_zzz) }";
    for (mesin, pesan) in [("interp", jalankan_keluaran_penuh(jauh, false)),
                           ("vm", jalankan_keluaran_penuh(jauh, true))] {
        assert!(!pesan.contains("maksudnya"),
            "{}: nama lokal yang tidak mirip malah dikasih saran\n{}", mesin, pesan.trim());
    }
}

#[test]
fn divergensi_yang_diketahui_masih_akurat() {
    let mut kabar_baik = Vec::new();
    for (nama, src, kenapa) in DIVERGENSI_DIKETAHUI {
        let i = jalankan(src, false);
        let v = jalankan(src, true);
        // Kalau VM-nya gagal jalan, itu bukan "sudah sepakat" — itu cuma
        // belum sampai ke titik yang dibandingkan.
        if !i.sukses || !v.sukses { continue; }
        if i.keluaran == v.keluaran {
            kabar_baik.push(format!(
                "  {}\n     sekarang SUDAH sepakat (dua-duanya {:?})\n     alasan lama: {}",
                nama, i.keluaran, kenapa
            ));
        }
    }
    assert!(
        kabar_baik.is_empty(),
        "Kabar baik: {} divergensi yang tercatat ternyata sudah sepakat.\n{}\n\n\
         Hapus entrinya dari `DIVERGENSI_DIKETAHUI` di file ini DAN dari tabel \
         divergensi di catatan proyek, supaya daftarnya tidak jadi basi.",
        kabar_baik.len(),
        kabar_baik.join("\n")
    );
}

/// Inti suite ini: program di `PARITAS` harus berjalan di DUA backend dan
/// menghasilkan keluaran yang identik.
#[test]
fn paritas_terjaga() {
    let mut gagal = Vec::new();
    for (nama, src) in PARITAS {
        let i = jalankan(src, false);
        let v = jalankan(src, true);
        if !i.sukses {
            gagal.push(format!(
                "  {} -> interpreter gagal; kasusnya sendiri tidak sahih\n     {}",
                nama, i.keluaran
            ));
        } else if !v.sukses {
            gagal.push(format!(
                "  {} -> VM gagal menjalankannya.\n     Kalau ini fitur yang memang \
                 belum didukung, pindahkan ke `BELUM_DIDUKUNG_VM`; jangan dibiarkan \
                 gagal di sini.",
                nama
            ));
        } else if i.keluaran != v.keluaran {
            gagal.push(format!(
                "  {} -> keluaran BEDA\n     interpreter: {:?}\n     vm         : {:?}",
                nama, i.keluaran, v.keluaran
            ));
        }
    }
    assert!(
        gagal.is_empty(),
        "{} kasus tidak sepakat antar-backend:\n{}",
        gagal.len(),
        gagal.join("\n")
    );
}

/// `--periksa` melaporkan nama yang tidak dideklarasikan tanpa menjalankan programnya — jadi
/// ia tidak boleh menuduh satu pun program yang JALAN. Tiap kasus `PARITAS` harus lolos, diam.
#[test]
fn periksa_tidak_menuduh_program_yang_jalan() {
    let mut gagal = Vec::new();
    for (nama, src) in PARITAS {
        let dir = crate::bersama::dir_unik("jrg-periksa-paritas");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        let file = dir.join("uji.jrg");
        std::fs::write(&file, src).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.arg(&file).arg("--periksa").env("JRG_SOPAN", "1");
        let out = jalankan_berbatas(cmd);
        let _ = std::fs::remove_dir_all(&dir);
        if !out.status_sukses || !out.stderr.is_empty() || !out.stdout.is_empty() {
            gagal.push(format!("  {} ->\n{}{}", nama,
                String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(gagal.is_empty(), "{} program yang jalan dituduh `--periksa`:\n{}", gagal.len(), gagal.join("\n"));
}

/// Tiap nama builtin harus ADA di dua backend, atau ditolak dengan menyebut
/// sebabnya.
#[test]
fn tidak_ada_builtin_yang_cuma_hidup_di_satu_backend() {
    // Nama telanjang yang memang bukan builtin biasa — masing-masing dengan
    // sebabnya, bukan dikecualikan lewat pola nama.
    const PENGECUALIAN: &[&str] = &[
        // Sintaks, bukan fungsi: `make_chan(int, 2)` diparse khusus.
        "make_chan",
        // Mengakhiri proses; memanggilnya dalam sapuan mengakhiri sapuannya.
        "exit", "std::exit",
    ];

    use std::collections::BTreeSet;

    // Dipanen dari sumbernya, bukan disalin ke sini — dan SELURUH `src/`
    // ditelusuri, bukan daftar berkas tulis tangan.
    let mut nama: BTreeSet<String> = BTreeSet::new();
    let mut berkas_dipindai = 0usize;
    fn kumpul(dir: &std::path::Path, nama: &mut BTreeSet<String>, berkas: &mut usize,
              kecuali: &[&str]) {
        let Ok(isi) = std::fs::read_dir(dir) else { return };
        for e in isi.flatten() {
            let p = e.path();
            if p.is_dir() { kumpul(&p, nama, berkas, kecuali); continue; }
            if p.extension().and_then(|s| s.to_str()) != Some("rs") { continue; }
            let Ok(teks) = std::fs::read_to_string(&p) else { continue };
            *berkas += 1;
            for pola in ["self.n(\"", ".def(\""] {
                for baris in teks.lines() {
                    let mut sisa = baris;
                    while let Some(i) = sisa.find(pola) {
                        sisa = &sisa[i + pola.len()..];
                        let Some(j) = sisa.find('"') else { break };
                        let n = &sisa[..j];
                        // Nama TELANJANG ikut dipanen sekarang.
                        if !n.starts_with("py::") && !n.contains("__")
                            && !kecuali.contains(&n) {
                            nama.insert(n.to_string());
                        }
                        sisa = &sisa[j..];
                    }
                }
            }
        }
    }
    kumpul(std::path::Path::new("src"), &mut nama, &mut berkas_dipindai, PENGECUALIAN);
    assert!(
        berkas_dipindai > 100,
        "penelusuran `src/` cuma membaca {} berkas — kemungkinan dijalankan dari \
         direktori yang salah, dan sapuan ini jadi hampa",
        berkas_dipindai
    );
    let dari_sumber = nama.len();
    assert!(
        dari_sumber > 400,
        "pemanenan dari sumber cuma dapat {} nama. Sebelum ini angkanya di atas \
         400; kalau turun drastis, polanya (`self.n(`/`.def(`) sudah tidak cocok \
         lagi dengan cara builtin didaftarkan",
        dari_sumber
    );
    // Sisi VM diambil dari VM-nya sendiri, bukan dari pemindaian sumber.
    for n in jrgc::vm::daftar_builtin() {
        if n.contains("::") && !n.starts_with("dok::") && !n.starts_with("py::") {
            nama.insert(n);
        }
    }

    assert!(
        nama.len() > 200,
        "pemanenan nama sepertinya rusak — cuma dapat {}. Test ini tidak \
         berguna kalau daftarnya kosong.",
        nama.len()
    );

    // Frasa yang menandai "nama ini memang tidak ada".
    const HILANG: &[&str] = &[
        "tidak ditemukan", "belum dikenal", "belum didefinisikan", "tidak ada di backend",
    ];
    // Sebab yang sah untuk sebuah nama cuma hidup di satu backend.
    const SEBAB: &[&str] = &[
        "cuma jalan di backend bytecode",   // butuh sel bersama milik VM
        "cuma tersedia di interpreter",     // namespace khusus interpreter
        "tidak ada di backend",
    ];

    let ada = |n: &str, vm: bool| -> (bool, String) {
        let src = format!("package main\npub fn main() {{ {}() }}\n", n);
        let h = jalankan_keluaran_penuh(&src, vm);
        (!HILANG.iter().any(|x| h.contains(x)), h)
    };

    // ── Penjaga instrumen ──────────────────────────────────────────────
    for vm in [false, true] {
        let (terdeteksi_ada, pesan) = ada("nsx__pasti__tidak__ada__nsx::fungsi", vm);
        assert!(
            !terdeteksi_ada,
            "penjaga instrumen: nama yang pasti tidak ada TIDAK terdeteksi hilang di {}. \
             Daftar `HILANG` kemungkinan basi — kalau begitu seluruh test ini hijau tanpa \
             memeriksa apa pun.\npesan: {}",
            if vm { "vm" } else { "interp" }, pesan.trim()
        );
    }

    let mut gagal = Vec::new();
    for n in &nama {
        let (di_i, pesan_i) = ada(n, false);
        let (di_v, pesan_v) = ada(n, true);
        if di_i == di_v { continue; }
        let (arah, pesan) = if di_v { ("cuma di VM", pesan_i) } else { ("cuma di interpreter", pesan_v) };
        if SEBAB.iter().any(|s| pesan.contains(s)) { continue; }
        gagal.push(format!(
            "  {} -> {}. Kalau memang tidak bisa dijembatani, tolak di sisi \
             yang tidak punya dengan pesan yang MENYEBUT SEBABNYA — jangan \
             biarkan ia terbaca seperti salah ketik.\n     pesannya: {}",
            n, arah, pesan.lines().next().unwrap_or("").trim()
        ));
    }
    assert!(
        gagal.is_empty(),
        "{} builtin cuma hidup di satu backend (dari {} yang dipanen):\n{}",
        gagal.len(), nama.len(), gagal.join("\n")
    );
}

/// Keluaran `json::` harus sama di antara JALAN yang berbeda, bukan cuma di
/// antara dua backend.
#[test]
fn keluaran_json_sama_di_setiap_jalan() {
    const SRC: &str = "package main\npub fn main() { \
        let d = json::parse(\"{\\\"z\\\":1,\\\"a\\\":2,\\\"m\\\":3,\\\"b\\\":4}\"); \
        wok(json::stringify(d)); wok(json::keys(d)); wok(json::values(d)); \
        wok(json::pretty(d)) }";
    for vm in [false, true] {
        let pertama = jalankan(SRC, vm);
        assert!(pertama.sukses, "program gagal:\n{}", pertama.keluaran);
        for i in 2..=6 {
            let lagi = jalankan(SRC, vm);
            assert_eq!(
                pertama.keluaran, lagi.keluaran,
                "keluaran `json::` berubah antar-jalan (backend {}, jalan ke-{}). \
                 Kemungkinan besar ada iterasi `HashMap` yang lolos tanpa diurutkan — \
                 urutannya diacak ulang tiap proses, jadi ini TIDAK akan konsisten \
                 gagal dan tidak akan terlihat di `PARITAS`.",
                if vm { "vm" } else { "interp" }, i
            );
        }
    }
}

/// Cetak ukuran tiap tabel.
#[test]
fn ukuran_tabel_dilaporkan() {
    eprintln!(
        "PARITAS={} DITOLAK_DUA_BACKEND={} VM_LEBIH_PERMISIF={} \
         DIVERGENSI_DIKETAHUI={} BELUM_DIDUKUNG_VM={} KONFORMANS_DUA_BACKEND={}",
        PARITAS.len(), DITOLAK_DUA_BACKEND.len(), VM_LEBIH_PERMISIF.len(),
        DIVERGENSI_DIKETAHUI.len(), BELUM_DIDUKUNG_VM.len(), KONFORMANS_DUA_BACKEND.len()
    );
}

// ════════════════════════════════════════════════════════════════════════
// `dok::serve` — server HTTP yang sama, dijalankan dua backend …
mod serve {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    /// Anak proses yang dibunuh saat keluar dari scope — sebuah server yang
    /// bocor akan menahan portnya dan membuat uji BERIKUTNYA gagal, di
    /// tempat yang tidak ada hubungannya.
    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }

    /// Port dari `bind(0)` dilepas sebelum anaknya mengikat, jadi tes lain bisa menyambarnya: anak
    /// yang mati sebelum siap dicoba lagi di port baru, dan pelayan yang menjawab harus anak KITA.
    fn ulang_port<T>(mut mulai: impl FnMut(u16) -> (Server, T)) -> (Server, u16, T) {
        for _ in 0..5 {
            let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            let (mut anak, ikut) = mulai(port);
            let batas = Instant::now() + Duration::from_secs(20);
            while Instant::now() < batas && anak.0.try_wait().ok().flatten().is_none() {
                if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    std::thread::sleep(Duration::from_millis(100));
                    if anak.0.try_wait().ok().flatten().is_none() { return (anak, port, ikut); }
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        panic!("pelayan uji tidak pernah siap di lima port berturut-turut");
    }

    /// Dua port yang PASTI berbeda.
    fn dua_port_bebas() -> (u16, u16) {
        let a = TcpListener::bind("127.0.0.1:0").unwrap();
        let b = TcpListener::bind("127.0.0.1:0").unwrap();
        let pa = a.local_addr().unwrap().port();
        let pb = b.local_addr().unwrap().port();
        drop(a);
        drop(b);
        (pa, pb)
    }

    /// Celah antara melepas port dan server mengikatnya tidak bisa ditutup dari
    /// sini — proses lain boleh menyambarnya.
    fn jalankan_ulet(vm: bool) -> Option<Vec<String>> {
        for _ in 0..3 {
            let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            if let Some(h) = jalankan(vm, p) {
                return Some(h);
            }
        }
        None
    }

    const PROGRAM: &str = r#"package main
var SALAM = "hai"
fn kali(n) -> int { return n * 3 }
fn root(req) { return dok::text(SALAM + " " + kali(4) as string) }
fn ident(req) { return dok::text("id=" + req.params["id"]) }
fn nf(req) { return dok::text("hilang") }
pub fn main() {
  let r = dok::router_new()
  dok::router_get(r, "/", root)
  dok::router_post(r, "/", root)
  dok::router_get(r, "/u/:id", ident)
  dok::router_get(r, "/tutup", |req| dok::text("closure"))
  dok::router_not_found(r, nf)
  dok::serve(r, PORT)
}
"#;

    /// Satu permintaan HTTP mentah. Tanpa pustaka klien: yang diuji justru
    /// bytes di kawat, jadi merakitnya sendiri lebih jujur.
    fn minta(port: u16, metode: &str, jalur: &str) -> Option<String> {
        let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        let req = format!(
            "{} {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            metode, jalur
        );
        s.write_all(req.as_bytes()).ok()?;
        let mut buf = String::new();
        s.read_to_string(&mut buf).ok()?;
        // Header dibuang: `Date` berubah tiap detik, jadi membandingkannya
        // akan membuat uji ini gagal karena waktu, bukan karena backend.
        Some(buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string())
    }

    fn jalankan(vm: bool, port: u16) -> Option<Vec<String>> {
        let dir = std::env::temp_dir().join(format!("jrg-serve-{}", port));
        std::fs::create_dir_all(&dir).ok()?;
        let berkas = dir.join("srv.jrg");
        std::fs::write(&berkas, PROGRAM.replace("PORT", &port.to_string())).ok()?;

        let mut cmd = Command::new(jrgc());
        cmd.arg(&berkas);
        if vm { cmd.arg("--vm"); }
        cmd.env("JRG_SOPAN", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let anak = Server(cmd.spawn().ok()?);

        // Tunggu sampai portnya benar-benar menerima koneksi, bukan tidur sekian
        // detik dan berharap: mesin CI yang lambat akan membuat tidur-tetap gagal …
        let batas = Instant::now() + Duration::from_secs(20);
        while Instant::now() < batas {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() { break; }
            std::thread::sleep(Duration::from_millis(50));
        }

        let hasil = vec![
            minta(port, "GET", "/")?,
            minta(port, "POST", "/")?,
            minta(port, "GET", "/u/77")?,
            minta(port, "GET", "/tutup")?,
            minta(port, "GET", "/tidak-ada")?,
        ];
        drop(anak);
        Some(hasil)
    }

    #[test]
    fn server_menjawab_sama_di_kedua_backend() {
        let interp = jalankan_ulet(false).expect("server interpreter tidak menjawab");
        let vm = jalankan_ulet(true).expect("server --vm tidak menjawab");

        // Isi yang benar diperiksa lebih dulu. Dua backend yang sama-sama
        // menjawab kosong juga "identik", dan itu akan lolos kalau yang
        // dibandingkan cuma keduanya satu sama lain.
        assert_eq!(interp[0], "hai 12", "handler memanggil fn + baca var modul");
        assert_eq!(interp[2], "id=77", "parameter route");
        assert_eq!(interp[3], "closure", "handler berupa closure");
        assert_eq!(interp[4], "hilang", "handler 404");

        for (i, (a, b)) in interp.iter().zip(vm.iter()).enumerate() {
            assert_eq!(a, b, "respons #{} berbeda antar-backend", i);
        }
    }

    /// Satu permintaan tidak boleh mematikan server: `Content-Length: 1099511627776` dulu membuat
    /// prosesnya memesan satu terabyte lalu abort, di dua mesin.
    #[test]
    fn permintaan_jahat_tidak_mematikan_server_di_dua_mesin() {
        for vm in [false, true] {
            let (anak, port, dir) = ulang_port(|port| {
                let dir = std::env::temp_dir().join(format!("jrg-serve-jahat-{}", port));
                std::fs::create_dir_all(&dir).unwrap();
                let berkas = dir.join("s.jrg");
                std::fs::write(&berkas, format!(
                    "package main\npub fn main() {{ let r = dok::router_new()\n \
                     dok::router_post(r, \"/gema\", |req| dok::text(str(len(req.body))))\n dok::serve(r, {}) }}\n", port)).unwrap();
                let mut cmd = Command::new(jrgc());
                cmd.arg(&berkas).env("JRG_SOPAN", "1")
                    .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                if vm { cmd.arg("--vm"); }
                (Server(cmd.spawn().unwrap()), dir)
            });
            let status = |kepala: &str| -> String {
                let mut s = TcpStream::connect(("127.0.0.1", port)).expect("server sudah mati");
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                write!(s, "POST /gema HTTP/1.1\r\n{}\r\nabc", kepala).unwrap();
                let mut buf = String::new();
                let _ = s.read_to_string(&mut buf);
                let badan = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                format!("{} {}", buf.split(' ').nth(1).unwrap_or("?"), badan)
            };
            let hasil = [
                status("Content-Length: 9223372036854775813\r\n"),
                status("Content-Length: 1099511627776\r\n"),
                status("Content-Length: abc\r\n"),
                status("Content-Length: 1\r\nContent-Length: 3\r\n"),
                status(&"X: 1\r\n".repeat(101)),
                status("Content-Length: 3\r\n"),
            ];
            drop(anak);
            let _ = std::fs::remove_dir_all(&dir);
            assert_eq!(hasil, ["413 Payload Too Large", "413 Payload Too Large", "400 Bad Request", "400 Bad Request",
                               "431 Request Header Fields Too Large", "200 3"], "vm={}", vm);
        }
    }

    /// Klien `http::`: badan berpotongan (chunked) diurai, `http::unduh` mengikuti pengalihan ke berkas,
    /// dan jawaban bukan 2xx tidak menulis berkas — dua mesin, dari server lokal yang ditulis di sini.
    #[test]
    fn klien_berpotongan_pengalihan_dan_unduh_di_dua_mesin() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let jalur = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let jawab: Vec<u8> = match jalur.as_str() {
                    "/potong" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n".to_vec(),
                    "/alih" => b"HTTP/1.1 302 Found\r\nLocation: /isi\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    "/isi" => { let mut v = b"HTTP/1.1 200 OK\r\nContent-Length: 300\r\n\r\n".to_vec(); v.extend((0..300u32).map(|i| (i % 256) as u8)); v }
                    _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 3\r\n\r\nno\n".to_vec(),
                };
                let _ = s.write_all(&jawab);
            }
        });
        let dir = std::env::temp_dir().join(format!("jrg-klien-{}", port));
        std::fs::create_dir_all(&dir).unwrap();
        let u = format!("http://127.0.0.1:{}", port);
        let program = format!(r#"pub fn main() {{
    wok(http::get("{u}/potong").unwrap()["body"])
    wok(dok::http_get("{u}/potong").unwrap().body)
    let h = http::unduh("{u}/alih", "{d}/b.bin").unwrap()
    wok(h["status"], h["ukuran"], h["url"], fs::size("{d}/b.bin"), fs::exists("{d}/b.bin.sebagian"))
    match http::unduh("{u}/hilang", "{d}/c.bin") {{ Ok(_) => wok("salah"), Err(e) => wok(str::contains(e, "404"), fs::exists("{d}/c.bin")) }}
}}
"#, u = u, d = dir.display());
        let berkas = dir.join("k.jrg");
        std::fs::write(&berkas, program).unwrap();
        for vm in [false, true] {
            let _ = std::fs::remove_file(dir.join("b.bin"));
            let mut c = Command::new(jrgc());
            c.arg(&berkas).env("JRG_SOPAN", "1");
            if vm { c.arg("--vm"); }
            let o = c.output().unwrap();
            let k = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
            assert_eq!(k, format!("Wikipedia\nWikipedia\n200 300 {}/isi 300 false\ntrue false\n", u), "vm={}", vm);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// HTTPS lewat libssl: sertifikat buatan sendiri dari `openssl s_server` lokal DITOLAK dengan sebabnya.
    #[test]
    fn https_menolak_sertifikat_yang_tidak_dipercaya() {
        let ada = |p: &str| std::env::var_os("PATH").map_or(false, |x| std::env::split_paths(&x).any(|d| d.join(p).is_file()));
        if !ada("openssl") { eprintln!("dilewati: tanpa openssl"); return; }
        let dir = std::env::temp_dir().join(format!("jrg-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = Command::new("openssl").args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost", "-days", "1",
            "-keyout"]).arg(dir.join("k.pem")).arg("-out").arg(dir.join("c.pem")).output().unwrap().status.success();
        assert!(ok, "openssl req gagal");
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let _srv = Server(Command::new("openssl").args(["s_server", "-quiet", "-www", "-accept", &port.to_string(), "-cert"])
            .arg(dir.join("c.pem")).arg("-key").arg(dir.join("k.pem"))
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap());
        let batas = Instant::now() + Duration::from_secs(10);
        while Instant::now() < batas && TcpStream::connect(("127.0.0.1", port)).is_err() { std::thread::sleep(Duration::from_millis(50)); }
        let berkas = dir.join("t.jrg");
        std::fs::write(&berkas, format!("pub fn main() {{\n    match http::get(\"https://localhost:{}/\") {{ Ok(_) => wok(\"diterima\"), Err(e) => wok(e) }}\n}}\n", port)).unwrap();
        for vm in [false, true] {
            let mut c = Command::new(jrgc());
            c.arg(&berkas).env("JRG_SOPAN", "1");
            if vm { c.arg("--vm"); }
            let k = String::from_utf8_lossy(&c.output().unwrap().stdout).to_string();
            if k.contains("butuh libssl") { eprintln!("dilewati: tanpa libssl"); return; }
            assert!(k.contains("sertifikat localhost ditolak") && k.contains("self-signed"), "vm={}: {}", vm, k);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Berkas statis yang BUKAN teks sampai byte demi byte. Dulu `dok::router_static` memaksanya
    /// jadi teks UTF-8: font 9.968 byte dilayani sebagai 18.006 byte, dan tidak ada yang bilang.
    #[test]
    fn berkas_statis_biner_utuh_di_dua_mesin() {
        let isi: Vec<u8> = (0..1024u32).map(|i| (i % 256) as u8).collect();
        for vm in [false, true] {
            let (anak, port, dir) = ulang_port(|port| {
                let dir = std::env::temp_dir().join(format!("jrg-serve-biner-{}", port));
                std::fs::create_dir_all(dir.join("aset")).unwrap();
                std::fs::write(dir.join("aset/b.bin"), &isi).unwrap();
                std::fs::write(dir.join("aset/t.txt"), "teks biasa é").unwrap();
                let berkas = dir.join("s.jrg");
                std::fs::write(&berkas, format!(
                    "package main\npub fn main() {{ let r = dok::router_new()\n \
                     dok::router_static(r, \"/s\", \"{}\")\n dok::serve(r, {}) }}\n",
                    dir.join("aset").display(), port)).unwrap();
                let mut cmd = Command::new(jrgc());
                cmd.arg(&berkas).env("JRG_SOPAN", "1")
                    .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                if vm { cmd.arg("--vm"); }
                (Server(cmd.spawn().unwrap()), dir)
            });
            let mentah = |jalur: &str| -> Vec<u8> {
                let mut s = TcpStream::connect(("127.0.0.1", port)).expect("sambung");
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                write!(s, "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", jalur).unwrap();
                let mut buf = Vec::new();
                s.read_to_end(&mut buf).unwrap();
                let awal = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(0);
                buf[awal..].to_vec()
            };
            let (biner, teks) = (mentah("/s/b.bin"), mentah("/s/t.txt"));
            drop(anak);
            let _ = std::fs::remove_dir_all(&dir);
            assert_eq!(biner.len(), isi.len(), "vm={}: panjang berkas biner berubah", vm);
            assert!(biner == isi, "vm={}: isi berkas biner berubah", vm);
            assert_eq!(teks, "teks biasa é".as_bytes(), "vm={}: berkas teks", vm);
        }
    }

    /// Koneksi tidak boleh saling memblokir. Diukur lewat WAKTU, karena itu
    /// satu-satunya cara membedakan "empat jawaban benar" yang konkuren dari
    /// yang terserial — keduanya menghasilkan isi yang sama persis.
    #[test]
    fn koneksi_tidak_saling_memblokir_di_vm() {
        let (anak, port, _dir) = ulang_port(|port| {
            let dir = std::env::temp_dir().join(format!("jrg-serve-par-{}", port));
            std::fs::create_dir_all(&dir).unwrap();
            let berkas = dir.join("lambat.jrg");
            std::fs::write(&berkas, format!(
                "package main\nfn lambat(req) {{ time::sleep_ms(700)\n return dok::text(\"ok\") }}\n\
                 pub fn main() {{ let r = dok::router_new()\n dok::router_get(r, \"/l\", lambat)\n \
                 dok::serve(r, {}) }}\n", port)).unwrap();
            (Server(Command::new(jrgc())
                .arg(&berkas).arg("--vm").env("JRG_SOPAN", "1")
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .spawn().unwrap()), dir)
        });

        // Satu permintaan percobaan dulu, DI LUAR pengukuran: kalau server ternyata
        // belum menerima koneksi, mengukur empat permintaan paralel hanya …
        if minta(port, "GET", "/l").is_none() {
            drop(anak);
            eprintln!("dilewati: server tidak menjawab permintaan pemanasan");
            return;
        }

        let mulai = Instant::now();
        let utas: Vec<_> = (0..4).map(|_| std::thread::spawn(move || minta(port, "GET", "/l"))).collect();
        let jawab: Vec<_> = utas.into_iter().map(|u| u.join().unwrap()).collect();
        let lama = mulai.elapsed();
        drop(anak);

        for (i, j) in jawab.iter().enumerate() {
            assert_eq!(j.as_deref(), Some("ok"), "permintaan paralel #{} gagal", i);
        }
        // Serial = ~2800 ms, konkuren = ~700 ms. Ambangnya 2000 ms: cukup
        // longgar untuk mesin lambat, cukup ketat untuk menangkap serialisasi.
        assert!(lama < Duration::from_millis(2000),
            "4 handler 700ms selesai dalam {:?} — terserial, bukan konkuren", lama);
    }

    /// Handler yang gagal meninggalkan jejak di stderr PELAYAN, bukan cuma di badan 500:
    /// klien yang tidak membaca badannya dulu tidak meninggalkan apa pun.
    #[test]
    fn handler_yang_gagal_tercatat_di_stderr_pelayan() {
        let mut baris: Vec<String> = Vec::new();
        for vm in [false, true] {
            let (anak, port, dir) = ulang_port(|port| {
                let dir = std::env::temp_dir().join(format!("jrg-serve-galat-{}", port));
                std::fs::create_dir_all(&dir).unwrap();
                let berkas = dir.join("gagal.jrg");
                std::fs::write(&berkas, format!(
                    "package main\npub fn main() {{ let r = dok::router_new()\n \
                     dok::router_get(r, \"/x\", |req| [1][5])\n dok::serve(r, {}) }}\n", port)).unwrap();
                let catatan = dir.join("stderr.txt");
                let mut c = Command::new(jrgc());
                c.arg(&berkas).env("JRG_SOPAN", "1")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::fs::File::create(&catatan).unwrap());
                if vm { c.arg("--vm"); }
                (Server(c.spawn().unwrap()), dir)
            });
            let catatan = dir.join("stderr.txt");
            let _ = minta(port, "GET", "/x");
            let mut isi = String::new();
            let batas = Instant::now() + Duration::from_secs(5);
            while Instant::now() < batas && !isi.contains("gagal di handler") {
                std::thread::sleep(Duration::from_millis(50));
                isi = std::fs::read_to_string(&catatan).unwrap_or_default();
            }
            drop(anak);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(isi.contains("dok::serve: GET /x gagal di handler"),
                "galat handler (vm={}) tidak tercatat di stderr pelayan:\n{}", vm, isi);
            baris.push(isi.lines().find(|b| b.contains("gagal di handler")).unwrap_or("").to_string());
        }
        // Satu kalimat untuk dua mesin: awalan `runtime error: ` dulu cuma muncul di interpreter.
        assert!(baris[0] == baris[1] && !baris[0].contains("runtime error"), "{:?}", baris);
    }
}

/// Lapisan web: middleware, cookie, sesi, kata sandi, template.
mod web {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }

    const PROGRAM: &str = r#"package main
const KUNCI = "kunci-uji-yang-cukup-panjang-sekali"

fn jejak(req) {
    // Menambah ladang ke request: handler harus melihatnya.
    req.jejak = "lewat-mw"
    return req
}

fn jaga(req) {
    if str::starts_with(req.path, "/rahasia") {
        return dok::response(401, "perlu masuk")
    }
    return nil
}

fn tandai(req, resp) {
    return dok::header_set(resp, "X-Uji", "ada")
}

fn root(req) { return dok::text("jejak=" + req.jejak) }
fn rahasia(req) { return dok::text("TIDAK BOLEH TERLIHAT") }

fn tpl(req) {
    let d = {"nama": "<b>budi</b>", "xs": [10, 20]}
    match dok::render("halo {{nama}}|{{& nama}}|{{#each xs}}[{{.}}:{{@index}}]{{/each}}", d) {
        Ok(h) => { return dok::text(h) }
        Err(e) => { return dok::response(500, e) }
    }
}

fn pasang(req) {
    let s = dok::session_encode({"user": "budi"}, KUNCI)
    return dok::cookie_set(dok::text("dipasang"), "sesi", s, {"max_age": 60})
}

fn baca(req) {
    match dok::cookie_get(req, "sesi") {
        Some(c) => {
            match dok::session_decode(c, KUNCI) {
                Ok(d) => { return dok::text("user=" + d.user) }
                Err(e) => { return dok::text("rusak") }
            }
        }
        None => { return dok::text("tidak ada cookie") }
    }
}

fn sandi(req) {
    let h = dok::password_hash("abc", 1000)
    return dok::text("benar=" + dok::password_verify("abc", h) as string
        + " salah=" + dok::password_verify("xyz", h) as string
        + " rusak=" + dok::password_verify("abc", "bukan-hash") as string)
}

pub fn main() {
  let r = dok::router_new()
  dok::router_use(r, jejak)
  dok::router_use(r, jaga)
  dok::router_after(r, tandai)
  dok::router_get(r, "/", root)
  dok::router_get(r, "/rahasia", rahasia)
  dok::router_get(r, "/tpl", tpl)
  dok::router_get(r, "/pasang", pasang)
  dok::router_get(r, "/baca", baca)
  dok::router_get(r, "/sandi", sandi)
  dok::serve(r, PORT)
}
"#;

    /// Satu permintaan mentah. Header IKUT dikembalikan (kecuali `Date`, yang
    /// berubah tiap detik) — `Set-Cookie` dan header dari middleware `sesudah`
    /// justru yang sedang diuji.
    fn minta(port: u16, jalur: &str, cookie: Option<&str>) -> Option<String> {
        let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        let ck = cookie.map(|c| format!("Cookie: {}\r\n", c)).unwrap_or_default();
        let req = format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n{}Connection: close\r\n\r\n", jalur, ck);
        s.write_all(req.as_bytes()).ok()?;
        let mut buf = String::new();
        s.read_to_string(&mut buf).ok()?;
        Some(buf.lines().filter(|l| !l.starts_with("Date:")).collect::<Vec<_>>().join("\n"))
    }

    fn jalankan(vm: bool, port: u16) -> Option<Vec<String>> {
        let dir = std::env::temp_dir().join(format!("jrg-web-{}-{}", port, std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let berkas = dir.join("web.jrg");
        std::fs::write(&berkas, PROGRAM.replace("PORT", &port.to_string())).ok()?;

        let mut cmd = Command::new(jrgc());
        cmd.arg(&berkas);
        if vm { cmd.arg("--vm"); }
        cmd.env("JRG_SOPAN", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let anak = Server(cmd.spawn().ok()?);

        let batas = Instant::now() + Duration::from_secs(20);
        while Instant::now() < batas {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() { break; }
            std::thread::sleep(Duration::from_millis(50));
        }

        // Cookie diambil dari respons `/pasang` lalu dikirim balik ke `/baca`:
        // memakai cookie yang dikarang di sisi tes akan menguji tanda
        // tangannya dengan kunci yang tidak pernah dipakai server.
        let pasang = minta(port, "/pasang", None)?;
        let sesi = pasang.lines()
            .find(|l| l.to_lowercase().starts_with("set-cookie:"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().split(';').next().unwrap_or("").to_string())
            .unwrap_or_default();

        let hasil = vec![
            minta(port, "/", None)?,
            minta(port, "/rahasia", None)?,
            minta(port, "/tpl", None)?,
            pasang,
            minta(port, "/baca", Some(&sesi))?,
            minta(port, "/baca", Some("sesi=palsu.palsu"))?,
            minta(port, "/baca", None)?,
            minta(port, "/sandi", None)?,
        ];
        drop(anak);
        Some(hasil)
    }

    fn jalankan_ulet(vm: bool) -> Option<Vec<String>> {
        for _ in 0..5 {
            let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            if let Some(h) = jalankan(vm, p) { return Some(h); }
        }
        None
    }

    #[test]
    fn lapisan_web_sama_di_kedua_backend() {
        let interp = jalankan_ulet(false).expect("server interpreter tidak menjawab");
        let vm = jalankan_ulet(true).expect("server vm tidak menjawab");

        // Isi diperiksa DULU. Dua backend yang sama-sama tidak menjawab apa
        // pun juga "identik", jadi membandingkan saja tidak membuktikan apa-apa.
        assert!(interp[0].contains("jejak=lewat-mw"),
            "middleware yang mengubah request tidak sampai ke handler:\n{}", interp[0]);
        assert!(interp[1].contains("401") && !interp[1].contains("TIDAK BOLEH"),
            "middleware yang menolak tidak menghentikan handler:\n{}", interp[1]);
        assert!(interp[0].contains("X-Uji: ada"),
            "middleware `sesudah` tidak menambah header:\n{}", interp[0]);
        assert!(interp[1].contains("X-Uji: ada"),
            "middleware `sesudah` tidak jalan untuk respons yang dipotong:\n{}", interp[1]);
        // Template: diloloskan secara bawaan, mentah kalau diminta.
        assert!(interp[2].contains("&lt;b&gt;budi&lt;/b&gt;"),
            "template tidak meloloskan HTML:\n{}", interp[2]);
        assert!(interp[2].contains("|<b>budi</b>|"),
            "`{{& }}` tidak mengeluarkan HTML mentah:\n{}", interp[2]);
        assert!(interp[2].contains("[10:0][20:1]"), "each/@index salah:\n{}", interp[2]);
        // Cookie + sesi bolak-balik lewat kawat.
        assert!(interp[3].to_lowercase().contains("set-cookie:")
                && interp[3].contains("HttpOnly") && interp[3].contains("SameSite=Lax"),
            "cookie tidak dipasang dengan atribut amannya:\n{}", interp[3]);
        assert!(interp[4].contains("user=budi"), "sesi tidak terbaca kembali:\n{}", interp[4]);
        assert!(interp[5].contains("rusak"), "sesi palsu diterima:\n{}", interp[5]);
        assert!(interp[6].contains("tidak ada cookie"),
            "cookie yang tidak ada tidak dibedakan:\n{}", interp[6]);
        assert!(interp[7].contains("benar=true") && interp[7].contains("salah=false")
                && interp[7].contains("rusak=false"),
            "verifikasi sandi salah:\n{}", interp[7]);

        for (i, (a, b)) in interp.iter().zip(vm.iter()).enumerate() {
            // Cookie sesi memuat tanda tangan yang sama (kuncinya tetap), tapi
            // `Set-Cookie` di respons `/pasang` identik juga — tidak ada
            // sumber acak di dalamnya.
            assert_eq!(a, b, "respons ke-{} berbeda antar backend", i);
        }
    }
}

/// "Modul tidak ketemu": satu kalimat di dua backend, tiap tempat disebut
/// sekali, dan direktori berkas yang ditulis tanpa path tampil sebagai `.`.
#[test]
fn modul_tak_ketemu_satu_kalimat_tanpa_tempat_ganda() {
    let dir = std::env::temp_dir().join(format!("jrg-modul-tak-ketemu-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).expect("bikin dir sementara");
    let program = "package main\nimport \"hantu\"\npub fn main() { wok(1) }\n";
    std::fs::write(dir.join("utama.jrg"), program).expect("tulis sumber");
    std::fs::write(dir.join("src").join("utama.jrg"), program).expect("tulis sumber");
    let galat = |berkas: &str, vm: bool| -> String {
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).args([berkas, "-I", "src", "--sopan"]);
        if vm { cmd.arg("--vm"); }
        String::from_utf8_lossy(&jalankan_berbatas(cmd).stderr).to_string()
    };
    for (berkas, daftar) in [("src/utama.jrg", "[src]"), ("utama.jrg", "[., src]")] {
        let i = galat(berkas, false);
        assert!(i.contains(&format!("Yang sudah dicari: {}", daftar)),
            "{}: daftar tempat yang dicari salah: {:?}", berkas, i);
        assert_eq!(i, galat(berkas, true), "{}: dua backend memakai kalimat berbeda", berkas);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `proses::kill` menghentikan TEPAT pid yang disebut, di dua backend.
#[test]
fn proses_kill_menghentikan_proses_yang_dituju() {
    if cfg!(windows) { return; }
    for vm in [false, true] {
        let mut anak = Command::new("sleep").arg("30").spawn().expect("jalankan sleep");
        let pid = anak.id();
        let src = format!("package main\npub fn main() {{ wok(proses::kill({})) }}\n", pid);
        let keluar = jalankan_keluaran_penuh(&src, vm);
        // Ditunggu berbatas: sinyal yang salah alamat membuat tes ini merah, bukan menggantung.
        let mut berhenti = false;
        for _ in 0..100 {
            if matches!(anak.try_wait(), Ok(Some(_))) { berhenti = true; break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !berhenti { let _ = anak.kill(); let _ = anak.wait(); }
        assert!(keluar.trim() == "true" && berhenti,
            "proses::kill({}) di {} tidak menghentikan proses yang dituju (keluaran: {:?})",
            pid, if vm { "vm" } else { "interpreter" }, keluar);
    }
}

/// **Tidak ada builtin yang boleh MEMATIKAN PROSES.**
#[test]
fn tidak_ada_builtin_yang_mematikan_proses() {
    // Namespace yang sengaja dilewati, dengan alasannya masing-masing: -
    // `dok::serve` dan `chan::`/`Chan::` MEMBLOKIR by design; - `py::`/`js::`/`php::` …
    const LEWATI: &[&str] = &[
        "dok::", "py::", "js::", "php::", "lua::", "chan::", "Chan::", "future::", "task::",
        "Future::", "Task::",
        "canvas::", "Canvas::", "std::", "time::", "http::", "fs::", "file::",
        "env::", "jawa::", "jawir::", "ptr::",
        // Mengirim sinyal ke pid SUNGGUHAN: `proses::kill(<i64 terbesar>)` pernah
        // terpotong jadi `kill -1` dan menghentikan seluruh sesi pemakai.
        "proses::",
        // Membunyikan speaker sungguhan.
        "suara::putar",
    ];
    // Bentuk argumen yang salah.
    const BENTUK: &[&str] = &[
        "", "5", "5, 5", "5, 5, 5", "[], []", "\"\", \"\"",
        "nil", "-1", "-1, -1", "9223372036854775807",
        "-9223372036854775808, -1", "\"\"", "[]", "[], -1", "{}", "true",
        "1.5", "[1,2], 99", "\"a\", -1, -1",
        // Ekstrem i128: rentang penuh pernah membuat `rand::int` membagi dengan nol.
        "-170141183460469231731687303715884105728, 170141183460469231731687303715884105727",
        "170141183460469231731687303715884105727",
    ];

    // `exit` memang mengakhiri prosesnya, dan `sleep` memang menunggu — dengan
    // angka raksasa ia menunggu sampai batas waktu tiap bentuk.
    const LEWATI_TELANJANG: &[&str] = &["exit", "sleep"];

    let nama = jrgc::vm::daftar_builtin();
    assert!(nama.len() > 200, "daftar builtin sepertinya rusak: {}", nama.len());

    let mut telanjang = 0;
    let mut mati = Vec::new();
    for n in &nama {
        // Nama TELANJANG dulu dilewati seluruhnya, dan di situ `range` bersembunyi:
        // bentuk-bentuk di atas menemukan 12 kematian proses di `range` dan
        // `range_inclusive` begitu nama telanjang ikut disapu.
        if LEWATI.iter().any(|l| n.starts_with(l)) || LEWATI_TELANJANG.contains(&n.as_str()) {
            continue;
        }
        telanjang += usize::from(!n.contains("::"));
        for b in BENTUK {
            let src = format!("package main\npub fn main() {{ wok({}({})) }}\n", n, b);
            for vm in [false, true] {
                let keluar = jalankan_keluaran_penuh(&src, vm);
                // Abort alokasi Rust tidak mencetak `panicked at`, cuma
                // `memory allocation of N bytes failed`, lalu prosesnya mati.
                if keluar.contains("panicked at") || keluar.contains("memory allocation of") {
                    mati.push(format!(
                        "  {}({}) di {} — {}", n, b, if vm { "vm" } else { "interp" },
                        keluar.lines()
                            .find(|l| l.contains("panicked") || l.contains("memory allocation of"))
                            .unwrap_or("").trim()));
                }
            }
        }
    }
    // Lantai: kalau nama telanjang berhenti ikut tersapu, butanya kembali tanpa suara.
    assert!(telanjang >= 40, "cuma {} nama telanjang tersapu", telanjang);
    assert!(
        mati.is_empty(),
        "{} pemanggilan builtin MEMATIKAN PROSES alih-alih memberi diagnostik:\n{}",
        mati.len(), mati.join("\n"));
}

/// Nama builtin yang DITERIMA satu backend dan DITOLAK yang lain.
#[test]
fn asimetri_terima_tolak_terpantau() {
    // Namespace yang dilewati, dengan alasan yang sama seperti di
    // `tidak_ada_builtin_yang_mematikan_proses`: memblokir, memakai proses
    // anak, atau memang mematikan proses.
    const LEWATI: &[&str] = &[
        "dok::", "py::", "js::", "php::", "lua::", "chan::", "Chan::", "future::", "task::",
        "Future::", "Task::",
        "canvas::", "Canvas::", "std::", "time::", "http::", "fs::", "file::",
        "env::",
        // `jvm::`, `kotlin::` dan `dotnet::` MENYALAKAN sebuah runtime penuh di
        // dalam prosesnya — `kotlin::` lewat JVM yang sama.
        "jvm::", "kotlin::", "dotnet::",
        // Mengirim sinyal dan menjalankan program sungguhan; lihat sapuan di atas.
        "proses::",
        // Membunyikan speaker sungguhan.
        "suara::putar",
    ];
    // Bentuk argumennya, dan tiga yang terakhir adalah KOREKSI.
    const BENTUK: &[&str] = &[
        "5", "5, 5", "5, 5, 5",
        "[1,2,3]", "\"halo\"", "[1,2,3], |x| x",
    ];

    // Angka yang TERUKUR saat instrumen ini ditulis, bukan target.
    const DIKETAHUI: usize = 0;

    let nama = jrgc::vm::daftar_builtin();
    assert!(nama.len() > 200, "daftar builtin sepertinya rusak: {}", nama.len());

    // Sebuah perbedaan DIULANG sebelum dicatat, dan itu bukan kehati-hatian
    // melainkan koreksi atas cacat di instrumen ini sendiri.
    let periksa = |n: &str, b: &str| -> (bool, bool) {
        let src = format!("package main\npub fn main() {{ wok({}({})) }}\n", n, b);
        (jalankan_keluaran_penuh(&src, false).contains("error"),
         jalankan_keluaran_penuh(&src, true).contains("error"))
    };

    let mut beda: Vec<String> = Vec::new();
    for n in &nama {
        if !n.contains("::") || LEWATI.iter().any(|l| n.starts_with(l)) { continue; }
        for b in BENTUK {
            let (i_tolak, v_tolak) = periksa(n, b);
            if i_tolak == v_tolak { continue; }
            let (i2, v2) = periksa(n, b);
            if i2 == v2 {
                eprintln!(
                    "CATATAN: `{}({})` berbeda sekali lalu tidak berulang — \
                     kemungkinan besar sebuah proses yang dibunuh batas waktu, \
                     bukan asimetri. Tidak dihitung.", n, b);
                continue;
            }
            beda.push(format!("  {}({}) — interp {}, vm {}", n, b,
                if i2 { "tolak" } else { "TERIMA" },
                if v2 { "tolak" } else { "TERIMA" }));
            break;
        }
    }

    if beda.len() > DIKETAHUI {
        panic!(
            "asimetri terima/tolak NAIK dari {} ke {} — sebuah nama baru \
             diterima satu backend dan ditolak yang lain:\n{}",
            DIKETAHUI, beda.len(), beda.join("\n"));
    }
    // Arah sebaliknya juga dilaporkan, dan itu bukan hiasan: sebuah angka yang
    // cuma bisa naik akan diam-diam jadi usang begitu seseorang menutup sesuatu
    // tanpa memperbaruinya, dan sejak itu ia berhenti mengukur apa pun.
    #[allow(clippy::absurd_extreme_comparisons)]
    {
        assert!(
            beda.len() >= DIKETAHUI,
            "KABAR BAIK: asimetri turun dari {} ke {}. Perbarui `DIKETAHUI` di sini \
             dan catatannya di catatan proyek.\nyang tersisa:\n{}",
            DIKETAHUI, beda.len(), beda.join("\n"));
    }
}

/// Bentuk **method** (`s.X(...)`) harus sepakat dengan bentuk **namespace**
/// (`str::X(s, ...)`), di kedua backend.
#[test]
fn bentuk_method_sepakat_dengan_bentuk_namespace() {
    // Penerima dan argumen yang MASUK AKAL untuk tiap nama.
    let kasus: &[(&str, &str, &str)] = &[
        // (nama, penerima, argumen)
        ("upper",        "\"halo\"",        ""),
        ("lower",        "\"HALO\"",        ""),
        ("trim",         "\"  a  \"",       ""),
        ("len",          "\"halo\"",        ""),
        ("reverse",      "\"abc\"",         ""),
        ("contains",     "\"halo\"",        "\"al\""),
        ("starts_with",  "\"halo\"",        "\"ha\""),
        ("ends_with",    "\"halo\"",        "\"lo\""),
        ("split",        "\"a,b\"",         "\",\""),
        ("replace",      "\"aaa\"",         "\"a\", \"b\""),
        ("repeat",       "\"ab\"",          "2"),
        ("char_at",      "\"halo\"",        "1"),
        ("pad_left",     "\"x\"",           "3, \".\""),
        ("count",        "\"aaa\"",         "\"a\""),
        // Dan bentuk TANPA argumen wajib: keempatnya harus menolak.
        ("contains",     "\"halo\"",        "@KOSONG"),
        ("starts_with",  "\"halo\"",        "@KOSONG"),
        ("split",        "\"halo\"",        "@KOSONG"),
        ("count",        "\"halo\"",        "@KOSONG"),
        ("char_at",      "\"halo\"",        "@KOSONG"),
        ("repeat",       "\"halo\"",        "@KOSONG"),
        ("pad_left",     "\"halo\"",        "@KOSONG"),
    ];

    let mut beda = Vec::new();
    for (nama, recv, arg) in kasus {
        let wajib_tolak = *arg == "@KOSONG";
        let arg = if wajib_tolak { "" } else { *arg };
        let ns = if arg.is_empty() {
            format!("str::{}({})", nama, recv)
        } else {
            format!("str::{}({}, {})", nama, recv, arg)
        };
        let me = format!("({}).{}({})", recv, nama, arg);

        let jalankan = |e: &str, vm: bool| -> String {
            let src = format!("package main\npub fn main() {{ wok({}) }}\n", e);
            let o = jalankan_keluaran_penuh(&src, vm);
            // Yang dibandingkan: berhasil dengan nilai apa, atau ditolak.
            // Kalimat galat boleh berbeda — yang tidak boleh berbeda adalah
            // ARAHNYA dan, kalau berhasil, nilainya.
            if o.contains("error") { "<ditolak>".to_string() } else { o.trim().to_string() }
        };

        let a = jalankan(&ns, false);
        let b = jalankan(&me, false);
        let c = jalankan(&ns, true);
        let d = jalankan(&me, true);

        if wajib_tolak {
            for (bentuk, hasil) in [("ns-i", &a), ("me-i", &b), ("ns-v", &c), ("me-v", &d)] {
                if hasil != "<ditolak>" {
                    beda.push(format!(
                        "  {}({}) tanpa argumen wajib DITERIMA di {} -> {:?}",
                        nama, recv, bentuk, hasil));
                }
            }
        } else if !(a == b && b == c && c == d) {
            beda.push(format!(
                "  {}: ns-i={:?} me-i={:?} ns-v={:?} me-v={:?}", nama, a, b, c, d));
        }
    }
    assert!(
        beda.is_empty(),
        "{} ketidaksepakatan antara bentuk method dan bentuk namespace:\n{}",
        beda.len(), beda.join("\n"));
}


/// Bentuk **method** tanpa argumen wajib, disapu dengan daftar yang
/// DITURUNKAN dari VM yang sedang berjalan.
#[test]
fn bentuk_method_disapu_dari_daftar_yang_diturunkan() {
    // Jawabannya acak, jadi empat pemanggilan tidak akan pernah sama.
    const ACAK: &[&str] = &["sample", "shuffle"];
    // Kosong, dan itu hasil pengukuran bukan kebetulan: sesudah perbaikan di
    // fase ini tidak ada satu pun nama yang keempat bentuknya berbeda.
    const BEDA_DISENGAJA: &[&str] = &[];
    // `.len()` ada sebagai method dan TIDAK ada sebagai `arr::len` (nama
    // telanjang `len` yang dipakai).
    const METHOD_SAJA: &[&str] = &["len", "to_string"];

    let semua = jrgc::vm::daftar_builtin();
    let mut nama: Vec<String> = semua.iter()
        .filter_map(|n| n.strip_prefix("str::").or_else(|| n.strip_prefix("arr::")))
        .map(|s| s.to_string())
        .collect();
    nama.sort();
    nama.dedup();
    assert!(nama.len() > 60, "daftar str::/arr:: sepertinya rusak: {}", nama.len());

    // Penolakan dibaca dari STATUS KELUAR, bukan dari ada-tidaknya kata `error`
    // di keluaran: sebuah program yang dibunuh batas waktu berkeluaran kosong,
    // dan itu terbaca sebagai jawaban — bukan sebagai kegagalan.
    let jawab = |e: &str, vm: bool| -> String {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", e);
        let h = jalankan(&src, vm);
        if !h.sukses { "<ditolak>".to_string() } else { h.keluaran.trim().to_string() }
    };

    let mut beda = Vec::new();
    let mut beda_disengaja_masih_beda = vec![false; BEDA_DISENGAJA.len()];
    for n in &nama {
        if ACAK.contains(&n.as_str()) || METHOD_SAJA.contains(&n.as_str()) { continue; }
        for (recv, ns) in [("\"halo dunia\"", "str"), ("[1,2,3,4]", "arr")] {
            let f_ns = format!("{}::{}({})", ns, n, recv);
            let f_me = format!("({}).{}()", recv, n);
            let r = [
                jawab(&f_ns, false), jawab(&f_me, false),
                jawab(&f_ns, true),  jawab(&f_me, true),
            ];
            // Nama itu memang bukan untuk penerima ini.
            if r.iter().all(|x| x == "<ditolak>") { continue; }
            if r.iter().any(|x| x != &r[0]) {
                if let Some(i) = BEDA_DISENGAJA.iter().position(|d| d == n) {
                    beda_disengaja_masih_beda[i] = true;
                    continue;
                }
                // DIULANG sebelum dicatat.
                let ulang = [
                    jawab(&f_ns, false), jawab(&f_me, false),
                    jawab(&f_ns, true),  jawab(&f_me, true),
                ];
                if ulang.iter().all(|x| x == &ulang[0]) {
                    eprintln!("[catatan] `{}` atas {} beda sekali lalu SAMA waktu \
                               diulang — dianggap beban mesin, bukan temuan", n, ns);
                    continue;
                }
                beda.push(format!(
                    "  {} atas {}: ns-i={:?} me-i={:?} ns-v={:?} me-v={:?}",
                    n, ns, r[0], r[1], r[2], r[3]));
            }
        }
    }
    assert!(
        beda.is_empty(),
        "{} nama menjawab BERBEDA antara bentuk method dan bentuk namespace \
         (atau antar-backend) waktu argumen wajibnya hilang:\n{}",
        beda.len(), beda.join("\n"));

    // Arah sebaliknya, supaya daftar pengecualiannya tidak diam-diam usang.
    for (i, d) in BEDA_DISENGAJA.iter().enumerate() {
        assert!(
            beda_disengaja_masih_beda[i],
            "KABAR BAIK: `{}` sekarang menjawab sama di keempat bentuk. \
             Hapus ia dari `BEDA_DISENGAJA` dan dari catatan di \
             `src/stdlib/koleksi.rs`.", d);
    }
}

/// WebSocket: jabat tangan, frame bolak-balik, dan penutupan — di kedua
/// backend, lewat SOKET SUNGGUHAN.
mod ws {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }

    const PROGRAM: &str = r#"package main
fn gema(c, req) {
  var n = 0
  while true {
    match dok::ws_recv(c) {
      Some(m) => { n = n + 1; if typeof(m) == "array" { dok::ws_send(c, m) } else { dok::ws_send(c, "gema:" + m) } }
      None => { break }
    }
  }
}
fn biasa(req) { return dok::text("http-biasa") }
pub fn main() {
  let r = dok::router_new()
  dok::router_get(r, "/", biasa)
  dok::router_ws(r, "/gema", gema)
  dok::serve(r, PORT)
}
"#;

    /// Frame teks dari KLIEN: wajib bermask (RFC 6455 §5.3).
    fn frame_klien(teks: &str) -> Vec<u8> { frame_mentah(0x81, teks.as_bytes()) }

    /// Frame klien dengan byte pertama dan muatan apa pun (biner, teks yang rusak).
    fn frame_mentah(kepala: u8, b: &[u8]) -> Vec<u8> {
        let mut f = vec![kepala];
        assert!(b.len() < 126, "uji ini cuma pakai pesan pendek");
        f.push(0x80 | b.len() as u8);
        let mask = [0xA1u8, 0xB2, 0xC3, 0xD4];
        f.extend_from_slice(&mask);
        for (i, x) in b.iter().enumerate() { f.push(x ^ mask[i % 4]); }
        f
    }

    /// Baca satu frame biner dari server, ditulis sebagai `biner:[..]`.
    fn baca_biner(s: &mut TcpStream) -> Option<String> {
        let mut h = [0u8; 2];
        s.read_exact(&mut h).ok()?;
        if h[0] & 0x0F != 2 { return Some(format!("op{}", h[0] & 0x0F)); }
        let mut b = vec![0u8; (h[1] & 0x7F) as usize];
        s.read_exact(&mut b).ok()?;
        Some(format!("biner:{:?}", b))
    }

    /// Baca satu frame teks dari server (tidak bermask).
    fn baca_teks(s: &mut TcpStream) -> Option<String> {
        let mut h = [0u8; 2];
        s.read_exact(&mut h).ok()?;
        if h[0] & 0x0F != 1 { return None; }
        let n = (h[1] & 0x7F) as usize;
        let mut b = vec![0u8; n];
        s.read_exact(&mut b).ok()?;
        String::from_utf8(b).ok()
    }

    fn jalankan(vm: bool, port: u16) -> Option<Vec<String>> {
        let dir = std::env::temp_dir().join(format!("jrg-ws-{}-{}", port, std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let berkas = dir.join("ws.jrg");
        std::fs::write(&berkas, PROGRAM.replace("PORT", &port.to_string())).ok()?;

        let mut cmd = Command::new(jrgc());
        cmd.arg(&berkas);
        if vm { cmd.arg("--vm"); }
        cmd.env("JRG_SOPAN", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let anak = Server(cmd.spawn().ok()?);

        let batas = Instant::now() + Duration::from_secs(20);
        while Instant::now() < batas {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() { break; }
            std::thread::sleep(Duration::from_millis(50));
        }

        let mut hasil = Vec::new();

        // 1. Jabat tangan.
        let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        let req = format!(
            "GET /gema HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n");
        s.write_all(req.as_bytes()).ok()?;
        let mut kepala = Vec::new();
        let mut satu = [0u8; 1];
        while !kepala.ends_with(b"\r\n\r\n") {
            if s.read(&mut satu).ok()? == 0 { return None; }
            kepala.push(satu[0]);
        }
        let kepala = String::from_utf8_lossy(&kepala).to_string();
        hasil.push(if kepala.starts_with("HTTP/1.1 101") { "101".into() } else { "BUKAN-101".into() });
        // Kunci jawaban untuk nonce contoh RFC 6455 — angkanya tetap, jadi
        // bisa dibandingkan langsung.
        hasil.push(if kepala.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=") { "kunci-ok".into() }
                   else { "kunci-salah".into() });

        // 2. Kirim dua pesan, harus digemakan urut.
        s.write_all(&frame_klien("satu")).ok()?;
        hasil.push(baca_teks(&mut s).unwrap_or_default());
        s.write_all(&frame_klien("dua")).ok()?;
        hasil.push(baca_teks(&mut s).unwrap_or_default());

        // 3. Ping harus dijawab pong OTOMATIS, tanpa sampai ke aplikasi.
        let mask = [1u8, 2, 3, 4];
        let mut ping = vec![0x89u8, 0x80 | 2, 1, 2, 3, 4];
        for (i, x) in b"hi".iter().enumerate() { ping.push(x ^ mask[i % 4]); }
        s.write_all(&ping).ok()?;
        let mut ph = [0u8; 2];
        s.read_exact(&mut ph).ok()?;
        hasil.push(if ph[0] & 0x0F == 0xA { "pong".into() } else { format!("op{}", ph[0] & 0x0F) });
        let mut sisa = vec![0u8; (ph[1] & 0x7F) as usize];
        s.read_exact(&mut sisa).ok()?;

        // 3b. Pesan biner sampai sebagai array byte, bukan teks yang byte rusaknya diganti.
        s.write_all(&frame_mentah(0x82, &[0, 255])).ok()?;
        hasil.push(baca_biner(&mut s).unwrap_or_default());

        // 3c. Pesan TEKS yang bukan UTF-8 menutup koneksinya dengan kode 1007.
        s.write_all(&frame_mentah(0x81, &[0xFF])).ok()?;
        let mut th = [0u8; 4];
        s.read_exact(&mut th).ok()?;
        hasil.push(if th == [0x88, 2, 0x03, 0xEF] { "tutup-1007".into() } else { format!("{:?}", th) });

        // 4. HTTP biasa tetap jalan di router yang sama.
        let mut h = TcpStream::connect(("127.0.0.1", port)).ok()?;
        h.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        h.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").ok()?;
        let mut b = String::new();
        h.read_to_string(&mut b).ok()?;
        hasil.push(if b.contains("http-biasa") { "http-ok".into() } else { "http-gagal".into() });

        drop(anak);
        Some(hasil)
    }

    fn jalankan_ulet(vm: bool) -> Option<Vec<String>> {
        for _ in 0..5 {
            let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            if let Some(h) = jalankan(vm, p) { return Some(h); }
        }
        None
    }

    /// Pengirim di goroutine lain tidak menunggu pembaca (Fase JRGM-1, laporan IDE #9): dulu
    /// `ws_recv` yang menunggu memegang kunci semua koneksi, jadi cuma `tik 0` yang sampai.
    #[test]
    fn websocket_dua_arah_tanpa_menunggu_pembaca() {
        const DUA_ARAH: &str = "package main\npub fn main() {\n let r = dok::router_new()\n \
            dok::router_ws(r, \"/ws\", |c, req| {\n spawn { var i = 0\n while i < 5 { dok::ws_send(c, \"tik \" + str(i))\n \
            time::sleep_ms(50)\n i = i + 1 } }\n while true { match dok::ws_recv(c) { Some(m) => { let _ = 1 }, None => { break } } }\n })\n \
            dok::serve(r, PORT)\n}\n";
        for vm in [false, true] {
            let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            let dir = std::env::temp_dir().join(format!("jrg-ws2-{}-{}", port, std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let berkas = dir.join("ws.jrg");
            std::fs::write(&berkas, DUA_ARAH.replace("PORT", &port.to_string())).unwrap();
            let mut cmd = Command::new(jrgc());
            cmd.arg(&berkas).env("JRG_SOPAN", "1").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            if vm { cmd.arg("--vm"); }
            let anak = Server(cmd.spawn().unwrap());
            let batas = Instant::now() + Duration::from_secs(20);
            let mut s = loop {
                if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) { break s; }
                assert!(Instant::now() < batas, "server ws tidak menjawab");
                std::thread::sleep(Duration::from_millis(50));
            };
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            s.write_all(b"GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").unwrap();
            let mut kepala = Vec::new();
            let mut satu = [0u8; 1];
            while !kepala.ends_with(b"\r\n\r\n") { s.read_exact(&mut satu).unwrap(); kepala.push(satu[0]); }
            let tik: Vec<String> = (0..5).filter_map(|_| baca_teks(&mut s)).collect();
            drop(anak);
            let _ = std::fs::remove_dir_all(&dir);
            assert_eq!(tik, ["tik 0", "tik 1", "tik 2", "tik 3", "tik 4"], "vm={}: pengirim tertahan pembaca", vm);
        }
    }

    #[test]
    fn websocket_sama_di_kedua_backend() {
        let i = jalankan_ulet(false).expect("server ws interpreter tidak menjawab");
        let v = jalankan_ulet(true).expect("server ws vm tidak menjawab");

        // Isi diperiksa DULU: dua backend yang sama-sama diam juga "identik".
        assert_eq!(i[0], "101", "jabat tangan tidak dijawab 101: {:?}", i);
        assert_eq!(i[1], "kunci-ok", "kunci Sec-WebSocket-Accept salah: {:?}", i);
        assert_eq!(i[2], "gema:satu", "pesan pertama tidak digemakan: {:?}", i);
        assert_eq!(i[3], "gema:dua", "pesan kedua tidak digemakan: {:?}", i);
        assert_eq!(i[4], "pong", "ping tidak dijawab pong otomatis: {:?}", i);
        assert_eq!(i[5], "biner:[0, 255]", "pesan biner tidak bolak-balik sebagai byte: {:?}", i);
        assert_eq!(i[6], "tutup-1007", "pesan teks bukan UTF-8 tidak menutup dengan 1007: {:?}", i);
        assert_eq!(i[7], "http-ok", "HTTP biasa rusak oleh route WS: {:?}", i);

        assert_eq!(i, v, "WebSocket berbeda antar backend");
    }
}

/// Driver SQLite: nilai bolak-balik, parameter DIIKAT, dan kegagalan yang
/// menyebut sebabnya — di kedua backend.
#[test]
fn sqlite_sama_di_kedua_backend() {
    let cek = "package main\npub fn main() { wok(dok::db_tersedia()) }\n";
    if !jalankan_keluaran_penuh(cek, false).contains("true") {
        eprintln!("LEWAT: libsqlite3 tidak bisa dimuat di mesin ini");
        return;
    }

    // Satu program, dijalankan utuh: yang diuji adalah URUTAN operasinya — buat,
    // isi, baca, ikat, tutup — karena sebuah driver bisa benar per fungsi dan …
    const PROGRAM: &str = r#"package main
pub fn main() {
  let db = dok::db_open(":memory:")?
  wok(dok::db_exec(db, "CREATE TABLE t(id INTEGER PRIMARY KEY, n TEXT, s REAL)")?)
  wok(dok::db_exec(db, "INSERT INTO t(n, s) VALUES (?, ?)", ["budi", 9.5])?)
  wok(dok::db_exec(db, "INSERT INTO t(n, s) VALUES (?, ?)", ["ani", 8.25])?)
  wok(dok::db_last_id(db)?)
  wok(dok::db_query(db, "SELECT * FROM t ORDER BY id")?)
  wok(dok::db_query(db, "SELECT n FROM t WHERE s > ?", [9.0])?)
  // Injeksi: nilainya DIIKAT, jadi ia dicari sebagai nama — bukan dijalankan.
  wok(dok::db_query(db, "SELECT * FROM t WHERE n = ?", ["budi'; DROP TABLE t; --"])?)
  wok(dok::db_query(db, "SELECT count(*) AS c FROM t")?)
  // NULL bolak-balik sebagai `nil`, bukan string kosong.
  wok(dok::db_exec(db, "INSERT INTO t(n, s) VALUES (?, ?)", [nil, nil])?)
  wok(dok::db_query(db, "SELECT id, n, s FROM t WHERE n IS NULL")?)
  // Kegagalan menyebut sebabnya, dan tidak mematikan koneksinya.
  wok(dok::db_query(db, "SELECT * FROM tidak_ada"))
  wok(dok::db_query(db, "SELECT ?", [{"a": 1}]))
  wok(dok::db_query(db, "SELECT count(*) AS c FROM t")?)
  dok::db_close(db)
  wok(dok::db_query(db, "SELECT 1"))
}
"#;
    let i = jalankan_keluaran_penuh(PROGRAM, false);
    let v = jalankan_keluaran_penuh(PROGRAM, true);

    // Isi diperiksa DULU: dua backend yang sama-sama gagal juga "identik".
    assert!(i.contains("budi") && i.contains("9.5"), "baris tidak terbaca:\n{}", i);
    assert!(i.contains("c: 2"), "tabel tidak utuh sesudah percobaan injeksi:\n{}", i);
    assert!(i.contains("n: nil"), "NULL tidak jadi `nil`:\n{}", i);
    assert!(i.contains("no such table"), "galat SQL tidak menyebut sebabnya:\n{}", i);
    assert!(i.contains("tidak punya padanan di sqlite"),
        "parameter bertipe asing tidak ditolak dengan sebabnya:\n{}", i);
    assert!(i.contains("koneksi sudah ditutup"),
        "pemakaian sesudah tutup tidak dilaporkan:\n{}", i);

    assert_eq!(i, v, "driver SQLite berbeda antar backend");
}

/// `NAMA_HOF` harus memuat setiap nama yang benar-benar didispatch
/// `run_hof_method`.
#[test]
fn nama_hof_lengkap() {
    let src = include_str!("../src/vm/vm.rs");
    let awal = src.find("fn run_hof_method").expect("run_hof_method tidak ketemu");
    // Badan fungsinya sampai method berikutnya di tingkat yang sama — dengan atau
    // tanpa `pub`/`pub(crate)`, yang dipasang PECAH pada method pindahan.
    let sisa = &src[awal..];
    let akhir = ["\n    fn ", "\n    pub fn ", "\n    pub(crate) fn "].iter()
        .filter_map(|p| sisa[10..].find(p)).min().map(|i| i + 10).unwrap_or(sisa.len());
    let badan = &sisa[..akhir];

    // Nama yang dipakai sebagai POLA match: `"x"` di awal baris atau sesudah
    // `|`. Sengaja tidak semua literal string di badan — sebuah pesan galat
    // juga memuat tanda kutip.
    let mut dari_sumber: Vec<String> = Vec::new();
    for baris in badan.lines() {
        let t = baris.trim();
        if !(t.starts_with('"') || t.starts_with("| \"")) { continue; }
        for bagian in t.split('|') {
            let b = bagian.trim();
            if let Some(n) = b.strip_prefix('"').and_then(|x| x.split('"').next()) {
                if !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    dari_sumber.push(n.to_string());
                }
            }
        }
    }
    dari_sumber.sort();
    dari_sumber.dedup();
    assert!(dari_sumber.len() >= 10,
        "pemindainya sepertinya rusak: cuma {} nama dari sumber", dari_sumber.len());

    let terdaftar: Vec<&str> = jrgc::vm::Vm::NAMA_HOF.to_vec();
    let hilang: Vec<&String> = dari_sumber.iter()
        .filter(|n| !terdaftar.contains(&n.as_str())).collect();
    assert!(hilang.is_empty(),
        "{} nama didispatch `run_hof_method` tapi TIDAK ada di `NAMA_HOF` — \
         akibatnya ia hilang dari `daftar_builtin()`, dan tiga instrumen \
         menurunkan daftar sapuannya dari sana:\n  {:?}", hilang.len(), hilang);

    // ── Arah SEBALIKNYA ────────────────────────────────────────────────
    let semua = jrgc::vm::daftar_builtin();
    let mut palsu = Vec::new();
    let mut diperiksa = 0usize;
    for h in jrgc::vm::Vm::NAMA_HOF {
        let n = format!("arr::{}", h);
        if !semua.contains(&n) { continue; }   // sengaja tidak diiklankan
        diperiksa += 1;
        let arg = match *h {
            "reduce"   => "[1,2], |a,b| a+b, 0",
            "fold"     => "[1,2], 0, |a,b| a+b",
            "zip_with" => "[1], [2], |a,b| a+b",
            "sort_by"  => "[2,1], |a,b| a-b",
            _          => "[1,2], |x| x",
        };
        let src = format!("package main\npub fn main() {{ wok({}({})) }}\n", n, arg);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            if keluar.contains("tidak ditemukan") {
                palsu.push(format!("  {} di {} — diiklankan registry, tidak bisa dipanggil", n, nama));
            }
        }
    }
    // Berapa yang BENAR-BENAR diperiksa dilaporkan, bukan diandaikan: sebuah
    // loop yang tidak pernah berjalan lulus dengan sempurna, dan itu bentuk
    // kegagalan instrumen yang paling sulit dilihat.
    assert!(diperiksa >= 15,
        "cuma {} nama HOF yang benar-benar diperiksa — loop-nya sepertinya \
         tidak berjalan, dan tes yang tidak menjalankan apa pun selalu hijau",
        diperiksa);
    assert!(palsu.is_empty(),
        "{} nama diiklankan `daftar_builtin()` tapi tidak bisa dipanggil:\n{}",
        palsu.len(), palsu.join("\n"));

    // Arah sebaliknya: entri yang tidak lagi didispatch harus dibuang, atau
    // ia akan muncul di `:cari` sebagai nama yang tidak bisa dipanggil.
    let usang: Vec<&&str> = terdaftar.iter()
        .filter(|n| !dari_sumber.contains(&n.to_string())).collect();
    assert!(usang.is_empty(),
        "{} entri `NAMA_HOF` tidak lagi didispatch `run_hof_method`: {:?}",
        usang.len(), usang);
}

/// Tiap entri `bantu::SINTAKS` harus benar-benar bisa ditulis.
#[test]
fn bantuan_sintaks_benar_benar_bisa_ditulis() {
    // (nama entri, program yang memakai bentuk itu, keluaran yang diharapkan)
    let kasus: &[(&str, &str, &str)] = &[
        ("for",   "for i, x in [10,20] { wok(i, x) }",                    "0 10\n1 20"),
        ("for",   "for _, x in [10,20] { wok(x) }",                        "10\n20"),
        ("for",   "for k, v in {\"a\": 1} { wok(k, v) }",                    "a 1"),
        ("for",   "for x in [10,20] { wok(x) }",                           "0\n1"),
        ("match", "wok(match 2 { 1 => \"satu\", n @ 2 => \"dua\", _ => \"lain\" })", "dua"),
        ("let",   "let [a, b] = [1, 2]; wok(a, b)",                        "1 2"),
        ("let",   "let (a, b) = (3, 4); wok(a, b)",                        "3 4"),
        ("$",     "wok(1 < 2 $ \"ya\" : \"tidak\")",                        "ya"),
        ("??",    "wok(arr::first([]) ?? 9)",                              "9"),
        ("??",    "wok(arr::first([5]) ?? 9)",                             "5"),
        ("\"\"\"", "wok(\"\"\"a${1+1}b\"\"\")",                                 "a2b"),
        ("`",     "wok(`mentah \\n utuh`)",                                 "mentah \\n utuh"),
        // Contoh `select` di hover LSP dulu ditulis `case v := <-c: wok(v)` — tanpa
        // blok — dan itu TIDAK PARSE.
        ("select", "let c = make_chan(int, 1); c <- 7; select { case v := <-c: { wok(v) } }", "7"),
        ("...",   "let a = [2,3]; wok([1, ...a, 4])",                      "[1, 2, 3, 4]"),
        ("...",   "let a = [1,2]; wok(arr::sum([...a, 3]))",               "6"),
        // Bentuk path butuh `impl` di tingkat atas, jadi contohnya membawa
        // `package`-nya sendiri.
        ("::",
         "package main\nstruct P { pub n: int }\nimpl P { pub fn nol() -> P { return P{n:0} }\n  pub fn dua(self) -> int { return self.n * 2 } }\npub fn main() {\n  wok(P::nol().n)\n  wok(P::dua(P{n:3}))\n  wok(arr::map([P{n:1}, P{n:2}], P::dua))\n}",
         "0\n6\n[2, 4]"),
    ];
    let mut gagal = Vec::new();
    for (nama, program, harap) in kasus {
        // Tiap entri yang diuji harus benar-benar ada di tabelnya.
        assert!(jrgc::bantu::SINTAKS.iter().any(|i| i.nama == *nama),
            "kasus untuk `{}` ada di tes tapi tidak ada di `SINTAKS`", nama);
        // Sebuah contoh yang sudah membawa `package` sendiri dipakai APA ADANYA.
        let src = if program.trim_start().starts_with("package ") {
            format!("{}\n", program)
        } else {
            format!("package main\npub fn main() {{ {} }}\n", program)
        };
        for vm in [false, true] {
            let o = jalankan_keluaran_penuh(&src, vm);
            if o.trim() != *harap {
                gagal.push(format!("  [{}] {} — {} menjawab {:?}, harusnya {:?}",
                    nama, program, if vm { "vm" } else { "interp" }, o.trim(), harap));
            }
        }
    }
    assert!(gagal.is_empty(),
        "{} contoh bentuk sintaks tidak berperilaku seperti yang dijelaskan:\n{}",
        gagal.len(), gagal.join("\n"));

    // Arah sebaliknya: tiap entri `SINTAKS` harus punya minimal satu kasus.
    // Sebuah entri tanpa contoh adalah kalimat yang tidak pernah diperiksa.
    for i in jrgc::bantu::SINTAKS {
        assert!(kasus.iter().any(|(n, _, _)| n == &i.nama)
                || matches!(i.nama, "?" | "spawn"),
            "entri `SINTAKS` `{}` tidak punya contoh yang dijalankan", i.nama);
    }
}

/// Tiap nama yang dijelaskan `bantu::DOKUMEN` harus benar-benar terdaftar.
#[test]
fn dokumentasi_builtin_tidak_basi() {
    let terdaftar = jrgc::vm::daftar_builtin();
    let basi: Vec<&str> = jrgc::bantu::DOKUMEN.iter()
        .map(|i| i.nama)
        .filter(|n| !terdaftar.iter().any(|t| t == n))
        .collect();
    assert!(basi.is_empty(),
        "{} entri dokumentasi menjelaskan nama yang TIDAK terdaftar:\n  {:?}",
        basi.len(), basi);

    // Bukan assertion, melainkan laporan: cakupan boleh berapa pun, yang
    // tidak boleh adalah angkanya diingat alih-alih diukur.
    eprintln!("[bantu] {} dari {} builtin punya penjelasan ({:.0}%)",
        jrgc::bantu::DOKUMEN.len(), terdaftar.len(),
        100.0 * jrgc::bantu::DOKUMEN.len() as f64 / terdaftar.len() as f64);
}

/// `wok(v)` harus mencetak teks yang SAMA di kedua backend, untuk setiap
/// bentuk nilai yang bisa dibuat sebuah program.
#[test]
fn tampilan_nilai_sama_di_dua_backend() {
    // Satu ekspresi per bentuk nilai. Yang penting keragaman BENTUK, bukan
    // jumlahnya.
    const NILAI: &[&str] = &[
        "42", "-1", "3.5", "0.0", "1e300", "-0.0", "true", "false", "nil", "()",
        "'a'", "\"teks\"", "\"\"", "[1,2,3]", "[]", "[[1],[2]]", "(1,2)",
        "{\"a\":1}", "{}", "2**100", "0-(2**100)", "1.0/0.0", "0.0/0.0",
        "typeof(1)", "arr::first([])", "arr::first([7])",
        "Canvas::new(4,4)", "Canvas::new(4,4,\"J\")", "Chan::new(1)",
        "jawa::alloc(8)", "Mutex::new()", "Atomic::new(0)", "Atomic::new()",
        "wokwok::array([1,2])", "wokwok::zeros([2,2])", "wokwok::arange(3)",
        "Some(1)", "None", "Ok(1)", "Err(\"x\")",
        "Option::Some(1)", "Option::None", "Result::Ok(1)", "Result::Err(\"x\")",
        "|x| x", "wok", "str::upper", "range(3)",
    ];

    let mut beda = Vec::new();
    for e in NILAI {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", e);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        // Program yang GAGAL di dua-duanya bukan urusan tes ini — yang
        // dibandingkan tampilan, bukan penerimaan. Tapi gagal di SATU sisi
        // saja tetap sebuah divergensi, dan itu dilaporkan.
        let i_galat = i.contains("error");
        let v_galat = v.contains("error");
        if i_galat && v_galat { continue; }
        if i_galat != v_galat {
            beda.push(format!("  {} — {} menolak, yang lain menjawab", e,
                if i_galat { "interp" } else { "vm" }));
            continue;
        }
        if i.trim() != v.trim() {
            beda.push(format!("  {} — interp={:?} vm={:?}", e, i.trim(), v.trim()));
        }
    }
    assert!(beda.is_empty(),
        "{} bentuk nilai dicetak BERBEDA di kedua backend:\n{}",
        beda.len(), beda.join("\n"));
}

/// `canvas::` disapu sendiri, karena setiap sapuan lain mengecualikannya.
#[test]
fn canvas_disapu_walau_sapuan_lain_mengecualikannya() {
    const LEWATI: &[&str] = &[
        "canvas::update", "canvas::is_open", "canvas::close",
        "Canvas::load_png",
    ];
    // Bentuk argumen: yang SALAH untuk menemukan panic, dan yang BENAR untuk
    // menemukan asimetri pada fungsi yang bekerja — pelajaran Fase BANTU,
    // di mana bentuk bertipe salah saja membuat 62 asimetri tak terlihat.
    const BENTUK: &[&str] = &[
        "", "5", "5, 5", "\"\"", "[]", "nil",
        "Canvas::new(4,4)", "Canvas::new(4,4), 0, 0",
        "Canvas::new(4,4), 0, 0, 2, 2, 255",
        "Canvas::new(4,4), Canvas::new(2,2), 0, 0",
    ];

    let nama: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| n.starts_with("canvas::") || n.starts_with("Canvas::"))
        .filter(|n| !LEWATI.contains(&n.as_str()))
        .collect();
    assert!(nama.len() > 40, "daftar canvas sepertinya rusak: {}", nama.len());

    let mut mati = Vec::new();
    let mut beda = Vec::new();
    for n in &nama {
        for b in BENTUK {
            let src = format!("package main\npub fn main() {{ wok({}({})) }}\n", n, b);
            let i = jalankan_keluaran_penuh(&src, false);
            let v = jalankan_keluaran_penuh(&src, true);
            for (keluar, sisi) in [(&i, "interp"), (&v, "vm")] {
                if keluar.contains("panicked at") {
                    mati.push(format!("  {}({}) di {}", n, b, sisi));
                }
            }
            let (it, vt) = (i.contains("error"), v.contains("error"));
            if it != vt {
                beda.push(format!("  {}({}) — interp {}, vm {}", n, b,
                    if it { "tolak" } else { "TERIMA" },
                    if vt { "tolak" } else { "TERIMA" }));
                break;
            }
        }
    }
    assert!(mati.is_empty(),
        "{} pemanggilan `canvas::` MEMATIKAN PROSES:\n{}", mati.len(), mati.join("\n"));
    assert!(beda.is_empty(),
        "{} nama `canvas::` berbeda arah terima/tolak antar-backend:\n{}",
        beda.len(), beda.join("\n"));
}

/// Sebuah pesan galat tidak boleh berubah ukuran mengikuti besar datanya.
#[test]
fn pesan_galat_tidak_tumbuh_dengan_besar_data() {
    // Disapu per BENTUK kesalahan, bukan satu contoh: kelas ini sudah muncul
    // di tiga pintu berbeda (indexing, `match`, lalu dua pintu di sini), jadi
    // menguncinya di satu bentuk cuma memindahkan tempat ia bisa kembali.
    let bentuk: &[(&str, &str)] = &[
        ("method tidak ada", "a.panjang()"),
        ("operator biner",   "a + 5"),
        ("pembandingan",     "a < 3"),
        ("index salah tipe", "a[\"k\"]"),
        ("memanggil bukan fungsi", "a(1)"),
        ("HOF dengan bukan fungsi", "arr::map([1], a)"),
        // Fase KONVERSI: dua keluarga yang dulu mencetak NILAI operandnya.
        ("negasi",           "-a"),
        ("balik bit",        "~a"),
        ("operator bit",     "a & 1"),
        ("geser bit",        "a << 1"),
        ("cast",             "a as int"),
    ];
    let prog = |n: usize, ekspr: &str| format!(
        "package main\npub fn main() {{\n    var a = []\n    \
         for i in range(0, {}) {{ a = a + [i] }}\n    wok({})\n}}\n", n, ekspr);

    for (label, ekspr) in bentuk {
        for vm in [false, true] {
            // Baris lokasi dibuang: jalur berkas sementaranya memuat nomor acak, yang
            // sesekali kebetulan berisi `2999` dan memerahkan tes ini tanpa sebab.
            let tanpa_lokasi = |s: String| s.lines().filter(|l| !l.trim_start().starts_with("-->"))
                .collect::<Vec<_>>().join("\n");
            let kecil = tanpa_lokasi(jalankan_keluaran_penuh(&prog(10, ekspr), vm));
            let besar = tanpa_lokasi(jalankan_keluaran_penuh(&prog(3000, ekspr), vm));
            let nama = if vm { "--vm" } else { "interpreter" };

            assert!(!besar.contains("2999"),
                "{} / {}: pesan galat memuat ISI array — ia mencetak nilainya, \
                 bukan tipenya", nama, label);

            // Ambang longgar dan disengaja: yang dilarang adalah pesan yang
            // TUMBUH, bukan selisih beberapa byte. 3000 elemen melawan 10 akan
            // memberi selisih ribuan byte kalau isinya ikut tercetak.
            let selisih = (besar.len() as i64 - kecil.len() as i64).abs();
            assert!(selisih < 64,
                "{} / {}: pesan galat tumbuh {} byte saat datanya membesar \
                 (10 elemen: {} byte, 3000 elemen: {} byte)\n--- besar ---\n{}",
                nama, label, selisih, kecil.len(), besar.len(),
                besar.chars().take(300).collect::<String>());
        }
    }
}

/// Satu kelas kesalahan operator, satu kalimat — di kedua backend.
#[test]
fn operator_tak_berlaku_pakai_satu_kalimat_di_dua_backend() {
    // (ekspresi, tipe kiri, tipe kanan)
    let kasus: &[(&str, &str, &str)] = &[
        ("[1,2,3] + 5",  "array",  "int"),
        ("5 - \"a\"",    "int",    "string"),
        ("true / 2",     "bool",   "int"),
        // Dua bentuk yang dulu menuduh operand yang salah.
        ("5 % \"a\"",    "int",    "string"),
        ("5 ** \"a\"",   "int",    "string"),
        // Operator bit (Fase KONVERSI): dulu interpreter mencetak NILAI-nya.
        ("[1] & 2",      "array",  "int"),
        ("1.5 | 2",      "float",  "int"),
        ("[1] << 1",     "array",  "int"),
        ("1 >> \"a\"",   "int",    "string"),
        ("1 &^ 2.5",     "int",    "float"),
    ];
    for (ekspr, kiri, kanan) in kasus {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        for (nama, keluar) in [("interpreter", &i), ("--vm", &v)] {
            assert!(keluar.contains(&format!("antara `{}` dan `{}`", kiri, kanan)),
                "{} atas `{}`: harus menyebut KEDUA tipe (`{}` dan `{}`);\n{}",
                nama, ekspr, kiri, kanan, keluar);
        }
    }
    // Operator prefix: tipe, bukan nilai — `-[1, 2]` dulu mencetak arraynya.
    for (ekspr, harap) in [
        ("-'a'",    "operator `-` cuma untuk angka (int/float), dapat `rune`"),
        ("-[1, 2]", "operator `-` cuma untuk angka (int/float), dapat `array`"),
        ("~1.5",    "operator `~` cuma untuk bilangan bulat, dapat `float`"),
        ("!\"a\"",  "operator `!` cuma untuk bool, dapat `string`"),
    ] {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            assert!(keluar.contains(harap), "{} atas `{}`: harus berbunyi `{}`;\n{}",
                nama, ekspr, harap, keluar);
        }
    }
    // Pembandingan: kata kerjanya beda, aturannya sama.
    let src = "package main\npub fn main() { wok([1,2] < 3) }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("tidak bisa membandingkan `array` dengan `int`"),
            "{}: pembandingan harus pakai kalimat bersama;\n{}", nama, keluar);
    }
}

/// Satu kesalahan, satu kalimat — di kedua backend dan untuk tiap jenis
/// penerima.
#[test]
fn method_tak_ada_pakai_satu_kalimat_di_dua_backend() {
    let kasus: &[(&str, &str)] = &[
        ("wok([1,2,3].panjang())",              "array"),
        ("wok(\"halo\".panjang())",             "string"),
        ("wok((1,2).panjang())",                "tuple"),
        ("wok(dict::new().panjang())",          "dict"),
        // Skalar tidak punya namespace method sama sekali — dulu VM
        // menjawabnya lewat kalimat yang lain lagi ("tidak bisa akses X
        // dari Y").
        ("wok(5.panjang())",                    "int"),
        ("wok(true.panjang())",                 "bool"),
        ("wok(Canvas::new(2,2).panjang())",     "canvas"),
        ("wok(make_chan(int,1).panjang())",     "channel"),
    ];
    for (ekspr, tipe) in kasus {
        let src = format!("package main\npub fn main() {{ {} }}\n", ekspr);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        for (nama, keluar) in [("interpreter", &i), ("--vm", &v)] {
            assert!(keluar.contains("tidak punya method atau field bernama"),
                "{} atas `{}`: kalimatnya bukan yang bersama;\n{}", nama, ekspr, keluar);
            assert!(keluar.contains(tipe),
                "{} atas `{}`: pesannya harus menyebut tipe `{}`;\n{}",
                nama, ekspr, tipe, keluar);
        }
    }
}

/// **Pintu method HOF: satu kalimat untuk penerima yang tidak punya method
/// itu, dan satu kalimat untuk jumlah argumen yang salah — di dua backend.**
#[test]
fn method_hof_satu_pintu_di_dua_backend() {
    const PRA: &str = "package main\nstruct P { pub a: int }\nenum E { A, B }\npub fn main() {\n";
    // Penerima tanpa method HOF bawaan. String sengaja tidak di sini: `find` memang method string.
    let penerima: &[(&str, &str)] = &[
        ("int", "5"), ("dict", "dict::new()"), ("struct P", "P { a: 1 }"),
        ("enum:E", "E::A"), ("tuple", "(1, 2)"), ("bool", "true"),
    ];
    let mut kasus: Vec<(String, String)> = Vec::new();
    for h in jrgc::vm::Vm::NAMA_HOF {
        let (n, _) = jrgc::interpreter::bentuk_hof_method(h)
            .unwrap_or_else(|| panic!("`{}` ada di NAMA_HOF tapi tidak punya bentuk aritas", h));
        let args = vec!["|x| x"; n].join(", ");
        for (tipe, ekspr) in penerima {
            kasus.push((format!("let r = {}\n wok(r.{}({}))", ekspr, h, args),
                format!("`{}` tidak punya method atau field bernama `{}`", tipe, h)));
        }
        if matches!(*h, "map_err" | "and_then") {
            kasus.push((format!("let r = [1, 2]\n wok(r.{}({}))", h, args),
                format!("`array` tidak punya method atau field bernama `{}`", h)));
        } else {
            for diberi in [0, n + 1] {
                kasus.push((format!("wok([1, 2].{}({}))", h, vec!["|x| x"; diberi].join(", ")),
                    format!("method `{}` butuh {} argumen", h, n)));
            }
        }
    }
    for ekspr in ["Option::Some(1).map()", "Result::Err(1).map_err(|x| x, |x| x)", "Option::None.and_then()"] {
        let m = ekspr.split('.').nth(1).and_then(|s| s.split('(').next()).unwrap_or("");
        kasus.push((format!("wok({})", ekspr), format!("method `{}` butuh 1 argumen", m)));
    }
    assert!(kasus.len() > 150, "cuma {} kasus — sapuannya sepertinya tidak berjalan", kasus.len());

    let mut salah = Vec::new();
    for (badan, harap) in &kasus {
        let src = format!("{}{}\n}}\n", PRA, badan);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        if !i.contains(harap.as_str()) || !v.contains(harap.as_str()) || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
            salah.push(format!("  {}\n    harap: {}\n    int  : {}\n    vm   : {}",
                badan.replace('\n', ";"), harap, jawaban_ringkas(&i), jawaban_ringkas(&v)));
        }
    }
    assert!(salah.is_empty(), "{} dari {} kasus pintu HOF tidak satu kalimat:\n{}",
        salah.len(), kasus.len(), salah.join("\n"));
}

/// **Pintu `math::`: satu implementasi, jadi satu jawaban di dua mesin.**
#[test]
fn pintu_math_satu_jawaban_di_dua_backend() {
    let nama: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter_map(|n| n.strip_prefix("math::").map(|s| s.to_string()))
        .filter(|s| !s.starts_with(|c: char| c.is_ascii_uppercase()))
        .collect();
    assert!(nama.len() >= 30, "cuma {} nama `math::` — registry-nya menyusut", nama.len());
    let bentuk = [
        "4", "2.25", "-3", "0.0", "-0.0", "0.0 / 0.0", "true", "\"a\"",
        "2, 4294967296", "1, 2.5", "1.1, 7", "5, 10, 0", "10, 0, 5",
        "170141183460469231731687303715884105727, 2",
    ];
    let mut kasus = 0;
    let mut salah = Vec::new();
    for n in &nama {
        for b in bentuk {
            kasus += 1;
            let src = format!("package main\npub fn main() {{ wok(math::{}({})) }}\n", n, b);
            let i = jalankan_keluaran_penuh(&src, false);
            let v = jalankan_keluaran_penuh(&src, true);
            if i.contains("panicked") || v.contains("panicked") {
                salah.push(format!("  math::{}({}): PANIK", n, b));
            } else if jawaban_ringkas(&i) != jawaban_ringkas(&v) {
                salah.push(format!("  math::{}({})\n    int: {}\n    vm : {}",
                    n, b, jawaban_ringkas(&i), jawaban_ringkas(&v)));
            }
        }
    }
    assert!(kasus >= 400, "cuma {} kasus — sapuannya menyusut", kasus);

    // Nilai yang benar, bukan cuma yang sama.
    let harus: &[(&str, &str)] = &[
        ("math::pow(2, 4294967296)", "inf"),
        ("math::sign(0.0)", "0.0"),
        ("math::sign(-0.0)", "0.0"),
        ("math::max(1, 2.5)", "2.5"),
        ("math::min(1, 2.5)", "1"),
        ("math::clamp(5, 10, 0)", "error: math::clamp: batas terbalik"),
        ("math::lcm(170141183460469231731687303715884105727, 2)", "error: math::lcm"),
        ("math::gcd(1.1, 7)", "error: math::gcd: butuh bilangan bulat, dapat pecahan"),
        ("math::sqrt(true)", "error: math::sqrt: butuh angka, dapat `bool`"),
        ("range(0, 2.5)", "error: range: argumen ke-2 butuh bilangan bulat, dapat pecahan"),
        ("range(0, true)", "error: range: argumen ke-2 butuh bilangan bulat, dapat `bool`"),
        // Bentuknya ada di baris bantuan (tanda tangan `--doc`) sejak Fase KURANG.
        ("math::pow(2)", "error: `math::pow` butuh 2 argumen, dapat 1"),
    ];
    for (ekspr, awal) in harus {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        for vm in [false, true] {
            let j = jawaban_ringkas(&jalankan_keluaran_penuh(&src, vm));
            if !j.starts_with(awal) {
                salah.push(format!("  {} ({}): harus diawali `{}`, dapat `{}`",
                    ekspr, if vm { "vm" } else { "int" }, awal, j));
            }
        }
    }
    assert!(salah.is_empty(), "{} kasus pintu `math::` berselisih atau salah:\n{}",
        salah.len(), salah.join("\n"));
}

/// **`range`: satu definisi untuk empat pintu, dan tidak satu pun mematikan
/// prosesnya.**
#[test]
fn range_satu_definisi_dan_tidak_mematikan_proses() {
    fn jalan_terbatas(ekspr: &str, vm: bool) -> String {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        let dir = crate::bersama::dir_unik("jrg-range");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        let berkas = dir.join("uji.jrg");
        std::fs::write(&berkas, src).expect("tulis sumber");
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("ulimit -v 2000000 && exec \"$0\" \"$@\"")
            .arg(jrgc()).arg(&berkas);
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        let _ = std::fs::remove_dir_all(&dir);
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    }

    const MAKS: &str = "170141183460469231731687303715884105727";
    let harus: Vec<(String, String)> = vec![
        // Dulu mematikan prosesnya — sekarang ditolak SEBELUM satu elemen dibuat.
        ("range(5, 0, 0)".into(), "error: range: langkah tidak boleh 0".into()),
        ("range(9223372036854775807)".into(), "error: range: minta 9223372036854775807 elemen".into()),
        ("range(0, 3000000000)".into(), "error: range: minta 3000000000 elemen".into()),
        ("0..3000000000".into(), "error: range: minta 3000000000 elemen".into()),
        (format!("range_inclusive(0, {})", MAKS), "error: range_inclusive: minta".into()),
        (format!("0..={}", MAKS), "error: range_inclusive: minta".into()),
        // Bentuk argumen: satu kalimat, menyebut `range` dan posisinya.
        ("range()".into(), "error: `range` butuh 1 sampai 3 argumen".into()),
        // Argumen LEBIH dijawab pintu native (Fase TELANJANG): kalimat pertama yang sama
        // dengan builtin lain; bentuk sah `range` pindah ke baris bantuannya.
        ("range(1, 2, 3, 4)".into(), "error: `range` kebanyakan argumen".into()),
        ("range(0, 2.5)".into(), "error: range: argumen ke-2 butuh bilangan bulat, dapat pecahan".into()),
        ("range(true)".into(), "error: range: argumen ke-1 butuh bilangan bulat, dapat `bool`".into()),
        // Sisi POSITIF — penjaga yang menolak semuanya juga hijau di atas.
        (format!("range({}, {}, 5)", "170141183460469231731687303715884105726", MAKS),
            "[170141183460469231731687303715884105726]".into()),
        ("range(0, 10, 3)".into(), "[0, 3, 6, 9]".into()),
        ("range(5, 0, -2)".into(), "[5, 3, 1]".into()),
        ("range(3, 3)".into(), "[]".into()),
        ("range_inclusive(3, 3)".into(), "[3]".into()),
        ("range_inclusive(5, 0)".into(), "[]".into()),
        ("0..=3".into(), "[0, 1, 2, 3]".into()),
        ("-2..2".into(), "[-2, -1, 0, 1]".into()),
    ];
    let mut salah = Vec::new();
    for (ekspr, awal) in &harus {
        for vm in [false, true] {
            let keluar = jalan_terbatas(ekspr, vm);
            let j = jawaban_ringkas(&keluar);
            if keluar.contains("panicked") || keluar.contains("memory allocation of")
                || !j.starts_with(awal.as_str())
            {
                salah.push(format!("  {} ({}): harus diawali `{}`, dapat `{}`",
                    ekspr, if vm { "vm" } else { "int" }, awal, j));
            }
        }
    }
    assert!(salah.is_empty(), "{} kasus `range` salah:\n{}", salah.len(), salah.join("\n"));
}

/// **Nama yang ejaannya BENAR tapi pintunya salah menyebut pintu yang ada.**
#[test]
fn nama_skalar_yang_benar_menyebut_pintunya() {
    let mut diuji = 0;
    let mut salah = Vec::new();
    let jalan2 = |src: &str| (jalankan_keluaran_penuh(src, false), jalankan_keluaran_penuh(src, true));
    let prog = |v: &str, m: &str| format!("package main\npub fn main() {{\n    let v = {}\n    wok(v.{}())\n}}\n", v, m);
    let float = jrgc::interpreter::calon_method_skalar("float");
    // Method float yang juga ada di `math::`: untuk `int` jalannya `math::`.
    for nama in float {
        if jrgc::interpreter::calon_method_skalar("int").contains(nama) { continue; }
        if !jrgc::vm::ada_builtin(&format!("math::{}", nama)) { continue; }
        diuji += 1;
        let (i, v) = jalan2(&prog("5", nama));
        let harap = format!("method itu cuma milik `float`; untuk `int` pakai `math::{}`", nama);
        if !i.contains(&harap) || !v.contains(&harap) {
            salah.push(format!("  (5).{}(): interp={} vm={}", nama,
                jawaban_ringkas(&i), jawaban_ringkas(&v)));
        }
    }
    assert!(diuji >= 10, "cuma {} nama diuji — daftarnya menyusut", diuji);
    // Nama `math::` yang BUKAN method float: tidak boleh diklaim "milik float",
    // di penerima `int` maupun `float`.
    let mut lain = 0;
    for n in jrgc::vm::daftar_builtin() {
        let Some(nama) = n.strip_prefix("math::") else { continue };
        if float.contains(&nama) || jrgc::interpreter::calon_method_skalar("int").contains(&nama) { continue; }
        lain += 1;
        for penerima in ["5", "2.5"] {
            let (i, v) = jalan2(&prog(penerima, nama));
            let harap = format!("yang ada `math::{}`, bukan method", nama);
            if !i.contains(&harap) || !v.contains(&harap) || i.contains("milik `float`") {
                salah.push(format!("  ({}).{}(): interp={} vm={}", penerima, nama,
                    jawaban_ringkas(&i), jawaban_ringkas(&v)));
            }
        }
    }
    assert!(lain >= 15, "cuma {} nama `math::` lain diuji — daftarnya menyusut", lain);
    // `math::` menolak `bool` dan `rune`, jadi keduanya tidak boleh disuruh ke sana.
    for (penerima, nama) in [("true", "sqrt"), ("true", "gcd"), ("'a'", "floor"), ("'a'", "abs")] {
        let (i, v) = jalan2(&prog(penerima, nama));
        if i.contains("math::") || v.contains("math::") || !i.contains("tidak punya method") {
            salah.push(format!("  ({}).{}(): interp={} vm={}", penerima, nama,
                jawaban_ringkas(&i), jawaban_ringkas(&v)));
        }
    }
    // Sisi lain: nama yang MEMANG salah ketik tidak boleh dikarang punya
    // padanan `math::`, dan nama yang memang method float tidak boleh diganggu.
    for (src_ekspr, terlarang) in [("5", "kambing"), ("2.5", "sqrt")] {
        let src = format!("package main\npub fn main() {{\n    let v = {}\n    wok(v.{}())\n}}\n",
            src_ekspr, terlarang);
        let i = jalankan_keluaran_penuh(&src, false);
        if i.contains("math::kambing") {
            salah.push("  (5).kambing(): mengarang padanan math:: untuk nama yang tidak ada".into());
        }
        if src_ekspr == "2.5" && i.contains("tidak punya method") {
            salah.push("  (2.5).sqrt(): method float yang sah malah ditolak".into());
        }
    }
    assert!(salah.is_empty(), "{} kasus:\n{}", salah.len(), salah.join("\n"));
}

/// **Method skalar = `math::` yang sama: satu nilai, satu hitungan argumen.**
#[test]
fn metode_skalar_satu_implementasi_dengan_math() {
    let mut salah = Vec::new();
    let mut diuji = 0;
    for (tipe, nilai) in [("int", "-7"), ("float", "2.5"), ("float", "-2.5"), ("float", "0.0 / 0.0")] {
        for m in jrgc::interpreter::calon_method_skalar(tipe) {
            if *m == "to_string" { continue; }
            // `metode_skalar_math` tidak mencari registry saat jalan (jalur panas),
            // jadi bahwa tiap nama memang ada di `math::` dikunci DI SINI.
            assert!(jrgc::vm::ada_builtin(&format!("math::{}", m)),
                "method `{}` milik `{}` tidak punya `math::{}`", m, tipe, m);
            // Aritas bentuk method ditulis di sini, BUKAN dibaca dari tabel yang diuji.
            let n: usize = if *m == "pow" { 1 } else { 0 };
            let arg: Vec<&str> = (0..n).map(|_| "3").collect();
            let jawab = |ekspr: String| {
                let src = format!("package main\npub fn main() {{\n    let v = {}\n    wok({})\n}}\n", nilai, ekspr);
                (jawaban_ringkas(&jalankan_keluaran_penuh(&src, false)),
                 jawaban_ringkas(&jalankan_keluaran_penuh(&src, true)))
            };
            let (im, vm) = jawab(format!("v.{}({})", m, arg.join(", ")));
            let (ins, vns) = jawab(format!("math::{}(v{})", m,
                arg.iter().map(|a| format!(", {}", a)).collect::<String>()));
            if im != vm || ins != vns || im != ins {
                salah.push(format!("  ({}).{}() lawan math::{}: method i={} vm={} | ns i={} vm={}",
                    nilai, m, m, im, vm, ins, vns));
            }
            diuji += 1;
            // Kurang dan lebih: ditolak dua mesin, dengan kalimat METHOD yang sama.
            for k in [n.checked_sub(1), Some(n + 1)].into_iter().flatten() {
                let arg: Vec<&str> = (0..k).map(|_| "1").collect();
                let src = format!("package main\npub fn main() {{\n    let v = {}\n    wok(v.{}({}))\n}}\n",
                    nilai, m, arg.join(", "));
                let (i, v) = (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true));
                let harap = format!("method `{}` butuh {} argumen", m, n);
                if !i.contains(&harap) || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
                    salah.push(format!("  ({}).{}() dengan {} argumen: interp={} vm={}",
                        nilai, m, k, jawaban_ringkas(&i), jawaban_ringkas(&v)));
                }
            }
        }
    }
    assert!(diuji >= 30, "cuma {} kasus — daftarnya menyusut", diuji);
    // Yang dulu terpotong jadi `1.0` di bentuk method mesin acuan.
    let src = "package main\npub fn main() {\n    wok((2.0).pow(4294967296), math::pow(2.0, 4294967296))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(src, vm);
        if k.trim() != "inf inf" {
            salah.push(format!("  (2.0).pow(4294967296) {}: {}", if vm { "vm" } else { "interp" }, k.trim()));
        }
    }
    assert!(salah.is_empty(), "{} kasus:\n{}", salah.len(), salah.join("\n"));
}

/// **Argumen method bawaan dihitung DUA arah, di dua mesin, dengan satu
/// kalimat.**
#[test]
fn argumen_method_bawaan_dihitung_dua_arah() {
    const PRA: &str = "package main\nstruct P { pub a: int }\nenum E { A, B(int) }\npub fn main() {\n";
    const TAK_ADA: &str = "tidak punya method atau field bernama";
    // Variadik dengan sengaja: jumlah argumennya ditentukan templatenya.
    const VARIADIK: &[&str] = &["format"];
    let penerima: &[(&str, &str, &str)] = &[
        ("array", "[3, 1, 2]", "arr"), ("string", "\"halo\"", "str"),
        ("dict", "{\"a\": 1}", "dict"), ("some", "Option::Some(1)", "Option"),
        ("none", "Option::None", "Option"), ("ok", "Result::Ok(1)", "Result"),
        ("err", "Result::Err(\"x\")", "Result"), ("channel", "Chan::new(2)", "chan"),
        ("float", "2.5", "float"), ("int", "5", "int"), ("bool", "true", "bool"),
    ];
    let reg = jrgc::vm::daftar_builtin();
    let calon_arg = ["1", "\"a\"", "[1]", "|x| x", "\",\""];
    let jawab = |ekspr: &str, m: &str, args: &[&str]| {
        let src = format!("{}wok({}.{}({}))\n}}\n", PRA, ekspr, m, args.join(", "));
        (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true))
    };
    let diterima = |k: &str| !k.lines().any(|l| l.starts_with("error"));
    let (mut dicek, mut salah) = (0usize, Vec::new());
    for (tipe, ekspr, ns) in penerima {
        let mut nama: std::collections::BTreeSet<String> = match *ns {
            "float" | "int" | "bool" => jrgc::interpreter::calon_method_skalar(ns)
                .iter().map(|s| s.to_string()).collect(),
            _ => reg.iter().filter_map(|n| n.strip_prefix(&format!("{}::", ns)))
                .filter(|s| !s.starts_with("__")).map(|s| s.to_string()).collect(),
        };
        nama.extend(["len", "to_string", "fill", "starts_with", "ends_with",
                     "parse_int", "parse_float"].iter().map(|s| s.to_string()));
        for m in &nama {
            // `recv` MEMBLOKIR sampai ada isi: itu kontraknya, bukan cacat.
            if *tipe == "channel" && m == "recv" { continue; }
            let (i0, v0) = jawab(ekspr, m, &[]);
            if i0.contains(TAK_ADA) && v0.contains(TAK_ADA) { continue; }
            // Argumen dasar yang DITERIMA dua mesin, dicari dengan menjalankan.
            let mut dasar: Option<Vec<&str>> = None;
            'cari: for k in 0..=2usize {
                let mut idx = vec![0usize; k];
                loop {
                    let args: Vec<&str> = idx.iter().map(|&j| calon_arg[j]).collect();
                    let (i, v) = jawab(ekspr, m, &args);
                    if diterima(&i) && diterima(&v) { dasar = Some(args); break 'cari; }
                    let mut p = 0;
                    while p < k { idx[p] += 1; if idx[p] < calon_arg.len() { break; } idx[p] = 0; p += 1; }
                    if p == k { break; }
                }
            }
            let Some(dasar) = dasar else { continue };
            if VARIADIK.contains(&m.as_str()) { continue; }
            dicek += 1;
            // Tiga argumen lebih melampaui batas method mana pun.
            let mut lebih = dasar.clone();
            lebih.extend(["9", "9", "9"]);
            let (i, v) = jawab(ekspr, m, &lebih);
            let harap = format!("method `{}` butuh", m);
            if !i.contains(&harap) || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
                salah.push(format!("  ({}).{}({}): interp={} vm={}",
                    ekspr, m, lebih.join(", "), jawaban_ringkas(&i), jawaban_ringkas(&v)));
            }
        }
    }
    assert!(dicek >= 150, "cuma {} method diperiksa — sapuannya menyusut", dicek);
    // KURANG: argumen yang ketiadaannya membuat operasinya dikarang.
    for (ekspr, m) in [("Option::None", "unwrap_or"), ("Result::Err(1)", "unwrap_or"),
                       ("Option::Some(1)", "expect"), ("{\"a\": 1}", "get"),
                       ("{\"a\": 1}", "set"), ("Chan::new(1)", "send")] {
        let (i, v) = jawab(ekspr, m, &[]);
        if !i.contains(&format!("method `{}` butuh", m)) || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
            salah.push(format!("  ({}).{}(): interp={} vm={}", ekspr, m,
                jawaban_ringkas(&i), jawaban_ringkas(&v)));
        }
    }
    assert!(salah.is_empty(), "{} kasus:\n{}", salah.len(), salah.join("\n"));
}

/// **Argumen bertipe salah di `arr::`/`str::`: satu jawaban di dua mesin, dua
/// bentuk.**
#[test]
fn argumen_bertipe_salah_satu_jawaban_di_dua_mesin() {
    let reg = jrgc::vm::daftar_builtin();
    let jalan = |ekspr: &str| {
        let src = format!("package main\npub fn main() {{\n    let a = [1, 2, 3]\n    wok({})\n}}\n", ekspr);
        (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true))
    };
    let tolak = |k: &str| k.lines().any(|l| l.starts_with("error"));
    let (mut n, mut salah) = (0usize, Vec::new());
    for nama in &reg {
        let (ns, m) = match nama.split_once("::") { Some(x) => x, None => continue };
        if !matches!(ns, "arr" | "str") || m.starts_with("__") { continue; }
        let penerima = if ns == "arr" { "[1, 2, 3]" } else { "\"abc\"" };
        for pos in 1..=2usize {
            for buruk in ["1.5", "true", "\"x\""] {
                let mut args: Vec<&str> = std::iter::once(penerima).chain((0..pos).map(|_| "1")).collect();
                args[pos] = buruk;
                for ekspr in [format!("{}({})", nama, args.join(", ")),
                              format!("{}.{}({})", penerima, m, args[1..].join(", "))] {
                    let (i, v) = jalan(&ekspr);
                    n += 1;
                    if tolak(&i) != tolak(&v) || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
                        salah.push(format!("  {}\n    int: {}\n    vm : {}", ekspr,
                            jawaban_ringkas(&i), jawaban_ringkas(&v)));
                    }
                }
            }
        }
    }
    assert!(n > 1000, "cuma {} kasus — sapuannya menyusut", n);
    // Indeks negatif dihitung dari belakang di tiap pintu builtin, seperti `a[i]` (Fase KEPUTUSAN-1).
    for ekspr in ["a.remove(-1)", "arr::remove(a, -1)", "a.insert(-1, 9)", "arr::insert(a, -1, 9)",
                  "a.swap(-1, 0)", "arr::swap(a, -1, 0)", "a.get_or(-1, 0)", "a.get(-1)"] {
        let (i, v) = jalan(ekspr);
        if tolak(&i) || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
            salah.push(format!("  {}: int={} vm={}", ekspr, jawaban_ringkas(&i), jawaban_ringkas(&v)));
        }
    }
    // Yang salah argumen KEDUA, jadi yang disebut argumen ke-2.
    let (i, v) = jalan("arr::zip_with(a, 1, |x, y| x)");
    if !i.contains("argumen ke-2") || jawaban_ringkas(&i) != jawaban_ringkas(&v) {
        salah.push(format!("  zip_with: int={} vm={}", jawaban_ringkas(&i), jawaban_ringkas(&v)));
    }
    assert!(salah.is_empty(), "{} kasus:\n{}", salah.len(), salah.join("\n"));
}

/// **`json::set` menjawab objek BARU; `c.close()`/`c.send(v)` menjawab
/// `unit`.**
#[test]
fn json_set_murni_dan_kanal_menjawab_unit() {
    let src = "package main\npub fn main() {\n\
               let o = json::parse(\"{\\\"a\\\": 1}\")\n\
               let r = json::set(o, \"b\", 2)\n\
               wok(o, r)\n\
               let c = Chan::new(2)\n\
               wok(typeof(c.send(1)), typeof(c.close()))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(src, vm);
        assert!(k.contains("dict {a: 1} dict {a: 1, b: 2}") && k.contains("unit unit"),
            "{}: {}", if vm { "--vm" } else { "interpreter" }, k);
    }
}

/// **Calon saran untuk enum dan skalar benar-benar method-nya — dua arah.**
#[test]
fn calon_method_enum_dan_skalar_lengkap() {
    const TAK_ADA: &str = "tidak punya method atau field bernama";
    const PRA: &str = "package main\nenum W { A, B(int) }\npub fn main() {\n    let v = ";

    // (ekspresi penerima, daftar yang dijanjikan)
    let penerima: Vec<(&str, Vec<&str>)> = vec![
        ("Option::Some(1)", jrgc::interpreter::calon_method_enum("Option").to_vec()),
        ("Option::None", jrgc::interpreter::calon_method_enum("Option").to_vec()),
        ("Result::Ok(1)", jrgc::interpreter::calon_method_enum("Result").to_vec()),
        ("Result::Err(\"x\")", jrgc::interpreter::calon_method_enum("Result").to_vec()),
        ("W::A", jrgc::interpreter::calon_method_enum("W").to_vec()),
        ("W::B(1)", jrgc::interpreter::calon_method_enum("W").to_vec()),
        ("5", jrgc::interpreter::calon_method_skalar("int").to_vec()),
        ("2.5", jrgc::interpreter::calon_method_skalar("float").to_vec()),
        ("true", jrgc::interpreter::calon_method_skalar("bool").to_vec()),
        ("str::chars(\"a\")[0]", jrgc::interpreter::calon_method_skalar("rune").to_vec()),
    ];

    // Universumnya gabungan seluruh daftar plus nama HOF: sebuah method baru
    // yang lupa didaftarkan muncul di arah "ada tapi tidak dijanjikan".
    let mut semua: std::collections::BTreeSet<&str> = Default::default();
    for (_, daftar) in &penerima { semua.extend(daftar.iter().copied()); }
    semua.extend(jrgc::vm::Vm::NAMA_HOF.iter().copied());
    assert!(semua.len() > 20, "cuma {} nama disapu — universumnya menyusut", semua.len());

    let mut salah = Vec::new();
    for (ekspr, dijanjikan) in &penerima {
        for nama in &semua {
            let src = format!("{}{}\n    wok(v.{}())\n}}\n", PRA, ekspr, nama);
            let i = jalankan_keluaran_penuh(&src, false);
            let v = jalankan_keluaran_penuh(&src, true);
            let ada_i = !i.contains(TAK_ADA);
            let ada_v = !v.contains(TAK_ADA);
            if ada_i != ada_v {
                salah.push(format!("  {}.{}(): ada di interp={} vm={} — satu nama, dua nasib",
                    ekspr, nama, ada_i, ada_v));
                continue;
            }
            let dijanji = dijanjikan.contains(nama);
            if ada_i && !dijanji {
                salah.push(format!("  {}.{}(): METHOD-nya ada tapi tidak ada di daftar calon \
                    — salah ketiknya tidak akan pernah disarankan", ekspr, nama));
            } else if !ada_i && dijanji {
                salah.push(format!("  {}.{}(): ada di daftar calon tapi BUKAN method \
                    — sarannya menuntun ke pintu yang juga menolak", ekspr, nama));
            }
        }
    }
    assert!(salah.is_empty(), "{} ketidakcocokan daftar calon:\n{}",
        salah.len(), salah.join("\n"));
}

/// **Satu nama HOF, satu kontrak — bentuk namespace dan bentuk method.**
#[test]
fn kontrak_hof_sama_di_bentuk_namespace_dan_method() {
    // Urutan argumennya berbeda per nama (`fold(a, awal, f)` lawan
    // `reduce(a, f, awal)`), jadi tiap panggilan ditulis apa adanya alih-alih
    // dirakit dari aritasnya — merakitnya akan memanggil separuhnya terbalik.
    let bentuk: &[(&str, &str, &str)] = &[
        ("map",         "arr::map(a, |x| x * 2)",           "a.map(|x| x * 2)"),
        ("filter",      "arr::filter(a, |x| x > 1)",        "a.filter(|x| x > 1)"),
        ("any",         "arr::any(a, |x| x > 2)",           "a.any(|x| x > 2)"),
        ("all",         "arr::all(a, |x| x > 0)",           "a.all(|x| x > 0)"),
        ("find",        "arr::find(a, |x| x > 1)",          "a.find(|x| x > 1)"),
        ("find_idx",    "arr::find_idx(a, |x| x > 1)",      "a.find_idx(|x| x > 1)"),
        ("count_if",    "arr::count_if(a, |x| x > 1)",      "a.count_if(|x| x > 1)"),
        ("each",        "arr::each(a, |x| x)",              "a.each(|x| x)"),
        ("flat_map",    "arr::flat_map(a, |x| [x, x])",     "a.flat_map(|x| [x, x])"),
        ("partition",   "arr::partition(a, |x| x > 1)",     "a.partition(|x| x > 1)"),
        ("group_by",    "arr::group_by(a, |x| x % 2)",      "a.group_by(|x| x % 2)"),
        ("min_by",      "arr::min_by(a, |x| x)",            "a.min_by(|x| x)"),
        ("max_by",      "arr::max_by(a, |x| x)",            "a.max_by(|x| x)"),
        ("sort_by",     "arr::sort_by(a, |p, q| p - q)",    "a.sort_by(|p, q| p - q)"),
        ("sort_by_key", "arr::sort_by_key(a, |x| 0 - x)",   "a.sort_by_key(|x| 0 - x)"),
        ("reduce",      "arr::reduce(a, |s, x| s + x, 0)",  "a.reduce(|s, x| s + x, 0)"),
        ("fold",        "arr::fold(a, 100, |s, x| s + x)",  "a.fold(100, |s, x| s + x)"),
        ("zip_with",    "arr::zip_with(a, b, |p, q| p + q)", "a.zip_with(b, |p, q| p + q)"),
    ];
    // Penjaga derivasi: sebuah HOF yang punya bentuk `arr::` dan tidak punya
    // baris di sini adalah nama yang tidak tersapu sama sekali.
    for h in jrgc::vm::Vm::NAMA_HOF {
        if !jrgc::vm::ada_builtin(&format!("arr::{}", h)) { continue; }
        assert!(bentuk.iter().any(|(n, _, _)| n == h),
            "`arr::{}` ada tapi kontraknya tidak disapu — tambahkan barisnya di `bentuk`", h);
    }

    let mut salah = Vec::new();
    for (nama, ns, me) in bentuk {
        let src = format!(
            "package main\npub fn main() {{\n    let a = [3, 1, 2, 3]\n    let b = [10, 20, 30, 40]\n\
             \n    let ns = {}\n    let me = {}\n    wok(typeof(ns), str(ns))\n    wok(typeof(me), str(me))\n    wok(b)\n}}\n",
            ns, me);
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert!(i.sukses && v.sukses, "`{}` gagal jalan:\n  int: {}\n  vm : {}", nama, i.keluaran, v.keluaran);
        let bi: Vec<&str> = i.keluaran.lines().collect();
        if bi.len() < 2 || bi[0] != bi[1] {
            salah.push(format!("  {:<12} interp | namespace: {} | method: {}",
                nama, bi.first().unwrap_or(&""), bi.get(1).unwrap_or(&"")));
        }
        let bv: Vec<&str> = v.keluaran.lines().collect();
        if bv.len() < 2 || bv[0] != bv[1] {
            salah.push(format!("  {:<12} --vm   | namespace: {} | method: {}",
                nama, bv.first().unwrap_or(&""), bv.get(1).unwrap_or(&"")));
        }
    }
    assert!(salah.is_empty(),
        "{} bentuk HOF punya DUA kontrak untuk satu nama:\n{}", salah.len(), salah.join("\n"));
}

/// **Pintu pengurutan: satu jawaban di dua backend, untuk tiap BENTUK
/// nilai.**
#[test]
fn pintu_urut_satu_jawaban_di_dua_backend() {
    // Bentuk yang menyentuh tiap cabang urutan: yang terbanding, yang tidak,
    // dan yang cuma terbanding sebagian (NaN).
    let bentuk: &[(&str, &str)] = &[
        ("bool",       "[true, false, true]"),
        ("nan",        "[1.0, 0.0 / 0.0, 0.5]"),
        ("nil",        "[nil, 1, nil]"),
        ("unit",       "[wok_(\"\"), 1]"),
        ("array leks", "[[2, 1], [1, 9], [1, 2]]"),
        ("tuple leks", "[(2, 1), (1, 9), (1, 2)]"),
        ("int+float",  "[2, 1.5, 3, 0.5]"),
        ("int+string", "[2, \"a\", 1]"),
        ("1 dan 1.0",  "[1.0, 1, 0]"),
        ("string",     "[\"b\", \"a\", \"C\"]"),
        ("rune",       "['b', 'a', 'c']"),
        ("kosong",     "[]"),
    ];
    let pintu: &[&str] = &[
        "arr::sort(a)", "a.sort()", "arr::sort_desc(a)", "a.sort_desc()",
        "arr::min(a)", "arr::max(a)", "a.min()", "a.max()", "arr::median(a)",
        "a.sort_by_key(|x| x)", "arr::sort_by_key(a, |x| x)",
        "a.min_by(|x| x)", "arr::max_by(a, |x| x)", "a.binary_search(1)",
    ];
    let mut kasus = 0usize;
    let mut salah = Vec::new();
    for (nama, lit) in bentuk {
        for ekspr in pintu {
            let src = format!("package main\npub fn main() {{ let a = {}\n wok({}) }}\n", lit, ekspr);
            let i = jalankan_keluaran_penuh(&src, false);
            let v = jalankan_keluaran_penuh(&src, true);
            kasus += 1;
            // Panik bukan diagnostik JRG: tanpa kode, tanpa lokasi, tanpa caret.
            for (mesin, keluar) in [("interp", &i), ("--vm", &v)] {
                if keluar.contains("panicked at") {
                    salah.push(format!("  {:12} {:28} {} PANIK", nama, ekspr, mesin));
                }
            }
            if jawaban_ringkas(&i) != jawaban_ringkas(&v) {
                salah.push(format!("  {:12} {:28}\n      interp : {}\n      --vm   : {}",
                    nama, ekspr, jawaban_ringkas(&i), jawaban_ringkas(&v)));
            }
        }
    }
    assert!(kasus > 150, "cuma {} kasus — sapuannya sepertinya tidak berjalan", kasus);
    assert!(salah.is_empty(), "{} dari {} kasus pintu urut berbeda:\n{}",
        salah.len(), kasus, salah.join("\n"));
}

/// **Pintu konversi: satu jawaban di dua mesin — dan jawaban itu benar.**
#[test]
fn pintu_konversi_satu_jawaban_di_dua_mesin() {
    const NILAI: &[&str] = &[
        "math::NAN", "math::INF", "-math::INF", "1e40", "5.9", "-5.9", "true", "nil",
        "\"5\"", "\"5.9\"", "\"abc\"", "\"\"", "[1]", "Option::Some(1)", "300", "-1", "65",
    ];
    const PINTU: &[&str] = &[
        "int(X)", "X as int", "uint(X)", "X as uint", "float(X)", "X as float",
        "rune(X)", "X as rune", "X as byte", "X as u8", "X as i8", "X as i64", "X as usize",
        "str::to_int(X)", "parse_int(X)", "str::to_float(X)", "parse_float(X)",
    ];
    // Dijepit ke batas atau jadi 0 adalah hasil salah yang diam.
    const TAK_BERPADANAN: &[&str] = &["math::NAN", "math::INF", "-math::INF", "1e40"];
    const PINTU_BULAT: &[&str] = &[
        "int(X)", "X as int", "uint(X)", "X as uint", "X as byte", "X as u8", "X as i8",
        "X as i64", "X as usize", "str::to_int(X)", "parse_int(X)",
    ];
    let ditolak = |keluar: &str| keluar.lines().any(|l| l.starts_with("error"));
    let mut kasus = 0usize;
    let mut salah = Vec::new();
    for pintu in PINTU {
        for nilai in NILAI {
            let ekspr = pintu.replace('X', &format!("({})", nilai));
            let src = format!("package main\npub fn main() {{ let r = {}\n wok(typeof(r), r) }}\n", ekspr);
            let i = jalankan_keluaran_penuh(&src, false);
            let v = jalankan_keluaran_penuh(&src, true);
            kasus += 1;
            if jawaban_ringkas(&i) != jawaban_ringkas(&v) {
                salah.push(format!("  {:30}\n      interp : {}\n      --vm   : {}",
                    ekspr, jawaban_ringkas(&i), jawaban_ringkas(&v)));
            }
            if TAK_BERPADANAN.contains(nilai) && PINTU_BULAT.contains(pintu) && !ditolak(&i) {
                salah.push(format!("  {:30} DITERIMA: {}", ekspr, jawaban_ringkas(&i)));
            }
        }
    }
    // Yang bukan kode karakter dulu diam-diam jadi NUL; `4294967361` dipotong
    // jadi `A`. Batas atas yang SAH tetap diterima.
    for (kode, sah) in [("-1", false), ("55296", false), ("57343", false),
                        ("1114112", false), ("4294967361", false), ("1114111", true)] {
        for pintu in ["rune(X)", "X as rune"] {
            let src = format!("package main\npub fn main() {{ wok(({}) as int) }}\n",
                pintu.replace('X', &format!("({})", kode)));
            for vm in [false, true] {
                kasus += 1;
                if ditolak(&jalankan_keluaran_penuh(&src, vm)) == sah {
                    salah.push(format!("  {} atas {} ({}): harusnya {}", pintu, kode,
                        if vm { "--vm" } else { "interp" }, if sah { "diterima" } else { "ditolak" }));
                }
            }
        }
    }
    // Sasaran yang tidak bermakna ditolak SEBELUM programnya jalan, di dua mesin:
    // dulu interpreter menolak waktu jalan (dan `as unit` diam-diam jadi no-op).
    for sasaran in ["unit", "T", "[3]int", "dict"] {
        let src = format!("package main\nstruct T {{ pub x: int }}\npub fn main() {{ wok(\"mulai\")\n wok(5 as {}) }}\n", sasaran);
        for vm in [false, true] {
            kasus += 1;
            let keluar = jalankan_keluaran_penuh(&src, vm);
            if keluar.contains("mulai") || !keluar.contains("tidak didukung") {
                salah.push(format!("  `as {}` ({}): harus ditolak sebelum jalan;\n{}",
                    sasaran, if vm { "--vm" } else { "interp" }, keluar));
            }
        }
    }
    assert!(kasus > 250, "cuma {} kasus — sapuannya sepertinya tidak berjalan", kasus);
    assert!(salah.is_empty(), "{} dari {} kasus pintu konversi salah:\n{}",
        salah.len(), kasus, salah.join("\n"));
}

/// **Array campur tipe DITOLAK, bukan memanikkan prosesnya.**
#[test]
fn array_campur_tipe_ditolak_bukan_memanik() {
    const A: &str = "[3, 2.5, 'x', 1, 2, false, 2, \"c\", nil, 1, false, \"a\", \
                     1, 2, 2.5, 2.5, 2, \"a\", 2, false, 2.5, 1, nil, 2]";
    let pintu: &[&str] = &[
        "arr::sort(a)", "a.sort()", "a.sort_desc()", "arr::sort_desc(a)",
        "a.sort_by_key(|x| x)", "arr::sort_by_key(a, |x| x)", "arr::median(a)",
        "a.min_by(|x| x)", "arr::max_by(a, |x| x)", "arr::min(a)", "arr::max(a)",
    ];
    for ekspr in pintu {
        let src = format!("package main\npub fn main() {{ let a = {}\n wok({}) }}\n", A, ekspr);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        for (mesin, keluar) in [("interp", &i), ("--vm", &v)] {
            assert!(!keluar.contains("panicked at"),
                "{} atas `{}` MEMANIK — sebuah galat Rust, bukan diagnostik JRG:\n{}",
                mesin, ekspr, keluar);
            assert!(keluar.contains("error:"),
                "{} atas `{}` harusnya menolak; dapat:\n{}", mesin, ekspr, keluar);
        }
        assert_eq!(jawaban_ringkas(&i), jawaban_ringkas(&v),
            "`{}` dijawab berbeda di dua backend", ekspr);
    }
}

/// **Pengurutan STABIL, kunci dihitung sekali, dan pembanding dipanggil dalam
/// urutan yang sama — di dua backend dan di dua bentuk.**
#[test]
fn urut_stabil_dan_pembandingnya_dipanggil_seragam() {
    let program: &[(&str, &str)] = &[
        // `1` dan `1.0` dianggap sama: yang stabil mempertahankan urutan asalnya.
        ("sort_desc stabil", "let a = [1.0, 1, 0]\n wok(a.sort_desc(), arr::sort_desc(a))"),
        ("sort stabil",      "let a = [1, 1.0, 0]\n wok(a.sort(), arr::sort(a))"),
        // Urutan panggilan pembanding: efek sampingnya terlihat pemakai.
        ("urutan pembanding", "let a = [3, 1, 2, 1]\n              wok(a.sort_by(|x, y| { wok_(\"(\", x, \",\", y, \")\")\n return x - y }))\n              wok(arr::sort_by(a, |x, y| { wok_(\"(\", x, \",\", y, \")\")\n return x - y }))"),
        // Kunci dihitung SEKALI per elemen: empat elemen, empat panggilan.
        ("kunci sekali", "let a = [3, 1, 2, 1]\n var n = 0\n              let b = a.sort_by_key(|x| { n = n + 1\n return x })\n              wok(b, n)\n n = 0\n              let c = arr::sort_by_key(a, |x| { n = n + 1\n return x })\n wok(c, n)"),
        ("kunci sekali min_by", "let a = [3, 1, 2]\n var n = 0\n              wok(a.min_by(|x| { n = n + 1\n return x }), n)"),
    ];
    for (nama, badan) in program {
        let src = format!("package main\npub fn main() {{ {} }}\n", badan);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        assert_eq!(jawaban_ringkas(&i), jawaban_ringkas(&v),
            "[{}] dijawab berbeda di dua backend", nama);
        assert!(!i.contains("error:"), "[{}] harusnya jalan; dapat:\n{}", nama, i);
    }
}

/// Sebuah method ADA di dua mesin, atau tidak ada di dua-duanya.
#[test]
fn method_ada_di_dua_mesin_atau_tidak_sama_sekali() {
    const TAK_ADA: &str = "tidak punya method atau field bernama";
    const PRA: &str = "package main\nstruct P { pub a: int }\nenum E { A, B(int) }\npub fn main() {\n";

    let penerima: &[(&str, &str)] = &[
        ("array", "[3, 1, 2]"), ("array kosong", "[]"), ("string", "\"halo\""),
        ("dict", "dict::new()"), ("int", "5"), ("float", "2.5"), ("bool", "true"),
        ("tuple", "(1, 2)"), ("struct", "P { a: 1 }"), ("enum unit", "E::A"),
        ("enum isi", "E::B(7)"), ("some", "Option::Some(1)"), ("none", "Option::None"),
        ("ok", "Result::Ok(1)"), ("err", "Result::Err(\"x\")"), ("channel", "Chan::new(1)"),
    ];

    // Dua sumber, supaya tidak ada satu pun yang jadi titik buta sendirian:
    // segmen terakhir tiap nama di registry yang SEDANG BERJALAN, dan nama yang …
    let mut nama: std::collections::BTreeSet<String> = Default::default();
    for n in jrgc::vm::daftar_builtin() {
        for ns in ["arr::", "str::", "dict::", "chan::"] {
            if let Some(sisa) = n.strip_prefix(ns) {
                if !sisa.starts_with("__") { nama.insert(sisa.to_string()); }
            }
        }
    }
    let akar = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for berkas in ["src/interpreter/metode.rs", "src/interpreter/metode_str.rs"] {
        let isi = std::fs::read_to_string(akar.join(berkas))
            .unwrap_or_else(|e| panic!("baca {}: {}", berkas, e));
        // Bentuk lengannya `(…, "nama")`, jadi yang dipanen persis yang dipakai
        // dispatch-nya — bukan daftar tangan.
        for awalan in ["(Value::Array(a), \"", "(Value::Str(_), \"", "(s, \"",
                       "(_, \"", "(Value::Channel(_), \"", "(Value::Channel(c), \""] {
            let mut sisa = isi.as_str();
            while let Some(i) = sisa.find(awalan) {
                sisa = &sisa[i + awalan.len()..];
                if let Some(j) = sisa.find('"') {
                    let kata = &sisa[..j];
                    if !kata.is_empty()
                        && kata.len() < 25
                        && kata.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                        && !kata.starts_with('_')
                    {
                        nama.insert(kata.to_string());
                    }
                }
            }
        }
    }
    assert!(nama.len() > 100, "cuma {} nama — panennya sepertinya tidak berjalan", nama.len());

    // Dilewati dengan sebabnya, bukan dikecualikan diam-diam.
    // `recv` MEMBLOKIR sampai ada isi: itu kontraknya, bukan cacat.
    let lewati = |penerima: &str, nama: &str| penerima == "channel" && nama == "recv";

    // Yang memang masih berbeda DICATAT dan diperiksa MASIH berbeda — bentuk
    // yang sama dengan `BELUM_DIDUKUNG_LLVM`, supaya daftarnya tidak bisa jadi …
    const DIVERGEN_KANAL: &[&str] = &[];
    let mut kanal_masih_beda = vec![false; DIVERGEN_KANAL.len()];

    let mut salah = Vec::new();
    let mut diperiksa = 0usize;
    for (tipe, ekspr) in penerima {
        for n in &nama {
            if lewati(tipe, n) { continue; }
            diperiksa += 1;
            let src = format!("{}wok({}.{}())\n}}\n", PRA, ekspr, n);
            let i = jalankan_keluaran_penuh(&src, false);
            let v = jalankan_keluaran_penuh(&src, true);
            let (ada_i, ada_v) = (!i.contains(TAK_ADA), !v.contains(TAK_ADA));
            if ada_i != ada_v {
                if *tipe == "channel" {
                    if let Some(k) = DIVERGEN_KANAL.iter().position(|d| d == n) {
                        kanal_masih_beda[k] = true;
                        continue;
                    }
                }
                salah.push(format!(
                    "  {}.{}() — ada di {} saja\n    int: {}\n    vm : {}",
                    ekspr, n, if ada_i { "interpreter" } else { "--vm" },
                    jawaban_ringkas(&i), jawaban_ringkas(&v)));
            }
        }
    }
    assert!(diperiksa > 1500, "cuma {} kasus — sapuannya sepertinya tidak berjalan", diperiksa);
    assert!(salah.is_empty(), "{} dari {} pintu method cuma ada di satu mesin:\n{}",
        salah.len(), diperiksa, salah.join("\n"));
    // Arah sebaliknya: kalau salah satunya sudah sama, entrinya HARUS dihapus.
    // Sebuah daftar pengecualian yang cuma diperiksa satu arah jadi basi tanpa
    // ada yang memerah.
    for (k, n) in DIVERGEN_KANAL.iter().enumerate() {
        assert!(kanal_masih_beda[k],
            "KABAR BAIK: `c.{}()` sekarang sama di dua mesin. Hapus ia dari \
             `DIVERGEN_KANAL` dan dari catatan Fase KANAL di catatan proyek.", n);
    }
}

/// Salah ketik nama method mendapat SARAN, dan sarannya diturunkan dari
/// registry yang sedang berjalan.
#[test]
fn salah_ketik_method_dapat_saran_nama() {
    // (ekspresi, nama yang harus disarankan)
    let kasus: &[(&str, &str)] = &[
        ("wok([1,2,3].leng())",      "len"),
        ("wok([3,1,2].sortt())",     "sort"),
        ("wok(\"halo\".uppr())",     "upper"),
        ("wok(\"halo\".Len())",      "len"),
        // Kanvas: sarannya diturunkan dari registry `canvas::`, bukan dari
        // daftar tangan di sebelah `match` dispatch-nya.
        ("wok(Canvas::new(2,2).fill_rekt())", "fill_rect"),
    ];
    for (ekspr, harus) in kasus {
        let src = format!("package main\npub fn main() {{ {} }}\n", ekspr);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            assert!(keluar.contains(&format!("maksudnya `{}`?", harus)),
                "{} atas `{}`: harusnya menyarankan `{}`;\n{}",
                nama, ekspr, harus, keluar);
        }
    }
}

/// Saran untuk method sebuah struct datang dari method yang BENAR-BENAR
/// terdaftar untuk tipe itu — termasuk yang diwarisi.
#[test]
fn saran_method_struct_ikut_warisan() {
    let src = "package main\n\
               struct Hewan { pub nama: string }\n\
               impl Hewan { pub fn bersuara(self) -> string { return \"...\" } }\n\
               struct Anjing: Hewan { }\n\
               impl Anjing { pub fn menggonggong(self) -> string { return \"guk\" } }\n\
               pub fn main() {\n\
                   let a = Anjing{nama:\"rex\"}\n\
                   wok(a.bersuar())\n\
               }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("maksudnya `bersuara`?"),
            "{}: method WARISAN harus ikut jadi calon saran;\n{}", nama, keluar);
    }
}

/// Sebuah PATH tidak boleh diturunkan dari rendering sebuah nilai.
#[test]
fn path_canvas_harus_string_bukan_rendering_nilai() {
    // (ekspresi, apa yang harus disebut pesannya)
    let kasus: &[(&str, &str)] = &[
        ("c.save_ppm(5)",                "int"),
        ("c.save_bmp([1,2])",            "array"),
        ("c.save_png(Canvas::new(2,2))", "canvas"),
        ("c.set_headless_export(9)",     "int"),
    ];
    for (ekspr, tipe) in kasus {
        let src = format!(
            "package main\npub fn main() {{ let c = Canvas::new(4,4)\n wok({}) }}\n", ekspr);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            assert!(keluar.contains("path harus string"),
                "{} atas `{}`: path bukan-string harus DITOLAK, bukan ditulis;\n{}",
                nama, ekspr, keluar);
            assert!(keluar.contains(tipe),
                "{} atas `{}`: pesannya harus menyebut tipe `{}`;\n{}",
                nama, ekspr, tipe, keluar);
        }
    }
    // Sisi positifnya dikunci juga: sebuah path yang SAH tetap harus jalan.
    // Sebuah penjaga yang menolak semuanya juga membuat separuh tes di atas
    // hijau.
    let dir = std::env::temp_dir().join(format!("jrg-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("bikin dir");
    let target = dir.join("keluar.ppm");
    let src = format!(
        "package main\npub fn main() {{ let c = Canvas::new(4,4)\n wok(c.save_ppm(\"{}\")) }}\n",
        target.display());
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(&src, vm);
        assert!(keluar.contains("true"),
            "{}: path yang sah harus tetap bisa disimpan;\n{}", nama, keluar);
        assert!(target.exists(), "{}: berkasnya tidak benar-benar ditulis", nama);
        let _ = std::fs::remove_file(&target);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Tidak satu pun builtin boleh punya pesan galat yang TUMBUH mengikuti
/// besar datanya.**
#[test]
fn tidak_ada_pesan_builtin_yang_tumbuh_dengan_data() {
    fn pesan(nama: &str, n: usize) -> Option<String> {
        let src = format!(
            "package main\npub fn main() {{ wok({}(arr::fill(0, {}))) }}\n", nama, n);
        let keluar = jalankan_keluaran_penuh(&src, false);
        keluar.lines().find(|l| l.starts_with("error:")).map(|l| l.to_string())
    }

    let nama: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| !n.contains("__"))
        // Menyentuh siklus hidup proses/jendela/berkas, bukan pesan galat.
        .filter(|n| !matches!(n.as_str(),
            "std::exit" | "exit" | "canvas::update" | "canvas::is_open" | "canvas::close"))
        // `panik(pesan)` — pesannya MEMANG datanya.
        .filter(|n| n != "panik")
        .collect();
    assert!(nama.len() > 500, "daftar builtin sepertinya rusak: {}", nama.len());

    let mut tumbuh = Vec::new();
    for n in &nama {
        let (Some(kecil), Some(besar)) = (pesan(n, 10), pesan(n, 3000)) else { continue };
        let selisih = (besar.len() as i64 - kecil.len() as i64).abs();
        if selisih > 64 {
            tumbuh.push(format!("  {} — {} byte jadi {} byte (+{})",
                n, kecil.len(), besar.len(), selisih));
        }
    }
    assert!(tumbuh.is_empty(),
        "{} builtin punya pesan galat yang tumbuh mengikuti besar datanya \
         (sebut TIPE-nya, jangan isinya — atau potong, seperti `assert_eq`):\n{}",
        tumbuh.len(), tumbuh.join("\n"));
}

/// **Tidak satu pun builtin boleh menjawab dengan TANDA TANGANNYA saja.**
#[test]
fn tidak_ada_pesan_builtin_yang_cuma_tanda_tangan() {
    // Bentuk tanda tangan telanjang: seluruh teksnya `nama(...)` atau
    // `ns::nama(...)`, boleh diikuti ` -> tipe`.
    fn telanjang(pesan: &str) -> bool {
        let t = pesan.trim();
        if t.contains('\n') { return false; }
        let inti = match t.split_once(" -> ") { Some((k, _)) => k.trim(), None => t };
        if !inti.ends_with(')') { return false; }
        let Some(k) = inti.find('(') else { return false; };
        let kepala = &inti[..k];
        !kepala.is_empty()
            && kepala.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
    }

    let nama: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| !n.contains("__"))
        .filter(|n| !matches!(n.as_str(), "std::exit" | "exit"))
        // `proses::kill(5)` mengirim SIGTERM ke pid 5 di mesin mana pun yang
        // menjalankan suite ini, dan `proses::jalankan("x")` menjalankan `x`.
        .filter(|n| !n.starts_with("proses::"))
        // Menulis berkas di jalur yang diberikan (dan menghubungi jaringan).
        .filter(|n| n != "http::unduh")
        // Membunyikan speaker sungguhan.
        .filter(|n| !n.starts_with("suara::putar"))
        .collect();
    assert!(nama.len() > 500, "daftar builtin sepertinya rusak: {}", nama.len());

    // Beberapa BENTUK argumen salah, karena sebuah builtin bisa lolos satu
    // bentuk dan tersandung bentuk lain — pelajaran Fase BANTU, di mana
    // argumen bertipe salah SAJA membuat 62 asimetri tak terlihat.
    const BENTUK: &[&str] = &["", "5", "\"x\"", "[1,2]", "5, 5"];

    let mut telanjang_semua = Vec::new();
    for n in &nama {
        for b in BENTUK {
            let src = format!("package main\npub fn main() {{ wok({}({})) }}\n", n, b);
            let keluar = jalankan_keluaran_penuh(&src, false);
            let Some(baris) = keluar.lines().find(|l| l.starts_with("error:")) else { continue };
            let pesan = baris.trim_start_matches("error:").trim();
            if telanjang(pesan) {
                telanjang_semua.push(format!("  {}({}) -> {}", n, b, pesan));
            }
        }
    }
    assert!(telanjang_semua.is_empty(),
        "{} pesan galat isinya CUMA tanda tangan — sebuah tanda tangan tidak \
         menyebutkan apa yang salah:\n{}",
        telanjang_semua.len(), telanjang_semua.join("\n"));
}

/// **Sebuah assert tanpa argumen lengkap harus GAGAL, bukan lulus.**
#[test]
fn assert_tanpa_argumen_gagal_bukan_lulus() {
    for ekspr in ["assert_eq()", "assert_eq(1)", "assert()"] {
        let src = format!("package main\npub fn main() {{ {}\n wok(\"SAMPAI SINI\") }}\n", ekspr);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            assert!(!keluar.contains("SAMPAI SINI"),
                "{}: `{}` harus menghentikan program, bukan lulus diam-diam;\n{}",
                nama, ekspr, keluar);
            assert!(keluar.contains("butuh") && keluar.contains("argumen"),
                "{}: `{}` harus menyebut argumen yang kurang;\n{}", nama, ekspr, keluar);
        }
    }
    // Sisi positifnya dikunci: assert yang LENGKAP harus tetap jalan. Sebuah
    // penjaga yang menolak semuanya juga membuat separuh di atas hijau.
    let src = "package main\npub fn main() { assert_eq(2, 2)\n assert(true)\n wok(\"LULUS\") }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("LULUS"),
            "{}: assert yang lengkap harus tetap jalan;\n{}", nama, keluar);
    }
}

/// **`is_some` dan `is_none` tidak boleh sama-sama menjawab `false`.**
#[test]
fn predikat_tanpa_nilai_ditolak_bukan_dijawab() {
    let predikat = ["Option::is_some()", "Option::is_none()",
                    "Result::is_ok()", "Result::is_err()", "bool()", "dict::is_dict()"];
    for p in predikat {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", p);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            // Sejak Fase KURANG yang menolak pintu native, dengan kalimatnya sendiri.
            let fungsi = p.trim_end_matches("()");
            assert!(keluar.contains(&format!("`{}` butuh 1 argumen, dapat 0", fungsi)),
                "{}: `{}` harus ditolak, bukan dijawab;\n{}", nama, p, keluar);
        }
    }
    // Dan yang DIBERI nilai harus tetap saling berlawanan.
    let src = "package main\npub fn main() {\n \
               let s = Option::Some(1)\n let n = Option::None\n \
               wok(Option::is_some(s), Option::is_none(s))\n \
               wok(Option::is_some(n), Option::is_none(n))\n \
               wok(bool(1), bool(0)) }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("true false") && keluar.contains("false true"),
            "{}: is_some/is_none harus tetap berlawanan;\n{}", nama, keluar);
    }
}

/// **Argumen yang bertipe BENAR tapi KOSONG: sumbu sapuan yang tidak dimiliki
/// instrumen mana pun.**
#[test]
fn koleksi_kosong_diperlakukan_sama_di_dua_backend() {
    // Nama yang sengaja DILEWATI, masing-masing dengan sebabnya — bukan
    // dikecualikan lewat awalan nama.
    const LEWATI: &[&str] = &[
        // Menyentuh siklus hidup proses/jendela.
        "std::exit", "exit", "canvas::update", "canvas::is_open", "canvas::close",
    ];
    let nama: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| !n.contains("__"))
        // `py::`/`js::`/`php::` INTERPRETER-SAJA secara sengaja, dan alasannya
        // tercatat: protokol worker-nya harus memanggil balik sebuah fungsi JRG …
        .filter(|n| !n.starts_with("py::") && !n.starts_with("js::") && !n.starts_with("php::") && !n.starts_with("lua::"))
        .filter(|n| !LEWATI.contains(&n.as_str()))
        .collect();
    assert!(nama.len() > 500, "daftar builtin sepertinya rusak: {}", nama.len());

    // Penolakan dibaca dari STATUS KELUAR, bukan dari ada-tidaknya kata `error`
    // di keluaran.
    let mut beda = Vec::new();
    let tolak = |src: &str, vm: bool| -> bool { !jalankan(src, vm).sukses };
    for n in &nama {
        for bentuk in ["[]", "\"\"", "dict::new()"] {
            let src = format!("package main\npub fn main() {{ wok({}({})) }}\n", n, bentuk);
            let (it, vt) = (tolak(&src, false), tolak(&src, true));
            if it == vt { continue; }
            // Diulang sebelum dicatat: sebuah perbedaan yang tidak berulang adalah
            // pernyataan tentang mesinnya, bukan tentang bahasanya — dan sebuah …
            let (it2, vt2) = (tolak(&src, false), tolak(&src, true));
            if it2 == vt2 {
                eprintln!("catatan: {}({}) berselisih sekali lalu tidak berulang — \
                           dianggap gangguan mesin, tidak dihitung", n, bentuk);
                continue;
            }
            beda.push(format!("  {}({}) — interp {}, vm {}", n, bentuk,
                if it2 { "tolak" } else { "TERIMA" },
                if vt2 { "tolak" } else { "TERIMA" }));
        }
    }
    assert!(beda.is_empty(),
        "{} asimetri pada argumen bertipe benar tapi KOSONG:\n{}",
        beda.len(), beda.join("\n"));
}

/// **Sebuah operasi aritmetik atas array tidak boleh menjawab TEKS.**
#[test]
fn aritmetika_array_menolak_elemen_bukan_angka() {
    for ekspr in ["arr::sum([1, \"x\", 3])", "arr::product([2, \"x\", 3])",
                  "arr::mean([1, \"x\"])",  "arr::std_dev([1, \"x\"])"] {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            assert!(keluar.contains("error"),
                "{}: `{}` harus ditolak, bukan menjawab teks;\n{}", nama, ekspr, keluar);
        }
    }
    // Sisi positifnya dikunci: angka murni harus tetap jalan, dan `sum` atas
    // int harus tetap menjawab INT — memvalidasi lewat `f64` akan membuatnya
    // menjawab `6.0`, dan sebuah berkas konformans menguncinya sebagai `6`.
    let src = "package main\npub fn main() {\n \
               wok(arr::sum([1,2,3]), typeof(arr::sum([1,2,3])))\n \
               wok(arr::product([2,3]), arr::sum([]))\n \
               wok(arr::mean([1.0,2.0,3.0])) }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("6 int"),
            "{}: `arr::sum` atas int harus tetap menjawab int;\n{}", nama, keluar);
    }
}

/// **Argumen NUMERIK yang degenerate: nol dan negatif di posisi ukuran.**
#[test]
fn ukuran_nol_dan_negatif_diperlakukan_sama() {
    // (ekspresi, harus ditolak?)
    let kasus: &[(&str, bool)] = &[
        ("arr::chunks([1,2,3], 0)",  true),
        ("arr::windows([1,2,3], 0)", true),
        ("arr::take([1,2], -1)",     true),
        ("arr::skip([1,2], -1)",     true),
        ("arr::repeat([1], -1)",     true),
        ("arr::fill(0, -1)",         true),
        ("str::repeat(\"ab\", -1)",  true),
        // Sisi POSITIF: nilai yang wajar harus tetap jalan. Sebuah penjaga
        // yang menolak semuanya juga membuat separuh di atas hijau.
        ("arr::chunks([1,2,3], 2)",  false),
        ("arr::take([1,2,3], 2)",    false),
        ("arr::fill(7, 3)",          false),
        ("arr::repeat([1], 2)",      false),
    ];
    for (ekspr, harus_tolak) in kasus {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        let (it, vt) = (i.contains("error"), v.contains("error"));
        assert_eq!(it, vt,
            "`{}` — interpreter {}, vm {}\n--- interp ---\n{}\n--- vm ---\n{}",
            ekspr, if it { "tolak" } else { "TERIMA" },
            if vt { "tolak" } else { "TERIMA" }, i, v);
        assert_eq!(it, *harus_tolak,
            "`{}` seharusnya {};\n{}", ekspr,
            if *harus_tolak { "DITOLAK" } else { "diterima" }, i);
        if !harus_tolak {
            assert_eq!(i, v, "`{}`: keluarannya harus sama persis", ekspr);
        }
    }
}

/// **Sumbu keempat: UKURAN RAKSASA.
#[test]
fn ukuran_raksasa_ditolak_bukan_mematikan_proses() {
    // Angka yang PASTI gagal alokasi seketika, jadi tesnya sendiri tidak
    // pernah menekan memori mesin yang menjalankannya.
    let kasus = [
        "arr::fill(0, 1000000000000000000)",
        "arr::repeat([1], 1000000000000000000)",
        "Chan::new(1000000000000000000)",
        "str::pad_left(\"a\", 99999999999, \" \")",
        "str::pad_right(\"a\", 99999999999, \" \")",
        "str::center(\"a\", 99999999999, \" \")",
        "str::repeat(\"ab\", 99999999999)",
        "crypto::random_hex(1000000000000000000)",
        // Bentuk METHOD: di interpreter dulu salinan kedua tanpa batas (Fase LET-POLA).
        "\"a\".pad_left(99999999999)",
        "\"a\".pad_right(99999999999)",
        "\"a\".center(99999999999)",
        "[1].repeat(1000000000000000000)",
        // Batasnya atas TOTAL, bukan atas satu argumen.
        "arr::repeat(range(0, 1000), 16000000)",
        "wokwok::outer(wokwok::ones([100000]), wokwok::ones([100000]))",
        "wokwok::linspace(0, 1, 99999999999)",
        // Sumbu 0 membuat hasil kalinya 0, dan stride sumbu lainnya dulu meluap.
        "wokwok::full(range(0, 26), 7)",
        "wokwok::zeros(arr::fill(1, 40))",
    ];
    for ekspr in kasus {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        for (nama, vm) in [("interpreter", false), ("--vm", true)] {
            let keluar = jalankan_keluaran_penuh(&src, vm);
            assert!(!keluar.contains("panicked at") && !keluar.contains("memory allocation"),
                "{}: `{}` mematikan prosesnya sendiri alih-alih menjawab \
                 diagnostik;\n{}", nama, ekspr, keluar);
            assert!(keluar.contains("error"),
                "{}: `{}` harus DITOLAK dengan diagnostik;\n{}", nama, ekspr, keluar);
        }
    }
    // Sisi POSITIF: ukuran wajar harus tetap jalan, dan `take`/`skip` dengan
    // angka raksasa harus tetap berarti "seluruhnya" — aturan saturasi yang …
    let src = "package main\npub fn main() {\n \
               wok(arr::fill(7, 3))\n wok(str::pad_left(\"a\", 4, \"0\"))\n \
               wok(arr::repeat([1], 3))\n \
               wok(arr::take([1,2,3], 18446744073709551617)) }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("[7, 7, 7]") && keluar.contains("000a")
                && keluar.contains("[1, 2, 3]"),
            "{}: ukuran wajar dan saturasi `take` harus tetap jalan;\n{}", nama, keluar);
    }
}

/// **Tiap batas arity harus benar-benar BERLAKU, dan tidak boleh terlalu
/// kecil.**
#[test]
fn tiap_batas_arity_berlaku_dan_tidak_terlalu_ketat() {
    // Hanya nama `canvas::`: penerimanya seragam (sebuah Canvas di argumen 0)
    // sehingga argumen sisa boleh diisi angka tanpa mengubah arti.
    let entri: Vec<(String, usize)> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| n.starts_with("canvas::"))
        .filter(|n| !matches!(n.as_str(),
            // Menyentuh siklus hidup jendela / sistem berkas.
            "canvas::update" | "canvas::is_open" | "canvas::close"
            | "canvas::save_png" | "canvas::save_bmp" | "canvas::save_ppm"
            | "canvas::set_headless_export"))
        .filter_map(|n| jrgc::interpreter::arity::maks(&n).map(|m| (n, m)))
        .collect();
    assert!(entri.len() > 30,
        "cuma {} entri arity `canvas::` yang ketemu — sapuannya sepertinya \
         tidak berjalan, dan sapuan yang tidak berjalan selalu hijau", entri.len());

    let mut salah = Vec::new();
    for (nama, maks) in &entri {
        // Batas 0 (`canvas::punya_jendela`) berarti tanpa penerima sama sekali.
        let pas: Vec<String> = std::iter::once("c".to_string()).filter(|_| *maks > 0)
            .chain((1..*maks).map(|_| "0".to_string())).collect();
        let lebih: Vec<String> = pas.iter().cloned()
            .chain(std::iter::once("0".to_string())).collect();
        for (args, harus_tolak) in [(pas, false), (lebih, true)] {
            let src = format!(
                "package main\npub fn main() {{ let c = Canvas::new(4,4)\n wok({}({})) }}\n",
                nama, args.join(", "));
            let keluar = jalankan_keluaran_penuh(&src, false);
            let ditolak = keluar.contains("kebanyakan argumen");
            if ditolak != harus_tolak {
                salah.push(format!("  {}({}) — batas {}, {}",
                    nama, args.join(", "), maks,
                    if harus_tolak { "argumen berlebih TIDAK ditolak" }
                    else { "batasnya TERLALU KECIL: menolak pemanggilan yang sah" }));
            }
        }
    }
    assert!(salah.is_empty(), "{} batas arity tidak berlaku benar:\n{}",
        salah.len(), salah.join("\n"));
}

/// **Builtin TELANJANG menolak argumen lebih dan tidak mengarang yang kurang, di dua
/// mesin, satu kalimat.** Lahir dari `len([1], 2)` → `1` dan `parse_int("1010", 2)` →
/// `1010` — nol asimetri, jadi tak satu tabel paritas pun melihatnya.
#[test]
fn argumen_builtin_telanjang_dihitung_dua_arah() {
    let nama: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| !n.contains("::") && !n.starts_with("__")).collect();
    assert!(nama.len() >= 45, "cuma {} nama telanjang — sapuannya menyusut", nama.len());
    let prog = |e: &str| format!("package main\npub fn main() {{\n    wok({})\n}}\n", e);
    let dua = |e: &str| (jalankan_keluaran_penuh(&prog(e), false), jalankan_keluaran_penuh(&prog(e), true));
    let ditolak = |k: &str| k.lines().any(|l| l.starts_with("error"));
    let mut salah = Vec::new();
    let mut berbatas = 0;
    for n in &nama {
        // Tanpa argumen: dua mesin menjawab IDENTIK (dulu 7 nama berbeda kalimat).
        let (i0, v0) = dua(&format!("{}()", n));
        if jawaban_ringkas(&i0) != jawaban_ringkas(&v0) {
            salah.push(format!("  {}(): i=[{}] v=[{}]", n, jawaban_ringkas(&i0), jawaban_ringkas(&v0)));
        }
        if let Some(m) = jrgc::interpreter::arity::maks(n) {
            berbatas += 1;
            let nol = |k: usize| vec!["0"; k].join(", ");
            // Tepat di batas: tidak boleh dijawab "kebanyakan" (batasnya tidak terlalu kecil).
            let (ip, vp) = dua(&format!("{}({})", n, nol(m)));
            if ip.contains("kebanyakan argumen") || vp.contains("kebanyakan argumen") {
                salah.push(format!("  {}({}) — batas {} TERLALU KECIL", n, nol(m), m));
            }
            // Lewat batas: ditolak di dua mesin, kalimat identik, menyebut nama YANG DITULIS.
            let (il, vl) = dua(&format!("{}({})", n, nol(m + 1)));
            let (bi, bv) = (jawaban_ringkas(&il), jawaban_ringkas(&vl));
            if !bi.contains("kebanyakan argumen") || bi != bv || !bi.contains(&format!("`{}`", n)) {
                salah.push(format!("  {}({}): i=[{}] v=[{}]", n, nol(m + 1), bi, bv));
            }
            continue;
        }
        // Tanpa batas: argumen ekstra harus DITOLAK atau benar-benar DIPAKAI.
        let dasar = ["", "1", "\"a\"", "[1, 2]", "1, 2", "1, 2, 3"].into_iter().find(|b| {
            let (i, v) = dua(&format!("{}({})", n, b));
            !ditolak(&i) && !ditolak(&v)
        });
        let Some(b) = dasar else {
            salah.push(format!("  {}: bentuk dasarnya tidak ketemu — tambahkan ke daftar dasar", n));
            continue;
        };
        let lebih = if b.is_empty() { "7".to_string() } else { format!("{}, 7", b) };
        for vm in [false, true] {
            let a = jalankan_keluaran_penuh(&prog(&format!("{}({})", n, b)), vm);
            let l = jalankan_keluaran_penuh(&prog(&format!("{}({})", n, lebih)), vm);
            if !ditolak(&l) && a == l {
                salah.push(format!("  {}({}) di {}: argumen ekstra DIBUANG diam-diam",
                    n, lebih, if vm { "--vm" } else { "interpreter" }));
            }
        }
    }
    assert!(berbatas >= 20, "cuma {} nama berbatas yang disapu", berbatas);
    // Argumen yang dulu dikarang: `typeof()` menjawab "nil", `str()` "nil" lawan "".
    for e in ["typeof()", "str()"] {
        let (i, v) = dua(e);
        if !ditolak(&i) || !ditolak(&v) { salah.push(format!("  {} masih mengarang argumennya", e)); }
    }
    assert!(salah.is_empty(), "{} masalah:\n{}", salah.len(), salah.join("\n"));
}

/// **Tiga jebakan dari bahasa lain dijawab dengan jalan keluarnya, lewat pintu mana pun.**
/// Langsung, lewat `terap`, lewat variabel, dan bentuk namespace di berkas ber-`import`
/// (tempat parser mundur) — satu kalimat di dua mesin, dan `len(c)` sama dengan `c.len()`.
#[test]
fn argumen_lebih_dijelaskan_di_tiap_pintu() {
    let kasus: &[(&str, &str, &str)] = &[
        ("wok(parse_int(\"1010\", 2))", "`parse_int` kebanyakan", "dok::num_from_base"),
        ("wok(int(\"10\", 2))", "`int` kebanyakan", "dok::num_from_base"),
        ("wok(enumerate([\"a\"], 1))", "`enumerate` kebanyakan", "for i, x in a"),
        ("wok(zip([1], [2], [3]))", "`zip` kebanyakan", "b[i]"),
        ("wok(terap(len, [[1], 2]))", "`len` kebanyakan", "buang 1 argumen"),
        ("let f = parse_int\n    wok(f(\"1\", 2))", "`parse_int` kebanyakan", "dok::num_from_base"),
        ("let len = |a, b| a + b\n    wok(len(1, 2))", "", ""),
    ];
    for (badan, harus, bantuan) in kasus {
        let src = format!("package main\npub fn main() {{\n    {}\n}}\n", badan);
        let (i, v) = (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true));
        assert_eq!(jawaban_ringkas(&i), jawaban_ringkas(&v), "{}: dua mesin berbeda", badan);
        if harus.is_empty() {
            assert_eq!(i.trim(), "3", "`len` yang dibayangi closure pemakai harus tetap jalan:\n{}", i);
            continue;
        }
        assert!(jawaban_ringkas(&i).contains(harus) && i.contains(bantuan),
            "{}: harus menyebut {} dan {}:\n{}", badan, harus, bantuan, i);
    }
    // Berkas ber-`import`: pemeriksa parser mundur, jadi pintu native yang menjawab —
    // dengan nama yang DITULIS, bukan kembarannya yang lebih pendek.
    let src = "package main\nimport \"std/set\"\npub fn main() {\n    wok(str::to_int(\"5\", 9))\n}\n";
    let (i, v) = (jalankan_keluaran_penuh(src, false), jalankan_keluaran_penuh(src, true));
    assert_eq!(jawaban_ringkas(&i), jawaban_ringkas(&v));
    assert!(jawaban_ringkas(&i).contains("`str::to_int` kebanyakan"), "{}", i);
    // `range` tetap menyebut bentuk sahnya — dari konstanta yang sama dengan kalimat hitungnya.
    let src = "package main\npub fn main() {\n    wok(range(1, 2, 3, 4))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(src, vm);
        assert!(k.contains("`range` kebanyakan") && k.contains("(awal, akhir, langkah)"), "vm={}:\n{}", vm, k);
    }
    // Keluarga cetak menjawab `unit` lewat nama mana pun — `fmt::eprint` dulu salinan
    // kedua di VM yang menjawab `nil`.
    let src = "package main\npub fn main() {\n    wok(typeof(fmt::eprint(1)), typeof(cetak_galat(1)), typeof(fmt::wok(1)))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(src, vm);
        assert!(k.contains("unit unit unit"), "vm={}:\n{}", vm, k);
    }
    let src = "package main\npub fn main() {\n    let c = make_chan(int, 2)\n    c <- 1\n    wok(len(c), c.len())\n    wok(len(5))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(src, vm);
        assert!(k.starts_with("1 1\n") && k.contains("dict, dan channel"), "vm={}:\n{}", vm, k);
    }
}

include!("bersama/contoh_argumen.rs");

/// Banyak panggilan dalam SATU proses per mesin: tiap badan jadi fungsi `test_*`, dan
/// `--uji` menangkap kegagalannya per fungsi. `None` lulus, `Some(baris)` gagal.
fn jalankan_uji_batch(badan: &[String], pakai_vm: bool) -> Vec<Option<String>> {
    let dir = crate::bersama::dir_unik("jrg-uji-batch");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let src: String = badan.iter().enumerate()
        .map(|(i, b)| format!("fn test_{}() {{\n    {}\n}}\n", i, b)).collect();
    std::fs::write(dir.join("uji.jrg"), src).expect("tulis sumber");
    let mut cmd = Command::new(jrgc());
    // Direktori kerjanya sendiri: contoh `fs::` menulis berkas di sini.
    cmd.current_dir(&dir).arg("uji.jrg").arg("--uji");
    if pakai_vm { cmd.arg("--vm"); }
    let out = jalankan_berbatas(cmd);
    let _ = std::fs::remove_dir_all(&dir);
    let teks = String::from_utf8_lossy(&out.stdout);
    let mut hasil: Vec<Option<Option<String>>> = vec![None; badan.len()];
    for l in teks.lines() {
        let l = l.trim_start();
        if let Some(sisa) = l.strip_prefix("ok    test_") {
            if let Ok(i) = sisa.trim().parse::<usize>() { hasil[i] = Some(None); }
        } else if let Some(sisa) = l.strip_prefix("GAGAL test_") {
            let (kepala, pesan) = sisa.split_once(" — ").unwrap_or((sisa, ""));
            let no = kepala.split_whitespace().next().unwrap_or("");
            if let Ok(i) = no.parse::<usize>() { hasil[i] = Some(Some(pesan.to_string())); }
        }
    }
    let hilang: Vec<usize> = (0..badan.len()).filter(|i| hasil[*i].is_none()).collect();
    assert!(hilang.is_empty(), "vm={}: {} dari {} fungsi tidak melapor — prosesnya mati atau \
        menggantung, jadi yang sesudahnya tidak pernah jalan (mis. `{}`)",
        pakai_vm, hilang.len(), badan.len(), badan[hilang[0]]);
    hasil.into_iter().map(|h| h.unwrap_or(None)).collect()
}

/// **Ekor `main` yang berupa nama fungsi telanjang ditolak**, seperti bentuk
/// pernyataannya sejak Fase KURUNG — `pub fn main() { f }` dulu diam di dua mesin.
#[test]
fn ekor_main_yang_berupa_fungsi_ditolak() {
    let tolak = [
        "package main\nfn f() { wok(\"dipanggil\") }\npub fn main() {\n    wok(\"a\")\n    f\n}\n",
        "package main\nfn f() { wok(\"dipanggil\") }\npub fn main() = f\n",
        "package main\nstruct P { pub a: int }\nimpl P { fn m(self) { wok(\"m\") } }\npub fn main() {\n    let p = P { a: 1 }\n    p.m\n}\n",
    ];
    for src in tolak {
        let (i, v) = (jalankan_keluaran_penuh(src, false), jalankan_keluaran_penuh(src, true));
        let galat = |t: &str| t.lines().find(|l| l.contains("error")).unwrap_or("").to_string();
        assert!(galat(&i).contains("tanpa `()`") || galat(&i).contains("berdiri sendiri"),
                "ekor main harus ditolak:\n{}", i);
        assert_eq!(galat(&i), galat(&v), "kalimat beda antar-mesin:\n{}\n{}", i, v);
        assert!(!i.contains("dipanggil"), "{}", i);
    }
    // Sisi positif: `Err` di ekor tetap dilaporkan, nama bukan-fungsi tetap jalan.
    let err = "package main\npub fn main() {\n    let r = Result::Err(\"x\")\n    r\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(err, vm);
        assert!(k.contains("merambat keluar dari `main`"), "vm={}: {}", vm, k);
    }
    let biasa = "package main\npub fn main() {\n    let n = 5\n    wok(n)\n    n\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(biasa, vm);
        assert_eq!(k.trim(), "5", "vm={}: {}", vm, k);
    }
}

/// `proses::keluaran` dengan opsi: stdin sampai, batas waktu MENGHENTIKAN programnya dan
/// melaporkannya, opsi asing ditolak — sama di dua mesin.
#[test]
fn proses_keluaran_opsi_di_dua_mesin() {
    if !cfg!(unix) { return; }
    let program = "package main\npub fn main() {\n\
        let a = proses::keluaran(\"cat\", [], {\"masukan\": \"dari stdin\"}).unwrap()\n\
        wok(a.keluaran, a.sukses, a.habis_waktu)\n\
        let t0 = time::now_ms()\n\
        let b = proses::keluaran(\"sleep\", [\"30\"], {\"batas_ms\": 300}).unwrap()\n\
        wok(b.sukses, b.habis_waktu, time::now_ms() - t0 < 5000)\n\
        let c = proses::keluaran(\"pwd\", [], {\"dir\": \"/\", \"env\": {\"X\": \"1\"}}).unwrap()\n\
        wok(str::trim(c.keluaran))\n\
        let d = proses::keluaran(\"true\", [], {\"batas\": 5})\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(program, vm);
        assert!(k.starts_with("dari stdin true false\nfalse true true\n/\n"), "vm={}: {}", vm, k);
        assert!(k.contains("opsi `batas` tidak dikenal") && k.contains("`batas_ms`"), "vm={}: {}", vm, k);
    }
}

/// `proses::mulai` dan kawan-kawannya: keluaran terbaca SELAGI program jalan, stdin sampai di tengah
/// jalan, `hentikan` dan `batas_ms` benar-benar menghentikan, nomor yang sudah selesai hangus.
#[test]
fn proses_mengalir_di_dua_mesin() {
    if !cfg!(unix) { return; }
    let program = "package main\n\
        fn tunggu(p) {\n    var k = \"\"\n    for i in 0..1000 {\n        let b = proses::baca(p).unwrap()\n        \
        k = k + b.keluaran\n        if b.selesai { return [k, b.sukses, b.habis_waktu] }\n        time::sleep(0.01)\n    }\n    \
        return [k, \"tidak selesai\"]\n}\n\
        pub fn main() {\n\
        let p = proses::mulai(\"cat\").unwrap()\n\
        proses::tulis(p, \"satu\\n\").unwrap()\n\
        var k = \"\"\n\
        for i in 0..1000 {\n    let b = proses::baca(p).unwrap()\n    k = k + b.keluaran\n    \
        if k != \"\" { wok(str::trim(k), b.selesai)\n break }\n    time::sleep(0.01)\n}\n\
        proses::tulis(p, \"dua\\n\").unwrap()\n\
        proses::tutup_masukan(p).unwrap()\n\
        wok(tunggu(p))\n\
        match proses::baca(p) { Ok(b) => wok(\"masih ada\"), Err(e) => wok(e) }\n\
        let q = proses::mulai(\"sleep\", [\"30\"]).unwrap()\n\
        wok(proses::hentikan(q).unwrap())\n\
        wok(tunggu(q))\n\
        let r = proses::mulai(\"sleep\", [\"30\"], {\"batas_ms\": 200}).unwrap()\n\
        wok(tunggu(r))\n\
        wok(proses::baca(\"x\"))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(program, vm);
        assert!(k.starts_with("satu false\n[dua\n, true, false]\nproses::baca: tidak ada proses bernomor 1"), "vm={}: {}", vm, k);
        assert!(k.contains("\ntrue\n[, false, false]\n[, false, true]\n"), "vm={}: {}", vm, k);
        assert!(k.contains("harus nomor proses dari `proses::mulai`"), "vm={}: {}", vm, k);
    }
}

/// Angka di luar 64 bit tidak dipotong diam-diam: `future::timeout(f, 2^64)` dulu menunggu 0 ms,
/// dan `Ptr` ber-`id` 2^64+1 dulu membaca alokasi nomor 1 — sejak Fase PEGANGAN `id`-nya tidak bisa ditulis.
#[test]
fn angka_di_luar_64_bit_tidak_dipotong_di_dua_mesin() {
    let program = "package main\n\
        pub fn main() {\n\
        let f = Future::new(|| { time::sleep(0.2)\n 7 })\n\
        wok(future::timeout(f, 18446744073709551616))\n\
        wok(future::timeout(Future::new(|| 1), 170141183460469231731687303715884105727))\n\
        let j = jawa::alloc(1)\n\
        let p = ptr::new(5)\n\
        var q = p\n\
        q.id = 18446744073709551616 + p.id\n\
        wok(ptr::get(q))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(program, vm);
        assert!(k.starts_with("Option::Some(7)\nOption::Some(1)\n"), "vm={}: {}", vm, k);
        assert!(k.contains("`Ptr` adalah handle buatan pustaka — field `id` tidak bisa ditulis"), "vm={}: {}", vm, k);
    }
}

/// Opsi `kelompok`: `hentikan` mematikan juga proses yang dimulai program itu sendiri, dan bacaan
/// sesudahnya tidak menggantung menunggu pipa yang masih dipegang turunannya.
/// Hidupnya ditanya ke `ps`, bukan `kill -0`: di PID namespace tanpa init, yang mati tinggal zombie.
#[test]
fn proses_kelompok_menghentikan_turunan_di_dua_mesin() {
    if !cfg!(unix) { return; }
    let program = "package main\n\
        pub fn main() {\n\
        let p = proses::mulai(\"sh\", [\"-c\", \"sleep 30 & echo $!; wait\"], {\"kelompok\": true}).unwrap()\n\
        var pid = \"\"\n\
        for i in 0..1000 {\n    pid = pid + proses::baca(p).unwrap().keluaran\n    \
        if str::contains(pid, \"\\n\") { break }\n    time::sleep(0.01)\n}\n\
        wok(str::trim(pid) != \"\", proses::hentikan(p).unwrap())\n\
        var selesai = false\n\
        for i in 0..1000 {\n    if proses::baca(p).unwrap().selesai { selesai = true\n break }\n    time::sleep(0.01)\n}\n\
        var hidup = true\n\
        for i in 0..300 {\n    \
        let st = str::trim(proses::keluaran(\"ps\", [\"-o\", \"stat=\", \"-p\", str::trim(pid)]).unwrap().keluaran)\n    \
        hidup = st != \"\" && !str::starts_with(st, \"Z\")\n    \
        if !hidup { break }\n    time::sleep(0.01)\n}\n\
        wok(selesai, hidup)\n\
        wok(proses::mulai(\"true\", [], {\"kelompok\": 1}))\n}\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(program, vm);
        assert!(k.starts_with("true true\ntrue false\n"), "vm={}: {}", vm, k);
        assert!(k.contains("opsi `kelompok` salah tipe"), "vm={}: {}", vm, k);
    }
}

/// `fs::write_base64`/`append_base64`/`read_base64`: byte APA PUN (bukan UTF-8 juga) sampai utuh di
/// berkas dan kembali; base64 yang rusak dan jalur yang tidak ada dijawab `Err`, bukan menulis separuh.
#[test]
fn berkas_biner_lewat_base64_di_dua_mesin() {
    for vm in [false, true] {
        let d = std::env::temp_dir().join(format!("jrg-b64-{}-{}", std::process::id(), vm));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("b.bin");
        let program = format!("package main\npub fn main() {{\n\
            let f = \"{}\"\n\
            wok(fs::write_base64(f, \"AP+AQQ==\"))\n\
            wok(fs::append_base64(f, \"Qg==\"))\n\
            wok(fs::read_base64(f))\n\
            wok(fs::write_base64(f, \"bukan base64!\").is_err())\n\
            wok(fs::read_base64(f))\n\
            wok(fs::read_base64(f + \".tidak-ada\").is_err())\n\
            wok(fs::write_base64(1, \"QQ==\"))\n}}\n", f.display());
        let k = jalankan_keluaran_penuh(&program, vm);
        assert!(k.starts_with("Result::Ok(4)\nResult::Ok(1)\nResult::Ok(AP+AQUI=)\ntrue\nResult::Ok(AP+AQUI=)\ntrue\n"), "vm={}: {}", vm, k);
        assert!(k.contains("fs::write_base64: argumen ke-1 harus string, dapat `int`"), "vm={}: {}", vm, k);
        assert_eq!(std::fs::read(&f).unwrap(), [0u8, 0xff, 0x80, b'A', b'B']);
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// **Tanda tangan `--doc` menyebut tipe yang BENAR-BENAR dijawab.** Tiap
/// contoh benar di `CONTOH_ARGUMEN` dijalankan di dua mesin dan `typeof`-nya …
#[test]
fn tanda_tangan_dok_menyebut_tipe_yang_dijawab() {
    // Tipe tertulis -> tipe `typeof` yang diterimanya. `nilai`/`any` berarti apa pun.
    fn cocok(tertulis: &str, nyata: &str) -> bool {
        let t = tertulis.trim();
        if t.contains('|') { return t.split('|').any(|b| cocok(b, nyata)); }
        if t.starts_with('(') && t.ends_with(')') { return nyata == "tuple"; }
        let dasar = t.split('<').next().unwrap_or(t);
        match dasar {
            "nilai" | "any" => true,
            "number" => nyata == "int" || nyata == "float",
            "nd" => nyata.starts_with("wokwok:"),
            "Option" | "Result" => nyata == format!("enum:{}", dasar),
            "array" | "[]string" => nyata == "array",
            lain => nyata == lain || nyata == format!("struct:{}", lain),
        }
    }
    // `term::` menulis ke terminal tanpa baris baru, jadi baris laporan `--uji`
    // sesudahnya tertempel dan tidak terbaca.
    let contoh: Vec<_> = CONTOH_ARGUMEN.iter().filter(|(n, _, _)| !n.starts_with("term::")).collect();
    let badan: Vec<String> = contoh.iter()
        .map(|(n, p, a)| format!("{}\n    panik(\"TIPE=\" + typeof({}({})))", p, n, a.join(", ")))
        .collect();
    let (ri, rv) = (jalankan_uji_batch(&badan, false), jalankan_uji_batch(&badan, true));
    let mut salah = Vec::new();
    let mut diperiksa = 0;
    for (i, (n, _, a)) in contoh.iter().enumerate() {
        let tipe = |r: &Option<String>| r.as_deref()
            .and_then(|m| m.split("TIPE=").nth(1)).map(|t| t.trim().to_string());
        let (ti, tv) = (tipe(&ri[i]), tipe(&rv[i]));
        if ti != tv {
            salah.push(format!("  {}({}) — beda antar-mesin: i={:?} v={:?}", n, a.join(", "), ri[i], rv[i]));
            continue;
        }
        let Some(t) = ti else { continue };
        let Some(info) = jrgc::bantu::DOKUMEN.iter().find(|d| d.nama == *n) else { continue };
        let Some((_, ditulis)) = info.tanda_tangan.rsplit_once("->") else {
            salah.push(format!("  {:26} tanda tangannya tidak menyebut tipe hasil: `{}`", n, info.tanda_tangan));
            continue;
        };
        diperiksa += 1;
        if !cocok(ditulis, &t) {
            salah.push(format!("  {:26} ditulis `{}`, dijawab `{}`", n, ditulis.trim(), t));
        }
    }
    assert!(diperiksa > 500, "cuma {} tanda tangan yang diperiksa — sapuannya menyusut", diperiksa);
    assert!(salah.is_empty(), "{} tanda tangan tidak cocok dengan jawabannya:\n{}",
        salah.len(), salah.join("\n"));
}

/// **Argumen KURANG ditolak di pintu native dua mesin, satu kalimat — dan tak ada builtin
/// yang sempat mengarangnya** (dulu 31: `env::get()` → `nil`, `ptr::set(p)` mengisi `nil`).
/// Contohnya diturunkan dengan menjalankan;
#[test]
fn argumen_kurang_satu_kalimat_di_dua_mesin() {
    use jrgc::interpreter::arity::minimal;
    let registry: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| !n.starts_with("__")).collect();
    let ada = |n: &str| registry.iter().any(|r| r == n);
    let mut salah = Vec::new();
    // Kelengkapan dua arah: nama berbatas bawah tanpa contoh maupun sebab, dan entri
    // yang namanya hilang atau jumlah argumennya bukan lagi batas bawahnya.
    let tercatat = |n: &str| CONTOH_ARGUMEN.iter().any(|(c, _, _)| *c == n)
        || TANPA_CONTOH_ARGUMEN.iter().chain(LEWATI_ARGUMEN).any(|(c, _)| *c == n);
    for n in &registry {
        if minimal(n).unwrap_or(0) >= 1 && !tercatat(n) {
            salah.push(format!("  `{}` berbatas bawah {:?} tanpa contoh maupun sebab tertulis", n, minimal(n)));
        }
    }
    for (n, _, a) in CONTOH_ARGUMEN {
        if !ada(n) || minimal(n) != Some(a.len()) {
            salah.push(format!("  contoh `{}`: {} argumen, batas bawah {:?}, terdaftar {}", n, a.len(), minimal(n), ada(n)));
        }
    }
    for (n, _) in TANPA_CONTOH_ARGUMEN.iter().chain(LEWATI_ARGUMEN) {
        if !ada(n) || minimal(n).unwrap_or(0) == 0 {
            salah.push(format!("  entri `{}` basi: terdaftar {}, batas bawah {:?}", n, ada(n), minimal(n)));
        }
    }
    // Positif: tepat `minimal` argumen DITERIMA dua mesin — batasnya tidak terlalu besar
    // untuk badan yang memang butuh segitu.
    let pos: Vec<String> = CONTOH_ARGUMEN.iter()
        .map(|(n, p, a)| format!("{}\n    wok({}({}))", p, n, a.join(", "))).collect();
    let (pi, pv) = (jalankan_uji_batch(&pos, false), jalankan_uji_batch(&pos, true));
    for (i, (n, _, a)) in CONTOH_ARGUMEN.iter().enumerate() {
        if pi[i].is_some() || pv[i].is_some() {
            salah.push(format!("  {}({}) — pemanggilan BENAR ditolak: i={:?} v={:?}", n, a.join(", "), pi[i], pv[i]));
        }
    }
    // Negatif: satu argumen kurang ditolak dua mesin, baris identik, menyebut nama YANG
    // DITULIS — dan ditolak di pintu, jadi badan builtinnya tidak pernah jalan.
    let neg: Vec<(&str, String)> = CONTOH_ARGUMEN.iter()
        .map(|(n, p, a)| (*n, format!("{}\n    wok({}({}))", p, n, a[..a.len() - 1].join(", "))))
        .chain(TANPA_CONTOH_ARGUMEN.iter().map(|(n, _)| {
            let k = minimal(n).unwrap_or(1).saturating_sub(1);
            (*n, format!("wok({}({}))", n, vec!["0"; k].join(", ")))
        })).collect();
    let badan: Vec<String> = neg.iter().map(|(_, b)| b.clone()).collect();
    let (ni, nv) = (jalankan_uji_batch(&badan, false), jalankan_uji_batch(&badan, true));
    for (i, (n, b)) in neg.iter().enumerate() {
        let ok = matches!((&ni[i], &nv[i]), (Some(x), Some(y))
            if x == y && x.contains(&format!("`{}` butuh", n)));
        if !ok { salah.push(format!("  {} — i={:?} v={:?}", b.replace('\n', " "), ni[i], nv[i])); }
    }
    // Lantai: sapuan yang menyusut juga hijau.
    assert!(CONTOH_ARGUMEN.len() > 500 && neg.len() > 550,
        "cuma {} contoh / {} kasus negatif — sapuannya menyusut", CONTOH_ARGUMEN.len(), neg.len());
    assert!(salah.is_empty(), "{} masalah di pintu argumen kurang:\n{}", salah.len(), salah.join("\n"));
}

/// **Kalimat "kurang argumen" sama lewat pintu mana pun**: langsung (dengan tanda tangan
/// `--doc` di baris bantuan), lewat `terap`, lewat variabel — termasuk builtin stdlib VM
/// yang interpreter capai lewat jembatan, yang dulu lolos dari pintu Fase TELANJANG.
#[test]
fn argumen_kurang_dijelaskan_di_tiap_pintu() {
    let kasus: &[(&str, &str, &str)] = &[
        ("wok(arr::chunks([1, 2]))", "`arr::chunks` butuh 2 argumen, dapat 1", "bentuknya `arr::chunks(a, ukuran) -> array`"),
        ("wok(range())", "`range` butuh 1 sampai 3 argumen, dapat 0", "range(awal, akhir, langkah?)"),
        ("wok(str::format())", "`str::format` butuh paling sedikit 1 argumen, dapat 0", ""),
        ("wok(terap(arr::chunks, [[1, 2]]))", "`arr::chunks` butuh 2 argumen, dapat 1", ""),
        ("let f = json::keys\n    wok(f())", "`json::keys` butuh 1 argumen, dapat 0", ""),
        ("let f = json::keys\n    wok(f({\"a\": 1}, 2))", "`json::keys` kebanyakan argumen", ""),
        ("wok(arr::map([1]))", "`arr::map` butuh 2 argumen, dapat 1", ""),
        ("let len = || 5\n    wok(len())", "", ""),
        ("wok(task::cancel())", "tidak pernah benar-benar membatalkan", ""),
        ("let f = arr::map\n    wok(f([1]))", "`arr::map` butuh 2 argumen, dapat 1", ""),
        ("let f = arr::count\n    wok(f([1], |x| x, 3))", "`arr::count` kebanyakan argumen", ""),
        ("wok(arr::map(a: [1], f: |x| x))", "argumen bernama cuma untuk fungsi yang ditulis di JRG", ""),
    ];
    for (badan, harus, bantuan) in kasus {
        let src = format!("package main\npub fn main() {{\n    {}\n}}\n", badan);
        let (i, v) = (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true));
        assert_eq!(jawaban_ringkas(&i), jawaban_ringkas(&v), "{}: dua mesin berbeda", badan);
        if harus.is_empty() {
            assert_eq!(i.trim(), "5", "`len` yang dibayangi closure pemakai harus tetap jalan:\n{}", i);
            continue;
        }
        assert!(jawaban_ringkas(&i).contains(harus), "{}: harus `{}`:\n{}", badan, harus, i);
        assert!(i.contains(bantuan) && v.contains(bantuan), "{}: bantuan `{}` hilang:\n{}", badan, bantuan, i);
    }
}

/// **Tiap builtin menjawab sama dipanggil langsung dan lewat NILAINYA** (`let f = nama; f(…)`),
/// di dua mesin — dulu HOF di `--vm` cuma ada di posisi panggilan, dan `let f = wokwok::var`
/// di akhir baris menelan baris berikutnya; tidak satu sapuan pun mengambil builtin sebagai nilai.
#[test]
fn tiap_builtin_dipanggil_lewat_nilainya_menjawab_sama() {
    // Menulis ke terminal tanpa baris baru, atau menjalankan program lain; acak dua elemen bisa
    // kebetulan sama di dua jalan; dan tiga `future::` menanyakan PENJADWAL (sudah selesaikah
    // thread-nya?), yang dua jawabannya sama-sama benar — terukur beda antar-mesin di profil dev.
    const LEWATI: &[&str] = &["term::", "proses::", "arr::sample", "arr::shuffle", "rand::",
                              "future::is_done", "future::timeout", "future::try_get"];
    let kasus: Vec<_> = CONTOH_ARGUMEN.iter()
        .filter(|(n, _, _)| !LEWATI.iter().any(|l| n.starts_with(l))).collect();
    let langsung: Vec<String> = kasus.iter()
        .map(|(n, p, a)| format!("{}\n    wok({}({}))", p, n, a.join(", "))).collect();
    let nilai: Vec<String> = kasus.iter()
        .map(|(n, p, a)| format!("{}\n    let f_nilai = {}\n    wok(f_nilai({}))", p, n, a.join(", ")))
        .collect();
    let (li, lv) = (jalankan_uji_keluaran("", &langsung, false), jalankan_uji_keluaran("", &langsung, true));
    let (ni, nv) = (jalankan_uji_keluaran("", &nilai, false), jalankan_uji_keluaran("", &nilai, true));
    // Jawaban yang berubah antar-jalan (waktu, direktori sementara) DITURUNKAN, bukan ditebak.
    let li2 = jalankan_uji_keluaran("", &langsung, false);
    let mut salah = Vec::new();
    for (k, (n, _, a)) in kasus.iter().enumerate() {
        if li[k] != li2[k] { continue; }
        if !(li[k] == ni[k] && ni[k] == lv[k] && lv[k] == nv[k]) {
            salah.push(format!("  {}({}) — langsung i={:?} v={:?}; lewat nilai i={:?} v={:?}",
                n, a.join(", "), li[k], lv[k], ni[k], nv[k]));
        }
    }
    assert!(kasus.len() > 500, "cuma {} contoh — sapuannya menyusut", kasus.len());
    assert!(salah.is_empty(), "{} builtin menjawab beda lewat nilainya:\n{}", salah.len(), salah.join("\n"));
}

/// **Kata kunci yang dipakai sebagai NAMA di akhir baris mengakhiri pernyataan** — dulu
/// `let t = d.type` lalu `wok(t)` diurai jadi `let t = d.type(wok(t))` lewat panggilan tanpa
/// kurung, dan dijawab "variabel `t` belum dikenal — maksudnya `d`?" di dua mesin.
#[test]
fn kata_kunci_sebagai_nama_di_akhir_baris_mengakhiri_pernyataan() {
    let src = "let d = {\"type\": 5, \"match\": 7}\nlet t = d.type\nwok(t)\nlet m = d.match\nwok(m)\n\
               let f = wokwok::var\nwok(f(wokwok::array([1.0, 3.0])))\nlet g = future::await\n\
               wok(g(Future::new(|| 9)))\n";
    for vm in [false, true] {
        let k = jalankan_keluaran_penuh(src, vm);
        assert_eq!(k.trim(), "5\n7\n1.0\n9", "vm={}:\n{}", vm, k);
    }
}

/// **Keluaran ke pipa yang ditutup pembacanya mengakhiri program diam-diam** (`jrgc p.jrg |
/// head -2`), seperti `cat` dan Go: dihentikan SIGPIPE, tanpa panik Rust berjalur `/rustc/…`
/// dan tanpa "ini bug di jrgc" — dulu keduanya, keluar 1 di interpreter dan 101 di `--vm`.
#[cfg(unix)]
#[test]
fn pipa_tertutup_mengakhiri_program_diam_diam() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    let dir = crate::bersama::dir_unik("jrg-pipa");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let p = dir.join("p.jrg");
    std::fs::write(&p, "for _, i in range(0, 1000000) { wok(i) }\n").expect("tulis");
    // Jauh lebih besar dari penyangga pipa (64 KB), jadi tulisannya PASTI menemui pipa tertutup;
    // `--emit-tokens` mencetak lewat `println!` milik `jrgc` sendiri, jalur pengait panik.
    let besar = dir.join("besar.jrg");
    std::fs::write(&besar, "wok(1)\n".repeat(40000)).expect("tulis");
    let s = |x: &std::path::Path| x.display().to_string();
    for args in [vec![s(&p)], vec![s(&p), "--vm".into()], vec![s(&besar), "--emit-tokens".into()]] {
        let mut anak = Command::new(jrgc()).args(&args).env("JRG_SOPAN", "1")
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().expect("jalankan jrgc");
        let mut baris = String::new();
        // Pembacanya pergi di akhir pernyataan ini: `BufReader` sementara di-drop.
        BufReader::new(anak.stdout.take().expect("stdout")).read_line(&mut baris).expect("baca");
        let keluar = anak.wait_with_output().expect("tunggu");
        let galat = String::from_utf8_lossy(&keluar.stderr);
        assert!(!baris.is_empty(), "{:?}: baris pertama kosong", args);
        assert!(!galat.contains("panicked") && !galat.contains("bug di jrgc") && !galat.contains("Broken pipe"),
            "{:?}: pipa tertutup dijawab sebagai galat:\n{}", args, galat);
        assert_eq!(keluar.status.signal(), Some(13), "{:?}: harus berakhir oleh SIGPIPE, dapat {:?}\n{}",
            args, keluar.status, galat);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Sisi kiri `=` yang bukan tempat, dan `_` yang dibaca, ditolak SEBELUM jalan** — satu
/// kalimat untuk dua mesin, jadi `wok("mulai")` di depannya tidak pernah tercetak. Dulu acuan
/// mencetaknya lalu gagal, sementara `--vm` gagal saat kompilasi dengan kalimat Inggris.
#[test]
fn sasaran_tugas_dan_pembuang_ditolak_sebelum_jalan() {
    for badan in ["5 = 3", "f() = 2", "a + 1 = 3", "a[0:1] = [9]", "math::PI = 3",
                  "wok(_)", "_ += 1", "_[0] = 1", "x := _"] {
        let src = format!("package main\nfn f() {{ return [1] }}\npub fn main() {{\n    \
                           var a = [1]\n    wok(\"mulai\")\n    {}\n}}\n", badan);
        let (i, v) = (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true));
        for (mesin, k) in [("interp", &i), ("--vm", &v)] {
            assert!(!k.contains("mulai"), "{}: `{}` jalan dulu sebelum ditolak:\n{}", mesin, badan, k);
            assert!(k.contains("bukan sesuatu yang bisa di-assign") || k.contains("`_` cuma untuk membuang"),
                "{}: `{}` dijawab:\n{}", mesin, badan, k);
        }
        assert_eq!(jawaban_ringkas(&i), jawaban_ringkas(&v), "`{}` dijawab beda", badan);
    }
}

/// Baris galat pertama sebuah keluaran penuh, atau seluruh keluarannya kalau
/// programnya diterima.
fn jawaban_ringkas(keluar: &str) -> String {
    match keluar.lines().find(|l| l.starts_with("error")) {
        Some(b) => b.trim().to_string(),
        None => keluar.trim().to_string(),
    }
}

/// `jalankan_uji_batch` yang membawa KELUARAN tiap fungsi: `Ok(keluaran)` kalau lulus,
/// `Err(baris galat)` kalau gagal. `kepala` mendahului semua fungsi test.
fn jalankan_uji_keluaran(kepala: &str, badan: &[String], pakai_vm: bool) -> Vec<Result<String, String>> {
    let dir = crate::bersama::dir_unik("jrg-uji-keluaran");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let mut src = String::from(kepala);
    for (i, b) in badan.iter().enumerate() {
        src.push_str(&format!("fn test_{}() {{\n    {}\n}}\n", i, b));
    }
    std::fs::write(dir.join("uji.jrg"), src).expect("tulis sumber");
    let mut cmd = Command::new(jrgc());
    // Direktori kerjanya sendiri: contoh `fs::`/`file::` menulis berkas di sini.
    cmd.current_dir(&dir).arg("uji.jrg").arg("--uji");
    if pakai_vm { cmd.arg("--vm"); }
    let out = jalankan_berbatas(cmd);
    let _ = std::fs::remove_dir_all(&dir);
    let teks = String::from_utf8_lossy(&out.stdout);
    let mut hasil: Vec<Option<Result<String, String>>> = vec![None; badan.len()];
    let mut tampung: Vec<&str> = Vec::new();
    for l in teks.lines() {
        let t = l.trim_start();
        if let Some(sisa) = t.strip_prefix("ok    test_") {
            if let Ok(i) = sisa.trim().parse::<usize>() { hasil[i] = Some(Ok(tampung.join("\n"))); }
            tampung.clear();
        } else if let Some(sisa) = t.strip_prefix("GAGAL test_") {
            let (no, pesan) = sisa.split_once(" — ").unwrap_or((sisa, ""));
            if let Ok(i) = no.split_whitespace().next().unwrap_or("").parse::<usize>() {
                hasil[i] = Some(Err(pesan.to_string()));
            }
            tampung.clear();
        } else {
            tampung.push(l);
        }
    }
    let hilang: Vec<usize> = (0..badan.len()).filter(|i| hasil[*i].is_none()).collect();
    assert!(hilang.is_empty(), "vm={}: {} dari {} fungsi tidak melapor — prosesnya mati atau \
        menggantung (mis. `{}`)", pakai_vm, hilang.len(), badan.len(), badan[hilang[0]]);
    hasil.into_iter().map(|h| h.expect("sudah diperiksa")).collect()
}

/// **Data bertipe salah — termasuk FUNGSI — di pintu builtin mana pun: satu jawaban di
/// dua mesin, dan fungsi tidak pernah dibaca sebagai data**.
#[test]
fn data_salah_satu_jawaban_di_dua_mesin() {
    use jrgc::interpreter::arity::maks;
    const BENTUK: &[&str] = &["|x| x", "abs", "fdat", "nil", "[1, 2]", r#"{"a": 1}"#, r#""teks""#,
                              "1.5", "true", "Option::Some(1)", "P { a: 1 }", "Some"];
    // Teks di posisi alamat berarti pencarian DNS sungguhan; sapuan ini tidak ke jaringan.
    const JARINGAN: &[&str] = &["dok::delete", "dok::get", "dok::http_delete", "dok::http_get",
        "dok::http_post", "dok::http_put", "dok::http_request", "dok::post", "dok::put",
        "dok::resolve", "dok::tcp_connect", "dok::tcp_listen", "dok::udp_bind", "dok::udp_send_to"];
    // Nama internal jembatan nilai yang dulu bocor ke keluaran program.
    const PENANDA: &[&str] = &["__jrg_fungsi_interp", "titipan", "\u{0}"];
    // Acak berentropi rendah (dua elemen): dua jalan bisa kebetulan SAMA, jadi turunan
    // di bawah tidak bisa menandainya dengan andal. Sisanya diturunkan dengan menjalankan.
    const ACAK: &[&str] = &["arr::sample", "arr::shuffle", "rand::"];
    let kepala = "struct P { pub a: int }\nfn fdat(x) { return x }\n";
    let mut kasus: Vec<(&str, String)> = Vec::new();
    for (nama, pembuka, args) in CONTOH_ARGUMEN {
        let tambah = maks(nama).map_or(false, |m| m > args.len());
        for b in BENTUK {
            if *b == r#""teks""# && JARINGAN.contains(nama) { continue; }
            for k in 0..args.len() + usize::from(tambah) {
                let mut a: Vec<&str> = args.to_vec();
                if k < args.len() { a[k] = b } else { a.push(b) }
                kasus.push((nama, format!("{}\n    wok({}({}))", pembuka, nama, a.join(", "))));
            }
        }
    }
    let badan: Vec<String> = kasus.iter().map(|(_, b)| b.clone()).collect();
    let jalan = |vm: bool| -> Vec<Result<String, String>> {
        badan.chunks(1200).flat_map(|p| jalankan_uji_keluaran(kepala, p, vm)).collect()
    };
    let (i1, i2, v) = (jalan(false), jalan(false), jalan(true));
    let mut salah = Vec::new();
    for (n, (nama, b)) in kasus.iter().enumerate() {
        let b1 = b.trim().replace("\n    ", " ; ");
        let acak = ACAK.iter().any(|a| nama.starts_with(a));
        for (sisi, r) in [("interpreter", &i1[n]), ("--vm", &v[n])] {
            let isi = match r { Ok(x) | Err(x) => x };
            if let Some(p) = PENANDA.iter().find(|p| isi.contains(**p)) {
                salah.push(format!("  {} — {} membocorkan `{}`: {}", b1, sisi, p.escape_default(), isi));
            }
        }
        match (&i1[n], &v[n]) {
            (Ok(_), Err(e)) => salah.push(format!("  {} — cuma --vm yang menolak: {}", b1, e)),
            (Err(e), Ok(_)) => salah.push(format!("  {} — cuma interpreter yang menolak: {}", b1, e)),
            (Err(x), Err(y)) if x != y => salah.push(format!("  {} — kalimat beda:\n      i: {}\n      v: {}", b1, x, y)),
            // Jawaban yang berubah antar-jalan di mesin yang SAMA tidak bisa dibandingkan.
            (Ok(x), Ok(y)) if x != y && i1[n] == i2[n] && !acak =>
                salah.push(format!("  {} — jawaban beda:\n      i: {}\n      v: {}", b1, x, y)),
            _ => {}
        }
    }
    assert!(kasus.len() > 9000, "cuma {} kasus — sapuannya menyusut", kasus.len());
    let n = salah.len();
    salah.truncate(60);
    assert!(n == 0, "{} masalah dari {} kasus (60 pertama):\n{}", n, kasus.len(), salah.join("\n"));
}

/// **Tiap temuan Fase FUNGSI-DATA dikunci dengan jawabannya, di dua mesin.** `=teks`
/// berarti keluaran persisnya; selain itu potongan baris galat pertamanya.
#[test]
fn pintu_data_menjawab_seperti_seharusnya() {
    let kasus: &[(&str, &str)] = &[
        // Fungsi di pintu ISI ditolak; pintu yang MERENDER menampilkannya.
        ("wok(json::keys(|x| x))", "dapat (closure)"),
        ("wok(dok::password_hash(|x| x))", "kata sandinya harus string, dapat `closure`"),
        ("wok(str::to_string(|x| x))", "=<closure>"),
        // Fungsi yang menyeberang jembatan kembali UTUH, di kedalaman mana pun.
        ("let f = Option::unwrap_or(Option::None, |x| x + 1)\n    wok(f(1))", "=2"),
        ("wok(Option::unwrap_or(arr::first([5]), |x| x))", "=5"),
        ("let r = json::set({\"a\": 1}, \"f\", |x| x * 2)\n    let g = json::get(r, \"f\").unwrap()\n    wok(g(4))", "=8"),
        // Handle `dok::` dikenali dari JENISNYA, bukan dari field `id`.
        ("let r = dok::router_new()\n    wok(dok::router_get(U { id: 1 }, \"/\", |q| q))",
         "butuh `DokRouter` (dari `dok::router_new()`), dapat `struct:U`"),
        ("wok(dok::router_get({\"id\": 1}, \"/\", |q| q))", "dapat `dict`"),
        ("wok(dok::tcp_close(U { id: 1 }))", "butuh `TcpConn`"),
        ("wok(dok::router_post(dok::router_new(), 5, |q| q))", "dok::router_post: argumen ke-2 (path) harus string"),
        ("wok(dok::cookie_get(dok::router_new(), \"a\"))", "harus request"),
        ("let l = dok::tcp_listen(\"127.0.0.1\", 0).unwrap()\n    wok(str::starts_with(dok::tcp_local_addr(l), \"127.0.0.1:\"))", "=true"),
        // Data template, lebar indentasi, dan daftar `terap_bernama` tidak diabaikan diam-diam.
        ("wok(dok::render(\"halo {{ x }}\", 5))", "(datanya) harus dict atau struct"),
        ("wok(json::pretty({\"a\": 1}, 4))", "={\n    \"a\": 1\n}"),
        ("wok(json::pretty({\"a\": 1}, \"x\"))", "(lebar indentasi) harus bilangan bulat"),
        ("wok(terap_bernama(|a, b| a, [1], 5))", "argumen ke-3 harus array"),
        ("wok(terap_bernama(|a, b| b, [1], [\"b\"]))", "1 nama tapi 0 nilai"),
        // Kematian proses dan penggantungan yang ditutup.
        ("wok(env::set(\"A=B\", \"x\"))", "nama variabel \"A=B\" tidak sah"),
        ("wok(Canvas::new(1099511627776, 2))", "kanvas 1099511627776x2"),
        ("wok(jawa::buffer(9223372036854775807))", "kelewat besar"),
        ("wok(jawa::buffer_from_hex(\"aéb\"))", "karakter `é` di posisi 1"),
        ("wok(time::sleep_ms(-5))", "=()"),
        ("wok(sleep(1e300))", "time::sleep: durasi"),
        // `jawa::` satu aturan: ukuran, urutan pemeriksaan, dan kalimatnya.
        ("wok(jawa::sizeof(jawa::alloc([1, 2, 3])), jawa::sizeof(jawa::buffer(3)))", "=72 3"),
        ("let b = jawa::buffer(4)\n    jawir::free(b)\n    wok(jawa::sizeof(b))", "use-after-free — buffer ini"),
        ("wok(jawa::array_pop(jawa::array(0)))", "jawa::array_pop: array kosong"),
        ("wok(jawa::get({\"id\": 1}))", "butuh Ptr (dari jawa::alloc/array/buffer), dapat `dict`"),
        ("wok(ptr::get({\"id\": 1}))", "ptr::get: butuh Ptr, dapat `dict`"),
        // Satu kalimat untuk satu kesalahan.
        ("wok(arr::sum(5))", "arr::sum: argumen ke-1 harus array, dapat `int`"),
        ("wok(arr::sum([{\"a\": 1}]))", "bertipe `dict`"),
        ("wok(arr::join([1, 2], 5))", "arr::join: pemisahnya harus string"),
        ("wok(task::join(Option::Some(1)))", "dapat `enum:Option`"),
        ("wok(Option::unwrap(5))", "Option::unwrap: argumen ke-1 harus `Option`, dapat `int`"),
        ("wok(Future::new(5))", "Future::new: argumen ke-1 harus fungsi atau closure"),
        ("var a = 5\n    a = a.push(1)\n    wok(a)", "`int` tidak punya method atau field bernama `push`"),
        ("let f = |a| a\n    wok(f(1, 2))", "fungsi `<closure>` kebanyakan argumen — dia cuma menerima 1, tapi dikasih 2."),
        ("wok(terap(|a, b| a, [1]))", "fungsi `<closure>`: parameter `b` (posisi 2) wajib diisi"),
        // Default sebelum parameter pengumpul: dua mesin dulu salah ke dua arah.
        ("fn g(a, b = 2, sisa...) { return [a, b, len(sisa)] }\n    wok(g(1), g(1, 5, 6))", "=[1, 2, 0] [1, 5, 1]"),
        ("fn g(a = 1, sisa...) { return [a, len(sisa)] }\n    wok(g())", "=[1, 0]"),
    ];
    let mut salah = Vec::new();
    for (badan, harus) in kasus {
        let src = format!("package main\nstruct U {{ pub id: int }}\npub fn main() {{\n    {}\n}}\n", badan);
        let (i, v) = (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true));
        let (ri, rv) = (jawaban_ringkas(&i), jawaban_ringkas(&v));
        if ri != rv {
            salah.push(format!("  {} — dua mesin berbeda:\n      i: {}\n      v: {}", badan, ri, rv));
        } else if let Some(persis) = harus.strip_prefix('=') {
            if ri != *persis { salah.push(format!("  {} — harus `{}`, dapat `{}`", badan, persis, ri)); }
        } else if !ri.starts_with("error") || !ri.contains(harus) {
            salah.push(format!("  {} — harus ditolak dengan `{}`, dapat `{}`", badan, harus, ri));
        }
    }
    assert!(salah.is_empty(), "{} masalah:\n{}", salah.len(), salah.join("\n"));
}

/// Parameter wajib SESUDAH parameter ber-default ditolak parser — default-nya tidak akan
/// pernah terpakai, dan `--vm` dulu menjawab `nil` untuk parameter wajib yang tak terisi.
#[test]
fn parameter_wajib_sesudah_default_ditolak_parser() {
    let src = "package main\nfn f(a, b = 1, c) { return c }\npub fn main() {\n    wok(\"mulai\")\n    wok(f(1, 2))\n}\n";
    for vm in [false, true] {
        let teks = jalankan_keluaran_penuh(src, vm);
        assert!(!teks.contains("mulai") && teks.contains("parameter `c` wajib diisi tapi ditulis sesudah `b`"),
            "vm={}: harus ditolak SEBELUM jalan, dengan sebabnya:\n{}", vm, teks);
    }
}

/// **Konstruktor sebagai nilai dan nama yang bukan nilai: satu jawaban, dan yang benar.**
///
#[test]
fn konstruktor_dan_nama_bukan_nilai_di_dua_mesin() {
    let kasus: &[(&str, &str)] = &[
        // Konstruktor sebagai NILAI, di tiap pintu yang menerima fungsi.
        ("let f = E::A\n    wok(f(3), typeof(f), f)", "=E::A(3) enum_ctor:E::A <ctor E::A>"),
        ("wok(arr::map([1, 2], Some), [1].map(Option::Some), arr::map([1], A))",
         "=[Option::Some(1), Option::Some(2)] [Option::Some(1)] [E::A(1)]"),
        ("let f = E::B\n    wok(f(1, 2), terap(E::A, [5]), 5 |> Some, Ok(...[1]))",
         "=E::B((1, 2)) E::A(5) Option::Some(5) Result::Ok(1)"),
        ("wok(future::await(Future::new(Some, 3)), task::join(Task::spawn(Result::Err, 4)))",
         "=Option::Some(3) Result::Err(4)"),
        ("wok([Some, Ok], typeof(Result::Err), \"x\" + Some)",
         "=[<ctor Option::Some>, <ctor Result::Ok>] enum_ctor:Result::Err x<ctor Option::Some>"),
        ("let Some = 5\n    wok(Some)", "=5"),
        // Menyeberang jembatan UTUH: pintu isi menolak dengan tipenya, pintu render menampilkannya.
        ("wok(str::len(Some))", "dapat `enum_ctor:Option::Some`"),
        ("wok(str::pad_left(Some, 3))", "=<ctor Option::Some>"),
        ("wok(Some.map(|x| x))", "`enum_ctor:Option::Some` tidak punya method"),
        // Aritas: saat JALAN, satu kalimat — langsung, lewat nilai, atau lewat HOF.
        ("wok(\"mulai\")\n    wok(E::A(1, 2))", "konstruktor `E::A` butuh 1 argumen, dapat 2"),
        ("wok(\"mulai\")\n    wok(Some())", "konstruktor `Option::Some` butuh 1 argumen, dapat 0"),
        ("let f = E::B\n    wok(\"mulai\")\n    wok(f(1))", "konstruktor `E::B` butuh 2 argumen, dapat 1"),
        ("wok(\"mulai\")\n    wok(arr::reduce([1, 2], Some, 0))", "konstruktor `Option::Some` butuh 1 argumen, dapat 2"),
        ("wok(\"mulai\")\n    wok(E::C(1))", "nilai bertipe `enum:E` bukan fungsi"),
        ("wok(\"mulai\")\n    wok(Option::None(1))", "nilai bertipe `enum:Option` bukan fungsi"),
        ("let f = E::A\n    wok(\"mulai\")\n    wok(f(a: 1))", "tidak menyimpan nama parameternya"),
        // Nama yang dideklarasikan tapi bukan nilai: dijelaskan, tidak dituduh salah ketik.
        ("wok(\"mulai\")\n    wok(E::V)", "`E::V` itu varian ber-field, bukan nilai — bangun nilainya dengan literal: `E::V { x: ... }`"),
        ("wok(\"mulai\")\n    wok(V)", "`E::V` itu varian ber-field"),
        ("wok(\"mulai\")\n    wok(P)", "`P` itu nama struct (tipe), bukan variabel"),
        ("wok(\"mulai\")\n    wok(E)", "`E` itu nama enum (tipe), bukan variabel"),
        ("wok(\"mulai\")\n    wok(T)", "`T` itu nama trait, bukan variabel"),
        ("wok(E::V { x: 1 })", "=E::V(V {x: 1})"),
    ];
    let kepala = "enum E { A(int), B(int, int), C, V { x: int } }\nstruct P { pub x: int }\ntrait T { fn m(self) -> int }\n";
    let mut salah = Vec::new();
    for (badan, harus) in kasus {
        let src = format!("package main\n{}pub fn main() {{\n    {}\n}}\n", kepala, badan);
        let (i, v) = (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true));
        let (ri, rv) = (jawaban_ringkas(&i), jawaban_ringkas(&v));
        if ri != rv {
            salah.push(format!("  {} — dua mesin berbeda:\n      i: {}\n      v: {}", badan, ri, rv));
        } else if let Some(persis) = harus.strip_prefix('=') {
            if ri != *persis { salah.push(format!("  {} — harus `{}`, dapat `{}`", badan, persis, ri)); }
        } else if !ri.starts_with("error") || !ri.contains(harus) {
            salah.push(format!("  {} — harus ditolak dengan `{}`, dapat `{}`", badan, harus, ri));
        } else if badan.contains("\"mulai\"") && !(i.contains("mulai") && v.contains("mulai")) {
            // Gagal saat JALAN, seperti acuan: keluaran sebelum barisnya tetap tercetak.
            salah.push(format!("  {} — gagal sebelum jalan:\n      i: {}\n      v: {}", badan, i, v));
        }
    }
    assert!(salah.is_empty(), "{} masalah:\n{}", salah.len(), salah.join("\n"));
}

/// **Enum dan struct milik MODUL terjangkau sama di dua mesin — termasuk impor berlian.**
///
#[test]
fn enum_dan_struct_modul_terjangkau_di_dua_mesin() {
    let dir = crate::bersama::dir_unik("jrg-tipe-modul");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let berkas: &[(&str, &str)] = &[
        ("a.jrg", "package a\npub struct T0 { pub x: int = 7 }\npub enum W { P(int), Q, K { n: int } }\n"),
        ("b.jrg", "package b\nimport \"a\"\npub fn buat_b() { return T0 {} }\n"),
        // `c` mengimpor `a` lewat CACHE (sudah dimuat `b`): jalur yang dulu tidak membawa varian.
        ("c.jrg", "package c\nimport \"a\"\npub fn w_c() { return W::P(3) }\npub fn f_c() { return W::P }\n"),
        ("main.jrg", concat!(
            "import \"b\"\nimport \"c\"\nimport \"a\"\n",
            "wok(b::buat_b(), T0 {}, a::T0 {})\n",
            "wok(c::w_c(), c::f_c()(4), W::P(1), a::W::P(2), a::P(5), W::Q, a::Q)\n",
            "let f = a::W::P\nwok(arr::map([1], f), typeof(W::P), W::K { n: 1 })\n",
            "wok(\"mulai\")\nwok(P(1))\n")),
    ];
    for (nama, isi) in berkas { std::fs::write(dir.join(nama), isi).expect("tulis modul"); }
    let jalan = |vm: bool| {
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg("main.jrg");
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };
    let (i, v) = (jalan(false), jalan(true));
    let _ = std::fs::remove_dir_all(&dir);
    let harus = [
        "T0 {x: 7} T0 {x: 7} T0 {x: 7}",
        "W::P(3) W::P(4) W::P(1) W::P(2) W::P(5) W::Q W::Q",
        "[W::P(1)] enum_ctor:W::P W::K(K {n: 1})",
        "mulai",
    ];
    for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
        let baris: Vec<&str> = teks.lines().collect();
        for (n, h) in harus.iter().enumerate() {
            assert_eq!(baris.get(n).copied(), Some(*h), "{}: baris {} salah:\n{}", sisi, n + 1, teks);
        }
        assert!(teks.contains("`P` itu varian `W::P` milik modul `a` — dari luar modulnya tulis `W::P` atau `a::P`"),
                "{}: varian telanjang milik modul harus dijelaskan:\n{}", sisi, teks);
    }
}

/// Galat I/O dilaporkan dengan kalimat JRG (`host::galat_io`), sama di dua mesin — dulu kalimat
/// OS (`No such file or directory (os error 2)`) bocor di tiap pintu.
#[test]
fn galat_io_berkalimat_jrg_di_dua_mesin() {
    if !cfg!(target_os = "linux") { return; }
    let tidak_ada = "berkas atau direktorinya tidak ada";
    let kasus: &[(&str, &str)] = &[
        ("fs::read(\"/tidak/ada\")", tidak_ada),
        ("fs::write(\"/tidak/ada/x\", \"a\")", tidak_ada),
        ("fs::append(\"/tidak/ada/x\", \"a\")", tidak_ada),
        ("fs::size(\"/tidak/ada\")", tidak_ada),
        ("fs::ls(\"/tidak/ada\")", tidak_ada),
        ("fs::read_lines(\"/tidak/ada\")", tidak_ada),
        ("fs::write(\"/proc\", \"a\")", "itu direktori, bukan berkas"),
        ("fs::read(\"/proc/self/exe\")", "isinya bukan teks UTF-8 yang sah"),
        ("fs::read_base64(\"/tidak/ada\")", tidak_ada),
        ("file::read(\"/tidak/ada\")", tidak_ada),
        ("file::write(\"/tidak/ada/x\", \"a\")", tidak_ada),
        ("file::delete(\"/tidak/ada\")", tidak_ada),
        ("file::list_dir(\"/tidak/ada\")", tidak_ada),
        ("dok::tcp_connect(\"127.0.0.1\", 1)", "sambungan ditolak"),
        ("dok::http_get(\"http://127.0.0.1:1/\")", "sambungan ditolak"),
        ("http::get(\"http://127.0.0.1:1/\")", "sambungan ditolak"),
        ("dok::log_to_file(\"/tidak/ada/x\")", tidak_ada),
        ("Canvas::load_png(\"/tidak/ada.png\")", tidak_ada),
        ("canvas::save_png(Canvas::new(2, 2), \"/tidak/ada/x.png\")", tidak_ada),
        ("suara::muat_wav(\"/tidak/ada\")", tidak_ada),
    ];
    let mut salah = Vec::new();
    for (panggilan, harap) in kasus {
        let program = format!("package main\npub fn main() {{ wok({}) }}\n", panggilan);
        let (i, v) = (jalankan_keluaran_penuh(&program, false), jalankan_keluaran_penuh(&program, true));
        for (sisi, k) in [("interpreter", &i), ("--vm", &v)] {
            if !k.contains(harap) || k.contains("os error") {
                salah.push(format!("  {} di {}: harap memuat {:?}, dapat: {}", panggilan, sisi, harap, k.trim()));
            }
        }
    }
    let out = Command::new(jrgc()).arg("/tidak/ada/program.jrg").output().expect("jalankan jrgc");
    let e = String::from_utf8_lossy(&out.stderr);
    if !e.contains(tidak_ada) || e.contains("os error") { salah.push(format!("  jrgc <berkas tak ada>: {}", e.trim())); }
    assert!(salah.is_empty(), "{} masalah:\n{}", salah.len(), salah.join("\n"));
}

/// Method default trait milik MODUL terjangkau dari pengimpornya, dan badannya berjalan di
/// modul pemilik trait-nya; `impl` atas trait yang tidak terlihat atau yang method
/// abstraknya belum diisi ditolak sebelum `main`, dengan satu kalimat.
#[test]
fn default_trait_modul_terjangkau_di_dua_mesin() {
    let dir = crate::bersama::dir_unik("jrg-trait-modul");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let berkas: &[(&str, &str)] = &[
        ("hewan.jrg", concat!(
            "package hewan\npub enum Nada { Rendah, Tinggi }\nconst TANDA = \"!\"\nfn hias(s) { s + TANDA }\n",
            "pub trait Bersuara {\n fn suara(self) -> string\n",
            " fn sapa(self) -> string { hias(\"halo, \" + self.suara()) }\n",
            " fn dua(self) = self.suara() + self.suara()\n fn nada(self) { Rendah }\n",
            " fn rahasia(self) { self.n }\n fn baru(self) { Self { n: 9 } }\n",
            " fn tutup(self) { let f = |x| hias(x + self.suara())\n f(\"<\") }\n}\n",
            "pub struct Anjing { n: int = 3 }\nimpl Bersuara for Anjing { fn suara(self) -> string { \"guk\" } }\n",
            "pub fn anjing() { Anjing {} }\n")),
        ("tengah.jrg", concat!(
            "package tengah\nimport \"hewan\"\npub struct Bebek { n: int = 2 }\n",
            "impl hewan::Bersuara for Bebek { fn suara(self) -> string { \"kwek\" } }\npub fn bebek() { Bebek {} }\n")),
        ("main.jrg", concat!(
            "package main\nimport \"hewan\"\nimport \"tengah\"\n",
            "struct Kucing { n: int = 1 }\nimpl hewan::Bersuara for Kucing { fn suara(self) -> string { \"meong\" } }\n",
            "struct Sapi { n: int = 7 }\nimpl Bersuara for Sapi {\n fn suara(self) -> string { \"moo\" }\n",
            " fn sapa(self) -> string { \"sapi: \" + self.suara() }\n}\n",
            "struct Anak: Kucing { m: int = 0 }\nfn hias(s) { \"SALAH \" + s }\n",
            "pub fn main() {\n wok(hewan::anjing().sapa(), hewan::anjing().dua())\n let k = Kucing {}\n",
            " wok(k.suara(), k.sapa(), k.dua(), k.nada(), k.rahasia(), k.baru(), k.tutup())\n",
            " let s = Sapi {}\n wok(s.sapa(), s.dua(), s.baru())\n wok(Anak {}.sapa(), Anak {}.rahasia())\n",
            " wok(tengah::bebek().sapa(), tengah::bebek().baru())\n wok(Kucing::sapa(k))\n}\n")),
        ("hantu.jrg", "package main\nimport \"hewan\"\nstruct Sapi { n: int = 1 }\n\
            impl Hantu for Sapi { fn suara(self) -> string { \"moo\" } }\npub fn main() { wok(\"jalan\") }\n"),
        ("kurang.jrg", "package main\nimport \"hewan\"\nstruct Sapi { n: int = 1 }\n\
            impl hewan::Bersuara for Sapi { }\npub fn main() { wok(\"jalan\") }\n"),
    ];
    for (nama, isi) in berkas { std::fs::write(dir.join(nama), isi).expect("tulis modul"); }
    let jalan = |berkas: &str, vm: bool| {
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg(berkas).env("JRG_SOPAN", "1");
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };
    let hasil: Vec<(&str, String, String)> = ["main.jrg", "hantu.jrg", "kurang.jrg"].iter()
        .map(|b| (*b, jalan(b, false), jalan(b, true))).collect();
    let _ = std::fs::remove_dir_all(&dir);
    let harus = [
        ("main.jrg", "halo, guk! gukguk\nmeong halo, meong! meongmeong Nada::Rendah 1 Kucing {n: 9} <meong!\n\
            sapi: moo moomoo Sapi {n: 9}\nhalo, meong! 1\nhalo, kwek! Bebek {n: 9}\nhalo, meong!\n"),
        ("hantu.jrg", "error: `impl Hantu for Sapi`: tidak ada trait bernama `Hantu`"),
        ("kurang.jrg", "error: `impl Bersuara for Sapi` belum menyediakan method yang diwajibkan trait-nya: `suara`"),
    ];
    for ((b, i, v), (_, h)) in hasil.iter().zip(harus) {
        assert_eq!(i, v, "{}: dua mesin harus menjawab sama", b);
        assert!(i.starts_with(h), "{}: harap diawali {:?}, dapat:\n{}", b, h, i);
    }
}

/// **Pola atas varian: satu arti di dua mesin, dan salah ketik DILAPORKAN.**
///
#[test]
fn pola_varian_satu_arti_di_dua_mesin() {
    let dir = crate::bersama::dir_unik("jrg-pola-varian");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    std::fs::write(dir.join("bantu.jrg"),
        "package bantu
pub enum Warna { Merah, Hijau, Rgb(int, int, int) }
")
        .expect("tulis modul");
    let jalan = |nama: &str, isi: &str, vm: bool| {
        std::fs::write(dir.join(nama), isi).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg(nama);
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };

    // ── yang harus DITOLAK, dengan kata yang menyebut hal yang benar ──
    let tolak: &[(&str, &str, &str)] = &[
        ("varian modul telanjang di pola",
         "import \"bantu\"\nlet w = bantu::Warna::Rgb(1, 2, 3)\nmatch w { Merah => wok(\"a\"), _ => wok(\"b\") }\n",
         "menelan semuanya"),
        ("sub-pola kebanyakan",
         "enum E { A(int, int) }\nlet a = E::A(1, 2)\nmatch a { E::A(x, y, z) => wok(x), _ => wok(\"lain\") }\n",
         "butuh 2 sub-pola, dapat 3"),
        ("sub-pola kurang untuk payload tunggal",
         "enum E { B(int) }\nlet b = E::B(7)\nmatch b { E::B() => wok(\"a\"), _ => wok(\"b\") }\n",
         "butuh 1 sub-pola, dapat 0"),
        ("varian modul telanjang BERSARANG di dalam pola lain",
         "import \"bantu\"\nlet t = (bantu::Warna::Merah, 1)\nmatch t { (Merah, n) => wok(n), _ => wok(\"lain\") }\n",
         "menelan semuanya"),
        ("sub-pola salah di lapis DALAM",
         "enum E { A(int, int) }\nmatch Option::Some(E::A(1, 2)) { Option::Some(E::A(x, y, z)) => wok(x), _ => wok(\"lain\") }\n",
         "butuh 2 sub-pola, dapat 3"),
        ("pola field struct menyebut field yang tidak ada",
         "struct P { pub a: int, pub b: int }\nlet p = P { a: 1, b: 2 }\nmatch p { P { a, zz } => wok(a), _ => wok(\"lain\") }\n",
         "menyebut field `zz` yang tidak ada"),
        ("pola field varian-struct menyebut field yang tidak ada",
         "enum F { V { x: int, y: int } }\nlet f = F::V { x: 1, y: 2 }\nmatch f { F::V { x, zz } => wok(x), _ => wok(\"lain\") }\n",
         "menyebut field `zz` yang tidak ada"),
        ("salah ketik satu huruf disarankan",
         "struct P { pub a: int, pub b: int }\nlet p = P { a: 1, b: 2 }\nmatch p { P { ab } => wok(1), _ => wok(\"lain\") }\n",
         "maksudnya `a`?"),
        ("Option::Some dengan dua sub-pola",
         "match Option::Some(5) { Option::Some(x, y) => wok(x), _ => wok(\"lain\") }\n",
         "butuh 1 sub-pola, dapat 2"),
    ];
    for (nama, src, kata) in tolak {
        let (i, v) = (jalan("uji.jrg", src, false), jalan("uji.jrg", src, true));
        for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
            assert!(teks.contains("error"), "{} [{}]: harus DITOLAK, malah:\n{}", nama, sisi, teks);
            assert!(teks.contains(kata), "{} [{}]: pesannya harus menyebut {:?}:\n{}", nama, sisi, kata, teks);
        }
        // Baris galat PERTAMA sama persis di dua mesin: satu kesalahan, satu kalimat.
        let baris = |t: &String| t.lines().find(|l| l.contains("error")).unwrap_or("").to_string();
        assert_eq!(baris(&i), baris(&v), "{}: dua mesin harus berkata sama", nama);
    }

    // ── WAKTU gagalnya juga sama: keluaran sebelum `match` tetap tercetak ──
    // Dulu `--vm` menolaknya saat KOMPILASI, jadi barisnya hilang di satu mesin
    // saja — kelas yang sama dengan aritas konstruktor di Fase KONSTRUKTOR.
    for (nama, src) in [
        ("sub-pola salah: gagal saat jalan",
         "enum E { A(int, int) }\nwok(\"sebelum\")\nmatch E::A(1, 2) { E::A(x, y, z) => wok(x), _ => wok(\"lain\") }\n"),
        ("varian modul telanjang: gagal saat jalan",
         "import \"bantu\"\nwok(\"sebelum\")\nmatch bantu::Warna::Merah { Merah => wok(\"a\"), _ => wok(\"b\") }\n"),
        ("pola field tak ada: gagal saat jalan",
         "struct P { pub a: int, pub b: int }\nlet p = P { a: 1, b: 2 }\nwok(\"sebelum\")\nmatch p { P { a, zz } => wok(a), _ => wok(\"lain\") }\n"),
    ] {
        let (i, v) = (jalan("uji.jrg", src, false), jalan("uji.jrg", src, true));
        for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
            assert!(teks.starts_with("sebelum"),
                    "{} [{}]: keluaran sebelum `match` harus tetap tercetak:\n{}", nama, sisi, teks);
            assert!(teks.contains("error"), "{} [{}]: harus tetap gagal:\n{}", nama, sisi, teks);
        }
    }

    // ── yang harus TETAP JALAN, dan menjawab hal yang benar ──
    let terima: &[(&str, &str, &str)] = &[
        ("varian lokal telanjang tetap MENGUJI",
         "enum E { A(int), C }\nlet a = E::A(1)\nmatch a { C => wok(\"salah\"), _ => wok(\"benar\") }\n",
         "benar"),
        ("dua segmen atas varian modul",
         "import \"bantu\"\nmatch bantu::Warna::Merah { Warna::Merah => wok(\"cocok\"), _ => wok(\"tidak\") }\n",
         "cocok"),
        ("TIGA segmen sah di pola, seperti di ekspresi",
         "import \"bantu\"\nmatch bantu::Warna::Rgb(1, 2, 3) { bantu::Warna::Rgb(r, g, b) => wok(r, g, b), _ => wok(\"tidak\") }\n",
         "1 2 3"),
        ("satu sub-pola mengikat payload majemuk utuh",
         "enum E { A(int, int) }\nmatch E::A(1, 2) { E::A(t) => wok(typeof(t), t), _ => wok(\"lain\") }\n",
         "tuple (1, 2)"),
        ("jumlah sub-pola yang benar",
         "enum E { A(int, int) }\nmatch E::A(4, 5) { E::A(x, y) => wok(x + y), _ => wok(\"lain\") }\n",
         "9"),
        ("sebagian field tetap cocok",
         "struct P { pub a: int, pub b: int }\nlet p = P { a: 7, b: 2 }\nmatch p { P { a } => wok(a), _ => wok(\"lain\") }\n",
         "7"),
        ("`..` tetap cocok",
         "struct P { pub a: int, pub b: int }\nlet p = P { a: 8, b: 2 }\nmatch p { P { a, .. } => wok(a), _ => wok(\"lain\") }\n",
         "8"),
        ("pola struct LAIN yang field-nya salah tetap jatuh, bukan dituduh",
         "struct P { pub a: int }\nstruct Q { pub b: int }\nlet p = P { a: 1 }\nmatch p { Q { zz } => wok(\"q\"), P { a } => wok(a) }\n",
         "1"),
        ("lengan SESUDAH yang cocok tidak diperiksa",
         "struct P { pub a: int }\nlet p = P { a: 1 }\nmatch p { _ => wok(\"dulu\"), P { zz } => wok(\"nanti\") }\n",
         "dulu"),
        ("dict bukan struct: tetap jatuh ke _, bukan dituduh",
         "struct P { pub a: int }\nlet d = dict::from_pairs([[\"a\", 1]])\nmatch d { P { a } => wok(\"salah\"), _ => wok(\"benar\") }\n",
         "benar"),
        ("varian yang tidak ada tetap jatuh ke _",
         "enum E { A(int) }\nmatch E::A(1) { E::Z(x) => wok(\"salah\"), _ => wok(\"benar\") }\n",
         "benar"),
    ];
    for (nama, src, harus) in terima {
        let (i, v) = (jalan("uji.jrg", src, false), jalan("uji.jrg", src, true));
        for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
            assert!(!teks.contains("error"), "{} [{}]: harus JALAN, malah ditolak:\n{}", nama, sisi, teks);
            assert!(teks.contains(harus), "{} [{}]: harus menjawab {:?}:\n{}", nama, sisi, harus, teks);
        }
        assert_eq!(i, v, "{}: dua mesin harus menjawab sama", nama);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Pintu KURUNG: sebuah callable tanpa `()` punya satu arti di dua mesin.**
///
#[test]
fn pintu_kurung_satu_arti_di_dua_mesin() {
    let dir = crate::bersama::dir_unik("jrg-kurung");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let jalan = |isi: &str, vm: bool| -> (String, bool) {
        std::fs::write(dir.join("uji.jrg"), isi).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg("uji.jrg").arg("--sopan");
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)),
         out.status_sukses)
    };

    const DASAR: &str = "struct H { pub n: int }\n\
        impl H { fn suara(self) { return \"umum\" } fn tambah(self, k) { return self.n + k } }\n\
        struct A: H { pub x: int }\n\
        trait T { fn sapa(self) { return \"hai\" } }\n\
        impl T for A { }\n\
        fn nol() { wok(\"EFEK\") }\n\
        let a = A { n: 1, x: 2 }\n";

    // ── yang harus DITOLAK, dengan kata yang menyebut hal yang benar ──
    let tolak: &[(&str, &str, &str)] = &[
        ("fungsi nullary sebagai pernyataan", "nol\nwok(\"akhir\")\n", "berdiri sendiri"),
        ("closure sebagai pernyataan", "let g = || 1\ng\nwok(\"akhir\")\n", "berdiri sendiri"),
        ("fungsi ber-argumen sebagai pernyataan",
         "fn k(a) { return a }\nk\nwok(\"akhir\")\n", "berdiri sendiri"),
        ("native sebagai pernyataan", "len\nwok(\"akhir\")\n", "berdiri sendiri"),
        ("konstruktor sebagai pernyataan", "enum E { A(int) }\nE::A\nwok(\"akhir\")\n",
         "berdiri sendiri"),
        ("titik koma tidak mengubah apa pun", "fn k() { }\nk;\nwok(\"akhir\")\n",
         "berdiri sendiri"),
        ("method pemakai tanpa kurung", "wok(typeof(a.suara))\n", "ditulis tanpa `()`"),
        ("method WARISAN tanpa kurung", "let b = a.suara\n", "ditulis tanpa `()`"),
        ("default trait tanpa kurung", "wok(typeof(a.sapa))\n", "ditulis tanpa `()`"),
        ("method ber-argumen tanpa kurung", "wok(typeof(a.tambah))\n", "ditulis tanpa `()`"),
        ("method bawaan string tanpa kurung", "let s = \"x\"\nwok(typeof(s.upper))\n",
         "ditulis tanpa `()`"),
        ("method bawaan array tanpa kurung", "let v = [1]\nwok(typeof(v.sort))\n",
         "ditulis tanpa `()`"),
        ("jalan keluarnya disebut", "wok(typeof(a.suara))\n", "A::suara"),
        // `--vm` dulu memanggil `f(k, 1)` dan BERHASIL — penerimanya disisipkan
        // sebagai argumen pertama, yang tidak pernah ditulis penulisnya.
        ("field bertipe fungsi dipanggil bentuk method",
         "struct K { pub f: function }\nlet k = K { f: |a, b| wok(b) }\nk.f(1)\n",
         "itu FIELD milik `K`, bukan method"),
        ("method dict tanpa kurung",
         "let d = {\"a\": 1}\nwok(typeof(d.keys))\n", "itu method milik `dict`"),
        ("jalan keluar field-fungsi disebut",
         "struct K { pub f: function }\nlet k = K { f: |x| x }\nk.f(1)\n",
         "(x.f)(...)"),
    ];
    let mut salah: Vec<String> = Vec::new();
    for (nama, potongan, kata) in tolak {
        let src = format!("{}{}", DASAR, potongan);
        let (i, ri) = jalan(&src, false);
        let (v, rv) = jalan(&src, true);
        for (sisi, teks, sukses) in [("interpreter", &i, ri), ("--vm", &v, rv)] {
            if sukses {
                salah.push(format!("{} [{}]: harus DITOLAK, tapi keluar 0:\n{}", nama, sisi, teks));
            }
            if !teks.contains(kata) {
                salah.push(format!("{} [{}]: harus menyebut {:?}:\n{}", nama, sisi, kata, teks));
            }
        }
        if i.lines().next() != v.lines().next() {
            salah.push(format!("{}: baris galat pertama beda:\n  i={:?}\n  v={:?}",
                               nama, i.lines().next(), v.lines().next()));
        }
    }

    // ── yang harus TETAP JALAN: penjaga yang menolak semuanya juga
    //    menghijaukan separuh negatifnya dengan sempurna ──
    let terima: &[(&str, &str, &str)] = &[
        ("pemanggilan method biasa", "wok(a.suara())\n", "umum"),
        ("method ber-argumen dipanggil", "wok(a.tambah(4))\n", "5"),
        ("default trait dipanggil", "wok(a.sapa())\n", "hai"),
        ("method sebagai nilai lewat path", "let g = A::suara\nwok(g(a))\n", "umum"),
        ("pseudo-field len", "let s = \"halo\"\nwok(s.len, [1, 2].len)\n", "4 2"),
        ("pseudo-field is_empty", "wok(\"\".is_empty, [].is_empty)\n", "true true"),
        ("method bawaan dipanggil", "wok(\"ab\".upper(), [2, 1].sort())\n", "AB [1, 2]"),
        ("field bertipe fungsi tetap NILAI",
         "struct K { pub f: function }\nlet k = K { f: |x| x + 1 }\nwok(typeof(k.f), (k.f)(1))\n",
         "closure 2"),
        ("nama BUKAN callable tetap no-op", "let x = 5\nx\nwok(\"akhir\")\n", "akhir"),
        ("ekspresi EKOR tetap nilai kembalian",
         "fn bikin() { nol }\nwok(typeof(bikin()))\n", "function"),
        ("nama di dalam blok bukan pernyataan", "if true { nol }\nwok(\"akhir\")\n", "akhir"),
        ("field struct biasa", "wok(a.n, a.x)\n", "1 2"),
        // Penjaga untuk Map ada DI DALAM pencarian field, sesudah kunci nyata
        // dicoba — tanpa itu sebuah kunci bernama `keys` jadi tidak terjangkau.
        ("kunci dict MENANG atas nama method",
         "let d = {\"keys\": 1, \"len\": 2}\nwok(d.keys, d.len)\n", "1 2"),
        ("method dict dipanggil", "let d = {\"a\": 1}\nwok(d.keys(), d.len())\n", "[a] 1"),
    ];
    for (nama, potongan, harap) in terima {
        let src = format!("{}{}", DASAR, potongan);
        let (i, _) = jalan(&src, false);
        let (v, _) = jalan(&src, true);
        for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
            if !teks.contains(harap) {
                salah.push(format!("{} [{}]: harus menjawab {:?}, dapat:\n{}",
                                   nama, sisi, harap, teks));
            }
        }
        if i != v {
            salah.push(format!("{}: dua mesin beda:\n  i={:?}\n  v={:?}", nama, i, v));
        }
    }

    // ── WAKTU gagalnya sama: keluaran sebelumnya tetap tercetak ──
    for (nama, src) in [
        ("fungsi sebagai pernyataan: gagal saat jalan",
         "fn k() { }\nwok(\"sebelum\")\nk\n"),
        ("method tanpa kurung: gagal saat jalan",
         "struct P { pub a: int }\nimpl P { fn m(self) { return 1 } }\n\
          let p = P { a: 1 }\nwok(\"sebelum\")\nwok(typeof(p.m))\n"),
    ] {
        let (i, _) = jalan(src, false);
        let (v, _) = jalan(src, true);
        for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
            if !teks.starts_with("sebelum") {
                salah.push(format!("{} [{}]: keluaran sebelumnya harus tercetak:\n{}",
                                   nama, sisi, teks));
            }
            if !teks.contains("error") {
                salah.push(format!("{} [{}]: harus tetap gagal:\n{}", nama, sisi, teks));
            }
        }
    }

    assert!(salah.is_empty(), "pintu kurung ({} masalah):\n{}", salah.len(), salah.join("\n"));
}

/// **Satu penyandi JSON: dua pintu menjawab sama, dan yang tak bisa
/// DITOLAK.**
#[test]
fn banding_callable_ditolak_di_dua_mesin() {
    let dir = crate::bersama::dir_unik("jrg-banding-fn");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let jalan = |isi: &str, vm: bool| -> (String, bool) {
        std::fs::write(dir.join("uji.jrg"), isi).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg("uji.jrg").arg("--sopan");
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)), out.status_sukses)
    };
    const DASAR: &str = "enum E { A(int), C }\nstruct P { pub a: int }\n\
                         struct H { pub fungsi: int }\n\
                         fn f(x) { return x }\nfn g(x) { return x + 1 }\n";
    let mut salah: Vec<String> = Vec::new();

    // ── HARUS DITOLAK ────────────────────────────────────────────────────
    // Termasuk posisi BERSARANG: sebuah fungsi di dalam array, dict atau argumen …
    let tolak: &[(&str, &str)] = &[
        ("fungsi dengan dirinya",  "wok(f == f)"),
        ("fungsi != dirinya",      "wok(f != f)"),
        ("dua fungsi berbeda",     "wok(f == g)"),
        ("lewat nama lain",        "let h = f\nwok(h == f)"),
        ("dua closure",            "let c = |x| x\nlet d = |x| x\nwok(c == d)"),
        ("closure dengan dirinya", "let c = |x| x\nwok(c == c)"),
        ("native",                 "wok(len == len)"),
        ("konstruktor bawaan",     "wok(Some == Some)"),
        ("konstruktor pemakai",    "wok(E::A == E::A)"),
        ("di dalam array",         "wok([f] == [f])"),
        ("di dalam dict",          "wok({\"a\": f} == {\"a\": f})"),
        ("di dalam tuple",         "wok((1, f) == (1, f))"),
        ("field struct berisi fungsi",
                                   "wok(H { fungsi: f } == H { fungsi: f })"),
        ("arr::contains",          "wok(arr::contains([f], f))"),
        ("arr::index_of",          "wok(arr::index_of([f], f))"),
        ("arr::unique",            "wok(arr::unique([f, f]))"),
        ("assert_eq",              "assert_eq(f, f)"),
        // Keenam operator, bukan cuma `==`: `f < g` sudah ditolak sejak lama dengan
        // kalimat TERSE, jadi menutup `==` saja akan menukar satu masalah dengan …
        ("urutan <",               "wok(f < g)"),
        ("urutan <=",              "wok(f <= g)"),
        ("urutan >",               "wok(f > g)"),
        ("urutan >=",              "wok(f >= g)"),
    ];
    for (nama, badan) in tolak {
        let src = format!("{}pub fn main() {{\n  wok(\"sebelum\")\n  {}\n}}\n", DASAR, badan);
        let (i, oki) = jalan(&src, false);
        let (v, okv) = jalan(&src, true);
        if oki || okv {
            salah.push(format!("{}: harus DITOLAK, tapi jalan (i={} v={})\n  i={:?}",
                               nama, oki, okv, i));
            continue;
        }
        let bi = i.lines().find(|l| l.starts_with("error:")).unwrap_or("");
        let bv = v.lines().find(|l| l.starts_with("error:")).unwrap_or("");
        if bi != bv {
            salah.push(format!("{}: kalimat beda\n  i={:?}\n  v={:?}", nama, bi, bv));
        }
        if !bi.contains("tidak bisa membandingkan") {
            salah.push(format!("{}: kalimatnya bukan kalimat pembanding: {:?}", nama, bi));
        }
        // Penjelasannya WAJIB ikut, bukan cuma kalimat pendeknya.
        if !bi.contains("tidak punya identitas") {
            salah.push(format!("{}: penjelasannya hilang (daftar nama tipe callable \
                                melenceng dari daftar variannya?): {:?}", nama, bi));
        }
        // Gagal SAAT JALAN, bukan saat kompilasi: keluaran sebelumnya utuh.
        if !i.contains("sebelum") || !v.contains("sebelum") {
            salah.push(format!("{}: keluaran sebelum galat hilang\n  i={:?}\n  v={:?}",
                               nama, i, v));
        }
    }

    // ── HARUS TETAP JALAN ────────────────────────────────────────────────
    // Penjaganya duduk di pintu `==`, jadi separuh ini yang menahannya dari …
    let terima: &[(&str, &str, &str)] = &[
        ("fungsi lawan nil",   "wok(f == nil)",                       "false"),
        ("nil lawan fungsi",   "wok(nil == f)",                       "false"),
        ("fungsi lawan int",   "wok(f == 1)",                         "false"),
        ("fungsi != int",      "wok(f != 1)",                         "true"),
        ("array sama",         "wok([1, 2] == [1, 2])",               "true"),
        ("array beda",         "wok([1, 2] == [1, 3])",               "false"),
        ("struct",             "wok(P { a: 1 } == P { a: 1 })",       "true"),
        ("dict",               "wok({\"a\": 1} == {\"a\": 1})",       "true"),
        ("enum payload",       "wok(E::A(2) == E::A(2))",             "true"),
        ("enum unit",          "wok(E::C == E::C)",                   "true"),
        ("Option",             "wok(Option::Some(1) == Option::Some(1))", "true"),
        ("contains campur",    "wok(arr::contains([1, f], 1))",       "true"),
        ("unique campur",      "wok(len(arr::unique([1, f, 1])))",    "2"),
        ("urut",               "wok(arr::sort([3, 1, 2]))",           "[1, 2, 3]"),
        ("assert_eq biasa",    "assert_eq(1, 1)\nwok(\"lulus\")",     "lulus"),
    ];
    // Tipe yang berbeda tetap dijawab kalimat PENDEK — penjaganya cuma berlaku
    // kalau KEDUA sisi bisa dipanggil, jadi ia tidak boleh menempelkan …
    {
        let src = format!("{}pub fn main() {{ wok([1] < [\"a\"]) }}\n", DASAR);
        let (i, oki) = jalan(&src, false);
        let (v, okv) = jalan(&src, true);
        if oki || okv {
            salah.push(format!("elemen tak sebanding: harus ditolak\n  i={:?}", i));
        } else if i != v {
            salah.push(format!("elemen tak sebanding: dua mesin beda\n  i={:?}\n  v={:?}", i, v));
        } else if i.contains("tidak punya identitas") {
            salah.push(format!("elemen tak sebanding: kebagian penjelasan FUNGSI: {:?}", i));
        }
    }
    for (nama, badan, harus) in terima {
        let src = format!("{}pub fn main() {{\n  {}\n}}\n", DASAR, badan);
        let (i, oki) = jalan(&src, false);
        let (v, okv) = jalan(&src, true);
        if !oki || !okv {
            salah.push(format!("{}: harus JALAN, tapi ditolak\n  i={:?}\n  v={:?}", nama, i, v));
            continue;
        }
        if i != v {
            salah.push(format!("{}: dua mesin beda\n  i={:?}\n  v={:?}", nama, i, v));
        }
        if i.trim() != *harus {
            salah.push(format!("{}: jawabnya {:?}, harusnya {:?}", nama, i.trim(), harus));
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
    assert!(salah.is_empty(), "pintu banding callable:\n{}", salah.join("\n"));
}

#[test]
fn penyandi_json_satu_jawaban_dan_menolak_yang_tak_bisa() {
    let dir = crate::bersama::dir_unik("jrg-json-satu");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let jalan = |isi: &str, vm: bool| -> (String, bool) {
        std::fs::write(dir.join("uji.jrg"), isi).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg("uji.jrg").arg("--sopan");
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)), out.status_sukses)
    };
    const DASAR: &str = "struct P { pub a: int, pub b: string }\nenum E { A(int), C }\n";
    let mut salah: Vec<String> = Vec::new();

    // ── DUA PINTU, SATU JAWABAN ──────────────────────────────────────────
    // `dok::json_encode` dulu punya penyandi sendiri: enum jadi …
    let bentuk: &[(&str, &str)] = &[
        ("int", "42"), ("float", "2.5"), ("bool", "true"),
        ("string", "\"h\u{e9}\\\"x\""), ("nil", "nil"),
        ("array", "[1, \"a\", nil]"), ("dict", "{\"z\": 1, \"a\": [1, 2]}"),
        ("tuple", "(1, \"a\")"), ("struct", "P { a: 1, b: \"x\" }"),
        ("enum payload", "E::A(5)"), ("enum unit", "E::C"),
        ("Option Some", "Option::Some(3)"), ("Option None", "Option::None"),
        ("Result Ok", "Result::Ok(1)"), ("rune", "'z'"),
        ("array kosong", "[]"), ("dict kosong", "dict::new()"),
        ("besar", "9007199254740993"),
    ];
    for (nama, ekspr) in bentuk {
        let src = format!("{}let v = {}\nwok(json::stringify(v))\nwok(dok::json_encode(v))\n",
                          DASAR, ekspr);
        let (i, _) = jalan(&src, false);
        let (v, _) = jalan(&src, true);
        if i != v {
            salah.push(format!("{}: dua mesin beda:\n  i={:?}\n  v={:?}", nama, i, v));
            continue;
        }
        let baris: Vec<&str> = i.lines().collect();
        if baris.len() < 2 || baris[0] != baris[1] {
            salah.push(format!("{}: dua PINTU beda — json::stringify={:?} dok::json_encode={:?}",
                               nama, baris.first(), baris.get(1)));
        }
    }

    // ── yang tidak punya padanan JSON: DITOLAK, bukan dipalsukan ─────────
    // `NaN`/`inf` dulu ditulis apa adanya, dan keluarannya ditolak `json.loads` …
    let tolak: &[(&str, &str)] = &[
        ("NaN", "math::NAN"), ("inf", "math::INF"), ("-inf", "0.0 - math::INF"),
        ("closure", "|x| x"), ("fungsi bernama", "id"), ("native", "len"),
        ("konstruktor", "E::A"), ("canvas", "Canvas::new(2, 2)"),
        // `Chan::new(1)` sengaja TIDAK di sini: sebuah channel tidak bisa
        // menyeberangi jembatan nilai sama sekali, jadi jembatan di sisi …
        ("NaN di dalam dict", "{\"a\": math::NAN}"),
        ("fungsi di dalam array", "[1, |x| x]"),
    ];
    for (nama, ekspr) in tolak {
        for pintu in ["json::stringify", "dok::json_encode", "json::pretty"] {
            if pintu == "json::pretty" && nama.starts_with("fungsi bernama") { continue; }
            let src = format!("{}fn id(x) {{ return x }}\nwok({}({}))\n", DASAR, pintu, ekspr);
            let (i, si) = jalan(&src, false);
            let (v, sv) = jalan(&src, true);
            for (sisi, teks, sukses) in [("interpreter", &i, si), ("--vm", &v, sv)] {
                if sukses {
                    salah.push(format!("{} lewat {} [{}]: harus DITOLAK, tapi keluar 0:\n{}",
                                       nama, pintu, sisi, teks));
                }
                if !teks.contains("tidak punya padanan di JSON") {
                    salah.push(format!("{} lewat {} [{}]: harus menyebut padanan JSON:\n{}",
                                       nama, pintu, sisi, teks));
                }
            }
        }
    }

    // ── sisi POSITIF: yang BISA jadi JSON harus tetap bisa ───────────────
    for (nama, src, harap) in [
        ("float biasa tetap jalan", "wok(json::stringify(2.5), json::stringify(2.0))", "2.5 2.0"),
        ("angka besar utuh", "wok(json::stringify(9007199254740993))", "9007199254740993"),
        ("pretty tetap jalan", "wok(json::pretty({\"a\": 1}))", "\"a\": 1"),
        ("respons JSON tetap jalan", "wok(typeof(dok::json_response({\"a\": 1})))", "struct"),
    ] {
        let s2 = format!("{}{}\n", DASAR, src);
        let (i, _) = jalan(&s2, false);
        let (v, _) = jalan(&s2, true);
        for (sisi, teks) in [("interpreter", &i), ("--vm", &v)] {
            if !teks.contains(harap) {
                salah.push(format!("{} [{}]: harus menjawab {:?}, dapat:\n{}", nama, sisi, harap, teks));
            }
        }
    }

    // ── ORACLE INDEPENDEN: yang keluar harus JSON menurut parser SUNGGUHAN ──
    // Membandingkan dua pintu kita sendiri tidak bisa melihat `NaN`/`inf`:
    // keduanya menulisnya, dan keduanya salah. Yang melihatnya cuma pihak lain.
    let py = Command::new("python3").arg("-c").arg("print(1)").output();
    if py.map(|o| o.status.success()).unwrap_or(false) {
        let src = format!("{}wok(json::stringify({{\"x\": [1, 2.5, \"a\", nil, true],              \"e\": E::A(3), \"s\": P {{ a: 1, b: \"q\" }}, \"o\": Option::Some(1),              \"n\": Option::None}}))\n", DASAR);
        for vm in [false, true] {
            let (keluar, _) = jalan(&src, vm);
            let teks = keluar.lines().next().unwrap_or("").to_string();
            let cek = Command::new("python3")
                .arg("-c").arg("import json,sys; json.loads(sys.argv[1])")
                .arg(&teks).output().expect("jalankan python3");
            assert!(cek.status.success(),
                    "keluaran JSON ditolak parser sungguhan [{}]:\n  {}\n  {}",
                    if vm { "--vm" } else { "interpreter" }, teks,
                    String::from_utf8_lossy(&cek.stderr).lines().last().unwrap_or(""));
        }
    } else {
        eprintln!("dilewati (separuh): `python3` tidak ada, oracle JSON tidak dijalankan");
    }

    assert!(salah.is_empty(), "penyandi JSON ({} masalah):\n{}", salah.len(), salah.join("\n"));
}

/// **Pintu `dict::`: struct dan enum bukan dict, dan tiap kesalahan satu kalimat.**
/// Nama diturunkan dari registry
#[test]
fn pintu_dict_satu_kalimat_di_dua_backend() {
    // `X` diganti tiap nilai salah, `D` sebuah dict yang benar.
    const BENTUK: &[(&str, &[&str])] = &[
        ("dict::from_pairs", &["X", "[X]", ""]),
        ("dict::get", &["X, \"a\"", "D", ""]),
        ("dict::get_or", &["X, \"a\", 0", "D, \"a\"", "D", ""]),
        ("dict::has", &["X, \"a\"", "D", ""]),
        ("dict::is_dict", &["X", ""]),
        ("dict::is_empty", &["X", ""]),
        ("dict::items", &["X", ""]),
        ("dict::keys", &["X", ""]),
        ("dict::len", &["X", ""]),
        ("dict::merge", &["X, D", "D, X", "D", ""]),
        ("dict::new", &["X"]),
        ("dict::remove", &["X, \"a\"", "D", ""]),
        ("dict::set", &["X, \"a\", 1", "D, \"a\"", "D", ""]),
        ("dict::values", &["X", ""]),
    ];
    const SALAH: &[&str] = &["P { a: 1 }", "Option::Some(1)", "E::A(3)", "E::B", "5", "[1, 2]", "(1, 2)"];

    let terdaftar: Vec<String> = jrgc::vm::daftar_builtin().into_iter()
        .filter(|n| n.starts_with("dict::")).collect();
    for n in &terdaftar {
        assert!(BENTUK.iter().any(|(b, _)| b == n),
            "`{}` terdaftar tapi belum punya bentuk di tes ini — pintu baru harus ikut disapu", n);
    }
    for (b, _) in BENTUK {
        assert!(terdaftar.iter().any(|n| n == b), "`{}` tidak terdaftar lagi — hapus dari BENTUK", b);
    }

    let mut salah = Vec::new();
    let mut diperiksa = 0;
    for (nama, bentuk) in BENTUK {
        for b in *bentuk {
            let isian: &[&str] = if b.contains('X') { SALAH } else { &[""] };
            for x in isian {
                let arg = b.replace('X', x).replace('D', "{\"a\": 1}");
                let src = format!(
                    "package main\nstruct P {{ pub a: int }}\nenum E {{ A(int), B }}\n\
                     pub fn main() {{ wok({}({})) }}\n", nama, arg);
                let i = jawaban_ringkas(&jalankan_keluaran_penuh(&src, false));
                let v = jawaban_ringkas(&jalankan_keluaran_penuh(&src, true));
                diperiksa += 1;
                // Yang boleh diterima cuma `is_dict(apa pun)` dan sebuah pasangan yang sah.
                let boleh_terima = (*nama == "dict::is_dict" && *b == "X")
                    || (*nama == "dict::from_pairs" && *b == "[X]" && (*x == "[1, 2]" || *x == "(1, 2)"));
                let ditolak = i.starts_with("error");
                if i != v {
                    salah.push(format!("  {}({}) — beda:\n    interp: {}\n    vm    : {}", nama, arg, i, v));
                } else if ditolak == boleh_terima {
                    salah.push(format!("  {}({}) — {}: {}", nama, arg,
                        if ditolak { "harusnya diterima" } else { "harusnya DITOLAK" }, i));
                } else if i.contains("__type__") || i.contains("__payload__") {
                    salah.push(format!("  {}({}) — membocorkan kunci internal: {}", nama, arg, i));
                }
            }
        }
    }
    assert!(diperiksa > 100, "cuma {} program yang diperiksa — sapuannya tidak berjalan", diperiksa);
    assert!(salah.is_empty(), "{} bentuk `dict::` salah:\n{}", salah.len(), salah.join("\n"));
}

/// **`for … in` atas tiap bentuk nilai: jawaban yang sama di dua backend, dan
/// selalu SELESAI** — tiga bentuk di sini dulu menggantung di `--vm`.
#[test]
fn iterasi_tiap_bentuk_nilai_sama_di_dua_backend() {
    const NILAI: &[&str] = &[
        "P { a: 1 }", "E::A(3)", "E::B", "Option::Some(1)", "Option::None", "Result::Ok(1)",
        "(7, 8)", "{\"a\": 1}", "{}", "[1, 2]", "[]", "\"ab\"", "3", "0", "2.5", "true",
        "nil", "()", "'a'", "wok", "Canvas::new(2, 2)", "range(2)",
    ];
    let mut beda = Vec::new();
    for x in NILAI {
        for kepala in ["for k in X { wok(k) }", "for k, v in X { wok(k, v) }"] {
            let badan = kepala.replace('X', x);
            let src = format!("package main\nstruct P {{ pub a: int }}\nenum E {{ A(int), B }}\n\
                               pub fn main() {{ {}\n wok(\"SELESAI\") }}\n", badan);
            let i = jawaban_ringkas(&jalankan_keluaran_penuh(&src, false));
            let v = jawaban_ringkas(&jalankan_keluaran_penuh(&src, true));
            if i != v {
                beda.push(format!("  {}\n    interp: {}\n    vm    : {}", badan, i, v));
            }
        }
    }
    assert!(beda.is_empty(), "{} bentuk iterasi dijawab BERBEDA:\n{}", beda.len(), beda.join("\n"));
}

/// **Menulis lewat jalur yang salah: satu kalimat di dua backend, tidak pernah
/// diterima, dan selalu SELESAI.**
#[test]
fn jalur_tulis_satu_kalimat_di_dua_backend() {
    const PRE: &str = "package main\nstruct Q { pub x: int }\nstruct P { pub a: int, pub q: Q }\n";
    const KASUS: &[&str] = &[
        "var p = P { a: 1, q: Q { x: 2 } }\n p.zz.x = 5",
        "var p = P { a: 1, q: Q { x: 2 } }\n p[\"zz\"].x = 5",
        "var d = {\"a\": {\"b\": 1}}\n d.zz.b = 5",
        "var d = {\"a\": {\"b\": 1}}\n d[\"zz\"].b = 5",
        "var e = [P { a: 1, q: Q { x: 2 } }]\n e[0].zz.x = 7",
        "var p = P { a: 1, q: Q { x: 2 } }\n p.a.zz = 5",
        "var p = P { a: 1, q: Q { x: 2 } }\n p.a.zz.x = 5",
        "var p = P { a: 1, q: Q { x: 2 } }\n p.a[0] = 5",
        "var p = P { a: 1, q: Q { x: 2 } }\n p.a[0].x = 5",
        "var t = (1, 2)\n t.zz = 9",
        "var t = (1, 2)\n t[\"a\"] = 9",
        "var t = (1, 2)\n t[9] = 9",
        "var t = (1, 2)\n t.7 = 9",
        "var s = \"ab\"\n s[0] = 'x'",
        "var s = \"ab\"\n s[0].x = 1",
        "var a = [1, 2]\n a[\"x\"] = 1",
        "var a = [[1], [2]]\n a[\"x\"][0] = 1",
        "var a = [1, 2]\n a[1.5] = 1",
        "var a = [1, 2]\n a[true] = 1",
        "var a = [1, 2]\n a[1.5] += 1",
        "var o = Option::Some(1)\n o.zz.x = 5",
        "var o = Option::Some(1)\n o[\"zz\"].x = 5",
    ];
    let mut salah = Vec::new();
    for badan in KASUS {
        let src = format!("{}pub fn main() {{\n {}\n wok(\"TERTULIS\")\n}}\n", PRE, badan);
        let i = jawaban_ringkas(&jalankan_keluaran_penuh(&src, false));
        let v = jawaban_ringkas(&jalankan_keluaran_penuh(&src, true));
        let bentuk = badan.replace('\n', ";");
        if !i.starts_with("error") {
            salah.push(format!("  {} — interpreter MENERIMA: {}", bentuk, i));
        } else if i != v {
            salah.push(format!("  {} — beda:\n    interp: {}\n    vm    : {}", bentuk, i, v));
        }
    }
    assert!(salah.is_empty(), "{} jalur tulis salah:\n{}", salah.len(), salah.join("\n"));
}

/// **Data tidak bisa memalsukan jenis sebuah nilai** — label struct/enum VM dulu
/// kunci biasa di peta.
#[test]
fn label_tipe_tidak_bisa_dipalsukan_data() {
    const PRE: &str =
        "package main\nstruct V { pub variant: string, pub __type__: string, pub __payload__: int }\n";
    // (nilai yang MEMUAT nama-nama label lama sebagai data, `typeof` yang benar)
    const DATA: &[(&str, &str)] = &[
        (r#"{"variant": "Some", "__type__": "Option", "__payload__": 5}"#, "dict"),
        (r#"json::parse("{\"variant\":\"Some\",\"__type__\":\"Option\",\"__payload__\":5}")"#, "dict"),
        (r#"json::parse("{\"variant\":\"x\"}")"#, "dict"),
        (r#"dict::from_pairs([["variant", "None"], ["__type__", "Option"]])"#, "dict"),
        (r#"V { variant: "Some", __type__: "Option", __payload__: 5 }"#, "struct:V"),
    ];
    // `X` diganti datanya; `cek` menyebut apa yang dituntut dari jawabannya.
    const AMATI: &[(&str, &str)] = &[
        ("wok(typeof(X))", "tipe"),
        ("wok(X)", "tampil"),
        ("let x = X\n wok(match x { Some(n) => n, None => -1, _ => -2 })", "cocok"),
        ("wok(json::stringify(X))", "json"),
        ("var x = X\n x.variant = \"None\"\n wok(typeof(x))", "tipe"),
        ("var x = X\n x[\"__type__\"] = \"Option\"\n wok(typeof(x))", "tipe"),
    ];
    let mut salah = Vec::new();
    let mut diperiksa = 0;
    for (x, tipe) in DATA {
        for (amati, cek) in AMATI {
            let badan = amati.replace('X', x);
            let src = format!("{}pub fn main() {{\n {}\n}}\n", PRE, badan);
            let i = jawaban_ringkas(&jalankan_keluaran_penuh(&src, false));
            let v = jawaban_ringkas(&jalankan_keluaran_penuh(&src, true));
            diperiksa += 1;
            // "Tidak ditolak" ikut dituntut: menolak semuanya juga membuat kedua backend sama.
            let masalah = if i != v {
                Some(format!("beda — interp: {} | vm: {}", i, v))
            } else if i.starts_with("error") {
                Some(format!("ditolak: {}", i))
            } else {
                match *cek {
                    "tipe" if i != *tipe => Some(format!("jenisnya `{}`, harusnya `{}`", i, tipe)),
                    "tampil" if i.contains("::") => Some(format!("tampil sebagai enum: {}", i)),
                    "cocok" if i != "-2" => Some(format!("dicocokkan sebagai enum: {}", i)),
                    "json" if !i.contains("\"variant\"") || (x.contains("__type__") && !i.contains("__type__")) =>
                        Some(format!("JSON membuang kunci pemakai: {}", i)),
                    _ => None,
                }
            };
            if let Some(m) = masalah {
                salah.push(format!("  {} — {}", badan.replace('\n', ";"), m));
            }
        }
    }
    assert_eq!(diperiksa, DATA.len() * AMATI.len(), "sapuannya tidak berjalan penuh");
    assert!(salah.is_empty(), "{} pengamatan salah:\n{}", salah.len(), salah.join("\n"));
}

/// **Satu kalimat untuk "argumen ke-N harus array", di setiap pintu.**
#[test]
fn argumen_bukan_array_pakai_satu_kalimat_di_tiap_pintu() {
    const KASUS: &[&str] = &[
        "arr::concat(5, [1])", "arr::concat([1], 5)", "arr::map(5, |x| x)",
        "arr::zip([1], 5)", "str::join(5, \",\")", "rand::sample(5, 1)",
        "arr::sort(5)", "arr::filter(5, |x| x)", "arr::reduce(5, |a,b| a, 0)",
        "arr::min(5)", "arr::max(5)",
        // Dulu dilayani penyaring BENTUK argumen di VM; sejak Fase FUNGSI-DATA lewat
        // kalimat bersama juga.
        "arr::push(5, 1)", "arr::sum(5)", "arr::reverse(5)",
    ];
    // Kosong, dan tetap ada: pintu berikutnya yang kalimatnya belum bersama punya rumah.
    const MEKANISME_LAIN: &[&str] = &[];

    // Baris yang MULAI dengan `error`, bukan baris pertama: diagnostik di sini
    // didahului satu baris SAPAAN yang eskalasi isinya mengikuti berapa kali …
    let pesan = |e: &str, vm: bool| -> String {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", e);
        let keluar = jalankan_keluaran_penuh(&src, vm);
        keluar.lines()
            .find(|l| l.trim_start().starts_with("error"))
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let mut beda = Vec::new();
    for e in KASUS {
        let (i, v) = (pesan(e, false), pesan(e, true));
        assert!(i.contains("harus array"),
            "`{}` di interpreter tidak lagi lewat pesan bersamanya: {}", e, i);
        if i != v { beda.push(format!("  {}\n    int: {}\n    vm : {}", e, i, v)); }
    }
    assert!(beda.is_empty(), "{} pesan argumen-bukan-array berbeda antar-backend:\n{}",
        beda.len(), beda.join("\n"));

    // Sisi sebaliknya: yang didaftar sebagai beda harus MASIH beda.
    for e in MEKANISME_LAIN {
        let (i, v) = (pesan(e, false), pesan(e, true));
        assert_ne!(i, v,
            "KABAR BAIK: `{}` sekarang sama di kedua backend — keluarkan dia dari \
             `MEKANISME_LAIN`, karena daftar yang basi berhenti berarti", e);
    }
}

/// **Satu kalimat untuk "index di luar batas", di setiap pintu.**
#[test]
fn index_luar_batas_pakai_satu_kalimat_di_tiap_pintu() {
    let kasus = [
        "[1,2,3].insert(99, 9)", "arr::insert([1,2,3], 99, 9)",
        "[1,2,3].remove(99)",    "arr::remove([1,2,3], 99)",
        "[1,2,3].swap(0, 99)",   "arr::swap([1,2,3], 0, 99)",
        "\"halo\".char_at(99)",  "str::char_at(\"halo\", 99)",
    ];
    for ekspr in kasus {
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", ekspr);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        for (nama, keluar) in [("interpreter", &i), ("--vm", &v)] {
            assert!(keluar.contains("index 99 di luar batas (panjang"),
                "{} atas `{}`: kalimatnya bukan yang bersama;\n{}", nama, ekspr, keluar);
            assert!(!keluar.contains("OOB") && !keluar.contains("len="),
                "{} atas `{}`: masih memakai singkatan Inggris;\n{}", nama, ekspr, keluar);
        }
        // Yang dibandingkan BARIS `error:`-nya saja: keluaran penuh memuat path
        // berkas sementara yang memang berbeda tiap jalan, dan membandingkannya utuh …
        let baris = |t: &str| t.lines().find(|l| l.starts_with("error:"))
            .unwrap_or("").to_string();
        assert_eq!(baris(&i), baris(&v), "`{}`: dua backend, dua kalimat", ekspr);
    }
    // Sisi POSITIF: index yang sah tetap bekerja di kedua bentuk. Sebuah
    // penjaga yang menolak semuanya juga membuat separuh di atas hijau.
    let src = "package main\npub fn main() {\n \
               wok([1,2,3].insert(1, 9))\n wok(arr::swap([1,2,3], 2, 0))\n \
               wok(\"halo\".char_at(1)) }\n";
    for (nama, vm) in [("interpreter", false), ("--vm", true)] {
        let keluar = jalankan_keluaran_penuh(src, vm);
        assert!(keluar.contains("[1, 9, 2, 3]") && keluar.contains("[3, 2, 1]"),
            "{}: index yang sah harus tetap bekerja;\n{}", nama, keluar);
    }
}

/// Tiap modul `std/` yang terdaftar harus benar-benar MEMUAT di kedua
/// backend.
#[test]
fn modul_memanggil_fungsinya_sendiri_bukan_milik_modul_lain() {
    use std::process::Command;
    let dir = crate::bersama::dir_unik("jrg-bentrok-modul");
    std::fs::create_dir_all(&dir).expect("bikin dir");

    // Tiga bentuk tabrakan sekaligus: fungsi biasa, fungsi yang dipanggil
    // lewat fungsi lain, dan nama yang juga dipakai modul KETIGA.
    std::fs::write(dir.join("satu.jrg"), concat!(
        "package satu\n",
        "pub fn nilai() { return \"SATU\" }\n",
        "pub fn ambil() { return nilai() }\n",
        "pub fn lewat() { return ambil() }\n")).unwrap();
    std::fs::write(dir.join("dua.jrg"), concat!(
        "package dua\n",
        "pub fn nilai() { return \"DUA\" }\n",
        "pub fn ambil() { return nilai() }\n")).unwrap();
    std::fs::write(dir.join("tiga.jrg"), concat!(
        "package tiga\n",
        "pub fn nilai() { return \"TIGA\" }\n")).unwrap();
    let utama = dir.join("utama.jrg");
    std::fs::write(&utama, concat!(
        "package main\n",
        "import \"satu\"\n import \"dua\"\n import \"tiga\"\n",
        "pub fn main() {\n",
        " wok(satu::ambil())\n wok(dua::ambil())\n",
        " wok(satu::lewat())\n wok(tiga::nilai())\n",
        "}\n")).unwrap();

    let harap = "SATU\nDUA\nSATU\nTIGA";
    let mut salah = Vec::new();
    for vm in [false, true] {
        let mut cmd = Command::new(jrgc());
        cmd.arg(&utama).arg("--sopan").arg("-I").arg(&dir);
        if vm { cmd.arg("--vm"); }
        let out = cmd.output().expect("jalankan jrgc");
        let teks = format!("{}{}", String::from_utf8_lossy(&out.stdout),
                                   String::from_utf8_lossy(&out.stderr));
        let dapat = teks.trim().replace("\r\n", "\n");
        if dapat != harap {
            salah.push(format!("  [{}] harap {:?}, dapat {:?}",
                if vm { "--vm" } else { "interp" }, harap, dapat));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(salah.is_empty(),
        "sebuah modul memanggil fungsi milik modul LAIN:\n{}", salah.join("\n"));
}

/// `uang` menghitung dengan bilangan bulat, jadi yang float tidak bisa.
#[test]
fn uang_menjumlah_tanpa_kehilangan_sepeser_pun() {
    let kasus: &[(&str, &str)] = &[
        // Seribu kali Rp 0,10. Dengan float: 99.9999999999986.
        ("{ var t = uang::nol(2)\n for i in range(0, 1000) { t = t.tambah(uang::rupiah(0.10)) }\n            wok(t.teks(), t.sama(uang::rupiah(100))) }", "100.00 true"),
        // Bagi tiga TANPA sisa yang hilang.
        ("wok(arr::map(uang::rupiah(100).bagi_rata(3), |u| u.teks()))",
         "[33.34, 33.33, 33.33]"),
        ("wok(uang::jumlah(uang::rupiah(100).bagi_rata(3), 2).teks())", "100.00"),
        ("wok(uang::jumlah(uang::rupiah(10).bagi_rata(7), 2).teks())", "10.00"),
        // Bentuk tampilan.
        ("wok(uang::rupiah(1999950.5).rupiah())", "Rp 1.999.950,50"),
        ("wok(uang::rupiah(0-2500).rupiah())", "-Rp 2.500,00"),
        ("wok(uang::rupiah(19.99).persen(15).teks())", "3.00"),
        ("wok(uang::dari(1234567, 0).internasional(\"$\"))", "$1,234,567"),
        ("wok(uang::rupiah(19.99).kali(3).teks())", "59.97"),
        ("wok(uang::rupiah(100).kurang(uang::rupiah(0.01)).teks())", "99.99"),
        ("wok(uang::dari(1.005, 2).ke_desimal(1).teks())", "1.0"),
        ("wok(uang::rupiah(5).bulat())", "5"),
    ];
    let mut salah = Vec::new();
    for (badan, harap) in kasus {
        let src = format!("package main\nimport \"std/uang\"\npub fn main() {}\n",
            if badan.starts_with('{') { badan.to_string() } else { format!("{{ {} }}", badan) });
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        if i.keluaran != *harap || v.keluaran != *harap {
            salah.push(format!("  {}\n    harap: {}\n    int  : {:?}\n    vm   : {:?}",
                badan, harap, i.keluaran, v.keluaran));
        }
    }
    assert!(salah.is_empty(), "{} dari {} bentuk `uang` menjawab lain:\n{}",
        salah.len(), kasus.len(), salah.join("\n"));
}

/// `tabel` menghitung lebar kolomnya, tidak menebaknya.
#[test]
fn tabel_menghitung_lebar_kolom_bukan_menebaknya() {
    let src = concat!(
        "package main\nimport \"std/tabel\"\npub fn main() {\n",
        " let t = tabel::baru([\"Kota\", \"Barang\", \"Total\"])\n",
        "  .baris_baru([\"Solo\", \"kopi\", \"59.97\"])\n",
        "  .baris_baru([\"Bandar Lampung\", \"teh tarik spesial\", \"1250000.00\"])\n",
        " wok(t.teks())\n}\n");
    let i = jalankan(&src, false);
    let v = jalankan(&src, true);
    assert_eq!(i.keluaran, v.keluaran, "tabel beda antar-mesin");
    let baris: Vec<&str> = i.keluaran.lines().collect();
    assert_eq!(baris.len(), 4, "harus kepala + garis + dua baris:\n{}", i.keluaran);

    // Semua isi utuh — tidak ada yang terpotong atau menempel.
    for kata in ["Bandar Lampung", "teh tarik spesial", "1250000.00"] {
        assert!(i.keluaran.contains(kata), "{:?} hilang dari tabel:\n{}", kata, i.keluaran);
    }
    assert!(!i.keluaran.contains("Lampungteh"),
        "kolom menempel — lebarnya ditebak, bukan dihitung:\n{}", i.keluaran);

    // Angka rata KANAN: `59.97` sejajar ujung dengan `1250000.00`.
    let akhir: Vec<usize> = baris[2..].iter().map(|b| b.trim_end().len()).collect();
    assert!(akhir.iter().all(|n| *n == akhir[0]),
        "kolom angka tidak rata kanan: {:?}\n{}", akhir, i.keluaran);

    // Lebar TAMPIL, bukan jumlah huruf: `東京` dua huruf, empat kolom layar — dulu kolom
    // sesudahnya bergeser dua kolom ke kiri di baris itu saja.
    let src2 = concat!(
        "package main\nimport \"std/tabel\"\npub fn main() {\n",
        " let t = tabel::baru([\"Kota\", \"Total\"])\n",
        "  .baris_baru([\"東京\", \"1\"])\n",
        "  .baris_baru([\"Solo\", \"22\"])\n",
        " wok(t.teks())\n}\n");
    let j = jalankan(src2, false);
    assert_eq!(j.keluaran, jalankan(src2, true).keluaran, "tabel lebar beda antar-mesin");
    let tampil = |b: &str| b.trim_end().chars().map(jrgc::lebar::karakter).sum::<usize>();
    let ujung: Vec<usize> = j.keluaran.lines().map(tampil).collect();
    assert!(ujung.len() == 4 && ujung.iter().all(|n| *n == ujung[0]),
        "kolom tidak rata menurut lebar tampil: {:?}\n{}", ujung, j.keluaran);
}

/// Bentuk dari bahasa lain dijawab PADANANNYA, bukan galat parse generik.
#[test]
fn bentuk_bahasa_lain_dijawab_padanannya() {
    // (program, kata yang WAJIB muncul di pesannya)
    let kasus: &[(&str, &str)] = &[
        ("var i = 0\n i++", "`i += 1`"),
        ("var i = 0\n ++i", "`i += 1`"),
        ("var i = 1\n i--", "`i -= 1`"),
        ("var i = 5\n wok(--i)", "`i -= 1`"),
        ("wok(true and false)", "pakai `&&`"),
        ("wok(true or false)", "pakai `||`"),
        ("wok(not true)", "pakai `!`"),
        ("wok(1 is 1)", "pakai `==`"),
        ("var a = 1\n if a = 1 { wok(1) }", "`==`"),
        ("let n = 1\n wok(f\"halo ${n}\")", "template"),
        ("wok(null)", "`nil`"),
        ("wok(undefined)", "`nil`"),
        ("wok(console)", "`wok(...)`"),
        ("wok(println)", "`wok(...)`"),
        ("wok(elif)", "`else if`"),
        ("wok(\"%d\" % 5)", "str::format"),
    ];
    let mut salah = Vec::new();
    for (badan, kata) in kasus {
        let src = format!("package main\npub fn main() {{\n {}\n}}\n", badan);
        for vm in [false, true] {
            let h = jalankan_keluaran_penuh(&src, vm);
            let mesin = if vm { "--vm" } else { "interp" };
            if !h.contains("error") {
                salah.push(format!("  {:?} [{}] TIDAK ditolak — jawabannya {}",
                    badan, mesin, jawaban_ringkas(&h)));
            } else if !h.contains(kata) {
                salah.push(format!("  {:?} [{}] tidak menyebut {:?}:\n    {}",
                    badan, mesin, kata, jawaban_ringkas(&h)));
            }
        }
    }
    assert!(salah.is_empty(), "{} bentuk bahasa lain tidak dijelaskan:\n{}",
        salah.len(), salah.join("\n"));
}

/// Bentuk JRG yang SAH tidak boleh ikut tertolak penjaga di atas.
#[test]
fn penjaga_bentuk_asing_tidak_menolak_yang_sah() {
    let sah: &[(&str, &str)] = &[
        ("wok(1 - -2)", "3"),
        ("wok(- -5)", "5"),
        ("wok(5 - 3)", "2"),
        ("wok(-5)", "-5"),
        ("var a = 5\n a -= 1\n wok(a)", "4"),
        ("var a = 1\n a += 1\n wok(a)", "2"),
        ("let a = 1\n if a == 1 { wok(\"ok\") }", "ok"),
        ("wok(7 % 3)", "1"),
        ("wok(true && false)", "false"),
        ("for x in [1, 2] { wok_(x) }", "01"),
        ("let f = 2\n wok(f)", "2"),
        ("wok(\"\"\"x ${1}\"\"\")", "x 1"),
    ];
    let mut salah = Vec::new();
    for (badan, harap) in sah {
        let src = format!("package main\npub fn main() {{\n {}\n}}\n", badan);
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        if i.keluaran != *harap || v.keluaran != *harap {
            salah.push(format!("  {:?}\n    harap: {}\n    int  : {:?}\n    vm   : {:?}",
                badan, harap, i.keluaran, v.keluaran));
        }
    }
    assert!(salah.is_empty(), "{} bentuk SAH ikut tertolak:\n{}",
        salah.len(), salah.join("\n"));
}

/// Tidak ada pesan galat yang memuat deretan spasi di tengah kalimat.
#[test]
fn pesan_galat_tidak_punya_spasi_liar() {
    let program: &[&str] = &[
        "wok(\"%d\" % 5)", "wok(null)", "wok(console)", "var i = 0\n i++",
        "wok(true and false)", "wok([1,2][9])", "wok([1,2].lenn())",
        "var a = 1\n if a = 1 { wok(1) }", "wok(1/0)", "wok(nam)",
        "let n = 1\n wok(f\"x ${n}\")", "wok(undefined)",
    ];
    let mut salah = Vec::new();
    for badan in program {
        let src = format!("package main\npub fn main() {{\n {}\n}}\n", badan);
        for vm in [false, true] {
            for baris in jalankan_keluaran_penuh(&src, vm).lines() {
                let b = baris.trim();
                // Baris cuplikan sumber dan caret memang memakai spasi untuk
                // menyejajarkan; yang diperiksa kalimat diagnostiknya.
                if !b.starts_with("error") && !b.starts_with("bantuan") { continue; }
                if b.split_whitespace().count() > 1 && b.contains("  ") {
                    salah.push(format!("  [{}] {:?}", if vm { "--vm" } else { "interp" }, b));
                }
            }
        }
    }
    assert!(salah.is_empty(), "{} pesan punya deretan spasi di tengah kalimat:\n{}",
        salah.len(), salah.join("\n"));
}

/// `teks` menjawab hal yang SAMA untuk array dan untuk tulisan.
#[test]
fn teks_melayani_array_dan_tulisan_dengan_method_yang_sama() {
    // (potongan program, jawaban yang diharapkan)
    let kasus: &[(&str, &str)] = &[
        // Bentuk yang sama, dua jenis isi.
        ("teks::dari([3,1,2]).panjang()", "3"),
        ("teks::dari(\"halo\").panjang()", "4"),
        ("teks::dari([3,1,2]).urut().daftar()", "[1, 2, 3]"),
        ("teks::dari(\"cab\").urut().jadi_teks()", "abc"),
        ("teks::dari([1,2,3]).saring(|x| x > 1).daftar()", "[2, 3]"),
        ("teks::dari(\"halo\").balik().jadi_teks()", "olah"),
        // Yang mungkin tidak ada menjawab Option — bukan nil, bukan -1.
        ("teks::dari([1,2]).ambil(9)", "Option::None"),
        ("teks::dari([1,2]).ambil(0-1)", "Option::Some(2)"),
        ("teks::dari([]).terbesar()", "Option::None"),
        ("teks::dari([1,2]).cari(9)", "Option::None"),
        ("teks::dari([1,2]).ambil_atau(9, \"kosong\")", "kosong"),
        // Berantai, dan tidak mengubah asalnya.
        ("teks::dari([1,2,3]).peta(|x| x*2).saring(|x| x>2).jumlah()", "10"),
        ("teks::dari(\"a,b,c\").pisah(\",\").urut_turun().sambung(\"-\")", "c-b-a"),
        ("teks::angka(1,5).lipat(0, |a,b| a+b)", "10"),
        ("teks::dari([1,2,2,3]).unik().daftar()", "[1, 2, 3]"),
        ("teks::dari(\"  hai  \").rapikan().besar().jadi_teks()", "HAI"),
    ];
    let mut salah = Vec::new();
    for (ekspr, harap) in kasus {
        let src = format!(
            "package main\nimport \"std/teks\"\npub fn main() {{ wok({}) }}\n", ekspr);
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        if i.keluaran != *harap || v.keluaran != *harap {
            salah.push(format!("  {}\n    harap: {}\n    int  : {:?}\n    vm   : {:?}",
                ekspr, harap, i.keluaran, v.keluaran));
        }
    }
    assert!(salah.is_empty(), "{} dari {} bentuk `teks` menjawab lain:\n{}",
        salah.len(), kasus.len(), salah.join("\n"));
}

/// Yang salah DILAPORKAN, dan kalimatnya menyebut jalan keluarnya.
#[test]
fn teks_melaporkan_yang_salah_dengan_kalimat_yang_menolong() {
    let kasus: &[(&str, &str)] = &[
        ("teks::dari([1,2,3]).buang(9)", "isinya cuma 3"),
        ("teks::dari([]).rata()", "kosong"),
        ("teks::dari([1,2]).saring(5)", "butuh sebuah fungsi"),
        ("teks::dari([1,2]).ambil_awal(0-3)", "tidak boleh negatif"),
        ("teks::ulang(1, 0-2)", "tidak boleh negatif"),
    ];
    let mut salah = Vec::new();
    for (ekspr, kata) in kasus {
        let src = format!(
            "package main\nimport \"std/teks\"\npub fn main() {{ wok({}) }}\n", ekspr);
        for vm in [false, true] {
            let h = jalankan_keluaran_penuh(&src, vm);
            let mesin = if vm { "--vm" } else { "interp" };
            if !h.contains(kata) {
                salah.push(format!("  {} [{}] tidak menyebut {:?}:\n    {}",
                    ekspr, mesin, kata, jawaban_ringkas(&h)));
            }
        }
    }
    assert!(salah.is_empty(), "{} kalimat galat `teks` tidak menolong:\n{}",
        salah.len(), salah.join("\n"));
}

#[test]
fn tiap_modul_std_bisa_di_import_di_dua_backend() {
    let mut gagal = Vec::new();
    for nama in jrgc::interpreter::stdlib_src::nama_modul() {
        let src = format!(
            "package main\nimport \"std/{}\"\npub fn main() {{ wok(\"MUAT\") }}\n",
            nama
        );
        for vm in [false, true] {
            let h = jalankan(&src, vm);
            if !h.sukses || h.keluaran != "MUAT" {
                gagal.push(format!(
                    "std/{} di {} -> sukses={} keluaran={:?}",
                    nama, if vm { "--vm" } else { "interp" }, h.sukses, h.keluaran
                ));
            }
        }
    }
    assert!(gagal.is_empty(), "modul std tidak bisa di-import:\n  {}", gagal.join("\n  "));
}

/// Sebuah modul milik pemakai MENANG atas nama namespace bawaan.
#[test]
fn modul_pemakai_menang_atas_nama_namespace_bawaan() {
    let dir = std::env::temp_dir().join(format!("jrg-modns-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("bikin dir");
    std::fs::write(dir.join("math.jrg"),
        "package math\npub fn tambah_dua(x) { return x + 2 }\n").expect("tulis modul");
    let utama = dir.join("main.jrg");
    std::fs::write(&utama,
        "package main\nimport \"math\"\npub fn main() { wok(math::tambah_dua(5)) }\n")
        .expect("tulis utama");

    for vm in [false, true] {
        let mut cmd = Command::new(jrgc());
        cmd.arg(&utama);
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        let teks = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(
            teks, "7",
            "modul pemakai `math.jrg` tertelan namespace bawaan di {}",
            if vm { "--vm" } else { "interp" }
        );
    }

    // Sisi sebaliknya: tanpa berkasnya, importnya tetap dilewati diam-diam
    // supaya program lama yang menulis `import "std/fmt"` tidak rusak.
    let dir2 = dir.join("kosong");
    std::fs::create_dir_all(&dir2).expect("bikin dir2");
    let u2 = dir2.join("main.jrg");
    std::fs::write(&u2,
        "package main\nimport \"std/fmt\"\nimport \"math\"\npub fn main() { wok(math::abs(-3)) }\n")
        .expect("tulis utama2");
    for vm in [false, true] {
        let mut cmd = Command::new(jrgc());
        cmd.arg(&u2);
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(), "3",
            "import namespace bawaan berhenti bekerja di {}",
            if vm { "--vm" } else { "interp" }
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Tiap nama KIND yang bisa dijawab `typeof` harus bisa ditulis sebagai
/// anotasi tipe.
#[test]
fn tiap_nama_typeof_bisa_ditulis_sebagai_anotasi() {
    const BENTUK: &[&str] = &[
        "1", "1.5", "\"a\"", "true", "'a'", "[1]", "(1,2)",
        "dict::new()", "|x| x", "wok", "()",
        // Handle: `typeof` menjawab nama kind-nya, dan nama itu harus bisa ditulis juga.
        "Canvas::new(1, 1)", "Chan::new(1)",
    ];
    let mut gagal = Vec::new();
    for e in BENTUK {
        let src = format!("package main\npub fn main() {{ wok(typeof({})) }}\n", e);
        let nama = jalankan(&src, false).keluaran;
        if nama.is_empty() || nama == "nil" { continue; }
        let src2 = format!(
            "package main\npub fn main() {{ let x: {} = 1\n wok(\"OK\") }}\n", nama);
        for vm in [false, true] {
            let h = jalankan(&src2, vm);
            if h.keluaran != "OK" {
                gagal.push(format!(
                    "typeof({}) = `{}` tidak bisa ditulis sebagai anotasi di {}",
                    e, nama, if vm { "--vm" } else { "interp" }
                ));
            }
        }
    }
    assert!(gagal.is_empty(), "kosakata typeof dan anotasi berselisih:\n  {}", gagal.join("\n  "));
}

/// FFI: array bertipe (`[]i32`, `[]f64`, …) diserahkan ke C.
mod ffi_array {
    use super::*;

    const SUMBER: &str = r#"
#include <stdint.h>
#include <stdbool.h>
long long s_i8 (const int8_t*  a, int n){ long long t=0; for(int i=0;i<n;i++) t+=a[i]; return t; }
long long s_i16(const int16_t* a, int n){ long long t=0; for(int i=0;i<n;i++) t+=a[i]; return t; }
long long s_i32(const int32_t* a, int n){ long long t=0; for(int i=0;i<n;i++) t+=a[i]; return t; }
long long s_i64(const int64_t* a, int n){ long long t=0; for(int i=0;i<n;i++) t+=a[i]; return t; }
long long s_u8 (const uint8_t* a, int n){ long long t=0; for(int i=0;i<n;i++) t+=a[i]; return t; }
long long s_u32(const uint32_t*a, int n){ long long t=0; for(int i=0;i<n;i++) t+=a[i]; return t; }
double    s_f32(const float*   a, int n){ double t=0;    for(int i=0;i<n;i++) t+=a[i]; return t; }
double    s_f64(const double*  a, int n){ double t=0;    for(int i=0;i<n;i++) t+=a[i]; return t; }
int       s_bool(const bool*   a, int n){ int t=0;       for(int i=0;i<n;i++) t+=a[i]?1:0; return t; }
long long pertama_i32(const int32_t* a){ return a[0]; }
"#;

    fn siapkan() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let ada = Command::new("gcc").arg("--version").output()
            .map(|o| o.status.success()).unwrap_or(false);
        if !ada {
            eprintln!("LEWAT: `gcc` tidak ada, array FFI tidak bisa diuji di mesin ini");
            return None;
        }
        let dir = crate::bersama::dir_unik("jrg-ffi-arr");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        let src = dir.join("elem.c");
        std::fs::write(&src, SUMBER).expect("tulis c");
        let lib = dir.join("libelem.so");
        let out = Command::new("gcc").args(["-shared", "-fPIC", "-O1", "-o"])
            .arg(&lib).arg(&src).output().expect("jalankan gcc");
        assert!(out.status.success(), "gcc gagal: {}", String::from_utf8_lossy(&out.stderr));
        Some((dir, lib))
    }

    const DEKLARASI: &str = "\
        fn s_i8(a: []i8, n: i32) -> i64\n\
        fn s_i16(a: []i16, n: i32) -> i64\n\
        fn s_i32(a: []i32, n: i32) -> i64\n\
        fn s_i64(a: []i64, n: i32) -> i64\n\
        fn s_u8(a: []u8, n: i32) -> i64\n\
        fn s_u32(a: []u32, n: i32) -> i64\n\
        fn s_f32(a: []f32, n: i32) -> f64\n\
        fn s_f64(a: []f64, n: i32) -> f64\n\
        fn s_bool(a: []bool, n: i32) -> i32\n\
        fn pertama_i32(a: []i32) -> i64\n";

    #[test]
    fn array_bertipe_sampai_ke_c_dengan_lebar_dan_urutan_yang_benar() {
        let Some((dir, lib)) = siapkan() else { return };
        let src = format!(
            "package main\nextern \"C\" \"{}\" {{\n{}}}\npub fn main() {{\n\
             wok(s_i8([0-128, 127, 1], 3))\n wok(s_i16([32767, 0-32768], 2))\n\
             wok(s_i32([1000000, 0-1000000, 7], 3))\n\
             wok(s_i64([9007199254740993, 1], 2))\n\
             wok(s_u8([255, 1], 2))\n wok(s_u32([4294967295], 1))\n\
             wok(s_f32([1.5, 2.25], 2))\n wok(s_f64([0.1, 0.2], 2))\n\
             wok(s_bool([true, false, true], 3))\n\
             wok(pertama_i32([305419896]))\n\
             wok(s_i32([], 0))\n}}\n",
            lib.display(), DEKLARASI);
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(i.sukses, "interpreter gagal: {}", i.keluaran);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda untuk array FFI");
        // Jawaban dari C, bukan dari harapan yang ditulis di sini.
        for harus in [
            "Result::Ok(0)",                    // i8: -128 + 127 + 1
            "Result::Ok(-1)",                   // i16: 32767 + (-32768)
            "Result::Ok(7)",                    // i32
            "Result::Ok(9007199254740994)",     // i64 di luar jangkauan f64
            "Result::Ok(256)",                  // u8: 255 + 1
            "Result::Ok(4294967295)",           // u32 maksimum
            "Result::Ok(3.75)",                 // f32: 1.5 + 2.25
            "Result::Ok(0.30000000000000004)",  // f64: galat float DIPERTAHANKAN
            "Result::Ok(2)",                    // bool
            "Result::Ok(305419896)",            // 0x12345678 — urutan byte
        ] {
            assert!(i.keluaran.contains(harus),
                "jawaban C {:?} tidak muncul:\n{}", harus, i.keluaran);
        }
    }

    /// Nilai yang tidak muat DILAPORKAN, tidak dipotong diam-diam.
    #[test]
    fn isi_array_yang_tidak_muat_dilaporkan_bukan_dipotong() {
        let Some((dir, lib)) = siapkan() else { return };
        let kasus: &[(&str, &str, &str)] = &[
            ("fn s_i8(a: []i8, n: i32) -> i64", "s_i8([200], 1)", "tidak muat"),
            ("fn s_u8(a: []u8, n: i32) -> i64", "s_u8([0-1], 1)", "tidak muat"),
            ("fn s_i32(a: []i32, n: i32) -> i64", "s_i32([3000000000], 1)", "tidak muat"),
            ("fn s_i32(a: []i32, n: i32) -> i64", "s_i32([1, \"a\"], 2)", "elemen ke-1"),
            ("fn s_i32(a: []i32, n: i32) -> i64", "s_i32(5, 1)", "butuh array"),
            ("fn s_i32(a: []string, n: i32) -> i64", "s_i32([\"a\"], 1)", "cuma boleh berisi angka"),
        ];
        let mut salah = Vec::new();
        for (sig, panggil, kata) in kasus {
            let src = format!(
                "package main\nextern \"C\" \"{}\" {{\n{}\n}}\npub fn main() {{ wok({}) }}\n",
                lib.display(), sig, panggil);
            for vm in [false, true] {
                let h = jalankan(&src, vm);
                if !h.keluaran.contains(kata) {
                    salah.push(format!("  {} [{}] tidak menyebut {:?}: {}",
                        panggil, if vm { "--vm" } else { "interp" }, kata, h.keluaran));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(salah.is_empty(), "{} penolakan array FFI tidak jelas:\n{}",
            salah.len(), salah.join("\n"));
    }

    /// Yang BELUM didukung ditolak sambil menyebut jalan keluarnya.
    #[test]
    fn yang_belum_didukung_menyebut_jalan_keluarnya() {
        let Some((dir, lib)) = siapkan() else { return };
        let kasus: &[(&str, &str, &str, &str)] = &[
            ("struct Titik { pub x: int, pub y: int }\n",
             "fn f(t: Titik) -> i32", "f(Titik{x:1,y:2})", "kirim fieldnya"),
            ("pub fn dobel(x) { return x * 2 }\n",
             "fn g(f: fn(i32) -> i32) -> i32", "g(dobel)", "`py::`"),
            // Jalur PARAMETER-nya sah, jadi tanpa penjaga tersendiri bentuk ini
            // terkompilasi dan menjawab `()` — hasil salah yang diam.
            ("", "fn s_i32(a: []i32, n: i32) -> []i32", "s_i32([1], 1)",
             "tidak membawa panjangnya"),
        ];
        let mut salah = Vec::new();
        for (pra, sig, panggil, kata) in kasus {
            let src = format!(
                "package main\n{}extern \"C\" \"{}\" {{\n{}\n}}\npub fn main() {{ wok({}) }}\n",
                pra, lib.display(), sig, panggil);
            for vm in [false, true] {
                let h = jalankan(&src, vm);
                if !h.keluaran.contains(kata) {
                    salah.push(format!("  {} [{}] tidak menyebut {:?}: {}",
                        panggil, if vm { "--vm" } else { "interp" }, kata, h.keluaran));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(salah.is_empty(), "{} penolakan FFI tidak mengajar:\n{}",
            salah.len(), salah.join("\n"));
    }
}

/// FFI C++: memanggil fungsi C++ yang TIDAK dibungkus `extern "C"`.
mod cpp {
    use super::*;

    fn ada_gpp() -> bool {
        Command::new("g++").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    const SUMBER: &str = r#"
int add(int a, int b) { return a + b; }
double scale(double x, int n) { return x * n; }
long jumlah(long a, long b) { return a + b; }
bool positif(double x) { return x > 0.0; }
const char* salam() { return "halo dari C++"; }
int panjang(const char* s) { int n = 0; while (s[n]) n++; return n; }
float setengah(float x) { return x / 2.0f; }
namespace math { int mul(int a, int b) { return a * b; } }
namespace math { int abs(int x) { return x < 0 ? -x * 11 : x * 11; } }
namespace a { namespace b { int dalam(int x) { return x + 100; } } }
int isi_ptr(int* p) { return *p * 10; }
void tulis_ptr(int* p, int v) { *p = v; }
"#;

    /// Kompilasi pustaka uji; `None` kalau `g++` tidak ada.
    fn siapkan() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        if !ada_gpp() {
            eprintln!("LEWAT: `g++` tidak ada, FFI C++ tidak bisa diuji di mesin ini");
            return None;
        }
        // Beberapa THREAD tes berbagi satu PID, jadi nomor urut se-proses yang membedakannya.
        let dir = crate::bersama::dir_unik("jrg-cpp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        let src = dir.join("uji.cpp");
        std::fs::write(&src, SUMBER).expect("tulis cpp");
        let lib = dir.join("libuji.so");
        let out = Command::new("g++")
            .args(["-shared", "-fPIC", "-O1", "-o"])
            .arg(&lib).arg(&src)
            .output().expect("jalankan g++");
        assert!(out.status.success(), "g++ gagal: {}", String::from_utf8_lossy(&out.stderr));
        Some((dir, lib))
    }

    #[test]
    fn fungsi_cpp_dipanggil_tanpa_pembungkus_extern_c() {
        let Some((dir, lib)) = siapkan() else { return };
        let src = format!(
            "package main\n\
             extern \"C++\" \"{}\" {{\n\
             fn add(a: i32, b: i32) -> i32\n\
             fn scale(x: f64, n: i32) -> f64\n\
             fn jumlah(a: i64, b: i64) -> i64\n\
             fn positif(x: f64) -> bool\n\
             fn salam() -> string\n\
             fn panjang(s: string) -> i32\n\
             fn setengah(x: f32) -> f32\n\
             fn math::mul(a: i32, b: i32) -> i32\n\
             fn math::abs(x: i32) -> i32\n\
             fn a::b::dalam(x: i32) -> i32\n\
             }}\n\
             pub fn main() {{\n\
             wok(add(2, 3))\n wok(scale(1.5, 4))\n wok(jumlah(1000000, 2000000))\n\
             wok(positif(-1.0))\n wok(positif(2.0))\n wok(salam())\n\
             wok(panjang(\"halo\"))\n wok(setengah(5.0))\n\
             wok(math::mul(6, 7))\n wok(a::b::dalam(5))\n wok(math::abs(-5))\n }}\n",
            lib.display()
        );
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert!(i.sukses, "interpreter gagal: {}", i.keluaran);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda untuk FFI C++");
        for harus in [
            "Result::Ok(5)", "Result::Ok(6.0)", "Result::Ok(3000000)",
            "Result::Ok(false)", "Result::Ok(true)", "Result::Ok(halo dari C++)",
            "Result::Ok(4)", "Result::Ok(2.5)", "Result::Ok(42)", "Result::Ok(105)",
            // `math::abs` yang DIDEKLARASIKAN menang atas bawaan bernama sama (`5`) — di
            // `--vm` sempat kalah sesudah bawaan dirujuk lewat nilai (Fase RUANG).
            "Result::Ok(55)",
        ] {
            assert!(i.keluaran.contains(harus), "hilang `{}` dari:\n{}", harus, i.keluaran);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tanda tangan yang meleset harus gagal dengan menyebut simbol yang
    /// DICARI. Tanpa nama itu, pembacanya cuma tahu "simbol tidak ada" dan
    /// akan memeriksa pustakanya — padahal yang salah tanda tangannya.
    #[test]
    fn tanda_tangan_meleset_menyebut_simbol_yang_dicari() {
        let Some((dir, lib)) = siapkan() else { return };
        let src = format!(
            "package main\n\
             extern \"C++\" \"{}\" {{ fn add(a: i64, b: i64) -> i64 }}\n\
             pub fn main() {{ wok(add(2, 3)) }}\n",
            lib.display()
        );
        for vm in [false, true] {
            let keluaran = jalankan(&src, vm).keluaran;
            assert!(
                keluaran.contains("_Z3addll"),
                "pesan tidak menyebut simbol yang dicari di {}: {}",
                if vm { "--vm" } else { "interp" }, keluaran
            );
            assert!(
                keluaran.contains("nm -D"),
                "pesan tidak menyebut cara membandingkannya: {}", keluaran
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tipe yang DITUNJUK sebuah pointer ikut menentukan simbolnya: `int*` adalah
    /// `Pi` dan `void*` adalah `Pv`.
    #[test]
    fn tipe_yang_ditunjuk_pointer_ikut_di_mangle() {
        let Some((dir, lib)) = siapkan() else { return };
        let src = format!(
            "package main\n\
             extern \"C\" {{ fn malloc(n: i64) -> *mut i32\n fn free(p: *mut i32) }}\n\
             extern \"C++\" \"{}\" {{\n\
             fn isi_ptr(p: *mut i32) -> i32\n\
             fn tulis_ptr(p: *mut i32, v: i32)\n\
             }}\n\
             pub fn main() {{\n\
             let p = malloc(8)?\n tulis_ptr(p, 7)?\n wok(isi_ptr(p)?)\n\
             tulis_ptr(p, 99)?\n wok(isi_ptr(p)?)\n free(p)? }}\n",
            lib.display()
        );
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, "70\n990", "pointer C++ salah baca: {}", i.keluaran);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda untuk pointer C++");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ABI yang tidak didukung harus menunjukkan JALAN, bukan cuma menolak.
    /// Untuk Rust dan Go jawabannya bukan menunggu: keduanya sudah terjangkau
    /// lewat `extern "C"`.
    #[test]
    fn abi_tak_didukung_menunjukkan_jalan_yang_ada() {
        for (abi, kata) in [("Rust", "#[no_mangle]"), ("Go", "c-shared")] {
            let src = format!(
                "package main\nextern \"{}\" {{ fn f() }}\npub fn main() {{ wok(1) }}\n", abi);
            for vm in [false, true] {
                let penuh = jalankan_keluaran_penuh(&src, vm);
                assert!(
                    penuh.contains(kata),
                    "pesan ABI `{}` tidak menyebut `{}` di {}: {}",
                    abi, kata, if vm { "--vm" } else { "interp" }, penuh
                );
            }
        }
    }

    /// Sebuah `extern fn` yang namanya sama dengan builtin MENIMPA builtin itu —
    /// perilaku yang sudah lama ada dan sengaja dipertahankan: itulah cara …
    #[test]
    fn extern_fn_menimpa_builtin_bernama_sama() {
        let src = "package main\n\
                   extern \"C\" { fn abs(n: i32) -> i32 }\n\
                   pub fn main() { match abs(-17) { Ok(v) => wok(v), Err(e) => wok(e) } }\n";
        let i = jalankan(src, false);
        let v = jalankan(src, true);
        assert_eq!(i.keluaran, "17", "extern `abs` tidak dipakai: {}", i.keluaran);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda soal penimpaan builtin");
    }
}

/// FFI JVM: memanggil metode statis Java lewat JNI.
mod kotlin {
    use super::*;

    /// Berkas uji Kotlin: tiap bentuk mewakili satu konvensi penamaan yang
    /// BERBEDA dari Java, dan masing-masing menentukan cara memanggilnya.
    const SUMBER: &str = r#"
package uji
fun tambah(a: Int, b: Int): Int = a + b
fun sapa(nama: String): String = "Halo, " + nama
fun bagi(a: Double, b: Double): Double = a / b
object Kalkulator {
    fun kali(a: Int, b: Int): Int = a * b
    val versi: String = "1.0"
}
class Titik(val x: Double, val y: Double) {
    fun jarak(): Double = Math.sqrt(x * x + y * y)
    val jauh: Boolean = x * x + y * y > 100.0
    companion object {
        fun asal(): Titik = Titik(0.0, 0.0)
        const val DIMENSI: Int = 2
    }
}
"#;

    fn kotlinc() -> Option<std::path::PathBuf> {
        // `kotlinc` jarang ada di PATH; cari juga tempat pasang yang lazim.
        for k in ["kotlinc"] {
            if Command::new(k).arg("-version").output()
                .map(|o| o.status.success()).unwrap_or(false) {
                return Some(std::path::PathBuf::from(k));
            }
        }
        let rumah = std::env::var("HOME").unwrap_or_default();
        for c in [format!("{}/opt/kotlinc/bin/kotlinc", rumah),
                  "/usr/local/kotlinc/bin/kotlinc".to_string()] {
            let p = std::path::PathBuf::from(&c);
            if p.exists() { return Some(p); }
        }
        None
    }

    fn stdlib(kc: &std::path::Path) -> Option<std::path::PathBuf> {
        // `kotlin-stdlib.jar` duduk di `../lib` relatif ke binernya.
        let j = kc.parent()?.parent()?.join("lib").join("kotlin-stdlib.jar");
        if j.exists() { Some(j) } else { None }
    }

    /// Seluruh konvensi Kotlin dalam SATU program: sebuah lapisan nama bisa
    /// benar per bentuk dan salah begitu dirangkai (companion yang isinya
    /// objek, properti yang dibaca dari objek hasil companion).
    #[test]
    fn bentuk_kotlin_dipanggil_sama_di_dua_backend() {
        let Some(kc) = kotlinc() else {
            eprintln!("LEWAT: `kotlinc` tidak ada di mesin ini, FFI Kotlin tidak diuji");
            return;
        };
        let Some(jar) = stdlib(&kc) else {
            eprintln!("LEWAT: `kotlin-stdlib.jar` tidak ketemu di sebelah `kotlinc`");
            return;
        };
        if !Command::new("java").arg("-version").output()
            .map(|o| o.status.success()).unwrap_or(false) {
            eprintln!("LEWAT: `java` tidak ada");
            return;
        }
        let dir = crate::bersama::dir_unik("jrg-kotlin");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        std::fs::write(dir.join("Uji.kt"), SUMBER).expect("tulis kt");
        let out = Command::new(&kc).arg("Uji.kt").arg("-d").arg(".")
            .current_dir(&dir).output().expect("jalankan kotlinc");
        if !out.status.success() {
            eprintln!("LEWAT: kotlinc gagal: {}", String::from_utf8_lossy(&out.stderr));
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let cp = format!("{}:{}", dir.display(), jar.display());
        let src = format!(
            "package main\n\
             fn buka(r) {{ return r.unwrap_or(nil) }}\n\
             pub fn main() {{\n\
             wok(jvm::mulai([\"-Djava.class.path={}\"]))\n\
             wok(kotlin::fungsi(\"uji.Uji\", \"tambah\", \"(II)I\", [3, 4]))\n\
             wok(kotlin::fungsi(\"uji.Uji\", \"sapa\", \"(Ljava/lang/String;)Ljava/lang/String;\", [\"Sari\"]))\n\
             wok(kotlin::fungsi(\"uji.Uji\", \"bagi\", \"(DD)D\", [7.0, 2.0]))\n\
             let kalk = buka(kotlin::objek(\"uji.Kalkulator\"))\n\
             wok(jvm::panggil(kalk, \"kali\", \"(II)I\", [6, 7]))\n\
             wok(kotlin::properti(kalk, \"versi\", \"Ljava/lang/String;\"))\n\
             let t = buka(jvm::baru(\"uji.Titik\", \"(DD)V\", [3.0, 4.0]))\n\
             wok(kotlin::properti(t, \"x\", \"D\"))\n\
             wok(kotlin::properti(t, \"jauh\", \"Z\"))\n\
             wok(jvm::panggil(t, \"jarak\", \"()D\", []))\n\
             let comp = buka(kotlin::companion(\"uji.Titik\"))\n\
             let asal = buka(jvm::panggil(comp, \"asal\", \"()Luji/Titik;\", []))\n\
             wok(kotlin::properti(asal, \"x\", \"D\"))\n\
             wok(jvm::field_statis(\"uji.Titik\", \"DIMENSI\", \"I\"))\n\
             }}\n", cp);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        assert_eq!(i, v, "Kotlin harus sama di kedua backend");
        for harus in [
            "Ok(7)",          // fungsi top-level -> kelas `UjiKt`
            "Ok(Halo, Sari)", // String masuk DAN keluar (butuh kotlin-stdlib)
            "Ok(3.5)",        // Double
            "Ok(42)",         // metode pada `object`
            "Ok(1.0)",        // properti `val` lewat getter `getVersi`
            "Ok(3.0)",        // properti konstruktor lewat `getX`
            // Diukur: `val jauh: Boolean` jadi `getJauh()` — Kotlin TIDAK
            // mengikuti aturan `is…` JavaBeans untuk nama biasa.
            "Ok(false)",
            "Ok(5.0)",        // metode instance biasa
            "Ok(0.0)",        // companion object yang mengembalikan objek
            "Ok(2)",          // `const val` di companion jadi field statis
        ] {
            assert!(i.contains(harus), "hilang `{}` dari:\n{}", harus, i.trim());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Kegagalan classpath yang paling sering: `kotlin-stdlib.jar` lupa
    /// dipasang. Pesan JVM aslinya tidak menyebut jar itu sama sekali.
    #[test]
    fn stdlib_kotlin_hilang_dijelaskan_bukan_dibiarkan() {
        let Some(kc) = kotlinc() else {
            eprintln!("LEWAT: `kotlinc` tidak ada di mesin ini");
            return;
        };
        let dir = crate::bersama::dir_unik("jrg-kotlin-cp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        std::fs::write(dir.join("Uji.kt"), SUMBER).expect("tulis kt");
        let out = Command::new(&kc).arg("Uji.kt").arg("-d").arg(".")
            .current_dir(&dir).output().expect("kotlinc");
        if !out.status.success() { let _ = std::fs::remove_dir_all(&dir); return; }
        // Sengaja TANPA kotlin-stdlib.jar.
        let src = format!(
            "package main\n\
             pub fn main() {{\n\
             wok(jvm::mulai([\"-Djava.class.path={}\"]))\n\
             wok(kotlin::fungsi(\"uji.Uji\", \"sapa\", \"(Ljava/lang/String;)Ljava/lang/String;\", [\"x\"]))\n\
             }}\n", dir.display());
        let i = jalankan_keluaran_penuh(&src, false);
        assert!(i.contains("kotlin-stdlib.jar"),
            "galat classpath harus menyebut jar yang kurang, bukan berhenti di \
             nama kelas internal Kotlin:\n{}", i.trim());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod jvm {
    use super::*;

    fn ada(bin: &str) -> bool {
        Command::new(bin).arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    const KELAS: &str = r#"
public class UjiJrg {
    public static int tambah(int a, int b) { return a + b; }
    public static String sapa(String nama) { return "halo " + nama; }
    public static double bagi(double a, double b) { return a / b; }
    public static long besar(long x) { return x * 1000000L; }
    public static boolean genap(int n) { return n % 2 == 0; }
    public static void diam() { }
    public static int meledak() { throw new IllegalStateException("sengaja meledak"); }
    public static String kosong() { return null; }

    // Bentuk-bentuk untuk lapisan OBJEK. Padanan Kotlin-nya diuji terpisah;
    // yang diperiksa di sini mesin JNI-nya, yang dipakai bersama.
    public int nilai;
    public String label;
    public static final int BATAS = 42;
    public UjiJrg(int n, String l) { this.nilai = n; this.label = l; }
    public int kali(int k) { return nilai * k; }
    public String gabung(String s) { return label + s; }
    public double bagiDiri(double d) { return nilai / d; }
    public boolean genapDiri() { return nilai % 2 == 0; }
    public UjiJrg kembar() { return new UjiJrg(nilai, label); }
    public String toString() { return "UjiJrg(" + nilai + "," + label + ")"; }
}
"#;

    fn siapkan() -> Option<std::path::PathBuf> {
        if !ada("javac") || !ada("java") {
            eprintln!("LEWAT: `javac`/`java` tidak ada, FFI JVM tidak bisa diuji di mesin ini");
            return None;
        }
        let dir = crate::bersama::dir_unik("jrg-jvm");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        std::fs::write(dir.join("UjiJrg.java"), KELAS).expect("tulis java");
        let out = Command::new("javac").arg("UjiJrg.java").current_dir(&dir)
            .output().expect("jalankan javac");
        if !out.status.success() {
            eprintln!("LEWAT: javac gagal: {}", String::from_utf8_lossy(&out.stderr));
            return None;
        }
        Some(dir)
    }

    /// Objek Java yang dipegang JRG: konstruktor, metode instance, field,
    /// metode yang MENGEMBALIKAN objek, dan pelepasan handle. Satu program,
    /// karena sebuah driver bisa benar per panggilan dan salah begitu dirangkai.
    #[test]
    fn objek_java_dipegang_sama_di_dua_backend() {
        let Some(dir) = siapkan() else { return };
        let src = format!(
            "package main\n\
             fn buka(r) {{ return r.unwrap_or(nil) }}\n\
             pub fn main() {{\n\
             wok(jvm::mulai([\"-Djava.class.path={}\"]))\n\
             let o = buka(jvm::baru(\"UjiJrg\", \"(ILjava/lang/String;)V\", [7, \"abc\"]))\n\
             wok(jvm::panggil(o, \"kali\", \"(I)I\", [6]))\n\
             wok(jvm::panggil(o, \"gabung\", \"(Ljava/lang/String;)Ljava/lang/String;\", [\"-x\"]))\n\
             wok(jvm::panggil(o, \"bagiDiri\", \"(D)D\", [2.0]))\n\
             wok(jvm::panggil(o, \"genapDiri\", \"()Z\", []))\n\
             wok(jvm::ke_teks(o))\n\
             wok(jvm::field(o, \"nilai\", \"I\"))\n\
             wok(jvm::field(o, \"label\", \"Ljava/lang/String;\"))\n\
             wok(jvm::field_statis(\"UjiJrg\", \"BATAS\", \"I\"))\n\
             let k = buka(jvm::panggil(o, \"kembar\", \"()LUjiJrg;\", []))\n\
             wok(jvm::ke_teks(k))\n\
             wok(jvm::objek_hidup())\n\
             wok(jvm::lepas(o))\n\
             wok(jvm::objek_hidup())\n\
             wok(jvm::panggil(o, \"kali\", \"(I)I\", [2]))\n\
             wok(jvm::panggil_statis(\"java.lang.String\", \"valueOf\", \
                 \"(Ljava/lang/Object;)Ljava/lang/String;\", [k]))\n\
             wok(jvm::panggil_statis(\"java.lang.String\", \"join\", \
                 \"(Ljava/lang/CharSequence;[Ljava/lang/CharSequence;)Ljava/lang/String;\", \
                 [\"-\", [\"a\", \"b\", \"c\"]]))\n\
             wok(jvm::panggil_statis(\"java.util.Objects\", \"toString\", \
                 \"(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;\", [nil, \"kosong\"]))\n\
             }}\n",
            dir.display());
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        assert_eq!(i, v, "objek Java harus sama di kedua backend");
        let a = i;
        // Nilainya diperiksa, bukan cuma kesamaannya: dua backend yang sama-sama
        // salah juga "identik".
        assert!(a.contains("Ok(42)"), "kali(6) atas nilai 7 harus 42 — dapat:\n{}", a);
        assert!(a.contains("Ok(abc-x)"), "gabung harus `abc-x` — dapat:\n{}", a);
        assert!(a.contains("Ok(3.5)"), "7/2.0 harus 3.5 — dapat:\n{}", a);
        assert!(a.contains("Ok(false)"), "7 itu ganjil — dapat:\n{}", a);
        assert!(a.contains("UjiJrg(7,abc)"), "toString harus lewat — dapat:\n{}", a);
        // Handle yang sudah dilepas harus DITOLAK, bukan dipakai lagi.
        assert!(a.contains("sudah dilepas"),
                "handle yang dilepas harus ditolak dengan sebabnya — dapat:\n{}", a);
        // Objek sebagai ARGUMEN, array objek, dan `nil` sebagai `null` Java.
        // Tanpa ketiganya sebagian besar API Java tidak terjangkau: sebuah objek
        // bisa dibuat tapi tidak bisa dioper ke metode berikutnya.
        assert!(a.contains("UjiJrg(7,abc)"), "objek sebagai argumen gagal:\n{}", a);
        assert!(a.contains("Ok(a-b-c)"), "array objek sebagai argumen gagal:\n{}", a);
        assert!(a.contains("Ok(kosong)"), "`nil` harus jadi `null` Java:\n{}", a);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nilai, tipe, dan KEGAGALAN, semuanya di satu program — sebuah driver
    /// bisa benar per panggilan dan salah begitu dirangkai (JVM yang sudah
    /// jalan, exception yang tertinggal menggantung ke panggilan berikutnya).
    #[test]
    fn metode_statis_java_dipanggil_dari_dua_backend() {
        let Some(dir) = siapkan() else { return };
        let src = format!(
            "package main\n\
             pub fn main() {{\n\
             wok(jvm::mulai([\"-Djava.class.path={}\"]))\n\
             wok(jvm::panggil_statis(\"java.lang.Math\", \"abs\", \"(I)I\", [-17]))\n\
             wok(jvm::panggil_statis(\"java.lang.Math\", \"sqrt\", \"(D)D\", [16.0]))\n\
             wok(jvm::panggil_statis(\"java.lang.String\", \"valueOf\", \"(I)Ljava/lang/String;\", [42]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"tambah\", \"(II)I\", [2, 3]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"sapa\", \"(Ljava/lang/String;)Ljava/lang/String;\", [\"dunia\"]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"bagi\", \"(DD)D\", [7.0, 2.0]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"besar\", \"(J)J\", [12345]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"genap\", \"(I)Z\", [4]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"diam\", \"()V\", []))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"kosong\", \"()Ljava/lang/String;\", []))\n\
             }}\n",
            dir.display()
        );
        let i = jalankan(&src, false);
        if i.keluaran.contains("libjvm tidak ketemu") {
            eprintln!("LEWAT: JDK ada tapi libjvm tidak ketemu — {}", i.keluaran);
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda untuk FFI JVM");
        for harus in [
            "Result::Ok(())",          // jvm::mulai, dan `void`
            "Result::Ok(17)",          // Math.abs
            "Result::Ok(4.0)",         // Math.sqrt
            "Result::Ok(42)",          // String.valueOf -> "42"
            "Result::Ok(5)",           // UjiJrg.tambah
            "Result::Ok(halo dunia)",  // String masuk DAN keluar
            "Result::Ok(3.5)",         // double
            "Result::Ok(12345000000)", // long, di luar jangkauan i32
            "Result::Ok(true)",        // boolean
            // `null` Java jadi `nil`, bukan string kosong — kalau ia jadi
            // `""`, "tidak ada" menyamar jadi "kosong".
            "Result::Ok(nil)",
        ] {
            assert!(i.keluaran.contains(harus), "hilang `{}` dari:\n{}", harus, i.keluaran);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tiga kegagalan yang berbeda harus terbaca BERBEDA. "Gagal" saja tidak
    /// memberi tahu apakah yang salah classpath-nya, tanda tangannya, atau
    /// kode Java-nya sendiri.
    #[test]
    fn kegagalan_java_menyebut_sebabnya_masing_masing() {
        let Some(dir) = siapkan() else { return };
        let src = format!(
            "package main\n\
             pub fn main() {{\n\
             wok(jvm::mulai([\"-Djava.class.path={}\"]))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"meledak\", \"()I\", []))\n\
             wok(jvm::panggil_statis(\"UjiJrg\", \"tidakAda\", \"()I\", []))\n\
             wok(jvm::panggil_statis(\"Hantu\", \"x\", \"()I\", []))\n\
             wok(jvm::mulai([]))\n\
             }}\n",
            dir.display()
        );
        let i = jalankan(&src, false);
        if i.keluaran.contains("libjvm tidak ketemu") {
            eprintln!("LEWAT: JDK ada tapi libjvm tidak ketemu");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, v.keluaran, "pesan kegagalan JVM beda antar-backend");
        for harus in [
            "IllegalStateException: sengaja meledak",  // exception Java, utuh
            "javap -s",                                // tanda tangan meleset
            "NoClassDefFoundError",                    // kelas tidak ada
            "JNI_CreateJavaVM",                        // mulai kedua kali
        ] {
            assert!(i.keluaran.contains(harus), "hilang `{}` dari:\n{}", harus, i.keluaran);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mesin tanpa JDK harus mendapat kalimat yang menyebut PAKETNYA, bukan
    /// kegagalan yang tidak menjelaskan apa pun. Ini yang membedakan
    /// "belum dipasang" dari "rusak".
    #[test]
    fn tanpa_jdk_menyebut_nama_paketnya() {
        let pesan = jrgc::ffi::jvm::alasan_tidak_ada();
        for kata in ["apt", "dnf", "pacman", "pkg", "JAVA_HOME"] {
            assert!(pesan.contains(kata), "pesan tanpa-JDK tidak menyebut `{}`: {}", kata, pesan);
        }
    }
}

/// WebAssembly: menjalankan modul `.wasm` dengan penerjemah milik proyek ini.
mod wasm {
    use super::*;

    fn ada(bin: &str, arg: &str) -> bool {
        Command::new(bin).arg(arg).output().map(|o| o.status.success()).unwrap_or(false)
    }

    /// Sengaja memuat yang paling mudah salah: `br_table` dengan default,
    /// `nearest` yang membulatkan ke GENAP, `min`/`max` yang menular NaN,
    /// operasi tak bertanda, konversi, dan blok bersarang.
    const WAT: &str = r#"
(module
  (memory (export "mem") 1 4)
  (global $hitung (mut i32) (i32.const 0))
  (func $fib (export "fib") (param i32) (result i32)
    (if (result i32) (i32.lt_s (local.get 0) (i32.const 2))
      (then (local.get 0))
      (else (i32.add (call $fib (i32.sub (local.get 0) (i32.const 1)))
                     (call $fib (i32.sub (local.get 0) (i32.const 2)))))))
  (func (export "tambah") (param i32 i32) (result i32)
    (i32.add (local.get 0) (local.get 1)))
  (func (export "kali_f64") (param f64 f64) (result f64)
    (f64.mul (local.get 0) (local.get 1)))
  (func (export "naik") (result i32)
    (global.set $hitung (i32.add (global.get $hitung) (i32.const 1)))
    (global.get $hitung))
  (func (export "simpan_baca") (param i32 i32) (result i32)
    (i32.store (local.get 0) (local.get 1))
    (i32.load (local.get 0)))
  (func (export "jumlah_loop") (param i32) (result i64)
    (local $i i32) (local $s i64)
    (block $keluar (loop $ulang
      (br_if $keluar (i32.ge_s (local.get $i) (local.get 0)))
      (local.set $s (i64.add (local.get $s) (i64.extend_i32_s (local.get $i))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $ulang)))
    (local.get $s))
  (func (export "tabel") (param i32) (result i32)
    (block $d (block $c (block $b (block $a
      (br_table $a $b $c $d (local.get 0))))
      (return (i32.const 200)))
      (return (i32.const 300)))
    (i32.const 999))
  (func (export "pilih") (param i32) (result i32)
    (select (i32.const 11) (i32.const 22) (local.get 0)))
  (func (export "dekat") (param f64) (result f64) (f64.nearest (local.get 0)))
  (func (export "bagi_u") (param i32 i32) (result i32) (i32.div_u (local.get 0) (local.get 1)))
  (func (export "geser") (param i32 i32) (result i32) (i32.shr_u (local.get 0) (local.get 1)))
  (func (export "putar") (param i32 i32) (result i32) (i32.rotl (local.get 0) (local.get 1)))
  (func (export "wrap") (param i64) (result i32) (i32.wrap_i64 (local.get 0)))
  (func (export "extend_u") (param i32) (result i64) (i64.extend_i32_u (local.get 0)))
  (func (export "trunc") (param f64) (result i32) (i32.trunc_f64_s (local.get 0)))
  (func (export "reinterp") (param f64) (result i64) (i64.reinterpret_f64 (local.get 0)))
  (func (export "ext8") (param i32) (result i32) (i32.extend8_s (local.get 0)))
  (func (export "tumbuh") (result i32) (drop (memory.grow (i32.const 1))) (memory.size))
  (func (export "clz") (param i32) (result i32) (i32.clz (local.get 0)))
  (func (export "popcnt") (param i64) (result i64) (i64.popcnt (local.get 0)))
  (func (export "f32ops") (param f32 f32) (result f32)
    (f32.copysign (f32.sqrt (local.get 0)) (local.get 1)))
  (func (export "bersarang") (param i32) (result i32)
    (local $s i32)
    (block $luar (loop $ulang
      (br_if $luar (i32.ge_s (local.get 0) (i32.const 10)))
      (if (i32.eq (i32.rem_s (local.get 0) (i32.const 2)) (i32.const 0))
        (then (local.set $s (i32.add (local.get $s) (local.get 0))))
        (else (local.set $s (i32.sub (local.get $s) (i32.const 1)))))
      (local.set 0 (i32.add (local.get 0) (i32.const 1)))
      (br $ulang)))
    (local.get $s))
)
"#;

    /// Panggilan yang diuji. Ditulis sekali dan dipakai baik oleh JRG maupun
    /// oleh Node, supaya keduanya benar-benar menjawab pertanyaan yang sama.
    const KASUS: &[(&str, &str, &str)] = &[
        // (nama, args JRG, args JS)
        ("tambah", "[2, 3]", "2,3"),
        ("fib", "[20]", "20"),
        ("kali_f64", "[1.5, 4.0]", "1.5,4.0"),
        ("naik", "[]", ""),
        ("naik", "[]", ""),
        ("simpan_baca", "[64, 12345]", "64,12345"),
        ("jumlah_loop", "[100]", "100"),
        ("tabel", "[0]", "0"),
        ("tabel", "[2]", "2"),
        ("tabel", "[5]", "5"),
        ("pilih", "[0]", "0"),
        ("pilih", "[1]", "1"),
        ("dekat", "[2.5]", "2.5"),
        ("dekat", "[3.5]", "3.5"),
        ("dekat", "[-2.5]", "-2.5"),
        ("bagi_u", "[-1, 2]", "-1,2"),
        ("geser", "[-8, 1]", "-8,1"),
        ("putar", "[1, 31]", "1,31"),
        ("wrap", "[4294967298]", "4294967298n"),
        ("extend_u", "[-1]", "-1"),
        ("trunc", "[3.9]", "3.9"),
        ("trunc", "[-3.9]", "-3.9"),
        ("reinterp", "[1.5]", "1.5"),
        ("ext8", "[255]", "255"),
        ("tumbuh", "[]", ""),
        ("clz", "[1]", "1"),
        ("popcnt", "[255]", "255n"),
        ("f32ops", "[9.0, -1.0]", "9.0,-1.0"),
        ("bersarang", "[0]", "0"),
    ];

    fn siapkan() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        if !ada("wat2wasm", "--version") {
            eprintln!("LEWAT: `wat2wasm` (WABT) tidak ada, modul uji wasm tidak bisa dibuat");
            return None;
        }
        let dir = crate::bersama::dir_unik("jrg-wasm");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        let wat = dir.join("u.wat");
        std::fs::write(&wat, WAT).expect("tulis wat");
        let wasm = dir.join("u.wasm");
        let out = Command::new("wat2wasm").arg(&wat).arg("-o").arg(&wasm)
            .output().expect("jalankan wat2wasm");
        if !out.status.success() {
            eprintln!("LEWAT: wat2wasm gagal: {}", String::from_utf8_lossy(&out.stderr));
            return None;
        }
        Some((dir, wasm))
    }

    fn program_jrg(wasm: &std::path::Path) -> String {
        let mut s = format!(
            "package main\npub fn main() {{\nlet h = wasm::muat(\"{}\")?\n", wasm.display());
        for (n, a, _) in KASUS {
            s.push_str(&format!("wok(wasm::panggil(h, \"{}\", {}))\n", n, a));
        }
        s.push_str("}\n");
        s
    }

    #[test]
    fn modul_wasm_dijalankan_sama_di_dua_backend() {
        let Some((dir, wasm)) = siapkan() else { return };
        let src = program_jrg(&wasm);
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert!(i.sukses, "interpreter gagal menjalankan wasm: {}", i.keluaran);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda untuk wasm");
        // `fib(20)` menuntut rekursi, `if/else`, dan aritmetika sekaligus —
        // kalau salah satunya rusak, angkanya tidak akan 6765.
        assert!(i.keluaran.contains("Result::Ok(6765)"), "fib(20) salah:\n{}", i.keluaran);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Oracle independen: WebAssembly milik Node menjawab pertanyaan yang
    /// SAMA, dan tiap jawabannya dibandingkan satu per satu.
    #[test]
    fn jawaban_cocok_dengan_webassembly_node() {
        let Some((dir, wasm)) = siapkan() else { return };
        if !ada("node", "--version") {
            eprintln!("LEWAT: `node` tidak ada, jawaban wasm tidak bisa diadu dengan \
                       implementasi lain");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let mut js = format!(
            "const fs=require('fs');\n\
             WebAssembly.instantiate(fs.readFileSync('{}'),{{}}).then(({{instance}})=>{{\n\
             const e=instance.exports;\n", wasm.display());
        for (n, _, a) in KASUS {
            js.push_str(&format!("console.log(String(e.{}({})));\n", n, a));
        }
        js.push_str("}).catch(x=>{console.error(x.message);process.exit(1)});\n");
        let jsp = dir.join("o.js");
        std::fs::write(&jsp, js).expect("tulis js");
        let out = Command::new("node").arg(&jsp).output().expect("jalankan node");
        assert!(out.status.success(), "node gagal: {}", String::from_utf8_lossy(&out.stderr));
        let dari_node: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines().map(|l| l.trim().to_string()).collect();

        let src = program_jrg(&wasm);
        let dari_jrg: Vec<String> = jalankan(&src, false).keluaran
            .lines()
            .map(|l| l.trim().trim_start_matches("Result::Ok(").trim_end_matches(')').to_string())
            .collect();
        assert_eq!(dari_jrg.len(), dari_node.len(),
                   "jumlah jawaban beda: jrg={:?} node={:?}", dari_jrg, dari_node);

        let mut beda = Vec::new();
        for (k, ((nama, args, _), (j, n))) in
            KASUS.iter().zip(dari_jrg.iter().zip(&dari_node)).enumerate()
        {
            // JRG mencetak float dengan `.0`, JS tidak — bandingkan sebagai
            // angka kalau keduanya bisa dibaca sebagai angka.
            let sama = j == n
                || match (j.parse::<f64>(), n.parse::<f64>()) {
                    (Ok(a), Ok(b)) => a == b || (a.is_nan() && b.is_nan()),
                    _ => false,
                };
            if !sama {
                beda.push(format!("#{} {}({}) jrg={} node={}", k, nama, args, j, n));
            }
        }
        assert!(beda.is_empty(), "jawaban wasm beda dari Node:\n  {}", beda.join("\n  "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Memori modul: teks yang bukan UTF-8 ditolak dan menyebut pintu byte-nya.
    #[test]
    fn memori_wasm_dibaca_sebagai_teks_atau_byte() {
        let Some((dir, wasm)) = siapkan() else { return };
        let src = format!(
            "package main\npub fn main() {{\n\
             let h = wasm::muat(\"{}\")?\n\
             wok(wasm::tulis_memori(h, 0, [104, 105, 255]))\n\
             wok(wasm::baca_memori(h, 0, 2))\n\
             wok(wasm::baca_memori(h, 0, 3))\n\
             wok(wasm::baca_memori_bytes(h, 0, 3))\n }}\n",
            wasm.display());
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, v.keluaran, "memori wasm beda antar-backend");
        assert!(i.keluaran.contains("Result::Ok(hi)") && i.keluaran.contains("wasm::baca_memori_bytes")
                && i.keluaran.contains("Result::Ok([104, 105, 255])"), "memori wasm salah:\n{}", i.keluaran);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Trap wasm harus DILAPORKAN, bukan dijawab angka yang masuk akal.
    #[test]
    fn trap_wasm_dilaporkan_bukan_dijawab_angka() {
        let Some((dir, wasm)) = siapkan() else { return };
        let src = format!(
            "package main\npub fn main() {{\n\
             let h = wasm::muat(\"{}\")?\n\
             wok(wasm::panggil(h, \"bagi_u\", [1, 0]))\n\
             wok(wasm::panggil(h, \"trunc\", [1e30]))\n\
             wok(wasm::panggil(h, \"hantu\", []))\n\
             wok(wasm::panggil(h, \"tambah\", [1]))\n\
             wok(wasm::tutup(h))\n\
             wok(wasm::panggil(h, \"tambah\", [2, 3]))\n }}\n",
            wasm.display());
        let i = jalankan(&src, false);
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, v.keluaran, "pesan trap wasm beda antar-backend");
        for harus in [
            "pembagian integer dengan nol",
            "di luar jangkauan",
            "tidak mengekspor fungsi `hantu`",
            "minta 2 argumen, diberi 1",
            "sudah ditutup",
        ] {
            assert!(i.keluaran.contains(harus), "hilang `{}` dari:\n{}", harus, i.keluaran);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Yang belum didukung DITOLAK dengan menyebut apa yang tidak didukung.
    /// Sebuah modul dengan impor yang dilewati diam-diam akan menggeser
    /// indeks tiap fungsinya — tiap `call` sesudahnya memanggil fungsi lain.
    #[test]
    fn yang_belum_didukung_ditolak_dengan_sebabnya() {
        let Some((dir, _)) = siapkan() else { return };
        let wat = dir.join("impor.wat");
        std::fs::write(&wat,
            "(module (import \"env\" \"log\" (func $log (param i32)))\n\
             (func (export \"f\") (call $log (i32.const 1))))\n").expect("tulis");
        let wasm = dir.join("impor.wasm");
        let out = Command::new("wat2wasm").arg(&wat).arg("-o").arg(&wasm).output().expect("wat2wasm");
        assert!(out.status.success(), "wat2wasm gagal: {}", String::from_utf8_lossy(&out.stderr));
        let src = format!(
            "package main\npub fn main() {{ wok(wasm::muat(\"{}\")) }}\n", wasm.display());
        for vm in [false, true] {
            let k = jalankan(&src, vm).keluaran;
            assert!(k.contains("impor belum didukung") && k.contains("env.log"),
                    "impor tidak ditolak dengan sebabnya di {}: {}",
                    if vm { "--vm" } else { "interp" }, k);
        }
        // Berkas yang bukan wasm sama sekali.
        let bukan = dir.join("bukan.wasm");
        std::fs::write(&bukan, "ini teks biasa").expect("tulis");
        let src2 = format!(
            "package main\npub fn main() {{ wok(wasm::muat(\"{}\")) }}\n", bukan.display());
        assert!(jalankan(&src2, false).keluaran.contains("bukan modul wasm"),
                "berkas bukan-wasm tidak ditolak dengan jelas");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// FFI .NET: memanggil metode `[UnmanagedCallersOnly]` lewat hosting API.
mod dotnet {
    use super::*;

    fn akar_dotnet() -> Option<std::path::PathBuf> {
        for a in [std::env::var("DOTNET_ROOT").unwrap_or_default(),
                  "/usr/share/dotnet".to_string(), "/usr/lib/dotnet".to_string()] {
            if a.is_empty() { continue; }
            let p = std::path::PathBuf::from(a);
            if p.join("host/fxr").exists() { return Some(p); }
        }
        None
    }

    fn cari_satu(dir: &std::path::Path) -> Option<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(dir).ok()?
            .filter_map(|e| e.ok()).map(|e| e.path()).collect();
        v.sort();
        v.pop()
    }

    const KELAS: &str = r#"
using System.Runtime.InteropServices;
public static class UjiJrg
{
    [UnmanagedCallersOnly] public static int Tambah(int a, int b) => a + b;
    [UnmanagedCallersOnly] public static double Bagi(double a, double b) => a / b;
    [UnmanagedCallersOnly] public static long Besar(long x) => x * 1000000L;
}
"#;

    const CONFIG: &str = r#"{
  "runtimeOptions": {
    "tfm": "net6.0",
    "framework": { "name": "Microsoft.NETCore.App", "version": "6.0.0" }
  }
}"#;

    fn siapkan() -> Option<std::path::PathBuf> {
        let Some(akar) = akar_dotnet() else {
            eprintln!("LEWAT: runtime .NET tidak ada di mesin ini");
            return None;
        };
        let Some(sdk) = cari_satu(&akar.join("sdk")) else {
            eprintln!("LEWAT: runtime .NET ada tapi SDK-nya tidak — assembly uji \
                       tidak bisa dikompilasi");
            return None;
        };
        let csc = sdk.join("Roslyn/bincore/csc.dll");
        let Some(fw) = cari_satu(&akar.join("shared/Microsoft.NETCore.App")) else {
            eprintln!("LEWAT: framework .NET tidak ketemu");
            return None;
        };
        if !csc.exists() {
            eprintln!("LEWAT: `csc.dll` tidak ada di SDK ini");
            return None;
        }
        let dir = crate::bersama::dir_unik("jrg-net");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bikin dir");
        std::fs::write(dir.join("Uji.cs"), KELAS).expect("tulis cs");
        std::fs::write(dir.join("UjiJrg.runtimeconfig.json"), CONFIG).expect("tulis config");
        let out = Command::new(akar.join("dotnet"))
            .arg("exec").arg(&csc)
            .args(["-nologo", "-target:library", "-out:UjiJrg.dll"])
            .arg(format!("-r:{}", fw.join("System.Runtime.dll").display()))
            .arg(format!("-r:{}", fw.join("System.Private.CoreLib.dll").display()))
            .arg(format!("-r:{}", fw.join("System.Runtime.InteropServices.dll").display()))
            .arg("Uji.cs")
            .current_dir(&dir)
            .output().expect("jalankan csc");
        if !out.status.success() || !dir.join("UjiJrg.dll").exists() {
            eprintln!("LEWAT: csc gagal: {}{}",
                      String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        Some(dir)
    }

    /// Nilai, tipe, dan kegagalan dalam SATU program: sebuah host bisa benar
    /// per panggilan dan salah begitu dirangkai — runtime yang sudah menyala,
    /// handle yang sudah dilepas.
    #[test]
    fn metode_dotnet_dipanggil_dari_dua_backend() {
        let Some(dir) = siapkan() else { return };
        let d = dir.display();
        let src = format!(
            "package main\npub fn main() {{\n\
             wok(dotnet::mulai(\"{d}/UjiJrg.runtimeconfig.json\"))\n\
             let f = dotnet::fungsi(\"{d}/UjiJrg.dll\", \"UjiJrg, UjiJrg\", \"Tambah\", \
                [\"i32\",\"i32\"], \"i32\")?\n\
             wok(dotnet::panggil(f, [2, 3]))\n\
             wok(dotnet::panggil(f, [40, 2]))\n\
             let g = dotnet::fungsi(\"{d}/UjiJrg.dll\", \"UjiJrg, UjiJrg\", \"Bagi\", \
                [\"f64\",\"f64\"], \"f64\")?\n\
             wok(dotnet::panggil(g, [7.0, 2.0]))\n\
             let b = dotnet::fungsi(\"{d}/UjiJrg.dll\", \"UjiJrg, UjiJrg\", \"Besar\", \
                [\"i64\"], \"i64\")?\n\
             wok(dotnet::panggil(b, [12345]))\n\
             wok(dotnet::fungsi(\"{d}/UjiJrg.dll\", \"UjiJrg, UjiJrg\", \"Hantu\", [], \"i32\"))\n\
             wok(dotnet::fungsi(\"{d}/tidakada.dll\", \"X, X\", \"Y\", [], \"i32\"))\n\
             wok(dotnet::lepas(f))\n\
             wok(dotnet::panggil(f, [1, 2]))\n }}\n", d = d);
        let i = jalankan(&src, false);
        if i.keluaran.contains("libhostfxr tidak ketemu") {
            eprintln!("LEWAT: SDK ada tapi libhostfxr tidak — {}", i.keluaran);
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let v = jalankan(&src, true);
        assert_eq!(i.keluaran, v.keluaran, "interpreter dan VM beda untuk FFI .NET");
        for harus in [
            "Result::Ok(5)", "Result::Ok(42)",
            "Result::Ok(3.5)",              // double masuk dan keluar
            "Result::Ok(12345000000)",      // long, di luar jangkauan i32
            "UnmanagedCallersOnly",         // metode tidak ketemu: sebabnya disebut
            "tidak ada",                    // assembly tidak ada
            "sudah dilepas",                // handle dipakai sesudah dilepas
        ] {
            assert!(i.keluaran.contains(harus), "hilang `{}` dari:\n{}", harus, i.keluaran);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mesin tanpa .NET harus mendapat kalimat yang menyebut PAKETNYA.
    #[test]
    fn tanpa_dotnet_menyebut_nama_paketnya() {
        let pesan = jrgc::ffi::dotnet::alasan_tidak_ada();
        for kata in ["apt", "dnf", "pacman", "DOTNET_ROOT"] {
            assert!(pesan.contains(kata), "pesan tanpa-.NET tidak menyebut `{}`: {}", kata, pesan);
        }
    }

    /// Nama tipe yang tidak dikenal ditolak dengan menyebut yang dikenal —
    /// dan kosakatanya SAMA dengan kosakata anotasi bahasa ini, karena
    /// keduanya membaca `prim_dari_nama` yang sama.
    #[test]
    fn tipe_tak_dikenal_ditolak_dengan_menyebut_yang_ada() {
        let src = "package main\npub fn main() {\n\
                   wok(dotnet::fungsi(\"a.dll\", \"T, A\", \"M\", [\"hantu\"], \"i32\")) }\n";
        for vm in [false, true] {
            let k = jalankan(src, vm).keluaran;
            assert!(k.contains("tipe `hantu` tidak dikenal"),
                    "tipe asing tidak ditolak dengan jelas di {}: {}",
                    if vm { "--vm" } else { "interp" }, k);
        }
    }
}

// ─── Jejak pemanggilan ───────────────────────────────────────────────────

/// Program uji untuk sapuan jejak: (nama kasus, sumber, potongan wajib).
const JEJAK: &[(&str, &str, &str)] = &[
    ("rantai tiga fungsi bernama",
     "package main\n\
      fn dalam(x) { return x / 0 }\n\
      fn tengah(x) { return dalam(x) }\n\
      fn luar(x) { return tengah(x) }\n\
      pub fn main() { wok(luar(5)) }\n",
     "`tengah` dipanggil di"),

    ("method struct memakai nama berkualifikasi",
     "package main\n\
      struct P { pub n: int }\n\
      impl P { pub fn pecah(self) { return self.n / 0 } }\n\
      pub fn main() { let p = P{n:1}; wok(p.pecah()) }\n",
     "`P::pecah` dipanggil di"),

    ("closure lewat HOF dapat situs panggil pemakai",
     "package main\npub fn main() { wok(arr::map([1,2], |x| x / 0)) }\n",
     "`<closure>` dipanggil di"),

    // Kasus yang melahirkan fase ini: galatnya mendarat di `std/functional`,
    // sebuah berkas yang pembacanya tidak pernah buka. Tanpa jejak, satu-
    // satunya lokasi yang tercetak ada di berkas orang lain.
    ("gagal di dalam modul std menyebut baris milik pemakai",
     "package main\n\
      import \"std/functional\"\n\
      pub fn main() { let f = functional::compose(5, 5); wok(f(1)) }\n",
     "uji.jrg:3:"),

    ("builtin yang menolak argumennya tidak menyumbang bingkai",
     "package main\npub fn main() { wok(str::len(5)) }\n",
     ""),

    // Rekursi 10.000 dalam akan mencetak 10.000 baris dan mengubur pesannya
    // sendiri.
    ("rekursi dalam dipotong dan sisanya dihitung benar",
     "package main\n\
      fn t(n) { if n == 0 { return 1 / 0 } return t(n - 1) }\n\
      pub fn main() { wok(t(30)) }\n",
     "… dan 21 panggilan lagi"),

    ("inisialisasi tingkat modul punya jejak",
     "package main\n\
      fn pecah(x) { return x / 0 }\n\
      let A = pecah(1)\n\
      pub fn main() { wok(A) }\n",
     "`pecah` dipanggil di"),

    // Argumen KURANG milik situs panggil, seperti argumen LEBIH: acuan dulu menambah
    // bingkai `g` karena memeriksanya sesudah bingkainya dibuka (Fase FUNGSI-DATA).
    ("argumen kurang tidak menyumbang bingkai fungsinya",
     "package main\n\
      fn g(a, b) { return a }\n\
      fn luar() { return g(1) }\n\
      pub fn main() { wok(luar()) }\n",
     "`luar` dipanggil di"),

    ("closure kebanyakan argumen",
     "package main\npub fn main() { let f = |a| a\n wok(f(1, 2)) }\n",
     ""),
];

/// Jejak pemanggilan harus **sama persis** di kedua backend.
#[test]
fn jejak_panggilan_sama_di_dua_backend() {
    let dir = std::env::temp_dir().join(format!("jrg-jejak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("bikin dir");
    let file = dir.join("uji.jrg");

    let mut gagal: Vec<String> = Vec::new();
    for (nama, src, wajib) in JEJAK {
        std::fs::write(&file, src).expect("tulis sumber");
        let ambil = |vm: bool| -> (String, bool) {
            let mut cmd = Command::new(jrgc());
            cmd.arg(&file).arg("--sopan");
            if vm { cmd.arg("--vm"); }
            let out = jalankan_berbatas(cmd);
            let teks = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let jejak = match teks.find("= jejak:") {
                Some(i) => teks[i..].trim_end().to_string(),
                None => String::new(),
            };
            (jejak, out.status_sukses)
        };
        let (ji, sukses_i) = ambil(false);
        let (jv, sukses_v) = ambil(true);

        // Program yang SUKSES akan lolos pemeriksaan "jejak kosong" tanpa
        // membuktikan apa pun — jadi kegagalannya dipastikan lebih dulu.
        if sukses_i || sukses_v {
            gagal.push(format!("{nama}: harusnya gagal, tapi sukses (i={sukses_i} vm={sukses_v})"));
            continue;
        }
        if ji != jv {
            gagal.push(format!("{nama}: jejak beda\n  interp:\n{ji}\n  vm:\n{jv}"));
            continue;
        }
        if wajib.is_empty() {
            if !ji.is_empty() {
                gagal.push(format!("{nama}: harusnya TANPA jejak, dapat:\n{ji}"));
            }
        } else if !ji.contains(wajib) {
            gagal.push(format!("{nama}: jejak tidak memuat `{wajib}`, dapat:\n{ji}"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(gagal.is_empty(), "jejak pemanggilan:\n{}", gagal.join("\n"));
}

/// Galat dari inisialisasi tingkat modul harus punya LOKASI di kedua backend.
#[test]
fn galat_inisialisasi_modul_punya_lokasi() {
    let src = "package main\n\
               fn pecah(x) { return x / 0 }\n\
               let A = pecah(1)\n\
               pub fn main() { wok(A) }\n";
    for vm in [false, true] {
        let teks = jalankan_keluaran_penuh(src, vm);
        let sisi = if vm { "--vm" } else { "interpreter" };
        assert!(teks.contains("-->"), "{sisi}: galat tanpa lokasi:\n{teks}");
        assert!(teks.contains('^'), "{sisi}: galat tanpa caret:\n{teks}");
        assert!(
            !teks.contains("error: runtime error:"),
            "{sisi}: awalan `runtime error:` tercetak dua kali:\n{teks}"
        );
    }
}

/// Laporkan ukuran tiap tabel diferensial — supaya angkanya DITURUNKAN, bukan
/// diingat.
#[test]
fn pencarian_menjawab_option_bukan_sentinel() {
    let kasus: &[(&str, &str, &str)] = &[
        // (ekspresi TIDAK ketemu, ekspresi KETEMU, jawaban untuk yang ketemu)
        ("arr::index_of([1,2], 9)",            "arr::index_of([1,2], 2)",            "Option::Some(1)"),
        ("arr::find_idx([1,2], |x| x > 9)",    "arr::find_idx([1,2], |x| x > 1)",    "Option::Some(1)"),
        ("arr::find([1,2], |x| x > 9)",        "arr::find([1,2], |x| x > 1)",        "Option::Some(2)"),
        ("[1,2].index_of(9)",                  "[1,2].index_of(2)",                  "Option::Some(1)"),
        ("[1,2].find_idx(|x| x > 9)",          "[1,2].find_idx(|x| x > 1)",          "Option::Some(1)"),
        ("[1,2].find(|x| x > 9)",              "[1,2].find(|x| x > 1)",              "Option::Some(2)"),
        ("[1,2,3].binary_search(9)",           "[1,2,3].binary_search(2)",           "Option::Some(1)"),
        ("str::find(\"abc\", \"z\")",           "str::find(\"abc\", \"b\")",           "Option::Some(1)"),
        ("str::index_of(\"abc\", \"z\")",       "str::index_of(\"abc\", \"b\")",       "Option::Some(1)"),
        ("\"abc\".find(\"z\")",                 "\"abc\".find(\"b\")",                 "Option::Some(1)"),
        ("\"abc\".index_of(\"z\")",             "\"abc\".index_of(\"b\")",             "Option::Some(1)"),
        // `re::find` adalah pintu KETIGA untuk kata kerja yang sama, dan
        // dulu ia punya kontrak ketiga: dict waktu ketemu, `nil` waktu tidak.
        ("re::find(\"z\", \"abc\")",             "re::find(\"b\", \"abc\")",
         "Option::Some(dict {end: 2, match: b, start: 1})"),
    ];
    for (miss, hit, mau) in kasus {
        for (e, harap) in [(miss, "Option::None"), (hit, *mau)] {
            let src = format!("package main\npub fn main() {{ wok({}) }}", e);
            let i = jalankan(&src, false);
            let v = jalankan(&src, true);
            assert_eq!(i.keluaran, v.keluaran, "`{}` beda antar-backend", e);
            assert_eq!(i.keluaran.trim(), harap,
                "`{}` harusnya `{}` — `-1` di sini adalah indeks yang SAH, jadi \
                 sentinel membuat `a[...]` membaca elemen terakhir", e, harap);
        }
    }
    // Jebakannya sendiri, dikunci sebagai program yang BERJALAN: dulu ini
    // mencetak `30`.
    let src = "package main\npub fn main() {\n  let a = [10,20,30]\n                 match arr::index_of(a, 99) { Some(i) => wok(a[i]), None => wok(\"tidak ada\") }\n}";
    let i = jalankan(src, false);
    let v = jalankan(src, true);
    assert_eq!(i.keluaran.trim(), "tidak ada");
    assert_eq!(i.keluaran, v.keluaran);
}

/// Tiap fungsi `std/` yang publik punya satu kalimat penjelasan.
#[test]
fn tiap_fungsi_std_punya_penjelasan() {
    let mut total = 0usize;
    let mut kosong: Vec<String> = Vec::new();
    for f in jrgc::interpreter::stdlib_src::fungsi_publik() {
        total += 1;
        if f.ringkas.trim().is_empty() { kosong.push(f.nama.clone()); }
    }
    assert!(total > 80, "daftar fungsi std/ sepertinya rusak: {}", total);
    eprintln!("\n── cakupan penjelasan std/: {}/{} ──", total - kosong.len(), total);
    assert!(kosong.is_empty(),
        "{} fungsi `std/` tanpa penjelasan — `--doc` menampilkannya sebagai \
         \"belum ada penjelasan tertulis\", dan itu terlihat pemakai:\n  {}",
        kosong.len(), kosong.join("\n  "));
}

#[test]
fn ukuran_tabel_diferensial_dilaporkan() {
    let baris = [
        ("PARITAS", PARITAS.len()),
        ("DITOLAK_DUA_BACKEND", DITOLAK_DUA_BACKEND.len()),
        ("DIVERGENSI_DIKETAHUI", DIVERGENSI_DIKETAHUI.len()),
        ("BELUM_DIDUKUNG_VM", BELUM_DIDUKUNG_VM.len()),
        ("VM_LEBIH_PERMISIF", VM_LEBIH_PERMISIF.len()),
        ("JEJAK", JEJAK.len()),
    ];
    eprintln!("\n── ukuran tabel diferensial (backend_parity) ──");
    for (n, k) in baris { eprintln!("  {n:24} {k}"); }
    // Tiga tabel di bawah dicatat KOSONG di dokumentasi.
    assert!(
        DIVERGENSI_DIKETAHUI.is_empty()
            && BELUM_DIDUKUNG_VM.is_empty()
            && VM_LEBIH_PERMISIF.is_empty(),
        "tabel yang dicatat KOSONG ternyata terisi — perbarui dokumentasinya \
         (divergensi={}, belum_didukung={}, lebih_permisif={})",
        DIVERGENSI_DIKETAHUI.len(), BELUM_DIDUKUNG_VM.len(), VM_LEBIH_PERMISIF.len()
    );
    assert!(!PARITAS.is_empty() && !DITOLAK_DUA_BACKEND.is_empty(),
        "tabel utama kosong — instrumen yang tidak memuat apa pun lulus dengan sempurna");
}

/// Nama representasi internal tidak boleh sampai ke pemakai.
#[test]
fn pesan_tidak_membocorkan_kunci_internal() {
    const BOCOR: &[&str] = &["__payload__", "__type__"];
    let kasus: &[&str] = &[
        "let o = Option::Some(1); wok(o.tidakada())",
        "let o = Option::None; wok(o.tidakada())",
        "let r = Ok(1); wok(r.tidakada())",
        "let o = Option::Some(1); wok(o.tidakada)",
    ];
    let mut gagal = Vec::new();
    for k in kasus {
        let src = format!("package main\npub fn main() {{ {} }}\n", k);
        for vm in [false, true] {
            let o = jalankan_keluaran_penuh(&src, vm);
            for b in BOCOR {
                if o.contains(b) {
                    gagal.push(format!("  [{}] {} membocorkan `{}`:\n{}",
                        if vm { "vm" } else { "interp" }, k, b, o.trim()));
                }
            }
        }
    }
    assert!(gagal.is_empty(), "nama internal bocor ke pemakai:\n{}", gagal.join("\n"));
}

/// Sisi positifnya: field pemakai bernama `variant` TETAP terlihat.
#[test]
fn field_pemakai_bernama_variant_tidak_ikut_disaring() {
    let src = "package main\n\
               struct P { pub variant: int, pub lain: int }\n\
               pub fn main() { let p = P{variant: 7, lain: 2}; wok(p); wok(p.variant) }\n";
    let i = jalankan(src, false);
    let v = jalankan(src, true);
    assert!(i.sukses && v.sukses, "program sah ditolak — i={} vm={}", i.sukses, v.sukses);
    assert_eq!(i.keluaran, v.keluaran, "tampilan struct berbeda antar-backend");
    for wajib in ["variant", "7"] {
        assert!(i.keluaran.contains(wajib),
            "field `variant` hilang dari tampilan struct pemakai: {:?}", i.keluaran);
    }
}

/// **Argumen PERTAMA sebuah `str::` harus teks — kecuali yang memang MERENDER.**
/// Lahir dari 251 kasus di 45 nama dengan **0 asimetri** — dua mesin sepakat,
/// jadi tak satu tabel pun melihatnya.
#[test]
fn argumen_pertama_str_harus_teks() {
    // Yang MERENDER: argumen pertamanya memang ditampilkan, bukan diolah.
    // `str::pad_left(42, 5)` menjawab `   42` dan itu yang penulisnya maksud —
    // `str::format` sendiri mengarahkan lebar angka ke sini.
    const MERENDER: &[&str] = &["pad_left", "pad_right", "center", "ljust", "rjust",
                                "to_string"];
    // Bukan pintu teks sama sekali: argumen pertamanya array (`join`) atau
    // sebuah nilai yang memang boleh angka (pintu parse Fase KONVERSI).
    const BUKAN_PINTU_TEKS: &[&str] = &["join", "to_int", "to_float"];

    let reg = jrgc::vm::daftar_builtin();
    let nama: Vec<String> = reg.iter()
        .filter_map(|n| n.strip_prefix("str::"))
        .filter(|s| !s.starts_with("__") && !BUKAN_PINTU_TEKS.contains(s))
        .map(|s| s.to_string()).collect();

    let jalan = |ekspr: &str| {
        let src = format!("package main\npub fn main() {{\n    wok({})\n}}\n", ekspr);
        (jalankan_keluaran_penuh(&src, false), jalankan_keluaran_penuh(&src, true))
    };
    let diterima = |k: &str| !k.lines().any(|l| l.starts_with("error"));
    let baris1 = |k: &str| k.lines().find(|l| l.starts_with("error"))
        .unwrap_or("(tidak ditolak)").to_string();

    // Ekor argumen yang DICOBA sampai bentuk berargumen-pertama STRING diterima
    // di DUA mesin. Yang tidak ketemu DILAPORKAN, bukan dilewati diam-diam:
    // melewatinya menyusutkan sapuannya tanpa satu baris pun berubah warna.
    const EKOR: &[&str] = &["", "\"a\"", "2", "\",\"", "0, 1", "\"a\", \"b\"",
                            "\",\", 2", "\"a\", 2", "5, \"0\""];
    let mut salah: Vec<String> = Vec::new();
    let mut dicek = 0usize;
    for m in &nama {
        let bentuk = EKOR.iter().find(|e| {
            let arg = if e.is_empty() { "\"halo dunia\"".to_string() }
                      else { format!("\"halo dunia\", {}", e) };
            let (i, v) = jalan(&format!("str::{}({})", m, arg));
            diterima(&i) && diterima(&v)
        });
        let Some(bentuk) = bentuk else {
            salah.push(format!("  str::{}: bentuk argumennya tidak ketemu — \
                                tambahkan ke EKOR, jangan biarkan tersapu keluar", m));
            continue;
        };
        // `rune` sengaja tidak ada di sini: ia memang satu karakter, dan tiap
        // pintu teks menerimanya (Fase POLA-STR).
        for lit in ["[1, 2]", "nil", "7", "true", "{\"a\": 1}", "1.5"] {
            let arg = if bentuk.is_empty() { lit.to_string() }
                      else { format!("{}, {}", lit, bentuk) };
            let (i, v) = jalan(&format!("str::{}({})", m, arg));
            dicek += 1;
            if MERENDER.contains(&m.as_str()) {
                // Sisi positif: yang merender harus tetap MENERIMA.
                if !diterima(&i) || !diterima(&v) {
                    salah.push(format!("  str::{}({}) ditolak, padahal ia MERENDER: \
                        interp={} vm={}", m, arg, baris1(&i), baris1(&v)));
                }
                continue;
            }
            if diterima(&i) || diterima(&v) {
                salah.push(format!("  str::{}({}) diterima (i={} v={}) — argumen \
                    pertamanya bukan teks", m, arg, diterima(&i), diterima(&v)));
            } else if baris1(&i) != baris1(&v) {
                salah.push(format!("  str::{}({}) dua kalimat:\n     interp: {}\n     vm    : {}",
                    m, arg, baris1(&i), baris1(&v)));
            } else if !baris1(&i).contains("harus string") || !baris1(&i).contains("str(x)") {
                // Dua mesin yang sepakat pada kalimat SALAH BENTUK tetap
                // "identik", jadi kesamaan saja tidak cukup — `str::len` dulu
                // menjawab bentuk tanda tangannya, tanpa `str(x)`.
                salah.push(format!("  str::{}({}) ditolak dengan kalimat lain: {}",
                    m, arg, baris1(&i)));
            }
        }
    }
    // Sebuah sapuan yang tidak berjalan lulus dengan sempurna.
    assert!(dicek >= 240, "cuma {} kasus disapu — sapuannya menyusut", dicek);
    assert!(salah.is_empty(), "{} kasus:\n{}", salah.len(), salah.join("\n"));
}

/// Fungsi milik modul disebut dengan nama yang SAMA di dua mesin saat jumlah
/// argumennya salah — `dua`, bukan `bantu::dua` di `--vm` saja (#354).
#[test]
fn nama_fungsi_modul_sama_di_pesan_aritas() {
    let dir = crate::bersama::dir_unik("jrg-nama-modul");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    std::fs::write(dir.join("bantu.jrg"),
        "package bantu\npub fn dua(a, b) { return a + b }\n\
         pub struct P { pub n: int }\nimpl P { pub fn m(self, k) { return self.n + k } }\n")
        .expect("tulis modul");
    let program = [
        ("kurang", "wok(bantu::dua(1))"),
        ("lebih", "wok(bantu::dua(1, 2, 3))"),
        ("method", "wok(P { n: 1 }.m())"),
    ];
    for (nama, isi) in program {
        std::fs::write(dir.join("main.jrg"),
            format!("import \"bantu\"\nwok(\"mulai\")\n{}\n", isi)).expect("tulis main");
        let jalan = |vm: bool| {
            let mut cmd = Command::new(jrgc());
            cmd.current_dir(&dir).arg("main.jrg").env("JRG_SOPAN", "1");
            if vm { cmd.arg("--vm"); }
            let out = jalankan_berbatas(cmd);
            let galat = String::from_utf8_lossy(&out.stderr).lines()
                .find(|b| b.starts_with("error")).unwrap_or("").to_string();
            (String::from_utf8_lossy(&out.stdout).to_string(), galat)
        };
        let (i, v) = (jalan(false), jalan(true));
        assert_eq!(i.0, "mulai\n", "{}: keluaran sebelum galat hilang: {:?}", nama, i);
        assert!(!i.1.is_empty(), "{}: interpreter harus menolak", nama);
        assert_eq!(i, v, "{}: dua mesin menyebutnya berbeda", nama);
        assert!(!i.1.contains("bantu::"), "{}: nama modul bocor ke pesan: {}", nama, i.1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Kode keluar `exit` sampai ke pemanggil apa adanya di dua mesin, dan yang tidak sah
/// gagal sesudah keluaran sebelumnya.** Dulu `exit(256)`, `exit(2.5)` dan `exit("a")`
/// keluar 0 — program yang melapor gagal terbaca sukses oleh skrip pemanggilnya.
#[test]
fn kode_keluar_sampai_ke_pemanggil() {
    let jalan = |badan: &str, pakai_vm: bool| {
        let dir = crate::bersama::dir_unik("jrg-keluar");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        let file = dir.join("uji.jrg");
        std::fs::write(&file, format!("wok(\"mulai\")\n{}\nwok(\"lewat\")\n", badan)).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.arg(&file);
        if pakai_vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        let _ = std::fs::remove_dir_all(&dir);
        (out.kode, String::from_utf8_lossy(&out.stdout).to_string(),
         String::from_utf8_lossy(&out.stderr).to_string())
    };
    let mut salah = Vec::new();
    for vm in [false, true] {
        for kode in [0, 1, 7, 42, 255] {
            for nama in ["exit", "std::exit"] {
                let (k, so, se) = jalan(&format!("{}({})", nama, kode), vm);
                if k != Some(kode) || so != "mulai\n" || !se.is_empty() {
                    salah.push(format!("  {}({}) vm={}: kode {:?}, stdout {:?}, stderr {:?}", nama, kode, vm, k, so, se));
                }
            }
        }
        let (k, so, _) = jalan("exit()", vm);
        if k != Some(0) || so != "mulai\n" { salah.push(format!("  exit() vm={}: kode {:?}, stdout {:?}", vm, k, so)); }
        // Yang tidak sah: gagal (kode 1) SESUDAH `mulai`, dengan kalimatnya, dan tidak lanjut.
        for (badan, potongan) in [
            ("exit(256)", "dapat 256"), ("exit(4294967296)", "dapat 4294967296"),
            ("exit(0 - 1)", "dapat -1"), ("exit(2.5)", "dapat `float`"),
            ("exit(\"a\")", "dapat `string`"), ("exit(nil)", "dapat `nil`"),
            ("exit(true)", "dapat `bool`"), ("exit([1])", "dapat `array`"),
        ] {
            let (k, so, se) = jalan(badan, vm);
            if k != Some(1) || so != "mulai\n" || !se.contains("kode keluar harus") || !se.contains(potongan) {
                salah.push(format!("  {} vm={}: kode {:?}, stdout {:?}, galat {:?}", badan, vm, k, so,
                    se.lines().find(|l| l.starts_with("error")).unwrap_or("")));
            }
        }
    }
    assert!(salah.is_empty(), "{} masalah kode keluar:\n{}", salah.len(), salah.join("\n"));
}

/// **Literal integer di luar jangkauan dan luapan aritmetika: satu kalimat, dua mesin, dan
/// waktu gagal yang sama.** Literal di atas `i128::MAX` dulu MEMBUNGKUS di acuan (`2^127`
/// dicetak negatif) dan ditolak tanpa lokasi di `--vm`; `-(i128::MIN)` membungkus di dua mesin.
#[test]
fn literal_dan_luapan_satu_kalimat_di_dua_mesin() {
    const M: &str = "170141183460469231731687303715884105727";
    const LITERAL: &str = "di luar jangkauan integer JRG";
    const LUBER: &str = "kelewat besar buat integer";
    const NEG: &str = "tidak ada lawan positifnya";
    // (badan sesudah `wok("mulai")`, potongan wajib, ditolak PARSER — `mulai` tidak tercetak)
    let kasus: Vec<(String, &str, bool)> = vec![
        ("wok(170141183460469231731687303715884105728)".into(), LITERAL, true),
        ("wok(0xffffffffffffffffffffffffffffffff)".into(), LITERAL, true),
        ("wok(-170141183460469231731687303715884105729)".into(), LITERAL, true),
        ("wok(-170141183460469231731687303715884105728.abs())".into(), LITERAL, true),
        ("wok(-(170141183460469231731687303715884105728))".into(), LITERAL, true),
        ("let x = [1, 170141183460469231731687303715884105728]".into(), LITERAL, true),
        ("match 1 { 170141183460469231731687303715884105728 => wok(1), _ => wok(2) }".into(), LITERAL, true),
        ("match 1 { -170141183460469231731687303715884105729 => wok(1), _ => wok(2) }".into(), LITERAL, true),
        ("match 1 { 0..=170141183460469231731687303715884105728 => wok(1), _ => wok(2) }".into(), LITERAL, true),
        (format!("let a = {}\nwok(a + 1)", M), LUBER, false),
        (format!("let a = 0 - {} - 1\nwok(a - 1)", M), LUBER, false),
        (format!("let a = {}\nwok(a * 2)", M), LUBER, false),
        (format!("let a = 0 - {} - 1\nwok(a / (0 - 1))", M), LUBER, false),
        (format!("let a = 0 - {} - 1\nwok(a % (0 - 1))", M), LUBER, false),
        ("let b = 2\nwok(b ** 200)".into(), LUBER, false),
        (format!("var a = {}\na += 1\nwok(a)", M), LUBER, false),
        (format!("wok(arr::sum([{}, 1]))", M), LUBER, false),
        (format!("wok(arr::product([{}, 2]))", M), LUBER, false),
        (format!("wok([{}, 2].product())", M), LUBER, false),
        (format!("let m = 0 - {} - 1\nwok(-m)", M), NEG, false),
        ("wok(- -170141183460469231731687303715884105728)".into(), NEG, false),
    ];
    let mut salah = Vec::new();
    for (badan, potongan, saat_urai) in &kasus {
        let src = format!("wok(\"mulai\")\n{}\n", badan);
        let i = jalankan_keluaran_penuh(&src, false);
        let v = jalankan_keluaran_penuh(&src, true);
        let (gi, gv) = (jawaban_ringkas(&i), jawaban_ringkas(&v));
        let bentuk = badan.replace('\n', "; ");
        if !gi.starts_with("error") || !gi.contains(potongan) {
            salah.push(format!("  {} — interpreter: {}", bentuk, gi));
        } else if gi != gv {
            salah.push(format!("  {} — beda:\n    interp: {}\n    vm    : {}", bentuk, gi, gv));
        } else if i.contains("mulai") == *saat_urai || v.contains("mulai") == *saat_urai {
            salah.push(format!("  {} — waktu gagalnya salah (harus {}): interp `mulai` {}, vm `mulai` {}",
                bentuk, if *saat_urai { "saat diurai" } else { "saat jalan" },
                i.contains("mulai"), v.contains("mulai")));
        }
    }
    assert!(salah.is_empty(), "{} dari {} kasus salah:\n{}", salah.len(), kasus.len(), salah.join("\n"));
}

mod ruang {
    include!("bersama/ruang.rs");
}

/// Ruang nama modul: tiap skenario multi-berkas diadu dengan jawaban yang BENAR di dua mesin.
/// Membandingkan dua mesin saja tidak cukup — beberapa cacatnya disepakati keduanya
/// (`fn terap` pemakai membajak `f(...v)`, dua modul bernama sama dari direktori berbeda).
#[test]
fn ruang_nama_modul_satu_arti_di_dua_mesin() {
    use ruang::Harap;
    let baris_galat = |err: &str| err.lines().find(|l| l.starts_with("error"))
        .unwrap_or("(tanpa baris galat)").chars().take(220).collect::<String>();
    let mut salah = Vec::new();
    for s in ruang::SKENARIO {
        for vm in [false, true] {
            let dir = crate::bersama::dir_unik("jrg-ruang");
            for (nama, isi) in s.berkas {
                let p = dir.join(nama);
                std::fs::create_dir_all(p.parent().expect("induk")).expect("bikin dir");
                std::fs::write(&p, isi).expect("tulis berkas");
            }
            let mut cmd = Command::new(jrgc());
            cmd.arg(dir.join("main.jrg")).arg("--sopan").current_dir(&dir);
            if vm { cmd.arg("--vm"); }
            let out = jalankan_berbatas(cmd);
            let _ = std::fs::remove_dir_all(&dir);
            let (so, se) = (String::from_utf8_lossy(&out.stdout).to_string(),
                            String::from_utf8_lossy(&out.stderr).to_string());
            let mesin = if vm { "--vm" } else { "interp" };
            let masalah = match &s.harap {
                Harap::Keluaran(h) if out.status_sukses && so == *h => None,
                Harap::Keluaran(h) => Some(format!("harap {:?}, dapat {:?} (kode {:?}) {}",
                    h, so, out.kode, baris_galat(&se))),
                Harap::Tolak { .. } if out.status_sukses =>
                    Some(format!("harus DITOLAK, tapi jalan: {:?}", so)),
                Harap::Tolak { pesan, .. } if !se.contains(pesan) =>
                    Some(format!("pesannya tidak memuat {:?}: {}", pesan, baris_galat(&se))),
                Harap::Tolak { sebelum, .. } if so != *sebelum =>
                    Some(format!("keluaran sebelum galat harus {:?}, dapat {:?}", sebelum, so)),
                Harap::Tolak { .. } => None,
            };
            if let Some(m) = masalah {
                salah.push(format!("  [{}] {}\n      {}", mesin, s.nama, m));
            }
        }
    }
    // Lantai: tabel yang menyusut diam-diam membuat tes ini hijau tanpa memeriksa apa pun.
    assert!(ruang::SKENARIO.len() >= 62, "skenario ruang nama tinggal {}", ruang::SKENARIO.len());
    assert!(salah.is_empty(), "{} dari {} (skenario x mesin) salah:\n{}",
        salah.len(), ruang::SKENARIO.len() * 2, salah.join("\n"));
}

/// **Operator x jenis operand: satu jawaban di dua mesin** (Fase LOGIKA). Tiap pasangan nilai
/// lewat tiap operator; yang dibandingkan nilai dan tipe hasilnya, atau kalimat galatnya.
/// Lahir dari 4274 beda atas 20.274 ekspresi — `1 && 2` menjawab `2` di `--vm`.
#[test]
fn operator_kali_jenis_sama_di_dua_mesin() {
    const NILAI: &[&str] = &[
        "7", "0", "-3", "2.5", "0.0", "true", "false", "\"s\"", "\"7\"", "\"\"", "'r'", "nil", "()",
        "[1, 2]", "[]", "[true]", "[math::NAN]", "(1, 2)", "{\"a\": 1}", "Option::Some(1)",
        "Option::None", "P { n: 3 }", "Q { n: 3 }", "f", "(|x| x)", "BESAR", "KECIL", "math::NAN",
        "math::INF", "E::A", "E::B(1)", "1.0", "4294967297", "-4294967297",
    ];
    const BINER: &[&str] = &["+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=",
        "&&", "||", "&", "|", "^", "<<", ">>", "&^", "??"];
    let kepala = "package main\nstruct P { pub n: int }\nstruct Q { pub n: int }\nimpl Q {\n\
        fn add(self, o) -> int { return 100 }\nfn sub(self, o) -> int { return 101 }\n\
        fn mul(self, o) -> int { return 102 }\nfn div(self, o) -> int { return 103 }\n\
        fn rem(self, o) -> int { return 104 }\nfn pow(self, o) -> int { return 105 }\n\
        fn bit_and(self, o) -> int { return 106 }\nfn bit_or(self, o) -> int { return 107 }\n\
        fn bit_xor(self, o) -> int { return 108 }\nfn shl(self, o) -> int { return 109 }\n\
        fn shr(self, o) -> int { return 110 }\nfn neg(self) -> int { return 111 }\n\
        fn not(self) -> int { return 112 }\nfn bit_not(self) -> int { return 113 }\n\
        fn eq(self, o) -> bool { return true }\nfn cmp(self, o) -> int { return 0 - 1 }\n}\n\
        enum E { A, B(int) }\nfn f(x) { return x }\n\
        const BESAR = 170141183460469231731687303715884105727\n\
        const KECIL = -170141183460469231731687303715884105727\n";
    let mut ekspresi: Vec<String> = Vec::new();
    for op in BINER {
        for a in NILAI { for b in NILAI { ekspresi.push(format!("({}) {} ({})", a, op, b)); } }
    }
    for op in ["-", "!", "~"] { for a in NILAI { ekspresi.push(format!("{}({})", op, a)); } }
    assert!(ekspresi.len() > 20_000, "sapuannya menyusut: {} ekspresi", ekspresi.len());

    let badan: Vec<String> = ekspresi.iter()
        .map(|e| format!("let h = {}\n    wok(str(h) + \"|\" + typeof(h))", e)).collect();
    // Dipotong per 1500 fungsi: satu proses per potong per mesin, empat potong sekaligus.
    let potong: Vec<&[String]> = badan.chunks(1500).collect();
    let giliran = std::sync::atomic::AtomicUsize::new(0);
    let beda = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let k = giliran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(bagian) = potong.get(k) else { break };
                let i = jalankan_uji_keluaran(kepala, bagian, false);
                let v = jalankan_uji_keluaran(kepala, bagian, true);
                for (n, (a, b)) in i.iter().zip(v.iter()).enumerate() {
                    if a != b {
                        beda.lock().unwrap().push(format!("  {}\n    int: {:?}\n    vm : {:?}",
                            ekspresi[k * 1500 + n], a, b));
                    }
                }
            });
        }
    });
    let mut beda = beda.into_inner().unwrap();
    beda.sort();
    assert!(beda.is_empty(), "{} dari {} ekspresi dijawab berbeda di dua mesin:\n{}",
        beda.len(), ekspresi.len(), beda.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
}

/// **`defer` tetap jalan saat galat membawa fungsinya keluar, di dua mesin** (Fase LOGIKA).
/// Dulu cuma interpreter: `--vm` langsung melapor, jadi keluaran SEBELUM galatnya berbeda —
/// dan `--uji --vm` menumpuk satu bingkai per test gagal sampai kena batas rekursi.
#[test]
fn defer_jalan_saat_galat_di_dua_mesin() {
    // (program, stdout sebelum galat, potongan yang harus ada di stderr)
    let kasus: &[(&str, &str, &[&str])] = &[
        ("fn f() { defer wok(\"tunda f\")\n panik(\"sengaja\") }\n\
          fn g() { defer wok(\"tunda g\")\n f()\n wok(\"tidak sampai\") }\nwok(\"mulai\")\ng()\n",
         "mulai\ntunda f\ntunda g\n",
         &["error: sengaja", "`f` dipanggil di", "`g` dipanggil di"]),
        ("fn gagal() { let a = [1]\n return a[9] }\n\
          fn f() { defer wok(\"a\")\n defer gagal()\n defer wok(\"c\")\n return 1 }\n\
          wok(\"mulai\")\nwok(f())\n",
         "mulai\nc\na\n", &["index [9] di luar batas"]),
        ("fn dalam() { let a = [1]\n return a[7] }\n\
          fn bersih() { let d = {\"k\": 1}\n return d[\"tak ada\"] }\n\
          fn f() { defer bersih()\n defer wok(\"tunda f\")\n return dalam() }\nwok(f())\n",
         "tunda f\n",
         &["index [7] di luar batas", "sebuah `defer` juga gagal saat membereskan: key `tak ada`",
           "`dalam` dipanggil di", "`f` dipanggil di"]),
        ("struct P { pub n: int }\n\
          impl P { fn m(self) { defer wok(\"tunda m\")\n return 1 / (self.n - self.n) } }\n\
          fn h() { defer wok(\"tunda h\")\n return arr::map([1], |x| { defer wok(\"tunda closure\")\n\
          return P { n: x }.m() }) }\nwok(h())\n",
         "tunda m\ntunda closure\ntunda h\n", &["nggak bisa bagi dengan nol"]),
    ];
    let jalan = |dir: &std::path::Path, uji: bool, vm: bool| {
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(dir).arg("p.jrg");
        if uji { cmd.arg("--uji"); }
        if vm { cmd.arg("--vm"); }
        let k = jalankan_berbatas(cmd);
        (String::from_utf8_lossy(&k.stdout).to_string(), String::from_utf8_lossy(&k.stderr).to_string())
    };
    let mut salah = Vec::new();
    for (src, keluar, potongan) in kasus {
        let dir = crate::bersama::dir_unik("jrg-tunda");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        std::fs::write(dir.join("p.jrg"), src).expect("tulis sumber");
        let (i, v) = (jalan(&dir, false, false), jalan(&dir, false, true));
        let _ = std::fs::remove_dir_all(&dir);
        for (mesin, (so, se)) in [("interpreter", &i), ("--vm", &v)] {
            if so != keluar {
                salah.push(format!("  {}: keluaran sebelum galat {:?}, harusnya {:?}\n{}", mesin, so, keluar, src));
            }
            for p in *potongan {
                if !se.contains(p) { salah.push(format!("  {}: stderr tanpa `{}`:\n{}\n{}", mesin, p, se, src)); }
            }
        }
        if i.1 != v.1 { salah.push(format!("  stderr beda:\n--- interpreter\n{}--- vm\n{}", i.1, v.1)); }
    }
    // Bingkai test yang gagal tidak boleh tertinggal: 200 test gagal, lalu rekursi 9.900 tingkat —
    // dengan bingkai yang menumpuk, yang terakhir dijawab "rekursi terlalu dalam".
    let mut src = String::from("package main\nfn dalam(n) { if n == 0 { return 0 }\n return 1 + dalam(n - 1) }\n");
    for n in 0..200 { src.push_str(&format!("fn test_{:03}() {{ defer wok(\"d\")\n panik(\"x\") }}\n", n)); }
    src.push_str("fn test_zz_dalam() { assert_eq(dalam(9900), 9900) }\n");
    let dir = crate::bersama::dir_unik("jrg-tunda-uji");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    std::fs::write(dir.join("p.jrg"), &src).expect("tulis sumber");
    let (so, se) = jalan(&dir, true, true);
    let _ = std::fs::remove_dir_all(&dir);
    if !so.contains("ok    test_zz_dalam") || !so.contains("1 lulus, 200 gagal") || so.matches("\nd\n").count() < 199 {
        salah.push(format!("  --uji --vm sesudah 200 test gagal:\n{}\n{}",
            so.lines().rev().take(4).collect::<Vec<_>>().join("\n"), se));
    }
    assert!(salah.is_empty(), "{} masalah:\n{}", salah.len(), salah.join("\n"));
}

/// **Callback berupa fungsi BERNAMA — dengan dan tanpa nilai kembalian — menjawab sama di dua
/// mesin, dan lokal pemanggilnya utuh** (Fase LOGIKA). Fungsi tanpa nilai tidak mendorong apa
/// pun, dan `--vm` dulu mengambil satu nilai milik pemanggil: `[1, 2].map(diam)` → `[2, 1]`.
#[test]
fn callback_bernama_sama_di_dua_mesin() {
    let kepala = "package main\n\
        fn k0() { let z = 0 }\nfn k1(p) { let z = p }\nfn k2(p, q) { let z = p }\nfn k3(p, q, r) { let z = p }\n\
        fn v0() { return 1 }\nfn v1(p) { return p }\nfn v2(p, q) { return p }\nfn v3(p, q, r) { return p }\n";
    // Tiap contoh BENAR yang menerima closure: closure-nya diganti fungsi bernama searitas.
    let mut badan: Vec<String> = Vec::new();
    for (nama, buka, args) in CONTOH_ARGUMEN {
        for (i, a) in args.iter().enumerate() {
            let Some(sisa) = a.trim_start().strip_prefix('|') else { continue };
            let Some((param, _)) = sisa.split_once('|') else { continue };
            let n = param.split(',').filter(|p| !p.trim().is_empty()).count();
            if n > 3 { continue; }
            for awalan in ["k", "v"] {
                let mut b: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                b[i] = format!("{}{}", awalan, n);
                let pembuka = if buka.is_empty() { String::new() } else { format!("{}\n    ", buka) };
                badan.push(format!("let a = 11\n    let b = 22\n    {}let h = {}({})\n    wok(a, b)\n    wok(h)",
                    pembuka, nama, b.join(", ")));
            }
        }
    }
    assert!(badan.len() >= 60, "sapuannya menyusut: {} kasus", badan.len());
    let (i, v) = (jalankan_uji_keluaran(kepala, &badan, false), jalankan_uji_keluaran(kepala, &badan, true));
    let mut salah = Vec::new();
    for (k, (a, b)) in i.iter().zip(v.iter()).enumerate() {
        let lokal_utuh = |h: &Result<String, String>| h.as_ref().map_or(true, |s| s.starts_with("11 22"));
        if a != b || !lokal_utuh(a) || !lokal_utuh(b) {
            salah.push(format!("  {}\n    int: {:?}\n    vm : {:?}", badan[k].lines().nth(2).unwrap_or("").trim(), a, b));
        }
    }
    assert!(salah.is_empty(), "{} dari {} callback bernama dijawab berbeda:\n{}",
        salah.len(), badan.len(), salah.join("\n"));
}

/// Nama TIPE milik unitnya (Fase RUANG-TIPE): dua modul boleh sama-sama punya `Titik`,
/// `Warna`, varian `Merah`; tiap nilai memakai method, field, dan pola tipenya sendiri di
/// dua mesin. Nama yang dimiliki dua import sama dekat ditolak sebelum `main`.
#[test]
fn nama_tipe_milik_unitnya_di_dua_mesin() {
    let dir = crate::bersama::dir_unik("jrg-ruang-tipe");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let berkas: &[(&str, &str)] = &[
        ("a.jrg", r#"package a
pub struct Titik { pub x: int = 7, rahasia: int = 1 }
pub struct P(int, int)
pub enum Warna { Merah, Rgb(int, int, int), Kotak { s: int } }
impl Titik {
    pub fn baru() -> Titik { Titik { x: 3 } }
    pub fn nama(self) -> string { "titik a" }
    pub fn to_string(self) -> string { "A(" + str(self.x) + ")" }
    pub fn intip(self) -> int { self.rahasia }
}
impl Warna { pub fn nama(self) -> string { match self { Warna::Merah => "merah a", Rgb(r, _, _) => "rgb a " + str(r), _ => "lain a" } } }
pub fn buat() { Titik { x: 1 } }
pub fn pasang() { P(1, 2) }
pub fn warna() { Rgb(1, 2, 3) }
pub fn cocok(t) -> string { match t { Titik { x } => "a:" + str(x), _ => "bukan a" } }
"#),
        ("b.jrg", r#"package b
pub struct Titik { pub x: int = 70, pub y: int = 80 }
pub struct P(string)
pub enum Warna { Merah, Biru, Rgb(string) }
impl Titik {
    pub fn baru() -> Titik { Titik { x: 30, y: 40 } }
    pub fn nama(self) -> string { "titik b" }
    pub fn add(self, o) { Titik { x: self.x + o.x, y: self.y + o.y } }
}
impl Warna { pub fn nama(self) -> string { match self { Warna::Merah => "merah b", Warna::Biru => "biru b", Rgb(s) => "rgb b " + s } } }
pub fn buat() { Titik { x: 1, y: 2 } }
pub fn pasang() { P("satu") }
pub fn warna() { Rgb("teks") }
pub fn cocok(t) -> string { match t { Titik { x, y } => "b:" + str(x + y), _ => "bukan b" } }
"#),
        ("c.jrg", r#"package c
import "b"
pub struct Anak: Titik { pub z: int = 9 }
pub fn anak() { Anak { x: 1, y: 2, z: 3 } }
pub fn dari_b() { Titik {} }
"#),
        ("lampu_a.jrg", r#"package lampu_a
pub enum Lampu { Merah, Hijau, Kedip(int), Atur { n: int } }
pub fn bikin(i) { if i == 0 { Merah } else if i == 1 { Hijau } else if i == 2 { Kedip(5) } else { Atur { n: 7 } } }
pub fn sebut(l) -> string {
    match l { Merah => "a merah", Hijau => "a hijau", Kedip(n) => "a kedip " + str(n), Atur { n } => "a atur " + str(n), _ => "a bukan" }
}
"#),
        ("lampu_b.jrg", r#"package lampu_b
pub enum Lampu { Hijau, Merah, Kedip(string, string), Atur { n: string, m: int } }
pub fn bikin(i) { if i == 0 { Merah } else if i == 1 { Hijau } else if i == 2 { Kedip("x", "y") } else { Atur { n: "t", m: 1 } } }
pub fn sebut(l) -> string {
    match l { Merah => "b merah", Hijau => "b hijau", Kedip(p, q) => "b kedip " + p + q, Atur { n, m } => "b atur " + n + str(m), _ => "b bukan" }
}
"#),
        ("main.jrg", r#"package main
import "a"
import "b"
import "c"
struct Titik { pub n: string = "utama" }
impl Titik { fn nama(self) -> string { "titik utama" } }
pub fn main() {
    let p = a::buat()
    let q = b::buat()
    let r = Titik {}
    wok(p.nama(), q.nama(), r.nama())
    wok(typeof(p), typeof(q), typeof(r))
    wok(p, q, r, [p], (q, 1), {"k": r})
    wok(«${p} ${q}», "x" + str(q), str::pad_left(q, 22))
    wok(p == q, p == a::buat(), q == b::buat(), q != r)
    wok(a::cocok(p), a::cocok(q), b::cocok(p), b::cocok(q), a::cocok(r))
    wok(a::Titik::baru(), b::Titik::baru(), a::Titik {}, b::Titik { x: 5 }, Titik { n: "n" })
    wok(a::pasang(), b::pasang(), a::P(8, 9), b::P("dua"))
    wok(a::warna(), b::warna(), a::Warna::Merah, b::Warna::Biru, a::Merah == b::Merah)
    wok(a::warna().nama(), b::warna().nama(), b::Warna::Merah.nama(), a::Merah.nama())
    wok(q + q, b::Titik::baru().add(q))
    wok(c::anak(), c::dari_b(), c::anak().nama())
    match q { a::Titik { x } => wok("salah", x), b::Titik { x, y } => wok("benar", x, y), _ => wok("lolos") }
    match r { Titik { n } => wok("utama", n) }
    match b::warna() { a::Rgb(_, _, _) => wok("salah"), b::Rgb(s) => wok("benar", s), _ => wok("lolos") }
    match a::Merah { b::Warna::Merah => wok("salah"), a::Warna::Merah => wok("benar"), _ => wok("lolos") }
    wok(json::stringify(q), typeof(a::Rgb), a::Rgb, typeof(a::Titik::nama))
    wok(p.intip())
}
"#),
        ("taksa.jrg", r#"package main
import "a"
import "b"
pub fn main() { wok(Titik {}) }
"#),
        ("taksa_jalur.jrg", r#"package main
import "a"
import "b"
pub fn main() { wok(Titik::baru()) }
"#),
        ("taksa_pola.jrg", r#"package main
import "a"
import "b"
pub fn main() { match a::buat() { Titik { x } => wok(x) } }
"#),
        ("taksa_impl.jrg", r#"package main
import "a"
import "b"
impl Warna { fn x(self) { 1 } }
pub fn main() { wok(1) }
"#),
        ("dekat.jrg", r#"package main
import "a"
import "c"
pub fn main() { wok(Titik {}, c::dari_b(), Titik::baru()) ; wok(a::buat().y) }
"#),
        ("privat.jrg", r#"package main
import "a"
import "b"
pub fn main() { let q = b::buat(); wok(q.rahasia) }
"#),
        ("lampu.jrg", r#"package main
import "lampu_a" as a
import "lampu_b" as b
pub fn main() {
    for i in 0..4 {
        let x = a::bikin(i)
        let y = b::bikin(i)
        wok(x, y, a::sebut(x), a::sebut(y), b::sebut(x), b::sebut(y))
    }
}
"#),
        ("enum_sama.jrg", r#"package main
enum A { X, Y(int) }
enum B { X, Y(int, int), Z { s: int } }
enum C { Z { s: int } }
pub fn main() {
    match B::X { A::X => wok("salah"), B::X => wok("benar"), _ => wok("lolos") }
    match B::Y(1, 2) { A::Y(n) => wok("salah", n), B::Y(a, b) => wok("benar", a + b), _ => wok("lolos") }
    let z = B::Z { s: 1 }
    match z { C::Z { s } => wok("salah", s), B::Z { s } => wok("benar", s), _ => wok("lolos") }
}
"#),
        ("diri.jrg", r#"package main
enum B { X, Y(int, int), Z { s: int } }
impl B {
    fn nama(self) -> string {
        match self { Self::X => "x", Self::Y(a, _) => "y" + str(a), Self::Z { s } => "z" + str(s) }
    }
}
struct Titik { x: int }
pub fn main() {
    let z = B::Z { s: 4 }
    wok(B::X.nama(), B::Y(1, 2).nama(), z.nama())
}
"#),
        ("std.jrg", r#"package main
import "std/mesin/fisika"
import "std/set"
struct Kotak { pub sisi: int }
struct Set { pub n: int }
impl Kotak { fn luas(self) -> int { self.sisi * self.sisi } }
pub fn main() {
    let k = fisika::kotak(0, 0, 4, 3)
    let m = Kotak { sisi: 5 }
    wok(k, m, m.luas(), typeof(k) == typeof(m), k == m)
    wok(set::from([1, 2, 2]).len(), Set { n: 1 })
}
"#),
    ];
    for (nama, isi) in berkas { std::fs::write(dir.join(nama), isi).expect("tulis modul"); }
    let jalan = |berkas: &str, vm: bool| {
        let mut cmd = Command::new(jrgc());
        cmd.current_dir(&dir).arg(berkas).env("JRG_SOPAN", "1");
        if vm { cmd.arg("--vm"); }
        let out = jalankan_berbatas(cmd);
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };
    // (berkas, seluruh keluaran harus persis?, harapan)
    let harus: &[(&str, bool, &str)] = &[
        ("main.jrg", true, r#"titik a titik b titik utama
struct:Titik struct:Titik struct:Titik
A(1) Titik {x: 1, y: 2} Titik {n: utama} [A(1)] (Titik {x: 1, y: 2}, 1) dict {k: Titik {n: utama}}
A(1) Titik {x: 1, y: 2} xTitik {x: 1, y: 2}     Titik {x: 1, y: 2}
false true true true
a:1 bukan a bukan b b:3 bukan a
A(3) Titik {x: 30, y: 40} A(7) Titik {x: 5, y: 80} Titik {n: n}
P {_0: 1, _1: 2} P {_0: satu} P {_0: 8, _1: 9} P {_0: dua}
Warna::Rgb((1, 2, 3)) Warna::Rgb(teks) Warna::Merah Warna::Biru false
rgb a 1 rgb b teks merah b merah a
Titik {x: 2, y: 4} Titik {x: 31, y: 42}
Anak {x: 1, y: 2, z: 3} Titik {x: 70, y: 80} titik b
benar 1 2
utama utama
benar teks
benar
{"x":1,"y":2} enum_ctor:Warna::Rgb <ctor Warna::Rgb> function
1
"#),
        ("lampu.jrg", true, r#"Lampu::Merah Lampu::Merah a merah a bukan b bukan b merah
Lampu::Hijau Lampu::Hijau a hijau a bukan b bukan b hijau
Lampu::Kedip(5) Lampu::Kedip((x, y)) a kedip 5 a bukan b bukan b kedip xy
Lampu::Atur(Atur {n: 7}) Lampu::Atur(Atur {m: 1, n: t}) a atur 7 a bukan b bukan b atur t1
"#),
        ("enum_sama.jrg", true, r#"benar
benar 3
benar 1
"#),
        ("diri.jrg", true, r#"x y1 z4
"#),
        ("std.jrg", true, r#"Kotak {h: 3.0, w: 4.0, x: 0.0, y: 0.0} Kotak {sisi: 5} 25 true false
2 Set {n: 1}
"#),
        ("taksa.jrg", false, r#"error: nama tipe `Titik` taksa di sini: modul `a` dan `b` sama-sama mendeklarasikannya"#),
        ("taksa_jalur.jrg", false, r#"error: nama tipe `Titik` taksa di sini: modul `a` dan `b` sama-sama mendeklarasikannya"#),
        ("taksa_pola.jrg", false, r#"error: nama tipe `Titik` taksa di sini: modul `a` dan `b` sama-sama mendeklarasikannya"#),
        ("taksa_impl.jrg", false, r#"error: nama tipe `Warna` taksa di sini: modul `a` dan `b` sama-sama mendeklarasikannya"#),
        ("dekat.jrg", false, r#"A(7) Titik {x: 70, y: 80} A(3)
error: `struct Titik` tidak punya method atau field bernama `y` (kalau maksudnya field/key, yang ada: rahasia, x) — cek lagi penulisannya ya"#),
        ("privat.jrg", false, r#"error: `struct Titik` tidak punya method atau field bernama `rahasia` (kalau maksudnya field/key, yang ada: x, y) — cek lagi penulisannya ya"#),
    ];
    let hasil: Vec<(String, String)> = harus.iter().map(|(b, _, _)| (jalan(b, false), jalan(b, true))).collect();
    let _ = std::fs::remove_dir_all(&dir);
    for ((b, persis, h), (i, v)) in harus.iter().zip(&hasil) {
        assert_eq!(i, v, "{}: dua mesin harus menjawab sama", b);
        assert!(!i.contains(jrgc::ruang_tipe::PENANDA), "{}: nomor label tipe bocor ke keluaran:\n{}", b, i);
        if *persis { assert_eq!(i, h, "{}", b); }
        else { assert!(i.starts_with(h), "{}: harap diawali {:?}, dapat:\n{}", b, h, i); }
    }
}

/// Galat saat JALAN berbunyi sama di dua mesin, dan
/// keluaran sebelum galatnya tetap tercetak.
#[test]
fn galat_saat_jalan_sama_di_dua_mesin_sesudah_keluaran_sebelumnya() {
    let kasus: &[(&str, &str, &str)] = &[
        ("field liar di literal struct: sesudah field sebelumnya dievaluasi",
         "package main\nstruct T { x: int }\nfn f() -> int { wok(\"f\")  return 1 }\n\
          pub fn main() { wok(1)\n let q = T { x: f(), w: f(), x2: f() }\n wok(q) }",
         "error: struct `T` tidak punya field `w` — maksudnya `x`?"),
        ("goroutine yang gagal: satu awalan, main lanjut",
         "package main\nfn g() { let a = [1]  wok(a[5]) }\n\
          pub fn main() { let c = make_chan(int)\n spawn { g() }\n spawn { c <- 1 }\n wok(<-c)\n time::sleep(0.2)\n wok(\"lanjut\") }",
         "goroutine error: index [5] di luar batas"),
    ];
    for (nama, src, harap) in kasus {
        let i = jalankan_keluaran_penuh(src, false);
        let v = jalankan_keluaran_penuh(src, true);
        let awal = |s: &str| s.split("error:").next().unwrap_or("").lines().count();
        assert_eq!(awal(&i), awal(&v), "{}: keluaran sebelum galat harus sama banyak:\n{}\n---\n{}", nama, i, v);
        assert!(i.contains(harap), "{}: interpreter harus memuat {:?}, dapat:\n{}", nama, harap, i);
        assert!(v.contains(harap), "{}: --vm harus memuat {:?}, dapat:\n{}", nama, harap, v);
    }
}
