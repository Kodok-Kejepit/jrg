//! `JRG_JIT_LOG=ringkas`: fungsi terpanas ke stderr saat program selesai (stdout tidak disentuh,
//! jadi tabel paritas tetap membandingkan keluaran program saja).

use std::sync::atomic::Ordering;

/// `JRG_JIT_LOG` dipasang (dibaca sekali).
pub fn diminta() -> bool {
    static A: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *A.get_or_init(|| std::env::var_os("JRG_JIT_LOG").is_some())
}

pub fn cetak_bila_diminta() {
    if !super::aktif() || !diminta() { return; }
    let mut semua = super::profil::semua();
    semua.retain(|p| p.panggilan.load(Ordering::Relaxed) + p.putaran.load(Ordering::Relaxed) > 0);
    semua.sort_by_key(|p| std::cmp::Reverse(
        p.panggilan.load(Ordering::Relaxed) as u64 + p.putaran.load(Ordering::Relaxed) as u64));
    eprintln!("[jit] profil T0: {} fungsi berjalan", semua.len());
    #[cfg(all(feature = "jit", not(target_arch = "wasm32")))]
    {
        let jadi = super::modul::terkompilasi();
        if !jadi.is_empty() { eprintln!("[jit] dikompilasi: {}", jadi.join(", ")); }
        for (nama, alasan) in super::modul::ditolak() {
            eprintln!("[jit] ditolak: {}: {alasan}", crate::ruang_tipe::bersih(&nama));
        }
        let d = super::deopt::jumlah();
        if d > 0 { eprintln!("[jit] deopt: {d}"); }
        for (sebab, n) in super::deopt::sebab().iter().take(12) { eprintln!("[jit]   {sebab}: {n}"); }
    }
    for p in semua.iter().take(20) {
        let situs: Vec<u16> = p.umpan.iter().map(|s| s.load(Ordering::Relaxed)).filter(|b| *b != 0).collect();
        let mono = situs.iter().filter(|b| b.count_ones() == 1).count();
        eprintln!("[jit]   {}: panggilan {}, putaran {}, situs {} ({} monomorfik)",
            crate::ruang_tipe::bersih(&p.nama), p.panggilan.load(Ordering::Relaxed),
            p.putaran.load(Ordering::Relaxed), situs.len(), mono);
    }
}
