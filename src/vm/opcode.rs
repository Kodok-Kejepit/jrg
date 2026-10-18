// src/vm/opcode.rs
// Instruction Set Architecture — JRG Bytecode VM

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OpCode {
    // Constants
    Const, Nil, True, False, IntImm,
    /// Dorong nilai `unit` — lihat catatan pada `VmValue::Unit`.
    Unit,

    // Stack
    Pop, Dup,
    /// Pop yang MENOLAK kalau nilainya bisa dipanggil: sebuah fungsi yang
    /// berdiri sendiri sebagai pernyataan tidak mengerjakan apa pun.
    PopPeriksa,
    /// Tukar dua nilai teratas.
    Swap,

    /// Jadikan nilai di puncak stack sebagai salinan yang terpisah.
    CopyVal,

    // Locals
    LoadLocal, StoreLocal,

    // ── Jalur penulisan (copy-on-write) ──────────────────────────────────
    PinjamLokal,
    /// Akar jalur target yang berupa binding tangkapan closure. Yang
    /// dipisahkan adalah nilai DI DALAM sel — selnya sendiri justru harus
    /// tetap dibagi, karena itu satu-satunya alias yang disengaja di JRG.
    PinjamUpval,
    /// Akar jalur target yang berupa variabel global.
    PinjamGlobal,
    /// Satu langkah menurun lewat `[...]` di jalur target.
    PinjamIndex,
    /// Satu langkah menurun lewat `.field` di jalur target.
    PinjamField,

    /// Ambil nilai slot lokal KELUAR, tinggalkan lubang, dorong nilainya.
    AmbilLokal,

    /// `a, b = e`: tuple di puncak stack diganti `arg` elemennya, urut; yang bukan tuple
    /// atau jumlahnya lain ditolak.
    BongkarTuple,

    /// Transformer array yang MENGAMBIL ALIH penyimpanannya.
    ArrUbahMilik,

    /// `GetField`/`SetField` yang MEMERIKSA visibilitas dulu.
    GetFieldPrivat,
    SetFieldPrivat,

    // ── Closure / upvalue ────────────────────────────────────────────────
    MakeClosure,
    /// Baca tangkapan ke-`arg` milik closure yang sedang berjalan.
    LoadUpval,
    /// Tulis ke tangkapan ke-`arg` — menembus ke sel, jadi perubahannya
    /// terlihat juga dari scope asal dan dari closure lain yang menangkap
    /// binding yang sama.
    StoreUpval,

    // Globals
    LoadGlobal, StoreGlobal, DefGlobal,
    /// Definisi `var` tingkat modul: nilainya dibungkus SEL, supaya goroutine (yang
    /// menerima salinan tabel global) tetap berbagi variabelnya dengan pemiliknya.
    DefVar,

    // Arithmetic / bitwise
    Add, Sub, Mul, Div, Mod, Pow, Neg,
    BitAnd, BitOr, BitXor, Shl, Shr, AndNot, BitNot,

    // Comparison
    Eq, Ne, Lt, Le, Gt, Ge,

    // Logic
    Not,
    /// `match` tanpa lengan yang cocok untuk nilai di puncak stack; `arg` 1 = hasil penurunan `??`.
    GagalCocok,
    /// Ganti puncak stack dengan `bool` truthiness-nya: hasil `&&`/`||` selalu `bool`.
    KeBool,

    // String
    Concat,

    // Collections
    BuildArray, BuildTuple, BuildMap, MergeMap,
    /// Instance struct: `arg` pasangan field di atas prototipe struct-nya.
    BuildStruct,
    /// Varian enum ber-payload; `tipe::varian` dari prototipe di konstanta ke-`arg`.
    BuildEnum,

    // Field/Index
    GetField, GetMethod, SetField, HasField, GetIndex, SetIndex, Slice,

    // Pola — membaca LABEL, tidak pernah kunci peta
    /// Uji varian enum; `arg` = konstanta `(tipe|nil, varian, wajib_payload)`.
    CekEnum,
    /// Uji instance struct bernama konstanta ke-`arg`.
    CekStruct,
    /// Ganti enum di puncak stack dengan payload-nya.
    AmbilPayload,

    // Control flow
    Jump, JumpFalse, JumpTrue, JumpFalseKeep, JumpTrueKeep,

    // Functions
    Call, Return, ReturnNil, LoadFn,
    /// Dorong jumlah argumen yang BENAR-BENAR diberikan pemanggil.
    LoadArgc,

    // Higher-order array functions (arr::map/filter/reduce/any/all/find) — beda
    // dari Call biasa karena butuh manggil BALIK closure milik user dari dalam …
    HofCall, HofMethod,

    // Iteration
    MakeIter, IterNext,
    /// Ambil variabel loop `for` dari state iterator.
    IterVar,
    /// `for` atas `a..b` (arg 1 = `a..=b`): state iterator TANPA array.
    MakeIterRentang,
    /// Dorong `true` selama iterator di slot `arg` masih punya elemen.
    IterAda,
    /// `obj.f += x` / `arr[i] += x` atas teks/array: sambung DI TEMPAT kalau aman, dorong
    /// `true`; selain itu tidak menyentuh apa pun dan dorong `false` (jalur biasa menyusul).
    CobaTambahField, CobaTambahIndex,
    /// `[kini, kanan]` + binding bersel (lokal/tangkapan/`var` modul, indeks di 31 bit bawah):
    /// kalau sel itu masih memegang penyimpanan `kini`, sambung `kanan` DI TEMPAT (bit 31:
    /// sebagai satu elemen), buang keduanya, dorong `true`; selain itu dorong `false` saja.
    CobaTumbuhLokal, CobaTumbuhUpval, CobaTumbuhGlobal,
    /// Slot `arg` = nilai yang tadi dibaca dari sebuah jalur, slot berikutnya = kanan, puncak
    /// stack = bacaan ulang jalur itu. Kalau penyimpanannya masih sama: kosongkan slot `arg`
    /// (bit 31: bungkus kanan jadi array satu elemen) dan dorong `true`; selain itu `false`.
    SamaTempat,
    /// `StoreLocal` lalu `Pop` dalam satu langkah: nilai di puncak PINDAH ke lokal `arg`.
    SimpanBuang,
    /// `[idx]` → `lokal[idx]`: membaca elemen tanpa mengangkat wadahnya ke stack.
    IndeksLokal,
    /// `[nilai, idx]` → `lokal[idx] = nilai`, keduanya dibuang.
    TulisIndexLokal,
    /// `[kini, kanan, idx]` atas lokal (24 bit bawah): kalau `lokal[idx]` masih penyimpanan
    /// `kini` dan keduanya array, sambung DI TEMPAT, buang ketiganya, lompati N instruksi
    /// (8 bit atas); selain itu buang `idx` saja.
    TumbuhIndexLokal,
    /// Penghitung iterator di lokal `arg` naik satu (`for` atas rentang, array, dict).
    IterMaju,

    // Misc
    Typeof, Len, Print, PrintNoNl,

    // Stack manipulation
    SlideDown,

    // Defer
    DeferPush,

    // Array repeat
    BuildRepeat,

    // Channels
    ChanMake, ChanSend, ChanRecv, ChanTrySend, ChanTryRecv, ChanClose,
    /// Operand `case` di `select` harus channel — diperiksa SEBELUM menunggu.
    CekChannelSelect,
    /// Tiap putaran `select`: channel yang sudah ditutup (arg 0 terima, 1 kirim) DITOLAK.
    CekTutupSelect,

    // Goroutine spawn
    Spawn,

    // Select
    SelectBegin, SelectRecvArm, SelectSendArm, SelectDefault, SelectEnd,

    // Propagate (?) operator
    Propagate,

    // Halt
    Panic,
    Halt,
}

impl std::fmt::Display for OpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Instruction {
    pub op:  OpCode,
    pub arg: u32,
    /// Indeks ke kolam span milik `Chunk`, BUKAN nomor baris.
    pub span: u32,
}

impl Instruction {
    #[inline]
    pub fn new(op: OpCode, arg: u32, span: u32) -> Self {
        Instruction { op, arg, span }
    }
    #[inline]
    pub fn simple(op: OpCode, span: u32) -> Self {
        Instruction { op, arg: 0, span }
    }
}

impl std::fmt::Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.op {
            OpCode::Const | OpCode::LoadLocal | OpCode::StoreLocal
            | OpCode::MakeClosure | OpCode::LoadUpval | OpCode::StoreUpval
            | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefGlobal | OpCode::DefVar
            | OpCode::Jump | OpCode::JumpFalse | OpCode::JumpTrue
            | OpCode::JumpFalseKeep | OpCode::JumpTrueKeep
            | OpCode::BuildArray | OpCode::BuildTuple | OpCode::BuildMap
            | OpCode::BuildStruct | OpCode::BuildEnum | OpCode::CekEnum | OpCode::CekStruct
            | OpCode::Call | OpCode::LoadFn | OpCode::IterNext
            | OpCode::MakeIterRentang | OpCode::IterAda | OpCode::CobaTambahField
            | OpCode::CobaTumbuhLokal | OpCode::CobaTumbuhUpval | OpCode::CobaTumbuhGlobal | OpCode::SamaTempat
            | OpCode::SimpanBuang | OpCode::IndeksLokal | OpCode::TulisIndexLokal
            | OpCode::TumbuhIndexLokal | OpCode::IterMaju
            | OpCode::GetField
            | OpCode::SetField | OpCode::HasField | OpCode::HofCall
            | OpCode::PinjamLokal | OpCode::PinjamUpval | OpCode::PinjamGlobal
            | OpCode::PinjamField | OpCode::AmbilLokal | OpCode::BongkarTuple
            | OpCode::GetFieldPrivat | OpCode::SetFieldPrivat => {
                write!(f, "{:<16} {}", self.op, self.arg)
            }
            // Nama method di 24 bit bawah, jumlah argumen di 8 bit atas.
            OpCode::HofMethod | OpCode::GetMethod =>
                write!(f, "{:<16} {} argc={}", self.op, self.arg & 0x00FF_FFFF, self.arg >> 24),
            OpCode::IntImm => write!(f, "{:<16} {}", self.op, self.arg as i32),
            _ => write!(f, "{}", self.op),
        }
    }
}
