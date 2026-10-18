// src/llvm/tulis_modul.rs — "bisakah ekspresi ini menulis `var` modul?", untuk nilai milik
// `var` modul yang dibaca LEWAT penunjuk (penerima method, operan kiri). Konservatif: ragu = ya.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::parser::Expr;

use super::codegen::Codegen;

/// Nama yang disebut sebuah teks AST, dan apakah ia menulis `var` modul secara langsung.
struct Jejak { sebut: HashSet<String>, tulis: bool, buta: bool }

/// Dibaca dari bentuk `Debug` AST, seperti `berkonkurensi` di `llvm::generate_ir`: murah, dan
/// kesalahannya cuma ke satu arah — nama yang tersebut lebih banyak berarti salinan lebih banyak.
fn jejak(teks: &str, var_modul: &HashSet<String>, fungsi: &HashSet<String>) -> Jejak {
    let ambil = |dari: &str, awalan: &str| -> Vec<String> {
        dari.match_indices(awalan).filter_map(|(i, _)| {
            let sisa = &dari[i + awalan.len()..];
            sisa.find('"').map(|t| sisa[..t].to_string())
        }).collect()
    };
    let mut sebut: HashSet<String> = ambil(teks, "Ident(\"").into_iter().collect();
    sebut.extend(ambil(teks, "method: \""));
    for (i, _) in teks.match_indices("PathExpr { segments: [") {
        let sisa = &teks[i..];
        // Segmen TERAKHIR `["a", "f"]` adalah nama fungsinya.
        if let Some(n) = sisa.find(']').and_then(|t| sisa[..t].rsplit('"').nth(1)) { sebut.insert(n.to_string()); }
    }
    let mut tulis = false;
    for (i, _) in teks.match_indices("Assign(AssignStmt { targets: [") {
        let sisa = &teks[i..];
        let akhir = sisa.find("], op: ").unwrap_or(sisa.len());
        if ambil(&sisa[..akhir], "Ident(\"").iter().any(|n| var_modul.contains(n)) { tulis = true; }
    }
    // Pemanggilan atas sesuatu yang bukan nama fungsi (closure di variabel, field, hasil
    // pemanggilan): tidak diketahui apa isinya.
    let buta = teks.match_indices("Call { func: ").any(|(i, _)| {
        let sisa = &teks[i + "Call { func: ".len()..];
        if let Some(n) = sisa.strip_prefix("Ident(\"") {
            let n = &n[..n.find('"').unwrap_or(0)];
            !fungsi.contains(n) && !crate::vm::ada_builtin(n)
        } else { !sisa.starts_with("PathExpr") }
    });
    Jejak { sebut, tulis, buta }
}

impl Codegen {
    /// Nama fungsi dan method milik program yang (lewat panggilan berikutnya) bisa menulis
    /// `var` modul. Dihitung sekali per program.
    fn penulis_modul(&mut self) -> Rc<HashSet<String>> {
        if let Some(p) = &self.penulis_modul { return p.clone(); }
        let var_modul: HashSet<String> = self.tables.vars.iter()
            .map(|v| super::modul::tampil(&v.name).to_string()).collect();
        let mut badan: HashMap<String, Vec<String>> = HashMap::new();
        for (k, d) in &self.tables.functions {
            badan.entry(super::modul::tampil(k).to_string()).or_default().push(format!("{:?}", d.body));
        }
        for ms in self.tables.impls.values() {
            for (n, d) in ms { badan.entry(n.clone()).or_default().push(format!("{:?}", d.body)); }
        }
        let fungsi: HashSet<String> = badan.keys().cloned().collect();
        let mut panggil: HashMap<String, HashSet<String>> = HashMap::new();
        let mut penulis: HashSet<String> = HashSet::new();
        for (n, teks) in &badan {
            for t in teks {
                let j = jejak(t, &var_modul, &fungsi);
                if j.tulis || j.buta { penulis.insert(n.clone()); }
                panggil.entry(n.clone()).or_default().extend(j.sebut.into_iter().filter(|s| fungsi.contains(s)));
            }
        }
        loop {
            let tambah: Vec<String> = panggil.iter()
                .filter(|(n, cs)| !penulis.contains(*n) && cs.iter().any(|c| penulis.contains(c)))
                .map(|(n, _)| n.clone()).collect();
            if tambah.is_empty() { break; }
            penulis.extend(tambah);
        }
        let p = Rc::new(penulis);
        self.penulis_modul = Some(p.clone());
        p
    }

    /// Bisakah menurunkan `e` menulis sebuah `var` modul?
    pub(crate) fn bisa_menulis_modul(&mut self, e: &Expr) -> bool {
        let penulis = self.penulis_modul();
        let var_modul: HashSet<String> = self.tables.vars.iter()
            .map(|v| super::modul::tampil(&v.name).to_string()).collect();
        let fungsi: HashSet<String> = self.tables.functions.keys()
            .map(|k| super::modul::tampil(k).to_string())
            .chain(self.tables.impls.values().flat_map(|m| m.keys().cloned())).collect();
        let j = jejak(&format!("{:?}", e), &var_modul, &fungsi);
        j.tulis || j.buta || j.sebut.iter().any(|s| penulis.contains(s))
    }

    /// Apakah method `nama` milik program bisa menulis `var` modul?
    pub(crate) fn metode_menulis_modul(&mut self, nama: &str) -> bool {
        self.penulis_modul().contains(nama)
    }

    /// `x`, `x.a`, `x[i]` yang berakar pada `var` modul unit ini, bukan pada lokal.
    pub(crate) fn akar_var_modul(&self, e: &Expr) -> bool {
        let Some(n) = Self::nama_akar(e) else { return false };
        if self.lookup_local(n).is_some() { return false; }
        let kunci = super::modul::kunci(self.unit_kini, n);
        self.tables.vars.iter().any(|v| v.name == kunci)
    }
}
