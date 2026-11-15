//! Satu `JITModule` per proses (memori kode W^X diurus cranelift-jit: ditulis, lalu jadi RX saat
//! finalisasi). Fungsi dan semua callee-nya dianalisis DULU; batch dibatalkan kalau satu tidak layak.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::FunctionBuilderContext;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use super::layak::Jenis;
use super::{deopt, lambat, layak, turun, KodeJit};
use crate::vm::value::VmFunction;

struct ModulJit {
    m: JITModule,
    /// Titik masuk → fungsi yang sudah didefinisikan (Arc dipegang supaya alamatnya tak dipakai ulang).
    jadi: HashMap<Kunci, (FuncId, Arc<VmFunction>, bool)>,
    /// Per nomor bingkai (`fid`): fungsi dan rencananya, dibaca `deopt::pulihkan`.
    info: Vec<(Arc<VmFunction>, Arc<layak::Rencana>)>,
    catat: FuncId,
    undi: Option<FuncId>,
    baca: FuncId,
    tulis: FuncId,
    lambat: turun::ImporLambat,
    fctx: FunctionBuilderContext,
    urut: u32,
    /// Fungsi yang ditolak dan alasannya, untuk `JRG_JIT_LOG`.
    tolak: Vec<(String, String)>,
    /// Badan CLIF callee bulat kecil per `FuncId`, disisipkan Cranelift di tempat panggilannya (`Sisip`).
    badan: HashMap<u32, cranelift_codegen::ir::Function>,
    /// Titik masuk yang bisa dipanggil tanpa bingkai VM didorong lebih dulu (`layak::ringan`).
    ringan: HashMap<Kunci, bool>,
}

/// Callee bulat yang badannya disisipkan paling besar sekian instruksi CLIF.
const SISIP_MAKS: usize = 300;

/// Penyisip Cranelift: panggilan langsung ke callee bulat yang badannya tersimpan (bukan ke dirinya sendiri)
/// diganti badannya. Status, deopt dan hasilnya tetap lewat nilai kembalian yang sama, jadi pemanggil
/// menanganinya persis seperti panggilan biasa.
struct Sisip<'a> {
    badan: &'a HashMap<u32, cranelift_codegen::ir::Function>,
    diri: u32,
}

impl cranelift_codegen::inline::Inline for Sisip<'_> {
    fn inline(
        &mut self, caller: &cranelift_codegen::ir::Function, _inst: cranelift_codegen::ir::Inst,
        _op: cranelift_codegen::ir::Opcode, callee: cranelift_codegen::ir::FuncRef, _args: &[cranelift_codegen::ir::Value],
    ) -> cranelift_codegen::inline::InlineCommand<'_> {
        use cranelift_codegen::inline::InlineCommand;
        use cranelift_codegen::ir::ExternalName;
        let ExternalName::User(r) = &caller.dfg.ext_funcs[callee].name else { return InlineCommand::KeepCall };
        let nama = &caller.params.user_named_funcs()[*r];
        if nama.namespace != 0 || nama.index == self.diri { return InlineCommand::KeepCall; }
        match self.badan.get(&nama.index) {
            Some(f) => InlineCommand::Inline { callee: std::borrow::Cow::Borrowed(f), visit_callee: false },
            None => InlineCommand::KeepCall,
        }
    }
}

// `JITModule` memegang penunjuk mentah; semua aksesnya lewat `Mutex` di bawah, satu per satu.
unsafe impl Send for ModulJit {}

fn modul() -> &'static Mutex<Option<ModulJit>> {
    static M: OnceLock<Mutex<Option<ModulJit>>> = OnceLock::new();
    M.get_or_init(|| {
        let mut fb = settings::builder();
        let _ = fb.set("opt_level", "speed");
        let _ = fb.set("is_pic", "false");
        let _ = fb.set("use_colocated_libcalls", "false");
        // Verifier IR memeriksa `turun.rs` di build debug dan mode stres; di jalan biasa cuma biaya kompilasi.
        let periksa = cfg!(debug_assertions) || std::env::var_os("JRG_JIT_STRES").is_some();
        let _ = fb.set("enable_verifier", if periksa { "true" } else { "false" });
        let isa = cranelift_native::builder().ok()
            .and_then(|ib| ib.finish(settings::Flags::new(fb)).ok());
        Mutex::new(isa.and_then(|isa| {
            let mut jb = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
            jb.symbol("jrg_jit_catat", deopt::catat as *const u8);
            jb.symbol("jrg_jit_undi", deopt::undi as *const u8);
            jb.symbol("jrg_jit_baca", lambat::baca as *const u8);
            jb.symbol("jrg_jit_tulis", lambat::tulis as *const u8);
            jb.symbol("jrg_jit_langkah", lambat::langkah as *const u8);
            jb.symbol("jrg_jit_muat", lambat::muat as *const u8);
            jb.symbol("jrg_jit_buang", lambat::buang as *const u8);
            jb.symbol("jrg_jit_benar", lambat::benar as *const u8);
            jb.symbol("jrg_jit_buka", lambat::buka as *const u8);
            jb.symbol("jrg_jit_taruh", lambat::taruh as *const u8);
            jb.symbol("jrg_jit_ambil", lambat::ambil as *const u8);
            jb.symbol("jrg_jit_simpan", lambat::simpan as *const u8);
            jb.symbol("jrg_jit_pola", lambat::pola as *const u8);
            jb.symbol("jrg_jit_payload", lambat::payload as *const u8);
            jb.symbol("jrg_jit_pola_lokal", lambat::pola_lokal as *const u8);
            jb.symbol("jrg_jit_payload_lokal", lambat::payload_lokal as *const u8);
            jb.symbol("jrg_jit_payload_angka_lokal", lambat::payload_angka_lokal as *const u8);
            jb.symbol("jrg_jit_pola_pilih", lambat::pola_pilih as *const u8);
            jb.symbol("jrg_jit_field_lokal", lambat::field_lokal as *const u8);
            jb.symbol("jrg_jit_field_lokal_banyak", lambat::field_lokal_banyak as *const u8);
            jb.symbol("jrg_jit_metode_tukar", lambat::metode_tukar as *const u8);
            jb.symbol("jrg_jit_metode_lokal", lambat::metode_lokal as *const u8);
            jb.symbol("jrg_jit_karakter_lokal", lambat::karakter_lokal as *const u8);
            jb.symbol("jrg_jit_konteks_mentah", lambat::konteks_mentah as *const u8);
            jb.symbol("jrg_jit_masuk_ringan", lambat::masuk_ringan as *const u8);
            jb.symbol("jrg_jit_keluar_ringan", lambat::keluar_ringan as *const u8);
            jb.symbol("jrg_jit_karakter", lambat::karakter as *const u8);
            jb.symbol("jrg_jit_bangun", lambat::bangun as *const u8);
            jb.symbol("jrg_jit_baca_pecahan", lambat::baca_pecahan as *const u8);
            jb.symbol("jrg_jit_jenis_sama", lambat::jenis_sama as *const u8);
            jb.symbol("jrg_jit_panjang", lambat::panjang as *const u8);
            jb.symbol("jrg_jit_panjang_lokal", lambat::panjang_lokal as *const u8);
            jb.symbol("jrg_jit_payload_jenis_sama", lambat::payload_jenis_sama as *const u8);
            jb.symbol("jrg_jit_payload_panjang", lambat::payload_panjang as *const u8);
            jb.symbol("jrg_jit_keluar_ringan_angka", lambat::keluar_ringan_angka as *const u8);
            jb.symbol("jrg_jit_buka_lokal", lambat::buka_lokal as *const u8);
            jb.symbol("jrg_jit_iter_ada", lambat::iter_ada as *const u8);
            jb.symbol("jrg_jit_maju", lambat::maju as *const u8);
            jb.symbol("jrg_jit_langkah_cabang", lambat::langkah_cabang as *const u8);
            jb.symbol("jrg_jit_sebar", lambat::sebar as *const u8);
            jb.symbol("jrg_jit_pindah", lambat::pindah as *const u8);
            jb.symbol("jrg_jit_muat_global", lambat::muat_global as *const u8);
            jb.symbol("jrg_jit_simpan_global", lambat::simpan_global as *const u8);
            jb.symbol("jrg_jit_indeks", lambat::indeks as *const u8);
            jb.symbol("jrg_jit_field", lambat::field as *const u8);
            jb.symbol("jrg_jit_masuk", lambat::masuk as *const u8);
            jb.symbol("jrg_jit_keluar", lambat::keluar as *const u8);
            jb.symbol("jrg_jit_salin", lambat::salin as *const u8);
            jb.symbol("jrg_jit_tukar", lambat::tukar as *const u8);
            jb.symbol("jrg_jit_cek_metode", lambat::cek_metode as *const u8);
            jb.symbol("jrg_jit_bawaan", lambat::bawaan as *const u8);
            jb.symbol("jrg_jit_biner", lambat::biner as *const u8);
            jb.symbol("jrg_jit_muat_upval", lambat::muat_upval as *const u8);
            jb.symbol("jrg_jit_simpan_upval", lambat::simpan_upval as *const u8);
            jb.symbol("jrg_jit_tulis_vm", lambat::tulis_vm as *const u8);
            jb.symbol("jrg_jit_tulis_lebar", lambat::tulis_lebar as *const u8);
            jb.symbol("jrg_jit_buka_lebar", lambat::buka_lebar as *const u8);
            let mut m = JITModule::new(jb);
            let cc = m.isa().default_call_conv();
            let catat = m.declare_function("jrg_jit_catat", Linkage::Import, &turun::tanda_tangan_catat(&m, cc)).ok()?;
            let baca = m.declare_function("jrg_jit_baca", Linkage::Import, &turun::tanda_tangan_baca(&m, cc)).ok()?;
            let tulis = m.declare_function("jrg_jit_tulis", Linkage::Import, &turun::tanda_tangan_tulis(cc)).ok()?;
            use cranelift_codegen::ir::types::{I32, I64};
            let p = m.target_config().pointer_type();
            let mut impor = |nama: &str, param: &[cranelift_codegen::ir::Type], hasil| {
                m.declare_function(nama, Linkage::Import, &turun::tanda_tangan_umum(cc, param, hasil)).ok()
            };
            let lambat = turun::ImporLambat {
                langkah: impor("jrg_jit_langkah", &[I64, p, I64, I64, I64, p], Some(I32))?,
                muat: impor("jrg_jit_muat", &[I64, I64], None)?,
                buang: impor("jrg_jit_buang", &[I64], None)?,
                benar: impor("jrg_jit_benar", &[I64, I32], Some(I32))?,
                buka: impor("jrg_jit_buka", &[I64, I32, p], Some(I32))?,
                taruh: impor("jrg_jit_taruh", &[I64, p], None)?,
                ambil: impor("jrg_jit_ambil", &[I64, I64], None)?,
                simpan: impor("jrg_jit_simpan", &[I64, I64, I64, I32, I32], None)?,
                pola: impor("jrg_jit_pola", &[I64, p], Some(I32))?,
                payload: impor("jrg_jit_payload", &[I64], Some(I32))?,
                pola_lokal: impor("jrg_jit_pola_lokal", &[I64, p], Some(I32))?,
                payload_lokal: impor("jrg_jit_payload_lokal", &[I64, I64], Some(I32))?,
                payload_angka: impor("jrg_jit_payload_angka_lokal", &[I64, I64, I32, p], Some(I32))?,
                pola_pilih: impor("jrg_jit_pola_pilih", &[I64, p], Some(I32))?,
                field_lokal: impor("jrg_jit_field_lokal", &[I64, I64, I64, I64], Some(I32))?,
                field_banyak: impor("jrg_jit_field_lokal_banyak", &[I64, I64, I64], Some(I64))?,
                metode_tukar: impor("jrg_jit_metode_tukar", &[I64, I64], Some(I32))?,
                metode_lokal: impor("jrg_jit_metode_lokal", &[I64, I64, I64, I32], Some(I32))?,
                karakter_lokal: impor("jrg_jit_karakter_lokal", &[I64, I64], Some(I64))?,
                konteks_mentah: impor("jrg_jit_konteks_mentah", &[p], None)?,
                masuk_ringan: impor("jrg_jit_masuk_ringan", &[I64, p, I64, I64, p, p, p], Some(I32))?,
                keluar_ringan: impor("jrg_jit_keluar_ringan", &[I64, I64, p], Some(I32))?,
                karakter: impor("jrg_jit_karakter", &[I64, I64], Some(I64))?,
                bangun: impor("jrg_jit_bangun", &[I64, I64, I64, I64, I64], None)?,
                baca_pecahan: impor("jrg_jit_baca_pecahan", &[I64, I64, p], Some(I32))?,
                jenis_sama: impor("jrg_jit_jenis_sama", &[I64, I64], Some(I32))?,
                panjang: impor("jrg_jit_panjang", &[I64, p], Some(I32))?,
                panjang_lokal: impor("jrg_jit_panjang_lokal", &[I64, p], Some(I32))?,
                payload_jenis_sama: impor("jrg_jit_payload_jenis_sama", &[I64, I64], Some(I32))?,
                payload_panjang: impor("jrg_jit_payload_panjang", &[I64, p], Some(I32))?,
                keluar_ringan_angka: impor("jrg_jit_keluar_ringan_angka", &[I64, I64, p, I64], Some(I32))?,
                buka_lokal: impor("jrg_jit_buka_lokal", &[I64, I32, p], Some(I32))?,
                iter_ada: impor("jrg_jit_iter_ada", &[I64], Some(I32))?,
                maju: impor("jrg_jit_maju", &[I64], Some(I32))?,
                langkah_cabang: impor("jrg_jit_langkah_cabang", &[I64, p, I64, I64, p, I64], Some(I64))?,
                sebar: impor("jrg_jit_sebar", &[I64], Some(I32))?,
                pindah: impor("jrg_jit_pindah", &[I64, I64], None)?,
                muat_global: impor("jrg_jit_muat_global", &[p, I64], Some(I32))?,
                simpan_global: impor("jrg_jit_simpan_global", &[p, I64, I64, I32], Some(I32))?,
                indeks: impor("jrg_jit_indeks", &[I64, I64, I32], Some(I32))?,
                field: impor("jrg_jit_field", &[p, p, I32], Some(I32))?,
                masuk: impor("jrg_jit_masuk", &[I64, p, I64, I64, p, p, p], Some(I32))?,
                keluar: impor("jrg_jit_keluar", &[I64, I64, p], Some(I32))?,
                salin: impor("jrg_jit_salin", &[I64, I64], None)?,
                tukar: impor("jrg_jit_tukar", &[I64, I64], None)?,
                cek_metode: impor("jrg_jit_cek_metode", &[I64, p], Some(I32))?,
                bawaan: impor("jrg_jit_bawaan", &[I64, p, I64, I64, p, I64], Some(I32))?,
                biner: impor("jrg_jit_biner", &[I64, p, I64, p], Some(I32))?,
                muat_upval: impor("jrg_jit_muat_upval", &[I64, I64], Some(I32))?,
                simpan_upval: impor("jrg_jit_simpan_upval", &[I64, I64, I64, I32], Some(I32))?,
                tulis_vm: impor("jrg_jit_tulis_vm", &[I64, I64, I32], None)?,
                tulis_lebar: impor("jrg_jit_tulis_lebar", &[I64, I64, I64], None)?,
                buka_lebar: impor("jrg_jit_buka_lebar", &[I64, p], Some(I32))?,
            };
            let undi = if deopt::acak_aktif() {
                Some(m.declare_function("jrg_jit_undi", Linkage::Import, &turun::tanda_tangan_undi(cc)).ok()?)
            } else { None };
            Some(ModulJit { m, jadi: HashMap::new(), info: Vec::new(), catat, undi, baca, tulis, lambat, fctx: FunctionBuilderContext::new(), urut: 0, tolak: Vec::new(), badan: HashMap::new(), ringan: HashMap::new() })
        }))
    })
}

/// Nama fungsi yang berhasil dikompilasi, untuk `JRG_JIT_LOG`.
pub fn terkompilasi() -> Vec<String> {
    let g = modul().lock().unwrap_or_else(|p| p.into_inner());
    let mut v: Vec<String> = g.as_ref().map(|m| m.jadi.values().map(|(_, f, _)| f.name.clone()).collect()).unwrap_or_default();
    v.sort();
    v.dedup();
    v
}

/// Fungsi yang ditolak beserta alasannya, untuk `JRG_JIT_LOG`.
pub fn ditolak() -> Vec<(String, String)> {
    let g = modul().lock().unwrap_or_else(|p| p.into_inner());
    g.as_ref().map(|m| m.tolak.clone()).unwrap_or_default()
}

/// Fungsi, instruksi masuk, boleh `Return` native, jenis stack di titik masuk, versi kode chunk-nya.
type Kunci = (usize, usize, u8, Vec<Jenis>, u32);

fn kunci(f: &Arc<VmFunction>, masuk: usize, kembali: u8, jenis: &[Jenis]) -> Kunci {
    let versi = f.chunk.kode_jit.versi.load(std::sync::atomic::Ordering::Relaxed);
    (Arc::as_ptr(f) as usize, masuk, kembali, jenis.to_vec(), versi)
}

/// Unit ke-`i` callee bulat (berkonvensi `turun::tanda_tangan_bulat`).
fn k_bulat(baru: &[(Kunci, Arc<VmFunction>, Arc<layak::Rencana>)], i: usize) -> bool { baru[i].0 .2 == layak::KEMBALI_BULAT }

/// Titik masuk callee yang dipanggil kode native: awal fungsi, semua argumen bulat.
fn kunci_callee(f: &Arc<VmFunction>) -> Kunci {
    kunci(f, 0, layak::KEMBALI_BULAT, &vec![Jenis::Bulat; f.arity as usize])
}

/// Fungsi dan rencana bingkai bernomor `fid` (nomor itu cuma lahir dari kompilasi yang berhasil).
pub fn info(fid: u32) -> (Arc<VmFunction>, Arc<layak::Rencana>) {
    let g = modul().lock().unwrap_or_else(|p| p.into_inner());
    let (f, r) = &g.as_ref().expect("modul JIT hidup selama kodenya").info[fid as usize];
    (Arc::clone(f), Arc::clone(r))
}

/// Kompilasi titik masuk `masuk` fungsi `f` (stack bingkai berjenis `jenis`) beserta callee-nya.
pub fn kompilasi(
    f: &Arc<VmFunction>, masuk: usize, jenis: &[Jenis], kembali: u8,
    cari: &dyn Fn(&str) -> Option<Arc<VmFunction>>,
) -> Option<KodeJit> {
    let mut g = modul().lock().unwrap_or_else(|p| p.into_inner());
    let mj = g.as_mut()?;
    let k0 = kunci(f, masuk, kembali, jenis);
    if let Some((id, _, langsung)) = mj.jadi.get(&k0) {
        return Some(KodeJit { masuk: unsafe { std::mem::transmute(mj.m.get_finalized_function(*id)) }, langsung: *langsung });
    }
    // Kumpulkan titik masuk ini dan semua callee yang belum jadi. Callee yang tak layak dilarang dan
    // analisisnya diulang: panggilan ke sana jadi titik keluar.
    // `larang`: tidak dikompilasi sama sekali; `larang_bulat`: cuma tidak bisa jadi callee bulat.
    let (mut larang, mut larang_bulat): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let baru = 'ulang: loop {
        let cari_boleh = |nama: &str| cari(nama).filter(|g| !larang.contains(&(Arc::as_ptr(g) as usize)));
        let mut baru: Vec<(Kunci, Arc<VmFunction>, Arc<layak::Rencana>)> = Vec::new();
        let mut antre = vec![(k0.clone(), Arc::clone(f), false)];
        while let Some((k, h, langsung)) = antre.pop() {
            if mj.jadi.contains_key(&k) || baru.iter().any(|(b, _, _)| *b == k) { continue; }
            let utama = k == k0;
            let hasil = layak::analisis(&h, &cari_boleh, k.1, k.3.clone(), k.2, langsung, &larang_bulat, &larang)
                .and_then(|r| if k.2 == layak::KEMBALI_BULAT && keluar_biasa(&h, &r) {
                    // Callee bulat yang tiap jalannya bisa keluar ke VM (deopt per panggilan): panggilan langsung saja.
                    Err("callee bulat dengan titik keluar".into())
                } else { Ok(r) });
            match hasil {
                Ok(r) => {
                    for i in r.callee_asli() { antre.push((kunci_callee(&r.callee[i]), Arc::clone(&r.callee[i]), false)); }
                    for (i, jenis) in &r.langsung {
                        let c = &r.callee[*i];
                        antre.push((kunci(c, 0, layak::KEMBALI_SKALAR, jenis), Arc::clone(c), true));
                    }
                    baru.push((k, h, Arc::new(r)));
                }
                // Tiap ulangan menambah satu larangan baru; yang tidak (bug) berhenti di sini, bukan berputar.
                Err(_) if !utama && larang.len() + larang_bulat.len() < 64 => {
                    let p = Arc::as_ptr(&h) as usize;
                    if k.2 == layak::KEMBALI_BULAT && !larang_bulat.contains(&p) { larang_bulat.push(p) } else { larang.push(p) }
                    continue 'ulang;
                }
                Err(alasan) => {
                    let catatan = (f.name.clone(), alasan);
                    if !mj.tolak.contains(&catatan) { mj.tolak.push(catatan); }
                    return None;
                }
            }
        }
        break baru;
    };
    let cc = mj.m.isa().default_call_conv();
    let sig = turun::tanda_tangan(&mj.m, cc);
    let mut id_baru = Vec::new();
    for (_, h, _) in &baru {
        mj.urut += 1;
        let nama = format!("jrg{}_{}", mj.urut, h.name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>());
        let tt = if k_bulat(&baru, id_baru.len()) { turun::tanda_tangan_bulat(cc, h.arity as usize) } else { sig.clone() };
        let id = mj.m.declare_function(&nama, Linkage::Local, &tt).ok()?;
        id_baru.push(id);
    }
    let id_kunci = |k: &Kunci, mj: &ModulJit| -> FuncId {
        match baru.iter().position(|(b, _, _)| b == k) {
            Some(i) => id_baru[i],
            None => mj.jadi[k].0,
        }
    };
    let mut ctx = mj.m.make_context();
    let mut utuh = true;
    let fid0 = mj.info.len() as u32;
    // Callee lebih dulu (ditemukan sesudah pemanggilnya), supaya badannya sudah ada saat pemanggilnya disisipi.
    for (i, (k, h, r)) in baru.iter().enumerate().rev() {
        let asli = r.callee_asli();
        let callee: Vec<Option<FuncId>> = (0..r.callee.len())
            .map(|i| asli.contains(&i).then(|| id_kunci(&kunci_callee(&r.callee[i]), mj))).collect();
        let langsung: Vec<FuncId> = r.langsung.iter()
            .map(|(i, jenis)| id_kunci(&kunci(&r.callee[*i], 0, layak::KEMBALI_SKALAR, jenis), mj)).collect();
        let ringan: Vec<bool> = r.langsung.iter()
            .map(|(i, jenis)| mj.ringan.get(&kunci(&r.callee[*i], 0, layak::KEMBALI_SKALAR, jenis)).copied().unwrap_or(false))
            .collect();
        mj.ringan.insert(k.clone(), layak::ringan(h, r));
        ctx.func.signature = if k.2 == layak::KEMBALI_BULAT { turun::tanda_tangan_bulat(cc, h.arity as usize) } else { sig.clone() };
        let impor = turun::Impor {
            catat: mj.catat, undi: mj.undi, baca: mj.baca, tulis: mj.tulis, fid: fid0 + i as u32, lambat: &mj.lambat, ringan: &ringan,
        };
        turun::turunkan(&mut mj.m, &mut ctx.func, &mut mj.fctx, h, r, &callee, &langsung, &impor);
        let diri = id_baru[i].as_u32();
        if ctx.inline(Sisip { badan: &mj.badan, diri }).is_err() { utuh = false; break; }
        if k.2 == layak::KEMBALI_BULAT && ctx.func.dfg.num_insts() <= SISIP_MAKS {
            mj.badan.insert(diri, ctx.func.clone());
        }
        // `JRG_JIT_LOG=clif`: IR tiap fungsi sesudah disisipi, lalu kode mesinnya.
        let clif = std::env::var("JRG_JIT_LOG").is_ok_and(|v| v == "clif");
        if clif { eprintln!("[jit] clif {}:\n{}", h.name, ctx.func.display()); ctx.set_disasm(true); }
        utuh = mj.m.define_function(id_baru[i], &mut ctx).is_ok();
        if clif { if let Some(v) = ctx.compiled_code().and_then(|c| c.vcode.as_ref()) { eprintln!("[jit] mesin {}:\n{}", h.name, v); } }
        mj.m.clear_context(&mut ctx);
        if !utuh { break; }
    }
    utuh = utuh && mj.m.finalize_definitions().is_ok();
    if !utuh {
        // Deklarasi yang tak terdefinisi menggagalkan finalisasi berikutnya; proses ini berhenti memakai JIT.
        *g = None;
        return None;
    }
    for (i, (k, h, r)) in baru.into_iter().enumerate() {
        let langsung = !r.langsung.is_empty();
        mj.info.push((Arc::clone(&h), r));
        mj.jadi.insert(k, (id_baru[i], h, langsung));
    }
    let (id, langsung) = (mj.jadi[&k0].0, mj.jadi[&k0].2);
    Some(KodeJit { masuk: unsafe { std::mem::transmute(mj.m.get_finalized_function(id)) }, langsung })
}

/// `JRG_JIT_LOG=rencana`: tiap instruksi yang terjangkau dengan caranya dan stack abstrak sebelumnya.
pub(super) fn cetak_rencana(f: &VmFunction, r: &layak::Rencana) {
    eprintln!("[jit] rencana {} (masuk {}, kembali {}):", f.name, r.masuk, r.kembali);
    for (ip, ins) in f.chunk.code.iter().enumerate() {
        let Some(st) = &r.keadaan[ip] else { continue };
        let cara = if r.keluar[ip] { "KELUAR".to_string() } else { format!("{:?}", r.cara[ip]) };
        eprintln!("[jit]   {ip:4} {:?} {} {cara} {st:?}", ins.op, ins.arg);
    }
}

/// Rencana punya titik keluar terjangkau selain cabang galat (`panik`, `match` tanpa lengan cocok).
fn keluar_biasa(f: &VmFunction, r: &layak::Rencana) -> bool {
    use crate::vm::opcode::OpCode;
    (0..r.keluar.len()).any(|ip| r.keluar[ip] && r.keadaan[ip].is_some()
        && !matches!(f.chunk.code[ip].op, OpCode::Panic | OpCode::GagalCocok | OpCode::Halt))
}
