//! Kelayakan JIT (`dokumentasi/jit/j4-rencana.md`): bytecode ditelusuri dengan stack ABSTRAK dari satu
//! titik masuk. Instruksi yang belum bisa dibuktikan jadi TITIK KELUAR: kode native deopt di sana dan VM
//! melanjutkan bingkainya, jadi fungsi apa pun bisa dikompilasi sebagian.

use std::sync::Arc;

use crate::vm::opcode::OpCode;
use crate::vm::value::{VmFunction, VmValue};

/// Jenis satu posisi stack di kode yang dikompilasi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Jenis {
    Bulat,
    Bool,
    /// `f64`; bitnya disimpan di variabel `i64`.
    Pecahan,
    /// Rujukan ke `Rencana::callee[i]` (hasil `LoadGlobal`, cuma boleh dipanggil).
    Fungsi(usize),
    /// `nil` (juga lubang yang ditinggal `AmbilLokal`).
    Kosong,
    /// `()`: seperti `Kosong`, tidak ditulis ke stack VM sampai ada yang perlu isinya (tepi, deopt, langkah VM).
    Satuan,
    /// Nilai lain, milik bingkai VM: tetap di stack VM di indeks yang sama, cuma disentuh lewat `lambat`.
    Nilai,
    /// Fungsi bawaan dari konstanta ke-`i` (`Const`): maya — tidak ditulis ke stack VM, diwujudkan dari
    /// konstantanya oleh pembacanya (tumpahan, tepi, deopt), seperti `Fungsi`. `Call` atasnya tidak pernah
    /// mendorong bingkai.
    Bawaan(u32),
    /// `Bawaan` yang dikenal (`BUNGKUS_*`: `math::wrap_*`, `str::char_at`, `int`/`uint`): dihitung native.
    Bungkus(u8, u32),
    /// Bulat i128 di variabel `I128`; ditulis ke stack VM (`lambat::tulis_lebar`) cuma di batas.
    Lebar,
    /// Salinan lokal milik VM `a` (`LoadLocal`) yang belum ditulis ke stack VM: cuma hidup melewati instruksi
    /// yang tidak menyentuh lokal (`boleh_lewat_salinan`), diwujudkan (`Rencana::wujud`, deopt) dari lokalnya.
    Salinan(u32),
    /// Kode karakter (`u32` di variabel `i64`); cuma lahir dari intrinsik `str::char_at`, tidak pernah
    /// jenis titik masuk (rune milik VM tetap `Nilai`).
    Rune,
    /// Konstanta ke-`i` berupa varian enum tanpa payload (`var op = E::A`): tidak ditulis ke stack VM selama
    /// cuma melewati instruksi `boleh_lewat_konst`; selain itu diwujudkan (`Rencana::wujud`, tepi, deopt).
    Konst(u32),
}

pub const BUNGKUS_TAMBAH: u8 = 0;
pub const BUNGKUS_KURANG: u8 = 1;
pub const BUNGKUS_KALI: u8 = 2;
/// `str::char_at` dan `int`/`uint`: intrinsik rune (`Jenis::Rune`), bukan `math::wrap_*`.
pub const BUNGKUS_CHAR_AT: u8 = 3;
pub const BUNGKUS_KE_BULAT: u8 = 4;
/// `float`: bulat jadi pecahan (`n as f64`), pecahan apa adanya.
pub const BUNGKUS_KE_PECAHAN: u8 = 5;

impl Jenis {
    /// Isinya ada di stack VM, bukan di variabel native.
    pub fn di_vm(self) -> bool { self == Jenis::Nilai }

    /// Nilainya tetap dan diketahui saat kompilasi (callee, fungsi bawaan): tidak ada di stack VM, jadi
    /// pembantu yang membaca stack VM tidak boleh menerimanya mentah.
    pub fn tetap(self) -> bool {
        matches!(self, Jenis::Fungsi(_) | Jenis::Bawaan(_) | Jenis::Bungkus(..) | Jenis::Salinan(_) | Jenis::Konst(_))
    }

    /// Indeks konstanta fungsi bawaan maya.
    pub fn konstanta(self) -> Option<u32> {
        match self { Jenis::Bawaan(i) | Jenis::Bungkus(_, i) | Jenis::Konst(i) => Some(i), _ => None }
    }

    /// `math::wrap_*` (hasil `Lebar`).
    pub fn wrap(self) -> bool { matches!(self, Jenis::Bungkus(k, _) if k <= BUNGKUS_KALI) }

    /// Intrinsik bawaan `k` (`BUNGKUS_*`).
    pub fn bungkus(self, k: u8) -> bool { matches!(self, Jenis::Bungkus(j, _) if j == k) }

    /// Kode jenis untuk pembantu dan titik masuk (`lambat::skalar_kode`); 4 = milik VM.
    pub fn kode(self) -> i64 {
        match self {
            Jenis::Bulat => 1,
            Jenis::Bool => 2,
            Jenis::Kosong => 3,
            Jenis::Pecahan => 5,
            Jenis::Lebar => 6,
            Jenis::Rune => 7,
            Jenis::Satuan => 8,
            Jenis::Nilai | Jenis::Bawaan(_) | Jenis::Bungkus(..) | Jenis::Fungsi(_) | Jenis::Salinan(_) | Jenis::Konst(_) => 4,
        }
    }

    /// Isinya angka di variabel `i64` (bukan `Lebar`, bukan milik VM).
    pub fn skalar(self) -> bool { matches!(self, Jenis::Bulat | Jenis::Bool | Jenis::Pecahan | Jenis::Rune) }
}

/// Fungsi bawaan `f` sebagai intrinsik `Jenis::Bungkus`.
fn bungkus(f: crate::vm::value::VmNativeFn) -> Option<u8> {
    match crate::vm::info_native::info(f)?.kanonik.as_str() {
        "math::wrap_add" => Some(BUNGKUS_TAMBAH),
        "math::wrap_sub" => Some(BUNGKUS_KURANG),
        "math::wrap_mul" => Some(BUNGKUS_KALI),
        "str::char_at" => Some(BUNGKUS_CHAR_AT),
        "int" | "uint" => Some(BUNGKUS_KE_BULAT),
        "float" => Some(BUNGKUS_KE_PECAHAN),
        _ => None,
    }
}

/// Cara satu instruksi dijalankan kode native (`Rencana::cara`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cara {
    Asli,
    /// Posisi `Nilai` ini dibuka jadi bulat/pecahan (jaga), lalu instruksinya asli.
    Buka(Vec<(usize, Jenis)>),
    /// VM menjalankan instruksinya (`lambat::langkah`): `ambil` operand teratas diganti `taruh` nilai;
    /// `tumpah` posisi teratas (paling sedikit `ambil`) disalin dulu ke stack VM.
    Langkah { ambil: usize, taruh: usize, tumpah: usize },
    /// `IterAda` atas iterator milik VM (`lambat::iter_ada`), hasilnya bool asli.
    IterAda,
    /// `IterMaju` atas indeks iterator milik VM (`lambat::maju`).
    Maju,
    /// VM menjalankan instruksi yang bercabang (`lambat::langkah_cabang`), lalu kode native lompat ke
    /// `tujuan[0]` bila `bool` hasilnya benar / `ip` berikutnya sama dengannya, selain itu `tujuan[1]`.
    Cabang { tumpah: usize, hasil_bool: bool, tujuan: [usize; 2] },
    /// `Typeof; Const "<nama>"; Eq` atas jenis native yang pasti: bool tetap, lanjut di `ip + 3`.
    TipeLipat(bool),
    /// `Typeof; Const "<nama>"; Eq` atas nilai milik VM (`lambat::jenis_sama`), lanjut di `ip + 3`.
    TipeSama(usize),
    /// `Len` atas nilai milik VM (`lambat::panjang`): bulat native; gagal = deopt, VM yang melapor.
    Panjang,
    /// `len(x)` atas salinan maya lokal ini (`lambat::panjang_lokal`): bulat native; gagal = deopt di
    /// panggilannya, VM yang melapor.
    PanjangLokal(u32),
    /// `LoadLocal` yang hasilnya `Jenis::Salinan`: tidak ada yang ditulis.
    Salinan,
    /// `BuildStruct`/`BuildArray`/`BuildTuple` atas `n` posisi teratas (`lambat::bangun`): angka native
    /// diambil dari tumpahan, tanpa lewat stack VM.
    Bangun(usize),
    /// `Propagate` atas `Nilai` (`lambat::sebar`).
    Sebar,
    /// `SlideDown` dengan posisi milik VM: yang dibuang dilepas (`lambat::buang`), puncaknya dipindah.
    Geser,
    /// `LoadGlobal` atas global yang bukan fungsi native (`lambat::muat_global`).
    MuatGlobal,
    /// `StoreGlobal` (`lambat::simpan_global`); puncaknya tetap.
    SimpanGlobal,
    /// `GetIndex` atas wadah milik VM (`lambat::indeks`), hasilnya di tempat wadahnya.
    Indeks,
    /// `GetField`/`GetFieldPrivat` atas nilai milik VM (`lambat::field`), di tempat.
    Field,
    /// `Call` ke fungsi native-layak dengan argumen bukan bulat: bingkai VM callee didorong
    /// (`lambat::masuk`), kode `Rencana::langsung[i]`-nya dipanggil langsung, lalu `lambat::keluar`.
    Langsung(usize),
    /// `Call` atas fungsi bawaan yang diketahui (`Jenis::Bawaan`): `lambat::bawaan` memanggilnya
    /// langsung dengan argumen dari kode native, tanpa langkah VM.
    Bawaan,
    /// Aritmetika/banding yang bukan dua angka native (`lambat::biner`, `Vm::op_biner`): hasil aritmetika
    /// milik VM, hasil banding bool asli.
    Biner,
    /// `LoadUpval` (`lambat::muat_upval`): isi sel tangkapan, jenisnya bisa dispekulasikan.
    MuatUpval,
    /// `StoreUpval` (`lambat::simpan_upval`); puncaknya tetap.
    SimpanUpval,
    /// `Dup` atas nilai milik VM (`lambat::salin`).
    Salin,
    /// `Swap` dengan posisi milik VM (`lambat::tukar`/`pindah`).
    Tukar,
    /// `GetMethod` lewat tembolok umpan: label penerima dijaga (`lambat::cek_metode`, label
    /// `Rencana::metode[i]`), hasilnya method pemakai yang sudah diketahui (`Jenis::Fungsi`).
    Metode(usize),
    /// `LoadLocal` atas lokal `Nilai` (`lambat::muat`).
    Muat,
    /// `Const` bukan angka dan `Unit` (`lambat::taruh`).
    Konstanta,
    /// `AmbilLokal` atas lokal `Nilai` (`lambat::ambil`).
    Ambil,
    /// `StoreLocal`/`SimpanBuang` yang melibatkan nilai milik VM (`lambat::simpan`).
    Simpan,
    /// `CekEnum` atas `Nilai` (`lambat::pola`), hasilnya bool asli.
    Pola,
    /// `AmbilPayload` atas `Nilai` (`lambat::payload`), di tempat.
    Payload,
    /// Truthiness `Nilai` di puncak (`lambat::benar`), untuk lompat bersyarat dan `KeBool`.
    Benar,
    /// `Pop` atas `Nilai`: nilainya dilepas (`lambat::buang`).
    Buang,
    /// `Return` atas `Nilai` (masuk dari VM): VM mengambilnya dari stack-nya.
    KembaliNilai,
    /// `ReturnNil` (masuk dari VM).
    KembaliUnit,
    /// `AmbilPayload; Typeof; Const "<nama>"; Eq` atas salinan maya lokal ini (`lambat::payload_jenis_sama`):
    /// bool asli, lanjut di `ip + 4`; bukan enum berpayload = deopt.
    PayloadSama(u32, usize),
    /// `AmbilPayload; Len` atas salinan maya lokal ini (`lambat::payload_panjang`): bulat asli, lanjut di
    /// `ip + 2`; gagal = deopt.
    PayloadPanjang(u32),
}

impl Cara {
    /// Banyaknya instruksi yang dijalankan cara ini sekaligus (yang dilewati tidak punya keadaan).
    pub fn lebar(&self) -> usize {
        match self {
            Cara::TipeLipat(_) | Cara::TipeSama(_) => 3,
            Cara::PayloadSama(..) => 4,
            Cara::PayloadPanjang(_) => 2,
            _ => 1,
        }
    }
}

pub const KEMBALI_TIDAK: u8 = 0;
pub const KEMBALI_BULAT: u8 = 1;
pub const KEMBALI_SKALAR: u8 = 2;

/// Jenis nilai VM di titik masuk.
pub fn jenis_nilai(v: &VmValue) -> Jenis {
    match v {
        VmValue::Int(n) => if i64::try_from(*n).is_ok() { Jenis::Bulat } else { Jenis::Lebar },
        VmValue::Bool(_) => Jenis::Bool,
        VmValue::Float(_) => Jenis::Pecahan,
        VmValue::Nil => Jenis::Kosong,
        VmValue::Unit => Jenis::Satuan,
        _ => Jenis::Nilai,
    }
}

#[derive(Debug)]
pub struct Rencana {
    /// Instruksi tempat kode native mulai (0 = panggilan, selain itu kepala loop lewat OSR).
    pub masuk: usize,
    pub jenis_masuk: Vec<Jenis>,
    /// `Return` native: `KEMBALI_TIDAK` (OSR; VM yang mengembalikan), `KEMBALI_BULAT` (callee native),
    /// `KEMBALI_SKALAR` (masuk dari VM: bulat, bool atau pecahan).
    pub kembali: u8,
    /// Stack abstrak SEBELUM tiap instruksi; `None` = tak terjangkau.
    pub keadaan: Vec<Option<Vec<Jenis>>>,
    /// Instruksi yang dijalankan VM: kode native deopt tepat sebelumnya.
    pub keluar: Vec<bool>,
    /// Fungsi yang dipanggil (dirinya sendiri juga, kalau rekursif).
    pub callee: Vec<Arc<VmFunction>>,
    /// Instruksi yang memulai blok dasar.
    pub pemimpin: Vec<bool>,
    /// Kedalaman stack terbesar (jumlah variabel yang dibutuhkan).
    pub maks: usize,
    /// Nilai `Const` bulat (atau bit pecahan) per instruksi.
    pub konstanta: Vec<Option<i64>>,
    /// Nilai `Const` bulat di luar 64 bit (`Jenis::Lebar`) per instruksi.
    pub konstanta128: Vec<Option<i128>>,
    /// Cara tiap instruksi yang bukan titik keluar dijalankan.
    pub cara: Vec<Cara>,
    /// `callee` sebagai nilai VM: posisi `Fungsi` yang dibaca langkah VM ditaruh dari sini.
    pub callee_nilai: Vec<VmValue>,
    /// Fungsinya punya `defer`: kembali selalu lewat VM, yang menjalankan `defer`-nya.
    pub ada_defer: bool,
    /// Callee panggilan langsung (`Cara::Langsung`): nomor `callee` dan jenis argumennya.
    pub langsung: Vec<(usize, Vec<Jenis>)>,
    /// Jumlah argumen tiap instruksi `Call` (yang lain `None`).
    pub argc_call: Vec<Option<usize>>,
    /// Label tipe yang dijaga tiap `Cara::Metode`.
    pub metode: Vec<String>,
    /// Keadaan yang dibawa tiap tepi `(dari, ke)`; yang lebih sempit dari `keadaan[ke]` (titik temu yang
    /// dilebarkan) diubah dulu di tepinya (`turun::tepi`).
    pub tepi: std::collections::HashMap<(usize, usize), Vec<Jenis>>,
    /// Fungsi (alamat `Arc`) yang tidak bisa jadi callee bulat: dipanggil langsung atau lewat VM.
    pub larang_bulat: Vec<usize>,
    /// Fungsi yang tidak dikompilasi sama sekali (callee yang analisisnya gagal): lewat VM.
    pub larang: Vec<usize>,
    /// Hasil instruksi yang dispekulasikan angka menurut umpan profilnya (`spek_hasil`): sesudah
    /// pembantunya, puncak dibuka berjaga; gagal = deopt di instruksi berikutnya (`PUNCAK_VM`).
    pub spek: Vec<Option<Jenis>>,
    /// Posisi maya yang diwujudkan sebelum instruksi `ip`: `Salinan(l)` lewat `lambat::muat`, `Konst(i)` lewat
    /// `lambat::taruh` dari konstantanya.
    pub wujud: Vec<Vec<(usize, Jenis)>>,
}

impl Rencana {
    /// Nomor `callee` yang dipanggil native dengan argumen bulat (`Call` asli), urut.
    pub fn callee_asli(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.cara.len())
            .filter(|&ip| self.keadaan[ip].is_some() && !self.keluar[ip] && matches!(self.cara[ip], Cara::Asli | Cara::Buka(_)))
            .filter_map(|ip| {
                let st = self.keadaan[ip].as_ref()?;
                let a = self.argc_call[ip]?;
                match st.get(st.len().checked_sub(a + 1)?) { Some(Jenis::Fungsi(i)) => Some(*i), _ => None }
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Jenis hasil instruksi `ip` yang selalu sama menurut umpan VM (`Vm::catat_hasil`), kecuali spekulasinya
/// pernah gagal di sana.
fn spek_hasil(f: &VmFunction, ip: usize) -> Option<Jenis> {
    use super::profil::bit;
    // Bulat yang pernah terlihat (atau terbukti lewat deopt) di luar 64 bit dispekulasikan `Lebar`.
    let b = f.chunk.profil.as_ref()?.umpan.get(ip)?.load(std::sync::atomic::Ordering::Relaxed);
    let bulat = b & !(bit::BULAT | bit::BESAR) == 0 && b != 0;
    match (f.chunk.kode_jit.spek_tingkat(ip), b) {
        (0, bit::BULAT) => Some(Jenis::Bulat),
        // Payload di lengan `match` yang belum dijalankan VM sebelum kompilasi: bulat dicoba sekali.
        (0, 0) if f.chunk.code[ip].op == OpCode::AmbilPayload => Some(Jenis::Bulat),
        (0 | 1, _) if bulat => Some(Jenis::Lebar),
        (0, bit::PECAHAN) => Some(Jenis::Pecahan),
        _ => None,
    }
}

/// Callee `Cara::Langsung` yang bisa jalan tanpa bingkai VM didorong dulu (`lambat::masuk_ringan`): pembantu
/// basis-relatif atau native saja, kegagalannya deopt (`keluar_ringan` mewujudkan bingkainya). Yang membaca
/// bingkai teratas (langkah VM, bawaan, operator VM, panggilan, upvalue, field privat, `char_at` atas nilai VM) tidak.
pub fn ringan(f: &VmFunction, r: &Rencana) -> bool {
    if r.ada_defer || r.masuk != 0 { return false; }
    let kode = &f.chunk.code;
    (0..kode.len()).all(|ip| {
        let Some(s) = &r.keadaan[ip] else { return true };
        if r.keluar[ip] { return true; }
        match &r.cara[ip] {
            Cara::Asli | Cara::Buka(_) if kode[ip].op == OpCode::Call => {
                let a = kode[ip].arg as usize;
                a < s.len() && match s[s.len() - a - 1] {
                    Jenis::Fungsi(_) => true,
                    // `char_at` atas teks lokal cuma `karakter_lokal` dan deopt; atas nilai VM lewat `bawaan`.
                    Jenis::Bungkus(k, _) => k != BUNGKUS_CHAR_AT || matches!(s[s.len() - 2], Jenis::Salinan(_)),
                    _ => false,
                }
            }
            Cara::Field => kode[ip].op == OpCode::GetField,
            Cara::Asli | Cara::Buka(_) | Cara::TipeLipat(_) | Cara::TipeSama(_) | Cara::Panjang | Cara::PanjangLokal(_)
            | Cara::PayloadSama(..) | Cara::PayloadPanjang(_) | Cara::Salinan
            | Cara::Bangun(_) | Cara::Geser | Cara::Indeks | Cara::Salin | Cara::Tukar | Cara::Muat
            | Cara::Konstanta | Cara::Ambil | Cara::Simpan | Cara::Pola | Cara::Payload | Cara::Benar
            | Cara::Buang | Cara::KembaliNilai | Cara::KembaliUnit => true,
            _ => false,
        }
    })
}

/// Sifat fungsi yang membuatnya bisa dipanggil langsung dari kode native (bingkainya disusun ulang
/// `deopt::pulihkan` tanpa closure dan dengan argumen tepat sebanyak aritasnya).
pub fn bentuk_layak(f: &VmFunction) -> bool {
    !f.is_closure && !f.variadic && f.arity_min == f.arity && f.upvals.is_empty()
        && f.chunk.tipe_self.is_none()
}

/// Rencana dari `masuk` dengan stack `jenis_masuk`, atau alasan tidak ada yang bisa dikompilasi. Posisi yang
/// kembali ke titik masuk sebagai nilai VM diulang sebagai `Nilai` sejak masuk. `langsung`: callee
/// `Cara::Langsung`, yang tidak membayar pintu masuk dari VM.
#[allow(clippy::too_many_arguments)]
pub fn analisis(
    f: &VmFunction, cari: &dyn Fn(&str) -> Option<Arc<VmFunction>>,
    masuk: usize, mut jenis_masuk: Vec<Jenis>, kembali: u8, langsung: bool, larang_bulat: &[usize], larang: &[usize],
) -> Result<Rencana, String> {
    for _ in 0..4 {
        let mut lebar = None;
        let r = analisis_sekali(f, cari, masuk, jenis_masuk.clone(), kembali, langsung, larang_bulat, larang, &mut lebar);
        match lebar {
            Some(j) if j != jenis_masuk => jenis_masuk = j,
            _ => return r,
        }
    }
    analisis_sekali(f, cari, masuk, jenis_masuk, kembali, langsung, larang_bulat, larang, &mut None)
}

/// Keadaan titik temu yang menampung `a` dan `b`: `Bulat` bertemu `Lebar` jadi `Lebar` (tepi cuma
/// memperluas register), posisi lain yang beda jadi `Nilai` (isinya di stack VM).
fn gabung(a: &[Jenis], b: &[Jenis], boleh_lebar: bool) -> Option<Vec<Jenis>> {
    if a.len() != b.len() { return None; }
    Some(a.iter().zip(b).map(|(x, y)| match (x, y) {
        _ if x == y => *x,
        (Jenis::Bulat | Jenis::Lebar, Jenis::Bulat | Jenis::Lebar) if boleh_lebar => Jenis::Lebar,
        _ => Jenis::Nilai,
    }).collect())
}

/// Jenis masuk yang diperlebar supaya `datang` bertemu dengannya: posisi yang beda jadi `Nilai`.
fn perlebar(masuk: &[Jenis], datang: &[Jenis]) -> Option<Vec<Jenis>> {
    let ok = |j: &Jenis| matches!(j, Jenis::Bulat | Jenis::Bool | Jenis::Pecahan | Jenis::Kosong | Jenis::Satuan | Jenis::Nilai | Jenis::Lebar);
    if masuk.len() != datang.len() || !masuk.iter().chain(datang).all(ok) { return None; }
    Some(masuk.iter().zip(datang).map(|(a, b)| match (a, b) {
        _ if a == b => *a,
        (Jenis::Bulat | Jenis::Lebar, Jenis::Bulat | Jenis::Lebar) => Jenis::Lebar,
        _ => Jenis::Nilai,
    }).collect())
}

#[allow(clippy::too_many_arguments)]
fn analisis_sekali(
    f: &VmFunction, cari: &dyn Fn(&str) -> Option<Arc<VmFunction>>,
    masuk: usize, jenis_masuk: Vec<Jenis>, kembali: u8, langsung: bool, larang_bulat: &[usize],
    larang: &[usize], lebar: &mut Option<Vec<Jenis>>,
) -> Result<Rencana, String> {
    let kode = &f.chunk.code;
    let n = kode.len();
    if masuk >= n { return Err("kode kosong".into()); }
    let mut r = Rencana {
        masuk, jenis_masuk: jenis_masuk.clone(), kembali,
        keadaan: vec![None; n], keluar: vec![false; n], callee: Vec::new(),
        pemimpin: vec![false; n], maks: jenis_masuk.len(), konstanta: vec![None; n], konstanta128: vec![None; n],
        cara: vec![Cara::Asli; n], callee_nilai: Vec::new(), wujud: vec![Vec::new(); n],
        ada_defer: kode.iter().any(|i| i.op == OpCode::DeferPush),
        langsung: Vec::new(),
        argc_call: kode.iter().map(|i| (i.op == OpCode::Call).then_some(i.arg as usize)).collect(),
        larang_bulat: larang_bulat.to_vec(),
        larang: larang.to_vec(),
        spek: vec![None; n],
        metode: Vec::new(),
        tepi: std::collections::HashMap::new(),
    };
    r.keadaan[masuk] = Some(jenis_masuk);
    let mut kerja = vec![masuk];
    // Titik temu yang dilebarkan selalu awal blok: tepi jatuh-terus ke sana juga lewat `turun::tepi`.
    let mut dilebarkan = vec![false; n];
    while let Some(ip) = kerja.pop() {
        let s = r.keadaan[ip].clone().expect("diisi saat masuk antrean");
        // Penerus yang keadaannya bentrok dengan kedatangan sebelumnya: instruksi ini dijalankan VM.
        let lanjut = langkah(f, cari, &mut r, ip, s);
        // Pemanggil native mengisi posisi masuk fungsi bulat dari registernya, bukan stack VM.
        if let (Some(ps), false) = (&lanjut, kembali == KEMBALI_BULAT) {
            for (t, st) in ps {
                if *t != masuk || r.keadaan[masuk].as_ref() == Some(st) { continue; }
                let dasar = lebar.as_ref().unwrap_or(&r.jenis_masuk);
                if let Some(j) = perlebar(dasar, st) { *lebar = Some(j); }
            }
        }
        // Kedatangan yang bentrok di titik temu (bukan titik masuk) melebarkan posisinya jadi `Nilai` dan
        // sasarannya dianalisis ulang; tepi yang membawa angka native ke sana menulisnya ke stack VM dulu.
        r.keluar[ip] = false;
        let mut lebarkan = Vec::new();
        let cocok = lanjut.as_ref().is_some_and(|ps| ps.iter().all(|(t, st)| *t < n && match &r.keadaan[*t] {
            None => true,
            Some(a) if a == st => true,
            Some(_) if *t == masuk || kembali == KEMBALI_BULAT => false,
            Some(a) => match gabung(a, st, kembali != KEMBALI_BULAT) {
                Some(w) => { lebarkan.push((*t, w)); true }
                None => false,
            },
        }));
        let (true, Some(ps)) = (cocok, lanjut) else { r.keluar[ip] = true; continue };
        for (t, st) in ps {
            r.maks = r.maks.max(st.len());
            r.tepi.insert((ip, t), st.clone());
            if r.keadaan[t].is_none() {
                r.keadaan[t] = Some(st);
                kerja.push(t);
            }
        }
        for (t, w) in lebarkan {
            dilebarkan[t] = true;
            if r.keadaan[t].as_ref() != Some(&w) {
                r.keadaan[t] = Some(w);
                kerja.push(t);
            }
        }
    }
    // Instruksi yang mewujudkan salinan maya bekerja (dan deopt) atas posisi yang sudah milik VM.
    for ip in 0..n {
        if r.wujud[ip].is_empty() { continue; }
        if let Some(st) = r.keadaan[ip].as_mut() {
            for &(p, _) in &r.wujud[ip] { st[p] = Jenis::Nilai; }
        }
    }
    if r.keluar[masuk] {
        return Err(format!("{:?} di ip {masuk} belum diturunkan", kode[masuk].op));
    }
    // Kode yang pasti keluar sebelum mengulang loop atau kembali cuma menambah ongkos masuk-keluar.
    let berguna = (0..n).any(|ip| r.keadaan[ip].is_some() && !r.keluar[ip] && match kode[ip].op {
        OpCode::Jump => (kode[ip].arg as usize) <= ip,
        OpCode::Return | OpCode::ReturnNil => true,
        _ => false,
    });
    if !berguna {
        let ip = (0..n).find(|&i| r.keluar[i]).unwrap_or(masuk);
        return Err(format!("keluar di {:?} (ip {ip}) sebelum loop atau kembali", kode[ip].op));
    }
    // Satu langkah VM dari kode native berongkos ±10 instruksi VM biasa (masuk loop VM, menyalin
    // operand); kode yang kebanyakan langkah lebih lambat dari VM sendiri.
    let jalan = |ip: usize| r.keadaan[ip].is_some() && !r.keluar[ip];
    let asli = (0..n).filter(|&ip| jalan(ip) && matches!(r.cara[ip], Cara::Asli | Cara::Buka(_) | Cara::TipeLipat(_)
        | Cara::TipeSama(_) | Cara::Panjang | Cara::PanjangLokal(_) | Cara::PayloadSama(..) | Cara::PayloadPanjang(_))).count();
    // Pintu masuk bingkai callee (`Call` ke fungsi VM) dibayar VM juga; yang ditambah cuma langkahnya.
    let langkah_vm = (0..n).filter(|&ip| jalan(ip) && matches!(r.cara[ip], Cara::Langkah { .. } | Cara::Cabang { .. })).count();
    // Mode stres memaksa jalur native sebanyak mungkin, untung atau tidak.
    let stres = super::ambang() == 0;
    if std::env::var("JRG_JIT_LOG").is_ok_and(|v| v == "rencana") { super::modul::cetak_rencana(f, &r); }
    if langkah_vm * 4 > asli && !stres {
        return Err(format!("{langkah_vm} instruksi dijalankan VM, cuma {asli} native"));
    }
    // Pintu masuk dari VM (±320 instruksi mesin) baru tertutup kalau badannya cukup panjang atau berulang.
    let berulang = (0..n).any(|ip| jalan(ip) && matches!(kode[ip].op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && (kode[ip].arg as usize) <= ip);
    let dikerjakan = (0..n).filter(|&ip| jalan(ip)).count();
    // Panggilan native langsung (rekursi `fib`) membawa kerja sebanyak apa pun; badan yang murni angka
    // sudah untung walau pendek. Yang tersisa: badan pendek yang hidup dari pembantu nilai VM.
    let memanggil = (0..n).any(|ip| jalan(ip) && kode[ip].op == OpCode::Call && matches!(r.cara[ip], Cara::Asli | Cara::Langsung(_)));
    let pembantu = (0..n).any(|ip| jalan(ip) && !matches!(r.cara[ip], Cara::Asli | Cara::Buka(_)));
    if kembali == KEMBALI_SKALAR && !langsung && !berulang && !memanggil && pembantu && dikerjakan < 32 && !stres {
        return Err(format!("cuma {dikerjakan} instruksi tanpa loop, lebih murah di VM"));
    }
    // Blok dasar: titik masuk, sasaran lompat, dan instruksi yang tidak didatangi dari pendahulunya.
    r.pemimpin[masuk] = true;
    for ip in 0..n { if dilebarkan[ip] { r.pemimpin[ip] = true; } }
    for ip in 0..n {
        if r.keadaan[ip].is_none() { continue; }
        let ins = kode[ip];
        if matches!(ins.op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
            | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && !r.keluar[ip] {
            r.pemimpin[ins.arg as usize] = true;
        }
        let jatuh = ip > 0 && r.keadaan[ip - 1].is_some() && !r.keluar[ip - 1]
            && !matches!(kode[ip - 1].op, OpCode::Jump | OpCode::Return | OpCode::ReturnNil | OpCode::JumpFalse
                | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep);
        if !jatuh { r.pemimpin[ip] = true; }
    }
    for ip in 0..n {
        if let (true, Cara::Cabang { tujuan, .. }) = (jalan(ip), &r.cara[ip]) {
            for t in *tujuan { r.pemimpin[t] = true; }
        }
        if jalan(ip) && r.cara[ip].lebar() > 1 { r.pemimpin[ip + r.cara[ip].lebar()] = true; }
    }
    // `JumpFalse` dkk. bercabang ke instruksi sesudahnya juga.
    for ip in 0..n.saturating_sub(1) {
        if r.keadaan[ip].is_some() && !r.keluar[ip] && matches!(kode[ip].op,
            OpCode::JumpFalse | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) {
            r.pemimpin[ip + 1] = true;
        }
    }
    r.maks += 1;
    r.callee_nilai = r.callee.iter().map(|c| VmValue::Function(Arc::clone(c))).collect();
    Ok(r)
}

/// Penerus instruksi `ip` dengan stack sebelum `s` (dan `r.cara[ip]`); `None` = titik keluar.
fn langkah(
    f: &VmFunction, cari: &dyn Fn(&str) -> Option<Arc<VmFunction>>,
    r: &mut Rencana, ip: usize, s: Vec<Jenis>,
) -> Option<Vec<(usize, Vec<Jenis>)>> {
    let mut s = s;
    // Salinan maya yang tidak boleh melewati instruksi ini diwujudkan dulu (lalu jadi milik VM).
    r.wujud[ip].clear();
    if s.iter().any(|j| matches!(j, Jenis::Salinan(_))) && !boleh_lewat_salinan(f, ip, &s) {
        for (p, j) in s.iter_mut().enumerate() {
            if let Jenis::Salinan(_) = *j { r.wujud[ip].push((p, *j)); *j = Jenis::Nilai; }
        }
    }
    if s.iter().any(|j| matches!(j, Jenis::Konst(_))) && !boleh_lewat_konst(f, ip, &s) {
        for (p, j) in s.iter_mut().enumerate() {
            if let Jenis::Konst(_) = *j { r.wujud[ip].push((p, *j)); *j = Jenis::Nilai; }
        }
    }
    // Konstanta maya dimuat dari lokal atau disimpan ke lokal yang bukan milik VM: cuma jenisnya yang berpindah.
    {
        let ins = f.chunk.code[ip];
        let (a, t) = (ins.arg as usize, s.len());
        match ins.op {
            OpCode::LoadLocal if matches!(s.get(a), Some(Jenis::Konst(_))) => {
                s.push(s[a]);
                r.cara[ip] = Cara::Salinan;
                return Some(vec![(ip + 1, s)]);
            }
            OpCode::StoreLocal | OpCode::SimpanBuang if t >= 1 && a + 1 < t && matches!(s[t - 1], Jenis::Konst(_))
                && !s[a].di_vm() && !matches!(s[a], Jenis::Salinan(_))
                && !s.iter().any(|j| matches!(j, Jenis::Salinan(l) if *l as usize == a)) => {
                s[a] = s[t - 1];
                if ins.op == OpCode::SimpanBuang { s.pop(); }
                r.cara[ip] = Cara::Salinan;
                return Some(vec![(ip + 1, s)]);
            }
            _ => {}
        }
    }
    if let Some((b, s2)) = tipe_lipat(f, ip, &s) {
        r.cara[ip] = Cara::TipeLipat(b);
        return Some(vec![(ip + 3, s2)]);
    }
    if r.kembali != KEMBALI_BULAT {
        if let Some((cara, s2)) = payload_gabung(f, ip, &s) {
            let lanjut = ip + cara.lebar();
            r.cara[ip] = cara;
            return Some(vec![(lanjut, s2)]);
        }
        if let Some(s2) = tipe_sama(f, ip, &s) {
            r.cara[ip] = Cara::TipeSama(f.chunk.code[ip + 1].arg as usize);
            return Some(vec![(ip + 3, s2)]);
        }
    }
    // `langkah_asli` boleh memilih cara lain (`ReturnNil`).
    r.cara[ip] = Cara::Asli;
    if let Some(p) = langkah_asli(f, cari, r, ip, s.clone()) { return Some(p); }
    // Nilai milik VM cuma ada di bingkai terluar (`lambat` memakai basis bingkai itu).
    if r.kembali == KEMBALI_BULAT { return None; }
    let (cara, p) = langkah_lambat(f, cari, r, ip, s)?;
    r.cara[ip] = cara;
    Some(p)
}

/// Pemeriksaan pola varian berfield banyak atas salinan maya: `AmbilPayload` yang langsung diperiksa jenisnya
/// (`Typeof; Const; Eq`) atau panjangnya (`Len`), tanpa payload disalin ke stack VM. Yang dilewati bukan
/// sasaran lompat.
fn payload_gabung(f: &VmFunction, ip: usize, s: &[Jenis]) -> Option<(Cara, Vec<Jenis>)> {
    let kode = &f.chunk.code;
    let Jenis::Salinan(l) = *s.last()? else { return None };
    if kode[ip].op != OpCode::AmbilPayload { return None; }
    let sasaran = |k: usize| kode.iter().any(|i| matches!(i.op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && (ip + 1..=ip + k).contains(&(i.arg as usize)));
    let mut s2 = s.to_vec();
    if kode.get(ip + 1)?.op == OpCode::Len && !sasaran(1) {
        *s2.last_mut()? = Jenis::Bulat;
        return Some((Cara::PayloadPanjang(l), s2));
    }
    let (ty, c, eq) = (kode.get(ip + 1)?, kode.get(ip + 2)?, kode.get(ip + 3)?);
    if ty.op != OpCode::Typeof || c.op != OpCode::Const || eq.op != OpCode::Eq || sasaran(3)
        || !matches!(f.chunk.constants.get(c.arg as usize)?, VmValue::Str(_)) { return None; }
    *s2.last_mut()? = Jenis::Bool;
    Some((Cara::PayloadSama(l, c.arg as usize), s2))
}

/// Pemeriksaan jenis pola atas nilai milik VM (payload, field): satu pembantu, bukan tiga langkah VM.
fn tipe_sama(f: &VmFunction, ip: usize, s: &[Jenis]) -> Option<Vec<Jenis>> {
    let kode = &f.chunk.code;
    if kode[ip].op != OpCode::Typeof || kode.get(ip + 2)?.op != OpCode::Eq || kode[ip + 1].op != OpCode::Const { return None; }
    if !matches!(f.chunk.constants.get(kode[ip + 1].arg as usize)?, VmValue::Str(_)) || !s.last()?.di_vm() { return None; }
    let sasaran = |i: &crate::vm::opcode::Instruction| matches!(i.op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && matches!(i.arg as usize, x if x == ip + 1 || x == ip + 2);
    if kode.iter().any(sasaran) { return None; }
    let mut s2 = s.to_vec();
    *s2.last_mut()? = Jenis::Bool;
    Some(s2)
}

/// Jenis elemen yang dispekulasikan untuk baca larik milik lokal (`lambat::baca`): pecahan bila umpannya cuma
/// pecahan, selain itu bulat (yang meleset = `jaga_spek`, kompilasi berikutnya lewat VM).
fn elemen_larik(f: &VmFunction, ip: usize) -> Jenis {
    if spek_hasil(f, ip) == Some(Jenis::Pecahan) { Jenis::Pecahan } else { Jenis::Bulat }
}

/// Instruksi yang salinan maya boleh hidup melewatinya: tidak menulis lokal asalnya, tidak menjalankan kode
/// asing, dan tidak memakai salinan itu sebagai operand — kecuali yang dibaca langsung dari lokal asalnya
/// (`CekEnum`/`AmbilPayload` lewat `pola_lokal`/`payload_lokal`, `GetIndex` berindeks bulat, `str::char_at`).
fn boleh_lewat_salinan(f: &VmFunction, ip: usize, s: &[Jenis]) -> bool {
    let t = s.len();
    let salinan = |j: &Jenis| matches!(j, Jenis::Salinan(_));
    let bebas = |k: usize| k <= t && !s[t - k..].iter().any(salinan);
    let ins = &f.chunk.code[ip];
    let a = ins.arg as usize;
    match ins.op {
        OpCode::LoadLocal | OpCode::LokalHitung | OpCode::IntImm | OpCode::Const | OpCode::Jump | OpCode::Pop
        | OpCode::SlideDown | OpCode::ReturnNil | OpCode::CekEnum | OpCode::AmbilPayload => true,
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Lt | OpCode::Le | OpCode::Gt
        | OpCode::Ge | OpCode::Eq | OpCode::Ne | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot
        | OpCode::Shl | OpCode::Shr => bebas(2),
        OpCode::Neg | OpCode::Not | OpCode::BitNot | OpCode::KeBool | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep | OpCode::Return => bebas(1),
        // Yang ditulis bukan salinan, sasarannya bukan posisi maya dan bukan asal salinan mana pun.
        OpCode::StoreLocal | OpCode::SimpanBuang => bebas(1) && a < t && !salinan(&s[a])
            && !s.iter().any(|j| matches!(j, Jenis::Salinan(l) if *l as usize == a)),
        OpCode::GetIndex => t >= 2 && matches!(s[t - 2], Jenis::Salinan(_)) && s[t - 1] == Jenis::Bulat
            && !f.chunk.kode_jit.spek_gagal_di(ip),
        // `str::char_at(teks, i)` atas teks lokal (`lambat::karakter_lokal`).
        OpCode::Call => (f.chunk.code[ip].arg == 2 && t >= 3 && s[t - 3].bungkus(BUNGKUS_CHAR_AT)
            && matches!(s[t - 2], Jenis::Salinan(_)) && s[t - 1] == Jenis::Bulat)
            || (f.chunk.code[ip].arg == 1 && t >= 2 && bawaan_len(f, s[t - 2]) && salinan(&s[t - 1])),
        _ => false,
    }
}

/// Instruksi yang konstanta maya (`Jenis::Konst`) boleh hidup melewatinya: tidak membacanya sebagai operand dan
/// tidak membaca lokal lewat stack VM (selain yang dibaca `LoadLocal`/`StoreLocal` di atas, yang memindah jenisnya).
fn boleh_lewat_konst(f: &VmFunction, ip: usize, s: &[Jenis]) -> bool {
    let t = s.len();
    let konst = |j: &Jenis| matches!(j, Jenis::Konst(_));
    let bebas = |k: usize| k <= t && !s[t - k..].iter().any(konst);
    let ins = f.chunk.code[ip];
    let a = ins.arg as usize;
    match ins.op {
        OpCode::LoadLocal | OpCode::LokalHitung | OpCode::IntImm | OpCode::Const | OpCode::Jump | OpCode::Pop
        | OpCode::Nil | OpCode::True | OpCode::False | OpCode::Unit => true,
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Lt | OpCode::Le | OpCode::Gt
        | OpCode::Ge | OpCode::Eq | OpCode::Ne | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot
        | OpCode::Shl | OpCode::Shr => bebas(2),
        OpCode::Neg | OpCode::Not | OpCode::BitNot | OpCode::KeBool | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep | OpCode::BuildEnum => bebas(1),
        OpCode::BuildTuple | OpCode::BuildArray => bebas(a),
        // Penulisan ke lokal maya menimpanya (isinya di stack VM basi, dilepas biasa); ke lokal VM dari maya: diwujudkan.
        OpCode::StoreLocal | OpCode::SimpanBuang => a < t && (bebas(1) && (!konst(&s[a]) || s[t - 1].di_vm())
            || konst(&s[t - 1]) && !s[a].di_vm()),
        OpCode::IterAda => s.get(a..a + 3).is_some_and(|w| !w.iter().any(konst)),
        OpCode::IterMaju => s.get(a).is_some_and(|j| !konst(j)),
        OpCode::IterVar => s.get(a / 2..a / 2 + 3).is_some_and(|w| !w.iter().any(konst)),
        _ => false,
    }
}

/// Fungsi bawaan maya `len`.
fn bawaan_len(f: &VmFunction, j: Jenis) -> bool {
    let Jenis::Bawaan(i) = j else { return false };
    matches!(f.chunk.constants.get(i as usize), Some(VmValue::NativeFn(g))
        if crate::vm::info_native::info(*g).is_some_and(|n| n.kanonik == "len"))
}

/// `LoadLocal` di `ip` tepat di atas callee `len` (`atas`) dan tepat sebelum `Call 1` yang bukan sasaran lompat:
/// panjangnya dibaca dari lokal asalnya (`Cara::PanjangLokal`).
fn salinan_ke_len(f: &VmFunction, ip: usize, atas: Option<&Jenis>) -> bool {
    let kode = &f.chunk.code;
    atas.is_some_and(|j| bawaan_len(f, *j))
        && kode.get(ip + 1).is_some_and(|i| i.op == OpCode::Call && i.arg == 1)
        && !kode.iter().any(|i| matches!(i.op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
            | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && i.arg as usize == ip + 1)
}

/// `LoadLocal` di `ip` yang hasilnya (posisi `p`) langsung dibaca lagi sebagai lokal — pemeriksa `match`:
/// salinan maya sampai ada yang perlu isinya di stack VM (`boleh_lewat_salinan`).
fn salinan_sementara(f: &VmFunction, ip: usize, p: usize) -> bool {
    f.chunk.code.get(ip + 1).is_some_and(|i| i.op == OpCode::LoadLocal && i.arg as usize == p)
}

/// Salinan yang instruksi sesudahnya baca langsung dari lokal asalnya (`pola_lokal`/`payload_lokal`).
fn salinan_dibaca_langsung(f: &VmFunction, ip: usize) -> bool {
    f.chunk.code.get(ip + 1).is_some_and(|i| matches!(i.op, OpCode::CekEnum | OpCode::AmbilPayload))
}

/// `LoadLocal` di `ip` yang hasilnya cuma dipakai sebagai wadah `GetIndex` (atau teks `str::char_at`, kalau
/// `char_at` di bawahnya) sesudah deret instruksi yang boleh dilewati salinan (tanpa sasaran lompat di
/// dalamnya): jadi `Jenis::Salinan`.
fn salinan_ke_indeks(f: &VmFunction, ip: usize, char_at: bool) -> bool {
    let kode = &f.chunk.code;
    let sasaran = |k: usize| kode.iter().any(|i| matches!(i.op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && i.arg as usize == k);
    // `tinggi`: posisi di atas salinan ini; operand sebuah biner = dua teratas.
    let mut tinggi = 0usize;
    for k in ip + 1..(ip + 16).min(kode.len()) {
        if sasaran(k) { return false; }
        match kode[k].op {
            OpCode::LoadLocal | OpCode::LokalHitung | OpCode::IntImm | OpCode::Const => tinggi += 1,
            OpCode::Add | OpCode::Sub | OpCode::Mul if tinggi >= 2 => tinggi -= 1,
            OpCode::GetIndex => return tinggi == 1,
            OpCode::Call if char_at && kode[k].arg == 2 => return tinggi == 1,
            _ => return false,
        }
    }
    false
}

/// Pemeriksaan jenis pola (`emit_pattern_type_check`) atas angka/bool native: jawabannya sudah pasti. Yang
/// dilewati tidak boleh jadi sasaran lompat.
fn tipe_lipat(f: &VmFunction, ip: usize, s: &[Jenis]) -> Option<(bool, Vec<Jenis>)> {
    let kode = &f.chunk.code;
    if kode[ip].op != OpCode::Typeof || kode.get(ip + 2)?.op != OpCode::Eq || kode[ip + 1].op != OpCode::Const { return None; }
    let VmValue::Str(nama) = f.chunk.constants.get(kode[ip + 1].arg as usize)? else { return None };
    let contoh = match s.last()? {
        Jenis::Bulat => VmValue::Int(0),
        Jenis::Pecahan => VmValue::Float(0.0),
        Jenis::Bool => VmValue::Bool(false),
        _ => return None,
    };
    let sasaran = |i: &crate::vm::opcode::Instruction| matches!(i.op, OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
        | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep) && matches!(i.arg as usize, x if x == ip + 1 || x == ip + 2);
    if kode.iter().any(sasaran) { return None; }
    let mut s2 = s.to_vec();
    *s2.last_mut()? = Jenis::Bool;
    Some((contoh.type_name() == nama.as_str(), s2))
}

/// Jumlah operand teratas yang dibaca instruksi yang boleh dibuka (`Cara::Buka`).
fn operand_angka(op: OpCode, arg: usize) -> usize {
    match op {
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Lt | OpCode::Le
        | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne | OpCode::MakeIterRentang
        | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot | OpCode::Shl | OpCode::Shr => 2,
        OpCode::Neg | OpCode::IndeksLokal | OpCode::BitNot => 1,
        // Cuma indeksnya; nilai yang ditulis boleh apa saja (`Cara::Langkah`).
        OpCode::TulisIndexLokal => 1,
        OpCode::Call => arg,
        _ => 0,
    }
}

/// Bentuk lambat: membuka `Nilai` jadi angka, atau menyerahkan instruksinya ke VM.
fn langkah_lambat(
    f: &VmFunction, cari: &dyn Fn(&str) -> Option<Arc<VmFunction>>,
    r: &mut Rencana, ip: usize, mut s: Vec<Jenis>,
) -> Option<(Cara, Vec<(usize, Vec<Jenis>)>)> {
    use Jenis::*;
    let kode = &f.chunk.code;
    let ins = kode[ip];
    let a = ins.arg as usize;
    let t = s.len();
    let lanjut = ip + 1;
    // Operand `Nilai` di samping angka dibuka ke jenis angkanya (pecahan kalau ada pecahan).
    let k = operand_angka(ins.op, a).min(t);
    let atas = &s[t - k..];
    let indeks = matches!(ins.op, OpCode::IndeksLokal | OpCode::TulisIndexLokal);
    // Spekulasi yang pernah gagal di sini (`PintuJit::tandai_spek_gagal`) dicoba sekali lagi sebagai `Lebar`
    // (operasi bit, banding, intrinsik `wrap_*`), lalu tidak lagi. Penerima method tidak pernah bulat.
    let metode_dipanggil = ins.op == OpCode::Call && a < t
        && matches!(s[t - a - 1], Fungsi(i) if r.callee[i].chunk.tipe_self.is_some());
    let tingkat = f.chunk.kode_jit.spek_tingkat(ip);
    let lebar_ok = r.kembali != KEMBALI_BULAT && match ins.op {
        OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot | OpCode::Shl | OpCode::Shr
        | OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::BitNot => true,
        OpCode::Call => a < t && s[t - a - 1].wrap(),
        _ => false,
    };
    let boleh = (tingkat == 0 || (tingkat == 1 && lebar_ok)) && !metode_dipanggil;
    if boleh && atas.contains(&Nilai) && (indeks || atas.iter().any(|j| matches!(j, Bulat | Pecahan | Lebar))) {
        let sasaran = if tingkat == 1 { Lebar } else if atas.contains(&Pecahan) && ins.op != OpCode::Call { Pecahan } else { Bulat };
        let buka: Vec<(usize, Jenis)> = (t - k..t).filter(|&i| s[i] == Nilai).map(|i| (i, sasaran)).collect();
        let mut s2 = s.clone();
        for (i, j) in &buka { s2[*i] = *j; }
        if let Some(p) = langkah_asli(f, cari, r, ip, s2) { return Some((Cara::Buka(buka), p)); }
    }
    // Bingkai tanpa posisi apa pun (`SelectEnd` di `main` kosong) tetap boleh dilangkahkan.
    let puncak = s.last().copied().unwrap_or(Kosong);
    match ins.op {
        OpCode::LoadLocal if s.get(a)?.di_vm()
            && (salinan_ke_indeks(f, ip, s.last().is_some_and(|j| j.bungkus(BUNGKUS_CHAR_AT))) || salinan_sementara(f, ip, t)
                || salinan_ke_len(f, ip, s.last())) => {
            s.push(Salinan(a as u32));
            return Some((Cara::Salinan, vec![(lanjut, s)]));
        }
        // Salinan dari salinan maya: maya juga kalau pembacanya membaca lokal asalnya; selain itu salinan
        // sungguhan dari lokal asalnya (`Cara::Muat`, `turun` membaca lokal asalnya).
        OpCode::LoadLocal if matches!(s.get(a), Some(Salinan(_))) => {
            let j = s[a];
            let maya = salinan_dibaca_langsung(f, ip)
                || salinan_ke_indeks(f, ip, s.last().is_some_and(|k| k.bungkus(BUNGKUS_CHAR_AT)));
            s.push(if maya { j } else { Nilai });
            return Some((if maya { Cara::Salinan } else { Cara::Muat }, vec![(lanjut, s)]));
        }
        OpCode::LoadLocal | OpCode::LokalHitung if s.get(a)?.di_vm() => {
            s.push(Nilai);
            return Some((Cara::Muat, vec![(lanjut, s)]));
        }
        OpCode::Pop if puncak.di_vm() => {
            s.pop();
            return Some((Cara::Buang, vec![(lanjut, s)]));
        }
        OpCode::KeBool if puncak.di_vm() => {
            s.pop();
            s.push(Bool);
            return Some((Cara::Benar, vec![(lanjut, s)]));
        }
        OpCode::JumpFalse | OpCode::JumpTrue if puncak.di_vm() => {
            s.pop();
            return Some((Cara::Benar, vec![(a, s.clone()), (lanjut, s)]));
        }
        OpCode::JumpFalseKeep | OpCode::JumpTrueKeep if puncak.di_vm() => {
            return Some((Cara::Benar, vec![(a, s.clone()), (lanjut, s)]));
        }
        OpCode::Return if r.kembali == KEMBALI_SKALAR && (puncak.di_vm() || puncak == Lebar) && !r.ada_defer => return Some((Cara::KembaliNilai, vec![])),
        OpCode::Dup if puncak.di_vm() => {
            s.push(Nilai);
            return Some((Cara::Salin, vec![(lanjut, s)]));
        }
        OpCode::Swap if t >= 2 && (s[t - 1].di_vm() || s[t - 2].di_vm()) => {
            s.swap(t - 1, t - 2);
            return Some((Cara::Tukar, vec![(lanjut, s)]));
        }
        // Aritas `ARGC_NANTI` diperiksa `Call` (yang salah jatuh ke langkah VM, yang melaporkannya).
        OpCode::GetMethod if puncak.di_vm() && !f.chunk.kode_jit.spek_gagal_di(ip) => {
            let nanti = (a >> 24) as u32 == crate::vm::opcode::ARGC_NANTI;
            if let Some((label, g)) = f.chunk.kode_jit.metode_di(ip) {
                if !g.is_closure && !g.variadic && g.arity_min == g.arity && g.upvals.is_empty()
                    && !r.larang.contains(&(Arc::as_ptr(&g) as usize))
                    && (nanti || g.arity as usize == (a >> 24) + 1) {
                    let i = match r.callee.iter().position(|c| Arc::ptr_eq(c, &g)) {
                        Some(i) => i,
                        None => { r.callee.push(g); r.callee.len() - 1 }
                    };
                    r.metode.push(label);
                    *s.last_mut()? = Fungsi(i);
                    return Some((Cara::Metode(r.metode.len() - 1), vec![(lanjut, s)]));
                }
            }
        }
        OpCode::SlideDown if a < t => {
            let atas = s.pop()?;
            s.truncate(t - a - 1);
            s.push(atas);
            return Some((Cara::Geser, vec![(lanjut, s)]));
        }
        OpCode::LoadUpval => {
            r.spek[ip] = spek_hasil(f, ip);
            s.push(r.spek[ip].unwrap_or(Nilai));
            return Some((Cara::MuatUpval, vec![(lanjut, s)]));
        }
        OpCode::StoreUpval if t >= 1 && !puncak.tetap() => return Some((Cara::SimpanUpval, vec![(lanjut, s)])),
        OpCode::LoadGlobal => {
            r.spek[ip] = spek_hasil(f, ip);
            s.push(r.spek[ip].unwrap_or(Nilai));
            return Some((Cara::MuatGlobal, vec![(lanjut, s)]));
        }
        OpCode::StoreGlobal if !puncak.tetap() => return Some((Cara::SimpanGlobal, vec![(lanjut, s)])),
        OpCode::GetIndex if t >= 2 && s[t - 2].di_vm() && !puncak.tetap() => {
            s.pop();
            r.spek[ip] = spek_hasil(f, ip);
            *s.last_mut()? = r.spek[ip].unwrap_or(Nilai);
            return Some((Cara::Indeks, vec![(lanjut, s)]));
        }
        OpCode::GetField | OpCode::GetFieldPrivat if puncak.di_vm() => {
            r.spek[ip] = spek_hasil(f, ip);
            *s.last_mut()? = r.spek[ip].unwrap_or(Nilai);
            return Some((Cara::Field, vec![(lanjut, s)]));
        }
        OpCode::Propagate if puncak.di_vm() => {
            *s.last_mut()? = Nilai;
            return Some((Cara::Sebar, vec![(lanjut, s)]));
        }
        // `[kini, kanan]` disambung di tempat (`emit_tumbuh_sel`): `JumpFalse` sesudahnya ikut diputuskan di sini.
        OpCode::CobaTumbuhLokal | OpCode::CobaTumbuhUpval | OpCode::CobaTumbuhGlobal
            if t >= 2 && kode.get(ip + 1).is_some_and(|i| i.op == OpCode::JumpFalse) => {
            let gagal = kode[ip + 1].arg as usize;
            let jadi = s[..t - 2].to_vec();
            s[t - 2] = Nilai;
            s[t - 1] = Nilai;
            let c = Cara::Cabang { tumpah: 2, hasil_bool: true, tujuan: [ip + 2, gagal] };
            return Some((c, vec![(ip + 2, jadi), (gagal, s)]));
        }
        // `[kini, kanan, idx]`: tersambung = lompat `arg >> 24` sesudahnya tanpa ketiganya.
        OpCode::TumbuhIndexLokal if t >= 3 && s.get(a & 0xFF_FFFF)?.di_vm() => {
            let lewat = lanjut + (a >> 24);
            let jadi = s[..t - 3].to_vec();
            s.pop();
            s[t - 3] = Nilai;
            s[t - 2] = Nilai;
            let c = Cara::Cabang { tumpah: 3, hasil_bool: false, tujuan: [lewat, lanjut] };
            return Some((c, vec![(lewat, jadi), (lanjut, s)]));
        }
        OpCode::SelectRecvArm => {
            s.pop();
            let lewat = s.clone();
            s.push(Nilai);
            let c = Cara::Cabang { tumpah: 1, hasil_bool: false, tujuan: [lanjut, a] };
            return Some((c, vec![(lanjut, s), (a, lewat)]));
        }
        OpCode::SelectSendArm if t >= 2 => {
            s.truncate(t - 2);
            let c = Cara::Cabang { tumpah: 2, hasil_bool: false, tujuan: [lanjut, a] };
            return Some((c, vec![(lanjut, s.clone()), (a, s)]));
        }
        OpCode::SelectEnd if ins.arg != u32::MAX => {
            let c = Cara::Cabang { tumpah: 0, hasil_bool: false, tujuan: [a, a] };
            return Some((c, vec![(a, s)]));
        }
        // Atas salinan maya: dibaca langsung dari lokal asalnya (`pola_lokal`/`payload_lokal`).
        OpCode::CekEnum if puncak.di_vm() || matches!(puncak, Salinan(_)) => {
            s.pop();
            s.push(Bool);
            return Some((Cara::Pola, vec![(lanjut, s)]));
        }
        OpCode::AmbilPayload if puncak.di_vm() || matches!(puncak, Salinan(_)) => {
            r.spek[ip] = spek_hasil(f, ip);
            *s.last_mut()? = r.spek[ip].unwrap_or(Nilai);
            return Some((Cara::Payload, vec![(lanjut, s)]));
        }
        // Iterator larik/teks/dict (`MakeIter` lewat langkah): ketiga posisinya milik VM.
        OpCode::Len if puncak.di_vm() => {
            *s.last_mut()? = Bulat;
            return Some((Cara::Panjang, vec![(lanjut, s)]));
        }
        OpCode::BuildStruct | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildEnum => {
            let n = match ins.op {
                OpCode::BuildEnum => match f.chunk.constants.get(a)? {
                    VmValue::Map(p) if matches!(p.label, crate::vm::value::Label::Enum { .. }) => 1,
                    _ => return None,
                },
                OpCode::BuildStruct => match f.chunk.constants.get(a)? {
                    VmValue::Map(p) => p.bentuk()?.nama.len(),
                    _ => return None,
                },
                _ => a,
            };
            if n > t { return None; }
            s.truncate(t - n);
            s.push(Nilai);
            return Some((Cara::Bangun(n), vec![(lanjut, s)]));
        }
        OpCode::IterAda if s.get(a..a + 3)?.iter().all(|j| j.di_vm()) => {
            s.push(Bool);
            return Some((Cara::IterAda, vec![(lanjut, s)]));
        }
        OpCode::IterMaju if s.get(a)?.di_vm() => {
            return Some((Cara::Maju, vec![(lanjut, s)]));
        }
        OpCode::Const | OpCode::Unit => {
            let j = match (ins.op, f.chunk.constants.get(a)?) {
                (OpCode::Const, VmValue::NativeFn(g)) => bungkus(*g).map_or(Bawaan(a as u32), |k| Bungkus(k, a as u32)),
                (OpCode::Const, v) if v.bagian_enum().is_some_and(|(_, _, p)| p.is_none()) => Konst(a as u32),
                _ => Nilai,
            };
            s.push(j);
            // Konstanta maya tidak ditulis ke stack VM sampai ada yang membutuhkannya.
            let cara = if matches!(j, Konst(_)) { Cara::Salinan } else { Cara::Konstanta };
            return Some((cara, vec![(lanjut, s)]));
        }
        OpCode::AmbilLokal if s.get(a)?.di_vm() => {
            s[a] = Nilai;
            s.push(Nilai);
            return Some((Cara::Ambil, vec![(lanjut, s)]));
        }
        // Lokal milik VM bisa sel tangkapan: penulisannya menembus sel (`Vm::simpan_lokal`).
        OpCode::StoreLocal | OpCode::SimpanBuang
            if a + 1 < t && (s[a].di_vm() || puncak.di_vm()) && !puncak.tetap() => {
            s[a] = Nilai;
            if ins.op == OpCode::SimpanBuang { s.pop(); }
            return Some((Cara::Simpan, vec![(lanjut, s)]));
        }
        _ => {}
    }
    // Operand yang bukan dua angka native (teks, operator pemakai, campur jenis): `Vm::op_biner` langsung.
    if matches!(ins.op, OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow
        | OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::BitAnd
        | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::AndNot)
        && t >= 2 && !s[t - 2..].iter().any(|j| j.tetap()) {
        let banding = matches!(ins.op, OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge);
        s.truncate(t - 2);
        s.push(if banding { Bool } else { Nilai });
        return Some((Cara::Biner, vec![(lanjut, s)]));
    }
    // `len(lokal)`: dibaca dari lokal asalnya, tanpa menyalinnya ke stack VM.
    if ins.op == OpCode::Call && a == 1 && t >= 2 && bawaan_len(f, s[t - 2]) {
        if let Salinan(l) = s[t - 1] {
            s.truncate(t - 2);
            s.push(Bulat);
            return Some((Cara::PanjangLokal(l), vec![(lanjut, s)]));
        }
    }
    // Fungsi bawaan: tidak pernah mendorong bingkai, dipanggil langsung oleh pembantu.
    if ins.op == OpCode::Call && a < t && matches!(s[t - a - 1], Bawaan(_) | Bungkus(..)) && !s[t - a..].iter().any(|j| j.tetap()) {
        s.truncate(t - a - 1);
        r.spek[ip] = spek_hasil(f, ip);
        s.push(r.spek[ip].unwrap_or(Nilai));
        return Some((Cara::Bawaan, vec![(lanjut, s)]));
    }
    // Fungsi native-layak dengan argumen milik VM: bingkai VM-nya didorong, kodenya dipanggil langsung.
    if let (OpCode::Call, true) = (ins.op, a < t) {
        if let Fungsi(i) = s[t - a - 1] {
            if r.callee[i].arity as usize == a && !s[t - a..].iter().any(|j| j.tetap()) {
                // `Lebar` tetap: isinya sudah di stack VM (tumpahan), callee membacanya di prolog.
                // Rune tidak pernah jenis titik masuk: ditumpahkan sebagai nilai VM.
                let jenis: Vec<Jenis> = s[t - a..].iter().map(|j| if j.di_vm() || *j == Rune { Nilai } else { *j }).collect();
                let k = match r.langsung.iter().position(|(c, js)| *c == i && *js == jenis) {
                    Some(k) => k,
                    None => { r.langsung.push((i, jenis)); r.langsung.len() - 1 }
                };
                s.truncate(t - a - 1);
                r.spek[ip] = spek_hasil(f, ip);
                s.push(r.spek[ip].unwrap_or(Nilai));
                return Some((Cara::Langsung(k), vec![(lanjut, s)]));
            }
        }
    }
    // Sisanya dijalankan VM. Operand fungsi milik kode native tidak punya padanan di stack VM.
    let (ambil, taruh, lokal) = efek_langkah(f, ip, &s)?;
    if ambil > t { return None; }
    // Closure menangkap lokal bingkai ini dari stack VM (lalu menjadikannya sel): semuanya ditumpahkan dulu.
    // Iterator yang separuh native (keadaan masuk OSR) dan tempat `SamaTempat` dibaca VM dari stack-nya.
    let mut tumpah = ambil;
    match ins.op {
        OpCode::IterAda | OpCode::IterVar => tumpah = t - if ins.op == OpCode::IterAda { a } else { a / 2 },
        OpCode::SamaTempat => {
            let k = a & 0x7FFF_FFFF;
            if k + 1 >= t { return None; }
            tumpah = t - k;
            s[k] = Nilai;
            s[k + 1] = Nilai;
        }
        _ => {}
    }
    if ins.op == OpCode::MakeClosure {
        let VmValue::Function(g) = f.chunk.constants.get(a)? else { return None };
        tumpah = t;
        for u in &g.upvals {
            if let crate::vm::value::UpvalSrc::ParentLocal(l) = u { *s.get_mut(*l as usize)? = Nilai; }
        }
    }
    s.truncate(t - ambil);
    s.extend(std::iter::repeat_n(Nilai, taruh));
    if let Some(l) = lokal { *s.get_mut(l)? = Nilai; }
    Some((Cara::Langkah { ambil, taruh, tumpah }, vec![(lanjut, s)]))
}

/// Instruksi yang aman dijalankan VM satu per satu dari dalam kode native: `(ambil, taruh, lokal
/// yang ditulis)`. Tidak melompat atau bergabung dengan instruksi sesudahnya; bingkai yang didorong `Call`
/// dijalankan VM sampai kembali (`lambat::langkah`).
fn efek_langkah(f: &VmFunction, ip: usize, s: &[Jenis]) -> Option<(usize, usize, Option<usize>)> {
    let kode = &f.chunk.code;
    let ins = kode[ip];
    let a = ins.arg as usize;
    let t = s.len();
    // Bentuk native dicoba lebih dulu (`langkah_asli`); yang sampai di sini (jenis campur, `typeof` atas angka,
    // galat yang harus dilaporkan VM) dilangkahkan apa pun jenisnya: posisi native ditumpahkan dulu.
    let ada_vm = |k: usize| k <= t;
    Some(match ins.op {
        OpCode::LoadFn => (0, 1, None),
        OpCode::PinjamLokal if s.get(a)?.di_vm() => (0, 1, Some(a)),
        OpCode::Dup if ada_vm(1) => (1, 2, None),
        OpCode::Swap if ada_vm(2) => (2, 2, None),
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow
        | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::AndNot
        | OpCode::Concat if ada_vm(2) => (2, 1, None),
        // `int_cepat` VM tidak menggabungkan banding dengan `JumpFalse` di langkah tunggal.
        OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge if ada_vm(2) => (2, 1, None),
        OpCode::Neg | OpCode::Not | OpCode::BitNot | OpCode::Len | OpCode::Typeof | OpCode::CopyVal
            if ada_vm(1) => (1, 1, None),
        OpCode::GetIndex if ada_vm(2) => (2, 1, None),
        OpCode::IndeksLokal if s.get(a)?.di_vm() => (1, 1, None),
        OpCode::TulisIndexLokal if s.get(a)?.di_vm() => (2, 0, Some(a)),
        OpCode::SetIndex if ada_vm(3) => (3, 0, None),
        OpCode::Slice if ada_vm(3) => (3, 1, None),
        OpCode::GetField | OpCode::GetFieldPrivat if ada_vm(1) => (1, 1, None),
        OpCode::BuildArray | OpCode::BuildTuple => (a, 1, None),
        // Pola dan literal tipe pemakai: tidak satu pun digabung VM dengan instruksi sesudahnya.
        OpCode::CekEnum | OpCode::CekStruct | OpCode::AmbilPayload | OpCode::HasField | OpCode::BuildEnum => (1, 1, None),
        OpCode::BuildStruct => match f.chunk.constants.get(a)? {
            VmValue::Map(p) => (p.bentuk()?.nama.len(), 1, None),
            _ => return None,
        },
        OpCode::SetField | OpCode::SetFieldPrivat if ada_vm(2) => (2, 0, None),
        OpCode::StoreGlobal => (1, 1, None),
        // Fungsi bawaan tidak pernah mendorong bingkai (`Vm::call_value`, cabang `NativeFn`).
        // Callee milik VM: bawaan dijalankan di tempat, fungsi/closure/method lewat bingkai VM sampai kembali.
        OpCode::Call if a < t && (s[t - a - 1].di_vm() || s[t - a - 1].tetap()) => (a + 1, 1, None),
        OpCode::GetMethod if ada_vm(1) => (1, 1, None),
        OpCode::PeriksaArgMetode => { let n = (a >> 24) + 2; (n, n, None) }
        OpCode::LoadArgc | OpCode::LokalPolos | OpCode::LoadUpval | OpCode::PinjamUpval | OpCode::PinjamGlobal
        | OpCode::MakeClosure => (0, 1, None),
        // Yang cuma mengintip puncak (`peek`) menggantinya dengan salinan milik VM.
        OpCode::StoreUpval | OpCode::DefGlobal | OpCode::DefVar | OpCode::CekChannelSelect
        | OpCode::GagalCocok => (1, 1, None),
        OpCode::PopPeriksa | OpCode::ChanClose | OpCode::CekTutupSelect | OpCode::Panic => (1, 0, None),
        OpCode::SelectBegin | OpCode::SelectDefault => (0, 0, None),
        OpCode::ArrUbahMilik => (if matches!(a & 3, 0 | 3) { 2 } else { 1 }, 1, None),
        OpCode::BongkarTuple => (1, a, None),
        OpCode::PinjamIndex | OpCode::MergeMap | OpCode::BuildRepeat | OpCode::CobaTambahField => (2, 1, None),
        OpCode::PinjamField | OpCode::Print | OpCode::PrintNoNl | OpCode::ChanMake | OpCode::ChanRecv
        | OpCode::ChanTryRecv => (1, 1, None),
        OpCode::ChanSend => (2, 0, None),
        OpCode::ChanTrySend => (2, 1, None),
        OpCode::CobaTambahIndex => (3, 1, None),
        OpCode::BuildMap => (2 * a, 1, None),
        OpCode::Spawn => (a + 1, 1, None),
        OpCode::MakeIter => (1, 3, None),
        // Iterator dengan posisi milik VM (yang native ditumpahkan dulu); `IterAda` tidak bergabung di langkah tunggal.
        OpCode::IterVar if s.get(a / 2..a / 2 + 3)?.iter().any(|j| j.di_vm()) => (0, 1, None),
        OpCode::IterAda if s.get(a..a + 3)?.iter().any(|j| j.di_vm()) => (0, 1, None),
        OpCode::SamaTempat => (1, 1, None),
        OpCode::HofMethod => ((a >> 24) + 1, 1, None),
        OpCode::DeferPush => (a + 1, 0, None),
        OpCode::SelectEnd => (0, 0, None),
        OpCode::HofCall => {
            let n = crate::vm::hof::layanan_hof(f.chunk.names.get(a)?)?;
            (if matches!(n, "arr::reduce" | "arr::fold" | "arr::zip_with") { 3 } else { 2 }, 1, None)
        }
        _ => return None,
    })
}

/// Penerus instruksi `ip` kalau semua operandnya milik kode native; `None` = bukan bentuk asli.
fn langkah_asli(
    f: &VmFunction, cari: &dyn Fn(&str) -> Option<Arc<VmFunction>>,
    r: &mut Rencana, ip: usize, mut s: Vec<Jenis>,
) -> Option<Vec<(usize, Vec<Jenis>)>> {
    use Jenis::*;
    let ins = f.chunk.code[ip];
    let a = ins.arg as usize;
    // `Lebar` cuma di bingkai yang punya stack VM (bukan callee bulat).
    let lebar = r.kembali != KEMBALI_BULAT;
    let skalar = |j: Jenis| matches!(j, Bulat | Bool | Pecahan | Kosong | Satuan) || (lebar && j == Lebar);
    let angka = |j: Jenis| matches!(j, Bulat | Pecahan);
    // Dua bulat yang salah satunya `Lebar`: hasilnya `Lebar`.
    let lebar2 = |x: Jenis, y: Jenis| lebar && matches!((x, y), (Lebar, Bulat | Lebar) | (Bulat, Lebar));
    // Dua angka: bulat dengan bulat tetap bulat, selain itu pecahan (aturan `VmValue::add` dkk.).
    let angka2 = |s: &mut Vec<Jenis>| -> Option<Jenis> {
        let y = s.pop()?; let x = s.pop()?;
        if !angka(x) || !angka(y) { return None; }
        Some(if x == Bulat && y == Bulat { Bulat } else { Pecahan })
    };
    let bulat2 = |s: &mut Vec<Jenis>| -> Option<()> {
        let y = s.pop()?; let x = s.pop()?;
        (x == Bulat && y == Bulat).then_some(())
    };
    let lanjut = ip + 1;
    match ins.op {
        OpCode::LoadLocal | OpCode::LokalHitung => {
            let j = *s.get(a)?;
            if !skalar(j) { return None; }
            s.push(j);
        }
        OpCode::AmbilLokal => {
            let j = *s.get(a)?;
            if !skalar(j) { return None; }
            s[a] = Kosong;
            s.push(j);
        }
        // Larik milik VM di lokal `a`, lewat `lambat::baca`/`tulis` (bukan larik bulat = titik keluar saat jalan).
        OpCode::IndeksLokal | OpCode::TulisIndexLokal if f.chunk.kode_jit.spek_gagal_di(ip) => return None,
        // Wadahnya salinan maya lokal `l` (`boleh_lewat_salinan` sudah memeriksa spekulasinya): `baca` atas `l`.
        OpCode::GetIndex if s.len() >= 2 && matches!(s[s.len() - 2], Salinan(_)) => {
            if s.pop()? != Bulat { return None; }
            let j = elemen_larik(f, ip);
            r.spek[ip] = Some(j);
            *s.last_mut()? = j;
        }
        // `a[i] = a[i] + x` (`TumbuhIndexLokal`): VM cuma menyambung di tempat bila elemen dan tambahannya
        // sama-sama larik; dengan angka native di salah satunya indeksnya dibuang dan lanjut ke `Add` biasa.
        OpCode::TumbuhIndexLokal if s.len() >= 3 && s[s.len() - 1] == Bulat
            && (s[s.len() - 2].skalar() || s[s.len() - 3].skalar()) => { s.pop(); }
        OpCode::IndeksLokal => {
            if s.pop()? != Bulat || *s.get(a)? != Nilai { return None; }
            let j = elemen_larik(f, ip);
            r.spek[ip] = Some(j);
            s.push(j);
        }
        OpCode::TulisIndexLokal => {
            let (i, v) = (s.pop()?, s.pop()?);
            if i != Bulat || !matches!(v, Bulat | Bool | Pecahan) || *s.get(a)? != Nilai { return None; }
        }
        OpCode::IntImm => s.push(Bulat),
        // Method pemakai yang sudah diketahui (`Cara::Metode`): tidak ada yang diperiksa.
        OpCode::PeriksaArgMetode => {
            let n = (a >> 24) + 2;
            if s.len() < n || !matches!(s[s.len() - n], Fungsi(_)) { return None; }
        }
        OpCode::True | OpCode::False => s.push(Bool),
        OpCode::Nil => s.push(Kosong),
        OpCode::Unit => s.push(Satuan),
        OpCode::Const => match f.chunk.constants.get(a)? {
            VmValue::Int(v) => match i64::try_from(*v) {
                Ok(k) => { r.konstanta[ip] = Some(k); s.push(Bulat) }
                Err(_) if lebar => { r.konstanta128[ip] = Some(*v); s.push(Lebar) }
                Err(_) => return None,
            },
            VmValue::Float(x) => { r.konstanta[ip] = Some(x.to_bits() as i64); s.push(Pecahan) }
            _ => return None,
        },
        // 128 bit: operand `Lebar`, atau dua bulat yang di situs ini pernah meluap 64 bit (jaga luapannya
        // spekulatif). `*` dua bulat 64 bit selalu muat 128 bit; `*` dengan `Lebar` lewat pembantu.
        OpCode::Add | OpCode::Sub | OpCode::Mul if lebar && s.len() >= 2 && {
            let (x, y) = (s[s.len() - 2], s[s.len() - 1]);
            let meluap = x == Bulat && y == Bulat && f.chunk.kode_jit.spek_tingkat(ip) >= 1;
            meluap || (ins.op != OpCode::Mul && lebar2(x, y))
        } => {
            s.truncate(s.len() - 2);
            s.push(Lebar)
        }
        // `%`/`/` dengan `Lebar` yang isinya muat 64 bit (dijaga spekulatif, `turun_lebar`): hasilnya bulat.
        OpCode::Mod | OpCode::Div if s.len() >= 2 && lebar2(s[s.len() - 2], s[s.len() - 1]) && !f.chunk.kode_jit.spek_gagal_di(ip) => {
            s.truncate(s.len() - 2);
            s.push(Bulat)
        }
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div => { let j = angka2(&mut s)?; s.push(j) }
        // `%` pecahan adalah `fmod`, yang tidak punya instruksi mesin: VM yang menghitungnya.
        OpCode::Mod => { bulat2(&mut s)?; s.push(Bulat) }
        OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne
            if s.len() >= 2 && lebar2(s[s.len() - 2], s[s.len() - 1]) => {
            s.truncate(s.len() - 2);
            s.push(Bool)
        }
        OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge => { angka2(&mut s)?; s.push(Bool) }
        // Bit atas `Lebar`: geseran yang jumlahnya bulat 64 bit.
        OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot
            if s.len() >= 2 && lebar2(s[s.len() - 2], s[s.len() - 1]) => {
            s.truncate(s.len() - 2);
            s.push(Lebar)
        }
        OpCode::Shl | OpCode::Shr if s.len() >= 2 && lebar && s[s.len() - 2] == Lebar && s[s.len() - 1] == Bulat => { s.pop(); }
        // Bit atas dua bulat 64 bit: hasil yang tidak muat (geser kiri) atau geseran di luar batas deopt.
        OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot | OpCode::Shl | OpCode::Shr => {
            bulat2(&mut s)?;
            s.push(Bulat)
        }
        OpCode::BitNot => if !matches!(*s.last()?, Bulat) && !(lebar && *s.last()? == Lebar) { return None },
        OpCode::Eq | OpCode::Ne => {
            let t = s.len();
            if t >= 2 && s[t - 1] == Bool && s[t - 2] == Bool { s.truncate(t - 2); } else { angka2(&mut s)?; }
            s.push(Bool)
        }
        OpCode::Neg => if !angka(*s.last()?) { return None },
        OpCode::Not => if *s.last()? != Bool { return None },
        OpCode::KeBool => {
            let j = s.pop()?;
            if !matches!(j, Bulat | Bool) && !(lebar && j == Lebar) { return None; }
            s.push(Bool);
        }
        // Nilai milik VM yang dibuang dilepas lewat `Cara::Buang`.
        OpCode::Pop => if s.pop()?.di_vm() { return None },
        OpCode::Dup => {
            let j = *s.last()?;
            if j.di_vm() { return None; }
            s.push(j);
        }
        OpCode::Swap => {
            let t = s.len();
            if t < 2 || s[t - 1].di_vm() || s[t - 2].di_vm() { return None; }
            s.swap(t - 1, t - 2);
        }
        // Akhir scope blok: puncak tetap, `a` di bawahnya dibuang (yang milik VM lewat `Cara::Langkah`).
        OpCode::SlideDown => {
            let t = s.len();
            if a >= t || s[t - a - 1..].iter().any(|j| j.di_vm()) { return None; }
            let atas = s.pop()?;
            s.truncate(t - a - 1);
            s.push(atas);
        }
        OpCode::StoreLocal | OpCode::SimpanBuang => {
            let v = *s.last()?;
            // Lokal `Nilai` bisa berupa sel tangkapan closure: penulisannya milik VM.
            if !skalar(v) || !skalar(*s.get(a)?) || a + 1 >= s.len() { return None; }
            s[a] = v;
            if ins.op == OpCode::SimpanBuang { s.pop(); }
        }
        OpCode::Jump => return Some(vec![(a, s)]),
        OpCode::JumpFalse | OpCode::JumpTrue => {
            let j = s.pop()?;
            if !matches!(j, Bulat | Bool) && !(lebar && j == Lebar) { return None; }
            return Some(vec![(a, s.clone()), (lanjut, s)]);
        }
        OpCode::JumpFalseKeep | OpCode::JumpTrueKeep => {
            let j = *s.last()?;
            if !matches!(j, Bulat | Bool) && !(lebar && j == Lebar) { return None; }
            return Some(vec![(a, s.clone()), (lanjut, s)]);
        }
        OpCode::Return | OpCode::ReturnNil if r.ada_defer => return None,
        OpCode::Return => {
            let ok = match r.kembali {
                KEMBALI_BULAT => *s.last()? == Bulat,
                KEMBALI_SKALAR => matches!(*s.last()?, Bulat | Bool | Pecahan),
                _ => false,
            };
            if !ok { return None; }
            return Some(vec![]);
        }
        OpCode::ReturnNil if r.kembali == KEMBALI_SKALAR => {
            r.cara[ip] = Cara::KembaliUnit;
            return Some(vec![]);
        }
        OpCode::LoadGlobal => {
            let g = cari(f.chunk.names.get(a)?)?;
            if !bentuk_layak(&g) { return None; }
            let i = match r.callee.iter().position(|c| Arc::ptr_eq(c, &g)) {
                Some(i) => i,
                None => { r.callee.push(g); r.callee.len() - 1 }
            };
            s.push(Fungsi(i));
        }
        // `math::wrap_*` atas dua bulat (64 bit bawahnya): hasil 0 sampai 2^64-1, selalu `Lebar`.
        OpCode::Call if a == 2 && lebar && s.len() >= 3 && s[s.len() - 3].wrap()
            && s[s.len() - 2..].iter().all(|j| matches!(j, Bulat | Lebar)) => {
            s.truncate(s.len() - 3);
            s.push(Lebar);
        }
        // `str::char_at(teks, i)`: kode karakter native (yang tak bisa dijawab di tempat lewat jalur umum).
        OpCode::Call if a == 2 && s.len() >= 3 && s[s.len() - 3].bungkus(BUNGKUS_CHAR_AT)
            && (s[s.len() - 2].di_vm() || matches!(s[s.len() - 2], Salinan(_))) && s[s.len() - 1] == Bulat => {
            s.truncate(s.len() - 3);
            s.push(Rune);
        }
        // `int`/`uint` atas bulat 128 bit: nilainya tetap (`uint` menolak negatif); bulat 64 bit kalau umpannya
        // begitu (jaga spekulasi), selain itu tetap `Lebar`.
        OpCode::Call if a == 1 && lebar && s.len() >= 2 && s[s.len() - 2].bungkus(BUNGKUS_KE_BULAT) && s[s.len() - 1] == Lebar => {
            let sempit = spek_hasil(f, ip) == Some(Bulat) && !f.chunk.kode_jit.spek_gagal_di(ip);
            s.truncate(s.len() - 2);
            s.push(if sempit { Bulat } else { Lebar });
        }
        // `int(rune)` / `rune as uint`: kodenya, tanpa panggilan.
        OpCode::Call if a == 1 && s.len() >= 2 && s[s.len() - 2].bungkus(BUNGKUS_KE_BULAT) && matches!(s[s.len() - 1], Rune | Bulat) => {
            s.truncate(s.len() - 2);
            s.push(Bulat);
        }
        OpCode::Call if a == 1 && s.len() >= 2 && s[s.len() - 2].bungkus(BUNGKUS_KE_PECAHAN) && matches!(s[s.len() - 1], Bulat | Pecahan) => {
            s.truncate(s.len() - 2);
            s.push(Pecahan);
        }
        OpCode::Call => {
            if s.len() < a + 1 { return None; }
            let Fungsi(i) = s[s.len() - a - 1] else { return None };
            if r.callee[i].arity as usize != a || r.larang_bulat.contains(&(Arc::as_ptr(&r.callee[i]) as usize)) { return None; }
            for _ in 0..a { if s.pop()? != Bulat { return None; } }
            s.pop();
            s.push(Bulat);
        }
        // `for` atas rentang bulat: [jumlah, awal, indeks] (`MakeIterRentang` VM).
        OpCode::MakeIterRentang => {
            bulat2(&mut s)?;
            s.extend([Bulat, Bulat, Bulat]);
        }
        OpCode::IterAda => {
            if *s.get(a)? != Bulat || *s.get(a + 1)? != Bulat || *s.get(a + 2)? != Bulat { return None; }
            s.push(Bool);
        }
        OpCode::IterMaju => if *s.get(a)? != Bulat { return None },
        OpCode::IterVar => {
            let k = a / 2;
            if *s.get(k)? != Bulat || *s.get(k + 1)? != Bulat || *s.get(k + 2)? != Bulat { return None; }
            s.push(Bulat);
        }
        _ => return None,
    }
    Some(vec![(lanjut, s)])
}
