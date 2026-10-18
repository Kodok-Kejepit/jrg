// src/vm/hof.rs — HOF bawaan bernamespace (`arr::map` … `ptr::update`): satu badan untuk
// panggilan langsung (`OpCode::HofCall`) dan panggilan lewat NILAI-nya (`let f = arr::map`).

use super::value::{VmNativeFn, VmValue};
use super::vm::*;

/// Fungsi pengganti per nama, supaya `arr::map` punya NILAI di tabel bawaan. Pintu native
/// tidak pernah menjalankannya: `NativeKhusus::Hof` mengalihkannya ke [`Vm::hof_lewat_nilai`].
macro_rules! pengganti {
    ($($n:literal),* $(,)?) => { &[$(($n, {
        // Teksnya beda per nama, jadi pengoptimal tidak bisa menggabung alamatnya (#462).
        fn f(_: &[VmValue]) -> Result<VmValue, String> {
            Err(concat!("`", $n, "` dipanggil tanpa pintu HOF-nya").to_string())
        }
        f as VmNativeFn
    })),*] };
}

pub(crate) const PENGGANTI_HOF: &[(&str, VmNativeFn)] = pengganti!(
    "arr::map", "arr::filter", "arr::reduce", "arr::fold", "arr::any", "arr::all",
    "arr::find", "arr::find_idx", "arr::flat_map", "arr::sort_by", "arr::partition",
    "arr::group_by", "arr::zip_with", "arr::each", "arr::count_if", "arr::max_by",
    "arr::min_by", "arr::sort_by_key", "ptr::update", "mutex::with_lock",
);

/// Nama kedua untuk HOF yang sama: (alias, yang melayaninya). Interpreter memberi
/// keduanya satu jenis, jadi di sini keduanya satu alamat.
pub(crate) const ALIAS_HOF: &[(&str, &str)] = &[("arr::count", "arr::count_if")];

/// HOF yang melayani `nama`, alias diselesaikan; `None` kalau `nama` bukan HOF bawaan.
pub(crate) fn layanan_hof(nama: &str) -> Option<&'static str> {
    let nama = ALIAS_HOF.iter().find(|(a, _)| *a == nama).map_or(nama, |(_, asli)| *asli);
    PENGGANTI_HOF.iter().find(|(n, _)| *n == nama).map(|(n, _)| *n)
}

impl Vm {
    /// Badan tiap HOF bawaan. Argumennya di tumpukan (yang terakhir di puncak), dan
    /// hasilnya didorong ke sana.
    pub(crate) fn hof_namespace(&mut self, name: &str, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        match name {
            "arr::map" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::map", sp)?;
                let mut result = Vec::with_capacity(items.len());
                for item in items {
                    result.push(self.call_value_immediate(f.clone(), vec![item])?);
                }
                self.push(VmValue::array(result));
            }
            "arr::filter" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::filter", sp)?;
                let mut result = Vec::new();
                for item in items {
                    let keep = self.call_value_immediate(f.clone(), vec![item.clone()])?;
                    if keep.is_truthy() { result.push(item); }
                }
                self.push(VmValue::array(result));
            }
            "arr::reduce" => {
                let init = self.pop(sp)?;
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::reduce", sp)?;
                let mut acc = init;
                for item in items {
                    acc = self.call_value_immediate(f.clone(), vec![acc, item])?;
                }
                self.push(acc);
            }
            // Pasangan namespace `a.fold(awal, f)`: nilai awal di DEPAN.
            "arr::fold" => {
                let f = self.pop(sp)?;
                let init = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::fold", sp)?;
                let mut acc = init;
                for item in items {
                    acc = self.call_value_immediate(f.clone(), vec![acc, item])?;
                }
                self.push(acc);
            }
            "arr::any" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::any", sp)?;
                let mut found = false;
                for item in items {
                    if self.call_value_immediate(f.clone(), vec![item])?.is_truthy() { found = true; break; }
                }
                self.push(VmValue::Bool(found));
            }
            "arr::all" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::all", sp)?;
                let mut all_true = true;
                for item in items {
                    if !self.call_value_immediate(f.clone(), vec![item])?.is_truthy() { all_true = false; break; }
                }
                self.push(VmValue::Bool(all_true));
            }
            "arr::find" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::find", sp)?;
                // `Option`, sama dengan bentuk method `.find(f)`.
                let mut found = opsi_none();
                for item in items {
                    if self.call_value_immediate(f.clone(), vec![item.clone()])?.is_truthy() {
                        found = opsi_some(item);
                        break;
                    }
                }
                self.push(found);
            }
            "arr::find_idx" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::find_idx", sp)?;
                let mut ketemu = opsi_none();
                for (i, item) in items.into_iter().enumerate() {
                    if self.call_value_immediate(f.clone(), vec![item])?.is_truthy() {
                        ketemu = opsi_some(VmValue::Int(i as i128));
                        break;
                    }
                }
                self.push(ketemu);
            }
            "arr::each" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::each", sp)?;
                for item in items {
                    self.call_value_immediate(f.clone(), vec![item])?;
                }
                self.push(VmValue::Unit);
            }
            "arr::flat_map" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::flat_map", sp)?;
                let mut out = Vec::new();
                for item in items {
                    match self.call_value_immediate(f.clone(), vec![item])? {
                        VmValue::Array(inner) => out.extend(
                            inner.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned()),
                        lain => out.push(lain),
                    }
                }
                self.push(VmValue::array(out));
            }
            "arr::count_if" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::count_if", sp)?;
                let mut n: i128 = 0;
                for item in items {
                    if self.call_value_immediate(f.clone(), vec![item])?.is_truthy() {
                        n += 1;
                    }
                }
                self.push(VmValue::Int(n));
            }
            "arr::max_by" | "arr::min_by" => {
                let cari_max = name == "arr::max_by";
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let nama_fn = if cari_max { "arr::max_by" } else { "arr::min_by" };
                let items = self.expect_array(&arr, nama_fn, sp)?;
                // Konteksnya nama METHOD, seperti mesin acuan: bentuk namespace
                // di sana memang dialihkan ke jalur method yang sama.
                let terbaik = self.pilih_dgn_kunci(items, f, nama_fn, !cari_max, sp)?;
                self.push(terbaik);
            }
            "arr::sort_by_key" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::sort_by_key", sp)?;
                let out = self.urut_dgn_kunci(items, f, "arr::sort_by_key", sp)?;
                self.push(VmValue::array(out));
            }
            // Komparator mengembalikan angka: <0, 0, >0.
            "arr::sort_by" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::sort_by", sp)?;
                // Ditolak DI SINI, bukan dibiarkan gagal nanti waktu closure-nya dipanggil
                // dengan dua argumen: pesan "fungsi kebanyakan argumen" menuduh closure-nya …
                if self.arity_nilai(&f) == Some(1) {
                    return Err(VmError::at(
                        crate::interpreter::pesan_sort_by_satu_param(), sp));
                }
                let out = self.urut_dgn_pembanding(items, f, sp)?;
                self.push(VmValue::array(out));
            }
            "arr::partition" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::partition", sp)?;
                let (mut ya, mut tidak) = (Vec::new(), Vec::new());
                for item in items {
                    if self.call_value_immediate(f.clone(), vec![item.clone()])?.is_truthy() {
                        ya.push(item);
                    } else {
                        tidak.push(item);
                    }
                }
                self.push(VmValue::tuple(vec![VmValue::array(ya), VmValue::array(tidak)]));
            }
            // Lewat `kelompokkan` yang sama dengan bentuk method-nya:
            // dua salinan adalah cara satu nama berakhir punya dua arti.
            "arr::group_by" => {
                let f = self.pop(sp)?;
                let arr = self.pop(sp)?;
                let items = self.expect_array(&arr, "arr::group_by", sp)?;
                let hasil = self.kelompokkan(items, f)?;
                self.push(hasil);
            }
            "arr::zip_with" => {
                let f = self.pop(sp)?;
                let b = self.pop(sp)?;
                let a = self.pop(sp)?;
                let xa = self.expect_array(&a, "arr::zip_with", sp)?;
                let xb = self.expect_array_ke(&b, "arr::zip_with", 2, sp)?;
                let mut out = Vec::new();
                for (p, q) in xa.into_iter().zip(xb) {
                    out.push(self.call_value_immediate(f.clone(), vec![p, q])?);
                }
                self.push(VmValue::array(out));
            }
            "ptr::update" => {
                let f = self.pop(sp)?;
                let ptr = self.pop(sp)?;
                let cell = vm_ptr_cell(&ptr, "ptr::update")
                    .map_err(|error| VmError::at(error, sp))?;
                let mut value = cell.lock().unwrap();
                let _sibuk = crate::dok::ptr::PenandaSibuk::baru(
                    super::vm::vm_ptr_id(&ptr, "ptr::update").unwrap_or(0));
                let updated = self.call_value_immediate(
                    f,
                    vec![value.clone()],
                )?;
                *value = updated.clone();
                self.push(updated);
            }
            "mutex::with_lock" => {
                let f = self.pop(sp)?;
                let m = self.pop(sp)?;
                let k = crate::stdlib::future::kunci(std::slice::from_ref(&m), "mutex::with_lock")
                    .map_err(|e| VmError::at(e, sp))?;
                // Dilepas saat keluar cakupan, juga kalau `f` gagal.
                let _terkunci = crate::sinkron::Terkunci::baru(k, "mutex::with_lock")
                    .map_err(|e| VmError::at(e, sp))?;
                let hasil = self.call_value_immediate(f, vec![])?;
                self.push(hasil);
            }
            other => return Err(VmError::at(format!("HOF tidak dikenal: {}", other), sp)),
        }
        Ok(())
    }

    /// HOF yang dipanggil lewat NILAI-nya (`let f = arr::map; f(a, g)`, `terap(arr::map, …)`):
    /// badannya sama dengan `HofCall`. Jumlah argumennya sudah diperiksa pintu native.
    pub(crate) fn hof_lewat_nilai(&mut self, nama: &str, args: &[VmValue]) -> VmResult<VmValue> {
        let dasar = self.stack.len();
        for a in args { self.push(a.clone()); }
        let hasil = self.hof_namespace(nama, None).and_then(|_| self.pop(None));
        self.stack.truncate(dasar);
        hasil
    }
}
