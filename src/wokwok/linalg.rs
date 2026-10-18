// src/wokwok/linalg.rs — wokwok/linalg.rs — aljabar linear.

use super::{Buf, Hasil, Nd};

/// Di bawah ini, biaya membuat utas lebih besar daripada kerjanya.
/// Diukur, bukan ditebak: pembuatan satu utas di mesin ini ~50 mikrodetik,
/// dan 64x64x64 perkalian-tambah selesai jauh di bawah itu.
const AMBANG_UTAS: usize = 1 << 18;

/// Sisi ubin. Dipilih supaya satu ubin `b` (SISI x SISI f64 = 128 KB)
/// muat nyaman di L2 pada mesin kelas biasa.
const SISI: usize = 128;

fn matriks(a: &Nd, nama: &str, posisi: &str) -> Hasil<(usize, usize)> {
    match a.shape.len() {
        2 => Ok((a.shape[0], a.shape[1])),
        n => Err(format!(
            "wokwok::{}: {} harus matriks 2 dimensi, dapat {} dimensi (bentuk {:?})",
            nama, posisi, n, a.shape
        )),
    }
}

/// Perkalian matriks.
pub fn matmul(a: &Nd, b: &Nd) -> Hasil<Nd> {
    let (kiri_vektor, ka) = if a.ndim() == 1 { (true, Nd { buf: a.buf.clone(), shape: vec![1, a.size()] }) }
                            else { (false, a.clone()) };
    let (kanan_vektor, kb) = if b.ndim() == 1 { (true, Nd { buf: b.buf.clone(), shape: vec![b.size(), 1] }) }
                             else { (false, b.clone()) };
    let (m, k1) = matriks(&ka, "matmul", "argumen kiri")?;
    let (k2, n) = matriks(&kb, "matmul", "argumen kanan")?;
    if k1 != k2 {
        return Err(format!(
            "wokwok::matmul: bentuk {:?} dan {:?} tidak nyambung — kolom kiri ({}) \
             harus sama dengan baris kanan ({})",
            a.shape, b.shape, k1, k2
        ));
    }
    let x = ka.f64_data();
    let y = kb.f64_data();
    let mut c = vec![0.0f64; m * n];

    let kerja = m * n * k1;
    let utas = if kerja >= AMBANG_UTAS {
        std::thread::available_parallelism().map(|p| p.get()).unwrap_or(1).min(m).max(1)
    } else { 1 };

    if utas <= 1 {
        kali_blok(&x, &y, &mut c, 0, m, k1, n);
    } else {
        // Baris `c` dibagi rata. Tiap utas memegang potongan `c` yang
        // TIDAK bertumpuk, jadi tidak ada penguncian sama sekali — dan
        // tidak ada yang bisa saling menimpa.
        let per = (m + utas - 1) / utas;
        let mut potongan: Vec<(usize, &mut [f64])> = Vec::with_capacity(utas);
        let mut sisa: &mut [f64] = &mut c;
        let mut mulai = 0usize;
        while mulai < m {
            let baris = per.min(m - mulai);
            let (kiri, kanan) = sisa.split_at_mut(baris * n);
            potongan.push((mulai, kiri));
            sisa = kanan;
            mulai += baris;
        }
        std::thread::scope(|s| {
            for (offset, bagian) in potongan {
                let (x, y) = (&x, &y);
                s.spawn(move || {
                    let baris = bagian.len() / n;
                    kali_blok_ke(x, y, bagian, offset, baris, k1, n);
                });
            }
        });
    }
    Let_bentuk(m, n, kiri_vektor, kanan_vektor, c)
}

#[allow(non_snake_case)]
fn Let_bentuk(m: usize, n: usize, kiri_vektor: bool, kanan_vektor: bool, c: Vec<f64>) -> Hasil<Nd> {
    let bentuk = match (kiri_vektor, kanan_vektor) {
        (true, true) => vec![],
        (true, false) => vec![n],
        (false, true) => vec![m],
        (false, false) => vec![m, n],
    };
    Nd::baru(Buf::F64(c), bentuk)
}

fn kali_blok(x: &[f64], y: &[f64], c: &mut [f64], offset: usize, m: usize, k: usize, n: usize) {
    kali_blok_ke(x, y, c, offset, m, k, n);
}

/// Inti perkalian: `c[0..m][0..n] += x[offset..][0..m][0..k] *
/// y[0..k][0..n]`.
fn kali_blok_ke(x: &[f64], y: &[f64], c: &mut [f64], offset: usize, m: usize, k: usize, n: usize) {
    for j0 in (0..n).step_by(SISI) {
        let j1 = (j0 + SISI).min(n);
        for k0 in (0..k).step_by(SISI) {
            let k1 = (k0 + SISI).min(k);
            for i in 0..m {
                let baris_x = (offset + i) * k;
                let baris_c = i * n;
                for kk in k0..k1 {
                    let a = x[baris_x + kk];
                    if a == 0.0 { continue; }
                    let baris_y = kk * n;
                    // Loop terdalam: `c` dan `y` sama-sama berurutan, dan
                    // `a` konstan. Bentuk inilah yang bisa divektorkan.
                    let cs = &mut c[baris_c + j0..baris_c + j1];
                    let ys = &y[baris_y + j0..baris_y + j1];
                    for t in 0..cs.len() {
                        cs[t] += a * ys[t];
                    }
                }
            }
        }
    }
}

/// Hasil kali titik dua vektor.
pub fn dot(a: &Nd, b: &Nd) -> Hasil<Nd> {
    if a.ndim() != 1 || b.ndim() != 1 {
        return matmul(a, b);
    }
    if a.size() != b.size() {
        return Err(format!(
            "wokwok::dot: panjangnya beda ({} lawan {})", a.size(), b.size()
        ));
    }
    let (x, y) = (a.f64_data(), b.f64_data());
    let hasil: Vec<f64> = x.iter().zip(y.iter()).map(|(&p, &q)| p * q).collect();
    Ok(Nd::skalar_f64(super::reduce::jumlah_kompensasi(&hasil)))
}

/// Faktorisasi LU dengan pivot parsial. Mengembalikan (LU gabungan,
/// permutasi, tanda) atau `None` kalau matriksnya singular.
fn lu(a: &[f64], n: usize) -> Option<(Vec<f64>, Vec<usize>, f64)> {
    let mut m = a.to_vec();
    let mut perm: Vec<usize> = (0..n).collect();
    let mut tanda = 1.0f64;
    for k in 0..n {
        // Pivot parsial: baris dengan nilai mutlak terbesar. Tanpa ini,
        // matriks yang sah pun bisa dijawab dengan galat besar hanya
        // karena urutan barisnya kebetulan buruk.
        let mut terbaik = k;
        let mut nilai = m[k * n + k].abs();
        for i in k + 1..n {
            let v = m[i * n + k].abs();
            if v > nilai { nilai = v; terbaik = i; }
        }
        if nilai == 0.0 || !nilai.is_finite() { return None; }
        if terbaik != k {
            for j in 0..n { m.swap(k * n + j, terbaik * n + j); }
            perm.swap(k, terbaik);
            tanda = -tanda;
        }
        let pivot = m[k * n + k];
        for i in k + 1..n {
            let f = m[i * n + k] / pivot;
            m[i * n + k] = f;
            if f != 0.0 {
                for j in k + 1..n { m[i * n + j] -= f * m[k * n + j]; }
            }
        }
    }
    Some((m, perm, tanda))
}

pub fn det(a: &Nd) -> Hasil<Nd> {
    let (r, c) = matriks(a, "det", "argumennya")?;
    if r != c {
        return Err(format!("wokwok::det: butuh matriks persegi, dapat {}x{}", r, c));
    }
    let v = a.f64_data();
    match lu(&v, r) {
        // Singular berarti determinannya memang nol — itu jawaban yang
        // benar, bukan kegagalan.
        None => Ok(Nd::skalar_f64(0.0)),
        Some((m, _, tanda)) => {
            let mut d = tanda;
            for i in 0..r { d *= m[i * r + i]; }
            Ok(Nd::skalar_f64(d))
        }
    }
}

pub fn inverse(a: &Nd) -> Hasil<Nd> {
    let (r, c) = matriks(a, "inv", "argumennya")?;
    if r != c {
        return Err(format!("wokwok::inv: butuh matriks persegi, dapat {}x{}", r, c));
    }
    let mut ident = vec![0.0f64; r * r];
    for i in 0..r { ident[i * r + i] = 1.0; }
    let x = selesaikan_banyak(&a.f64_data(), &ident, r, r, "inv")?;
    Nd::baru(Buf::F64(x), vec![r, r])
}

pub fn solve(a: &Nd, b: &Nd) -> Hasil<Nd> {
    let (r, c) = matriks(a, "solve", "matriksnya")?;
    if r != c {
        return Err(format!("wokwok::solve: butuh matriks persegi, dapat {}x{}", r, c));
    }
    let (kolom, bentuk_hasil) = match b.ndim() {
        1 => {
            if b.size() != r {
                return Err(format!(
                    "wokwok::solve: sisi kanan panjangnya {}, harusnya {}", b.size(), r));
            }
            (1usize, vec![r])
        }
        2 => {
            if b.shape[0] != r {
                return Err(format!(
                    "wokwok::solve: sisi kanan punya {} baris, harusnya {}", b.shape[0], r));
            }
            (b.shape[1], vec![r, b.shape[1]])
        }
        n => return Err(format!("wokwok::solve: sisi kanan harus 1 atau 2 dimensi, dapat {}", n)),
    };
    let x = selesaikan_banyak(&a.f64_data(), &b.f64_data(), r, kolom, "solve")?;
    Nd::baru(Buf::F64(x), bentuk_hasil)
}

/// Selesaikan `A X = B` untuk banyak kolom sekaligus, lewat satu
/// faktorisasi LU.
fn selesaikan_banyak(a: &[f64], b: &[f64], n: usize, kolom: usize, nama: &str) -> Hasil<Vec<f64>> {
    let (m, perm, _) = lu(a, n).ok_or_else(|| format!(
        "wokwok::{}: matriksnya singular — tidak punya balikan, jadi tidak ada \
         jawaban tunggal yang bisa diberikan", nama
    ))?;
    let mut x = vec![0.0f64; n * kolom];
    for k in 0..kolom {
        // Substitusi maju (Ly = Pb)
        let mut y = vec![0.0f64; n];
        for i in 0..n {
            let mut s = b[perm[i] * kolom + k];
            for j in 0..i { s -= m[i * n + j] * y[j]; }
            y[i] = s;
        }
        // Substitusi mundur (Ux = y)
        for i in (0..n).rev() {
            let mut s = y[i];
            for j in i + 1..n { s -= m[i * n + j] * x[j * kolom + k]; }
            x[i * kolom + k] = s / m[i * n + i];
        }
    }
    Ok(x)
}

pub fn trace(a: &Nd) -> Hasil<Nd> {
    let (r, c) = matriks(a, "trace", "argumennya")?;
    let v = a.f64_data();
    let n = r.min(c);
    let d: Vec<f64> = (0..n).map(|i| v[i * c + i]).collect();
    Ok(Nd::skalar_f64(super::reduce::jumlah_kompensasi(&d)))
}

/// Norma Euclid (Frobenius untuk matriks).
pub fn norm(a: &Nd) -> Hasil<Nd> {
    let v = a.f64_data();
    let kuadrat: Vec<f64> = v.iter().map(|&x| x * x).collect();
    Ok(Nd::skalar_f64(super::reduce::jumlah_kompensasi(&kuadrat).sqrt()))
}

/// Hasil kali luar dua vektor.
pub fn outer(a: &Nd, b: &Nd) -> Hasil<Nd> {
    let (x, y) = (a.f64_data(), b.f64_data());
    let (m, n) = (x.len(), y.len());
    super::daftar::periksa_ukuran(m.saturating_mul(n), "wokwok::outer")?;
    let mut out = vec![0.0f64; m * n];
    for i in 0..m {
        for j in 0..n { out[i * n + j] = x[i] * y[j]; }
    }
    Nd::baru(Buf::F64(out), vec![m, n])
}
