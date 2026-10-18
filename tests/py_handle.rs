// tests/py_handle.rs

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

/// Apakah `python3` ada dan bisa dijalankan?
fn ada_python() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct Hasil {
    keluaran: String,
    sukses: bool,
}

fn jalankan(src: &str) -> Hasil {
    let dir = crate::bersama::dir_unik("jrg-pyhandle");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let out = Command::new(jrgc()).arg(&file).arg("--sopan").output().expect("jalankan jrgc");
    let vm = Command::new(jrgc()).arg(&file).arg("--sopan").arg("--vm").output().expect("jalankan jrgc --vm");
    let _ = std::fs::remove_dir_all(&dir);
    let gabung = |o: &std::process::Output| format!(
        "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    crate::bersama::dua_mesin_sama(src, &gabung(&out), &gabung(&vm));
    Hasil { keluaran: gabung(&out), sukses: out.status.success() }
}

/// Bungkus badan program dalam fungsi ber-`Result` supaya `?` bisa dipakai
/// — tiap operasi `py::` fallible menurut desainnya, jadi kode uji di sini
/// terlihat seperti kode JRG yang sebenarnya.
fn program(badan: &str) -> String {
    format!(
        "package main\nfn coba() {{\n{}\n    return Ok(0)\n}}\n\
         pub fn main() {{ match coba() {{ Ok(_) => wok(\"SELESAI\"), \
         Err(e) => wok(\"GAGAL:\", e) }} }}\n",
        badan
    )
}

// ---------------------------------------------------------------------------

/// Objek Python yang tidak punya padanan di JRG harus jadi HANDLE.
#[test]
fn objek_tak_terkonversi_jadi_handle_bukan_string() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let f = py::eval(\"open('/dev/null')\")?\n\
         \x20   wok(\"TIPE:\", typeof(f))",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("TIPE: py:_io.TextIOWrapper"),
        "objek file harus jadi handle dengan tipe Python-nya, bukan string.\n\
         Kalau di sini terbaca `string`, artinya fallback `repr()` yang lama \
         sudah kembali dan `py::eval` diam-diam berbohong.\naktual:\n{}",
        h.keluaran
    );
}

/// "argumen ke-N" menghitung argumen yang DITERUSKAN, bukan posisinya di
/// `py::call(modul, fungsi, ...)`, jadi kalimatnya harus mengatakan itu.
#[test]
fn nomor_argumen_menyebut_yang_diteruskan() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan("let c = Chan::new(1)\nwok(py::call(\"math\", \"hypot\", 1, c))\n");
    assert!(h.keluaran.contains("argumen ke-2 yang diteruskan ke Python bertipe `channel`"),
        "nomor argumen harus menyebut bahwa ia dihitung dari yang diteruskan:\n{}", h.keluaran);
}

/// Memaksa objek yang memang tak punya padanan harus GAGAL dengan sebab
/// yang jelas — bukan memberi teks yang tampak seperti nilai.
#[test]
fn memaksa_objek_tak_terkonversi_ditolak_dengan_sebab() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let f = py::eval(\"open('/dev/null')\")?\n\
         \x20   match py::to_jrg(f) {\n\
         \x20       Ok(v)  => wok(\"TAK TERDUGA:\", v),\n\
         \x20       Err(e) => wok(\"DITOLAK:\", e),\n\
         \x20   }",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("DITOLAK:"), "aktual:\n{}", h.keluaran);
    for kunci in ["tidak punya padanan", "TextIOWrapper"] {
        assert!(
            h.keluaran.contains(kunci),
            "penolakan harus menyebut `{}` supaya pemakainya tahu APA yang \
             tidak bisa menyeberang.\naktual:\n{}",
            kunci, h.keluaran
        );
    }
}

/// `py::ref` menahan objeknya di Python walau sebenarnya bisa dikonversi,
/// dan methodnya tetap bisa dipanggil dari JRG.
#[test]
fn ref_menahan_objek_di_python_dan_methodnya_tetap_jalan() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let a = py::ref(\"[1, 2, 3, 2, 2]\")?\n\
         \x20   wok(\"TIPE:\", typeof(a))\n\
         \x20   wok(\"HITUNG:\", py::method(a, \"count\", 2)?)\n\
         \x20   wok(\"NILAI:\", py::to_jrg(a)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("TIPE: py:list"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("HITUNG: 3"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("NILAI: [1, 2, 3, 2, 2]"), "aktual:\n{}", h.keluaran);
}

/// Method dipanggil LANGSUNG di atas handle (`s.upper()`), bukan lewat
/// `py::method(s, "upper")`. Inilah beda "bisa dipakai" dan "mudah
/// dipakai", jadi ia dikunci sendiri.
#[test]
fn method_dipanggil_langsung_di_atas_handle() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let s = py::ref(\"'halo dunia'\")?\n\
         \x20   wok(\"BESAR:\", s.upper()?)\n\
         \x20   wok(\"PECAH:\", s.split(\" \")?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("BESAR: HALO DUNIA"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("PECAH: [halo, dunia]"), "aktual:\n{}", h.keluaran);
}

/// Handle harus bisa dipakai sebagai ARGUMEN pemanggilan Python lain.
#[test]
fn handle_bisa_jadi_argumen_pemanggilan_python() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let a = py::ref(\"[1, 2, 3, 4]\")?\n\
         \x20   wok(\"JUMLAH:\", py::call(\"builtins\", \"sum\", a)?)\n\
         \x20   wok(\"TERBESAR:\", py::call(\"builtins\", \"max\", a)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("JUMLAH: 10"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("TERBESAR: 4"), "aktual:\n{}", h.keluaran);
}

/// Objeknya dilepas begitu pemegang JRG terakhirnya mati — tanpa `py::free`
/// yang wajib diingat.
#[test]
fn handle_dilepas_otomatis_saat_pemegang_terakhir_mati() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    buat_lalu_buang()?\n\
         \x20   // `py::refs()` sekaligus menyiram antrean pelepasan, jadi\n\
         \x20   // angkanya mencerminkan keadaan SESUDAH yang mati dibereskan.\n\
         \x20   wok(\"SISA:\", py::refs()?)",
    ).replace(
        "package main\n",
        "package main\nfn buat_lalu_buang() { let s = py::ref(\"list(range(1000))\")?; \
         wok(\"HIDUP:\", py::method(s, \"__len__\")?); return Ok(0) }\n",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("HIDUP: 1000"), "aktual:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("SISA: 0"),
        "objek harus sudah dilepas sesudah pemegangnya mati — kalau di sini \
         terbaca angka selain 0, `Drop` tidak lagi mengantre pelepasannya \
         atau antreannya tidak pernah disiram.\naktual:\n{}",
        h.keluaran
    );
}

/// Dua nama yang memegang objek Python yang SAMA harus dianggap sama; dua
/// objek berbeda tidak.
#[test]
fn handle_yang_sama_dianggap_sama_yang_beda_tidak() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let a = py::ref(\"[1, 2]\")?\n\
         \x20   let b = a\n\
         \x20   let c = py::ref(\"[1, 2]\")?\n\
         \x20   wok(\"SALINAN:\", a == b)\n\
         \x20   wok(\"BEDA:\", a == c)\n\
         \x20   wok(\"BEDA_NE:\", a != c)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("SALINAN: true"),
        "dua nama yang memegang objek Python yang sama harus dianggap sama.\n\
         Kalau di sini `false`, `val_eq` kembali menjatuhkan handle ke \
         cabang penampung terakhirnya.\naktual:\n{}",
        h.keluaran
    );
    assert!(h.keluaran.contains("BEDA: false"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("BEDA_NE: true"), "aktual:\n{}", h.keluaran);
}

/// Handle tidak bisa diurutkan atau dibandingkan besar-kecil, dan itu harus
/// GAGAL — bukan diam-diam dianggap sama.
#[test]
fn handle_tidak_bisa_diurutkan_dan_itu_gagal_keras() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let a = py::ref(\"[1]\")?\n\
         \x20   let b = py::ref(\"[2]\")?\n\
         \x20   wok(a < b)",
    ));
    assert!(
        !h.sukses,
        "membandingkan besar-kecil dua handle harus gagal, bukan dijawab:\n{}",
        h.keluaran
    );
    assert!(
        h.keluaran.contains("tidak bisa membandingkan"),
        "pesannya harus menyebut bahwa keduanya tidak bisa dibandingkan.\naktual:\n{}",
        h.keluaran
    );
}

/// Handle dari sesi worker yang sudah berganti harus DITOLAK.
#[test]
fn handle_dari_sesi_worker_lama_ditolak() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let a = py::ref(\"[1, 2, 3]\")?\n\
         \x20   wok(\"SEBELUM:\", py::to_jrg(a)?)\n\
         \x20   let _ = py::exec(\"import os; os._exit(1)\")\n\
         \x20   wok(\"BARU:\", py::eval(\"1 + 1\")?)\n\
         \x20   match py::to_jrg(a) {\n\
         \x20       Ok(v)  => wok(\"BAHAYA:\", v),\n\
         \x20       Err(e) => wok(\"DITOLAK:\", e),\n\
         \x20   }",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("SEBELUM: [1, 2, 3]"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("BARU: 2"), "worker harus di-respawn:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("DITOLAK:"),
        "handle dari sesi lama HARUS ditolak. Kalau di sini terbaca `BAHAYA`, \
         nomor sesi tidak lagi diperiksa dan handle basi menunjuk objek yang \
         kebetulan bernomor sama.\naktual:\n{}",
        h.keluaran
    );
    assert!(h.keluaran.contains("sesi worker sebelumnya"), "aktual:\n{}", h.keluaran);
}

/// Operasi handle yang diberi nilai BIASA harus menolak dengan jelas,
/// bukan mencoba menebak maksudnya.
#[test]
fn operasi_handle_menolak_nilai_biasa() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program("    wok(py::to_jrg(42))"));
    assert!(
        !h.sukses,
        "memberi nilai biasa ke operasi handle harus gagal, bukan diterima:\n{}",
        h.keluaran
    );
    for kunci in ["py::to_jrg", "handle Python"] {
        assert!(
            h.keluaran.contains(kunci),
            "pesannya harus menyebut `{}`.\naktual:\n{}",
            kunci, h.keluaran
        );
    }
}

/// numpy dan pandas TIDAK boleh diimpor saat worker start.
#[test]
fn numpy_dan_pandas_tidak_diimpor_saat_worker_start() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let cek = \"__import__('sys').modules.get('%s') is not None\"\n\
         \x20   wok(\"AWAL_NUMPY:\", py::eval(str::replace(cek, \"%s\", \"numpy\"))?)\n\
         \x20   wok(\"AWAL_PANDAS:\", py::eval(str::replace(cek, \"%s\", \"pandas\"))?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("AWAL_NUMPY: false"),
        "numpy tidak boleh sudah terimpor sebelum ada yang memintanya — \
         kalau di sini `true`, impor eager-nya sudah kembali dan tiap program \
         `py::` kembali membayar ~0,17 detik yang tidak dipakainya.\naktual:\n{}",
        h.keluaran
    );
    assert!(
        h.keluaran.contains("AWAL_PANDAS: false"),
        "pandas tidak boleh sudah terimpor sebelum diminta (~0,72 detik).\naktual:\n{}",
        h.keluaran
    );
}

/// Sesudah diminta, konversi numpy/pandas HARUS tetap bekerja.
#[test]
fn konversi_numpy_tetap_bekerja_sesudah_impor_ditunda() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let ada_numpy = Command::new("python3")
        .args(["-c", "import numpy"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ada_numpy {
        eprintln!("dilewati: numpy tidak terpasang (bukan dependensi JRG)");
        return;
    }
    let h = jalankan(&program(
        "    py::import(\"numpy\")?\n\
         \x20   wok(\"ARRAY:\", py::eval(\"numpy.arange(4)\")?)\n\
         \x20   wok(\"SKALAR:\", py::eval(\"numpy.int64(7)\")?)\n\
         \x20   wok(\"TIPE_SKALAR:\", typeof(py::eval(\"numpy.int64(7)\")?))",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("ARRAY: [0, 1, 2, 3]"), "aktual:\n{}", h.keluaran);
    assert!(h.keluaran.contains("SKALAR: 7"), "aktual:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("TIPE_SKALAR: int"),
        "skalar numpy harus jadi int JRG asli, bukan handle — np.int64 BUKAN \
         instance int bawaan Python, jadi tanpa penanganan khusus ia akan \
         jatuh jadi handle.\naktual:\n{}",
        h.keluaran
    );
}

// ─── Perantaian yang mempertahankan handle ─────────────────────────────────

/// `py::ref_method` dan saudaranya menjaga hasilnya TETAP di sisi Python.
#[test]
fn ref_method_mempertahankan_handle() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let d = py::ref(\"{'a': [1,2,3]}\")?\n\
         \x20   let biasa = py::item(d, \"a\")?\n\
         \x20   let tetap = py::ref_item(d, \"a\")?\n\
         \x20   wok(\"BIASA:\", typeof(biasa))\n\
         \x20   wok(\"TETAP:\", typeof(tetap))",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("BIASA: array"),
        "`py::item` harus tetap mengonversi kalau bisa — itu perilaku lama \
         yang tidak boleh berubah.\naktual:\n{}",
        h.keluaran
    );
    assert!(
        h.keluaran.contains("TETAP: py:list"),
        "`py::ref_item` harus MEMPERTAHANKAN handle walau nilainya bisa \
         dikonversi. Kalau di sini terbaca `array`, bendera `ref_saja` tidak \
         sampai ke worker dan perantaian kembali menyeberangkan seluruh isi \
         tiap langkah.\naktual:\n{}",
        h.keluaran
    );
}

/// `py::ref_get` dan `py::ref_call` juga, supaya keempatnya terkunci.
#[test]
fn ref_get_dan_ref_call_mempertahankan_handle() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::import(\"builtins\")?\n\
         \x20   let t = py::ref_call(\"builtins\", \"list\", [1,2,3])?\n\
         \x20   wok(\"CALL:\", typeof(t))\n\
         \x20   let s = py::ref(\"'halo'\")?\n\
         \x20   let m = py::ref_method(s, \"upper\")?\n\
         \x20   wok(\"METHOD:\", typeof(m))\n\
         \x20   wok(\"NILAI:\", py::to_jrg(m)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    for (label, harus) in [("CALL:", "py:list"), ("METHOD:", "py:str")] {
        assert!(
            h.keluaran.contains(&format!("{} {}", label, harus)),
            "{} harus mengembalikan handle `{}`.\naktual:\n{}",
            label, harus, h.keluaran
        );
    }
    // Nilainya tetap bisa diambil di akhir — handle yang tidak bisa
    // diseberangkan sama sekali akan jadi jalan buntu, bukan optimasi.
    assert!(
        h.keluaran.contains("NILAI: HALO"),
        "handle harus tetap bisa diseberangkan lewat `py::to_jrg`.\naktual:\n{}",
        h.keluaran
    );
}

// ─── Fungsi JRG sebagai callback Python ────────────────────────────────────

/// Sebuah fungsi JRG bisa diserahkan ke API Python yang menerima fungsi.
#[test]
fn fungsi_jrg_dipakai_python_sebagai_callback() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::import(\"builtins\")?\n\
         \x20   let urut = py::call_kw(\"builtins\", \"sorted\", [[1, 2, 3]], [[\"key\", |x| 0 - x]])?\n\
         \x20   wok(\"URUT:\", urut)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("URUT: [3, 2, 1]"),
        "`sorted(key=)` dengan closure JRG harus membalik urutannya. Kalau di sini \
         terbaca `[1, 2, 3]`, fungsinya tidak pernah dipanggil dan Python diam-diam \
         memakai urutan bawaan.\naktual:\n{}",
        h.keluaran
    );
}

/// Callback yang dipanggil BELAKANGAN, di permintaan yang berbeda.
#[test]
fn callback_malas_masih_bisa_dipanggil_di_permintaan_berikutnya() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::import(\"builtins\")?\n\
         \x20   let m = py::ref_call(\"builtins\", \"map\", |x| x * 2, [1, 2, 3])?\n\
         \x20   wok(\"MAP:\", py::call(\"builtins\", \"list\", m)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("MAP: [2, 4, 6]"),
        "`map` malas, jadi fungsinya dipanggil di permintaan BERIKUTNYA — \
         registri fungsinya harus hidup se-sesi, bukan se-permintaan.\naktual:\n{}",
        h.keluaran
    );
}

/// Kegagalan di dalam callback harus MELEDAK, bukan jadi `nil`.
#[test]
fn callback_yang_gagal_diteruskan_bukan_ditelan() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::import(\"builtins\")?\n\
         \x20   match py::call_kw(\"builtins\", \"sorted\", [[1, 2]], [[\"key\", |x| [1][9]]]) {\n\
         \x20       Ok(v) => wok(\"HASIL:\", v),\n\
         \x20       Err(e) => wok(\"DITOLAK:\", e),\n\
         \x20   }",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("DITOLAK:"),
        "callback yang gagal harus meledak di dalam `sorted()`, bukan menyamar \
         jadi sukses.\naktual:\n{}",
        h.keluaran
    );
}

// ─── Mengubah objek Python di tempat ───────────────────────────────────────

/// `py::set`, `py::set_item`, `py::del_item`, `py::has`, `py::len`.
#[test]
fn objek_python_bisa_diubah_di_tempat() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let d = py::ref(\"{}\")?\n\
         \x20   py::set_item(d, \"a\", 1)?\n\
         \x20   py::set_item(d, \"b\", 2)?\n\
         \x20   wok(\"ADA:\", py::has(d, \"a\")?)\n\
         \x20   py::del_item(d, \"a\")?\n\
         \x20   wok(\"SISA:\", py::repr(d)?)\n\
         \x20   let besar = py::ref(\"list(range(100000))\")?\n\
         \x20   wok(\"PANJANG:\", py::len(besar)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    for potongan in ["ADA: true", "SISA: {'b': 2}", "PANJANG: 100000"] {
        assert!(
            h.keluaran.contains(potongan),
            "tidak menemukan {:?}.\naktual:\n{}", potongan, h.keluaran
        );
    }
}

/// Keyword argument PADA METODE — bentuk paling lazim di pandas.
#[test]
fn kwargs_pada_metode_handle() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    let s = py::ref(\"'a,b,c'\")?\n\
         \x20   wok(\"PECAH:\", py::method_kw(s, \"split\", [\",\"], [[\"maxsplit\", 1]])?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("PECAH: [a, b,c]"),
        "`maxsplit=1` harus membatasi pemecahannya jadi dua bagian; kalau hasilnya \
         tiga, kwargs-nya tidak sampai ke Python.\naktual:\n{}",
        h.keluaran
    );
}

/// `py::` dari DALAM sebuah callback ditolak, tidak menggantung.
#[test]
fn py_dari_dalam_callback_ditolak_bukan_menggantung() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::import(\"builtins\")?\n\
         \x20   let f = |x| match py::eval(\"1\") { Ok(_) => 0, Err(e) => 1 }\n\
         \x20   let u = py::call_kw(\"builtins\", \"sorted\", [[2, 1]], [[\"key\", f]])?\n\
         \x20   wok(\"SELESAI:\", u)",
    ));
    // Yang diuji BUKAN hasilnya, melainkan bahwa programnya selesai sama
    // sekali. `jalankan` punya batas waktunya sendiri; kalau perbaikan ini
    // dicabut, test ini berhenti karena batas itu, bukan karena assert.
    assert!(
        h.keluaran.contains("SELESAI:"),
        "program tidak sampai ke akhir — kemungkinan deadlock callback kembali.\naktual:\n{}",
        h.keluaran
    );
}

// ─── Callback dari thread Python ───────────────────────────────────────────

/// Beberapa thread Python memanggil balik ke JRG SEKALIGUS.
#[test]
fn callback_dari_banyak_thread_python_tidak_saling_mencuri() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::exec(\"import concurrent.futures as cf\")?\n\
         \x20   py::import(\"builtins\")?\n\
         \x20   let ex = py::ref(\"cf.ThreadPoolExecutor(max_workers=8)\")?\n\
         \x20   let xs = py::ref(\"list(range(64))\")?\n\
         \x20   let it = py::ref_method(ex, \"map\", |x| x * 2, xs)?\n\
         \x20   let hasil = py::call(\"builtins\", \"list\", it)?\n\
         \x20   wok(\"JUMLAH:\", py::call(\"builtins\", \"sum\", hasil)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    // sum(2*i for i in 0..63) = 2 * (63*64/2) = 4032
    assert!(
        h.keluaran.contains("JUMLAH: 4032"),
        "delapan thread memanggil balik bersamaan; hasilnya harus tetap benar. \
         `operasi tidak dikenal dari jrgc` di sini berarti kunci pipanya lepas.\naktual:\n{}",
        h.keluaran
    );
}

/// Callback dari thread LEPAS — yang menyala sesudah permintaan pembuatnya
/// sudah dijawab.
#[test]
fn callback_dari_thread_lepas_dilayani_py_apa_pun() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let h = jalankan(&program(
        "    py::exec(\"import threading, time\")?\n\
         \x20   py::exec(\"hasil = []\")?\n\
         \x20   py::exec(\"def mulai(f):\\n    def w():\\n        time.sleep(0.3)\\n        \
         hasil.append(f(7))\\n    t = threading.Thread(target=w)\\n    t.start()\\n    return t\")?\n\
         \x20   let mulai = py::ref(\"mulai\")?\n\
         \x20   let _t = py::ref_method(mulai, \"__call__\", |x| x * 3)?\n\
         \x20   // Ditunggu lewat `py::eval` — jalur yang DULU tidak bisa\n\
         \x20   // melayani panggilan balik sama sekali.\n\
         \x20   py::eval(\"time.sleep(0.8)\")?\n\
         \x20   wok(\"HASIL:\", py::eval(\"hasil\")?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(
        h.keluaran.contains("HASIL: [21]"),
        "callback dari thread lepas harus dilayani oleh `py::` mana pun yang \
         kebetulan berjalan berikutnya. Daftar kosong di sini berarti `py::eval` \
         kembali tidak membawa dispatcher.\naktual:\n{}",
        h.keluaran
    );
}

// ════════════════════════════════════════════════════════════════════════
// Batas nilai di jembatan Python …

#[test]
fn float_tetap_float_saat_menyeberang_ke_python() {
    if !ada_python() { eprintln!("dilewati: `python3` tidak ada di PATH"); return; }
    let h = jalankan(&program(
        "    py::import(\"builtins\")?\n\
         \x20   wok(\"A:\", py::call(\"builtins\", \"repr\", 2.0)?)\n\
         \x20   wok(\"B:\", py::call(\"builtins\", \"repr\", [1.0, 2.0])?)\n\
         \x20   wok(\"C:\", py::call(\"builtins\", \"repr\", 2)?)",
    ));
    assert!(h.sukses, "program gagal:\n{}", h.keluaran);
    assert!(h.keluaran.contains("A: 2.0"),
        "float JRG tiba di Python sebagai int — `numpy.array([1.0,2.0])` akan bertipe Int64:\n{}",
        h.keluaran);
    assert!(h.keluaran.contains("B: [1.0, 2.0]"),
        "elemen array float kehilangan tipenya:\n{}", h.keluaran);
    assert!(h.keluaran.contains("C: 2"),
        "int JRG malah berubah jadi float:\n{}", h.keluaran);
}

#[test]
fn nan_dan_tak_hingga_menyeberang_dua_arah() {
    if !ada_python() { eprintln!("dilewati: `python3` tidak ada di PATH"); return; }
    let h = jalankan(&program(
        "    let n = py::eval(\"float('nan')\")?\n\
         \x20   wok(\"NAN:\", typeof(n), n != n)\n\
         \x20   let i = py::eval(\"float('inf')\")?\n\
         \x20   wok(\"INF:\", typeof(i))\n\
         \x20   py::import(\"builtins\")?\n\
         \x20   wok(\"KIRIM:\", py::call(\"builtins\", \"repr\", 1.0/0.0)?)",
    ));
    assert!(h.sukses, "NaN/inf dari Python menggagalkan jembatan:\n{}", h.keluaran);
    // NaN != NaN adalah satu-satunya cara memastikan yang tiba benar-benar NaN
    // dan bukan sekadar sesuatu yang tercetak "NaN".
    assert!(h.keluaran.contains("NAN: float true"),
        "NaN tidak tiba sebagai float yang benar-benar NaN:\n{}", h.keluaran);
    assert!(h.keluaran.contains("INF: float"), "inf tidak tiba sebagai float:\n{}", h.keluaran);
    assert!(h.keluaran.contains("KIRIM: inf"),
        "inf dari JRG diubah jadi `None` saat menyeberang:\n{}", h.keluaran);
}

#[test]
fn integer_python_terlalu_besar_ditolak_bukan_dibulatkan() {
    if !ada_python() { eprintln!("dilewati: `python3` tidak ada di PATH"); return; }
    let h = jalankan(&program("    wok(py::eval(\"10**40\")?)"));
    assert!(
        h.keluaran.contains("terlalu besar untuk `int` JRG"),
        "integer Python di luar 128 bit harus DITOLAK dengan sebabnya, bukan dibulatkan \
         jadi f64 dan dikembalikan seolah berhasil.\naktual:\n{}",
        h.keluaran
    );
    // Dan ia bukan kegagalan protokol, jadi ia tidak boleh dibingkai sebagai
    // satu.
    assert!(
        !h.keluaran.contains("respons worker python tidak valid")
            && !h.keluaran.contains("mentah:"),
        "nilai yang tidak muat bukan kegagalan protokol, jadi pesannya tidak boleh \
         membingkainya begitu atau menempelkan JSON mentahnya.\naktual:\n{}",
        h.keluaran
    );
    // Yang muat harus tetap eksak, dan jalan keluar yang disebut pesannya
    // harus benar-benar bekerja.
    let h2 = jalankan(&program(
        "    wok(\"MUAT:\", py::eval(\"10**30\")?)\n\
         \x20   wok(\"STR:\", py::eval(\"str(10**40)\")?)",
    ));
    assert!(h2.sukses, "program gagal:\n{}", h2.keluaran);
    assert!(h2.keluaran.contains("MUAT: 1000000000000000000000000000000"),
        "integer yang muat harus tetap eksak:\n{}", h2.keluaran);
    assert!(h2.keluaran.contains("STR: 10000000000000000000000000000000000000000"),
        "jalan keluar `str(x)` yang disebut pesannya tidak bekerja:\n{}", h2.keluaran);
}

/// Beberapa program `py::` yang jalan BERSAMAAN tidak boleh saling merusak
/// runner-nya.
/// `h.x` tanpa `()` tidak memanggil apa pun: dulu interpreter diam-diam menjalankan `x()`.
#[test]
fn atribut_handle_tanpa_kurung_ditolak_di_dua_mesin() {
    if !ada_python() { eprintln!("dilewati: `python3` tidak ada di PATH"); return; }
    let h = jalankan("package main\npub fn main() {\n    let h = py::ref(\"[1]\").unwrap()\n    wok(h.append)\n}\n");
    assert!(!h.sukses && h.keluaran.contains("ditulis tanpa `()` di atas handle `py:list`")
        && h.keluaran.contains("py::get(h, \"append\")"), "{}", h.keluaran);
}

#[test]
fn runner_python_tidak_rusak_saat_banyak_program_jalan_bersamaan() {
    if !ada_python() { eprintln!("dilewati: `python3` tidak ada di PATH"); return; }
    let sumber = program("    let v = py::eval(\"1+1\")?\n\x20   wok(\"NILAI:\", v)");
    let mut anak = Vec::new();
    for _ in 0..12 {
        let src = sumber.clone();
        anak.push(std::thread::spawn(move || jalankan(&src)));
    }
    let hasil: Vec<Hasil> = anak.into_iter().map(|h| h.join().expect("thread uji")).collect();
    let rusak: Vec<&Hasil> = hasil.iter().filter(|h| !h.keluaran.contains("NILAI: 2")).collect();
    assert!(
        rusak.is_empty(),
        "{} dari {} program `py::` yang jalan bersamaan gagal — runner-nya \
         ditulis tidak atomik, jadi ada yang membaca berkas setengah jadi.\n\
         contoh keluaran:\n{}",
        rusak.len(), hasil.len(), rusak[0].keluaran
    );
}

// ── php:: — worker yang sama, bahasa lain. Di berkas ini, bukan berkas baru:
// biner tes integrasi ke-17 adalah biaya nyata di mesin 7 GB.

/// Tulisan MENTAH ke fd 1 dari kode pemakai (di luar `print`/`echo`/`console`)
/// diteruskan ke stderr, dan jawaban sesudahnya TIDAK bergeser. Dulu baris itu
/// dibaca sebagai jawaban rusak dan `eval("1")` berikutnya menjawab `nil`.
#[test]
fn tulisan_mentah_ke_fd1_tidak_menggeser_jawaban() {
    let ada = |b: &str, a: &str| Command::new(b).arg(a).output().map(|o| o.status.success()).unwrap_or(false);
    let kasus = [
        ("python3", "--version", "py::exec(\"import os; os.write(1, b'MENTAH-PY\\\\n')\")", "py::eval(\"1 + 1\")"),
        ("node", "--version", "js::exec(\"require('fs').writeSync(1, 'MENTAH-JS\\\\n')\")", "js::eval(\"1 + 1\")"),
        ("php", "--version", "php::exec(\"fwrite(STDOUT, 'MENTAH-PHP' . PHP_EOL);\")", "php::eval(\"1 + 1\")"),
    ];
    for (bin, arg, tulis, tanya) in kasus {
        if !ada(bin, arg) { eprintln!("dilewati: `{}` tidak ada di PATH", bin); continue; }
        let dir = crate::bersama::dir_unik("jrg-fd1");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        let f = dir.join("uji.jrg");
        std::fs::write(&f, format!("wok(\"TULIS:\", {})\nwok(\"TANYA:\", {})\n", tulis, tanya)).unwrap();
        let out = Command::new(jrgc()).arg(&f).arg("--sopan").stdin(std::process::Stdio::null())
            .output().expect("jalankan jrgc");
        let _ = std::fs::remove_dir_all(&dir);
        let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(o.contains("TULIS: Result::Ok(nil)") && o.contains("TANYA: Result::Ok(2)"),
                "{}: jawabannya bergeser atau rusak:\n{}\n{}", bin, o, e);
        assert!(e.contains("MENTAH-"), "{}: tulisan mentahnya harus terlihat di stderr:\n{}", bin, e);
    }
}

/// Enum menyeberang ke worker dengan bentuk yang sama dengan `json::stringify`
/// (`"None"`, `{"Some":3}`), bukan `{"__jrg_enum__":..}`, dan kembali sebagai DATA.
#[test]
fn enum_menyeberang_dengan_bentuk_json() {
    let ada = |b: &str| Command::new(b).arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    let nilai = "[Option::Some(3), Option::None, W::Rgb(1, 2), W::Kotak { l: 2, p: 1 }, Result::Err(\"x\")]";
    let kasus = [
        ("python3", format!("py::call(\"json\", \"dumps\", {})", nilai),
         "py::call(\"copy\", \"copy\", Option::None)"),
        ("node", format!("js::call(\"util\", \"format\", \"%j\", {})", nilai),
         "js::call(\"util\", \"format\", \"%s\", Option::None)"),
        ("php", format!("php::call(\"json_encode\", {})", nilai),
         "php::call(\"array_values\", [Option::None]).map(|a| a[0])"),
    ];
    for (bin, kirim, balik) in kasus {
        if !ada(bin) { eprintln!("dilewati: `{}` tidak ada di PATH", bin); continue; }
        let dir = crate::bersama::dir_unik("jrg-enum-ffi");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        let f = dir.join("uji.jrg");
        std::fs::write(&f, format!(
            "package main\nenum W {{ Merah, Rgb(int, int), Kotak {{ l: int, p: int }} }}\n\
             pub fn main() {{\n    wok(\"KIRIM:\", {}.unwrap())\n    let b = {}.unwrap()\n    wok(\"BALIK:\", typeof(b), b)\n}}\n",
            kirim, balik)).unwrap();
        let out = Command::new(jrgc()).arg(&f).arg("--sopan").stdin(std::process::Stdio::null())
            .output().expect("jalankan jrgc");
        let _ = std::fs::remove_dir_all(&dir);
        let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(o.replace(' ', "").contains(r#"KIRIM:[{"Some":3},"None",{"Rgb":[1,2]},{"Kotak":{"l":2,"p":1}},{"Err":"x"}]"#),
                "{}: bentuk enum di sisi lain:\n{}\n{}", bin, o, e);
        assert!(o.contains("BALIK: string None"), "{}: enum kembali sebagai data:\n{}\n{}", bin, o, e);
        assert!(!o.contains("__jrg_enum__"), "{}: nama representasi bocor:\n{}", bin, o);
    }
}

/// Puncak RSS (kB) sesudah `n` putaran yang tiap kalinya menyerahkan closure
/// ber-tangkapan besar ke worker; `None` kalau `/proc` tidak terbaca.
fn puncak_serah_fungsi(panggil: &str, n: u32) -> Option<i64> {
    let src = format!(
        "var i = 0\nwhile i < {} {{\n    let besar = arr::fill(i, 2000)\n\
         \x20   let f = |x| x + len(besar)\n    let _ = {}\n    i = i + 1\n}}\n\
         for _, l in str::split(file::read(\"/proc/self/status\").unwrap_or(\"\"), \"\\n\") {{\n\
         \x20   if str::starts_with(l, \"VmHWM\") {{ wok(l) }}\n}}\n", n, panggil);
    let h = jalankan(&src);
    h.keluaran.lines().find(|l| l.starts_with("VmHWM"))
        .and_then(|l| l.split_whitespace().nth(1)).and_then(|k| k.parse().ok())
}

/// Fungsi JRG yang pembungkusnya sudah mati di worker dilepas dari registri
/// jrgc — dulu tertahan sampai sesi berakhir.
#[test]
fn fungsi_yang_diserahkan_ke_python_dilepas() {
    if !ada_python() { eprintln!("dilewati: `python3` tidak ada di PATH"); return; }
    let panggil = "py::call(\"builtins\", \"callable\", f)";
    let (Some(kecil), Some(besar)) = (puncak_serah_fungsi(panggil, 20), puncak_serah_fungsi(panggil, 220))
    else { eprintln!("dilewati: /proc/self/status tidak terbaca"); return; };
    assert!(besar - kecil < 20_000,
        "puncak memori tumbuh {} kB untuk 200 closure tambahan — fungsi yang sudah mati di \
         Python masih ditahan registri jrgc", besar - kecil);
}

mod php {
    use super::jrgc;
    use std::process::Command;

    /// PHP 8.0+ ada? Tanpanya tiap tes MELEWAT sambil mengatakannya.
    fn ada_php() -> bool {
        Command::new("php")
            .args(["-r", "exit(PHP_VERSION_ID >= 80000 ? 0 : 1);"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Sama dengan versi Python: `__destruct` penjaga fungsinya mengabarkan jrgc.
    #[test]
    fn fungsi_yang_diserahkan_ke_php_dilepas() {
        if !ada_php() { eprintln!("dilewati: `php` tidak ada di PATH"); return; }
        let panggil = "php::call(\"is_callable\", f)";
        let (Some(kecil), Some(besar)) =
            (super::puncak_serah_fungsi(panggil, 20), super::puncak_serah_fungsi(panggil, 220))
        else { eprintln!("dilewati: /proc/self/status tidak terbaca"); return; };
        assert!(besar - kecil < 20_000,
            "puncak memori tumbuh {} kB untuk 200 closure tambahan — fungsi yang sudah mati di \
             PHP masih ditahan registri jrgc", besar - kecil);
    }

    struct Keluaran { stdout: String, stderr: String, sukses: bool }

    /// Satu program di direktorinya sendiri (jalur relatif `php::require`
    /// dan `php::render` dibaca dari sana), dengan berkas pendukung.
    fn jalankan_di(badan: &str, berkas: &[(&str, &str)], vm: bool) -> Keluaran {
        let dir = crate::bersama::dir_unik("jrg-php");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        for (nama, isi) in berkas { std::fs::write(dir.join(nama), isi).expect("tulis berkas"); }
        let src = format!(
            "package main\nfn coba() {{\n{}\n    return Ok(0)\n}}\n\
             pub fn main() {{ match coba() {{ Ok(_) => wok(\"SELESAI\"), \
             Err(e) => wok(\"GAGAL:\", e) }} }}\n", badan);
        std::fs::write(dir.join("uji.jrg"), src).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.arg("uji.jrg").arg("--sopan").current_dir(&dir);
        if vm { cmd.arg("--vm"); }
        let out = cmd.output().expect("jalankan jrgc");
        let _ = std::fs::remove_dir_all(&dir);
        Keluaran {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            sukses: out.status.success(),
        }
    }

    fn jalankan(badan: &str) -> Keluaran { jalankan_dua(badan, &[]) }

    /// Jalankan di dua mesin dan tuntut keluaran yang sama; yang dijawab milik interpreter.
    fn jalankan_dua(badan: &str, berkas: &[(&str, &str)]) -> Keluaran {
        let i = jalankan_di(badan, berkas, false);
        let v = jalankan_di(badan, berkas, true);
        crate::bersama::dua_mesin_sama(badan,
            &format!("{}{}", i.stdout, i.stderr), &format!("{}{}", v.stdout, v.stderr));
        i
    }

    macro_rules! lewati_tanpa_php {
        () => { if !ada_php() { eprintln!("dilewati: `php` 8.0+ tidak ada di PATH"); return; } };
    }

    /// Tiap bentuk nilai menyeberang dengan TIPE-nya — `2.0` tetap float
    /// (`json_encode` menulisnya `2`), `{}` tetap dict, `NAN` bukan nil.
    #[test]
    fn nilai_menyeberang_dengan_tipenya() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    let v = php::eval(\"[1, 2.0, -0.5, INF, NAN, 'héé', null, true]\")?\n\
             \x20   for _, x in v { wok(\"T\", typeof(x), x) }\n\
             \x20   wok(\"ASSOC\", php::eval(\"['a' => 1, 'b' => [1, 2]]\")?)\n\
             \x20   wok(\"KOSONG\", typeof(php::eval(\"new stdClass\")?), typeof(php::eval(\"[]\")?))\n\
             \x20   wok(\"MASUK\", php::call(\"array_sum\", [1.5, 2.5])?, php::call(\"is_nan\", 0.0 / 0.0)?)\n\
             \x20   wok(\"DICT\", php::call(\"array_keys\", {\"x\": 1, \"y\": 2})?)",
        );
        assert!(k.sukses && k.stdout.contains("SELESAI"), "program gagal:\n{}{}", k.stdout, k.stderr);
        for harap in ["T int 1", "T float 2.0", "T float -0.5", "T float inf", "T float NaN",
                      "T string héé", "T nil nil", "T bool true", "ASSOC dict {a: 1, b: [1, 2]}",
                      "KOSONG dict array", "MASUK 4.0 true", "DICT [x, y]"] {
            assert!(k.stdout.contains(harap), "`{}` tidak ada.\naktual:\n{}", harap, k.stdout);
        }
    }

    /// Warning PHP adalah GALAT: `file_get_contents` yang gagal menjawab
    /// `false`, dan `false` yang menyeberang diam-diam adalah hasil salah.
    #[test]
    fn warning_jadi_galat_dan_at_tetap_berlaku() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    match php::call(\"file_get_contents\", \"/tidak/ada/sama/sekali\") {\n\
             \x20       Ok(v) => wok(\"DIAM:\", v),\n\
             \x20       Err(e) => wok(\"GALAT:\", e),\n\
             \x20   }\n\
             \x20   wok(\"AT:\", php::eval(\"@file_get_contents('/tidak/ada/sama/sekali')\")?)\n\
             \x20   match php::eval(\"intdiv(1, 0)\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"GALAT:\", e) }",
        );
        assert!(k.stdout.contains("GALAT: Warning: file_get_contents(/tidak/ada/sama/sekali)"),
                "warning harus jadi Err yang menyebut jenis dan fungsinya:\n{}", k.stdout);
        assert!(k.stdout.contains("AT: false"),
                "`@` harus tetap meredam warning, seperti di PHP:\n{}", k.stdout);
        assert!(k.stdout.contains("GALAT: DivisionByZeroError: Division by zero"), "{}", k.stdout);
        assert!(!k.stdout.contains("DIAM:"), "{}", k.stdout);
    }

    /// `echo` milik PHP pergi ke stderr — ia tidak boleh masuk kawat
    /// protokol, dan tidak boleh hilang.
    #[test]
    fn echo_ke_stderr_tanpa_merusak_protokol() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    php::exec(\"echo 'GEMA-PHP'; print_r([7]); var_dump(1.5);\")?\n\
             \x20   wok(\"UTUH:\", php::eval(\"'masih bicara'\")?)",
        );
        assert!(k.stdout.contains("UTUH: masih bicara"), "protokol rusak:\n{}{}", k.stdout, k.stderr);
        assert!(k.stderr.contains("GEMA-PHP") && k.stderr.contains("float(1.5)"),
                "keluaran PHP harus terlihat di stderr:\n{}", k.stderr);
        assert!(!k.stdout.contains("GEMA-PHP"), "keluaran PHP bocor ke stdout:\n{}", k.stdout);
    }

    /// Fungsi JRG jadi callback PHP; gagalnya MERAMBAT, dan panggilan
    /// `php::` dari dalamnya ditolak alih-alih menggantung.
    #[test]
    fn callback_jrg_dari_php() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    wok(\"MAP:\", php::call(\"array_map\", |x| x * 10, [1, 2, 3])?)\n\
             \x20   wok(\"FILTER:\", php::call(\"array_values\", php::call(\"array_filter\", [1, 2, 3, 4], |x| x % 2 == 0)?)?)\n\
             \x20   let a = php::ref(\"[3, 1, 2]\")?\n\
             \x20   php::call(\"usort\", a, |p, q| q - p)?\n\
             \x20   wok(\"USORT:\", php::to_jrg(a)?)\n\
             \x20   match php::call(\"array_map\", |x| x / 0, [1]) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"GAGAL-CB:\", e) }\n\
             \x20   let r = php::call(\"array_map\", |x| php::eval(\"1\"), [1])?\n\
             \x20   wok(\"REENTRAN:\", r)",
        );
        assert!(k.stdout.contains("MAP: [10, 20, 30]"), "{}{}", k.stdout, k.stderr);
        assert!(k.stdout.contains("FILTER: [2, 4]"), "{}", k.stdout);
        assert!(k.stdout.contains("USORT: [3, 2, 1]"), "usort harus mengubah array di handle:\n{}", k.stdout);
        assert!(k.stdout.contains("GAGAL-CB: RuntimeException: fungsi JRG gagal:"),
                "callback yang gagal harus merambat, bukan jadi null:\n{}", k.stdout);
        assert!(k.stdout.contains("dari dalam callback php tidak bisa dilayani"),
                "panggilan php:: dari dalam callback harus ditolak:\n{}", k.stdout);
        assert!(k.stdout.contains("SELESAI"), "program harus SAMPAI AKHIR:\n{}{}", k.stdout, k.stderr);
    }

    /// Parameter by-reference cuma bisa diisi handle; nilai JRG adalah
    /// salinan, jadi `sort([3,1,2])` yang "berhasil" adalah hasil salah.
    #[test]
    fn by_reference_butuh_handle() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    match php::call(\"sort\", [3, 1, 2]) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }\n\
             \x20   let a = php::ref(\"[3, 1, 2]\")?\n\
             \x20   php::call(\"sort\", a)?\n\
             \x20   wok(\"HANDLE:\", php::to_jrg(a)?)",
        );
        assert!(k.stdout.contains("TOLAK: Warning: sort(): Argument #1 ($array) must be passed by reference"),
                "{}{}", k.stdout, k.stderr);
        assert!(k.stdout.contains("bantuan: parameter by-reference cuma bisa diisi handle PHP"), "{}", k.stdout);
        assert!(k.stdout.contains("HANDLE: [1, 2, 3]"), "{}", k.stdout);
    }

    /// `int` PHP 64 bit: yang lebih besar ditolak di tiap kedalaman, bukan
    /// dibiarkan `json_decode` jadi float yang tidak tepat.
    #[test]
    fn integer_di_luar_64_bit_ditolak() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    match php::call(\"count\", [1, [2, 9223372036854775808]]) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }\n\
             \x20   wok(\"MUAT:\", php::call(\"intdiv\", 9223372036854775807, 1)?)",
        );
        assert!(k.stdout.contains("TOLAK: argumen ke-1 yang diteruskan ke PHP memuat bilangan bulat 9223372036854775808 yang tidak muat di `int` PHP (64 bit"),
                "{}{}", k.stdout, k.stderr);
        assert!(k.stdout.contains("MUAT: 9223372036854775807"), "{}", k.stdout);
    }

    /// Objek tanpa padanan jadi HANDLE; method-nya bisa dipanggil langsung,
    /// dan yang tidak ada adalah galat, bukan nil.
    #[test]
    fn objek_jadi_handle() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    let t = php::new(\"DateTime\", \"2020-01-02\")?\n\
             \x20   wok(\"TIPE:\", typeof(t))\n\
             \x20   wok(\"FORMAT:\", t.format(\"Y-m-d\")?)\n\
             \x20   let o = php::ref(\"(object)['nama' => 'budi']\")?\n\
             \x20   php::set(o, \"umur\", 30)?\n\
             \x20   wok(\"OBJ:\", php::get(o, \"nama\")?, php::to_jrg(o)?)\n\
             \x20   match php::get(o, \"sayap\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }\n\
             \x20   match t.terbang() { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }\n\
             \x20   match php::new(\"KelasHantu\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }\n\
             \x20   let b = php::eval(\"\\\"a\\\\xffb\\\"\")?\n\
             \x20   wok(\"BINER:\", typeof(b), php::len(b)?)\n\
             \x20   wok(\"REPR:\", php::repr(b)?)\n\
             \x20   match php::to_jrg(b) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }",
        );
        assert!(k.sukses, "{}{}", k.stdout, k.stderr);
        for harap in ["TIPE: php:DateTime", "FORMAT: 2020-01-02", "OBJ: budi dict {nama: budi, umur: 30}",
                      "TOLAK: Warning: Undefined property: stdClass::$sayap",
                      "TOLAK: Error: objek `DateTime` tidak punya method `terbang`",
                      "TOLAK: Error: kelas `KelasHantu` tidak ada di PHP",
                      "BINER: php:string 3", "REPR: string biner 3 byte: 61ff62", "bin2hex"] {
            assert!(k.stdout.contains(harap), "`{}` tidak ada.\naktual:\n{}", harap, k.stdout);
        }
        assert!(!k.stdout.contains("DIAM:"), "{}", k.stdout);
    }

    /// `exit()`, kehabisan memori dan rekursi tak berujung DILAPORKAN, worker
    /// berikutnya hidup, dan handle lama ditolak dengan kalimat SESI sejak
    /// panggilan pertama sesudahnya.
    #[test]
    fn exit_dan_fatal_dilaporkan_lalu_pulih() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    let t = php::new(\"DateTime\")?\n\
             \x20   match php::exec(\"exit(4);\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"EXIT:\", e) }\n\
             \x20   match t.format(\"Y\") { Ok(v) => wok(\"BAHAYA:\", v), Err(e) => wok(\"BASI:\", e) }\n\
             \x20   wok(\"PULIH1:\", php::eval(\"1 + 1\")?)\n\
             \x20   match php::exec(\"ini_set('memory_limit', '64M'); $a = []; while (true) { $a[] = str_repeat('x', 1000); }\") {\n\
             \x20       Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"MEMORI:\", e) }\n\
             \x20   match php::exec(\"ini_set('memory_limit', '64M'); function r($n) { return r($n + 1); } r(0);\") {\n\
             \x20       Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"REKURSI:\", e) }\n\
             \x20   wok(\"PULIH2:\", php::eval(\"2 + 2\")?)",
        );
        assert!(k.sukses, "{}{}", k.stdout, k.stderr);
        assert!(k.stdout.contains("EXIT: kode PHP memanggil exit()"), "{}", k.stdout);
        assert!(k.stdout.contains("BASI:") && k.stdout.contains("sesi worker sebelumnya"),
                "handle lama harus ditolak dengan kalimat sesi:\n{}", k.stdout);
        assert!(k.stdout.contains("PULIH1: 2") && k.stdout.contains("PULIH2: 4"), "{}", k.stdout);
        assert!(k.stdout.contains("MEMORI: fatal error PHP: Allowed memory size"), "{}", k.stdout);
        assert!(k.stdout.contains("REKURSI: worker php berhenti tanpa menjawab (kode keluar 255)"),
                "{}", k.stdout);
        assert!(!k.stdout.contains("BAHAYA:") && !k.stdout.contains("DIAM:"), "{}", k.stdout);
    }

    /// Berkas PHP dibaca relatif terhadap direktori kerja: `require` menjawab
    /// nilai `return`-nya, dan templat jadi teks dengan data dari dict.
    #[test]
    fn require_dan_render() {
        lewati_tanpa_php!();
        let k = jalankan_dua(
            "    wok(\"KONF:\", php::require(\"konf.php\")?)\n\
             \x20   wok(\"FN:\", php::call(\"ganda\", 21)?)\n\
             \x20   wok(\"HTML:\", php::render(\"t.php\", {\"judul\": \"<Halo>\", \"daftar\": [1, 2]})?)\n\
             \x20   wok(\"STR:\", php::render_str(\"Hai <?= $n ?>!\", {\"n\": \"Ani\"})?)\n\
             \x20   match php::require(\"hilang.php\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }\n\
             \x20   match php::render_str(\"x\", {\"a-b\": 1}) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }",
            &[("konf.php", "<?php\nfunction ganda($x) { return $x * 2; }\nreturn ['db' => 'mysql', 'port' => 3306];\n"),
              ("t.php", "<h1><?= htmlspecialchars($judul) ?></h1><?php foreach ($daftar as $x): ?><i><?= $x ?></i><?php endforeach; ?>")],
        );
        assert!(k.sukses, "{}{}", k.stdout, k.stderr);
        for harap in ["KONF: dict {db: mysql, port: 3306}", "FN: 42",
                      "HTML: <h1>&lt;Halo&gt;</h1><i>1</i><i>2</i>", "STR: Hai Ani!",
                      "TOLAK: RuntimeException: berkas `hilang.php` tidak ada",
                      "TOLAK: TypeError: kunci `a-b` di data templat bukan nama variabel PHP yang sah"] {
            assert!(k.stdout.contains(harap), "`{}` tidak ada.\naktual:\n{}", harap, k.stdout);
        }
    }

    /// Variabel dari `php::exec` dan dari tingkat atas berkas `require` adalah
    /// variabel GLOBAL PHP — versi pertama menyimpannya di lingkup tiruan, dan
    /// `global $x` di dalam fungsi PHP tidak pernah melihatnya.
    #[test]
    fn variabel_exec_dan_require_adalah_global_php() {
        lewati_tanpa_php!();
        let k = jalankan_dua(
            "    php::exec(\"$diskon = 10; function neto($x) { global $diskon; return $x - $diskon; }\")?\n\
             \x20   wok(\"NETO:\", php::call(\"neto\", 100)?, php::eval(\"$GLOBALS['diskon']\")?)\n\
             \x20   php::require(\"situs.php\")?\n\
             \x20   wok(\"SITUS:\", php::call(\"nama_situs\")?, php::eval(\"$situs\")?)\n\
             \x20   let _ = php::exec(\"$sebelum = 7; throw new Exception('x');\")\n\
             \x20   wok(\"TETAP:\", php::eval(\"$sebelum\")?)",
            &[("situs.php", "<?php\n$situs = 'blog';\nfunction nama_situs() { global $situs; return strtoupper($situs); }\n")],
        );
        assert!(k.stdout.contains("NETO: 90 10"), "{}{}", k.stdout, k.stderr);
        assert!(k.stdout.contains("SITUS: BLOG blog"), "{}", k.stdout);
        assert!(k.stdout.contains("TETAP: 7"), "{}", k.stdout);
    }

    /// Argumen bernama PHP 8 lewat dict, untuk fungsi dan konstruktor.
    #[test]
    fn argumen_bernama() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    wok(\"PAD:\", php::call_kw(\"str_pad\", [\"7\"], {\"length\": 3, \"pad_string\": \"0\", \"pad_type\": 0})?)\n\
             \x20   let t = php::new_kw(\"DateTimeImmutable\", [], {\"datetime\": \"2021-03-04\"})?\n\
             \x20   wok(\"TGL:\", t.format(\"d/m/Y\")?)\n\
             \x20   match php::call_kw(\"str_pad\", [\"7\"], {\"hantu\": 1}) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TOLAK:\", e) }",
        );
        assert!(k.stdout.contains("PAD: 007") && k.stdout.contains("TGL: 04/03/2021"), "{}{}", k.stdout, k.stderr);
        assert!(k.stdout.contains("TOLAK: Error: Unknown named parameter $hantu"), "{}", k.stdout);
    }

    /// Handle yang mati di JRG dilepas di sisi PHP — janjinya bisa DIPERIKSA.
    #[test]
    fn handle_dilepas_otomatis() {
        lewati_tanpa_php!();
        let k = jalankan(
            "    for i in range(0, 5) { let _h = php::new(\"ArrayObject\")? }\n\
             \x20   let tetap = php::new(\"ArrayObject\")?\n\
             \x20   wok(\"REFS:\", php::refs()?)\n\
             \x20   wok(\"MASIH:\", php::len(tetap)?)",
        );
        assert!(k.stdout.contains("REFS: 1") && k.stdout.contains("MASIH: 0"), "{}{}", k.stdout, k.stderr);
    }

    /// **Sebuah dict yang menyeberang menjawab SAMA di tiap jalan.**
    #[test]
    fn dict_menyeberang_dengan_urutan_yang_sama_tiap_jalan() {
        lewati_tanpa_php!();
        const BADAN: &str = "    let d = {\"z\": 1, \"a\": 2, \"m\": 3, \"b\": 4}\n             \x20   wok(\"KUNCI\", php::call(\"array_keys\", d)?)\n             \x20   wok(\"NILAI\", php::call(\"array_values\", d)?)";
        let pertama = jalankan(BADAN);
        assert!(pertama.sukses, "program gagal:\n{}{}", pertama.stdout, pertama.stderr);
        // Terurut, sama dengan setiap pintu lain di bahasa ini.
        assert!(pertama.stdout.contains("KUNCI [a, b, m, z]"),
                "kunci harus terurut seperti `dict::keys`:\n{}", pertama.stdout);
        assert!(pertama.stdout.contains("NILAI [2, 4, 3, 1]"),
                "nilai harus ikut urutan kuncinya:\n{}", pertama.stdout);
        // Dan sama di tiap PROSES: pengacakan `HashMap` Rust per-proses adalah
        // yang membuat versi lamanya lolos berkali-kali sebelum memerah.
        for i in 0..7 {
            let lagi = jalankan(BADAN);
            assert_eq!(pertama.stdout, lagi.stdout,
                       "jalan ke-{} menjawab beda:\n{}", i + 2, lagi.stdout);
        }
    }
}

// ── js:: — sejauh ini tanpa tes sama sekali; ini yang pertama.
mod js {
    use super::jrgc;
    use std::process::Command;

    fn ada_node() -> bool {
        Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    struct Keluaran { stdout: String, stderr: String }

    /// Satu program di direktorinya sendiri; `coba()` menjawab `Result` supaya `?` bisa dipakai.
    fn jalankan(badan: &str) -> Keluaran {
        let dir = crate::bersama::dir_unik("jrg-js");
        std::fs::create_dir_all(&dir).expect("bikin dir sementara");
        let src = format!(
            "package main\nfn coba() {{\n{}\n    return Ok(0)\n}}\n\
             pub fn main() {{ match coba() {{ Ok(_) => wok(\"SELESAI\"), \
             Err(e) => wok(\"GAGAL:\", e) }} }}\n", badan);
        std::fs::write(dir.join("uji.jrg"), src).expect("tulis sumber");
        let out = Command::new(jrgc()).arg("uji.jrg").arg("--sopan").current_dir(&dir)
            .stdin(std::process::Stdio::null()).output().expect("jalankan jrgc");
        let vm = Command::new(jrgc()).arg("uji.jrg").arg("--sopan").arg("--vm").current_dir(&dir)
            .stdin(std::process::Stdio::null()).output().expect("jalankan jrgc --vm");
        let _ = std::fs::remove_dir_all(&dir);
        let gabung = |o: &std::process::Output| format!(
            "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        crate::bersama::dua_mesin_sama(badan, &gabung(&out), &gabung(&vm));
        Keluaran {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    macro_rules! lewati_tanpa_node {
        () => { if !ada_node() { eprintln!("dilewati: `node` tidak ada di PATH"); return; } };
    }

    fn harus(k: &Keluaran, potongan: &[&str]) {
        for p in potongan {
            assert!(k.stdout.contains(p), "`{}` tidak ada.\nstdout:\n{}\nstderr:\n{}", p, k.stdout, k.stderr);
        }
    }

    /// Tiap bentuk nilai menyeberang dengan tipenya; galat JavaScript jadi `Err`, dan
    /// Promise ditunggu.
    #[test]
    fn nilai_dan_galat_menyeberang() {
        lewati_tanpa_node!();
        let k = jalankan(
            "    for _, x in js::eval(\"[1, 2.5, Infinity, NaN, 'héé', null, true]\")? { wok(\"T\", typeof(x), x) }\n\
             \x20   wok(\"OBJ\", js::eval(\"({b: [1, 2], a: 1})\")?, typeof(js::eval(\"({})\")?))\n\
             \x20   wok(\"BIG\", js::eval(\"10n ** 20n\")?, \"PROM\", js::eval(\"Promise.resolve(42)\")?)\n\
             \x20   match js::eval(\"null.x\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"GALAT:\", e) }\n\
             \x20   match js::eval(\"Promise.reject(new RangeError('habis'))\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"GALAT:\", e) }",
        );
        harus(&k, &["T int 1", "T float 2.5", "T float inf", "T float NaN", "T string héé",
                    "T nil nil", "T bool true", "OBJ dict {a: 1, b: [1, 2]} dict",
                    "BIG 100000000000000000000 PROM 42",
                    "GALAT: TypeError: Cannot read properties of null", "GALAT: RangeError: habis",
                    "SELESAI"]);
        assert!(!k.stdout.contains("DIAM:"), "{}", k.stdout);
    }

    /// Yang ditulis kode JavaScript ke stdout pergi ke stderr: stdout adalah kawat
    /// protokol, dan `process.stdout.write` dulu merusaknya.
    #[test]
    fn stdout_javascript_tidak_merusak_protokol() {
        lewati_tanpa_node!();
        let k = jalankan(
            "    js::exec(\"console.log('GEMA', {a: 1}); process.stdout.write('TULIS\\\\n')\")?\n\
             \x20   wok(\"UTUH:\", js::eval(\"'masih bicara'\")?)",
        );
        harus(&k, &["UTUH: masih bicara", "SELESAI"]);
        assert!(k.stderr.contains("GEMA { a: 1 }") && k.stderr.contains("TULIS"),
                "keluaran JavaScript harus terlihat di stderr:\n{}", k.stderr);
        assert!(!k.stdout.contains("GEMA") && !k.stdout.contains("TULIS"), "{}", k.stdout);
    }

    /// Callback JRG SINKRON: pembanding `sort` benar-benar mengurutkan (dulu Promise
    /// dibaca NaN, jadi urutannya tidak berubah tanpa pesan), galatnya merambat
    /// sebagai `Err` alih-alih mematikan worker, dan `js::` dari dalamnya ditolak.
    #[test]
    fn callback_jrg_sinkron() {
        lewati_tanpa_node!();
        let k = jalankan(
            "    wok(\"SORT:\", js::method(js::ref(\"[3, 1, 2]\")?, \"sort\", |a, b| a - b)?)\n\
             \x20   wok(\"MAP:\", js::method(js::ref(\"[1, 2, 3]\")?, \"map\", |x, i, s| x * 10)?)\n\
             \x20   match js::method(js::ref(\"[1]\")?, \"map\", |x, i, s| x / 0) { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"GAGAL-CB:\", e) }\n\
             \x20   wok(\"MASIH:\", js::eval(\"1 + 1\")?)\n\
             \x20   wok(\"THEN:\", js::eval(\"Promise.resolve(5).then(x => x * 2)\")?)\n\
             \x20   let r = js::method(js::ref(\"[1]\")?, \"map\", |x, i, s| {\n\
             \x20       return match js::eval(\"1\") { Ok(_) => \"dilayani\", Err(e) => \"ditolak\" }\n\
             \x20   })?\n\
             \x20   wok(\"REENTRAN:\", r)",
        );
        harus(&k, &["SORT: [1, 2, 3]", "MAP: [10, 20, 30]", "GAGAL-CB: Error:", "MASIH: 2",
                    "THEN: 10", "REENTRAN: [ditolak]", "SELESAI"]);
    }

    /// Galat dari kode yang jalan SESUDAH permintaannya (timer, Promise lepas) tidak
    /// mematikan worker diam-diam: ia jadi `Err` di panggilan berikutnya.
    #[test]
    fn galat_di_luar_permintaan_dilaporkan() {
        lewati_tanpa_node!();
        let k = jalankan(
            // Timer dan putaran sibuknya di SATU exec: timer baru bisa jalan sesudah
            // jawaban exec terkirim, jadi galatnya pasti jatuh ke permintaan berikutnya.
            "    js::exec(\"setTimeout(() => { throw new Error('bom-timer') }, 0); const t0 = Date.now(); while (Date.now() - t0 < 20) {}\")?\n\
             \x20   match js::eval(\"1\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"TIMER:\", e) }\n\
             \x20   js::exec(\"Promise.reject(new Error('lepas'))\")?\n\
             \x20   match js::eval(\"3\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"REJ:\", e) }\n\
             \x20   wok(\"LAGI:\", js::eval(\"2\")?)",
        );
        harus(&k, &["TIMER: Error: bom-timer", "REJ: Error: lepas", "LAGI: 2", "SELESAI"]);
    }

    /// `process.exit` dilaporkan, panggilan berikutnya memulai worker baru, handle
    /// lamanya ditolak menurut sesinya, dan rekursi tak berujung jadi `Err`.
    #[test]
    fn exit_dan_rekursi_dilaporkan_lalu_pulih() {
        lewati_tanpa_node!();
        let k = jalankan(
            "    let h = js::ref(\"({x: 1})\")?\n\
             \x20   match js::exec(\"process.exit(3)\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"EXIT:\", e) }\n\
             \x20   wok(\"PULIH:\", js::eval(\"2 + 2\")?)\n\
             \x20   match js::get(h, \"x\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"BASI:\", e) }\n\
             \x20   match js::eval(\"(function f() { return f() })()\") { Ok(v) => wok(\"DIAM:\", v), Err(e) => wok(\"REKURSI:\", e) }",
        );
        harus(&k, &["EXIT: worker javascript berhenti tanpa menjawab (kode keluar 3)", "PULIH: 4",
                    "BASI: js::get: handle javascript ini dari sesi worker sebelumnya",
                    "REKURSI: RangeError: Maximum call stack size exceeded", "SELESAI"]);
        assert!(!k.stdout.contains("DIAM:"), "{}", k.stdout);
    }

    /// Handle dilepas otomatis saat pemegang terakhirnya mati — janji yang bisa DIPERIKSA.
    #[test]
    fn handle_dilepas_otomatis() {
        lewati_tanpa_node!();
        let k = jalankan(
            "    let tetap = js::ref(\"({n: 0})\")?\n\
             \x20   for i in range(0, 50) { let h = js::ref(\"({n: 1})\")? }\n\
             \x20   js::eval(\"1\")?\n\
             \x20   wok(\"SISA:\", js::refs()?)",
        );
        harus(&k, &["SISA: 1", "SELESAI"]);
    }

    /// Bilangan bulat di luar ±2^53 DITOLAK: `JSON.parse` membacanya sebagai float
    /// 64 bit, jadi `9007199254740993` dulu tiba di Node sebagai `…992`.
    #[test]
    fn integer_di_luar_angka_aman_ditolak() {
        lewati_tanpa_node!();
        let k = jalankan(
            "    wok(\"AMAN:\", js::call(\"util\", \"inspect\", 9007199254740991))\n\
             \x20   wok(\"LEWAT:\", js::call(\"util\", \"inspect\", [1, 9007199254740993]))",
        );
        harus(&k, &["AMAN: Result::Ok(9007199254740991)",
                    "LEWAT: Result::Err(argumen ke-1 yang diteruskan ke JavaScript memuat bilangan bulat 9007199254740993 di luar jangkauan angka aman JavaScript"]);
    }
}
