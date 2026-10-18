// src/vm/panggil.rs — pemanggilan fungsi, closure, dan method

use std::sync::Arc;

use super::value::{VmValue, VmFunction};
use super::vm::*;

impl Vm {
    // ── Function call helpers ─────────────────────────────────────────────

    /// `langsung`: dipanggil instruksi `Call` (situs panggilan yang ditulis pemakai),
    /// bukan `defer` atau panggilan balik HOF/`terap`.
    pub(crate) fn call_value_at(&mut self, argc: usize, sp: Option<crate::lexer::Span>, langsung: bool) -> VmResult<()> {
        let fn_idx = self.stack.len().saturating_sub(argc + 1);
        let callee = self.stack[fn_idx].clone();
        self.call_value(callee, argc, fn_idx, sp, langsung)
    }

    /// Nama callee yang DITULIS di instruksi `Call` yang sedang jalan — kompilator
    /// mencatatnya hanya untuk panggilan yang pasti salah jumlah argumennya.
    fn nama_ditulis(&self, f: super::value::VmNativeFn, langsung: bool) -> Option<String> {
        if !langsung { return None; }
        let fr = self.frames.last()?;
        let n = fr.func.chunk.nama_panggil_di(fr.ip.checked_sub(1)?)?;
        (super::info_native::alamat_nama(n) == Some(f as usize)).then(|| n.to_string())
    }

    /// Pengecekan jumlah argumen, dengan pesan yang menyebut penyebabnya —
    /// bukan sekadar "butuh N dapat M", karena dengan nilai default jumlah
    /// yang sah adalah sebuah RENTANG.
    pub(crate) fn periksa_arity(&self, func: &VmFunction, argc: usize, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        // Parameter pengumpul menerima berapa pun sisanya.
        if !func.variadic && argc > func.arity as usize {
            return Err(VmError::at(crate::interpreter::pesan_fungsi_kebanyakan(
                self.nama_tampil(&func.name), func.arity as usize, argc, &func.params), sp));
        }
        if argc < func.arity_min as usize {
            // Parameter ber-default tidak pernah mendahului yang wajib (parser menolaknya),
            // jadi parameter ke-`argc` memang yang wajib dan tidak terisi.
            let nama = func.params.get(argc).cloned().unwrap_or_default();
            return Err(VmError::at(
                crate::interpreter::pesan_parameter_wajib(self.nama_tampil(&func.name), &nama, argc + 1), sp));
        }
        Ok(())
    }

    /// Kemas argumen sisa jadi satu array untuk parameter TERAKHIR.
    /// (Prolog default membaca [`diberi_tetap`], bukan hasil pengemasan ini.)
    pub(crate) fn kemas_variadic(&mut self, func: &VmFunction, argc: usize) -> usize {
        if !func.variadic || func.arity == 0 { return argc; }
        let tetap = func.arity as usize - 1;
        let ambil = argc.saturating_sub(tetap);
        let mut sisa = Vec::with_capacity(ambil);
        for _ in 0..ambil { sisa.push(self.stack.pop().unwrap_or(VmValue::Nil)); }
        sisa.reverse();
        // Slot parameter tetap yang tidak terisi didorong dulu, supaya array
        // pengumpulnya mendarat di slot yang benar.
        for _ in argc..tetap { self.push(VmValue::Nil); }
        self.push(VmValue::array(sisa));
        tetap + 1
    }

    /// Sebuah method dipanggil lewat bentuk PATH (`P::tambah(p, 4)`) sampai ke
    /// sini sebagai fungsi biasa, jadi tidak ada yang menjamin argumen pertamanya …
    pub(crate) fn periksa_penerima(
        &self, func: &VmFunction, argc: usize, fn_idx: usize,
        sp: Option<crate::lexer::Span>,
    ) -> VmResult<()> {
        let Some(pemilik) = func.chunk.tipe_self.as_deref() else { return Ok(()) };
        if func.params.first().map(|s| s.as_str()) != Some("self") { return Ok(()) }
        if argc == 0 { return Ok(()) }
        let penerima = &self.stack[fn_idx + 1];
        // Dict tidak berlabel tipe; `impl dict` milik pemakai menerimanya sebagai `dict`.
        let tipe = match penerima {
            VmValue::Map(m) => m.nama_tipe().unwrap_or("dict").to_string(),
            _ => String::new(),
        };
        if !tipe.is_empty() && crate::vm::visibilitas::menurun_dari(&tipe, pemilik) {
            return Ok(());
        }
        Err(VmError::at(
            crate::interpreter::pesan_penerima_salah(
                &func.name, pemilik, &penerima.type_name_penuh()),
            sp,
        ))
    }

    /// Tolak rekursi yang tidak berujung SEBELUM stack habis.
    pub(crate) fn periksa_kedalaman(&self, nama: &str, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        const BATAS: usize = crate::interpreter::Interpreter::MAX_CALL_DEPTH;
        // Dua bingkai dasar (skrip dan `main`) tidak dihitung acuan sebagai panggilan bersarang.
        if self.frames.len() > BATAS + 1 {
            return Err(VmError::at(
                crate::interpreter::pesan_rekursi_dalam(nama, BATAS),
                sp,
            ));
        }
        Ok(())
    }

    pub(crate) fn call_value(&mut self, callee: VmValue, argc: usize, fn_idx: usize, sp: Option<crate::lexer::Span>, langsung: bool) -> VmResult<()> {
        match callee {
            VmValue::Function(func) => {
                self.periksa_arity(&func, argc, sp)?;
                self.periksa_penerima(&func, argc, fn_idx, sp)?;
                self.periksa_kedalaman(&func.name, sp)?;
                if self.profiling { crate::profiler::enter(&func.name); }
                let diberi = diberi_tetap(&func, argc);
                let argc = self.kemas_variadic(&func, argc);
                // Parameter bernilai default yang tidak diisi tetap harus
                // punya slot di stack; prolog fungsinya yang mengisinya.
                for _ in argc..func.arity as usize { self.push(VmValue::Nil); }
                self.frames.push(CallFrame { func, ip: 0, slot: fn_idx, defers: Vec::new(), argc: diberi, closure: None });
                Ok(())
            }
            // Sama seperti Function, cuma frame-nya ikut membawa closure-nya
            // supaya `LoadUpval`/`StoreUpval` punya sel yang mau dibaca.
            VmValue::Closure(c) => {
                self.periksa_arity(&c.func, argc, sp)?;
                self.periksa_kedalaman(&c.func.name, sp)?;
                if self.profiling { crate::profiler::enter(&c.func.name); }
                let diberi = diberi_tetap(&c.func, argc);
                let argc = self.kemas_variadic(&c.func, argc);
                for _ in argc..c.func.arity as usize { self.push(VmValue::Nil); }
                let func = Arc::clone(&c.func);
                self.frames.push(CallFrame { func, ip: 0, slot: fn_idx, defers: Vec::new(), argc: diberi, closure: Some(c) });
                Ok(())
            }
            VmValue::NativeFn(f) => {
                // SATU pencarian per panggilan: batas argumen, layanan khusus, cetak.
                let info = super::info_native::info(f);
                if let Some(i) = info {
                    let (lebih, kurang) = (i.maks.filter(|m| argc > *m), i.min.filter(|m| argc < *m));
                    if lebih.is_some() || kurang.is_some() {
                        use crate::interpreter::arity as ar;
                        let nama = self.nama_ditulis(f, langsung).unwrap_or_else(|| i.kanonik.clone());
                        let pesan = match lebih {
                            Some(m) => ar::pesan_kebanyakan(&nama, m, argc),
                            None => ar::pesan_kurang(&nama, kurang.unwrap_or(0), i.maks, argc,
                                                     ar::tanda_tangan(&nama).or(i.tanda)),
                        };
                        return Err(VmError::at(pesan, sp));
                    }
                }
                let start = fn_idx + 1;
                let mut args: Vec<VmValue> = self.stack[start..].to_vec();
                self.stack.truncate(fn_idx);
                // Builtin cetak menerima argumennya SESUDAH hook tampilan dijalankan — `def`
                // memberi penunjuk fungsi biasa yang tidak bisa memanggil balik ke VM, jadi …
                if let Some(hasil) = info.and_then(|i| i.khusus).and_then(|apa| self.coba_native_pemanggil(apa, &args)) {
                    let v = hasil.map_err(|e| if e.span.is_none() { VmError::at(e.msg, sp) } else { e })?;
                    self.push(v);
                    return Ok(());
                }
                if info.is_some_and(|i| i.cetak) {
                    self.siapkan_argumen_cetak(&mut args)
                        // Error dari hook lahir tanpa span (ia tidak tahu di
                        // mana ia dipanggil); span situs pemanggilanlah yang
                        // benar, dan tanpa ini diagnostiknya kehilangan caret.
                        .map_err(|e| if e.span.is_none() { VmError::at(e.msg, sp) } else { e })?;
                }
                let result = f(&args).map_err(|e| VmError::at(
                    crate::vm::value::perjelas_pesan_native(e, &args), sp))?;
                self.push(result);
                Ok(())
            }
            // Fungsi `extern "C"`. Marshalling-nya dikerjakan lapisan yang
            // sama dengan interpreter (lihat `vm::ffi_jembatan`), jadi tidak
            // ada aturan tipe kedua yang bisa menyimpang diam-diam.
            VmValue::ExternFn(info) => {
                let start = fn_idx + 1;
                let args: Vec<VmValue> = self.stack[start..].to_vec();
                self.stack.truncate(fn_idx);
                let result = super::ffi_jembatan::panggil(&info, &args)
                    .map_err(|e| VmError::at(e, sp))?;
                self.push(result);
                Ok(())
            }
            // Method atas handle asing (`df.head(5)`): penerimanya argumen pertama.
            VmValue::Asing(super::ffi_asing::Asing::Metode(nama)) => {
                let args: Vec<VmValue> = self.stack[fn_idx + 1..].to_vec();
                self.stack.truncate(fn_idx);
                let v = self.metode_asing_vm(&nama, &args)
                    .map_err(|e| if e.span.is_none() { VmError::at(e.msg, sp) } else { e })?;
                self.push(v);
                Ok(())
            }
            // Konstruktor varian sebagai nilai: kalimat aritas milik interpreter, dan
            // payload-nya dibentuk seperti `BuildEnum` (lebih dari satu jadi tuple).
            VmValue::Ktor(k) => {
                if argc != k.arity {
                    return Err(VmError::at(crate::interpreter::pesan_konstruktor_argumen(
                        &k.tipe, &k.varian, k.arity, argc), sp));
                }
                let mut args: Vec<VmValue> = self.stack[fn_idx + 1..].to_vec();
                self.stack.truncate(fn_idx);
                let payload = match args.len() {
                    0 => None,
                    1 => args.pop(),
                    _ => Some(VmValue::tuple(args)),
                };
                self.push(VmValue::Map(Arc::new(super::value::Peta::baru(
                    super::value::Label::Enum { tipe: k.tipe.clone(), varian: k.varian.clone(), payload },
                    std::collections::HashMap::new(),
                ))));
                Ok(())
            }
            other => Err(VmError::at(crate::interpreter::pesan_bukan_fungsi(&other.type_name_penuh()), sp)),
        }
    }
}

/// Jumlah argumen yang benar-benar mengisi parameter TETAP — yang dibaca prolog
/// nilai default. Sesudah pengemasan, `argc` selalu mencakup slot pengumpul, jadi
/// `fn g(a, b = 2, sisa...)` lalu `g(1)` dulu melewati default `b` dan menjawab `nil`.
fn diberi_tetap(func: &VmFunction, argc: usize) -> u8 {
    let n = if func.variadic { argc.min((func.arity as usize).saturating_sub(1)) } else { argc };
    n as u8
}
