// src/wokwok/reduce.rs — wokwok/reduce.rs — reduksi (jumlah, rata-rata, min/maks, dst).

use super::{Buf, Hasil, Nd};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Reduksi { Jumlah, Hasilkali, Rata, Min, Maks, Ragam, Simpangan,
                   ArgMin, ArgMaks, Ada, Semua, Cacah }

impl Reduksi {
    fn nama(self) -> &'static str {
        match self {
            Reduksi::Jumlah => "sum", Reduksi::Hasilkali => "prod",
            Reduksi::Rata => "mean", Reduksi::Min => "min", Reduksi::Maks => "max",
            Reduksi::Ragam => "var", Reduksi::Simpangan => "std",
            Reduksi::ArgMin => "argmin", Reduksi::ArgMaks => "argmax",
            Reduksi::Ada => "any", Reduksi::Semua => "all", Reduksi::Cacah => "count",
        }
    }
    /// Reduksi yang menjawab POSISI, bukan nilai.
    fn indeks(self) -> bool { matches!(self, Reduksi::ArgMin | Reduksi::ArgMaks) }
    fn boolean(self) -> bool { matches!(self, Reduksi::Ada | Reduksi::Semua) }
}

/// Penjumlahan BERPASANGAN.
pub fn jumlah_pairwise(v: &[f64]) -> f64 {
    // Di bawah ambang ini, rekursinya lebih mahal daripada kerjanya.
    const BLOK: usize = 128;
    if v.len() > BLOK {
        let tengah = v.len() / 2;
        return jumlah_pairwise(&v[..tengah]) + jumlah_pairwise(&v[tengah..]);
    }
    // Delapan akumulator terpisah: masing-masing rantainya sendiri, jadi
    // prosesor bisa mengerjakannya bersamaan dan compiler bisa memvektorkannya.
    let mut acc = [0.0f64; 8];
    let potongan = v.chunks_exact(8);
    let sisa = potongan.remainder();
    for c in potongan {
        for j in 0..8 { acc[j] += c[j]; }
    }
    let mut jum = ((acc[0] + acc[1]) + (acc[2] + acc[3]))
                + ((acc[4] + acc[5]) + (acc[6] + acc[7]));
    for &x in sisa { jum += x; }
    jum
}

/// Nama lama, dipertahankan supaya pemanggil di modul lain tidak perlu
/// tahu algoritmanya berubah.
pub fn jumlah_kompensasi(v: &[f64]) -> f64 { jumlah_pairwise(v) }

/// Penjumlahan TERKOMPENSASI (Kahan-Babuska-Neumaier).
pub fn jumlah_teliti(v: &[f64]) -> f64 {
    let mut jum = 0.0f64;
    let mut koreksi = 0.0f64;
    for &x in v {
        let t = jum + x;
        koreksi += if jum.abs() >= x.abs() { (jum - t) + x } else { (x - t) + jum };
        jum = t;
    }
    jum + koreksi
}

fn kosong(nama: &str) -> String {
    format!("wokwok::{}: array kosong tidak punya jawaban — cek dulu ukurannya", nama)
}

/// Reduksi atas SELURUH array.
pub fn semua(a: &Nd, r: Reduksi) -> Hasil<Nd> {
    if r == Reduksi::Cacah {
        return Ok(Nd::skalar_i64(a.size() as i64));
    }
    if r.boolean() {
        let v = a.f64_pinjam();
        let jawab = if r == Reduksi::Ada { v.iter().any(|&x| x != 0.0) }
                    else { v.iter().all(|&x| x != 0.0) };
        return Ok(Nd::skalar_bool(jawab));
    }
    if a.size() == 0 {
        // `sum` dan `prod` punya identitas; sisanya tidak, dan mengarang
        // jawaban untuk himpunan kosong adalah cara paling halus untuk
        // berbohong.
        return match r {
            Reduksi::Jumlah => Ok(Nd::skalar_f64(0.0)),
            Reduksi::Hasilkali => Ok(Nd::skalar_f64(1.0)),
            _ => Err(kosong(r.nama())),
        };
    }
    if r.indeks() {
        let v = a.f64_pinjam();
        let i = posisi_ekstrem(&v, r == Reduksi::ArgMin)
            .ok_or_else(|| format!("wokwok::{}: semua elemennya NaN", r.nama()))?;
        return Ok(Nd::skalar_i64(i as i64));
    }
    // Integer tetap integer untuk jumlah/hasilkali/min/maks — supaya
    // menjumlahkan hitungan tidak diam-diam berubah jadi pecahan.
    if let Buf::I64(v) = &a.buf {
        if let Some(x) = reduksi_i64(v, r)? { return Ok(Nd::skalar_i64(x)); }
    }
    let v = a.f64_pinjam();
    Ok(Nd::skalar_f64(reduksi_f64(&v, r)?))
}

fn reduksi_i64(v: &[i64], r: Reduksi) -> Hasil<Option<i64>> {
    Ok(Some(match r {
        Reduksi::Jumlah => {
            let mut s: i64 = 0;
            for &x in v {
                s = s.checked_add(x).ok_or_else(|| format!(
                    "wokwok::sum: jumlahnya kelewat besar buat integer 64 bit"))?;
            }
            s
        }
        Reduksi::Hasilkali => {
            let mut p: i64 = 1;
            for &x in v {
                p = p.checked_mul(x).ok_or_else(|| format!(
                    "wokwok::prod: hasilkalinya kelewat besar buat integer 64 bit"))?;
            }
            p
        }
        Reduksi::Min => *v.iter().min().unwrap(),
        Reduksi::Maks => *v.iter().max().unwrap(),
        _ => return Ok(None),
    }))
}

fn reduksi_f64(v: &[f64], r: Reduksi) -> Hasil<f64> {
    Ok(match r {
        Reduksi::Jumlah => jumlah_kompensasi(v),
        Reduksi::Hasilkali => v.iter().product(),
        Reduksi::Rata => jumlah_kompensasi(v) / v.len() as f64,
        // NaN MENULAR.
        Reduksi::Min => {
            let m = v.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            if v.iter().any(|x| x.is_nan()) { f64::NAN } else { m }
        }
        Reduksi::Maks => {
            let m = v.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            if v.iter().any(|x| x.is_nan()) { f64::NAN } else { m }
        }
        Reduksi::Ragam | Reduksi::Simpangan => {
            let n = v.len() as f64;
            let rata = jumlah_kompensasi(v) / n;
            // Dua lintasan, bukan rumus E[x^2]-E[x]^2.
            let ragam = jumlah_kompensasi(
                &v.iter().map(|&x| (x - rata) * (x - rata)).collect::<Vec<_>>()
            ) / n;
            if r == Reduksi::Ragam { ragam } else { ragam.sqrt() }
        }
        _ => unreachable!("reduksi ini sudah ditangani di atas"),
    })
}

fn posisi_ekstrem(v: &[f64], cari_min: bool) -> Option<usize> {
    let mut terbaik: Option<(usize, f64)> = None;
    for (i, &x) in v.iter().enumerate() {
        if x.is_nan() { continue; }
        match terbaik {
            None => terbaik = Some((i, x)),
            Some((_, b)) if (cari_min && x < b) || (!cari_min && x > b) => terbaik = Some((i, x)),
            _ => {}
        }
    }
    terbaik.map(|(i, _)| i)
}

/// Reduksi sepanjang satu SUMBU. Sumbu itu hilang dari bentuk hasilnya.
pub fn per_sumbu(a: &Nd, r: Reduksi, sumbu: i64) -> Hasil<Nd> {
    let nd = a.ndim();
    if nd == 0 {
        return Err(format!("wokwok::{}: skalar tidak punya sumbu", r.nama()));
    }
    let s = if sumbu < 0 { nd as i64 + sumbu } else { sumbu };
    if s < 0 || s >= nd as i64 {
        return Err(format!(
            "wokwok::{}: sumbu {} di luar jangkauan — arraynya {}-dimensi",
            r.nama(), sumbu, nd
        ));
    }
    let s = s as usize;
    let panjang = a.shape[s];
    if panjang == 0 && !matches!(r, Reduksi::Jumlah | Reduksi::Hasilkali | Reduksi::Cacah) {
        return Err(kosong(r.nama()));
    }
    let luar: usize = a.shape[..s].iter().product();
    let dalam: usize = a.shape[s + 1..].iter().product::<usize>().max(1);
    let bentuk_hasil: Vec<usize> = a.shape.iter().enumerate()
        .filter(|(i, _)| *i != s).map(|(_, &d)| d).collect();

    let data = a.f64_data();
    let mut potong = vec![0.0f64; panjang];
    let total_hasil = luar * dalam;

    if r == Reduksi::Cacah {
        return Nd::baru(Buf::I64(vec![panjang as i64; total_hasil]), bentuk_hasil);
    }
    if r.indeks() {
        let mut out = Vec::with_capacity(total_hasil);
        for l in 0..luar {
            for d in 0..dalam {
                for k in 0..panjang { potong[k] = data[(l * panjang + k) * dalam + d]; }
                let i = posisi_ekstrem(&potong, r == Reduksi::ArgMin)
                    .ok_or_else(|| format!("wokwok::{}: ada potongan yang semuanya NaN", r.nama()))?;
                out.push(i as i64);
            }
        }
        return Nd::baru(Buf::I64(out), bentuk_hasil);
    }
    if r.boolean() {
        let mut out = Vec::with_capacity(total_hasil);
        for l in 0..luar {
            for d in 0..dalam {
                for k in 0..panjang { potong[k] = data[(l * panjang + k) * dalam + d]; }
                out.push(if r == Reduksi::Ada { potong.iter().any(|&x| x != 0.0) }
                         else { potong.iter().all(|&x| x != 0.0) });
            }
        }
        return Nd::baru(Buf::Bool(out), bentuk_hasil);
    }
    // Integer dipertahankan, sama alasannya dengan jalur `semua`.
    if let Buf::I64(v) = &a.buf {
        if matches!(r, Reduksi::Jumlah | Reduksi::Hasilkali | Reduksi::Min | Reduksi::Maks) {
            let mut out = Vec::with_capacity(total_hasil);
            let mut pi = vec![0i64; panjang];
            for l in 0..luar {
                for d in 0..dalam {
                    for k in 0..panjang { pi[k] = v[(l * panjang + k) * dalam + d]; }
                    out.push(reduksi_i64(&pi, r)?.expect("sudah disaring di atas"));
                }
            }
            return Nd::baru(Buf::I64(out), bentuk_hasil);
        }
    }
    let mut out = Vec::with_capacity(total_hasil);
    for l in 0..luar {
        for d in 0..dalam {
            for k in 0..panjang { potong[k] = data[(l * panjang + k) * dalam + d]; }
            out.push(reduksi_f64(&potong, r)?);
        }
    }
    Nd::baru(Buf::F64(out), bentuk_hasil)
}

/// Jumlah/hasilkali kumulatif sepanjang array yang sudah diratakan.
pub fn kumulatif(a: &Nd, kali: bool) -> Hasil<Nd> {
    let v = a.f64_data();
    let mut out = Vec::with_capacity(v.len());
    let mut akum = if kali { 1.0 } else { 0.0 };
    for &x in &v {
        akum = if kali { akum * x } else { akum + x };
        out.push(akum);
    }
    Nd::baru(Buf::F64(out), a.shape.clone())
}

/// Urutkan (menaik) seluruh isi, mengabaikan bentuk aslinya selain
/// mempertahankannya di hasil.
pub fn urutkan(a: &Nd) -> Hasil<Nd> {
    match &a.buf {
        Buf::I64(v) => {
            let mut w = v.clone();
            w.sort_unstable();
            Nd::baru(Buf::I64(w), a.shape.clone())
        }
        _ => {
            let mut w = a.f64_data();
            // NaN dikumpulkan di belakang. `partial_cmp().unwrap()` akan
            // panik, dan urutan yang "menganggap NaN sama" akan diam-diam
            // menghasilkan urutan yang bukan urutan.
            w.sort_by(|x, y| x.partial_cmp(y).unwrap_or_else(|| {
                if x.is_nan() && y.is_nan() { std::cmp::Ordering::Equal }
                else if x.is_nan() { std::cmp::Ordering::Greater }
                else { std::cmp::Ordering::Less }
            }));
            Nd::baru(Buf::F64(w), a.shape.clone())
        }
    }
}

/// Posisi-posisi yang akan mengurutkan array ini.
pub fn urutan_indeks(a: &Nd) -> Hasil<Nd> {
    let v = a.f64_data();
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&i, &j| v[i].partial_cmp(&v[j]).unwrap_or_else(|| {
        if v[i].is_nan() && v[j].is_nan() { std::cmp::Ordering::Equal }
        else if v[i].is_nan() { std::cmp::Ordering::Greater }
        else { std::cmp::Ordering::Less }
    }));
    let n = idx.len();
    Nd::baru(Buf::I64(idx.into_iter().map(|i| i as i64).collect()), vec![n])
}
