//! Bytecode → Cranelift IR (`dokumentasi/jit/j2-rencana.md`, `j4-rencana.md`). Tiap posisi stack abstrak
//! = satu variabel `i64`. Jaga yang gagal dan titik keluar mencatat keadaan bingkai lewat `deopt::catat`
//! lalu keluar; status 0 berhasil, 1 deopt, 2 panggilan belum dimulai.

use cranelift_codegen::ir::{condcodes::{FloatCC, IntCC}, types, AbiParam, Block, FuncRef, InstBuilder, MemFlagsData, Signature, StackSlot, StackSlotData, StackSlotKind, Type, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Module};

use super::layak::{Cara, Jenis, Rencana, Simpul};
use crate::vm::opcode::OpCode;
use crate::vm::value::VmFunction;

pub const BERHASIL: i64 = 0;
/// Bingkai native sudah dicatat (yang paling dalam lebih dulu); VM melanjutkannya.
pub const DEOPT: i64 = 1;
/// Callee keluar sebelum mulai (terlalu dalam): pemanggilnya deopt DI instruksi `Call`, VM yang memanggil.
pub const ULANG: i64 = 2;
/// Berhasil, hasilnya pecahan (bitnya di `keluar`).
pub const BERHASIL_PECAHAN: i64 = 4;
/// Berhasil, hasilnya `bool` (cuma kode yang dimasuki dari VM, `KEMBALI_SKALAR`).
pub const BERHASIL_BOOL: i64 = 3;
/// Instruksi yang dijalankan VM dari dalam kode native gagal; galatnya di `lambat::ambil_galat`.
pub const GAGAL: i64 = 5;
/// Berhasil, hasilnya nilai milik VM di posisi `keluar` bingkai (cuma `KEMBALI_SKALAR`).
pub const BERHASIL_NILAI: i64 = 6;
/// `ReturnNil` (cuma `KEMBALI_SKALAR`).
pub const BERHASIL_UNIT: i64 = 7;
/// Kedalaman panggilan native paling banyak; `panggil::coba` menurunkannya mendekati batas bingkai VM.
pub const KEDALAMAN_MAKS: i64 = 4096;

/// `fn(args: *const i64, kedalaman: i32, keluar: *mut i64) -> i32`, satu bentuk untuk semua.
pub fn tanda_tangan(m: &impl Module, cc: CallConv) -> Signature {
    let ptr = m.target_config().pointer_type();
    let mut sig = Signature::new(cc);
    sig.params.push(AbiParam::new(ptr));
    sig.params.push(AbiParam::new(types::I32));
    sig.params.push(AbiParam::new(ptr));
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// Callee bulat (`KEMBALI_BULAT`, cuma dipanggil kode native): `fn(a0..an: i64, kedalaman: i32) -> (i32, i64)` —
/// argumen dan hasil di register, statusnya sama dengan `tanda_tangan`.
pub fn tanda_tangan_bulat(cc: CallConv, n: usize) -> Signature {
    let mut sig = Signature::new(cc);
    for _ in 0..n { sig.params.push(AbiParam::new(types::I64)); }
    sig.params.push(AbiParam::new(types::I32));
    sig.returns.push(AbiParam::new(types::I32));
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// Fungsi yang sedang diturunkan berkonvensi `tanda_tangan_bulat`.
fn register(b: &FunctionBuilder) -> bool { b.func.signature.returns.len() == 2 }

/// `deopt::catat(fid, ip, nilai, n)`.
pub fn tanda_tangan_catat(m: &impl Module, cc: CallConv) -> Signature {
    let ptr = m.target_config().pointer_type();
    let mut sig = Signature::new(cc);
    for t in [types::I32, types::I32, ptr, types::I32] { sig.params.push(AbiParam::new(t)); }
    sig
}

/// `deopt::undi() -> i32`.
pub fn tanda_tangan_undi(cc: CallConv) -> Signature {
    let mut sig = Signature::new(cc);
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// `lambat::baca(rel, i, keluar) -> i32`.
pub fn tanda_tangan_baca(m: &impl Module, cc: CallConv) -> Signature {
    let mut sig = Signature::new(cc);
    for t in [types::I64, types::I64, m.target_config().pointer_type()] { sig.params.push(AbiParam::new(t)); }
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// `lambat::tulis(rel, i, nilai, jenis) -> i32`.
pub fn tanda_tangan_tulis(cc: CallConv) -> Signature {
    let mut sig = Signature::new(cc);
    for t in [types::I64, types::I64, types::I64, types::I32] { sig.params.push(AbiParam::new(t)); }
    sig.returns.push(AbiParam::new(types::I32));
    sig
}

/// Tanda tangan fungsi `lambat` lainnya (`i64` dan penunjuk sama lebar di sasaran JIT).
pub fn tanda_tangan_umum(cc: CallConv, param: &[Type], hasil: Option<Type>) -> Signature {
    let mut sig = Signature::new(cc);
    for t in param { sig.params.push(AbiParam::new(*t)); }
    if let Some(h) = hasil { sig.returns.push(AbiParam::new(h)); }
    sig
}

/// Fungsi `lambat` di luar `baca`/`tulis`.
pub struct ImporLambat {
    pub langkah: FuncId, pub muat: FuncId, pub buang: FuncId, pub benar: FuncId, pub buka: FuncId,
    pub taruh: FuncId, pub ambil: FuncId, pub simpan: FuncId, pub pola: FuncId, pub payload: FuncId,
    pub pola_lokal: FuncId, pub payload_lokal: FuncId, pub payload_angka: FuncId, pub pola_pilih: FuncId, pub buka_lokal: FuncId, pub field_lokal: FuncId, pub field_banyak: FuncId, pub metode_tukar: FuncId, pub metode_lokal: FuncId, pub karakter_lokal: FuncId, pub konteks_mentah: FuncId, pub masuk_ringan: FuncId, pub keluar_ringan: FuncId, pub karakter: FuncId, pub bangun: FuncId, pub baca_pecahan: FuncId, pub jenis_sama: FuncId, pub panjang: FuncId, pub panjang_lokal: FuncId,
    pub payload_jenis_sama: FuncId, pub payload_panjang: FuncId, pub keluar_ringan_angka: FuncId, pub teks_bulat: FuncId,
    pub tambah_nilai: FuncId, pub keluar_ringan_nilai: FuncId, pub ubah_baca: FuncId, pub ubah_tulis: FuncId, pub ubah_tulis_lokal: FuncId, pub ubah_siap: FuncId,
    pub iter_ada: FuncId, pub maju: FuncId, pub langkah_cabang: FuncId, pub sebar: FuncId,
    pub pindah: FuncId, pub muat_global: FuncId, pub simpan_global: FuncId, pub indeks: FuncId, pub field: FuncId,
    pub masuk: FuncId, pub keluar: FuncId, pub salin: FuncId, pub tukar: FuncId, pub cek_metode: FuncId,
    pub bawaan: FuncId, pub biner: FuncId, pub muat_upval: FuncId, pub simpan_upval: FuncId,
    pub tulis_vm: FuncId, pub tulis_lebar: FuncId, pub buka_lebar: FuncId,
}

/// Fungsi impor dan identitas bingkai untuk blok deopt.
pub struct Impor<'a> {
    pub catat: FuncId, pub undi: Option<FuncId>, pub baca: FuncId, pub tulis: FuncId, pub fid: u32,
    pub lambat: &'a ImporLambat,
    /// Per `Rencana::langsung`: callee bisa dipanggil tanpa bingkai VM didorong lebih dulu (`layak::ringan`).
    pub ringan: &'a [bool],
}

/// Kembali dengan status `st` tanpa mencatat apa pun.
fn kembali_status(b: &mut FunctionBuilder, st: i64) {
    let v = b.ins().iconst(types::I32, st);
    pulang(b, v);
}

/// `return` dengan status `st` saja; konvensi register membawa hasil nol.
fn pulang(b: &mut FunctionBuilder, st: Value) {
    if register(b) { let nol = b.ins().iconst(types::I64, 0); b.ins().return_(&[st, nol]); } else { b.ins().return_(&[st]); }
}

/// `bendera`: `BUKA_SUDAH` selama sisa instruksi `Cara::Buka` sesudah bukaannya berhasil.
/// `v128`/`lebar`: variabel `I128` tiap posisi dan `lambat::tulis_lebar` (posisi `Jenis::Lebar`).
struct Keluar {
    catat: FuncRef, fid: i64, ptr: Type, slot: StackSlot, bendera: std::cell::Cell<i64>, v128: Vec<Variable>, lebar: FuncRef,
    buka_lebar: FuncRef, wadah16: StackSlot,
    /// Alamat konstanta chunk yang dikompilasi (tetap selama fungsinya hidup): sumber fungsi bawaan maya.
    konstanta: *const crate::vm::value::VmValue,
    /// `lambat::muat`: salinan maya diwujudkan dari lokalnya (tepi titik temu).
    muat: FuncRef,
}

impl Keluar {
    /// Alamat konstanta asal posisi maya (`Jenis::konstanta`).
    fn alamat_konstanta(&self, j: Jenis) -> Option<i64> {
        j.konstanta().map(|i| unsafe { self.konstanta.add(i as usize) } as i64)
    }
}

/// Alamat fungsi bawaan maya `j` (`Jenis::Bawaan`/`Bungkus`) dari konstanta `f`.
/// Nama kanonik fungsi bawaan maya `j` (konstanta chunk).
fn nama_bawaan(f: &VmFunction, j: Jenis) -> Option<&'static str> {
    match f.chunk.constants.get(j.konstanta()? as usize)? {
        crate::vm::value::VmValue::NativeFn(g) => crate::vm::info_native::info(*g).map(|i| i.kanonik.as_str()),
        _ => None,
    }
}

fn fungsi_bawaan(f: &VmFunction, j: Jenis) -> i64 {
    match f.chunk.constants.get(j.konstanta().expect("callee bawaan maya") as usize) {
        Some(crate::vm::value::VmValue::NativeFn(g)) => *g as usize as i64,
        _ => unreachable!("dijamin layak.rs: konstanta fungsi bawaan"),
    }
}

/// Posisi `p` milik VM dibuka jadi `Jenis::Lebar` (`lambat::buka_lebar`): status (bukan 0 = bukan bulat).
fn buka_ke_lebar(b: &mut FunctionBuilder, k: &Keluar, p: usize) -> Value {
    let pk = b.ins().stack_addr(k.ptr, k.wadah16, 0);
    let pv = b.ins().iconst(types::I64, p as i64);
    let c = b.ins().call(k.buka_lebar, &[pv, pk]);
    b.inst_results(c)[0]
}

/// Isi `wadah16` sesudah `buka_ke_lebar` yang berhasil ke variabel `I128` posisi `p`.
fn ambil_lebar(b: &mut FunctionBuilder, k: &Keluar, p: usize) {
    let lo = b.ins().stack_load(k.ptr, types::I64, k.wadah16, 0);
    let hi = b.ins().stack_load(k.ptr, types::I64, k.wadah16, 8);
    let v = b.ins().iconcat(lo, hi);
    b.def_var(k.v128[p], v);
}

/// Isi native posisi `dari` ke posisi `ke` (variabel 64 dan 128 bit sekaligus).
fn salin_pos(b: &mut FunctionBuilder, var: &[Variable], v128: &[Variable], dari: usize, ke: usize) {
    let v = b.use_var(var[dari]);
    b.def_var(var[ke], v);
    let w = b.use_var(v128[dari]);
    b.def_var(v128[ke], w);
}

/// `Jenis::Lebar` di posisi `i` ditulis ke stack VM (batas: deopt, tumpah, tepi, simpan, kembali).
fn tulis_lebar(b: &mut FunctionBuilder, k: &Keluar, i: usize) {
    let v = b.use_var(k.v128[i]);
    let (lo, hi) = b.ins().isplit(v);
    let p = b.ins().iconst(types::I64, i as i64);
    b.ins().call(k.lebar, &[p, lo, hi]);
}

/// Bit `ip` deopt: posisi yang dibuka instruksi itu sudah berisi angka native (`deopt::pulihkan`).
pub const BUKA_SUDAH: i64 = 1 << 30;
/// Bit `ip` deopt: jaga spekulasi yang gagal (`PintuJit::tandai_spek_gagal`).
pub const GAGAL_SPEK: i64 = 1 << 29;
/// Bit `ip` deopt: puncak stack masih milik VM (hasil instruksi sebelumnya yang spekulasinya gagal).
pub const PUNCAK_VM: i64 = 1 << 28;

/// Blok saat ini menjadi keluar deopt: nilai stack `jenis` dicatat untuk dilanjutkan VM di `ip`.
/// `LoadLocal`/`AmbilLokal` di `ip` diikuti `Dup; GetMethod(Cara::Metode); Swap` tanpa sasaran lompat:
/// nomor method-nya (`Rencana::metode`).
fn metode_sesudah(r: &Rencana, ip: usize) -> Option<usize> {
    let polos = |i: usize| !r.pemimpin[i] && r.keadaan[i].is_some() && !r.keluar[i] && r.wujud[i].is_empty();
    match (r.cara.get(ip + 1)?, r.cara.get(ip + 2)?, r.cara.get(ip + 3)?) {
        (Cara::Salin, Cara::Metode(n), Cara::Tukar) if (1..=3).all(|d| polos(ip + d)) => Some(*n),
        _ => None,
    }
}

/// Lowering `metode_sesudah`: lokal `a` langsung ke posisi argumen `t + 1`; gagal = deopt di `GetMethod`.
#[allow(clippy::too_many_arguments)]
fn metode_lokal(
    b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], r: &Rencana, f: FuncRef, ip: usize, a: usize, t: usize,
    n: usize, ambil: bool,
) {
    let (d, p) = (b.ins().iconst(types::I64, a as i64), b.ins().iconst(types::I64, t as i64));
    // Nama tipe diintern (sama dengan label struct dari kompilator VM): pembantunya cukup membanding alamat.
    let label: &'static std::sync::Arc<str> = Box::leak(Box::new(crate::vm::value::VmValue::intern_nama_struct(&r.metode[n])));
    let l = b.ins().iconst(types::I64, label as *const _ as i64);
    let am = b.ins().iconst(types::I32, ambil as i64);
    let c = b.ins().call(f, &[d, p, l, am]);
    let c = b.inst_results(c)[0];
    jaga_spek(b, k, var, c, ip + 2, r.keadaan[ip + 2].as_ref().expect("dijaga metode_sesudah"));
}

/// Field angka native (bulat/pecahan) dari `BuildStruct` di `ip` yang namanya ada di tabel nama chunk
/// (bacaan `GetField`), masing-masing di variabelnya sendiri.
#[allow(clippy::too_many_arguments)]
fn benih_struct(
    b: &mut FunctionBuilder, f: &VmFunction, var: &[Variable], s: &[Jenis], t: usize, n: usize, ip: usize,
    bentuk: &crate::vm::peta::Bentuk,
) -> Option<(usize, Vec<(u32, i32, Variable)>)> {
    if !bentuk.unik || bentuk.nama.len() != n { return None; }
    let mut daftar = Vec::new();
    for (i, nama) in bentuk.nama.iter().enumerate() {
        let j = match s[t - n + i] { Jenis::Bulat => 1, Jenis::Pecahan => 5, _ => continue };
        let Some(idx) = f.chunk.names.iter().position(|x| x == nama.as_str()) else { continue };
        let cv = b.declare_var(types::I64);
        let v = b.use_var(var[t - n + i]);
        b.def_var(cv, v);
        daftar.push((idx as u32, j, cv));
    }
    Some((ip, daftar))
}

/// Bacaan field angka lain (bukan privat) atas lokal `a` sesudah `ip` di wilayah yang sama dengan tembolok
/// `cse` (berhenti di awal blok atau instruksi di luar `cse_aman`), paling banyak delapan.
fn field_sesudahnya(
    f: &VmFunction, r: &Rencana, blok: &[Option<Block>], ip: usize, a: usize, kunci: (usize, u32, bool, i32),
    cse: &[((usize, u32, bool, i32), Variable)],
) -> Vec<(usize, u32, bool, i32)> {
    let kode = &f.chunk.code;
    let mut ekstra = Vec::new();
    let mut k = ip + 2;
    while k + 1 < kode.len() && ekstra.len() < 8 {
        if blok[k].is_some() || !r.wujud[k].is_empty() || r.keadaan[k].is_none() || r.keluar[k] { break; }
        let field = r.cara[k] == Cara::Muat && kode[k].arg as usize == a && r.cara[k + 1] == Cara::Field
            && kode[k + 1].op == OpCode::GetField && !r.pemimpin[k + 1] && r.keadaan[k + 1].is_some()
            && !r.keluar[k + 1] && r.wujud[k + 1].is_empty();
        if field {
            if let Some(j @ (Jenis::Bulat | Jenis::Pecahan)) = r.spek[k + 1] {
                let kk = (a, kode[k + 1].arg, false, if j == Jenis::Bulat { 1 } else { 5 });
                if kk != kunci && !ekstra.contains(&kk) && !cse.iter().any(|(c, _)| *c == kk) { ekstra.push(kk); }
            }
            k += 2;
            continue;
        }
        if !cse_aman(kode[k].op, &r.cara[k]) { break; }
        k += 1;
    }
    ekstra
}

/// Instruksi yang tidak menulis lokal dan tidak memanggil kode lain: tembolok field lokal (`cse`) tetap sah.
fn cse_aman(op: OpCode, cara: &Cara) -> bool {
    use OpCode::*;
    match cara {
        Cara::Muat | Cara::Konstanta | Cara::Field => true,
        Cara::Asli | Cara::Buka(_) => matches!(op, LoadLocal | LokalHitung | IntImm | Const | Add | Sub | Mul | Div | Mod
            | Neg | Lt | Le | Gt | Ge | Eq | Ne | BitAnd | BitOr | BitXor | Shl | Shr),
        _ => false,
    }
}

/// Lokal `a` mati sesudah `LoadLocal a` di `ip`: di jalur lurus berikutnya (lompat tanpa syarat diikuti) ia
/// ditimpa, di-`Pop` atau fungsinya kembali sebelum dibaca. Cuma opcode yang efek stack-nya pasti; lainnya,
/// cabang bersyarat, atau salinan maya atas `a` = dianggap hidup. Sel tangkapan aman: `ambil` menyalin isinya.
fn mati_sesudah(f: &VmFunction, r: &Rencana, ip: usize, a: usize) -> bool {
    use OpCode::*;
    let kode = &f.chunk.code;
    let mut j = ip + 1;
    for _ in 0..64 {
        let (Some(ins), Some(Some(s))) = (kode.get(j), r.keadaan.get(j)) else { return false };
        if s.iter().any(|x| *x == Jenis::Salinan(a as u32)) { return false; }
        let (h, arg) = (s.len(), ins.arg as usize);
        if h <= a { return true; }
        // Posisi teratas yang dibaca instruksi ini (`h` = tidak membaca stack).
        let baca = match ins.op {
            LoadLocal | LokalHitung if arg == a => return false,
            LoadLocal | LokalHitung | IntImm | Const | Nil | Unit | True | False | LoadGlobal => h,
            SimpanBuang | StoreLocal if arg == a => return h - 1 > a,
            SimpanBuang | StoreLocal => h - 1,
            Pop => return h - 1 == a || { j += 1; continue },
            Add | Sub | Mul | Div | Mod | Lt | Le | Gt | Ge | Eq | Ne | BitAnd | BitOr | BitXor | Shl | Shr => h - 2,
            Neg | Not => h - 1,
            Call => h.saturating_sub(arg + 1),
            Return => return h - 1 != a,
            ReturnNil => return true,
            Jump => { j = arg; continue }
            _ => return false,
        };
        if a >= baca { return false; }
        j += 1;
    }
    false
}

/// Rantai lengan `match` atas satu lokal: kepala tiap lengan `LoadLocal a; CekEnum d; JumpFalse x` (digabung,
/// tanpa sasaran lompat lain di antaranya) dan lengan berikutnya di ujung `Jump` tanpa syarat dari `x`, tiap
/// titiknya berpendahulu tunggal. Hasil: ip kepala -> (nomor rantai, urutan); daftar ip kepala per rantai.
fn rantai_pola(f: &VmFunction, r: &Rencana) -> (std::collections::HashMap<usize, (usize, usize)>, Vec<Vec<usize>>) {
    use OpCode::*;
    let kode = &f.chunk.code;
    let n = kode.len();
    let jalan = |ip: usize| r.keadaan.get(ip).is_some_and(|k| k.is_some()) && !r.keluar[ip] && r.wujud[ip].is_empty();
    // Pendahulu tiap ip: sasaran lompat, tujuan `Cabang`, `TipeLipat`/`TipeSama`, dan jatuhan.
    let mut datang = vec![0u32; n + 1];
    for ip in 0..n {
        if !jalan(ip) { continue; }
        let op = kode[ip].op;
        if matches!(op, Jump | JumpFalse | JumpTrue | JumpFalseKeep | JumpTrueKeep) {
            datang[(kode[ip].arg as usize).min(n)] += 1;
        }
        if let Cara::Cabang { tujuan, .. } = &r.cara[ip] { for &t in tujuan.iter() { datang[t.min(n)] += 1; } }
        if r.cara[ip].lebar() > 1 { datang[(ip + r.cara[ip].lebar()).min(n)] += 1; }
        if !matches!(op, Jump | Return | ReturnNil) { datang[ip + 1] += 1; }
    }
    let kepala = |h: usize| -> Option<usize> {
        (h + 2 < n && kode[h].op == LoadLocal && r.cara.get(h + 1) == Some(&Cara::Pola) && kode[h + 2].op == JumpFalse
            && (h..h + 3).all(jalan) && !r.pemimpin[h + 1] && !r.pemimpin[h + 2]
            && (r.cara[h] == Cara::Muat || (r.cara[h] == Cara::Salinan
                && matches!(r.keadaan[h + 1].as_ref().and_then(|k| k.last()), Some(Jenis::Salinan(_))))))
            .then(|| kode[h + 2].arg as usize)
    };
    let mut peta = std::collections::HashMap::new();
    let mut rantai: Vec<Vec<usize>> = Vec::new();
    for h in 0..n {
        if peta.contains_key(&h) || kepala(h).is_none() { continue; }
        let a = kode[h].arg;
        let mut daftar = vec![h];
        let mut kini = h;
        while let Some(mut x) = kepala(kini) {
            while x < n && kode[x].op == Jump && datang[x] == 1 && jalan(x) { x = kode[x].arg as usize; }
            if x >= n || datang[x] != 1 || kode[x].arg != a || kepala(x).is_none() || daftar.contains(&x) { break; }
            daftar.push(x);
            kini = x;
        }
        if daftar.len() >= 2 {
            for (k, &ip) in daftar.iter().enumerate() { peta.insert(ip, (rantai.len(), k)); }
            rantai.push(daftar);
        }
    }
    (peta, rantai)
}

/// Lanjut di blok baru kalau `benar`, selain itu ke `lambat`.
fn lanjut_jika(b: &mut FunctionBuilder, benar: Value, lambat: Block) {
    let terus = b.create_block();
    b.ins().brif(benar, terus, &[], lambat, &[]);
    b.switch_to_block(terus);
}

/// Tembolok penyimpanan larik lokal di satu wilayah (`ok`, penunjuk data, panjang; `true` = sudah ditetapkan
/// sebelumnya di wilayah ini). `ok` 0 dari jalur lambat situs mana pun yang menyentuhnya.
type TemboLarik = (Variable, Variable, Variable, bool);

/// Alamat elemen `a[i]` atas lokal `rel` lewat tata letak yang ditemukan (aturan `norm_indeks`); tiap pemeriksaan
/// yang gagal lompat ke `lambat`. Cuma penyimpanan milik tunggal (hitungan kuat 1, lemah mentah 1) — yang tidak
/// dijangkau thread lain, sama dengan `value::tunggal`.
fn elemen_sebaris(
    b: &mut FunctionBuilder, tl: &super::tata_letak::TataLetak, vm_p: Value, basis: Value, rel: usize, i: Value,
    lambat: Block, tb: Option<TemboLarik>,
) -> Value {
    let lanjut_jika = |b: &mut FunctionBuilder, benar: Value| lanjut_jika(b, benar, lambat);
    let ukuran = tl.ukuran as i64;
    let (data, n) = match tb {
        // Penyimpanan yang sama sudah diperiksa di wilayah ini (penulisan elemen di tempat tidak mengubahnya).
        Some((ok, dv, nv, true)) => {
            let (cepat, penuh, normal) = (b.create_block(), b.create_block(), b.create_block());
            b.append_block_param(normal, types::I64);
            b.append_block_param(normal, types::I64);
            let okv = b.use_var(ok);
            b.ins().brif(okv, cepat, &[], penuh, &[]);
            b.switch_to_block(cepat);
            let (d, n) = (b.use_var(dv), b.use_var(nv));
            b.ins().jump(normal, &[d.into(), n.into()]);
            b.switch_to_block(penuh);
            let (d, n) = periksa_larik(b, tl, vm_p, basis, rel, lambat);
            let satu = b.ins().iconst(types::I64, 1);
            b.def_var(ok, satu);
            b.def_var(dv, d);
            b.def_var(nv, n);
            b.ins().jump(normal, &[d.into(), n.into()]);
            b.switch_to_block(normal);
            (b.block_params(normal)[0], b.block_params(normal)[1])
        }
        Some((ok, dv, nv, false)) => {
            let (d, n) = periksa_larik(b, tl, vm_p, basis, rel, lambat);
            let satu = b.ins().iconst(types::I64, 1);
            b.def_var(ok, satu);
            b.def_var(dv, d);
            b.def_var(nv, n);
            (d, n)
        }
        None => periksa_larik(b, tl, vm_p, basis, rel, lambat),
    };
    let negatif = b.ins().icmp_imm_s(IntCC::SignedLessThan, i, 0);
    let dari_belakang = b.ins().iadd(i, n);
    let j = b.ins().select(negatif, dari_belakang, i);
    let dalam = b.ins().icmp(IntCC::UnsignedLessThan, j, n);
    lanjut_jika(b, dalam);
    let off = b.ins().imul_imm_s(j, ukuran);
    b.ins().iadd(data, off)
}

/// Tembolok penyimpanan lokal `a` di wilayah ini: yang sudah ada (`true`), atau tiga variabel baru.
fn tembolok_larik(b: &mut FunctionBuilder, larik_tb: &mut Vec<(usize, Variable, Variable, Variable)>, a: usize) -> TemboLarik {
    if let Some(&(_, ok, d, n)) = larik_tb.iter().find(|(l, ..)| *l == a) { return (ok, d, n, true); }
    let (ok, d, n) = (b.declare_var(types::I64), b.declare_var(types::I64), b.declare_var(types::I64));
    larik_tb.push((a, ok, d, n));
    (ok, d, n, false)
}

/// Jalur lambat sebuah situs: penyimpanannya tidak lagi pasti untuk akses berikutnya di wilayah ini.
fn larik_tak_pasti(b: &mut FunctionBuilder, (ok, d, n, _): TemboLarik) {
    let nol = b.ins().iconst(types::I64, 0);
    b.def_var(ok, nol);
    b.def_var(d, nol);
    b.def_var(n, nol);
}

/// Penyimpanan larik lokal `rel` (milik tunggal) lewat tata letak: penunjuk data dan panjang `Vec`-nya; tiap
/// pemeriksaan yang gagal lompat ke `lambat`.
fn periksa_larik(
    b: &mut FunctionBuilder, tl: &super::tata_letak::TataLetak, vm_p: Value, basis: Value, rel: usize, lambat: Block,
) -> (Value, Value) {
    let fl = MemFlagsData::trusted();
    let lanjut_jika = |b: &mut FunctionBuilder, benar: Value| lanjut_jika(b, benar, lambat);
    let ukuran = tl.ukuran as i64;
    // Slot lokal di stack VM; penunjuk dan panjang stack dibaca ulang tiap kali (stack bisa dialokasi ulang).
    let stack = b.ins().load(types::I64, fl, vm_p, (tl.vm_stack + tl.vec_ptr) as i32);
    let panjang = b.ins().load(types::I64, fl, vm_p, (tl.vm_stack + tl.vec_len) as i32);
    let posisi = b.ins().iadd_imm_s(basis, rel as i64);
    let ada = b.ins().icmp(IntCC::UnsignedLessThan, posisi, panjang);
    lanjut_jika(b, ada);
    let off = b.ins().imul_imm_s(posisi, ukuran);
    let slot = b.ins().iadd(stack, off);
    let tag = b.ins().uload8(types::I64, fl, slot, tl.tag as i32);
    let larik = b.ins().icmp_imm_u(IntCC::Equal, tag, tl.tag_array as i64);
    lanjut_jika(b, larik);
    let inner = b.ins().load(types::I64, fl, slot, tl.isi_array as i32);
    let kuat = b.ins().load(types::I64, fl, inner, tl.arc_kuat as i32);
    let lemah = b.ins().load(types::I64, fl, inner, tl.arc_lemah as i32);
    let (k1, l1) = (b.ins().icmp_imm_u(IntCC::Equal, kuat, 1), b.ins().icmp_imm_u(IntCC::Equal, lemah, 1));
    let tunggal = b.ins().band(k1, l1);
    lanjut_jika(b, tunggal);
    let vec = (tl.arc_isi + tl.mutex_isi) as i32;
    let data = b.ins().load(types::I64, fl, inner, vec + tl.vec_ptr as i32);
    let n = b.ins().load(types::I64, fl, inner, vec + tl.vec_len as i32);
    (data, n)
}

/// `a[i]` sebaris (`elemen_sebaris`): bit `i64` elemennya kalau jenisnya yang dispekulasikan, selain itu `lambat`.
#[allow(clippy::too_many_arguments)]
fn baca_sebaris(
    b: &mut FunctionBuilder, tl: &super::tata_letak::TataLetak, vm_p: Value, basis: Value, rel: usize, i: Value,
    pecahan: bool, lambat: Block, tb: Option<TemboLarik>,
) -> Value {
    let fl = MemFlagsData::trusted();
    let lanjut_jika = |b: &mut FunctionBuilder, benar: Value| lanjut_jika(b, benar, lambat);
    let el = elemen_sebaris(b, tl, vm_p, basis, rel, i, lambat, tb);
    let tag = b.ins().uload8(types::I64, fl, el, tl.tag as i32);
    if pecahan {
        let cocok = b.ins().icmp_imm_u(IntCC::Equal, tag, tl.tag_float as i64);
        lanjut_jika(b, cocok);
        return b.ins().load(types::I64, fl, el, tl.isi_float as i32);
    }
    let cocok = b.ins().icmp_imm_u(IntCC::Equal, tag, tl.tag_int as i64);
    lanjut_jika(b, cocok);
    let lo = b.ins().load(types::I64, fl, el, tl.isi_int as i32);
    let hi = b.ins().load(types::I64, fl, el, tl.isi_int as i32 + 8);
    let tanda = b.ins().sshr_imm_u(lo, 63);
    let muat = b.ins().icmp(IntCC::Equal, hi, tanda);
    lanjut_jika(b, muat);
    lo
}

/// `a[i] = v` sebaris untuk `v` bulat (1), bool (2) atau pecahan (5): elemen lama harus `Nil`/`Bool`/`Int`/`Float`
/// (tidak memegang apa pun yang perlu dilepas), lalu tag dan isinya ditimpa di tempat; selain itu `lambat`.
#[allow(clippy::too_many_arguments)]
fn tulis_sebaris(
    b: &mut FunctionBuilder, tl: &super::tata_letak::TataLetak, vm_p: Value, basis: Value, rel: usize, i: Value,
    v: Value, kode: i64, lambat: Block, tb: Option<TemboLarik>,
) {
    let fl = MemFlagsData::trusted();
    let el = elemen_sebaris(b, tl, vm_p, basis, rel, i, lambat, tb);
    let tag = b.ins().uload8(types::I64, fl, el, tl.tag as i32);
    let skalar = [tl.tag_nil, tl.tag_bool, tl.tag_int, tl.tag_float]
        .map(|t| b.ins().icmp_imm_u(IntCC::Equal, tag, t as i64));
    let a = b.ins().bor(skalar[0], skalar[1]);
    let c = b.ins().bor(skalar[2], skalar[3]);
    let lepas_bebas = b.ins().bor(a, c);
    lanjut_jika(b, lepas_bebas, lambat);
    let tag_baru = match kode {
        1 => {
            b.ins().store(fl, v, el, tl.isi_int as i32);
            let tanda = b.ins().sshr_imm_u(v, 63);
            b.ins().store(fl, tanda, el, tl.isi_int as i32 + 8);
            tl.tag_int
        }
        5 => { b.ins().store(fl, v, el, tl.isi_float as i32); tl.tag_float }
        _ => {
            // `istore8` menuntut nilai paling sedikit 16 bit; hasil `icmp` cuma `i8`.
            let bit = b.ins().icmp_imm_u(IntCC::NotEqual, v, 0);
            let bit = b.ins().uextend(types::I32, bit);
            b.ins().istore8(fl, bit, el, tl.isi_bool as i32);
            tl.tag_bool
        }
    };
    let t = b.ins().iconst(types::I8, tag_baru as i64);
    b.ins().store(fl, t, el, tl.tag as i32);
}

fn keluar_deopt(b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], ip: usize, jenis: &[Jenis]) {
    let slot = k.slot;
    // `PUNCAK_VM`: puncaknya nilai milik VM (hasil yang bukaannya gagal) — `tulis_lebar` akan menimpanya.
    let puncak_vm = k.bendera.get() & PUNCAK_VM != 0;
    for (i, j) in jenis.iter().enumerate() {
        if puncak_vm && i + 1 == jenis.len() { continue; }
        // Fungsi, `nil` dan nilai milik VM tidak punya isi native; `deopt::pulihkan` membangunnya.
        if *j == Jenis::Lebar { tulis_lebar(b, k, i); continue; }
        if !j.skalar() { continue; }
        let v = b.use_var(var[i]);
        b.ins().stack_store(k.ptr, v, slot, (i * 8) as i32);
    }
    let pn = b.ins().stack_addr(k.ptr, slot, 0);
    let fid = b.ins().iconst(types::I32, k.fid);
    let ipv = b.ins().iconst(types::I32, ip as i64 | k.bendera.get());
    let n = b.ins().iconst(types::I32, jenis.len() as i64);
    b.ins().call(k.catat, &[fid, ipv, pn, n]);
    let st = b.ins().iconst(types::I32, DEOPT);
    pulang(b, st);
}

/// `cond` benar = deopt di `ip` dengan stack `jenis`; sesudahnya builder ada di blok lanjutan.
fn jaga(b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], cond: Value, ip: usize, jenis: &[Jenis]) {
    let d = b.create_block();
    b.set_cold_block(d);
    let terus = b.create_block();
    b.ins().brif(cond, d, &[], terus, &[]);
    b.switch_to_block(d);
    keluar_deopt(b, k, var, ip, jenis);
    b.switch_to_block(terus);
}

/// `jaga` atas spekulasi: kegagalannya dicatat supaya kompilasi berikutnya tidak berspekulasi lagi.
fn jaga_spek(b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], cond: Value, ip: usize, jenis: &[Jenis]) {
    let lama = k.bendera.get();
    k.bendera.set(lama | GAGAL_SPEK);
    jaga(b, k, var, cond, ip, jenis);
    k.bendera.set(lama);
}

/// Hasil spekulatif (`Rencana::spek`) di posisi `pos` dibuka jadi `j`; gagal = deopt di `lanjut` dengan
/// puncak milik VM, dan spekulasinya tidak diulang.
#[allow(clippy::too_many_arguments)]
/// `LoadLocal` + `AmbilPayload` dengan payload berspekulasi angka: satu pembantu (`payload_angka_lokal`).
/// 1 = bukan enum berpayload, deopt di `ip` (stack sebelum instruksinya); 2 = payload jenis lain, sudah di
/// posisi `pos`, deopt spekulasi di `lanjut` seperti `buka` yang gagal; 0 = angkanya di `var[pos]`.
#[allow(clippy::too_many_arguments)]
fn payload_angka(
    b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], r: &Rencana, f: FuncRef, wadah: StackSlot, slot: Value,
    ip: usize, lanjut: usize, pos: usize, j: Jenis, s: &[Jenis],
) {
    let pk = b.ins().stack_addr(k.ptr, wadah, 0);
    let pv = b.ins().iconst(types::I64, pos as i64);
    let jv = b.ins().iconst(types::I32, if j == Jenis::Pecahan { 5 } else { 1 });
    let c = b.ins().call(f, &[slot, pv, jv, pk]);
    let c = b.inst_results(c)[0];
    let bukan = b.ins().icmp_imm_u(IntCC::Equal, c, 1);
    jaga(b, k, var, bukan, ip, s);
    let lain = b.ins().icmp_imm_u(IntCC::Equal, c, 2);
    let lama = k.bendera.replace(PUNCAK_VM | GAGAL_SPEK);
    jaga(b, k, var, lain, lanjut, r.keadaan[lanjut].as_ref().expect("penerus hasil terjangkau"));
    k.bendera.set(lama);
    let v = b.ins().stack_load(k.ptr, types::I64, wadah, 0);
    b.def_var(var[pos], v);
}

fn buka_hasil(
    b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], r: &Rencana, buka: FuncRef, wadah: StackSlot,
    lanjut: usize, pos: usize, j: Jenis,
) {
    if j == Jenis::Lebar {
        let st = buka_ke_lebar(b, k, pos);
        let lama = k.bendera.replace(PUNCAK_VM | GAGAL_SPEK);
        jaga(b, k, var, st, lanjut, r.keadaan[lanjut].as_ref().expect("penerus hasil terjangkau"));
        k.bendera.set(lama);
        ambil_lebar(b, k, pos);
        return;
    }
    let pk = b.ins().stack_addr(k.ptr, wadah, 0);
    let pv = b.ins().iconst(types::I64, pos as i64);
    let jv = b.ins().iconst(types::I32, if j == Jenis::Pecahan { 5 } else { 1 });
    let c = b.ins().call(buka, &[pv, jv, pk]);
    let st = b.inst_results(c)[0];
    let lama = k.bendera.replace(PUNCAK_VM | GAGAL_SPEK);
    jaga(b, k, var, st, lanjut, r.keadaan[lanjut].as_ref().expect("penerus hasil terjangkau"));
    k.bendera.set(lama);
    let v = b.ins().stack_load(k.ptr, types::I64, wadah, 0);
    b.def_var(var[pos], v);
}

/// Blok sasaran tepi `dari → ke`. Kalau sasarannya titik temu yang dilebarkan (`Rencana::tepi`), tepinya
/// lewat blok stub yang menulis angka/fungsi native ke stack VM dulu; stub diisi `isi_tepi` sesudah
/// instruksi penutup blok sekarang (`FunctionBuilder` tidak boleh pindah dari blok yang belum ditutup).
fn tepi(b: &mut FunctionBuilder, r: &Rencana, blok: &[Option<Block>], dari: usize, ke: usize, tunda: &mut Vec<(Block, usize, usize)>) -> Block {
    let sasaran = blok[ke].expect("sasaran tepi terjangkau");
    let (Some(asal), Some(tuju)) = (r.tepi.get(&(dari, ke)), r.keadaan[ke].as_ref()) else { return sasaran };
    let perlu = |a: &Jenis, t: &Jenis| (t.di_vm() && !a.di_vm()) || (*t == Jenis::Lebar && *a == Jenis::Bulat);
    if !asal.iter().zip(tuju).any(|(a, t)| perlu(a, t)) { return sasaran; }
    let stub = b.create_block();
    tunda.push((stub, dari, ke));
    stub
}

/// Isi blok stub `tepi` yang tertunda; builder berakhir di stub terakhir (sudah tertutup).
#[allow(clippy::too_many_arguments)]
fn isi_tepi(
    b: &mut FunctionBuilder, k: &Keluar, r: &Rencana, blok: &[Option<Block>], var: &[Variable],
    tunda: &mut Vec<(Block, usize, usize)>, tulis: FuncRef, taruh: FuncRef,
) {
    for (stub, dari, ke) in tunda.drain(..) {
        b.switch_to_block(stub);
        let (asal, tuju) = (&r.tepi[&(dari, ke)], r.keadaan[ke].as_ref().expect("sasaran tepi terjangkau"));
        for (i, (a, t)) in asal.iter().zip(tuju).enumerate() {
            // Bulat ke titik temu `Lebar`: cukup diperluas di register.
            if *t == Jenis::Lebar && *a == Jenis::Bulat {
                let v = b.use_var(var[i]);
                let w = b.ins().sextend(types::I128, v);
                b.def_var(k.v128[i], w);
                continue;
            }
            if !t.di_vm() || a.di_vm() { continue; }
            if *a == Jenis::Lebar { tulis_lebar(b, k, i); continue; }
            let p = b.ins().iconst(types::I64, i as i64);
            match a {
                Jenis::Fungsi(c) => {
                    let kv = b.ins().iconst(types::I64, &r.callee_nilai[*c] as *const _ as i64);
                    b.ins().call(taruh, &[p, kv]);
                }
                Jenis::Bawaan(_) | Jenis::Bungkus(..) | Jenis::Konst(_) => {
                    let kv = b.ins().iconst(types::I64, k.alamat_konstanta(*a).expect("maya"));
                    b.ins().call(taruh, &[p, kv]);
                }
                // Lokal asalnya belum ditulis sejak salinan ini dimuat (`layak::boleh_lewat_salinan`).
                Jenis::Salinan(l) => {
                    let d = b.ins().iconst(types::I64, *l as i64);
                    b.ins().call(k.muat, &[d, p]);
                }
                j => {
                    let kode = j.kode();
                    let v = if j.skalar() { b.use_var(var[i]) } else { b.ins().iconst(types::I64, 0) };
                    let kv = b.ins().iconst(types::I32, kode);
                    b.ins().call(tulis, &[p, v, kv]);
                }
            }
        }
        b.ins().jump(blok[ke].expect("sasaran tepi terjangkau"), &[]);
    }
}

/// Nilai `iconst` yang mendefinisikan `v` (di blok yang sama), kalau memang konstanta.
fn konstanta_iconst(b: &FunctionBuilder, v: Value) -> Option<i64> {
    use cranelift_codegen::ir::{InstructionData, Opcode, ValueDef};
    let ValueDef::Result(i, _) = b.func.dfg.value_def(v) else { return None };
    match b.func.dfg.insts[i] {
        InstructionData::UnaryImm { opcode: Opcode::Iconst, imm } => Some(imm.bits()),
        _ => None,
    }
}

/// Operasi atas `Jenis::Lebar`; `false` = bukan bentuk `Lebar` (lowering biasa yang menanganinya).
#[allow(clippy::too_many_arguments)]
fn turun_lebar(
    b: &mut FunctionBuilder, k: &Keluar, r: &Rencana, var: &[Variable], v128: &[Variable], blok: &[Option<Block>],
    tunda: &mut Vec<(Block, usize, usize)>, tulis: FuncRef, taruh: FuncRef, ip: usize, op: OpCode, a: usize, s: &[Jenis],
) -> bool {
    use Jenis::{Bulat, Bungkus, Lebar};
    let t = s.len();
    // Operand ke i128: `Lebar` apa adanya, bulat 64 bit diperluas bertanda.
    let ke128 = |b: &mut FunctionBuilder, i: usize| -> Value {
        if s[i] == Lebar { b.use_var(v128[i]) } else { let x = b.use_var(var[i]); b.ins().sextend(types::I128, x) }
    };
    let nol = |b: &mut FunctionBuilder| -> Value { let z = b.ins().iconst(types::I64, 0); b.ins().sextend(types::I128, z) };
    let dua = t >= 2 && matches!((s[t - 2], s[t - 1]), (Lebar, Bulat | Lebar) | (Bulat, Lebar));
    match op {
        OpCode::Const => {
            let Some(v) = r.konstanta128[ip] else { return false };
            let lo = b.ins().iconst(types::I64, v as i64);
            let hi = b.ins().iconst(types::I64, (v >> 64) as i64);
            let h = b.ins().iconcat(lo, hi);
            b.def_var(v128[t], h);
        }
        OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne if dua => {
            let cc = match op {
                OpCode::Lt => IntCC::SignedLessThan,
                OpCode::Le => IntCC::SignedLessThanOrEqual,
                OpCode::Gt => IntCC::SignedGreaterThan,
                OpCode::Ge => IntCC::SignedGreaterThanOrEqual,
                OpCode::Eq => IntCC::Equal,
                _ => IntCC::NotEqual,
            };
            let (x, y) = (ke128(b, t - 2), ke128(b, t - 1));
            let c = b.ins().icmp(cc, x, y);
            let v = b.ins().uextend(types::I64, c);
            b.def_var(var[t - 2], v);
        }
        // `+ -` 128 bit dengan luapan diperiksa (VM yang melaporkannya); `*` dua bulat 64 bit selalu muat.
        OpCode::Add | OpCode::Sub | OpCode::Mul
            if r.tepi.get(&(ip, ip + 1)).and_then(|st| st.last()) == Some(&Lebar) => {
            let (x, y) = (ke128(b, t - 2), ke128(b, t - 1));
            let h = match op {
                OpCode::Add => b.ins().iadd(x, y),
                OpCode::Sub => b.ins().isub(x, y),
                _ => b.ins().imul(x, y),
            };
            if op != OpCode::Mul {
                // Luapan bertanda: tanda hasil beda dari kedua operand (`+`) / dari `x` sedang `x`, `y` beda tanda (`-`).
                let (a1, a2) = if op == OpCode::Add { (b.ins().bxor(x, h), b.ins().bxor(y, h)) } else { (b.ins().bxor(x, y), b.ins().bxor(x, h)) };
                let m = b.ins().band(a1, a2);
                let z = nol(b);
                let luap = b.ins().icmp(IntCC::SignedLessThan, m, z);
                jaga(b, k, var, luap, ip, s);
            }
            b.def_var(v128[t - 2], h);
        }
        // `%`/`/` dengan `Lebar` (`layak`): kalau kedua isinya muat 64 bit dihitung seperti `int_biner`; yang
        // tidak muat = spekulasi gagal, pembagi 0/-1 = VM yang melapor.
        OpCode::Mod | OpCode::Div if dua => {
            let (x, y) = (ke128(b, t - 2), ke128(b, t - 1));
            let (x64, y64) = (b.ins().ireduce(types::I64, x), b.ins().ireduce(types::I64, y));
            let (xe, ye) = (b.ins().sextend(types::I128, x64), b.ins().sextend(types::I128, y64));
            let (bx, by) = (b.ins().icmp(IntCC::NotEqual, xe, x), b.ins().icmp(IntCC::NotEqual, ye, y));
            let tak_muat = b.ins().bor(bx, by);
            jaga_spek(b, k, var, tak_muat, ip, s);
            let nol = b.ins().icmp_imm_s(IntCC::Equal, y64, 0);
            let min1 = b.ins().icmp_imm_s(IntCC::Equal, y64, -1);
            let tolak = b.ins().bor(nol, min1);
            jaga(b, k, var, tolak, ip, s);
            let h = if op == OpCode::Mod { b.ins().srem(x64, y64) } else { b.ins().sdiv(x64, y64) };
            b.def_var(var[t - 2], h);
        }
        OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot if dua => {
            let (x, y) = (ke128(b, t - 2), ke128(b, t - 1));
            let h = match op {
                OpCode::BitAnd => b.ins().band(x, y),
                OpCode::BitOr => b.ins().bor(x, y),
                OpCode::BitXor => b.ins().bxor(x, y),
                _ => b.ins().band_not(x, y),
            };
            b.def_var(v128[t - 2], h);
        }
        // Geseran 0-127 atas 128 bit (bit yang keluar dibuang, seperti `checked_shl` VM); selain itu VM melapor.
        OpCode::Shl | OpCode::Shr if t >= 2 && s[t - 2] == Lebar && s[t - 1] == Bulat => {
            let (x, y) = (b.use_var(v128[t - 2]), b.use_var(var[t - 1]));
            // Jumlah geser konstan 1-63: dua geser 64 bit, bukan geser 128 bit generik (yang tidak dilipat Cranelift).
            if let Some(kk @ 1..=63) = konstanta_iconst(b, y) {
                let (lo, hi) = b.ins().isplit(x);
                let (nlo, nhi) = if op == OpCode::Shl {
                    let (a1, a2) = (b.ins().ishl_imm_u(hi, kk), b.ins().ushr_imm_u(lo, 64 - kk));
                    (b.ins().ishl_imm_u(lo, kk), b.ins().bor(a1, a2))
                } else {
                    let (a1, a2) = (b.ins().ushr_imm_u(lo, kk), b.ins().ishl_imm_u(hi, 64 - kk));
                    (b.ins().bor(a1, a2), b.ins().sshr_imm_u(hi, kk))
                };
                let h = b.ins().iconcat(nlo, nhi);
                b.def_var(v128[t - 2], h);
                return true;
            }
            let salah = b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, y, 127);
            jaga(b, k, var, salah, ip, s);
            let h = if op == OpCode::Shl { b.ins().ishl(x, y) } else { b.ins().sshr(x, y) };
            b.def_var(v128[t - 2], h);
        }
        OpCode::BitNot if s[t - 1] == Lebar => {
            let x = b.use_var(v128[t - 1]);
            let h = b.ins().bnot(x);
            b.def_var(v128[t - 1], h);
        }
        OpCode::KeBool if s[t - 1] == Lebar => {
            let (x, z) = (b.use_var(v128[t - 1]), nol(b));
            let c = b.ins().icmp(IntCC::NotEqual, x, z);
            let v = b.ins().uextend(types::I64, c);
            b.def_var(var[t - 1], v);
        }
        OpCode::JumpFalse | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep if s[t - 1] == Lebar => {
            let (x, z) = (b.use_var(v128[t - 1]), nol(b));
            let c = b.ins().icmp(IntCC::NotEqual, x, z);
            let (sesudah, sasaran) = (tepi(b, r, blok, ip, ip + 1, tunda), tepi(b, r, blok, ip, a, tunda));
            let (ya, tidak) = if matches!(op, OpCode::JumpFalse | OpCode::JumpFalseKeep) { (sesudah, sasaran) } else { (sasaran, sesudah) };
            b.ins().brif(c, ya, &[], tidak, &[]);
            isi_tepi(b, k, r, blok, var, tunda, tulis, taruh);
        }
        // `math::wrap_*`: 64 bit bawah kedua operand, hasil tak bertanda 0..2^64-1 (aturan `MathWrap*`).
        // Cuma panggilan `wrap_*` itu sendiri (dua argumen): `int(..)` di atasnya juga punya `wrap` di `t - 3`.
        OpCode::Call if a == 2 && t >= 3 && s[t - 3].wrap() => {
            let Bungkus(jenis, _) = s[t - 3] else { return false };
            let bawah = |b: &mut FunctionBuilder, i: usize| -> Value {
                if s[i] == Lebar { let x = b.use_var(v128[i]); b.ins().ireduce(types::I64, x) } else { b.use_var(var[i]) }
            };
            let (x, y) = (bawah(b, t - 2), bawah(b, t - 1));
            let h = match jenis {
                super::layak::BUNGKUS_TAMBAH => b.ins().iadd(x, y),
                super::layak::BUNGKUS_KURANG => b.ins().isub(x, y),
                _ => b.ins().imul(x, y),
            };
            let h = b.ins().uextend(types::I128, h);
            b.def_var(v128[t - 3], h);
        }
        _ => return false,
    }
    true
}

/// Nilai angka jadi `f64`: pecahan dibaca bitnya, bulat dikonversi seperti `i128 as f64` acuan.
fn ke_f64(b: &mut FunctionBuilder, v: Value, j: Jenis) -> Value {
    if j == Jenis::Pecahan { b.ins().bitcast(types::F64, MemFlagsData::new(), v) }
    else { b.ins().fcvt_from_sint(types::F64, v) }
}

fn dari_f64(b: &mut FunctionBuilder, f: Value) -> Value {
    b.ins().bitcast(types::I64, MemFlagsData::new(), f)
}

/// Posisi `rentang` milik kode native ke slot rekaman (`lambat::langkah` menyalinnya ke stack VM);
/// fungsi yang dimuat kode native (`LoadGlobal`) ditaruh langsung sebagai nilai VM-nya.
#[allow(clippy::too_many_arguments)]
fn tumpahkan(
    b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], s: &[Jenis], rentang: std::ops::Range<usize>,
    slot: StackSlot, ptr: Type, taruh: FuncRef, callee: &[crate::vm::value::VmValue],
) {
    for i in rentang {
        // `Lebar` langsung ke stack VM; pembantu membacanya dari sana (`nilai_skalar` tidak mengenalnya).
        if s[i] == Jenis::Lebar { tulis_lebar(b, k, i); continue; }
        if let Jenis::Fungsi(c) = s[i] {
            let ke = b.ins().iconst(types::I64, i as i64);
            let kv = b.ins().iconst(types::I64, &callee[c] as *const _ as i64);
            b.ins().call(taruh, &[ke, kv]);
            continue;
        }
        if let Some(alamat) = k.alamat_konstanta(s[i]) {
            let (ke, kv) = (b.ins().iconst(types::I64, i as i64), b.ins().iconst(types::I64, alamat));
            b.ins().call(taruh, &[ke, kv]);
            continue;
        }
        if !s[i].skalar() { continue; }
        let v = b.use_var(var[i]);
        b.ins().stack_store(ptr, v, slot, (i * 8) as i32);
    }
}

/// `Cara::Ubah`/`UbahLokal`: penerima di posisi atau lokal `p` diperiksa dan field bacaannya ke penyangga
/// (`lambat::ubah_baca`); gagal = deopt spekulasi di `ip`. Hasil: penyangga, `p` dan situs sebagai konstanta.
#[allow(clippy::too_many_arguments)]
fn rumus_baca(
    b: &mut FunctionBuilder, k: &Keluar, var: &[Variable], f_baca: FuncRef, rm: &super::layak::Rumus,
    situs: &'static super::lambat::SitusUbah, p: usize, ip: usize, s: &[Jenis],
) -> (StackSlot, Value, Value) {
    let lebar = (rm.baca.len().max(rm.tulis.len()).max(1) * 8) as u32;
    let wb = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, lebar, 3));
    let (pv, sv, pk) = (b.ins().iconst(types::I64, p as i64), b.ins().iconst(types::I64, situs as *const _ as i64), b.ins().stack_addr(k.ptr, wb, 0));
    let c = b.ins().call(f_baca, &[pv, sv, pk]);
    let st = b.inst_results(c)[0];
    jaga_spek(b, k, var, st, ip, s);
    (wb, pv, sv)
}

/// Simpul rumus yang dipakai dihitung sekali, berurutan (operandnya selalu simpul sebelumnya); nilai field
/// yang berubah ke penyangga `wb` (bit pecahan), argumen ke-`j` dari `arg`; hasil rumus nilai sebagai bit pecahan.
fn rumus_hitung(
    b: &mut FunctionBuilder, rm: &super::layak::Rumus, wb: StackSlot, ptr: Type,
    arg: impl Fn(&mut FunctionBuilder, usize) -> Value,
) -> Option<Value> {
    let mut perlu = vec![false; rm.simpul.len()];
    for &(_, v) in &rm.tulis { perlu[v] = true; }
    if let Some(v) = rm.hasil { perlu[v] = true; }
    for v in (0..rm.simpul.len()).rev() {
        if !perlu[v] { continue; }
        match rm.simpul[v] {
            Simpul::Biner(_, l, r) => { perlu[l] = true; perlu[r] = true; }
            Simpul::Neg(x) | Simpul::KeF64(x) => perlu[x] = true,
            _ => {}
        }
    }
    let mut nilai: Vec<Option<Value>> = vec![None; rm.simpul.len()];
    for v in 0..rm.simpul.len() {
        if !perlu[v] { continue; }
        let h = match rm.simpul[v] {
            // Field bulat tetap `I64` (selalu dibungkus `KeF64`), pecahan dibaca bitnya.
            Simpul::Field(i) => {
                let x = b.ins().stack_load(ptr, types::I64, wb, (i * 8) as i32);
                if rm.baca[i].1 == 5 { b.ins().bitcast(types::F64, MemFlagsData::new(), x) } else { x }
            }
            Simpul::KeF64(x) => b.ins().fcvt_from_sint(types::F64, nilai[x].expect("simpul sebelumnya")),
            Simpul::Arg(j) => arg(b, j),
            Simpul::Konst(x) => b.ins().f64const(x),
            Simpul::Neg(x) => b.ins().fneg(nilai[x].expect("simpul sebelumnya")),
            Simpul::Biner(op, l, r) => {
                let (x, y) = (nilai[l].expect("simpul sebelumnya"), nilai[r].expect("simpul sebelumnya"));
                match op {
                    OpCode::Add => b.ins().fadd(x, y),
                    OpCode::Sub => b.ins().fsub(x, y),
                    OpCode::Mul => b.ins().fmul(x, y),
                    _ => b.ins().fdiv(x, y),
                }
            }
        };
        nilai[v] = Some(h);
    }
    for (j, &(_, v)) in rm.tulis.iter().enumerate() {
        let x = dari_f64(b, nilai[v].expect("simpul tulis dihitung"));
        b.ins().stack_store(ptr, x, wb, (j * 8) as i32);
    }
    rm.hasil.map(|v| dari_f64(b, nilai[v].expect("simpul hasil dihitung")))
}

pub fn turunkan(
    m: &mut impl Module,
    fungsi: &mut cranelift_codegen::ir::Function,
    fctx: &mut FunctionBuilderContext,
    f: &VmFunction,
    r: &Rencana,
    id_callee: &[Option<FuncId>],
    id_langsung: &[FuncId],
    impor: &Impor,
) {
    let ptr = m.target_config().pointer_type();
    let mut b = FunctionBuilder::new(fungsi, fctx);
    let var: Vec<Variable> = (0..r.maks.max(1)).map(|_| b.declare_var(types::I64)).collect();
    // Variabel `I128` tiap posisi untuk `Jenis::Lebar`; salinan posisi menyalin keduanya (yang tak terpakai
    // hilang di SSA).
    let v128: Vec<Variable> = (0..r.maks.max(1)).map(|_| b.declare_var(types::I128)).collect();
    // Penyangga argumen dan hasil milik pemanggil: selaras, tidak pernah menjebak.
    let fl = MemFlagsData::trusted();
    let kode = &f.chunk.code;
    let blok: Vec<Option<Block>> = (0..kode.len())
        .map(|i| (r.pemimpin[i] && r.keadaan[i].is_some()).then(|| b.create_block())).collect();

    let masuk = b.create_block();
    b.append_block_params_for_function_params(masuk);
    b.switch_to_block(masuk);
    // Konvensi register: argumen = parameter, tanpa penyangga dan tanpa `keluar` (tidak dipakai di sana).
    let parameter = b.block_params(masuk).to_vec();
    let (args, kedalaman, keluar) = if register(&b) {
        let nol = b.ins().iconst(ptr, 0);
        (nol, parameter[parameter.len() - 1], nol)
    } else {
        (parameter[0], parameter[1], parameter[2])
    };
    // Satu slot rekaman untuk semua titik deopt fungsi ini (cuma satu yang pernah dipakai per panggilan).
    let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, (r.maks.max(1) * 8) as u32, 3));
    let k = Keluar {
        catat: m.declare_func_in_func(impor.catat, b.func), fid: impor.fid as i64, ptr, slot, bendera: Default::default(),
        v128: v128.clone(), lebar: m.declare_func_in_func(impor.lambat.tulis_lebar, b.func),
        buka_lebar: m.declare_func_in_func(impor.lambat.buka_lebar, b.func),
        wadah16: b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3)),
        konstanta: f.chunk.constants.as_ptr(),
        muat: m.declare_func_in_func(impor.lambat.muat, b.func),
    };
    let undi = impor.undi.map(|u| m.declare_func_in_func(u, b.func));
    let (baca, tulis) = (m.declare_func_in_func(impor.baca, b.func), m.declare_func_in_func(impor.tulis, b.func));
    let wadah = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    // Sembilan: field pertama + paling banyak delapan sesudahnya (`field_sesudahnya`).
    let wadah8 = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 72, 3));
    // Titipan `masuk` untuk `keluar` (kedalaman, basis, slot); panggilan langsung di satu bingkai tidak bertumpuk.
    // `masuk_ringan` menitipkan enam kata lagi (fungsi, argc, jenis, nilai, t, ip).
    let titip = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 72, 3));
    let il = impor.lambat;
    let [f_langkah, f_muat, f_buang, f_benar, f_buka, f_taruh, f_ambil, f_simpan, f_pola, f_payload] =
        [il.langkah, il.muat, il.buang, il.benar, il.buka, il.taruh, il.ambil, il.simpan, il.pola, il.payload]
        .map(|id| m.declare_func_in_func(id, b.func));
    let [f_pola_lokal, f_payload_lokal, f_buka_lokal, f_iter_ada, f_maju, f_cabang, f_sebar, f_payload_angka, f_pola_pilih] =
        [il.pola_lokal, il.payload_lokal, il.buka_lokal, il.iter_ada, il.maju, il.langkah_cabang, il.sebar, il.payload_angka, il.pola_pilih]
        .map(|id| m.declare_func_in_func(id, b.func));
    let [f_pindah, f_muat_global, f_simpan_global, f_indeks, f_field, f_masuk, f_keluar, f_salin, f_tukar, f_cek_metode, f_bawaan, f_biner] =
        [il.pindah, il.muat_global, il.simpan_global, il.indeks, il.field, il.masuk, il.keluar, il.salin, il.tukar, il.cek_metode, il.bawaan, il.biner]
        .map(|id| m.declare_func_in_func(id, b.func));
    let [f_muat_upval, f_simpan_upval, f_tulis_vm, f_field_lokal, f_karakter, f_bangun, f_baca_pecahan, f_jenis_sama, f_panjang, f_field_banyak, f_metode_tukar, f_metode_lokal, f_karakter_lokal, f_masuk_ringan, f_keluar_ringan] =
        [il.muat_upval, il.simpan_upval, il.tulis_vm, il.field_lokal, il.karakter, il.bangun, il.baca_pecahan, il.jenis_sama, il.panjang, il.field_banyak, il.metode_tukar, il.metode_lokal, il.karakter_lokal, il.masuk_ringan, il.keluar_ringan]
        .map(|id| m.declare_func_in_func(id, b.func));
    let [f_panjang_lokal, f_payload_jenis_sama, f_payload_panjang, f_keluar_ringan_angka, f_teks_bulat, f_tambah_nilai, f_keluar_ringan_nilai, f_ubah_baca, f_ubah_tulis, f_ubah_tulis_lokal, f_ubah_siap] =
        [il.panjang_lokal, il.payload_jenis_sama, il.payload_panjang, il.keluar_ringan_angka, il.teks_bulat, il.tambah_nilai, il.keluar_ringan_nilai, il.ubah_baca, il.ubah_tulis, il.ubah_tulis_lokal, il.ubah_siap]
        .map(|id| m.declare_func_in_func(id, b.func));
    let dalam = b.create_block();
    b.set_cold_block(dalam);
    // Kedalaman native dijaga di pintu masuk; parameter dimuat dari penyangga argumen.
    let terlalu = b.ins().icmp_imm_s(IntCC::SignedGreaterThan, kedalaman, KEDALAMAN_MAKS);
    let mulai = b.create_block();
    b.ins().brif(terlalu, dalam, &[], mulai, &[]);
    b.switch_to_block(mulai);
    // Jalur sebaris larik (`tata_letak`): alamat VM dan basis dibaca sekali, cuma di fungsi yang memakainya.
    // Rantai lengan `match`: satu `pola_pilih` di lengan pertama, lengan lain membandingkan indeksnya.
    let (rantai_peta, rantai_daftar) = rantai_pola(f, r);
    let rantai_var: Vec<Variable> = rantai_daftar.iter().map(|_| b.declare_var(types::I64)).collect();
    let rantai_situs: Vec<i64> = rantai_daftar.iter().map(|d| {
        let situs: Vec<&'static super::lambat::SitusPola> = d.iter()
            .map(|&h| super::lambat::SitusPola::bocor(&f.chunk.constants[kode[h + 1].arg as usize])).collect();
        super::lambat::RantaiPola::bocor(situs) as *const _ as i64
    }).collect();
    let sebaris = super::tata_letak::tata_letak().filter(|_| kode.iter().enumerate()
        .any(|(i, x)| r.keadaan[i].as_ref().is_some_and(|s| !r.keluar[i] && match x.op {
            OpCode::IndeksLokal | OpCode::TulisIndexLokal => true,
            OpCode::GetIndex => s.len() >= 2 && matches!(s[s.len() - 2], Jenis::Salinan(_)),
            _ => false,
        })))
        .map(|tl| {
            let ks = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
            let pk = b.ins().stack_addr(ptr, ks, 0);
            let f = m.declare_func_in_func(impor.lambat.konteks_mentah, b.func);
            b.ins().call(f, &[pk]);
            (tl, b.ins().stack_load(ptr, types::I64, ks, 0), b.ins().stack_load(ptr, types::I64, ks, 8))
        });
    // Isi bingkai di titik masuk; posisi tanpa isi native diberi nol supaya tiap variabel terdefinisi.
    let nol = b.ins().iconst(types::I64, 0);
    for v in &var { b.def_var(*v, nol); }
    let nol128 = b.ins().sextend(types::I128, nol);
    for v in &v128 { b.def_var(*v, nol128); }
    for (i, j) in r.jenis_masuk.iter().enumerate() {
        // `Lebar` dibaca dari stack VM (isinya di sana); yang bukan bulat lagi = ULANG, belum ada yang berubah.
        if *j == Jenis::Lebar {
            let st = buka_ke_lebar(&mut b, &k, i);
            let (ulang, terus) = (b.create_block(), b.create_block());
            b.set_cold_block(ulang);
            b.ins().brif(st, ulang, &[], terus, &[]);
            b.switch_to_block(ulang);
            kembali_status(&mut b, ULANG);
            b.switch_to_block(terus);
            ambil_lebar(&mut b, &k, i);
            continue;
        }
        if !j.skalar() { continue; }
        let v = if register(&b) { parameter[i] } else { b.ins().load(types::I64, fl, args, (i * 8) as i32) };
        b.def_var(var[i], v);
    }
    let kedalaman1 = b.ins().iadd_imm_s(kedalaman, 1);
    b.ins().jump(blok[r.masuk].expect("titik masuk selalu pemimpin"), &[]);

    let mut terbuka = false;
    // `LoadLocal` milik VM yang digabung dengan instruksi sesudahnya: yang sudah dikerjakan / bukaan
    // instruksi sesudahnya sudah dikerjakan.
    // `lewati_lagi`: instruksi tambahan yang ikut dikerjakan bersama (sesudah yang pertama).
    let (mut lewati, mut dibuka, mut lewati_lagi) = (false, false, 0usize);
    let mut tunda: Vec<(Block, usize, usize)> = Vec::new();
    // Field lokal yang sudah dibaca sebagai angka di blok ini: (lokal, nama, privat, jenis) → variabelnya.
    let mut cse: Vec<((usize, u32, bool, i32), Variable)> = Vec::new();
    // Penyimpanan larik lokal yang sudah diperiksa di wilayah ini (`TemboLarik` tanpa bendera): lokal, ok, data, panjang.
    let mut larik_tb: Vec<(usize, Variable, Variable, Variable)> = Vec::new();
    // Bacaan yang sudah diambil `field_lokal_banyak` di blok ini: kunci → (berhasil, isi).
    let mut tunggu: Vec<((usize, u32, bool, i32), Variable, Variable)> = Vec::new();
    // `BuildStruct` di `ip` dari angka native yang langsung disimpan ke lokal: field-nya mengisi `cse`.
    let mut benih: Option<(usize, Vec<(u32, i32, Variable)>)> = None;
    for ip in 0..kode.len() {
        let Some(s) = &r.keadaan[ip] else { terbuka = false; continue };
        if let Some(bl) = blok[ip] {
            if terbuka {
                let ke = tepi(&mut b, r, &blok, ip - 1, ip, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
            }
            b.switch_to_block(bl);
            terbuka = true;
        }
        if std::mem::take(&mut lewati) {
            if lewati_lagi > 0 { lewati_lagi -= 1; lewati = true; }
            continue;
        }
        if blok[ip].is_some() || !r.wujud[ip].is_empty() || !cse_aman(kode[ip].op, &r.cara[ip]) { cse.clear(); tunggu.clear(); }
        // Baca/tulis elemen larik sebaris tidak mengubah penyimpanan larik mana pun; selebihnya sama dengan `cse`.
        let elemen = matches!(kode[ip].op, OpCode::IndeksLokal | OpCode::TulisIndexLokal)
            || (kode[ip].op == OpCode::GetIndex && r.keadaan[ip].as_ref().is_some_and(|s| s.len() >= 2
                && matches!(s[s.len() - 2], Jenis::Salinan(_))));
        if blok[ip].is_some() || !r.wujud[ip].is_empty() || !(elemen || cse_aman(kode[ip].op, &r.cara[ip])) { larik_tb.clear(); }
        // Salinan lokal ke stack VM = pemegang kedua penyimpanannya: tulisan berikutnya harus memeriksa ulang.
        if matches!(kode[ip].op, OpCode::LoadLocal | OpCode::LokalHitung | OpCode::AmbilLokal) {
            let l = kode[ip].arg as usize;
            larik_tb.retain(|(x, ..)| *x != l);
        }
        if benih.as_ref().is_some_and(|(bi, _)| bi + 1 != ip || blok[ip].is_some()) { benih = None; }
        // Posisi yang ditulis instruksi ini (puncak sesudahnya dan di atasnya) bukan lagi lokal yang ditembolok.
        if let Some(Some(nx)) = r.keadaan.get(ip + 1) {
            let h = nx.len();
            cse.retain(|(k, _)| k.0 + 1 < h);
            tunggu.retain(|(k, _, _)| k.0 + 1 < h);
            larik_tb.retain(|(l, ..)| l + 1 < h);
        }
        k.bendera.set(0);
        // Salinan maya yang tidak boleh melewati instruksi ini ditulis dulu ke stack VM (`layak::boleh_lewat_salinan`).
        for &(p, j) in &r.wujud[ip] {
            let ke = b.ins().iconst(types::I64, p as i64);
            match j {
                Jenis::Salinan(l) => { let d = b.ins().iconst(types::I64, l as i64); b.ins().call(f_muat, &[d, ke]); }
                _ => { let kv = b.ins().iconst(types::I64, k.alamat_konstanta(j).expect("maya")); b.ins().call(f_taruh, &[ke, kv]); }
            }
        }
        // Sesudah bukaan gabungan, posisinya belum ada di stack VM: deopt acak cuma di instruksi pertamanya.
        if let (Some(u), false) = (undi, dibuka) {
            let c = b.ins().call(u, &[]);
            let c = b.inst_results(c)[0];
            jaga(&mut b, &k, &var, c, ip, s);
        }
        let t = s.len();
        if r.keluar[ip] {
            keluar_deopt(&mut b, &k, &var, ip, s);
            terbuka = false;
            continue;
        }
        let ins = kode[ip];
        let a = ins.arg as usize;
        let atas = |b: &mut FunctionBuilder, k: usize| b.use_var(var[t - k]);
        let i64c = |b: &mut FunctionBuilder, v: i64| b.ins().iconst(types::I64, v);
        let mut buka_s;
        let s: &Vec<Jenis> = match &r.cara[ip] {
            Cara::Asli => s,
            Cara::Salinan => continue,
            Cara::Buka(daftar) => {
                // Nilai milik VM yang dipakai sebagai angka: jaga jenisnya, lalu instruksinya asli.
                buka_s = s.clone();
                for &(p, j) in daftar {
                    buka_s[p] = j;
                    if dibuka { continue; }
                    if j == Jenis::Lebar {
                        let st = buka_ke_lebar(&mut b, &k, p);
                        jaga_spek(&mut b, &k, &var, st, ip, s);
                        ambil_lebar(&mut b, &k, p);
                        continue;
                    }
                    let pk = b.ins().stack_addr(ptr, wadah, 0);
                    let (pv, jv) = (i64c(&mut b, p as i64), b.ins().iconst(types::I32, if j == Jenis::Pecahan { 5 } else { 1 }));
                    let c = b.ins().call(f_buka, &[pv, jv, pk]);
                    let st = b.inst_results(c)[0];
                    jaga_spek(&mut b, &k, &var, st, ip, s);
                    let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                    b.def_var(var[p], v);
                }
                dibuka = false;
                k.bendera.set(BUKA_SUDAH);
                &buka_s
            }
            Cara::Langkah { ambil, taruh, tumpah } => {
                // Operand milik kode native ke slot rekaman, lalu VM menjalankan instruksinya.
                tumpahkan(&mut b, &k, &var, s, t - tumpah..t, slot, ptr, f_taruh, &r.callee_nilai);
                let args = [
                    i64c(&mut b, ip as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64),
                    i64c(&mut b, *tumpah as i64), i64c(&mut b, (t - ambil + taruh) as i64),
                    b.ins().stack_addr(ptr, slot, 0),
                ];
                let c = b.ins().call(f_langkah, &args);
                let st = b.inst_results(c)[0];
                let (gagal, terus) = (b.create_block(), b.create_block());
                b.set_cold_block(gagal);
                b.ins().brif(st, gagal, &[], terus, &[]);
                b.switch_to_block(gagal);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(terus);
                continue;
            }
            Cara::Muat => {
                // Lokal yang isinya salinan maya: lokal asalnya yang dibaca (dan ditembolok).
                let a = match s[a] { Jenis::Salinan(l) => l as usize, _ => a };
                // Dipakai langsung oleh instruksi sesudahnya (bukan sasaran lompat): lokalnya dibaca di
                // tempat; gagal = deopt di sini, sebelum keduanya.
                let gabung = !r.pemimpin.get(ip + 1).copied().unwrap_or(true)
                    && r.keadaan.get(ip + 1).is_some_and(|k| k.is_some()) && !r.keluar[ip + 1]
                    && r.wujud.get(ip + 1).is_some_and(|w| w.is_empty());
                let d = i64c(&mut b, a as i64);
                if let (true, Some(n)) = (gabung, metode_sesudah(r, ip)) {
                    metode_lokal(&mut b, &k, &var, r, f_metode_lokal, ip, a, t, n, false);
                    (lewati, lewati_lagi) = (true, 2);
                    continue;
                }
                match r.cara.get(ip + 1) {
                    Some(Cara::Pola) if gabung && rantai_peta.contains_key(&ip) => {
                        let (ri, kk) = rantai_peta[&ip];
                        if kk == 0 {
                            let rp = i64c(&mut b, rantai_situs[ri]);
                            let c = b.ins().call(f_pola_pilih, &[d, rp]);
                            let c = b.inst_results(c)[0];
                            let c = b.ins().uextend(types::I64, c);
                            b.def_var(rantai_var[ri], c);
                        }
                        let idx = b.use_var(rantai_var[ri]);
                        // Galat jumlah sub-pola di lengan ini: deopt di sini, VM yang melapor (lengan sebelumnya tidak cocok).
                        let salah = b.ins().icmp_imm_u(IntCC::Equal, idx, 1000 + kk as i64);
                        jaga(&mut b, &k, &var, salah, ip, s);
                        let v = b.ins().icmp_imm_u(IntCC::Equal, idx, kk as i64);
                        let v = b.ins().uextend(types::I64, v);
                        b.def_var(var[t], v);
                        lewati = true;
                        continue;
                    }
                    Some(Cara::Pola) if gabung => {
                        let situs = super::lambat::SitusPola::bocor(&f.chunk.constants[kode[ip + 1].arg as usize]);
                        let desc = i64c(&mut b, situs as *const _ as i64);
                        let c = b.ins().call(f_pola_lokal, &[d, desc]);
                        let c = b.inst_results(c)[0];
                        let salah = b.ins().icmp_imm_u(IntCC::Equal, c, 2);
                        jaga(&mut b, &k, &var, salah, ip, s);
                        let v = b.ins().uextend(types::I64, c);
                        b.def_var(var[t], v);
                        lewati = true;
                        continue;
                    }
                    // Field lokal: angka yang dispekulasikan langsung ke register; gagal di field = deopt di
                    // `GetField` (lokalnya sudah di posisi VM), jenis lain = deopt spekulasi sesudahnya.
                    Some(Cara::Field) if gabung && matches!(r.spek[ip + 1], None | Some(Jenis::Bulat | Jenis::Pecahan)) => {
                        let kunci = (a, kode[ip + 1].arg, kode[ip + 1].op == OpCode::GetFieldPrivat,
                            match r.spek[ip + 1] { Some(Jenis::Bulat) => 1, Some(_) => 5, None => 0 });
                        // Lokalnya tidak berubah sejak bacaan yang sama di blok ini (nilai tidak pernah beralias).
                        if let Some(&(_, cv)) = cse.iter().find(|(k, _)| *k == kunci && k.3 != 0) {
                            let v = b.use_var(cv);
                            b.def_var(var[t], v);
                            lewati = true;
                            continue;
                        }
                        // Sudah diambil bersama bacaan sebelumnya: dipakai kalau bitnya ada, jalur biasa kalau tidak.
                        let mut temu = match tunggu.iter().position(|(k, _, _)| *k == kunci) {
                            Some(i) => {
                                let (_, ok, isi) = tunggu.remove(i);
                                let (kena, biasa, lanjut) = (b.create_block(), b.create_block(), b.create_block());
                                let okv = b.use_var(ok);
                                b.ins().brif(okv, kena, &[], biasa, &[]);
                                b.switch_to_block(kena);
                                let cv = b.declare_var(types::I64);
                                let v = b.use_var(isi);
                                b.def_var(var[t], v);
                                b.def_var(cv, v);
                                b.ins().jump(lanjut, &[]);
                                b.switch_to_block(biasa);
                                Some((lanjut, cv))
                            }
                            None => None,
                        };
                        // Ada field angka lain sesudahnya: field ini ikut diambil di panggilan yang sama (bit 0); gagal =
                        // jalur `field_lokal` biasa di bawah, bit field lain tetap dipakai.
                        if temu.is_none() && !kunci.2 && kunci.3 != 0 {
                            let ekstra = field_sesudahnya(f, r, &blok, ip, a, kunci, &cse);
                            if !ekstra.is_empty() {
                                let daftar = super::lambat::SitusBanyak::bocor(std::iter::once(&kunci).chain(ekstra.iter())
                                    .map(|kk| super::lambat::SitusField::bocor(&f.chunk.names[kk.1 as usize], false, kk.3))
                                    .collect());
                                let pl = i64c(&mut b, daftar as *const _ as i64);
                                let ke = b.ins().stack_addr(k.ptr, wadah8, 0);
                                let c = b.ins().call(f_field_banyak, &[d, pl, ke]);
                                let bit = b.inst_results(c)[0];
                                for (j, kk) in ekstra.into_iter().enumerate() {
                                    let (ok, isi) = (b.declare_var(types::I64), b.declare_var(types::I64));
                                    let okv = b.ins().band_imm_u(bit, 1i64 << (j + 1));
                                    b.def_var(ok, okv);
                                    let v = b.ins().stack_load(k.ptr, types::I64, wadah8, ((j + 1) * 8) as i32);
                                    b.def_var(isi, v);
                                    tunggu.push((kk, ok, isi));
                                }
                                let (cepat, biasa, lanjut) = (b.create_block(), b.create_block(), b.create_block());
                                let b0 = b.ins().band_imm_u(bit, 1);
                                b.ins().brif(b0, cepat, &[], biasa, &[]);
                                b.switch_to_block(cepat);
                                let cv = b.declare_var(types::I64);
                                let v = b.ins().stack_load(k.ptr, types::I64, wadah8, 0);
                                b.def_var(var[t], v);
                                b.def_var(cv, v);
                                b.ins().jump(lanjut, &[]);
                                b.switch_to_block(biasa);
                                temu = Some((lanjut, cv));
                            }
                        }
                        let situs = super::lambat::SitusField::bocor(
                            &f.chunk.names[kode[ip + 1].arg as usize], kode[ip + 1].op == OpCode::GetFieldPrivat,
                            match r.spek[ip + 1] { Some(Jenis::Bulat) => 1, Some(_) => 5, None => 0 });
                        let situs = i64c(&mut b, situs as *const _ as i64);
                        let pk = b.ins().stack_addr(k.ptr, wadah, 0);
                        let ke = i64c(&mut b, t as i64);
                        let c = b.ins().call(f_field_lokal, &[d, situs, pk, ke]);
                        let c = b.inst_results(c)[0];
                        let gagal = b.ins().icmp_imm_u(IntCC::Equal, c, 1);
                        let s1 = r.keadaan[ip + 1].as_ref().expect("penerus Muat terjangkau");
                        jaga(&mut b, &k, &var, gagal, ip + 1, s1);
                        if r.spek[ip + 1].is_some() {
                            let bukan = b.ins().icmp_imm_u(IntCC::NotEqual, c, 4);
                            let lama = k.bendera.replace(PUNCAK_VM | GAGAL_SPEK);
                            jaga(&mut b, &k, &var, bukan, ip + 2, r.keadaan[ip + 2].as_ref().expect("penerus hasil terjangkau"));
                            k.bendera.set(lama);
                            let v = b.ins().stack_load(k.ptr, types::I64, wadah, 0);
                            b.def_var(var[t], v);
                            if let Some((lanjut, cv)) = temu {
                                b.def_var(cv, v);
                                b.ins().jump(lanjut, &[]);
                                b.switch_to_block(lanjut);
                                cse.push((kunci, cv));
                            } else {
                                let cv = b.declare_var(types::I64);
                                b.def_var(cv, v);
                                cse.push((kunci, cv));
                                // Field angka lain lokal ini yang dibaca sesudahnya di blok yang sama: sekali kunci.
                                let ekstra = field_sesudahnya(f, r, &blok, ip, a, kunci, &cse);
                                if !ekstra.is_empty() {
                                    let daftar = super::lambat::SitusBanyak::bocor(ekstra.iter()
                                        .map(|kk| super::lambat::SitusField::bocor(&f.chunk.names[kk.1 as usize], false, kk.3))
                                        .collect());
                                    let pl = i64c(&mut b, daftar as *const _ as i64);
                                    let ke = b.ins().stack_addr(k.ptr, wadah8, 0);
                                    let c = b.ins().call(f_field_banyak, &[d, pl, ke]);
                                    let bit = b.inst_results(c)[0];
                                    for (j, kk) in ekstra.into_iter().enumerate() {
                                        let (ok, isi) = (b.declare_var(types::I64), b.declare_var(types::I64));
                                        let okv = b.ins().band_imm_u(bit, 1i64 << j);
                                        b.def_var(ok, okv);
                                        let v = b.ins().stack_load(k.ptr, types::I64, wadah8, (j * 8) as i32);
                                        b.def_var(isi, v);
                                        tunggu.push((kk, ok, isi));
                                    }
                                }
                            }
                        } else if let Some((lanjut, _)) = temu {
                            b.ins().jump(lanjut, &[]);
                            b.switch_to_block(lanjut);
                        }
                        lewati = true;
                        continue;
                    }
                    Some(Cara::Payload) if gabung => {
                        if let Some(j @ (Jenis::Bulat | Jenis::Pecahan)) = r.spek[ip + 1] {
                            payload_angka(&mut b, &k, &var, r, f_payload_angka, wadah, d, ip, ip + 2, t, j, s);
                            lewati = true;
                            continue;
                        }
                        let ke = i64c(&mut b, t as i64);
                        let c = b.ins().call(f_payload_lokal, &[d, ke]);
                        let c = b.inst_results(c)[0];
                        jaga(&mut b, &k, &var, c, ip, s);
                        if let Some(j) = r.spek[ip + 1] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 2, t, j); }
                        lewati = true;
                        continue;
                    }
                    // Semua bukaan instruksi sesudahnya dikerjakan di sini, jadi deopt-nya tetap di `ip`.
                    Some(Cara::Buka(daftar)) if gabung && daftar.iter().any(|(p, _)| *p == t) && !daftar.iter().any(|(_, j)| *j == Jenis::Lebar) => {
                        for &(p, j) in daftar {
                            let pk = b.ins().stack_addr(ptr, wadah, 0);
                            let jv = b.ins().iconst(types::I32, if j == Jenis::Pecahan { 5 } else { 1 });
                            let c = if p == t { b.ins().call(f_buka_lokal, &[d, jv, pk]) } else {
                                let pv = i64c(&mut b, p as i64);
                                b.ins().call(f_buka, &[pv, jv, pk])
                            };
                            let c = b.inst_results(c)[0];
                            jaga_spek(&mut b, &k, &var, c, ip, s);
                            let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                            b.def_var(var[p], v);
                        }
                        dibuka = true;
                        continue;
                    }
                    _ => {}
                }
                let ke = i64c(&mut b, t as i64);
                // Lokal yang mati sesudah ini dipindah, bukan disalin (`ambil`).
                let mati = !matches!(s[kode[ip].arg as usize], Jenis::Salinan(_)) && mati_sesudah(f, r, ip, a);
                b.ins().call(if mati { f_ambil } else { f_muat }, &[d, ke]);
                continue;
            }
            // Fungsi bawaan maya: tidak ditulis ke stack VM (pembacanya mewujudkannya).
            Cara::Konstanta if ins.op == OpCode::Const && matches!(f.chunk.constants[a], crate::vm::value::VmValue::NativeFn(_)) => continue,
            Cara::Konstanta => {
                let k = if ins.op == OpCode::Const { &f.chunk.constants[a] as *const _ as i64 } else { 0 };
                let (ke, kv) = (i64c(&mut b, t as i64), i64c(&mut b, k));
                b.ins().call(f_taruh, &[ke, kv]);
                continue;
            }
            Cara::Ambil => {
                if let Some(n) = metode_sesudah(r, ip) {
                    metode_lokal(&mut b, &k, &var, r, f_metode_lokal, ip, a, t, n, true);
                    (lewati, lewati_lagi) = (true, 2);
                    continue;
                }
                let (d, ke) = (i64c(&mut b, a as i64), i64c(&mut b, t as i64));
                b.ins().call(f_ambil, &[d, ke]);
                continue;
            }
            Cara::Simpan => {
                if s[t - 1] == Jenis::Lebar { tulis_lebar(&mut b, &k, t - 1); }
                let j = s[t - 1];
                let kode = if matches!(j, Jenis::Lebar) { 4 } else { j.kode() };
                let v = if j.skalar() { b.use_var(var[t - 1]) } else { i64c(&mut b, 0) };
                let args = [
                    i64c(&mut b, a as i64), i64c(&mut b, t as i64 - 1), v,
                    b.ins().iconst(types::I32, kode), b.ins().iconst(types::I32, (ins.op == OpCode::SimpanBuang) as i64),
                ];
                b.ins().call(f_simpan, &args);
                // Lokal ini sekarang struct yang baru dibangun: isi field-nya = angka yang membangunnya.
                if let Some((_, daftar)) = benih.take() {
                    for (nama, j, cv) in daftar { cse.push(((a, nama, false, j), cv)); }
                }
                continue;
            }
            Cara::Buang => {
                let p = i64c(&mut b, t as i64 - 1);
                b.ins().call(f_buang, &[p]);
                continue;
            }
            Cara::Benar => {
                let p = i64c(&mut b, t as i64 - 1);
                let simpan = matches!(ins.op, OpCode::JumpFalseKeep | OpCode::JumpTrueKeep);
                let lepas = b.ins().iconst(types::I32, (!simpan) as i64);
                let c = b.ins().call(f_benar, &[p, lepas]);
                let c = b.inst_results(c)[0];
                let v = b.ins().uextend(types::I64, c);
                if ins.op == OpCode::KeBool { b.def_var(var[t - 1], v); continue; }
                let (sesudah, sasaran) = (tepi(&mut b, r, &blok, ip, ip + 1, &mut tunda), tepi(&mut b, r, &blok, ip, a, &mut tunda));
                let (ya, tidak) = if matches!(ins.op, OpCode::JumpFalse | OpCode::JumpFalseKeep) {
                    (sesudah, sasaran)
                } else { (sasaran, sesudah) };
                b.ins().brif(v, ya, &[], tidak, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            // Lengan rantai atas salinan maya (`rantai_pola`; kepalanya `LoadLocal` di `ip - 1`).
            Cara::Pola if ip > 0 && rantai_peta.contains_key(&(ip - 1)) && matches!(s[t - 1], Jenis::Salinan(_)) => {
                let Jenis::Salinan(l) = s[t - 1] else { unreachable!("baru diperiksa") };
                let (ri, kk) = rantai_peta[&(ip - 1)];
                if kk == 0 {
                    let (lv, rp) = (i64c(&mut b, l as i64), i64c(&mut b, rantai_situs[ri]));
                    let c = b.ins().call(f_pola_pilih, &[lv, rp]);
                    let c = b.inst_results(c)[0];
                    let c = b.ins().uextend(types::I64, c);
                    b.def_var(rantai_var[ri], c);
                }
                let idx = b.use_var(rantai_var[ri]);
                let salah = b.ins().icmp_imm_u(IntCC::Equal, idx, 1000 + kk as i64);
                jaga(&mut b, &k, &var, salah, ip, s);
                let v = b.ins().icmp_imm_u(IntCC::Equal, idx, kk as i64);
                let v = b.ins().uextend(types::I64, v);
                b.def_var(var[t - 1], v);
                continue;
            }
            Cara::Pola => {
                // Gagal (2) = deopt di sini dengan stack sebelum instruksinya; VM yang melapor.
                let situs = super::lambat::SitusPola::bocor(&f.chunk.constants[a]);
                let (p, d) = (i64c(&mut b, t as i64 - 1), i64c(&mut b, situs as *const _ as i64));
                // Salinan maya: lokal asalnya yang dibaca, tanpa salinan di stack VM.
                let c = match s[t - 1] {
                    Jenis::Salinan(l) => { let lv = i64c(&mut b, l as i64); b.ins().call(f_pola_lokal, &[lv, d]) }
                    _ => b.ins().call(f_pola, &[p, d]),
                };
                let c = b.inst_results(c)[0];
                let salah = b.ins().icmp_imm_u(IntCC::Equal, c, 2);
                jaga(&mut b, &k, &var, salah, ip, s);
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t - 1], v);
                continue;
            }
            Cara::Bangun(n) => {
                use super::lambat::SitusBangun;
                let situs = SitusBangun::bocor(match ins.op {
                    OpCode::BuildStruct => match &f.chunk.constants[a] {
                        // Langsung dikembalikan: lokal bingkai ini yang sebentuk dan milik tunggal boleh dipakai ulang.
                        crate::vm::value::VmValue::Map(p) if kode.get(ip + 1).is_some_and(|i| i.op == OpCode::Return)
                            && matches!(r.cara.get(ip + 1), Some(Cara::KembaliNilai)) =>
                            SitusBangun::StrukturUlang(p.label.clone(), p.bentuk().expect("dijamin layak.rs")),
                        crate::vm::value::VmValue::Map(p) => SitusBangun::Struktur(p.label.clone(), p.bentuk().expect("dijamin layak.rs")),
                        _ => unreachable!("dijamin layak.rs"),
                    },
                    OpCode::BuildTuple => SitusBangun::Tuple,
                    OpCode::BuildEnum => match &f.chunk.constants[a] {
                        crate::vm::value::VmValue::Map(p) => match &p.label {
                            crate::vm::value::Label::Enum { nama, .. } => SitusBangun::Varian(nama),
                            _ => unreachable!("dijamin layak.rs"),
                        },
                        _ => unreachable!("dijamin layak.rs"),
                    },
                    _ => SitusBangun::Larik,
                });
                if let (OpCode::BuildStruct, super::lambat::SitusBangun::Struktur(_, bentuk)) = (ins.op, situs) {
                    benih = benih_struct(&mut b, f, &var, s, t, *n, ip, bentuk);
                }
                tumpahkan(&mut b, &k, &var, s, t - n..t, slot, ptr, f_taruh, &r.callee_nilai);
                let args = [
                    i64c(&mut b, situs as *const _ as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64),
                    i64c(&mut b, *n as i64), b.ins().stack_addr(ptr, slot, 0),
                ];
                b.ins().call(f_bangun, &args);
                // Hasilnya tinggal di posisinya sebagai lokal (`let`): field-nya = angka yang membangunnya.
                if let Some((_, daftar)) = &benih {
                    for &(nama, j, cv) in daftar { cse.push(((t - n, nama, false, j), cv)); }
                }
                continue;
            }
            Cara::PayloadSama(l, ci) => {
                let (lv, kv) = (i64c(&mut b, *l as i64), i64c(&mut b, &f.chunk.constants[*ci] as *const _ as i64));
                let c = b.ins().call(f_payload_jenis_sama, &[lv, kv]);
                let c = b.inst_results(c)[0];
                let galat = b.ins().icmp_imm_s(IntCC::Equal, c, 2);
                jaga(&mut b, &k, &var, galat, ip, s);
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t - 1], v);
                let ke = tepi(&mut b, r, &blok, ip, ip + 4, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::PayloadPanjang(l) => {
                let (lv, pk) = (i64c(&mut b, *l as i64), b.ins().stack_addr(ptr, wadah, 0));
                let c = b.ins().call(f_payload_panjang, &[lv, pk]);
                let st = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, st, ip, s);
                let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                b.def_var(var[t - 1], v);
                let ke = tepi(&mut b, r, &blok, ip, ip + 2, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::TipeSama(ci) => {
                let (p, kv) = (i64c(&mut b, t as i64 - 1), i64c(&mut b, &f.chunk.constants[*ci] as *const _ as i64));
                let c = b.ins().call(f_jenis_sama, &[p, kv]);
                let c = b.inst_results(c)[0];
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t - 1], v);
                let ke = tepi(&mut b, r, &blok, ip, ip + 3, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::Panjang => {
                let (p, pk) = (i64c(&mut b, t as i64 - 1), b.ins().stack_addr(ptr, wadah, 0));
                let c = b.ins().call(f_panjang, &[p, pk]);
                let st = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, st, ip, s);
                let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                b.def_var(var[t - 1], v);
                continue;
            }
            Cara::TeksBulat => {
                let (p, n) = (i64c(&mut b, t as i64 - 2), b.use_var(var[t - 1]));
                b.ins().call(f_teks_bulat, &[p, n]);
                continue;
            }
            Cara::Ubah(u) => {
                let rm = &r.ubah[*u];
                let p = t - a;
                let situs = super::lambat::SitusUbah::bocor(rm);
                let (wb, pv, sv) = rumus_baca(&mut b, &k, &var, f_ubah_baca, rm, situs, p, ip, s);
                rumus_hitung(&mut b, rm, wb, ptr, |b, j| { let x = b.use_var(var[p + j]); b.ins().bitcast(types::F64, MemFlagsData::new(), x) });
                let pk = b.ins().stack_addr(ptr, wb, 0);
                b.ins().call(f_ubah_tulis, &[pv, sv, pk]);
                continue;
            }
            Cara::UbahLokal { rumus, lebar } => {
                let rm = &r.ubah[*rumus];
                let situs = super::lambat::SitusUbah::bocor(rm);
                let arg = |b: &mut FunctionBuilder, j: usize| match rm.sumber[j - 1] {
                    super::layak::Sumber::Lokal(x) => { let v = b.use_var(var[x]); b.ins().bitcast(types::F64, MemFlagsData::new(), v) }
                    super::layak::Sumber::Konst(x) => b.ins().f64const(x),
                };
                if let Some(tl) = super::tata_letak::tata_letak() {
                    // Struct milik tunggal disiapkan pembantu; pecahan ditulis langsung ke isi field-nya.
                    let lebar_b = (rm.baca.len().max(rm.tulis.len()).max(1) * 8) as u32;
                    let wb = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, lebar_b, 3));
                    let (lv, sv, pk) = (i64c(&mut b, a as i64), i64c(&mut b, situs as *const _ as i64), b.ins().stack_addr(ptr, wb, 0));
                    let c = b.ins().call(f_ubah_siap, &[lv, sv, pk]);
                    let isi = b.inst_results(c)[0];
                    let gagal = b.ins().icmp_imm_u(IntCC::Equal, isi, 0);
                    jaga_spek(&mut b, &k, &var, gagal, ip, s);
                    rumus_hitung(&mut b, rm, wb, ptr, arg);
                    for (j, &(q, _)) in rm.tulis.iter().enumerate() {
                        let x = b.ins().stack_load(ptr, types::I64, wb, (j * 8) as i32);
                        b.ins().store(fl, x, isi, (q * tl.ukuran + tl.isi_float) as i32);
                    }
                } else {
                    let (wb, lv, sv) = rumus_baca(&mut b, &k, &var, f_ubah_baca, rm, situs, a, ip, s);
                    rumus_hitung(&mut b, rm, wb, ptr, arg);
                    let pk = b.ins().stack_addr(ptr, wb, 0);
                    b.ins().call(f_ubah_tulis_lokal, &[lv, sv, pk]);
                }
                let ke = tepi(&mut b, r, &blok, ip, ip + lebar, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::RumusLokal { rumus, lebar } => {
                let rm = &r.ubah[*rumus];
                let situs = super::lambat::SitusUbah::bocor(rm);
                let (wb, _, _) = rumus_baca(&mut b, &k, &var, f_ubah_baca, rm, situs, a, ip, s);
                let h = rumus_hitung(&mut b, rm, wb, ptr, |b, j| match rm.sumber[j - 1] {
                    super::layak::Sumber::Lokal(x) => { let v = b.use_var(var[x]); b.ins().bitcast(types::F64, MemFlagsData::new(), v) }
                    super::layak::Sumber::Konst(x) => b.ins().f64const(x),
                }).expect("rumus nilai");
                b.def_var(var[t], h);
                let ke = tepi(&mut b, r, &blok, ip, ip + lebar, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::PanjangLokal(l) => {
                let (lv, pk) = (i64c(&mut b, *l as i64), b.ins().stack_addr(ptr, wadah, 0));
                let c = b.ins().call(f_panjang_lokal, &[lv, pk]);
                let st = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, st, ip, s);
                let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                b.def_var(var[t - 2], v);
                continue;
            }
            Cara::TipeLipat(ya) => {
                let v = b.ins().iconst(types::I64, *ya as i64);
                b.def_var(var[t - 1], v);
                let ke = tepi(&mut b, r, &blok, ip, ip + 3, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::Cabang { tumpah, hasil_bool, tujuan } => {
                tumpahkan(&mut b, &k, &var, s, t - tumpah..t, slot, ptr, f_taruh, &r.callee_nilai);
                let args = [
                    i64c(&mut b, ip as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64),
                    i64c(&mut b, *tumpah as i64), b.ins().stack_addr(ptr, slot, 0), i64c(&mut b, *hasil_bool as i64),
                ];
                let c = b.ins().call(f_cabang, &args);
                let h = b.inst_results(c)[0];
                let (gagal, terus) = (b.create_block(), b.create_block());
                b.set_cold_block(gagal);
                let salah = b.ins().icmp_imm_s(IntCC::SignedLessThan, h, 0);
                b.ins().brif(salah, gagal, &[], terus, &[]);
                b.switch_to_block(gagal);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(terus);
                let (ya, tidak) = (tepi(&mut b, r, &blok, ip, tujuan[0], &mut tunda), tepi(&mut b, r, &blok, ip, tujuan[1], &mut tunda));
                let pilih = if *hasil_bool { h } else { b.ins().icmp_imm_s(IntCC::Equal, h, tujuan[0] as i64) };
                b.ins().brif(pilih, ya, &[], tidak, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
                continue;
            }
            Cara::Geser => {
                // Yang dibuang dilepas dari stack VM; puncak milik VM dipindah, yang native cukup variabelnya.
                let d0 = t - a - 1;
                for p in d0..t - 1 {
                    if !s[p].di_vm() { continue; }
                    let pv = i64c(&mut b, p as i64);
                    b.ins().call(f_buang, &[pv]);
                }
                if s[t - 1].di_vm() {
                    let (dari, ke) = (i64c(&mut b, t as i64 - 1), i64c(&mut b, d0 as i64));
                    b.ins().call(f_pindah, &[dari, ke]);
                } else {
                    salin_pos(&mut b, &var, &v128, t - 1, d0);
                }
                continue;
            }
            Cara::MuatGlobal => {
                let nama = i64c(&mut b, &f.chunk.names[a] as *const String as i64);
                let ke = i64c(&mut b, t as i64);
                let c = b.ins().call(f_muat_global, &[nama, ke]);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                if let Some(j) = r.spek[ip] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t, j); }
                continue;
            }
            Cara::SimpanGlobal => {
                if s[t - 1] == Jenis::Lebar { tulis_lebar(&mut b, &k, t - 1); }
                let j = s[t - 1];
                let kode = if matches!(j, Jenis::Lebar) { 4 } else { j.kode() };
                let v = if j.skalar() { b.use_var(var[t - 1]) } else { i64c(&mut b, 0) };
                let args = [
                    i64c(&mut b, &f.chunk.names[a] as *const String as i64), i64c(&mut b, t as i64 - 1), v,
                    b.ins().iconst(types::I32, kode),
                ];
                let c = b.ins().call(f_simpan_global, &args);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                continue;
            }
            Cara::Indeks => {
                if s[t - 1] == Jenis::Lebar { tulis_lebar(&mut b, &k, t - 1); }
                let j = s[t - 1];
                let kode = if matches!(j, Jenis::Lebar) { 4 } else { j.kode() };
                let v = if j.skalar() { b.use_var(var[t - 1]) } else { i64c(&mut b, 0) };
                let (p, kv) = (i64c(&mut b, t as i64 - 2), b.ins().iconst(types::I32, kode));
                let c = b.ins().call(f_indeks, &[p, v, kv]);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                // Indeks milik VM di atasnya sudah tidak dipakai.
                if j.di_vm() { let q = i64c(&mut b, t as i64 - 1); b.ins().call(f_buang, &[q]); }
                if let Some(j) = r.spek[ip] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t - 2, j); }
                continue;
            }
            Cara::Field => {
                let p = i64c(&mut b, t as i64 - 1);
                let nama = i64c(&mut b, &f.chunk.names[a] as *const String as i64);
                let privat = b.ins().iconst(types::I32, (ins.op == OpCode::GetFieldPrivat) as i64);
                let c = b.ins().call(f_field, &[p, nama, privat]);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                if let Some(j) = r.spek[ip] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t - 1, j); }
                continue;
            }
            Cara::Langsung(n) => {
                // Bingkai VM callee didorong `masuk`; kode callee dipanggil langsung; `keluar` menaruh hasilnya.
                let jenis = &r.langsung[*n].1;
                // Callee maya: `masuk` mengambilnya dari rencana, posisinya tidak perlu ditulis ke stack VM.
                let (dari, fungsi) = match s[t - a - 1] {
                    Jenis::Fungsi(c) => (t - a, &r.callee_nilai[c] as *const _ as i64),
                    _ => (t - a - 1, 0),
                };
                tumpahkan(&mut b, &k, &var, s, dari..t, slot, ptr, f_taruh, &r.callee_nilai);
                let ringan = fungsi != 0 && impor.ringan.get(*n).copied().unwrap_or(false);
                // Hasil angka sejenis spekulasi dipakai dari register: `keluar_ringan_angka` tidak menaruhnya.
                let angka = match (ringan, r.spek[ip]) {
                    (true, Some(Jenis::Bulat)) => Some(BERHASIL),
                    (true, Some(Jenis::Pecahan)) => Some(BERHASIL_PECAHAN),
                    _ => None,
                };
                let args = [
                    i64c(&mut b, ip as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64),
                    i64c(&mut b, a as i64), b.ins().stack_addr(ptr, slot, 0), i64c(&mut b, fungsi),
                    b.ins().stack_addr(ptr, titip, 0),
                ];
                let c = b.ins().call(if ringan { f_masuk_ringan } else { f_masuk }, &args);
                let m0 = b.inst_results(c)[0];
                let (gagal, cek, asli, akhir) = (b.create_block(), b.create_block(), b.create_block(), b.create_block());
                b.set_cold_block(gagal);
                b.append_block_param(akhir, types::I32);
                let galat = b.ins().icmp_imm_u(IntCC::Equal, m0, 2);
                b.ins().brif(galat, gagal, &[], cek, &[]);
                b.switch_to_block(cek);
                let ulang = b.ins().iconst(types::I32, ULANG);
                b.ins().brif(m0, akhir, &[ulang.into()], asli, &[]);
                b.switch_to_block(asli);
                let argv = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, (a.max(1) * 8) as u32, 3));
                for (k2, j) in jenis.iter().enumerate() {
                    if !j.skalar() { continue; }
                    let v = b.use_var(var[t - a + k2]);
                    b.ins().stack_store(ptr, v, argv, (k2 * 8) as i32);
                }
                let fref = m.declare_func_in_func(id_langsung[*n], b.func);
                let (pa, ph) = (b.ins().stack_addr(ptr, argv, 0), b.ins().stack_addr(ptr, wadah, 0));
                let c = b.ins().call(fref, &[pa, kedalaman1, ph]);
                let st = b.inst_results(c)[0];
                b.ins().jump(akhir, &[st.into()]);
                b.switch_to_block(gagal);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(akhir);
                let st = b.block_params(akhir)[0];
                let st = b.ins().sextend(types::I64, st);
                let h = b.ins().stack_load(ptr, types::I64, wadah, 0);
                let tp = b.ins().stack_addr(ptr, titip, 0);
                let c = match angka {
                    Some(kode) => { let kv = i64c(&mut b, kode); b.ins().call(f_keluar_ringan_angka, &[st, h, tp, kv]) }
                    None if ringan && r.spek[ip].is_none() => b.ins().call(f_keluar_ringan_nilai, &[st, h, tp]),
                    None => b.ins().call(if ringan { f_keluar_ringan } else { f_keluar }, &[st, h, tp]),
                };
                let r0 = b.inst_results(c)[0];
                let (gagal2, terus) = (b.create_block(), b.create_block());
                b.set_cold_block(gagal2);
                let galat = b.ins().icmp_imm_u(IntCC::Equal, r0, 2);
                b.ins().brif(galat, gagal2, &[], terus, &[]);
                b.switch_to_block(gagal2);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(terus);
                // 3 = kode callee basi: hasilnya sudah di slot VM, VM melanjutkan sesudah panggilan ini.
                let basi = b.ins().icmp_imm_u(IntCC::Equal, r0, 3);
                let lama = k.bendera.replace(PUNCAK_VM);
                jaga(&mut b, &k, &var, basi, ip + 1, r.keadaan[ip + 1].as_ref().expect("penerus panggilan terjangkau"));
                k.bendera.set(lama);
                match r.spek[ip] {
                    // Callee kembali dengan angka native sejenis spekulasi: itulah `h` (yang `keluar` taruh di slot
                    // sebagai `VmValue`), jadi tidak perlu dibuka lagi dari stack VM.
                    Some(j @ (Jenis::Bulat | Jenis::Pecahan)) => {
                        let (cepat, biasa, lanjut) = (b.create_block(), b.create_block(), b.create_block());
                        let kode = if j == Jenis::Bulat { BERHASIL } else { BERHASIL_PECAHAN };
                        let cocok = b.ins().icmp_imm_u(IntCC::Equal, st, kode);
                        b.ins().brif(cocok, cepat, &[], biasa, &[]);
                        b.switch_to_block(cepat);
                        b.def_var(var[t - a - 1], h);
                        b.ins().jump(lanjut, &[]);
                        b.switch_to_block(biasa);
                        buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t - a - 1, j);
                        b.ins().jump(lanjut, &[]);
                        b.switch_to_block(lanjut);
                    }
                    Some(j) => buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t - a - 1, j),
                    None => {}
                }
                continue;
            }
            Cara::Bawaan => {
                tumpahkan(&mut b, &k, &var, s, t - a..t, slot, ptr, f_taruh, &r.callee_nilai);
                let fb = fungsi_bawaan(f, s[t - a - 1]);
                // Bawaan polos yang bukan kandidat `bawaan_cepat`: diputuskan sekali di sini, bukan tiap panggilan.
                let polos = if super::lambat::bawaan_polos(fb, a) { super::lambat::BAWAAN_POLOS } else { 0 };
                let args = [
                    i64c(&mut b, ip as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64),
                    i64c(&mut b, a as i64 | polos), b.ins().stack_addr(ptr, slot, 0), i64c(&mut b, fb),
                ];
                let c = b.ins().call(f_bawaan, &args);
                let st = b.inst_results(c)[0];
                let (gagal, terus) = (b.create_block(), b.create_block());
                b.set_cold_block(gagal);
                b.ins().brif(st, gagal, &[], terus, &[]);
                b.switch_to_block(gagal);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(terus);
                if let Some(j) = r.spek[ip] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t - a - 1, j); }
                continue;
            }
            Cara::Biner => {
                tumpahkan(&mut b, &k, &var, s, t - 2..t, slot, ptr, f_taruh, &r.callee_nilai);
                let args = [i64c(&mut b, ip as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64), b.ins().stack_addr(ptr, slot, 0)];
                // `+`: dua teks milik VM disambung langsung (`lambat::tambah_nilai`), selebihnya `biner`.
                let c = b.ins().call(if ins.op == OpCode::Add { f_tambah_nilai } else { f_biner }, &args);
                let st = b.inst_results(c)[0];
                let (gagal, terus) = (b.create_block(), b.create_block());
                b.set_cold_block(gagal);
                let salah = b.ins().icmp_imm_u(IntCC::Equal, st, 2);
                b.ins().brif(salah, gagal, &[], terus, &[]);
                b.switch_to_block(gagal);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(terus);
                if matches!(ins.op, OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge) {
                    let v = b.ins().uextend(types::I64, st);
                    b.def_var(var[t - 2], v);
                }
                continue;
            }
            Cara::MuatUpval => {
                let (idx, ke) = (i64c(&mut b, a as i64), i64c(&mut b, t as i64));
                let c = b.ins().call(f_muat_upval, &[idx, ke]);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                if let Some(j) = r.spek[ip] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t, j); }
                continue;
            }
            Cara::SimpanUpval => {
                if s[t - 1] == Jenis::Lebar { tulis_lebar(&mut b, &k, t - 1); }
                let j = s[t - 1];
                let kode = if matches!(j, Jenis::Lebar) { 4 } else { j.kode() };
                let v = if j.skalar() { b.use_var(var[t - 1]) } else { i64c(&mut b, 0) };
                let args = [i64c(&mut b, a as i64), i64c(&mut b, t as i64 - 1), v, b.ins().iconst(types::I32, kode)];
                let c = b.ins().call(f_simpan_upval, &args);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                continue;
            }
            Cara::Salin => {
                // `Dup; GetMethod; Swap` atas penerima milik VM dengan method yang diketahui: satu pembantu.
                let polos = |i: usize| !r.pemimpin[i] && r.keadaan[i].is_some() && !r.keluar[i] && r.wujud[i].is_empty();
                if let (Some(Cara::Metode(n)), Some(Cara::Tukar)) = (r.cara.get(ip + 1), r.cara.get(ip + 2)) {
                    if s[t - 1].di_vm() && polos(ip + 1) && polos(ip + 2) {
                        let (p, l) = (i64c(&mut b, t as i64 - 1), i64c(&mut b, &r.metode[*n] as *const String as i64));
                        let c = b.ins().call(f_metode_tukar, &[p, l]);
                        let c = b.inst_results(c)[0];
                        jaga_spek(&mut b, &k, &var, c, ip + 1, r.keadaan[ip + 1].as_ref().expect("dijaga polos"));
                        salin_pos(&mut b, &var, &v128, t, t - 1);
                        (lewati, lewati_lagi) = (true, 1);
                        continue;
                    }
                }
                let (dari, ke) = (i64c(&mut b, t as i64 - 1), i64c(&mut b, t as i64));
                b.ins().call(f_salin, &[dari, ke]);
                continue;
            }
            Cara::Tukar => {
                // Isi native ikut variabelnya; isi milik VM ikut posisinya di stack VM.
                let (x, y) = (t - 2, t - 1);
                let (px, py) = (i64c(&mut b, x as i64), i64c(&mut b, y as i64));
                match (s[x].di_vm(), s[y].di_vm()) {
                    (true, true) => { b.ins().call(f_tukar, &[px, py]); }
                    (true, false) => { b.ins().call(f_pindah, &[px, py]); salin_pos(&mut b, &var, &v128, y, x); }
                    _ => { b.ins().call(f_pindah, &[py, px]); salin_pos(&mut b, &var, &v128, x, y); }
                }
                continue;
            }
            Cara::Metode(n) => {
                let (p, l) = (i64c(&mut b, t as i64 - 1), i64c(&mut b, &r.metode[*n] as *const String as i64));
                let c = b.ins().call(f_cek_metode, &[p, l]);
                let c = b.inst_results(c)[0];
                jaga_spek(&mut b, &k, &var, c, ip, s);
                continue;
            }
            Cara::Sebar => {
                // 1 = `Err`/`None`: fungsinya kembali dengan nilai itu (masuk dari VM), selain itu VM yang menjalankannya.
                let p = i64c(&mut b, t as i64 - 1);
                let c = b.ins().call(f_sebar, &[p]);
                let c = b.inst_results(c)[0];
                if r.kembali == super::layak::KEMBALI_SKALAR && !r.ada_defer {
                    let (pulang, terus) = (b.create_block(), b.create_block());
                    let satu = b.ins().icmp_imm_u(IntCC::Equal, c, 1);
                    b.ins().brif(satu, pulang, &[], terus, &[]);
                    b.switch_to_block(pulang);
                    b.ins().store(fl, p, keluar, 0);
                    kembali_status(&mut b, BERHASIL_NILAI);
                    b.switch_to_block(terus);
                }
                jaga(&mut b, &k, &var, c, ip, s);
                continue;
            }
            Cara::IterAda => {
                // Gagal (2) = deopt di sini; VM yang melaporkan state iterator yang rusak.
                let p = i64c(&mut b, a as i64);
                let c = b.ins().call(f_iter_ada, &[p]);
                let c = b.inst_results(c)[0];
                let salah = b.ins().icmp_imm_u(IntCC::Equal, c, 2);
                jaga(&mut b, &k, &var, salah, ip, s);
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t], v);
                continue;
            }
            Cara::Maju => {
                let p = i64c(&mut b, a as i64);
                let c = b.ins().call(f_maju, &[p]);
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                continue;
            }
            Cara::Payload => {
                if let (Jenis::Salinan(l), Some(j @ (Jenis::Bulat | Jenis::Pecahan))) = (s[t - 1], r.spek[ip]) {
                    let lv = i64c(&mut b, l as i64);
                    payload_angka(&mut b, &k, &var, r, f_payload_angka, wadah, lv, ip, ip + 1, t - 1, j, s);
                    continue;
                }
                let p = i64c(&mut b, t as i64 - 1);
                let c = match s[t - 1] {
                    Jenis::Salinan(l) => { let lv = i64c(&mut b, l as i64); b.ins().call(f_payload_lokal, &[lv, p]) }
                    _ => b.ins().call(f_payload, &[p]),
                };
                let c = b.inst_results(c)[0];
                jaga(&mut b, &k, &var, c, ip, s);
                if let Some(j) = r.spek[ip] { buka_hasil(&mut b, &k, &var, r, f_buka, wadah, ip + 1, t - 1, j); }
                continue;
            }
            Cara::KembaliNilai => {
                if s[t - 1] == Jenis::Lebar { tulis_lebar(&mut b, &k, t - 1); }
                let p = i64c(&mut b, t as i64 - 1);
                b.ins().store(fl, p, keluar, 0);
                kembali_status(&mut b, BERHASIL_NILAI);
                terbuka = false;
                continue;
            }
            Cara::KembaliUnit => {
                kembali_status(&mut b, BERHASIL_UNIT);
                terbuka = false;
                continue;
            }
        };
        // `Jenis::Lebar` (i128 di `v128`): bentuk yang `layak::langkah_asli` izinkan.
        let hasil_lebar = matches!(ins.op, OpCode::Add | OpCode::Sub | OpCode::Mul)
            && r.tepi.get(&(ip, ip + 1)).and_then(|st| st.last()) == Some(&Jenis::Lebar);
        let ada_lebar = r.konstanta128[ip].is_some() || hasil_lebar
            || s[t.saturating_sub(3)..].iter().any(|j| *j == Jenis::Lebar || j.wrap());
        if ada_lebar && turun_lebar(&mut b, &k, r, &var, &v128, &blok, &mut tunda, f_tulis_vm, f_taruh, ip, ins.op, a, s) {
            if matches!(ins.op, OpCode::JumpFalse | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) { terbuka = false; }
            continue;
        }
        match ins.op {
            OpCode::LoadLocal | OpCode::LokalHitung => salin_pos(&mut b, &var, &v128, a, t),
            OpCode::IntImm => { let v = b.ins().iconst(types::I64, ins.arg as i32 as i64); b.def_var(var[t], v); }
            OpCode::Const => { let v = b.ins().iconst(types::I64, r.konstanta[ip].unwrap_or(0)); b.def_var(var[t], v); }
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div if s[t - 1] == Jenis::Pecahan || s[t - 2] == Jenis::Pecahan => {
                let x = atas(&mut b, 2);
                let x = ke_f64(&mut b, x, s[t - 2]);
                let y = atas(&mut b, 1);
                let y = ke_f64(&mut b, y, s[t - 1]);
                let h = match ins.op {
                    OpCode::Add => b.ins().fadd(x, y),
                    OpCode::Sub => b.ins().fsub(x, y),
                    OpCode::Mul => b.ins().fmul(x, y),
                    _ => b.ins().fdiv(x, y),
                };
                let h = dari_f64(&mut b, h);
                b.def_var(var[t - 2], h);
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul => {
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                let (h, luap) = match ins.op {
                    OpCode::Add => b.ins().sadd_overflow(x, y),
                    OpCode::Sub => b.ins().ssub_overflow(x, y),
                    _ => b.ins().smul_overflow(x, y),
                };
                // Luapan 64 bit di bingkai ber-stack VM dicatat: kompilasi berikutnya menghitung di 128 bit.
                if r.kembali == super::layak::KEMBALI_BULAT { jaga(&mut b, &k, &var, luap, ip, s) } else { jaga_spek(&mut b, &k, &var, luap, ip, s) }
                b.def_var(var[t - 2], h);
            }
            OpCode::Mod | OpCode::Div => {
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                // Pembagi 0 dan -1 jatuh ke VM: kalimat galatnya, luapan `MIN / -1`, dan aturan `int_biner`.
                let nol = b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                let min1 = b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                let tolak = b.ins().bor(nol, min1);
                jaga(&mut b, &k, &var, tolak, ip, s);
                let h = if ins.op == OpCode::Mod { b.ins().srem(x, y) } else { b.ins().sdiv(x, y) };
                b.def_var(var[t - 2], h);
            }
            OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne
                if s[t - 1] == Jenis::Pecahan || s[t - 2] == Jenis::Pecahan => {
                // Urutan: NaN membuat keempatnya `false`; `!=` benar untuk NaN (`!vm_eq`).
                let cc = match ins.op {
                    OpCode::Lt => FloatCC::LessThan,
                    OpCode::Le => FloatCC::LessThanOrEqual,
                    OpCode::Gt => FloatCC::GreaterThan,
                    OpCode::Ge => FloatCC::GreaterThanOrEqual,
                    OpCode::Eq => FloatCC::Equal,
                    _ => FloatCC::NotEqual,
                };
                let x = atas(&mut b, 2);
                let x = ke_f64(&mut b, x, s[t - 2]);
                let y = atas(&mut b, 1);
                let y = ke_f64(&mut b, y, s[t - 1]);
                let c = b.ins().fcmp(cc, x, y);
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t - 2], v);
            }
            OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne => {
                let cc = match ins.op {
                    OpCode::Lt => IntCC::SignedLessThan,
                    OpCode::Le => IntCC::SignedLessThanOrEqual,
                    OpCode::Gt => IntCC::SignedGreaterThan,
                    OpCode::Ge => IntCC::SignedGreaterThanOrEqual,
                    OpCode::Eq => IntCC::Equal,
                    _ => IntCC::NotEqual,
                };
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                let c = b.ins().icmp(cc, x, y);
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t - 2], v);
            }
            OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot => {
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                let h = match ins.op {
                    OpCode::BitAnd => b.ins().band(x, y),
                    OpCode::BitOr => b.ins().bor(x, y),
                    OpCode::BitXor => b.ins().bxor(x, y),
                    _ => b.ins().band_not(x, y),
                };
                b.def_var(var[t - 2], h);
            }
            OpCode::BitNot => { let x = atas(&mut b, 1); let h = b.ins().bnot(x); b.def_var(var[t - 1], h); }
            // Geser kiri cuma kalau hasil i128-nya muat di 64 bit (bisa digeser balik); sisanya VM.
            OpCode::Shl => {
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                let jauh = b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, y, 63);
                jaga(&mut b, &k, &var, jauh, ip, s);
                let h = b.ins().ishl(x, y);
                let balik = b.ins().sshr(h, y);
                let hilang = b.ins().icmp(IntCC::NotEqual, balik, x);
                jaga(&mut b, &k, &var, hilang, ip, s);
                b.def_var(var[t - 2], h);
            }
            // Geser kanan atas nilai 64 bit: di atas 63 cuma tinggal tandanya; negatif dan >= 128 ke VM.
            OpCode::Shr => {
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                let salah = b.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, y, 128);
                jaga(&mut b, &k, &var, salah, ip, s);
                let enam3 = b.ins().iconst(types::I64, 63);
                let besar = b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, y, 63);
                let kk = b.ins().select(besar, enam3, y);
                let h = b.ins().sshr(x, kk);
                b.def_var(var[t - 2], h);
            }
            OpCode::AmbilLokal => salin_pos(&mut b, &var, &v128, a, t),
            // Wadahnya salinan maya lokal `l`: dibaca langsung dari lokal itu (`layak`: hasil bulat dispekulasikan).
            // `IndeksLokal` atas lokal `a`, atau `GetIndex` atas salinan maya lokal `l` (dibaca langsung dari `l`):
            // hasilnya di posisi wadahnya.
            OpCode::IndeksLokal | OpCode::GetIndex if ins.op == OpCode::IndeksLokal || matches!(s[t - 2], Jenis::Salinan(_)) => {
                let (a, tujuan) = match s.get(t.wrapping_sub(2)) {
                    Some(Jenis::Salinan(l)) if ins.op == OpCode::GetIndex => (*l as usize, t - 2),
                    _ => (a, t - 1),
                };
                let (rel, i) = (b.ins().iconst(types::I64, a as i64), atas(&mut b, 1));
                let pecahan = r.spek[ip] == Some(Jenis::Pecahan);
                // Jalur sebaris; yang tidak pasti (bukan larik milik tunggal, di luar batas, jenis lain) ke pembantu.
                let selesai = sebaris.map(|(tl, vm_p, basis)| {
                    let (lambat, selesai) = (b.create_block(), b.create_block());
                    b.set_cold_block(lambat);
                    b.append_block_param(selesai, types::I64);
                    let tb = tembolok_larik(&mut b, &mut larik_tb, a);
                    let v = baca_sebaris(&mut b, tl, vm_p, basis, a, i, pecahan, lambat, Some(tb));
                    b.ins().jump(selesai, &[v.into()]);
                    b.switch_to_block(lambat);
                    larik_tak_pasti(&mut b, tb);
                    selesai
                });
                let pk = b.ins().stack_addr(ptr, wadah, 0);
                let f_baca = if pecahan { f_baca_pecahan } else { baca };
                let c = b.ins().call(f_baca, &[rel, i, pk]);
                let st = b.inst_results(c)[0];
                jaga_spek(&mut b, &k, &var, st, ip, s);
                let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                let v = match selesai {
                    Some(sel) => { b.ins().jump(sel, &[v.into()]); b.switch_to_block(sel); b.block_params(sel)[0] }
                    None => v,
                };
                b.def_var(var[tujuan], v);
            }
            // Tidak ada yang disambung (`layak`: angka native): indeksnya cuma dibuang.
            OpCode::TumbuhIndexLokal => {}
            OpCode::TulisIndexLokal => {
                let (v, i) = (atas(&mut b, 2), atas(&mut b, 1));
                let kode = s[t - 2].kode();
                // Jalur sebaris untuk bulat/bool/pecahan; selebihnya (dan semua yang tidak pasti) ke pembantu.
                let selesai = sebaris.filter(|_| matches!(kode, 1 | 2 | 5)).map(|(tl, vm_p, basis)| {
                    let (lambat, selesai) = (b.create_block(), b.create_block());
                    b.set_cold_block(lambat);
                    let tb = tembolok_larik(&mut b, &mut larik_tb, a);
                    tulis_sebaris(&mut b, tl, vm_p, basis, a, i, v, kode, lambat, Some(tb));
                    b.ins().jump(selesai, &[]);
                    b.switch_to_block(lambat);
                    larik_tak_pasti(&mut b, tb);
                    selesai
                });
                // Tanpa jalur sebaris, pembantunya bisa mengganti penyimpanan: tembolok lokal ini tidak berlaku lagi.
                if selesai.is_none() { larik_tb.retain(|(l, ..)| *l != a); }
                let rel = b.ins().iconst(types::I64, a as i64);
                let jenis = b.ins().iconst(types::I32, kode);
                let c = b.ins().call(tulis, &[rel, i, v, jenis]);
                let st = b.inst_results(c)[0];
                jaga_spek(&mut b, &k, &var, st, ip, s);
                if let Some(sel) = selesai { b.ins().jump(sel, &[]); b.switch_to_block(sel); }
            }
            OpCode::True | OpCode::False => {
                let v = b.ins().iconst(types::I64, (ins.op == OpCode::True) as i64);
                b.def_var(var[t], v);
            }
            OpCode::Nil | OpCode::Unit => {}
            OpCode::Neg if s[t - 1] == Jenis::Pecahan => {
                let x = atas(&mut b, 1);
                let x = ke_f64(&mut b, x, Jenis::Pecahan);
                let h = b.ins().fneg(x);
                let h = dari_f64(&mut b, h);
                b.def_var(var[t - 1], h);
            }
            OpCode::Neg => {
                let x = atas(&mut b, 1);
                // -MIN tidak muat 64 bit: VM menghitungnya di 128 bit.
                let luap = b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
                jaga(&mut b, &k, &var, luap, ip, s);
                let h = b.ins().ineg(x);
                b.def_var(var[t - 1], h);
            }
            OpCode::Not => { let x = atas(&mut b, 1); let h = b.ins().bxor_imm_s(x, 1); b.def_var(var[t - 1], h); }
            OpCode::KeBool => {
                let x = atas(&mut b, 1);
                let c = b.ins().icmp_imm_s(IntCC::NotEqual, x, 0);
                let h = b.ins().uextend(types::I64, c);
                b.def_var(var[t - 1], h);
            }
            OpCode::Dup => salin_pos(&mut b, &var, &v128, t - 1, t),
            OpCode::Swap => {
                let (x, y) = (atas(&mut b, 2), atas(&mut b, 1));
                let (x2, y2) = (b.use_var(v128[t - 2]), b.use_var(v128[t - 1]));
                b.def_var(var[t - 2], y);
                b.def_var(var[t - 1], x);
                b.def_var(v128[t - 2], y2);
                b.def_var(v128[t - 1], x2);
            }
            OpCode::SlideDown => salin_pos(&mut b, &var, &v128, t - 1, t - a - 1),
            OpCode::Pop | OpCode::LoadGlobal | OpCode::PeriksaArgMetode => {}
            OpCode::StoreLocal | OpCode::SimpanBuang => salin_pos(&mut b, &var, &v128, t - 1, a),
            OpCode::Jump => {
                let ke = tepi(&mut b, r, &blok, ip, a, &mut tunda);
                b.ins().jump(ke, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
            }
            OpCode::JumpFalse | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep => {
                let c = atas(&mut b, 1);
                let (sesudah, sasaran) = (tepi(&mut b, r, &blok, ip, ip + 1, &mut tunda), tepi(&mut b, r, &blok, ip, a, &mut tunda));
                let (ya, tidak) = if matches!(ins.op, OpCode::JumpFalse | OpCode::JumpFalseKeep) {
                    (sesudah, sasaran)
                } else { (sasaran, sesudah) };
                b.ins().brif(c, ya, &[], tidak, &[]);
                isi_tepi(&mut b, &k, r, &blok, &var, &mut tunda, f_tulis_vm, f_taruh);
                terbuka = false;
            }
            OpCode::MakeIterRentang => {
                let (lo, hi) = (atas(&mut b, 2), atas(&mut b, 1));
                // Aturan `range_hitung` untuk langkah 1; selisih yang tak muat 64 bit dihitung VM.
                let (d, luap) = b.ins().ssub_overflow(hi, lo);
                jaga(&mut b, &k, &var, luap, ip, s);
                let (jumlah, ada) = if a == 1 {
                    let satu = b.ins().iconst(types::I64, 1);
                    let (d1, luap) = b.ins().sadd_overflow(d, satu);
                    jaga(&mut b, &k, &var, luap, ip, s);
                    (d1, b.ins().icmp(IntCC::SignedLessThanOrEqual, lo, hi))
                } else { (d, b.ins().icmp(IntCC::SignedLessThan, lo, hi)) };
                let nol = b.ins().iconst(types::I64, 0);
                let jumlah = b.ins().select(ada, jumlah, nol);
                b.def_var(var[t - 2], jumlah);
                b.def_var(var[t - 1], lo);
                b.def_var(var[t], nol);
            }
            OpCode::IterAda => {
                let (jumlah, idx) = (b.use_var(var[a]), b.use_var(var[a + 2]));
                let c = b.ins().icmp(IntCC::SignedLessThan, idx, jumlah);
                let v = b.ins().uextend(types::I64, c);
                b.def_var(var[t], v);
            }
            OpCode::IterMaju => {
                let x = b.use_var(var[a]);
                let luap = b.ins().icmp_imm_s(IntCC::Equal, x, i64::MAX);
                jaga(&mut b, &k, &var, luap, ip, s);
                let h = b.ins().iadd_imm_s(x, 1);
                b.def_var(var[a], h);
            }
            OpCode::IterVar => {
                let kk = a / 2;
                let idx = b.use_var(var[kk + 2]);
                let v = if a % 2 == 0 { idx } else {
                    let awal = b.use_var(var[kk + 1]);
                    let (h, luap) = b.ins().sadd_overflow(awal, idx);
                    jaga(&mut b, &k, &var, luap, ip, s);
                    h
                };
                b.def_var(var[t], v);
            }
            OpCode::Return => {
                let v = atas(&mut b, 1);
                let st = match s[t - 1] { Jenis::Bool => BERHASIL_BOOL, Jenis::Pecahan => BERHASIL_PECAHAN, _ => BERHASIL };
                let st = b.ins().iconst(types::I32, st);
                if register(&b) { b.ins().return_(&[st, v]); } else { b.ins().store(fl, v, keluar, 0); b.ins().return_(&[st]); }
                terbuka = false;
            }
            // `str::char_at` atas teks lokal: `karakter_lokal` menjawab semua yang sah, −1 = galat, deopt (VM melapor).
            OpCode::Call if a == 2 && s[t - 3].bungkus(super::layak::BUNGKUS_CHAR_AT) && matches!(s[t - 2], Jenis::Salinan(_)) => {
                let Jenis::Salinan(l) = s[t - 2] else { unreachable!() };
                let (lv, iv) = (i64c(&mut b, l as i64), b.use_var(var[t - 1]));
                let c = b.ins().call(f_karakter_lokal, &[lv, iv]);
                let h = b.inst_results(c)[0];
                let galat = b.ins().icmp_imm_s(IntCC::SignedLessThan, h, 0);
                jaga(&mut b, &k, &var, galat, ip, s);
                b.def_var(var[t - 3], h);
            }
            // `str::char_at` (`layak`: hasil `Rune`): dijawab di tempat, atau jalur bawaan umum di blok dingin.
            OpCode::Call if a == 2 && s[t - 3].bungkus(super::layak::BUNGKUS_CHAR_AT) => {
                let salinan = match s[t - 2] { Jenis::Salinan(l) => Some(l), _ => None };
                let iv = b.use_var(var[t - 1]);
                let c = match salinan {
                    Some(l) => { let lv = i64c(&mut b, l as i64); b.ins().call(f_karakter_lokal, &[lv, iv]) }
                    None => { let pt = i64c(&mut b, t as i64 - 2); b.ins().call(f_karakter, &[pt, iv]) }
                };
                let h = b.inst_results(c)[0];
                let (lambat, gabung) = (b.create_block(), b.create_block());
                b.set_cold_block(lambat);
                b.append_block_param(gabung, types::I64);
                let umum = b.ins().icmp_imm_s(IntCC::SignedLessThan, h, 0);
                b.ins().brif(umum, lambat, &[], gabung, &[h.into()]);
                b.switch_to_block(lambat);
                // Teks salinan maya diwujudkan dulu: jalur bawaan umum membacanya dari stack VM.
                if let Some(l) = salinan {
                    let (d, ke) = (i64c(&mut b, l as i64), i64c(&mut b, t as i64 - 2));
                    b.ins().call(f_muat, &[d, ke]);
                }
                tumpahkan(&mut b, &k, &var, s, t - 2..t, slot, ptr, f_taruh, &r.callee_nilai);
                let args = [
                    i64c(&mut b, ip as i64), i64c(&mut b, s.as_ptr() as i64), i64c(&mut b, t as i64),
                    i64c(&mut b, 2), b.ins().stack_addr(ptr, slot, 0), i64c(&mut b, fungsi_bawaan(f, s[t - 3])),
                ];
                let c = b.ins().call(f_bawaan, &args);
                let st = b.inst_results(c)[0];
                let (gagal, terus) = (b.create_block(), b.create_block());
                b.set_cold_block(gagal);
                b.ins().brif(st, gagal, &[], terus, &[]);
                b.switch_to_block(gagal);
                kembali_status(&mut b, GAGAL);
                b.switch_to_block(terus);
                let pk = b.ins().stack_addr(ptr, wadah, 0);
                let (pv, jv) = (i64c(&mut b, t as i64 - 3), b.ins().iconst(types::I32, Jenis::Rune.kode() as i64));
                let c = b.ins().call(f_buka, &[pv, jv, pk]);
                let bukan = b.inst_results(c)[0];
                let lama = k.bendera.replace(PUNCAK_VM);
                jaga(&mut b, &k, &var, bukan, ip + 1, r.keadaan[ip + 1].as_ref().expect("penerus panggilan terjangkau"));
                k.bendera.set(lama);
                let v = b.ins().stack_load(ptr, types::I64, wadah, 0);
                b.ins().jump(gabung, &[v.into()]);
                b.switch_to_block(gabung);
                let h = b.block_params(gabung)[0];
                b.def_var(var[t - 3], h);
            }
            OpCode::Call if a == 1 && s[t - 2].bungkus(super::layak::BUNGKUS_KE_BULAT) && s[t - 1] == Jenis::Lebar => {
                let x = b.use_var(k.v128[t - 1]);
                if nama_bawaan(f, s[t - 2]) == Some("uint") {
                    let nol = b.ins().iconst(types::I64, 0);
                    let nol = b.ins().sextend(types::I128, nol);
                    let neg = b.ins().icmp(IntCC::SignedLessThan, x, nol);
                    jaga(&mut b, &k, &var, neg, ip, s);
                }
                if r.keadaan[ip + 1].as_ref().and_then(|j| j.get(t - 2).copied()) == Some(Jenis::Bulat) {
                    let lo = b.ins().ireduce(types::I64, x);
                    let balik = b.ins().sextend(types::I128, lo);
                    let lepas = b.ins().icmp(IntCC::NotEqual, balik, x);
                    jaga_spek(&mut b, &k, &var, lepas, ip, s);
                    b.def_var(var[t - 2], lo);
                } else {
                    b.def_var(k.v128[t - 2], x);
                }
            }
            OpCode::Call if a == 1 && s[t - 2].bungkus(super::layak::BUNGKUS_KE_BULAT) => {
                let v = b.use_var(var[t - 1]);
                // `uint` atas bulat negatif: VM yang melaporkannya.
                if s[t - 1] == Jenis::Bulat && nama_bawaan(f, s[t - 2]) == Some("uint") {
                    let neg = b.ins().icmp_imm_s(IntCC::SignedLessThan, v, 0);
                    jaga(&mut b, &k, &var, neg, ip, s);
                }
                b.def_var(var[t - 2], v);
            }
            OpCode::Call if a == 1 && s[t - 2].bungkus(super::layak::BUNGKUS_KE_PECAHAN) => {
                let v = b.use_var(var[t - 1]);
                let v = if s[t - 1] == Jenis::Bulat { let x = b.ins().fcvt_from_sint(types::F64, v); dari_f64(&mut b, x) } else { v };
                b.def_var(var[t - 2], v);
            }
            OpCode::Call => {
                let Jenis::Fungsi(i) = s[t - a - 1] else { unreachable!("dijamin layak.rs") };
                // Callee bulat berkonvensi register (`tanda_tangan_bulat`).
                let fref = m.declare_func_in_func(id_callee[i].expect("callee asli dikompilasi"), b.func);
                let mut argv: Vec<Value> = (0..a).map(|k| b.use_var(var[t - a + k])).collect();
                argv.push(kedalaman1);
                let panggil = b.ins().call(fref, &argv);
                let (st, hasil) = (b.inst_results(panggil)[0], b.inst_results(panggil)[1]);
                let terus = b.create_block();
                let gagal = b.create_block();
                b.set_cold_block(gagal);
                b.ins().brif(st, gagal, &[], terus, &[]);
                b.switch_to_block(gagal);
                // Callee belum mulai: VM menjalankan `Call` ini sendiri. Callee sudah tercatat: lanjut sesudahnya.
                let ulang = b.ins().icmp_imm_s(IntCC::Equal, st, ULANG);
                let (bu, bb) = (b.create_block(), b.create_block());
                b.set_cold_block(bu);
                b.set_cold_block(bb);
                b.ins().brif(ulang, bu, &[], bb, &[]);
                b.switch_to_block(bu);
                keluar_deopt(&mut b, &k, &var, ip, s);
                b.switch_to_block(bb);
                // Rekaman di `ip + 1`: bendera instruksi ini (`BUKA_SUDAH` dari bukaan argumennya) bukan miliknya —
                // dulu `Buka` di `ip + 1` membuat deopt membaca posisi milik VM sebagai angka dari slot kosong.
                let lama = k.bendera.replace(0);
                keluar_deopt(&mut b, &k, &var, ip + 1, &s[..t - a - 1]);
                k.bendera.set(lama);
                b.switch_to_block(terus);
                b.def_var(var[t - a - 1], hasil);
            }
            _ => unreachable!("dijamin layak.rs"),
        }
    }
    b.switch_to_block(dalam);
    let st = b.ins().iconst(types::I32, ULANG);
    pulang(&mut b, st);
    b.seal_all_blocks();
    b.finalize(m.target_config());
}
