// src/stdlib/proses.rs
// `proses::` — menjalankan program lain. Tanpa ini sebuah program JRG tidak
// bisa memanggil `git`, `ls`, atau dirinya sendiri.

use crate::interpreter::Value;
use crate::stdlib::sistem::lewat_hasil;
use crate::vm::Vm;
use std::collections::HashMap;
use std::process::Command;

pub fn daftar(vm: &mut Vm) {
    vm.def("proses::jalankan", |a| lewat_hasil(a, b_jalankan));
    vm.def("proses::keluaran", |a| lewat_hasil(a, b_keluaran));
    vm.def("proses::status", |a| lewat_hasil(a, b_status));
    vm.def("proses::ada", |a| lewat_hasil(a, b_ada));
    vm.def("proses::pid", |a| lewat_hasil(a, b_pid));
    vm.def("proses::kill", |a| lewat_hasil(a, b_kill));
    vm.def("proses::mulai", |a| lewat_hasil(a, b_mulai));
    vm.def("proses::baca", |a| lewat_hasil(a, b_baca));
    vm.def("proses::tulis", |a| lewat_hasil(a, b_tulis));
    vm.def("proses::tutup_masukan", |a| lewat_hasil(a, b_tutup_masukan));
    vm.def("proses::hentikan", |a| lewat_hasil(a, b_hentikan));
}

fn teks(v: &Value, nama: &str, ke: usize) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        lain => Err(format!("{}: argumen ke-{} harus string, dapat `{}`",
                            nama, ke + 1, lain.type_name())),
    }
}

fn args(v: Option<&Value>, nama: &str) -> Result<Vec<String>, String> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) => a.iter().enumerate()
            .map(|(i, x)| teks(x, nama, i)).collect(),
        Some(l) => Err(format!("{}: argumen harus array of string, dapat `{}`",
                               nama, l.type_name())),
    }
}

/// Program dijalankan LANGSUNG, tidak lewat shell. Sebuah nama berkas yang
/// memuat spasi atau `;` karena itu tidak pernah berubah jadi perintah lain.
fn siapkan(a: &[Value], nama: &str) -> Result<Command, String> {
    let prog = teks(a.first().ok_or_else(|| format!("{}(program, args?)", nama))?, nama, 0)?;
    let arg = args(a.get(1), nama)?;
    let mut c = Command::new(&prog);
    c.args(&arg);
    Ok(c)
}

fn galat_spawn(prog: &str, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        format!("program `{}` tidak ketemu di PATH", prog)
    } else {
        format!("gagal menjalankan `{}`: {}", prog, crate::host::galat_io(&e))
    }
}

/// Jalankan sampai selesai, kembalikan stdout. stderr ikut supaya pesan
/// galat program tidak hilang begitu saja.
fn b_jalankan(a: &[Value]) -> Result<Value, String> {
    if a.is_empty() || a.len() > 2 {
        return Err("proses::jalankan(program, args?) -> Result".to_string());
    }
    let prog = teks(&a[0], "proses::jalankan", 0)?;
    let mut c = siapkan(a, "proses::jalankan")?;
    match c.output() {
        Err(e) => Ok(crate::interpreter::err_value(galat_spawn(&prog, e))),
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            if !o.stderr.is_empty() {
                s.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            Ok(crate::interpreter::ok_value(Value::Str(s)))
        }
    }
}

/// Opsi `proses::keluaran`: batas waktu, isi stdin, direktori kerja, dan variabel lingkungan.
#[derive(Default)]
struct Opsi { batas_ms: Option<u64>, masukan: Option<String>, dir: Option<String>, env: Vec<(String, String)>, kelompok: bool }

/// Matikan `anak`. Dengan `kelompok` (ia dimulai sebagai pemimpin kelompok proses), seluruh
/// kelompoknya ikut — tanpa itu cucunya tetap hidup dan menahan pipa yang sedang dibaca.
fn matikan(anak: &mut std::process::Child, kelompok: bool) {
    #[cfg(unix)]
    if kelompok {
        extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
        let pid = anak.id() as i32;
        if pid > 1 { unsafe { kill(-pid, 9); } }
    }
    let _ = (kelompok, anak.kill());
}

/// `kelompok`: program jadi pemimpin kelompok proses baru, supaya `matikan` menjangkau turunannya.
fn atur_kelompok(c: &mut std::process::Command, kelompok: bool) {
    #[cfg(unix)]
    if kelompok { std::os::unix::process::CommandExt::process_group(c, 0); }
    let _ = (c, kelompok);
}

fn opsi(v: Option<&Value>, nama: &str) -> Result<Opsi, String> {
    let mut o = Opsi::default();
    let isi = match v {
        None => return Ok(o),
        Some(Value::Struct { name, fields }) if name == "dict" => fields,
        Some(l) => return Err(format!("{}: opsi harus dict, dapat `{}`", nama, l.type_name())),
    };
    for (k, x) in isi.iter() {
        match (k.as_str(), x) {
            ("batas_ms", Value::Int(n)) if *n > 0 && *n <= 86_400_000 => o.batas_ms = Some(*n as u64),
            ("batas_ms", l) => return Err(format!("{}: `batas_ms` harus bilangan bulat 1 sampai \
                                                   86400000 (sehari), dapat `{}`", nama, l)),
            ("masukan", Value::Str(t)) => o.masukan = Some(t.to_string()),
            ("dir", Value::Str(t)) => o.dir = Some(t.to_string()),
            ("env", Value::Struct { name, fields: e }) if name == "dict" => for (n, w) in e.iter() {
                match w {
                    Value::Str(t) => o.env.push((n.clone(), t.to_string())),
                    l => return Err(format!("{}: nilai `env` harus teks, `{}` bertipe `{}`", nama, n, l.type_name())),
                }
            },
            ("kelompok", Value::Bool(b)) => o.kelompok = *b,
            ("masukan" | "dir" | "env" | "kelompok", l) => return Err(format!("{}: opsi `{}` salah tipe (`{}`)", nama, k, l.type_name())),
            _ => return Err(format!("{}: opsi `{}` tidak dikenal\n  bantuan: yang ada `batas_ms`, \
                                     `masukan`, `dir`, `env`, `kelompok`", nama, k)),
        }
    }
    Ok(o)
}

/// Bentuk penuh: stdout, stderr, dan kode keluar dipisah — sebuah program
/// yang gagal dengan pesan di stderr tidak terlihat sama dengan yang berhasil.
fn b_keluaran(a: &[Value]) -> Result<Value, String> {
    if a.is_empty() || a.len() > 3 {
        return Err("proses::keluaran(program, args?, opsi?) -> Result".to_string());
    }
    let prog = teks(&a[0], "proses::keluaran", 0)?;
    let o = opsi(a.get(2), "proses::keluaran")?;
    let mut c = siapkan(a, "proses::keluaran")?;
    if let Some(d) = &o.dir { c.current_dir(d); }
    for (k, v) in &o.env { c.env(k, v); }
    atur_kelompok(&mut c, o.kelompok);
    use std::process::Stdio;
    c.stdin(if o.masukan.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut anak = match c.spawn() {
        Err(e) => return Ok(crate::interpreter::err_value(galat_spawn(&prog, e))),
        Ok(x) => x,
    };
    // Tiga pipa dikuras di utasnya sendiri: program yang menulis banyak tidak boleh macet menunggu dibaca.
    let tulis = anak.stdin.take().zip(o.masukan.clone()).map(|(mut p, t)| std::thread::spawn(move || {
        use std::io::Write;
        let _ = p.write_all(t.as_bytes());
    }));
    fn kuras<R: std::io::Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || { let mut b = Vec::new(); if let Some(mut r) = r { let _ = std::io::Read::read_to_end(&mut r, &mut b); } b })
    }
    let (keluar, galat) = (kuras(anak.stdout.take()), kuras(anak.stderr.take()));
    let mulai = std::time::Instant::now();
    let mut habis = false;
    let status = loop {
        match anak.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if o.batas_ms.map_or(false, |b| mulai.elapsed().as_millis() as u64 >= b) {
            habis = true;
            matikan(&mut anak, o.kelompok);
            break anak.wait().ok();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    if let Some(t) = tulis { let _ = t.join(); }
    let (k, g) = (keluar.join().unwrap_or_default(), galat.join().unwrap_or_default());
    let mut f: HashMap<String, Value> = HashMap::new();
    f.insert("keluaran".into(), Value::Str(String::from_utf8_lossy(&k).to_string()));
    f.insert("galat".into(), Value::Str(String::from_utf8_lossy(&g).to_string()));
    f.insert("kode".into(), Value::Int(status.and_then(|s| s.code()).unwrap_or(-1) as i128));
    f.insert("sukses".into(), Value::Bool(!habis && status.map_or(false, |s| s.success())));
    f.insert("habis_waktu".into(), Value::Bool(habis));
    Ok(crate::interpreter::ok_value(Value::Struct { name: "ProsesHasil".to_string(), fields: f.into() }))
}

/// Proses yang dimulai `proses::mulai` dan masih bisa ditanya: pipanya dikuras utas ke penampung,
/// jadi `proses::baca` tidak pernah menunggu.
struct Jalan {
    anak: std::process::Child,
    masukan: Option<std::process::ChildStdin>,
    keluar: Penampung,
    galat: Penampung,
    pembaca: Vec<std::thread::JoinHandle<()>>,
    habis: bool,
    kelompok: bool,
}

type Penampung = std::sync::Arc<std::sync::Mutex<Vec<u8>>>;

fn daftar_jalan() -> std::sync::MutexGuard<'static, (i128, HashMap<i128, Jalan>)> {
    static D: std::sync::OnceLock<std::sync::Mutex<(i128, HashMap<i128, Jalan>)>> = std::sync::OnceLock::new();
    D.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn tampung<R: std::io::Read + Send + 'static>(r: Option<R>) -> (Penampung, Option<std::thread::JoinHandle<()>>) {
    let p: Penampung = Default::default();
    let q = p.clone();
    let utas = r.map(|mut r| std::thread::spawn(move || {
        let mut b = [0u8; 8192];
        while let Ok(n) = std::io::Read::read(&mut r, &mut b) {
            if n == 0 { break; }
            q.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&b[..n]);
        }
    }));
    (p, utas)
}

/// Ambil isi penampung sebagai teks. Karakter yang baru datang SEPARUH ditinggal untuk bacaan
/// berikutnya (kecuali `semua`), supaya huruf multi-byte tidak pecah jadi dua tanda tanya.
fn ambil(p: &Penampung, semua: bool) -> String {
    let mut b = p.lock().unwrap_or_else(|e| e.into_inner());
    let sampai = match std::str::from_utf8(&b) {
        Err(e) if !semua && e.error_len().is_none() => e.valid_up_to(),
        _ => b.len(),
    };
    let siap: Vec<u8> = b.drain(..sampai).collect();
    String::from_utf8_lossy(&siap).to_string()
}

fn nomor(a: &[Value], nama: &str) -> Result<i128, String> {
    match a.first() {
        Some(Value::Int(n)) => Ok(*n),
        Some(l) => Err(format!("{}: argumen ke-1 harus nomor proses dari `proses::mulai` (int), dapat `{}`", nama, l.type_name())),
        None => Err(format!("{}: butuh nomor proses dari `proses::mulai`", nama)),
    }
}

fn tidak_ada(nama: &str, n: i128) -> Value {
    crate::interpreter::err_value(format!("{}: tidak ada proses bernomor {} (belum pernah dimulai, atau sudah selesai \
                                           dan sudah dibaca)", nama, n))
}

/// Mulai program TANPA menunggunya; hasilnya nomor untuk `baca`/`tulis`/`hentikan`.
fn b_mulai(a: &[Value]) -> Result<Value, String> {
    if a.is_empty() || a.len() > 3 {
        return Err("proses::mulai(program, args?, opsi?) -> Result".to_string());
    }
    let prog = teks(&a[0], "proses::mulai", 0)?;
    let o = opsi(a.get(2), "proses::mulai")?;
    let mut c = siapkan(a, "proses::mulai")?;
    if let Some(d) = &o.dir { c.current_dir(d); }
    for (k, v) in &o.env { c.env(k, v); }
    atur_kelompok(&mut c, o.kelompok);
    use std::process::Stdio;
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut anak = match c.spawn() {
        Err(e) => return Ok(crate::interpreter::err_value(galat_spawn(&prog, e))),
        Ok(x) => x,
    };
    let mut masukan = anak.stdin.take();
    if let (Some(p), Some(t)) = (masukan.as_mut(), &o.masukan) {
        use std::io::Write;
        let _ = p.write_all(t.as_bytes()).and_then(|_| p.flush());
    }
    let (keluar, u1) = tampung(anak.stdout.take());
    let (galat, u2) = tampung(anak.stderr.take());
    let mut d = daftar_jalan();
    d.0 += 1;
    let id = d.0;
    d.1.insert(id, Jalan { anak, masukan, keluar, galat, pembaca: u1.into_iter().chain(u2).collect(), habis: false, kelompok: o.kelompok });
    drop(d);
    if let Some(batas) = o.batas_ms {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(batas));
            if let Some(j) = daftar_jalan().1.get_mut(&id) {
                if let Ok(None) = j.anak.try_wait() { j.habis = true; matikan(&mut j.anak, j.kelompok); }
            }
        });
    }
    Ok(crate::interpreter::ok_value(Value::Int(id)))
}

/// Keluaran yang datang SEJAK bacaan terakhir, tanpa menunggu. Sesudah menjawab `selesai`, nomornya hangus.
fn b_baca(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("proses::baca(nomor) -> Result".to_string()); }
    let id = nomor(a, "proses::baca")?;
    let mut d = daftar_jalan();
    let status = match d.1.get_mut(&id) {
        None => return Ok(tidak_ada("proses::baca", id)),
        Some(j) => j.anak.try_wait().ok().flatten(),
    };
    let mut f: HashMap<String, Value> = HashMap::new();
    if let Some(s) = status {
        let mut j = d.1.remove(&id).expect("baru saja ada");
        drop(d);
        j.masukan = None;
        // Sisa kelompoknya (pekerjaan latar sebuah shell) masih memegang pipanya; tanpa ini `join` di bawah menunggu selamanya.
        if j.kelompok { matikan(&mut j.anak, true); }
        for u in j.pembaca.drain(..) { let _ = u.join(); }
        f.insert("keluaran".into(), Value::Str(ambil(&j.keluar, true)));
        f.insert("galat".into(), Value::Str(ambil(&j.galat, true)));
        f.insert("selesai".into(), Value::Bool(true));
        f.insert("kode".into(), Value::Int(s.code().unwrap_or(-1) as i128));
        f.insert("sukses".into(), Value::Bool(!j.habis && s.success()));
        f.insert("habis_waktu".into(), Value::Bool(j.habis));
    } else {
        let j = d.1.get(&id).expect("baru saja ada");
        f.insert("keluaran".into(), Value::Str(ambil(&j.keluar, false)));
        f.insert("galat".into(), Value::Str(ambil(&j.galat, false)));
        f.insert("selesai".into(), Value::Bool(false));
        f.insert("kode".into(), Value::Int(-1));
        f.insert("sukses".into(), Value::Bool(false));
        f.insert("habis_waktu".into(), Value::Bool(false));
    }
    Ok(crate::interpreter::ok_value(Value::Struct { name: "ProsesBagian".to_string(), fields: f.into() }))
}

/// Kirim teks ke stdin proses yang sedang jalan.
fn b_tulis(a: &[Value]) -> Result<Value, String> {
    if a.len() != 2 { return Err("proses::tulis(nomor, teks) -> Result".to_string()); }
    let id = nomor(a, "proses::tulis")?;
    let t = teks(&a[1], "proses::tulis", 1)?;
    let mut d = daftar_jalan();
    let j = match d.1.get_mut(&id) { None => return Ok(tidak_ada("proses::tulis", id)), Some(j) => j };
    let p = match j.masukan.as_mut() {
        None => return Ok(crate::interpreter::err_value(format!("proses::tulis: masukan proses {} sudah ditutup", id))),
        Some(p) => p,
    };
    use std::io::Write;
    Ok(match p.write_all(t.as_bytes()).and_then(|_| p.flush()) {
        Ok(()) => crate::interpreter::ok_value(Value::Unit),
        Err(e) => crate::interpreter::err_value(format!("proses::tulis: proses {} tidak menerima masukan lagi ({})", id, crate::host::galat_io(&e))),
    })
}

/// Tutup stdin: program yang membaca sampai habis baru bisa selesai sesudah ini.
fn b_tutup_masukan(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("proses::tutup_masukan(nomor) -> Result".to_string()); }
    let id = nomor(a, "proses::tutup_masukan")?;
    match daftar_jalan().1.get_mut(&id) {
        None => Ok(tidak_ada("proses::tutup_masukan", id)),
        Some(j) => { j.masukan = None; Ok(crate::interpreter::ok_value(Value::Unit)) }
    }
}

/// Hentikan proses yang dimulai `proses::mulai`. `true` kalau ia memang masih jalan.
fn b_hentikan(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("proses::hentikan(nomor) -> Result".to_string()); }
    let id = nomor(a, "proses::hentikan")?;
    match daftar_jalan().1.get_mut(&id) {
        None => Ok(tidak_ada("proses::hentikan", id)),
        Some(j) => {
            let hidup = matches!(j.anak.try_wait(), Ok(None));
            if hidup { matikan(&mut j.anak, j.kelompok); }
            Ok(crate::interpreter::ok_value(Value::Bool(hidup)))
        }
    }
}

/// Jalankan sambil MEWARISI terminal — keluarannya langsung tampil, dan
/// program interaktif tetap bisa dipakai. Yang dikembalikan kode keluarnya.
fn b_status(a: &[Value]) -> Result<Value, String> {
    if a.is_empty() || a.len() > 2 {
        return Err("proses::status(program, args?) -> Result".to_string());
    }
    let prog = teks(&a[0], "proses::status", 0)?;
    let mut c = siapkan(a, "proses::status")?;
    match c.status() {
        Err(e) => Ok(crate::interpreter::err_value(galat_spawn(&prog, e))),
        Ok(s) => Ok(crate::interpreter::ok_value(
            Value::Int(s.code().unwrap_or(-1) as i128))),
    }
}

/// Apakah sebuah program ada di PATH. Ditanya DULU supaya program bisa
/// memilih jalan lain alih-alih gagal di tengah.
fn b_ada(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("proses::ada(program) -> bool".to_string()); }
    let prog = teks(&a[0], "proses::ada", 0)?;
    let cek = if cfg!(windows) { "where" } else { "which" };
    Ok(Value::Bool(Command::new(cek).arg(&prog)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)))
}

/// PID proses JRG yang sedang berjalan.
fn b_pid(_a: &[Value]) -> Result<Value, String> {
    Ok(Value::Int(std::process::id() as i128))
}

/// Hentikan proses lain.
fn b_kill(a: &[Value]) -> Result<Value, String> {
    let pid = match a.first() {
        Some(Value::Int(n)) if *n > 0 && *n <= i32::MAX as i128 => *n,
        Some(Value::Int(n)) if *n > 0 => return Err(format!(
            "proses::kill: pid `{}` di luar jangkauan — paling besar {}, dan \
             perintah `kill` memotong angka di atasnya jadi 32 bit sehingga yang \
             dihentikan proses LAIN (`9223372036854775807` bahkan jadi -1: semua \
             proses milikmu)", n, i32::MAX)),
        Some(Value::Int(n)) => return Err(format!(
            "proses::kill: pid harus positif, dapat `{}` — pid 0 dan negatif di \
             Unix berarti SEKELOMPOK proses (termasuk grup proses ini sendiri), \
             dan itu tidak pernah yang dimaksud", n)),
        Some(l) => return Err(format!(
            "proses::kill: argumen ke-1 harus int, dapat `{}`", l.type_name())),
        None => return Err("proses::kill(pid: int) -> bool".to_string()),
    };
    let mut c = if cfg!(windows) {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/F"]);
        c
    } else {
        let mut c = Command::new("kill");
        c.args(["-TERM", &pid.to_string()]);
        c
    };
    match c.output() {
        Ok(o) => Ok(Value::Bool(o.status.success())),
        Err(e) => Err(format!("proses::kill: tidak bisa menjalankan `kill`: {}", crate::host::galat_io(&e))),
    }
}
