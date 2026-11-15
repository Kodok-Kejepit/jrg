// src/kutip_inti.rs — masukan yang dikutip di pesan galat. Runtime LLVM memakai SALINAN persis berkas ini.

use alloc::format;
use alloc::string::String;

/// Kendali dan pengubah arah di-escape (baris log palsu, CWE-117); yang lebih dari 64 karakter dipotong sambil
/// menyebut panjangnya, jadi pesan tidak tumbuh mengikuti masukan. Tanda kutipnya dipasang pemanggil.
pub fn kutip(t: impl AsRef<str>) -> String {
    let t = t.as_ref();
    let n = t.chars().count();
    if n <= 64 { return lolos(t.chars()); }
    format!("{}…{} ({} karakter)", lolos(t.chars().take(40)), lolos(t.chars().skip(n - 12)), n)
}

fn lolos(cs: impl Iterator<Item = char>) -> String {
    let mut s = String::new();
    for c in cs {
        let samar = matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}');
        if c.is_control() || samar { s.extend(c.escape_unicode()); } else { s.push(c); }
    }
    s
}

#[cfg(test)]
mod tes {
    use super::kutip;

    #[test]
    fn pesan_tidak_tumbuh_dan_tidak_memalsukan_baris() {
        assert_eq!(kutip("abc"), "abc");
        assert_eq!(kutip("a\nb\u{202E}c"), "a\\u{a}b\\u{202e}c");
        let k = kutip("é".repeat(1_000_000));
        assert!(k.chars().count() < 80 && k.ends_with("(1000000 karakter)"), "{}", k);
        assert_eq!(kutip("x".repeat(64)), "x".repeat(64));
    }
}
