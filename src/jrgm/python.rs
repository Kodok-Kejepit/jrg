// src/jrgm/python.rs — pemasangan otomatis paket Python

use super::dependensi::kumpul_jrg;
use super::manifes::Manifest;

// ─── Python dependency auto-install
// ────────────────────────────────────────

/// Scan token stream (bukan teks mentah — supaya comment/string yang
/// kebetulan mirip pola ini tidak ikut ke-deteksi) mencari `py::import("X")` …
pub(crate) fn scan_python_deps_from_tokens(tokens: &[jrgc::lexer::Token]) -> Vec<String> {
    use jrgc::lexer::TokenKind;
    let mut found = std::collections::BTreeSet::new();
    let mut i = 0;
    while i + 4 < tokens.len() {
        let is_py     = matches!(&tokens[i].kind, TokenKind::Ident(s) if s == "py");
        let is_colon2 = matches!(&tokens[i + 1].kind, TokenKind::ColonColon);
        // `import` adalah keyword JRG (dipakai untuk `import "std/fmt"`), jadi lexer
        // men-tokenize-nya sebagai `KwImport`, BUKAN `Ident("import")` — persis …
        let is_fn = matches!(&tokens[i + 2].kind, TokenKind::KwImport)
            || matches!(&tokens[i + 2].kind, TokenKind::Ident(s) if s == "call" || s == "call_kw");
        let is_lparen = matches!(&tokens[i + 3].kind, TokenKind::LParen);
        if is_py && is_colon2 && is_fn && is_lparen {
            if let TokenKind::StringLit(module) = &tokens[i + 4].kind {
                found.insert(module.clone());
            }
        }
        i += 1;
    }
    found.into_iter().collect()
}

/// Gabungkan daftar eksplisit di manifest dengan hasil scan otomatis atas
/// entry file, lalu pastikan semuanya terpasang (lihat …
pub(crate) fn resolve_and_ensure_python_deps(manifest: &Manifest, verbose: bool, skip: bool) -> Result<(), String> {
    if skip {
        return Ok(());
    }

    let mut packages: std::collections::BTreeSet<String> =
        manifest.python_packages.iter().cloned().collect();

    // SELURUH berkas `.jrg` di proyek dipindai, bukan cuma entry point.
    let akar = manifest.entry.parent().map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut berkas = Vec::new();
    kumpul_jrg(&akar, &mut berkas);
    if !berkas.iter().any(|p| p == &manifest.entry) {
        berkas.push(manifest.entry.clone());
    }
    for p in &berkas {
        let Ok(src) = std::fs::read_to_string(p) else { continue };
        let sf = jrgc::lexer::SourceFile { id: 0, name: p.display().to_string(), src };
        if let Ok(tokens) = jrgc::lexer::lex(&sf) {
            for pkg in scan_python_deps_from_tokens(&tokens) {
                packages.insert(pkg);
            }
        }
        // Lex gagal di sini bukan error fatal untuk langkah ini — biarkan
        // pipeline utama (`run_jrg_file`/`check_jrg_file`) yang melaporkan
        // error syntax-nya dengan pesan yang lebih baik.
    }

    if packages.is_empty() {
        return Ok(());
    }

    let packages: Vec<String> = packages.into_iter().collect();
    if verbose {
        eprintln!("[jrgm] python packages terdeteksi: {}", packages.join(", "));
    }
    ensure_python_packages(&packages, verbose)
}

/// Nama modul Python yang BERBEDA dari nama paket pip-nya.
pub(crate) const MODUL_KE_PAKET: &[(&str, &str)] = &[
    ("cv2",       "opencv-python"),
    ("sklearn",   "scikit-learn"),
    ("PIL",       "Pillow"),
    ("yaml",      "PyYAML"),
    ("bs4",       "beautifulsoup4"),
    ("dateutil",  "python-dateutil"),
    ("serial",    "pyserial"),
    ("OpenGL",    "PyOpenGL"),
    ("Crypto",    "pycryptodome"),
    ("fitz",      "PyMuPDF"),
    ("docx",      "python-docx"),
    ("pptx",      "python-pptx"),
    ("git",       "GitPython"),
    ("attr",      "attrs"),
    ("jwt",       "PyJWT"),
    ("psycopg2",  "psycopg2-binary"),
    ("MySQLdb",   "mysqlclient"),
    ("Xlib",      "python-xlib"),
    ("usb",       "pyusb"),
    ("magic",     "python-magic"),
    ("lxml",      "lxml"),
    ("skimage",   "scikit-image"),
    ("Bio",       "biopython"),
    ("google",    "protobuf"),
];

/// Nama paket pip untuk sebuah nama modul.
pub(crate) fn paket_pip(modul: &str) -> &str {
    MODUL_KE_PAKET
        .iter()
        .find(|(m, _)| *m == modul)
        .map(|(_, p)| *p)
        .unwrap_or(modul)
}

/// Nama modul yang bisa di-`import`, dari sebuah entri manifest.
pub(crate) fn nama_modul(entri: &str) -> &str {
    entri
        .split(|c| c == '=' || c == '>' || c == '<' || c == '!' || c == '~' || c == '[')
        .next()
        .unwrap_or(entri)
        .trim()
}

pub(crate) fn pip_install(pkg: &str, extra_args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new("python3")
        .arg("-m").arg("pip").arg("install")
        .args(extra_args)
        .arg(pkg)
        .output()
}

/// Pastikan tiap nama di `packages` bisa di-`import` oleh `python3` — kalau
/// belum, pasang otomatis lewat `python3 -m pip install <nama>` (dipanggil …
pub(crate) fn ensure_python_packages(packages: &[String], verbose: bool) -> Result<(), String> {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return Err(
            "jrgm: `python3` tidak ditemukan di PATH — pasang dulu (Termux: `pkg install python`; \
             Debian/Kali: `apt install python3`) sebelum menjalankan project yang pakai py::"
                .to_string(),
        );
    }

    for pkg in packages {
        // Dipisah: yang DI-IMPORT adalah nama modul, yang DIPASANG adalah nama paket
        // pip.
        let modul = nama_modul(pkg);
        let paket = paket_pip(modul);
        let importable = std::process::Command::new("python3")
            .args(["-c", &format!("import {}", modul)])
            .output();
        if matches!(&importable, Ok(o) if o.status.success()) {
            if verbose { eprintln!("[jrgm] python: `{}` sudah terpasang", modul); }
            continue;
        }

        if paket == modul {
            eprintln!("[jrgm] python: memasang `{}`...", paket);
        } else {
            // Perbedaannya disebutkan, supaya yang membaca log tidak bingung
            // kenapa `py::import("cv2")` memasang sesuatu bernama lain.
            eprintln!("[jrgm] python: memasang `{}` (untuk modul `{}`)...", paket, modul);
        }
        // Entri manifest dipakai UTUH kalau ia memuat batasan versi
        // (`numpy>=2.0`); pemetaan nama cuma berlaku untuk nama telanjang.
        let sasaran: &str = if pkg == modul { paket } else { pkg };
        let first = pip_install(sasaran, &[]);

        let result: Result<(), String> = match &first {
            Ok(o) if o.status.success() => Ok(()),
            // PEP 668 "externally managed environment" — umum di Debian/Ubuntu/Kali;
            // Termux biasanya tidak mengalami ini sama sekali, jadi percobaan pertama di …
            Ok(o) if String::from_utf8_lossy(&o.stderr).contains("externally-managed-environment") => {
                if verbose {
                    eprintln!("[jrgm] python: environment externally-managed, coba ulang dengan --break-system-packages");
                }
                match pip_install(sasaran, &["--break-system-packages"]) {
                    Ok(o2) if o2.status.success() => Ok(()),
                    Ok(o2) => Err(last_meaningful_line(&o2.stderr)),
                    Err(e) => Err(format!("gagal menjalankan pip: {}", jrgc::host::galat_io(&e))),
                }
            }
            Ok(o) => Err(last_meaningful_line(&o.stderr)),
            Err(e) => Err(format!("gagal menjalankan pip: {}", jrgc::host::galat_io(&e))),
        };

        match result {
            Ok(()) => eprintln!("[jrgm] python: `{}` terpasang", pkg),
            Err(reason) => {
                return Err(format!("jrgm: gagal memasang python package `{}`: {}", pkg, reason));
            }
        }
    }
    Ok(())
}

pub(crate) fn last_meaningful_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap_or("(tidak ada detail dari pip)")
        .trim()
        .to_string()
}
