//! Bayangan varian (JIR-3b, `jir-rencana.md` langkah 103–104): posisi berisi varian yang penanda dan payload
//! angkanya isi native; `pola_pilih` dan bukaan payload dilipat darinya, dibawa lintas titik temu sebagai parameter
//! blok. Varian berbayangan lengkap tidak dibangun (`basi`) dan ditulis ke stack VM (`Inst::Wujud`) sebelum dibaca.

use std::collections::HashMap;

use super::{Akhir, Arg, Banding, Biner, Blok, Inst, Jenis, Nilai, Pb, Pembangun, Tipe};
use crate::jit::jir::Maya;
use crate::vm::value::{Label, NamaVarian, VmValue};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Bayang {
    /// Indeks varian di `Pembangun::varian`.
    pub tag: Nilai,
    /// Bit tag yang mungkin.
    pub mungkin: u64,
    /// Isi payload: jenisnya (`Bulat`/`Pecahan`), nilainya, dan bit tag yang payload-nya memang isi ini.
    pub muatan: Vec<(Jenis, Nilai, u64)>,
    /// Bit tag yang isi stack VM-nya mungkin basi (varian tidak dibangun): diwujudkan sebelum dibaca.
    pub basi: u64,
}

/// Bayangan per posisi, urut posisi.
pub(super) type Potret = Vec<(usize, Bayang)>;

/// Posisi stack yang ditulis `i`; `None` = tidak diketahui (dianggap semua).
pub(super) fn ditulis(i: &Inst) -> Option<Vec<usize>> {
    Some(match i {
        Inst::KonstI { .. } | Inst::KonstF { .. } | Inst::KonstL { .. } | Inst::Biner { .. } | Inst::Unar { .. }
        | Inst::BinerL { .. } | Inst::Pilih { .. } | Inst::JagaJika { .. } | Inst::Undi { .. } | Inst::Sempit { .. }
        | Inst::Rantai { .. } | Inst::CekMetode { .. } | Inst::IterAda { .. } | Inst::Wujud(_) => vec![],
        Inst::Muat { lokal, ke, pindah } => if *pindah { vec![*ke, *lokal] } else { vec![*ke] },
        Inst::Simpan { ke, dari, buang, .. } => if *buang { vec![*ke, *dari] } else { vec![*ke] },
        Inst::Salin { ke, .. } => vec![*ke],
        Inst::Pindah { dari, ke } => vec![*ke, *dari],
        Inst::Tukar { x, y } => vec![*x, *y],
        Inst::Buang { pos } | Inst::Taruh { pos, .. } | Inst::TulisVm { pos, .. } | Inst::TulisLebar { pos, .. }
        | Inst::TeksBulat { pos, .. } | Inst::Sebar { pos, .. } | Inst::Buka { pos, .. } | Inst::BukaLebar { pos, .. } => vec![*pos],
        Inst::Maju { slot, .. } => vec![*slot],
        Inst::Benar { pos, lepas, .. } => if *lepas { vec![*pos] } else { vec![] },
        Inst::Panjang { pos, lokal, .. } => if *lokal { vec![] } else { vec![*pos] },
        Inst::Bangun { t, n, .. } => (t - n..*t).collect(),
        Inst::Rumus { lokal, .. } => vec![*lokal],
        Inst::Pembantu { pb, args, .. } => {
            let pos = |k: usize| match args.get(k) { Some(Arg::I64(p)) => Some(*p as usize), _ => None };
            match pb {
                Pb::PolaLokal | Pb::PayloadJenisSama | Pb::PayloadPanjang | Pb::KarakterLokal => vec![],
                Pb::PayloadLokal | Pb::PayloadAngkaLokal => pos(1).into_iter().collect(),
                Pb::FieldLokal => pos(3).into_iter().collect(),
                pb => super::posisi_pb(*pb).iter().filter_map(|k| pos(*k)).collect(),
            }
        }
        _ => return None,
    })
}

/// Posisi stack yang dibaca `i` dari stack VM; `None` = tidak diketahui (dianggap semua).
pub(super) fn dibaca(i: &Inst) -> Option<Vec<usize>> {
    Some(match i {
        Inst::KonstI { .. } | Inst::KonstF { .. } | Inst::KonstL { .. } | Inst::Biner { .. } | Inst::Unar { .. }
        | Inst::BinerL { .. } | Inst::Pilih { .. } | Inst::JagaJika { .. } | Inst::Undi { .. } | Inst::Sempit { .. }
        | Inst::Wujud(_) | Inst::Taruh { .. } | Inst::TulisVm { .. } | Inst::TulisLebar { .. } | Inst::TeksBulat { .. } => vec![],
        Inst::Muat { lokal, .. } => vec![*lokal],
        Inst::Simpan { dari, .. } | Inst::Salin { dari, .. } | Inst::Pindah { dari, .. } => vec![*dari],
        Inst::Tukar { x, y } => vec![*x, *y],
        Inst::Buang { pos } | Inst::Benar { pos, .. } | Inst::Buka { pos, .. } | Inst::BukaLebar { pos, .. }
        | Inst::Panjang { pos, .. } | Inst::CekMetode { pos, .. } | Inst::Sebar { pos, .. } => vec![*pos],
        Inst::IterAda { slot, .. } | Inst::Maju { slot, .. } => vec![*slot],
        Inst::Rantai { lokal, .. } | Inst::Rumus { lokal, .. } => vec![*lokal],
        Inst::Bangun { t, n, .. } => (t - n..*t).collect(),
        Inst::Pembantu { pb, args, .. } => super::posisi_pb(*pb).iter()
            .filter_map(|k| match args.get(*k) { Some(Arg::I64(p)) => Some(*p as usize), _ => None }).collect(),
        _ => return None,
    })
}

impl Pembangun<'_> {
    /// Penanda varian `nama` berpayload `payload` dengan slot pecahan `apung`: varian yang sama dengan jenis isi lain
    /// mendapat penanda lain, jadi jenis tiap isi selalu milik penandanya (`TabelVarian`).
    pub(super) fn tag_varian(&mut self, nama: &'static NamaVarian, payload: Option<usize>, konst: usize, apung: u64) -> Option<u32> {
        let tabel = self.tabel;
        let i = match self.varian.iter().enumerate().position(|(k, (n, p))| std::ptr::eq(*n, nama) && *p == payload
            && tabel.0.read().unwrap_or_else(|e| e.into_inner())[k].3 == apung) {
            Some(i) => i,
            None => {
                self.varian.push((nama, payload));
                self.tabel.0.write().unwrap_or_else(|p| p.into_inner()).push((nama, payload, konst, apung));
                self.varian.len() - 1
            }
        };
        (i < 64).then_some(i as u32)
    }

    /// Posisi `pos` kini varian `nama` dengan payload `muatan` (atau tanpa isi native: `payload` jumlahnya saja);
    /// `basi` = varian itu tidak ditulis ke stack VM (bayangannya harus lengkap).
    pub(super) fn lahir_varian(&mut self, b: Blok, pos: usize, nama: &'static NamaVarian, payload: Option<usize>,
        muatan: Vec<(Jenis, Nilai)>, konst: usize, basi: bool) -> bool {
        if !self.bayang_aktif { return false; }
        let apung = muatan.iter().enumerate().filter(|(_, m)| m.0 == Jenis::Pecahan).fold(0u64, |a, (k, _)| a | 1 << k);
        let Some(i) = self.tag_varian(nama, payload, konst, apung) else { return false };
        let tag = self.konst_i(b, i as i64);
        let bit = 1u64 << i;
        let x = Bayang { tag, mungkin: bit, muatan: muatan.into_iter().map(|(j, v)| (j, v, bit)).collect(), basi: 0 };
        let basi = basi && self.lengkap(&x) & bit != 0;
        self.bayang.insert(pos, Bayang { basi: if basi { bit } else { 0 }, ..x });
        self.lahir.push(pos);
        basi
    }

    /// Konstanta ke-`k` yang berupa varian tanpa payload (maya bagi `Rencana`: isinya belum di stack VM).
    pub(super) fn lahir_konst(&mut self, b: Blok, pos: usize, k: usize) {
        if let Some(c @ VmValue::Map(m)) = self.f.chunk.constants.get(k) {
            if let Label::Enum { nama, payload: None } = &m.label {
                self.lahir_varian(b, pos, nama, None, Vec::new(), c as *const VmValue as usize, true);
            }
        }
    }

    /// `BuildTuple` tepat sebelum `BuildEnum` di `ip` (puncak `t`) yang baru dibangun dari angka native saja, sebagai
    /// instruksi terakhir blok `b`: isinya, urut elemen.
    pub(super) fn tuple_native(&self, b: Blok, ip: usize, t: usize) -> Option<Vec<(Jenis, Nilai)>> {
        let kode = &self.f.chunk.code;
        if ip == 0 || self.r.pemimpin[ip] || kode[ip - 1].op != crate::vm::opcode::OpCode::BuildTuple { return None; }
        let n = kode[ip - 1].arg as usize;
        let Some(Inst::Bangun { situs, t: tt, n: nn, isi, taruh, .. }) = self.j.blok[b.0 as usize].inst.last() else { return None };
        let tuple = matches!(unsafe { &*(*situs as *const crate::jit::lambat::SitusBangun) }, crate::jit::lambat::SitusBangun::Tuple);
        if !tuple || *nn != n || *tt != t - 1 + n || !taruh.is_empty() { return None; }
        let s = self.r.keadaan[ip - 1].as_ref()?;
        (0..n).map(|k| {
            let p = t - 1 + k;
            let j = *s.get(p)?;
            if !matches!(j, Jenis::Bulat | Jenis::Pecahan) { return None; }
            isi.iter().find(|(q, _)| *q == p).map(|(_, v)| (j, *v))
        }).collect()
    }

    /// Bit tag yang bisa dibangun ulang dari bayangan `x` saja.
    pub(super) fn lengkap(&self, x: &Bayang) -> u64 {
        let t = self.tabel.0.read().unwrap_or_else(|p| p.into_inner());
        (0..64).filter(|i| x.mungkin >> i & 1 == 1).filter(|&i| match t[i] {
            (_, _, k, _) if k != 0 => true,
            (_, None, _, _) => true,
            (_, Some(n), _, _) => x.muatan.len() >= n && x.muatan[..n].iter().all(|m| m.2 >> i & 1 == 1),
        }).fold(0, |m, i| m | 1 << i)
    }

    pub(super) fn maya_dari(&self, pos: usize, x: &Bayang) -> Maya {
        Maya {
            pos, tag: x.tag, muatan: x.muatan.iter().map(|m| m.1).collect(), basi: x.basi,
            pecahan: x.muatan.iter().enumerate().filter(|(_, m)| m.0 == Jenis::Pecahan).fold(0, |a, (k, _)| a | 1 << k),
            tabel: self.tabel as *const _ as i64, tuple: false,
        }
    }

    /// Varian basi di posisi `pos` ditulis ke stack VM di blok `b` (langsung, tanpa `dorong`).
    pub(super) fn wujudkan(&mut self, b: Blok, pos: usize) {
        let Some(x) = self.bayang.get(&pos).cloned() else { return };
        if x.basi == 0 { return; }
        let m = self.maya_dari(pos, &x);
        self.j.blok[b.0 as usize].inst.push(Inst::Wujud(m));
        if self.kini_b == Some(b) { if let Some(y) = self.bayang.get_mut(&pos) { y.basi = 0; } }
    }

    /// `dorong` atas blok yang sedang dibangun: pemindahan varian basi tidak dikerjakan (bayangannya yang pindah,
    /// `true`); selain itu varian basi yang dibaca instruksinya diwujudkan dulu.
    pub(super) fn cegat(&mut self, b: Blok, i: &Inst) -> bool {
        let basi = |me: &Self, p: &usize| me.bayang.get(p).is_some_and(|x| x.basi != 0);
        // Cuma kalau SEMUA varian yang mungkin basi: tujuannya tidak ditulis, jadi basi untuk semuanya.
        let semua = |me: &Self, p: &usize| me.bayang.get(p).is_some_and(|x| x.basi == x.mungkin);
        let lewat = match i {
            Inst::Simpan { dari, nilai: None, .. } => semua(self, dari),
            Inst::Muat { lokal, .. } => semua(self, lokal),
            Inst::Salin { dari, .. } | Inst::Pindah { dari, .. } => semua(self, dari),
            Inst::Buang { pos } => basi(self, pos),
            _ => false,
        };
        if lewat { return true; }
        // Pemindahan yang tetap dikerjakan menyalin isi basinya apa adanya (bayangannya ikut, dengan bit basi sama).
        if matches!(i, Inst::Simpan { nilai: None, .. } | Inst::Muat { .. } | Inst::Salin { .. } | Inst::Pindah { .. }) { return false; }
        let baca: Vec<usize> = match dibaca(i) {
            Some(ps) => ps,
            None => self.bayang.keys().copied().collect(),
        };
        for p in baca { if basi(self, &p) { self.wujudkan(b, p); } }
        false
    }

    /// Semua varian basi blok ini diwujudkan (sebelum VM menjalankan sesuatu atasnya).
    pub(super) fn wujudkan_semua(&mut self, b: Blok) {
        let mut ps: Vec<usize> = self.bayang.iter().filter(|(_, x)| x.basi != 0).map(|(p, _)| *p).collect();
        ps.sort();
        for p in ps { self.wujudkan(b, p); }
    }

    /// Bayangan basi di keadaan deopt (posisi di bawah `panjang`).
    pub(super) fn maya_keadaan(&self, panjang: usize) -> Vec<Maya> {
        let mut v: Vec<Maya> = self.bayang.iter().filter(|(p, x)| x.basi != 0 && **p < panjang).map(|(p, x)| self.maya_dari(*p, x)).collect();
        v.sort_by_key(|m| m.pos);
        v
    }

    pub(super) fn salin_bayang(&mut self, dari: usize, ke: usize) {
        match self.bayang.get(&dari).cloned() {
            Some(x) => { self.bayang.insert(ke, x); self.lahir.push(ke); }
            None => { self.bayang.remove(&ke); }
        }
    }

    /// Instruksi `i` didorong ke blok yang sedang dibangun: bayangan posisi yang ditulisnya ikut berpindah atau hilang.
    pub(super) fn bayang_inst(&mut self, i: &Inst) {
        match i {
            Inst::Muat { lokal, ke, pindah } => { self.salin_bayang(*lokal, *ke); if *pindah { self.bayang.remove(lokal); } }
            Inst::Simpan { ke, dari, buang, .. } => { self.salin_bayang(*dari, *ke); if *buang { self.bayang.remove(dari); } }
            Inst::Salin { dari, ke } => self.salin_bayang(*dari, *ke),
            Inst::Pindah { dari, ke } => { self.salin_bayang(*dari, *ke); self.bayang.remove(dari); }
            Inst::Tukar { x, y } => {
                let (a, b) = (self.bayang.remove(x), self.bayang.remove(y));
                if let Some(a) = a { self.bayang.insert(*y, a); self.lahir.push(*y); }
                if let Some(b) = b { self.bayang.insert(*x, b); self.lahir.push(*x); }
            }
            // Panggilan tidak menyentuh lokal di bawah posisi fungsinya (nilai tidak beralias; tanpa closure).
            Inst::Langsung { t, a, .. } => { let d = t - a - 1; self.bayang.retain(|p, _| *p < d); }
            Inst::Bawaan { t, argc, .. } => { let d = t.saturating_sub((*argc as usize & 0xffff) + 1); self.bayang.retain(|p, _| *p < d); }
            Inst::BinerVm { t, .. } => { let d = t.saturating_sub(2); self.bayang.retain(|p, _| *p < d); }
            Inst::MuatUpval { ke, .. } => { self.bayang.remove(ke); }
            i => match ditulis(i) {
                Some(ps) => for p in ps { self.bayang.remove(&p); },
                None => self.bayang.clear(),
            },
        }
    }

    /// Sesudah satu instruksi bytecode yang mulai dengan tinggi stack `t0`: posisi yang tersentuh puncak tanpa
    /// dilahirkan, di atas stack, atau berisi native tidak berbayangan.
    pub(super) fn rapikan_bayang(&mut self, t0: usize, isi: &[Option<super::Nilai>]) {
        let h = t0.min(isi.len());
        let lahir = std::mem::take(&mut self.lahir);
        self.bayang.retain(|p, _| (*p < h || lahir.contains(p)) && isi.get(*p).is_some_and(|v| v.is_none()));
    }

    /// Tepi dari blok `asal` ke pemimpin `ke` (blok `sasaran`) yang stack-nya `panjang`: argumen bayangan tambahan
    /// kalau bloknya sudah dibangun, selain itu dicatat untuk dipasang saat bloknya mulai.
    pub(super) fn tepi_bayang(&mut self, asal: Blok, sasaran: Blok, ke: usize, panjang: usize) -> Result<Vec<Nilai>, String> {
        let mut potret: Potret = self.bayang.iter().filter(|(p, _)| **p < panjang).map(|(p, b)| (*p, b.clone())).collect();
        potret.sort_by_key(|x| x.0);
        let Some(params) = self.masuk_bayang.get(&ke).cloned() else {
            self.sisi.entry(ke).or_default().push((asal, sasaran, potret));
            return Ok(Vec::new());
        };
        match self.args_bayang(asal, &params, &potret) {
            Some(args) => Ok(args),
            None => {
                self.ulang = Some(ke);
                Err(format!("bayangan varian di ip {ke} tidak cocok dengan tepi yang datang belakangan"))
            }
        }
    }

    /// Argumen parameter bayangan `params` dari potret tepi yang berangkat dari `asal`; `None` = tidak cocok.
    fn args_bayang(&mut self, asal: Blok, params: &Potret, potret: &Potret) -> Option<Vec<Nilai>> {
        // Varian basi yang bayangannya tidak dibawa ke tujuan: ditulis di asal tepinya.
        for (pos, e) in potret {
            if e.basi != 0 && !params.iter().any(|(p, _)| p == pos) {
                let m = self.maya_dari(*pos, e);
                self.j.blok[asal.0 as usize].inst.push(Inst::Wujud(m));
                if self.kini_b == Some(asal) { if let Some(y) = self.bayang.get_mut(pos) { y.basi = 0; } }
            }
        }
        let mut args = Vec::new();
        for (pos, pb) in params {
            let e = potret.iter().find(|(p, _)| p == pos).map(|x| x.1.clone()).filter(|e| {
                e.mungkin & !pb.mungkin == 0 && pb.muatan.iter().enumerate().all(|(k, (j, _, sah))| {
                    let perlu = e.mungkin & sah;
                    perlu == 0 || e.muatan.get(k).is_some_and(|(je, _, se)| (je == j || *j == Jenis::Nilai) && perlu & !se == 0)
                })
            })?;
            // Varian yang tujuannya anggap sudah di stack VM: ditulis di asal tepinya.
            if e.basi & !pb.basi != 0 {
                let m = self.maya_dari(*pos, &e);
                self.j.blok[asal.0 as usize].inst.push(Inst::Wujud(m));
                if self.kini_b == Some(asal) { if let Some(y) = self.bayang.get_mut(pos) { y.basi = 0; } }
            }
            args.push(e.tag);
            for (k, (j, _, _)) in pb.muatan.iter().enumerate() {
                let v = match e.muatan.get(k) {
                    Some((je, v, _)) if je == j => *v,
                    // Ke slot bit campur: pecahan dibawa bitnya, bulat apa adanya.
                    Some((Jenis::Pecahan, v, _)) if *j == Jenis::Nilai => {
                        let h = self.j.nilai_baru(Tipe::I);
                        self.j.blok[asal.0 as usize].inst.push(Inst::Unar { hasil: h, op: super::Unar::BitF, a: *v, jaga: None });
                        h
                    }
                    Some((_, v, _)) if *j == Jenis::Nilai => *v,
                    _ => self.isi_nol(asal, *j),
                };
                args.push(v);
            }
        }
        Some(args)
    }

    fn isi_nol(&mut self, b: Blok, j: Jenis) -> Nilai {
        if j == Jenis::Pecahan {
            let h = self.j.nilai_baru(Tipe::F);
            self.dorong(b, Inst::KonstF { hasil: h, nilai: 0.0 });
            h
        } else { self.konst_i(b, 0) }
    }

    /// Pemimpin `ip` (blok `b`) mulai dibangun: bayangan yang dibawa SEMUA tepi yang sudah tercatat jadi parameter
    /// tambahannya, dan tepi itu diberi argumennya. Tepi yang datang belakangan (loop) diperiksa `tepi_bayang`.
    pub(super) fn mulai_bayang(&mut self, ip: usize, b: Blok) {
        self.bayang.clear();
        self.lahir.clear();
        let mut tepi = self.sisi.remove(&ip).unwrap_or_default();
        let mut lihat = Vec::new();
        tepi.retain(|(a, s, _)| if lihat.contains(&(*a, *s)) { false } else { lihat.push((*a, *s)); true });
        if !self.bayang_aktif || self.larang.contains(&ip) || tepi.is_empty() {
            self.masuk_bayang.insert(ip, Vec::new());
            for (asal, _, potret) in tepi { self.args_bayang(asal, &Vec::new(), &potret); }
            return;
        }
        let mut params: Potret = Vec::new();
        for (pos, _) in &tepi[0].2 {
            let semua: Vec<&Bayang> = tepi.iter().filter_map(|(_, _, p)| p.iter().find(|(q, _)| q == pos).map(|x| &x.1)).collect();
            if semua.len() != tepi.len() { continue; }
            let mungkin = semua.iter().fold(0, |m, e| m | e.mungkin);
            let lebar = semua.iter().map(|e| e.muatan.len()).max().unwrap_or(0);
            let mut muatan = Vec::new();
            for k in 0..lebar {
                let j0 = semua.iter().find_map(|e| e.muatan.get(k).map(|x| x.0)).expect("lebar dari salah satunya");
                // Jenis bercampur antar-tepi: slotnya dibawa sebagai bit (`Nilai`), jenisnya milik penanda (tabel).
                let j = if semua.iter().any(|e| e.muatan.get(k).is_some_and(|x| x.0 != j0)) { Jenis::Nilai } else { j0 };
                let ada = semua.iter().fold(0, |m, e| m | e.muatan.get(k).map_or(0, |x| x.2));
                let hilang = semua.iter().fold(0, |m, e| m | (e.mungkin & !e.muatan.get(k).map_or(0, |x| x.2)));
                let v = self.j.nilai_baru(if j == Jenis::Pecahan { Tipe::F } else { Tipe::I });
                muatan.push((j, v, ada & !hilang));
            }
            let mut x = Bayang { tag: self.j.nilai_baru(Tipe::I), mungkin, muatan, basi: 0 };
            x.basi = semua.iter().fold(0, |m, e| m | e.basi) & self.lengkap(&x);
            params.push((*pos, x));
        }
        for (_, x) in &params {
            let blok = &mut self.j.blok[b.0 as usize].params;
            blok.push(x.tag);
            blok.extend(x.muatan.iter().map(|m| m.1));
        }
        self.masuk_bayang.insert(ip, params.clone());
        for (asal, sasaran, potret) in tepi {
            let args = self.args_bayang(asal, &params, &potret).expect("tepi yang membentuk parameternya cocok");
            self.tempel_args(asal, sasaran, args);
        }
        self.bayang = params.into_iter().collect();
    }

    /// Argumen bayangan tepi `asal → sasaran` yang dicatat sebelum bloknya mulai, ditempel ke akhir `asal`.
    pub(super) fn tempel_args(&mut self, asal: Blok, sasaran: Blok, args: Vec<Nilai>) {
        let tambah = |ke: &Blok, xs: &mut Vec<Nilai>| if *ke == sasaran { xs.extend(args.iter().copied()) };
        match self.j.blok[asal.0 as usize].akhir.as_mut() {
            Some(Akhir::Lompat { ke, args }) => tambah(ke, args),
            Some(Akhir::Cabang { ya, ya_args, tidak, tidak_args, .. }) => { tambah(ya, ya_args); tambah(tidak, tidak_args); }
            Some(Akhir::CabangVm { tujuan, .. }) => for (ke, xs, _) in tujuan.iter_mut() { tambah(ke, xs) },
            _ => {}
        }
    }

    /// Bit tag `mungkin` yang memenuhi `pilih`, sebagai 0/1 dari `tag` (instruksinya ke `keluar`).
    pub(super) fn lipat_tag(&mut self, keluar: &mut Vec<Inst>, x: &Bayang, pilih: impl Fn(u32) -> bool) -> Nilai {
        let set: Vec<u32> = (0..64).filter(|t| x.mungkin >> t & 1 == 1 && pilih(*t)).collect();
        let semua = x.mungkin.count_ones() as usize;
        let konst = |s: &mut Self, keluar: &mut Vec<Inst>, n: i64| { let h = s.j.nilai_baru(Tipe::I); keluar.push(Inst::KonstI { hasil: h, nilai: n }); h };
        if set.is_empty() { return konst(self, keluar, 0); }
        if set.len() == semua { return konst(self, keluar, 1); }
        let mut acc: Option<Nilai> = None;
        for t in set {
            let k = konst(self, keluar, t as i64);
            let sama = self.j.nilai_baru(Tipe::I);
            keluar.push(Inst::Biner { hasil: sama, op: Biner::ICmp(Banding::Eq), a: x.tag, b: k, jaga: None });
            acc = Some(match acc {
                None => sama,
                Some(a) => { let h = self.j.nilai_baru(Tipe::I); keluar.push(Inst::Biner { hasil: h, op: Biner::IAtau, a, b: sama, jaga: None }); h }
            });
        }
        acc.expect("set tidak kosong")
    }

    /// Lengan `urutan` rantai `match` di alamat `situs` atas posisi berbayangan `x`: cocok tidaknya, kalau tiap
    /// varian yang mungkin terjawab tanpa galat saat kompilasi.
    pub(super) fn lipat_rantai(&mut self, keluar: &mut Vec<Inst>, x: &Bayang, situs: i64, urutan: usize) -> Option<Nilai> {
        let rantai = unsafe { &*(situs as *const crate::jit::lambat::RantaiPola) };
        let mut lengan = HashMap::new();
        for t in (0..64u32).filter(|t| x.mungkin >> t & 1 == 1) {
            let (nama, payload) = *self.varian.get(t as usize)?;
            lengan.insert(t, rantai.pilih_statis(nama, payload)?);
        }
        Some(self.lipat_tag(keluar, x, |t| lengan[&t] == urutan))
    }

    /// `pola_lokal` (situs `SitusPola` di alamat `situs`) atas posisi berbayangan `x`: 0/1, kalau tiap varian yang
    /// mungkin terjawab tanpa galat saat kompilasi.
    pub(super) fn lipat_pola(&mut self, keluar: &mut Vec<Inst>, x: &Bayang, situs: i64) -> Option<Nilai> {
        let situs = unsafe { &*(situs as *const crate::jit::lambat::SitusPola) };
        let mut cocok = 0u64;
        for t in (0..64u32).filter(|t| x.mungkin >> t & 1 == 1) {
            let (nama, payload) = *self.varian.get(t as usize)?;
            if situs.cocok_statis(nama, payload).ok()? { cocok |= 1 << t; }
        }
        Some(self.lipat_tag(keluar, x, |t| cocok >> t & 1 == 1))
    }

    /// `payload_angka_lokal` atas posisi berbayangan `x` dengan spekulasi `j`: (status 0/1 seperti pembantunya —
    /// 1 = bukan varian yang payload-nya isi ini, deopt — dan isinya).
    pub(super) fn lipat_payload(&mut self, keluar: &mut Vec<Inst>, x: &Bayang, j: Jenis) -> Option<(Nilai, Nilai)> {
        let (jm, v, sah) = *x.muatan.first()?;
        let satu: u64 = (0..self.varian.len().min(64)).filter(|t| self.varian[*t].1 == Some(1)).fold(0, |m, t| m | 1 << t);
        let baik = sah & satu & x.mungkin & self.slot_berjenis(0, j);
        if baik == 0 || !(jm == j || jm == Jenis::Nilai) { return None; }
        let st = self.lipat_tag(keluar, x, |t| baik >> t & 1 == 0);
        Some((st, self.dari_slot(keluar, jm, j, v)))
    }

    /// `PayloadJenisSama` (nama jenis di alamat `nama`) atau `PayloadPanjang` (`nama` = `None`) atas posisi berbayangan
    /// `x`: (gagal 0/1 — varian yang pembantunya menjawab VM, deopt — dan jawabannya).
    pub(super) fn lipat_payload_bentuk(&mut self, keluar: &mut Vec<Inst>, x: &Bayang, nama: Option<i64>) -> Option<(Nilai, Nilai)> {
        let nama = match nama {
            Some(a) => match unsafe { &*(a as *const VmValue) } { VmValue::Str(s) => Some(s.as_str().to_string()), _ => return None },
            None => None,
        };
        // Jawaban tiap varian yang mungkin: `None` = pembantunya menjawab VM (bukan payload yang terukur).
        let mut jawab: Vec<(u32, Option<i64>)> = Vec::new();
        for t in (0..64u32).filter(|t| x.mungkin >> t & 1 == 1) {
            let (_, payload) = *self.varian.get(t as usize)?;
            let jenis = match payload {
                None => None,
                // Jenis isi milik penandanya (tabel), juga untuk slot bit campur.
                Some(1) => match x.muatan.first() {
                    Some((_, _, sah)) if sah >> t & 1 == 1 => Some(if self.slot_berjenis(0, Jenis::Pecahan) >> t & 1 == 1 {
                        VmValue::Float(0.0).type_name() } else { VmValue::Int(0).type_name() }),
                    _ => return None,
                },
                Some(_) => Some(VmValue::tuple(Vec::new()).type_name()),
            };
            jawab.push((t, match (&nama, jenis, payload) {
                (_, None, _) => None,
                (Some(n), Some(j), _) => Some((n == j) as i64),
                (None, _, Some(n)) if n > 1 => Some(n as i64),
                (None, _, _) => None,
            }));
        }
        if jawab.iter().all(|(_, j)| j.is_none()) { return None; }
        let gagal = self.lipat_tag(keluar, x, |t| jawab.iter().any(|(u, j)| *u == t && j.is_none()));
        let mut v: Option<Nilai> = None;
        for (t, j) in jawab.iter().filter_map(|(t, j)| j.map(|j| (*t, j))) {
            let k = self.j.nilai_baru(Tipe::I);
            keluar.push(Inst::KonstI { hasil: k, nilai: j });
            v = Some(match v {
                None => k,
                Some(lain) => {
                    let sama = self.lipat_tag(keluar, x, |u| u == t);
                    let h = self.j.nilai_baru(Tipe::I);
                    keluar.push(Inst::Pilih { hasil: h, jika: sama, ya: k, tidak: lain });
                    h
                }
            });
        }
        Some((gagal, v.expect("ada jawaban")))
    }

    /// Kumpulan instruksi ke blok `b` lewat `dorong` (supaya jejak bayangan dan CSE ikut).
    pub(super) fn dorong_semua(&mut self, b: Blok, xs: Vec<Inst>) { for i in xs { self.dorong(b, i); } }
}

/// Jaga milik instruksi `i` (keadaan deopt-nya), untuk ditambah tuple maya (langkah 105).
pub(super) fn jaga_inst(i: &mut Inst) -> Vec<&mut super::Jaga> {
    match i {
        Inst::Biner { jaga, .. } | Inst::Unar { jaga, .. } => jaga.iter_mut().collect(),
        Inst::BinerL { jaga, jaga_muat, .. } => jaga.iter_mut().chain(jaga_muat.iter_mut()).collect(),
        Inst::Sempit { jaga, .. } | Inst::JagaJika { jaga, .. } | Inst::Buka { jaga, .. } | Inst::BukaLebar { jaga, .. }
        | Inst::MuatUpval { jaga, .. } | Inst::SimpanUpval { jaga, .. } | Inst::Sebar { jaga, .. }
        | Inst::Rantai { jaga, .. } | Inst::Rumus { jaga, .. } | Inst::CekMetode { jaga, .. }
        | Inst::IterAda { jaga, .. } | Inst::Maju { jaga, .. } | Inst::Panjang { jaga, .. } => vec![jaga],
        Inst::Pembantu { gagal, .. } => gagal.iter_mut().map(|(_, j)| j).collect(),
        Inst::Langsung { hasil, .. } => hasil.iter_mut().map(|(_, _, j)| j).collect(),
        _ => Vec::new(),
    }
}

/// Blok callee yang membutuhkan tuple maya `ke` dari `PayloadLokal` di blok `bi`: blok terjangkau yang didominasi
/// `bi` dan punya keadaan deopt dengan `ke` milik VM. `None` = lipatan tidak aman: `ke` dibaca atau ditulis di blok
/// terjangkau mana pun, atau hidup di keadaan blok terjangkau yang juga dicapai jalur lain.
pub(super) fn tuple_tak_bocor(c: &super::Fungsi, bi: usize, ke: usize) -> Option<Vec<usize>> {
    let n = c.blok.len();
    let penerus = |b: usize| -> Vec<usize> {
        match &c.blok[b].akhir {
            Some(Akhir::Lompat { ke, .. }) => vec![ke.0 as usize],
            Some(Akhir::Cabang { ya, tidak, .. }) => vec![ya.0 as usize, tidak.0 as usize],
            _ => Vec::new(),
        }
    };
    let mut pend = vec![Vec::new(); n];
    for b in 0..n { for p in penerus(b) { if p < n { pend[p].push(b); } } }
    // Terjangkau dari `bi`, lalu yang didominasi `bi`: semua pendahulunya didominasi (titik tetap, dari `bi`).
    let (mut capai, mut tumpuk) = (vec![false; n], penerus(bi));
    while let Some(b) = tumpuk.pop() {
        if b >= n || capai[b] { continue; }
        capai[b] = true;
        tumpuk.extend(penerus(b));
    }
    let mut dom = vec![false; n];
    dom[bi] = true;
    let mut ubah = true;
    while ubah {
        ubah = false;
        for b in 0..n {
            if !dom[b] && capai[b] && !pend[b].is_empty() && pend[b].iter().all(|p| dom[*p]) { dom[b] = true; ubah = true; }
        }
    }
    let hidup = |k: &super::Keadaan| k.isi.len() > ke && k.isi[ke].is_none();
    let mut butuh = Vec::new();
    for b in (0..n).filter(|b| capai[*b] && *b != bi) {
        let mut ada = false;
        for i in &c.blok[b].inst {
            if dibaca(i).is_none_or(|d| d.contains(&ke)) || ditulis(i).is_none_or(|d| d.contains(&ke)) { return None; }
            if jaga_inst(&mut i.clone()).iter().any(|j| hidup(&j.keadaan)) { ada = true; }
        }
        match &c.blok[b].akhir {
            Some(Akhir::Keluar { keadaan }) if hidup(keadaan) => ada = true,
            Some(Akhir::KembaliNilai { pos }) if *pos == ke => return None,
            _ => {}
        }
        if ada { if dom[b] { butuh.push(b); } else { return None; } }
    }
    Some(butuh)
}

impl Pembangun<'_> {
    /// Bit varian yang payload tuple-nya (dua isi atau lebih) seluruhnya ada di bayangan `x`.
    pub(super) fn tuple_baik(&self, x: &Bayang) -> u64 {
        let t = self.tabel.0.read().unwrap_or_else(|p| p.into_inner());
        (0..64).filter(|i| x.mungkin >> i & 1 == 1).filter(|&i| match t[i] {
            (_, Some(n), 0, _) if n >= 2 => x.muatan.len() >= n && x.muatan[..n].iter().all(|m| m.2 >> i & 1 == 1),
            _ => false,
        }).fold(0, |m, i| m | 1 << i)
    }

    /// Bit penanda yang slot `k`-nya berjenis `j` (`Bulat`/`Pecahan`), menurut tabel.
    pub(super) fn slot_berjenis(&self, k: usize, j: Jenis) -> u64 {
        let t = self.tabel.0.read().unwrap_or_else(|p| p.into_inner());
        t.iter().enumerate().take(64).filter(|(_, e)| (e.3 >> k & 1 == 1) == (j == Jenis::Pecahan)).fold(0, |m, (i, _)| m | 1 << i)
    }

    /// Isi slot berjenis `jm` (`Nilai` = bit campur) sebagai `j`.
    pub(super) fn dari_slot(&mut self, keluar: &mut Vec<Inst>, jm: Jenis, j: Jenis, v: Nilai) -> Nilai {
        if jm != Jenis::Nilai || j != Jenis::Pecahan { return v; }
        let h = self.j.nilai_baru(Tipe::F);
        keluar.push(Inst::Unar { hasil: h, op: super::Unar::DariBit, a: v, jaga: None });
        h
    }

    /// Jumlah isi payload varian `t` (tabel).
    pub(super) fn jumlah_payload(&self, t: u32) -> usize {
        self.tabel.0.read().unwrap_or_else(|p| p.into_inner()).get(t as usize).and_then(|e| e.1).unwrap_or(0)
    }
}
