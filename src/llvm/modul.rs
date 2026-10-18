// src/llvm/modul.rs — pemuat modul untuk backend LLVM: tiap berkas sumber jadi satu UNIT
// bernomor (0 = berkas utama).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::lexer::{lex, SourceFile};
use crate::module::ModuleResolver;
use crate::parser::{parse, ParsedFile};

/// Satu berkas sumber yang ikut dikompilasi.
pub struct UnitLlvm {
    pub file: ParsedFile,
    /// Alias yang ditulis berkas INI -> nomor unit; tidak ada visibilitas transitif.
    pub alias: HashMap<String, usize>,
    pub nama: String,
    pub file_id: u32,
}

/// Awalan kunci tabel untuk nama tingkat atas milik unit `n` (`$` tidak bisa ditulis di JRG).
pub fn kunci(n: usize, nama: &str) -> String {
    if n == 0 { nama.to_string() } else { format!("m{}${}", n, nama) }
}

/// Nama seperti yang DITULIS pemakainya: awalan `mN$` tidak pernah sampai ke sebuah pesan.
pub fn tampil(kunci: &str) -> &str {
    if unit_dari(kunci) > 0 { kunci.split_once('$').map(|(_, n)| n).unwrap_or(kunci) } else { kunci }
}

/// Nomor unit dari sebuah kunci atau simbol ber-awalan `mN$`; 0 kalau tanpa awalan.
pub fn unit_dari(kunci: &str) -> usize {
    let Some(sisa) = kunci.strip_prefix('m') else { return 0 };
    let Some((angka, _)) = sisa.split_once('$') else { return 0 };
    angka.parse().unwrap_or(0)
}

struct Pemuat {
    unit: Vec<Option<UnitLlvm>>,
    selesai: HashMap<String, usize>,
    sedang: Vec<(String, String)>,
    /// Nama tipe dan trait milik unitnya, sama dengan dua mesin lain (`crate::ruang_tipe`).
    ruang: crate::ruang_tipe::Ruang,
}

/// Nama unit lapis ketiga: builtin pustaka yang ditulis dalam JRG (`lapis.jrg`).
pub const NAMA_LAPIS: &str = "<lapis>";

/// Telusuri `import` berkas utama secara rekursif. Unit 0 adalah berkas utama sendiri, dan
/// unit TERAKHIR selalu lapis ketiga — di ujung, supaya nomor unit milik program tidak bergeser.
pub fn muat(utama: &ParsedFile, jalur: &[PathBuf]) -> Result<Vec<UnitLlvm>, String> {
    use crate::ruang_tipe::UNIT_UTAMA;
    let mut p = Pemuat { unit: vec![None], selesai: HashMap::new(), sedang: Vec::new(), ruang: Default::default() };
    p.ruang.klaim(UNIT_UTAMA, UNIT_UTAMA, utama);
    let (alias, daftar) = p.impor(utama, jalur)?;
    let mut utama = utama.clone();
    p.ruang.olah(UNIT_UTAMA, UNIT_UTAMA, &mut utama, &daftar)
        .map_err(|(pesan, span)| super::galat::tandai(span, pesan))?;
    p.unit[0] = Some(UnitLlvm { file: utama, alias, nama: "<utama>".into(), file_id: 0 });
    let mut unit: Vec<UnitLlvm> = p.unit.into_iter().flatten().collect();
    unit.push(unit_lapis()?);
    Ok(unit)
}

fn unit_lapis() -> Result<UnitLlvm, String> {
    let sumber = include_str!("lapis.jrg").to_string();
    let file_id = crate::diag::register_module_source(NAMA_LAPIS.to_string(), sumber.clone());
    let sf = SourceFile { id: file_id, name: NAMA_LAPIS.to_string(), src: sumber };
    let tokens = lex(&sf).map_err(|e| format!("lapis.jrg: {}", e.msg))?;
    let (file, errors) = parse(tokens, false);
    if let Some(e) = errors.first() { return Err(format!("lapis.jrg: {}", e.msg)); }
    Ok(UnitLlvm { file, alias: HashMap::new(), nama: NAMA_LAPIS.to_string(), file_id })
}

impl Pemuat {
    /// Peta alias -> nomor unit, dan `(alias, identitas)` tiap import untuk `ruang_tipe`.
    fn impor(&mut self, file: &ParsedFile, jalur: &[PathBuf]) -> Result<(HashMap<String, usize>, Vec<(String, String)>), String> {
        let resolver = ModuleResolver::new(jalur.to_vec());
        let mut peta = HashMap::new();
        let mut daftar: Vec<(String, String)> = Vec::new();
        for imp in &file.imports {
            // Baris `import` milik sebuah MODUL membawa nomor berkasnya.
            let tandai = |m: String| super::galat::beri_berkas(
                super::galat::tandai(imp.span, m), imp.span.file_id);
            // Urutan dan kalimatnya sama dengan dua mesin lain: `std/` tertanam, lalu berkas
            // nyata, dan namespace bawaan (`import "math"`) dilewati kalau tidak ada berkasnya.
            let (id, nama, sumber, induk) = match ModuleResolver::std_module_name(&imp.path) {
                Some(n) => match crate::interpreter::stdlib_src::source_of(n) {
                    Some(src) => (crate::module::identitas_std(n), format!("<std/{}>", n), src.to_string(), None),
                    None if ModuleResolver::is_stdlib(&imp.path) => continue,
                    None => return Err(tandai(crate::interpreter::stdlib_src::pesan_modul_std_tak_ada(&imp.path))),
                },
                None => {
                    let Some(jalan) = resolver.resolve(&imp.path) else {
                        if ModuleResolver::is_stdlib(&imp.path) { continue; }
                        return Err(tandai(resolver.pesan_tak_ketemu(&imp.path)));
                    };
                    let isi = crate::module::baca_sumber(&jalan)
                        .map_err(|e| tandai(format!("tidak bisa baca '{}': {}", jalan.display(), e)))?;
                    let induk = jalan.parent().map(|d| d.to_path_buf());
                    (crate::module::identitas(&jalan), jalan.display().to_string(), isi, induk)
                }
            };
            let alias = imp.alias.clone().unwrap_or_else(|| ModuleResolver::module_name(&imp.path));
            daftar.push((alias.clone(), id.clone()));
            if let Some(&n) = self.selesai.get(&id) {
                peta.insert(alias, n);
                continue;
            }
            if let Some(mulai) = self.sedang.iter().position(|(i, _)| *i == id) {
                let mut rantai: Vec<String> = self.sedang[mulai..].iter().map(|(_, n)| n.clone()).collect();
                rantai.push(alias);
                return Err(tandai(crate::module::pesan_impor_melingkar(&rantai)));
            }
            let file_id = crate::diag::register_module_source(nama.clone(), sumber.clone());
            let sf = SourceFile { id: file_id, name: nama.clone(), src: sumber };
            let galat = |g: Vec<(String, crate::lexer::Span)>| {
                let (pesan, span) = crate::pipa::galat_parse_modul(&sf.src, &g);
                format!("[{}:{}@{}] {}", span.lo.0, span.hi.0, file_id, pesan)
            };
            let tokens = lex(&sf).map_err(|e| galat(vec![(e.msg, e.span)]))?;
            let (mut modul, errors) = parse(tokens, false);
            if !errors.is_empty() {
                return Err(galat(errors.into_iter().map(|e| (e.msg, e.span)).collect()));
            }
            // Direktori modul dicari lebih dulu untuk import miliknya sendiri.
            let mut dalam = jalur.to_vec();
            if let Some(d) = induk { dalam.insert(0, d); }
            let nomor = self.unit.len();
            self.unit.push(None);
            self.sedang.push((id.clone(), alias.clone()));
            let (alias_modul, daftar_modul) = self.impor(&modul, &dalam)?;
            self.sedang.pop();
            self.ruang.olah(&id, &ModuleResolver::module_name(&imp.path), &mut modul, &daftar_modul)
                .map_err(|(pesan, span)| super::galat::beri_berkas(super::galat::tandai(span, pesan), span.file_id))?;
            self.unit[nomor] = Some(UnitLlvm { file: modul, alias: alias_modul, nama, file_id });
            self.selesai.insert(id, nomor);
            peta.insert(alias, nomor);
        }
        Ok((peta, daftar))
    }
}
