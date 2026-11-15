// src/llvm/codegen_polimorf.rs — method milik program yang dipanggil atas nilai bertag
// (`[Circle { .. }, Square { .. }]` lalu `b.luas()`): tipe isinya dipilih saat jalan.

use crate::lexer::Span;
use super::codegen::{Codegen, Val};
use super::codegen_any::TAG_BURAM;
use super::types::JrgType;

/// Satu method yang dipanggil atas nilai bertag: badannya dipancarkan per tipe buram.
#[derive(Clone, Debug)]
pub(crate) struct MetodeDinamis {
    nama: String,
    n: usize,
    /// Berapa tipe buram yang sudah ditimbang, dan nomor yang memang punya method-nya.
    terpancar: usize,
    ada: Vec<usize>,
    gagal: bool,
}

fn nol() -> Span { Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 } }

fn simbol(nama: &str, n: usize) -> String {
    let aman: String = nama.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    format!("md.{}.{}.{}", aman, nama.len(), n)
}

impl Codegen {
    /// Ada tipe milik program yang punya method `nama` ber-`self` dengan `n` argumen lain.
    pub(crate) fn ada_metode_program(&self, nama: &str, n: usize) -> bool {
        self.tables.impls.values().filter_map(|m| m.get(nama)).any(|d| d.params.len() == n + 1 && Self::ber_self(d))
    }

    fn ber_self(d: &crate::parser::FnDecl) -> bool {
        d.params.first().and_then(|p| super::codegen::pattern_simple_name(&p.pattern)).as_deref() == Some("self")
    }

    pub(crate) fn ada_metode_dinamis_tertunda(&self) -> bool {
        self.metode_dinamis.iter().any(|m| !m.gagal || m.terpancar < self.tipe_buram.len())
    }

    /// `x.m(args)` atas nilai bertag: argumennya dikotak, lalu `@md.m.N` memilih badan per tipe.
    pub(crate) fn panggil_metode_dinamis(&mut self, v: &Val, nama: &str, args: &[crate::parser::Expr]) -> Result<Val, String> {
        if !self.metode_dinamis.iter().any(|m| m.nama == nama && m.n == args.len()) {
            self.metode_dinamis.push(MetodeDinamis { nama: nama.to_string(), n: args.len(), terpancar: 0, ada: Vec::new(), gagal: false });
        }
        self.pakai_buram = true;
        self.ensure_extern("declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        let larik = self.alloca_raw("mdarg.", &format!("[{} x %JrgAny]", args.len().max(1)), Some(8));
        for (i, a) in args.iter().enumerate() {
            let x = self.codegen_expr(a, None)?;
            let k = self.coerce(&x, &JrgType::Any);
            let p = self.fresh("mda.");
            self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i64 {}", p, larik, i));
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, p));
        }
        let r = self.fresh("mdr.");
        self.emit(format!("{} = call %JrgAny @{}(%JrgAny {}, ptr {})", r, simbol(nama, args.len()), v.text, larik));
        Ok(Val { text: r, ty: JrgType::Any })
    }

    /// Method bawaan teks/array/dict atas nilai bertag (`str::len(x)`, `x.upper()`): golongannya dibaca
    /// saat jalan dan dilayani penurun aslinya; golongan lain lewat method program atau jembatan.
    pub(crate) fn metode_bawaan_bertag(&mut self, v: &Val, nama: &str, args: &[crate::parser::Expr], span: Span)
        -> Option<Result<Val, String>>
    {
        use super::codegen_any::{TAG_DICT, TAG_SLICE, TAG_STR};
        let pseudo = matches!(nama, "len" | "is_empty") && args.is_empty();
        let golongan: Vec<(i32, JrgType)> = [(TAG_STR, JrgType::Str, "str"),
            (TAG_SLICE, JrgType::Slice(Box::new(JrgType::Any)), "arr"), (TAG_DICT, JrgType::Dict(Box::new(JrgType::Any)), "dict")]
            .into_iter().filter(|(_, _, ns)| pseudo || crate::vm::ada_builtin(&format!("{}::{}", ns, nama)))
            .map(|(t, ty, _)| (t, ty)).collect();
        // `Atomic`/`Mutex` sama-sama buram: dibedakan kata-tag utuhnya, bukan bita tagnya.
        let mut golongan = golongan;
        for (k, ns) in [(false, "atomic"), (true, "mutex")] {
            if crate::sinkron::metode_ada(ns, nama) { golongan.push((-1, JrgType::Sinkron(k))); }
        }
        if golongan.is_empty() { return None; }
        Some(self.metode_bawaan_bertag_dgn(v, nama, args, &golongan, span))
    }

    fn metode_bawaan_bertag_dgn(&mut self, v: &Val, nama: &str, args: &[crate::parser::Expr],
                                golongan: &[(i32, JrgType)], span: Span) -> Result<Val, String> {
        let slot = self.alloca_typed("mbt.", &JrgType::Any);
        let kata = self.fresh("mbt.");
        self.emit(format!("{} = extractvalue %JrgAny {}, 0", kata, v.text));
        let tag = self.fresh("mbt.");
        self.emit(format!("{} = and i32 {}, 255", tag, kata));
        let (lain, akhir) = (self.fresh_label("mbt.lain."), self.fresh_label("mbt.end."));
        let label: Vec<String> = golongan.iter().map(|_| self.fresh_label("mbt.g.")).collect();
        for ((t, ty), l) in golongan.iter().zip(&label) {
            if *t >= 0 { continue; }
            let k = self.kata_buram(ty);
            let (c, terus) = (self.fresh("mbt."), self.fresh_label("mbt.b."));
            self.emit(format!("{} = icmp eq i32 {}, {}", c, kata, k));
            self.terminate(format!("br i1 {}, label %{}, label %{}", c, l, terus));
            self.start_block(terus);
        }
        let kasus: Vec<String> = golongan.iter().zip(&label).filter(|((t, _), _)| *t >= 0)
            .map(|((t, _), l)| format!("i32 {}, label %{}", t, l)).collect();
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag, lain, kasus.join(" ")));
        for ((_, ty), l) in golongan.iter().zip(label) {
            self.start_block(l);
            let x = self.buka_any(v, ty, span)?;
            let tempat = self.spill_to_place(&x);
            let o = self.lokal_sementara(tempat, ty.clone(), span);
            // Dicoba di salinan dulu: golongan yang penurunnya belum ada gagal SAAT JALAN, bukan menolak programnya.
            let jadi = std::mem::take(&mut self.fn_defs);
            let mut coba = self.clone();
            self.fn_defs = jadi;
            coba.mode_duga = true;
            // Galat koersi DIKUMPULKAN (`lapor`), tidak dikembalikan: yang lolos cuma yang dua-duanya bersih.
            let sebelum = coba.errors.len();
            if coba.codegen_method(&o, nama, args, None, span).is_ok() && coba.errors.len() == sebelum {
                let r = self.codegen_method(&o, nama, args, None, span)?;
                if !self.is_terminated() {
                    let r = if matches!(r.ty, JrgType::Unit | JrgType::Never) { Val::unit() } else { r };
                    let k = self.coerce(&r, &JrgType::Any);
                    self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
                    self.terminate(format!("br label %{}", akhir));
                }
            } else {
                self.panik(&format!("method `{}` atas `{}` di dalam nilai bertag belum didukung backend `--emit-llvm` \
                    — ini celah backend, bukan salah programmu", nama, ty.nama_typeof()));
            }
        }
        self.start_block(lain);
        let r = if self.ada_metode_program(nama, args.len()) { self.panggil_metode_dinamis(v, nama, args)? }
                else { self.metode_titipan(v, nama, args, span)? };
        if !self.is_terminated() {
            let k = self.coerce(&r, &JrgType::Any);
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
            self.terminate(format!("br label %{}", akhir));
        }
        self.start_block(akhir);
        Ok(self.load_place(&slot, &JrgType::Any))
    }

    /// Satu langkah pekerjaan: fungsi gagal sebuah method, atau badannya untuk satu tipe buram lagi.
    pub(crate) fn pancarkan_metode_dinamis(&mut self) -> Result<(), String> {
        let Some(k) = self.metode_dinamis.iter().position(|m| !m.gagal || m.terpancar < self.tipe_buram.len()) else { return Ok(()) };
        let m = self.metode_dinamis[k].clone();
        if !m.gagal {
            self.metode_dinamis[k].gagal = true;
            self.mulai_fungsi_kosong(JrgType::Any);
            let kotak = Val { text: "%k".to_string(), ty: JrgType::Any };
            let nama = m.nama.clone();
            let lama = std::mem::replace(&mut self.warisi_situs, true);
            self.panik_tipe_any(&kotak, |t| crate::interpreter::pesan_method_tak_ada(t, &nama, "", None));
            self.warisi_situs = lama;
            let badan = self.take_function_body();
            self.tambah_definisi(format!("define internal void @{}.gagal(%JrgAny %k) cold {{\n{}}}\n", simbol(&m.nama, m.n), badan));
            self.selesai_fungsi_kosong();
            return Ok(());
        }
        let no = m.terpancar;
        self.metode_dinamis[k].terpancar += 1;
        let t = self.tipe_buram[no].clone();
        let tipe = match &t { JrgType::Struct(x) | JrgType::Enum(x) => x.clone(), _ => return Ok(()) };
        let Some(decl) = self.tables.find_method(&tipe, &m.nama).cloned() else { return Ok(()) };
        if decl.params.len() != m.n + 1 || !Self::ber_self(&decl) { return Ok(()); }
        self.mulai_fungsi_kosong(JrgType::Any);
        let mut args = Vec::with_capacity(m.n);
        for i in 0..m.n {
            let p = self.fresh("mdp.");
            self.emit(format!("{} = getelementptr %JrgAny, ptr %a, i64 {}", p, i));
            args.push(self.lokal_sementara(p, JrgType::Any, nol()));
        }
        let hasil = match self.call_method_impl(&tipe, &decl, "%p", &t, &args, nol()) {
            Ok(h) => h,
            Err(e) => { let _ = self.take_function_body(); self.selesai_fungsi_kosong(); return Err(e); }
        };
        let hasil = if matches!(hasil.ty, JrgType::Unit | JrgType::Never) { Val::unit() } else { hasil };
        let kotak = self.coerce(&hasil, &JrgType::Any);
        if !self.is_terminated() { self.terminate(format!("ret %JrgAny {}", kotak.text)); }
        let badan = self.take_function_body();
        self.tambah_definisi(format!("define internal %JrgAny @{}.{}(ptr %p, ptr %a) {{\n{}}}\n", simbol(&m.nama, m.n), no + 1, badan));
        self.selesai_fungsi_kosong();
        self.metode_dinamis[k].ada.push(no);
        Ok(())
    }

    /// Kalimat acuan untuk struct/enum milik program yang tidak punya method `nama`, dihitung saat
    /// kompilasi: tipe, field yang ADA, dan saran ejaan, sama dengan dua mesin lain.
    pub(crate) fn pesan_tak_punya(&self, t: &JrgType, nama: &str) -> Option<String> {
        let (label, kunci) = match t {
            JrgType::Struct(n) => (format!("struct {}", super::modul::tampil(super::generik::dasar(n))), n),
            JrgType::Enum(n) => (format!("enum:{}", super::modul::tampil(super::generik::dasar(n))), n),
            _ => return None,
        };
        // Method warisan ikut jadi calon saran, seperti `lineage_of` di acuan.
        let mut calon: Vec<String> = self.tables.silsilah(super::generik::dasar(kunci)).iter()
            .filter_map(|t| self.tables.impls.get(t)).flat_map(|m| m.keys().cloned()).collect();
        let mut tambahan = String::new();
        if let JrgType::Struct(n) = t {
            let mut punya: Vec<String> = self.types.structs.get(n).map(|l| l.fields.iter().map(|(f, _)| f.clone()).collect()).unwrap_or_default();
            punya.sort();
            if !punya.is_empty() { tambahan = format!(" (kalau maksudnya field/key, yang ada: {})", punya.join(", ")); }
        } else {
            calon.extend(crate::interpreter::calon_method_enum(super::modul::tampil(kunci)).iter().map(|s| s.to_string()));
        }
        let saran = crate::diag::saran_nama_terdekat(nama, calon.iter().map(|s| s.as_str()));
        Some(crate::interpreter::pesan_method_tak_ada(&label, nama, &tambahan, saran.as_deref()))
    }

    /// `@md.m.N(kotak, args)`: pilih badan menurut kata-tag; tipe tanpa method itu gagal dengan kalimat acuan.
    pub(crate) fn definisi_metode_dinamis(&self) -> String {
        let mut out = String::new();
        for (j, m) in self.metode_dinamis.iter().enumerate() {
            let s = simbol(&m.nama, m.n);
            let mut kepala = format!("define internal %JrgAny @{}(%JrgAny %k, ptr %a) {{\nentry:\n  %w = extractvalue %JrgAny %k, 0\n  \
                %p0 = extractvalue %JrgAny %k, 1, 0\n  %p = inttoptr i64 %p0 to ptr\n  switch i32 %w, label %lain [", s);
            let mut badan = String::new();
            for (no, t) in self.tipe_buram.iter().enumerate() {
                let kata = TAG_BURAM | ((no as i32 + 1) << 8);
                if m.ada.contains(&no) {
                    kepala.push_str(&format!(" i32 {}, label %b{}", kata, no + 1));
                    badan.push_str(&format!("b{0}:\n  %r{0} = call %JrgAny @{1}.{0}(ptr %p, ptr %a)\n  ret %JrgAny %r{0}\n", no + 1, s));
                } else if let Some(pesan) = self.pesan_tak_punya(t, &m.nama) {
                    out.push_str(&format!("@.mdg.{}.{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                        j, no + 1, pesan.len(), super::mangle::llvm_escape(pesan.as_bytes())));
                    kepala.push_str(&format!(" i32 {}, label %g{}", kata, no + 1));
                    badan.push_str(&format!("g{0}:\n  call void @jrg_panic(ptr @.mdg.{1}.{0}, i64 {2})\n  unreachable\n", no + 1, j, pesan.len()));
                }
            }
            out.push_str(&format!("{} ]\n{}lain:\n  call void @{}.gagal(%JrgAny %k)\n  unreachable\n}}\n", kepala, badan, s));
        }
        out
    }
}
