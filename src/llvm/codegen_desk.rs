//! Deskriptor nilai fungsi: nama dan parameter fungsi yang kodenya ada di balik sebuah `%JrgFn`, untuk argumen
//! bernama ke fungsi yang baru diketahui saat jalan. Alasannya di `dokumentasi/src/llvm/codegen_desk.md`.

use super::codegen::{Codegen, Val};
use super::codegen_konversi::{Cetak, PENANDA};
use super::types::JrgType;
use crate::lexer::Span;
use crate::parser::{Expr, FnParam, LitKind};

/// Fungsi di balik sebuah alamat kode: nama yang disebut kalimat acuan dan parameternya.
#[derive(Clone)]
pub(crate) struct DeskFn {
    pub kode: String,
    pub disebut: String,
    pub params: Vec<FnParam>,
}

/// Situs `f(...v)` atas nilai fungsi: aritas tipenya.
#[derive(Clone)]
pub(crate) struct SitusSebar {
    pub aritas: usize,
}

/// Situs `f(a: 1)` atas nilai fungsi: aritas tipenya, jumlah posisional, nama yang ditulis.
#[derive(Clone)]
pub(crate) struct SitusBernama {
    pub aritas: usize,
    pub posisional: usize,
    pub nama: Vec<String>,
}

fn nama_param(p: &FnParam, i: usize) -> String {
    super::codegen_closure::pola_nama(&p.pattern).unwrap_or_else(|| format!("_arg{}", i))
}

impl Codegen {
    /// Catat fungsi di balik alamat `kode` (sekali per alamat).
    pub(crate) fn catat_desk(&mut self, kode: &str, disebut: &str, params: &[FnParam]) {
        if self.desk_fn.iter().any(|d| d.kode == kode) { return; }
        self.desk_fn.push(DeskFn { kode: kode.to_string(), disebut: disebut.to_string(), params: params.to_vec() });
    }

    /// `f(a: 1)` atas nilai fungsi yang baru diketahui saat jalan. Urutan argumen tiap fungsi yang mungkin disusun
    /// SAAT KOMPILASI oleh `susun_argumen_bernama` acuan (jadi kalimat galatnya sama); saat jalan cuma barisnya dipilih.
    pub(crate) fn terap_bernama_dinamis(&mut self, args: &[Expr], span: Span) -> Option<Result<Val, String>> {
        let [f_e, Expr::Array { elems: pos, .. }, Expr::Array { elems: nama, .. }, Expr::Array { elems: nilai, .. }] = args
            else { return None };
        let nama: Vec<String> = nama.iter().map(|n| match n {
            Expr::Literal(LitKind::Str(s), _) => Some(s.clone()),
            _ => None,
        }).collect::<Option<_>>()?;
        let f = match self.codegen_expr(f_e, None) { Ok(v) => v, Err(g) => return Some(Err(g)) };
        let JrgType::Fn(ps, _) = f.ty.clone() else { return None };
        // Urutan evaluasi acuan: fungsinya, posisional, lalu nilai bernama — urutan tulis.
        let mut nilai_v = Vec::new();
        for e in pos.iter().chain(nilai.iter()) {
            match self.codegen_expr(e, None) { Ok(v) => nilai_v.push(v), Err(g) => return Some(Err(g)) }
        }
        let k = ps.len();
        let situs = self.situs_bernama.len();
        self.situs_bernama.push(SitusBernama { aritas: k, posisional: pos.len(), nama });
        self.pakai_desk = true;
        let (kode, env, id) = (self.fresh("dkk."), self.fresh("dke."), self.fresh("dki."));
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, f.text));
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, f.text));
        self.emit(format!("{} = call i32 @jrg_desk_id(ptr {}, ptr {})", id, kode, env));
        let dikenal = self.fresh("dkd.");
        self.emit(format!("{} = icmp sge i32 {}, 0", dikenal, id));
        let (l_kenal, l_asing) = (self.fresh_label("dk.kenal."), self.fresh_label("dk.asing."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !1", dikenal, l_kenal, l_asing));
        self.start_block(l_asing);
        // Builtin dan konstruktor memang tanpa nama parameter (kalimat acuan); `typeof`-nya bukan `function`.
        self.pakai_nama_fn = true;
        let (jn, jl, fungsi) = (self.fresh("dkjn."), self.fresh("dkjl."), self.fresh("dkjf."));
        self.emit(format!("{} = call %JrgStr @jrg_jenis_fn(ptr {})", jn, kode));
        self.emit(format!("{} = extractvalue %JrgStr {}, 1", jl, jn));
        self.emit(format!("{} = icmp eq i64 {}, 8", fungsi, jl));
        let (l_celah, l_bawaan) = (self.fresh_label("dk.celah."), self.fresh_label("dk.bawaan."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", fungsi, l_celah, l_bawaan));
        self.start_block(l_bawaan);
        self.panik_sisip(&crate::interpreter::pesan_bernama_tanpa_param(PENANDA), &Cetak::Teks(Val { text: jn, ty: JrgType::Str }));
        self.start_block(l_celah);
        self.panik("argumen bernama (`f(a: 1)`) ke nilai fungsi ini belum didukung backend LLVM: nama \
                    parameternya tidak tercatat (method sebagai nilai). Interpreter dan `--vm` mendukungnya");
        self.start_block(l_kenal);
        let ent = format!("{{ i32, [{} x i32], %JrgStr }}", k);
        let (ep, jp, jenis) = (self.fresh("dkep."), self.fresh("dkjp."), self.fresh("dkj."));
        self.emit(format!("{} = getelementptr {}, ptr @bern.{}, i32 {}", ep, ent, situs, id));
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", jp, ent, ep));
        self.emit(format!("{} = load i32, ptr {}", jenis, jp));
        let ok = self.fresh("dko.");
        self.emit(format!("{} = icmp eq i32 {}, 0", ok, jenis));
        let (l_ok, l_galat) = (self.fresh_label("dk.ok."), self.fresh_label("dk.galat."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !1", ok, l_ok, l_galat));
        self.start_block(l_galat);
        let (mp, m) = (self.fresh("dkmp."), self.fresh("dkm."));
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 2", mp, ent, ep));
        self.emit(format!("{} = load %JrgStr, ptr {}", m, mp));
        self.panik_teks(&m);
        self.start_block(l_ok);
        let mut akhir = Vec::new();
        for (j, pj) in ps.iter().enumerate() {
            let (ip, idx) = (self.fresh("dkip."), self.fresh("dkix."));
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1, i32 {}", ip, ent, ep, j));
            self.emit(format!("{} = load i32, ptr {}", idx, ip));
            let slot = self.alloca_typed("dks.", pj);
            let gab = self.fresh_label("dk.gab.");
            let lain = self.fresh_label("dk.lain.");
            let label: Vec<String> = (0..nilai_v.len()).map(|_| self.fresh_label("dk.arg.")).collect();
            let kasus: Vec<String> = label.iter().enumerate().map(|(m, l)| format!("i32 {}, label %{}", m, l)).collect();
            self.terminate(format!("switch i32 {}, label %{} [ {} ]", idx, lain, kasus.join(" ")));
            for (m, l) in label.into_iter().enumerate() {
                self.start_block(l);
                let v = self.coerce_diam(&nilai_v[m], pj);
                if let Some(v) = v {
                    self.emit(format!("store {} {}, ptr {}", pj.llvm_ty(), v.text, slot));
                    self.terminate(format!("br label %{}", gab));
                } else {
                    self.panik(&format!(
                        "argumen bernama: nilai bertipe `{}` tidak bisa mengisi parameter bertipe `{}` — di backend \
                         `--emit-llvm` tipe sebuah tempat tetap (interpreter dan `--vm` menyimpan tipe bersama nilainya)",
                        nilai_v[m].ty.nama_typeof(), pj.nama_typeof()));
                }
            }
            self.start_block(lain);
            self.panik("bug di jrgc: argumen bernama tanpa sumber");
            self.start_block(gab);
            akhir.push(self.load_place(&slot, pj));
        }
        let _ = span;
        Some(self.panggil_val(&f, &akhir))
    }

    /// `f(...v)` atas nilai fungsi yang baru diketahui saat jalan. Jumlah yang pas dipanggil; yang lain gagal dengan
    /// kalimat acuan milik fungsi di balik nilainya, dari tabel situs ini.
    pub(crate) fn terap_sebar_dinamis(&mut self, args: &[Expr], span: Span) -> Option<Result<Val, String>> {
        let [f_e, daftar] = args else { return None };
        let f = match self.codegen_expr(f_e, None) { Ok(v) => v, Err(g) => return Some(Err(g)) };
        let JrgType::Fn(ps, _) = f.ty.clone() else { return None };
        let k = ps.len();
        let v = match self.codegen_expr(daftar, None) { Ok(v) => v, Err(g) => return Some(Err(g)) };
        if !matches!(v.ty, JrgType::Array(..) | JrgType::Slice(_)) { return None; }
        let n = match self.panjang_koleksi(&v, span) { Ok(n) => n, Err(g) => return Some(Err(g)) };
        let ptr = self.spill_to_place(&v);
        let isi = self.lokal_sementara(ptr, v.ty.clone(), span);
        let pas = self.fresh("dsp.");
        self.emit(format!("{} = icmp eq i64 {}, {}", pas, n.text, k));
        let (l_pas, l_galat) = (self.fresh_label("ds.pas."), self.fresh_label("ds.galat."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !1", pas, l_pas, l_galat));
        self.start_block(l_galat);
        let situs = self.situs_sebar.len();
        self.situs_sebar.push(SitusSebar { aritas: k });
        self.pakai_desk = true;
        let (kode, env, id) = (self.fresh("dsk."), self.fresh("dse."), self.fresh("dsi."));
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, f.text));
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, f.text));
        self.emit(format!("{} = call i32 @jrg_desk_id(ptr {}, ptr {})", id, kode, env));
        let dikenal = self.fresh("dsd.");
        self.emit(format!("{} = icmp sge i32 {}, 0", dikenal, id));
        let (l_kenal, l_asing) = (self.fresh_label("ds.kenal."), self.fresh_label("ds.asing."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", dikenal, l_kenal, l_asing));
        self.start_block(l_asing);
        self.panik("spread di pemanggilan (`f(...v)`) dengan jumlah argumen yang tidak pas ke nilai fungsi ini belum \
                    didukung backend LLVM: parameternya tidak tercatat (builtin, konstruktor, atau method sebagai \
                    nilai). Interpreter dan `--vm` mendukungnya");
        self.start_block(l_kenal);
        let ent = format!("{{ [{} x %JrgStr], %JrgStr, %JrgStr }}", k);
        let ep = self.fresh("dsep.");
        self.emit(format!("{} = getelementptr {}, ptr @sebar.{}, i32 {}", ep, ent, situs, id));
        let lebih = self.fresh("dsl.");
        self.emit(format!("{} = icmp ugt i64 {}, {}", lebih, n.text, k));
        let (l_lebih, l_kurang) = (self.fresh_label("ds.lebih."), self.fresh_label("ds.kurang."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", lebih, l_lebih, l_kurang));
        self.start_block(l_lebih);
        let mut bagian = Vec::new();
        for i in 1..=2 {
            let (bp, b) = (self.fresh("dsbp."), self.fresh("dsb."));
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", bp, ent, ep, i));
            self.emit(format!("{} = load %JrgStr, ptr {}", b, bp));
            bagian.push(self.split_str(&Val { text: b, ty: JrgType::Str }));
        }
        self.ensure_extern(
            "declare void @jrg_panic_sisip_bulat(ptr, i64, i64, i64, i32, ptr, i64) cold noreturn",
            "jrg_panic_sisip_bulat");
        self.emit(format!("call void @jrg_panic_sisip_bulat(ptr {}, i64 {}, i64 {}, i64 0, i32 1, ptr {}, i64 {})",
            bagian[0].0, bagian[0].1, n.text, bagian[1].0, bagian[1].1));
        self.terminate("unreachable");
        self.start_block(l_kurang);
        let (mp, m) = (self.fresh("dsmp."), self.fresh("dsm."));
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0, i64 {}", mp, ent, ep, n.text));
        self.emit(format!("{} = load %JrgStr, ptr {}", m, mp));
        self.panik_teks(&m);
        self.start_block(l_pas);
        let mut akhir = Vec::new();
        for (j, pj) in ps.iter().enumerate() {
            let e = Expr::Index { obj: Box::new(isi.clone()), idx: Box::new(Expr::Literal(LitKind::Int(j as u128), span)), span };
            let x = match self.codegen_expr(&e, Some(pj)) { Ok(x) => x, Err(g) => return Some(Err(g)) };
            let e_ty = x.ty.clone();
            let Some(x) = self.coerce_diam(&x, pj) else {
                return Some(Err(super::galat::tandai(span, format!(
                    "spread: elemen bertipe `{}` tidak bisa mengisi parameter bertipe `{}` — di backend `--emit-llvm` \
                     tipe sebuah tempat tetap (interpreter dan `--vm` menyimpan tipe bersama nilainya)",
                    e_ty.nama_typeof(), pj.nama_typeof()))));
            };
            akhir.push(x);
        }
        Some(self.panggil_val(&f, &akhir))
    }

    /// `coerce` yang tidak melapor: pasangan tipe yang mustahil dijawab `None` (galatnya dibuang), karena situs dinamis
    /// mencoba tiap argumen ke tiap parameter dan cuma satu kombinasi yang terpakai saat jalan.
    fn coerce_diam(&mut self, v: &Val, ke: &JrgType) -> Option<Val> {
        let n = self.errors.len();
        let r = self.coerce(v, ke);
        if self.errors.len() > n { self.errors.truncate(n); return None; }
        (r.ty == *ke).then_some(r)
    }

    /// Gagal dengan kalimat UTUH yang baru dibaca saat jalan (bukan masukan pemakai, jadi tidak dikutip/dipotong).
    fn panik_teks(&mut self, m: &str) {
        let (p, l) = self.split_str(&Val { text: m.to_string(), ty: JrgType::Str });
        self.ensure_extern("declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", p, l));
        self.terminate("unreachable");
    }

    /// Teks konstan sebagai ekspresi `%JrgStr` untuk penginisialisasi global (tanpa instruksi).
    fn teks_konstan(&mut self, s: &str) -> String {
        let s: &str = &crate::ruang_tipe::bersih(s);
        let nama = match self.string_dedup.get(s) {
            Some(n) => n.clone(),
            None => {
                let n = format!("@.str.{}", self.string_globals.len());
                self.string_globals.push((n.clone(), s.as_bytes().to_vec()));
                self.string_dedup.insert(s.to_string(), n.clone());
                n
            }
        };
        format!("%JrgStr {{ ptr {}, i64 {} }}", nama, s.len())
    }

    /// Dirakit sesudah antrean kerja habis: `@jrg_desk_id` dan satu tabel per situs `f(a: 1)` dinamis.
    pub(crate) fn rakit_desk(&mut self) {
        if !self.pakai_desk { return; }
        let mut id = String::from("define internal i32 @jrg_desk_id(ptr %k, ptr %e) {\nentry:\n  br label %c0\n");
        let mut n = 0usize;
        for (i, d) in self.desk_fn.iter().enumerate() {
            id.push_str(&format!("c{n}:\n  %s{n} = icmp eq ptr %k, @{p}\n  br i1 %s{n}, label %r{n}, label %c{m}\nr{n}:\n  ret i32 {i}\n",
                n = n, m = n + 1, i = i, p = super::mangle::llvm_quoted_ident(&d.kode)));
            n += 1;
        }
        // Adaptor membungkus fungsi aslinya di lingkungannya: yang ditanyakan fungsi asli itu.
        let mut adaptor: Vec<&String> = self.adaptor_ada.values().collect();
        adaptor.sort();
        for a in adaptor {
            id.push_str(&format!(
                "c{n}:\n  %s{n} = icmp eq ptr %k, @{p}\n  br i1 %s{n}, label %r{n}, label %c{m}\nr{n}:\n  %f{n} = load %JrgFn, ptr %e\n  \
                 %k{n} = extractvalue %JrgFn %f{n}, 0\n  %e{n} = extractvalue %JrgFn %f{n}, 1\n  \
                 %i{n} = call i32 @jrg_desk_id(ptr %k{n}, ptr %e{n})\n  ret i32 %i{n}\n",
                n = n, m = n + 1, p = super::mangle::llvm_quoted_ident(a)));
            n += 1;
        }
        id.push_str(&format!("c{}:\n  ret i32 -1\n}}\n", n));
        let mut keluar = id;
        let desk = self.desk_fn.clone();
        for (s, situs) in self.situs_bernama.clone().iter().enumerate() {
            let k = situs.aritas;
            let ent = format!("{{ i32, [{} x i32], %JrgStr }}", k);
            let mut baris = Vec::new();
            for d in &desk {
                let (jenis, perm, pesan) = self.baris_bernama(d, situs);
                let perm: Vec<String> = perm.iter().map(|p| format!("i32 {}", p)).collect();
                let pesan = self.teks_konstan(&pesan);
                baris.push(format!("{} {{ i32 {}, [{} x i32] [{}], {} }}", ent, jenis, k, perm.join(", "), pesan));
            }
            keluar.push_str(&format!("@bern.{} = private constant [{} x {}] [{}]\n", s, desk.len(), ent, baris.join(", ")));
        }
        for (s, situs) in self.situs_sebar.clone().iter().enumerate() {
            let k = situs.aritas;
            let ent = format!("{{ [{} x %JrgStr], %JrgStr, %JrgStr }}", k);
            let mut baris = Vec::new();
            for d in &desk {
                let (kurang, awal, akhir) = self.baris_sebar(d, k);
                let kurang: Vec<String> = kurang.iter().map(|t| self.teks_konstan(t)).collect();
                let (awal, akhir) = (self.teks_konstan(&awal), self.teks_konstan(&akhir));
                baris.push(format!("{} {{ [{} x %JrgStr] [{}], {}, {} }}", ent, k, kurang.join(", "), awal, akhir));
            }
            keluar.push_str(&format!("@sebar.{} = private constant [{} x {}] [{}]\n", s, desk.len(), ent, baris.join(", ")));
        }
        self.tambah_definisi(keluar);
    }

    /// Kalimat `f(...v)` per jumlah argumen kurang dari `k`, dan kalimat "kebanyakan" terbelah di jumlahnya.
    fn baris_sebar(&self, d: &DeskFn, k: usize) -> (Vec<String>, String, String) {
        let pn: Vec<String> = d.params.iter().enumerate().map(|(i, p)| nama_param(p, i)).collect();
        if d.params.len() != k || d.params.iter().any(|p| p.is_variadic) {
            let celah = "spread ke fungsi berparameter pengumpul (`sisa...`) lewat nilai fungsi belum didukung backend \
                         LLVM; interpreter dan `--vm` mendukungnya".to_string();
            return (vec![celah.clone(); k], celah, String::new());
        }
        let wajib = d.params.iter().take_while(|p| p.default.is_none()).count();
        let kurang = (0..k).map(|n| if n < wajib {
            crate::interpreter::pesan_parameter_wajib(&d.disebut, &pn[n], n + 1)
        } else {
            format!("fungsi `{}`: nilai default parameter `{}` lewat nilai fungsi belum didukung backend LLVM; \
                     interpreter dan `--vm` mendukungnya", d.disebut, pn[n])
        }).collect();
        const TANDA: usize = 3_987_654_321;
        let kalimat = crate::interpreter::pesan_fungsi_kebanyakan(&d.disebut, k, TANDA, &pn);
        let (awal, akhir) = kalimat.split_once(&TANDA.to_string()).unwrap_or((&kalimat, ""));
        (kurang, awal.to_string(), akhir.to_string())
    }

    /// Satu baris tabel: (0 = urutan sah, 1 = galat acuan, 2 = celah LLVM; urutan per parameter; kalimat).
    fn baris_bernama(&self, d: &DeskFn, s: &SitusBernama) -> (i32, Vec<i32>, String) {
        let kosong = vec![-1; s.aritas];
        if d.params.len() != s.aritas || d.params.iter().any(|p| p.is_variadic) {
            return (2, kosong, "argumen bernama ke fungsi berparameter pengumpul (`sisa...`) lewat nilai fungsi \
                                belum didukung backend LLVM; interpreter dan `--vm` mendukungnya".to_string());
        }
        let pn: Vec<String> = d.params.iter().enumerate().map(|(i, p)| nama_param(p, i)).collect();
        let (posisi, panjang) = match crate::interpreter::susun_argumen_bernama(&pn, s.posisional, &s.nama) {
            Ok(x) => x,
            Err(e) => return (1, kosong, e),
        };
        let wajib = d.params.iter().take_while(|p| p.default.is_none()).count();
        if panjang < wajib {
            return (1, kosong, crate::interpreter::pesan_parameter_wajib(&d.disebut, &pn[panjang], panjang + 1));
        }
        if panjang < s.aritas {
            return (2, kosong, format!("fungsi `{}`: nilai default parameter `{}` lewat nilai fungsi belum \
                                        didukung backend LLVM; interpreter dan `--vm` mendukungnya", d.disebut, pn[panjang]));
        }
        let mut perm = kosong;
        for (j, p) in perm.iter_mut().enumerate().take(s.posisional) { *p = j as i32; }
        for (m, idx) in posisi.into_iter().enumerate() { perm[idx] = (s.posisional + m) as i32; }
        (0, perm, String::new())
    }
}
