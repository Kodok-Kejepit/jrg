// src/stdlib/regex.rs
// Basic Regex Engine (NFA-based, no deps)

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
                Err(e) => Err(format!("re::compile '{}': {}", pat, e)),
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

// ─── NFA-based Regex Engine ───────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    Literal(char),
    AnyChar,
    Class(Vec<ClassItem>, bool), // items, negated
    Anchor(bool),                // true=start, false=end
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Node>, usize, Option<usize>), // min, max (None=infinity)
    Group(Box<Node>),
    Empty,
}

#[derive(Debug, Clone)]
enum ClassItem { Single(char), Range(char, char) }

/// Urai sebuah pola jadi pohon.
fn compile_pattern(pat: &str) -> Result<Node, String> {
    let chars: Vec<char> = pat.chars().collect();
    let mut pos = 0;
    let node = parse_alt(&chars, &mut pos)?;
    // `parse_concat` berhenti di `)`, jadi kurung tutup yang tidak punya
    // pembuka membuat pengurai berhenti di tengah dan sisanya hilang.
    if pos < chars.len() {
        return Err(format!(
            "ada `{}` di posisi {} yang tidak punya pembuka — sisa polanya \
             (`{}`) jadi tidak ikut terpakai sama sekali",
            chars[pos], pos, chars[pos..].iter().collect::<String>()
        ));
    }
    Ok(node)
}

fn parse_alt(chars: &[char], pos: &mut usize) -> Result<Node, String> {
    let mut alts = vec![parse_concat(chars, pos)?];
    while *pos < chars.len() && chars[*pos] == '|' {
        *pos += 1;
        alts.push(parse_concat(chars, pos)?);
    }
    if alts.len() == 1 { Ok(alts.remove(0)) }
    else { Ok(Node::Alt(alts)) }
}

fn parse_concat(chars: &[char], pos: &mut usize) -> Result<Node, String> {
    let mut items = Vec::new();
    while *pos < chars.len() && !matches!(chars[*pos], '|' | ')') {
        let atom = parse_atom(chars, pos)?;
        let node = parse_quantifier(chars, pos, atom)?;
        items.push(node);
    }
    if items.len() == 1 { Ok(items.remove(0)) }
    else { Ok(Node::Concat(items)) }
}

fn parse_atom(chars: &[char], pos: &mut usize) -> Result<Node, String> {
    if *pos >= chars.len() { return Ok(Node::Empty); }
    match chars[*pos] {
        '.' => { *pos += 1; Ok(Node::AnyChar) }
        '^' => { *pos += 1; Ok(Node::Anchor(true)) }
        '$' => { *pos += 1; Ok(Node::Anchor(false)) }
        '(' => {
            *pos += 1;
            let inner = parse_alt(chars, pos)?;
            if *pos >= chars.len() || chars[*pos] != ')' {
                return Err("grup `(` tidak pernah ditutup `)`".to_string());
            }
            *pos += 1;
            Ok(Node::Group(Box::new(inner)))
        }
        '[' => {
            *pos += 1;
            let negated = *pos < chars.len() && chars[*pos] == '^';
            if negated { *pos += 1; }
            let mut items = Vec::new();
            while *pos < chars.len() && chars[*pos] != ']' {
                let c = escape_char(chars, pos);
                if *pos + 1 < chars.len() && chars[*pos] == '-' && chars[*pos + 1] != ']' {
                    *pos += 1;
                    let c2 = escape_char(chars, pos);
                    items.push(ClassItem::Range(c, c2));
                } else {
                    items.push(ClassItem::Single(c));
                }
            }
            if *pos >= chars.len() {
                return Err("kelas karakter `[` tidak pernah ditutup `]`".to_string());
            }
            *pos += 1; // ']'
            Ok(Node::Class(items, negated))
        }
        '\\' => {
            *pos += 1;
            if *pos >= chars.len() { return Err("trailing backslash".to_string()); }
            let node = match chars[*pos] {
                'd' => Node::Class(vec![ClassItem::Range('0','9')], false),
                'D' => Node::Class(vec![ClassItem::Range('0','9')], true),
                'w' => Node::Class(vec![
                    ClassItem::Range('a','z'), ClassItem::Range('A','Z'),
                    ClassItem::Range('0','9'), ClassItem::Single('_'),
                ], false),
                'W' => Node::Class(vec![
                    ClassItem::Range('a','z'), ClassItem::Range('A','Z'),
                    ClassItem::Range('0','9'), ClassItem::Single('_'),
                ], true),
                's' => Node::Class(vec![
                    ClassItem::Single(' '), ClassItem::Single('\t'),
                    ClassItem::Single('\n'), ClassItem::Single('\r'),
                ], false),
                'S' => Node::Class(vec![
                    ClassItem::Single(' '), ClassItem::Single('\t'),
                    ClassItem::Single('\n'), ClassItem::Single('\r'),
                ], true),
                c   => Node::Literal(c),
            };
            *pos += 1;
            Ok(node)
        }
        c => { *pos += 1; Ok(Node::Literal(c)) }
    }
}

fn escape_char(chars: &[char], pos: &mut usize) -> char {
    if chars[*pos] == '\\' && *pos + 1 < chars.len() {
        *pos += 1;
        let c = match chars[*pos] {
            'n' => '\n', 'r' => '\r', 't' => '\t', c => c,
        };
        *pos += 1;
        c
    } else {
        let c = chars[*pos];
        *pos += 1;
        c
    }
}

fn parse_quantifier(chars: &[char], pos: &mut usize, node: Node) -> Result<Node, String> {
    if *pos >= chars.len() { return Ok(node); }
    let (min, max) = match chars[*pos] {
        '*' => { *pos += 1; (0, None) }
        '+' => { *pos += 1; (1, None) }
        '?' => { *pos += 1; (0, Some(1)) }
        '{' => {
            *pos += 1;
            let start = *pos;
            while *pos < chars.len() && chars[*pos] != '}' && chars[*pos] != ',' { *pos += 1; }
            let min_s: String = chars[start..*pos].iter().collect();
            // `unwrap_or(0)` dulu di sini: `a{x}` diam-diam jadi `a{0,0}`,
            // yang artinya `a` justru TIDAK BOLEH ada — kebalikan dari apa
            // pun yang mungkin dimaksud penulisnya.
            let min_n: usize = match min_s.parse() {
                Ok(n) => n,
                Err(_) => return Err(format!(
                    "jumlah pengulangan `{{{}}}` bukan angka", min_s
                )),
            };
            if *pos < chars.len() && chars[*pos] == ',' {
                *pos += 1;
                let start2 = *pos;
                while *pos < chars.len() && chars[*pos] != '}' { *pos += 1; }
                let max_s: String = chars[start2..*pos].iter().collect();
                let max_n = if max_s.is_empty() {
                    None
                } else {
                    match max_s.parse() {
                        Ok(n) => Some(n),
                        Err(_) => return Err(format!(
                            "batas atas pengulangan `{}` bukan angka", max_s
                        )),
                    }
                };
                if *pos >= chars.len() || chars[*pos] != '}' {
                    return Err("pengulangan `{` tidak pernah ditutup `}`".to_string());
                }
                *pos += 1;
                (min_n, max_n)
            } else {
                if *pos >= chars.len() || chars[*pos] != '}' {
                    return Err("pengulangan `{` tidak pernah ditutup `}`".to_string());
                }
                *pos += 1;
                (min_n, Some(min_n))
            }
        }
        _ => return Ok(node),
    };
    // Greedy: try max, fallback
    let lazy = *pos < chars.len() && chars[*pos] == '?';
    if lazy { *pos += 1; }
    Ok(Node::Repeat(Box::new(node), min, max))
}

// ─── Matching engine ──────────────────────────────────────────────────────

/// Keempat fungsi di bawah dulu MENELAN kegagalan kompilasi pola:
/// `regex_test` menjawab `false`, `regex_find` menjawab `None` …
fn regex_test(pattern: &str, s: &str) -> Result<bool, String> {
    let node = compile_pattern(pattern)?;
    let chars: Vec<char> = s.chars().collect();
    // Try matching at each position
    for start in 0..=chars.len() {
        if let Some(_) = match_node(&node, &chars, start, matches_start(&node)) {
            return Ok(true);
        }
        if matches_start(&node) { break; } // anchored at start: only try pos 0
    }
    Ok(false)
}

fn matches_start(node: &Node) -> bool {
    matches!(node, Node::Anchor(true))
        || matches!(node, Node::Concat(items) if items.first().map(matches_start).unwrap_or(false))
}

fn regex_find(pattern: &str, s: &str) -> Result<Option<(usize, usize)>, String> {
    let node = compile_pattern(pattern)?;
    let chars: Vec<char> = s.chars().collect();
    let anchored = matches_start(&node);
    for start in 0..=chars.len() {
        if let Some(end) = match_node(&node, &chars, start, false) {
            // Convert char indices to byte indices
            let byte_start = chars[..start].iter().collect::<String>().len();
            let byte_end   = chars[..end].iter().collect::<String>().len();
            return Ok(Some((byte_start, byte_end)));
        }
        if anchored { break; }
    }
    Ok(None)
}

fn regex_find_all(pattern: &str, s: &str) -> Result<Vec<(usize, usize)>, String> {
    let node = compile_pattern(pattern)?;
    let chars: Vec<char> = s.chars().collect();
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= chars.len() {
        if let Some(end) = match_node(&node, &chars, pos, false) {
            let bs = chars[..pos].iter().collect::<String>().len();
            let be = chars[..end].iter().collect::<String>().len();
            results.push((bs, be));
            pos = if end > pos { end } else { pos + 1 };
        } else {
            pos += 1;
        }
    }
    Ok(results)
}

fn regex_replace(pattern: &str, s: &str, repl: &str, all: bool) -> Result<String, String> {
    let node = compile_pattern(pattern)?;
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    let mut pos = 0;
    let mut replaced = false;
    while pos <= chars.len() {
        if !all && replaced { break; }
        if let Some(end) = match_node(&node, &chars, pos, false) {
            result.push_str(repl);
            pos = if end > pos { end } else { pos + 1 };
            replaced = true;
        } else {
            if pos < chars.len() { result.push(chars[pos]); }
            pos += 1;
        }
    }
    if pos <= chars.len() {
        result.push_str(&chars[pos..].iter().collect::<String>());
    }
    Ok(result)
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

// Returns Some(end_pos) if match found starting at `pos`
fn match_node(node: &Node, chars: &[char], pos: usize, _anchored: bool) -> Option<usize> {
    match node {
        Node::Empty          => Some(pos),
        Node::Anchor(true)   => if pos == 0 { Some(pos) } else { None },
        Node::Anchor(false)  => if pos == chars.len() { Some(pos) } else { None },
        Node::Literal(c)     => {
            if pos < chars.len() && chars[pos] == *c { Some(pos + 1) } else { None }
        }
        Node::AnyChar        => {
            if pos < chars.len() && chars[pos] != '\n' { Some(pos + 1) } else { None }
        }
        Node::Class(items, neg) => {
            if pos >= chars.len() { return None; }
            let c = chars[pos];
            let in_class = items.iter().any(|item| match item {
                ClassItem::Single(x) => *x == c,
                ClassItem::Range(a, b) => c >= *a && c <= *b,
            });
            if in_class != *neg { Some(pos + 1) } else { None }
        }
        Node::Group(inner)   => match_node(inner, chars, pos, false),
        Node::Concat(items)  => {
            let mut cur = pos;
            for item in items {
                cur = match_node(item, chars, cur, false)?;
            }
            Some(cur)
        }
        Node::Alt(alts)      => {
            for alt in alts {
                if let Some(end) = match_node(alt, chars, pos, false) {
                    return Some(end);
                }
            }
            None
        }
        Node::Repeat(inner, min, max) => {
            // Greedy matching
            let mut count = 0;
            let mut cur = pos;
            let mut positions = vec![cur];
            loop {
                if let Some(max_n) = max { if count >= *max_n { break; } }
                match match_node(inner, chars, cur, false) {
                    Some(next) if next > cur => { cur = next; count += 1; positions.push(cur); }
                    Some(_)                  => { count += 1; break; } // zero-width match
                    None                     => break,
                }
            }
            // Try from max down to min (greedy)
            for &end in positions.iter().rev() {
                let matched = positions.iter().position(|&p| p == end).unwrap_or(0);
                if matched >= *min { return Some(end); }
            }
            None
        }
    }
}
