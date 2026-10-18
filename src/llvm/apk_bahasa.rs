// src/llvm/apk_bahasa.rs — penjalan bahasa lain di dalam APK (`[android] bahasa = ["lua", ...]`):
// sumber resminya diunduh SEKALI ke ~/.cache/jrgm/, sidiknya diperiksa, lalu dikompilasi per ABI
// dengan clang NDK jadi executable `lib<nama>_exe.so`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Berkas sumber sebuah resep, relatif akar arsipnya.
pub enum Sumber {
    /// Semua `*.c` di sebuah direktori, kecuali yang disebut.
    Semua { dir: &'static str, kecuali: &'static [&'static str] },
    Daftar(&'static [&'static str]),
}

pub struct Resep {
    pub id: &'static str,
    pub label: &'static str,
    pub versi: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    /// Direktori teratas di dalam arsipnya.
    pub akar: &'static str,
    pub sumber: Sumber,
    pub bendera: &'static [&'static str],
    /// Nama berkas di `lib/<abi>/`: Android cuma memasang `lib*.so`, executable pun begitu.
    pub keluaran: &'static str,
    /// Berkas C milik JRG yang ikut dikompilasi (nama, isi).
    pub pengantar: Option<(&'static str, &'static str)>,
}

pub const RESEP: &[Resep] = &[
    Resep {
        id: "lua", label: "Lua", versi: "5.4.7",
        url: "https://www.lua.org/ftp/lua-5.4.7.tar.gz",
        sha256: "9fbf5e28ef86c69858f6d3d34eccc32e911c1a28b4120ff3e84aaa70cfbf1e30",
        akar: "lua-5.4.7",
        sumber: Sumber::Semua { dir: "src", kecuali: &["luac.c"] },
        bendera: &["-DLUA_USE_POSIX"],
        keluaran: "liblua_exe.so",
        pengantar: None,
    },
    Resep {
        id: "js", label: "JavaScript (QuickJS)", versi: "2024-01-13",
        url: "https://bellard.org/quickjs/quickjs-2024-01-13.tar.xz",
        sha256: "3c4bf8f895bfa54beb486c8d1218112771ecfc5ac3be1036851ef41568212e03",
        akar: "quickjs-2024-01-13",
        sumber: Sumber::Daftar(&["quickjs.c", "libregexp.c", "libunicode.c", "cutils.c", "quickjs-libc.c", "libbf.c"]),
        bendera: &["-D_GNU_SOURCE", "-DCONFIG_VERSION=\"2024-01-13\""],
        keluaran: "libqjs_exe.so",
        pengantar: Some(("jrg_qjs.c", include_str!("android/jrg_qjs.c"))),
    },
    Resep {
        id: "sql", label: "SQL (SQLite)", versi: "3.46.1",
        url: "https://www.sqlite.org/2024/sqlite-amalgamation-3460100.zip",
        sha256: "77823cb110929c2bcb0f5d48e4833b5c59a8a6e40cdea3936b99e199dbbe5784",
        akar: "sqlite-amalgamation-3460100",
        sumber: Sumber::Daftar(&["shell.c", "sqlite3.c"]),
        bendera: &["-DSQLITE_OMIT_LOAD_EXTENSION", "-DSQLITE_THREADSAFE=0"],
        keluaran: "libsqlite3_exe.so",
        pengantar: None,
    },
];

pub fn resep(id: &str) -> Result<&'static Resep, String> {
    RESEP.iter().find(|r| r.id == id).ok_or_else(|| format!(
        "bahasa `{}` di [android] bahasa tidak dikenal\n  bantuan: yang ada {}", id,
        RESEP.iter().map(|r| format!("`{}` ({})", r.id, r.label)).collect::<Vec<_>>().join(", ")))
}

fn tembolok() -> Result<PathBuf, String> {
    let h = std::env::var_os("HOME").ok_or("HOME tidak diatur; jrgm menyimpan sumber bahasa di ~/.cache/jrgm/")?;
    Ok(PathBuf::from(h).join(".cache/jrgm"))
}

fn heksa(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

fn jalan(cmd: &mut Command, apa: &str) -> Result<(), String> {
    let o = crate::putar::jalankan(cmd, apa).map_err(|e| format!("gagal {}: {}", apa, e))?;
    if o.status.success() { return Ok(()); }
    Err(format!("gagal {}:\n{}", apa, String::from_utf8_lossy(&o.stderr).trim_end()))
}

/// Akar sumber sebuah resep: diunduh kalau arsipnya belum ada, DITOLAK kalau sidiknya lain.
fn sumber_resep(r: &Resep, verbose: bool) -> Result<PathBuf, String> {
    let d = tembolok()?.join("sumber");
    let akar = d.join(r.akar);
    if akar.join(".utuh").is_file() { return Ok(akar); }
    std::fs::create_dir_all(&d).map_err(|e| format!("gagal membuat {}: {}", d.display(), e))?;
    let arsip = d.join(r.url.rsplit('/').next().unwrap_or(r.akar));
    if !arsip.is_file() {
        eprintln!("[jrgm] mengunduh {} {} ({})", r.label, r.versi, r.url);
        let sementara = arsip.with_extension("unduh");
        jalan(Command::new("curl").args(["-fsSL", "--max-time", "600", "-o"]).arg(&sementara).arg(r.url),
              &format!("mengunduh {}", r.url))
            .map_err(|e| format!("{}\n  bantuan: tanpa jaringan, taruh sendiri arsipnya di {}", e, arsip.display()))?;
        std::fs::rename(&sementara, &arsip).map_err(|e| e.to_string())?;
    }
    let isi = std::fs::read(&arsip).map_err(|e| format!("gagal membaca {}: {}", arsip.display(), e))?;
    let sidik = heksa(&crate::dok::crypto::sha256(&isi));
    if sidik != r.sha256 {
        return Err(format!("sidik {} tidak cocok, jadi tidak dipakai\n  SHA-256 seharusnya {}\n  yang didapat       {}\n  \
                            bantuan: hapus berkas itu supaya diunduh ulang", arsip.display(), r.sha256, sidik));
    }
    if verbose { eprintln!("[jrgm] sidik {} cocok", arsip.display()); }
    let _ = std::fs::remove_dir_all(&akar);
    if arsip.extension().map_or(false, |e| e == "zip") {
        jalan(Command::new("unzip").arg("-qo").arg(&arsip).arg("-d").arg(&d), "membuka arsip (unzip)")?;
    } else {
        jalan(Command::new("tar").arg("xf").arg(&arsip).arg("-C").arg(&d), "membuka arsip (tar)")?;
    }
    std::fs::write(akar.join(".utuh"), r.sha256).map_err(|e| format!("arsip {} tidak berisi `{}`: {}", arsip.display(), r.akar, e))?;
    Ok(akar)
}

fn berkas_sumber(r: &Resep, akar: &Path) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = match &r.sumber {
        Sumber::Daftar(d) => d.iter().map(|f| akar.join(f)).collect(),
        Sumber::Semua { dir, kecuali } => {
            let d = akar.join(dir);
            let mut v: Vec<PathBuf> = std::fs::read_dir(&d).map_err(|e| format!("gagal membaca {}: {}", d.display(), e))?
                .flatten().map(|e| e.path())
                .filter(|p| p.extension().map_or(false, |e| e == "c"))
                .filter(|p| !kecuali.iter().any(|k| p.file_name().map_or(false, |n| n == *k)))
                .collect();
            v.sort();
            v
        }
    };
    if let Some((nama, _)) = r.pengantar { v.push(akar.join(nama)); }
    match v.iter().find(|f| !f.is_file()) {
        Some(f) => Err(format!("sumber {} tidak lengkap: `{}` tidak ada", r.label, f.display())),
        None => Ok(v),
    }
}

/// Executable sebuah bahasa untuk satu ABI. Hasilnya disimpan per versi dan ABI, jadi kompilasi
/// (SQLite: dua menit) cuma terjadi sekali.
pub fn bangun(id: &str, abi: &str, sasaran: &str, ndk_bin: &Path, verbose: bool) -> Result<PathBuf, String> {
    let r = resep(id)?;
    // Pengantar milik JRG ikut menentukan hasilnya, jadi sidiknya masuk nama direktori simpanan.
    let sidik = r.pengantar.map(|(_, isi)| format!("-{}", &heksa(&crate::dok::crypto::sha256(isi.as_bytes()))[..8])).unwrap_or_default();
    let d = tembolok()?.join("program").join(abi).join(format!("{}-{}{}", r.id, r.versi, sidik));
    let out = d.join(r.keluaran);
    if out.is_file() { return Ok(out); }
    let akar = sumber_resep(r, verbose)?;
    if let Some((nama, isi)) = r.pengantar {
        std::fs::write(akar.join(nama), isi).map_err(|e| format!("gagal menulis {}: {}", nama, e))?;
    }
    let sumber = berkas_sumber(r, &akar)?;
    std::fs::create_dir_all(&d).map_err(|e| format!("gagal membuat {}: {}", d.display(), e))?;
    eprintln!("[jrgm] mengompilasi {} {} untuk {} (sekali saja)", r.label, r.versi, abi);
    let sementara = d.join("belum");
    let mut cmd = Command::new(ndk_bin.join("clang"));
    cmd.arg(sasaran).args(["-O2", "-fPIE", "-pie", "-w", "-s", "-Wl,-z,max-page-size=16384"]).args(r.bendera)
       .arg("-I").arg(&akar).args(&sumber).args(["-lm", "-ldl", "-o"]).arg(&sementara);
    if verbose { eprintln!("[jrgm] {:?}", cmd); }
    jalan(&mut cmd, &format!("mengompilasi {} untuk {}", r.label, abi))?;
    std::fs::rename(&sementara, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn resep_utuh_dan_tidak_ganda() {
        for (i, r) in RESEP.iter().enumerate() {
            assert!(r.sha256.len() == 64 && r.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{}", r.id);
            assert!(r.url.starts_with("https://"), "{}", r.id);
            assert!(r.keluaran.starts_with("lib") && r.keluaran.ends_with("_exe.so"), "{}", r.id);
            assert!(RESEP[..i].iter().all(|l| l.id != r.id && l.keluaran != r.keluaran), "{} ganda", r.id);
        }
        assert!(resep("cobol").err().map_or(false, |e| e.contains("`lua`")));
    }
}
