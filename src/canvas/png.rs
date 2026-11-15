// src/canvas/png.rs — canvas/png.rs — ekspor PNG; DEFLATE dan CRC-nya di `crate::kompres`.

use crate::kompres::{adler32, crc32, deflate};

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // CMF=0x78 (deflate, jendela 32K), FLG dipilih supaya (CMF<<8|FLG)
    // habis dibagi 31 — itu syarat yang diperiksa pembaca zlib.
    out.push(0x78);
    out.push(0x01);
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn potongan(keluar: &mut Vec<u8>, jenis: &[u8; 4], isi: &[u8]) {
    keluar.extend_from_slice(&(isi.len() as u32).to_be_bytes());
    let mut ber_crc = Vec::with_capacity(4 + isi.len());
    ber_crc.extend_from_slice(jenis);
    ber_crc.extend_from_slice(isi);
    keluar.extend_from_slice(&ber_crc);
    keluar.extend_from_slice(&crc32(&ber_crc).to_be_bytes());
}

/// Prediktor Paeth (PNG filter 4).
pub(crate) fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let (pa, pb, pc) = ((p - a as i32).abs(), (p - b as i32).abs(), (p - c as i32).abs());
    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
}

/// Susun PNG RGBA 8-bit dari piksel ARGB.
pub fn susun(lebar: usize, tinggi: usize, piksel: &[u32]) -> Vec<u8> {
    let bpp = 4usize;
    let baris = lebar * bpp;
    let mut mentah = Vec::with_capacity((baris + 1) * tinggi);
    let mut sebelum = vec![0u8; baris];
    let mut kini = vec![0u8; baris];
    let mut calon = vec![vec![0u8; baris]; 5];

    for y in 0..tinggi {
        for x in 0..lebar {
            let p = piksel[y * lebar + x];
            kini[x * 4]     = ((p >> 16) & 0xFF) as u8; // R
            kini[x * 4 + 1] = ((p >> 8) & 0xFF) as u8;  // G
            kini[x * 4 + 2] = (p & 0xFF) as u8;         // B
            kini[x * 4 + 3] = ((p >> 24) & 0xFF) as u8; // A
        }
        let mut skor = [0u64; 5];
        for i in 0..baris {
            let a = if i >= bpp { kini[i - bpp] } else { 0 };
            let b = sebelum[i];
            let c = if i >= bpp { sebelum[i - bpp] } else { 0 };
            let x = kini[i];
            calon[0][i] = x;
            calon[1][i] = x.wrapping_sub(a);
            calon[2][i] = x.wrapping_sub(b);
            calon[3][i] = x.wrapping_sub((((a as u16) + (b as u16)) / 2) as u8);
            calon[4][i] = x.wrapping_sub(paeth(a, b, c));
            for f in 0..5 {
                skor[f] += (calon[f][i] as i8).unsigned_abs() as u64;
            }
        }
        let terbaik = (0..5).min_by_key(|&f| skor[f]).unwrap_or(0);
        mentah.push(terbaik as u8);
        mentah.extend_from_slice(&calon[terbaik]);
        std::mem::swap(&mut sebelum, &mut kini);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(lebar as u32).to_be_bytes());
    ihdr.extend_from_slice(&(tinggi as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, deflate, adaptif, non-interlace
    potongan(&mut out, b"IHDR", &ihdr);
    potongan(&mut out, b"IDAT", &zlib(&mentah));
    potongan(&mut out, b"IEND", &[]);
    out
}
