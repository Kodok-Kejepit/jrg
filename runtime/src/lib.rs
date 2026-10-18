// runtime/src/lib.rs — jrgrt — JawaRustGo++ native runtime

// `no_std` for the real (staticlib) build only.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;
use core::fmt::{self, Write as FmtWrite};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

// ─────────────────────────────────────────────────────────────────────────
// libc bindings (the only external surface this crate needs)
// ─────────────────────────────────────────────────────────────────────────

extern "C" {
    fn write(fd: i32, buf: *const u8, count: usize) -> isize;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn exit(code: i32) -> !;
    fn atexit(cb: extern "C" fn()) -> i32;
    fn malloc(size: usize) -> *mut u8;
    fn realloc(ptr: *mut u8, size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn posix_memalign(out: *mut *mut c_void, align: usize, size: usize) -> i32;
}

const FD_STDIN: i32 = 0;
const FD_STDOUT: i32 = 1;
const FD_STDERR: i32 = 2;

/// `malloc` already returns memory aligned for any scalar (16 bytes on
/// x86-64 / aarch64); only an over-aligned request needs `posix_memalign`.
const MALLOC_GUARANTEED_ALIGN: usize = 16;

// ── Alokator ───────────────────────────────────────────────────────────

/// Ukuran chunk yang diminta ke `malloc` sekaligus. Cukup besar supaya
/// `malloc` jarang dipanggil, cukup kecil supaya program mungil tidak
/// membayar megabyte yang tak dipakai.
const UKURAN_CHUNK: usize = 1 << 20; // 1 MiB
/// Batas ruang lebih di belakang alokasi raksasa.
const RUANG_LEBIH_MAKS: usize = 64 << 20;

struct Arena {
    kursor: usize,
    ujung: usize,
    /// Awal alokasi terakhir — dipakai `dealloc`/`realloc` untuk memundurkan
    /// kursor kalau yang dilepas memang yang paling baru.
    terakhir: usize,
}

static mut ARENA: Arena = Arena { kursor: 0, ujung: 0, terakhir: 0 };

// ── dict ──────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
struct KunciStr { p: *const u8, n: i64 }

unsafe fn kunci_di(ptr: *const u8, i: i64, stride: i64) -> &'static [u8] {
    let k = &*(ptr.offset((i * stride) as isize) as *const KunciStr);
    borrow_bytes(k.p, k.n)
}

/// Urutkan pasangan menurut kunci lalu buang kunci ganda — yang TERAKHIR
/// menang.
#[no_mangle]
pub extern "C" fn jrg_dict_build(ptr: *mut u8, len: i64, stride: i64) -> i64 {
    if ptr.is_null() || len <= 0 || stride <= 0 { return 0; }
    let n = len as usize;
    let st = stride as usize;

    // Urutkan indeks, bukan byte-nya: memindahkan pasangan berarti menyalin
    // `stride` byte, dan pengurutan langsung akan melakukannya berkali-kali.
    let mut urut: alloc::vec::Vec<usize> = (0..n).collect();
    urut.sort_by(|&a, &b| unsafe {
        kunci_di(ptr, a as i64, stride).cmp(kunci_di(ptr, b as i64, stride))
            .then(a.cmp(&b))          // stabil: urutan asli jadi penentu
    });

    let mut keluar: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(n * st);
    let mut i = 0usize;
    while i < n {
        // Lompati semua yang kuncinya sama kecuali yang TERAKHIR.
        let mut j = i;
        while j + 1 < n {
            let sama = unsafe {
                kunci_di(ptr, urut[j] as i64, stride) == kunci_di(ptr, urut[j + 1] as i64, stride)
            };
            if !sama { break; }
            j += 1;
        }
        let src = unsafe { core::slice::from_raw_parts(ptr.add(urut[j] * st), st) };
        keluar.extend_from_slice(src);
        i = j + 1;
    }
    let baru = keluar.len() / st;
    unsafe { core::ptr::copy_nonoverlapping(keluar.as_ptr(), ptr, keluar.len()) };
    baru as i64
}

// `jrg_dict_find`, `jrg_dict_tempat`, `jrg_dict_rapikan`, `jrg_dict_baru`: src/llvm/primitif.c

/// Urutkan sebuah larik `i64` di tempat.
#[no_mangle]
pub extern "C" fn jrg_sort_i64(ptr: *mut i64, len: i64) {
    if ptr.is_null() || len <= 1 { return; }
    let s = unsafe { core::slice::from_raw_parts_mut(ptr, len as usize) };
    s.sort_unstable();
}

/// Urutkan sebuah larik `f64` di tempat.
#[no_mangle]
pub extern "C" fn jrg_sort_f64(ptr: *mut f64, len: i64) {
    if ptr.is_null() || len <= 1 { return; }
    let s = unsafe { core::slice::from_raw_parts_mut(ptr, len as usize) };
    s.sort_unstable_by(|a, b| a.total_cmp(b));
}

/// Urutkan larik string (`{ptr, len}` berpasangan) di tempat.
#[no_mangle]
pub extern "C" fn jrg_sort_str(ptr: *mut u8, len: i64) {
    if ptr.is_null() || len <= 1 { return; }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Ptrlen { p: *const u8, n: i64 }
    let s = unsafe {
        core::slice::from_raw_parts_mut(ptr as *mut Ptrlen, len as usize)
    };
    s.sort_unstable_by(|a, b| {
        let x = unsafe { borrow_bytes(a.p, a.n) };
        let y = unsafe { borrow_bytes(b.p, b.n) };
        x.cmp(y)
    });
}

/// Alokasi mentah untuk codegen — dipakai membuat buffer milik sendiri.
#[no_mangle]
pub extern "C" fn jrg_alloc(size: i64, align: i64) -> *mut u8 {
    if size <= 0 {
        // Penunjuk yang sah dan tidak nol, walau tidak pernah dibaca —
        // panjang nol menjaga setiap pembacanya.
        return core::ptr::NonNull::<u8>::dangling().as_ptr();
    }
    let align = if align <= 0 { 1 } else { align as usize };
    unsafe {
        let l = core::alloc::Layout::from_size_align_unchecked(size as usize, align);
        let p = alloc::alloc::alloc(l);
        if p.is_null() { memori_habis() }
        p
    }
}

/// Alokasi gagal: dilaporkan, bukan dibiarkan jadi SIGSEGV saat penunjuk nolnya ditulisi.
/// Pesannya tetap — di titik ini tidak ada memori untuk merakit kalimat.
#[cold]
#[inline(never)]
fn memori_habis() -> ! {
    const PESAN: &str = "memori habis — program ini meminta lebih banyak memori daripada yang tersedia\n  \
        bantuan: biner `--emit-llvm` tidak membebaskan salinan lama, jadi menyalin koleksi besar \
        berulang-ulang menghabiskan memori; pakai bentuk yang mengubah di tempat \
        (`a += [x]`, `q = q.push(x)`, `q = q.rest()`), atau jalankan tanpa `--emit-llvm`";
    jrg_panic(PESAN.as_ptr(), PESAN.len() as i64)
}

/// Kalau `ptr..ptr+len` berakhir PERSIS di kursor arena, perpanjang kursornya
/// sebanyak `tambahan` byte dan kembalikan `true`.
#[inline]
unsafe fn arena_perpanjang(ptr: *const u8, len: usize, tambahan: usize) -> bool {
    if len == 0 {
        return false;
    }
    let a = &mut *core::ptr::addr_of_mut!(ARENA);
    let ujung_str = ptr as usize + len;
    if ujung_str != a.kursor || ujung_str + tambahan > a.ujung {
        return false;
    }
    a.kursor = ujung_str + tambahan;
    true
}

struct BumpAllocator;

impl BumpAllocator {
    #[inline]
    unsafe fn ambil(&self, size: usize, align: usize) -> *mut u8 {
        let a = &mut *core::ptr::addr_of_mut!(ARENA);
        let mulai = (a.kursor + align - 1) & !(align - 1);
        if mulai + size <= a.ujung {
            a.kursor = mulai + size;
            a.terakhir = mulai;
            return mulai as *mut u8;
        }
        self.chunk_baru(size, align)
    }

    /// Jalur lambat: minta chunk baru ke `malloc`.
    #[inline(never)]
    unsafe fn chunk_baru(&self, size: usize, align: usize) -> *mut u8 {
        // Permintaan yang lebih besar dari satu chunk mendapat ruang lebih di belakangnya:
        // tanpa itu teks raksasa yang terus disambung disalin utuh tiap langkah.
        let perlu = size + align;
        let lebih = if perlu < RUANG_LEBIH_MAKS { perlu } else { RUANG_LEBIH_MAKS };
        let besar = if perlu > UKURAN_CHUNK { perlu + lebih } else { UKURAN_CHUNK };
        let blok = malloc(besar);
        if blok.is_null() {
            // Dilaporkan sekali; kalau melapor pun butuh memori, yang kedua menyerah diam.
            static mut SUDAH_LAPOR: bool = false;
            if !SUDAH_LAPOR { SUDAH_LAPOR = true; memori_habis() }
            return core::ptr::null_mut();
        }
        let a = &mut *core::ptr::addr_of_mut!(ARENA);
        let awal = blok as usize;
        let mulai = (awal + align - 1) & !(align - 1);
        a.kursor = mulai + size;
        a.ujung = awal + besar;
        a.terakhir = mulai;
        mulai as *mut u8
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = if layout.align() < 1 { 1 } else { layout.align() };
        self.ambil(layout.size(), align)
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let a = &mut *core::ptr::addr_of_mut!(ARENA);
        // Hanya alokasi paling baru yang bisa dikembalikan. Selain itu
        // dibiarkan — memang itu harga yang dibayar untuk alokasi secepat
        // penambahan kursor.
        if ptr as usize == a.terakhir && a.terakhir + layout.size() == a.kursor {
            a.kursor = a.terakhir;
        }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let a = &mut *core::ptr::addr_of_mut!(ARENA);
        // `Vec` yang tumbuh hampir selalu memperbesar blok TERAKHIR; kalau
        // sisanya cukup, ia diperpanjang di tempat tanpa menyalin apa pun.
        if ptr as usize == a.terakhir
            && a.terakhir + layout.size() == a.kursor
            && a.terakhir + new_size <= a.ujung
        {
            a.kursor = a.terakhir + new_size;
            return ptr;
        }
        let align = if layout.align() < 1 { 1 } else { layout.align() };
        let fresh = self.ambil(new_size, align);
        if !fresh.is_null() {
            core::ptr::copy_nonoverlapping(ptr, fresh, core::cmp::min(layout.size(), new_size));
        }
        fresh
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

/// Reached only on allocation failure or an internal `unwrap` that JRG
/// codegen is not supposed to be able to trigger. Prints and exits with
/// the same code every other JRG panic uses, so behavior stays uniform.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    flush_stdout();
    write_all(FD_STDERR, b"ada yang gagal saat program jalan\n");
    unsafe { exit(101) }
}

/// Never actually called: nothing here unwinds (`panic = "abort"`), but
/// rustc still emits a reference to the personality routine for a
/// `no_std` staticlib, and the link fails without a definition.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn rust_eh_personality() {}

// ─────────────────────────────────────────────────────────────────────────
// Internal helpers (not exported)
// ─────────────────────────────────────────────────────────────────────────

/// Borrow a `(ptr, len)` pair coming from JRG code as a `&str`.
unsafe fn borrow_bytes<'a>(ptr: *const u8, len: i64) -> &'a [u8] {
    if ptr.is_null() || len <= 0 {
        return &[];
    }
    core::slice::from_raw_parts(ptr, len as usize)
}

/// Cari `pola` di dalam `bak`, kembalikan offset byte pertamanya.
fn cari_byte(bak: &[u8], pola: &[u8]) -> Option<usize> {
    if pola.is_empty() { return Some(0); }
    if pola.len() > bak.len() { return None; }
    bak.windows(pola.len()).position(|w| w == pola)
}

unsafe fn borrow_str<'a>(ptr: *const u8, len: i64) -> &'a str {
    if ptr.is_null() || len <= 0 {
        return "";
    }
    let bytes = core::slice::from_raw_parts(ptr, len as usize);
    core::str::from_utf8(bytes).unwrap_or("")
}

/// Leak a `String` as a `(ptr, len)` pair written through the two
/// out-parameters every string-returning runtime function takes.
unsafe fn emit_bytes(v: Vec<u8>, out_ptr: *mut *const u8, out_len: *mut i64) {
    let len = v.len() as i64;
    let ptr = if len == 0 {
        core::ptr::NonNull::<u8>::dangling().as_ptr() as *const u8
    } else {
        let p = v.as_ptr();
        core::mem::forget(v); // bocor, sama seperti `emit_string`
        p
    };
    *out_ptr = ptr;
    *out_len = len;
}

unsafe fn emit_string(s: String, out_ptr: *mut *const u8, out_len: *mut i64) {
    let len = s.len() as i64;
    let ptr = if len == 0 {
        // Still a valid, non-null, well-aligned pointer even though it's
        // never read (len == 0 guards every consumer).
        core::ptr::NonNull::<u8>::dangling().as_ptr() as *const u8
    } else {
        let p = s.as_ptr();
        core::mem::forget(s); // leak: keeps `p` valid for the whole process
        p
    };
    *out_ptr = ptr;
    *out_len = len;
}

/// `write(2)` returns short on signals/pipes; loop until the whole slice
/// is out or the fd stops accepting bytes.
fn write_all(fd: i32, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let written = unsafe { write(fd, bytes.as_ptr(), bytes.len()) };
        if written <= 0 {
            return;
        }
        bytes = &bytes[written as usize..];
    }
}

fn format_f64_jrg(v: f64) -> String {
    // Mirrors jrgc's tree-walking interpreter's `Display for Value::Float`
    // exactly, so a program compiled with --vm/--emit-llvm/interpreter prints …
    if v.fract() == 0.0 && v.is_finite() {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

fn format_f32_jrg(v: f32) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

// `f64::fract`/`is_finite` are `std` inherent methods; in `no_std` the
// same math has to be spelled out. Both are pure bit/arithmetic work, no
// libm call needed.
trait FloatExt: Copy {
    fn fract(self) -> Self;
    fn is_finite(self) -> bool;
}

impl FloatExt for f64 {
    fn fract(self) -> f64 {
        self - trunc_f64(self)
    }
    fn is_finite(self) -> bool {
        self.to_bits() & 0x7FF0_0000_0000_0000 != 0x7FF0_0000_0000_0000
    }
}

impl FloatExt for f32 {
    fn fract(self) -> f32 {
        self - (self as f64 as i64 as f32 * 0.0 + trunc_f64(self as f64) as f32)
    }
    fn is_finite(self) -> bool {
        self.to_bits() & 0x7F80_0000 != 0x7F80_0000
    }
}

/// Truncate toward zero without libm: values beyond 2^52 are already
/// integral for f64, so the round-trip through i64 is only taken in the
/// range where it is exact.
fn trunc_f64(v: f64) -> f64 {
    if !FloatExt::is_finite(v) {
        return v;
    }
    if v >= 9.007_199_254_740_992e15 || v <= -9.007_199_254_740_992e15 {
        return v;
    }
    v as i64 as f64
}

const STDOUT_BUFFER_CAPACITY: usize = 16 * 1024;
const STDOUT_BUFFERING_LINE: u8 = 0;
const STDOUT_BUFFERING_FULL: u8 = 1;

static STDOUT_BUFFERING: AtomicU8 = AtomicU8::new(STDOUT_BUFFERING_LINE);
static ATEXIT_REGISTERED: AtomicBool = AtomicBool::new(false);

fn stdout_is_line_buffered() -> bool {
    STDOUT_BUFFERING.load(Ordering::Relaxed) == STDOUT_BUFFERING_LINE
}

struct StdoutBuffer {
    bytes: Vec<u8>,
}

impl StdoutBuffer {
    const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.reserve_once();
        self.bytes.extend_from_slice(bytes);
        if self.bytes.len() >= STDOUT_BUFFER_CAPACITY
            || (stdout_is_line_buffered() && bytes.contains(&b'\n'))
        {
            self.flush();
        }
    }

    fn write_fmt_args(&mut self, args: fmt::Arguments<'_>) {
        self.reserve_once();
        let before = self.bytes.len();
        let _ = fmt::Write::write_fmt(self, args);
        if self.bytes.len() >= STDOUT_BUFFER_CAPACITY
            || (stdout_is_line_buffered() && self.bytes[before..].contains(&b'\n'))
        {
            self.flush();
        }
    }

    fn reserve_once(&mut self) {
        if self.bytes.capacity() == 0 {
            self.bytes.reserve(STDOUT_BUFFER_CAPACITY);
        }
    }

    fn flush(&mut self) {
        if self.bytes.is_empty() {
            return;
        }
        write_all(FD_STDOUT, &self.bytes);
        self.bytes.clear();
    }
}

impl fmt::Write for StdoutBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.bytes.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// The buffered stdout of the running program.
static mut STDOUT_BUFFER: StdoutBuffer = StdoutBuffer::new();

fn with_stdout<R>(f: impl FnOnce(&mut StdoutBuffer) -> R) -> R {
    ensure_flush_at_exit();
    // SAFETY: single-threaded by construction, see STDOUT_BUFFER.
    unsafe { f(&mut *core::ptr::addr_of_mut!(STDOUT_BUFFER)) }
}

extern "C" fn flush_stdout_at_exit() {
    flush_stdout();
}

/// The std build flushed on `Drop` of the thread-local buffer.
fn ensure_flush_at_exit() {
    if ATEXIT_REGISTERED
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        unsafe { atexit(flush_stdout_at_exit) };
    }
}

fn flush_stdout() {
    // Deliberately does not go through `with_stdout`: this runs on the exit path
    // too, where registering another atexit hook would be pointless (and on the …
    unsafe { (*core::ptr::addr_of_mut!(STDOUT_BUFFER)).flush() };
}

fn write_out(s: &str, to_stderr: i32) {
    if to_stderr != 0 {
        write_all(FD_STDERR, s.as_bytes());
    } else {
        with_stdout(|buffer| buffer.write_bytes(s.as_bytes()));
    }
}

fn write_fmt_out(args: fmt::Arguments<'_>, to_stderr: i32) {
    if to_stderr != 0 {
        let mut s = String::new();
        let _ = s.write_fmt(args);
        write_all(FD_STDERR, s.as_bytes());
    } else {
        with_stdout(|buffer| buffer.write_fmt_args(args));
    }
}

/// Galat yang sedang membawa program keluar: kalimat pertamanya, lalu kalimat
/// tiap `defer` yang ikut gagal saat membereskan.
struct Panik {
    sedang: bool,
    pesan: Option<String>,
    ikut: Vec<String>,
}

static mut PANIK: Panik = Panik { sedang: false, pesan: None, ikut: Vec::new() };

/// Kalimat galat dicatat dulu, tidak langsung dicetak: `defer` yang masih
/// terdaftar jalan lebih dulu (`mati`), dan keluarannya harus mendahuluinya.
fn eprint_line(args: fmt::Arguments<'_>) {
    let mut s = String::new();
    let _ = s.write_fmt(args);
    let p = unsafe { &mut *core::ptr::addr_of_mut!(PANIK) };
    if p.pesan.is_none() { p.pesan = Some(s); } else { p.ikut.push(s); }
}

/// Ujung tiap panik: jalankan `defer` korutin ini, cetak galatnya, keluar.
/// Panik dari dalam sebuah `defer` masuk ke sini lagi dan meneruskan sisanya.
fn mati() -> ! {
    unsafe { (*core::ptr::addr_of_mut!(PANIK)).sedang = true };
    jrg_tunda_jalankan(0);
    flush_stdout();
    let p = unsafe { &mut *core::ptr::addr_of_mut!(PANIK) };
    let mut s = p.pesan.take().unwrap_or_default();
    for t in p.ikut.drain(..) {
        s.push_str("\n  catatan: sebuah `defer` juga gagal saat membereskan: ");
        s.push_str(&t);
    }
    // Badan Future/Task yang gagal menitipkan galatnya ke yang mengambil hasilnya.
    if unsafe { !TAK_GUGUR } {
        p.sedang = false;
        unsafe { jrg_masa_gagal(s.as_ptr(), s.len() as i64) };
        p.sedang = true;
    }
    s.push('\n');
    // Goroutine `spawn` yang gagal cuma menghentikan dirinya; stack yang habis tidak bisa.
    if unsafe { !TAK_GUGUR && jrg_ko_gugur(1) != 0 } {
        write_all(FD_STDERR, b"goroutine error: ");
        write_all(FD_STDERR, s.as_bytes());
        p.sedang = false;
        unsafe { jrg_ko_gugur(0) };
    }
    write_all(FD_STDERR, s.as_bytes());
    unsafe { exit(PANIC_EXIT_CODE) }
}

/// Dipasang penjaga stack: galatnya dilaporkan dari stack cadangan, jadi tidak ada
/// goroutine yang bisa ditinggalkan dengan selamat.
static mut TAK_GUGUR: bool = false;

extern "C" {
    fn jrg_ko_gugur(tanya: i32) -> i32;
    fn jrg_masa_gagal(s: *const u8, n: i64) -> i32;
}

#[no_mangle]
pub extern "C" fn jrg_set_stdout_buffering(mode: i32) {
    flush_stdout();
    let mode = if mode == 0 {
        STDOUT_BUFFERING_LINE
    } else {
        STDOUT_BUFFERING_FULL
    };
    STDOUT_BUFFERING.store(mode, Ordering::Relaxed);
}

// ─────────────────────────────────────────────────────────────────────────
// Printing
// ─────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn jrg_print_i64(v: i64, to_stderr: i32) {
    write_fmt_out(format_args!("{}", v), to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_u64(v: u64, to_stderr: i32) {
    write_fmt_out(format_args!("{}", v), to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_i128(lo: u64, hi: u64, to_stderr: i32) {
    let v: i128 = (((hi as u128) << 64) | (lo as u128)) as i128;
    write_fmt_out(format_args!("{}", v), to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_u128(lo: u64, hi: u64, to_stderr: i32) {
    let v: u128 = ((hi as u128) << 64) | (lo as u128);
    write_fmt_out(format_args!("{}", v), to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_f64(v: f64, to_stderr: i32) {
    write_out(&format_f64_jrg(v), to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_f32(v: f32, to_stderr: i32) {
    write_out(&format_f32_jrg(v), to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_bool(v: i32, to_stderr: i32) {
    write_out(if v != 0 { "true" } else { "false" }, to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_str(ptr: *const u8, len: i64, to_stderr: i32) {
    let s = unsafe { borrow_str(ptr, len) };
    write_out(s, to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_char(cp: u32, to_stderr: i32) {
    if let Some(c) = char::from_u32(cp) {
        let mut buf = [0u8; 4];
        write_out(c.encode_utf8(&mut buf), to_stderr);
    }
}

#[no_mangle]
pub extern "C" fn jrg_print_space(to_stderr: i32) {
    write_out(" ", to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_print_newline(to_stderr: i32) {
    write_out("\n", to_stderr);
}

#[no_mangle]
pub extern "C" fn jrg_flush(to_stderr: i32) {
    if to_stderr != 0 {
        // stderr is written unbuffered; nothing can be pending.
    } else {
        flush_stdout();
    }
}

// ─────────────────────────────────────────────────────────────────────────
// to-string conversions (out-param pair)
// ─────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn jrg_i64_to_str(v: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    unsafe { emit_string(v.to_string(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_u64_to_str(v: u64, out_ptr: *mut *const u8, out_len: *mut i64) {
    unsafe { emit_string(v.to_string(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_i128_to_str(lo: u64, hi: u64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let v: i128 = (((hi as u128) << 64) | (lo as u128)) as i128;
    unsafe { emit_string(v.to_string(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_u128_to_str(lo: u64, hi: u64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let v: u128 = ((hi as u128) << 64) | (lo as u128);
    unsafe { emit_string(v.to_string(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_f64_to_str(v: f64, out_ptr: *mut *const u8, out_len: *mut i64) {
    unsafe { emit_string(format_f64_jrg(v), out_ptr, out_len) };
}

/// `{:.N}` untuk `float`. Aturannya sama dengan `Interpreter::desimal_tetap`:
/// nilai yang tidak berhingga tidak punya desimal, jadi ia dirender lewat
/// jalur tampil yang biasa — supaya satu nilai tidak punya dua nama.
#[no_mangle]
pub extern "C" fn jrg_f64_to_str_desimal(
    v: f64, n: i64, out_ptr: *mut *const u8, out_len: *mut i64,
) {
    let teks = if v.is_finite() {
        format!("{:.*}", n.max(0) as usize, v)
    } else {
        format_f64_jrg(v)
    };
    unsafe { emit_string(teks, out_ptr, out_len) };
}

/// `{:.N}` untuk `int`. Nol ditempel, bukan dilewatkan `f64`:
/// `9007199254740993` melebihi apa yang `f64` bisa wakili persis.
#[no_mangle]
pub extern "C" fn jrg_i64_to_str_desimal(
    v: i64, n: i64, out_ptr: *mut *const u8, out_len: *mut i64,
) {
    let n = n.max(0) as usize;
    let teks = if n == 0 { v.to_string() } else { format!("{}.{}", v, "0".repeat(n)) };
    unsafe { emit_string(teks, out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_f32_to_str(v: f32, out_ptr: *mut *const u8, out_len: *mut i64) {
    unsafe { emit_string(format_f32_jrg(v), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_bool_to_str(v: i32, out_ptr: *mut *const u8, out_len: *mut i64) {
    unsafe { emit_string((if v != 0 { "true" } else { "false" }).to_string(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_char_to_str(cp: u32, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = char::from_u32(cp).map(|c| c.to_string()).unwrap_or_default();
    unsafe { emit_string(s, out_ptr, out_len) };
}

// ─────────────────────────────────────────────────────────────────────────
// String operations
// ─────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn jrg_str_concat(
    aptr: *const u8, alen: i64,
    bptr: *const u8, blen: i64,
    out_ptr: *mut *const u8, out_len: *mut i64,
) {
    // Level byte: menyambung dua UTF-8 sah selalu menghasilkan UTF-8 sah,
    // jadi tidak ada yang perlu divalidasi di sini.
    let a = unsafe { borrow_bytes(aptr, alen) };
    let b = unsafe { borrow_bytes(bptr, blen) };
    if b.is_empty() {
        unsafe { *out_ptr = if a.is_empty() { core::ptr::NonNull::<u8>::dangling().as_ptr() } else { aptr as *mut u8 } as *const u8; *out_len = a.len() as i64; }
        return;
    }
    // Jalur cepat: kalau `a` adalah alokasi arena paling baru, `b` ditempel
    // langsung sesudahnya — satu alokasi untuk seluruh rantai `x + y + z`,
    // bukan satu per langkah. Lihat `arena_perpanjang`.
    unsafe {
        if arena_perpanjang(aptr, a.len(), b.len()) {
            core::ptr::copy_nonoverlapping(b.as_ptr(), (aptr as *mut u8).add(a.len()), b.len());
            *out_ptr = aptr;
            *out_len = (a.len() + b.len()) as i64;
            return;
        }
    }
    let mut v = Vec::with_capacity(a.len() + b.len());
    v.extend_from_slice(a);
    v.extend_from_slice(b);
    unsafe { emit_bytes(v, out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_eq(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i32 {
    let a = unsafe { borrow_str(aptr, alen) };
    let b = unsafe { borrow_str(bptr, blen) };
    (a == b) as i32
}

#[no_mangle]
pub extern "C" fn jrg_str_lt(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i32 {
    let a = unsafe { borrow_str(aptr, alen) };
    let b = unsafe { borrow_str(bptr, blen) };
    (a < b) as i32
}

/// `len()` on a string counts Unicode scalar values (chars), not bytes —
/// matching the tree-walking interpreter's `s.chars().count()`.
#[no_mangle]
pub extern "C" fn jrg_str_char_count(ptr: *const u8, len: i64) -> i64 {
    let s = unsafe { borrow_str(ptr, len) };
    s.chars().count() as i64
}

#[no_mangle]
pub extern "C" fn jrg_str_to_upper(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    // Jalur cepat ASCII: kalau seluruh isinya ASCII — kasus yang paling sering —
    // pengubahan huruf adalah pemetaan per byte dengan panjang hasil yang sama …
    let b = unsafe { borrow_bytes(ptr, len) };
    if b.is_ascii() {
        let mut v = b.to_vec();
        v.make_ascii_uppercase();
        unsafe { emit_bytes(v, out_ptr, out_len) };
        return;
    }
    let s = unsafe { borrow_str(ptr, len) };
    unsafe { emit_string(s.to_uppercase(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_to_lower(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let b = unsafe { borrow_bytes(ptr, len) };
    if b.is_ascii() {
        let mut v = b.to_vec();
        v.make_ascii_lowercase();
        unsafe { emit_bytes(v, out_ptr, out_len) };
        return;
    }
    let s = unsafe { borrow_str(ptr, len) };
    unsafe { emit_string(s.to_lowercase(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_trim(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    // NOL alokasi: hasil `trim` selalu sebuah SUBSLICE dari masukannya, jadi
    // yang dikembalikan cukup penunjuk ke dalam buffer yang sama.
    let s = unsafe { borrow_str(ptr, len) };
    let t = s.trim();
    unsafe {
        *out_ptr = if t.is_empty() {
            core::ptr::NonNull::<u8>::dangling().as_ptr() as *const u8
        } else {
            t.as_ptr()
        };
        *out_len = t.len() as i64;
    }
}

#[no_mangle]
pub extern "C" fn jrg_str_starts_with(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i32 {
    let a = unsafe { borrow_bytes(aptr, alen) };
    let b = unsafe { borrow_bytes(bptr, blen) };
    a.starts_with(b) as i32
}

#[no_mangle]
pub extern "C" fn jrg_str_ends_with(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i32 {
    let a = unsafe { borrow_bytes(aptr, alen) };
    let b = unsafe { borrow_bytes(bptr, blen) };
    a.ends_with(b) as i32
}

/// Posisi `b` di dalam `a`, dihitung per KARAKTER — atau `-1`.
#[no_mangle]
pub extern "C" fn jrg_str_find(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i64 {
    let a = unsafe { borrow_bytes(aptr, alen) };
    let b = unsafe { borrow_bytes(bptr, blen) };
    match cari_byte(a, b) {
        Some(i) => match core::str::from_utf8(&a[..i]) {
            Ok(depan) => depan.chars().count() as i64,
            // Bukan UTF-8 valid: tidak bisa dihitung per karakter, dan
            // mengarang angka lebih buruk daripada mengaku tidak ketemu.
            Err(_) => -1,
        },
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn jrg_str_contains(aptr: *const u8, alen: i64, bptr: *const u8, blen: i64) -> i32 {
    // UTF-8 self-synchronizing: pencarian per byte tidak bisa "menemukan"
    // pola di tengah sebuah karakter, jadi jawabannya sama dengan per
    // karakter.
    let a = unsafe { borrow_bytes(aptr, alen) };
    let b = unsafe { borrow_bytes(bptr, blen) };
    cari_byte(a, b).is_some() as i32
}

#[no_mangle]
pub extern "C" fn jrg_str_reverse(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    // Panjang byte hasilnya PERSIS sama dengan masukannya — membalik urutan
    // karakter tidak mengubah berapa byte yang dipakai masing-masing.
    let s = unsafe { borrow_str(ptr, len) };
    let mut v: Vec<u8> = Vec::with_capacity(s.len());
    for c in s.chars().rev() {
        let mut buf = [0u8; 4];
        v.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    unsafe { emit_bytes(v, out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_repeat(ptr: *const u8, len: i64, n: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_bytes(ptr, len) };
    let n = if n < 0 { 0 } else { n as usize };
    unsafe { emit_bytes(s.repeat(n), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_replace(
    sptr: *const u8, slen: i64, fptr: *const u8, flen: i64, tptr: *const u8, tlen: i64,
    out_ptr: *mut *const u8, out_len: *mut i64,
) {
    // Level byte.
    let s = unsafe { borrow_bytes(sptr, slen) };
    let from = unsafe { borrow_bytes(fptr, flen) };
    let to = unsafe { borrow_bytes(tptr, tlen) };
    if from.is_empty() {
        // Pola kosong menyisipkan penggantinya di setiap BATAS KARAKTER, termasuk di
        // awal dan di akhir — `replace("aé漢", "", "-")` menghasilkan `-a-é-漢-`.
        let mut out: Vec<u8> = Vec::with_capacity(s.len() + to.len());
        for (i, b) in s.iter().enumerate() {
            if b & 0xC0 != 0x80 {
                out.extend_from_slice(to);
            }
            let _ = i;
            out.push(*b);
        }
        out.extend_from_slice(to);
        unsafe { emit_bytes(out, out_ptr, out_len) };
        return;
    }
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        match cari_byte(&s[i..], from) {
            Some(k) => {
                out.extend_from_slice(&s[i..i + k]);
                out.extend_from_slice(to);
                i += k + from.len();
            }
            None => {
                out.extend_from_slice(&s[i..]);
                break;
            }
        }
    }
    unsafe { emit_bytes(out, out_ptr, out_len) };
}

/// One-entry memo for `jrg_str_char_at`: the last string it looked at
/// (already UTF-8-validated) plus the `(char index, byte offset)` its scan …
struct CharAtCursor {
    s: Option<&'static str>,
    ptr: *const u8,
    len: i64,
    char_idx: i64,
    byte_off: usize,
}
static mut CHAR_AT_CURSOR: CharAtCursor = CharAtCursor {
    s: None,
    ptr: core::ptr::null(),
    len: 0,
    char_idx: 0,
    byte_off: 0,
};

/// Returns the Unicode scalar at char index `idx` (0-based), or 0 if out of
/// range (callers are expected to bounds-check against `jrg_str_char_count` …
#[no_mangle]
pub extern "C" fn jrg_str_slice(
    ptr: *const u8,
    len: i64,
    lo: i64,
    hi: i64,
    out_ptr: *mut *const u8,
    out_len: *mut i64,
) {
    let s = unsafe { borrow_str(ptr, len) };
    let lo = lo.max(0) as usize;
    let hi = hi.max(0) as usize;

    let mut byte_lo = s.len();
    let mut byte_hi = s.len();
    let mut i = 0usize;
    for (off, _) in s.char_indices() {
        if i == lo { byte_lo = off; }
        if i == hi { byte_hi = off; break; }
        i += 1;
    }
    if lo >= hi || byte_lo > byte_hi {
        unsafe {
            *out_ptr = s.as_ptr();
            *out_len = 0;
        }
        return;
    }
    let potongan = &s[byte_lo..byte_hi];
    unsafe {
        *out_ptr = potongan.as_ptr();
        *out_len = potongan.len() as i64;
    }
}

#[no_mangle]
pub extern "C" fn jrg_str_char_at(ptr: *const u8, len: i64, idx: i64) -> u32 {
    if idx < 0 { return 0; }
    let target = idx;

    // Reuse the memo when this is the same string as last time; otherwise
    // validate it once and start a fresh memo.
    let (s, mut char_idx, mut byte_off) = unsafe {
        let cursor = &*core::ptr::addr_of!(CHAR_AT_CURSOR);
        match cursor.s {
            Some(cached) if cursor.ptr == ptr && cursor.len == len => {
                if cursor.char_idx <= target {
                    (cached, cursor.char_idx, cursor.byte_off)
                } else {
                    (cached, 0i64, 0usize) // backward seek: rescan from the start
                }
            }
            _ => (borrow_str(ptr, len), 0i64, 0usize),
        }
    };

    let found = loop {
        if byte_off >= s.len() {
            break None;
        }
        // `s` is validated UTF-8 and `byte_off` only ever advances by whole
        // `len_utf8()` steps from 0, so it always sits on a char boundary
        // and this decode cannot fail.
        let c = match s[byte_off..].chars().next() {
            Some(c) => c,
            None => break None,
        };
        if char_idx == target {
            break Some((c as u32, char_idx, byte_off));
        }
        byte_off += c.len_utf8();
        char_idx += 1;
    };

    unsafe {
        let cursor = &mut *core::ptr::addr_of_mut!(CHAR_AT_CURSOR);
        cursor.s = Some(s);
        cursor.ptr = ptr;
        cursor.len = len;
        match found {
            // Park the memo exactly on the char just returned, so the next
            // index in a forward scan is one decode away.
            Some((_, ci, bo)) => { cursor.char_idx = ci; cursor.byte_off = bo; }
            // Ran off the end: keep the string cached (still validated) but
            // reset the position so a later in-range index is found.
            None => { cursor.char_idx = 0; cursor.byte_off = 0; }
        }
    }
    found.map(|(c, _, _)| c).unwrap_or(0)
}

/// UTF-8 decode-one-char-and-advance, used to lower `for i, c in some_str`.
#[no_mangle]
pub extern "C" fn jrg_str_next_char(
    ptr: *const u8, len: i64, offset: i64,
    out_cp: *mut u32, out_next_offset: *mut i64,
) -> i32 {
    let s = unsafe { borrow_str(ptr, len) };
    let off = if offset < 0 { 0usize } else { offset as usize };
    if off >= s.len() {
        return 0;
    }
    match s[off..].chars().next() {
        Some(c) => {
            unsafe {
                *out_cp = c as u32;
                *out_next_offset = (off + c.len_utf8()) as i64;
            }
            1
        }
        None => 0,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Panics — every one prints to stderr and terminates the process with exit …

const PANIC_EXIT_CODE: i32 = 101;

#[no_mangle]
pub extern "C" fn jrg_panic(ptr: *const u8, len: i64) -> ! {
    let s = unsafe { borrow_str(ptr, len) };
    flush_stdout();
    eprint_line(format_args!("{}", s));
    mati()
}

// ── Pesan panik ────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn jrg_panic_index_oob(idx: i64, len: i64) -> ! {
    jrg_panic_index_oob_jenis(idx, len, b"koleksi".as_ptr(), 7, b"elemen".as_ptr(), 6)
}

/// Versi yang menyebut JENIS koleksinya (`array`, `string`, `tuple`) dan
/// satuan isinya (`elemen`, `karakter`).
#[no_mangle]
pub extern "C" fn jrg_panic_index_oob_jenis(
    idx: i64, len: i64,
    jenis_ptr: *const u8, jenis_len: i64,
    satuan_ptr: *const u8, satuan_len: i64,
) -> ! {
    let jenis = unsafe { borrow_str(jenis_ptr, jenis_len) };
    let satuan = unsafe { borrow_str(satuan_ptr, satuan_len) };
    flush_stdout();
    if len == 0 {
        eprint_line(format_args!(
            "index [{}] di luar batas — {} ini kosong (0 {}), jadi belum ada index yang valid",
            idx, jenis, satuan
        ));
    } else {
        eprint_line(format_args!(
            "index [{}] di luar batas — {} ini isinya {} {} (index valid: 0..{}, atau -1..-{} kalau mau hitung dari belakang)",
            idx, jenis, len, satuan, len - 1, len
        ));
    }
    mati()
}

#[no_mangle]
pub extern "C" fn jrg_panic_slice_oob(lo: i64, hi: i64, len: i64) -> ! {
    flush_stdout();
    if lo > hi {
        eprint_line(format_args!(
            "potongan [{}:{}] terbalik — batas awal tidak boleh lebih besar dari batas akhir",
            lo, hi
        ));
    } else {
        eprint_line(format_args!(
            "batas potongan [{}:{}] di luar jangkauan — panjangnya {}, jadi batas yang sah 0..{} (atau negatif kalau mau hitung dari belakang)",
            lo, hi, len, len
        ));
    }
    mati()
}

#[no_mangle]
pub extern "C" fn jrg_panic_div_by_zero(kiri: i64, sisa: i32) -> ! {
    flush_stdout();
    // Kalimatnya sama dengan `interpreter::pesan_bagi_nol` — `%` punya kalimatnya sendiri.
    if sisa != 0 {
        eprint_line(format_args!(
            "`{} % 0` — nggak bisa cari sisa bagi dengan nol. Cek lagi angka pembaginya sebelum dipakai `%`", kiri));
    } else {
        eprint_line(format_args!(
            "`{} / 0` — nggak bisa bagi dengan nol, hasilnya nggak pernah terdefinisi. Cek lagi angka pembaginya sebelum dipakai bagi", kiri));
    }
    mati()
}

/// Geseran yang tidak sah. Dua kalimat pertama sama dengan `Interpreter::geser_terperiksa`;
/// yang ketiga milik backend ini: hasilnya ada di 128 bit acuan, tidak di lebar di sini.
#[no_mangle]
pub extern "C" fn jrg_panic_geser(x: i64, y: i64, ke_kiri: i32, bit: i32) -> ! {
    flush_stdout();
    let (op, balik) = if ke_kiri != 0 { ("<<", ">>") } else { (">>", "<<") };
    if y < 0 {
        eprint_line(format_args!(
            "`{} {} {}` — jumlah geseran tidak boleh negatif; kalau maksudnya menggeser ke arah sebaliknya, pakai `{}` dengan jumlah positif",
            x, op, y, balik));
    } else if y >= 128 {
        eprint_line(format_args!(
            "`{} {} {}` — integer JRG cuma 128 bit, jadi menggeser {} kali tidak punya arti (semua bitnya sudah keluar jauh sebelum itu)",
            x, op, y, y));
    } else {
        eprint_line(format_args!(
            "`{} {} {}` — hasilnya tidak muat di bilangan bulat {} bit backend LLVM, sementara mesin acuan memakai 128 bit. Jalankan tanpa `--emit-llvm` kalau angkanya memang sebesar itu",
            x, op, y, bit));
    }
    mati()
}

/// `+`/`-`/`*` yang hasilnya tidak muat di lebar tipenya. Hasil sebenarnya dihitung ulang
/// di 128 bit supaya pesannya menyebut angka yang dimaksud penulisnya.
#[no_mangle]
pub extern "C" fn jrg_panic_luber(a: i64, b: i64, op: i32, bit: i32, bertanda: i32) -> ! {
    let (x, y) = if bertanda != 0 { (a as i128, b as i128) } else { (a as u64 as i128, b as u64 as i128) };
    let (simbol, hasil) = match op {
        0 => ("+", x.checked_add(y)),
        1 => ("-", x.checked_sub(y)),
        _ => ("*", x.checked_mul(y)),
    };
    let tipe = if bertanda != 0 { "bertanda" } else { "tak bertanda" };
    flush_stdout();
    match hasil {
        Some(h) if bit <= 64 => eprint_line(format_args!(
            "hasil `{} {} {}` = {} tidak muat di bilangan bulat {} bit {} backend LLVM (dua mesin lain \
             menghitung di 128 bit) — jalankan tanpa `--emit-llvm`, pakai float, atau tulis \
             `math::wrap_add`/`wrap_sub`/`wrap_mul` kalau memang ingin membungkus di 64 bit",
            x, simbol, y, h, bit, tipe)),
        _ => eprint_line(format_args!(
            "hasil `{} {} {}` kelewat besar buat bilangan bulat {} bit {} backend LLVM", x, simbol, y, bit, tipe)),
    }
    mati()
}

#[no_mangle]
pub extern "C" fn jrg_panic_unwrap_none() -> ! {
    flush_stdout();
    eprint_line(format_args!("unwrap() pada Option::None"));
    mati()
}

#[no_mangle]
pub extern "C" fn jrg_panic_unwrap_err(ptr: *const u8, len: i64) -> ! {
    let s = unsafe { borrow_str(ptr, len) };
    flush_stdout();
    eprint_line(format_args!("unwrap() pada Result::Err: {}", s));
    mati()
}

#[no_mangle]
pub extern "C" fn jrg_assert_fail(ptr: *const u8, len: i64) -> ! {
    let s = unsafe { borrow_str(ptr, len) };
    flush_stdout();
    eprint_line(format_args!("assert gagal: {}", s));
    mati()
}

// ─────────────────────────────────────────────────────────────────────────
// Misc builtins
// ─────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn jrg_exit(code: i32) -> ! {
    flush_stdout();
    unsafe { exit(code) }
}

// ─── konversi ────────────────────────────────────────────────────────────
// Kalimatnya dibangun jrgc saat kompilasi dari `interpreter::konversi`; di sini
// cuma NILAINYA yang disisipkan, dicetak dengan cara acuan.

fn panik_sisip(pre: (*const u8, i64), nilai: &dyn fmt::Display, suf: (*const u8, i64)) -> ! {
    let pre = unsafe { borrow_str(pre.0, pre.1) };
    let suf = unsafe { borrow_str(suf.0, suf.1) };
    flush_stdout();
    eprint_line(format_args!("{}{}{}", pre, nilai, suf));
    mati()
}

#[no_mangle]
pub extern "C" fn jrg_panic_sisip_bulat(
    pre_ptr: *const u8, pre_len: i64, lo: u64, hi: u64, tak_bertanda: i32,
    suf_ptr: *const u8, suf_len: i64,
) -> ! {
    let u = ((hi as u128) << 64) | (lo as u128);
    if tak_bertanda != 0 {
        panik_sisip((pre_ptr, pre_len), &u, (suf_ptr, suf_len))
    } else {
        panik_sisip((pre_ptr, pre_len), &(u as i128), (suf_ptr, suf_len))
    }
}

#[no_mangle]
pub extern "C" fn jrg_panic_sisip_f64(
    pre_ptr: *const u8, pre_len: i64, v: f64, suf_ptr: *const u8, suf_len: i64,
) -> ! {
    panik_sisip((pre_ptr, pre_len), &format_f64_jrg(v), (suf_ptr, suf_len))
}

#[no_mangle]
pub extern "C" fn jrg_panic_sisip_rune(
    pre_ptr: *const u8, pre_len: i64, cp: u32, suf_ptr: *const u8, suf_len: i64,
) -> ! {
    let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
    panik_sisip((pre_ptr, pre_len), &c, (suf_ptr, suf_len))
}

/// Teksnya dicetak sesudah di-trim, sama dengan yang disebut acuannya.
#[no_mangle]
pub extern "C" fn jrg_panic_sisip_teks(
    pre_ptr: *const u8, pre_len: i64, s_ptr: *const u8, s_len: i64,
    suf_ptr: *const u8, suf_len: i64,
) -> ! {
    let t = unsafe { borrow_str(s_ptr, s_len) }.trim();
    panik_sisip((pre_ptr, pre_len), &t, (suf_ptr, suf_len))
}

/// Teks → bilangan bulat 128 bit. Kodenya urutan `GagalTeksBulat` milik acuan:
/// 0 berhasil, 1 kosong, 2 di luar 128 bit, 3 bukan bilangan bulat, 4 bukan
/// bilangan bulat tapi terbaca sebagai pecahan.
#[no_mangle]
pub extern "C" fn jrg_urai_bulat(ptr: *const u8, len: i64, out_lo: *mut u64, out_hi: *mut u64) -> i32 {
    use core::num::IntErrorKind;
    let t = unsafe { borrow_str(ptr, len) }.trim();
    match t.parse::<i128>() {
        Ok(v) => {
            let u = v as u128;
            unsafe { *out_lo = u as u64; *out_hi = (u >> 64) as u64; }
            0
        }
        Err(e) => match e.kind() {
            IntErrorKind::Empty => 1,
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => 2,
            _ if t.parse::<f64>().is_ok() => 4,
            _ => 3,
        },
    }
}

/// Teks → pecahan: 0 berhasil, 1 kosong, 2 bukan angka (`GagalTeksPecahan`).
#[no_mangle]
pub extern "C" fn jrg_urai_pecahan(ptr: *const u8, len: i64, out: *mut f64) -> i32 {
    let t = unsafe { borrow_str(ptr, len) }.trim();
    if t.is_empty() { return 1; }
    match t.parse::<f64>() {
        Ok(v) => { unsafe { *out = v }; 0 }
        Err(_) => 2,
    }
}

/// Read one byte from stdin.
fn read_byte() -> Option<u8> {
    let mut b = 0u8;
    loop {
        let n = unsafe { read(FD_STDIN, &mut b as *mut u8, 1) };
        if n == 1 {
            return Some(b);
        }
        if n == 0 {
            return None; // EOF
        }
        if n < 0 {
            return None; // error
        }
    }
}

/// Reads one line from stdin (trailing `\n`/`\r\n` stripped), mirroring
/// the interpreter's `tok()`. Returns 0 on EOF/error (out-params
/// untouched), 1 on success.
#[no_mangle]
pub extern "C" fn jrg_read_line(out_ptr: *mut *const u8, out_len: *mut i64) -> i32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut saw_any = false;
    while let Some(b) = read_byte() {
        saw_any = true;
        if b == b'\n' {
            break;
        }
        bytes.push(b);
    }
    if !saw_any {
        return 0; // EOF, nothing read
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    match String::from_utf8(bytes) {
        Ok(line) => {
            unsafe { emit_string(line, out_ptr, out_len) };
            1
        }
        Err(_) => 0,
    }
}

/// Reads all remaining stdin as one string (used by any future `read_all`
/// builtin); exposed now since it's a one-liner over the same plumbing.
#[no_mangle]
pub extern "C" fn jrg_read_all(out_ptr: *mut *const u8, out_len: *mut i64) -> i32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = unsafe { read(FD_STDIN, chunk.as_mut_ptr(), chunk.len()) };
        if n <= 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n as usize]);
    }
    match String::from_utf8(bytes) {
        Ok(buf) => { unsafe { emit_string(buf, out_ptr, out_len) }; 1 }
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn jrg_ipow_i64(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        flush_stdout();
        eprint_line(format_args!(
            "`{} ** {}`: pangkat negatif menjawab pecahan, sedangkan di backend LLVM `int ** int` \
             bertipe `int` kecuali eksponennya ditulis negatif langsung (`2 ** -1`) — tulis \
             `(x as float) ** n` (dua mesin lain menjawab pecahan di sini)", base, exp));
        mati()
    }
    // Dulu `exp as u32` memotong (2 ** 4294967296 → 1) dan hasilnya membungkus 64 bit.
    match base {
        0 => return if exp == 0 { 1 } else { 0 },
        1 => return 1,
        -1 => return if exp % 2 == 0 { 1 } else { -1 },
        _ => {}
    }
    let mut acc: i128 = 1;
    for _ in 0..exp {
        acc = match acc.checked_mul(base as i128) {
            Some(v) => v,
            None => {
                flush_stdout();
                eprint_line(format_args!(
                    "hasil `{} ** {}` kelewat besar buat integer — integer JRG maksimal 128 bit \
                     (kira-kira 1.7e38). Kalau memang butuh angka sebesar itu, pakai float \
                     (mis. `{}.0 ** {}.0`), tapi ingat float tidak eksak untuk bilangan bulat besar",
                    base, exp, base, exp));
                mati()
            }
        };
    }
    match i64::try_from(acc) {
        Ok(v) => v,
        Err(_) => {
            flush_stdout();
            eprint_line(format_args!(
                "hasil `{} ** {}` = {} tidak muat di `int` 64 bit backend LLVM (acuannya 128 bit) \
                 — jalankan tanpa `--emit-llvm`, atau pakai float", base, exp, acc));
            mati()
        }
    }
}

#[no_mangle]
pub extern "C" fn jrg_fact_i64(n: i64) -> i64 {
    if n < 0 {
        flush_stdout();
        eprint_line(format_args!(
            "math::factorial: n harus >= 0 (dapat {})",
            n
        ));
        mati()
    }
    // Dihitung 128 bit seperti acuan; dulu membungkus di 64 bit (`factorial(40)` → negatif).
    let mut acc: i128 = 1;
    let mut i: i64 = 2;
    while i <= n {
        acc = match acc.checked_mul(i as i128) {
            Some(v) => v,
            None => {
                flush_stdout();
                eprint_line(format_args!(
                    "math::factorial({}) kelewat besar buat integer 128 bit — mulai kelewat di \
                     sekitar {}!. Pakai float kalau cuma butuh perkiraan besarannya", n, i));
                mati()
            }
        };
        i += 1;
    }
    match i64::try_from(acc) {
        Ok(v) => v,
        Err(_) => {
            flush_stdout();
            eprint_line(format_args!(
                "math::factorial({}) = {} tidak muat di `int` 64 bit backend LLVM (acuannya 128 \
                 bit) — jalankan tanpa `--emit-llvm`, atau pakai float", n, acc));
            mati()
        }
    }
}

#[no_mangle]
pub extern "C" fn jrg_gcd_i64(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.wrapping_abs(), b.wrapping_abs());
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

#[no_mangle]
pub extern "C" fn jrg_lcm_i64(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 { return 0; }
    let g = jrg_gcd_i64(a, b);
    (a / g * b).wrapping_abs()
}

#[no_mangle]
pub extern "C" fn jrg_sign_f64(v: f64) -> f64 {
    if v.is_nan() { v } else if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 }
}

#[no_mangle]
pub extern "C" fn jrg_sign_i64(v: i64) -> i64 {
    if v > 0 { 1 } else if v < 0 { -1 } else { 0 }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn char_at(s: &str, idx: i64) -> u32 {
        jrg_str_char_at(s.as_ptr(), s.len() as i64, idx)
    }

    /// The optimized `char_at` keeps a one-entry memo of the last string and
    /// position it visited.
    fn assert_matches_naive(s: &str) {
        let n = s.chars().count() as i64;
        // forward scan — the fast path the memo exists for
        for i in 0..n {
            let want = s.chars().nth(i as usize).unwrap() as u32;
            assert_eq!(char_at(s, i), want, "maju: {:?} idx {}", s, i);
        }
        // backward scan — every call misses the memo's forward assumption
        for i in (0..n).rev() {
            let want = s.chars().nth(i as usize).unwrap() as u32;
            assert_eq!(char_at(s, i), want, "mundur: {:?} idx {}", s, i);
        }
        // out of range and negative both yield 0, as before
        assert_eq!(char_at(s, n), 0, "tepat di ujung: {:?}", s);
        assert_eq!(char_at(s, n + 50), 0, "jauh di luar: {:?}", s);
        assert_eq!(char_at(s, -1), 0, "index negatif: {:?}", s);
        // ...and an in-range index still works after an out-of-range one
        if n > 0 {
            let want = s.chars().next().unwrap() as u32;
            assert_eq!(char_at(s, 0), want, "setelah OOB: {:?}", s);
        }
    }

    #[test]
    fn char_at_matches_naive_for_ascii_and_unicode() {
        assert_matches_naive("");
        assert_matches_naive("a");
        assert_matches_naive("hello world");
        assert_matches_naive("héllo wörld");        // 2-byte
        assert_matches_naive("snowman ☃ here");     // 3-byte
        assert_matches_naive("math 𝔘 sym");         // 4-byte
        assert_matches_naive("mix é☃𝔘 aZ9");        // all widths together
    }

    /// Alternating between two strings makes every call a memo miss, which
    /// must fall back to a full rescan rather than reusing the other
    /// string's cached byte offset.
    #[test]
    fn char_at_interleaved_across_strings_is_correct() {
        let a = "abcdef";
        let b = "héllo☃";
        let an = a.chars().count() as i64;
        for i in 0..an {
            assert_eq!(char_at(a, i), a.chars().nth(i as usize).unwrap() as u32);
            assert_eq!(char_at(b, i), b.chars().nth(i as usize).unwrap() as u32);
        }
    }

    /// Reading the same index repeatedly must be stable — the memo parks on
    /// the char it returned, so an off-by-one would show up immediately.
    #[test]
    fn char_at_repeated_index_is_stable() {
        let s = "héllo☃𝔘";
        for _ in 0..5 {
            assert_eq!(char_at(s, 0), 'h' as u32);
            assert_eq!(char_at(s, 1), 'é' as u32);
            assert_eq!(char_at(s, 5), '☃' as u32);
            assert_eq!(char_at(s, 5), '☃' as u32);
            assert_eq!(char_at(s, 6), '𝔘' as u32);
        }
    }

    /// `emit_string` leaks the buffer without shrinking it; the caller only
    /// ever sees `(ptr, len)`, so spare capacity must not leak into `len`.
    #[test]
    fn emit_string_reports_exact_length_not_capacity() {
        let mut s = String::with_capacity(128);
        s.push_str("héllo");
        let expected = s.clone();
        let mut ptr: *const u8 = core::ptr::null();
        let mut len: i64 = 0;
        unsafe { emit_string(s, &mut ptr, &mut len) };
        assert_eq!(len, expected.len() as i64);
        let got = unsafe { borrow_str(ptr, len) };
        assert_eq!(got, expected);
    }

    #[test]
    fn emit_string_handles_empty_without_null_pointer() {
        let mut ptr: *const u8 = core::ptr::null();
        let mut len: i64 = -1;
        unsafe { emit_string(String::new(), &mut ptr, &mut len) };
        assert_eq!(len, 0);
        assert!(!ptr.is_null(), "pointer kosong harus tetap non-null");
        assert_eq!(unsafe { borrow_str(ptr, len) }, "");
    }
}

/// Pecah `s` menurut `sep`, hasilnya buffer `{ ptr, i64 }` sebanyak potongan.
#[no_mangle]
pub extern "C" fn jrg_str_split(
    sptr: *const u8, slen: i64,
    pptr: *const u8, plen: i64,
    out_ptr: *mut *const u8, out_len: *mut i64,
) {
    let s = unsafe { borrow_str(sptr, slen) };
    let sep = unsafe { borrow_str(pptr, plen) };
    let bagian: Vec<&str> = s.split(sep).collect();

    let n = bagian.len();
    let byte = n.saturating_mul(core::mem::size_of::<KunciStr>());
    let buf = jrg_alloc(byte as i64, 8) as *mut KunciStr;
    for (i, b) in bagian.iter().enumerate() {
        unsafe {
            core::ptr::write(buf.add(i), KunciStr { p: b.as_ptr(), n: b.len() as i64 });
        }
    }
    unsafe {
        *out_ptr = buf as *const u8;
        *out_len = n as i64;
    }
}

// ── `defer` ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EntriTunda {
    pub jalan: extern "C" fn(*mut u8),
    pub arg: *mut u8,
}

#[repr(C)]
pub struct DaftarTunda {
    pub ptr: *mut EntriTunda,
    pub len: i64,
    pub kap: i64,
}

/// Tumpukan `defer` korutin yang sedang jalan; `korutin.c` menukarnya tiap pindah.
#[no_mangle]
pub static mut jrg_tunda_kini: DaftarTunda =
    DaftarTunda { ptr: core::ptr::null_mut(), len: 0, kap: 0 };

/// Tinggi tumpukan saat sebuah fungsi masuk; ia menjalankan sampai ke sini saat pulang.
#[no_mangle]
pub extern "C" fn jrg_tunda_tinggi() -> i64 {
    unsafe { (*core::ptr::addr_of!(jrg_tunda_kini)).len }
}

#[no_mangle]
pub extern "C" fn jrg_tunda_push(jalan: extern "C" fn(*mut u8), arg: *mut u8) {
    let d = unsafe { &mut *core::ptr::addr_of_mut!(jrg_tunda_kini) };
    if d.len == d.kap {
        let kap_baru = if d.kap == 0 { 8 } else { d.kap * 2 };
        let byte = (kap_baru as usize) * core::mem::size_of::<EntriTunda>();
        let baru = jrg_alloc(byte as i64, 8) as *mut EntriTunda;
        if d.len > 0 {
            unsafe { core::ptr::copy_nonoverlapping(d.ptr, baru, d.len as usize) };
        }
        d.ptr = baru;
        d.kap = kap_baru;
    }
    unsafe { core::ptr::write(d.ptr.offset(d.len as isize), EntriTunda { jalan, arg }) };
    d.len += 1;
}

/// Jalankan entri dari yang terakhir sampai tumpukannya setinggi `dasar`. Entrinya
/// dilepas SEBELUM dipanggil, jadi yang panik di tengah tidak dijalankan dua kali.
#[no_mangle]
pub extern "C" fn jrg_tunda_jalankan(dasar: i64) {
    loop {
        let e = unsafe {
            let d = &mut *core::ptr::addr_of_mut!(jrg_tunda_kini);
            if d.len <= dasar || d.len <= 0 { return; }
            d.len -= 1;
            *d.ptr.offset(d.len as isize)
        };
        (e.jalan)(e.arg);
    }
}

/// Lupakan semua `defer` korutin ini: stack yang habis tidak bisa menjalankannya.
#[no_mangle]
pub extern "C" fn jrg_tunda_buang() {
    unsafe { TAK_GUGUR = true };
    unsafe { (*core::ptr::addr_of_mut!(jrg_tunda_kini)).len = 0 };
}

// ── `jawa::` / `jawir::` — alokasi manual ────────────────────────────────

const JAWA_TUNGGAL: i32 = 0;
const JAWA_ARRAY: i32 = 1;
const JAWA_BUFFER: i32 = 2;

struct SlotJawa {
    jenis: i32,
    sah: bool,
    nilai: i64,
    larik: Vec<i64>,
    /// Sejajar `larik`: elemen yang sudah pernah ditulis.
    terisi: Vec<bool>,
    buf: Vec<u8>,
}

// Indeks 0 tidak dipakai, supaya id 0 selalu berarti "bukan handle".
static mut REGISTRI: Vec<SlotJawa> = Vec::new();

#[allow(static_mut_refs)]
unsafe fn registri() -> &'static mut Vec<SlotJawa> {
    if REGISTRI.is_empty() {
        REGISTRI.push(SlotJawa { jenis: -1, sah: false, nilai: 0, larik: Vec::new(), terisi: Vec::new(), buf: Vec::new() });
    }
    &mut REGISTRI
}

unsafe fn slot(id: i64, jenis: i32, nama: &str) -> &'static mut SlotJawa {
    let r = registri();
    if id <= 0 || id as usize >= r.len() {
        jrg_panic_str(&alloc::format!("{}: handle tidak dikenal", nama));
    }
    let s = &mut r[id as usize];
    if !s.sah {
        // Kata demi kata sama dengan interpreter: pesan yang MIRIP tapi tidak
        // sama membuat pembaca mengira ia menemui masalah yang lain.
        let apa = match s.jenis {
            JAWA_ARRAY => "array",
            JAWA_BUFFER => "buffer",
            _ => "pointer",
        };
        jrg_panic_str(&alloc::format!(
            "{}: use-after-free — {} ini sudah di-`jawir::free`", nama, apa));
    }
    if jenis >= 0 && s.jenis != jenis {
        let asal = |j: i32| match j { JAWA_ARRAY => "array", JAWA_BUFFER => "buffer", _ => "alloc" };
        jrg_panic_str(&alloc::format!(
            "{}: handle ini hasil `jawa::{}(...)`, bukan `jawa::{}(...)` — pakai fungsi yang sesuai buat jenis ini",
            nama, asal(s.jenis), asal(jenis)));
    }
    s
}

fn jrg_panic_str(pesan: &str) -> ! {
    jrg_panic(pesan.as_ptr(), pesan.len() as i64);
    unreachable!()
}

unsafe fn slot_baru(s: SlotJawa) -> i64 {
    let r = registri();
    r.push(s);
    (r.len() - 1) as i64
}

#[no_mangle]
pub extern "C" fn jrg_jawa_alloc(v: i64) -> i64 {
    unsafe { slot_baru(SlotJawa { jenis: JAWA_TUNGGAL, sah: true, nilai: v, larik: Vec::new(), terisi: Vec::new(), buf: Vec::new() }) }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_get(id: i64) -> i64 {
    unsafe { slot(id, JAWA_TUNGGAL, "jawa::get").nilai }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_set(id: i64, v: i64) {
    unsafe { slot(id, JAWA_TUNGGAL, "jawa::set").nilai = v };
}

/// `is_valid` TIDAK boleh panik pada handle yang sudah dibebaskan — justru
/// itu pertanyaannya. Jadi ia tidak lewat `slot`.
#[no_mangle]
pub extern "C" fn jrg_jawa_valid(id: i64) -> i32 {
    unsafe {
        let r = registri();
        if id <= 0 || id as usize >= r.len() { return 0; }
        if r[id as usize].sah { 1 } else { 0 }
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_free(id: i64) {
    unsafe {
        let r = registri();
        if id <= 0 || id as usize >= r.len() {
            jrg_panic_str("jawir::free: handle tidak dikenal");
        }
        let s = &mut r[id as usize];
        if !s.sah {
            jrg_panic_str(
                "jawir::free: double free — pointer ini sudah pernah di-`jawir::free` sebelumnya");
        }
        s.sah = false;
        s.larik = Vec::new();
        s.buf = Vec::new();
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_array(n: i64) -> i64 {
    let n = if n < 0 { 0 } else { n as usize };
    let mut v = Vec::new();
    v.resize(n, 0i64);
    let mut terisi = Vec::new();
    terisi.resize(n, false);
    unsafe { slot_baru(SlotJawa { jenis: JAWA_ARRAY, sah: true, nilai: 0, larik: v, terisi, buf: Vec::new() }) }
}

/// `jawa::array(n, isi)`: semua elemen terisi `isi`, seperti acuannya.
#[no_mangle]
pub extern "C" fn jrg_jawa_array_isi(n: i64, isi: i64) -> i64 {
    let n = if n < 0 { 0 } else { n as usize };
    let mut v = Vec::new();
    v.resize(n, isi);
    let mut terisi = Vec::new();
    terisi.resize(n, true);
    unsafe { slot_baru(SlotJawa { jenis: JAWA_ARRAY, sah: true, nilai: 0, larik: v, terisi, buf: Vec::new() }) }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_array_len(id: i64) -> i64 {
    unsafe { slot(id, JAWA_ARRAY, "jawa::array_len").larik.len() as i64 }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_array_get(id: i64, i: i64) -> i64 {
    unsafe {
        let s = slot(id, JAWA_ARRAY, "jawa::array_get");
        if i < 0 || i as usize >= s.larik.len() {
            jrg_panic_str(&alloc::format!(
                "jawa::array_get: index {} di luar jangkauan — panjangnya {}", i, s.larik.len()));
        }
        // Acuan menjawab `nil` untuk elemen yang belum diisi; di sini tidak ada `nil` di larik.
        if !s.terisi[i as usize] {
            jrg_panic_str(&alloc::format!(
                "jawa::array_get: elemen ke-{} belum pernah diisi — di backend LLVM larik `jawa::` \
                 cuma menyimpan bilangan bulat, jadi tidak ada `nil` untuk dijawab; isi dulu \
                 dengan `jawa::array_set`", i));
        }
        s.larik[i as usize]
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_array_set(id: i64, i: i64, v: i64) {
    unsafe {
        let s = slot(id, JAWA_ARRAY, "jawa::array_set");
        if i < 0 || i as usize >= s.larik.len() {
            jrg_panic_str(&alloc::format!(
                "jawa::array_set: index {} di luar jangkauan — panjangnya {}", i, s.larik.len()));
        }
        s.larik[i as usize] = v;
        s.terisi[i as usize] = true;
    }
}

/// Menumbuhkan larik YANG SAMA lalu mengembalikan handle yang sama.
#[no_mangle]
pub extern "C" fn jrg_jawa_array_push(id: i64, v: i64) -> i64 {
    unsafe {
        let s = slot(id, JAWA_ARRAY, "jawa::array_push");
        s.larik.push(v);
        s.terisi.push(true);
        id
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_array_pop(id: i64) -> i64 {
    unsafe {
        let s = slot(id, JAWA_ARRAY, "jawa::array_pop");
        let (Some(v), Some(isi)) = (s.larik.pop(), s.terisi.pop()) else {
            jrg_panic_str("jawa::array_pop: array kosong")
        };
        if !isi {
            jrg_panic_str("jawa::array_pop: elemen terakhirnya belum pernah diisi — di backend LLVM larik \
                `jawa::` cuma menyimpan bilangan bulat, jadi tidak ada `nil` untuk dijawab");
        }
        v
    }
}

/// Isi larik sebagai penyangga `i64` baru; panjangnya dibaca lewat `jrg_jawa_array_len`.
#[no_mangle]
pub extern "C" fn jrg_jawa_array_salin(id: i64) -> *const i64 {
    unsafe {
        let s = slot(id, JAWA_ARRAY, "jawa::array_to_list");
        if let Some(i) = s.terisi.iter().position(|t| !*t) {
            jrg_panic_str(&alloc::format!(
                "jawa::array_to_list: elemen ke-{} belum pernah diisi — di backend LLVM larik `jawa::` \
                 cuma menyimpan bilangan bulat, jadi tidak ada `nil` untuk dijawab", i));
        }
        let buf = jrg_alloc((s.larik.len().max(1) * 8) as i64, 8) as *mut i64;
        for (i, x) in s.larik.iter().enumerate() { core::ptr::write(buf.add(i), *x); }
        buf
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_buffer_fill(id: i64, v: i64) {
    unsafe {
        let s = slot(id, JAWA_BUFFER, "jawa::buffer_fill");
        if !(0..=255).contains(&v) {
            jrg_panic_str(&alloc::format!("jawa::buffer_fill: nilai {} di luar jangkauan byte (0..255)", v));
        }
        for b in s.buf.iter_mut() { *b = v as u8; }
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_buffer_to_hex(id: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    unsafe {
        let s = slot(id, JAWA_BUFFER, "jawa::buffer_to_hex");
        let mut t = String::with_capacity(s.buf.len() * 2);
        for b in s.buf.iter() { let _ = write!(t, "{:02x}", b); }
        emit_string(t, out_ptr, out_len);
    }
}

/// Ukuran isi sebuah alokasi, dengan taksiran acuan: bilangan bulat 16 byte, byte buffer 1.
#[no_mangle]
pub extern "C" fn jrg_jawa_sizeof(id: i64) -> i64 {
    unsafe {
        let s = slot(id, -1, "jawa::sizeof");
        match s.jenis {
            JAWA_ARRAY => s.terisi.iter().filter(|t| **t).count() as i64 * 16,
            JAWA_BUFFER => s.buf.len() as i64,
            _ => 16,
        }
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_buffer(n: i64) -> i64 {
    let n = if n < 0 { 0 } else { n as usize };
    let mut v = Vec::new();
    v.resize(n, 0u8);
    unsafe { slot_baru(SlotJawa { jenis: JAWA_BUFFER, sah: true, nilai: 0, larik: Vec::new(), terisi: Vec::new(), buf: v }) }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_buffer_len(id: i64) -> i64 {
    unsafe { slot(id, JAWA_BUFFER, "jawa::buffer_len").buf.len() as i64 }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_buffer_get(id: i64, i: i64) -> i64 {
    unsafe {
        let s = slot(id, JAWA_BUFFER, "jawa::buffer_get");
        if i < 0 || i as usize >= s.buf.len() {
            jrg_panic_str(&alloc::format!(
                "jawa::buffer_get: index {} di luar jangkauan — panjangnya {}", i, s.buf.len()));
        }
        s.buf[i as usize] as i64
    }
}

#[no_mangle]
pub extern "C" fn jrg_jawa_buffer_set(id: i64, i: i64, v: i64) {
    unsafe {
        let s = slot(id, JAWA_BUFFER, "jawa::buffer_set");
        if i < 0 || i as usize >= s.buf.len() {
            jrg_panic_str(&alloc::format!(
                "jawa::buffer_set: index {} di luar jangkauan — panjangnya {}", i, s.buf.len()));
        }
        if !(0..=255).contains(&v) {
            jrg_panic_str(&alloc::format!(
                "jawa::buffer_set: nilai {} di luar 0..255 — buffer isinya byte", v));
        }
        s.buf[i as usize] = v as u8;
    }
}

// ── `str::` yang butuh golongan karakter Unicode ────────────────────────
// Tiap badan di bawah SAMA dengan milik interpreter (`builtin/str_ops.rs`), ekspresi demi
// ekspresi; `tests/llvm_parity.rs` membandingkan keduanya.

#[no_mangle]
pub extern "C" fn jrg_str_capitalize(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    let mut c = s.chars();
    let r = match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    };
    unsafe { emit_string(r, out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_title(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    let r: String = s.split_whitespace().map(|w| {
        let mut ch = w.chars();
        match ch.next() { None => String::new(), Some(c) => c.to_uppercase().to_string() + ch.as_str() }
    }).collect::<Vec<_>>().join(" ");
    unsafe { emit_string(r, out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_snake_case(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    let mut r = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 { r.push('_'); }
        r.push(c.to_lowercase().next().unwrap_or(c));
    }
    unsafe { emit_string(r.replace(' ', "_"), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_camel_case(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    let words: Vec<&str> = s.split(|c: char| c == '_' || c == ' ').collect();
    let r: String = words.iter().enumerate().map(|(i, w)| {
        if i == 0 { w.to_lowercase() } else {
            let mut ch = w.chars();
            match ch.next() { None => String::new(), Some(c) => c.to_uppercase().to_string() + &ch.as_str().to_lowercase() }
        }
    }).collect();
    unsafe { emit_string(r, out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_swap_case(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    let r: String = s.chars().map(|c| {
        if c.is_uppercase() { c.to_lowercase().next().unwrap_or(c) }
        else if c.is_lowercase() { c.to_uppercase().next().unwrap_or(c) }
        else { c }
    }).collect();
    unsafe { emit_string(r, out_ptr, out_len) };
}

/// Potongan dari masukannya sendiri: nol alokasi, seperti `jrg_str_trim`.
unsafe fn emit_potongan(t: &str, out_ptr: *mut *const u8, out_len: *mut i64) {
    *out_ptr = if t.is_empty() { core::ptr::NonNull::<u8>::dangling().as_ptr() as *const u8 } else { t.as_ptr() };
    *out_len = t.len() as i64;
}

#[no_mangle]
pub extern "C" fn jrg_str_trim_start(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    unsafe { emit_potongan(s.trim_start(), out_ptr, out_len) };
}

#[no_mangle]
pub extern "C" fn jrg_str_trim_end(ptr: *const u8, len: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(ptr, len) };
    unsafe { emit_potongan(s.trim_end(), out_ptr, out_len) };
}

/// Predikat atas seluruh teks: 0 alpha, 1 alphanumeric, 2 lower, 3 upper, 4 numeric.
#[no_mangle]
pub extern "C" fn jrg_str_adalah(kode: i32, ptr: *const u8, len: i64) -> i32 {
    let s = unsafe { borrow_str(ptr, len) };
    let r = match kode {
        0 => !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
        1 => !s.is_empty() && s.chars().all(|c| c.is_alphanumeric()),
        2 => !s.is_empty() && s.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) && s.chars().any(|c| c.is_alphabetic()),
        3 => !s.is_empty() && s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()) && s.chars().any(|c| c.is_alphabetic()),
        _ => {
            let t = s.trim();
            let t = t.strip_prefix('-').unwrap_or(t);
            let (bulat, pecahan) = t.split_once('.').unwrap_or((t, ""));
            let digit = |x: &str| x.chars().all(|c| c.is_ascii_digit());
            digit(bulat) && digit(pecahan) && !(bulat.is_empty() && pecahan.is_empty())
        }
    };
    r as i32
}

/// `str::words`: potongan antar-spasi, sebagai buffer `{ ptr, i64 }` seperti `jrg_str_split`.
#[no_mangle]
pub extern "C" fn jrg_str_words(sptr: *const u8, slen: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let s = unsafe { borrow_str(sptr, slen) };
    let bagian: Vec<&str> = s.split_whitespace().collect();
    let n = bagian.len();
    let buf = jrg_alloc(n.saturating_mul(core::mem::size_of::<KunciStr>()) as i64, 8) as *mut KunciStr;
    for (i, b) in bagian.iter().enumerate() {
        unsafe { core::ptr::write(buf.add(i), KunciStr { p: b.as_ptr(), n: b.len() as i64 }); }
    }
    unsafe {
        *out_ptr = buf as *const u8;
        *out_len = n as i64;
    }
}

/// `str::bytes`: tiap bita sebagai `i64`.
#[no_mangle]
pub extern "C" fn jrg_str_bytes(sptr: *const u8, slen: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
    let b = unsafe { borrow_bytes(sptr, slen) };
    let buf = jrg_alloc((b.len() * 8) as i64, 8) as *mut i64;
    for (i, x) in b.iter().enumerate() {
        unsafe { core::ptr::write(buf.add(i), *x as i64); }
    }
    unsafe {
        *out_ptr = buf as *const u8;
        *out_len = b.len() as i64;
    }
}

// ── sumber acak untuk `rand::` (dipakai lapis ketiga lewat `lapis::acak`) ──

static BENIH_ACAK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// 63 bit acak tak negatif: xorshift64*, benihnya dari `getentropy`.
#[no_mangle]
pub extern "C" fn jrg_acak63() -> i64 {
    use core::sync::atomic::Ordering;
    let mut x = BENIH_ACAK.load(Ordering::Relaxed);
    if x == 0 {
        extern "C" { fn getentropy(buf: *mut u8, len: usize) -> i32; }
        let mut b = [0u8; 8];
        let dari_os = unsafe { getentropy(b.as_mut_ptr(), 8) } == 0;
        // Tanpa sumber OS: alamat sebuah lokal (ASLR) — lemah, tapi tetap beda tiap jalan.
        x = if dari_os { u64::from_le_bytes(b) } else { (&b as *const _ as u64) ^ 0x9E3779B97F4A7C15 };
        if x == 0 { x = 0x9E3779B97F4A7C15; }
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    BENIH_ACAK.store(x, Ordering::Relaxed);
    (x.wrapping_mul(0x2545F4914F6CDD1D) >> 1) as i64
}

// ── `json::` untuk backend LLVM: pengurai/penyandi bersama atas nilai bertag ──
// `json_inti.rs` di sini SALINAN PERSIS `src/json_inti.rs` (dijaga tes di `tests/llvm_parity.rs`).
mod json_inti;

#[cfg(not(test))]
mod json_llvm {
    use super::json_inti::{self, Json};
    use super::{borrow_str, emit_string, jrg_alloc, jrg_panic};
    use alloc::format;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    /// `%JrgAny`: tag (8 bit bawah) + lukisan isi (24 bit atas), lalu dua kata muatan.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct KotakAny { tag: i32, sela: i32, a: u64, b: u64 }

    #[repr(C)]
    struct Dua { ptr: *mut u8, len: i64 }

    extern "C" {
        fn jrg_dict_tempat(d: *mut Dua, langkah: i64, jajar: i64, kp: *const u8, kl: i64) -> *mut u8;
        fn jrg_dict_rapikan(p: *mut u8, len: i64, langkah: i64);
        fn jrg_any_ubah(a: *const KotakAny, kata: i32, hasil: *mut KotakAny, pola: *const u8, pl: i64);
    }

    const LANGKAH: i64 = 40;                 // pasangan `{kunci 16 bita, nilai bertag 24 bita}`
    const KATA_ARRAY: i32 = 6 | 1 << 8;      // array ber-elemen bertag
    const KATA_DICT: i32 = 7 | 1 << 8;       // dict bernilai bertag

    fn gagal(pesan: String) -> ! { jrg_panic(pesan.as_ptr(), pesan.len() as i64) }

    fn bocorkan(s: String) -> (u64, u64) {
        let (mut p, mut n) = (core::ptr::null(), 0i64);
        unsafe { emit_string(s, &mut p, &mut n) };
        (p as u64, n as u64)
    }

    fn ke_any(j: Json) -> KotakAny {
        let k = |tag: i32, a: u64, b: u64| KotakAny { tag, sela: 0, a, b };
        match j {
            Json::Null => k(0, 0, 0),
            Json::Bool(b) => k(1, b as u64, 0),
            Json::Int(n) => match i64::try_from(n) {
                Ok(x) => k(2, x as u64, 0),
                Err(_) => gagal(format!(
                    "json::parse: bilangan bulat `{}` tidak muat di 64 bit backend `--emit-llvm` \
                     (dua mesin lain menyimpannya di 128 bit)", n)),
            },
            Json::Float(f) => k(3, f.to_bits(), 0),
            Json::Str(s) => { let (p, n) = bocorkan(s); k(5, p, n) }
            Json::Arr(a) => {
                let n = a.len();
                let buf = jrg_alloc((n.max(1) * 24) as i64, 8) as *mut KotakAny;
                for (i, x) in a.into_iter().enumerate() { unsafe { buf.add(i).write(ke_any(x)); } }
                k(KATA_ARRAY, buf as u64, n as u64)
            }
            Json::Obj(o) => {
                let mut d = Dua { ptr: core::ptr::null_mut(), len: 0 };
                for (kunci, v) in o {
                    let (kp, kl) = bocorkan(kunci);
                    let nilai = ke_any(v);
                    unsafe {
                        let pas = jrg_dict_tempat(&mut d, LANGKAH, 8, kp as *const u8, kl as i64);
                        (pas.add(16) as *mut KotakAny).write(nilai);
                    }
                }
                k(KATA_DICT, d.ptr as u64, d.len as u64)
            }
        }
    }

    /// Isi kotak sebagai array/dict ber-elemen bertag; isi yang tak terlukis gagal menyebut tipenya.
    unsafe fn seragamkan(a: &KotakAny, kata: i32) -> KotakAny {
        if a.tag == kata { return *a; }
        let pola = "nilai bertipe `\u{1}` tidak punya padanan JSON di backend `--emit-llvm`";
        let mut hasil = KotakAny { tag: 0, sela: 0, a: 0, b: 0 };
        jrg_any_ubah(a, kata, &mut hasil, pola.as_ptr(), pola.len() as i64);
        hasil
    }

    unsafe fn dari_any(a: &KotakAny) -> Json {
        match a.tag & 255 {
            1 => Json::Bool(a.a & 255 != 0),
            2 => Json::Int(a.a as i64 as i128),
            3 => Json::Float(f64::from_bits(a.a)),
            4 => Json::Str(char::from_u32(a.a as u32).map(|c| c.to_string()).unwrap_or_default()),
            5 => Json::Str(borrow_str(a.a as *const u8, a.b as i64).to_string()),
            6 => {
                let s = seragamkan(a, KATA_ARRAY);
                let p = s.a as *const KotakAny;
                let mut isi = Vec::with_capacity(s.b as usize);
                for i in 0..s.b as usize { isi.push(dari_any(&*p.add(i))); }
                Json::Arr(isi)
            }
            7 => {
                let s = seragamkan(a, KATA_DICT);
                let p = s.a as *mut u8;
                jrg_dict_rapikan(p, s.b as i64, LANGKAH);
                let mut isi = Vec::with_capacity(s.b as usize);
                for i in 0..s.b as usize {
                    let pas = p.add(i * LANGKAH as usize);
                    let kunci = &*(pas as *const Dua);
                    isi.push((borrow_str(kunci.ptr, kunci.len).to_string(), dari_any(&*(pas.add(16) as *const KotakAny))));
                }
                Json::Obj(isi)
            }
            0 | 8 => Json::Null,
            _ => gagal("nilai ini (struct, tuple, enum, fungsi, atau handle) tidak punya padanan JSON di backend `--emit-llvm`".to_string()),
        }
    }

    #[no_mangle]
    pub extern "C" fn jrg_json_parse(ptr: *const u8, len: i64, out: *mut KotakAny) {
        let s = unsafe { borrow_str(ptr, len) };
        match json_inti::urai(s) {
            Ok(j) => unsafe { out.write(ke_any(j)) },
            Err(e) => gagal(format!("json::parse: {}", e)),
        }
    }

    /// `rapi == 0`: rapat (`json::stringify`); selain itu rapi selebar `indent` (`json::pretty`).
    #[no_mangle]
    pub extern "C" fn jrg_json_tulis(a: *const KotakAny, rapi: i32, indent: i64, out_ptr: *mut *const u8, out_len: *mut i64) {
        if rapi != 0 && !(0..=16).contains(&indent) {
            gagal(format!("json::pretty: lebar indentasi {} di luar jangkauan 0 sampai 16", indent));
        }
        let j = unsafe { dari_any(&*a) };
        let lebar = if rapi == 0 { None } else { Some(indent as usize) };
        match json_inti::tulis(&j, lebar, 0) {
            Ok(t) => unsafe { emit_string(t, out_ptr, out_len) },
            Err(e) => gagal(e),
        }
    }
}
