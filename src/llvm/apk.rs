// src/llvm/apk.rs — `jrgc program.jrg --apk app.apk`: program jadi pustaka NativeActivity,
// dibungkus APK bertanda tangan debug. Tanpa Java/Kotlin dan tanpa Gradle.

use std::path::{Path, PathBuf};
use std::process::Command;

/// API Android terendah: `ANativeWindow`, `AAudio` dan 64 bit penuh ada sejak 26.
const API: u32 = 26;

#[derive(Debug)]
pub struct OpsiApk {
    pub abi: Vec<String>,
    pub opt_level: u8,
    /// Tanda tangan rilis dari `aplikasi.kunci`, bukan kunci debug.
    pub rilis: bool,
    /// Keluarannya Android App Bundle (`.aab`, untuk Play Store), bukan APK.
    pub aab: bool,
    pub aplikasi: Aplikasi,
}

/// Identitas aplikasi; yang kosong diturunkan dari nama berkas (`jrgm` mengisinya dari `[android]`).
#[derive(Default, Clone, Debug)]
pub struct Aplikasi {
    pub nama: Option<String>,
    pub paket: Option<String>,
    pub versi: Option<String>,
    pub kode: Option<u32>,
    pub orientasi: Option<String>,
    pub ikon: Option<PathBuf>,
    /// Pustaka native milik proyek; `{abi}` diganti nama ABI, mis. `native/{abi}/libhitung.so`.
    pub pustaka: Vec<String>,
    /// Kunci rilis (keystore milik pemakai) dan aliasnya; sandinya TIDAK pernah ditulis di berkas.
    pub kunci: Option<PathBuf>,
    pub alias: Option<String>,
    /// Izin Android dengan nama ramah (`internet`, `getar`, ...).
    pub izin: Vec<String>,
    /// Pustaka yang DIKOMPILASI dari sumber C/C++ proyek: `(libnama, [berkas])`.
    pub pustaka_c: Vec<(String, Vec<PathBuf>)>,
    /// Pustaka dari crate Rust proyek (`crate-type = ["cdylib"]`): `(libnama, direktori crate)`.
    pub pustaka_rust: Vec<(String, PathBuf)>,
    /// Program (executable) dari sumber C/C++ proyek, dipasang sebagai `lib/<abi>/<libnama>.so`.
    pub program_c: Vec<(String, Vec<PathBuf>)>,
    /// Penjalan bahasa lain yang ikut di APK (`lua`, `js`, `sql`); resepnya di `apk_bahasa`.
    pub bahasa: Vec<String>,
    /// Tampilan `"web"`: `(jalur di dalam proyek, berkas)` yang ikut ke `assets/proyek/`.
    pub berkas_proyek: Vec<(String, PathBuf)>,
    /// Tampilan `"web"`: berkas entri (relatif proyek), port pelayannya, mesin, dan direktori `-I`.
    pub entri: Option<String>,
    pub port: Option<u16>,
    pub mesin: Option<String>,
    pub impor: Vec<String>,
    /// `targetSdkVersion`; bawaan 34. Di bawah 29 program yang diunduh ke folder data bisa
    /// dijalankan (seperti Termux), dan Play Store menolaknya.
    pub target_sdk: Option<u32>,
    /// Pustaka sistem tambahan untuk `pustaka_c`/`program_c` (`["z"]` jadi `-lz`).
    pub tautan: Vec<String>,
    /// Ikon adaptif Android 8+: latar (warna `#RRGGBB` atau PNG) dan latar depan (PNG).
    pub ikon_latar: Option<String>,
    pub ikon_depan: Option<PathBuf>,
}

/// Izin yang cukup ditulis di manifes: dipasang bersama aplikasinya, tanpa dialog.
const IZIN_NORMAL: &[(&str, &str)] = &[
    ("internet", "INTERNET"), ("jaringan", "ACCESS_NETWORK_STATE"), ("wifi", "ACCESS_WIFI_STATE"),
    ("getar", "VIBRATE"), ("tetap_menyala", "WAKE_LOCK"),
];
// `latar` (izin_android) bukan satu izin: layanan latar, notifikasinya, dan wake lock.
/// Izin "berbahaya": Android memintanya lewat dialog SAAT JALAN. Tampilan `"web"` memunculkannya
/// (`JRGAndroid.mintaIzin`); tampilan kanvas belum bisa, jadi di sana ditolak.
const IZIN_BERBAHAYA: &[(&str, &str)] = &[
    ("kamera", "CAMERA"), ("mikrofon", "RECORD_AUDIO"), ("lokasi", "ACCESS_FINE_LOCATION"),
    ("kontak", "READ_CONTACTS"), ("penyimpanan", "READ_EXTERNAL_STORAGE"), ("telepon", "CALL_PHONE"),
    ("sms", "SEND_SMS"), ("kalender", "READ_CALENDAR"),
];

/// Nama izin ramah atau `android.permission.X` ke nama Android-nya. `dialog`: aplikasinya bisa
/// memunculkan dialog izin, jadi izin berbahaya boleh; tanpa itu ia ditolak, begitu juga yang asing.
pub fn izin_android(daftar: &[String], dialog: bool) -> Result<Vec<String>, String> {
    let mut keluar = Vec::new();
    for i in daftar {
        // `latar`: tetap jalan di latar dengan notifikasi (Layanan di Aktivitas.java, `JRGAndroid.latar`).
        if i == "latar" {
            if !dialog { return Err("izin `latar` cuma untuk `tampilan = \"web\"`".into()); }
            for a in ["FOREGROUND_SERVICE", "FOREGROUND_SERVICE_SPECIAL_USE", "POST_NOTIFICATIONS", "WAKE_LOCK"] {
                let penuh = format!("android.permission.{}", a);
                if !keluar.contains(&penuh) { keluar.push(penuh); }
            }
            continue;
        }
        let pendek = i.strip_prefix("android.permission.").unwrap_or(i);
        let cocok = |t: &[(&str, &'static str)]| t.iter().find(|(r, a)| *r == pendek || *a == pendek).map(|(_, a)| *a);
        if let Some(a) = cocok(IZIN_NORMAL) {
            let penuh = format!("android.permission.{}", a);
            if !keluar.contains(&penuh) { keluar.push(penuh); }
        } else if let Some(a) = cocok(IZIN_BERBAHAYA) {
            if !dialog {
                return Err(format!("izin `{}` harus diminta lewat dialog SAAT aplikasi jalan, dan tampilan kanvas belum bisa \
                                    memunculkannya — ditolak daripada dipasang tanpa pernah diberikan\n  \
                                    bantuan: `tampilan = \"web\"` bisa (halamannya memanggil `JRGAndroid.mintaIzin`)", i));
            }
            let penuh = format!("android.permission.{}", a);
            if !keluar.contains(&penuh) { keluar.push(penuh); }
            // WebView baru mau membuka mikrofon kalau izin NORMAL ini juga ada ("Could not start audio source").
            let atur = "android.permission.MODIFY_AUDIO_SETTINGS".to_string();
            if a == "RECORD_AUDIO" && !keluar.contains(&atur) { keluar.push(atur); }
            // Menulis ke /sdcard butuh izin tulisnya juga (satu grup, satu dialog) di targetSdk 28 ke bawah.
            let tulis = "android.permission.WRITE_EXTERNAL_STORAGE".to_string();
            if a == "READ_EXTERNAL_STORAGE" && !keluar.contains(&tulis) { keluar.push(tulis); }
        } else {
            return Err(format!("izin `{}` tidak dikenal\n  bantuan: yang ada {}", i,
                IZIN_NORMAL.iter().chain(IZIN_BERBAHAYA).map(|(r, _)| format!("`{}`", r)).collect::<Vec<_>>().join(", ")));
        }
    }
    Ok(keluar)
}

/// `libhitung` atau `libhitung.so` -> `libhitung.so`; nama lain tidak bisa dipasang Android.
pub fn nama_pustaka(n: &str) -> Result<String, String> {
    let dasar = n.strip_suffix(".so").unwrap_or(n);
    let sah = dasar.len() > 3 && dasar.starts_with("lib")
        && dasar.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if sah { Ok(format!("{}.so", dasar)) } else {
        Err(format!("nama pustaka `{}` tidak sah\n  bantuan: diawali `lib`, huruf/angka/`_` saja, mis. `libhitung`", n))
    }
}

/// Paket Android: paling sedikit dua bagian, tiap bagian huruf kecil lalu huruf/angka/`_`.
pub fn periksa_paket(p: &str) -> Result<(), String> {
    let bagian: Vec<&str> = p.split('.').collect();
    let sah = bagian.len() >= 2 && bagian.iter().all(|b| {
        b.starts_with(|c: char| c.is_ascii_lowercase())
            && b.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    });
    if sah { Ok(()) } else {
        Err(format!("nama paket `{}` tidak sah\n  bantuan: paling sedikit dua bagian dipisah titik, \
                     tiap bagian diawali huruf kecil, mis. `id.namaku.gameku`", p))
    }
}

/// `orientasi` ke nilai `android:screenOrientation`.
pub fn orientasi_android(o: &str) -> Result<&'static str, String> {
    match o {
        "bebas" => Ok("unspecified"),
        "landscape" | "mendatar" => Ok("sensorLandscape"),
        "portrait" | "tegak" => Ok("sensorPortrait"),
        lain => Err(format!("orientasi `{}` tidak dikenal\n  bantuan: yang ada `bebas`, `landscape` \
                             (atau `mendatar`), `portrait` (atau `tegak`)", lain)),
    }
}

/// `1.2.3` -> 1002003: naik tiap versi naik, seperti yang diminta Android untuk pembaruan.
pub fn kode_dari_versi(v: &str) -> Result<u32, String> {
    let angka: Vec<&str> = v.split('.').collect();
    let mut kode: u64 = 0;
    if angka.is_empty() || angka.len() > 3 { return Err(pesan_versi(v)); }
    for i in 0..3 {
        let n: u64 = match angka.get(i) {
            Some(a) => a.parse().map_err(|_| pesan_versi(v))?,
            None => 0,
        };
        if n > 999 { return Err(pesan_versi(v)); }
        kode = kode * 1000 + n;
    }
    if kode == 0 { return Err(pesan_versi(v)); }
    Ok(kode as u32)
}

fn pesan_versi(v: &str) -> String {
    format!("versi `{}` tidak bisa jadi kode versi Android\n  bantuan: tulis `utama.minor.tambalan` \
             dengan angka 0 sampai 999 (mis. `1.2.0`), atau isi `kode = <angka>` sendiri", v)
}

struct Abi {
    nama: &'static str,
    rust: &'static str,
    clang: &'static str,
}

const ABI: &[Abi] = &[
    Abi { nama: "arm64-v8a", rust: "aarch64-linux-android", clang: "aarch64-linux-android" },
    Abi { nama: "x86_64", rust: "x86_64-linux-android", clang: "x86_64-linux-android" },
];

struct Alat {
    /// `d8` (pengubah kelas Java ke dex), kalau SDK-nya punya; cuma dipakai tampilan `"web"`.
    d8: Option<PathBuf>,
    ndk_bin: PathBuf,
    glue: PathBuf,
    aapt2: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
}

/// Versi bertitik (`36.1.0`) sebagai angka, supaya `10` lebih besar dari `9`.
fn versi(nama: &str) -> Vec<u64> {
    nama.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// Subdirektori `d` dengan versi tertinggi yang lolos `cocok`.
fn tertinggi(d: &Path, cocok: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut calon: Vec<PathBuf> = std::fs::read_dir(d).ok()?.flatten().map(|e| e.path()).filter(|p| cocok(p)).collect();
    calon.sort_by_key(|p| versi(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    calon.pop()
}

fn akar_sdk() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for k in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(p) = std::env::var_os(k).filter(|p| !p.is_empty()) { v.push(PathBuf::from(p)); }
    }
    if let Some(h) = std::env::var_os("HOME") { v.push(PathBuf::from(h).join("Android").join("Sdk")); }
    v.push(PathBuf::from("/usr/lib/android-sdk"));
    v.into_iter().filter(|p| p.is_dir()).collect()
}

/// `adb` dari SDK yang sama, atau dari PATH.
pub fn cari_adb() -> Result<PathBuf, String> {
    for a in akar_sdk() {
        let p = a.join("platform-tools").join("adb");
        if p.is_file() { return Ok(p); }
    }
    for d in std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default() {
        if d.join("adb").is_file() { return Ok(d.join("adb")); }
    }
    Err("`adb` tidak ditemukan\n  bantuan: pasang platform-tools Android (`sdkmanager \"platform-tools\"`)".into())
}

fn cari_alat() -> Result<Alat, String> {
    let akar = akar_sdk();
    if akar.is_empty() {
        return Err("SDK Android tidak ditemukan\n  bantuan: pasang Android SDK (mis. lewat Android Studio) lalu set \
                    ANDROID_HOME ke direktorinya".into());
    }
    let tag = if cfg!(target_os = "macos") { "darwin-x86_64" } else { "linux-x86_64" };
    let mut ndk: Vec<PathBuf> = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"].iter()
        .filter_map(|k| std::env::var_os(k).filter(|p| !p.is_empty()).map(PathBuf::from)).collect();
    for a in &akar {
        if let Some(n) = tertinggi(&a.join("ndk"), |p| p.join("toolchains").is_dir()) { ndk.push(n); }
        ndk.push(a.join("ndk-bundle"));
    }
    let ndk = ndk.into_iter().find(|n| n.join("toolchains/llvm/prebuilt").join(tag).join("bin").is_dir())
        .ok_or("NDK Android tidak ditemukan\n  bantuan: pasang lewat `sdkmanager \"ndk;26.3.11579264\"` \
                atau set ANDROID_NDK_HOME")?;
    let bt = akar.iter().filter_map(|a| tertinggi(&a.join("build-tools"), |p| p.join("aapt2").is_file() && p.join("apksigner").is_file()))
        .max_by_key(|p| versi(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()))
        .ok_or("build-tools Android (aapt2, apksigner) tidak ditemukan\n  bantuan: `sdkmanager \"build-tools;35.0.0\"`")?;
    let jar = akar.iter().filter_map(|a| tertinggi(&a.join("platforms"), |p| p.join("android.jar").is_file()))
        .max_by_key(|p| versi(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()))
        .ok_or("platform Android (android.jar) tidak ditemukan\n  bantuan: `sdkmanager \"platforms;android-34\"`")?;
    let d8 = Some(bt.join("d8")).filter(|p| p.is_file()).or_else(|| akar.iter()
        .filter_map(|a| tertinggi(&a.join("cmdline-tools"), |p| p.join("bin/d8").is_file())).map(|p| p.join("bin/d8")).next());
    Ok(Alat {
        d8,
        ndk_bin: ndk.join("toolchains/llvm/prebuilt").join(tag).join("bin"),
        glue: ndk.join("sources/android/native_app_glue"),
        aapt2: bt.join("aapt2"),
        apksigner: bt.join("apksigner"),
        android_jar: jar.join("android.jar"),
    })
}

fn jalankan(cmd: &mut Command, apa: &str, verbose: bool) -> Result<(), String> {
    if verbose { eprintln!("[jrgc] {:?}", cmd); }
    let out = crate::putar::jalankan(cmd, apa).map_err(|e| format!("{}: gagal menjalankan {:?}: {}", apa, cmd.get_program(), e))?;
    if !out.status.success() {
        return Err(format!("{} gagal:\n{}{}", apa, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

/// Nama paket dari nama berkas: `game_tangkap.jrg` -> `id.jrg.game_tangkap`.
pub fn nama_paket(stem: &str) -> String {
    let mut s: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    if !s.starts_with(|c: char| c.is_ascii_lowercase()) { s.insert(0, 'p'); }
    format!("id.jrg.{}", s)
}

/// Manifes tampilan `"web"`: aktivitas Java ber-WebView, program JRG sebagai proses `libjrgc.so`.
fn manifes_web(paket: &str, label: &str, orientasi: &str, ikon: bool, izin: &[String], app: &Aplikasi) -> String {
    let label: String = label.chars().filter(|c| !matches!(c, '<' | '>' | '&' | '"' | '\'')).collect();
    let ikon = if ikon { r#" android:icon="@mipmap/ikon""# } else { "" };
    let layanan = if izin.iter().any(|i| i == "android.permission.FOREGROUND_SERVICE") {
        r#"
        <service android:name="jrg.apk.Aktivitas$Layanan" android:exported="false" android:foregroundServiceType="specialUse">
            <property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="terminal dan pelayan pengembangan lokal"/>
        </service>"#
    } else { "" };
    let izin: String = izin.iter().map(|i| format!("    <uses-permission android:name=\"{}\"/>\n", i)).collect();
    let bersih = |t: &str| t.chars().filter(|c| !matches!(c, '<' | '>' | '&' | '"')).collect::<String>();
    let (port, entri) = (app.port.unwrap_or(8080), bersih(app.entri.as_deref().unwrap_or("src/main.jrg")));
    let (mesin, impor) = (bersih(app.mesin.as_deref().unwrap_or("interp")), bersih(&app.impor.join(",")));
    format!(r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{paket}">
{izin}    <application android:label="{label}"{ikon} android:hasCode="true" android:extractNativeLibs="true"
        android:usesCleartextTraffic="true">
        <meta-data android:name="jrg.port" android:value="{port}"/>
        <meta-data android:name="jrg.entri" android:value="{entri}"/>
        <meta-data android:name="jrg.mesin" android:value="{mesin}"/>
        <meta-data android:name="jrg.impor" android:value="{impor}"/>
        <activity android:name="jrg.apk.Aktivitas" android:exported="true"
            android:screenOrientation="{orientasi}" android:windowSoftInputMode="adjustResize"
            android:configChanges="orientation|screenSize|screenLayout|keyboardHidden|keyboard|uiMode"
            android:theme="@android:style/Theme.DeviceDefault.NoActionBar">
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>{layanan}
    </application>
</manifest>
"#)
}

/// `jrgc` SENDIRI untuk Android (`lib/<abi>/libjrgc.so`, sebuah executable): mesin tampilan `"web"`.
fn jrgc_android(abi: &Abi, alat: &Alat, verbose: bool) -> Result<PathBuf, String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hasil = dir.join("target").join(abi.rust).join("release").join("jrgc");
    fn terbaru(d: &Path, kini: &mut std::time::SystemTime) {
        let Ok(isi) = std::fs::read_dir(d) else { return };
        for e in isi.flatten() {
            let p = e.path();
            if p.is_dir() { terbaru(&p, kini); continue; }
            if let Ok(t) = e.metadata().and_then(|m| m.modified()) { if t > *kini { *kini = t; } }
        }
    }
    let perlu = match std::fs::metadata(&hasil).and_then(|m| m.modified()) {
        Err(_) => true,
        Ok(t) => {
            let mut kini = std::time::SystemTime::UNIX_EPOCH;
            for d in ["src", "stdlib", "editor"] { terbaru(&dir.join(d), &mut kini); }
            kini > t
        }
    };
    if perlu {
        if verbose { eprintln!("[jrgc] membangun jrgc untuk {} — beberapa menit, di-cache setelahnya...", abi.nama); }
        let linker = alat.ndk_bin.join(format!("{}{}-clang", abi.clang, API));
        let besar = abi.rust.to_uppercase().replace('-', "_");
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release", "-j", "1", "--bin", "jrgc", "--target", abi.rust]).current_dir(dir)
           .env(format!("CARGO_TARGET_{}_LINKER", besar), &linker)
           .env(format!("CARGO_TARGET_{}_RUSTFLAGS", besar), "-C link-arg=-Wl,-z,max-page-size=16384")
           .env(format!("CC_{}", abi.rust.replace('-', "_")), &linker)
           .env(format!("AR_{}", abi.rust.replace('-', "_")), alat.ndk_bin.join("llvm-ar"));
        jalankan(&mut cmd, &format!("membangun jrgc untuk {}", abi.nama), verbose).map_err(|e| saran_target(e, abi))?;
    }
    Ok(hasil)
}

/// `Aktivitas.java` -> `classes.dex`, lewat `javac` dan `d8` milik SDK (tanpa Gradle).
fn dex_aktivitas(kerja: &Path, alat: &Alat, verbose: bool) -> Result<Vec<u8>, String> {
    let d8 = alat.d8.as_ref().ok_or("tampilan `web` butuh `d8` (ada di build-tools Android)\n  \
        bantuan: `sdkmanager \"build-tools;35.0.0\"`")?;
    let sumber = kerja.join("java/jrg/apk");
    let (kelas, dex) = (kerja.join("kelas"), kerja.join("dex"));
    for d in [&sumber, &kelas, &dex] { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
    std::fs::write(sumber.join("Aktivitas.java"), include_str!("android/Aktivitas.java")).map_err(|e| e.to_string())?;
    jalankan(Command::new("javac").args(["-source", "11", "-target", "11", "-Xlint:-options", "-nowarn", "-cp"])
                 .arg(&alat.android_jar).arg("-d").arg(&kelas).arg(sumber.join("Aktivitas.java")),
             "mengompilasi aktivitas (javac — butuh JDK)", verbose)?;
    let mut berkas = Vec::new();
    kumpul(&kelas, &mut berkas);
    jalankan(Command::new(d8).args(["--min-api", &API.to_string(), "--lib"]).arg(&alat.android_jar)
                 .arg("--output").arg(&dex).args(&berkas), "mengubah kelas ke dex (d8)", verbose)?;
    std::fs::read(dex.join("classes.dex")).map_err(|e| format!("d8 tidak menghasilkan classes.dex: {}", e))
}

fn manifes(paket: &str, label: &str, orientasi: &str, ikon: bool, izin: &[String]) -> String {
    let label: String = label.chars().filter(|c| !matches!(c, '<' | '>' | '&' | '"' | '\'')).collect();
    let ikon = if ikon { r#" android:icon="@mipmap/ikon""# } else { "" };
    let izin: String = izin.iter().map(|i| format!("    <uses-permission android:name=\"{}\"/>\n", i)).collect();
    format!(r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{paket}">
{izin}    <application android:label="{label}"{ikon} android:hasCode="false" android:extractNativeLibs="false">
        <activity android:name="android.app.NativeActivity" android:exported="true"
            android:screenOrientation="{orientasi}"
            android:configChanges="orientation|screenSize|screenLayout|keyboardHidden|keyboard"
            android:theme="@android:style/Theme.NoTitleBar.Fullscreen">
            <meta-data android:name="android.app.lib_name" android:value="jrg"/>
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>
"#)
}

/// Bangun `libjrg.so` satu ABI; jawabannya daftar pustaka yang masuk `lib/<abi>/`.
fn bangun_abi(ir_path: &Path, kerja: &Path, abi: &Abi, alat: &Alat, opsi: &OpsiApk, verbose: bool) -> Result<Vec<PathBuf>, String> {
    let d = kerja.join(abi.nama);
    std::fs::create_dir_all(&d).map_err(|e| format!("gagal membuat {}: {}", d.display(), e))?;
    let sasaran = format!("--target={}{}", abi.clang, API);
    let ndk_clang = alat.ndk_bin.join("clang");
    let runtime = super::ensure_runtime(verbose, Some(abi.rust)).map_err(|e| saran_target(e, abi))?;
    // IR lewat clang milik `--emit-llvm` (yang paham IR versi ini); C dan penautan lewat NDK.
    let obj = d.join("program.o");
    let opt = format!("-O{}", opsi.opt_level.min(3));
    jalankan(Command::new(super::find_clang()?).arg(&sasaran).arg("-fPIC").arg(&opt).arg("-c").arg(ir_path).arg("-o").arg(&obj),
             "mengompilasi IR program", verbose)?;
    let c = d.join("pengantar.c");
    std::fs::write(&c, concat!(include_str!("primitif.c"), "\n", include_str!("korutin.c"), "\n",
                               include_str!("jaga_stack.c"), "\n", include_str!("android.c")))
        .map_err(|e| format!("gagal menulis {}: {}", c.display(), e))?;
    let mut objek = vec![obj];
    for (sumber, nama) in [(c, "pengantar.o"), (alat.glue.join("android_native_app_glue.c"), "glue.o")] {
        let o = d.join(nama);
        jalankan(Command::new(&ndk_clang).arg(&sasaran).arg("-fPIC").arg("-O2").arg("-w").arg("-I").arg(&alat.glue)
                     .arg("-c").arg(&sumber).arg("-o").arg(&o), "mengompilasi pengantar C", verbose)?;
        objek.push(o);
    }
    let so = d.join("libjrg.so");
    let mut cmd = Command::new(&ndk_clang);
    cmd.arg(&sasaran).arg("-shared").arg("-o").arg(&so).args(&objek).arg(&runtime);
    let mut pustaka = vec![so.clone()];
    if super::PAKAI_JEMBATAN.load(std::sync::atomic::Ordering::Relaxed) {
        let linker = alat.ndk_bin.join(format!("{}{}-clang", abi.clang, API));
        let lib = super::ensure_jembatan_untuk(verbose, Some((abi.rust, &linker, &alat.ndk_bin.join("llvm-ar"))))
            .map_err(|e| saran_target(e, abi))?;
        cmd.arg("-L").arg(lib.parent().unwrap_or(Path::new("."))).arg("-ljrgjembatan");
        pustaka.push(lib);
    }
    // 16 KB: halaman memori Android 15 ke atas; pustakanya dimuat langsung dari APK.
    cmd.args(["-landroid", "-llog", "-lm", "-ldl", "-u", "ANativeActivity_onCreate",
              "-Wl,--gc-sections", "-Wl,-z,max-page-size=16384", "-s"]);
    jalankan(&mut cmd, "menautkan libjrg.so", verbose)?;
    Ok(pustaka)
}

/// Target Rust yang belum terpasang disebut dengan perintah pemasangnya.
fn saran_target(e: String, abi: &Abi) -> String {
    if e.contains("target may not be installed") || e.contains("can't find crate for `core`") {
        format!("target Rust `{}` belum terpasang\n  bantuan: `rustup target add {}`", abi.rust, abi.rust)
    } else { e }
}

/// Zip tanpa kompresi; data `.so` rata 16 KB dan sisanya 4 byte, seperti hasil `zipalign -P 16`.
fn tulis_zip(tujuan: &Path, isi: &[(String, Vec<u8>)]) -> Result<(), String> {
    let mut out: Vec<u8> = Vec::new();
    let mut pusat: Vec<u8> = Vec::new();
    let u16le = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let u32le = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (nama, data) in isi {
        let rata = if nama.ends_with(".so") { 16384 } else { 4 };
        let mulai = out.len();
        let dasar = mulai + 30 + nama.len();
        let mut tambah = (rata - dasar % rata) % rata;
        if tambah != 0 && tambah < 6 { tambah += rata; }
        let crc = crate::canvas::png::crc32(data);
        u32le(&mut out, 0x04034b50); u16le(&mut out, 10); u16le(&mut out, 0); u16le(&mut out, 0);
        u16le(&mut out, 0); u16le(&mut out, 0x21);
        u32le(&mut out, crc); u32le(&mut out, data.len() as u32); u32le(&mut out, data.len() as u32);
        u16le(&mut out, nama.len() as u16); u16le(&mut out, tambah as u16);
        out.extend_from_slice(nama.as_bytes());
        if tambah != 0 {
            // Bidang ekstra 0xD935 milik zipalign: penanda perataan, sisanya nol.
            u16le(&mut out, 0xD935); u16le(&mut out, (tambah - 4) as u16); u16le(&mut out, rata as u16);
            out.resize(out.len() + tambah - 6, 0);
        }
        out.extend_from_slice(data);
        u32le(&mut pusat, 0x02014b50); u16le(&mut pusat, 20); u16le(&mut pusat, 10); u16le(&mut pusat, 0);
        u16le(&mut pusat, 0); u16le(&mut pusat, 0); u16le(&mut pusat, 0x21);
        u32le(&mut pusat, crc); u32le(&mut pusat, data.len() as u32); u32le(&mut pusat, data.len() as u32);
        u16le(&mut pusat, nama.len() as u16); u16le(&mut pusat, 0); u16le(&mut pusat, 0);
        u16le(&mut pusat, 0); u16le(&mut pusat, 0); u32le(&mut pusat, 0); u32le(&mut pusat, mulai as u32);
        pusat.extend_from_slice(nama.as_bytes());
    }
    let awal_pusat = out.len();
    out.extend_from_slice(&pusat);
    u32le(&mut out, 0x06054b50); u16le(&mut out, 0); u16le(&mut out, 0);
    u16le(&mut out, isi.len() as u16); u16le(&mut out, isi.len() as u16);
    u32le(&mut out, pusat.len() as u32); u32le(&mut out, awal_pusat as u32); u16le(&mut out, 0);
    std::fs::write(tujuan, out).map_err(|e| format!("gagal menulis {}: {}", tujuan.display(), e))
}

fn kumpul(d: &Path, keluar: &mut Vec<PathBuf>) {
    let Ok(isi) = std::fs::read_dir(d) else { return };
    for e in isi.flatten() {
        let p = e.path();
        if p.is_dir() { kumpul(&p, keluar) } else { keluar.push(p) }
    }
}

/// `e_machine` ELF untuk tiap ABI: pustaka arsitektur lain ditolak sebelum masuk APK.
fn mesin_elf(abi: &str) -> u16 { if abi == "arm64-v8a" { 183 } else { 62 } }

/// Pustaka native proyek untuk satu ABI: ada, bernama `lib*.so`, ELF 64 bit arsitektur itu.
fn pustaka_proyek(pola: &[String], abi: &Abi) -> Result<Vec<PathBuf>, String> {
    let mut keluar = Vec::new();
    for p in pola {
        let jalur = PathBuf::from(p.replace("{abi}", abi.nama));
        let nama = jalur.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !nama.starts_with("lib") || !nama.ends_with(".so") {
            return Err(format!("pustaka `{}`: Android cuma memasang berkas bernama `lib*.so`", jalur.display()));
        }
        let b = std::fs::read(&jalur).map_err(|e| format!("pustaka `{}` untuk {} tidak bisa dibaca: {}\n  \
            bantuan: tulis `{{abi}}` di jalurnya supaya tiap ABI punya berkasnya sendiri", jalur.display(), abi.nama, e))?;
        let elf64 = b.len() > 20 && b.starts_with(b"\x7fELF") && b[4] == 2;
        let mesin = if elf64 { u16::from_le_bytes([b[18], b[19]]) } else { 0 };
        if mesin != mesin_elf(abi.nama) {
            let nyata = match mesin { 183 => "arm64-v8a", 62 => "x86_64", 40 => "armeabi-v7a", 3 => "x86", 0 => "bukan ELF 64 bit", _ => "arsitektur lain" };
            return Err(format!("pustaka `{}` bukan untuk {} (isinya: {})\n  bantuan: kompilasi dengan clang NDK \
                                `--target={}{}`", jalur.display(), abi.nama, nyata, abi.clang, API));
        }
        keluar.push(jalur);
    }
    Ok(keluar)
}

/// Tanda tangan rilis: sandi dari `JRG_KUNCI_SANDI` (dan `JRG_KUNCI_SANDI_ALIAS` kalau beda),
/// atau ditanyakan apksigner langsung di terminal. Tidak pernah dari berkas.
fn tanda_tangan_rilis(alat: &Alat, app: &Aplikasi, mentah: &Path, tujuan: &Path, verbose: bool) -> Result<(), String> {
    let ks = app.kunci.as_ref().ok_or("`--rilis` butuh `kunci = \"rilis.jks\"` di [android]\n  \
        bantuan: buat kuncinya sekali dengan `jrgm kunci`")?;
    if !ks.is_file() {
        return Err(format!("kunci rilis `{}` tidak ada\n  bantuan: buat dengan `jrgm kunci`", ks.display()));
    }
    let alias = app.alias.clone().unwrap_or_else(|| "rilis".to_string());
    let mut cmd = Command::new(&alat.apksigner);
    cmd.arg("sign").arg("--ks").arg(ks).arg("--ks-key-alias").arg(&alias);
    if std::env::var_os("JRG_KUNCI_SANDI").is_some() {
        cmd.args(["--ks-pass", "env:JRG_KUNCI_SANDI"]);
        let k = if std::env::var_os("JRG_KUNCI_SANDI_ALIAS").is_some() { "env:JRG_KUNCI_SANDI_ALIAS" } else { "env:JRG_KUNCI_SANDI" };
        cmd.args(["--key-pass", k]);
        cmd.arg("--out").arg(tujuan).arg(mentah);
        return jalankan(&mut cmd, "menandatangani APK rilis (apksigner)", verbose);
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err("sandi kunci rilis tidak ada: set `JRG_KUNCI_SANDI`, atau jalankan dari terminal \
                    supaya apksigner menanyakannya".into());
    }
    cmd.arg("--out").arg(tujuan).arg(mentah);
    let st = cmd.status().map_err(|e| format!("gagal menjalankan apksigner: {}", e))?;
    if !st.success() { return Err("apksigner menolak menandatangani (sandi salah, atau alias tidak ada di kunci itu)".into()); }
    Ok(())
}

/// Ikon sebagai sumber daya `aapt2` (jawabannya berkas `.flat`): ikon lama di `mipmap-xxxhdpi`
/// (di `mipmap` polos ia dianggap mdpi, buram), plus `<adaptive-icon>` kalau ada `ikon_depan`/
/// `ikon_latar` — tanpanya peluncur Android 8+ mengecilkannya ke dalam lingkaran putih.
fn siapkan_ikon(app: &Aplikasi, kerja: &Path, alat: &Alat, verbose: bool) -> Result<Vec<PathBuf>, String> {
    let (sumber, flat) = (kerja.join("sumber"), kerja.join("flat"));
    let tulis = |rel: &str, isi: &[u8]| -> Result<PathBuf, String> {
        let p = sumber.join(rel);
        if let Some(d) = p.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
        std::fs::write(&p, isi).map_err(|e| e.to_string())?;
        Ok(p)
    };
    let baca = |p: &Path| std::fs::read(p).map_err(|e| format!("ikon `{}` tidak bisa dibaca: {}", p.display(), e));
    let mut mentah = Vec::new();
    if let Some(i) = app.ikon.as_ref().or(app.ikon_depan.as_ref()) { mentah.push(tulis("mipmap-xxxhdpi/ikon.png", &baca(i)?)?); }
    if let (Some(depan), Some(latar)) = (&app.ikon_depan, &app.ikon_latar) {
        mentah.push(tulis("mipmap-xxxhdpi/ikon_depan.png", &baca(depan)?)?);
        let rujukan = if latar.starts_with('#') {
            mentah.push(tulis("values/ikon.xml", format!(
                "<resources><color name=\"ikon_latar\">{}</color></resources>\n", latar).as_bytes())?);
            "@color/ikon_latar"
        } else {
            mentah.push(tulis("mipmap-xxxhdpi/ikon_latar.png", &baca(Path::new(latar))?)?);
            "@mipmap/ikon_latar"
        };
        mentah.push(tulis("mipmap-anydpi-v26/ikon.xml", format!(
            "<adaptive-icon xmlns:android=\"http://schemas.android.com/apk/res/android\">\n  \
             <background android:drawable=\"{}\"/>\n  <foreground android:drawable=\"@mipmap/ikon_depan\"/>\n\
             </adaptive-icon>\n", rujukan).as_bytes())?);
    }
    if mentah.is_empty() { return Ok(Vec::new()); }
    std::fs::create_dir_all(&flat).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&alat.aapt2);
    cmd.arg("compile").arg("-o").arg(&flat).args(&mentah);
    jalankan(&mut cmd, "menyiapkan ikon (aapt2 compile)", verbose)?;
    let mut hasil: Vec<PathBuf> = std::fs::read_dir(&flat).map_err(|e| e.to_string())?
        .flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "flat")).collect();
    hasil.sort();
    Ok(hasil)
}

/// Kompilasi sumber C/C++ dan crate Rust proyek untuk satu ABI; jawabannya pustaka `.so`-nya.
fn kompilasi_proyek(app: &Aplikasi, abi: &Abi, alat: &Alat, d: &Path, verbose: bool) -> Result<Vec<PathBuf>, String> {
    let mut keluar = Vec::new();
    let sasaran = format!("--target={}{}", abi.clang, API);
    let semua_c = app.pustaka_c.iter().map(|x| (false, x)).chain(app.program_c.iter().map(|x| (true, x)));
    for (program, (nama, sumber)) in semua_c {
        let berkas = nama_pustaka(nama)?;
        let bagian = if program { "program_c" } else { "pustaka_c" };
        if sumber.is_empty() { return Err(format!("`{}` di [android.{}] tidak punya berkas sumber", nama, bagian)); }
        let adalah_cpp = |f: &Path| matches!(f.extension().and_then(|e| e.to_str()), Some("cpp" | "cc" | "cxx"));
        let cpp = sumber.iter().any(|f| adalah_cpp(f));
        // Tiap berkas dengan kompilatornya SENDIRI: `clang++` atas `.c` memperlakukannya sebagai C++,
        // namanya di-mangle, dan `extern "C"` tidak menemukannya (terukur di Fase APK-5).
        let mut objek = Vec::new();
        for (i, f) in sumber.iter().enumerate() {
            if !f.is_file() { return Err(format!("sumber `{}` untuk `{}` tidak ada", f.display(), berkas)); }
            let o = d.join(format!("{}-{}.o", berkas, i));
            let mut cmd = Command::new(alat.ndk_bin.join(if adalah_cpp(f) { "clang++" } else { "clang" }));
            cmd.arg(&sasaran).args([if program { "-fPIE" } else { "-fPIC" }, "-O2", "-ffunction-sections", "-fdata-sections", "-c"]);
            if let Some(p) = f.parent() { cmd.arg("-I").arg(p); }
            cmd.arg(f).arg("-o").arg(&o);
            jalankan(&mut cmd, &format!("mengompilasi `{}` untuk {}", f.display(), abi.nama), verbose)?;
            objek.push(o);
        }
        let mut cmd = Command::new(alat.ndk_bin.join(if cpp { "clang++" } else { "clang" }));
        cmd.arg(&sasaran).args([if program { "-pie" } else { "-shared" }, "-Wl,-z,max-page-size=16384", "-Wl,--gc-sections", "-s"]).args(&objek);
        // libc++ ikut DI DALAM pustakanya: APK tidak membawa libc++_shared.so.
        if cpp { cmd.arg("-static-libstdc++"); }
        let out = d.join(&berkas);
        cmd.args(["-llog", "-lm"]);
        for t in &app.tautan { cmd.arg(format!("-l{}", t)); }
        cmd.arg("-o").arg(&out);
        jalankan(&mut cmd, &format!("menautkan `{}` untuk {}", berkas, abi.nama), verbose)?;
        keluar.push(out);
    }
    for (nama, dir) in &app.pustaka_rust {
        let berkas = nama_pustaka(nama)?;
        let manifes = dir.join("Cargo.toml");
        let isi = std::fs::read_to_string(&manifes)
            .map_err(|e| format!("crate Rust `{}` untuk `{}` tidak bisa dibaca: {}", manifes.display(), berkas, e))?;
        if !isi.contains("cdylib") {
            return Err(format!("crate `{}` harus pustaka dinamis\n  bantuan: tambahkan `[lib] crate-type = [\"cdylib\"]` \
                                di Cargo.toml-nya", dir.display()));
        }
        let crate_nama = nama_crate(&isi).ok_or_else(|| format!("`name` tidak ada di {}", manifes.display()))?;
        let target_dir = d.join(format!("rust-{}", berkas));
        let linker = alat.ndk_bin.join(format!("{}{}-clang", abi.clang, API));
        let besar = abi.rust.to_uppercase().replace('-', "_");
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release", "-j", "1", "--target", abi.rust, "--manifest-path"]).arg(&manifes)
           .arg("--target-dir").arg(&target_dir)
           .env(format!("CARGO_TARGET_{}_LINKER", besar), &linker)
           .env(format!("CARGO_TARGET_{}_RUSTFLAGS", besar), "-C link-arg=-Wl,-z,max-page-size=16384")
           .env(format!("CC_{}", abi.rust.replace('-', "_")), &linker)
           .env(format!("AR_{}", abi.rust.replace('-', "_")), alat.ndk_bin.join("llvm-ar"));
        jalankan(&mut cmd, &format!("mengompilasi crate Rust `{}` untuk {}", dir.display(), abi.nama), verbose)
            .map_err(|e| saran_target(e, abi))?;
        let hasil = target_dir.join(abi.rust).join("release").join(format!("lib{}.so", crate_nama.replace('-', "_")));
        let out = d.join(&berkas);
        std::fs::copy(&hasil, &out).map_err(|e| format!("hasil crate `{}` tidak ditemukan ({}): {}", dir.display(), hasil.display(), e))?;
        keluar.push(out);
    }
    for b in &app.bahasa {
        keluar.push(super::apk_bahasa::bangun(b, abi.nama, &sasaran, &alat.ndk_bin, verbose)?);
    }
    Ok(keluar)
}

/// Tolak nama bahasa yang tidak punya resep SEBELUM pembangunan yang panjang dimulai.
pub fn apk_bahasa_dikenal(id: &str) -> Result<(), String> { super::apk_bahasa::resep(id).map(|_| ()) }

/// `name` dari `[lib]` kalau ada, kalau tidak dari `[package]`.
fn nama_crate(cargo: &str) -> Option<String> {
    let mut bagian = "";
    let (mut lib, mut paket) = (None, None);
    for b in cargo.lines().map(str::trim) {
        if b.starts_with('[') { bagian = b; continue; }
        if let Some((k, v)) = b.split_once('=') {
            if k.trim() == "name" {
                let v = v.trim().trim_matches('"').to_string();
                if bagian == "[lib]" { lib = Some(v) } else if bagian == "[package]" { paket = Some(v) }
            }
        }
    }
    lib.or(paket)
}

/// `bundletool` (jar): `JRG_BUNDLETOOL`, lalu `~/.cache/jrgm/bundletool.jar`.
pub fn cari_bundletool() -> Result<PathBuf, String> {
    let mut calon = Vec::new();
    if let Some(p) = std::env::var_os("JRG_BUNDLETOOL").filter(|p| !p.is_empty()) { calon.push(PathBuf::from(p)); }
    if let Some(h) = std::env::var_os("HOME") { calon.push(PathBuf::from(h).join(".cache/jrgm/bundletool.jar")); }
    calon.into_iter().find(|p| p.is_file()).ok_or_else(|| "AAB butuh `bundletool` (jar resmi Google)\n  bantuan: unduh \
        bundletool-all-<versi>.jar dari github.com/google/bundletool/releases ke ~/.cache/jrgm/bundletool.jar, \
        atau set JRG_BUNDLETOOL ke jalurnya".to_string())
}

/// AAB ditandatangani `jarsigner` (apksigner tidak menangani bundel): kunci debug, atau kunci rilis.
fn tanda_tangan_aab(app: &Aplikasi, rilis: bool, aab: &Path, tujuan: &Path, verbose: bool) -> Result<(), String> {
    let mut cmd = Command::new("jarsigner");
    cmd.args(["-sigalg", "SHA256withRSA", "-digestalg", "SHA-256", "-signedjar"]).arg(tujuan);
    if !rilis {
        let ks = kunci_debug(verbose)?;
        cmd.arg("-keystore").arg(&ks).args(["-storepass", "android", "-keypass", "android"]).arg(aab).arg("androiddebugkey");
        return jalankan(&mut cmd, "menandatangani AAB (jarsigner)", verbose);
    }
    let ks = app.kunci.as_ref().ok_or("`--rilis` butuh `kunci = \"rilis.jks\"` di [android]\n  bantuan: `jrgm kunci`")?;
    if !ks.is_file() { return Err(format!("kunci rilis `{}` tidak ada\n  bantuan: buat dengan `jrgm kunci`", ks.display())); }
    let alias = app.alias.clone().unwrap_or_else(|| "rilis".to_string());
    cmd.arg("-keystore").arg(ks);
    if std::env::var_os("JRG_KUNCI_SANDI").is_some() {
        cmd.args(["-storepass:env", "JRG_KUNCI_SANDI"]);
        let k = if std::env::var_os("JRG_KUNCI_SANDI_ALIAS").is_some() { "JRG_KUNCI_SANDI_ALIAS" } else { "JRG_KUNCI_SANDI" };
        cmd.args(["-keypass:env", k]).arg(aab).arg(&alias);
        return jalankan(&mut cmd, "menandatangani AAB rilis (jarsigner)", verbose);
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err("sandi kunci rilis tidak ada: set `JRG_KUNCI_SANDI`, atau jalankan dari terminal".into());
    }
    cmd.arg(aab).arg(&alias);
    let st = cmd.status().map_err(|e| format!("gagal menjalankan jarsigner: {}", e))?;
    if !st.success() { return Err("jarsigner menolak menandatangani (sandi salah, atau alias tidak ada di kunci itu)".into()); }
    Ok(())
}

/// Kunci debug standar Android; dibuat kalau belum ada, dengan sandi bawaan `android`.
fn kunci_debug(verbose: bool) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME tidak diset, kunci debug tidak bisa dicari")?;
    let ks = home.join(".android").join("debug.keystore");
    if !ks.exists() {
        std::fs::create_dir_all(home.join(".android")).map_err(|e| e.to_string())?;
        jalankan(Command::new("keytool").args(["-genkeypair", "-keystore"]).arg(&ks)
                     .args(["-storepass", "android", "-alias", "androiddebugkey", "-keypass", "android",
                            "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000",
                            "-dname", "CN=Android Debug,O=Android,C=US"]),
                 "membuat kunci debug (keytool)", verbose)?;
    }
    Ok(ks)
}

/// `ir` ada: program dikompilasi LLVM jadi `libjrg.so` (kanvas). `ir` kosong: tampilan `"web"` —
/// sumber proyek ikut sebagai aset dan dijalankan `jrgc` Android di dalam APK.
pub fn bangun(ir: Option<&str>, sumber: &Path, tujuan: &Path, opsi: &OpsiApk, verbose: bool) -> Result<PathBuf, String> {
    let abi: Vec<&Abi> = opsi.abi.iter().map(|n| ABI.iter().find(|a| a.nama == n.as_str()).ok_or_else(|| format!(
        "ABI `{}` tidak dikenal\n  bantuan: yang ada `arm64-v8a` (HP) dan `x86_64` (emulator); beberapa dipisah koma", n)))
        .collect::<Result<_, _>>()?;
    let stem = sumber.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "jrg".into());
    let app = &opsi.aplikasi;
    let paket = app.paket.clone().unwrap_or_else(|| nama_paket(&stem));
    periksa_paket(&paket)?;
    let label = app.nama.clone().unwrap_or_else(|| stem.clone());
    let versi = app.versi.clone().unwrap_or_else(|| "0.1.0".to_string());
    let kode = match app.kode { Some(k) if k > 0 => k, Some(_) => return Err("kode versi harus lebih dari 0".into()),
                                None => kode_dari_versi(&versi)? };
    let orientasi = orientasi_android(app.orientasi.as_deref().unwrap_or("bebas"))?;
    let web = ir.is_none();
    // Pelayan loopback pun butuh izin soket: tanpa `internet` WebView tidak akan pernah tersambung.
    let mut diminta = app.izin.clone();
    if web && !diminta.iter().any(|i| i == "internet" || i.ends_with(".INTERNET")) { diminta.push("internet".into()); }
    let izin = izin_android(&diminta, web)?;
    let bundel = if opsi.aab { Some(cari_bundletool()?) } else { None };
    let latar_png = app.ikon_latar.as_deref().filter(|l| !l.starts_with('#')).map(PathBuf::from);
    for i in app.ikon.iter().chain(app.ikon_depan.iter()).chain(latar_png.iter()) {
        let b = std::fs::read(i).map_err(|e| format!("ikon `{}` tidak bisa dibaca: {}", i.display(), e))?;
        if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(format!("ikon `{}` bukan berkas PNG\n  bantuan: pakai PNG persegi, mis. 512x512", i.display()));
        }
    }
    if let Some(l) = app.ikon_latar.as_deref().filter(|l| l.starts_with('#')) {
        if !(l.len() == 7 || l.len() == 9) || !l[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("`ikon_latar = \"{}\"` bukan warna\n  bantuan: tulis `#RRGGBB`, atau jalur PNG", l));
        }
    }
    if app.ikon_depan.is_some() != app.ikon_latar.is_some() {
        return Err("ikon adaptif butuh `ikon_depan` DAN `ikon_latar` di [android]".into());
    }
    for t in &app.tautan {
        if t.is_empty() || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-') {
            return Err(format!("`tautan` memuat `{}`: tulis nama pustakanya saja, mis. `\"z\"` untuk libz", t));
        }
    }
    let target_sdk = app.target_sdk.unwrap_or(34);
    if target_sdk < API || target_sdk > 36 {
        return Err(format!("`target_sdk = {}` di luar {}..36 (minSdk aplikasi ini {})", target_sdk, API, API));
    }
    let ada_ikon = app.ikon.is_some() || app.ikon_depan.is_some();
    let alat = cari_alat()?;
    let kerja = std::env::temp_dir().join(format!("jrgc-apk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&kerja);
    std::fs::create_dir_all(&kerja).map_err(|e| format!("gagal membuat {}: {}", kerja.display(), e))?;
    let hasil = (|| {
        let ir_path = kerja.join("program.ll");
        std::fs::write(&ir_path, ir.unwrap_or("")).map_err(|e| e.to_string())?;
        let mut isi: Vec<(String, Vec<u8>)> = Vec::new();
        let m = kerja.join("AndroidManifest.xml");
        let teks_manifes = if web { manifes_web(&paket, &label, orientasi, ada_ikon, &izin, app) }
                           else { manifes(&paket, &label, orientasi, ada_ikon, &izin) };
        std::fs::write(&m, teks_manifes).map_err(|e| e.to_string())?;
        let res = kerja.join("res");
        std::fs::create_dir_all(&res).map_err(|e| format!("gagal membuat {}: {}", res.display(), e))?;
        let mut link = Command::new(&alat.aapt2);
        link.arg("link").arg("--manifest").arg(&m).arg("-I").arg(&alat.android_jar)
            .args(["--min-sdk-version", &API.to_string(), "--target-sdk-version", &target_sdk.to_string(),
                   "--version-code", &kode.to_string(), "--version-name", &versi, "--output-to-dir", "-o"])
            .arg(&res);
        // `-R` adalah overlay: tanpa `--auto-add-overlay` sumber daya BARU (warna latar) ditolak.
        let ikon = siapkan_ikon(app, &kerja, &alat, verbose)?;
        if !ikon.is_empty() { link.arg("--auto-add-overlay"); }
        for f in ikon { link.arg("-R").arg(f); }
        // AAB memakai manifes dan sumber daya dalam bentuk protobuf, di `manifest/` modulnya.
        if opsi.aab { link.arg("--proto-format"); }
        jalankan(&mut link, "menyusun manifes (aapt2)", verbose)?;
        let mut berkas = Vec::new();
        kumpul(&res, &mut berkas);
        berkas.sort();
        for p in berkas {
            let mut nama = p.strip_prefix(&res).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            if opsi.aab && nama == "AndroidManifest.xml" { nama = "manifest/AndroidManifest.xml".into(); }
            let b = std::fs::read(&p).map_err(|e| format!("gagal membaca {}: {}", p.display(), e))?;
            isi.push((nama, b));
        }
        if web {
            let dex = dex_aktivitas(&kerja, &alat, verbose)?;
            isi.push((if opsi.aab { "dex/classes.dex" } else { "classes.dex" }.to_string(), dex));
            for (nama, berkas) in &app.berkas_proyek {
                let b = std::fs::read(berkas).map_err(|e| format!("gagal membaca {}: {}", berkas.display(), e))?;
                isi.push((format!("assets/proyek/{}", nama), b));
            }
        }
        for a in &abi {
            let mut milik = if web {
                let d = kerja.join(a.nama);
                std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                // Android cuma memasang berkas `lib*.so`; executable pun harus bernama begitu.
                let tiruan = d.join("libjrgc.so");
                std::fs::copy(jrgc_android(a, &alat, verbose)?, &tiruan).map_err(|e| e.to_string())?;
                vec![tiruan]
            } else {
                bangun_abi(&ir_path, &kerja, a, &alat, opsi, verbose)?
            };
            milik.extend(pustaka_proyek(&app.pustaka, a)?);
            milik.extend(kompilasi_proyek(app, a, &alat, &kerja.join(a.nama), verbose)?);
            for lib in milik {
                let nama = lib.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let b = std::fs::read(&lib).map_err(|e| format!("gagal membaca {}: {}", lib.display(), e))?;
                if isi.iter().any(|(n, _)| *n == format!("lib/{}/{}", a.nama, nama)) {
                    return Err(format!("pustaka `{}` muncul dua kali untuk {}\n  bantuan: `libjrg.so` dan \
                                        `libjrgjembatan.so` milik JRG; pakai nama lain", nama, a.nama));
                }
                isi.push((format!("lib/{}/{}", a.nama, nama), b));
            }
        }
        if let Some(bt) = &bundel {
            let modul = kerja.join("base.zip");
            tulis_zip(&modul, &isi)?;
            let aab = kerja.join("belum.aab");
            let mut cmd = Command::new("java");
            cmd.arg("-jar").arg(bt).arg("build-bundle").arg(format!("--modules={}", modul.display()))
               .arg(format!("--output={}", aab.display()));
            jalankan(&mut cmd, "menyusun AAB (bundletool)", verbose)?;
            tanda_tangan_aab(app, opsi.rilis, &aab, tujuan, verbose)?;
            return Ok(tujuan.to_path_buf());
        }
        let mentah = kerja.join("belum.apk");
        tulis_zip(&mentah, &isi)?;
        if opsi.rilis {
            tanda_tangan_rilis(&alat, app, &mentah, tujuan, verbose)?;
        } else {
            let ks = kunci_debug(verbose)?;
            jalankan(Command::new(&alat.apksigner).arg("sign").arg("--ks").arg(&ks)
                         .args(["--ks-pass", "pass:android", "--key-pass", "pass:android", "--ks-key-alias", "androiddebugkey", "--out"])
                         .arg(tujuan).arg(&mentah), "menandatangani APK (apksigner)", verbose)?;
        }
        let _ = std::fs::remove_file(tujuan.with_extension("apk.idsig"));
        Ok(tujuan.to_path_buf())
    })();
    if !verbose { let _ = std::fs::remove_dir_all(&kerja); }
    hasil
}

#[cfg(test)]
mod tes {
    use super::*;

    fn izin(daftar: &[&str]) -> Vec<String> {
        izin_android(&daftar.iter().map(|s| s.to_string()).collect::<Vec<_>>(), true).unwrap()
    }

    /// `latar` membawa layanannya; `penyimpanan` membawa izin tulisnya (laporan IDE #10, #18).
    #[test]
    fn latar_dan_penyimpanan_membawa_izin_yang_dibutuhkan() {
        let i = izin(&["latar", "penyimpanan"]);
        for perlu in ["FOREGROUND_SERVICE", "FOREGROUND_SERVICE_SPECIAL_USE", "POST_NOTIFICATIONS", "WAKE_LOCK",
                      "READ_EXTERNAL_STORAGE", "WRITE_EXTERNAL_STORAGE"] {
            assert!(i.contains(&format!("android.permission.{}", perlu)), "{} tidak ada: {:?}", perlu, i);
        }
        assert!(izin_android(&["latar".to_string()], false).is_err(), "`latar` di tampilan kanvas harus ditolak");
        let m = manifes_web("id.jrg.x", "x", "portrait", true, &i, &Aplikasi::default());
        assert!(m.contains("jrg.apk.Aktivitas$Layanan") && m.contains("specialUse"), "layanan tidak ada di manifes:\n{}", m);
        let tanpa = manifes_web("id.jrg.x", "x", "portrait", true, &izin(&["internet"]), &Aplikasi::default());
        assert!(!tanpa.contains("<service"), "layanan muncul tanpa `latar`");
    }
}
