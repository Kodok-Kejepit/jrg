// src/interpreter/metode.rs — resolusi dan dispatch method

use std::collections::HashMap;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TemplatePart, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_new, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
    enum_payload, enum_parts,
};
use crate::interpreter::builtin::{NativeFnKind, fisher_yates_shuffle};
use crate::runtime_err;
use crate::interpreter::eval_expr::{nama_tipe_sasaran, nilai_ke_usize, nilai_ke_i128, nilai_ke_f64, cacah_untuk, indeks_untuk, indeks_opsional};

impl Interpreter {
    /// Cari metode milik pengguna untuk sebuah nama tipe.
    pub(crate) fn resolusi_metode(&self, tname: &str, method: &str) -> Option<std::sync::Arc<JrgFunction>> {
        if let Some(func) = self.methods.find_instance(tname, method) {
            return Some(func.clone());
        }
        let mut current = tname.to_string();
        let mut visited: Vec<String> = Vec::new();
        let mut traits_in_chain: Vec<String> = Vec::new();
        // Yang ketemu lewat warisan atau default trait berjalan "di dalam" tipe
        // PENERIMA, seperti salinan per tipe anak yang dipasang VM.
        let di_penerima = |f: &JrgFunction| std::sync::Arc::new(JrgFunction { ruang: Some(std::sync::Arc::new(tname.to_string())), ..f.clone() });
        loop {
            if let Some(func) = self.methods.find_instance(&current, method) {
                return Some(di_penerima(func));
            }
            if let Some(ts) = self.type_traits.get(&current) {
                for t in ts { if !traits_in_chain.contains(t) { traits_in_chain.push(t.clone()); } }
            }
            visited.push(current.clone());
            match self.struct_defs.get(&current).and_then(|d| d.parent.clone()) {
                Some(p) if !visited.contains(&p) && visited.len() < 64 => current = p,
                _ => break,
            }
        }
        for tr in &traits_in_chain {
            if let Some(func) = self.trait_defaults.get(tr).and_then(|m| m.get(method)) {
                return Some(di_penerima(func));
            }
        }
        None
    }

    pub(crate) fn eval_method(&mut self, obj: Value, method: &str, args: Vec<Value>) -> EvalResult {
        // Step 1: user-defined instance methods from impl blocks.
        // INHERITANCE: kalau tipenya sendiri tidak punya method ini, `resolusi_metode`
        // menelusuri rantai `struct Dog: Animal` ke atas.
        let milik = match &obj {
            Value::Struct { name, .. } => self.resolusi_metode(name, method),
            Value::Enum { type_name, .. } => self.resolusi_metode(type_name, method),
            _ => None,
        };
        if let Some(func) = milik {
            let mut full_args = Vec::with_capacity(args.len() + 1);
            full_args.push(obj);
            full_args.extend(args);
            return self.call_jrg(func, full_args);
        }
        // Step 1b: handle asing (`df.head(5)`, `tgl.format("Y-m-d")`) diteruskan ke
        // worker-nya, lengkap dengan kemampuan memanggil balik (`df.apply(f)`).
        if matches!(obj, Value::PyObj(_) | Value::JsObj(_) | Value::PhpObj(_)) {
            let mut panggil = |f: &Value, a: Vec<Value>| -> Result<Value, String> {
                match self.call_fn(f.clone(), a) {
                    Ok(v) | Err(Signal::Return(v)) => Ok(v),
                    Err(e) => Err(crate::interpreter::builtin::ffi_pintu::pesan_sinyal(e)),
                }
            };
            return Ok(crate::interpreter::builtin::ffi_pintu::metode_handle(&obj, method, args, &mut panggil)
                .unwrap_or(Value::Nil));
        }

        // Step 2: built-in methods
        self.builtin_method(obj, method, args)
    }

    pub(crate) fn builtin_method(&mut self, obj: Value, method: &str, args: Vec<Value>) -> EvalResult {
        // Method HOF bawaan: jumlah argumennya diperiksa di SATU pintu, kurang maupun lebih —
        // dulu argumen berlebih diterima diam-diam (`[1].map(f, 5)` menjawab `[1]`).
        if let Some((n, _)) = crate::interpreter::bentuk_hof_method(method) {
            let punya = match &obj {
                Value::Array(_) => !matches!(method, "map_err" | "and_then"),
                Value::Enum { variant, .. } => match method {
                    "map" | "and_then" => matches!(variant.as_str(), "Some" | "None" | "Ok" | "Err"),
                    "map_err" => matches!(variant.as_str(), "Ok" | "Err"),
                    _ => false,
                },
                _ => false,
            };
            if punya && args.len() != n {
                return runtime_err!("{}", crate::interpreter::pesan_hof_method_argumen(method, args.len()));
            }
        }
        // Method skalar yang dilayani `math::` lewat `math::` yang sama, seperti
        // di VM. Salinan lengan di sini dulu memotong eksponen `pow`
        // (`(2.0).pow(4294967296)` menjawab `1.0`) dan membuang argumen lebih.
        if let Some(tipe) = match &obj {
            Value::Int(_) => Some("int"),
            Value::Float(_) => Some("float"),
            _ => None,
        } {
            if crate::interpreter::metode_skalar_math(tipe, method) {
                if let Some(p) = crate::interpreter::pesan_arg_metode_skalar(tipe, method, args.len()) {
                    return runtime_err!("{}", p);
                }
                let Some(jenis) = crate::interpreter::builtin::jenis_metode_math(method) else {
                    return runtime_err!("method `{}` tidak punya fungsi `math::`", method);
                };
                // Tanpa argumen (hampir semuanya) tidak ada alokasi: jalur panas.
                let (tunggal, semua);
                let irisan: &[Value] = if args.is_empty() {
                    tunggal = [obj];
                    &tunggal
                } else {
                    let mut v = Vec::with_capacity(args.len() + 1);
                    v.push(obj);
                    v.extend(args);
                    semua = v;
                    &semua
                };
                return match jenis {
                    NativeFnKind::DariStdlibVm(n) =>
                        crate::interpreter::builtin::jembatan_stdlib::panggil(n, irisan),
                    k => Self::dispatch_math_lepas(&k, irisan),
                };
            }
        }
        // Method bawaan: jumlah argumennya — kurang MAUPUN lebih — diperiksa di
        // SATU pintu, dari definisi yang sama yang dipakai VM di `GetMethod`.
        // Yang lebih dulu dibuang diam-diam di dua mesin (`[1,2].len(5)`).
        if let Some(ns) = match &obj {
            Value::Array(_) => Some("arr"),
            Value::Str(_) => Some("str"),
            Value::Channel(_) => Some("chan"),
            Value::Canvas(_) => Some("canvas"),
            Value::Sinkron(p) => p.ns_metode(),
            Value::Struct { name, .. } if name == crate::interpreter::builtin::dict::DICT_TYPE_NAME
                => Some("dict"),
            Value::Enum { type_name, .. } if type_name == "Option" || type_name == "Result"
                => Some(type_name.as_str()),
            // `to_string` bawaan tidak menerima argumen, apa pun penerimanya;
            // hook `to_string` milik pemakai sudah dipilih sebelum pintu ini.
            _ if method == "to_string" => Some("*"),
            _ => None,
        } {
            if let Some(p) = crate::interpreter::pesan_jumlah_arg_metode(ns, method, args.len()) {
                return runtime_err!("{}", p);
            }
        }

        // Pegangan `Atomic`/`Mutex`: method-nya `atomic::nama`/`mutex::nama` dengan penerima di depan.
        if let Value::Sinkron(p) = &obj {
            if let Some(ns) = p.ns_metode().filter(|ns| crate::sinkron::metode_ada(ns, method)) {
                let mut semua = Vec::with_capacity(args.len() + 1);
                semua.push(obj);
                semua.extend(args);
                if method == "with_lock" { return self.mutex_with_lock(&semua); }
                return crate::interpreter::builtin::jembatan_stdlib::panggil(
                    &format!("{}::{}", ns, method), &semua);
            }
        }

        // Metode array yang MENGUBAH isi dilayani sebelum match utama.
        if matches!(&obj, Value::Array(_))
            && matches!(method, "push" | "pop" | "insert" | "remove" | "concat" | "swap" | "reverse")
        {
            let Value::Array(arr) = obj else { unreachable!("baru saja diperiksa di atas") };
            return self.arr_method_konsumsi(method, arr, args);
        }

        // String built-in method: satu definisi, di `metode_str`, karena VM
        // memanggil yang sama.
        if let Value::Str(s) = &obj {
            if let Some(r) = Self::metode_str(s, method, &args) { return r; }
        }

        match (&obj, method) {
            // String built-in methods: satu definisi, di `metode_str`,
            // karena VM memanggil yang sama.

            // -----------------------------------------------------------
            // Array built-in methods (including HOF)
            // -----------------------------------------------------------
            (Value::Array(a), "len")      => Ok(Value::Int(a.len() as i128)),
            (Value::Array(a), "is_empty") => Ok(Value::Bool(a.is_empty())),
            // `.first()`/`.last()`/`.get(i)` mengembalikan `Option`, BUKAN `nil` polos.
            (Value::Array(a), "first") => Ok(match a.first() {
                Some(v) => some_value(v.clone()),
                None    => none_value(),
            }),
            (Value::Array(a), "last") => Ok(match a.last() {
                Some(v) => some_value(v.clone()),
                None    => none_value(),
            }),
            (Value::Array(a), "get") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "get", args.len()));
                }
                let i = indeks_opsional("arr::get", &args[0], a.len())?;
                Ok(match i.and_then(|i| a.get(i)) {
                    Some(v) => some_value(v.clone()),
                    None    => none_value(),
                })
            }
            (Value::Array(a), "get_or") => {
                // Kedua argumennya wajib: tanpa indeks pertanyaannya tidak
                // lengkap, dan tanpa cadangan `get_or` cuma `get` yang
                // menjawab `nil` — bentuk yang justru dihapus dari bahasa ini.
                if args.len() < 2 {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "get_or", args.len()));
                }
                let i = indeks_opsional("arr::get_or", &args[0], a.len())?;
                let bawaan = args.get(1).cloned().unwrap_or(Value::Nil);
                Ok(i.and_then(|i| a.get(i)).cloned().unwrap_or(bawaan))
            }
            (Value::Array(a), "contains") => {
                // Argumennya WAJIB.
                let Some(t) = args.first() else {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "contains", args.len()));
                };
                Ok(Value::Bool(self.ada_eq(a.iter(), t)?))
            }
            (Value::Array(a), "index_of") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "index_of", args.len()));
                }
                let t = args.first().cloned().unwrap_or(Value::Nil);
                Ok(match self.posisi_eq(a.iter(), &t)? {
                    Some(i) => crate::interpreter::some_value(Value::Int(i as i128)),
                    None => crate::interpreter::none_value(),
                })
            }
            (Value::Array(a), "join") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "join", args.len()));
                }
                // Satu definisi dengan `arr::join` di dua mesin (`str_ops::gabung`).
                let mut semua = vec![Value::Array(a.clone())];
                semua.extend(args.iter().cloned());
                crate::interpreter::builtin::str_ops::gabung("arr::join", &semua)
            }
            (Value::Array(a), "reverse") => {
                let mut r = a.clone(); r.reverse(); Ok(Value::Array(r))
            }
            (Value::Array(a), "sort") =>
                Ok(Value::Array(Self::sort_checked(a.to_vec(), false, "arr::sort")?.into())),
            (Value::Array(a), "unique") => {
                let mut seen: Vec<Value> = Vec::new();
                for v in a {
                    if !self.ada_eq(seen.iter(), v)? { seen.push(v.clone()); }
                }
                Ok(Value::Array(seen.into()))
            }
            (Value::Array(a), "flatten") => {
                let mut flat = Vec::new();
                for v in a {
                    match v { Value::Array(i) => flat.extend(i.clone()), o => flat.push(o.clone()) }
                }
                Ok(Value::Array(flat.into()))
            }
            // Keenamnya lewat definisi yang SAMA dengan bentuk `arr::`-nya.
            (Value::Array(a), "sum") => {
                Self::cek_semua_angka(a, "arr::sum")?;
                let mut t = Value::Int(0);
                for v in a { t = self.v_add(t, v.clone())?; }
                Ok(t)
            }
            (Value::Array(a), "product") => {
                Self::cek_semua_angka(a, "arr::product")?;
                let mut t = Value::Int(1);
                for v in a { t = self.v_mul(t, v.clone())?; }
                Ok(t)
            }
            (Value::Array(a), "mean")     => Self::stat_mean(a),
            (Value::Array(a), "median")   => Self::stat_median(a),
            (Value::Array(a), "variance") => Self::stat_variance(a),
            (Value::Array(a), "std_dev")  => Self::stat_std_dev(a),
            (Value::Array(a), "min") => Self::array_min_max(a, false, "arr::min"),
            (Value::Array(a), "max") => Self::array_min_max(a, true,  "arr::max"),
            (Value::Array(a), "take") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "take", args.len()));
                }
                let n = args.first().map(|v| cacah_untuk("arr::take", v)).transpose()?.unwrap_or(0);
                Ok(Value::Array(a.iter().take(n).cloned().collect()))
            }
            // Nama yang DITULIS diteruskan: kalimat galatnya menyebutnya.
            (Value::Array(a), "skip") | (Value::Array(a), "drop") =>
                Self::metode_arr_lepas(a, method, &args),
            (Value::Array(a), "chunks") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "chunks", args.len()));
                }
                let size = args.first().map(|v| cacah_untuk("arr::chunks", v)).transpose()?.unwrap_or(1);
                // Dulu `size` 0 diam-diam dijepit jadi 1 (`.max(1)`), jadi
                // `arr.chunks(0)` — hampir pasti salah ketik atau salah
                // hitung — jalan terus seolah-olah `arr.chunks(1)` diminta.
                if size == 0 {
                    return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal("arr::chunks", "ukuran", 0));
                }
                Ok(Value::Array(a.chunks(size).map(|c| Value::Array(c.to_vec().into())).collect()))
            }
            (Value::Array(a), "windows") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "windows", args.len()));
                }
                let size = args.first().map(|v| cacah_untuk("arr::windows", v)).transpose()?.unwrap_or(1);
                if size == 0 {
                    return runtime_err!("{}", crate::interpreter::pesan_ukuran_tak_masuk_akal("arr::windows", "ukuran", 0));
                }
                Ok(Value::Array(a.windows(size).map(|w| Value::Array(w.to_vec().into())).collect()))
            }
            (Value::Array(a), "enumerate") => {
                Ok(Value::Array(a.iter().enumerate()
                    .map(|(i, v)| Value::Tuple(vec![Value::Int(i as i128), v.clone()].into()))
                    .collect()))
            }
            // HOF on arrays
            (Value::Array(a), "map") => {
                let func = args[0].clone(); let arr = a.clone();
                let mut result = Vec::with_capacity(arr.len());
                for v in arr { result.push(self.call_fn(func.clone(), vec![v])?); }
                Ok(Value::Array(result.into()))
            }
            (Value::Array(a), "filter") => {
                let func = args[0].clone(); let arr = a.clone();
                let mut result = Vec::new();
                for v in arr {
                    let keep = self.call_fn(func.clone(), vec![v.clone()])?;
                    if self.truthy(&keep) { result.push(v); }
                }
                Ok(Value::Array(result.into()))
            }
            (Value::Array(a), "reduce") => {
                let func = args[0].clone(); let mut acc = args[1].clone(); let arr = a.clone();
                for v in arr { acc = self.call_fn(func.clone(), vec![acc, v])?; }
                Ok(acc)
            }
            (Value::Array(a), "any") => {
                let func = args[0].clone(); let arr = a.clone();
                for v in arr {
                    let r = self.call_fn(func.clone(), vec![v])?;
                    if self.truthy(&r) { return Ok(Value::Bool(true)); }
                }
                Ok(Value::Bool(false))
            }
            (Value::Array(a), "all") => {
                let func = args[0].clone(); let arr = a.clone();
                for v in arr {
                    let r = self.call_fn(func.clone(), vec![v])?;
                    if !self.truthy(&r) { return Ok(Value::Bool(false)); }
                }
                Ok(Value::Bool(true))
            }
            // `Option`, bukan `nil` polos — dulu "tidak ketemu" dan
            // "ketemu, isinya memang nil" sama-sama menjawab `nil`.
            (Value::Array(a), "find") => {
                let func = args[0].clone(); let arr = a.clone();
                for v in arr {
                    let r = self.call_fn(func.clone(), vec![v.clone()])?;
                    if self.truthy(&r) { return Ok(some_value(v)); }
                }
                Ok(none_value())
            }
            (Value::Array(a), "find_idx") => {
                let func = args[0].clone(); let arr = a.clone();
                // `Option`, sama dengan `.find(f)` dan dengan bentuk
                // namespace-nya. `-1` adalah indeks yang SAH di JRG.
                for (i, v) in arr.into_iter().enumerate() {
                    let r = self.call_fn(func.clone(), vec![v])?;
                    if self.truthy(&r) { return Ok(some_value(Value::Int(i as i128))); }
                }
                Ok(none_value())
            }
            (Value::Array(a), "count_if") => {
                let func = args[0].clone(); let arr = a.clone();
                let mut cnt = 0i128;
                for v in arr { let r = self.call_fn(func.clone(), vec![v])?; if self.truthy(&r) { cnt += 1; } }
                Ok(Value::Int(cnt))
            }
            // `sort_by(cmp)` — `cmp(a, b)` mengembalikan angka: negatif kalau `a`
            // duluan, positif kalau `b` duluan, 0 kalau sama.
            (Value::Array(a), "sort_by") => {
                let Some(func) = args.first().cloned() else {
                    return runtime_err!("{}", crate::interpreter::pesan_hof_method_argumen("sort_by", 0));
                };
                if let Some(1) = self.arity_of(&func) {
                    return runtime_err!("{}", crate::interpreter::pesan_sort_by_satu_param());
                }
                self.sort_with_comparator(a.to_vec(), func)
            }
            (Value::Array(a), "sort_by_key") => {
                let Some(func) = args.first().cloned() else {
                    return runtime_err!("{}", crate::interpreter::pesan_hof_method_argumen("sort_by_key", 0));
                };
                self.sort_by_key_checked(a.to_vec(), func)
            }
            // ----------------------------------------------------------- Array methods
            // "full power" tambahan — semua tetap value semantics (kembalikan array …
            (Value::Array(a), "push") => {
                if args.is_empty() { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "push", args.len())); }
                let mut r = a.clone(); r.push(args[0].clone()); Ok(Value::Array(r))
            }
            // `(array_baru, Option<elemen>)` — bukan `(array_baru, nil)`
            // polos buat array kosong, yang dulu tidak bisa dibedakan
            // dari `.pop()` pada `[nil]` (sama-sama menjawab `([], nil)`).
            (Value::Array(a), "pop") => {
                let mut r = a.clone();
                let v = match r.pop() {
                    Some(v) => some_value(v),
                    None    => none_value(),
                };
                Ok(Value::Tuple(vec![Value::Array(r), v].into()))
            }
            (Value::Array(a), "insert") => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "insert", args.len())); }
                let mut r = a.clone();
                let idx = indeks_untuk("arr::insert", &args[0], r.len(), true)?;
                r.insert(idx, args[1].clone());
                Ok(Value::Array(r))
            }
            (Value::Array(a), "remove") => {
                if args.is_empty() { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "remove", args.len())); }
                let mut r = a.clone();
                let idx = indeks_untuk("arr::remove", &args[0], r.len(), false)?;
                let v = r.remove(idx);
                Ok(Value::Tuple(vec![Value::Array(r), v].into()))
            }
            (Value::Array(a), "swap") => {
                if args.len() < 2 { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "swap", args.len())); }
                let i = indeks_untuk("arr::swap", &args[0], a.len(), false)?;
                let j = indeks_untuk("arr::swap", &args[1], a.len(), false)?;
                let mut r = a.clone();
                r.swap(i, j);
                Ok(Value::Array(r))
            }
            (Value::Array(a), "fill") => {
                // Sama alasannya dengan `contains`: dulu berbawaan `nil`,
                // jadi `a.fill()` menjawab `[nil, nil, nil]` — sebuah array
                // penuh yang isinya bukan apa-apa.
                let Some(v) = args.first().cloned() else {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "fill", args.len()));
                };
                Ok(Value::Array(vec![v; a.len()].into()))
            }
            (Value::Array(a), "concat") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::concat", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "concat", 0)),
                };
                let mut r = a.clone(); r.extend(other); Ok(Value::Array(r))
            }
            (Value::Array(a), "repeat") => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "repeat", args.len()));
                }
                // Satu definisi dengan `arr::repeat`: salinan kedua di sini dulu berbatas lain,
                // jadi `[1].repeat(20000000)` jalan di acuan dan ditolak `--vm`.
                let mut semua = vec![Value::Array(a.clone())];
                semua.extend(args.iter().cloned());
                self.dispatch_arr_inner(&crate::interpreter::builtin::NativeFnKind::ArrRepeat, &semua)
            }
            (Value::Array(a), "rotate_left") => Self::metode_arr_lepas(a, "rotate_left", &args),
            (Value::Array(a), "rotate_right") => Self::metode_arr_lepas(a, "rotate_right", &args),
            (Value::Array(a), "sort_desc") =>
                Ok(Value::Array(Self::sort_checked(a.to_vec(), true, "arr::sort_desc")?.into())),
            (Value::Array(a), "dedup") => Self::metode_arr_lepas(a, "dedup", &args),
            (Value::Array(a), "binary_search") => Self::metode_arr_lepas(a, "binary_search", &args),
            // `rotate` sudah ada sebagai `arr::rotate` di dua mesin dan sebagai
            // method cuma di VM — sama seperti tujuh nama `str::` di sebelah.
            (Value::Array(a), "rotate") => {
                use crate::interpreter::builtin::NativeFnKind::ArrRotate;
                let mut v = vec![Value::Array(a.clone())];
                v.extend(args.iter().cloned());
                self.call_native(ArrRotate, v)
            }
            (Value::Array(a), "starts_with") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::starts_with", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "starts_with", 0)),
                };
                Ok(Value::Bool(a.len() >= other.len()
                    && self.semua_eq(a.iter().zip(other.iter()))?))
            }
            (Value::Array(a), "ends_with") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::ends_with", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "ends_with", 0)),
                };
                Ok(Value::Bool(a.len() >= other.len()
                    && self.semua_eq(a[a.len()-other.len()..].iter().zip(other.iter()))?))
            }
            (Value::Array(a), "union") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::union", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "union", 0)),
                };
                let mut r: Vec<Value> = Vec::new();
                for v in a.iter().chain(other.iter()) {
                    if !self.ada_eq(r.iter(), v)? { r.push(v.clone()); }
                }
                Ok(Value::Array(r.into()))
            }
            (Value::Array(a), "intersect") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::intersect", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "intersect", 0)),
                };
                let mut r: Vec<Value> = Vec::new();
                for v in a.iter() {
                    if self.ada_eq(other.iter(), v)? && !self.ada_eq(r.iter(), v)? { r.push(v.clone()); }
                }
                Ok(Value::Array(r.into()))
            }
            (Value::Array(a), "difference") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::difference", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "difference", 0)),
                };
                let mut r: Vec<Value> = Vec::new();
                for v in a.iter() {
                    if !self.ada_eq(other.iter(), v)? && !self.ada_eq(r.iter(), v)? { r.push(v.clone()); }
                }
                Ok(Value::Array(r.into()))
            }
            (Value::Array(a), "shuffle") => Self::metode_arr_lepas(a, "shuffle", &args),
            (Value::Array(a), "sample") => Self::metode_arr_lepas(a, "sample", &args),
            (Value::Array(a), "zip") => {
                let other = match args.first() {
                    Some(Value::Array(o)) => o.clone(),
                    Some(lain) => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array(
                        "arr::zip", 2, &lain.type_name())),
                    None => return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", "zip", 0)),
                };
                Ok(Value::Array(a.iter().zip(other.iter()).map(|(x, y)| Value::Tuple(vec![x.clone(), y.clone()].into())).collect()))
            }
            (Value::Array(a), "zip_with") => {
                let other = match &args[0] { Value::Array(o) => o.clone(), o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("zip_with", 1, &o.type_name())) };
                let func = args[1].clone();
                let mut result = Vec::new();
                for (x, y) in a.iter().zip(other.iter()) {
                    result.push(self.call_fn(func.clone(), vec![x.clone(), y.clone()])?);
                }
                Ok(Value::Array(result.into()))
            }
            // Lewat pintu `arr::each` yang sama, bukan salinan kedua: dulu bentuk
            // method menjawab ARRAY dan bentuk namespace menjawab `unit`, untuk
            // satu nama — dan dua mesin sepakat, jadi tak satu pun tabel melihatnya.
            (Value::Array(a), "each") => {
                use crate::interpreter::builtin::NativeFnKind::ArrEach;
                let mut v = vec![Value::Array(a.clone())];
                v.extend(args.iter().cloned());
                self.call_native(ArrEach, v)
            }
            (Value::Array(a), "flat_map") => {
                let func = args[0].clone(); let arr = a.clone();
                let mut result = Vec::new();
                for v in arr {
                    match self.call_fn(func.clone(), vec![v])? {
                        Value::Array(inner) => result.extend(inner),
                        other => result.push(other),
                    }
                }
                Ok(Value::Array(result.into()))
            }
            (Value::Array(a), "fold") => {
                let mut acc = args[0].clone(); let func = args[1].clone(); let arr = a.clone();
                for v in arr { acc = self.call_fn(func.clone(), vec![acc, v])?; }
                Ok(acc)
            }
            (Value::Array(a), "partition") => {
                let func = args[0].clone(); let arr = a.clone();
                let (mut yes, mut no) = (Vec::new(), Vec::new());
                for v in arr {
                    let r = self.call_fn(func.clone(), vec![v.clone()])?;
                    if self.truthy(&r) { yes.push(v); } else { no.push(v); }
                }
                Ok(Value::Tuple(vec![Value::Array(yes.into()), Value::Array(no.into())].into()))
            }
            // Sama: `arr::group_by` dan `iterator::group_by` sama-sama menjawab
            // dict, sementara bentuk method ini menjawab array pasangan.
            (Value::Array(a), "group_by") => {
                use crate::interpreter::builtin::NativeFnKind::ArrGroupBy;
                let mut v = vec![Value::Array(a.clone())];
                v.extend(args.iter().cloned());
                self.call_native(ArrGroupBy, v)
            }
            // Dulu perbandingan kuncinya (`key_lt`) tulisan sendiri dengan `_ => false`
            // di ujung — pasangan kunci yang tidak sebanding (mis. campur string dan …
            (Value::Array(a), "min_by") | (Value::Array(a), "max_by") => {
                // Array kosong DITOLAK, bukan dijawab `nil` telanjang — aturan yang sama
                // dengan `arr::min`/`arr::max`.
                if a.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                        &format!("arr::{}", method),
                        if method == "min_by" { "nilai terkecil" } else { "nilai terbesar" }));
                }
                let want_min = method == "min_by";
                let func = args[0].clone(); let arr = a.clone();
                let mut best = arr[0].clone();
                let mut best_key = self.call_fn(func.clone(), vec![best.clone()])?;
                for v in arr.into_iter().skip(1) {
                    let k = self.call_fn(func.clone(), vec![v.clone()])?;
                    let Some(o) = Self::cmp_values(&k, &best_key) else {
                        return runtime_err!("{}", crate::interpreter::pesan_kunci_tak_sebanding(
                            &format!("arr::{}", method), &k.type_name(), &best_key.type_name()));
                    };
                    let better = if want_min {
                        o == std::cmp::Ordering::Less
                    } else {
                        o == std::cmp::Ordering::Greater
                    };
                    if better { best = v; best_key = k; }
                }
                Ok(best)
            }
            // -----------------------------------------------------------
            // Method angka lain (`abs`, `sqrt`, `pow`, …) dilayani `math::` di pintu atas.
            (Value::Int(n),   "to_string") => Ok(Value::Str(n.to_string())),
            (Value::Float(f), "to_string") => Ok(Value::Str(format!("{}", Value::Float(*f)))),
            // ----------------------------------------------------------- Result<T, E> /
            // Option<T> methods
            (Value::Enum { variant, .. }, "is_ok")   if variant == "Ok"   => Ok(Value::Bool(true)),
            (Value::Enum { variant, .. }, "is_ok")   if variant == "Err"  => Ok(Value::Bool(false)),
            (Value::Enum { variant, .. }, "is_err")  if variant == "Err"  => Ok(Value::Bool(true)),
            (Value::Enum { variant, .. }, "is_err")  if variant == "Ok"   => Ok(Value::Bool(false)),
            (Value::Enum { variant, .. }, "is_some") if variant == "Some" => Ok(Value::Bool(true)),
            (Value::Enum { variant, .. }, "is_some") if variant == "None" => Ok(Value::Bool(false)),
            (Value::Enum { variant, .. }, "is_none") if variant == "None" => Ok(Value::Bool(true)),
            (Value::Enum { variant, .. }, "is_none") if variant == "Some" => Ok(Value::Bool(false)),

            (Value::Enum { variant, .. }, "unwrap") if variant == "Ok" || variant == "Some" => {
                let payload = enum_payload(obj);
                Ok(payload.unwrap_or(Value::Unit))
            }
            // `unwrap_err` terdaftar di VM dan tidak di sini — satu program,
            // dua nasib, dan yang menolak justru mesin acuannya.
            (Value::Enum { variant, .. }, "unwrap_err") if variant == "Err" => {
                let Value::Enum { payload, .. } = &obj else { unreachable!() };
                Ok(payload.as_ref().map(|p| (**p).clone()).unwrap_or(Value::Unit))
            }
            (Value::Enum { variant, .. }, "unwrap_err") if variant == "Ok" =>
                runtime_err!("unwrap_err() pada Result::Ok"),
            (Value::Enum { variant, .. }, "unwrap") if variant == "Err" => {
                let payload = enum_payload(obj);
                runtime_err!("unwrap() pada Result::Err: {}",
                    payload.map(|v| format!("{}", v)).unwrap_or_default())
            }
            (Value::Enum { variant, .. }, "unwrap") if variant == "None" =>
                runtime_err!("unwrap() pada Option::None"),

            (Value::Enum { variant, .. }, "expect") if variant == "Ok" || variant == "Some" => {
                let payload = enum_payload(obj);
                Ok(payload.unwrap_or(Value::Unit))
            }
            (Value::Enum { variant, .. }, "expect") if variant == "Err" || variant == "None" => {
                let msg = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    Some(other) => format!("{}", other),
                    None => "expect() gagal".to_string(),
                };
                runtime_err!("{}", msg)
            }

            (Value::Enum { variant, .. }, "unwrap_or") if variant == "Ok" || variant == "Some" => {
                let payload = enum_payload(obj);
                Ok(payload.unwrap_or(Value::Unit))
            }
            (Value::Enum { variant, .. }, "unwrap_or") if variant == "Err" || variant == "None" =>
                Ok(args.into_iter().next().unwrap_or(Value::Unit)),

            // `ok()` converts a Result into an Option, discarding the
            // error: `Ok(x) -> Some(x)`, `Err(_) -> None`. On an Option
            // value itself, `ok()` is the identity (already an Option).
            (Value::Enum { variant, .. }, "ok") if variant == "Ok" => {
                let payload = enum_payload(obj);
                Ok(some_value(payload.unwrap_or(Value::Unit)))
            }
            (Value::Enum { variant, .. }, "ok") if variant == "Err" => Ok(none_value()),
            (Value::Enum { type_name, .. }, "ok") if type_name == "Option" => Ok(obj),

            // `err()` is `ok()`'s mirror: keeps the error, discards a
            // success value. Only meaningful on `Result`.
            (Value::Enum { variant, .. }, "err") if variant == "Err" => {
                let payload = enum_payload(obj);
                Ok(some_value(payload.unwrap_or(Value::Unit)))
            }
            (Value::Enum { variant, .. }, "err") if variant == "Ok" => Ok(none_value()),

            // `map(f)`: transforms the success payload through `f`,
            // leaving an error/`None` untouched. `map_err(f)`: the
            // mirror, transforming only the error payload.
            (Value::Enum { variant, .. }, "map") if variant == "Ok" || variant == "Some" => {
                let f = args.into_iter().next()
                    .ok_or_else(|| Signal::RuntimeError(RuntimeError::new(crate::interpreter::pesan_hof_method_argumen("map", 0))))?;
                let (type_name, variant, payload) = enum_parts(obj);
                let inner = payload.unwrap_or(Value::Unit);
                let mapped = self.call_fn(f, vec![inner])?;
                Ok(Value::Enum { type_name, variant, payload: Some(Box::new(mapped)) })
            }
            (Value::Enum { variant, .. }, "map") if variant == "Err" || variant == "None" => Ok(obj),
            (Value::Enum { variant, .. }, "map_err") if variant == "Err" => {
                let f = args.into_iter().next()
                    .ok_or_else(|| Signal::RuntimeError(RuntimeError::new(crate::interpreter::pesan_hof_method_argumen("map_err", 0))))?;
                let (type_name, variant, payload) = enum_parts(obj);
                let inner = payload.unwrap_or(Value::Unit);
                let mapped = self.call_fn(f, vec![inner])?;
                Ok(Value::Enum { type_name, variant, payload: Some(Box::new(mapped)) })
            }
            (Value::Enum { variant, .. }, "map_err") if variant == "Ok" => Ok(obj),

            // `and_then(f)`: like `map`, but `f` itself returns a whole new
            // `Result`/`Option` (rather than a plain payload value) — useful for …
            (Value::Enum { variant, .. }, "and_then") if variant == "Ok" || variant == "Some" => {
                let f = args.into_iter().next()
                    .ok_or_else(|| Signal::RuntimeError(RuntimeError::new(crate::interpreter::pesan_hof_method_argumen("and_then", 0))))?;
                let payload = enum_payload(obj);
                let inner = payload.unwrap_or(Value::Unit);
                self.call_fn(f, vec![inner])
            }
            (Value::Enum { variant, .. }, "and_then") if variant == "Err" || variant == "None" => Ok(obj),

            // ----------------------------------------------------------- Channel
            // methods — an alternative, method-call spelling of the primary `ch <- val` …
            (Value::Channel(_), "send") => {
                let handle = if let Value::Channel(h) = &obj { h.clone() } else { unreachable!() };
                // Tanpa argumen ini bukan pengiriman. Dulu `unwrap_or(Value::Unit)`
                // MENGARANG sebuah `()` untuk dikirim, jadi `c.send()` menjawab
                // berhasil atas nilai yang tidak pernah ditulis siapa pun.
                let Some(val) = args.into_iter().next() else {
                    return runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("chan::send"));
                };
                match channel_send(&handle, val) {
                    Ok(())   => Ok(Value::Unit),
                    Err(msg) => runtime_err!("{}", msg),
                }
            }
            (Value::Channel(_), "recv") => {
                let handle = if let Value::Channel(h) = &obj { h.clone() } else { unreachable!() };
                match channel_recv(&handle) {
                    Ok(v)    => Ok(v),
                    Err(msg) => runtime_err!("{}", msg),
                }
            }
            (Value::Channel(_), "try_send") => {
                let handle = if let Value::Channel(h) = &obj { h.clone() } else { unreachable!() };
                // Tanpa argumen ini bukan pengiriman. Dulu `unwrap_or(Value::Unit)`
                // MENGARANG sebuah `()` untuk dikirim, jadi `c.try_send()` menjawab
                // berhasil atas nilai yang tidak pernah ditulis siapa pun.
                let Some(val) = args.into_iter().next() else {
                    return runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("chan::try_send"));
                };
                match channel_try_send(&handle, val) {
                    Ok(())     => Ok(ok_value(Value::Unit)),
                    Err(true)  => Ok(err_value("channel penuh, kirim akan block (would block)")),
                    Err(false) => Ok(err_value("channel sudah ditutup: tidak ada penerima")),
                }
            }
            (Value::Channel(_), "try_recv") => {
                let handle = if let Value::Channel(h) = &obj { h.clone() } else { unreachable!() };
                match channel_try_recv(&handle) {
                    Ok(v)      => Ok(ok_value(v)),
                    Err(true)  => Ok(err_value("channel kosong, terima akan block (would block)")),
                    Err(false) => Ok(err_value("channel sudah ditutup: tidak ada pengirim")),
                }
            }
            (Value::Channel(c), "cap") => Ok(Value::Int(c.kapasitas() as i128)),
            // Ketiganya jalan di `--vm` dan ditolak di sini sampai antreannya
            // diganti: `mpsc::sync_channel` tidak punya satu pun dari mereka.
            (Value::Channel(c), "len") => Ok(Value::Int(c.panjang() as i128)),
            (Value::Channel(c), "is_closed") => Ok(Value::Bool(c.tertutup())),
            (Value::Channel(c), "close") => { c.tutup(); Ok(Value::Unit) }
            (Value::Channel(_), "has_value") => {
                let Value::Channel(h) = &obj else { unreachable!("baru saja dicocokkan") };
                // ADA ISI: tertutup dan kosong menjawab `false` (`channel_ada_isi` berarti
                // "tidak akan memblokir" dan dipakai `future::`, bukan di sini).
                Ok(Value::Bool(h.panjang() > 0))
            }

            // ── Method dict ──────────────────────────────────────────
            (Value::Struct { name, .. }, _) if name == crate::interpreter::builtin::dict::DICT_TYPE_NAME
                && crate::interpreter::builtin::dict::METODE_DICT.contains(&method) =>
            {
                use crate::interpreter::builtin::NativeFnKind::*;
                let kind = match method {
                    "len"      => DictLen,
                    "is_empty" => DictIsEmpty,
                    "keys"     => DictKeys,
                    "values"   => DictValues,
                    "items"    => DictItems,
                    "has"      => DictHas,
                    "get"      => DictGet,
                    "get_or"   => DictGetOr,
                    "set"      => DictSet,
                    "remove"   => DictRemove,
                    "merge"    => DictMerge,
                    _ => unreachable!("daftar method sudah disaring di guard di atas"),
                };
                let mut semua = Vec::with_capacity(args.len() + 1);
                semua.push(obj.clone());
                semua.extend(args);
                self.call_native(kind, semua)
            }

            // `to_string` bawaan untuk tipe yang tidak punya hook sendiri.
            (_, "to_string") => {
                let teks = self.tampil(&obj)?;
                Ok(Value::Str(teks))
            }
            // Canvas method dispatch
            (Value::Canvas(_), _) => {
                if let Value::Canvas(handle) = obj {
                    crate::canvas::dispatch_canvas_method(
                        std::sync::Arc::clone(&handle),
                        method,
                        &args,
                    )
                } else { unreachable!() }
            }
            // Method not found
            _ => {
                // Namespace tempat method penerima ini dicari, kalau ada.
                // Dipakai untuk MENURUNKAN daftar calon saran nama dari
                // registry runtime alih-alih menebak.
                let ns = match &obj {
                    Value::Array(_)  => Some("arr"),
                    Value::Str(_)    => Some("str"),
                    Value::Canvas(_) => Some("canvas"),
                    Value::Channel(_) => Some("chan"),
                    Value::Sinkron(p) => p.ns_metode(),
                    Value::Struct { name, .. } if name == "dict" => Some("dict"),
                    _ => None,
                };
                let (tipe, extra) = match &obj {
                    // Tuple tidak punya namespace method sama sekali; yang sah cuma indeks
                    // numerik, jadi katakan itu alih-alih membiarkan pembacanya menebak.
                    Value::Tuple(_) => ("tuple".to_string(),
                        " (yang ada untuk tuple cuma indeks numerik: `t.0`, `t.1`, ...)".to_string()),
                    // Dict disimpan sebagai `Value::Struct` bernama "dict" di dalam, tapi dari
                    // sisi bahasa ia koleksi tersendiri — menyebutnya "struct dict" mengirim …
                    Value::Struct { name, fields } if name == "dict" => {
                        let mut names: Vec<&str> = fields.keys().map(|s| s.as_str()).collect();
                        names.sort();
                        let hint = if names.is_empty() { String::new() } else {
                            format!(" (kalau maksudnya field/key, yang ada: {})", names.join(", "))
                        };
                        ("dict".to_string(), hint)
                    }
                    Value::Struct { name, fields } => {
                        let mut names: Vec<&str> = fields.keys().map(|s| s.as_str()).collect();
                        names.sort();
                        let hint = if names.is_empty() {
                            String::new()
                        } else {
                            format!(" (kalau maksudnya field/key, yang ada: {})", names.join(", "))
                        };
                        (format!("struct {}", name), hint)
                    }
                    // TIPE-nya, bukan ISI-nya.
                    other => (other.type_name(), String::new()),
                };
                let mut calon = ns.map(crate::interpreter::calon_method).unwrap_or_default();
                // Untuk struct buatan pemakai, calonnya adalah method yang benar-benar
                // terdaftar untuk tipe itu — DAN untuk setiap leluhurnya, lewat …
                if let Value::Struct { name, .. } = &obj {
                    if name != "dict" {
                        for t in self.lineage_of(name) {
                            if let Some(m) = self.methods.instance.get(&t) {
                                calon.extend(m.keys().cloned());
                            }
                        }
                    }
                }
                // Enum punya method bawaan DAN method `impl`-nya sendiri; dulu
                // tidak ada satu pun calon, jadi `Option::Some(1).unwrapp()`
                // dijawab "cek lagi penulisannya" tanpa menyebut `unwrap`.
                if let Value::Enum { type_name, .. } = &obj {
                    calon.extend(crate::interpreter::calon_method_enum(type_name)
                        .iter().map(|s| s.to_string()));
                    if let Some(m) = self.methods.instance.get(type_name) {
                        calon.extend(m.keys().cloned());
                    }
                }
                // Skalar: `abs`, `sqrt`, `round`, … Daftarnya per tipe, jadi
                // tidak ada saran yang menuntun ke pintu yang juga menolak.
                calon.extend(crate::interpreter::calon_method_skalar(&obj.type_name())
                    .iter().map(|s| s.to_string()));
                // FIELD bernama sama yang isinya fungsi: kalimat di bawah
                // menyangkal dirinya sendiri ("tidak punya field `f` … yang ada: f").
                if let Value::Struct { name, fields } = &obj {
                    if fields.get(method).is_some_and(|v| v.bisa_dipanggil()) {
                        return runtime_err!("{}",
                            crate::interpreter::galat_index::field_fungsi_bukan_metode(name, method));
                    }
                }
                // Ejaannya benar dan pintunya yang salah: jalan keluarnya
                // disebut, dan ia MENGGANTI ekor "cek lagi penulisannya ya".
                if let Some(jalan) = crate::interpreter::padanan_math(&tipe, method) {
                    return runtime_err!("{}", crate::interpreter::pesan_method_tak_ada_dgn_jalan(
                        &tipe, method, &jalan));
                }
                let saran = crate::diag::saran_nama_terdekat(
                    method, calon.iter().map(|s| s.as_str()));
                runtime_err!("{}", crate::interpreter::pesan_method_tak_ada(
                    &tipe, method, &extra, saran.as_deref()))
            }
        }
    }

    // -----------------------------------------------------------------------
}
