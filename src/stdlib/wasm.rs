// src/stdlib/wasm.rs — src/stdlib/wasm.rs — `wasm::` di kedua backend.

use crate::vm::Vm;
use super::sistem::lewat_hasil;

pub fn register(vm: &mut Vm) {
    vm.def("wasm::muat",         |a| lewat_hasil(a, crate::ffi::wasm::b_muat));
    vm.def("wasm::ekspor",       |a| lewat_hasil(a, crate::ffi::wasm::b_ekspor));
    vm.def("wasm::panggil",      |a| lewat_hasil(a, crate::ffi::wasm::b_panggil));
    vm.def("wasm::baca_memori",  |a| lewat_hasil(a, crate::ffi::wasm::b_baca_memori));
    vm.def("wasm::baca_memori_bytes", |a| lewat_hasil(a, crate::ffi::wasm::b_baca_memori_bytes));
    vm.def("wasm::tulis_memori", |a| lewat_hasil(a, crate::ffi::wasm::b_tulis_memori));
    vm.def("wasm::tutup",        |a| lewat_hasil(a, crate::ffi::wasm::b_tutup));
}
