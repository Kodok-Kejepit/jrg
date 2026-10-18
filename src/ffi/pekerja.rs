//! Mesin worker FFI: satu proses anak, satu pipa, protokol baris JSON.

use crate::interpreter::{err_value, ok_value, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Satu proses worker yang hidup.
pub struct Worker {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

/// Satu-satunya jalan worker bisa menjalankan kode JRG.
pub type Panggil<'a> = &'a mut dyn FnMut(&Value, Vec<Value>) -> Result<Value, String>;

/// Segala yang membedakan satu bahasa FFI dari yang lain.
pub struct Runtime {
    /// Untuk pesan galat: "worker python", "worker javascript".
    pub nama: &'static str,
    /// Awalan namespace tanpa `::`, mis. `"py"`.
    pub ns: &'static str,
    pub bin: &'static str,
    pub bin_args: &'static [&'static str],
    pub skrip_src: &'static str,
    /// Nama berkas skrip di temp dir, mis. `"jrg_py_runner.py"`.
    pub skrip_nama: &'static str,
    /// Saran pemasangan kalau binernya tidak ada.
    pub bantuan_pasang: &'static str,

    pub slot: fn() -> &'static Mutex<Option<Worker>>,
    pub generasi: &'static AtomicU64,
    pub antrean: fn() -> &'static Mutex<Vec<u64>>,
    pub registri: fn() -> &'static Mutex<RegistriFungsi>,

    pub fungsi_boleh_get: fn() -> bool,
    pub fungsi_boleh_set: fn(bool),
    pub dalam_cb_get: fn() -> bool,
    pub dalam_cb_set: fn(bool),

    /// Bikin nilai handle milik bahasa ini (`Value::PyObj` / `Value::JsObj`).
    pub buat_handle: fn(u64, u64, String) -> Value,
    /// Tulis sebuah handle milik bahasa ini ke JSON. `None` kalau nilainya
    /// bukan handle bahasa ini — pemanggil lalu meneruskan ke aturan umum.
    pub tulis_handle: fn(&Value, &mut String) -> Option<Result<(), String>>,
}

/// Fungsi JRG yang sedang dipegang worker. Nomornya tidak pernah dipakai ulang
/// dalam satu sesi, jadi nomor yang sudah dibebaskan tidak bisa menunjuk fungsi lain.
#[derive(Default)]
pub struct RegistriFungsi {
    isi: std::collections::HashMap<usize, Value>,
    berikut: usize,
}

impl RegistriFungsi {
    pub fn jumlah(&self) -> usize { self.isi.len() }
}

impl Runtime {
    pub fn generasi_kini(&self) -> u64 { self.generasi.load(Ordering::SeqCst) }

    /// Antre sebuah id untuk dilepas di permintaan berikutnya.
    pub fn antre_lepas(&self, id: u64) {
        if let Ok(mut q) = (self.antrean)().lock() { q.push(id); }
    }

    /// Daftarkan sebuah fungsi JRG untuk diserahkan ke worker, kalau
    /// permintaan yang sedang dirakit memang punya cara menjawab panggilan
    /// baliknya.
    pub fn daftar_fungsi(&self, f: Value) -> Option<usize> {
        if !(self.fungsi_boleh_get)() { return None; }
        let mut r = (self.registri)().lock().ok()?;
        let n = r.berikut;
        r.berikut += 1;
        r.isi.insert(n, f);
        Some(n)
    }

    /// Pastikan sebuah handle masih milik sesi worker yang sekarang.
    pub fn periksa_generasi(&self, gen: u64, fname: &str) -> Result<(), Value> {
        let kini = self.generasi_kini();
        if gen == kini { return Ok(()); }
        Err(err_value(format!(
            "{}: handle {} ini dari sesi worker sebelumnya (sesi #{}, sekarang #{}) — \
             worker {} sempat berhenti lalu dijalankan ulang, dan objeknya ikut hilang \
             bersama prosesnya. Bangun ulang objeknya di sesi yang sekarang",
            fname, self.nama, gen, kini, self.nama)))
    }
}

// ---------------------------------------------------------------------------
// Skrip runner
// ---------------------------------------------------------------------------

/// Tulis runner yang ditanam ke temp dir dan kembalikan path-nya, **atomik**.
fn path_skrip(rt: &Runtime) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir();
    let path = dir.join(rt.skrip_nama);

    static URUT: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".{}.{}.{}.tmp", rt.skrip_nama, std::process::id(),
        URUT.fetch_add(1, Ordering::Relaxed)));

    std::fs::write(&tmp, rt.skrip_src)?;
    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(path),
        // `rename` gagal (mis. temp dir lintas filesystem). Berkas sementaranya
        // lengkap dan bernama unik, jadi memakainya apa adanya tetap benar.
        Err(_) => Ok(tmp),
    }
}

fn mulai_worker(rt: &Runtime) -> Result<Worker, String> {
    let script = path_skrip(rt).map_err(|e| format!(
        "tidak bisa menulis runner {} di temp dir: {}", rt.nama, e))?;

    let mut cmd = Command::new(rt.bin);
    for a in rt.bin_args { cmd.arg(a); }
    let mut child = cmd
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // stderr diwariskan: `print`/`console.log`/traceback milik pemakai
        // tetap terlihat di terminal, dan TIDAK mengotori pipa protokol.
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("tidak bisa menjalankan '{}': {} ({})",
                             rt.bin, e, rt.bantuan_pasang))?;

    let stdin = child.stdin.take().ok_or("tidak bisa membuka stdin worker")?;
    let stdout = child.stdout.take().ok_or("tidak bisa membuka stdout worker")?;
    Ok(Worker { child, stdin, stdout: BufReader::new(stdout) })
}

// ---------------------------------------------------------------------------
// Pertukaran
// ---------------------------------------------------------------------------

/// Worker ini dinyatakan MATI: nomor sesi naik SEKARANG, jadi handle lamanya
/// ditolak dengan kalimat sesi sejak panggilan pertama sesudahnya; antrean
/// pelepasan dan registri fungsinya milik proses lama dan ikut dibuang.
fn sesi_berakhir(rt: &Runtime, guard: &mut Option<Worker>) {
    *guard = None;
    rt.generasi.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut q) = (rt.antrean)().lock() { q.clear(); }
    if let Ok(mut r) = (rt.registri)().lock() { r.isi.clear(); }
}

/// Tunggu worker yang sedang berhenti, paling lama dua detik (lalu dibunuh), dan
/// jawab cara ia berhenti — `" (kode keluar 255)"` — atau `""` kalau tidak tahu.
fn tunggu_selesai(child: &mut Child) -> String {
    for _ in 0..400 {
        match child.try_wait() {
            Ok(Some(st)) => return uraikan_status(st),
            Ok(None) => crate::host::tidur(std::time::Duration::from_millis(5)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    String::new()
}

fn uraikan_status(st: std::process::ExitStatus) -> String {
    if let Some(k) = st.code() { return format!(" (kode keluar {})", k); }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(s) = st.signal() { return format!(" (dihentikan sinyal {})", s); }
    }
    String::new()
}

/// Sisipkan `"lepas":[...]` ke sebuah objek JSON yang sudah jadi.
fn sisipkan_lepas(req_json: &str, ids: &[String]) -> String {
    let sisip = format!("\"lepas\":[{}],", ids.join(","));
    match req_json.strip_prefix('{') {
        Some(sisa) => format!("{{{}{}", sisip, sisa),
        None => req_json.to_string(),
    }
}

/// Jalankan permintaan yang BOLEH membawa fungsi JRG sebagai argumen.
pub fn jalankan_dgn_fungsi(
    rt: &'static Runtime,
    bangun: impl FnOnce(&mut String) -> Result<(), String>,
    panggil: Panggil<'_>,
) -> Value {
    (rt.fungsi_boleh_set)(true);
    let mut req = String::new();
    let hasil = bangun(&mut req);
    (rt.fungsi_boleh_set)(false);
    if let Err(e) = hasil { return err_value(e); }
    kirim(rt, &req, Some(panggil))
}

pub fn kirim_tanpa_fungsi(rt: &'static Runtime, req_json: &str) -> Value {
    kirim(rt, req_json, None)
}

/// Apakah baris ini permintaan memanggil fungsi JRG? Kalau ya, nomornya,
/// nomor urut pertukarannya, dan argumennya.
pub fn permintaan_callback(rt: &'static Runtime, baris: &str)
    -> Option<(usize, i64, Vec<Value>)>
{
    let Ok(Json::Obj(pairs)) = parse_json(baris) else { return None };
    let mut n: Option<usize> = None;
    let mut seq: i64 = 0;
    let mut args: Vec<Value> = Vec::new();
    for (k, v) in pairs {
        match (k.as_str(), v) {
            ("cb", Json::Int(i)) if i >= 0 => n = Some(i as usize),
            ("seq", Json::Int(i)) => seq = i as i64,
            ("args", Json::Arr(items)) => args = items.into_iter().map(|v| json_to_value(rt, v)).collect(),
            _ => {}
        }
    }
    n.map(|n| (n, seq, args))
}

/// Kirim satu permintaan, layani panggilan balik selama menunggu, kembalikan
/// jawabannya sebagai `Result` JRG.
pub fn kirim(rt: &'static Runtime, req_json: &str, mut panggil: Option<Panggil<'_>>) -> Value {
    if (rt.dalam_cb_get)() {
        return err_value(format!(
            "panggilan `{}::` dari dalam callback {} tidak bisa dilayani — worker {} \
             sedang berhenti menunggu hasil callback ini, jadi ia tidak akan menjawab \
             permintaan baru.\n  \
             bantuan: hitung dulu yang butuh {} SEBELUM memanggil API-nya, lalu serahkan \
             hasilnya lewat argumen callback atau lewat nilai yang ditangkap closure-nya",
            rt.ns, rt.nama, rt.nama, rt.nama));
    }

    let mtx = (rt.slot)();
    let mut guard = match mtx.lock() { Ok(g) => g, Err(p) => p.into_inner() };

    let mati = match guard.as_mut() {
        Some(w) => matches!(w.child.try_wait(), Ok(Some(_)) | Err(_)),
        None => false,
    };
    if mati { sesi_berakhir(rt, &mut guard); }
    if guard.is_none() {
        match mulai_worker(rt) {
            Ok(w) => *guard = Some(w),
            Err(e) => return err_value(e),
        }
    }
    let worker = guard.as_mut().expect("worker baru saja diisi di atas");

    // Titipkan id yang menunggu dilepas — satu perjalanan pipa, bukan dua.
    let req_json = match (rt.antrean)().lock() {
        Ok(mut q) if !q.is_empty() => {
            let ids: Vec<String> = q.drain(..).map(|i| i.to_string()).collect();
            sisipkan_lepas(req_json, &ids)
        }
        _ => req_json.to_string(),
    };

    if writeln!(worker.stdin, "{}", req_json).is_err() || worker.stdin.flush().is_err() {
        sesi_berakhir(rt, &mut guard);
        return err_value(format!("gagal kirim permintaan ke worker {}", rt.nama));
    }

    loop {
        let mut line = String::new();
        match worker.stdout.read_line(&mut line) {
            Ok(0) => {
                let status = tunggu_selesai(&mut worker.child);
                sesi_berakhir(rt, &mut guard);
                return err_value(format!(
                    "worker {} berhenti tanpa menjawab{} — pesan galatnya, kalau ada, tercetak \
                     di stderr di atas; panggilan berikutnya memulai worker baru",
                    rt.nama, status));
            }
            Err(e) => {
                sesi_berakhir(rt, &mut guard);
                return err_value(format!("gagal baca respons worker {}: {}", rt.nama, e));
            }
            Ok(_) => {}
        }
        // Baris protokol selalu diawali `{"`. Yang lain adalah tulisan MENTAH kode
        // pemakai ke fd 1 (`os.write(1, ..)`): diteruskan ke stderr, dan jawaban
        // sungguhannya tetap ditunggu — dulu tiap jawaban sesudahnya bergeser satu.
        let baris = line.trim_end();
        let baris = match baris.find("{\"") {
            Some(0) => baris,
            Some(i) => { crate::host::galat_baris(&baris[..i]); &baris[i..] }
            None => { crate::host::galat_baris(baris); continue; }
        };
        let Some((n, seq, args)) = permintaan_callback(rt, baris) else {
            // Jawaban TERAKHIR sebuah worker yang sedang berhenti (`exit()`, fatal):
            // tunggu prosesnya, supaya permintaan berikutnya tidak terkirim ke sana.
            if baris.starts_with("{\"berhenti\":true") {
                tunggu_selesai(&mut worker.child);
                let hasil = response_to_value(rt, baris);
                sesi_berakhir(rt, &mut guard);
                return hasil;
            }
            return response_to_value(rt, baris);
        };

        let jawab = match panggil.as_mut() {
            Some(p) => match (rt.registri)().lock().ok().and_then(|r| r.isi.get(&n).cloned()) {
                Some(f) => {
                    (rt.dalam_cb_set)(true);
                    let hasil = p(&f, args);
                    (rt.dalam_cb_set)(false);
                    hasil
                }
                None => Err(format!("worker meminta fungsi #{} yang tidak pernah dikirim atau sudah dibebaskan", n)),
            },
            None => Err("permintaan callback datang untuk permintaan yang tidak membawa fungsi"
                .to_string()),
        };

        // Nomor urutnya DIKEMBALIKAN: sisi worker memeriksanya, jadi jawaban
        // yang nyasar ke pertukaran lain ketahuan sebagai kesalahan protokol
        // alih-alih dipakai sebagai hasil yang salah.
        let mut balas = format!("{{\"seq\":{},", seq);
        match jawab {
            Ok(v) => {
                balas.push_str("\"ok\":true,\"value\":");
                if let Err(e) = value_to_json(rt, &v, &mut balas) {
                    balas = format!("{{\"seq\":{},\"ok\":false,\"error\":", seq);
                    escape_json_string(&format!("hasil callback {}", e), &mut balas);
                    balas.push('}');
                } else {
                    balas.push('}');
                }
            }
            Err(e) => {
                balas.push_str("\"ok\":false,\"error\":");
                escape_json_string(&e, &mut balas);
                balas.push('}');
            }
        }
        if writeln!(worker.stdin, "{}", balas).is_err() || worker.stdin.flush().is_err() {
            sesi_berakhir(rt, &mut guard);
            return err_value(format!("gagal mengirim hasil callback ke worker {}", rt.nama));
        }
    }
}

// -------------------------------------------------------------------------
// JSON: perakitan dan pembacaan
// -------------------------------------------------------------------------

/// "argumen ke-N" menghitung argumen yang DITERUSKAN ke pekerja, bukan posisi
/// di pemanggilan JRG (`js::call(modul, fungsi, ...)`), jadi kalimatnya menyebut itu.
pub fn argumen_diteruskan(i: usize, ke: &str) -> String {
    format!("argumen ke-{} yang diteruskan ke {}", i + 1, ke)
}

pub fn escape_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn value_to_json(rt: &'static Runtime, v: &Value, out: &mut String) -> Result<(), String> {
    // Handle milik bahasa ini ditangani DI SINI, sekali, sebelum `match`.
    if let Some(hasil) = (rt.tulis_handle)(v, out) { return hasil; }

    match v {
        Value::Int(n) => out.push_str(&n.to_string()),
        // `{:?}`, BUKAN `to_string()`.
        Value::Float(f) => {
            if f.is_finite() {
                out.push_str(&format!("{:?}", f));
            } else if f.is_nan() {
                // `json` milik Python menerima ketiga token ini secara bawaan.
                out.push_str("NaN");
            } else if *f > 0.0 {
                out.push_str("Infinity");
            } else {
                out.push_str("-Infinity");
            }
        }
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Rune(c) => escape_json_string(&c.to_string(), out),
        Value::Str(s) => escape_json_string(s, out),
        Value::Unit | Value::Nil => out.push_str("null"),
        Value::Array(a) | Value::Tuple(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                value_to_json(rt, item, out)?;
            }
            out.push(']');
        }
        Value::Struct { fields, .. } => {
            // Kuncinya DIURUTKAN, seperti di setiap pintu lain (tampilan sejak
            // Fase 5F-B, `json::` sejak Fase JEMBATAN). Tanpa ini urutan
            // `HashMap` yang Rust acak per proses bocor ke program lain.
            let mut kunci: Vec<&String> = fields.keys().collect();
            kunci.sort();
            out.push('{');
            for (i, k) in kunci.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                escape_json_string(k, out);
                out.push(':');
                value_to_json(rt, &fields[*k], out)?;
            }
            out.push('}');
        }
        // Bentuk yang sama dengan `json::stringify` (Fase JSON-ENUM): `"None"`,
        // `{"Some":3}`. Dulu `{"__jrg_enum__":..,"variant":..,"payload":..}` —
        // nama representasi di program lain, dan kembali sebagai `dict`.
        Value::Enum { variant, payload, .. } => match payload {
            None => escape_json_string(variant, out),
            Some(p) => {
                out.push('{');
                escape_json_string(variant, out);
                out.push(':');
                value_to_json(rt, p, out)?;
                out.push('}');
            }
        },
        // Handle dikirim balik sebagai NOMORNYA, dan worker menukarnya dengan objek
        // aslinya.
        v if v.bisa_dipanggil() => {
            match rt.daftar_fungsi(v.clone()) {
                Some(n) => out.push_str(&format!("{{\"__jrgfn__\":{}}}", n)),
                None => return Err(format!(
                    "berupa fungsi, dan fungsi cuma bisa dikirim lewat `{ns}::call` atau \
                     `{ns}::method` — bukan lewat `{ns}::eval`/`{ns}::exec`, yang tidak \
                     punya tempat menampung panggilan baliknya", ns = rt.ns)
                ),
            }
        }
        // TIPE-nya, bukan isinya — dan nama pekerjanya, bukan selalu "Python".
        other => return Err(format!("bertipe `{}` tidak bisa dikirim ke pekerja `{}::`",
                                    other.type_name(), rt.ns)),
    }
    Ok(())
}

/// Salinan nilai fungsi, untuk didaftarkan di tabel callback.
#[derive(Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        JsonParser { bytes: s.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect_lit(&mut self, lit: &str) -> Result<(), String> {
        let end = self.pos + lit.len();
        if end <= self.bytes.len() && &self.bytes[self.pos..end] == lit.as_bytes() {
            self.pos = end;
            Ok(())
        } else {
            Err(format!("JSON: diharapkan literal '{}'", lit))
        }
    }

    fn parse_value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'"') => self.parse_string().map(Json::Str),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => {
                self.expect_lit("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.expect_lit("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.expect_lit("null")?;
                Ok(Json::Null)
            }
            // `NaN` / `Infinity` / `-Infinity`.
            Some(b'N') => {
                self.expect_lit("NaN")?;
                Ok(Json::Float(f64::NAN))
            }
            Some(b'I') => {
                self.expect_lit("Infinity")?;
                Ok(Json::Float(f64::INFINITY))
            }
            Some(b'-') if self.bytes.get(self.pos + 1) == Some(&b'I') => {
                self.expect_lit("-Infinity")?;
                Ok(Json::Float(f64::NEG_INFINITY))
            }
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            Some(c) => Err(format!("JSON: karakter tak terduga '{}'", c as char)),
            None => Err("JSON: input berakhir tiba-tiba".to_string()),
        }
    }

    fn parse_number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        let mut is_float = false;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(b) = self.peek() {
            match b {
                b'0'..=b'9' => self.pos += 1,
                b'.' => {
                    is_float = true;
                    self.pos += 1;
                }
                b'e' | b'E' => {
                    is_float = true;
                    self.pos += 1;
                    if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "JSON: angka bukan UTF-8 valid".to_string())?;
        if is_float {
            text.parse::<f64>()
                .map(Json::Float)
                .map_err(|e| format!("JSON: angka invalid '{}': {}", text, e))
        } else {
            // Integer Python tidak terbatas lebarnya; `int` JRG 128 bit.
            text.parse::<i128>().map(Json::Int).map_err(|_| {
                if text.parse::<f64>().is_ok() {
                    format!(
                        "{}bilangan bulat dari Python terlalu besar untuk `int` JRG (128 bit): {}\n  \
                         bantuan: minta Python mengubahnya lebih dulu — `float(x)` kalau perkiraan \
                         cukup, atau `str(x)` kalau angkanya harus utuh",
                        NILAI_TAK_MUAT, text
                    )
                } else {
                    format!("JSON: angka invalid '{}'", text)
                }
            })
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, String> {
        if self.pos + 4 > self.bytes.len() {
            return Err("JSON: \\u butuh 4 digit hex".to_string());
        }
        let text = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| "JSON: \\u bukan UTF-8 valid".to_string())?;
        let v = u32::from_str_radix(text, 16).map_err(|_| "JSON: \\u bukan hex valid".to_string())?;
        self.pos += 4;
        Ok(v)
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1; // buka '"'
        let mut s = String::new();
        loop {
            let start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' {
                    break;
                }
                self.pos += 1;
            }
            // Rentang start..pos hanya berisi byte UTF-8 polos (tanpa '"'/'\\' di
            // dalamnya) — karena kedua delimiter itu ASCII satu-byte, potongan ini tidak …
            s.push_str(
                std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| "JSON: string bukan UTF-8 valid".to_string())?,
            );
            match self.bump() {
                None => return Err("JSON: string tidak ditutup".to_string()),
                Some(b'"') => return Ok(s),
                Some(b'\\') => match self.bump() {
                    Some(b'"') => s.push('"'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'/') => s.push('/'),
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(b'r') => s.push('\r'),
                    Some(b'b') => s.push('\u{8}'),
                    Some(b'f') => s.push('\u{c}'),
                    Some(b'u') => {
                        let cp1 = self.parse_hex4()?;
                        if (0xD800..=0xDBFF).contains(&cp1) {
                            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                return Err("JSON: surrogate pair tidak lengkap".to_string());
                            }
                            let cp2 = self.parse_hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&cp2) {
                                return Err("JSON: low surrogate invalid".to_string());
                            }
                            let c = 0x10000 + ((cp1 - 0xD800) << 10) + (cp2 - 0xDC00);
                            if let Some(ch) = char::from_u32(c) {
                                s.push(ch);
                            }
                        } else if let Some(ch) = char::from_u32(cp1) {
                            s.push(ch);
                        }
                    }
                    _ => return Err("JSON: escape tidak valid".to_string()),
                },
                Some(_) => unreachable!(),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Json, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.bump() {
                Some(b',') => self.skip_ws(),
                Some(b']') => return Ok(Json::Arr(items)),
                _ => return Err("JSON: array: diharapkan ',' atau ']'".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<Json, String> {
        self.pos += 1; // '{'
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err("JSON: object: key harus string".to_string());
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.bump() != Some(b':') {
                return Err("JSON: object: diharapkan ':'".to_string());
            }
            let val = self.parse_value()?;
            pairs.push((key, val));
            self.skip_ws();
            match self.bump() {
                Some(b',') => {}
                Some(b'}') => return Ok(Json::Obj(pairs)),
                _ => return Err("JSON: object: diharapkan ',' atau '}'".to_string()),
            }
        }
    }
}

/// Penanda untuk galat yang BUKAN kegagalan protokol: barisnya sampai utuh
/// dan terbaca, cuma ada satu nilai di dalamnya yang tidak muat di JRG.
pub(crate) const NILAI_TAK_MUAT: &str = "\u{1}nilai-tak-muat\u{1}";

pub fn parse_json(s: &str) -> Result<Json, String> {
    let mut p = JsonParser::new(s);
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err("JSON: ada karakter tambahan setelah value".to_string());
    }
    Ok(v)
}

/// Maps a parsed `Json` value back into a JRG `Value`, following the same
/// conventions `py_runner.py`'s `_to_jrg` uses on the way out of Python: …
pub fn json_to_value(rt: &'static Runtime, j: Json) -> Value {
    match j {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Bool(b),
        Json::Int(n) => Value::Int(n),
        Json::Float(f) => Value::Float(f),
        Json::Str(s) => Value::Str(s),
        Json::Arr(items) => Value::Array(items.into_iter().map(|v| json_to_value(rt, v)).collect()),
        Json::Obj(pairs) => {
            if pairs.len() == 1 && pairs[0].0 == "__pyrepr__" {
                if let Json::Str(s) = &pairs[0].1 {
                    return Value::Str(s.clone());
                }
            }
            // Handle yang dikirim INLINE, bukan di tingkat teratas respons.
            if pairs.iter().any(|(k, _)| k == "__pyref__") {
                let mut id = None;
                let mut tipe = String::from("object");
                for (k, v) in &pairs {
                    match (k.as_str(), v) {
                        ("__pyref__", Json::Int(n)) if *n >= 0 => id = Some(*n as u64),
                        ("__pytipe__", Json::Str(t)) => tipe = t.clone(),
                        _ => {}
                    }
                }
                if let Some(id) = id {
                    // Handle dibuat lewat `rt`, bukan langsung: yang datang dari worker
                    // JavaScript harus jadi `Value::JsObj`, dan membuat `PyObj` di sini akan …
                    let terbungkus = (rt.buat_handle)(id, rt.generasi_kini(), tipe);
                    return match terbungkus {
                        Value::Enum { payload: Some(p), .. } => *p,
                        lain => lain,
                    };
                }
            }
            let fields = pairs.into_iter().map(|(k, v)| (k, json_to_value(rt, v))).collect();
            Value::Struct { name: "dict".to_string(), fields }
        }
    }
}

/// Parses one line of the worker's `{"ok":.., "value":.., "error":..}`
/// response protocol into the `Result`-shaped `Value` the JRG side
/// expects (see `ok_value`/`err_value` in `crate::interpreter`).

pub fn response_to_value(rt: &'static Runtime, line: &str) -> Value {
    let parsed = match parse_json(line) {
        Ok(j) => j,
        Err(e) => {
            return match e.strip_prefix(NILAI_TAK_MUAT) {
                Some(pesan) => err_value(pesan.to_string()),
                None => err_value(format!(
                    "respons worker {} tidak valid: {} (mentah: {})",
                    rt.nama, e, line
                )),
            }
        }
    };
    let pairs = match parsed {
        Json::Obj(pairs) => pairs,
        _ => return err_value(format!("respons worker {} bukan JSON object", rt.nama)),
    };

    let mut ok: Option<bool> = None;
    let mut value = Json::Null;
    let mut error: Option<String> = None;
    let mut r#ref: Option<u64> = None;
    let mut tipe = String::from("object");
    for (k, v) in pairs {
        match k.as_str() {
            "ok" => ok = if let Json::Bool(b) = v { Some(b) } else { None },
            "value" => value = v,
            "error" => error = if let Json::Str(s) = v { Some(s) } else { None },
            // Worker menaruh objeknya di tabelnya sendiri dan mengirim
            // nomornya. Ini SATU-SATUNYA tempat handle lahir.
            "ref" => r#ref = match v { Json::Int(n) if n >= 0 => Some(n as u64), _ => None },
            "tipe" => if let Json::Str(s) = v { tipe = s },
            // Pembungkus fungsi JRG yang sudah mati di sisi worker.
            "fn_lepas" => if let (Json::Arr(ids), Ok(mut r)) = (v, (rt.registri)().lock()) {
                for id in ids { if let Json::Int(n) = id { r.isi.remove(&(n as usize)); } }
            },
            _ => {}
        }
    }

    match ok {
        Some(true) => match r#ref {
            Some(id) => (rt.buat_handle)(id, rt.generasi_kini(), tipe),
            None => ok_value(json_to_value(rt, value)),
        },
        Some(false) => err_value(error.unwrap_or_else(|| format!("error tidak diketahui dari {}", rt.nama))),
        None => err_value(format!("respons worker {} tidak punya field 'ok'", rt.nama)),
    }
}

