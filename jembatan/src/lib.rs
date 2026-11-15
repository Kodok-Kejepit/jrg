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
    /// Struct/tuple milik program (tag 10) jadi dict bertanda; kosong kalau biner tidak memancarkannya.
    buram: Option<unsafe extern "C" fn(*const KotakAny, *mut KotakAny)>,
    /// Beri goroutine (korutin) lain di biner itu satu giliran.
    serah: extern "C" fn(),
    /// Jumlah goroutine yang belum selesai.
    lain: extern "C" fn() -> i64,
}

const LANGKAH: i64 = 40;
const KATA_ARRAY: i32 = 6 | 1 << 8;
const KATA_DICT: i32 = 7 | 1 << 8;
/// Nilai milik `jrgc` yang tidak punya bentuk di biner LLVM (handle, struct, tuple): dititipkan utuh.
const TAG_TITIPAN: i32 = 9;
/// Nilai fungsi milik program: penunjuk ke `{ %JrgFn, adaptor }` (`llvm::codegen_jembatan`).
const TAG_FUNGSI: i32 = 11;
/// Penanda struct dan tuple milik program, sama dengan `llvm::codegen_jembatan::TANDA_*`.
const TANDA_STRUCT: &str = "\u{1}struct";
const TANDA_TUPLE: &str = "\u{1}tuple";
const TANDA_ENUM: &str = "\u{1}enum";
const TANDA_VARIAN: &str = "\u{1}varian";
const TANDA_ISI: &str = "\u{1}isi";
const TAG_BURAM: i32 = 10;

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
            let (mut nama_enum, mut varian): (Option<String>, Option<String>) = (None, None);
            let mut tuple = false;
            for i in 0..s.b as usize {
                let pas = p.add(i * LANGKAH as usize);
                let kunci = &*(pas as *const Dua);
                let nama = teks(kunci.ptr as u64, kunci.len as u64).to_string();
                let nilai = dari_any(&*(pas.add(16) as *const KotakAny), k)?;
                if nama == TANDA_STRUCT {
                    if let VmValue::Str(t) = &nilai { nama_struct = Some(t.to_string()); continue; }
                }
                if nama == TANDA_TUPLE { tuple = true; continue; }
                if nama == TANDA_ENUM || nama == TANDA_VARIAN {
                    if let VmValue::Str(t) = &nilai {
                        if nama == TANDA_ENUM { nama_enum = Some(t.to_string()) } else { varian = Some(t.to_string()) }
                        continue;
                    }
                }
                m.insert(nama, nilai);
            }
            if let (Some(e), Some(v)) = (nama_enum, varian) {
                return Ok(VmValue::varian(&e, &v, m.remove(TANDA_ISI)));
            }
            if tuple {
                return Ok(VmValue::tuple((0..m.len()).map(|i| m.remove(&i.to_string()).unwrap_or(VmValue::Nil)).collect()));
            }
            match nama_struct { Some(t) => VmValue::struktur(&t, m), None => VmValue::map(m) }
        }
        TAG_TITIPAN => (*(a.a as *const VmValue)).clone(),
        TAG_FUNGSI => fungsi_program(a.a as *const u8, k),
        TAG_BURAM if k.buram.is_some() => {
            let mut o = kotak(0, 0, 0);
            (k.buram.unwrap())(a, &mut o);
            if o.tag & 255 == TAG_BURAM { return Err(PESAN_MILIK_PROGRAM.to_string()); }
            dari_any(&o, k)?
        }
        // Struct/tuple/enum/fungsi milik program: tidak punya bentuk di sisi `jrgc`.
        _ => return Err(PESAN_MILIK_PROGRAM.to_string()),
    })
}

type Adaptor = unsafe extern "C" fn(*const u8, *const KotakAny, i64, *mut KotakAny) -> i32;

/// Penunjuk mentah yang hidup sepanjang proses (kotak fungsi dan tabel kait milik biner).
#[derive(Clone, Copy)]
struct Mentah(*const u8, *const Kait);
unsafe impl Send for Mentah {}
unsafe impl Sync for Mentah {}

/// Nilai yang dipindah utuh antar-thread; `VmValue` tidak `Send` karena isinya bisa sel bersama.
struct Kirim<T>(T);
unsafe impl<T> Send for Kirim<T> {}

type Tugas = Box<dyn FnOnce() + Send>;
type Hasil = std::thread::Result<Option<Result<VmValue, String>>>;

/// Panggilan fungsi milik biner yang dititipkan thread lain, dijalankan thread biner selama ia
/// menunggu builtin pemblokir (`layani`). `AKTIF` menghitung penunggu yang sedang melayani.
static ANTRE: std::sync::OnceLock<(std::sync::mpsc::Sender<Kirim<Tugas>>, std::sync::Mutex<std::sync::mpsc::Receiver<Kirim<Tugas>>>)> =
    std::sync::OnceLock::new();
static AKTIF: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn antre() -> &'static (std::sync::mpsc::Sender<Kirim<Tugas>>, std::sync::Mutex<std::sync::mpsc::Receiver<Kirim<Tugas>>>) {
    ANTRE.get_or_init(|| { let (tx, rx) = std::sync::mpsc::channel(); (tx, std::sync::Mutex::new(rx)) })
}

/// Builtin yang memblokir: dijalankan di thread pembantu supaya korutin lain tetap mendapat
/// giliran, dan (untuk `dok::serve`) supaya handler dari thread koneksi bisa dititipkan.
fn pemblokir(nama: &str) -> bool {
    matches!(nama, "dok::serve" | "dok::tcp_accept" | "dok::tcp_recv" | "dok::tcp_recv_line" | "dok::tcp_recv_bytes"
        | "dok::tcp_recv_all" | "dok::tcp_connect" | "dok::udp_recv_from" | "dok::udp_recv_bytes_from" | "dok::ws_recv")
}

unsafe fn jalankan_fungsi(m: Mentah, args: &[VmValue]) -> Result<VmValue, String> {
    let k = &*m.1;
    let masuk: Vec<KotakAny> = args.iter().map(|x| ke_any(x, k)).collect();
    let mut out = kotak(0, 0, 0);
    let adaptor: Adaptor = std::mem::transmute(*(m.0.add(16) as *const usize));
    let r = adaptor(m.0, masuk.as_ptr(), masuk.len() as i64, &mut out);
    if r < 0 { return Err(teks(out.a, out.b).to_string()); }
    dari_any(&out, k)
}

/// Fungsi milik biner LLVM sebagai nilai VM: argumennya dikotak, adaptornya yang memanggil.
/// Biner itu satu thread: panggilan dari thread lain dititipkan ke thread biner kalau ia sedang
/// menunggu builtin pemblokir (`layani`), selain itu DITOLAK.
unsafe fn fungsi_program(kotak_fn: *const u8, k: &Kait) -> VmValue {
    let m = Mentah(kotak_fn, k as *const Kait);
    let pemilik = std::thread::current().id();
    let panggil = move |args: &[VmValue]| -> Result<VmValue, String> {
        // Utuh, bukan per field: penunjuknya cuma boleh menyeberang lewat `Mentah`.
        let m = &m;
        if std::thread::current().id() == pemilik { return jalankan_fungsi(*m, args); }
        if AKTIF.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err("fungsi milik biner `--emit-llvm` dipanggil dari thread lain — biner itu satu thread, \
                        jadi pemanggilan ini ditolak; ini celah backend, bukan salah programmu".to_string());
        }
        let (rtx, rrx) = std::sync::mpsc::channel::<Kirim<Result<VmValue, String>>>();
        let (m, args) = (*m, Kirim(args.to_vec()));
        let tugas: Tugas = Box::new(move || { let a = args; let _ = rtx.send(Kirim(jalankan_fungsi(m, &a.0))); });
        if antre().0.send(Kirim(tugas)).is_err() {
            return Err("thread biner `--emit-llvm` sudah berhenti melayani".to_string());
        }
        rrx.recv().map(|r| r.0).unwrap_or_else(|_| Err("handler di thread biner `--emit-llvm` tidak menjawab".to_string()))
    };
    VmValue::Asing(jrgc::vm::ffi_asing::Asing::Fungsi(std::sync::Arc::new(
        jrgc::vm::ffi_asing::FungsiLuar { panggil: Box::new(panggil) })))
}

/// `nama` di thread pembantu; selama menunggu, thread ini menjalankan panggilan yang dititipkan
/// padanya dan memberi korutin lain giliran.
/// Thread pembantu yang menganggur, dipakai ulang: membuat thread per panggilan memakan ±50 µs.
static NGANGGUR: std::sync::Mutex<Vec<std::sync::mpsc::Sender<Tugas>>> = std::sync::Mutex::new(Vec::new());

fn kerjakan(t: Tugas) {
    let siap = NGANGGUR.lock().unwrap_or_else(|p| p.into_inner()).pop();
    let t = match siap { Some(tx) => match tx.send(t) { Ok(()) => return, Err(e) => e.0 }, None => t };
    let (tx, rx) = std::sync::mpsc::channel::<Tugas>();
    let _ = tx.send(t);
    std::thread::spawn(move || {
        while let Ok(t) = rx.recv() {
            t();
            NGANGGUR.lock().unwrap_or_else(|p| p.into_inner()).push(tx.clone());
        }
    });
}

fn layani(nama: &str, masuk: Vec<VmValue>, k: &Kait) -> Hasil {
    let (tx, rx) = std::sync::mpsc::channel::<Kirim<Hasil>>();
    let (nama, masuk) = (nama.to_string(), Kirim(masuk));
    AKTIF.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    kerjakan(Box::new(move || {
        let m = masuk;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| jrgc::vm::panggil_bawaan(&nama, &m.0)));
        let _ = tx.send(Kirim(r));
    }));
    let hasil = loop {
        // Kuncinya dilepas sebelum tugasnya jalan: tugas itu boleh masuk `layani` lagi.
        let tugas = antre().1.lock().unwrap_or_else(|p| p.into_inner()).try_recv().ok();
        if let Some(t) = tugas { (t.0)(); continue; }
        (k.serah)();
        match rx.recv_timeout(std::time::Duration::from_micros(300)) {
            Ok(r) => break r.0,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break Ok(Some(Err("thread pembantu jembatan berhenti tanpa jawaban".to_string()))),
        }
    };
    AKTIF.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    hasil
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

/// Bebaskan satu kotak titipan (handle FFI yang tidak keluar dari bloknya); handle asingnya dilepas worker.
/// # Safety
/// `a` menunjuk kotak sah yang tidak dipakai lagi oleh siapa pun.
#[no_mangle]
pub unsafe extern "C" fn jrg_jembatan_lepas_titipan(a: *const KotakAny) {
    let k = &*a;
    if k.tag & 0xff == TAG_TITIPAN && k.a != 0 { drop(Box::from_raw(k.a as *mut VmValue)); }
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
            // Dict dari builtin (`zip::list`): `d.k` membaca kuncinya, kalimat kunci hilangnya milik `--vm`.
            VmValue::Map(_) => match jrgc::vm::baca_field(&v, &f) {
                Ok(x) => { out.write(ke_any(&x, k)); 0 }
                Err(e) => { out.write(gagal(e)); -1 }
            },
            // Tuple (`t.0`) dan jenis lain: pintu field `--vm` sendiri, dengan kalimatnya.
            _ => match jrgc::vm::baca_field(&v, &f) {
                Ok(x) => { out.write(ke_any(&x, k)); 0 }
                Err(e) => { out.write(gagal(e)); -1 }
            },
        };
    }
    // `::field_tulis`: `x.f = v` atas nilai titipan; nilai milik program (bertag 10) tidak lewat sini.
    if nama == "::field_tulis" {
        if (*args).tag & 255 == TAG_BURAM {
            out.write(gagal("menulis field atas struct milik program yang bertag belum didukung backend `--emit-llvm` \
                             — ini celah backend, bukan salah programmu".to_string()));
            return -1;
        }
        let ambil = |i: usize| dari_any(&*args.add(i), k);
        let r = match (ambil(0), ambil(1), ambil(2)) {
            (Ok(v), Ok(VmValue::Str(f)), Ok(x)) => jrgc::vm::tulis_field(&v, &f, x),
            (Err(e), _, _) | (_, _, Err(e)) => Err(e),
            _ => Err("nama field bukan teks".to_string()),
        };
        return match r { Ok(v) => { out.write(ke_any(&v, k)); 0 } Err(e) => { out.write(gagal(e)); -1 } };
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
    // Selama ada goroutine lain atau yang melayani (server), panggilan apa pun bisa menunggu
    // sesuatu yang harus jalan di korutin lain: `proses::`, `http::`, `dok::http_get` ke server sendiri.
    let ramai = AKTIF.load(std::sync::atomic::Ordering::SeqCst) > 0 || (k.lain)() > 0;
    if pemblokir(nama) || (ramai && !nama.starts_with("::")) {
        let hasil = layani(nama, masuk, k);
        return tulis_hasil(nama, hasil, out, k);
    }
    // `::extern`: argumen pertamanya deskripsi `extern fn`-nya, sisanya argumen panggilannya.
    // `::metode`: argumen pertamanya nama method, lalu penerima dan argumennya.
    let hasil = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match (nama, masuk.first()) {
        ("::extern", Some(VmValue::Str(d))) => Some(jrgc::vm::panggil_extern(d, &masuk[1..])),
        ("::metode", Some(VmValue::Str(m))) => Some(jrgc::vm::panggil_metode(m, &masuk[1..])),
        // Registry `jawa::` milik biner itu sendiri: hitungannya datang dari sana, bentuknya dari `jrgc`.
        ("::jawa_stats", _) => {
            let n: Vec<i128> = masuk.iter().map(|v| if let VmValue::Int(x) = v { *x } else { 0 }).collect();
            Some(Ok(jrgc::stdlib::jawa::heap_stats(n[0], n[1], n[2], n[3], n[4])))
        }
        _ => jrgc::vm::panggil_bawaan(nama, &masuk),
    }));
    tulis_hasil(nama, hasil, out, k)
}

unsafe fn tulis_hasil(nama: &str, hasil: Hasil, out: *mut KotakAny, k: &Kait) -> i32 {
    let gagal = |pesan: String| { let (p, l) = bocorkan(&pesan); kotak(5, p, l) };
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
