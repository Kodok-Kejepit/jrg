// src/hash_cepat.rs — hasher cepat untuk peta yang kuncinya NAMA dari sumber program
// (variabel, field, method). Bukan untuk data pemakai: tanpa garam acak, jadi
// kunci dari luar (request HTTP, berkas) tetap memakai `RandomState`.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Pengali yang sama dengan FxHash milik rustc.
const K: u64 = 0x517c_c1b7_2722_0a95;

#[derive(Default, Clone, Copy)]
pub struct HasherCepat { h: u64 }

impl HasherCepat {
    #[inline]
    fn campur(&mut self, kata: u64) {
        self.h = (self.h.rotate_left(5) ^ kata).wrapping_mul(K);
    }
}

impl Hasher for HasherCepat {
    #[inline]
    fn write(&mut self, mut b: &[u8]) {
        while b.len() >= 8 {
            self.campur(u64::from_le_bytes(b[..8].try_into().unwrap()));
            b = &b[8..];
        }
        if b.len() >= 4 {
            self.campur(u32::from_le_bytes(b[..4].try_into().unwrap()) as u64);
            b = &b[4..];
        }
        for &x in b { self.campur(x as u64); }
    }
    #[inline] fn write_u8(&mut self, i: u8) { self.campur(i as u64); }
    #[inline] fn write_u32(&mut self, i: u32) { self.campur(i as u64); }
    #[inline] fn write_u64(&mut self, i: u64) { self.campur(i); }
    #[inline] fn write_usize(&mut self, i: usize) { self.campur(i as u64); }
    #[inline] fn finish(&self) -> u64 { self.h }
}

pub type BangunCepat = BuildHasherDefault<HasherCepat>;
pub type PetaCepat<K, V> = HashMap<K, V, BangunCepat>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn h<T: Hash>(t: &T) -> u64 { BangunCepat::default().hash_one(t) }

    #[test]
    fn deterministik_dan_membedakan_nama_yang_mirip() {
        assert_eq!(h(&"jumlah"), h(&"jumlah"));
        let nama = ["a", "b", "ab", "ba", "x1", "x2", "panjang_sekali_namanya", "panjang_sekali_namanyb", ""];
        let mut lihat = std::collections::HashSet::new();
        for n in nama { assert!(lihat.insert(h(&n)), "tabrakan untuk {:?}", n); }
    }

    #[test]
    fn peta_cepat_berperilaku_seperti_hashmap() {
        let mut m: PetaCepat<String, i32> = PetaCepat::default();
        for i in 0..1000 { m.insert(format!("v{}", i), i); }
        assert_eq!(m.len(), 1000);
        assert_eq!(m.get("v517"), Some(&517));
        assert_eq!(m.get("v1000"), None);
    }
}
