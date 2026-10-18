// src/canvas/jendela_android.rs — mode `"tab"` di APK: bingkai digambar ke layar HP.
// Pengantar C APK (`src/llvm/android.c`) memasang tabel fungsinya lewat `pasang`;
// antarmukanya sama dengan `jendela.rs`.

use std::sync::atomic::{AtomicPtr, Ordering};

/// Keadaan masukan sesudah satu bingkai ditampilkan.
pub struct Laporan {
    pub terbuka: bool,
    pub ditekan: Vec<String>,
    pub ditahan: Vec<String>,
    pub tetikus: Option<(f32, f32)>,
    pub tombol_tetikus: [bool; 3],
    pub galat: Option<String>,
    /// Semua jari yang menempel, koordinat kanvas.
    pub sentuhan: Option<Vec<(f32, f32)>>,
}

/// Disusun sama persis dengan `JrgAndroid` di `android.c`.
#[repr(C)]
pub struct TabelAndroid {
    /// Tampilkan satu bingkai ARGB; 1 = masih terbuka, 0 = aplikasinya ditutup.
    pub tampil: extern "C" fn(*const u32, i32, i32) -> i32,
    /// Sentuhan terakhir dalam koordinat kanvas, apakah layar sedang disentuh, dan tombol kembali.
    pub masukan: extern "C" fn(*mut f32, *mut f32, *mut i32, *mut i32),
    /// Isi pasangan x,y untuk paling banyak `maks` jari; jawabannya jumlah yang terisi.
    pub sentuhan: extern "C" fn(*mut f32, i32) -> i32,
}

static TABEL: AtomicPtr<TabelAndroid> = AtomicPtr::new(std::ptr::null_mut());

/// Dipanggil pengantar APK sebelum `main` program jalan.
pub fn pasang(t: *const TabelAndroid) {
    TABEL.store(t as *mut TabelAndroid, Ordering::Release);
}

pub fn ada_layar() -> bool {
    !TABEL.load(Ordering::Acquire).is_null()
}

pub struct Jendela {
    lebar: usize,
    tinggi: usize,
}

impl Jendela {
    pub fn buka(_judul: &str, lebar: usize, tinggi: usize) -> Result<Jendela, String> {
        if !ada_layar() {
            return Err("mode `\"tab\"` di Android cuma ada di dalam APK (`jrgc program.jrg --apk app.apk`)\n\
                        bantuan: di luar APK pakai mode `\"png\"`".to_string());
        }
        Ok(Jendela { lebar, tinggi })
    }

    pub fn tampilkan(&self, piksel: &[u32]) -> Result<Laporan, String> {
        let t = unsafe { &*TABEL.load(Ordering::Acquire) };
        let terbuka = (t.tampil)(piksel.as_ptr(), self.lebar as i32, self.tinggi as i32) == 1;
        let (mut x, mut y, mut sentuh, mut kembali) = (0f32, 0f32, 0i32, 0i32);
        (t.masukan)(&mut x, &mut y, &mut sentuh, &mut kembali);
        let mut xy = [0f32; 20];
        let n = ((t.sentuhan)(xy.as_mut_ptr(), 10).clamp(0, 10)) as usize;
        let jari = (0..n).map(|i| (xy[2 * i], xy[2 * i + 1])).collect();
        // Tombol kembali menutup kanvas, sama dengan `Escape` di desktop.
        Ok(Laporan {
            terbuka: terbuka && kembali == 0,
            ditekan: Vec::new(),
            ditahan: Vec::new(),
            tetikus: Some((x, y)),
            tombol_tetikus: [sentuh != 0, false, false],
            galat: None,
            sentuhan: Some(jari),
        })
    }

    pub fn judul(&self, _judul: &str) {}
}
