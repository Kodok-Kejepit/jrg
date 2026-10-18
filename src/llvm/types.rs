// src/llvm/types.rs — jrgc LLVM backend — src/llvm/types.rs

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;

// ─── The type itself ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JrgType {
    Bool,
    I8, I16, I32, I64, I128,
    U8, U16, U32, U64, U128,
    F32, F64,
    /// Satu Unicode scalar.
    Rune,
    /// `nil` — nilai tunggal berukuran nol.
    Nil,
    Unit,
    /// `{ ptr, i64 }` fat pointer: (bytes, char-length-agnostic byte count).
    Str,
    /// Fixed-size array, `[N x T]`, length known at compile time.
    Array(Box<JrgType>, u64),
    /// Slice / view, `{ ptr, i64 }`, over elements of type T. Also what a
    /// bare `array` parameter type monomorphizes to.
    Slice(Box<JrgType>),
    Tuple(Vec<JrgType>),
    /// Dict berkunci STRING dengan tipe nilai seragam.
    Dict(Box<JrgType>),
    /// Nilai BERTAG: `{ i32 tag, [16 x i8] muatan }`.
    Any,
    /// Dict yang nilainya BEDA tipe, dengan tipe tiap kuncinya diingat.
    DictCampur(std::rc::Rc<Vec<(String, JrgType)>>),
    /// Already-mangled struct name — look up its field layout in `TypeTable`.
    Struct(String),
    /// Already-mangled enum name — look up its variant layout in `TypeTable`.
    Enum(String),
    /// Nilai fungsi: `{ ptr kode, ptr lingkungan }`.
    Fn(Vec<JrgType>, Box<JrgType>),
    /// Channel ber-elemen T: penunjuk ke `JrgChan` (korutin.c) — sebuah HANDLE.
    Chan(Box<JrgType>),
    /// `Future`/`Task` berhasil T (`true` = Task): channel sekali-serah.
    Masa(Box<JrgType>, bool),
    /// Pegangan `Atomic` (`false`) atau `Mutex` (`true`): penunjuk ke keadaannya di heap.
    Sinkron(bool),
    /// Return type of an expression that never produces a value because
    /// every path panics/returns/breaks (e.g. the body of `Err(...)?`'s
    /// early-return arm). Unifies with anything.
    Never,
}

impl JrgType {
    pub fn is_int(&self) -> bool {
        matches!(self, JrgType::I8 | JrgType::I16 | JrgType::I32 | JrgType::I64 | JrgType::I128
                     | JrgType::U8 | JrgType::U16 | JrgType::U32 | JrgType::U64 | JrgType::U128)
    }
    pub fn is_signed_int(&self) -> bool {
        matches!(self, JrgType::I8 | JrgType::I16 | JrgType::I32 | JrgType::I64 | JrgType::I128)
    }
    pub fn is_unsigned_int(&self) -> bool {
        matches!(self, JrgType::U8 | JrgType::U16 | JrgType::U32 | JrgType::U64 | JrgType::U128)
    }
    pub fn is_float(&self) -> bool {
        matches!(self, JrgType::F32 | JrgType::F64)
    }
    pub fn is_numeric(&self) -> bool { self.is_int() || self.is_float() }

    /// Tipe yang REPRESENTASINYA bilangan bulat, termasuk `rune`.
    pub fn berbasis_int(&self) -> bool { self.is_int() || matches!(self, JrgType::Rune) }

    pub fn int_bits(&self) -> Option<u32> {
        Some(match self {
            JrgType::I8  | JrgType::U8  => 8,
            JrgType::I16 | JrgType::U16 => 16,
            JrgType::I32 | JrgType::U32 => 32,
            JrgType::I64 | JrgType::U64 => 64,
            JrgType::I128| JrgType::U128 => 128,
            JrgType::Rune => 32,
            _ => return None,
        })
    }

    /// LLVM spelling for this type used as a *value* (SSA register /
    /// alloca'd storage / struct field / array element). `Unit` becomes an
    /// empty struct here since `void` is only legal as a function return.
    pub fn llvm_ty(&self) -> String {
        match self {
            JrgType::Bool => "i8".to_string(), // stored as i8; truncated to i1 only at branch/icmp use sites
            JrgType::I8  | JrgType::U8  => "i8".to_string(),
            JrgType::I16 | JrgType::U16 => "i16".to_string(),
            JrgType::I32 | JrgType::U32 => "i32".to_string(),
            JrgType::I64 | JrgType::U64 => "i64".to_string(),
            JrgType::I128| JrgType::U128 => "i128".to_string(),
            JrgType::F32 => "float".to_string(),
            JrgType::F64 => "double".to_string(),
            // Representasi sama dengan i32; yang beda cuma perilakunya.
            JrgType::Rune => "i32".to_string(),
            // Berukuran nol, sama seperti Unit — tidak ada bit untuk disimpan.
            JrgType::Nil => "{}".to_string(),
            JrgType::Unit => "{}".to_string(),
            JrgType::Str  => "%JrgStr".to_string(),
            JrgType::Array(elem, n) => format!("[{} x {}]", n, elem.llvm_ty()),
            JrgType::Slice(_) => "%JrgSlice".to_string(),
            JrgType::Dict(_) => "%JrgDict".to_string(),
            JrgType::Any => "%JrgAny".to_string(),
            // Perwujudannya IDENTIK dengan `Dict(Any)`; yang berbeda cuma
            // apa yang diketahui kompilator tentang isinya.
            JrgType::DictCampur(_) => "%JrgDict".to_string(),
            JrgType::Tuple(elems) => {
                let parts: Vec<String> = elems.iter().map(|e| e.llvm_ty()).collect();
                format!("{{ {} }}", parts.join(", "))
            }
            JrgType::Struct(name) => format!("%\"{}\"", name),
            JrgType::Enum(name)   => format!("%\"{}\"", name),
            JrgType::Fn(..) => "%JrgFn".to_string(),
            JrgType::Chan(_) | JrgType::Masa(..) | JrgType::Sinkron(_) => "ptr".to_string(),
            JrgType::Never => "void".to_string(),
        }
    }

    /// LLVM spelling usable in a function's *return* position, where
    /// `Unit`/`Never` really do mean `void`.
    pub fn llvm_ret_ty(&self) -> String {
        match self {
            JrgType::Unit | JrgType::Never => "void".to_string(),
            other => other.llvm_ty(),
        }
    }

    pub fn is_zero_sized(&self) -> bool {
        matches!(self, JrgType::Unit)
    }

    /// A short, LLVM-identifier-safe fragment used to build mangled names
    /// for monomorphized generics (`Option$i64`, `Result$f64$str`, ...).
    pub fn mangle_frag(&self) -> String {
        match self {
            JrgType::Bool => "bool".into(),
            JrgType::I8 => "i8".into(),  JrgType::I16 => "i16".into(),
            JrgType::I32 => "i32".into(), JrgType::I64 => "i64".into(), JrgType::I128 => "i128".into(),
            JrgType::U8 => "u8".into(),  JrgType::U16 => "u16".into(),
            JrgType::U32 => "u32".into(), JrgType::U64 => "u64".into(), JrgType::U128 => "u128".into(),
            JrgType::F32 => "f32".into(), JrgType::F64 => "f64".into(),
            JrgType::Rune => "rune".into(),
            JrgType::Nil => "nil".into(),
            JrgType::Unit => "unit".into(),
            JrgType::Str => "str".into(),
            JrgType::Dict(v) => format!("dict${}", v.mangle_frag()),
            JrgType::Any => "any".to_string(),
            JrgType::DictCampur(_) => "dict$any".to_string(),
            JrgType::Array(e, n) => format!("arr{}x{}", n, e.mangle_frag()),
            JrgType::Slice(e) => format!("sl_{}", e.mangle_frag()),
            JrgType::Tuple(es) => {
                let parts: Vec<String> = es.iter().map(|e| e.mangle_frag()).collect();
                format!("tup_{}_", parts.join("_"))
            }
            JrgType::Struct(n) => n.clone(),
            JrgType::Enum(n) => n.clone(),
            JrgType::Fn(ps, r) => format!("fn{}${}", ps.iter().map(|p| p.mangle_frag()).collect::<Vec<_>>().join("$"), r.mangle_frag()),
            JrgType::Chan(e) => format!("chan${}", e.mangle_frag()),
            JrgType::Masa(e, _) => format!("masa${}", e.mangle_frag()),
            JrgType::Sinkron(k) => if *k { "kunci".into() } else { "atom".into() },
            JrgType::Never => "never".into(),
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            JrgType::Bool => "bool".into(),
            JrgType::I8 => "i8".into(), JrgType::I16 => "i16".into(), JrgType::I32 => "i32".into(),
            // `int`/`float` adalah nama yang ditulis pemakai; `i64`/`f64` cuma lebarnya di sini.
            JrgType::I64 => "int".into(), JrgType::I128 => "i128".into(),
            JrgType::U8 => "u8".into(), JrgType::U16 => "u16".into(), JrgType::U32 => "u32".into(),
            JrgType::U64 => "u64".into(), JrgType::U128 => "u128".into(),
            JrgType::F32 => "f32".into(), JrgType::F64 => "float".into(),
            JrgType::Rune => "rune".into(),
            JrgType::Nil => "nil".into(),
            JrgType::Unit => "()".into(),
            JrgType::Str => "string".into(),
            JrgType::Array(e, n) => format!("[{}]{}", n, e.display_name()),
            JrgType::Slice(e) => format!("array<{}>", e.display_name()),
            JrgType::Dict(v) => format!("dict<{}>", v.display_name()),
            JrgType::Any => "nilai bertag".to_string(),
            JrgType::DictCampur(_) => "dict<campur>".to_string(),
            JrgType::Tuple(es) => {
                let parts: Vec<String> = es.iter().map(|e| e.display_name()).collect();
                format!("({})", parts.join(", "))
            }
            JrgType::Struct(n) | JrgType::Enum(n) => n.clone(),
            JrgType::Fn(ps, r) => format!("fn({}) -> {}", ps.iter().map(|p| p.display_name()).collect::<Vec<_>>().join(", "), r.display_name()),
            JrgType::Chan(e) => format!("chan<{}>", e.display_name()),
            JrgType::Masa(e, t) => format!("{}<{}>", if *t { "Task" } else { "Future" }, e.display_name()),
            JrgType::Sinkron(k) => if *k { "Mutex".into() } else { "Atomic".into() },
            JrgType::Never => "!".into(),
        }
    }


    /// Nama tipe yang DILIHAT PROGRAM lewat `typeof`.
    pub fn nama_typeof(&self) -> String {
        match self {
            JrgType::Bool => "bool".into(),
            // Semua lebar bilangan bulat menjawab `int`, dan itu memang jawaban
            // acuannya: `typeof(u32)` dan `typeof(i8)` dua-duanya `int` di interpreter …
            JrgType::I8 | JrgType::I16 | JrgType::I32 | JrgType::I64 | JrgType::I128
            | JrgType::U8 | JrgType::U16 | JrgType::U32 | JrgType::U64 | JrgType::U128
                => "int".into(),
            JrgType::F32 | JrgType::F64 => "float".into(),
            JrgType::Rune => "rune".into(),
            JrgType::Nil => "nil".into(),
            JrgType::Str => "string".into(),
            JrgType::Unit => "unit".into(),
            JrgType::Array(_, _) | JrgType::Slice(_) => "array".into(),
            JrgType::Dict(_) => "dict".into(),
            // Sebuah nilai bertag TIDAK punya jawaban `typeof` yang statis — jawabannya
            // justru tag yang dibawanya saat jalan, dan itu yang membuat pengetikan …
            JrgType::Any => "<any: harus dibaca dari tag>".into(),
            // Sebuah dict tetap sebuah dict, apa pun isinya — itu yang
            // dijawab acuannya.
            JrgType::DictCampur(_) => "dict".into(),
            JrgType::Tuple(_) => "tuple".into(),
            JrgType::Struct(n) => format!("struct:{}", super::generik::dasar(n)),
            // Nama SUMBERNYA, bukan nama termangel: `Option<int>` di backend ini bernama
            // `Option$i64`, tapi `typeof` menjawab sesuatu yang dibaca program — dan …
            JrgType::Enum(n) => format!("enum:{}", n.split('$').next().unwrap_or(n)),
            JrgType::Fn(..) => "function".into(),
            JrgType::Chan(_) => "channel".into(),
            JrgType::Masa(_, t) => if *t { "struct:Task".into() } else { "struct:Future".into() },
            JrgType::Sinkron(k) => if *k { "mutex".into() } else { "atomic".into() },
            JrgType::Never => "unit".into(),
        }
    }
}

impl fmt::Display for JrgType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ─── Struct / enum layouts ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StructLayout {
    pub name: String,                       // mangled, matches JrgType::Struct(name)
    pub fields: Vec<(String, JrgType)>,      // declaration order
}

impl StructLayout {
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }
    pub fn field_type(&self, name: &str) -> Option<&JrgType> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub enum EnumPayload {
    Unit,
    Tuple(Vec<JrgType>),
    /// Struct-like variant; fields keep their declared names for pattern
    /// binding by name (`Triangle { base, height }`).
    Struct(Vec<(String, JrgType)>),
}

impl EnumPayload {
    /// The payload re-expressed as an (anonymous) LLVM struct type — this
    /// is what gets stored into / loaded out of the tag union's byte blob.
    pub fn as_llvm_struct(&self) -> String {
        let field_tys: Vec<String> = match self {
            EnumPayload::Unit => vec![],
            EnumPayload::Tuple(ts) => ts.iter().map(|t| t.llvm_ty()).collect(),
            EnumPayload::Struct(fs) => fs.iter().map(|(_, t)| t.llvm_ty()).collect(),
        };
        format!("{{ {} }}", field_tys.join(", "))
    }
    pub fn field_types(&self) -> Vec<JrgType> {
        match self {
            EnumPayload::Unit => vec![],
            EnumPayload::Tuple(ts) => ts.clone(),
            EnumPayload::Struct(fs) => fs.iter().map(|(_, t)| t.clone()).collect(),
        }
    }
    /// Nama field payload, untuk pesan galat yang menyebut yang ada.
    pub fn field_names(&self) -> Vec<String> {
        match self {
            EnumPayload::Struct(fs) => fs.iter().map(|(n, _)| n.clone()).collect(),
            _ => vec![],
        }
    }
    pub fn field_index_by_name(&self, name: &str) -> Option<usize> {
        match self {
            EnumPayload::Struct(fs) => fs.iter().position(|(n, _)| n == name),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnumVariantLayout {
    pub name: String,
    pub tag: u32,
    pub payload: EnumPayload,
}

#[derive(Debug, Clone)]
pub struct EnumLayout {
    pub name: String,                       // mangled, matches JrgType::Enum(name)
    pub variants: Vec<EnumVariantLayout>,
    /// Bytes needed for the largest variant's payload.
    pub payload_bytes: u64,
    /// 8 for everything except a variant containing an i128/u128 (or a
    /// nested aggregate requiring 16-byte alignment), which needs 16.
    pub payload_align: u64,
}

impl EnumLayout {
    pub fn variant(&self, name: &str) -> Option<&EnumVariantLayout> {
        self.variants.iter().find(|v| v.name == name)
    }
    pub fn variant_index(&self, name: &str) -> Option<usize> {
        self.variants.iter().position(|v| v.name == name)
    }
    /// Storage element type for the `[M x elem]` payload blob field —
    /// chosen purely to guarantee correct alignment for whatever the
    /// largest variant actually needs.
    pub fn blob_elem_ty(&self) -> &'static str {
        if self.payload_align > 8 { "i128" } else { "i64" }
    }
    pub fn blob_elem_size(&self) -> u64 {
        if self.payload_align > 8 { 16 } else { 8 }
    }
    pub fn blob_len(&self) -> u64 {
        let es = self.blob_elem_size();
        (self.payload_bytes + es - 1) / es
    }
    pub fn llvm_struct_def(&self) -> String {
        let len = self.blob_len();
        if len == 0 {
            format!("%\"{}\" = type {{ i32 }}", self.name)
        } else {
            format!("%\"{}\" = type {{ i32, [{} x {}] }}", self.name, len, self.blob_elem_ty())
        }
    }
}

// ─── Type table: size/align + registry of every concrete struct/enum ──────

#[derive(Clone)]
pub struct TypeTable {
    pub structs: HashMap<String, StructLayout>,
    pub enums:   HashMap<String, EnumLayout>,
    /// Emission order for `%Name = type {...}` declarations — later entries may
    /// reference earlier ones (opaque `ptr` means this almost never actually …
    pub emit_order: Vec<String>,
}

impl TypeTable {
    pub fn new() -> Self {
        TypeTable { structs: HashMap::new(), enums: HashMap::new(), emit_order: Vec::new() }
    }

    pub fn register_struct(&mut self, layout: StructLayout) {
        if !self.structs.contains_key(&layout.name) {
            self.emit_order.push(layout.name.clone());
            self.structs.insert(layout.name.clone(), layout);
        }
    }

    pub fn register_enum(&mut self, mut layout: EnumLayout) {
        // Sebuah varian yang PUNYA field tapi seluruhnya berukuran nol (`Some(nil)`)
        // menghasilkan `payload_bytes == 0`, dan strukturnya dipancarkan sebagai `{ …
        if layout.payload_bytes == 0
            && layout.variants.iter().any(|v| !v.payload.field_types().is_empty())
        {
            layout.payload_bytes = 1;
        }
        if !self.enums.contains_key(&layout.name) {
            self.emit_order.push(layout.name.clone());
            self.enums.insert(layout.name.clone(), layout);
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.structs.contains_key(name) || self.enums.contains_key(name)
    }

    /// Size in bytes, following ordinary C/LLVM natural-alignment layout rules
    /// (sequential fields, padded to each field's own alignment, total padded to …
    pub fn sizeof(&self, ty: &JrgType) -> u64 {
        match ty {
            JrgType::Unit | JrgType::Nil => 0,
            JrgType::Bool | JrgType::I8 | JrgType::U8 => 1,
            JrgType::I16 | JrgType::U16 => 2,
            JrgType::I32 | JrgType::U32 | JrgType::Rune => 4,
            JrgType::I64 | JrgType::U64 => 8,
            JrgType::I128 | JrgType::U128 => 16,
            JrgType::F32 => 4,
            JrgType::F64 => 8,
            JrgType::Str | JrgType::Slice(_) | JrgType::Dict(_) => 16,
            // `{ i32 tag, [2 x i64] muatan }`: tag di 0, muatan di 8
            // (dijajarkan 8 supaya `%JrgStr`/`%JrgSlice`/`%JrgDict` yang
            // semuanya `{ptr, i64}` bisa disimpan langsung ke dalamnya).
            JrgType::Any => 24,
            JrgType::DictCampur(_) => 16,
            JrgType::Array(elem, n) => {
                let es = self.sizeof(elem);
                let ea = self.alignof(elem).max(1);
                let stride = round_up(es, ea);
                stride * n
            }
            JrgType::Tuple(elems) => self.layout_seq(elems).0,
            JrgType::Struct(name) => {
                match self.structs.get(name) {
                    Some(s) => {
                        let tys: Vec<JrgType> = s.fields.iter().map(|(_, t)| t.clone()).collect();
                        self.layout_seq(&tys).0
                    }
                    None => 0,
                }
            }
            JrgType::Enum(name) => {
                match self.enums.get(name) {
                    Some(e) => {
                        let tag = 4u64;
                        let pad = round_up(tag, e.payload_align.max(1));
                        round_up(pad + e.payload_bytes, e.payload_align.max(4))
                    }
                    None => 4,
                }
            }
            JrgType::Fn(..) => 16,
            JrgType::Chan(_) | JrgType::Masa(..) | JrgType::Sinkron(_) => 8,
            JrgType::Never => 0,
        }
    }

    pub fn alignof(&self, ty: &JrgType) -> u64 {
        match ty {
            JrgType::Unit | JrgType::Nil => 1,
            JrgType::Bool | JrgType::I8 | JrgType::U8 => 1,
            JrgType::I16 | JrgType::U16 => 2,
            JrgType::I32 | JrgType::U32 | JrgType::Rune => 4,
            JrgType::I64 | JrgType::U64 => 8,
            JrgType::I128 | JrgType::U128 => 16,
            JrgType::F32 => 4,
            JrgType::F64 => 8,
            JrgType::Str | JrgType::Slice(_) | JrgType::Dict(_) => 8,
            JrgType::Any | JrgType::DictCampur(_) => 8,
            JrgType::Array(elem, _) => self.alignof(elem),
            JrgType::Tuple(elems) => self.layout_seq(elems).1,
            JrgType::Struct(name) => match self.structs.get(name) {
                Some(s) => {
                    let tys: Vec<JrgType> = s.fields.iter().map(|(_, t)| t.clone()).collect();
                    self.layout_seq(&tys).1
                }
                None => 1,
            },
            JrgType::Enum(name) => match self.enums.get(name) {
                Some(e) => e.payload_align.max(4),
                None => 4,
            },
            JrgType::Fn(..) => 8,
            JrgType::Chan(_) | JrgType::Masa(..) | JrgType::Sinkron(_) => 8,
            JrgType::Never => 1,
        }
    }

    /// Returns (total_size, max_align) for a sequence of fields laid out
    /// one after another with natural alignment padding.
    fn layout_seq(&self, tys: &[JrgType]) -> (u64, u64) {
        let mut offset = 0u64;
        let mut max_align = 1u64;
        for t in tys {
            let a = self.alignof(t).max(1);
            let s = self.sizeof(t);
            offset = round_up(offset, a);
            offset += s;
            max_align = max_align.max(a);
        }
        (round_up(offset, max_align), max_align)
    }
}

fn round_up(v: u64, align: u64) -> u64 {
    if align == 0 { return v; }
    (v + align - 1) / align * align
}
