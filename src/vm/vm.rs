// src/vm/vm.rs — rangka VM: galat, bingkai, struktur `Vm`, dan helper stack

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use super::chunk::Chunk;
use super::value::{VmValue, VmFunction, VmClosure};

pub(crate) type VmPtrCell = Arc<Mutex<VmValue>>;

pub(crate) static NEXT_VM_PTR_ID: AtomicU64 = AtomicU64::new(1);
pub(crate) static VM_PTR_CELLS: OnceLock<Mutex<HashMap<u64, VmPtrCell>>> = OnceLock::new();

pub(crate) fn vm_ptr_cells() -> &'static Mutex<HashMap<u64, VmPtrCell>> {
    VM_PTR_CELLS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn vm_ptr_value(id: u64) -> VmValue {
    let mut fields = HashMap::new();
    fields.insert("id".to_string(), VmValue::Int(id as i128));
    VmValue::struktur("Ptr", fields)
}

pub(crate) fn vm_ptr_id(value: &VmValue, context: &str) -> Result<u64, String> {
    // Kalimatnya milik `dok::ptr::ptr_id`: dict dan struct lain disebut TIPENYA.
    let (VmValue::Map(fields), Some("Ptr")) = (value, value.nama_struct()) else {
        return Err(format!("{}: butuh Ptr, dapat `{}`", context, value.type_name_penuh()));
    };
    let fields = fields.lock().unwrap_or_else(|p| p.into_inner());
    match fields.get("id") {
        Some(VmValue::Int(id)) => u64::try_from(*id)
            .map_err(|_| format!("{}: Ptr tidak valid (`id` {} bukan nomor alokasi)", context, id)),
        _ => Err(format!("{}: Ptr tidak valid (field `id` hilang)", context)),
    }
}

pub(crate) fn vm_ptr_cell(value: &VmValue, context: &str) -> Result<VmPtrCell, String> {
    let id = vm_ptr_id(value, context)?;
    if id == 0 {
        return Err(format!("{}: null pointer dereference", context));
    }
    crate::dok::ptr::periksa_sibuk(id, context)?;
    vm_ptr_cells()
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("{}: pointer tidak valid (sudah di-free?)", context))
}

// ─── Error ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VmError {
    pub msg: String,
    /// Lokasi PERSIS tempat error terjadi — bukan nomor baris.
    pub span: Option<crate::lexer::Span>,
    /// Rantai pemanggilan menuju galat ini, TERDALAM lebih dulu.
    pub jejak: Vec<crate::diag::BingkaiJejak>,
    /// Bingkai yang tidak disimpan karena melewati batas.
    pub jejak_sisa: usize,
}

impl VmError {
    pub fn new(msg: impl Into<String>) -> Self {
        VmError { msg: crate::ruang_tipe::bersihkan(msg.into()), span: None, jejak: Vec::new(), jejak_sisa: 0 }
    }
    /// `at` menerima `Option<Span>` supaya situs panggilnya tidak perlu tahu
    /// apakah instruksi yang sedang jalan punya lokasi atau tidak — yang tanpa …
    pub fn at(msg: impl Into<String>, span: Option<crate::lexer::Span>) -> Self {
        VmError { msg: crate::ruang_tipe::bersihkan(msg.into()), span, jejak: Vec::new(), jejak_sisa: 0 }
    }
    /// Galat yang lahir tanpa lokasi — mis. pemanggilan balik sebuah HOF atas nilai bukan
    /// fungsi — diberi lokasi situs pemicunya; yang sudah punya lokasi dibiarkan.
    pub fn isi_span_bila_kosong(mut self, sp: Option<crate::lexer::Span>) -> Self {
        if self.span.is_none() { self.span = sp; }
        self
    }
}
impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "runtime error: {}", self.msg)?;
        for fr in &self.jejak { write!(f, "\n  di {}", fr.nama)?; }
        Ok(())
    }
}

pub(crate) type VmResult<T> = Result<T, VmError>;

// ─── CallFrame ───────────────────────────────────────────────────────────

pub(crate) struct CallFrame {
    pub(crate) func:   Arc<VmFunction>,
    pub(crate) ip:     usize,
    pub(crate) slot:   usize,
    /// Defer stack (LIFO) — dieksekusi saat Return/ReturnNil.
    pub(crate) defers: Vec<(VmValue, Vec<VmValue>)>,
    /// Jumlah argumen yang benar-benar diberikan pemanggil (sebelum
    /// parameter bernilai default diisi).
    pub(crate) argc: u8,
    /// Closure yang sedang dijalankan frame ini, kalau memang closure.
    pub(crate) closure: Option<Arc<VmClosure>>,
}
impl CallFrame {
    pub(crate) fn chunk(&self) -> &Chunk { &self.func.chunk }
    pub(crate) fn read_instr(&mut self) -> Option<&super::opcode::Instruction> {
        let i = self.func.chunk.code.get(self.ip);
        if i.is_some() { self.ip += 1; }
        i
    }
}

// ─── VM ──────────────────────────────────────────────────────────────────

/// Builtin yang tidak bisa dilayani penunjuk fungsi biasa.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NativeKhusus {
    /// Memanggil balik fungsi JRG (`dok::num_solve`).
    NumSolve,
    /// Menjalankan fungsi di thread lain, dan runtime barunya harus
    /// MEWARISI global pemanggilnya (`Future::new`, `Task::spawn`).
    Tugas { task: bool },
    /// Menyimpan sebuah handler route.
    Router { metode: &'static str },
    /// Menjalankan server HTTP dengan pemanggil handler milik VM
    /// (`dok::serve`).
    Serve,
    /// `terap(f, args)` — memanggil `f` dengan isi array sebagai argumennya.
    /// Mesin di balik gula `f(...v)`.
    Terap,
    /// `terap_bernama(f, posisional, nama, nilai)` — mesin di balik gula
    /// `f(a, b: 2)`.
    TerapBernama,
    /// HOF bawaan yang dipanggil lewat NILAI-nya (`let f = arr::map`).
    Hof { nama: &'static str },
    /// `py::`/`js::`/`php::`/`lua::`: worker-nya memanggil balik fungsi JRG.
    Ffi { nomor: u16 },
}

pub struct Vm {
    pub stack:    Vec<VmValue>,
    pub globals:  crate::hash_cepat::PetaCepat<String, VmValue>,
    pub(crate) frames:   Vec<CallFrame>,
    pub(crate) profiling: bool,
    /// Berapa hook tampilan sedang bersarang. Lihat `tampil::MAX_SARANG_HOOK`.
    pub(crate) sarang_hook: usize,
    /// Nama namespace tiap modul yang dimuat lewat `import`.
    pub(crate) namespace_modul: std::collections::HashSet<String>,
}

impl Vm {
    /// `globals` lahir KOSONG: bawaan bahasa tinggal di `bawaan::tabel_bawaan()`, jadi
    /// deklarasi program tidak pernah bisa menimpa sebuah builtin.
    pub fn new() -> Self {
        Vm {
            stack: Vec::with_capacity(256),
            globals: Default::default(),
            frames: Vec::with_capacity(64),
            profiling: false,
            sarang_hook: 0,
            namespace_modul: std::collections::HashSet::new(),
        }
    }

    pub fn enable_profiling(&mut self) { self.profiling = true; }

    // ── Run ───────────────────────────────────────────────────────────────

    pub fn run(&mut self, chunk: Chunk) -> VmResult<()> {
        self.stack.clear();
        self.frames.clear();
        let script_fn = Arc::new(VmFunction::new("<script>", 0, chunk));
        self.frames.push(CallFrame { func: script_fn, ip: 0, slot: 0, defers: Vec::new(), argc: 0, closure: None });
        self.run_until(0)
    }

    /// Used by channel.rs to call a function value from a spawned goroutine.
    pub fn call_value_immediate(&mut self, fn_val: VmValue, args: Vec<VmValue>) -> VmResult<VmValue> {
        let argc = args.len();
        let dasar = self.frames.len();
        let tinggi = self.stack.len();
        self.push(fn_val);
        for a in args { self.push(a); }
        // Native dan `extern "C"` selesai di dalam `call_value_at` tanpa bingkai baru. Menjalankan
        // `run_until` untuk mereka melanjutkan bingkai PEMANGGIL dari tengah instruksinya.
        let hasil = self.call_value_at(argc, None, false)
            .and_then(|_| if self.frames.len() > dasar { self.run_until(dasar) } else { Ok(()) });
        if let Err(e) = hasil {
            // Bingkainya sudah dilepas `run_until`; fungsi dan argumennya dibuang di sini.
            self.stack.truncate(tinggi);
            return Err(e);
        }
        // Fungsi yang selesai tanpa nilai (`ReturnNil`) tidak mendorong apa pun: jawabannya `unit`.
        // Dulu `pop()` di sini mengambil nilai milik PEMANGGIL, jadi lokalnya bergeser diam-diam.
        let nilai = if self.stack.len() > tinggi { self.stack.pop() } else { None };
        self.stack.truncate(tinggi);
        Ok(nilai.unwrap_or(VmValue::Unit))
    }

    /// Validasi + salin isi array — dipakai handler `OpCode::HofCall`
    /// (arr::map/filter/reduce/any/all/find).
    pub(crate) fn expect_array(&self, v: &VmValue, ctx: &str, sp: Option<crate::lexer::Span>) -> VmResult<Vec<VmValue>> {
        self.expect_array_ke(v, ctx, 1, sp)
    }

    /// Seperti `expect_array`, untuk array yang BUKAN argumen pertama — tanpanya
    /// `arr::zip_with(a, 1, f)` menyebut "argumen ke-1" untuk argumen ke-2.
    pub(crate) fn expect_array_ke(&self, v: &VmValue, ctx: &str, ke: usize, sp: Option<crate::lexer::Span>) -> VmResult<Vec<VmValue>> {
        match v {
            VmValue::Array(a) => Ok(a.lock().unwrap().clone()),
            other => Err(VmError::at(crate::interpreter::pesan_arg_harus_array(
                ctx, ke, &other.type_name_penuh()), sp)),
        }
    }

    pub(crate) fn tagged_parts(v: &VmValue) -> Option<(String, String, Option<VmValue>)> {
        let (tipe, varian, payload) = v.bagian_enum()?;
        Some((tipe.to_string(), varian.to_string(), payload.cloned()))
    }

    pub(crate) fn tagged_value(ty: &str, variant: &str, payload: Option<VmValue>) -> VmValue {
        VmValue::varian(ty, variant, payload)
    }

    /// Method HOF: builtin yang memanggil sebuah fungsi JRG per elemen.
    /// Ini const TERASOSIASI (`Vm::NAMA_HOF`), bukan item bebas — dulu
    /// ditulis di kolom nol dan terbaca seperti item bebas.
    pub const NAMA_HOF: &[&str] = &[
        "all", "and_then", "any", "count_if", "each", "filter", "find",
        "find_idx", "flat_map", "fold", "group_by", "map", "map_err", "max_by",
        "min_by", "partition", "reduce", "sort_by", "sort_by_key", "zip_with",
    ];

    /// Berapa parameter yang diterima sebuah nilai fungsi.
    pub(crate) fn arity_nilai(&self, f: &VmValue) -> Option<usize> {
        match f {
            VmValue::Function(fu) => Some(fu.arity as usize),
            VmValue::Closure(c) => Some(c.func.arity as usize),
            _ => None,
        }
    }

    /// `group_by`: satu definisi untuk bentuk namespace DAN bentuk method.
    pub(crate) fn kelompokkan(
        &mut self,
        items: Vec<VmValue>,
        f: VmValue,
    ) -> VmResult<VmValue> {
        let mut kelompok: std::collections::HashMap<String, Vec<VmValue>> =
            std::collections::HashMap::new();
        for item in items {
            let kunci = format!("{}", self.call_value_immediate(f.clone(), vec![item.clone()])?);
            kelompok.entry(kunci).or_default().push(item);
        }
        Ok(VmValue::map(
            kelompok.into_iter().map(|(k, isi)| (k, VmValue::array(isi))).collect(),
        ))
    }

    /// `sort_by`: pembanding buatan pemakai, lewat sisip biner BERSAMA.
    pub(crate) fn urut_dgn_pembanding(
        &mut self,
        items: Vec<VmValue>,
        f: VmValue,
        sp: Option<crate::lexer::Span>,
    ) -> VmResult<Vec<VmValue>> {
        crate::interpreter::urut_sisip_biner(items, |item, pembanding| {
            match self.call_value_immediate(f.clone(), vec![item.clone(), pembanding.clone()])? {
                VmValue::Int(n) => Ok(n),
                VmValue::Float(x) => Ok(if x < 0.0 { -1 } else if x > 0.0 { 1 } else { 0 }),
                // Bool DITOLAK: `true`/`false` tidak membedakan "lebih kecil" dari "sama".
                lain => Err(VmError::at(crate::interpreter::pesan_pembanding_bukan_angka(
                    &lain.type_name_penuh()), sp)),
            }
        })
    }

    /// `sort_by_key`: kunci dihitung SEKALI per elemen, lalu urut stabil.
    pub(crate) fn urut_dgn_kunci(
        &mut self,
        items: Vec<VmValue>,
        f: VmValue,
        konteks: &str,
        sp: Option<crate::lexer::Span>,
    ) -> VmResult<Vec<VmValue>> {
        let mut pasangan: Vec<(VmValue, VmValue)> = Vec::with_capacity(items.len());
        for item in items {
            let k = self.call_value_immediate(f.clone(), vec![item.clone()])?;
            pasangan.push((k, item));
        }
        let urut = crate::interpreter::urut_stabil(pasangan, |(ka, _), (kb, _)| {
            crate::stdlib::koleksi::cmp_vm(ka, kb).ok_or_else(|| VmError::at(
                crate::interpreter::pesan_kunci_tak_sebanding(
                    konteks, &ka.type_name_penuh(), &kb.type_name_penuh()), sp))
        })?;
        Ok(urut.into_iter().map(|(_, v)| v).collect())
    }

    /// `min_by`/`max_by`: satu definisi, jadi keduanya tidak bisa berselisih.
    pub(crate) fn pilih_dgn_kunci(
        &mut self,
        items: Vec<VmValue>,
        f: VmValue,
        konteks: &str,
        cari_min: bool,
        sp: Option<crate::lexer::Span>,
    ) -> VmResult<VmValue> {
        let mut it = items.into_iter();
        // Array kosong DITOLAK: "yang terkecil dari tidak ada" bukan pertanyaan.
        let Some(mut terbaik) = it.next() else {
            return Err(VmError::at(crate::interpreter::pesan_koleksi_kosong(
                konteks, if cari_min { "nilai terkecil" } else { "nilai terbesar" }), sp));
        };
        let mut kunci = self.call_value_immediate(f.clone(), vec![terbaik.clone()])?;
        for v in it {
            let k = self.call_value_immediate(f.clone(), vec![v.clone()])?;
            let o = crate::stdlib::koleksi::cmp_vm(&k, &kunci).ok_or_else(|| VmError::at(
                crate::interpreter::pesan_kunci_tak_sebanding(
                    konteks, &k.type_name_penuh(), &kunci.type_name_penuh()), sp))?;
            let lebih_baik = if cari_min {
                o == core::cmp::Ordering::Less
            } else {
                o == core::cmp::Ordering::Greater
            };
            if lebih_baik { terbaik = v; kunci = k; }
        }
        Ok(terbaik)
    }

    pub(crate) fn run_hof_method(&mut self, method: &str, argc: usize, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop(sp)?);
        }
        args.reverse();
        let obj = self.pop(sp)?;

        // 1. Method tipe pemakai MENANG, berapa pun argumennya: `impl P { fn map(self, f, g) }`
        //    dulu ditolak saat kompilasi, karena nama `map` dikira selalu milik array.
        if let Some(tipe) = obj.nama_tipe().map(|t| t.to_string()) {
            if !matches!(tipe.as_str(), "Option" | "Result") {
                if let Some(callee) = self.metode_pemakai(&tipe, method) {
                    let mut semua = Vec::with_capacity(argc + 1);
                    semua.push(obj);
                    semua.extend(args);
                    let hasil = self.call_value_immediate(callee, semua)?;
                    self.push(hasil);
                    return Ok(());
                }
            }
        }

        // 2. Penerima bawaan yang memang punya HOF ini — aturan varian sama dengan interpreter.
        let varian = obj.bagian_enum().map(|(_, v, _)| v.to_string());
        let punya = match (&obj, varian.as_deref()) {
            (VmValue::Array(_), _) => !matches!(method, "map_err" | "and_then"),
            (_, Some(v)) => match method {
                "map" | "and_then" => matches!(v, "Some" | "None" | "Ok" | "Err"),
                "map_err" => matches!(v, "Ok" | "Err"),
                _ => false,
            },
            _ => false,
        };
        if !punya {
            // 3.
            let callee = self.get_method(&obj, method, sp)?;
            if matches!(callee, VmValue::NativeFn(_)) {
                if let Some(p) = Self::pesan_jumlah_arg(&obj, method, argc) {
                    return Err(VmError::at(p, sp));
                }
            }
            let mut semua = Vec::with_capacity(argc + 1);
            semua.push(obj);
            semua.extend(args);
            let hasil = self.call_value_immediate(callee, semua)?;
            self.push(hasil);
            return Ok(());
        }
        let wajib = crate::interpreter::bentuk_hof_method(method).map(|(n, _)| n).unwrap_or(1);
        if argc != wajib {
            return Err(VmError::at(crate::interpreter::pesan_hof_method_argumen(method, argc), sp));
        }
        let mut sisa = args.into_iter();
        let a0 = sisa.next().unwrap_or(VmValue::Nil);
        let a1 = sisa.next().unwrap_or(VmValue::Nil);

        if let VmValue::Array(_) = &obj {
            let items = self.expect_array(&obj, method, sp)?;
            match method {
                "map" => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        out.push(self.call_value_immediate(a0.clone(), vec![item])?);
                    }
                    self.push(VmValue::array(out));
                }
                "filter" => {
                    let mut out = Vec::new();
                    for item in items {
                        if self.call_value_immediate(a0.clone(), vec![item.clone()])?.is_truthy() {
                            out.push(item);
                        }
                    }
                    self.push(VmValue::array(out));
                }
                "any" => {
                    let mut found = false;
                    for item in items {
                        if self.call_value_immediate(a0.clone(), vec![item])?.is_truthy() {
                            found = true;
                            break;
                        }
                    }
                    self.push(VmValue::Bool(found));
                }
                "all" => {
                    let mut all = true;
                    for item in items {
                        if !self.call_value_immediate(a0.clone(), vec![item])?.is_truthy() {
                            all = false;
                            break;
                        }
                    }
                    self.push(VmValue::Bool(all));
                }
                // `Option`, sama seperti `.first()`/`.last()`/`.get()`.
                "find" => {
                    let mut found = opsi_none();
                    for item in items {
                        if self.call_value_immediate(a0.clone(), vec![item.clone()])?.is_truthy() {
                            found = opsi_some(item);
                            break;
                        }
                    }
                    self.push(found);
                }
                "partition" => {
                    let mut yes = Vec::new();
                    let mut no = Vec::new();
                    for item in items {
                        if self.call_value_immediate(a0.clone(), vec![item.clone()])?.is_truthy() {
                            yes.push(item);
                        } else {
                            no.push(item);
                        }
                    }
                    self.push(VmValue::tuple(vec![VmValue::array(yes), VmValue::array(no)]));
                }
                // `unit`, sama dengan `arr::each` — satu nama, satu kontrak.
                // Dulu bentuk method ini menjawab array aslinya "biar bisa
                // dirangkai", dan yang dirangkai justru hasil yang dibuang.
                "each" => {
                    for item in items {
                        self.call_value_immediate(a0.clone(), vec![item])?;
                    }
                    self.push(VmValue::Unit);
                }
                "count_if" => {
                    let mut n: i128 = 0;
                    for item in items {
                        if self.call_value_immediate(a0.clone(), vec![item])?.is_truthy() {
                            n += 1;
                        }
                    }
                    self.push(VmValue::Int(n));
                }
                // `Option`, sama seperti `find`: `-1` adalah indeks yang SAH di JRG.
                "find_idx" => {
                    let mut ketemu = opsi_none();
                    for (i, item) in items.into_iter().enumerate() {
                        if self.call_value_immediate(a0.clone(), vec![item])?.is_truthy() {
                            ketemu = opsi_some(VmValue::Int(i as i128));
                            break;
                        }
                    }
                    self.push(ketemu);
                }
                "flat_map" => {
                    let mut out = Vec::new();
                    for item in items {
                        match self.call_value_immediate(a0.clone(), vec![item])? {
                            VmValue::Array(inner) => out.extend(inner.lock().unwrap().iter().cloned()),
                            lain => out.push(lain),
                        }
                    }
                    self.push(VmValue::array(out));
                }
                // Array kosong DITOLAK di dalam `pilih_dgn_kunci`, satu definisi
                // untuk bentuk method maupun namespace.
                "min_by" | "max_by" => {
                    let terbaik = self.pilih_dgn_kunci(
                        items, a0, &format!("arr::{}", method), method == "min_by", sp)?;
                    self.push(terbaik);
                }
                // Array BARU; aslinya tidak disentuh, karena builtin VM harus murni.
                "sort_by" => {
                    if self.arity_nilai(&a0) == Some(1) {
                        return Err(VmError::at(crate::interpreter::pesan_sort_by_satu_param(), sp));
                    }
                    let out = self.urut_dgn_pembanding(items, a0, sp)?;
                    self.push(VmValue::array(out));
                }
                "sort_by_key" => {
                    let out = self.urut_dgn_kunci(items, a0, "arr::sort_by_key", sp)?;
                    self.push(VmValue::array(out));
                }
                "group_by" => {
                    let hasil = self.kelompokkan(items, a0)?;
                    self.push(hasil);
                }
                "reduce" => {
                    let mut acc = a1;
                    for item in items {
                        acc = self.call_value_immediate(a0.clone(), vec![acc, item])?;
                    }
                    self.push(acc);
                }
                // `a.fold(awal, f)` — nilai awal di DEPAN, kebalikan `reduce(f, awal)`.
                "fold" => {
                    let mut acc = a0;
                    for item in items {
                        acc = self.call_value_immediate(a1.clone(), vec![acc, item])?;
                    }
                    self.push(acc);
                }
                "zip_with" => {
                    let right = self.expect_array(&a0, "zip_with", sp)?;
                    let mut out = Vec::with_capacity(items.len().min(right.len()));
                    for (x, y) in items.into_iter().zip(right) {
                        out.push(self.call_value_immediate(a1.clone(), vec![x, y])?);
                    }
                    self.push(VmValue::array(out));
                }
                _ => return Err(VmError::at(
                    crate::interpreter::pesan_method_tak_ada("array", method, "", None), sp)),
            }
            return Ok(());
        }

        // Option/Result. `Err`/`None` untuk `map`/`and_then` dan `Ok` untuk `map_err`
        // diteruskan apa adanya.
        let (ty, variant, payload) = Self::tagged_parts(&obj).ok_or_else(|| VmError::at(
            crate::interpreter::pesan_method_tak_ada(&obj.type_name_penuh(), method, "", None), sp))?;
        match method {
            "map" if variant == "Ok" || variant == "Some" => {
                let mapped = self.call_value_immediate(a0, vec![payload.unwrap_or(VmValue::Nil)])?;
                self.push(Self::tagged_value(&ty, &variant, Some(mapped)));
            }
            "map_err" if variant == "Err" => {
                let mapped = self.call_value_immediate(a0, vec![payload.unwrap_or(VmValue::Nil)])?;
                self.push(Self::tagged_value(&ty, &variant, Some(mapped)));
            }
            "and_then" if variant == "Ok" || variant == "Some" => {
                let next = self.call_value_immediate(a0, vec![payload.unwrap_or(VmValue::Nil)])?;
                self.push(next);
            }
            _ => self.push(obj),
        }
        Ok(())
    }
    // ── Stack helpers ─────────────────────────────────────────────────────

    #[inline] pub fn push(&mut self, v: VmValue) { self.stack.push(v); }
    #[inline] pub(crate) fn pop(&mut self, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        self.stack.pop().ok_or_else(|| VmError::at("stack underflow", sp))
    }
    #[inline] pub(crate) fn pop2(&mut self, sp: Option<crate::lexer::Span>) -> VmResult<(VmValue, VmValue)> {
        let b = self.pop(sp)?; let a = self.pop(sp)?; Ok((a, b))
    }
    #[inline] pub(crate) fn peek(&self, offset: usize, sp: Option<crate::lexer::Span>) -> VmResult<&VmValue> {
        let idx = self.stack.len().checked_sub(1 + offset)
            .ok_or_else(|| VmError::at("stack underflow", sp))?;
        Ok(&self.stack[idx])
    }
    #[inline] pub(crate) fn frame_slot(&self) -> usize {
        self.frames.last().map(|f| f.slot + 1).unwrap_or(0)
    }

    /// Sel tangkapan ke-`idx` milik closure yang sedang berjalan.
    pub(crate) fn upval_at(&self, idx: u32, sp: Option<crate::lexer::Span>) -> VmResult<Arc<Mutex<VmValue>>> {
        let c = self.frames.last().and_then(|f| f.closure.as_ref()).ok_or_else(|| {
            VmError::at("akses tangkapan closure di frame yang bukan closure", sp)
        })?;
        c.captures.get(idx as usize).map(Arc::clone).ok_or_else(|| {
            VmError::at(format!("tangkapan closure ke-{} tidak ada", idx), sp)
        })
    }
    pub(crate) fn const_at(&self, idx: u32) -> VmResult<VmValue> {
        let f = self.frames.last().unwrap();
        f.chunk().constants.get(idx as usize).cloned()
            .ok_or_else(|| VmError::new(format!("const idx {} OOB", idx)))
    }
    pub(crate) fn name_at(&self, idx: u32) -> VmResult<String> {
        let f = self.frames.last().unwrap();
        f.chunk().names.get(idx as usize).cloned()
            .ok_or_else(|| VmError::new(format!("name idx {} OOB", idx)))
    }
    pub(crate) fn chunk_at(&self, idx: u32) -> VmResult<Chunk> {
        let f = self.frames.last().unwrap();
        f.chunk().functions.get(idx as usize).cloned()
            .ok_or_else(|| VmError::new(format!("fn idx {} OOB", idx)))
    }
}

/// Pesan untuk global yang tidak ketemu.
impl Vm {
    /// Tegakkan `pub`/privat untuk satu akses field.
    pub(crate) fn periksa_visibilitas(&self, obj: &VmValue, field: &str, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        // Dict dan enum tidak punya deklarasi field, jadi tidak ada yang bisa privat.
        let Some(tipe_objek) = obj.nama_struct().map(|s| s.to_string()) else { return Ok(()) };
        let tipe_self = self.frames.last().and_then(|f| f.func.chunk.tipe_self.clone());
        crate::vm::visibilitas::periksa(&tipe_objek, field, tipe_self.as_deref())
            .map_err(|e| VmError::at(e, sp))
    }
}

pub(crate) fn jelaskan_global_hilang(name: &str) -> String {
    // Kalimatnya TIDAK ditulis di sini.
    crate::interpreter::pesan_nama_tak_dikenal(name, None)
}

/// Nama seperti yang DITULIS: awalan modul (`alat#1::`) dibuang. `#` tidak bisa
/// ditulis di nama mana pun, jadi cuma awalan buatan pemuat yang kena.
pub(crate) fn nama_ditulis(nama: &str) -> &str {
    match nama.split_once("::") {
        Some((awalan, ekor)) if awalan.contains('#') => ekor,
        _ => nama,
    }
}

/// Apakah `ns` awalan sebuah namespace bawaan (`str`, `arr`, `math`, …).
pub(crate) fn namespace_bawaan(ns: &str) -> bool {
    static NS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    NS.get_or_init(|| {
        crate::vm::daftar_builtin().iter()
            .filter_map(|n| n.split_once("::").map(|(a, _)| a.to_string()))
            .collect()
    }).contains(ns)
}

impl Vm {
    /// Pesan "global tidak ada" untuk frame yang sedang jalan. Saran dan penjelasan untuk
    /// nama telanjang dan jalur lewat alias dihitung kompilator; di sini tinggal jalur
    /// yang memang baru diketahui saat jalan (`Tipe::metode`).
    pub(crate) fn jelaskan_global_hilang_kini(&self, name: &str) -> String {
        self.jelaskan_global_hilang_di(self.frames.last().map(|f| &f.func.chunk), name)
    }

    /// Sama, untuk chunk yang DISEBUT: `--periksa` menanyakannya tanpa menjalankan apa pun.
    pub(crate) fn jelaskan_global_hilang_di(&self, chunk: Option<&Chunk>, name: &str) -> String {
        let name = nama_ditulis(name);
        // Dideklarasikan di tingkat atas unit ini, tapi barisnya belum dijalankan.
        if chunk.is_some_and(|c| c.tingkat_atas.iter().any(|n| n == name)) {
            return crate::interpreter::pesan_belum_diisi(name);
        }
        // Nama tipe, trait, varian ber-field, atau modul: dijelaskan, bukan dituduh salah ketik.
        if let Some(alasan) = chunk.and_then(|c| c.bukan_nilai.iter()
            .find(|(n, _)| n == name).map(|(_, a)| a.clone())) {
            return alasan;
        }
        let saran = chunk.and_then(|c| {
            c.saran_lokal.iter()
                .find(|(n, _)| n == name).map(|(_, s)| s.as_str())
        });
        // Modulnya dimuat, tapi oleh berkas LAIN: yang salah bukan ejaannya melainkan
        // `import` yang belum ada. (Alias milik unit ini sudah dijawab kompilator.)
        if let Some((kepala, _)) = name.split_once("::") {
            if self.namespace_modul.contains(kepala) && !namespace_bawaan(kepala) {
                return crate::interpreter::pesan_modul_tak_diimpor(name, kepala);
            }
        }
        jelaskan_global_hilang_dgn_saran_plus(&self.globals, name, saran)
    }
}

pub(crate) fn jelaskan_global_hilang_dgn_saran_plus(
    globals: &crate::hash_cepat::PetaCepat<String, VmValue>, name: &str, saran_lokal: Option<&str>,
) -> String {
    if !name.contains("::") {
        return crate::interpreter::pesan_nama_tak_dikenal(name, saran_lokal);
    }
    // Daftarnya dari `vm::daftar_builtin()`: nama HOF (`arr::map`) didispatch lewat nama
    // method dan tidak pernah jadi global. Nama ber-awalan modul (`#`) dan nama internal
    // (`__`) tidak pernah jadi saran — keduanya tidak bisa ditulis.
    let daftar = crate::vm::daftar_builtin();
    let pemakai = || globals.keys().map(|k| k.as_str())
        .filter(|k| !k.contains('#') && !k.starts_with("__"));
    let saran = saran_lokal.map(|s| s.to_string())
        .or_else(|| crate::diag::saran_nama_terdekat(name, daftar.iter().map(|s| s.as_str())))
        .or_else(|| crate::diag::saran_nama_terdekat(name, pemakai()));
    let lain = crate::diag::nama_di_namespace_lain(name, daftar.iter().map(|s| s.as_str()));
    // Nama telanjangnya ada? Itu jawaban terbaik, dan aturannya satu —
    // dipakai kedua backend lewat fungsi pesan yang sama.
    let ekor = name.rsplit("::").next().unwrap_or("");
    let telanjang = super::bawaan::bawaan(ekor).is_some() || globals.contains_key(ekor);
    crate::interpreter::pesan_path_tak_dikenal_plus(name, saran.as_deref(), &lain, telanjang)
}

/// `wadah`/`satuan` menyebut apa yang sedang di-index ("array"/"elemen",
/// "string"/"karakter", "tuple"/"elemen").
pub(crate) fn norm_idx(
    i: i128,
    len: usize,
    wadah: &str,
    satuan: &str,
    sp: Option<crate::lexer::Span>,
) -> VmResult<usize> {
    let idx = if i < 0 { len as i128 + i } else { i };
    // Perbandingan dilakukan di ruang i128, BUKAN sesudah `as usize`.
    if idx < 0 || idx >= len as i128 {
        Err(VmError::at(
            crate::interpreter::Interpreter::oob_msg(wadah, satuan, i, len),
            sp,
        ))
    } else {
        Ok(idx as usize)
    }
}

/// `Option::Some(v)` / `Option::None` — nama pendek untuk `VmValue::some`/`none`.
pub(crate) fn opsi_some(v: VmValue) -> VmValue { VmValue::some(v) }

pub(crate) fn opsi_none() -> VmValue { VmValue::none() }

pub(crate) fn optional_index(v: &VmValue, sp: Option<crate::lexer::Span>) -> VmResult<Option<i128>> {
    match v {
        VmValue::Nil => Ok(None),
        // Nilai MENTAH, termasuk yang negatif. Batas negatif dihitung dari
        // belakang (`a[-2:]` = dua elemen terakhir) — itu semantik JRG, dan
        // penyelesaiannya butuh panjang, jadi dikerjakan di `slice_value`.
        VmValue::Int(i) => Ok(Some(*i)),
        other => Err(VmError::at(
            format!("index slice harus integer, dapat `{}`", other.type_name_penuh()),
            sp,
        )),
    }
}

/// Cast ke integer, dengan aturan yang SAMA persis dengan interpreter.
pub(crate) fn cast_int(a: &[VmValue]) -> Result<VmValue, String> {
    ubah_bulat(a.get(0), "int", crate::interpreter::konversi::Lebar::I128, "int()")
}

/// `x as <bulat>` dan `int()`/`uint()`: aturannya di `interpreter::konversi`,
/// yang juga dipakai interpreter — di sini cuma pemetaan varian nilainya.
pub(crate) fn ubah_bulat(
    v: Option<&VmValue>, nama: &str, lebar: crate::interpreter::konversi::Lebar, pintu: &str,
) -> Result<VmValue, String> {
    use crate::interpreter::konversi as k;
    use VmValue::*;
    let n = match v {
        Some(Int(n))   => k::cek_muat(*n, nama, lebar)?,
        Some(Float(f)) => k::float_ke_bulat(*f, nama, lebar)?,
        Some(Bool(b))  => *b as i128,
        // Rune ke bilangan bulat memberi kode karakternya, sama seperti interpreter.
        Some(Rune(c))  => k::cek_muat_asli(*c as i128, nama, lebar, || c.to_string())?,
        Some(Ptr(a))   => k::cek_muat_asli(*a as i128, nama, lebar, || Ptr(*a).to_string())?,
        Some(Str(s))   => k::teks_ke_bulat(s, nama, lebar)?,
        Some(Nil)      => return Err(k::pesan_nil_jadi(nama)),
        Some(lain)     => return Err(k::pesan_tak_bisa_jadi(&lain.type_name_penuh(), nama)),
        None           => return Err(crate::interpreter::pesan_butuh_nilai(pintu)),
    };
    Ok(Int(n))
}

/// Nama fungsi untuk pesan error `ArrUbahMilik`, supaya penolakannya
/// menyebut nama yang benar-benar ditulis pemakai — bukan nama opcode
/// yang tidak pernah dilihat siapa pun.
pub(crate) fn nama_arr_ubah(arg: u32) -> &'static str {
    match arg {
        0 => "arr::push",
        1 => "arr::pop",
        2 => "arr::reverse",
        _ => "arr::concat",
    }
}

/// `ptr::inc` / `ptr::dec` untuk VM.
pub(crate) fn vm_ptr_step(a: &[VmValue], fname: &str, delta: i128) -> Result<VmValue, String> {
    let id = vm_ptr_id(a.get(0).unwrap_or(&VmValue::Nil), fname)?;
    if id == 0 {
        return Err(format!("{}: null pointer dereference", fname));
    }
    let cell = vm_ptr_cell(a.get(0).unwrap_or(&VmValue::Nil), fname)?;
    let mut g = cell.lock().unwrap_or_else(|p| p.into_inner());
    let baru = match &*g {
        VmValue::Int(n)   => VmValue::Int(n.checked_add(delta).ok_or_else(|| format!(
            "{}: {}", fname, crate::interpreter::pesan_luber("+", *n, delta)))?),
        VmValue::Float(f) => VmValue::Float(f + delta as f64),
        lain => return Err(format!("{}: isi Ptr bukan angka (dapat `{}`)", fname, lain.type_name_penuh())),
    };
    *g = baru.clone();
    Ok(baru)
}
