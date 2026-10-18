// src/llvm/codegen_dict.rs — llvm/codegen_dict.rs — namespace `dict::` dan literal `{ "k": v }`.

use crate::lexer::Span;
use crate::parser::{Block, Expr, Ident};

use super::codegen::{Codegen, LoopCtx, Val};
use super::types::JrgType;

impl Codegen {
    /// Layani `dict::…`. `None` kalau namanya bukan milik namespace ini.
    pub fn coba_builtin_dict(
        &mut self,
        nama: &str,
        args: &[Expr],
        harap: Option<&JrgType>,
        span: Span,
    ) -> Option<Result<Val, String>> {
        let item = nama.strip_prefix("dict::")?;
        let arity = match item {
            "from_pairs" | "len" | "keys" | "values" | "is_empty" => 1,
            "has" | "get" | "remove" => 2,
            "set" | "get_or" => 3,
            "merge" => 2,
            "new" => 0,
            _ => return None,
        };
        if args.len() != arity {
            return Some(Err(super::galat::tandai(span, format!(
                "`dict::{}` butuh {} argumen, diberi {}", item, arity, args.len()))));
        }
        Some(match item {
            "from_pairs" => self.dict_dari_pasangan(&args[0], span),
            "len" => self.dict_panjang(&args[0], span),
            "is_empty" => self.dict_kosong(&args[0], span),
            "has" => self.dict_punya(args, span),
            "get" => self.dict_ambil(args, span),
            "keys" => self.dict_kunci_atau_nilai(&args[0], span, true),
            "values" => self.dict_kunci_atau_nilai(&args[0], span, false),
            "remove" => self.dict_buang(args, span),
            "set" => self.dict_pasang(args, span),
            "get_or" => self.dict_ambil_atau(args, span),
            "merge" => self.dict_gabung(args, span),
            // `dict::new()` — tipe nilainya dari konteks.
            "new" => {
                let v = match harap {
                    Some(JrgType::Dict(v)) => (**v).clone(),
                    // Tanpa petunjuk tipe nilai: nilai bertag, supaya isinya boleh apa saja.
                    _ => JrgType::Any,
                };
                self.dict_kosong_bertipe(&v)
            }
            _ => unreachable!("arity sudah menyaring nama yang tidak dikenal"),
        })
    }

    /// Kunci dict adalah bentuk TAMPILAN nilainya: `d[0]`, `d["0"]` dan `dict::get(d, 0)` satu kunci.
    pub fn kunci_dict(&mut self, e: &Expr, span: Span) -> Result<Val, String> {
        let k = self.codegen_expr(e, Some(&JrgType::Str))?;
        if k.ty == JrgType::Str { return Ok(k); }
        if !(k.ty.is_int() || k.ty.is_float() || matches!(k.ty, JrgType::Bool | JrgType::Rune | JrgType::Any)) {
            return Err(super::galat::tandai(span, format!(
                "kunci dict bertipe `{}` belum didukung backend `--emit-llvm` — baru teks, angka, bool dan rune\n  \
                 bantuan: ubah jadi teks dulu dengan `str(x)`", k.ty.nama_typeof())));
        }
        self.do_cast(&k, &JrgType::Str, span)
    }

    /// `dict::get_or(d, k, bawaan)` — nilainya, atau `bawaan` kalau kuncinya tidak ada.
    fn dict_ambil_atau(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (d, vty) = self.arg_dict(&args[0], "get_or", span)?;
        let k = self.kunci_dict(&args[1], span)?;
        let bawaan = self.codegen_expr(&args[2], Some(&vty))?;
        let bawaan = self.coerce(&bawaan, &vty);
        let (idx, p, _) = self.cari_kunci(&d, &vty, &k);
        let slot = self.alloca_typed("dgor.", &vty);
        if vty != JrgType::Unit {
            self.emit(format!("store {} {}, ptr {}", vty.llvm_ty(), bawaan.text, slot));
        }
        let ada = self.fresh("dgoa.");
        self.emit(format!("{} = icmp sge i64 {}, 0", ada, idx));
        let l_ada = self.fresh_label("dgoada.");
        let l_end = self.fresh_label("dgoend.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_end));
        self.start_block(l_ada);
        let pty = self.tipe_pasangan(&vty);
        let vp = self.fresh("dgov.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 1", vp, pty, p, idx));
        let v = self.load_place(&vp, &vty);
        if vty != JrgType::Unit {
            self.emit(format!("store {} {}, ptr {}", vty.llvm_ty(), v.text, slot));
        }
        self.terminate(format!("br label %{}", l_end));
        self.start_block(l_end);
        Ok(self.load_place(&slot, &vty))
    }

    /// `dict<T>` ke `dict` bernilai bertag: tiap nilainya dikotak ke penyangga baru.
    pub(crate) fn kotak_tiap_nilai(&mut self, d: &Val, vty: &JrgType) -> Option<Val> {
        super::codegen_any::tag_untuk(vty)?;
        self.ubah_tiap_nilai(d, vty, &JrgType::Any)
    }

    /// Dict baru yang tiap nilainya `coerce` dari `vty` ke `ke` (masuk kotak, peluruhan).
    pub(crate) fn ubah_tiap_nilai(&mut self, d: &Val, vty: &JrgType, ke: &JrgType) -> Option<Val> {
        let (data, n) = self.bagian_dict(d);
        let buf = self.buffer_pasangan(&n, ke);
        let (pl, pb) = (self.tipe_pasangan(vty), self.tipe_pasangan(ke));
        let i = self.alloca_raw("dkti.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i));
        let (cek, badan, usai) = (self.fresh_label("dktcek."), self.fresh_label("dktbadan."), self.fresh_label("dktusai."));
        self.terminate(format!("br label %{}", cek));
        self.start_block(cek.clone());
        let iv = self.load_place(&i, &JrgType::I64);
        let c = self.fresh("dktc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, n));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));
        self.start_block(badan);
        let kp = self.fresh("dktk.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 0", kp, pl, data, iv.text));
        let k = self.load_place(&kp, &JrgType::Str);
        let vp = self.fresh("dktv.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 1", vp, pl, data, iv.text));
        let v = self.load_place(&vp, vty);
        let kotak = self.coerce(&v, ke);
        if kotak.ty != *ke { return None; }
        let dk = self.fresh("dktdk.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 0", dk, pb, buf, iv.text));
        self.emit(format!("store %JrgStr {}, ptr {}", k.text, dk));
        let dv = self.fresh("dktdv.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 1", dv, pb, buf, iv.text));
        self.emit(format!("store {} {}, ptr {}", ke.llvm_ty(), kotak.text, dv));
        let inc = self.fresh("dktn.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i));
        self.terminate(format!("br label %{}", cek));
        self.start_block(usai);
        Some(self.rakit_dict(&buf, &n, ke))
    }

    /// `dict::set(d, k, v)` — dict BARU; aslinya tidak berubah.
    fn dict_pasang(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (d, vty) = self.arg_dict(&args[0], "set", span)?;
        let d = self.salin_dict(&d);
        let k = self.kunci_dict(&args[1], span)?;
        let v = self.codegen_expr(&args[2], Some(&vty))?;
        let v = self.ambil_milik(&args[2], v)?;
        let v = self.coerce(&v, &vty);
        let (tempat, baru) = self.dict_tempat_tulis(&d, &k, span)?;
        if vty != JrgType::Unit {
            self.emit(format!("store {} {}, ptr {}", vty.llvm_ty(), v.text, tempat));
        }
        Ok(baru)
    }

    /// Sebuah `%JrgDict` kosong bertipe nilai `v`.
    fn dict_kosong_bertipe(&mut self, v: &JrgType) -> Result<Val, String> {
        let a = self.fresh("dkos.");
        self.emit(format!("{} = insertvalue %JrgDict undef, ptr null, 0", a));
        let b = self.fresh("dkos.");
        self.emit(format!("{} = insertvalue %JrgDict {}, i64 0, 1", b, a));
        Ok(Val { text: b, ty: JrgType::Dict(Box::new(v.clone())) })
    }

    /// Tipe LLVM satu pasangan `{ %JrgStr, V }`.
    fn tipe_pasangan(&self, v: &JrgType) -> String {
        format!("{{ %JrgStr, {} }}", v.llvm_ty())
    }

    fn ukuran_pasangan(&self, v: &JrgType) -> u64 {
        // Kunci `%JrgStr` 16 byte dan align 8; nilai mengikuti perataannya
        // sendiri, jadi ukurannya dibulatkan seperti struct LLVM.
        let uv = self.types.sizeof(v);
        let av = self.types.alignof(v).max(1);
        let ofs = (16 + av - 1) / av * av;
        let total = ofs + uv;
        let a = av.max(8);
        (total + a - 1) / a * a
    }

    /// Turunkan sebuah argumen dan pastikan ia dict.
    fn arg_dict(&mut self, e: &Expr, fungsi: &str, span: Span)
        -> Result<(Val, JrgType), String>
    {
        // Gerbang: `DictCampur` masuk ke sini sebagai dict biasa bernilai `Any`.
        let d = self.codegen_expr(e, None)?;
        let d = super::codegen_any::samakan_dict(&d);
        // Nilai bertag dibuka sebagai dict bertag; yang bukan dict gagal saat jalan.
        let d = match &d.ty {
            JrgType::Any => self.buka_terperiksa(&d, &JrgType::Dict(Box::new(JrgType::Any))).unwrap_or(d),
            _ => d,
        };
        match &d.ty {
            JrgType::Dict(v) => { let v = (**v).clone(); Ok((d, v)) }
            other => Err(super::galat::tandai(span, format!(
                "`dict::{}` butuh dict, diberi `{}`", fungsi, other.display_name()))),
        }
    }

    /// (penunjuk pasangan, panjang) untuk dibaca BERURUTAN: pasangannya dirapikan dulu.
    pub fn bagian_dict(&mut self, d: &Val) -> (String, String) {
        let (p, n) = self.bagian_dict_mentah(d);
        let vty = match &d.ty {
            JrgType::Dict(v) => (**v).clone(),
            _ => JrgType::Any,
        };
        let uk = self.ukuran_pasangan(&vty);
        self.ensure_extern("declare void @jrg_dict_rapikan(ptr, i64, i64)", "jrg_dict_rapikan");
        self.emit(format!("call void @jrg_dict_rapikan(ptr {}, i64 {}, i64 {})", p, n, uk));
        (p, n)
    }

    /// (penunjuk pasangan, panjang) apa adanya — urutannya urutan sisip.
    pub fn bagian_dict_mentah(&mut self, d: &Val) -> (String, String) {
        let p = self.fresh("dkp.");
        self.emit(format!("{} = extractvalue %JrgDict {}, 0", p, d.text));
        let n = self.fresh("dkn.");
        self.emit(format!("{} = extractvalue %JrgDict {}, 1", n, d.text));
        (p, n)
    }

    fn rakit_dict(&mut self, ptr: &str, len: &str, v: &JrgType) -> Val {
        let a = self.fresh("mkd.");
        self.emit(format!("{} = insertvalue %JrgDict undef, ptr {}, 0", a, ptr));
        let b = self.fresh("mkd.");
        self.emit(format!("{} = insertvalue %JrgDict {}, i64 {}, 1", b, a, len));
        Val { text: b, ty: JrgType::Dict(Box::new(v.clone())) }
    }

    /// Penyangga pasangan berkepala (kapasitas, indeks hash, tanda urut) dari primitif C.
    fn buffer_pasangan(&mut self, jumlah: &str, v: &JrgType) -> String {
        let uk = self.ukuran_pasangan(v);
        let al = self.types.alignof(v).max(8);
        self.ensure_extern("declare ptr @jrg_dict_baru(i64, i64, i64)", "jrg_dict_baru");
        let p = self.fresh("dbp.");
        self.emit(format!("{} = call ptr @jrg_dict_baru(i64 {}, i64 {}, i64 {})", p, jumlah, uk, al));
        p
    }

    /// `{ "a": 1, "b": 2 }` — diturunkan parser jadi `dict::from_pairs([[k, v],
    /// …])`.
    fn dict_dari_pasangan(&mut self, arg: &Expr, span: Span) -> Result<Val, String> {
        let Expr::Array { elems, .. } = arg else {
            return Err(super::galat::tandai(span,
                "`dict::from_pairs` butuh daftar pasangan `[[kunci, nilai], …]`"
                    .to_string()));
        };

        // Kumpulkan (kunci, ekspresi nilai) tanpa menurunkan apa pun dulu —
        // tipe nilainya baru bisa disatukan sesudah semuanya terlihat.
        let mut pasangan: Vec<(&Expr, &Expr)> = Vec::with_capacity(elems.len());
        for e in elems {
            let Expr::Array { elems: kv, .. } = e else {
                return Err(super::galat::tandai(span,
                    "tiap pasangan dict harus berbentuk `[kunci, nilai]`".to_string()));
            };
            if kv.len() != 2 {
                return Err(super::galat::tandai(span,
                    "tiap pasangan dict harus punya tepat 2 unsur".to_string()));
            }
            pasangan.push((&kv[0], &kv[1]));
        }

        if pasangan.is_empty() {
            return Err(super::galat::tandai(span,
                "dict kosong tidak menyebut tipe nilainya, dan backend LLVM \
                 tidak punya tempat untuk menyimpulkannya\n\
                 bantuan: bangun dengan setidaknya satu isi".to_string()));
        }

        // Semua kunci dan nilai diturunkan TEPAT SEKALI, lalu tipenya disatukan.
        let mut kunci = Vec::with_capacity(pasangan.len());
        let mut nilai = Vec::with_capacity(pasangan.len());
        for (ke, ve) in &pasangan {
            let k = self.kunci_dict(ke, span)?;
            kunci.push(k);
            nilai.push(self.codegen_expr(ve, None)?);
        }
        // Array literal dengan panjang berbeda punya TIPE berbeda di sini (`[2]i64`
        // vs `[1]i64`) sementara acuan semantiknya menganggapnya satu tipe.
        let vty = {
            let t0 = nilai[0].ty.clone();
            let beda = nilai.iter().any(|v| v.ty != t0);
            let elem = match &t0 { JrgType::Array(e, _) => Some((**e).clone()), _ => None };
            match (beda, elem) {
                // Juga kalau panjangnya SAMA: nilai dict boleh diganti array berpanjang lain belakangan.
                (_, Some(e)) if nilai.iter().all(|v| matches!(&v.ty, JrgType::Array(x, _) if **x == e)) => {
                    let target = JrgType::Slice(Box::new(e));
                    for v in nilai.iter_mut() {
                        let luruh = self.coerce(v, &target);
                        *v = self.salin_slice(&luruh);
                    }
                    target
                }
                // Tipe nilainya benar-benar campur.
                (true, _) => JrgType::Any,
                _ => t0,
            }
        };

        // Tabel per-kunci, cuma untuk kunci yang tertulis sebagai literal.
        let tabel_kunci: Vec<(String, JrgType)> = if vty == JrgType::Any {
            pasangan.iter().zip(nilai.iter()).filter_map(|((ke, _), v)| {
                match ke {
                    Expr::Literal(crate::parser::LitKind::Str(k), _)
                    | Expr::Literal(crate::parser::LitKind::RawStr(k), _) =>
                        Some((k.clone(), super::codegen_any::luruh(&v.ty))),
                    _ => None,
                }
            }).collect()
        } else { Vec::new() };

        let jumlah = pasangan.len();
        let buf = self.buffer_pasangan(&jumlah.to_string(), &vty);
        let pty = self.tipe_pasangan(&vty);

        for i in 0..jumlah {
            let k = kunci[i].clone();
            let v = nilai[i].clone();
            let v = if vty == JrgType::Any { self.kotak_any(&v, span)? } else { v };
            if v.ty != vty {
                return Err(super::galat::tandai(span, format!(
                    "isian dict ke-{} bertipe `{}`, yang tidak bisa disatukan \
                     dengan `{}`\n\
                     bantuan: samakan tipenya, atau pakai struct kalau memang \
                     tiap field beda jenis",
                    i + 1, v.ty.display_name(), vty.display_name())));
            }
            let slot = self.fresh("dfp.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", slot, pty, buf, i));
            let kp = self.fresh("dfk.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", kp, pty, slot));
            self.emit(format!("store %JrgStr {}, ptr {}", k.text, kp));
            let vp = self.fresh("dfv.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", vp, pty, slot));
            self.emit(format!("store {} {}, ptr {}", vty.llvm_ty(), v.text, vp));
        }

        // Diurutkan dan kunci gandanya dibuang oleh runtime; panjangnya bisa
        // menyusut, jadi yang dipakai panjang yang DIKEMBALIKAN.
        self.ensure_extern("declare i64 @jrg_dict_build(ptr, i64, i64)", "jrg_dict_build");
        let uk = self.ukuran_pasangan(&vty);
        let n = self.fresh("dfn.");
        self.emit(format!("{} = call i64 @jrg_dict_build(ptr {}, i64 {}, i64 {})",
                          n, buf, jumlah, uk));
        let d = self.rakit_dict(&buf, &n, &vty);
        // Tipenya dinaikkan jadi `DictCampur` HANYA di sini: bentuk nilainya
        // sudah persis sama dengan `Dict(Any)`, yang berubah cuma apa yang
        // diketahui kompilator tentang isinya.
        Ok(if tabel_kunci.is_empty() { d } else {
            Val { text: d.text, ty: JrgType::DictCampur(std::rc::Rc::new(tabel_kunci)) }
        })
    }

    /// Cari kunci; kembalikan (indeks, penunjuk pasangan, panjang, ptr buf).
    fn cari_kunci(&mut self, d: &Val, vty: &JrgType, k: &Val) -> (String, String, String) {
        self.ensure_extern(
            "declare i64 @jrg_dict_find(ptr, i64, i64, ptr, i64)", "jrg_dict_find");
        let (p, n) = self.bagian_dict_mentah(d);
        let (kp, kl) = self.split_str(k);
        let uk = self.ukuran_pasangan(vty);
        let idx = self.fresh("dfi.");
        self.emit(format!(
            "{} = call i64 @jrg_dict_find(ptr {}, i64 {}, i64 {}, ptr {}, i64 {})",
            idx, p, n, uk, kp, kl));
        (idx, p, n)
    }

    /// `d["k"]` — nilai untuk sebuah kunci. Kunci yang tidak ada memanikkan.
    pub fn dict_baca(&mut self, d: &Val, k: &Val, span: Span) -> Result<Val, String> {
        let JrgType::Dict(vty) = &d.ty else {
            return Err(super::galat::tandai(span, "bukan dict".to_string()));
        };
        let vty = (**vty).clone();
        let (idx, p, _n) = self.cari_kunci(d, &vty, k);

        let ada = self.fresh("dba.");
        self.emit(format!("{} = icmp sge i64 {}, 0", ada, idx));
        let ok = self.fresh_label("dkok.");
        let hilang = self.fresh_label("dkhilang.");
        self.terminate_unlikely_true(&ada, &ok, &hilang);

        self.start_block(hilang);
        self.ensure_extern(
            "declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        let pesan = self.intern_string(
            "kunci ini tidak ada di dict — cek dulu dengan `dict::has` atau pakai `dict::get`");
        let (mp, ml) = self.split_str(&pesan);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());

        self.start_block(ok);
        let pty = self.tipe_pasangan(&vty);
        let slot = self.fresh("dbs.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", slot, pty, p, idx));
        let vp = self.fresh("dbv.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", vp, pty, slot));
        Ok(self.load_place(&vp, &vty))
    }

    /// TEMPAT nilai untuk sebuah kunci, menyisipkannya kalau belum ada.
    /// Langkah ANTARA sebuah jalur tulis (`zz` di `d.zz.b = 5`) tidak boleh menumbuhkan
    /// dict: kunci yang belum ada gagal saat jalan, sama seperti acuannya.
    pub fn wajib_kunci_ada(&mut self, d: &Val, k: &Val) -> Result<(), String> {
        let JrgType::Dict(vty) = &d.ty else { return Ok(()) };
        let vty = (**vty).clone();
        let (idx, _, _) = self.cari_kunci(d, &vty, k);
        let ada = self.fresh("dwa.");
        self.emit(format!("{} = icmp sge i64 {}, 0", ada, idx));
        let l_hilang = self.fresh_label("dwhilang.");
        let l_ok = self.fresh_label("dwok.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ok, l_hilang));

        self.start_block(l_hilang);
        let utuh = crate::interpreter::galat_index::belum_ada_untuk_ditulis("key", "\u{0}");
        let (depan, belakang) = utuh.split_once('\u{0}').expect("penanda kunci");
        let depan = self.intern_string(depan);
        let belakang = self.intern_string(belakang);
        let pesan = self.str_concat(&depan, k)?;
        let pesan = self.str_concat(&pesan, &belakang)?;
        self.ensure_extern(
            "declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        let (mp, ml) = self.split_str(&pesan);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());

        self.start_block(l_ok);
        Ok(())
    }

    pub fn dict_tempat_tulis(&mut self, d: &Val, k: &Val, span: Span)
        -> Result<(String, Val), String>
    {
        let JrgType::Dict(vty) = &d.ty else {
            return Err(super::galat::tandai(span, "bukan dict".to_string()));
        };
        let vty = (**vty).clone();
        let uk = self.ukuran_pasangan(&vty);
        let pty = self.tipe_pasangan(&vty);

        let al = self.types.alignof(&vty).max(8);
        // Cari-atau-sisip di primitif C: kunci baru ditambahkan di ujung penyangga yang sama.
        let slot_dict = self.alloca_typed("dtd.", &d.ty);
        self.emit(format!("store %JrgDict {}, ptr {}", d.text, slot_dict));
        self.ensure_extern("declare ptr @jrg_dict_tempat(ptr, i64, i64, ptr, i64)", "jrg_dict_tempat");
        let (kp, kl) = self.split_str(k);
        let pas = self.fresh("dtpas.");
        self.emit(format!("{} = call ptr @jrg_dict_tempat(ptr {}, i64 {}, i64 {}, ptr {}, i64 {})",
                          pas, slot_dict, uk, al, kp, kl));
        let ptr = self.fresh("dtr.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", ptr, pty, pas));
        let dnew = self.load_place(&slot_dict, &d.ty);
        Ok((ptr, dnew))
    }

    fn dict_panjang(&mut self, e: &Expr, span: Span) -> Result<Val, String> {
        let (d, _) = self.arg_dict(e, "len", span)?;
        let (_, n) = self.bagian_dict_mentah(&d);
        Ok(Val { text: n, ty: JrgType::I64 })
    }

    fn dict_kosong(&mut self, e: &Expr, span: Span) -> Result<Val, String> {
        let (d, _) = self.arg_dict(e, "is_empty", span)?;
        let (_, n) = self.bagian_dict_mentah(&d);
        let r = self.fresh("dke.");
        self.emit(format!("{} = icmp eq i64 {}, 0", r, n));
        Ok(self.from_i1(&r))
    }

    fn dict_punya(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (d, vty) = self.arg_dict(&args[0], "has", span)?;
        let k = self.kunci_dict(&args[1], span)?;
        let (idx, _, _) = self.cari_kunci(&d, &vty, &k);
        let r = self.fresh("dkh.");
        self.emit(format!("{} = icmp sge i64 {}, 0", r, idx));
        Ok(self.from_i1(&r))
    }

    /// `dict::get(d, k)` — `Option::Some(v)` atau `Option::None`.
    fn tipe_kunci_tertulis(&mut self, d: &Expr, k: &Expr) -> Option<JrgType> {
        let nama = match k {
            Expr::Literal(crate::parser::LitKind::Str(t), _)
            | Expr::Literal(crate::parser::LitKind::RawStr(t), _) => t.clone(),
            _ => return None,
        };
        let t = self.peek_expr_type(d).or_else(|| self.get_place(d).ok().map(|(_, t)| t))?;
        let JrgType::DictCampur(tabel) = t else { return None };
        tabel.iter().find(|(n, _)| *n == nama).map(|(_, t)| t.clone())
    }

    fn dict_ambil(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        // Tipe HASILNYA bisa lebih sempit dari tipe SIMPANANNYA.
        let vbaca = self.tipe_kunci_tertulis(&args[0], &args[1]);
        let (d, vty) = self.arg_dict(&args[0], "get", span)?;
        let k = self.kunci_dict(&args[1], span)?;
        let (idx, p, _) = self.cari_kunci(&d, &vty, &k);
        let vhasil = vbaca.clone().unwrap_or_else(|| vty.clone());

        // `Option<V>` diinstansiasi lewat jalur yang sudah ada, bukan dibangun
        // sendiri — layout enum-nya harus persis sama dengan yang dipakai …
        let en = self.instantiate_option(&vhasil);
        let opt_ty = JrgType::Enum(en.clone());
        let layout = self.types.enums.get(&en).cloned()
            .ok_or_else(|| "layout Option tidak terdaftar".to_string())?;
        let slot = self.alloca_typed("dgo.", &opt_ty);
        let ada = self.fresh("dga.");
        self.emit(format!("{} = icmp sge i64 {}, 0", ada, idx));
        let l_ada = self.fresh_label("dgada.");
        let l_tidak = self.fresh_label("dgtidak.");
        let l_end = self.fresh_label("dgend.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_tidak));

        self.start_block(l_ada);
        let pty = self.tipe_pasangan(&vty);
        let s = self.fresh("dgs.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", s, pty, p, idx));
        let vp = self.fresh("dgv.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", vp, pty, s));
        let v = self.load_place(&vp, &vty);
        let v = match &vbaca {
            Some(t) => self.buka_any(&v, t, span)?,
            None => v,
        };
        let vsome = layout.variant("Some").cloned()
            .ok_or_else(|| "variant Some tidak ada".to_string())?;
        let some = self.build_enum_value(&en, &layout, &vsome, &[v])?;
        self.emit(format!("store {} {}, ptr {}", opt_ty.llvm_ty(), some.text, slot));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_tidak);
        let none = self.build_enum_unit_variant(&en, "None")?;
        self.emit(format!("store {} {}, ptr {}", opt_ty.llvm_ty(), none.text, slot));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_end);
        Ok(self.load_place(&slot, &opt_ty))
    }

    /// `dict::keys(d)` / `dict::values(d)` — larik, terurut menurut kunci.
    fn dict_kunci_atau_nilai(&mut self, e: &Expr, span: Span, kunci: bool)
        -> Result<Val, String>
    {
        let nama = if kunci { "keys" } else { "values" };
        let (d, vty) = self.arg_dict(e, nama, span)?;
        let (p, n) = self.bagian_dict(&d);
        let elem = if kunci { JrgType::Str } else { vty.clone() };
        let pty = self.tipe_pasangan(&vty);
        let field = if kunci { 0 } else { 1 };

        let ukuran = self.types.sizeof(&elem);
        let align = self.types.alignof(&elem);
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let byte = self.fresh("dkvb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, n, ukuran));
        let buf = self.fresh("dkvp.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, byte, align));

        let idx = self.alloca_raw("dkvi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));
        let cond = self.fresh_label("dkv.cond.");
        let body = self.fresh_label("dkv.body.");
        let end = self.fresh_label("dkv.end.");
        self.terminate(format!("br label %{}", cond));

        self.start_block(cond.clone());
        let i = self.fresh("dkvv.");
        self.emit(format!("{} = load i64, ptr {}", i, idx));
        let lanjut = self.fresh("dkvc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", lanjut, i, n));
        self.terminate(format!("br i1 {}, label %{}, label %{}", lanjut, body, end));

        self.start_block(body);
        let s = self.fresh("dkvs.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", s, pty, p, i));
        let f = self.fresh("dkvf.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", f, pty, s, field));
        let v = self.load_place(&f, &elem);
        let dst = self.fresh("dkvd.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", dst, elem.llvm_ty(), buf, i));
        self.emit(format!("store {} {}, ptr {}", elem.llvm_ty(), v.text, dst));
        let inext = self.fresh("dkvn.");
        self.emit(format!("{} = add i64 {}, 1", inext, i));
        self.emit(format!("store i64 {}, ptr {}", inext, idx));
        self.terminate(format!("br label %{}", cond));

        self.start_block(end);
        let a = self.fresh("dkva.");
        self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", a, buf));
        let b = self.fresh("dkva.");
        self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", b, a, n));
        Ok(Val { text: b, ty: JrgType::Slice(Box::new(elem)) })
    }

    /// `dict::remove(d, k)` — dict BARU tanpa kunci itu.
    fn dict_buang(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (d, vty) = self.arg_dict(&args[0], "remove", span)?;
        let k = self.kunci_dict(&args[1], span)?;
        let uk = self.ukuran_pasangan(&vty);
        let pty = self.tipe_pasangan(&vty);
        // Hasilnya ditandai urut, jadi sumbernya dirapikan sebelum nomor pasangannya dicari.
        let _ = self.bagian_dict(&d);
        let (idx, p, n) = self.cari_kunci(&d, &vty, &k);

        let slot = self.alloca_typed("drd.", &d.ty);
        self.emit(format!("store %JrgDict {}, ptr {}", d.text, slot));
        let ada = self.fresh("dra.");
        self.emit(format!("{} = icmp sge i64 {}, 0", ada, idx));
        let l_ada = self.fresh_label("drada.");
        let l_end = self.fresh_label("drend.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_end));

        self.start_block(l_ada);
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");
        let n2 = self.fresh("drn.");
        self.emit(format!("{} = sub i64 {}, 1", n2, n));
        let buf = self.buffer_pasangan(&n2, &vty);
        let b1 = self.fresh("drb.");
        self.emit(format!("{} = mul i64 {}, {}", b1, idx, uk));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, p, b1));
        let sisa = self.fresh("drs.");
        self.emit(format!("{} = sub i64 {}, {}", sisa, n2, idx));
        let b2 = self.fresh("drb2.");
        self.emit(format!("{} = mul i64 {}, {}", b2, sisa, uk));
        let spos = self.fresh("drsp.");
        self.emit(format!("{} = add i64 {}, 1", spos, idx));
        let src = self.fresh("drsr.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", src, pty, p, spos));
        let dst = self.fresh("drds.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", dst, pty, buf, idx));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            dst, src, b2));
        let baru = self.rakit_dict(&buf, &n2, &vty);
        self.emit(format!("store %JrgDict {}, ptr {}", baru.text, slot));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_end);
        Ok(self.load_place(&slot, &d.ty))
    }

    /// `dict::merge(a, b)` — dict BARU; isi `b` menimpa isi `a`.
    fn dict_gabung(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (a, vty) = self.arg_dict(&args[0], "merge", span)?;
        let (b, vty2) = self.arg_dict(&args[1], "merge", span)?;
        if vty != vty2 {
            return Err(super::galat::tandai(span, format!(
                "`dict::merge` butuh dua dict dengan tipe nilai sama, diberi `{}` dan `{}`",
                vty.display_name(), vty2.display_name())));
        }
        let uk = self.ukuran_pasangan(&vty);
        let (pa, na) = self.bagian_dict(&a);
        let (pb, nb) = self.bagian_dict(&b);
        let total = self.fresh("dmt.");
        self.emit(format!("{} = add i64 {}, {}", total, na, nb));
        let buf = self.buffer_pasangan(&total, &vty);

        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");
        let ba = self.fresh("dma.");
        self.emit(format!("{} = mul i64 {}, {}", ba, na, uk));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, pa, ba));
        let ujung = self.fresh("dmu.");
        self.emit(format!("{} = getelementptr i8, ptr {}, i64 {}", ujung, buf, ba));
        let bb = self.fresh("dmb.");
        self.emit(format!("{} = mul i64 {}, {}", bb, nb, uk));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            ujung, pb, bb));

        self.ensure_extern("declare i64 @jrg_dict_build(ptr, i64, i64)", "jrg_dict_build");
        let n = self.fresh("dmn.");
        self.emit(format!("{} = call i64 @jrg_dict_build(ptr {}, i64 {}, i64 {})",
                          n, buf, total, uk));
        Ok(self.rakit_dict(&buf, &n, &vty))
    }

    /// Salin buffer sebuah dict — dipakai `ambil_milik`.
    pub fn salin_dict(&mut self, d: &Val) -> Val {
        let vty = match &d.ty {
            JrgType::Dict(vty) => (**vty).clone(),
            JrgType::DictCampur(_) => JrgType::Any,
            _ => return d.clone(),
        };
        let uk = self.ukuran_pasangan(&vty);
        let (p, n) = self.bagian_dict(d);
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");
        let buf = self.buffer_pasangan(&n, &vty);
        let byte = self.fresh("dsb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, n, uk));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, p, byte));
        // Nilai yang memegang penyangga (array, dict) disalin juga — kalau tidak, salinan dict
        // dan aslinya berbagi isi, dan menulis lewat satu mengubah yang lain.
        if self.butuh_salin(&vty) {
            let pty = self.tipe_pasangan(&vty);
            let i = self.alloca_raw("dsi.", "i64", Some(8));
            self.emit(format!("store i64 0, ptr {}", i));
            let (cek, badan, usai) = (self.fresh_label("ds.cek."), self.fresh_label("ds.badan."), self.fresh_label("ds.usai."));
            self.terminate(format!("br label %{}", cek));
            self.start_block(cek.clone());
            let iv = self.load_place(&i, &JrgType::I64);
            let c = self.fresh("dsc.");
            self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, n));
            self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));
            self.start_block(badan);
            let ep = self.fresh("dsp.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 1", ep, pty, buf, iv.text));
            let ev = self.load_place(&ep, &vty);
            let baru = self.salin_dalam(&ev);
            self.emit(format!("store {} {}, ptr {}", vty.llvm_ty(), baru.text, ep));
            let iv2 = self.load_place(&i, &JrgType::I64);
            let inc = self.fresh("dsn.");
            self.emit(format!("{} = add i64 {}, 1", inc, iv2.text));
            self.emit(format!("store i64 {}, ptr {}", inc, i));
            if !self.is_terminated() { self.terminate(format!("br label %{}", cek)); }
            self.start_block(usai);
        }
        let r = self.rakit_dict(&buf, &n, &vty);
        Val { text: r.text, ty: d.ty.clone() }
    }
}

impl Codegen {
    /// `dict {a: 1, b: 2}` — sama persis dengan yang dicetak interpreter dan VM.
    pub fn tampilan_dict(&mut self, d: &Val, vty: &JrgType) -> Result<Val, String> {
        let (data, n) = self.bagian_dict(d);
        let pty = self.tipe_pasangan(vty);

        let acc = self.alloca_raw("ddacc.", "%JrgStr", Some(8));
        let awal = self.intern_string("dict {");
        self.emit(format!("store %JrgStr {}, ptr {}", awal.text, acc));
        let sep = self.alloca_raw("ddsep.", "%JrgStr", Some(8));
        let kosong = self.intern_string("");
        self.emit(format!("store %JrgStr {}, ptr {}", kosong.text, sep));

        let i = self.alloca_raw("ddi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i));
        let (cek, badan, usai) = (self.fresh_label("ddcek."), self.fresh_label("ddbadan."), self.fresh_label("ddusai."));
        self.terminate(format!("br label %{}", cek));

        self.start_block(cek.clone());
        let iv = self.load_place(&i, &JrgType::I64);
        let c = self.fresh("ddc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, n));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));

        self.start_block(badan);
        let pp = self.fresh("ddpp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", pp, pty, data, iv.text));
        let kp = self.fresh("ddkp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", kp, pty, pp));
        let kv = self.load_place(&kp, &JrgType::Str);
        let vp = self.fresh("ddvp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", vp, pty, pp));
        let vv = self.load_place(&vp, vty);
        let vs = self.build_display_str(&vv)?;

        let mut cur = self.load_place(&acc, &JrgType::Str);
        let sv = self.load_place(&sep, &JrgType::Str);
        cur = self.str_concat(&cur, &sv)?;
        cur = self.str_concat(&cur, &kv)?;
        let titik = self.intern_string(": ");
        cur = self.str_concat(&cur, &titik)?;
        cur = self.str_concat(&cur, &vs)?;
        self.emit(format!("store %JrgStr {}, ptr {}", cur.text, acc));
        let koma = self.intern_string(", ");
        self.emit(format!("store %JrgStr {}, ptr {}", koma.text, sep));

        let inc = self.fresh("ddinc.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cek)); }

        self.start_block(usai);
        let hasil = self.load_place(&acc, &JrgType::Str);
        let tutup = self.intern_string("}");
        self.str_concat(&hasil, &tutup)
    }

    /// Dua dict sama kalau panjangnya sama dan tiap pasangan pada indeks yang
    /// sama cocok.
    pub fn kumpul_sama_dict(
        &mut self, slot: &str, vty: &JrgType, lval: &str, rval: &str, span: Span,
    ) -> Result<(), String> {
        let ld = Val { text: lval.to_string(), ty: JrgType::Dict(Box::new(vty.clone())) };
        let rd = Val { text: rval.to_string(), ty: JrgType::Dict(Box::new(vty.clone())) };
        let (da, na) = self.bagian_dict(&ld);
        let (db, nb) = self.bagian_dict(&rd);
        let pty = self.tipe_pasangan(vty);

        // Panjang lebih dulu, dan loopnya cuma jalan kalau panjangnya sama —
        // kalau tidak, ia membaca melewati ujung yang lebih pendek.
        let sama_n = self.fresh("dqn.");
        self.emit(format!("{} = icmp eq i64 {}, {}", sama_n, na, nb));
        let sn8 = self.fresh("dqn8.");
        self.emit(format!("{} = zext i1 {} to i8", sn8, sama_n));
        self.dan_ke_slot(slot, &Val { text: sn8, ty: JrgType::Bool });

        let (mulai, cek, badan, terus, usai) = (
            self.fresh_label("dqmulai."), self.fresh_label("dqcek."),
            self.fresh_label("dqbadan."), self.fresh_label("dqterus."),
            self.fresh_label("dqusai."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", sama_n, mulai, usai));

        self.start_block(mulai);
        let i = self.alloca_raw("dqi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i));
        self.terminate(format!("br label %{}", cek));

        // Keluar lebih awal begitu akumulatornya nol: jumlah pasangan tidak
        // berbatas, jadi tanpa ini dua dict besar yang beda di pasangan
        // pertama tetap dibaca habis.
        self.start_block(cek.clone());
        let iv = self.load_place(&i, &JrgType::I64);
        let blm = self.fresh("dqblm.");
        self.emit(format!("{} = icmp slt i64 {}, {}", blm, iv.text, na));
        let acc = self.fresh("dqacc.");
        self.emit(format!("{} = load i8, ptr {}", acc, slot));
        let acc1 = self.fresh("dqacc1.");
        self.emit(format!("{} = trunc i8 {} to i1", acc1, acc));
        let lanjut = self.fresh("dql.");
        self.emit(format!("{} = and i1 {}, {}", lanjut, blm, acc1));
        self.terminate(format!("br i1 {}, label %{}, label %{}", lanjut, badan, usai));

        self.start_block(badan);
        for (idx, t) in [(0usize, JrgType::Str), (1, vty.clone())] {
            let pa = self.fresh("dqpa.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 {}", pa, pty, da, iv.text, idx));
            let pb = self.fresh("dqpb.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 {}", pb, pty, db, iv.text, idx));
            let va = self.load_place(&pa, &t);
            let vb = self.load_place(&pb, &t);
            self.kumpul_sama(slot, &t, &va.text, &vb.text, span)?;
        }
        if !self.is_terminated() { self.terminate(format!("br label %{}", terus)); }

        self.start_block(terus);
        let inc = self.fresh("dqinc.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cek)); }

        self.start_block(usai);
        Ok(())
    }
}

impl Codegen {
    /// `for k in d` menyebut KUNCI, `for k, v in d` menyebut kunci dan nilai.
    pub fn codegen_for_dict(
        &mut self, vars: &[Ident], d: &Val, vty: &JrgType, body: &Block,
        label: &Option<String>,
    ) -> Result<(), String> {
        // Yang ditelusuri SALINAN: badan loop boleh menyisipkan ke dict aslinya, dan
        // penyisipan itu tidak boleh menggeser pasangan di bawah penelusuran.
        let d = &self.salin_dict(d);
        let (data, n) = self.bagian_dict(d);
        let pty = self.tipe_pasangan(vty);

        let i = self.alloca_raw("fdidx.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i));
        let (cek, badan, pasca, usai) = (
            self.fresh_label("fdcek."), self.fresh_label("fdbadan."),
            self.fresh_label("fdpasca."), self.fresh_label("fdusai."));
        self.terminate(format!("br label %{}", cek));

        self.start_block(cek.clone());
        let iv = self.load_place(&i, &JrgType::I64);
        let c = self.fresh("fdc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, n));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));

        self.start_block(badan);
        self.push_scope();
        let iv2 = self.load_place(&i, &JrgType::I64);
        let kp = self.fresh("fdkp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 0", kp, pty, data, iv2.text));
        let kv = self.load_place(&kp, &JrgType::Str);
        if vars[0] != "_" {
            let s = self.alloca_local(&vars[0], JrgType::Str);
            self.emit(format!("store %JrgStr {}, ptr {}", kv.text, s));
        }
        if vars.len() == 2 && vars[1] != "_" {
            let vp = self.fresh("fdvp.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 1", vp, pty, data, iv2.text));
            let vv = self.load_place(&vp, vty);
            // Nilainya MILIK dict; yang disebut variabel loop harus salinan,
            // kalau tidak menulis ke variabel itu menulis balik ke dictnya.
            let vv = if matches!(vty, JrgType::Slice(_)) { self.salin_slice(&vv) }
                     else if matches!(vty, JrgType::Dict(_)) { self.salin_dict(&vv) }
                     else { vv };
            let s = self.alloca_local(&vars[1], vty.clone());
            if *vty != JrgType::Unit {
                self.emit(format!("store {} {}, ptr {}", vty.llvm_ty(), vv.text, s));
            }
        }
        self.push_loop(LoopCtx { label: label.clone(), continue_bb: pasca.clone(), break_bb: usai.clone() });
        self.codegen_stmt_block(body)?;
        self.pop_loop();
        self.pop_scope();
        if !self.is_terminated() { self.terminate(format!("br label %{}", pasca)); }

        self.start_block(pasca);
        let iv3 = self.load_place(&i, &JrgType::I64);
        let inc = self.fresh("fdn.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv3.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cek)); }

        self.start_block(usai);
        Ok(())
    }
}
