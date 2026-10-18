// src/wokwok/ops.rs — wokwok/ops.rs — operasi elemen-demi-elemen, broadcasting, dan bentuk.

use super::{Buf, Hasil, Nd};

// ─── Promosi tipe ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tipe { Bool, I64, F64 }

fn tipe_dari(b: &Buf) -> Tipe {
    match b { Buf::Bool(_) => Tipe::Bool, Buf::I64(_) => Tipe::I64, Buf::F64(_) => Tipe::F64 }
}

fn gabung_tipe(a: &Buf, b: &Buf) -> Tipe {
    tipe_dari(a).max(tipe_dari(b))
}

// ─── Broadcasting ────────────────────────────────────────────────────────

/// Bentuk hasil dari dua bentuk, mengikuti aturan yang sama dengan numpy:
/// disejajarkan dari KANAN, tiap sumbu harus sama atau salah satunya 1.
pub fn bentuk_broadcast(a: &[usize], b: &[usize]) -> Hasil<Vec<usize>> {
    let n = a.len().max(b.len());
    let mut out = vec![0usize; n];
    for i in 0..n {
        let da = if i < n - a.len() { 1 } else { a[i - (n - a.len())] };
        let db = if i < n - b.len() { 1 } else { b[i - (n - b.len())] };
        out[i] = if da == db { da }
            else if da == 1 { db }
            else if db == 1 { da }
            else {
                return Err(format!(
                    "bentuk {:?} dan {:?} tidak bisa disejajarkan (sumbu {}: {} lawan {})",
                    a, b, i, da, db
                ));
            };
    }
    Ok(out)
}

/// Langkah (stride) per sumbu untuk membaca `shape` seolah-olah ia sudah
/// dilebarkan ke `target`. Sumbu berukuran 1 dapat langkah 0 — itulah
/// seluruh trik broadcasting: sumbu itu dibaca ulang, bukan disalin.
fn langkah_broadcast(shape: &[usize], target: &[usize]) -> Vec<usize> {
    let mut langkah = vec![0usize; target.len()];
    let geser = target.len() - shape.len();
    let mut akum = 1usize;
    for i in (0..shape.len()).rev() {
        langkah[i + geser] = if shape[i] == 1 { 0 } else { akum };
        akum *= shape[i];
    }
    langkah
}

/// Jalankan `f` untuk tiap posisi di `target`, memberikan indeks rata ke
/// dalam `a` dan `b`.
fn per_elemen_broadcast(
    sa: &[usize], sb: &[usize], target: &[usize],
    mut f: impl FnMut(usize, usize),
) {
    let la = langkah_broadcast(sa, target);
    let lb = langkah_broadcast(sb, target);
    let total: usize = target.iter().product();
    let mut koord = vec![0usize; target.len()];
    let (mut ia, mut ib) = (0usize, 0usize);
    for _ in 0..total {
        f(ia, ib);
        // Naikkan koordinat dari sumbu paling dalam, sambil memperbarui
        // kedua indeks — jauh lebih murah daripada menghitung ulang
        // indeks rata dari koordinat di tiap langkah.
        for s in (0..target.len()).rev() {
            koord[s] += 1;
            ia += la[s];
            ib += lb[s];
            if koord[s] < target[s] { break; }
            ia -= la[s] * target[s];
            ib -= lb[s] * target[s];
            koord[s] = 0;
        }
    }
}

// ─── Paralel ─────────────────────────────────────────────────────────────

/// Ambang paralel untuk operasi elemen-demi-elemen.
const AMBANG_UTAS: usize = 6_000_000;

/// Bagi keluaran jadi potongan yang TIDAK bertumpuk, kerjakan paralel.
fn paralel<T: Send>(out: &mut [T], f: impl Fn(usize, &mut [T]) + Sync) {
    let n = out.len();
    let ambang = std::env::var("JRG_WW_AMBANG").ok()
        .and_then(|v| v.parse::<usize>().ok()).unwrap_or(AMBANG_UTAS);
    let utas = if n >= ambang {
        std::thread::available_parallelism().map(|p| p.get()).unwrap_or(1)
    } else { 1 };
    if utas <= 1 {
        f(0, out);
        return;
    }
    let per = (n + utas - 1) / utas;
    let f = &f;
    std::thread::scope(|s| {
        let mut sisa = out;
        let mut mulai = 0usize;
        while !sisa.is_empty() {
            let ambil = per.min(sisa.len());
            let (kiri, kanan) = sisa.split_at_mut(ambil);
            let awal = mulai;
            s.spawn(move || f(awal, kiri));
            sisa = kanan;
            mulai += ambil;
        }
    });
}

// ─── Operasi biner ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Biner { Tambah, Kurang, Kali, Bagi, Pangkat, Sisa,
                 Min, Maks,
                 Sama, Beda, Kurang_, KurangSama, Lebih, LebihSama,
                 Dan, Atau }

impl Biner {
    fn simbol(self) -> &'static str {
        match self {
            Biner::Tambah => "+", Biner::Kurang => "-", Biner::Kali => "*",
            Biner::Bagi => "/", Biner::Pangkat => "**", Biner::Sisa => "%",
            Biner::Min => "min", Biner::Maks => "maks",
            Biner::Sama => "==", Biner::Beda => "!=",
            Biner::Kurang_ => "<", Biner::KurangSama => "<=",
            Biner::Lebih => ">", Biner::LebihSama => ">=",
            Biner::Dan => "&&", Biner::Atau => "||",
        }
    }
    fn pembanding(self) -> bool {
        matches!(self, Biner::Sama | Biner::Beda | Biner::Kurang_
                     | Biner::KurangSama | Biner::Lebih | Biner::LebihSama)
    }
    fn logika(self) -> bool { matches!(self, Biner::Dan | Biner::Atau) }
}

fn f_op(op: Biner, x: f64, y: f64) -> f64 {
    match op {
        Biner::Tambah => x + y, Biner::Kurang => x - y,
        Biner::Kali => x * y, Biner::Bagi => x / y,
        Biner::Pangkat => x.powf(y), Biner::Sisa => x % y,
        // NaN menular, bukan diam-diam kalah. `min` yang mengabaikan NaN
        // akan menjawab angka yang tampak sah untuk data yang jelas rusak.
        Biner::Min => if x.is_nan() || y.is_nan() { f64::NAN } else { x.min(y) },
        Biner::Maks => if x.is_nan() || y.is_nan() { f64::NAN } else { x.max(y) },
        _ => unreachable!("pembanding/logika tidak lewat f_op"),
    }
}

fn i_op(op: Biner, x: i64, y: i64) -> Hasil<i64> {
    let luber = |o: &str| format!(
        "wokwok: `{} {} {}` kelewat besar buat integer 64 bit", x, o, y);
    Ok(match op {
        Biner::Tambah => x.checked_add(y).ok_or_else(|| luber("+"))?,
        Biner::Kurang => x.checked_sub(y).ok_or_else(|| luber("-"))?,
        Biner::Kali   => x.checked_mul(y).ok_or_else(|| luber("*"))?,
        Biner::Sisa   => {
            if y == 0 { return Err("wokwok: sisa bagi dengan nol".to_string()); }
            x.checked_rem(y).ok_or_else(|| luber("%"))?
        }
        Biner::Min => x.min(y),
        Biner::Maks => x.max(y),
        _ => unreachable!("op ini tidak dilayani jalur integer"),
    })
}

fn banding(op: Biner, x: f64, y: f64) -> bool {
    match op {
        Biner::Sama => x == y, Biner::Beda => x != y,
        Biner::Kurang_ => x < y, Biner::KurangSama => x <= y,
        Biner::Lebih => x > y, Biner::LebihSama => x >= y,
        _ => unreachable!(),
    }
}

/// Operasi biner dengan broadcasting.
pub fn biner(a: &Nd, b: &Nd, op: Biner) -> Hasil<Nd> {
    let shape = bentuk_broadcast(&a.shape, &b.shape)
        .map_err(|e| format!("wokwok `{}`: {}", op.simbol(), e))?;
    let total: usize = shape.iter().product();

    if op.logika() {
        let (x, y) = (bool_data(a), bool_data(b));
        let mut out = vec![false; total];
        isi_bool(&a.shape, &b.shape, &shape, &x, &y, &mut out, |p, q| {
            if op == Biner::Dan { p && q } else { p || q }
        });
        return Nd::baru(Buf::Bool(out), shape);
    }

    if op.pembanding() {
        let (x, y) = (a.f64_pinjam(), b.f64_pinjam());
        let mut out = vec![false; total];
        isi_f64_ke_bool(&a.shape, &b.shape, &shape, &x, &y, &mut out, op);
        return Nd::baru(Buf::Bool(out), shape);
    }

    // Pembagian dan pemangkatan SELALU menghasilkan pecahan.
    let hasil_int = gabung_tipe(&a.buf, &b.buf) <= Tipe::I64
        && !matches!(op, Biner::Bagi | Biner::Pangkat);

    if hasil_int {
        let (x, y) = (i64_data(a), i64_data(b));
        let mut out = vec![0i64; total];
        isi_i64(&a.shape, &b.shape, &shape, &x, &y, &mut out, op)?;
        return Nd::baru(Buf::I64(out), shape);
    }

    let (x, y) = (a.f64_pinjam(), b.f64_pinjam());
    let mut out = vec![0.0f64; total];
    isi_f64(&a.shape, &b.shape, &shape, &x, &y, &mut out, op);
    Nd::baru(Buf::F64(out), shape)
}

// Tiap `isi_*` punya tiga cabang yang sama: bentuk sama, salah satu skalar,
// lalu jalur umum.

/// Kenapa operasinya dipilih DI LUAR loop.
macro_rules! jalankan_f64 {
    ($sa:expr, $sb:expr, $target:expr, $x:expr, $y:expr, $out:expr, $f:expr) => {{
        let f = $f;
        if $sa == $sb {
            // `zip` alih-alih indeks: menghilangkan pemeriksaan batas di
            // loop terdalam, yang juga menghalangi vektorisasi.
            paralel($out, |awal, bagian| {
                let (xs, ys) = (&$x[awal..awal + bagian.len()], &$y[awal..awal + bagian.len()]);
                for (o, (&p, &q)) in bagian.iter_mut().zip(xs.iter().zip(ys.iter())) {
                    *o = f(p, q);
                }
            });
        } else if $x.len() == 1 {
            let k = $x[0];
            paralel($out, |awal, bagian| {
                let ys = &$y[awal..awal + bagian.len()];
                for (o, &q) in bagian.iter_mut().zip(ys.iter()) { *o = f(k, q); }
            });
        } else if $y.len() == 1 {
            let k = $y[0];
            paralel($out, |awal, bagian| {
                let xs = &$x[awal..awal + bagian.len()];
                for (o, &p) in bagian.iter_mut().zip(xs.iter()) { *o = f(p, k); }
            });
        } else {
            let mut i = 0usize;
            per_elemen_broadcast($sa, $sb, $target, |ia, ib| {
                $out[i] = f($x[ia], $y[ib]); i += 1;
            });
        }
    }};
}

fn isi_f64(sa: &[usize], sb: &[usize], target: &[usize],
           x: &[f64], y: &[f64], out: &mut [f64], op: Biner) {
    match op {
        Biner::Tambah  => jalankan_f64!(sa, sb, target, x, y, out, |p: f64, q: f64| p + q),
        Biner::Kurang  => jalankan_f64!(sa, sb, target, x, y, out, |p: f64, q: f64| p - q),
        Biner::Kali    => jalankan_f64!(sa, sb, target, x, y, out, |p: f64, q: f64| p * q),
        Biner::Bagi    => jalankan_f64!(sa, sb, target, x, y, out, |p: f64, q: f64| p / q),
        Biner::Sisa    => jalankan_f64!(sa, sb, target, x, y, out, |p: f64, q: f64| p % q),
        Biner::Pangkat => jalankan_f64!(sa, sb, target, x, y, out, |p: f64, q: f64| p.powf(q)),
        // NaN menular di min/maks — lihat catatan di `f_op`.
        Biner::Min => jalankan_f64!(sa, sb, target, x, y, out,
            |p: f64, q: f64| if p.is_nan() || q.is_nan() { f64::NAN } else { p.min(q) }),
        Biner::Maks => jalankan_f64!(sa, sb, target, x, y, out,
            |p: f64, q: f64| if p.is_nan() || q.is_nan() { f64::NAN } else { p.max(q) }),
        _ => unreachable!("pembanding/logika tidak lewat isi_f64"),
    }
}

fn isi_i64(sa: &[usize], sb: &[usize], target: &[usize],
           x: &[i64], y: &[i64], out: &mut [i64], op: Biner) -> Hasil<()> {
    if sa == sb {
        for i in 0..out.len() { out[i] = i_op(op, x[i], y[i])?; }
    } else if x.len() == 1 {
        let k = x[0];
        for i in 0..out.len() { out[i] = i_op(op, k, y[i])?; }
    } else if y.len() == 1 {
        let k = y[0];
        for i in 0..out.len() { out[i] = i_op(op, x[i], k)?; }
    } else {
        let mut galat: Option<String> = None;
        let mut i = 0usize;
        per_elemen_broadcast(sa, sb, target, |ia, ib| {
            if galat.is_some() { return; }
            match i_op(op, x[ia], y[ib]) {
                Ok(v) => out[i] = v,
                Err(e) => galat = Some(e),
            }
            i += 1;
        });
        if let Some(e) = galat { return Err(e); }
    }
    Ok(())
}

macro_rules! jalankan_bool {
    ($sa:expr, $sb:expr, $target:expr, $x:expr, $y:expr, $out:expr, $f:expr) => {{
        let f = $f;
        if $sa == $sb {
            for (o, (&p, &q)) in $out.iter_mut().zip($x.iter().zip($y.iter())) { *o = f(p, q); }
        } else if $x.len() == 1 {
            let k = $x[0];
            for (o, &q) in $out.iter_mut().zip($y.iter()) { *o = f(k, q); }
        } else if $y.len() == 1 {
            let k = $y[0];
            for (o, &p) in $out.iter_mut().zip($x.iter()) { *o = f(p, k); }
        } else {
            let mut i = 0usize;
            per_elemen_broadcast($sa, $sb, $target, |ia, ib| {
                $out[i] = f($x[ia], $y[ib]); i += 1;
            });
        }
    }};
}

fn isi_f64_ke_bool(sa: &[usize], sb: &[usize], target: &[usize],
                   x: &[f64], y: &[f64], out: &mut [bool], op: Biner) {
    match op {
        Biner::Sama       => jalankan_bool!(sa, sb, target, x, y, out, |p: f64, q: f64| p == q),
        Biner::Beda       => jalankan_bool!(sa, sb, target, x, y, out, |p: f64, q: f64| p != q),
        Biner::Kurang_    => jalankan_bool!(sa, sb, target, x, y, out, |p: f64, q: f64| p < q),
        Biner::KurangSama => jalankan_bool!(sa, sb, target, x, y, out, |p: f64, q: f64| p <= q),
        Biner::Lebih      => jalankan_bool!(sa, sb, target, x, y, out, |p: f64, q: f64| p > q),
        Biner::LebihSama  => jalankan_bool!(sa, sb, target, x, y, out, |p: f64, q: f64| p >= q),
        _ => unreachable!(),
    }
}

fn isi_bool(sa: &[usize], sb: &[usize], target: &[usize],
            x: &[bool], y: &[bool], out: &mut [bool],
            f: impl Fn(bool, bool) -> bool) {
    if sa == sb {
        for i in 0..out.len() { out[i] = f(x[i], y[i]); }
    } else if x.len() == 1 {
        let k = x[0];
        for i in 0..out.len() { out[i] = f(k, y[i]); }
    } else if y.len() == 1 {
        let k = y[0];
        for i in 0..out.len() { out[i] = f(x[i], k); }
    } else {
        let mut i = 0usize;
        per_elemen_broadcast(sa, sb, target, |ia, ib| {
            out[i] = f(x[ia], y[ib]); i += 1;
        });
    }
}

fn i64_data(a: &Nd) -> Vec<i64> {
    match &a.buf {
        Buf::I64(v) => v.clone(),
        Buf::Bool(v) => v.iter().map(|&b| b as i64).collect(),
        Buf::F64(v) => v.iter().map(|&x| x as i64).collect(),
    }
}

fn bool_data(a: &Nd) -> Vec<bool> {
    match &a.buf {
        Buf::Bool(v) => v.clone(),
        Buf::I64(v) => v.iter().map(|&x| x != 0).collect(),
        Buf::F64(v) => v.iter().map(|&x| x != 0.0).collect(),
    }
}

// ─── Operasi satu argumen ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub enum Unari { Akar, Exp, Ln, Log2, Log10, Sin, Cos, Tan, Asin, Acos, Atan,
                 Sinh, Cosh, Tanh, Mutlak, Lantai, Langit, Bulat, Potong,
                 Tanda, Negasi, Balik, BukanLogika }

pub fn unari(a: &Nd, op: Unari) -> Hasil<Nd> {
    // Negasi dan nilai mutlak MEMPERTAHANKAN tipe: menaikkan integer jadi
    // pecahan cuma karena tandanya dibalik akan mengagetkan, dan diam-diam
    // membuat hasil selanjutnya ikut pecahan.
    if matches!(op, Unari::Negasi | Unari::Mutlak) {
        if let Buf::I64(v) = &a.buf {
            let mut out = Vec::with_capacity(v.len());
            for &x in v {
                let y = if matches!(op, Unari::Negasi) { x.checked_neg() } else { x.checked_abs() };
                out.push(y.ok_or_else(|| format!(
                    "wokwok: lawan dari {} tidak muat di integer 64 bit", x))?);
            }
            return Nd::baru(Buf::I64(out), a.shape.clone());
        }
    }
    if matches!(op, Unari::BukanLogika) {
        let v = bool_data(a);
        return Nd::baru(Buf::Bool(v.iter().map(|&b| !b).collect()), a.shape.clone());
    }
    if matches!(op, Unari::Balik) {
        // Dibalik urutannya di sumbu PALING LUAR, sama seperti `arr::reverse`
        // pada array biasa dan sama seperti `a[::-1]` di numpy.
        return balik_sumbu_luar(a);
    }
    let v = a.f64_pinjam();
    // Sama seperti operasi biner: fungsinya dipilih SEKALI di sini, bukan
    // per elemen. Tanpa ini `sqrt` atas sejuta elemen membawa satu `match`
    // 20-cabang di dalam loop terdalamnya.
    macro_rules! peta {
        ($f:expr) => {{
            let f = $f;
            let mut out = vec![0.0f64; v.len()];
            paralel(&mut out, |awal, bagian| {
                let xs = &v[awal..awal + bagian.len()];
                for (o, &x) in bagian.iter_mut().zip(xs.iter()) { *o = f(x); }
            });
            out
        }};
    }
    let out: Vec<f64> = match op {
        Unari::Akar => peta!(f64::sqrt),   Unari::Exp => peta!(f64::exp),
        Unari::Ln => peta!(f64::ln),       Unari::Log2 => peta!(f64::log2),
        Unari::Log10 => peta!(f64::log10), Unari::Sin => peta!(f64::sin),
        Unari::Cos => peta!(f64::cos),     Unari::Tan => peta!(f64::tan),
        Unari::Asin => peta!(f64::asin),   Unari::Acos => peta!(f64::acos),
        Unari::Atan => peta!(f64::atan),   Unari::Sinh => peta!(f64::sinh),
        Unari::Cosh => peta!(f64::cosh),   Unari::Tanh => peta!(f64::tanh),
        Unari::Mutlak => peta!(f64::abs),  Unari::Lantai => peta!(f64::floor),
        Unari::Langit => peta!(f64::ceil), Unari::Bulat => peta!(f64::round),
        Unari::Potong => peta!(f64::trunc),
        Unari::Tanda => peta!(|x: f64| if x == 0.0 { 0.0 } else { x.signum() }),
        Unari::Negasi => peta!(|x: f64| -x),
        Unari::Balik | Unari::BukanLogika => unreachable!("sudah ditangani di atas"),
    };
    Nd::baru(Buf::F64(out), a.shape.clone())
}

fn balik_sumbu_luar(a: &Nd) -> Hasil<Nd> {
    if a.shape.is_empty() { return Ok(a.clone()); }
    let n = a.shape[0];
    let blok: usize = a.shape[1..].iter().product::<usize>().max(1);
    macro_rules! balik {
        ($v:expr, $bikin:expr) => {{
            let mut out = Vec::with_capacity($v.len());
            for i in (0..n).rev() {
                out.extend_from_slice(&$v[i * blok..(i + 1) * blok]);
            }
            Nd::baru($bikin(out), a.shape.clone())
        }};
    }
    match &a.buf {
        Buf::F64(v) => balik!(v, Buf::F64),
        Buf::I64(v) => balik!(v, Buf::I64),
        Buf::Bool(v) => balik!(v, Buf::Bool),
    }
}

/// Transpose: membalik urutan seluruh sumbu.
pub fn transpose(a: &Nd) -> Hasil<Nd> {
    if a.ndim() < 2 { return Ok(a.clone()); }
    let bentuk_baru: Vec<usize> = a.shape.iter().rev().cloned().collect();
    let total = a.size();
    let mut peta = Vec::with_capacity(total);
    // Langkah tiap sumbu di array ASAL.
    let mut langkah = vec![1usize; a.ndim()];
    for i in (0..a.ndim() - 1).rev() {
        langkah[i] = langkah[i + 1] * a.shape[i + 1];
    }
    let mut koord = vec![0usize; bentuk_baru.len()];
    for _ in 0..total {
        // Koordinat di hasil dibaca terbalik untuk mendapat koordinat asal.
        let mut src = 0usize;
        for (i, &k) in koord.iter().enumerate() {
            src += k * langkah[a.ndim() - 1 - i];
        }
        peta.push(src);
        for s in (0..bentuk_baru.len()).rev() {
            koord[s] += 1;
            if koord[s] < bentuk_baru[s] { break; }
            koord[s] = 0;
        }
    }
    let buf = match &a.buf {
        Buf::F64(v) => Buf::F64(peta.iter().map(|&i| v[i]).collect()),
        Buf::I64(v) => Buf::I64(peta.iter().map(|&i| v[i]).collect()),
        Buf::Bool(v) => Buf::Bool(peta.iter().map(|&i| v[i]).collect()),
    };
    Nd::baru(buf, bentuk_baru)
}

/// `where(kondisi, a, b)` — pilih elemen demi elemen.
pub fn pilih(k: &Nd, a: &Nd, b: &Nd) -> Hasil<Nd> {
    let s1 = bentuk_broadcast(&k.shape, &a.shape)
        .map_err(|e| format!("wokwok::pilih: {}", e))?;
    let shape = bentuk_broadcast(&s1, &b.shape)
        .map_err(|e| format!("wokwok::pilih: {}", e))?;
    let total: usize = shape.iter().product();
    let kb = bool_data(k);
    let ambil = |nd: &Nd, i: usize| -> usize {
        if nd.size() == 1 { 0 } else { i.min(nd.size() - 1) }
    };
    // Jalur umum saja: `pilih` jarang jadi titik panas, dan menyalakan
    // tiga jalur cepat untuk tiga operand berarti delapan kombinasi yang
    // semuanya harus diuji.
    let idx_k = |i: usize| if kb.len() == 1 { 0 } else { i };
    if matches!((&a.buf, &b.buf), (Buf::I64(_), Buf::I64(_))) {
        let (x, y) = (i64_data(a), i64_data(b));
        let out: Vec<i64> = (0..total)
            .map(|i| if kb[idx_k(i)] { x[ambil(a, i)] } else { y[ambil(b, i)] })
            .collect();
        return Nd::baru(Buf::I64(out), shape);
    }
    let (x, y) = (a.f64_data(), b.f64_data());
    let out: Vec<f64> = (0..total)
        .map(|i| if kb[idx_k(i)] { x[ambil(a, i)] } else { y[ambil(b, i)] })
        .collect();
    Nd::baru(Buf::F64(out), shape)
}

/// Jepit tiap elemen ke rentang `[lo, hi]`.
pub fn jepit(a: &Nd, lo: f64, hi: f64) -> Hasil<Nd> {
    if lo > hi {
        return Err(format!("wokwok::jepit: batas bawah {} lebih besar dari batas atas {}", lo, hi));
    }
    match &a.buf {
        Buf::I64(v) => {
            let (l, h) = (lo as i64, hi as i64);
            Nd::baru(Buf::I64(v.iter().map(|&x| x.clamp(l, h)).collect()), a.shape.clone())
        }
        _ => {
            let v = a.f64_data();
            Nd::baru(Buf::F64(v.iter().map(|&x| x.clamp(lo, hi)).collect()), a.shape.clone())
        }
    }
}
