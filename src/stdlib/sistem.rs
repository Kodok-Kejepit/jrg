// src/stdlib/sistem.rs
// Namespace `time::`, `env::`, `rand::`, dan `file::` untuk backend VM.

use crate::interpreter::builtin::NativeFnKind;
use crate::interpreter::{Interpreter, Signal, Value};
use crate::vm::ffi_jembatan::TitipanVm;
use crate::vm::value::VmValue;
use crate::vm::vm::Vm;

/// Jalankan dispatch bawaan interpreter dengan argumen dari VM.
pub(crate) fn lewat(
    args: &[VmValue],
    f: impl FnOnce(&[Value]) -> Result<Value, Signal>,
) -> Result<VmValue, String> {
    let mut titipan = TitipanVm::default();
    match terjemahkan(args, &mut titipan, f)? {
        Ok(v) => titipan.ke_vm(&v),
        // Kegagalan yang WAJAR sudah berupa `Result::Err` (sebuah nilai);
        // `Signal` di sini berarti error runtime yang keras, dan pesannya
        // diteruskan apa adanya supaya sama dengan interpreter.
        Err(Signal::RuntimeError(e)) => Err(e.message),
        Err(lain) => Err(format!("sinyal tak terduga dari builtin: {:?}", lain)),
    }
}

/// Varian `lewat` untuk fungsi yang errornya sudah berupa `String`
/// (gaya `dok::`), bukan `Signal`.
pub(crate) fn lewat_hasil(
    args: &[VmValue],
    f: impl FnOnce(&[Value]) -> Result<Value, String>,
) -> Result<VmValue, String> {
    let mut titipan = TitipanVm::default();
    let hasil = terjemahkan(args, &mut titipan, f)??;
    titipan.ke_vm(&hasil)
}

/// Argumen diterjemahkan ke larik di STACK sampai empat buah (`math::` dipanggil di loop
/// terpanas; alokasi per panggilan terukur sebagai harga terbesar jembatan ini). Fungsi
/// menyeberang sebagai penanda (`TitipanVm`), jadi badannya yang memutuskan nasibnya.
fn terjemahkan<R>(
    args: &[VmValue],
    titipan: &mut TitipanVm,
    f: impl FnOnce(&[Value]) -> R,
) -> Result<R, String> {
    const KECIL: usize = 4;
    if args.len() <= KECIL {
        let mut buf: [Value; KECIL] = [Value::Nil, Value::Nil, Value::Nil, Value::Nil];
        for (i, a) in args.iter().enumerate() {
            buf[i] = titipan.ke_interp(a)?;
        }
        return Ok(f(&buf[..args.len()]));
    }
    let mut nilai = Vec::with_capacity(args.len());
    for a in args {
        nilai.push(titipan.ke_interp(a)?);
    }
    Ok(f(&nilai))
}

pub fn register(vm: &mut Vm) {

    // ── time:: ──
    vm.def("time::now", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeNow, v)));
    // `now_ms` tinggal di dispatch UTAMA, bukan di `time_ext`, jadi ditulis
    // langsung — memakai sumber waktu yang sama (`host::sejak_epoch`).
    vm.def("time::now_ms", |_| {
        let ms = crate::host::sejak_epoch()
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Ok(VmValue::Int(ms as i128))
    });
    vm.def("time::year", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeYear, v)));
    vm.def("time::month", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeMonth, v)));
    vm.def("time::day", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeDay, v)));
    vm.def("time::hour", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeHour, v)));
    vm.def("time::minute", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeMinute, v)));
    vm.def("time::second", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeSecond, v)));
    vm.def("time::weekday", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeWeekday, v)));
    vm.def("time::format", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeFormat, v)));
    // `sleep` adalah ALIAS: satu fungsi bernama berarti satu alamat, jadi dua mesin
    // mengelompokkan kedua nama itu persis sama (lihat `vm::info_native`).
    fn tidur(a: &[VmValue]) -> Result<VmValue, String> {
        lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeSleep, v))
    }
    vm.def("time::sleep", tidur);
    vm.def("sleep", tidur);
    vm.def("time::sleep_ms", |a| lewat(a, |v| Interpreter::dispatch_time_ext_inner(&NativeFnKind::TimeSleepMs, v)));

    // ── env:: ──
    vm.def("env::get", |a| lewat(a, |v| Interpreter::dispatch_sysenv_inner(&NativeFnKind::EnvGet, v)));
    vm.def("env::get_or", |a| lewat(a, |v| Interpreter::dispatch_sysenv_inner(&NativeFnKind::EnvGetOr, v)));
    vm.def("env::set", |a| lewat(a, |v| Interpreter::dispatch_sysenv_inner(&NativeFnKind::EnvSet, v)));
    vm.def("env::args", |a| lewat(a, |v| Interpreter::dispatch_sysenv_inner(&NativeFnKind::EnvArgs, v)));
    vm.def("env::exe", |a| lewat(a, |v| Interpreter::dispatch_sysenv_inner(&NativeFnKind::EnvExe, v)));

    // ── rand:: ──
    vm.def("rand::int", |a| lewat(a, |v| Interpreter::dispatch_rand_inner(&NativeFnKind::RandInt, v)));
    vm.def("rand::float", |a| lewat(a, |v| Interpreter::dispatch_rand_inner(&NativeFnKind::RandFloat, v)));
    vm.def("rand::bool", |a| lewat(a, |v| Interpreter::dispatch_rand_inner(&NativeFnKind::RandBool, v)));
    vm.def("rand::choice", |a| lewat(a, |v| Interpreter::dispatch_rand_inner(&NativeFnKind::RandChoice, v)));
    vm.def("rand::shuffle", |a| lewat(a, |v| Interpreter::dispatch_rand_inner(&NativeFnKind::RandShuffle, v)));
    vm.def("rand::sample", |a| lewat(a, |v| Interpreter::dispatch_rand_inner(&NativeFnKind::RandSample, v)));

    // ── crypto:: (sisa yang belum ada di VM) ────────────────────────────
    vm.def("crypto::hex_encode", |a| lewat_hasil(a, crate::dok::crypto::f_hex_encode));
    vm.def("crypto::hex_decode", |a| lewat_hasil(a, crate::dok::crypto::f_hex_decode));
    vm.def("crypto::base64_encode", |a| lewat_hasil(a, crate::dok::crypto::f_base64_encode));
    vm.def("crypto::base64_decode", |a| lewat_hasil(a, crate::dok::crypto::f_base64_decode));
    vm.def("crypto::aes_decrypt_bytes", |a| lewat_hasil(a, crate::dok::crypto::f_aes_decrypt_bytes));
    vm.def("crypto::base64_decode_bytes", |a| lewat_hasil(a, crate::dok::crypto::f_base64_decode_bytes));
    vm.def("crypto::hex_decode_bytes", |a| lewat_hasil(a, crate::dok::crypto::f_hex_decode_bytes));
    vm.def("crypto::random_hex", |a| lewat_hasil(a, crate::dok::crypto::f_random_hex));
    vm.def("crypto::random_int", |a| lewat_hasil(a, crate::dok::crypto::f_random_int));

    // ── file:: ──
    vm.def("file::read", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileRead, v)));
    vm.def("file::write", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileWrite, v)));
    vm.def("file::append", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileAppend, v)));
    vm.def("file::exists", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileExists, v)));
    vm.def("file::delete", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileDelete, v)));
    vm.def("file::read_lines", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileReadLines, v)));
    vm.def("file::list_dir", |a| lewat(a, |v| Interpreter::dispatch_file_inner(&NativeFnKind::FileListDir, v)));
}
