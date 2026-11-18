// src/llvm/terjemah.rs — tabel terjemahan galat yang ditanam di biner `--emit-llvm`: pola yang tiap potongan
// Indonesianya muncul di teks konstanta biner itu atau di sumber runtime-nya (`runtime/src/bahasa.rs` memakainya).

use std::sync::OnceLock;

/// Isi literal teks di sumber runtime (yang bisa dicetak runtime sendiri), sekali per proses. Sambungan baris di
/// dalam literal (`\` + baris baru di Rust) dan literal C bersebelahan (`"a" "b"`) dirapatkan.
fn runtime() -> &'static str {
    static T: OnceLock<String> = OnceLock::new();
    T.get_or_init(|| {
        let mut s = String::new();
        for isi in [include_str!("../../runtime/src/lib.rs"), include_str!("../../runtime/src/json_inti.rs"),
                    include_str!("../../runtime/src/kutip_inti.rs"), include_str!("primitif.c"),
                    include_str!("korutin.c"), include_str!("jaga_stack.c"), include_str!("korutin_wasm.c")] {
            literal(isi, &mut s);
        }
        s
    })
}

fn literal(isi: &str, out: &mut String) {
    let b = isi.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => { while i < b.len() && b[i] != b'\n' { i += 1; } }
            b'\'' => {
                // Literal karakter (`'"'`, `'\\'`) dilompati; selain itu umur (`'a`).
                if b.get(i + 1) == Some(&b'\\') && b.get(i + 3) == Some(&b'\'') { i += 4 }
                else if b.get(i + 2) == Some(&b'\'') { i += 3 } else { i += 1 }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && b.get(i + 1) == Some(&b'\n') {
                        i += 2;
                        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') { i += 1; }
                        continue;
                    }
                    if b[i] == b'\\' {
                        match b.get(i + 1) {
                            Some(b'n') => { out.push('\n'); i += 2; continue }
                            Some(&c @ (b'"' | b'\\' | b'\'')) => { out.push(c as char); i += 2; continue }
                            _ => {}
                        }
                    }
                    let mulai = i;
                    i += if b[i] == b'\\' { 2 } else { 1 };
                    while i < b.len() && (b[i] & 0xC0) == 0x80 { i += 1; }
                    out.push_str(&isi[mulai..i.min(b.len())]);
                }
                i += 1;
                let mut j = i;
                while j < b.len() && b[j].is_ascii_whitespace() { j += 1; }
                if !(j > i && b.get(j) == Some(&b'"')) { out.push('\n'); }
            }
            _ => i += 1,
        }
    }
}

/// Global tabel untuk `jrg_terjemah_pasang`, dari teks-teks konstanta biner ini. Biner yang memakai jembatan
/// (yang memuat `jrgc` utuh) bisa menerima kalimat apa pun dari sana sebagai nilai `Err`: tabelnya utuh.
pub(crate) fn definisi(konstanta: &[(String, Vec<u8>)], jembatan: bool) -> String {
    let mut program = String::new();
    for (_, b) in konstanta {
        program.push_str(&String::from_utf8_lossy(b));
        program.push('\n');
    }
    let rt = runtime();
    let blob = crate::terjemah::blob_tersaring(|id| {
        jembatan || id.split("{}").all(|s| s.is_empty() || rt.contains(s) || program.contains(s))
    });
    let mut teks: Vec<u8> = blob.into_bytes();
    let n = teks.len();
    teks.push(0);
    format!("@jrg_terjemah_teks = private constant [{} x i8] c\"{}\"\n@jrg_terjemah_n = private constant i64 {}\n",
        teks.len(), teks.iter().map(|b| format!("\\{:02X}", b)).collect::<String>(), n)
}
