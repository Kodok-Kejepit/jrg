// src/llvm/codegen_konversi.rs — `x as T`, `int()`/`uint()`/`float()`/`rune()` dan
// pintu parse di backend LLVM, dengan aturan dan kalimat acuan (`interpreter::konversi`).

use crate::interpreter::konversi::{self as k, GagalTeksBulat, GagalTeksPecahan, Lebar};
use crate::lexer::Span;
use crate::parser::{PrimTy, TypeExpr};

use super::codegen::{Codegen, Val};
use super::codegen_any::{TAG_BOOL, TAG_DICT, TAG_FLOAT, TAG_INT, TAG_NIL, TAG_RUNE,
                         TAG_SLICE, TAG_STR, TAG_UNIT};
use super::codegen_expr::fmt_hexfloat;
use super::types::JrgType;

/// Tempat nilai di dalam kalimat acuan: karakter pakai-pribadi yang tidak pernah
/// muncul di kalimat mana pun, jadi pemisahannya tidak bisa meleset.
const PENANDA: &str = "\u{E000}";

/// Sasaran sebuah konversi eksplisit.
#[derive(Clone)]
pub enum Sasaran {
    /// Nama yang DITULIS, lebarnya menurut acuan, dan tipe penyimpanannya di
    /// sini — `int`/`uint` 64 bit di backend ini, 128 bit di acuan.
    Bulat { nama: &'static str, lebar: Lebar, repr: JrgType },
    Pecahan,
    Rune,
}

impl Sasaran {
    pub fn dari_prim(p: &PrimTy) -> Option<Sasaran> {
        if let Some((nama, lebar)) = k::sasaran_bulat(p) {
            return Some(Sasaran::Bulat { nama, lebar, repr: super::sema::resolve_prim(p) });
        }
        match p {
            PrimTy::F32 | PrimTy::F64 | PrimTy::F128 => Some(Sasaran::Pecahan),
            PrimTy::Rune => Some(Sasaran::Rune),
            _ => None,
        }
    }

    pub fn dari_tipe(ty: &TypeExpr) -> Option<Sasaran> {
        match ty {
            TypeExpr::Primitive(p) => Self::dari_prim(p),
            TypeExpr::Named(n) => crate::parser::prim_dari_nama(&n.name)
                .and_then(|p| Self::dari_prim(&p)),
            _ => None,
        }
    }

    fn nama(&self) -> &'static str {
        match self { Sasaran::Bulat { nama, .. } => nama, Sasaran::Pecahan => "float",
                     Sasaran::Rune => "rune" }
    }

    fn tipe_hasil(&self) -> JrgType {
        match self { Sasaran::Bulat { repr, .. } => repr.clone(), Sasaran::Pecahan => JrgType::F64,
                     Sasaran::Rune => JrgType::Rune }
    }
}

/// Cara nilai aslinya ditulis di kalimat — sama dengan yang ditulis acuan.
#[derive(Clone)]
enum Cetak {
    Bulat(Val),
    Pecahan(Val),
    /// Karakternya sendiri, bukan kodenya.
    Rune(Val),
    /// Teks, sesudah di-trim.
    Teks(Val),
}

/// Jangkauan sebuah tipe bilangan bulat di backend ini. `u128` dipotong di
/// `i128::MAX` (batas nilai JRG); lihat `melebihi` untuk sumber `u128`.
pub(crate) fn jangkauan(t: &JrgType) -> (i128, i128) {
    match t {
        JrgType::I8 => (i8::MIN as i128, i8::MAX as i128),
        JrgType::I16 => (i16::MIN as i128, i16::MAX as i128),
        JrgType::I32 => (i32::MIN as i128, i32::MAX as i128),
        JrgType::I64 => (i64::MIN as i128, i64::MAX as i128),
        JrgType::I128 => (i128::MIN, i128::MAX),
        JrgType::U8 => (0, u8::MAX as i128),
        JrgType::U16 => (0, u16::MAX as i128),
        JrgType::U32 => (0, u32::MAX as i128),
        JrgType::U64 => (0, u64::MAX as i128),
        JrgType::U128 => (0, i128::MAX),
        JrgType::Rune => (0, 0x10FFFF),
        _ => (0, 0),
    }
}

/// Apakah sebuah nilai bertipe `t` bisa lebih besar dari `batas`.
fn melebihi(t: &JrgType, batas: i128) -> bool {
    *t == JrgType::U128 || jangkauan(t).1 > batas
}

/// Konstanta `n` ditulis sebagai bilangan bertanda selebar `t` — bentuk yang
/// selalu diterima pengurai IR, termasuk untuk tipe tak bertanda.
fn konstanta(t: &JrgType, n: i128) -> String {
    let bit = t.int_bits().unwrap_or(128);
    if bit >= 128 { return n.to_string(); }
    let m = (n as u128) & ((1u128 << bit) - 1);
    let s = if m >> (bit - 1) == 1 { m as i128 - (1i128 << bit) } else { m as i128 };
    s.to_string()
}

/// Muat di acuan tapi tidak di penyimpanan backend ini — cuma `int`/`uint`,
/// yang di sini 64 bit sementara di acuan 128 bit.
fn pesan_batas(asli: &str, nama: &str, repr: &JrgType) -> String {
    let (lo, hi) = jangkauan(repr);
    let lebar_128 = if repr.is_signed_int() { "i128" } else { "u128" };
    format!(
        "`{}` tidak muat di `{}` backend LLVM — di sini `{}` disimpan {} bit ({} sampai {}), \
         sementara mesin acuan memakai 128 bit. Kalau angkanya memang sebesar itu, tulis `{}`, \
         atau jalankan tanpa `--emit-llvm`",
        asli, nama, nama, repr.int_bits().unwrap_or(64), lo, hi, lebar_128)
}

/// `2^m` untuk `batas` = `2^m − 1`, sebagai `f64` yang eksak.
fn pangkat_dua_sesudah(batas: i128) -> f64 {
    let n = (batas as u128).wrapping_add(1);
    debug_assert!(n.is_power_of_two(), "batas {} bukan 2^m − 1", batas);
    2f64.powi(n.trailing_zeros() as i32)
}

impl Codegen {
    /// `x as T`, `int(x)`, `uint(x)`, `float(x)`, `rune(x)`: aturan acuan,
    /// dengan kegagalan SAAT JALAN di titik yang sama dengan acuannya.
    pub fn cast_eksplisit(&mut self, v: &Val, s: &Sasaran, span: Span) -> Result<Val, String> {
        let lama = std::mem::replace(&mut self.koersi_longgar, true);
        let r = self.cast_eksplisit_dalam(v, s, span);
        self.koersi_longgar = lama;
        r
    }

    fn cast_eksplisit_dalam(&mut self, v: &Val, s: &Sasaran, span: Span) -> Result<Val, String> {
        let hasil = s.tipe_hasil();
        if v.ty == JrgType::Never {
            return Ok(Val { text: "undef".into(), ty: hasil });
        }
        if v.ty == JrgType::Any {
            return self.per_tag(v, &hasil, span, |cg, isi| cg.cast_eksplisit(isi, s, span),
                                |tipe| k::pesan_tak_bisa_jadi(tipe, s.nama()));
        }
        let v = if v.ty == JrgType::F32 { self.coerce_longgar(v, &JrgType::F64) } else { v.clone() };
        let t = &v.ty;
        Ok(match s {
            Sasaran::Bulat { nama, lebar, repr } => {
                if t.is_int() || *t == JrgType::Rune {
                    let cetak = if *t == JrgType::Rune { Cetak::Rune(v.clone()) }
                                else { Cetak::Bulat(v.clone()) };
                    self.periksa_bulat(&v, nama, *lebar, repr, &cetak, PENANDA, "");
                    self.coerce_longgar(&v, repr)
                } else if *t == JrgType::F64 {
                    self.periksa_pecahan_ke_bulat(&v, nama, *lebar, repr);
                    self.ubah_numerik(&v, repr)
                } else if *t == JrgType::Bool {
                    // 0 atau 1 muat di lebar mana pun.
                    self.ubah_numerik(&v, repr)
                } else if *t == JrgType::Str {
                    self.teks_ke_bulat(&v, nama, *lebar, repr, None)
                } else if *t == JrgType::Nil {
                    self.gagal_pasti(&k::pesan_nil_jadi(nama), &hasil)
                } else {
                    self.gagal_pasti(&k::pesan_tak_bisa_jadi(&t.nama_typeof(), nama), &hasil)
                }
            }
            Sasaran::Pecahan => {
                if t.is_numeric() {
                    self.coerce_longgar(&v, &JrgType::F64)
                } else if *t == JrgType::Bool {
                    let r = self.fresh("kvbf.");
                    self.emit(format!("{} = uitofp i8 {} to double", r, v.text));
                    Val { text: r, ty: JrgType::F64 }
                } else if *t == JrgType::Str {
                    self.teks_ke_pecahan(&v, None)
                } else if *t == JrgType::Nil {
                    self.gagal_pasti(&k::pesan_nil_jadi("float"), &hasil)
                } else {
                    self.gagal_pasti(&k::pesan_tak_bisa_jadi(&t.nama_typeof(), "float"), &hasil)
                }
            }
            Sasaran::Rune => {
                if *t == JrgType::Rune {
                    v.clone()
                } else if t.is_int() {
                    self.bulat_ke_rune(&v);
                    self.coerce_longgar(&v, &JrgType::Rune)
                } else {
                    self.gagal_pasti(&k::pesan_tak_bisa_jadi(&t.nama_typeof(), "rune"), &hasil)
                }
            }
        })
    }

    /// Pintu parse bilangan bulat (`str::to_int`, `parse_int`, `.to_int()`).
    pub fn pintu_parse_bulat(&mut self, v: &Val, pintu: &str, span: Span) -> Result<Val, String> {
        let repr = JrgType::I64;
        if v.ty == JrgType::Never { return Ok(Val { text: "undef".into(), ty: repr }); }
        if v.ty == JrgType::Any {
            return self.per_tag(v, &repr, span, |cg, isi| cg.pintu_parse_bulat(isi, pintu, span),
                                |tipe| k::pesan_parse_butuh_teks(pintu, tipe));
        }
        if v.ty == JrgType::Str {
            return Ok(self.teks_ke_bulat(v, "int", Lebar::I128, &repr, Some(pintu)));
        }
        if v.ty.is_int() {
            let awalan = format!("{}: ", pintu);
            self.periksa_bulat(v, "int", Lebar::I128, &repr, &Cetak::Bulat(v.clone()), PENANDA, &awalan);
            return Ok(self.coerce_longgar(v, &repr));
        }
        let tipe = if v.ty.is_float() { "float".to_string() } else { v.ty.nama_typeof() };
        Ok(self.gagal_pasti(&k::pesan_parse_butuh_teks(pintu, &tipe), &repr))
    }

    /// Pintu parse pecahan (`str::to_float`, `parse_float`, `.to_float()`).
    pub fn pintu_parse_pecahan(&mut self, v: &Val, pintu: &str, span: Span) -> Result<Val, String> {
        let hasil = JrgType::F64;
        if v.ty == JrgType::Never { return Ok(Val { text: "undef".into(), ty: hasil }); }
        if v.ty == JrgType::Any {
            return self.per_tag(v, &hasil, span, |cg, isi| cg.pintu_parse_pecahan(isi, pintu, span),
                                |tipe| k::pesan_parse_butuh_teks(pintu, tipe));
        }
        if v.ty == JrgType::Str { return Ok(self.teks_ke_pecahan(v, Some(pintu))); }
        if v.ty.is_numeric() { return Ok(self.coerce_longgar(v, &hasil)); }
        Ok(self.gagal_pasti(&k::pesan_parse_butuh_teks(pintu, &v.ty.nama_typeof()), &hasil))
    }

    // ── pemeriksaan ─────────────────────────────────────────────────────

    /// Bilangan bulat (atau rune) ke sasaran bilangan bulat. Jalur cepatnya
    /// satu atau dua perbandingan; kalimatnya dipilih di jalur dingin.
    #[allow(clippy::too_many_arguments)]
    fn periksa_bulat(&mut self, v: &Val, nama: &str, lebar: Lebar, repr: &JrgType,
                     cetak: &Cetak, asli: &str, awalan: &str) {
        let (rlo, rhi) = lebar.jangkauan();
        let (plo, phi) = jangkauan(repr);
        let (alo, ahi) = (rlo.max(plo), rhi.min(phi));
        let (slo, _) = jangkauan(&v.ty);
        let bertanda = v.ty.is_signed_int();
        let ty = v.ty.llvm_ty();
        let gt = if bertanda { "sgt" } else { "ugt" };

        let mut buruk = Vec::new();
        if bertanda && slo < alo {
            buruk.push(self.bandingkan("slt", &ty, &v.text, &konstanta(&v.ty, alo)));
        }
        if melebihi(&v.ty, ahi) {
            buruk.push(self.bandingkan(gt, &ty, &v.text, &konstanta(&v.ty, ahi)));
        }
        let Some(b) = self.gabung_atau(&buruk) else { return };

        self.gagal_bila(&b, |cg| {
            if !lebar.bertanda() && bertanda {
                let c = cg.bandingkan("slt", &ty, &v.text, "0");
                cg.gagal_bila(&c, |cg| cg.panik_sisip(
                    &format!("{}{}", awalan, k::pesan_tak_muat(asli, nama, lebar, true)), cetak));
            }
            if melebihi(&v.ty, rhi) {
                let c = cg.bandingkan(gt, &ty, &v.text, &konstanta(&v.ty, rhi));
                cg.gagal_bila(&c, |cg| cg.panik_sisip(
                    &format!("{}{}", awalan, k::pesan_tak_muat(asli, nama, lebar, false)), cetak));
            }
            if lebar.bertanda() && bertanda && slo < rlo {
                let c = cg.bandingkan("slt", &ty, &v.text, &konstanta(&v.ty, rlo));
                cg.gagal_bila(&c, |cg| cg.panik_sisip(
                    &format!("{}{}", awalan, k::pesan_tak_muat(asli, nama, lebar, false)), cetak));
            }
            if plo > rlo || phi < rhi {
                cg.panik_sisip(&format!("{}{}", awalan, pesan_batas(asli, nama, repr)), cetak);
            } else {
                cg.terminate("unreachable");
            }
        });
    }

    /// Pecahan ke sasaran bilangan bulat, dipotong ke arah nol. NaN, tak hingga
    /// dan yang di luar jangkauan gagal — `fptosi` atas nilai itu tidak terdefinisi.
    fn periksa_pecahan_ke_bulat(&mut self, f: &Val, nama: &str, lebar: Lebar, repr: &JrgType) {
        let (rlo, rhi) = lebar.jangkauan();
        let (plo, phi) = jangkauan(repr);
        let (alo, ahi) = (rlo.max(plo), rhi.min(phi));
        // Predikat unordered, jadi NaN ikut ke jalur dingin.
        let bawah = self.pecahan_di_bawah(f, alo, true);
        let atas = self.pecahan_di_atas(f, ahi, true);
        let b = self.gabung_atau(&[bawah, atas]).expect("dua syarat");
        let cetak = Cetak::Pecahan(f.clone());

        self.gagal_bila(&b, |cg| {
            let c = cg.bandingkan_f("uno", &f.text, "0.0");
            cg.gagal_bila(&c, |cg| cg.panik(&k::pesan_nan(nama)));

            let pos = cg.bandingkan_f("oeq", &f.text, &fmt_hexfloat(f64::INFINITY));
            let neg = cg.bandingkan_f("oeq", &f.text, &fmt_hexfloat(f64::NEG_INFINITY));
            let c = cg.gabung_atau(&[pos, neg]).expect("dua syarat");
            cg.gagal_bila(&c, |cg| cg.panik_sisip(&k::pesan_tak_hingga(PENANDA, nama), &cetak));

            let besar = cg.bandingkan_f("oge", &f.text, &fmt_hexfloat(2f64.powi(127)));
            let kecil = cg.bandingkan_f("olt", &f.text, &fmt_hexfloat(-(2f64.powi(127))));
            let c = cg.gabung_atau(&[besar, kecil]).expect("dua syarat");
            cg.gagal_bila(&c, |cg| cg.panik_sisip(&k::pesan_luar_128(PENANDA, nama), &cetak));

            if !lebar.bertanda() {
                let c = cg.pecahan_di_bawah(f, 0, false);
                cg.gagal_bila(&c, |cg| cg.panik_sisip(
                    &k::pesan_tak_muat(PENANDA, nama, lebar, true), &cetak));
            }
            if rhi < i128::MAX {
                let c = cg.pecahan_di_atas(f, rhi, false);
                cg.gagal_bila(&c, |cg| cg.panik_sisip(
                    &k::pesan_tak_muat(PENANDA, nama, lebar, false), &cetak));
            }
            if lebar.bertanda() && rlo > i128::MIN {
                let c = cg.pecahan_di_bawah(f, rlo, false);
                cg.gagal_bila(&c, |cg| cg.panik_sisip(
                    &k::pesan_tak_muat(PENANDA, nama, lebar, false), &cetak));
            }
            if plo > rlo || phi < rhi {
                cg.panik_sisip(&pesan_batas(PENANDA, nama, repr), &cetak);
            } else {
                cg.terminate("unreachable");
            }
        });
    }

    /// i1: potongan `f` lebih besar dari `batas` (= 2^m − 1), yaitu `f >= 2^m`.
    fn pecahan_di_atas(&mut self, f: &Val, batas: i128, atau_nan: bool) -> String {
        let ambang = pangkat_dua_sesudah(batas);
        self.bandingkan_f(if atau_nan { "uge" } else { "oge" }, &f.text, &fmt_hexfloat(ambang))
    }

    /// i1: potongan `f` lebih kecil dari `batas` (0 atau −2^k). Di atas 2^52
    /// tidak ada `f64` di antara −2^k − 1 dan −2^k, jadi pembandingnya bergeser.
    fn pecahan_di_bawah(&mut self, f: &Val, batas: i128, atau_nan: bool) -> String {
        let (ambang, pred) = if batas == 0 {
            (-1.0, if atau_nan { "ule" } else { "ole" })
        } else {
            let kk = (batas as u128).wrapping_neg().trailing_zeros() as i32;
            if kk <= 52 { (-(2f64.powi(kk) + 1.0), if atau_nan { "ule" } else { "ole" }) }
            else { (-(2f64.powi(kk)), if atau_nan { "ult" } else { "olt" }) }
        };
        self.bandingkan_f(pred, &f.text, &fmt_hexfloat(ambang))
    }

    /// Kode karakter Unicode yang sah: 0..=0x10FFFF, di luar separuh surrogate.
    fn bulat_ke_rune(&mut self, v: &Val) {
        let ty = v.ty.llvm_ty();
        let bertanda = v.ty.is_signed_int();
        let mut buruk = Vec::new();
        if bertanda {
            buruk.push(self.bandingkan("slt", &ty, &v.text, "0"));
        }
        if melebihi(&v.ty, 0x10FFFF) {
            let gt = if bertanda { "sgt" } else { "ugt" };
            buruk.push(self.bandingkan(gt, &ty, &v.text, &konstanta(&v.ty, 0x10FFFF)));
        }
        if melebihi(&v.ty, 0xD7FF) {
            let d = self.fresh("kvsur.");
            self.emit(format!("{} = sub {} {}, {}", d, ty, v.text, konstanta(&v.ty, 0xD800)));
            buruk.push(self.bandingkan("ult", &ty, &d, &konstanta(&v.ty, 0x800)));
        }
        let Some(b) = self.gabung_atau(&buruk) else { return };
        let cetak = Cetak::Bulat(v.clone());
        self.gagal_bila(&b, |cg| cg.panik_sisip(&k::pesan_bukan_kode_rune(PENANDA), &cetak));
    }

    /// Teks ke bilangan bulat: diurai 128 bit seperti acuannya, lalu diperiksa
    /// lebarnya. `pintu` = `Some` untuk pintu parse, yang kalimatnya lain.
    fn teks_ke_bulat(&mut self, v: &Val, nama: &str, lebar: Lebar, repr: &JrgType,
                     pintu: Option<&str>) -> Val {
        self.ensure_extern("declare i32 @jrg_urai_bulat(ptr, i64, ptr, ptr)", "jrg_urai_bulat");
        let (p, l) = self.split_str(v);
        let olo = self.alloca_raw("kvlo.", "i64", Some(8));
        let ohi = self.alloca_raw("kvhi.", "i64", Some(8));
        let kode = self.fresh("kvkode.");
        self.emit(format!("{} = call i32 @jrg_urai_bulat(ptr {}, i64 {}, ptr {}, ptr {})",
            kode, p, l, olo, ohi));
        let b = self.bandingkan("ne", "i32", &kode, "0");
        let teks = Cetak::Teks(v.clone());
        let pesan = |g: GagalTeksBulat| match pintu {
            None => k::pesan_teks_jadi_bulat(g, PENANDA, nama),
            Some(pt) => k::pesan_parse_teks_bulat(pt, g, PENANDA),
        };
        self.gagal_bila(&b, |cg| {
            // Urutan kodenya urutan `GagalTeksBulat`, lihat `jrg_urai_bulat`.
            for (nomor, g) in [(1, GagalTeksBulat::Kosong), (2, GagalTeksBulat::Luar128),
                               (4, GagalTeksBulat::Pecahan)] {
                let c = cg.bandingkan("eq", "i32", &kode, &nomor.to_string());
                cg.gagal_bila(&c, |cg| cg.panik_sisip(&pesan(g), &teks));
            }
            cg.panik_sisip(&pesan(GagalTeksBulat::BukanBulat), &teks);
        });

        let lo = self.fresh("kvlov.");
        self.emit(format!("{} = load i64, ptr {}", lo, olo));
        let hi = self.fresh("kvhiv.");
        self.emit(format!("{} = load i64, ptr {}", hi, ohi));
        let lo128 = self.fresh("kvlo128.");
        self.emit(format!("{} = zext i64 {} to i128", lo128, lo));
        let hi128 = self.fresh("kvhi128.");
        self.emit(format!("{} = zext i64 {} to i128", hi128, hi));
        let geser = self.fresh("kvshl.");
        self.emit(format!("{} = shl i128 {}, 64", geser, hi128));
        let n = self.fresh("kvn.");
        self.emit(format!("{} = or i128 {}, {}", n, geser, lo128));
        let n = Val { text: n, ty: JrgType::I128 };

        let (asli, awalan) = match pintu {
            None => (format!("\"{}\"", PENANDA), String::new()),
            Some(pt) => (format!("\"{}\"", PENANDA), format!("{}: ", pt)),
        };
        self.periksa_bulat(&n, nama, lebar, repr, &teks, &asli, &awalan);
        self.coerce_longgar(&n, repr)
    }

    fn teks_ke_pecahan(&mut self, v: &Val, pintu: Option<&str>) -> Val {
        self.ensure_extern("declare i32 @jrg_urai_pecahan(ptr, i64, ptr)", "jrg_urai_pecahan");
        let (p, l) = self.split_str(v);
        let out = self.alloca_raw("kvf.", "double", Some(8));
        let kode = self.fresh("kvkode.");
        self.emit(format!("{} = call i32 @jrg_urai_pecahan(ptr {}, i64 {}, ptr {})",
            kode, p, l, out));
        let b = self.bandingkan("ne", "i32", &kode, "0");
        let teks = Cetak::Teks(v.clone());
        let pesan = |g: GagalTeksPecahan| match pintu {
            None => k::pesan_teks_jadi_pecahan(g, PENANDA),
            Some(pt) => k::pesan_parse_teks_pecahan(pt, g, PENANDA),
        };
        self.gagal_bila(&b, |cg| {
            let c = cg.bandingkan("eq", "i32", &kode, "1");
            cg.gagal_bila(&c, |cg| cg.panik_sisip(&pesan(GagalTeksPecahan::Kosong), &teks));
            cg.panik_sisip(&pesan(GagalTeksPecahan::BukanAngka), &teks);
        });
        let r = self.fresh("kvfv.");
        self.emit(format!("{} = load double, ptr {}", r, out));
        Val { text: r, ty: JrgType::F64 }
    }

    // ── nilai bertag ────────────────────────────────────────────────────

    /// Nilai yang tipenya baru diketahui saat jalan: dibuka menurut tagnya,
    /// lalu tiap golongan lewat aturan yang sama dengan nilai biasa.
    fn per_tag(&mut self, v: &Val, hasil: &JrgType, span: Span,
               mut ubah: impl FnMut(&mut Self, &Val) -> Result<Val, String>,
               tak_bisa: impl Fn(&str) -> String) -> Result<Val, String> {
        let tag = self.tag_any(v);
        let slot = self.alloca_typed("kvany.", hasil);
        let gab = self.fresh_label("kvany.gab.");
        let golongan: [(i32, Option<JrgType>, &str); 9] = [
            (TAG_NIL, Some(JrgType::Nil), "nil"), (TAG_BOOL, Some(JrgType::Bool), "bool"),
            (TAG_INT, Some(JrgType::I64), "int"), (TAG_FLOAT, Some(JrgType::F64), "float"),
            (TAG_RUNE, Some(JrgType::Rune), "rune"), (TAG_STR, Some(JrgType::Str), "string"),
            (TAG_UNIT, None, "unit"), (TAG_SLICE, None, "array"), (TAG_DICT, None, "dict"),
        ];
        let label: Vec<String> = golongan.iter()
            .map(|(t, _, _)| self.fresh_label(&format!("kvany.{}.", t))).collect();
        let lain = self.fresh_label("kvany.lain.");
        let kasus: Vec<String> = golongan.iter().zip(&label)
            .map(|((t, _, _), l)| format!("i32 {}, label %{}", t, l)).collect();
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag.text, lain, kasus.join(" ")));

        for ((_, ty, nama), l) in golongan.iter().zip(&label) {
            self.start_block(l.clone());
            match ty {
                Some(ty) => {
                    let isi = self.buka_any(v, ty, span)?;
                    let r = ubah(self, &isi)?;
                    self.emit(format!("store {} {}, ptr {}", hasil.llvm_ty(), r.text, slot));
                    self.terminate(format!("br label %{}", gab));
                }
                None => self.panik(&tak_bisa(nama)),
            }
        }
        // Tag di luar daftar cuma mungkin kalau kotaknya dipasang jalur yang
        // belum ada di sini — bug di jrgc, bukan di program pemakainya.
        self.start_block(lain);
        self.panik("tag nilai tidak dikenal — ini bug di jrgc, bukan di programmu");

        self.start_block(gab);
        Ok(self.load_place(&slot, hasil))
    }

    // ── perakitan ───────────────────────────────────────────────────────

    fn bandingkan(&mut self, pred: &str, ty: &str, a: &str, b: &str) -> String {
        let r = self.fresh("kvc.");
        self.emit(format!("{} = icmp {} {} {}, {}", r, pred, ty, a, b));
        r
    }

    fn bandingkan_f(&mut self, pred: &str, a: &str, b: &str) -> String {
        let r = self.fresh("kvfc.");
        self.emit(format!("{} = fcmp {} double {}, {}", r, pred, a, b));
        r
    }

    fn gabung_atau(&mut self, syarat: &[String]) -> Option<String> {
        let mut it = syarat.iter();
        let mut acc = it.next()?.clone();
        for s in it {
            let r = self.fresh("kvor.");
            self.emit(format!("{} = or i1 {}, {}", r, acc, s));
            acc = r;
        }
        Some(acc)
    }

    /// Kode keluar `exit`: di luar 0–255 gagal saat jalan dengan kalimat acuannya,
    /// selain itu dijadikan `i32`.
    pub(super) fn kode_keluar_terperiksa(&mut self, v: &Val) -> Val {
        let ty = v.ty.llvm_ty();
        let mut buruk = Vec::new();
        if v.ty.is_signed_int() {
            buruk.push(self.bandingkan("slt", &ty, &v.text, "0"));
        }
        if melebihi(&v.ty, 255) {
            let gt = if v.ty.is_signed_int() { "sgt" } else { "ugt" };
            buruk.push(self.bandingkan(gt, &ty, &v.text, &konstanta(&v.ty, 255)));
        }
        if let Some(b) = self.gabung_atau(&buruk) {
            let cetak = Cetak::Bulat(v.clone());
            self.gagal_bila(&b, |cg| cg.panik_sisip(
                &crate::host::pesan_kode_keluar_luar(PENANDA), &cetak));
        }
        self.coerce_longgar(v, &JrgType::I32)
    }

    /// Kalau `syarat` benar, jalur `gagal` — yang harus berakhir gagal — diambil;
    /// kalau tidak, pemancaran berlanjut di blok baru.
    fn gagal_bila(&mut self, syarat: &str, gagal: impl FnOnce(&mut Self)) {
        let l_gagal = self.fresh_label("kvgagal.");
        let l_lanjut = self.fresh_label("kvlanjut.");
        self.terminate_unlikely_true(syarat, &l_gagal, &l_lanjut);
        self.start_block(l_gagal);
        gagal(self);
        self.start_block(l_lanjut);
    }

    /// Nilai bertag yang menuju tempat bertipe `tujuan`: tagnya diperiksa saat jalan, dan
    /// yang tidak cocok gagal KERAS dengan tipe aslinya — tidak pernah dibaca sebagai tipe lain.
    pub(crate) fn buka_terperiksa(&mut self, v: &Val, tujuan: &JrgType) -> Option<Val> {
        let tag = super::codegen_any::tag_untuk(tujuan)?;
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let t = self.tag_any(v);
        let c = self.fresh("bkt.");
        self.emit(format!("{} = icmp ne i32 {}, {}", c, t.text, tag));
        let pesan = format!(
            "nilai bertipe `{}` tidak bisa mengisi tempat yang bertipe `{}` — di backend `--emit-llvm` \
             tipe sebuah tempat tetap (interpreter dan `--vm` menyimpan tipe bersama nilainya)",
            PENANDA, tujuan.nama_typeof());
        // Nama tipenya dirakit di jalur gagal saja: jalur panas cuma satu pembandingan tag.
        let kotak = v.clone();
        self.gagal_bila(&c, |cg| match cg.typeof_any(&kotak) {
            Ok(nama) => cg.panik_sisip(&pesan, &Cetak::Teks(nama)),
            Err(_) => cg.panik(&pesan.replace(PENANDA, "lain")),
        });
        self.buka_any(v, tujuan, nol).ok()
    }

    /// Tolak saat jalan kalau `v` negatif, dengan kalimat acuan untuk sebuah ukuran.
    pub(crate) fn tolak_negatif(&mut self, nama: &str, apa: &str, v: &Val) {
        let contoh = crate::interpreter::pesan_ukuran_tak_masuk_akal(nama, apa, -1);
        let pesan = format!("{}{}", contoh.strip_suffix("-1").unwrap_or(&contoh), PENANDA);
        let c = self.fresh("neg.");
        self.emit(format!("{} = icmp slt i64 {}, 0", c, v.text));
        let cetak = Cetak::Bulat(v.clone());
        self.gagal_bila(&c, |cg| cg.panik_sisip(&pesan, &cetak));
    }

    /// Gagal dengan kalimat buatan `pesan(tipe)`, tipenya dibaca dari tag `v` saat jalan.
    pub(crate) fn panik_tipe_any(&mut self, v: &Val, pesan: impl Fn(&str) -> String) {
        let pola = pesan(PENANDA);
        match self.typeof_any(v) {
            Ok(nama) => self.panik_sisip(&pola, &Cetak::Teks(nama)),
            Err(_) => self.panik(&pesan("nilai")),
        }
    }

    /// Gagal dengan `pesan`, nilainya disisipkan di tempat penanda. Kalimat
    /// tanpa penanda (mis. teks kosong) dicetak apa adanya.
    fn panik_sisip(&mut self, pesan: &str, cetak: &Cetak) {
        let Some((pre, suf)) = pesan.split_once(PENANDA) else {
            self.panik(pesan);
            return;
        };
        let pv = self.intern_string(pre);
        let (pp, pl) = self.split_str(&pv);
        let sv = self.intern_string(suf);
        let (sp, sl) = self.split_str(&sv);
        match cetak {
            Cetak::Bulat(v) => {
                let w = if v.ty.int_bits() == Some(128) { v.clone() } else {
                    let r = self.fresh("kvw.");
                    let op = if v.ty.is_signed_int() { "sext" } else { "zext" };
                    self.emit(format!("{} = {} {} {} to i128", r, op, v.ty.llvm_ty(), v.text));
                    Val { text: r, ty: JrgType::I128 }
                };
                let (lo, hi) = self.split_i128(&w);
                let tb = if v.ty.is_unsigned_int() { 1 } else { 0 };
                self.ensure_extern(
                    "declare void @jrg_panic_sisip_bulat(ptr, i64, i64, i64, i32, ptr, i64) cold noreturn",
                    "jrg_panic_sisip_bulat");
                self.emit(format!(
                    "call void @jrg_panic_sisip_bulat(ptr {}, i64 {}, i64 {}, i64 {}, i32 {}, ptr {}, i64 {})",
                    pp, pl, lo, hi, tb, sp, sl));
            }
            Cetak::Pecahan(v) => {
                self.ensure_extern(
                    "declare void @jrg_panic_sisip_f64(ptr, i64, double, ptr, i64) cold noreturn",
                    "jrg_panic_sisip_f64");
                self.emit(format!(
                    "call void @jrg_panic_sisip_f64(ptr {}, i64 {}, double {}, ptr {}, i64 {})",
                    pp, pl, v.text, sp, sl));
            }
            Cetak::Rune(v) => {
                self.ensure_extern(
                    "declare void @jrg_panic_sisip_rune(ptr, i64, i32, ptr, i64) cold noreturn",
                    "jrg_panic_sisip_rune");
                self.emit(format!(
                    "call void @jrg_panic_sisip_rune(ptr {}, i64 {}, i32 {}, ptr {}, i64 {})",
                    pp, pl, v.text, sp, sl));
            }
            Cetak::Teks(v) => {
                let (tp, tl) = self.split_str(v);
                self.ensure_extern(
                    "declare void @jrg_panic_sisip_teks(ptr, i64, ptr, i64, ptr, i64) cold noreturn",
                    "jrg_panic_sisip_teks");
                self.emit(format!(
                    "call void @jrg_panic_sisip_teks(ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, i64 {})",
                    pp, pl, tp, tl, sp, sl));
            }
        }
        self.terminate("unreachable");
    }

    /// Konversi yang PASTI gagal tetap gagal saat jalan, di titik yang sama
    /// dengan acuannya — keluaran sebelumnya tetap tercetak.
    fn gagal_pasti(&mut self, pesan: &str, hasil: &JrgType) -> Val {
        self.panik(pesan);
        let l = self.fresh_label("kvmati.");
        self.start_block(l);
        Val { text: "undef".into(), ty: hasil.clone() }
    }
}
