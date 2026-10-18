//! `lua::` — memanggil Lua dari JRG lewat mesin worker yang sama dengan `py::`, `js::`
//! dan `php::`. Tahap 1 berbasis NILAI: hasil Lua yang tidak bisa dikonversi (fungsi,
//! userdata) ditolak dengan sebabnya, belum dijadikan handle.

use crate::interpreter::{err_value, Value};
use std::sync::{Mutex, OnceLock};

use super::pekerja::{self, Panggil, Runtime, Worker};

fn slot() -> &'static Mutex<Option<Worker>> {
    static SLOT: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

static GENERASI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn antrean_lepas() -> &'static Mutex<Vec<u64>> {
    static Q: OnceLock<Mutex<Vec<u64>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

fn registri_fungsi() -> &'static Mutex<pekerja::RegistriFungsi> {
    static R: OnceLock<Mutex<pekerja::RegistriFungsi>> = OnceLock::new();
    R.get_or_init(Default::default)
}

thread_local! {
    static FUNGSI_BOLEH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DALAM_CALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub static RT: Runtime = Runtime {
    nama: "Lua",
    ns: "lua",
    bin: "lua",
    bin_args: &[],
    skrip_src: include_str!("lua_runner.lua"),
    skrip_nama: "jrg_lua_runner.lua",
    bantuan_pasang: "pastikan `lua` (5.3 ke atas) ada di PATH — di Debian/Ubuntu: apt install \
                     lua5.4; di Termux: pkg install lua54",
    slot,
    generasi: &GENERASI,
    antrean: antrean_lepas,
    registri: registri_fungsi,
    fungsi_boleh_get: || FUNGSI_BOLEH.with(|b| b.get()),
    fungsi_boleh_set: |v| FUNGSI_BOLEH.with(|b| b.set(v)),
    dalam_cb_get: || DALAM_CALLBACK.with(|c| c.get()),
    dalam_cb_set: |v| DALAM_CALLBACK.with(|c| c.set(v)),
    // Runner tahap 1 tidak pernah menjawab dengan `ref`; kalau sampai terjadi, itu bug.
    buat_handle: |_, _, tipe| err_value(format!(
        "worker Lua mengembalikan handle (`{}`), padahal `lua::` belum mendukung handle — \
         ini bug di JRG sendiri", tipe)),
    tulis_handle: |v, _| match v {
        // Integer Lua 64 bit: yang lebih besar akan jadi float yang tidak tepat.
        Value::Int(n) if *n > i64::MAX as i128 || *n < i64::MIN as i128 => Some(Err(format!(
            "memuat bilangan bulat {} yang tidak muat di integer Lua (64 bit) — Lua akan \
             mengubahnya jadi float yang tidak tepat.\n  bantuan: kirim sebagai teks (`str(x)`) \
             kalau angkanya harus utuh, atau `x as float` kalau perkiraan cukup", n))),
        _ => None,
    },
};

fn kirim(panggil: Panggil<'_>, bangun: impl FnOnce(&mut String) -> Result<(), String>) -> Value {
    pekerja::jalankan_dgn_fungsi(&RT, bangun, panggil)
}

fn teks_json(s: &str, out: &mut String) { pekerja::escape_json_string(s, out) }

/// Nilai satu ekspresi Lua (`lua::eval("1 + 2")`); beberapa nilai balik jadi array.
pub fn lua_eval(kode: &str, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"eval\",\"code\":");
        teks_json(kode, req);
        req.push('}');
        Ok(())
    })
}

/// Jalankan pernyataan Lua; global, fungsi dan modulnya bertahan selama worker hidup.
pub fn lua_exec(kode: &str, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"exec\",\"code\":");
        teks_json(kode, req);
        req.push('}');
        Ok(())
    })
}

/// Panggil fungsi Lua lewat nama global atau jalur bertitik (`"string.upper"`).
pub fn lua_call(fungsi: &str, args: &[Value], panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"call\",\"func\":");
        teks_json(fungsi, req);
        req.push_str(",\"args\":[");
        for (i, a) in args.iter().enumerate() {
            if i > 0 { req.push(','); }
            pekerja::value_to_json(&RT, a, req)
                .map_err(|e| format!("{} {}", pekerja::argumen_diteruskan(i, "Lua"), e))?;
        }
        req.push_str("]}");
        Ok(())
    })
}

/// `require(nama)`, lalu modulnya dipasang sebagai global bernama segmen terakhirnya.
pub fn lua_require(nama: &str, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"require\",\"name\":");
        teks_json(nama, req);
        req.push('}');
        Ok(())
    })
}

/// Apakah worker Lua bisa dijalankan di mesin ini.
pub fn lua_tersedia(panggil: Panggil<'_>) -> Value {
    let jawab = kirim(panggil, |req| { req.push_str("{\"op\":\"ping\"}"); Ok(()) });
    Value::Bool(matches!(jawab, Value::Enum { ref variant, .. } if variant == "Ok"))
}
