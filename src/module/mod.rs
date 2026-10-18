// src/module/mod.rs
// Module Resolver — resolusi path import ke file .jrg di filesystem

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ─── Namespace bawaan ─────────────────────────────────────────────────────
static NAMESPACE_BAWAAN: OnceLock<HashSet<String>> = OnceLock::new();

/// Nama namespace yang sudah ada di runtime, diturunkan dari registry.
pub fn namespace_bawaan() -> &'static HashSet<String> {
    NAMESPACE_BAWAAN.get_or_init(|| {
        let mut set: HashSet<String> = crate::vm::daftar_builtin()
            .iter()
            .filter_map(|n| n.split_once("::").map(|(ns, _)| ns.to_string()))
            .collect();
        // `py::`/`js::` cuma hidup di interpreter, jadi registry VM tidak
        // menyebutkan keduanya — tapi importnya harus tetap dilewati.
        set.insert("py".to_string());
        set.insert("js".to_string());
        set
    })
}

/// Identitas sebuah modul berkas: path kanoniknya. Dua import yang sampai ke berkas yang
/// sama — lewat alias atau path relatif yang berbeda — adalah SATU modul, dan dua berkas
/// bernama sama di direktori berbeda adalah dua modul.
pub fn identitas(path: &Path) -> String {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).display().to_string()
}

/// Akar MAYA penyunting tertanam: `jrgc --sunting` menjalankan `editor/jrgedit.jrg` dari
/// biner, dan `import "sunting/x"` di dalamnya dijawab dari `editor_src` — tata letak repo.
pub const AKAR_EDITOR: &str = "<jrgedit>";

/// Isi tertanam untuk `path` di bawah akar maya, kalau ada.
pub fn sumber_tertanam(path: &Path) -> Option<&'static str> {
    let rel = path.strip_prefix(AKAR_EDITOR).ok()?;
    crate::editor_src::source_of(&rel.to_string_lossy().replace('\\', "/"))
}

/// Isi berkas sebuah modul: dari tabel tertanam di bawah akar maya, dari disk selain itu.
pub fn baca_sumber(path: &Path) -> std::io::Result<String> {
    match sumber_tertanam(path) {
        Some(isi) => Ok(isi.to_string()),
        None => std::fs::read_to_string(path),
    }
}

/// Import yang melingkar. `rantai` adalah tumpukan pemuatan apa adanya: `a -> b -> a`.
pub fn pesan_impor_melingkar(rantai: &[String]) -> String {
    format!(
        "import muter-muter: {}\n  \
         bantuan: putus salah satu import di rantai itu, atau pindahkan bagian \
         yang dipakai bersama ke modul ketiga yang tidak mengimpor balik",
        rantai.join(" -> ")
    )
}

/// Identitas modul `std/…` yang tertanam di biner.
pub fn identitas_std(nama: &str) -> String {
    format!("<std/{}>", nama.replace("::", "/").replace('.', "/"))
}

// ─── ModuleResolver ───────────────────────────────────────────────────────

/// Resolusi path import ke file `.jrg` di filesystem.
#[derive(Debug, Clone)]
pub struct ModuleResolver {
    /// Urutan path yang dicari saat resolve import.
    pub search_paths: Vec<PathBuf>,
}

/// Direktori berkas yang ditulis tanpa path adalah `""`: ditampilkan dan
/// dibandingkan sebagai `.`, bukan sebagai entri kosong.
fn sebagai_tempat(p: &Path) -> &Path {
    if p.as_os_str().is_empty() { Path::new(".") } else { p }
}

impl ModuleResolver {
    /// Buat resolver dengan daftar search path. Tempat yang sama dibuang di
    /// sini, karena pesan "modul tidak ketemu" MENCETAK daftar ini.
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        let mut r = ModuleResolver { search_paths: Vec::with_capacity(search_paths.len()) };
        for p in search_paths { r.add_path(p); }
        r
    }

    /// Buat resolver dari satu root directory (project root).
    pub fn from_root(root: &Path) -> Self {
        Self::new(vec![root.to_path_buf()])
    }

    /// Tambahkan search path (mis. dir dari file yang melakukan import).
    pub fn add_path(&mut self, path: PathBuf) {
        if !self.search_paths.iter().any(|p| sebagai_tempat(p) == sebagai_tempat(&path)) {
            self.search_paths.push(path);
        }
    }

    /// Kalimat "modul tidak ketemu" — satu definisi untuk interpreter dan VM.
    pub fn pesan_tak_ketemu(&self, import_path: &str) -> String {
        let tempat: Vec<String> = self.search_paths.iter()
            .map(|p| sebagai_tempat(p).display().to_string()).collect();
        format!(
            "modul '{}' tidak ketemu. Yang sudah dicari: [{}]\n  \
             bantuan: nama file-nya harus `{}.jrg` (atau `{}/mod.jrg`) \
             relatif terhadap salah satu tempat di atas",
            import_path, tempat.join(", "), Self::jalur_impor(import_path), Self::jalur_impor(import_path)
        )
    }

    // ─── Static helpers ───────────────────────────────────────────────────

    /// Apakah `import_path` menyebut sebuah namespace bawaan?
    pub fn is_stdlib(import_path: &str) -> bool {
        if namespace_bawaan().contains(import_path) {
            return true;
        }
        match Self::std_module_name(import_path) {
            Some(nama) => namespace_bawaan().contains(nama),
            None => false,
        }
    }

    /// Kalau path ini berbentuk `std/<nama>` (atau `std::<nama>` / `std.<nama>`),
    /// kembalikan `<nama>`-nya.
    pub fn std_module_name(import_path: &str) -> Option<&str> {
        for awalan in ["std/", "std::", "std."] {
            if let Some(sisa) = import_path.strip_prefix(awalan) {
                if !sisa.is_empty() {
                    return Some(sisa);
                }
            }
        }
        None
    }

    /// Jalur sebuah import: `a.b` dan `a::b` jalur modul bertitik (`a/b`); yang memuat `/`
    /// jalur berkas apa adanya, karena titiknya milik `./`, `../`, atau nama berkasnya. Akhiran
    /// `.jrg` boleh ditulis — dulu `import "bantu.jrg"` disuruh mencari `bantu.jrg.jrg`.
    pub fn jalur_impor(import_path: &str) -> String {
        let p = import_path.strip_suffix(".jrg").unwrap_or(import_path);
        if p.contains('/') { p.to_string() } else { p.replace("::", "/").replace('.', "/") }
    }

    /// Nama modul dari import path: segmen terakhir jalurnya. `sub::alat`, `sub.alat`, dan
    /// `sub/alat` sama-sama `alat` — dulu dua yang pertama jadi alias yang tak bisa ditulis.
    pub fn module_name(import_path: &str) -> String {
        let jalur = Self::jalur_impor(import_path);
        jalur.rsplit('/').next().unwrap_or(&jalur).to_string()
    }

    /// Resolve import path ke file `.jrg` di filesystem.
    pub fn resolve(&self, import_path: &str) -> Option<PathBuf> {
        let normalized = Self::jalur_impor(import_path);

        for base in &self.search_paths {
            // Kandidat path yang dicoba
            let candidates: Vec<PathBuf> = vec![
                // Langsung: base/import_path.jrg
                base.join(format!("{}.jrg", normalized)),
                // Directory module: base/import_path/mod.jrg
                base.join(&normalized).join("mod.jrg"),
                // Directory entry: base/import_path/main.jrg
                base.join(&normalized).join("main.jrg"),
            ];

            for candidate in candidates {
                // Di bawah akar maya tidak ada disk: yang ada cuma tabel tertanam.
                if base.starts_with(AKAR_EDITOR) {
                    if sumber_tertanam(&candidate).is_some() { return Some(candidate); }
                    continue;
                }
                if candidate.exists() && candidate.is_file() {
                    // Canonicalize biar absolute path
                    if let Ok(canonical) = candidate.canonicalize() {
                        return Some(canonical);
                    }
                    return Some(candidate);
                }
            }
        }

        None
    }

    /// Cari semua file `.jrg` di bawah search paths (untuk autocomplete / LSP).
    pub fn discover_all(&self) -> Vec<(String, PathBuf)> {
        let mut results = Vec::new();
        for base in &self.search_paths {
            collect_jrg_files(base, base, &mut results);
        }
        results
    }
}

// ─── Internal helper ─────────────────────────────────────────────────────

fn collect_jrg_files(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jrg_files(base, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jrg") {
            // Buat import path relatif ke base
            if let Ok(rel) = path.strip_prefix(base) {
                let import_path = rel
                    .to_string_lossy()
                    .replace('\\', "/")
                    .replace(".jrg", "");
                out.push((import_path, path));
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name() {
        assert_eq!(ModuleResolver::module_name("utils/math_ext"), "math_ext");
        assert_eq!(ModuleResolver::module_name("helpers"), "helpers");
        assert_eq!(ModuleResolver::module_name("std/fmt"), "fmt");
        assert_eq!(ModuleResolver::module_name("a/b/c"), "c");
        assert_eq!(ModuleResolver::module_name("sub::alat"), "alat");
        assert_eq!(ModuleResolver::module_name("sub.alat"), "alat");
        assert_eq!(ModuleResolver::module_name("./bantu"), "bantu");
        assert_eq!(ModuleResolver::module_name("bantu.jrg"), "bantu");
        assert_eq!(ModuleResolver::module_name("../lib/x.jrg"), "x");
        assert_eq!(ModuleResolver::jalur_impor("../lib/x.jrg"), "../lib/x");
        assert_eq!(ModuleResolver::jalur_impor("a.b::c"), "a/b/c");
    }

    #[test]
    fn test_is_stdlib() {
        assert!(ModuleResolver::is_stdlib("fmt"));
        assert!(ModuleResolver::is_stdlib("math"));
        assert!(ModuleResolver::is_stdlib("arr"));
        assert!(ModuleResolver::is_stdlib("str"));
        assert!(ModuleResolver::is_stdlib("dict"));
        assert!(!ModuleResolver::is_stdlib("utils/helper"));
        assert!(!ModuleResolver::is_stdlib("mymodule"));
        // `std/<namespace-bawaan>` cuma ejaan lain untuk namespace yang sama
        // (`examples/goroutines.jrg` mengimpor persis begini) — harus TETAP …
        assert!(ModuleResolver::is_stdlib("std/fmt"));
        assert!(ModuleResolver::is_stdlib("std/math"));
        assert!(ModuleResolver::is_stdlib("std::arr"));
        assert!(ModuleResolver::is_stdlib("std.dict"));
        // `std/...` yang BUKAN namespace bawaan: itu modul yang ditulis
        // dalam JRG dan harus benar-benar dimuat, bukan dilewati begitu
        // saja.
        assert!(!ModuleResolver::is_stdlib("std/option"));
        assert!(!ModuleResolver::is_stdlib("std/ngarang"));
    }

    #[test]
    fn nama_modul_std_dikenali_dari_tiga_bentuk_penulisan() {
        assert_eq!(ModuleResolver::std_module_name("std/option"), Some("option"));
        assert_eq!(ModuleResolver::std_module_name("std::option"), Some("option"));
        assert_eq!(ModuleResolver::std_module_name("std.option"), Some("option"));
        assert_eq!(ModuleResolver::std_module_name("option"), None);
        assert_eq!(ModuleResolver::std_module_name("std/"), None);
    }

    #[test]
    fn test_normalize() {
        let r = ModuleResolver::new(vec![]);
        // Should try both :: and / separators
        // (resolve returns None when file not found, but shouldn't panic)
        let _ = r.resolve("my::module");
        let _ = r.resolve("my.module");
    }
}
