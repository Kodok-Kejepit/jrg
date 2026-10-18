// src/jrgm/perintah.rs — tiap subperintah `jrgm`

use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;
use super::rangka::cmd_new;
use super::dependensi::kumpul_jrg;
use std::path::{Path, PathBuf};
use std::process;

use super::manifes::Manifest;
use super::dependensi::{graf_dependensi, path_dependensi, tambah_path_dependensi};
use super::python::resolve_and_ensure_python_deps;
use super::jalankan::{run_jrg_file, check_jrg_file};
use super::rangka::rangka_web;

pub(crate) fn cmd_build(profile: &str, target: Option<&str>, verbose: bool, skip_python_deps: bool) -> Result<(), String> {
    if let Some((m, r, akar)) = super::bahasa::proyek_kini()? { return super::bahasa::bangun(&m, r, &akar); }
    let cwd = std::env::current_dir()
        .map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;
    let manifest = Manifest::load(&cwd)?;
    if verbose {
        eprintln!("[jrgm] build: {} v{} (profile: {}, target: {})",
            manifest.name, manifest.version,
            profile, target.unwrap_or("host"));
    }
    resolve_and_ensure_python_deps(&manifest, verbose, skip_python_deps)?;

    // Graf dependensi diperiksa di sini, bukan cuma saat dijalankan.
    let mut jalur = vec![cwd.join("src"), cwd.join("tests"), cwd.clone()];
    if cwd.join("jrgm.toml").is_file() {
        let graf = graf_dependensi(&cwd)?;
        jalur.extend(path_dependensi(&graf)?);
    }

    // Dan SETIAP berkas yang bisa dijangkau, bukan cuma entry point.
    let mut berkas: Vec<std::path::PathBuf> = Vec::new();
    for d in &jalur {
        let Ok(baca) = std::fs::read_dir(d) else { continue };
        for e in baca.flatten() {
            let f = e.path();
            if f.extension().and_then(|x| x.to_str()) == Some("jrg") && !berkas.contains(&f) {
                berkas.push(f);
            }
        }
    }
    berkas.sort();
    if !berkas.contains(&manifest.entry) { berkas.push(manifest.entry.clone()); }

    // Galat parse dicetak lengkap di tempat (pesannya kosong di sini), jadi
    // "ada yang gagal" dicatat terpisah dari daftar pesannya.
    let mut ada_gagal = false;
    let mut gagal = Vec::new();
    for f in &berkas {
        if let Err(e) = check_jrg_file(f, verbose) {
            ada_gagal = true;
            if !e.is_empty() { gagal.push(e); }
        }
    }

    // ── `import` yang tidak menunjuk apa pun ─────────────────────────────
    if !ada_gagal {
        let resolver = jrgc::module::ModuleResolver::new(jalur.clone());
        for f in &berkas {
            let Ok(src) = std::fs::read_to_string(f) else { continue };
            let sf = SourceFile { id: 0, name: f.display().to_string(), src };
            let Ok(tokens) = lex(&sf) else { continue };
            let (ast, _) = parse(tokens, true);
            for imp in &ast.imports {
                if jrgc::module::ModuleResolver::is_stdlib(&imp.path) { continue; }
                if resolver.resolve(&imp.path).is_some() { continue; }
                gagal.push(format!(
                    "{}: `import \"{}\"` tidak menunjuk modul mana pun\n  \
                     bantuan: dicari sebagai `{}.jrg` (atau `{}/mod.jrg`) di: {}",
                    f.display(), imp.path,
                    jrgc::module::ModuleResolver::module_name(&imp.path),
                    jrgc::module::ModuleResolver::module_name(&imp.path),
                    jalur.iter().map(|d| d.display().to_string())
                        .collect::<Vec<_>>().join(", ")));
            }
        }
    }
    if ada_gagal || !gagal.is_empty() {
        // Semua yang gagal, bukan yang pertama: kalau dua berkas rusak, berhenti
        // di yang pertama berarti orang memperbaikinya, menjalankan lagi, dan
        // menemukan yang kedua — satu perjalanan bolak-balik per berkas.
        return Err(gagal.join("\n"));
    }
    eprintln!("[jrgm] {} berkas diperiksa", berkas.len());
    eprintln!("[jrgm] build selesai (mode: interpreter — tidak ada binary output)");
    Ok(())
}

pub(crate) fn cmd_run(profile: &str, args: &[String], verbose: bool, prof: bool,
           use_vm: bool, emit_bc: bool, watch: bool, clear: bool, skip_python_deps: bool) -> Result<(), String> {
    if let Some((m, r, akar)) = super::bahasa::proyek_kini()? {
        if use_vm || prof || emit_bc {
            return Err(format!("`--vm`, `--profile` dan `--emit-bc` cuma untuk proyek JRG; ini proyek {}", r.nama));
        }
        if watch { return super::bahasa::awasi(&m, r, &akar, args, clear); }
        let kode = super::bahasa::jalan(&m, r, &akar, args)?;
        if kode != 0 { process::exit(kode); }
        return Ok(());
    }
    let cwd = std::env::current_dir().map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;
    let manifest = Manifest::load(&cwd)?;

    if verbose {
        eprintln!("[jrgm] run: {} v{} [{}{}]",
            manifest.name, manifest.version,
            if use_vm { "VM" } else { "interpreter" },
            if watch { "+watch" } else { "" });
    }

    resolve_and_ensure_python_deps(&manifest, verbose, skip_python_deps)?;

    if watch {
        // Hot reload mode
        let watch_dir = manifest.entry.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(cwd.clone());
        let mut config = jrgc::hotreload::ReloadConfig::new(manifest.entry);
        config.use_vm       = use_vm;
        config.profiling    = prof;
        config.emit_bc      = emit_bc;
        config.clear_screen = clear;
        config.watch_dir    = watch_dir.clone();
        config.search_paths = vec![watch_dir, cwd.clone()];
        tambah_path_dependensi(&cwd, &mut config.search_paths);
        config.search_paths.dedup();
        jrgc::hotreload::watch(config);
        return Ok(());
    }

    run_jrg_file(&manifest.entry, verbose, prof, use_vm, emit_bc)
}

pub(crate) fn cmd_exec(file: &Path, args: &[String], verbose: bool, prof: bool,
            use_vm: bool, emit_bc: bool, watch: bool, clear: bool) -> Result<(), String> {
    if let Some(r) = super::bahasa::resep_berkas(file) {
        if use_vm || prof || emit_bc || watch {
            return Err(format!("`--vm`, `--profile`, `--emit-bc` dan `--watch` cuma untuk berkas .jrg; ini berkas {}", r.nama));
        }
        let kode = super::bahasa::exec(file, r, args)?;
        if kode != 0 { process::exit(kode); }
        return Ok(());
    }
    if verbose { eprintln!("[jrgm] exec: {}", file.display()); }

    if watch {
        let watch_dir = file.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let cwd = std::env::current_dir().unwrap_or_else(|_| watch_dir.clone());
        let mut config = jrgc::hotreload::ReloadConfig::new(file.to_path_buf());
        config.use_vm       = use_vm;
        config.profiling    = prof;
        config.emit_bc      = emit_bc;
        config.clear_screen = clear;
        config.watch_dir    = watch_dir.clone();
        config.search_paths = vec![watch_dir, cwd.clone()];
        tambah_path_dependensi(&cwd, &mut config.search_paths);
        config.search_paths.dedup();
        jrgc::hotreload::watch(config);
        return Ok(());
    }

    run_jrg_file(file, verbose, prof, use_vm, emit_bc)
}

/// Jalankan test proyek ini.
pub(crate) fn cmd_test(filter: Option<&str>, use_vm: bool, batas_detik: u64) -> Result<(), String> {
    if let Some((m, r, akar)) = super::bahasa::proyek_kini()? {
        if use_vm { return Err(format!("`--vm` cuma untuk proyek JRG; ini proyek {}", r.nama)); }
        return super::bahasa::uji(&m, r, &akar, filter);
    }
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    if !akar.join("jrgm.toml").is_file() {
        return Err("jrgm test: tidak ada `jrgm.toml` di sini — jalankan dari akar proyek".into());
    }
    let dir_uji = akar.join("tests");
    if !dir_uji.is_dir() {
        return Err(format!(
            "jrgm test: tidak ada direktori `tests/` di `{}`\n  bantuan: taruh berkas ujinya \
             di `tests/*.jrg`; sebuah fungsi dianggap test kalau namanya diawali `test_` \
             atau `uji_`, dan tidak perlu `main`",
            akar.display()
        ));
    }

    let mut berkas: Vec<std::path::PathBuf> = std::fs::read_dir(&dir_uji)
        .map_err(|e| format!("jrgm test: tidak bisa baca `tests/`: {}", jrgc::host::galat_io(&e)))?
        .flatten().map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jrg"))
        .collect();
    berkas.sort();

    // Penemuan dilakukan DI SINI, bukan diserahkan ke tiap subproses, karena
    // runner-nya perlu tahu di depan berapa test yang ada: sebuah berkas tanpa …
    let mut rencana: Vec<(std::path::PathBuf, Vec<String>)> = Vec::new();
    let mut total = 0usize;
    for f in &berkas {
        let src = std::fs::read_to_string(f)
            .map_err(|e| format!("jrgm test: tidak bisa baca `{}`: {}", f.display(), jrgc::host::galat_io(&e)))?;
        let sf = SourceFile { id: 0, name: f.display().to_string(), src };
        let peta = jrgc::diag::SourceMap::with_file(sf.clone());
        let tokens = jrgc::pipa::lex_dilaporkan(&sf, &peta).ok_or_else(String::new)?;
        let (ast, galat) = parse(tokens, true);
        if !galat.is_empty() {
            jrgc::pipa::laporkan_galat_parse(&galat, false, &peta);
            return Err(String::new());
        }
        let mut uji = jrgc::uji::daftar_uji(&ast);
        if let Some(f) = filter { uji.retain(|n| n.contains(f)); }
        if uji.is_empty() { continue; }
        total += uji.len();
        rencana.push((f.clone(), uji));
    }

    if rencana.is_empty() {
        return Err(match filter {
            Some(f) => format!("jrgm test: tidak ada test yang cocok dengan `{}`", f),
            None => format!(
                "jrgm test: tidak ada fungsi test di `tests/*.jrg`\n  bantuan: namanya \
                 harus diawali `test_` atau `uji_`"),
        });
    }

    // `-I`: `src/` proyeknya sendiri plus tiap dependensi, jadi berkas uji
    // bisa `import "bantu"` sama persis seperti berkas di dalam `src/`.
    let mut inc = vec![akar.join("src")];
    super::dependensi::tambah_path_dependensi_dev(&akar, &mut inc);
    inc.dedup();

    let jrgc = jrgc::hotreload::biner_jrgc();
    eprintln!("[jrgm] {} test di {} berkas{}", total, rencana.len(),
        if use_vm { " (VM)" } else { "" });

    // Path ditulis relatif terhadap akar proyek. Yang absolut benar tapi
    // lebarnya bisa melebihi terminal, dan bagian yang menarik — nama
    // berkasnya — justru yang terdorong keluar layar.
    let pendek = |f: &std::path::Path| f.strip_prefix(&akar).unwrap_or(f).display().to_string();

    let mut gagal: Vec<String> = Vec::new();
    for (f, uji) in &rencana {
        eprintln!("\n== {} ({} test)", pendek(f), uji.len());
        let mut cmd = std::process::Command::new(&jrgc);
        cmd.arg("--uji").arg(f);
        if use_vm { cmd.arg("--vm"); }
        if let Some(s) = filter { cmd.arg("--uji-filter").arg(s); }
        for d in &inc { cmd.arg("-I").arg(d); }
        cmd.current_dir(&akar);

        let mut anak = match cmd.spawn() {
            Ok(a) => a,
            Err(e) => return Err(format!(
                "jrgm test: tidak bisa menjalankan `{}`: {}\n  bantuan: set `JRGC_BIN` \
                 kalau `jrgc` ada di tempat lain", jrgc.display(), e)),
        };

        // Ditunggu dengan `try_wait`, bukan `wait`.
        let mulai = std::time::Instant::now();
        let status = loop {
            match anak.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    if mulai.elapsed().as_secs() >= batas_detik {
                        let _ = anak.kill();
                        let _ = anak.wait();
                        break None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => return Err(format!("jrgm test: {}", jrgc::host::galat_io(&e))),
            }
        };

        match status {
            Some(s) if s.success() => {}
            // Kode keluar selain 1 disebutkan.
            Some(st) => gagal.push(match st.code() {
                Some(1) => pendek(f),
                Some(c) => format!("{} (proses keluar dengan kode {} — sebuah test \
                                    memanggil `std::exit`?)", pendek(f), c),
                None    => format!("{} (proses dihentikan sinyal)", pendek(f)),
            }),
            None => {
                eprintln!("  DIBUNUH setelah {} detik — ada test yang tidak selesai", batas_detik);
                gagal.push(format!("{} (timeout)", pendek(f)));
            }
        }
    }

    eprintln!();
    if gagal.is_empty() {
        eprintln!("[jrgm] semua {} test lulus", total);
        return Ok(());
    }
    // Berkas yang gagal disebut namanya lagi di akhir: tiap berkas sudah
    // mencetak rangkumannya sendiri, tapi sesudah keluaran beberapa berkas
    // yang panjang, yang mana saja yang merah sudah tergulung ke atas layar.
    Err(format!("jrgm test: {} dari {} berkas gagal:\n{}",
        gagal.len(), rencana.len(),
        gagal.iter().map(|g| format!("  - {}", g)).collect::<Vec<_>>().join("\n")))
}

pub(crate) fn cmd_check() -> Result<(), String> {
    if let Some((m, r, akar)) = super::bahasa::proyek_kini()? { return super::bahasa::bangun(&m, r, &akar); }
    let cwd = std::env::current_dir()
        .map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;
    let manifest = Manifest::load(&cwd)?;
    check_jrg_file(&manifest.entry, true)
}

/// Menolak sebuah subperintah yang BELUM ADA isinya.
pub(crate) fn belum_ada(perintah: &str, alasan: &str, bantuan: &str) -> Result<(), String> {
    let map = jrgc::diag::SourceMap::new();
    jrgc::diag::laporkan_pesan(
        &map,
        &format!("`jrgm {}` belum ada isinya — {}\nbantuan: {}", perintah, alasan, bantuan),
        false,
    );
    // String kosong: main() sudah tahu artinya "diagnostiknya sudah dicetak
    // lengkap di tempat kejadian, jangan cetak lagi". Yang penting di sini
    // adalah jalur Err-nya, karena itu yang membuat exit code jadi 1.
    Err(String::new())
}

/// Tambahkan sebuah dependensi (path lokal atau repositori git) ke `jrgm.toml`.
pub(crate) fn cmd_add(name: &str, version: Option<&str>, git: Option<&str>, rev: Option<&str>,
           path: Option<&Path>, dev: bool) -> Result<(), String> {
    if path.is_none() && git.is_none() {
        return belum_ada(
            "add",
            &format!("tanpa `--path` atau `--git`, `{}` tidak bisa dicari di mana pun — belum ada registry", name),
            "`jrgm add nama --path ../nama`, atau `jrgm add nama --git <url> --version 0.1.0`",
        );
    }
    if path.is_some() && (git.is_some() || version.is_some() || rev.is_some()) {
        return Err("jrgm add: `--path` tidak bisa digabung dengan `--git`/`--version`/`--rev` — \
            dependensi path selalu terbaca isi direktorinya kini".into());
    }
    if version.is_some() && rev.is_some() {
        return Err("jrgm add: pilih salah satu, `--version` (tag `v<versi>`) atau `--rev`".into());
    }
    let bagian = if dev { "[dev-dependencies]" } else { "[dependencies]" };
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    let manifes = akar.join("jrgm.toml");
    if !manifes.is_file() {
        return Err("jrgm add: tidak ada `jrgm.toml` di sini — jalankan dari akar proyek".into());
    }
    // Diperiksa SEKARANG, bukan saat dijalankan nanti. Sebuah baris yang
    // menunjuk tempat kosong adalah kegagalan yang ditunda, dan yang menunggu
    // di ujungnya orang yang sudah lupa pernah menulisnya.
    if let Some(path) = path.filter(|p| !p.join("jrgm.toml").is_file()) {
        return Err(format!(
            "jrgm add: `{}` bukan proyek JRG — tidak ada `jrgm.toml` di sana\n  \
             bantuan: path-nya akan ditulis apa adanya ke `jrgm.toml`, jadi ia relatif \
             terhadap berkas itu",
            path.display()
        ));
    }

    let isi = std::fs::read_to_string(&manifes)
        .map_err(|e| format!("jrgm add: tidak bisa baca `jrgm.toml`: {}", jrgc::host::galat_io(&e)))?;
    if isi.lines().any(|l| {
        let l = l.trim();
        l.starts_with(name) && l[name.len()..].trim_start().starts_with('=')
    }) {
        return Err(format!(
            "jrgm add: `{}` sudah ada di `jrgm.toml`\n  bantuan: ubah barisnya, atau \
             `jrgm remove {}` dulu", name, name));
    }

    let baris = match (path, git) {
        (Some(p), _) => format!("{} = {{ path = \"{}\" }}\n", name, p.display()),
        (None, Some(url)) => match (version, rev) {
            (Some(v), _) => format!("{} = {{ git = \"{}\", version = \"{}\" }}\n", name, url, v),
            (_, Some(r)) => format!("{} = {{ git = \"{}\", rev = \"{}\" }}\n", name, url, r),
            _ => format!("{} = {{ git = \"{}\" }}\n", name, url),
        },
        _ => unreachable!(),
    };
    let baru = if let Some(i) = isi.find(bagian) {
        let sisip = isi[i..].find('\n').map(|j| i + j + 1).unwrap_or(isi.len());
        format!("{}{}{}", &isi[..sisip], baris, &isi[sisip..])
    } else {
        format!("{}\n{}\n{}", isi.trim_end(), bagian, baris)
    };
    std::fs::write(&manifes, baru)
        .map_err(|e| format!("jrgm add: tidak bisa tulis `jrgm.toml`: {}", jrgc::host::galat_io(&e)))?;

    // Graf diperiksa SESUDAH menulis, dan kegagalannya dilaporkan — sebuah
    // `add` yang membuat siklus harus ketahuan di sini, bukan nanti.
    let graf = if dev { super::dependensi::graf_dependensi_dev(&akar) } else { graf_dependensi(&akar) };
    match graf.and_then(|g| path_dependensi(&g).map(|_| g)) {
        Ok(_) => {
            let asal = path.map(|p| p.display().to_string()).or(git.map(str::to_string)).unwrap_or_default();
            println!("menambah `{}` dari `{}`{}", name, asal, if dev { " (dev)" } else { "" });
            Ok(())
        }
        Err(e) => Err(format!("{}\n  catatan: barisnya sudah ditulis; hapus lagi dengan \
                               `jrgm remove {}`", e, name)),
    }
}

/// Buang sebuah dependensi dari `jrgm.toml`.
pub(crate) fn cmd_remove(name: &str) -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    let manifes = akar.join("jrgm.toml");
    let isi = std::fs::read_to_string(&manifes)
        .map_err(|_| "jrgm remove: tidak ada `jrgm.toml` di sini".to_string())?;
    let mut ketemu = false;
    let baru: String = isi.lines().filter(|l| {
        let t = l.trim();
        let cocok = t.starts_with(name) && t[name.len()..].trim_start().starts_with('=');
        if cocok { ketemu = true; }
        !cocok
    }).map(|l| format!("{}\n", l)).collect();
    if !ketemu {
        return Err(format!("jrgm remove: `{}` tidak ada di `jrgm.toml`", name));
    }
    std::fs::write(&manifes, baru)
        .map_err(|e| format!("jrgm remove: tidak bisa tulis `jrgm.toml`: {}", jrgc::host::galat_io(&e)))?;
    println!("membuang `{}`", name);
    Ok(())
}


/// Gambar graf dependensi yang SEBENARNYA.
pub(crate) fn cmd_tree() -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    if !akar.join("jrgm.toml").is_file() {
        return Err("jrgm tree: tidak ada `jrgm.toml` di sini — jalankan dari akar proyek".into());
    }
    let m = Manifest::load(&akar)?;
    let graf = graf_dependensi(&akar)?;
    // Dipanggil supaya bentrok nama modul ketahuan di sini juga, bukan cuma
    // saat program dijalankan.
    path_dependensi(&graf)?;

    println!("{} {}", m.name, m.version);
    if graf.is_empty() {
        println!("  (tidak ada dependency lokal)");
        return Ok(());
    }
    // Batang `│` hanya boleh diteruskan ke bawah leluhur yang MASIH punya
    // saudara berikutnya.
    let mut leluhur_terakhir: Vec<bool> = Vec::new();
    for (i, s) in graf.iter().enumerate() {
        // "Terakhir di tingkatnya" berarti: sesudah ini, tidak ada lagi simpul
        // sekedalaman SEBELUM muncul yang lebih dangkal.
        let terakhir = !graf[i + 1..].iter()
            .take_while(|n| n.kedalaman >= s.kedalaman)
            .any(|n| n.kedalaman == s.kedalaman);
        leluhur_terakhir.truncate(s.kedalaman - 1);
        let awalan: String = leluhur_terakhir.iter()
            .map(|&t| if t { "    " } else { "│   " })
            .collect();
        leluhur_terakhir.push(terakhir);
        let cabang = if terakhir { "└──" } else { "├──" };
        let ulang = if s.berulang { "  (*)" } else { "" };
        println!("{}{} {} {}{}", awalan, cabang, s.nama, s.versi, ulang);
    }
    if graf.iter().any(|s| s.berulang) {
        println!("\n(*) sudah muncul di atas — ditandai, bukan disembunyikan, supaya");
        println!("    terlihat kalau sebuah paket dipakai dari dua tempat.");
    }
    Ok(())
}

pub(crate) fn cmd_clean()  -> Result<(), String> {
    if std::path::Path::new("target").exists() {
        std::fs::remove_dir_all("target")
            .map_err(|e| format!("tidak bisa hapus target dir: {}", jrgc::host::galat_io(&e)))?;
    }
    eprintln!("[jrgm] target directory dibersihkan");
    Ok(())
}

pub(crate) fn cmd_fmt(paths: &[std::path::PathBuf], check: bool) -> Result<(), String> {
    if paths.is_empty() {
        if let Some((m, r, akar)) = super::bahasa::proyek_kini()? { return super::bahasa::rapikan(&m, r, &akar, check); }
    }
    let cwd = std::env::current_dir()
        .map_err(|e| format!("tidak bisa get cwd: {}", jrgc::host::galat_io(&e)))?;

    // Tanpa argumen: `src/` kalau ada, kalau tidak seluruh direktori kerja.
    // Sengaja tidak memerlukan `jrgm.toml`, supaya `jrgm fmt` juga berguna di
    // folder berisi berkas .jrg lepas.
    let mut berkas = Vec::new();
    let akar;
    if paths.is_empty() {
        akar = if cwd.join("src").is_dir() { cwd.join("src") } else { cwd.clone() };
        kumpul_jrg(&akar, &mut berkas);
    } else {
        akar = paths[0].clone();
        for p in paths {
            if p.is_dir() {
                kumpul_jrg(p, &mut berkas);
            } else if p.is_file() {
                // Sebuah berkas yang DISEBUT namanya dirapikan apa adanya —
                // tidak disaring ekstensinya, karena menyebutkannya sendiri
                // sudah pernyataan niat yang jelas.
                if !berkas.contains(p) { berkas.push(p.clone()); }
            } else {
                return Err(format!("`{}` bukan berkas maupun direktori", p.display()));
            }
        }
    }

    if berkas.is_empty() {
        return Err(format!("tidak ada berkas .jrg di `{}`", akar.display()));
    }

    let mut berubah = Vec::new();
    let mut rusak = Vec::new();

    for p in &berkas {
        let src = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => { rusak.push((p.clone(), format!("tidak bisa dibaca: {}", jrgc::host::galat_io(&e)))); continue; }
        };
        let sf = SourceFile { id: 0, name: p.display().to_string(), src: src.clone() };
        match jrgc::fmt::format_sumber(&sf) {
            Err(e) => rusak.push((p.clone(), e)),
            Ok(hasil) => {
                if hasil == src { continue; }
                berubah.push(p.clone());
                if !check {
                    if let Err(e) = std::fs::write(p, &hasil) {
                        rusak.push((p.clone(), format!("tidak bisa ditulis: {}", e)));
                    }
                }
            }
        }
    }

    // Berkas yang menolak diformat dilaporkan lebih dulu, dan membuat seluruh
    // perintah gagal. Melewatinya diam-diam adalah cara `jrgm fmt` versi lama
    // berbohong, cuma dengan bentuk yang berbeda.
    if !rusak.is_empty() {
        for (p, e) in &rusak {
            eprintln!("  {}: {}", p.display(), e);
        }
        return Err(format!(
            "{} dari {} berkas tidak bisa diformat (lihat di atas)",
            rusak.len(), berkas.len()
        ));
    }

    if check {
        if berubah.is_empty() {
            eprintln!("[jrgm] {} berkas, semuanya sudah rapi", berkas.len());
            return Ok(());
        }
        for p in &berubah {
            eprintln!("  belum rapi: {}", p.display());
        }
        // Keluar TIDAK NOL. Inilah seluruh guna `--check`: versi lama selalu
        // keluar 0, jadi gerbang format di CI mana pun selalu hijau tanpa
        // pernah memeriksa apa pun.
        return Err(format!(
            "{} dari {} berkas belum rapi — jalankan `jrgm fmt` tanpa --check",
            berubah.len(), berkas.len()
        ));
    }

    eprintln!(
        "[jrgm] {} berkas diperiksa, {} dirapikan",
        berkas.len(), berubah.len()
    );
    Ok(())
}


