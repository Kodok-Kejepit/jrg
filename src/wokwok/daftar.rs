// src/wokwok/daftar.rs — wokwok/daftar.rs — permukaan `wokwok::` yang dilihat program JRG.

use std::sync::Arc;

use crate::vm::value::VmValue;
use crate::vm::vm::Vm;
use super::{Buf, Hasil, Nd};
use super::ops::{self, Biner, Unari};
use super::reduce::{self, Reduksi};
use super::linalg;

// ─── Konversi masuk ──────────────────────────────────────────────────────

/// Ubah nilai JRG apa pun jadi `Nd`.
pub fn ke_nd<'a>(v: &'a VmValue, konteks: &str) -> Hasil<std::borrow::Cow<'a, Nd>> {
    use std::borrow::Cow;
    match v {
        VmValue::Nd(nd) => Ok(Cow::Borrowed(nd)),
        VmValue::Float(x) => Ok(Cow::Owned(Nd::skalar_f64(*x))),
        VmValue::Int(n) => {
            // Integer JRG 128 bit, buffer wokwok 64 bit. Yang tidak muat
            // DITOLAK, bukan dipotong — memotong adalah cara paling halus
            // untuk menjawab angka yang salah.
            i64::try_from(*n).map(|k| Cow::Owned(Nd::skalar_i64(k))).map_err(|_| format!(
                "{}: {} kelewat besar buat array numerik (64 bit)", konteks, n))
        }
        VmValue::Bool(b) => Ok(Cow::Owned(Nd::skalar_bool(*b))),
        VmValue::Array(_) => {
            let mut bentuk = Vec::new();
            let mut rata = Vec::new();
            let mut tipe = Tipe::Bool;
            ratakan(v, 0, &mut bentuk, &mut rata, &mut tipe, konteks)?;
            let buf = match tipe {
                Tipe::Bool => Buf::Bool(rata.iter().map(|&x| x != 0.0).collect()),
                Tipe::I64 => Buf::I64(rata.iter().map(|&x| x as i64).collect()),
                Tipe::F64 => Buf::F64(rata),
            };
            Nd::baru(buf, bentuk).map(Cow::Owned)
        }
        lain => Err(format!(
            "{}: `{}` bukan angka atau array — wokwok cuma menerima angka, bool, \
             array, atau hasil wokwok lain",
            konteks, lain.type_name()
        )),
    }
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Tipe { Bool, I64, F64 }

/// Ratakan array bersarang sambil memeriksa bahwa bentuknya PERSEGI.
fn ratakan(
    v: &VmValue, level: usize,
    bentuk: &mut Vec<usize>, keluar: &mut Vec<f64>, tipe: &mut Tipe,
    konteks: &str,
) -> Hasil<()> {
    match v {
        VmValue::Array(a) => {
            let isi = a.lock().unwrap_or_else(|p| p.into_inner());
            if bentuk.len() == level {
                bentuk.push(isi.len());
            } else if bentuk[level] != isi.len() {
                return Err(format!(
                    "{}: bentuknya tidak persegi — di kedalaman {} ada baris berisi {} \
                     elemen sementara yang lain {}",
                    konteks, level, isi.len(), bentuk[level]
                ));
            }
            for x in isi.iter() { ratakan(x, level + 1, bentuk, keluar, tipe, konteks)?; }
            Ok(())
        }
        VmValue::Float(x) => { *tipe = (*tipe).max(Tipe::F64); keluar.push(*x); Ok(()) }
        VmValue::Int(n) => {
            let n = i64::try_from(*n).map_err(|_| format!(
                "{}: {} kelewat besar buat array numerik (64 bit)", konteks, n))?;
            *tipe = (*tipe).max(Tipe::I64);
            keluar.push(n as f64);
            Ok(())
        }
        VmValue::Bool(b) => { keluar.push(if *b { 1.0 } else { 0.0 }); Ok(()) }
        lain => Err(format!(
            "{}: ada `{}` di dalam datanya — isi array numerik harus angka atau bool",
            konteks, lain.type_name()
        )),
    }
}

impl Tipe {
    fn max(self, lain: Tipe) -> Tipe { if lain > self { lain } else { self } }
}

fn dari_nd(a: Nd) -> VmValue { VmValue::Nd(Arc::new(a)) }

/// Hasil yang dijanjikan sebuah ANGKA: skalar 0-dimensi dibuka. Dulu
/// `wokwok::sum(a) == 3.0` menjawab `false` karena yang kembali larik.
fn dari_nd_skalar(a: Nd) -> VmValue {
    if !a.skalar() { return dari_nd(a); }
    match &a.buf {
        Buf::F64(v) => VmValue::Float(v[0]),
        Buf::I64(v) => VmValue::Int(v[0] as i128),
        Buf::Bool(v) => VmValue::Bool(v[0]),
    }
}

fn ke_f64(v: &VmValue, konteks: &str) -> Hasil<f64> {
    match v {
        VmValue::Float(x) => Ok(*x),
        VmValue::Int(n) => Ok(*n as f64),
        VmValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        VmValue::Nd(nd) => nd.ke_f64_tunggal(konteks),
        lain => Err(format!("{}: butuh angka, dapat `{}`", konteks, lain.type_name())),
    }
}

fn ke_i64(v: &VmValue, konteks: &str) -> Hasil<i64> {
    match v {
        VmValue::Int(n) => i64::try_from(*n).map_err(|_| format!(
            "{}: {} kelewat besar (64 bit)", konteks, n)),
        VmValue::Float(x) => {
            if x.fract() != 0.0 || !x.is_finite() {
                return Err(format!("{}: butuh bilangan bulat, dapat {}", konteks, x));
            }
            Ok(*x as i64)
        }
        VmValue::Nd(nd) => {
            let x = nd.ke_f64_tunggal(konteks)?;
            if x.fract() != 0.0 { return Err(format!("{}: butuh bilangan bulat, dapat {}", konteks, x)); }
            Ok(x as i64)
        }
        lain => Err(format!("{}: butuh bilangan bulat, dapat `{}`", konteks, lain.type_name())),
    }
}

/// Bentuk dari sebuah angka (`5` → `[5]`) atau array angka (`[2,3]`).
const MAKS_ELEMEN: usize = 256 * 1024 * 1024 / 8;

/// Tolak permintaan alokasi yang kegedean, dengan menyebut angkanya.
pub(crate) fn periksa_ukuran(total: usize, konteks: &str) -> Hasil<()> {
    if total > MAKS_ELEMEN {
        return Err(format!(
            "{}: minta {} elemen (~{} MiB) — batasnya {} elemen (256 MiB). \
             Cek lagi ukurannya",
            konteks, total, total.saturating_mul(8) / (1024 * 1024), MAKS_ELEMEN));
    }
    Ok(())
}

fn ke_bentuk(v: &VmValue, konteks: &str) -> Hasil<Vec<usize>> {
    let ambil = |x: i64| -> Hasil<usize> {
        if x < 0 { Err(format!("{}: panjang sumbu tidak boleh negatif ({})", konteks, x)) }
        else { Ok(x as usize) }
    };
    match v {
        VmValue::Int(_) | VmValue::Float(_) => {
            let n = ambil(ke_i64(v, konteks)?)?;
            periksa_ukuran(n, konteks)?;
            Ok(vec![n])
        }
        VmValue::Array(a) => {
            let isi = a.lock().unwrap_or_else(|p| p.into_inner());
            let mut out = Vec::with_capacity(isi.len());
            for x in isi.iter() { out.push(ambil(ke_i64(x, konteks)?)?); }
            if out.len() > 32 {
                return Err(format!(
                    "{}: bentuknya {} dimensi, batasnya 32 — sama dengan numpy", konteks, out.len()));
            }
            // Sumbu bukan-nol juga harus muat: dengan satu sumbu 0 hasil kali totalnya 0, tapi
            // stride sumbu lain tetap dihitung dan dulu meluap diam-diam.
            out.iter().filter(|n| **n != 0).try_fold(1usize, |t, n| t.checked_mul(*n))
                .ok_or_else(|| format!("{}: hasil kali bentuknya melebihi ukuran mesin", konteks))?;
            // Hasil kali sumbu-sumbunya, bukan tiap sumbu sendiri: sebuah
            // bentuk `[100000, 100000]` isinya wajar per sumbu dan sepuluh
            // miliar elemen kalau dikalikan.
            let total = out.iter().try_fold(1usize, |t, n| t.checked_mul(*n))
                .ok_or_else(|| format!("{}: hasil kali bentuknya melebihi ukuran mesin", konteks))?;
            periksa_ukuran(total, konteks)?;
            Ok(out)
        }
        lain => Err(format!(
            "{}: bentuk harus angka atau array angka, dapat `{}`", konteks, lain.type_name())),
    }
}

/// `Nd` kembali jadi array JRG bersarang.
pub fn ke_daftar(a: &Nd) -> VmValue {
    fn susun(a: &Nd, shape: &[usize], kursor: &mut usize) -> VmValue {
        if shape.is_empty() {
            let v = match &a.buf {
                Buf::F64(v) => VmValue::Float(v[*kursor]),
                Buf::I64(v) => VmValue::Int(v[*kursor] as i128),
                Buf::Bool(v) => VmValue::Bool(v[*kursor]),
            };
            *kursor += 1;
            return v;
        }
        let mut out = Vec::with_capacity(shape[0]);
        for _ in 0..shape[0] { out.push(susun(a, &shape[1..], kursor)); }
        VmValue::array(out)
    }
    let mut k = 0usize;
    susun(a, &a.shape, &mut k)
}

// ─── Pembangkit acak ─────────────────────────────────────────────────────
static ACAK: std::sync::Mutex<u64> = std::sync::Mutex::new(0x853c49e6748fea9b);

fn pcg() -> u32 {
    let mut s = ACAK.lock().unwrap_or_else(|p| p.into_inner());
    let lama = *s;
    *s = lama.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let xorshifted = (((lama >> 18) ^ lama) >> 27) as u32;
    let rot = (lama >> 59) as u32;
    xorshifted.rotate_right(rot)
}

fn acak_f64() -> f64 {
    // 53 bit mantissa dari dua tarikan 32-bit — supaya seluruh presisi f64
    // terpakai, bukan cuma 32 bit teratas.
    let hi = pcg() as u64;
    let lo = pcg() as u64;
    (((hi << 21) ^ lo) & ((1u64 << 53) - 1)) as f64 / (1u64 << 53) as f64
}

// ─── Pendaftaran ─────────────────────────────────────────────────────────

macro_rules! arg {
    ($a:expr, $i:expr, $nama:expr) => {
        $a.get($i).ok_or_else(|| format!("{}: kurang argumen", $nama))?
    };
}

pub fn register(vm: &mut Vm) {
    // ── Pembuatan ────────────────────────────────────────────────────────
    vm.def("wokwok::array", |a| {
        Ok(dari_nd(ke_nd(arg!(a, 0, "wokwok::array"), "wokwok::array")?.into_owned()))
    });
    vm.def("wokwok::zeros", |a| {
        let b = ke_bentuk(arg!(a, 0, "wokwok::zeros"), "wokwok::zeros")?;
        let n: usize = b.iter().product();
        Ok(dari_nd(Nd::baru(Buf::F64(vec![0.0; n]), b)?))
    });
    vm.def("wokwok::ones", |a| {
        let b = ke_bentuk(arg!(a, 0, "wokwok::ones"), "wokwok::ones")?;
        let n: usize = b.iter().product();
        Ok(dari_nd(Nd::baru(Buf::F64(vec![1.0; n]), b)?))
    });
    vm.def("wokwok::full", |a| {
        let b = ke_bentuk(arg!(a, 0, "wokwok::full"), "wokwok::full")?;
        let v = ke_f64(arg!(a, 1, "wokwok::full"), "wokwok::full")?;
        let n: usize = b.iter().product();
        Ok(dari_nd(Nd::baru(Buf::F64(vec![v; n]), b)?))
    });
    vm.def("wokwok::arange", |a| {
        let (mulai, henti, langkah) = match a.len() {
            0 => return Err("wokwok::arange butuh minimal (henti)".to_string()),
            1 => (0.0, ke_f64(&a[0], "wokwok::arange")?, 1.0),
            2 => (ke_f64(&a[0], "wokwok::arange")?, ke_f64(&a[1], "wokwok::arange")?, 1.0),
            _ => (ke_f64(&a[0], "wokwok::arange")?, ke_f64(&a[1], "wokwok::arange")?,
                  ke_f64(&a[2], "wokwok::arange")?),
        };
        if langkah == 0.0 {
            return Err("wokwok::arange: langkah 0 tidak pernah sampai ke ujung".to_string());
        }
        let n = ((henti - mulai) / langkah).ceil();
        let n = if n.is_finite() && n > 0.0 { n as usize } else { 0 };
        periksa_ukuran(n, "wokwok::arange")?;
        // Dihitung dari indeks (`mulai + i*langkah`), bukan ditambahkan
        // berulang — penambahan berulang mengakumulasi galat pembulatan,
        // jadi elemen terakhirnya melenceng makin jauh untuk n besar.
        let bulat = mulai.fract() == 0.0 && langkah.fract() == 0.0;
        let v: Vec<f64> = (0..n).map(|i| mulai + i as f64 * langkah).collect();
        let buf = if bulat { Buf::I64(v.iter().map(|&x| x as i64).collect()) } else { Buf::F64(v) };
        Ok(dari_nd(Nd::vektor(buf)))
    });
    vm.def("wokwok::linspace", |a| {
        let mulai = ke_f64(arg!(a, 0, "wokwok::linspace"), "wokwok::linspace")?;
        let henti = ke_f64(arg!(a, 1, "wokwok::linspace"), "wokwok::linspace")?;
        let n = ke_i64(arg!(a, 2, "wokwok::linspace"), "wokwok::linspace")?;
        if n < 0 { return Err("wokwok::linspace: jumlah titik tidak boleh negatif".to_string()); }
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        periksa_ukuran(n, "wokwok::linspace")?;
        let v: Vec<f64> = if n == 0 { vec![] }
            else if n == 1 { vec![mulai] }
            else {
                // Titik terakhir dipasang EKSAK, bukan hasil hitungan —
                // supaya `linspace(0, 1, n)` benar-benar berakhir di 1,0
                // dan bukan di 0,9999999999999999.
                let d = (henti - mulai) / (n - 1) as f64;
                let mut v: Vec<f64> = (0..n).map(|i| mulai + i as f64 * d).collect();
                v[n - 1] = henti;
                v
            };
        Ok(dari_nd(Nd::vektor(Buf::F64(v))))
    });
    vm.def("wokwok::eye", |a| {
        let n = ke_i64(arg!(a, 0, "wokwok::eye"), "wokwok::eye")?;
        if n < 0 { return Err("wokwok::eye: ukuran tidak boleh negatif".to_string()); }
        let n = n as usize;
        let total = n.checked_mul(n).ok_or_else(||
            "wokwok::eye: n*n melebihi ukuran mesin".to_string())?;
        periksa_ukuran(total, "wokwok::eye")?;
        let mut v = vec![0.0f64; total];
        for i in 0..n { v[i * n + i] = 1.0; }
        Ok(dari_nd(Nd::baru(Buf::F64(v), vec![n, n])?))
    });
    vm.def("wokwok::random", |a| {
        let b = ke_bentuk(arg!(a, 0, "wokwok::random"), "wokwok::random")?;
        let n: usize = b.iter().product();
        Ok(dari_nd(Nd::baru(Buf::F64((0..n).map(|_| acak_f64()).collect()), b)?))
    });
    vm.def("wokwok::seed", |a| {
        let s = ke_i64(arg!(a, 0, "wokwok::seed"), "wokwok::seed")?;
        *ACAK.lock().unwrap_or_else(|p| p.into_inner()) = s as u64;
        Ok(VmValue::Unit)
    });

    // ── Informasi ────────────────────────────────────────────────────────
    vm.def("wokwok::shape", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::shape"), "wokwok::shape")?;
        Ok(VmValue::array(x.shape.iter().map(|&d| VmValue::Int(d as i128)).collect()))
    });
    vm.def("wokwok::ndim", |a| {
        Ok(VmValue::Int(ke_nd(arg!(a, 0, "wokwok::ndim"), "wokwok::ndim")?.ndim() as i128))
    });
    vm.def("wokwok::size", |a| {
        Ok(VmValue::Int(ke_nd(arg!(a, 0, "wokwok::size"), "wokwok::size")?.size() as i128))
    });
    vm.def("wokwok::dtype", |a| {
        Ok(VmValue::str(ke_nd(arg!(a, 0, "wokwok::dtype"), "wokwok::dtype")?.dtype()))
    });
    vm.def("wokwok::to_list", |a| {
        Ok(ke_daftar(&*ke_nd(arg!(a, 0, "wokwok::to_list"), "wokwok::to_list")?))
    });

    // ── Bentuk ───────────────────────────────────────────────────────────
    vm.def("wokwok::reshape", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::reshape"), "wokwok::reshape")?;
        let b = match arg!(a, 1, "wokwok::reshape") {
            VmValue::Array(v) => {
                let isi = v.lock().unwrap_or_else(|p| p.into_inner());
                let mut out = Vec::with_capacity(isi.len());
                for e in isi.iter() { out.push(ke_i64(e, "wokwok::reshape")?); }
                out
            }
            lain => vec![ke_i64(lain, "wokwok::reshape")?],
        };
        Ok(dari_nd(x.reshape(&b).map_err(|e| format!("wokwok::{}", e))?))
    });
    vm.def("wokwok::flatten", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::flatten"), "wokwok::flatten")?;
        let n = x.size();
        Ok(dari_nd(Nd::baru(x.into_owned().buf, vec![n])?))
    });
    vm.def("wokwok::transpose", |a| {
        Ok(dari_nd(ops::transpose(&*ke_nd(arg!(a, 0, "wokwok::transpose"), "wokwok::transpose")?)?))
    });

    // ── Elemen-demi-elemen ───────────────────────────────────────────────
    macro_rules! biner {
        ($nama:literal, $op:expr) => {
            vm.def(concat!("wokwok::", $nama), |a| {
                let x = ke_nd(arg!(a, 0, concat!("wokwok::", $nama)), concat!("wokwok::", $nama))?;
                let y = ke_nd(arg!(a, 1, concat!("wokwok::", $nama)), concat!("wokwok::", $nama))?;
                Ok(dari_nd(ops::biner(&x, &y, $op)?))
            });
        };
    }
    biner!("add", Biner::Tambah);      biner!("sub", Biner::Kurang);
    biner!("mul", Biner::Kali);        biner!("div", Biner::Bagi);
    biner!("pow", Biner::Pangkat);     biner!("rem", Biner::Sisa);
    biner!("minimum", Biner::Min);     biner!("maximum", Biner::Maks);
    biner!("eq", Biner::Sama);         biner!("ne", Biner::Beda);
    biner!("lt", Biner::Kurang_);      biner!("le", Biner::KurangSama);
    biner!("gt", Biner::Lebih);        biner!("ge", Biner::LebihSama);
    biner!("and", Biner::Dan);         biner!("or", Biner::Atau);

    macro_rules! unari {
        ($nama:literal, $op:expr) => {
            vm.def(concat!("wokwok::", $nama), |a| {
                let x = ke_nd(arg!(a, 0, concat!("wokwok::", $nama)), concat!("wokwok::", $nama))?;
                Ok(dari_nd(ops::unari(&x, $op)?))
            });
        };
    }
    unari!("sqrt", Unari::Akar);   unari!("exp", Unari::Exp);
    unari!("log", Unari::Ln);      unari!("log2", Unari::Log2);
    unari!("log10", Unari::Log10); unari!("sin", Unari::Sin);
    unari!("cos", Unari::Cos);     unari!("tan", Unari::Tan);
    unari!("asin", Unari::Asin);   unari!("acos", Unari::Acos);
    unari!("atan", Unari::Atan);   unari!("sinh", Unari::Sinh);
    unari!("cosh", Unari::Cosh);   unari!("tanh", Unari::Tanh);
    unari!("abs", Unari::Mutlak);  unari!("floor", Unari::Lantai);
    unari!("ceil", Unari::Langit); unari!("round", Unari::Bulat);
    unari!("trunc", Unari::Potong);unari!("sign", Unari::Tanda);
    unari!("neg", Unari::Negasi);  unari!("reverse", Unari::Balik);
    unari!("not", Unari::BukanLogika);

    vm.def("wokwok::where", |a| {
        let k = ke_nd(arg!(a, 0, "wokwok::where"), "wokwok::where")?;
        let x = ke_nd(arg!(a, 1, "wokwok::where"), "wokwok::where")?;
        let y = ke_nd(arg!(a, 2, "wokwok::where"), "wokwok::where")?;
        Ok(dari_nd(ops::pilih(&k, &x, &y)?))
    });
    vm.def("wokwok::clip", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::clip"), "wokwok::clip")?;
        let lo = ke_f64(arg!(a, 1, "wokwok::clip"), "wokwok::clip")?;
        let hi = ke_f64(arg!(a, 2, "wokwok::clip"), "wokwok::clip")?;
        Ok(dari_nd(ops::jepit(&x, lo, hi)?))
    });

    // ── Reduksi ──────────────────────────────────────────────────────────
    macro_rules! reduksi {
        ($nama:literal, $r:expr) => {
            vm.def(concat!("wokwok::", $nama), |a| {
                let x = ke_nd(arg!(a, 0, concat!("wokwok::", $nama)), concat!("wokwok::", $nama))?;
                match a.get(1) {
                    None | Some(VmValue::Nil) => Ok(dari_nd_skalar(reduce::semua(&x, $r)?)),
                    Some(s) => {
                        let s = ke_i64(s, concat!("wokwok::", $nama))?;
                        Ok(dari_nd(reduce::per_sumbu(&x, $r, s)?))
                    }
                }
            });
        };
    }
    reduksi!("sum", Reduksi::Jumlah);     reduksi!("prod", Reduksi::Hasilkali);
    reduksi!("mean", Reduksi::Rata);      reduksi!("min", Reduksi::Min);
    reduksi!("max", Reduksi::Maks);       reduksi!("var", Reduksi::Ragam);
    reduksi!("std", Reduksi::Simpangan);  reduksi!("argmin", Reduksi::ArgMin);
    reduksi!("argmax", Reduksi::ArgMaks); reduksi!("any", Reduksi::Ada);
    reduksi!("all", Reduksi::Semua);      reduksi!("count", Reduksi::Cacah);

    // Jumlah TELITI. Lihat `reduce::jumlah_teliti` — ini pasangan sadar
    // dari `sum`, bukan duplikatnya: satu memilih kecepatan, satu memilih
    // digit terakhir, dan yang memakai boleh memilih.
    vm.def("wokwok::fsum", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::fsum"), "wokwok::fsum")?;
        let v = x.f64_pinjam();
        Ok(dari_nd_skalar(Nd::skalar_f64(reduce::jumlah_teliti(&v))))
    });
    vm.def("wokwok::fmean", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::fmean"), "wokwok::fmean")?;
        if x.size() == 0 {
            return Err("wokwok::fmean: array kosong tidak punya rata-rata".to_string());
        }
        let v = x.f64_pinjam();
        Ok(dari_nd_skalar(Nd::skalar_f64(reduce::jumlah_teliti(&v) / v.len() as f64)))
    });
    vm.def("wokwok::cumsum", |a| {
        Ok(dari_nd(reduce::kumulatif(&*ke_nd(arg!(a, 0, "wokwok::cumsum"), "wokwok::cumsum")?, false)?))
    });
    vm.def("wokwok::cumprod", |a| {
        Ok(dari_nd(reduce::kumulatif(&*ke_nd(arg!(a, 0, "wokwok::cumprod"), "wokwok::cumprod")?, true)?))
    });
    vm.def("wokwok::sort", |a| {
        Ok(dari_nd(reduce::urutkan(&*ke_nd(arg!(a, 0, "wokwok::sort"), "wokwok::sort")?)?))
    });
    vm.def("wokwok::argsort", |a| {
        Ok(dari_nd(reduce::urutan_indeks(&*ke_nd(arg!(a, 0, "wokwok::argsort"), "wokwok::argsort")?)?))
    });

    // ── Aljabar linear ───────────────────────────────────────────────────
    macro_rules! la1 {
        ($nama:literal, $f:path) => {
            vm.def(concat!("wokwok::", $nama), |a| {
                Ok(dari_nd_skalar($f(&*ke_nd(arg!(a, 0, concat!("wokwok::", $nama)),
                                     concat!("wokwok::", $nama))?)?))
            });
        };
    }
    macro_rules! la2 {
        ($nama:literal, $f:path) => {
            vm.def(concat!("wokwok::", $nama), |a| {
                let x = ke_nd(arg!(a, 0, concat!("wokwok::", $nama)), concat!("wokwok::", $nama))?;
                let y = ke_nd(arg!(a, 1, concat!("wokwok::", $nama)), concat!("wokwok::", $nama))?;
                Ok(dari_nd_skalar($f(&x, &y)?))
            });
        };
    }
    la2!("matmul", linalg::matmul);  la2!("dot", linalg::dot);
    la2!("outer", linalg::outer);    la2!("solve", linalg::solve);
    la1!("inv", linalg::inverse);    la1!("det", linalg::det);
    la1!("trace", linalg::trace);    la1!("norm", linalg::norm);

    // ── Akses elemen ─────────────────────────────────────────────────────
    vm.def("wokwok::get", |a| {
        let x = ke_nd(arg!(a, 0, "wokwok::get"), "wokwok::get")?;
        let mut koord = Vec::with_capacity(a.len() - 1);
        for v in &a[1..] { koord.push(ke_i64(v, "wokwok::get")?); }
        let i = x.indeks_rata(&koord, "wokwok::get")?;
        Ok(match &x.buf {
            Buf::F64(v) => VmValue::Float(v[i]),
            Buf::I64(v) => VmValue::Int(v[i] as i128),
            Buf::Bool(v) => VmValue::Bool(v[i]),
        })
    });
    vm.def("wokwok::set", |a| {
        // Menjawab array BARU, tidak mengubah yang lama — sama seperti `arr::push`
        // dan sama seperti seluruh JRG.
        if a.len() < 3 {
            return Err(format!(
                "wokwok::set butuh (array, indeks..., nilai) — minimal tiga argumen, dapat {}", a.len()));
        }
        let mut x = ke_nd(arg!(a, 0, "wokwok::set"), "wokwok::set")?.into_owned();
        let nilai = ke_f64(&a[a.len() - 1], "wokwok::set")?;
        let mut koord = Vec::with_capacity(a.len() - 2);
        for v in &a[1..a.len() - 1] { koord.push(ke_i64(v, "wokwok::set")?); }
        let i = x.indeks_rata(&koord, "wokwok::set")?;
        match &mut x.buf {
            Buf::F64(v) => v[i] = nilai,
            Buf::I64(v) => v[i] = nilai as i64,
            Buf::Bool(v) => v[i] = nilai != 0.0,
        }
        Ok(dari_nd(x))
    });
}
