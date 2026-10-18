// src/canvas/png_baca.rs — canvas/png_baca.rs — pembaca PNG, tanpa dependensi.

use super::png::{crc32, paeth};

/// Adler-32, untuk memeriksa hasil inflate.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// ─── Pembaca bit (LSB dulu, seperti DEFLATE) ────────────────────────────────

struct Bit<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    simpan: u32,
}

impl<'a> Bit<'a> {
    fn baru(data: &'a [u8]) -> Self {
        Bit { data, pos: 0, bit: 0, simpan: 0 }
    }

    fn ambil(&mut self, n: u32) -> Result<u32, String> {
        while self.bit < n {
            if self.pos >= self.data.len() {
                return Err("aliran deflate habis di tengah simbol — berkasnya terpotong".into());
            }
            self.simpan |= (self.data[self.pos] as u32) << self.bit;
            self.pos += 1;
            self.bit += 8;
        }
        let v = self.simpan & ((1u32 << n) - 1);
        self.simpan >>= n;
        self.bit -= n;
        Ok(v)
    }

    fn selaraskan(&mut self) {
        self.simpan = 0;
        self.bit = 0;
    }
}

// ─── Huffman kanonik ────────────────────────────────────────────────────────

/// Pohon Huffman kanonik, dibangun dari PANJANG KODE tiap simbol.
struct Huffman {
    /// `hitung[n]` = berapa simbol berkode sepanjang `n` bit.
    hitung: [u16; 16],
    /// Simbol, diurutkan menaik menurut (panjang kode, nilai simbol).
    simbol: Vec<u16>,
}

impl Huffman {
    fn baru(panjang: &[u8]) -> Result<Self, String> {
        let mut hitung = [0u16; 16];
        for &p in panjang {
            if p as usize >= 16 {
                return Err(format!("panjang kode huffman {} di luar 0..15", p));
            }
            hitung[p as usize] += 1;
        }
        hitung[0] = 0;
        // Kode dengan panjang 0 tidak dipakai; sisanya harus membentuk pohon yang
        // PENUH.
        let mut sisa = 1i32;
        for n in 1..16 {
            sisa = sisa * 2 - hitung[n] as i32;
            if sisa < 0 {
                return Err("tabel huffman kelebihan kode (over-subscribed)".into());
            }
        }
        let mut offset = [0u16; 16];
        for n in 1..15 {
            offset[n + 1] = offset[n] + hitung[n];
        }
        let mut simbol = vec![0u16; panjang.len()];
        for (s, &p) in panjang.iter().enumerate() {
            if p != 0 {
                simbol[offset[p as usize] as usize] = s as u16;
                offset[p as usize] += 1;
            }
        }
        Ok(Huffman { hitung, simbol })
    }

    fn baca(&self, b: &mut Bit) -> Result<u16, String> {
        let (mut kode, mut pertama, mut indeks) = (0i32, 0i32, 0i32);
        for n in 1..16 {
            kode |= b.ambil(1)? as i32;
            let jumlah = self.hitung[n] as i32;
            if kode - jumlah < pertama {
                // `.get`, bukan indeks langsung.
                let i = (indeks + (kode - pertama)) as usize;
                return self.simbol.get(i).copied().ok_or_else(||
                    "tabel huffman menunjuk simbol yang tidak ada — alirannya rusak".to_string());
            }
            indeks += jumlah;
            pertama = (pertama + jumlah) << 1;
            kode <<= 1;
        }
        Err("kode huffman tidak dikenal — alirannya rusak".into())
    }
}

const PANJANG_DASAR: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59,
    67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const PANJANG_EKSTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4,
    5, 5, 5, 5, 0,
];
const JARAK_DASAR: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513,
    769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const JARAK_EKSTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10,
    11, 11, 12, 12, 13, 13,
];

/// Batas ukuran hasil inflate.
const MAKS_HASIL: usize = 64 * 1024 * 1024;

fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut b = Bit::baru(data);
    let mut keluar: Vec<u8> = Vec::new();
    loop {
        let terakhir = b.ambil(1)?;
        let jenis = b.ambil(2)?;
        match jenis {
            0 => {
                b.selaraskan();
                if b.pos + 4 > b.data.len() {
                    return Err("blok tersimpan terpotong".into());
                }
                let len = b.data[b.pos] as usize | ((b.data[b.pos + 1] as usize) << 8);
                let nlen = b.data[b.pos + 2] as usize | ((b.data[b.pos + 3] as usize) << 8);
                if len != !nlen & 0xFFFF {
                    return Err("blok tersimpan: LEN dan NLEN tidak cocok".into());
                }
                b.pos += 4;
                if b.pos + len > b.data.len() {
                    return Err("blok tersimpan melewati akhir berkas".into());
                }
                if keluar.len() + len > MAKS_HASIL {
                    return Err("hasil inflate melebihi batas 64 MiB".into());
                }
                keluar.extend_from_slice(&b.data[b.pos..b.pos + len]);
                b.pos += len;
            }
            1 | 2 => {
                let (lit, jar) = if jenis == 1 {
                    // Huffman TETAP: panjang kodenya ditentukan spesifikasi.
                    let mut pl = [0u8; 288];
                    for (i, p) in pl.iter_mut().enumerate() {
                        *p = match i {
                            0..=143 => 8,
                            144..=255 => 9,
                            256..=279 => 7,
                            _ => 8,
                        };
                    }
                    (Huffman::baru(&pl)?, Huffman::baru(&[5u8; 30])?)
                } else {
                    baca_tabel_dinamis(&mut b)?
                };
                loop {
                    let sym = lit.baca(&mut b)? as usize;
                    if sym == 256 {
                        break;
                    }
                    if sym < 256 {
                        if keluar.len() + 1 > MAKS_HASIL {
                            return Err("hasil inflate melebihi batas 64 MiB".into());
                        }
                        keluar.push(sym as u8);
                        continue;
                    }
                    let i = sym - 257;
                    if i >= 29 {
                        return Err(format!("kode panjang {} tidak sah", sym));
                    }
                    let panjang = PANJANG_DASAR[i] as usize
                        + b.ambil(PANJANG_EKSTRA[i] as u32)? as usize;
                    let dsym = jar.baca(&mut b)? as usize;
                    if dsym >= 30 {
                        return Err(format!("kode jarak {} tidak sah", dsym));
                    }
                    let jarak = JARAK_DASAR[dsym] as usize
                        + b.ambil(JARAK_EKSTRA[dsym] as u32)? as usize;
                    if jarak > keluar.len() {
                        return Err("rujukan balik menunjuk sebelum awal data".into());
                    }
                    if keluar.len() + panjang > MAKS_HASIL {
                        return Err("hasil inflate melebihi batas 64 MiB".into());
                    }
                    let mulai = keluar.len() - jarak;
                    for k in 0..panjang {
                        let v = keluar[mulai + k];
                        keluar.push(v);
                    }
                }
            }
            _ => return Err("blok deflate bertipe 3 — tidak ada artinya".into()),
        }
        if terakhir == 1 {
            break;
        }
    }
    Ok(keluar)
}

/// Tabel Huffman blok DINAMIS.
fn baca_tabel_dinamis(b: &mut Bit) -> Result<(Huffman, Huffman), String> {
    const URUT: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let hlit = b.ambil(5)? as usize + 257;
    let hdist = b.ambil(5)? as usize + 1;
    let hclen = b.ambil(4)? as usize + 4;
    let mut pk = [0u8; 19];
    for i in 0..hclen {
        pk[URUT[i]] = b.ambil(3)? as u8;
    }
    let kode = Huffman::baru(&pk)?;
    let mut panjang = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < panjang.len() {
        let s = kode.baca(b)?;
        match s {
            0..=15 => {
                panjang[i] = s as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("kode 16 mengulang panjang sebelumnya, tapi belum ada".into());
                }
                let n = 3 + b.ambil(2)? as usize;
                let v = panjang[i - 1];
                for _ in 0..n {
                    if i >= panjang.len() {
                        return Err("pengulangan panjang kode melewati akhir tabel".into());
                    }
                    panjang[i] = v;
                    i += 1;
                }
            }
            // 17 dan 18 berarti "ulangi NOL sekian kali", dan tabelnya sudah berisi nol
            // — jadi yang perlu cuma melompat.
            17 | 18 => {
                let n = if s == 17 { 3 + b.ambil(3)? as usize } else { 11 + b.ambil(7)? as usize };
                if i + n > panjang.len() {
                    return Err("pengulangan nol melewati akhir tabel panjang kode".into());
                }
                i += n;
            }
            _ => return Err(format!("kode panjang-kode {} tidak sah", s)),
        }
    }
    Ok((
        Huffman::baru(&panjang[..hlit])?,
        Huffman::baru(&panjang[hlit..])?,
    ))
}

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
    let mentah = inflate(&idat[2..idat.len() - 4])?;
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
