// src/canvas/png.rs — canvas/png.rs — ekspor PNG, lengkap dengan kompresinya sendiri.

// ─── CRC32 (PNG) dan Adler-32 (zlib) ────────────────────────────────────

fn tabel_crc() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for n in 0..256u32 {
            let mut c = n;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            t[n as usize] = c;
        }
        t
    })
}

/// CRC32 PNG. `pub(crate)` supaya `png_baca.rs` memakai definisi yang SAMA
/// alih-alih menyalinnya — dua tabel CRC yang harus cocok adalah tempat yang
/// bagus untuk melenceng tanpa ketahuan.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let t = tabel_crc();
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // Dikelompokkan supaya modulonya jarang dijalankan; 5552 adalah
    // jumlah byte terbanyak yang dijamin tidak meluapkan u32.
    for potong in data.chunks(5552) {
        for &x in potong {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

// ─── Penulis bit ────────────────────────────────────────────────────────

struct Bit {
    keluar: Vec<u8>,
    tampung: u32,
    jumlah: u32,
}

impl Bit {
    fn baru() -> Bit { Bit { keluar: Vec::new(), tampung: 0, jumlah: 0 } }

    /// Deflate menulis kode Huffman dari bit paling SIGNIFIKAN, tapi mengemasnya
    /// ke byte dari bit paling KECIL.
    fn kode(&mut self, nilai: u32, lebar: u32) {
        for i in (0..lebar).rev() {
            self.bit((nilai >> i) & 1);
        }
    }
    fn bit(&mut self, b: u32) {
        self.tampung |= (b & 1) << self.jumlah;
        self.jumlah += 1;
        if self.jumlah == 8 {
            self.keluar.push(self.tampung as u8);
            self.tampung = 0;
            self.jumlah = 0;
        }
    }
    /// Bit tambahan (panjang/jarak) ditulis dari bit terkecil — beda
    /// arah dengan kode Huffman, dan itu memang aturannya.
    fn tambahan(&mut self, nilai: u32, lebar: u32) {
        for i in 0..lebar { self.bit((nilai >> i) & 1); }
    }
    fn rata(&mut self) {
        while self.jumlah != 0 { self.bit(0); }
    }
}

// ─── Huffman tetap ──────────────────────────────────────────────────────

fn tulis_literal(w: &mut Bit, sym: u32) {
    match sym {
        0..=143   => w.kode(0x30 + sym, 8),
        144..=255 => w.kode(0x190 + (sym - 144), 9),
        256..=279 => w.kode(sym - 256, 7),
        _         => w.kode(0xC0 + (sym - 280), 8),
    }
}

/// Tabel panjang deflate: (kode, bit tambahan, panjang dasar).
const PANJANG: [(u32, u32, u32); 29] = [
    (257,0,3),(258,0,4),(259,0,5),(260,0,6),(261,0,7),(262,0,8),(263,0,9),(264,0,10),
    (265,1,11),(266,1,13),(267,1,15),(268,1,17),
    (269,2,19),(270,2,23),(271,2,27),(272,2,31),
    (273,3,35),(274,3,43),(275,3,51),(276,3,59),
    (277,4,67),(278,4,83),(279,4,99),(280,4,115),
    (281,5,131),(282,5,163),(283,5,195),(284,5,227),(285,0,258),
];

/// Tabel jarak: (kode, bit tambahan, jarak dasar).
const JARAK: [(u32, u32, u32); 30] = [
    (0,0,1),(1,0,2),(2,0,3),(3,0,4),(4,1,5),(5,1,7),(6,2,9),(7,2,13),
    (8,3,17),(9,3,25),(10,4,33),(11,4,49),(12,5,65),(13,5,97),
    (14,6,129),(15,6,193),(16,7,257),(17,7,385),(18,8,513),(19,8,769),
    (20,9,1025),(21,9,1537),(22,10,2049),(23,10,3073),
    (24,11,4097),(25,11,6145),(26,12,8193),(27,12,12289),
    (28,13,16385),(29,13,24577),
];

const MIN_COCOK: usize = 3;
const MAKS_COCOK: usize = 258;
const MAKS_JARAK: usize = 32768;
const BIT_HASH: usize = 15;

/// DEFLATE dengan Huffman tetap dan pencocokan LZ77.
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = Bit::baru();
    w.bit(1);              // BFINAL
    // BTYPE ditulis LSB-DULU (`tambahan`), bukan MSB-dulu (`kode`).
    w.tambahan(0b01, 2);   // BTYPE = Huffman tetap

    // Satu slot per hash — rantai penuh akan memampatkan sedikit lebih baik
    // dengan biaya waktu yang jauh lebih besar.
    let mut kepala = vec![usize::MAX; 1 << BIT_HASH];
    let mut i = 0usize;
    let n = data.len();

    let hash = |d: &[u8], p: usize| -> usize {
        (((d[p] as usize) << 10) ^ ((d[p + 1] as usize) << 5) ^ (d[p + 2] as usize))
            & ((1 << BIT_HASH) - 1)
    };

    while i < n {
        let mut panjang = 0usize;
        let mut jarak = 0usize;
        if i + MIN_COCOK <= n {
            let h = hash(data, i);
            let calon = kepala[h];
            kepala[h] = i;
            if calon != usize::MAX && i - calon <= MAKS_JARAK {
                let batas = MAKS_COCOK.min(n - i);
                let mut l = 0usize;
                while l < batas && data[calon + l] == data[i + l] { l += 1; }
                if l >= MIN_COCOK {
                    panjang = l;
                    jarak = i - calon;
                }
            }
        }
        if panjang >= MIN_COCOK {
            let (kode_p, xb_p, dasar_p) = *PANJANG.iter().rev()
                .find(|(_, _, d)| *d as usize <= panjang).expect("tabel panjang menutupi 3..258");
            tulis_literal(&mut w, kode_p);
            w.tambahan(panjang as u32 - dasar_p, xb_p);
            let (kode_j, xb_j, dasar_j) = *JARAK.iter().rev()
                .find(|(_, _, d)| *d as usize <= jarak).expect("tabel jarak menutupi 1..32768");
            w.kode(kode_j, 5); // jarak Huffman-tetap: 5 bit, MSB dulu
            w.tambahan(jarak as u32 - dasar_j, xb_j);
            // Posisi di dalam kecocokan tetap didaftarkan ke tabel hash;
            // tanpa itu, pengulangan berikutnya kehilangan jangkarnya.
            for k in 1..panjang {
                if i + k + MIN_COCOK <= n {
                    let h = hash(data, i + k);
                    kepala[h] = i + k;
                }
            }
            i += panjang;
        } else {
            tulis_literal(&mut w, data[i] as u32);
            i += 1;
        }
    }
    tulis_literal(&mut w, 256); // akhir blok
    w.rata();
    w.keluar
}

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
