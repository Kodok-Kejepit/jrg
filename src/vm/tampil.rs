// src/vm/tampil.rs — src/vm/tampil.rs — hook tampilan (`to_string`) di sisi VM.

use super::value::VmValue;
use super::vm::{Vm, VmResult};

/// Nama metode hook. Harus sama persis dengan interpreter.
pub const NAMA_HOOK: &str = crate::interpreter::tampil::NAMA_HOOK;

/// Builtin yang keluarannya DIBACA MANUSIA, jadi argumennya lewat hook.
pub const BUILTIN_CETAK: &[&str] = &[
    "wok", "wok_", "eprint", "tok",
    "fmt::wok", "fmt::wok_", "fmt::eprint", "fmt::sprint", "fmt::tok",
    // `x.to_string()`: keluarannya dibaca manusia, jadi ia lewat hook juga —
    // termasuk untuk nilai bersarang.
    "__to_string",
];

impl Vm {
    /// `terap(f, args)` — memanggil `f` dengan isi array sebagai argumennya.
    pub(crate) fn terap_vm(&mut self, args: &[VmValue]) -> VmResult<VmValue> {
        use super::vm::VmError;
        let f = match args.first() {
            Some(v) if v.bisa_dipanggil() => v.clone(),
            Some(lain) => return Err(VmError::new(crate::interpreter::pesan_bukan_fungsi(&lain.type_name_penuh()))),
            None => return Err(VmError::new(
                "terap dipanggil tanpa argumen — ia butuh fungsinya dan array \
                 argumennya: `terap(f, [1, 2])`".to_string())),
        };
        let daftar = match args.get(1) {
            Some(VmValue::Array(a)) => a.lock().unwrap().clone(),
            Some(lain) => return Err(VmError::new(format!(
                "terap: argumen 2 harus array, dapat `{}`", lain.type_name_penuh()))),
            None => Vec::new(),
        };
        self.call_value_immediate(f, daftar)
    }

    /// `terap_bernama(f, posisional, nama, nilai)` — mesin di balik gula `f(a, b:
    /// 2)`.
    pub(crate) fn terap_bernama_vm(&mut self, args: &[VmValue]) -> VmResult<VmValue> {
        use super::vm::VmError;
        let (f, param) = match args.first() {
            Some(VmValue::Function(fu)) => (VmValue::Function(fu.clone()), fu.params.clone()),
            Some(VmValue::Closure(c)) =>
                (VmValue::Closure(c.clone()), c.func.params.clone()),
            Some(v) if v.bisa_dipanggil() => return Err(VmError::new(
                crate::interpreter::pesan_bernama_tanpa_param(&v.type_name_penuh()))),
            Some(lain) => return Err(VmError::new(crate::interpreter::pesan_bukan_fungsi(&lain.type_name_penuh()))),
            None => return Err(VmError::new(
                "terap_bernama dipanggil tanpa argumen".to_string())),
        };
        let galat = |m: String| VmError::new(crate::interpreter::pesan_terap_bernama(&m));
        let ambil = |i: usize| -> VmResult<Vec<VmValue>> {
            match args.get(i) {
                Some(VmValue::Array(a)) => Ok(a.lock().unwrap_or_else(|p| p.into_inner()).clone()),
                None => Ok(Vec::new()),
                Some(lain) => Err(galat(format!("argumen ke-{} harus array, dapat `{}`",
                                                i + 1, lain.type_name_penuh()))),
            }
        };
        let (posisional, nilai) = (ambil(1)?, ambil(3)?);
        let nama: Vec<String> = ambil(2)?.into_iter()
            .map(|v| match v {
                VmValue::Str(s) => Ok(s.to_string()),
                lain => Err(galat(format!("nama parameternya harus string, dapat `{}`", lain.type_name_penuh()))),
            })
            .collect::<VmResult<_>>()?;
        if nama.len() != nilai.len() {
            return Err(galat(format!("{} nama tapi {} nilai", nama.len(), nilai.len())));
        }
        let (posisi, panjang) = crate::interpreter::susun_argumen_bernama(
            &param, posisional.len(), &nama).map_err(VmError::new)?;
        let mut akhir: Vec<VmValue> = vec![VmValue::Nil; panjang];
        for (i, v) in posisional.into_iter().enumerate() { akhir[i] = v; }
        for (k, idx) in posisi.into_iter().enumerate() {
            akhir[idx] = nilai.get(k).cloned().unwrap_or(VmValue::Nil);
        }
        self.call_value_immediate(f, akhir)
    }

    /// Builtin yang perlu MEMANGGIL BALIK fungsi JRG, dilayani VM sendiri.
    pub(crate) fn coba_native_pemanggil(
        &mut self,
        apa: super::vm::NativeKhusus,
        args: &[VmValue],
    ) -> Option<VmResult<VmValue>> {
        use super::vm::NativeKhusus as NK;
        match apa {
            NK::Tugas { task } => return Some(self.mulai_tugas_vm(task, args)),
            NK::Router { metode } => return Some(self.pasang_route_vm(metode, args)),
            NK::Serve => return Some(self.serve_vm(args)),
            NK::Terap => return Some(self.terap_vm(args)),
            NK::TerapBernama => return Some(self.terap_bernama_vm(args)),
            NK::Hof { nama } => return Some(self.hof_lewat_nilai(nama, args)),
            NK::Ffi { nomor } => return Some(self.ffi_vm(nomor, args)),
            NK::NumSolve => {}
        }
        // Argumen PERTAMA adalah fungsinya, dan ia sengaja TIDAK diseberangkan.
        let Some(fungsi_vm) = args.first().cloned() else {
            return Some(Err(super::vm::VmError::new(
                "dok::num_solve butuh minimal (fungsi, x0, x1)".to_string(),
            )));
        };
        // Penanda cuma dipasang kalau argumennya MEMANG fungsi.
        let fungsi_beneran = fungsi_vm.bisa_dipanggil();
        let mut titipan = crate::vm::ffi_jembatan::TitipanVm::default();
        let mut nilai = Vec::with_capacity(args.len());
        if fungsi_beneran {
            nilai.push(crate::interpreter::Value::NativeFn(
                crate::interpreter::builtin::NativeFnKind::FmtWok,
            ));
        } else {
            match titipan.ke_interp(&fungsi_vm) {
                Ok(v) => nilai.push(v),
                Err(e) => return Some(Err(super::vm::VmError::new(e))),
            }
        }
        for a in &args[1..] {
            match titipan.ke_interp(a) {
                Ok(v) => nilai.push(v),
                Err(e) => return Some(Err(super::vm::VmError::new(e))),
            }
        }
        let mut panggil = |_f: &crate::interpreter::Value, a: Vec<crate::interpreter::Value>|
            -> Result<crate::interpreter::Value, String> {
            let mut avm = Vec::with_capacity(a.len());
            for x in &a { avm.push(crate::vm::ffi_jembatan::nilai_ke_vm(x)?); }
            let hasil = self
                .call_value_immediate(fungsi_vm.clone(), avm)
                .map_err(|e| e.msg)?;
            // Hasil yang berupa fungsi menyeberang sebagai penanda, jadi kalimatnya
            // milik `num_solve` ("harus angka"), sama dengan interpreter.
            crate::vm::ffi_jembatan::TitipanVm::default().ke_interp(&hasil)
        };
        // Implementasi yang SAMA dengan interpreter — bukan salinan.
        let hasil = crate::dok::numeric::num_solve_dgn(&mut panggil, &nilai);
        Some(match hasil {
            Ok(v) => titipan.ke_vm(&v).map_err(super::vm::VmError::new),
            Err(e) => Err(super::vm::VmError::new(e)),
        })
    }


    /// Ubah tiap argumen yang punya hook jadi string, di tempat.
    pub(crate) fn siapkan_argumen_cetak(&mut self, args: &mut [VmValue]) -> VmResult<()> {
        for a in args.iter_mut() {
            if Self::mungkin_punya_hook(a) {
                *a = VmValue::str(self.tampil(a)?);
            }
        }
        Ok(())
    }

    /// Ubah sebuah nilai jadi teks seperti yang dilihat pengguna.
    pub(crate) fn tampil(&mut self, v: &VmValue) -> VmResult<String> {
        if !Self::mungkin_punya_hook(v) {
            return Ok(format!("{}", v));
        }
        match v {
            VmValue::Map(m) => {
                // Isi map dibaca lalu kuncinya DILEPAS sebelum hook dipanggil.
                let tipe = m.nama_tipe().map(|s| s.to_string());
                let bagian_enum = v.bagian_enum().map(|(_, var, p)| (var.to_string(), p.cloned()));
                let isi: Vec<(String, VmValue)> = {
                    let g = m.lock().unwrap();
                    let mut kunci: Vec<String> = g.keys().cloned().collect();
                    kunci.sort();
                    kunci.into_iter().map(|k| { let v = g[&k].clone(); (k, v) }).collect()
                };

                if let Some(ty) = &tipe {
                    if let Some(teks) = self.coba_hook(v, ty)? {
                        return Ok(teks);
                    }
                }

                // Enum: `Tipe::Varian` atau `Tipe::Varian(payload)`.
                if let (Some(ty), Some((var, payload))) = (&tipe, bagian_enum) {
                    return Ok(match payload {
                        Some(p) => format!("{}::{}({})", crate::ruang_tipe::bersih(ty), var, self.tampil(&p)?),
                        None => format!("{}::{}", crate::ruang_tipe::bersih(ty), var),
                    });
                }

                let awalan = tipe.unwrap_or_else(|| "dict".to_string());
                let mut bagian = Vec::with_capacity(isi.len());
                for (k, val) in &isi {
                    bagian.push(format!("{}: {}", k, self.tampil(val)?));
                }
                Ok(format!("{} {{{}}}", crate::ruang_tipe::bersih(&awalan), bagian.join(", ")))
            }
            VmValue::Array(a) => {
                let isi: Vec<VmValue> = a.lock().unwrap().clone();
                let mut bagian = Vec::with_capacity(isi.len());
                for x in &isi {
                    bagian.push(self.tampil(x)?);
                }
                Ok(format!("[{}]", bagian.join(", ")))
            }
            VmValue::Tuple(t) => {
                let isi: Vec<VmValue> = crate::vm::value::isi_tuple(t);
                let mut bagian = Vec::with_capacity(isi.len());
                for x in &isi {
                    bagian.push(self.tampil(x)?);
                }
                Ok(format!("({})", bagian.join(", ")))
            }
            _ => Ok(format!("{}", v)),
        }
    }

    fn mungkin_punya_hook(v: &VmValue) -> bool {
        matches!(v, VmValue::Map(_) | VmValue::Array(_) | VmValue::Tuple(_))
    }

    /// Panggil hook kalau tipe ini punya. `Ok(None)` = tidak punya.
    fn coba_hook(&mut self, v: &VmValue, tipe: &str) -> VmResult<Option<String>> {
        // Dicari lewat kunci berkualifikasi, BUKAN lewat `get_method`.
        let Some(f) = self.metode_pemakai(tipe, NAMA_HOOK) else {
            return Ok(None);
        };

        // Penjaga yang SAMA dengan interpreter, dengan konstanta dan teks yang sama
        // pula.
        if self.sarang_hook >= crate::interpreter::tampil::MAX_SARANG_HOOK {
            return Err(super::vm::VmError::new(
                crate::interpreter::tampil::pesan_sarang(tipe),
            ));
        }

        if let VmValue::Function(func) = &f {
            if func.arity != 1 {
                return Err(super::vm::VmError::new(format!(
                    "`{}::{}` dipakai sebagai hook tampilan, jadi ia harus menerima \
                     `self` saja — yang ini minta {} parameter. Ganti namanya kalau \
                     memang bukan hook tampilan",
                    tipe, NAMA_HOOK, func.arity
                )));
            }
        }

        self.sarang_hook += 1;
        let hasil = self.call_value_immediate(f, vec![v.clone()]);
        self.sarang_hook -= 1;
        match hasil? {
            VmValue::Str(s) => Ok(Some(s.to_string())),
            lain => Err(super::vm::VmError::new(format!(
                "`{}::{}` dipakai sebagai hook tampilan, jadi ia harus \
                 mengembalikan string — yang ini mengembalikan `{}`",
                tipe, NAMA_HOOK, lain.type_name()
            ))),
        }
    }
}

impl Vm {
    /// `"x" + nilai` — ubah operand yang punya hook jadi string dulu.
    pub(crate) fn siapkan_gabung_string(
        &mut self,
        a: VmValue,
        b: VmValue,
    ) -> VmResult<(VmValue, VmValue)> {
        match (&a, &b) {
            (VmValue::Str(_), _) if Self::mungkin_punya_hook(&b) => {
                let s = self.tampil(&b)?;
                Ok((a, VmValue::str(s)))
            }
            (_, VmValue::Str(_)) if Self::mungkin_punya_hook(&a) => {
                let s = self.tampil(&a)?;
                Ok((VmValue::str(s), b))
            }
            _ => Ok((a, b)),
        }
    }
}

// ─── Operator yang bisa didefinisikan pengguna
// ──────────────────────────────

use crate::interpreter::operator as op_tabel;
use crate::parser::{BinaryOp, UnaryOp};

impl Vm {
    /// Nama tipe sebuah nilai VM, kalau ia bisa punya metode.
    fn tipe_bermetode(v: &VmValue) -> Option<String> {
        v.nama_tipe().map(|s| s.to_string())
    }

    /// Coba layani operator biner lewat metode milik pengguna.
    pub(crate) fn operator_biner_pengguna(
        &mut self,
        op: &BinaryOp,
        a: &VmValue,
        b: &VmValue,
    ) -> VmResult<Option<VmValue>> {
        let Some(metode) = op_tabel::nama_biner(op) else { return Ok(None) };
        // Operand KIRI saja, sama seperti interpreter — mencari di kanan
        // akan menukar operand pada operator yang tidak komutatif.
        let Some(tipe) = Self::tipe_bermetode(a) else { return Ok(None) };
        let Some(f) = self.metode_pemakai(&tipe, metode) else {
            return Ok(None);
        };
        if let VmValue::Function(func) = &f {
            if func.arity != 2 {
                return Err(super::vm::VmError::new(op_tabel::pesan_arity(
                    &tipe, metode, func.arity as usize,
                )));
            }
        }
        let hasil = self.call_value_immediate(f, vec![a.clone(), b.clone()])?;

        if op_tabel::pakai_eq(op) {
            let VmValue::Bool(sama) = hasil else {
                return Err(super::vm::VmError::new(op_tabel::pesan_hasil(
                    &tipe, metode, "bool", hasil.type_name(),
                )));
            };
            return Ok(Some(VmValue::Bool(
                if matches!(op, BinaryOp::Ne) { !sama } else { sama },
            )));
        }
        if op_tabel::pakai_cmp(op) {
            let VmValue::Int(urut) = hasil else {
                return Err(super::vm::VmError::new(op_tabel::pesan_hasil(
                    &tipe, metode, "int", hasil.type_name(),
                )));
            };
            return Ok(Some(VmValue::Bool(op_tabel::tafsir_cmp(op, urut))));
        }
        Ok(Some(hasil))
    }

    /// Coba layani operator awalan lewat metode milik pengguna.
    pub(crate) fn operator_unary_pengguna(
        &mut self,
        op: &UnaryOp,
        v: &VmValue,
    ) -> VmResult<Option<VmValue>> {
        let Some(metode) = op_tabel::nama_unary(op) else { return Ok(None) };
        let Some(tipe) = Self::tipe_bermetode(v) else { return Ok(None) };
        let Some(f) = self.metode_pemakai(&tipe, metode) else {
            return Ok(None);
        };
        if let VmValue::Function(func) = &f {
            if func.arity != 1 {
                return Err(super::vm::VmError::new(op_tabel::pesan_arity_unary(
                    &tipe, metode, func.arity as usize,
                )));
            }
        }
        Ok(Some(self.call_value_immediate(f, vec![v.clone()])?))
    }
}

// ─── `Future::new` / `Task::spawn` di sisi VM ──────────────────────────────

impl Vm {
    /// Jalankan sebuah fungsi di thread lain, dengan runtime yang MEWARISI global
    /// pemanggilnya.
    pub(crate) fn pasang_route_vm(&mut self, metode: &str, args: &[VmValue]) -> VmResult<VmValue> {
        use crate::dok::http::{pasang_not_found, pasang_route, Handler};
        let e = |m: String| super::vm::VmError::new(m);
        let n404 = metode == "404";
        let mw = metode == "USE" || metode == "AFTER";
        let ws = metode == "WS";
        let nama = crate::dok::http::nama_router(metode);

        let Some(r) = args.first() else {
            return Err(e(format!("{}: butuh routernya", nama)));
        };
        let r_nilai = crate::vm::ffi_jembatan::TitipanVm::default().ke_interp(r).map_err(e)?;
        let id = crate::dok::http::id_router(&r_nilai, nama).map_err(e)?;

        // Urutan argumen berbeda: `router_not_found(r, h)` vs `router_get(r, p, h)`.
        let (path, i_handler) = if n404 || mw {
            (String::new(), 1)
        } else {
            match args.get(1) {
                Some(VmValue::Str(p)) => (p.to_string(), 2),
                lain => return Err(e(crate::dok::http::pesan_path_route(
                    nama, &lain.map(|v| v.type_name_penuh()).unwrap_or_default()))),
            }
        };

        let Some(h) = args.get(i_handler) else {
            return Err(e(format!("{}: butuh handler-nya", nama)));
        };
        if !h.bisa_dipanggil() {
            return Err(e(crate::dok::http::pesan_handler_route(nama, i_handler + 1, &h.type_name_penuh())));
        }
        let hid = crate::stdlib::dok::simpan_handler(h.clone());

        let hasil = if n404 {
            pasang_not_found(id, Handler::Vm(hid))
        } else if ws {
            crate::dok::http::pasang_ws(id, &path, Handler::Vm(hid))
        } else if mw {
            crate::dok::http::pasang_middleware(id, Handler::Vm(hid), metode == "USE")
        } else {
            pasang_route(id, metode, &path, Handler::Vm(hid))
        };
        hasil.map_err(e).map(|_| VmValue::Unit)
    }

    /// `dok::serve` di VM.
    pub(crate) fn serve_vm(&mut self, args: &[VmValue]) -> VmResult<VmValue> {
        use crate::dok::http::{Handler, PemanggilHandler};
        let e = |m: String| super::vm::VmError::new(m);

        let mut titipan = crate::vm::ffi_jembatan::TitipanVm::default();
        let mut nilai = Vec::with_capacity(args.len());
        for a in args {
            nilai.push(titipan.ke_interp(a).map_err(e)?);
        }

        // Global disalin sekali di sini, lalu diklon per koneksi.
        let globals = self.globals.clone();

        let mut buat = || -> PemanggilHandler {
            let globals = globals.clone();
            // `Vm`-nya dibangun DI DALAM closure, bukan di sini, supaya tiap
            // koneksi punya tumpukan dan frame sendiri. Satu `Vm` dipakai
            // beberapa thread akan saling menimpa stack-nya.
            let mut vm: Option<crate::vm::vm::Vm> = None;
            Box::new(move |h: &Handler, argumen: Vec<crate::interpreter::Value>| {
                let Handler::Vm(id) = h else {
                    return Err("handler ini milik interpreter, tapi server \
                                dijalankan dengan `--vm`"
                        .to_string());
                };
                let Some(f) = crate::stdlib::dok::ambil_handler(*id) else {
                    return Err(format!("handler #{} hilang dari tabel VM", id));
                };
                let vm = vm.get_or_insert_with(|| {
                    let mut v = crate::vm::vm::Vm::new();
                    for (k, val) in globals.clone() {
                        v.globals.insert(k, val);
                    }
                    v
                });
                // Semua argumen diseberangkan, bukan cuma yang pertama: `dok::router_after`
                // menerima `(req, resp)`, dan sebuah middleware yang cuma dapat separuh …
                let mut arg_vm = Vec::with_capacity(argumen.len());
                for a in &argumen {
                    arg_vm.push(crate::vm::ffi_jembatan::nilai_ke_vm(a)?);
                }
                let hasil = vm
                    .call_value_immediate(f, arg_vm)
                    .map_err(|er| er.msg)?;
                crate::vm::ffi_jembatan::TitipanVm::default().ke_interp(&hasil)
            })
        };

        match crate::dok::http::serve_dgn(&mut buat, &nilai) {
            Ok(v) => titipan.ke_vm(&v).map_err(e),
            // Pesannya saja, seperti `stdlib::sistem::lewat`: `Display` milik `Signal`
            // menambah awalan "runtime error:" di depan awalan "error:" diagnostiknya.
            Err(crate::interpreter::Signal::RuntimeError(r)) => Err(e(r.message)),
            Err(lain) => Err(e(format!("sinyal tak terduga dari dok::serve: {:?}", lain))),
        }
    }

    pub(crate) fn mulai_tugas_vm(&mut self, task: bool, args: &[VmValue]) -> VmResult<VmValue> {
        // Nama untuk pesan ARITAS (`Future::new butuh ...`) tetap nama fungsinya;
        // label untuk pesan KEGAGALAN memakai nama tipenya, sama dengan interpreter …
        let nama = if task { "Task::spawn" } else { "Future::new" };
        let tipe = if task { "Task" } else { "Future" };
        let Some(fungsi) = args.first().cloned() else {
            return Err(super::vm::VmError::new(format!("{} butuh minimal 1 argumen: fungsinya", nama)));
        };
        if !fungsi.bisa_dipanggil() {
            return Err(super::vm::VmError::new(crate::interpreter::pesan_tugas_bukan_fungsi(
                nama, &fungsi.type_name_penuh())));
        }
        let sisa: Vec<VmValue> = args[1..].to_vec();
        let globals = self.globals.clone();

        let ch = crate::vm::channel::VmChannel::new(1);
        let ch_kirim = ch.clone();
        let label = tipe.to_string();

        let hasil = std::thread::Builder::new()
            .name("jrg-tugas".to_string())
            .spawn(move || {
                let mut vm = crate::vm::vm::Vm::new();
                // Global pemanggil disalin, BUKAN dibagi: `VmValue` agregat sudah berupa
                // handle ber-Arc di VM, jadi yang perlu dibagi tetap terbagi, sementara yang …
                for (k, v) in globals {
                    vm.globals.insert(k, v);
                }
                let v = match vm.call_value_immediate(fungsi, sisa) {
                    Ok(v) => v,
                    // Kegagalan IKUT dibawa, dan sekarang dengan BENTUK-nya sendiri.
                    Err(e) => crate::stdlib::future::gagal_vm(format!("{} gagal: {}", label, e.msg)),
                };
                let _ = ch_kirim.send(v);
            });

        if let Err(e) = hasil {
            return Err(super::vm::VmError::new(format!("{}: tidak bisa membuat thread: {}", nama, e)));
        }

        let mut map = std::collections::HashMap::new();
        map.insert("_ch".to_string(), crate::stdlib::future::buram(ch));
        // Hak-ambil, diisi SEKARANG: sebuah tugas yang belum selesai pun sudah
        // punya hak-ambil yang belum terpakai. Penjelasan lengkap kenapa ini
        // sebuah channel dan bukan bool ada di `interpreter::tugas`.
        let serah = crate::vm::channel::VmChannel::new(1);
        let _ = serah.send(VmValue::Unit);
        map.insert(
            crate::interpreter::tugas::FIELD_SERAH.to_string(),
            crate::stdlib::future::buram(serah),
        );
        Ok(VmValue::struktur(if task { "Task" } else { "Future" }, map))
    }
}
