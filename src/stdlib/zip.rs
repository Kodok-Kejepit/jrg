// src/stdlib/zip.rs — `zip::`: membaca, membongkar dan membuat arsip ZIP, tanpa dependensi.
// Celah yang ditutup sejak lahir: zip-slip, tautan simbolik, bom zip, nama ganda atau ambigu, CRC dan ukuran bohong.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// Batas arsip yang dibaca: jumlah entri, total isi terbongkar, dan isi satu entri di memori.
pub const MAKS_ENTRI: usize = 65_535;
pub const MAKS_TOTAL: u64 = 1 << 30;
pub const MAKS_SATU: u64 = 256 << 20;

const SIG_LOKAL: u32 = 0x0403_4b50;
const SIG_PUSAT: u32 = 0x0201_4b50;
const SIG_AKHIR: u32 = 0x0605_4b50;

pub fn register(vm: &mut Vm) {
    vm.def("zip::list", |a| crate::stdlib::sistem::lewat_hasil(a, z_list));
    vm.def("zip::read", |a| crate::stdlib::sistem::lewat_hasil(a, |a| z_read(a, false)));
    vm.def("zip::read_text", |a| crate::stdlib::sistem::lewat_hasil(a, |a| z_read(a, true)));
    vm.def("zip::extract", |a| crate::stdlib::sistem::lewat_hasil(a, z_extract));
    vm.def("zip::create", |a| crate::stdlib::sistem::lewat_hasil(a, z_create));
}

/// Satu entri dari direktori pusat, sudah diperiksa.
struct Entri {
    nama: String,
    folder: bool,
    metode: u16,
    crc: u32,
    mampat: u64,
    ukuran: u64,
    data: usize,
    exe: bool,
}

fn u16_di(b: &[u8], i: usize) -> Option<u16> { b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]])) }
fn u32_di(b: &[u8], i: usize) -> Option<u32> { b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) }

/// Nama di dalam arsip yang aman jadi jalur relatif: tanpa `..`, jalur absolut, `\`, huruf drive, NUL
/// atau bagian kosong. Ini satu-satunya penjaga zip-slip, dipakai saat membaca DAN membuat arsip.
pub fn periksa_nama(nama: &str) -> Result<(), String> {
    match alasan_jalur(nama) {
        Some(a) if a.contains("`..`") => Err(format!("nama `{}` di arsip ditolak: {} (zip-slip)", nama.escape_debug(), a)),
        Some(a) => Err(format!("nama `{}` di arsip ditolak: {}", nama.escape_debug(), a)),
        None => Ok(()),
    }
}

/// Kenapa `nama` tidak aman jadi jalur relatif di bawah sebuah folder — aturan yang sama untuk arsip
/// dan `fs::safe_join`; `None` kalau aman.
pub fn alasan_jalur(nama: &str) -> Option<&'static str> {
    let inti = nama.strip_suffix('/').unwrap_or(nama);
    if inti.is_empty() { Some("namanya kosong") }
        else if nama.len() > 4096 { Some("namanya lebih dari 4096 byte") }
        else if nama.contains('\0') || nama.chars().any(|c| c.is_control()) { Some("namanya memuat karakter kendali") }
        else if nama.contains('\\') { Some("memuat `\\` — di Windows itu pemisah folder") }
        else if nama.starts_with('/') { Some("jalur absolut") }
        else if nama.as_bytes().get(1) == Some(&b':') { Some("diawali huruf drive") }
        else if inti.split('/').any(|b| b.is_empty()) { Some("memuat bagian jalur kosong (`//`)") }
        else if inti.split('/').any(|b| b == "..") { Some("memuat `..` — bisa keluar dari folder tujuan") }
        else if inti.split('/').any(|b| b == ".") { Some("memuat `.` sebagai bagian jalur") }
        else if cfg!(windows) && inti.split('/').any(nama_windows_terlarang) {
            Some("bagian jalurnya tidak sah di Windows (`:`, nama perangkat, atau titik/spasi di ujung)")
        }
        else { None }
}

/// `CON`, `a:b` (aliran data), `x.` — Windows membaca ketiganya sebagai sesuatu yang lain.
fn nama_windows_terlarang(b: &str) -> bool {
    let dasar = b.split('.').next().unwrap_or("").to_ascii_uppercase();
    b.contains(':') || b.ends_with('.') || b.ends_with(' ')
        || ["CON", "PRN", "AUX", "NUL"].contains(&dasar.as_str())
        || (dasar.len() == 4 && (dasar.starts_with("COM") || dasar.starts_with("LPT"))
            && dasar.as_bytes()[3].is_ascii_digit())
}

/// Nama yang jadi jalur sama (huruf besar-kecil tidak dibedakan, seperti sebagian sistem berkas), atau berkas
/// yang juga dipakai sebagai folder (`a` dan `a/b`), ditolak sebelum apa pun dibongkar atau ditulis.
pub fn periksa_tabrakan<'a>(nama: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut ada: HashMap<String, bool> = HashMap::new();
    let semua: Vec<&str> = nama.collect();
    for n in &semua {
        let folder = n.ends_with('/');
        if ada.insert(n.trim_end_matches('/').to_lowercase(), folder).is_some() {
            return Err(format!("nama `{}` muncul dua kali (huruf besar-kecil tidak dibedakan)", n));
        }
    }
    for n in &semua {
        let inti = n.trim_end_matches('/').to_lowercase();
        let mut induk = inti.as_str();
        while let Some((i, _)) = induk.rsplit_once('/') {
            if ada.get(i) == Some(&false) { return Err(format!("`{}` dipakai sebagai berkas dan juga folder", i)); }
            induk = i;
        }
    }
    Ok(())
}

/// Baca dan periksa seluruh direktori pusat. Tidak ada byte yang dibongkar di sini.
fn baca_daftar(b: &[u8]) -> Result<Vec<Entri>, String> {
    // Akhir direktori pusat: yang TERAKHIR yang komentarnya pas sampai ujung berkas.
    let mulai = b.len().saturating_sub(22 + 65_535);
    let akhir = (mulai..=b.len().saturating_sub(22)).rev()
        .find(|&i| u32_di(b, i) == Some(SIG_AKHIR) && u16_di(b, i + 20).map(|k| i + 22 + k as usize) == Some(b.len()))
        .ok_or("bukan arsip ZIP: penanda akhir direktori pusat tidak ada")?;
    let (disk, disk_pusat) = (u16_di(b, akhir + 4).unwrap(), u16_di(b, akhir + 6).unwrap());
    let (n_disk, n) = (u16_di(b, akhir + 8).unwrap(), u16_di(b, akhir + 10).unwrap() as usize);
    let (ukuran_pusat, awal_pusat) = (u32_di(b, akhir + 12).unwrap() as usize, u32_di(b, akhir + 16).unwrap() as usize);
    if disk != 0 || disk_pusat != 0 || n_disk as usize != n {
        return Err("arsip ZIP berjilid banyak tidak didukung".into());
    }
    if n == 0xFFFF || awal_pusat == 0xFFFF_FFFF || ukuran_pusat == 0xFFFF_FFFF {
        return Err("arsip ZIP64 (di atas 65 535 entri atau 4 GiB) tidak didukung".into());
    }
    if awal_pusat.checked_add(ukuran_pusat) != Some(akhir) {
        return Err("direktori pusat tidak berakhir tepat di penanda akhirnya — arsipnya rusak, \
                    atau ada data yang disisipkan di depannya".into());
    }
    if n > MAKS_ENTRI { return Err(format!("arsip berisi {} entri, batasnya {}", n, MAKS_ENTRI)); }
    let mut hasil = Vec::with_capacity(n);
    let mut total = 0u64;
    let mut p = awal_pusat;
    for _ in 0..n {
        if u32_di(b, p) != Some(SIG_PUSAT) { return Err("entri direktori pusat rusak".into()); }
        let dibuat = u16_di(b, p + 4).ok_or("entri terpotong")?;
        let flag = u16_di(b, p + 8).ok_or("entri terpotong")?;
        let metode = u16_di(b, p + 10).ok_or("entri terpotong")?;
        let crc = u32_di(b, p + 16).ok_or("entri terpotong")?;
        let mampat = u32_di(b, p + 20).ok_or("entri terpotong")? as u64;
        let ukuran = u32_di(b, p + 24).ok_or("entri terpotong")? as u64;
        let (ln, le, lk) = (u16_di(b, p + 28).ok_or("entri terpotong")? as usize,
                            u16_di(b, p + 30).ok_or("entri terpotong")? as usize,
                            u16_di(b, p + 32).ok_or("entri terpotong")? as usize);
        let atribut = u32_di(b, p + 38).ok_or("entri terpotong")?;
        let lokal = u32_di(b, p + 42).ok_or("entri terpotong")? as usize;
        let mentah = b.get(p + 46..p + 46 + ln).ok_or("nama entri melewati akhir arsip")?;
        p += 46 + ln + le + lk;
        if p > akhir { return Err("direktori pusat melewati batasnya".into()); }
        let nama = std::str::from_utf8(mentah).map_err(|_| {
            "nama entri bukan UTF-8 — arsip ini memakai pengodean lama yang tidak bisa dibaca pasti".to_string()
        })?.to_string();
        periksa_nama(&nama)?;
        if flag & 1 != 0 { return Err(format!("`{}` terenkripsi — arsip bersandi tidak didukung", nama)); }
        if metode != 0 && metode != 8 {
            return Err(format!("`{}` dimampatkan dengan metode {} — yang didukung cuma simpan (0) dan deflate (8)", nama, metode));
        }
        if mampat == 0xFFFF_FFFF || ukuran == 0xFFFF_FFFF || lokal == 0xFFFF_FFFF {
            return Err(format!("`{}` memakai ukuran ZIP64 — tidak didukung", nama));
        }
        // Atribut Unix (dibuat di Unix): cuma berkas biasa dan folder; tautan simbolik ditolak.
        let mode = if dibuat >> 8 == 3 { atribut >> 16 } else { 0 };
        let jenis = mode & 0o170_000;
        let folder = nama.ends_with('/');
        if jenis == 0o120_000 { return Err(format!("`{}` adalah tautan simbolik — ditolak, ia bisa menunjuk ke luar folder tujuan", nama)); }
        if jenis != 0 && jenis != 0o100_000 && jenis != 0o040_000 {
            return Err(format!("`{}` bukan berkas biasa atau folder", nama));
        }
        if folder && ukuran != 0 { return Err(format!("folder `{}` punya isi", nama)); }
        // Header lokal harus menyebut nama yang sama: pembaca lain memakai yang itu.
        if u32_di(b, lokal) != Some(SIG_LOKAL) { return Err(format!("header lokal `{}` rusak", nama)); }
        let (lnl, lel) = (u16_di(b, lokal + 26).ok_or("header lokal terpotong")? as usize,
                          u16_di(b, lokal + 28).ok_or("header lokal terpotong")? as usize);
        if b.get(lokal + 30..lokal + 30 + lnl) != Some(mentah) {
            return Err(format!("nama `{}` di direktori pusat berbeda dengan di header lokalnya — dua pembaca \
                                bisa melihat dua arsip berbeda", nama));
        }
        let data = lokal + 30 + lnl + lel;
        if (data as u64).checked_add(mampat).map_or(true, |x| x > awal_pusat as u64) {
            return Err(format!("isi `{}` melewati awal direktori pusat", nama));
        }
        total = total.saturating_add(ukuran);
        if total > MAKS_TOTAL {
            return Err(format!("isi arsip terbongkar lebih dari {} MiB — dianggap bom zip", MAKS_TOTAL >> 20));
        }
        hasil.push(Entri { nama, folder, metode, crc, mampat, ukuran, data, exe: mode & 0o111 != 0 });
    }
    if p != akhir { return Err("direktori pusat berisi lebih dari jumlah entri yang dinyatakan".into()); }
    periksa_tabrakan(hasil.iter().map(|e| e.nama.as_str()))?;
    // Isi dua entri tidak boleh berbagi byte: begitulah bom zip bertumpuk dibuat.
    let mut rentang: Vec<(usize, u64)> = hasil.iter().map(|e| (e.data, e.data as u64 + e.mampat)).collect();
    rentang.sort();
    if rentang.windows(2).any(|w| (w[1].0 as u64) < w[0].1) {
        return Err("isi dua entri saling tumpang-tindih — dianggap bom zip".into());
    }
    Ok(hasil)
}

/// Isi satu entri, dibongkar dan diperiksa ukuran serta CRC-nya.
fn isi_entri(b: &[u8], e: &Entri) -> Result<Vec<u8>, String> {
    let mentah = &b[e.data..e.data + e.mampat as usize];
    let isi = match e.metode {
        0 => mentah.to_vec(),
        _ => crate::kompres::inflate(mentah, e.ukuran as usize)
            .map_err(|g| format!("`{}`: {}", e.nama, g))?,
    };
    if isi.len() as u64 != e.ukuran {
        return Err(format!("`{}`: isinya {} byte, arsipnya menyatakan {}", e.nama, isi.len(), e.ukuran));
    }
    if crate::kompres::crc32(&isi) != e.crc {
        return Err(format!("`{}`: CRC tidak cocok — arsipnya rusak", e.nama));
    }
    Ok(isi)
}

fn teks_arg(a: &[Value], ke: usize, nama: &str) -> Result<String, String> {
    match a.get(ke) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus string, dapat `{}`", nama, ke + 1, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke + 1)),
    }
}

/// Baca berkas arsip lalu daftarnya; galat apa pun jadi `Err` berawalan nama builtin dan jalurnya.
fn buka(a: &[Value], nama: &str) -> Result<Result<(Vec<u8>, Vec<Entri>), Value>, String> {
    let jalur = teks_arg(a, 0, nama)?;
    let b = match std::fs::read(&jalur) {
        Ok(b) => b,
        Err(e) => return Ok(Err(err_value(format!("{} '{}': {}", nama, jalur, crate::host::galat_io(&e))))),
    };
    Ok(match baca_daftar(&b) {
        Ok(d) => Ok((b, d)),
        Err(e) => Err(err_value(format!("{} '{}': {}", nama, jalur, e))),
    })
}

fn dict(isi: Vec<(&str, Value)>) -> Value {
    let m: HashMap<String, Value> = isi.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Struct { name: "dict".to_string(), fields: m.into() }
}

/// `zip::list(arsip)` → `Result<array>`: tiap entri `{nama, ukuran, mampat, folder}`, urut seperti di arsip.
fn z_list(a: &[Value]) -> Result<Value, String> {
    let (_, d) = match buka(a, "zip::list")? { Ok(x) => x, Err(e) => return Ok(e) };
    Ok(ok_value(Value::Array(d.iter().map(|e| dict(vec![
        ("nama", Value::Str(e.nama.clone().into())),
        ("ukuran", Value::Int(e.ukuran as i128)),
        ("mampat", Value::Int(e.mampat as i128)),
        ("folder", Value::Bool(e.folder)),
    ])).collect::<Vec<_>>().into())))
}

/// `zip::read(arsip, nama)` → `Result<array>` (byte); `zip::read_text` → `Result<string>`, bukan UTF-8 jadi `Err`.
fn z_read(a: &[Value], teks: bool) -> Result<Value, String> {
    let fname = if teks { "zip::read_text" } else { "zip::read" };
    let dicari = teks_arg(a, 1, fname)?;
    let (b, d) = match buka(a, fname)? { Ok(x) => x, Err(e) => return Ok(e) };
    let Some(e) = d.iter().find(|e| e.nama == dicari) else {
        return Ok(err_value(format!("{}: tidak ada `{}` di arsip", fname, dicari)));
    };
    if e.folder { return Ok(err_value(format!("{}: `{}` adalah folder", fname, dicari))); }
    if e.ukuran > MAKS_SATU {
        return Ok(err_value(format!("{}: `{}` berukuran {} MiB, di atas {} MiB yang dibaca ke memori — \
                                     pakai `zip::extract`", fname, dicari, e.ukuran >> 20, MAKS_SATU >> 20)));
    }
    Ok(match isi_entri(&b, e) {
        Err(g) => err_value(format!("{}: {}", fname, g)),
        Ok(isi) if teks => match String::from_utf8(isi) {
            Ok(s) => ok_value(Value::teks(s)),
            Err(_) => err_value(format!("{}: `{}` bukan teks UTF-8 — baca byte-nya dengan `zip::read`", fname, dicari)),
        },
        Ok(isi) => ok_value(crate::dok::crypto::byte_ke_array(isi)),
    })
}

/// Folder `jalur` di bawah `akar`, dibuat bagian demi bagian; bagian yang sudah ada harus folder SUNGGUHAN,
/// bukan tautan simbolik — tautan di folder tujuan adalah jalan zip-slip yang kedua.
pub fn siapkan_folder(akar: &Path, bagian: &[&str]) -> Result<PathBuf, String> {
    let mut p = akar.to_path_buf();
    for (i, b) in bagian.iter().enumerate() {
        p.push(b);
        let nama = bagian[..=i].join("/");
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => {}
            Ok(m) if m.file_type().is_symlink() =>
                return Err(format!("`{}` di folder tujuan adalah tautan simbolik — tidak diikuti", nama)),
            Ok(_) => return Err(format!("`{}` sudah ada di folder tujuan dan bukan folder", nama)),
            Err(_) => std::fs::create_dir(&p).map_err(|e| format!("`{}`: {}", nama, crate::host::galat_io(&e)))?,
        }
    }
    Ok(p)
}

/// `zip::extract(arsip, folder)` → `Result<int>` (jumlah berkas). Seluruh daftar diperiksa SEBELUM satu byte
/// ditulis, dan berkas yang sudah ada tidak pernah ditimpa.
fn z_extract(a: &[Value]) -> Result<Value, String> {
    let tujuan = teks_arg(a, 1, "zip::extract")?;
    let (b, d) = match buka(a, "zip::extract")? { Ok(x) => x, Err(e) => return Ok(e) };
    let akar = PathBuf::from(&tujuan);
    if let Err(e) = std::fs::create_dir_all(&akar) {
        return Ok(err_value(format!("zip::extract '{}': {}", tujuan, crate::host::galat_io(&e))));
    }
    let mut ditulis = 0i128;
    for e in &d {
        let bagian: Vec<&str> = e.nama.trim_end_matches('/').split('/').collect();
        let hasil = (|| -> Result<(), String> {
            if e.folder { siapkan_folder(&akar, &bagian)?; return Ok(()); }
            let folder = siapkan_folder(&akar, &bagian[..bagian.len() - 1])?;
            let akhir = folder.join(bagian[bagian.len() - 1]);
            let isi = isi_entri(&b, e)?;
            // `create_new` (O_EXCL): gagal kalau jalurnya sudah ada, tautan simbolik sekalipun — tidak ada yang
            // ditimpa dan tidak ada tautan yang diikuti.
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&akhir)
                .map_err(|g| format!("`{}`: {}", e.nama, crate::host::galat_io(&g)))?;
            let tulis = f.write_all(&isi).map_err(|g| format!("`{}`: {}", e.nama, crate::host::galat_io(&g)));
            drop(f);
            if let Err(g) = tulis { let _ = std::fs::remove_file(&akhir); return Err(g); }
            #[cfg(unix)]
            if e.exe {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&akhir, std::fs::Permissions::from_mode(0o755));
            }
            Ok(())
        })();
        match hasil {
            Ok(()) => if !e.folder { ditulis += 1 },
            Err(g) => return Ok(err_value(format!("zip::extract '{}': {} ({} berkas sudah ditulis sebelumnya)",
                                                  tujuan, g, ditulis))),
        }
    }
    Ok(ok_value(Value::Int(ditulis)))
}

/// `zip::create(arsip, isi)` → `Result<int>`: `isi` adalah dict nama → teks atau array byte, ditulis urut nama.
/// Nama diperiksa dengan aturan yang sama dengan pembacanya, jadi JRG tidak pernah membuat arsip zip-slip.
fn z_create(a: &[Value]) -> Result<Value, String> {
    let jalur = teks_arg(a, 0, "zip::create")?;
    let isi = match a.get(1) {
        Some(Value::Struct { name, fields }) if name == "dict" => fields,
        Some(lain) => return Err(format!("zip::create: argumen ke-2 harus dict nama → isi, dapat `{}`", lain.type_name())),
        None => return Err("zip::create: argumen ke-2 wajib diisi".into()),
    };
    let mut nama: Vec<&String> = isi.keys().collect();
    nama.sort();
    if nama.len() > MAKS_ENTRI { return Ok(err_value(format!("zip::create: {} entri, batasnya {}", nama.len(), MAKS_ENTRI))); }
    if let Err(g) = periksa_tabrakan(nama.iter().map(|n| n.as_str())) { return Ok(err_value(format!("zip::create: {}", g))); }
    let (mut keluar, mut pusat) = (Vec::new(), Vec::new());
    for n in &nama {
        if let Err(g) = periksa_nama(n) { return Ok(err_value(format!("zip::create: {}", g))); }
        let data: Vec<u8> = match &isi[*n] {
            Value::Str(s) => s.as_bytes().to_vec(),
            Value::Array(x) => crate::dok::crypto::array_ke_byte(x, "zip::create")?,
            lain => return Err(format!("zip::create: isi `{}` harus teks atau array byte, dapat `{}`", n, lain.type_name())),
        };
        if n.ends_with('/') && !data.is_empty() { return Ok(err_value(format!("zip::create: folder `{}` tidak boleh berisi", n))); }
        if data.len() as u64 >= 0xFFFF_FFFF || keluar.len() as u64 >= 0xFFFF_FFFF {
            return Ok(err_value("zip::create: lebih dari 4 GiB butuh ZIP64, yang tidak didukung".to_string()));
        }
        let mampat = crate::kompres::deflate(&data);
        let (metode, simpan) = if mampat.len() < data.len() { (8u16, mampat) } else { (0u16, data.clone()) };
        let crc = crate::kompres::crc32(&data);
        let lokal = keluar.len() as u32;
        let tambah = |v: &mut Vec<u8>, sig: u32| {
            v.extend_from_slice(&sig.to_le_bytes());
        };
        // Header lokal: versi 2.0, bit 11 = nama UTF-8, tanggal 1980-01-01 (arsip yang sama dari isi yang sama).
        tambah(&mut keluar, SIG_LOKAL);
        for x in [20u16, 0x0800, metode, 0, 0x21] { keluar.extend_from_slice(&x.to_le_bytes()); }
        for x in [crc, simpan.len() as u32, data.len() as u32] { keluar.extend_from_slice(&x.to_le_bytes()); }
        for x in [n.len() as u16, 0] { keluar.extend_from_slice(&x.to_le_bytes()); }
        keluar.extend_from_slice(n.as_bytes());
        keluar.extend_from_slice(&simpan);
        // Entri pusat: dibuat di Unix, mode 644 (berkas) atau 755 (folder).
        let mode: u32 = if n.ends_with('/') { 0o040_755 } else { 0o100_644 };
        tambah(&mut pusat, SIG_PUSAT);
        for x in [0x031Eu16, 20, 0x0800, metode, 0, 0x21] { pusat.extend_from_slice(&x.to_le_bytes()); }
        for x in [crc, simpan.len() as u32, data.len() as u32] { pusat.extend_from_slice(&x.to_le_bytes()); }
        for x in [n.len() as u16, 0, 0, 0, 0] { pusat.extend_from_slice(&x.to_le_bytes()); }
        for x in [mode << 16, lokal] { pusat.extend_from_slice(&x.to_le_bytes()); }
        pusat.extend_from_slice(n.as_bytes());
    }
    let awal_pusat = keluar.len() as u32;
    keluar.extend_from_slice(&pusat);
    keluar.extend_from_slice(&SIG_AKHIR.to_le_bytes());
    for x in [0u16, 0, nama.len() as u16, nama.len() as u16] { keluar.extend_from_slice(&x.to_le_bytes()); }
    for x in [pusat.len() as u32, awal_pusat] { keluar.extend_from_slice(&x.to_le_bytes()); }
    keluar.extend_from_slice(&0u16.to_le_bytes());
    Ok(match std::fs::write(&jalur, &keluar) {
        Ok(()) => ok_value(Value::Int(nama.len() as i128)),
        Err(e) => err_value(format!("zip::create '{}': {}", jalur, crate::host::galat_io(&e))),
    })
}

#[cfg(test)]
mod tes {
    use super::*;

    /// Satu entri arsip rakitan: nama pusat, nama di header lokal, isi (disimpan), mode Unix, ukuran yang DINYATAKAN.
    struct Rakit<'a> { nama: &'a str, lokal: &'a str, isi: &'a [u8], mode: u32, ukuran: Option<u32> }

    fn entri<'a>(nama: &'a str, isi: &'a [u8]) -> Rakit<'a> {
        Rakit { nama, lokal: nama, isi, mode: 0o100_644, ukuran: None }
    }

    /// Arsip "simpan" (metode 0) dari entri rakitan, tanpa pemeriksaan apa pun — bahan untuk arsip jahat.
    fn rakit(daftar: &[Rakit]) -> Vec<u8> {
        let (mut b, mut pusat) = (Vec::new(), Vec::new());
        for e in daftar {
            let crc = crate::kompres::crc32(e.isi);
            let uk = e.ukuran.unwrap_or(e.isi.len() as u32);
            let lokal = b.len() as u32;
            b.extend_from_slice(&SIG_LOKAL.to_le_bytes());
            for x in [20u16, 0x0800, 0, 0, 0x21] { b.extend_from_slice(&x.to_le_bytes()); }
            for x in [crc, e.isi.len() as u32, uk] { b.extend_from_slice(&x.to_le_bytes()); }
            for x in [e.lokal.len() as u16, 0] { b.extend_from_slice(&x.to_le_bytes()); }
            b.extend_from_slice(e.lokal.as_bytes());
            b.extend_from_slice(e.isi);
            pusat.extend_from_slice(&SIG_PUSAT.to_le_bytes());
            for x in [0x031Eu16, 20, 0x0800, 0, 0, 0x21] { pusat.extend_from_slice(&x.to_le_bytes()); }
            for x in [crc, e.isi.len() as u32, uk] { pusat.extend_from_slice(&x.to_le_bytes()); }
            for x in [e.nama.len() as u16, 0, 0, 0, 0] { pusat.extend_from_slice(&x.to_le_bytes()); }
            for x in [e.mode << 16, lokal] { pusat.extend_from_slice(&x.to_le_bytes()); }
            pusat.extend_from_slice(e.nama.as_bytes());
        }
        let awal = b.len() as u32;
        b.extend_from_slice(&pusat);
        b.extend_from_slice(&SIG_AKHIR.to_le_bytes());
        for x in [0u16, 0, daftar.len() as u16, daftar.len() as u16] { b.extend_from_slice(&x.to_le_bytes()); }
        for x in [pusat.len() as u32, awal] { b.extend_from_slice(&x.to_le_bytes()); }
        b.extend_from_slice(&0u16.to_le_bytes());
        b
    }

    fn tolak(b: &[u8], kata: &str) {
        match baca_daftar(b) {
            Ok(_) => panic!("arsip diterima, harusnya ditolak dengan `{}`", kata),
            Err(e) => assert!(e.contains(kata), "ditolak dengan kalimat lain: {}", e),
        }
    }

    #[test]
    fn nama_berbahaya_ditolak() {
        for n in ["../x", "a/../../x", "/etc/passwd", "C:/x", "a\\..\\x", "a//b", "./a", "", "a/\0b", "a\nb"] {
            assert!(periksa_nama(n).is_err(), "{:?} lolos", n);
        }
        for n in ["a.txt", "folder/", "folder/isi.txt", "nama..titik", "..a", "é/文"] {
            assert!(periksa_nama(n).is_ok(), "{:?} ditolak", n);
        }
    }

    #[test]
    fn arsip_sah_terbaca_utuh() {
        let b = rakit(&[entri("a.txt", b"halo"), entri("d/", b""), entri("d/b.bin", &[0, 255, 7])]);
        let d = baca_daftar(&b).expect("arsip sah ditolak");
        assert_eq!(d.iter().map(|e| e.nama.as_str()).collect::<Vec<_>>(), ["a.txt", "d/", "d/b.bin"]);
        assert_eq!(isi_entri(&b, &d[2]).unwrap(), vec![0, 255, 7]);
    }

    #[test]
    fn arsip_jahat_ditolak_sebelum_dibongkar() {
        tolak(&rakit(&[entri("../../.bashrc", b"x")]), "zip-slip");
        tolak(&rakit(&[Rakit { mode: 0o120_777, ..entri("tautan", b"/etc") }]), "tautan simbolik");
        tolak(&rakit(&[entri("A.txt", b"1"), entri("a.txt", b"2")]), "muncul dua kali");
        tolak(&rakit(&[entri("a", b"1"), entri("a/b", b"2")]), "berkas dan juga folder");
        tolak(&rakit(&[Rakit { lokal: "lain.txt", ..entri("aman.txt", b"x") }]), "berbeda dengan di header lokalnya");
        tolak(&rakit(&[Rakit { ukuran: Some(0xFFFF_FFFE), ..entri("bom", b"x") }]), "bom zip");
        // Bom bertumpuk: isi `a` memuat seluruh entri `b`, jadi rentang isi keduanya bertindihan.
        let dalam = rakit(&[entri("b", b"isi-b")]);
        let ujung = dalam.len() - 22 - (46 + 1);
        let mut luar = rakit(&[entri("a", &dalam[..ujung]), entri("b", b"isi-b")]);
        let geser = 30 + 1;
        let i = luar.len() - 22 - (46 + 1) + 42;
        luar[i..i + 4].copy_from_slice(&(geser as u32).to_le_bytes());
        tolak(&luar, "tumpang-tindih");
    }

    #[test]
    fn isi_yang_bohong_ketahuan() {
        let mut b = rakit(&[entri("a.txt", b"halo")]);
        b[30 + 5] ^= 1;
        let d = baca_daftar(&b).unwrap();
        assert!(isi_entri(&b, &d[0]).unwrap_err().contains("CRC"));
        let b = rakit(&[Rakit { ukuran: Some(10), ..entri("a.txt", b"halo") }]);
        let d = baca_daftar(&b).unwrap();
        assert!(isi_entri(&b, &d[0]).unwrap_err().contains("menyatakan 10"));
    }
}
