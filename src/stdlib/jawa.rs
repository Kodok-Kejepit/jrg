// src/stdlib/jawa.rs
// Namespace `jawa::` / `jawir::` untuk backend VM.

use crate::vm::value::VmValue;
use crate::vm::vm::Vm;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Isi sebuah alokasi. Varian membedakan tiga bentuk yang punya operasi
/// berbeda, supaya `jawa::array_get` pada alokasi `single` gagal dengan
/// jujur alih-alih menebak.
#[derive(Clone)]
enum Isi {
    Tunggal(VmValue),
    Larik(Vec<VmValue>),
    Bufer(Vec<u8>),
}

struct Alokasi {
    isi: Isi,
    /// `false` sesudah `jawir::free`. Entri TIDAK dihapus, supaya
    /// use-after-free dan double-free bisa dibedakan dari "id ngawur".
    hidup: bool,
}

static REGISTRI: OnceLock<Mutex<HashMap<u64, Alokasi>>> = OnceLock::new();
static ID_BERIKUT: AtomicU64 = AtomicU64::new(1);
static TOTAL_ALLOC: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREE: AtomicU64 = AtomicU64::new(0);

fn registri() -> &'static Mutex<HashMap<u64, Alokasi>> {
    REGISTRI.get_or_init(|| Mutex::new(HashMap::new()))
}

fn handle(id: u64, kind: &str) -> VmValue {
    let mut m = HashMap::new();
    m.insert("id".to_string(), VmValue::Int(id as i128));
    m.insert("kind".to_string(), VmValue::str(kind));
    VmValue::struktur("Ptr", m)
}

use crate::interpreter::builtin::heap as acuan;

/// Handle `Ptr` beserta jenisnya. Urutan pemeriksaan dan kalimatnya milik
/// `interpreter::builtin::heap`: registrinya terpisah per mesin, aturannya tidak.
fn pegangan(v: Option<&VmValue>, f: &str) -> Result<(u64, String), String> {
    let v = v.unwrap_or(&VmValue::Nil);
    if let (Some("Ptr"), VmValue::Map(m)) = (v.nama_struct(), v) {
        let g = m.lock().unwrap_or_else(|p| p.into_inner());
        let id = match g.get("id") {
            Some(VmValue::Int(n)) => u64::try_from(*n)
                .map_err(|_| format!("{}: handle tidak valid (`id` {} bukan nomor alokasi)", f, n))?,
            _ => return Err(format!("{}: handle tidak valid (field `id` hilang)", f)),
        };
        let kind = match g.get("kind") {
            Some(VmValue::Str(s)) => s.to_string(),
            _ => return Err(format!("{}: handle tidak valid (field `kind` hilang)", f)),
        };
        return Ok((id, kind));
    }
    Err(acuan::pesan_bukan_ptr(f, &v.type_name_penuh()))
}

fn pegangan_jenis(v: Option<&VmValue>, mau: &str, f: &str) -> Result<u64, String> {
    let (id, kind) = pegangan(v, f)?;
    if kind != mau {
        return Err(acuan::pesan_jenis_salah(f, &kind, mau));
    }
    Ok(id)
}

/// Kerjakan atas alokasi yang MASIH HIDUP, atau jelaskan kenapa tidak bisa.
fn hidup<T>(id: u64, jenis: &str, f: &str, kerja: impl FnOnce(&mut Isi) -> Result<T, String>) -> Result<T, String> {
    let mut reg = registri().lock().unwrap_or_else(|p| p.into_inner());
    match reg.get_mut(&id) {
        Some(a) if a.hidup => kerja(&mut a.isi),
        Some(_) => Err(acuan::pesan_sudah_bebas(f, jenis)),
        // Cuma handle yang DIKARANG (struct `Ptr` tulisan tangan) sampai ke sini.
        None => Err(format!("{}: pointer tidak dikenal (id {})", f, id)),
    }
}

fn simpan(isi: Isi, kind: &str) -> VmValue {
    let id = ID_BERIKUT.fetch_add(1, Ordering::SeqCst);
    registri()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(id, Alokasi { isi, hidup: true });
    TOTAL_ALLOC.fetch_add(1, Ordering::Relaxed);
    handle(id, kind)
}

fn bulat(v: Option<&VmValue>, f: &str) -> Result<i128, String> {
    match v {
        Some(VmValue::Int(n)) => Ok(*n),
        Some(VmValue::Float(x)) => Err(crate::interpreter::pesan_bukan_bulat(
            f, "float", Some(&VmValue::Float(*x).to_string()))),
        Some(o) => Err(crate::interpreter::pesan_bukan_bulat(f, &o.type_name_penuh(), None)),
        None => Err(format!("{}: kurang argumen", f)),
    }
}

fn byte_dari(v: Option<&VmValue>, f: &str) -> Result<u8, String> {
    let n = bulat(v, f)?;
    if !(0..=255).contains(&n) {
        return Err(acuan::pesan_byte_luar(f, n));
    }
    Ok(n as u8)
}

/// Ukuran sebuah nilai VM, dihitung fungsi yang SAMA dengan interpreter.
fn ukuran(v: &VmValue) -> Result<usize, String> {
    let nilai = crate::vm::ffi_jembatan::TitipanVm::default().ke_interp(v)?;
    Ok(acuan::value_size_estimate(&nilai))
}

pub fn register(vm: &mut Vm) {
    vm.def("jawa::alloc", |a| {
        Ok(simpan(Isi::Tunggal(a.get(0).cloned().unwrap_or(VmValue::Nil)), "single"))
    });
    // `jawa`/`jawir` telanjang adalah ALIAS (tabel alias di `vm/bawaan.rs`),
    // bukan salinan — satu definisi, satu kalimat galat.
    vm.def("jawa::get", |a| {
        const F: &str = "jawa::get";
        let id = pegangan_jenis(a.get(0), "single", F)?;
        hidup(id, "single", F, |isi| match isi {
            Isi::Tunggal(v) => Ok(v.clone()),
            _ => Err(format!("{}: alokasi ini bukan nilai tunggal", F)),
        })
    });
    vm.def("jawa::set", |a| {
        const F: &str = "jawa::set";
        let id = pegangan_jenis(a.get(0), "single", F)?;
        let baru = a.get(1).cloned().unwrap_or(VmValue::Nil);
        hidup(id, "single", F, |isi| match isi {
            Isi::Tunggal(v) => { *v = baru; Ok(VmValue::Unit) }
            _ => Err(format!("{}: alokasi ini bukan nilai tunggal", F)),
        })
    });
    // `is_valid` sengaja TIDAK memakai `hidup`: pointer yang sudah bebas
    // bukan error di sini — justru itu yang mau ditanyakan.
    vm.def("jawa::is_valid", |a| {
        let Ok((id, _)) = pegangan(a.get(0), "jawa::is_valid") else {
            return Ok(VmValue::Bool(false));
        };
        let reg = registri().lock().unwrap_or_else(|p| p.into_inner());
        Ok(VmValue::Bool(reg.get(&id).map(|x| x.hidup).unwrap_or(false)))
    });
    // Handle dilihat di registri MILIK VM, tapi ukurannya dihitung seperti interpreter:
    // dulu `16`/`len*16` di sini lawan perkiraan isinya di sana, dan ukuran alokasi yang
    // sudah dibebaskan dijawab angka alih-alih ditolak.
    vm.def("jawa::sizeof", |a| {
        const F: &str = "jawa::sizeof";
        let Some(v) = a.get(0) else {
            return Err(format!("{}: butuh satu argumen", F));
        };
        let Ok((id, kind)) = pegangan(Some(v), F) else {
            return Ok(VmValue::Int(ukuran(v)? as i128));
        };
        hidup(id, &kind, F, |isi| {
            let n = match isi {
                Isi::Tunggal(v) => ukuran(v)?,
                Isi::Larik(v) => v.iter().map(ukuran).sum::<Result<usize, String>>()?,
                Isi::Bufer(b) => b.len(),
            };
            Ok(VmValue::Int(n as i128))
        })
    });
    // Bentuk dan nama fieldnya disamakan PERSIS dengan interpreter (`HeapStats {
    // alive_array, alive_buffer, alive_single, alive_total, total_allocated …
    vm.def("jawa::stats", |_| {
        let reg = registri().lock().unwrap_or_else(|p| p.into_inner());
        let (mut single, mut array, mut buffer) = (0i128, 0i128, 0i128);
        for a in reg.values().filter(|a| a.hidup) {
            match a.isi {
                Isi::Tunggal(_) => single += 1,
                Isi::Larik(_) => array += 1,
                Isi::Bufer(_) => buffer += 1,
            }
        }
        let mut m = HashMap::new();
        m.insert("alive_array".to_string(), VmValue::Int(array));
        m.insert("alive_buffer".to_string(), VmValue::Int(buffer));
        m.insert("alive_single".to_string(), VmValue::Int(single));
        m.insert("alive_total".to_string(), VmValue::Int(single + array + buffer));
        m.insert("total_allocated".to_string(), VmValue::Int(TOTAL_ALLOC.load(Ordering::Relaxed) as i128));
        m.insert("total_freed".to_string(), VmValue::Int(TOTAL_FREE.load(Ordering::Relaxed) as i128));
        Ok(VmValue::struktur("HeapStats", m))
    });

    // ── array ───────────────────────────────────────────────────────────
    vm.def("jawa::array", |a| {
        let n = acuan::cek_panjang("jawa::array", bulat(a.get(0), "jawa::array")?)?;
        let isi = a.get(1).cloned().unwrap_or(VmValue::Nil);
        Ok(simpan(Isi::Larik(vec![isi; n]), "array"))
    });
    vm.def("jawa::array_len", |a| {
        const F: &str = "jawa::array_len";
        let id = pegangan_jenis(a.get(0), "array", F)?;
        hidup(id, "array", F, |isi| match isi {
            Isi::Larik(v) => Ok(VmValue::Int(v.len() as i128)),
            _ => Err(format!("{}: alokasi ini bukan array", F)),
        })
    });
    vm.def("jawa::array_get", |a| {
        const F: &str = "jawa::array_get";
        let id = pegangan_jenis(a.get(0), "array", F)?;
        let i = bulat(a.get(1), F)?;
        hidup(id, "array", F, |isi| match isi {
            Isi::Larik(v) => Ok(v[acuan::cek_index(F, i, v.len())?].clone()),
            _ => Err(format!("{}: alokasi ini bukan array", F)),
        })
    });
    vm.def("jawa::array_set", |a| {
        const F: &str = "jawa::array_set";
        let id = pegangan_jenis(a.get(0), "array", F)?;
        let i = bulat(a.get(1), F)?;
        let baru = a.get(2).cloned().unwrap_or(VmValue::Nil);
        hidup(id, "array", F, |isi| match isi {
            Isi::Larik(v) => {
                let k = acuan::cek_index(F, i, v.len())?;
                v[k] = baru;
                Ok(VmValue::Unit)
            }
            _ => Err(format!("{}: alokasi ini bukan array", F)),
        })
    });
    vm.def("jawa::array_push", |a| {
        const F: &str = "jawa::array_push";
        let id = pegangan_jenis(a.get(0), "array", F)?;
        let baru = a.get(1).cloned().unwrap_or(VmValue::Nil);
        hidup(id, "array", F, |isi| match isi {
            Isi::Larik(v) => { v.push(baru); Ok(VmValue::Unit) }
            _ => Err(format!("{}: alokasi ini bukan array", F)),
        })
    });
    vm.def("jawa::array_pop", |a| {
        const F: &str = "jawa::array_pop";
        let id = pegangan_jenis(a.get(0), "array", F)?;
        hidup(id, "array", F, |isi| match isi {
            Isi::Larik(v) => v.pop().ok_or_else(|| format!("{}: array kosong", F)),
            _ => Err(format!("{}: alokasi ini bukan array", F)),
        })
    });
    vm.def("jawa::array_to_list", |a| {
        const F: &str = "jawa::array_to_list";
        let id = pegangan_jenis(a.get(0), "array", F)?;
        hidup(id, "array", F, |isi| match isi {
            Isi::Larik(v) => Ok(VmValue::array(v.clone())),
            _ => Err(format!("{}: alokasi ini bukan array", F)),
        })
    });

    // ── buffer ──────────────────────────────────────────────────────────
    vm.def("jawa::buffer", |a| {
        let n = acuan::cek_panjang("jawa::buffer", bulat(a.get(0), "jawa::buffer")?)?;
        Ok(simpan(Isi::Bufer(vec![0u8; n]), "buffer"))
    });
    vm.def("jawa::buffer_len", |a| {
        const F: &str = "jawa::buffer_len";
        let id = pegangan_jenis(a.get(0), "buffer", F)?;
        hidup(id, "buffer", F, |isi| match isi {
            Isi::Bufer(b) => Ok(VmValue::Int(b.len() as i128)),
            _ => Err(format!("{}: alokasi ini bukan buffer", F)),
        })
    });
    vm.def("jawa::buffer_get", |a| {
        const F: &str = "jawa::buffer_get";
        let id = pegangan_jenis(a.get(0), "buffer", F)?;
        let i = bulat(a.get(1), F)?;
        hidup(id, "buffer", F, |isi| match isi {
            Isi::Bufer(b) => Ok(VmValue::Int(b[acuan::cek_index(F, i, b.len())?] as i128)),
            _ => Err(format!("{}: alokasi ini bukan buffer", F)),
        })
    });
    vm.def("jawa::buffer_set", |a| {
        const F: &str = "jawa::buffer_set";
        let id = pegangan_jenis(a.get(0), "buffer", F)?;
        let i = bulat(a.get(1), F)?;
        let nilai = byte_dari(a.get(2), F)?;
        hidup(id, "buffer", F, |isi| match isi {
            Isi::Bufer(b) => {
                let k = acuan::cek_index(F, i, b.len())?;
                b[k] = nilai;
                Ok(VmValue::Unit)
            }
            _ => Err(format!("{}: alokasi ini bukan buffer", F)),
        })
    });
    vm.def("jawa::buffer_fill", |a| {
        const F: &str = "jawa::buffer_fill";
        let id = pegangan_jenis(a.get(0), "buffer", F)?;
        let nilai = byte_dari(a.get(1), F)?;
        hidup(id, "buffer", F, |isi| match isi {
            Isi::Bufer(b) => { b.iter_mut().for_each(|x| *x = nilai); Ok(VmValue::Unit) }
            _ => Err(format!("{}: alokasi ini bukan buffer", F)),
        })
    });
    vm.def("jawa::buffer_to_hex", |a| {
        const F: &str = "jawa::buffer_to_hex";
        let id = pegangan_jenis(a.get(0), "buffer", F)?;
        hidup(id, "buffer", F, |isi| match isi {
            Isi::Bufer(b) => Ok(VmValue::str(crate::dok::crypto::hex_encode(b))),
            _ => Err(format!("{}: alokasi ini bukan buffer", F)),
        })
    });
    // Satu definisi dengan interpreter: salinan di sini dulu memotong teksnya per
    // BYTE, jadi `jawa::buffer_from_hex("aéb")` MEMATIKAN prosesnya.
    vm.def("jawa::buffer_from_hex", |a| {
        let isi = acuan::buffer_dari_hex(a.get(0).map(|v| match v {
            VmValue::Str(s) => Ok(s.as_str()),
            o => Err(o.type_name_penuh()),
        }))?;
        Ok(simpan(Isi::Bufer(isi), "buffer"))
    });

    // ── pembebasan ──────────────────────────────────────────────────────
    vm.def("jawir::free", |a| {
        let (id, _) = pegangan(a.get(0), "jawir::free")?;
        let mut reg = registri().lock().unwrap_or_else(|p| p.into_inner());
        match reg.get_mut(&id) {
            None => Err(format!("jawir::free: pointer tidak dikenal (id {})", id)),
            Some(al) if !al.hidup => Err(acuan::PESAN_DOUBLE_FREE.to_string()),
            Some(al) => {
                al.hidup = false;
                TOTAL_FREE.fetch_add(1, Ordering::Relaxed);
                Ok(VmValue::Unit)
            }
        }
    });
}
