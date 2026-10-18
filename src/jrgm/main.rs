// src/jrgm/main.rs — jrgm, manajer proyek JawaRustGo++.

#![allow(dead_code, unused_variables, unused_imports)]

use std::path::{Path, PathBuf};
use std::process;
use clap::{Parser, Subcommand};

use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;
use jrgc::interpreter::interpret_with_paths;

mod manifes;
mod dependensi;
mod python;
mod cli;
mod jalankan;
mod rangka;
mod perintah;
mod android;
mod bahasa;
mod siapkan;
mod konsol;
mod lint;
mod git;

use cli::{Cli, JrgmCommand};
use perintah::*;
use rangka::cmd_new;

// ─── Main ─────────────────────────────────────────────────────────────────

fn main() {
    jrgc::term::pasang_pengait_panik();
    utama(std::env::args().collect());
}

/// Badan `jrgm`, juga dipakai `jrgc --jrgm ...` (APK dan Ubuntu di HP cuma membawa `jrgc`).
pub fn utama(argumen: Vec<String>) {
    let cli = Cli::parse_from(singkatan_bahasa(argumen));

    // Nada disetel sekali, sebelum apa pun sempat melapor — sama persis
    // dengan urutan di `jrgc`.
    if let Some(n) = &cli.nada {
        match jrgc::diag::Nada::dari_nama(n) {
            Some(nd) => jrgc::diag::set_nada(nd),
            None => {
                eprintln!(
                    "error: nada `{}` tidak ada. yang tersedia: {}",
                    n, jrgc::diag::Nada::SEMUA.join(", ")
                );
                process::exit(2);
            }
        }
    } else if cli.sopan {
        jrgc::diag::set_nada(jrgc::diag::Nada::Sopan);
    } else if cli.galak {
        jrgc::diag::set_nada(jrgc::diag::Nada::Galak);
    }

    let result = match cli.command {
        JrgmCommand::New    { name, kind, android, bahasa: Some(b) }
            if !b.trim().eq_ignore_ascii_case("jrg")
            => if android || kind != "bin" {
                Err("`--bahasa` tidak bisa digabung dengan `--kind`/`--android`: rangka bahasa lain cuma program biasa".into())
            } else { bahasa::resep(&b).and_then(|r| bahasa::rangka(&name, r)) },

        JrgmCommand::New    { name, kind, android, .. }
            => cmd_new(&name, if android { "android" } else { &kind }),

        JrgmCommand::Apk    { abi, output, rilis, aab, verbose }
            => android::cmd_apk(abi.as_deref(), output.as_deref(), rilis, aab, verbose),

        JrgmCommand::Kunci  => android::cmd_kunci(),

        JrgmCommand::Siapkan { apa, ya, target_sdk, sdk, tanpa_ndk } => if apa == "android" {
            siapkan::android(&siapkan::Opsi { ya, target_sdk, sdk, tanpa_ndk })
        } else { Err(format!("`jrgm siapkan {}` tidak ada; yang ada: android", apa)) },

        JrgmCommand::Run    { verbose, android: true, .. }
            => android::cmd_run_android(verbose),

        JrgmCommand::Build  { profile, target, verbose, skip_python_deps }
            => cmd_build(&profile, target.as_deref(), verbose, skip_python_deps),

        JrgmCommand::Run    { build_profile, args, verbose, prof, use_vm, emit_bc, watch, clear, skip_python_deps, .. }
            => cmd_run(&build_profile, &args, verbose, prof, use_vm, emit_bc, watch, clear, skip_python_deps),

        JrgmCommand::Exec   { file, args, verbose, prof, use_vm, emit_bc, watch, clear }
            => cmd_exec(&file, &args, verbose, prof, use_vm, emit_bc, watch, clear),

        JrgmCommand::Test   { filter, use_vm, timeout }
            => cmd_test(filter.as_deref(), use_vm, if timeout == 0 { u64::MAX } else { timeout }),

        JrgmCommand::Check
            => cmd_check(),

        JrgmCommand::Add    { name, version, git, rev, path, dev }
            => cmd_add(&name, version.as_deref(), git.as_deref(), rev.as_deref(), path.as_deref(), dev),

        JrgmCommand::Remove { name }
            => cmd_remove(&name),

        JrgmCommand::Update { name } => git::cmd_update(name.as_deref()),
        JrgmCommand::Tree    => cmd_tree(),
        JrgmCommand::Clean   => cmd_clean(),

        JrgmCommand::Fmt    { paths, check }
            => cmd_fmt(&paths, check),

        JrgmCommand::Lint { ketat } => lint::cmd_lint(ketat),
        JrgmCommand::Lsp     => { jrgc::lsp::run_server(); Ok(()) }

        JrgmCommand::Publish { dry_run }
            => git::cmd_publish(dry_run),
    };

    if let Err(e) = result {
        // String KOSONG berarti diagnostiknya sudah dicetak lengkap di tempat
        // kejadian (dengan sapaan, cuplikan sumber, dan caret).
        if !e.is_empty() {
            eprintln!("error: {}", e);
        }
        process::exit(1);
    }
}

/// `jrgm new app --c` = `jrgm new app --bahasa c` (juga `--python`, `--js`, `--cpp`, ...).
fn singkatan_bahasa(mut a: Vec<String>) -> Vec<String> {
    if a.get(1).map(String::as_str) != Some("new") { return a; }
    for i in 2..a.len() {
        let Some(nama) = a[i].strip_prefix("--") else { continue };
        if nama == "android" || nama == "kind" || nama == "bahasa" || nama == "lang" { continue; }
        if let Ok(r) = bahasa::resep(nama) {
            a[i] = r.nama.to_string();
            a.insert(i, "--bahasa".to_string());
            break;
        }
    }
    a
}

// ─── Tes unit ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tes {
    use super::*;
    use python::{paket_pip, nama_modul};

    /// Nama modul Python yang berbeda dari nama paket pip-nya harus dipetakan.
    #[test]
    fn nama_modul_dipetakan_ke_nama_paket_pip() {
        for (modul, paket) in [
            ("cv2", "opencv-python"),
            ("sklearn", "scikit-learn"),
            ("PIL", "Pillow"),
            ("yaml", "PyYAML"),
            ("bs4", "beautifulsoup4"),
        ] {
            assert_eq!(paket_pip(modul), paket, "pemetaan `{}` salah", modul);
        }
        // Nama yang TIDAK dipetakan dipakai apa adanya — itu yang benar untuk
        // sebagian besar paket, dan yang membuat tabel tak-lengkap tetap aman.
        for sama in ["numpy", "pandas", "requests", "scipy", "flask"] {
            assert_eq!(paket_pip(sama), sama, "`{}` seharusnya tidak dipetakan", sama);
        }
    }

    /// `[android]` dibaca utuh, komentar ujung baris dibuang, kunci asing dilaporkan.
    #[test]
    fn bagian_android_dibaca_dan_kunci_asing_dilaporkan() {
        let m = manifes::Manifest::parse_from_str("[package]\nname = \"gameku\"\nversion = \"1.2.3\"\n\
            [android]\nnama = \"Game # Ku\"   # komentar\npaket = \"id.saya.gameku\"\n\
            orientasi = \"landscape\"\nabi = [\"arm64-v8a\", \"x86_64\"]  # dua\nikon = \"aset/i.png\"\n",
            std::path::Path::new("/p")).unwrap();
        let o = android::opsi_dari_manifes(&m, None).unwrap();
        assert_eq!(o.aplikasi.nama.as_deref(), Some("Game # Ku"));
        assert_eq!(o.aplikasi.paket.as_deref(), Some("id.saya.gameku"));
        assert_eq!(o.aplikasi.versi.as_deref(), Some("1.2.3"), "versi bawaan dari [package]");
        assert_eq!(o.abi, vec!["arm64-v8a", "x86_64"]);
        assert_eq!(o.aplikasi.ikon.as_deref(), Some(std::path::Path::new("/p/aset/i.png")));
        assert_eq!(android::opsi_dari_manifes(&m, Some("x86_64")).unwrap().abi, vec!["x86_64"]);
        let n = manifes::Manifest::parse_from_str("[android]\npustaka = [\"native/{abi}/libh.so\"]\n\
            kunci = \"k/rilis.jks\"\nalias = \"saya\"\n", std::path::Path::new("/p")).unwrap();
        let o = android::opsi_dari_manifes(&n, None).unwrap();
        assert_eq!(o.aplikasi.pustaka, vec!["/p/native/{abi}/libh.so"], "`{{abi}}` dipertahankan, jalur relatif akar");
        assert_eq!(o.aplikasi.kunci.as_deref(), Some(std::path::Path::new("/p/k/rilis.jks")));
        assert_eq!(o.aplikasi.alias.as_deref(), Some("saya"));
        assert!(!o.rilis, "rilis cuma lewat `jrgm apk --rilis`");
        let c = manifes::Manifest::parse_from_str("[android]\nizin = [\"internet\"]\n[android.pustaka_c]\n\
            libhitung = [\"native/a.c\", \"native/b.cpp\"]\n[android.pustaka_rust]\nlibrumus = \"rust/rumus\"\n",
            std::path::Path::new("/p")).unwrap();
        let o = android::opsi_dari_manifes(&c, None).unwrap();
        assert_eq!(o.aplikasi.izin, vec!["internet"]);
        assert_eq!(o.aplikasi.pustaka_c, vec![("libhitung".to_string(),
            vec![std::path::PathBuf::from("/p/native/a.c"), std::path::PathBuf::from("/p/native/b.cpp")])]);
        assert_eq!(o.aplikasi.pustaka_rust, vec![("librumus".to_string(), std::path::PathBuf::from("/p/rust/rumus"))]);
        let asing = manifes::Manifest::parse_from_str("[android.pustaka_go]\nlibx = \"x\"\n", std::path::Path::new("/p")).unwrap();
        assert!(android::opsi_dari_manifes(&asing, None).unwrap_err().contains("[android.pustaka_go] libx"));
        assert_eq!(jrgc::llvm::apk::izin_android(&["internet".into(), "android.permission.VIBRATE".into()], false),
                   Ok(vec!["android.permission.INTERNET".to_string(), "android.permission.VIBRATE".to_string()]));
        assert!(jrgc::llvm::apk::izin_android(&["kamera".into()], false).unwrap_err().contains("dialog SAAT aplikasi jalan"));
        assert_eq!(jrgc::llvm::apk::izin_android(&["kamera".into(), "CAMERA".into()], true), Ok(vec!["android.permission.CAMERA".to_string()]));
        assert!(jrgc::llvm::apk::izin_android(&["terbang".into()], true).unwrap_err().contains("tidak dikenal"));
        // Mikrofon membawa izin normal yang WebView tuntut sebelum mau merekam.
        assert_eq!(jrgc::llvm::apk::izin_android(&["mikrofon".into()], true),
                   Ok(vec!["android.permission.RECORD_AUDIO".to_string(), "android.permission.MODIFY_AUDIO_SETTINGS".to_string()]));
        assert_eq!(jrgc::llvm::apk::nama_pustaka("libhitung"), Ok("libhitung.so".to_string()));
        assert!(jrgc::llvm::apk::nama_pustaka("hitung").is_err());
        let salah = manifes::Manifest::parse_from_str("[android]\nikom = \"a.png\"\n", std::path::Path::new("/p")).unwrap();
        let e = android::opsi_dari_manifes(&salah, None).unwrap_err();
        assert!(e.contains("`ikom`") && e.contains("`ikon`"), "{}", e);
    }

    /// Array yang dipecah ke beberapa baris dibaca utuh, dengan komentar dan koma penutup.
    #[test]
    fn array_banyak_baris_dibaca_utuh() {
        let m = manifes::Manifest::parse_from_str("[package]\nname = \"x\"\nbahasa = \"sh\"\n\
            [perintah]\njalan = [   # dua langkah\n  \"echo [satu]\",\n  \"echo \\\"dua]\\\"\",  # kedua\n]\n\
            uji = \"sh t.sh\"\n[android]\nabi = [\n  \"arm64-v8a\",\n  \"x86_64\",\n]\n",
            std::path::Path::new("/p")).unwrap();
        assert_eq!(m.perintah, vec![("jalan".to_string(), vec!["echo [satu]".to_string(), "echo \"dua]\"".to_string()]),
                                    ("uji".to_string(), vec!["sh t.sh".to_string()])]);
        assert_eq!(m.android.abi, vec!["arm64-v8a", "x86_64"]);
    }

    /// Kode versi naik bersama versinya; yang tidak bisa jadi angka ditolak, tidak ditebak.
    #[test]
    fn kode_versi_paket_dan_orientasi_android() {
        use jrgc::llvm::apk::{kode_dari_versi, periksa_paket, orientasi_android};
        assert_eq!(kode_dari_versi("0.1.0"), Ok(1000));
        assert_eq!(kode_dari_versi("1.2.3"), Ok(1002003));
        assert_eq!(kode_dari_versi("2"), Ok(2000000));
        assert!(kode_dari_versi("1.2.3") < kode_dari_versi("1.10.0"));
        for buruk in ["0.0.0", "1.x", "1.2.3.4", "1000.0.0", ""] {
            assert!(kode_dari_versi(buruk).is_err(), "`{}` harusnya ditolak", buruk);
        }
        assert!(periksa_paket("id.jrg.gameku").is_ok());
        for buruk in ["gameku", "id.jrg.Game", "id..x", "id.1abc", "id.jrg-x"] {
            assert!(periksa_paket(buruk).is_err(), "`{}` harusnya ditolak", buruk);
        }
        assert_eq!(orientasi_android("tegak"), Ok("sensorPortrait"));
        assert!(orientasi_android("miring").is_err());
    }

    #[test]
    fn singkatan_bahasa_di_jrgm_new() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(singkatan_bahasa(v(&["jrgm", "new", "app", "--c"])), v(&["jrgm", "new", "app", "--bahasa", "c"]));
        assert_eq!(singkatan_bahasa(v(&["jrgm", "new", "--py", "app"])), v(&["jrgm", "new", "--bahasa", "python", "app"]));
        assert_eq!(singkatan_bahasa(v(&["jrgm", "new", "app", "--android"])), v(&["jrgm", "new", "app", "--android"]));
        assert_eq!(singkatan_bahasa(v(&["jrgm", "run", "--c"])), v(&["jrgm", "run", "--c"]));
    }

    /// Batasan versi dibuang saat memeriksa importability.
    #[test]
    fn batasan_versi_dibuang_untuk_cek_import() {
        for (entri, modul) in [
            ("numpy>=2.0", "numpy"),
            ("pandas==1.5.3", "pandas"),
            ("requests!=2.0", "requests"),
            ("scipy<2", "scipy"),
            ("uvicorn[standard]", "uvicorn"),
            ("flask ~= 2.0", "flask"),
            ("numpy", "numpy"),
        ] {
            assert_eq!(nama_modul(entri), modul, "gagal memisahkan versi dari `{}`", entri);
        }
    }
}
