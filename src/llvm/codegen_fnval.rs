// src/llvm/codegen_fnval.rs — llvm/codegen_fnval.rs — fungsi sebagai NILAI.

use crate::lexer::Span;
use crate::parser::{EnumVariantKind, Expr, TypeExpr};

use super::codegen::{Codegen, KerjaFn, Val};
use super::types::JrgType;
use crate::llvm::TypeSubst;

impl Codegen {
    pub fn pancarkan_kerja_fn(&mut self, k: KerjaFn) -> Result<(), String> {
        match k {
            KerjaFn::Bungkus { pembungkus, asli, params, ret } =>
                self.pancarkan_pembungkus(&pembungkus, &asli, &params, &ret),
            KerjaFn::TampilKotak => self.pancarkan_tampil_kotak(),
            KerjaFn::Adaptor { nama, dari, ke } => self.pancarkan_adaptor(&nama, &dari, &ke),
            KerjaFn::Banding { nama, ty } => self.pancarkan_banding(&nama, &ty),
            KerjaFn::Tunda { nama, panggilan, beku, generik } =>
                self.pancarkan_situs_tunda(&nama, &panggilan, &beku, generik),
            KerjaFn::Closure { nama, params, ret, body, tangkap, rantai_hasil } =>
                self.pancarkan_closure(&nama, &params, &ret, &body, &tangkap, &rantai_hasil),
        }
    }

    /// Nilai fungsi bertipe `ke` yang membungkus `f` (bertipe lain): lingkungannya memegang `f`.
    pub(crate) fn adaptasi_fungsi(&mut self, f: &Val, ke: &JrgType) -> Val {
        let kunci = format!("{:?}=>{:?}", f.ty, ke);
        let nama = match self.adaptor_ada.get(&kunci) {
            Some(n) => n.clone(),
            None => {
                let n = format!("adaptor.{}", self.adaptor_ada.len());
                self.adaptor_ada.insert(kunci, n.clone());
                self.antre_fn(KerjaFn::Adaptor { nama: n.clone(), dari: f.ty.clone(), ke: ke.clone() });
                n
            }
        };
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let env = self.fresh("adpe.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 16, i64 8)", env));
        self.emit(format!("store %JrgFn {}, ptr {}", f.text, env));
        let a = self.fresh("adpf.");
        self.emit(format!("{} = insertvalue %JrgFn undef, ptr @{}, 0", a, super::mangle::llvm_quoted_ident(&nama)));
        let b = self.fresh("adpf.");
        self.emit(format!("{} = insertvalue %JrgFn {}, ptr {}, 1", b, a, env));
        Val { text: b, ty: ke.clone() }
    }

    fn pancarkan_adaptor(&mut self, nama: &str, dari: &JrgType, ke: &JrgType) -> Result<(), String> {
        let (JrgType::Fn(pd, rd), JrgType::Fn(pk, rk)) = (dari, ke) else { return Ok(()) };
        self.mulai_fungsi_kosong((**rk).clone());
        let asli = self.fresh("adpo.");
        self.emit(format!("{} = load %JrgFn, ptr %env", asli));
        let mut sig = vec!["ptr %env".to_string()];
        let mut pakai = Vec::new();
        let (kode, env) = (self.fresh("adpk."), self.fresh("adpv."));
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, asli));
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, asli));
        pakai.push(format!("ptr {}", env));
        for (i, (tk, td)) in pk.iter().zip(pd.iter()).enumerate() {
            sig.push(format!("{} %a{}", tk.llvm_ty(), i));
            let v = self.coerce(&Val { text: format!("%a{}", i), ty: tk.clone() }, td);
            pakai.push(format!("{} {}", td.llvm_ty(), v.text));
        }
        if **rd == JrgType::Unit || **rd == JrgType::Never {
            self.emit(format!("call void {}({})", kode, pakai.join(", ")));
            if **rk == JrgType::Unit { self.terminate("ret void".to_string()); }
            else { let u = self.coerce(&Val::unit(), rk); self.terminate(format!("ret {} {}", rk.llvm_ty(), u.text)); }
        } else {
            let r = self.fresh("adpr.");
            self.emit(format!("{} = call {} {}({})", r, rd.llvm_ty(), kode, pakai.join(", ")));
            let v = self.coerce(&Val { text: r, ty: (**rd).clone() }, rk);
            if **rk == JrgType::Unit { self.terminate("ret void".to_string()); }
            else { self.terminate(format!("ret {} {}", rk.llvm_ty(), v.text)); }
        }
        let badan = self.take_function_body();
        self.tambah_definisi(format!("define internal {} @{}({}) {{\n{}}}\n",
            rk.llvm_ret_ty(), super::mangle::llvm_quoted_ident(nama), sig.join(", "), badan));
        self.selesai_fungsi_kosong();
        Ok(())
    }

    fn pancarkan_pembungkus(&mut self, pembungkus: &str, asli: &str,
                            params: &[JrgType], ret: &JrgType) -> Result<(), String> {
        let mut sig = vec!["ptr %env".to_string()];
        let mut pakai = Vec::new();
        for (i, t) in params.iter().enumerate() {
            sig.push(format!("{} %a{}", t.llvm_ty(), i));
            pakai.push(format!("{} %a{}", t.llvm_ty(), i));
        }
        let badan = if *ret == JrgType::Unit || *ret == JrgType::Never {
            format!("  call void @{}({})\n  ret void\n",
                super::mangle::llvm_quoted_ident(asli), pakai.join(", "))
        } else {
            format!("  %r = call {} @{}({})\n  ret {} %r\n",
                ret.llvm_ty(), super::mangle::llvm_quoted_ident(asli), pakai.join(", "),
                ret.llvm_ty())
        };
        self.tambah_definisi(format!(
            "define internal {} @{}({}) local_unnamed_addr nounwind {{\nentry:\n{}}}\n",
            ret.llvm_ret_ty(), super::mangle::llvm_quoted_ident(pembungkus), sig.join(", "), badan));
        Ok(())
    }

    /// `T::m` sebagai nilai: `|p0, p1, ..| T::m(p0, p1, ..)`, dengan tipe parameter dari deklarasinya.
    pub(crate) fn nilai_metode(&mut self, tipe: &str, decl: &crate::parser::FnDecl,
                               harap: Option<&JrgType>, span: Span) -> Result<Val, String> {
        use crate::parser::{BorrowKind, FnParam, NamedType, Pattern};
        let mut params = Vec::new();
        let mut args = Vec::new();
        for (i, p) in decl.params.iter().enumerate() {
            let nama = format!("p{}", i);
            let adalah_self = super::codegen_access::pattern_simple_name_pub(&p.pattern).as_deref() == Some("self");
            let ty = if adalah_self {
                TypeExpr::Named(NamedType { package: None, modul: None, name: tipe.to_string(), args: Vec::new(), span })
            } else { p.ty.clone() };
            params.push(FnParam { is_variadic: false, is_mut: false, borrow: BorrowKind::None,
                pattern: Pattern::Ident { name: nama.clone(), is_mut: false, span }, ty,
                default: None, lifetime: None, span });
            args.push(Expr::Ident(nama, span));
        }
        let badan = Expr::Call {
            func: Box::new(Expr::PathExpr { segments: vec![tipe.to_string(), decl.name.clone()], span }),
            args, span,
        };
        let closure = Expr::Closure { params, is_move: false, ret: None, body: std::sync::Arc::new(badan), span };
        self.closure_bernama = Some(format!("{}::{}", tipe, decl.name));
        let r = self.codegen_expr(&closure, harap);
        self.closure_bernama = None;
        r
    }

    /// Varian ber-payload enum NON-generik sebagai nilai (`let f = E::A`): closure bernama
    /// `<ctor E::A>` yang memanggil konstruktornya, seperti `nilai_metode`. Generik masih celah.
    pub(crate) fn nilai_konstruktor(&mut self, jalur: &str, harap: Option<&JrgType>,
                                    span: Span) -> Option<Result<Val, String>> {
        use crate::parser::{BorrowKind, EnumVariantKind, FnParam, Pattern};
        let segs: Vec<&str> = jalur.split("::").collect();
        let (en, var) = match segs.as_slice() {
            [e, v] => (self.tables.enums.get(*e)?.clone(), v.to_string()),
            [v] => {
                let mut cocok = self.tables.enums.values()
                    .filter(|en| en.variants.iter().any(|x| x.name == *v));
                let en = cocok.next()?.clone();
                if cocok.next().is_some() { return None; }
                (en, v.to_string())
            }
            _ => return None,
        };
        if !en.generics.params.is_empty() { return None; }
        let EnumVariantKind::Tuple(tipe) = &en.variants.iter().find(|x| x.name == var)?.kind else { return None };
        if tipe.is_empty() { return None; }
        let (mut params, mut args) = (Vec::new(), Vec::new());
        for (i, ty) in tipe.iter().enumerate() {
            let nama = format!("p{}", i);
            params.push(FnParam { is_variadic: false, is_mut: false, borrow: BorrowKind::None,
                pattern: Pattern::Ident { name: nama.clone(), is_mut: false, span }, ty: ty.clone(),
                default: None, lifetime: None, span });
            args.push(Expr::Ident(nama, span));
        }
        let badan = Expr::Call {
            func: Box::new(Expr::PathExpr { segments: vec![en.name.clone(), var.clone()], span }),
            args, span,
        };
        let closure = Expr::Closure { params, is_move: false, ret: None, body: std::sync::Arc::new(badan), span };
        self.closure_bernama = Some(format!("<ctor {}::{}>", crate::ruang_tipe::bersih(&en.name), var));
        let r = self.codegen_expr(&closure, harap);
        self.closure_bernama = None;
        Some(r)
    }

    /// Tampilan sebuah nilai fungsi, sama dengan dua mesin lain: `<closure>` kalau ia membawa
    /// lingkungan, `<fn nama>` kalau fungsi bernama (namanya dicari `@jrg_nama_fn`).
    pub(crate) fn tampilan_fungsi(&mut self, v: &Val) -> Val {
        self.pakai_nama_fn = true;
        let kode = self.fresh("fnk.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, v.text));
        let env = self.fresh("fne.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, v.text));
        let bernama = self.fresh("fnb.");
        self.emit(format!("{} = icmp eq ptr {}, null", bernama, env));
        let nama = self.fresh("fnn.");
        self.emit(format!("{} = call %JrgStr @jrg_nama_fn(ptr {})", nama, kode));
        let closure = self.intern_string("<closure>");
        let r = self.fresh("fnt.");
        self.emit(format!("{} = select i1 {}, %JrgStr {}, %JrgStr {}", r, bernama, nama, closure.text));
        Val { text: r, ty: JrgType::Str }
    }

    /// `@jrg_nama_fn(kode)`: rantai pembanding alamat pembungkus -> teks `<fn nama>`.
    pub(crate) fn definisi_nama_fn(&self) -> String {
        let mut konst = String::new();
        let mut badan = String::from("define internal %JrgStr @jrg_nama_fn(ptr %k) cold {\nentry:\n  br label %c0\n");
        let teks = |i: usize, t: &str| -> String {
            format!("@.nfn.{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                    i, t.len(), super::mangle::llvm_escape(t.as_bytes()))
        };
        for (i, (pembungkus, nama)) in self.nama_fnval.iter().enumerate() {
            let t = if nama.starts_with('<') { nama.clone() } else { format!("<fn {}>", nama) };
            konst.push_str(&teks(i, &t));
            badan.push_str(&format!(
                "c{i}:\n  %s{i} = icmp eq ptr %k, @{p}\n  br i1 %s{i}, label %r{i}, label %c{n}\n\
                 r{i}:\n  ret %JrgStr {{ ptr @.nfn.{i}, i64 {l} }}\n",
                i = i, n = i + 1, p = super::mangle::llvm_quoted_ident(pembungkus), l = t.len()));
        }
        let n = self.nama_fnval.len();
        let t = "<fn>";
        konst.push_str(&teks(n, t));
        badan.push_str(&format!("c{}:\n  ret %JrgStr {{ ptr @.nfn.{}, i64 {} }}\n}}\n", n, n, t.len()));
        konst + &badan + &self.definisi_jenis_fn()
    }

    /// `@jrg_jenis_fn(kode)`: jawaban `typeof` fungsi tanpa lingkungan — `enum_ctor:E::A`
    /// untuk konstruktor, `function` untuk yang lain.
    fn definisi_jenis_fn(&self) -> String {
        let mut konst = String::new();
        let mut badan = String::from("define internal %JrgStr @jrg_jenis_fn(ptr %k) cold {\nentry:\n  br label %c0\n");
        let ktor: Vec<(&String, String)> = self.nama_fnval.iter().filter_map(|(p, n)| {
            let isi = n.strip_prefix("<ctor ")?.strip_suffix('>')?;
            Some((p, format!("enum_ctor:{}", isi)))
        }).collect();
        for (i, (pembungkus, t)) in ktor.iter().enumerate() {
            konst.push_str(&format!("@.jfn.{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                i, t.len(), super::mangle::llvm_escape(t.as_bytes())));
            badan.push_str(&format!(
                "c{i}:\n  %s{i} = icmp eq ptr %k, @{p}\n  br i1 %s{i}, label %r{i}, label %c{n}\n\
                 r{i}:\n  ret %JrgStr {{ ptr @.jfn.{i}, i64 {l} }}\n",
                i = i, n = i + 1, p = super::mangle::llvm_quoted_ident(pembungkus), l = t.len()));
        }
        let n = ktor.len();
        konst.push_str(&format!("@.jfn.{} = private unnamed_addr constant [8 x i8] c\"function\", align 1\n", n));
        badan.push_str(&format!("c{}:\n  ret %JrgStr {{ ptr @.jfn.{}, i64 8 }}\n}}\n", n, n));
        konst + &badan
    }

    /// Sebuah nama fungsi yang dipakai sebagai nilai.
    pub fn nilai_fungsi_bernama(&mut self, name: &str, harap: Option<&JrgType>, span: Span) -> Option<Result<Val, String>> {
        // Fungsi BERSARANG juga sebuah nilai, dan ia menang atas yang
        // tingkat atas bernama sama — aturan bayangan yang sama dengan
        // pemanggilan biasa.
        let (name, decl, bersarang) = match self.cari_fn_bersarang(name) {
            Some((unik, d)) => (unik, d, true),
            None => (self.k(name), self.tables.functions.get(&self.k(name)).cloned()?, false),
        };
        let name = name.as_str();
        Some((|| {
            // Tanpa anotasi, tipe parameter fungsi ini baru diketahui dari situs
            // pemanggilnya — dan sebagai NILAI ia tidak punya situs pemanggil yang bisa …
            // Tempat yang dituju sudah bertipe `fn(..) -> ..`: itulah "situs pemanggil"-nya.
            let dari_harap: Option<Vec<JrgType>> = match harap {
                Some(JrgType::Fn(ps, _)) if ps.len() == decl.params.len() => Some(ps.clone()),
                _ => None,
            };
            let mut subst = TypeSubst::new();
            let mut params = Vec::new();
            for (i, p) in decl.params.iter().enumerate() {
                if matches!(p.ty, TypeExpr::Inferred) {
                    // Tanpa tempat bertipe yang dituju, parameternya jadi nilai BERTAG: tipenya dibawa
                    // saat jalan, seperti di dua mesin lain. Dulu bentuk ini ditolak.
                    let tipe = dari_harap.as_ref().map_or(JrgType::Any, |ps| ps[i].clone());
                    subst.insert(format!("$param{}", i), tipe);
                }
                params.push(self.tipe_param(p, i, &subst)
                    .map_err(|e| super::galat::beri_span_bila_kosong(e, span))?);
            }
            let simbol = if subst.is_empty() { name.to_string() } else { super::mangle::mangle_fn(name, &params) };
            let ret = self.tipe_return_duga(&decl, &subst, &simbol, None, decl.params.len())
                .map_err(|e| super::galat::beri_span_bila_kosong(e, span))?;

            if bersarang { self.catat_sarang(&simbol); }
            let u = self.unit_simbol_dari(name);
            self.unit_simbol.entry(simbol.clone()).or_insert(u);
            self.enqueue_fn(name, &decl, subst, simbol.clone(), None);
            let name = simbol.as_str();
            let pembungkus = format!("fnval.{}", name);
            if !self.nama_fnval.iter().any(|(p, _)| *p == pembungkus) {
                self.nama_fnval.push((pembungkus.clone(), super::modul::tampil(&decl.name).to_string()));
            }
            if !self.is_fn_emitted_or_queued(&pembungkus) {
                self.tandai_fn_terpakai(&pembungkus);
                self.antre_fn(KerjaFn::Bungkus {
                    pembungkus: pembungkus.clone(),
                    asli: name.to_string(),
                    params: params.clone(),
                    ret: ret.clone(),
                });
            }
            let ty = JrgType::Fn(params, Box::new(ret));
            let a = self.fresh("fnv.");
            self.emit(format!("{} = insertvalue %JrgFn undef, ptr @{}, 0",
                a, super::mangle::llvm_quoted_ident(&pembungkus)));
            let b = self.fresh("fnv.");
            // Lingkungan `null`: itulah yang membedakan fungsi bernama dari
            // closure saat `typeof` ditanya.
            self.emit(format!("{} = insertvalue %JrgFn {}, ptr null, 1", b, a));
            Ok(Val { text: b, ty })
        })())
    }

    /// Panggil sebuah nilai `%JrgFn` dengan argumen yang SUDAH berupa nilai.
    pub fn panggil_val(&mut self, f: &Val, args: &[Val]) -> Result<Val, String> {
        let JrgType::Fn(params, ret) = f.ty.clone() else {
            return Err(format!("`{}` bukan sesuatu yang bisa dipanggil", f.ty.display_name()));
        };
        if args.len() != params.len() {
            return Err(format!("nilai fungsi ini butuh {} argumen, diberi {}",
                params.len(), args.len()));
        }
        let kode = self.fresh("fnk.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, f.text));
        let env = self.fresh("fne.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, f.text));
        let mut pakai = vec![format!("ptr {}", env)];
        for (a, t) in args.iter().zip(params.iter()) {
            let v = self.coerce(a, t);
            pakai.push(format!("{} {}", t.llvm_ty(), v.text));
        }
        if *ret == JrgType::Unit || *ret == JrgType::Never {
            self.emit(format!("call void {}({})", kode, pakai.join(", ")));
            return Ok(Val::unit());
        }
        let r = self.fresh("fnr.");
        self.emit(format!("{} = call {} {}({})", r, ret.llvm_ty(), kode, pakai.join(", ")));
        Ok(Val { text: r, ty: (*ret).clone() })
    }

    /// Panggil sebuah nilai `%JrgFn`.
    pub fn panggil_nilai_fungsi(&mut self, f: &Val, args: &[Expr], span: Span) -> Result<Val, String> {
        let JrgType::Fn(params, ret) = f.ty.clone() else {
            return Err(super::galat::tandai(span, format!(
                "`{}` bukan sesuatu yang bisa dipanggil", f.ty.display_name())));
        };
        if args.len() != params.len() {
            return Err(super::galat::tandai(span, format!(
                "nilai fungsi ini butuh {} argumen, diberi {}", params.len(), args.len())));
        }
        let kode = self.fresh("fnk.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, f.text));
        let env = self.fresh("fne.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, f.text));

        let mut pakai = vec![format!("ptr {}", env)];
        for (a, t) in args.iter().zip(params.iter()) {
            let v = self.codegen_expr(a, Some(t))?;
            let v = self.coerce(&v, t);
            // Parameter callee adalah pengikatan baru, jadi ia harus punya
            // salinannya sendiri — aturan yang sama dengan panggilan biasa.
            let v = self.ambil_milik(a, v)?;
            pakai.push(format!("{} {}", t.llvm_ty(), v.text));
        }
        let sig: Vec<String> = std::iter::once("ptr".to_string())
            .chain(params.iter().map(|t| t.llvm_ty())).collect();
        if *ret == JrgType::Unit || *ret == JrgType::Never {
            self.emit(format!("call void {}({})", kode, pakai.join(", ")));
            return Ok(Val::unit());
        }
        let r = self.fresh("fnr.");
        self.emit(format!("{} = call {} {}({})", r, ret.llvm_ty(), kode, pakai.join(", ")));
        let _ = sig;
        Ok(Val { text: r, ty: (*ret).clone() })
    }
}

impl Codegen {
    /// Nama yang DIDEKLARASIKAN tapi dipakai sebagai nilai: kalimat interpreter, bukan
    /// "tidak ditemukan"; konstruktor sebagai nilai disebut celah backend ini.
    pub(crate) fn penjelasan_bukan_nilai(&self, jalur: &str) -> Option<String> {
        use crate::interpreter as it;
        let ktor = |t: &str, v: &str| format!(
            "konstruktor `{t}::{v}` dipakai sebagai NILAI (tanpa dipanggil), dan backend \
             `--emit-llvm` belum mendukung konstruktor sebagai nilai\n\
             bantuan: panggil langsung — `{t}::{v}(x)` — atau jalankan tanpa `--emit-llvm`",
            t = t, v = v);
        let segs: Vec<&str> = jalur.split("::").collect();
        match segs.as_slice() {
            ["Some"] | ["Option", "Some"] => return Some(ktor("Option", "Some")),
            ["Ok"] | ["Result", "Ok"] => return Some(ktor("Result", "Ok")),
            ["Err"] | ["Result", "Err"] => return Some(ktor("Result", "Err")),
            [e, v] => {
                let en = self.tables.enums.get(*e)?;
                let var = en.variants.iter().find(|x| x.name == *v)?;
                return match &var.kind {
                    EnumVariantKind::Struct(f) => Some(it::pesan_varian_berfield(
                        e, v, &f.iter().map(|x| x.name.clone()).collect::<Vec<_>>())),
                    EnumVariantKind::Tuple(f) if !f.is_empty() => Some(ktor(e, v)),
                    _ => None,
                };
            }
            [n] => {
                if self.tables.structs.contains_key(*n) { return Some(it::pesan_nama_struct(n)); }
                if let Some(en) = self.tables.enums.get(*n) {
                    return Some(it::pesan_nama_enum(n, en.variants.first().map(|v| v.name.as_str())));
                }
                if self.tables.traits.iter().any(|t| t == n) { return Some(it::pesan_nama_trait(n)); }
                // Varian telanjang: ber-payload (`A` milik `enum E { A(int) }`) atau ber-field.
                // Urutan nama, bukan urutan `HashMap` — yang itu berganti tiap proses.
                let mut semua: Vec<_> = self.tables.enums.values().collect();
                semua.sort_by(|a, b| a.name.cmp(&b.name));
                for en in semua {
                    for v in en.variants.iter().filter(|v| v.name == *n) {
                        match &v.kind {
                            EnumVariantKind::Tuple(f) if !f.is_empty() => return Some(ktor(&en.name, n)),
                            EnumVariantKind::Struct(f) => return Some(it::pesan_varian_berfield(
                                &en.name, n, &f.iter().map(|x| x.name.clone()).collect::<Vec<_>>())),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// `B` untuk `enum E { A(int), B }` — varian tanpa payload yang disebut tanpa
    /// kualifikasi.
    pub fn varian_unit_telanjang(&mut self, name: &str, span: Span) -> Option<Result<Val, String>> {
        let cocok = self.enum_punya_varian(name, true);
        match cocok.len() {
            0 => None,
            1 => Some(self.buat_varian_enum(&cocok[0], name, &[], span)),
            _ => Some(Err(super::galat::tandai(span, format!(
                "`{}` ambigu — varian tanpa payload dengan nama itu ada di {}\n\
                 bantuan: tulis lengkap, mis. `{}::{}`",
                name, cocok.join(" dan "), cocok[0], name)))),
        }
    }
}

impl Codegen {
    /// Enum mana saja yang punya varian bernama `name`.
    pub fn enum_punya_varian(&self, name: &str, harus_unit: bool) -> Vec<String> {
        use crate::parser::EnumVariantKind;
        let mut cocok: Vec<String> = self.tables.enums.iter()
            .filter(|(_, ed)| ed.variants.iter().any(|v| {
                v.name == name
                    && (!harus_unit || matches!(v.kind, EnumVariantKind::Unit))
                    && (harus_unit || !matches!(v.kind, EnumVariantKind::Unit))
            }))
            .map(|(n, _)| n.clone())
            .collect();
        // Urutan tetap: IR yang sama untuk sumber yang sama, dan pesan
        // ambiguitas yang menyebut nama dalam urutan yang sama tiap kali.
        cocok.sort();
        cocok
    }

    pub fn buat_varian_enum(&mut self, enum_name: &str, variant: &str, args: &[Expr], span: Span)
        -> Result<Val, String>
    {
        self.construct_user_enum_variant_pub(enum_name, variant, args, span)
    }
}
