// src/dok/numeric.rs — dok::numeric — "numerik tajam": vektor, matriks, statistik lanjutan,
// teori bilangan, dan pencarian akar numerik.

use std::collections::HashMap;

use crate::interpreter::{err_value, ok_value, Interpreter, RuntimeError, Signal, Value};

// ─── Konversi dasar ─────────────────────────────────────────────────────

fn as_f64(v: &Value) -> Result<f64, String> {
    match v {
        Value::Int(n) => Ok(*n as f64),
        Value::Float(f) => Ok(*f),
        other => Err(format!("butuh angka, dapat `{}`", other.type_name())),
    }
}

fn as_i128(v: &Value) -> Result<i128, String> {
    match v {
        Value::Int(n) => Ok(*n),
        // `as i128` menjenuhkan: pecahan di luar 128 bit dulu diam-diam jadi batasnya.
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 1.7e38 => Ok(*f as i128),
        Value::Float(f) if f.fract() == 0.0 => Err(format!("pecahan `{:e}` di luar jangkauan 128 bit", f)),
        other => Err(format!("butuh int, dapat `{}`", other.type_name())),
    }
}

/// `ke` adalah posisi argumennya (1-berbasis), supaya kalimatnya satu dengan
/// seluruh keluarga `arr::` lewat `pesan_arg_harus_array`.
fn as_vec(v: &Value, fname: &str, ke: usize) -> Result<Vec<f64>, String> {
    match v {
        Value::Array(_) => isi_vec(v, fname),
        other => Err(crate::interpreter::pesan_arg_harus_array(fname, ke, &other.type_name())),
    }
}

/// Isi sebuah array angka. Dipisah dari `as_vec` karena sebuah BARIS matriks
/// bukan argumen — menyebutnya "argumen ke-N" akan menunjuk hal yang salah.
fn isi_vec(v: &Value, fname: &str) -> Result<Vec<f64>, String> {
    match v {
        Value::Array(a) => a.iter().map(as_f64).collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("{}: {}", fname, e)),
        other => Err(format!("{}: tiap baris matriks harus array, dapat `{}`",
                             fname, other.type_name())),
    }
}

fn vec_to_value(v: Vec<f64>) -> Value { Value::Array(v.into_iter().map(Value::Float).collect()) }

fn as_matrix(v: &Value, fname: &str) -> Result<Vec<Vec<f64>>, String> {
    match v {
        Value::Array(rows) => rows.iter().map(|r| isi_vec(r, fname)).collect(),
        other => Err(format!("{}: butuh matriks (array berisi array), dapat `{}`", fname, other.type_name())),
    }
}

fn matrix_to_value(m: Vec<Vec<f64>>) -> Value { Value::Array(m.into_iter().map(vec_to_value).collect()) }

fn arg(args: &[Value], i: usize) -> &Value { args.get(i).unwrap_or(&Value::Nil) }

// ─── Vektor ─────────────────────────────────────────────────────────────

pub fn vec_add(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_add", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_add", 2)?;
    if a.len() != b.len() { return Err(format!("dok::vec_add: dimensi beda ({} vs {})", a.len(), b.len())); }
    Ok(vec_to_value(a.iter().zip(&b).map(|(x, y)| x + y).collect()))
}

pub fn vec_sub(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_sub", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_sub", 2)?;
    if a.len() != b.len() { return Err(format!("dok::vec_sub: dimensi beda ({} vs {})", a.len(), b.len())); }
    Ok(vec_to_value(a.iter().zip(&b).map(|(x, y)| x - y).collect()))
}

pub fn vec_scale(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_scale", 1)?;
    let k = as_f64(arg(args, 1)).map_err(|e| format!("dok::vec_scale: {}", e))?;
    Ok(vec_to_value(a.iter().map(|x| x * k).collect()))
}

pub fn vec_dot(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_dot", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_dot", 2)?;
    if a.len() != b.len() { return Err(format!("dok::vec_dot: dimensi beda ({} vs {})", a.len(), b.len())); }
    Ok(Value::Float(a.iter().zip(&b).map(|(x, y)| x * y).sum()))
}

pub fn vec_cross(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_cross", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_cross", 2)?;
    if a.len() != 3 || b.len() != 3 { return Err("dok::vec_cross: hanya untuk vektor 3D".to_string()); }
    Ok(vec_to_value(vec![
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]))
}

pub fn vec_norm(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_norm", 1)?;
    Ok(Value::Float(a.iter().map(|x| x * x).sum::<f64>().sqrt()))
}

pub fn vec_normalize(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_normalize", 1)?;
    let n = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    Ok(vec_to_value(a.iter().map(|x| x / n).collect()))
}

pub fn vec_lerp(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_lerp", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_lerp", 2)?;
    let t = as_f64(arg(args, 2)).map_err(|e| format!("dok::vec_lerp: {}", e))?;
    if a.len() != b.len() { return Err(format!("dok::vec_lerp: dimensi beda ({} vs {})", a.len(), b.len())); }
    Ok(vec_to_value(a.iter().zip(&b).map(|(x, y)| x + (y - x) * t).collect()))
}

pub fn vec_distance(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_distance", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_distance", 2)?;
    if a.len() != b.len() { return Err(format!("dok::vec_distance: dimensi beda ({} vs {})", a.len(), b.len())); }
    Ok(Value::Float(a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>().sqrt()))
}

pub fn vec_angle(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::vec_angle", 1)?;
    let b = as_vec(arg(args, 1), "dok::vec_angle", 2)?;
    if a.len() != b.len() { return Err(format!("dok::vec_angle: dimensi beda ({} vs {})", a.len(), b.len())); }
    let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    Ok(Value::Float((dot / (na * nb)).clamp(-1.0, 1.0).acos()))
}

// ─── Matriks ────────────────────────────────────────────────────────────

pub fn mat_new(args: &[Value]) -> Result<Value, String> {
    let rows = as_i128(arg(args, 0)).map_err(|e| format!("dok::mat_new: {}", e))?.max(0) as usize;
    let cols = as_i128(arg(args, 1)).map_err(|e| format!("dok::mat_new: {}", e))?.max(0) as usize;
    let fill = match args.get(2) { Some(v) => as_f64(v).map_err(|e| format!("dok::mat_new: {}", e))?, None => 0.0 };
    Ok(matrix_to_value(vec![vec![fill; cols]; rows]))
}

pub fn mat_identity(args: &[Value]) -> Result<Value, String> {
    let n = as_i128(arg(args, 0)).map_err(|e| format!("dok::mat_identity: {}", e))?.max(0) as usize;
    let mut m = vec![vec![0.0; n]; n];
    for (i, row) in m.iter_mut().enumerate() { row[i] = 1.0; }
    Ok(matrix_to_value(m))
}

pub fn mat_add(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_add")?;
    let b = as_matrix(arg(args, 1), "dok::mat_add")?;
    if a.len() != b.len() || a.iter().zip(&b).any(|(r1, r2)| r1.len() != r2.len()) {
        return Err("dok::mat_add: dimensi matriks tidak sama".to_string());
    }
    Ok(matrix_to_value(a.iter().zip(&b).map(|(r1, r2)| r1.iter().zip(r2).map(|(x, y)| x + y).collect()).collect()))
}

pub fn mat_sub(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_sub")?;
    let b = as_matrix(arg(args, 1), "dok::mat_sub")?;
    if a.len() != b.len() || a.iter().zip(&b).any(|(r1, r2)| r1.len() != r2.len()) {
        return Err("dok::mat_sub: dimensi matriks tidak sama".to_string());
    }
    Ok(matrix_to_value(a.iter().zip(&b).map(|(r1, r2)| r1.iter().zip(r2).map(|(x, y)| x - y).collect()).collect()))
}

pub fn mat_mul(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_mul")?;
    let b = as_matrix(arg(args, 1), "dok::mat_mul")?;
    let (p, q) = (a.len(), a.first().map(|r| r.len()).unwrap_or(0));
    let (q2, r) = (b.len(), b.first().map(|r| r.len()).unwrap_or(0));
    if q != q2 { return Err(format!("dok::mat_mul: dimensi tidak cocok ({}x{} × {}x{})", p, q, q2, r)); }
    let mut out = vec![vec![0.0; r]; p];
    for i in 0..p {
        for k in 0..q {
            let aik = a[i][k];
            if aik == 0.0 { continue; }
            for j in 0..r { out[i][j] += aik * b[k][j]; }
        }
    }
    Ok(matrix_to_value(out))
}

pub fn mat_transpose(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_transpose")?;
    let rows = a.len();
    let cols = a.first().map(|r| r.len()).unwrap_or(0);
    let mut out = vec![vec![0.0; rows]; cols];
    for (i, row) in a.iter().enumerate() {
        for (j, v) in row.iter().enumerate() { out[j][i] = *v; }
    }
    Ok(matrix_to_value(out))
}

pub fn mat_get(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_get")?;
    let i = as_i128(arg(args, 1)).map_err(|e| format!("dok::mat_get: {}", e))? as usize;
    let j = as_i128(arg(args, 2)).map_err(|e| format!("dok::mat_get: {}", e))? as usize;
    a.get(i).and_then(|r| r.get(j)).map(|v| Value::Float(*v))
        .ok_or_else(|| format!("dok::mat_get: index ({}, {}) di luar jangkauan", i, j))
}

pub fn mat_set(args: &[Value]) -> Result<Value, String> {
    let mut a = as_matrix(arg(args, 0), "dok::mat_set")?;
    let i = as_i128(arg(args, 1)).map_err(|e| format!("dok::mat_set: {}", e))? as usize;
    let j = as_i128(arg(args, 2)).map_err(|e| format!("dok::mat_set: {}", e))? as usize;
    let v = as_f64(arg(args, 3)).map_err(|e| format!("dok::mat_set: {}", e))?;
    match a.get_mut(i).and_then(|r| r.get_mut(j)) {
        Some(cell) => { *cell = v; Ok(matrix_to_value(a)) }
        None => Err(format!("dok::mat_set: index ({}, {}) di luar jangkauan", i, j)),
    }
}

/// Eliminasi Gauss dengan partial pivoting — dipakai bersama oleh
/// `mat_det` dan `mat_inverse` supaya logikanya tidak dobel.
fn gauss_eliminate(mut m: Vec<Vec<f64>>) -> (Vec<Vec<f64>>, f64, bool) {
    let n = m.len();
    let mut sign = 1.0;
    let mut singular = false;
    for col in 0..n {
        let mut pivot_row = col;
        let mut best = m[col].get(col).copied().unwrap_or(0.0).abs();
        for r in (col + 1)..n {
            let v = m[r].get(col).copied().unwrap_or(0.0).abs();
            if v > best { best = v; pivot_row = r; }
        }
        if best < 1e-12 { singular = true; continue; }
        if pivot_row != col { m.swap(pivot_row, col); sign = -sign; }
        for r in (col + 1)..n {
            let factor = m[r][col] / m[col][col];
            if factor == 0.0 { continue; }
            for c in col..n { m[r][c] -= factor * m[col][c]; }
        }
    }
    (m, sign, singular)
}

pub fn mat_det(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_det")?;
    let n = a.len();
    if a.iter().any(|r| r.len() != n) { return Err("dok::mat_det: matriks harus persegi".to_string()); }
    if n == 0 { return Ok(Value::Float(1.0)); }
    let (upper, sign, singular) = gauss_eliminate(a);
    if singular { return Ok(Value::Float(0.0)); }
    let mut det = sign;
    for i in 0..n { det *= upper[i][i]; }
    Ok(Value::Float(det))
}

pub fn mat_inverse(args: &[Value]) -> Result<Value, String> {
    let a = as_matrix(arg(args, 0), "dok::mat_inverse")?;
    let n = a.len();
    if a.iter().any(|r| r.len() != n) { return Err("dok::mat_inverse: matriks harus persegi".to_string()); }

    // Augmented [A | I], lalu Gauss-Jordan penuh (bukan cuma segitiga atas)
    // supaya hasil akhirnya langsung A^-1, bukan perlu back-substitution manual.
    let mut aug: Vec<Vec<f64>> = a.iter().enumerate().map(|(i, row)| {
        let mut r = row.clone();
        for j in 0..n { r.push(if i == j { 1.0 } else { 0.0 }); }
        r
    }).collect();

    for col in 0..n {
        let mut pivot_row = col;
        let mut best = aug[col][col].abs();
        for r in (col + 1)..n {
            if aug[r][col].abs() > best { best = aug[r][col].abs(); pivot_row = r; }
        }
        if best < 1e-12 { return Err("dok::mat_inverse: matriks singular, tidak punya invers".to_string()); }
        aug.swap(pivot_row, col);
        let pivot = aug[col][col];
        for c in 0..(2 * n) { aug[col][c] /= pivot; }
        for r in 0..n {
            if r == col { continue; }
            let factor = aug[r][col];
            if factor == 0.0 { continue; }
            for c in 0..(2 * n) { aug[r][c] -= factor * aug[col][c]; }
        }
    }

    Ok(matrix_to_value(aug.into_iter().map(|r| r[n..].to_vec()).collect()))
}

// ─── Statistik lanjutan ─────────────────────────────────────────────────

fn mean(a: &[f64]) -> f64 { if a.is_empty() { 0.0 } else { a.iter().sum::<f64>() / a.len() as f64 } }

pub fn stat_correlation(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::stat_correlation", 1)?;
    let b = as_vec(arg(args, 1), "dok::stat_correlation", 2)?;
    if a.len() != b.len() || a.len() < 2 { return Err("dok::stat_correlation: butuh dua array sama panjang (≥2)".to_string()); }
    let (ma, mb) = (mean(&a), mean(&b));
    let cov: f64 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let sa: f64 = a.iter().map(|x| (x - ma).powi(2)).sum::<f64>().sqrt();
    let sb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum::<f64>().sqrt();
    Ok(Value::Float(cov / (sa * sb)))
}

pub fn stat_covariance(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::stat_covariance", 1)?;
    let b = as_vec(arg(args, 1), "dok::stat_covariance", 2)?;
    if a.len() != b.len() || a.is_empty() { return Err("dok::stat_covariance: butuh dua array sama panjang".to_string()); }
    let (ma, mb) = (mean(&a), mean(&b));
    let n = if a.len() > 1 { (a.len() - 1) as f64 } else { 1.0 }; // sample covariance
    Ok(Value::Float(a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum::<f64>() / n))
}

pub fn stat_percentile(args: &[Value]) -> Result<Value, String> {
    let mut a = as_vec(arg(args, 0), "dok::stat_percentile", 1)?;
    let p = as_f64(arg(args, 1)).map_err(|e| format!("dok::stat_percentile: {}", e))?;
    if a.is_empty() { return Err("dok::stat_percentile: array kosong".to_string()); }
    if !(0.0..=100.0).contains(&p) { return Err("dok::stat_percentile: p harus 0..100".to_string()); }
    // NaN tidak berurutan terhadap apa pun, jadi persentil dari data yang
    // mengandungnya tidak berarti apa-apa.
    if a.iter().any(|x| x.is_nan()) {
        return Err(crate::interpreter::pesan_nan_tak_terurut("dok::stat_percentile"));
    }
    a.sort_by(|x, y| x.total_cmp(y));
    let rank = (p / 100.0) * (a.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi { return Ok(Value::Float(a[lo])); }
    let frac = rank - lo as f64;
    Ok(Value::Float(a[lo] + (a[hi] - a[lo]) * frac))
}

pub fn stat_mode(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::stat_mode", 1)?;
    if a.is_empty() { return Err("dok::stat_mode: array kosong".to_string()); }
    let mut counts: Vec<(String, f64, usize)> = Vec::new();
    for &x in &a {
        let key = format!("{}", x);
        match counts.iter_mut().find(|(k, _, _)| *k == key) {
            Some(entry) => entry.2 += 1,
            None => counts.push((key, x, 1)),
        }
    }
    let best = counts.iter().max_by_key(|(_, _, c)| *c).unwrap();
    Ok(Value::Float(best.1))
}

pub fn stat_linreg(args: &[Value]) -> Result<Value, String> {
    let xs = as_vec(arg(args, 0), "dok::stat_linreg", 1)?;
    let ys = as_vec(arg(args, 1), "dok::stat_linreg", 2)?;
    if xs.len() != ys.len() || xs.len() < 2 { return Err("dok::stat_linreg: butuh dua array sama panjang (≥2)".to_string()); }
    let (mx, my) = (mean(&xs), mean(&ys));
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let ss_tot: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    let ss_res: f64 = xs.iter().zip(&ys).map(|(x, y)| { let pred = slope * x + intercept; (y - pred).powi(2) }).sum();
    let r2 = if ss_tot.abs() < 1e-15 { 1.0 } else { 1.0 - ss_res / ss_tot };
    let mut fields = HashMap::new();
    fields.insert("slope".to_string(), Value::Float(slope));
    fields.insert("intercept".to_string(), Value::Float(intercept));
    fields.insert("r2".to_string(), Value::Float(r2));
    Ok(Value::Struct { name: "DokLinReg".to_string(), fields: fields.into() })
}

pub fn stat_zscore(args: &[Value]) -> Result<Value, String> {
    let a = as_vec(arg(args, 0), "dok::stat_zscore", 1)?;
    if a.is_empty() { return Err("dok::stat_zscore: array kosong".to_string()); }
    let m = mean(&a);
    let variance = a.iter().map(|x| (x - m).powi(2)).sum::<f64>() / a.len() as f64;
    let sd = variance.sqrt();
    Ok(vec_to_value(a.iter().map(|x| if sd == 0.0 { 0.0 } else { (x - m) / sd }).collect()))
}

pub fn stat_normal_pdf(args: &[Value]) -> Result<Value, String> {
    let x = as_f64(arg(args, 0)).map_err(|e| format!("dok::stat_normal_pdf: {}", e))?;
    let mu = match args.get(1) { Some(v) => as_f64(v).map_err(|e| format!("dok::stat_normal_pdf: {}", e))?, None => 0.0 };
    let sigma = match args.get(2) { Some(v) => as_f64(v).map_err(|e| format!("dok::stat_normal_pdf: {}", e))?, None => 1.0 };
    let z = (x - mu) / sigma;
    Ok(Value::Float((-0.5 * z * z).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())))
}

/// Aproksimasi error function (Abramowitz & Stegun 7.1.26, akurasi ~1.5e-7)
/// — dipakai untuk CDF normal karena Rust `std` tidak punya `erf` bawaan.
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let a1 = 0.254829592; let a2 = -0.284496736; let a3 = 1.421413741;
    let a4 = -1.453152027; let a5 = 1.061405429; let p = 0.3275911;
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    sign * y
}

pub fn stat_normal_cdf(args: &[Value]) -> Result<Value, String> {
    let x = as_f64(arg(args, 0)).map_err(|e| format!("dok::stat_normal_cdf: {}", e))?;
    let mu = match args.get(1) { Some(v) => as_f64(v).map_err(|e| format!("dok::stat_normal_cdf: {}", e))?, None => 0.0 };
    let sigma = match args.get(2) { Some(v) => as_f64(v).map_err(|e| format!("dok::stat_normal_cdf: {}", e))?, None => 1.0 };
    let z = (x - mu) / (sigma * std::f64::consts::SQRT_2);
    Ok(Value::Float(0.5 * (1.0 + erf(z))))
}

// ─── Teori bilangan ─────────────────────────────────────────────────────

/// `(a * b) mod m` tanpa meluap, untuk `m` sampai 2^127: gandakan-dan-jumlahkan, karena
/// hasil kali dua bilangan 127 bit tidak muat di 128 bit.
fn kali_mod(mut a: u128, mut b: u128, m: u128) -> u128 {
    if let (Ok(x), Ok(y)) = (u64::try_from(a), u64::try_from(b)) { return (x as u128 * y as u128) % m; }
    let mut hasil = 0u128;
    a %= m;
    while b > 0 {
        if b & 1 == 1 { hasil = (hasil + a) % m; }
        a = (a + a) % m;
        b >>= 1;
    }
    hasil
}

fn pangkat_mod(mut basis: u128, mut eks: u128, m: u128) -> u128 {
    let mut hasil = 1 % m;
    basis %= m;
    while eks > 0 {
        if eks & 1 == 1 { hasil = kali_mod(hasil, basis, m); }
        basis = kali_mod(basis, basis, m);
        eks >>= 1;
    }
    hasil
}

/// Basis Miller-Rabin: 13 yang pertama PASTI untuk n < 3,3 x 10^24; di atas itu ke-40-nya
/// dipakai (belum ada bilangan komposit di bawah 2^127 yang diketahui lolos semuanya).
const BASIS_MR: [u128; 40] = [
    2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71,
    73, 79, 83, 89, 97, 101, 103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173,
];

fn is_prime_i128(n: i128) -> bool {
    if n < 2 { return false; }
    let n = n as u128;
    for p in BASIS_MR {
        if n == p { return true; }
        if n % p == 0 { return false; }
    }
    let (mut d, mut r) = (n - 1, 0u32);
    while d & 1 == 0 { d >>= 1; r += 1; }
    let banyak = if n < 3_317_044_064_679_887_385_961_981 { 13 } else { BASIS_MR.len() };
    'basis: for a in &BASIS_MR[..banyak] {
        let mut x = pangkat_mod(*a, d, n);
        if x == 1 || x == n - 1 { continue; }
        for _ in 1..r {
            x = kali_mod(x, x, n);
            if x == n - 1 { continue 'basis; }
        }
        return false;
    }
    true
}

pub fn num_is_prime(args: &[Value]) -> Result<Value, String> {
    let n = as_i128(arg(args, 0)).map_err(|e| format!("dok::num_is_prime: {}", e))?;
    Ok(Value::Bool(is_prime_i128(n)))
}

pub fn num_next_prime(args: &[Value]) -> Result<Value, String> {
    let n = as_i128(arg(args, 0)).map_err(|e| format!("dok::num_next_prime: {}", e))?;
    let mut c = n.max(1);
    loop {
        // 2^127 - 1 sendiri prima, jadi sesudahnya tidak ada lagi yang muat.
        c = c.checked_add(1).ok_or_else(|| format!(
            "dok::num_next_prime: tidak ada bilangan prima sesudah `{}` yang muat di integer 128 bit", n))?;
        if is_prime_i128(c) { return Ok(Value::Int(c)); }
    }
}

/// Satu pembagi sejati `n` (komposit, ganjil) lewat rho Pollard, atau `None` kalau anggaran
/// langkahnya habis — dua faktor prima raksasa tidak terjangkau cara ini.
fn pembagi_rho(n: u128) -> Option<u128> {
    fn fpb(mut a: u128, mut b: u128) -> u128 { while b != 0 { (a, b) = (b, a % b); } a }
    for c in 1..=8u128 {
        let (mut x, mut y, mut d) = (2u128, 2u128, 1u128);
        let mut langkah = 0u32;
        while d == 1 && langkah < 2_000_000 {
            x = (kali_mod(x, x, n) + c) % n;
            y = (kali_mod(y, y, n) + c) % n;
            y = (kali_mod(y, y, n) + c) % n;
            d = fpb(x.abs_diff(y), n);
            langkah += 1;
        }
        if d != 1 && d != n { return Some(d); }
    }
    None
}

pub fn num_prime_factors(args: &[Value]) -> Result<Value, String> {
    let asal = as_i128(arg(args, 0)).map_err(|e| format!("dok::num_prime_factors: {}", e))?;
    if asal < 2 { return Ok(Value::Array(Vec::new().into())); }
    let mut faktor: Vec<u128> = Vec::new();
    let mut n = asal as u128;
    // Faktor kecil lewat pembagian biasa; sisanya (kalau ada) lewat uji prima dan rho.
    let mut d = 2u128;
    while d < 100_000 && d * d <= n {
        while n % d == 0 { faktor.push(d); n /= d; }
        d += if d == 2 { 1 } else { 2 };
    }
    let mut antre = if n > 1 { vec![n] } else { Vec::new() };
    while let Some(k) = antre.pop() {
        if is_prime_i128(k as i128) { faktor.push(k); continue; }
        let Some(p) = pembagi_rho(k) else {
            return Err(format!("dok::num_prime_factors: faktor `{}` dari `{}` terlalu besar untuk diuraikan \
                (bukan prima, dan pembaginya di luar jangkauan pencarian)", k, asal));
        };
        antre.push(p);
        antre.push(k / p);
    }
    faktor.sort_unstable();
    Ok(Value::Array(faktor.into_iter().map(|f| Value::Int(f as i128)).collect::<Vec<_>>().into()))
}

pub fn num_mod_pow(args: &[Value]) -> Result<Value, String> {
    let base = as_i128(arg(args, 0)).map_err(|e| format!("dok::num_mod_pow: {}", e))?;
    let exp = as_i128(arg(args, 1)).map_err(|e| format!("dok::num_mod_pow: {}", e))?;
    let modulus = as_i128(arg(args, 2)).map_err(|e| format!("dok::num_mod_pow: {}", e))?;
    if modulus == 0 { return Err("dok::num_mod_pow: modulus tidak boleh 0".to_string()); }
    if exp < 0 { return Err("dok::num_mod_pow: eksponen negatif tidak didukung".to_string()); }
    // Hasilnya di 0..|modulus|, seperti `rem_euclid`.
    let m = modulus.unsigned_abs();
    let b = if base < 0 { (m - base.unsigned_abs() % m) % m } else { base as u128 % m };
    Ok(Value::Int(pangkat_mod(b, exp as u128, m) as i128))
}

const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

pub fn num_to_base(args: &[Value]) -> Result<Value, String> {
    let n = as_i128(arg(args, 0)).map_err(|e| format!("dok::num_to_base: {}", e))?;
    let base = as_i128(arg(args, 1)).map_err(|e| format!("dok::num_to_base: {}", e))?;
    if !(2..=36).contains(&base) { return Err("dok::num_to_base: base harus 2..36".to_string()); }
    if n == 0 { return Ok(Value::Str("0".to_string())); }
    let neg = n < 0;
    // `-MIN` tidak muat di i128: besarnya dihitung tanpa tanda.
    let (mut n, base) = (n.unsigned_abs(), base as u128);
    let mut out = Vec::new();
    while n > 0 { out.push(DIGITS[(n % base) as usize]); n /= base; }
    if neg { out.push(b'-'); }
    out.reverse();
    Ok(Value::Str(String::from_utf8(out).unwrap()))
}

pub fn num_from_base(args: &[Value]) -> Result<Value, String> {
    let s = match args.get(0) { Some(Value::Str(s)) => s.clone(), other => return Err(format!("dok::num_from_base: butuh string, dapat `{}`",
        other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada argumen".to_string()))) };
    let base = as_i128(arg(args, 1)).map_err(|e| format!("dok::num_from_base: {}", e))?;
    if !(2..=36).contains(&base) { return Err("dok::num_from_base: base harus 2..36".to_string()); }
    let (neg, digits) = match s.strip_prefix('-') { Some(rest) => (true, rest), None => (false, s.as_str()) };
    if digits.is_empty() { return Err("dok::num_from_base: string kosong".to_string()); }
    let mut n: i128 = 0;
    for c in digits.chars() {
        let d = DIGITS.iter().position(|&b| (b as char).eq_ignore_ascii_case(&c))
            .ok_or_else(|| format!("dok::num_from_base: digit tidak valid `{}`", c))? as i128;
        if d >= base { return Err(format!("dok::num_from_base: digit `{}` tidak valid untuk base {}", c, base)); }
        // Dikumpulkan di sisi tandanya sendiri, supaya batas bawah 128 bit ikut terjangkau.
        n = n.checked_mul(base).and_then(|k| if neg { k.checked_sub(d) } else { k.checked_add(d) })
            .ok_or_else(|| format!("dok::num_from_base: `{}` di luar jangkauan integer 128 bit", s))?;
    }
    Ok(Value::Int(n))
}

// ─── Pencarian akar (butuh callback ke interpreter) ─────────────────────

/// `dok::num_solve(f, x0, x1, tol?, max_iter?) -> Result<float, str>` —
/// metode secant (tidak butuh turunan eksplisit, cukup dua tebakan awal).
pub type Pemanggil<'a> = &'a mut dyn FnMut(&Value, Vec<Value>) -> Result<Value, String>;

/// Pembungkus untuk interpreter, supaya situs panggilan lamanya tak berubah.
pub fn num_solve(interp: &mut Interpreter, args: &[Value]) -> Result<Value, Signal> {
    let mut panggil = |f: &Value, a: Vec<Value>| -> Result<Value, String> {
        match interp.call_fn(f.clone(), a) {
            Ok(v) => Ok(v),
            Err(Signal::Return(v)) => Ok(v),
            Err(e) => Err(format!("{}", e)),
        }
    };
    num_solve_dgn(&mut panggil, args)
        .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))
}

pub fn num_solve_dgn(panggil: Pemanggil<'_>, args: &[Value]) -> Result<Value, String> {
    let f = match args.get(0) {
        Some(v) if v.bisa_dipanggil() => v.clone(),
        _ => return Err("dok::num_solve: argumen 1 harus function/closure".to_string()),
    };
    let mut x0 = as_f64(arg(args, 1)).map_err(|e| format!("dok::num_solve: {}", e))?;
    let mut x1 = as_f64(arg(args, 2)).map_err(|e| format!("dok::num_solve: {}", e))?;
    let tol = match args.get(3) { Some(v) => as_f64(v).unwrap_or(1e-9), None => 1e-9 };
    let max_iter = match args.get(4) { Some(Value::Int(n)) => (*n).clamp(1, 10_000) as usize, _ => 100 };

    let call = |panggil: Pemanggil<'_>, x: f64| -> Result<f64, String> {
        let r = panggil(&f, vec![Value::Float(x)])?;
        as_f64(&r).map_err(|e| format!("dok::num_solve: fungsi harus balikin number: {}", e))
    };

    let mut f0 = call(panggil, x0)?;
    for _ in 0..max_iter {
        let f1 = call(panggil, x1)?;
        if (f1 - f0).abs() < 1e-15 {
            return Ok(if f1.abs() < tol { ok_value(Value::Float(x1)) } else { err_value("dok::num_solve: turunan mendekati nol, berhenti") });
        }
        let x2 = x1 - f1 * (x1 - x0) / (f1 - f0);
        if (x2 - x1).abs() < tol {
            return Ok(ok_value(Value::Float(x2)));
        }
        x0 = x1; f0 = f1; x1 = x2;
    }
    Ok(err_value("dok::num_solve: tidak konvergen dalam batas iterasi"))
}
