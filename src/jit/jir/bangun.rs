//! Bytecode + `layak::Rencana` → JIR. Cakupan JIR-1: semua instruksi terjangkau `Cara::Asli` atas bulat, pecahan
//! dan `bool` (plus `nil`/`()` tanpa isi), tanpa panggilan, tanpa posisi milik VM, tanpa tepi yang mengubah jenis.
//! Di luar itu `Err(alasan)`, dan unitnya diturunkan penurun langsung.

use super::{OpL, Akhir, Arg, Banding, Biner, Blok, DataBlok, Fungsi, Hasil, Inst, Jaga, Keadaan, Nilai, Pb, Tipe, Uji, Unar};
use crate::jit::layak::{Cara, Jenis, Rencana, KEMBALI_BULAT};
use crate::vm::opcode::OpCode;
use crate::vm::value::VmFunction;

#[path = "bayang.rs"]
mod bayang;

/// Isi native jenis `j`: tipenya, atau `None` kalau posisinya tidak punya isi (`nil`, `()`).
fn tipe_jenis(j: Jenis) -> Result<Option<Tipe>, String> {
    match j {
        // Rune: kode karakter di register; jenisnya dibawa keadaan (`Jenis::kode` 7).
        Jenis::Bulat | Jenis::Bool | Jenis::Rune => Ok(Some(Tipe::I)),
        Jenis::Pecahan => Ok(Some(Tipe::F)),
        Jenis::Lebar => Ok(Some(Tipe::L)),
        // Tanpa isi native: callee/bawaan/konstanta maya, nilai milik VM, salinan lokal (diwujudkan lewat
        // `Rencana::wujud`); `deopt::pulihkan` membangun atau membiarkannya menurut jenisnya.
        Jenis::Kosong | Jenis::Satuan | Jenis::Fungsi(_) | Jenis::Bawaan(_) | Jenis::Bungkus(..) | Jenis::Nilai
        | Jenis::Salinan(_) | Jenis::Konst(_) => Ok(None),
    }
}

/// Opcode yang dicakup JIR-1.
fn opcode_dicakup(op: OpCode) -> bool {
    use OpCode::*;
    matches!(op, LoadLocal | LokalHitung | AmbilLokal | IntImm | Const | True | False | Nil | Unit
        | Add | Sub | Mul | Div | Mod | Lt | Le | Gt | Ge | Eq | Ne | BitAnd | BitOr | BitXor | AndNot | BitNot
        | Shl | Shr | Neg | Not | KeBool | Dup | Swap | SlideDown | Pop | StoreLocal | SimpanBuang
        | Jump | JumpFalse | JumpTrue | JumpFalseKeep | JumpTrueKeep
        | MakeIterRentang | IterAda | IterMaju | IterVar | Return | Call | LoadGlobal | IndeksLokal | GetIndex | TulisIndexLokal
        | TumbuhIndexLokal | PeriksaArgMetode)
}

struct Pembangun<'a> {
    f: &'a VmFunction,
    r: &'a Rencana,
    j: Fungsi,
    /// Blok per instruksi pemimpin yang terjangkau.
    blok_ip: Vec<Option<Blok>>,
    acak: bool,
    /// Rantai lengan `match` (`turun::rantai_pola`): ip kepala -> (rantai, urutan), dan alamat `RantaiPola` tiap rantai.
    rantai: (std::collections::HashMap<usize, (usize, usize)>, Vec<i64>),
    /// Instruksi sesudahnya yang sudah ikut diturunkan (fusi), dilewati badan.
    lewati: usize,
    /// Callee yang boleh disisipkan (`Sisip`), blok dan stack tempat pemanggil melanjutkan sesudah callee yang
    /// disisipkan, panjang stack terbesar callee yang disisipkan (digeser), dan geser nomor rantai `match`-nya.
    sisip: &'a Sisip,
    lanjutan: Option<(Blok, Vec<Option<Nilai>>)>,
    maks_sisip: usize,
    rantai_sisip: usize,
    /// Bacaan field angka lokal (`Pb::FieldLokal`) di blok ini: (blok, lokal, situs) -> nilainya. Dibuang di awal blok
    /// dan sesudah instruksi apa pun yang bukan aritmetika, jaga, atau bacaan.
    cse_field: Vec<((Blok, usize, i64), Nilai)>,
    /// Struct yang baru dibangun dari angka native di `ip` (langkah 23 penurun langsung): (ip, [(kunci field, nilai)]),
    /// jadi lokal tempat ia tinggal atau disimpan sesudahnya tidak perlu membaca field-nya lagi.
    benih: Option<(usize, Vec<(i64, Nilai)>)>,
    /// Bayangan varian (`bayang.rs`): aktif, tabel varian, bayangan blok yang sedang dibangun dan posisi yang baru
    /// diberinya, tepi yang menunggu pemimpinnya, parameter bayangan pemimpin yang sudah mulai, pemimpin yang
    /// dilarang berbayangan (tepi belakangan tidak cocok: bangun ulang), dan rantai yang lengan pertamanya dilipat.
    bayang_aktif: bool,
    varian: Vec<(&'static crate::vm::value::NamaVarian, Option<usize>)>,
    bayang: std::collections::HashMap<usize, bayang::Bayang>,
    lahir: Vec<usize>,
    kini_b: Option<Blok>,
    beku: bool,
    sisi: std::collections::HashMap<usize, Vec<(Blok, Blok, bayang::Potret)>>,
    masuk_bayang: std::collections::HashMap<usize, bayang::Potret>,
    larang: std::collections::HashSet<usize>,
    ulang: Option<usize>,
    lipat: std::collections::HashSet<usize>,
    tabel: &'static crate::jit::lambat::TabelVarian,
}

/// JIR titik masuk `r` fungsi `f`; `acak` = deopt acak di tiap instruksi (`JRG_JIT_STRES=deopt-acak`).
/// JIR callee panggilan langsung yang boleh disisipkan, sejajar `Rencana::langsung`: fungsinya dan `fid`-nya.
pub type Sisip = [Option<(std::sync::Arc<Fungsi>, u32)>];

pub fn bangun(f: &VmFunction, r: &Rencana, acak: bool, sisip: &Sisip) -> Result<Fungsi, String> {
    periksa_cakupan(f, r)?;
    let mut larang = std::collections::HashSet::new();
    loop {
        match bangun_sekali(f, r, acak, sisip, &larang) {
            Ok(j) => return Ok(j),
            Err((e, Some(ip))) if larang.insert(ip) => { let _ = e; }
            Err((e, _)) => return Err(e),
        }
    }
}

/// Satu percobaan; galat membawa pemimpin yang bayangannya harus dilarang lalu dicoba lagi.
fn bangun_sekali(f: &VmFunction, r: &Rencana, acak: bool, sisip: &Sisip, larang: &std::collections::HashSet<usize>)
    -> Result<Fungsi, (String, Option<usize>)> {
    let kode = &f.chunk.code;
    let aktif = std::env::var("JRG_JIT_BAYANG").map_or(true, |v| v != "mati")
        && !kode.iter().any(|i| i.op == OpCode::MakeClosure);
    let mut p = Pembangun {
        f, r, acak, sisip, lanjutan: None, maks_sisip: 0, rantai_sisip: 0,
        j: Fungsi { blok: Vec::new(), masuk: Blok(0), tipe: Vec::new(), posisi_masuk: Vec::new(), maks: 1, rumus: r.ubah.clone() },
        blok_ip: vec![None; kode.len()],
        rantai: Default::default(),
        lewati: 0,
        cse_field: Vec::new(),
        benih: None,
        bayang_aktif: aktif, varian: Vec::new(), bayang: Default::default(), lahir: Vec::new(), kini_b: None, beku: false,
        sisi: Default::default(), masuk_bayang: Default::default(), larang: larang.clone(), ulang: None,
        lipat: Default::default(),
        tabel: Box::leak(Box::new(crate::jit::lambat::TabelVarian(Default::default()))),
    };
    let hasil = p.jalankan();
    hasil.map_err(|e| (e, p.ulang))
}

impl Pembangun<'_> {
    fn jalankan(&mut self) -> Result<Fungsi, String> {
        let (f, r) = (self.f, self.r);
        let kode = &f.chunk.code;
        let p = self;
        let (peta, daftar) = crate::jit::turun::rantai_pola(f, r);
        let situs = daftar.iter().map(|d| {
            let s: Vec<&'static crate::jit::lambat::SitusPola> = d.iter()
                .map(|&h| crate::jit::lambat::SitusPola::bocor(&f.chunk.constants[kode[h + 1].arg as usize])).collect();
            crate::jit::lambat::RantaiPola::bocor(s) as *const _ as i64
        }).collect();
        p.rantai = (peta, situs);
        // Blok masuk lebih dulu: parameternya argumen fungsi (atau isi bingkai OSR).
        let mut params = Vec::new();
        for (i, j) in r.jenis_masuk.iter().enumerate() {
            if let Some(t) = tipe_jenis(*j)? {
                params.push(p.j.nilai_baru(t));
                p.j.posisi_masuk.push(i);
            }
        }
        p.j.blok.push(DataBlok { ip: r.masuk, params: params.clone(), inst: Vec::new(), akhir: None });
        for ip in 0..kode.len() {
            if r.keadaan[ip].is_some() && r.pemimpin[ip] {
                let b = p.blok_baru(ip)?;
                p.blok_ip[ip] = Some(b);
            }
        }
        let ke = p.blok_ip[r.masuk].ok_or("titik masuk bukan pemimpin")?;
        let args = p.args_ke(Blok(0), r.masuk, &p.isi_masuk(&params)?)?;
        p.j.blok[0].akhir = Some(Akhir::Lompat { ke, args });
        p.tepi_bayang(Blok(0), ke, r.masuk, 0)?;
        p.badan()?;
        p.j.maks = r.keadaan.iter().flatten().map(|s| s.len()).max().unwrap_or(1).max(1).max(p.maks_sisip);
        super::periksa::periksa(&p.j)?;
            let kosong = Fungsi { blok: Vec::new(), masuk: Blok(0), tipe: Vec::new(), posisi_masuk: Vec::new(), maks: 1, rumus: Vec::new() };
        Ok(std::mem::replace(&mut p.j, kosong))
    }
}

/// Indeks argumen `Inst::Pembantu` yang berupa posisi stack (digeser saat callee disisipkan); lainnya alamat, kode,
/// atau isi native.
fn posisi_pb(pb: Pb) -> &'static [usize] {
    match pb {
        Pb::Field | Pb::Pola | Pb::Payload | Pb::Indeks | Pb::JenisSama | Pb::Karakter => &[0],
        Pb::PolaLokal | Pb::PayloadJenisSama | Pb::PayloadPanjang | Pb::KarakterLokal | Pb::Baca | Pb::BacaPecahan | Pb::Tulis => &[0],
        Pb::PayloadLokal | Pb::PayloadAngkaLokal => &[0, 1],
        Pb::MuatGlobal | Pb::SimpanGlobal => &[1],
        Pb::FieldLokal => &[0, 3],
    }
}

/// Seluruh instruksi terjangkau harus berada di cakupan JIR-1.
fn periksa_cakupan(f: &VmFunction, r: &Rencana) -> Result<(), String> {
    if r.ada_defer { return Err("fungsi ber-defer".into()); }
    for j in &r.jenis_masuk { tipe_jenis(*j)?; }
    for (ip, ins) in f.chunk.code.iter().enumerate() {
        let Some(s) = &r.keadaan[ip] else { continue };
        for j in s { tipe_jenis(*j)?; }
        if r.keluar[ip] { continue; }
        match &r.cara[ip] {
            Cara::Asli | Cara::Buka(_) => if !opcode_dicakup(ins.op) { return Err(format!("{:?} di ip {ip}", ins.op)); },
            Cara::Konstanta | Cara::Muat | Cara::Ambil | Cara::Simpan | Cara::Buang | Cara::Benar | Cara::KembaliNilai
            | Cara::KembaliUnit | Cara::Langkah { .. } | Cara::Bawaan | Cara::Biner | Cara::MuatUpval | Cara::SimpanUpval
            | Cara::TeksBulat | Cara::Geser | Cara::Cabang { .. } | Cara::Bangun(_) | Cara::Salinan | Cara::Salin | Cara::Tukar
            | Cara::Metode(_) | Cara::IterAda | Cara::Maju | Cara::Langsung(_) | Cara::Panjang | Cara::PanjangLokal(_)
            | Cara::Field | Cara::Pola | Cara::Payload | Cara::MuatGlobal | Cara::SimpanGlobal | Cara::Indeks
            | Cara::TipeLipat(_) | Cara::TipeSama(_) | Cara::PayloadSama(..) | Cara::PayloadPanjang(_) | Cara::Sebar
            | Cara::UbahLokal { .. } | Cara::RumusLokal { .. } => {}
            c => return Err(format!("{c:?} di ip {ip}")),
        }
        // `Return` cuma untuk titik masuk yang boleh kembali native.
        if ins.op == OpCode::Return && r.kembali == crate::jit::layak::KEMBALI_TIDAK { return Err("Return OSR".into()); }
    }
    Ok(())
}

impl Pembangun<'_> {
    fn keadaan(&self, ip: usize) -> &Vec<Jenis> { self.r.keadaan[ip].as_ref().expect("ip terjangkau") }

    /// Blok untuk pemimpin `ip`: satu parameter per posisi skalar keadaannya.
    fn blok_baru(&mut self, ip: usize) -> Result<Blok, String> {
        let mut params = Vec::new();
        for j in self.keadaan(ip).clone() {
            if let Some(t) = tipe_jenis(j)? { params.push(self.j.nilai_baru(t)); }
        }
        self.j.blok.push(DataBlok { ip, params, inst: Vec::new(), akhir: None });
        Ok(Blok(self.j.blok.len() as u32 - 1))
    }

    /// Stack abstrak dari parameter blok (urut posisi skalar keadaan `ip`).
    fn isi_dari_params(&self, ip: usize, params: &[Nilai]) -> Result<Vec<Option<Nilai>>, String> {
        let mut it = params.iter();
        self.keadaan(ip).iter().map(|j| Ok(match tipe_jenis(*j)? { Some(_) => Some(*it.next().ok_or("parameter kurang")?), None => None }))
            .collect()
    }

    fn isi_masuk(&self, params: &[Nilai]) -> Result<Vec<Option<Nilai>>, String> {
        let mut it = params.iter();
        self.r.jenis_masuk.iter().map(|j| Ok(match tipe_jenis(*j)? { Some(_) => Some(*it.next().ok_or("parameter kurang")?), None => None }))
            .collect()
    }

    /// Argumen lompatan ke pemimpin `ke` dari stack `isi`: posisi skalar keadaan tujuan, yang jenisnya harus
    /// sama dengan isinya (tepi pengubah jenis sudah ditolak).
    /// Bulat yang bertemu `Lebar` di titik temu dilebarkan di blok asal `dari`.
    fn args_ke(&mut self, dari: Blok, ke: usize, isi: &[Option<Nilai>]) -> Result<Vec<Nilai>, String> {
        let s = self.keadaan(ke).clone();
        if s.len() > isi.len() { return Err(format!("stack ke ip {ke} lebih panjang dari asalnya")); }
        let mut args = Vec::new();
        for (i, j) in s.iter().enumerate() {
            match (tipe_jenis(*j)?, isi[i]) {
                (Some(t), Some(v)) if self.j.tipe(v) == t => args.push(v),
                (Some(Tipe::L), Some(v)) if self.j.tipe(v) == Tipe::I => {
                    let h = self.j.nilai_baru(Tipe::L);
                    self.dorong(dari, Inst::Unar { hasil: h, op: Unar::KeL, a: v, jaga: None });
                    args.push(h);
                }
                (None, _) => {}
                _ => return Err(format!("isi posisi {i} ke ip {ke} tidak cocok")),
            }
        }
        Ok(args)
    }

    /// Keadaan deopt di `ip`: isi native cuma untuk posisi yang skalar menurut keadaan `ip` (nilai milik VM yang baru
    /// dibuka tetap utuh di stack VM, jadi VM membacanya dari sana).
    fn keadaan_di(&self, ip: usize, isi: &[Option<Nilai>]) -> Keadaan {
        let s = self.keadaan(ip);
        Keadaan {
            ip, isi: isi.iter().enumerate().map(|(i, v)| if s.get(i).is_some_and(|j| j.skalar() || *j == Jenis::Lebar) { *v } else { None }).collect(),
            fid: None, geser: 0, induk: None, maya: self.maya_keadaan(isi.len()),
        }
    }

    fn badan(&mut self) -> Result<(), String> {
        let kode = &self.f.chunk.code;
        let mut kini: Option<(Blok, Vec<Option<Nilai>>)> = None;
        for ip in 0..kode.len() {
            if self.r.keadaan[ip].is_none() { kini = None; continue; }
            if let Some(b) = self.blok_ip[ip] {
                // Jatuh ke pemimpin: lompat ke bloknya.
                if let Some((asal, isi)) = kini.take() {
                    let (ke, args) = self.tepi(asal, ip - 1, ip, &isi)?;
                    self.akhiri(asal, Akhir::Lompat { ke, args });
                }
                let params = self.j.blok[b.0 as usize].params.clone();
                kini = Some((b, self.isi_dari_params(ip, &params)?));
                self.mulai_bayang(ip, b);
                self.kini_b = Some(b);
            }
            let Some((b, mut isi)) = kini.take() else { continue };
            if self.lewati > 0 { self.lewati -= 1; kini = Some((b, isi)); continue; }
            if isi.len() != self.keadaan(ip).len() { return Err(format!("panjang stack di ip {ip} tidak cocok")); }
            // Posisi maya yang tidak boleh melewati instruksi ini diwujudkan ke stack VM dulu.
            // Isinya tidak berubah, jadi bayangannya tetap (`beku`).
            for (p, j) in self.r.wujud[ip].clone() {
                // Varian basi (bayangannya lengkap) tidak perlu ditulis: bayangannya yang dibawa.
                let basi = |me: &Self, q: usize| me.bayang.get(&q).is_some_and(|x| x.basi != 0 && x.basi == x.mungkin);
                self.beku = true;
                match j {
                    Jenis::Salinan(l) if basi(self, l as usize) => {}
                    Jenis::Salinan(l) => self.dorong(b, Inst::Muat { lokal: l as usize, ke: p, pindah: false }),
                    _ if basi(self, p) => {}
                    j => {
                        let k = j.konstanta().ok_or("wujud bukan salinan atau konstanta")? as usize;
                        self.dorong(b, Inst::Taruh { pos: p, alamat: &self.f.chunk.constants[k] as *const _ as i64 });
                    }
                }
                self.beku = false;
                if let Jenis::Salinan(l) = j { self.salin_bayang(l as usize, p); }
                isi[p] = None;
            }
            if self.acak {
                let u = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::Undi { hasil: u });
                let jaga = Jaga { keadaan: self.keadaan_di(ip, &isi), spek: false, puncak_vm: false };
                self.dorong(b, Inst::JagaJika { kondisi: u, jaga });
            }
            if self.r.keluar[ip] {
                self.akhiri(b, Akhir::Keluar { keadaan: self.keadaan_di(ip, &isi) });
                continue;
            }
            let t0 = isi.len();
            self.lahir.clear();
            match self.cara(b, ip, &mut isi)? {
                Some(akhir) => {
                    // VM yang menjalankan cabangnya membaca stack-nya; hasil milik VM dibaca pemanggil.
                    match &akhir {
                        Akhir::CabangVm { .. } => self.wujudkan_semua(b),
                        Akhir::KembaliNilai { pos } => self.wujudkan(b, *pos),
                        _ => {}
                    }
                    self.akhiri(b, akhir);
                    // Callee yang disisipkan berakhir di blok lanjutan: pemanggil melanjutkan di sana.
                    if let Some(l) = self.lanjutan.take() {
                        self.rapikan_bayang(t0, &l.1);
                        self.kini_b = Some(l.0);
                        kini = Some(l);
                    }
                }
                None => {
                    // Keadaan yang dibawa tepinya (sebelum titik temu melebarkannya: `isi_tepi` yang menulis).
                    // Sesudah fusi, keadaan yang dicocokkan milik ip sesudah instruksi terakhir yang ikut.
                    let nx = ip + 1 + self.lewati;
                    let lanjut = self.r.tepi.get(&(nx - 1, nx)).or_else(|| self.r.keadaan.get(nx).and_then(|s| s.as_ref()))
                        .ok_or_else(|| format!("ip {nx} tak terjangkau sesudah ip {ip}"))?.clone();
                    if lanjut.len() != isi.len() { return Err(format!("panjang stack sesudah ip {ip}: {} lawan {}", isi.len(), lanjut.len())); }
                    // Posisi tanpa isi native di keadaan berikutnya (lubang `AmbilLokal`, milik VM) tidak membawa nilai.
                    for (i, j) in lanjut.iter().enumerate() {
                        match (tipe_jenis(*j)?, isi[i]) {
                            (None, _) => isi[i] = None,
                            (Some(t), Some(v)) if self.j.tipe(v) == t => {}
                            _ => return Err(format!("posisi {i} sesudah ip {ip} tanpa isi native yang cocok")),
                        }
                    }
                    self.rapikan_bayang(t0, &isi);
                    kini = Some((b, isi));
                }
            }
        }
        if kini.is_some() { return Err("kode habis tanpa akhir".into()); }
        Ok(())
    }

    fn dorong(&mut self, b: Blok, i: Inst) {
        let murni = matches!(i, Inst::KonstI { .. } | Inst::KonstF { .. } | Inst::KonstL { .. } | Inst::Biner { .. }
            | Inst::Unar { .. } | Inst::BinerL { .. } | Inst::Pilih { .. } | Inst::JagaJika { .. } | Inst::Sempit { .. }
            | Inst::Pembantu { pb: Pb::FieldLokal | Pb::Baca | Pb::BacaPecahan, .. });
        if !murni { self.cse_field.clear(); }
        if self.bayang_aktif && !self.beku && self.kini_b == Some(b) {
            if self.cegat(b, &i) { self.bayang_inst(&i); return; }
            self.bayang_inst(&i);
        }
        self.j.blok[b.0 as usize].inst.push(i);
    }

    fn akhiri(&mut self, b: Blok, a: Akhir) { self.j.blok[b.0 as usize].akhir = Some(a); }

    fn jaga(&self, ip: usize, isi: &[Option<Nilai>], spek: bool) -> Option<Jaga> {
        Some(Jaga { keadaan: self.keadaan_di(ip, isi), spek, puncak_vm: false })
    }

    /// Nilai di posisi `i` stack yang wajib skalar.
    fn ambil(isi: &[Option<Nilai>], i: usize) -> Result<Nilai, String> {
        isi.get(i).copied().flatten().ok_or_else(|| format!("posisi {i} tanpa isi native"))
    }

    /// Pecahan dari nilai bulat atau pecahan (operand campur, aturan VM).
    fn ke_f(&mut self, b: Blok, v: Nilai) -> Nilai {
        if self.j.tipe(v) == Tipe::F { return v; }
        let h = self.j.nilai_baru(Tipe::F);
        self.dorong(b, Inst::Unar { hasil: h, op: Unar::KeF, a: v, jaga: None });
        h
    }

    fn konst_i(&mut self, b: Blok, n: i64) -> Nilai {
        let h = self.j.nilai_baru(Tipe::I);
        self.dorong(b, Inst::KonstI { hasil: h, nilai: n });
        h
    }

    fn biner(&mut self, b: Blok, op: Biner, x: Nilai, y: Nilai, jaga: Option<Jaga>) -> Nilai {
        let t = match op { Biner::FTambah | Biner::FKurang | Biner::FKali | Biner::FBagi => Tipe::F, _ => Tipe::I };
        let h = self.j.nilai_baru(t);
        self.dorong(b, Inst::Biner { hasil: h, op, a: x, b: y, jaga });
        h
    }

    /// Instruksi `ip` menurut `Rencana::cara`-nya; `Some` = pengakhir blok.
    fn cara(&mut self, b: Blok, ip: usize, isi: &mut Vec<Option<Nilai>>) -> Result<Option<Akhir>, String> {
        let ins = self.f.chunk.code[ip];
        let a = ins.arg as usize;
        let t = isi.len();
        let s = self.keadaan(ip).clone();
        match self.r.cara[ip].clone() {
            Cara::Asli => return self.instruksi(b, ip, isi, &s),
            Cara::Buka(daftar) => {
                // Nilai milik VM yang dipakai sebagai angka: dibuka berjaga (spekulasi), lalu instruksinya asli.
                let mut s2 = s.clone();
                for (p, j) in daftar {
                    let tipe = tipe_jenis(j)?.ok_or("buka ke jenis tanpa isi")?;
                    let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: true, puncak_vm: false };
                    let h = self.j.nilai_baru(tipe);
                    if tipe == Tipe::L { self.dorong(b, Inst::BukaLebar { hasil: h, pos: p, jaga }); }
                    else { self.dorong(b, Inst::Buka { hasil: h, pos: p, kode: j.kode() as i32, jaga }); }
                    isi[p] = Some(h);
                    s2[p] = j;
                }
                return self.instruksi(b, ip, isi, &s2);
            }
            Cara::Konstanta => {
                let alamat = match (ins.op, self.f.chunk.constants.get(a)) {
                    (OpCode::Const, Some(crate::vm::value::VmValue::NativeFn(_))) => { isi.push(None); return Ok(None); }
                    (OpCode::Const, Some(k)) => k as *const _ as i64,
                    _ => 0,
                };
                self.dorong(b, Inst::Taruh { pos: t, alamat });
                isi.push(None);
            }
            // `LoadLocal; GetField` atas lokal milik VM (syarat gabung penurun langsung): satu `field_lokal` tanpa menyalin
            // lokalnya; angka yang dispekulasikan langsung ke register. 1 = galat di `GetField` (lokal sudah ditaruh
            // di puncak, VM yang melapor); spekulasi gagal = deopt di `ip + 2` dengan puncak milik VM.
            Cara::Muat if !matches!(s[a], Jenis::Salinan(_)) && self.gabung(ip + 1) && self.r.cara.get(ip + 1) == Some(&Cara::Field) => {
                let gf = self.f.chunk.code[ip + 1];
                let spek = self.r.spek[ip + 1];
                // Cuma bulat/pecahan yang dijawab pembantu di register; jenis lain selalu ditaruh di puncak (kode 0).
                let jk = match spek { Some(Jenis::Bulat) => 1, Some(Jenis::Pecahan) => 5, _ => 0 };
                // Field angka yang sama dari lokal yang sama sudah dibaca di blok ini tanpa apa pun di antaranya.
                let kunci = (b, a, Self::kunci_field(gf.arg as usize, gf.op == OpCode::GetFieldPrivat, jk));
                if jk != 0 {
                    if let Some(&(_, v)) = self.cse_field.iter().find(|(k, _)| *k == kunci) {
                        isi.push(Some(v));
                        self.lewati = 1;
                        return Ok(None);
                    }
                }
                let situs = crate::jit::lambat::SitusField::bocor(&self.f.chunk.names[gf.arg as usize], gf.op == OpCode::GetFieldPrivat, jk);
                let st = self.j.nilai_baru(Tipe::I);
                let k = match spek {
                    Some(j @ (Jenis::Bulat | Jenis::Pecahan)) => Some(self.j.nilai_baru(tipe_jenis(j)?.ok_or("field angka")?)),
                    _ => None,
                };
                let args = vec![Arg::I64(a as i64), Arg::I64(situs as *const _ as i64), Arg::Wadah, Arg::I64(t as i64)];
                self.dorong(b, Inst::Pembantu { pb: Pb::FieldLokal, args, hasil: Some(st), gagal: None, keluar: k });
                isi.push(None);
                let satu = self.konst_i(b, 1);
                let gagal = self.biner(b, Biner::ICmp(Banding::Eq), st, satu, None);
                self.dorong(b, Inst::JagaJika { kondisi: gagal, jaga: Jaga { keadaan: self.keadaan_di(ip + 1, isi), spek: false, puncak_vm: false } });
                if let Some(k) = k {
                    let mut kl = self.keadaan_di(ip + 2, isi);
                    if let Some(p) = kl.isi.last_mut() { *p = None; }
                    let empat = self.konst_i(b, 4);
                    let bukan = self.biner(b, Biner::ICmp(Banding::Ne), st, empat, None);
                    self.dorong(b, Inst::JagaJika { kondisi: bukan, jaga: Jaga { keadaan: kl, spek: true, puncak_vm: true } });
                    isi[t] = Some(k);
                    self.cse_field.push((kunci, k));
                } else if spek.is_some() {
                    // Spekulasi jenis lain: hasilnya milik VM di puncak, dibuka berjaga seperti `GetField` biasa.
                    self.buka_hasil(b, ip + 1, isi)?;
                }
                self.lewati = 1;
            }
            Cara::Muat => {
                // Lokal yang isinya salinan maya: lokal asalnya yang dibaca (disalin, tidak pernah dipindah).
                let (lokal, pindah) = match s[a] {
                    Jenis::Salinan(l) => (l as usize, false),
                    _ => (a, crate::jit::turun::mati_sesudah(self.f, self.r, ip, a)),
                };
                self.dorong(b, Inst::Muat { lokal, ke: t, pindah });
                isi.push(None);
            }
            Cara::Ambil => { self.dorong(b, Inst::Muat { lokal: a, ke: t, pindah: true }); isi.push(None); }
            Cara::Simpan => {
                let j = s[t - 1];
                self.lebar_ke_vm(b, t - 1, j, isi)?;
                let nilai = if j.skalar() { Some(Self::ambil(isi, t - 1)?) } else { None };
                let buang = ins.op == OpCode::SimpanBuang;
                self.dorong(b, Inst::Simpan { ke: a, dari: t - 1, nilai, kode: j.kode() as i32, buang });
                isi[a] = None;
                if buang { isi.pop(); }
                // Lokal ini sekarang struct yang baru dibangun tepat sebelumnya: isi field-nya = angka pembangunnya.
                if let Some((ib, benih)) = self.benih.take() {
                    if ib + 1 == ip { for (k, v) in benih { self.cse_field.push(((b, a, k), v)); } }
                }
            }
            Cara::Buang => { self.dorong(b, Inst::Buang { pos: t - 1 }); isi.pop(); }
            Cara::Benar => {
                let tetap = matches!(ins.op, OpCode::JumpFalseKeep | OpCode::JumpTrueKeep);
                let h = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::Benar { hasil: h, pos: t - 1, lepas: !tetap });
                if ins.op == OpCode::KeBool { isi[t - 1] = Some(h); return Ok(None); }
                let sesudah: Vec<Option<Nilai>> = if tetap { isi.clone() } else { isi[..t - 1].to_vec() };
                return self.cabang(b, ip, a, h, &sesudah, matches!(ins.op, OpCode::JumpFalse | OpCode::JumpFalseKeep)).map(Some);
            }
            Cara::KembaliNilai => {
                // Bulat 128 bit di register: ditulis ke stack VM dulu, lalu kembali sebagai milik VM.
                if let Some(Some(v)) = isi.get(t - 1) {
                    if self.j.tipe(*v) != Tipe::L { return Err("KembaliNilai atas isi native 64 bit".into()); }
                    self.dorong(b, Inst::TulisLebar { pos: t - 1, nilai: *v });
                }
                return Ok(Some(Akhir::KembaliNilai { pos: t - 1 }));
            }
            Cara::KembaliUnit => return Ok(Some(Akhir::KembaliUnit)),
            // `l.m(..)` berumus atas lokal `a`: satu instruksi, lanjut di `ip + lebar` (seperti penurun langsung).
            Cara::UbahLokal { rumus, lebar } | Cara::RumusLokal { rumus, lebar } => {
                let tulis = matches!(self.r.cara[ip], Cara::UbahLokal { .. });
                let rm = &self.r.ubah[rumus];
                let args = rm.sumber.iter().map(|s| match s {
                    crate::jit::layak::Sumber::Lokal(x) => {
                        let v = Self::ambil(isi, *x)?;
                        Ok(Some(self.ke_f(b, v)))
                    }
                    crate::jit::layak::Sumber::Konst(_) => Ok(None),
                }).collect::<Result<Vec<_>, String>>()?;
                let hasil = (!tulis).then(|| self.j.nilai_baru(Tipe::F));
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: true, puncak_vm: false };
                self.dorong(b, Inst::Rumus { rumus, lokal: a, args, tulis, hasil, jaga });
                if let Some(h) = hasil { isi.push(Some(h)); }
                let (ke, args) = self.tepi(b, ip, ip + lebar, isi)?;
                return Ok(Some(Akhir::Lompat { ke, args }));
            }
            Cara::Sebar => {
                let pulang = self.r.kembali == crate::jit::layak::KEMBALI_SKALAR && !self.r.ada_defer;
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                self.dorong(b, Inst::Sebar { pos: t - 1, pulang, keluarga: ins.arg, jaga });
            }
            // `Typeof; Const; Eq` (dan `AmbilPayload` di depannya, atau `Len` sesudah `AmbilPayload`) jadi satu: hasilnya
            // di puncak, lanjut di `ip + lebar` (seperti penurun langsung).
            Cara::TipeLipat(ya) => {
                let v = self.konst_i(b, ya as i64);
                isi[t - 1] = Some(v);
                let (ke, args) = self.tepi(b, ip, ip + 3, isi)?;
                return Ok(Some(Akhir::Lompat { ke, args }));
            }
            Cara::TipeSama(ci) => {
                let alamat = &self.f.chunk.constants[ci] as *const _ as i64;
                let h = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::Pembantu { pb: Pb::JenisSama, args: vec![Arg::I64(t as i64 - 1), Arg::I64(alamat)], hasil: Some(h), gagal: None, keluar: None });
                isi[t - 1] = Some(h);
                let (ke, args) = self.tepi(b, ip, ip + 3, isi)?;
                return Ok(Some(Akhir::Lompat { ke, args }));
            }
            Cara::PayloadSama(l, ci) => {
                // 2 = bukan enum berpayload: deopt.
                let alamat = &self.f.chunk.constants[ci] as *const _ as i64;
                let h = self.j.nilai_baru(Tipe::I);
                let gagal = Some((Uji::Sama(2), Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false }));
                self.dorong(b, Inst::Pembantu { pb: Pb::PayloadJenisSama, args: vec![Arg::I64(l as i64), Arg::I64(alamat)], hasil: Some(h), gagal, keluar: None });
                isi[t - 1] = Some(h);
                let (ke, args) = self.tepi(b, ip, ip + 4, isi)?;
                return Ok(Some(Akhir::Lompat { ke, args }));
            }
            Cara::PayloadPanjang(l) => {
                let k = self.j.nilai_baru(Tipe::I);
                let gagal = Some((Uji::BukanNol, Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false }));
                self.dorong(b, Inst::Pembantu { pb: Pb::PayloadPanjang, args: vec![Arg::I64(l as i64), Arg::Wadah], hasil: None, gagal, keluar: Some(k) });
                isi[t - 1] = Some(k);
                let (ke, args) = self.tepi(b, ip, ip + 2, isi)?;
                return Ok(Some(Akhir::Lompat { ke, args }));
            }
            Cara::Langkah { ambil, taruh, tumpah } => {
                let (isi_n, taruh_n) = self.tumpahan(&s, t - tumpah..t, isi)?;
                let jenis = self.jenis_ip(ip);
                self.dorong(b, Inst::Langkah { ip, jenis, t, tumpah, sesudah: t - ambil + taruh, isi: isi_n, taruh: taruh_n });
                isi.truncate(t - ambil);
                for _ in 0..taruh { isi.push(None); }
            }
            Cara::Bawaan => {
                let fungsi = match self.f.chunk.constants.get(s[t - a - 1].konstanta().ok_or("callee bawaan bukan konstanta")? as usize) {
                    Some(crate::vm::value::VmValue::NativeFn(g)) => *g as usize as i64,
                    _ => return Err("callee bawaan bukan fungsi bawaan".into()),
                };
                let polos = if crate::jit::lambat::bawaan_polos(fungsi, a) { crate::jit::lambat::BAWAAN_POLOS } else { 0 };
                let (isi_n, taruh_n) = self.tumpahan(&s, t - a..t, isi)?;
                let jenis = self.jenis_ip(ip);
                self.dorong(b, Inst::Bawaan { ip, jenis, t, argc: a as i64 | polos, fungsi, isi: isi_n, taruh: taruh_n });
                isi.truncate(t - a - 1);
                isi.push(None);
                self.buka_hasil(b, ip, isi)?;
            }
            Cara::Biner => {
                let (isi_n, taruh_n) = self.tumpahan(&s, t - 2..t, isi)?;
                let banding = matches!(ins.op, OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge);
                let hasil = banding.then(|| self.j.nilai_baru(Tipe::I));
                let jenis = self.jenis_ip(ip);
                self.dorong(b, Inst::BinerVm { hasil, tambah: ins.op == OpCode::Add, ip, jenis, t, isi: isi_n, taruh: taruh_n });
                isi.truncate(t - 2);
                isi.push(hasil);
            }
            Cara::MuatUpval => {
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                self.dorong(b, Inst::MuatUpval { idx: a, ke: t, jaga });
                isi.push(None);
                self.buka_hasil(b, ip, isi)?;
            }
            Cara::Bangun(n) => {
                use crate::jit::lambat::SitusBangun;
                use crate::vm::value::{Label, VmValue};
                let kode = &self.f.chunk.code;
                let situs = SitusBangun::bocor(match ins.op {
                    OpCode::BuildStruct => match self.f.chunk.constants.get(a) {
                        // Langsung dikembalikan: lokal bingkai ini yang sebentuk dan milik tunggal boleh dipakai ulang.
                        Some(VmValue::Map(p)) if kode.get(ip + 1).is_some_and(|i| i.op == OpCode::Return)
                            && matches!(self.r.cara.get(ip + 1), Some(Cara::KembaliNilai)) =>
                            SitusBangun::StrukturUlang(p.label.clone(), p.bentuk().ok_or("prototipe tanpa bentuk")?),
                        Some(VmValue::Map(p)) => SitusBangun::Struktur(p.label.clone(), p.bentuk().ok_or("prototipe tanpa bentuk")?),
                        _ => return Err("prototipe struct bukan peta".into()),
                    },
                    OpCode::BuildTuple => SitusBangun::Tuple,
                    OpCode::BuildEnum => match self.f.chunk.constants.get(a) {
                        Some(VmValue::Map(p)) => match &p.label {
                            Label::Enum { nama, .. } => SitusBangun::Varian(nama),
                            _ => return Err("prototipe varian bukan enum".into()),
                        },
                        _ => return Err("prototipe varian bukan peta".into()),
                    },
                    _ => SitusBangun::Larik,
                });
                // Benih: field angka struct sebentuk unik = isi native yang membangunnya.
                let benih: Vec<(i64, Nilai)> = match situs {
                    SitusBangun::Struktur(_, bentuk) if ins.op == OpCode::BuildStruct && bentuk.unik && bentuk.nama.len() == n =>
                        bentuk.nama.iter().enumerate().filter_map(|(i, nama)| {
                            let jk = match s[t - n + i] { Jenis::Bulat => 1, Jenis::Pecahan => 5, _ => return None };
                            let idx = self.f.chunk.names.iter().position(|x| x == nama.as_str())?;
                            Some((Self::kunci_field(idx, false, jk), isi[t - n + i]?))
                        }).collect(),
                    _ => Vec::new(),
                };
                // Varian: penandanya, dan payload angkanya kalau ada (payload tuple dari `BuildTuple` tepat sebelumnya). Yang
                // payload-nya angka native semua tidak dibangun (basi): tuple-nya pun tidak.
                let mut varian = None;
                if let SitusBangun::Varian(nama) = situs {
                    let tuple = self.tuple_native(b, ip, t);
                    let (jumlah, muatan) = match (s[t - 1], isi[t - 1]) {
                        (j @ (Jenis::Bulat | Jenis::Pecahan), Some(v)) => (Some(1), vec![(j, v)]),
                        (Jenis::Bool | Jenis::Rune | Jenis::Lebar | Jenis::Kosong | Jenis::Satuan | Jenis::Bulat | Jenis::Pecahan, _) => (Some(1), Vec::new()),
                        (Jenis::Nilai, _) if ip > 0 && !self.r.pemimpin[ip] && kode[ip - 1].op == OpCode::BuildTuple =>
                            (Some(kode[ip - 1].arg as usize), tuple.clone().unwrap_or_default()),
                        _ => (None, Vec::new()),
                    };
                    if let Some(jumlah) = jumlah {
                        let apung = muatan.iter().enumerate().filter(|(_, m)| m.0 == Jenis::Pecahan).fold(0u64, |a, (k, _)| a | 1 << k);
                        let basi = self.bayang_aktif && muatan.len() == jumlah && self.tag_varian(nama, Some(jumlah), 0, apung).is_some();
                        varian = Some((nama, jumlah, muatan, basi, tuple.is_some() && jumlah > 1));
                    }
                }
                if let Some((_, _, _, true, buang_tuple)) = &varian {
                    if *buang_tuple { self.j.blok[b.0 as usize].inst.pop(); }
                } else {
                    let (isi_n, taruh_n) = self.tumpahan(&s, t - n..t, isi)?;
                    let jenis = self.jenis_ip(ip);
                    self.dorong(b, Inst::Bangun { situs: situs as *const _ as i64, jenis, t, n, isi: isi_n, taruh: taruh_n });
                }
                if let Some((nama, jumlah, muatan, basi, _)) = varian {
                    self.lahir_varian(b, t - 1, nama, Some(jumlah), muatan, 0, basi);
                }
                isi.truncate(t - n);
                isi.push(None);
                // Hasilnya tinggal di posisinya sebagai lokal (`let`), atau disimpan instruksi berikutnya.
                for &(k, v) in &benih { self.cse_field.push(((b, t - n, k), v)); }
                if !benih.is_empty() { self.benih = Some((ip, benih)); }
            }
            Cara::Salinan => match ins.op {
                // Salinan atau konstanta maya: cuma jenisnya yang berpindah.
                OpCode::LoadLocal => { isi.push(None); self.salin_bayang(a, t); }
                OpCode::Const => { isi.push(None); self.lahir_konst(b, t, a); }
                OpCode::LokalHitung => isi.push(None),
                OpCode::StoreLocal => { isi[a] = None; self.salin_bayang(t - 1, a); }
                OpCode::SimpanBuang => { isi[a] = None; self.salin_bayang(t - 1, a); isi.pop(); }
                op => return Err(format!("Cara::Salinan atas {op:?}")),
            },
            Cara::Langsung(n) if self.sisip.get(n).is_some_and(|x| x.is_some()) && self.bisa_sisip(n, ip, isi, &s) => {
                return self.sisipkan(b, ip, isi, &s, n).map(Some);
            }
            Cara::Langsung(n) => {
                let (dari, fungsi) = match s[t - a - 1] {
                    Jenis::Fungsi(c) => (t - a, &self.r.callee_nilai[c] as *const _ as i64),
                    _ => (t - a - 1, 0),
                };
                let (isi_n, taruh_n) = self.tumpahan(&s, dari..t, isi)?;
                let jenis_arg = self.r.langsung[n].1.clone();
                let mut args = Vec::new();
                for (k, j) in jenis_arg.iter().enumerate() {
                    if j.skalar() { args.push((k, Self::ambil(isi, t - a + k)?)); }
                }
                let mut sesudah: Vec<Option<Nilai>> = isi[..t - a - 1].to_vec();
                sesudah.push(None);
                let mut basi = self.keadaan_di(ip + 1, &sesudah);
                if let Some(p) = basi.isi.last_mut() { *p = None; }
                let hasil = match self.r.spek[ip] {
                    None => None,
                    Some(j @ (Jenis::Bulat | Jenis::Pecahan)) => {
                        let tipe = tipe_jenis(j)?.ok_or("spekulasi tanpa isi")?;
                        let h = self.j.nilai_baru(tipe);
                        Some((h, j.kode() as i32, Jaga { keadaan: basi.clone(), spek: true, puncak_vm: true }))
                    }
                    // Jenis lain (128 bit, bool, rune): hasilnya milik VM, dibuka berjaga sesudah panggilan.
                    Some(_) => None,
                };
                let jenis = self.jenis_ip(ip);
                let buka = hasil.is_none() && self.r.spek[ip].is_some();
                self.dorong(b, Inst::Langsung { n, ip, jenis, t, a, fungsi, isi: isi_n, taruh: taruh_n, args, basi, hasil: hasil.clone() });
                isi.truncate(t - a - 1);
                isi.push(hasil.map(|(h, ..)| h));
                if buka { self.buka_hasil(b, ip, isi)?; }
            }
            Cara::Panjang | Cara::PanjangLokal(_) => {
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                let h = self.j.nilai_baru(Tipe::I);
                let (pos, lokal, hapus) = match self.r.cara[ip] {
                    // `len(lokal)`: posisi callee `len` dan salinannya diganti panjangnya.
                    Cara::PanjangLokal(l) => (l as usize, true, 2),
                    _ => (t - 1, false, 1),
                };
                self.dorong(b, Inst::Panjang { hasil: h, pos, lokal, jaga });
                isi.truncate(t - hapus);
                isi.push(Some(h));
            }
            Cara::Field => {
                let args = vec![Arg::I64(t as i64 - 1), Arg::I64(&self.f.chunk.names[a] as *const String as i64),
                    Arg::I32((ins.op == OpCode::GetFieldPrivat) as i32)];
                self.pembantu(b, ip, isi, Pb::Field, args, false, Some(Uji::BukanNol));
                self.buka_hasil(b, ip, isi)?;
            }
            // Lengan rantai `match` (kepalanya `LoadLocal` di `ip - 1`): satu `pola_pilih` untuk seluruh rantai.
            Cara::Pola if ip > 0 && self.rantai.0.contains_key(&(ip - 1)) => {
                let (ri, kk) = self.rantai.0[&(ip - 1)];
                let lokal = match s[t - 1] { Jenis::Salinan(l) => l as usize, _ => self.f.chunk.code[ip - 1].arg as usize };
                if let Some(x) = self.bayang.get(&lokal).cloned() {
                    let mut xs = Vec::new();
                    if let Some(h) = self.lipat_rantai(&mut xs, &x, self.rantai.1[ri], kk) {
                        self.dorong_semua(b, xs);
                        if kk == 0 { self.lipat.insert(ri); }
                        isi[t - 1] = Some(h);
                        return Ok(None);
                    }
                }
                let h = self.j.nilai_baru(Tipe::I);
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                let panggil = kk > 0 && self.lipat.contains(&ri);
                self.dorong(b, Inst::Rantai { hasil: h, rantai: ri, urutan: kk, lokal, situs: self.rantai.1[ri], panggil, jaga });
                isi[t - 1] = Some(h);
            }
            Cara::Pola => {
                let situs = crate::jit::lambat::SitusPola::bocor(&self.f.chunk.constants[a]) as *const _ as i64;
                let (pb, args) = match s[t - 1] {
                    Jenis::Salinan(l) => (Pb::PolaLokal, vec![Arg::I64(l as i64), Arg::I64(situs)]),
                    _ => (Pb::Pola, vec![Arg::I64(t as i64 - 1), Arg::I64(situs)]),
                };
                // 0/1 = cocok tidaknya; 2 = jumlah sub-pola salah, VM yang melaporkannya.
                let h = self.pembantu(b, ip, isi, pb, args, true, Some(Uji::Sama(2)));
                isi[t - 1] = h;
            }
            // Payload angka atas salinan lokal: satu pembantu, angkanya langsung ke register (`payload_angka_lokal`).
            // 1 = bukan enum berpayload, deopt di `ip`; 2 = payload jenis lain, sudah ditaruh di puncak, deopt
            // spekulasi di `ip + 1` dengan puncak milik VM (seperti `buka` yang gagal).
            Cara::Payload if matches!((s[t - 1], self.r.spek[ip]), (Jenis::Salinan(_), Some(Jenis::Bulat | Jenis::Pecahan))) => {
                let (Jenis::Salinan(l), Some(j)) = (s[t - 1], self.r.spek[ip]) else { unreachable!() };
                if let Some(x) = self.bayang.get(&(l as usize)).cloned() {
                    let mut xs = Vec::new();
                    if let Some((st, v)) = self.lipat_payload(&mut xs, &x, j) {
                        self.dorong_semua(b, xs);
                        let jaga_bukan = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                        self.dorong(b, Inst::JagaJika { kondisi: st, jaga: jaga_bukan });
                        isi[t - 1] = Some(v);
                        return Ok(None);
                    }
                }
                let jk = if j == Jenis::Pecahan { 5 } else { 1 };
                let (st, k) = (self.j.nilai_baru(Tipe::I), self.j.nilai_baru(tipe_jenis(j)?.ok_or("payload angka")?));
                let jaga_bukan = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                let args = vec![Arg::I64(l as i64), Arg::I64(t as i64 - 1), Arg::I32(jk), Arg::Wadah];
                self.dorong(b, Inst::Pembantu { pb: Pb::PayloadAngkaLokal, args, hasil: Some(st), gagal: None, keluar: Some(k) });
                let satu = self.konst_i(b, 1);
                let bukan = self.biner(b, Biner::ICmp(Banding::Eq), st, satu, None);
                self.dorong(b, Inst::JagaJika { kondisi: bukan, jaga: jaga_bukan });
                isi[t - 1] = None;
                let mut kl = self.keadaan_di(ip + 1, isi);
                if let Some(p) = kl.isi.last_mut() { *p = None; }
                let dua = self.konst_i(b, 2);
                let lain = self.biner(b, Biner::ICmp(Banding::Eq), st, dua, None);
                self.dorong(b, Inst::JagaJika { kondisi: lain, jaga: Jaga { keadaan: kl, spek: true, puncak_vm: true } });
                isi[t - 1] = Some(k);
            }
            Cara::Payload => {
                let (pb, args) = match s[t - 1] {
                    Jenis::Salinan(l) => (Pb::PayloadLokal, vec![Arg::I64(l as i64), Arg::I64(t as i64 - 1)]),
                    _ => (Pb::Payload, vec![Arg::I64(t as i64 - 1)]),
                };
                self.pembantu(b, ip, isi, pb, args, false, Some(Uji::BukanNol));
                isi[t - 1] = None;
                self.buka_hasil(b, ip, isi)?;
            }
            Cara::MuatGlobal => {
                let args = vec![Arg::I64(&self.f.chunk.names[a] as *const String as i64), Arg::I64(t as i64)];
                self.pembantu(b, ip, isi, Pb::MuatGlobal, args, false, Some(Uji::BukanNol));
                isi.push(None);
                self.buka_hasil(b, ip, isi)?;
            }
            Cara::SimpanGlobal => {
                let j = s[t - 1];
                self.lebar_ke_vm(b, t - 1, j, isi)?;
                let v = if j.skalar() { Arg::Isi(Self::ambil(isi, t - 1)?) } else { Arg::I64(0) };
                let args = vec![Arg::I64(&self.f.chunk.names[a] as *const String as i64), Arg::I64(t as i64 - 1), v, Arg::I32(j.kode() as i32)];
                self.pembantu(b, ip, isi, Pb::SimpanGlobal, args, false, Some(Uji::BukanNol));
            }
            Cara::Indeks => {
                let j = s[t - 1];
                self.lebar_ke_vm(b, t - 1, j, isi)?;
                let v = if j.skalar() { Arg::Isi(Self::ambil(isi, t - 1)?) } else { Arg::I64(0) };
                let args = vec![Arg::I64(t as i64 - 2), v, Arg::I32(j.kode() as i32)];
                self.pembantu(b, ip, isi, Pb::Indeks, args, false, Some(Uji::BukanNol));
                if j.di_vm() { self.dorong(b, Inst::Buang { pos: t - 1 }); }
                isi.truncate(t - 1);
                isi[t - 2] = None;
                self.buka_hasil(b, ip, isi)?;
            }
            Cara::Salin => { self.dorong(b, Inst::Salin { dari: t - 1, ke: t }); isi.push(None); }
            Cara::Tukar => {
                let (x, y) = (t - 2, t - 1);
                match (s[x].di_vm(), s[y].di_vm()) {
                    (true, true) => self.dorong(b, Inst::Tukar { x, y }),
                    (true, false) => self.dorong(b, Inst::Pindah { dari: x, ke: y }),
                    _ => self.dorong(b, Inst::Pindah { dari: y, ke: x }),
                }
                isi.swap(x, y);
            }
            Cara::Metode(n) => {
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: true, puncak_vm: false };
                self.dorong(b, Inst::CekMetode { pos: t - 1, label: &self.r.metode[n] as *const String as i64, jaga });
                isi[t - 1] = None;
            }
            Cara::IterAda => {
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                let h = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::IterAda { hasil: h, slot: a, jaga });
                isi.push(Some(h));
            }
            Cara::Maju => {
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                self.dorong(b, Inst::Maju { slot: a, jaga });
            }
            Cara::TeksBulat => {
                let n = Self::ambil(isi, t - 1)?;
                self.dorong(b, Inst::TeksBulat { pos: t - 2, n });
                isi.truncate(t - 2);
                isi.push(None);
            }
            Cara::Geser => {
                // `SlideDown` dengan nilai milik VM: yang tertimpa dilepas, puncaknya dipindah (atau isinya disalin).
                let d0 = t - a - 1;
                for p in d0..t - 1 { if s[p].di_vm() { self.dorong(b, Inst::Buang { pos: p }); } }
                if s[t - 1].di_vm() { self.dorong(b, Inst::Pindah { dari: t - 1, ke: d0 }); }
                isi[d0] = isi[t - 1];
                isi.truncate(d0 + 1);
            }
            Cara::Cabang { tumpah, hasil_bool, tujuan } => {
                let (isi_n, taruh_n) = self.tumpahan(&s, t - tumpah..t, isi)?;
                let jenis = self.jenis_ip(ip);
                // Posisi yang disentuh VM tidak punya isi native di tujuan mana pun (dijamin layak: hasilnya milik VM).
                let mut tj = Vec::new();
                for ke in tujuan {
                    let st = self.keadaan(ke).clone();
                    let mut sesudah: Vec<Option<Nilai>> = isi.iter().take(st.len()).copied().collect();
                    for (i, j) in st.iter().enumerate() {
                        if i >= t - tumpah && (j.skalar() || *j == Jenis::Lebar) { return Err("cabang VM ke posisi native yang disentuhnya".into()); }
                        if i >= sesudah.len() { sesudah.push(None); }
                    }
                    let (blok, args) = self.tepi(b, ip, ke, &sesudah)?;
                    tj.push((blok, args, ke));
                }
                let tujuan = [tj[0].clone(), tj[1].clone()];
                return Ok(Some(Akhir::CabangVm { ip, jenis, t, tumpah, hasil_bool, isi: isi_n, taruh: taruh_n, tujuan }));
            }
            Cara::SimpanUpval => {
                let j = s[t - 1];
                self.lebar_ke_vm(b, t - 1, j, isi)?;
                let nilai = if j.skalar() { Some(Self::ambil(isi, t - 1)?) } else { None };
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                self.dorong(b, Inst::SimpanUpval { idx: a, dari: t - 1, nilai, kode: j.kode() as i32, jaga });
            }
            c => return Err(format!("{c:?} belum diturunkan JIR")),
        }
        Ok(None)
    }

    /// `Inst::Pembantu` dengan jaga di `ip` (keadaan sebelum instruksinya); `hasil` = statusnya jadi nilai bulat.
    #[allow(clippy::too_many_arguments)]
    fn pembantu(&mut self, b: Blok, ip: usize, isi: &[Option<Nilai>], pb: Pb, args: Vec<Arg>, hasil: bool, uji: Option<Uji>) -> Option<Nilai> {
        let gagal = uji.map(|u| (u, Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false }));
        let h = hasil.then(|| self.j.nilai_baru(Tipe::I));
        self.dorong(b, Inst::Pembantu { pb, args, hasil: h, gagal, keluar: None });
        h
    }

    /// Alamat `Rencana::keadaan[ip]` (dibaca `lambat::langkah`/`bawaan`; rencana hidup selama kodenya).
    fn jenis_ip(&self, ip: usize) -> i64 { self.keadaan(ip).as_ptr() as i64 }

    /// Posisi `rentang` yang dibaca pembantu dari stack VM: isi native ke slot rekaman, callee dan bawaan maya
    /// ditaruh dari alamatnya (seperti `tumpahkan` penurun langsung).
    #[allow(clippy::type_complexity)]
    fn tumpahan(&self, s: &[Jenis], rentang: std::ops::Range<usize>, isi: &[Option<Nilai>]) -> Result<(Vec<(usize, Nilai)>, Vec<(usize, i64)>), String> {
        let (mut isi_n, mut taruh_n) = (Vec::new(), Vec::new());
        for i in rentang {
            match s[i] {
                Jenis::Fungsi(c) => taruh_n.push((i, &self.r.callee_nilai[c] as *const _ as i64)),
                j if j.konstanta().is_some() => {
                    let k = j.konstanta().expect("baru diperiksa") as usize;
                    taruh_n.push((i, &self.f.chunk.constants[k] as *const _ as i64));
                }
                j if j.skalar() || j == Jenis::Lebar => isi_n.push((i, Self::ambil(isi, i)?)),
                _ => {}
            }
        }
        Ok((isi_n, taruh_n))
    }

    /// Hasil milik VM di puncak yang dispekulasikan angka (`Rencana::spek`): dibuka berjaga di `ip + 1` dengan puncak
    /// milik VM; gagal = deopt spekulasi.
    fn buka_hasil(&mut self, b: Blok, ip: usize, isi: &mut [Option<Nilai>]) -> Result<(), String> {
        let Some(j) = self.r.spek[ip] else { return Ok(()) };
        let tipe = tipe_jenis(j)?.ok_or("spekulasi hasil tanpa isi")?;
        let t = isi.len();
        let mut k = self.keadaan_di(ip + 1, isi);
        if let Some(p) = k.isi.last_mut() { *p = None; }
        let h = self.j.nilai_baru(tipe);
        let jaga = Jaga { keadaan: k, spek: true, puncak_vm: true };
        if tipe == Tipe::L { self.dorong(b, Inst::BukaLebar { hasil: h, pos: t - 1, jaga }); }
        else { self.dorong(b, Inst::Buka { hasil: h, pos: t - 1, kode: j.kode() as i32, jaga }); }
        isi[t - 1] = Some(h);
        Ok(())
    }

    /// Tujuan tepi `dari → ke` dengan stack `isi`. Kalau titik temu `ke` dilebarkan (`Rencana::tepi`: posisi native
    /// jadi milik VM), tepinya lewat blok stub yang menulis isinya ke stack VM dulu (seperti `isi_tepi`).
    fn tepi(&mut self, asal_b: Blok, dari: usize, ke: usize, isi: &[Option<Nilai>]) -> Result<(Blok, Vec<Nilai>), String> {
        let sasaran = self.blok_ip[ke].ok_or_else(|| format!("tujuan ip {ke} bukan pemimpin"))?;
        let tuju = self.keadaan(ke).clone();
        let asal = self.r.tepi.get(&(dari, ke)).cloned().unwrap_or_else(|| tuju.clone());
        let perlu: Vec<usize> = (0..tuju.len().min(asal.len())).filter(|&i| tuju[i].di_vm() && !asal[i].di_vm()).collect();
        if perlu.is_empty() {
            let mut args = self.args_ke(asal_b, ke, isi)?;
            args.extend(self.tepi_bayang(asal_b, sasaran, ke, tuju.len())?);
            return Ok((sasaran, args));
        }
        // Stub: parameternya isi native yang dibawa, nilai lain tidak boleh melintas blok.
        let (mut params, mut args) = (Vec::new(), Vec::new());
        let mut isi_s: Vec<Option<Nilai>> = Vec::new();
        for v in isi.iter().take(tuju.len()) {
            isi_s.push(match v {
                Some(v) => { let p = self.j.nilai_baru(self.j.tipe(*v)); params.push(p); args.push(*v); Some(p) }
                None => None,
            });
        }
        self.j.blok.push(super::DataBlok { ip: ke, params, inst: Vec::new(), akhir: None });
        let stub = Blok(self.j.blok.len() as u32 - 1);
        for i in perlu {
            match asal[i] {
                Jenis::Fungsi(c) => self.dorong(stub, Inst::Taruh { pos: i, alamat: &self.r.callee_nilai[c] as *const _ as i64 }),
                // Konstanta varian yang basi untuk semua variannya: bayangannya yang dibawa (`tepi_bayang` mewujudkannya
                // di sini kalau tujuannya tidak membawanya).
                j if j.konstanta().is_some() && self.bayang.get(&i).is_some_and(|x| x.basi != 0 && x.basi == x.mungkin) => {}
                j if j.konstanta().is_some() => {
                    let k = j.konstanta().expect("baru diperiksa") as usize;
                    self.dorong(stub, Inst::Taruh { pos: i, alamat: &self.f.chunk.constants[k] as *const _ as i64 });
                }
                Jenis::Salinan(l) => {
                    self.wujudkan(stub, l as usize);
                    self.dorong(stub, Inst::Muat { lokal: l as usize, ke: i, pindah: false });
                }
                Jenis::Lebar => {
                    let n = isi_s[i].ok_or("isi tepi lebar hilang")?;
                    self.dorong(stub, Inst::TulisLebar { pos: i, nilai: n });
                }
                j => {
                    let nilai = if j.skalar() { Some(isi_s[i].ok_or("isi tepi native hilang")?) } else { None };
                    self.dorong(stub, Inst::TulisVm { pos: i, nilai, kode: j.kode() as i32 });
                }
            }
            isi_s[i] = None;
        }
        let mut args_s = self.args_ke(stub, ke, &isi_s)?;
        args_s.extend(self.tepi_bayang(stub, sasaran, ke, tuju.len())?);
        self.akhiri(stub, Akhir::Lompat { ke: sasaran, args: args_s });
        Ok((stub, args))
    }

    /// Cabang atas kondisi `c` ke `ip + 1` dan `sasaran`, dengan stack `sesudah`.
    fn cabang(&mut self, b: Blok, ip: usize, sasaran: usize, c: Nilai, sesudah: &[Option<Nilai>], salah: bool) -> Result<Akhir, String> {
        let (b_lanjut, args_lanjut) = self.tepi(b, ip, ip + 1, sesudah)?;
        let (b_sasar, args_sasar) = self.tepi(b, ip, sasaran, sesudah)?;
        let (ya, ya_args, tidak, tidak_args) = if salah {
            (b_lanjut, args_lanjut, b_sasar, args_sasar)
        } else { (b_sasar, args_sasar, b_lanjut, args_lanjut) };
        Ok(Akhir::Cabang { kondisi: c, ya, ya_args, tidak, tidak_args })
    }

    /// Satu instruksi asli atas stack `isi` berjenis `s` (sesudah bukaan); `Some` = pengakhir blok.
    fn instruksi(&mut self, b: Blok, ip: usize, isi: &mut Vec<Option<Nilai>>, s: &[Jenis]) -> Result<Option<Akhir>, String> {
        let ins = self.f.chunk.code[ip];
        let a = ins.arg as usize;
        let s = s.to_vec();
        let t = isi.len();
        let pecahan = |k: usize| s.get(t.wrapping_sub(k)) == Some(&Jenis::Pecahan);
        if let Some(akhir) = self.lebar(b, ip, isi, &s)? { return Ok(akhir); }
        match ins.op {
            OpCode::LoadLocal | OpCode::LokalHitung | OpCode::AmbilLokal => {
                let v = *isi.get(a).ok_or("lokal di luar stack")?;
                isi.push(v);
            }
            OpCode::IntImm => { let v = self.konst_i(b, ins.arg as i32 as i64); isi.push(Some(v)); }
            OpCode::Const if matches!(self.f.chunk.constants.get(a), Some(crate::vm::value::VmValue::NativeFn(_))) => isi.push(None),
            OpCode::LoadGlobal => isi.push(None),
            OpCode::Call => return self.panggil(b, ip, isi, &s).map(|_| None),
            OpCode::Const => {
                let k = self.r.konstanta[ip].ok_or("konstanta di luar 64 bit")?;
                let lanjut = self.keadaan(ip + 1);
                let v = if lanjut.last() == Some(&Jenis::Pecahan) {
                    let h = self.j.nilai_baru(Tipe::F);
                    self.dorong(b, Inst::KonstF { hasil: h, nilai: f64::from_bits(k as u64) });
                    h
                } else { self.konst_i(b, k) };
                isi.push(Some(v));
            }
            OpCode::True | OpCode::False => { let v = self.konst_i(b, (ins.op == OpCode::True) as i64); isi.push(Some(v)); }
            OpCode::Nil | OpCode::Unit => isi.push(None),
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div if pecahan(1) || pecahan(2) => {
                let (x, y) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let (x, y) = (self.ke_f(b, x), self.ke_f(b, y));
                let op = match ins.op { OpCode::Add => Biner::FTambah, OpCode::Sub => Biner::FKurang, OpCode::Mul => Biner::FKali, _ => Biner::FBagi };
                let h = self.biner(b, op, x, y, None);
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul => {
                let (x, y) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let op = match ins.op { OpCode::Add => Biner::ITambah, OpCode::Sub => Biner::IKurang, _ => Biner::IKali };
                // Luapan di bingkai ber-stack VM dicatat: kompilasi berikutnya menghitung di 128 bit.
                let jaga = self.jaga(ip, isi, self.r.kembali != KEMBALI_BULAT);
                let h = self.biner(b, op, x, y, jaga);
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            OpCode::Mod | OpCode::Div => {
                let (x, y) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let op = if ins.op == OpCode::Mod { Biner::ISisa } else { Biner::IBagi };
                let jaga = self.jaga(ip, isi, false);
                let h = self.biner(b, op, x, y, jaga);
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne => {
                let c = match ins.op {
                    OpCode::Lt => Banding::Lt, OpCode::Le => Banding::Le, OpCode::Gt => Banding::Gt,
                    OpCode::Ge => Banding::Ge, OpCode::Eq => Banding::Eq, _ => Banding::Ne,
                };
                let (x, y) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let h = if pecahan(1) || pecahan(2) {
                    let (x, y) = (self.ke_f(b, x), self.ke_f(b, y));
                    self.biner(b, Biner::FCmp(c), x, y, None)
                } else { self.biner(b, Biner::ICmp(c), x, y, None) };
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot | OpCode::Shl | OpCode::Shr => {
                let (x, y) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let (op, jaga) = match ins.op {
                    OpCode::BitAnd => (Biner::IDan, None),
                    OpCode::BitOr => (Biner::IAtau, None),
                    OpCode::BitXor => (Biner::IXor, None),
                    OpCode::AndNot => (Biner::IDanBukan, None),
                    OpCode::Shl => (Biner::IGeserKiri, self.jaga(ip, isi, false)),
                    _ => (Biner::IGeserKanan, self.jaga(ip, isi, false)),
                };
                let h = self.biner(b, op, x, y, jaga);
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            OpCode::BitNot | OpCode::Neg | OpCode::Not | OpCode::KeBool => {
                let x = Self::ambil(isi, t - 1)?;
                let (op, tipe, jaga) = match ins.op {
                    OpCode::BitNot => (Unar::IBukanBit, Tipe::I, None),
                    OpCode::Neg if pecahan(1) => (Unar::FNeg, Tipe::F, None),
                    OpCode::Neg => (Unar::INeg, Tipe::I, self.jaga(ip, isi, false)),
                    OpCode::Not => (Unar::Bukan, Tipe::I, None),
                    _ => (Unar::KeBool, Tipe::I, None),
                };
                let h = self.j.nilai_baru(tipe);
                self.dorong(b, Inst::Unar { hasil: h, op, a: x, jaga });
                isi[t - 1] = Some(h);
            }
            // `a[i] = a[i] + x` atas angka native (`layak`): indeksnya cuma dibuang, lanjut ke `Add` biasa.
            OpCode::TumbuhIndexLokal => { isi.pop(); }
            // Method pemakai yang sudah diketahui (`Cara::Metode`): tidak ada yang diperiksa.
            OpCode::PeriksaArgMetode => {}
            OpCode::Dup => { let v = *isi.last().ok_or("stack kosong")?; isi.push(v); }
            OpCode::Swap => isi.swap(t - 2, t - 1),
            OpCode::SlideDown => { let v = isi[t - 1]; isi[t - a - 1] = v; isi.truncate(t - a); }
            OpCode::Pop => { isi.pop(); }
            OpCode::StoreLocal => { isi[a] = isi[t - 1]; }
            OpCode::SimpanBuang => { isi[a] = isi[t - 1]; isi.pop(); }
            OpCode::Jump => {
                let (ke, args) = self.tepi(b, ip, a, isi)?;
                return Ok(Some(Akhir::Lompat { ke, args }));
            }
            OpCode::JumpFalse | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep => {
                let c = Self::ambil(isi, t - 1)?;
                let tetap = matches!(ins.op, OpCode::JumpFalseKeep | OpCode::JumpTrueKeep);
                let sesudah: Vec<Option<Nilai>> = if tetap { isi.clone() } else { isi[..t - 1].to_vec() };
                return self.cabang(b, ip, a, c, &sesudah, matches!(ins.op, OpCode::JumpFalse | OpCode::JumpFalseKeep)).map(Some);
            }
            OpCode::MakeIterRentang => {
                let (lo, hi) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let d = self.biner(b, Biner::IKurang, hi, lo, self.jaga(ip, isi, false));
                let (jumlah, ada) = if a == 1 {
                    let satu = self.konst_i(b, 1);
                    let d1 = self.biner(b, Biner::ITambah, d, satu, self.jaga(ip, isi, false));
                    (d1, self.biner(b, Biner::ICmp(Banding::Le), lo, hi, None))
                } else { (d, self.biner(b, Biner::ICmp(Banding::Lt), lo, hi, None)) };
                let nol = self.konst_i(b, 0);
                let h = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::Pilih { hasil: h, jika: ada, ya: jumlah, tidak: nol });
                isi[t - 2] = Some(h);
                isi[t - 1] = Some(lo);
                isi.push(Some(nol));
            }
            OpCode::IterAda => {
                let (jumlah, idx) = (Self::ambil(isi, a)?, Self::ambil(isi, a + 2)?);
                let h = self.biner(b, Biner::ICmp(Banding::Lt), idx, jumlah, None);
                isi.push(Some(h));
            }
            OpCode::IterMaju => {
                let x = Self::ambil(isi, a)?;
                let maks = self.konst_i(b, i64::MAX);
                let c = self.biner(b, Biner::ICmp(Banding::Eq), x, maks, None);
                let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                self.dorong(b, Inst::JagaJika { kondisi: c, jaga });
                let satu = self.konst_i(b, 1);
                // Luapan sudah dijaga di atas.
                let h = self.biner(b, Biner::ITambah, x, satu, None);
                isi[a] = Some(h);
            }
            OpCode::IterVar => {
                let kk = a / 2;
                let idx = Self::ambil(isi, kk + 2)?;
                let v = if a % 2 == 0 { idx } else {
                    let awal = Self::ambil(isi, kk + 1)?;
                    self.biner(b, Biner::ITambah, awal, idx, self.jaga(ip, isi, false))
                };
                isi.push(Some(v));
            }
            OpCode::IndeksLokal | OpCode::GetIndex => {
                // `a[i]` atas larik lokal (atau salinan maya lokal): elemen bulat/pecahan dari `lambat::baca*`;
                // jenis lain = deopt spekulasi.
                let (lokal, tujuan) = match s.get(t.wrapping_sub(2)) {
                    Some(Jenis::Salinan(l)) if ins.op == OpCode::GetIndex => (*l as usize, t - 2),
                    _ if ins.op == OpCode::IndeksLokal => (a, t - 1),
                    _ => return Err("GetIndex asli bukan atas salinan".into()),
                };
                let i = Self::ambil(isi, t - 1)?;
                let pecahan = self.r.spek[ip] == Some(Jenis::Pecahan);
                let k = self.j.nilai_baru(if pecahan { Tipe::F } else { Tipe::I });
                let gagal = Some((Uji::BukanNol, Jaga { keadaan: self.keadaan_di(ip, isi), spek: true, puncak_vm: false }));
                let pb = if pecahan { Pb::BacaPecahan } else { Pb::Baca };
                self.dorong(b, Inst::Pembantu { pb, args: vec![Arg::I64(lokal as i64), Arg::Isi(i), Arg::Wadah], hasil: None, gagal, keluar: Some(k) });
                isi.truncate(tujuan);
                isi.push(Some(k));
            }
            OpCode::TulisIndexLokal => {
                let (v, i) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
                let gagal = Some((Uji::BukanNol, Jaga { keadaan: self.keadaan_di(ip, isi), spek: true, puncak_vm: false }));
                let args = vec![Arg::I64(a as i64), Arg::Isi(i), Arg::Isi(v), Arg::I32(s[t - 2].kode() as i32)];
                self.dorong(b, Inst::Pembantu { pb: Pb::Tulis, args, hasil: None, gagal, keluar: None });
                isi.truncate(t - 2);
            }
            OpCode::Return => {
                let v = Self::ambil(isi, t - 1)?;
                if self.j.tipe(v) == Tipe::L || s[t - 1] == Jenis::Rune { return Err(format!("Return atas {:?}", s[t - 1])); }
                let hasil = match s[t - 1] { Jenis::Bool => Hasil::Bool, Jenis::Pecahan => Hasil::Pecahan, _ => Hasil::Bulat };
                return Ok(Some(Akhir::Kembali { nilai: v, hasil }));
            }
            op => return Err(format!("{op:?} belum diturunkan JIR")),
        }
        Ok(None)
    }

    /// Instruksi atas bulat 128 bit (`Jenis::Lebar`), aturan `turun::turun_lebar`: `Some` = sudah diturunkan.
    #[allow(clippy::type_complexity)]
    fn lebar(&mut self, b: Blok, ip: usize, isi: &mut Vec<Option<Nilai>>, s: &[Jenis]) -> Result<Option<Option<Akhir>>, String> {
        use Jenis::{Bulat, Lebar};
        let ins = self.f.chunk.code[ip];
        let t = isi.len();
        let dua = t >= 2 && matches!((s[t - 2], s[t - 1]), (Lebar, Bulat | Lebar) | (Bulat, Lebar));
        let op = match ins.op {
            OpCode::Const if self.r.konstanta[ip].is_none() => {
                let Some(v) = self.r.konstanta128[ip] else { return Ok(None) };
                let h = self.j.nilai_baru(Tipe::L);
                self.dorong(b, Inst::KonstL { hasil: h, nilai: v });
                isi.push(Some(h));
                return Ok(Some(None));
            }
            OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne if dua => OpL::Cmp(match ins.op {
                OpCode::Lt => Banding::Lt, OpCode::Le => Banding::Le, OpCode::Gt => Banding::Gt,
                OpCode::Ge => Banding::Ge, OpCode::Eq => Banding::Eq, _ => Banding::Ne,
            }),
            OpCode::Add | OpCode::Sub | OpCode::Mul if self.r.tepi.get(&(ip, ip + 1)).and_then(|st| st.last()) == Some(&Lebar) =>
                match ins.op { OpCode::Add => OpL::Tambah, OpCode::Sub => OpL::Kurang, _ => OpL::Kali },
            OpCode::Mod | OpCode::Div if dua => if ins.op == OpCode::Mod { OpL::Sisa } else { OpL::Bagi },
            OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::AndNot if dua => match ins.op {
                OpCode::BitAnd => OpL::Dan, OpCode::BitOr => OpL::Atau, OpCode::BitXor => OpL::Xor, _ => OpL::DanBukan,
            },
            OpCode::Shl | OpCode::Shr if t >= 2 && s[t - 2] == Lebar && s[t - 1] == Bulat =>
                if ins.op == OpCode::Shl { OpL::GeserKiri } else { OpL::GeserKanan },
            OpCode::BitNot | OpCode::KeBool if t >= 1 && s[t - 1] == Lebar => {
                let x = Self::ambil(isi, t - 1)?;
                let (op, tipe) = if ins.op == OpCode::BitNot { (Unar::BukanBitL, Tipe::L) } else { (Unar::KeBoolL, Tipe::I) };
                let h = self.j.nilai_baru(tipe);
                self.dorong(b, Inst::Unar { hasil: h, op, a: x, jaga: None });
                isi[t - 1] = Some(h);
                return Ok(Some(None));
            }
            OpCode::JumpFalse | OpCode::JumpTrue | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep if t >= 1 && s[t - 1] == Lebar => {
                let x = Self::ambil(isi, t - 1)?;
                let c = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::Unar { hasil: c, op: Unar::KeBoolL, a: x, jaga: None });
                let tetap = matches!(ins.op, OpCode::JumpFalseKeep | OpCode::JumpTrueKeep);
                let sesudah: Vec<Option<Nilai>> = if tetap { isi.clone() } else { isi[..t - 1].to_vec() };
                let salah = matches!(ins.op, OpCode::JumpFalse | OpCode::JumpFalseKeep);
                return self.cabang(b, ip, ins.arg as usize, c, &sesudah, salah).map(|a| Some(Some(a)));
            }
            _ => return Ok(None),
        };
        let (x, y) = (Self::ambil(isi, t - 2)?, Self::ambil(isi, t - 1)?);
        let geser = matches!(op, OpL::GeserKiri | OpL::GeserKanan);
        let x = self.ke_l(b, x);
        let y = if geser { y } else { self.ke_l(b, y) };
        let (jaga, jaga_muat) = match op {
            OpL::Tambah | OpL::Kurang | OpL::GeserKiri | OpL::GeserKanan => (self.jaga(ip, isi, false), None),
            OpL::Bagi | OpL::Sisa => (self.jaga(ip, isi, false), self.jaga(ip, isi, true)),
            _ => (None, None),
        };
        let tipe = if matches!(op, OpL::Cmp(_) | OpL::Bagi | OpL::Sisa) { Tipe::I } else { Tipe::L };
        let h = self.j.nilai_baru(tipe);
        self.dorong(b, Inst::BinerL { hasil: h, op, a: x, b: y, jaga, jaga_muat });
        isi.truncate(t - 2);
        isi.push(Some(h));
        Ok(Some(None))
    }

    /// Bulat 128 bit di posisi `pos` ditulis ke stack VM: pembantu yang menerima kode `Lebar` (6) membacanya dari sana.
    fn lebar_ke_vm(&mut self, b: Blok, pos: usize, j: Jenis, isi: &[Option<Nilai>]) -> Result<(), String> {
        if j == Jenis::Lebar { let v = Self::ambil(isi, pos)?; self.dorong(b, Inst::TulisLebar { pos, nilai: v }); }
        Ok(())
    }

    /// Kunci tembolok bacaan field: nomor nama, privat, kode jenis angka.
    fn kunci_field(nama: usize, privat: bool, jk: i32) -> i64 { ((nama as i64) << 8) | ((privat as i64) << 4) | jk as i64 }

    /// Instruksi `ip` boleh ikut diturunkan bersama instruksi sebelumnya: bukan pemimpin, terjangkau, tanpa titik
    /// keluar dan tanpa wujud (syarat `gabung` penurun langsung).
    fn gabung(&self, ip: usize) -> bool {
        !self.r.pemimpin.get(ip).copied().unwrap_or(true) && self.r.keadaan.get(ip).is_some_and(|k| k.is_some())
            && !self.r.keluar[ip] && self.r.wujud.get(ip).is_some_and(|w| w.is_empty())
    }

    /// Callee langsung ke-`n` bisa disisipkan di `ip`: JIR-nya kecil dan cuma memakai instruksi posisional (tanpa
    /// bingkai VM, upvalue, panggilan, atau `?` yang pulang), kembaliannya seragam dan cocok dengan spekulasi hasil
    /// pemanggil, dan tiap argumen yang ia baca native ada isinya di pemanggil.
    fn bisa_sisip(&self, n: usize, ip: usize, isi: &[Option<Nilai>], s: &[Jenis]) -> bool {
        let Some(Some((c, _))) = self.sisip.get(n) else { return false };
        let a = self.f.chunk.code[ip].arg as usize;
        let t = isi.len();
        if !matches!(s.get(t.wrapping_sub(a + 1)), Some(Jenis::Fungsi(_))) { return false; }
        if c.blok.len() > 64 || c.blok.iter().map(|b| b.inst.len()).sum::<usize>() > 200 { return false; }
        let posisional = c.blok.iter().flat_map(|b| &b.inst).all(|i| matches!(i,
            Inst::KonstI { .. } | Inst::KonstF { .. } | Inst::KonstL { .. } | Inst::Biner { .. } | Inst::Unar { .. }
            | Inst::BinerL { .. } | Inst::Pilih { .. } | Inst::JagaJika { .. } | Inst::Undi { .. } | Inst::Sempit { .. }
            | Inst::Muat { .. } | Inst::Simpan { .. } | Inst::Buang { .. } | Inst::Benar { .. } | Inst::Taruh { .. }
            | Inst::Buka { .. } | Inst::BukaLebar { .. } | Inst::TulisLebar { .. } | Inst::TulisVm { .. }
            | Inst::Salin { .. } | Inst::Tukar { .. } | Inst::Pindah { .. } | Inst::Pembantu { .. } | Inst::Rantai { .. }
            | Inst::Rumus { .. } | Inst::Bangun { .. } | Inst::Panjang { .. } | Inst::TeksBulat { .. }
            | Inst::IterAda { .. } | Inst::Maju { .. } | Inst::CekMetode { .. } | Inst::Sebar { pulang: false, .. }
            | Inst::Wujud(_)));
        if !posisional { return false; }
        // Kembalian: semuanya angka native setipe spekulasi pemanggil, atau semuanya milik VM tanpa spekulasi.
        let kembali: Vec<&Akhir> = c.blok.iter().filter_map(|b| b.akhir.as_ref())
            .filter(|a| matches!(a, Akhir::Kembali { .. } | Akhir::KembaliNilai { .. } | Akhir::KembaliUnit)).collect();
        let cocok = match self.r.spek[ip] {
            Some(Jenis::Bulat) => kembali.iter().all(|a| matches!(a, Akhir::Kembali { hasil: super::Hasil::Bulat, .. })),
            Some(Jenis::Pecahan) => kembali.iter().all(|a| matches!(a, Akhir::Kembali { hasil: super::Hasil::Pecahan, .. })),
            // Hasil milik VM: `StrukturUlang` di callee jadi `Struktur` saat disalin (lihat `sisipkan`).
            None => kembali.iter().all(|a| matches!(a, Akhir::KembaliNilai { .. })),
            _ => false,
        };
        if kembali.is_empty() || !cocok { return false; }
        if c.blok.iter().any(|b| matches!(b.akhir, Some(Akhir::CabangVm { .. }) | Some(Akhir::KembaliUnit) | None)) { return false; }
        // Parameter masuk callee: posisi argumen yang di pemanggil berisi native setipe.
        let geser = t - a;
        c.posisi_masuk.iter().zip(&c.blok[0].params)
            .all(|(p, par)| isi.get(geser + p).copied().flatten().is_some_and(|v| self.j.tipe(v) == c.tipe(*par)))
            // Posisi argumen yang di pemanggil native tapi bagi callee milik VM belum ditangani.
            && (0..a).all(|k| isi[geser + k].is_none() || c.posisi_masuk.contains(&k))
    }

    /// Menyalin JIR callee langsung ke-`n` ke fungsi ini: nilai dan blok diberi nomor baru, posisi stack digeser ke
    /// atas posisi fungsinya, tiap keadaan deopt membawa keadaan pemanggil di `Call` sebagai induknya, dan
    /// kembaliannya lompat ke blok lanjutan tempat pemanggil melanjutkan (`Pembangun::lanjutan`).
    fn sisipkan(&mut self, b: Blok, ip: usize, isi: &mut Vec<Option<Nilai>>, s: &[Jenis], n: usize) -> Result<Akhir, String> {
        let (c, fid) = self.sisip[n].clone().expect("diperiksa bisa_sisip");
        let a = self.f.chunk.code[ip].arg as usize;
        let t = isi.len();
        let geser = t - a;
        let induk = Box::new(self.keadaan_di(ip, isi));
        let (dasar_n, dasar_b) = (self.j.tipe.len() as u32, self.j.blok.len() as u32);
        self.j.tipe.extend(c.tipe.iter().copied());
        let rumus0 = self.j.rumus.len();
        self.j.rumus.extend(c.rumus.iter().cloned());
        self.rantai_sisip += 1;
        let rantai0 = 1_000_000 * self.rantai_sisip;
        self.maks_sisip = self.maks_sisip.max(geser + c.maks);
        let nv = |v: Nilai| Nilai(v.0 + dasar_n);
        let nb = |x: Blok| Blok(x.0 + dasar_b);
        let keadaan = |k: &Keadaan| -> Keadaan {
            let mut x = k.clone();
            x.isi = k.isi.iter().map(|v| v.map(nv)).collect();
            x.maya = k.maya.iter().map(|m| super::Maya { tag: nv(m.tag), muatan: m.muatan.iter().map(|v| nv(*v)).collect(), ..m.clone() }).collect();
            if x.induk.is_none() { x.fid = Some(fid); x.geser = geser; x.induk = Some(induk.clone()); }
            x
        };
        let jaga = |j: &Jaga| Jaga { keadaan: keadaan(&j.keadaan), spek: j.spek, puncak_vm: j.puncak_vm };
        let jo = |j: &Option<Jaga>| j.as_ref().map(jaga);
        let lanjut = Blok(dasar_b + c.blok.len() as u32);
        let hasil = match self.r.spek[ip] {
            Some(j @ (Jenis::Bulat | Jenis::Pecahan)) => Some(self.j.nilai_baru(tipe_jenis(j)?.ok_or("hasil angka")?)),
            _ => None,
        };
        // Bayangan varian argumen pemanggil yang tidak pernah ditulis callee: rantai `match` dan payload angka atasnya
        // dilipat (callee cuma membaca posisi parameternya kalau tidak ada instruksi yang menulisnya).
        let ditulis: Option<std::collections::HashSet<usize>> = c.blok.iter().flat_map(|b| &b.inst)
            .try_fold(std::collections::HashSet::new(), |mut m, i| bayang::ditulis(i).map(|v| { m.extend(v); m }));
        let bayang_arg = |me: &Self, l: usize| -> Option<bayang::Bayang> {
            if ditulis.as_ref().is_none_or(|d| d.contains(&l)) { return None; }
            me.bayang.get(&(l + geser)).cloned()
        };
        // Callee yang membaca tak terduga atau menulis posisi argumennya: argumen varian basi diwujudkan sebelum masuk.
        let baca_semua = c.blok.iter().flat_map(|b| &b.inst).any(|i| bayang::dibaca(i).is_none());
        for l in 0..a {
            if baca_semua || ditulis.as_ref().is_none_or(|d| d.contains(&l)) { self.wujudkan(b, l + geser); }
        }
        // Konstanta bulat callee (indeks `Baca` atas tuple maya).
        let konst_callee: std::collections::HashMap<Nilai, i64> = c.blok.iter().flat_map(|b| &b.inst)
            .filter_map(|i| match i { Inst::KonstI { hasil, nilai } => Some((*hasil, *nilai)), _ => None }).collect();
        // Blok yang didominasi lipatan `PayloadLokal`: keadaan deopt-nya ikut menulis tuple-nya.
        let mut tuple_turun: std::collections::HashMap<usize, (usize, bayang::Bayang, u64)> = Default::default();
        for (bi, blk) in c.blok.iter().enumerate() {
            let mut inst = Vec::with_capacity(blk.inst.len());
            // Tuple maya (langkah 105): `PayloadLokal` yang dilipat — (posisi callee, bayangan, varian yang sah).
            let mut tmaya: Option<(usize, bayang::Bayang, u64)> = None;
            let turunan = tuple_turun.get(&bi).cloned();
            for (ii, i) in blk.inst.iter().enumerate() {
                let mut pra = Vec::new();
                // `PayloadLokal` atas argumen berbayangan yang semua variannya ber-payload tuple lengkap: tidak ditulis.
                // Cuma di lengan tuple: pembaca pertama posisinya di blok ini `Baca` elemen (lengan payload tunggal
                // membaca payload utuh, dan jaganya akan deopt tiap kali).
                if let Inst::Pembantu { pb: Pb::PayloadLokal, args, gagal: Some((_, jg)), .. } = i {
                    if let (Some(Arg::I64(l)), Some(Arg::I64(ke))) = (args.first(), args.get(1)) {
                        let ke = *ke as usize;
                        let lengan_tuple = blk.inst[ii + 1..].iter().find(|j| bayang::dibaca(j).is_none_or(|d| d.contains(&ke)))
                            .is_some_and(|j| matches!(j, Inst::Pembantu { pb: Pb::Baca | Pb::BacaPecahan, .. }));
                        // Blok yang menerima tuple maya harus disalin SESUDAH blok ini (urutan blok callee).
                        let butuh = bayang::tuple_tak_bocor(&c, bi, ke).filter(|v| lengan_tuple && v.iter().all(|b2| *b2 > bi));
                        if let (Some(x), Some(butuh)) = (bayang_arg(self, *l as usize), butuh) {
                            let baik = self.tuple_baik(&x);
                            if baik != 0 {
                                for b2 in butuh { tuple_turun.insert(b2, (ke, x.clone(), baik)); }
                                let bukan = self.lipat_tag(&mut pra, &x, |t| baik >> t & 1 == 0);
                                pra.push(Inst::JagaJika { kondisi: bukan, jaga: jaga(jg) });
                                inst.append(&mut pra);
                                tmaya = Some((ke, x, baik));
                                continue;
                            }
                        }
                    }
                }
                // `Baca` elemen konstan atas tuple maya: isi bayangannya.
                if let (Some((ke, x, baik)), Inst::Pembantu { pb: pb @ (Pb::Baca | Pb::BacaPecahan), args, keluar: Some(kv), gagal: Some((_, jg)), .. }) = (&tmaya, i) {
                    let k = match (args.first(), args.get(1)) {
                        (Some(Arg::I64(p)), Some(Arg::Isi(v))) if *p as usize == *ke => konst_callee.get(v).copied(),
                        _ => None,
                    };
                    let j = if *pb == Pb::BacaPecahan { Jenis::Pecahan } else { Jenis::Bulat };
                    // Varian yang slot k-nya bukan isi berjenis `j` tidak mungkin sampai ke lengan ini: dijaga (deopt).
                    let isi = k.and_then(|k| {
                        let m = x.muatan.get(k as usize).copied().filter(|m| m.0 == j || m.0 == Jenis::Nilai)?;
                        let ada = (0..64u32).filter(|t| self.jumlah_payload(*t) > k as usize).fold(0u64, |a, t| a | 1 << t);
                        let ok = baik & m.2 & self.slot_berjenis(k as usize, j) & ada;
                        (ok != 0).then_some((m, ok))
                    });
                    if let Some((m, ok)) = isi {
                        let x2 = x.clone();
                        if ok != *baik {
                            let mut pra2 = Vec::new();
                            let bukan = self.lipat_tag(&mut pra2, &x2, |t| ok >> t & 1 == 0);
                            inst.append(&mut pra2);
                            inst.push(Inst::JagaJika { kondisi: bukan, jaga: jaga(jg) });
                        }
                        let v = self.dari_slot(&mut inst, m.0, j, m.1);
                        inst.push(Inst::Pilih { hasil: nv(*kv), jika: x.tag, ya: v, tidak: v });
                        continue;
                    }
                }
                // Tuple maya yang dibuang tidak pernah ada di stack VM: tidak ada yang perlu dilepas.
                if matches!((&tmaya, i), (Some((ke, _, _)), Inst::Buang { pos }) if pos == ke) { tmaya = None; continue; }
                // Pembaca lain tuple maya: ditulis dulu ke stack VM; penulisnya: bayangannya selesai.
                if let Some((ke, x, baik)) = tmaya.clone() {
                    if bayang::dibaca(i).is_none_or(|d| d.contains(&ke)) {
                        let mut m = self.maya_dari(ke + geser, &bayang::Bayang { basi: baik, ..x });
                        m.tuple = true;
                        inst.push(Inst::Wujud(m));
                        tmaya = None;
                    } else if bayang::ditulis(i).is_none_or(|d| d.contains(&ke)) {
                        tmaya = None;
                    }
                }
                let lipat = match i {
                    Inst::Rantai { hasil, lokal, situs, urutan, .. } => bayang_arg(self, *lokal)
                        .and_then(|x| self.lipat_rantai(&mut pra, &x, *situs, *urutan))
                        .map(|h| Inst::Pilih { hasil: nv(*hasil), jika: h, ya: h, tidak: h }),
                    // Pola tunggal (`PolaLokal`) atas argumen berbayangan: dijawab saat kompilasi tiap varian yang mungkin.
                    Inst::Pembantu { pb: Pb::PolaLokal, args, hasil: Some(h), .. } => {
                        let (Some(Arg::I64(l)), Some(Arg::I64(situs))) = (args.first(), args.get(1)) else { unreachable!("bentuk pola_lokal") };
                        bayang_arg(self, *l as usize).and_then(|x| self.lipat_pola(&mut pra, &x, *situs))
                            .map(|v| Inst::Pilih { hasil: nv(*h), jika: v, ya: v, tidak: v })
                    }
                    Inst::Pembantu { pb: pb @ (Pb::PayloadJenisSama | Pb::PayloadPanjang), args, hasil, gagal: Some((_, jg)), keluar } => {
                        let Some(Arg::I64(l)) = args.first() else { unreachable!("posisi payload") };
                        let nama = match (pb, args.get(1)) { (Pb::PayloadJenisSama, Some(Arg::I64(a))) => Some(*a), _ => None };
                        let tujuan = if *pb == Pb::PayloadPanjang { *keluar } else { *hasil };
                        bayang_arg(self, *l as usize).and_then(|x| self.lipat_payload_bentuk(&mut pra, &x, nama)).map(|(gagal, v)| {
                            pra.push(Inst::JagaJika { kondisi: gagal, jaga: jaga(jg) });
                            let h = nv(tujuan.expect("hasil pembantu payload"));
                            Inst::Pilih { hasil: h, jika: v, ya: v, tidak: v }
                        })
                    }
                    Inst::Pembantu { pb: Pb::PayloadAngkaLokal, args, hasil: Some(st), keluar: Some(k), .. } => {
                        let (Some(Arg::I64(l)), Some(Arg::I32(jk))) = (args.first(), args.get(2)) else { unreachable!("bentuk payload_angka_lokal") };
                        let j = if *jk == 5 { Jenis::Pecahan } else { Jenis::Bulat };
                        bayang_arg(self, *l as usize).and_then(|x| self.lipat_payload(&mut pra, &x, j)).map(|(s0, v)| {
                            pra.push(Inst::Pilih { hasil: nv(*st), jika: s0, ya: s0, tidak: s0 });
                            Inst::Pilih { hasil: nv(*k), jika: s0, ya: v, tidak: v }
                        })
                    }
                    _ => None,
                };
                if let Some(x) = lipat { inst.append(&mut pra); inst.push(x); continue; }
                // Argumen varian basi yang dibaca callee: diwujudkan tepat sebelum instruksinya.
                if let Some(ps) = bayang::dibaca(i) {
                    for l in ps {
                        if let Some(x) = self.bayang.get(&(l + geser)).filter(|x| x.basi != 0) {
                            inst.push(Inst::Wujud(self.maya_dari(l + geser, x)));
                        }
                    }
                }
                inst.push(match i.clone() {
                    Inst::KonstI { hasil, nilai } => Inst::KonstI { hasil: nv(hasil), nilai },
                    Inst::KonstF { hasil, nilai } => Inst::KonstF { hasil: nv(hasil), nilai },
                    Inst::KonstL { hasil, nilai } => Inst::KonstL { hasil: nv(hasil), nilai },
                    Inst::Biner { hasil, op, a, b, jaga: j } => Inst::Biner { hasil: nv(hasil), op, a: nv(a), b: nv(b), jaga: jo(&j) },
                    Inst::Unar { hasil, op, a, jaga: j } => Inst::Unar { hasil: nv(hasil), op, a: nv(a), jaga: jo(&j) },
                    Inst::BinerL { hasil, op, a, b, jaga: j, jaga_muat } =>
                        Inst::BinerL { hasil: nv(hasil), op, a: nv(a), b: nv(b), jaga: jo(&j), jaga_muat: jo(&jaga_muat) },
                    Inst::Pilih { hasil, jika, ya, tidak } => Inst::Pilih { hasil: nv(hasil), jika: nv(jika), ya: nv(ya), tidak: nv(tidak) },
                    Inst::JagaJika { kondisi, jaga: j } => Inst::JagaJika { kondisi: nv(kondisi), jaga: jaga(&j) },
                    Inst::Undi { hasil } => Inst::Undi { hasil: nv(hasil) },
                    Inst::Sempit { hasil, a, jaga: j } => Inst::Sempit { hasil: nv(hasil), a: nv(a), jaga: jaga(&j) },
                    Inst::Muat { lokal, ke, pindah } => Inst::Muat { lokal: lokal + geser, ke: ke + geser, pindah },
                    Inst::Simpan { ke, dari, nilai, kode, buang } => Inst::Simpan { ke: ke + geser, dari: dari + geser, nilai: nilai.map(nv), kode, buang },
                    Inst::Buang { pos } => Inst::Buang { pos: pos + geser },
                    Inst::Benar { hasil, pos, lepas } => Inst::Benar { hasil: nv(hasil), pos: pos + geser, lepas },
                    Inst::Taruh { pos, alamat } => Inst::Taruh { pos: pos + geser, alamat },
                    Inst::Buka { hasil, pos, kode, jaga: j } => Inst::Buka { hasil: nv(hasil), pos: pos + geser, kode, jaga: jaga(&j) },
                    Inst::BukaLebar { hasil, pos, jaga: j } => Inst::BukaLebar { hasil: nv(hasil), pos: pos + geser, jaga: jaga(&j) },
                    Inst::TulisLebar { pos, nilai } => Inst::TulisLebar { pos: pos + geser, nilai: nv(nilai) },
                    Inst::TulisVm { pos, nilai, kode } => Inst::TulisVm { pos: pos + geser, nilai: nilai.map(nv), kode },
                    Inst::Salin { dari, ke } => Inst::Salin { dari: dari + geser, ke: ke + geser },
                    Inst::Tukar { x, y } => Inst::Tukar { x: x + geser, y: y + geser },
                    Inst::Pindah { dari, ke } => Inst::Pindah { dari: dari + geser, ke: ke + geser },
                    Inst::Pembantu { pb, args, hasil, gagal, keluar } => {
                        let posisi = posisi_pb(pb);
                        let args = args.into_iter().enumerate().map(|(k, x)| match x {
                            Arg::I64(v) if posisi.contains(&k) => Arg::I64(v + geser as i64),
                            Arg::Isi(v) => Arg::Isi(nv(v)),
                            x => x,
                        }).collect();
                        Inst::Pembantu { pb, args, hasil: hasil.map(nv), gagal: gagal.map(|(u, j)| (u, jaga(&j))), keluar: keluar.map(nv) }
                    }
                    Inst::Rantai { hasil, rantai, urutan, lokal, situs, panggil, jaga: j } =>
                        Inst::Rantai { hasil: nv(hasil), rantai: rantai + rantai0, urutan, lokal: lokal + geser, situs, panggil, jaga: jaga(&j) },
                    Inst::Rumus { rumus, lokal, args, tulis, hasil, jaga: j } => Inst::Rumus {
                        rumus: rumus + rumus0, lokal: lokal + geser, args: args.into_iter().map(|x| x.map(nv)).collect(), tulis,
                        hasil: hasil.map(nv), jaga: jaga(&j),
                    },
                    Inst::Bangun { situs, jenis, t: tb, n: nn, isi: bi, taruh } => {
                        // `jenis` dibaca menurut posisi mutlak: diawali `geser` posisi pemanggil (tak dibaca).
                        let asli = unsafe { std::slice::from_raw_parts(jenis as *const Jenis, tb) };
                        let mut j2 = vec![Jenis::Kosong; geser];
                        j2.extend_from_slice(asli);
                        let j2: &'static [Jenis] = Box::leak(j2.into_boxed_slice());
                        // Bingkai callee yang disisipkan adalah bingkai pemanggil: lokal yang dipakai ulang
                        // `StrukturUlang` milik pemanggil dan masih terlihat sesudahnya, jadi jadi `Struktur` biasa.
                        use crate::jit::lambat::SitusBangun;
                        let situs = match unsafe { &*(situs as *const SitusBangun) } {
                            SitusBangun::StrukturUlang(l, b) =>
                                SitusBangun::bocor(SitusBangun::Struktur(l.clone(), b.clone())) as *const _ as i64,
                            _ => situs,
                        };
                        Inst::Bangun {
                            situs, jenis: j2.as_ptr() as i64, t: tb + geser, n: nn,
                            isi: bi.into_iter().map(|(p, v)| (p + geser, nv(v))).collect(),
                            taruh: taruh.into_iter().map(|(p, x)| (p + geser, x)).collect(),
                        }
                    }
                    Inst::Panjang { hasil, pos, lokal, jaga: j } => Inst::Panjang { hasil: nv(hasil), pos: pos + geser, lokal, jaga: jaga(&j) },
                    Inst::TeksBulat { pos, n } => Inst::TeksBulat { pos: pos + geser, n: nv(n) },
                    Inst::IterAda { hasil, slot, jaga: j } => Inst::IterAda { hasil: nv(hasil), slot: slot + geser, jaga: jaga(&j) },
                    Inst::Maju { slot, jaga: j } => Inst::Maju { slot: slot + geser, jaga: jaga(&j) },
                    Inst::CekMetode { pos, label, jaga: j } => Inst::CekMetode { pos: pos + geser, label, jaga: jaga(&j) },
                    Inst::Sebar { pos, pulang, keluarga, jaga: j } => Inst::Sebar { pos: pos + geser, pulang, keluarga, jaga: jaga(&j) },
                    Inst::Wujud(m) => Inst::Wujud(super::Maya {
                        pos: m.pos + geser, tag: nv(m.tag), muatan: m.muatan.iter().map(|v| nv(*v)).collect(), ..m
                    }),
                    lain => return Err(format!("instruksi callee tak bisa disisipkan: {lain:?}")),
                });
                // Deopt sesudah `PayloadLokal` yang dilipat menulis tuple-nya dulu (posisi callee, digeser `deopt`).
                if let Some((ke, x, baik)) = tmaya.as_ref().or(turunan.as_ref()) {
                    let mut m = self.maya_dari(*ke, &bayang::Bayang { basi: *baik, ..x.clone() });
                    m.tuple = true;
                    if let Some(akhir) = inst.last_mut() {
                        for j in bayang::jaga_inst(akhir) { j.keadaan.maya.push(m.clone()); }
                    }
                }
            }
            let akhir = match blk.akhir.clone().expect("diperiksa") {
                Akhir::Lompat { ke, args } => Akhir::Lompat { ke: nb(ke), args: args.into_iter().map(nv).collect() },
                Akhir::Cabang { kondisi, ya, ya_args, tidak, tidak_args } => Akhir::Cabang {
                    kondisi: nv(kondisi), ya: nb(ya), ya_args: ya_args.into_iter().map(nv).collect(),
                    tidak: nb(tidak), tidak_args: tidak_args.into_iter().map(nv).collect(),
                },
                Akhir::Keluar { keadaan: k } => {
                    let mut kk = keadaan(&k);
                    if let Some((ke, x, baik)) = tmaya.as_ref().or(turunan.as_ref()) {
                        let mut m = self.maya_dari(*ke, &bayang::Bayang { basi: *baik, ..x.clone() });
                        m.tuple = true;
                        kk.maya.push(m);
                    }
                    Akhir::Keluar { keadaan: kk }
                }
                Akhir::Kembali { nilai, .. } => Akhir::Lompat { ke: lanjut, args: vec![nv(nilai)] },
                // Hasil milik VM: ke posisi fungsinya, seperti `Return` VM.
                Akhir::KembaliNilai { pos } => {
                    if let Some((ke, x, baik)) = tmaya.take().filter(|t| t.0 == pos) {
                        let mut m = self.maya_dari(ke + geser, &bayang::Bayang { basi: baik, ..x });
                        m.tuple = true;
                        inst.push(Inst::Wujud(m));
                    }
                    if let Some(x) = self.bayang.get(&(pos + geser)).filter(|x| x.basi != 0) {
                        inst.push(Inst::Wujud(self.maya_dari(pos + geser, x)));
                    }
                    inst.push(Inst::Pindah { dari: pos + geser, ke: geser - 1 });
                    Akhir::Lompat { ke: lanjut, args: Vec::new() }
                }
                lain => return Err(format!("akhir callee tak bisa disisipkan: {lain:?}")),
            };
            self.j.blok.push(DataBlok { ip: blk.ip, params: blk.params.iter().map(|p| nv(*p)).collect(), inst, akhir: Some(akhir) });
        }
        self.j.blok.push(DataBlok { ip: ip + 1, params: hasil.into_iter().collect(), inst: Vec::new(), akhir: None });
        let _ = s;
        // Masuk: parameter blok masuk callee = isi native argumennya di pemanggil.
        let args: Vec<Nilai> = c.posisi_masuk.iter().map(|p| isi[*p + geser]).collect::<Option<Vec<_>>>()
            .ok_or("argumen callee tanpa isi native")?;
        // Pemanggil melanjutkan di blok lanjutan: stack sampai posisi fungsi, lalu hasilnya.
        let mut sesudah: Vec<Option<Nilai>> = isi[..geser - 1].to_vec();
        sesudah.push(hasil);
        self.lanjutan = Some((lanjut, sesudah));
        isi.truncate(geser - 1);
        isi.push(hasil);
        Ok(Akhir::Lompat { ke: nb(c.masuk), args })
    }

    fn ke_l(&mut self, b: Blok, v: Nilai) -> Nilai {
        if self.j.tipe(v) == Tipe::L { return v; }
        let h = self.j.nilai_baru(Tipe::L);
        self.dorong(b, Inst::Unar { hasil: h, op: Unar::KeL, a: v, jaga: None });
        h
    }

    /// `Call`: callee bulat yang dikompilasi native, atau intrinsik `int`/`uint`/`float` atas bulat.
    fn panggil(&mut self, b: Blok, ip: usize, isi: &mut Vec<Option<Nilai>>, s: &[Jenis]) -> Result<(), String> {
        use crate::jit::layak::{BUNGKUS_KE_BULAT, BUNGKUS_KE_PECAHAN};
        let a = self.f.chunk.code[ip].arg as usize;
        let t = isi.len();
        if a + 1 > t { return Err("argumen panggilan di luar stack".into()); }
        if a == 2 && s[t - 3].wrap() {
            // `math::wrap_*`: modulo 2^64 atas 64 bit bawah, hasil tak bertanda di 128 bit.
            let Jenis::Bungkus(jenis, _) = s[t - 3] else { unreachable!() };
            let bawah = |me: &mut Self, i: usize| -> Result<Nilai, String> {
                let v = Self::ambil(isi, i)?;
                if me.j.tipe(v) != Tipe::L { return Ok(v); }
                let h = me.j.nilai_baru(Tipe::I);
                me.dorong(b, Inst::Unar { hasil: h, op: Unar::Rendah, a: v, jaga: None });
                Ok(h)
            };
            let (x, y) = (bawah(self, t - 2)?, bawah(self, t - 1)?);
            let op = match jenis {
                crate::jit::layak::BUNGKUS_TAMBAH => Biner::ITambah,
                crate::jit::layak::BUNGKUS_KURANG => Biner::IKurang,
                _ => Biner::IKali,
            };
            let w = self.biner(b, op, x, y, None);
            let h = self.j.nilai_baru(Tipe::L);
            self.dorong(b, Inst::Unar { hasil: h, op: Unar::KeLU, a: w, jaga: None });
            isi.truncate(t - 3);
            isi.push(Some(h));
            return Ok(());
        }
        if a == 2 && s[t - 3].bungkus(crate::jit::layak::BUNGKUS_CHAR_AT) && s[t - 1] == Jenis::Bulat {
            // `str::char_at` atas teks lokal (salinan maya) atau milik VM: kode karakternya; −1 (indeks negatif, di luar
            // batas, bukan teks) = deopt, VM yang menjawab atau melapor.
            let i = Self::ambil(isi, t - 1)?;
            let (pb, asal) = match s[t - 2] {
                Jenis::Salinan(l) => (Pb::KarakterLokal, l as i64),
                Jenis::Nilai => (Pb::Karakter, t as i64 - 2),
                lain => return Err(format!("char_at atas {lain:?}")),
            };
            let h = self.j.nilai_baru(Tipe::I);
            let gagal = Some((Uji::Negatif, Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false }));
            self.dorong(b, Inst::Pembantu { pb, args: vec![Arg::I64(asal), Arg::Isi(i)], hasil: Some(h), gagal, keluar: None });
            isi.truncate(t - 3);
            isi.push(Some(h));
            return Ok(());
        }
        if a == 1 && s[t - 1] == Jenis::Rune && matches!(s[t - 2], Jenis::Bungkus(BUNGKUS_KE_BULAT, _)) {
            // `int`/`uint` atas rune: kodenya sendiri.
            let v = Self::ambil(isi, t - 1)?;
            isi.truncate(t - 2);
            isi.push(Some(v));
            return Ok(());
        }
        match s[t - a - 1] {
            Jenis::Bungkus(BUNGKUS_KE_BULAT, k) if a == 1 && s[t - 1] == Jenis::Lebar => {
                let x = Self::ambil(isi, t - 1)?;
                let nama = match self.f.chunk.constants.get(k as usize) {
                    Some(crate::vm::value::VmValue::NativeFn(g)) => crate::vm::info_native::info(*g).map(|i| i.kanonik.clone()),
                    _ => None,
                };
                if nama.as_deref() == Some("uint") {
                    let nol = self.j.nilai_baru(Tipe::L);
                    self.dorong(b, Inst::KonstL { hasil: nol, nilai: 0 });
                    let neg = self.j.nilai_baru(Tipe::I);
                    self.dorong(b, Inst::BinerL { hasil: neg, op: OpL::Cmp(Banding::Lt), a: x, b: nol, jaga: None, jaga_muat: None });
                    let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                    self.dorong(b, Inst::JagaJika { kondisi: neg, jaga });
                }
                let h = if self.keadaan(ip + 1).get(t - 2) == Some(&Jenis::Bulat) {
                    let h = self.j.nilai_baru(Tipe::I);
                    let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: true, puncak_vm: false };
                    self.dorong(b, Inst::Sempit { hasil: h, a: x, jaga });
                    h
                } else { x };
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            Jenis::Bungkus(BUNGKUS_KE_BULAT, k) if a == 1 && s[t - 1] == Jenis::Bulat => {
                let v = Self::ambil(isi, t - 1)?;
                // `uint` atas bulat negatif: VM yang melaporkannya.
                let nama = match self.f.chunk.constants.get(k as usize) {
                    Some(crate::vm::value::VmValue::NativeFn(g)) => crate::vm::info_native::info(*g).map(|i| i.kanonik.clone()),
                    _ => None,
                };
                if nama.as_deref() == Some("uint") {
                    let nol = self.konst_i(b, 0);
                    let neg = self.biner(b, Biner::ICmp(Banding::Lt), v, nol, None);
                    let jaga = Jaga { keadaan: self.keadaan_di(ip, isi), spek: false, puncak_vm: false };
                    self.dorong(b, Inst::JagaJika { kondisi: neg, jaga });
                }
                isi.truncate(t - 2);
                isi.push(Some(v));
            }
            Jenis::Bungkus(BUNGKUS_KE_PECAHAN, _) if a == 1 && matches!(s[t - 1], Jenis::Bulat | Jenis::Pecahan) => {
                let v = Self::ambil(isi, t - 1)?;
                let h = self.ke_f(b, v);
                isi.truncate(t - 2);
                isi.push(Some(h));
            }
            Jenis::Fungsi(c) if self.r.callee_asli().contains(&c) => {
                let args: Vec<Nilai> = (t - a..t).map(|i| Self::ambil(isi, i)).collect::<Result<_, _>>()?;
                if args.iter().any(|v| self.j.tipe(*v) != Tipe::I) { return Err("argumen callee bulat bukan bulat".into()); }
                let di = self.keadaan_di(ip, isi);
                let sesudah = self.keadaan_di(ip + 1, &isi[..t - a - 1]);
                let h = self.j.nilai_baru(Tipe::I);
                self.dorong(b, Inst::Panggil { hasil: h, callee: c, args, di, sesudah });
                isi.truncate(t - a - 1);
                isi.push(Some(h));
            }
            j => return Err(format!("panggilan atas {j:?} belum diturunkan JIR")),
        }
        Ok(())
    }
}
