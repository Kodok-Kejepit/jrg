// src/stdlib/regex.rs
// `re::` — pintu builtin; mesinnya di `regex_inti` (Pike VM, waktu linear).

use crate::vm::vm::Vm;
use crate::vm::value::VmValue;
use std::sync::Arc;
use std::collections::HashMap;

pub fn register(vm: &mut Vm) {
    use VmValue::*;

    vm.def("re::compile", |a| match a.get(0) {
        Some(Str(pat)) => {
            let mut m = HashMap::new();
            m.insert("pattern".to_string(), Str(pat.clone()));
            // Validate pattern at compile time
            match compile_pattern(pat.as_str()) {
                Ok(_)  => Ok(VmValue::struktur("Regex", m)),
                Err(e) => Err(format!("re::compile '{}': {}", crate::kutip_inti::kutip(pat.as_str()), e)),
            }
        }
        _ => Err("re::compile(pattern: string)".to_string()),
    });

    vm.def("re::test", |a| match (a.get(0), a.get(1)) {
        (Some(Map(re)), Some(Str(s))) => {
            let pat = re.lock().unwrap()
                .get("pattern").and_then(|v| if let Str(s) = v { Some(s.clone()) } else { None });
            match pat {
                Some(p) => Ok(Bool(regex_test(p.as_str(), s.as_str())?)),
                None    => Err("re::test: invalid Regex object".to_string()),
            }
        }
        // Convenience: (pattern_str, s)
        (Some(Str(pat)), Some(Str(s))) => Ok(Bool(regex_test(pat.as_str(), s.as_str())?)),
        _ => Err("re::test(re, s)".to_string()),
    });

    vm.def("re::find", |a| match (a.get(0), a.get(1)) {
        (Some(re_val), Some(Str(s))) => {
            let pat = extract_pattern(re_val)?;
            match regex_find(&pat, s.as_str())? {
                // `Option`, sama dengan `arr::find` dan `str::find`.
                Some((start, end)) => {
                    let mut m = HashMap::new();
                    m.insert("start".to_string(),   Int(start as i128));
                    m.insert("end".to_string(),     Int(end as i128));
                    m.insert("match".to_string(),   Str(Arc::new(s[start..end].to_string())));
                    Ok(VmValue::some(VmValue::map(m)))
                }
                None => Ok(VmValue::none()),
            }
        }
        _ => Err("re::find(re, s)".to_string()),
    });

    vm.def("re::find_all", |a| match (a.get(0), a.get(1)) {
        (Some(re_val), Some(Str(s))) => {
            let pat = extract_pattern(re_val)?;
            let matches = regex_find_all(&pat, s.as_str())?;
            let results: Vec<VmValue> = matches.into_iter().map(|(start, end)| {
                let mut m = HashMap::new();
                m.insert("start".to_string(), Int(start as i128));
                m.insert("end".to_string(),   Int(end as i128));
                m.insert("match".to_string(), Str(Arc::new(s[start..end].to_string())));
                VmValue::map(m)
            }).collect();
            Ok(Array(Arc::new(std::sync::Mutex::new(results))))
        }
        _ => Err("re::find_all(re, s)".to_string()),
    });

    vm.def("re::replace", |a| match (a.get(0), a.get(1), a.get(2)) {
        (Some(re_val), Some(Str(s)), Some(Str(repl))) => {
            let pat = extract_pattern(re_val)?;
            Ok(Str(Arc::new(regex_replace(&pat, s.as_str(), repl.as_str(), false)?)))
        }
        _ => Err("re::replace(re, s, replacement)".to_string()),
    });

    vm.def("re::replace_all", |a| match (a.get(0), a.get(1), a.get(2)) {
        (Some(re_val), Some(Str(s)), Some(Str(repl))) => {
            let pat = extract_pattern(re_val)?;
            Ok(Str(Arc::new(regex_replace(&pat, s.as_str(), repl.as_str(), true)?)))
        }
        _ => Err("re::replace_all(re, s, replacement)".to_string()),
    });

    vm.def("re::split", |a| match (a.get(0), a.get(1)) {
        (Some(re_val), Some(Str(s))) => {
            let pat = extract_pattern(re_val)?;
            let parts = regex_split(&pat, s.as_str())?;
            let items: Vec<VmValue> = parts.into_iter()
                .map(|p| Str(Arc::new(p)))
                .collect();
            Ok(Array(Arc::new(std::sync::Mutex::new(items))))
        }
        _ => Err("re::split(re, s)".to_string()),
    });

    vm.def("re::match_str", |a| match (a.get(0), a.get(1)) {
        (Some(Str(pat)), Some(Str(s))) => Ok(Bool(regex_test(pat.as_str(), s.as_str())?)),
        _ => Err("re::match_str(pattern, s)".to_string()),
    });
}

fn extract_pattern(v: &VmValue) -> Result<String, String> {
    match v {
        VmValue::Map(m) => m.lock().unwrap()
            .get("pattern")
            .and_then(|v| if let VmValue::Str(s) = v { Some(s.to_string()) } else { None })
            .ok_or_else(|| "invalid Regex object".to_string()),
        VmValue::Str(s) => Ok(s.to_string()),
        _ => Err("butuh Regex atau pola berupa string".to_string()),
    }
}

// ─── Mesin: `regex_inti` (Pike VM, waktu linear) ─────────────────────────

use super::regex_inti::Regex;

fn compile_pattern(pat: &str) -> Result<Regex, String> { Regex::baru(pat) }

/// Teks sebagai karakter, plus offset byte tiap karakter (dan ujungnya) — indeks hasil dalam byte.
fn karakter(s: &str) -> (Vec<char>, Vec<usize>) {
    let mut cs = Vec::with_capacity(s.len());
    let mut off = Vec::with_capacity(s.len() + 1);
    for (i, c) in s.char_indices() { cs.push(c); off.push(i); }
    off.push(s.len());
    (cs, off)
}

/// Gagal menyusun pola DITOLAK, tidak pernah dijawab `false`/`None`.
fn regex_test(pattern: &str, s: &str) -> Result<bool, String> {
    let r = compile_pattern(pattern)?;
    let (cs, _) = karakter(s);
    Ok(r.cari_dari(&cs, 0).is_some())
}

fn regex_find(pattern: &str, s: &str) -> Result<Option<(usize, usize)>, String> {
    let r = compile_pattern(pattern)?;
    let (cs, off) = karakter(s);
    Ok(r.cari_dari(&cs, 0).map(|(a, b)| (off[a], off[b])))
}

fn regex_find_all(pattern: &str, s: &str) -> Result<Vec<(usize, usize)>, String> {
    let r = compile_pattern(pattern)?;
    let (cs, off) = karakter(s);
    Ok(r.semua(&cs).into_iter().map(|(a, b)| (off[a], off[b])).collect())
}

/// Teks di antara kecocokan disalin utuh — dulu kecocokan kosong menelan karakter sesudahnya.
fn regex_replace(pattern: &str, s: &str, repl: &str, all: bool) -> Result<String, String> {
    let cocok = if all { regex_find_all(pattern, s)? } else { regex_find(pattern, s)?.into_iter().collect() };
    let mut out = String::with_capacity(s.len());
    let mut akhir = 0;
    for (a, b) in cocok {
        out.push_str(&s[akhir..a]);
        out.push_str(repl);
        akhir = b;
    }
    out.push_str(&s[akhir..]);
    Ok(out)
}

fn regex_split(pattern: &str, s: &str) -> Result<Vec<String>, String> {
    let matches = regex_find_all(pattern, s)?;
    if matches.is_empty() { return Ok(vec![s.to_string()]); }
    let mut parts = Vec::new();
    let mut last = 0;
    for (start, end) in matches {
        parts.push(s[last..start].to_string());
        last = end;
    }
    parts.push(s[last..].to_string());
    Ok(parts)
}
