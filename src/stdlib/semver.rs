// src/stdlib/semver.rs — `semver::`: versi SemVer 2.0.0 dan rentangnya, pengurai linear tanpa regex.
// Bentuk yang dibaca lain oleh pengurai lain jadi `Err`: awalan `v`/`=`, nol di depan, angka di luar 64 bit,
// versi telanjang di rentang (tepat menurut npm, `^` menurut Cargo), spasi sebagai "dan", rentang bertanda hubung.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// Panjang paling besar sebuah versi, sama dengan `MAX_LENGTH` npm.
pub const MAKS_VERSI: usize = 256;
/// Panjang paling besar sebuah rentang.
pub const MAKS_RENTANG: usize = 1024;

pub fn register(vm: &mut Vm) {
    vm.def("semver::parse", |a| crate::stdlib::sistem::lewat_hasil(a, s_parse));
    vm.def("semver::compare", |a| crate::stdlib::sistem::lewat_hasil(a, s_compare));
    vm.def("semver::satisfies", |a| crate::stdlib::sistem::lewat_hasil(a, s_satisfies));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Id { Angka(u64), Teks(String) }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versi { pub mayor: u64, pub minor: u64, pub patch: u64, pub pra: Vec<Id>, pub build: Vec<String> }

impl Versi {
    fn inti(&self) -> (u64, u64, u64) { (self.mayor, self.minor, self.patch) }
}

/// Urutan SemVer §11: `build` tidak ikut; tanpa pra-rilis lebih tinggi; angka di bawah teks.
impl Ord for Versi {
    fn cmp(&self, o: &Self) -> Ordering {
        self.inti().cmp(&o.inti()).then_with(|| match (self.pra.is_empty(), o.pra.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pra.iter().zip(&o.pra) {
                    let c = match (a, b) {
                        (Id::Angka(x), Id::Angka(y)) => x.cmp(y),
                        (Id::Angka(_), Id::Teks(_)) => Ordering::Less,
                        (Id::Teks(_), Id::Angka(_)) => Ordering::Greater,
                        (Id::Teks(x), Id::Teks(y)) => x.as_bytes().cmp(y.as_bytes()),
                    };
                    if c != Ordering::Equal { return c; }
                }
                self.pra.len().cmp(&o.pra.len())
            }
        })
    }
}
impl PartialOrd for Versi {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> { Some(self.cmp(o)) }
}

/// Satu bilangan versi: digit saja, tanpa nol di depan, muat di bilangan bulat 64 bit bertanda.
fn angka(s: &str, apa: &str) -> Result<u64, String> {
    if s.is_empty() { return Err(format!("{} kosong", apa)); }
    if !s.bytes().all(|b| b.is_ascii_digit()) { return Err(format!("{} `{}` bukan bilangan", apa, s)); }
    if s.len() > 1 && s.starts_with('0') { return Err(format!("{} `{}` berawalan nol", apa, s)); }
    match s.parse::<u64>() {
        Ok(n) if n <= i64::MAX as u64 => Ok(n),
        _ => Err(format!("{} `{}` di luar bilangan bulat 64 bit", apa, s)),
    }
}

fn id_sah(s: &str, apa: &str) -> Result<(), String> {
    if s.is_empty() { return Err(format!("{} punya bagian kosong", apa)); }
    match s.bytes().find(|b| !(b.is_ascii_alphanumeric() || *b == b'-')) {
        Some(b) => Err(format!("{} memuat `{}`; yang boleh cuma huruf ASCII, angka dan `-`", apa, b as char)),
        None => Ok(()),
    }
}

fn pra_rilis(s: &str) -> Result<Vec<Id>, String> {
    s.split('.').map(|b| {
        id_sah(b, "pra-rilis")?;
        if b.bytes().all(|c| c.is_ascii_digit()) { angka(b, "bagian pra-rilis").map(Id::Angka) } else { Ok(Id::Teks(b.into())) }
    }).collect()
}

/// `MAYOR.MINOR.PATCH[-pra][+build]` persis, tanpa awalan dan tanpa spasi.
pub fn urai(s: &str) -> Result<Versi, String> {
    if s.len() > MAKS_VERSI { return Err(format!("versi lebih dari {} byte", MAKS_VERSI)); }
    if let Some(c) = s.chars().next().filter(|c| matches!(c, 'v' | 'V' | '=' )) {
        return Err(format!("`{}`: versi tidak berawalan `{}` — tulis `{}`", s, c, &s[1..]));
    }
    if s.trim() != s || s.is_empty() { return Err(format!("`{}` bukan versi: kosong atau berspasi", s)); }
    let (sisa, build) = match s.split_once('+') {
        Some((a, b)) => {
            let build: Vec<String> = b.split('.').map(str::to_string).collect();
            for x in &build { id_sah(x, "build").map_err(|e| format!("`{}`: {}", s, e))?; }
            (a, build)
        }
        None => (s, Vec::new()),
    };
    let (inti, pra) = match sisa.split_once('-') {
        Some((a, p)) => (a, pra_rilis(p).map_err(|e| format!("`{}`: {}", s, e))?),
        None => (sisa, Vec::new()),
    };
    let b: Vec<&str> = inti.split('.').collect();
    if b.len() != 3 {
        return Err(format!("`{}` bukan bentuk MAYOR.MINOR.PATCH (mis. `1.2.3`): {} bagian", s, b.len()));
    }
    let f = |i: usize, apa: &str| angka(b[i], apa).map_err(|e| format!("`{}`: {}", s, e));
    Ok(Versi { mayor: f(0, "mayor")?, minor: f(1, "minor")?, patch: f(2, "patch")?, pra, build })
}

// ─── Rentang ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op { Tepat, Lebih, LebihSama, Kurang, KurangSama, Caret, Tilde }

/// Satu pembanding sesudah dijabarkan: batas bawah dan atas, plus versi pra-rilis yang membuka pintu
/// pra-rilis untuk inti yang sama (aturan npm dan Cargo).
#[derive(Debug)]
struct Batas { bawah: Option<(Versi, bool)>, atas: Option<(Versi, bool)>, pra: Option<(u64, u64, u64)> }

fn v3(a: u64, b: u64, c: u64) -> Versi { Versi { mayor: a, minor: b, patch: c, pra: Vec::new(), build: Vec::new() } }
/// Versi terendah di atas semua `x.y.z` dan pra-rilisnya: `x.y.z-0`.
fn nol(mut v: Versi) -> Versi { v.pra = vec![Id::Angka(0)]; v }

fn pembanding(s: &str) -> Result<Batas, String> {
    let t = s.trim();
    if t == "*" { return Ok(Batas { bawah: None, atas: None, pra: None }); }
    let polos = t.trim_start_matches(['<', '>', '=', '^', '~']).trim_start();
    if polos.split(['-', '+']).next().unwrap_or("").split('.').any(|x| matches!(x, "x" | "X" | "*")) {
        return Err(format!("`{}`: wildcard di dalam versi tidak dipakai — tulis `^`/`~` dengan versi sebagian (mis. `~1.2`)", t));
    }
    let (op, sisa) = [(">=", Op::LebihSama), ("<=", Op::KurangSama), (">", Op::Lebih), ("<", Op::Kurang),
                      ("=", Op::Tepat), ("^", Op::Caret), ("~", Op::Tilde)]
        .iter().find(|(p, _)| t.starts_with(p)).map(|(p, o)| (*o, t[p.len()..].trim_start()))
        .ok_or_else(|| format!("`{}` tanpa operator — tulis `={}` untuk tepat atau `^{}` untuk yang cocok \
            (npm membacanya tepat, Cargo `^`)", t, t, t))?;
    if sisa.contains(char::is_whitespace) {
        return Err(format!("`{}` berspasi di tengah — pisahkan pembanding dengan koma", t));
    }
    if sisa.starts_with(['v', 'V', '=']) { return Err(format!("`{}`: versi tidak berawalan `{}`", t, &sisa[..1])); }
    let (inti, ekor) = match sisa.find(['-', '+']) { Some(i) => (&sisa[..i], &sisa[i..]), None => (sisa, "") };
    let b: Vec<&str> = inti.split('.').collect();
    if b.len() > 3 { return Err(format!("`{}`: versi lebih dari tiga bagian", t)); }
    let n: Vec<u64> = b.iter().zip(["mayor", "minor", "patch"]).map(|(x, a)| angka(x, a))
        .collect::<Result<_, _>>().map_err(|e| format!("`{}`: {}", t, e))?;
    if !ekor.is_empty() && n.len() != 3 { return Err(format!("`{}`: pra-rilis/build butuh versi utuh", t)); }
    let v = if ekor.is_empty() { v3(n[0], *n.get(1).unwrap_or(&0), *n.get(2).unwrap_or(&0)) } else { urai(sisa)? };
    let pra = if v.pra.is_empty() { None } else { Some(v.inti()) };
    let (a, b2, c) = v.inti();
    // Batas atas eksklusif untuk versi sebagian: `1.2` = `>=1.2.0, <1.3.0-0`.
    let naik = |k: usize| -> Versi {
        match k { 0 => nol(v3(a + 1, 0, 0)), 1 => nol(v3(a, b2 + 1, 0)), _ => nol(v3(a, b2, c + 1)) }
    };
    let sebagian = n.len();
    let bawah = |v: &Versi| Some((v.clone(), true));
    Ok(match op {
        Op::Tepat if sebagian == 3 => Batas { bawah: bawah(&v), atas: Some((v.clone(), true)), pra },
        Op::Tepat => Batas { bawah: bawah(&v), atas: Some((naik(sebagian - 1), false)), pra },
        Op::LebihSama => Batas { bawah: bawah(&v), atas: None, pra },
        Op::Lebih if sebagian == 3 => Batas { bawah: Some((v.clone(), false)), atas: None, pra },
        Op::Lebih => Batas { bawah: Some((naik(sebagian - 1), true)), atas: None, pra },
        Op::Kurang => Batas { bawah: None, atas: Some((if sebagian == 3 && pra.is_some() { v.clone() } else { nol(v.clone()) }, false)), pra },
        Op::KurangSama if sebagian == 3 => Batas { bawah: None, atas: Some((v.clone(), true)), pra },
        Op::KurangSama => Batas { bawah: None, atas: Some((naik(sebagian - 1), false)), pra },
        Op::Tilde => Batas { bawah: bawah(&v), atas: Some((naik(if sebagian == 1 { 0 } else { 1 }), false)), pra },
        Op::Caret => {
            // Angka bukan-nol pertama dikunci (Cargo); `^0` dan `^0.0` mengunci sebanyak yang ditulis.
            let k = if a > 0 || sebagian == 1 { 0 } else if b2 > 0 || sebagian == 2 { 1 } else { 2 };
            Batas { bawah: bawah(&v), atas: Some((naik(k), false)), pra }
        }
    })
}

fn kena(b: &Batas, v: &Versi) -> bool {
    let bawah = b.bawah.as_ref().map_or(true, |(x, t)| if *t { v >= x } else { v > x });
    let atas = b.atas.as_ref().map_or(true, |(x, t)| if *t { v <= x } else { v < x });
    bawah && atas
}

/// `a, b || c`: koma berarti "dan", `||` berarti "atau". Versi pra-rilis cuma cocok kalau satu pembanding
/// di kelompoknya menulis pra-rilis berinti sama.
pub fn cocok(v: &Versi, rentang: &str) -> Result<bool, String> {
    if rentang.len() > MAKS_RENTANG { return Err(format!("rentang lebih dari {} byte", MAKS_RENTANG)); }
    if rentang.trim().is_empty() { return Err("rentang kosong — tulis `*` untuk versi apa pun".into()); }
    if rentang.contains(" - ") {
        return Err(format!("`{}`: rentang bertanda hubung tidak dipakai — tulis `>=a, <=b`", rentang.trim()));
    }
    let mut hasil = false;
    for kelompok in rentang.split("||") {
        if kelompok.trim().is_empty() { return Err(format!("`{}`: ada kelompok kosong di sekitar `||`", rentang.trim())); }
        let mut semua = true;
        let mut pintu_pra = false;
        for p in kelompok.split(',') {
            if p.trim().is_empty() { return Err(format!("`{}`: ada pembanding kosong di sekitar koma", rentang.trim())); }
            let b = pembanding(p)?;
            semua &= kena(&b, v);
            pintu_pra |= b.pra == Some(v.inti());
        }
        hasil |= semua && (v.pra.is_empty() || pintu_pra);
    }
    Ok(hasil)
}

// ─── Pintu builtin ────────────────────────────────────────────────────────

fn teks_arg<'a>(a: &'a [Value], i: usize, nama: &str) -> Result<&'a str, String> {
    match a.get(i) {
        Some(Value::Str(s)) => Ok(s),
        Some(lain) => Err(format!("{}: argumen ke-{} harus string, dapat `{}`", nama, i + 1, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, i + 1)),
    }
}

fn ke_nilai(v: &Versi) -> Value {
    let mut m: HashMap<String, Value> = HashMap::new();
    m.insert("mayor".into(), Value::Int(v.mayor as i128));
    m.insert("minor".into(), Value::Int(v.minor as i128));
    m.insert("patch".into(), Value::Int(v.patch as i128));
    m.insert("pra".into(), Value::Array(v.pra.iter().map(|i| match i {
        Id::Angka(n) => Value::Int(*n as i128), Id::Teks(t) => Value::Str(std::sync::Arc::new(t.clone())),
    }).collect::<Vec<_>>().into()));
    m.insert("build".into(), Value::Array(v.build.iter().map(|b| Value::Str(std::sync::Arc::new(b.clone()))).collect::<Vec<_>>().into()));
    Value::Struct { name: "dict".to_string(), fields: m.into() }
}

/// `semver::parse(teks)` → `Result<dict>` `{mayor, minor, patch, pra, build}`.
fn s_parse(a: &[Value]) -> Result<Value, String> {
    let t = teks_arg(a, 0, "semver::parse")?;
    Ok(match urai(t) { Ok(v) => ok_value(ke_nilai(&v)), Err(e) => err_value(format!("semver::parse: {}", e)) })
}

/// `semver::compare(a, b)` → `Result<int>`: -1, 0 atau 1 menurut urutan SemVer; `build` tidak ikut.
fn s_compare(a: &[Value]) -> Result<Value, String> {
    let (x, y) = (teks_arg(a, 0, "semver::compare")?, teks_arg(a, 1, "semver::compare")?);
    Ok(match (urai(x), urai(y)) {
        (Ok(p), Ok(q)) => ok_value(Value::Int(match p.cmp(&q) { Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1 })),
        (Err(e), _) | (_, Err(e)) => err_value(format!("semver::compare: {}", e)),
    })
}

/// `semver::satisfies(versi, rentang)` → `Result<bool>`.
fn s_satisfies(a: &[Value]) -> Result<Value, String> {
    let (x, r) = (teks_arg(a, 0, "semver::satisfies")?, teks_arg(a, 1, "semver::satisfies")?);
    Ok(match urai(x).and_then(|v| cocok(&v, r)) {
        Ok(b) => ok_value(Value::Bool(b)),
        Err(e) => err_value(format!("semver::satisfies: {}", e)),
    })
}

#[cfg(test)]
mod tes {
    use super::*;

    fn c(v: &str, r: &str) -> bool { cocok(&urai(v).unwrap(), r).unwrap() }

    #[test]
    fn urutan_dan_bentuk_ketat() {
        let urut = ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2",
                    "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1", "1.10.0", "2.0.0"];
        for w in urut.windows(2) { assert!(urai(w[0]).unwrap() < urai(w[1]).unwrap(), "{} < {}", w[0], w[1]); }
        assert_eq!(urai("1.0.0+a").unwrap().cmp(&urai("1.0.0+b").unwrap()), Ordering::Equal);
        for s in ["v1.2.3", "=1.2.3", " 1.2.3", "1.2", "1.2.3.4", "01.2.3", "1.02.3", "1.2.3-01", "1.2.3-",
                  "1.2.3-a..b", "1.2.3+", "1.2.3-a_b", "9223372036854775808.0.0", "+1.2.3", "1.2.3-é"] {
            assert!(urai(s).is_err(), "{:?} diterima", s);
        }
        assert!(urai(&format!("1.2.3-{}", "a".repeat(300))).unwrap_err().contains("256"));
        assert_eq!(urai("1.2.3-x.7+b.01").unwrap().pra, vec![Id::Teks("x".into()), Id::Angka(7)]);
    }

    #[test]
    fn rentang_caret_tilde_dan_pra_rilis() {
        assert!(c("1.9.9", "^1.2.3") && !c("2.0.0", "^1.2.3") && !c("1.2.2", "^1.2.3"));
        assert!(c("0.2.9", "^0.2.3") && !c("0.3.0", "^0.2.3"));
        assert!(c("0.0.3", "^0.0.3") && !c("0.0.4", "^0.0.3"));
        assert!(c("0.9.0", "^0") && !c("1.0.0", "^0") && c("0.0.9", "^0.0") && !c("0.1.0", "^0.0"));
        assert!(c("1.2.9", "~1.2.3") && !c("1.3.0", "~1.2.3") && c("1.9.0", "~1"));
        assert!(c("1.2.7", "=1.2") && !c("1.3.0", "=1.2") && c("1.3.0", ">1.2") && !c("1.2.9", ">1.2"));
        assert!(c("1.5.0", ">=1.2, <2") && !c("2.0.0", ">=1.2, <2") && c("3.0.0", "<1 || >=3"));
        // Pra-rilis tidak menyusup lewat batas atas atau pembanding yang tidak menulisnya.
        assert!(!c("2.0.0-rc.1", "<2.0.0") && !c("1.3.0-beta", "^1.2.3") && !c("1.0.0-a", "*"));
        assert!(c("1.2.3-beta.2", "^1.2.3-beta.1") && !c("1.2.4-beta", "^1.2.3-beta.1") && c("1.2.4", "^1.2.3-beta.1"));
        for r in ["1.2.3", ">= 1.2 <2", "1.2.3 - 2.0.0", "1.x", "^1.*", ">=v1.0.0", "", ">=1,", "|| ^1", "^1.2.3.4", "~01.2"] {
            assert!(cocok(&urai("1.2.3").unwrap(), r).is_err(), "{:?} diterima", r);
        }
        assert!(cocok(&urai("1.2.3").unwrap(), &"^1, ".repeat(400)).unwrap_err().contains("1024"));
    }
}
