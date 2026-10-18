// src/jrgm/cli.rs — bentuk baris perintah
// Dipecah dari `main.rs`, satu urusan per berkas.

use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name    = "jrgm",
    version,
    about   = "JawaRustGo++ Package Manager",
    long_about = "jrgm — manajer proyek untuk JawaRustGo++.",
    after_help = "\
Contoh:
  jrgm new proyekku          bikin proyek baru (--kind lib | web)
  jrgm new app --c           proyek bahasa lain (--python --js --cpp --rust --go --java --php --lua --ruby --sh, atau --bahasa X)
  jrgm run                   jalankan (entry: src/main.jrg)
  jrgm exec halo.c           jalankan satu berkas bahasa apa pun yang dikenal
  jrgm run --vm              lewat VM
  jrgm run -w                jalankan ulang tiap berkas berubah
  jrgm test                  jalankan tests/*.jrg
  jrgm add bantu --path ../bantu    dependensi lokal
  jrgm add bantu --git <url> --version 0.1.0   dependensi git (dikunci di jrgm.lock)
  jrgm lint                  periksa tanpa menjalankan (galat + peringatan)
  jrgm publish               tag v<versi> lalu dorong ke repositori git
  jrgm new gameku --android  proyek aplikasi Android
  jrgm apk                   bungkus jadi target/<nama>.apk
  jrgm siapkan android       pasang SDK/NDK/build-tools (ditawarkan otomatis oleh jrgm apk)
  jrgm run --android         pasang ke HP, buka, tampilkan keluarannya
  jrgm kunci / apk --rilis   kunci rilis sekali, lalu APK siap terbit
  jrgm apk --rilis --aab     bundel untuk Play Store

Penjelasan lengkap: https://jawarustgoplusplus.web.id/web_dokumentasi/jrgm.html",
    after_long_help = "\
Contoh:
  jrgm new proyekku          bikin proyek baru (--kind lib | web)
  jrgm run                   jalankan (entry: src/main.jrg)
  jrgm test                  jalankan tests/*.jrg
  jrgm add bantu --path ../bantu    dependensi lokal
  jrgm lint                  periksa tanpa menjalankan

Penjelasan lengkap: https://jawarustgoplusplus.web.id/web_dokumentasi/jrgm.html"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: JrgmCommand,

    /// Nada diagnostik SOPAN — buang baris sapaan di atas tiap error.
    #[arg(long = "sopan", global = true, conflicts_with_all = ["galak", "nada"])]
    pub(crate) sopan: bool,

    /// Pilih nada diagnostik: sopan, galak, judes, hangat, cool, lawak.
    #[arg(long = "nada", global = true, value_name = "NAMA")]
    pub(crate) nada: Option<String>,

    /// Nada GALAK — ini sudah bawaan, jadi flag ini cuma dipakai kalau
    /// `JRG_SOPAN` di lingkungan sedang mematikannya.
    #[arg(long = "galak", global = true)]
    pub(crate) galak: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum JrgmCommand {
    /// Buat project JawaRustGo++ baru
    New {
        name: String,
        #[arg(long, default_value = "bin")]
        kind: String,
        /// Sama dengan `--kind android`: proyek aplikasi Android (kanvas + sentuhan)
        #[arg(long)]
        android: bool,
        /// Proyek bahasa lain: python, javascript, c, cpp, rust, go, java, php, lua, ruby, sh
        #[arg(long, visible_alias = "lang")]
        bahasa: Option<String>,
    },

    /// Bungkus proyek jadi APK Android (identitasnya dari `[android]` di jrgm.toml)
    Apk {
        /// ABI, dipisah koma: arm64-v8a (HP), x86_64 (emulator)
        #[arg(long)]
        abi: Option<String>,
        /// Berkas hasil (bawaan: target/<nama>.apk, atau target/<nama>-rilis.apk)
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        /// Tanda tangani dengan kunci rilis dari `[android] kunci` (buat dulu dengan `jrgm kunci`)
        #[arg(long)]
        rilis: bool,
        /// Android App Bundle (`.aab`) untuk Play Store, bukan APK (butuh bundletool)
        #[arg(long)]
        aab: bool,
        #[arg(short = 'v', long)]
        verbose: bool,
    },

    /// Buat kunci rilis APK sekali (keytool menanyakan nama dan sandinya)
    Kunci,

    /// Pasang perkakas: `jrgm siapkan android` (platform, build-tools, NDK dari katalog Google, target Rust)
    Siapkan {
        /// Yang disiapkan: android
        apa: String,
        /// Jangan bertanya: setujui lisensi dan pakai pilihan bawaan
        #[arg(long, short = 'y')]
        ya: bool,
        /// Target SDK (bawaan: ditanyakan; tanpa terminal: yang terbaru)
        #[arg(long = "target-sdk")]
        target_sdk: Option<u32>,
        /// Folder SDK (bawaan: $ANDROID_HOME, lalu ~/Android/Sdk)
        #[arg(long)]
        sdk: Option<PathBuf>,
        /// Tanpa NDK (±1 GB); `jrgm apk` tetap butuh NDK, jadi pakai kalau NDK-nya dipasang sendiri
        #[arg(long = "tanpa-ndk")]
        tanpa_ndk: bool,
    },

    /// Build project (check syntax)
    Build {
        #[arg(long, default_value = "dev")]
        profile: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Lewati auto-install python packages (buat lingkungan offline/CI
        /// yang sudah menyiapkan dependency-nya sendiri)
        #[arg(long = "skip-python-deps")]
        skip_python_deps: bool,
    },

    /// Jalankan project via Tree-Walking Interpreter
    Run {
        #[arg(long = "build-profile", default_value = "dev")]
        build_profile: String,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Aktifkan built-in profiler
        #[arg(long = "profile")]
        prof: bool,
        /// Gunakan Bytecode VM
        #[arg(long = "vm")]
        use_vm: bool,
        /// Emit bytecode disassembly (butuh --vm)
        #[arg(long = "emit-bc")]
        emit_bc: bool,
        /// Hot reload: watch perubahan file dan re-run otomatis
        #[arg(long = "watch", short = 'w')]
        watch: bool,
        /// Bersihkan terminal sebelum setiap reload (pakai dengan --watch)
        #[arg(long = "clear")]
        clear: bool,
        /// Lewati auto-install python packages (buat lingkungan offline/CI
        /// yang sudah menyiapkan dependency-nya sendiri)
        #[arg(long = "skip-python-deps")]
        skip_python_deps: bool,
        /// Bangun APK, pasang ke HP/emulator di adb, buka, dan tampilkan keluarannya
        #[arg(long)]
        android: bool,
    },

    /// Jalankan satu berkas langsung (.jrg, atau .c .cpp .py .js .rs .go .java .php .lua .rb .sh)
    Exec {
        file: std::path::PathBuf,
        /// Argumen untuk programnya (sesudah `--`)
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Aktifkan built-in profiler
        #[arg(long = "profile")]
        prof: bool,
        /// Gunakan Bytecode VM
        #[arg(long = "vm")]
        use_vm: bool,
        /// Emit bytecode disassembly (butuh --vm)
        #[arg(long = "emit-bc")]
        emit_bc: bool,
        /// Hot reload: watch perubahan file dan re-run otomatis
        #[arg(long = "watch", short = 'w')]
        watch: bool,
        /// Bersihkan terminal sebelum setiap reload
        #[arg(long = "clear")]
        clear: bool,
    },

    /// Jalankan test di tests/*.jrg
    Test {
        /// Cuma jalankan test yang namanya memuat teks ini
        #[arg(long)]
        filter: Option<String>,
        /// Jalankan lewat bytecode VM
        #[arg(long = "vm")]
        use_vm: bool,
        /// Batas waktu per berkas, dalam detik (0 = tanpa batas)
        #[arg(long = "timeout", default_value_t = 60)]
        timeout: u64,
    },

    /// Check syntax tanpa menjalankan
    Check,

    /// Tambah dependency ke jrgm.toml (--path direktori, atau --git url)
    Add {
        name: String,
        /// Versi yang diterbitkan `jrgm publish` (tag `v<versi>`); butuh --git
        #[arg(long)]
        version: Option<String>,
        /// Repositori git paketnya (dikunci di jrgm.lock)
        #[arg(long)]
        git: Option<String>,
        /// Cabang, tag, atau commit; butuh --git
        #[arg(long)]
        rev: Option<String>,
        /// Direktori paketnya, relatif terhadap jrgm.toml ini
        #[arg(long)]
        path: Option<PathBuf>,
        /// Masuk [dev-dependencies]: cuma untuk jrgm test dan lint
        #[arg(long)]
        dev: bool,
    },

    /// Hapus dependency dari jrgm.toml
    Remove { name: String },

    /// Majukan dependency git ke commit terbaru (jrgm.lock)
    Update { name: Option<String> },

    /// Tampilkan dependency tree
    Tree,

    /// Bersihkan build artifacts
    Clean,

    /// Format source files
    Fmt {
        /// Berkas atau direktori yang mau dirapikan. Tanpa ini: `src/` kalau
        /// ada, kalau tidak seluruh direktori kerja.
        paths: Vec<std::path::PathBuf>,
        /// Jangan tulis apa pun — cuma laporkan berkas yang belum rapi, dan
        /// keluar dengan kode 1 kalau ada. Untuk CI.
        #[arg(long)]
        check: bool,
    },

    /// Periksa sumber tanpa menjalankan: galat + peringatan
    Lint {
        /// Peringatan juga membuat kode keluar 1
        #[arg(long)]
        ketat: bool,
    },

    /// Jalankan Language Server Protocol server (untuk VS Code / Neovim)
    Lsp,

    /// Terbitkan versi: tag v<versi> didorong ke repositori git
    Publish {
        #[arg(long)]
        dry_run: bool,
    },
}
