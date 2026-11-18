//! JIR → CLIF. Konvensi sama dengan penurun langsung (`super::super::turun`): tanda tangan umum atau register
//! (`tanda_tangan_bulat`), kedalaman dijaga di pintu masuk (`ULANG`), tiap jaga yang gagal mencatat keadaan
//! bingkainya lewat `deopt::catat` lalu kembali `DEOPT`, hasil `Return` dengan status menurut jenisnya.

use cranelift_codegen::ir::{condcodes::{FloatCC, IntCC}, types, Block, FuncRef, InstBuilder, MemFlagsData, StackSlot, StackSlotData, StackSlotKind, Type, Value};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Module};

use super::{Akhir, Arg, Banding, Biner, Fungsi, Hasil, Inst, Jaga, Keadaan, Maya, Nilai, OpL, Pb, Tipe, Uji, Unar};
use crate::jit::turun::{Impor, BERHASIL, BERHASIL_BOOL, BERHASIL_NILAI, BERHASIL_PECAHAN, BERHASIL_UNIT, DEOPT, GAGAL, GAGAL_SPEK, KEDALAMAN_MAKS, PUNCAK_VM, ULANG};

/// Pembantu nilai VM (`lambat`) yang dipanggil JIR.
struct Pembantu {
    muat: FuncRef, ambil: FuncRef, simpan: FuncRef, buang: FuncRef, benar: FuncRef, taruh: FuncRef, langkah: FuncRef, buka: FuncRef,
    bawaan: FuncRef, biner: FuncRef, tambah_nilai: FuncRef, muat_upval: FuncRef, simpan_upval: FuncRef,
    teks_bulat: FuncRef, pindah: FuncRef, cabang: FuncRef, tulis_vm: FuncRef, bangun: FuncRef,
    salin: FuncRef, tukar: FuncRef, cek_metode: FuncRef, iter_ada: FuncRef, maju: FuncRef,
    masuk: FuncRef, masuk_ringan: FuncRef, keluar: FuncRef, keluar_ringan: FuncRef, keluar_ringan_angka: FuncRef,
    keluar_ringan_nilai: FuncRef, panjang: FuncRef, panjang_lokal: FuncRef,
    /// `Inst::Pembantu`, menurut `Pb`.
    pb: Vec<(Pb, FuncRef)>,
    tulis_lebar: FuncRef, buka_lebar: FuncRef, sebar: FuncRef, pola_pilih: FuncRef, wujud_varian: FuncRef,
    ubah_baca: FuncRef, ubah_siap: FuncRef, ubah_tulis_lokal: FuncRef,
}

struct Penurun<'a, 'b> {
    b: FunctionBuilder<'b>,
    j: &'a Fungsi,
    nilai: Vec<Option<Value>>,
    slot: StackSlot,
    catat: FuncRef,
    undi: Option<FuncRef>,
    fid: i64,
    ptr: Type,
    /// Penunjuk hasil (`tanda_tangan` umum).
    keluar: Value,
    register: bool,
    /// Callee bulat per nomor `Rencana::callee` (yang dipanggil native).
    callee: Vec<Option<FuncRef>>,
    /// Kedalaman untuk callee (kedalaman bingkai ini + 1).
    kedalaman1: Value,
    lb: Pembantu,
    /// Penyangga satu kata (`lambat::buka`, hasil callee langsung).
    wadah: StackSlot,
    /// Titipan `masuk*` untuk `keluar*` (sembilan kata).
    titip: StackSlot,
    /// Callee `Rencana::langsung` dan apakah ia ringan (`layak::ringan`).
    langsung: Vec<FuncRef>,
    ringan: Vec<bool>,
    /// Dua kata (`lambat::buka_lebar`).
    wadah16: StackSlot,
    /// Tata letak `VmValue` dan alamat VM/basis bingkai (`lambat::konteks_mentah`) untuk larik lokal sebaris.
    sebaris: Option<(&'static crate::jit::tata_letak::TataLetak, Value, Value)>,
    /// Penyimpanan larik lokal yang sudah diperiksa di wilayah ini (`turun::tembolok_larik`): dibuang di awal blok
    /// dan sesudah instruksi apa pun yang bisa menyentuh stack atau penyimpanan VM.
    larik_tb: Vec<(usize, Variable, Variable, Variable)>,
    /// Slot indeks lengan tiap rantai `match` (`Inst::Rantai`).
    rantai_slot: std::collections::HashMap<usize, StackSlot>,
    /// Penyangga payload `lambat::wujud_varian` (dibuat saat pertama dipakai).
    slot_wujud: Option<StackSlot>,
    /// Bulat konstanta yang sudah diturunkan (geseran tetap 128 bit).
    konst: std::collections::HashMap<u32, i64>,
}

fn tipe_clif(t: Tipe) -> Type {
    match t { Tipe::I => types::I64, Tipe::F => types::F64, Tipe::L => types::I128 }
}

pub fn turunkan(
    m: &mut impl Module, fungsi: &mut cranelift_codegen::ir::Function, fctx: &mut FunctionBuilderContext,
    j: &Fungsi, impor: &Impor, id_callee: &[Option<FuncId>], id_langsung: &[FuncId],
) {
    let ptr = m.target_config().pointer_type();
    let register = fungsi.signature.returns.len() == 2;
    let mut b = FunctionBuilder::new(fungsi, fctx);
    let masuk = b.create_block();
    b.append_block_params_for_function_params(masuk);
    b.switch_to_block(masuk);
    let parameter = b.block_params(masuk).to_vec();
    let (args, kedalaman, keluar) = if register {
        let nol = b.ins().iconst(ptr, 0);
        (nol, parameter[parameter.len() - 1], nol)
    } else { (parameter[0], parameter[1], parameter[2]) };
    let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, (j.maks * 8) as u32, 3));
    let catat = m.declare_func_in_func(impor.catat, b.func);
    let undi = impor.undi.map(|u| m.declare_func_in_func(u, b.func));
    let blok: Vec<Block> = j.blok.iter().map(|_| b.create_block()).collect();
    for (i, d) in j.blok.iter().enumerate().skip(1) {
        for p in &d.params { b.append_block_param(blok[i], tipe_clif(j.tipe(*p))); }
    }
    let callee: Vec<Option<FuncRef>> = id_callee.iter().map(|i| i.map(|i| m.declare_func_in_func(i, b.func))).collect();
    let il = impor.lambat;
    let [muat, ambil, simpan, buang, benar, taruh, langkah, buka, bawaan, biner, tambah_nilai, muat_upval, simpan_upval, teks_bulat, pindah, cabang, tulis_vm, bangun,
        salin, tukar, cek_metode, iter_ada, maju, masuk, masuk_ringan, keluar_f, keluar_ringan, keluar_ringan_angka, keluar_ringan_nilai,
        panjang, panjang_lokal] =
        [il.muat, il.ambil, il.simpan, il.buang, il.benar, il.taruh, il.langkah, il.buka, il.bawaan, il.biner, il.tambah_nilai,
            il.muat_upval, il.simpan_upval, il.teks_bulat, il.pindah, il.langkah_cabang, il.tulis_vm, il.bangun,
            il.salin, il.tukar, il.cek_metode, il.iter_ada, il.maju, il.masuk, il.masuk_ringan, il.keluar, il.keluar_ringan,
            il.keluar_ringan_angka, il.keluar_ringan_nilai, il.panjang, il.panjang_lokal].map(|id| m.declare_func_in_func(id, b.func));
    let lb = Pembantu {
        muat, ambil, simpan, buang, benar, taruh, langkah, buka, bawaan, biner, tambah_nilai, muat_upval, simpan_upval, teks_bulat, pindah, cabang, tulis_vm, bangun,
        salin, tukar, cek_metode, iter_ada, maju, masuk, masuk_ringan, keluar: keluar_f, keluar_ringan, keluar_ringan_angka,
        keluar_ringan_nilai, panjang, panjang_lokal,
        pb: [(Pb::Field, il.field), (Pb::Pola, il.pola), (Pb::PolaLokal, il.pola_lokal), (Pb::Payload, il.payload),
            (Pb::PayloadLokal, il.payload_lokal), (Pb::MuatGlobal, il.muat_global), (Pb::SimpanGlobal, il.simpan_global),
            (Pb::Indeks, il.indeks), (Pb::Baca, impor.baca), (Pb::BacaPecahan, il.baca_pecahan), (Pb::Tulis, impor.tulis),
            (Pb::JenisSama, il.jenis_sama), (Pb::PayloadJenisSama, il.payload_jenis_sama), (Pb::PayloadPanjang, il.payload_panjang),
            (Pb::Karakter, il.karakter), (Pb::KarakterLokal, il.karakter_lokal), (Pb::PayloadAngkaLokal, il.payload_angka), (Pb::FieldLokal, il.field_lokal)].into_iter().map(|(p, id)| (p, m.declare_func_in_func(id, b.func))).collect(),
        tulis_lebar: m.declare_func_in_func(il.tulis_lebar, b.func), buka_lebar: m.declare_func_in_func(il.buka_lebar, b.func),
        sebar: m.declare_func_in_func(il.sebar, b.func), pola_pilih: m.declare_func_in_func(il.pola_pilih, b.func),
        wujud_varian: m.declare_func_in_func(il.wujud_varian, b.func),
        ubah_baca: m.declare_func_in_func(il.ubah_baca, b.func), ubah_siap: m.declare_func_in_func(il.ubah_siap, b.func),
        ubah_tulis_lokal: m.declare_func_in_func(il.ubah_tulis_lokal, b.func),
    };
    let wadah = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let titip = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 72, 3));
    let wadah16 = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
    let langsung: Vec<FuncRef> = id_langsung.iter().map(|i| m.declare_func_in_func(*i, b.func)).collect();
    let ringan = impor.ringan.to_vec();
    let kedalaman1 = b.ins().iadd_imm_s(kedalaman, 1);
    let mut p = Penurun {
        b, j, nilai: vec![None; j.tipe.len()], slot, catat, undi, fid: impor.fid as i64, ptr, keluar, register, callee, kedalaman1,
        lb, wadah, titip, langsung, ringan, wadah16, konst: Default::default(), sebaris: None,
        larik_tb: Vec::new(), rantai_slot: Default::default(), slot_wujud: None,
    };
    // Kedalaman native dijaga di pintu masuk; parameter dimuat dari register atau penyangga argumen.
    let dalam = p.b.create_block();
    p.b.set_cold_block(dalam);
    let terlalu = p.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, kedalaman, KEDALAMAN_MAKS);
    p.b.ins().brif(terlalu, dalam, &[], blok[0], &[]);
    p.b.switch_to_block(dalam);
    p.pulang_status(ULANG);
    p.b.switch_to_block(blok[0]);
    let fl = MemFlagsData::trusted();
    // Elemen larik lokal dibaca/ditulis sebaris (seperti penurun langsung) kalau tata letaknya ditemukan.
    let larik = j.blok.iter().flat_map(|d| &d.inst)
        .any(|i| matches!(i, Inst::Pembantu { pb: Pb::Baca | Pb::BacaPecahan | Pb::Tulis, .. }));
    if let Some(tl) = crate::jit::tata_letak::tata_letak().filter(|_| larik) {
        let ks = p.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
        let pk = p.b.ins().stack_addr(ptr, ks, 0);
        let f = m.declare_func_in_func(il.konteks_mentah, p.b.func);
        p.b.ins().call(f, &[pk]);
        let (vm_p, basis) = (p.b.ins().stack_load(ptr, types::I64, ks, 0), p.b.ins().stack_load(ptr, types::I64, ks, 8));
        p.sebaris = Some((tl, vm_p, basis));
    }
    for (k, pos) in j.posisi_masuk.iter().enumerate() {
        let par = j.blok[0].params[k];
        // `Lebar` dibaca dari stack VM (isinya di sana); yang bukan bulat lagi = ULANG, belum ada yang berubah.
        if j.tipe(par) == Tipe::L {
            let st = p.buka_lebar_ke(*pos);
            let (ulang, terus) = (p.b.create_block(), p.b.create_block());
            p.b.set_cold_block(ulang);
            p.b.ins().brif(st, ulang, &[], terus, &[]);
            p.b.switch_to_block(ulang);
            p.pulang_status(ULANG);
            p.b.switch_to_block(terus);
            let v = p.lebar_dari_wadah();
            p.nilai[par.0 as usize] = Some(v);
            continue;
        }
        let v = if register { parameter[*pos] } else { p.b.ins().load(types::I64, fl, args, (*pos * 8) as i32) };
        let v = if j.tipe(par) == Tipe::F { p.b.ins().bitcast(types::F64, MemFlagsData::new(), v) } else { v };
        p.nilai[par.0 as usize] = Some(v);
    }
    for (i, d) in j.blok.iter().enumerate() {
        if i > 0 {
            p.b.switch_to_block(blok[i]);
            let ps = p.b.block_params(blok[i]).to_vec();
            for (n, v) in d.params.iter().zip(ps) { p.nilai[n.0 as usize] = Some(v); }
        }
        p.larik_tb.clear();
        for ins in &d.inst {
            p.inst(ins);
            // Cuma aritmetika, jaga dan akses larik sebaris yang tidak mengubah penyimpanan larik lokal.
            let murni = matches!(ins, Inst::KonstI { .. } | Inst::KonstF { .. } | Inst::KonstL { .. } | Inst::Biner { .. }
                | Inst::Unar { .. } | Inst::BinerL { .. } | Inst::Pilih { .. } | Inst::JagaJika { .. } | Inst::Sempit { .. }
                | Inst::Pembantu { pb: Pb::Baca | Pb::BacaPecahan | Pb::Tulis, .. });
            if !murni { p.larik_tb.clear(); }
        }
        match d.akhir.as_ref().expect("diperiksa periksa") {
            Akhir::Lompat { ke, args } => {
                let a = p.args(args);
                p.b.ins().jump(blok[ke.0 as usize], &a);
            }
            Akhir::Cabang { kondisi, ya, ya_args, tidak, tidak_args } => {
                let c = p.v(*kondisi);
                let (ya_a, tidak_a) = (p.args(ya_args), p.args(tidak_args));
                p.b.ins().brif(c, blok[ya.0 as usize], &ya_a, blok[tidak.0 as usize], &tidak_a);
            }
            Akhir::Kembali { nilai, hasil } => p.kembali(*nilai, *hasil),
            Akhir::KembaliNilai { pos } => {
                let v = p.b.ins().iconst(types::I64, *pos as i64);
                p.b.ins().store(MemFlagsData::trusted(), v, p.keluar, 0);
                p.pulang_status(BERHASIL_NILAI);
            }
            Akhir::KembaliUnit => p.pulang_status(BERHASIL_UNIT),
            Akhir::CabangVm { ip, jenis, t, tumpah, hasil_bool, isi, taruh, tujuan } => {
                p.tumpah(isi, taruh);
                let args = [
                    p.i64c(*ip as i64), p.i64c(*jenis), p.i64c(*t as i64), p.i64c(*tumpah as i64),
                    p.b.ins().stack_addr(p.ptr, p.slot, 0), p.i64c(*hasil_bool as i64),
                ];
                let c = p.b.ins().call(p.lb.cabang, &args);
                let h = p.b.inst_results(c)[0];
                let salah = p.b.ins().icmp_imm_s(IntCC::SignedLessThan, h, 0);
                p.gagal_jika(salah);
                let pilih = if *hasil_bool { h } else { p.b.ins().icmp_imm_s(IntCC::Equal, h, tujuan[0].2 as i64) };
                let (ya, tidak) = (p.args(&tujuan[0].1), p.args(&tujuan[1].1));
                p.b.ins().brif(pilih, blok[tujuan[0].0 .0 as usize], &ya, blok[tujuan[1].0 .0 as usize], &tidak);
            }
            Akhir::Keluar { keadaan } => p.deopt(keadaan, 0),
        }
    }
    p.b.seal_all_blocks();
    p.b.finalize(m.target_config());
}

impl Penurun<'_, '_> {
    fn v(&self, n: Nilai) -> Value { self.nilai[n.0 as usize].expect("didefinisikan lebih dulu (periksa)") }

    fn tetapkan(&mut self, n: Nilai, v: Value) { self.nilai[n.0 as usize] = Some(v); }

    fn i64c(&mut self, v: i64) -> Value { self.b.ins().iconst(types::I64, v) }

    fn tulis_lebar(&mut self, pos: usize, n: Nilai) {
        let v = self.v(n);
        let (lo, hi) = self.b.ins().isplit(v);
        let p = self.i64c(pos as i64);
        self.b.ins().call(self.lb.tulis_lebar, &[p, lo, hi]);
    }

    /// `lambat::buka_lebar` atas posisi `pos` ke `wadah16`: statusnya (bukan 0 = bukan bulat).
    fn buka_lebar_ke(&mut self, pos: usize) -> Value {
        let (p, w) = (self.i64c(pos as i64), self.b.ins().stack_addr(self.ptr, self.wadah16, 0));
        let c = self.b.ins().call(self.lb.buka_lebar, &[p, w]);
        self.b.inst_results(c)[0]
    }

    fn lebar_dari_wadah(&mut self) -> Value {
        let lo = self.b.ins().stack_load(self.ptr, types::I64, self.wadah16, 0);
        let hi = self.b.ins().stack_load(self.ptr, types::I64, self.wadah16, 8);
        self.b.ins().iconcat(lo, hi)
    }

    fn nol128(&mut self) -> Value { let z = self.i64c(0); self.b.ins().sextend(types::I128, z) }

    fn biner_l(&mut self, op: OpL, a: Nilai, y: Nilai, jaga: Option<&Jaga>, jaga_muat: Option<&Jaga>) -> Value {
        let (x, yv) = (self.v(a), self.v(y));
        match op {
            OpL::Tambah | OpL::Kurang => {
                let h = if op == OpL::Tambah { self.b.ins().iadd(x, yv) } else { self.b.ins().isub(x, yv) };
                if let Some(j) = jaga {
                    // Luapan 128 bit: tanda operand sama tapi tanda hasil berbeda.
                    let (a1, a2) = if op == OpL::Tambah { (self.b.ins().bxor(x, h), self.b.ins().bxor(yv, h)) }
                        else { (self.b.ins().bxor(x, yv), self.b.ins().bxor(x, h)) };
                    let m = self.b.ins().band(a1, a2);
                    let z = self.nol128();
                    let luap = self.b.ins().icmp(IntCC::SignedLessThan, m, z);
                    self.jaga(luap, j);
                }
                h
            }
            OpL::Kali => self.b.ins().imul(x, yv),
            OpL::Dan => self.b.ins().band(x, yv),
            OpL::Atau => self.b.ins().bor(x, yv),
            OpL::Xor => self.b.ins().bxor(x, yv),
            OpL::DanBukan => self.b.ins().band_not(x, yv),
            OpL::Cmp(c) => { let h = self.b.ins().icmp(Self::banding_i(c), x, yv); self.b.ins().uextend(types::I64, h) }
            OpL::Bagi | OpL::Sisa => {
                let (x64, y64) = (self.b.ins().ireduce(types::I64, x), self.b.ins().ireduce(types::I64, yv));
                let (xe, ye) = (self.b.ins().sextend(types::I128, x64), self.b.ins().sextend(types::I128, y64));
                let (bx, by) = (self.b.ins().icmp(IntCC::NotEqual, xe, x), self.b.ins().icmp(IntCC::NotEqual, ye, yv));
                let tak_muat = self.b.ins().bor(bx, by);
                self.jaga(tak_muat, jaga_muat.expect("bagi 128 bit dijaga"));
                let nol = self.b.ins().icmp_imm_s(IntCC::Equal, y64, 0);
                let min1 = self.b.ins().icmp_imm_s(IntCC::Equal, y64, -1);
                let tolak = self.b.ins().bor(nol, min1);
                self.jaga(tolak, jaga.expect("bagi 128 bit dijaga"));
                if op == OpL::Sisa { self.b.ins().srem(x64, y64) } else { self.b.ins().sdiv(x64, y64) }
            }
            OpL::GeserKiri | OpL::GeserKanan => {
                // Geseran tetap 1–63: dua geser 64 bit atas belahannya (langkah 76), tanpa jaga.
                if let Some(kk @ 1..=63) = self.konst.get(&y.0).copied() {
                    let (lo, hi) = self.b.ins().isplit(x);
                    let (nlo, nhi) = if op == OpL::GeserKiri {
                        let (a1, a2) = (self.b.ins().ishl_imm_u(hi, kk), self.b.ins().ushr_imm_u(lo, 64 - kk));
                        (self.b.ins().ishl_imm_u(lo, kk), self.b.ins().bor(a1, a2))
                    } else {
                        let (a1, a2) = (self.b.ins().ushr_imm_u(lo, kk), self.b.ins().ishl_imm_u(hi, 64 - kk));
                        (self.b.ins().bor(a1, a2), self.b.ins().sshr_imm_u(hi, kk))
                    };
                    return self.b.ins().iconcat(nlo, nhi);
                }
                let salah = self.b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, yv, 127);
                self.jaga(salah, jaga.expect("geser 128 bit dijaga"));
                if op == OpL::GeserKiri { self.b.ins().ishl(x, yv) } else { self.b.ins().sshr(x, yv) }
            }
        }
    }

    /// Posisi `pos` milik VM dibuka jadi angka berkode `kode` (`lambat::buka`); jenis lain = deopt `jaga`. Nilainya
    /// bertipe nilai `n`.
    fn buka_nilai(&mut self, pos: usize, kode: i32, jaga: &Jaga, n: Nilai) -> Value {
        let p = self.i64c(pos as i64);
        let k = self.b.ins().iconst(types::I32, kode as i64);
        let w = self.b.ins().stack_addr(self.ptr, self.wadah, 0);
        let c = self.b.ins().call(self.lb.buka, &[p, k, w]);
        let st = self.b.inst_results(c)[0];
        self.jaga(st, jaga);
        let v = self.b.ins().stack_load(self.ptr, types::I64, self.wadah, 0);
        if self.j.tipe(n) == Tipe::F { self.b.ins().bitcast(types::F64, MemFlagsData::new(), v) } else { v }
    }

    /// Isi native ke slot rekaman dan posisi maya ke stack VM, sebelum pembantu yang membaca stack VM.
    fn tumpah(&mut self, isi: &[(usize, Nilai)], taruh: &[(usize, i64)]) {
        for (pos, n) in isi {
            // `Lebar` ditulis ke stack VM; pembantu membacanya dari sana.
            if self.j.tipe(*n) == Tipe::L { self.tulis_lebar(*pos, *n); continue; }
            let v = self.bit(*n);
            self.b.ins().stack_store(self.ptr, v, self.slot, (*pos * 8) as i32);
        }
        for (pos, alamat) in taruh {
            let (p, a) = (self.i64c(*pos as i64), self.i64c(*alamat));
            self.b.ins().call(self.lb.taruh, &[p, a]);
        }
    }

    /// `st` bukan nol = instruksi yang dijalankan VM gagal: kembali `GAGAL`.
    fn gagal_jika(&mut self, st: Value) {
        let (gagal, terus) = (self.b.create_block(), self.b.create_block());
        self.b.set_cold_block(gagal);
        self.b.ins().brif(st, gagal, &[], terus, &[]);
        self.b.switch_to_block(gagal);
        self.pulang_status(GAGAL);
        self.b.switch_to_block(terus);
    }

    /// Isi native sebagai bit `i64` (pecahan dibaca bitnya).
    fn bit(&mut self, n: Nilai) -> Value {
        let v = self.v(n);
        if self.j.tipe(n) == Tipe::F { self.b.ins().bitcast(types::I64, MemFlagsData::new(), v) } else { v }
    }

    fn args(&self, a: &[Nilai]) -> Vec<cranelift_codegen::ir::BlockArg> { a.iter().map(|n| self.v(*n).into()).collect() }

    fn pulang_status(&mut self, st: i64) {
        let s = self.b.ins().iconst(types::I32, st);
        if self.register { let nol = self.b.ins().iconst(types::I64, 0); self.b.ins().return_(&[s, nol]); } else { self.b.ins().return_(&[s]); }
    }

    /// Isi native keadaan ke slot rekaman, `deopt::catat`, lalu kembali `DEOPT`.
    /// Rekaman bingkai terdalam lebih dulu: callee yang disisipkan (`Keadaan::induk`, bit `SISIP`) lalu pemanggilnya.
    fn deopt(&mut self, k: &Keadaan, bendera: i64) {
        // Varian berbayangan yang isi stack VM-nya basi ditulis dulu, jadi `pulihkan` membacanya seperti biasa.
        let mut kini = Some(k);
        while let Some(k) = kini {
            for m in &k.maya { self.wujud(m, k.geser); }
            kini = k.induk.as_deref();
        }
        let mut kini = Some(k);
        let mut pertama = true;
        while let Some(k) = kini {
            for (i, isi) in k.isi.iter().enumerate() {
                let Some(n) = isi else { continue };
                // `Lebar` langsung ke stack VM; `deopt::pulihkan` membacanya dari sana.
                if self.j.tipe(*n) == Tipe::L { self.tulis_lebar(k.geser + i, *n); continue; }
                let v = self.v(*n);
                let v = if self.j.tipe(*n) == Tipe::F { self.b.ins().bitcast(types::I64, MemFlagsData::new(), v) } else { v };
                self.b.ins().stack_store(self.ptr, v, self.slot, (i * 8) as i32);
            }
            let pn = self.b.ins().stack_addr(self.ptr, self.slot, 0);
            let fid = self.b.ins().iconst(types::I32, k.fid.map_or(self.fid, |f| f as i64));
            let tanda = if pertama { bendera } else { 0 } | if k.induk.is_some() { crate::jit::turun::SISIP } else { 0 };
            let ipv = self.b.ins().iconst(types::I32, k.ip as i64 | tanda);
            let n = self.b.ins().iconst(types::I32, k.isi.len() as i64);
            self.b.ins().call(self.catat, &[fid, ipv, pn, n]);
            pertama = false;
            kini = k.induk.as_deref();
        }
        self.pulang_status(DEOPT);
    }

    /// `lambat::wujud_varian` atas bayangan `m` (posisinya digeser `geser`).
    fn wujud(&mut self, m: &Maya, geser: usize) {
        let s = *self.slot_wujud.get_or_insert_with(|| self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 64, 3)));
        for (k, n) in m.muatan.iter().enumerate().take(8) {
            let v = self.bit(*n);
            self.b.ins().stack_store(self.ptr, v, s, (k * 8) as i32);
        }
        let isi = self.b.ins().stack_addr(self.ptr, s, 0);
        let tag = self.v(m.tag);
        let bentuk = if m.tuple { 1i64 << 63 } else { 0 };
        let (pos, tabel, basi, pecahan) = (self.i64c((m.pos + geser) as i64), self.i64c(m.tabel), self.i64c(m.basi as i64), self.i64c(m.pecahan as i64 | bentuk));
        self.b.ins().call(self.lb.wujud_varian, &[pos, tabel, tag, basi, pecahan, isi]);
    }

    /// `cond` bukan nol = deopt; sesudahnya builder di blok lanjutan.
    fn jaga(&mut self, cond: Value, j: &Jaga) {
        let (d, terus) = (self.b.create_block(), self.b.create_block());
        self.b.set_cold_block(d);
        self.b.ins().brif(cond, d, &[], terus, &[]);
        self.b.switch_to_block(d);
        self.deopt(&j.keadaan, if j.spek { GAGAL_SPEK } else { 0 } | if j.puncak_vm { PUNCAK_VM } else { 0 });
        self.b.switch_to_block(terus);
    }

    fn kembali(&mut self, n: Nilai, h: Hasil) {
        let v = self.v(n);
        let v = if self.j.tipe(n) == Tipe::F { self.b.ins().bitcast(types::I64, MemFlagsData::new(), v) } else { v };
        let st = match h { Hasil::Bool => BERHASIL_BOOL, Hasil::Pecahan => BERHASIL_PECAHAN, Hasil::Bulat => BERHASIL };
        let st = self.b.ins().iconst(types::I32, st);
        if self.register { self.b.ins().return_(&[st, v]); } else {
            self.b.ins().store(MemFlagsData::trusted(), v, self.keluar, 0);
            self.b.ins().return_(&[st]);
        }
    }

    /// `Inst::Pembantu` lewat panggilan pembantunya (jalur umum, dan jalur lambat larik sebaris).
    fn lambat_inst(&mut self, ins: &Inst) {
        let Inst::Pembantu { pb, args, hasil, gagal, keluar } = ins else { unreachable!("cuma Pembantu") };
        let f = self.lb.pb.iter().find(|(p, _)| p == pb).expect("pembantu terdaftar").1;
        let argv: Vec<Value> = args.iter().map(|a| match a {
            Arg::I64(v) => self.i64c(*v),
            Arg::I32(v) => self.b.ins().iconst(types::I32, *v as i64),
            Arg::Isi(n) => self.bit(*n),
            Arg::Wadah => self.b.ins().stack_addr(self.ptr, self.wadah, 0),
        }).collect();
        let c = self.b.ins().call(f, &argv);
        let st = self.b.inst_results(c)[0];
        if let Some((uji, j)) = gagal {
            let cond = match uji {
                Uji::BukanNol => st,
                Uji::Sama(k) => self.b.ins().icmp_imm_u(IntCC::Equal, st, *k),
                Uji::Negatif => self.b.ins().icmp_imm_s(IntCC::SignedLessThan, st, 0),
            };
            self.jaga(cond, j);
        }
        if let Some(h) = hasil {
            let v = if self.b.func.dfg.value_type(st) == types::I64 { st } else { self.b.ins().uextend(types::I64, st) };
            self.tetapkan(*h, v);
        }
        if let Some(k) = keluar {
            let v = self.b.ins().stack_load(self.ptr, types::I64, self.wadah, 0);
            let v = if self.j.tipe(*k) == Tipe::F { self.b.ins().bitcast(types::F64, MemFlagsData::new(), v) } else { v };
            self.tetapkan(*k, v);
        }
    }

    fn inst(&mut self, ins: &Inst) {
        match ins {
            Inst::KonstI { hasil, nilai } => {
                let v = self.b.ins().iconst(types::I64, *nilai);
                self.konst.insert(hasil.0, *nilai);
                self.tetapkan(*hasil, v);
            }
            Inst::KonstL { hasil, nilai } => {
                let (lo, hi) = (self.i64c(*nilai as i64), self.i64c((*nilai >> 64) as i64));
                let v = self.b.ins().iconcat(lo, hi);
                self.tetapkan(*hasil, v);
            }
            Inst::BinerL { hasil, op, a, b: y, jaga, jaga_muat } => {
                let v = self.biner_l(*op, *a, *y, jaga.as_ref(), jaga_muat.as_ref());
                self.tetapkan(*hasil, v);
            }
            Inst::Sempit { hasil, a, jaga } => {
                let x = self.v(*a);
                let lo = self.b.ins().ireduce(types::I64, x);
                let balik = self.b.ins().sextend(types::I128, lo);
                let lepas = self.b.ins().icmp(IntCC::NotEqual, balik, x);
                self.jaga(lepas, jaga);
                self.tetapkan(*hasil, lo);
            }
            Inst::TulisLebar { pos, nilai } => self.tulis_lebar(*pos, *nilai),
            Inst::Rumus { rumus, lokal, args, tulis, hasil, jaga } => {
                let jf = self.j;
                let rm = &jf.rumus[*rumus];
                let situs = crate::jit::lambat::SitusUbah::bocor(rm) as *const _ as i64;
                let lebar = (rm.baca.len().max(rm.tulis.len()).max(1) * 8) as u32;
                let wb = self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, lebar, 3));
                let (lv, sv, pk) = (self.i64c(*lokal as i64), self.i64c(situs), self.b.ins().stack_addr(self.ptr, wb, 0));
                let arg: Vec<Value> = args.iter().zip(&rm.sumber).map(|(a, s)| match (a, s) {
                    (Some(n), _) => self.v(*n),
                    (None, crate::jit::layak::Sumber::Konst(x)) => self.b.ins().f64const(*x),
                    (None, _) => unreachable!("argumen lokal tanpa nilai"),
                }).collect();
                let tl = crate::jit::tata_letak::tata_letak().filter(|_| *tulis);
                // Pembaruan atas struct milik tunggal (`ubah_siap`): pecahan ditulis langsung ke isi field-nya.
                let isi = if tl.is_some() {
                    let c = self.b.ins().call(self.lb.ubah_siap, &[lv, sv, pk]);
                    let isi = self.b.inst_results(c)[0];
                    let gagal = self.b.ins().icmp_imm_u(IntCC::Equal, isi, 0);
                    self.jaga(gagal, jaga);
                    Some(isi)
                } else {
                    let c = self.b.ins().call(self.lb.ubah_baca, &[lv, sv, pk]);
                    let st = self.b.inst_results(c)[0];
                    self.jaga(st, jaga);
                    None
                };
                let h = crate::jit::turun::rumus_hitung(&mut self.b, rm, wb, self.ptr, |_, j| arg[j - 1]);
                match (tl, isi) {
                    (Some(tl), Some(isi)) => for (j, &(q, _)) in rm.tulis.iter().enumerate() {
                        let x = self.b.ins().stack_load(self.ptr, types::I64, wb, (j * 8) as i32);
                        self.b.ins().store(MemFlagsData::trusted(), x, isi, (q * tl.ukuran + tl.isi_float) as i32);
                    },
                    _ if *tulis => { let pk = self.b.ins().stack_addr(self.ptr, wb, 0); self.b.ins().call(self.lb.ubah_tulis_lokal, &[lv, sv, pk]); }
                    _ => {}
                }
                if let Some(hn) = hasil {
                    let v = h.expect("rumus nilai");
                    let v = self.b.ins().bitcast(types::F64, MemFlagsData::new(), v);
                    self.tetapkan(*hn, v);
                }
            }
            Inst::Rantai { hasil, rantai, urutan, lokal, situs, panggil, jaga } => {
                let slot = *self.rantai_slot.entry(*rantai)
                    .or_insert_with(|| self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3)));
                // Lengan berikutnya cuma dicapai dari kegagalan lengan sebelumnya (`rantai_pola`): slotnya sudah terisi.
                if *urutan == 0 || *panggil {
                    let (l, sp) = (self.i64c(*lokal as i64), self.i64c(*situs));
                    let pilih = self.b.ins().call(self.lb.pola_pilih, &[l, sp]);
                    let c = self.b.inst_results(pilih)[0];
                    let c = self.b.ins().uextend(types::I64, c);
                    self.b.ins().stack_store(self.ptr, c, slot, 0);
                }
                let idx = self.b.ins().stack_load(self.ptr, types::I64, slot, 0);
                let salah = self.b.ins().icmp_imm_u(IntCC::Equal, idx, 1000 + *urutan as i64);
                self.jaga(salah, jaga);
                let v = self.b.ins().icmp_imm_u(IntCC::Equal, idx, *urutan as i64);
                let v = self.b.ins().uextend(types::I64, v);
                self.tetapkan(*hasil, v);
            }
            Inst::Sebar { pos, pulang, keluarga, jaga } => {
                let p = self.i64c(*pos as i64);
                let pk = self.i64c(*pos as i64 | (*keluarga as i64) << 32);
                let c = self.b.ins().call(self.lb.sebar, &[pk]);
                let c = self.b.inst_results(c)[0];
                // 1 = `Err`/`None`: fungsinya kembali dengan nilai itu (masuk dari VM).
                if *pulang {
                    let (balik, terus) = (self.b.create_block(), self.b.create_block());
                    let satu = self.b.ins().icmp_imm_u(IntCC::Equal, c, 1);
                    self.b.ins().brif(satu, balik, &[], terus, &[]);
                    self.b.switch_to_block(balik);
                    self.b.ins().store(MemFlagsData::trusted(), p, self.keluar, 0);
                    self.pulang_status(BERHASIL_NILAI);
                    self.b.switch_to_block(terus);
                }
                self.jaga(c, jaga);
            }
            Inst::BukaLebar { hasil, pos, jaga } => {
                let st = self.buka_lebar_ke(*pos);
                self.jaga(st, jaga);
                let v = self.lebar_dari_wadah();
                self.tetapkan(*hasil, v);
            }
            Inst::KonstF { hasil, nilai } => { let v = self.b.ins().f64const(*nilai); self.tetapkan(*hasil, v); }
            Inst::Biner { hasil, op, a, b: y, jaga } => { let v = self.biner(*op, *a, *y, jaga.as_ref()); self.tetapkan(*hasil, v); }
            Inst::Unar { hasil, op, a, jaga } => { let v = self.unar(*op, *a, jaga.as_ref()); self.tetapkan(*hasil, v); }
            Inst::Pilih { hasil, jika, ya, tidak } => {
                let (c, y, t) = (self.v(*jika), self.v(*ya), self.v(*tidak));
                let v = self.b.ins().select(c, y, t);
                self.tetapkan(*hasil, v);
            }
            Inst::JagaJika { kondisi, jaga } => { let c = self.v(*kondisi); self.jaga(c, jaga); }
            Inst::Panggil { hasil, callee, args, di, sesudah } => {
                let fref = self.callee[*callee].expect("callee bulat dikompilasi");
                let mut argv: Vec<Value> = args.iter().map(|a| self.v(*a)).collect();
                argv.push(self.kedalaman1);
                let c = self.b.ins().call(fref, &argv);
                let (st, h) = (self.b.inst_results(c)[0], self.b.inst_results(c)[1]);
                let (terus, gagal, ulang, deopt) = (self.b.create_block(), self.b.create_block(), self.b.create_block(), self.b.create_block());
                for bl in [gagal, ulang, deopt] { self.b.set_cold_block(bl); }
                self.b.ins().brif(st, gagal, &[], terus, &[]);
                self.b.switch_to_block(gagal);
                let u = self.b.ins().icmp_imm_s(IntCC::Equal, st, ULANG);
                self.b.ins().brif(u, ulang, &[], deopt, &[]);
                self.b.switch_to_block(ulang);
                self.deopt(di, 0);
                self.b.switch_to_block(deopt);
                self.deopt(sesudah, 0);
                self.b.switch_to_block(terus);
                self.tetapkan(*hasil, h);
            }
            Inst::Muat { lokal, ke, pindah } => {
                let (d, k) = (self.i64c(*lokal as i64), self.i64c(*ke as i64));
                self.b.ins().call(if *pindah { self.lb.ambil } else { self.lb.muat }, &[d, k]);
            }
            Inst::Simpan { ke, dari, nilai, kode, buang } => {
                let v = match nilai { Some(n) => self.bit(*n), None => self.i64c(0) };
                let (k, d) = (self.i64c(*ke as i64), self.i64c(*dari as i64));
                let (kd, bg) = (self.b.ins().iconst(types::I32, *kode as i64), self.b.ins().iconst(types::I32, *buang as i64));
                self.b.ins().call(self.lb.simpan, &[k, d, v, kd, bg]);
            }
            Inst::Buang { pos } => { let p = self.i64c(*pos as i64); self.b.ins().call(self.lb.buang, &[p]); }
            Inst::Benar { hasil, pos, lepas } => {
                let p = self.i64c(*pos as i64);
                let l = self.b.ins().iconst(types::I32, *lepas as i64);
                let c = self.b.ins().call(self.lb.benar, &[p, l]);
                let c = self.b.inst_results(c)[0];
                let v = self.b.ins().uextend(types::I64, c);
                self.tetapkan(*hasil, v);
            }
            Inst::Taruh { pos, alamat } => {
                let (p, a) = (self.i64c(*pos as i64), self.i64c(*alamat));
                self.b.ins().call(self.lb.taruh, &[p, a]);
            }
            Inst::Langkah { ip, jenis, t, tumpah, sesudah, isi, taruh } => {
                self.tumpah(isi, taruh);
                let args = [
                    self.i64c(*ip as i64), self.i64c(*jenis), self.i64c(*t as i64), self.i64c(*tumpah as i64),
                    self.i64c(*sesudah as i64), self.b.ins().stack_addr(self.ptr, self.slot, 0),
                ];
                let c = self.b.ins().call(self.lb.langkah, &args);
                let st = self.b.inst_results(c)[0];
                self.gagal_jika(st);
            }
            Inst::Bawaan { ip, jenis, t, argc, fungsi, isi, taruh } => {
                self.tumpah(isi, taruh);
                let args = [
                    self.i64c(*ip as i64), self.i64c(*jenis), self.i64c(*t as i64), self.i64c(*argc),
                    self.b.ins().stack_addr(self.ptr, self.slot, 0), self.i64c(*fungsi),
                ];
                let c = self.b.ins().call(self.lb.bawaan, &args);
                let st = self.b.inst_results(c)[0];
                self.gagal_jika(st);
            }
            Inst::BinerVm { hasil, tambah, ip, jenis, t, isi, taruh } => {
                self.tumpah(isi, taruh);
                let args = [self.i64c(*ip as i64), self.i64c(*jenis), self.i64c(*t as i64), self.b.ins().stack_addr(self.ptr, self.slot, 0)];
                let c = self.b.ins().call(if *tambah { self.lb.tambah_nilai } else { self.lb.biner }, &args);
                let st = self.b.inst_results(c)[0];
                // 2 = galat; banding: 0/1 hasilnya.
                let salah = self.b.ins().icmp_imm_u(IntCC::Equal, st, 2);
                self.gagal_jika(salah);
                if let Some(h) = hasil { let v = self.b.ins().uextend(types::I64, st); self.tetapkan(*h, v); }
            }
            Inst::TeksBulat { pos, n } => {
                let (p, v) = (self.i64c(*pos as i64), self.v(*n));
                self.b.ins().call(self.lb.teks_bulat, &[p, v]);
            }
            Inst::TulisVm { pos, nilai, kode } => {
                let v = match nilai { Some(n) => self.bit(*n), None => self.i64c(0) };
                let p = self.i64c(*pos as i64);
                let k = self.b.ins().iconst(types::I32, *kode as i64);
                self.b.ins().call(self.lb.tulis_vm, &[p, v, k]);
            }
            Inst::Bangun { situs, jenis, t, n, isi, taruh } => {
                self.tumpah(isi, taruh);
                let args = [
                    self.i64c(*situs), self.i64c(*jenis), self.i64c(*t as i64), self.i64c(*n as i64),
                    self.b.ins().stack_addr(self.ptr, self.slot, 0),
                ];
                self.b.ins().call(self.lb.bangun, &args);
            }
            Inst::Salin { dari, ke } => {
                let (d, k) = (self.i64c(*dari as i64), self.i64c(*ke as i64));
                self.b.ins().call(self.lb.salin, &[d, k]);
            }
            Inst::Tukar { x, y } => {
                let (a, b) = (self.i64c(*x as i64), self.i64c(*y as i64));
                self.b.ins().call(self.lb.tukar, &[a, b]);
            }
            Inst::CekMetode { pos, label, jaga } => {
                let (p, l) = (self.i64c(*pos as i64), self.i64c(*label));
                let c = self.b.ins().call(self.lb.cek_metode, &[p, l]);
                let c = self.b.inst_results(c)[0];
                self.jaga(c, jaga);
            }
            Inst::IterAda { hasil, slot, jaga } => {
                let p = self.i64c(*slot as i64);
                let c = self.b.ins().call(self.lb.iter_ada, &[p]);
                let c = self.b.inst_results(c)[0];
                let salah = self.b.ins().icmp_imm_u(IntCC::Equal, c, 2);
                self.jaga(salah, jaga);
                let v = self.b.ins().uextend(types::I64, c);
                self.tetapkan(*hasil, v);
            }
            Inst::Langsung { n, ip, jenis, t, a, fungsi, isi, taruh, args, basi, hasil } => {
                use crate::jit::turun::{BERHASIL, BERHASIL_PECAHAN};
                self.tumpah(isi, taruh);
                let ringan = *fungsi != 0 && self.ringan.get(*n).copied().unwrap_or(false);
                let angka = match (ringan, hasil.as_ref().map(|h| h.1)) {
                    (true, Some(1)) => Some(BERHASIL),
                    (true, Some(5)) => Some(BERHASIL_PECAHAN),
                    _ => None,
                };
                let argv0 = [
                    self.i64c(*ip as i64), self.i64c(*jenis), self.i64c(*t as i64), self.i64c(*a as i64),
                    self.b.ins().stack_addr(self.ptr, self.slot, 0), self.i64c(*fungsi), self.b.ins().stack_addr(self.ptr, self.titip, 0),
                ];
                let c = self.b.ins().call(if ringan { self.lb.masuk_ringan } else { self.lb.masuk }, &argv0);
                let m0 = self.b.inst_results(c)[0];
                let (gagal, cek, asli, akhir) = (self.b.create_block(), self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.b.set_cold_block(gagal);
                self.b.append_block_param(akhir, types::I32);
                let galat = self.b.ins().icmp_imm_u(IntCC::Equal, m0, 2);
                self.b.ins().brif(galat, gagal, &[], cek, &[]);
                self.b.switch_to_block(cek);
                let ulang = self.b.ins().iconst(types::I32, ULANG);
                self.b.ins().brif(m0, akhir, &[ulang.into()], asli, &[]);
                self.b.switch_to_block(asli);
                let argv = self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, ((*a).max(1) * 8) as u32, 3));
                for (k2, n2) in args {
                    let v = self.bit(*n2);
                    self.b.ins().stack_store(self.ptr, v, argv, (*k2 * 8) as i32);
                }
                let (pa, ph) = (self.b.ins().stack_addr(self.ptr, argv, 0), self.b.ins().stack_addr(self.ptr, self.wadah, 0));
                let c = self.b.ins().call(self.langsung[*n], &[pa, self.kedalaman1, ph]);
                let st = self.b.inst_results(c)[0];
                self.b.ins().jump(akhir, &[st.into()]);
                self.b.switch_to_block(gagal);
                self.pulang_status(GAGAL);
                self.b.switch_to_block(akhir);
                let st = self.b.block_params(akhir)[0];
                let st = self.b.ins().sextend(types::I64, st);
                let h = self.b.ins().stack_load(self.ptr, types::I64, self.wadah, 0);
                let tp = self.b.ins().stack_addr(self.ptr, self.titip, 0);
                let c = match angka {
                    Some(kode) => { let kv = self.i64c(kode); self.b.ins().call(self.lb.keluar_ringan_angka, &[st, h, tp, kv]) }
                    None if ringan && hasil.is_none() => self.b.ins().call(self.lb.keluar_ringan_nilai, &[st, h, tp]),
                    None => self.b.ins().call(if ringan { self.lb.keluar_ringan } else { self.lb.keluar }, &[st, h, tp]),
                };
                let r0 = self.b.inst_results(c)[0];
                let galat = self.b.ins().icmp_imm_u(IntCC::Equal, r0, 2);
                self.gagal_jika(galat);
                // Pemanggil basi (callee deopt sesudah spekulasi lain gagal): deopt sesudah panggilan, hasil milik VM.
                let basi_c = self.b.ins().icmp_imm_u(IntCC::Equal, r0, 3);
                self.jaga(basi_c, &Jaga { keadaan: basi.clone(), spek: false, puncak_vm: true });
                if let Some((hn, kode, jg)) = hasil {
                    let tipe = tipe_clif(self.j.tipe(*hn));
                    let (cepat, biasa, lanjut) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                    self.b.append_block_param(lanjut, tipe);
                    let st_kode = if *kode == 1 { BERHASIL } else { BERHASIL_PECAHAN };
                    let cocok = self.b.ins().icmp_imm_u(IntCC::Equal, st, st_kode);
                    self.b.ins().brif(cocok, cepat, &[], biasa, &[]);
                    self.b.switch_to_block(cepat);
                    let v = if self.j.tipe(*hn) == Tipe::F { self.b.ins().bitcast(types::F64, MemFlagsData::new(), h) } else { h };
                    self.b.ins().jump(lanjut, &[v.into()]);
                    self.b.switch_to_block(biasa);
                    let v = self.buka_nilai(*t - *a - 1, *kode, jg, *hn);
                    self.b.ins().jump(lanjut, &[v.into()]);
                    self.b.switch_to_block(lanjut);
                    let v = self.b.block_params(lanjut)[0];
                    self.tetapkan(*hn, v);
                }
            }
            Inst::Panjang { hasil, pos, lokal, jaga } => {
                let (p, w) = (self.i64c(*pos as i64), self.b.ins().stack_addr(self.ptr, self.wadah, 0));
                let c = self.b.ins().call(if *lokal { self.lb.panjang_lokal } else { self.lb.panjang }, &[p, w]);
                let st = self.b.inst_results(c)[0];
                self.jaga(st, jaga);
                let v = self.b.ins().stack_load(self.ptr, types::I64, self.wadah, 0);
                self.tetapkan(*hasil, v);
            }
            Inst::Pembantu { pb: pb @ (Pb::Baca | Pb::BacaPecahan | Pb::Tulis), args, gagal: Some(_), .. } if self.sebaris.is_some() => {
                let (tl, vm_p, basis) = self.sebaris.expect("baru diperiksa");
                let Arg::I64(rel) = args[0] else { unreachable!("lokal larik konstan") };
                let Arg::Isi(i) = args[1] else { unreachable!("indeks native") };
                let iv = self.v(i);
                let (lambat, selesai) = (self.b.create_block(), self.b.create_block());
                self.b.set_cold_block(lambat);
                if *pb == Pb::Tulis {
                    let (Arg::Isi(n), Arg::I32(kode)) = (args[2], args[3]) else { unreachable!("nilai dan kode") };
                    let vv = self.bit(n);
                    // Cuma bulat/bool/pecahan yang ditulis sebaris; jenis lain langsung ke pembantu.
                    let tb = if matches!(kode, 1 | 2 | 5) {
                        let tb = crate::jit::turun::tembolok_larik(&mut self.b, &mut self.larik_tb, rel as usize);
                        crate::jit::turun::tulis_sebaris(&mut self.b, tl, vm_p, basis, rel as usize, iv, vv, kode as i64, lambat, Some(tb));
                        self.b.ins().jump(selesai, &[]);
                        Some(tb)
                    } else {
                        // Pembantu bisa mengganti penyimpanannya: tembolok lokal ini tidak berlaku lagi.
                        self.larik_tb.retain(|(l, ..)| *l != rel as usize);
                        self.b.ins().jump(lambat, &[]);
                        None
                    };
                    self.b.switch_to_block(lambat);
                    if let Some(tb) = tb { crate::jit::turun::larik_tak_pasti(&mut self.b, tb); }
                    self.lambat_inst(ins);
                    self.b.ins().jump(selesai, &[]);
                    self.b.switch_to_block(selesai);
                    return;
                }
                let Inst::Pembantu { keluar: Some(k), .. } = ins else { unreachable!("baca berhasil") };
                self.b.append_block_param(selesai, types::I64);
                let tb = crate::jit::turun::tembolok_larik(&mut self.b, &mut self.larik_tb, rel as usize);
                let v = crate::jit::turun::baca_sebaris(&mut self.b, tl, vm_p, basis, rel as usize, iv, *pb == Pb::BacaPecahan, lambat, Some(tb));
                self.b.ins().jump(selesai, &[v.into()]);
                self.b.switch_to_block(lambat);
                crate::jit::turun::larik_tak_pasti(&mut self.b, tb);
                self.lambat_inst(ins);
                let v = self.v(*k);
                let v = if self.j.tipe(*k) == Tipe::F { self.b.ins().bitcast(types::I64, MemFlagsData::new(), v) } else { v };
                self.b.ins().jump(selesai, &[v.into()]);
                self.b.switch_to_block(selesai);
                let v = self.b.block_params(selesai)[0];
                let v = if self.j.tipe(*k) == Tipe::F { self.b.ins().bitcast(types::F64, MemFlagsData::new(), v) } else { v };
                self.tetapkan(*k, v);
            }
            Inst::Pembantu { .. } => self.lambat_inst(ins),
            Inst::Wujud(m) => self.wujud(m, 0),
            Inst::Maju { slot, jaga } => {
                let p = self.i64c(*slot as i64);
                let c = self.b.ins().call(self.lb.maju, &[p]);
                let c = self.b.inst_results(c)[0];
                self.jaga(c, jaga);
            }
            Inst::Pindah { dari, ke } => {
                let (d, k) = (self.i64c(*dari as i64), self.i64c(*ke as i64));
                self.b.ins().call(self.lb.pindah, &[d, k]);
            }
            Inst::MuatUpval { idx, ke, jaga } => {
                let (i, k) = (self.i64c(*idx as i64), self.i64c(*ke as i64));
                let c = self.b.ins().call(self.lb.muat_upval, &[i, k]);
                let c = self.b.inst_results(c)[0];
                self.jaga(c, jaga);
            }
            Inst::SimpanUpval { idx, dari, nilai, kode, jaga } => {
                let v = match nilai { Some(n) => self.bit(*n), None => self.i64c(0) };
                let (i, d) = (self.i64c(*idx as i64), self.i64c(*dari as i64));
                let kd = self.b.ins().iconst(types::I32, *kode as i64);
                let c = self.b.ins().call(self.lb.simpan_upval, &[i, d, v, kd]);
                let c = self.b.inst_results(c)[0];
                self.jaga(c, jaga);
            }
            Inst::Buka { hasil, pos, kode, jaga } => {
                let v = self.buka_nilai(*pos, *kode, jaga, *hasil);
                self.tetapkan(*hasil, v);
            }
            Inst::Undi { hasil } => {
                let u = self.undi.expect("undi cuma dibangun di deopt acak");
                let c = self.b.ins().call(u, &[]);
                let c = self.b.inst_results(c)[0];
                let v = self.b.ins().uextend(types::I64, c);
                self.tetapkan(*hasil, v);
            }
        }
    }

    fn banding_i(c: Banding) -> IntCC {
        match c {
            Banding::Lt => IntCC::SignedLessThan, Banding::Le => IntCC::SignedLessThanOrEqual,
            Banding::Gt => IntCC::SignedGreaterThan, Banding::Ge => IntCC::SignedGreaterThanOrEqual,
            Banding::Eq => IntCC::Equal, Banding::Ne => IntCC::NotEqual,
        }
    }

    /// Urutan atas NaN selalu `false`; `!=` benar untuk NaN (`!vm_eq`).
    fn banding_f(c: Banding) -> FloatCC {
        match c {
            Banding::Lt => FloatCC::LessThan, Banding::Le => FloatCC::LessThanOrEqual,
            Banding::Gt => FloatCC::GreaterThan, Banding::Ge => FloatCC::GreaterThanOrEqual,
            Banding::Eq => FloatCC::Equal, Banding::Ne => FloatCC::NotEqual,
        }
    }

    fn biner(&mut self, op: Biner, a: Nilai, y: Nilai, jaga: Option<&Jaga>) -> Value {
        let (x, y) = (self.v(a), self.v(y));
        match op {
            Biner::ITambah | Biner::IKurang | Biner::IKali => match jaga {
                Some(j) => {
                    let (h, luap) = match op {
                        Biner::ITambah => self.b.ins().sadd_overflow(x, y),
                        Biner::IKurang => self.b.ins().ssub_overflow(x, y),
                        _ => self.b.ins().smul_overflow(x, y),
                    };
                    self.jaga(luap, j);
                    h
                }
                None => match op {
                    Biner::ITambah => self.b.ins().iadd(x, y),
                    Biner::IKurang => self.b.ins().isub(x, y),
                    _ => self.b.ins().imul(x, y),
                },
            },
            Biner::IBagi | Biner::ISisa => {
                // Pembagi 0 dan -1 jatuh ke VM: kalimat galatnya, luapan `MIN / -1`, dan aturan `int_biner`.
                if let Some(j) = jaga {
                    let nol = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                    let min1 = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                    let tolak = self.b.ins().bor(nol, min1);
                    self.jaga(tolak, j);
                }
                if op == Biner::ISisa { self.b.ins().srem(x, y) } else { self.b.ins().sdiv(x, y) }
            }
            Biner::IDan => self.b.ins().band(x, y),
            Biner::IAtau => self.b.ins().bor(x, y),
            Biner::IXor => self.b.ins().bxor(x, y),
            Biner::IDanBukan => self.b.ins().band_not(x, y),
            Biner::IGeserKiri => {
                let j = jaga.expect("geser kiri selalu dijaga");
                let jauh = self.b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, y, 63);
                self.jaga(jauh, j);
                let h = self.b.ins().ishl(x, y);
                let balik = self.b.ins().sshr(h, y);
                let hilang = self.b.ins().icmp(IntCC::NotEqual, balik, x);
                self.jaga(hilang, j);
                h
            }
            Biner::IGeserKanan => {
                let j = jaga.expect("geser kanan selalu dijaga");
                let salah = self.b.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, y, 128);
                self.jaga(salah, j);
                let enam3 = self.b.ins().iconst(types::I64, 63);
                let besar = self.b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, y, 63);
                let k = self.b.ins().select(besar, enam3, y);
                self.b.ins().sshr(x, k)
            }
            Biner::ICmp(c) => { let h = self.b.ins().icmp(Self::banding_i(c), x, y); self.b.ins().uextend(types::I64, h) }
            Biner::FCmp(c) => { let h = self.b.ins().fcmp(Self::banding_f(c), x, y); self.b.ins().uextend(types::I64, h) }
            Biner::FTambah => self.b.ins().fadd(x, y),
            Biner::FKurang => self.b.ins().fsub(x, y),
            Biner::FKali => self.b.ins().fmul(x, y),
            Biner::FBagi => self.b.ins().fdiv(x, y),
        }
    }

    fn unar(&mut self, op: Unar, a: Nilai, jaga: Option<&Jaga>) -> Value {
        let x = self.v(a);
        match op {
            Unar::INeg => {
                if let Some(j) = jaga {
                    let luap = self.b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
                    self.jaga(luap, j);
                }
                self.b.ins().ineg(x)
            }
            Unar::FNeg => self.b.ins().fneg(x),
            Unar::Bukan => self.b.ins().bxor_imm_s(x, 1),
            Unar::IBukanBit => self.b.ins().bnot(x),
            Unar::KeBool => { let c = self.b.ins().icmp_imm_s(IntCC::NotEqual, x, 0); self.b.ins().uextend(types::I64, c) }
            Unar::KeF => self.b.ins().fcvt_from_sint(types::F64, x),
            Unar::BitF => self.b.ins().bitcast(types::I64, MemFlagsData::new(), x),
            Unar::DariBit => self.b.ins().bitcast(types::F64, MemFlagsData::new(), x),
            Unar::KeL => self.b.ins().sextend(types::I128, x),
            Unar::KeLU => self.b.ins().uextend(types::I128, x),
            Unar::BukanBitL => self.b.ins().bnot(x),
            Unar::KeBoolL => {
                let z = self.nol128();
                let c = self.b.ins().icmp(IntCC::NotEqual, x, z);
                self.b.ins().uextend(types::I64, c)
            }
            Unar::Rendah => self.b.ins().ireduce(types::I64, x),
        }
    }
}
