// src/canvas/web.rs — mode kanvas `"web"`: bingkai disajikan ke peramban lewat HTTP lokal.
// Satu utas pelayan di 127.0.0.1; halaman HTML/JS-nya meminta bingkai terbaru dan
// mengirim balik tombol dan tetikus.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// Keadaan yang dibagi antara kanvas dan utas pelayan.
#[derive(Default)]
pub struct Bersama {
    pub lebar: usize,
    pub tinggi: usize,
    /// Bingkai terakhir sebagai RGBA berurutan.
    pub rgba: Vec<u8>,
    pub ditahan: HashSet<String>,
    pub ditekan: HashSet<String>,
    pub tetikus: (f32, f32),
    pub tombol_tetikus: [bool; 3],
    pub judul: String,
}

pub struct Web {
    pub bersama: Arc<Mutex<Bersama>>,
    pub alamat: String,
}

impl Web {
    /// Buka pelayan di `127.0.0.1` (port dari `JRG_KANVAS_PORT`, atau dipilih sistem).
    pub fn buka(judul: &str, lebar: usize, tinggi: usize) -> Result<Web, String> {
        let port = std::env::var("JRG_KANVAS_PORT").ok().and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
        let dengar = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("mode `web`: port {} tidak bisa dipakai: {}", port, crate::host::galat_io(&e)))?;
        let alamat = format!("http://{}/", dengar.local_addr().map_err(|e| e.to_string())?);
        let bersama = Arc::new(Mutex::new(Bersama {
            lebar, tinggi, rgba: vec![0; lebar * tinggi * 4], judul: judul.to_string(), ..Default::default()
        }));
        let b = bersama.clone();
        std::thread::Builder::new().name("jrg-kanvas-web".into())
            .spawn(move || { for s in dengar.incoming().flatten() { let _ = layani(s, &b); } })
            .map_err(|e| format!("mode `web`: utas pelayan tidak bisa dibuat: {}", crate::host::galat_io(&e)))?;
        Ok(Web { bersama, alamat })
    }

    /// Salin bingkai ARGB ke penyangga RGBA yang diminta peramban.
    pub fn kirim(&self, piksel: &[u32]) {
        let mut b = self.bersama.lock().unwrap_or_else(|p| p.into_inner());
        for (i, p) in piksel.iter().enumerate() {
            let o = i * 4;
            if o + 3 >= b.rgba.len() { break; }
            b.rgba[o] = (p >> 16) as u8;
            b.rgba[o + 1] = (p >> 8) as u8;
            b.rgba[o + 2] = *p as u8;
            b.rgba[o + 3] = (p >> 24) as u8;
        }
    }
}

fn layani(mut s: TcpStream, b: &Arc<Mutex<Bersama>>) -> std::io::Result<()> {
    // Kepala permintaan bisa datang dalam beberapa potong: dibaca sampai baris kosongnya.
    let mut buf = Vec::with_capacity(512);
    let mut potong = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 8192 {
        let n = s.read(&mut potong)?;
        if n == 0 { break; }
        buf.extend_from_slice(&potong[..n]);
    }
    let kepala = String::from_utf8_lossy(&buf);
    let jalur = kepala.split_whitespace().nth(1).unwrap_or("/").to_string();
    let (jalan, kueri) = jalur.split_once('?').unwrap_or((jalur.as_str(), ""));
    let param = |k: &str| kueri.split('&').find_map(|p| p.strip_prefix(&format!("{}=", k))).unwrap_or("").to_string();
    match jalan {
        "/bingkai" => {
            let isi = b.lock().unwrap_or_else(|p| p.into_inner()).rgba.clone();
            balas(&mut s, "application/octet-stream", &isi)
        }
        "/tombol" => {
            if let Ok(nama) = super::kenali_tombol(&nama_dari_kode(&param("k"))) {
                let mut g = b.lock().unwrap_or_else(|p| p.into_inner());
                if param("d") == "1" {
                    if g.ditahan.insert(nama.to_string()) { g.ditekan.insert(nama.to_string()); }
                } else {
                    g.ditahan.remove(nama);
                }
            }
            balas(&mut s, "text/plain", b"ok")
        }
        "/tetikus" => {
            let mut g = b.lock().unwrap_or_else(|p| p.into_inner());
            g.tetikus = (param("x").parse().unwrap_or(0.0), param("y").parse().unwrap_or(0.0));
            let tombol: u8 = param("b").parse().unwrap_or(0);
            g.tombol_tetikus = [tombol & 1 != 0, tombol & 2 != 0, tombol & 4 != 0];
            drop(g);
            balas(&mut s, "text/plain", b"ok")
        }
        _ => {
            let g = b.lock().unwrap_or_else(|p| p.into_inner());
            let html = halaman(&g.judul, g.lebar, g.tinggi);
            drop(g);
            balas(&mut s, "text/html; charset=utf-8", html.as_bytes())
        }
    }
}

fn balas(s: &mut TcpStream, jenis: &str, isi: &[u8]) -> std::io::Result<()> {
    write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
           jenis, isi.len())?;
    s.write_all(isi)
}

/// `KeyboardEvent.code` peramban ke nama tombol kanvas (`KeyA` -> `A`, `Digit1` -> `Key1`).
fn nama_dari_kode(kode: &str) -> String {
    if let Some(h) = kode.strip_prefix("Key") { return h.to_string(); }
    if let Some(d) = kode.strip_prefix("Digit") { return format!("Key{}", d); }
    if let Some(a) = kode.strip_prefix("Arrow") { return a.to_string(); }
    match kode {
        "ShiftLeft" => "LeftShift", "ShiftRight" => "RightShift", "ControlLeft" => "LeftCtrl",
        "ControlRight" => "RightCtrl", "AltLeft" => "LeftAlt", "AltRight" => "RightAlt",
        lain => lain,
    }.to_string()
}

fn halaman(judul: &str, lebar: usize, tinggi: usize) -> String {
    let skala = (720 / lebar.max(1)).clamp(1, 8);
    let aman: String = judul.chars().filter(|c| !matches!(c, '<' | '>' | '&' | '"')).collect();
    format!(r#"<!doctype html><html><head><meta charset="utf-8"><title>{aman}</title>
<style>body{{margin:0;background:#111;display:flex;height:100vh;align-items:center;justify-content:center}}
canvas{{image-rendering:pixelated;width:{w}px;height:{h}px}}</style></head><body>
<canvas id="k" width="{lebar}" height="{tinggi}" tabindex="0"></canvas><script>
const k=document.getElementById('k'),g=k.getContext('2d'),gb=g.createImageData({lebar},{tinggi});
async function putar(){{try{{const r=await fetch('/bingkai');gb.data.set(new Uint8ClampedArray(await r.arrayBuffer()));g.putImageData(gb,0,0)}}catch(e){{}}requestAnimationFrame(putar)}}
const tombol=(e,d)=>{{if(e.repeat)return;fetch('/tombol?k='+e.code+'&d='+d);e.preventDefault()}};
addEventListener('keydown',e=>tombol(e,1));addEventListener('keyup',e=>tombol(e,0));
const tetikus=e=>{{const b=k.getBoundingClientRect();fetch('/tetikus?x='+((e.clientX-b.left)/{skala})+'&y='+((e.clientY-b.top)/{skala})+'&b='+e.buttons)}};
k.addEventListener('mousemove',tetikus);k.addEventListener('mousedown',tetikus);k.addEventListener('mouseup',tetikus);
k.addEventListener('contextmenu',e=>e.preventDefault());k.focus();putar();
</script></body></html>"#, aman = aman, lebar = lebar, tinggi = tinggi, w = lebar * skala, h = tinggi * skala, skala = skala)
}
