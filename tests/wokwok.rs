// tests/wokwok.rs — tests/wokwok.rs — mengunci `wokwok::`, library numerik JRG.

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

fn jalankan(badan: &str) -> (String, bool) {
    let dir = crate::bersama::dir_unik("jrg-wokwok");
    std::fs::create_dir_all(&dir).expect("bikin dir");
    let file = dir.join("uji.jrg");
    // `lewat`/`lewat2` dipasang di tiap program supaya kasus alias bisa
    // menyeberangkan nilai lewat batas fungsi tanpa tiap kasus perlu
    // mendeklarasikannya sendiri.
    std::fs::write(&file, format!(
        "package main\nfn lewat(x) {{ return x }}\nfn lewat2(x) {{ return x }}\n\
         pub fn main() {{\n{}\n}}\n", badan))
        .expect("tulis sumber");

    let mut keluaran = Vec::new();
    let mut sukses = true;
    for vm in [false, true] {
        let mut c = Command::new(jrgc());
        c.arg(&file).arg("--sopan");
        if vm { c.arg("--vm"); }
        let out = c.output().expect("jalankan jrgc");
        keluaran.push(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
        sukses &= out.status.success();
    }
    let _ = std::fs::remove_dir_all(&dir);
    // Keluaran error memuat kutipan sumber di interpreter dan tidak di VM,
    // jadi yang dibandingkan cuma keluaran program yang BERHASIL — sama
    // dengan kontrak `PARITAS`.
    if sukses {
        assert_eq!(
            keluaran[0], keluaran[1],
            "wokwok menjawab beda antar-backend, padahal implementasinya satu:\n\
             interp:\n{}\nvm:\n{}", keluaran[0], keluaran[1]
        );
    }
    (keluaran[0].clone(), sukses)
}

fn harus(badan: &str, potongan: &[&str]) {
    let (keluaran, sukses) = jalankan(badan);
    assert!(sukses, "program gagal:\n{}", keluaran);
    for p in potongan {
        assert!(
            keluaran.contains(p),
            "keluaran tidak memuat `{}`:\n{}", p, keluaran
        );
    }
}

fn harus_tolak(badan: &str, kunci: &[&str]) {
    let (keluaran, sukses) = jalankan(badan);
    assert!(!sukses, "program ini seharusnya DITOLAK, tapi jalan:\n{}", keluaran);
    for k in kunci {
        assert!(
            keluaran.to_lowercase().contains(&k.to_lowercase()),
            "penolakannya tidak menyebut `{}`:\n{}", k, keluaran
        );
    }
}

// ─── 1. Kebenaran ────────────────────────────────────────────────────────

#[test]
fn pembuatan_dan_bentuk() {
    harus(
        r#"    wok(wokwok::array([1, 2, 3]))
    wok(wokwok::zeros([2, 2]))
    wok(wokwok::ones([3]))
    wok(wokwok::full([2], 7.5))
    wok(wokwok::arange(5))
    wok(wokwok::arange(2, 8, 2))
    wok(wokwok::linspace(0, 1, 5))
    wok(wokwok::eye(2))
    wok(wokwok::shape(wokwok::zeros([2, 3])), wokwok::ndim(wokwok::zeros([2, 3])), wokwok::size(wokwok::zeros([2, 3])))"#,
        &["[1, 2, 3]", "[[0.0, 0.0], [0.0, 0.0]]", "[1.0, 1.0, 1.0]",
          "[7.5, 7.5]", "[0, 1, 2, 3, 4]", "[2, 4, 6]",
          "[0.0, 0.25, 0.5, 0.75, 1.0]", "[[1.0, 0.0], [0.0, 1.0]]",
          "[2, 3] 2 6"],
    );
}

#[test]
fn elemen_demi_elemen_dan_broadcasting() {
    harus(
        r#"    let a = wokwok::array([1, 2, 3])
    let m = wokwok::reshape(wokwok::arange(6), [2, 3])
    wok(wokwok::add(a, 10))
    wok(wokwok::sub(a, a))
    wok(wokwok::mul(a, a))
    wok(wokwok::div(a, 2))
    wok(wokwok::pow(a, 2))
    wok(wokwok::add(m, a))
    wok(wokwok::add(m, wokwok::reshape(wokwok::array([10, 20]), [2, 1])))
    wok(wokwok::gt(a, 1))
    wok(wokwok::where(wokwok::gt(a, 1), a, 0))"#,
        &[
            "[11, 12, 13]", "[0, 0, 0]", "[1, 4, 9]",
            // Pembagian SELALU pecahan, bahkan untuk dua integer: `3 / 2`
            // yang menjawab `1` adalah hasil salah yang diam.
            "[0.5, 1.0, 1.5]",
            "[1.0, 4.0, 9.0]",
            "[[1, 3, 5], [4, 6, 8]]",
            "[[10, 11, 12], [23, 24, 25]]",
            "[false, true, true]",
            "[0, 2, 3]",
        ],
    );
}

#[test]
fn reduksi_seluruh_dan_per_sumbu() {
    harus(
        r#"    let m = wokwok::reshape(wokwok::arange(6), [2, 3])
    wok(wokwok::sum(m), wokwok::mean(m), wokwok::min(m), wokwok::max(m))
    wok(wokwok::sum(m, 0))
    wok(wokwok::sum(m, 1))
    wok(wokwok::mean(m, 1))
    wok(wokwok::argmax(wokwok::array([3, 9, 1])), wokwok::argmin(wokwok::array([3, 9, 1])))
    wok(wokwok::any(wokwok::array([0, 0, 1])), wokwok::all(wokwok::array([0, 0, 1])))
    wok(wokwok::std(wokwok::array([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])))
    wok(wokwok::cumsum(wokwok::array([1, 2, 3])))
    wok(wokwok::sort(wokwok::array([3, 1, 2])), wokwok::argsort(wokwok::array([3, 1, 2])))"#,
        &["15 2.5 0 5", "[3, 5, 7]", "[3, 12]", "[1.0, 4.0]",
          "1 2", "true false", "2.0", "[1.0, 3.0, 6.0]", "[1, 2, 3] [1, 2, 0]"],
    );
}

/// Yang dijanjikan sebuah ANGKA menjawab angka, bukan larik 0-dimensi. Dulu
/// `wokwok::sum(a) == 3.0` menjawab `false` — tampil `3.0`, bertipe
/// `wokwok:f64` — dan `+ 1`, `as float`, `math::sqrt` semuanya menolaknya.
#[test]
fn reduksi_tanpa_sumbu_menjawab_angka_biasa() {
    harus(
        r#"    let a = wokwok::ones(3)
    let m = wokwok::array([[1.0, 2.0], [3.0, 4.0]])
    wok(typeof(wokwok::sum(a)), typeof(wokwok::sum(wokwok::arange(3))), typeof(wokwok::any(a)))
    wok(wokwok::sum(a) == 3.0, wokwok::sum(a) + 1, wokwok::mean(a) > 0.5)
    wok(math::sqrt(wokwok::sum(wokwok::full([4], 1.0))), str::format("{:.2}", wokwok::fsum(a)))
    wok(typeof(wokwok::det(m)), typeof(wokwok::trace(m)), typeof(wokwok::norm(a)))
    wok(typeof(wokwok::dot(a, a)), typeof(wokwok::fmean(a)), typeof(wokwok::argmax(a)))
    wok(typeof(wokwok::count(a)), typeof(wokwok::max(m)), wokwok::dot(a, a) * 2)
    wok(typeof(wokwok::sum(m, 0)), typeof(wokwok::matmul(m, m)), typeof(wokwok::dot(m, m)))"#,
        &["float int bool", "true 4.0 true", "2.0 3.00",
          "float float float", "float float int", "int float 6.0",
          "wokwok:f64 wokwok:f64 wokwok:f64"],
    );
}

#[test]
fn aljabar_linear() {
    harus(
        r#"    let b = wokwok::array([[1.0, 2.0], [3.0, 4.0]])
    wok(wokwok::det(b))
    wok(wokwok::trace(b))
    wok(wokwok::matmul(b, wokwok::eye(2)))
    wok(wokwok::solve(b, wokwok::array([1.0, 1.0])))
    wok(wokwok::dot(wokwok::array([1, 2, 3]), wokwok::array([4, 5, 6])))
    wok(wokwok::norm(wokwok::array([3.0, 4.0])))
    wok(wokwok::transpose(wokwok::reshape(wokwok::arange(6), [2, 3])))"#,
        &["-2.0", "5.0", "[[1.0, 2.0], [3.0, 4.0]]",
          "[-1.0, 1.0]", "32", "5.0", "[[0, 3], [1, 4], [2, 5]]"],
    );
}

/// `matmul` yang diparalelkan harus menjawab SAMA dengan yang tidak.
#[test]
fn matmul_besar_sama_dengan_hitungan_tangan() {
    harus(
        r#"    let n = 101
    let a = wokwok::ones([n, n])
    let c = wokwok::matmul(a, a)
    // Tiap elemen hasil = jumlah n buah 1.0 = n.
    wok(wokwok::min(c), wokwok::max(c), wokwok::shape(c))
    let i = wokwok::eye(n)
    let r = wokwok::random([n, n])
    // A @ I harus mengembalikan A persis, sampai bit terakhir.
    wok(wokwok::max(wokwok::abs(wokwok::sub(wokwok::matmul(r, i), r))))"#,
        &["101.0 101.0 [101, 101]", "0.0"],
    );
}

// ─── 2. Semantik nilai ───────────────────────────────────────────────────

/// Array numerik TIDAK boleh beralias.
#[test]
fn array_numerik_tidak_pernah_beralias() {
    harus(
        r#"    var a = wokwok::array([1.0, 2.0, 3.0])
    var b = a
    b = wokwok::set(b, 0, 99.0)
    wok("binding :", a, b)
    var c = wokwok::array([1.0, 2.0])
    let d = lewat(c)
    c = wokwok::set(c, 0, 7.0)
    wok("fungsi  :", c, d)
    var e = wokwok::array([1.0, 2.0])
    let f = [e, e]
    e = wokwok::set(e, 1, 5.0)
    wok("koleksi :", e, f[0], f[1])
    var g = wokwok::array([1.0])
    let h = || g
    g = wokwok::set(g, 0, 8.0)
    wok("closure :", g, h())"#,
        &[
            "binding : [1.0, 2.0, 3.0] [99.0, 2.0, 3.0]",
            "fungsi  : [7.0, 2.0] [1.0, 2.0]",
            "koleksi : [1.0, 5.0] [1.0, 2.0] [1.0, 2.0]",
            // Closure menangkap BINDING, bukan nilai — jadi di sini `h()`
            // memang HARUS ikut berubah. Aturan itu berlaku untuk semua
            // nilai, dan `Nd` tidak boleh jadi pengecualian diam-diam.
            "closure : [8.0] [8.0]",
        ],
    );
    harus("    wok(lewat2(1))", &["1"]);
}

#[test]
fn kesamaan_membandingkan_isi_bukan_penyimpanan() {
    harus(
        r#"    wok(wokwok::array([1, 2]) == wokwok::array([1, 2]))
    wok(wokwok::array([1, 2]) == wokwok::array([1, 3]))
    wok(wokwok::array([1, 2]) == wokwok::array([[1, 2]]))
    wok(typeof(wokwok::array([1])), typeof(wokwok::array([1.0])), typeof(wokwok::gt(wokwok::array([1]), 0)))"#,
        &["true", "false", "false", "wokwok:i64 wokwok:f64 wokwok:bool"],
    );
}

// ─── 3. Ketelitian ───────────────────────────────────────────────────────

/// Klaim di `reduce.rs`: penjumlahan berpasangan menjaga ketelitian, dan
/// `fsum` memberi f64 terdekat.
#[test]
fn penjumlahan_tetap_teliti_untuk_sejuta_elemen() {
    harus(
        r#"    let a = wokwok::full([1000000], 0.1)
    wok("sum :", wokwok::sum(a))
    wok("mean:", wokwok::mean(a))
    var c = wokwok::full([1000000], 1.0)
    c = wokwok::set(c, 0, 1.0e16)
    wok("fsum:", wokwok::fsum(c))"#,
        &[
            // Sejuta 0,1 dijumlahkan berpasangan menjawab 100000 TEPAT.
            // Penjumlahan berurutan biasa menjawab 100000.00000001334.
            "sum : 100000.0",
            "mean: 0.1",
            // f64 terdekat dari 1e16 + 999999. Diperiksa dengan
            // `math.fsum` Python: 1.0000000001e16.
            "fsum: 10000000001000000.0",
        ],
    );
}

// ─── 4. Penolakan ────────────────────────────────────────────────────────

#[test]
fn bentuk_yang_tidak_nyambung_ditolak_bukan_dipotong() {
    harus_tolak(
        "    wok(wokwok::add(wokwok::zeros([3]), wokwok::zeros([4])))",
        &["tidak bisa disejajarkan"],
    );
    harus_tolak(
        "    wok(wokwok::matmul(wokwok::zeros([2, 3]), wokwok::zeros([2, 3])))",
        &["tidak nyambung"],
    );
    harus_tolak(
        "    wok(wokwok::reshape(wokwok::arange(5), [2, 3]))",
        &["tidak muat"],
    );
    // Array bergerigi: bentuknya tidak persegi, jadi tidak ada array
    // n-dimensi yang bisa mewakilinya.
    harus_tolak(
        "    wok(wokwok::array([[1, 2], [3]]))",
        &["tidak persegi"],
    );
    harus_tolak(
        "    wok(wokwok::det(wokwok::zeros([2, 3])))",
        &["persegi"],
    );
    // Singular: tidak ada balikan, dan menjawab angka apa pun akan
    // berbohong.
    harus_tolak(
        "    wok(wokwok::inv(wokwok::zeros([2, 2])))",
        &["singular"],
    );
    harus_tolak(
        "    wok(wokwok::sum(wokwok::zeros([2, 2]), 5))",
        &["sumbu 5 di luar jangkauan"],
    );
    harus_tolak(
        "    wok(wokwok::get(wokwok::arange(3), 9))",
        &["di luar batas"],
    );
    // Himpunan kosong tidak punya nilai terkecil; mengarang jawaban untuk
    // itu adalah cara paling halus untuk berbohong.
    harus_tolak(
        "    wok(wokwok::min(wokwok::zeros([0])))",
        &["kosong"],
    );
    // Integer 64 bit dijaga sama ketatnya dengan integer bahasa.
    harus_tolak(
        "    let a = wokwok::full([2], 9000000000000000000.0)\n    \
             wok(wokwok::add(wokwok::array([9000000000000000000, 9000000000000000000]), \
             wokwok::array([9000000000000000000, 1])))",
        &["kelewat besar"],
    );
}

#[test]
fn nan_menular_bukan_diam_diam_kalah() {
    harus(
        r#"    let n = 0.0 / 0.0
    let a = wokwok::array([1.0, n, 3.0])
    wok(wokwok::min(a), wokwok::max(a))
    wok(wokwok::sort(wokwok::array([3.0, n, 1.0])))"#,
        // Min/maks yang MELEWATI NaN akan menjawab angka yang tampak sah
        // untuk data yang jelas rusak.
        &["NaN NaN", "[1.0, 3.0, NaN]"],
    );
}

/// `suara::`: bunyi adalah larik wokwok mono 44100 Hz. Nilai sampelnya dihitung
/// tangan (11025 Hz = seperempat laju, jadi sinusnya tepat 0, 1, 0, -1).
mod suara {
    use super::*;

    #[test]
    fn pembangkit_menjawab_sampel_yang_benar() {
        harus(r#"
    let s = suara::nada(11025, 10)
    wok("N", wokwok::size(s), suara::durasi_ms(s))
    wok("SINUS", math::round(wokwok::get(s, 1) * 1000.0), math::round(wokwok::get(s, 3) * 1000.0))
    let k = suara::nada(441, 10, "kotak")
    wok("KOTAK", wokwok::get(k, 0), wokwok::get(k, 49), wokwok::get(k, 50), wokwok::get(k, 99))
    let g = suara::nada(441, 10, "gergaji")
    wok("GERGAJI", wokwok::get(g, 0), wokwok::get(g, 50))
    let t = suara::nada(441, 10, "segitiga")
    wok("SEGITIGA", wokwok::get(t, 0), wokwok::get(t, 25), wokwok::get(t, 75))
    let e = suara::selubung(suara::nada(441, 10, "kotak"), 1, 1)
    wok("SELUBUNG", wokwok::get(e, 0), wokwok::get(e, 22), wokwok::get(e, 440))
    wok("SAMBUNG", wokwok::size(suara::sambung(suara::sunyi(10), suara::nada(441, 20))))
    let d1 = suara::derau(5, 7)
    let d2 = suara::derau(5, 7)
    let d3 = suara::derau(5, 8)
    wok("DERAU", wokwok::to_list(d1) == wokwok::to_list(d2), wokwok::to_list(d1) == wokwok::to_list(d3),
        wokwok::fsum(wokwok::where(wokwok::gt(wokwok::abs(d1), 1.0), 1, 0)))
    let campur = wokwok::add(wokwok::mul(suara::nada(441, 10, "kotak"), 0.5), suara::sunyi(10))
    wok("CAMPUR", wokwok::get(campur, 0))
"#, &["N 441 10.0", "SINUS 1000.0 -1000.0", "KOTAK 1.0 1.0 -1.0 -1.0", "GERGAJI -1.0 0.0",
             "SEGITIGA -1.0 0.0 0.0", "SELUBUNG 0.0 0.5 0.0", "SAMBUNG 1323", "DERAU true false 0", "CAMPUR 0.5"]);
    }

    #[test]
    fn salah_pakai_ditolak_dengan_sebabnya() {
        for (badan, kunci) in [
            ("wok(suara::nada(0, 10))", "frekuensi harus di atas 0"),
            ("wok(suara::nada(30000, 10))", "separuh laju sampel"),
            (r#"wok(suara::nada(440, 10, "kotek"))"#, "bentuk `kotek` tidak dikenal"),
            ("wok(suara::sunyi(-1))", "durasi tidak boleh negatif"),
            ("wok(suara::sunyi(700000))", "melewati batas"),
            ("wok(suara::selubung(suara::sunyi(10), 8, 8))", "lebih panjang dari bunyinya"),
            ("wok(suara::durasi_ms(wokwok::zeros([2, 2])))", "harus bunyi 1-D"),
            ("wok(suara::sambung([1, 2], suara::sunyi(1)))", "harus bunyi (larik `wokwok::`"),
            ("wok(suara::derau(10, 1.5))", "benih harus bilangan bulat"),
        ] {
            harus_tolak(badan, &[kunci]);
        }
    }

    /// WAV yang ditulis dibaca modul `wave` Python — pembaca yang bukan milik proyek ini —
    /// termasuk kliping; lalu `muat_wav` membaca balik, dan stereo buatan Python dirata-rata.
    #[test]
    fn wav_dibaca_python_dan_dibaca_balik() {
        if std::process::Command::new("python3").arg("-c").arg("import wave").output()
            .map(|o| !o.status.success()).unwrap_or(true) {
            eprintln!("dilewati: butuh python3 dengan modul wave");
            return;
        }
        let dir = crate::bersama::dir_unik("jrg-suara");
        std::fs::create_dir_all(&dir).expect("bikin dir");
        let a = dir.join("a.wav");
        let stereo = dir.join("s.wav");
        let delapan = dir.join("d.wav");
        let py = format!(r#"
import wave, struct
with wave.open("{s}", "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(44100)
    w.writeframes(struct.pack("<4h", 32767, -32767, 16384, 16384))
with wave.open("{d}", "wb") as w:
    w.setnchannels(1); w.setsampwidth(1); w.setframerate(44100)
    w.writeframes(bytes([128, 200]))
"#, s = stereo.display(), d = delapan.display());
        assert!(std::process::Command::new("python3").arg("-c").arg(&py).status().expect("python").success());
        for vm in [false, true] {
            let src = format!(r#"package main
pub fn main() {{
    let k = wokwok::mul(suara::nada(441, 10, "kotak"), 2)
    wok(suara::simpan_wav(k, "{a}"))
    match suara::muat_wav("{a}") {{
        Result::Ok(b) => wok("BALIK", wokwok::size(b), wokwok::get(b, 0), wokwok::get(b, 99))
        Result::Err(e) => wok("GAGAL", e)
    }}
    match suara::muat_wav("{s}") {{
        Result::Ok(b) => wok("STEREO", wokwok::size(b), wokwok::get(b, 0), math::round(wokwok::get(b, 1) * 1000.0))
        Result::Err(e) => wok("GAGAL", e)
    }}
    match suara::muat_wav("{d}") {{
        Result::Ok(b) => wok("SALAH", b)
        Result::Err(e) => wok("TOLAK", e)
    }}
}}
"#, a = a.display(), s = stereo.display(), d = delapan.display());
            let f = dir.join("uji.jrg");
            std::fs::write(&f, src).expect("tulis");
            let mut c = Command::new(jrgc());
            c.arg(&f).arg("--sopan");
            if vm { c.arg("--vm"); }
            let out = c.output().expect("jalankan");
            let k = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(out.status.success(), "{}{}", k, String::from_utf8_lossy(&out.stderr));
            assert!(k.contains("Result::Ok(())"), "{}", k);
            assert!(k.contains("BALIK 441 1.0 -1.0"), "{}", k);
            assert!(k.contains("STEREO 2 0.0 500.0"), "{}", k);
            assert!(k.contains("TOLAK") && k.contains("cuma PCM 16-bit"), "{}", k);
            // Oracle: header dan sampelnya menurut `wave`, dengan nilai 2.0 dijepit ke 32767.
            let cek = std::process::Command::new("python3").arg("-c").arg(format!(r#"
import wave, struct
with wave.open("{a}") as w:
    n = w.getnframes()
    s = struct.unpack("<%dh" % n, w.readframes(n))
    print(w.getnchannels(), w.getsampwidth(), w.getframerate(), n, s[0], s[49], s[50], s[99])
"#, a = a.display())).output().expect("python");
            assert_eq!(String::from_utf8_lossy(&cek.stdout).trim(),
                "1 2 44100 441 32767 32767 -32767 -32767", "WAV salah menurut `wave`");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Jalur pemutar diuji dengan SUNYI — koneksi, tulis, dan kuras berjalan lengkap
/// tanpa satu suara pun terdengar; `putar_tunggu` menunggu, `putar` tidak.
#[test]
fn pemutar_sunyi_dan_server_yang_tidak_ada() {
    let dir = crate::bersama::dir_unik("jrg-suara-putar");
    std::fs::create_dir_all(&dir).expect("bikin dir");
    let f = dir.join("p.jrg");
    std::fs::write(&f, "package main\npub fn main() {\n  if !suara::tersedia() { wok(\"TIDAK TERSEDIA\")\n    return }\n\
        let t0 = time::now_ms()\n  wok(suara::putar_tunggu(suara::sunyi(300)))\n\
        wok(\"MENUNGGU\", time::now_ms() - t0 >= 250)\n  let t1 = time::now_ms()\n\
        wok(suara::putar(suara::sunyi(300)))\n  wok(\"LANGSUNG\", time::now_ms() - t1 < 250)\n}\n").expect("tulis");
    for vm in [false, true] {
        let mut c = Command::new(jrgc());
        c.arg(&f).arg("--sopan");
        if vm { c.arg("--vm"); }
        let k = String::from_utf8_lossy(&c.output().expect("jalankan").stdout).to_string();
        if k.contains("TIDAK TERSEDIA") {
            eprintln!("bagian pemutar dilewati: tidak ada server suara di sini");
        } else {
            assert_eq!(k.trim(), "Result::Ok(())\nMENUNGGU true\nResult::Ok(())\nLANGSUNG true", "di {}", vm);
        }
        // Server yang tidak ada: `Err` yang menyebut sebabnya, dan `tersedia()` menjawab false.
        let mut c = Command::new(jrgc());
        c.arg(&f).arg("--sopan").env("PULSE_SERVER", "unix:/tidak/ada");
        if vm { c.arg("--vm"); }
        let k = String::from_utf8_lossy(&c.output().expect("jalankan").stdout).to_string();
        assert_eq!(k.trim(), "TIDAK TERSEDIA", "di {}", vm);
        let g = dir.join("g.jrg");
        std::fs::write(&g, "package main\npub fn main() { wok(suara::putar_tunggu(suara::sunyi(10))) }\n").expect("tulis");
        let mut c = Command::new(jrgc());
        c.arg(&g).arg("--sopan").env("PULSE_SERVER", "unix:/tidak/ada");
        if vm { c.arg("--vm"); }
        let k = String::from_utf8_lossy(&c.output().expect("jalankan").stdout).to_string();
        assert!(k.contains("Result::Err(suara::putar_tunggu: tidak bisa terhubung ke server suara"), "{}", k);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
