// src/vm/akses.rs — field, index, dan jalur penulisan

use super::value::{Label, VmValue};
use super::vm::*;

// Daftar "nama namespace ini BUKAN method" tinggal bersama kosakata pintu
// method lainnya, karena ia aturan bahasa, bukan detail VM.
use crate::interpreter::{BUKAN_METODE_ARR, BUKAN_METODE_CHAN, BUKAN_METODE_STR};
use super::bawaan::bawaan;

impl Vm {
    // ── Field / Index ─────────────────────────────────────────────────────

    /// Resolusi nama untuk pemanggilan method (`x.f()`), beda tipis dari
    /// [`Self::get_field`] tapi bedanya penting.
    pub(crate) fn get_method(&self, obj: &VmValue, name: &str, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        // Hanya array dan string — interpreter menolak `(1,2).len()` dan
        // menyuruh pakai `len(t)`, jadi VM tidak boleh lebih longgar.
        // Handle asing: nama method-nya milik worker, jadi tidak ada yang bisa dicari di sini.
        if matches!(obj, VmValue::Asing(a) if a.ke_nilai().is_some()) {
            return Ok(VmValue::Asing(super::ffi_asing::Asing::Metode(name.into())));
        }
        let intrinsik = matches!(obj, VmValue::Array(_) | VmValue::Str(_)) && name == "len";
        if intrinsik {
            if let Some(f) = bawaan("len") {
                return Ok(f);
            }
        }
        // `len` dan `is_empty` adalah PSEUDO-FIELD di pintu field (`s.is_empty`
        // menjawab bool). Di pintu METHOD keduanya fungsi biasa — tanpa baris
        // ini `s.is_empty()` memanggil sebuah `bool`.
        if name == "is_empty" {
            let ns = match obj {
                VmValue::Array(_) => Some("arr"),
                VmValue::Str(_) => Some("str"),
                _ => None,
            };
            if let Some(f) = ns.and_then(|n| bawaan(&format!("{}::{}", n, name))) {
                return Ok(f);
            }
        }
        // Di pintu METHOD, method milik tipenya menang atas field bernama sama.
        if let VmValue::Map(m) = obj {
            // Dict biasa ikut: `{"len": 5}.len()` dijawab `1` (jumlah entri) oleh mesin
            // acuan, bukan "5 bukan fungsi".
            if let Some(f) = self.metode_tipe(m.nama_tipe(), name) {
                return Ok(f);
            }
            // FIELD bernama sama, tanpa method: dulu isinya dipanggil dengan
            // penerimanya disisipkan sebagai argumen pertama — dan BERHASIL.
            let punya_field = m.lock().unwrap().get(name)
                .is_some_and(|v| v.bisa_dipanggil());
            if punya_field {
                let t = m.nama_tipe().unwrap_or("dict").to_string();
                return Err(VmError::at(
                    crate::interpreter::galat_index::field_fungsi_bukan_metode(&t, name), sp));
            }
        }
        self.get_field_pintu(obj, name, sp, true)
    }

    /// Jumlah argumen method bawaan yang DITULIS — kurang maupun lebih — dari
    /// definisi yang sama dengan mesin acuan. Pemanggilnya menyaring dulu bahwa
    /// yang terpilih fungsi native: method `impl` pemakai memeriksa parameternya sendiri.
    pub(crate) fn pesan_jumlah_arg(obj: &VmValue, name: &str, argc: usize) -> Option<String> {
        if name == "to_string" {
            return crate::interpreter::pesan_jumlah_arg_metode("*", name, argc);
        }
        let ns = match obj {
            VmValue::Int(_) => return crate::interpreter::pesan_arg_metode_skalar("int", name, argc),
            VmValue::Float(_) => return crate::interpreter::pesan_arg_metode_skalar("float", name, argc),
            VmValue::Array(_) => "arr",
            VmValue::Str(_) => "str",
            VmValue::Channel(_) => "chan",
            VmValue::Canvas(_) => "canvas",
            VmValue::Sinkron(p) => p.ns_metode()?,
            VmValue::Map(m) => match &m.label {
                Label::Dict => "dict",
                Label::Enum { tipe, .. } if matches!(tipe.as_ref(), "Option" | "Result") => tipe.as_ref(),
                _ => return None,
            },
            _ => return None,
        };
        crate::interpreter::pesan_jumlah_arg_metode(ns, name, argc)
    }

    /// Method yang dimiliki penerima SKALAR di mesin acuan, dipetakan ke fungsi
    /// yang sudah terdaftar. Daftarnya cerminan `interpreter/metode.rs`, dan
    /// dikunci sapuan pintu method — bukan dijaga ingatan.
    fn fungsi_metode_bawaan(obj: &VmValue, name: &str) -> Option<String> {
        // `to_string` ada untuk SEMUA penerima; lewat nama internal supaya ia
        // tidak jadi fungsi telanjang yang acuan tidak punya.
        if name == "to_string" {
            return Some("__to_string".to_string());
        }
        const MATEMATIKA: &[&str] = &[
            "abs", "ceil", "cos", "exp", "floor", "is_finite", "is_inf", "is_nan",
            "ln", "log10", "pow", "round", "sin", "sqrt", "tan",
        ];
        match obj {
            VmValue::Float(_) if MATEMATIKA.contains(&name) => Some(format!("math::{}", name)),
            // Int cuma `abs` di acuan — `5.sqrt()` ditolak di sana.
            VmValue::Int(_) if name == "abs" => Some("math::abs".to_string()),
            _ => None,
        }
    }

    /// Method per-tipe MENANG; `fungsi_metode_bawaan` cuma cadangan. Urutannya
    /// begitu supaya `impl D { fn to_string(self) }` milik pemakai tidak pernah
    /// tertutup oleh bawaan.
    pub(crate) fn get_field(&self, obj: &VmValue, name: &str, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        self.get_field_pintu(obj, name, sp, false)
    }

    /// `pintu_metode` membedakan `x.m()` dari `x.m`. Tanpa pembedaan itu pintu
    /// field menyerahkan methodnya sebagai nilai, sementara acuan memanggilnya.
    pub(crate) fn get_field_pintu(&self, obj: &VmValue, name: &str, sp: Option<crate::lexer::Span>,
                                  pintu_metode: bool) -> VmResult<VmValue> {
        if !pintu_metode {
            let tn = obj.type_name();
            if matches!(obj, VmValue::Asing(_)) {
                return Err(VmError::at(crate::interpreter::galat_index::field_handle_asing(tn, name), sp));
            }
            if !crate::interpreter::galat_index::pseudo_field(&tn, name) {
                if let Some(ns) = crate::interpreter::galat_index::ns_metode_bawaan(&tn) {
                    if crate::vm::ada_builtin(&format!("{}::{}", ns, name)) {
                        return Err(VmError::at(
                            crate::interpreter::galat_index::metode_tanpa_kurung(&tn, name), sp));
                    }
                }
            }
        }
        match self.get_field_khusus(obj, name, sp) {
            Ok(v) => Ok(v),
            Err(e) => Self::fungsi_metode_bawaan(obj, name)
                .and_then(|g| bawaan(&g))
                .ok_or(e),
        }
    }

    fn get_field_khusus(&self, obj: &VmValue, name: &str, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        match obj {
            VmValue::Map(m) => {
                // 1. Field langsung
                if let Some(v) = m.lock().unwrap().get(name).cloned() { return Ok(v); }
                // 2.
                let ns = m.nama_tipe().unwrap_or("dict");
                // Namanya METHOD, dan di sini ditulis tanpa `()`. Dulu ia
                // diserahkan sebagai nilai sementara acuan MEMANGGILNYA — satu
                // program, dua arti. Sekarang ditolak di dua mesin.
                if self.metode_tipe(m.nama_tipe(), name).is_some() {
                    return Err(VmError::at(
                        crate::interpreter::galat_index::metode_tanpa_kurung(ns, name), sp));
                }
                // Tidak ada langkah ketiga.
                if let Label::Enum { tipe, .. } = &m.label {
                    let mut calon: Vec<String> = crate::interpreter::calon_method_enum(tipe)
                        .iter().map(|s| s.to_string()).collect();
                    calon.extend(self.nama_metode_tipe(tipe));
                    let saran = crate::diag::saran_nama_terdekat(
                        name, calon.iter().map(|s| s.as_str()));
                    return Err(VmError::at(crate::interpreter::pesan_method_tak_ada(
                        &format!("enum:{}", tipe), name, "", saran.as_deref()), sp));
                }
                Err({
                    let tipe = match &m.label {
                        Label::Struct(n) => format!("struct {}", n),
                        _ => "dict".to_string(),
                    };
                    // Sebut field/key yang MEMANG ada — pembacanya biasanya salah ketik
                    // key, bukan salah ketik method.
                    let mut punya: Vec<String> = m.lock().unwrap().keys().cloned().collect();
                    punya.sort();
                    let tambahan = if punya.is_empty() { String::new() } else {
                        format!(" (kalau maksudnya field/key, yang ada: {})", punya.join(", "))
                    };
                    let calon = self.nama_metode_tipe(ns);
                    let saran = crate::diag::saran_nama_terdekat(name, calon.iter().map(|s| s.as_str()));
                    VmError::at(crate::interpreter::pesan_method_tak_ada(
                        &tipe, name, &tambahan, saran.as_deref()), sp)
                })
            }
            // Method kanvas dicari di namespace `canvas::`, pola yang sama
            // dengan array (`arr::`) dan channel (`chan::`).
            VmValue::Canvas(_) => bawaan(&format!("canvas::{}", name))
                .ok_or_else(|| VmError::at(self.pesan_method_hilang("canvas", "canvas", name), sp)),
            VmValue::Channel(_) if !BUKAN_METODE_CHAN.contains(&name) =>
                bawaan(&format!("chan::{}", name))
                    .ok_or_else(|| VmError::at(self.pesan_method_hilang("channel", "chan", name), sp)),
            VmValue::Channel(_) =>
                Err(VmError::at(self.pesan_method_hilang("channel", "chan", name), sp)),
            VmValue::Sinkron(p) if p.ns_metode().is_some() => {
                let ns = p.ns_metode().unwrap_or("");
                bawaan(&format!("{}::{}", ns, name)).filter(|_| crate::sinkron::metode_ada(ns, name))
                    .ok_or_else(|| VmError::at(self.pesan_method_hilang(p.nama_tipe(), ns, name), sp))
            }
            VmValue::Array(a) => match name {
                "len"  => Ok(VmValue::Int(a.lock().unwrap().len() as i128)),
                // Pseudo-field, sama dengan acuan: `a.is_empty` menjawab NILAI,
                // bukan fungsi — didaftar di `galat_index::pseudo_field`.
                "is_empty" => Ok(VmValue::Bool(a.lock().unwrap().is_empty())),
                // `a.fill(v)` MENGGANTI tiap elemen dengan `v` dan mempertahankan
                // panjangnya; `arr::fill(v, n)` MEMBUAT array baru sepanjang `n`.
                "fill" | "starts_with" | "ends_with" =>
                    bawaan(&format!("arr::__metode_{}", name))
                        .ok_or_else(|| VmError::at(self.pesan_method_hilang("array", "arr", name), sp)),
                // Terdaftar sebagai builtin sejak HOF bisa jadi nilai, tapi bukan method.
                _ if BUKAN_METODE_ARR.contains(&name) =>
                    Err(VmError::at(self.pesan_method_hilang("array", "arr", name), sp)),
                _ => bawaan(&format!("arr::{}", name))
                        .ok_or_else(|| VmError::at(self.pesan_method_hilang("array", "arr", name), sp)),
            }
            VmValue::Str(s) => match name {
                "len"  => Ok(VmValue::Int(s.chars().count() as i128)),
                "is_empty" => Ok(VmValue::Bool(s.is_empty())),
                // Ejaan kedua yang ada di acuan sebagai method dan tidak punya
                // bentuk `str::` sama sekali.
                "parse_int"   => bawaan("str::to_int")
                        .ok_or_else(|| VmError::at(self.pesan_method_hilang("string", "str", name), sp)),
                "parse_float" => bawaan("str::to_float")
                        .ok_or_else(|| VmError::at(self.pesan_method_hilang("string", "str", name), sp)),
                // `str::join(arr, sep)` menerima ARRAY di posisi pertama, jadi ia bukan
                // method sebuah string: `"halo".join(",")` dulu dijawab "argumen ke-1 harus …
                _ if BUKAN_METODE_STR.contains(&name) =>
                    Err(VmError::at(self.pesan_method_hilang("string", "str", name), sp)),
                _ => bawaan(&format!("str::{}", name))
                        .ok_or_else(|| VmError::at(self.pesan_method_hilang("string", "str", name), sp)),
            }
            VmValue::Tuple(t) => { let t = crate::vm::value::isi_tuple(t); match name {
                // Tidak ada `.len` untuk tuple: interpreter pun menolaknya
                // (pakai `len(t)`). Yang ada cuma indeks numerik.
                // `t.0` / `t.1` — indeks tuple, seperti di interpreter.
                _ if name.chars().all(|c| c.is_ascii_digit()) && !name.is_empty() => {
                    let i: usize = name.parse().map_err(|_| VmError::at(
                        format!("index tuple `{}` kegedean", name), sp))?;
                    t.get(i).cloned().ok_or_else(|| VmError::at(format!(
                        "index tuple .{} di luar batas — tuple ini isinya {} elemen \
                         (index valid: 0..{})",
                        i, t.len(), t.len().saturating_sub(1)
                    ), sp))
                }
                // Tuple tidak punya namespace method, jadi tidak ada calon
                // untuk disarankan — yang sah cuma indeks numerik.
                _ => Err(VmError::at(crate::interpreter::pesan_method_tak_ada(
                        "tuple", name,
                        " (yang ada untuk tuple cuma indeks numerik: `t.0`, `t.1`, ...)",
                        None), sp)),
            } }
            // Skalar punya method per tipe (`abs`, `sqrt`, `to_string`). Nama PROTOKOL
            // mencari calonnya, nama LENGKAP masuk kalimat (`enum_ctor:Option::Some`).
            _ => {
                let tipe = obj.type_name_penuh();
                if let Some(jalan) = crate::interpreter::padanan_math(obj.type_name(), name) {
                    return Err(VmError::at(
                        crate::interpreter::pesan_method_tak_ada_dgn_jalan(
                            &tipe, name, &jalan), sp));
                }
                let calon = crate::interpreter::calon_method_skalar(obj.type_name());
                let saran = crate::diag::saran_nama_terdekat(name, calon.iter().copied());
                Err(VmError::at(crate::interpreter::pesan_method_tak_ada(
                    &tipe, name, "", saran.as_deref()), sp))
            }
        }
    }

    /// Method `tipe::name` untuk nilai ber-label (struct, enum, dict): `impl` milik
    /// PROGRAM menang, lalu bawaan — untuk dict cuma nama di `METODE_DICT`. Penerima
    /// bawaan lain (array, string, …) tidak lewat sini: mereka selalu bawaan.
    pub(crate) fn metode_tipe(&self, tipe: Option<&str>, name: &str) -> Option<VmValue> {
        let kunci = format!("{}::{}", tipe.unwrap_or("dict"), name);
        if let Some(f) = self.globals.get(&kunci) { return Some(f.clone()); }
        let boleh = tipe.is_some()
            || crate::interpreter::builtin::dict::METODE_DICT.contains(&name);
        if boleh { bawaan(&kunci) } else { None }
    }

    /// Method milik PROGRAM untuk tipe `tipe` (`impl`, warisan, default trait) — semuanya
    /// sudah jadi global `Tipe::nama` saat unitnya dimuat, jadi ini satu pencarian.
    pub(crate) fn metode_pemakai(&self, tipe: &str, name: &str) -> Option<VmValue> {
        self.globals.get(&format!("{}::{}", tipe, name)).cloned()
    }

    /// Nama method yang bisa disarankan untuk tipe ber-label `tipe`.
    fn nama_metode_tipe(&self, tipe: &str) -> Vec<String> {
        let awalan = format!("{}::", tipe);
        let dict = tipe == "dict";
        self.globals.keys().map(|k| k.as_str())
            .chain(crate::vm::bawaan::tabel_bawaan().keys().map(|k| k.as_str())
                .filter(|k| !dict || k.strip_prefix("dict::").is_some_and(
                    |m| crate::interpreter::builtin::dict::METODE_DICT.contains(&m))))
            .filter_map(|k| k.strip_prefix(&awalan))
            .filter(|k| !k.starts_with("__") && !k.contains("::"))
            .map(|k| k.to_string())
            .collect()
    }

    /// Kalimat "penerima ini tidak punya method bernama X", dengan calon saran
    /// **diturunkan dari `globals` yang sedang hidup**.
    pub(crate) fn pesan_method_hilang(&self, tipe: &str, ns: &str, name: &str) -> String {
        let calon = crate::interpreter::calon_method(ns);
        let saran = crate::diag::saran_nama_terdekat(name, calon.iter().map(|s| s.as_str()));
        crate::interpreter::pesan_method_tak_ada(tipe, name, "", saran.as_deref())
    }

    pub(crate) fn set_field(&self, obj: &VmValue, name: &str, val: VmValue, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        if let Some(i) = Self::field_tuple(obj, name) {
            return self.set_index(obj, &VmValue::Int(i), val, sp);
        }
        match obj {
            // Enum tidak punya field yang bisa ditulis: `o.variant = ..` dulu mengganti variannya.
            VmValue::Map(m) if !matches!(m.label, Label::Enum { .. }) => {
                // Field yang tidak dideklarasikan ditolak untuk struct yang dideklarasikan
                // PROGRAM — satu aturan dengan interpreter.
                if let Label::Struct(tipe) = &m.label {
                    if crate::interpreter::galat_index::HANDLE_PUSTAKA.contains(&tipe.as_ref())
                        && crate::vm::visibilitas::field_dideklarasikan(tipe).is_none()
                    {
                        return Err(VmError::at(
                            crate::interpreter::galat_index::tulis_handle(tipe, name), sp));
                    }
                    let belum_ada = !m.lock().unwrap().contains_key(name);
                    if belum_ada {
                        if let Some(dideklarasikan) =
                            crate::vm::visibilitas::field_dideklarasikan(tipe)
                        {
                            if !dideklarasikan.iter().any(|f| f == name) {
                                let saran = crate::diag::saran_nama_terdekat(
                                    name, dideklarasikan.iter().map(|x| x.as_str()));
                                return Err(VmError::at(
                                    crate::interpreter::galat_index::field_tak_dideklarasikan(
                                        tipe, name, saran.as_deref()), sp));
                            }
                        }
                    }
                }
                m.lock().unwrap().insert(name.to_string(), val);
                Ok(())
            }
            _ => Err(VmError::at(
                crate::interpreter::galat_index::bukan_pemilik_field(&obj.type_name_penuh(), name), sp)),
        }
    }

/// Kalimat untuk key/field yang tidak ada di sebuah dict atau struct.
fn pesan_map_hilang(label: &Label, g: &std::collections::HashMap<String, VmValue>, k: &str) -> String {
    let ada: Vec<String> = g.keys().cloned().collect();
    match label {
        Label::Struct(t) => crate::interpreter::galat_index::field_hilang(t, k, &ada),
        _ => crate::interpreter::galat_index::key_hilang(k, &ada),
    }
}

    pub(crate) fn get_index(&self, obj: &VmValue, idx: &VmValue, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        match (obj, idx) {
            (VmValue::Array(a), VmValue::Int(i)) => {
                let v = a.lock().unwrap();
                let i = norm_idx(*i, v.len(), "array", "elemen", sp)?;
                Ok(v[i].clone())
            }
            (VmValue::Tuple(t), VmValue::Int(i)) => {
                let t = t.lock().unwrap_or_else(|p| p.into_inner());
                let i = norm_idx(*i, t.len(), "tuple", "elemen", sp)?;
                Ok(t[i].clone())
            }
            (VmValue::Str(s), VmValue::Int(i)) => {
                let chars: Vec<char> = s.chars().collect();
                let i = norm_idx(*i, chars.len(), "string", "karakter", sp)?;
                // Satu karakter dari string adalah RUNE, bukan string
                // sepanjang satu — sama seperti interpreter, sehingga
                // `"halo"[-1] == 'o'` bernilai benar di kedua backend.
                Ok(VmValue::Rune(chars[i]))
            }
            // Enum bukan wadah: `o["variant"]` dulu membaca label yang tersimpan sebagai kunci.
            (VmValue::Map(m), _) if matches!(m.label, Label::Enum { .. }) => Err(VmError::at(
                crate::interpreter::galat_index::bukan_wadah(&obj.type_name_penuh()), sp)),
            (VmValue::Map(m), key) => {
                let k = format!("{}", key);
                let g = m.lock().unwrap_or_else(|p| p.into_inner());
                match g.get(&k) {
                    Some(v) => Ok(v.clone()),
                    None => Err(VmError::at(Self::pesan_map_hilang(&m.label, &g, &k), sp)),
                }
            }
            // `a[0..2]` — sebuah Range yang sudah terlanjur jadi array biasa.
            // Diberi kalimatnya sendiri karena gejalanya tidak menyerupai
            // sebabnya sama sekali.
            (o, VmValue::Array(_)) => Err(VmError::at(
                crate::interpreter::galat_index::range_dikira_potongan(&o.type_name_penuh()),
                sp,
            )),
            (o, i) => Err(VmError::at(
                if matches!(o, VmValue::Array(_) | VmValue::Str(_) | VmValue::Tuple(_)) {
                    // `type_name_penuh`, bukan `type_name`: yang kedua menjawab `map`
                    // untuk index dict — kata yang tidak ada di bahasa ini.
                    crate::interpreter::galat_index::tipe_index_salah(
                        &o.type_name_penuh(), &i.type_name_penuh())
                } else {
                    crate::interpreter::galat_index::bukan_wadah(&o.type_name_penuh())
                },
                sp,
            )),
        }
    }

    /// Satu langkah menurun lewat `[...]` di JALUR TARGET sebuah assignment.
    pub(crate) fn pinjam_index(&self, induk: &VmValue, idx: &VmValue, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        match (induk, idx) {
            (VmValue::Array(a), VmValue::Int(i)) => {
                let mut v = a.lock().unwrap_or_else(|p| p.into_inner());
                let i = norm_idx(*i, v.len(), "array", "elemen", sp)?;
                v[i].pisahkan();
                Ok(v[i].clone())
            }
            // Tuple ditulis seperti array (Fase TULIS-TUPLE): elemennya dipisahkan DI TEMPAT,
            // jadi salinan tuple yang lain tidak ikut berubah.
            (VmValue::Tuple(a), VmValue::Int(i)) => {
                let mut v = a.lock().unwrap_or_else(|p| p.into_inner());
                let i = norm_idx(*i, v.len(), "tuple", "elemen", sp)?;
                v[i].pisahkan();
                Ok(v[i].clone())
            }
            (VmValue::Tuple(_), i) => Err(VmError::at(
                crate::interpreter::galat_index::tipe_index_salah("tuple", &i.type_name_penuh()), sp)),
            (VmValue::Map(m), key) => {
                let k = format!("{}", key);
                if let Some(slot) = m.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&k) {
                    slot.pisahkan();
                    return Ok(slot.clone());
                }
                // MENULIS lewat sesuatu yang belum ada bukan situasi yang sama dengan
                // MEMBACA sesuatu yang tidak ada. Enum tidak punya anggota, jadi selalu ke sini.
                Err(VmError::at(Self::pesan_tulis_hilang(&induk.type_name_penuh(), &k, true), sp))
            }
            // Array ber-index bukan int memakai kalimat jalur baca, sama seperti interpreter.
            (VmValue::Array(_), i) => Err(VmError::at(
                crate::interpreter::galat_index::tipe_index_salah("array", &i.type_name_penuh()), sp)),
            // String bisa DIBACA lewat `[...]`, tidak bisa DITULIS.
            _ => Err(VmError::at(
                crate::interpreter::galat_index::tak_bisa_ditulis_lewat_index(&induk.type_name_penuh()), sp)),
        }
    }

    /// `.N` atas sebuah tuple di jalur tulis, dibaca sebagai `[N]` — sama dengan interpreter.
    fn field_tuple(obj: &VmValue, name: &str) -> Option<i128> {
        if !matches!(obj, VmValue::Tuple(_)) || name.is_empty()
            || !name.chars().all(|c| c.is_ascii_digit()) { return None; }
        name.parse::<i128>().ok()
    }

    /// Satu langkah menurun lewat `.field` di jalur target — pasangan
    /// `pinjam_index` untuk field, dengan alasan penulisan balik yang sama.
    pub(crate) fn pinjam_field(&self, induk: &VmValue, name: &str, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        if let Some(i) = Self::field_tuple(induk, name) {
            return self.pinjam_index(induk, &VmValue::Int(i), sp);
        }
        match induk {
            VmValue::Map(m) => {
                if let Some(slot) = m.lock().unwrap_or_else(|p| p.into_inner()).get_mut(name) {
                    slot.pisahkan();
                    return Ok(slot.clone());
                }
                // Enum tidak punya field sama sekali, jadi selalu sampai ke sini.
                Err(VmError::at(Self::pesan_tulis_hilang(&induk.type_name_penuh(), name, false), sp))
            }
            _ => Err(VmError::at(
                crate::interpreter::galat_index::bukan_pemilik_field(&induk.type_name_penuh(), name), sp)),
        }
    }

    /// Menulis lewat anggota yang belum ada, dari TIPE wadahnya — satu kalimat
    /// dengan interpreter. Enum tidak punya anggota yang bisa ditulis sama sekali.
    fn pesan_tulis_hilang(tipe: &str, nama: &str, lewat_index: bool) -> String {
        use crate::interpreter::galat_index::{
            belum_ada_untuk_ditulis, bukan_pemilik_field, sebutan_anggota, tak_bisa_ditulis_lewat_index,
        };
        match (tipe.starts_with("enum:"), lewat_index) {
            (true, true) => tak_bisa_ditulis_lewat_index(tipe),
            (true, false) => bukan_pemilik_field(tipe, nama),
            (false, _) => belum_ada_untuk_ditulis(sebutan_anggota(tipe == "dict"), nama),
        }
    }

    pub(crate) fn set_index(&self, obj: &VmValue, idx: &VmValue, val: VmValue, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        match (obj, idx) {
            (VmValue::Array(a), VmValue::Int(i)) => {
                let mut v = a.lock().unwrap();
                let i = norm_idx(*i, v.len(), "array", "elemen", sp)?;
                v[i] = val;
                Ok(())
            }
            (VmValue::Tuple(a), VmValue::Int(i)) => {
                let mut v = a.lock().unwrap_or_else(|p| p.into_inner());
                let i = norm_idx(*i, v.len(), "tuple", "elemen", sp)?;
                v[i] = val;
                Ok(())
            }
            (VmValue::Tuple(_), i) => Err(VmError::at(
                crate::interpreter::galat_index::tipe_index_salah("tuple", &i.type_name_penuh()), sp)),
            (VmValue::Map(m), _) if !matches!(m.label, Label::Enum { .. }) => {
                m.lock().unwrap().insert(format!("{}", idx), val);
                Ok(())
            }
            // Kalimat yang sama dengan langkah tengah (`pinjam_index`) dan dengan interpreter.
            (VmValue::Array(_), i) => Err(VmError::at(
                crate::interpreter::galat_index::tipe_index_salah("array", &i.type_name_penuh()), sp)),
            _ => Err(VmError::at(
                crate::interpreter::galat_index::tak_bisa_ditulis_lewat_index(&obj.type_name_penuh()), sp)),
        }
    }

    /// Menyelesaikan satu batas potongan terhadap panjang.
    pub(crate) fn batas_potongan(
        &self,
        nilai: Option<i128>,
        bawaan: usize,
        panjang: usize,
        sisi: &str,
        sp: Option<crate::lexer::Span>,
    ) -> VmResult<usize> {
        let Some(i) = nilai else { return Ok(bawaan) };
        let diselesaikan = if i < 0 { i + panjang as i128 } else { i };
        if diselesaikan < 0 || diselesaikan > panjang as i128 {
            return Err(VmError::at(
                format!(
                    "batas {} potongan [{}] di luar jangkauan — panjangnya {}, jadi batas yang sah 0..{} (atau -1..-{} kalau dihitung dari belakang)",
                    sisi, i, panjang, panjang, panjang
                ),
                sp,
            ));
        }
        Ok(diselesaikan as usize)
    }

    pub(crate) fn slice_value(
        &self,
        obj: &VmValue,
        lo: Option<i128>,
        hi: Option<i128>,
        sp: Option<crate::lexer::Span>,
    ) -> VmResult<VmValue> {
        let panjang = match obj {
            VmValue::Array(a) => a.lock().unwrap_or_else(|p| p.into_inner()).len(),
            VmValue::Str(s) => s.chars().count(),
            other => {
                return Err(VmError::at(
                    format!("tidak bisa slice `{}`", other.type_name_penuh()),
                    sp,
                ))
            }
        };
        let awal = self.batas_potongan(lo, 0, panjang, "awal", sp)?;
        let akhir = self.batas_potongan(hi, panjang, panjang, "akhir", sp)?;
        if awal > akhir {
            return Err(VmError::at(
                format!(
                    "potongan [{}:{}] terbalik — batas awal ({}) lebih besar dari batas akhir ({}); tulis yang kecil dulu, mis. [{}:{}]",
                    awal, akhir, awal, akhir, akhir, awal
                ),
                sp,
            ));
        }
        match obj {
            VmValue::Array(a) => {
                let a = a.lock().unwrap_or_else(|p| p.into_inner());
                Ok(VmValue::array(a[awal..akhir].to_vec()))
            }
            VmValue::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                Ok(VmValue::str(chars[awal..akhir].iter().collect::<String>()))
            }
            _ => unreachable!("tipe sudah diperiksa di atas"),
        }
    }

    pub(crate) fn vm_len(&self, val: &VmValue, sp: Option<crate::lexer::Span>) -> VmResult<usize> {
        match val {
            VmValue::Str(s)     => Ok(s.chars().count()),
            VmValue::Array(a)   => Ok(a.lock().unwrap().len()),
            VmValue::Tuple(t)   => Ok(t.lock().unwrap_or_else(|p| p.into_inner()).len()),
            VmValue::Map(m)     => Ok(m.lock().unwrap().len()),
            VmValue::Channel(c) => Ok(c.len()),
            _ => Err(VmError::at(format!("len() tidak bisa untuk `{}`", val.type_name_penuh()), sp)),
        }
    }
}
