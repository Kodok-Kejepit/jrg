// src/interpreter/builtin/str_ops.rs — interpreter/builtin/str_ops.rs
// Dispatch untuk namespace `str::` (format/pad/case/split/join/slice,

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use crate::interpreter::eval_expr::{nilai_ke_usize, nilai_ke_i128};
use crate::runtime_err;
use super::NativeFnKind;

/// Argumen yang HARUS berupa teks, dengan PERANNYA disebut. `rune` diterima
/// karena ia memang satu karakter; sisanya ditolak alih-alih di-Display.
pub(crate) fn teks_peran(v: &Value, siapa: &str, peran: &str) -> Result<String, Signal> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Rune(c) => Ok(c.to_string()),
        lain => Err(Signal::RuntimeError(RuntimeError::new(
            crate::interpreter::pesan_arg_harus_teks(siapa, peran, &lain.type_name())))),
    }
}

/// Argumen PERTAMA sebuah `str::`, yang harus berupa teks.
pub(crate) fn teks(v: &Value, siapa: &str) -> Result<String, Signal> {
    teks_peran(v, siapa, "argumen pertama")
}

/// `arr::join`/`str::join` — SATU definisi untuk dua nama dan dua mesin. Pemisahnya
/// harus teks: dulu `arr::join(a, nil)` menjawab `1nil2` di interpreter dan `12`
/// di `--vm`, dan kalimat `str::join` menyebut pemisahnya "argumen pertama".
pub(crate) fn gabung(nama: &str, args: &[Value]) -> EvalResult {
    if args.len() < 2 { return runtime_err!("{} butuh (arr, sep)", nama); }
    let arr = match &args[0] {
        Value::Array(a) => a,
        o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(nama, 1, &o.type_name())),
    };
    let sep = teks_peran(&args[1], nama, "pemisahnya")?;
    Ok(Value::Str(arr.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(&sep)))
}

/// Argumen pertama dari daftar argumen, kosong dibaca sebagai `nil` seperti dulu.
fn teks1(args: &[Value], siapa: &str) -> Result<String, Signal> {
    teks(args.first().unwrap_or(&Value::Nil), siapa)
}

impl Interpreter {
    pub(super) fn dispatch_str(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind,
            StrFormat | StrPadLeft | StrPadRight | StrCenter | StrRepeat |
            StrWords | StrLines | StrIsNumeric | StrIsAlpha | StrIsAlphaNum |
            StrTitle | StrSnakeCase | StrCamelCase | StrReverse | StrCount |
            StrStartsWith | StrEndsWith | StrContains | StrSplit | StrJoin |
            StrTrim | StrUpper | StrLower | StrReplace | StrSlice | StrCharAt |
            StrBytes | StrChars | StrFind |
            StrCapitalize | StrIsEmpty | StrStripPrefix | StrStripSuffix | StrSubstr |
            StrIsUpper | StrIsLower | StrSwapCase
        ) {
            return None;
        }
        Some(Self::dispatch_str_inner(kind, args))
    }

    /// Isi pelebar: TEPAT satu karakter. Dulu `"xy"` diam-diam jadi `x` dan `""` jadi spasi.
    fn isi_pad(nama: &str, args: &[Value]) -> Result<char, Signal> {
        let Some(v) = args.get(2) else { return Ok(' ') };
        let t = format!("{}", v);
        let mut cs = t.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(Signal::RuntimeError(RuntimeError::new(format!(
                "{}: isinya harus tepat satu karakter, dapat {} karakter\n  \
                 bantuan: untuk pola berulang, bangun dulu dengan `str::repeat`, lalu potong \
                 dengan `str::slice`", nama, t.chars().count())))),
        }
    }

    /// Bebas `self`, dan itu yang membuat VM bisa memanggil yang SAMA.
    pub fn dispatch_str_inner(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        match kind {
            StrFormat => Self::dispatch_str_inner_lepas(&StrFormat, args),
            StrPadLeft => {
                if args.len() < 2 {
                    // `< 2`, bukan `is_empty()`.
                    return runtime_err!(
                        "str::pad_left butuh (s, lebar[, isi]) — mau dilebarkan sampai berapa?");
                }
                let s  = format!("{}", args[0]);
                let w  = if args.len() > 1 { nilai_ke_usize(&args[1])? } else { 0 };
                let pc = Self::isi_pad("str::pad_left", args)?;
                let n  = s.chars().count();
                if n >= w { return Ok(Value::Str(s)); }
                // Lebarnya menentukan BESAR ALOKASI, jadi batasnya diperiksa sebelum
                // alokasinya.
                crate::interpreter::cek_batas_elemen("str::pad", w as i128)
                    .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                let pad: String = std::iter::repeat(pc).take(w - n).collect();
                Ok(Value::Str(format!("{}{}", pad, s)))
            }
            StrPadRight => {
                if args.len() < 2 {
                    // `< 2`, bukan `is_empty()`.
                    return runtime_err!(
                        "str::pad_right butuh (s, lebar[, isi]) — mau dilebarkan sampai berapa?");
                }
                let s  = format!("{}", args[0]);
                let w  = if args.len() > 1 { nilai_ke_usize(&args[1])? } else { 0 };
                let pc = Self::isi_pad("str::pad_right", args)?;
                let n  = s.chars().count();
                if n >= w { return Ok(Value::Str(s)); }
                // Lebarnya menentukan BESAR ALOKASI, jadi batasnya diperiksa sebelum
                // alokasinya.
                crate::interpreter::cek_batas_elemen("str::pad", w as i128)
                    .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                let pad: String = std::iter::repeat(pc).take(w - n).collect();
                Ok(Value::Str(format!("{}{}", s, pad)))
            }
            StrCenter => {
                if args.len() < 2 {
                    // `< 2`, bukan `is_empty()`.
                    return runtime_err!(
                        "str::center butuh (s, lebar[, isi]) — mau dilebarkan sampai berapa?");
                }
                let s  = format!("{}", args[0]);
                let w  = if args.len() > 1 { nilai_ke_usize(&args[1])? } else { 0 };
                let pc = Self::isi_pad("str::center", args)?;
                let n  = s.chars().count();
                if n >= w { return Ok(Value::Str(s)); }
                // Lebarnya menentukan BESAR ALOKASI, jadi batasnya diperiksa sebelum
                // alokasinya.
                crate::interpreter::cek_batas_elemen("str::pad", w as i128)
                    .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                let total = w - n; let lp = total / 2; let rp = total - lp;
                let lpad: String = std::iter::repeat(pc).take(lp).collect();
                let rpad: String = std::iter::repeat(pc).take(rp).collect();
                Ok(Value::Str(format!("{}{}{}", lpad, s, rpad)))
            }
            // `Interpreter::v_repeat_str` yang sama dengan operator `*` (`"ab" * n`) —
            // dulu di sini `s.repeat(n)` dipanggil langsung tanpa batas apa pun, jadi …
            StrRepeat => {
                if args.len() < 2 { return runtime_err!("str::repeat butuh (s, n)"); }
                let s = teks(&args[0], "str::repeat")?;
                let n = nilai_ke_i128(&args[1])?;
                Interpreter::v_repeat_str(&s, n)
            }
            StrWords     => { let s = teks1(args, "str::words")?; Ok(Value::Array(s.split_whitespace().map(|w| Value::Str(w.to_string())).collect())) }
            StrLines     => { let s = teks1(args, "str::lines")?; Ok(Value::Array(s.lines().map(|l| Value::Str(l.to_string())).collect())) }
            // Yang diperiksa kosong adalah hasil TRIM-nya: `" "` men-trim jadi
            // kosong, dan `"".chars().all(..)` selalu true — jadi spasi dulu
            // dijawab "angka", sementara `is_alpha`/`is_alphanumeric` menolaknya.
            StrIsNumeric => {
                // Satu angka utuh (`-3.5`, ` 42 `), bukan "tiap karakter digit, `.` atau `-`" —
                // yang dulu menjawab true untuk `-`, `.` dan `1.2.3`.
                let s = teks1(args, "str::is_numeric")?;
                let t = s.trim();
                let t = t.strip_prefix('-').unwrap_or(t);
                let (bulat, pecahan) = t.split_once('.').unwrap_or((t, ""));
                let digit = |x: &str| x.chars().all(|c| c.is_ascii_digit());
                Ok(Value::Bool(digit(bulat) && digit(pecahan) && !(bulat.is_empty() && pecahan.is_empty())))
            }
            StrIsAlpha   => { let s = teks1(args, "str::is_alpha")?; Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphabetic()))) }
            StrIsAlphaNum => { let s = teks1(args, "str::is_alphanumeric")?; Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphanumeric()))) }
            StrTitle => {
                let s = teks1(args, "str::title")?;
                let r: String = s.split_whitespace().map(|w| {
                    let mut ch = w.chars();
                    match ch.next() { None => String::new(), Some(c) => c.to_uppercase().to_string() + ch.as_str() }
                }).collect::<Vec<_>>().join(" ");
                Ok(Value::Str(r))
            }
            StrSnakeCase => {
                let s = teks1(args, "str::snake_case")?; let mut r = String::new();
                for (i, c) in s.chars().enumerate() {
                    if c.is_uppercase() && i > 0 { r.push('_'); }
                    r.push(c.to_lowercase().next().unwrap_or(c));
                }
                Ok(Value::Str(r.replace(' ', "_")))
            }
            StrCamelCase => {
                let s = teks1(args, "str::camel_case")?;
                let words: Vec<&str> = s.split(|c: char| c == '_' || c == ' ').collect();
                let r: String = words.iter().enumerate().map(|(i, w)| {
                    if i == 0 { w.to_lowercase() } else {
                        let mut ch = w.chars();
                        match ch.next() { None => String::new(), Some(c) => c.to_uppercase().to_string() + &ch.as_str().to_lowercase() }
                    }
                }).collect();
                Ok(Value::Str(r))
            }
            StrReverse   => { let s = teks1(args, "str::reverse")?; Ok(Value::Str(s.chars().rev().collect())) }
            StrCount => {
                if args.len() < 2 { return runtime_err!("str::count butuh (s, sub)"); }
                let s = teks(&args[0], "str::count")?; let sub = teks_peran(&args[1], "str::count", "yang dihitung")?;
                Ok(Value::Int(s.matches(&sub as &str).count() as i128))
            }
            StrStartsWith => {
                if args.len() < 2 { return runtime_err!("str::starts_with butuh (s, prefix)"); }
                let s = teks(&args[0], "str::starts_with")?; let p = teks_peran(&args[1], "str::starts_with", "awalan yang dicocokkan")?;
                Ok(Value::Bool(s.starts_with(&p as &str)))
            }
            StrEndsWith => {
                if args.len() < 2 { return runtime_err!("str::ends_with butuh (s, suffix)"); }
                let s = teks(&args[0], "str::ends_with")?; let p = teks_peran(&args[1], "str::ends_with", "akhiran yang dicocokkan")?;
                Ok(Value::Bool(s.ends_with(&p as &str)))
            }
            StrContains => {
                if args.len() < 2 { return runtime_err!("str::contains butuh (s, sub)"); }
                let s = teks(&args[0], "str::contains")?; let p = teks_peran(&args[1], "str::contains", "yang dicari")?;
                Ok(Value::Bool(s.contains(&p as &str)))
            }
            StrSplit => {
                if args.len() < 2 { return runtime_err!("str::split butuh (s, sep)"); }
                let s = teks(&args[0], "str::split")?; let sep = teks_peran(&args[1], "str::split", "pemisahnya")?;
                Ok(Value::Array(s.split(&sep as &str).map(|p| Value::Str(p.to_string())).collect()))
            }
            StrJoin => gabung("str::join", args),
            StrTrim    => { let s = teks1(args, "str::trim")?; Ok(Value::Str(s.trim().to_string()))    }
            StrUpper   => { let s = teks1(args, "str::upper")?; Ok(Value::Str(s.to_uppercase()))         }
            StrLower   => { let s = teks1(args, "str::lower")?; Ok(Value::Str(s.to_lowercase()))         }
            StrReplace => {
                if args.len() < 3 { return runtime_err!("str::replace butuh (s, from, to)"); }
                let s    = teks(&args[0], "str::replace")?;
                let from = teks_peran(&args[1], "str::replace", "yang diganti")?;
                let to   = format!("{}", args[2]);
                Ok(Value::Str(s.replace(&from as &str, &to)))
            }
            // Sama persis dengan `s[awal:akhir]` — dilempar ke `eval_slice` yang sama,
            // bukan diimplementasikan ulang di sini.
            StrSlice => {
                Self::dispatch_str_inner_lepas(&StrSlice, args)
            }
            StrCharAt => {
                if args.len() < 2 { return runtime_err!("str::char_at butuh (s, idx)"); }
                let s = teks(&args[0], "str::char_at")?;
                let i = crate::interpreter::eval_expr::indeks_karakter(&args[1], &s)?;
                Ok(Value::Rune(s.chars().nth(i).unwrap_or_default()))
            }
            StrBytes => { let s = teks1(args, "str::bytes")?; Ok(Value::Array(s.bytes().map(|b| Value::Int(b as i128)).collect())) }
            StrChars => { let s = teks1(args, "str::chars")?; Ok(Value::Array(s.chars().map(Value::Rune).collect())) }
            StrFind => {
                if args.len() < 2 { return runtime_err!("str::find butuh (s, sub)"); }
                let s = teks(&args[0], "str::find")?; let sub = teks_peran(&args[1], "str::find", "yang dicari")?;
                Ok(posisi_karakter(&s, &sub))
            }
            StrCapitalize => {
                let s = teks1(args, "str::capitalize")?;
                let mut c = s.chars();
                Ok(Value::Str(match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }))
            }
            StrIsEmpty => { let s = teks1(args, "str::is_empty")?; Ok(Value::Bool(s.is_empty())) }
            StrStripPrefix => {
                if args.len() < 2 { return runtime_err!("str::strip_prefix butuh (s, prefix)"); }
                let s = teks(&args[0], "str::strip_prefix")?; let p = teks_peran(&args[1], "str::strip_prefix", "awalannya")?;
                Ok(Value::Str(s.strip_prefix(&p as &str).unwrap_or(&s).to_string()))
            }
            StrStripSuffix => {
                if args.len() < 2 { return runtime_err!("str::strip_suffix butuh (s, suffix)"); }
                let s = teks(&args[0], "str::strip_suffix")?; let p = teks_peran(&args[1], "str::strip_suffix", "akhirannya")?;
                Ok(Value::Str(s.strip_suffix(&p as &str).unwrap_or(&s).to_string()))
            }
            StrSubstr => Self::dispatch_str_inner_lepas(&StrSubstr, args),
            StrIsUpper => { let s = teks1(args, "str::is_upper")?; Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()) && s.chars().any(|c| c.is_alphabetic()))) }
            StrIsLower => { let s = teks1(args, "str::is_lower")?; Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) && s.chars().any(|c| c.is_alphabetic()))) }
            StrSwapCase => {
                let s = teks1(args, "str::swap_case")?;
                Ok(Value::Str(s.chars().map(|c| {
                    if c.is_uppercase() { c.to_lowercase().next().unwrap_or(c) }
                    else if c.is_lowercase() { c.to_uppercase().next().unwrap_or(c) }
                    else { c }
                }).collect()))
            }
            // BUKAN `unreachable!`.
            lain => runtime_err!(
                "builtin `{:?}` dikirim ke dispatch string, padahal bukan miliknya — \
                 ini bug di jrgc, bukan di programmu", lain),
        }
    }
}

impl Interpreter {
    /// Arm `str::` yang aturannya dipakai VM juga.
    pub const MAKS_DESIMAL: usize = 30;

    /// Render satu nilai dengan `n` desimal tetap.
    pub fn desimal_tetap(n: usize, v: &Value) -> Result<String, Signal> {
        match v {
            Value::Float(f) => Ok(if f.is_finite() {
                format!("{:.*}", n, f)
            } else {
                // `inf`/`nan` tidak punya desimal. Dirender lewat jalur tampil
                // yang biasa supaya satu nilai tidak punya dua nama.
                format!("{}", v)
            }),
            Value::Int(i) => Ok(if n == 0 {
                format!("{}", i)
            } else {
                format!("{}.{}", i, "0".repeat(n))
            }),
            lain => runtime_err!(
                "str::format: spec `{{:.{}}}` cuma untuk angka, dapat `{}`",
                n, lain.type_name()),
        }
    }

    /// Berapa placeholder yang benar-benar akan disubstitusi.
    fn hitung_placeholder(template: &str) -> usize {
        SegmenFormat::pindai(template)
            .map(|segs| segs.iter().filter(|s| matches!(s, SegmenFormat::Nilai { .. })).count())
            .unwrap_or(0)
    }

    pub fn dispatch_str_inner_lepas(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        match kind {
            StrSlice => {
                if args.is_empty() { return runtime_err!("str::slice butuh >= 2 arg"); }
                // Pintunya `str::`, jadi penerimanya harus teks. `potong_nilai`
                // sendiri juga melayani array, dan tanpa penjaga ini
                // `str::slice([1, 2], 0, 1)` menjawab sebuah ARRAY.
                let s = teks(&args[0], "str::slice")?;
                Self::potong_nilai(Value::Str(s), args.get(1).cloned(), args.get(2).cloned())
            }
            // `str::format(template, ...)` — SATU definisi, dipakai kedua backend.
            StrFormat => {
                let Some(tv) = args.first() else {
                    return runtime_err!("str::format butuh minimal templatenya, satu string");
                };
                let template = teks_peran(tv, "str::format", "polanya")?;
                let segmen = match SegmenFormat::pindai(&template) {
                    Ok(s) => s,
                    Err(e) => return runtime_err!("{}", e),
                };
                let jml = segmen.iter()
                    .filter(|s| matches!(s, SegmenFormat::Nilai { .. })).count();
                let diberi = args.len() - 1;
                // Kedua arah diperiksa. Dulu cuma yang KURANG: argumen berlebih
                // dibuang diam-diam, jadi separuh kesalahan yang sama lewat
                // tanpa suara.
                if jml != diberi {
                    return runtime_err!("{}",
                        SegmenFormat::pesan_jumlah(jml, diberi, &template));
                }
                let mut out = String::with_capacity(template.len());
                let mut idx = 1usize;
                for seg in &segmen {
                    match seg {
                        SegmenFormat::Teks(t) => out.push_str(t),
                        SegmenFormat::Nilai { desimal } => {
                            let v = &args[idx];
                            idx += 1;
                            match desimal {
                                None => out.push_str(&format!("{}", v)),
                                Some(n) => out.push_str(&Self::desimal_tetap(*n, v)?),
                            }
                        }
                    }
                }
                Ok(Value::Str(out))
            }
            StrSubstr => {
                if args.len() < 2 { return runtime_err!("str::substr butuh (s, start[, panjang])"); }
                let chars: Vec<char> = teks(&args[0], "str::substr")?.chars().collect();
                let tulis = nilai_ke_i128(&args[1])?;
                // Awal negatif dihitung dari belakang, seperti `s[i]` (Fase SUBSTR); di luar
                // jangkauan gagal, tidak pernah dijepit ke ujung string.
                let Some(start) = crate::interpreter::normal_indeks(tulis, chars.len(), true) else {
                    return runtime_err!(
                        "str::substr: posisi awal {} di luar jangkauan — string ini cuma {} karakter",
                        tulis, chars.len()
                    );
                };
                // Argumen ketiga adalah PANJANG, bukan indeks akhir. Itu beda
                // dari `str::slice`, dan bedanya disengaja — keduanya memang
                // dua fungsi, bukan dua nama untuk satu fungsi.
                let len = if args.len() > 2 { nilai_ke_usize(&args[2])? } else { chars.len() - start };
                let end = (start + len).min(chars.len());
                Ok(Value::Str(chars[start..end].iter().collect()))
            }
            lain => unreachable!("{:?} bukan arm `dispatch_str_inner_lepas`", lain),
        }
    }
}


/// Posisi `sub` di dalam `s`, dihitung per KARAKTER — atau `-1`.
fn posisi_karakter(s: &str, sub: &str) -> Value {
    match s.find(sub) {
        Some(b) => crate::interpreter::some_value(
            Value::Int(s[..b].chars().count() as i128)),
        None => crate::interpreter::none_value(),
    }
}

/// Satu potongan dari sebuah template `str::format`.
#[derive(Debug, Clone, PartialEq)]
pub enum SegmenFormat {
    /// Teks apa adanya. Termasuk `{x}`, `{ }`, `{1,2}` dan `{` tanpa penutup —
    /// sebuah string memang boleh berisi kurung kurawal.
    Teks(String),
    /// Placeholder yang mengkonsumsi satu argumen.
    /// `desimal` kosong untuk `{}`, `Some(n)` untuk `{:.n}`.
    Nilai { desimal: Option<usize> },
}

impl SegmenFormat {
    /// Pindai template SEKALI dari kiri ke kanan.
    pub fn pindai(template: &str) -> Result<Vec<SegmenFormat>, String> {
        let mut keluar: Vec<SegmenFormat> = Vec::new();
        let mut teks = String::new();
        let mut sisa = template;
        while let Some(buka) = sisa.find('{') {
            let (depan, ekor) = sisa.split_at(buka);
            teks.push_str(depan);
            let Some(rel) = ekor.find('}') else {
                // `{` tanpa penutup: teks biasa sampai habis.
                teks.push_str(ekor);
                sisa = "";
                break;
            };
            let isi = &ekor[1..rel];
            if isi.is_empty() {
                if !teks.is_empty() { keluar.push(SegmenFormat::Teks(std::mem::take(&mut teks))); }
                keluar.push(SegmenFormat::Nilai { desimal: None });
            } else if isi.chars().all(|c| c.is_ascii_digit()) {
                // `{0}` DULU dikembalikan apa adanya di interpreter dan disubstitusi di VM —
                // satu program, dua jawaban, dua-duanya "berhasil".
                return Err(format!(
                    "str::format: `{{{}}}` tidak didukung — placeholder berposisi belum ada \
                     di JRG. Pakai `{{}}` sesuai urutan argumennya; kalau satu nilai perlu \
                     muncul dua kali, serahkan dua kali", isi));
            } else if let Some(spec) = isi.strip_prefix(':') {
                let n = Self::baca_desimal(spec)?;
                if !teks.is_empty() { keluar.push(SegmenFormat::Teks(std::mem::take(&mut teks))); }
                keluar.push(SegmenFormat::Nilai { desimal: Some(n) });
            } else {
                // `{x}`, `{ }`, `{1,2}`: teks biasa.
                teks.push_str(&ekor[..=rel]);
            }
            sisa = &ekor[rel + 1..];
        }
        teks.push_str(sisa);
        if !teks.is_empty() { keluar.push(SegmenFormat::Teks(teks)); }
        Ok(keluar)
    }

    /// Kalimat untuk jumlah argumen yang tidak cocok.
    pub fn pesan_jumlah(jml: usize, diberi: usize, template: &str) -> String {
        if diberi < jml {
            format!(
                "str::format: template ini punya {} placeholder, tapi cuma dikasih {} \
                 argumen (template: `{}`)", jml, diberi, template)
        } else {
            format!(
                "str::format: template ini punya {} placeholder, tapi dikasih {} argumen \
                 — {} argumen terakhir tidak terpakai (template: `{}`)",
                jml, diberi, diberi - jml, template)
        }
    }

    /// Baca spec sesudah `:`.
    fn baca_desimal(spec: &str) -> Result<usize, String> {
        let Some(digit) = spec.strip_prefix('.') else {
            return Err(format!(
                "str::format: spec `{{:{}}}` tidak dikenal — yang ada cuma `{{:.N}}` \
                 (jumlah desimal). Untuk merapikan lebar pakai `str::pad_left`/\
                 `str::pad_right`/`str::center`", spec));
        };
        if digit.is_empty() || !digit.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "str::format: spec `{{:{}}}` tidak dikenal — sesudah `.` harus angka, \
                 mis. `{{:.2}}`", spec));
        }
        let n: usize = digit.parse().unwrap_or(usize::MAX);
        if n > Interpreter::MAKS_DESIMAL {
            return Err(format!(
                "str::format: `{{:.{}}}` terlalu banyak desimal — maksimum {}. `float` \
                 cuma membawa sekitar 17 digit berarti, jadi selebihnya bukan ketelitian",
                n, Interpreter::MAKS_DESIMAL));
        }
        Ok(n)
    }
}
