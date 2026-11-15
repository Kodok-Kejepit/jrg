//! Tata letak `VmValue`, `Vec` dan `ArcInner` di biner ini, ditemukan saat jalan dan diperiksa sendiri, supaya
//! kode JIT bisa membaca elemen larik tanpa memanggil pembantu. Rancangan: `dokumentasi/jit/j8-rencana.md`.
//! `None` = tata letak tidak bisa dipastikan; kode JIT lalu cuma memakai pembantu.

use std::sync::{Arc, Mutex, OnceLock};

use crate::vm::value::VmValue;

/// Posisi dalam byte. Tag dibaca selebar satu byte di `tag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TataLetak {
    pub ukuran: usize,
    pub tag: usize,
    pub tag_nil: u8,
    pub tag_bool: u8,
    pub tag_int: u8,
    pub tag_float: u8,
    pub tag_array: u8,
    pub isi_bool: usize,
    pub isi_int: usize,
    pub isi_float: usize,
    pub isi_array: usize,
    /// Dari awal `ArcInner`: hitungan kuat, hitungan lemah (mentah: 1 = tanpa `Weak`), isi.
    pub arc_kuat: usize,
    pub arc_lemah: usize,
    pub arc_isi: usize,
    /// `Vec` di dalam `Mutex<Vec<VmValue>>`.
    pub mutex_isi: usize,
    pub vec_ptr: usize,
    pub vec_len: usize,
    pub vm_stack: usize,
}

static TATA: OnceLock<Option<TataLetak>> = OnceLock::new();

/// Tata letak yang ditemukan dan lolos pemeriksaan, sekali per proses.
pub fn tata_letak() -> Option<&'static TataLetak> {
    TATA.get_or_init(temukan).as_ref()
}

/// Byte sebuah nilai. Nilainya ditulis ke penyangga yang sudah dinolkan, jadi celah yang tidak ditulis
/// konstruktornya terbaca nol, bukan isi lama memori.
fn byte<T>(v: T) -> Vec<u8> {
    let mut b = std::mem::MaybeUninit::<T>::zeroed();
    unsafe {
        b.as_mut_ptr().write(v);
        let p = b.as_ptr() as *const u8;
        let hasil = (0..std::mem::size_of::<T>()).map(|i| std::ptr::read_volatile(p.add(i))).collect();
        b.assume_init_drop();
        hasil
    }
}

fn kata(b: &[u8], o: usize) -> usize {
    usize::from_le_bytes(b[o..o + 8].try_into().expect("delapan byte"))
}

/// Posisi `o` (kelipatan `langkah`) tempat `pola` muncul di semua contoh pasangan (byte, pola).
fn posisi(contoh: &[(Vec<u8>, Vec<u8>)], langkah: usize) -> Option<usize> {
    let n = contoh.first()?.0.len();
    let mut cocok = (0..n).step_by(langkah).filter(|&o| contoh.iter().all(|(b, p)| b.get(o..o + p.len()) == Some(&p[..])));
    let o = cocok.next()?;
    // Dua posisi yang sama-sama cocok = ambigu.
    if cocok.next().is_some() { return None; }
    Some(o)
}

fn temukan() -> Option<TataLetak> {
    if cfg!(target_endian = "big") || std::mem::size_of::<usize>() != 8 { return None; }
    let ukuran = std::mem::size_of::<VmValue>();

    // Contoh per jenis: tiap posisi byte isinya divariasikan penuh, supaya tag yang menumpang di celah isi
    // varian lain (niche) ketahuan.
    let mut lain: Vec<Vec<u8>> = Vec::new();
    for v in [VmValue::Unit] { lain.push(byte(v)); }
    for b in 0..8u32 {
        for x in 0..=255usize { lain.push(byte(VmValue::Ptr(x << (8 * b)))); }
    }
    lain.push(byte(VmValue::Ptr(usize::MAX)));
    for b in 0..3u32 {
        for x in 0..=255u32 {
            for atas in [0u32, 0x10] {
                if let Some(c) = char::from_u32((x << (8 * b)) | (atas << 16)) { lain.push(byte(VmValue::Rune(c))); }
            }
        }
    }
    let teks: Vec<VmValue> = (0..8).map(|i| VmValue::str(format!("t{i}"))).collect();
    for t in &teks { lain.push(byte(t.clone())); }

    let nil = vec![byte(VmValue::Nil), byte(VmValue::Nil)];
    let boolean = [byte(VmValue::Bool(false)), byte(VmValue::Bool(true))];
    let bulat_isi: Vec<i128> = vec![0, 1, -1, 0x0123_4567_89ab_cdef, i128::MIN, i128::MAX, 1 << 64, -(1 << 70), 255, 256];
    let bulat: Vec<Vec<u8>> = bulat_isi.iter().map(|&n| byte(VmValue::Int(n))).collect();
    let pecahan_isi = [0.0f64, -0.0, 1.5, f64::NAN, f64::INFINITY, f64::from_bits(0xFFFF_FFFF_FFFF_FFFF), f64::from_bits(1)];
    let pecahan: Vec<Vec<u8>> = pecahan_isi.iter().map(|&x| byte(VmValue::Float(x))).collect();
    let larik_nilai: Vec<VmValue> = (0..6).map(|i| VmValue::array(vec![VmValue::Int(i); i as usize])).collect();
    let larik: Vec<Vec<u8>> = larik_nilai.iter().map(|v| byte(v.clone())).collect();

    // Tag: satu byte yang tetap dalam tiap jenis, beda antar-jenis, dan tidak pernah muncul di jenis lain mana pun.
    let kelompok = [&nil[..], &boolean[..], &bulat[..], &pecahan[..], &larik[..]];
    let tag = (0..ukuran).find(|&p| {
        let nilai: Vec<Option<u8>> = kelompok.iter().map(|k| {
            let b = k[0][p];
            k.iter().all(|x| x[p] == b).then_some(b)
        }).collect();
        let Some(nilai) = nilai.into_iter().collect::<Option<Vec<u8>>>() else { return false };
        let unik = (0..nilai.len()).all(|i| !nilai[..i].contains(&nilai[i]));
        unik && lain.iter().all(|b| !nilai.contains(&b[p]))
    })?;
    let [tag_nil, tag_bool, tag_int, tag_float, tag_array] = [nil[0][tag], boolean[0][tag], bulat[0][tag], pecahan[0][tag], larik[0][tag]];

    let isi_int = posisi(&bulat.iter().zip(&bulat_isi).map(|(b, n)| (b.clone(), n.to_le_bytes().to_vec())).collect::<Vec<_>>(), 8)?;
    let isi_float = posisi(&pecahan.iter().zip(&pecahan_isi).map(|(b, x)| (b.clone(), x.to_bits().to_le_bytes().to_vec())).collect::<Vec<_>>(), 8)?;
    let isi_bool = posisi(&[(boolean[0].clone(), vec![0u8]), (boolean[1].clone(), vec![1u8])], 1)?;
    if [isi_int..isi_int + 16, isi_float..isi_float + 8, isi_bool..isi_bool + 1].iter().any(|r| r.contains(&tag)) { return None; }

    // `Arc`: kata di isi `Array` yang menunjuk awal `ArcInner`; hitungan dan isinya dari `Arc::as_ptr`.
    let arc_contoh: Vec<Arc<Mutex<Vec<VmValue>>>> = larik_nilai.iter().map(|v| match v {
        VmValue::Array(a) => Arc::clone(a),
        _ => unreachable!("dibangun sebagai larik"),
    }).collect();
    let isi_array = (0..ukuran - 7).step_by(8).find(|&o| {
        o != tag && larik.iter().zip(&arc_contoh).all(|(b, a)| {
            let w = kata(b, o);
            let isi = Arc::as_ptr(a) as usize;
            w != 0 && isi > w && isi - w <= 64
        })
    })?;
    let arc_isi = Arc::as_ptr(&arc_contoh[0]) as usize - kata(&larik[0], isi_array);

    // Hitungan: di awal `ArcInner` (`repr(C)` di std), diperiksa dengan mengubahnya.
    let a = Arc::clone(&arc_contoh[1]);
    let inner = Arc::as_ptr(&a) as usize - arc_isi;
    let baca = |o: usize| unsafe { std::ptr::read_volatile((inner + o) as *const usize) };
    let kuat0 = Arc::strong_count(&a);
    let lemah0 = Arc::weak_count(&a);
    let salinan = Arc::clone(&a);
    let lemah = Arc::downgrade(&a);
    let (arc_kuat, arc_lemah) = (0..arc_isi).step_by(8).fold((None, None), |(k, l), o| {
        let v = baca(o);
        (if v == kuat0 + 1 { k.or(Some(o)) } else { k }, if v == lemah0 + 2 { l.or(Some(o)) } else { l })
    });
    drop((salinan, lemah));
    let (arc_kuat, arc_lemah) = (arc_kuat?, arc_lemah?);
    if arc_kuat == arc_lemah || baca(arc_kuat) != kuat0 || baca(arc_lemah) != lemah0 + 1 { return None; }

    // `Vec` di dalam `Mutex`: alamat isi dari pinjamannya; penunjuk dan panjang dari `Vec` yang dikenal.
    let m: &Mutex<Vec<VmValue>> = &arc_contoh[3];
    let mutex_isi = {
        let g = m.lock().ok()?;
        &*g as *const Vec<VmValue> as usize - m as *const _ as usize
    };
    let mut v: Vec<VmValue> = Vec::with_capacity(7);
    v.extend((0..3).map(VmValue::Int));
    let (vp, vl) = (v.as_ptr() as usize, v.len());
    let vb = byte_vec(&v);
    let vec_ptr = (0..vb.len()).step_by(8).find(|&o| kata(&vb, o) == vp)?;
    let vec_len = (0..vb.len()).step_by(8).find(|&o| kata(&vb, o) == vl && o != vec_ptr)?;
    if (0..vb.len()).step_by(8).filter(|&o| kata(&vb, o) == vl).count() != 1 { return None; }

    let t = TataLetak {
        ukuran, tag, tag_nil, tag_bool, tag_int, tag_float, tag_array, isi_bool, isi_int, isi_float, isi_array,
        arc_kuat, arc_lemah, arc_isi, mutex_isi, vec_ptr, vec_len,
        vm_stack: std::mem::offset_of!(crate::vm::Vm, stack),
    };
    periksa(&t).then_some(t)
}

/// Byte sebuah `Vec` (tiga kata), dibaca di tempatnya.
fn byte_vec(v: &Vec<VmValue>) -> Vec<u8> {
    let p = v as *const Vec<VmValue> as *const u8;
    (0..std::mem::size_of::<Vec<VmValue>>()).map(|i| unsafe { std::ptr::read_volatile(p.add(i)) }).collect()
}

/// Bacaan lewat tata letak yang ditemukan, seperti yang akan dilakukan kode JIT. `None` = bukan larik milik
/// tunggal berisi bulat 64 bit di indeks itu.
pub fn baca_bulat(t: &TataLetak, v: &VmValue, i: usize) -> Option<i64> {
    let p = v as *const VmValue as usize;
    unsafe {
        if *((p + t.tag) as *const u8) != t.tag_array { return None; }
        let inner = *((p + t.isi_array) as *const usize);
        if *((inner + t.arc_kuat) as *const usize) != 1 || *((inner + t.arc_lemah) as *const usize) != 1 { return None; }
        let vec = inner + t.arc_isi + t.mutex_isi;
        let (data, len) = (*((vec + t.vec_ptr) as *const usize), *((vec + t.vec_len) as *const usize));
        if i >= len { return None; }
        let el = data + i * t.ukuran;
        if *((el + t.tag) as *const u8) != t.tag_int { return None; }
        let (lo, hi) = (*((el + t.isi_int) as *const i64), *((el + t.isi_int + 8) as *const i64));
        (hi == lo >> 63).then_some(lo)
    }
}

/// Pemeriksaan diri: nilai-nilai baru dibaca balik lewat posisi yang ditemukan.
fn periksa(t: &TataLetak) -> bool {
    let tunggal = VmValue::array((0..5).map(|i| VmValue::Int(i * 1_000_000_007 - 3)).collect());
    let bersama = VmValue::array(vec![VmValue::Int(9)]);
    let _pemegang = bersama.clone();
    let campur = VmValue::array(vec![VmValue::Int(1 << 80), VmValue::Float(2.5), VmValue::Int(-7), VmValue::str("x")]);
    let kosong = VmValue::array(Vec::new());
    let ok = (0..5).all(|i| baca_bulat(t, &tunggal, i) == Some(i as i64 * 1_000_000_007 - 3))
        && baca_bulat(t, &tunggal, 5).is_none()
        && baca_bulat(t, &bersama, 0).is_none()
        && baca_bulat(t, &campur, 0).is_none()
        && baca_bulat(t, &campur, 1).is_none()
        && baca_bulat(t, &campur, 2) == Some(-7)
        && baca_bulat(t, &campur, 3).is_none()
        && baca_bulat(t, &kosong, 0).is_none()
        && baca_bulat(t, &VmValue::Int(3), 0).is_none()
        && baca_bulat(t, &VmValue::str("abc"), 0).is_none();
    let skalar = byte(VmValue::Float(-1.25)).get(t.isi_float..t.isi_float + 8) == Some(&(-1.25f64).to_bits().to_le_bytes()[..])
        && byte(VmValue::Bool(true))[t.isi_bool] == 1
        && byte(VmValue::Nil)[t.tag] == t.tag_nil;
    ok && skalar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tata_letak_ditemukan_dan_benar() {
        let t = tata_letak().expect("tata letak VmValue harus ditemukan di x86_64/aarch64");
        assert!(periksa(t));
        eprintln!("tata letak: {t:?}");
        // Stack VM: slot di `vm.stack` terbaca lewat `vm_stack` + `vec_ptr`.
        let mut vm = crate::vm::Vm::new();
        vm.stack.push(VmValue::array(vec![VmValue::Int(41), VmValue::Int(42)]));
        let p = &vm as *const crate::vm::Vm as usize + t.vm_stack;
        let data = unsafe { *((p + t.vec_ptr) as *const usize) };
        let slot = unsafe { &*(data as *const VmValue) };
        assert_eq!(baca_bulat(t, slot, 1), Some(42));
    }
}
