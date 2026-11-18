// src/jrgm/rangka_en.rs — kerangka `jrgm new` dalam bahasa Inggris (bahasa pesan bawaan); padanan `rangka.rs`.
// Isinya program yang SAMA: tes yang menjalankan kerangka Indonesia juga menjalankan yang ini.

pub(crate) fn web(name: &str) -> String {
    format!(
        r#"// {name} — HTTP server. Run: jrgm run, then open http://127.0.0.1:8080
package main

// `{{{{ x }}}}` escapes HTML on its own; `{{{{#each}}}}` repeats; `{{{{#if}}}}` chooses.
const PAGE: string = """<!doctype html><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>{{{{ title }}}}</title>
<style>body{{font-family:system-ui;max-width:38rem;margin:3rem auto;padding:0 1rem;line-height:1.6}}</style>
<h1>{{{{ title }}}}</h1>
<p>{{{{ message }}}}</p>
<ul>
{{{{#each links}}}}  <li><a href="{{{{ url }}}}">{{{{ text }}}}</a></li>
{{{{/each}}}}</ul>"""

pub fn main() {{
    let r = dok::router_new()

    dok::router_get(r, "/", fn(req) => home())
    dok::router_get(r, "/hello/:name", fn(req) => hello(req))
    dok::router_get(r, "/api", fn(req) => dok::json_response({{"ok": true}}))
    dok::router_not_found(r, fn(req) => dok::response(404, "page not found"))

    wok("server running at http://127.0.0.1:8080")
    dok::serve(r, 8080, "127.0.0.1")
}}

fn home() {{
    let body = dok::render(PAGE, {{
        "title": "{name}",
        "message": "This page was made by `jrgm new --kind web`.",
        "links": [
            {{"url": "/api", "text": "JSON example"}},
            {{"url": "/hello/world", "text": "Route with a parameter"}},
        ],
    }}).unwrap_or("<p>render failed</p>")
    return dok::html(body)
}}

fn hello(req) {{
    // The name comes from the URL — something someone else wrote. `{{{{ x }}}}`
    // escapes it before it is placed, so `<script>` shows up as text.
    let name = req.params["name"]
    return dok::html(dok::render(PAGE, {{
        "title": "Hello, " + name,
        "message": "The name in the URL is escaped before it goes into the page.",
        "links": [{{"url": "/", "text": "Back"}}],
    }}).unwrap_or(""))
}}
"#, name = name)
}

pub(crate) fn android(name: &str) -> String {
    format!(
        r#"// {name} — an Android app made with JRG.
// Build the APK: jrgm apk     Install on the phone and run: jrgm run --android
// On the desktop: jrgm run (an ordinary window; the mouse = touch)
package main

const WIDTH = 180
const HEIGHT = 320

fn random(seed: int) -> int {{
    return (seed * 1103515245 + 12345) % 2147483648
}}

pub fn main() {{
    let c = Canvas::new(WIDTH, HEIGHT, "{name}", "tab")
    var score = 0
    var seed = 7
    var x: float = 90.0
    var y: float = 160.0
    var before = false
    while canvas::update(c) {{
        let m = canvas::get_mouse_pos(c)
        let pressed = canvas::get_mouse_button(c, 0)
        // Once per touch: counted when the finger first lands.
        if pressed && !before {{
            let dx = m.x - x
            let dy = m.y - y
            if dx * dx + dy * dy < 900.0 {{
                score = score + 1
                seed = random(seed)
                x = (20 + seed % (WIDTH - 40)) as float
                seed = random(seed)
                y = (60 + seed % (HEIGHT - 100)) as float
            }}
        }}
        before = pressed
        canvas::clear(c, 0x101828)
        canvas::fill_circle(c, x, y, 24, 0xF59E0B)
        canvas::draw_text_scaled(c, 8, 8, "SCORE " + str(score), 2, 0xFFFFFF)
        time::sleep_ms(16)
    }}
    wok("final score:", score)
}}
"#, name = name)
}

pub(crate) fn lib(name: &str) -> String {
    format!(
        r#"// {name} — library
package {name}

/// Greet someone. This doc comment shows up in `jrgc --doc`.
pub fn greet(name: string) -> string {{
    return "Hello, " + name + "!"
}}
"#)
}

pub(crate) fn bin(name: &str) -> String {
    format!(
        r#"// {name}
package main

pub fn main() {{
    wok("Hello from JawaRustGo++!")

    // `for i, x in a` gives the INDEX and the VALUE at once.
    let numbers = [1, 2, 3]
    for i, x in numbers {{
        wok(i, "->", x * x)
    }}
}}
"#)
}

pub(crate) fn manifes_umum(name: &str, entry: &str) -> String {
    format!(
        "[package]\nname        = \"{name}\"\nversion     = \"0.1.0\"\n\
         edition     = \"jrg2025\"\nentry       = \"{entry}\"\n\n\
         # Dependencies: a local directory, or a git repository (locked in jrgm.lock):\n\
         #   jrgm add helper --path ../helper\n\
         #   jrgm add helper --git https://example.org/helper.git --version 0.1.0\n\
         [dependencies]\n\n\
         # Python packages used through py::import/py::call/py::call_kw.\n\
         # jrgm also scans {entry} automatically and adds whatever it\n\
         # finds there — this line is for what the scan cannot catch\n\
         # (e.g. py::exec(\"import ...\")) or to be explicit.\n\
         # [python]\n\
         # packages = [\"requests\", \"numpy\"]\n")
}

pub(crate) const MANIFES_WEB: &str = "\n\
    # Make it an Android app: `jrgm apk`. This page shows in a WebView, and this program\n\
    # runs inside the phone as its server (jrgc goes into the APK too).\n\
    [android]\n\
    tampilan = \"web\"\n\
    port     = 8080\n";

pub(crate) fn manifes_android(name: &str, paket: &str) -> String {
    format!("\n\
             # APK identity for `jrgm apk` and `jrgm run --android`.\n\
             [android]\n\
             nama      = \"{name}\"\n\
             paket     = \"{paket}\"\n\
             versi     = \"0.1.0\"          # the version code is derived: 0.1.0 -> 1000\n\
             orientasi = \"portrait\"       # bebas | portrait | landscape\n\
             abi       = [\"arm64-v8a\"]    # add \"x86_64\" for the emulator\n\
             # ikon    = \"aset/ikon.png\"  # square PNG, e.g. 512x512\n\
             # izin    = [\"internet\"]      # internet | jaringan | wifi | getar | tetap_menyala\n\
             # pustaka = [\"native/{{abi}}/libhitung.so\"]  # the .so becomes part of the project\n\
             #\n\
             # Or let jrgm compile the sources per ABI:\n\
             # [android.pustaka_c]\n\
             # libhitung = [\"native/hitung.c\"]\n\
             # [android.pustaka_rust]\n\
             # librumus = \"rust/rumus\"      # a crate with crate-type = [\"cdylib\"]\n\
             # Release key: `jrgm kunci` once, then `jrgm apk --rilis`.\n")
}

pub(crate) fn uji(name: &str) -> String {
    format!(
        "// Tests for `{name}`. Run them with `jrgm test`.\n\
         //\n\
         // A function is a test when its name starts with `test_` or\n\
         // `uji_`. No `main` needed — the runner calls each test\n\
         // directly, and catches its failure so the next one still\n\
         // runs.\n\
         package uji_{name}\n\n\
         pub fn test_example() {{\n    assert_eq(1 + 1, 2)\n}}\n\n\
         // For a group of checks reported together, instead of stopping\n\
         // at the first failure:\n\
         //\n\
         //     import \"std/testing\"\n\
         //     pub fn uji_many() {{\n\
         //         testing::cek_sama(\"add\", 1 + 1, 2)\n\
         //         assert_eq(testing::hitungan().1, 0)\n\
         //     }}\n")
}

/// Kerangka bahasa lain (`jrgm new --bahasa`): cuma teks yang terbaca orang, nama berkas dan fungsi tetap.
pub(crate) fn teks_lain(isi: &str) -> String {
    const GANTI: &[(&str, &str)] = &[
        ("Halo dari ", "Hello from "),
        ("Dijalankan `jrgm test` (unittest, src/ sudah di PYTHONPATH).", "Run by `jrgm test` (unittest, src/ is already on PYTHONPATH)."),
        ("Dijalankan `jrgm test` (node --test).", "Run by `jrgm test` (node --test)."),
        ("dikompilasi bersama src/ (tanpa main.c) lalu dijalankan; keluar 0 = lulus.", "is compiled together with src/ (without main.c) and then run; exit 0 = pass."),
        ("dikompilasi bersama src/ (tanpa main.cpp) lalu dijalankan; keluar 0 = lulus.", "is compiled together with src/ (without main.cpp) and then run; exit 0 = pass."),
        ("dikompilasi bersama src/ (tanpa Main) lalu main-nya dijalankan dengan -ea.", "is compiled together with src/ (without Main) and then its main is run with -ea."),
        ("dijalankan (src/ di LUA_PATH); keluar 0 = lulus.", "is run (src/ on LUA_PATH); exit 0 = pass."),
        ("dijalankan (src/ di -I); keluar 0 = lulus.", "is run (src/ on -I); exit 0 = pass."),
        ("dijalankan; keluar 0 = lulus.", "is run; exit 0 = pass."),
        ("`jrgm test`: tiap ", "`jrgm test`: each "),
    ];
    GANTI.iter().fold(isi.to_string(), |s, (id, en)| s.replace(id, en))
}
