// src/vm/channel.rs
// VmChannel — Go-style channels untuk Bytecode VM

use super::value::VmValue;

// ─── VmChannel ───────────────────────────────────────────────────────────

/// Channel VM: `Antrean` dengan muatan `VmValue`.
pub type VmChannel = crate::antrean::Antrean<VmValue>;

/// Ejaan lama (`send`/`recv`/…) dipertahankan sebagai pembungkus satu baris,
/// bukan sebagai implementasi kedua.
impl VmChannel {
    pub fn send(&self, val: VmValue) -> Result<(), String> { self.kirim(val) }
    pub fn try_send(&self, val: VmValue) -> Result<bool, String> { self.coba_kirim(val) }
    pub fn recv(&self) -> Result<Option<VmValue>, String> { Ok(self.terima()) }
    pub fn try_recv(&self) -> Result<Option<VmValue>, String> {
        match self.coba_terima() {
            Ok(v) => Ok(Some(v)),
            Err(_) => Ok(None),
        }
    }
    pub fn close(&self) { self.tutup() }
    pub fn is_closed(&self) -> bool { self.tertutup() }
    pub fn len(&self) -> usize { self.panjang() }
    pub fn capacity(&self) -> usize { self.kapasitas() }
    pub fn has_value(&self) -> bool { self.ada_isi() }
}

// ─── SpawnTask ────────────────────────────────────────────────────────────

pub struct SpawnTask {
    pub fn_val:  VmValue,
    pub args:    Vec<VmValue>,
    pub globals: crate::hash_cepat::PetaCepat<String, VmValue>,
}

unsafe impl Send for SpawnTask {}

/// Handle setiap goroutine yang masih berjalan.
static HANDLE_GOROUTINE: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

/// Tunggu semua goroutine, sama seperti `Interpreter::join_all_goroutines`.
pub fn join_all_goroutines() {
    loop {
        let h = {
            let mut g = HANDLE_GOROUTINE.lock().unwrap_or_else(|p| p.into_inner());
            g.pop()
        };
        match h {
            Some(h) => { let _ = h.join(); }
            None => break,
        }
    }
}

pub fn spawn_goroutine(task: SpawnTask) {
    // Didaftarkan SEBELUM utasnya lahir: kalau sesudah, laporan bisa
    // terlanjur dicetak sebelum penghitungnya naik.
    crate::profiler::utas_baru();
    let h = std::thread::spawn(move || {
        let mut vm = super::vm::Vm::new();
        // Goroutine mewarisi keadaan profiler dari proses, bukan dari VM induknya —
        // `Vm::new()` selalu lahir dengan profiling mati, jadi tanpa baris ini …
        if crate::profiler::sedang_aktif() { vm.enable_profiling(); }
        for (k, v) in task.globals {
            vm.globals.insert(k, v);
        }
        match vm.call_value_immediate(task.fn_val, task.args) {
            Ok(_)  => {}
            Err(e) => crate::host::galat_baris(&format!("goroutine error: {}", e.msg)),
        }
        // Sama seperti goroutine interpreter: setor hasil profiler
        // thread ini sebelum ia selesai.
        crate::profiler::flush();
    });
    HANDLE_GOROUTINE.lock().unwrap_or_else(|p| p.into_inner()).push(h);
}
