// src/stdlib/terminal.rs
// `term::` — kendali terminal untuk program JRG. Mesinnya sama dengan yang
// dipakai REPL (`crate::term`), jadi tidak ada salinan kedua.

use crate::interpreter::Value;
use crate::stdlib::sistem::lewat_hasil;
use crate::term::{self, AksiTetikus, Bacaan, Tombol};
use crate::vm::Vm;

pub fn daftar(vm: &mut Vm) {
    vm.def("term::raw", |a| lewat_hasil(a, b_raw));
    vm.def("term::mode", |a| lewat_hasil(a, b_mode));
    vm.def("term::ukuran", |a| lewat_hasil(a, b_ukuran));
    vm.def("term::interaktif", |a| lewat_hasil(a, b_interaktif));
    vm.def("term::tombol", |a| lewat_hasil(a, b_tombol));
    vm.def("term::tombol_tunggu", |a| lewat_hasil(a, b_tombol_tunggu));
    vm.def("term::tulis", |a| lewat_hasil(a, b_tulis));
    vm.def("term::lebar", |a| lewat_hasil(a, b_lebar));
    vm.def("term::lebar_tiap", |a| lewat_hasil(a, b_lebar_tiap));
}

fn bool_ke(a: &[Value], i: usize, fungsi: &str) -> Result<bool, String> {
    match a.get(i) {
        Some(Value::Bool(b)) => Ok(*b),
        Some(l) => Err(format!("{}: argumen ke-{} harus `true` atau `false`, dapat `{}`",
            fungsi, i + 1, l.type_name())),
        None => Err(format!("{}: argumen ke-{} belum diisi", fungsi, i + 1)),
    }
}

fn teks_ke<'a>(a: &'a [Value], i: usize, fungsi: &str) -> Result<&'a str, String> {
    match a.get(i) {
        Some(Value::Str(s)) => Ok(s.as_str()),
        Some(l) => Err(format!("{}: argumen ke-{} harus teks, dapat `{}`",
            fungsi, i + 1, l.type_name())),
        None => Err(format!("{}: argumen ke-{} belum diisi", fungsi, i + 1)),
    }
}

fn b_raw(a: &[Value]) -> Result<Value, String> {
    let nyala = bool_ke(a, 0, "term::raw")?;
    Ok(Value::Bool(term::raw_program(nyala)))
}

/// Mode layar: layar alternatif, laporan tetikus, tempel berkurung, kursor. Yang
/// dinyalakan di sini dimatikan sendiri di jalan keluar mana pun (`term::pulihkan`).
fn b_mode(a: &[Value]) -> Result<Value, String> {
    let nama = teks_ke(a, 0, "term::mode")?;
    let nyala = bool_ke(a, 1, "term::mode")?;
    match term::mode(nama, nyala) {
        Some(jadi) => Ok(Value::Bool(jadi)),
        None => {
            let ada = term::NAMA_MODE.join(", ");
            Err(match crate::diag::saran_nama_terdekat(nama, term::NAMA_MODE.iter().copied()) {
                Some(s) => format!(
                    "term::mode: mode `{}` tidak ada — maksudnya `{}`? Yang ada: {}", nama, s, ada),
                None => format!("term::mode: mode `{}` tidak ada — yang ada: {}", nama, ada),
            })
        }
    }
}

fn b_ukuran(_: &[Value]) -> Result<Value, String> {
    let (k, b) = term::ukuran();
    Ok(Value::Tuple(vec![Value::Int(k as i128), Value::Int(b as i128)].into()))
}

fn b_interaktif(_: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(term::interaktif()))
}

/// Satu tombol, sebagai NAMA — bukan kode angka. Sebuah program yang
/// membandingkan dengan `27` harus tahu tabel escape; yang membandingkan
/// dengan `"atas"` tidak.
fn b_tombol(_: &[Value]) -> Result<Value, String> {
    match term::baca_tombol_terminal() {
        None => Ok(Value::Str("eof".to_string())),
        Some(t) => Ok(Value::Str(nama_tombol(&t))),
    }
}

/// Paling lama menunggu satu panggilan: sehari. Di atas itu hampir pasti satuannya
/// yang keliru (detik ditulis sebagai milidetik tidak sampai ke sini; sebaliknya iya).
const MAKS_TUNGGU_MS: i128 = 24 * 60 * 60 * 1000;

/// Seperti `term::tombol`, tapi menyerah sesudah `ms` milidetik: `None` berarti tidak
/// ada tombol — bukan teks kosong, yang akan terlihat seperti sebuah jawaban.
fn b_tombol_tunggu(a: &[Value]) -> Result<Value, String> {
    let ms = match a.first() {
        Some(Value::Int(n)) if (0..=MAKS_TUNGGU_MS).contains(n) => *n as u64,
        Some(Value::Int(n)) => return Err(format!(
            "term::tombol_tunggu: lama tunggu harus 0 sampai {} milidetik (sehari), dapat {}",
            MAKS_TUNGGU_MS, n)),
        Some(l) => return Err(format!(
            "term::tombol_tunggu: lama tunggu harus bilangan bulat milidetik, dapat `{}`", l.type_name())),
        None => return Err("term::tombol_tunggu: lama tunggu (milidetik) belum diisi".to_string()),
    };
    Ok(match term::baca_tombol_tunggu(ms) {
        Bacaan::Tombol(t) => crate::interpreter::some_value(Value::Str(nama_tombol(&t))),
        Bacaan::Habis => crate::interpreter::some_value(Value::Str("eof".to_string())),
        Bacaan::Kosong => crate::interpreter::none_value(),
    })
}

fn nama_aksi(a: AksiTetikus) -> &'static str {
    match a {
        AksiTetikus::Kiri => "kiri",
        AksiTetikus::Tengah => "tengah",
        AksiTetikus::Kanan => "kanan",
        AksiTetikus::Lepas => "lepas",
        AksiTetikus::Seret => "seret",
        AksiTetikus::RodaAtas => "roda-atas",
        AksiTetikus::RodaBawah => "roda-bawah",
        AksiTetikus::RodaKiri => "roda-kiri",
        AksiTetikus::RodaKanan => "roda-kanan",
    }
}

fn awalan_ubah(m: u8) -> String {
    // Urutannya tetap: ctrl, alt, shift — supaya sebuah program bisa
    // membandingkan dengan string harfiah alih-alih menguraikan lagi.
    let mut n = String::new();
    if m & 4 != 0 { n.push_str("ctrl-"); }
    if m & 2 != 0 { n.push_str("alt-"); }
    if m & 1 != 0 { n.push_str("shift-"); }
    n
}

fn nama_ctrl(c: char) -> String {
    if c == ' ' { "spasi".to_string() } else { c.to_string() }
}

pub fn nama_tombol(t: &Tombol) -> String {
    match t {
        Tombol::Karakter(c) => c.to_string(),
        Tombol::Enter => "enter".into(),
        Tombol::Tab => "tab".into(),
        Tombol::BackTab => "shift-tab".into(),
        Tombol::Backspace => "backspace".into(),
        Tombol::Delete => "delete".into(),
        Tombol::Insert => "insert".into(),
        Tombol::Kiri => "kiri".into(),
        Tombol::Kanan => "kanan".into(),
        Tombol::Atas => "atas".into(),
        Tombol::Bawah => "bawah".into(),
        Tombol::Home => "home".into(),
        Tombol::End => "end".into(),
        Tombol::PageUp => "pgup".into(),
        Tombol::PageDown => "pgdn".into(),
        Tombol::F(n) => format!("f{}", n),
        Tombol::Esc => "esc".into(),
        Tombol::Ctrl(c) => format!("ctrl-{}", nama_ctrl(*c)),
        Tombol::Alt(c) => format!("alt-{}", c),
        // Ctrl dan Alt di dalam `Ubah` dilebur ke awalannya, supaya urutan awalan tetap
        // satu: `ESC` + Ctrl-S bernama `ctrl-alt-s`, bukan `alt-ctrl-s`.
        Tombol::Ubah(m, d) => match &**d {
            Tombol::Ctrl(c) => format!("{}{}", awalan_ubah(m | 4), nama_ctrl(*c)),
            Tombol::Alt(c) => format!("{}{}", awalan_ubah(m | 2), c),
            Tombol::BackTab => format!("{}tab", awalan_ubah(m | 1)),
            lain => format!("{}{}", awalan_ubah(*m), nama_tombol(lain)),
        },
        Tombol::Tempel { teks, terpotong: false } => format!("tempel:{}", teks),
        Tombol::Tempel { teks, terpotong: true } => format!("tempel-terpotong:{}", teks),
        Tombol::Tetikus { aksi, x, y } => format!("tetikus:{}:{}:{}", nama_aksi(*aksi), x, y),
        // Dilaporkan, bukan dibuang: tombol yang tidak dikenal tetap terlihat oleh program.
        // Tanpa byte pun namanya tetap dua karakter, jadi tak tertukar dengan `?` yang diketik.
        Tombol::TakDikenal(b) if b.is_empty() => "?-".to_string(),
        Tombol::TakDikenal(b) => format!("?{}", b.iter()
            .map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join("")),
    }
}

fn b_tulis(a: &[Value]) -> Result<Value, String> {
    let s = teks_ke(a, 0, "term::tulis")?;
    term::tulis(s);
    Ok(Value::Unit)
}

/// Kolom layar yang ditempati teks: huruf lebar dua, tanda gabung dan kendali nol.
fn b_lebar(a: &[Value]) -> Result<Value, String> {
    let s = teks_ke(a, 0, "term::lebar")?;
    Ok(Value::Int(crate::lebar::teks(s) as i128))
}

/// Lebar TIAP karakter, sekali panggil — untuk memetakan indeks karakter ke kolom layar.
fn b_lebar_tiap(a: &[Value]) -> Result<Value, String> {
    let s = teks_ke(a, 0, "term::lebar_tiap")?;
    Ok(Value::Array(s.chars().map(|c| Value::Int(crate::lebar::karakter(c) as i128))
        .collect::<Vec<_>>().into()))
}

#[cfg(test)]
mod tes {
    use super::*;

    /// Nama tombol adalah antarmuka program JRG: sebuah nama yang berubah diam-diam
    /// membuat tombol di program pemakainya mati tanpa pesan.
    #[test]
    fn nama_tombol_tetap() {
        let b = |t: Tombol| Box::new(t);
        let kasus: Vec<(Tombol, &str)> = vec![
            (Tombol::Karakter('a'), "a"),
            (Tombol::Karakter('日'), "日"),
            (Tombol::Enter, "enter"),
            (Tombol::BackTab, "shift-tab"),
            (Tombol::Insert, "insert"),
            (Tombol::F(5), "f5"),
            (Tombol::F(12), "f12"),
            (Tombol::Ctrl('s'), "ctrl-s"),
            (Tombol::Ctrl(' '), "ctrl-spasi"),
            (Tombol::Ctrl('_'), "ctrl-_"),
            (Tombol::Alt('/'), "alt-/"),
            (Tombol::Ubah(4, b(Tombol::Kanan)), "ctrl-kanan"),
            (Tombol::Ubah(5, b(Tombol::Home)), "ctrl-shift-home"),
            (Tombol::Ubah(7, b(Tombol::Atas)), "ctrl-alt-shift-atas"),
            (Tombol::Ubah(1, b(Tombol::F(3))), "shift-f3"),
            (Tombol::Ubah(2, b(Tombol::Backspace)), "alt-backspace"),
            (Tombol::Ubah(2, b(Tombol::Enter)), "alt-enter"),
            (Tombol::Ubah(2, b(Tombol::Ctrl('s'))), "ctrl-alt-s"),
            (Tombol::Ubah(4, b(Tombol::Alt('x'))), "ctrl-alt-x"),
            (Tombol::Ubah(2, b(Tombol::BackTab)), "alt-shift-tab"),
            (Tombol::Tempel { teks: "a\nb".into(), terpotong: false }, "tempel:a\nb"),
            (Tombol::Tempel { teks: "a".into(), terpotong: true }, "tempel-terpotong:a"),
            (Tombol::Tetikus { aksi: AksiTetikus::Kiri, x: 12, y: 5 }, "tetikus:kiri:12:5"),
            (Tombol::Tetikus { aksi: AksiTetikus::RodaBawah, x: 1, y: 2 }, "tetikus:roda-bawah:1:2"),
            (Tombol::Ubah(4, b(Tombol::Tetikus { aksi: AksiTetikus::Kiri, x: 3, y: 4 })),
             "ctrl-tetikus:kiri:3:4"),
            (Tombol::TakDikenal(vec![0x1b, 0x5b, 0x39]), "?1b5b39"),
        ];
        for (t, nama) in kasus {
            assert_eq!(nama_tombol(&t), nama, "{:?}", t);
        }
    }

    /// Nama tombol khusus selalu lebih dari satu karakter, jadi program bisa membedakan
    /// KETIKAN (tepat satu karakter) dari tombol tanpa daftar pengecualian.
    #[test]
    fn cuma_ketikan_yang_satu_karakter() {
        let khusus = [Tombol::Enter, Tombol::Tab, Tombol::Esc, Tombol::Insert, Tombol::F(1),
            Tombol::Ctrl('a'), Tombol::Alt('a'), Tombol::TakDikenal(vec![]),
            Tombol::Tempel { teks: String::new(), terpotong: false }];
        for t in khusus {
            assert!(nama_tombol(&t).chars().count() > 1, "{:?} bernama satu karakter", t);
        }
    }

    #[test]
    fn argumen_salah_ditolak_dengan_sebabnya() {
        let g = |r: Result<Value, String>| r.unwrap_err();
        assert!(g(b_mode(&[Value::Str("layr".into()), Value::Bool(true)])).contains("maksudnya `layar`?"));
        assert!(g(b_mode(&[Value::Int(1), Value::Bool(true)])).contains("harus teks"));
        assert!(g(b_mode(&[Value::Str("layar".into()), Value::Int(1)])).contains("`true` atau `false`"));
        assert!(g(b_tombol_tunggu(&[Value::Int(-1)])).contains("0 sampai"));
        assert!(g(b_tombol_tunggu(&[Value::Float(1.5)])).contains("bilangan bulat"));
        assert!(g(b_tombol_tunggu(&[Value::Int(MAKS_TUNGGU_MS + 1)])).contains("sehari"));
        assert!(g(b_lebar(&[Value::Int(5)])).contains("harus teks"));
        assert_eq!(b_lebar(&[Value::Str("日本".into())]).unwrap().to_string(), "4");
        assert_eq!(b_lebar_tiap(&[Value::Str("a日\t".into())]).unwrap().to_string(), "[1, 2, 0]");
    }
}
