// src/llvm/codegen_jawa.rs — llvm/codegen_jawa.rs — `jawa::` / `jawir::`, alokasi manual.

use crate::lexer::Span;
use crate::parser::Expr;

use super::codegen::{Codegen, Val};
use super::types::JrgType;

impl Codegen {
    /// Layani `jawa::…` / `jawir::…`. `None` kalau namanya bukan milik
    /// keduanya.
    pub fn coba_builtin_jawa(
        &mut self, nama: &str, args: &[Expr], span: Span,
    ) -> Option<Result<Val, String>> {
        // Bentuk telanjang: `jawa(v)` == `jawa::alloc(v)`, `jawir(p)` ==
        // `jawir::free(p)`. Diukur; keduanya sah di acuannya.
        let item = match nama {
            "jawa" => "alloc",
            "jawir" => "free",
            // Nama yang tidak ada di acuan (`jawa::free`, `jawir::alloc`) bukan milik pintu ini:
            // dulu kedua awalan diterima untuk semua nama.
            _ if !crate::vm::ada_builtin(nama) => return None,
            _ => nama.strip_prefix("jawa::").or_else(|| nama.strip_prefix("jawir::"))?,
        };
        // Registry `jawa::` milik runtime biner ini: jembatan akan membaca registry `jrgc` yang kosong.
        if item == "stats" {
            if !args.is_empty() { return None; }
            return Some(self.jawa_stats(span));
        }
        // Arity diperiksa sebelum dispatch: `None` berarti "bukan namespace
        // ini", bukan "argumennya salah".
        let arity = match item {
            "alloc" | "get" | "is_valid" | "free" | "array" | "array_len"
            | "buffer" | "buffer_len" | "array_pop" | "array_to_list" | "buffer_to_hex" | "sizeof" => 1,
            "set" | "array_get" | "array_push" | "buffer_get" | "buffer_fill" => 2,
            "array_set" | "buffer_set" => 3,
            _ => return None,
        };
        // `jawa::array(n, isi)`: isi awal opsional, seperti acuannya.
        if args.len() != arity && !(item == "array" && args.len() == 2) {
            return Some(Err(super::galat::tandai(span, format!(
                "`{}` butuh {} argumen, diberi {}", nama, arity, args.len()))));
        }
        Some(self.jawa(item, args, span))
    }

    fn jawa(&mut self, item: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        // Argumen ke-0 sebuah handle untuk semua kecuali fungsi pembuat.
        let pembuat = matches!(item, "alloc" | "array" | "buffer");
        let mut nilai: Vec<String> = Vec::with_capacity(args.len());
        let mut pegangan = None;
        for (i, a) in args.iter().enumerate() {
            if i == 0 && !pembuat {
                let h = self.codegen_expr(a, None)?;
                // `jawa::sizeof` atas nilai yang bukan pegangan menaksir ukuran nilainya, seperti
                // dua mesin lain: lewat jembatan (taksirannya tidak menyentuh registry `jawa::`).
                if item == "sizeof" && !matches!(&h.ty, JrgType::Struct(n) if n == "Ptr") {
                    let slot = self.alloca_typed("jws.", &h.ty);
                    self.emit(format!("store {} {}, ptr {}", h.ty.llvm_ty(), h.text, slot));
                    let e = self.lokal_sementara(slot, h.ty.clone(), span);
                    return self.panggil_jembatan("jawa::sizeof", None, None, &[e], span);
                }
                pegangan = Some(h);
            } else {
                let v = self.codegen_expr(a, Some(&JrgType::I64))?;
                if !v.ty.is_int() {
                    return Err(super::galat::tandai(span, format!(
                        "`jawa::{}` di backend LLVM cuma menyimpan bilangan bulat, \
                         diberi `{}`\n\
                         catatan: interpreter menyimpan nilai apa pun; di sini registrinya \
                         bertipe tunggal, dan menerima nilai lain berarti menyimpan bit yang \
                         ditafsirkan sebagai bilangan", item, v.ty.display_name())));
                }
                nilai.push(self.coerce_longgar(&v, &JrgType::I64).text);
            }
        }
        // Pegangan diperiksa sesudah semua argumen dievaluasi, seperti acuan.
        if let Some(h) = pegangan {
            let ns = if item == "free" { "jawir" } else { "jawa" };
            let id = self.id_handle(&h, &format!("{}::{}", ns, item), span)?;
            nilai.insert(0, id);
        }

        let (fungsi, hasil) = match item {
            "alloc"      => ("jrg_jawa_alloc", Hasil::Handle),
            "get"        => ("jrg_jawa_get", Hasil::Int),
            "set"        => ("jrg_jawa_set", Hasil::Kosong),
            "is_valid"   => ("jrg_jawa_valid", Hasil::Bool),
            "free"       => ("jrg_jawa_free", Hasil::Kosong),
            "array" if nilai.len() == 2 => ("jrg_jawa_array_isi", Hasil::Handle),
            "array"      => ("jrg_jawa_array", Hasil::Handle),
            "array_len"  => ("jrg_jawa_array_len", Hasil::Int),
            "array_get"  => ("jrg_jawa_array_get", Hasil::Int),
            "array_set"  => ("jrg_jawa_array_set", Hasil::Kosong),
            // Tidak menjawab apa-apa: `unit`, seperti acuannya (Fase KEMBALIAN).
            "array_push" => ("jrg_jawa_array_push", Hasil::Dibuang),
            "buffer"     => ("jrg_jawa_buffer", Hasil::Handle),
            "buffer_len" => ("jrg_jawa_buffer_len", Hasil::Int),
            "buffer_get" => ("jrg_jawa_buffer_get", Hasil::Int),
            "buffer_set" => ("jrg_jawa_buffer_set", Hasil::Kosong),
            "array_pop"  => ("jrg_jawa_array_pop", Hasil::Int),
            "buffer_fill" => ("jrg_jawa_buffer_fill", Hasil::Kosong),
            "sizeof"     => ("jrg_jawa_sizeof", Hasil::Int),
            "array_to_list" => {
                self.ensure_extern("declare ptr @jrg_jawa_array_salin(i64)", "jrg_jawa_array_salin");
                self.ensure_extern("declare i64 @jrg_jawa_array_len(i64)", "jrg_jawa_array_len");
                let (buf, n) = (self.fresh("jwl."), self.fresh("jwn."));
                self.emit(format!("{} = call ptr @jrg_jawa_array_salin(i64 {})", buf, nilai[0]));
                self.emit(format!("{} = call i64 @jrg_jawa_array_len(i64 {})", n, nilai[0]));
                return Ok(self.rakit_slice(&buf, &n, &JrgType::I64));
            }
            "buffer_to_hex" => {
                self.ensure_extern("declare void @jrg_jawa_buffer_to_hex(i64, ptr, ptr)", "jrg_jawa_buffer_to_hex");
                let outp = self.alloca_raw("jwhp.", "ptr", None);
                let outl = self.alloca_raw("jwhl.", "i64", None);
                self.emit(format!("call void @jrg_jawa_buffer_to_hex(i64 {}, ptr {}, ptr {})", nilai[0], outp, outl));
                return self.join_str(&outp, &outl);
            }
            lain => return Err(super::galat::tandai(span, format!(
                "`jawa::{}` belum didukung backend LLVM v1", lain))),
        };

        let tipe_ret = match hasil {
            Hasil::Kosong => "void",
            Hasil::Dibuang => "i64",
            Hasil::Bool => "i32",
            _ => "i64",
        };
        let param: Vec<&str> = nilai.iter().map(|_| "i64").collect();
        self.ensure_extern(
            &format!("declare {} @{}({})", tipe_ret, fungsi, param.join(", ")), fungsi);
        let arg_teks: Vec<String> = nilai.iter().map(|v| format!("i64 {}", v)).collect();

        match hasil {
            Hasil::Kosong => {
                self.emit(format!("call void @{}({})", fungsi, arg_teks.join(", ")));
                Ok(Val::unit())
            }
            Hasil::Dibuang => {
                let r = self.fresh("jw.");
                self.emit(format!("{} = call i64 @{}({})", r, fungsi, arg_teks.join(", ")));
                Ok(Val::unit())
            }
            Hasil::Int => {
                let r = self.fresh("jw.");
                self.emit(format!("{} = call i64 @{}({})", r, fungsi, arg_teks.join(", ")));
                Ok(Val { text: r, ty: JrgType::I64 })
            }
            Hasil::Bool => {
                let r = self.fresh("jw.");
                self.emit(format!("{} = call i32 @{}({})", r, fungsi, arg_teks.join(", ")));
                let b = self.fresh("jwb.");
                self.emit(format!("{} = trunc i32 {} to i8", b, r));
                Ok(Val { text: b, ty: JrgType::Bool })
            }
            Hasil::Handle => {
                let r = self.fresh("jw.");
                self.emit(format!("{} = call i64 @{}({})", r, fungsi, arg_teks.join(", ")));
                let jenis = match item { "array" => "array", "buffer" => "buffer", _ => "single" };
                Ok(self.bungkus_handle(&r, jenis))
            }
        }
    }

    /// Daftarkan struct `Ptr` kalau belum, lalu bungkus sebuah id jadi
    /// nilainya.
    fn bungkus_handle(&mut self, id: &str, jenis: &str) -> Val {
        let ty = self.tipe_ptr();
        let a = self.fresh("jwp.");
        self.emit(format!("{} = insertvalue {} undef, i64 {}, 0", a, ty.llvm_ty(), id));
        // Program yang punya `struct Ptr` sendiri tidak punya field `kind`; ia ditolak `id_handle`.
        if self.types.structs.get("Ptr").and_then(|l| l.field_index("kind")) != Some(1) {
            return Val { text: a, ty };
        }
        let k = self.intern_string(jenis);
        let b = self.fresh("jwp.");
        self.emit(format!("{} = insertvalue {} {}, {} {}, 1", b, ty.llvm_ty(), a, JrgType::Str.llvm_ty(), k.text));
        Val { text: b, ty }
    }

    fn tipe_ptr(&mut self) -> JrgType {
        // Kalau program penggunanya sendiri punya `struct Ptr`, yang dipakai
        // punya dia — dan `id_handle` akan menolaknya dengan menyebut
        // bentuknya, bukan diam-diam membaca field pertamanya sebagai id.
        if !self.types.structs.contains_key("Ptr") {
            self.types.register_struct(super::types::StructLayout {
                name: "Ptr".to_string(),
                fields: vec![("id".to_string(), JrgType::I64), ("kind".to_string(), JrgType::Str)],
            });
        }
        JrgType::Struct("Ptr".to_string())
    }

    /// Ambil id dari sebuah handle; yang bukan handle gagal SAAT JALAN dengan kalimat acuan.
    fn id_handle(&mut self, v: &Val, nama: &str, span: Span) -> Result<String, String> {
        let ptr = self.tipe_ptr();
        if v.ty == JrgType::Any {
            let kata = self.kata_buram(&ptr);
            let slot = self.alloca_typed("jwt.", &JrgType::Any);
            self.emit(format!("store %JrgAny {}, ptr {}", v.text, slot));
            let (w, c) = (self.fresh("jwt."), self.fresh("jwt."));
            self.emit(format!("{} = load i32, ptr {}", w, slot));
            self.emit(format!("{} = icmp ne i32 {}, {}", c, w, kata));
            let kotak = v.clone();
            self.gagal_bila(&c, |cg| {
                // `Ptr` milik `ptr::` (titipan jembatan, bukan struct buram pengguna) ditolak khusus.
                if let Ok(t) = cg.typeof_any(&kotak) {
                    let (tp, tl) = cg.split_str(&t);
                    let k = cg.intern_string("struct:Ptr");
                    let (kp, kl) = cg.split_str(&k);
                    cg.ensure_extern("declare i32 @jrg_str_eq(ptr, i64, ptr, i64)", "jrg_str_eq");
                    let (r, b, g, nb, s) = (cg.fresh("jwt."), cg.fresh("jwt."), cg.fresh("jwt."), cg.fresh("jwt."), cg.fresh("jwt."));
                    cg.emit(format!("{} = call i32 @jrg_str_eq(ptr {}, i64 {}, ptr {}, i64 {})", r, tp, tl, kp, kl));
                    cg.emit(format!("{} = icmp ne i32 {}, 0", b, r));
                    cg.emit(format!("{} = and i32 {}, 255", g, w));
                    cg.emit(format!("{} = icmp ne i32 {}, {}", nb, g, super::codegen_any::TAG_BURAM));
                    cg.emit(format!("{} = and i1 {}, {}", s, b, nb));
                    cg.gagal_bila(&s, |cg| cg.panik(&format!("{}: handle tidak valid (field `kind` hilang)", nama)));
                }
                cg.panik_tipe_any(&kotak, |t| crate::interpreter::builtin::heap::pesan_bukan_ptr(nama, t))
            });
            let buka = self.buka_any(v, &ptr, span)?;
            return self.id_handle(&buka, nama, span);
        }
        if v.ty != ptr {
            self.panik_statis(&crate::interpreter::builtin::heap::pesan_bukan_ptr(nama, &v.ty.nama_typeof()));
            return Ok("0".into());
        }
        if self.types.structs.get("Ptr").and_then(|l| l.field_index("id")) != Some(0) {
            return Err(super::galat::tandai(span,
                "program ini punya `struct Ptr` sendiri, dan `jawa::` memakai nama itu \
                 untuk handle-nya\n\
                 bantuan: ganti nama struct itu\n\
                 catatan: membaca field pertama struct milikmu sebagai id akan menjawab \
                 sesuatu yang terlihat masuk akal dan salah".to_string()));
        }
        let r = self.fresh("jwid.");
        self.emit(format!("{} = extractvalue {} {}, 0", r, v.ty.llvm_ty(), v.text));
        Ok(r)
    }

    /// Lima hitungan dari runtime, lalu `HeapStats` dibentuk `jrgc` lewat jembatan.
    fn jawa_stats(&mut self, span: Span) -> Result<Val, String> {
        self.ensure_extern("declare void @jrg_jawa_stats(ptr)", "jrg_jawa_stats");
        let buf = self.alloca_raw("jst.", "[5 x i64]", Some(8));
        self.emit(format!("call void @jrg_jawa_stats(ptr {})", buf));
        let mut args = Vec::with_capacity(5);
        for i in 0..5 {
            let p = self.fresh("jst.");
            self.emit(format!("{} = getelementptr [5 x i64], ptr {}, i64 0, i64 {}", p, buf, i));
            args.push(self.lokal_sementara(p, JrgType::I64, span));
        }
        self.panggil_jembatan_pintu("::jawa_stats", "HeapStats", &args, span)
    }

}

enum Hasil { Kosong, Dibuang, Int, Bool, Handle }
