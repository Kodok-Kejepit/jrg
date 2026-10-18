// jembatan/src/lib.rs — pintu C dari biner `--emit-llvm` ke builtin milik `jrgc`.
// Nilai menyeberang sebagai kotak bertag (`%JrgAny`).

use jrgc::vm::value::{Label, VmValue};
use std::collections::HashMap;

/// `%JrgAny`: tag (8 bit bawah) + lukisan isi (24 bit atas), lalu dua kata muatan.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KotakAny { tag: i32, sela: i32, a: u64, b: u64 }

#[repr(C)]
struct Dua { ptr: *mut u8, len: i64 }

/// Primitif milik biner pemanggil: pemesan memori, penyisip dict, dan pengubah isi kotak.
#[repr(C)]
pub struct Kait {
    alloc: extern "C" fn(i64, i64) -> *mut u8,
    dict_tempat: unsafe extern "C" fn(*mut Dua, i64, i64, *const u8, i64) -> *mut u8,
    any_ubah: unsafe extern "C" fn(*const KotakAny, i32, *mut KotakAny, *const u8, i64),
}

const LANGKAH: i64 = 40;
const KATA_ARRAY: i32 = 6 | 1 << 8;
const KATA_DICT: i32 = 7 | 1 << 8;
/// Nilai milik `jrgc` yang tidak punya bentuk di biner LLVM (handle, struct, tuple): dititipkan utuh.
const TAG_TITIPAN: i32 = 9;
/// Penanda struct milik program, sama dengan `llvm::codegen_jembatan::TANDA_STRUCT`.
const TANDA_STRUCT: &str = "\u{1}struct";

const PESAN_MILIK_PROGRAM: &str = "nilai milik program (struct, enum, tuple, atau fungsi) tidak bisa diserahkan ke builtin ini dari backend `--emit-llvm` — ini celah backend, bukan salah programmu\n  bantuan: serahkan isinya sebagai dict, array, atau teks";

fn kotak(tag: i32, a: u64, b: u64) -> KotakAny { KotakAny { tag, sela: 0, a, b } }

fn bocorkan(s: &str) -> (u64, u64) {
    if s.is_empty() { return (1, 0); }
    let b: &'static mut [u8] = Box::leak(s.as_bytes().to_vec().into_boxed_slice());
    (b.as_ptr() as u64, b.len() as u64)
}

unsafe fn teks<'a>(p: u64, n: u64) -> &'a str {
    if n == 0 { return ""; }
    std::str::from_utf8(std::slice::from_raw_parts(p as *const u8, n as usize)).unwrap_or("")
}

unsafe fn dari_any(a: &KotakAny, k: &Kait) -> Result<VmValue, String> {
    let pola = "nilai bertipe `\u{1}` tidak bisa diserahkan ke builtin ini dari backend `--emit-llvm`";
    Ok(match a.tag & 255 {
        0 => VmValue::Nil,
        8 => VmValue::Unit,
        1 => VmValue::Bool(a.a & 255 != 0),
        2 => VmValue::Int(a.a as i64 as i128),
        3 => VmValue::Float(f64::from_bits(a.a)),
        4 => char::from_u32(a.a as u32).map(VmValue::Rune).unwrap_or(VmValue::Nil),
        5 => VmValue::str(teks(a.a, a.b)),
        6 => {
            let mut s = *a;
            if a.tag != KATA_ARRAY { (k.any_ubah)(a, KATA_ARRAY, &mut s, pola.as_ptr(), pola.len() as i64); }
            let p = s.a as *const KotakAny;
            VmValue::array((0..s.b as usize).map(|i| dari_any(&*p.add(i), k)).collect::<Result<_, _>>()?)
        }
        7 => {
            let mut s = *a;
            if a.tag != KATA_DICT { (k.any_ubah)(a, KATA_DICT, &mut s, pola.as_ptr(), pola.len() as i64); }
            let p = s.a as *const u8;
            let mut m = HashMap::new();
            let mut nama_struct: Option<String> = None;
            for i in 0..s.b as usize {
                let pas = p.add(i * LANGKAH as usize);
                let kunci = &*(pas as *const Dua);
                let nama = teks(kunci.ptr as u64, kunci.len as u64).to_string();
                let nilai = dari_any(&*(pas.add(16) as *const KotakAny), k)?;
                if nama == TANDA_STRUCT {
                    if let VmValue::Str(t) = &nilai { nama_struct = Some(t.to_string()); continue; }
                }
                m.insert(nama, nilai);
            }
            match nama_struct { Some(t) => VmValue::struktur(&t, m), None => VmValue::map(m) }
        }
        TAG_TITIPAN => (*(a.a as *const VmValue)).clone(),
        // Struct/tuple/enum/fungsi milik program: tidak punya bentuk di sisi `jrgc`.
        _ => return Err(PESAN_MILIK_PROGRAM.to_string()),
    })
}

unsafe fn ke_any(v: &VmValue, k: &Kait) -> KotakAny {
    match v {
        VmValue::Nil => kotak(0, 0, 0),
        VmValue::Unit => kotak(8, 0, 0),
        VmValue::Bool(b) => kotak(1, *b as u64, 0),
        VmValue::Int(n) => match i64::try_from(*n) {
            Ok(x) => kotak(2, x as u64, 0),
            Err(_) => titip(v),
        },
        VmValue::Float(f) => kotak(3, f.to_bits(), 0),
        VmValue::Rune(c) => kotak(4, *c as u64, 0),
        VmValue::Str(s) => { let (p, n) = bocorkan(s); kotak(5, p, n) }
        VmValue::Array(a) => {
            let isi = a.lock().unwrap_or_else(|p| p.into_inner());
            let buf = (k.alloc)((isi.len().max(1) * 24) as i64, 8) as *mut KotakAny;
            for (i, x) in isi.iter().enumerate() { buf.add(i).write(ke_any(x, k)); }
            kotak(KATA_ARRAY, buf as u64, isi.len() as u64)
        }
        VmValue::Map(m) if matches!(m.label, Label::Dict) => {
            let isi = m.lock().unwrap_or_else(|p| p.into_inner());
            let mut d = Dua { ptr: std::ptr::null_mut(), len: 0 };
            for (kunci, x) in isi.iter() {
                let (kp, kl) = bocorkan(kunci);
                let nilai = ke_any(x, k);
                let pas = (k.dict_tempat)(&mut d, LANGKAH, 8, kp as *const u8, kl as i64);
                (pas.add(16) as *mut KotakAny).write(nilai);
            }
            kotak(KATA_DICT, d.ptr as u64, d.len as u64)
        }
        lain => titip(lain),
    }
}

fn titip(v: &VmValue) -> KotakAny {
    kotak(TAG_TITIPAN, Box::into_raw(Box::new(v.clone())) as u64, 0)
}

/// Panggil builtin `nama`: 0 nilai biasa, 1 `Some`/`Ok`, 2 `None`, 3 `Err`, -1 gagal (teksnya di `out`).
/// # Safety
/// `args` menunjuk `n` kotak sah, `out` satu kotak yang bisa ditulis, `kait` primitif pemanggil.
#[no_mangle]
pub unsafe extern "C" fn jrg_jembatan_panggil(
    nama: *const u8, nl: i64, args: *const KotakAny, n: i64, out: *mut KotakAny, kait: *const Kait,
) -> i32 {
    let k = &*kait;
    let nama = teks(nama as u64, nl as u64);
    let gagal = |pesan: String| { let (p, l) = bocorkan(&pesan); kotak(5, p, l) };
    // `::tampil` dan `::typeof`: tampilan dan nama tipe sebuah nilai titipan.
    // `::field`: `x.f` atas nilai titipan (struct milik `jrgc`, mis. `MousePos`).
    if nama == "::field" {
        let v = match dari_any(&*args, k) { Ok(v) => v, Err(e) => { out.write(gagal(e)); return -1; } };
        let f = match dari_any(&*args.add(1), k) { Ok(VmValue::Str(f)) => f.to_string(), _ => String::new() };
        return match &v {
            VmValue::Map(m) if !matches!(m.label, Label::Dict) => match m.lock().unwrap_or_else(|p| p.into_inner()).get(&f) {
                Some(x) => { out.write(ke_any(x, k)); 0 }
                None => { out.write(gagal(format!("`{}` tidak punya field `{}`", v.type_name_penuh(), f))); -1 }
            },
            _ => { out.write(gagal(format!("tidak bisa akses field `.{}` pada `{}`", f, v.type_name_penuh()))); -1 }
        };
    }
    if nama == "::tampil" || nama == "::typeof" {
        let v = match dari_any(&*args, k) { Ok(v) => v, Err(e) => { out.write(gagal(e)); return -1; } };
        let t = if nama == "::tampil" { v.to_string() } else {
            // Lewat builtin `typeof` yang sama dengan `--vm`, bukan nama tipe internalnya.
            match jrgc::vm::panggil_bawaan("typeof", &[v.clone()]) {
                Some(Ok(VmValue::Str(t))) => t.to_string(),
                _ => v.type_name().to_string(),
            }
        };
        let (p, l) = bocorkan(&t);
        out.write(kotak(5, p, l));
        return 0;
    }
    let masuk: Vec<VmValue> = match (0..n as usize).map(|i| dari_any(&*args.add(i), k)).collect() {
        Ok(m) => m,
        Err(e) => { out.write(gagal(e)); return -1; }
    };
    // `::extern`: argumen pertamanya deskripsi `extern fn`-nya, sisanya argumen panggilannya.
    let hasil = std::panic::catch_unwind(|| match (nama, masuk.first()) {
        ("::extern", Some(VmValue::Str(d))) => Some(jrgc::vm::panggil_extern(d, &masuk[1..])),
        _ => jrgc::vm::panggil_bawaan(nama, &masuk),
    });
    match hasil {
        Err(_) => { out.write(gagal(format!("`{}` panik di dalam jrgc — ini bug di jrgc, bukan di programmu", nama))); -1 }
        Ok(None) => { out.write(gagal(format!("`{}` tidak ada di pustaka standar", nama))); -1 }
        Ok(Some(Err(e))) => { out.write(gagal(e)); -1 }
        Ok(Some(Ok(v))) => match v.bagian_enum() {
            Some(("Option", "Some", Some(p))) | Some(("Result", "Ok", Some(p))) => { out.write(ke_any(p, k)); 1 }
            Some(("Result", "Ok", None)) => { out.write(kotak(8, 0, 0)); 1 }
            Some(("Option", "None", _)) => { out.write(kotak(0, 0, 0)); 2 }
            Some(("Result", "Err", p)) => { out.write(p.map(|x| ke_any(x, k)).unwrap_or(kotak(0, 0, 0))); 3 }
            _ => { out.write(ke_any(&v, k)); 0 }
        },
    }
}

/// Dipanggil pengantar APK (`src/llvm/android.c`) sebelum `main` program jalan.
/// # Safety
/// `t` menunjuk tabel yang hidup selama prosesnya.
#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn jrg_android_pasang(t: *const jrgc::canvas::jendela::TabelAndroid) {
    jrgc::canvas::jendela::pasang(t)
}
