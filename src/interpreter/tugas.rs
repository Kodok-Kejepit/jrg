// src/interpreter/tugas.rs — src/interpreter/tugas.rs — `Future::` dan `Task::` di interpreter.

use std::sync::Arc;

use super::{Interpreter, Value};
use crate::interpreter::value::{
    channel_ada_isi, channel_coba_ambil, channel_new, channel_recv, channel_send,
};

/// Nama tipe yang dibuat modul ini.
pub const NAMA_GAGAL: &str = "__tugas_gagal__";

pub const NAMA_FUTURE: &str = "Future";
pub const NAMA_TASK: &str = "Task";

/// Bungkus sebuah channel jadi nilai `Future`/`Task`.
pub fn nilai_gagal(pesan: String) -> Value {
    let mut f = std::collections::HashMap::new();
    f.insert("pesan".to_string(), Value::Str(pesan));
    Value::Struct { name: NAMA_GAGAL.to_string(), fields: f.into() }
}

/// Ubah penanda kegagalan jadi `Err`; nilai biasa lewat apa adanya.
pub fn cek_gagal(v: Value) -> Result<Value, String> {
    if let Value::Struct { name, fields } = &v {
        if name == NAMA_GAGAL {
            return Err(match fields.get("pesan") {
                Some(Value::Str(p)) => p.to_string(),
                _ => "tugas gagal".to_string(),
            });
        }
    }
    Ok(v)
}

/// Nama field penanda "hasilnya sudah diserahkan".
pub const FIELD_SERAH: &str = "_serah";

/// Saluran dalam sebuah Future/Task disimpan BURAM: `<-t._ch` dulu mencuri hasilnya
/// dan membuat `task::join` menunggu selamanya.
fn buram(ch: crate::interpreter::value::ChannelHandle) -> Value {
    Value::Sinkron(crate::sinkron::Pegangan::buram(ch))
}

fn saluran(v: &Value) -> Option<crate::interpreter::value::ChannelHandle> {
    match v {
        Value::Sinkron(p) => p.isi_buram::<crate::interpreter::value::ChannelHandle>().cloned(),
        _ => None,
    }
}

fn bungkus(nama: &str, ch: crate::interpreter::value::ChannelHandle) -> Value {
    let mut f = std::collections::HashMap::new();
    f.insert("_ch".to_string(), buram(ch));
    // Token diisi SEKARANG, bukan saat hasilnya datang: sebuah future yang
    // belum selesai pun sudah punya hak-ambil yang belum terpakai.
    let serah = channel_new(1);
    let _ = channel_send(&serah, Value::Unit);
    f.insert(FIELD_SERAH.to_string(), buram(serah));
    Value::Struct { name: nama.to_string(), fields: f.into() }
}

/// Ambil kedua channel sebuah future/task: hasil, dan token serah.
fn kanal(v: &Value, siapa: &str)
    -> Result<(crate::interpreter::value::ChannelHandle,
               Option<crate::interpreter::value::ChannelHandle>), String>
{
    let ch = ambil_channel(v, siapa)?;
    let serah = match v {
        Value::Struct { fields, .. } => fields.get(FIELD_SERAH).and_then(saluran),
        _ => None,
    };
    Ok((ch, serah))
}

/// Ambil hak-serah. `false` berarti hasilnya sudah diserahkan sebelumnya.
fn klaim(serah: &Option<crate::interpreter::value::ChannelHandle>) -> bool {
    match serah {
        Some(c) => channel_coba_ambil(c).is_some(),
        // Future dari versi lama tanpa field ini: jangan menolak apa pun.
        None => true,
    }
}

/// Apakah hak-serah masih ada, TANPA mengambilnya.
fn masih_boleh(serah: &Option<crate::interpreter::value::ChannelHandle>) -> bool {
    match serah {
        Some(c) => channel_ada_isi(c),
        None => true,
    }
}

pub fn pesan_sudah_diserahkan(siapa: &str) -> String {
    format!(
        "{}: hasilnya sudah diambil sebelumnya — sebuah Future/Task menyerahkan \
         hasilnya SEKALI. Simpan nilainya di sebuah variabel kalau perlu dipakai \
         berkali-kali, atau bikin tugas baru",
        siapa
    )
}

/// Kalimat "ini bukan Future/Task", dibaca KEDUA backend.
pub fn pesan_bukan_tugas(siapa: &str, dapat: &str) -> String {
    format!("{}: harus `Future`/`Task`, dapat `{}`", siapa, dapat)
}

fn ambil_channel(v: &Value, siapa: &str) -> Result<crate::interpreter::value::ChannelHandle, String> {
    match v {
        Value::Struct { name, fields } if name == NAMA_FUTURE || name == NAMA_TASK => {
            fields.get("_ch").and_then(saluran)
                .ok_or_else(|| format!("{}: `{}` rusak — field `_ch` hilang", siapa, name))
        }
        lain => Err(pesan_bukan_tugas(siapa, &lain.type_name())),
    }
}

impl Interpreter {
    /// `Future::new(f, args...)` / `Task::spawn(f, args...)`.
    pub(crate) fn mulai_tugas(&mut self, nama_tipe: &str, args: Vec<Value>) -> Result<Value, String> {
        let nama = if nama_tipe == "Task" { "Task::spawn" } else { "Future::new" };
        let Some(f) = args.first().cloned() else {
            return Err(format!("{} butuh minimal 1 argumen: fungsinya", nama));
        };
        if !f.bisa_dipanggil() {
            return Err(super::pesan_tugas_bukan_fungsi(nama, &f.type_name()));
        }
        let sisa = args[1..].to_vec();
        // Dimiliki sendiri: closure yang menyeberang ke thread lain harus
        // `'static`, jadi ia tidak boleh memegang `&str` dari pemanggil.
        let label = nama_tipe.to_string();

        // Kapasitas 1: hasilnya dikirim tepat sekali, dan pengirimnya tidak boleh
        // menunggu ada yang membaca — sebuah future yang tidak pernah di-`await` …
        let ch = channel_new(1);
        let ch_kirim = ch.clone();

        // Interpreter DI-CLONE, sama seperti `spawn_goroutine`: closure yang
        // menyeberang ke thread lain harus `'static`, jadi ia tidak boleh
        // meminjam apa pun dari frame ini.
        let mut interp = self.clone();
        // Frame `defer` sendiri, dengan alasan yang sama seperti goroutine:
        // tanpa ini `defer` di dalam fungsi yang dijalankan future menumpuk
        // ke frame bekas clone-an dan tidak pernah jalan.
        interp.defer_frames.push(Vec::new());

        let hasil = std::thread::Builder::new()
            .name("jrg-future".to_string())
            .spawn(move || {
                // Kegagalan IKUT dibawa, tidak ditelan.
                let v = match interp.call_fn(f, sisa) {
                    Ok(v) => v,
                    // Sebuah `return` di badan fungsi sampai ke sini sebagai
                    // sinyal, bukan error — nilainya itulah hasilnya.
                    Err(super::value::Signal::Return(v)) => v,
                    // Pesannya diambil dari `RuntimeError` langsung, bukan lewat `Display` milik
                    // `Signal` — yang menyisipkan "runtime error: " dan membuat kalimatnya …
                    Err(super::value::Signal::RuntimeError(re)) => {
                        nilai_gagal(format!("{} gagal: {}", label, re.message))
                    }
                    Err(e) => nilai_gagal(format!("{} gagal: {}", label, e)),
                };
                let _ = channel_send(&ch_kirim, v);
            });

        if let Err(e) = hasil {
            return Err(crate::host::tanpa_thread(&format!("`{}::new`", nama_tipe), &e)
                .unwrap_or_else(|| format!("{}::new: tidak bisa membuat thread: {}", nama_tipe, e)));
        }
        Ok(bungkus(nama_tipe, ch))
    }

    /// `future::await(f)` / `task::join(t)` — tunggu, lalu kembalikan hasilnya.
    pub(crate) fn tunggu_tugas(&mut self, args: &[Value], siapa: &str) -> Result<Value, String> {
        let Some(v) = args.first() else {
            return Err(format!("{} butuh 1 argumen", siapa));
        };
        let (ch, serah) = kanal(v, siapa)?;
        // Hak-ambil DIKLAIM sebelum menunggu.
        if !klaim(&serah) {
            return Err(pesan_sudah_diserahkan(siapa));
        }
        let hasil = channel_recv(&ch).map_err(|e| format!("{}: {}", siapa, e))?;
        cek_gagal(hasil)
    }

    /// Kumpulkan channel dari sebuah array future, atau tolak menyebut elemen
    /// mana yang bukan future.
    #[allow(clippy::type_complexity)]
    fn channel_dari_array(
        v: Option<&Value>,
        siapa: &str,
    ) -> Result<Vec<(crate::interpreter::value::ChannelHandle,
                     Option<crate::interpreter::value::ChannelHandle>)>, String> {
        let Some(Value::Array(arr)) = v else {
            return Err(format!(
                "{}: argumen 1 harus array berisi Future/Task, dapat `{}`",
                siapa,
                v.map(|x| x.type_name()).unwrap_or_else(|| "tidak ada".to_string())
            ));
        };
        let mut out = Vec::with_capacity(arr.len());
        for (i, f) in arr.iter().enumerate() {
            out.push(kanal(f, &format!("{}: elemen ke-{}", siapa, i))?);
        }
        Ok(out)
    }

    /// `future::all(fs)` — tunggu SEMUA, hasilnya urut sesuai masukannya.
    pub(crate) fn tugas_semua(&mut self, args: &[Value], siapa: &str) -> Result<Value, String> {
        let chans = Self::channel_dari_array(args.first(), siapa)?;
        let mut hasil = Vec::with_capacity(chans.len());
        for (i, (ch, serah)) in chans.iter().enumerate() {
            if !klaim(serah) {
                return Err(pesan_sudah_diserahkan(&format!("{}: elemen ke-{}", siapa, i)));
            }
            let v = channel_recv(ch).map_err(|e| format!("{}: {}", siapa, e))?;
            hasil.push(cek_gagal(v)?);
        }
        Ok(Value::Array(hasil.into()))
    }

    /// `future::race(fs)` — nilai pertama yang siap.
    pub(crate) fn tugas_balapan(&mut self, args: &[Value], siapa: &str) -> Result<Value, String> {
        let chans = Self::channel_dari_array(args.first(), siapa)?;
        if chans.is_empty() {
            return Err(format!(
                "{}: arraynya kosong — tidak ada yang bisa dibalap, dan menunggu \
                 tidak akan pernah menghasilkan apa pun",
                siapa
            ));
        }
        // Diperiksa SEKALI di depan, bukan tiap putaran: sebuah future yang
        // hasilnya sudah diambil tidak akan pernah menang, jadi menunggunya
        // hanya menunda pesan yang sudah pasti.
        for (i, (_, serah)) in chans.iter().enumerate() {
            if !masih_boleh(serah) {
                return Err(pesan_sudah_diserahkan(&format!("{}: elemen ke-{}", siapa, i)));
            }
        }
        loop {
            for (ch, serah) in &chans {
                if let Some(v) = channel_coba_ambil(ch) {
                    klaim(serah);
                    return cek_gagal(v);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// `future::timeout(f, ms)` — `Some(hasil)`, atau `None` kalau lewat waktu.
    pub(crate) fn tugas_batas_waktu(&mut self, args: &[Value], siapa: &str) -> Result<Value, String> {
        let (ch, serah) = match args.first() {
            Some(v) => kanal(v, siapa)?,
            // Menyulih `Value::Nil` di sini akan melaporkan "dapat `nil`"
            // untuk pemanggilan yang sama sekali TIDAK memberi argumen — dua
            // kesalahan berbeda dengan satu kalimat.
            None => return Err(pesan_bukan_tugas(siapa, "tidak ada")),
        };
        let ms = match args.get(1) {
            Some(Value::Int(n)) if *n >= 0 => *n,
            lain => {
                return Err(format!(
                    "{}: argumen 2 harus int >= 0 (milidetik), dapat `{}`",
                    siapa,
                    lain.map(|x| x.type_name()).unwrap_or_else(|| "tidak ada".to_string())
                ))
            }
        };
        // `None` harus berarti SATU hal: "belum siap".
        if !masih_boleh(&serah) {
            return Err(pesan_sudah_diserahkan(siapa));
        }
        let batas = batas_tunggu(ms);
        loop {
            if let Some(v) = channel_coba_ambil(&ch) {
                klaim(&serah);
                return Ok(crate::interpreter::some_value(cek_gagal(v)?));
            }
            if lewat(batas) {
                return Ok(crate::interpreter::none_value());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// `future::try_get(f)` — `Some(hasil)` kalau sudah siap, `None` kalau belum.
    pub(crate) fn tugas_coba_ambil(&mut self, args: &[Value], siapa: &str) -> Result<Value, String> {
        let (ch, serah) = match args.first() {
            Some(v) => kanal(v, siapa)?,
            // Menyulih `Value::Nil` di sini akan melaporkan "dapat `nil`"
            // untuk pemanggilan yang sama sekali TIDAK memberi argumen — dua
            // kesalahan berbeda dengan satu kalimat.
            None => return Err(pesan_bukan_tugas(siapa, "tidak ada")),
        };
        if !masih_boleh(&serah) {
            return Err(pesan_sudah_diserahkan(siapa));
        }
        Ok(match channel_coba_ambil(&ch) {
            Some(v) => {
                klaim(&serah);
                crate::interpreter::some_value(cek_gagal(v)?)
            }
            None => crate::interpreter::none_value(),
        })
    }

    /// `future::is_done(f)` — sudah ada hasilnya di channel?
    pub(crate) fn tugas_selesai(&mut self, args: &[Value], siapa: &str) -> Result<Value, String> {
        let Some(v) = args.first() else {
            return Err(format!("{} butuh 1 argumen", siapa));
        };
        let (ch, serah) = kanal(v, siapa)?;
        // "Selesai" = hasilnya sudah ada, ATAU sudah diserahkan.
        Ok(Value::Bool(channel_ada_isi(&ch) || !masih_boleh(&serah)))
    }
}

/// Tenggat `ms` milidetik dari sekarang; `None` = tidak akan pernah lewat (dulu `as u64` memotong
/// 2^64 jadi 0 ms, dan penjumlahan `Instant` meluap).
pub fn batas_tunggu(ms: i128) -> Option<std::time::Instant> {
    let ms = u64::try_from(ms).unwrap_or(u64::MAX);
    std::time::Instant::now().checked_add(std::time::Duration::from_millis(ms))
}

pub fn lewat(batas: Option<std::time::Instant>) -> bool {
    batas.map_or(false, |b| std::time::Instant::now() >= b)
}
