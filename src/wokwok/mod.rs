// src/wokwok/mod.rs — src/wokwok/ — `wokwok::`, library numerik JRG

pub mod ops;
pub mod reduce;
pub mod linalg;
pub mod daftar;

use std::sync::Arc;

/// Tipe elemen. Sengaja sedikit: tiga ini menutup hampir semua kerja
/// numerik, dan tiap tipe tambahan mengalikan jumlah kombinasi operasi
/// biner yang harus benar DAN diuji.
#[derive(Clone, Debug, PartialEq)]
pub enum Buf {
    F64(Vec<f64>),
    I64(Vec<i64>),
    Bool(Vec<bool>),
}

impl Buf {
    pub fn len(&self) -> usize {
        match self {
            Buf::F64(v) => v.len(),
            Buf::I64(v) => v.len(),
            Buf::Bool(v) => v.len(),
        }
    }
    pub fn nama(&self) -> &'static str {
        match self {
            Buf::F64(_) => "f64",
            Buf::I64(_) => "i64",
            Buf::Bool(_) => "bool",
        }
    }
    /// Pinjam sebagai `&[f64]` KALAU memang sudah f64.
    pub fn slice_f64(&self) -> Option<&[f64]> {
        match self { Buf::F64(v) => Some(v), _ => None }
    }

    /// f64 tanpa menyalin kalau bisa, dengan menyalin kalau harus.
    pub fn pinjam_f64(&self) -> std::borrow::Cow<'_, [f64]> {
        match self {
            Buf::F64(v) => std::borrow::Cow::Borrowed(v),
            _ => std::borrow::Cow::Owned(self.ke_f64()),
        }
    }

    /// Salinan sebagai f64. Dipakai jalur yang memang harus pecahan
    /// (pembagian, fungsi transenden, statistik).
    pub fn ke_f64(&self) -> Vec<f64> {
        match self {
            Buf::F64(v) => v.clone(),
            Buf::I64(v) => v.iter().map(|&x| x as f64).collect(),
            Buf::Bool(v) => v.iter().map(|&b| if b { 1.0 } else { 0.0 }).collect(),
        }
    }
}

/// Array n-dimensi.
#[derive(Clone, Debug, PartialEq)]
pub struct Nd {
    pub buf: Buf,
    /// Panjang tiap sumbu, dari paling luar. Kosong = skalar 0-dimensi.
    pub shape: Vec<usize>,
}

/// Hasil operasi wokwok. `String` supaya bisa dipakai apa adanya oleh
/// kedua backend — masing-masing membungkusnya jadi error runtime dengan
/// gayanya sendiri.
pub type Hasil<T> = Result<T, String>;

impl Nd {
    pub fn baru(buf: Buf, shape: Vec<usize>) -> Hasil<Nd> {
        let perlu: usize = shape.iter().product();
        if perlu != buf.len() {
            return Err(format!(
                "bentuk {:?} butuh {} elemen, tapi datanya {}",
                shape, perlu, buf.len()
            ));
        }
        Ok(Nd { buf, shape })
    }

    /// Vektor 1-D dari data yang sudah ada. Tidak bisa gagal.
    pub fn vektor(buf: Buf) -> Nd {
        let n = buf.len();
        Nd { buf, shape: vec![n] }
    }

    pub fn skalar_f64(x: f64) -> Nd {
        Nd { buf: Buf::F64(vec![x]), shape: vec![] }
    }
    pub fn skalar_i64(x: i64) -> Nd {
        Nd { buf: Buf::I64(vec![x]), shape: vec![] }
    }
    pub fn skalar_bool(x: bool) -> Nd {
        Nd { buf: Buf::Bool(vec![x]), shape: vec![] }
    }

    pub fn size(&self) -> usize { self.buf.len() }
    pub fn ndim(&self) -> usize { self.shape.len() }
    pub fn dtype(&self) -> &'static str { self.buf.nama() }

    /// Apakah ini 0-dimensi (skalar terbungkus)?
    pub fn skalar(&self) -> bool { self.shape.is_empty() }

    pub fn f64_data(&self) -> Vec<f64> { self.buf.ke_f64() }
    /// Lihat `Buf::pinjam_f64`. Jalur cepat seluruh library lewat sini.
    pub fn f64_pinjam(&self) -> std::borrow::Cow<'_, [f64]> { self.buf.pinjam_f64() }

    /// Nilai tunggal, kalau memang cuma berisi satu.
    pub fn ke_f64_tunggal(&self, konteks: &str) -> Hasil<f64> {
        if self.size() != 1 {
            return Err(format!(
                "{}: butuh satu nilai, tapi arraynya {} elemen (bentuk {:?})",
                konteks, self.size(), self.shape
            ));
        }
        Ok(self.buf.ke_f64()[0])
    }

    /// Ubah bentuk.
    pub fn reshape(&self, bentuk: &[i64]) -> Hasil<Nd> {
        let n = self.size();
        let auto: Vec<usize> = bentuk.iter().enumerate()
            .filter(|(_, &d)| d < 0).map(|(i, _)| i).collect();
        if auto.len() > 1 {
            return Err("reshape: cuma satu sumbu yang boleh `-1`".to_string());
        }
        for &d in bentuk {
            if d < -1 { return Err(format!("reshape: panjang sumbu tidak boleh {}", d)); }
        }
        let mut shape: Vec<usize> = bentuk.iter().map(|&d| if d < 0 { 1 } else { d as usize }).collect();
        if let Some(&i) = auto.first() {
            let lain: usize = shape.iter().enumerate()
                .filter(|(j, _)| *j != i).map(|(_, &d)| d).product();
            if lain == 0 || n % lain != 0 {
                return Err(format!(
                    "reshape: {} elemen tidak bisa dibagi rata ke bentuk {:?}", n, bentuk
                ));
            }
            shape[i] = n / lain;
        }
        let perlu: usize = shape.iter().product();
        if perlu != n {
            return Err(format!(
                "reshape: {} elemen tidak muat di bentuk {:?} (butuh {})", n, shape, perlu
            ));
        }
        Ok(Nd { buf: self.buf.clone(), shape })
    }

    /// Indeks rata dari koordinat per-sumbu (row-major).
    pub fn indeks_rata(&self, koord: &[i64], konteks: &str) -> Hasil<usize> {
        if koord.len() != self.shape.len() {
            return Err(format!(
                "{}: array ini {}-dimensi, tapi diberi {} indeks",
                konteks, self.shape.len(), koord.len()
            ));
        }
        let mut rata = 0usize;
        for (sumbu, (&k, &panjang)) in koord.iter().zip(self.shape.iter()).enumerate() {
            // Negatif menghitung dari belakang, dan di luar jangkauan
            // SELALU error — aturan indeks yang sama dengan array biasa
            // (lihat `norm_idx` di VM dan `eval_index` di interpreter).
            let i = if k < 0 { panjang as i64 + k } else { k };
            if i < 0 || i >= panjang as i64 {
                return Err(format!(
                    "{}: indeks {} di luar batas untuk sumbu {} (panjangnya {})",
                    konteks, k, sumbu, panjang
                ));
            }
            rata = rata * panjang + i as usize;
        }
        Ok(rata)
    }
}

/// Pisahkan penyimpanan kalau masih dipegang orang lain.
pub fn milik_sendiri(a: &mut Arc<Nd>) -> &mut Nd {
    Arc::make_mut(a)
}

/// Tampilan yang dicetak `wok`.
impl std::fmt::Display for Nd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        tulis(f, self, &self.shape, 0, &mut 0)
    }
}

const MAKS_TAMPIL: usize = 6;

fn fmt_elem(buf: &Buf, i: usize) -> String {
    match buf {
        // Pecahan yang nilainya bulat tetap ditulis dengan `.0`, sama
        // seperti `Value::Float` — tanpa itu, sebuah f64 tidak bisa
        // dibedakan dari integer di keluaran.
        Buf::F64(v) => {
            let x = v[i];
            if x.fract() == 0.0 && x.is_finite() { format!("{:.1}", x) } else { format!("{}", x) }
        }
        Buf::I64(v) => format!("{}", v[i]),
        Buf::Bool(v) => format!("{}", v[i]),
    }
}

fn tulis(
    f: &mut std::fmt::Formatter<'_>,
    a: &Nd,
    shape: &[usize],
    _depth: usize,
    kursor: &mut usize,
) -> std::fmt::Result {
    if shape.is_empty() {
        write!(f, "{}", fmt_elem(&a.buf, *kursor))?;
        *kursor += 1;
        return Ok(());
    }
    let n = shape[0];
    let sisa = &shape[1..];
    let blok: usize = sisa.iter().product::<usize>().max(1);
    write!(f, "[")?;
    let potong = n > MAKS_TAMPIL;
    for i in 0..n {
        if potong && i == MAKS_TAMPIL / 2 {
            write!(f, ", ... ({} lagi)", n - MAKS_TAMPIL)?;
            *kursor += (n - MAKS_TAMPIL) * blok;
            continue;
        }
        if potong && i > MAKS_TAMPIL / 2 && i < n - MAKS_TAMPIL / 2 { continue; }
        if i > 0 { write!(f, ", ")?; }
        tulis(f, a, sisa, _depth + 1, kursor)?;
    }
    write!(f, "]")
}
