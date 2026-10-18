// src/vm/bawaan.rs — builtin yang VM layani sendiri

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use super::value::{VmValue, VmNativeFn};
use super::channel::VmChannel;
use super::vm::*;

/// Argumen pertama sebuah `str::` di sisi VM — kalimatnya satu definisi dengan
/// acuan (`pesan_arg_harus_teks`), jadi pintu ini tidak bisa menyimpang darinya.
fn teks_vm(v: Option<&VmValue>, f: &str) -> Result<String, String> {
    match v {
        Some(VmValue::Str(s)) => Ok(s.to_string()),
        Some(VmValue::Rune(c)) => Ok(c.to_string()),
        Some(o) => Err(crate::interpreter::pesan_arg_harus_teks(
            f, "argumen pertama", &o.type_name_penuh())),
        None => Err(format!("{}: kurang argumen", f)),
    }
}

/// Isi tabel bawaan: fungsi native, atau angka tetap (`math::PI`).
#[derive(Clone, Copy)]
pub(crate) enum Bawaan {
    Fungsi(VmNativeFn),
    Pecahan(f64),
}

impl Bawaan {
    pub(crate) fn nilai(self) -> VmValue {
        match self {
            Bawaan::Fungsi(f) => VmValue::NativeFn(f),
            Bawaan::Pecahan(x) => VmValue::Float(x),
        }
    }
}

/// Bawaan bahasa, dibangun SEKALI per proses dan tidak pernah berubah. Dulu tinggal di
/// `Vm::globals` bersama deklarasi program, jadi `fn len` atau modul bernama `str`
/// menimpa builtin-nya untuk SEMUA unit, termasuk modul `std/`.
pub(crate) fn tabel_bawaan() -> &'static crate::interpreter::arity::PetaFnv<String, Bawaan> {
    static T: std::sync::OnceLock<crate::interpreter::arity::PetaFnv<String, Bawaan>> =
        std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut vm = Vm::new();
        vm.register_natives();
        vm.globals.drain().filter_map(|(k, v)| match v {
            VmValue::NativeFn(f) => Some((k, Bawaan::Fungsi(f))),
            VmValue::Float(x) => Some((k, Bawaan::Pecahan(x))),
            // `register_natives` cuma mendaftarkan dua bentuk itu; tes unit menjaganya.
            _ => None,
        }).collect()
    })
}

/// Bawaan bernama `nama`, sebagai nilai.
pub(crate) fn bawaan(nama: &str) -> Option<VmValue> {
    tabel_bawaan().get(nama).map(|b| b.nilai())
}

/// Seperti [`bawaan`], plus konstruktor `Option`/`Result` — untuk `::Some`, yang di
/// acuan memang nilai bawaan biasa.
pub(crate) fn bawaan_atau_konstruktor(nama: &str) -> Option<VmValue> {
    bawaan(nama).or_else(|| match nama {
        "Some" | "Option::Some" => Some(VmValue::ktor("Option", "Some", 1)),
        "None" | "Option::None" => Some(VmValue::none()),
        "Ok" | "Result::Ok" => Some(VmValue::ktor("Result", "Ok", 1)),
        "Err" | "Result::Err" => Some(VmValue::ktor("Result", "Err", 1)),
        _ => None,
    })
}

impl Vm {
    // ── Native stdlib ─────────────────────────────────────────────────────

    // ─── Defer execution ─────────────────────────────────────────────────────

    /// Jalankan semua `defer` milik bingkai teratas, LIFO. `galat` adalah galat yang sedang membawa
    /// bingkainya keluar, kalau ada. Defer yang gagal tidak menghentikan defer berikutnya: galatnya
    /// menggantikan hasil yang sukses, atau jadi catatan di galat yang sudah ada — seperti acuan.
    pub(crate) fn run_defers(
        &mut self, sp: Option<crate::lexer::Span>, mut galat: Option<VmError>,
    ) -> Option<VmError> {
        let defers = match self.frames.last_mut() {
            Some(f) => std::mem::take(&mut f.defers),
            None => return galat,
        };
        for (defer_fn, args) in defers.into_iter().rev() {
            let Err(d) = self.call_value_immediate(defer_fn, args) else { continue };
            galat = Some(match galat {
                None => d.isi_span_bila_kosong(sp),
                Some(mut asli) => {
                    asli.msg = crate::interpreter::pesan_defer_ikut_gagal(&asli.msg, &d.msg);
                    asli
                }
            });
        }
        galat
    }

    pub fn def(&mut self, name: &str, f: VmNativeFn) {
        self.globals.insert(name.to_string(), VmValue::NativeFn(f));
    }

    pub(crate) fn register_natives(&mut self) {
        use VmValue::*;

        // I/O
        self.def("wok",    |a| { crate::host::tulis_baris(&a.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ")); Ok(VmValue::Unit) });
        self.def("wok_",   |a| { crate::host::tulis(&a.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ")); crate::host::siram(); Ok(VmValue::Unit) });
        self.def("tok",    |a| {
            if !a.is_empty() {
                crate::host::tulis(&a.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" "));
                crate::host::siram();
            }
            let mut sp = String::new();
            match crate::host::baca_baris(&mut sp) {
                Ok(_) => { if sp.ends_with('\n') { sp.pop(); } if sp.ends_with('\r') { sp.pop(); } Ok(Str(Arc::new(sp))) }
                Err(e) => Err(format!("tok: {}", e)),
            }
        });
        self.def("eprint", |a| { crate::host::galat_baris(&a.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ")); Ok(VmValue::Unit) });
        self.def("sprint", |a| Ok(Str(Arc::new(a.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ")))));
        // `fmt::wok`/`fmt::wok_`/`fmt::tok`/`fmt::eprint`/`fmt::sprint` adalah ALIAS
        // (tabel alias di bawah). Dulu salinan kedua, dan `fmt::eprint` menjawab `nil`.

        // Option/Result bentuk path: argumen pertamanya HARUS tipe yang diminta — dulu varian
        // saja yang dicocokkan, jadi `Option::unwrap_or(5, 9)` menjawab 9 dan
        // `Option::is_some(5)`/`is_none(5)` sama-sama `false`.
        fn bagian<'a>(nama: &str, tipe: &str, v: Option<&'a VmValue>)
            -> Result<(&'a str, Option<&'a VmValue>), String> {
            let Some(v) = v else { return Err(crate::interpreter::pesan_butuh_nilai(nama)) };
            match v.bagian_enum() {
                Some((t, varian, payload)) if t == tipe => Ok((varian, payload)),
                _ => Err(format!("{}: argumen ke-1 harus `{}`, dapat `{}`", nama, tipe, v.type_name_penuh())),
            }
        }
        fn isi(p: Option<&VmValue>) -> VmValue { p.cloned().unwrap_or(Nil) }
        fn vm_some(x: VmValue) -> VmValue { VmValue::some(x) }
        fn vm_none() -> VmValue { VmValue::none() }
        self.def("Option::is_some", |a| Ok(Bool(bagian("Option::is_some", "Option", a.get(0))?.0 == "Some")));
        self.def("Option::is_none", |a| Ok(Bool(bagian("Option::is_none", "Option", a.get(0))?.0 == "None")));
        self.def("Result::is_ok", |a| Ok(Bool(bagian("Result::is_ok", "Result", a.get(0))?.0 == "Ok")));
        self.def("Result::is_err", |a| Ok(Bool(bagian("Result::is_err", "Result", a.get(0))?.0 == "Err")));
        self.def("Option::unwrap", |a| match bagian("Option::unwrap", "Option", a.get(0))? {
            ("Some", p) => Ok(isi(p)),
            _ => Err("unwrap() pada Option::None".to_string()),
        });
        self.def("Result::unwrap", |a| match bagian("Result::unwrap", "Result", a.get(0))? {
            ("Ok", p) => Ok(isi(p)),
            (_, p) => Err(format!("unwrap() pada Result::Err: {}", isi(p))),
        });
        self.def("Option::unwrap_or", |a| match bagian("Option::unwrap_or", "Option", a.get(0))? {
            ("Some", p) => Ok(isi(p)),
            _ => Ok(a.get(1).cloned().unwrap_or(Nil)),
        });
        self.def("Result::unwrap_or", |a| match bagian("Result::unwrap_or", "Result", a.get(0))? {
            ("Ok", p) => Ok(isi(p)),
            _ => Ok(a.get(1).cloned().unwrap_or(Nil)),
        });
        // `.ok()`/`.err()` ada di mesin acuan dan tidak di sini, jadi satu
        // program punya dua nasib. `Option::ok` sengaja identitas: menanyakan
        // "jadikan Option" atas sesuatu yang sudah Option tidak mengubah apa pun.
        self.def("Result::ok", |a| match bagian("Result::ok", "Result", a.get(0))? {
            ("Ok", p) => Ok(VmValue::some(isi(p))),
            _ => Ok(VmValue::none()),
        });
        self.def("Result::err", |a| match bagian("Result::err", "Result", a.get(0))? {
            ("Err", p) => Ok(VmValue::some(isi(p))),
            _ => Ok(VmValue::none()),
        });
        self.def("Option::ok", |a| {
            bagian("Option::ok", "Option", a.get(0))?;
            Ok(a[0].clone())
        });
        self.def("Result::unwrap_err", |a| match bagian("Result::unwrap_err", "Result", a.get(0))? {
            ("Err", p) => Ok(isi(p)),
            _ => Err("unwrap_err() pada Result::Ok".to_string()),
        });
        self.def("Option::expect", |a| match bagian("Option::expect", "Option", a.get(0))? {
            ("Some", p) => Ok(isi(p)),
            _ => Err(a.get(1).map_or_else(|| "expect() gagal".to_string(), |m| format!("{}", m))),
        });
        self.def("Result::expect", |a| match bagian("Result::expect", "Result", a.get(0))? {
            ("Ok", p) => Ok(isi(p)),
            _ => Err(a.get(1).map_or_else(|| "expect() gagal".to_string(), |m| format!("{}", m))),
        });

        // Type coercion Cast yang tidak bermakna DITOLAK, bukan dijawab `0`.
        self.def("int", cast_int);
        self.def("uint", |a| crate::vm::vm::ubah_bulat(
            a.get(0), "uint", crate::interpreter::konversi::Lebar::U128, "uint()"));
        // `x as u8`, `x as i32`, `x as byte`, … — sasarannya dibawa sebagai NOMOR
        // di `konversi::SASARAN_BULAT`, karena `int`/`uint` saja tidak tahu 300
        // tidak muat di `u8`, dan mencocokkan nama tiap cast terukur 2x lebih lambat.
        self.def("__ubah_bulat", |a| {
            let (nama, lebar) = match a.get(1) {
                Some(Int(i)) => crate::interpreter::konversi::SASARAN_BULAT.get(*i as usize).copied(),
                _ => None,
            }.ok_or_else(|| "__ubah_bulat: nomor sasaran tidak dikenal".to_string())?;
            crate::vm::vm::ubah_bulat(a.get(0), nama, lebar, nama)
        });
        // Ke tipe pointer: 0 berarti `nil`, sama seperti interpreter.
        // Literal struct yang namanya belum dikenal saat kompilasi (lihat `Expr::Struct`).
        self.def("__periksa_struct", |a| {
            let Some(Str(nama)) = a.first() else { return Ok(VmValue::Unit) };
            if super::visibilitas::field_dideklarasikan(nama).is_some() { return Ok(VmValue::Unit); }
            let ada = super::visibilitas::nama_struct();
            let saran = crate::diag::saran_nama_terdekat(nama, ada.iter().map(|x| x.as_str()));
            Err(crate::interpreter::galat_index::struct_tak_dideklarasikan(nama, saran.as_deref()))
        });
        // Basis `...` sebuah literal: harus wadah, dan tidak membawa field di luar deklarasi
        // sasarannya. Argumen: basis, nama tipe, daftar field varian (`nil` untuk struct).
        self.def("__periksa_sebar", |a| {
            let (Some(basis), Some(Str(tipe))) = (a.first(), a.get(1)) else { return Ok(VmValue::Unit) };
            let isi = match basis {
                VmValue::Map(b) => match &b.label {
                    super::value::Label::Enum { payload: Some(VmValue::Map(p)), .. }
                        if !matches!(p.label, super::value::Label::Enum { .. }) => p.clone(),
                    super::value::Label::Enum { .. } => return Err(
                        crate::interpreter::pesan_sebar_bukan_wadah(&basis.type_name_penuh())),
                    _ => b.clone(),
                },
                lain => return Err(crate::interpreter::pesan_sebar_bukan_wadah(&lain.type_name_penuh())),
            };
            let sasaran = match a.get(2) {
                Some(Array(v)) => Some((v.lock().unwrap().iter().map(|x| x.to_string()).collect::<Vec<String>>(),
                    format!("varian `{}`", tipe))),
                _ => super::visibilitas::field_dideklarasikan(tipe)
                    .map(|d| (d, format!("struct `{}`", crate::ruang_tipe::bersih(tipe)))),
            };
            if let Some((boleh, sebutan)) = sasaran {
                let peta = isi.lock().unwrap();
                if let Some(f) = crate::interpreter::galat_index::field_sebar_liar(peta.keys(), &boleh) {
                    return Err(crate::interpreter::galat_index::sebar_field_liar(&sebutan, f));
                }
            }
            Ok(basis.clone())
        });
        self.def("__jadi_pointer", |a| {
            use crate::interpreter::konversi as k;
            match a.get(0) {
                Some(Ptr(p)) => Ok(Ptr(*p)),
                Some(Nil)    => Ok(Nil),
                Some(Int(n)) => Ok(k::alamat_dari_bulat(*n)?.map_or(Nil, Ptr)),
                Some(lain)   => Err(k::pesan_tak_bisa_jadi(&lain.type_name_penuh(), "pointer")),
                None         => Err(crate::interpreter::pesan_butuh_nilai("pointer")),
            }
        });
        self.def("rune", |a| {
            use crate::interpreter::konversi as k;
            match a.get(0) {
                Some(Rune(c)) => Ok(Rune(*c)),
                Some(Int(n))  => Ok(Rune(k::kode_ke_rune(*n)?)),
                Some(lain)    => Err(k::pesan_tak_bisa_jadi(&lain.type_name_penuh(), "rune")),
                None          => Err(crate::interpreter::pesan_butuh_nilai("rune()")),
            }
        });
        self.def("float", |a| {
            use crate::interpreter::konversi as k;
            match a.get(0) {
                Some(Int(n))   => Ok(Float(*n as f64)),
                Some(Float(f)) => Ok(Float(*f)),
                Some(Bool(b))  => Ok(Float(*b as u8 as f64)),
                Some(Str(s))   => Ok(Float(k::teks_ke_float(s)?)),
                Some(Nil)      => Err(k::pesan_nil_jadi("float")),
                Some(lain)     => Err(k::pesan_tak_bisa_jadi(&lain.type_name_penuh(), "float")),
                None           => Err(crate::interpreter::pesan_butuh_nilai("float()")),
            }
        });
        // Tanpa argumen DITOLAK, seperti `int()` — dulu `""` di sini, "nil" di acuan.
        self.def("str",  |a| match a.get(0) {
            Some(v) => Ok(Str(Arc::new(format!("{}", v)))),
            None => Err(crate::interpreter::pesan_butuh_nilai("str()")),
        });
        self.def("bool", |a| match a.get(0) {
            Some(v) => Ok(Bool(v.is_truthy())),
            // `int()` dan `float()` sudah menolak; `bool()` menjawab
            // `false`. Acuannya berselisih dengan dirinya sendiri.
            None => Err(crate::interpreter::pesan_butuh_nilai("bool()")),
        });

        // Builtins
        self.def("len", |a| match a.get(0) {
            Some(Str(s))   => Ok(Int(s.chars().count() as i128)),
            Some(Array(v)) => Ok(Int(v.lock().unwrap().len() as i128)),
            Some(Tuple(t)) => Ok(Int(crate::vm::value::isi_tuple(t).len() as i128)),
            Some(Channel(c)) => Ok(Int(c.len() as i128)),
            // Dict adalah koleksi berukuran juga; struct dan enum (yang juga `Map`)
            // sengaja tidak — keduanya bukan koleksi.
            Some(Map(m)) => match m.label {
                crate::vm::value::Label::Dict =>
                    Ok(Int(m.lock().unwrap_or_else(|p| p.into_inner()).len() as i128)),
                _ => Err(crate::interpreter::pesan_len_tak_bisa(
                    &a.get(0).map(|x| x.type_name_penuh()).unwrap_or_default())),
            },
            Some(o) => Err(crate::interpreter::pesan_len_tak_bisa(&o.type_name_penuh())),
            None => Err(crate::interpreter::pesan_butuh_nilai("len()")),
        });
        self.def("typeof", |a| match a.get(0) {
            Some(v) => Ok(Str(Arc::new(crate::ruang_tipe::bersihkan(v.type_name_penuh())))),
            None => Err(crate::interpreter::pesan_butuh_nilai("typeof()")),
        });
        // `terap` didaftarkan supaya NAMANYA ada — badannya tidak pernah jalan,
        // karena ia dicegat lewat alamat di situs pemanggilan …
        self.def("terap", |_a| Err("terap: dicegat di situs pemanggilan".to_string()));
        self.def("terap_bernama",
            |_a| Err("terap_bernama: dicegat di situs pemanggilan".to_string()));
        // Dipanggil compiler tepat sesudah `main()` selesai (lihat
        // `compile_unit`). Pesannya disamakan dengan interpreter.
        self.def("__periksa_hasil_main__", |a| {
            let v = a.get(0).unwrap_or(&Nil);
            if let Some((ty, varian, muatan)) = Vm::tagged_parts(v) {
                if varian == "Err" || varian == "None" || varian == "Error" {
                    let isi = muatan.map(|p| format!(": {}", p)).unwrap_or_default();
                    return Err(format!(
                        "program berhenti karena `{}::{}` merambat keluar dari `main`{} \
                         — tangani dengan `match` di dalam `main`, atau pakai \
                         `.unwrap_or(...)`, kalau kegagalan ini memang boleh terjadi",
                        ty, varian, isi
                    ));
                }
            }
            Ok(Nil)
        });
        // Satu-satunya cara kode JRG melempar galat sendiri.
        self.def("panik", |a| {
            Err(match a.first() {
                None => "panik() tanpa alasan — sebutkan apa yang salah".to_string(),
                Some(v) => format!("{}", v),
            })
        });
        // Teksnya TIDAK ditulis ulang di sini.
        self.def("assert", |a| {
            if a.is_empty() {
                return Err(crate::interpreter::pesan_assert_kurang("assert", 1, 0));
            }
            if a.get(0).map(|v| v.is_truthy()).unwrap_or(false) { return Ok(VmValue::Unit); }
            let msg = a.get(1).map(|v| format!("{}", v));
            Err(crate::interpreter::pesan_assert(msg.as_deref()))
        });
        self.def("assert_eq", |a| {
            if a.len() < 2 {
                return Err(crate::interpreter::pesan_assert_kurang("assert_eq", 2, a.len()));
            }
            let kiri  = a.get(0).cloned().unwrap_or(Nil);
            let kanan = a.get(1).cloned().unwrap_or(Nil);
            if kiri.vm_eq(&kanan)? { return Ok(VmValue::Unit); }
            let tambahan = a.get(2).map(|v| format!("{}", v));
            Err(crate::interpreter::pesan_assert_eq(
                &kiri.to_string(),  &kiri.type_name_penuh(),
                &kanan.to_string(), &kanan.type_name_penuh(),
                tambahan.as_deref()))
        });
        self.def("num_cpus", |_| {
            Ok(Int(std::thread::available_parallelism()
                .map(|count| count.get() as i128)
                .unwrap_or(1)))
        });
        self.def("crypto::md5", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_md5));
        self.def("crypto::sha1", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_sha1));
        self.def("crypto::sha256", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_sha256));
        // Satu definisi (`dok::crypto::f_*`) untuk dua mesin: salinan kedua di sini dulu
        // menjawab `Err` dengan kalimat lain (`aes_*`), di tabel yang tidak membandingkannya.
        self.def("crypto::sha512", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_sha512));
        self.def("crypto::hmac_sha256", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_hmac_sha256));
        self.def("crypto::crc32", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_crc32));
        self.def("crypto::aes_gen_key", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_aes_gen_key));
        self.def("crypto::aes_gen_iv", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_aes_gen_iv));
        self.def("crypto::aes_encrypt", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_aes_encrypt));
        self.def("crypto::aes_decrypt", |a| crate::stdlib::sistem::lewat_hasil(
            a, crate::dok::crypto::f_aes_decrypt));
        self.def("ptr::new", |a| {
            let value = a.get(0).cloned().unwrap_or(Nil);
            let id = NEXT_VM_PTR_ID.fetch_add(1, Ordering::SeqCst);
            vm_ptr_cells()
                .lock()
                .unwrap()
                .insert(id, Arc::new(Mutex::new(value)));
            Ok(vm_ptr_value(id))
        });
        self.def("ptr::null", |_| Ok(vm_ptr_value(0)));
        self.def("ptr::is_null", |a| {
            Ok(Bool(vm_ptr_id(
                a.get(0).unwrap_or(&Nil),
                "ptr::is_null",
            )? == 0))
        });
        self.def("ptr::addr", |a| {
            Ok(Int(vm_ptr_id(
                a.get(0).unwrap_or(&Nil),
                "ptr::addr",
            )? as i128))
        });
        self.def("ptr::get", |a| {
            let cell = vm_ptr_cell(a.get(0).unwrap_or(&Nil), "ptr::get")?;
            let value = cell.lock().unwrap().clone();
            Ok(value)
        });
        self.def("ptr::set", |a| {
            let cell = vm_ptr_cell(a.get(0).unwrap_or(&Nil), "ptr::set")?;
            *cell.lock().unwrap() = a.get(1).cloned().unwrap_or(Nil);
            Ok(Unit)
        });
        self.def("ptr::free", |a| {
            let id = vm_ptr_id(a.get(0).unwrap_or(&Nil), "ptr::free")?;
            vm_ptr_cells().lock().unwrap().remove(&id);
            Ok(Unit)
        });
        // `ptr::inc`/`ptr::dec` sebelumnya cuma ada di interpreter — salah satu dari
        // tiga asimetri arah-sebaliknya yang ditemukan Fase AUDIT.
        self.def("ptr::inc", |a| vm_ptr_step(a, "ptr::inc", 1));
        self.def("ptr::dec", |a| vm_ptr_step(a, "ptr::dec", -1));
        self.def("chan::is_closed", |a| match a.get(0) {
            Some(Channel(channel)) => Ok(Bool(channel.is_closed())),
            _ => Err(bukan_channel("chan::is_closed", a)),
        });
        self.def("chan::has_value", |a| match a.get(0) {
            // ADA ISI, bukan "tidak akan memblokir": channel tertutup yang kosong menjawab `false`.
            Some(Channel(channel)) => Ok(Bool(channel.len() > 0)),
            _ => Err(bukan_channel("chan::has_value", a)),
        });
        // `std::exit` adalah nama panjang dari `exit` — ALIAS (tabel alias di
        // bawah), bukan salinan kedua.
        self.def("exit", |a| {
            let kode = match a.first() {
                None => 0,
                Some(Int(n)) => crate::host::kode_keluar(*n)?,
                Some(l) => return Err(crate::host::pesan_kode_keluar(&l.type_name_penuh())),
            };
            crate::host::keluar(kode)
        });
        // `range` dan `range_inclusive`: SATU definisi dengan acuan
        // (`interpreter::range_jumlah`). Salinan VM yang dulu ada di sini
        // berputar selamanya untuk langkah 0 sampai prosesnya dibunuh OOM.
        fn arg_vm(v: &VmValue) -> crate::interpreter::ArgAngka {
            match v {
                Int(n) => crate::interpreter::ArgAngka::Bulat(*n),
                Float(f) => crate::interpreter::ArgAngka::Pecahan(*f),
                lain => crate::interpreter::ArgAngka::Lain(lain.type_name_penuh()),
            }
        }
        self.def("range", |a| {
            let aa: Vec<_> = a.iter().map(arg_vm).collect();
            let (s, e, st) = crate::interpreter::range_argumen(&aa)?;
            let n = crate::interpreter::range_jumlah("range", s, e, st, false)?;
            Ok(VmValue::array(crate::interpreter::range_isi(s, st, n).map(Int).collect()))
        });
        self.def("range_inclusive", |a| {
            if a.len() != 2 {
                return Err(format!("`range_inclusive` butuh 2 argumen (awal, akhir), dapat {}", a.len()));
            }
            let (s, e) = crate::interpreter::range_batas("range_inclusive", &arg_vm(&a[0]), &arg_vm(&a[1]))?;
            let n = crate::interpreter::range_jumlah("range_inclusive", s, e, 1, true)?;
            Ok(VmValue::array(crate::interpreter::range_isi(s, 1, n).map(Int).collect()))
        });

        // Channel natives
        self.def("Chan::new", |a| {
            // Kapasitas yang bukan `int` DITOLAK, tidak dijatuhkan jadi 0.
            let cap = match a.get(0) {
                None => 0,
                Some(Int(n)) if *n >= 0 => {
                    // Kapasitas menentukan besar buffer yang dialokasi, jadi
                    // batasnya diperiksa sebelum alokasinya — tanpa ini
                    // `Chan::new(1e18)` mematikan proses dengan `capacity overflow`.
                    crate::interpreter::cek_batas_elemen("Chan::new", *n)?;
                    *n as usize
                }
                Some(Int(n)) => return Err(crate::interpreter::pesan_kapasitas_chan(Some(*n), "int")),
                Some(v) => return Err(crate::interpreter::pesan_kapasitas_chan(None, &v.type_name_penuh())),
            };
            Ok(Channel(VmChannel::new(cap)))
        });
        self.def("chan::send",  |a| match (a.get(0), a.get(1)) {
            // `unit`, bukan `nil`, seperti acuannya: operasi ini tidak MENEMUKAN apa pun.
            (Some(Channel(c)), Some(v)) => c.send(v.clone()).map(|_| VmValue::Unit),
            (Some(Channel(_)), None) => Err(crate::interpreter::pesan_butuh_nilai("chan::send")),
            _ => Err(bukan_channel("chan::send", a)),
        });
        self.def("chan::recv",  |a| match a.get(0) {
            // Tertutup dan kosong DITOLAK, seperti `<-c` dan acuannya — `nil`
            // sah dibawa channel, jadi ia tidak boleh berarti "sudah habis".
            Some(Channel(c)) => c.recv()?.ok_or_else(|| crate::antrean::PESAN_TERIMA_TERTUTUP.to_string()),
            _ => Err(bukan_channel("chan::recv", a)),
        });
        // Menjawab `Result`, seperti mesin acuan.
        self.def("chan::try_recv", |a| match a.get(0) {
            // Kosong dan TERTUTUP dua jawaban berbeda, seperti acuannya.
            Some(Channel(c)) => Ok(match c.coba_terima() {
                Ok(v) => VmValue::varian("Result", "Ok", Some(v)),
                Err(true) => VmValue::varian("Result", "Err", Some(VmValue::str(
                    "channel kosong, terima akan block (would block)"))),
                Err(false) => VmValue::varian("Result", "Err", Some(VmValue::str(
                    "channel sudah ditutup: tidak ada pengirim"))),
            }),
            _ => Err(bukan_channel("chan::try_recv", a)),
        });
        // `c.try_send(v)` ada di mesin acuan dan tidak ada di sini sama sekali.
        self.def("chan::try_send", |a| match (a.get(0), a.get(1)) {
            (Some(Channel(c)), Some(v)) => Ok(match c.try_send(v.clone()) {
                Ok(true) => VmValue::varian("Result", "Ok", Some(VmValue::Unit)),
                Ok(false) => VmValue::varian("Result", "Err", Some(VmValue::str(
                    "channel penuh, kirim akan block (would block)"))),
                Err(_) => VmValue::varian("Result", "Err", Some(VmValue::str(
                    "channel sudah ditutup: tidak ada penerima"))),
            }),
            (Some(Channel(_)), None) => Err(crate::interpreter::pesan_butuh_nilai("chan::try_send")),
            _ => Err(bukan_channel("chan::try_send", a)),
        });
        self.def("chan::close", |a| match a.get(0) {
            Some(Channel(c)) => { c.close(); Ok(VmValue::Unit) },
            _ => Err(bukan_channel("chan::close", a)),
        });
        self.def("chan::len",   |a| match a.get(0) {
            Some(Channel(c)) => Ok(Int(c.len() as i128)),
            _ => Err(bukan_channel("chan::len", a)),
        });
        self.def("chan::cap",   |a| match a.get(0) {
            Some(Channel(c)) => Ok(Int(c.capacity() as i128)),
            _ => Err(bukan_channel("chan::cap", a)),
        });

        // Math — SATU implementasi, milik acuan, dicapai lewat jembatan nilai.
        // Salinan VM yang dulu ada di sini memotong eksponen `math::pow` lewat
        // `as i32` dan membuat `math::clamp(5, 10, 0)` MEMATIKAN prosesnya.
        macro_rules! math_acuan { ($($n:expr => $k:ident),* $(,)?) => { $(
            self.def($n, |a| crate::stdlib::sistem::lewat(a, |v|
                crate::interpreter::Interpreter::dispatch_math_lepas(
                    &crate::interpreter::builtin::NativeFnKind::$k, v)));
        )* } }
        // `abs`/`sqrt`/`floor`/`ceil`/`round` telanjang ada di tabel alias: ditulis di sini
        // keduanya jadi DUA closure, yang cuma beralamat sama kalau pengoptimal menggabungnya.
        math_acuan!(
            "math::abs" => MathAbs,         "math::sqrt" => MathSqrt,
            "math::floor" => MathFloor,     "math::ceil" => MathCeil,
            "math::round" => MathRound,
            "math::trunc" => MathTrunc,     "math::sin" => MathSin,
            "math::cos" => MathCos,         "math::tan" => MathTan,
            "math::asin" => MathAsin,       "math::acos" => MathAcos,
            "math::atan" => MathAtan,       "math::atan2" => MathAtan2,
            "math::ln" => MathLn,           "math::log" => MathLog,
            "math::log2" => MathLog2,       "math::log10" => MathLog10,
            "math::exp" => MathExp,         "math::pow" => MathPow,
            "math::min" => MathMin,         "math::max" => MathMax,
            "math::clamp" => MathClamp,     "math::sign" => MathSign,
            "math::factorial" => MathFact,  "math::gcd" => MathGcd,
            "math::lcm" => MathLcm,         "math::hypot" => MathHypot,
            "math::copysign" => MathCopysign, "math::degrees" => MathDegrees,
            "math::radians" => MathRadians, "math::is_nan" => MathIsNan,
            "math::is_inf" => MathIsInf,
            "math::wrap_add" => MathWrapAdd, "math::wrap_sub" => MathWrapSub,
            "math::wrap_mul" => MathWrapMul,
        );
        self.globals.insert("math::PI".to_string(), Float(std::f64::consts::PI));
        self.globals.insert("math::E".to_string(), Float(std::f64::consts::E));
        self.globals.insert("math::TAU".to_string(), Float(std::f64::consts::TAU));
        self.globals.insert("math::INF".to_string(), Float(f64::INFINITY));
        self.globals.insert("math::NAN".to_string(), Float(f64::NAN));

        // String ops ── `str::` di VM: TIDAK ada implementasi kedua di sini
        // ──────────

        // Ketiganya lewat `teks_vm`; dulu menjawab bentuk tanda tangannya.
        self.def("str::len", |a| Ok(Int(teks_vm(a.get(0), "str::len")?.chars().count() as i128)));
        self.def("str::to_uppercase",
            |a| Ok(Str(Arc::new(teks_vm(a.get(0), "str::to_uppercase")?.to_uppercase()))));
        self.def("str::to_lowercase",
            |a| Ok(Str(Arc::new(teks_vm(a.get(0), "str::to_lowercase")?.to_lowercase()))));

        // `str::format` DIHAPUS dari sini: ia salinan kedua, dan seperti tiap
        // salinan kedua di proyek ini ia melenceng — tujuh bentuk menjawab berbeda …

        // Array ops `arr::len` sengaja TIDAK didefinisikan: interpreter menolaknya
        // dan mengarahkan ke `len` biasa, jadi VM tidak boleh menerima nama yang …
        fn bukan_array(nama: &str, a: &[VmValue]) -> String {
            // Kalimat acuan; dulu tanda tangannya saja, yang `perjelas_pesan_native`
            // ubah jadi kalimat LAIN dari yang acuan pakai.
            match a.first() {
                Some(v) => crate::interpreter::pesan_arg_harus_array(nama, 1, &v.type_name_penuh()),
                // Tak terjangkau lewat pintu native (batas bawahnya diperiksa lebih dulu).
                None => crate::interpreter::arity::pesan_kurang(nama,
                    crate::interpreter::arity::minimal(nama).unwrap_or(1),
                    crate::interpreter::arity::maks(nama), 0,
                    crate::interpreter::arity::tanda_tangan(nama)),
            }
        }
        self.def("arr::push", |a| match (a.get(0), a.get(1)) {
            (Some(Array(v)), Some(item)) => {
                let mut isi = v.lock().unwrap().clone();
                isi.push(item.clone());
                Ok(VmValue::array(isi))
            }
            _ => Err(bukan_array("arr::push", a)),
        });
        // Konsisten dengan interpreter: `(array_baru, Option)`, bukan elemen
        // polos — dan array kosong bukan error, tapi `Option::None`.
        self.def("arr::pop",  |a| match a.get(0) {
            Some(Array(v)) => {
                let mut isi = v.lock().unwrap().clone();
                let diambil = match isi.pop() {
                    Some(x) => vm_some(x),
                    None    => vm_none(),
                };
                Ok(VmValue::tuple(vec![VmValue::array(isi), diambil]))
            }
            _ => Err(bukan_array("arr::pop", a)),
        });
        self.def("arr::reverse", |a| match a.get(0) {
            Some(Array(v)) => { let mut i = v.lock().unwrap().clone(); i.reverse(); Ok(Array(Arc::new(std::sync::Mutex::new(i)))) },
            _ => Err(bukan_array("arr::reverse", a)),
        });
        // Pesannya menyebut apa yang KURANG, bukan cuma tanda tangannya.
        // Interpreter menolaknya dengan kalimat yang sama.
        self.def("arr::contains", |a| match (a.get(0), a.get(1)) {
            (Some(Array(v)), Some(item)) => {
                let isi = v.lock().unwrap().clone();
                Ok(Bool(crate::vm::value::ada_vm_eq(isi.iter(), item)?))
            }
            // Kalimat bentuk NAMESPACE: `def` ini melayani dua pintu, dan
            // pintu method sudah dijawab lebih dulu oleh pemeriksaan di
            // `GetMethod` — yang tahu berapa argumen yang benar-benar DITULIS.
            (Some(Array(_)), None) => Err("arr::contains butuh (arr, val)".to_string()),
            _ => Err(bukan_array("arr::contains", a)),
        });
        self.def("arr::sum",  |a| match a.get(0) {
            Some(Array(v)) => {
                let v = v.lock().unwrap();
                // `+` antara angka dan string MENYAMBUNG, jadi tanpa
                // penjagaan ini `arr::sum` menjawab teks. Lihat
                // `pesan_elemen_bukan_angka`.
                if let Some(x) = v.iter().find(|x| !matches!(x, Int(_) | Float(_))) {
                    return Err(crate::interpreter::pesan_elemen_bukan_angka(
                        "arr::sum", &x.type_name_penuh()));
                }
                v.iter().try_fold(Int(0), |acc, x| acc.add(x))
            }
            _ => Err(bukan_array("arr::sum", a)),
        });
        // Satu definisi dengan interpreter (`str_ops::gabung`), bukan salinan.
        self.def("arr::join", |a| crate::stdlib::sistem::lewat(a, |v|
            crate::interpreter::builtin::str_ops::gabung("arr::join", v)));
        // `arr::sort`/`arr::sort_desc` didaftarkan di `stdlib::koleksi`, yang jalan
        // SESUDAH fungsi ini dan karena itu menang.
        self.def("arr::union", |a| match (a.get(0), a.get(1)) {
            (Some(Array(left)), Some(Array(right))) => {
                let left = left.lock().unwrap().clone();
                let right = right.lock().unwrap().clone();
                let mut out = Vec::new();
                for item in left.into_iter().chain(right) {
                    if !crate::vm::value::ada_vm_eq(out.iter(), &item)? {
                        out.push(item);
                    }
                }
                Ok(VmValue::array(out))
            }
            // Tipe salah dan jumlah kurang adalah DUA kesalahan; kalimatnya
            // ikut mesin acuan, dan bentuk namespace-nya tetap bentuk namespace.
            (Some(Array(_)), Some(lain)) => Err(crate::interpreter::pesan_arg_harus_array(
                "arr::union", 2, &lain.type_name_penuh())),
            (Some(lain), _) => Err(crate::interpreter::pesan_arg_harus_array(
                "arr::union", 1, &lain.type_name_penuh())),
            _ => Err("arr::union butuh (arr1, arr2)".to_string()),
        });
        self.def("arr::intersect", |a| match (a.get(0), a.get(1)) {
            (Some(Array(left)), Some(Array(right))) => {
                let left = left.lock().unwrap().clone();
                let right = right.lock().unwrap().clone();
                let mut out = Vec::new();
                for item in left {
                    if crate::vm::value::ada_vm_eq(right.iter(), &item)?
                        && !crate::vm::value::ada_vm_eq(out.iter(), &item)?
                    {
                        out.push(item);
                    }
                }
                Ok(VmValue::array(out))
            }
            // Tipe salah dan jumlah kurang adalah DUA kesalahan; kalimatnya
            // ikut mesin acuan, dan bentuk namespace-nya tetap bentuk namespace.
            (Some(Array(_)), Some(lain)) => Err(crate::interpreter::pesan_arg_harus_array(
                "arr::intersect", 2, &lain.type_name_penuh())),
            (Some(lain), _) => Err(crate::interpreter::pesan_arg_harus_array(
                "arr::intersect", 1, &lain.type_name_penuh())),
            _ => Err("arr::intersect butuh (arr1, arr2)".to_string()),
        });
        self.def("arr::difference", |a| match (a.get(0), a.get(1)) {
            (Some(Array(left)), Some(Array(right))) => {
                let left = left.lock().unwrap().clone();
                let right = right.lock().unwrap().clone();
                let mut out = Vec::new();
                for item in left {
                    if !crate::vm::value::ada_vm_eq(right.iter(), &item)?
                        && !crate::vm::value::ada_vm_eq(out.iter(), &item)?
                    {
                        out.push(item);
                    }
                }
                Ok(VmValue::array(out))
            }
            // Tipe salah dan jumlah kurang adalah DUA kesalahan; kalimatnya
            // ikut mesin acuan, dan bentuk namespace-nya tetap bentuk namespace.
            (Some(Array(_)), Some(lain)) => Err(crate::interpreter::pesan_arg_harus_array(
                "arr::difference", 2, &lain.type_name_penuh())),
            (Some(lain), _) => Err(crate::interpreter::pesan_arg_harus_array(
                "arr::difference", 1, &lain.type_name_penuh())),
            _ => Err("arr::difference butuh (arr1, arr2)".to_string()),
        });
        self.def("arr::filter", |a| match (a.get(0), a.get(1)) {
            (Some(Array(_)), Some(Function(_) | NativeFn(_))) => {
                Err("arr::filter dengan fn: gunakan loop manual di VM".to_string())
            },
            _ => Err("arr::filter(a,fn)".to_string()),
        });
        self.def("arr::mean", |a| match a.get(0) {
            Some(Array(v)) => {
                let v = v.lock().unwrap();
                // Elemen bukan-angka dipetakan jadi `0.0` di bawah,
                // jadi `arr::mean([1, "x"])` menjawab sebuah ANGKA yang
                // dihitung dari data yang tidak bisa dibaca.
                if let Some(x) = v.iter().find(|x| !matches!(x, Int(_) | Float(_))) {
                    return Err(crate::interpreter::pesan_elemen_bukan_angka(
                        "arr::mean", &x.type_name_penuh()));
                }
                if v.is_empty() {
                    return Err(crate::interpreter::pesan_koleksi_kosong(
                        "arr::mean", "rata-rata"));
                }
                let sum: f64 = v.iter().map(|x| match x { Int(n) => *n as f64, Float(f) => *f, _ => 0.0 }).sum();
                Ok(Float(sum / v.len() as f64))
            },
            _ => Err(bukan_array("arr::mean", a)),
        });
        self.def("arr::std_dev", |a| match a.get(0) {
            Some(Array(v)) => {
                let v = v.lock().unwrap();
                // Elemen bukan-angka dipetakan jadi `0.0` di bawah,
                // jadi `arr::std_dev([1, "x"])` menjawab sebuah ANGKA yang
                // dihitung dari data yang tidak bisa dibaca.
                if let Some(x) = v.iter().find(|x| !matches!(x, Int(_) | Float(_))) {
                    return Err(crate::interpreter::pesan_elemen_bukan_angka(
                        "arr::std_dev", &x.type_name_penuh()));
                }
                if v.is_empty() {
                    return Err(crate::interpreter::pesan_koleksi_kosong(
                        "arr::std_dev", "simpangan baku"));
                }
                let vals: Vec<f64> = v.iter().map(|x| match x { Int(n) => *n as f64, Float(f) => *f, _ => 0.0 }).collect();
                let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64;
                Ok(Float(var.sqrt()))
            },
            _ => Err(bukan_array("arr::std_dev", a)),
        });

        // Queue / Stack / Deque / LinkedList DIHAPUS dari VM.

        // ── External stdlib modules ────────────────────────────────────────
        crate::stdlib::register_all(self);

        // `list` BUKAN alias: aturannya sendiri (array lewat apa adanya, string jadi
        // deret rune, tuple jadi array).
        self.def("list", |a| match a.get(0) {
            Some(Array(v)) => Ok(Array(v.clone())),
            Some(Str(s))   => Ok(VmValue::array(s.chars().map(Rune).collect())),
            Some(Tuple(t)) => Ok(VmValue::array(crate::vm::value::isi_tuple(t))),
            // `type_name_penuh` supaya kalimatnya sama persis dengan
            // interpreter: `struct:P`, bukan cuma `struct`.
            Some(o) => Err(format!("list(): tidak bisa dari `{}`", o.type_name_penuh())),
            None => Err(crate::interpreter::pesan_butuh_nilai("list()")),
        });

        // HOF bawaan sebagai NILAI (`let f = arr::map`); dilayani `NativeKhusus::Hof`.
        for (n, f) in super::hof::PENGGANTI_HOF { self.def(n, *f); }

        // `py::`/`js::`/`php::`/`lua::`: wakil per builtin; dilayani `NativeKhusus::Ffi`.
        for (n, f) in super::ffi_asing::daftar() { self.def(&n, f); }

        // ── Alias nama telanjang ──────────────────────────────────────────
        for (telanjang, penuh) in [
            ("cetak", "wok"), ("cetak_", "wok_"),
            ("cetak_galat", "eprint"), ("baca", "tok"),
            ("teks", "sprint"),
            ("fmt::wok", "wok"), ("fmt::wok_", "wok_"), ("fmt::tok", "tok"),
            ("fmt::eprint", "eprint"), ("fmt::sprint", "sprint"),
            ("abs", "math::abs"), ("sqrt", "math::sqrt"), ("floor", "math::floor"),
            ("ceil", "math::ceil"), ("round", "math::round"),
            ("sin", "math::sin"), ("cos", "math::cos"), ("tan", "math::tan"),
            ("ln", "math::ln"), ("exp", "math::exp"), ("pow", "math::pow"),
            ("clamp", "math::clamp"),
            ("log10", "math::log10"), ("log2", "math::log2"),
            ("parse_int", "str::to_int"), ("parse_float", "str::to_float"),
            ("enumerate", "arr::enumerate"), ("zip", "arr::zip"),
            ("jawa", "jawa::alloc"), ("jawir", "jawir::free"),
            // Satu fungsi, dua nama — seperti di interpreter (satu jenis untuk keduanya).
            ("std::exit", "exit"),
            ("chan::make", "Chan::new"),
            ("str::rjust", "str::pad_left"),
            ("str::ljust", "str::pad_right"),
            ("str::index_of", "str::find"),
            ("dok::get", "dok::http_get"),
            ("dok::post", "dok::http_post"),
            ("dok::put", "dok::http_put"),
            ("dok::delete", "dok::http_delete"),
            ("ptr::deref", "ptr::get"),
            ("crypto::hash", "crypto::sha256"),
        ].into_iter().chain(super::hof::ALIAS_HOF.iter().copied()) {
            if let Some(v) = self.globals.get(penuh).cloned() {
                self.globals.insert(telanjang.to_string(), v);
            }
        }
    }
}

/// Argumen pertama `chan::` yang bukan channel.
fn bukan_channel(nama: &str, a: &[VmValue]) -> String {
    crate::interpreter::pesan_arg_harus_channel(
        nama, &a.first().map(|v| v.type_name_penuh()).unwrap_or_else(|| "nil".into()))
}
