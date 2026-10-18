// tests/canvas.rs — tests/canvas.rs — mengunci library gambar.

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

fn ada_python() -> bool {
    Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

struct Ruang(std::path::PathBuf);
impl Ruang {
    fn baru() -> Ruang {
        let d = crate::bersama::dir_unik("jrg-canvas");
        std::fs::create_dir_all(&d).expect("bikin dir");
        Ruang(d)
    }
    fn jalankan(&self, badan: &str) -> (String, bool) { self.jalankan_di(badan, false) }
    fn jalankan_di(&self, badan: &str, vm: bool) -> (String, bool) {
        let f = self.0.join("uji.jrg");
        std::fs::write(&f, format!("package main\npub fn main() {{\n{}\n}}\n", badan))
            .expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.arg(&f).arg("--sopan").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
        if vm { cmd.arg("--vm"); }
        let out = cmd.output().expect("jalankan");
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)), out.status.success())
    }
}
impl Drop for Ruang {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn harus(badan: &str, potongan: &[&str]) {
    let r = Ruang::baru();
    let (keluaran, sukses) = r.jalankan(badan);
    assert!(sukses, "program gagal:\n{}", keluaran);
    for p in potongan {
        assert!(keluaran.contains(p), "keluaran tidak memuat `{}`:\n{}", p, keluaran);
    }
}

/// Warna yang ditulis `0xRRGGBB` harus OPAK.
#[test]
fn warna_rgb_tanpa_alpha_berarti_opak() {
    harus(
        r#"    let c = Canvas::new(4, 4, "w")
    c.clear(0x000000)
    c.fill_rect(0, 0, 2, 2, 0xFF0000)
    wok("rect   :", c.get_pixel(0, 0))
    c.fill_circle(3, 3, 1, 0x00FF00)
    wok("circle :", c.get_pixel(3, 3))
    // Alpha EKSPLISIT tetap dihormati: 0x80 di atas hitam = setengah.
    c.fill_rect(0, 3, 1, 1, 0x800000FF)
    wok("alpha  :", c.get_pixel(0, 3))"#,
        &[
            "rect   : Warna {a: 255, b: 0, g: 0, r: 255}",
            "circle : Warna {a: 255, b: 0, g: 255, r: 0}",
            // 255 * 0x80/255 = 128 -> biru setengah di atas hitam opak.
            "alpha  : Warna {a: 255, b: 128, g: 0, r: 0}",
        ],
    );
}

#[test]
fn bentuk_lanjutan_benar_benar_menggambar() {
    harus(
        r#"    let c = Canvas::new(60, 60, "w")
    c.clear(0x000000)
    c.fill_ellipse(30.0, 30.0, 20.0, 10.0, 0xFF0000)
    wok("elips tengah:", c.get_pixel(30, 30))
    wok("elips luar  :", c.get_pixel(30, 55))
    c.clear(0x000000)
    c.fill_poly([[5,5],[55,5],[30,50]], 0x00FF00)
    wok("poli dalam  :", c.get_pixel(30, 20))
    wok("poli luar   :", c.get_pixel(5, 50))
    c.clear(0x000000)
    c.fill_round_rect(5.0, 5.0, 50.0, 50.0, 15.0, 0x0000FF)
    wok("bulat tengah:", c.get_pixel(30, 30))
    // Sudut kotak berjari-jari 15 harus KOSONG — kalau terisi, radiusnya
    // tidak berlaku dan bentuknya cuma kotak biasa.
    wok("bulat sudut :", c.get_pixel(6, 6))"#,
        &[
            "elips tengah: Warna {a: 255, b: 0, g: 0, r: 255}",
            "elips luar  : Warna {a: 255, b: 0, g: 0, r: 0}",
            "poli dalam  : Warna {a: 255, b: 0, g: 255, r: 0}",
            "poli luar   : Warna {a: 255, b: 0, g: 0, r: 0}",
            "bulat tengah: Warna {a: 255, b: 255, g: 0, r: 0}",
            "bulat sudut : Warna {a: 255, b: 0, g: 0, r: 0}",
        ],
    );
}

#[test]
fn transformasi_dan_pemotongan_berlaku() {
    harus(
        r#"    let c = Canvas::new(40, 40, "w")
    c.clear(0x000000)
    c.push()
    c.translate(20.0, 20.0)
    c.fill_poly([[0,0],[10,0],[10,10],[0,10]], 0xFF0000)
    c.pop()
    wok("digeser    :", c.get_pixel(25, 25))
    wok("asal kosong:", c.get_pixel(5, 5))
    // `pop` mengembalikan keadaan: gambar berikutnya kembali ke asal.
    c.fill_poly([[0,0],[10,0],[10,10],[0,10]], 0x00FF00)
    wok("sesudah pop:", c.get_pixel(5, 5))
    c.clear(0x000000)
    c.clip(0, 0, 20, 40)
    c.fill_poly([[0,0],[40,0],[40,40],[0,40]], 0x0000FF)
    c.clip_reset()
    wok("dlm potong :", c.get_pixel(10, 10))
    wok("luar potong:", c.get_pixel(30, 10))"#,
        &[
            "digeser    : Warna {a: 255, b: 0, g: 0, r: 255}",
            "asal kosong: Warna {a: 255, b: 0, g: 0, r: 0}",
            "sesudah pop: Warna {a: 255, b: 0, g: 255, r: 0}",
            "dlm potong : Warna {a: 255, b: 255, g: 0, r: 0}",
            "luar potong: Warna {a: 255, b: 0, g: 0, r: 0}",
        ],
    );
}

/// SETIAP primitif mematuhi geser, skala dan clip — dulu cuma jalur `lanjut.rs`
/// (poligon, elips) yang patuh, dan tes di atas kebetulan cuma memakai `fill_poly`.
#[test]
fn semua_primitif_mematuhi_transformasi_dan_clip() {
    const PRIMITIF: &[&str] = &[
        "canvas::fill_rect(c, 0, 0, 4, 4, W)",
        "canvas::draw_rect(c, 0, 0, 4, 4, W)",
        "canvas::draw_line(c, 0, 0, 4, 3, W)",
        "canvas::draw_pixel(c, 1, 1, W)",
        "canvas::fill_circle(c, 2, 2, 2, W)",
        "canvas::draw_circle(c, 2, 2, 2, W)",
        "canvas::fill_triangle(c, 0, 0, 4, 0, 0, 4, W)",
        "canvas::draw_triangle(c, 0, 0, 4, 0, 0, 4, W)",
        "canvas::draw_text(c, 0, 0, \"#\", W)",
        "canvas::draw_text_scaled(c, 0, 0, \"#\", 2, W)",
        "canvas::draw_crosshair(c, 3, 3, 2, W)",
        "canvas::blit(c, s, 0, 0)",
        "canvas::blit_region(c, s, 0, 0, 3, 3, 0, 0)",
    ];
    let pembuka = "    let s = Canvas::new(3, 3)\n    canvas::clear(s, 0xFFFFFF)\n\
        \x20   fn nyala(c, x0, y0, x1, y1) {\n        var n = 0\n\
        \x20       for _, y in range(y0, y1) { for _, x in range(x0, x1) { if canvas::get_pixel(c, x, y).r > 0 { n = n + 1 } } }\n\
        \x20       return n\n    }\n";
    let mut gagal = Vec::new();
    for p in PRIMITIF {
        let gambar = p.replace("W", "0xFFFFFF");
        let badan = format!(
            "{pembuka}    let c = Canvas::new(40, 40)\n    canvas::translate(c, 20, 20)\n    {g}\n\
             \x20   wok(\"GESER\", nyala(c, 20, 20, 36, 36), nyala(c, 0, 0, 16, 16))\n\
             \x20   let d = Canvas::new(40, 40)\n    canvas::clip(d, 30, 30, 2, 2)\n    {h}\n\
             \x20   wok(\"CLIP\", nyala(d, 0, 0, 40, 40))\n\
             \x20   let e = Canvas::new(40, 40)\n    canvas::scale(e, 3, 3)\n    {k}\n\
             \x20   let u = Canvas::new(40, 40)\n    {m}\n\
             \x20   wok(\"SKALA\", nyala(e, 0, 0, 40, 40) > nyala(u, 0, 0, 40, 40))\n",
            pembuka = pembuka, g = gambar.clone(), h = gambar.replace("(c,", "(d,"),
            k = gambar.replace("(c,", "(e,"), m = gambar.replace("(c,", "(u,"));
        for vm in [false, true] {
        let r = Ruang::baru();
        let (keluaran, sukses) = r.jalankan_di(&badan, vm);
        let baris = |awal: &str| keluaran.lines().find(|l| l.starts_with(awal)).unwrap_or("").to_string();
        let geser = baris("GESER");
        let v: Vec<&str> = geser.split_whitespace().collect();
        let ok_geser = v.len() == 3 && v[1] != "0" && v[2] == "0";
        let ok_clip = baris("CLIP") == "CLIP 0";
        let skala = baris("SKALA");
        if !sukses || !ok_geser || !ok_clip || skala != "SKALA true" {
            gagal.push(format!("  {} [{}]: {} | {} | {}\n{}", p, if vm { "vm" } else { "interp" }, geser, baris("CLIP"), skala,
                               if sukses { String::new() } else { keluaran.clone() }));
        }
        }
    }
    assert!(gagal.is_empty(), "primitif yang mengabaikan transformasi/clip:\n{}", gagal.join("\n"));
}

/// `push` di atas batas tumpukan DITOLAK — dulu diam-diam tidak disimpan, jadi
/// `pop` sesudahnya memulihkan keadaan yang salah tanpa sepatah kata.
#[test]
fn push_melewati_batas_ditolak_dan_nama_tombol_dijawab() {
    let badan = "    let c = Canvas::new(2, 2)\n    wok(\"TOMBOL\", canvas::nama_tombol(\"space\"), canvas::nama_tombol(\"esc\"))\n\
        \x20   wok(\"JENDELA\", typeof(canvas::punya_jendela()))\n\
        \x20   for i in range(0, 256) { canvas::push(c) }\n    wok(\"PENUH\")\n    canvas::push(c)\n    wok(\"TIDAK SAMPAI\")\n";
    for vm in [false, true] {
        let r = Ruang::baru();
        let (k, sukses) = r.jalankan_di(badan, vm);
        assert!(!sukses, "push ke-257 harus gagal:\n{}", k);
        for p in ["TOMBOL Space Escape", "JENDELA bool", "PENUH", "256"] {
            assert!(k.contains(p), "keluaran tidak memuat `{}`:\n{}", p, k);
        }
        assert!(!k.contains("TIDAK SAMPAI"), "{}", k);
    }
    let r = Ruang::baru();
    let (k, sukses) = r.jalankan("    canvas::nama_tombol(\"spasii\")");
    assert!(!sukses && k.contains("Space"), "nama asing harus ditolak dengan saran:\n{}", k);
}

#[test]
fn pop_tanpa_push_ditolak_bukan_diabaikan() {
    let r = Ruang::baru();
    let (keluaran, sukses) = r.jalankan("    let c = Canvas::new(4, 4, \"w\")\n    c.pop()");
    assert!(!sukses, "pop tanpa push seharusnya ditolak:\n{}", keluaran);
    assert!(
        keluaran.contains("tidak ada keadaan tersimpan"),
        "penolakannya harus menyebutkan sebabnya:\n{}", keluaran
    );
}

/// PNG yang ditulis harus bisa dibuka pembaca LAIN, dan isinya harus sama
/// persis dengan framebuffer-nya.
#[test]
fn png_bisa_dibaca_balik_dan_pikselnya_sama() {
    if !ada_python() {
        eprintln!("dilewati: `python3` tidak ada di PATH");
        return;
    }
    let r = Ruang::baru();
    let png = r.0.join("a.png");
    let bmp = r.0.join("a.bmp");
    let (keluaran, sukses) = r.jalankan(&format!(
        r#"    let c = Canvas::new(64, 48, "w")
    c.fill_linear_gradient(0, 0, 64, 48, 0x203050, 0xE0A030)
    c.fill_ellipse(32.0, 24.0, 20.0, 14.0, 0x40C080)
    c.line_aa(2.0, 46.0, 62.0, 2.0, 0xFF4060)
    c.draw_text_scaled(4, 4, "JRG", 2, 0xFFFFFF)
    wok(c.save_png("{}"))
    wok(c.save_bmp("{}"))"#,
        png.display(), bmp.display()
    ));
    assert!(sukses, "program gagal:\n{}", keluaran);

    let skrip = format!(r#"
import struct, zlib, sys
d = open({png:?}, 'rb').read()
assert d[:8] == b'\x89PNG\r\n\x1a\n', 'tanda tangan PNG salah'
i = 8; idat = b''; w = h = 0
while i < len(d):
    n = struct.unpack('>I', d[i:i+4])[0]; t = d[i+4:i+8]; isi = d[i+8:i+8+n]
    crc = struct.unpack('>I', d[i+8+n:i+12+n])[0]
    assert zlib.crc32(t+isi) & 0xFFFFFFFF == crc, 'CRC %s salah' % t
    if t == b'IHDR':
        w, h, bd, ct, cm, fm, il = struct.unpack('>IIBBBBB', isi)
        assert (bd, ct, cm, fm, il) == (8, 6, 0, 0, 0), 'IHDR tak terduga'
    if t == b'IDAT': idat += isi
    i += 12 + n
raw = zlib.decompress(idat)
bpp = 4; stride = w*bpp; png = []; prev = bytearray(stride); p = 0
def paeth(a,b,c):
    pp=a+b-c; pa,pb,pc=abs(pp-a),abs(pp-b),abs(pp-c)
    return a if pa<=pb and pa<=pc else (b if pb<=pc else c)
for y in range(h):
    f = raw[p]; p += 1; line = bytearray(raw[p:p+stride]); p += stride
    for x in range(stride):
        a = line[x-bpp] if x>=bpp else 0; b = prev[x]; c = prev[x-bpp] if x>=bpp else 0
        if f==1: line[x]=(line[x]+a)&255
        elif f==2: line[x]=(line[x]+b)&255
        elif f==3: line[x]=(line[x]+(a+b)//2)&255
        elif f==4: line[x]=(line[x]+paeth(a,b,c))&255
    png.append(bytes(line)); prev = line
b = open({bmp:?}, 'rb').read()
off = struct.unpack('<I', b[10:14])[0]
bw, bh = struct.unpack('<ii', b[18:26])
atas = bh < 0; bh = abs(bh)
assert (bw, bh) == (w, h), 'ukuran PNG dan BMP beda'
bstride = (bw*3+3)//4*4
beda = 0
for y in range(h):
    yy = y if atas else (bh-1-y)
    brow = b[off+yy*bstride:]
    for x in range(w):
        pr, pg, pb, pa = png[y][x*4:x*4+4]
        bb, bg, br = brow[x*3], brow[x*3+1], brow[x*3+2]
        assert pa == 255, 'alpha bukan 255 di (%d,%d)' % (x, y)
        if (pr, pg, pb) != (br, bg, bb): beda += 1
assert beda == 0, '%d piksel beda antara PNG dan BMP' % beda
print('OK %dx%d png=%d bmp=%d' % (w, h, len(d), len(b)))
"#, png = png.to_string_lossy(), bmp = bmp.to_string_lossy());

    let out = Command::new("python3").arg("-c").arg(&skrip).output().expect("jalankan python3");
    let pesan = format!("{}{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "PNG tidak lolos pembacaan balik:\n{}", pesan);
    assert!(pesan.contains("OK 64x48"), "hasil tak terduga:\n{}", pesan);

    // PNG harus benar-benar TERKOMPRESI. Kalau suatu saat deflate-nya
    // diganti blok tersimpan (yang tetap sah), berkasnya akan membengkak
    // jadi sebesar BMP dan tidak ada yang gagal — ini yang menangkapnya.
    let (bp, bb) = (
        std::fs::metadata(&png).map(|m| m.len()).unwrap_or(0),
        std::fs::metadata(&bmp).map(|m| m.len()).unwrap_or(0),
    );
    // Ambangnya sengaja longgar dan bukan angka bulat yang enak dilihat: gambar
    // uji ini kecil (64x48) dan hampir seluruhnya gradien, jadi entropinya …
    assert!(
        bp * 3 < bb * 2,
        "PNG ({} byte) tidak cukup lebih kecil dari BMP ({} byte) — \
         kompresi deflate-nya sepertinya tidak jalan", bp, bb
    );
}

/// Kanvas harus menghasilkan gambar yang IDENTIK di dua backend.
#[test]
fn kanvas_menggambar_identik_di_dua_backend() {
    let r = Ruang::baru();
    let a = r.0.join("interp.bmp");
    let b = r.0.join("vm.bmp");
    let sumber = r.0.join("gambar.jrg");
    // Dipilih supaya SETIAP jalur ikut terpakai: gradien (per piksel),
    // poligon ber-anti-alias (cakupan pecahan), garis AA (Xiaolin Wu),
    // transformasi bertumpuk, pemotongan, kurva, dan teks.
    std::fs::write(&sumber, format!(
        r#"package main
pub fn main() {{
    let c = Canvas::new(200, 150, "b")
    c.fill_linear_gradient(0, 0, 200, 150, 0x203050, 0xE0A030)
    c.fill_radial_gradient(60.0, 40.0, 90.0, 0x40FFFFFF, 0x00000000)
    c.fill_round_rect(20.0, 20.0, 90.0, 60.0, 12.0, 0xC0FF4060)
    c.fill_ellipse(150.0, 100.0, 35.0, 22.0, 0x40C080)
    c.line_aa(5.0, 145.0, 195.0, 5.0, 0xFFFFFF)
    c.push()
    c.translate(100.0, 75.0)
    c.rotate(25.0)
    c.scale(1.2, 0.8)
    c.fill_poly([[-20,-12],[20,-12],[20,12],[-20,12]], 0x80FFD060)
    c.pop()
    c.clip(0, 0, 120, 150)
    c.fill_circle(120, 30, 30, 0xFF00FF)
    c.clip_reset()
    c.draw_bezier([[10,120],[60,60],[120,140],[190,80]], 0x60E0FF)
    c.draw_arc(60.0, 110.0, 30.0, 20.0, 300.0, 0xFFAA00)
    c.draw_text_scaled(24, 100, "JRG", 2, 0xFFFFFF)
    wok(c.save_bmp(env::get_or("KELUAR", "/dev/null")))
}}
"#)).expect("tulis sumber");

    for (vm, keluar) in [(false, &a), (true, &b)] {
        let mut c = Command::new(jrgc());
        c.arg(&sumber).arg("--sopan").env("KELUAR", keluar);
        if vm { c.arg("--vm"); }
        let out = c.output().expect("jalankan jrgc");
        assert!(
            out.status.success(),
            "gagal di backend {}:\n{}{}",
            if vm { "vm" } else { "interp" },
            String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)
        );
    }
    let (ia, ib) = (
        std::fs::read(&a).expect("baca hasil interpreter"),
        std::fs::read(&b).expect("baca hasil vm"),
    );
    assert_eq!(ia.len(), ib.len(), "ukuran gambar beda antar-backend");
    let beda = ia.iter().zip(ib.iter()).filter(|(p, q)| p != q).count();
    assert_eq!(
        beda, 0,
        "{} byte berbeda antara gambar interpreter dan VM, padahal keduanya \
         memakai jalur gambar yang sama", beda
    );
}

/// Pembaca PNG, diuji terhadap berkas dari PERKAKAS LAIN.
mod png_baca {
    use jrgc::canvas::png_baca::baca;

    /// PNG `rgba` (4x3), dibuat PIL — bukan oleh penulis PNG proyek ini.
    const RGBA_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x03,
        0x08, 0x06, 0x00, 0x00, 0x00, 0xB4, 0xF4, 0xAE, 0xC6, 0x00, 0x00, 0x00,
        0x38, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x05, 0xC1, 0xA1, 0x11, 0x80,
        0x30, 0x14, 0x05, 0xC1, 0x7B, 0x33, 0xB8, 0x58, 0xF4, 0xD7, 0x78, 0x2A,
        0xC0, 0x45, 0xE2, 0xD3, 0x4C, 0xD2, 0x0C, 0x95, 0xA5, 0x8F, 0x63, 0x17,
        0x41, 0x64, 0x82, 0xA8, 0x1E, 0xED, 0x2C, 0xAE, 0xBB, 0x7A, 0xBD, 0x63,
        0x27, 0x1F, 0x51, 0x0C, 0x62, 0x60, 0xCD, 0xF5, 0xFC, 0xEE, 0x80, 0x12,
        0x91, 0x3A, 0x96, 0x58, 0x07, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    const RGBA_PIKSEL: &[u32] = &[
        0xFFFF0000, 0x8000FF00, 0x000000FF, 0xFFFFFFFF,
        0xFF0A141E, 0xC828323C, 0x6446505A, 0xFF000000,
        0xFFFFFF00, 0xFF00FFFF, 0xFFFF00FF, 0x40808080,
    ];


    /// PNG `rgb` (3x2), dibuat PIL — bukan oleh penulis PNG proyek ini.
    const RGB_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xF1, 0x4D, 0x00, 0x00, 0x00,
        0x18, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x64, 0x64, 0x62, 0x86,
        0x00, 0x86, 0x5F, 0xBF, 0xFF, 0x30, 0x30, 0x30, 0xFC, 0xFF, 0xFF, 0x1F,
        0x00, 0x1F, 0x05, 0x06, 0x08, 0x10, 0x7A, 0xC7, 0x43, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    const RGB_PIKSEL: &[u32] = &[
        0xFF010203, 0xFF040506, 0xFF070809, 0xFFFAFBFC,
        0xFF000000, 0xFFFFFFFF,
    ];


    /// PNG `palet` (4x2), dibuat PIL — bukan oleh penulis PNG proyek ini.
    const PALET_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x02,
        0x08, 0x03, 0x00, 0x00, 0x00, 0x48, 0x76, 0x8D, 0x51, 0x00, 0x00, 0x03,
        0x00, 0x50, 0x4C, 0x54, 0x45, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00,
        0x00, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x17, 0xDD, 0xF5, 0x8B, 0x00, 0x00, 0x00,
        0x12, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0x64, 0x62,
        0x66, 0x60, 0x66, 0x62, 0x64, 0x00, 0x00, 0x00, 0x46, 0x00, 0x0D, 0x5E,
        0x72, 0x8B, 0x87, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
        0x42, 0x60, 0x82,
    ];
    const PALET_PIKSEL: &[u32] = &[
        0xFFFF0000, 0xFF00FF00, 0xFF0000FF, 0xFFFFFF00,
        0xFFFFFF00, 0xFF0000FF, 0xFF00FF00, 0xFFFF0000,
    ];


    /// PNG `abu` (3x3), dibuat PIL — bukan oleh penulis PNG proyek ini.
    const ABU_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03,
        0x08, 0x00, 0x00, 0x00, 0x00, 0x73, 0x43, 0xEA, 0x63, 0x00, 0x00, 0x00,
        0x14, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x64, 0x70, 0x70, 0x60,
        0x38, 0xF0, 0x5F, 0x81, 0x65, 0xC1, 0x83, 0x03, 0x00, 0x15, 0x22, 0x04,
        0xA5, 0x3A, 0xC7, 0x0D, 0x9E, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    const ABU_PIKSEL: &[u32] = &[
        0xFF000000, 0xFF404040, 0xFF808080, 0xFFC0C0C0,
        0xFFFFFFFF, 0xFF202020, 0xFF606060, 0xFFA0A0A0,
        0xFFE0E0E0,
    ];

    /// Bandingkan hasil baca dengan piksel yang diketahui, per piksel.
    fn cocok(nama: &str, data: &[u8], lebar: usize, tinggi: usize, harap: &[u32]) {
        let (w, h, piksel) = baca(data).unwrap_or_else(|e| panic!("{}: {}", nama, e));
        assert_eq!((w, h), (lebar, tinggi), "{}: ukurannya beda", nama);
        assert_eq!(piksel.len(), harap.len(), "{}: jumlah pikselnya beda", nama);
        for (i, (dapat, mau)) in piksel.iter().zip(harap).enumerate() {
            assert_eq!(dapat, mau,
                "{}: piksel ke-{} ({},{}) beda — dapat {:#010X}, harusnya {:#010X}",
                nama, i, i % lebar, i / lebar, dapat, mau);
        }
    }

    #[test]
    fn rgba_huffman_dinamis() {
        // Yang paling penting dari keempatnya: alirannya BTYPE=2, bentuk yang
        // tidak pernah dihasilkan penulis proyek ini.
        cocok("rgba", RGBA_PNG, 4, 3, RGBA_PIKSEL);
    }

    #[test]
    fn rgb_tanpa_alfa() { cocok("rgb", RGB_PNG, 3, 2, RGB_PIKSEL); }

    #[test]
    fn berpalet() { cocok("palet", PALET_PNG, 4, 2, PALET_PIKSEL); }

    #[test]
    fn abu_abu() { cocok("abu", ABU_PNG, 3, 3, ABU_PIKSEL); }

    #[test]
    fn putar_balik_dengan_penulis_sendiri() {
        // Putar-balik TETAP diuji — ia menjaga separuh yang lain: sebuah perubahan
        // di penulis yang membuat berkasnya tidak terbaca lagi.
        let (w, h) = (7usize, 5usize);
        let mut piksel = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                // Gradien + alfa bervariasi, supaya pemilih filter per-baris
                // punya sesuatu untuk dipilih dan kelima filter berpeluang
                // terpakai.
                let a = ((x * 37 + y * 11) % 256) as u32;
                let r = ((x * 30) % 256) as u32;
                let g = ((y * 50) % 256) as u32;
                let b = (((x + y) * 20) % 256) as u32;
                piksel[y * w + x] = (a << 24) | (r << 16) | (g << 8) | b;
            }
        }
        let bytes = jrgc::canvas::png::susun(w, h, &piksel);
        let (rw, rh, kembali) = baca(&bytes).expect("PNG tulisan sendiri harusnya terbaca");
        assert_eq!((rw, rh), (w, h));
        assert_eq!(kembali, piksel, "putar-balik tidak identik");
    }

    #[test]
    fn berkas_rusak_ditolak_bukan_dibaca_jadi_sampah() {
        // Tiap kerusakan harus MELAPOR, bukan menghasilkan piksel yang salah.
        let kasus: Vec<(&str, Box<dyn Fn(&mut Vec<u8>)>)> = vec![
            ("tanda tangan", Box::new(|d: &mut Vec<u8>| d[1] = b'X')),
            ("CRC IHDR",     Box::new(|d: &mut Vec<u8>| d[29] ^= 0xFF)),
            ("data IDAT",    Box::new(|d: &mut Vec<u8>| { let n = d.len(); d[n - 10] ^= 0xFF; })),
            ("terpotong",    Box::new(|d: &mut Vec<u8>| d.truncate(40))),
        ];
        for (nama, rusak) in kasus {
            let mut d = RGBA_PNG.to_vec();
            rusak(&mut d);
            assert!(baca(&d).is_err(),
                "kerusakan `{}` harusnya ditolak, malah menghasilkan gambar", nama);
        }
    }

    #[test]
    fn yang_belum_didukung_menolak_dengan_menyebut_sebabnya() {
        // Sebuah penolakan yang tidak menyebut sebabnya sama tidak bergunanya
        // dengan hasil yang salah: pembacanya tidak tahu harus mengubah apa.
        let mut d = RGBA_PNG.to_vec();
        d[24] = 16;                    // IHDR: kedalaman bit
        perbaiki_crc_ihdr(&mut d);
        let e = baca(&d).unwrap_err();
        assert!(e.contains("16 bit"), "pesannya harus menyebut 16 bit: {}", e);

        let mut d = RGBA_PNG.to_vec();
        d[28] = 1;                     // IHDR: interlace
        perbaiki_crc_ihdr(&mut d);
        let e = baca(&d).unwrap_err();
        assert!(e.to_lowercase().contains("interlace"),
            "pesannya harus menyebut interlace: {}", e);
    }

    #[test]
    fn adler32_menangkap_yang_crc_tidak_bisa() {
        // Kasus yang paling penting di berkas ini, dan yang paling gampang tidak
        // ditulis.
        let mut d = RGBA_PNG.to_vec();
        let (awal, panjang) = cari_potongan(&d, b"IDAT").expect("IDAT harusnya ada");
        d[awal + panjang - 1] ^= 0xFF;                 // byte terakhir = Adler-32
        let crc = crc32_uji(&d[awal - 4..awal + panjang]);
        d[awal + panjang..awal + panjang + 4].copy_from_slice(&crc.to_be_bytes());

        let e = baca(&d).unwrap_err();
        assert!(e.contains("Adler-32"),
            "kerusakan yang lolos CRC harus ditangkap Adler-32; pesannya: {}", e);
    }

    #[test]
    fn aliran_deflate_rusak_dilaporkan_bukan_jadi_piksel() {
        // Satu bit di TENGAH data terkompresi, dengan CRC dibetulkan.
        let mut d = RGBA_PNG.to_vec();
        let (awal, panjang) = cari_potongan(&d, b"IDAT").expect("IDAT harusnya ada");
        d[awal + panjang / 2] ^= 0x55;
        let crc = crc32_uji(&d[awal - 4..awal + panjang]);
        d[awal + panjang..awal + panjang + 4].copy_from_slice(&crc.to_be_bytes());

        // Yang dituntut cuma: TIDAK menghasilkan gambar diam-diam.
        assert!(baca(&d).is_err(),
            "aliran deflate yang rusak harusnya dilaporkan, bukan dibaca jadi piksel");
    }

    /// Cari potongan bernama `jenis`; kembalikan (awal isi, panjang isi).
    fn cari_potongan(d: &[u8], jenis: &[u8; 4]) -> Option<(usize, usize)> {
        let mut i = 8usize;
        while i + 8 <= d.len() {
            let n = u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
            if &d[i + 4..i + 8] == &jenis[..] {
                return Some((i + 8, n));
            }
            i += 12 + n;
        }
        None
    }

    /// Hitung ulang CRC IHDR sesudah isinya diubah.
    fn perbaiki_crc_ihdr(d: &mut [u8]) {
        let crc = crc32_uji(&d[12..29]);
        d[29..33].copy_from_slice(&crc.to_be_bytes());
    }

    /// CRC32 versi tes sendiri — sengaja TIDAK memakai milik `png.rs`.
    fn crc32_uji(data: &[u8]) -> u32 {
        let mut tabel = [0u32; 256];
        for (i, t) in tabel.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 { c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 }; }
            *t = c;
        }
        let mut c = 0xFFFF_FFFFu32;
        for &b in data { c = tabel[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8); }
        c ^ 0xFFFF_FFFF
    }

}

/// Nama tombol yang tidak dikenal ditolak, dengan kalimat yang sama di dua
/// mesin; dulu `c.key_held("spasi")` menjawab `false` selamanya.
#[test]
fn nama_tombol_asing_ditolak_dan_yang_dikenal_diterima() {
    let r = Ruang::baru();
    let jalan = |badan: &str, vm: bool| -> (String, String, bool) {
        let f = r.0.join("tombol.jrg");
        std::fs::write(&f, format!(
            "package main\npub fn main() {{\n    let c = Canvas::new(4, 4, \"w\")\n    wok(\"mulai\")\n{}\n}}\n",
            badan)).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.arg(&f).arg("--sopan").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
        if vm { cmd.arg("--vm"); }
        let out = cmd.output().expect("jalankan");
        (String::from_utf8_lossy(&out.stdout).into_owned(),
         String::from_utf8_lossy(&out.stderr).into_owned(), out.status.success())
    };
    let baris_galat = |e: &str| e.lines().find(|l| l.contains("error")).unwrap_or("").to_string();

    // Sisi POSITIF: tiap nama kanonik, huruf kecilnya, dan tiap alias diterima.
    let mut sah: Vec<String> = Vec::new();
    for t in jrgc::canvas::TOMBOL {
        sah.push(t.to_string());
        sah.push(t.to_lowercase());
    }
    for a in [" ", "ArrowLeft", "return", "esc", "Shift", "ctrl", "Control", "alt", "0", "9", "space", "enter"] {
        sah.push(a.to_string());
    }
    let badan: String = sah.iter()
        .map(|t| format!("    if c.key_pressed({0:?}) || c.key_held({0:?}) {{ wok(\"ditekan\") }}\n", t))
        .collect();
    for vm in [false, true] {
        let (o, e, ok) = jalan(&badan, vm);
        assert!(ok && o == "mulai\n", "nama tombol yang sah ditolak (vm={}):\n{}{}", vm, o, e);
    }

    // Sisi NEGATIF: ditolak di kedua pintu, sesudah keluaran sebelumnya.
    for (badan, perlu) in [
        ("    wok(c.key_held(\"spasi\"))", "`spasi` bukan nama tombol"),
        ("    wok(c.key_pressed(\"Spase\"))", "maksudnya `Space`?"),
        ("    wok(canvas::key_pressed(c, \"kiri\"))", "`kiri` bukan nama tombol"),
        ("    wok(c.key_held(\"\"))", "bukan nama tombol"),
    ] {
        let (oi, ei, oki) = jalan(badan, false);
        let (ov, ev, okv) = jalan(badan, true);
        assert!(!oki && !okv, "`{}` harus ditolak di dua mesin:\n{}\n{}", badan, ei, ev);
        assert_eq!((oi.as_str(), ov.as_str()), ("mulai\n", "mulai\n"), "keluaran sebelum galat hilang: {}", badan);
        assert!(ei.contains(perlu) && ei.contains("argumen ke-2"), "kalimatnya kurang untuk `{}`:\n{}", badan, ei);
        assert_eq!(baris_galat(&ei), baris_galat(&ev), "kalimat beda antar-mesin untuk `{}`", badan);
    }
}

/// `std/mesin`: tiap modulnya dijalankan di dua mesin dan jawabannya diperiksa —
/// termasuk PIKSEL untuk yang menggambar, karena keluaran yang identik membuktikan
/// paritas, bukan kebenaran.
mod mesin {
    use super::*;

    fn jalankan_sumber(src: &str, vm: bool) -> (String, bool) {
        let r = Ruang::baru();
        let f = r.0.join("mesin.jrg");
        std::fs::write(&f, src).expect("tulis sumber");
        let mut cmd = Command::new(jrgc());
        cmd.arg(&f).arg("--sopan").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
        if vm { cmd.arg("--vm"); }
        let out = cmd.output().expect("jalankan");
        (format!("{}{}", String::from_utf8_lossy(&out.stdout),
                 String::from_utf8_lossy(&out.stderr)), out.status.success())
    }

    const PROGRAM: &str = r##"package main

import "std/mesin"
import "std/mesin/vek"
import "std/mesin/warna"
import "std/mesin/tween"
import "std/mesin/masukan"
import "std/mesin/fisika"
import "std/mesin/kamera"
import "std/mesin/sprite"
import "std/mesin/peta"
import "std/mesin/partikel"
import "std/mesin/ui"
import "std/mesin/adegan"

fn hitung(c, x0, y0, x1, y1) {
    var n = 0
    for _, y in range(y0, y1) {
        for _, x in range(x0, x1) {
            let p = canvas::get_pixel(c, x, y)
            if p.r > 200 { n = n + 1 }
        }
    }
    return n
}

fn perbarui(k, m, dt) {
    var s = k
    if m.tekan("Space") { s.lompat = k.lompat + 1 }
    s.x = k.x + m.arah("Left", "Right", "Up", "Down").x * dt
    return s
}

struct K { pub x: float, pub lompat: int }

fn gambar(c, k) { canvas::fill_rect(c, 0, 0, 2, 2, warna::MERAH) }

fn adegan_main(k, m, dt) {
    if m.tekan("P") { return [k + 1, adegan::Pindah::Dorong(adegan::baru("jeda", 0, adegan_jeda, gambar_jeda))] }
    return [k + 1, adegan::Pindah::Tetap]
}
fn adegan_jeda(k, m, dt) {
    if m.tekan("P") { return [k, adegan::Pindah::Lepas] }
    return [k + 1, adegan::Pindah::Tetap]
}
fn gambar_main(c, k) { canvas::fill_rect(c, 0, 0, 4, 4, warna::MERAH) }
fn gambar_jeda(c, k) { canvas::fill_rect(c, 4, 0, 4, 4, warna::MERAH) }

pub fn main() {
    // vek
    let a = vek::baru(3, 4)
    wok("VEK", a, a.panjang(), a + vek::baru(1, 1), a * 2, -a, a == vek::baru(3.0, 4.0))
    wok("VEK2", vek::baru(1, 0).putar(90).sudut(), a.normal(), a.batasi(1.0).panjang())
    // warna
    wok("WARNA", warna::hex(0x102030), warna::MERAH.campur(warna::PUTIH, 0.5), warna::BIRU.alfa(10))
    // tween
    let t = tween::baru(0, 10, 2, tween::masuk_kuad).maju(1.0)
    wok("TWEEN", t.nilai(), t.selesai(), t.maju(5).nilai(), t.maju(5).selesai(), tween::pantul(1.0))
    // masukan: selisih ditekan/dilepas
    let m1 = masukan::buatan(masukan::kosong(), ["space", "Left"])
    let m2 = masukan::buatan(m1, ["Space"])
    wok("MASUKAN", m1.tekan("Space"), m2.tekan("Space"), m2.tahan("Space"), m2.lepas("Left"), m1.arah("Left", "Right", "Up", "Down"))
    // mesin: simulasi deterministik
    let g = mesin::baru("uji", 8, 8, K { x: 0.0, lompat: 0 }, perbarui, gambar).dengan_dt(0.5)
    let h = g.simulasi([["Space"], ["Space"], [], ["Space", "Right"], ["Right"]])
    wok("MESIN", h.langkah_ke, h.keadaan.lompat, h.keadaan.x)
    // fisika
    let k1 = fisika::kotak(0, 0, 10, 10)
    let k2 = fisika::kotak(8, 0, 10, 10)
    wok("FISIKA", k1.tabrak(k2), k1.tabrak(fisika::kotak(10, 0, 5, 5)), k1.dorong_keluar(k2),
        fisika::lingkaran(vek::baru(12, 5), 3).tabrak(k1), fisika::benda(vek::nol()).dengan_perc(vek::baru(0, 10)).maju(1).pos)
    // peta
    let p = peta::dari_teks(["#..", "#.#"], 16, {"#": 1, ".": 0})
    wok("PETA", p.di(0, 0), p.di(1, 0), p.di(3, 0), p.tabrak(fisika::kotak(17, 1, 10, 10), [1]),
        p.tabrak(fisika::kotak(20, 18, 10, 10), [1]), p.tabrak(fisika::kotak(33, 1, 10, 10), [1]), p.tabrak(fisika::kotak(28, 20, 10, 5), [1]),
        p.tabrak(fisika::kotak(-5, 0, 4, 4), [1]), p.atur(1, 0, 5).di(1, 0))
    // partikel: deterministik
    let s = partikel::baru(vek::baru(0, 10), 42).pancar(5, vek::nol(), 0, 30, 10, 1.0, warna::MERAH)
    wok("PARTIKEL", s.jumlah(), s.maju(0.5).jumlah(), s.maju(2.0).jumlah(), s.isi[0].kec)
    // sprite animasi
    let an = sprite::animasi([3, 4, 5], 10, false)
    wok("ANIMASI", an.petak(), an.maju(0.15).petak(), an.maju(9.0).petak(), an.maju(9.0).selesai())
    // kamera: titik pusat dunia ada di tengah layar, zoom 2 menggandakan ukuran
    let kam = kamera::baru(40, 40, vek::baru(100, 100)).dengan_zoom(2)
    wok("KAMERA", kam.ke_layar(vek::baru(100, 100)), kam.ke_dunia(kam.ke_layar(vek::baru(107, 93))))
    let c = Canvas::new(40, 40)
    kam.pasang(c)
    canvas::fill_rect(c, 100, 100, 3, 3, warna::MERAH)
    kam.lepas(c)
    canvas::fill_rect(c, 0, 0, 2, 2, warna::MERAH)
    wok("KAMERA-PIKSEL", hitung(c, 20, 20, 26, 26), hitung(c, 0, 0, 40, 40))
    // sprite: blit petak ke-1
    let lembar = Canvas::new(8, 4)
    canvas::fill_rect(lembar, 4, 0, 4, 4, warna::MERAH)
    let sp = sprite::dari_kanvas(lembar, 4, 4)
    let c2 = Canvas::new(10, 10)
    sp.gambar(c2, 0, 0, 0)
    sp.gambar(c2, 1, 5, 5)
    wok("SPRITE", sp.jumlah(), hitung(c2, 0, 0, 5, 5), hitung(c2, 5, 5, 10, 10))
    // ui
    let tb = ui::tombol(0, 0, 20, 10, "OK")
    let mk = masukan::buatan_tetikus(masukan::kosong(), [], 5, 5, [true, false, false])
    wok("UI", tb.disorot(mk), tb.diklik(mk), tb.diklik(masukan::buatan_tetikus(mk, [], 5, 5, [true, false, false])), ui::lebar_teks("abc", 2))
    let c3 = Canvas::new(20, 4)
    ui::bilah(c3, 0, 0, 20, 4, 0.5, warna::MERAH, warna::HITAM)
    wok("BILAH", hitung(c3, 0, 0, 20, 4))
    // adegan: dorong jeda, lalu lepas
    let gm = adegan::sebagai_game("a", 8, 4, adegan::baru("main", 0, adegan_main, gambar_main))
    let g1 = gm.simulasi([[], ["P"], [], []])
    let c4 = Canvas::new(8, 4)
    g1.gambar_ke(c4)
    wok("ADEGAN", g1.keadaan.kedalaman(), g1.keadaan.atas().nama, g1.keadaan.atas().keadaan, hitung(c4, 0, 0, 8, 4))
    let g2 = g1.simulasi([[], ["P"]])
    wok("ADEGAN2", g2.keadaan.kedalaman(), g2.keadaan.atas().keadaan)
}"##;

    const HARAP: &str = "VEK Vek2(3.0, 4.0) 5.0 Vek2(4.0, 5.0) Vek2(6.0, 8.0) Vek2(-3.0, -4.0) true\nVEK2 90.0 Vek2(0.6, 0.8) 1.0\nWARNA Warna(16, 32, 48, 255) Warna(238, 153, 151, 255) Warna(38, 139, 210, 10)\nTWEEN 2.5 false 10.0 true 1.0\nMASUKAN true false true true Vek2(-1.0, 0.0)\nMESIN 5 2 1.0\nFISIKA true false Option::Some(Vek2(-2.0, 0.0)) true Vek2(0.0, 10.0)\nPETA Option::Some(1) Option::Some(0) Option::None false false false true true Option::Some(5)\nPARTIKEL 5 5 0 Vek2(6.8080812143588725, -3.921678395755214)\nANIMASI 3 4 5 true\nKAMERA Vek2(20.0, 20.0) Vek2(107.0, 93.0)\nKAMERA-PIKSEL 36 40\nSPRITE 2 0 16\nUI true true false 36\nBILAH 40\nADEGAN 2 jeda 2 32\nADEGAN2 1 2";

    #[test]
    fn tiap_modul_menjawab_benar_di_dua_mesin() {
        for vm in [false, true] {
            let (k, sukses) = jalankan_sumber(PROGRAM, vm);
            assert!(sukses, "gagal di {}:\n{}", if vm { "--vm" } else { "interp" }, k);
            assert_eq!(k.trim(), HARAP, "jawaban `std/mesin` salah di {}", if vm { "--vm" } else { "interp" });
        }
    }

    /// Salah pakai DITOLAK dengan kalimat yang menyebut sebabnya, sama di dua mesin.
    #[test]
    fn salah_pakai_ditolak_dengan_kalimat() {
        let kasus: &[(&str, &str, &str)] = &[
            ("std/mesin/vek", "wok(vek::nol().normal())", "vektor nol tidak punya arah"),
            ("std/mesin/vek", "wok(vek::baru(1, 2) / 0)", "pembagi nol"),
            ("std/mesin/warna", "wok(warna::rgb(300, 0, 0))", "harus 0-255, dapat 300"),
            ("std/mesin/masukan", r#"wok(masukan::buatan(masukan::kosong(), ["Spasii"]))"#, "Spasii"),
            ("std/mesin", r#"wok(mesin::baru("a", 0, 4, 0, |k, m, dt| k, |c, k| k))"#, "bilangan bulat positif"),
            ("std/mesin", r#"wok(mesin::baru("a", 4, 4, 0, 5, |c, k| k))"#, "argumen ke-5 harus fungsi"),
            ("std/mesin", r#"mesin::baru("a", 4, 4, 0, |k, m, dt| k, |c, k| k).jalankan()"#, "tanpa jendela"),
            ("std/mesin", r#"wok(mesin::baru("a", 4, 4, 0, |k, m, dt| k, |c, k| k).dengan_selesai(|k| 1).simulasi([[]]))"#,
             "harus menjawab bool"),
            ("std/mesin/fisika", "wok(fisika::kotak(0, 0, -1, 2))", "tidak boleh negatif"),
            ("std/mesin/peta", r##"wok(peta::dari_teks(["#.", "#"], 8, {"#": 1, ".": 0}))"##, "baris 1 panjangnya 1"),
            ("std/mesin/peta", r##"wok(peta::dari_teks(["#x"], 8, {"#": 1}))"##, "karakter `x`"),
            ("std/mesin/peta", "wok(peta::baru(2, 2, 8, 0).atur(5, 0, 1))", "di luar peta"),
            ("std/mesin/sprite", "wok(sprite::dari_kanvas(Canvas::new(8, 4), 4, 4).gambar(Canvas::new(4, 4), 2, 0, 0))",
             "petak 2 tidak ada"),
            ("std/mesin/sprite", "wok(sprite::dari_kanvas(Canvas::new(2, 2), 4, 4))", "lebih kecil dari satu petak"),
            ("std/mesin/kamera", "wok(kamera::baru(4, 4, vek_nol()).dengan_zoom(0))", "zoom harus angka positif"),
            ("std/mesin/tween", "wok(tween::baru(0, 1, 0, tween::linear))", "durasi` harus positif"),
            ("std/mesin", r#"wok(mesin::baru("a", 4, 4, 0, |k, m, dt| k, |c, k| k).putar([[1, 2]]))"#,
             "entri ke-0 bukan `[masukan, berapa_langkah]`"),
            ("std/mesin/peta", "wok(peta::baru(4, 4, 8, 1).gerak(fisika_kotak(), vek_nol(), [1]))", "sudah di dalam petak padat"),
            ("std/mesin/peta", "wok(peta::baru(4, 4, 8, 0).gerak(fisika_kotak(), [1, 0], [1]))", "geseran harus `Vek2`"),
            ("std/mesin/kamera", r#"wok(kamera::baru(4, 4, vek_nol()).batasi("a", 4))"#, "ukuran dunia harus angka"),
            ("std/mesin/adegan",
             r#"wok(adegan::panggung(adegan::baru("a", 0, |k, m, dt| [k, adegan::Pindah::Lepas], |c, k| k)).langkah(0, 0.1))"#,
             "tidak bisa `Lepas` adegan terakhir"),
        ];
        let mut salah = Vec::new();
        for (modul, badan, potongan) in kasus {
            let src = format!("package main\nimport \"{}\"\nimport \"std/mesin/vek\"\nimport \"std/mesin/fisika\"\n\
                fn vek_nol() {{ return vek::nol() }}\n\
                fn fisika_kotak() {{ return fisika::kotak(2, 2, 3, 3) }}\npub fn main() {{\n    {}\n}}\n", modul, badan);
            let (i, si) = jalankan_sumber(&src, false);
            let (v, sv) = jalankan_sumber(&src, true);
            let baris = |s: &str| s.lines().find(|l| l.starts_with("error")).unwrap_or("").to_string();
            if si || sv || !i.contains(potongan) || baris(&i) != baris(&v) || baris(&i).is_empty() {
                salah.push(format!("{} -> interp {}:{:?}\n   vm {}:{:?}", badan, si, baris(&i), sv, baris(&v)));
            }
        }
        assert!(salah.is_empty(), "salah pakai `std/mesin` tidak ditolak dengan benar:\n  {}", salah.join("\n  "));
    }

    /// Contoh game lengkap berjalan sama persis di dua mesin, dengan angka yang dikunci.
    #[test]
    fn contoh_game_tangkap_sama_di_dua_mesin() {
        let contoh = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/game_tangkap.jrg");
        let mut hasil = Vec::new();
        for vm in [false, true] {
            let mut cmd = Command::new(jrgc());
            cmd.arg(contoh).arg("--sopan").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
            if vm { cmd.arg("--vm"); }
            let out = cmd.output().expect("jalankan");
            let k = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(out.status.success(), "game_tangkap gagal:\n{}{}", k, String::from_utf8_lossy(&out.stderr));
            hasil.push(k);
        }
        assert!(hasil[0].contains("langkah: 1200 skor: 19 bintang di layar: 5"), "keluaran: {}", hasil[0]);
        assert_eq!(hasil[0], hasil[1]);
    }
    const PROGRAM_GERAK: &str = r##"package main

import "std/mesin/vek"
import "std/mesin/warna"
import "std/mesin/fisika"
import "std/mesin/peta"
import "std/mesin/kamera"
import "std/mesin/sprite"

fn hitung(c, x0, y0, x1, y1) {
    var n = 0
    for _, y in range(y0, y1) {
        for _, x in range(x0, x1) {
            if canvas::get_pixel(c, x, y).r > 200 { n = n + 1 }
        }
    }
    return n
}

fn gerak(p, x, y, w, h, dx, dy) {
    let r = p.gerak(fisika::kotak(x, y, w, h), vek::baru(dx, dy), [1])
    let k = r[0]
    let s = r[1]
    return [k.x, k.y, s.kiri, s.kanan, s.atas, s.bawah]
}

pub fn main() {
    // Dunia 10x10 petak ukuran 10: dinding di kolom 5, lantai di baris 8.
    let p = peta::dari_teks([
        "..........",
        "..........",
        ".....#....",
        ".....#....",
        ".....#....",
        ".....#....",
        ".....#....",
        ".....#....",
        "==========",
        "..........",
    ], 10, {"#": 1, "=": 1, ".": 0})
    // Ke kanan menabrak dinding: tepi kanan berhenti tepat di x = 50.
    wok("KANAN", gerak(p, 20, 30, 6, 6, 40, 0))
    // Ke kiri menabrak dinding dari kanan: tepi kiri berhenti di x = 60.
    wok("KIRI", gerak(p, 80, 30, 6, 6, -40, 0))
    // Jatuh ke lantai: tepi bawah berhenti di y = 80.
    wok("JATUH", gerak(p, 20, 10, 6, 6, 0, 200))
    // Gerak cepat TIDAK menembus dinding tipis (satu petak).
    wok("CEPAT", gerak(p, 10, 30, 6, 6, 500, 0))
    // Diagonal: x tertahan dinding, y tetap jatuh sampai lantai.
    wok("DIAGONAL", gerak(p, 30, 20, 6, 6, 30, 100))
    // Keluar peta di kiri atas: tepi peta dihitung padat.
    wok("TEPI", gerak(p, 5, 5, 4, 4, -20, -20))
    // Bebas: tidak menyentuh apa pun.
    wok("BEBAS", gerak(p, 10, 10, 4, 4, 5, 5))
    // Kamera dibatasi ke dunia 100x100 dengan layar 40x40: pusat 20..80.
    let k = kamera::baru(40, 40, vek::baru(0, 0))
    wok("KAMERA", k.batasi(100, 100).pusat, k.ikuti(vek::baru(500, 50), 1.0).batasi(100, 100).pusat,
        k.dengan_zoom(2).batasi(100, 100).pusat, k.batasi(30, 100).pusat)
    // Sprite dicerminkan: petak 4x4 yang cuma kolom kirinya merah jadi kolom kanan.
    let lembar = Canvas::new(4, 4)
    canvas::fill_rect(lembar, 0, 0, 1, 4, warna::MERAH)
    let sp = sprite::dari_kanvas(lembar, 4, 4)
    let c = Canvas::new(10, 4)
    sp.gambar(c, 0, 0, 0)
    sp.gambar_balik(c, 0, 5, 0)
    wok("BALIK", hitung(c, 0, 0, 1, 4), hitung(c, 5, 0, 8, 4), hitung(c, 8, 0, 9, 4), hitung(c, 0, 0, 10, 4))
    // Peta digambar dari lembar: petak bernilai 1 memakai petak sprite 0.
    let c2 = Canvas::new(100, 100)
    p.gambar_lembar(c2, sp, |v| v == 1 $ 0 : nil)
    wok("LEMBAR", hitung(c2, 0, 0, 100, 100))
}"##;

    const HARAP_GERAK: &str = "KANAN [44.0, 30.0, false, true, false, false]\nKIRI [60.0, 30.0, true, false, false, false]\nJATUH [20.0, 74.0, false, false, false, true]\nCEPAT [44.0, 30.0, false, true, false, false]\nDIAGONAL [44.0, 74.0, false, true, false, true]\nTEPI [0.0, 0.0, true, false, true, false]\nBEBAS [15.0, 15.0, false, false, false, false]\nKAMERA Vek2(20.0, 20.0) Vek2(80.0, 50.0) Vek2(10.0, 10.0) Vek2(15.0, 20.0)\nBALIK 4 0 4 8\nLEMBAR 64";

    /// `peta.gerak` berhenti TEPAT di tepi petak (termasuk gerak cepat yang tidak
    /// boleh menembus dinding tipis), kamera dibatasi ke dunia, sprite dicerminkan.
    /// Angka harapannya dihitung tangan lebih dulu.
    #[test]
    fn gerak_petak_kamera_dan_cermin_benar_di_dua_mesin() {
        for vm in [false, true] {
            let (k, sukses) = jalankan_sumber(PROGRAM_GERAK, vm);
            assert!(sukses, "gagal di {}:\n{}", if vm { "--vm" } else { "interp" }, k);
            assert_eq!(k.trim(), HARAP_GERAK, "jawaban salah di {}", if vm { "--vm" } else { "interp" });
        }
    }

    /// Contoh platformer berjalan sama persis di dua mesin, dengan angka yang dikunci.
    #[test]
    fn contoh_game_lompat_sama_di_dua_mesin() {
        let contoh = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/game_lompat.jrg");
        let mut hasil = Vec::new();
        for vm in [false, true] {
            let mut cmd = Command::new(jrgc());
            cmd.arg(contoh).arg("--sopan").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
            if vm { cmd.arg("--vm"); }
            let out = cmd.output().expect("jalankan");
            let k = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(out.status.success(), "game_lompat gagal:\n{}{}", k, String::from_utf8_lossy(&out.stderr));
            hasil.push(k);
        }
        assert!(hasil[0].contains("langkah: 600 koin: 1 pemain: 178.0 56.0 di tanah: true"), "keluaran: {}", hasil[0]);
        assert_eq!(hasil[0], hasil[1]);
    }
    const PROGRAM_REKAM: &str = r#"package main

import "std/mesin"
import "std/mesin/masukan"

struct K { pub x: float, pub lompat: int, pub klik: int }

fn perbarui(k, m, dt) {
    var s = k
    if m.tekan("Space") { s.lompat = k.lompat + 1 }
    if m.tetikus_tekan(0) { s.klik = k.klik + 1 }
    s.x = k.x + m.arah("Left", "Right", "Up", "Down").x * dt + m.tetikus.x * 0.001
    return s
}

fn gambar(c, k) {}

pub fn main() {
    let g = mesin::baru("rekam", 8, 8, K { x: 0.0, lompat: 0, klik: 0 }, perbarui, gambar).dengan_dt(0.5)
    // 3 langkah Space ditahan, 2 kosong, 4 Right: entri RLE = Space(tekan), Space(tahan)x2,
    // kosong(lepas), kosong, Right(tekan), Right(tahan)x3.
    let h = g.dengan_rekam().simulasi([["Space"], ["Space"], ["Space"], [], [], ["Right"], ["Right"], ["Right"], ["Right"]])
    wok("REKAMAN", len(h.rekaman), h.rekaman.map(|e| e[1]))
    let u = g.putar(h.rekaman)
    wok("PUTAR", u.langkah_ke == h.langkah_ke, u.keadaan == h.keadaan, u.keadaan.lompat, u.keadaan.x)
    // Tetikus ikut terekam dan terputar.
    let m0 = masukan::kosong()
    let m1 = masukan::buatan_tetikus(m0, [], 100, 5, [true, false, false])
    let m2 = masukan::buatan_tetikus(m1, [], 100, 5, [true, false, false])
    let gt = g.dengan_rekam().langkah(m1).langkah(m2).langkah(m2)
    let ut = g.putar(gt.rekaman)
    wok("TETIKUS", gt.rekaman.map(|e| e[1]), ut.keadaan == gt.keadaan, ut.keadaan.klik, ut.keadaan.x)
    // Tanpa dengan_rekam tidak ada yang direkam.
    wok("TANPA", len(g.simulasi([["Space"], []]).rekaman))
    // Sesi yang lebih panjang: dua segmen 600 langkah jadi EMPAT entri saja.
    var r = []
    for i in range(0, 1200) {
        if (i / 600) % 2 == 0 { r = r + [["Right"]] } else { r = r + [[]] }
    }
    let p = g.dengan_rekam().simulasi(r)
    wok("PANJANG", p.langkah_ke, len(p.rekaman), g.putar(p.rekaman).keadaan == p.keadaan)
}"#;

    const HARAP_REKAM: &str = "REKAMAN 6 [1, 2, 1, 1, 1, 3]\nPUTAR true true 1 2.0\nTETIKUS [1, 2] true 1 0.30000000000000004\nTANPA 0\nPANJANG 1200 4 true";

    /// Rekaman masukan (run-length) yang diputar ulang sampai di keadaan yang SAMA,
    /// tetikus ikut — game yang dimainkan di jendela bisa direproduksi tanpa jendela.
    #[test]
    fn rekam_dan_putar_ulang_sampai_di_keadaan_yang_sama() {
        for vm in [false, true] {
            let (k, sukses) = jalankan_sumber(PROGRAM_REKAM, vm);
            assert!(sukses, "gagal di {}:\n{}", if vm { "--vm" } else { "interp" }, k);
            assert_eq!(k.trim(), HARAP_REKAM, "jawaban salah di {}", if vm { "--vm" } else { "interp" });
        }
    }
    /// `ui::menu`: navigasi berputar, pilihan dengan Enter, dan baris aktif DIGAMBAR
    /// dengan warna sorot (diperiksa pikselnya); contoh menu-main-jeda dikunci jejaknya.
    #[test]
    fn menu_dan_tumpukan_adegan_benar_di_dua_mesin() {
        let src = r#"package main
import "std/mesin/ui"
import "std/mesin/masukan"
import "std/mesin/warna"
fn kuning(c, y0, y1) {
    var n = 0
    for _, y in range(y0, y1) { for _, x in range(0, 60) {
        let p = canvas::get_pixel(c, x, y)
        if p.r > 200 && p.g > 150 && p.b < 100 { n = n + 1 }
    } }
    return n
}
pub fn main() {
    let m = ui::menu(["A", "B", "C"])
    let atas = masukan::buatan(masukan::kosong(), ["Up"])
    let r = m.perbarui(atas)
    wok(r[0].aktif, r[1])
    let pilih = masukan::buatan(masukan::kosong(), ["Enter"])
    wok(r[0].perbarui(pilih)[1])
    let c = Canvas::new(60, 30)
    r[0].gambar(c, 0, 0, warna::PUTIH, warna::KUNING)
    wok(kuning(c, 0, 20) == 0, kuning(c, 20, 30) > 0)
}"#;
        for vm in [false, true] {
            let (k, sukses) = jalankan_sumber(src, vm);
            assert!(sukses, "gagal:\n{}", k);
            assert_eq!(k.trim(), "2 Option::None\nOption::Some(2)\ntrue true", "di {}", if vm { "--vm" } else { "interp" });
        }
        let contoh = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/game_menu.jrg");
        let harap = "langkah 4 | adegan menu | tumpukan 1 | aktif 0\n\
                     langkah 10 | adegan main | tumpukan 1 | langkah 5\n\
                     langkah 14 | adegan jeda | tumpukan 2 | 3\n\
                     langkah 17 | adegan main | tumpukan 1 | langkah 7\n\
                     langkah 21 | adegan selesai | tumpukan 1 | 0\n\
                     selesai: true";
        for vm in [false, true] {
            let mut cmd = Command::new(jrgc());
            cmd.arg(contoh).arg("--sopan").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
            if vm { cmd.arg("--vm"); }
            let out = cmd.output().expect("jalankan");
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), harap, "game_menu di {}", if vm { "--vm" } else { "interp" });
        }
    }
    /// `dengan_bunyi`: bunyi diturunkan dari PERUBAHAN keadaan dan cuma diminta lewat
    /// `bunyi_dari`; `perbarui` tetap murni, jadi simulasi tidak pernah berbunyi.
    #[test]
    fn bunyi_diturunkan_dari_perubahan_keadaan() {
        let src = r#"package main
import "std/mesin"
import "std/mesin/masukan"
struct K { pub n: int }
fn maju(k, m, dt) { return K { n: k.n + 1 } }
fn gambar(c, k) {}
fn genap(lama, baru) { if baru.n % 2 == 0 { return [suara::sunyi(10)] }
    return [] }
pub fn main() {
    let g = mesin::baru("b", 4, 4, K { n: 0 }, maju, gambar).dengan_bunyi(genap)
    let h1 = g.langkah(masukan::kosong())
    let h2 = h1.langkah(masukan::kosong())
    wok(len(h1.bunyi_dari(g.keadaan)), len(h2.bunyi_dari(h1.keadaan)), typeof(h2.bunyi_dari(h1.keadaan)[0]))
    wok(len(mesin::baru("b", 4, 4, K { n: 0 }, maju, gambar).bunyi_dari(K { n: 0 })))
    wok(mesin::baru("b", 4, 4, K { n: 0 }, maju, gambar).dengan_bunyi(|l, b| 5).bunyi_dari(K { n: 0 }))
}"#;
        for vm in [false, true] {
            let (k, sukses) = jalankan_sumber(src, vm);
            assert!(!sukses, "fungsi bunyi yang menjawab angka harus ditolak:\n{}", k);
            assert!(k.starts_with("0 1 wokwok:f64\n0\n"), "di {}: {}", vm, k);
            assert!(k.contains("fungsi `dengan_bunyi` harus menjawab array bunyi, dapat `int`"), "{}", k);
        }
    }
}

/// Mode `Canvas::new(.., mode)`: `"png"` menimpa satu berkas tiap `update`, `"web"` menyajikan
/// bingkai dan menerima tombol lewat HTTP lokal, mode asing ditolak — di dua mesin.
mod mode_kanvas {
    use super::*;
    use std::io::{Read, Write};

    fn http(port: u16, jalur: &str) -> Vec<u8> {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("sambung ke kanvas web");
        write!(s, "GET {} HTTP/1.1\r\nHost: x\r\n\r\n", jalur).unwrap();
        let mut isi = Vec::new();
        s.read_to_end(&mut isi).unwrap();
        let awal = isi.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(0);
        isi[awal..].to_vec()
    }

    #[test]
    fn png_dan_mode_asing_di_dua_mesin() {
        for vm in [false, true] {
            let d = bersama::dir_unik("jrg-kanvas-mode");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("p.jrg"), "let c = Canvas::new(6, 4, \"Uji Mode\", [\"png\", \"default\"])\n\
                canvas::clear(c, 0x2040FF)\nwok(canvas::update(c), fs::exists(\"uji_mode.png\"))\n\
                let t = Canvas::new(2, 2, \"x\", \"tabb\")\n").unwrap();
            let mut cmd = Command::new(jrgc());
            cmd.arg("p.jrg").arg("--sopan").current_dir(&d).env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
            if vm { cmd.arg("--vm"); }
            let out = cmd.output().expect("jalankan");
            let (k, g) = (String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string());
            assert_eq!(k.trim(), "true true", "mode png (vm={}): {}{}", vm, k, g);
            assert!(g.contains("mode `\"tabb\"` tidak dikenal") && g.contains("maksudnya `\"tab\"`"), "mode asing (vm={}): {}", vm, g);
            let png = std::fs::read(d.join("uji_mode.png")).expect("berkas png ditulis");
            assert_eq!(&png[1..4], b"PNG");
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn web_menyajikan_bingkai_dan_menerima_tombol() {
        let d = bersama::dir_unik("jrg-kanvas-web");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("w.jrg"), "let c = Canvas::new(3, 2, \"Web\", \"web\")\n\
            canvas::clear(c, 0x2040FF)\ncanvas::fill_rect(c, 1, 1, 1, 1, 0xFF0000)\n\
            var n = 0\nvar a = false\n\
            while n < 1500 && canvas::update(c) {\n if canvas::key_held(c, \"A\") { a = true }\n time::sleep_ms(10)\n n = n + 1\n}\n\
            wok(\"tombol A:\", a)\n").unwrap();
        let port = 47000 + (std::process::id() % 900) as u16;
        let mut anak = Command::new(jrgc()).arg("w.jrg").arg("--sopan").current_dir(&d)
            .env("JRG_KANVAS_PORT", port.to_string()).env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY")
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().expect("jalankan");
        let mut bingkai = Vec::new();
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            // Bingkai baru ada sesudah `canvas::update` pertama: tunggu sampai isinya bukan nol.
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                bingkai = http(port, "/bingkai");
                if bingkai.iter().any(|b| *b != 0) { break; }
            }
        }
        let halaman = String::from_utf8_lossy(&http(port, "/")).to_string();
        http(port, "/tombol?k=KeyA&d=1");
        std::thread::sleep(std::time::Duration::from_millis(200));
        http(port, "/tombol?k=Escape&d=1");
        let out = anak.wait_with_output().expect("tunggu");
        let k = String::from_utf8_lossy(&out.stdout).to_string();
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(bingkai.len(), 3 * 2 * 4, "ukuran bingkai RGBA");
        assert_eq!(&bingkai[0..4], &[0x20, 0x40, 0xFF, 0xFF], "piksel (0,0) biru");
        assert_eq!(&bingkai[16..20], &[0xFF, 0x00, 0x00, 0xFF], "piksel (1,1) merah");
        assert!(halaman.contains("<canvas") && halaman.contains("<title>Web</title>"), "halaman: {}", halaman);
        assert_eq!(k.trim(), "tombol A: true", "keluaran: {}{}", k, String::from_utf8_lossy(&out.stderr));
    }

    /// Tanpa layar sentuh: nol jari, nomor jari asing DITOLAK (bukan dijawab 0,0), di dua mesin.
    #[test]
    fn sentuhan_tanpa_layar_di_dua_mesin() {
        let r = Ruang::baru();
        let program = "    let c = Canvas::new(4, 4, \"s\")\n    canvas::update(c)\n\
            wok(canvas::jumlah_sentuhan(c), canvas::layar_sentuh())\n    let p = canvas::sentuhan(c, 0)";
        for vm in [false, true] {
            let (k, sukses) = r.jalankan_di(program, vm);
            assert!(!sukses && k.starts_with("0 false"), "vm={}: {}", vm, k);
            assert!(k.contains("tidak ada jari ke-0") && k.contains("jumlah_sentuhan"), "vm={}: {}", vm, k);
        }
    }

    /// Lewat mode `"web"` tetikus kiri dihitung SATU jari, dan tombol layar `std/mesin` ikut tertahan.
    #[test]
    fn tetikus_web_jadi_satu_jari_dan_menahan_tombol_layar() {
        let d = bersama::dir_unik("jrg-kanvas-sentuh");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("s.jrg"), "import \"std/mesin/masukan\"\n\
            let t = masukan::susun_tombol_layar([\"Left\", \"Right\", \"Space\"], 160, 120)\n\
            wok(masukan::tinggi_pita(t, 120), t[0].y)\n\
            let c = Canvas::new(160, 150, \"s\", \"web\")\nvar n = 0\nvar tadi = \"\"\n\
            while n < 1500 && canvas::update(c) {\n\
             let s = str(canvas::jumlah_sentuhan(c)) + \" \" + str(dict::keys(masukan::layar_ditahan(c, t)))\n\
             if s != tadi { wok(s)\n tadi = s }\n time::sleep_ms(10)\n n = n + 1\n}\n").unwrap();
        let port = 47900 + (std::process::id() % 90) as u16;
        let anak = Command::new(jrgc()).arg("s.jrg").arg("--sopan").current_dir(&d)
            .env("JRG_KANVAS_PORT", port.to_string()).env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY")
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().expect("jalankan");
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { break; }
        }
        let tunggu = || std::thread::sleep(std::time::Duration::from_millis(300));
        tunggu();
        // Tombolnya di PITA di bawah area game 160x120, bukan di atas game.
        http(port, "/tetikus?x=10&y=135&b=1");
        tunggu();
        http(port, "/tetikus?x=140&y=135&b=1");
        tunggu();
        http(port, "/tetikus?x=140&y=110&b=1");
        tunggu();
        http(port, "/tetikus?x=140&y=110&b=0");
        tunggu();
        http(port, "/tombol?k=Escape&d=1");
        let out = anak.wait_with_output().expect("tunggu");
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "30 125.0\n0 []\n1 [Left]\n1 [Space]\n1 []\n0 []",
                   "{}", String::from_utf8_lossy(&out.stderr));
    }
}
