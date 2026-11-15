//! Jalur lambat J4 (`dokumentasi/jit/j4-rencana.md`): kode native membaca dan menulis nilai milik VM
//! (`Jenis::Nilai`) lewat fungsi di sini, dengan aturan VM yang sama. Gagal (status 1) = belum dikerjakan
//! apa pun yang terlihat, jadi kode native deopt di instruksi itu dan VM yang menjalankannya.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use super::layak::Jenis;
use crate::vm::opcode::OpCode;
use crate::vm::value::VmValue;
use crate::vm::vm::{Vm, VmError};

thread_local! {
    /// Satu struct milik tunggal berisi skalar yang baru dilepas `buang`, untuk dipakai ulang `bangun`.
    static CADANGAN_STRUCT: Cell<Option<Arc<crate::vm::value::Peta>>> = const { Cell::new(None) };
    /// VM dan basis bingkai terluar yang sedang dijalankan kode native di thread ini.
    static KONTEKS: Cell<(*mut Vm, usize)> = const { Cell::new((std::ptr::null_mut(), 0)) };
    /// Panggilan langsung yang sedang berjalan (`masuk`/`keluar`): kedalaman bingkai pemanggil, basisnya,
    /// dan posisi mutlak slot fungsi callee.
    /// Galat dari instruksi yang dijalankan `langkah`; diambil `panggil::jalankan`.
    static GALAT: RefCell<Option<VmError>> = const { RefCell::new(None) };
}

/// Galat `langkah` terakhir di thread ini (status `GAGAL` kode native).
pub(crate) fn ambil_galat() -> VmError {
    GALAT.with(|g| g.borrow_mut().take()).unwrap_or_else(|| VmError::new("bug di jrgc: galat jalur lambat JIT hilang"))
}

/// VM dan basis bingkai terluar. Kode native berjalan di dalam `panggil::jalankan`, yang tidak
/// menyentuh VM sampai kode itu kembali.
fn konteks() -> (&'static mut Vm, usize) {
    let (vm, basis) = KONTEKS.with(|k| k.get());
    assert!(!vm.is_null(), "bug di jrgc: jalur lambat JIT tanpa VM");
    (unsafe { &mut *vm }, basis)
}

/// Posisi `p` bingkai terluar ada di stack VM (yang lebih pendek diisi `nil`). Dipanggil hampir tiap
/// pembantu: di-inline, yang jarang (stack lebih pendek dari puncak) di fungsi terpisah.
#[inline(always)]
fn sediakan(vm: &mut Vm, basis: usize, p: usize) -> &mut VmValue {
    let i = basis + p;
    // Posisi baru di puncak (sering): satu `push`; lebih tinggi dari itu (jarang): `resize` terpisah.
    if vm.stack.len() == i { vm.stack.push(VmValue::Nil); } else if vm.stack.len() < i { tumbuh(vm, i); }
    &mut vm.stack[i]
}

/// Celahnya biasanya satu-dua posisi (argumen native callee ringan yang tidak ditulis `masuk_ringan`): `Nil`
/// ditulis langsung ke kapasitas yang dipesan, tanpa `resize` (`clone` umum) atau `push` yang memeriksa ulang.
#[cold]
#[inline(never)]
fn tumbuh(vm: &mut Vm, i: usize) {
    let n = vm.stack.len();
    vm.stack.reserve(i + 1 - n);
    // Aman: `reserve` menjamin kapasitasnya, dan tiap posisi n..=i ditulis utuh sebelum panjangnya dinaikkan.
    unsafe {
        let p = vm.stack.as_mut_ptr();
        for k in n..=i { std::ptr::write(p.add(k), VmValue::Nil); }
        vm.stack.set_len(i + 1);
    }
}

/// Isi struct yang dipakai ulang ditulis dari posisi `rentang` (angka native atau nilai milik VM yang dipindah).
#[inline(always)]
fn isi_ulang(vm: &mut Vm, basis: usize, jenis: &[Jenis], nilai: &[i64], rentang: std::ops::Range<usize>, isi: &mut [VmValue]) {
    for (j, i) in rentang.enumerate() {
        // Angka atas angka sejenis (biasa: struct yang diperbarui per langkah): cuma isinya yang ditulis.
        match (jenis[i], &mut isi[j]) {
            (Jenis::Pecahan, VmValue::Float(x)) => { *x = f64::from_bits(nilai[i] as u64); continue }
            (Jenis::Bulat, VmValue::Int(x)) => { *x = nilai[i] as i128; continue }
            _ => {}
        }
        let v = match nilai_skalar(jenis[i], nilai[i]) {
            Some(v) => v,
            None => std::mem::replace(sediakan(vm, basis, i), VmValue::Nil),
        };
        timpa(&mut isi[j], v);
    }
}

/// Isi tanpa destruktor: cuma struct seperti ini yang boleh disimpan di `CADANGAN_STRUCT`, jadi menahannya
/// tidak memperpanjang umur pegangan apa pun (FFI, kanal) yang bisa diamati.
#[inline(always)]
fn skalar(v: &VmValue) -> bool {
    matches!(v, VmValue::Nil | VmValue::Unit | VmValue::Bool(_) | VmValue::Int(_) | VmValue::Rune(_)
        | VmValue::Float(_) | VmValue::Ptr(_))
}

/// `*tempat = v` tanpa memanggil `drop_glue` kalau isi lamanya skalar (yang biasa di posisi stack VM).
#[inline(always)]
fn timpa(tempat: &mut VmValue, v: VmValue) {
    if skalar(tempat) {
        unsafe { std::ptr::write(tempat, v) }
    } else {
        *tempat = v;
    }
}

/// `Vec::truncate` untuk bingkai callee: skalar tanpa destruktor dilewati tanpa memanggil `drop_glue`. Panjangnya
/// dipendekkan lebih dulu, jadi destruktor yang panik membocorkan sisanya, bukan melepasnya dua kali.
#[inline(always)]
pub(super) fn potong(stack: &mut Vec<VmValue>, n: usize) {
    let len = stack.len();
    if n >= len { return; }
    unsafe {
        stack.set_len(n);
        let p = stack.as_mut_ptr();
        for k in n..len {
            let v = &mut *p.add(k);
            if !matches!(v, VmValue::Nil | VmValue::Unit | VmValue::Bool(_) | VmValue::Int(_) | VmValue::Rune(_)
                | VmValue::Float(_) | VmValue::Ptr(_)) {
                std::ptr::drop_in_place(v);
            }
        }
    }
}

/// Satu `match` atas jenisnya (sama artinya dengan `skalar_kode(j.kode())`, tanpa pembandingan `Lebar` dan
/// kode antara): dipanggil per posisi di `bangun`, `masuk`, `bawaan`.
#[inline(always)]
fn nilai_skalar(j: Jenis, v: i64) -> Option<VmValue> {
    Some(match j {
        Jenis::Bulat => VmValue::Int(v as i128),
        Jenis::Pecahan => VmValue::Float(f64::from_bits(v as u64)),
        Jenis::Bool => VmValue::Bool(v != 0),
        Jenis::Kosong => VmValue::Nil,
        Jenis::Satuan => VmValue::Unit,
        Jenis::Rune => VmValue::Rune(char::from_u32(v as u32).expect("kode rune dari char")),
        _ => return None,
    })
}

/// Satu definisi isi native → nilai VM menurut `Jenis::kode`; `None` = milik VM (4) atau `Lebar` (6).
pub(crate) fn skalar_kode(kode: i32, nilai: i64) -> Option<VmValue> {
    Some(match kode {
        1 => VmValue::Int(nilai as i128),
        2 => VmValue::Bool(nilai != 0),
        3 => VmValue::Nil,
        5 => VmValue::Float(f64::from_bits(nilai as u64)),
        // Kode rune selalu lahir dari `char` (`karakter`, `buka` kode 7).
        7 => VmValue::Rune(char::from_u32(nilai as u32).expect("kode rune dari char")),
        8 => VmValue::Unit,
        _ => return None,
    })
}

/// Satu situs `Cara::Bangun`, disiapkan saat kompilasi: struct (label + bentuk prototipe), larik, atau tuple.
pub enum SitusBangun {
    Struktur(crate::vm::value::Label, Arc<crate::vm::peta::Bentuk>),
    /// `Struktur` yang langsung dikembalikan (`Return` sesudahnya): lokal bingkai ini yang sebentuk, berlabel sama
    /// dan milik tunggal — sesudah `Return` tidak terlihat siapa pun — dipakai ulang, nilainya ditimpa di tempat.
    StrukturUlang(crate::vm::value::Label, Arc<crate::vm::peta::Bentuk>),
    Larik,
    Tuple,
    /// `BuildEnum`: nama varian milik prototipenya, payloadnya satu posisi.
    Varian(&'static crate::vm::value::NamaVarian),
}

impl SitusBangun {
    /// Hidup selama kode yang memakainya (kode JIT tidak pernah dibebaskan).
    pub fn bocor(s: SitusBangun) -> &'static SitusBangun { Box::leak(Box::new(s)) }
}

/// Literal atas posisi `t - n .. t` (angka native dari tumpahan `nilai`, sisanya dipindah dari stack VM);
/// hasilnya di posisi `t - n`, dibangun lewat definisi yang sama dengan VM.
pub extern "C" fn bangun(situs: *const SitusBangun, jenis: *const Jenis, t: i64, n: i64, nilai: *const i64) {
    let (vm, basis) = konteks();
    let (t, n) = (t as usize, n as usize);
    let jenis = unsafe { std::slice::from_raw_parts(jenis, t) };
    let nilai = unsafe { std::slice::from_raw_parts(nilai, t) };
    let situs = unsafe { &*situs };
    // Varian: satu payload, tanpa `Vec` antara.
    if let SitusBangun::Varian(nama) = situs {
        let p = nilai_skalar(jenis[t - 1], nilai[t - 1])
            .unwrap_or_else(|| std::mem::replace(sediakan(vm, basis, t - 1), VmValue::Nil));
        timpa(sediakan(vm, basis, t - n), VmValue::varian_berisi(nama, p));
        return;
    }
    if let SitusBangun::StrukturUlang(label, bentuk) = situs {
        if let Some(p) = vm.calon_pakai_ulang(basis, t - n, label, bentuk) {
            let mut lama = std::mem::replace(&mut vm.stack[basis + p], VmValue::Nil);
            let VmValue::Map(m) = &mut lama else { unreachable!("diperiksa pakai_ulang") };
            let Some(crate::vm::peta::IsiPeta::Rapat { nilai: isi, .. }) = crate::vm::value::Peta::isi_tunggal(m)
                else { unreachable!("diperiksa pakai_ulang") };
            isi_ulang(vm, basis, jenis, nilai, t - n..t, isi);
            timpa(sediakan(vm, basis, t - n), lama);
            return;
        }
    }
    // Struct sebentuk yang baru dilepas `buang` (isinya skalar semua): dipakai ulang, tanpa alokasi.
    if let SitusBangun::Struktur(label, bentuk) | SitusBangun::StrukturUlang(label, bentuk) = situs {
        if let Some(mut m) = CADANGAN_STRUCT.with(|c| c.take()) {
            let sama_label = matches!((&m.label, label), (crate::vm::value::Label::Struct(a), crate::vm::value::Label::Struct(b))
                if Arc::ptr_eq(a, b) || a == b);
            if let (true, Some(crate::vm::peta::IsiPeta::Rapat { bentuk: b, nilai: isi })) =
                (sama_label, crate::vm::value::Peta::isi_tunggal(&mut m)) {
                if Arc::ptr_eq(b, bentuk) && isi.len() == n {
                    isi_ulang(vm, basis, jenis, nilai, t - n..t, isi);
                    timpa(sediakan(vm, basis, t - n), VmValue::Map(m));
                    return;
                }
            }
            CADANGAN_STRUCT.with(|c| c.set(Some(m)));
        }
    }
    // Rentang berukuran pasti: `collect` mengalokasi sekali tanpa cek kapasitas per elemen.
    let isi: Vec<VmValue> = (t - n..t).map(|i| match nilai_skalar(jenis[i], nilai[i]) {
        Some(v) => v,
        None => std::mem::replace(sediakan(vm, basis, i), VmValue::Nil),
    }).collect();
    let v = match situs {
        SitusBangun::Struktur(label, bentuk) | SitusBangun::StrukturUlang(label, bentuk) =>
            VmValue::Map(crate::vm::value::Peta::struktur_arc(label.clone(), Arc::clone(bentuk), isi)),
        SitusBangun::Tuple => VmValue::tuple(isi),
        SitusBangun::Larik => VmValue::array(isi),
        SitusBangun::Varian(_) => unreachable!("ditangani di atas"),
    };
    timpa(sediakan(vm, basis, t - n), v);
}

/// `Typeof; Const; Eq` atas posisi `p` milik VM: nama jenisnya (`VmValue::type_name`, sama dengan opcode
/// `Typeof`) sama dengan teks konstanta `nama`. Posisinya dilepas.
pub extern "C" fn jenis_sama(p: i64, nama: *const VmValue) -> i32 {
    let (vm, basis) = konteks();
    let v = std::mem::replace(sediakan(vm, basis, p as usize), VmValue::Nil);
    matches!(unsafe { &*nama }, VmValue::Str(n) if v.type_name() == n.as_str()) as i32
}

/// `Len` atas posisi `p` milik VM (`Vm::vm_len`): panjangnya di `*keluar`, posisinya dilepas; 1 = galat,
/// belum ada yang berubah (VM yang melaporkannya).
pub extern "C" fn panjang(p: i64, keluar: *mut i64) -> i32 {
    let (vm, basis) = konteks();
    let i = basis + p as usize;
    let Some(v) = vm.stack.get(i) else { return 1 };
    match vm.vm_len(v, None) {
        Ok(n) => { unsafe { *keluar = n as i64 }; vm.stack[i] = VmValue::Nil; 0 }
        Err(_) => 1,
    }
}

/// `len(x)` atas lokal `slot` (sel tangkapan: isinya) lewat `Vm::vm_len`, tanpa menyalinnya ke stack VM:
/// panjangnya di `*keluar`; 1 = galat, VM yang melaporkannya.
pub extern "C" fn panjang_lokal(slot: i64, keluar: *mut i64) -> i32 {
    let (vm, basis) = konteks();
    let n = match vm.stack.get(basis + slot as usize) {
        Some(VmValue::Cell(c)) => vm.vm_len(&c.lock().unwrap_or_else(|p| p.into_inner()), None),
        Some(v) => vm.vm_len(v, None),
        None => return 1,
    };
    match n { Ok(n) => { unsafe { *keluar = n as i64 }; 0 } Err(_) => 1 }
}

/// `str::char_at(teks, i)` atas posisi `p` milik VM: kode karakternya; −1 = jalur bawaan umum (indeks negatif
/// atau di luar batas, bukan teks — yang juga melapor galatnya). Prefiks ASCII lewat indeks byte.
pub extern "C" fn karakter(p: i64, i: i64) -> i64 {
    if i < 0 { return -1; }
    let (vm, basis) = konteks();
    let Some(VmValue::Str(s)) = vm.stack.get(basis + p as usize) else { return -1 };
    let (b, i) = (s.as_bytes(), i as usize);
    if i < b.len() && b[..=i].is_ascii() { return b[i] as i64; }
    s.chars().nth(i).map_or(-1, |c| c as i64)
}

/// `karakter` atas teks di lokal `slot` (salinan maya, `Jenis::Salinan`), dibaca di tempat.
pub extern "C" fn karakter_lokal(slot: i64, i: i64) -> i64 {
    let (vm, basis) = konteks();
    if let (true, Some(VmValue::Str(s))) = (i >= 0, vm.stack.get(basis + slot as usize)) {
        let (b, i) = (s.as_bytes(), i as usize);
        if i < b.len() && crate::vm::value::prefiks_ascii(b, i + 1) { return b[i] as i64; }
    }
    karakter_lambat(slot, i)
}

/// Sisa `karakter_lokal`: sel tangkapan, teks bukan ASCII sebelum `i`, indeks negatif (dari belakang,
/// `normal_indeks`). −1 cuma untuk galat (di luar batas, bukan teks), yang dilaporkan VM.
#[cold]
#[inline(never)]
fn karakter_lambat(slot: i64, i: i64) -> i64 {
    lihat_lokal(slot, |v| match v {
        VmValue::Str(s) => {
            let k = if i >= 0 { Some(i as usize) } else { crate::interpreter::normal_indeks(i as i128, s.chars().count(), false) };
            k.and_then(|k| s.chars().nth(k)).map_or(-1, |c| c as i64)
        }
        _ => -1,
    }).unwrap_or(-1)
}


/// VM menjalankan instruksi `ip` bingkai terluar. Stack VM dipotong ke `t` posisi dan `tumpah` posisi
/// teratas diisi dari `nilai` (posisi `Nilai` sudah di sana). `Some(sesudah)`: tinggi yang diharapkan,
/// juga untuk melengkapi `ReturnNil` bersarang. Hasilnya kedalaman bingkai; galat disimpan (`ambil_galat`).
fn satu(ip: i64, jenis: *const Jenis, t: i64, tumpah: i64, sesudah: Option<usize>, nilai: *const i64) -> Option<usize> {
    let (vm, basis) = konteks();
    let (t, ambil) = (t as usize, tumpah as usize);
    // Tabel jenis milik `Rencana` dan slot nilai bingkai native, masing-masing `t` posisi.
    let (jenis, nilai) = unsafe { (std::slice::from_raw_parts(jenis, t), std::slice::from_raw_parts(nilai, t)) };
    // Yang di atas `t` sudah dibuang kode native: dilepas di sini, sebelum ada yang menghitung pemegangnya.
    vm.stack.truncate(basis + t);
    while vm.stack.len() < basis + t { vm.stack.push(VmValue::Nil); }
    for i in t - ambil..t {
        if let Some(v) = nilai_skalar(jenis[i], nilai[i]) { vm.stack[basis + i] = v; }
    }
    let d = vm.frames.len();
    // Opcode yang badannya fungsi `Vm` sendiri dijalankan langsung, tanpa masuk loop dispatch.
    let fr = vm.frames.last_mut().expect("bingkai terluar");
    let ins = fr.func.chunk.code[ip as usize];
    let sp = if ins.span == 0 { None } else { fr.func.chunk.spans.get(ins.span as usize).copied() };
    fr.ip = ip as usize + 1;
    let pertama = match ins.op {
        OpCode::Call => vm.call_value_at(ins.arg as usize, sp, true),
        OpCode::GetMethod => vm.op_get_method(ins.arg, sp),
        OpCode::PeriksaArgMetode => vm.op_periksa_arg_metode(ins.arg, sp),
        OpCode::BuildStruct => vm.op_build_struct(ins.arg, sp),
        OpCode::BuildArray | OpCode::BuildTuple => { vm.op_build_larik(ins.arg, ins.op == OpCode::BuildTuple); Ok(()) }
        OpCode::SetIndex => vm.op_set_index(sp),
        OpCode::Typeof => vm.op_typeof(sp),
        OpCode::Len => vm.op_len(sp),
        OpCode::CopyVal => vm.op_copy_val(sp),
        OpCode::Dup => { let v = vm.stack[vm.stack.len() - 1].clone(); vm.stack.push(v); Ok(()) }
        OpCode::Swap => { let n = vm.stack.len(); vm.stack.swap(n - 1, n - 2); Ok(()) }
        _ => {
            vm.frames[d - 1].ip = ip as usize;
            vm.run_until_inner::<true, true>(d - 1)
        }
    };
    // `Call` atas fungsi VM mendorong bingkai: badannya dijalankan sampai kembali ke bingkai ini.
    // `ReturnNil` yang kembali tepat ke kedalaman itu tidak mendorong apa pun: jawabannya `unit`
    // (aturan `Vm::call_value_immediate`).
    let hasil = pertama.and_then(|_| {
        if vm.frames.len() <= d { return Ok(()); }
        vm.run_until_inner::<true, false>(d)?;
        if vm.frames.len() == d && sesudah.is_some_and(|n| vm.stack.len() + 1 == basis + n) { vm.stack.push(VmValue::Unit); }
        Ok(())
    });
    if let Err(e) = hasil {
        GALAT.with(|g| *g.borrow_mut() = Some(e));
        return None;
    }
    let ok = vm.frames.len() == d && sesudah.is_none_or(|n| vm.frames[d - 1].ip == ip as usize + 1 && vm.stack.len() == basis + n);
    assert!(ok, "bug di jrgc: langkah JIT {:?} di ip {ip} meninggalkan VM dalam bentuk lain (bingkai {}/{d}, ip {}, stack {}/{:?})",
        vm.frames.last().map(|f| f.func.chunk.code[ip as usize].op), vm.frames.len(), vm.frames.last().map_or(0, |f| f.ip),
        vm.stack.len(), sesudah.map(|n| basis + n));
    Some(d)
}

/// Satu instruksi yang tidak melompat; sesudahnya tingginya harus `sesudah`. 0 = selesai, 2 = galat.
pub extern "C" fn langkah(ip: i64, jenis: *const Jenis, t: i64, tumpah: i64, sesudah: i64, nilai: *const i64) -> i32 {
    if satu(ip, jenis, t, tumpah, Some(sesudah as usize), nilai).is_some() { 0 } else { 2 }
}

/// Satu instruksi yang bercabang (`Cara::Cabang`): `ambil_bool` = `bool` di puncak diambil dan
/// dikembalikan (0/1), selain itu `ip` berikutnya; -1 = galat.
pub extern "C" fn langkah_cabang(ip: i64, jenis: *const Jenis, t: i64, tumpah: i64, nilai: *const i64, ambil_bool: i64) -> i64 {
    let Some(d) = satu(ip, jenis, t, tumpah, None, nilai) else { return -1 };
    let (vm, _) = konteks();
    if ambil_bool == 0 { return vm.frames[d - 1].ip as i64; }
    match vm.stack.pop() {
        Some(VmValue::Bool(b)) => b as i64,
        lain => panic!("bug di jrgc: langkah cabang JIT di ip {ip} menaruh {lain:?}, bukan bool"),
    }
}

/// Panggilan langsung (`Cara::Langsung`): callee dan `argc` argumennya (posisi `t - argc - 1..t`) ke stack
/// VM, bingkai callee didorong dengan aturan `Vm::call_value`, dan basis konteks jadi milik callee.
/// 0 = jalankan kode native callee; 1 = VM yang menjalankannya; 2 = galat (`ambil_galat`).
pub extern "C" fn masuk(
    ip: i64, jenis: *const Jenis, t: i64, argc: i64, nilai: *const i64, fungsi: *const VmValue, titip: *mut [usize; 3],
) -> i32 {
    let (vm, basis) = konteks();
    let (t, argc) = (t as usize, argc as usize);
    let jenis = unsafe { std::slice::from_raw_parts(jenis, t) };
    let nilai = unsafe { std::slice::from_raw_parts(nilai, t) };
    vm.stack.truncate(basis + t);
    while vm.stack.len() < basis + t { vm.stack.push(VmValue::Nil); }
    for i in t - argc..t {
        if let Some(v) = nilai_skalar(jenis[i], nilai[i]) { vm.stack[basis + i] = v; }
    }
    let d = vm.frames.len();
    let fr = vm.frames.last_mut().expect("bingkai terluar");
    fr.ip = ip as usize + 1;
    let slot = basis + t - argc - 1;
    // Callee maya (`Jenis::Fungsi`): dari rencana, posisinya tidak ditulis; selain itu dari stack VM.
    let callee = match unsafe { fungsi.as_ref() } { Some(f) => f.clone(), None => vm.stack[slot].clone() };
    let VmValue::Function(func) = callee else { panic!("bug di jrgc: panggilan langsung JIT bukan atas fungsi") };
    if vm.periksa_kedalaman(&func.name, None).is_err() {
        // Span cuma untuk kalimat galatnya.
        let fr = vm.frames.last().expect("bingkai terluar");
        let sp = fr.func.chunk.code.get(ip as usize).and_then(|i| (i.span != 0).then(|| fr.func.chunk.spans.get(i.span as usize).copied()).flatten());
        let e = vm.periksa_kedalaman(&func.name, sp).expect_err("diperiksa barusan");
        GALAT.with(|g| *g.borrow_mut() = Some(e));
        return 2;
    }
    if let Some(p) = &func.chunk.profil { p.catat_panggilan(); }
    vm.dorong_bingkai(crate::vm::vm::CallFrame { func, ip: 0, slot, defers: Vec::new(), argc: argc as u8, closure: None });
    // `(kedalaman, basis, slot)` dititipkan di bingkai native pemanggil sampai `keluar` (tidak bertumpuk di sana).
    unsafe { *titip = [d, basis, slot] };
    // Rekursi native sedalam itu diteruskan VM (lihat `panggil::jalankan`).
    let ukuran = crate::interpreter::Interpreter::INTERPRETER_STACK_SIZE;
    if crate::interpreter::stack_terpakai().is_some_and(|t| t > ukuran / 2) { return 1; }
    KONTEKS.with(|k| k.set((vm, slot + 1)));
    0
}

/// `masuk` untuk callee `layak::ringan`: basis konteks digeser, bingkai VM-nya belum didorong — argumen native
/// dibaca callee dari penyangganya, jadi baru ditulis ke stack VM kalau bingkainya diwujudkan. `titip` =
/// (kedalaman, basis, slot, fungsi, argc, jenis, nilai, t, ip) untuk `keluar_ringan`.
pub extern "C" fn masuk_ringan(
    ip: i64, jenis: *const Jenis, t: i64, argc: i64, nilai: *const i64, fungsi: *const VmValue, titip: *mut [usize; 9],
) -> i32 {
    let (vm, basis) = konteks();
    let (t, argc) = (t as usize, argc as usize);
    // Posisi argumen native cukup `nil` (isinya ditulis `keluar_ringan` kalau bingkainya diwujudkan), supaya
    // tulisan pertama callee di atasnya lewat jalur `push` `sediakan`, bukan `tumbuh`.
    vm.stack.truncate(basis + t);
    while vm.stack.len() < basis + t { vm.stack.push(VmValue::Nil); }
    let slot = basis + t - argc - 1;
    if let VmValue::Function(func) = unsafe { &*fungsi } {
        if let Some(p) = &func.chunk.profil { p.catat_panggilan(); }
    }
    let d = vm.frames.len();
    unsafe { *titip = [d, basis, slot, fungsi as usize, argc, jenis as usize, nilai as usize, t, ip as usize] };
    KONTEKS.with(|k| k.set((vm, slot + 1)));
    0
}

/// `keluar_ringan` di situs yang memakai hasil berstatus `kode` (bulat/pecahan) dari register: hasil itu tidak
/// ditaruh di slot (posisinya berjenis native di pemanggil); status lain lewat `keluar_ringan`.
pub extern "C" fn keluar_ringan_angka(st: i64, hasil: i64, titip: *const [usize; 9], kode: i64) -> i32 {
    if st != kode { return keluar_ringan(st, hasil, titip); }
    let (vm, _) = konteks();
    let [_, basis, slot, ..] = unsafe { *titip };
    KONTEKS.with(|k| k.set((vm, basis)));
    potong(&mut vm.stack, slot);
    0
}

/// `keluar` untuk `masuk_ringan`. Hasil biasa: tanpa bingkai yang perlu dilepas. Deopt, belum mulai, atau
/// galat: bingkai callee diwujudkan dulu persis seperti `masuk`, lalu jalur `keluar` biasa.
pub extern "C" fn keluar_ringan(st: i64, hasil: i64, titip: *const [usize; 9]) -> i32 {
    use super::turun::{BERHASIL, BERHASIL_BOOL, BERHASIL_NILAI, BERHASIL_PECAHAN, BERHASIL_UNIT};
    let (vm, _) = konteks();
    let [_, basis, slot, fungsi, argc, jenis, nilai, t, ip] = unsafe { *titip };
    let v = match st {
        BERHASIL => VmValue::Int(hasil as i128),
        BERHASIL_BOOL => VmValue::Bool(hasil != 0),
        BERHASIL_PECAHAN => VmValue::Float(f64::from_bits(hasil as u64)),
        BERHASIL_NILAI => std::mem::replace(&mut vm.stack[slot + 1 + hasil as usize], VmValue::Nil),
        BERHASIL_UNIT => VmValue::Unit,
        _ => {
            // Yang `masuk` biasa kerjakan: stack sampai posisi pemanggil (`Return` VM membaca panjangnya), `ip`
            // pemanggil sesudah panggilan, argumen native di stack VM. Posisi callee di atasnya tidak dipotong.
            while vm.stack.len() < basis + t { vm.stack.push(VmValue::Nil); }
            vm.frames.last_mut().expect("bingkai terluar").ip = ip + 1;
            let (jenis, nilai) = unsafe {
                (std::slice::from_raw_parts(jenis as *const Jenis, t), std::slice::from_raw_parts(nilai as *const i64, t))
            };
            for i in t - argc..t {
                if let Some(v) = nilai_skalar(jenis[i], nilai[i]) { timpa(sediakan(vm, basis, i), v); }
            }
            let VmValue::Function(func) = (unsafe { &*(fungsi as *const VmValue) }) else { unreachable!("dijamin turun.rs") };
            vm.dorong_bingkai(crate::vm::vm::CallFrame {
                func: Arc::clone(func), ip: 0, slot, defers: Vec::new(), argc: argc as u8, closure: None,
            });
            return keluar(st, hasil, titip as *const [usize; 3]);
        }
    };
    KONTEKS.with(|k| k.set((vm, basis)));
    // Hasilnya tepat di slot callee, juga kalau stack belum sepanjang itu (`masuk_ringan` tidak memanjangkannya).
    potong(&mut vm.stack, slot);
    while vm.stack.len() < slot { vm.stack.push(VmValue::Nil); }
    // Tanpa `catat_hasil`: `ip` bingkai pemanggil basi di sini (`masuk_ringan` tidak menulisnya), jadi umpannya
    // jatuh ke situs lain; pemanggilnya sudah terkompilasi, dan VM mencatat sendiri kalau spekulasinya gagal.
    vm.stack.push(v);
    0
}

/// Akhir panggilan langsung: status kode callee (`turun::BERHASIL`…; `ULANG` juga untuk "VM yang
/// menjalankan"), basis konteks pemanggil dipulihkan, hasilnya di slot fungsi callee. 0 = selesai; 2 = galat.
pub extern "C" fn keluar(st: i64, hasil: i64, titip: *const [usize; 3]) -> i32 {
    use super::turun::{BERHASIL, BERHASIL_BOOL, BERHASIL_NILAI, BERHASIL_PECAHAN, BERHASIL_UNIT, DEOPT, GAGAL};
    let (vm, _) = konteks();
    let [d, basis, slot] = unsafe { *titip };
    KONTEKS.with(|k| k.set((vm, basis)));
    let v = match st {
        BERHASIL => VmValue::Int(hasil as i128),
        BERHASIL_BOOL => VmValue::Bool(hasil != 0),
        BERHASIL_PECAHAN => VmValue::Float(f64::from_bits(hasil as u64)),
        BERHASIL_NILAI => std::mem::replace(&mut vm.stack[slot + 1 + hasil as usize], VmValue::Nil),
        BERHASIL_UNIT => VmValue::Unit,
        GAGAL => return 2,
        _ => {
            // Deopt di callee: bingkainya disusun ulang lalu VM menyelesaikannya; `ULANG` = belum mulai.
            if st == DEOPT { super::deopt::ambil_spek_baru(); super::deopt::pulihkan(vm, super::deopt::ambil()); }
            if let Err(e) = vm.run_until_inner::<true, false>(d) {
                GALAT.with(|g| *g.borrow_mut() = Some(e));
                return 2;
            }
            // `Return` VM menaruh hasilnya di slot; `ReturnNil` tidak menaruh apa pun (`unit`).
            let v = if vm.stack.len() > slot { vm.stack.pop().unwrap_or(VmValue::Unit) } else { VmValue::Unit };
            vm.stack.truncate(slot);
            vm.stack.push(v);
            // Spekulasi callee gagal: salinan kodenya di pemanggil basi, jadi pemanggil keluar ke VM juga.
            return if st == DEOPT && super::deopt::ambil_spek_baru() { 3 } else { 0 };
        }
    };
    if let Some(fr) = vm.ambil_bingkai() { vm.stack.truncate(fr.slot); }
    vm.catat_hasil(&v);
    vm.stack.push(v);
    0
}

/// `Call` atas fungsi bawaan di posisi `t - argc - 1` (milik VM): argumennya dari nilai native atau
/// posisi VM (diambil), hasilnya di posisi fungsinya (`Vm::panggil_native`). 0 = selesai, 2 = galat.
pub extern "C" fn bawaan(ip: i64, jenis: *const Jenis, t: i64, argc: i64, nilai: *const i64, f: i64) -> i32 {
    let (vm, basis) = konteks();
    let tetap_polos = argc & BAWAAN_POLOS != 0;
    let (t, argc) = (t as usize, (argc & !BAWAAN_POLOS) as usize);
    let jenis = unsafe { std::slice::from_raw_parts(jenis, t) };
    let nilai = unsafe { std::slice::from_raw_parts(nilai, t) };
    // Callee-nya fungsi bawaan maya (`Jenis::Bawaan`): alamatnya dari konstanta, bukan dari stack VM.
    let f: crate::vm::value::VmNativeFn = unsafe { std::mem::transmute(f as usize) };
    if !tetap_polos {
        if let Some(v) = bawaan_cepat(vm, basis, jenis, nilai, t, argc, f as usize) {
            vm.catat_hasil(&v);
            timpa(sediakan(vm, basis, t - argc - 1), v);
            return 0;
        }
    }
    let span = |vm: &mut Vm| {
        let fr = vm.frames.last_mut().expect("bingkai terluar");
        fr.ip = ip as usize + 1;
        fr.func.chunk.code.get(ip as usize).and_then(|i| (i.span != 0).then(|| fr.func.chunk.spans.get(i.span as usize).copied()).flatten())
    };
    // Bawaan murni (tanpa layanan khusus, bukan cetak) dengan jumlah argumen yang sah: dipanggil langsung atas
    // argumennya di stack VM, tanpa `Vec` per panggilan. Selebihnya — termasuk aritas salah, yang kalimatnya
    // lahir di `panggil_native` — lewat pintu umum.
    if tetap_polos || polos_untuk(f, argc) {
        for i in t - argc..t {
            if let Some(v) = nilai_skalar(jenis[i], nilai[i]) { timpa(sediakan(vm, basis, i), v); }
        }
        let _ = sediakan(vm, basis, t - 1);
        let r = f(&vm.stack[basis + t - argc..basis + t]);
        let r = r.map_err(|e| crate::vm::value::perjelas_pesan_native(e, &vm.stack[basis + t - argc..basis + t]));
        // Argumennya dilepas di sini, seperti yang dulu dipindah ke larik argumen.
        for i in t - argc..t { vm.stack[basis + i] = VmValue::Nil; }
        return match r {
            Ok(v) => {
                let _ = span(vm);
                vm.catat_hasil(&v);
                timpa(sediakan(vm, basis, t - argc - 1), v);
                0
            }
            Err(e) => {
                let sp = span(vm);
                GALAT.with(|g| *g.borrow_mut() = Some(crate::vm::VmError::at(e, sp)));
                2
            }
        };
    }
    let mut args = Vec::with_capacity(argc);
    for i in t - argc..t {
        args.push(match nilai_skalar(jenis[i], nilai[i]) {
            Some(v) => v,
            None => std::mem::replace(sediakan(vm, basis, i), VmValue::Nil),
        });
    }
    let sp = span(vm);
    match vm.panggil_native(f, args, sp, true) {
        Ok(v) => {
            vm.catat_hasil(&v);
            timpa(sediakan(vm, basis, t - argc - 1), v);
            0
        }
        Err(e) => {
            GALAT.with(|g| *g.borrow_mut() = Some(e));
            2
        }
    }
}

/// `str(n)` atas bulat native: posisi `p` (callee bawaan `str`) = teks desimalnya, sama dengan `bawaan_cepat`.
pub extern "C" fn teks_bulat(p: i64, n: i64) {
    let (vm, basis) = konteks();
    timpa(sediakan(vm, basis, p as usize), VmValue::str(desimal(n)));
}

/// Bit di `argc` `bawaan`: situsnya sudah diputuskan polos saat kompilasi (`bawaan_polos`).
pub const BAWAAN_POLOS: i64 = 1 << 32;

/// Bawaan murni (tanpa layanan khusus, bukan cetak) dengan jumlah argumen yang sah.
fn polos_untuk(f: crate::vm::value::VmNativeFn, argc: usize) -> bool {
    crate::vm::info_native::info(f).is_none_or(|i| i.khusus.is_none() && !i.cetak
        && i.maks.is_none_or(|m| argc <= m) && i.min.is_none_or(|m| argc >= m))
}

/// Situs `bawaan` atas `f` (alamat) dengan `argc` argumen yang selalu lewat jalur polos: bukan kandidat
/// `bawaan_cepat`, dan polos menurut `info_native` (tabelnya tetap selama proses).
pub fn bawaan_polos(f: i64, argc: usize) -> bool {
    if f == 0 { return false; }
    let g: crate::vm::value::VmNativeFn = unsafe { std::mem::transmute(f as usize) };
    let cepat = ["str::char_at", "int", "uint", "str", "len"];
    let nama = crate::vm::info_native::info(g).map(|i| i.kanonik.as_str());
    nama.is_some_and(|n| !cepat.contains(&n)) && polos_untuk(g, argc)
}

/// Desimal sebuah bulat 64 bit, sama dengan `{}` atas `i128` (yang dipakai tampilan `VmValue::Int`), tanpa mesin
/// `fmt`: digit ditulis dari belakang ke penyangga di stack.
fn desimal(n: i64) -> String {
    let mut b = [0u8; 20];
    let mut i = b.len();
    let mut x = n.unsigned_abs();
    loop {
        i -= 1;
        b[i] = b'0' + (x % 10) as u8;
        x /= 10;
        if x == 0 { break; }
    }
    let mut s = String::with_capacity(b.len() - i + 1);
    if n < 0 { s.push('-'); }
    s.push_str(std::str::from_utf8(&b[i..]).expect("digit ASCII"));
    s
}

/// Bawaan yang dijawab di tempat, tanpa larik argumen dan jembatan nilai; `None` = jalur umum (yang juga
/// melapor galatnya). Posisi milik VM saja yang sah dibaca dari stack VM (posisi native bisa basi).
fn bawaan_cepat(vm: &Vm, basis: usize, jenis: &[Jenis], nilai: &[i64], t: usize, argc: usize, f: usize) -> Option<VmValue> {
    fn alamat(nama: &str) -> usize {
        match crate::vm::bawaan::bawaan(nama) { Some(VmValue::NativeFn(f)) => f as usize, _ => 0 }
    }
    static ALAMAT: std::sync::OnceLock<[usize; 5]> = std::sync::OnceLock::new();
    let [char_at, int, uint, teks, len] = *ALAMAT.get_or_init(|| {
        [alamat("str::char_at"), alamat("int"), alamat("uint"), alamat("str"), alamat("len")]
    });
    match argc {
        // `str(n)` atas bulat native: tampilan `VmValue::Int` yang sama dengan bawaan `str`.
        1 if f == teks && jenis[t - 1] == Jenis::Bulat => Some(VmValue::str(desimal(nilai[t - 1]))),
        // `len(teks)` milik VM: `Vm::vm_len`, definisi yang sama dengan opcode `Len`.
        1 if f == len && jenis[t - 1].di_vm() => match vm.stack.get(basis + t - 1) {
            Some(v @ VmValue::Str(_)) => vm.vm_len(v, None).ok().map(|n| VmValue::Int(n as i128)),
            _ => None,
        },
        // `str::char_at(s, i)`, `i` tak negatif yang ada: prefiks ASCII (per kata mesin) lewat indeks byte.
        2 if f == char_at && jenis[t - 1] == Jenis::Bulat && nilai[t - 1] >= 0 && jenis[t - 2].di_vm() => {
            let Some(VmValue::Str(s)) = vm.stack.get(basis + t - 2) else { return None };
            let (b, i) = (s.as_bytes(), nilai[t - 1] as usize);
            if i < b.len() && b[..=i].is_ascii() { return Some(VmValue::Rune(b[i] as char)); }
            s.chars().nth(i).map(VmValue::Rune)
        }
        // `rune as int` / `as uint`: kode karakternya (selalu muat).
        1 if (f == int || f == uint) && jenis[t - 1].di_vm() => match vm.stack.get(basis + t - 1) {
            Some(VmValue::Rune(c)) => Some(VmValue::Int(*c as i128)),
            _ => None,
        },
        _ => None,
    }
}

/// `+` (`biner`) atas dua posisi milik VM: dua teks disambung langsung lewat `VmValue::tambah_milik`, aturan
/// `Vm::op_biner` untuk teks + teks (tanpa operator pemakai dan hook tampilan); selebihnya `biner`.
pub extern "C" fn tambah_nilai(ip: i64, jenis: *const Jenis, t: i64, nilai: *const i64) -> i32 {
    let (vm, basis) = konteks();
    let p = t as usize;
    let js = unsafe { std::slice::from_raw_parts(jenis, p) };
    let i = basis + p - 2;
    let teks = js[p - 2].di_vm() && js[p - 1].di_vm()
        && matches!((vm.stack.get(i), vm.stack.get(i + 1)), (Some(VmValue::Str(_)), Some(VmValue::Str(_))));
    if !teks { return biner(ip, jenis, t, nilai); }
    let kanan = std::mem::replace(&mut vm.stack[i + 1], VmValue::Nil);
    let kiri = std::mem::replace(&mut vm.stack[i], VmValue::Nil);
    match kiri.tambah_milik(&kanan) {
        Ok(v) => { timpa(&mut vm.stack[i], v); 0 }
        Err(e) => {
            let fr = vm.frames.last_mut().expect("bingkai terluar");
            let ins = fr.func.chunk.code[ip as usize];
            let sp = if ins.span == 0 { None } else { fr.func.chunk.spans.get(ins.span as usize).copied() };
            fr.ip = ip as usize + 1;
            GALAT.with(|g| *g.borrow_mut() = Some(VmError::at(e, sp)));
            2
        }
    }
}

/// Aritmetika/banding instruksi `ip` atas posisi `t - 2`, `t - 1` (angka native atau milik VM, diambil),
/// lewat `Vm::op_biner`. Aritmetika: hasilnya di posisi `t - 2`, 0 = selesai. Banding: 0/1. 2 = galat.
pub extern "C" fn biner(ip: i64, jenis: *const Jenis, t: i64, nilai: *const i64) -> i32 {
    let (vm, basis) = konteks();
    let t = t as usize;
    let jenis = unsafe { std::slice::from_raw_parts(jenis, t) };
    let nilai = unsafe { std::slice::from_raw_parts(nilai, t) };
    let mut ambil = |i: usize| nilai_skalar(jenis[i], nilai[i])
        .unwrap_or_else(|| std::mem::replace(sediakan(vm, basis, i), VmValue::Nil));
    let (a, b) = (ambil(t - 2), ambil(t - 1));
    let fr = vm.frames.last_mut().expect("bingkai terluar");
    let ins = fr.func.chunk.code[ip as usize];
    let sp = if ins.span == 0 { None } else { fr.func.chunk.spans.get(ins.span as usize).copied() };
    fr.ip = ip as usize + 1;
    match vm.op_biner(ins.op, a, b, sp) {
        Ok(v) if matches!(ins.op, OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge) => v.is_truthy() as i32,
        Ok(v) => { timpa(sediakan(vm, basis, t - 2), v); 0 }
        Err(e) => {
            GALAT.with(|g| *g.borrow_mut() = Some(e));
            2
        }
    }
}

/// `Propagate` (`?`) atas posisi `p`: 0 = `Ok`/`Some`, posisinya jadi payload; 1 = `Err`/`None`, tidak
/// disentuh (fungsinya kembali dengan nilai itu); 2 = bukan `Result`/`Option` (VM yang melapor).
pub extern "C" fn sebar(p: i64) -> i32 {
    let (vm, basis) = konteks();
    let v = sediakan(vm, basis, p as usize);
    match Vm::tagged_parts(v) {
        Some((ty, var, isi)) if ty == "Result" || ty == "Option" => match var.as_str() {
            "Ok" | "Some" => { *v = isi.unwrap_or(VmValue::Nil); 0 }
            "Err" | "None" => 1,
            _ => 2,
        },
        _ => 2,
    }
}

/// `LoadLocal` atas lokal milik VM: posisi `ke` = isi lokal `dari` (sel dibaca isinya).
pub extern "C" fn muat(dari: i64, ke: i64) {
    let (vm, basis) = konteks();
    // Di loop posisinya biasanya masih memegang penyimpanan yang sama dari putaran sebelumnya.
    if let (Some(a), Some(b)) = (vm.stack.get(basis + dari as usize), vm.stack.get(basis + ke as usize)) {
        if sama_penyimpanan(a, b) { return; }
    }
    let v = match vm.stack.get(basis + dari as usize) {
        Some(VmValue::Cell(c)) => c.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        Some(v) => v.clone(),
        None => VmValue::Nil,
    };
    timpa(sediakan(vm, basis, ke as usize), v);
}

/// Dua nilai berbagi penyimpanan yang sama (isinya identik); sel tangkapan tidak termasuk.
fn sama_penyimpanan(a: &VmValue, b: &VmValue) -> bool {
    match (a, b) {
        (VmValue::Str(x), VmValue::Str(y)) => Arc::ptr_eq(x, y),
        (VmValue::Array(x), VmValue::Array(y)) | (VmValue::Tuple(x), VmValue::Tuple(y)) => Arc::ptr_eq(x, y),
        (VmValue::Map(x), VmValue::Map(y)) => Arc::ptr_eq(x, y),
        (VmValue::Function(x), VmValue::Function(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// `Const`/`Unit`: posisi `ke` = salinan konstanta di `k` (milik chunk yang dikompilasi); null = `()`.
pub extern "C" fn taruh(ke: i64, k: *const VmValue) {
    let (vm, basis) = konteks();
    // Callee bawaan di loop: posisinya biasanya sudah memegang fungsi yang sama dari putaran sebelumnya.
    if let (Some(VmValue::NativeFn(k)), Some(VmValue::NativeFn(ada))) = (unsafe { k.as_ref() }, vm.stack.get(basis + ke as usize)) {
        if *k as usize == *ada as usize { return; }
    }
    let v = if k.is_null() { VmValue::Unit } else { unsafe { (*k).clone() } };
    timpa(sediakan(vm, basis, ke as usize), v);
}

/// `AmbilLokal` atas lokal milik VM (`Vm::ambil_lokal`): posisi `ke` = isinya.
pub extern "C" fn ambil(dari: i64, ke: i64) {
    let (vm, basis) = konteks();
    // `Vm::ambil_lokal` di tempat (tanpa `Option` dan `nil` cadangan): sel cuma dibaca isinya.
    let v = match vm.stack.get_mut(basis + dari as usize) {
        Some(VmValue::Cell(c)) => c.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        Some(tempat) => std::mem::replace(tempat, VmValue::Nil),
        None => VmValue::Nil,
    };
    timpa(sediakan(vm, basis, ke as usize), v);
}

/// `StoreLocal` (`pindah` 0) / `SimpanBuang` (1): lokal `ke` = nilai posisi `dari`, berjenis `jenis`
/// (kode `panggil::kode_jenis`; 4 = milik VM, dibaca dari stack-nya).
pub extern "C" fn simpan(ke: i64, dari: i64, nilai: i64, jenis: i32, pindah: i32) {
    let (vm, basis) = konteks();
    let v = match skalar_kode(jenis, nilai) {
        Some(v) => v,
        None => {
            let t = sediakan(vm, basis, dari as usize);
            if pindah != 0 { std::mem::replace(t, VmValue::Nil) } else { t.clone() }
        }
    };
    // `Vm::simpan_lokal` di tempat: binding yang sudah jadi sel ditulis menembus selnya, selain itu `timpa`.
    match vm.stack.get_mut(basis + ke as usize) {
        Some(VmValue::Cell(c)) => *c.lock().unwrap_or_else(|p| p.into_inner()) = v,
        Some(tempat) => timpa(tempat, v),
        None => {}
    }
}

/// `CekEnum` atas posisi `p` milik VM dengan pola `desc` (konstanta chunk): 0/1 = hasilnya, posisinya
/// dilepas; 2 = jumlah sub-polanya salah, belum ada yang berubah (VM yang melaporkannya).
pub extern "C" fn pola(p: i64, situs: *const SitusPola) -> i32 {
    let (vm, basis) = konteks();
    let v = sediakan(vm, basis, p as usize);
    let situs = unsafe { &*situs };
    if situs.tolak(v) { timpa(v, VmValue::Nil); return 0; }
    match situs.cocok(v) {
        Ok(b) => { *v = VmValue::Nil; b as i32 }
        Err(_) => 2,
    }
}

/// Satu situs pola enum (`CekEnum`): deskriptornya dan tembolok hasil per varian. Kunci = alamat nama varian
/// nilainya (diintern: satu alamat per tipe+varian, hidup selamanya) | bit payload, isi = hasilnya; `Err`
/// (sub-pola salah jumlah) tidak pernah ditembolok.
pub struct SitusPola {
    desc: *const VmValue,
    /// Pola berkualifikasi (`E::V`): alamat nama internnya. Nilai enum dengan alamat lain pasti tidak cocok —
    /// galat jumlah sub-pola cuma mungkin kalau tipe DAN varian sama (`cocok_enum`). 0 = tanpa kualifikasi.
    nama: usize,
    // Delapan: situs `CekEnum` melihat semua varian enum-nya, bukan cuma yang dicocokkannya.
    tembolok: [std::sync::atomic::AtomicUsize; 8],
}

impl SitusPola {
    /// Hidup selama kode yang memakainya (kode JIT tidak pernah dibebaskan).
    pub fn bocor(desc: &VmValue) -> &'static SitusPola {
        let nama = match desc {
            VmValue::Tuple(d) => match &d.lock().unwrap_or_else(|p| p.into_inner())[..] {
                [VmValue::Str(t), VmValue::Str(v), ..] => crate::vm::value::NamaVarian::intern(t, v) as *const _ as usize,
                _ => 0,
            },
            _ => 0,
        };
        Box::leak(Box::new(SitusPola { desc, nama, tembolok: Default::default() }))
    }

    /// Pola berkualifikasi atas enum yang alamat namanya lain: pasti tidak cocok (`cocok` memeriksa hal yang
    /// sama di awalnya). Di-inline di pembantunya supaya penolakan tidak membayar panggilan `cocok`.
    #[inline(always)]
    fn tolak(&self, v: &VmValue) -> bool {
        self.nama != 0 && matches!(v, VmValue::Map(m) if matches!(&m.label,
            crate::vm::value::Label::Enum { nama, .. } if *nama as *const _ as usize != self.nama))
    }

    #[inline(always)]
    fn cocok(&self, v: &VmValue) -> Result<bool, String> {
        use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
        let kunci = match v {
            VmValue::Map(m) => match &m.label {
                crate::vm::value::Label::Enum { nama, payload } =>
                    Some(*nama as *const _ as usize | (payload.is_some() as usize) << 1),
                _ => None,
            },
            _ => None,
        };
        if let Some(k) = kunci {
            if self.nama != 0 && k & !2 != self.nama { return Ok(false); }
            for e in &self.tembolok {
                let isi = e.load(Acquire);
                if isi & !1 == k { return Ok(isi & 1 == 1); }
            }
        }
        let hasil = crate::vm::cocok_enum(v, unsafe { &*self.desc })?;
        if let Some(k) = kunci {
            if let Some(e) = self.tembolok.iter().find(|e| e.load(Relaxed) == 0) {
                let _ = e.compare_exchange(0, k | hasil as usize, Release, Relaxed);
            }
        }
        Ok(hasil)
    }
}

/// `AmbilPayload`: posisi `p` diganti payload enumnya; 1 = tidak berpayload (VM yang melaporkannya).
pub extern "C" fn payload(p: i64) -> i32 {
    let (vm, basis) = konteks();
    let v = sediakan(vm, basis, p as usize);
    let Some((_, _, Some(isi))) = v.bagian_enum() else { return 1 };
    *v = isi.clone();
    0
}

/// Isi lokal `slot` bingkai terluar dibaca di tempat (sel lewat kuncinya); `None` = di luar stack.
fn lihat_lokal<T>(slot: i64, f: impl FnOnce(&VmValue) -> T) -> Option<T> {
    let (vm, basis) = konteks();
    Some(match vm.stack.get(basis + slot as usize)? {
        VmValue::Cell(c) => f(&c.lock().unwrap_or_else(|p| p.into_inner())),
        v => f(v),
    })
}

/// Pola-pola satu rantai lengan `match` (`turun::rantai_pola`), dicoba berurutan oleh `pola_pilih`.
pub struct RantaiPola { situs: Box<[&'static SitusPola]> }

impl RantaiPola {
    /// Hidup selama kode yang memakainya (kode JIT tidak pernah dibebaskan).
    pub fn bocor(situs: Vec<&'static SitusPola>) -> &'static RantaiPola {
        Box::leak(Box::new(RantaiPola { situs: situs.into_boxed_slice() }))
    }
}

/// Rantai lengan atas lokal `slot`: indeks lengan pertama yang cocok, banyaknya lengan kalau tidak ada,
/// `1000 + k` kalau pola lengan ke-`k` galat (lengan sebelumnya tidak cocok). Urutannya sama dengan
/// `pola_lokal` per lengan; lokal yang tak terbaca = galat di lengan pertama (VM yang menjalankan).
pub extern "C" fn pola_pilih(slot: i64, rantai: *const RantaiPola) -> i32 {
    let rantai = unsafe { &*rantai };
    let (vm, basis) = konteks();
    let Some(v) = vm.stack.get(basis + slot as usize) else { return 1000 };
    let pilih = |v: &VmValue| {
        for (k, situs) in rantai.situs.iter().enumerate() {
            if situs.tolak(v) { continue; }
            match situs.cocok(v) {
                Ok(true) => return k as i32,
                Ok(false) => {}
                Err(_) => return 1000 + k as i32,
            }
        }
        rantai.situs.len() as i32
    };
    match v {
        VmValue::Cell(c) => pilih(&c.lock().unwrap_or_else(|p| p.into_inner())),
        v => pilih(v),
    }
}

/// `LoadLocal` + `CekEnum` tanpa menyalin lokalnya: 0/1 = hasil; 2 = VM yang menjalankan keduanya.
pub extern "C" fn pola_lokal(slot: i64, situs: *const SitusPola) -> i32 {
    let situs = unsafe { &*situs };
    let (vm, basis) = konteks();
    let Some(v) = vm.stack.get(basis + slot as usize) else { return 2 };
    if situs.tolak(v) { return 0; }
    match v {
        VmValue::Cell(c) => situs.cocok(&c.lock().unwrap_or_else(|p| p.into_inner())),
        v => situs.cocok(v),
    }.map_or(2, |b| b as i32)
}

/// `LoadLocal` + `AmbilPayload` + bukaan angka (`jenis` 1 bulat 64 bit, 5 pecahan): 0 = angkanya di `*keluar`,
/// stack VM tidak disentuh; 1 = bukan enum berpayload (VM yang menjalankan); 2 = payload jenis lain, ditaruh di
/// posisi `ke` seperti `payload_lokal` (spekulasi gagal sesudahnya, seperti `buka`).
pub extern "C" fn payload_angka_lokal(slot: i64, ke: i64, jenis: i32, keluar: *mut i64) -> i32 {
    let hasil = lihat_lokal(slot, |v| match v.bagian_enum() {
        Some((_, _, Some(isi))) => Some(match (isi, jenis) {
            (VmValue::Int(n), 1) => i64::try_from(*n).map_err(|_| isi.clone()),
            (VmValue::Float(x), 5) => Ok(x.to_bits() as i64),
            _ => Err(isi.clone()),
        }),
        _ => None,
    }).flatten();
    match hasil {
        Some(Ok(n)) => { unsafe { *keluar = n }; 0 }
        Some(Err(isi)) => {
            let (vm, basis) = konteks();
            timpa(sediakan(vm, basis, ke as usize), isi);
            2
        }
        None => 1,
    }
}

/// `AmbilPayload; Typeof; Const; Eq` atas lokal `slot`: 1 kalau nama jenis payload-nya (`VmValue::type_name`)
/// sama dengan teks konstanta `nama`, 0 kalau tidak; 2 = bukan enum berpayload (VM yang menjalankannya).
pub extern "C" fn payload_jenis_sama(slot: i64, nama: *const VmValue) -> i32 {
    let VmValue::Str(n) = (unsafe { &*nama }) else { return 2 };
    lihat_lokal(slot, |v| match v.bagian_enum() {
        Some((_, _, Some(isi))) => (isi.type_name() == n.as_str()) as i32,
        _ => 2,
    }).unwrap_or(2)
}

/// `AmbilPayload; Len` atas lokal `slot` (`Vm::vm_len` atas payload-nya): panjangnya di `*keluar`; 1 = bukan
/// enum berpayload atau tak berpanjang (VM yang menjalankannya dan melapor).
pub extern "C" fn payload_panjang(slot: i64, keluar: *mut i64) -> i32 {
    let (vm, basis) = konteks();
    let vm: &Vm = vm;
    let ukur = |v: &VmValue| match v.bagian_enum() {
        Some((_, _, Some(isi))) => vm.vm_len(isi, None).ok(),
        _ => None,
    };
    let n = match vm.stack.get(basis + slot as usize) {
        Some(VmValue::Cell(c)) => ukur(&c.lock().unwrap_or_else(|p| p.into_inner())),
        Some(v) => ukur(v),
        None => None,
    };
    match n { Some(n) => { unsafe { *keluar = n as i64 }; 0 } None => 1 }
}

/// `LoadLocal` + `AmbilPayload`: posisi `ke` = payload lokal `slot`; 1 = VM yang menjalankan keduanya.
pub extern "C" fn payload_lokal(slot: i64, ke: i64) -> i32 {
    let Some(Some(isi)) = lihat_lokal(slot, |v| match v.bagian_enum() {
        Some((_, _, Some(isi))) => Some(isi.clone()),
        _ => None,
    }) else { return 1 };
    let (vm, basis) = konteks();
    timpa(sediakan(vm, basis, ke as usize), isi);
    0
}

/// Satu situs `x.f` yang digabung: nama, privat, jenis spekulasi, dan tembolok bentuk (monomorfik — diisi
/// sekali, memegang referensi kuat ke bentuknya, jadi alamatnya tidak pernah dipakai bentuk lain).
pub struct SitusField {
    pub nama: *const String,
    pub privat: bool,
    pub jenis: i32,
    bentuk: std::sync::atomic::AtomicPtr<crate::vm::peta::Bentuk>,
    indeks: std::sync::atomic::AtomicUsize,
}

impl SitusField {
    /// Hidup selama kode yang memakainya (kode JIT tidak pernah dibebaskan).
    pub fn bocor(nama: &String, privat: bool, jenis: i32) -> &'static SitusField {
        Box::leak(Box::new(SitusField {
            nama, privat, jenis,
            bentuk: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
            indeks: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    #[inline]
    fn baca<'a>(&self, isi: &'a crate::vm::peta::IsiPeta, nama: &str) -> Option<&'a VmValue> {
        use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
        let crate::vm::peta::IsiPeta::Rapat { bentuk, nilai } = isi else { return isi.get(nama) };
        let p = Arc::as_ptr(bentuk) as *mut crate::vm::peta::Bentuk;
        if self.bentuk.load(Acquire) == p { return nilai.get(self.indeks.load(Relaxed)); }
        let i = bentuk.posisi(nama)?;
        if self.bentuk.load(Relaxed).is_null() {
            self.indeks.store(i, Relaxed);
            let kuat = Arc::into_raw(bentuk.clone()) as *mut crate::vm::peta::Bentuk;
            if self.bentuk.compare_exchange(std::ptr::null_mut(), kuat, Release, Relaxed).is_err() {
                unsafe { drop(Arc::from_raw(kuat)) }
            }
        }
        nilai.get(i)
    }
}

/// `LoadLocal` + `GetField` tanpa menyalin lokalnya. 4 = angka berjenis `situs.jenis` (1 bulat, 5 pecahan)
/// di `keluar`; 0 = nilai di posisi `ke`; 1 = gagal sesudah lokalnya ditaruh di `ke` (VM yang melapor).
pub extern "C" fn field_lokal(slot: i64, situs: *const SitusField, keluar: *mut i64, ke: i64) -> i32 {
    let (vm, basis) = konteks();
    let situs = unsafe { &*situs };
    let (nama, jenis) = (unsafe { &*situs.nama }, situs.jenis);
    // Angka yang cocok dengan spekulasi dibaca di tempat; yang lain disalin.
    let angka = |v: &VmValue| match (v, jenis) {
        (VmValue::Int(n), 1) => i64::try_from(*n).ok(),
        (VmValue::Float(x), 5) => Some(x.to_bits() as i64),
        _ => None,
    };
    // Jalur panas: struct milik lokal (bukan sel), bukan privat, angka yang cocok spekulasi — tanpa `VmValue`.
    if !situs.privat {
        if let Some(VmValue::Map(m)) = vm.stack.get_mut(basis + slot as usize) {
            let n = match crate::vm::value::Peta::isi_tunggal(m) {
                Some(isi) => situs.baca(isi, nama).and_then(angka),
                None => situs.baca(&m.lock().unwrap_or_else(|p| p.into_inner()), nama).and_then(angka),
            };
            if let Some(n) = n {
                unsafe { *keluar = n };
                return 4;
            }
        }
    }
    let cepat: Option<Result<i64, VmValue>> = {
        let vm: &Vm = vm;
        let lihat = |v: &VmValue| match v {
            VmValue::Map(m) if !situs.privat || vm.periksa_visibilitas(v, nama, None).is_ok() =>
                situs.baca(&m.lock().unwrap_or_else(|p| p.into_inner()), nama).map(|x| angka(x).ok_or_else(|| x.clone())),
            _ => None,
        };
        match vm.stack.get(basis + slot as usize) {
            Some(VmValue::Cell(c)) => lihat(&c.lock().unwrap_or_else(|p| p.into_inner())),
            Some(v) => lihat(v),
            None => None,
        }
    };
    let v = match cepat {
        Some(Ok(n)) => { unsafe { *keluar = n }; return 4; }
        Some(Err(v)) => v,
        None => {
            let obj = vm.baca_lokal(basis + slot as usize).unwrap_or(VmValue::Nil);
            timpa(sediakan(vm, basis, ke as usize), obj);
            if field(ke, nama, situs.privat as i32) != 0 { return 1; }
            let i = basis + ke as usize;
            if let Some(n) = angka(&vm.stack[i]) { unsafe { *keluar = n }; return 4; }
            return 0;
        }
    };
    timpa(sediakan(vm, basis, ke as usize), v);
    0
}

/// Field angka lain milik lokal `slot` yang dibaca sesudahnya di blok yang sama, sekali kunci (`situs[j]`
/// tidak privat). Bit `j` hasilnya = `keluar[j]` berisi angka berjenis `situs[j].jenis`; sisanya dibaca
/// lewat `field_lokal` biasa di tempatnya.
pub extern "C" fn field_lokal_banyak(slot: i64, banyak: *const SitusBanyak, keluar: *mut i64) -> i64 {
    let (vm, basis) = konteks();
    let Some(VmValue::Map(m)) = vm.stack.get_mut(basis + slot as usize) else { return 0 };
    let kunci;
    let g: &crate::vm::peta::IsiPeta = match crate::vm::value::Peta::isi_tunggal(m) {
        Some(isi) => isi,
        None => { kunci = m.lock().unwrap_or_else(|p| p.into_inner()); &kunci }
    };
    let banyak = unsafe { &*banyak };
    if let Some(bit) = banyak.cepat(g, keluar) { return bit; }
    let mut bit = 0i64;
    for (j, s) in banyak.situs.iter().enumerate() {
        let n = s.baca(g, unsafe { &*s.nama }).and_then(|v| match (v, s.jenis) {
            (VmValue::Int(n), 1) => i64::try_from(*n).ok(),
            (VmValue::Float(x), 5) => Some(x.to_bits() as i64),
            _ => None,
        });
        if let Some(n) = n { unsafe { *keluar.add(j) = n }; bit |= 1 << j; }
    }
    bit
}

/// Daftar field angka (bukan privat) satu lokal yang dibaca sekali kunci: satu tembolok bentuk untuk seluruh
/// daftar (monomorfik, referensi kuat ke bentuknya), indeks tiap field diisi sebelum bentuknya dipasang.
pub struct SitusBanyak {
    situs: Box<[&'static SitusField]>,
    bentuk: std::sync::atomic::AtomicPtr<crate::vm::peta::Bentuk>,
    indeks: Box<[std::sync::atomic::AtomicUsize]>,
    /// Semua situsnya berspekulasi pecahan: loop baca tanpa melihat jenis per field.
    pecahan: bool,
}

impl SitusBanyak {
    /// Hidup selama kode yang memakainya (kode JIT tidak pernah dibebaskan).
    pub fn bocor(situs: Vec<&'static SitusField>) -> &'static SitusBanyak {
        let indeks = situs.iter().map(|_| std::sync::atomic::AtomicUsize::new(0)).collect();
        let pecahan = situs.iter().all(|s| s.jenis == 5);
        Box::leak(Box::new(SitusBanyak {
            situs: situs.into_boxed_slice(), bentuk: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()), indeks, pecahan,
        }))
    }

    /// Bentuk yang ditembolok: tiap field dibaca lewat indeksnya, bitnya kalau angkanya sejenis spekulasi.
    /// `None` = bentuk lain (jalur per field).
    #[inline]
    fn cepat(&self, isi: &crate::vm::peta::IsiPeta, keluar: *mut i64) -> Option<i64> {
        use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
        let crate::vm::peta::IsiPeta::Rapat { bentuk, nilai } = isi else { return None };
        let p = Arc::as_ptr(bentuk) as *mut crate::vm::peta::Bentuk;
        if self.bentuk.load(Acquire) != p {
            if !self.bentuk.load(Relaxed).is_null() { return None; }
            for (s, i) in self.situs.iter().zip(self.indeks.iter()) {
                i.store(bentuk.posisi(unsafe { &*s.nama }.as_str())?, Relaxed);
            }
            let kuat = Arc::into_raw(bentuk.clone()) as *mut crate::vm::peta::Bentuk;
            if self.bentuk.compare_exchange(std::ptr::null_mut(), kuat, Release, Relaxed).is_err() {
                unsafe { drop(Arc::from_raw(kuat)) }
                return None;
            }
        }
        let mut bit = 0i64;
        if self.pecahan {
            for (j, i) in self.indeks.iter().enumerate() {
                if let Some(VmValue::Float(x)) = nilai.get(i.load(Relaxed)) {
                    unsafe { *keluar.add(j) = x.to_bits() as i64 };
                    bit |= 1 << j;
                }
            }
            return Some(bit);
        }
        for (j, (s, i)) in self.situs.iter().zip(self.indeks.iter()).enumerate() {
            let n = match (nilai.get(i.load(Relaxed)), s.jenis) {
                (Some(VmValue::Int(n)), 1) => i64::try_from(*n).ok(),
                (Some(VmValue::Float(x)), 5) => Some(x.to_bits() as i64),
                _ => None,
            };
            if let Some(n) = n { unsafe { *keluar.add(j) = n }; bit |= 1 << j; }
        }
        Some(bit)
    }
}

/// `LoadLocal` yang langsung dibuka (`buka`) tanpa menyalin lokalnya ke stack VM.
pub extern "C" fn buka_lokal(slot: i64, jenis: i32, keluar: *mut i64) -> i32 {
    let n = lihat_lokal(slot, |v| match (v, jenis) {
        (VmValue::Int(n), 1) => i64::try_from(*n).ok(),
        (VmValue::Float(x), 5) => Some(x.to_bits() as i64),
        _ => None,
    }).flatten();
    match n {
        Some(n) => { unsafe { *keluar = n }; 0 }
        None => 1,
    }
}

/// `IterAda` atas iterator di posisi `slot` (tiga posisi milik VM): 0/1 = masih ada; 2 = VM yang melapor.
pub extern "C" fn iter_ada(slot: i64) -> i32 {
    let (vm, basis) = konteks();
    vm.iter_ada(basis + slot as usize, None).map_or(2, |b| b as i32)
}

/// `IterMaju`: indeks iterator di posisi `slot` (bulat milik VM) maju satu; 1 = bukan bulat (VM yang melapor).
pub extern "C" fn maju(slot: i64) -> i32 {
    let (vm, basis) = konteks();
    match vm.stack.get_mut(basis + slot as usize) {
        Some(VmValue::Int(n)) if *n < i128::MAX => { *n += 1; 0 }
        _ => 1,
    }
}

/// `SlideDown`: posisi `dari` (milik VM) pindah ke `ke`; yang tertinggal di `dari` jadi `nil`.
pub extern "C" fn pindah(dari: i64, ke: i64) {
    let (vm, basis) = konteks();
    let v = std::mem::replace(sediakan(vm, basis, dari as usize), VmValue::Nil);
    timpa(sediakan(vm, basis, ke as usize), v);
}

/// `LoadGlobal`: posisi `ke` = global `nama` (isi sel `var` modul); 1 = tidak ada (VM yang melapor).
pub extern "C" fn muat_global(nama: *const String, ke: i64) -> i32 {
    let (vm, basis) = konteks();
    let v = match vm.globals.get(unsafe { &*nama }) {
        Some(VmValue::Cell(sel)) => sel.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        Some(v) => v.clone(),
        None => return 1,
    };
    timpa(sediakan(vm, basis, ke as usize), v);
    0
}

/// `LoadUpval`: posisi `ke` = isi sel tangkapan ke-`idx` closure bingkai terluar; 1 = tidak ada (VM yang melapor).
pub extern "C" fn muat_upval(idx: i64, ke: i64) -> i32 {
    let (vm, basis) = konteks();
    let Ok(sel) = vm.upval_at(idx as u32, None) else { return 1 };
    let v = sel.lock().unwrap_or_else(|p| p.into_inner()).clone();
    timpa(sediakan(vm, basis, ke as usize), v);
    0
}

/// `StoreUpval`: sel tangkapan ke-`idx` = nilai posisi `dari` berjenis `jenis` (kode `simpan`); 1 = tidak ada.
pub extern "C" fn simpan_upval(idx: i64, dari: i64, nilai: i64, jenis: i32) -> i32 {
    let (vm, basis) = konteks();
    let Ok(sel) = vm.upval_at(idx as u32, None) else { return 1 };
    let v = match skalar_kode(jenis, nilai) {
        Some(v) => v,
        None => sediakan(vm, basis, dari as usize).clone(),
    };
    *sel.lock().unwrap_or_else(|p| p.into_inner()) = v;
    0
}

/// `StoreGlobal`: sel `var` modul `nama` = nilai posisi `dari` berjenis `jenis` (kode `simpan`);
/// 1 = bukan `var` yang sudah didefinisikan (VM yang melapor), belum ada yang berubah.
pub extern "C" fn simpan_global(nama: *const String, dari: i64, nilai: i64, jenis: i32) -> i32 {
    let (vm, basis) = konteks();
    let v = match skalar_kode(jenis, nilai) {
        Some(v) => v,
        None => sediakan(vm, basis, dari as usize).clone(),
    };
    match vm.globals.get(unsafe { &*nama }) {
        Some(VmValue::Cell(sel)) => { *sel.lock().unwrap_or_else(|p| p.into_inner()) = v; 0 }
        _ => 1,
    }
}

/// `GetIndex`: posisi `p` (wadah milik VM) diganti elemennya; indeksnya bulat/bool/pecahan (`jenis`,
/// kode `simpan`) atau milik VM di `p + 1`. 1 = galat, belum ada yang berubah (VM yang melapor).
pub extern "C" fn indeks(p: i64, nilai: i64, jenis: i32) -> i32 {
    let (vm, basis) = konteks();
    let i = basis + p as usize;
    let idx = match skalar_kode(jenis, nilai) {
        Some(v) => v,
        None => match vm.stack.get(i + 1) { Some(v) => v.clone(), None => return 1 },
    };
    let Some(wadah) = vm.stack.get(i) else { return 1 };
    match vm.get_index(wadah, &idx, None) {
        Ok(v) => { vm.stack[i] = v; 0 }
        Err(_) => 1,
    }
}

/// `GetField`/`GetFieldPrivat` (`privat` 1): posisi `p` (milik VM) diganti field `nama`-nya. 1 = galat
/// atau method `math::` yang ditulis tanpa `()` (VM yang menjalankannya), belum ada yang berubah.
pub extern "C" fn field(p: i64, nama: *const String, privat: i32) -> i32 {
    let (vm, basis) = konteks();
    let (i, nama) = (basis + p as usize, unsafe { &*nama });
    let Some(obj) = vm.stack.get(i) else { return 1 };
    if privat != 0 && vm.periksa_visibilitas(obj, nama, None).is_err() { return 1; }
    match vm.get_field(obj, nama, None) {
        Ok(VmValue::NativeFn(_)) if matches!(obj, VmValue::Float(_) | VmValue::Int(_)) => 1,
        Ok(v) => { vm.stack[i] = v; 0 }
        Err(_) => 1,
    }
}

/// `Dup` atas posisi `dari` milik VM: posisi `ke` = salinannya.
pub extern "C" fn salin(dari: i64, ke: i64) {
    let (vm, basis) = konteks();
    let v = sediakan(vm, basis, dari as usize).clone();
    timpa(sediakan(vm, basis, ke as usize), v);
}

/// `Swap` atas dua posisi milik VM.
pub extern "C" fn tukar(x: i64, y: i64) {
    let (vm, basis) = konteks();
    sediakan(vm, basis, x.max(y) as usize);
    vm.stack.swap(basis + x as usize, basis + y as usize);
}

/// Jaga tembolok method: penerima di posisi `p` berlabel `label`; lolos = posisinya dilepas (dipakai
/// `GetMethod`) dan galat method tertunda di atasnya dibuang, seperti `op_get_method` yang berhasil.
/// 1 = label lain, belum ada yang berubah.
pub extern "C" fn cek_metode(p: i64, label: *const String) -> i32 {
    let (vm, basis) = konteks();
    let v = sediakan(vm, basis, p as usize);
    if v.nama_tipe() != Some(unsafe { (*label).as_str() }) { return 1; }
    *v = VmValue::Nil;
    let slot = (basis + p as usize).saturating_sub(1);
    if !vm.galat_metode.is_empty() { vm.galat_metode.retain(|(i, _)| *i < slot); }
    0
}

/// `Dup; GetMethod; Swap` atas penerima milik VM di posisi `p` dengan method yang sudah diketahui: label
/// dijaga di tempat, penerimanya pindah ke `p + 1` (seperti `cek_metode` lalu `Swap`). 1 = label lain:
/// salinan `Dup` ditaruh di `p + 1` dan deopt di `GetMethod`.
pub extern "C" fn metode_tukar(p: i64, label: *const String) -> i32 {
    let (vm, basis) = konteks();
    let i = basis + p as usize;
    let v = sediakan(vm, basis, p as usize);
    if v.nama_tipe() != Some(unsafe { (*label).as_str() }) {
        let salinan = v.clone();
        timpa(sediakan(vm, basis, p as usize + 1), salinan);
        return 1;
    }
    let v = std::mem::replace(v, VmValue::Nil);
    timpa(sediakan(vm, basis, p as usize + 1), v);
    if !vm.galat_metode.is_empty() { vm.galat_metode.retain(|(j, _)| *j < i); }
    0
}

/// `LoadLocal`/`AmbilLokal` lalu `metode_tukar`: lokal `slot` (dibaca, atau diambil kalau `ambil`) langsung
/// ke `p + 1`. 1 = label lain: lokalnya di `p`, salinan `Dup` di `p + 1`, deopt di `GetMethod`.
pub extern "C" fn metode_lokal(slot: i64, p: i64, label: *const Arc<str>, ambil: i32) -> i32 {
    let (vm, basis) = konteks();
    let i = basis + slot as usize;
    let l: &str = unsafe { &**label };
    // Label struct dan label ini sama-sama diintern: alamatnya dulu, teksnya kalau beda (enum, label lain).
    let cocok = |v: &VmValue| v.nama_tipe().is_some_and(|s| (s.as_ptr() == l.as_ptr() && s.len() == l.len()) || s == l);
    // Jalur panas: lokal biasa (bukan sel) yang labelnya cocok diperiksa di tempat lalu dipindah/disalin sekali,
    // tanpa `Option` dan `nil` cadangan di antaranya.
    if let Some(tempat) = vm.stack.get_mut(i) {
        if !matches!(tempat, VmValue::Cell(_)) && cocok(tempat) {
            let v = if ambil != 0 { std::mem::replace(tempat, VmValue::Nil) } else { tempat.clone() };
            timpa(sediakan(vm, basis, p as usize), VmValue::Nil);
            timpa(sediakan(vm, basis, p as usize + 1), v);
            let j = basis + p as usize;
            if !vm.galat_metode.is_empty() { vm.galat_metode.retain(|(x, _)| *x < j); }
            return 0;
        }
    }
    let v = if ambil != 0 { vm.ambil_lokal(i) } else { vm.baca_lokal(i) }.unwrap_or(VmValue::Nil);
    let sama = cocok(&v);
    if !sama {
        let salinan = v.clone();
        timpa(sediakan(vm, basis, p as usize), v);
        timpa(sediakan(vm, basis, p as usize + 1), salinan);
        return 1;
    }
    timpa(sediakan(vm, basis, p as usize), VmValue::Nil);
    timpa(sediakan(vm, basis, p as usize + 1), v);
    let j = basis + p as usize;
    if !vm.galat_metode.is_empty() { vm.galat_metode.retain(|(x, _)| *x < j); }
    0
}

/// Tepi ke titik temu yang dilebarkan: angka native di posisi `p` jadi nilai VM (kode jenis `simpan`).
pub extern "C" fn tulis_vm(p: i64, nilai: i64, jenis: i32) {
    let (vm, basis) = konteks();
    // Tepi cuma menulis posisi native (`Jenis::kode` selain 4).
    let v = skalar_kode(jenis, nilai).expect("tepi menulis angka native");
    timpa(sediakan(vm, basis, p as usize), v);
}

/// `Jenis::Lebar` di posisi `p` ke stack VM: bulat i128 dari dua belahan 64 bit.
pub extern "C" fn tulis_lebar(p: i64, lo: i64, hi: i64) {
    let (vm, basis) = konteks();
    timpa(sediakan(vm, basis, p as usize), VmValue::Int(((hi as i128) << 64) | (lo as u64 as i128)));
}

/// Posisi `p` (milik VM) sebagai bulat i128: `keluar[0]` belahan bawah, `keluar[1]` atas; 1 = bukan bulat.
pub extern "C" fn buka_lebar(p: i64, keluar: *mut i64) -> i32 {
    let (vm, basis) = konteks();
    match vm.stack.get(basis + p as usize) {
        Some(VmValue::Int(n)) => {
            unsafe { *keluar = *n as i64; *keluar.add(1) = (*n >> 64) as i64; }
            0
        }
        _ => 1,
    }
}

/// Posisi `p` dilepas (`Pop` atas nilai milik VM).
pub extern "C" fn buang(p: i64) {
    let (vm, basis) = konteks();
    let Some(v) = vm.stack.get_mut(basis + p as usize) else { return };
    if skalar(v) { return unsafe { std::ptr::write(v, VmValue::Nil) }; }
    // Struct milik tunggal berisi skalar: disimpan untuk `bangun` berikutnya, bukan dibebaskan.
    if let VmValue::Map(m) = v {
        if matches!(m.label, crate::vm::value::Label::Struct(_)) && matches!(crate::vm::value::Peta::isi_tunggal(m),
            Some(crate::vm::peta::IsiPeta::Rapat { nilai, .. }) if nilai.iter().all(skalar)) {
            let VmValue::Map(m) = std::mem::replace(v, VmValue::Nil) else { unreachable!("baru diperiksa") };
            CADANGAN_STRUCT.with(|c| c.set(Some(m)));
            return;
        }
    }
    *v = VmValue::Nil;
}

/// Truthiness posisi `p` (`VmValue::is_truthy`); `lepas` = posisinya dibuang sesudahnya.
pub extern "C" fn benar(p: i64, lepas: i32) -> i32 {
    let (vm, basis) = konteks();
    let Some(v) = vm.stack.get_mut(basis + p as usize) else { return 0 };
    let b = v.is_truthy();
    if lepas != 0 { *v = VmValue::Nil; }
    b as i32
}

/// Posisi `p` sebagai bulat 64 bit (`jenis` 1) atau pecahan (5) di `*keluar`; 1 = jenisnya lain.
pub extern "C" fn buka(p: i64, jenis: i32, keluar: *mut i64) -> i32 {
    let (vm, basis) = konteks();
    let v = match (vm.stack.get(basis + p as usize), jenis) {
        (Some(VmValue::Int(n)), 1) => i64::try_from(*n).ok(),
        (Some(VmValue::Float(x)), 5) => Some(x.to_bits() as i64),
        (Some(VmValue::Rune(c)), 7) => Some(*c as i64),
        _ => None,
    };
    match v {
        Some(n) => { unsafe { *keluar = n }; 0 }
        None => 1,
    }
}

/// Pasang konteks selama `f` (kode native) berjalan; konteks sebelumnya dipulihkan.
pub(crate) fn dengan_konteks<T>(vm: *mut Vm, basis: usize, f: impl FnOnce() -> T) -> T {
    let lama = KONTEKS.with(|k| k.replace((vm, basis)));
    let h = f();
    KONTEKS.with(|k| k.set(lama));
    h
}

/// Slot lokal `rel` bingkai terluar sebagai larik (bukan sel tangkapan): `f` atas isinya.
fn atas_larik<T>(rel: i64, tuple: bool, f: impl FnOnce(&mut VmValue) -> Option<T>) -> Option<T> {
    let (vm, basis) = KONTEKS.with(|k| k.get());
    if vm.is_null() { return None; }
    // Kode native berjalan di dalam `panggil::jalankan`, yang tidak menyentuh VM sampai kode itu kembali.
    let vm = unsafe { &mut *vm };
    let tempat = vm.stack.get_mut(basis.checked_add(usize::try_from(rel).ok()?)?)?;
    if !(matches!(tempat, VmValue::Array(_)) || tuple && matches!(tempat, VmValue::Tuple(_))) { return None; }
    f(tempat)
}

/// Indeks dengan aturan `norm_idx` (negatif dari belakang); di luar batas = `None` (VM yang menolak).
#[inline(always)]
fn norm_indeks(i: i64, n: usize) -> Option<usize> {
    // Panjang larik selalu di bawah `i64::MAX`, jadi `i + n` untuk `i` negatif tidak meluap.
    let j = if i < 0 { i + n as i64 } else { i };
    ((j as u64) < n as u64).then_some(j as usize)
}

/// Alamat VM dan basis bingkai ini untuk jalur sebaris (`jit::tata_letak`): dibaca sekali di pintu masuk fungsi,
/// tetap selama panggilannya (`masuk`/`keluar` memulihkannya sebelum kembali ke sini).
pub extern "C" fn konteks_mentah(keluar: *mut usize) {
    let (vm, basis) = KONTEKS.with(|k| k.get());
    unsafe { *keluar = vm as usize; *keluar.add(1) = basis; }
}

/// `IndeksLokal` atas larik berelemen bulat 64 bit: 0 = `*keluar` terisi.
pub extern "C" fn baca(rel: i64, i: i64, keluar: *mut i64) -> i32 {
    let bulat = |g: &[VmValue]| match g.get(norm_indeks(i, g.len())?)? {
        VmValue::Int(n) => i64::try_from(*n).ok(),
        _ => None,
    };
    baca_dengan(rel, keluar, bulat)
}

/// `IndeksLokal` atas larik berelemen pecahan (umpan VM): bit `f64`-nya di `*keluar`.
pub extern "C" fn baca_pecahan(rel: i64, i: i64, keluar: *mut i64) -> i32 {
    let pecahan = |g: &[VmValue]| match g.get(norm_indeks(i, g.len())?)? {
        VmValue::Float(x) => Some(x.to_bits() as i64),
        _ => None,
    };
    baca_dengan(rel, keluar, pecahan)
}

#[inline(always)]
fn baca_dengan(rel: i64, keluar: *mut i64, bulat: impl Fn(&[VmValue]) -> Option<i64>) -> i32 {
    // Tuple dibaca dengan aturan indeks yang sama (`GetIndex` atas salinan maya).
    let v = atas_larik(rel, true, |t| {
        // Milik tunggal: tanpa kunci (aturan `IndeksLokal` VM).
        if let Some(g) = crate::vm::larik_milik_tunggal(t) { return bulat(g); }
        let (VmValue::Array(a) | VmValue::Tuple(a)) = t else { return None };
        let g = a.lock().unwrap_or_else(|p| p.into_inner());
        bulat(&g)
    });
    match v {
        Some(n) => { unsafe { *keluar = n }; 0 }
        None => 1,
    }
}

/// `TulisIndexLokal`: elemen ke-`i` = nilai berjenis `jenis` (1 bulat, 2 bool, 5 pecahan); 0 = tertulis.
pub extern "C" fn tulis(rel: i64, i: i64, nilai: i64, jenis: i32) -> i32 {
    let Some(v) = skalar_kode(jenis, nilai) else { return 1 };
    let ok = atas_larik(rel, false, |t| {
        if let Some(g) = crate::vm::larik_milik_tunggal(t) {
            let k = norm_indeks(i, g.len())?;
            g[k] = v;
            return Some(());
        }
        // Aturan `PinjamLokal`: isi dipisahkan dari pemegang lain sebelum ditulis (tidak terlihat bila gagal).
        t.pisahkan();
        let VmValue::Array(a) = t else { return None };
        let mut g = a.lock().unwrap_or_else(|p| p.into_inner());
        let k = norm_indeks(i, g.len())?;
        g[k] = v;
        Some(())
    });
    if ok.is_some() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn desimal_sama_dengan_fmt() {
        for n in [0i64, 1, -1, 9, 10, -10, 99, 100, 12345, i64::MAX, i64::MIN, i64::MIN + 1, 1 << 40, -(1 << 50)] {
            assert_eq!(super::desimal(n), format!("{}", n as i128), "{n}");
        }
    }
}
