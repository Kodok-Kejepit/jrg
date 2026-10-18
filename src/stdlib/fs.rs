// src/stdlib/fs.rs
// Filesystem native functions

use crate::vm::vm::Vm;
use crate::vm::value::VmValue;
use std::sync::Arc;

pub fn register(vm: &mut Vm) {
    use VmValue::*;

    // Direktori sementara milik sistem (`TMPDIR`, atau `/tmp`).
    vm.def("fs::temp_dir", |_a| Ok(Str(Arc::new(
        std::env::temp_dir().display().to_string()))));

    // Jalur absolut secara LEKSIKAL: cwd digabung, lalu `.`/`..` diratakan.
    vm.def("fs::abs", |a| match a.get(0) {
        Some(Str(p)) => {
            let mut jalur = std::path::PathBuf::new();
            let masuk = std::path::Path::new(p.as_str());
            if !masuk.is_absolute() {
                jalur = std::env::current_dir()
                    .map_err(|e| format!("fs::abs: tidak bisa baca direktori kerja: {}", crate::host::galat_io(&e)))?;
            }
            for bagian in masuk.components() {
                use std::path::Component::*;
                match bagian {
                    CurDir => {}
                    ParentDir => { jalur.pop(); }
                    lain => jalur.push(lain.as_os_str()),
                }
            }
            Ok(Str(Arc::new(jalur.display().to_string())))
        }
        _ => Err("fs::abs(path: string) -> string".to_string()),
    });

    vm.def("fs::is_symlink", |a| match a.get(0) {
        Some(Str(p)) => Ok(Bool(std::fs::symlink_metadata(p.as_str())
            .map(|m| m.file_type().is_symlink()).unwrap_or(false))),
        _ => Err("fs::is_symlink(path: string) -> bool".to_string()),
    });

    // Dicoba SUNGGUHAN — buka untuk ditulis tanpa memotong isinya — karena bit izin saja tidak
    // menjawab untuk root, ACL, atau sistem berkas yang dipasang baca-saja (`/proc`).
    vm.def("fs::is_writable", |a| match a.get(0) {
        Some(Str(p)) => {
            let jalur = std::path::Path::new(p.as_str());
            Ok(Bool(jalur.is_file() && std::fs::OpenOptions::new().write(true).open(jalur).is_ok()))
        }
        _ => Err("fs::is_writable(path: string) -> bool".to_string()),
    });

    // Seluruh BERKAS di bawah sebuah direktori, rekursif.
    vm.def("fs::walk", |a| match a.get(0) {
        Some(Str(p)) => {
            fn telusuri(d: &std::path::Path, sisa: u32, keluar: &mut Vec<VmValue>) {
                if sisa == 0 { return }
                let Ok(isi) = std::fs::read_dir(d) else { return };
                let mut anak: Vec<std::path::PathBuf> =
                    isi.flatten().map(|e| e.path()).collect();
                // Urut, supaya keluarannya sama tiap jalan — `read_dir` tidak
                // menjanjikan urutan apa pun.
                anak.sort();
                for f in anak {
                    if f.is_dir() && !f.is_symlink() {
                        telusuri(&f, sisa - 1, keluar);
                    } else {
                        keluar.push(Str(Arc::new(f.display().to_string())));
                    }
                }
            }
            // Jalur yang tidak ada atau bukan direktori dulu menjawab `[]`, sama dengan direktori kosong.
            match std::fs::metadata(p.as_str()) {
                Err(e) => return Err(format!("fs::walk '{}': {}", p, crate::host::galat_io(&e))),
                Ok(m) if !m.is_dir() => return Err(format!("fs::walk '{}': {}", p, PESAN_BUKAN_DIREKTORI)),
                Ok(_) => {}
            }
            let mut keluar = Vec::new();
            telusuri(std::path::Path::new(p.as_str()), 64, &mut keluar);
            Ok(VmValue::array(keluar))
        }
        _ => Err("fs::walk(dir: string) -> []string".to_string()),
    });

    // Hanya Unix: bit permission tidak punya padanan di Windows, dan sebuah
    // no-op yang menjawab `true` akan membuat program percaya berkasnya sudah
    // dilindungi padahal tidak.
    vm.def("fs::chmod", |a| match (a.get(0), a.get(1)) {
        (Some(Str(p)), Some(Int(mode))) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let m = u32::try_from(*mode).map_err(|_| format!(
                    "fs::chmod: mode `{}` di luar jangkauan", mode))?;
                std::fs::set_permissions(p.as_str(), std::fs::Permissions::from_mode(m))
                    .map(|_| Unit)
                    .map_err(|e| format!("fs::chmod '{}': {}", p, crate::host::galat_io(&e)))
            }
            #[cfg(not(unix))]
            {
                let _ = (p, mode);
                Err("fs::chmod cuma ada di Unix — Windows tidak punya bit \
                     permission yang setara, dan menjawab `true` di sana berarti \
                     berbohong tentang berkas yang sebenarnya tidak terlindungi"
                    .to_string())
            }
        }
        _ => Err("fs::chmod(path: string, mode: int)".to_string()),
    });

    vm.def("fs::read", |a| match a.get(0) {
        Some(Str(p)) => std::fs::read_to_string(p.as_str())
            .map(|s| Str(Arc::new(s)))
            .map_err(|e| format!("fs::read '{}': {}", p, crate::host::galat_io(&e))),
        _ => Err("fs::read(path: string)".to_string()),
    });

    vm.def("fs::write", |a| match (a.get(0), a.get(1)) {
        (Some(Str(p)), Some(Str(c))) =>
            std::fs::write(p.as_str(), c.as_str())
                .map(|_| Unit)
                .map_err(|e| format!("fs::write '{}': {}", p, crate::host::galat_io(&e))),
        (Some(Str(p)), Some(v)) =>
            std::fs::write(p.as_str(), format!("{}", v))
                .map(|_| Unit)
                .map_err(|e| format!("fs::write '{}': {}", p, crate::host::galat_io(&e))),
        _ => Err("fs::write(path, content)".to_string()),
    });

    vm.def("fs::append", |a| match (a.get(0), a.get(1)) {
        (Some(Str(p)), Some(v)) => {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true).create(true)
                .open(p.as_str())
                .map_err(|e| format!("fs::append '{}': {}", p, crate::host::galat_io(&e)))?;
            write!(f, "{}", v).map_err(|e| format!("fs::append write: {}", crate::host::galat_io(&e)))?;
            Ok(Unit)
        }
        _ => Err("fs::append(path, content)".to_string()),
    });

    vm.def("fs::exists", |a| match a.get(0) {
        Some(Str(p)) => Ok(Bool(std::path::Path::new(p.as_str()).exists())),
        _ => Err("fs::exists(path)".to_string()),
    });

    vm.def("fs::delete", |a| match a.get(0) {
        Some(Str(p)) => {
            let path = std::path::Path::new(p.as_str());
            if path.is_dir() {
                Ok(Bool(std::fs::remove_dir_all(path).is_ok()))
            } else {
                Ok(Bool(std::fs::remove_file(path).is_ok()))
            }
        }
        _ => Err("fs::delete(path)".to_string()),
    });

    // Direktori KOSONG saja: yang berisi, atau yang bukan direktori, dijawab `false`.
    vm.def("fs::rmdir", |a| match a.get(0) {
        Some(Str(p)) => Ok(Bool(std::fs::remove_dir(p.as_str()).is_ok())),
        _ => Err("fs::rmdir(path)".to_string()),
    });

    vm.def("fs::mkdir", |a| match a.get(0) {
        Some(Str(p)) => Ok(Bool(std::fs::create_dir_all(p.as_str()).is_ok())),
        _ => Err("fs::mkdir(path)".to_string()),
    });

    vm.def("fs::ls", |a| match a.get(0) {
        Some(Str(p)) => {
            let rd = std::fs::read_dir(p.as_str())
                .map_err(|e| format!("fs::ls '{}': {}", p, crate::host::galat_io(&e)))?;
            // Terurut, seperti `file::list_dir`: urutan `read_dir` milik sistem berkasnya, bukan janji.
            let mut nama: Vec<String> = rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            nama.sort();
            let items: Vec<VmValue> = nama.into_iter().map(|n| Str(Arc::new(n))).collect();
            Ok(Array(Arc::new(std::sync::Mutex::new(items))))
        }
        _ => Err("fs::ls(path)".to_string()),
    });

    vm.def("fs::is_file", |a| match a.get(0) {
        Some(Str(p)) => Ok(Bool(std::path::Path::new(p.as_str()).is_file())),
        _ => Err("fs::is_file(path)".to_string()),
    });

    vm.def("fs::is_dir", |a| match a.get(0) {
        Some(Str(p)) => Ok(Bool(std::path::Path::new(p.as_str()).is_dir())),
        _ => Err("fs::is_dir(path)".to_string()),
    });

    vm.def("fs::read_lines", |a| match a.get(0) {
        Some(Str(p)) => {
            let content = std::fs::read_to_string(p.as_str())
                .map_err(|e| format!("fs::read_lines '{}': {}", p, crate::host::galat_io(&e)))?;
            let lines: Vec<VmValue> = content.lines()
                .map(|l| Str(Arc::new(l.to_string())))
                .collect();
            Ok(Array(Arc::new(std::sync::Mutex::new(lines))))
        }
        _ => Err("fs::read_lines(path)".to_string()),
    });

    vm.def("fs::write_lines", |a| match (a.get(0), a.get(1)) {
        (Some(Str(p)), Some(Array(lines))) => {
            let content: String = lines.lock().unwrap()
                .iter()
                .map(|v| format!("{}\n", v))
                .collect();
            std::fs::write(p.as_str(), content)
                .map(|_| Unit)
                .map_err(|e| format!("fs::write_lines '{}': {}", p, crate::host::galat_io(&e)))
        }
        _ => Err("fs::write_lines(path, lines: array)".to_string()),
    });

    // Bentuk `try_*`: sebabnya sampai sebagai `Err`. Bentuk `bool` di atasnya tetap, karena
    // `false` menjawab pertanyaannya dengan benar — cuma tanpa sebab.
    vm.def("fs::try_mkdir", |a| coba_fs(a, "fs::try_mkdir", |p, _| std::fs::create_dir_all(p)));
    vm.def("fs::try_rmdir", |a| coba_fs(a, "fs::try_rmdir", |p, _| std::fs::remove_dir(p)));
    vm.def("fs::try_copy", |a| coba_fs(a, "fs::try_copy", |p, q| std::fs::copy(p, q).map(|_| ())));
    vm.def("fs::try_rename", |a| coba_fs(a, "fs::try_rename", |p, q| std::fs::rename(p, q)));
    vm.def("fs::try_delete", |a| coba_fs(a, "fs::try_delete", |p, _| {
        let j = std::path::Path::new(p);
        if j.is_dir() && !j.is_symlink() { std::fs::remove_dir_all(j) } else { std::fs::remove_file(j) }
    }));

    vm.def("fs::copy", |a| match (a.get(0), a.get(1)) {
        (Some(Str(src)), Some(Str(dst))) =>
            Ok(Bool(std::fs::copy(src.as_str(), dst.as_str()).is_ok())),
        _ => Err("fs::copy(src, dst)".to_string()),
    });

    vm.def("fs::rename", |a| match (a.get(0), a.get(1)) {
        (Some(Str(src)), Some(Str(dst))) =>
            Ok(Bool(std::fs::rename(src.as_str(), dst.as_str()).is_ok())),
        _ => Err("fs::rename(src, dst)".to_string()),
    });

    vm.def("fs::size", |a| match a.get(0) {
        Some(Str(p)) => std::fs::metadata(p.as_str())
            .map(|m| Int(m.len() as i128))
            .map_err(|e| format!("fs::size '{}': {}", p, crate::host::galat_io(&e))),
        _ => Err("fs::size(path)".to_string()),
    });

    vm.def("fs::cwd", |_| {
        std::env::current_dir()
            .map(|p| Str(Arc::new(p.display().to_string())))
            .map_err(|e| format!("fs::cwd: {}", crate::host::galat_io(&e)))
    });

    // SEMUA potongan ikut: dulu yang ketiga dan seterusnya dibuang tanpa pesan.
    vm.def("fs::join", |a| {
        if a.len() < 2 { return Err("fs::join(a, b, ...sisa)".to_string()); }
        let mut p = std::path::PathBuf::new();
        for (i, x) in a.iter().enumerate() {
            match x {
                Str(s) => p.push(s.as_str()),
                lain => return Err(format!("fs::join: potongan ke-{} harus string, dapat `{}`", i + 1, lain.type_name())),
            }
        }
        Ok(Str(Arc::new(p.display().to_string())))
    });

    vm.def("fs::basename", |a| match a.get(0) {
        Some(Str(p)) => {
            let name = std::path::Path::new(p.as_str())
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Ok(Str(Arc::new(name)))
        }
        _ => Err("fs::basename(path)".to_string()),
    });

    vm.def("fs::dirname", |a| match a.get(0) {
        Some(Str(p)) => {
            let dir = std::path::Path::new(p.as_str())
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| ".".to_string());
            Ok(Str(Arc::new(dir)))
        }
        _ => Err("fs::dirname(path)".to_string()),
    });

    vm.def("fs::read_base64", |a| crate::stdlib::sistem::lewat_hasil(a, b_read_base64));
    vm.def("fs::try_read", |a| crate::stdlib::sistem::lewat_hasil(a, b_try_read));
    vm.def("fs::write_base64", |a| crate::stdlib::sistem::lewat_hasil(a, |a| b_tulis_base64(a, "fs::write_base64", false)));
    vm.def("fs::append_base64", |a| crate::stdlib::sistem::lewat_hasil(a, |a| b_tulis_base64(a, "fs::append_base64", true)));
}

use crate::interpreter::{err_value, ok_value, Value};

fn teks_b64(a: &[Value], ke: usize, nama: &str) -> Result<String, String> {
    match a.get(ke) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus string, dapat `{}`", nama, ke + 1, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke + 1)),
    }
}

/// `fs::read` yang menjawab `Result`: berkas yang hilang atau bukan teks jadi `Err`, bukan galat
/// yang menghentikan program. Kalimatnya sama dengan `fs::read`.
fn b_try_read(a: &[Value]) -> Result<Value, String> {
    let p = teks_b64(a, 0, "fs::try_read")?;
    Ok(match std::fs::read_to_string(&p) {
        Ok(isi) => ok_value(Value::Str(isi.into())),
        Err(e) => err_value(format!("fs::try_read '{}': {}", p, crate::host::galat_io(&e))),
    })
}

/// Isi berkas APA PUN sebagai base64: jalan untuk berkas biner, yang ditolak `fs::read`.
fn b_read_base64(a: &[Value]) -> Result<Value, String> {
    let p = teks_b64(a, 0, "fs::read_base64")?;
    Ok(match std::fs::read(&p) {
        Ok(isi) => ok_value(Value::Str(crate::dok::crypto::base64_encode_bytes(&isi).into())),
        Err(e) => err_value(format!("fs::read_base64 '{}': {}", p, crate::host::galat_io(&e))),
    })
}

/// Tulis (atau sambung) byte hasil dekode base64. Jawabannya jumlah byte yang ditulis.
fn b_tulis_base64(a: &[Value], nama: &str, sambung: bool) -> Result<Value, String> {
    use std::io::Write;
    let p = teks_b64(a, 0, nama)?;
    let b64 = teks_b64(a, 1, nama)?;
    let isi = match crate::dok::crypto::base64_decode_str(&b64) {
        Ok(v) => v,
        Err(e) => return Ok(err_value(format!("{}: isinya bukan base64 yang sah ({})", nama, e))),
    };
    let mut o = std::fs::OpenOptions::new();
    if sambung { o.append(true).create(true); } else { o.write(true).create(true).truncate(true); }
    Ok(match o.open(&p).and_then(|mut f| f.write_all(&isi)) {
        Ok(()) => ok_value(Value::Int(isi.len() as i128)),
        Err(e) => err_value(format!("{} '{}': {}", nama, p, crate::host::galat_io(&e))),
    })
}

/// Kalimat `fs::walk` atas jalur yang ada tapi bukan direktori; lapis LLVM menulis yang sama.
pub const PESAN_BUKAN_DIREKTORI: &str = "bukan direktori — `fs::walk` menelusuri isi sebuah direktori";
/// Satu atau dua jalur, satu operasi berkas, jawabannya `Result<unit>` dengan sebab dari OS.
fn coba_fs(a: &[VmValue], nama: &'static str, f: fn(&str, &str) -> std::io::Result<()>) -> Result<VmValue, String> {
    crate::stdlib::sistem::lewat_hasil(a, |v: &[Value]| {
        let dua = nama == "fs::try_copy" || nama == "fs::try_rename";
        let p = teks_b64(v, 0, nama)?;
        let q = if dua { teks_b64(v, 1, nama)? } else { String::new() };
        Ok(match f(&p, &q) {
            Ok(()) => ok_value(Value::Unit),
            Err(e) if dua => err_value(format!("{} '{}' -> '{}': {}", nama, p, q, crate::host::galat_io(&e))),
            Err(e) => err_value(format!("{} '{}': {}", nama, p, crate::host::galat_io(&e))),
        })
    })
}
