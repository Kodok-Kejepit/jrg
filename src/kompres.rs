// src/kompres.rs — DEFLATE (mampat dan bongkar), CRC32 dan Adler-32, tanpa dependensi.
// Satu definisi untuk PNG (`canvas::png`, `canvas::png_baca`), `zip::` dan `crypto::crc32`.

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

/// CRC32 (PNG, ZIP, `crypto::crc32`) — satu tabel untuk semuanya.
pub fn crc32(data: &[u8]) -> u32 {
    let t = tabel_crc();
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

pub fn adler32(data: &[u8]) -> u32 {
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

struct Penulis {
    keluar: Vec<u8>,
    tampung: u32,
    jumlah: u32,
}

impl Penulis {
    fn baru() -> Penulis { Penulis { keluar: Vec::new(), tampung: 0, jumlah: 0 } }

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
    /// Penulis tambahan (panjang/jarak) ditulis dari bit terkecil — beda
    /// arah dengan kode Huffman, dan itu memang aturannya.
    fn tambahan(&mut self, nilai: u32, lebar: u32) {
        for i in 0..lebar { self.bit((nilai >> i) & 1); }
    }
    fn rata(&mut self) {
        while self.jumlah != 0 { self.bit(0); }
    }
}

// ─── Huffman tetap ──────────────────────────────────────────────────────

fn tulis_literal(w: &mut Penulis, sym: u32) {
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
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = Penulis::baru();
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


// ─── Pembaca bit (LSB dulu, seperti DEFLATE) ────────────────────────────────

struct Pembaca<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    simpan: u32,
}

impl<'a> Pembaca<'a> {
    fn baru(data: &'a [u8]) -> Self {
        Pembaca { data, pos: 0, bit: 0, simpan: 0 }
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

    fn baca(&self, b: &mut Pembaca) -> Result<u16, String> {
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

/// Bongkar aliran DEFLATE mentah; hasil di atas `batas` byte ditolak (bom).
pub fn inflate(data: &[u8], batas: usize) -> Result<Vec<u8>, String> {
    inflate_sisa(data, batas).map(|(isi, _)| isi)
}

/// `inflate` yang juga menjawab berapa byte `data` dipakai aliran itu — ekor sesudahnya milik pemanggil.
pub fn inflate_sisa(data: &[u8], batas: usize) -> Result<(Vec<u8>, usize), String> {
    let mut b = Pembaca::baru(data);
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
                if keluar.len() + len > batas {
                    return Err(format!("hasil bongkaran melewati batas {} byte", batas));
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
                        if keluar.len() + 1 > batas {
                            return Err(format!("hasil bongkaran melewati batas {} byte", batas));
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
                    if keluar.len() + panjang > batas {
                        return Err(format!("hasil bongkaran melewati batas {} byte", batas));
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
    Ok((keluar, b.pos))
}

/// Tabel Huffman blok DINAMIS.
fn baca_tabel_dinamis(b: &mut Pembaca) -> Result<(Huffman, Huffman), String> {
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

