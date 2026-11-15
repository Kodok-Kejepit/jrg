// src/dok/konteks_html.rs — konteks HTML tempat `dok::render` menyisipkan nilai.
// Meloloskan `<>&"'` cuma aman di teks dan di nilai atribut berkutip; di tempat lain nilai ditolak.

/// Pengganti nilai URL berskema berbahaya — terlihat, bukan URL yang tampak sah.
pub const URL_DITOLAK: &str = "#jrg-url-ditolak";

const SKEMA_AMAN: [&str; 4] = ["http", "https", "mailto", "tel"];

const ATRIBUT_URL: [&str; 17] = [
    "href", "src", "action", "formaction", "xlink:href", "poster", "cite", "background", "data",
    "codebase", "manifest", "ping", "longdesc", "usemap", "icon", "classid", "archive",
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Keadaan {
    Teks,
    /// Sesudah `<`, `<!`, `<!-`.
    Buka, BukaSeru, BukaSeruMin,
    NamaTag,
    DalamTag,
    NamaAtribut,
    /// Sesudah nama atribut, sebelum `=` atau atribut berikutnya.
    SesudahNama,
    SesudahSama,
    Kutip(u8),
    TanpaKutip,
    Komentar,
    /// `<!DOCTYPE ..>`, `<?..>`.
    Deklarasi,
    /// Isi `<script>`/`<style>` (teks mentah) atau `<textarea>`/`<title>` (entitas dibaca).
    Mentah,
}

pub struct Jejak {
    k: Keadaan,
    pos: usize,
    tag: String,
    tutup: bool,
    atribut: String,
    /// Isi elemen yang sedang dibaca sebagai teks mentah, dan kemajuan mencocokkan `</nama`.
    mentah: String,
    cocok: usize,
    strip: usize,
    awal_nilai: usize,
    berisi_data: bool,
}

impl Jejak {
    pub fn baru() -> Jejak {
        Jejak { k: Keadaan::Teks, pos: 0, tag: String::new(), tutup: false, atribut: String::new(),
                mentah: String::new(), cocok: 0, strip: 0, awal_nilai: 0, berisi_data: false }
    }

    /// Baca bagian `out` yang belum dibaca; nilai URL berskema berbahaya diganti di `out`.
    pub fn maju(&mut self, out: &mut String) {
        while self.pos < out.len() {
            let c = out.as_bytes()[self.pos];
            if let Keadaan::Kutip(q) = self.k {
                if c == q {
                    self.tutup_nilai(out);
                    self.k = Keadaan::DalamTag;
                }
                self.pos += 1;
                continue;
            }
            self.langkah(c);
            self.pos += 1;
        }
    }

    fn tutup_nilai(&mut self, out: &mut String) {
        if self.berisi_data && ATRIBUT_URL.contains(&self.atribut.as_str())
            && !url_aman(&out[self.awal_nilai..self.pos])
        {
            out.replace_range(self.awal_nilai..self.pos, URL_DITOLAK);
            self.pos = self.awal_nilai + URL_DITOLAK.len();
        }
        self.berisi_data = false;
    }

    fn langkah(&mut self, c: u8) {
        use Keadaan::*;
        let spasi = matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c);
        self.k = match self.k {
            Teks => if c == b'<' { Buka } else { Teks },
            Buka => match c {
                b'!' => BukaSeru,
                b'?' => Deklarasi,
                b'/' => { self.tutup = true; self.tag.clear(); NamaTag }
                c if c.is_ascii_alphabetic() => {
                    self.tutup = false;
                    self.tag.clear();
                    self.tag.push(c.to_ascii_lowercase() as char);
                    NamaTag
                }
                b'<' => Buka,
                _ => Teks,
            },
            BukaSeru => if c == b'-' { BukaSeruMin } else if c == b'>' { Teks } else { Deklarasi },
            BukaSeruMin => if c == b'-' { self.strip = 0; Komentar } else if c == b'>' { Teks } else { Deklarasi },
            Komentar => {
                if c == b'>' && self.strip >= 2 { Teks } else {
                    self.strip = if c == b'-' { self.strip + 1 } else { 0 };
                    Komentar
                }
            }
            Deklarasi => if c == b'>' { Teks } else { Deklarasi },
            NamaTag => {
                if spasi || c == b'/' { DalamTag }
                else if c == b'>' { self.akhir_tag() }
                else { if self.tag.len() < 16 { self.tag.push(c.to_ascii_lowercase() as char); } NamaTag }
            }
            DalamTag | SesudahNama if c == b'>' => self.akhir_tag(),
            DalamTag => {
                if spasi || c == b'/' { DalamTag } else {
                    self.atribut.clear();
                    self.atribut.push(c.to_ascii_lowercase() as char);
                    NamaAtribut
                }
            }
            NamaAtribut => {
                if c == b'=' { SesudahSama }
                else if spasi { SesudahNama }
                else if c == b'>' { self.akhir_tag() }
                else if c == b'/' { DalamTag }
                else { if self.atribut.len() < 32 { self.atribut.push(c.to_ascii_lowercase() as char); } NamaAtribut }
            }
            SesudahNama => {
                if c == b'=' { SesudahSama } else if spasi { SesudahNama } else if c == b'/' { DalamTag } else {
                    self.atribut.clear();
                    self.atribut.push(c.to_ascii_lowercase() as char);
                    NamaAtribut
                }
            }
            SesudahSama => match c {
                b'"' | b'\'' => { self.awal_nilai = self.pos + 1; self.berisi_data = false; Kutip(c) }
                b'>' => self.akhir_tag(),
                _ if spasi => SesudahSama,
                _ => TanpaKutip,
            },
            TanpaKutip => if spasi { DalamTag } else if c == b'>' { self.akhir_tag() } else { TanpaKutip },
            Mentah => {
                let pola = self.mentah.as_bytes();
                let harus = if self.cocok == 0 { b'<' } else if self.cocok == 1 { b'/' } else { pola[self.cocok - 2] };
                if c.to_ascii_lowercase() == harus {
                    self.cocok += 1;
                    if self.cocok == pola.len() + 2 {
                        self.tutup = true;
                        self.tag = self.mentah.clone();
                        self.cocok = 0;
                        NamaTag
                    } else { Mentah }
                } else {
                    self.cocok = if c == b'<' { 1 } else { 0 };
                    Mentah
                }
            }
            Kutip(_) => unreachable!("Kutip ditangani di maju"),
        };
    }

    fn akhir_tag(&mut self) -> Keadaan {
        if !self.tutup && matches!(self.tag.as_str(), "script" | "style" | "textarea" | "title") {
            self.mentah = self.tag.clone();
            self.cocok = 0;
            Keadaan::Mentah
        } else {
            Keadaan::Teks
        }
    }

    /// Boleh sebuah nilai yang diloloskan disisipkan di sini? `Err` menyebut alasannya dan jalan keluarnya.
    pub fn periksa_nilai(&mut self, nama: &str) -> Result<(), String> {
        use Keadaan::*;
        let awal = format!("dok::render: `{{{{ {} }}}}`", nama);
        match self.k {
            Teks | Komentar => Ok(()),
            Mentah if matches!(self.mentah.as_str(), "textarea" | "title") => Ok(()),
            Mentah => Err(format!(
                "{} ada di dalam `<{}>` — meloloskan HTML tidak melindungi {}; taruh nilainya di atribut \
                 `data-*` (`<div id=\"d\" data-isi=\"{{{{ {} }}}}\">`) lalu baca dari skrip",
                awal, self.mentah, if self.mentah == "script" { "JavaScript" } else { "CSS" }, nama)),
            Kutip(_) => {
                let a = self.atribut.as_str();
                if a.starts_with("on") || a == "style" || a == "srcdoc" {
                    return Err(format!(
                        "{} ada di atribut `{}` — isinya dibaca lagi sebagai {}, dan meloloskan HTML tidak \
                         melindunginya; taruh nilainya di atribut `data-*` lalu baca dari skrip",
                        awal, a, if a == "style" { "CSS" } else if a == "srcdoc" { "HTML" } else { "JavaScript" }));
                }
                self.berisi_data = true;
                Ok(())
            }
            SesudahSama | TanpaKutip => Err(format!(
                "{} adalah nilai atribut `{}` tanpa tanda kutip — spasi di nilainya menambah atribut baru; \
                 beri kutip: `{}=\"{{{{ {} }}}}\"`", awal, self.atribut, self.atribut, nama)),
            _ => Err(format!(
                "{} ada di dalam tag, di tempat nama tag atau atribut — nilai di sana bisa menyisipkan atribut \
                 seperti `onclick`; tulis atributnya di templat dan isi nilainya di dalam kutip, atau pilih \
                 dengan `{{{{#if}}}}`", awal)),
        }
    }
}

/// Nilai atribut URL (masih berentitas) aman kalau relatif atau berskema `http`/`https`/`mailto`/`tel`.
pub fn url_aman(nilai: &str) -> bool {
    let teks = urai_entitas(nilai);
    // Seperti pengurai URL peramban: tab dan baris baru dibuang, kendali dan spasi di depan dipangkas.
    let bersih: String = teks.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let bersih = bersih.trim_start_matches(|c: char| c <= ' ');
    match bersih.find([':', '/', '?', '#']) {
        Some(i) if bersih.as_bytes()[i] == b':' => {
            let skema = bersih[..i].to_ascii_lowercase();
            SKEMA_AMAN.contains(&skema.as_str())
        }
        _ => true,
    }
}

/// Entitas yang dipakai peloloskan dan entitas angka — sekali, seperti peramban membaca atribut.
fn urai_entitas(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut sisa = s;
    while let Some(i) = sisa.find('&') {
        o.push_str(&sisa[..i]);
        let ekor = &sisa[i + 1..];
        let akhir = ekor.find(';').filter(|&j| j <= 10);
        let ganti = akhir.and_then(|j| {
            let nama = &ekor[..j];
            let c = match nama {
                "amp" => Some('&'), "lt" => Some('<'), "gt" => Some('>'), "quot" => Some('"'),
                "apos" => Some('\''), "colon" => Some(':'), "Tab" => Some('\t'), "NewLine" => Some('\n'),
                _ => {
                    let angka = nama.strip_prefix('#')?;
                    let n = match angka.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => angka.parse::<u32>().ok()?,
                    };
                    char::from_u32(n)
                }
            };
            c.map(|c| (c, j))
        });
        match ganti {
            Some((c, j)) => { o.push(c); sisa = &ekor[j + 1..]; }
            None => { o.push('&'); sisa = ekor; }
        }
    }
    o.push_str(sisa);
    o
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn url_berskema_berbahaya_dikenali() {
        for aman in ["/a", "a/b:c", "https://x", "HTTP://x", "mailto:a@b", "?q=1:2", "#x", "", "tel:1"] {
            assert!(url_aman(aman), "{}", aman);
        }
        for bahaya in ["javascript:alert(1)", " JavaScript:x", "java\tscript:x", "\u{1}javascript:x",
                       "data:text/html,x", "vbscript:x", "&#106;avascript:x", "javascript&colon;x", ":x"] {
            assert!(!url_aman(bahaya), "{:?}", bahaya);
        }
        // Data yang sudah diloloskan dibaca SEKALI: `&amp;colon;` adalah teks, bukan `:`.
        assert!(url_aman("javascript&amp;colon;x"));
    }
}
