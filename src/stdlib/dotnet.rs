// src/stdlib/dotnet.rs — src/stdlib/dotnet.rs — `dotnet::` di kedua backend.

use crate::vm::Vm;
use super::sistem::lewat_hasil;

pub fn register(vm: &mut Vm) {
    vm.def("dotnet::tersedia", |a| lewat_hasil(a, crate::ffi::dotnet::b_tersedia));
    vm.def("dotnet::mulai",    |a| lewat_hasil(a, crate::ffi::dotnet::b_mulai));
    vm.def("dotnet::fungsi",   |a| lewat_hasil(a, crate::ffi::dotnet::b_fungsi));
    vm.def("dotnet::panggil",  |a| lewat_hasil(a, crate::ffi::dotnet::b_panggil));
    vm.def("dotnet::lepas",    |a| lewat_hasil(a, crate::ffi::dotnet::b_lepas));
}
