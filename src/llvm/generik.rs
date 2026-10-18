// src/llvm/generik.rs — struct generik: satu tata letak per susunan argumen tipe
// (`Kotak<int>` dan `Kotak<string>` dua struct berbeda).

use crate::parser::*;

use super::codegen::Codegen;
use super::sema::TypeSubst;
use super::types::*;

/// Nama struct seperti yang DITULIS pemakainya: instans `Kotak$i64` kembali jadi `Kotak`.
pub fn dasar(nama: &str) -> &str {
    nama.split_once('$').map_or(nama, |(d, _)| d)
}

/// Nama instans untuk `dasar<args>`.
pub fn nama_instans(dasar: &str, args: &[JrgType]) -> String {
    let frag: Vec<String> = args.iter().map(|t| t.mangle_frag()).collect();
    format!("{}${}", dasar, frag.join("$"))
}

impl Codegen {
    /// Parameter tipe struct `nama`, kalau ia generik.
    pub(crate) fn param_struct(&self, nama: &str) -> Option<Vec<String>> {
        let d = self.tables.structs.get(nama)?;
        if d.generics.params.is_empty() { return None; }
        Some(d.generics.params.iter().map(|p| p.name.clone()).collect())
    }

    /// Daftarkan (sekali) tata letak `dasar<args>` dan kembalikan nama instansnya.
    pub(crate) fn instantiate_struct(&mut self, dasar: &str, args: &[JrgType]) -> Result<String, String> {
        let params = self.param_struct(dasar)
            .ok_or_else(|| format!("struct `{}` tidak generik", dasar))?;
        if params.len() != args.len() {
            return Err(format!("struct `{}` butuh {} parameter tipe, diberi {}", dasar, params.len(), args.len()));
        }
        let nama = nama_instans(dasar, args);
        if self.struct_instans.contains_key(&nama) { return Ok(nama); }
        // Dicatat SEBELUM field-nya diturunkan: field boleh menyebut instans ini sendiri.
        self.struct_instans.insert(nama.clone(), (dasar.to_string(), args.to_vec()));
        let subst: TypeSubst = params.iter().cloned().zip(args.iter().cloned()).collect();
        let mut fields = Vec::new();
        for (pemilik, f) in self.tables.field_rata(dasar) {
            let t = if super::codegen::is_bare_array_type(&f.ty) {
                JrgType::Slice(Box::new(JrgType::Any))
            } else {
                self.resolve_type_with_subst(&f.ty, &subst).map_err(|e| super::galat::beri_span_bila_kosong(
                    format!("{}\ncatatan: dipakai sebagai tipe field `{}` di struct `{}`", e, f.name, pemilik), f.span))?
            };
            fields.push((f.name.clone(), t));
        }
        self.types.register_struct(StructLayout { name: nama.clone(), fields });
        Ok(nama)
    }

    /// Ikatan parameter tipe milik sebuah instans, dengan nama yang dipakai `impl` method-nya.
    /// `Self` di badan dan tanda tangan sebuah method: tipe PENERIMANYA, seperti di dua mesin
    /// lain (method warisan berjalan sebagai salinan milik tipe anak).
    pub(crate) fn ikat_diri(&self, tipe: &str, subst: &mut TypeSubst) {
        let t = if self.types.enums.contains_key(tipe) { JrgType::Enum(tipe.to_string()) } else { JrgType::Struct(tipe.to_string()) };
        subst.entry("Self".to_string()).or_insert(t);
    }

    pub(crate) fn ikat_generik_penerima(&self, tipe: &str, metode: &str, subst: &mut TypeSubst) {
        self.ikat_diri(tipe, subst);
        let Some((d, args)) = self.struct_instans.get(tipe) else { return };
        let nama = self.tables.param_impl.get(&(d.clone(), metode.to_string())).cloned()
            .or_else(|| self.param_struct(d)).unwrap_or_default();
        for (n, t) in nama.iter().zip(args.iter()) {
            subst.entry(n.clone()).or_insert_with(|| t.clone());
        }
    }

    /// Cocokkan tipe yang DITULIS dengan tipe sebuah nilai, mengikat parameter `params`
    /// yang belum terikat. Yang tidak memberi keterangan (array kosong, `nil`) dilewati.
    pub(crate) fn satukan(&self, te: &TypeExpr, t: &JrgType, params: &[String], subst: &mut TypeSubst) {
        match (te, t) {
            (_, JrgType::Never) | (_, JrgType::Nil) => {}
            (TypeExpr::Named(nt), _) if nt.args.is_empty() && params.contains(&nt.name) => {
                subst.entry(nt.name.clone()).or_insert_with(|| t.clone());
            }
            (TypeExpr::Slice(s), JrgType::Slice(e)) => self.satukan(&s.elem, e, params, subst),
            (TypeExpr::Slice(s), JrgType::Array(e, n)) if *n > 0 => self.satukan(&s.elem, e, params, subst),
            (TypeExpr::Array(a), JrgType::Array(e, n)) if *n > 0 => self.satukan(&a.elem, e, params, subst),
            (TypeExpr::Tuple(es), JrgType::Tuple(ts)) if es.len() == ts.len() => {
                for (e, t) in es.iter().zip(ts) { self.satukan(e, t, params, subst); }
            }
            (TypeExpr::Named(nt), JrgType::Enum(en)) if nt.name == "Option" && nt.args.len() == 1 => {
                if let Some(isi) = self.option_args.get(en).cloned() { self.satukan(&nt.args[0], &isi, params, subst); }
            }
            (TypeExpr::Named(nt), JrgType::Enum(en)) if nt.name == "Result" && nt.args.len() == 2 => {
                if let Some((a, b)) = self.result_args.get(en).cloned() {
                    self.satukan(&nt.args[0], &a, params, subst);
                    self.satukan(&nt.args[1], &b, params, subst);
                }
            }
            (TypeExpr::Named(nt), JrgType::Dict(v)) if nt.name == "dict" && !nt.args.is_empty() => {
                self.satukan(&nt.args[nt.args.len() - 1], v, params, subst);
            }
            (TypeExpr::Named(nt), JrgType::Struct(inst)) => {
                if let Some((d, args)) = self.struct_instans.get(inst).cloned() {
                    if d == nt.name && args.len() == nt.args.len() {
                        for (e, t) in nt.args.iter().zip(args.iter()) { self.satukan(e, t, params, subst); }
                    }
                }
            }
            (TypeExpr::Fn(f), JrgType::Fn(ps, r)) if f.params.len() == ps.len() => {
                for (e, t) in f.params.iter().zip(ps.iter()) { self.satukan(e, t, params, subst); }
                if let Some(rt) = &f.ret { self.satukan(rt, r, params, subst); }
            }
            (TypeExpr::Pointer(p), _) => self.satukan(&p.inner, t, params, subst),
            (TypeExpr::Ownership(o), _) => self.satukan(&o.inner, t, params, subst),
            _ => {}
        }
    }
}
