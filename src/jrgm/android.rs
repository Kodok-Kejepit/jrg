// src/jrgm/android.rs — `jrgm apk` dan `jrgm run --android`: proyek jadi APK lewat mesin
// `jrgc --apk` yang sama, dengan identitas dari `[android]` di jrgm.toml.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::dependensi::tambah_path_dependensi;
use super::manifes::Manifest;
use jrgc::llvm::apk::{self, Aplikasi, OpsiApk};

const KUNCI_ANDROID: &[&str] = &["nama", "paket", "versi", "kode", "orientasi", "ikon", "abi", "pustaka", "kunci", "alias",
                                  "izin", "tampilan", "port", "mesin", "bahasa", "[android.pustaka_c]", "[android.pustaka_rust]",
                                  "[android.program_c]", "target_sdk", "abaikan", "tautan", "ikon_latar", "ikon_depan"];

/// Opsi APK dari manifes; `abi` dari baris perintah menang atas `[android] abi`.
pub(crate) fn opsi_dari_manifes(m: &Manifest, abi: Option<&str>) -> Result<OpsiApk, String> {
    let a = &m.android;
    if let Some(r) = super::bahasa::resep_proyek(m)? {
        if !super::konsol::didukung(r) {
            return Err(format!("jrgm apk membungkus program JRG, C, dan C++; proyek {} belum bisa jadi APK\n  bantuan: tulis \
                aplikasinya dalam JRG (`jrgm new app --android`, atau `[android] tampilan = \"web\"`) dan bawa \
                penjalan bahasa lain lewat `[android] bahasa = [\"lua\", \"js\"]`", r.nama));
        }
        if a.tampilan.is_some() || a.mesin.is_some() {
            return Err(format!("proyek {} jadi aplikasi konsol; `tampilan` dan `mesin` di [android] tidak berlaku", r.nama));
        }
    }
    if let Some(k) = a.asing.first() {
        return Err(format!("kunci `{}` di [android] tidak dikenal\n  bantuan: yang ada {}", k,
                           KUNCI_ANDROID.iter().map(|k| format!("`{}`", k)).collect::<Vec<_>>().join(", ")));
    }
    let kode = match &a.kode {
        Some(k) => Some(k.parse::<u32>().map_err(|_| format!("`kode = {}` di [android] harus bilangan bulat positif", k))?),
        None => None,
    };
    let web = match a.tampilan.as_deref() {
        None | Some("kanvas") => false,
        Some("web") => true,
        Some(l) => return Err(format!("tampilan `{}` di [android] tidak dikenal\n  bantuan: `kanvas` (bawaan: program \
                                       menggambar sendiri) atau `web` (halaman yang dilayani programnya di WebView)", l)),
    };
    let port = match &a.port {
        Some(p) => Some(p.parse::<u16>().ok().filter(|n| *n >= 1024).ok_or_else(|| format!(
            "`port = {}` di [android] harus angka 1024 sampai 65535", p))?),
        None => None,
    };
    match a.mesin.as_deref() {
        None | Some("interp") | Some("vm") => {}
        Some(l) => return Err(format!("mesin `{}` di [android] tidak dikenal\n  bantuan: `interp` (bawaan) atau `vm`", l)),
    }
    if !web && (a.mesin.is_some() || a.port.is_some()) {
        return Err("`mesin` dan `port` di [android] cuma berlaku untuk `tampilan = \"web\"`; tampilan kanvas \
                    dikompilasi lewat LLVM dan tidak melayani port".into());
    }
    for b in &a.bahasa { apk::apk_bahasa_dikenal(b)?; }
    let abi: Vec<String> = match abi {
        Some(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        None if !a.abi.is_empty() => a.abi.clone(),
        None => vec!["arm64-v8a".to_string()],
    };
    let target_sdk = match &a.target_sdk {
        None => None,
        Some(t) => Some(t.parse::<u32>().map_err(|_| format!("`target_sdk = {}` di [android] bukan bilangan", t))?),
    };
    if target_sdk.map_or(false, |t| t < 33) {
        eprintln!("[jrgm] peringatan: `target_sdk = {}` — Play Store menolak APK di bawah 33; \
                   cocok untuk rilis di luar Play (F-Droid, GitHub)", target_sdk.unwrap_or(0));
    }
    Ok(OpsiApk {
        abi,
        opt_level: 2,
        rilis: false,
        aab: false,
        aplikasi: Aplikasi {
            nama: a.nama.clone().or_else(|| Some(m.name.clone())),
            paket: a.paket.clone(),
            versi: a.versi.clone().or_else(|| Some(m.version.clone())),
            kode,
            orientasi: a.orientasi.clone(),
            ikon: a.ikon.clone(),
            pustaka: a.pustaka.clone(),
            kunci: a.kunci.clone(),
            alias: a.alias.clone(),
            izin: a.izin.clone(),
            pustaka_c: a.pustaka_c.clone(),
            pustaka_rust: a.pustaka_rust.clone(),
            program_c: a.program_c.clone(),
            bahasa: a.bahasa.clone(),
            berkas_proyek: Vec::new(),
            entri: if web { Some("src/main.jrg".to_string()) } else { None },
            port,
            mesin: a.mesin.clone(),
            impor: Vec::new(),
            target_sdk,
            tautan: a.tautan.clone(),
            ikon_latar: a.ikon_latar.clone(),
            ikon_depan: a.ikon_depan.clone(),
        },
    })
}

/// Berkas yang TIDAK ikut ke APK: keluaran build, riwayat git, kunci, dan paket yang sudah jadi.
fn dilewati(nama: &str) -> bool {
    nama == "target" || nama.starts_with('.') || [".jks", ".keystore", ".apk", ".aab", ".idsig"].iter().any(|e| nama.ends_with(e))
}

/// `abaikan = ["linux/proot", "uji"]`: jalur relatif akar proyek yang tidak ikut ke APK.
fn diabaikan(di: &str, abaikan: &[String]) -> bool {
    abaikan.iter().any(|a| { let a = a.trim_matches('/'); di == a || di.starts_with(&format!("{}/", a)) })
}

pub(crate) fn kumpul_proyek_pub(akar: &Path, abaikan: &[String], keluar: &mut Vec<(String, PathBuf)>) {
    kumpul_proyek(akar, "", abaikan, keluar)
}

fn kumpul_proyek(dir: &Path, awalan: &str, abaikan: &[String], keluar: &mut Vec<(String, PathBuf)>) {
    let Ok(isi) = std::fs::read_dir(dir) else { return };
    let mut isi: Vec<PathBuf> = isi.flatten().map(|e| e.path()).collect();
    isi.sort();
    for p in isi {
        let nama = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if dilewati(&nama) { continue; }
        let di = if awalan.is_empty() { nama.clone() } else { format!("{}/{}", awalan, nama) };
        if diabaikan(&di, abaikan) { continue; }
        if p.is_dir() { kumpul_proyek(&p, &di, abaikan, keluar) } else { keluar.push((di, p)) }
    }
}

/// Tampilan `"web"`: tiap berkas proyek dan dependensinya diperiksa sintaksnya, lalu ikut sebagai
/// aset; `jrgc` Android di dalam APK yang menjalankannya.
fn bangun_web(akar: &Path, m: &Manifest, opsi: &OpsiApk, tujuan: &Path, verbose: bool) -> Result<PathBuf, String> {
    let entri = m.entry.strip_prefix(akar).map_err(|_| "entri proyek harus di dalam direktori proyek".to_string())?
        .to_string_lossy().replace('\\', "/");
    let mut app = opsi.aplikasi.clone();
    app.entri = Some(entri);
    kumpul_proyek(akar, "", &m.android.abaikan, &mut app.berkas_proyek);
    let mut dep = Vec::new();
    tambah_path_dependensi(akar, &mut dep);
    for (i, d) in dep.iter().enumerate() {
        let nama = format!("_dep/{}", i);
        kumpul_proyek(d, &nama, &[], &mut app.berkas_proyek);
        app.impor.push(nama);
    }
    for (nama, p) in &app.berkas_proyek {
        if nama.ends_with(".jrg") { super::jalankan::check_jrg_file(p, false)?; }
    }
    if let Some(d) = tujuan.parent() { let _ = std::fs::create_dir_all(d); }
    let opsi = OpsiApk { abi: opsi.abi.clone(), opt_level: opsi.opt_level, rilis: opsi.rilis, aab: opsi.aab, aplikasi: app };
    apk::bangun(None, Path::new(&m.name), tujuan, &opsi, verbose)
}

/// Urai entri proyek, turunkan ke IR LLVM, lalu bungkus jadi APK.
pub(crate) fn bangun_apk(akar: &Path, m: &Manifest, opsi: &OpsiApk, tujuan: &Path, verbose: bool) -> Result<PathBuf, String> {
    if let Some(r) = super::bahasa::resep_proyek(m)? { return super::konsol::bangun(akar, m, r, opsi, tujuan, verbose); }
    if opsi.aplikasi.entri.is_some() { return bangun_web(akar, m, opsi, tujuan, verbose); }
    let src = std::fs::read_to_string(&m.entry)
        .map_err(|e| format!("tidak bisa baca `{}`: {}", m.entry.display(), jrgc::host::galat_io(&e)))?;
    let sf = jrgc::lexer::SourceFile { id: 0, name: m.entry.display().to_string(), src };
    let mut peta = jrgc::diag::SourceMap::with_file(sf.clone());
    let token = jrgc::pipa::lex_dilaporkan(&sf, &peta).ok_or_else(String::new)?;
    let (ast, galat) = jrgc::parser::parse(token, false);
    if !galat.is_empty() {
        jrgc::pipa::laporkan_galat_parse(&galat, false, &peta);
        return Err(String::new());
    }
    let mut jalur = vec![m.entry.parent().map(Path::to_path_buf).unwrap_or_else(|| akar.to_path_buf()), akar.to_path_buf()];
    tambah_path_dependensi(akar, &mut jalur);
    jalur.dedup();
    let ir = jrgc::llvm::generate_ir_dgn_jalur(&ast,
        jrgc::llvm::codegen::CodegenOptions { fast_math: false, full_buffered_stdout: false }, &jalur)
        .map_err(|errs| {
            let g: Vec<_> = errs.iter().map(|e| jrgc::llvm::galat::GalatLlvm::dari_pesan(e)).collect();
            jrgc::diag::laporkan_llvm(&mut peta, &g);
            String::new()
        })?;
    if let Some(d) = tujuan.parent() { let _ = std::fs::create_dir_all(d); }
    // Nama berkas sumber cuma dipakai untuk nama bawaan; identitasnya sudah ada di `opsi`.
    apk::bangun(Some(&ir), Path::new(&m.name), tujuan, opsi, verbose)
}

pub(crate) fn tujuan_bawaan(akar: &Path, m: &Manifest) -> PathBuf {
    akar.join("target").join(format!("{}.apk", m.name))
}

pub(crate) fn cmd_apk(abi: Option<&str>, keluaran: Option<&Path>, rilis: bool, aab: bool, verbose: bool) -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;
    let m = Manifest::load(&akar)?;
    let mut opsi = opsi_dari_manifes(&m, abi)?;
    opsi.rilis = rilis;
    // `-o x.aab` juga berarti AAB; nama bawaannya mengikuti jenis dan tanda tangannya.
    opsi.aab = aab || keluaran.map_or(false, |k| k.extension().map_or(false, |e| e == "aab"));
    let tujuan = keluaran.map(Path::to_path_buf).unwrap_or_else(|| akar.join("target").join(format!(
        "{}{}.{}", m.name, if rilis { "-rilis" } else { "" }, if opsi.aab { "aab" } else { "apk" })));
    let hasil = match bangun_apk(&akar, &m, &opsi, &tujuan, verbose) {
        Err(e) if super::siapkan::galat_perkakas(&e) => {
            eprintln!("{}", e);
            let o = super::siapkan::Opsi { ya: false, target_sdk: opsi.aplikasi.target_sdk, sdk: None, tanpa_ndk: false };
            if !super::siapkan::tanya("Perkakas Android belum lengkap. Mau dibantu dipasangkan sekarang (`jrgm siapkan android`)?", false) {
                return Err("perkakas Android belum ada\n  bantuan: `jrgm siapkan android` memasangnya".into());
            }
            super::siapkan::android(&o)?;
            bangun_apk(&akar, &m, &opsi, &tujuan, verbose)?
        }
        h => h?,
    };
    eprintln!("[jrgm] {}{}: {} ({})", if opsi.aab { "AAB" } else { "APK" }, if rilis { " rilis" } else { "" },
              hasil.display(), opsi.abi.join(", "));
    Ok(())
}

/// Buat kunci rilis sekali: keytool menanyakan nama dan sandinya langsung di terminal.
/// Kunci yang sudah ada TIDAK ditimpa — kehilangan kunci berarti aplikasinya tidak bisa diperbarui lagi.
pub(crate) fn cmd_kunci() -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;
    let m = Manifest::load(&akar)?;
    let kunci = m.android.kunci.clone().unwrap_or_else(|| akar.join("rilis.jks"));
    let alias = m.android.alias.clone().unwrap_or_else(|| "rilis".to_string());
    if kunci.exists() {
        return Err(format!("kunci `{}` sudah ada — tidak ditimpa\n  catatan: aplikasi yang ditandatangani kunci \
                            ini cuma bisa diperbarui dengan kunci yang SAMA; simpan salinannya di tempat aman", kunci.display()));
    }
    let st = Command::new("keytool").arg("-genkeypair").arg("-keystore").arg(&kunci)
        .args(["-alias", &alias, "-keyalg", "RSA", "-keysize", "4096", "-validity", "10000"])
        .status().map_err(|e| format!("gagal menjalankan keytool (butuh Java): {}", jrgc::host::galat_io(&e)))?;
    if !st.success() { return Err("keytool tidak membuat kuncinya".into()); }
    if m.android.kunci.is_none() { tulis_kunci_ke_manifes(&akar, &alias)?; }
    eprintln!("[jrgm] kunci rilis: {} (alias `{}`)\n  \
               simpan salinannya dan sandinya: tanpa keduanya aplikasi ini tidak bisa diperbarui\n  \
               bangun: jrgm apk --rilis (sandi lewat JRG_KUNCI_SANDI, atau ditanyakan)", kunci.display(), alias);
    Ok(())
}

/// Tambah `kunci`/`alias` ke bagian `[android]` (dibuat kalau belum ada); baris lain tidak disentuh.
fn tulis_kunci_ke_manifes(akar: &Path, alias: &str) -> Result<(), String> {
    let p = akar.join("jrgm.toml");
    let isi = std::fs::read_to_string(&p).map_err(|e| format!("tidak bisa baca jrgm.toml: {}", jrgc::host::galat_io(&e)))?;
    let baris = format!("kunci     = \"rilis.jks\"\nalias     = \"{}\"", alias);
    let baru = match isi.lines().position(|l| l.trim() == "[android]") {
        Some(i) => {
            let mut v: Vec<String> = isi.lines().map(String::from).collect();
            v.insert(i + 1, baris);
            v.join("\n") + "\n"
        }
        None => format!("{}\n[android]\n{}\n", isi.trim_end(), baris),
    };
    std::fs::write(&p, baru).map_err(|e| format!("tidak bisa tulis jrgm.toml: {}", jrgc::host::galat_io(&e)))
}

fn adb_keluaran(adb: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(adb).args(args).output().map_err(|e| format!("gagal menjalankan adb: {}", jrgc::host::galat_io(&e)))?;
    if !out.status.success() {
        return Err(format!("`adb {}` gagal:\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Perangkat yang dituju: tepat satu yang siap, atau `ANDROID_SERIAL`; yang lain dijelaskan.
fn pilih_perangkat(adb: &Path) -> Result<(), String> {
    if std::env::var_os("ANDROID_SERIAL").is_some() { return Ok(()); }
    let daftar = adb_keluaran(adb, &["devices"])?;
    let baris: Vec<(&str, &str)> = daftar.lines().skip(1)
        .filter_map(|l| l.split_once('\t')).collect();
    let siap: Vec<&str> = baris.iter().filter(|(_, s)| *s == "device").map(|(n, _)| *n).collect();
    match siap.len() {
        1 => Ok(()),
        0 => {
            if let Some((n, s)) = baris.first() {
                let sebab = match *s {
                    "unauthorized" => "belum diizinkan: ketuk \"Izinkan\" di dialog USB debugging pada HP",
                    "offline" => "offline: cabut-colok kabelnya, atau coba kabel/port lain",
                    lain => lain,
                };
                Err(format!("perangkat `{}` {}", n, sebab))
            } else {
                Err("tidak ada HP atau emulator yang tersambung\n  bantuan: nyalakan USB debugging di HP, \
                     colok kabel data, lalu cek dengan `adb devices`".into())
            }
        }
        _ => Err(format!("{} perangkat tersambung ({})\n  bantuan: pilih satu dengan `ANDROID_SERIAL=<nama> jrgm run --android`",
                         siap.len(), siap.join(", "))),
    }
}

/// Bangun untuk ABI perangkat, pasang, buka, lalu tampilkan keluaran programnya sampai ia ditutup.
pub(crate) fn cmd_run_android(verbose: bool) -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;
    let m = Manifest::load(&akar)?;
    let adb = apk::cari_adb()?;
    pilih_perangkat(&adb)?;
    let abi = adb_keluaran(&adb, &["shell", "getprop", "ro.product.cpu.abi"])?;
    let mut opsi = opsi_dari_manifes(&m, Some(&abi))?;
    opsi.abi = vec![abi.clone()];
    let tujuan = tujuan_bawaan(&akar, &m);
    bangun_apk(&akar, &m, &opsi, &tujuan, verbose)?;
    let paket = opsi.aplikasi.paket.clone().unwrap_or_else(|| apk::nama_paket(&m.name));
    eprintln!("[jrgm] memasang {} ({}) ...", paket, abi);
    // Cuma untuk pengguna yang sedang aktif: profil lain (mis. Secure Folder) tidak disentuh.
    let t = tujuan.to_string_lossy().to_string();
    adb_keluaran(&adb, &["install", "-r", "--user", "current", &t])?;
    let _ = adb_keluaran(&adb, &["logcat", "-c"]);
    let aktivitas = if opsi.aplikasi.entri.is_some() { "jrg.apk.Aktivitas" } else { "android.app.NativeActivity" };
    adb_keluaran(&adb, &["shell", "am", "start", "-n", &format!("{}/{}", paket, aktivitas)])?;
    eprintln!("[jrgm] jalan di perangkat; keluarannya di bawah (tutup aplikasinya atau Ctrl-C untuk berhenti)");
    let mut log = Command::new(&adb).args(["logcat", "-v", "raw", "-s", "jrg:I"])
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        .map_err(|e| format!("gagal membaca logcat: {}", jrgc::host::galat_io(&e)))?;
    let keluar = log.stdout.take();
    let utas = std::thread::spawn(move || {
        if let Some(k) = keluar {
            for baris in std::io::BufReader::new(k).lines().map_while(Result::ok) {
                if !baris.starts_with("--------- beginning of") { println!("{}", baris); }
            }
        }
    });
    let mut pernah_hidup = false;
    for detik in 0.. {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let hidup = adb_keluaran(&adb, &["shell", "pidof", &paket]).map_or(false, |p| !p.is_empty());
        if hidup { pernah_hidup = true; }
        if !hidup && (pernah_hidup || detik >= 15) { break; }
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    let _ = log.kill();
    let _ = log.wait();
    let _ = utas.join();
    if !pernah_hidup {
        return Err(format!("aplikasi `{}` tidak terlihat jalan dalam 15 detik\n  bantuan: lihat `adb logcat` untuk sebabnya", paket));
    }
    eprintln!("[jrgm] aplikasi ditutup");
    Ok(())
}
