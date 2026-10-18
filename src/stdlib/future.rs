// src/stdlib/future.rs
// Future / Task Handle (berbasis channel + OS threads)

use crate::vm::vm::Vm;
use crate::vm::value::VmValue;
use crate::vm::channel::VmChannel;
use std::sync::{Arc, Mutex};
use crate::sinkron::{self, Pegangan};
use crate::vm::value::VmValue::{Int, Sinkron};
use std::sync::atomic::{AtomicI64, Ordering};
use std::collections::HashMap;

pub fn register(vm: &mut Vm) {
    use VmValue::*;

    // ── Future ────────────────────────────────────────────────────────────

    vm.def("Future::new", |a| {
        if a.is_empty() {
            return Err("Future::new(fn, args...) butuh minimal 1 arg".to_string());
        }
        let fn_val = a[0].clone();
        let args   = a[1..].to_vec();

        // Buat channel kapasitas 1 untuk menampung hasil
        let ch = VmChannel::new(1);

        // Clone untuk goroutine
        let ch_clone = ch.clone();
        let globals  = HashMap::new(); // goroutine dapat globals dari caller

        // Spawn OS thread untuk eksekusi fn
        std::thread::spawn(move || {
            let mut vm = crate::vm::vm::Vm::new();
            for (k, v) in globals { vm.globals.insert(k, v); }
            // Kegagalan IKUT dibawa, tidak ditelan jadi `nil`.
            let result = match vm.call_value_immediate(fn_val, args) {
                Ok(v) => v,
                Err(e) => VmValue::str(format!("Future gagal: {}", e.msg)),
            };
            let _ = ch_clone.send(result);
        });

        let mut map = HashMap::new();
        map.insert("_ch".to_string(),      buram(ch));
        map.insert("done".to_string(),     Bool(false));
        Ok(VmValue::struktur("Future", map))
    });

    vm.def("future::await", |a| {
        let (c, serah) = kanal_tunggal(a.first(), "future::await")?;
        // Hak-ambil DIKLAIM sebelum menunggu. Tanpa ini `await` kedua
        // menggantung selamanya — terukur di kedua backend, tanpa keluaran,
        // tanpa pesan, tanpa lokasi.
        if !klaim(&serah) {
            return Err(crate::interpreter::tugas::pesan_sudah_diserahkan("future::await"));
        }
        let val = c.recv().map_err(|e| format!("future::await: {}", e))?.unwrap_or(Nil);
        cek_gagal_vm(val)
    });

    vm.def("future::is_done", |a| {
        let (c, serah) = kanal_tunggal(a.first(), "future::is_done")?;
        // "Selesai" = hasilnya ada, ATAU sudah diserahkan. Sebelum ini ia
        // menjawab `false` sesudah `await` — jawaban yang salah, bukan
        // sekadar kurang.
        Ok(Bool(c.has_value() || !masih_boleh(&serah)))
    });

    // ── future::all / race / timeout / try_get ──────────────────────────
    type Kanal = (crate::vm::channel::VmChannel, Option<crate::vm::channel::VmChannel>);

    fn klaim(serah: &Option<crate::vm::channel::VmChannel>) -> bool {
        match serah {
            Some(c) => matches!(c.try_recv(), Ok(Some(_))),
            None => true,
        }
    }

    fn masih_boleh(serah: &Option<crate::vm::channel::VmChannel>) -> bool {
        match serah {
            Some(c) => c.has_value(),
            None => true,
        }
    }

    fn ambil_kanal(f: &VmValue) -> Option<Kanal> {
        // Jenisnya dibaca dari LABEL, seperti acuan: dict ber-`_ch` bukan Future.
        if !matches!(f.nama_struct(), Some("Future" | "Task")) { return None; }
        let Map(m) = f else { return None };
        let g = m.lock().unwrap_or_else(|p| p.into_inner());
        let ch = g.get("_ch").and_then(saluran)?;
        let serah = g.get(crate::interpreter::tugas::FIELD_SERAH).and_then(saluran);
        Some((ch, serah))
    }

    fn kanal_dari_array(v: Option<&VmValue>, siapa: &str) -> Result<Vec<Kanal>, String> {
        let Some(Array(arr)) = v else {
            return Err(format!(
                "{}: argumen 1 harus array berisi Future/Task, dapat `{}`",
                siapa,
                v.map(|x| x.type_name_penuh()).unwrap_or_else(|| "tidak ada".to_string())
            ));
        };
        let isi = arr.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut out = Vec::with_capacity(isi.len());
        for (i, f) in isi.iter().enumerate() {
            match ambil_kanal(f) {
                Some(k) => out.push(k),
                None => return Err(crate::interpreter::tugas::pesan_bukan_tugas(
                    &format!("{}: elemen ke-{}", siapa, i),
                    &f.type_name_penuh())),
            }
        }
        Ok(out)
    }

    fn kanal_tunggal(v: Option<&VmValue>, siapa: &str) -> Result<Kanal, String> {
        match v.and_then(ambil_kanal) {
            Some(k) => Ok(k),
            None => Err(crate::interpreter::tugas::pesan_bukan_tugas(
                siapa,
                &v.map(|x| x.type_name_penuh()).unwrap_or_else(|| "tidak ada".to_string()))),
        }
    }

    vm.def("future::all", |a| {
        let chans = kanal_dari_array(a.first(), "future::all")?;
        let mut hasil = Vec::with_capacity(chans.len());
        for (i, (c, serah)) in chans.iter().enumerate() {
            if !klaim(serah) {
                return Err(crate::interpreter::tugas::pesan_sudah_diserahkan(
                    &format!("future::all: elemen ke-{}", i)));
            }
            let v = c.recv().map_err(|e| format!("future::all: {}", e))?.unwrap_or(Nil);
            hasil.push(cek_gagal_vm(v)?);
        }
        Ok(Array(Arc::new(Mutex::new(hasil))))
    });

    vm.def("future::race", |a| {
        let chans = kanal_dari_array(a.first(), "future::race")?;
        if chans.is_empty() {
            return Err("future::race: arraynya kosong — tidak ada yang bisa dibalap, \
                        dan menunggu tidak akan pernah menghasilkan apa pun"
                .to_string());
        }
        // Diperiksa sekali di depan: sebuah future yang hasilnya sudah
        // diambil tidak akan pernah menang, jadi menunggunya hanya menunda
        // pesan yang sudah pasti.
        for (i, (_, serah)) in chans.iter().enumerate() {
            if !masih_boleh(serah) {
                return Err(crate::interpreter::tugas::pesan_sudah_diserahkan(
                    &format!("future::race: elemen ke-{}", i)));
            }
        }
        loop {
            for (c, serah) in &chans {
                if let Ok(Some(v)) = c.try_recv() {
                    klaim(serah);
                    return cek_gagal_vm(v);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    vm.def("future::timeout", |a| {
        let (c, serah) = kanal_tunggal(a.first(), "future::timeout")?;
        let ms = match a.get(1) {
            Some(Int(n)) if *n >= 0 => *n,
            lain => return Err(format!(
                "future::timeout: argumen 2 harus int >= 0 (milidetik), dapat `{}`",
                lain.map(|x| x.type_name_penuh()).unwrap_or_else(|| "tidak ada".to_string()))),
        };
        // `None` harus berarti SATU hal: "belum siap".
        if !masih_boleh(&serah) {
            return Err(crate::interpreter::tugas::pesan_sudah_diserahkan("future::timeout"));
        }
        let batas = crate::interpreter::tugas::batas_tunggu(ms);
        loop {
            if let Ok(Some(v)) = c.try_recv() {
                klaim(&serah);
                return Ok(VmValue::some(cek_gagal_vm(v)?));
            }
            if crate::interpreter::tugas::lewat(batas) { return Ok(VmValue::none()); }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    vm.def("future::try_get", |a| {
        let (c, serah) = kanal_tunggal(a.first(), "future::try_get")?;
        if !masih_boleh(&serah) {
            return Err(crate::interpreter::tugas::pesan_sudah_diserahkan("future::try_get"));
        }
        Ok(match c.try_recv() {
            Ok(Some(v)) => {
                klaim(&serah);
                VmValue::some(cek_gagal_vm(v)?)
            }
            _ => VmValue::none(),
        })
    });

    // Definisi lama `is_done`/`all`/`race`/`timeout` DIHAPUS dari sini.

    // ── Task ─────────────────────────────────────────────────────────────

    static TASK_ID: AtomicI64 = AtomicI64::new(1);

    vm.def("Task::spawn", |a| {
        if a.is_empty() {
            return Err("Task::spawn(fn, args...) butuh minimal 1 arg".to_string());
        }
        let fn_val = a[0].clone();
        let args   = a[1..].to_vec();
        let id     = TASK_ID.fetch_add(1, Ordering::SeqCst);

        // Done channel: kapasitas 1, dikirim nil saat selesai
        let done_ch = VmChannel::new(1);
        let done_clone = done_ch.clone();

        std::thread::spawn(move || {
            let mut vm = crate::vm::vm::Vm::new();
            // Hasilnya DIKIRIM, bukan dibuang.
            let hasil = match vm.call_value_immediate(fn_val, args) {
                Ok(v) => v,
                Err(e) => VmValue::str(format!("task gagal: {}", e.msg)),
            };
            let _ = done_clone.send(hasil);
        });

        let mut map = HashMap::new();
        map.insert("_done".to_string(),    buram(done_ch));
        map.insert("id".to_string(),       Int(id as i128));
        Ok(VmValue::struktur("Task", map))
    });

    vm.def("task::join", |a| {
        // Nilai yang diterima DIKEMBALIKAN (versi lama menunggu lalu membuangnya), dan
        // yang bukan Task ditolak dengan tipenya — dulu `dict` untuk Option maupun struct.
        let (c, serah) = kanal_tunggal(a.get(0), "task::join")?;
        if !klaim(&serah) {
            return Err(crate::interpreter::tugas::pesan_sudah_diserahkan("task::join"));
        }
        cek_gagal_vm(c.recv().ok().flatten().unwrap_or(Nil))
    });

    // Ditolak apa pun argumennya, seperti interpreter: menyebut bentuk `(t: Task)` untuk
    // pemanggilan yang salah bentuk menunjuk jalan yang tetap akan ditolak.
    vm.def("task::cancel", |_| Err(alasan_ditolak_dua_backend("task::cancel").unwrap().to_string()));
    vm.def("task::id", |_| Err(alasan_ditolak_dua_backend("task::id").unwrap().to_string()));

    // ── Atomic dan Mutex: pegangan `crate::sinkron`, satu badan untuk dua mesin ──

    vm.def("Atomic::new", |a| match a.get(0) {
        None => Ok(Sinkron(Pegangan::atom(0))),
        Some(Int(n)) => Ok(Sinkron(Pegangan::atom(*n))),
        Some(v) => Err(sinkron::pesan_bukan_int("Atomic::new", &v.type_name_penuh())),
    });

    vm.def("atomic::get", |a| Ok(Int(sinkron::atom_baca(atom(a, "atomic::get")?))));

    vm.def("atomic::set", |a| {
        let sel = atom(a, "atomic::set")?;
        sinkron::atom_tulis(sel, bulat(a, 1, "atomic::set")?);
        Ok(VmValue::Unit)
    });

    vm.def("atomic::add", |a| {
        let sel = atom(a, "atomic::add")?;
        sinkron::atom_tambah(sel, bulat(a, 1, "atomic::add")?, false, "atomic::add").map(Int)
    });

    vm.def("atomic::sub", |a| {
        let sel = atom(a, "atomic::sub")?;
        sinkron::atom_tambah(sel, bulat(a, 1, "atomic::sub")?, true, "atomic::sub").map(Int)
    });

    vm.def("atomic::cas", |a| {
        let sel = atom(a, "atomic::cas")?;
        let (harap, baru) = (bulat(a, 1, "atomic::cas")?, bulat(a, 2, "atomic::cas")?);
        Ok(Bool(sinkron::atom_tukar(sel, harap, baru)))
    });

    vm.def("Mutex::new", |_| Ok(Sinkron(Pegangan::kunci())));

    vm.def("mutex::lock", |a| {
        sinkron::kunci(kunci(a, "mutex::lock")?, "mutex::lock").map(|_| VmValue::Unit)
    });

    vm.def("mutex::unlock", |a| {
        sinkron::lepas(kunci(a, "mutex::unlock")?, "mutex::unlock").map(|_| VmValue::Unit)
    });
}

/// Saluran dalam sebuah Future/Task, disimpan buram (lihat `interpreter::tugas`).
pub(crate) fn buram(ch: VmChannel) -> VmValue { Sinkron(Pegangan::buram(ch)) }

fn saluran(v: &VmValue) -> Option<VmChannel> {
    match v { Sinkron(p) => p.isi_buram::<VmChannel>().cloned(), _ => None }
}

/// Argumen pertama sebagai Atomic, atau kalimat yang menyebut tipenya.
fn atom<'a>(a: &'a [VmValue], siapa: &str) -> Result<&'a Arc<Mutex<i128>>, String> {
    match a.first() {
        Some(Sinkron(Pegangan::Atom(sel))) => Ok(sel),
        v => Err(sinkron::pesan_bukan(siapa, "Atomic",
            &v.map_or_else(|| "nil".to_string(), |v| v.type_name_penuh()))),
    }
}

/// Argumen pertama sebagai Mutex, atau kalimat yang menyebut tipenya.
pub(crate) fn kunci<'a>(a: &'a [VmValue], siapa: &str) -> Result<&'a Arc<sinkron::Kunci>, String> {
    match a.first() {
        Some(Sinkron(Pegangan::Kunci(k))) => Ok(k),
        v => Err(sinkron::pesan_bukan(siapa, "Mutex",
            &v.map_or_else(|| "nil".to_string(), |v| v.type_name_penuh()))),
    }
}

/// Argumen ke-`i` sebagai bilangan bulat; pecahan dan `bool` ditolak, tidak dipotong.
fn bulat(a: &[VmValue], i: usize, siapa: &str) -> Result<i128, String> {
    match a.get(i) {
        Some(Int(n)) => Ok(*n),
        v => Err(sinkron::pesan_bukan_int(siapa,
            &v.map_or_else(|| "nil".to_string(), |v| v.type_name_penuh()))),
    }
}

/// Alasan penolakan untuk nama `task::` yang ditolak **di kedua backend**.
pub fn alasan_ditolak_dua_backend(path: &str) -> Option<&'static str> {
    Some(match path {
        // Nomornya dulu disimpan hanya untuk dibaca kembali oleh `task::id`,
        // dan tidak pernah menunjuk apa pun — bukan tid OS, bukan indeks
        // apa-apa. Menjawab 0 memberi angka yang tampak berarti padahal bukan.
        "task::id" => "`task::id` tidak menyimpan nomor yang berarti apa pun — dulu isinya cuma angka yang dibuat untuk dibaca kembali, bukan id thread dan bukan indeks apa pun. Kalau perlu menandai task, simpan sendiri nomornya di sisi program",

        // Diukur, bukan dikira: sebuah task yang di-`cancel` tetap berjalan sampai
        // selesai dan tetap menulis hasil sampingannya.
        "task::cancel" => "`task::cancel` tidak pernah benar-benar membatalkan: task-nya tetap jalan sampai selesai. Sebuah thread OS tidak bisa dihentikan dari luar dengan aman. Pakai bendera yang dibaca sendiri oleh task-nya — `let stop = Atomic::new(0)`, `atomic::set(stop, 1)` dari luar, dan `if atomic::get(stop) == 1 { return }` di dalam loop task",
        _ => return None,
    })
}

/// Bungkus kegagalan tugas di sisi VM. Bentuknya cermin
/// `interpreter::tugas::nilai_gagal`, dan namanya dibaca dari sana supaya
/// tidak ada dua ejaan untuk satu penanda.
pub fn gagal_vm(pesan: String) -> VmValue {
    let mut m = HashMap::new();
    m.insert("pesan".to_string(), VmValue::str(pesan));
    VmValue::struktur(crate::interpreter::tugas::NAMA_GAGAL, m)
}

/// Ubah penanda kegagalan jadi `Err`; nilai biasa lewat apa adanya.
fn cek_gagal_vm(v: VmValue) -> Result<VmValue, String> {
    if let VmValue::Map(m) = &v {
        if v.nama_struct() == Some(crate::interpreter::tugas::NAMA_GAGAL) {
            let g = m.lock().unwrap_or_else(|p| p.into_inner());
            return Err(match g.get("pesan") {
                Some(VmValue::Str(p)) => p.to_string(),
                _ => "tugas gagal".to_string(),
            });
        }
    }
    Ok(v)
}
