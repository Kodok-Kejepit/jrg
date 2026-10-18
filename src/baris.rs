// src/baris.rs
// Penyuntingan satu baris: panah, riwayat, Home/End, hapus kata. Dipakai REPL.

use crate::term::{baca_tombol_terminal, ukuran, Raw, Tombol};
use std::io::Write;

pub struct Penyunting {
    riwayat: Vec<String>,
    /// Batas supaya sesi panjang tidak menumbuhkan memori tanpa henti.
    maks: usize,
}

/// Keadaan satu baris yang sedang disunting. Dipisah dari terminal supaya
/// aturan tiap tombol bisa diuji tanpa PTY — yang di CI belum tentu ada.
#[derive(Default, Clone)]
pub struct Keadaan {
    pub isi: Vec<char>,
    pub kursor: usize,
    /// Posisi di riwayat; `None` berarti sedang mengetik baris baru.
    pub ke_riwayat: Option<usize>,
    /// Baris yang sedang diketik sebelum riwayat dibuka.
    pub simpanan: String,
}

impl Keadaan {
    pub fn teks(&self) -> String { self.isi.iter().collect() }
}

/// Apa yang harus dilakukan pemanggil sesudah sebuah tombol diterapkan.
#[derive(Debug, PartialEq, Eq)]
pub enum Aksi { Lanjut, Kirim, Batal, Selesai, Bersihkan }

/// Terapkan satu tombol. Fungsi ini TIDAK menyentuh terminal, jadi seluruh
/// aturan penyuntingan bisa diperiksa tanpa menjalankan apa pun.
pub fn terapkan(k: &mut Keadaan, t: &Tombol, riwayat: &[String]) -> Aksi {
    match t {
        Tombol::Enter => return Aksi::Kirim,
        Tombol::Ctrl('c') => return Aksi::Batal,
        Tombol::Ctrl('d') => {
            if k.isi.is_empty() { return Aksi::Selesai; }
            // Di tengah baris ia berarti "hapus ke depan", sama seperti di shell.
            if k.kursor < k.isi.len() { k.isi.remove(k.kursor); }
        }
        Tombol::Karakter(c) => { k.isi.insert(k.kursor, *c); k.kursor += 1; }
        Tombol::Backspace => {
            if k.kursor > 0 { k.kursor -= 1; k.isi.remove(k.kursor); }
        }
        Tombol::Delete => { if k.kursor < k.isi.len() { k.isi.remove(k.kursor); } }
        Tombol::Kiri => { if k.kursor > 0 { k.kursor -= 1; } }
        Tombol::Kanan => { if k.kursor < k.isi.len() { k.kursor += 1; } }
        Tombol::Home | Tombol::Ctrl('a') => k.kursor = 0,
        Tombol::End | Tombol::Ctrl('e') => k.kursor = k.isi.len(),
        Tombol::Ctrl('u') => { k.isi.drain(..k.kursor); k.kursor = 0; }
        Tombol::Ctrl('k') => k.isi.truncate(k.kursor),
        Tombol::Ctrl('w') => hapus_kata(k),
        // Alt+Backspace: hapus satu kata, seperti di shell.
        Tombol::Ubah(2, d) if **d == Tombol::Backspace => hapus_kata(k),
        // Yang disunting di sini satu baris, jadi baris baru di tempelan jadi spasi.
        Tombol::Tempel { teks, .. } => {
            for c in teks.chars() {
                let c = if c == '\n' || c == '\t' { ' ' } else { c };
                if !c.is_control() { k.isi.insert(k.kursor, c); k.kursor += 1; }
            }
        }
        Tombol::Ctrl('l') => return Aksi::Bersihkan,
        // Ctrl+panah: pindah per KATA.
        Tombol::Ubah(m, d) if m & 4 != 0 => match **d {
            Tombol::Kiri => k.kursor = awal_kata(&k.isi, k.kursor),
            Tombol::Kanan => k.kursor = akhir_kata(&k.isi, k.kursor),
            Tombol::Home => k.kursor = 0,
            Tombol::End => k.kursor = k.isi.len(),
            _ => return Aksi::Lanjut,
        },
        Tombol::Atas => {
            if riwayat.is_empty() { return Aksi::Lanjut; }
            let i = match k.ke_riwayat {
                None => { k.simpanan = k.teks(); riwayat.len() - 1 }
                Some(0) => 0,
                Some(i) => i - 1,
            };
            k.ke_riwayat = Some(i);
            k.isi = riwayat[i].chars().collect();
            k.kursor = k.isi.len();
        }
        Tombol::Bawah => match k.ke_riwayat {
            None => return Aksi::Lanjut,
            Some(i) if i + 1 < riwayat.len() => {
                k.ke_riwayat = Some(i + 1);
                k.isi = riwayat[i + 1].chars().collect();
                k.kursor = k.isi.len();
            }
            Some(_) => {
                // Turun dari entri terakhir mengembalikan yang sedang diketik.
                k.ke_riwayat = None;
                k.isi = k.simpanan.chars().collect();
                k.kursor = k.isi.len();
            }
        },
        _ => return Aksi::Lanjut,
    }
    Aksi::Lanjut
}

/// Apa yang terjadi pada satu pembacaan baris.
pub enum Hasil {
    Baris(String),
    /// Ctrl-D pada baris kosong: akhir masukan.
    Selesai,
    /// Ctrl-C: baris dibatalkan, sesi tetap hidup.
    Batal,
}

impl Penyunting {
    pub fn baru() -> Self { Penyunting { riwayat: Vec::new(), maks: 500 } }

    pub fn ingat(&mut self, b: &str) {
        if b.trim().is_empty() { return; }
        // Perintah yang sama berturut-turut tidak digandakan — menekan panah
        // atas lima kali untuk melewati lima salinan yang sama itu menyebalkan.
        if self.riwayat.last().map(|x| x == b).unwrap_or(false) { return; }
        self.riwayat.push(b.to_string());
        if self.riwayat.len() > self.maks { self.riwayat.remove(0); }
    }

    pub fn riwayat(&self) -> &[String] { &self.riwayat }

    /// Baca satu baris dengan penyuntingan penuh. `prompt` dicetak ulang tiap
    /// kali layar digambar, jadi ia harus tanpa baris baru.
    pub fn baca(&mut self, prompt: &str) -> Hasil {
        let raw = Raw::masuk();
        if !raw.aktif() {
            // Tanpa raw mode (mis. di dalam pipa) penyuntingan tidak mungkin;
            // membaca baris biasa lebih baik daripada menolak jalan.
            drop(raw);
            return self.baca_polos(prompt);
        }
        // Loopnya memanggil `terapkan` yang sama dengan yang diuji — bukan
        // salinan kedua, yang akan berpisah jalan pada perubahan berikutnya.
        let mut k = Keadaan::default();
        gambar(prompt, &k.isi, k.kursor);
        loop {
            let Some(t) = baca_tombol_terminal() else {
                crate::term::tulis("\r\n");
                return Hasil::Selesai;
            };
            match terapkan(&mut k, &t, &self.riwayat) {
                Aksi::Kirim => {
                    crate::term::tulis("\r\n");
                    return Hasil::Baris(k.teks());
                }
                Aksi::Batal => {
                    crate::term::tulis("^C\r\n");
                    return Hasil::Batal;
                }
                Aksi::Selesai => {
                    crate::term::tulis("\r\n");
                    return Hasil::Selesai;
                }
                Aksi::Bersihkan => crate::term::tulis("\x1b[2J\x1b[H"),
                Aksi::Lanjut => {}
            }
            gambar(prompt, &k.isi, k.kursor);
        }
    }

    fn baca_polos(&mut self, prompt: &str) -> Hasil {
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
        let mut b = String::new();
        match crate::host::baca_baris(&mut b) {
            Ok(0) | Err(_) => Hasil::Selesai,
            Ok(_) => Hasil::Baris(b.trim_end_matches(['\n', '\r']).to_string()),
        }
    }
}

fn hapus_kata(k: &mut Keadaan) {
    let awal = awal_kata(&k.isi, k.kursor);
    k.isi.drain(awal..k.kursor);
    k.kursor = awal;
}

fn awal_kata(baris: &[char], kursor: usize) -> usize {
    let mut i = kursor;
    while i > 0 && baris[i - 1].is_whitespace() { i -= 1; }
    while i > 0 && !baris[i - 1].is_whitespace() { i -= 1; }
    i
}

/// Cermin `awal_kata`: lewati spasi dulu, baru katanya.
fn akhir_kata(baris: &[char], kursor: usize) -> usize {
    let n = baris.len();
    let mut i = kursor;
    while i < n && baris[i].is_whitespace() { i += 1; }
    while i < n && !baris[i].is_whitespace() { i += 1; }
    i
}

/// Gambar ulang satu baris. Yang ditulis adalah baris PENUH tiap kali, karena
/// penggambaran parsial harus tahu persis apa yang berubah — dan yang salah
/// meninggalkan sisa karakter lama di layar.
fn gambar(prompt: &str, baris: &[char], kursor: usize) {
    let (kolom, _) = ukuran();
    let lebar_prompt = crate::lebar::teks(prompt);
    let muat = (kolom as usize).saturating_sub(lebar_prompt + 1);
    // Kalau barisnya lebih panjang dari layar, yang ditampilkan jendela di
    // sekitar kursor — bukan potongan dari awal, yang membuat kursor hilang.
    let (potong, kursor_tampil) = jendela(baris, kursor, muat);
    // `ESC [ 0 C` tetap bergeser SATU kolom, jadi geseran nol tidak ditulis sama sekali.
    let geser = match lebar_prompt + kursor_tampil {
        0 => String::new(),
        n => format!("\x1b[{}C", n),
    };
    crate::term::tulis(&format!("\r\x1b[K{}{}\r{}", prompt, potong, geser));
}

/// Potongan baris yang muat di `muat` KOLOM LAYAR di sekitar kursor, dan kolom kursor di
/// dalamnya. Dihitung dalam lebar tampil: huruf Asia Timur dua kolom, tanda gabung nol.
fn jendela(baris: &[char], kursor: usize, muat: usize) -> (String, usize) {
    let lebar: Vec<usize> = baris.iter().map(|c| crate::lebar::karakter(*c)).collect();
    let rentang = |a: usize, b: usize| lebar[a..b].iter().sum::<usize>();
    if muat == 0 || rentang(0, baris.len()) <= muat {
        return (baris.iter().collect(), rentang(0, kursor));
    }
    // Awal jendela digeser ke kanan sampai kursor masuk, lalu ke kiri lagi selama
    // ekor barisnya masih muat utuh — supaya ujung baris tidak menyisakan ruang kosong.
    let mut mulai = 0;
    while rentang(mulai, kursor) > muat.saturating_sub(1) { mulai += 1; }
    while mulai > 0 && rentang(mulai - 1, baris.len()) <= muat { mulai -= 1; }
    let (mut akhir, mut terpakai) = (mulai, 0);
    while akhir < baris.len() && terpakai + lebar[akhir] <= muat {
        terpakai += lebar[akhir];
        akhir += 1;
    }
    (baris[mulai..akhir].iter().collect(), rentang(mulai, kursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hapus_kata_berhenti_di_batas_kata() {
        let b: Vec<char> = "halo dunia luas".chars().collect();
        assert_eq!(awal_kata(&b, b.len()), 11);      // "luas"
        assert_eq!(awal_kata(&b, 10), 5);            // di tengah "dunia"
        assert_eq!(awal_kata(&b, 0), 0);
        // Spasi di kiri kursor dilewati DULU, baru katanya dihapus.
        let c: Vec<char> = "halo   ".chars().collect();
        assert_eq!(awal_kata(&c, c.len()), 0);
    }

    /// Baris yang lebih panjang dari layar harus tetap memperlihatkan kursor.
    /// Memotong dari awal membuat kursor keluar layar tanpa ada yang melapor.
    #[test]
    fn jendela_mengikuti_kursor() {
        let s: Vec<char> = "0123456789abcdef".chars().collect();
        let (p, k) = jendela(&s, 15, 8);
        assert!(k < 8, "kursor harus di dalam jendela, dapat {}", k);
        assert!(p.chars().count() <= 8);
        assert!(p.ends_with('f'), "ujung kanan harus terlihat, dapat {:?}", p);
        // Kursor di ujung: ekornya tampil penuh dan kursor tepat sesudahnya.
        assert_eq!(jendela(&s, 16, 8), ("89abcdef".to_string(), 8));
        // Kursor di awal: jendela tidak boleh bergeser.
        let (p2, k2) = jendela(&s, 0, 8);
        assert_eq!(k2, 0);
        assert!(p2.starts_with('0'));
        // Muat lebih besar dari isi: apa adanya.
        let abc: Vec<char> = "abc".chars().collect();
        let (p3, k3) = jendela(&abc, 2, 8);
        assert_eq!((p3.as_str(), k3), ("abc", 2));
    }

    /// Kolom kursor dihitung dalam LEBAR TAMPIL. Dulu dihitung per karakter, jadi
    /// sesudah satu huruf Asia Timur kursor tergambar satu kolom terlalu kiri.
    #[test]
    fn jendela_menghitung_lebar_tampil() {
        let s: Vec<char> = "日本語x".chars().collect();
        assert_eq!(jendela(&s, 3, 20), ("日本語x".to_string(), 6));
        assert_eq!(jendela(&s, 1, 20).1, 2);
        // Tanda gabung menumpang di kolom hurufnya.
        let e: Vec<char> = "e\u{0301}a".chars().collect();
        assert_eq!(jendela(&e, 2, 20).1, 1);
        // Jendela sempit: yang tampil tidak pernah lebih lebar dari yang muat.
        let p: Vec<char> = "日本語日本語日本語".chars().collect();
        for kursor in 0..=p.len() {
            let (potong, k) = jendela(&p, kursor, 7);
            assert!(crate::lebar::teks(&potong) <= 7, "{:?} melebihi 7 kolom", potong);
            assert!(k <= 7, "kursor {} di kolom {}", kursor, k);
        }
    }

    #[test]
    fn tempelan_masuk_sebagai_satu_baris() {
        let t = Tombol::Tempel { teks: "a\nb\tc".to_string(), terpotong: false };
        let (teks, k, _) = jalankan("x", &[t], &[]);
        assert_eq!((teks.as_str(), k), ("xa b c", 6));
        let alt_bs = Tombol::Ubah(2, Box::new(Tombol::Backspace));
        let (teks, _, _) = jalankan("satu dua", &[alt_bs], &[]);
        assert_eq!(teks, "satu ");
    }

    fn jalankan(awal: &str, tombol: &[Tombol], riwayat: &[String]) -> (String, usize, Aksi) {
        let mut k = Keadaan { isi: awal.chars().collect(), kursor: awal.chars().count(),
                              ..Default::default() };
        let mut a = Aksi::Lanjut;
        for t in tombol { a = terapkan(&mut k, t, riwayat); }
        (k.teks(), k.kursor, a)
    }

    /// Sisip di TENGAH baris. Sebuah penyunting yang cuma bisa menambah di
    /// ujung terlihat bekerja sampai seseorang menekan panah kiri.
    #[test]
    fn sisip_dan_hapus_di_tengah() {
        let (t, k, _) = jalankan("wok(2)", &[Tombol::Kiri, Tombol::Karakter('+'),
                                             Tombol::Karakter('3')], &[]);
        assert_eq!(t, "wok(2+3)");
        assert_eq!(k, 7);
        // Backspace menghapus ke KIRI kursor, Delete ke kanan.
        let (t, _, _) = jalankan("abc", &[Tombol::Kiri, Tombol::Backspace], &[]);
        assert_eq!(t, "ac");
        let (t, _, _) = jalankan("abc", &[Tombol::Home, Tombol::Delete], &[]);
        assert_eq!(t, "bc");
    }

    #[test]
    fn home_end_dan_potong_baris() {
        // Ctrl+panah pindah per KATA, dan batasnya HARUS sama dengan Ctrl-W —
        // dua definisi akan membuat hapus-kata dan pindah-kata berhenti di
        // tempat yang berbeda untuk baris yang sama.
        let ctrl = |d: Tombol| Tombol::Ubah(4, Box::new(d));
        let (_, k, _) = jalankan("satu dua tiga", &[ctrl(Tombol::Kiri)], &[]);
        assert_eq!(k, 9, "ctrl-kiri tidak berhenti di awal kata terakhir");
        let (_, k, _) = jalankan("satu dua", &[Tombol::Home, ctrl(Tombol::Kanan)], &[]);
        assert_eq!(k, 4, "ctrl-kanan tidak berhenti di akhir kata pertama");
        let (t, _, _) = jalankan("satu dua tiga", &[Tombol::Ctrl('w')], &[]);
        assert_eq!(t, "satu dua ", "ctrl-w tidak sepakat dengan ctrl-kiri");
        let (_, k, _) = jalankan("abc", &[ctrl(Tombol::Home)], &[]);
        assert_eq!(k, 0);

        let (t, k, _) = jalankan("ok(7)", &[Tombol::Home, Tombol::Karakter('w')], &[]);
        assert_eq!((t.as_str(), k), ("wok(7)", 1));
        // Ctrl-U membuang ke KIRI kursor, Ctrl-K ke kanan.
        let (t, k, _) = jalankan("buang ini", &[Tombol::Ctrl('u')], &[]);
        assert_eq!((t.as_str(), k), ("", 0));
        let (t, _, _) = jalankan("simpan buang", &[Tombol::Home, Tombol::Ctrl('k')], &[]);
        assert_eq!(t, "");
        let (t, _, _) = jalankan("ab", &[Tombol::Kiri, Tombol::Ctrl('k')], &[]);
        assert_eq!(t, "a");
        // Ctrl-A dan Ctrl-E setara Home dan End.
        let (_, k, _) = jalankan("halo", &[Tombol::Ctrl('a')], &[]);
        assert_eq!(k, 0);
        let (_, k, _) = jalankan("halo", &[Tombol::Ctrl('a'), Tombol::Ctrl('e')], &[]);
        assert_eq!(k, 4);
    }

    #[test]
    fn riwayat_naik_turun_dan_kembali_ke_yang_diketik() {
        let r = vec!["satu".to_string(), "dua".to_string()];
        // Naik sekali memberi entri TERAKHIR.
        let (t, _, _) = jalankan("", &[Tombol::Atas], &r);
        assert_eq!(t, "dua");
        // Naik dua kali memberi yang sebelumnya, dan tidak melewati ujungnya.
        let (t, _, _) = jalankan("", &[Tombol::Atas, Tombol::Atas, Tombol::Atas], &r);
        assert_eq!(t, "satu");
        // Turun dari entri terakhir MENGEMBALIKAN yang sedang diketik —
        // kalau tidak, apa yang sudah ditulis hilang tanpa bisa dipulihkan.
        let (t, _, _) = jalankan("sedang diketik", &[Tombol::Atas, Tombol::Bawah], &r);
        assert_eq!(t, "sedang diketik");
        // Turun tanpa pernah naik tidak mengubah apa pun.
        let (t, _, _) = jalankan("utuh", &[Tombol::Bawah], &r);
        assert_eq!(t, "utuh");
        // Riwayat kosong: panah atas tidak boleh menghapus yang diketik.
        let (t, _, _) = jalankan("tetap", &[Tombol::Atas], &[]);
        assert_eq!(t, "tetap");
    }

    #[test]
    fn tombol_menghasilkan_aksi_yang_benar() {
        let (_, _, a) = jalankan("x", &[Tombol::Enter], &[]);
        assert_eq!(a, Aksi::Kirim);
        let (_, _, a) = jalankan("x", &[Tombol::Ctrl('c')], &[]);
        assert_eq!(a, Aksi::Batal);
        // Ctrl-D pada baris KOSONG mengakhiri; di tengah baris ia menghapus.
        let (_, _, a) = jalankan("", &[Tombol::Ctrl('d')], &[]);
        assert_eq!(a, Aksi::Selesai);
        let (t, _, a) = jalankan("abc", &[Tombol::Home, Tombol::Ctrl('d')], &[]);
        assert_eq!((t.as_str(), a), ("bc", Aksi::Lanjut));
    }

    /// Penyuntingan harus bekerja pada karakter, bukan byte — kalau tidak,
    /// satu Backspace merusak huruf beraksen dan kerusakannya tidak melapor.
    #[test]
    fn penyuntingan_pada_karakter_bukan_byte() {
        let (t, k, _) = jalankan("héllo", &[Tombol::Backspace], &[]);
        assert_eq!((t.as_str(), k), ("héll", 4));
        let (t, _, _) = jalankan("日本語", &[Tombol::Home, Tombol::Delete], &[]);
        assert_eq!(t, "本語");
        let (t, k, _) = jalankan("aé", &[Tombol::Kiri, Tombol::Karakter('X')], &[]);
        assert_eq!((t.as_str(), k), ("aXé", 2));
    }

    #[test]
    fn riwayat_tidak_menyimpan_kosong_atau_kembar() {
        let mut p = Penyunting::baru();
        p.ingat("a"); p.ingat("a"); p.ingat("  "); p.ingat("b");
        assert_eq!(p.riwayat(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn riwayat_dibatasi_supaya_tidak_tumbuh_tanpa_henti() {
        let mut p = Penyunting::baru();
        p.maks = 3;
        for i in 0..10 { p.ingat(&format!("baris {}", i)); }
        assert_eq!(p.riwayat().len(), 3);
        assert_eq!(p.riwayat()[2], "baris 9");
    }
}
