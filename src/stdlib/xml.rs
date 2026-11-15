// src/stdlib/xml.rs — `xml::`: mengurai dan menulis XML, tanpa dependensi.
// DTD ditolak utuh (XXE, ledakan entitas); sarang, atribut ganda dan rujukan karakter diperiksa; penulis menolak nama
// yang bisa menyisipkan markup.

use std::collections::HashMap;

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// Sarang elemen paling dalam, sama dengan batas sarang pengurai JRG.
pub const MAKS_SARANG: usize = 256;

pub fn register(vm: &mut Vm) {
    vm.def("xml::parse", |a| crate::stdlib::sistem::lewat_hasil(a, x_parse));
    vm.def("xml::stringify", |a| crate::stdlib::sistem::lewat_hasil(a, x_stringify));
    vm.def("xml::escape", |a| crate::stdlib::sistem::lewat_hasil(a, x_escape));
}

/// Satu simpul hasil urai: elemen (nama, atribut urut tulis, anak) atau teks.
#[derive(Debug, PartialEq)]
pub enum Simpul { Elemen(String, Vec<(String, String)>, Vec<Simpul>), Teks(String) }

struct Pengurai<'a> { s: &'a str, i: usize }

/// Karakter yang boleh muncul di dokumen XML 1.0 (`Char`): tanpa kendali selain tab/baris, tanpa surrogate, U+FFFE/FFFF.
fn char_xml(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn awal_nama(c: char) -> bool { c.is_alphabetic() || c == '_' || c == ':' }
fn isi_nama(c: char) -> bool { awal_nama(c) || c.is_alphanumeric() || matches!(c, '-' | '.' | '\u{B7}') }

/// Nama elemen atau atribut yang sah — dipakai pengurai DAN penulis, jadi `a onload=x` tidak pernah jadi nama.
pub fn nama_sah(n: &str) -> bool {
    let mut c = n.chars();
    c.next().map_or(false, awal_nama) && c.all(isi_nama)
}

impl<'a> Pengurai<'a> {
    fn sisa(&self) -> &'a str { &self.s[self.i..] }
    fn mulai(&self, t: &str) -> bool { self.sisa().starts_with(t) }

    /// Baris dan kolom posisi kini (kolom dihitung karakter), untuk kalimat galat.
    fn galat(&self, pesan: &str) -> String {
        let depan = &self.s[..self.i];
        let baris = depan.matches('\n').count() + 1;
        let kolom = depan.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("baris {} kolom {}: {}", baris, kolom, pesan)
    }

    fn lewati_spasi(&mut self) {
        while let Some(c) = self.sisa().chars().next() {
            if matches!(c, ' ' | '\t' | '\n' | '\r') { self.i += 1 } else { break }
        }
    }

    fn nama(&mut self) -> Result<String, String> {
        let mulai = self.i;
        let mut c = self.sisa().char_indices();
        match c.next() {
            Some((_, a)) if awal_nama(a) => {}
            _ => return Err(self.galat("harusnya ada nama di sini")),
        }
        let akhir = c.find(|(_, x)| !isi_nama(*x)).map_or(self.sisa().len(), |(k, _)| k);
        self.i += akhir;
        Ok(self.s[mulai..self.i].to_string())
    }

    /// `&lt;` dan kawan-kawan, `&#65;`, `&#x41;` — entitas lain tidak ada, karena DTD tidak didukung.
    fn rujukan(&mut self, keluar: &mut String) -> Result<(), String> {
        let akhir = self.sisa().find(';').filter(|k| *k <= 12)
            .ok_or_else(|| self.galat("`&` tanpa `;` penutup — tulis `&amp;` untuk tanda & biasa"))?;
        let isi = &self.sisa()[1..akhir];
        let c = match isi {
            "lt" => '<', "gt" => '>', "amp" => '&', "quot" => '"', "apos" => '\'',
            _ if isi.starts_with("#x") => u32::from_str_radix(&isi[2..], 16).ok().and_then(char::from_u32)
                .filter(|c| char_xml(*c)).ok_or_else(|| self.galat(&format!("`&{};` bukan karakter XML yang sah", isi)))?,
            _ if isi.starts_with('#') => isi[1..].parse::<u32>().ok().and_then(char::from_u32)
                .filter(|c| char_xml(*c)).ok_or_else(|| self.galat(&format!("`&{};` bukan karakter XML yang sah", isi)))?,
            _ => return Err(self.galat(&format!("entitas `&{};` tidak dikenal — cuma lt, gt, amp, quot, apos dan \
                                                 rujukan angka (`&#65;`); entitas buatan butuh DTD, yang ditolak", isi))),
        };
        keluar.push(c);
        self.i += akhir + 1;
        Ok(())
    }

    fn nilai_atribut(&mut self) -> Result<String, String> {
        let kutip = match self.sisa().chars().next() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(self.galat("nilai atribut harus diapit tanda kutip")),
        };
        self.i += 1;
        let mut v = String::new();
        loop {
            let Some(c) = self.sisa().chars().next() else { return Err(self.galat("nilai atribut tidak ditutup")) };
            match c {
                _ if c == kutip => { self.i += 1; return Ok(v); }
                '<' => return Err(self.galat("`<` di dalam nilai atribut — tulis `&lt;`")),
                '&' => self.rujukan(&mut v)?,
                // Normalisasi nilai atribut XML 1.0: tab dan baris baru jadi spasi.
                '\t' | '\n' | '\r' => { v.push(' '); self.i += 1; }
                _ if !char_xml(c) => return Err(self.galat(&format!("karakter U+{:04X} tidak boleh ada di XML", c as u32))),
                _ => { v.push(c); self.i += c.len_utf8(); }
            }
        }
    }

    /// Komentar, PI, dan (di prolog) deklarasi `<?xml ..?>`; `<!DOCTYPE` ditolak di mana pun.
    fn lewati_lain(&mut self) -> Result<bool, String> {
        if self.mulai("<!--") {
            let k = self.sisa()[4..].find("--").ok_or_else(|| self.galat("komentar tidak ditutup"))?;
            if !self.sisa()[4 + k..].starts_with("-->") { return Err(self.galat("`--` di dalam komentar")); }
            self.i += 4 + k + 3;
            return Ok(true);
        }
        if self.mulai("<?") {
            let k = self.sisa().find("?>").ok_or_else(|| self.galat("instruksi pemrosesan tidak ditutup"))?;
            self.i += k + 2;
            return Ok(true);
        }
        if self.mulai("<!DOCTYPE") || self.mulai("<!ENTITY") {
            return Err(self.galat("DTD (`<!DOCTYPE`) ditolak: lewat DTD sebuah dokumen bisa membaca berkas atau \
                                   jaringan (XXE) dan mengembang jutaan kali lipat (ledakan entitas)"));
        }
        Ok(false)
    }

    fn elemen(&mut self, kedalaman: usize) -> Result<Simpul, String> {
        if kedalaman > MAKS_SARANG {
            return Err(self.galat(&format!("elemen bersarang lebih dari {} tingkat", MAKS_SARANG)));
        }
        self.i += 1; // `<`
        let nama = self.nama()?;
        let mut atribut: Vec<(String, String)> = Vec::new();
        // Kembar diperiksa lewat himpunan: memindai `atribut` tiap kali kuadrat (160 ribu atribut 34 dtk, AMAN-42).
        let mut terlihat: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let ada_spasi = self.sisa().starts_with(|c: char| c.is_whitespace());
            self.lewati_spasi();
            if self.mulai("/>") { self.i += 2; return Ok(Simpul::Elemen(nama, atribut, Vec::new())); }
            if self.mulai(">") { self.i += 1; break; }
            if !ada_spasi { return Err(self.galat("harusnya ada spasi sebelum atribut")); }
            let a = self.nama()?;
            self.lewati_spasi();
            if !self.mulai("=") { return Err(self.galat(&format!("atribut `{}` tanpa `=`", a))); }
            self.i += 1;
            self.lewati_spasi();
            let v = self.nilai_atribut()?;
            if !terlihat.insert(a.clone()) {
                return Err(self.galat(&format!("atribut `{}` ditulis dua kali di `<{}>`", a, nama)));
            }
            atribut.push((a, v));
        }
        let mut anak = Vec::new();
        let mut teks = String::new();
        loop {
            if self.i >= self.s.len() { return Err(self.galat(&format!("`<{}>` tidak ditutup", nama))); }
            if self.mulai("</") {
                if !teks.is_empty() { anak.push(Simpul::Teks(std::mem::take(&mut teks))); }
                self.i += 2;
                let tutup = self.nama()?;
                if tutup != nama {
                    return Err(self.galat(&format!("`</{}>` menutup `<{}>`", tutup, nama)));
                }
                self.lewati_spasi();
                if !self.mulai(">") { return Err(self.galat("harusnya `>` di sini")); }
                self.i += 1;
                return Ok(Simpul::Elemen(nama, atribut, anak));
            }
            if self.mulai("<![CDATA[") {
                let k = self.sisa().find("]]>").ok_or_else(|| self.galat("CDATA tidak ditutup"))?;
                teks.push_str(&self.sisa()[9..k]);
                self.i += k + 3;
                continue;
            }
            if self.lewati_lain()? { continue; }
            let c = self.sisa().chars().next().unwrap();
            match c {
                '<' => {
                    if !teks.is_empty() { anak.push(Simpul::Teks(std::mem::take(&mut teks))); }
                    anak.push(self.elemen(kedalaman + 1)?);
                }
                '&' => self.rujukan(&mut teks)?,
                _ if self.mulai("]]>") => return Err(self.galat("`]]>` di luar CDATA — tulis `]]&gt;`")),
                _ if !char_xml(c) => return Err(self.galat(&format!("karakter U+{:04X} tidak boleh ada di XML", c as u32))),
                _ => { teks.push(c); self.i += c.len_utf8(); }
            }
        }
    }
}

/// Urai satu dokumen: prolog (deklarasi, komentar, PI), tepat satu elemen akar, lalu cuma komentar/PI/spasi.
pub fn urai(s: &str) -> Result<Simpul, String> {
    let mut p = Pengurai { s: s.strip_prefix('\u{FEFF}').unwrap_or(s), i: 0 };
    loop { p.lewati_spasi(); if !p.lewati_lain()? { break } }
    if !p.mulai("<") { return Err(p.galat("dokumen XML harus diawali satu elemen")); }
    let akar = p.elemen(1)?;
    loop { p.lewati_spasi(); if !p.lewati_lain()? { break } }
    if p.i < p.s.len() { return Err(p.galat("ada isi sesudah elemen akar — dokumen XML cuma punya satu akar")); }
    Ok(akar)
}

/// Escape teks (`&`, `<`, `>`) atau nilai atribut (juga `"` dan tab/baris, supaya terbaca kembali utuh).
pub fn escape(t: &str, atribut: bool) -> String {
    let mut o = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '&' => o.push_str("&amp;"), '<' => o.push_str("&lt;"), '>' => o.push_str("&gt;"),
            '"' if atribut => o.push_str("&quot;"),
            '\t' if atribut => o.push_str("&#9;"), '\n' if atribut => o.push_str("&#10;"), '\r' => o.push_str("&#13;"),
            _ => o.push(c),
        }
    }
    o
}

fn dict(isi: Vec<(&str, Value)>) -> Value {
    let m: HashMap<String, Value> = isi.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Struct { name: "dict".to_string(), fields: m.into() }
}

fn ke_nilai(s: Simpul) -> Value {
    match s {
        Simpul::Teks(t) => Value::teks(t),
        Simpul::Elemen(n, a, anak) => dict(vec![
            ("nama", Value::teks(n)),
            ("atribut", dict(a.iter().map(|(k, v)| (k.as_str(), Value::Str(v.clone().into()))).collect())),
            ("anak", Value::Array(anak.into_iter().map(ke_nilai).collect::<Vec<_>>().into())),
        ]),
    }
}

/// Tulis kembali sebuah simpul (dict ber-`nama`/`atribut`/`anak`, atau teks). Yang tidak bisa jadi XML sah ditolak.
fn tulis(v: &Value, keluar: &mut String, kedalaman: usize) -> Result<(), String> {
    if kedalaman > MAKS_SARANG { return Err(format!("simpul bersarang lebih dari {} tingkat", MAKS_SARANG)); }
    let f = match v {
        Value::Str(t) => {
            if let Some(c) = t.chars().find(|c| !char_xml(*c)) {
                return Err(format!("teks memuat U+{:04X}, yang tidak bisa ditulis di XML", c as u32));
            }
            keluar.push_str(&escape(t, false));
            return Ok(());
        }
        Value::Struct { name, fields } if name == "dict" => fields,
        lain => return Err(format!("simpul harus dict {{nama, atribut, anak}} atau teks, dapat `{}`", lain.type_name())),
    };
    let nama = match f.get("nama") {
        Some(Value::Str(n)) if nama_sah(n) => n.to_string(),
        Some(Value::Str(n)) => return Err(format!("`{}` bukan nama elemen XML yang sah", n)),
        _ => return Err("simpul tanpa `nama` berupa teks".into()),
    };
    keluar.push('<');
    keluar.push_str(&nama);
    match f.get("atribut") {
        None | Some(Value::Nil) => {}
        Some(Value::Struct { name, fields }) if name == "dict" => {
            let mut kunci: Vec<&String> = fields.keys().collect();
            kunci.sort();
            for k in kunci {
                if !nama_sah(k) { return Err(format!("`{}` bukan nama atribut XML yang sah", k)); }
                let isi = match &fields[k] { Value::Str(s) => s.to_string(), lain => format!("{}", lain) };
                if let Some(c) = isi.chars().find(|c| !char_xml(*c)) {
                    return Err(format!("atribut `{}` memuat U+{:04X}, yang tidak bisa ditulis di XML", k, c as u32));
                }
                keluar.push_str(&format!(" {}=\"{}\"", k, escape(&isi, true)));
            }
        }
        Some(lain) => return Err(format!("`atribut` harus dict, dapat `{}`", lain.type_name())),
    }
    let anak = match f.get("anak") {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Array(a)) => a.to_vec(),
        Some(lain) => return Err(format!("`anak` harus array, dapat `{}`", lain.type_name())),
    };
    if anak.is_empty() { keluar.push_str("/>"); return Ok(()); }
    keluar.push('>');
    for a in &anak { tulis(a, keluar, kedalaman + 1)?; }
    keluar.push_str(&format!("</{}>", nama));
    Ok(())
}

fn teks_arg(a: &[Value], nama: &str) -> Result<String, String> {
    match a.first() {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-1 harus string, dapat `{}`", nama, lain.type_name())),
        None => Err(format!("{}: argumen ke-1 wajib diisi", nama)),
    }
}

/// `xml::parse(teks)` → `Result<dict>`: akar `{nama, atribut, anak}`; anak berupa dict yang sama atau teks.
fn x_parse(a: &[Value]) -> Result<Value, String> {
    let t = teks_arg(a, "xml::parse")?;
    Ok(match urai(&t) {
        Ok(s) => ok_value(ke_nilai(s)),
        Err(e) => err_value(format!("xml::parse: {}", e)),
    })
}

/// `xml::stringify(simpul)` → `Result<string>`: kebalikan `xml::parse`, atribut urut nama.
fn x_stringify(a: &[Value]) -> Result<Value, String> {
    let v = a.first().ok_or("xml::stringify: argumen ke-1 wajib diisi")?;
    let mut o = String::new();
    Ok(match tulis(v, &mut o, 1) {
        Ok(()) => ok_value(Value::teks(o)),
        Err(e) => err_value(format!("xml::stringify: {}", e)),
    })
}

/// `xml::escape(teks)` → `string`, aman di dalam teks MAUPUN nilai atribut berkutip ganda.
fn x_escape(a: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(escape(&teks_arg(a, "xml::escape")?, true).into()))
}

#[cfg(test)]
mod tes {
    /// AMAN-42: pemeriksaan atribut kembar dulu kuadrat (160 ribu atribut 34 dtk). 4x ukuran harus < 10x waktu.
    #[test]
    fn atribut_banyak_tetap_linear() {
        let waktu = |n: usize| {
            let teks = format!("<a{}/>", (0..n).map(|i| format!(" k{}=\"v\"", i)).collect::<String>());
            (0..3).map(|_| { let t = std::time::Instant::now(); assert!(urai(&teks).is_ok()); t.elapsed() }).min().unwrap()
        };
        let (kecil, besar) = (waktu(20_000), waktu(80_000));
        assert!(besar < kecil * 10, "4x atribut makan {:?} lawan {:?} — kuadrat lagi?", besar, kecil);
    }

    use super::*;

    fn ditolak(s: &str, kata: &str) {
        match urai(s) {
            Ok(x) => panic!("{:?} diterima: {:?}", s, x),
            Err(e) => assert!(e.contains(kata), "{:?} ditolak dengan kalimat lain: {}", s, e),
        }
    }

    #[test]
    fn dokumen_sah_terurai() {
        let s = urai("<?xml version=\"1.0\"?><!-- c --><a x='1' y=\"&lt;&#x41;&#66;\">t<b/><![CDATA[<i>]]>&amp;</a>").unwrap();
        assert_eq!(s, Simpul::Elemen("a".into(), vec![("x".into(), "1".into()), ("y".into(), "<AB".into())],
            vec![Simpul::Teks("t".into()), Simpul::Elemen("b".into(), vec![], vec![]), Simpul::Teks("<i>&".into())]));
    }

    #[test]
    fn serangan_klasik_ditolak() {
        // XXE dan ledakan entitas: DTD tidak pernah dibaca.
        ditolak("<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><r>&x;</r>", "XXE");
        ditolak("<!DOCTYPE l [<!ENTITY a \"aaaa\"><!ENTITY b \"&a;&a;&a;\">]><l>&b;</l>", "ledakan entitas");
        ditolak("<r>&x;</r>", "tidak dikenal");
        ditolak(&format!("{}{}", "<a>".repeat(300), "</a>".repeat(300)), "bersarang lebih dari");
        ditolak("<a x='1' x='2'/>", "dua kali");
        ditolak("<a>&#0;</a>", "bukan karakter XML");
        ditolak("<a>&#xD800;</a>", "bukan karakter XML");
        ditolak("<a></b>", "menutup");
        ditolak("<a/><b/>", "satu akar");
        ditolak("<a x='<'/>", "`<` di dalam nilai atribut");
        ditolak("<a>]]></a>", "di luar CDATA");
    }

    #[test]
    fn penulis_tidak_bisa_disisipi() {
        assert!(!nama_sah("a onload=x") && !nama_sah("a><script") && !nama_sah("1a") && nama_sah("svg:rect"));
        assert_eq!(escape("\"<&>\n", true), "&quot;&lt;&amp;&gt;&#10;");
        // Bolak-balik: atribut berisi kutip dan baris baru kembali utuh.
        let s = urai(&format!("<a t=\"{}\"/>", escape("x\"y\nz", true))).unwrap();
        assert_eq!(s, Simpul::Elemen("a".into(), vec![("t".into(), "x\"y\nz".into())], vec![]));
    }
}
