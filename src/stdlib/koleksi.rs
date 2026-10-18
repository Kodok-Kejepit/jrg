// src/stdlib/koleksi.rs
// Melengkapi namespace `arr::`, `str::`, dan `math::` di backend VM.

use crate::interpreter::{Interpreter, Value};
use crate::interpreter::builtin::NativeFnKind;
use crate::stdlib::sistem::lewat;
use crate::runtime_err;
use crate::vm::value::VmValue;
use crate::vm::vm::Vm;

// ─── Bantuan ─────────────────────────────────────────────────────────────

/// `ke` adalah POSISI argumennya (1-berbasis). Menyebutkannya bukan hiasan:
/// `arr::concat(a, 5)` dan `arr::concat(5, a)` adalah dua kesalahan berbeda,
/// dan tanpa posisinya pembacanya harus menebak yang mana.
fn arr_ke(v: Option<&VmValue>, f: &str, ke: usize) -> Result<Vec<VmValue>, String> {
    match v {
        Some(VmValue::Array(a)) => Ok(a.lock().unwrap_or_else(|p| p.into_inner()).clone()),
        Some(o) => Err(crate::interpreter::pesan_arg_harus_array(f, ke, &o.type_name_penuh())),
        None => Err(format!("{}: kurang argumen", f)),
    }
}

/// Baca sebuah array TANPA menyalinnya: untuk yang cuma melihat satu-dua elemen. `arr_ke`
/// menyalin seluruh isi, jadi `arr::get(a, 0)` dulu O(n) per panggilan.
fn arr_pinjam<R>(v: Option<&VmValue>, f: &str, ke: usize, baca: impl FnOnce(&[VmValue]) -> R) -> Result<R, String> {
    match v {
        Some(VmValue::Array(a)) => Ok(baca(&a.lock().unwrap_or_else(|p| p.into_inner()))),
        Some(o) => Err(crate::interpreter::pesan_arg_harus_array(f, ke, &o.type_name_penuh())),
        None => Err(format!("{}: kurang argumen", f)),
    }
}

fn bil(v: Option<&VmValue>, f: &str) -> Result<i128, String> {
    match v {
        Some(VmValue::Int(n)) => Ok(*n),
        // Kalimat yang sama dengan acuan (`cacah_untuk`/`bulat_untuk`).
        Some(o) => {
            let pecahan = matches!(o, VmValue::Float(_)).then(|| format!("{}", o));
            Err(crate::interpreter::pesan_bukan_bulat(f, &o.type_name_penuh(), pecahan.as_deref()))
        }
        None => Err(format!("{}: kurang argumen", f)),
    }
}

fn pecahan(v: Option<&VmValue>, f: &str) -> Result<f64, String> {
    match v {
        Some(VmValue::Float(x)) => Ok(*x),
        Some(VmValue::Int(n)) => Ok(*n as f64),
        Some(o) => Err(format!("{}: butuh angka, dapat `{}`", f, o.type_name_penuh())),
        None => Err(format!("{}: kurang argumen", f)),
    }
}

// `some`/`none` sekarang milik `VmValue` — lihat `vm/value.rs`. Dulu berkas
// ini dan `stdlib/dict.rs` masing-masing punya salinannya sendiri.
fn some(v: VmValue) -> VmValue { VmValue::some(v) }
fn none() -> VmValue { VmValue::none() }

/// Argumen bilangan yang WAJIB ada.
fn bil_wajib(v: Option<&VmValue>, f: &str, bentuk: &str) -> Result<i128, String> {
    match v {
        None => Err(format!("{} butuh ({})", f, bentuk)),
        Some(x) => bil(Some(x), f),
    }
}

/// Urutan sebuah PASANGAN nilai, atau `None` kalau keduanya tidak terbanding.
pub fn cmp_vm(a: &VmValue, b: &VmValue) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (a, b) {
        (VmValue::Int(x),   VmValue::Int(y))   => Some(x.cmp(y)),
        (VmValue::Str(x),   VmValue::Str(y))   => Some(x.cmp(y)),
        (VmValue::Rune(x),  VmValue::Rune(y))  => Some(x.cmp(y)),
        (VmValue::Bool(x),  VmValue::Bool(y))  => Some(x.cmp(y)),
        // Angka campur int/float dibandingkan sebagai angka; NaN tidak punya
        // urutan terhadap apa pun — bukan "sama", dan bukan "terbesar".
        (VmValue::Int(_) | VmValue::Float(_), VmValue::Int(_) | VmValue::Float(_)) => {
            let f = |v: &VmValue| match v {
                VmValue::Int(n) => *n as f64,
                VmValue::Float(x) => *x,
                _ => f64::NAN,
            };
            f(a).partial_cmp(&f(b))
        }
        (VmValue::Array(x), VmValue::Array(y)) => {
            // Penyimpanan yang sama: `Equal` tanpa mengunci. Mengunci Mutex yang
            // sama dua kali menggantung selamanya.
            if std::sync::Arc::ptr_eq(x, y) {
                // Dikunci sekali lalu dibandingkan dengan dirinya: array ber-NaN tidak
                // "sama" dengan dirinya sendiri, siapa pun pemilik penyimpanannya.
                let g = x.lock().unwrap_or_else(|p| p.into_inner());
                return urut_leksikografis(&g, &g);
            }
            let (xa, ya) = (x.lock().unwrap_or_else(|p| p.into_inner()),
                            y.lock().unwrap_or_else(|p| p.into_inner()));
            urut_leksikografis(&xa, &ya)
        }
        (VmValue::Tuple(x), VmValue::Tuple(y)) => urut_leksikografis(
            &crate::vm::value::isi_tuple(x), &crate::vm::value::isi_tuple(y)),
        _ => None,
    }
}

/// Leksikografis: elemen per elemen, lalu panjangnya. Elemen yang tidak
/// terbanding membatalkan seluruh pembandingan, bukan dilewati.
fn urut_leksikografis(xs: &[VmValue], ys: &[VmValue]) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    for (ex, ey) in xs.iter().zip(ys.iter()) {
        match cmp_vm(ex, ey)? {
            Ordering::Equal => continue,
            lain => return Some(lain),
        }
    }
    Some(xs.len().cmp(&ys.len()))
}

fn urutkan(v: Vec<VmValue>, f: &str) -> Result<Vec<VmValue>, String> {
    urut_arah(v, f, false)
}

/// Urutkan naik atau turun, lewat pengurutan bersama yang pembandingnya boleh
/// GAGAL — `sort_by` bawaan Rust cuma bisa dijawab `Equal` untuk pasangan tak …
fn urut_arah(v: Vec<VmValue>, f: &str, turun: bool) -> Result<Vec<VmValue>, String> {
    crate::interpreter::urut_stabil(v, |a, b| {
        // Kalimatnya satu definisi (`pesan_urut_tak_sebanding`), supaya
        // interpreter dan VM tidak bisa menjawab berbeda untuk program yang sama.
        let o = cmp_vm(a, b).ok_or_else(|| crate::interpreter::pesan_urut_tak_sebanding(
            f, &a.type_name_penuh(), &b.type_name_penuh()))?;
        Ok(if turun { o.reverse() } else { o })
    })
}

/// Ubah sebuah CACAH (jumlah/panjang) dari integer bahasa ke `usize` dengan
/// MENJENUH, bukan memotong.
fn cacah_wajib(n: i128, f: &str, apa: &str) -> Result<usize, String> {
    if n < 0 {
        return Err(crate::interpreter::pesan_ukuran_tak_masuk_akal(f, apa, n));
    }
    Ok(cacah(n))
}

fn cacah(n: i128) -> usize {
    usize::try_from(n.max(0)).unwrap_or(usize::MAX)
}

/// Index argumen builtin, aturan `a[i]` (negatif dari belakang), satu definisi dengan acuannya
/// (`interpreter::normal_indeks`). `sisip`: `insert` boleh menunjuk satu lewat ujung.
fn idx(i: i128, panjang: usize, f: &str, sisip: bool) -> Result<usize, String> {
    crate::interpreter::normal_indeks(i, panjang, sisip)
        .ok_or_else(|| crate::interpreter::pesan_index_luar_batas(f, i, panjang))
}

// ─── Registrasi ──────────────────────────────────────────────────────────

pub fn register(vm: &mut Vm) {
    daftar_arr(vm);
    daftar_str(vm);
    daftar_math(vm);
}

fn daftar_arr(vm: &mut Vm) {
    vm.def("arr::first", |a| {
        Ok(arr_pinjam(a.get(0), "arr::first", 1, |v| v.first().cloned())?.map(some).unwrap_or_else(none))
    });
    vm.def("arr::last", |a| {
        Ok(arr_pinjam(a.get(0), "arr::last", 1, |v| v.last().cloned())?.map(some).unwrap_or_else(none))
    });
    // Penopang method `a.get(i)`. Mengembalikan Option — apa pun yang
    // mungkin tidak ketemu selalu Option; index negatif dihitung dari belakang seperti `a[i]`.
    vm.def("arr::get", |a| {
        let panjang = arr_pinjam(a.get(0), "arr::get", 1, |v| v.len())?;
        let i = bil_wajib(a.get(1), "arr::get", "arr, idx")?;
        let Some(i) = crate::interpreter::normal_indeks(i, panjang, false) else { return Ok(none()) };
        Ok(arr_pinjam(a.get(0), "arr::get", 1, |v| v.get(i).cloned())?.map(some).unwrap_or_else(none))
    });
    vm.def("arr::get_or", |a| {
        let panjang = arr_pinjam(a.get(0), "arr::get_or", 1, |v| v.len())?;
        let i = bil_wajib(a.get(1), "arr::get_or", "arr, idx, cadangan")?;
        let cadangan = a.get(2).cloned().unwrap_or(VmValue::Nil);
        let Some(i) = crate::interpreter::normal_indeks(i, panjang, false) else { return Ok(cadangan) };
        Ok(arr_pinjam(a.get(0), "arr::get_or", 1, |v| v.get(i).cloned())?.unwrap_or(cadangan))
    });
    vm.def("arr::is_empty", |a| {
        Ok(VmValue::Bool(arr_pinjam(a.get(0), "arr::is_empty", 1, |v| v.is_empty())?))
    });
    // `Option`, sama dengan `find` dan dengan bentuk method-nya. `-1` dulu
    // dipakai sebagai "tidak ketemu" — dan `-1` adalah indeks yang SAH di JRG,
    // jadi `a[arr::index_of(a, x)]` diam-diam membaca elemen TERAKHIR.
    vm.def("arr::index_of", |a| {
        let v = arr_ke(a.get(0), "arr::index_of", 1)?;
        // Yang dicari WAJIB disebut: tanpa itu jawabannya "tidak ketemu"
        // untuk pencarian yang tidak pernah dinyatakan.
        let Some(cari) = a.get(1).cloned() else {
            return Err("arr::index_of butuh (arr, val)".to_string());
        };
        Ok(match crate::vm::value::posisi_vm_eq(v.iter(), &cari)? {
            Some(i) => some(VmValue::Int(i as i128)),
            None => none(),
        })
    });
    vm.def("arr::concat", |a| {
        let mut v = arr_ke(a.get(0), "arr::concat", 1)?;
        v.extend(arr_ke(a.get(1), "arr::concat", 2)?);
        Ok(VmValue::array(v))
    });
    // Turunan parser untuk `[...x]` dan `f(...x)`: pesannya menyebut `...`, bukan nama ini.
    vm.def("arr::__sebar", |a| {
        let potong = |v: Option<&VmValue>| match v {
            Some(VmValue::Array(x)) => Ok(x.clone()),
            lain => Err(crate::interpreter::pesan_sebar_bukan_array(
                &lain.map(|x| x.type_name_penuh()).unwrap_or_else(|| "nil".to_string()))),
        };
        let kiri = potong(a.get(0))?;
        let kanan = potong(a.get(1))?;
        // Kiri kosong (`[...x]`, `f(...x)`): isinya dipakai bersama, disalin baru saat ditulis.
        let kosong = kiri.lock().unwrap().is_empty();
        if kosong {
            return Ok(VmValue::Array(kanan));
        }
        let mut v = kiri.lock().unwrap().clone();
        let tambahan = kanan.lock().unwrap().clone();
        v.extend(tambahan);
        Ok(VmValue::array(v))
    });
    vm.def("arr::unique", |a| {
        let v = arr_ke(a.get(0), "arr::unique", 1)?;
        let mut out: Vec<VmValue> = Vec::new();
        for x in v {
            if !crate::vm::value::ada_vm_eq(out.iter(), &x)? {
                out.push(x);
            }
        }
        Ok(VmValue::array(out))
    });
    vm.def("arr::take", |a| {
        arr_pinjam(a.get(0), "arr::take", 1, |_| ())?;
        let n = cacah_wajib(bil_wajib(a.get(1), "arr::take", "arr, n")?, "arr::take", "jumlah")?;
        Ok(VmValue::array(arr_pinjam(a.get(0), "arr::take", 1, |v| v.iter().take(n).cloned().collect())?))
    });
    vm.def("arr::skip", |a| {
        let v = arr_ke(a.get(0), "arr::skip", 1)?;
        let n = cacah_wajib(bil_wajib(a.get(1), "arr::skip", "arr, n")?, "arr::skip", "jumlah")?;
        Ok(VmValue::array(v.into_iter().skip(n).collect()))
    });
    vm.def("arr::min", |a| {
        let v = arr_ke(a.get(0), "arr::min", 1)?;
        let mut it = v.into_iter();
        let Some(mut m) = it.next() else {
            return Err(crate::interpreter::pesan_koleksi_kosong("arr::min", "nilai terkecil"));
        };
        for x in it {
            let o = cmp_vm(&x, &m).ok_or_else(|| crate::interpreter::pesan_min_max_tak_sebanding(
                "arr::min", &x.type_name_penuh(), &m.type_name_penuh()))?;
            if o == std::cmp::Ordering::Less { m = x; }
        }
        Ok(m)
    });
    vm.def("arr::max", |a| {
        let v = arr_ke(a.get(0), "arr::max", 1)?;
        let mut it = v.into_iter();
        let Some(mut m) = it.next() else {
            return Err(crate::interpreter::pesan_koleksi_kosong("arr::max", "nilai terbesar"));
        };
        for x in it {
            let o = cmp_vm(&x, &m).ok_or_else(|| crate::interpreter::pesan_min_max_tak_sebanding(
                "arr::max", &x.type_name_penuh(), &m.type_name_penuh()))?;
            if o == std::cmp::Ordering::Greater { m = x; }
        }
        Ok(m)
    });
    // Dilipat kiri-ke-kanan dengan `*` bahasa, seperti acuan dan `arr::sum`. Dulu int dan
    // float dikalikan di dua akumulator, jadi `[MAX, 0.5, 4]` ditolak di sini saja.
    vm.def("arr::product", |a| {
        let v = arr_ke(a.get(0), "arr::product", 1)?;
        if let Some(x) = v.iter().find(|x| !matches!(x, VmValue::Int(_) | VmValue::Float(_))) {
            return Err(crate::interpreter::pesan_elemen_bukan_angka(
                "arr::product", &x.type_name_penuh()));
        }
        v.iter().try_fold(VmValue::Int(1), |acc, x| acc.mul(x))
    });
    vm.def("arr::flatten", |a| {
        let v = arr_ke(a.get(0), "arr::flatten", 1)?;
        let mut out = Vec::new();
        for x in v {
            match x {
                VmValue::Array(inner) => {
                    out.extend(inner.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned())
                }
                lain => out.push(lain),
            }
        }
        Ok(VmValue::array(out))
    });
    vm.def("arr::chunks", |a| {
        let v = arr_ke(a.get(0), "arr::chunks", 1)?;
        let n = bil_wajib(a.get(1), "arr::chunks", "arr, size")?;
        if n <= 0 {
            return Err(crate::interpreter::pesan_ukuran_tak_masuk_akal(
                "arr::chunks", "ukuran", n));
        }
        Ok(VmValue::array(
            v.chunks(cacah(n)).map(|c| VmValue::array(c.to_vec())).collect(),
        ))
    });
    vm.def("arr::windows", |a| {
        let v = arr_ke(a.get(0), "arr::windows", 1)?;
        let n = bil_wajib(a.get(1), "arr::windows", "arr, size")?;
        if n <= 0 {
            return Err(crate::interpreter::pesan_ukuran_tak_masuk_akal(
                "arr::windows", "ukuran", n));
        }
        if v.len() < cacah(n) {
            return Ok(VmValue::array(Vec::new()));
        }
        Ok(VmValue::array(
            v.windows(cacah(n)).map(|c| VmValue::array(c.to_vec())).collect(),
        ))
    });
    vm.def("arr::zip", |a| {
        if a.len() < 2 { return Err(crate::interpreter::pesan_zip_kurang()); }
        let x = arr_ke(a.get(0), "arr::zip", 1)?;
        let y = arr_ke(a.get(1), "arr::zip", 2)?;
        Ok(VmValue::array(
            x.into_iter()
                .zip(y)
                .map(|(p, q)| VmValue::tuple(vec![p, q]))
                .collect(),
        ))
    });
    vm.def("arr::enumerate", |a| {
        if a.is_empty() {
            return Err(crate::interpreter::pesan_butuh_argumen("arr::enumerate", 1, "(arr)", 0));
        }
        let v = arr_ke(a.get(0), "arr::enumerate", 1)?;
        Ok(VmValue::array(
            v.into_iter()
                .enumerate()
                .map(|(i, x)| VmValue::tuple(vec![VmValue::Int(i as i128), x]))
                .collect(),
        ))
    });
    vm.def("arr::insert", |a| {
        // Jumlah DULU, seperti acuannya — dan nilainya wajib: dulu yang tidak
        // ditulis diisi `nil`, sebuah argumen yang dikarang.
        if a.len() < 3 { return Err("arr::insert butuh (arr, idx, val)".to_string()); }
        let mut v = arr_ke(a.get(0), "arr::insert", 1)?;
        let i = bil(a.get(1), "arr::insert")?;
        // Menyisip di ujung (== panjang) sah; itu artinya menambah di akhir.
        let pos = idx(i, v.len(), "arr::insert", true)?;
        v.insert(pos, a[2].clone());
        Ok(VmValue::array(v))
    });
    // Mengembalikan (array baru, nilai yang dibuang) — murni, jadi array
    // asalnya tidak berubah.
    vm.def("arr::remove", |a| {
        if a.len() < 2 { return Err("arr::remove butuh (arr, idx)".to_string()); }
        let mut v = arr_ke(a.get(0), "arr::remove", 1)?;
        let i = idx(bil(a.get(1), "arr::remove")?, v.len(), "arr::remove", false)?;
        let dibuang = v.remove(i);
        Ok(VmValue::tuple(vec![VmValue::array(v), dibuang]))
    });
    vm.def("arr::swap", |a| {
        if a.len() < 3 { return Err("arr::swap butuh (arr, i, j)".to_string()); }
        let mut v = arr_ke(a.get(0), "arr::swap", 1)?;
        let i = idx(bil(a.get(1), "arr::swap")?, v.len(), "arr::swap", false)?;
        let j = idx(bil(a.get(2), "arr::swap")?, v.len(), "arr::swap", false)?;
        v.swap(i, j);
        Ok(VmValue::array(v))
    });
    vm.def("arr::rotate", |a| {
        let v = arr_ke(a.get(0), "arr::rotate", 1)?;
        // `n` diperiksa SEBELUM pintasan array kosong. Dulu terbalik, jadi
        // `arr::rotate([])` menjawab `[]` tanpa pernah menyebut bahwa `n`
        // tidak diberikan — sementara interpreter menolaknya.
        let n = match a.get(1) {
            Some(VmValue::Int(n)) => *n,
            Some(o) => return Err(
                crate::interpreter::pesan_rotate_n(&o.type_name_penuh())),
            None => return Err(crate::interpreter::pesan_rotate_kurang()),
        };
        if v.is_empty() {
            return Ok(VmValue::array(v));
        }
        let len = v.len() as i128;
        let k = ((n % len) + len) % len;
        let mut out = v[k as usize..].to_vec();
        out.extend_from_slice(&v[..k as usize]);
        Ok(VmValue::array(out))
    });
    vm.def("arr::repeat", |a| {
        let v = arr_ke(a.get(0), "arr::repeat", 1)?;
        let nn = bil_wajib(a.get(1), "arr::repeat", "arr, n")?;
        // `n` menentukan BESAR ALOKASI, jadi batasnya diperiksa SEBELUM alokasinya.
        crate::interpreter::cek_batas_elemen("arr::repeat", nn)?;
        let n = cacah_wajib(nn, "arr::repeat", "jumlah")?;
        // Yang dialokasikan TOTALnya, bukan `n`.
        crate::interpreter::cek_batas_elemen("arr::repeat",
            (v.len() as i128).saturating_mul(n as i128))?;
        let mut out = Vec::with_capacity(v.len() * n);
        for _ in 0..n {
            out.extend(v.iter().cloned());
        }
        Ok(VmValue::array(out))
    });
    // Urutannya `(nilai, jumlah)` — nilainya dulu, baru berapa kali.
    vm.def("arr::fill", |a| {
        if a.len() < 2 {
            return Err("arr::fill butuh (val, n)".to_string());
        }
        let n = bil(a.get(1), "arr::fill")?;
        if n < 0 {
            return Err(crate::interpreter::pesan_ukuran_tak_masuk_akal(
                "arr::fill", "jumlah", n));
        }
        // Batas diperiksa SEBELUM alokasinya; tanpa ini `arr::fill(x, 1e18)`
        // mematikan proses dengan `capacity overflow` dari dalam Rust.
        crate::interpreter::cek_batas_elemen("arr::fill", n)?;
        Ok(VmValue::array(vec![a[0].clone(); n as usize]))
    });
    // Sengaja di sini, bukan di `vm.rs`: `register_all` jalan sesudah
    // `register_natives`, jadi pendaftaran ini yang menang.
    vm.def("arr::sort", |a| {
        Ok(VmValue::array(urutkan(arr_ke(a.get(0), "arr::sort", 1)?, "arr::sort")?))
    });
    vm.def("arr::sort_desc", |a| {
        let v = urut_arah(arr_ke(a.get(0), "arr::sort_desc", 1)?, "arr::sort_desc", true)?;
        Ok(VmValue::array(v))
    });
    vm.def("arr::median", |a| {
        // Syarat ANGKA diperiksa lebih dulu, seperti mesin acuan: sebuah array
        // berisi string bukan "urutannya tidak terdefinisi" melainkan bukan data
        // yang punya median sama sekali.
        let v = arr_ke(a.get(0), "arr::median", 1)?;
        if v.is_empty() {
            return Err(crate::interpreter::pesan_koleksi_kosong("arr::median", "median"));
        }
        let mut fs = Vec::with_capacity(v.len());
        for x in &v {
            fs.push(match x {
                VmValue::Int(n) => *n as f64,
                VmValue::Float(f) => *f,
                o => return Err(crate::interpreter::pesan_elemen_bukan_angka(
                    "arr::median", &o.type_name_penuh())),
            });
        }
        if fs.iter().any(|x| x.is_nan()) {
            return Err(crate::interpreter::pesan_nan_tak_terurut("arr::median"));
        }
        fs.sort_by(|a, b| a.total_cmp(b));
        let n = fs.len();
        Ok(VmValue::Float(if n % 2 == 1 {
            fs[n / 2]
        } else {
            (fs[n / 2 - 1] + fs[n / 2]) / 2.0
        }))
    });
    vm.def("arr::variance", |a| {
        let v = arr_ke(a.get(0), "arr::variance", 1)?;
        if v.is_empty() {
            return Err("arr::variance: array kosong tidak punya variansi".to_string());
        }
        let mut angka = Vec::with_capacity(v.len());
        for x in &v {
            angka.push(match x {
                VmValue::Int(n) => *n as f64,
                VmValue::Float(f) => *f,
                o => return Err(crate::interpreter::pesan_elemen_bukan_angka(
                    "arr::variance", &o.type_name_penuh())),
            });
        }
        let rata = angka.iter().sum::<f64>() / angka.len() as f64;
        Ok(VmValue::Float(
            angka.iter().map(|x| (x - rata) * (x - rata)).sum::<f64>() / angka.len() as f64,
        ))
    });
    // `arr::sample` DULU menjawab `Option::Some(elemen pertama)` dan
    // `arr::shuffle` mengembalikan masukannya apa adanya.
}

fn daftar_str(vm: &mut Vm) {
    // `str::substr` dan `str::slice` DULU punya salinan sendiri di sini, dan
    // salinan itu sudah menyimpang dari acuannya dengan dua cara sekaligus:
    vm.def("str::substr", |a| lewat(a, |v| Interpreter::dispatch_str_inner_lepas(
        &NativeFnKind::StrSubstr, v)));
    vm.def("str::slice", |a| lewat(a, |v| Interpreter::dispatch_str_inner_lepas(
        &NativeFnKind::StrSlice, v)));

    // Enam method string yang sebelumnya cuma hidup di interpreter.
    macro_rules! metode_str {
        ($($nama:literal),* $(,)?) => { $(
            vm.def(concat!("str::", $nama), |a| lewat(a, |v| {
                // Tanpa argumen sama sekali ini DITOLAK, bukan dijawab.
                if v.is_empty() {
                    return runtime_err!(
                        "str::{}: butuh string-nya sebagai argumen pertama", $nama);
                }
                // `to_string` MEMANG mengubah nilai apa pun jadi teks; sisanya
                // mengolah ISI, jadi argumen pertama yang bukan teks ditolak
                // lewat pintu yang sama dengan `str::` lainnya.
                let s = if $nama == "to_string" {
                    format!("{}", v.first().cloned().unwrap_or(Value::Nil))
                } else {
                    match crate::interpreter::builtin::str_ops::teks(
                        v.first().unwrap_or(&Value::Nil), concat!("str::", $nama)) {
                        Ok(s) => s,
                        Err(e) => return Err(e),
                    }
                };
                Interpreter::metode_str(&s, $nama, &v[1..]).unwrap_or_else(
                    || unreachable!("`{}` didaftarkan tapi tidak dikenal `metode_str`", $nama))
            }));
        )* };
    }
    metode_str!("trim_start", "trim_end", "to_string", "byte_len", "sub", "substring");

    // ── Seluruh `str::` memanggil implementasi INTERPRETER ──────────
    vm.def("str::bytes", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrBytes, v)));
    vm.def("str::camel_case", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrCamelCase, v)));
    vm.def("str::capitalize", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrCapitalize, v)));
    vm.def("str::center", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrCenter, v)));
    vm.def("str::char_at", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrCharAt, v)));
    vm.def("str::chars", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrChars, v)));
    vm.def("str::count", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrCount, v)));
    vm.def("str::find", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrFind, v)));
    vm.def("str::is_alpha", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrIsAlpha, v)));
    vm.def("str::is_alphanumeric", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrIsAlphaNum, v)));
    vm.def("str::is_empty", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrIsEmpty, v)));
    vm.def("str::is_lower", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrIsLower, v)));
    vm.def("str::is_numeric", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrIsNumeric, v)));
    vm.def("str::is_upper", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrIsUpper, v)));
    vm.def("str::join", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrJoin, v)));
    vm.def("str::lines", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrLines, v)));
    vm.def("str::pad_left", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrPadLeft, v)));
    vm.def("str::pad_right", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrPadRight, v)));
    vm.def("str::repeat", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrRepeat, v)));
    vm.def("str::snake_case", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrSnakeCase, v)));
    vm.def("str::strip_prefix", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrStripPrefix, v)));
    vm.def("str::strip_suffix", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrStripSuffix, v)));
    vm.def("str::swap_case", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrSwapCase, v)));
    // `str::to_float` bukan keluarga `Str*` — `NativeFnKind`-nya `ParseFloat`, dan
    // dispatch-nya ada di `builtin/mod.rs`, bukan di `str_ops.rs`.
    vm.def("str::to_float", |a| lewat(a, |v| Interpreter::parse_float_lepas(
        v.first().unwrap_or(&Value::Nil))));
    // `str::to_int` bukan keluarga `Str*` — `NativeFnKind`-nya `ParseInt`, dan
    // dispatch-nya ada di `builtin/mod.rs`, bukan di `str_ops.rs`.
    vm.def("str::to_int", |a| lewat(a, |v| Interpreter::parse_int_lepas(
        v.first().unwrap_or(&Value::Nil))));
    vm.def("str::words", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrWords, v)));

    // Sepuluh nama yang dulu ditulis ulang di `vm/vm.rs`, sekarang memanggil
    // implementasi interpreter yang SAMA. Lihat catatan di tempat asalnya
    // untuk apa yang melenceng selama ada dua salinan.
    vm.def("str::upper", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrUpper, v)));
    vm.def("str::lower", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrLower, v)));
    vm.def("str::trim", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrTrim, v)));
    vm.def("str::reverse", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrReverse, v)));
    vm.def("str::contains", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrContains, v)));
    vm.def("str::starts_with", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrStartsWith, v)));
    vm.def("str::ends_with", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrEndsWith, v)));
    vm.def("str::replace", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrReplace, v)));
    vm.def("str::split", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrSplit, v)));
    vm.def("str::title", |a| lewat(a, |v| Interpreter::dispatch_str_inner(
        &NativeFnKind::StrTitle, v)));

    // `str::format` memanggil dispatch interpreter, bukan salinan kedua.
    // Ia tidak lewat `metode_str!` karena bentuknya beda: argumen pertamanya
    // template, bukan penerima sebuah method.
    vm.def("str::format", |a| {
        lewat(a, |v| Interpreter::dispatch_str_inner_lepas(&NativeFnKind::StrFormat, v))
    });

    // Enam method array yang sebelumnya cuma hidup di interpreter, dengan
    // pola yang sama: aturannya SATU, di `Interpreter::metode_arr_lepas`.
    macro_rules! metode_arr {
        ($($nama:literal),* $(,)?) => { $(
            vm.def(concat!("arr::", $nama), |a| lewat(a, |v| {
                // Argumen yang BUKAN array ditolak, tidak diperlakukan array kosong.
                let arr = match v.first() {
                    Some(Value::Array(x)) => x.to_vec(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        concat!("arr::", $nama), 1, &lain.type_name())),
                    None => return runtime_err!(
                        "arr::{}: butuh array-nya sebagai argumen pertama", $nama),
                };
                Interpreter::metode_arr_lepas(&arr, $nama, &v[1..])
            }));
        )* };
    }
    // `to_string` sengaja TIDAK di sini: ia harus lewat jalur ber-hook (`__to_string`),
    // dan `metode_arr_lepas` tanpa-self tidak bisa memanggil hook.
    metode_arr!("binary_search", "dedup", "drop", "rotate_left", "rotate_right",
                "shuffle", "sample");

    // Method `.fill(v)` — namanya sengaja tidak `arr::fill`, karena fungsi
    // namespace dengan nama itu berarti hal LAIN (`arr::fill(v, n)` membuat …
    vm.def("arr::__metode_starts_with", |a| {
        let (x, y) = (arr_ke(a.first(), "arr::starts_with", 1)?,
                      arr_ke(a.get(1), "arr::starts_with", 2)?);
        Ok(VmValue::Bool(x.len() >= y.len()
            && crate::vm::value::semua_vm_eq(x.iter(), y.iter())?))
    });
    vm.def("arr::__metode_ends_with", |a| {
        let (x, y) = (arr_ke(a.first(), "arr::ends_with", 1)?,
                      arr_ke(a.get(1), "arr::ends_with", 2)?);
        Ok(VmValue::Bool(x.len() >= y.len()
            && crate::vm::value::semua_vm_eq(x[x.len() - y.len()..].iter(), y.iter())?))
    });
    vm.def("arr::__metode_fill", |a| {
        let v = arr_ke(a.get(0), "fill", 1)?;
        let Some(isi) = a.get(1) else {
            return Err(crate::interpreter::pesan_metode_argumen("arr", "fill", 0).to_string());
        };
        Ok(VmValue::array(vec![isi.clone(); v.len()]))
    });

}

fn daftar_math(vm: &mut Vm) {
    // `x.to_string()` untuk tipe tanpa hook sendiri. Namanya INTERNAL supaya ia
    // tidak jadi fungsi telanjang `to_string(x)` yang mesin acuan tidak punya;
    // argumennya tiba sudah dirender lewat hook (lihat `BUILTIN_CETAK`).
    vm.def("__to_string", |a| {
        Ok(match a.first() {
            Some(VmValue::Str(s)) => VmValue::Str(s.clone()),
            Some(lain) => VmValue::str(&format!("{}", lain)),
            None => return Err(crate::interpreter::pesan_butuh_nilai("to_string")),
        })
    });
    // Satu-satunya `math::` yang tinggal di sini: acuan tidak punya
    // padanannya dan meminjam definisi ini lewat jembatan nilai. Sisanya
    // didaftarkan `vm/bawaan.rs` LEWAT implementasi acuan.
    vm.def("math::is_finite", |a| {
        Ok(VmValue::Bool(pecahan(a.get(0), "math::is_finite")?.is_finite()))
    });
}
