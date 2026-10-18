// src/vm/ffi_asing.rs — `py::`/`js::`/`php::`/`lua::` di VM: handle asing sebagai nilai,
// dan pintu yang menyerahkan panggilannya ke `interpreter::builtin::ffi_pintu` (satu
// definisi untuk dua mesin) dengan callback yang menjalankan fungsi di VM ini.

use std::sync::{Arc, Mutex, OnceLock};

use crate::interpreter::builtin::{aritas, ffi_pintu, NativeFnKind};
use crate::interpreter::Value;
use super::ffi_jembatan as jb;
use super::value::{VmNativeFn, VmValue};
use super::vm::{Vm, VmError, VmResult};

/// Handle ke objek milik worker bahasa lain — pegangan, bukan nilai.
#[derive(Clone, Debug)]
pub enum Asing {
    Py(crate::ffi::python::PyRef),
    Js(crate::ffi::javascript::JsRef),
    Php(crate::ffi::php::PhpRef),
    /// `h.nama` di posisi panggilan: method milik worker, belum dipanggil.
    Metode(Arc<str>),
}

impl Asing {
    pub fn dari(v: &Value) -> Option<Asing> {
        Some(match v {
            Value::PyObj(h) => Asing::Py(h.clone()),
            Value::JsObj(h) => Asing::Js(h.clone()),
            Value::PhpObj(h) => Asing::Php(h.clone()),
            _ => return None,
        })
    }

    pub fn ke_nilai(&self) -> Option<Value> {
        Some(match self {
            Asing::Py(h) => Value::PyObj(h.clone()),
            Asing::Js(h) => Value::JsObj(h.clone()),
            Asing::Php(h) => Value::PhpObj(h.clone()),
            Asing::Metode(_) => return None,
        })
    }

    /// Nama tipe dan tampilannya dibaca dari nilai interpreter-nya: satu definisi.
    pub fn nama_tipe(&self) -> String {
        self.ke_nilai().map(|v| v.type_name()).unwrap_or_else(|| "function".to_string())
    }

    pub fn tampil(&self) -> String {
        self.ke_nilai().map(|v| v.to_string()).unwrap_or_else(|| "<fn>".to_string())
    }

    pub fn sama(&self, lain: &Asing) -> bool {
        match (self, lain) {
            (Asing::Py(a), Asing::Py(b)) => a.id == b.id && a.generasi == b.generasi,
            (Asing::Js(a), Asing::Js(b)) => a.id == b.id && a.generasi == b.generasi,
            (Asing::Php(a), Asing::Php(b)) => a.id == b.id && a.generasi == b.generasi,
            _ => false,
        }
    }
}

/// `type_name` VM menjawab `&'static str`; nama tipe asing (`py:DataFrame`) diinternir.
pub(crate) fn nama_tipe_statis(a: &Asing) -> &'static str {
    static NAMA: OnceLock<Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let n = a.nama_tipe();
    let mut g = NAMA.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.get(n.as_str()) { return s; }
    let s: &'static str = Box::leak(n.into_boxed_str());
    g.insert(s);
    s
}

/// Wakil di tabel bawaan: satu ALAMAT per builtin, tidak pernah dijalankan —
/// `NativeKhusus::Ffi` mengalihkannya. `N` di badannya mencegah penggabungan fungsi.
fn w<const N: usize>(_: &[VmValue]) -> Result<VmValue, String> {
    Err(format!("builtin FFI nomor {} dipanggil tanpa mesinnya — ini bug di JRG sendiri", N))
}

const WAKIL: &[VmNativeFn] = &[
    w::<0>, w::<1>, w::<2>, w::<3>, w::<4>, w::<5>, w::<6>, w::<7>,
    w::<8>, w::<9>, w::<10>, w::<11>, w::<12>, w::<13>, w::<14>, w::<15>,
    w::<16>, w::<17>, w::<18>, w::<19>, w::<20>, w::<21>, w::<22>, w::<23>,
    w::<24>, w::<25>, w::<26>, w::<27>, w::<28>, w::<29>, w::<30>, w::<31>,
    w::<32>, w::<33>, w::<34>, w::<35>, w::<36>, w::<37>, w::<38>, w::<39>,
    w::<40>, w::<41>, w::<42>, w::<43>, w::<44>, w::<45>, w::<46>, w::<47>,
    w::<48>, w::<49>, w::<50>, w::<51>, w::<52>, w::<53>, w::<54>, w::<55>,
    w::<56>, w::<57>, w::<58>, w::<59>, w::<60>, w::<61>, w::<62>, w::<63>,
    w::<64>, w::<65>, w::<66>, w::<67>, w::<68>, w::<69>, w::<70>, w::<71>,
    w::<72>, w::<73>, w::<74>, w::<75>, w::<76>, w::<77>, w::<78>, w::<79>,
    w::<80>, w::<81>, w::<82>, w::<83>, w::<84>, w::<85>, w::<86>, w::<87>,
    w::<88>, w::<89>, w::<90>, w::<91>, w::<92>, w::<93>, w::<94>, w::<95>,
];

/// Jenis builtin FFI, berurut nama kanoniknya; indeksnya dipakai `NativeKhusus::Ffi`.
fn jenis() -> &'static Vec<(NativeFnKind, Vec<String>)> {
    static J: OnceLock<Vec<(NativeFnKind, Vec<String>)>> = OnceLock::new();
    J.get_or_init(|| {
        let mut v: Vec<(NativeFnKind, Vec<String>)> = Vec::new();
        for n in crate::interpreter::Env::nama_ffi() {
            let Some(k) = aritas::jenis_nama(&n) else { continue };
            match v.iter_mut().find(|(j, _)| *j == k) {
                Some((_, nama)) => nama.push(n),
                None => v.push((k, vec![n])),
            }
        }
        assert!(v.len() <= WAKIL.len(), "wakil FFI kurang: {} jenis, {} wakil", v.len(), WAKIL.len());
        v
    })
}

/// Nama dan wakilnya, untuk didaftarkan ke tabel bawaan.
pub(crate) fn daftar() -> Vec<(String, VmNativeFn)> {
    jenis().iter().enumerate()
        .flat_map(|(i, (_, nama))| nama.iter().map(move |n| (n.clone(), WAKIL[i])))
        .collect()
}

/// Nomor jenis FFI di balik sebuah nama bawaan.
pub(crate) fn nomor(nama: &str) -> Option<u16> {
    jenis().iter().position(|(_, n)| n.iter().any(|x| x == nama)).map(|i| i as u16)
}

/// Fungsi VM yang sedang dipegang worker. Hidup lintas panggilan — registri fungsi
/// worker milik SESI, jadi callback bisa datang di panggilan lain — dan sebuah entri
/// dibuang begitu penandanya tidak dipegang siapa pun lagi (registrinya melepasnya).
#[derive(Default)]
struct TitipanSesi {
    isi: std::collections::HashMap<usize, (VmValue, std::sync::Weak<Vec<String>>)>,
    berikut: usize,
    ambang: usize,
}

impl TitipanSesi {
    fn titip(&mut self, f: &VmValue) -> Result<Value, String> {
        // Channel menyeberang sebagai channel kosong: lapisan FFI yang menolaknya,
        // dengan kalimat yang sama dengan interpreter.
        if let VmValue::Channel(_) = f {
            return Ok(Value::Channel(crate::interpreter::channel_new(0)));
        }
        if let Some(r) = jb::titip_bernama(f) { return r; }
        if self.isi.len() >= self.ambang {
            self.isi.retain(|_, (_, w)| w.strong_count() > 0);
            self.ambang = (self.isi.len() * 2).max(64);
        }
        let no = self.berikut;
        self.berikut += 1;
        let v = jb::penanda(f, no);
        if let Some((_, params)) = jb::nomor_penanda(&v) {
            self.isi.insert(no, (f.clone(), Arc::downgrade(params)));
        }
        Ok(v)
    }

    fn pulihkan(&self, f: &Value) -> Result<VmValue, String> {
        match jb::nomor_penanda(f).and_then(|(no, _)| self.isi.get(&no)) {
            Some((asli, _)) => Ok(asli.clone()),
            None => jb::pulih_bernama(f),
        }
    }
}

fn titipan() -> &'static Mutex<TitipanSesi> {
    static T: OnceLock<Mutex<TitipanSesi>> = OnceLock::new();
    T.get_or_init(Default::default)
}

fn ke_interp(v: &VmValue) -> Result<Value, String> {
    let mut t = titipan().lock().unwrap_or_else(|p| p.into_inner());
    jb::vm_ke_nilai_dgn(v, &mut |f| t.titip(f))
}

fn ke_vm(v: &Value) -> Result<VmValue, String> {
    let t = titipan().lock().unwrap_or_else(|p| p.into_inner());
    jb::nilai_ke_vm_dgn(v, &mut |f| t.pulihkan(f))
}

impl Vm {
    /// Jalankan `kerja` dengan callback yang memanggil fungsi JRG di VM ini.
    fn dgn_panggil<T>(&mut self, kerja: impl FnOnce(ffi_pintu::Panggil) -> T) -> T {
        let mut panggil = |f: &Value, a: Vec<Value>| -> Result<Value, String> {
            let f = ke_vm(f)?;
            let mut avm = Vec::with_capacity(a.len());
            for x in &a { avm.push(ke_vm(x)?); }
            let hasil = self.call_value_immediate(f, avm).map_err(|e| e.msg)?;
            ke_interp(&hasil)
        };
        kerja(&mut panggil)
    }

    /// Layani builtin FFI bernomor `no` lewat pintu yang sama dengan interpreter.
    pub(crate) fn ffi_vm(&mut self, no: u16, args: &[VmValue]) -> VmResult<VmValue> {
        let Some((jenis, _)) = jenis().get(no as usize) else {
            return Err(VmError::new("builtin FFI tanpa jenis — ini bug di JRG sendiri".to_string()));
        };
        let mut nilai = Vec::with_capacity(args.len());
        for a in args { nilai.push(ke_interp(a).map_err(VmError::new)?); }
        match self.dgn_panggil(|p| ffi_pintu::pintu_ffi(jenis, &nilai, p)) {
            Some(Ok(v)) => ke_vm(&v).map_err(VmError::new),
            Some(Err(s)) => Err(VmError::new(ffi_pintu::pesan_sinyal(s))),
            None => Err(VmError::new("builtin FFI tanpa pintu — ini bug di JRG sendiri".to_string())),
        }
    }

    /// `h.nama(args)` atas handle asing: `args[0]` penerimanya.
    pub(crate) fn metode_asing_vm(&mut self, nama: &str, args: &[VmValue]) -> VmResult<VmValue> {
        let mut nilai = Vec::with_capacity(args.len());
        for a in args { nilai.push(ke_interp(a).map_err(VmError::new)?); }
        if nilai.is_empty() {
            return Err(VmError::new(format!("method `{}` dipanggil tanpa penerimanya", nama)));
        }
        let obj = nilai.remove(0);
        match self.dgn_panggil(|p| ffi_pintu::metode_handle(&obj, nama, nilai, p)) {
            Some(v) => ke_vm(&v).map_err(VmError::new),
            None => Err(VmError::new(crate::interpreter::pesan_method_tak_ada(
                &obj.type_name(), nama, "", None))),
        }
    }
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn tiap_wakil_ffi_punya_alamat_sendiri() {
        let mut a: Vec<usize> = WAKIL.iter().map(|f| *f as usize).collect();
        a.sort();
        a.dedup();
        assert_eq!(a.len(), WAKIL.len(), "dua wakil FFI digabung jadi satu alamat");
        assert!(!jenis().is_empty());
    }
}
