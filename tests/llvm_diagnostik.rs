// tests/llvm_diagnostik.rs — tests/llvm_diagnostik.rs — instrumen diagnostik backend LLVM.

use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;

/// Jalankan lex → parse → codegen LLVM, kembalikan daftar error terurai.
fn galat_llvm(src: &str) -> Vec<jrgc::llvm::galat::GalatLlvm> {
    let sf = SourceFile { id: 0, name: "uji.jrg".to_string(), src: src.to_string() };
    let tokens = lex(&sf).expect("program uji harus bisa di-lex");
    let (ast, perr) = parse(tokens, false);
    assert!(perr.is_empty(), "program uji harus bisa di-parse, dapat: {:?}", perr);

    match jrgc::llvm::generate_ir(&ast) {
        Ok(_) => Vec::new(),
        Err(errs) => errs.iter()
            .map(|e| jrgc::llvm::galat::GalatLlvm::dari_pesan(e))
            .collect(),
    }
}

/// Ubah span jadi (nomor baris 1-basis, teks yang tertunjuk).
fn baris_dan_kutipan(src: &str, sp: jrgc::lexer::Span) -> (usize, String) {
    let lo = sp.lo.0 as usize;
    let hi = sp.hi.0 as usize;
    assert!(hi <= src.len(), "span melewati ujung sumber: {}..{} dari {}", lo, hi, src.len());
    let baris = src[..lo].matches('\n').count() + 1;
    (baris, src[lo..hi].to_string())
}

/// Satu kasus: program, potongan pesan yang diharapkan, baris, dan teks tepat
/// yang harus tertunjuk caret.
struct Kasus {
    nama: &'static str,
    src: &'static str,
    pesan_memuat: &'static str,
    baris: usize,
    kutipan: &'static str,
}

/// Tabel utama. Tiap entri adalah satu KELAS error yang diukur keluar tanpa
/// lokasi sebelum perbaikan.
const LOKASI_TEPAT: &[Kasus] = &[
    // (Kasus "nilai yang akan dibuang ditunjuk pada return-nya" usang sejak Fase DUGA: tipe
    // kembalian `fn g(x: int) { return math::abs(x) }` sekarang dibaca dari penurunan badannya.)

    // Kasus intinya dulu `fn ubah(a)` tanpa anotasi, yang dilaporkan di baris
    // PEMANGGILNYA — 25 baris dari masalahnya.
    Kasus {
        nama: "index tuple non-konstan ditunjuk pada ekspresi indexnya",
        src: "package main\n\
              \n\
              pub fn main() {\n\
              \x20   var t: (int, string) = (1, \"a\")\n\
              \x20   var i: int = 0\n\
              \x20   wok(t[i])\n\
              }\n",
        pesan_memuat: "sudah pasti saat kompilasi",
        baris: 6,
        kutipan: "t[i]",
    },

    // Namespace stdlib: pesannya harus mengakui fungsinya NYATA dan menyebut
    // backend mana yang belum mengimplementasikannya — bukan menuduh salah ketik …
    Kasus {
        nama: "namespace stdlib ditunjuk pada pemanggilannya",
        src: "package main\n\
              \n\
              pub fn main() {\n\
              \x20   var r = py::eval(\"1\")\n\
              \x20   wok(r)\n\
              }\n",
        pesan_memuat: "namespace `py::`",
        baris: 4,
        kutipan: "py::eval",
    },
    // Closure dan `defer` dulu ada di sini dan dua-duanya sekarang jalan.
    // Di sini dulu berdiri dua kasus berturut-turut, dan KEDUANYA dicabut oleh
    // tabel ini sendiri — yang keempat dan kelima di berkas ini.
    Kasus {
        nama: "penugasan ditunjuk pada penugasannya, bukan pada kurung tutup",
        src: "package main\n\
              \n\
              pub fn main() {\n\
              \x20   var a: [1]int = [0]\n\
              \x20   for i in 0..2 { a = a + [i] }\n\
              \x20   wok(len(a))\n\
              }\n",
        pesan_memuat: "sebagai `[1]int`",
        baris: 5,
        kutipan: "a = a + [i]",
    },
    // Di sini dulu ada dua kasus berturut-turut, dan KEDUANYA dicabut oleh tes
    // ini sendiri dengan pesan "kasusnya sudah usang?" — persis gunanya.

    // Situs yang MENGUMPULKAN error alih-alih mengembalikan `Err`, jadi ia
    // melewati pembungkus `map_err` sama sekali.
    Kasus {
        nama: "koersi tipe yang mengumpulkan error tetap dapat lokasi",
        src: "package main\n\
              \n\
              fn kosong() {}\n\
              \n\
              pub fn main() {\n\
              \x20   var x: int = kosong()\n\
              \x20   wok(x)\n\
              }\n",
        pesan_memuat: "sebagai `int`",
        baris: 6,
        kutipan: "kosong()",
    },
    // Dua kasus "mencetak nilai fungsi" dulu di sini; sejak Fase SISA fungsi tampil seperti acuan.
    // Kasus "spawn ditunjuk pada statement-nya" dulu di sini; sejak Fase KORUTIN `spawn` jalan.
];

#[test]
fn lokasi_diagnostik_llvm_tepat_bukan_hampir() {
    let mut gagal = Vec::new();

    for k in LOKASI_TEPAT {
        let galat = galat_llvm(k.src);
        if galat.is_empty() {
            gagal.push(format!("[{}] tidak ada error sama sekali — kasusnya sudah usang?", k.nama));
            continue;
        }

        // Ambil error yang cocok pesannya, bukan sekadar yang pertama: satu
        // kompilasi bisa gagal di beberapa tempat, dan mengunci "yang
        // pertama" akan retak setiap kali urutannya bergeser.
        let Some(g) = galat.iter().find(|g| g.msg.contains(k.pesan_memuat)) else {
            gagal.push(format!(
                "[{}] tidak ada error yang memuat `{}`; yang ada: {:?}",
                k.nama, k.pesan_memuat,
                galat.iter().map(|g| g.msg.lines().next().unwrap_or("").to_string()).collect::<Vec<_>>()
            ));
            continue;
        };

        let Some(sp) = g.span else {
            gagal.push(format!("[{}] error tanpa lokasi: {}", k.nama, g.msg.lines().next().unwrap_or("")));
            continue;
        };

        let (baris, kutipan) = baris_dan_kutipan(k.src, sp);
        if baris != k.baris || kutipan != k.kutipan {
            gagal.push(format!(
                "[{}] lokasi salah\n     diharap: baris {} menunjuk `{}`\n     dapat  : baris {} menunjuk `{}`",
                k.nama, k.baris, k.kutipan, baris, kutipan
            ));
        }
    }

    assert!(
        gagal.is_empty(),
        "diagnostik LLVM menunjuk tempat yang salah pada {} kasus:\n\n{}\n\n\
         Caret yang rapi tapi menunjuk baris yang salah lebih menyesatkan\n\
         daripada tidak ada caret — lihat catatan di kepala berkas ini.",
        gagal.len(),
        gagal.join("\n")
    );
}

/// Kelas error yang memang TIDAK punya lokasi, beserta alasannya.
const TANPA_LOKASI_WAJAR: &[(&str, &str, &str)] = &[
    (
        "tidak ada main",
        "package main\n\nfn bantu() -> int { return 1 }\n",
        "fungsi `main` tidak ditemukan",
    ),
];

#[test]
fn error_tanpa_lokasi_hanya_yang_memang_tak_punya_tempat() {
    for (nama, src, memuat) in TANPA_LOKASI_WAJAR {
        let galat = galat_llvm(src);
        let g = galat.iter().find(|g| g.msg.contains(memuat))
            .unwrap_or_else(|| panic!(
                "[{}] error `{}` tidak muncul; yang ada: {:?}",
                nama, memuat,
                galat.iter().map(|g| g.msg.clone()).collect::<Vec<_>>()
            ));
        assert!(
            g.span.is_none(),
            "[{}] sekarang PUNYA lokasi — bagus, tapi pindahkan kasusnya ke \
             `LOKASI_TEPAT` supaya ketepatannya ikut dikunci",
            nama
        );
    }
}

/// Penanda `[lo:hi]` tidak boleh bocor ke pesan yang dibaca orang.
#[test]
fn penanda_lokasi_tidak_bocor_ke_pesan() {
    let mut bocor = Vec::new();
    for k in LOKASI_TEPAT {
        for g in galat_llvm(k.src) {
            if g.msg.starts_with('[') && g.msg[1..].split(']').next()
                .map(|d| d.split_once(':').is_some_and(|(a, b)|
                    a.trim().parse::<u32>().is_ok() && b.trim().parse::<u32>().is_ok()))
                .unwrap_or(false)
            {
                bocor.push(format!("[{}] {}", k.nama, g.msg));
            }
        }
    }
    assert!(bocor.is_empty(), "penanda lokasi bocor ke pesan:\n{}", bocor.join("\n"));
}

// ════════════════════════════════════════════════════════════════════════
// Sebab, bukan gejala …

/// Program gagal, lalu kata yang HARUS ada dan yang TIDAK BOLEH sendirian.
const SEBAB_DISEBUT: &[(&str, &str, &str)] = &[
    // Programnya diganti, dan alasannya bagian dari kasusnya.
    (
        "var [] tak tersimpulkan dipakai sebagai string",
        "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\n\
         pub fn main() { catat(bikin())\n assert_eq(jejak, [\"a\"]) }\n\
         fn bikin() { return \"a\" }\n",
        "`jejak` dideklarasikan sebagai array kosong",
    ),
];

#[test]
fn galat_menyebut_variabel_penyebab_bukan_cuma_gejalanya() {
    for (nama, src, sebab) in SEBAB_DISEBUT {
        let galat = galat_llvm(src);
        assert!(!galat.is_empty(), "[{}] harusnya gagal, tapi lolos", nama);
        let semua: Vec<String> = galat.iter().map(|g| g.msg.clone()).collect();
        assert!(
            semua.iter().any(|m| m.contains(sebab)),
            "[{}] tidak ada pesan yang menyebut sebabnya (`{}`). Yang ada cuma gejala:\n{}",
            nama, sebab, semua.join("\n---\n")
        );
        // Sebabnya harus DI DEPAN: gejalanya bisa muncul berkali-kali, dan
        // pembaca berhenti di baris pertama.
        assert!(
            semua[0].contains(sebab),
            "[{}] sebabnya ada tapi tidak di urutan pertama — yang pertama: {}",
            nama, semua[0]
        );
        assert!(
            semua[0].contains("bantuan:"),
            "[{}] sebabnya disebut tanpa jalan keluar: {}", nama, semua[0]
        );
    }
}

/// Yang TIDAK boleh ikut berubah: sebuah `var []` yang memang dipakai sebagai
/// int harus tetap terkompilasi. Bawaan `int` itu benar untuk program begitu,
/// dan menolaknya akan menukar satu masalah dengan yang lebih besar.
#[test]
fn var_kosong_yang_memang_int_tetap_jalan() {
    for (nama, src) in [
        ("langsung", "package main\nvar xs = []\npub fn main() { xs = xs + [1]\n wok(xs) }\n"),
        ("lewat fungsi", "package main\nvar xs = []\nfn tambah(n) { xs = arr::push(xs, n) }\n\
                          pub fn main() { tambah(7)\n wok(xs) }\n"),
        ("beranotasi", "package main\nvar xs: []string = []\nfn tambah(s) { xs = arr::push(xs, s) }\n\
                        pub fn main() { tambah(\"a\")\n wok(xs) }\n"),
    ] {
        let galat = galat_llvm(src);
        assert!(
            galat.is_empty(),
            "[{}] harusnya terkompilasi, tapi ditolak: {:?}",
            nama,
            galat.iter().map(|g| g.msg.clone()).collect::<Vec<_>>()
        );
    }
}


// ── `ns::x` dari sebuah `import`: sebut sebabnya, dan berhenti di situ ───

/// Program, potongan yang HARUS ada, potongan yang TIDAK BOLEH ada.
const PATH_HILANG: &[(&str, &str, &str, &str)] = &[
    // Sejak Fase MODUL-LLVM modulnya benar-benar DICARI dan dimuat: berkas yang tidak ada dan
    // anggota yang tidak ada dijawab dengan kalimat dua mesin lain.
    (
        "modul yang di-import tidak ada berkasnya",
        "package main\nimport \"bantu\"\npub fn main() { wok(bantu::dua()) }\n",
        "modul 'bantu' tidak ketemu",
        "periksa ejaan",
    ),
    (
        "anggota yang tidak ada di modul std yang dimuat",
        "package main\nimport \"std/path\"\npub fn main() { wok(path::tidak_ada(\"a\")) }\n",
        "`path::tidak_ada` tidak ditemukan",
        "tidak memuat modul",
    ),
    (
        "namespace TIDAK di-import: tetap menawar",
        "package main\npub fn main() { wok(hantu::x()) }\n",
        "periksa ejaan",
        "tidak memuat modul antar-berkas",
    ),
    (
        "fungsi stdlib nyata: penjelasan stdlib, bukan modul",
        "package main\npub fn main() { wok(dotnet::fungsi(\"a\", \"b\", \"c\", [], \"int\")) }\n",
        "ada di pustaka standar",
        "modul antar-berkas",
    ),
];

#[test]
fn path_hilang_menyebut_sebab_yang_benar_per_kelas() {
    for (nama, src, harus, jangan) in PATH_HILANG {
        let galat = galat_llvm(src);
        assert!(!galat.is_empty(), "[{}] harusnya gagal, tapi lolos", nama);
        let semua = galat.iter().map(|g| g.msg.clone()).collect::<Vec<_>>().join("\n---\n");
        assert!(
            semua.contains(harus),
            "[{}] pesannya tidak menyebut `{}`:\n{}", nama, harus, semua
        );
        assert!(
            !semua.contains(jangan),
            "[{}] pesannya masih memuat `{}`, yang salah untuk kelas ini:\n{}",
            nama, jangan, semua
        );
    }
}

/// Sejak Fase MODUL-LLVM backend ini MEMBACA modulnya, jadi nama yang salah di dalam modul
/// yang dimuat dipastikan tidak ada — dan nama privat dijelaskan, bukan dituduh salah ketik.
#[test]
fn anggota_modul_yang_salah_dijawab_pasti() {
    let pesan = |src: &str| galat_llvm(src).iter().map(|g| g.msg.clone()).collect::<Vec<_>>().join("\n");
    let salah = pesan("package main\nimport \"std/path\"\npub fn main() { wok(path::nama_yang_salah()) }\n");
    assert!(salah.contains("`path::nama_yang_salah` tidak ditemukan"), "anggota yang tidak ada:\n{}", salah);
    assert!(!salah.contains("belum diperiksa"), "kalimat lama tidak berlaku lagi:\n{}", salah);
}
