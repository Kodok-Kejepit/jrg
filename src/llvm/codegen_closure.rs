// src/llvm/codegen_closure.rs — llvm/codegen_closure.rs — closure.

use std::collections::HashSet;

use crate::lexer::Span;
use crate::parser::{Expr, FnParam, Pattern, TypeExpr};

use super::codegen::{Codegen, KerjaFn, Val};
use super::types::JrgType;
use crate::llvm::TypeSubst;

/// Nama yang harus naik ke heap di fungsi ini.
pub fn nama_yang_naik_ke_heap(b: &crate::parser::Block) -> HashSet<String> {
    if !crate::interpreter::capture::blok_ada_closure(b) {
        return HashSet::new();
    }
    let mut out = HashSet::new();
    crate::interpreter::capture::kumpul_nama_blok(b, &mut out);
    out
}

/// Sama, untuk badan sebuah closure.
pub fn nama_naik_ke_heap_ekspr(e: &Expr) -> HashSet<String> {
    if !crate::interpreter::capture::ada_closure(e) {
        return HashSet::new();
    }
    crate::interpreter::capture::names_mentioned(e)
}

pub fn pola_nama(p: &Pattern) -> Option<String> {
    super::codegen_access::pattern_simple_name_pub(p)
}

impl Codegen {
    /// Alokasikan sebuah lokal DI HEAP, bukan di `alloca`.
    pub fn kotak_lokal(&mut self, nama: &str, ty: &JrgType) -> String {
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let uk = self.types.sizeof(ty).max(1);
        let al = self.types.alignof(ty).max(1);
        let p = self.fresh("kotak.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", p, uk, al));
        self.define_local(nama, p.clone(), ty.clone());
        p
    }

    pub fn codegen_closure(
        &mut self, params: &[FnParam], body: &Expr, is_move: bool,
        harap: Option<&JrgType>, span: Span,
    ) -> Result<Val, String> {
        let harap_fn = match harap {
            Some(JrgType::Fn(ps, r)) if ps.len() == params.len() => Some((ps.clone(), (**r).clone())),
            _ => None,
        };
        self.closure_dgn(params, body, is_move, harap_fn.map(|(p, r)| (p, Some(r))), span)
    }

    /// Sama, tapi tipe parameternya diketahui sementara tipe hasilnya belum.
    pub fn closure_dgn_params(
        &mut self, params: &[FnParam], body: &Expr, ptys: &[JrgType],
        is_move: bool, span: Span,
    ) -> Result<Val, String> {
        self.closure_dgn(params, body, is_move, Some((ptys.to_vec(), None)), span)
    }

    /// Closure `move` tanpa parameter dan tanpa hasil — badan sebuah goroutine.
    pub(crate) fn closure_tanpa_hasil(&mut self, body: &Expr, span: Span) -> Result<Val, String> {
        self.closure_dgn(&[], body, true, Some((Vec::new(), Some(JrgType::Unit))), span)
    }

    /// Sama, untuk parameter bertipe `fn(..) -> []K`: hasil berukuran tetap jadi array biasa.
    pub fn closure_dgn_params_luruh(
        &mut self, params: &[FnParam], body: &Expr, ptys: &[JrgType],
        is_move: bool, span: Span,
    ) -> Result<Val, String> {
        self.hasil_closure_luruh = true;
        let r = self.closure_dgn(params, body, is_move, Some((ptys.to_vec(), None)), span);
        self.hasil_closure_luruh = false;
        r
    }

    fn closure_dgn(
        &mut self, params: &[FnParam], body: &Expr, is_move: bool,
        harap: Option<(Vec<JrgType>, Option<JrgType>)>, span: Span,
    ) -> Result<Val, String> {
        // Tipe parameter closure: dari anotasinya, atau dari tipe yang
        // diharapkan di posisi ini (`fn pakai(f: fn(int) -> int)`).
        let nama_tampil = self.closure_bernama.take();
        let harap_fn: Option<(Vec<JrgType>, Option<JrgType>)> = match harap {
            Some((ps, r)) if ps.len() == params.len() => Some((ps, r)),
            _ => None,
        };
        let mut ptys = Vec::with_capacity(params.len());
        for (i, p) in params.iter().enumerate() {
            let t = if matches!(p.ty, TypeExpr::Inferred) {
                match &harap_fn {
                    Some((ps, _)) => ps[i].clone(),
                    // Situs pemanggilan yang sudah dipindai lebih dulu: `let byparam = |p| p`
                    // diikuti `byparam(5)` memberi tahu bahwa `p` sebuah `int`.
                    None => match self.nama_diikat.clone()
                        .and_then(|n| self.tipe_argumen.get(&n).cloned())
                        .or_else(|| self.rantai_hasil.first().cloned())
                        .filter(|ts| ts.len() == params.len())
                    {
                        Some(ts) => ts[i].clone(),
                        // Tidak terlihat dari mana pun: nilai bertag (lapis B) — closure-nya jalan
                        // untuk isi apa saja, dan anotasi (`|x: int| ..`) memberi tipe yang cepat.
                        None => JrgType::Any,
                    },
                }
            } else {
                // Parameter tipe fungsi yang memuat closure ini ikut terlihat (`|x: T| x`).
                let luar = self.cur_fn_generics.clone();
                self.resolve_type_with_subst(&p.ty, &luar)
                    .map_err(|e| super::galat::tandai(span, e))?
            };
            ptys.push(t);
        }

        // Tangkapan: nama yang disebut badan, dikurangi parameternya sendiri,
        // yang benar-benar ADA sebagai lokal di sini.
        let mut disebut = crate::interpreter::capture::names_mentioned(body);
        for p in params {
            if let Some(n) = pola_nama(&p.pattern) { disebut.remove(&n); }
        }
        let mut tangkap: Vec<(String, JrgType, String)> = Vec::new();
        let mut nama_urut: Vec<String> = disebut.into_iter().collect();
        nama_urut.sort();   // urutan tetap: IR yang sama untuk sumber yang sama
        for n in nama_urut {
            if let Some((ptr, ty)) = self.lookup_local(&n) {
                tangkap.push((n, ty, ptr));
            }
        }

        // Lingkungan: larik penunjuk ke kotak masing-masing tangkapan. Yang
        // disalin penunjuknya, bukan nilainya — itulah yang membuat mutasi
        // terlihat dari kedua arah.
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let n_env = tangkap.len().max(1) as u64;   // minimal 1: lihat catatan `typeof`
        let env = self.fresh("env.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 8)", env, n_env * 8));
        for (i, (_, ty, ptr)) in tangkap.iter().enumerate() {
            // `move` menyimpan penunjuk ke KOTAK BARU berisi salinan, bukan ke kotak
            // milik binding aslinya.
            let simpan = if is_move {
                let uk = self.types.sizeof(ty).max(1);
                let al = self.types.alignof(ty).max(1);
                let baru = self.fresh("mvkotak.");
                self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", baru, uk, al));
                let nilai = self.fresh("mvnilai.");
                self.emit(format!("{} = load {}, ptr {}", nilai, ty.llvm_ty(), ptr));
                let nilai = self.salin_dalam(&Val { text: nilai, ty: ty.clone() }).text;
                self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), nilai, baru));
                baru
            } else {
                ptr.clone()
            };
            let f = self.fresh("envf.");
            self.emit(format!("{} = getelementptr ptr, ptr {}, i64 {}", f, env, i));
            self.emit(format!("store ptr {}, ptr {}", simpan, f));
        }

        // Rantai dihitung DI SINI, sebelum tipe hasil disimpulkan — karena penyimpul
        // itulah yang perlu tahu tipe parameter closure yang DIKEMBALIKAN badan ini.
        let rantai_hasil = self.rantai_hasil_untuk_badan();
        let luruhkan = std::mem::replace(&mut self.hasil_closure_luruh, false);
        let ret = match &harap_fn {
            Some((_, Some(r))) => r.clone(),
            _ => self.simpulkan_ret_closure(params, &ptys, body, &tangkap, &rantai_hasil, span)?,
        };
        let ret = if luruhkan { super::codegen_any::luruh(&ret) } else { ret };

        let nama_fn = self.nama_closure_baru();
        self.tandai_fn_terpakai(&nama_fn);
        self.antre_fn(KerjaFn::Closure {
            nama: nama_fn.clone(),
            params: params.iter().enumerate()
                .map(|(i, p)| (pola_nama(&p.pattern).unwrap_or_else(|| format!("_arg{}", i)), ptys[i].clone()))
                .collect(),
            ret: ret.clone(),
            body: body.clone(),
            tangkap: tangkap.iter().map(|(n, t, _)| (n.clone(), t.clone())).collect(),
            rantai_hasil,
        });

        // Closure sintetis untuk `T::m`: tanpa lingkungan, jadi ia tampil dan ber-`typeof` fungsi.
        let env = match nama_tampil {
            Some(n) => { self.nama_fnval.push((nama_fn.clone(), n)); "null".to_string() }
            None => env,
        };
        let a = self.fresh("clo.");
        self.emit(format!("{} = insertvalue %JrgFn undef, ptr @{}, 0",
            a, super::mangle::llvm_quoted_ident(&nama_fn)));
        let b = self.fresh("clo.");
        self.emit(format!("{} = insertvalue %JrgFn {}, ptr {}, 1", b, a, env));
        Ok(Val { text: b, ty: JrgType::Fn(ptys, Box::new(ret)) })
    }
}

impl Codegen {
    /// Tipe return sebuah closure yang tidak punya konteks tipe.
    pub(crate) fn rantai_hasil_untuk_badan(&self) -> Vec<Vec<JrgType>> {
        match &self.nama_diikat {
            Some(n) => {
                let mut r = Vec::new();
                let mut k = n.clone();
                loop {
                    k.push_str("$hasil");
                    match self.tipe_argumen.get(&k) {
                        Some(ts) => r.push(ts.clone()),
                        None => break,
                    }
                }
                r
            }
            None => self.rantai_hasil.iter().skip(1).cloned().collect(),
        }
    }

    fn simpulkan_ret_closure(
        &mut self, params: &[FnParam], ptys: &[JrgType], body: &Expr,
        tangkap: &[(String, JrgType, String)], rantai: &[Vec<JrgType>], span: Span,
    ) -> Result<JrgType, String> {
        // Mesin penyimpulan yang SAMA dengan fungsi tanpa anotasi. Closure
        // dijadikan `FnDecl` sementara supaya tidak ada penyimpul kedua yang
        // bisa menyimpang dari yang pertama.
        let mut palsu = crate::parser::FnDecl {
            vis: crate::parser::Visibility::Private,
            is_async: false,
            name: "$closure".to_string(),
            generics: Default::default(),
            params: params.to_vec(),
            ret_type: None,
            where_clause: Vec::new(),
            body: match body {
                Expr::Block(b) => crate::parser::FnBody::Block(b.clone()).into(),
                lain => crate::parser::FnBody::Block(crate::parser::Block {
                    stmts: Vec::new(),
                    tail: Some(Box::new(lain.clone())),
                    span,
                }).into(),
            },
            span,
        };
        let mut subst = TypeSubst::new();
        for (i, t) in ptys.iter().enumerate() {
            subst.insert(format!("$param{}", i), t.clone());
        }
        // Rantai dititipkan lewat `subst`, bukan lewat keadaan `self`.
        crate::llvm::infer::semai_rantai(&mut subst, rantai);
        // Tangkapan DAN `var` tingkat modul.
        let mut awal: Vec<(String, JrgType)> =
            tangkap.iter().map(|(n, t, _)| (n.clone(), t.clone())).collect();
        for (n, (_, t)) in &self.var_global {
            awal.push((n.clone(), t.clone()));
        }
        if let Some(t) = self.simpulkan_tipe_return_dgn(&palsu, &subst, &awal) {
            self.wujudkan_opsi();
            return Ok(t);
        }
        // Tidak ada satu pun `return` bernilai dan tidak ada ekspresi ekor:
        // closure ini memang tidak menghasilkan apa-apa. Itu KESIMPULAN,
        // bukan kegagalan menyimpulkan.
        if let crate::parser::FnBody::Block(b) = std::sync::Arc::make_mut(&mut palsu.body) {
            if b.tail.is_none() && !crate::llvm::infer::blok_punya_return_bernilai(b) {
                return Ok(JrgType::Unit);
            }
        }
        // Penyimpul statis menyerah: badannya diturunkan di salinan, seperti fungsi tanpa anotasi.
        self.benih_lokal = tangkap.iter().map(|(n, t, _)| (n.clone(), t.clone())).collect();
        let nama = format!("$duga_closure{}_{}", span.lo.0, span.hi.0);
        let duga = self.tipe_return_duga(&palsu, &subst, &nama, None, params.len());
        self.benih_lokal.clear();
        if let Ok(t) = duga {
            if !matches!(t, JrgType::Unit | JrgType::Never) { return Ok(t); }
            // Penduganya SELESAI tanpa galat dan tidak melihat nilai dikembalikan: badannya
            // memang tidak menghasilkan apa-apa (`|x| wok(x)`).
            if !self.galat_duga.contains_key(&nama) { return Ok(JrgType::Unit); }
        }
        Err(super::galat::tandai(span,
            "tipe hasil closure ini tidak bisa disimpulkan\n\
             bantuan: pakai closure-nya di tempat yang tipenya sudah diketahui, mis. \
             parameter bertipe `fn(int) -> int`\n\
             catatan: menebak tipe hasil berarti nilainya di-coerce ke tipe yang salah — \
             itu jawaban salah yang tidak memberi pesan".to_string()))
    }

    /// Pancarkan badan sebuah closure sebagai fungsi tersendiri.
    pub fn pancarkan_closure(
        &mut self, nama: &str, params: &[(String, JrgType)], ret: &JrgType,
        body: &Expr, tangkap: &[(String, JrgType)], rantai_hasil: &[Vec<JrgType>],
    ) -> Result<(), String> {
        self.mulai_fungsi_kosong(ret.clone());
        self.cur_self_ty = self.ruang_simbol.get(nama).cloned();
        self.tangkapan = nama_naik_ke_heap_ekspr(body);
        let masuk_lama = std::mem::replace(
            &mut self.tangkap_masuk, tangkap.iter().map(|(n, _)| n.clone()).collect());
        // Rantai dipasang SELAMA badan ini diturunkan, lalu dikembalikan — badan ini
        // bisa berada di tengah pengurasan antrean, dan meninggalkan keadaan yang …
        let rantai_lama = std::mem::replace(&mut self.rantai_hasil, rantai_hasil.to_vec());
        // Closure tidak pernah MENEBAK tipe hasilnya (ia menolak), jadi penjaga
        // `periksa_nilai_terbuang` milik fungsi pembungkusnya tidak berlaku di sini.
        let ditebak_lama = std::mem::replace(&mut self.ret_ditebak, false);
        let nama_lama = std::mem::replace(&mut self.nama_diikat, None);

        // Tangkapan: `%env` adalah larik penunjuk ke kotak masing-masing.
        // Yang diikat sebagai lokal adalah PENUNJUKNYA, jadi baca dan tulis
        // di dalam closure mengenai kotak yang sama dengan di luar.
        for (i, (n, t)) in tangkap.iter().enumerate() {
            let f = self.fresh("cenv.");
            self.emit(format!("{} = getelementptr ptr, ptr %env, i64 {}", f, i));
            let p = self.fresh("cbox.");
            self.emit(format!("{} = load ptr, ptr {}", p, f));
            self.define_local(n, p, t.clone());
        }

        let mut sig = vec!["ptr %env".to_string()];
        for (i, (n, t)) in params.iter().enumerate() {
            sig.push(format!("{} %p{}", t.llvm_ty(), i));
            // Lewat `alloca_local`, bukan `alloca_typed` + `define_local`: parameter
            // sebuah closure juga bisa ditangkap closure lain, dan pintu pengikatan …
            let slot = self.alloca_local(n, t.clone());
            if *t != JrgType::Unit {
                self.emit(format!("store {} %p{}, ptr {}", t.llvm_ty(), i, slot));
            }
        }

        // `defer` di badan closure: bingkainya milik closure ini, bukan pembungkusnya.
        if super::codegen_tunda::di_ekspr(body) { self.buka_bingkai_tunda(ret); }

        let v = self.codegen_expr(body, Some(ret))?;
        self.emit_return(&v, ret);
        if !self.is_terminated() {
            self.emit_return(&Val::unit(), ret);
        }
        self.pancarkan_blok_tunda(ret)?;

        let badan = self.take_function_body();
        self.tambah_definisi(format!(
            "define internal {} @{}({}) local_unnamed_addr nounwind {{\n{}}}\n",
            ret.llvm_ret_ty(), super::mangle::llvm_quoted_ident(nama), sig.join(", "), badan));
        self.selesai_fungsi_kosong();
        self.tangkap_masuk = masuk_lama;
        self.rantai_hasil = rantai_lama;
        self.ret_ditebak = ditebak_lama;
        self.nama_diikat = nama_lama;
        Ok(())
    }
}
