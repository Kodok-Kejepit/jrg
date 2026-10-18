// src/interpreter/env.rs — interpreter/env.rs
// `Env` — lingkungan (scope) variabel: stack of scopes + tabel global

use std::collections::HashMap;
use crate::hash_cepat::PetaCepat;
use crate::parser::Pattern;
use super::value::Value;

/// Satu binding lokal.
#[derive(Debug, Clone)]
pub enum Binding {
    Plain(Value),
    Shared(std::sync::Arc<std::sync::Mutex<Value>>),
}

impl Binding {
    /// Nilai binding ini, apa pun bentuknya.
    pub fn get(&self) -> Value {
        match self {
            Binding::Plain(v) => v.clone(),
            Binding::Shared(cell) => cell.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        }
    }

    /// Tulis nilai baru ke binding ini.
    pub fn set(&mut self, v: Value) {
        match self {
            Binding::Plain(slot) => *slot = v,
            Binding::Shared(cell) => *cell.lock().unwrap_or_else(|p| p.into_inner()) = v,
        }
    }
}

/// Awalan kunci penanda `var` tingkat modul di dalam `globals`. Karakter kendali di
/// depannya tidak bisa ditulis di nama mana pun, jadi tidak pernah bertabrakan.
const AWALAN_VAR: &str = "\u{1}var:";

/// Tabel global satu program, dibagi antar-`Env` (dan antar-goroutine) lewat `Arc`.
pub type Globals = std::sync::Arc<std::sync::Mutex<PetaCepat<String, Value>>>;

#[derive(Debug, Clone)]
pub struct Env {
    pub globals: Globals,
    pub scopes: Vec<PetaCepat<String, Binding>>,
}

impl Env {
    pub fn new() -> Self {
        let mut e = Env { globals: std::sync::Arc::new(std::sync::Mutex::new(PetaCepat::default())), scopes: Vec::new() };
        e.register_builtins();
        e
    }

    /// Env baru yang BERBAGI `globals` yang sama (Arc::clone, O(1)) tapi mulai
    /// dengan scope lokal kosong — dipakai di titik-titik yang dulu melakukan …
    pub fn with_globals(globals: Globals) -> Self {
        Env { globals, scopes: Vec::new() }
    }


    pub fn set_global(&mut self, name: &str, val: Value) {
        self.globals.lock().unwrap_or_else(|p| p.into_inner()).insert(name.to_string(), val);
    }
    pub fn push_scope(&mut self) { self.scopes.push(PetaCepat::default()); }
    pub fn pop_scope(&mut self)  { self.scopes.pop(); }
    pub fn define(&mut self, name: &str, val: Value) {
        // Kalau tidak ada scope lokal aktif (mis. dipanggil dari kode yang belum
        // sempat `push_scope`), definisikan ke `globals` alih-alih panik — situasi …
        match self.scopes.last_mut() {
            Some(scope) => { scope.insert(name.to_string(), Binding::Plain(val)); }
            None => { self.set_global(name, val); }
        }
    }

    /// Seperti `define`, tapi kalau nama itu sudah ada di scope TERATAS, binding-nya
    /// DIGANTI (bukan ditulis menembus sel yang mungkin sudah ditangkap closure).
    pub fn define_ulang(&mut self, name: &str, val: Value) {
        if let Some(b) = self.scopes.last_mut().and_then(|s| s.get_mut(name)) {
            *b = Binding::Plain(val);
        } else {
            self.define(name, val);
        }
    }

    /// Pasang binding yang SUDAH berupa sel bersama — dipakai waktu
    /// memasang hasil tangkapan closure ke env pemanggilan.
    pub fn define_shared(&mut self, name: &str, cell: std::sync::Arc<std::sync::Mutex<Value>>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), Binding::Shared(cell));
        }
    }

    /// Pinjam isi binding `name` untuk DIUBAH DI TEMPAT.
    pub fn with_binding_mut<R>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Value) -> R,
    ) -> Option<R> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                return Some(match b {
                    Binding::Plain(v) => f(v),
                    Binding::Shared(cell) => {
                        let mut g = cell.lock().unwrap_or_else(|p| p.into_inner());
                        f(&mut g)
                    }
                });
            }
        }
        // Binding global: `globals` memegang `Value` langsung, jadi bisa
        // diubah di tempat juga.
        let mut g = self.globals.lock().unwrap_or_else(|p| p.into_inner());
        g.get_mut(name).map(f)
    }

    /// Apakah binding terdekat `name` lokal dan polos (tidak ditangkap closure mana pun).
    pub fn lokal_polos(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) { return matches!(b, Binding::Plain(_)); }
        }
        false
    }

    /// Apakah binding LOKAL terdekat bernama `name` adalah array atau string polos —
    /// syarat jalur tumbuh-di-tempat, diperiksa tanpa menyalin nilainya.
    pub fn lokal_polos_koleksi(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                return matches!(b, Binding::Plain(Value::Array(_) | Value::Str(_)));
            }
        }
        false
    }

    /// Isi array milik binding yang BUKAN lokal polos (`var` modul, atau lokal yang ditangkap
    /// closure): pemegang kedua yang O(1), untuk jalur tumbuh yang memeriksa penyimpanannya.
    pub fn array_bukan_lokal_polos(&self, name: &str) -> Option<super::value::Isi> {
        for scope in self.scopes.iter().rev() {
            match scope.get(name) {
                Some(Binding::Plain(_)) => return None,
                Some(Binding::Shared(cell)) => {
                    return match &*cell.lock().unwrap_or_else(|p| p.into_inner()) {
                        Value::Array(a) => Some(a.clone()),
                        _ => None,
                    };
                }
                None => {}
            }
        }
        match self.globals.lock().unwrap_or_else(|p| p.into_inner()).get(name) {
            Some(Value::Array(a)) => Some(a.clone()),
            _ => None,
        }
    }

    /// Nama tipe struct yang dipegang binding LOKAL polos `name`, tanpa menyalin nilainya.
    pub fn lokal_polos_struct(&self, name: &str) -> Option<&str> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                return match b {
                    Binding::Plain(Value::Struct { name: t, .. }) if t != "dict" => Some(t.as_str()),
                    _ => None,
                };
            }
        }
        None
    }

    /// Ambil nilai binding LOKAL `name` keluar, meninggalkan lubang sementara —
    /// dipakai optimasi keunikan untuk pola menumbuhkan koleksi (`a = a + [x]` …
    pub fn ambil_lokal_polos(&mut self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                return match b {
                    Binding::Plain(v) => Some(std::mem::replace(v, Value::Nil)),
                    Binding::Shared(_) => None,
                };
            }
        }
        None
    }

    /// Ambil sel bersama untuk `name`, mempromosikan binding-nya dari `Plain`
    /// jadi `Shared` kalau perlu.
    pub fn capture_salinan(&mut self, name: &str) -> Option<std::sync::Arc<std::sync::Mutex<Value>>> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                let nilai = match b {
                    Binding::Shared(cell) => cell.lock().unwrap_or_else(|p| p.into_inner()).clone(),
                    Binding::Plain(v) => v.clone(),
                };
                return Some(std::sync::Arc::new(std::sync::Mutex::new(nilai)));
            }
        }
        // Global tidak ditangkap by-value: ia memang bukan binding lokal, dan
        // closure biasa pun membacanya lewat `globals`.
        None
    }

    pub fn capture_cell(&mut self, name: &str) -> Option<std::sync::Arc<std::sync::Mutex<Value>>> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                let cell = match b {
                    Binding::Shared(cell) => std::sync::Arc::clone(cell),
                    Binding::Plain(v) => {
                        let cell = std::sync::Arc::new(std::sync::Mutex::new(v.clone()));
                        *b = Binding::Shared(std::sync::Arc::clone(&cell));
                        cell
                    }
                };
                return Some(cell);
            }
        }
        None
    }
    /// Cari `name` dari scope lokal (innermost dulu), baru fallback ke `globals`.
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) { return Some(b.get()); }
        }
        self.globals.lock().unwrap_or_else(|p| p.into_inner()).get(name).cloned()
    }
    /// Bawaan bahasa bernama `name`, tanpa peduli apa yang dideklarasikan program — tabelnya
    /// dibangun sekali per proses dari `Env` yang belum disentuh siapa pun.
    pub fn bawaan_murni(name: &str) -> Option<Value> {
        static TABEL: std::sync::OnceLock<HashMap<String, Value>> = std::sync::OnceLock::new();
        TABEL.get_or_init(|| {
            let e = Env::new();
            let g = e.globals.lock().unwrap_or_else(|p| p.into_inner());
            // `Option`/`Result` terdaftar sebagai `nil` penanda nama tipe, bukan nilai.
            g.iter().filter(|(_, v)| !matches!(v, Value::Nil))
                .map(|(k, v)| (k.clone(), v.clone())).collect()
        }).get(name).cloned()
    }

    /// Kalimat untuk penugasan ke `name` yang bukan variabel di sini: belum dideklarasikan,
    /// atau ada tapi berupa fungsi/nilai tetap.
    pub fn pesan_sasaran_tugas(&self, name: &str) -> String {
        match self.get(name) {
            Some(v) => crate::interpreter::pesan_tugas_bukan_variabel(name, v.bisa_dipanggil()),
            None => crate::interpreter::pesan_tugas_tanpa_deklarasi(
                name, self.saran_variabel(name).as_deref()),
        }
    }

    /// Tandai `name` sebagai `var` tingkat modul — satu-satunya GLOBAL yang boleh ditugasi.
    /// Dicatat di dalam `globals` (kunci ber-awalan yang tak bisa ditulis sebagai nama),
    /// supaya tiap lingkungan yang berbagi `globals` ikut melihatnya.
    pub fn tandai_var(&mut self, name: &str) {
        self.set_global(&format!("{}{}", AWALAN_VAR, name), Value::Unit);
    }

    fn var_modul(&self, name: &str) -> bool {
        self.globals.lock().unwrap_or_else(|p| p.into_inner())
            .contains_key(&format!("{}{}", AWALAN_VAR, name))
    }

    /// Nilai sebuah VARIABEL: binding lokal, atau `var` tingkat modul. Builtin dan fungsi
    /// bukan variabel, jadi `len += 1` tanpa `var len` tidak membaca builtin-nya.
    pub fn get_variabel(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) { return Some(b.get()); }
        }
        if !self.var_modul(name) { return None; }
        self.globals.lock().unwrap_or_else(|p| p.into_inner()).get(name).cloned()
    }

    /// Tugasi variabel yang SUDAH dideklarasikan. `false`: nama itu bukan variabel di
    /// sini — pemanggilnya menolak, bukan mendefinisikannya diam-diam.
    pub fn set_variabel(&mut self, name: &str, val: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                b.set(val);
                return true;
            }
        }
        if !self.var_modul(name) { return false; }
        self.set_global(name, val);
        true
    }

    /// Variabel yang ejaannya paling dekat dengan `name`, untuk saran di pesan galat.
    pub fn saran_variabel(&self, name: &str) -> Option<String> {
        let mut calon: Vec<String> = Vec::new();
        for scope in &self.scopes {
            for k in scope.keys() { calon.push(k.clone()); }
        }
        for k in self.globals.lock().unwrap_or_else(|p| p.into_inner()).keys() {
            if let Some(n) = k.strip_prefix(AWALAN_VAR) { calon.push(n.to_string()); }
        }
        crate::diag::saran_nama_terdekat(name, calon.iter().map(|s| s.as_str()))
    }

    pub fn set(&mut self, name: &str, val: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                // `Binding::set` yang memutuskan apakah tulisan ini masuk
                // ke slot lokal atau menembus sel bersama.
                b.set(val);
                return true;
            }
        }
        let mut g = self.globals.lock().unwrap_or_else(|p| p.into_inner());
        if g.contains_key(name) {
            g.insert(name.to_string(), val);
            true
        } else {
            false
        }
    }

    /// Apakah lingkungan ini punya nama global ber-awalan `awalan` (mis. `util::`).
    /// Menyusuri seluruh global, jadi cuma untuk jalur GAGAL.
    pub fn ada_awalan(&self, awalan: &str) -> bool {
        self.globals.lock().unwrap_or_else(|p| p.into_inner()).keys().any(|k| k.starts_with(awalan))
    }

    /// Semua nama yang KELIHATAN dari titik ini (scope lokal + global), digabung
    /// tanpa duplikat — dipakai buat cari saran "maksudnya `x`?" waktu ada nama …
    pub fn all_names(&self) -> Vec<String> {
        let mut set = std::collections::HashSet::new();
        for scope in &self.scopes {
            for k in scope.keys() { set.insert(k.clone()); }
        }
        for k in self.globals.lock().unwrap_or_else(|p| p.into_inner()).keys() {
            // Kunci penanda (`AWALAN_VAR`) bukan nama: tidak boleh jadi saran ejaan.
            if !k.starts_with('\u{1}') { set.insert(k.clone()); }
        }
        set.into_iter().collect()
    }

    /// Cari nama terdekat dengan `target` di antara semua nama yang kelihatan.
    pub fn suggest_similar(&self, target: &str) -> Option<String> {
        let nama = self.all_names();
        crate::diag::saran_nama_terdekat(target, nama.iter().map(|s| s.as_str()))
    }
}
