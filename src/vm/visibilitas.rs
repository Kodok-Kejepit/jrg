// src/vm/visibilitas.rs — vm/visibilitas.rs
// Penegakan `pub`/privat pada field struct — DIPAKAI BERSAMA oleh backend

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// nama struct → (induknya, nama field yang privat di struct ITU sendiri)
type Tabel = HashMap<String, (Option<String>, Vec<String>)>;

fn tabel() -> &'static Mutex<Tabel> {
    static T: OnceLock<Mutex<Tabel>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// nama struct → (induknya, SEMUA nama field yang dideklarasikannya)
type TabelField = HashMap<String, (Option<String>, Vec<String>)>;

fn tabel_field() -> &'static Mutex<TabelField> {
    static T: OnceLock<Mutex<TabelField>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Nama field yang DIDEKLARASIKAN sebuah struct, termasuk warisannya — atau
/// `None` kalau namanya bukan struct yang dideklarasikan program.
pub fn field_dideklarasikan(nama: &str) -> Option<Vec<String>> {
    let t = tabel_field().lock().unwrap_or_else(|p| p.into_inner());
    t.get(nama)?;
    let mut hasil: Vec<String> = Vec::new();
    let mut kini = Some(nama.to_string());
    let mut lihat: Vec<String> = Vec::new();
    while let Some(n) = kini {
        if lihat.iter().any(|x| x == &n) || lihat.len() >= 64 { break; }
        lihat.push(n.clone());
        let Some((induk, fields)) = t.get(&n) else { break };
        for f in fields {
            if !hasil.iter().any(|x| x == f) { hasil.push(f.clone()); }
        }
        kini = induk.clone();
    }
    Some(hasil)
}

/// Nama tiap struct yang dideklarasikan program, untuk saran ejaan.
pub fn nama_struct() -> Vec<String> {
    tabel_field().lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect()
}

/// Dipanggil compiler sekali per unit kompilasi.
pub fn pasang(t: Tabel) {
    let mut g = tabel().lock().unwrap_or_else(|p| p.into_inner());
    g.extend(t);
}

/// Isi tabel dari deklarasi struct apa adanya.
pub fn pasang_dari_deklarasi<'a>(decls: impl Iterator<Item = &'a crate::parser::StructDecl>) {
    let decls: Vec<&crate::parser::StructDecl> = decls.collect();
    {
        let mut g = tabel_field().lock().unwrap_or_else(|p| p.into_inner());
        for d in &decls {
            g.insert(d.name.clone(),
                     (d.parent.clone(),
                      d.fields.iter().map(|f| f.name.clone()).collect()));
        }
    }
    let decls = decls.into_iter();
    pasang(
        decls
            .map(|d| {
                (
                    d.name.clone(),
                    (
                        d.parent.clone(),
                        d.fields.iter()
                            .filter(|f| matches!(f.vis, crate::parser::Visibility::Private))
                            .map(|f| f.name.clone())
                            .collect::<Vec<String>>(),
                    ),
                )
            })
            .collect(),
    );
}

/// Apakah nama field ini pernah dideklarasikan privat oleh struct mana pun?
pub fn perlu_diperiksa(field: &str) -> bool {
    tabel()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .any(|(_, privat)| privat.iter().any(|f| f == field))
}

/// Rantai pewarisan sebuah struct, dari dirinya sendiri ke atas.
fn garis_keturunan(nama: &str) -> Vec<String> {
    let t = tabel().lock().unwrap_or_else(|p| p.into_inner());
    let mut rantai = Vec::new();
    let mut kini = Some(nama.to_string());
    while let Some(n) = kini {
        if rantai.iter().any(|x: &String| x == &n) || rantai.len() >= 64 {
            break;
        }
        kini = t.get(&n).and_then(|(induk, _)| induk.clone());
        rantai.push(n);
    }
    rantai
}

/// Apakah `anak` adalah `leluhur` sendiri, atau menurun darinya?
/// Tabelnya global (bukan `thread_local`), jadi jawabannya sama di goroutine.
pub fn menurun_dari(anak: &str, leluhur: &str) -> bool {
    anak == leluhur || garis_keturunan(anak).iter().any(|n| n == leluhur)
}

/// Boleh tidak mengakses `field` pada objek bertipe `tipe_objek`, dari dalam
/// method bertipe `tipe_self`?
pub fn periksa(tipe_objek: &str, field: &str, tipe_self: Option<&str>) -> Result<(), String> {
    let keturunan = garis_keturunan(tipe_objek);
    // Deklarasi TERDEKAT yang menentukan, seperti acuan: anak yang mendeklarasikan ulang
    // field induk sebagai `pub` membukanya. Dulu privat di mana pun sepanjang rantai menang.
    let pemilik = {
        let semua = tabel_field().lock().unwrap_or_else(|p| p.into_inner());
        let t = tabel().lock().unwrap_or_else(|p| p.into_inner());
        let mut ketemu = None;
        for n in &keturunan {
            if t.get(n).map(|(_, privat)| privat.iter().any(|f| f == field)).unwrap_or(false) {
                ketemu = Some(n.clone());
                break;
            }
            if semua.get(n).map(|(_, f)| f.iter().any(|x| x == field)).unwrap_or(false) { break; }
        }
        ketemu
    };
    // Field yang tidak dideklarasikan privat di mana pun sepanjang rantai
    // (mis. key dict, atau struct bikinan native) tetap publik.
    let Some(pemilik) = pemilik else { return Ok(()) };

    if let Some(st) = tipe_self {
        if keturunan.iter().any(|n| n == st) || garis_keturunan(st).iter().any(|n| n == tipe_objek) {
            return Ok(());
        }
    }
    Err(format!(
        "field `{}` di `{}` bersifat private — cuma bisa diakses dari dalam method \
         tipe itu sendiri atau turunannya. Tambahkan `pub` di depan field itu di \
         deklarasi struct kalau memang harus publik.",
        field, pemilik
    ))
}

/// Opcode yang tepat untuk membaca sebuah field.
pub fn opcode_get(field: &str) -> crate::vm::opcode::OpCode {
    if perlu_diperiksa(field) {
        crate::vm::opcode::OpCode::GetFieldPrivat
    } else {
        crate::vm::opcode::OpCode::GetField
    }
}

/// Pasangan `opcode_get` untuk penulisan.
pub fn opcode_set(field: &str) -> crate::vm::opcode::OpCode {
    if perlu_diperiksa(field) {
        crate::vm::opcode::OpCode::SetFieldPrivat
    } else {
        crate::vm::opcode::OpCode::SetField
    }
}
