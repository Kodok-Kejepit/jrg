// tests/error_behavior.rs

use jrgc::interpreter::interpret_with_paths;
use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;

/// Lihat komentar `STACK_SIZE` di `tests/conformance.rs` — alasannya sama.
const STACK_SIZE: usize = 32 * 1024 * 1024;

/// Hasil menjalankan satu program: pesan error pertama, atau `None` kalau
/// program justru selesai dengan sukses.
fn run_expecting_error(src: &str) -> Option<String> {
    let sf = SourceFile { id: 0, name: "<kasus>".to_string(), src: src.to_string() };

    let tokens = match lex(&sf) {
        Ok(t) => t,
        Err(e) => return Some(e.msg),
    };
    let (ast, errors) = parse(tokens, false);
    if !errors.is_empty() {
        return Some(errors.iter().map(|e| e.msg.clone()).collect::<Vec<_>>().join("; "));
    }
    interpret_with_paths(&ast, Vec::new()).err().map(|e| e.message)
}

fn run_on_big_stack(src: &'static str) -> Option<String> {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || run_expecting_error(src))
        .expect("tidak bisa spawn thread")
        .join()
        .expect("program panik saat dijalankan")
}

/// Satu kasus negatif: nama, program, dan kata kunci yang wajib muncul di
/// pesan errornya (semuanya, dicek tanpa peduli huruf besar/kecil).
struct Kasus {
    nama: &'static str,
    src: &'static str,
    kata_kunci: &'static [&'static str],
}

fn periksa(kasus: &[Kasus]) {
    let mut gagal = Vec::new();
    for k in kasus {
        match run_on_big_stack(k.src) {
            None => gagal.push(format!(
                "  {} -> program TIDAK error padahal seharusnya error",
                k.nama
            )),
            Some(msg) => {
                let lower = msg.to_lowercase();
                let hilang: Vec<&str> = k
                    .kata_kunci
                    .iter()
                    .copied()
                    .filter(|kw| !lower.contains(&kw.to_lowercase()))
                    .collect();
                if !hilang.is_empty() {
                    gagal.push(format!(
                        "  {} -> pesan tidak menyebut {:?}\n     pesan: {}",
                        k.nama, hilang, msg
                    ));
                }
            }
        }
    }
    assert!(gagal.is_empty(), "{} kasus gagal:\n{}", gagal.len(), gagal.join("\n"));
}

// ── Index dan slice ────────────────────────────────────────────────────
//
// Semua kasus di sini dulu mengembalikan nilai diam-diam, bukan error.

#[test]
fn index_di_luar_batas_selalu_error() {
    periksa(&[
        Kasus {
            nama: "array: index positif kelewat",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[3]) }",
            kata_kunci: &["di luar batas", "3 elemen"],
        },
        Kasus {
            // Dulu: dijepit jadi index 0, jadi `a[-100]` mengembalikan 1.
            nama: "array: index negatif kelewat jauh",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[-100]) }",
            kata_kunci: &["di luar batas", "-100"],
        },
        Kasus {
            nama: "array: negatif tepat di luar batas",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[-4]) }",
            kata_kunci: &["di luar batas"],
        },
        Kasus {
            nama: "array kosong",
            src: "package main\npub fn main() { let a = []; wok(a[0]) }",
            kata_kunci: &["kosong"],
        },
        Kasus {
            // Dulu: dijepit jadi karakter pertama.
            nama: "string: index negatif kelewat jauh",
            src: "package main\npub fn main() { wok(\"halo\"[-99]) }",
            kata_kunci: &["di luar batas"],
        },
        Kasus {
            nama: "string: index positif kelewat",
            src: "package main\npub fn main() { wok(\"halo\"[9]) }",
            kata_kunci: &["di luar batas", "karakter"],
        },
        Kasus {
            nama: "tuple: index kelewat",
            src: "package main\npub fn main() { let t = (1,2); wok(t[5]) }",
            kata_kunci: &["di luar batas"],
        },
        Kasus {
            nama: "tuple: .N kelewat",
            src: "package main\npub fn main() { let t = (1,2); wok(t.9) }",
            kata_kunci: &["di luar batas"],
        },
        Kasus {
            // Dulu: `as usize` memotong 1.9 jadi 1 tanpa bilang apa-apa.
            nama: "index pecahan dibulatkan diam-diam",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[0:1.9]) }",
            kata_kunci: &["bulat", "as int"],
        },
        Kasus {
            nama: "index bukan angka",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[true]) }",
            kata_kunci: &["index"],
        },
    ]);
}

#[test]
fn slice_dengan_batas_ngawur_selalu_error() {
    periksa(&[
        Kasus {
            // Dulu: diam-diam jadi `[]`, jadi salah tulis batas terlihat
            // persis seperti "memang tidak ada isinya".
            nama: "slice terbalik",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[2:1]) }",
            kata_kunci: &["terbalik"],
        },
        Kasus {
            nama: "slice lewat ujung",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[0:99]) }",
            kata_kunci: &["di luar jangkauan"],
        },
        Kasus {
            nama: "slice mulai setelah ujung",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[9:10]) }",
            kata_kunci: &["di luar jangkauan"],
        },
        Kasus {
            nama: "slice string lewat ujung",
            src: "package main\npub fn main() { wok(\"halo\"[0:99]) }",
            kata_kunci: &["di luar jangkauan"],
        },
        Kasus {
            // Batas negatif DIDUKUNG (lihat konformansi `indexing.jrg`),
            // jadi ini bukan lagi kasus "ditolak" — yang diuji di sini
            // justru batas negatif yang genuinely di luar jangkauan.
            nama: "slice dengan batas negatif yang di luar jangkauan",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[-9:]) }",
            kata_kunci: &["di luar jangkauan"],
        },
        Kasus {
            nama: "batas slice pecahan ditolak",
            src: "package main\npub fn main() { let a = [1,2,3]; wok(a[0:1.5]) }",
            kata_kunci: &["bulat", "as int"],
        },
    ]);
}

// ── Rekursi ────────────────────────────────────────────────────────────

#[test]
fn rekursi_tanpa_ujung_dilaporkan_bukan_bikin_stack_overflow() {
    periksa(&[Kasus {
        nama: "rekursi tanpa base case",
        src: "package main\nfn boom(n: int) -> int { return boom(n + 1) }\n\
              pub fn main() { wok(boom(0)) }",
        kata_kunci: &["rekursi terlalu dalam", "boom", "base case"],
    }]);
}

// ── Template literal ───────────────────────────────────────────────────

#[test]
fn template_yang_cacat_ditolak() {
    periksa(&[
        Kasus {
            nama: "interpolasi kosong",
            src: "package main\npub fn main() { wok(«a ${} b») }",
            kata_kunci: &["kosong"],
        },
        Kasus {
            nama: "template tidak ditutup",
            src: "package main\npub fn main() { wok(«halo ${nama}) }",
            kata_kunci: &["belum ditutup"],
        },
        Kasus {
            nama: "interpolasi tidak ditutup",
            src: "package main\npub fn main() { wok(«halo ${nama») }",
            kata_kunci: &["belum ditutup"],
        },
        Kasus {
            nama: "variabel tak dikenal di dalam interpolasi tetap ketahuan",
            src: "package main\npub fn main() { wok(«halo ${tidakada}») }",
            kata_kunci: &["tidakada", "belum dikenal"],
        },
    ]);
}

// ── Field, method, dan nama ────────────────────────────────────────────

#[test]
fn nama_yang_tidak_ada_dilaporkan_dengan_petunjuk() {
    periksa(&[
        Kasus {
            nama: "field struct salah ketik",
            src: "package main\nstruct P { pub a: int, pub b: int }\n\
                  pub fn main() { let p = P { a: 1, b: 2 }; wok(p.c) }",
            kata_kunci: &["c", "a, b"],
        },
        Kasus {
            nama: "key dict tidak ada",
            src: "package main\npub fn main() { let d = { \"a\": 1 }; wok(d[\"b\"]) }",
            kata_kunci: &["b"],
        },
        Kasus {
            nama: "variabel belum dideklarasikan",
            src: "package main\npub fn main() { wok(belum_ada) }",
            kata_kunci: &["belum_ada", "belum dikenal"],
        },
        Kasus {
            nama: "method tidak ada",
            src: "package main\npub fn main() { wok([1,2,3].tidakada()) }",
            kata_kunci: &["tidakada"],
        },
    ]);
}

// ── Tipe ───────────────────────────────────────────────────────────────

#[test]
fn operasi_antar_tipe_yang_tidak_masuk_akal_ditolak() {
    periksa(&[
        Kasus {
            nama: "bagi nol (integer)",
            src: "package main\npub fn main() { let n = 0; wok(1 / n) }",
            kata_kunci: &["nol"],
        },
        Kasus {
            nama: "modulo nol",
            src: "package main\npub fn main() { let n = 0; wok(1 % n) }",
            kata_kunci: &["nol"],
        },
        Kasus {
            nama: "panggil sesuatu yang bukan fungsi",
            src: "package main\npub fn main() { let x = 5; wok(x()) }",
            kata_kunci: &["fungsi"],
        },
        Kasus {
            nama: "index sesuatu yang tidak bisa di-index",
            src: "package main\npub fn main() { let n = 5; wok(n[0]) }",
            kata_kunci: &["index"],
        },
    ]);
}

// ── Aritmetika ─────────────────────────────────────────────────────────

#[test]
fn integer_yang_kelewat_besar_dilaporkan_bukan_dibalik_diam_diam() {
    // Dulu `+`/`-`/`*`/`**` memakai `wrapping_*`: melewati batas 128 bit
    // membalik tandanya dan program jalan terus dengan angka yang salah.
    periksa(&[
        Kasus {
            nama: "penjumlahan lewat batas",
            src: "package main\npub fn main() { \
                  let n = 170141183460469231731687303715884105727; wok(n + 1) }",
            kata_kunci: &["kelewat besar", "128 bit"],
        },
        Kasus {
            nama: "pengurangan lewat batas bawah",
            src: "package main\npub fn main() { \
                  let n = -170141183460469231731687303715884105728; wok(n - 1) }",
            kata_kunci: &["kelewat besar"],
        },
        Kasus {
            nama: "perkalian lewat batas",
            src: "package main\npub fn main() { \
                  let n = 170141183460469231731687303715884105727; wok(n * 2) }",
            kata_kunci: &["kelewat besar"],
        },
        Kasus {
            nama: "pangkat lewat batas",
            src: "package main\npub fn main() { wok(2 ** 200) }",
            kata_kunci: &["kelewat besar"],
        },
    ]);
}

#[test]
fn pengulangan_string_yang_ngawur_ditolak() {
    periksa(&[
        Kasus {
            // Dulu dijepit ke 0, jadi hasilnya string kosong — tidak bisa
            // dibedakan dari pengulangan nol kali yang memang disengaja.
            nama: "jumlah pengulangan negatif",
            src: "package main\npub fn main() { let n = -3; wok(\"ab\" * n) }",
            kata_kunci: &["negatif"],
        },
        Kasus {
            nama: "pengulangan kegedean",
            src: "package main\npub fn main() { let n = 999999999; wok(\"abcdefgh\" * n) }",
            kata_kunci: &["kegedean"],
        },
    ]);
}

// ── Loop berlabel ──────────────────────────────────────────────────────

#[test]
fn label_yang_tidak_cocok_dilaporkan_bukan_diam_diam_salah_sasaran() {
    // `break`/`continue` berlabel dulu selalu menembus SEMUA loop (labelnya
    // tidak pernah dicocokkan ke mana pun, karena tidak ada loop yang tahu label …
    periksa(&[
        Kasus {
            nama: "break dengan label yang tidak ada loop-nya",
            src: "package main\npub fn main() { \
                  'luar: for i in 0..2 { for j in 0..2 { break 'salah } } }",
            kata_kunci: &["'salah", "berlabel"],
        },
        Kasus {
            nama: "continue dengan label yang tidak ada loop-nya",
            src: "package main\npub fn main() { \
                  'luar: for i in 0..2 { for j in 0..2 { continue 'salah } } }",
            kata_kunci: &["'salah", "berlabel"],
        },
        Kasus {
            nama: "break tanpa label di luar loop apa pun",
            src: "package main\npub fn main() { break }",
            kata_kunci: &["break", "luar loop"],
        },
        Kasus {
            nama: "label dipasang di depan sesuatu yang bukan loop",
            src: "package main\npub fn main() { 'x: let a = 1 }",
            kata_kunci: &["label", "for", "while", "loop"],
        },
    ]);
}

// ── Aritmetika yang dulu bikin interpreter PANIK ───────────────────────

#[test]
fn aritmetika_di_ujung_jangkauan_jadi_diagnostik_bukan_panic() {
    // Semua kasus di bawah dulu memicu panic runtime Rust ("attempt to divide
    // with overflow", dst) yang mematikan seluruh interpreter dan dilaporkan …
    periksa(&[
        Kasus {
            nama: "i128::MIN dibagi -1",
            src: "package main\npub fn main() { \
                  let a = -170141183460469231731687303715884105728; wok(a / -1) }",
            kata_kunci: &["kelewat besar"],
        },
        Kasus {
            nama: "i128::MIN modulo -1",
            src: "package main\npub fn main() { \
                  let a = -170141183460469231731687303715884105728; wok(a % -1) }",
            kata_kunci: &["kelewat besar"],
        },
        Kasus {
            nama: "abs dari i128::MIN",
            src: "package main\npub fn main() { \
                  let a = -170141183460469231731687303715884105728; wok(math::abs(a)) }",
            kata_kunci: &["lawan positif"],
        },
    ]);
}

#[test]
fn jumlah_geseran_bit_yang_ngawur_ditolak() {
    // Dulu jumlah geseran dipotong diam-diam dengan `y & 127`: `1 << -1`
    // jadi menggeser 127 kali dan `1 << 200` jadi 72 kali — dua kesalahan
    // berbeda yang sama-sama menghasilkan angka yang kelihatan sah.
    periksa(&[
        Kasus {
            nama: "geseran negatif",
            src: "package main\npub fn main() { let n = 0 - 1; wok(1 << n) }",
            kata_kunci: &["negatif"],
        },
        Kasus {
            nama: "geseran melebihi lebar integer",
            src: "package main\npub fn main() { wok(1 << 200) }",
            kata_kunci: &["128 bit"],
        },
    ]);
}

// ── Perbandingan dan urutan ────────────────────────────────────────────

#[test]
fn nilai_yang_tidak_sebanding_ditolak_bukan_dianggap_sama() {
    periksa(&[
        Kasus {
            // Dulu `if let Ok(..)` menelan errornya dan perulangannya
            // lanjut, jadi yang keluar salah satu elemen yang kebetulan
            // tidak pernah kalah.
            nama: "min pada array campur tipe",
            src: "package main\npub fn main() { let a = [1, \"dua\", 3]; wok(a.min()) }",
            kata_kunci: &["tidak bisa dibandingkan"],
        },
        Kasus {
            nama: "max pada array campur tipe",
            src: "package main\npub fn main() { let a = [1, \"dua\", 3]; wok(a.max()) }",
            kata_kunci: &["tidak bisa dibandingkan"],
        },
        Kasus {
            // Dulu `_ => 0.0`: elemen non-angka dihitung sebagai nol, jadi
            // rata-ratanya angka yang kelihatan sah tapi salah total.
            nama: "mean pada array yang ada non-angkanya",
            src: "package main\npub fn main() { wok([1, 2, \"oops\", 4].mean()) }",
            kata_kunci: &["harus angka"],
        },
        Kasus {
            nama: "median pada array yang ada non-angkanya",
            src: "package main\npub fn main() { wok([1, \"x\"].median()) }",
            kata_kunci: &["harus angka"],
        },
    ]);
}

// ── Closure ────────────────────────────────────────────────────────────

#[test]
fn kesalahan_seputar_closure_dilaporkan_jelas() {
    periksa(&[
        Kasus {
            nama: "jumlah argumen closure salah",
            src: "package main\npub fn main() { let f = |a, b| a + b; wok(f(1)) }",
            kata_kunci: &["closure", "arg"],
        },
        Kasus {
            // Nama yang tidak ada di mana pun tetap gagal di titik
            // pemakaian, bukan diam-diam jadi `nil` waktu ditangkap.
            nama: "closure menyebut nama yang tidak ada",
            src: "package main\npub fn main() { let f = || tidakada; wok(f()) }",
            kata_kunci: &["tidakada", "belum dikenal"],
        },
        Kasus {
            nama: "memanggil hasil tangkapan yang bukan fungsi",
            src: "package main\npub fn main() { var x = 5; let f = || x(); wok(f()) }",
            kata_kunci: &["fungsi"],
        },
    ]);
}

// ── Memori manual (`jawa::` / `jawir::`) ───────────────────────────────

#[test]
fn pelanggaran_memori_manual_dilaporkan_bukan_diam() {
    // Fitur ini dulu bernama `new::`/`delete::`; penggantiannya semata di
    // nama permukaan. Semua pemeriksaan keamanannya harus tetap sama.
    periksa(&[
        Kasus {
            nama: "use-after-free",
            src: "package main\npub fn main() { \
                  let p = jawa::alloc(1); jawir::free(p); wok(jawa::get(p)) }",
            kata_kunci: &["use-after-free"],
        },
        Kasus {
            nama: "double free",
            src: "package main\npub fn main() { \
                  let p = jawa::alloc(1); jawir::free(p); jawir::free(p) }",
            kata_kunci: &["double free"],
        },
        Kasus {
            nama: "use-after-free lewat SALINAN handle",
            src: "package main\npub fn main() { \
                  let a = jawa::alloc(1); let b = a; jawir::free(b); wok(jawa::get(a)) }",
            kata_kunci: &["use-after-free"],
        },
        Kasus {
            nama: "array manual di luar jangkauan",
            src: "package main\npub fn main() { let a = jawa::array(3); wok(jawa::array_get(a, 99)) }",
            kata_kunci: &["di luar batas", "panjang"],
        },
        Kasus {
            nama: "byte buffer di luar 0..255",
            src: "package main\npub fn main() { let b = jawa::buffer(4); jawa::buffer_set(b, 0, 256) }",
            kata_kunci: &["byte"],
        },
        Kasus {
            nama: "jawir pada nilai yang bukan handle",
            src: "package main\npub fn main() { let x = 5; jawir::free(x) }",
            kata_kunci: &["jawir::free"],
        },
        Kasus {
            // Nama lama harus memberi petunjuk pindah-nama, bukan cuma
            // "tidak ditemukan" yang tidak menjelaskan apa pun.
            nama: "nama lama new:: memberi petunjuk",
            src: "package main\npub fn main() { let p = new::alloc(1); wok(p) }",
            kata_kunci: &["jawa::alloc", "namanya sekarang"],
        },
        Kasus {
            nama: "nama lama delete:: memberi petunjuk",
            src: "package main\npub fn main() { delete::free(1) }",
            kata_kunci: &["jawir::free", "namanya sekarang"],
        },
    ]);
}

// ── Jumlah argumen ─────────────────────────────────────────────────────

#[test]
fn jumlah_argumen_yang_salah_ditolak() {
    periksa(&[
        Kasus {
            nama: "kurang argumen",
            src: "package main\nfn tambah(a: int, b: int) -> int { return a + b }\n\
                  pub fn main() { wok(tambah(1)) }",
            kata_kunci: &["tambah", "argumen"],
        },
        Kasus {
            nama: "kelebihan argumen",
            src: "package main\nfn tambah(a: int, b: int) -> int { return a + b }\n\
                  pub fn main() { wok(tambah(1, 2, 3)) }",
            kata_kunci: &["tambah", "argumen", "a, b"],
        },
    ]);
}

/// Perbaikan Phase 5F-F: hal-hal yang dulu SUKSES diam-diam padahal
/// seharusnya gagal, atau yang dulu diam-diam tidak berbuat apa-apa.
#[test]
fn temuan_audit_pondasi_5f_f() {
    periksa(&[
        Kasus {
            // `?` bekerja dengan kembali lebih awal.
            nama: "`?` yang gagal di dalam main tidak boleh diam",
            src: "package main\nfn g() { return Err(\"gagal\") }\npub fn main() { let v = g()?; wok(v) }",
            kata_kunci: &["merambat keluar", "main"],
        },
        Kasus {
            nama: "None yang merambat keluar main juga dilaporkan",
            src: "package main\nfn g() { return None }\npub fn main() { let v = g()?; wok(v) }",
            kata_kunci: &["merambat keluar", "main"],
        },
        Kasus {
            // Sasaran cast yang tidak dikenal dulu mengembalikan nilainya
            // apa adanya, jadi `5 as P` "berhasil" dan menghasilkan 5.
            nama: "cast ke tipe struct ditolak, bukan jadi operasi kosong",
            src: "package main\nstruct P { pub x: int = 0 }\npub fn main() { wok(5 as P) }",
            kata_kunci: &["cast", "tidak didukung"],
        },
        Kasus {
            nama: "cast ke tipe array ditolak",
            src: "package main\npub fn main() { wok(5 as []int) }",
            kata_kunci: &["cast", "tidak didukung"],
        },
        Kasus {
            // Dulu `None` adalah pola pengikat yang cocok dengan apa saja,
            // jadi match ini "berhasil" dan mengembalikan 111.
            nama: "None tidak lagi menelan Some, jadi match ini tidak exhaustive",
            src: "package main\npub fn main() { let r = match Some(1) { None => 111 }; wok(r) }",
            kata_kunci: &["exhaustive"],
        },
    ]);
}

/// Sisi positif dari perbaikan yang sama: yang dulu SALAH DIAM sekarang
/// harus menghasilkan jawaban yang benar (bukan sekadar gagal).
#[test]
fn temuan_audit_pondasi_5f_f_sisi_positif() {
    fn jalan(src: &'static str) -> Result<(), String> {
        match run_on_big_stack(src) {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }
    // `as float` dulu tidak mengubah apa-apa: `3 as float` tetap int, dan
    // `(3 as float) / 2` menjawab 1 (pembagian integer) alih-alih 1,5.
    jalan(
        "package main\npub fn main() {\n\
         assert_eq(typeof(3 as float), \"float\")\n\
         assert_eq((3 as float) / 2, 1.5)\n\
         }",
    )
    .expect("`as float` harus benar-benar mengubah tipe");

    // `len()` dulu menolak dict, padahal `d.len()` dan `dict::len(d)` bisa.
    jalan(
        "package main\npub fn main() {\n\
         let d = {\"a\": 1, \"b\": 2}\n\
         assert_eq(len(d), 2)\n\
         assert_eq(len(d), d.len())\n\
         }",
    )
    .expect("`len()` harus bisa mengukur dict");

    // `None` sebagai pola tidak boleh menelan `Some`.
    jalan(
        "package main\npub fn main() {\n\
         assert_eq(match Some(1) { None => \"salah\", Some(x) => \"benar\" }, \"benar\")\n\
         assert_eq(match None { None => \"benar\", Some(x) => \"salah\" }, \"benar\")\n\
         }",
    )
    .expect("pola None harus menguji varian, bukan mengikat");

    // Nilai unit bukan nil.
    jalan(
        "package main\nfn kosong() { let x = 1 }\npub fn main() {\n\
         assert_eq(typeof(kosong()), \"unit\")\n\
         assert(kosong() != nil)\n\
         }",
    )
    .expect("unit harus berbeda dari nil");
}

/// Sisi NEGATIF fitur yang baru didukung VM di Phase 5F-G.
#[test]
fn kasus_negatif_fitur_baru_5f_g() {
    periksa(&[
        Kasus {
            nama: "label yang tidak ada ditolak, bukan jadi break biasa",
            src: "package main\npub fn main() { for i in 0..3 { break 'tidak_ada }; wok(1) }",
            kata_kunci: &["label", "tidak_ada"],
        },
        Kasus {
            nama: "continue berlabel yang tidak membungkus juga ditolak",
            src: "package main\npub fn main() { 'a: for i in 0..2 { }; for j in 0..2 { continue 'a }; wok(1) }",
            kata_kunci: &["label", "a"],
        },
        Kasus {
            // Fungsi bersarang BUKAN closure: ia tidak melihat lokal
            // pembungkusnya. Kalau suatu saat ia diam-diam jadi closure,
            // kasus ini yang menangkapnya.
            nama: "fn bersarang tidak melihat lokal pembungkusnya",
            src: "package main\nfn luar() { let x = 10; fn dalam() { return x }; return dalam() }\npub fn main() { wok(luar()) }",
            kata_kunci: &["x"],
        },
        Kasus {
            nama: "kelebihan argumen ditolak walau ada nilai default",
            src: "package main\nfn f(a, b = 10) { return a + b }\npub fn main() { wok(f(1,2,3)) }",
            kata_kunci: &["kebanyakan argumen"],
        },
        Kasus {
            nama: "parameter wajib yang kosong ditolak dan disebut namanya",
            src: "package main\nfn f(a, b = 10) { return a + b }\npub fn main() { wok(f()) }",
            kata_kunci: &["parameter", "wajib diisi"],
        },
        Kasus {
            nama: "`defer` harus berupa pemanggilan",
            src: "package main\npub fn main() { var n = 1; defer n }",
            kata_kunci: &["defer"],
        },
        Kasus {
            nama: "use-after-free dilaporkan",
            src: "package main\npub fn main() { let p = jawa::alloc(1); let _ = jawir::free(p); wok(jawa::get(p)) }",
            kata_kunci: &["use-after-free"],
        },
        Kasus {
            nama: "double free dilaporkan",
            src: "package main\npub fn main() { let p = jawa::alloc(1); let _ = jawir::free(p); let _ = jawir::free(p) }",
            kata_kunci: &["double free"],
        },
        Kasus {
            nama: "destructure yang tidak cocok ditolak, bukan mengikat seadanya",
            src: "package main\npub fn main() { let [a, b] = [1,2,3]; wok(a) }",
            kata_kunci: &["tidak cocok dengan polanya"],
        },
        Kasus {
            nama: "index array di luar batas lewat jawa:: juga ditolak",
            src: "package main\npub fn main() { let a = jawa::array(2, 0); wok(jawa::array_get(a, 9)) }",
            kata_kunci: &["di luar batas", "panjang"],
        },
        Kasus {
            nama: "byte di luar 0..255 ditolak, bukan dipotong",
            src: "package main\npub fn main() { let b = jawa::buffer(2); let _ = jawa::buffer_set(b, 0, 300) }",
            kata_kunci: &["byte"],
        },
        Kasus {
            nama: "literal integer di luar 128 bit ditolak",
            src: "package main\npub fn main() { wok(340282366920938463463374607431768211456) }",
            kata_kunci: &["angka bulat", "tidak sah"],
        },
    ]);
}

/// Batas FFI: yang TIDAK boleh menyeberang harus ditolak dengan pesan yang
/// menyebut sebabnya, dan tidak boleh bikin proses mati.
#[test]
fn batas_ffi_ditolak_dengan_jujur() {
    fn hasil(src: &'static str) -> String {
        // Kegagalan FFI yang WAJAR berupa `Result::Err` (nilai), bukan error
        // runtime — jadi programnya sukses dan pesannya ada di stdout.
        // Yang diperiksa di sini isi pesannya.
        match run_on_big_stack(src) {
            Some(e) => e,
            None => String::new(),
        }
    }
    // Deklarasi yang tidak didukung tidak boleh menghentikan program saat
    // dideklarasikan — errornya muncul saat DIPANGGIL, sebagai Result::Err.
    assert!(
        hasil("package main\nextern \"C\" { fn strlen(s: []int) -> i64 }\npub fn main() { let _ = strlen([1,2]) }").is_empty(),
        "deklarasi extern yang tak didukung tidak boleh menggagalkan program"
    );

    // ABI selain "C" juga bukan error keras: fungsinya tetap ada, dan
    // pemanggilannya mengembalikan `Result::Err` yang menyebut ABI-nya.
    assert!(
        hasil("package main\nextern \"Go\" { fn f(n: i32) -> i32 }\npub fn main() { let _ = f(1) }").is_empty(),
        "ABI tak didukung tidak boleh menggagalkan program; errornya lewat Result"
    );

    periksa(&[
        Kasus {
            // `extern static` belum didukung; membacanya harus dijelaskan
            // apa adanya, bukan jadi "variabel belum didefinisikan" atau —
            // lebih buruk — diam-diam `nil`.
            nama: "extern static dijelaskan, bukan jadi nil",
            src: "package main\nextern \"C\" { static errno: i32 }\npub fn main() { wok(errno) }",
            kata_kunci: &["extern static", "belum didukung"],
        },
    ]);
}

/// Konstruksi yang JUJUR menolak, alih-alih menerima sesuatu yang tidak
/// pernah punya efek.
#[test]
fn bound_negatif_ditolak_bukan_diterima_diam_diam() {
    periksa(&[
        Kasus {
            nama: "bound negatif `?Sized`",
            src: "package main\nfn f<T: ?Sized>(x: T) { wok(x) }\npub fn main() { f(1) }",
            kata_kunci: &["bound negatif", "belum menegakkan"],
        },
        Kasus {
            nama: "bound negatif ganda `??Sized`",
            src: "package main\nfn f<T: ??Sized>(x: T) { wok(x) }\npub fn main() { f(1) }",
            kata_kunci: &["bound negatif", "belum menegakkan"],
        },
        Kasus {
            nama: "bound negatif di struct generik",
            src: "package main\nstruct B<T: ?Sized> { pub v: int }\npub fn main() { let b = B { v: 1 }; wok(b.v) }",
            kata_kunci: &["bound negatif"],
        },
    ]);

    // `impl` yang tidak menyediakan method ABSTRAK harus ditolak, dan ditolak di
    // tempat yang benar: pada `impl`-nya, saat parse — bukan nanti di tempat …
    periksa(&[
        Kasus {
            nama: "impl tidak menyediakan method abstrak",
            src: "package main\ntrait S { fn wajib(self) -> string }\nstruct K { pub n: int }\nimpl S for K { }\npub fn main() { let k = K { n: 1 }; wok(k.wajib()) }",
            kata_kunci: &["belum menyediakan", "wajib"],
        },
        Kasus {
            nama: "impl menyediakan sebagian saja",
            src: "package main\ntrait S { fn satu(self) -> string\n  fn dua(self) -> string }\nstruct K { pub n: int }\nimpl S for K { fn satu(self) -> string { return \"a\" } }\npub fn main() { let k = K { n: 1 }; wok(k.satu()) }",
            kata_kunci: &["belum menyediakan", "dua"],
        },
    ]);

    // Nama trait di bound harus benar-benar ada. Sebelum pemeriksaan ini,
    // apa pun lolos — termasuk salah ketik — jadi bound memberi KESAN ada
    // batasan padahal tidak ada yang pernah dicek.
    periksa(&[
        Kasus {
            nama: "bound menyebut trait yang tidak ada",
            src: "package main\nfn f<T: SamaSekaliTidakAda>(x: T) { wok(x) }\npub fn main() { f(1) }",
            kata_kunci: &["tidak ada", "belum menegakkan"],
        },
        Kasus {
            nama: "salah ketik nama trait dapat saran",
            src: "package main\ntrait Bentuk { fn luas(self) -> f64 { return 0.0 } }\nfn f<T: bentuk>(x: T) { wok(x) }\npub fn main() { f(1) }",
            kata_kunci: &["tidak ada", "maksudnya", "Bentuk"],
        },
    ]);

    // Sisi sebaliknya: bound yang namanya ADA tetap lolos.
    let biasa = run_on_big_stack(
        "package main\ntrait Bentuk { fn luas(self) -> f64 { return 0.0 } }\n\
         fn f<T: Bentuk>(x: T) { wok(x) }\npub fn main() { f(1) }",
    );
    assert!(
        biasa.is_none(),
        "bound yang traitnya ada tidak boleh ikut ditolak, tapi gagal: {:?}",
        biasa
    );

    // Dan berkas ber-`import` harus MUNDUR: trait dari modul lain tidak
    // kelihatan dari parser, jadi menuduhnya salah ketik akan jadi tuduhan
    // palsu. Lebih baik tidak memeriksa daripada menuduh yang salah.
    let dengan_import = run_on_big_stack(
        "package main\nimport \"./entah\"\nfn f<T: TraitDariModulLain>(x: T) { wok(x) }\npub fn main() { wok(1) }",
    );
    let pesan = dengan_import.unwrap_or_default();
    assert!(
        !pesan.contains("TraitDariModulLain"),
        "berkas ber-import tidak boleh dituduh soal nama trait, tapi pesannya: {}",
        pesan
    );
}

/// `TIPE_BAWAAN` harus memuat SETIAP struct yang dibuat interpreter sendiri.
#[test]
fn tipe_bawaan_lengkap() {
    use std::collections::BTreeSet;

    fn kumpul(dir: &std::path::Path, keluar: &mut BTreeSet<String>) {
        let Ok(isi) = std::fs::read_dir(dir) else { return };
        for e in isi.flatten() {
            let p = e.path();
            if p.is_dir() { kumpul(&p, keluar); continue; }
            if p.extension().and_then(|s| s.to_str()) != Some("rs") { continue; }
            let Ok(src) = std::fs::read_to_string(&p) else { continue };
            // KOMENTAR dibuang lebih dulu.
            let kode: String = src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for potongan in kode.split("Value::Struct").skip(1) {
                let kepala: String = potongan.chars().take(200).collect();
                if let Some(i) = kepala.find("name: \"") {
                    let sisa = &kepala[i + 7..];
                    if let Some(j) = sisa.find('"') {
                        let nama = &sisa[..j];
                        // Nama yang dibangun saat jalan (mis. dari sebuah
                        // variabel) tidak bisa dibaca dari sumber, dan nama
                        // huruf-kecil bukan tipe yang bisa ditulis di anotasi.
                        if nama.starts_with(|c: char| c.is_ascii_uppercase()) {
                            keluar.insert(nama.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut ditemukan = BTreeSet::new();
    kumpul(std::path::Path::new("src"), &mut ditemukan);
    assert!(
        !ditemukan.is_empty(),
        "tidak menemukan satu pun `Value::Struct {{ name: \"X\" }}` di `src/` — \
         pemindainya yang rusak, bukan kodenya"
    );

    let terdaftar: BTreeSet<&str> = jrgc::parser::TIPE_BAWAAN.iter().copied().collect();
    let hilang: Vec<&String> = ditemukan
        .iter()
        // Varian enum bawaan (`Ok`/`Err`/`Some`/`None`) hidup di ruang nama
        // VARIAN, bukan tipe, dan posisi pola memang tidak diperiksa.
        .filter(|n| !matches!(n.as_str(), "Ok" | "Err" | "Some" | "None" | "Option" | "Result"))
        .filter(|n| !terdaftar.contains(n.as_str()))
        .collect();

    assert!(
        hilang.is_empty(),
        "struct native ini dibuat interpreter tapi TIDAK ada di `TIPE_BAWAAN`: {:?}\n\
         Akibatnya `let x: {}` pada program yang benar akan dituduh tidak ada.\n\
         Tambahkan ke daftar di `src/parser/mod.rs`.",
        hilang,
        hilang.first().map(|s| s.as_str()).unwrap_or("X")
    );
}

// ════════════════════════════════════════════════════════════════════════
// Diagnostik tidak boleh membocorkan nama internal …

/// Program salah, dan ejaan yang HARUS muncul di pesannya.
const EJAAN_MUNCUL: &[(&str, &str)] = &[
    ("package main\nif true { }",                          "`if`"),
    ("package main\nfn 5() { }",                           "`5`"),
    ("package main\npub fn main() { let x = }",            "`}`"),
    ("package main\nfn f(x: 5) { }",                       "`5`"),
    ("package main\npub fn main() { match 1 { + => 1 } }", "`+`"),
    ("package main\nextern \"C\" { let x = 1 }",           "`let`"),
    ("package main\npub fn main() { match 1 { -x => 1 } }", "`x`"),
    ("package main\npub fn main() { if true  wok(1) }",    "`{`"),
    ("package main\npub fn main() { let a = [1]\n wok(a.+) }", "`+`"),
    ("package main\npub fn main() {\n  'lab: wok(1)\n}",   "`wok`"),
    // `PrimTy::String_` -> ejaan JRG-nya `string`, tanpa garis bawah.
    ("package main\nfn f<T: string>(x: T) -> T { return x }\npub fn main() { wok(f(1)) }", "`string`"),
];

/// Potongan yang TIDAK BOLEH muncul di diagnostik mana pun. Semuanya nama
/// dari dalam kompilator: varian enum, sintaks tuple-struct Rust, atau bentuk
/// `Debug` sebuah `String`.
const JEJAK_INTERNAL: &[&str] = &[
    "KwIf", "KwLet", "KwFor", "KwWhile", "KwFn", "KwElse", "KwMatch",
    "RBrace", "LBrace", "RParen", "LParen", "RBracket", "LBracket",
    "IntLit(", "FloatLit(", "StringLit(", "Ident(", "BoolLit(", "RuneLit(",
    "FatArrow", "ColonColon", "DotDotEq", "QuestionQuestion",
    "TokenKind", "PrimTy", "string_",
];

#[test]
fn diagnostik_tidak_membocorkan_nama_internal() {
    let mut gagal = Vec::new();
    for (src, ejaan) in EJAAN_MUNCUL {
        let Some(pesan) = run_expecting_error(src) else {
            gagal.push(format!("  program ini seharusnya DITOLAK, tapi lolos:\n    {}", src));
            continue;
        };
        if !pesan.contains(ejaan) {
            gagal.push(format!(
                "  pesan tidak menyebut {} — yang dilihat orangnya bukan yang ia tulis:\n    {}",
                ejaan,
                pesan.lines().next().unwrap_or("")
            ));
        }
        for jejak in JEJAK_INTERNAL {
            if pesan.contains(jejak) {
                gagal.push(format!(
                    "  pesan membocorkan `{}`, nama dari dalam kompilator:\n    {}",
                    jejak,
                    pesan.lines().next().unwrap_or("")
                ));
            }
        }
    }
    assert!(
        gagal.is_empty(),
        "{} masalah pada diagnostik parser.\n\
         Sebuah nama seperti `RBrace` atau `Ident(\"x\")` tidak pernah ditulis \
         pemakainya dan tidak ada di dokumentasi bahasanya — ia cuma bisa ditebak.\n{}",
        gagal.len(),
        gagal.join("\n")
    );
}

/// Panjang sebuah pesan galat tidak boleh tumbuh mengikuti UKURAN datanya.
#[test]
fn pesan_galat_tidak_tumbuh_mengikuti_ukuran_data() {
    let kecil = "package main\npub fn main() { let s = \"xxxxxxxxxx\"\n match s { 1 => wok(1) } }";
    let besar = format!(
        "package main\npub fn main() {{ let s = \"{}\"\n match s {{ 1 => wok(1) }} }}",
        "x".repeat(5000)
    );
    let pk = run_expecting_error(kecil).expect("program kecil harus ditolak");
    let pb = run_expecting_error(&besar).expect("program besar harus ditolak");

    assert!(
        pb.len() < pk.len() * 2,
        "pesan galat tumbuh mengikuti data: {} bita untuk nilai 10 karakter, \
         {} bita untuk 5000. Sebuah diagnostik tidak boleh berubah perilaku \
         mengikuti besar datanya.\npesan besar:\n{}",
        pk.len(), pb.len(), &pb[..pb.len().min(300)]
    );

    // Dan nilai yang wajar tetap dicetak utuh — memotong semuanya akan
    // menghilangkan bagian yang justru paling menolong.
    let angka = run_expecting_error("package main\npub fn main() { match 5 { 1 => wok(1) } }")
        .expect("harus ditolak");
    assert!(
        angka.contains("`5`"),
        "nilai kecil harus tetap tercetak utuh, bukan ikut dipotong:\n{}", angka
    );
}
