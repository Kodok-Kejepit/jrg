// src/interpreter/urut.rs — satu definisi pengurutan untuk dua mesin

use std::cmp::Ordering;

/// Urutkan STABIL dengan pembanding yang boleh GAGAL.
pub fn urut_stabil<T: Clone, E>(
    v: Vec<T>,
    mut banding: impl FnMut(&T, &T) -> Result<Ordering, E>,
) -> Result<Vec<T>, E> {
    let n = v.len();
    if n < 2 {
        return Ok(v);
    }
    let mut kini = v;
    let mut sisi: Vec<T> = Vec::with_capacity(n);
    let mut lebar = 1usize;
    while lebar < n {
        sisi.clear();
        let mut i = 0usize;
        while i < n {
            let tengah = (i + lebar).min(n);
            let akhir = (i + 2 * lebar).min(n);
            let (mut a, mut b) = (i, tengah);
            while a < tengah && b < akhir {
                // Kanan diambil HANYA kalau tegas lebih kecil: itu yang membuat
                // gabungnya stabil, dan itu pula yang membuat `sort_desc` stabil
                // waktu pembandingnya dibalik.
                if banding(&kini[b], &kini[a])? == Ordering::Less {
                    sisi.push(kini[b].clone());
                    b += 1;
                } else {
                    sisi.push(kini[a].clone());
                    a += 1;
                }
            }
            sisi.extend_from_slice(&kini[a..tengah]);
            sisi.extend_from_slice(&kini[b..akhir]);
            i = akhir;
        }
        std::mem::swap(&mut kini, &mut sisi);
        lebar *= 2;
    }
    Ok(kini)
}

/// Urutkan dengan pembanding buatan pemakai (`|a, b| ...` yang menjawab
/// angka).
pub fn urut_sisip_biner<T, E>(
    v: Vec<T>,
    mut banding: impl FnMut(&T, &T) -> Result<i128, E>,
) -> Result<Vec<T>, E> {
    let mut keluar: Vec<T> = Vec::with_capacity(v.len());
    for item in v {
        let (mut lo, mut hi) = (0usize, keluar.len());
        while lo < hi {
            let tengah = (lo + hi) / 2;
            if banding(&item, &keluar[tengah])? < 0 {
                hi = tengah;
            } else {
                lo = tengah + 1;
            }
        }
        keluar.insert(lo, item);
    }
    Ok(keluar)
}

#[cfg(test)]
mod tes {
    use super::*;

    fn naik(a: &i32, b: &i32) -> Result<Ordering, ()> { Ok(a.cmp(b)) }

    #[test]
    fn stabil_untuk_yang_dianggap_sama() {
        // Kunci sama, isi beda: urutan asalnya harus bertahan.
        let v = vec![(1, 'a'), (0, 'b'), (1, 'c'), (0, 'd'), (1, 'e')];
        let hasil = urut_stabil(v, |x, y| Ok::<_, ()>(x.0.cmp(&y.0))).unwrap();
        assert_eq!(hasil, vec![(0, 'b'), (0, 'd'), (1, 'a'), (1, 'c'), (1, 'e')]);
    }

    #[test]
    fn stabil_juga_waktu_pembandingnya_dibalik() {
        // `sort_desc` membalik pembandingnya, bukan membalik hasilnya — kalau
        // ia membalik hasilnya, yang dianggap sama ikut terbalik urutannya.
        let v = vec![(1, 'a'), (0, 'b'), (1, 'c'), (0, 'd')];
        let hasil = urut_stabil(v, |x, y| Ok::<_, ()>(x.0.cmp(&y.0).reverse())).unwrap();
        assert_eq!(hasil, vec![(1, 'a'), (1, 'c'), (0, 'b'), (0, 'd')]);
    }

    #[test]
    fn berhenti_di_pasangan_pertama_yang_gagal() {
        let v = vec![3, 1, 2, 9];
        let hasil: Result<Vec<i32>, &str> = urut_stabil(v, |a, b| {
            if *a == 9 || *b == 9 { Err("tak terbanding") } else { Ok(a.cmp(b)) }
        });
        assert_eq!(hasil, Err("tak terbanding"));
    }

    #[test]
    fn panjang_ganjil_dan_genap_tetap_terurut() {
        for n in 0..40usize {
            let v: Vec<i32> = (0..n as i32).rev().collect();
            let hasil = urut_stabil(v, naik).unwrap();
            assert_eq!(hasil, (0..n as i32).collect::<Vec<_>>(), "n={}", n);
        }
    }

    #[test]
    fn sisip_biner_stabil_dan_urutan_panggilannya_tetap() {
        let mut jejak = Vec::new();
        let hasil = urut_sisip_biner(vec![2, 1, 3], |a, b| {
            jejak.push((*a, *b));
            Ok::<_, ()>((*a as i128) - (*b as i128))
        }).unwrap();
        assert_eq!(hasil, vec![1, 2, 3]);
        // Dikunci apa adanya, dan diadu dengan interpreter yang sudah jalan:
        // `a.sort_by(...)` atas `[2, 1, 3]` memang memanggil dua kali.
        assert_eq!(jejak, vec![(1, 2), (3, 2)]);
    }
}
