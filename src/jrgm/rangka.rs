// src/jrgm/rangka.rs — kerangka proyek baru

use std::path::PathBuf;
use std::path::Path;

// ─── Commands ─────────────────────────────────────────────────────────────

/// Kerangka aplikasi web yang LANGSUNG JALAN: route, JSON, template yang
/// melolosi HTML sendiri, dan 404. Dijalankan tesnya sebelum boleh ada.
pub(crate) fn rangka_web(name: &str) -> String {
    format!(
        r#"// {name} — server HTTP. Jalankan: jrgm run, lalu buka http://127.0.0.1:8080
package main

// `{{{{ x }}}}` melolosi HTML sendiri; `{{{{#each}}}}` mengulang; `{{{{#if}}}}` memilih.
const HALAMAN: string = """<!doctype html><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>{{{{ judul }}}}</title>
<style>body{{font-family:system-ui;max-width:38rem;margin:3rem auto;padding:0 1rem;line-height:1.6}}</style>
<h1>{{{{ judul }}}}</h1>
<p>{{{{ pesan }}}}</p>
<ul>
{{{{#each tautan}}}}  <li><a href="{{{{ url }}}}">{{{{ teks }}}}</a></li>
{{{{/each}}}}</ul>"""

pub fn main() {{
    let r = dok::router_new()

    dok::router_get(r, "/", fn(req) => beranda())
    dok::router_get(r, "/halo/:nama", fn(req) => halo(req))
    dok::router_get(r, "/api", fn(req) => dok::json_response({{"ok": true}}))
    dok::router_not_found(r, fn(req) => dok::response(404, "halaman tidak ada"))

    wok("server jalan di http://127.0.0.1:8080")
    dok::serve(r, 8080, "127.0.0.1")
}}

fn beranda() {{
    let isi = dok::render(HALAMAN, {{
        "judul": "{name}",
        "pesan": "Halaman ini dibuat `jrgm new --kind web`.",
        "tautan": [
            {{"url": "/api", "teks": "Contoh JSON"}},
            {{"url": "/halo/dunia", "teks": "Rute berparameter"}},
        ],
    }}).unwrap_or("<p>gagal render</p>")
    return dok::html(isi)
}}

fn halo(req) {{
    // Nama datang dari URL — sesuatu yang ditulis orang lain. `{{{{ x }}}}`
    // melolosinya sebelum dipasang, jadi `<script>` tampil sebagai teks.
    let nama = req.params["nama"]
    return dok::html(dok::render(HALAMAN, {{
        "judul": "Halo, " + nama,
        "pesan": "Nama di URL dilolosi sebelum dipasang ke halaman.",
        "tautan": [{{"url": "/", "teks": "Kembali"}}],
    }}).unwrap_or(""))
}}
"#, name = name)
}

/// Kerangka aplikasi Android: ketuk lingkaran, skor bertambah. Jalan juga di desktop (mode `"tab"`).
pub(crate) fn rangka_android(name: &str) -> String {
    format!(
        r#"// {name} — aplikasi Android dari JRG.
// Bangun APK: jrgm apk     Pasang ke HP lalu jalankan: jrgm run --android
// Di desktop: jrgm run (jendela biasa; tetikus = sentuhan)
package main

const LEBAR = 180
const TINGGI = 320

fn acak(benih: int) -> int {{
    return (benih * 1103515245 + 12345) % 2147483648
}}

pub fn main() {{
    let c = Canvas::new(LEBAR, TINGGI, "{name}", "tab")
    var skor = 0
    var benih = 7
    var x: float = 90.0
    var y: float = 160.0
    var tadi = false
    while canvas::update(c) {{
        let m = canvas::get_mouse_pos(c)
        let tekan = canvas::get_mouse_button(c, 0)
        // Sekali per sentuhan: dihitung saat jari baru menempel.
        if tekan && !tadi {{
            let dx = m.x - x
            let dy = m.y - y
            if dx * dx + dy * dy < 900.0 {{
                skor = skor + 1
                benih = acak(benih)
                x = (20 + benih % (LEBAR - 40)) as float
                benih = acak(benih)
                y = (60 + benih % (TINGGI - 100)) as float
            }}
        }}
        tadi = tekan
        canvas::clear(c, 0x101828)
        canvas::fill_circle(c, x, y, 24, 0xF59E0B)
        canvas::draw_text_scaled(c, 8, 8, "SKOR " + str(skor), 2, 0xFFFFFF)
        time::sleep_ms(16)
    }}
    wok("skor akhir:", skor)
}}
"#, name = name)
}

pub(crate) fn cmd_new(name: &str, kind: &str) -> Result<(), String> {
    let root = PathBuf::from(name);
    std::fs::create_dir_all(root.join("src"))
        .map_err(|e| format!("tidak bisa buat direktori project: {}", jrgc::host::galat_io(&e)))?;

    if !["bin", "lib", "web", "android"].contains(&kind) {
        return Err(format!(
            "jenis project `{}` tidak dikenal — yang ada: `bin` (program biasa), \
             `lib` (pustaka), `web` (server HTTP yang langsung jalan), `android` (aplikasi HP)", kind));
    }
    let entry = if kind == "lib" { "src/lib.jrg" } else { "src/main.jrg" };

    let entry_content = if kind == "web" {
        rangka_web(name)
    } else if kind == "android" {
        rangka_android(name)
    } else if kind == "lib" {
        format!(
            r#"// {} — pustaka
package {}

/// Sapa seseorang. Doc-comment ini muncul di `jrgc --doc`.
pub fn sapa(nama: string) -> string {{
    return "Halo, " + nama + "!"
}}
"#,
            name, name
        )
    } else {
        // TANPA `import "std/fmt"`: `fmt` bukan modul `std/` dan `wok` bawaan, jadi
        // baris itu tidak melakukan apa-apa.
        format!(
            // Raw string dengan baris sungguhan: versi pertama memakai sambungan `\` dan
            // indentasi sumbernya ikut masuk ke berkas yang ditulis — berkas pertama …
            r#"// {}
package main

pub fn main() {{
    wok("Halo dari JawaRustGo++!")

    // `for i, x in a` memberi INDEKS dan NILAI sekaligus.
    let angka = [1, 2, 3]
    for i, x in angka {{
        wok(i, "->", x * x)
    }}
}}
"#,
            name
        )
    };

    std::fs::write(root.join(entry), entry_content)
        .map_err(|e| format!("tidak bisa tulis entry file: {}", jrgc::host::galat_io(&e)))?;

    let manifest = format!(
        "[package]\nname        = \"{name}\"\nversion     = \"0.1.0\"\n\
         edition     = \"jrg2025\"\nentry       = \"{entry}\"\n\n\
         # Dependensi: direktori lokal, atau repositori git (dikunci di jrgm.lock):\n\
         #   jrgm add bantu --path ../bantu\n\
         #   jrgm add bantu --git https://contoh.org/bantu.git --version 0.1.0\n\
         [dependencies]\n\n\
         # Package Python yang dipakai lewat py::import/py::call/py::call_kw.\n\
         # jrgm juga men-scan {entry} otomatis dan menambah apa pun yang\n\
         # terdeteksi di sana — baris ini untuk kasus yang scan tidak bisa\n\
         # tangkap (mis. py::exec(\"import ...\")) atau supaya eksplisit.\n\
         # [python]\n\
         # packages = [\"requests\", \"numpy\"]\n"
    );
    let manifest = if kind == "web" {
        format!("{manifest}\n\
                 # Jadikan aplikasi Android: `jrgm apk`. Halaman ini tampil di WebView, dan program ini\n\
                 # jalan di dalam HP sebagai pelayannya (jrgc ikut di dalam APK).\n\
                 [android]\n\
                 tampilan = \"web\"\n\
                 port     = 8080\n")
    } else { manifest };
    let manifest = if kind == "android" {
        let paket = jrgc::llvm::apk::nama_paket(name);
        format!("{manifest}\n\
                 # Identitas APK untuk `jrgm apk` dan `jrgm run --android`.\n\
                 [android]\n\
                 nama      = \"{name}\"\n\
                 paket     = \"{paket}\"\n\
                 versi     = \"0.1.0\"          # kode versi diturunkan: 0.1.0 -> 1000\n\
                 orientasi = \"portrait\"       # bebas | portrait | landscape\n\
                 abi       = [\"arm64-v8a\"]    # tambah \"x86_64\" untuk emulator\n\
                 # ikon    = \"aset/ikon.png\"  # PNG persegi, mis. 512x512\n\
                 # izin    = [\"internet\"]      # internet | jaringan | wifi | getar | tetap_menyala\n\
                 # pustaka = [\"native/{{abi}}/libhitung.so\"]  # .so jadi milik proyek\n\
                 #\n\
                 # Atau biar jrgm yang mengompilasi sumbernya per ABI:\n\
                 # [android.pustaka_c]\n\
                 # libhitung = [\"native/hitung.c\"]\n\
                 # [android.pustaka_rust]\n\
                 # librumus = \"rust/rumus\"      # crate dengan crate-type = [\"cdylib\"]\n\
                 # Kunci rilis: `jrgm kunci` sekali, lalu `jrgm apk --rilis`.\n")
    } else { manifest };
    std::fs::write(root.join("jrgm.toml"), manifest)
        .map_err(|e| format!("tidak bisa tulis jrgm.toml: {}", jrgc::host::galat_io(&e)))?;

    // Kunci rilis Android tidak boleh ikut ke repositori.
    let abaikan = if kind == "android" { "/target\njrgm.lock\n*.jks\n*.keystore\n" } else { "/target\njrgm.lock\n" };
    std::fs::write(root.join(".gitignore"), abaikan)
        .map_err(|e| format!("tidak bisa tulis .gitignore: {}", jrgc::host::galat_io(&e)))?;

    // Sebuah berkas uji contoh, supaya `jrgm test` berhasil sejak menit pertama
    // dan konvensinya terlihat tanpa perlu dicari di dokumentasi.
    std::fs::create_dir_all(root.join("tests"))
        .map_err(|e| format!("tidak bisa buat direktori tests: {}", jrgc::host::galat_io(&e)))?;
    let contoh_uji = format!(
        "// Test untuk `{name}`. Dijalankan dengan `jrgm test`.\n\
         //\n\
         // Sebuah fungsi dianggap test kalau namanya diawali `test_` atau\n\
         // `uji_`. Tidak perlu `main` — runner-nya memanggil tiap test\n\
         // langsung, dan menangkap kegagalannya supaya yang berikutnya tetap\n\
         // jalan.\n\
         package uji_{name}\n\n\
         pub fn test_contoh() {{\n    assert_eq(1 + 1, 2)\n}}\n\n\
         // Untuk kumpulan cek yang mau dilaporkan sekaligus, bukan berhenti\n\
         // di kegagalan pertama:\n\
         //\n\
         //     import \"std/testing\"\n\
         //     pub fn uji_banyak() {{\n\
         //         testing::cek_sama(\"tambah\", 1 + 1, 2)\n\
         //         assert_eq(testing::hitungan().1, 0)\n\
         //     }}\n"
    );
    std::fs::write(root.join("tests").join("dasar.jrg"), contoh_uji)
        .map_err(|e| format!("tidak bisa tulis contoh test: {}", jrgc::host::galat_io(&e)))?;

    eprintln!("Created `{}` ({}) package", name, kind);
    Ok(())
}
