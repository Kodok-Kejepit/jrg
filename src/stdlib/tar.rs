// src/stdlib/tar.rs — `tar::`: arsip tar (ustar, POSIX pax, nama panjang GNU), juga `.tar.gz`, tanpa dependensi.
// Kelas celah pustaka tar bahasa lain ditutup sejak lahir: tar-slip, tautan, dua ukuran untuk satu entri, sparse.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use crate::interpreter::{err_value, ok_value, Value};
use crate::stdlib::zip::{periksa_nama, periksa_tabrakan, siapkan_folder, MAKS_ENTRI, MAKS_SATU, MAKS_TOTAL};
use crate::vm::vm::Vm;

/// Kepala pax dan nama panjang GNU paling besar — isinya cuma nama dan beberapa angka.
const MAKS_META: u64 = 1 << 20;
const BLOK: usize = 512;

pub fn register(vm: &mut Vm) {
    vm.def("tar::list", |a| crate::stdlib::sistem::lewat_hasil(a, t_list));
    vm.def("tar::read", |a| crate::stdlib::sistem::lewat_hasil(a, |a| t_read(a, false)));
    vm.def("tar::read_text", |a| crate::stdlib::sistem::lewat_hasil(a, |a| t_read(a, true)));
    vm.def("tar::extract", |a| crate::stdlib::sistem::lewat_hasil(a, t_extract));
    vm.def("tar::create", |a| crate::stdlib::sistem::lewat_hasil(a, t_create));
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Jenis { Berkas, Folder, Symlink, Hardlink, Perangkat, Fifo }

impl Jenis {
    fn kata(self) -> &'static str {
        match self {
            Jenis::Berkas => "berkas", Jenis::Folder => "folder", Jenis::Symlink => "symlink",
            Jenis::Hardlink => "hardlink", Jenis::Perangkat => "perangkat", Jenis::Fifo => "fifo",
        }
    }
}

/// Satu entri yang sudah diperiksa; `data` adalah posisi isinya di arsip (tanpa gzip).
#[derive(Debug)]
pub struct Entri {
    pub nama: String,
    pub jenis: Jenis,
    pub tautan: String,
    pub ukuran: u64,
    pub data: usize,
    pub exe: bool,
}

/// Angka kepala: oktal berakhiran spasi/NUL, atau base-256 GNU (bit tinggi); selain itu ditolak, bukan dibaca 0.
fn angka(f: &[u8], apa: &str) -> Result<u64, String> {
    let salah = || format!("{} di kepala tar bukan angka yang sah", apa);
    if f[0] & 0x80 != 0 {
        if f[0] & 0x40 != 0 { return Err(format!("{} di kepala tar negatif", apa)); }
        let mut v = (f[0] & 0x3f) as u64;
        for &b in &f[1..] { v = v.checked_mul(256).and_then(|v| v.checked_add(b as u64)).ok_or_else(salah)?; }
        return Ok(v);
    }
    let mut i = 0;
    while i < f.len() && f[i] == b' ' { i += 1; }
    let mut v: u64 = 0;
    while i < f.len() && (b'0'..=b'7').contains(&f[i]) {
        v = v.checked_mul(8).and_then(|v| v.checked_add((f[i] - b'0') as u64)).ok_or_else(salah)?;
        i += 1;
    }
    if f[i..].iter().any(|&b| b != b' ' && b != 0) { return Err(salah()); }
    Ok(v)
}

fn teks_c(f: &[u8], apa: &str) -> Result<String, String> {
    let n = f.iter().position(|&b| b == 0).unwrap_or(f.len());
    String::from_utf8(f[..n].to_vec()).map_err(|_| format!("{} di kepala tar bukan UTF-8", apa))
}

/// Rekaman pax `"<panjang> <kunci>=<nilai>\n"`; panjangnya harus tepat, kunci kembar ditolak.
fn urai_pax(d: &[u8]) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    let mut p = 0;
    while p < d.len() {
        let sp = d[p..].iter().position(|&b| b == b' ').ok_or("rekaman pax tanpa panjang")?;
        let pj: usize = std::str::from_utf8(&d[p..p + sp]).ok().filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            .and_then(|s| s.parse().ok()).ok_or("panjang rekaman pax bukan angka")?;
        if pj <= sp + 1 || p + pj > d.len() || d[p + pj - 1] != b'\n' {
            return Err("panjang rekaman pax tidak cocok dengan isinya".into());
        }
        let isi = std::str::from_utf8(&d[p + sp + 1..p + pj - 1]).map_err(|_| "rekaman pax bukan UTF-8")?;
        let (k, v) = isi.split_once('=').ok_or("rekaman pax tanpa `=`")?;
        if k.is_empty() { return Err("rekaman pax berkunci kosong".into()); }
        if m.insert(k.to_string(), v.to_string()).is_some() {
            return Err(format!("kunci pax `{}` muncul dua kali — pembaca lain bisa memilih yang lain", k));
        }
        p += pj;
    }
    Ok(m)
}

/// `.tar.gz` dibongkar dulu (batas bom yang sama); format mampat lain disebut, bukan dibaca sebagai tar rusak.
pub fn buka_mampat(b: Vec<u8>) -> Result<Vec<u8>, String> {
    if b.starts_with(&[0x1f, 0x8b]) {
        return crate::stdlib::gzip::bongkar(&b, MAKS_TOTAL as usize + (64 << 20)).map_err(|e| {
            if e.contains("melewati batas") { format!("isi gzip-nya lebih dari {} MiB — dianggap bom", MAKS_TOTAL >> 20) }
            else { format!("lapisan gzip: {}", e) }
        });
    }
    for (sidik, apa) in [(&b"BZh"[..], "bzip2"), (&[0xfd, b'7', b'z', b'X', b'Z', 0][..], "xz"), (&[0x28, 0xb5, 0x2f, 0xfd][..], "zstd")] {
        if b.starts_with(sidik) { return Err(format!("dimampatkan dengan {} — yang didukung cuma tar polos dan gzip", apa)); }
    }
    Ok(b)
}

/// Baca dan periksa seluruh arsip. Tidak ada yang ditulis di sini, dan tidak ada entri yang dibuang diam-diam.
pub fn baca_daftar(b: &[u8]) -> Result<Vec<Entri>, String> {
    let mut hasil: Vec<Entri> = Vec::new();
    let (mut p, mut total) = (0usize, 0u64);
    let mut pax: Option<HashMap<String, String>> = None;
    let (mut nama_gnu, mut tautan_gnu): (Option<String>, Option<String>) = (None, None);
    loop {
        if p == b.len() { return Err("arsip berakhir tanpa penanda akhir (dua blok nol) — terpotong?".into()); }
        if p + BLOK > b.len() { return Err("kepala tar terpotong".into()); }
        let h = &b[p..p + BLOK];
        if h.iter().all(|&x| x == 0) {
            if pax.is_some() || nama_gnu.is_some() || tautan_gnu.is_some() {
                return Err("kepala pax atau nama panjang tanpa entri sesudahnya".into());
            }
            // Sesudah penanda akhir cuma boleh ada nol (pengisi rekaman); data lain ditolak, bukan dibuang.
            if b[p..].iter().any(|&x| x != 0) {
                return Err(format!("ada data sesudah penanda akhir arsip (byte ke-{}) — ditolak, bukan dibuang diam-diam",
                                   p + b[p..].iter().position(|&x| x != 0).unwrap()));
            }
            break;
        }
        let ke = hasil.len() + 1;
        let cek = angka(&h[148..156], "checksum")?;
        let tak_bertanda: u64 = h.iter().enumerate().map(|(i, &x)| if (148..156).contains(&i) { 32 } else { x as u64 }).sum();
        let bertanda: i64 = h.iter().enumerate().map(|(i, &x)| if (148..156).contains(&i) { 32 } else { x as i8 as i64 }).sum();
        if cek != tak_bertanda && cek as i64 != bertanda {
            return Err(if p == 0 { "bukan arsip tar (checksum kepala pertama tidak cocok)".into() }
                       else { format!("checksum kepala entri ke-{} tidak cocok — arsipnya rusak", ke) });
        }
        let posix = &h[257..263] == b"ustar\0";
        let gnu = &h[257..265] == b"ustar  \0";
        if !posix && !gnu && h[257..265].iter().any(|&x| x != 0) {
            return Err(format!("penanda format kepala entri ke-{} tidak dikenal", ke));
        }
        let jenis_b = h[156];
        let mut ukuran = angka(&h[124..136], "ukuran")?;
        let awal = p + BLOK;
        let isi_akhir = |u: u64| -> Result<usize, String> {
            let u = usize::try_from(u).map_err(|_| "ukuran entri terlalu besar".to_string())?;
            awal.checked_add(u).filter(|&a| a <= b.len()).ok_or_else(|| format!("isi entri ke-{} terpotong", ke))
        };
        if matches!(jenis_b, b'x' | b'g' | b'L' | b'K') {
            if ukuran > MAKS_META { return Err(format!("kepala tambahan berukuran {} byte — dianggap serangan", ukuran)); }
            let akhir = isi_akhir(ukuran)?;
            let d = &b[awal..akhir];
            match jenis_b {
                b'x' => {
                    if pax.is_some() { return Err("dua kepala pax berturut-turut untuk satu entri".into()); }
                    pax = Some(urai_pax(d)?);
                }
                b'g' => {
                    let g = urai_pax(d)?;
                    if let Some(k) = g.keys().find(|k| matches!(k.as_str(), "path" | "linkpath" | "size") || k.starts_with("GNU.sparse")) {
                        return Err(format!("kepala pax global memuat `{}` — berlaku untuk semua entri, ditolak", k));
                    }
                }
                _ => {
                    let slot = if jenis_b == b'L' { &mut nama_gnu } else { &mut tautan_gnu };
                    if slot.is_some() { return Err("dua nama panjang GNU berturut-turut untuk satu entri".into()); }
                    *slot = Some(teks_c(d, "nama panjang GNU")?);
                }
            }
            p = awal + (akhir - awal).div_ceil(BLOK) * BLOK;
            continue;
        }
        let jenis = match jenis_b {
            b'0' | 0 | b'7' => Jenis::Berkas,
            b'5' => Jenis::Folder,
            b'2' => Jenis::Symlink,
            b'1' => Jenis::Hardlink,
            b'3' | b'4' => Jenis::Perangkat,
            b'6' => Jenis::Fifo,
            b'S' => return Err(format!("entri ke-{} berkas sparse GNU — tidak didukung, isinya akan terbaca salah", ke)),
            lain => return Err(format!("jenis entri `{}` di entri ke-{} tidak didukung", (lain as char).escape_default(), ke)),
        };
        let px = pax.take().unwrap_or_default();
        if let Some(k) = px.keys().find(|k| k.starts_with("GNU.sparse")) {
            return Err(format!("entri ke-{} memuat `{}` (sparse) — tidak didukung, isinya akan terbaca salah", ke, k));
        }
        // Ukuran pax menang, tapi dua ukuran BERBEDA untuk satu entri adalah dua arsip (kelas CVE-2025-62518).
        if let Some(s) = px.get("size") {
            let n: u64 = s.parse().ok().filter(|_| s.bytes().all(|c| c.is_ascii_digit())).ok_or("ukuran pax bukan angka")?;
            if ukuran != 0 && ukuran != n {
                return Err(format!("entri ke-{} punya dua ukuran ({} di kepala ustar, {} di pax) — pembaca lain bisa \
                                    membaca arsip yang berbeda", ke, ukuran, n));
            }
            ukuran = n;
        }
        let mut nama = match (px.get("path"), nama_gnu.take()) {
            (Some(_), Some(_)) => return Err(format!("entri ke-{} punya nama pax dan nama panjang GNU sekaligus", ke)),
            (Some(n), None) => n.clone(),
            (None, Some(n)) => n,
            (None, None) => {
                let n = teks_c(&h[0..100], "nama")?;
                let awalan = if posix { teks_c(&h[345..500], "awalan nama")? } else { String::new() };
                if awalan.is_empty() { n } else { format!("{}/{}", awalan, n) }
            }
        };
        let tautan = match (px.get("linkpath"), tautan_gnu.take()) {
            (Some(_), Some(_)) => return Err(format!("entri ke-{} punya tujuan tautan pax dan GNU sekaligus", ke)),
            (Some(t), None) => t.clone(),
            (None, Some(t)) => t,
            (None, None) => teks_c(&h[157..257], "tujuan tautan")?,
        };
        let mode = angka(&h[100..108], "mode")?;
        let akhir = isi_akhir(ukuran)?;
        p = awal + (akhir - awal).div_ceil(BLOK) * BLOK;
        // `./a` (keluaran `tar -C d .`) berarti `a`; `./` sendiri adalah folder tujuan itu sendiri.
        while let Some(s) = nama.strip_prefix("./") { nama = s.to_string(); }
        let jenis = if jenis == Jenis::Berkas && nama.ends_with('/') { Jenis::Folder } else { jenis };
        if jenis == Jenis::Folder {
            if nama.is_empty() || nama == "." { continue; }
            if !nama.ends_with('/') { nama.push('/'); }
        } else if nama.ends_with('/') {
            return Err(format!("`{}` berakhiran `/` tapi bukan folder", nama));
        }
        periksa_nama(&nama)?;
        if jenis != Jenis::Berkas && ukuran != 0 {
            return Err(format!("`{}` ({}) punya isi {} byte", nama, jenis.kata(), ukuran));
        }
        if matches!(jenis, Jenis::Symlink | Jenis::Hardlink) && tautan.is_empty() {
            return Err(format!("`{}` adalah tautan tanpa tujuan", nama));
        }
        total = total.saturating_add(ukuran);
        if total > MAKS_TOTAL { return Err(format!("isi arsip lebih dari {} MiB — dianggap bom", MAKS_TOTAL >> 20)); }
        if hasil.len() >= MAKS_ENTRI { return Err(format!("arsip berisi lebih dari {} entri", MAKS_ENTRI)); }
        hasil.push(Entri { nama, jenis, tautan, ukuran, data: awal, exe: mode & 0o111 != 0 });
    }
    periksa_tabrakan(hasil.iter().map(|e| e.nama.as_str()))?;
    Ok(hasil)
}

fn teks_arg(a: &[Value], ke: usize, nama: &str) -> Result<String, String> {
    match a.get(ke) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus string, dapat `{}`", nama, ke + 1, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke + 1)),
    }
}

/// Baca berkas arsip (gzip dibongkar) lalu daftarnya; galat apa pun jadi `Err` berawalan nama builtin dan jalurnya.
fn buka(a: &[Value], nama: &str) -> Result<Result<(Vec<u8>, Vec<Entri>), Value>, String> {
    let jalur = teks_arg(a, 0, nama)?;
    let b = match std::fs::read(&jalur) {
        Ok(b) => b,
        Err(e) => return Ok(Err(err_value(format!("{} '{}': {}", nama, jalur, crate::host::galat_io(&e))))),
    };
    Ok(match buka_mampat(b).and_then(|b| baca_daftar(&b).map(|d| (b, d))) {
        Ok(x) => Ok(x),
        Err(e) => Err(err_value(format!("{} '{}': {}", nama, jalur, e))),
    })
}

fn dict(isi: Vec<(&str, Value)>) -> Value {
    let m: HashMap<String, Value> = isi.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Struct { name: "dict".to_string(), fields: m.into() }
}

/// `tar::list(arsip)` → `Result<array>`: `{nama, ukuran, folder, jenis}` urut seperti di arsip, plus `tautan` untuk tautan.
fn t_list(a: &[Value]) -> Result<Value, String> {
    let (_, d) = match buka(a, "tar::list")? { Ok(x) => x, Err(e) => return Ok(e) };
    Ok(ok_value(Value::Array(d.iter().map(|e| {
        let mut f = vec![
            ("nama", Value::Str(e.nama.clone().into())),
            ("ukuran", Value::Int(e.ukuran as i128)),
            ("folder", Value::Bool(e.jenis == Jenis::Folder)),
            ("jenis", Value::teks(e.jenis.kata())),
        ];
        if matches!(e.jenis, Jenis::Symlink | Jenis::Hardlink) { f.push(("tautan", Value::teks(e.tautan.clone()))); }
        dict(f)
    }).collect::<Vec<_>>().into())))
}

/// `tar::read(arsip, nama)` → `Result<array>` (byte); `tar::read_text` → `Result<string>`, bukan UTF-8 jadi `Err`.
fn t_read(a: &[Value], teks: bool) -> Result<Value, String> {
    let fname = if teks { "tar::read_text" } else { "tar::read" };
    let dicari = teks_arg(a, 1, fname)?;
    let (b, d) = match buka(a, fname)? { Ok(x) => x, Err(e) => return Ok(e) };
    let Some(e) = d.iter().find(|e| e.nama == dicari || e.nama.strip_suffix('/') == Some(dicari.as_str())) else {
        return Ok(err_value(format!("{}: tidak ada `{}` di arsip", fname, dicari)));
    };
    if e.jenis != Jenis::Berkas {
        let tujuan = if e.tautan.is_empty() { String::new() } else { format!(" ke `{}`", e.tautan) };
        return Ok(err_value(format!("{}: `{}` adalah {}{}, bukan berkas", fname, dicari, e.jenis.kata(), tujuan)));
    }
    if e.ukuran > MAKS_SATU {
        return Ok(err_value(format!("{}: `{}` berukuran {} MiB, di atas {} MiB yang dibaca ke memori — \
                                     pakai `tar::extract`", fname, dicari, e.ukuran >> 20, MAKS_SATU >> 20)));
    }
    let isi = b[e.data..e.data + e.ukuran as usize].to_vec();
    Ok(if teks {
        match String::from_utf8(isi) {
            Ok(s) => ok_value(Value::teks(s)),
            Err(_) => err_value(format!("{}: `{}` bukan teks UTF-8 — baca byte-nya dengan `tar::read`", fname, dicari)),
        }
    } else { ok_value(crate::dok::crypto::byte_ke_array(isi)) })
}

/// `tar::extract(arsip, folder)` → `Result<int>` (jumlah berkas). Tautan, perangkat dan fifo ditolak, dan seluruh
/// daftar diperiksa SEBELUM satu byte ditulis; berkas yang sudah ada tidak pernah ditimpa, mode setuid tidak dibawa.
fn t_extract(a: &[Value]) -> Result<Value, String> {
    let tujuan = teks_arg(a, 1, "tar::extract")?;
    let (b, d) = match buka(a, "tar::extract")? { Ok(x) => x, Err(e) => return Ok(e) };
    if let Some(e) = d.iter().find(|e| !matches!(e.jenis, Jenis::Berkas | Jenis::Folder)) {
        let tujuan_t = if e.tautan.is_empty() { String::new() } else { format!(" ke `{}`", e.tautan) };
        return Ok(err_value(format!("tar::extract '{}': `{}` adalah {}{} — `tar::extract` tidak membuatnya (tautan bisa \
                                     menunjuk ke luar folder tujuan); tidak ada yang ditulis, lihat isinya dengan `tar::list`",
                                    tujuan, e.nama, e.jenis.kata(), tujuan_t)));
    }
    let akar = PathBuf::from(&tujuan);
    if let Err(e) = std::fs::create_dir_all(&akar) {
        return Ok(err_value(format!("tar::extract '{}': {}", tujuan, crate::host::galat_io(&e))));
    }
    let mut ditulis = 0i128;
    for e in &d {
        let bagian: Vec<&str> = e.nama.trim_end_matches('/').split('/').collect();
        let hasil = (|| -> Result<(), String> {
            if e.jenis == Jenis::Folder { siapkan_folder(&akar, &bagian)?; return Ok(()); }
            let folder = siapkan_folder(&akar, &bagian[..bagian.len() - 1])?;
            let akhir = folder.join(bagian[bagian.len() - 1]);
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&akhir)
                .map_err(|g| format!("`{}`: {}", e.nama, crate::host::galat_io(&g)))?;
            let tulis = f.write_all(&b[e.data..e.data + e.ukuran as usize])
                .map_err(|g| format!("`{}`: {}", e.nama, crate::host::galat_io(&g)));
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
            Ok(()) => if e.jenis == Jenis::Berkas { ditulis += 1 },
            Err(g) => return Ok(err_value(format!("tar::extract '{}': {} ({} berkas sudah ditulis sebelumnya)",
                                                  tujuan, g, ditulis))),
        }
    }
    Ok(ok_value(Value::Int(ditulis)))
}

fn kepala(nama: &[u8], ukuran: u64, jenis: u8) -> [u8; BLOK] {
    let mut h = [0u8; BLOK];
    h[..nama.len()].copy_from_slice(nama);
    let mode: &[u8] = if jenis == b'5' { b"0000755\0" } else { b"0000644\0" };
    h[100..108].copy_from_slice(mode);
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    h[124..136].copy_from_slice(format!("{:011o}\0", ukuran).as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[156] = jenis;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].copy_from_slice(b"        ");
    let cek: u32 = h.iter().map(|&x| x as u32).sum();
    h[148..156].copy_from_slice(format!("{:06o}\0 ", cek).as_bytes());
    h
}

/// Arsip tar dari pasangan nama → isi yang sudah urut; nama di atas 100 byte lewat kepala pax `path`.
pub fn rakit(isi: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut o = Vec::new();
    let pad = |o: &mut Vec<u8>| { let n = o.len().div_ceil(BLOK) * BLOK; o.resize(n, 0); };
    for (n, data) in isi {
        let jenis = if n.ends_with('/') { b'5' } else { b'0' };
        let nb = n.as_bytes();
        if nb.len() > 100 {
            let rek = |pj: usize| format!("{} path={}\n", pj, n);
            let mut pj = nb.len() + 7;
            while rek(pj).len() != pj { pj = rek(pj).len(); }
            o.extend_from_slice(&kepala(b"././@PaxHeader", pj as u64, b'x'));
            o.extend_from_slice(rek(pj).as_bytes());
            pad(&mut o);
        }
        // Nama yang dipotong cuma untuk pembaca tanpa pax; potongannya di batas karakter.
        let mut potong = nb.len().min(100);
        while !n.is_char_boundary(potong) { potong -= 1; }
        o.extend_from_slice(&kepala(&nb[..potong], data.len() as u64, jenis));
        o.extend_from_slice(data);
        pad(&mut o);
    }
    o.resize(o.len() + 2 * BLOK, 0);
    o.resize(o.len().div_ceil(10240) * 10240, 0);
    o
}

/// `tar::create(arsip, isi)` → `Result<int>`: `isi` dict nama → teks atau array byte (folder berakhiran `/`, isi kosong),
/// ditulis urut nama; jalur berakhiran `.gz`/`.tgz` dimampatkan gzip. Nama diperiksa dengan aturan pembacanya.
fn t_create(a: &[Value]) -> Result<Value, String> {
    let jalur = teks_arg(a, 0, "tar::create")?;
    let isi = match a.get(1) {
        Some(Value::Struct { name, fields }) if name == "dict" => fields,
        Some(lain) => return Err(format!("tar::create: argumen ke-2 harus dict nama → isi, dapat `{}`", lain.type_name())),
        None => return Err("tar::create: argumen ke-2 wajib diisi".into()),
    };
    let mut nama: Vec<&String> = isi.keys().collect();
    nama.sort();
    if nama.len() > MAKS_ENTRI { return Ok(err_value(format!("tar::create: {} entri, batasnya {}", nama.len(), MAKS_ENTRI))); }
    if let Err(g) = periksa_tabrakan(nama.iter().map(|n| n.as_str())) { return Ok(err_value(format!("tar::create: {}", g))); }
    let mut pasangan = Vec::with_capacity(nama.len());
    for n in &nama {
        if let Err(g) = periksa_nama(n) { return Ok(err_value(format!("tar::create: {}", g))); }
        let data: Vec<u8> = match &isi[*n] {
            Value::Str(s) => s.as_bytes().to_vec(),
            Value::Array(x) => crate::dok::crypto::array_ke_byte(x, "tar::create")?,
            lain => return Err(format!("tar::create: isi `{}` harus teks atau array byte, dapat `{}`", n, lain.type_name())),
        };
        if n.ends_with('/') && !data.is_empty() { return Ok(err_value(format!("tar::create: folder `{}` tidak boleh berisi", n))); }
        pasangan.push(((*n).clone(), data));
    }
    let mut keluar = rakit(&pasangan);
    if jalur.ends_with(".gz") || jalur.ends_with(".tgz") { keluar = crate::stdlib::gzip::mampatkan(&keluar); }
    Ok(match std::fs::write(&jalur, &keluar) {
        Ok(()) => ok_value(Value::Int(nama.len() as i128)),
        Err(e) => err_value(format!("tar::create '{}': {}", jalur, crate::host::galat_io(&e))),
    })
}

#[cfg(test)]
mod tes {
    use super::*;

    fn satu(nama: &[u8], isi: &[u8], jenis: u8) -> Vec<u8> {
        let mut o = kepala(nama, isi.len() as u64, jenis).to_vec();
        o.extend_from_slice(isi);
        o.resize(o.len().div_ceil(BLOK) * BLOK, 0);
        o
    }
    fn tutup(mut o: Vec<u8>) -> Vec<u8> { o.resize(o.len() + 2 * BLOK, 0); o }
    fn galat(b: &[u8]) -> String { baca_daftar(b).unwrap_err() }

    #[test]
    fn bolak_balik_nama_panjang_dan_gzip() {
        let panjang = format!("{}/akhir.txt", "folder-bernama-panjang".repeat(8));
        let isi = vec![("a/".to_string(), vec![]), ("a/b.txt".to_string(), b"halo".to_vec()),
                       (panjang.clone(), vec![7; 1000])];
        let b = rakit(&isi);
        assert_eq!(b.len() % 10240, 0);
        let d = baca_daftar(&b).unwrap();
        assert_eq!(d.iter().map(|e| e.nama.as_str()).collect::<Vec<_>>(), ["a/", "a/b.txt", panjang.as_str()]);
        assert_eq!(&b[d[1].data..d[1].data + 4], b"halo");
        assert_eq!(d[2].ukuran, 1000);
        let gz = buka_mampat(crate::stdlib::gzip::mampatkan(&b)).unwrap();
        assert_eq!(gz, b);
        // `./x` dari `tar -C d .`, dan folder `./` itu sendiri dilewati.
        let mut o = satu(b"./", b"", b'5');
        o.extend(satu(b"./x", b"1", b'0'));
        assert_eq!(baca_daftar(&tutup(o)).unwrap()[0].nama, "x");
        assert!(buka_mampat(b"BZh91AY".to_vec()).unwrap_err().contains("bzip2"));
    }

    #[test]
    fn arsip_jahat_ditolak() {
        assert!(galat(&tutup(satu(b"../luar", b"x", b'0'))).contains("tar-slip") || galat(&tutup(satu(b"../luar", b"x", b'0'))).contains(".."));
        assert!(galat(&tutup(satu(b"/etc/passwd", b"x", b'0'))).contains("absolut"));
        // Nama lewat pax juga diperiksa.
        let mut o = satu(b"p", b"19 path=../../evil\n", b'x');
        o.extend(satu(b"aman", b"x", b'0'));
        assert!(galat(&tutup(o)).contains(".."));
        // Dua ukuran untuk satu entri (kelas CVE-2025-62518).
        let mut o = satu(b"p", b"12 size=100\n", b'x');
        o.extend(satu(b"a", b"x", b'0'));
        assert!(galat(&tutup(o)).contains("dua ukuran"));
        let mut o = satu(b"p", b"6 a=1\n6 a=2\n", b'x');
        o.extend(satu(b"a", b"x", b'0'));
        assert!(galat(&tutup(o)).contains("dua kali"));
        let mut o = satu(b"p", b"99 path=x\n", b'x');
        o.extend(satu(b"a", b"x", b'0'));
        assert!(galat(&tutup(o)).contains("tidak cocok"));
        let mut o = satu(b"p", b"20 GNU.sparse.map=1\n", b'x');
        o.extend(satu(b"a", b"x", b'0'));
        assert!(galat(&tutup(o)).contains("sparse"));
        // Checksum, angka yang bukan oktal, terpotong, ekor liar, nama ganda, entri tanpa penanda akhir.
        let mut o = satu(b"a", b"x", b'0'); o[0] = b'b';
        assert!(galat(&tutup(o)).contains("checksum"));
        let mut h = kepala(b"a", 1, b'0'); h[124] = b'9';
        h[148..156].copy_from_slice(b"        ");
        let c: u32 = h.iter().map(|&x| x as u32).sum();
        h[148..156].copy_from_slice(format!("{:06o}\0 ", c).as_bytes());
        assert!(galat(&tutup(h.to_vec())).contains("bukan angka"));
        assert!(galat(&satu(b"a", &[1; 600], b'0')[..800]).contains("terpotong"));
        let mut o = tutup(satu(b"a", b"x", b'0')); o.push(1);
        assert!(galat(&o).contains("sesudah penanda akhir"));
        let mut o = satu(b"a", b"x", b'0'); o.extend(satu(b"A", b"y", b'0'));
        assert!(galat(&tutup(o)).contains("dua kali"));
        assert!(galat(&satu(b"a", b"x", b'0')).contains("penanda akhir"));
        let mut o = satu(b"a", b"x", b'0'); o.extend(satu(b"a/b", b"y", b'0'));
        assert!(galat(&tutup(o)).contains("berkas dan juga folder"));
        // Tautan dibaca (untuk daftar), tidak pernah tanpa tujuan; sparse GNU ditolak.
        let mut h = kepala(b"t", 0, b'2'); h[157..161].copy_from_slice(b"/etc");
        h[148..156].copy_from_slice(b"        ");
        let c: u32 = h.iter().map(|&x| x as u32).sum();
        h[148..156].copy_from_slice(format!("{:06o}\0 ", c).as_bytes());
        let d = baca_daftar(&tutup(h.to_vec())).unwrap();
        assert_eq!((d[0].jenis, d[0].tautan.as_str()), (Jenis::Symlink, "/etc"));
        assert!(galat(&tutup(satu(b"t", b"", b'2'))).contains("tanpa tujuan"));
        assert!(galat(&tutup(satu(b"s", b"", b'S'))).contains("sparse"));
    }
}
