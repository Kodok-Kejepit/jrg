// src/canvas/png_baca.rs — canvas/png_baca.rs — pembaca PNG, tanpa dependensi.

use super::png::paeth;
use crate::kompres::{adler32, crc32, inflate};

/// Batas ukuran hasil inflate.
const MAKS_HASIL: usize = 64 * 1024 * 1024;

/// Baca sebuah PNG jadi `(lebar, tinggi, piksel ARGB)`.
pub fn baca(data: &[u8]) -> Result<(usize, usize, Vec<u32>), String> {
    const TANDA: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 8 || data[..8] != TANDA {
        return Err("bukan berkas PNG: delapan byte pertamanya bukan tanda tangan PNG".into());
    }

    let mut pos = 8usize;
    let (mut lebar, mut tinggi) = (0usize, 0usize);
    let (mut kedalaman, mut jenis_warna, mut interlace) = (0u8, 0u8, 0u8);
    let mut idat: Vec<u8> = Vec::new();
    let mut palet: Vec<[u8; 3]> = Vec::new();
    let mut palet_alfa: Vec<u8> = Vec::new();
    let mut lihat_ihdr = false;
    let mut lihat_trns = false;

    while pos + 8 <= data.len() {
        let panjang = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let jenis = &data[pos + 4..pos + 8];
        let awal_isi = pos + 8;
        if awal_isi + panjang + 4 > data.len() {
            return Err(format!(
                "potongan `{}` mengaku {} byte tapi berkasnya sudah habis",
                String::from_utf8_lossy(jenis), panjang));
        }
        let isi = &data[awal_isi..awal_isi + panjang];
        let crc_tertulis = u32::from_be_bytes([
            data[awal_isi + panjang], data[awal_isi + panjang + 1],
            data[awal_isi + panjang + 2], data[awal_isi + panjang + 3],
        ]);
        // CRC atas (jenis + isi), sesuai spesifikasi.
        let mut buf = Vec::with_capacity(4 + panjang);
        buf.extend_from_slice(jenis);
        buf.extend_from_slice(isi);
        if crc32(&buf) != crc_tertulis {
            return Err(format!(
                "CRC potongan `{}` tidak cocok — berkasnya rusak",
                String::from_utf8_lossy(jenis)));
        }

        // Dibandingkan dengan penjaga, bukan pola literal-byte: pola `b"IHDR"`
        // bertipe `&[u8; 4]` sementara `jenis` sebuah slice, dan kecocokan keduanya …
        match jenis {
            j if j == &b"IHDR"[..] => {
                if panjang != 13 {
                    return Err("IHDR harus 13 byte".into());
                }
                lebar = u32::from_be_bytes([isi[0], isi[1], isi[2], isi[3]]) as usize;
                tinggi = u32::from_be_bytes([isi[4], isi[5], isi[6], isi[7]]) as usize;
                kedalaman = isi[8];
                jenis_warna = isi[9];
                interlace = isi[12];
                lihat_ihdr = true;
                if lebar == 0 || tinggi == 0 {
                    return Err("PNG berukuran nol".into());
                }
                // Sama seperti panjang frame WebSocket: angkanya datang dari
                // berkas, jadi ia dibatasi SEBELUM dipakai mengalokasi.
                if lebar.saturating_mul(tinggi) > 64 * 1024 * 1024 {
                    return Err(format!("PNG {}x{} terlalu besar", lebar, tinggi));
                }
            }
            j if j == &b"PLTE"[..] => {
                if panjang % 3 != 0 {
                    return Err("PLTE panjangnya bukan kelipatan 3".into());
                }
                palet = isi.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            j if j == &b"tRNS"[..] => {
                palet_alfa = isi.to_vec();
                lihat_trns = true;
            }
            j if j == &b"IDAT"[..] => idat.extend_from_slice(isi),
            j if j == &b"IEND"[..] => break,
            _ => {}
        }
        pos = awal_isi + panjang + 4;
    }

    if !lihat_ihdr {
        return Err("PNG tanpa IHDR".into());
    }
    if interlace != 0 {
        return Err("PNG interlace (Adam7) belum didukung — ia bukan varian kecil \
                    melainkan tata letak piksel yang sama sekali lain.\n  \
                    bantuan: simpan ulang tanpa interlace".into());
    }
    if idat.is_empty() {
        return Err("PNG tanpa data gambar (IDAT)".into());
    }
    // `tRNS` berarti dua hal BERBEDA tergantung tipe warnanya: pada PNG berpalet
    // ia alfa per-indeks (dibaca di bawah), pada abu-abu dan RGB ia sebuah WARNA …
    if lihat_trns && jenis_warna != 3 {
        return Err("PNG ini memakai `tRNS` sebagai warna kunci transparan pada \
                    gambar bukan-palet, dan itu belum didukung.\n  \
                    bantuan: simpan ulang sebagai RGBA (tipe warna 6), yang \
                    menyimpan alfa per piksel".into());
    }

    // Pembungkus zlib: dua byte kepala, lalu Adler-32 di ekor.
    if idat.len() < 6 {
        return Err("aliran zlib terlalu pendek".into());
    }
    let cmf = idat[0];
    if cmf & 0x0F != 8 {
        return Err(format!("metode kompresi zlib {} bukan deflate", cmf & 0x0F));
    }
    if (idat[0] as u16 * 256 + idat[1] as u16) % 31 != 0 {
        return Err("kepala zlib gagal uji modulo-31".into());
    }
    if idat[1] & 0x20 != 0 {
        return Err("aliran zlib memakai kamus awal — tidak dipakai PNG".into());
    }
    let mentah = inflate(&idat[2..idat.len() - 4], MAKS_HASIL)?;
    let adler_tertulis = u32::from_be_bytes([
        idat[idat.len() - 4], idat[idat.len() - 3], idat[idat.len() - 2], idat[idat.len() - 1],
    ]);
    // Ini yang menangkap inflate yang salah. CRC potongan tidak bisa
    // melihatnya: Fase KANVAS menemukan satu bit BTYPE yang salah di
    // penulisnya, dengan SEMUA CRC benar.
    if adler32(&mentah) != adler_tertulis {
        return Err("Adler-32 tidak cocok — hasil dekompresinya salah".into());
    }

    let kanal: usize = match jenis_warna {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        lain => return Err(format!("tipe warna PNG {} tidak dikenal", lain)),
    };
    if kedalaman == 16 {
        return Err("PNG 16 bit per kanal belum didukung: menurunkannya ke 8 bit \
                    MERUGIKAN data, dan pilihan semacam itu tidak diambil diam-diam.\n  \
                    bantuan: simpan ulang sebagai 8 bit".into());
    }
    if jenis_warna != 3 && kedalaman != 8 {
        return Err(format!(
            "kedalaman {} bit cuma didukung untuk PNG berpalet, bukan tipe warna {}",
            kedalaman, jenis_warna));
    }
    if jenis_warna == 3 && !matches!(kedalaman, 1 | 2 | 4 | 8) {
        return Err(format!("kedalaman palet {} bit tidak sah", kedalaman));
    }

    let bit_per_piksel = kanal * kedalaman as usize;
    let byte_per_baris = (lebar * bit_per_piksel + 7) / 8;
    // Jarak byte antar-piksel untuk filter Sub/Paeth. Di bawah 8 bit,
    // spesifikasi menetapkannya 1 — bukan pecahan.
    let bpp = ((bit_per_piksel + 7) / 8).max(1);
    let perlu = (byte_per_baris + 1) * tinggi;
    if mentah.len() < perlu {
        return Err(format!(
            "data gambar kurang: butuh {} byte, dapat {}", perlu, mentah.len()));
    }

    // ── Buang filter, baris demi baris ──────────────────────────────────
    let mut baris_atas = vec![0u8; byte_per_baris];
    let mut baris = vec![0u8; byte_per_baris];
    let mut piksel = vec![0u32; lebar * tinggi];

    for y in 0..tinggi {
        let awal = y * (byte_per_baris + 1);
        let filter = mentah[awal];
        baris.copy_from_slice(&mentah[awal + 1..awal + 1 + byte_per_baris]);
        match filter {
            0 => {}
            1 => for i in bpp..byte_per_baris {
                baris[i] = baris[i].wrapping_add(baris[i - bpp]);
            },
            2 => for i in 0..byte_per_baris {
                baris[i] = baris[i].wrapping_add(baris_atas[i]);
            },
            3 => for i in 0..byte_per_baris {
                let kiri = if i >= bpp { baris[i - bpp] as u32 } else { 0 };
                baris[i] = baris[i].wrapping_add(((kiri + baris_atas[i] as u32) / 2) as u8);
            },
            4 => for i in 0..byte_per_baris {
                let kiri = if i >= bpp { baris[i - bpp] } else { 0 };
                let kiri_atas = if i >= bpp { baris_atas[i - bpp] } else { 0 };
                baris[i] = baris[i].wrapping_add(paeth(kiri, baris_atas[i], kiri_atas));
            },
            lain => return Err(format!("filter baris {} tidak sah (0..4)", lain)),
        }

        for x in 0..lebar {
            let warna = match jenis_warna {
                3 => {
                    let i = ambil_indeks(&baris, x, kedalaman);
                    let Some(p) = palet.get(i) else {
                        return Err(format!("indeks palet {} di luar PLTE", i));
                    };
                    let a = *palet_alfa.get(i).unwrap_or(&255);
                    ((a as u32) << 24) | ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
                }
                0 => {
                    let g = baris[x] as u32;
                    0xFF00_0000 | (g << 16) | (g << 8) | g
                }
                4 => {
                    let g = baris[x * 2] as u32;
                    let a = baris[x * 2 + 1] as u32;
                    (a << 24) | (g << 16) | (g << 8) | g
                }
                2 => {
                    let i = x * 3;
                    0xFF00_0000
                        | ((baris[i] as u32) << 16)
                        | ((baris[i + 1] as u32) << 8)
                        | baris[i + 2] as u32
                }
                _ => {
                    let i = x * 4;
                    ((baris[i + 3] as u32) << 24)
                        | ((baris[i] as u32) << 16)
                        | ((baris[i + 1] as u32) << 8)
                        | baris[i + 2] as u32
                }
            };
            piksel[y * lebar + x] = warna;
        }
        std::mem::swap(&mut baris_atas, &mut baris);
    }

    Ok((lebar, tinggi, piksel))
}

/// Indeks palet piksel ke-`x` pada baris berkedalaman `kedalaman` bit.
fn ambil_indeks(baris: &[u8], x: usize, kedalaman: u8) -> usize {
    match kedalaman {
        8 => baris[x] as usize,
        4 => ((baris[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 0x0F) as usize,
        2 => ((baris[x / 4] >> (6 - 2 * (x % 4))) & 0x03) as usize,
        _ => ((baris[x / 8] >> (7 - (x % 8))) & 0x01) as usize,
    }
}
