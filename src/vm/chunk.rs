// src/vm/chunk.rs
// Bytecode Chunk — unit kompilasi satu fungsi JRG

use std::collections::HashMap;
use std::sync::Arc;

use super::opcode::{Instruction, OpCode};
use super::value::VmValue;

/// Unit kompilasi satu fungsi JRG.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Nama fungsi (untuk error + disassembly)
    pub name: String,
    /// Jumlah parameter
    pub arity: u8,
    /// Instruksi
    pub code: Vec<Instruction>,
    /// Constant pool: literal string, float, dll
    pub constants: Vec<VmValue>,
    /// Name pool: nama global, nama field, nama method
    pub names: Vec<String>,
    /// Fungsi nested yang di-compile dari chunk ini
    pub functions: Vec<Chunk>,
    /// Jumlah slot lokal yang dibutuhkan
    pub local_count: usize,
    /// Kolam span.
    pub spans: Vec<crate::lexer::Span>,
    /// Span yang sedang berlaku saat memancarkan instruksi.
    pub span_kini: u32,
    /// Tipe pemilik, kalau chunk ini badan sebuah METHOD (`impl T`).
    pub tipe_self: Option<String>,
    /// Saran nama untuk global yang kemungkinan salah ketik dari sebuah LOKAL.
    /// Hanya diisi kalau memang ada yang mirip, jadi program yang benar
    /// menyimpan nol entri.
    pub saran_lokal: Vec<(String, String)>,
    /// (posisi instruksi `Call`, indeks nama) — nama callee yang DITULIS, dicatat
    /// cuma untuk panggilan yang pasti kelebihan argumen. Program benar: nol entri.
    pub nama_panggil: Vec<(u32, u32)>,
    /// (nama, penjelasan) untuk nama yang DIDEKLARASIKAN tapi bukan nilai (tipe,
    /// trait, varian ber-field, modul) dan disebut sebagai nilai di chunk ini.
    pub bukan_nilai: Vec<(String, String)>,
    /// (kalimat, nomor span) penugasan ke nama yang bukan variabel — pasti gagal kalau
    /// barisnya jalan, jadi `--periksa` melaporkannya tanpa menjalankan.
    pub sasaran_ditolak: Vec<(String, u32)>,
    /// `const`/`let`/`var` tingkat atas milik unit yang dirujuk chunk ini — kalau globalnya
    /// tidak ada saat jalan, barisnya belum dijalankan.
    pub tingkat_atas: Vec<String>,
    /// Indeks ketiga kolam di atas. Dulu dicari linear, jadi kompilasi KUADRAT dalam jumlah
    /// ekspresi (tiap ekspresi punya span unik): 40 ribu baris `wok(i)` 16 detik.
    indeks_span: HashMap<crate::lexer::Span, u32>,
    indeks_nama: HashMap<String, u32>,
    indeks_konstanta: HashMap<KunciKonstanta, u32>,
}

impl Chunk {
    pub fn new(name: impl Into<String>, arity: u8) -> Self {
        Chunk {
            tipe_self:   None,
            saran_lokal: Vec::new(),
            nama_panggil: Vec::new(),
            bukan_nilai: Vec::new(),
            sasaran_ditolak: Vec::new(),
            tingkat_atas: Vec::new(),
            spans:       vec![crate::lexer::Span::dummy()],
            indeks_span: HashMap::from([(crate::lexer::Span::dummy(), 0)]),
            indeks_nama: HashMap::new(),
            indeks_konstanta: HashMap::new(),
            span_kini:   0,
            name:        name.into(),
            arity,
            code:        Vec::new(),
            constants:   Vec::new(),
            names:       Vec::new(),
            functions:   Vec::new(),
            local_count: 0,
        }
    }

    // ── Emit helpers ──────────────────────────────────────────────────────

    /// Daftarkan sebuah span ke kolam, kembalikan indeksnya.
    pub fn span_ix(&mut self, sp: crate::lexer::Span) -> u32 {
        if self.spans.is_empty() {
            // Slot 0 = tidak ada lokasi. Dijaga di sini supaya tidak ada
            // jalur pembuatan `Chunk` yang bisa melewatkannya.
            self.spans.push(crate::lexer::Span::dummy());
            self.indeks_span.insert(crate::lexer::Span::dummy(), 0);
        }
        if let Some(i) = self.indeks_span.get(&sp) {
            return *i;
        }
        let i = self.spans.len() as u32;
        self.spans.push(sp);
        self.indeks_span.insert(sp, i);
        i
    }

    /// Setel span yang berlaku untuk instruksi-instruksi berikutnya, dan
    /// kembalikan yang sebelumnya supaya pemanggil bisa memulihkannya.
    pub fn masuk_span(&mut self, sp: crate::lexer::Span) -> u32 {
        let lama = self.span_kini;
        self.span_kini = self.span_ix(sp);
        lama
    }
    pub fn pulih_span(&mut self, lama: u32) {
        self.span_kini = lama;
    }

    pub fn emit(&mut self, op: OpCode) -> usize {
        let pos = self.code.len();
        let sp = self.span_kini;
        self.code.push(Instruction::simple(op, sp));
        pos
    }

    pub fn emit_arg(&mut self, op: OpCode, arg: u32) -> usize {
        let pos = self.code.len();
        let sp = self.span_kini;
        self.code.push(Instruction::new(op, arg, sp));
        pos
    }

    /// Emit jump placeholder — patch nanti dengan patch_jump
    pub fn emit_jump(&mut self, op: OpCode) -> usize {
        self.emit_arg(op, u32::MAX) // placeholder
    }

    /// Patch jump di posisi `jump_pos` → target current position
    pub fn patch_jump(&mut self, jump_pos: usize) {
        let target = self.code.len() as u32;
        self.code[jump_pos].arg = target;
    }

    /// Emit loop: jump mundur ke `loop_start`
    pub fn emit_loop(&mut self, loop_start: usize) {
        self.emit_arg(OpCode::Jump, loop_start as u32);
    }

    // ── Constant pool ─────────────────────────────────────────────────────

    /// Tambah constant, return index. Yang punya kunci (`kunci_konstanta`) dipakai bersama.
    pub fn add_constant(&mut self, val: VmValue) -> u32 {
        let kunci = kunci_konstanta(&val);
        if let Some(i) = kunci.as_ref().and_then(|k| self.indeks_konstanta.get(k)) {
            return *i;
        }
        let idx = self.constants.len() as u32;
        self.constants.push(val);
        if let Some(k) = kunci { self.indeks_konstanta.insert(k, idx); }
        idx
    }

    // ── Name pool ─────────────────────────────────────────────────────────

    /// Tambah nama ke name pool, return index. Deduplicate.
    pub fn catat_nama_panggil(&mut self, pos: usize, nama: &str) {
        let i = self.add_name(nama);
        self.nama_panggil.push((pos as u32, i));
    }

    pub fn nama_panggil_di(&self, pos: usize) -> Option<&str> {
        let (_, i) = self.nama_panggil.iter().find(|(p, _)| *p as usize == pos)?;
        self.names.get(*i as usize).map(|s| s.as_str())
    }

    pub fn add_name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.indeks_nama.get(name) { return *i; }
        let idx = self.names.len() as u32;
        self.names.push(name.to_string());
        self.indeks_nama.insert(name.to_string(), idx);
        idx
    }

    /// Tambah fungsi nested, return index
    pub fn add_function(&mut self, chunk: Chunk) -> u32 {
        let idx = self.functions.len() as u32;
        self.functions.push(chunk.rampungkan());
        idx
    }

    /// Buang indeks kolam: cuma dipakai selama chunk ini DIBANGUN, dan chunk fungsi
    /// bersarang disalin `LoadFn` tiap kali jalan.
    pub(crate) fn rampungkan(mut self) -> Self {
        self.indeks_span = HashMap::new();
        self.indeks_nama = HashMap::new();
        self.indeks_konstanta = HashMap::new();
        self
    }

    // ── Disassembler ──────────────────────────────────────────────────────

    /// Cetak disassembly ke stdout (untuk --emit-bc / debug)
    pub fn disassemble(&self) {
        println!("╔═══════════════════════════════════════════════════════╗");
        println!("║  Chunk: {:44} ║", self.name);
        println!("║  arity={} locals={} consts={} names={}{}",
            self.arity, self.local_count,
            self.constants.len(), self.names.len(),
            " ".repeat(23));
        println!("╠═══════════════════════════════════════════════════════╣");
        println!("║  {:4}  {:16}  {:5}  {:18} ║", "IP", "OpCode", "Arg", "Annotation");
        println!("╠═══════════════════════════════════════════════════════╣");

        for (i, instr) in self.code.iter().enumerate() {
            let annotation = self.annotate(instr);
            println!("║  {:4}  {:16}  {:5}  {:18} ║",
                i, format!("{:?}", instr.op), instr.arg, annotation);
        }

        println!("╚═══════════════════════════════════════════════════════╝");

        if !self.constants.is_empty() {
            println!("  Constants:");
            for (i, c) in self.constants.iter().enumerate() {
                println!("    [{:3}] {:?}", i, c);
            }
        }
        if !self.names.is_empty() {
            println!("  Names:");
            for (i, n) in self.names.iter().enumerate() {
                println!("    [{:3}] {}", i, n);
            }
        }

        // Recursive: disassemble nested functions
        for f in &self.functions {
            println!();
            f.disassemble();
        }
    }

    fn annotate(&self, instr: &Instruction) -> String {
        match instr.op {
            OpCode::Const => match self.constants.get(instr.arg as usize) {
                // Bawaan dirujuk lewat nilainya; namanya yang berarti bagi pembaca.
                Some(VmValue::NativeFn(f)) => match super::info_native::info(*f) {
                    Some(i) => format!("bawaan {}", i.kanonik),
                    None => "bawaan".to_string(),
                },
                Some(c) => format!("{:?}", c),
                None => String::new(),
            },
            OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefGlobal | OpCode::DefVar
            | OpCode::PinjamGlobal | OpCode::GetField | OpCode::SetField => {
                self.names.get(instr.arg as usize)
                    .cloned().unwrap_or_default()
            }
            OpCode::LoadFn => {
                self.functions.get(instr.arg as usize)
                    .map(|f| f.name.clone()).unwrap_or_default()
            }
            _ => String::new(),
        }
    }
}

/// Kunci dedup konstanta. Float lewat bitnya, jadi `0.0` dan `-0.0` tetap dua konstanta.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KunciKonstanta {
    Int(i128), Float(u64), Str(Arc<String>), Bool(bool), Nil, Native(usize),
    // Prototipe struct dan varian unit tidak berisi apa pun yang bisa ditulis.
    Struct(Arc<str>), Varian(Arc<str>, Arc<str>),
}

fn kunci_konstanta(v: &VmValue) -> Option<KunciKonstanta> {
    use crate::vm::value::Label;
    Some(match v {
        VmValue::Int(x) => KunciKonstanta::Int(*x),
        VmValue::Float(x) => KunciKonstanta::Float(x.to_bits()),
        VmValue::Str(x) => KunciKonstanta::Str(Arc::clone(x)),
        VmValue::Bool(x) => KunciKonstanta::Bool(*x),
        VmValue::Nil => KunciKonstanta::Nil,
        VmValue::NativeFn(f) => KunciKonstanta::Native(*f as usize),
        VmValue::Map(m) if m.lock().map_or(false, |g| g.is_empty()) => match &m.label {
            Label::Struct(n) => KunciKonstanta::Struct(Arc::clone(n)),
            Label::Enum { tipe, varian, payload: None } => {
                KunciKonstanta::Varian(Arc::clone(tipe), Arc::clone(varian))
            }
            _ => return None,
        },
        // Array, fungsi, peta berisi: tidak pernah dipakai bersama.
        _ => return None,
    })
}
