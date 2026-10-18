// src/stdlib/kotlin.rs
// Pendaftaran `kotlin::`. Semua lewat mesin JNI yang sama dengan `jvm::`.

use crate::stdlib::sistem::lewat_hasil;
use crate::vm::Vm;

pub fn daftar(vm: &mut Vm) {
    vm.def("kotlin::tersedia", |a| lewat_hasil(a, crate::ffi::kotlin::b_tersedia));
    vm.def("kotlin::fungsi", |a| lewat_hasil(a, crate::ffi::kotlin::b_fungsi));
    vm.def("kotlin::objek", |a| lewat_hasil(a, crate::ffi::kotlin::b_objek));
    vm.def("kotlin::companion", |a| lewat_hasil(a, crate::ffi::kotlin::b_companion));
    vm.def("kotlin::properti", |a| lewat_hasil(a, crate::ffi::kotlin::b_properti));
}
