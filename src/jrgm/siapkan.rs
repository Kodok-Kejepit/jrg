// src/jrgm/siapkan.rs — `jrgm siapkan android`: memasang platform, build-tools dan NDK dari katalog resmi
// Google (repository2-3.xml: URL, ukuran, sidik sha1), tanpa sdkmanager. `jrgm apk` menawarkannya kalau
// perkakasnya belum ada. Katalog itu tidak punya build-tools/NDK untuk Linux aarch64; di sana ditolak.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const KATALOG: &str = "https://dl.google.com/android/repository/repository2-3.xml";
const DASAR: &str = "https://dl.google.com/android/repository/";

#[derive(Debug, Clone)]
pub(crate) struct Paket {
    pub(crate) jalur: String,
    pub(crate) versi: Vec<u64>,
    pub(crate) stabil: bool,
    pub(crate) url: String,
    pub(crate) sha1: String,
    pub(crate) ukuran: u64,
}

/// Isi `<tag>..</tag>` atau `<tag atribut>..</tag>` pertama.
fn isi_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let buka = format!("<{}", tag);
    let mut dari = 0;
    loop {
        let i = s[dari..].find(&buka)? + dari + buka.len();
        match s[i..].chars().next() {
            Some('>') | Some(' ') => {
                let a = s[i..].find('>')? + i + 1;
                let b = s[a..].find(&format!("</{}>", tag))? + a;
                return Some(s[a..b].trim());
            }
            _ => dari = i,
        }
    }
}

/// (os, arch) katalog untuk mesin ini, atau kenapa tidak ada.
pub(crate) fn tuan() -> Result<(&'static str, &'static str), String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("linux", "x64")),
        ("macos", "x86_64") => Ok(("macosx", "x64")),
        ("macos", "aarch64") => Ok(("macosx", "aarch64")),
        (os, arch) => Err(format!(
            "katalog Android dari Google tidak punya build-tools dan NDK untuk {} {} (cuma Linux x86_64, macOS, Windows)\n  \
             bantuan: bangun APK-nya di laptop/PC, atau pakai JRG IDE yang sudah membawa jrgc; di Termux/Ubuntu HP, \
             `jrgm run`/`test` jalan tanpa SDK", os, arch)),
    }
}

/// Paket-paket katalog yang punya arsip untuk mesin ini.
pub(crate) fn urai_katalog(xml: &str, os: &str, arch: &str) -> Vec<Paket> {
    let mut hasil = Vec::new();
    let mut sisa = xml;
    while let Some(i) = sisa.find("<remotePackage path=\"") {
        sisa = &sisa[i + 21..];
        let Some(j) = sisa.find('"') else { break };
        let jalur = sisa[..j].to_string();
        let akhir = sisa.find("</remotePackage>").unwrap_or(sisa.len());
        let badan = &sisa[..akhir];
        sisa = &sisa[akhir..];
        let rev = isi_tag(badan, "revision").unwrap_or("");
        let mut versi: Vec<u64> = ["major", "minor", "micro"].iter()
            .map(|t| isi_tag(rev, t).and_then(|v| v.parse().ok()).unwrap_or(0)).collect();
        // Platform diurutkan menurut level API-nya (`platforms;android-35`), bukan revisinya.
        if let Some(api) = jalur.strip_prefix("platforms;android-") {
            match api.parse::<u64>() { Ok(n) => versi.insert(0, n), Err(_) => continue }
        }
        if let Some(p) = isi_tag(rev, "preview").and_then(|v| v.parse().ok()) { versi.push(p); }
        let stabil = badan.contains("<channelRef ref=\"channel-0\"/>") && isi_tag(rev, "preview").is_none();
        let mut arsip = badan;
        while let Some(k) = arsip.find("<archive>") {
            let a = &arsip[k + 9..];
            let ujung = a.find("</archive>").unwrap_or(a.len());
            let satu = &a[..ujung];
            arsip = &a[ujung..];
            let os_ok = isi_tag(satu, "host-os").map_or(true, |o| o == os);
            let arch_ok = isi_tag(satu, "host-arch").map_or(true, |x| x == arch);
            if !(os_ok && arch_ok) { continue; }
            let (Some(url), Some(sha1)) = (isi_tag(satu, "url"), isi_tag(satu, "checksum")) else { continue };
            let ukuran = isi_tag(satu, "size").and_then(|v| v.parse().ok()).unwrap_or(0);
            hasil.push(Paket { jalur: jalur.clone(), versi: versi.clone(), stabil, url: url.to_string(), sha1: sha1.to_string(), ukuran });
            break;
        }
    }
    hasil
}

/// Versi stabil tertinggi dengan awalan `ns;` (mis. `ndk;`, `build-tools;`).
pub(crate) fn terbaru<'a>(p: &'a [Paket], awalan: &str) -> Option<&'a Paket> {
    p.iter().filter(|x| x.stabil && x.jalur.starts_with(awalan)).max_by(|a, b| a.versi.cmp(&b.versi))
}

fn ada(program: &str) -> bool {
    std::env::var_os("PATH").map_or(false, |p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// `[Y/n]` di terminal; tanpa terminal jawabannya `bawaan` hanya kalau `--ya`.
pub(crate) fn tanya(teks: &str, ya: bool) -> bool {
    if ya { return true; }
    if !std::io::stdin().is_terminal() { return false; }
    eprint!("{} [Y/n] ", teks);
    let _ = std::io::stderr().flush();
    let mut b = String::new();
    if std::io::stdin().lock().read_line(&mut b).is_err() { return false; }
    let b = b.trim().to_lowercase();
    b.is_empty() || b == "y" || b == "ya" || b == "yes"
}

fn pilih(teks: &str, pilihan: &[(String, String)], ya: bool) -> usize {
    if ya || !std::io::stdin().is_terminal() { return 0; }
    eprintln!("{}", teks);
    for (i, (a, b)) in pilihan.iter().enumerate() { eprintln!("  {}) {}  {}", i + 1, a, b); }
    eprint!("pilih [1]: ");
    let _ = std::io::stderr().flush();
    let mut s = String::new();
    let _ = std::io::stdin().lock().read_line(&mut s);
    s.trim().parse::<usize>().ok().filter(|n| *n >= 1 && *n <= pilihan.len()).map(|n| n - 1).unwrap_or(0)
}

fn tembolok() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir).join("jrgm").join("android")
}

fn unduh(url: &str, ke: &Path, judul: &str) -> Result<(), String> {
    let mut c = if ada("curl") {
        let mut c = Command::new("curl"); c.args(["-fL", "--retry", "3", "-o"]).arg(ke).arg(url); c
    } else if ada("wget") {
        let mut c = Command::new("wget"); c.args(["-q", "-O"]).arg(ke).arg(url); c
    } else {
        return Err("butuh `curl` atau `wget` untuk mengunduh\n  bantuan: `sudo apt install curl`".into());
    };
    let o = jrgc::putar::jalankan(&mut c, judul).map_err(|e| format!("gagal menjalankan pengunduh: {}", jrgc::host::galat_io(&e)))?;
    if !o.status.success() {
        let _ = std::fs::remove_file(ke);
        return Err(format!("unduhan {} gagal: {}", url, String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("")));
    }
    Ok(())
}

fn sha1_berkas(p: &Path) -> Result<String, String> {
    let (prog, arg): (&str, &[&str]) = if ada("sha1sum") { ("sha1sum", &[]) } else if ada("shasum") { ("shasum", &["-a", "1"]) }
        else { return Err("butuh `sha1sum` atau `shasum` untuk memeriksa sidik unduhan".into()) };
    let o = Command::new(prog).args(arg).arg(p).output().map_err(|e| jrgc::host::galat_io(&e))?;
    Ok(String::from_utf8_lossy(&o.stdout).split_whitespace().next().unwrap_or("").to_string())
}

/// Folder SDK untuk paket ini: `ndk;26.3.1` → `ndk/26.3.1`.
fn tujuan(sdk: &Path, p: &Paket) -> PathBuf {
    p.jalur.split(';').fold(sdk.to_path_buf(), |d, b| d.join(b))
}

/// Unduh (sekali, ditembolok), periksa sidiknya, buka ke tempatnya di SDK.
fn pasang(sdk: &Path, p: &Paket) -> Result<(), String> {
    let ke = tujuan(sdk, p);
    if ke.join("source.properties").is_file() { eprintln!("[jrgm] {} sudah ada", p.jalur); return Ok(()); }
    let t = tembolok();
    std::fs::create_dir_all(&t).map_err(|e| format!("{}: {}", t.display(), jrgc::host::galat_io(&e)))?;
    let zip = t.join(&p.url);
    if !zip.is_file() || sha1_berkas(&zip)? != p.sha1 {
        unduh(&format!("{}{}", DASAR, p.url), &zip, &format!("mengunduh {} ({} MB)", p.jalur, p.ukuran / 1_000_000))?;
    }
    let s = sha1_berkas(&zip)?;
    if s != p.sha1 {
        let _ = std::fs::remove_file(&zip);
        return Err(format!("sidik {} tidak cocok (dapat {}, katalog {}); unduhannya dibuang", p.url, s, p.sha1));
    }
    if !ada("unzip") { return Err("butuh `unzip` untuk membuka paket\n  bantuan: `sudo apt install unzip`".into()); }
    let sementara = sdk.join(".jrgm-sementara");
    let _ = std::fs::remove_dir_all(&sementara);
    std::fs::create_dir_all(&sementara).map_err(|e| jrgc::host::galat_io(&e))?;
    let mut c = Command::new("unzip");
    c.arg("-q").arg(&zip).arg("-d").arg(&sementara);
    let o = jrgc::putar::jalankan(&mut c, &format!("membuka {}", p.jalur)).map_err(|e| jrgc::host::galat_io(&e))?;
    if !o.status.success() { return Err(format!("unzip {} gagal: {}", p.url, String::from_utf8_lossy(&o.stderr))); }
    // Isi arsip satu folder (`android-ndk-r26d/`, `android-15/`): folder itu yang jadi tujuannya.
    let isi: Vec<PathBuf> = std::fs::read_dir(&sementara).map_err(|e| jrgc::host::galat_io(&e))?.flatten().map(|e| e.path()).collect();
    let akar = if isi.len() == 1 && isi[0].is_dir() { isi[0].clone() } else { sementara.clone() };
    if let Some(d) = ke.parent() { std::fs::create_dir_all(d).map_err(|e| jrgc::host::galat_io(&e))?; }
    let _ = std::fs::remove_dir_all(&ke);
    std::fs::rename(&akar, &ke).map_err(|e| format!("memindah {} ke {}: {}", akar.display(), ke.display(), jrgc::host::galat_io(&e)))?;
    let _ = std::fs::remove_dir_all(&sementara);
    eprintln!("[jrgm] {} terpasang di {}", p.jalur, ke.display());
    Ok(())
}

fn tawar_pasang_sistem(program: &str, apt: &str, pkg: &str, ya: bool) -> Result<(), String> {
    if ada(program) { return Ok(()); }
    let termux = std::env::var("PREFIX").map_or(false, |p| p.contains("com.termux"));
    let (alat, args): (&str, Vec<&str>) = if termux { ("pkg", vec!["install", "-y", pkg]) }
        else if ada("apt-get") { if ada("sudo") { ("sudo", vec!["apt-get", "install", "-y", apt]) } else { ("apt-get", vec!["install", "-y", apt]) } }
        else { return Err(format!("`{}` tidak ada; pasang `{}` lewat pengelola paket sistemmu", program, apt)); };
    if !tanya(&format!("`{}` belum ada. Pasang sekarang ({} {})?", program, alat, args.join(" ")), ya) {
        return Err(format!("`{}` dibutuhkan; pasang dulu: {} {}", program, alat, args.join(" ")));
    }
    let st = Command::new(alat).args(&args).status().map_err(|e| jrgc::host::galat_io(&e))?;
    if !st.success() { return Err(format!("`{} {}` gagal", alat, args.join(" "))); }
    Ok(())
}

pub(crate) struct Opsi {
    pub(crate) ya: bool,
    pub(crate) target_sdk: Option<u32>,
    pub(crate) sdk: Option<PathBuf>,
    pub(crate) tanpa_ndk: bool,
}

/// Folder SDK yang dipakai: `--sdk`, `$ANDROID_HOME`, lalu `~/Android/Sdk` (yang dicari `jrgm apk`).
fn folder_sdk(o: &Opsi) -> Result<PathBuf, String> {
    if let Some(d) = &o.sdk { return Ok(d.clone()); }
    for k in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(p) = std::env::var_os(k).filter(|p| !p.is_empty()) { return Ok(PathBuf::from(p)); }
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Android").join("Sdk")).ok_or_else(|| "HOME tidak diset".into())
}

pub(crate) fn android(o: &Opsi) -> Result<(), String> {
    let (os, arch) = tuan()?;
    let sdk = folder_sdk(o)?;
    for (prog, apt, pkg) in [("curl", "curl", "curl"), ("unzip", "unzip", "unzip"), ("java", "default-jdk-headless", "openjdk-17")] {
        if prog == "curl" && ada("wget") { continue; }
        tawar_pasang_sistem(prog, apt, pkg, o.ya)?;
    }
    let t = tembolok();
    std::fs::create_dir_all(&t).map_err(|e| jrgc::host::galat_io(&e))?;
    let kat = t.join("repository2-3.xml");
    unduh(KATALOG, &kat, "mengunduh katalog Android")?;
    let xml = std::fs::read_to_string(&kat).map_err(|e| jrgc::host::galat_io(&e))?;
    let semua = urai_katalog(&xml, os, arch);
    let platform_tertinggi = terbaru(&semua, "platforms;android-").ok_or("katalog tanpa platform Android")?;
    let tinggi = platform_tertinggi.jalur.trim_start_matches("platforms;android-").parse::<u32>().unwrap_or(35);
    let sasaran = match o.target_sdk {
        Some(n) => n,
        None => {
            let pil = vec![
                (format!("{}", tinggi), "terbaru (Play Store meminta yang terbaru)".to_string()),
                ("34".into(), "Android 14".into()),
                ("28".into(), "Android 9, seperti Termux: aplikasi boleh menjalankan program dari folder datanya".into()),
            ];
            pil[pilih("Target SDK Android:", &pil, o.ya)].0.parse().unwrap_or(tinggi)
        }
    };
    // Kompilasi butuh platform >= target; target lama tetap memakai platform terbaru.
    let platform = semua.iter().find(|p| p.jalur == format!("platforms;android-{}", sasaran.max(tinggi)))
        .or(Some(platform_tertinggi)).cloned().unwrap();
    let mut rencana = vec![platform, terbaru(&semua, "build-tools;").ok_or("katalog tanpa build-tools")?.clone()];
    if !o.tanpa_ndk { rencana.push(terbaru(&semua, "ndk;").ok_or("katalog tanpa NDK")?.clone()); }
    rencana.retain(|p| !tujuan(&sdk, p).join("source.properties").is_file());
    if rencana.is_empty() { eprintln!("[jrgm] perkakas Android di {} sudah lengkap", sdk.display()); }
    else {
        let total: u64 = rencana.iter().map(|p| p.ukuran).sum();
        eprintln!("[jrgm] akan dipasang di {} ({} MB diunduh):", sdk.display(), total / 1_000_000);
        for p in &rencana { eprintln!("  {}  ({} MB)", p.jalur, p.ukuran / 1_000_000); }
        eprintln!("  Lisensinya Android Software Development Kit License Agreement: https://developer.android.com/studio/terms");
        if !tanya("Setuju dengan lisensinya dan pasang sekarang?", o.ya) { return Err("dibatalkan; tidak ada yang dipasang".into()); }
        for p in &rencana { pasang(&sdk, p)?; }
    }
    if ada("rustup") {
        let terpasang = Command::new("rustup").args(["target", "list", "--installed"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let kurang: Vec<&str> = ["aarch64-linux-android", "x86_64-linux-android"].into_iter().filter(|t| !terpasang.lines().any(|l| l.trim() == *t)).collect();
        if !kurang.is_empty() && tanya(&format!("Target Rust {} belum ada (dipakai `jrgm apk`). Pasang?", kurang.join(", ")), o.ya) {
            let st = Command::new("rustup").args(["target", "add"]).args(&kurang).status().map_err(|e| jrgc::host::galat_io(&e))?;
            if !st.success() { return Err("`rustup target add` gagal".into()); }
        }
    } else {
        eprintln!("[jrgm] catatan: rustup tidak ada; `jrgm apk` butuh Rust (https://rustup.rs) untuk membangun runtime Android");
    }
    let bawaan = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Android").join("Sdk"));
    if bawaan.as_deref() != Some(sdk.as_path()) && std::env::var_os("ANDROID_HOME").is_none() {
        eprintln!("[jrgm] set `export ANDROID_HOME={}` supaya `jrgm apk` menemukannya", sdk.display());
    }
    if sasaran != 34 { eprintln!("[jrgm] tulis `target_sdk = {}` di [android] jrgm.toml untuk memakainya", sasaran); }
    eprintln!("[jrgm] siap: jalankan `jrgm apk`");
    Ok(())
}

/// Galat `jrgm apk` yang berarti perkakas Android belum ada.
pub(crate) fn galat_perkakas(e: &str) -> bool {
    ["SDK Android tidak ditemukan", "NDK Android tidak ditemukan", "build-tools Android", "platform Android (android.jar)"]
        .iter().any(|k| e.contains(k))
}

#[cfg(test)]
mod tes {
    use super::*;

    const CONTOH: &str = r#"<remotePackage path="ndk;26.3.11579264"><revision><major>26</major><minor>3</minor><micro>11579264</micro></revision>
<channelRef ref="channel-0"/><archives><archive><complete><size>100</size><checksum type="sha1">aa</checksum><url>ndk-mac.zip</url></complete><host-os>macosx</host-os></archive>
<archive><complete><size>200</size><checksum type="sha1">bb</checksum><url>ndk-linux.zip</url></complete><host-os>linux</host-os><host-arch>x64</host-arch></archive></archives></remotePackage>
<remotePackage path="ndk;27.0.1"><revision><major>27</major><minor>0</minor><micro>1</micro><preview>1</preview></revision><channelRef ref="channel-2"/>
<archives><archive><complete><size>1</size><checksum type="sha1">cc</checksum><url>x.zip</url></complete><host-os>linux</host-os></archive></archives></remotePackage>
<remotePackage path="platforms;android-35"><revision><major>3</major></revision><channelRef ref="channel-0"/><archives><archive><complete><size>5</size><checksum type="sha1">dd</checksum><url>p35.zip</url></complete></archive></archives></remotePackage>"#;

    #[test]
    fn katalog_diurai_per_mesin_dan_pratinjau_dilewati() {
        let p = urai_katalog(CONTOH, "linux", "x64");
        let ndk = terbaru(&p, "ndk;").unwrap();
        assert_eq!((ndk.jalur.as_str(), ndk.url.as_str(), ndk.sha1.as_str(), ndk.ukuran), ("ndk;26.3.11579264", "ndk-linux.zip", "bb", 200));
        assert_eq!(terbaru(&p, "platforms;").unwrap().url, "p35.zip", "arsip tanpa host-os berlaku di mana pun");
        assert!(urai_katalog(CONTOH, "linux", "aarch64").iter().all(|x| x.jalur != "ndk;26.3.11579264"), "tidak ada NDK aarch64");
        assert_eq!(tujuan(Path::new("/s"), ndk), PathBuf::from("/s/ndk/26.3.11579264"));
        let xml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/jrgm/main.rs")).unwrap();
        assert!(urai_katalog(&xml, "linux", "x64").is_empty(), "bukan katalog: kosong, bukan panik");
    }
}
