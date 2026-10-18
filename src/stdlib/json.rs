// src/stdlib/json.rs
// JSON parse + stringify (zero deps, pakai parser dari lsp/mod.rs)

use crate::vm::vm::Vm;
use crate::vm::value::VmValue;
use std::sync::Arc;
use std::collections::HashMap;

/// Pasangan key/value sebuah map, DIURUTKAN menurut key.
fn pasangan_terurut(
    g: &std::collections::HashMap<String, VmValue>,
) -> Vec<(&String, &VmValue)> {
    let mut v: Vec<(&String, &VmValue)> =
        g.iter().collect();
    v.sort_unstable_by(|a, b| a.0.cmp(b.0));
    v
}

/// Pasangan yang ditulis untuk sebuah `Map` biasa (dict atau struct).
fn bagian_json(m: &crate::vm::value::Peta) -> Vec<(String, VmValue)> {
    let g = m.lock().unwrap_or_else(|p| p.into_inner());
    pasangan_terurut(&g).into_iter().map(|(k, x)| (k.clone(), x.clone())).collect()
}

/// Sama, untuk penulis yang boleh GAGAL: payload sebuah varian bisa memuat
/// nilai yang tidak punya padanan JSON.
fn enum_ke_json_hasil(v: &VmValue) -> Result<Option<String>, String> {
    let Some((_, varian, payload)) = v.bagian_enum() else { return Ok(None) };
    Ok(Some(match payload {
        None => json_escape(varian),
        Some(p) => format!("{{{}:{}}}", json_escape(varian), stringify(p, 0, false)?),
    }))
}

/// Pintu untuk `dok::json_encode`, supaya bahasa ini punya SATU penyandi JSON.
pub fn stringify_pub(v: &VmValue) -> Result<String, String> { stringify(v, 0, false) }

pub fn register(vm: &mut Vm) {
    use VmValue::*;

    vm.def("json::parse", |a| match a.get(0) {
        Some(Str(s)) => parse_json(s.as_str())
            .map_err(|e| format!("json::parse: {}", e)),
        _ => Err("json::parse(s: string)".to_string()),
    });

    vm.def("json::stringify", |a| match a.get(0) {
        Some(v) => Ok(Str(Arc::new(stringify(v, 0, false)?))),
        None    => Ok(Str(Arc::new("null".to_string()))),
    });

    // Lebar indentasinya DIPAKAI: dulu dibaca lalu dibuang, jadi `json::pretty(v, 4)`
    // tetap dua spasi dan argumen bertipe apa pun diterima diam-diam.
    vm.def("json::pretty", |a| {
        let val    = a.get(0).unwrap_or(&Nil);
        let indent = match a.get(1) {
            None => 2,
            Some(Int(n)) if (0..=16).contains(n) => *n as usize,
            Some(Int(n)) => return Err(format!(
                "json::pretty: lebar indentasi {} di luar jangkauan 0 sampai 16", n)),
            Some(lain) => return Err(format!(
                "json::pretty: argumen ke-2 (lebar indentasi) harus bilangan bulat, dapat `{}`",
                lain.type_name_penuh())),
        };
        Ok(Str(Arc::new(stringify_pretty(val, 0, indent)?)))
    });

    vm.def("json::get", |a| match (a.get(0), a.get(1)) {
        (Some(Map(m)), Some(Str(k))) =>
            // `Option`: kunci ber-isi `null` dan kunci yang tidak ada dulu sama-sama `nil`.
            Ok(m.lock().unwrap().get(k.as_str()).cloned().map(VmValue::some).unwrap_or_else(VmValue::none)),
        _ => Err("json::get(obj, key)".to_string()),
    });

    // Menjawab objek BARU, seperti `dict::set`. Dulu ia menulis ke peta
    // argumennya: `--vm` mengubah milik pemanggil lewat alias, sementara acuan
    // (lewat jembatan yang menyalin) tidak mengubah apa pun — dan dua-duanya `nil`.
    vm.def("json::set", |a| match (a.get(0), a.get(1), a.get(2)) {
        (Some(o @ Map(m)), Some(Str(k)), Some(v)) if o.type_name_penuh() == "dict" => {
            let mut isi = m.lock().unwrap().clone();
            isi.insert(k.as_str().to_string(), v.clone());
            Ok(VmValue::map(isi))
        }
        _ => Err("json::set(obj, key, val)".to_string()),
    });

    vm.def("json::has", |a| match (a.get(0), a.get(1)) {
        (Some(Map(m)), Some(Str(k))) =>
            Ok(Bool(m.lock().unwrap().contains_key(k.as_str()))),
        _ => Err("json::has(obj, key)".to_string()),
    });

    vm.def("json::keys", |a| match a.get(0) {
        Some(Map(m)) => {
            let g = m.lock().unwrap();
            let ks: Vec<VmValue> = pasangan_terurut(&g).into_iter()
                .map(|(k, _)| Str(Arc::new(k.clone())))
                .collect();
            Ok(Array(Arc::new(std::sync::Mutex::new(ks))))
        }
        _ => Err("json::keys(obj)".to_string()),
    });

    vm.def("json::values", |a| match a.get(0) {
        Some(Map(m)) => {
            // Urutannya harus SAMA dengan `json::keys`, supaya `keys[i]` dan `values[i]`
            // benar-benar sepasang.
            let g = m.lock().unwrap();
            let vs: Vec<VmValue> = pasangan_terurut(&g).into_iter()
                .map(|(_, v)| v.clone())
                .collect();
            Ok(Array(Arc::new(std::sync::Mutex::new(vs))))
        }
        _ => Err("json::values(obj)".to_string()),
    });

    vm.def("json::merge", |a| match (a.get(0), a.get(1)) {
        (Some(Map(a_map)), Some(Map(b_map))) => {
            let mut merged = a_map.lock().unwrap().clone();
            for (k, v) in b_map.lock().unwrap().iter() {
                merged.insert(k.clone(), v.clone());
            }
            // Label mengikuti aturan kuncinya: milik `b` menang kalau ia bukan dict.
            let label = match &b_map.label {
                crate::vm::value::Label::Dict => a_map.label.clone(),
                lain => lain.clone(),
            };
            Ok(Map(Arc::new(crate::vm::value::Peta::baru(label, merged))))
        }
        _ => Err("json::merge(a, b)".to_string()),
    });
}

// ─── Parser ──────────────────────────────────────────────────────────────

/// Urai lewat pengurai bersama (`json_inti`), lalu jadikan nilai VM. Kunci kembar: yang terakhir.
fn parse_json(s: &str) -> Result<VmValue, String> {
    fn ke_vm(j: crate::json_inti::Json) -> VmValue {
        use crate::json_inti::Json;
        match j {
            Json::Null => VmValue::Nil,
            Json::Bool(b) => VmValue::Bool(b),
            Json::Int(n) => VmValue::Int(n),
            Json::Float(f) => VmValue::Float(f),
            Json::Str(t) => VmValue::str(t),
            Json::Arr(a) => VmValue::array(a.into_iter().map(ke_vm).collect()),
            Json::Obj(o) => {
                let mut map = HashMap::new();
                for (k, v) in o { map.insert(k, ke_vm(v)); }
                VmValue::map(map)
            }
        }
    }
    crate::json_inti::urai(s).map(ke_vm)
}

// ─── Serializer ───────────────────────────────────────────────────────────

/// Nilai yang tidak punya padanan JSON DITOLAK, bukan dipalsukan jadi teks:
/// `NaN`/`inf` menghasilkan keluaran yang bukan JSON, dan `"<function>"`
/// tampak seperti data di program yang membacanya.
fn stringify(v: &VmValue, _depth: usize, _pretty: bool) -> Result<String, String> {
    let tolak = |apa: &str, jalan: &str| -> Result<String, String> {
        Err(crate::interpreter::galat_index::tidak_bisa_jadi_json(apa, jalan))
    };
    match v {
        VmValue::Nil          => Ok("null".to_string()),
        VmValue::Unit         => Ok("null".to_string()),
        VmValue::Bool(b)      => Ok(b.to_string()),
        VmValue::Int(n)       => Ok(n.to_string()),
        VmValue::Float(f)     => match crate::json_inti::sandi_pecahan(*f) {
            Some(t) => Ok(t),
            None => tolak(&format!("{}", f), crate::json_inti::JALAN_PECAHAN),
        },
        VmValue::Str(s)       => Ok(json_escape(s)),
        // Rune di-serialisasi sebagai string 1-karakter, sama seperti JSON
        // tidak punya tipe karakter tersendiri.
        VmValue::Rune(c)      => Ok(json_escape(&c.to_string())),
        VmValue::Array(a)     => {
            let items: Vec<String> = a.lock().unwrap().iter()
                .map(|x| stringify(x, 0, false)).collect::<Result<_, _>>()?;
            Ok(format!("[{}]", items.join(",")))
        }
        VmValue::Tuple(t)     => {
            let items: Vec<String> = crate::vm::value::isi_tuple(t).iter()
                .map(|x| stringify(x, 0, false)).collect::<Result<_, _>>()?;
            Ok(format!("[{}]", items.join(",")))
        }
        VmValue::Map(m)       => {
            if let Some(j) = enum_ke_json_hasil(v)? { return Ok(j); }
            let mut pairs: Vec<String> = Vec::new();
            for (k, x) in bagian_json(m).iter() {
                pairs.push(format!("{}:{}", json_escape(k), stringify(x, 0, false)?));
            }
            Ok(format!("{{{}}}", pairs.join(",")))
        }
        // Array numerik jadi array JSON biasa — itu satu-satunya bentuk
        // JSON yang punya arti untuknya, dan menuliskannya sebagai
        // `"<wokwok>"` akan membuang datanya tanpa memberi tahu.
        VmValue::Nd(a) => stringify(&crate::wokwok::daftar::ke_daftar(a), _depth, _pretty),
        // Fungsi dan handle tidak punya padanan JSON. Dulu keduanya jadi
        // TEKS (`"<function>"`, `"<canvas>"`) yang tampak seperti data.
        VmValue::Function(_) | VmValue::NativeFn(_) | VmValue::Ktor(_)
        | VmValue::ExternFn(_) | VmValue::Closure(_) =>
            tolak("fungsi", crate::json_inti::JALAN_FUNGSI),
        VmValue::Channel(_)  => tolak("channel", crate::json_inti::JALAN_CHANNEL),
        VmValue::Sinkron(p)  => tolak(p.nama_tipe(), crate::json_inti::JALAN_PEGANGAN),
        VmValue::Asing(a)    => tolak(&a.nama_tipe(), "ambil nilainya dulu (`py::to_jrg`, `js::to_jrg`, \
                                      `php::to_jrg`), bukan handle-nya"),
        VmValue::Canvas(_)   => tolak("canvas",
            "simpan gambarnya (`canvas::save_png`) lalu kirim nama berkasnya"),
        VmValue::Ptr(_)      => tolak("ptr",
            "kirim nilai yang ditunjuknya (`ptr::get(p)`), bukan pointernya"),
        // Sel binding closure tidak pernah sampai ke sini lewat jalur
        // normal, tapi kalau sampai, yang harus di-serialisasi adalah NILAI
        // di dalam binding-nya.
        VmValue::Cell(c)     => stringify(&c.lock().unwrap_or_else(|p| p.into_inner()), _depth, _pretty),
    }
}

fn stringify_pretty(v: &VmValue, depth: usize, indent: usize) -> Result<String, String> {
    let pad   = " ".repeat(depth * indent);
    let inner = " ".repeat((depth + 1) * indent);
    match v {
        VmValue::Array(a) => {
            let g = a.lock().unwrap();
            if g.is_empty() { return Ok("[]".to_string()); }
            let mut items: Vec<String> = Vec::new();
            for x in g.iter() {
                items.push(format!("{}{}", inner, stringify_pretty(x, depth + 1, indent)?));
            }
            Ok(format!("[\n{}\n{}]", items.join(",\n"), pad))
        }
        VmValue::Map(m) => {
            // Enum ditulis dengan indentasi yang sama dengan objek lain; memakai
            // `enum_ke_json` di sini akan menaruh pembungkusnya di satu baris
            // sementara isinya multi-baris.
            if let Some((_, varian, payload)) = v.bagian_enum() {
                return match payload {
                    None => Ok(json_escape(varian)),
                    Some(p) => Ok(format!("{{\n{}{}: {}\n{}}}",
                        inner, json_escape(varian),
                        stringify_pretty(p, depth + 1, indent)?, pad)),
                };
            }
            let mut pairs: Vec<String> = Vec::new();
            for (k, val) in bagian_json(m).iter() {
                pairs.push(format!("{}{}: {}",
                    inner, json_escape(k), stringify_pretty(val, depth + 1, indent)?));
            }
            if pairs.is_empty() { return Ok("{}".to_string()); }
            Ok(format!("{{\n{}\n{}}}", pairs.join(",\n"), pad))
        }
        _ => stringify(v, depth, false),
    }
}

fn json_escape(s: &str) -> String { crate::json_inti::sandi_teks(s) }
