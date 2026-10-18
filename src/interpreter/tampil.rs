// src/interpreter/tampil.rs — src/interpreter/tampil.rs — hook tampilan buatan pengguna (`to_string`).

use super::{EvalResult, Interpreter, Value};
use crate::interpreter::Signal;

/// Nama metode yang dipakai sebagai hook.
pub const NAMA_HOOK: &str = "to_string";

/// Berapa hook boleh bersarang sebelum dianggap tidak berujung.
pub const MAX_SARANG_HOOK: usize = 64;

/// Pesan untuk hook yang memanggil dirinya sendiri.
pub fn pesan_sarang(tipe: &str) -> String {
    format!(
        "`{}::{}` menampilkan dirinya sendiri, jadi ia tidak pernah selesai — \
         sudah {} tingkat bersarang.\n  \
         bantuan: di dalam hook tampilan, susun teksnya dari field (`self.n`), \
         jangan mencetak `self`",
        tipe, NAMA_HOOK, MAX_SARANG_HOOK
    )
}

impl Interpreter {
    /// Ubah sebuah nilai jadi teks seperti yang dilihat pengguna.
    pub(crate) fn tampil(&mut self, v: &Value) -> Result<String, Signal> {
        // Nilai yang tidak mungkin memuat hook diserahkan langsung ke `Display`.
        if !Self::mungkin_punya_hook(v) {
            return Ok(format!("{}", v));
        }

        match v {
            Value::Struct { name, fields } => {
                // `dict` diwakili sebagai `Value::Struct` bernama "dict" di dalam, tapi dari
                // sisi bahasa ia koleksi tersendiri.
                if name != "dict" {
                    if let Some(teks) = self.coba_hook(v, name)? {
                        return Ok(teks);
                    }
                }
                let mut kv: Vec<_> = fields.iter().collect();
                kv.sort_by_key(|(k, _)| k.as_str());
                let mut out = if name == "dict" {
                    "dict {".to_string()
                } else {
                    format!("{} {{", crate::ruang_tipe::bersih(name))
                };
                for (i, (k, val)) in kv.iter().enumerate() {
                    if i > 0 { out.push_str(", "); }
                    out.push_str(k);
                    out.push_str(": ");
                    out.push_str(&self.tampil(val)?);
                }
                out.push('}');
                Ok(out)
            }
            Value::Enum { type_name, variant, payload } => {
                if let Some(teks) = self.coba_hook(v, type_name)? {
                    return Ok(teks);
                }
                let mut out = format!("{}::{}", crate::ruang_tipe::bersih(type_name), variant);
                if let Some(p) = payload {
                    out.push('(');
                    out.push_str(&self.tampil(p)?);
                    out.push(')');
                }
                Ok(out)
            }
            // Koleksi ditelusuri, karena sebuah nilai harus tampil sama apakah ia
            // berdiri sendiri atau ada di dalam array.
            Value::Array(a) => {
                let mut out = String::from("[");
                for (i, x) in a.iter().enumerate() {
                    if i > 0 { out.push_str(", "); }
                    out.push_str(&self.tampil(x)?);
                }
                out.push(']');
                Ok(out)
            }
            Value::Tuple(t) => {
                let mut out = String::from("(");
                for (i, x) in t.iter().enumerate() {
                    if i > 0 { out.push_str(", "); }
                    out.push_str(&self.tampil(x)?);
                }
                out.push(')');
                Ok(out)
            }
            _ => Ok(format!("{}", v)),
        }
    }

    /// Apakah nilai ini, atau sesuatu di dalamnya, mungkin punya hook.
    fn mungkin_punya_hook(v: &Value) -> bool {
        matches!(
            v,
            Value::Struct { .. } | Value::Enum { .. } | Value::Array(_) | Value::Tuple(_)
        )
    }

    /// Panggil hook kalau tipe ini punya, dan pastikan hasilnya masuk akal.
    ///
    /// `Ok(None)` berarti tipe ini tidak punya hook — bukan kegagalan.
    fn coba_hook(&mut self, v: &Value, tname: &str) -> Result<Option<String>, Signal> {
        // Penelusuran yang SAMA dengan pemanggilan metode biasa, termasuk pewarisan
        // dan default trait.
        let Some(func) = self.resolusi_metode(tname, NAMA_HOOK) else {
            return Ok(None);
        };

        // Penjaga dipasang di sekitar PEMANGGILAN hook, bukan di sekitar
        // penelusuran kontainer — lihat `MAX_SARANG_HOOK`.
        if self.sarang_hook >= MAX_SARANG_HOOK {
            return Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                pesan_sarang(tname),
            )));
        }

        // Bentuknya diperiksa, dan pelanggarannya ERROR alih-alih diabaikan.
        if func.params.len() != 1 {
            return Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                format!(
                    "`{}::{}` dipakai sebagai hook tampilan, jadi ia harus menerima \
                     `self` saja — yang ini minta {} parameter. Ganti namanya kalau \
                     memang bukan hook tampilan",
                    tname, NAMA_HOOK, func.params.len()
                ),
            )));
        }

        self.sarang_hook += 1;
        let hasil = self.call_jrg(func, vec![v.clone()]);
        self.sarang_hook -= 1;
        let hasil = hasil?;
        match hasil {
            Value::Str(s) => Ok(Some(s)),
            lain => Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                format!(
                    "`{}::{}` dipakai sebagai hook tampilan, jadi ia harus \
                     mengembalikan string — yang ini mengembalikan `{}`",
                    tname, NAMA_HOOK, lain.type_name()
                ),
            ))),
        }
    }

    /// `tampil` untuk beberapa nilai sekaligus, dipisah spasi.
    ///
    /// Bentuk yang dipakai seluruh keluarga cetak (`wok(a, b, c)`).
    pub(crate) fn tampil_gabung(&mut self, args: &[Value]) -> Result<String, Signal> {
        let mut out = String::new();
        for (i, v) in args.iter().enumerate() {
            if i > 0 { out.push(' '); }
            out.push_str(&self.tampil(v)?);
        }
        Ok(out)
    }
}

/// Supaya `EvalResult` tetap terpakai di tanda tangan modul ini kalau nanti
/// ada helper yang mengembalikan `Value`.
#[allow(dead_code)]
type _Pakai = EvalResult;

// ─── Operator yang bisa didefinisikan pengguna
// ──────────────────────────────

use crate::interpreter::operator as op_tabel;
use crate::parser::{BinaryOp, UnaryOp};

impl Interpreter {
    /// Nama tipe sebuah nilai, kalau ia bisa punya metode.
    fn tipe_bermetode(v: &Value) -> Option<&str> {
        match v {
            // `dict` diwakili sebagai struct bernama "dict" di dalam, tapi ia
            // koleksi bawaan — bukan tipe buatan pemakai.
            Value::Struct { name, .. } if name != "dict" => Some(name),
            Value::Enum { type_name, .. } => Some(type_name),
            _ => None,
        }
    }

    /// Coba layani operator biner lewat metode milik pengguna.
    pub(crate) fn operator_biner_pengguna(
        &mut self,
        op: &BinaryOp,
        a: &Value,
        b: &Value,
    ) -> Result<Option<Value>, Signal> {
        let Some(metode) = op_tabel::nama_biner(op) else { return Ok(None) };
        // Dicari pada operand KIRI saja.
        let Some(tname) = Self::tipe_bermetode(a) else { return Ok(None) };
        let tname = tname.to_string();
        let Some(func) = self.resolusi_metode(&tname, metode) else { return Ok(None) };

        if func.params.len() != 2 {
            return Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                op_tabel::pesan_arity(&tname, metode, func.params.len()),
            )));
        }

        let hasil = self.call_jrg(func, vec![a.clone(), b.clone()])?;

        if op_tabel::pakai_eq(op) {
            let Value::Bool(sama) = hasil else {
                return Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                    op_tabel::pesan_hasil(&tname, metode, "bool", &hasil.type_name()),
                )));
            };
            return Ok(Some(Value::Bool(if matches!(op, BinaryOp::Ne) { !sama } else { sama })));
        }
        if op_tabel::pakai_cmp(op) {
            let Value::Int(urut) = hasil else {
                return Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                    op_tabel::pesan_hasil(&tname, metode, "int", &hasil.type_name()),
                )));
            };
            return Ok(Some(Value::Bool(op_tabel::tafsir_cmp(op, urut))));
        }
        Ok(Some(hasil))
    }

    /// Coba layani operator awalan lewat metode milik pengguna.
    pub(crate) fn operator_unary_pengguna(
        &mut self,
        op: &UnaryOp,
        v: &Value,
    ) -> Result<Option<Value>, Signal> {
        let Some(metode) = op_tabel::nama_unary(op) else { return Ok(None) };
        let Some(tname) = Self::tipe_bermetode(v) else { return Ok(None) };
        let tname = tname.to_string();
        let Some(func) = self.resolusi_metode(&tname, metode) else { return Ok(None) };

        if func.params.len() != 1 {
            return Err(Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                op_tabel::pesan_arity_unary(&tname, metode, func.params.len()),
            )));
        }
        Ok(Some(self.call_jrg(func, vec![v.clone()])?))
    }
}
