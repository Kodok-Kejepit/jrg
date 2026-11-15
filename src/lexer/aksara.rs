// src/lexer/aksara.rs — identifier yang tampak sama harus SAMA (homoglyph, CVE-2021-42694).
// Dua aturan: satu aksara per nama (UTS #39 "Moderately Restrictive") dan bentuk NFC.
// Tabelnya dibangkitkan `perkakas/aksara_unicode.py`.

use std::cmp::Ordering;
use std::sync::OnceLock;

use super::aksara_tabel as t;

/// Himpunan aksara sebagai bit; 192 cukup untuk semua aksara Unicode plus tiga himpunan tambahan.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Himpunan([u64; 3]);

impl Himpunan {
    const SEMUA: Himpunan = Himpunan([u64::MAX; 3]);
    const KOSONG: Himpunan = Himpunan([0; 3]);
    fn satu(i: u8) -> Self { let mut h = Self::KOSONG; h.pasang(i); h }
    fn pasang(&mut self, i: u8) { self.0[i as usize / 64] |= 1 << (i % 64); }
    fn ada(&self, i: u8) -> bool { self.0[i as usize / 64] & (1 << (i % 64)) != 0 }
    fn irisan(self, o: Self) -> Self { Himpunan([self.0[0] & o.0[0], self.0[1] & o.0[1], self.0[2] & o.0[2]]) }
    fn kosong(&self) -> bool { self.0 == [0; 3] }
}

struct Nomor { latn: u8, cyrl: u8, grek: u8, hani: u8, hira: u8, kana: u8, hang: u8, bopo: u8, jpan: u8, kore: u8, hanb: u8 }

fn nomor() -> &'static Nomor {
    static N: OnceLock<Nomor> = OnceLock::new();
    N.get_or_init(|| {
        let i = |k: &str| t::KODE.iter().position(|x| *x == k).expect("kode aksara di tabel") as u8;
        Nomor { latn: i("Latn"), cyrl: i("Cyrl"), grek: i("Grek"), hani: i("Hani"), hira: i("Hira"), kana: i("Kana"),
                hang: i("Hang"), bopo: i("Bopo"), jpan: i("Jpan"), kore: i("Kore"), hanb: i("Hanb") }
    })
}

fn cari<V: Copy>(daftar: &[(u32, u32, V)], cp: u32) -> Option<V> {
    daftar.binary_search_by(|(a, z, _)| if cp < *a { Ordering::Greater } else if cp > *z { Ordering::Less } else { Ordering::Equal })
        .ok().map(|i| daftar[i].2)
}

/// Aksara sebuah karakter, sudah ditambah himpunan Jpan/Kore/Hanb; `None` = Common/Inherited (cocok dengan semua).
fn aksara(c: char) -> Option<Himpunan> {
    let cp = c as u32;
    if cp < 0x80 { return if c.is_ascii_alphabetic() { Some(Himpunan::satu(nomor().latn)) } else { None }; }
    let mut h = match cari(t::BANYAK, cp) {
        Some(isi) => { let mut h = Himpunan::KOSONG; for i in isi { h.pasang(*i); } h }
        None => Himpunan::satu(cari(t::SATU, cp)?),
    };
    let n = nomor();
    if h.ada(n.hani) { h.pasang(n.jpan); h.pasang(n.kore); h.pasang(n.hanb); }
    if h.ada(n.hira) || h.ada(n.kana) { h.pasang(n.jpan); }
    if h.ada(n.hang) { h.pasang(n.kore); }
    if h.ada(n.bopo) { h.pasang(n.hanb); }
    Some(h)
}

/// Huruf Yunani yang tampak seperti huruf Latin (dari confusables.txt UTS #39).
const TIRUAN_YUNANI: &[char] = &['Α', 'Β', 'Ε', 'Ζ', 'Η', 'Ι', 'Κ', 'Μ', 'Ν', 'Ο', 'Ρ', 'Τ', 'Υ', 'Χ', 'ϒ', 'Ϲ', 'Ϳ',
    'α', 'γ', 'ι', 'κ', 'ν', 'ο', 'ρ', 'υ', 'χ', 'ϲ', 'ϳ', 'ϱ', 'ϰ'];

fn nama_aksara(h: Himpunan) -> &'static str {
    let n = nomor();
    if h.ada(n.latn) { return t::NAMA[n.latn as usize]; }
    (0..t::KODE.len() as u8).find(|i| h.ada(*i)).map(|i| t::NAMA[i as usize]).unwrap_or("?")
}

/// Galat atas sebuah identifier: posisi byte karakter yang disalahkan, panjangnya, dan kalimatnya.
pub(crate) struct Tolak { pub awal: usize, pub panjang: usize, pub pesan: String }

/// Diperiksa hanya untuk identifier yang memuat karakter non-ASCII.
pub(crate) fn periksa(nama: &str) -> Result<(), Tolak> {
    periksa_nfc(nama)?;
    periksa_campur(nama)
}

fn periksa_campur(nama: &str) -> Result<(), Tolak> {
    let n = nomor();
    let semua = nama.chars().filter_map(aksara).fold(Himpunan::SEMUA, Himpunan::irisan);
    if !semua.kosong() { return Ok(()); }
    // Latin plus SATU aksara lain boleh, kecuali Kiril dan Yunani — huruf merekalah yang meniru Latin.
    let sisa = nama.chars().filter_map(aksara).filter(|h| !h.ada(n.latn)).fold(Himpunan::SEMUA, Himpunan::irisan);
    let mut tanpa_tiruan = sisa;
    tanpa_tiruan.0[n.cyrl as usize / 64] &= !(1 << (n.cyrl % 64));
    tanpa_tiruan.0[n.grek as usize / 64] &= !(1 << (n.grek % 64));
    if !tanpa_tiruan.kosong() { return Ok(()); }
    // Latin bersama Yunani yang tidak meniru Latin (`Δx`, `σ_x`, `θ1`) tetap boleh.
    if sisa.ada(n.grek) && nama.chars().all(|c| !matches!(aksara(c), Some(h) if !h.ada(n.latn)) || !TIRUAN_YUNANI.contains(&c)) {
        return Ok(());
    }
    // Kelompokkan huruf per aksara; yang disalahkan kelompok terkecil.
    let mut kelompok: Vec<(&'static str, Vec<(usize, char)>)> = Vec::new();
    for (i, c) in nama.char_indices() {
        let Some(h) = aksara(c) else { continue };
        let a = nama_aksara(h);
        match kelompok.iter_mut().find(|(k, _)| *k == a) {
            Some((_, v)) => v.push((i, c)),
            None => kelompok.push((a, vec![(i, c)])),
        }
    }
    let salah = (0..kelompok.len()).rev().min_by_key(|i| kelompok[*i].1.len()).unwrap_or(0);
    let rinci: Vec<String> = kelompok.iter().map(|(a, v)| {
        if v.iter().all(|(_, c)| c.is_ascii()) { return format!("{} (`{}`)", a, v.iter().map(|(_, c)| c).collect::<String>()); }
        let huruf: Vec<String> = v.iter().take(4).map(|(_, c)| format!("`{}` U+{:04X}", c, *c as u32)).collect();
        format!("{} ({}{})", a, huruf.join(", "), if v.len() > 4 { ", …" } else { "" })
    }).collect();
    let (awal, c) = kelompok[salah].1[0];
    Err(Tolak { awal, panjang: c.len_utf8(), pesan: format!(
        "nama `{}` mencampur aksara: {} — huruf aksara lain bisa tampak persis sama dengan huruf Latin, \
         jadi dua nama yang terlihat sama bisa berarti dua hal berbeda (homoglyph, CVE-2021-42694)\n\
         bantuan: tulis seluruh nama dalam satu aksara; Latin bersama Han, kana atau Hangul tetap boleh",
        nama, rinci.join(" dan ")) })
}

fn ccc(c: char) -> u8 { if (c as u32) < 0x300 { 0 } else { cari(t::CCC, c as u32).unwrap_or(0) } }

fn gabungan(a: char, b: char) -> Option<char> {
    let (x, y) = (a as u32, b as u32);
    // Hangul digabung menurut rumus, bukan tabel (Unicode §3.12).
    if (0x1100..=0x1112).contains(&x) && (0x1161..=0x1175).contains(&y) {
        return char::from_u32(0xAC00 + ((x - 0x1100) * 21 + (y - 0x1161)) * 28);
    }
    if (0xAC00..=0xD7A3).contains(&x) && (x - 0xAC00) % 28 == 0 && (0x11A8..=0x11C2).contains(&y) {
        return char::from_u32(x + (y - 0x11A7));
    }
    t::PASANG.binary_search_by(|(p, q, _)| (*p, *q).cmp(&(x, y))).ok().and_then(|i| char::from_u32(t::PASANG[i].2))
}

/// Penguraian kanonik: kebalikan `PASANG`. Yang lain (pengecualian, tunggal) ada di `BUKAN_NFC`.
fn urai(c: char) -> Option<(char, char)> {
    static BALIK: OnceLock<Vec<(u32, u32, u32)>> = OnceLock::new();
    let b = BALIK.get_or_init(|| { let mut v: Vec<_> = t::PASANG.iter().map(|(a, b, c)| (*c, *a, *b)).collect(); v.sort(); v });
    let i = b.binary_search_by(|(k, _, _)| k.cmp(&(c as u32))).ok()?;
    Some((char::from_u32(b[i].1)?, char::from_u32(b[i].2)?))
}

fn urai_ke(c: char, ke: &mut Vec<char>) {
    let cp = c as u32;
    if (0xAC00..=0xD7A3).contains(&cp) {
        let s = cp - 0xAC00;
        ke.extend([0x1100 + s / 588, 0x1161 + (s % 588) / 28].iter().filter_map(|x| char::from_u32(*x)));
        if s % 28 != 0 { ke.extend(char::from_u32(0x11A7 + s % 28)); }
    } else if let Some((a, b)) = urai(c) { urai_ke(a, ke); urai_ke(b, ke); } else { ke.push(c); }
}

/// NFC (UAX #15): urai, urutkan tanda gabung menurut kelasnya, susun ulang.
fn nfc(s: &str) -> Vec<char> {
    let mut d = Vec::new();
    for c in s.chars() { urai_ke(c, &mut d); }
    for i in 1..d.len() {
        let mut j = i;
        while j > 0 && ccc(d[j]) != 0 && ccc(d[j - 1]) > ccc(d[j]) { d.swap(j, j - 1); j -= 1; }
    }
    let mut hasil: Vec<char> = Vec::with_capacity(d.len());
    let (mut induk, mut kelas_lalu) = (None::<usize>, 0u16);
    for c in d {
        let k = ccc(c) as u16;
        if let Some(i) = induk {
            if kelas_lalu < k || (kelas_lalu == 0 && hasil.len() == i + 1) {
                if let Some(g) = gabungan(hasil[i], c) { hasil[i] = g; continue; }
            }
        }
        if k == 0 { induk = Some(hasil.len()); }
        kelas_lalu = k;
        hasil.push(c);
    }
    hasil
}

fn kode(cs: &[char]) -> String { cs.iter().map(|c| format!("U+{:04X}", *c as u32)).collect::<Vec<_>>().join(" ") }

/// `e` + U+0301 dan `é` tampak sama, tapi tanpa ini adalah dua nama.
fn periksa_nfc(nama: &str) -> Result<(), Tolak> {
    let bantuan = "\nbantuan: kebanyakan penyunting menulis NFC; teks ini mungkin disalin dari nama berkas macOS \
                   atau dokumen lain — ketik ulang namanya";
    for (i, c) in nama.char_indices() {
        if let Ok(k) = t::BUKAN_NFC.binary_search_by(|(cp, _)| cp.cmp(&(c as u32))) {
            return Err(Tolak { awal: i, panjang: c.len_utf8(), pesan: format!(
                "nama `{}` tidak dalam bentuk NFC: `{}` (U+{:04X}) selalu ditulis sebagai `{}` di NFC, jadi dua \
                 nama yang tampak sama bisa berarti dua hal berbeda (homoglyph){}",
                nama, c, c as u32, t::BUKAN_NFC[k].1, bantuan) });
        }
    }
    let asli: Vec<char> = nama.chars().collect();
    let n = nfc(nama);
    if n == asli { return Ok(()); }
    let awal = asli.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let akhir = asli[awal..].iter().rev().zip(n[awal..].iter().rev()).take_while(|(a, b)| a == b).count();
    let (o, b) = (&asli[awal..asli.len() - akhir], &n[awal..n.len() - akhir]);
    let byte: usize = asli[..awal].iter().map(|c| c.len_utf8()).sum();
    Err(Tolak { awal: byte, panjang: o.iter().map(|c| c.len_utf8()).sum::<usize>().max(1), pesan: format!(
        "nama `{}` tidak dalam bentuk NFC: `{}` ({}) ditulis `{}` ({}) di NFC, jadi dua nama yang tampak sama \
         bisa berarti dua hal berbeda (homoglyph); bentuk NFC namanya `{}`{}",
        nama, o.iter().collect::<String>(), kode(o), b.iter().collect::<String>(), kode(b),
        n.iter().collect::<String>(), bantuan) })
}

#[cfg(test)]
mod tes {
    use super::*;

    fn ditolak(s: &str) -> String { periksa(s).err().map(|t| t.pesan).unwrap_or_default() }

    #[test]
    fn satu_aksara_dan_campuran_yang_sah_diterima() {
        for s in ["héllo", "wörld", "日本語", "a日b", "tx日本", "東京", "변수", "имя", "ελληνικά", "Δx", "σ_x", "θ1", "café",
                  "データ値", "x日本です", "𝔘𝔘", "नमस्ते", "valor_ñ", "ŧ", "ǅ"] {
            assert!(periksa(s).is_ok(), "{}: {}", s, ditolak(s));
        }
    }

    #[test]
    fn campuran_latin_kiril_yunani_ditolak() {
        let g = ditolak("p\u{0430}ypal");
        assert!(g.contains("Kiril (`а` U+0430)") && g.contains("Latin (`pypal`)") && g.contains("CVE-2021-42694"), "{}", g);
        let g = periksa("\u{0440}aypal").err().unwrap();
        assert_eq!((g.awal, g.panjang), (0, 2));
        assert!(ditolak("x\u{03BF}").contains("Yunani") && ditolak("\u{0394}x\u{03BD}").contains("Yunani"));
        assert!(ditolak("имяx").contains("Kiril"));
        assert!(ditolak("\u{0430}\u{03B1}").contains("Kiril"));
    }

    #[test]
    fn bukan_nfc_ditolak_dengan_bentuk_nfcnya() {
        let g = ditolak("cafe\u{0301}");
        assert!(g.contains("`é` (U+00E9)") && g.contains("NFC"), "{}", g);
        assert!(ditolak("\u{212B}ngstrom").contains("U+212B"));
        assert!(ditolak("\u{1100}\u{1161}").contains("U+AC00"));
        assert!(ditolak("a\u{0301}\u{0323}").contains("U+1EA1 U+0301"));
        assert!(ditolak("q\u{0301}\u{0323}").contains("(U+0323 U+0301) di NFC"));
        assert!(ditolak("\u{00DC}\u{031B}").contains("U+01AF U+0308"));
        assert!(periksa("a\u{0323}\u{0302}x").err().map(|t| t.pesan.contains("U+1EAD")).unwrap_or(false));
        assert!(periksa("x\u{0301}").is_ok() && periksa("q\u{0323}\u{0301}").is_ok());
    }

    /// Pemeriksa NFC dibandingkan dengan `unicodedata.is_normalized` Python atas teks acak yang dirakit
    /// dari karakter berisiko (tanda gabung, Hangul, karakter bukan-NFC).
    #[test]
    fn nfc_sepakat_dengan_unicodedata() {
        let akar = env!("CARGO_MANIFEST_DIR");
        let skrip = r#"
import random, unicodedata as u, sys
print(u.unidata_version)
random.seed(7)
kolam = [c for c in map(chr, range(0x20000)) if u.combining(c) or u.normalize('NFC', c) != c or u.decomposition(c)][:4000]
kolam += [chr(x) for x in list(range(0x1100, 0x1113)) + list(range(0x1161, 0x1176)) + list(range(0x11A8, 0x11C3))]
kolam += list("aeiouAEIOUxyz") + [chr(0xAC00), chr(0xAC01)]
kolam = [c for c in kolam if not 0xD800 <= ord(c) <= 0xDFFF]
for _ in range(30000):
    s = ''.join(random.choice(kolam) for _ in range(random.randint(1, 4)))
    print(int(u.is_normalized('NFC', s)), ' '.join('%X' % ord(c) for c in s))
"#;
        let Ok(k) = std::process::Command::new("python3").arg("-c").arg(skrip).current_dir(akar).output() else {
            eprintln!("LEWAT: python3 tidak ada, pemeriksa NFC tidak dibandingkan");
            return;
        };
        let teks = String::from_utf8_lossy(&k.stdout);
        let mut baris = teks.lines();
        let versi = baris.next().unwrap_or("");
        if versi != t::VERSI_NFC { eprintln!("LEWAT: unicodedata {} lain dari tabel ({})", versi, t::VERSI_NFC); return; }
        let mut beda = Vec::new();
        for b in baris {
            let (harap, kode) = b.split_once(' ').unwrap();
            let s: String = kode.split(' ').map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap()).collect();
            if (periksa_nfc(&s).is_ok()) != (harap == "1") { beda.push(kode.to_string()); }
        }
        assert!(beda.is_empty(), "{} beda, contoh: {:?}", beda.len(), &beda[..beda.len().min(8)]);
    }

    #[test]
    fn tabel_sepakat_dengan_pembangkitnya() {
        let akar = env!("CARGO_MANIFEST_DIR");
        let Ok(k) = std::process::Command::new("python3").arg("perkakas/aksara_unicode.py").arg("--periksa")
            .current_dir(akar).output() else { eprintln!("LEWAT: python3 tidak ada"); return; };
        let teks = String::from_utf8_lossy(&k.stdout);
        match k.status.code() {
            Some(0) => {}
            Some(3) => eprintln!("LEWAT: {} (tabel dari Unicode {})", teks.trim(), t::VERSI_AKSARA),
            lain => panic!("tabel aksara menyimpang dari pembangkitnya ({:?}): {}{}", lain, teks, String::from_utf8_lossy(&k.stderr)),
        }
    }
}
