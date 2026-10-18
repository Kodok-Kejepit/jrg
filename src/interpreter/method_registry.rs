// src/interpreter/method_registry.rs — interpreter/method_registry.rs
// Tabel dispatch method per-tipe: `impl Foo { ... }` instance & static

use crate::hash_cepat::PetaCepat;
use super::value::JrgFunction;

#[derive(Debug, Clone, Default)]
pub struct MethodRegistry {
    pub instance: PetaCepat<String, PetaCepat<String, std::sync::Arc<JrgFunction>>>,
    pub static_:  PetaCepat<String, PetaCepat<String, std::sync::Arc<JrgFunction>>>,
}

impl MethodRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn reg_instance(&mut self, ty: &str, method: &str, f: JrgFunction) {
        self.instance.entry(ty.to_string()).or_default()
            .insert(method.to_string(), std::sync::Arc::new(f));
    }
    pub fn reg_static(&mut self, ty: &str, method: &str, f: JrgFunction) {
        self.static_.entry(ty.to_string()).or_default()
            .insert(method.to_string(), std::sync::Arc::new(f));
    }
    pub fn find_instance(&self, ty: &str, method: &str) -> Option<&std::sync::Arc<JrgFunction>> {
        self.instance.get(ty)?.get(method)
    }
    pub fn find_static(&self, ty: &str, method: &str) -> Option<&std::sync::Arc<JrgFunction>> {
        self.static_.get(ty)?.get(method)
    }
}
