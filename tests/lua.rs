// tests/lua.rs — `lua::`, FFI Lua di atas mesin worker bersama. Dilewati kalau `lua`
// tidak ada di PATH (sama seperti tes PHP/Node), karena itu soal mesin, bukan soal JRG.

mod bersama;
use std::process::Command;

fn ada_lua() -> bool {
    Command::new("lua").arg("-v").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Jalankan `src` dari direktori sementara yang juga berisi `berkas` tambahan.
fn jalankan(src: &str, berkas: &[(&str, &str)]) -> (String, String) {
    let dir = bersama::dir_unik("jrg-lua");
    std::fs::create_dir_all(&dir).expect("bikin dir sementara");
    for (nama, isi) in berkas { std::fs::write(dir.join(nama), isi).expect("tulis berkas"); }
    std::fs::write(dir.join("uji.jrg"), src).expect("tulis sumber");
    let out = Command::new(env!("CARGO_BIN_EXE_jrgc"))
        .arg("uji.jrg").arg("--sopan").current_dir(&dir)
        .output().expect("jalankan jrgc");
    let vm = Command::new(env!("CARGO_BIN_EXE_jrgc"))
        .arg("uji.jrg").arg("--sopan").arg("--vm").current_dir(&dir)
        .output().expect("jalankan jrgc --vm");
    let _ = std::fs::remove_dir_all(&dir);
    let gabung = |o: &std::process::Output| format!(
        "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    bersama::dua_mesin_sama(src, &gabung(&out), &gabung(&vm));
    (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn lua_nilai_callback_dan_galat_menjawab_benar() {
    if !ada_lua() { eprintln!("lewati: `lua` tidak ada"); return; }
    let (keluar, galat) = jalankan(r##"
package main
fn ganda(x) { return x * 2 }
pub fn main() {
    wok(lua::eval("1 + 2"))
    wok(lua::eval("{1, 2.5, 'x', true, {a = 1, n = 5}}"))
    lua::exec("function tambah(a, b) return a + b end")
    wok(lua::call("tambah", 40, 2))
    wok(lua::call("string.rep", "ab", 3, "-"))
    lua::exec("function terapkan(f, xs) local r = {} for i, v in ipairs(xs) do r[i] = f(v) end return r end")
    wok(lua::call("terapkan", ganda, [1, 2, 3]))
    wok(lua::call("select", "#", nil, nil, 3))
    wok(lua::eval("error('sengaja')"))
    wok(lua::eval("function() end"))
    wok(lua::call("tambah", 170141183460469231731687303715884105727, 1))
    wok(lua::call("tidak_ada", 1))
    lua::exec("hitung = 0")
    wok(lua::eval("(function() hitung = hitung + 1; return hitung end)()"))
    lua::exec("print('dari lua')")
    wok(lua::eval("2^0.5, 7 // 2, 1.0"))
    wok(lua::require("modul_uji"))
    wok(lua::call("modul_uji.kuadrat", 12))
}
"##, &[("modul_uji.lua", "local M = {}\nfunction M.kuadrat(x) return x * x end\nreturn M\n")]);
    let harap = "Result::Ok(3)\n\
Result::Ok([1, 2.5, x, true, dict {a: 1, n: 5}])\n\
Result::Ok(42)\n\
Result::Ok(ab-ab-ab)\n\
Result::Ok([2, 4, 6])\n\
Result::Ok(3)\n\
Result::Err(lua::eval:1: sengaja)\n";
    assert!(keluar.starts_with(harap), "keluaran awal salah:\n{}\nstderr:\n{}", keluar, galat);
    for kunci in [
        "nilai Lua bertipe `function` tidak bisa dikonversi",
        "tidak muat di integer Lua (64 bit)",
        "`tidak_ada` bukan fungsi Lua yang dikenal",
        // Kode `eval` dijalankan SEKALI: penghitungnya 1, bukan 2.
        "Result::Ok(1)\n",
        "Result::Ok([1.4142135623730951, 3, 1.0])",
        "Result::Ok(144)",
    ] {
        assert!(keluar.contains(kunci), "tidak ada `{}` di keluaran:\n{}", kunci, keluar);
    }
    // `print` milik Lua ke stderr, tidak pernah ke kawat protokol.
    assert!(galat.contains("dari lua"), "print Lua hilang:\n{}", galat);
    assert!(!keluar.contains("dari lua"), "print Lua bocor ke stdout:\n{}", keluar);
}
