// src/interpreter/builtin/arr.rs — interpreter/builtin/arr.rs
// Dispatch untuk namespace `arr::` (map/filter/reduce/sort/HOF, statistik

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use crate::interpreter::eval_expr::{cacah_untuk, bulat_untuk, indeks_untuk};
use crate::runtime_err;
use super::NativeFnKind;

pub(crate) fn fisher_yates_shuffle(idxs: &mut [usize]) {
    let n = idxs.len();
    if n < 2 { return; }
    let rand_bytes = crate::dok::crypto::secure_random_bytes(n * 8);
    for i in (1..n).rev() {
        let mut b = [0u8; 8];
        b.copy_from_slice(&rand_bytes[(i - 1) * 8..(i - 1) * 8 + 8]);
        let r = (u64::from_le_bytes(b) as usize) % (i + 1);
        idxs.swap(i, r);
    }
}

impl Interpreter {
    pub(super) fn dispatch_arr(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind,
            ChanNew | ArrEach |
            ArrMap | ArrFilter | ArrReduce | ArrAny | ArrAll | ArrFind | ArrFindIdx |
            ArrFlatten | ArrZip | ArrUnique | ArrSort | ArrSortBy | ArrReverse |
            ArrSum | ArrProduct | ArrMean | ArrMedian | ArrVariance | ArrStdDev |
            ArrMin | ArrMax | ArrContains | ArrIndexOf | ArrJoin | ArrPush | ArrPop |
            ArrInsert | ArrRemove | ArrTake | ArrSkip | ArrChunks | ArrWindows |
            ArrRepeat | ArrFill | ArrEnumerate |
            ArrFirst | ArrLast | ArrIsEmpty | ArrConcat | ArrSebar | ArrFlatMap | ArrGroupBy |
            ArrPartition | ArrZipWith | ArrShuffle | ArrSample | ArrSwap | ArrRotate |
            ArrCountIf | ArrMaxBy | ArrMinBy | ArrSortByKey | ArrFold
        ) {
            return None;
        }
        Some(self.dispatch_arr_inner(kind, args))
    }

    pub(crate) fn dispatch_arr_inner(&mut self, kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        fn fst(args: &[Value]) -> Value { args.get(0).cloned().unwrap_or(Value::Nil) }
        match kind {
            ChanNew => {
                // Kapasitasnya boleh hilang; `Chan::new()` berarti tanpa buffer, sama dengan
                // `make_chan(int)`.
                let kapasitas = match args.first() {
                    None => 0,
                    Some(Value::Int(n)) if *n >= 0 => {
                        crate::interpreter::cek_batas_elemen("Chan::new", *n)
                            .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                        *n as usize
                    }
                    Some(Value::Int(n)) => return runtime_err!("{}",
                        crate::interpreter::pesan_kapasitas_chan(Some(*n), "int")),
                    Some(v) => return runtime_err!("{}",
                        crate::interpreter::pesan_kapasitas_chan(None, &v.type_name())),
                };
                Ok(Value::Channel(crate::interpreter::value::channel_new(kapasitas)))
            }
            ArrEach => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::each")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::each", 1, &o.type_name())) };
                let f = args[1].clone();
                for v in arr { self.call_fn(f.clone(), vec![v])?; }
                // `unit`, bukan `nil`: ia tidak MENCARI apa-apa, ia cuma
                // menjalankan sesuatu — dan aturan bahasa ini membedakan
                // keduanya (`typeof` membedakannya).
                Ok(Value::Unit)
            }
            ArrMap => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::map")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::map", 1, &o.type_name())) };
                let f = args[1].clone();
                let mut result = Vec::with_capacity(arr.len());
                for v in arr { result.push(self.call_fn(f.clone(), vec![v])?); }
                Ok(Value::Array(result.into()))
            }
            ArrFilter => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::filter")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::filter", 1, &o.type_name())) };
                let f = args[1].clone();
                let mut result = Vec::new();
                for v in arr { let keep = self.call_fn(f.clone(), vec![v.clone()])?; if self.truthy(&keep) { result.push(v); } }
                Ok(Value::Array(result.into()))
            }
            ArrReduce => {
                if args.len() < 3 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::reduce")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::reduce", 1, &o.type_name())) };
                let f = args[1].clone(); let mut acc = args[2].clone();
                for v in arr { acc = self.call_fn(f.clone(), vec![acc, v])?; }
                Ok(acc)
            }
            // Pasangan namespace untuk `a.fold(awal, f)`, yang selama ini cuma ada sebagai method.
            ArrFold => {
                if args.len() < 3 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::fold")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::fold", 1, &o.type_name())) };
                let mut acc = args[1].clone(); let f = args[2].clone();
                for v in arr { acc = self.call_fn(f.clone(), vec![acc, v])?; }
                Ok(acc)
            }
            ArrAny => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::any")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::any", 1, &o.type_name())) };
                let f = args[1].clone();
                for v in arr { let r = self.call_fn(f.clone(), vec![v])?; if self.truthy(&r) { return Ok(Value::Bool(true)); } }
                Ok(Value::Bool(false))
            }
            ArrAll => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::all")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::all", 1, &o.type_name())) };
                let f = args[1].clone();
                for v in arr { let r = self.call_fn(f.clone(), vec![v])?; if !self.truthy(&r) { return Ok(Value::Bool(false)); } }
                Ok(Value::Bool(true))
            }
            ArrFind => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::find")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::find", 1, &o.type_name())) };
                let f = args[1].clone();
                // `Option`, sama dengan bentuk method `.find(f)`. Sebelumnya bentuk
                // namespace menjawab nilai telanjang dan `nil` waktu tidak ketemu —
                // dan `nil` juga BISA jadi isi array, jadi keduanya tak terbedakan.
                for v in arr {
                    let r = self.call_fn(f.clone(), vec![v.clone()])?;
                    if self.truthy(&r) { return Ok(crate::interpreter::some_value(v)); }
                }
                Ok(crate::interpreter::none_value())
            }
            ArrFindIdx => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::find_idx")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::find_idx", 1, &o.type_name())) };
                let f = args[1].clone();
                // `Option`, bukan `-1`: di JRG `-1` adalah indeks yang SAH (dihitung dari
                // belakang), jadi `a[arr::find_idx(a, p)]` waktu tidak ketemu membaca elemen …
                for (i, v) in arr.into_iter().enumerate() {
                    let r = self.call_fn(f.clone(), vec![v])?;
                    if self.truthy(&r) {
                        return Ok(crate::interpreter::some_value(Value::Int(i as i128)));
                    }
                }
                Ok(crate::interpreter::none_value())
            }
            // ── Bentuk namespace untuk empat HOF yang tadinya method-saja ──
            ArrCountIf | ArrMaxBy | ArrMinBy | ArrSortByKey => {
                let metode = match kind {
                    ArrCountIf   => "count_if",
                    ArrMaxBy     => "max_by",
                    ArrMinBy     => "min_by",
                    _            => "sort_by_key",
                };
                if args.len() < 2 {
                    return runtime_err!("{}", crate::interpreter::pesan_hof_kurang(
                        &format!("arr::{}", metode)));
                }
                if !matches!(args[0], Value::Array(_)) {
                    return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        &format!("arr::{}", metode), 1, &args[0].type_name()));
                }
                // Array kosong diperiksa DI SINI, bukan diserahkan ke jalur method: pesannya
                // harus menyebut nama yang DITULIS pemakai (`arr::max_by`), bukan nama …
                if matches!(kind, ArrMaxBy | ArrMinBy) {
                    if let Value::Array(a) = &args[0] {
                        if a.is_empty() {
                            return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                                &format!("arr::{}", metode),
                                if matches!(kind, ArrMinBy) { "nilai terkecil" }
                                else { "nilai terbesar" }));
                        }
                    }
                }
                self.eval_method(args[0].clone(), metode, vec![args[1].clone()])
            }
            ArrFlatten => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::flatten", 1, &o.type_name())) };
                let mut flat = Vec::new();
                for v in arr { match v { Value::Array(i) => flat.extend(i), o => flat.push(o) } }
                Ok(Value::Array(flat.into()))
            }
            ArrZip => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_zip_kurang()); }
                let a = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::zip", 1, &o.type_name())) };
                let b = match args[1].clone() { Value::Array(b) => b, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::zip", 2, &o.type_name())) };
                let len = a.len().min(b.len());
                Ok(Value::Array(a.into_iter().take(len).zip(b.into_iter().take(len))
                    .map(|(x, y)| Value::Tuple(vec![x, y].into())).collect()))
            }
            ArrUnique => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::unique", 1, &o.type_name())) };
                let mut seen: Vec<Value> = Vec::new();
                for v in arr { if !self.ada_eq(seen.iter(), &v)? { seen.push(v); } }
                Ok(Value::Array(seen.into()))
            }
            ArrSort => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::sort", 1, &o.type_name())) };
                // Satu pintu perbandingan yang sama dengan method `.sort()`
                // (lihat `Interpreter::sort_checked`), termasuk soal nilai
                // yang tidak sebanding jadi error, bukan diam-diam "sama".
                Ok(Value::Array(Interpreter::sort_checked(arr.ambil(), false, "arr::sort")?.into()))
            }
            ArrSortBy => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::sort_by")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::sort_by", 1, &o.type_name())) };
                let f = args[1].clone();
                // Komentar di sini dulu berbunyi "sama seperti method `.sort_by`" — dan itu
                // TIDAK benar: bentuk method menolak fungsi satu parameter, dengan pesan …
                if let Some(1) = self.arity_of(&f) {
                    return runtime_err!("{}", crate::interpreter::pesan_sort_by_satu_param());
                }
                self.sort_with_comparator_public(arr.ambil(), f)
            }
            ArrReverse => {
                let mut arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::reverse", 1, &o.type_name())) };
                arr.reverse(); Ok(Value::Array(arr))
            }
            ArrSum => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::sum", 1, &o.type_name())) };
                // Divalidasi dulu, TIDAK dikonversi: `numeric_vec`
                // mengembalikan `f64`, dan memakainya di sini akan
                // membuat `arr::sum([1,2,3])` menjawab `6.0` alih-alih `6`.
                for v in arr.iter() {
                    if !matches!(v, Value::Int(_) | Value::Float(_)) {
                        return runtime_err!("{}",
                            crate::interpreter::pesan_elemen_bukan_angka(
                                "arr::sum", &v.type_name()));
                    }
                }
                let mut t = Value::Int(0);
                for v in arr { t = self.v_add(t, v)?; }
                Ok(t)
            }
            ArrProduct => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::product", 1, &o.type_name())) };
                Interpreter::cek_semua_angka(&arr, "arr::product")?;
                let mut t = Value::Int(1);
                for v in arr { t = self.v_mul(t, v)?; }
                Ok(t)
            }
            ArrMean => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::mean", 1, &o.type_name())) };
                Interpreter::stat_mean(&arr)
            }
            ArrMedian => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::median", 1, &o.type_name())) };
                Interpreter::stat_median(&arr)
            }
            ArrVariance => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::variance", 1, &o.type_name())) };
                Interpreter::stat_variance(&arr)
            }
            ArrStdDev => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::std_dev", 1, &o.type_name())) };
                Interpreter::stat_std_dev(&arr)
            }
            ArrMin => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::min", 1, &o.type_name())) };
                Interpreter::array_min_max(&arr, false, "arr::min")
            }
            ArrMax => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::max", 1, &o.type_name())) };
                Interpreter::array_min_max(&arr, true, "arr::max")
            }
            ArrContains => {
                if args.len() < 2 { return runtime_err!("arr::contains butuh (arr, val)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::contains", 1, &o.type_name())) };
                let t = args[1].clone();
                Ok(Value::Bool(self.ada_eq(arr.iter(), &t)?))
            }
            ArrIndexOf => {
                if args.len() < 2 { return runtime_err!("arr::index_of butuh (arr, val)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::index_of", 1, &o.type_name())) };
                let t = args[1].clone();
                Ok(match self.posisi_eq(arr.iter(), &t)? {
                    Some(i) => crate::interpreter::some_value(Value::Int(i as i128)),
                    None => crate::interpreter::none_value(),
                })
            }
            ArrJoin => super::str_ops::gabung("arr::join", args),
            ArrPush => {
                if args.len() < 2 { return runtime_err!("arr::push butuh (arr, val)"); }
                let mut arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::push", 1, &o.type_name())) };
                arr.push(args[1].clone());
                Ok(Value::Array(arr))
            }
            // Konsisten dengan `.pop()`: `Option`, bukan `nil` polos.
            ArrPop => {
                let mut arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::pop", 1, &o.type_name())) };
                let val = match arr.pop() {
                    Some(v) => crate::interpreter::some_value(v),
                    None    => crate::interpreter::none_value(),
                };
                Ok(Value::Tuple(vec![Value::Array(arr), val].into()))
            }
            ArrInsert => {
                if args.len() < 3 { return runtime_err!("arr::insert butuh (arr, idx, val)"); }
                let mut arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::insert", 1, &o.type_name())) };
                let ix = indeks_untuk("arr::insert", &args[1], arr.len(), true)?;
                arr.insert(ix, args[2].clone());
                Ok(Value::Array(arr))
            }
            ArrRemove => {
                if args.len() < 2 { return runtime_err!("arr::remove butuh (arr, idx)"); }
                let mut arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::remove", 1, &o.type_name())) };
                let ix = indeks_untuk("arr::remove", &args[1], arr.len(), false)?;
                let val = arr.remove(ix);
                Ok(Value::Tuple(vec![Value::Array(arr), val].into()))
            }
            ArrTake => {
                if args.len() < 2 { return runtime_err!("arr::take butuh (arr, n)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::take", 1, &o.type_name())) };
                // `to_usize` berbunyi "index tidak boleh negatif" — dan ini
                // sebuah JUMLAH, bukan index. Pembacanya dikirim mencari
                // indexing yang tidak ada di programnya.
                let n = {
                    let v = bulat_untuk("arr::take", &args[1])?;
                    if v < 0 {
                        return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal(
                            "arr::take", "jumlah", v));
                    }
                    // SATURASI, bukan `as usize`: `as` MEMBUNGKUS, jadi
                    // `take(18446744073709551617)` menjawab `[1]` alih-alih seluruh array.
                    usize::try_from(v).unwrap_or(usize::MAX)
                };
                Ok(Value::Array(arr.iter().take(n).cloned().collect()))
            }
            ArrSkip => {
                if args.len() < 2 { return runtime_err!("arr::skip butuh (arr, n)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::skip", 1, &o.type_name())) };
                // `to_usize` berbunyi "index tidak boleh negatif" — dan ini
                // sebuah JUMLAH, bukan index. Pembacanya dikirim mencari
                // indexing yang tidak ada di programnya.
                let n = {
                    let v = bulat_untuk("arr::skip", &args[1])?;
                    if v < 0 {
                        return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal(
                            "arr::skip", "jumlah", v));
                    }
                    // SATURASI, bukan `as usize`: `as` MEMBUNGKUS, jadi
                    // `take(18446744073709551617)` menjawab `[1]` alih-alih seluruh array.
                    usize::try_from(v).unwrap_or(usize::MAX)
                };
                Ok(Value::Array(arr.into_iter().skip(n).collect()))
            }
            ArrChunks => {
                if args.len() < 2 { return runtime_err!("arr::chunks butuh (arr, size)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::chunks", 1, &o.type_name())) };
                let n = bulat_untuk("arr::chunks", &args[1])?;
                // `.max(1)` dulu di sini: ukuran 0 diam-diam jadi 1, jadi
                // `arr::chunks(a, 0)` menjawab potongan berisi SATU elemen.
                if n < 1 {
                    return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal(
                        "arr::chunks", "ukuran", n));
                }
                // SATURASI, bukan `as usize`: `as` memotong 2^64 jadi 0.
                let size = usize::try_from(n).unwrap_or(usize::MAX);
                Ok(Value::Array(arr.chunks(size).map(|c| Value::Array(c.to_vec().into())).collect()))
            }
            ArrWindows => {
                if args.len() < 2 { return runtime_err!("arr::windows butuh (arr, size)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::windows", 1, &o.type_name())) };
                let n = bulat_untuk("arr::windows", &args[1])?;
                // `.max(1)` dulu di sini: ukuran 0 diam-diam jadi 1, jadi
                // `arr::windows(a, 0)` menjawab potongan berisi SATU elemen.
                if n < 1 {
                    return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal(
                        "arr::windows", "ukuran", n));
                }
                // SATURASI, bukan `as usize`: `as` memotong 2^64 jadi 0.
                let size = usize::try_from(n).unwrap_or(usize::MAX);
                Ok(Value::Array(arr.windows(size).map(|w| Value::Array(w.to_vec().into())).collect()))
            }
            ArrRepeat => {
                if args.len() < 2 { return runtime_err!("arr::repeat butuh (arr, n)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::repeat", 1, &o.type_name())) };
                // `to_usize` berbunyi "index tidak boleh negatif" — dan ini
                // sebuah JUMLAH, bukan index. Pembacanya dikirim mencari
                // indexing yang tidak ada di programnya.
                let n = {
                    let v = bulat_untuk("arr::repeat", &args[1])?;
                    if v < 0 {
                        return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal(
                            "arr::repeat", "jumlah", v));
                    }
                    // SATURASI, bukan `as usize`: `as` MEMBUNGKUS, jadi
                    // `take(18446744073709551617)` menjawab `[1]` alih-alih seluruh array.
                    crate::interpreter::cek_batas_elemen("arr::repeat", v)
                        .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                    usize::try_from(v).unwrap_or(usize::MAX)
                };
                // Yang dialokasikan TOTALnya: `arr::repeat(range(0, 1000), 16000000)` dulu
                // lolos batas `n` lalu mematikan prosesnya.
                crate::interpreter::cek_batas_elemen("arr::repeat",
                    (arr.len() as i128).saturating_mul(n as i128))
                    .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                let mut r = Vec::with_capacity(arr.len() * n);
                for _ in 0..n { r.extend(arr.clone()); }
                Ok(Value::Array(r.into()))
            }
            ArrFill => {
                if args.len() < 2 { return runtime_err!("arr::fill butuh (val, n)"); }
                // `to_usize` berbunyi "index tidak boleh negatif" — dan ini
                // sebuah JUMLAH, bukan index. Pembacanya dikirim mencari
                // indexing yang tidak ada di programnya.
                let n = {
                    let v = bulat_untuk("arr::fill", &args[1])?;
                    if v < 0 {
                        return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal(
                            "arr::fill", "jumlah", v));
                    }
                    // SATURASI, bukan `as usize`: `as` MEMBUNGKUS, jadi
                    // `take(18446744073709551617)` menjawab `[1]` alih-alih seluruh array.
                    crate::interpreter::cek_batas_elemen("arr::fill", v)
                        .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                    usize::try_from(v).unwrap_or(usize::MAX)
                };
                Ok(Value::Array(vec![args[0].clone(); n].into()))
            }
            ArrEnumerate if args.is_empty() => runtime_err!("{}",
                crate::interpreter::pesan_butuh_argumen("arr::enumerate", 1, "(arr)", 0)),
            ArrEnumerate => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::enumerate", 1, &o.type_name())) };
                Ok(Value::Array(arr.into_iter().enumerate()
                    .map(|(i, v)| Value::Tuple(vec![Value::Int(i as i128), v].into()))
                    .collect()))
            }
            // Konsisten dengan method `.first()`/`.last()`: `Option`, bukan
            // `nil` polos yang tidak bisa dibedakan dari "elemennya
            // memang nil".
            ArrFirst => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::first", 1, &o.type_name())) };
                Ok(match arr.first().cloned() {
                    Some(v) => crate::interpreter::some_value(v),
                    None    => crate::interpreter::none_value(),
                })
            }
            ArrLast => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::last", 1, &o.type_name())) };
                Ok(match arr.last().cloned() {
                    Some(v) => crate::interpreter::some_value(v),
                    None    => crate::interpreter::none_value(),
                })
            }
            ArrIsEmpty => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::is_empty", 1, &o.type_name())) };
                Ok(Value::Bool(arr.is_empty()))
            }
            ArrConcat => {
                if args.len() < 2 { return runtime_err!("arr::concat butuh (arr1, arr2)"); }
                let mut a = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::concat", 1, &o.type_name())) };
                let b = match args[1].clone() { Value::Array(b) => b, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::concat", 2, &o.type_name())) };
                a.extend(b);
                Ok(Value::Array(a))
            }
            // Turunan `[...x]`/`f(...x)`: kirinya selalu buatan parser, jadi yang bisa salah cuma
            // potongan yang disebar — dan pesannya menyebut `...`, bukan nama turunan ini.
            ArrSebar => {
                let mut kiri = match args.first().cloned().unwrap_or(Value::Nil) {
                    Value::Array(a) => a,
                    o => return runtime_err!("{}", crate::interpreter::pesan_sebar_bukan_array(&o.type_name())),
                };
                let kanan = match args.get(1).cloned().unwrap_or(Value::Nil) {
                    Value::Array(b) => b,
                    o => return runtime_err!("{}", crate::interpreter::pesan_sebar_bukan_array(&o.type_name())),
                };
                if kiri.is_empty() { return Ok(Value::Array(kanan)); }
                kiri.extend(kanan);
                Ok(Value::Array(kiri))
            }
            ArrFlatMap => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::flat_map")); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::flat_map", 1, &o.type_name())) };
                let f = args[1].clone();
                let mut out = Vec::new();
                for v in arr {
                    match self.call_fn(f.clone(), vec![v])? {
                        Value::Array(inner) => out.extend(inner),
                        other => out.push(other),
                    }
                }
                Ok(Value::Array(out.into()))
            }
            ArrGroupBy => {
                if args.len() < 2 { return runtime_err!("arr::group_by butuh (arr, fn) — fn(elem) balikin key grup-nya"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::group_by", 1, &o.type_name())) };
                let f = args[1].clone();
                let mut fields: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
                for v in arr {
                    let key = format!("{}", self.call_fn(f.clone(), vec![v.clone()])?);
                    match fields.get_mut(&key) {
                        Some(Value::Array(bucket)) => bucket.push(v),
                        _ => { fields.insert(key, Value::Array(vec![v].into())); }
                    }
                }
                Ok(Value::Struct { name: super::dict::DICT_TYPE_NAME.to_string(), fields: fields.into() })
            }
            ArrPartition => {
                if args.len() < 2 { return runtime_err!("arr::partition butuh (arr, fn) — balikin (cocok, tidak_cocok)"); }
                let arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::partition", 1, &o.type_name())) };
                let f = args[1].clone();
                let (mut yes, mut no) = (Vec::new(), Vec::new());
                for v in arr {
                    let r = self.call_fn(f.clone(), vec![v.clone()])?;
                    if self.truthy(&r) { yes.push(v); } else { no.push(v); }
                }
                Ok(Value::Tuple(vec![Value::Array(yes.into()), Value::Array(no.into())].into()))
            }
            ArrZipWith => {
                if args.len() < 3 { return runtime_err!("{}", crate::interpreter::pesan_hof_kurang("arr::zip_with")); }
                let a = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::zip_with", 1, &o.type_name())) };
                let b = match args[1].clone() { Value::Array(b) => b, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::zip_with", 2, &o.type_name())) };
                let f = args[2].clone();
                let mut out = Vec::with_capacity(a.len().min(b.len()));
                for (x, y) in a.into_iter().zip(b.into_iter()) {
                    out.push(self.call_fn(f.clone(), vec![x, y])?);
                }
                Ok(Value::Array(out.into()))
            }
            ArrShuffle => {
                let arr = match fst(args) { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::shuffle", 1, &o.type_name())) };
                let mut idxs: Vec<usize> = (0..arr.len()).collect();
                fisher_yates_shuffle(&mut idxs);
                Ok(Value::Array(idxs.into_iter().map(|i| arr[i].clone()).collect()))
            }
            ArrSample => {
                let arr = match args.first().cloned() { Some(Value::Array(a)) => a, Some(o) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::sample", 1, &o.type_name())), None => return runtime_err!("arr::sample butuh (arr, n?)") };
                // `n` boleh hilang, bawaannya 1 — mengikuti bentuk METHOD, yang dikunci
                // kasus `PARITAS` "shuffle dan sample punya BENTUK yang sama": keduanya …
                let n = match args.get(1) {
                    Some(v) => cacah_untuk("arr::sample", v)?.min(arr.len()),
                    None => 1.min(arr.len()),
                };
                let mut idxs: Vec<usize> = (0..arr.len()).collect();
                fisher_yates_shuffle(&mut idxs);
                Ok(Value::Array(idxs.into_iter().take(n).map(|i| arr[i].clone()).collect()))
            }
            ArrSwap => {
                if args.len() < 3 { return runtime_err!("arr::swap butuh (arr, i, j)"); }
                let mut arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::swap", 1, &o.type_name())) };
                // Yang disebut index yang BENAR-BENAR di luar batas, apa adanya.
                let i = indeks_untuk("arr::swap", &args[1], arr.len(), false)?;
                let j = indeks_untuk("arr::swap", &args[2], arr.len(), false)?;
                arr.swap(i, j);
                Ok(Value::Array(arr))
            }
            ArrRotate => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_rotate_kurang()); }
                let mut arr = match args[0].clone() { Value::Array(a) => a, o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::rotate", 1, &o.type_name())) };
                if arr.is_empty() { return Ok(Value::Array(arr)); }
                let n = match &args[1] {
                    Value::Int(n) => *n,
                    o => return runtime_err!("{}",
                        crate::interpreter::pesan_rotate_n(&o.type_name())),
                };
                let len = arr.len() as i128;
                let shift = ((n % len) + len) % len;
                arr.rotate_left(shift as usize);
                Ok(Value::Array(arr))
            }
            _ => unreachable!("kind {:?} lolos filter dispatch_arr tapi tak ada arm-nya di dispatch_arr_inner", kind),
        }
    }
}

// ============================================================================
// Jalur KONSUMSI — transformer array yang mengambil alih penyimpanannya …
impl Interpreter {
    /// Layani transformer array yang bisa mengubah di tempat, dengan argumen yang
    /// DIMILIKI.
    pub(crate) fn arr_konsumsi(
        &mut self,
        kind: &NativeFnKind,
        args: Vec<Value>,
    ) -> Result<EvalResult, Vec<Value>> {
        use NativeFnKind::*;
        // Diperiksa DULU, sebelum `args` disentuh — kalau bukan milik kita,
        // argumennya harus kembali utuh.
        if !matches!(kind, ArrPush | ArrPop | ArrInsert | ArrRemove | ArrConcat | ArrReverse) {
            return Err(args);
        }
        Ok(self.arr_konsumsi_inner(kind, args))
    }

    fn arr_konsumsi_inner(&mut self, kind: &NativeFnKind, args: Vec<Value>) -> EvalResult {
        use NativeFnKind::*;
        let n = args.len();
        let mut it = args.into_iter();
        let pertama = it.next().unwrap_or(Value::Nil);

        let nama = match kind {
            ArrPush => "arr::push", ArrPop => "arr::pop", ArrInsert => "arr::insert",
            ArrRemove => "arr::remove", ArrConcat => "arr::concat", _ => "arr::reverse",
        };
        let butuh = match kind { ArrPush => 2, ArrInsert => 3, ArrRemove => 2, ArrConcat => 2, _ => 1 };
        if n < butuh {
            return match kind {
                ArrPush   => runtime_err!("arr::push butuh (arr, val)"),
                ArrInsert => runtime_err!("arr::insert butuh (arr, idx, val)"),
                ArrRemove => runtime_err!("arr::remove butuh (arr, idx)"),
                ArrConcat => runtime_err!("arr::concat butuh (arr1, arr2)"),
                _         => runtime_err!("{}: butuh sebuah array", nama),
            };
        }
        let mut arr = match pertama {
            Value::Array(a) => a,
            o => return match kind {
                ArrPop     => runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::pop", 1, &o.type_name())),
                ArrReverse => runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::reverse", 1, &o.type_name())),
                _          => runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                    nama, 1, &o.type_name())),
            },
        };

        match kind {
            ArrPush   => Ok(Self::arr_pasti(Pasti::Push, arr, it.next())),
            ArrPop    => Ok(Self::arr_pasti(Pasti::Pop, arr, None)),
            ArrReverse=> Ok(Self::arr_pasti(Pasti::Reverse, arr, None)),
            ArrConcat => match it.next().unwrap_or(Value::Nil) {
                v @ Value::Array(_) => Ok(Self::arr_pasti(Pasti::Concat, arr, Some(v))),
                o => runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("arr::concat", 2, &o.type_name())),
            },
            ArrInsert => {
                let idx_v = it.next().unwrap_or(Value::Nil);
                let val   = it.next().unwrap_or(Value::Nil);
                let ix = indeks_untuk("arr::insert", &idx_v, arr.len(), true)?;
                arr.insert(ix, val);
                Ok(Value::Array(arr))
            }
            _ => {
                let idx_v = it.next().unwrap_or(Value::Nil);
                let ix = indeks_untuk("arr::remove", &idx_v, arr.len(), false)?;
                let val = arr.remove(ix);
                Ok(Value::Tuple(vec![Value::Array(arr), val].into()))
            }
        }
    }

    /// Empat operasi yang TIDAK BISA GAGAL setelah penjagaannya lolos:
    /// penerimanya array, arity-nya cukup, dan (untuk `concat`) argumennya juga …
    pub(crate) fn arr_pasti(
        op: Pasti,
        mut arr: crate::interpreter::value::Isi,
        arg: Option<Value>,
    ) -> Value {
        match op {
            Pasti::Push => {
                arr.push(arg.unwrap_or(Value::Nil));
                Value::Array(arr)
            }
            Pasti::Pop => {
                let v = match arr.pop() {
                    Some(v) => crate::interpreter::some_value(v),
                    None    => crate::interpreter::none_value(),
                };
                Value::Tuple(vec![Value::Array(arr), v].into())
            }
            Pasti::Reverse => { arr.reverse(); Value::Array(arr) }
            Pasti::Concat => {
                if let Some(Value::Array(lain)) = arg {
                    // `ambil()` gratis kalau kita pemegang terakhir isinya.
                    arr.extend(lain.ambil());
                }
                Value::Array(arr)
            }
        }
    }

    /// Versi method (`a.push(x)`) — penerimanya DIMILIKI, bukan dipinjam.
    pub(crate) fn arr_method_konsumsi(
        &mut self,
        method: &str,
        mut arr: crate::interpreter::value::Isi,
        args: Vec<Value>,
    ) -> EvalResult {
        let mut it = args.into_iter();
        match method {
            "push" => {
                let Some(v) = it.next() else { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "push", 0)) };
                Ok(Self::arr_pasti(Pasti::Push, arr, Some(v)))
            }
            "pop" => Ok(Self::arr_pasti(Pasti::Pop, arr, None)),
            "insert" => {
                let (Some(i), Some(v)) = (it.next(), it.next()) else {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "insert", 0));
                };
                let idx = indeks_untuk("arr::insert", &i, arr.len(), true)?;
                arr.insert(idx, v);
                Ok(Value::Array(arr))
            }
            "remove" => {
                let Some(i) = it.next() else { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "remove", 0)) };
                let idx = indeks_untuk("arr::remove", &i, arr.len(), false)?;
                let v = arr.remove(idx);
                Ok(Value::Tuple(vec![Value::Array(arr), v].into()))
            }
            "concat" => match it.next() {
                Some(v @ Value::Array(_)) => Ok(Self::arr_pasti(Pasti::Concat, arr, Some(v))),
                // Argumen yang ADA tapi bukan array adalah kesalahan TIPE.
                // Menjawabnya dengan kalimat JUMLAH menyuruh pembacanya
                // menghitung argumen yang jumlahnya sudah benar.
                Some(lain) => runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                    "arr::concat", 2, &lain.type_name())),
                None => runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                    "arr", "concat", 0)),
            },
            "swap" => {
                let (Some(a), Some(b)) = (it.next(), it.next()) else {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "swap", 0));
                };
                let (i, j) = (indeks_untuk("arr::swap", &a, arr.len(), false)?, indeks_untuk("arr::swap", &b, arr.len(), false)?);
                arr.swap(i, j);
                Ok(Value::Array(arr))
            }
            _ => Ok(Self::arr_pasti(Pasti::Reverse, arr, None)),
        }
    }
}

/// Operasi array yang tidak punya jalur gagal. Lihat `arr_pasti`.
#[derive(Clone, Copy)]
pub(crate) enum Pasti { Push, Pop, Reverse, Concat }
