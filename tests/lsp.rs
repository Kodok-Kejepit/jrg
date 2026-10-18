// tests/lsp.rs — tests/lsp.rs — mengunci language server.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

// ─── Alat ──────────────────────────────────────────────────────────────────

fn jrgm() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") { p.pop(); }
    p.join("jrgm")
}

fn jrgc() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") { p.pop(); }
    p.join("jrgc")
}

fn bingkai(isi: &str) -> Vec<u8> {
    let mut v = format!("Content-Length: {}\r\n\r\n", isi.len()).into_bytes();
    v.extend_from_slice(isi.as_bytes());
    v
}

/// Jalankan satu sesi LSP dan kembalikan tiap pesan yang diterima, mentah.
fn sesi(pesan: &[String]) -> Vec<String> {
    let j = jrgm();
    if !j.exists() { return Vec::new(); }

    let mut anak = Command::new(&j)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("gagal menjalankan jrgm lsp");

    {
        let masuk = anak.stdin.as_mut().unwrap();
        for p in pesan {
            masuk.write_all(&bingkai(p)).unwrap();
        }
    }
    let keluar = anak.wait_with_output().expect("jrgm lsp tidak selesai");
    let teks = String::from_utf8_lossy(&keluar.stdout).to_string();

    // Bongkar bingkai. Sengaja tidak memakai pustaka JSON: yang diuji
    // termasuk pembingkaiannya sendiri, jadi pembacanya harus berdiri di
    // luar barang yang diuji.
    let mut out = Vec::new();
    let mut sisa = teks.as_str();
    while let Some(i) = sisa.find("Content-Length:") {
        let sesudah = &sisa[i + "Content-Length:".len()..];
        let Some(j) = sesudah.find("\r\n\r\n") else { break };
        let Ok(n) = sesudah[..j].trim().parse::<usize>() else { break };
        let badan = &sesudah[j + 4..];
        if badan.len() < n { break; }
        out.push(badan[..n].to_string());
        sisa = &badan[n..];
    }
    out
}

fn permintaan(id: u32, metode: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{},"method":"{}","params":{}}}"#, id, metode, params)
}

fn notifikasi(metode: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","method":"{}","params":{}}}"#, metode, params)
}

fn json_teks(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn buka(uri: &str, src: &str) -> String {
    notifikasi(
        "textDocument/didOpen",
        &format!(
            r#"{{"textDocument":{{"uri":"{}","languageId":"jrg","version":1,"text":{}}}}}"#,
            uri,
            json_teks(src)
        ),
    )
}

fn init() -> Vec<String> {
    vec![
        permintaan(1, "initialize", r#"{"processId":null,"capabilities":{}}"#),
        notifikasi("initialized", "{}"),
    ]
}

fn di(uri: &str, baris: u32, kolom: u32) -> String {
    format!(
        r#"{{"textDocument":{{"uri":"{}"}},"position":{{"line":{},"character":{}}}}}"#,
        uri, baris, kolom
    )
}

/// Ambil angka bilangan bulat pertama sesudah `kunci` di dalam teks JSON.
fn angka(json: &str, kunci: &str) -> Option<i64> {
    let i = json.find(&format!("\"{}\":", kunci))? + kunci.len() + 3;
    let sisa = &json[i..];
    let n: String = sisa.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    n.parse().ok()
}

const CONTOH: &str = concat!(
    "package main\n",                              // 0
    "struct Titik { pub x: int }\n",               // 1
    "pub fn hitung_luas_total(a) { return a }\n",  // 2
    "pub fn luas(a, b) { return a * b }\n",        // 3
    "impl Titik {\n",                              // 4
    "    fn geser(self, d) { return d }\n",        // 5
    "}\n",                                         // 6
    "pub fn main() {\n",                           // 7
    "    wok(luas(2, 3))\n",                       // 8
    "}\n",                                         // 9
);

// ─── Sifat pokok ───────────────────────────────────────────────────────────

/// Bug yang menjadikan seluruh language server ini tidak berguna.
#[test]
fn menjawab_banyak_permintaan_dalam_satu_sesi() {
    if !jrgm().exists() {
        eprintln!("LEWAT: biner jrgm belum dibangun");
        return;
    }
    let mut m = init();
    m.push(buka("file:///a.jrg", CONTOH));
    m.push(permintaan(2, "textDocument/hover", &di("file:///a.jrg", 8, 5)));
    m.push(permintaan(3, "textDocument/definition", &di("file:///a.jrg", 8, 9)));
    m.push(permintaan(4, "textDocument/completion", &di("file:///a.jrg", 8, 4)));
    m.push(permintaan(5, "textDocument/documentSymbol",
        r#"{"textDocument":{"uri":"file:///a.jrg"}}"#));
    m.push(permintaan(6, "shutdown", "{}"));
    m.push(notifikasi("exit", "{}"));

    let jawab = sesi(&m);
    // 1 initialize + 1 publishDiagnostics + 4 permintaan + 1 shutdown = 7
    assert!(
        jawab.len() >= 7,
        "cuma {} jawaban untuk 6 permintaan + 1 notifikasi diagnostik.\n\
         Satu jawaban berarti bug pembingkaian kembali: badan pesan harus \
         dibaca TEPAT sebanyak Content-Length byte, bukan per baris.\n{:#?}",
        jawab.len(), jawab
    );
    for id in [1u32, 2, 3, 4, 5, 6] {
        assert!(
            jawab.iter().any(|j| j.contains(&format!("\"id\":{}", id))),
            "tidak ada jawaban untuk id {}", id
        );
    }
}

/// Diagnostik harus menunjuk baris dan kolom yang SAMA dengan caret `jrgc`.
#[test]
fn lokasi_diagnostik_sama_dengan_caret_jrgc() {
    if !jrgm().exists() || !jrgc().exists() {
        eprintln!("LEWAT: biner belum dibangun");
        return;
    }
    let kasus: &[(&str, &str)] = &[
        ("kurung tak ditutup",      "package main\npub fn main() {\n    wok(1\n}\n"),
        ("kurawal kurang",          "package main\npub fn main() {\n    let x = 1\n"),
        ("token asing",             "package main\npub fn main() {\n    let x = @@\n}\n"),
        ("string tak ditutup",      "package main\npub fn main() {\n    let s = \"belum\n}\n"),
        // Baris ber-non-ASCII sebelum errornya: kolom LSP dihitung dalam
        // satuan UTF-16, jadi kasus ini yang membedakan implementasi yang
        // benar dari yang kebetulan benar untuk ASCII.
        ("sesudah baris non-ASCII", "package main\npub fn main() {\n    let s = \"«jalan»\"\n    wok(\n}\n"),
    ];

    let tmp = std::env::temp_dir().join("jrg_lsp_diag");
    let _ = std::fs::create_dir_all(&tmp);

    for (nama, src) in kasus {
        // Caret compiler.
        let f = tmp.join("x.jrg");
        std::fs::write(&f, src).unwrap();
        let out = Command::new(jrgc())
            .arg(&f)
            .env("JRG_SOPAN", "1")
            .output()
            .unwrap();
        let teks = String::from_utf8_lossy(&out.stderr).to_string()
            + &String::from_utf8_lossy(&out.stdout);
        let Some(i) = teks.find("--> ") else { continue };
        let potong: Vec<&str> = teks[i..].lines().next().unwrap().rsplitn(3, ':').collect();
        let (baris_c, kolom_c): (u32, u32) = match potong.as_slice() {
            [k, b, _] => (b.parse().unwrap_or(0), k.parse().unwrap_or(0)),
            _ => continue,
        };

        // Diagnostik LSP.
        let mut m = init();
        m.push(buka("file:///x.jrg", src));
        let jawab = sesi(&m);
        let d = jawab
            .iter()
            .find(|j| j.contains("publishDiagnostics"))
            .unwrap_or_else(|| panic!("{}: tidak ada notifikasi diagnostik", nama));
        let baris_l = angka(d, "line").expect("tidak ada `line`") as u32 + 1;
        let kolom_l = angka(d, "character").expect("tidak ada `character`") as u32 + 1;

        assert_eq!(
            (baris_l, kolom_l),
            (baris_c, kolom_c),
            "{}: LSP melapor di {}:{}, jrgc di {}:{}.\n\
             Kalau LSP menjawab 1:1 untuk semua kasus, pencarian lokasinya \
             kembali mengorek teks pesan alih-alih memakai Span error.",
            nama, baris_l, kolom_l, baris_c, kolom_c
        );
    }
}

/// `definition` mencocokkan nama PERSIS.
#[test]
fn definition_tidak_mendarat_di_nama_yang_kebetulan_memuatnya() {
    if !jrgm().exists() { return; }
    let mut m = init();
    m.push(buka("file:///a.jrg", CONTOH));
    m.push(permintaan(2, "textDocument/definition", &di("file:///a.jrg", 8, 9)));
    let jawab = sesi(&m);
    let d = jawab.iter().find(|j| j.contains("\"id\":2")).expect("tidak ada jawaban");
    let baris = angka(d, "line").expect("tidak ada `line`");
    assert_eq!(
        baris, 3,
        "definition `luas` mendarat di baris {} — baris 2 berarti ia jatuh ke \
         `hitung_luas_total`, yaitu pencocokan substring yang lama.\n{}",
        baris, d
    );
}

/// Metode di dalam `impl` harus bisa ditemukan.
#[test]
fn metode_impl_ada_di_indeks() {
    if !jrgm().exists() { return; }
    let mut m = init();
    m.push(buka("file:///a.jrg", CONTOH));
    m.push(permintaan(2, "textDocument/documentSymbol",
        r#"{"textDocument":{"uri":"file:///a.jrg"}}"#));
    m.push(permintaan(3, "textDocument/definition", &di("file:///a.jrg", 5, 8)));
    let jawab = sesi(&m);

    let sim = jawab.iter().find(|j| j.contains("\"id\":2")).expect("tidak ada simbol");
    assert!(
        sim.contains("Titik::geser"),
        "metode `geser` tidak ada di daftar simbol:\n{}", sim
    );
    let def = jawab.iter().find(|j| j.contains("\"id\":3")).expect("tidak ada definition");
    assert_eq!(angka(def, "line"), Some(5), "definition metode salah baris:\n{}", def);
}

/// Hover menjawab untuk builtin DAN untuk simbol buatan pengguna.
#[test]
fn hover_menjawab_builtin_dan_simbol_sendiri() {
    if !jrgm().exists() { return; }
    let mut m = init();
    m.push(buka("file:///a.jrg", CONTOH));
    m.push(permintaan(2, "textDocument/hover", &di("file:///a.jrg", 8, 5)));  // wok
    m.push(permintaan(3, "textDocument/hover", &di("file:///a.jrg", 8, 9)));  // luas
    m.push(permintaan(4, "textDocument/hover", &di("file:///a.jrg", 1, 8)));  // Titik
    let jawab = sesi(&m);
    for (id, apa) in [(2u32, "wok"), (3, "luas"), (4, "Titik")] {
        let h = jawab.iter().find(|j| j.contains(&format!("\"id\":{}", id)))
            .unwrap_or_else(|| panic!("tidak ada jawaban hover untuk {}", apa));
        assert!(
            !h.contains("\"result\":null"),
            "hover atas `{}` menjawab null:\n{}", apa, h
        );
    }
}

/// Header rusak tidak boleh membuat server mati diam atau menggantung.
#[test]
fn header_rusak_tidak_menjatuhkan_server() {
    let j = jrgm();
    if !j.exists() { return; }
    let mut anak = Command::new(&j)
        .arg("lsp")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().unwrap();
    {
        let masuk = anak.stdin.as_mut().unwrap();
        let _ = masuk.write_all(b"Content-Length: 99999999999999\r\n\r\n{}");
    }
    let keluar = anak.wait_with_output().expect("server menggantung pada header rusak");
    assert!(
        keluar.status.code().is_some(),
        "server tidak keluar dengan wajar pada header rusak"
    );
}

/// Pelengkapan otomatis harus menawarkan pustaka standarnya.
#[test]
fn completion_menawarkan_pustaka_standar() {
    let src = "package main\npub fn main() { str:: }\n";
    let mut m = vec![
        permintaan(1, "initialize", r#"{"capabilities":{}}"#),
        buka("file:///c.jrg", src),
    ];
    // Kolom 21 = tepat sesudah `str::` di baris kedua.
    m.push(permintaan(2, "textDocument/completion", &di("file:///c.jrg", 1, 21)));
    // Awalan kosong: kursor di awal badan.
    m.push(permintaan(3, "textDocument/completion", &di("file:///c.jrg", 1, 16)));
    let balas = sesi(&m);
    if balas.is_empty() { eprintln!("lewati: jrgm tidak ada"); return; }

    let cari = |id: &str| -> String {
        balas.iter().find(|b| b.contains(&format!("\"id\":{}", id)))
            .cloned().unwrap_or_default()
    };

    let r2 = cari("2");
    assert!(!r2.is_empty(), "tidak ada respons untuk permintaan completion");
    for wajib in ["str::upper", "str::split", "str::format"] {
        assert!(r2.contains(wajib), "completion `str::` tidak memuat `{}`:\n{}", wajib, r2);
    }
    // Penyaringnya harus benar-benar MENYARING, bukan cuma memperbesar
    // batasnya: sebuah awalan `str::` tidak boleh membawa serta `math::`.
    assert!(!r2.contains("math::sqrt"),
        "awalan `str::` ikut membawa `math::` — penyaringnya tidak menyaring:\n{}", r2);
    // Satu namespace harus muat UTUH.
    assert!(r2.contains("\"isIncomplete\":false") || r2.contains("\"isIncomplete\": false"),
        "satu namespace (`str::`) tidak muat dalam satu respons — naikkan BATAS_COMPLETION:\n{}", r2);

    // Awalan kosong boleh dipotong, tapi harus MENGAKUINYA. `isIncomplete:
    // false` atas daftar yang dipotong adalah pernyataan yang salah, dan
    // klien bertindak atas dasar itu.
    let r3 = cari("3");
    assert!(!r3.is_empty(), "tidak ada respons untuk completion awalan kosong");
    assert!(r3.contains("\"isIncomplete\":true") || r3.contains("\"isIncomplete\": true"),
        "daftar dipotong tapi dijawab lengkap:\n{}", r3);
}

/// Hover harus menjawab untuk SELURUH permukaan builtin, bukan 15 nama.
#[test]
fn hover_menjawab_dari_registry_bukan_daftar_tangan() {
    let src = "package main\npub fn main() { str::upper(\"a\") }\n";
    let mut m = vec![
        permintaan(1, "initialize", r#"{"capabilities":{}}"#),
        buka("file:///h.jrg", src),
    ];
    // Kolom 24 jatuh di tengah `upper`.
    m.push(permintaan(2, "textDocument/hover", &di("file:///h.jrg", 1, 24)));
    let balas = sesi(&m);
    if balas.is_empty() { eprintln!("lewati: jrgm tidak ada"); return; }

    let r = balas.iter().find(|b| b.contains("\"id\":2")).cloned().unwrap_or_default();
    assert!(!r.is_empty(), "tidak ada respons hover");
    // Yang dituntut bukan kata-katanya melainkan bahwa ia MENJAWAB: sebuah
    // tes yang mengunci teksnya akan memaksa penulis berikutnya menyalin
    // kalimat, yang justru cara salinan ketiga itu lahir.
    assert!(!r.contains("\"result\":null") && r.contains("str::upper"),
        "hover atas `str::upper` tidak menjawab:\n{}", r);
}
