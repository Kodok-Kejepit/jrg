// src/llvm/codegen.rs — jrgc LLVM backend — src/llvm/codegen.rs
// Core codegen state + statement-level lowering. See mod.rs for the

#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::parser::*;
use crate::lexer::Span;

use super::mangle::*;
use super::sema::*;
use super::types::*;

// ─── Small value/codegen-context types ─────────────────────────────────────

/// An already-emitted LLVM operand: either an SSA register (`%v12`) or a
/// literal constant (`42`, `0x3FE...`) — always paired with its JrgType so
/// later codegen steps never have to re-derive "what type is this".
#[derive(Clone, Debug)]
pub struct Val {
    pub text: String,
    pub ty: JrgType,
}

impl Val {
    pub fn unit() -> Self { Val { text: "zeroinitializer".to_string(), ty: JrgType::Unit } }
}

#[derive(Clone)]
pub struct LoopCtx {
    pub label: Option<String>,
    pub continue_bb: String,
    pub break_bb: String,
}

/// Result accumulator threaded through an `if`/`else if`/`else` chain that is
/// being lowered in expression position.
pub(crate) struct IfExprValue {
    pub(crate) slot: Option<(String, JrgType)>,
    pub(crate) saw_valueless_branch: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CodegenOptions {
    pub fast_math: bool,
    pub full_buffered_stdout: bool,
}

/// Keadaan `defer` satu fungsi.
#[derive(Clone)]
pub struct Tunda {
    /// Tinggi tumpukan `defer` runtime saat fungsi ini masuk (nilai `i64`).
    pub dasar: String,
    /// Label blok yang menjalankan daftarnya, satu untuk seluruh fungsi.
    pub blok: String,
    /// `alloca` tempat nilai kembalian ditaruh sebelum melompat ke `blok`.
    /// `None` kalau fungsinya tidak mengembalikan nilai.
    pub slot_ret: Option<String>,
    pub jumlah: usize,
}

/// Fungsi tambahan yang harus dipancarkan, di luar monomorfisasi biasa.
#[derive(Clone)]
pub enum KerjaFn {
    /// Pembungkus supaya fungsi bernama bisa dipakai sebagai NILAI.
    Bungkus {
        pembungkus: String,
        asli: String,
        params: Vec<JrgType>,
        ret: JrgType,
    },
    /// `@jrg_tampil_kotak`: tampilan array/dict yang tersimpan di kotak bertag.
    TampilKotak,
    /// Adaptor nilai fungsi `dari` -> `ke`: argumen dan hasilnya dikoersi (masuk/keluar kotak).
    Adaptor { nama: String, dari: JrgType, ke: JrgType },
    /// Pembanding tiga-arah untuk mengurutkan elemen bertipe `ty`.
    Banding { nama: String, ty: JrgType },
    /// Satu situs `defer`: fungsi `void(ptr)` yang membuka nilai bekunya lalu memanggil.
    Tunda {
        nama: String,
        panggilan: Expr,
        beku: Vec<(String, JrgType)>,
        generik: TypeSubst,
    },
    /// Badan sebuah closure.
    Closure {
        nama: String,
        params: Vec<(String, JrgType)>,
        ret: JrgType,
        body: Expr,
        tangkap: Vec<(String, JrgType)>,
        /// Tipe argumen untuk closure-closure yang DIKEMBALIKAN dari badan ini, satu
        /// tingkat per elemen.
        rantai_hasil: Vec<Vec<JrgType>>,
    },
}

#[derive(Clone)]
pub(crate) struct MonoFn {
    pub(crate) base_name: String,
    pub(crate) mangled: String,
    pub(crate) subst: TypeSubst,
    /// Present for instance methods — (self type, is-a-method-with-self).
    pub(crate) self_ty: Option<JrgType>,
    pub(crate) decl: FnDecl,
    /// Berapa parameter yang benar-benar DATANG sebagai parameter LLVM.
    pub(crate) arity: usize,
}

#[derive(Clone)]
pub struct Codegen {
    pub tables: GlobalTables,
    pub types: TypeTable,
    pub options: CodegenOptions,

    // Option<T>/Result<T,E> instantiation bookkeeping: mangled enum name -> its type argument(s).
    pub option_args: HashMap<String, JrgType>,
    pub result_args: HashMap<String, (JrgType, JrgType)>,

    // ---- module-level output being accumulated ----
    pub(crate) string_dedup: HashMap<String, String>,     // raw bytes -> @global name
    pub(crate) string_globals: Vec<(String, Vec<u8>)>,    // @global name -> raw bytes, in emission order
    pub(crate) declared_externs: HashSet<String>,         // `declare` lines already emitted, by symbol name
    pub(crate) extern_decls: Vec<String>,                 // the `declare ...` lines themselves, in first-seen order
    pub(crate) fn_defs: Vec<String>,                      // fully rendered `define ... { ... }` bodies

    // ---- monomorphization ----
    pub(crate) fn_cache: HashMap<String, ()>,             // mangled fn name -> already emitted or queued
    pub(crate) worklist: VecDeque<MonoFn>,
    /// Fungsi yang harus dipancarkan SESUDAH yang sedang berjalan selesai:
    /// pembungkus nilai-fungsi dan badan closure.
    pub(crate) antrian_fn: VecDeque<KerjaFn>,

    // ---- fresh name counters ----
    pub(crate) next_id: u64,

    // ---- per-function state (reset in `begin_function`) ----
    pub(crate) locals: Vec<HashMap<String, (String, JrgType)>>, // stack of scopes: name -> (ptr register, type)
    pub(crate) cur_label: String,
    pub(crate) cur_lines: Vec<String>,
    pub(crate) entry_allocas: Vec<String>,
    pub(crate) finished_blocks: Vec<(String, Vec<String>)>,
    pub(crate) terminated: bool,
    pub(crate) loop_stack: Vec<LoopCtx>,
    pub(crate) cur_ret_ty: JrgType,
    /// `cur_ret_ty` adalah `()` TEBAKAN, bukan kesimpulan — lihat `periksa_nilai_terbuang`.
    pub(crate) ret_ditebak: bool,
    pub(crate) cur_fn_generics: TypeSubst,
    pub(crate) is_main_fn: bool,

    /// Nama struct yang sedang jadi `self`, kalau fungsi ini sebuah method.
    pub(crate) cur_self_ty: Option<String>,
    /// Simbol closure -> tipe pemilik method tempat ia ditulis.
    pub(crate) ruang_simbol: HashMap<String, String>,

    /// Lokal yang harus jadi array bertumbuh (slice), dihitung SEKALI per
    /// fungsi sebelum badannya diturunkan. Lihat `infer::lokal_yang_tumbuh`.
    pub(crate) lokal_tumbuh: std::collections::HashMap<String, JrgType>,

    /// Span ekspresi/statement yang sedang diturunkan.
    pub(crate) span_kini: Option<Span>,

    /// Keadaan `defer` untuk fungsi yang sedang diturunkan.
    pub tunda: Option<Tunda>,

    /// `var` tingkat modul: nama JRG -> (simbol LLVM, tipe).
    pub tangkapan: std::collections::HashSet<String>,
    /// Tipe argumen pada pemanggilan PERTAMA tiap nama di fungsi ini.
    pub tipe_argumen: HashMap<String, Vec<JrgType>>,
    /// Paruh `Result`/`Option` per (nama fungsi, posisi argumen), dikumpulkan
    /// dari SEMUA situs panggilan di badan yang sedang diturunkan.
    pub paruh_result: super::infer::PetaParuh,
    /// Nama yang sedang diikat oleh `let`/`var` yang nilainya sedang
    /// diturunkan — supaya closure di sisi kanannya tahu ia akan dipanggil
    /// dengan nama apa.
    pub nama_diikat: Option<String>,
    /// Lihat `get_place`: langkah antara jalur tulis tidak boleh menumbuhkan dict.
    pub tulis_antara: bool,
    /// Rantai tipe argumen untuk closure yang dikembalikan dari badan yang
    /// sedang diturunkan. Diisi `pancarkan_closure` dari titipan antrean,
    /// dibaca `closure_dgn` sebagai sumber KEEMPAT tipe parameter.
    pub rantai_hasil: Vec<Vec<JrgType>>,

    /// Tambahan yang ditempelkan ke pesan `coerce` yang gagal, dipasang oleh
    /// situs yang tahu konteksnya.
    pub(crate) catatan_koersi: Option<String>,
    /// Menyala selama `coerce_longgar`: jenis boleh berganti.
    pub(crate) koersi_longgar: bool,
    /// `var` tingkat modul yang dideklarasikan `[]` dan tipe elemennya TIDAK bisa
    /// disimpulkan dari program.
    pub(crate) var_kosong_tak_tersimpulkan: Vec<String>,

    /// Fungsi BERSARANG yang terlihat di sini, satu lapis per blok.
    pub(crate) fn_bersarang: Vec<HashMap<String, (String, Rc<FnDecl>)>>,
    pub(crate) fn_bersarang_ke: u64,
    /// Lingkup fungsi-bersarang yang berlaku untuk sebuah simbol saat ia diantre.
    pub(crate) sarang_fn: HashMap<String, Vec<HashMap<String, (String, Rc<FnDecl>)>>>,
    pub(crate) closure_berikutnya: u64,

    pub var_global: HashMap<String, (String, JrgType)>,
    pub(crate) var_global_baris: Vec<String>,

    pub errors: Vec<String>,

    /// Unit (berkas sumber) yang badannya sedang diturunkan; 0 = berkas utama. Menentukan
    /// nama tingkat atas dan alias mana yang terlihat (Fase MODUL-LLVM).
    pub(crate) unit_kini: usize,
    /// Fungsi dan method yang bisa menulis `var` modul (`tulis_modul`), dihitung sekali.
    pub(crate) penulis_modul: Option<std::rc::Rc<HashSet<String>>>,
    pub(crate) unit_simbol: HashMap<String, usize>,
    pub(crate) unit_berkas: Vec<u32>,
    /// Nomor unit lapis ketiga (`lapis.jrg`), tempat `ns::nama` tanpa penurun asli dicari.
    pub(crate) unit_lapis: Option<usize>,
    pub(crate) penerima_lapis: u64,
    pub(crate) hasil_closure_luruh: bool,
    /// Nama tampil untuk closure berikutnya yang diturunkan (`T::m` sebagai nilai).
    pub(crate) closure_bernama: Option<String>,
    /// Program ini memuat `spawn`/channel/`Future`: `main` menunggu goroutine sebelum selesai.
    pub(crate) berkonkurensi: bool,
    /// Tipe yang pernah masuk kotak sebagai BURAM, bernomor urut; `buram_terpancar` pencetaknya.
    pub(crate) tipe_buram: Vec<JrgType>,
    pub(crate) buram_terpancar: usize,
    pub(crate) pakai_buram: bool,
    /// Pembanding `==` tipe buram: dipancarkan terpisah, cuma kalau dua kotak pernah dibandingkan.
    pub(crate) sama_terpancar: usize,
    /// Penyeberang tipe buram ke jembatan (struct jadi dict bertanda), dipancarkan kalau dipakai.
    pub(crate) jemb_terpancar: usize,
    /// Adaptor nilai fungsi yang sudah dibuat, per pasangan tipe.
    pub(crate) adaptor_ada: HashMap<String, String>,
    pub(crate) pakai_jemb_buram: bool,
    pub(crate) pakai_sama_buram: bool,

    /// Slot kapasitas tiap variabel yang pernah ditumbuhkan di tempat: slot variabel -> slot
    /// `{ pemilik, kap }`. Lihat `codegen_tumbuh.rs`.
    pub(crate) kap_tumbuh: HashMap<String, String>,

    /// `Option<T>` yang NAMANYA sudah dipakai penyimpul statis tapi tata letaknya belum
    /// didaftarkan; diwujudkan `wujudkan_opsi` sebelum tipe itu dipakai penurunan.
    pub(crate) opsi_tertunda: std::cell::RefCell<HashMap<String, JrgType>>,

    /// Penduga tipe kembalian (`duga_tipe_return`): menyala hanya di SALINAN yang
    /// menurunkan satu badan fungsi untuk membaca tipe nilai yang dikembalikannya.
    /// Instans struct generik yang sudah punya tata letak: nama instans -> (nama dasar, argumen).
    pub(crate) struct_instans: HashMap<String, (String, Vec<JrgType>)>,
    /// Lokal yang disemai ke fungsi berikutnya yang diturunkan PENDUGA: tangkapan sebuah closure.
    pub(crate) benih_lokal: Vec<(String, JrgType)>,
    /// `self` method yang sedang diturunkan adalah PENUNJUK ke milik pemanggil (bukan salinan).
    pub(crate) self_dipinjam: bool,
    /// Nama di fungsi ini yang MENUMPANG penyangga pihak lain: parameter yang cuma dibaca,
    /// `self` pinjaman, elemen `for` yang tidak disalin. Yang dikembalikan dari sana disalin.
    pub(crate) pinjaman: std::collections::HashSet<String>,
    /// Nama yang badan closure ini tangkap dari luar: kotaknya dipegang pihak lain juga.
    pub(crate) tangkap_masuk: std::collections::HashSet<String>,
    /// Panggilan method berikutnya memakai varian ber-`self` milik (`x = x.m(..)`).
    pub(crate) milik_berikutnya: bool,
    /// Nama lokal yang sebagai argumen PERTAMA panggilan berikutnya dipindahkan, bukan disalin.
    pub(crate) arg_pindah: Option<String>,
    /// Penduga tipe lengan `match`: kedalaman match yang sedang diturunkan, kedalaman yang
    /// direkam, dan tipe nilai lengan pertama yang terlihat di sana.
    pub(crate) kedalaman_match: u32,
    pub(crate) rekam_lengan: Option<u32>,
    pub(crate) lengan_terlihat: Option<JrgType>,
    /// Pembungkus nilai-fungsi bernama dan nama yang ditampilkannya (`<fn nama>`).
    pub(crate) nama_fnval: Vec<(String, String)>,
    /// Tipe isi kotak yang tak terlukis, bernomor urut daftar (`kata_kotak`).
    pub(crate) jenis_buram: Vec<String>,
    pub(crate) pakai_nama_fn: bool,
    pub(crate) mode_duga: bool,
    pub(crate) hasil_duga: Option<JrgType>,
    /// Separuh `Result` yang sudah terlihat penduga: (`T` dari `Ok(..)`, `E` dari `Err(..)`).
    pub(crate) separuh_duga: (Option<JrgType>, Option<JrgType>),
    /// Galat PERTAMA yang menghentikan penduga per simbol fungsi: sebab sebenarnya ketika
    /// `None`/`Ok`/`Err` di fungsi itu lalu terlihat "butuh anotasi".
    pub(crate) galat_duga: HashMap<String, String>,
    pub(crate) galat_duga_kini: Option<String>,
    pub(crate) sedang_duga: HashSet<String>,
    pub(crate) cache_duga: HashMap<String, Option<JrgType>>,
}

impl Codegen {
    pub fn new(tables: GlobalTables) -> Self {
        Self::new_with_options(tables, CodegenOptions::default())
    }

    pub fn new_with_options(tables: GlobalTables, options: CodegenOptions) -> Self {
        Codegen {
            tables,
            types: TypeTable::new(),
            options,
            option_args: HashMap::new(),
            result_args: HashMap::new(),
            string_dedup: HashMap::new(),
            string_globals: Vec::new(),
            declared_externs: HashSet::new(),
            extern_decls: Vec::new(),
            fn_defs: Vec::new(),
            fn_cache: HashMap::new(),
            worklist: VecDeque::new(),
            antrian_fn: VecDeque::new(),
            next_id: 0,
            locals: Vec::new(),
            cur_label: String::new(),
            tunda: None,
            tangkapan: std::collections::HashSet::new(),
            tipe_argumen: HashMap::new(),
            paruh_result: HashMap::new(),
            nama_diikat: None,
            tulis_antara: false,
            rantai_hasil: Vec::new(),
            catatan_koersi: None,
            koersi_longgar: false,
            var_kosong_tak_tersimpulkan: Vec::new(),
            fn_bersarang: Vec::new(),
            fn_bersarang_ke: 0,
            sarang_fn: HashMap::new(),
            closure_berikutnya: 0,
            var_global: HashMap::new(),
            var_global_baris: Vec::new(),
            cur_lines: Vec::new(),
            entry_allocas: Vec::new(),
            finished_blocks: Vec::new(),
            terminated: false,
            loop_stack: Vec::new(),
            cur_ret_ty: JrgType::Unit,
            ret_ditebak: false,
            cur_fn_generics: TypeSubst::new(),
            is_main_fn: false,
            cur_self_ty: None,
            ruang_simbol: HashMap::new(),
            lokal_tumbuh: std::collections::HashMap::new(),
            span_kini: None,
            errors: Vec::new(),
            unit_kini: 0,
            penulis_modul: None,
            unit_simbol: HashMap::new(),
            unit_berkas: Vec::new(),
            unit_lapis: None,
            penerima_lapis: 0,
            hasil_closure_luruh: false,
            closure_bernama: None,
            berkonkurensi: false,
            tipe_buram: Vec::new(),
            buram_terpancar: 0,
            pakai_buram: false,
            sama_terpancar: 0,
            jemb_terpancar: 0,
            adaptor_ada: HashMap::new(),
            pakai_jemb_buram: false,
            pakai_sama_buram: false,
            kap_tumbuh: HashMap::new(),
            opsi_tertunda: Default::default(),
            struct_instans: HashMap::new(),
            benih_lokal: Vec::new(),
            self_dipinjam: false,
            pinjaman: std::collections::HashSet::new(),
            tangkap_masuk: std::collections::HashSet::new(),
            milik_berikutnya: false,
            arg_pindah: None,
            kedalaman_match: 0,
            rekam_lengan: None,
            lengan_terlihat: None,
            nama_fnval: Vec::new(),
            jenis_buram: Vec::new(),
            pakai_nama_fn: false,
            mode_duga: false,
            hasil_duga: None,
            separuh_duga: (None, None),
            galat_duga: HashMap::new(),
            galat_duga_kini: None,
            sedang_duga: HashSet::new(),
            cache_duga: HashMap::new(),
        }
    }

    /// Catat error dengan lokasi yang diketahui situsnya.
    pub(crate) fn err(&mut self, span: Span, msg: impl Into<String>) {
        let e = self.di_berkas_unit(super::galat::tandai(span, msg.into()));
        self.errors.push(e);
    }

    /// Daftarkan tata letak tiap `Option<T>` yang baru disebut penyimpul statis.
    pub(crate) fn wujudkan_opsi(&mut self) {
        if self.opsi_tertunda.borrow().is_empty() { return; }
        // Yang di DALAM dulu: `Option<Option<int>>` butuh ukuran `Option<int>` yang sudah
        // terdaftar. Urutan `HashMap` acak — dulu, sesekali, isinya terbaca berukuran salah
        // dan `Some(Some(5))` menjawab `1`. Nama isi selalu lebih pendek dari nama pembungkusnya.
        let mut daftar: Vec<(String, JrgType)> = self.opsi_tertunda.borrow_mut().drain().collect();
        daftar.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(&b.0)));
        for (_, t) in daftar { self.instantiate_option(&t); }
    }

    /// Kunci tabel untuk nama tingkat atas `nama` dilihat dari unit yang sedang diturunkan.
    /// Kunci yang sudah ber-awalan (`mN$…`) dikembalikan apa adanya.
    pub(crate) fn k(&self, nama: &str) -> String {
        if self.unit_kini == 0 || nama.contains('$') { return nama.to_string(); }
        super::modul::kunci(self.unit_kini, nama)
    }

    /// Galat dari badan milik modul menunjuk BERKAS modul itu, bukan berkas utama.
    pub(crate) fn di_berkas_unit(&self, e: String) -> String {
        let berkas = self.unit_berkas.get(self.unit_kini).copied().unwrap_or(0);
        super::galat::beri_berkas(e, berkas)
    }

    /// Unit pemilik sebuah simbol: yang dicatat saat diantre, atau awalan `mN$`-nya.
    pub(crate) fn unit_simbol_dari(&self, simbol: &str) -> usize {
        self.unit_simbol.get(simbol).copied().unwrap_or_else(|| super::modul::unit_dari(simbol))
    }

    /// Catat error yang situsnya tidak tahu lokasinya, memakai span
    /// ekspresi/statement yang sedang diturunkan.
    pub(crate) fn lapor(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        // Catatan konteks TIDAK dilekatkan di sini.
        match self.span_kini {
            Some(sp) => {
                let e = self.di_berkas_unit(super::galat::beri_span_bila_kosong(msg, sp));
                self.errors.push(e);
            }
            None => self.errors.push(msg),
        }
    }

    /// `lapor`, plus catatan konteks yang dipasang situs pemanggilnya.
    pub(crate) fn lapor_koersi(&mut self, msg: impl Into<String>) {
        let mut msg: String = msg.into();
        if let Some(c) = self.catatan_koersi.clone() {
            msg.push('\n');
            msg.push_str(&c);
        }
        self.lapor(msg);
    }

    /// Pasang span aktif, kembalikan yang lama supaya bisa dipulihkan.
    pub(crate) fn masuk_span(&mut self, sp: Span) -> Option<Span> {
        self.span_kini.replace(sp)
    }

    pub(crate) fn pulih_span(&mut self, lama: Option<Span>) {
        self.span_kini = lama;
    }

    // ── fresh names ─────────────────────────────────────────────────────

    pub fn fresh(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("%{}{}", prefix, self.next_id)
    }
    pub fn fresh_label(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{}{}", prefix, self.next_id)
    }

    // ── instruction emission / block management ────────────────────────

    pub fn emit(&mut self, line: impl Into<String>) {
        if self.terminated { return; }
        self.cur_lines.push(format!("  {}", line.into()));
    }

    pub fn terminate(&mut self, line: impl Into<String>) {
        if self.terminated { return; }
        self.cur_lines.push(format!("  {}", line.into()));
        self.terminated = true;
    }

    pub fn terminate_unlikely_true(&mut self, cond: &str, true_bb: &str, false_bb: &str) {
        self.terminate(format!(
            "br i1 {}, label %{}, label %{}, !prof !0",
            cond, true_bb, false_bb
        ));
    }

    pub fn terminate_unlikely_false(&mut self, cond: &str, true_bb: &str, false_bb: &str) {
        self.terminate(format!(
            "br i1 {}, label %{}, label %{}, !prof !1",
            cond, true_bb, false_bb
        ));
    }

    /// Force-close the current block (defensive: every block must end in
    /// exactly one terminator) and open a fresh one under `label`.
    pub fn start_block(&mut self, label: impl Into<String>) {
        if !self.terminated {
            self.cur_lines.push("  unreachable".to_string());
        }
        let lbl = std::mem::take(&mut self.cur_label);
        let lines = std::mem::take(&mut self.cur_lines);
        self.finished_blocks.push((lbl, lines));
        self.cur_label = label.into();
        self.terminated = false;
    }

    pub fn is_terminated(&self) -> bool { self.terminated }

    pub fn take_function_body(&mut self) -> String {
        // Flush whatever is still the "current" block into finished_blocks
        // (mirrors start_block's flush logic, minus opening a new block —
        // there's nothing after this, we're done with the function).
        if !self.terminated {
            self.cur_lines.push("  unreachable".to_string());
        }
        let lbl = std::mem::take(&mut self.cur_label);
        let lines = std::mem::take(&mut self.cur_lines);
        self.finished_blocks.push((lbl, lines));

        let mut out = String::new();
        let mut entry_allocas = Some(std::mem::take(&mut self.entry_allocas));
        for (label, lines) in self.finished_blocks.drain(..) {
            out.push_str(&label);
            out.push_str(":\n");
            if label == "entry" {
                for line in entry_allocas.take().unwrap_or_default() {
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            for l in lines {
                out.push_str(&l);
                out.push('\n');
            }
        }
        out
    }

    // ── scopes / locals ─────────────────────────────────────────────────

    pub fn push_scope(&mut self) { self.locals.push(HashMap::new()); }
    pub fn pop_scope(&mut self) { self.locals.pop(); }

    pub fn define_local(&mut self, name: &str, ptr_reg: String, ty: JrgType) {
        self.locals.last_mut().expect("no active scope").insert(name.to_string(), (ptr_reg, ty));
    }

    pub fn lookup_local(&self, name: &str) -> Option<(String, JrgType)> {
        for scope in self.locals.iter().rev() {
            if let Some((p, t)) = scope.get(name) {
                return Some((p.clone(), t.clone()));
            }
        }
        None
    }

    /// alloca a fresh stack slot for `ty` and bind `name` to it in the
    /// current innermost scope.
    pub fn alloca_local(&mut self, name: &str, ty: JrgType) -> String {
        // SATU pintu untuk setiap pengikatan lokal, dan itu disengaja: lokal yang
        // ditangkap closure harus hidup di heap, dan ada belasan tempat yang …
        if self.tangkapan.contains(name) {
            return self.kotak_lokal(name, &ty);
        }
        let reg = self.alloca_typed("local.", &ty);
        self.define_local(name, reg.clone(), ty);
        reg
    }

    pub fn alloca_typed(&mut self, prefix: &str, ty: &JrgType) -> String {
        self.alloca_raw(prefix, &ty.llvm_ty(), Some(self.align_of_hint(ty)))
    }

    pub fn alloca_raw(&mut self, prefix: &str, llvm_ty: &str, align: Option<u64>) -> String {
        let reg = self.fresh(prefix);
        let line = match align {
            Some(value) => format!("  {} = alloca {}, align {}", reg, llvm_ty, value),
            None => format!("  {} = alloca {}", reg, llvm_ty),
        };
        self.entry_allocas.push(line);
        reg
    }

    pub fn fast_math_flag(&self) -> &'static str {
        if self.options.fast_math { " fast" } else { "" }
    }

    pub fn align_of_hint(&self, ty: &JrgType) -> u64 {
        // Only needs to be *a* correct alignment, not byte-perfect vs LLVM's own
        // struct layout (LLVM computes the real one from the type itself); this just …
        match ty {
            JrgType::I128 | JrgType::U128 => 16,
            JrgType::I64 | JrgType::U64 | JrgType::F64 | JrgType::Str | JrgType::Slice(_) => 8,
            JrgType::I32 | JrgType::U32 | JrgType::F32 => 4,
            JrgType::I16 | JrgType::U16 => 2,
            JrgType::Struct(n) => self.types.structs.get(n).map(|_| self.types.alignof(ty)).unwrap_or(8),
            JrgType::Enum(n) => self.types.enums.get(n).map(|_| self.types.alignof(ty)).unwrap_or(8),
            _ => 8,
        }
    }

    // ── globals: string constants ───────────────────────────────────────

    /// Intern a string literal as a module-level constant; returns a Val
    /// of type Str ({ptr,i64}) built directly from the global (no runtime
    /// call needed for literals).
    pub fn intern_string(&mut self, s: &str) -> Val {
        // Nama tipe berlabel (`ruang_tipe`) sampai ke teks yang tercetak tanpa nomornya.
        let s: &str = &crate::ruang_tipe::bersih(s);
        let bytes = s.as_bytes().to_vec();
        let name = if let Some(existing) = self.string_dedup.get(s) {
            existing.clone()
        } else {
            let gname = format!("@.str.{}", self.string_globals.len());
            self.string_globals.push((gname.clone(), bytes.clone()));
            self.string_dedup.insert(s.to_string(), gname.clone());
            gname
        };
        let len = bytes.len();
        let ptrreg = self.fresh("strptr.");
        self.emit(format!(
            "{} = getelementptr [{} x i8], ptr {}, i64 0, i64 0",
            ptrreg, len + 1, name
        ));
        let structreg = self.fresh("strval.");
        self.emit(format!("{} = insertvalue %JrgStr undef, ptr {}, 0", structreg, ptrreg));
        let structreg2 = self.fresh("strval.");
        self.emit(format!("{} = insertvalue %JrgStr {}, i64 {}, 1", structreg2, structreg, len));
        Val { text: structreg2, ty: JrgType::Str }
    }

    // ── extern declarations (runtime + libc) ───────────────────────────

    pub fn ensure_extern(&mut self, sig_line: &str, symbol: &str) {
        if self.declared_externs.insert(symbol.to_string()) {
            let mut line = sig_line.to_string();
            if symbol.starts_with("jrg_") {
                if let Some(rest) = line.strip_prefix("declare ") {
                    line = format!("declare dso_local {}", rest);
                }
                if !line.split_ascii_whitespace().any(|part| part == "nounwind") {
                    line.push_str(" nounwind");
                }
            }
            self.extern_decls.push(line);
        }
    }

    // ── Option<T> / Result<T,E> lazy instantiation ──────────────────────

    pub fn instantiate_option(&mut self, t: &JrgType) -> String {
        let name = mangle_option(t);
        if !self.types.enums.contains_key(&name) {
            let payload_bytes = self.types.sizeof(t);
            let payload_align = self.types.alignof(t).max(1);
            let layout = EnumLayout {
                name: name.clone(),
                variants: vec![
                    EnumVariantLayout { name: "Some".into(), tag: 0, payload: EnumPayload::Tuple(vec![t.clone()]) },
                    EnumVariantLayout { name: "None".into(), tag: 1, payload: EnumPayload::Unit },
                ],
                payload_bytes,
                payload_align,
            };
            self.types.register_enum(layout);
            self.option_args.insert(name.clone(), t.clone());
        }
        name
    }

    pub fn instantiate_result(&mut self, t: &JrgType, e: &JrgType) -> String {
        let name = mangle_result(t, e);
        if !self.types.enums.contains_key(&name) {
            let payload_bytes = self.types.sizeof(t).max(self.types.sizeof(e));
            let payload_align = self.types.alignof(t).max(self.types.alignof(e)).max(1);
            let layout = EnumLayout {
                name: name.clone(),
                variants: vec![
                    EnumVariantLayout { name: "Ok".into(), tag: 0, payload: EnumPayload::Tuple(vec![t.clone()]) },
                    EnumVariantLayout { name: "Err".into(), tag: 1, payload: EnumPayload::Tuple(vec![e.clone()]) },
                ],
                payload_bytes,
                payload_align,
            };
            self.types.register_enum(layout);
            self.result_args.insert(name.clone(), (t.clone(), e.clone()));
        }
        name
    }

    // ── struct / enum layout registration (non-generic user types) ─────

    pub fn register_all_struct_enum_layouts(&mut self) {
        let mut structs: Vec<StructDecl> = self.tables.structs.values().cloned().collect();
        structs.sort_by(|a, b| a.name.cmp(&b.name));
        for s in &structs {
            if !s.generics.params.is_empty() { continue; } // rejected lazily at first use
            // Leluhur yang tidak bisa diratakan ditolak DI SINI, bukan dilewati:
            // layout tanpa field induknya tetap terkompilasi, dan yang keluar
            // bukan galat melainkan angka yang kelihatan masuk akal.
            if let Some((leluhur, sebab)) = self.tables.leluhur_bermasalah(&s.name) {
                let pesan = format!(
                    "struct `{}` mewarisi `{}`, tapi `{}` {} — jadi field warisannya \
                     tidak bisa diratakan\nbantuan: jalankan tanpa `--emit-llvm` \
                     (interpreter) atau dengan `--vm`",
                    s.name, leluhur, leluhur, sebab);
                self.errors.push(crate::llvm::galat::beri_span_bila_kosong(pesan, s.span));
                continue;
            }
            let subst = TypeSubst::new();
            // Field warisan diratakan di SATU pintu ini, jadi ketiga situs yang
            // mencari field menurut nama (`field_index`) tidak perlu tahu apa-apa
            // tentang warisan.
            let daftar = self.tables.field_rata(&s.name);
            let mut fields = Vec::with_capacity(daftar.len());
            let mut ok = true;
            for (pemilik, f) in &daftar {
                // Field `array` TANPA tipe elemen: elemennya disimpan bertag (lapis B) — satu tata
                // letak untuk isi apa pun. Yang ingin kecepatan statis menulis tipe elemennya.
                if is_bare_array_type(&f.ty) {
                    fields.push((f.name.clone(), JrgType::Slice(Box::new(JrgType::Any))));
                    continue;
                }
                match self.resolve_type_with_subst(&f.ty, &subst) {
                    Ok(t) => fields.push((f.name.clone(), t)),
                    Err(e) => {
                        // Konteksnya ditambahkan sebagai `catatan:`, bukan ditempelkan di DEPAN
                        // pesan: `e` bisa sudah membawa penanda `[lo:hi]`, dan menempel di depan …
                        let pesan = format!("{}\ncatatan: dipakai sebagai tipe field `{}` di struct `{}`", e, f.name, pemilik);
                        let sp = f.span;
                        self.errors.push(crate::llvm::galat::beri_span_bila_kosong(pesan, sp));
                        ok = false;
                    }
                }
            }
            if ok {
                self.types.register_struct(StructLayout { name: s.name.clone(), fields });
            }
        }

        let mut enums: Vec<EnumDecl> = self.tables.enums.values().cloned().collect();
        enums.sort_by(|a, b| a.name.cmp(&b.name));
        for en in &enums {
            if !en.generics.params.is_empty() { continue; }
            let subst = TypeSubst::new();
            let mut variants = Vec::with_capacity(en.variants.len());
            let mut max_bytes = 0u64;
            let mut max_align = 1u64;
            let mut ok = true;
            for (i, v) in en.variants.iter().enumerate() {
                let payload = match &v.kind {
                    EnumVariantKind::Unit => EnumPayload::Unit,
                    EnumVariantKind::Tuple(tys) => {
                        let mut out = Vec::with_capacity(tys.len());
                        for t in tys {
                            match self.resolve_type_with_subst(t, &subst) {
                                Ok(rt) => out.push(rt),
                                Err(e) => {
                                    let pesan = format!("{}\ncatatan: dipakai di variant `{}::{}`", e, en.name, v.name);
                                    let sp = v.span;
                                    self.errors.push(crate::llvm::galat::beri_span_bila_kosong(pesan, sp));
                                    ok = false;
                                }
                            }
                        }
                        EnumPayload::Tuple(out)
                    }
                    EnumVariantKind::Struct(fields) => {
                        let mut out = Vec::with_capacity(fields.len());
                        for f in fields {
                            match self.resolve_type_with_subst(&f.ty, &subst) {
                                Ok(rt) => out.push((f.name.clone(), rt)),
                                Err(e) => {
                                    let pesan = format!("{}\ncatatan: dipakai sebagai tipe field `{}` di variant `{}::{}`", e, f.name, en.name, v.name);
                                    let sp = f.span;
                                    self.errors.push(crate::llvm::galat::beri_span_bila_kosong(pesan, sp));
                                    ok = false;
                                }
                            }
                        }
                        EnumPayload::Struct(out)
                    }
                };
                let struct_ty = payload_as_jrgtype(&payload);
                max_bytes = max_bytes.max(self.types.sizeof(&struct_ty));
                max_align = max_align.max(self.types.alignof(&struct_ty));
                variants.push(EnumVariantLayout { name: v.name.clone(), tag: i as u32, payload });
            }
            if ok {
                self.types.register_enum(EnumLayout {
                    name: en.name.clone(), variants, payload_bytes: max_bytes, payload_align: max_align.max(1),
                });
            }
        }

        // A concrete Option/Result can be discovered while lowering a struct that
        // refers to a type declared later in the source.
        self.recompute_enum_layouts();
    }

    pub(crate) fn recompute_enum_layouts(&mut self) {
        let max_rounds = self.types.enums.len().saturating_add(1);
        for _ in 0..max_rounds {
            let names: Vec<String> = self.types.enums.keys().cloned().collect();
            let mut changed = false;
            for name in names {
                let Some(layout) = self.types.enums.get(&name).cloned() else {
                    continue;
                };
                let mut payload_bytes = 0u64;
                let mut payload_align = 1u64;
                for variant in &layout.variants {
                    let payload_ty = payload_as_jrgtype(&variant.payload);
                    payload_bytes = payload_bytes.max(self.types.sizeof(&payload_ty));
                    payload_align = payload_align.max(self.types.alignof(&payload_ty));
                }
                if let Some(existing) = self.types.enums.get_mut(&name) {
                    if existing.payload_bytes != payload_bytes
                        || existing.payload_align != payload_align.max(1)
                    {
                        existing.payload_bytes = payload_bytes;
                        existing.payload_align = payload_align.max(1);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    // ── entry point / monomorphization drain ────────────────────────────

    pub fn enqueue_fn(&mut self, base_name: &str, decl: &FnDecl, subst: TypeSubst, mangled: String, self_ty: Option<JrgType>) {
        let n = decl.params.len();
        self.enqueue_fn_arity(base_name, decl, subst, mangled, self_ty, n);
    }

    pub fn enqueue_fn_arity(&mut self, base_name: &str, decl: &FnDecl, subst: TypeSubst,
                            mangled: String, self_ty: Option<JrgType>, arity: usize) {
        if self.fn_cache.contains_key(&mangled) { return; }
        // Unit pemiliknya: awalan `mN$`, atau — untuk `fn` bersarang — unit yang sedang berjalan.
        // Method sudah dicatat situs pemanggilnya menurut tipe penerimanya.
        let u = super::modul::unit_dari(&mangled);
        let u = if u > 0 { u } else { self.unit_kini };
        self.unit_simbol.entry(mangled.clone()).or_insert(u);
        self.fn_cache.insert(mangled.clone(), ());
        self.worklist.push_back(MonoFn {
            base_name: base_name.to_string(), mangled, subst, self_ty, decl: decl.clone(), arity });
    }

    /// Catat sebuah nama sebagai sudah-diantre, supaya ia tidak dipancarkan dua
    /// kali.
    pub fn mulai_fungsi_kosong(&mut self, ret: JrgType) {
        self.locals.clear();
        self.push_scope();
        self.finished_blocks.clear();
        self.cur_lines.clear();
        self.entry_allocas.clear();
        self.cur_label = "entry".to_string();
        self.terminated = false;
        self.loop_stack.clear();
        self.cur_fn_generics = TypeSubst::new();
        self.is_main_fn = false;
        self.cur_self_ty = None;
        self.cur_ret_ty = ret;
        self.tunda = None;
        self.lokal_tumbuh = HashMap::new();
        self.tangkapan = std::collections::HashSet::new();
        self.pinjaman.clear();
        self.self_dipinjam = false;
    }

    pub fn selesai_fungsi_kosong(&mut self) {
        self.pop_scope();
    }

    /// Panik saat jalan dengan `pesan`, lalu blok ini selesai.
    pub fn panik(&mut self, pesan: &str) {
        self.ensure_extern(
            "declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        let p = self.intern_string(pesan);
        let (mp, ml) = self.split_str(&p);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());
    }

    pub fn nama_closure_baru(&mut self) -> String {
        self.closure_berikutnya += 1;
        format!("clo.{}", self.closure_berikutnya)
    }

    pub fn tandai_fn_terpakai(&mut self, mangled: &str) {
        self.fn_cache.insert(mangled.to_string(), ());
    }

    /// Tambahkan sebuah `define ...` jadi ke keluaran modul.
    pub fn tambah_definisi(&mut self, def: String) {
        self.fn_defs.push(def);
    }

    pub fn is_fn_emitted_or_queued(&self, mangled: &str) -> bool {
        self.fn_cache.contains_key(mangled)
    }

    pub fn antre_fn(&mut self, k: KerjaFn) {
        if let KerjaFn::Closure { nama, .. } | KerjaFn::Tunda { nama, .. } = &k {
            self.unit_simbol.insert(nama.clone(), self.unit_kini);
            // Closure membawa ruang (tipe pemilik method) penciptanya, untuk aturan field privat.
            if let Some(t) = self.cur_self_ty.clone() { self.ruang_simbol.insert(nama.clone(), t); }
        }
        self.antrian_fn.push_back(k);
    }

    pub fn drain_worklist(&mut self) {
        loop {
            self.kuras_mono();
            match self.antrian_fn.pop_front() {
                // Antrean habis: pencetak tipe buram yang belum ada dipancarkan — ia bisa menambah
                // pekerjaan (dan tipe buram) baru, jadi putarannya diulang sampai tenang.
                None if self.pakai_buram && self.buram_terpancar < self.tipe_buram.len() => {
                    let no = self.buram_terpancar;
                    self.buram_terpancar += 1;
                    if let Err(e) = self.pancarkan_tampil_buram(no) { self.errors.push(e); }
                }
                None if self.pakai_jemb_buram && self.jemb_terpancar < self.tipe_buram.len() => {
                    let no = self.jemb_terpancar;
                    self.jemb_terpancar += 1;
                    if let Err(e) = self.pancarkan_buram_jembatan(no) { self.errors.push(e); }
                }
                None if self.pakai_sama_buram && self.sama_terpancar < self.tipe_buram.len() => {
                    let no = self.sama_terpancar;
                    self.sama_terpancar += 1;
                    self.pancarkan_sama_buram(no);
                }
                None => break,
                Some(k) => {
                    if let KerjaFn::Closure { nama, .. } | KerjaFn::Tunda { nama, .. } = &k {
                        self.unit_kini = self.unit_simbol_dari(nama);
                    }
                    if let Err(e) = self.pancarkan_kerja_fn(k) {
                        let e = self.di_berkas_unit(e);
                        self.errors.push(e);
                    }
                }
            }
            if self.errors.len() > 200 { break; }
        }
    }

    pub(crate) fn kuras_mono(&mut self) {
        while let Some(job) = self.worklist.pop_front() {
            if let Err(e) = self.codegen_function(&job.base_name, &job.decl, &job.subst, &job.mangled, job.self_ty.clone(), job.arity) {
                let e = self.di_berkas_unit(e);
                self.errors.push(e);
            }
            if self.errors.len() > 200 { break; } // don't runaway on a fundamentally broken program
        }
    }

    /// Assemble every accumulated piece (fixed runtime-ABI types, struct/
    /// enum layouts, interned string constants, extern declarations, and
    /// function bodies) into one complete `.ll` module.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("; Generated by jrgc --emit-llvm — the JRG LLVM backend.\n");
        out.push_str("; Do not edit by hand; regenerate from the .jrg source instead.\n\n");
        out.push_str("%JrgStr = type { ptr, i64 }\n");
        out.push_str("%JrgSlice = type { ptr, i64 }\n");
        out.push_str("%JrgDict = type { ptr, i64 }\n");
        // Nilai bertag. Tag di offset 0, muatan di 8 — dijajarkan 8 supaya
        // `%JrgStr`/`%JrgSlice`/`%JrgDict` (semuanya `{ptr, i64}`) bisa
        // disimpan ke dalamnya tanpa penyalinan bita per bita.
        out.push_str("%JrgAny = type { i32, [2 x i64] }\n");
        // Nilai fungsi: kode dan lingkungan. Lingkungan `null` berarti fungsi
        // BERNAMA — itu yang membedakannya dari closure saat `typeof` ditanya.
        out.push_str("%JrgFn = type { ptr, ptr }\n");
        for name in &self.types.emit_order {
            if let Some(s) = self.types.structs.get(name) {
                let fields: Vec<String> = s.fields.iter().map(|(_, t)| t.llvm_ty()).collect();
                out.push_str(&format!("%{} = type {{ {} }}\n", llvm_quoted_ident(name), fields.join(", ")));
            } else if let Some(e) = self.types.enums.get(name) {
                out.push_str(&e.llvm_struct_def());
                out.push('\n');
            }
        }
        // `var` tingkat modul SESUDAH definisi tipe: `@gv.G = global %"V" …` butuh `%"V"` sudah dikenal.
        for b in &self.var_global_baris {
            out.push_str(b);
            out.push('\n');
        }
        out.push('\n');
        for (gname, bytes) in &self.string_globals {
            let escaped = llvm_escape(bytes);
            out.push_str(&format!("{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n", gname, bytes.len() + 1, escaped));
        }
        out.push('\n');
        for d in &self.extern_decls {
            out.push_str(d);
            out.push('\n');
        }
        out.push('\n');
        for def in &self.fn_defs {
            out.push_str(def);
            out.push('\n');
        }
        if self.pakai_nama_fn {
            out.push_str(&self.definisi_nama_fn());
        }
        if self.pakai_buram {
            out.push_str(&self.definisi_buram());
        }
        if self.pakai_sama_buram {
            out.push_str(&self.definisi_sama_buram());
        }
        if self.pakai_jemb_buram {
            out.push_str(&self.definisi_buram_jembatan());
        }
        out.push_str("!0 = !{!\"branch_weights\", i32 1, i32 2000}\n");
        out.push_str("!1 = !{!\"branch_weights\", i32 2000, i32 1}\n");
        out
    }

}


pub(crate) fn payload_as_jrgtype(p: &EnumPayload) -> JrgType {
    JrgType::Tuple(p.field_types())
}

pub(crate) fn pattern_simple_name(p: &Pattern) -> Option<String> {
    match p {
        Pattern::Ident { name, .. } => Some(name.clone()),
        Pattern::Ref { inner, .. } => pattern_simple_name(inner),
        _ => None,
    }
}

pub(crate) fn is_bare_array_type(te: &TypeExpr) -> bool {
    matches!(te, TypeExpr::Named(NamedType { name, args, .. }) if name == "array" && args.is_empty())
}

pub fn compound_to_binop(op: &AssignOp) -> BinaryOp {
    match op {
        AssignOp::PlusEq => BinaryOp::Add,
        AssignOp::PowEq  => BinaryOp::Pow,
        AssignOp::MinusEq => BinaryOp::Sub,
        AssignOp::StarEq => BinaryOp::Mul,
        AssignOp::SlashEq => BinaryOp::Div,
        AssignOp::PercentEq => BinaryOp::Rem,
        AssignOp::AndEq => BinaryOp::BitAnd,
        AssignOp::OrEq => BinaryOp::BitOr,
        AssignOp::CaretEq => BinaryOp::BitXor,
        AssignOp::ShlEq => BinaryOp::Shl,
        AssignOp::ShrEq => BinaryOp::Shr,
        AssignOp::Eq => unreachable!("Eq is not compound"),
    }
}
