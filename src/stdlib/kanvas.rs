// src/stdlib/kanvas.rs — stdlib/kanvas.rs — `Canvas::` untuk backend VM.

use crate::canvas;
use crate::vm::ffi_jembatan::TitipanVm;
use crate::vm::value::VmValue;
use crate::vm::vm::Vm;

/// Panggil satu method kanvas lewat jembatan nilai.
fn panggil(a: &[VmValue], metode: &str) -> Result<VmValue, String> {
    let Some(VmValue::Canvas(h)) = a.first() else {
        return Err(format!(
            "canvas::{}: argumen pertamanya harus sebuah Canvas, dapat `{}`",
            metode,
            a.first().map(|v| v.type_name_penuh()).unwrap_or_else(|| "tidak ada".to_string())
        ));
    };
    let mut titipan = TitipanVm::default();
    let mut arg = Vec::with_capacity(a.len().saturating_sub(1));
    for v in &a[1..] {
        arg.push(titipan.ke_interp(v)?);
    }
    match canvas::dispatch_canvas_method(h.clone(), metode, &arg) {
        Ok(v) => titipan.ke_vm(&v),
        // Kegagalannya diteruskan APA ADANYA, jadi pesan errornya sama
        // persis dengan interpreter — bukan dibungkus ulang jadi kalimat
        // kedua yang harus dijaga tetap sinkron.
        Err(crate::interpreter::Signal::RuntimeError(e)) => Err(e.message),
        Err(lain) => Err(format!("canvas::{}: sinyal tak terduga: {:?}", metode, lain)),
    }
}

pub fn register(vm: &mut Vm) {
    // `Canvas::new` — satu definisi (`canvas::canvas_baru`) untuk dua mesin; dulu
    // salinan kedua di sini memotong ukurannya ke `i64` dan mati di ukuran raksasa.
    vm.def("Canvas::new", |a| {
        let mut titipan = TitipanVm::default();
        let mut arg = Vec::with_capacity(a.len());
        for v in a { arg.push(titipan.ke_interp(v)?); }
        canvas::canvas_baru(&arg).map(VmValue::Canvas)
    });

    // `Canvas::load_png` — pasangan `canvas::save_png`.
    vm.def("Canvas::load_png", |a| {
        let Some(VmValue::Str(path)) = a.first() else {
            return Err(format!(
                "Canvas::load_png butuh 1 arg: (path), dapat `{}`",
                a.first().map(|v| v.type_name_penuh()).unwrap_or_else(|| "tidak ada".to_string())));
        };
        let isi = std::fs::read(path.as_str()).map_err(|e|
            // Sebabnya disebut, bukan "gagal" polos: path salah, izin, dan
            // berkas tidak ada adalah tiga masalah dengan tiga perbaikan.
            format!("Canvas::load_png '{}': {}", path, crate::host::galat_io(&e)))?;
        let (w, h, piksel) = crate::canvas::png_baca::baca(&isi)
            .map_err(|e| format!("Canvas::load_png '{}': {}", path, e))?;
        let handle = canvas::canvas_new(w, h, path.as_str());
        {
            let mut inner = handle.lock().map_err(|e|
                format!("Canvas::load_png: canvas lock: {}", e))?;
            inner.fb.pixels.copy_from_slice(&piksel);
        }
        Ok(VmValue::Canvas(handle))
    });

    vm.def("canvas::punya_jendela", |_| Ok(VmValue::Bool(canvas::punya_jendela())));
    vm.def("canvas::layar_sentuh", |_| Ok(VmValue::Bool(canvas::layar_sentuh())));
    // Nama kanonik tombol (sama dengan yang dipakai `key_held`); yang asing ditolak.
    vm.def("canvas::nama_tombol", |a| match a.first() {
        Some(VmValue::Str(s)) => canvas::kenali_tombol(s.as_str()).map(|t| VmValue::str(t)),
        lain => Err(format!("canvas::nama_tombol: argumen ke-1 harus teks, dapat `{}`",
                            lain.map(|v| v.type_name_penuh()).unwrap_or_else(|| "tidak ada".into()))),
    });

    // Tiap method dapat `fn`-nya sendiri dengan namanya tertanam.
    macro_rules! m {
        ($n:literal) => {
            vm.def(concat!("canvas::", $n), |a| panggil(a, $n));
        };
    }
    m!("blit");
    m!("blit_region");
    m!("clear");
    m!("clip");
    m!("clip_reset");
    m!("close");
    m!("delta_time");
    m!("draw_arc");
    m!("draw_bezier");
    m!("draw_circle");
    m!("draw_crosshair");
    m!("draw_ellipse");
    m!("draw_grid");
    m!("draw_line");
    m!("draw_pixel");
    m!("draw_poly");
    m!("draw_rect");
    m!("draw_round_rect");
    m!("draw_text");
    m!("draw_text_scaled");
    m!("draw_triangle");
    m!("fill_circle");
    m!("fill_ellipse");
    m!("fill_linear_gradient");
    m!("fill_poly");
    m!("fill_radial_gradient");
    m!("fill_rect");
    m!("fill_round_rect");
    m!("fill_triangle");
    m!("fps");
    m!("frame");
    m!("get_mouse_button");
    m!("get_mouse_pos");
    m!("jumlah_sentuhan");
    m!("sentuhan");
    m!("get_pixel");
    m!("height");
    m!("is_open");
    m!("key_held");
    m!("key_pressed");
    m!("line_aa");
    m!("pop");
    m!("push");
    m!("reset_transform");
    m!("rotate");
    m!("save_bmp");
    m!("save_png");
    m!("save_ppm");
    m!("scale");
    m!("set_headless_export");
    m!("set_title");
    m!("translate");
    m!("update");
    m!("width");
}
