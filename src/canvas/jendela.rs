// src/canvas/jendela.rs — jendela minifb di thread miliknya sendiri.
// `minifb::Window` bukan `Send`, sementara kanvas menyeberang antar-thread; jadi
// kanvas cuma memegang channel ke thread yang memiliki jendelanya.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

enum Perintah {
    Bingkai(Vec<u32>),
    Judul(String),
    Tutup,
}

/// Keadaan masukan sesudah satu bingkai ditampilkan.
pub struct Laporan {
    pub terbuka: bool,
    pub ditekan: Vec<String>,
    pub ditahan: Vec<String>,
    pub tetikus: Option<(f32, f32)>,
    pub tombol_tetikus: [bool; 3],
    pub galat: Option<String>,
    /// Desktop: tidak ada layar sentuh, kanvas menurunkannya dari tetikus.
    pub sentuhan: Option<Vec<(f32, f32)>>,
}

pub struct Jendela {
    kirim: Sender<Perintah>,
    terima: Receiver<Laporan>,
    utas: Option<JoinHandle<()>>,
}

impl Jendela {
    /// Buka jendela; gagalnya (mis. tidak ada layar) DILAPORKAN, bukan ditelan.
    pub fn buka(judul: &str, lebar: usize, tinggi: usize) -> Result<Jendela, String> {
        let (kirim, terima_perintah) = channel::<Perintah>();
        let (kirim_laporan, terima) = channel::<Laporan>();
        let (kirim_siap, terima_siap) = channel::<Result<(), String>>();
        let judul = judul.to_string();
        let utas = std::thread::Builder::new()
            .name("jrg-jendela".into())
            .spawn(move || jalankan(judul, lebar, tinggi, terima_perintah, kirim_laporan, kirim_siap))
            .map_err(|e| format!("thread jendela tidak bisa dibuat: {}", e))?;
        match terima_siap.recv() {
            Ok(Ok(())) => Ok(Jendela { kirim, terima, utas: Some(utas) }),
            Ok(Err(e)) => { let _ = utas.join(); Err(e) }
            Err(_) => { let _ = utas.join(); Err("thread jendela berhenti sebelum jendelanya siap".into()) }
        }
    }

    /// Tampilkan satu bingkai, lalu jawab keadaan masukannya.
    pub fn tampilkan(&self, piksel: &[u32]) -> Result<Laporan, String> {
        self.kirim.send(Perintah::Bingkai(piksel.to_vec()))
            .map_err(|_| "jendela sudah ditutup".to_string())?;
        self.terima.recv().map_err(|_| "jendela sudah ditutup".to_string())
    }

    pub fn judul(&self, judul: &str) {
        let _ = self.kirim.send(Perintah::Judul(judul.to_string()));
    }
}

impl Drop for Jendela {
    fn drop(&mut self) {
        let _ = self.kirim.send(Perintah::Tutup);
        if let Some(u) = self.utas.take() { let _ = u.join(); }
    }
}

fn jalankan(judul: String, lebar: usize, tinggi: usize, perintah: Receiver<Perintah>,
            laporan: Sender<Laporan>, siap: Sender<Result<(), String>>) {
    use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Scale, Window, WindowOptions};
    let opsi = WindowOptions { resize: false, scale: Scale::X1, ..WindowOptions::default() };
    let mut w = match Window::new(&judul, lebar, tinggi, opsi) {
        Ok(w) => { let _ = siap.send(Ok(())); w }
        Err(e) => { let _ = siap.send(Err(pesan_gagal_buka(&e.to_string()))); return; }
    };
    for p in perintah {
        match p {
            Perintah::Judul(j) => w.set_title(&j),
            Perintah::Tutup => return,
            Perintah::Bingkai(piksel) => {
                let mut l = Laporan { terbuka: w.is_open() && !w.is_key_down(Key::Escape),
                    ditekan: Vec::new(), ditahan: Vec::new(), tetikus: None,
                    tombol_tetikus: [false; 3], galat: None, sentuhan: None };
                if l.terbuka {
                    if let Err(e) = w.update_with_buffer(&piksel, lebar, tinggi) {
                        l.galat = Some(format!("canvas update: {}", e));
                    }
                    l.ditekan = w.get_keys_pressed(KeyRepeat::No).iter().map(|k| format!("{:?}", k)).collect();
                    l.ditahan = w.get_keys().iter().map(|k| format!("{:?}", k)).collect();
                    l.tetikus = w.get_mouse_pos(MouseMode::Clamp);
                    l.tombol_tetikus = [w.get_mouse_down(MouseButton::Left),
                        w.get_mouse_down(MouseButton::Right), w.get_mouse_down(MouseButton::Middle)];
                }
                if laporan.send(l).is_err() { return; }
            }
        }
    }
}

/// Sebab paling sering: tidak ada layar sama sekali. Disebut, bukan ditebak dari teks minifb.
fn pesan_gagal_buka(asli: &str) -> String {
    let ada = |v: &str| std::env::var_os(v).map_or(false, |x| !x.is_empty());
    if !ada("DISPLAY") && !ada("WAYLAND_DISPLAY") {
        format!("jendela tidak bisa dibuka ({}) — tidak ada layar: DISPLAY dan WAYLAND_DISPLAY \
                 kosong. Tanpa layar, pakai mode `\"web\"` (peramban) atau `\"png\"` (berkas)", asli)
    } else {
        format!("jendela tidak bisa dibuka: {}", asli)
    }
}
