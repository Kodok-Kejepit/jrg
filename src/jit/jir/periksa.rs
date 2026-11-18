//! Pemeriksa JIR: tiap nilai didefinisikan sekali, tiap pemakaian didahului definisinya di bloknya atau di blok yang
//! mendominasinya (callee yang disisipkan memakai nilai pemanggil), tipe operand cocok, lompatan membawa argumen
//! sebanyak dan setipe parameter tujuannya, dan tiap blok punya akhir.

use std::collections::HashSet;

use super::{Akhir, Biner, Blok, Fungsi, Inst, Jaga, Keadaan, Maya, Nilai, OpL, Tipe, Unar};

/// Nilai yang dibaca keadaan deopt `k` dan induknya: isi native dan bayangan varian yang basi.
fn nilai_keadaan(k: &Keadaan) -> Vec<Nilai> {
    let mut v = Vec::new();
    let mut x = Some(k);
    while let Some(k) = x {
        v.extend(k.isi.iter().flatten().copied());
        for m in &k.maya { v.extend(nilai_maya(m)); }
        x = k.induk.as_deref();
    }
    v
}

fn nilai_maya(m: &Maya) -> Vec<Nilai> { std::iter::once(m.tag).chain(m.muatan.iter().copied()).collect() }

/// Penerus blok `b` dari akhirnya.
fn penerus(f: &Fungsi, b: usize) -> Vec<usize> {
    match &f.blok[b].akhir {
        Some(Akhir::Lompat { ke, .. }) => vec![ke.0 as usize],
        Some(Akhir::Cabang { ya, tidak, .. }) => vec![ya.0 as usize, tidak.0 as usize],
        Some(Akhir::CabangVm { tujuan, .. }) => tujuan.iter().map(|t| t.0 .0 as usize).collect(),
        _ => Vec::new(),
    }
}

/// Urutan pasca-urut terbalik dari blok masuk dan dominator langsung tiap blok terjangkau (Cooper–Harvey–Kennedy).
fn dominator(f: &Fungsi) -> (Vec<usize>, Vec<Option<usize>>) {
    let n = f.blok.len();
    let (mut urut, mut lihat) = (Vec::new(), vec![false; n]);
    let mut tumpuk = vec![(f.masuk.0 as usize, 0usize)];
    lihat[f.masuk.0 as usize] = true;
    while let Some((b, i)) = tumpuk.pop() {
        let p = penerus(f, b);
        if i < p.len() {
            tumpuk.push((b, i + 1));
            if !lihat[p[i]] { lihat[p[i]] = true; tumpuk.push((p[i], 0)); }
        } else { urut.push(b); }
    }
    urut.reverse();
    let mut nomor = vec![usize::MAX; n];
    for (i, b) in urut.iter().enumerate() { nomor[*b] = i; }
    let mut pendahulu = vec![Vec::new(); n];
    for &b in &urut { for c in penerus(f, b) { pendahulu[c].push(b); } }
    let mut idom: Vec<Option<usize>> = vec![None; n];
    let masuk = f.masuk.0 as usize;
    idom[masuk] = Some(masuk);
    let temu = |idom: &[Option<usize>], mut a: usize, mut b: usize| {
        while a != b {
            while nomor[a] > nomor[b] { a = idom[a].expect("diproses"); }
            while nomor[b] > nomor[a] { b = idom[b].expect("diproses"); }
        }
        a
    };
    let mut berubah = true;
    while berubah {
        berubah = false;
        for &b in urut.iter().skip(1) {
            let mut baru: Option<usize> = None;
            for &p in &pendahulu[b] {
                if idom[p].is_none() { continue; }
                baru = Some(match baru { None => p, Some(x) => temu(&idom, p, x) });
            }
            if baru.is_some() && idom[b] != baru { idom[b] = baru; berubah = true; }
        }
    }
    (urut, idom)
}

pub fn periksa(f: &Fungsi) -> Result<(), String> {
    let mut sekali: HashSet<Nilai> = HashSet::new();
    let (urut, idom) = dominator(f);
    let mut milik: Vec<HashSet<Nilai>> = vec![HashSet::new(); f.blok.len()];
    let mut sisa: Vec<usize> = (0..f.blok.len()).filter(|b| !urut.contains(b)).collect();
    let mut urutan = urut.clone();
    urutan.append(&mut sisa);
    for bi in urutan {
        let b = &f.blok[bi];
        // Nilai yang sudah ada: definisi blok-blok yang mendominasi blok ini (yang tak terjangkau: blok sendiri).
        let mut ada: HashSet<Nilai> = HashSet::new();
        if urut.contains(&bi) {
            let mut d = bi;
            while let Some(i) = idom[d].filter(|i| *i != d) { ada.extend(milik[i].iter().copied()); d = i; }
        }
        let awal: HashSet<Nilai> = ada.clone();
        let mut definisi = |v: Nilai, ada: &mut HashSet<Nilai>| -> Result<(), String> {
            if v.0 as usize >= f.tipe.len() { return Err(format!("b{bi}: {v} tanpa tipe")); }
            if !sekali.insert(v) { return Err(format!("b{bi}: {v} didefinisikan dua kali")); }
            ada.insert(v);
            Ok(())
        };
        for p in &b.params { definisi(*p, &mut ada)?; }
        let pakai = |v: Nilai, ada: &HashSet<Nilai>| -> Result<(), String> {
            if ada.contains(&v) { Ok(()) } else { Err(format!("b{bi}: {v} dipakai sebelum didefinisikan di bloknya")) }
        };
        let tipe = |v: Nilai, t: Tipe| -> Result<(), String> {
            if f.tipe(v) == t { Ok(()) } else { Err(format!("b{bi}: {v} bertipe {:?}, diharapkan {t:?}", f.tipe(v))) }
        };
        // Pembantu dan stack VM cuma menerima kata 64 bit; `Lebar` lewat `TulisLebar`/tumpahan.
        let bukan_l = |v: Nilai| -> Result<(), String> {
            if f.tipe(v) == Tipe::L { Err(format!("b{bi}: {v} bertipe 128 bit di tempat 64 bit")) } else { Ok(()) }
        };
        let jaga = |j: &Jaga, ada: &HashSet<Nilai>| -> Result<(), String> {
            for v in nilai_keadaan(&j.keadaan) { pakai(v, ada)?; }
            Ok(())
        };
        for ins in &b.inst {
            match ins {
                Inst::KonstI { hasil, .. } => { definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::KonstF { hasil, .. } => { definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::F)?; }
                Inst::Biner { hasil, op, a, b: y, jaga: j } => {
                    pakai(*a, &ada)?;
                    pakai(*y, &ada)?;
                    if let Some(j) = j { jaga(j, &ada)?; }
                    let (masuk, keluar) = match op {
                        Biner::FTambah | Biner::FKurang | Biner::FKali | Biner::FBagi => (Tipe::F, Tipe::F),
                        Biner::FCmp(_) => (Tipe::F, Tipe::I),
                        _ => (Tipe::I, Tipe::I),
                    };
                    tipe(*a, masuk)?;
                    tipe(*y, masuk)?;
                    definisi(*hasil, &mut ada)?;
                    tipe(*hasil, keluar)?;
                }
                Inst::Unar { hasil, op, a, jaga: j } => {
                    pakai(*a, &ada)?;
                    if let Some(j) = j { jaga(j, &ada)?; }
                    let (masuk, keluar) = match op {
                        Unar::FNeg => (Tipe::F, Tipe::F),
                        Unar::KeF | Unar::DariBit => (Tipe::I, Tipe::F),
                        Unar::BitF => (Tipe::F, Tipe::I),
                        Unar::KeL | Unar::KeLU => (Tipe::I, Tipe::L),
                        Unar::BukanBitL => (Tipe::L, Tipe::L),
                        Unar::KeBoolL | Unar::Rendah => (Tipe::L, Tipe::I),
                        _ => (Tipe::I, Tipe::I),
                    };
                    tipe(*a, masuk)?;
                    definisi(*hasil, &mut ada)?;
                    tipe(*hasil, keluar)?;
                }
                Inst::Pilih { hasil, jika, ya, tidak } => {
                    for v in [jika, ya, tidak] { pakai(*v, &ada)?; }
                    tipe(*jika, Tipe::I)?;
                    if f.tipe(*ya) != f.tipe(*tidak) { return Err(format!("b{bi}: pilih atas dua tipe")); }
                    definisi(*hasil, &mut ada)?;
                    tipe(*hasil, f.tipe(*ya))?;
                }
                Inst::JagaJika { kondisi, jaga: j } => { pakai(*kondisi, &ada)?; tipe(*kondisi, Tipe::I)?; jaga(j, &ada)?; }
                Inst::Undi { hasil } => { definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::Muat { .. } | Inst::Buang { .. } | Inst::Taruh { .. } => {}
                Inst::Simpan { nilai, .. } => if let Some(v) = nilai { pakai(*v, &ada)?; bukan_l(*v)?; },
                Inst::Benar { hasil, .. } => { definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::Langkah { isi, .. } => for (_, v) in isi { pakai(*v, &ada)?; },
                Inst::Buka { hasil, jaga: j, .. } => { jaga(j, &ada)?; definisi(*hasil, &mut ada)?; }
                Inst::Bawaan { isi, .. } => for (_, v) in isi { pakai(*v, &ada)?; },
                Inst::BinerVm { hasil, isi, .. } => {
                    for (_, v) in isi { pakai(*v, &ada)?; }
                    if let Some(h) = hasil { definisi(*h, &mut ada)?; tipe(*h, Tipe::I)?; }
                }
                Inst::MuatUpval { jaga: j, .. } => jaga(j, &ada)?,
                Inst::TeksBulat { n, .. } => { pakai(*n, &ada)?; tipe(*n, Tipe::I)?; }
                Inst::Pindah { .. } => {}
                Inst::Bangun { isi, .. } => for (_, v) in isi { pakai(*v, &ada)?; },
                Inst::Salin { .. } | Inst::Tukar { .. } => {}
                Inst::CekMetode { jaga: j, .. } | Inst::Maju { jaga: j, .. } => jaga(j, &ada)?,
                Inst::KonstL { hasil, .. } => { definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::L)?; }
                Inst::BinerL { hasil, op, a, b: y, jaga: j, jaga_muat } => {
                    pakai(*a, &ada)?;
                    pakai(*y, &ada)?;
                    for g in [j, jaga_muat].into_iter().flatten() { jaga(g, &ada)?; }
                    tipe(*a, Tipe::L)?;
                    tipe(*y, if matches!(op, OpL::GeserKiri | OpL::GeserKanan) { Tipe::I } else { Tipe::L })?;
                    definisi(*hasil, &mut ada)?;
                    tipe(*hasil, if matches!(op, OpL::Cmp(_) | OpL::Bagi | OpL::Sisa) { Tipe::I } else { Tipe::L })?;
                }
                Inst::Sempit { hasil, a, jaga: j } => { pakai(*a, &ada)?; tipe(*a, Tipe::L)?; jaga(j, &ada)?; definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::Sebar { jaga: j, .. } => jaga(j, &ada)?,
                Inst::Rumus { args, hasil, jaga: j, .. } => {
                    for v in args.iter().flatten() { pakai(*v, &ada)?; tipe(*v, Tipe::F)?; }
                    jaga(j, &ada)?;
                    if let Some(h) = hasil { definisi(*h, &mut ada)?; tipe(*h, Tipe::F)?; }
                }
                Inst::Rantai { hasil, jaga: j, .. } => { jaga(j, &ada)?; definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::TulisLebar { nilai, .. } => { pakai(*nilai, &ada)?; tipe(*nilai, Tipe::L)?; }
                Inst::BukaLebar { hasil, jaga: j, .. } => { jaga(j, &ada)?; definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::L)?; }
                Inst::Pembantu { args, hasil, gagal, keluar, .. } => {
                    for a in args { if let super::Arg::Isi(v) = a { pakai(*v, &ada)?; bukan_l(*v)?; } }
                    if let Some((_, j)) = gagal { jaga(j, &ada)?; }
                    if let Some(h) = hasil { definisi(*h, &mut ada)?; tipe(*h, Tipe::I)?; }
                    if let Some(k) = keluar { definisi(*k, &mut ada)?; }
                }
                Inst::Panjang { hasil, jaga: j, .. } => { jaga(j, &ada)?; definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::Langsung { isi, args, basi, hasil, .. } => {
                    for (_, v) in isi.iter().chain(args) { pakai(*v, &ada)?; }
                    for (_, v) in args { bukan_l(*v)?; }
                    for v in nilai_keadaan(basi) { pakai(v, &ada)?; }
                    if let Some((h, _, j)) = hasil { jaga(j, &ada)?; definisi(*h, &mut ada)?; }
                }
                Inst::Wujud(m) => for v in nilai_maya(m) { pakai(v, &ada)?; bukan_l(v)?; },
                Inst::IterAda { hasil, jaga: j, .. } => { jaga(j, &ada)?; definisi(*hasil, &mut ada)?; tipe(*hasil, Tipe::I)?; }
                Inst::TulisVm { nilai, .. } => if let Some(v) = nilai { pakai(*v, &ada)?; bukan_l(*v)?; },
                Inst::SimpanUpval { nilai, jaga: j, .. } => { if let Some(v) = nilai { pakai(*v, &ada)?; bukan_l(*v)?; } jaga(j, &ada)?; }
                Inst::Panggil { hasil, args, di, sesudah, .. } => {
                    for a in args { pakai(*a, &ada)?; tipe(*a, Tipe::I)?; }
                    for v in nilai_keadaan(di).into_iter().chain(nilai_keadaan(sesudah)) { pakai(v, &ada)?; }
                    definisi(*hasil, &mut ada)?;
                    tipe(*hasil, Tipe::I)?;
                }
            }
        }
        let lompat = |ke: Blok, args: &[Nilai], ada: &HashSet<Nilai>| -> Result<(), String> {
            let t = f.blok.get(ke.0 as usize).ok_or_else(|| format!("b{bi}: lompat ke {ke} yang tidak ada"))?;
            if t.params.len() != args.len() {
                return Err(format!("b{bi}: {ke} butuh {} argumen, dapat {}", t.params.len(), args.len()));
            }
            for (a, p) in args.iter().zip(&t.params) {
                pakai(*a, ada)?;
                if f.tipe(*a) != f.tipe(*p) { return Err(format!("b{bi}: argumen {a} ke {ke} beda tipe")); }
            }
            if ke == f.masuk { return Err(format!("b{bi}: lompat ke blok masuk")); }
            Ok(())
        };
        match &b.akhir {
            Some(Akhir::Lompat { ke, args }) => lompat(*ke, args, &ada)?,
            Some(Akhir::Cabang { kondisi, ya, ya_args, tidak, tidak_args }) => {
                pakai(*kondisi, &ada)?;
                tipe(*kondisi, Tipe::I)?;
                lompat(*ya, ya_args, &ada)?;
                lompat(*tidak, tidak_args, &ada)?;
            }
            Some(Akhir::Kembali { nilai, .. }) => pakai(*nilai, &ada)?,
            Some(Akhir::KembaliNilai { .. } | Akhir::KembaliUnit) => {}
            Some(Akhir::CabangVm { isi, tujuan, .. }) => {
                for (_, v) in isi { pakai(*v, &ada)?; }
                for (ke, args, _) in tujuan { lompat(*ke, args, &ada)?; }
            }
            Some(Akhir::Keluar { keadaan }) => {
                for v in nilai_keadaan(keadaan) { pakai(v, &ada)?; }
            }
            None => return Err(format!("b{bi} tanpa akhir")),
        }
        milik[bi] = ada.difference(&awal).copied().collect();
    }
    Ok(())
}
