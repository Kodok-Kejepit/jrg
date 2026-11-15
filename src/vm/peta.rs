//! Isi sebuah `Peta`: tabel hash (dict, struct besar) atau bentuk rapat (struct kecil: nama field bersama +
//! larik nilai, jadi field bisa dibaca lewat indeks). Rancangannya di `dokumentasi/jit/j7-rencana.md`.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use super::value::{HashPeta, Kunci, VmValue};

/// Struct sampai sekian field disimpan rapat; cari namanya linear, lebih murah dari hash untuk segini.
pub const RAPAT_MAKS: usize = 16;
/// Tabel ber-hash cepat (bukan dict) yang melewati ini pindah ke SipHash berkunci acak.
pub const KUNCI_CEPAT: usize = 64;

/// Nama field berurutan, dibagi semua struct yang dibuat dari literal yang sama.
#[derive(Debug)]
pub struct Bentuk {
    pub nama: Box<[Kunci]>,
    /// Tanpa nama kembar: literal `P { a: 1, a: 2 }` dibangun lewat jalur tabel.
    pub unik: bool,
}

impl Bentuk {
    pub fn baru(nama: Vec<Kunci>) -> Arc<Bentuk> {
        let unik = Self::unik(&nama);
        Arc::new(Bentuk { nama: nama.into_boxed_slice(), unik })
    }

    /// Bentuk prototipe literal, diintern: literal berbeda dengan nama field berurutan sama berbagi satu
    /// bentuk (tembolok per situs di JIT tidak meleset antar-literal). Cuma saat kompilasi, jadi terbatas.
    pub fn intern(nama: Vec<Kunci>) -> Arc<Bentuk> {
        static TABEL: std::sync::OnceLock<std::sync::Mutex<HashMap<Vec<Kunci>, Arc<Bentuk>>>> = std::sync::OnceLock::new();
        let mut t = TABEL.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
        t.entry(nama.clone()).or_insert_with(|| Bentuk::baru(nama)).clone()
    }

    pub fn unik(nama: &[Kunci]) -> bool {
        nama.iter().enumerate().all(|(i, k)| !nama[..i].contains(k))
    }

    #[inline(always)]
    pub fn posisi<Q: ?Sized + Eq>(&self, k: &Q) -> Option<usize> where Kunci: Borrow<Q> {
        self.nama.iter().position(|n| n.borrow() == k)
    }
}

#[derive(Clone, Debug)]
pub enum IsiPeta {
    Hash(HashMap<Kunci, VmValue, HashPeta>),
    Rapat { bentuk: Arc<Bentuk>, nilai: Vec<VmValue> },
}

fn sip() -> HashPeta { HashPeta(Some(std::collections::hash_map::RandomState::new())) }

impl IsiPeta {
    /// Tabel kosong: SipHash berkunci acak untuk dict, hash cepat untuk yang lain.
    pub fn tabel(n: usize, dict: bool) -> Self {
        IsiPeta::Hash(HashMap::with_capacity_and_hasher(n, if dict { sip() } else { HashPeta(None) }))
    }

    pub fn rapat(bentuk: Arc<Bentuk>, nilai: Vec<VmValue>) -> Self {
        debug_assert_eq!(bentuk.nama.len(), nilai.len());
        IsiPeta::Rapat { bentuk, nilai }
    }

    /// Tabel ber-SipHash (aman untuk kunci dari luar).
    #[inline]
    pub fn pakai_sip(&self) -> bool { matches!(self, IsiPeta::Hash(h) if h.hasher().0.is_some()) }

    #[inline]
    pub fn len(&self) -> usize {
        match self { IsiPeta::Hash(h) => h.len(), IsiPeta::Rapat { nilai, .. } => nilai.len() }
    }

    #[inline]
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    #[inline]
    pub fn get<Q: ?Sized + Hash + Eq>(&self, k: &Q) -> Option<&VmValue> where Kunci: Borrow<Q> {
        match self {
            IsiPeta::Hash(h) => h.get(k),
            IsiPeta::Rapat { bentuk, nilai } => bentuk.posisi(k).map(|i| &nilai[i]),
        }
    }

    #[inline]
    pub fn get_mut<Q: ?Sized + Hash + Eq>(&mut self, k: &Q) -> Option<&mut VmValue> where Kunci: Borrow<Q> {
        match self {
            IsiPeta::Hash(h) => h.get_mut(k),
            IsiPeta::Rapat { bentuk, nilai } => bentuk.posisi(k).map(|i| &mut nilai[i]),
        }
    }

    #[inline]
    pub fn contains_key<Q: ?Sized + Hash + Eq>(&self, k: &Q) -> bool where Kunci: Borrow<Q> { self.get(k).is_some() }

    /// Bentuk rapat yang bertambah nama mendapat bentuk baru (milik nilai ini saja).
    #[inline(always)]
    pub fn insert(&mut self, k: Kunci, v: VmValue) -> Option<VmValue> {
        match self {
            IsiPeta::Hash(h) if h.hasher().0.is_some() => h.insert(k, v),
            IsiPeta::Rapat { bentuk, nilai } if bentuk.posisi(&k).is_some() => {
                let i = bentuk.posisi(&k).expect("diperiksa");
                Some(std::mem::replace(&mut nilai[i], v))
            }
            _ => self.insert_lambat(k, v),
        }
    }

    #[cold]
    fn insert_lambat(&mut self, k: Kunci, v: VmValue) -> Option<VmValue> {
        match self {
            IsiPeta::Hash(h) => {
                let lama = h.insert(k, v);
                if h.hasher().0.is_none() && h.len() > KUNCI_CEPAT {
                    let mut baru = HashMap::with_capacity_and_hasher(h.len(), sip());
                    baru.extend(h.drain());
                    *h = baru;
                }
                lama
            }
            IsiPeta::Rapat { bentuk, nilai } => {
                if let Some(i) = bentuk.posisi(&k) { return Some(std::mem::replace(&mut nilai[i], v)); }
                if nilai.len() < RAPAT_MAKS {
                    let mut nama = bentuk.nama.to_vec();
                    nama.push(k);
                    *bentuk = Bentuk::baru(nama);
                    nilai.push(v);
                    return None;
                }
                let mut h = Self::tabel(nilai.len() + 1, false);
                h.extend(std::mem::take(self));
                *self = h;
                self.insert(k, v)
            }
        }
    }

    #[inline]
    pub fn remove<Q: ?Sized + Hash + Eq>(&mut self, k: &Q) -> Option<VmValue> where Kunci: Borrow<Q> {
        match self {
            IsiPeta::Hash(h) => h.remove(k),
            IsiPeta::Rapat { bentuk, nilai } => {
                let i = bentuk.posisi(k)?;
                let mut nama = bentuk.nama.to_vec();
                nama.remove(i);
                *bentuk = Bentuk::baru(nama);
                Some(nilai.remove(i))
            }
        }
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        match self {
            IsiPeta::Hash(h) => Iter::Hash(h.iter()),
            IsiPeta::Rapat { bentuk, nilai } => Iter::Rapat(bentuk.nama.iter().zip(nilai.iter())),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &Kunci> + '_ { self.iter().map(|(k, _)| k) }

    pub fn values(&self) -> impl Iterator<Item = &VmValue> + '_ { self.iter().map(|(_, v)| v) }
}

impl Default for IsiPeta {
    fn default() -> Self { Self::tabel(0, false) }
}

impl Extend<(Kunci, VmValue)> for IsiPeta {
    fn extend<T: IntoIterator<Item = (Kunci, VmValue)>>(&mut self, it: T) {
        for (k, v) in it { self.insert(k, v); }
    }
}

impl<Q: ?Sized + Hash + Eq> std::ops::Index<&Q> for IsiPeta where Kunci: Borrow<Q> {
    type Output = VmValue;
    fn index(&self, k: &Q) -> &VmValue { self.get(k).expect("kunci ada di peta") }
}

pub enum Iter<'a> {
    Hash(std::collections::hash_map::Iter<'a, Kunci, VmValue>),
    Rapat(std::iter::Zip<std::slice::Iter<'a, Kunci>, std::slice::Iter<'a, VmValue>>),
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a Kunci, &'a VmValue);
    fn next(&mut self) -> Option<Self::Item> {
        match self { Iter::Hash(i) => i.next(), Iter::Rapat(i) => i.next() }
    }
}

impl<'a> IntoIterator for &'a IsiPeta {
    type Item = (&'a Kunci, &'a VmValue);
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Iter<'a> { self.iter() }
}

pub enum IntoIter {
    Hash(std::collections::hash_map::IntoIter<Kunci, VmValue>),
    Rapat(std::iter::Zip<std::vec::IntoIter<Kunci>, std::vec::IntoIter<VmValue>>),
}

impl Iterator for IntoIter {
    type Item = (Kunci, VmValue);
    fn next(&mut self) -> Option<Self::Item> {
        match self { IntoIter::Hash(i) => i.next(), IntoIter::Rapat(i) => i.next() }
    }
}

impl IntoIterator for IsiPeta {
    type Item = (Kunci, VmValue);
    type IntoIter = IntoIter;
    fn into_iter(self) -> IntoIter {
        match self {
            IsiPeta::Hash(h) => IntoIter::Hash(h.into_iter()),
            IsiPeta::Rapat { bentuk, nilai } => IntoIter::Rapat(bentuk.nama.to_vec().into_iter().zip(nilai)),
        }
    }
}
