// src/stdlib/jvm.rs — src/stdlib/jvm.rs — `jvm::` di kedua backend.

use crate::vm::Vm;
use super::sistem::lewat_hasil;

pub fn register(vm: &mut Vm) {
    vm.def("jvm::tersedia", |a| lewat_hasil(a, crate::ffi::jvm::b_tersedia));
    vm.def("jvm::mulai", |a| lewat_hasil(a, crate::ffi::jvm::b_mulai));
    vm.def("jvm::versi", |a| lewat_hasil(a, crate::ffi::jvm::b_versi));
    vm.def("jvm::panggil_statis", |a| lewat_hasil(a, crate::ffi::jvm::b_panggil_statis));
    // Objek + metode instance + field: ini yang membuat `object`/`companion`
    // Kotlin dan kelas Java biasa terjangkau, bukan cuma metode statis.
    vm.def("jvm::baru", |a| lewat_hasil(a, crate::ffi::jvm::b_baru));
    vm.def("jvm::panggil", |a| lewat_hasil(a, crate::ffi::jvm::b_panggil));
    vm.def("jvm::field_statis", |a| lewat_hasil(a, crate::ffi::jvm::b_field_statis));
    vm.def("jvm::field", |a| lewat_hasil(a, crate::ffi::jvm::b_field));
    vm.def("jvm::ke_teks", |a| lewat_hasil(a, crate::ffi::jvm::b_ke_teks));
    vm.def("jvm::lepas", |a| lewat_hasil(a, crate::ffi::jvm::b_lepas));
    vm.def("jvm::objek_hidup", |a| lewat_hasil(a, crate::ffi::jvm::b_objek_hidup));
}
