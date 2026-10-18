// src/suara/mod.rs — `suara::`: bunyi sebagai larik `wokwok::` mono 44100 Hz.
// Campuran dan volume memakai `wokwok::add`/`mul` yang sudah ada; di sini cuma
// pembangkit, berkas WAV, dan pemutar (`libpulse-simple`, di Android AAudio; lewat dlopen).

#[cfg(not(target_os = "android"))]
pub mod pemutar;
#[cfg(target_os = "android")]
#[path = "pemutar_android.rs"]
pub mod pemutar;

use std::sync::Arc;

use crate::vm::value::VmValue;
use crate::vm::vm::Vm;
use crate::wokwok::{Buf, Nd};

/// Sampel per detik.
pub const LAJU: u32 = 44100;
/// Bunyi terpanjang yang boleh dibangkitkan: sepuluh menit. Angkanya dari
/// program, jadi dibatasi SEBELUM dialokasikan (pelajaran `arr::fill`).
const MAKS_MS: f64 = 600_000.0;

fn angka(v: Option<&VmValue>, nama: &str, ke: usize) -> Result<f64, String> {
    match v {
        Some(VmValue::Int(n)) => Ok(*n as f64),
        Some(VmValue::Float(x)) if x.is_finite() => Ok(*x),
        Some(VmValue::Float(x)) => Err(format!("{}: argumen ke-{} harus angka terhingga, dapat {}", nama, ke, x)),
        Some(lain) => Err(format!("{}: argumen ke-{} harus angka, dapat `{}`", nama, ke, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke)),
    }
}

fn teks(v: Option<&VmValue>, nama: &str, ke: usize) -> Result<String, String> {
    match v {
        Some(VmValue::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus teks, dapat `{}`", nama, ke, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke)),
    }
}

/// Bunyi dari argumen: larik `wokwok::` 1-D (bilangan apa pun, dibaca sebagai f64).
fn bunyi(v: Option<&VmValue>, nama: &str, ke: usize) -> Result<Vec<f64>, String> {
    match v {
        Some(VmValue::Nd(nd)) if nd.shape.len() == 1 => Ok(nd.buf.ke_f64()),
        Some(VmValue::Nd(nd)) => Err(format!(
            "{}: argumen ke-{} harus bunyi 1-D (satu sampel per elemen), dapat larik berbentuk {:?}",
            nama, ke, nd.shape)),
        Some(lain) => Err(format!(
            "{}: argumen ke-{} harus bunyi (larik `wokwok::` dari `suara::nada` dan kawan-kawan), dapat `{}`",
            nama, ke, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke)),
    }
}

fn jadi(sampel: Vec<f64>) -> VmValue { VmValue::Nd(Arc::new(Nd::vektor(Buf::F64(sampel)))) }

/// Jumlah sampel untuk `ms` milidetik; negatif dan yang kelewat panjang ditolak.
fn jumlah_sampel(ms: f64, nama: &str) -> Result<usize, String> {
    if ms < 0.0 { return Err(format!("{}: durasi tidak boleh negatif, dapat {} ms", nama, ms)) }
    if ms > MAKS_MS {
        return Err(format!("{}: durasi {} ms melewati batas {} ms (sepuluh menit)", nama, ms, MAKS_MS))
    }
    Ok((ms * LAJU as f64 / 1000.0).round() as usize)
}

/// Gelombang bentuk `bentuk` berfrekuensi `frek` Hz, fase mulai dari nol.
pub fn nada(frek: f64, ms: f64, bentuk: &str) -> Result<Vec<f64>, String> {
    if frek <= 0.0 || frek > LAJU as f64 / 2.0 {
        return Err(format!(
            "suara::nada: frekuensi harus di atas 0 dan paling tinggi {} Hz (separuh laju sampel), dapat {}",
            LAJU / 2, frek))
    }
    let n = jumlah_sampel(ms, "suara::nada")?;
    let f: fn(f64) -> f64 = match bentuk {
        "sinus" => |p| (2.0 * std::f64::consts::PI * p).sin(),
        "kotak" => |p| if p < 0.5 { 1.0 } else { -1.0 },
        "gergaji" => |p| 2.0 * p - 1.0,
        "segitiga" => |p| if p < 0.5 { 4.0 * p - 1.0 } else { 3.0 - 4.0 * p },
        lain => return Err(format!(
            "suara::nada: bentuk `{}` tidak dikenal — yang ada: sinus, kotak, gergaji, segitiga", lain)),
    };
    // Fase dihitung dari indeks, bukan dijumlahkan, supaya galat pembulatan tidak menumpuk.
    Ok((0..n).map(|i| f((i as f64 * frek / LAJU as f64).fract())).collect())
}

/// Derau putih dari LCG berbenih (Park-Miller): sama tiap jalan, sama di dua mesin.
pub fn derau(ms: f64, benih: i128) -> Result<Vec<f64>, String> {
    let n = jumlah_sampel(ms, "suara::derau")?;
    let mut s = (benih.rem_euclid(2147483646) + 1) as u64;
    Ok((0..n).map(|_| {
        s = s * 48271 % 2147483647;
        s as f64 / 2147483647.0 * 2.0 - 1.0
    }).collect())
}

/// Naik linear selama `serang` sampel, turun linear selama `lepas` sampel.
pub fn selubung(mut b: Vec<f64>, serang_ms: f64, lepas_ms: f64) -> Result<Vec<f64>, String> {
    let s = jumlah_sampel(serang_ms, "suara::selubung")?;
    let l = jumlah_sampel(lepas_ms, "suara::selubung")?;
    if s + l > b.len() {
        return Err(format!(
            "suara::selubung: serang {} ms + lepas {} ms lebih panjang dari bunyinya ({} ms)",
            serang_ms, lepas_ms, b.len() as f64 * 1000.0 / LAJU as f64))
    }
    let n = b.len();
    for i in 0..s { b[i] *= i as f64 / s as f64; }
    for i in 0..l { b[n - 1 - i] *= i as f64 / l as f64; }
    Ok(b)
}

/// WAV PCM 16-bit mono. Sampel di luar -1..1 DIJEPIT (kliping) — perilaku baku audio.
pub fn wav(b: &[f64]) -> Vec<u8> {
    let data = (b.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&LAJU.to_le_bytes());
    v.extend_from_slice(&(LAJU * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for x in pcm16(b) { v.extend_from_slice(&x.to_le_bytes()); }
    v
}

pub fn pcm16(b: &[f64]) -> Vec<i16> {
    b.iter().map(|&x| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect()
}

/// Baca WAV PCM 16-bit (mono, atau stereo yang dirata-rata) ber-laju 44100 Hz.
pub fn baca_wav(d: &[u8], nama: &str) -> Result<Vec<f64>, String> {
    let rusak = |s: &str| format!("suara::muat_wav: `{}` bukan WAV yang bisa dibaca — {}", nama, s);
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return Err(rusak("tidak diawali RIFF/WAVE"))
    }
    let u16_ = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
    let u32_ = |i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
    let (mut kanal, mut fmt_ok, mut i) = (0u16, false, 12usize);
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let ukuran = u32_(i + 4) as usize;
        let isi = i + 8;
        if isi + ukuran > d.len() { return Err(rusak("potongannya terpotong")) }
        if id == b"fmt " {
            if ukuran < 16 { return Err(rusak("potongan fmt terlalu pendek")) }
            let (format, k, laju, bit) = (u16_(isi), u16_(isi + 2), u32_(isi + 4), u16_(isi + 14));
            if format != 1 || bit != 16 {
                return Err(rusak(&format!("cuma PCM 16-bit yang didukung, berkas ini format {} {}-bit", format, bit)))
            }
            if laju != LAJU {
                return Err(rusak(&format!("laju sampelnya {} Hz, yang didukung {} Hz", laju, LAJU)))
            }
            if k != 1 && k != 2 { return Err(rusak(&format!("{} kanal — yang didukung mono atau stereo", k))) }
            kanal = k;
            fmt_ok = true;
        } else if id == b"data" {
            if !fmt_ok { return Err(rusak("potongan data datang sebelum fmt")) }
            let s: Vec<f64> = d[isi..isi + ukuran].chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f64 / 32767.0).collect();
            if kanal == 1 { return Ok(s) }
            return Ok(s.chunks_exact(2).map(|p| (p[0] + p[1]) / 2.0).collect())
        }
        i = isi + ukuran + (ukuran & 1);
    }
    Err(rusak("tidak ada potongan data"))
}

fn ok_unit() -> VmValue { VmValue::varian("Result", "Ok", Some(VmValue::Unit)) }
fn err_teks(s: String) -> VmValue { VmValue::varian("Result", "Err", Some(VmValue::Str(Arc::new(s)))) }

pub fn register(vm: &mut Vm) {
    vm.def("suara::nada", |a| {
        let frek = angka(a.get(0), "suara::nada", 1)?;
        let ms = angka(a.get(1), "suara::nada", 2)?;
        let bentuk = match a.get(2) { None => "sinus".to_string(), v => teks(v, "suara::nada", 3)? };
        Ok(jadi(nada(frek, ms, &bentuk)?))
    });
    vm.def("suara::derau", |a| {
        let ms = angka(a.get(0), "suara::derau", 1)?;
        let benih = match a.get(1) {
            Some(VmValue::Int(n)) => *n,
            Some(lain) => return Err(format!("suara::derau: benih harus bilangan bulat, dapat `{}`", lain.type_name())),
            None => return Err("suara::derau: argumen ke-2 (benih) wajib diisi".to_string()),
        };
        Ok(jadi(derau(ms, benih)?))
    });
    vm.def("suara::sunyi", |a| {
        let n = jumlah_sampel(angka(a.get(0), "suara::sunyi", 1)?, "suara::sunyi")?;
        Ok(jadi(vec![0.0; n]))
    });
    vm.def("suara::selubung", |a| {
        let b = bunyi(a.get(0), "suara::selubung", 1)?;
        let s = angka(a.get(1), "suara::selubung", 2)?;
        let l = angka(a.get(2), "suara::selubung", 3)?;
        Ok(jadi(selubung(b, s, l)?))
    });
    vm.def("suara::sambung", |a| {
        let mut x = bunyi(a.get(0), "suara::sambung", 1)?;
        x.extend(bunyi(a.get(1), "suara::sambung", 2)?);
        Ok(jadi(x))
    });
    vm.def("suara::durasi_ms", |a| {
        Ok(VmValue::Float(bunyi(a.get(0), "suara::durasi_ms", 1)?.len() as f64 * 1000.0 / LAJU as f64))
    });
    vm.def("suara::simpan_wav", |a| {
        let b = bunyi(a.get(0), "suara::simpan_wav", 1)?;
        let p = teks(a.get(1), "suara::simpan_wav", 2)?;
        Ok(match std::fs::write(&p, wav(&b)) {
            Ok(()) => ok_unit(),
            Err(e) => err_teks(format!("suara::simpan_wav: tidak bisa menulis `{}`: {}", p, crate::host::galat_io(&e))),
        })
    });
    vm.def("suara::muat_wav", |a| {
        let p = teks(a.get(0), "suara::muat_wav", 1)?;
        Ok(match std::fs::read(&p) {
            Ok(d) => match baca_wav(&d, &p) {
                Ok(s) => VmValue::varian("Result", "Ok", Some(jadi(s))),
                Err(e) => err_teks(e),
            },
            Err(e) => err_teks(format!("suara::muat_wav: tidak bisa membaca `{}`: {}", p, crate::host::galat_io(&e))),
        })
    });
    vm.def("suara::tersedia", |a| {
        if !a.is_empty() { return Err(format!("suara::tersedia: tidak butuh argumen, dapat {}", a.len())) }
        Ok(VmValue::Bool(pemutar::tersedia()))
    });
    vm.def("suara::putar_tunggu", |a| {
        let b = bunyi(a.get(0), "suara::putar_tunggu", 1)?;
        Ok(match pemutar::putar_tunggu(pcm16(&b)) {
            Ok(()) => ok_unit(),
            Err(e) => err_teks(e),
        })
    });
    vm.def("suara::putar", |a| {
        let b = bunyi(a.get(0), "suara::putar", 1)?;
        Ok(match pemutar::putar(pcm16(&b)) {
            Ok(()) => ok_unit(),
            Err(e) => err_teks(e),
        })
    });
}
