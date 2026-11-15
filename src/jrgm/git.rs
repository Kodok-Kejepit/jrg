// src/jrgm/git.rs — dependensi dari repositori git (`nama = { git = "url", rev = "v0.1.0" }`), dikunci
// di `jrgm.lock` ke satu commit; `jrgm update` memajukan kuncinya, `jrgm publish` menandai versi
// (`v<versi>`) lalu mendorongnya. Git adalah registry-nya: tidak ada server lain yang dituju.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `(url, rev)` → commit, isi `jrgm.lock` di akar proyek.
pub(crate) struct Kunci {
    berkas: PathBuf,
    isi: Vec<(String, String, String)>,
    dipakai: Vec<(String, String)>,
    berubah: bool,
}

const KEPALA: &str = "# jrgm.lock — dibuat jrgm. Commit berkas ini supaya semua orang memakai commit yang sama;\n\
                      # `jrgm update` memajukannya.\n[git]\n";

impl Kunci {
    /// Diurai `toml::urai`; berkas yang rusak jadi galat, bukan kunci yang hilang diam-diam (lalu ditulis ulang).
    pub(crate) fn baca(akar: &Path) -> Result<Kunci, String> {
        use crate::stdlib::toml::Simpul;
        let berkas = akar.join("jrgm.lock");
        let mut isi = Vec::new();
        let teks = match std::fs::read_to_string(&berkas) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("jrgm.lock: {}", crate::host::galat_io(&e))),
        };
        let rusak = |m: String| format!("jrgm.lock: {}\n  bantuan: hapus berkasnya, lalu `jrgm update` membuatnya lagi", m);
        let Simpul::Tabel(akar_t) = crate::stdlib::toml::urai(&teks).map_err(rusak)? else { unreachable!() };
        for (bagian, v) in &akar_t.isi {
            let Simpul::Tabel(t) = v else { return Err(rusak(format!("`{}` bukan tabel `[git]`", bagian))) };
            if bagian != "git" { return Err(rusak(format!("tabel `[{}]` tidak dikenal", bagian))); }
            for (k, c) in &t.isi {
                let (Some((url, rev)), Simpul::Teks(c)) = (k.rsplit_once(" @ "), c) else {
                    return Err(rusak(format!("entri {:?} bukan `\"url @ rev\" = \"commit\"`", k)));
                };
                isi.push((url.to_string(), rev.to_string(), c.clone()));
            }
        }
        Ok(Kunci { berkas, isi, dipakai: Vec::new(), berubah: false })
    }

    fn cari(&self, url: &str, rev: &str) -> Option<&str> {
        self.isi.iter().find(|(u, r, _)| u == url && r == rev).map(|(_, _, c)| c.as_str())
    }

    fn pasang(&mut self, url: &str, rev: &str, commit: &str) {
        self.dipakai.push((url.to_string(), rev.to_string()));
        if self.cari(url, rev) == Some(commit) { return; }
        self.isi.retain(|(u, r, _)| !(u == url && r == rev));
        self.isi.push((url.to_string(), rev.to_string(), commit.to_string()));
        self.berubah = true;
    }

    /// Dipanggil sesudah graf ditelusuri UTUH: entri yang tidak lagi disebut siapa pun dibuang.
    pub(crate) fn simpan(&mut self) -> Result<(), String> {
        let n = self.isi.len();
        let dipakai = std::mem::take(&mut self.dipakai);
        self.isi.retain(|(u, r, _)| dipakai.iter().any(|(du, dr)| du == u && dr == r));
        self.berubah |= self.isi.len() != n;
        if !self.berubah { return Ok(()); }
        if self.isi.is_empty() { let _ = std::fs::remove_file(&self.berkas); self.berubah = false; return Ok(()); }
        self.isi.sort();
        let mut s = KEPALA.to_string();
        for (u, r, c) in &self.isi {
            let t = crate::stdlib::toml::tulis_teks;
            s.push_str(&format!("{} = {}\n", t(&format!("{} @ {}", u, r)), t(c)));
        }
        std::fs::write(&self.berkas, s).map_err(|e| format!("jrgm.lock: {}", crate::host::galat_io(&e)))?;
        self.berubah = false;
        Ok(())
    }
}

fn tembolok() -> PathBuf {
    std::env::var_os("JRGM_TEMBOLOK").map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(|p| PathBuf::from(p).join("jrgm")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache").join("jrgm")))
        .unwrap_or_else(|| std::env::temp_dir().join("jrgm")).join("git")
}

fn sidik(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() { h = (h ^ b as u64).wrapping_mul(0x100000001b3); }
    format!("{:016x}", h)
}

/// Jalankan git; keluarannya (stdout) kalau sukses, kalimat galat bersebab kalau tidak.
fn git(args: &[&str], dir: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    if let Some(d) = dir { c.current_dir(d); }
    c.args(args).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
    let out = c.output().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
        "dependensi git butuh `git`, dan perintahnya tidak ada\n  bantuan: `sudo apt install git` (Termux: `pkg install git`)".to_string()
    } else { format!("git: {}", crate::host::galat_io(&e)) })?;
    if out.status.success() { return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()); }
    Err(format!("`git {}` gagal: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
}

/// Cermin telanjang repositori `url` di tembolok, diambil atau diperbarui kalau `segarkan`.
fn cermin(url: &str, segarkan: bool) -> Result<PathBuf, String> {
    let db = tembolok().join("db").join(format!("{}.git", sidik(url)));
    let db_s = db.to_string_lossy().to_string();
    if !db.join("HEAD").is_file() {
        std::fs::create_dir_all(db.parent().unwrap()).map_err(|e| crate::host::galat_io(&e))?;
        let _ = std::fs::remove_dir_all(&db);
        let judul = format!("mengambil {}", url);
        let mut c = Command::new("git");
        c.args(["-c", "protocol.ext.allow=never", "clone", "--bare", "--quiet", "--", url, &db_s]).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
        let out = crate::putar::jalankan(&mut c, &judul).map_err(|e| format!("git: {}", crate::host::galat_io(&e)))?;
        if !out.status.success() {
            return Err(format!("tidak bisa mengambil `{}`: {}", url, String::from_utf8_lossy(&out.stderr).trim()));
        }
    } else if segarkan {
        git(&["-c", "protocol.ext.allow=never", "fetch", "--quiet", "--force", "--tags", "--", url, "+refs/heads/*:refs/heads/*"], Some(&db))
            .map_err(|e| format!("tidak bisa memperbarui `{}`: {}", url, e))?;
    }
    Ok(db)
}

fn sidik_commit(c: &str) -> bool {
    (c.len() == 40 || c.len() == 64) && c.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn commit_dari(db: &Path, rev: &str) -> Result<String, String> {
    let r = if rev.is_empty() { "HEAD" } else { rev };
    let c = git(&["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", r)], Some(db))
        .map_err(|_| format!("`{}` tidak ada di repositorinya (bukan cabang, tag, maupun commit)", r))?;
    if !sidik_commit(&c) { return Err(format!("git menjawab {:?} untuk `{}`, bukan sidik commit", c, r)); }
    Ok(c)
}

/// Direktori berisi isi `url` pada commit yang dikunci; diambil sekali, dipakai ulang.
pub(crate) fn siapkan(kunci: &mut Kunci, nama: &str, url: &str, rev: &str, segarkan: bool) -> Result<PathBuf, String> {
    super::manifes::periksa_nama_dependensi(nama)?;
    super::manifes::periksa_git(nama, url, rev)?;
    let ada = if segarkan { None } else { kunci.cari(url, rev).map(str::to_string) };
    // Isi `jrgm.lock` ikut ke jalur tembolok dan ke `git archive`, jadi cuma sidik commit yang diterima.
    if let Some(c) = ada.as_deref().filter(|c| !sidik_commit(c)) {
        return Err(format!("`jrgm.lock` mengunci `{} @ {}` ke {:?}, yang bukan sidik commit\n  \
            bantuan: hapus baris itu dari `jrgm.lock`, lalu `jrgm update {}`", url, rev, c, nama));
    }
    let commit = match ada {
        Some(c) => c,
        None => { let db = cermin(url, true)?; commit_dari(&db, rev)? }
    };
    kunci.pasang(url, rev, &commit);
    let tujuan = tembolok().join("co").join(format!("{}-{}", nama, &commit[..commit.len().min(12)]));
    if tujuan.join("jrgm.toml").is_file() { return Ok(tujuan); }
    let mut db = cermin(url, false)?;
    if commit_dari(&db, &commit).is_err() { db = cermin(url, true)?; commit_dari(&db, &commit)?; }
    let sementara = tujuan.with_extension("sebagian");
    let _ = std::fs::remove_dir_all(&sementara);
    std::fs::create_dir_all(&sementara).map_err(|e| crate::host::galat_io(&e))?;
    let mut arsip = Command::new("git").arg("--git-dir").arg(&db).args(["archive", "--format=tar", &commit])
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("git: {}", crate::host::galat_io(&e)))?;
    let tar = Command::new("tar").arg("-x").arg("-C").arg(&sementara).stdin(arsip.stdout.take().unwrap())
        .status().map_err(|e| format!("tar: {}", crate::host::galat_io(&e)))?;
    let a = arsip.wait().map_err(|e| crate::host::galat_io(&e))?;
    if !a.success() || !tar.success() {
        return Err(format!("tidak bisa membuka commit {} dari `{}`", &commit[..12.min(commit.len())], url));
    }
    let _ = std::fs::remove_dir_all(&tujuan);
    std::fs::rename(&sementara, &tujuan).map_err(|e| crate::host::galat_io(&e))?;
    Ok(tujuan)
}

/// `--version 0.2.0` berarti tag `v0.2.0`, bentuk yang ditulis `jrgm publish`; versinya SemVer persis, karena
/// registry git tidak punya rentang — `^1` dulu jadi tag `v^1` yang tidak pernah ada.
pub(crate) fn rev_versi(v: &str) -> Result<String, String> {
    match crate::stdlib::semver::urai(v) {
        Ok(_) => Ok(format!("v{}", v)),
        Err(e) => Err(format!("version {}\n  bantuan: tulis versi terbit persis (mis. `0.1.0`); \
            rentang belum didukung, `rev` menerima cabang atau tag lain", e)),
    }
}

/// `jrgm update [nama]`: tiap dependensi git dimajukan ke commit terbaru rev-nya.
pub(crate) fn cmd_update(nama: Option<&str>) -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", crate::host::galat_io(&e)))?;
    if !akar.join("jrgm.toml").is_file() {
        return Err("jrgm update: tidak ada `jrgm.toml` di sini — jalankan dari akar proyek".into());
    }
    let mut kunci = Kunci::baca(&akar)?;
    let mut tumpukan = vec![akar.clone()];
    let mut sudah: Vec<String> = Vec::new();
    let (mut maju, mut tetap, mut cocok) = (0, 0, false);
    while let Some(dir) = tumpukan.pop() {
        let m = super::manifes::Manifest::load(&dir)?;
        for d in m.dependencies.iter().chain(if dir == akar { m.dev_dependencies.iter() } else { [].iter() }) {
            let Some((url, rev)) = &d.git else { tumpukan.push(d.akar.clone()); continue };
            let kunci_ini = format!("{} @ {}", url, rev);
            if sudah.contains(&kunci_ini) { continue; }
            sudah.push(kunci_ini);
            let lama = kunci.cari(url, rev).map(str::to_string);
            let segarkan = nama.map_or(true, |n| n == d.nama);
            cocok |= nama == Some(d.nama.as_str());
            let dir_dep = siapkan(&mut kunci, &d.nama, url, rev, segarkan)?;
            let baru = kunci.cari(url, rev).unwrap_or("").to_string();
            match &lama {
                Some(l) if *l == baru => tetap += 1,
                _ => {
                    maju += 1;
                    println!("  {} {} -> {}", d.nama, lama.as_deref().map_or("(baru)", |l| &l[..12.min(l.len())]), &baru[..12.min(baru.len())]);
                }
            }
            tumpukan.push(dir_dep);
        }
    }
    if let Some(n) = nama.filter(|_| !cocok) {
        return Err(format!("jrgm update: `{}` bukan dependensi git proyek ini", n));
    }
    kunci.simpan()?;
    if maju + tetap == 0 {
        println!("tidak ada dependensi git; dependensi path selalu terbaca versi terakhirnya");
    } else {
        println!("{} dimajukan, {} sudah terbaru", maju, tetap);
    }
    Ok(())
}

/// `jrgm publish`: sumber bersih, terbangun dan lulus uji, lalu tag `v<versi>` didorong ke repositorinya.
pub(crate) fn cmd_publish(dry_run: bool) -> Result<(), String> {
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", crate::host::galat_io(&e)))?;
    if !akar.join("jrgm.toml").is_file() {
        return Err("jrgm publish: tidak ada `jrgm.toml` di sini — jalankan dari akar proyek".into());
    }
    let m = super::manifes::Manifest::load(&akar)?;
    if let Err(e) = crate::stdlib::semver::urai(&m.version) {
        return Err(format!("jrgm publish: versi paket {}", e));
    }
    if m.dependencies.iter().any(|d| d.git.is_none()) {
        let n: Vec<&str> = m.dependencies.iter().filter(|d| d.git.is_none()).map(|d| d.nama.as_str()).collect();
        return Err(format!("jrgm publish: dependensi path ({}) tidak ikut terbawa ke pemakai\n  bantuan: \
            terbitkan dulu paket itu, lalu `jrgm add <nama> --git <url> --version <versi>`", n.join(", ")));
    }
    git(&["rev-parse", "--show-toplevel"], Some(&akar))
        .map_err(|_| "jrgm publish: proyek ini bukan repositori git\n  bantuan: `git init`, commit, lalu tambahkan remote".to_string())?;
    let kotor = git(&["status", "--porcelain", "--", "."], Some(&akar))?;
    if !kotor.is_empty() {
        return Err(format!("jrgm publish: ada perubahan yang belum di-commit:\n{}\n  bantuan: commit dulu — \
            yang diterbitkan adalah commit, bukan isi direktori", kotor));
    }
    let tag = format!("v{}", m.version);
    if git(&["rev-parse", "--verify", "--quiet", &format!("refs/tags/{}", tag)], Some(&akar)).is_ok() {
        return Err(format!("jrgm publish: tag `{}` sudah ada — versi yang sudah terbit tidak ditimpa\n  \
            bantuan: naikkan `version` di jrgm.toml", tag));
    }
    let tujuan = match &m.repositori {
        Some(r) => r.clone(),
        None => git(&["remote", "get-url", "origin"], Some(&akar)).map_err(|_|
            "jrgm publish: tidak tahu ke mana mendorong — tidak ada remote `origin`\n  bantuan: \
             `git remote add origin <url>`, atau `repositori = \"<url>\"` di [package]".to_string())?,
    };
    super::perintah::cmd_build("dev", None, false, true)?;
    if akar.join("tests").is_dir() { super::perintah::cmd_test(None, false, 120)?; }
    let commit = git(&["rev-parse", "HEAD"], Some(&akar))?;
    if dry_run {
        println!("akan menandai commit {} sebagai `{}` dan mendorongnya ke {}", &commit[..12], tag, tujuan);
        println!("(--dry-run: tidak ada yang ditulis)");
        return Ok(());
    }
    git(&["tag", "-a", &tag, "-m", &format!("{} {}", m.name, m.version)], Some(&akar))?;
    if let Err(e) = git(&["push", "--quiet", &tujuan, &format!("refs/tags/{}", tag)], Some(&akar)) {
        let _ = git(&["tag", "-d", &tag], Some(&akar));
        return Err(format!("jrgm publish: {}\n  catatan: tag lokal `{}` dibuang lagi, jadi publish bisa diulang", e, tag));
    }
    println!("{} {} diterbitkan ({} di {})", m.name, m.version, tag, tujuan);
    println!("pemakai: jrgm add {} --git {} --version {}", m.name, tujuan, m.version);
    Ok(())
}
