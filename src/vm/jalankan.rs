// src/vm/jalankan.rs — loop dispatch opcode

use std::sync::{Arc, Mutex};

use super::opcode::OpCode;
use super::value::{Label, Peta, VmValue, VmFunction, VmClosure, UpvalSrc};
use super::channel::{VmChannel, spawn_goroutine, SpawnTask};
use super::vm::*;

impl Vm {
    // ── Dispatch loop ─────────────────────────────────────────────────────

    /// Pembungkus: jalankan, lalu kalau gagal ISI JEJAKNYA sebelum galat itu
    /// keluar — `self.frames` masih utuh di titik ini, karena tidak ada yang …
    pub(crate) fn run_until(&mut self, exit_depth: usize) -> VmResult<()> {
        // Dua salinan loop: `--jit` merekam profil, `--vm` tanpa satu cabang pun untuk itu.
        let hasil = if self.profil_jit { self.run_until_inner::<true, false>(exit_depth) } else { self.run_until_inner::<false, false>(exit_depth) };
        hasil.map_err(|e| self.galat_keluar(exit_depth, e))
    }

    /// Galat yang keluar sampai `exit_depth`: jejaknya diisi (yang terdalam duluan, `run_until`
    /// bersarang lewat `call_value_immediate`), lalu bingkai di atasnya dilepas beserta `defer`-nya.
    pub(crate) fn galat_keluar(&mut self, exit_depth: usize, mut e: VmError) -> VmError {
        if e.jejak.is_empty() {
            let (jejak, sisa) = self.jejak_sekarang();
            e.jejak = jejak;
            e.jejak_sisa = sisa;
        }
        self.lepas_bingkai(exit_depth, e)
    }

    /// Galat keluar: tiap bingkai di atas `exit_depth` dilepas dari yang terdalam dan `defer`-nya
    /// DIJALANKAN, seperti acuan. Dulu bingkainya ditinggal — `defer` tidak pernah jalan, dan
    /// `--uji` menumpuk satu bingkai per test gagal sampai kena batas rekursi.
    fn lepas_bingkai(&mut self, exit_depth: usize, e: VmError) -> VmError {
        let mut galat = Some(e);
        while self.frames.len() > exit_depth {
            galat = self.run_defers(None, galat);
            let Some(frame) = self.ambil_bingkai() else { break };
            if self.profiling { crate::profiler::exit(); }
            self.stack.truncate(frame.slot);
        }
        galat.unwrap_or_else(|| VmError::new("galat hilang saat melepas bingkai"))
    }

    /// Nama fungsi seperti interpreter menyebutnya. Fungsi modul sudah DISIMPAN telanjang
    /// (`dua`, bukan `bantu::dua`) sejak kompilasi; awalan struktur (`P::m`) dipertahankan.
    pub(crate) fn nama_tampil<'a>(&self, nama: &'a str) -> &'a str {
        nama
    }

    pub(crate) fn jejak_sekarang(&self) -> (Vec<crate::diag::BingkaiJejak>, usize) {
        let mut out = Vec::new();
        let mut sisa = 0usize;
        // Frame 0 adalah `<script>` — bukan sesuatu yang ditulis pemakai,
        // jadi ia tidak pernah jadi bingkai; ia cuma jadi SUMBER situs
        // panggil untuk frame 1.
        for i in (1..self.frames.len()).rev() {
            let pemanggil = &self.frames[i - 1];
            let span = pemanggil.func.chunk.code
                .get(pemanggil.ip.saturating_sub(1))
                .and_then(|ins| if ins.span == 0 { None }
                          else { pemanggil.func.chunk.spans.get(ins.span as usize).copied() });
            // Bingkai tanpa situs panggil tidak disimpan DAN tidak dihitung.
            let Some(span) = span else { continue };
            if out.len() >= crate::diag::MAKS_BINGKAI { sisa += 1; continue; }
            let f = &self.frames[i];
            // Nama fungsi MODUL disimpan berkualifikasi di sini (`functional::compose`)
            // dan telanjang di interpreter — dan modulnya toh sudah tersebut di baris …
            let nama = if f.func.name.is_empty() {
                "<anonim>".to_string()
            } else {
                self.nama_tampil(&f.func.name).to_string()
            };
            out.push(crate::diag::BingkaiJejak::baru(nama, Some(span)));
        }
        (out, sisa)
    }

    /// `SATU`: jalankan tepat satu instruksi (jalur lambat JIT, `jit::lambat::langkah`) — salinan
    /// sendiri, supaya loop biasa tidak membayar pemeriksaannya.
    pub(crate) fn run_until_inner<const PROFIL: bool, const SATU: bool>(&mut self, exit_depth: usize) -> VmResult<()> {
        let mut sudah = false;
        loop {
            if SATU { if sudah { return Ok(()); } sudah = true; }
            if self.frames.len() <= exit_depth { return Ok(()); }
            let (op, arg, sp) = {
                let fr = self.frames.last_mut()
                    .ok_or_else(|| VmError::new("no active frame"))?;
                let i = fr.read_instr()
                    .ok_or_else(|| VmError::new("instructions exhausted"))?;
                let (op, arg, ix) = (i.op, i.arg, i.span);
                // Indeks dipetakan ke span di sini, SEKALI per instruksi, dari kolam milik
                // chunk yang sedang berjalan.
                let sp = if ix == 0 {
                    None
                } else {
                    fr.func.chunk.spans.get(ix as usize).copied()
                };
                (op, arg, sp)
            };

            // Dua bulat tidak punya operator milik pemakai: dijawab di tempat. Sisanya (operator pemakai,
            // luapan, pembagi nol) lewat `op_biner` (src/vm/op.rs), satu tempat juga untuk jalur lambat JIT.
            macro_rules! int_cepat {
            () => {
            if let [.., VmValue::Int(x), VmValue::Int(y)] = self.stack.as_slice() {
            let h = int_biner(op, *x, *y);
            if let Some(h) = h {
            self.stack.pop();
            // Banding yang langsung diikuti `JumpFalse`: lompat tanpa menaruh `bool`-nya.
            if let VmValue::Bool(benar) = h {
            let fr = self.frames.last_mut().unwrap();
            // Langkah tunggal JIT tidak menggabungkan: `JumpFalse`-nya dijalankan kode native.
            if let Some(lompat) = fr.func.chunk.code.get(fr.ip).filter(|i| !SATU && matches!(i.op, OpCode::JumpFalse)) {
            fr.ip = if benar { fr.ip + 1 } else { lompat.arg as usize };
            self.stack.pop();
            continue;
            }
            }
            if let Some(puncak) = self.stack.last_mut() { *puncak = h; }
            continue;
            }
            }
            };
            }

            macro_rules! unary_pengguna {

            ($op:expr, $a:expr) => {

            match self.operator_unary_pengguna(&$op, &$a) {

            Err(e) => return Err(if e.span.is_none() { VmError::at(e.msg, sp) } else { e }),

            Ok(Some(h)) => { self.push(h); continue; }

            Ok(None) => {}

            }

            };

            }

            if PROFIL && self.umpan_jit { self.catat_umpan(op, arg); }
            match op {
                // ── Constants ─────────────────────────────────────────────
                OpCode::Nil    => self.push(VmValue::Nil),
                OpCode::Unit   => self.push(VmValue::Unit),
                OpCode::LoadArgc => {
                    let n = self.frames.last().map(|f| f.argc).unwrap_or(0);
                    self.push(VmValue::Int(n as i128));
                }
                OpCode::True   => self.push(VmValue::Bool(true)),
                OpCode::False  => self.push(VmValue::Bool(false)),
                OpCode::IntImm => self.push(VmValue::Int(arg as i32 as i128)),
                OpCode::Const  => { let v = self.const_at(arg)?; self.push(v); }
                OpCode::Pop    => { self.pop(sp)?; }
                // Nama telanjang sebagai pernyataan: kalau nilainya bisa
                // dipanggil, ia tidak mengerjakan apa pun — jadi ditolak.
                OpCode::PopPeriksa => {
                    let v = self.pop(sp)?;
                    if v.bisa_dipanggil() {
                        let nama = {
                            let fr = self.frames.last()
                                .ok_or_else(|| VmError::new("no active frame"))?;
                            fr.func.chunk.names.get(arg as usize).cloned()
                                .unwrap_or_else(|| format!("{}", v))
                        };
                        return Err(VmError::at(
                            crate::interpreter::galat_index::fungsi_sebagai_pernyataan(&nama), sp));
                    }
                }
                OpCode::Dup    => { let v = self.peek(0, sp)?.clone(); self.push(v); }
                OpCode::Swap   => {
                    let n = self.stack.len();
                    if n < 2 { return Err(VmError::at("stack underflow Swap", sp)); }
                    self.stack.swap(n - 1, n - 2);
                }

                // ── Locals ────────────────────────────────────────────────
                // Lihat catatan pada `OpCode::CopyVal` di opcode.rs.
                OpCode::CopyVal => self.op_copy_val(sp)?,

                OpCode::LoadLocal | OpCode::LokalHitung => {
                    let slot = self.frame_slot() + arg as usize;
                    // Fusi `LoadLocal; IntImm|LoadLocal; Add|Sub|Mul|Mod` atas bulat: satu dispatch,
                    // aturan `int_biner`; yang ditolaknya berjalan instruksi demi instruksi seperti biasa.
                    if let (OpCode::LokalHitung, Some(VmValue::Int(x))) = (op, self.stack.get(slot)) {
                        let (x, basis) = (*x, self.basis);
                        let fr = self.frames.last_mut().unwrap();
                        let kode = &fr.func.chunk.code;
                        if let (Some(i1), Some(i2)) = (kode.get(fr.ip), kode.get(fr.ip + 1)) {
                            if matches!(i2.op, OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Mod) {
                                let y = match i1.op {
                                    OpCode::IntImm => Some(i1.arg as i32 as i128),
                                    OpCode::LoadLocal | OpCode::LokalHitung => match self.stack.get(basis + i1.arg as usize) {
                                        Some(VmValue::Int(y)) => Some(*y),
                                        _ => None,
                                    },
                                    _ => None,
                                };
                                if let Some(VmValue::Int(h)) = y.and_then(|y| int_biner(i2.op, x, y)) {
                                    fr.ip += 2;
                                    self.stack.push(VmValue::Int(h));
                                    continue;
                                }
                            }
                        }
                    }
                    let v = self.baca_lokal(slot)
                        .ok_or_else(|| VmError::at(format!("local slot {} OOB", arg), sp))?;
                    self.push(v);
                }
                // ── Jalur penulisan (copy-on-write) ───────────────────────
                OpCode::PinjamLokal => {
                    let slot = self.frame_slot() + arg as usize;
                    let v = {
                        let tempat = self.stack.get_mut(slot).ok_or_else(|| {
                            VmError::at(format!("local slot {} OOB", arg), sp)
                        })?;
                        match tempat {
                            // Binding tangkapan: selnya tetap dibagi (itu
                            // memang aliasnya), yang dipisahkan isinya.
                            VmValue::Cell(c) => {
                                let c = Arc::clone(c);
                                let mut g = c.lock().unwrap_or_else(|p| p.into_inner());
                                g.pisahkan();
                                g.clone()
                            }
                            biasa => { biasa.pisahkan(); biasa.clone() }
                        }
                    };
                    self.push(v);
                }
                OpCode::ArrUbahMilik => {
                    let (arg, bentuk_method) = (arg & 3, arg & 4 != 0);
                    // Argumen tambahan (push/concat) ada DI ATAS array.
                    let tambahan = if arg == 0 || arg == 3 { Some(self.pop(sp)?) } else { None };
                    let sasaran = self.pop(sp)?;
                    let VmValue::Array(a) = sasaran else {
                        if bentuk_method {
                            // Kalimat pintu method yang sama dengan `x.push(1)` biasa.
                            let nama = &nama_arr_ubah(arg)["arr::".len()..];
                            // Konteksnya PEMANGGILAN method, jadi pintu method.
                            self.get_field_pintu(&sasaran, nama, sp, true)?;
                        }
                        return Err(VmError::at(
                            crate::interpreter::pesan_arg_harus_array(
                                nama_arr_ubah(arg), 1, &sasaran.type_name_penuh()),
                            sp,
                        ));
                    };
                    // Kalau kita bukan pemegang terakhir, salin dulu —
                    // aturan copy-on-write yang sama, cuma di tempat yang
                    // bisa memanfaatkan kepemilikan kalau memang tunggal.
                    let a = if Arc::strong_count(&a) == 1 {
                        a
                    } else {
                        let isi = a.lock().unwrap_or_else(|p| p.into_inner()).clone();
                        Arc::new(std::sync::Mutex::new(isi))
                    };
                    let hasil = {
                        let mut g = a.lock().unwrap_or_else(|p| p.into_inner());
                        match arg {
                            0 => { g.push(tambahan.unwrap_or(VmValue::Nil)); None }
                            1 => Some(match g.pop() {
                                Some(v) => opsi_some(v),
                                None => opsi_none(),
                            }),
                            2 => { g.reverse(); None }
                            _ => {
                                match tambahan {
                                    Some(VmValue::Array(b)) => {
                                        let tmb: Vec<VmValue> = b
                                            .lock().unwrap_or_else(|p| p.into_inner())
                                            .iter().cloned().collect();
                                        g.extend(tmb);
                                    }
                                    Some(lain) => return Err(VmError::at(
                                        crate::interpreter::pesan_arg_harus_array(
                                            "arr::concat", 2, &lain.type_name_penuh()),
                                        sp,
                                    )),
                                    None => {}
                                }
                                None
                            }
                        }
                    };
                    match hasil {
                        // `pop` menjawab `(array_baru, Option)`, persis
                        // seperti builtin dan seperti interpreter.
                        Some(opsi) => self.push(VmValue::tuple(vec![VmValue::Array(a), opsi])),
                        None => self.push(VmValue::Array(a)),
                    }
                }
                OpCode::BongkarTuple => {
                    let n = arg as usize;
                    let nilai = self.pop(sp)?;
                    let isi = match &nilai {
                        VmValue::Tuple(t) => t.lock().unwrap_or_else(|p| p.into_inner()).clone(),
                        lain => return Err(VmError::at(
                            crate::interpreter::pesan_bongkar_bukan_tuple(n, &lain.type_name_penuh()), sp)),
                    };
                    if isi.len() != n {
                        return Err(VmError::at(crate::interpreter::pesan_bongkar_jumlah(n, isi.len()), sp));
                    }
                    drop(nilai);
                    for v in isi { self.push(v); }
                }
                OpCode::AmbilLokal => {
                    let slot = self.frame_slot() + arg as usize;
                    let v = self.ambil_lokal(slot).ok_or_else(|| VmError::at(format!("local slot {} OOB", arg), sp))?;
                    self.push(v);
                }
                OpCode::PinjamUpval => {
                    let cell = self.upval_at(arg, sp)?;
                    let v = {
                        let mut g = cell.lock().unwrap_or_else(|p| p.into_inner());
                        g.pisahkan();
                        g.clone()
                    };
                    self.push(v);
                }
                OpCode::PinjamGlobal => {
                    let name = self.name_at(arg)?;
                    let v = match self.globals.get_mut(&name) {
                        // `var` tingkat modul: yang dipisahkan isi SEL-nya, selnya tetap dibagi.
                        Some(VmValue::Cell(sel)) => {
                            let mut g = sel.lock().unwrap_or_else(|p| p.into_inner());
                            g.pisahkan();
                            g.clone()
                        }
                        Some(tempat) => { tempat.pisahkan(); tempat.clone() }
                        None => return Err(VmError::at(
                            self.jelaskan_global_hilang_kini(&name), sp,
                        )),
                    };
                    self.push(v);
                }
                OpCode::PinjamIndex => {
                    let (induk, idx) = self.pop2(sp)?;
                    let v = self.pinjam_index(&induk, &idx, sp)?;
                    self.push(v);
                }
                OpCode::PinjamField => {
                    let name = self.name_at(arg)?;
                    let induk = self.pop(sp)?;
                    let v = self.pinjam_field(&induk, &name, sp)?;
                    self.push(v);
                }

                OpCode::IndeksLokal => {
                    let slot = self.frame_slot() + arg as usize;
                    let idx = self.pop(sp)?;
                    // Larik milik tunggal dibaca tanpa kunci; aturan indeks tetap `norm_idx`.
                    if let (Some(v), VmValue::Int(i)) = (self.stack.get_mut(slot).and_then(larik_milik_tunggal), &idx) {
                        let i = norm_idx(*i, v.len(), "array", "elemen", sp)?;
                        let val = v[i].clone();
                        // Umpan jenis elemen untuk JIT (`IndeksLokal` atas larik pecahan).
                        if PROFIL { self.catat_hasil(&val); }
                        self.push(val);
                        continue;
                    }
                    let val = match self.stack.get(slot) {
                        Some(VmValue::Cell(c)) => {
                            let wadah = c.lock().unwrap_or_else(|p| p.into_inner()).clone();
                            self.get_index(&wadah, &idx, sp)?
                        }
                        Some(wadah) => self.get_index(wadah, &idx, sp)?,
                        None => return Err(VmError::at(format!("local slot {} OOB", arg), sp)),
                    };
                    if PROFIL { self.catat_hasil(&val); }
                    self.push(val);
                }
                OpCode::TulisIndexLokal => {
                    let slot = self.frame_slot() + arg as usize;
                    let (val, idx) = self.pop2(sp)?;
                    // Aturan `PinjamLokal`: isi dipisahkan dari pemegang lain sebelum ditulis.
                    let sel = match self.stack.get_mut(slot) {
                        Some(VmValue::Cell(c)) => Some(Arc::clone(c)),
                        Some(tempat) => { tempat.pisahkan(); None }
                        None => return Err(VmError::at(format!("local slot {} OOB", arg), sp)),
                    };
                    match sel {
                        Some(c) => {
                            let wadah = {
                                let mut g = c.lock().unwrap_or_else(|p| p.into_inner());
                                g.pisahkan();
                                g.clone()
                            };
                            self.set_index(&wadah, &idx, val, sp)?;
                        }
                        None => match (larik_milik_tunggal(&mut self.stack[slot]), &idx) {
                            // Sesudah `pisahkan` larik ini milik tunggal: tulis tanpa kunci.
                            (Some(v), VmValue::Int(i)) => {
                                let i = norm_idx(*i, v.len(), "array", "elemen", sp)?;
                                v[i] = val;
                            }
                            _ => self.set_index(&self.stack[slot], &idx, val, sp)?,
                        },
                    }
                }
                OpCode::TumbuhIndexLokal => {
                    let (slot, lompat) = (self.frame_slot() + (arg & 0xFF_FFFF) as usize, (arg >> 24) as usize);
                    let idx = self.pop(sp)?;
                    let n = self.stack.len();
                    if n < 2 || slot >= n { return Err(VmError::at("stack underflow", sp)); }
                    let mut selesai = false;
                    if matches!((&self.stack[n - 2], &self.stack[n - 1]), (VmValue::Array(_), VmValue::Array(_))) {
                        // Aturan `PinjamLokal`, lalu elemennya disambung kalau belum ditulis ulang.
                        let wadah = match &mut self.stack[slot] {
                            VmValue::Cell(c) => {
                                let c = Arc::clone(c);
                                let mut g = c.lock().unwrap_or_else(|p| p.into_inner());
                                g.pisahkan();
                                g.clone()
                            }
                            biasa => { biasa.pisahkan(); biasa.clone() }
                        };
                        let kini = std::mem::replace(&mut self.stack[n - 2], VmValue::Nil);
                        let kanan = &self.stack[n - 1];
                        let sambung = |cur: &mut VmValue, kini: VmValue| -> Option<VmValue> {
                            match (&*cur, &kini) {
                                (VmValue::Array(a), VmValue::Array(b)) if Arc::ptr_eq(a, b) => {
                                    drop(kini);
                                    tambah_tempat(cur, kanan);
                                    None
                                }
                                _ => Some(kini),
                            }
                        };
                        let sisa = match (&wadah, &idx) {
                            (VmValue::Array(a), VmValue::Int(i)) => {
                                let mut g = a.lock().unwrap_or_else(|p| p.into_inner());
                                let banyak = g.len() as i128;
                                let ix = if *i < 0 { banyak + *i } else { *i };
                                if (0..banyak).contains(&ix) { sambung(&mut g[ix as usize], kini) } else { Some(kini) }
                            }
                            (VmValue::Map(m), VmValue::Str(k)) if matches!(m.label, Label::Dict) => {
                                let mut g = m.lock().unwrap_or_else(|p| p.into_inner());
                                match g.get_mut(k.as_str()) { Some(cur) => sambung(cur, kini), None => Some(kini) }
                            }
                            _ => Some(kini),
                        };
                        match sisa {
                            Some(kini) => self.stack[n - 2] = kini,
                            None => selesai = true,
                        }
                    }
                    if selesai {
                        self.stack.truncate(n - 2);
                        self.frames.last_mut().unwrap().ip += lompat;
                    }
                }
                OpCode::SimpanBuang => {
                    let slot = self.frame_slot() + arg as usize;
                    let v = self.pop(sp)?;
                    self.simpan_lokal(slot, v);
                }
                OpCode::LokalPolos => {
                    let slot = self.frame_slot() + arg as usize;
                    let polos = !matches!(self.stack.get(slot), Some(VmValue::Cell(_)) | None);
                    self.push(VmValue::Bool(polos));
                }
                OpCode::StoreLocal => {
                    let slot = self.frame_slot() + arg as usize;
                    let v = self.peek(0, sp)?.clone();
                    self.simpan_lokal(slot, v);
                }

                // ── Closure / upvalue ─────────────────────────────────────
                OpCode::MakeClosure => {
                    let tmpl = self.const_at(arg)?;
                    let VmValue::Function(func) = tmpl else {
                        return Err(VmError::at(
                            format!("MakeClosure butuh template fungsi, dapat `{}`", tmpl.type_name_penuh()),
                            sp,
                        ));
                    };
                    let mut captures: Vec<Arc<Mutex<VmValue>>> = Vec::with_capacity(func.upvals.len());
                    // `move` menangkap SALINAN, bukan sel bersama.
                    let salin = func.is_move;
                    for src in &func.upvals {
                        if salin {
                            let nilai = match *src {
                                UpvalSrc::ParentLocal(sl) => {
                                    let idx = self.frame_slot() + sl as usize;
                                    let slot = self.stack.get(idx).ok_or_else(|| {
                                        VmError::at(format!("tangkapan closure: local slot {} OOB", sl), sp)
                                    })?;
                                    match slot {
                                        VmValue::Cell(c) => c.lock().unwrap_or_else(|p| p.into_inner()).clone(),
                                        biasa => biasa.clone(),
                                    }
                                }
                                UpvalSrc::ParentUpval(i) => {
                                    let induk = self.frames.last().and_then(|f| f.closure.as_ref())
                                        .ok_or_else(|| VmError::at(
                                            "tangkapan closure merujuk tangkapan induk, tapi frame ini bukan closure",
                                            sp,
                                        ))?;
                                    let c = induk.captures.get(i as usize).ok_or_else(|| {
                                        VmError::at(format!("tangkapan induk ke-{} tidak ada", i), sp)
                                    })?;
                                    c.lock().unwrap_or_else(|p| p.into_inner()).clone()
                                }
                            };
                            captures.push(Arc::new(Mutex::new(nilai)));
                            continue;
                        }
                        let cell = match *src {
                            // Binding masih hidup di stack frame ini.
                            UpvalSrc::ParentLocal(s) => {
                                let idx = self.frame_slot() + s as usize;
                                let slot = self.stack.get_mut(idx).ok_or_else(|| {
                                    VmError::at(format!("tangkapan closure: local slot {} OOB", s), sp)
                                })?;
                                match slot {
                                    VmValue::Cell(c) => Arc::clone(c),
                                    biasa => {
                                        let cell = Arc::new(Mutex::new(std::mem::replace(biasa, VmValue::Nil)));
                                        *biasa = VmValue::Cell(Arc::clone(&cell));
                                        cell
                                    }
                                }
                            }
                            // Binding datang dari lebih luar; pembuat closure
                            // ini sendiri sudah menangkapnya, jadi tinggal
                            // ikut memegang sel yang sama.
                            UpvalSrc::ParentUpval(i) => {
                                let induk = self.frames.last().and_then(|f| f.closure.as_ref())
                                    .ok_or_else(|| VmError::at(
                                        "tangkapan closure merujuk tangkapan induk, tapi frame ini bukan closure",
                                        sp,
                                    ))?;
                                induk.captures.get(i as usize).map(Arc::clone).ok_or_else(|| {
                                    VmError::at(format!("tangkapan induk ke-{} tidak ada", i), sp)
                                })?
                            }
                        };
                        captures.push(cell);
                    }
                    self.push(VmValue::Closure(Arc::new(VmClosure::new(func, captures))));
                }
                OpCode::LoadUpval => {
                    let cell = self.upval_at(arg, sp)?;
                    let v = cell.lock().unwrap_or_else(|p| p.into_inner()).clone();
                    if PROFIL { self.catat_hasil(&v); }
                    self.push(v);
                }
                OpCode::StoreUpval => {
                    let v = self.peek(0, sp)?.clone();
                    let cell = self.upval_at(arg, sp)?;
                    *cell.lock().unwrap_or_else(|p| p.into_inner()) = v;
                }

                // ── Globals ───────────────────────────────────────────────
                OpCode::LoadGlobal => {
                    // Namanya dipinjam, bukan disalin: `name_at` mengembalikan `String` hasil
                    // `clone`, jadi tiap akses global dulu mengalokasikan satu String cuma untuk …
                    let found = {
                        let fr = self.frames.last()
                            .ok_or_else(|| VmError::new("no active frame"))?;
                        let name = fr.func.chunk.names.get(arg as usize)
                            .ok_or_else(|| VmError::new(format!("name idx {} OOB", arg)))?;
                        // `var` tingkat modul tinggal di sel bersama: isinya yang dibaca,
                        // tanpa menyalin `Arc` selnya.
                        match self.globals.get(name) {
                            Some(VmValue::Cell(sel)) =>
                                Some(sel.lock().unwrap_or_else(|p| p.into_inner()).clone()),
                            lain => lain.cloned(),
                        }
                    };
                    match found {
                        Some(v) => { if PROFIL { self.catat_hasil(&v); } self.push(v) }
                        None => {
                            let name = self.name_at(arg)?;
                            return Err(VmError::at(self.jelaskan_global_hilang_kini(&name), sp));
                        }
                    }
                }
                OpCode::DefGlobal => {
                    let name = self.name_at(arg)?;
                    let v = self.peek(0, sp)?.clone();
                    self.globals.insert(name, v);
                }
                OpCode::DefVar => {
                    let name = self.name_at(arg)?;
                    let v = self.peek(0, sp)?.clone();
                    self.globals.insert(name, VmValue::Cell(Arc::new(Mutex::new(v))));
                }
                // Kompilator cuma memancarkannya untuk `var` tingkat atas unitnya sendiri; yang
                // belum sempat didefinisikan (dipakai sebelum barisnya jalan) ditolak, bukan dibuat.
                OpCode::StoreGlobal => {
                    let name = self.name_at(arg)?;
                    let v = self.peek(0, sp)?.clone();
                    match self.globals.get(&name) {
                        Some(VmValue::Cell(sel)) => {
                            *sel.lock().unwrap_or_else(|p| p.into_inner()) = v;
                        }
                        _ => return Err(VmError::at(crate::interpreter::pesan_tugas_tanpa_deklarasi(
                            super::vm::nama_ditulis(&name), None), sp)),
                    }
                }

                // ── Arithmetic ──────────────────────────────────────────── `tambah_milik`,
                // bukan `add`: sisi kiri sudah kita miliki (baru saja di-pop), jadi kalau …

                // Badan bukan-bulat satu definisi dengan jalur lambat JIT (`op_biner`).
                OpCode::Add  => { int_cepat!(); let (a, b) = self.pop2(sp)?; let h = self.op_biner(op, a, b, sp)?; self.push(h); }
                OpCode::Sub | OpCode::Mul | OpCode::Mod => { int_cepat!(); let (a, b) = self.pop2(sp)?; let h = self.op_biner(op, a, b, sp)?; self.push(h); }
                OpCode::Div | OpCode::Pow => { let (a, b) = self.pop2(sp)?; let h = self.op_biner(op, a, b, sp)?; self.push(h); }
                OpCode::Neg  => { let a = self.pop(sp)?; unary_pengguna!(crate::parser::UnaryOp::Neg, a); self.push(a.neg().map_err(|e| VmError::at(e, sp))?); }
                OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::Shl | OpCode::Shr | OpCode::AndNot => {
                    // Dua bulat (tanpa operator milik pemakai, tanpa galat): di tempat, seperti `int_cepat`.
                    if let [.., VmValue::Int(x), VmValue::Int(y)] = self.stack.as_mut_slice() {
                        let h = match op {
                            OpCode::BitAnd => Some(*x & *y),
                            OpCode::BitOr => Some(*x | *y),
                            OpCode::BitXor => Some(*x ^ *y),
                            OpCode::AndNot => Some(*x & !*y),
                            _ => None,
                        };
                        if let Some(h) = h { *x = h; self.stack.pop(); continue; }
                    }
                    let (a, b) = self.pop2(sp)?;
                    let h = self.op_biner(op, a, b, sp)?;
                    self.push(h);
                }
                // `~` ikut operator milik pemakai (`bit_not`) seperti `-` dan `!` — dulu tidak, jadi
                // `~p` jalan di interpreter dan ditolak di sini.
                OpCode::BitNot => { let a = self.pop(sp)?; unary_pengguna!(crate::parser::UnaryOp::BitNot, a); self.push(a.bit_not().map_err(|e| VmError::at(e, sp))?); }
                OpCode::KeBool => { let a = self.pop(sp)?; self.push(VmValue::Bool(a.is_truthy())); }

                // ── Comparison ────────────────────────────────────────────
                OpCode::Eq | OpCode::Ne | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge => {
                    int_cepat!();
                    let (a, b) = self.pop2(sp)?;
                    let h = self.op_biner(op, a, b, sp)?;
                    self.push(h);
                }
                // `!` cuma untuk bool — sama seperti interpreter.
                OpCode::Not  => {
                    let a = self.pop(sp)?;
                    unary_pengguna!(crate::parser::UnaryOp::Not, a);
                    match a {
                        VmValue::Bool(b) => self.push(VmValue::Bool(!b)),
                        VmValue::Int(_) => return Err(VmError::at(
                            "operator `!` cuma untuk bool, dapat `int` — kalau maksudnya \
                             membalik bit, pakai `~`; kalau maksudnya \"nilainya nol atau \
                             bukan\", tulis perbandingannya eksplisit (mis. `x == 0`)", sp)),
                        lain => return Err(VmError::at(
                            crate::interpreter::pesan_op_unary("!", &lain.type_name_penuh()), sp)),
                    }
                }
                // `Concat` melayani template literal DAN `"x" + nilai`, jadi ia harus lewat
                // hook tampilan seperti `wok` — kalau tidak, `wok(p)` dan `wok(«${p}»)` …
                OpCode::Concat => {
                    let (a, b) = self.pop2(sp)?;
                    let sa = self.tampil(&a).map_err(|e| if e.span.is_none() { VmError::at(e.msg, sp) } else { e })?;
                    let sb = self.tampil(&b).map_err(|e| if e.span.is_none() { VmError::at(e.msg, sp) } else { e })?;
                    self.push(VmValue::str(format!("{}{}", sa, sb)));
                }

                // ── Collections ───────────────────────────────────────────
                OpCode::BuildArray => self.op_build_larik(arg, false),
                OpCode::BuildTuple => self.op_build_larik(arg, true),
                OpCode::BuildMap => {
                    let n = arg as usize;
                    let start = self.stack.len().saturating_sub(n * 2);
                    let kv: Vec<VmValue> = self.stack.drain(start..).collect();
                    self.push(VmValue::map(kv.chunks(2).filter_map(|c| match c {
                        [k, v] => Some((format!("{}", k), v.clone())),
                        _ => None,
                    })));
                }
                OpCode::MergeMap => {
                    let overrides = self.pop(sp)?;
                    let base = self.pop(sp)?;
                    let VmValue::Map(o) = &overrides else {
                        return Err(VmError::at("MergeMap: literal struct-nya bukan struct", sp));
                    };
                    // Basisnya struct/dict, atau payload varian enum ber-field — aturan interpreter.
                    let mut merged = match &base {
                        VmValue::Map(b) => match &b.label {
                            Label::Enum { payload: Some(VmValue::Map(p)), .. }
                                if !matches!(p.label, Label::Enum { .. }) => p.lock().unwrap().clone(),
                            Label::Enum { .. } => return Err(VmError::at(
                                crate::interpreter::pesan_sebar_bukan_wadah(&base.type_name_penuh()), sp)),
                            _ => b.lock().unwrap().clone(),
                        },
                        lain => return Err(VmError::at(
                            crate::interpreter::pesan_sebar_bukan_wadah(&lain.type_name_penuh()), sp)),
                    };
                    merged.extend(o.lock().unwrap().clone());
                    // Label dari literalnya sendiri: `P { a: 9, ...q }` adalah sebuah `P`.
                    self.push(VmValue::Map(Arc::new(Peta::dari_isi(o.label.clone(), merged))));
                }
                OpCode::BuildStruct => self.op_build_struct(arg, sp)?,
                OpCode::BuildEnum => {
                    let payload = self.pop(sp)?;
                    let nama = match self.const_at(arg)? {
                        VmValue::Map(p) => match &p.label {
                            Label::Enum { nama, .. } => *nama,
                            _ => return Err(VmError::at("BuildEnum: prototipe bukan enum", sp)),
                        },
                        _ => return Err(VmError::at("BuildEnum: prototipe bukan enum", sp)),
                    };
                    self.push(VmValue::varian_berisi(nama, payload));
                }

                // ── Field / Index ───────────────────────────────────────── Sama seperti
                // LoadGlobal: nama dipinjam dari constant pool, tidak disalin.
                OpCode::GetField => {
                    let obj = self.pop(sp)?;
                    let val = {
                        let fr = self.frames.last()
                            .ok_or_else(|| VmError::new("no active frame"))?;
                        let name = fr.func.chunk.names.get(arg as usize)
                            .ok_or_else(|| VmError::new(format!("name idx {} OOB", arg)))?;
                        self.get_field(&obj, name, sp)?
                    };
                    if PROFIL { self.catat_hasil(&val); }
                    // Method `math::` atas bilangan yang ditulis tanpa `()` dibaca acuan (dan LLVM)
                    // sebagai nilainya: `f.floor` = `f.floor()`.
                    if matches!(obj, VmValue::Float(_) | VmValue::Int(_)) && matches!(val, VmValue::NativeFn(_)) {
                        self.push(val);
                        self.push(obj);
                        self.call_value_at(1, sp, false)?;
                        continue;
                    }
                    self.push(val);
                }
                OpCode::GetMethod => {
                    // Umpan method sekali per situs (bahan tembolok method JIT): label penerima dan method-nya.
                    let label = if PROFIL { self.label_metode_baru() } else { None };
                    self.op_get_method(arg, sp)?;
                    if let Some(label) = label { self.catat_metode(label); }
                }
                // Stack: [fungsi, penerima, arg0..argN-1]; tidak ada yang diambil.
                OpCode::PeriksaArgMetode => self.op_periksa_arg_metode(arg, sp)?,
                OpCode::SetField => {
                    let name = self.name_at(arg)?;
                    let val  = self.pop(sp)?;
                    let obj  = self.pop(sp)?;
                    self.set_field(&obj, &name, val, sp)?;
                }
                // Varian TERPERIKSA dari GetField/SetField.
                OpCode::GetFieldPrivat => {
                    let name = self.name_at(arg)?;
                    let obj  = self.pop(sp)?;
                    self.periksa_visibilitas(&obj, &name, sp)?;
                    let val = self.get_field(&obj, &name, sp)?;
                    self.push(val);
                }
                OpCode::SetFieldPrivat => {
                    let name = self.name_at(arg)?;
                    let val  = self.pop(sp)?;
                    let obj  = self.pop(sp)?;
                    self.periksa_visibilitas(&obj, &name, sp)?;
                    self.set_field(&obj, &name, val, sp)?;
                }
                OpCode::HasField => {
                    let name = self.name_at(arg)?;
                    let obj = self.pop(sp)?;
                    let has_field = match obj {
                        VmValue::Map(m) => m.lock().unwrap().contains_key(&name),
                        _ => false,
                    };
                    self.push(VmValue::Bool(has_field));
                }
                // Pola membaca LABEL — tidak ada kunci peta yang bisa memalsukannya.
                OpCode::CekEnum => {
                    let v = self.pop(sp)?;
                    let desc = self.const_at(arg)?;
                    let cocok = cocok_enum(&v, &desc).map_err(|p| VmError::at(p, sp))?;
                    self.push(VmValue::Bool(cocok));
                }
                OpCode::CekStruct => {
                    let v = self.pop(sp)?;
                    let desc = self.const_at(arg)?;
                    let (harap, minta) = match &desc {
                        VmValue::Tuple(d) => { let d = crate::vm::value::isi_tuple(d); (
                            match &d[0] { VmValue::Str(n) => Some(n.to_string()), _ => None },
                            match d.get(1) { Some(VmValue::Array(a)) => Some(a.clone()), _ => None },
                        ) }
                        VmValue::Str(n) => (Some(n.to_string()), None),
                        _ => (None, None),
                    };
                    let cocok = match (&harap, v.nama_struct()) {
                        (Some(n), Some(nama)) => n == nama,
                        _ => false,
                    };
                    // Nama strukturnya cocok tapi polanya menyebut field yang
                    // tidak ada: salah ketik, bukan "tidak cocok".
                    if cocok {
                        if let Some(minta) = minta {
                            let punya: Vec<String> = match &v {
                                VmValue::Map(m) => {
                                    let mut k: Vec<String> = m.lock().map(|g| g.keys().map(|k| k.to_string()).collect())
                                        .unwrap_or_default();
                                    k.sort();
                                    k
                                }
                                _ => Vec::new(),
                            };
                            let daftar: Vec<VmValue> = minta.lock()
                                .map(|g| g.clone()).unwrap_or_default();
                            for f in &daftar {
                                let VmValue::Str(nm) = f else { continue };
                                if punya.iter().any(|p| p == nm.as_str()) { continue; }
                                let saran = crate::diag::saran_nama_terdekat(
                                    nm.as_str(), punya.iter().map(|x| x.as_str()));
                                return Err(VmError::at(
                                    crate::interpreter::galat_index::field_pola_tak_ada(
                                        harap.as_deref().unwrap_or("struct"), nm.as_str(),
                                        &punya, saran.as_deref()), sp));
                            }
                        }
                    }
                    self.push(VmValue::Bool(cocok));
                }
                OpCode::AmbilPayload => {
                    let v = self.pop(sp)?;
                    let payload = match v.bagian_enum() {
                        Some((_, _, Some(p))) => p.clone(),
                        _ => return Err(VmError::at(format!(
                            "pola enum: `{}` tidak membawa payload", v.type_name_penuh()), sp)),
                    };
                    if PROFIL { self.catat_hasil(&payload); }
                    self.push(payload);
                }
                OpCode::GetIndex => {
                    let (obj, idx) = self.pop2(sp)?;
                    let val = self.get_index(&obj, &idx, sp)?;
                    if PROFIL { self.catat_hasil(&val); }
                    self.push(val);
                }
                OpCode::SetIndex => self.op_set_index(sp)?,
                OpCode::Slice => {
                    let hi = self.pop(sp)?;
                    let lo = self.pop(sp)?;
                    let obj = self.pop(sp)?;
                    // Bit 0/1 dari arg: batas awal/akhir DITULIS (`nil` yang ditulis ditolak).
                    let lo = (arg & 1 != 0).then_some(&lo);
                    let hi = (arg & 2 != 0).then_some(&hi);
                    let v = self.slice_value(&obj, lo, hi, sp)?;
                    self.push(v);
                }

                // ── Control flow ──────────────────────────────────────────
                OpCode::Jump => {
                    let fr = self.frames.last_mut().unwrap();
                    let mundur = (arg as usize) < fr.ip;
                    fr.ip = arg as usize;
                    if PROFIL && mundur { self.putaran_jit()?; }
                }
                OpCode::JumpFalse => {
                    let v = self.pop(sp)?;
                    if !v.is_truthy() { self.frames.last_mut().unwrap().ip = arg as usize; }
                }
                OpCode::JumpTrue => {
                    let v = self.pop(sp)?;
                    if v.is_truthy() { self.frames.last_mut().unwrap().ip = arg as usize; }
                }
                OpCode::JumpFalseKeep => {
                    let v = self.peek(0, sp)?.clone();
                    if !v.is_truthy() { self.frames.last_mut().unwrap().ip = arg as usize; }
                }
                OpCode::JumpTrueKeep => {
                    let v = self.peek(0, sp)?.clone();
                    if v.is_truthy() { self.frames.last_mut().unwrap().ip = arg as usize; }
                }

                // ── Iteration ─────────────────────────────────────────────
                OpCode::MakeIter => {
                    let val = self.pop(sp)?;
                    // `pasangan` menandai iterable yang menghasilkan
                    // (kunci, nilai) alih-alih index — yaitu dict.
                    let (items, pasangan) = match &val {
                        VmValue::Array(a)  => (a.lock().unwrap().clone(), false),
                        // Tuple sengaja tidak ada di sini: interpreter menolaknya,
                        // dan `pesan_tak_bisa_iterasi` yang menjelaskannya.
                        VmValue::Str(s)    => (s.chars().map(VmValue::Rune).collect(), false),
                        // `for i in n` tidak membuat n elemen: state rentang 0..n.
                        VmValue::Int(n) => {
                            self.push(VmValue::Int((*n).max(0)));
                            self.push(VmValue::Int(0));
                            self.push(VmValue::Int(0));
                            continue;
                        }
                        VmValue::Map(m) => {
                            // Struct dan enum bukan koleksi; jenisnya dari label, tanpa mengunci.
                            let tipe = val.type_name_penuh();
                            if tipe != "dict" {
                                return Err(VmError::at(
                                    crate::interpreter::pesan_tak_bisa_iterasi(&tipe), sp));
                            }
                            let g = m.lock().unwrap_or_else(|p| p.into_inner());
                            // Kunci diurutkan supaya urutannya deterministik,
                            // sama seperti `dict::keys`.
                            let mut kunci: Vec<String> = g.keys().map(|k| k.to_string()).collect();
                            kunci.sort();
                            (kunci.into_iter()
                                .map(|k| { let v = g[&k].clone(); VmValue::tuple(vec![VmValue::str(k), v]) })
                                .collect(), true)
                        }
                        _ => return Err(VmError::at(
                            crate::interpreter::pesan_tak_bisa_iterasi(&val.type_name_penuh()), sp)),
                    };
                    self.push(VmValue::array(items));
                    self.push(VmValue::Bool(pasangan));
                    self.push(VmValue::Int(0));
                }
                // State rentang: slot 0 = jumlah (`Int`), slot 1 = awal (`Int`),
                // bukan array + penanda `Bool` seperti iterable lain.
                OpCode::MakeIterRentang => {
                    let hi = self.pop(sp)?;
                    let lo = self.pop(sp)?;
                    let inklusif = arg == 1;
                    let nama = if inklusif { "range_inclusive" } else { "range" };
                    let arg_vm = |v: &VmValue| match v {
                        VmValue::Int(n) => crate::interpreter::ArgAngka::Bulat(*n),
                        VmValue::Float(f) => crate::interpreter::ArgAngka::Pecahan(*f),
                        lain => crate::interpreter::ArgAngka::Lain(lain.type_name_penuh()),
                    };
                    let (awal, akhir) = crate::interpreter::range_batas(nama, &arg_vm(&lo), &arg_vm(&hi))
                        .map_err(|m| VmError::at(m, sp))?;
                    let jumlah = crate::interpreter::range_hitung(nama, awal, akhir, 1, inklusif)
                        .map_err(|m| VmError::at(m, sp))?;
                    // Di atas i128::MAX putaran tidak bisa diamati; dijenuhkan.
                    self.push(VmValue::Int(i128::try_from(jumlah).unwrap_or(i128::MAX)));
                    self.push(VmValue::Int(awal));
                    self.push(VmValue::Int(0));
                }
                OpCode::CobaTambahField => {
                    let rhs = self.pop(sp)?;
                    let obj = self.pop(sp)?;
                    let name = self.name_at(arg)?;
                    let mut ok = false;
                    // Field privat yang tidak boleh disentuh dari sini: jalur biasa yang menolaknya.
                    let boleh = !crate::vm::visibilitas::perlu_diperiksa(&name)
                        || self.periksa_visibilitas(&obj, &name, sp).is_ok();
                    if boleh {
                        if let VmValue::Map(m) = &obj {
                            let mut g = m.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(cur) = g.get_mut(&name) { ok = tambah_tempat(cur, &rhs); }
                        }
                    }
                    self.push(VmValue::Bool(ok));
                }
                OpCode::CobaTumbuhLokal | OpCode::CobaTumbuhUpval | OpCode::CobaTumbuhGlobal => {
                    let (idx, dorong) = (arg & 0x7FFF_FFFF, arg >> 31 != 0);
                    let sel = if matches!(op, OpCode::CobaTumbuhUpval) { Some(self.upval_at(idx, sp)?) }
                    else if matches!(op, OpCode::CobaTumbuhLokal) {
                        match self.stack.get(self.frame_slot() + idx as usize) {
                            Some(VmValue::Cell(c)) => Some(Arc::clone(c)),
                            _ => None,
                        }
                    } else {
                        let nama = self.frames.last().and_then(|f| f.func.chunk.names.get(idx as usize));
                        match nama.and_then(|n| self.globals.get(n)) {
                            Some(VmValue::Cell(c)) => Some(Arc::clone(c)),
                            _ => None,
                        }
                    };
                    let ok = match sel {
                        Some(sel) => tumbuh_sel(&sel, self.peek(1, sp)?, self.peek(0, sp)?, dorong),
                        None => false,
                    };
                    if ok { self.pop2(sp)?; }
                    self.push(VmValue::Bool(ok));
                }
                OpCode::SamaTempat => {
                    let (k, dorong) = (self.frame_slot() + (arg & 0x7FFF_FFFF) as usize, arg >> 31 != 0);
                    let ulang = self.pop(sp)?;
                    let ok = match (self.stack.get(k), &ulang, self.stack.get(k + 1)) {
                        (Some(VmValue::Array(a)), VmValue::Array(b), Some(kanan)) =>
                            Arc::ptr_eq(a, b) && (dorong || matches!(kanan, VmValue::Array(_))),
                        _ => false,
                    };
                    drop(ulang);
                    if ok {
                        self.stack[k] = VmValue::Nil;
                        if dorong {
                            let v = std::mem::replace(&mut self.stack[k + 1], VmValue::Nil);
                            self.stack[k + 1] = VmValue::array(vec![v]);
                        }
                    }
                    self.push(VmValue::Bool(ok));
                }
                OpCode::CobaTambahIndex => {
                    let rhs = self.pop(sp)?;
                    let (obj, idx) = self.pop2(sp)?;
                    let mut ok = false;
                    if let (VmValue::Array(a), VmValue::Int(i)) = (&obj, &idx) {
                        let mut g = a.lock().unwrap_or_else(|p| p.into_inner());
                        let n = g.len() as i128;
                        let ix = if *i < 0 { n + *i } else { *i };
                        if (0..n).contains(&ix) { ok = tambah_tempat(&mut g[ix as usize], &rhs); }
                    }
                    // Dict berkunci teks: kunci yang sudah ada disambung di tempat juga.
                    if let (VmValue::Map(m), VmValue::Str(k)) = (&obj, &idx) {
                        if matches!(m.label, Label::Dict) {
                            let mut g = m.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(cur) = g.get_mut(k.as_str()) { ok = tambah_tempat(cur, &rhs); }
                        }
                    }
                    self.push(VmValue::Bool(ok));
                }
                OpCode::IterAda => {
                    let slot = self.frame_slot() + arg as usize;
                    let ada = self.iter_ada(slot, sp)?;
                    // Selalu diikuti `JumpFalse` di kepala `for`: lompat tanpa menaruh `bool`-nya.
                    let fr = self.frames.last_mut().unwrap();
                    if let Some(lompat) = fr.func.chunk.code.get(fr.ip).filter(|i| !SATU && matches!(i.op, OpCode::JumpFalse)) {
                        fr.ip = if ada { fr.ip + 1 } else { lompat.arg as usize };
                        continue;
                    }
                    self.push(VmValue::Bool(ada));
                }
                OpCode::IterMaju => {
                    let slot = self.frame_slot() + arg as usize;
                    match self.stack.get_mut(slot) {
                        Some(VmValue::Int(n)) if *n < i128::MAX => *n += 1,
                        _ => return Err(VmError::at("state iterator rusak", sp)),
                    }
                    // Fusi `IterMaju; Jump kepala` dengan kepala `IterAda; JumpFalse` iterator yang sama.
                    let fr = self.frames.last().unwrap();
                    let kode = &fr.func.chunk.code;
                    if let Some(j) = kode.get(fr.ip).filter(|i| i.op == OpCode::Jump) {
                        let t = j.arg as usize;
                        if let (Some(a), Some(jf)) = (kode.get(t), kode.get(t + 1)) {
                            if a.op == OpCode::IterAda && jf.op == OpCode::JumpFalse && self.basis + a.arg as usize == slot - 2 {
                                let keluar = jf.arg as usize;
                                let ada = self.iter_ada(slot - 2, sp)?;
                                self.frames.last_mut().unwrap().ip = if ada { t + 2 } else { keluar };
                            }
                        }
                    }
                    if PROFIL { self.putaran_jit()?; }
                }
                OpCode::IterVar => {
                    let iter_slot = self.frame_slot() + (arg / 2) as usize;
                    let var_ke = arg % 2;
                    let idx = match &self.stack[iter_slot + 2] {
                        VmValue::Int(n) => *n,
                        _ => return Err(VmError::at("state iterator rusak", sp)),
                    };
                    if let VmValue::Int(awal) = self.stack[iter_slot + 1] {
                        self.push(VmValue::Int(if var_ke == 0 { idx } else { awal.wrapping_add(idx) }));
                        continue;
                    }
                    // Array (penanda `false`): indeks tanpa menyentuh isinya, nilai langsung dari salinan privatnya.
                    if let (VmValue::Array(a), VmValue::Bool(false)) = (&self.stack[iter_slot], &self.stack[iter_slot + 1]) {
                        let elem = if var_ke == 0 { Some(VmValue::Int(idx)) }
                            else { usize::try_from(idx).ok().and_then(|i| a.lock().unwrap().get(i).cloned()) };
                        if let Some(v) = elem { self.push(v); continue; }
                    }
                    let items = self.stack[iter_slot].clone();
                    let pasangan = self.stack[iter_slot + 1].is_truthy();
                    let elem = self.get_index(&items, &VmValue::Int(idx), sp)?;
                    let nilai = if pasangan {
                        // dict: elemennya (kunci, nilai)
                        self.get_index(&elem, &VmValue::Int(var_ke as i128), sp)?
                    } else if var_ke == 0 {
                        // array/string/int: variabel pertama adalah INDEX
                        VmValue::Int(idx)
                    } else {
                        elem
                    };
                    self.push(nilai);
                }
                OpCode::IterNext => {
                    let slot = self.frame_slot() + arg as usize;
                    let (items, idx) = {
                        let items = match self.stack.get(slot) {
                            Some(VmValue::Array(a)) => a.lock().unwrap().clone(),
                            _ => return Err(VmError::at("iterator tidak valid", sp)),
                        };
                        let idx = match self.stack.get(slot + 1) {
                            Some(VmValue::Int(i)) => *i as usize,
                            _ => return Err(VmError::at("iterator index tidak valid", sp)),
                        };
                        (items, idx)
                    };
                    if idx >= items.len() {
                        self.frames.last_mut().unwrap().ip = arg as usize;
                    } else {
                        self.stack[slot + 1] = VmValue::Int((idx + 1) as i128);
                        self.push(items[idx].clone());
                    }
                }

                // ── Call / Return ─────────────────────────────────────────
                OpCode::Call => {
                    let argc = arg as usize;
                    let d = self.frames.len();
                    self.call_value_at(argc, sp, true)?;
                    // Panggilan yang selesai di tempat (bawaan): jenis hasilnya umpan spekulasi JIT.
                    if PROFIL && self.frames.len() == d { if let Some(v) = self.stack.last() { self.catat_hasil(v); } }
                }
                // arr::map/filter/reduce/any/all/find — lihat komentar panjang di opcode.rs
                // & compiler.rs (is_hof_name) buat alasan kenapa ini opcode terpisah, bukan …
                OpCode::HofCall => {
                    let name = self.name_at(arg)?;
                    // Galat pemanggilan balik yang lahir tanpa lokasi (`arr::map(a, 5)`) diberi
                    // lokasi situs HOF-nya; yang sudah punya lokasi dibiarkan.
                    self.hof_namespace(&name, sp).map_err(|e| e.isi_span_bila_kosong(sp))?;
                }
                OpCode::HofMethod => {
                    // Nama method di 24 bit bawah, jumlah argumen di 8 bit atas.
                    let method = self.name_at(arg & 0x00FF_FFFF)?;
                    self.run_hof_method(&method, (arg >> 24) as usize, sp)
                        .map_err(|e| e.isi_span_bila_kosong(sp))?;
                }
                OpCode::LoadFn => {
                    let ch = self.chunk_at(arg)?;
                    let f  = VmFunction::new(ch.name.clone(), ch.arity, ch);
                    self.push(VmValue::Function(Arc::new(f)));
                }
                OpCode::Return => {
                    let result = self.pop(sp)?;
                    // ── Defer: eksekusi semua deferred callables (LIFO) ──────
                    if let Some(e) = self.run_defers(sp, None) { return Err(e); }
                    let frame  = self.ambil_bingkai().ok_or_else(|| VmError::new("stack underflow Return"))?;
                    if self.profiling { crate::profiler::exit(); }
                    self.stack.truncate(frame.slot);
                    // Jenis hasil per situs `Call` pemanggil, bahan spekulasi JIT.
                    if PROFIL { self.catat_hasil(&result); }
                    if self.frames.len() <= exit_depth { self.push(result); return Ok(()); }
                    self.push(result);
                }
                OpCode::ReturnNil => {
                    // ── Defer: eksekusi semua deferred callables (LIFO) ──────
                    if let Some(e) = self.run_defers(sp, None) { return Err(e); }
                    let frame = self.ambil_bingkai().ok_or_else(|| VmError::new("stack underflow ReturnNil"))?;
                    if self.profiling { crate::profiler::exit(); }
                    self.stack.truncate(frame.slot);
                    if self.frames.len() <= exit_depth { return Ok(()); }
                    // Fungsi yang selesai tanpa `return` (atau dengan
                    // `return` telanjang) menghasilkan unit, bukan nil —
                    // sama seperti interpreter.
                    self.push(VmValue::Unit);
                }

                // ── Channel ops ───────────────────────────────────────────
                // ── New opcodes: SlideDown / DeferPush / BuildRepeat ──────
                OpCode::SlideDown => {
                    // Pertahankan top-of-stack, pop `arg` nilai di bawahnya
                    let n   = arg as usize;
                    let top = self.pop(sp)?;
                    for _ in 0..n { self.pop(sp)?; }
                    self.push(top);
                }
                OpCode::DeferPush => {
                    // Stack: [callee, arg0..argN-1] dengan `arg` = N.
                    // Semuanya SUDAH dievaluasi di baris `defer`; yang
                    // ditunda cuma pemanggilannya.
                    let n = arg as usize;
                    let mut args = Vec::with_capacity(n);
                    for _ in 0..n { args.push(self.pop(sp)?); }
                    args.reverse();
                    let callee = self.pop(sp)?;
                    let gagal = self.galat_tunda_metode.take();
                    if let Some(f) = self.frames.last_mut() {
                        f.defers.push((callee, args, gagal));
                    }
                }
                OpCode::BuildRepeat => {
                    // Stack: [elem, count] → Array([elem; count])
                    let n = self.pop(sp)?;
                    let a = match &n {
                        VmValue::Int(k) => crate::interpreter::ArgAngka::Bulat(*k),
                        VmValue::Float(f) => crate::interpreter::ArgAngka::Pecahan(*f),
                        lain => crate::interpreter::ArgAngka::Lain(lain.type_name_penuh()),
                    };
                    let count = crate::interpreter::jumlah_ulang(&a).map_err(|m| VmError::at(m, sp))?;
                    let elem  = self.pop(sp)?;
                    let items: Vec<VmValue> = std::iter::repeat(elem).take(count).collect();
                    self.push(VmValue::array(items));
                }

                OpCode::ChanMake => {
                    // stack top = capacity (Int)
                    let cap = match self.pop(sp)? {
                        VmValue::Int(n) => n.max(0) as usize,
                        _ => 0,
                    };
                    self.push(VmValue::Channel(VmChannel::new(cap)));
                }
                OpCode::ChanSend => {
                    // stack: [..., channel, value] → send value to channel
                    let val = self.pop(sp)?;
                    let ch  = self.pop(sp)?;
                    match ch {
                        VmValue::Channel(c) => c.send(val).map_err(|e| VmError::at(e, sp))?,
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel("`c <- v` (kirim)", &other.type_name_penuh()), sp)),
                    }
                }
                OpCode::ChanRecv => {
                    // stack: [..., channel] → push received value (blocking)
                    let ch = self.pop(sp)?;
                    match ch {
                        VmValue::Channel(c) => {
                            // Ditutup DAN kosong bukan `nil`: `nil` nilai yang sah untuk dibawa sebuah
                            // channel, jadi menjawabnya membuat "sudah habis" tak bisa dibedakan dari …
                            let val = c.recv().map_err(|e| VmError::at(e, sp))?
                                .ok_or_else(|| VmError::at(
                                    crate::antrean::PESAN_TERIMA_TERTUTUP.to_string(), sp))?;
                            self.push(val);
                        }
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel("`<-c` (terima)", &other.type_name_penuh()), sp)),
                    }
                }
                OpCode::ChanTrySend => {
                    let val = self.pop(sp)?;
                    let ch  = self.pop(sp)?;
                    match ch {
                        VmValue::Channel(c) => {
                            let ok = c.try_send(val).map_err(|e| VmError::at(e, sp))?;
                            self.push(VmValue::Bool(ok));
                        }
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel("`c <- v` (kirim)", &other.type_name_penuh()), sp)),
                    }
                }
                OpCode::ChanTryRecv => {
                    let ch = self.pop(sp)?;
                    match ch {
                        VmValue::Channel(c) => {
                            let val = c.try_recv().map_err(|e| VmError::at(e, sp))?
                                .unwrap_or(VmValue::Nil);
                            self.push(val);
                        }
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel("`<-c` (terima)", &other.type_name_penuh()), sp)),
                    }
                }
                OpCode::ChanClose => {
                    let ch = self.pop(sp)?;
                    match ch {
                        VmValue::Channel(c) => c.close(),
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel("`chan::close`", &other.type_name_penuh()), sp)),
                    }
                }

                // ── Spawn goroutine ───────────────────────────────────────
                OpCode::Spawn => {
                    let argc   = arg as usize;
                    let fn_idx = self.stack.len().saturating_sub(argc + 1);
                    let fn_val = self.stack[fn_idx].clone();
                    let args: Vec<VmValue> = self.stack[fn_idx+1..].to_vec();
                    self.stack.truncate(fn_idx); // pop fn + args

                    // Snapshot globals for the goroutine
                    let globals_snapshot = self.globals.clone();

                    spawn_goroutine(SpawnTask {
                        fn_val,
                        args,
                        globals: globals_snapshot,
                    }).map_err(|m| VmError::at(m, sp))?;
                    self.push(VmValue::Nil);
                }

                // ── Select ────────────────────────────────────────────────
                OpCode::CekChannelSelect => {
                    let v = self.peek(0, sp)?;
                    if !matches!(v, VmValue::Channel(_)) {
                        return Err(VmError::at(crate::interpreter::pesan_bukan_channel(
                            "`case` di `select`", &v.type_name_penuh()), sp));
                    }
                }
                OpCode::CekTutupSelect => {
                    // Sama dengan `<-c`/`c <- v`: operasi yang tak akan pernah berhasil adalah
                    // galat — dulu `select` menggantung, atau menjawab `nil`.
                    if let VmValue::Channel(c) = self.pop(sp)? {
                        if arg == 0 && c.is_closed() && c.len() == 0 {
                            return Err(VmError::at(crate::antrean::PESAN_TERIMA_TERTUTUP.to_string(), sp));
                        }
                        if arg == 1 && c.is_closed() {
                            return Err(VmError::at(crate::antrean::PESAN_KIRIM_TERTUTUP.to_string(), sp));
                        }
                    }
                }
                OpCode::SelectBegin => {
                    // arg = number of arms. Stack has nothing special here.
                    // Arms follow immediately.
                }
                OpCode::SelectRecvArm => {
                    // Stack top = channel to try recv from
                    // arg = jump target if NOT ready (skip this arm)
                    let ch = self.peek(0, sp)?.clone();
                    match ch {
                        VmValue::Channel(c) => {
                            // `coba_terima`, bukan `has_value` + `recv`: di channel tanpa penyangga
                            // pengirim yang menunggu baru menaruh nilainya sesudah melihat penerima.
                            self.pop(sp)?;
                            match c.coba_terima() {
                                Ok(val) => self.push(val),
                                Err(_) => self.frames.last_mut().unwrap().ip = arg as usize,
                            }
                        }
                        // Bukan channel DITOLAK: dulu dianggap "belum siap", jadi
                        // `select` menggantung selamanya atau diam-diam ke `default`.
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel(
                            "`case` di `select`", &other.type_name_penuh()), sp)),
                    }
                }
                OpCode::SelectSendArm => {
                    // Stack: [..., channel, value]
                    // arg = jump target if NOT ready
                    let val = self.peek(0, sp)?.clone();
                    let ch  = self.peek(1, sp)?.clone();
                    match ch {
                        VmValue::Channel(c) => {
                            if c.try_send(val.clone()).map_err(|e| VmError::at(e, sp))? {
                                self.pop(sp)?; // val
                                self.pop(sp)?; // ch
                                // Fall through to arm body
                            } else {
                                self.pop(sp)?; // val
                                self.pop(sp)?; // ch
                                self.frames.last_mut().unwrap().ip = arg as usize;
                            }
                        }
                        other => return Err(VmError::at(crate::interpreter::pesan_bukan_channel(
                            "`case` di `select`", &other.type_name_penuh()), sp)),
                    }
                }
                OpCode::SelectEnd => {
                    if arg != u32::MAX {
                        std::thread::sleep(std::time::Duration::from_micros(200));
                        self.frames.last_mut().unwrap().ip = arg as usize;
                    }
                }
                OpCode::Propagate => {
                    let v = self.pop(sp)?;
                    let (ty, variant, payload) = Self::tagged_parts(&v).ok_or_else(|| {
                        VmError::at(
                            format!("operator `?` hanya berlaku untuk Result/Option, dapat `{}`", v.type_name_penuh()),
                            sp,
                        )
                    })?;
                    if ty != "Result" && ty != "Option" {
                        return Err(VmError::at(
                            format!("operator `?` hanya berlaku untuk Result/Option, dapat `{}`", ty),
                            sp,
                        ));
                    }

                    if variant == "Err" || variant == "None" {
                        if let Some(e) = self.run_defers(sp, None) { return Err(e); }
                        let frame = self.ambil_bingkai().ok_or_else(|| VmError::new("stack underflow Propagate"))?;
                        self.stack.truncate(frame.slot);
                        if self.frames.len() <= exit_depth {
                            self.push(v);
                            return Ok(());
                        }
                        self.push(v);
                    } else if variant == "Ok" || variant == "Some" {
                        self.push(payload.unwrap_or(VmValue::Nil));
                    } else {
                        return Err(VmError::at(
                            format!("variant {}::{} tidak valid untuk operator `?`", ty, variant),
                            sp,
                        ));
                    }
                }

                // ── Misc ──────────────────────────────────────────────────
                OpCode::Typeof => self.op_typeof(sp)?,
                OpCode::Len => self.op_len(sp)?,
                OpCode::Print => {
                    let v = self.pop(sp)?;
                    crate::host::tulis_baris(&v.to_string());
                    self.push(VmValue::Nil);
                }
                OpCode::PrintNoNl => {
                    let v = self.pop(sp)?;
                    crate::host::tulis(&v.to_string());
                    self.push(VmValue::Nil);
                }
                OpCode::SelectDefault => { /* default arm no-op */ }
                OpCode::GagalCocok => {
                    let v = self.peek(0, sp)?;
                    let pesan = if arg >= 2 {
                        crate::interpreter::pesan_let_tak_cocok((arg - 2) as u8, &v.type_name_penuh())
                    } else if arg == 1 {
                        crate::interpreter::pesan_koalesensi_bukan_option(&v.type_name_penuh())
                    } else {
                        crate::interpreter::pesan_match_tak_cocok(&v.to_string())
                    };
                    return Err(VmError::at(pesan, sp));
                }
                OpCode::Panic => {
                    let v = self.pop(sp)?;
                    return Err(VmError::at(format!("{}", v), sp));
                }
                OpCode::Halt => return Ok(()),
            }
        }
    }
}

/// Sambung `kanan` ke array milik sel sebuah binding, di tempat: hanya kalau sel itu masih
/// memegang penyimpanan `kini` (tidak ada yang menulisnya sejak dibaca) dan pemegangnya cuma
/// berdua, sel dan `kini`.
fn tumbuh_sel(sel: &Mutex<VmValue>, kini: &VmValue, kanan: &VmValue, dorong: bool) -> bool {
    let VmValue::Array(k) = kini else { return false };
    let g = sel.lock().unwrap_or_else(|p| p.into_inner());
    let VmValue::Array(a) = &*g else { return false };
    if !Arc::ptr_eq(a, k) || Arc::strong_count(a) != 2 { return false; }
    let tambahan: Vec<VmValue> = match (dorong, kanan) {
        (true, v) => vec![v.clone()],
        (false, VmValue::Array(b)) => b.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        _ => return false,
    };
    a.lock().unwrap_or_else(|p| p.into_inner()).extend(tambahan);
    true
}

/// Sambung `rhs` ke teks/array di `cur` tanpa menyalin isinya, HANYA kalau hasilnya pasti
/// sama dengan `Add` (tanpa operator pemakai, tanpa hook tampilan); `false` = tak disentuh.
fn tambah_tempat(cur: &mut VmValue, rhs: &VmValue) -> bool {
    let aman = match (&*cur, rhs) {
        (VmValue::Str(_), VmValue::Str(_) | VmValue::Int(_) | VmValue::Float(_)
            | VmValue::Bool(_) | VmValue::Rune(_)) => true,
        (VmValue::Array(_), VmValue::Array(_)) => true,
        _ => false,
    };
    if !aman { return false; }
    let lama = std::mem::replace(cur, VmValue::Nil);
    *cur = lama.tambah_milik(rhs).expect("teks + skalar dan array + array tidak pernah gagal");
    true
}

/// Isi larik yang cuma dimiliki slot ini, tanpa kunci (`Arc::get_mut` + `Mutex::get_mut`).
pub(crate) fn larik_milik_tunggal(v: &mut VmValue) -> Option<&mut Vec<VmValue>> {
    let VmValue::Array(a) = v else { return None };
    crate::vm::value::tunggal(a).map(|m| m.get_mut().unwrap_or_else(|p| p.into_inner()))
}

/// Dua bulat di jalur cepat; `None` = luapan, pembagi 0/-1, atau bukan operator ini (jalur biasa
/// yang punya kalimatnya). Satu definisi untuk `int_cepat` dan fusi `LoadLocal`.
#[inline]
fn int_biner(op: OpCode, x: i128, y: i128) -> Option<VmValue> {
    let kecil = i64::try_from(x).ok().zip(i64::try_from(y).ok());
    match op {
        OpCode::Add => x.checked_add(y).map(VmValue::Int),
        OpCode::Sub => x.checked_sub(y).map(VmValue::Int),
        OpCode::Mul => match kecil {
            Some((p, q)) => Some(VmValue::Int(p as i128 * q as i128)),
            None => x.checked_mul(y).map(VmValue::Int),
        },
        OpCode::Mod => match kecil {
            Some((p, q)) if q != 0 && q != -1 => Some(VmValue::Int((p % q) as i128)),
            _ => None,
        },
        OpCode::Eq => Some(VmValue::Bool(x == y)),
        OpCode::Ne => Some(VmValue::Bool(x != y)),
        OpCode::Lt => Some(VmValue::Bool(x < y)),
        OpCode::Le => Some(VmValue::Bool(x <= y)),
        OpCode::Gt => Some(VmValue::Bool(x > y)),
        OpCode::Ge => Some(VmValue::Bool(x >= y)),
        _ => None,
    }
}

impl Vm {
    /// Apakah iterator di `slot` (`[items, penanda, idx]`) masih punya elemen; satu definisi
    /// untuk `IterAda` dan fusi `IterMaju`.
    #[inline]
    pub(crate) fn iter_ada(&self, slot: usize, sp: Option<crate::lexer::Span>) -> VmResult<bool> {
        let idx = match self.stack.get(slot + 2) {
            Some(VmValue::Int(n)) => *n,
            _ => return Err(VmError::at("state iterator rusak", sp)),
        };
        match &self.stack[slot] {
            VmValue::Int(jumlah) => Ok(idx < *jumlah),
            VmValue::Array(a) => Ok((idx as usize) < a.lock().unwrap().len()),
            _ => Err(VmError::at("state iterator rusak", sp)),
        }
    }
}

impl Vm {
    /// Isi slot lokal (`LoadLocal`): slot yang ditangkap closure menyimpan SEL, dan membaca variabelnya
    /// tetap memberi nilainya.
    #[inline]
    pub(crate) fn baca_lokal(&self, slot: usize) -> Option<VmValue> {
        Some(match self.stack.get(slot)? {
            VmValue::Cell(c) => c.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            lain => lain.clone(),
        })
    }

    /// `AmbilLokal`: nilai slot keluar dan slotnya jadi lubang. Binding tertangkap closure: selnya tidak
    /// boleh dikosongkan, jadi cuma dibaca (hasilnya tetap benar, cuma tidak ikut jalur cepatnya).
    #[inline]
    pub(crate) fn ambil_lokal(&mut self, slot: usize) -> Option<VmValue> {
        Some(match self.stack.get_mut(slot)? {
            VmValue::Cell(c) => c.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            tempat => std::mem::replace(tempat, VmValue::Nil),
        })
    }

    /// `StoreLocal`/`SimpanBuang`: binding yang sudah jadi sel ditulis MENEMBUS selnya.
    #[inline]
    pub(crate) fn simpan_lokal(&mut self, slot: usize, v: VmValue) {
        match self.stack.get_mut(slot) {
            Some(VmValue::Cell(c)) => *c.lock().unwrap_or_else(|p| p.into_inner()) = v,
            Some(tempat) => *tempat = v,
            None => {}
        }
    }

    /// Satu putaran loop di bingkai teratas; sesekali dicoba OSR ke kode native (`--jit`).
    fn putaran_jit(&mut self) -> VmResult<()> {
        let Some(p) = self.frames.last().and_then(|f| f.func.chunk.profil.as_ref()) else { return Ok(()) };
        let _n = p.catat_putaran();
        #[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
        if _n % crate::jit::osr_tiap() == 0 && !self.profiling { return crate::jit::panggil::coba_osr(self); }
        Ok(())
    }

    /// Label tipe penerima `GetMethod` yang situsnya belum pernah direkam (umpan `LAIN` menandainya).
    #[inline]
    fn label_metode_baru(&self) -> Option<String> {
        let fr = self.frames.last()?;
        let p = fr.func.chunk.profil.as_ref()?;
        let ip = fr.ip.wrapping_sub(1);
        if p.umpan.get(ip)?.load(std::sync::atomic::Ordering::Relaxed) != 0 { return None; }
        p.catat_tipe(ip, crate::jit::profil::bit::LAIN);
        Some(self.stack.last()?.nama_tipe()?.to_string())
    }

    /// Method pemakai hasil `GetMethod` (puncak stack) dicatat untuk label `label`.
    fn catat_metode(&self, label: String) {
        let (Some(fr), Some(VmValue::Function(f))) = (self.frames.last(), self.stack.last()) else { return };
        fr.func.chunk.kode_jit.catat_metode(fr.ip.wrapping_sub(1), label, Arc::clone(f));
    }

    /// Jenis nilai yang dihasilkan instruksi yang baru jalan (`ip - 1` bingkai teratas), untuk spekulasi JIT.
    #[inline]
    pub(crate) fn catat_hasil(&self, v: &VmValue) {
        if let Some(fr) = self.frames.last() {
            if let Some(p) = fr.func.chunk.profil.as_ref() { p.catat_tipe(fr.ip.wrapping_sub(1), crate::jit::profil::bit_tipe(v)); }
        }
    }

    /// Umpan tipe T0 untuk situs operator: tipe operand yang SUDAH di stack saat opcode ini jalan.
    /// Opcode diperiksa lebih dulu: sebagian besar instruksi bukan situs, dan tidak boleh membayar
    /// pembacaan bingkai.
    #[inline(always)]
    fn catat_umpan(&self, op: OpCode, arg: u32) {
        use crate::jit::profil::bit_tipe;
        if !matches!(op, OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod
            | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne
            | OpCode::LokalHitung) { return; }
        let Some(fr) = self.frames.last() else { return };
        let Some(p) = fr.func.chunk.profil.as_ref() else { return };
        let ip = fr.ip.wrapping_sub(1);
        let n = self.stack.len();
        let b = match op {
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod
            | OpCode::Lt | OpCode::Le | OpCode::Gt | OpCode::Ge | OpCode::Eq | OpCode::Ne if n >= 2 =>
                bit_tipe(&self.stack[n - 1]) | bit_tipe(&self.stack[n - 2]),
            OpCode::LokalHitung => match self.stack.get(self.basis + arg as usize) {
                Some(v) => bit_tipe(v),
                None => return,
            },
            _ => return,
        };
        p.catat_tipe(ip, b);
    }
}

/// Pola enum `desc` (`(tipe, varian, berpayload, jumlah sub-pola)`) atas `v`; `Err` = kalimat galat
/// sub-pola yang jumlahnya salah. Satu definisi untuk VM dan jalur lambat JIT.
pub(crate) fn cocok_enum(v: &VmValue, desc: &VmValue) -> Result<bool, String> {
    let cocok = match (desc, v.bagian_enum()) {
        (VmValue::Tuple(d), Some((tipe, varian, payload))) => {
            // Dibaca di tempat: deskriptornya konstanta chunk, payload-nya cuma dihitung.
            let d = d.lock().unwrap_or_else(|p| p.into_inner());
            // Varian lebih dulu: yang tidak cocok (kebanyakan lengan) selesai tanpa membanding tipenya.
            if !matches!(&d[1], VmValue::Str(s) if s.as_str() == varian) { return Ok(false); }
            let tipe_cocok = match &d[0] { VmValue::Str(t) => t.as_str() == tipe, _ => true };
            let varian_cocok = true;
            let payload_cocok = !matches!(d[2], VmValue::Bool(true)) || payload.is_some();
            // Nama varian cocok tapi jumlah sub-polanya tidak:
            // salah ketik, bukan "tidak cocok". Di sini, supaya
            // waktu gagalnya sama dengan interpreter.
            if tipe_cocok && varian_cocok {
                if let Some(VmValue::Int(dapat)) = d.get(3) {
                    let butuh = match payload {
                        None => 0usize,
                        Some(VmValue::Tuple(t)) => t.lock().unwrap_or_else(|p| p.into_inner()).len(),
                        Some(_) => 1,
                    };
                    let dapat = *dapat as usize;
                    if dapat != butuh && !(dapat == 1 && butuh > 1) {
                        let nama = match (&d[0], &d[1]) {
                            (VmValue::Str(t), VmValue::Str(x)) => format!("{}::{}", t, x),
                            (_, VmValue::Str(x)) => x.to_string(),
                            _ => varian.to_string(),
                        };
                        return Err(crate::interpreter::pesan_subpola_varian(&nama, butuh, dapat));
                    }
                }
            }
            tipe_cocok && varian_cocok && payload_cocok
        }
        _ => false,
    };
    Ok(cocok)
}
