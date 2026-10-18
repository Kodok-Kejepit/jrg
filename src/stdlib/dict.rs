// src/stdlib/dict.rs
// Namespace `dict::` untuk backend VM.

use crate::interpreter::{
    cek_dict_kurang, pesan_arg_harus_array, pesan_arg_harus_dict, pesan_bukan_pasangan,
    pesan_butuh_nilai, pesan_dict_kurang, pesan_pasangan_bukan_dua,
};
use crate::vm::value::VmValue;
use crate::vm::vm::Vm;
use std::collections::HashMap;

/// Isi dict dari argumen ke-`ke` (mulai 1). Struct dan enum juga `Map` di VM,
/// jadi yang diterima cuma yang tipenya `dict` — tanpa itu
/// `dict::keys(Option::Some(1))` menjawab kunci internalnya.
fn isi(v: Option<&VmValue>, ke: usize, fname: &str) -> Result<HashMap<String, VmValue>, String> {
    let Some(d) = v else { return Err(pesan_dict_kurang(fname)) };
    // Tipe dibaca dari label, tanpa mengunci peta.
    let tipe = d.type_name_penuh();
    match d {
        VmValue::Map(m) if tipe == "dict" => Ok(m.lock().unwrap_or_else(|p| p.into_inner()).clone()),
        _ => Err(pesan_arg_harus_dict(fname, ke, &tipe)),
    }
}

/// Baca isi sebuah dict TANPA menyalinnya — `isi` menyalin seluruh peta, jadi `dict::has`
/// dulu O(n) per panggilan.
fn pinjam_isi<R>(v: Option<&VmValue>, ke: usize, fname: &str, baca: impl FnOnce(&HashMap<String, VmValue>) -> R) -> Result<R, String> {
    let Some(d) = v else { return Err(pesan_dict_kurang(fname)) };
    let tipe = d.type_name_penuh();
    match d {
        VmValue::Map(m) if tipe == "dict" => Ok(baca(&m.lock().unwrap_or_else(|p| p.into_inner()))),
        _ => Err(pesan_arg_harus_dict(fname, ke, &tipe)),
    }
}

/// Dict yang cuma dipegang argumen ini (`d = dict::set(d, k, v)` sesudah slotnya dikosongkan):
/// mengubahnya di tempat tidak bisa diamati siapa pun, jadi builtin-nya tetap murni.
fn milik_tunggal(v: Option<&VmValue>) -> Option<&std::sync::Arc<crate::vm::value::Peta>> {
    match v {
        Some(VmValue::Map(m)) if std::sync::Arc::strong_count(m) == 1 && v?.type_name_penuh() == "dict" => Some(m),
        _ => None,
    }
}

fn kunci(v: Option<&VmValue>, fname: &str) -> Result<String, String> {
    match v {
        Some(VmValue::Str(s)) => Ok(s.to_string()),
        // Kunci non-teks diterima dengan diformat, sama seperti interpreter
        // memperlakukan `d[1]`.
        Some(other) => Ok(format!("{}", other)),
        None => Err(pesan_dict_kurang(fname)),
    }
}

fn jadi_dict(m: HashMap<String, VmValue>) -> VmValue {
    VmValue::map(m)
}

/// Kunci yang terurut — dipakai semua fungsi yang mengembalikan urutan.
fn kunci_terurut(m: &HashMap<String, VmValue>) -> Vec<String> {
    let mut k: Vec<String> = m.keys().cloned().collect();
    k.sort();
    k
}

// Salinan ketiga `Option::Some`/`None` dulu ada di sini; sekarang satu, di
// `VmValue::some`/`none`.
fn vm_some(v: VmValue) -> VmValue { VmValue::some(v) }
fn vm_none() -> VmValue { VmValue::none() }

pub fn register(vm: &mut Vm) {
    // Literal `{"a": 1}` diturunkan parser jadi `dict::from_pairs([[k, v], [k2,
    // v2], ...])` — SATU argumen berupa array pasangan, `{}` pun `[]`.
    vm.def("dict::from_pairs", |a| {
        cek_dict_kurang("dict::from_pairs", a.len())?;
        let daftar = match &a[0] {
            VmValue::Array(arr) => arr.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            lain => return Err(pesan_arg_harus_array("dict::from_pairs", 1, &lain.type_name_penuh())),
        };
        let mut m = HashMap::new();
        for (i, pasangan) in daftar.iter().enumerate() {
            let bagian: Vec<VmValue> = match pasangan {
                VmValue::Array(p) => p.lock().unwrap_or_else(|q| q.into_inner()).clone(),
                VmValue::Tuple(t) => crate::vm::value::isi_tuple(t),
                lain => return Err(pesan_bukan_pasangan("dict::from_pairs", i, &lain.type_name_penuh())),
            };
            if bagian.len() != 2 {
                return Err(pesan_pasangan_bukan_dua("dict::from_pairs", i, bagian.len()));
            }
            m.insert(kunci(bagian.get(0), "dict::from_pairs")?, bagian[1].clone());
        }
        Ok(jadi_dict(m))
    });

    vm.def("dict::new", |_| Ok(jadi_dict(HashMap::new())));

    vm.def("dict::get", |a| {
        cek_dict_kurang("dict::get", a.len())?;
        pinjam_isi(a.get(0), 1, "dict::get", |_| ())?;
        let k = kunci(a.get(1), "dict::get")?;
        // Mengembalikan Option, bukan nil — sama seperti interpreter:
        // apa pun yang mungkin tidak ketemu selalu Option.
        Ok(pinjam_isi(a.get(0), 1, "dict::get", |m| m.get(&k).cloned())?.map(vm_some).unwrap_or_else(vm_none))
    });

    vm.def("dict::get_or", |a| {
        // Cadangannya WAJIB: dulu `nil` dikarang untuknya, jadi
        // `dict::get_or(d, "a")` menjawab tanpa cadangan yang pernah ditulis.
        cek_dict_kurang("dict::get_or", a.len())?;
        pinjam_isi(a.get(0), 1, "dict::get_or", |_| ())?;
        let k = kunci(a.get(1), "dict::get_or")?;
        Ok(pinjam_isi(a.get(0), 1, "dict::get_or", |m| m.get(&k).cloned())?.unwrap_or_else(|| a[2].clone()))
    });

    vm.def("dict::has", |a| {
        cek_dict_kurang("dict::has", a.len())?;
        pinjam_isi(a.get(0), 1, "dict::has", |_| ())?;
        let k = kunci(a.get(1), "dict::has")?;
        Ok(VmValue::Bool(pinjam_isi(a.get(0), 1, "dict::has", |m| m.contains_key(&k))?))
    });

    vm.def("dict::len", |a| {
        cek_dict_kurang("dict::len", a.len())?;
        Ok(VmValue::Int(pinjam_isi(a.get(0), 1, "dict::len", |m| m.len())? as i128))
    });

    vm.def("dict::is_empty", |a| {
        cek_dict_kurang("dict::is_empty", a.len())?;
        Ok(VmValue::Bool(pinjam_isi(a.get(0), 1, "dict::is_empty", |m| m.is_empty())?))
    });

    vm.def("dict::is_dict", |a| match a.get(0) {
        // Struct dan enum juga `Map`; yang membedakannya tipenya, satu definisi.
        Some(v) => Ok(VmValue::Bool(v.type_name_penuh() == "dict")),
        None => Err(pesan_butuh_nilai("dict::is_dict")),
    });

    vm.def("dict::keys", |a| {
        cek_dict_kurang("dict::keys", a.len())?;
        let m = isi(a.get(0), 1, "dict::keys")?;
        Ok(VmValue::array(
            kunci_terurut(&m).into_iter().map(VmValue::str).collect(),
        ))
    });

    vm.def("dict::values", |a| {
        cek_dict_kurang("dict::values", a.len())?;
        let m = isi(a.get(0), 1, "dict::values")?;
        Ok(VmValue::array(
            kunci_terurut(&m).iter().map(|k| m[k].clone()).collect(),
        ))
    });

    vm.def("dict::items", |a| {
        cek_dict_kurang("dict::items", a.len())?;
        let m = isi(a.get(0), 1, "dict::items")?;
        Ok(VmValue::array(
            kunci_terurut(&m)
                .into_iter()
                .map(|k| {
                    let v = m[&k].clone();
                    VmValue::tuple(vec![VmValue::str(k), v])
                })
                .collect(),
        ))
    });

    // set/remove/merge MENGEMBALIKAN dict baru; yang asal tidak berubah.
    vm.def("dict::set", |a| {
        cek_dict_kurang("dict::set", a.len())?;
        if let Some(d) = milik_tunggal(a.get(0)) {
            let k = kunci(a.get(1), "dict::set")?;
            d.lock().unwrap_or_else(|p| p.into_inner()).insert(k, a[2].clone());
            return Ok(a[0].clone());
        }
        let mut m = isi(a.get(0), 1, "dict::set")?;
        let k = kunci(a.get(1), "dict::set")?;
        m.insert(k, a[2].clone());
        Ok(jadi_dict(m))
    });

    vm.def("dict::remove", |a| {
        cek_dict_kurang("dict::remove", a.len())?;
        if let Some(d) = milik_tunggal(a.get(0)) {
            let k = kunci(a.get(1), "dict::remove")?;
            d.lock().unwrap_or_else(|p| p.into_inner()).remove(&k);
            return Ok(a[0].clone());
        }
        let mut m = isi(a.get(0), 1, "dict::remove")?;
        let k = kunci(a.get(1), "dict::remove")?;
        m.remove(&k);
        Ok(jadi_dict(m))
    });

    vm.def("dict::merge", |a| {
        cek_dict_kurang("dict::merge", a.len())?;
        let mut m = isi(a.get(0), 1, "dict::merge")?;
        for (k, v) in isi(a.get(1), 2, "dict::merge")? {
            m.insert(k, v);
        }
        Ok(jadi_dict(m))
    });
}
