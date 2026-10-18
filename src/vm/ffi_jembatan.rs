// src/vm/ffi_jembatan.rs
// Jembatan `extern "C"` untuk backend VM.

use std::sync::Arc;

use crate::ffi::native::ExternFnInfo;
use crate::interpreter::Value;
use crate::vm::value::{VmNativeFn, VmValue};

/// Fungsi VM yang tidak pernah DITITIPKAN (lahir di seberang) tidak bisa dibawa balik.
const PESAN_FUNGSI: &str = "sebuah fungsi tidak bisa menyeberang antar-backend: jembatan ini \
     memindahkan NILAI, dan fungsi bukan nilai — ia terikat pada runtime \
     yang membuatnya, lengkap dengan bytecode dan tangkapannya";

/// Penelusur nilai VM → interpreter; `fungsi` memutuskan nasib tiap nilai fungsi di
/// kedalaman mana pun — satu definisi konversi untuk semua jalur (lihat `TitipanVm`).
pub(crate) fn vm_ke_nilai_dgn(
    v: &VmValue,
    fungsi: &mut dyn FnMut(&VmValue) -> Result<Value, String>,
) -> Result<Value, String> {
    Ok(match v {
        VmValue::Nil => Value::Nil,
        VmValue::Unit => Value::Unit,
        VmValue::Bool(b) => Value::Bool(*b),
        VmValue::Int(n) => Value::Int(*n),
        VmValue::Float(f) => Value::Float(*f),
        VmValue::Rune(c) => Value::Rune(*c),
        VmValue::Str(s) => Value::Str(s.to_string()),
        VmValue::Ptr(a) => Value::Ptr(*a),
        // Array numerik `wokwok::` menyeberang dengan MENYALIN PENUNJUK, bukan
        // isinya.
        VmValue::Nd(a) => Value::Nd(Arc::clone(a)),
        VmValue::Canvas(c) => Value::Canvas(Arc::clone(c)),
        VmValue::Sinkron(p) => Value::Sinkron(p.clone()),
        VmValue::Asing(a) if a.ke_nilai().is_some() => a.ke_nilai().unwrap_or(Value::Nil),
        // Sel binding tidak pernah jadi nilai yang dilihat program, tapi
        // kalau bocor, artinya tetap "nilai di dalam binding".
        VmValue::Cell(c) => vm_ke_nilai_dgn(&c.lock().unwrap_or_else(|p| p.into_inner()), fungsi)?,

        // Agregat SENGAJA ikut diterjemahkan, bukan ditolak di sini.
        VmValue::Array(a) => {
            let isi = a.lock().unwrap_or_else(|p| p.into_inner());
            let mut out = Vec::with_capacity(isi.len());
            for x in isi.iter() { out.push(vm_ke_nilai_dgn(x, fungsi)?); }
            Value::Array(out.into())
        }
        VmValue::Tuple(t) => {
            let t = crate::vm::value::isi_tuple(t);
            let mut out = Vec::with_capacity(t.len());
            for x in t.iter() { out.push(vm_ke_nilai_dgn(x, fungsi)?); }
            Value::Tuple(out.into())
        }
        // Struct, enum, dict, dan handle `ptr::`/`jawa::` semuanya `Map` di VM; jenisnya
        // dari LABEL, jadi kunci `variant` milik data (mis. hasil JSON) tetap data.
        VmValue::Map(m) => match &m.label {
            crate::vm::value::Label::Enum { tipe, varian, payload } => Value::Enum {
                type_name: tipe.to_string(),
                variant: varian.to_string(),
                payload: match payload {
                    Some(p) => Some(Box::new(vm_ke_nilai_dgn(p, fungsi)?)),
                    None => None,
                },
            },
            label => {
                let nama = match label {
                    crate::vm::value::Label::Struct(n) => n.to_string(),
                    _ => "dict".to_string(),
                };
                let g = m.lock().unwrap_or_else(|p| p.into_inner());
                let mut f = std::collections::HashMap::new();
                for (k, v) in g.iter() {
                    f.insert(k.clone(), vm_ke_nilai_dgn(v, fungsi)?);
                }
                Value::Struct { name: nama, fields: f.into() }
            }
        },
        // Channel ikut ditanyakan: pintu FFI menyeberangkannya untuk DITOLAK lapisannya sendiri.
        VmValue::Function(_) | VmValue::Closure(_) | VmValue::NativeFn(_) | VmValue::Channel(_) => return fungsi(v),
        // Konstruktor cuma data (tipe, varian, jumlah payload), jadi menyeberang utuh.
        VmValue::Ktor(k) => Value::EnumCtor {
            type_name: k.tipe.to_string(), variant: k.varian.to_string(), arity: k.arity,
        },
        lain => return Err(pesan_tak_menyeberang(lain)),
    })
}

/// Arah sebaliknya: nilai balik dari lapisan FFI kembali ke VM.
pub fn nilai_ke_vm(v: &Value) -> Result<VmValue, String> {
    nilai_ke_vm_dgn(v, &mut |f| Err(format!("nilai balik `{}` tidak punya padanan di VM", f.type_name())))
}

pub(crate) fn nilai_ke_vm_dgn(
    v: &Value,
    fungsi: &mut dyn FnMut(&Value) -> Result<VmValue, String>,
) -> Result<VmValue, String> {
    Ok(match v {
        Value::Nil => VmValue::Nil,
        Value::Unit => VmValue::Unit,
        Value::Bool(b) => VmValue::Bool(*b),
        Value::Int(n) => VmValue::Int(*n),
        Value::Float(f) => VmValue::Float(*f),
        Value::Rune(c) => VmValue::Rune(*c),
        Value::Str(s) => VmValue::str(s.clone()),
        Value::Ptr(a) => VmValue::Ptr(*a),
        Value::Nd(a) => VmValue::Nd(Arc::clone(a)),
        Value::Canvas(c) => VmValue::Canvas(Arc::clone(c)),
        Value::Sinkron(p) => VmValue::Sinkron(p.clone()),
        Value::PyObj(_) | Value::JsObj(_) | Value::PhpObj(_) => match super::ffi_asing::Asing::dari(v) {
            Some(a) => VmValue::Asing(a),
            None => VmValue::Nil,
        },
        // `Result::Ok(..)` / `Result::Err(..)` dari lapisan FFI. Bentuk
        // ber-tag di VM harus sama persis dengan yang dipakai enum lain,
        // supaya `match` dan `?` memperlakukannya seperti Result biasa.
        Value::Enum { type_name, variant, payload } => VmValue::varian(
            type_name,
            variant,
            match payload { Some(p) => Some(nilai_ke_vm_dgn(p, fungsi)?), None => None },
        ),
        // Agregat: dipakai bukan cuma oleh FFI (yang memang tidak melewatkannya)
        // tapi juga oleh jembatan builtin sistem — mis. `env::args()` mengembalikan …
        Value::Array(v) => {
            let mut out = Vec::with_capacity(v.len());
            for x in v { out.push(nilai_ke_vm_dgn(x, fungsi)?); }
            VmValue::array(out)
        }
        Value::Tuple(v) => {
            let mut out = Vec::with_capacity(v.len());
            for x in v { out.push(nilai_ke_vm_dgn(x, fungsi)?); }
            VmValue::tuple(out)
        }
        // Struct interpreter jadi struct ber-label — kecuali `dict`, yang di dalam
        // interpreter memang sebuah struct bernama "dict".
        Value::Struct { name, fields } => {
            let mut m = std::collections::HashMap::new();
            for (k, v) in fields {
                m.insert(k.clone(), nilai_ke_vm_dgn(v, fungsi)?);
            }
            if name == "dict" { VmValue::map(m) } else { VmValue::struktur(name, m) }
        }
        Value::Function(_) | Value::Closure { .. } | Value::NativeFn(_) => return fungsi(v),
        Value::EnumCtor { type_name, variant, arity } => VmValue::ktor(type_name, variant, *arity),
        lain => {
            return Err(format!(
                "nilai balik `{}` tidak punya padanan di VM",
                lain.type_name()
            ))
        }
    })
}

/// Penanda fungsi VM di sisi interpreter: nama parameter pertamanya tidak bisa
/// ditulis sebagai identifier, jadi data tidak bisa memalsukannya.
const PENANDA: &str = "\u{0}titipan-vm";

/// Fungsi VM yang diserahkan ke badan milik interpreter. Yang menyeberang
/// PENANDA bertipe dan bertampilan sama; sesudahnya penanda itu dipulihkan.
#[derive(Default)]
pub struct TitipanVm { asli: Vec<VmValue> }

impl TitipanVm {
    pub fn ke_interp(&mut self, v: &VmValue) -> Result<Value, String> {
        vm_ke_nilai_dgn(v, &mut |f| self.titip(f))
    }

    pub fn ke_vm(&self, v: &Value) -> Result<VmValue, String> {
        nilai_ke_vm_dgn(v, &mut |f| self.pulihkan(f))
    }

    fn titip(&mut self, f: &VmValue) -> Result<Value, String> {
        if let Some(r) = titip_bernama(f) { return r; }
        self.asli.push(f.clone());
        Ok(penanda(f, self.asli.len() - 1))
    }
}

fn pesan_tak_menyeberang(v: &VmValue) -> String {
    format!("`{}` tidak bisa diserahkan lewat jembatan nilai antar-backend", v.type_name())
}

/// Builtin menyeberang sebagai dirinya sendiri (ia punya nama); channel tidak menyeberang.
pub(crate) fn titip_bernama(f: &VmValue) -> Option<Result<Value, String>> {
    use crate::interpreter::builtin::{jembatan_stdlib, NativeFnKind};
    match f {
        VmValue::NativeFn(p) => Some(match jembatan_stdlib::nama_native(*p) {
            Some(n) => Ok(Value::NativeFn(NativeFnKind::DariStdlibVm(n))),
            None => Err("fungsi bawaan tanpa nama di jembatan nilai — ini bug di JRG sendiri".to_string()),
        }),
        VmValue::Channel(_) => Some(Err(pesan_tak_menyeberang(f))),
        _ => None,
    }
}

/// Penanda bernomor `no` untuk fungsi VM `f`: tipe dan tampilannya sama dengan aslinya.
pub(crate) fn penanda(f: &VmValue, no: usize) -> Value {
    let params = vec![PENANDA.to_string(), no.to_string()];
    match f {
            VmValue::Function(g) if !g.is_closure => Value::Function(crate::interpreter::JrgFunction {
                name: g.name.clone(),
                params: params.into(),
                defaults: Default::default(),
                variadic: false,
                body: crate::parser::FnBody::Abstrak.into(),
                env: std::sync::Arc::new(crate::interpreter::Env::with_globals(Default::default())),
                group: None,
                pemilik: None,
                ruang: None,
            }.into()),
            _ => Value::Closure {
                params: params.into(),
                body: Box::new(crate::parser::Expr::Literal(
                    crate::parser::LitKind::Nil,
                    crate::lexer::Span::dummy(),
                )).into(),
                captures: Vec::new(),
                globals: Default::default(),
                ruang: None,
            },
    }
}

/// `params` sebuah penanda dan nomornya, kalau `f` memang penanda.
pub(crate) fn nomor_penanda(f: &Value) -> Option<(usize, &Arc<Vec<String>>)> {
    let p = match f {
        Value::Function(g) => &g.params,
        Value::Closure { params, .. } => params,
        _ => return None,
    };
    if p.len() != 2 || p[0] != PENANDA { return None; }
    Some((p[1].parse().ok()?, p))
}

/// Builtin interpreter yang punya padanan di VM pulang sebagai native VM-nya.
pub(crate) fn pulih_bernama(f: &Value) -> Result<VmValue, String> {
    if let Value::NativeFn(k) = f {
        if let Some(p) = crate::interpreter::builtin::jembatan_stdlib::native_vm_untuk(k) {
            return Ok(VmValue::NativeFn(p));
        }
    }
    Err(format!("nilai balik `{}` tidak punya padanan di VM", f.type_name()))
}

impl TitipanVm {
    fn pulihkan(&self, f: &Value) -> Result<VmValue, String> {
        match nomor_penanda(f).and_then(|(i, _)| self.asli.get(i)) {
            Some(asli) => Ok(asli.clone()),
            None => pulih_bernama(f),
        }
    }
}

/// Fungsi interpreter yang diserahkan ke badan milik VM. Penandanya dikenali
/// lewat IDENTITAS (`Arc` yang sama, alamat yang sama), bukan isinya.
#[derive(Default)]
pub struct TitipanInterp { asli: Vec<Value>, penanda: Vec<VmValue> }

/// Wakil builtin yang cuma ada di interpreter (`py::`, `js::`); tidak pernah dipanggil.
fn fungsi_interpreter_saja(_: &[VmValue]) -> Result<VmValue, String> {
    Err("fungsi ini milik interpreter dan tidak bisa dipanggil dari sini — ini bug di JRG sendiri".to_string())
}

impl TitipanInterp {
    pub fn ke_vm(&mut self, v: &Value) -> Result<VmValue, String> {
        nilai_ke_vm_dgn(v, &mut |f| self.titip(f))
    }

    pub fn ke_interp(&self, v: &VmValue) -> Result<Value, String> {
        vm_ke_nilai_dgn(v, &mut |f| self.pulihkan(f))
    }

    fn titip(&mut self, f: &Value) -> Result<VmValue, String> {
        let p = match f {
            Value::NativeFn(k) => match crate::interpreter::builtin::jembatan_stdlib::native_vm_untuk(k) {
                Some(p) => VmValue::NativeFn(p),
                None => {
                    // Satu wakil untuk semuanya: dua yang BERBEDA tidak bisa dibedakan lagi.
                    let dipakai = self.asli.iter().zip(&self.penanda).any(|(a, p)|
                        matches!(p, VmValue::NativeFn(g) if *g as usize == fungsi_interpreter_saja as VmNativeFn as usize)
                        && !matches!((a, f), (Value::NativeFn(x), Value::NativeFn(y)) if x == y));
                    if dipakai {
                        return Err("dua fungsi bawaan yang cuma ada di interpreter tidak bisa \
                                    diserahkan dalam satu panggilan ini".to_string());
                    }
                    VmValue::NativeFn(fungsi_interpreter_saja)
                }
            },
            Value::Function(g) => VmValue::Function(Arc::new(crate::vm::value::VmFunction::new(
                g.name.clone(), 0, crate::vm::chunk::Chunk::new(g.name.clone(), 0)))),
            _ => VmValue::Function(Arc::new(crate::vm::value::VmFunction::closure(
                "<closure>", 0, crate::vm::chunk::Chunk::new("<closure>", 0), Vec::new(), false))),
        };
        self.asli.push(f.clone());
        self.penanda.push(p.clone());
        Ok(p)
    }

    fn pulihkan(&self, f: &VmValue) -> Result<Value, String> {
        if let VmValue::Channel(_) = f { return Err(pesan_tak_menyeberang(f)); }
        for (p, a) in self.penanda.iter().zip(&self.asli) {
            let sama = match (p, f) {
                (VmValue::Function(x), VmValue::Function(y)) => Arc::ptr_eq(x, y),
                (VmValue::NativeFn(x), VmValue::NativeFn(y)) => *x as usize == *y as usize,
                _ => false,
            };
            if sama { return Ok(a.clone()); }
        }
        match f {
            VmValue::NativeFn(p) => match crate::interpreter::builtin::jembatan_stdlib::nama_native(*p) {
                Some(n) => Ok(Value::NativeFn(crate::interpreter::builtin::NativeFnKind::DariStdlibVm(n))),
                None => Err(PESAN_FUNGSI.to_string()),
            },
            _ => Err(PESAN_FUNGSI.to_string()),
        }
    }
}

/// Galat keras dari lapisan FFI: pesannya apa adanya, seperti interpreter yang
/// meneruskan `Signal`-nya — bukan bentuk `Debug` Rust di depan pemakai.
fn pesan_sinyal(nama: &str, s: crate::interpreter::Signal) -> String {
    match s {
        crate::interpreter::Signal::RuntimeError(r) => r.message,
        lain => format!("extern fn `{}`: sinyal tak terduga: {:?}", nama, lain),
    }
}

/// Panggil sebuah fungsi `extern "C"` dari VM.
pub fn panggil(info: &Arc<ExternFnInfo>, args: &[VmValue]) -> Result<VmValue, String> {
    // Simbol/pustaka/signature yang gagal diresolusi dijelaskan lapisan FFI
    // TANPA melihat argumen.
    if info.resolved.is_err() {
        return match crate::ffi::native::call_extern_fn(info, Vec::new()) {
            Ok(v) => nilai_ke_vm(&v),
            Err(sinyal) => Err(pesan_sinyal(&info.name, sinyal)),
        };
    }
    let mut titipan = TitipanVm::default();
    let mut arg_nilai = Vec::with_capacity(args.len());
    for a in args {
        arg_nilai.push(titipan.ke_interp(a)?);
    }
    match crate::ffi::native::call_extern_fn(info, arg_nilai) {
        Ok(v) => nilai_ke_vm(&v),
        // Lapisan FFI mengembalikan kegagalan yang WAJAR sebagai
        // `Result::Err` (nilai, bukan error), jadi `Err` di sini berarti
        // kesalahan pemakaian yang keras — diteruskan apa adanya.
        Err(sinyal) => Err(pesan_sinyal(&info.name, sinyal)),
    }
}
