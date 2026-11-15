// src/dok/crypto.rs — crypto:: — hashing (MD5/SHA-1/SHA-256/SHA-512), HMAC-SHA256, CRC32,
// Base64/Hex, random aman (OS RNG), dan AES-256-CBC. Semuanya ditulis

use std::sync::atomic::{AtomicU64, Ordering};

use crate::interpreter::{err_value, ok_value, Value};

// ─── Helpers Value <-> bytes/str ────────────────────────────────────────

/// Isi yang diolah sebagai byte: teks (byte UTF-8-nya) atau array byte.
fn isi_byte(v: &Value, fname: &str) -> Result<Vec<u8>, String> {
    match v {
        Value::Array(a) => array_ke_byte(a, fname),
        lain => Ok(as_str(lain, fname)?.as_bytes().to_vec()),
    }
}

fn as_str<'a>(v: &'a Value, fname: &str) -> Result<&'a str, String> {
    match v { Value::Str(s) => Ok(s.as_str()), other => Err(format!("{}: butuh string, dapat `{}`", fname, other.type_name())) }
}

// ============================================================================
//  HASHING
// ============================================================================

const MD5_S: [u32; 64] = [
    7,12,17,22, 7,12,17,22, 7,12,17,22, 7,12,17,22,
    5, 9,14,20, 5, 9,14,20, 5, 9,14,20, 5, 9,14,20,
    4,11,16,23, 4,11,16,23, 4,11,16,23, 4,11,16,23,
    6,10,15,21, 6,10,15,21, 6,10,15,21, 6,10,15,21,
];
const MD5_K: [u32; 64] = [
    0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
    0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
    0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
    0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
    0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
    0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
    0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
    0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391,
];

pub fn md5(input: &[u8]) -> [u8; 16] {
    let (mut a0, mut b0, mut c0, mut d0): (u32, u32, u32, u32) = (0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476);
    let mut msg = input.to_vec();
    let bitlen = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bitlen.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for i in 0..16 { m[i] = u32::from_le_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = if i < 16 { ((b & c) | (!b & d), i) }
                else if i < 32 { ((d & b) | (!d & c), (5*i + 1) % 16) }
                else if i < 48 { (b ^ c ^ d, (3*i + 5) % 16) }
                else { (c ^ (b | !d), (7*i) % 16) };
            let f = f.wrapping_add(a).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            a = d; d = c; c = b; b = b.wrapping_add(f.rotate_left(MD5_S[i]));
        }
        a0 = a0.wrapping_add(a); b0 = b0.wrapping_add(b); c0 = c0.wrapping_add(c); d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes()); out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes()); out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

pub fn sha1(input: &[u8]) -> [u8; 20] {
    let (mut h0, mut h1, mut h2, mut h3, mut h4): (u32,u32,u32,u32,u32) =
        (0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0);
    let mut msg = input.to_vec();
    let ml = (input.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        for i in 16..80 { w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1); }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = if i < 20 { ((b & c) | ((!b) & d), 0x5A827999u32) }
                else if i < 40 { (b ^ c ^ d, 0x6ED9EBA1u32) }
                else if i < 60 { ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32) }
                else { (b ^ c ^ d, 0xCA62C1D6u32) };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            e = d; d = c; c = b.rotate_left(30); b = a; a = temp;
        }
        h0 = h0.wrapping_add(a); h1 = h1.wrapping_add(b); h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d); h4 = h4.wrapping_add(e);
    }
    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes()); out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes()); out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

const SHA256_K: [u32; 64] = [
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
];

const SHA256_IV: [u32; 8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];

/// Satu blok kompresi SHA-256 — SATU definisi untuk `sha256` dan HMAC yang disiapkan (`HmacSha256`). Instruksi
/// SHA-NI kalau prosesornya punya (±3,5x), selain itu putaran skalar yang dibuka; hasil keduanya dites sama.
fn sha256_kompres(h: &mut [u32; 8], chunk: &[u8]) {
    #[cfg(target_arch = "x86_64")]
    if shani::ada() { return shani::kompres(h, chunk); }
    sha256_kompres_lunak(h, chunk)
}

fn sha256_kompres_lunak(h: &mut [u32; 8], chunk: &[u8]) {
    let mut w = [0u32; 16];
    for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
    let mut s = *h;
    // Satu putaran; peran a..h bergeser lewat indeks, bukan lewat delapan salinan. Jadwal pesan 16 kata berputar.
    macro_rules! r {
        ($i:expr, $a:expr, $b:expr, $c:expr, $d:expr, $e:expr, $f:expr, $g:expr, $hh:expr) => {{
            if $i >= 16 {
                let (w15, w2) = (w[($i + 1) & 15], w[($i + 14) & 15]);
                let s0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
                let s1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
                w[$i & 15] = w[$i & 15].wrapping_add(s0).wrapping_add(w[($i + 9) & 15]).wrapping_add(s1);
            }
            let e = s[$e];
            let t1 = s[$hh].wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & s[$f]) ^ (!e & s[$g])).wrapping_add(SHA256_K[$i]).wrapping_add(w[$i & 15]);
            let a = s[$a];
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & s[$b]) ^ (a & s[$c]) ^ (s[$b] & s[$c]));
            s[$d] = s[$d].wrapping_add(t1);
            s[$hh] = t1.wrapping_add(t2);
        }};
    }
    macro_rules! delapan { ($i:expr) => {
        r!($i, 0,1,2,3,4,5,6,7); r!($i+1, 7,0,1,2,3,4,5,6); r!($i+2, 6,7,0,1,2,3,4,5); r!($i+3, 5,6,7,0,1,2,3,4);
        r!($i+4, 4,5,6,7,0,1,2,3); r!($i+5, 3,4,5,6,7,0,1,2); r!($i+6, 2,3,4,5,6,7,0,1); r!($i+7, 1,2,3,4,5,6,7,0);
    } }
    delapan!(0); delapan!(8); delapan!(16); delapan!(24); delapan!(32); delapan!(40); delapan!(48); delapan!(56);
    for i in 0..8 { h[i] = h[i].wrapping_add(s[i]); }
}

#[cfg(target_arch = "x86_64")]
mod shani {
    use std::arch::x86_64::*;
    pub fn ada() -> bool {
        std::arch::is_x86_feature_detected!("sha") && std::arch::is_x86_feature_detected!("sse4.1")
            && std::arch::is_x86_feature_detected!("ssse3")
    }
    pub fn kompres(h: &mut [u32; 8], blok: &[u8]) { assert!(blok.len() >= 64); unsafe { kompres_ni(h, blok) } }
    #[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
    unsafe fn kompres_ni(h: &mut [u32; 8], blok: &[u8]) {
        let k = &super::SHA256_K;
        let mask = _mm_set_epi64x(0x0C0D_0E0F_0809_0A0Bu64 as i64, 0x0405_0607_0001_0203u64 as i64);
        let p = h.as_ptr() as *const __m128i;
        let (dcba, efgh) = unsafe { (_mm_loadu_si128(p), _mm_loadu_si128(p.add(1))) };
        let cdab = _mm_shuffle_epi32(dcba, 0xB1);
        let efgh = _mm_shuffle_epi32(efgh, 0x1B);
        let mut abef = _mm_alignr_epi8(cdab, efgh, 8);
        let mut cdgh = _mm_blend_epi16(efgh, cdab, 0xF0);
        let (abef0, cdgh0) = (abef, cdgh);
        let d = blok.as_ptr() as *const __m128i;
        let mut w = [_mm_setzero_si128(); 5];
        for (i, x) in w.iter_mut().take(4).enumerate() { *x = _mm_shuffle_epi8(unsafe { _mm_loadu_si128(d.add(i)) }, mask); }
        for i in 0..16 {
            if i >= 4 {
                let j = i - 4;
                let t1 = _mm_sha256msg1_epu32(w[j % 5], w[(j + 1) % 5]);
                let t2 = _mm_alignr_epi8(w[(j + 3) % 5], w[(j + 2) % 5], 4);
                w[(j + 4) % 5] = _mm_sha256msg2_epu32(_mm_add_epi32(t1, t2), w[(j + 3) % 5]);
            }
            let kv = _mm_set_epi32(k[4*i+3] as i32, k[4*i+2] as i32, k[4*i+1] as i32, k[4*i] as i32);
            let t1 = _mm_add_epi32(w[i % 5], kv);
            cdgh = _mm_sha256rnds2_epu32(cdgh, abef, t1);
            abef = _mm_sha256rnds2_epu32(abef, cdgh, _mm_shuffle_epi32(t1, 0x0E));
        }
        abef = _mm_add_epi32(abef, abef0);
        cdgh = _mm_add_epi32(cdgh, cdgh0);
        let feba = _mm_shuffle_epi32(abef, 0x1B);
        let dchg = _mm_shuffle_epi32(cdgh, 0xB1);
        let q = h.as_mut_ptr() as *mut __m128i;
        unsafe {
            _mm_storeu_si128(q, _mm_blend_epi16(feba, dchg, 0xF0));
            _mm_storeu_si128(q.add(1), _mm_alignr_epi8(dchg, feba, 8));
        }
    }
}

/// Sisa pesan sesudah `sebelum` byte yang sudah dikompres: isi, `0x80`, nol, panjang total dalam bit.
fn sha256_tutup(mut h: [u32; 8], sisa: &[u8], sebelum: u64) -> [u8; 32] {
    let mut msg = sisa.to_vec();
    let ml = (sebelum + sisa.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&ml.to_be_bytes());
    for chunk in msg.chunks(64) { sha256_kompres(&mut h, chunk); }
    let mut out = [0u8; 32];
    for i in 0..8 { out[i*4..i*4+4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}

pub fn sha256(input: &[u8]) -> [u8; 32] { sha256_tutup(SHA256_IV, input, 0) }

/// HMAC-SHA256 dengan kunci yang sudah disiapkan: state sesudah blok ipad/opad dihitung SEKALI, jadi tiap tanda
/// tangan cuma mengompres pesannya — PBKDF2 dulu membayar empat kompresi dan alokasi per putaran.
pub struct HmacSha256 { dalam: [u32; 8], luar: [u32; 8] }

impl HmacSha256 {
    pub fn baru(key: &[u8]) -> HmacSha256 {
        let mut k = [0u8; 64];
        if key.len() > 64 { k[..32].copy_from_slice(&sha256(key)); } else { k[..key.len()].copy_from_slice(key); }
        let (mut dalam, mut luar) = (SHA256_IV, SHA256_IV);
        sha256_kompres(&mut dalam, &k.map(|b| b ^ 0x36));
        sha256_kompres(&mut luar, &k.map(|b| b ^ 0x5c));
        HmacSha256 { dalam, luar }
    }
    pub fn tanda(&self, msg: &[u8]) -> [u8; 32] {
        let dalam = sha256_tutup(self.dalam, msg, 64);
        sha256_tutup(self.luar, &dalam, 64)
    }
    /// `tanda` atas tepat 32 byte (putaran PBKDF2) dengan dua blok tetap, tanpa alokasi.
    pub fn tanda32(&self, m: &[u8; 32]) -> [u8; 32] {
        let mut blok = [0u8; 64];
        blok[32] = 0x80;
        blok[62] = 0x03; // panjang total 64 + 32 byte = 768 bit
        let jadi = |mut h: [u32; 8], isi: &[u8], blok: &mut [u8; 64]| {
            blok[..32].copy_from_slice(isi);
            sha256_kompres(&mut h, blok);
            let mut o = [0u8; 32];
            for i in 0..8 { o[i*4..i*4+4].copy_from_slice(&h[i].to_be_bytes()); }
            o
        };
        let dalam = jadi(self.dalam, m, &mut blok);
        jadi(self.luar, &dalam, &mut blok)
    }
}

const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22,0x7137449123ef65cd,0xb5c0fbcfec4d3b2f,0xe9b5dba58189dbbc,
    0x3956c25bf348b538,0x59f111f1b605d019,0x923f82a4af194f9b,0xab1c5ed5da6d8118,
    0xd807aa98a3030242,0x12835b0145706fbe,0x243185be4ee4b28c,0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,0x80deb1fe3b1696b1,0x9bdc06a725c71235,0xc19bf174cf692694,
    0xe49b69c19ef14ad2,0xefbe4786384f25e3,0x0fc19dc68b8cd5b5,0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,0x4a7484aa6ea6e483,0x5cb0a9dcbd41fbd4,0x76f988da831153b5,
    0x983e5152ee66dfab,0xa831c66d2db43210,0xb00327c898fb213f,0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,0xd5a79147930aa725,0x06ca6351e003826f,0x142929670a0e6e70,
    0x27b70a8546d22ffc,0x2e1b21385c26c926,0x4d2c6dfc5ac42aed,0x53380d139d95b3df,
    0x650a73548baf63de,0x766a0abb3c77b2a8,0x81c2c92e47edaee6,0x92722c851482353b,
    0xa2bfe8a14cf10364,0xa81a664bbc423001,0xc24b8b70d0f89791,0xc76c51a30654be30,
    0xd192e819d6ef5218,0xd69906245565a910,0xf40e35855771202a,0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,0x1e376c085141ab53,0x2748774cdf8eeb99,0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,0x4ed8aa4ae3418acb,0x5b9cca4f7763e373,0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,0x78a5636f43172f60,0x84c87814a1f0ab72,0x8cc702081a6439ec,
    0x90befffa23631e28,0xa4506cebde82bde9,0xbef9a3f7b2c67915,0xc67178f2e372532b,
    0xca273eceea26619c,0xd186b8c721c0c207,0xeada7dd6cde0eb1e,0xf57d4f7fee6ed178,
    0x06f067aa72176fba,0x0a637dc5a2c898a6,0x113f9804bef90dae,0x1b710b35131c471b,
    0x28db77f523047d84,0x32caab7b40c72493,0x3c9ebe0a15c9bebc,0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,0x597f299cfc657e2a,0x5fcb6fab3ad6faec,0x6c44198c4a475817,
];

pub fn sha512(input: &[u8]) -> [u8; 64] {
    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908,0xbb67ae8584caa73b,0x3c6ef372fe94f82b,0xa54ff53a5f1d36f1,
        0x510e527fade682d1,0x9b05688c2b3e6c1f,0x1f83d9abfb41bd6b,0x5be0cd19137e2179,
    ];
    let mut msg = input.to_vec();
    let ml: u128 = (input.len() as u128) * 8;
    msg.push(0x80);
    while msg.len() % 128 != 112 { msg.push(0); }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut b = [0u8; 8]; b.copy_from_slice(&chunk[i*8..i*8+8]); w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i-15].rotate_right(1) ^ w[i-15].rotate_right(8) ^ (w[i-15] >> 7);
            let s1 = w[i-2].rotate_right(19) ^ w[i-2].rotate_right(61) ^ (w[i-2] >> 6);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA512_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(temp1); d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        h[0]=h[0].wrapping_add(a); h[1]=h[1].wrapping_add(b); h[2]=h[2].wrapping_add(c); h[3]=h[3].wrapping_add(d);
        h[4]=h[4].wrapping_add(e); h[5]=h[5].wrapping_add(f); h[6]=h[6].wrapping_add(g); h[7]=h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 64];
    for i in 0..8 { out[i*8..i*8+8].copy_from_slice(&h[i].to_be_bytes()); }
    out
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] { HmacSha256::baru(key).tanda(msg) }

pub use crate::kompres::crc32;

pub fn hex_encode(input: &[u8]) -> String {
    let mut s = String::with_capacity(input.len() * 2);
    for b in input { s.push_str(&format!("{:02x}", b)); }
    s
}

// ─── Wrapper native (Value-level) untuk hashing ─────────────────────────

pub fn f_md5(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&md5(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::md5")?)))))
}
pub fn f_sha1(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&sha1(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::sha1")?)))))
}
pub fn f_sha256(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&sha256(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::sha256")?)))))
}
pub fn f_sha512(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&sha512(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::sha512")?)))))
}
pub fn f_hmac_sha256(args: &[Value]) -> Result<Value, String> {
    let key = isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::hmac_sha256")?;
    let msg = isi_byte(args.get(1).unwrap_or(&Value::Nil), "crypto::hmac_sha256")?;
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&hmac_sha256(&key, &msg)))))
}
/// Perbandingan yang waktunya tidak bergantung pada isinya — satu definisi untuk sesi, sandi, `unseal`
/// dan `crypto::compare_digest`. Panjang yang beda langsung `false`: panjang tanda tangan bukan rahasia.
pub fn sama_waktu_tetap(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let beda = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(beda) == 0
}
/// `crypto::compare_digest(a, b)`: padanan `hmac.compare_digest` Python, untuk tanda tangan dan token.
pub fn f_compare_digest(args: &[Value]) -> Result<Value, String> {
    let a = isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::compare_digest")?;
    let b = isi_byte(args.get(1).unwrap_or(&Value::Nil), "crypto::compare_digest")?;
    Ok(Value::Bool(sama_waktu_tetap(&a, &b)))
}
pub fn f_crc32(args: &[Value]) -> Result<Value, String> {
    let s = isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::crc32")?;
    Ok(Value::Int(crc32(&s) as i128))
}

// ============================================================================
//  ENCODING: Base64 / Hex
// ============================================================================

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode_bytes(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(B64_CHARS[((n >> 18) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { B64_CHARS[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64_CHARS[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}

/// Base64 (RFC 4648) dengan SATU ejaan per isi: spasi di ujung boleh, di tengah cuma CR/LF (baris MIME); `=` cuma
/// di blok terakhir; bit sisa wajib nol. Dulu `aGk=aGk=` terbaca `hihi` dan `aGl=` sama dengan `aGk=` — dua pembaca
/// bisa membaca satu teks berbeda (kelas CVE-2026-3446). Posisi di pesan adalah posisi karakter di teks asli.
pub fn base64_decode_str(input: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62), b'/' => Some(63), _ => None,
        }
    }
    let awal = input.chars().count() - input.trim_start().chars().count();
    let isi = input.trim();
    let mut clean: Vec<(usize, u8)> = Vec::with_capacity(isi.len());
    for (i, ch) in isi.chars().enumerate() {
        let pos = awal + i;
        match ch {
            '\r' | '\n' => {}
            ' ' | '\t' => return Err(format!("base64_decode: spasi di posisi {} — di tengah data cuma ganti baris yang boleh", pos)),
            c if c.is_ascii() && (val(c as u8).is_some() || c == '=') => clean.push((pos, c as u8)),
            c => return Err(format!("base64_decode: karakter `{}` di posisi {} bukan base64 (A-Z a-z 0-9 + /)",
                crate::kutip_inti::kutip(c.to_string()), pos)),
        }
    }
    if clean.is_empty() { return Ok(Vec::new()); }
    if clean.len() % 4 != 0 {
        return Err(format!("base64_decode: panjang {} bukan kelipatan 4 — padding `=` wajib", clean.len()));
    }
    let blok_akhir = clean.len() / 4 - 1;
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for (b, chunk) in clean.chunks(4).enumerate() {
        let c: Vec<u8> = chunk.iter().map(|x| x.1).collect();
        let pad = match (c[2], c[3]) {
            (b'=', b'=') => 2,
            (_, b'=') => 1,
            _ => 0,
        };
        if let Some(&(pos, _)) = chunk[..4 - pad].iter().find(|x| x.1 == b'=') {
            return Err(format!("base64_decode: `=` di posisi {} di tengah data — padding cuma boleh di ujung", pos));
        }
        if pad > 0 && b != blok_akhir {
            return Err(format!("base64_decode: padding di posisi {} sebelum akhir data — dua isi yang disambung?", chunk[4 - pad].0));
        }
        let v: Vec<u32> = c[..4 - pad].iter().map(|&x| val(x).unwrap_or(0)).collect();
        let c2 = if pad < 2 { v[2] } else { 0 };
        let c3 = if pad < 1 { v[3] } else { 0 };
        if (pad == 1 && c2 & 0x3 != 0) || (pad == 2 && v[1] & 0xF != 0) {
            return Err(format!("base64_decode: bit sisa di blok mulai posisi {} bukan nol — ejaan kedua dari byte yang sama", chunk[0].0));
        }
        let n = (v[0] << 18) | (v[1] << 12) | (c2 << 6) | c3;
        out.push(((n >> 16) & 0xFF) as u8);
        if pad < 2 { out.push(((n >> 8) & 0xFF) as u8); }
        if pad < 1 { out.push((n & 0xFF) as u8); }
    }
    Ok(out)
}

/// Base64url tanpa padding (RFC 4648 §5), bentuk untuk URL, cookie dan token.
pub fn base64url_encode_bytes(b: &[u8]) -> String {
    base64_encode_bytes(b).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

/// Base64url KETAT: cuma `[A-Za-z0-9_-]`, tanpa padding, panjang yang mungkin, bit sisa nol — satu isi, satu ejaan.
pub fn base64url_decode_str(s: &str) -> Result<Vec<u8>, String> {
    if let Some((i, c)) = s.chars().enumerate().find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_')) {
        let lain = if c == '=' { " — base64url di sini tanpa padding `=`" } else if c == '+' || c == '/' { " — itu huruf base64 biasa, pakai `crypto::base64_decode`" } else { "" };
        return Err(format!("base64url_decode: karakter `{}` di posisi {} bukan base64url (A-Z a-z 0-9 - _){}",
            crate::kutip_inti::kutip(c.to_string()), i, lain));
    }
    if s.len() % 4 == 1 { return Err(format!("base64url_decode: panjang {} tidak mungkin (sisa bagi 4 tidak boleh 1)", s.len())); }
    let mut t = s.replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 { t.push('='); }
    let b = base64_decode_str(&t).map_err(|e| e.replace("base64_decode:", "base64url_decode:"))?;
    Ok(b)
}

pub fn f_base64url_encode(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(base64url_encode_bytes(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::base64url_encode")?))))
}
pub fn f_base64url_decode(args: &[Value]) -> Result<Value, String> {
    let s = as_str(args.get(0).unwrap_or(&Value::Nil), "crypto::base64url_decode")?;
    match base64url_decode_str(s) {
        Ok(bytes) => Ok(teks_atau_err(bytes, "crypto::base64url_decode", "pakai `crypto::base64url_decode_bytes` untuk mengambil byte-nya")),
        Err(e) => Ok(err_value(e)),
    }
}
pub fn f_base64url_decode_bytes(args: &[Value]) -> Result<Value, String> {
    let s = as_str(args.get(0).unwrap_or(&Value::Nil), "crypto::base64url_decode_bytes")?;
    Ok(match base64url_decode_str(s) { Ok(b) => ok_value(byte_ke_array(b)), Err(e) => err_value(e) })
}

/// Posisinya posisi KARAKTER (seperti `s[i]`) dan karakternya disebut: dulu dihitung
/// per byte berpasangan, jadi `"0g"` menunjuk posisi 0 dan `"aé"` dituduh ganjil.
pub fn hex_decode_str(input: &str) -> Result<Vec<u8>, String> {
    let awal = input.chars().take_while(|c| c.is_whitespace()).count();
    let mut digit = Vec::with_capacity(input.len());
    for (i, c) in input.trim().chars().enumerate() {
        match c.to_digit(16) {
            Some(d) => digit.push(d as u8),
            None => return Err(format!(
                "hex_decode: karakter `{}` di posisi {} bukan digit heksadesimal (0-9, a-f)",
                c, awal + i)),
        }
    }
    if digit.len() % 2 != 0 {
        return Err(format!("hex_decode: jumlah digitnya harus genap, dapat {}", digit.len()));
    }
    Ok(digit.chunks(2).map(|p| p[0] << 4 | p[1]).collect())
}

pub fn f_base64_encode(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(base64_encode_bytes(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::base64_encode")?))))
}
pub fn f_base64_decode(args: &[Value]) -> Result<Value, String> {
    let s = as_str(args.get(0).unwrap_or(&Value::Nil), "crypto::base64_decode")?;
    match base64_decode_str(s) {
        Ok(bytes) => Ok(teks_atau_err(bytes, "crypto::base64_decode", "pakai `crypto::base64_decode_bytes` untuk mengambil byte-nya")),
        Err(e) => Ok(err_value(e)),
    }
}

/// Byte hasil dekode sebagai teks. Yang bukan UTF-8 sah dijawab `Err` yang menyebut jalan
/// keluarnya; dulu byte rusaknya diganti U+FFFD tanpa suara.
fn teks_atau_err(bytes: Vec<u8>, nama: &str, jalan: &str) -> Value {
    let n = bytes.len();
    match String::from_utf8(bytes) {
        Ok(s) => ok_value(Value::Str(std::sync::Arc::new(s))),
        Err(_) => err_value(format!("{}: hasilnya ({} byte) bukan teks UTF-8 yang sah; {}", nama, n, jalan)),
    }
}

/// Kebalikan `byte_ke_array`: tiap elemen harus bilangan 0 sampai 255.
pub(crate) fn array_ke_byte(isi: &[Value], fname: &str) -> Result<Vec<u8>, String> {
    isi.iter().enumerate().map(|(i, e)| match e {
        Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
        Value::Int(n) => Err(format!("{}: elemen ke-{} array byte harus 0 sampai 255, dapat {}", fname, i, n)),
        lain => Err(format!("{}: elemen ke-{} array byte harus bilangan 0 sampai 255, dapat `{}`",
                            fname, i, lain.type_name())),
    }).collect()
}

pub(crate) fn byte_ke_array(bytes: Vec<u8>) -> Value {
    Value::Array(bytes.into_iter().map(|b| Value::Int(b as i128)).collect::<Vec<_>>().into())
}

/// `crypto::base64_decode_bytes`: byte apa pun, sebagai array bilangan 0 sampai 255.
pub fn f_base64_decode_bytes(args: &[Value]) -> Result<Value, String> {
    let s = as_str(args.get(0).unwrap_or(&Value::Nil), "crypto::base64_decode_bytes")?;
    Ok(match base64_decode_str(s) { Ok(b) => ok_value(byte_ke_array(b)), Err(e) => err_value(e) })
}

/// `crypto::hex_decode_bytes`: padanan `base64_decode_bytes` untuk heksadesimal.
pub fn f_hex_decode_bytes(args: &[Value]) -> Result<Value, String> {
    let s = as_str(args.get(0).unwrap_or(&Value::Nil), "crypto::hex_decode_bytes")?;
    Ok(match hex_decode_str(s) { Ok(b) => ok_value(byte_ke_array(b)), Err(e) => err_value(e) })
}
pub fn f_hex_encode(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::hex_encode")?))))
}
pub fn f_hex_decode(args: &[Value]) -> Result<Value, String> {
    let s = as_str(args.get(0).unwrap_or(&Value::Nil), "crypto::hex_decode")?;
    match hex_decode_str(s) {
        Ok(bytes) => Ok(teks_atau_err(bytes, "crypto::hex_decode", "pakai `crypto::hex_decode_bytes` untuk mengambil byte-nya")),
        Err(e) => Ok(err_value(e)),
    }
}

// ============================================================================
//  RANDOM
// ============================================================================

static FALLBACK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Coba `/dev/urandom` (aman secara kriptografis) dulu; kalau sungguhan tidak
/// tersedia (platform non-Unix), baru jatuh ke PRNG splitmix64 yang di-seed …
/// Untuk kripto (kunci, IV, garam, token): sumber OS atau GAGAL — cadangan berbenih jam bisa ditebak.
pub fn acak_aman(n: usize, siapa: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if let Some(dari_host) = crate::host::acak(n) { return Ok(dari_host); }
    let mut buf = vec![0u8; n];
    match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)) {
        Ok(()) => Ok(buf),
        Err(e) => Err(format!("{}: sumber acak yang aman (`/dev/urandom`) tidak bisa dibaca: {} — angka dari jam \
            bisa ditebak, jadi tidak dipakai untuk kripto", siapa, crate::host::galat_io(&e))),
    }
}

pub fn secure_random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    if let Some(dari_host) = crate::host::acak(n) { return dari_host; }
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let mut buf = vec![0u8; n];
        if f.read_exact(&mut buf).is_ok() { return buf; }
    }
    let counter = FALLBACK_COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = crate::host::sejak_epoch().map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut state = nanos ^ counter.wrapping_mul(0x9E3779B97F4A7C15) ^ (std::process::id() as u64);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        out.extend_from_slice(&z.to_le_bytes());
    }
    out.truncate(n);
    out
}

pub fn f_random_hex(args: &[Value]) -> Result<Value, String> {
    let n = match args.get(0) { Some(Value::Int(n)) if *n >= 0 => *n, _ => return Err("crypto::random_hex(n)".to_string()) };
    // Batasnya diperiksa SEBELUM jadi `usize`: `as usize` memotong 2^64+5 jadi 5.
    crate::interpreter::cek_batas_elemen("crypto::random_hex", n)?;
    Ok(Value::Str(std::sync::Arc::new(hex_encode(&acak_aman(n as usize, "crypto::random_hex")?))))
}

pub fn f_random_int(args: &[Value]) -> Result<Value, String> {
    let lo = match args.get(0) { Some(Value::Int(n)) => *n, _ => return Err("crypto::random_int(min, max)".to_string()) };
    let hi = match args.get(1) { Some(Value::Int(n)) => *n, _ => return Err("crypto::random_int(min, max)".to_string()) };
    if hi < lo { return Err("crypto::random_int: max harus >= min".to_string()); }
    // Tanpa bias modulo: `n % m` cuma seragam kalau `n` di bawah kelipatan `m` terbesar, sisanya ditarik ulang
    // (dulu rentang selebar mendekati 2^127 memberi separuh nilainya peluang dua kali lipat).
    let m = (hi.wrapping_sub(lo) as u128).wrapping_add(1);
    let ambang = if m == 0 { 0 } else { m.wrapping_neg() % m };
    loop {
        let mut n: u128 = 0;
        for b in acak_aman(16, "crypto::random_int")? { n = (n << 8) | b as u128; }
        if n >= ambang { return Ok(Value::Int(crate::interpreter::acak_dalam_rentang(lo, hi, n))); }
    }
}

// ============================================================================
//  AES-256 (FIPS-197) — block cipher murni dari nol, mode CBC + padding PKCS7
// ============================================================================

// Tabel S-box cuma untuk tes (`sbox_hitung_sama_dengan_tabel`): AES di bawah tidak membaca tabel berindeks
// rahasia — cache-nya membocorkan kunci lewat waktu (Bernstein 2005). Perangkat keras dulu, sisanya hitungan.
#[cfg(test)]
fn sbox() -> &'static [u8; 256] {
    static SBOX: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    SBOX.get_or_init(|| [
        0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
        0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
        0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
        0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
        0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
        0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
        0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
        0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
        0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
        0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
        0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
        0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
        0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
        0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
        0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
        0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
    ])
}

#[cfg(test)]
fn inv_sbox() -> &'static [u8; 256] {
    static INV: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    INV.get_or_init(|| {
        let s = sbox();
        let mut inv = [0u8; 256];
        for i in 0..256 { inv[s[i] as usize] = i as u8; }
        inv
    })
}

fn xtime(a: u8) -> u8 { if a & 0x80 != 0 { (a << 1) ^ 0x1B } else { a << 1 } }

/// Perkalian GF(2^8) tanpa cabang atas datanya: tiap langkah memakai topeng, bukan `if`.
fn gmul(a: u8, b: u8) -> u8 {
    let (mut a, mut b) = (a, b);
    let mut p = 0u8;
    for _ in 0..8 {
        p ^= a & 0u8.wrapping_sub(b & 1);
        let hi = 0u8.wrapping_sub(a >> 7);
        a = (a << 1) ^ (0x1B & hi);
        b >>= 1;
    }
    p
}

/// Invers GF(2^8) sebagai x^254 (0 tetap 0) — rantai tetap, jadi waktunya tidak bergantung pada `x`.
fn balik_gf(x: u8) -> u8 {
    let x2 = gmul(x, x);
    let x3 = gmul(x2, x);
    let x6 = gmul(x3, x3);
    let x12 = gmul(x6, x6);
    let x15 = gmul(x12, x3);
    let x30 = gmul(x15, x15);
    let x60 = gmul(x30, x30);
    let x120 = gmul(x60, x60);
    let x240 = gmul(x120, x120);
    gmul(gmul(x240, x12), x2)
}

/// S-box AES dihitung: invers lalu transformasi afin (FIPS-197 §5.1.1).
fn sb(x: u8) -> u8 {
    let b = balik_gf(x);
    b ^ b.rotate_left(1) ^ b.rotate_left(2) ^ b.rotate_left(3) ^ b.rotate_left(4) ^ 0x63
}

/// Kebalikan `sb`: afin terbalik lalu invers.
#[cfg(test)]
fn sb_balik(x: u8) -> u8 { balik_gf(x.rotate_left(1) ^ x.rotate_left(3) ^ x.rotate_left(6) ^ 0x05) }

// Rumus yang sama atas delapan byte sekaligus di satu `u64` (SWAR): SubBytes delapan kali lebih murah, tetap
// tanpa cabang dan tanpa tabel. `L` = bit terendah tiap byte.
const L: u64 = 0x0101_0101_0101_0101;
fn xt64(a: u64) -> u64 { ((a & 0x7f7f_7f7f_7f7f_7f7f) << 1) ^ (((a >> 7) & L) * 0x1b) }
fn gmul64(a: u64, b: u64) -> u64 {
    let (mut a, mut p) = (a, 0u64);
    for i in 0..8 { p ^= a & (((b >> i) & L) * 0xff); a = xt64(a); }
    p
}
fn rotl64(b: u64, k: u32) -> u64 {
    ((b << k) & (((0xffu64 << k) & 0xff) * L)) | ((b >> (8 - k)) & ((0xffu64 >> (8 - k)) * L))
}
/// Kuadrat di GF(2^8) linear: XOR dari `(x^i)^2` untuk tiap bit `i` yang menyala — tiga operasi per bit.
fn kuadrat64(x: u64) -> u64 {
    const K: [u64; 8] = [0x01, 0x04, 0x10, 0x40, 0x1b, 0x6c, 0xab, 0x9a];
    let mut p = 0u64;
    for (i, k) in K.iter().enumerate() { p ^= ((x >> i) & L) * k; }
    p
}
fn balik64(x: u64) -> u64 {
    let x2 = kuadrat64(x);
    let x3 = gmul64(x2, x);
    let x6 = kuadrat64(x3);
    let x12 = kuadrat64(x6);
    let x15 = gmul64(x12, x3);
    let x30 = kuadrat64(x15);
    let x60 = kuadrat64(x30);
    let x120 = kuadrat64(x60);
    let x240 = kuadrat64(x120);
    gmul64(gmul64(x240, x12), x2)
}
fn sb64(x: u64) -> u64 {
    let b = balik64(x);
    b ^ rotl64(b, 1) ^ rotl64(b, 2) ^ rotl64(b, 3) ^ rotl64(b, 4) ^ (0x63 * L)
}
fn sb_balik64(x: u64) -> u64 { balik64(rotl64(x, 1) ^ rotl64(x, 3) ^ rotl64(x, 6) ^ (0x05 * L)) }

/// Seluruh state (16 byte) lewat `f` per delapan byte.
fn per_state(state: &mut [[u8; 4]; 4], f: fn(u64) -> u64) {
    for separuh in 0..2 {
        let mut b = [0u8; 8];
        for i in 0..8 { b[i] = state[separuh * 2 + i / 4][i % 4]; }
        let h = f(u64::from_le_bytes(b)).to_le_bytes();
        for i in 0..8 { state[separuh * 2 + i / 4][i % 4] = h[i]; }
    }
}

/// Kali 2 di GF(2^8) bertopeng — MixColumns lewat kelipatan ini, bukan `gmul` delapan putaran.
fn xt(a: u8) -> u8 { (a << 1) ^ (0x1B & 0u8.wrapping_sub(a >> 7)) }

fn rcon(i: usize) -> u8 {
    let mut c = 1u8;
    for _ in 1..i { c = xtime(c); }
    c
}

/// AES-256: kunci 8 word (32 byte) → 60 word key schedule (Nr=14, 15 round key @ 4 word).
fn key_expansion_256(key: &[u8; 32]) -> [[u8; 4]; 60] {
    let mut w: [[u8; 4]; 60] = [[0; 4]; 60];
    for i in 0..8 { w[i] = [key[4*i], key[4*i+1], key[4*i+2], key[4*i+3]]; }
    for i in 8..60 {
        let mut temp = w[i-1];
        if i % 8 == 0 {
            temp = [temp[1], temp[2], temp[3], temp[0]];
            temp = [sb(temp[0]), sb(temp[1]), sb(temp[2]), sb(temp[3])];
            temp[0] ^= rcon(i / 8);
        } else if i % 8 == 4 {
            temp = [sb(temp[0]), sb(temp[1]), sb(temp[2]), sb(temp[3])];
        }
        for j in 0..4 { w[i][j] = w[i-8][j] ^ temp[j]; }
    }
    w
}

fn add_round_key(state: &mut [[u8; 4]; 4], w: &[[u8; 4]; 60], round: usize) {
    for c in 0..4 { for r in 0..4 { state[r][c] ^= w[round*4 + c][r]; } }
}

fn sub_bytes(state: &mut [[u8; 4]; 4]) { per_state(state, sb64) }
fn inv_sub_bytes(state: &mut [[u8; 4]; 4]) { per_state(state, sb_balik64) }
fn shift_rows(state: &mut [[u8; 4]; 4]) { for r in 1..4 { state[r].rotate_left(r); } }
fn inv_shift_rows(state: &mut [[u8; 4]; 4]) { for r in 1..4 { state[r].rotate_right(r); } }

fn mix_columns(state: &mut [[u8; 4]; 4]) {
    for c in 0..4 {
        let (a0, a1, a2, a3) = (state[0][c], state[1][c], state[2][c], state[3][c]);
        let (b0, b1, b2, b3) = (xt(a0), xt(a1), xt(a2), xt(a3));
        state[0][c] = b0 ^ b1 ^ a1 ^ a2 ^ a3;
        state[1][c] = a0 ^ b1 ^ b2 ^ a2 ^ a3;
        state[2][c] = a0 ^ a1 ^ b2 ^ b3 ^ a3;
        state[3][c] = b0 ^ a0 ^ a1 ^ a2 ^ b3;
    }
}
fn inv_mix_columns(state: &mut [[u8; 4]; 4]) {
    for c in 0..4 {
        // 9 = 8+1, 11 = 8+2+1, 13 = 8+4+1, 14 = 8+4+2, dari kelipatan 2/4/8 bertopeng.
        let k = |a: u8| { let (d, e) = (xt(a), xt(xt(a))); let l = xt(e); (l ^ a, l ^ d ^ a, l ^ e ^ a, l ^ e ^ d) };
        let (a0, a1, a2, a3) = (k(state[0][c]), k(state[1][c]), k(state[2][c]), k(state[3][c]));
        state[0][c] = a0.3 ^ a1.1 ^ a2.2 ^ a3.0;
        state[1][c] = a0.0 ^ a1.3 ^ a2.1 ^ a3.2;
        state[2][c] = a0.2 ^ a1.0 ^ a2.3 ^ a3.1;
        state[3][c] = a0.1 ^ a1.2 ^ a2.0 ^ a3.3;
    }
}

fn bytes_to_state(block: &[u8; 16]) -> [[u8; 4]; 4] {
    let mut s = [[0u8; 4]; 4];
    for i in 0..16 { s[i % 4][i / 4] = block[i]; }
    s
}
fn state_to_bytes(state: &[[u8; 4]; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for i in 0..16 { out[i] = state[i % 4][i / 4]; }
    out
}

const NR: usize = 14; // AES-256 selalu 14 round

fn encrypt_block(block: &[u8; 16], w: &[[u8; 4]; 60]) -> [u8; 16] {
    let mut state = bytes_to_state(block);
    add_round_key(&mut state, w, 0);
    for round in 1..NR {
        sub_bytes(&mut state); shift_rows(&mut state); mix_columns(&mut state);
        add_round_key(&mut state, w, round);
    }
    sub_bytes(&mut state); shift_rows(&mut state);
    add_round_key(&mut state, w, NR);
    state_to_bytes(&state)
}

fn decrypt_block(block: &[u8; 16], w: &[[u8; 4]; 60]) -> [u8; 16] {
    let mut state = bytes_to_state(block);
    add_round_key(&mut state, w, NR);
    for round in (1..NR).rev() {
        inv_shift_rows(&mut state); inv_sub_bytes(&mut state);
        add_round_key(&mut state, w, round);
        inv_mix_columns(&mut state);
    }
    inv_shift_rows(&mut state); inv_sub_bytes(&mut state);
    add_round_key(&mut state, w, 0);
    state_to_bytes(&state)
}

/// Jadwal kunci dan jalurnya: AES-NI kalau prosesornya punya (waktu tetap, cepat), selain itu hitungan di atas.
struct KunciAes { w: [[u8; 4]; 60], rk: [[u8; 16]; 15], ni: bool }

impl KunciAes {
    fn baru(key: &[u8; 32]) -> KunciAes { KunciAes::dengan(key, ni::ada()) }
    fn dengan(key: &[u8; 32], ni: bool) -> KunciAes {
        let w = key_expansion_256(key);
        let mut rk = [[0u8; 16]; 15];
        for (r, k) in rk.iter_mut().enumerate() { for i in 0..16 { k[i] = w[r * 4 + i / 4][i % 4]; } }
        KunciAes { w, rk, ni }
    }
    fn sandi(&self, b: &[u8; 16]) -> [u8; 16] { if self.ni { ni::sandi(b, &self.rk) } else { encrypt_block(b, &self.w) } }
    fn buka(&self, b: &[u8; 16]) -> [u8; 16] { if self.ni { ni::buka(b, &self.rk) } else { decrypt_block(b, &self.w) } }
}

#[cfg(target_arch = "x86_64")]
mod ni {
    use std::arch::x86_64::*;
    pub fn ada() -> bool { std::arch::is_x86_feature_detected!("aes") && std::arch::is_x86_feature_detected!("sse2") }
    pub fn sandi(b: &[u8; 16], rk: &[[u8; 16]; 15]) -> [u8; 16] { unsafe { sandi_ni(b, rk) } }
    pub fn buka(b: &[u8; 16], rk: &[[u8; 16]; 15]) -> [u8; 16] { unsafe { buka_ni(b, rk) } }
    #[target_feature(enable = "aes,sse2")]
    unsafe fn sandi_ni(b: &[u8; 16], rk: &[[u8; 16]; 15]) -> [u8; 16] {
        let k = |i: usize| unsafe { _mm_loadu_si128(rk[i].as_ptr() as *const __m128i) };
        let mut s = unsafe { _mm_xor_si128(_mm_loadu_si128(b.as_ptr() as *const __m128i), k(0)) };
        for i in 1..14 { s = _mm_aesenc_si128(s, k(i)); }
        s = _mm_aesenclast_si128(s, k(14));
        let mut o = [0u8; 16];
        unsafe { _mm_storeu_si128(o.as_mut_ptr() as *mut __m128i, s) };
        o
    }
    #[target_feature(enable = "aes,sse2")]
    unsafe fn buka_ni(b: &[u8; 16], rk: &[[u8; 16]; 15]) -> [u8; 16] {
        let k = |i: usize| unsafe { _mm_loadu_si128(rk[i].as_ptr() as *const __m128i) };
        let mut s = unsafe { _mm_xor_si128(_mm_loadu_si128(b.as_ptr() as *const __m128i), k(14)) };
        for i in (1..14).rev() { s = _mm_aesdec_si128(s, _mm_aesimc_si128(k(i))); }
        s = _mm_aesdeclast_si128(s, k(0));
        let mut o = [0u8; 16];
        unsafe { _mm_storeu_si128(o.as_mut_ptr() as *mut __m128i, s) };
        o
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod ni {
    pub fn ada() -> bool { false }
    pub fn sandi(_: &[u8; 16], _: &[[u8; 16]; 15]) -> [u8; 16] { unreachable!("tanpa AES-NI") }
    pub fn buka(_: &[u8; 16], _: &[[u8; 16]; 15]) -> [u8; 16] { unreachable!("tanpa AES-NI") }
}

pub fn aes256_cbc_encrypt(plaintext: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let k = KunciAes::baru(key);
    let mut padded = plaintext.to_vec();
    let pad_len = 16 - (padded.len() % 16);
    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    let mut out = Vec::with_capacity(padded.len());
    let mut prev = *iv;
    for chunk in padded.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        for i in 0..16 { block[i] ^= prev[i]; }
        let enc = k.sandi(&block);
        out.extend_from_slice(&enc);
        prev = enc;
    }
    out
}

pub fn aes256_cbc_decrypt(ciphertext: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err("aes_decrypt: ciphertext harus kelipatan 16 byte dan tidak kosong".to_string());
    }
    let k = KunciAes::baru(key);
    let mut out = Vec::with_capacity(ciphertext.len());
    let mut prev = *iv;
    for chunk in ciphertext.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let dec = k.buka(&block);
        let mut plain = [0u8; 16];
        for i in 0..16 { plain[i] = dec[i] ^ prev[i]; }
        out.extend_from_slice(&plain);
        prev = block;
    }
    match out.last().copied() {
        Some(pad) if pad >= 1 && pad as usize <= 16 && (pad as usize) <= out.len()
            && out[out.len() - pad as usize..].iter().all(|&b| b == pad) => {
            out.truncate(out.len() - pad as usize);
            Ok(out)
        }
        _ => Err("aes_decrypt: padding tidak valid (kunci/IV salah, atau data korup)".to_string()),
    }
}

// ─── Wrapper native (Value-level) untuk AES ─────────────────────────────

fn parse_key32(s: &str, fname: &str) -> Result<[u8; 32], String> {
    let bytes = hex_decode_str(s).map_err(|e| format!("{}: key tidak valid ({})", fname, e))?;
    if bytes.len() != 32 { return Err(format!("{}: key harus 32 byte (64 karakter hex) buat AES-256, dapat {} byte", fname, bytes.len())); }
    let mut k = [0u8; 32]; k.copy_from_slice(&bytes); Ok(k)
}
fn parse_iv16(s: &str, fname: &str) -> Result<[u8; 16], String> {
    let bytes = hex_decode_str(s).map_err(|e| format!("{}: iv tidak valid ({})", fname, e))?;
    if bytes.len() != 16 { return Err(format!("{}: iv harus 16 byte (32 karakter hex), dapat {} byte", fname, bytes.len())); }
    let mut iv = [0u8; 16]; iv.copy_from_slice(&bytes); Ok(iv)
}

pub fn f_aes_gen_key(_args: &[Value]) -> Result<Value, String> { Ok(Value::Str(std::sync::Arc::new(hex_encode(&acak_aman(32, "crypto::aes_gen_key")?)))) }
pub fn f_aes_gen_iv(_args: &[Value]) -> Result<Value, String> { Ok(Value::Str(std::sync::Arc::new(hex_encode(&acak_aman(16, "crypto::aes_gen_iv")?)))) }

pub fn f_aes_encrypt(args: &[Value]) -> Result<Value, String> {
    let plaintext = isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::aes_encrypt")?;
    let key_hex = as_str(args.get(1).unwrap_or(&Value::Nil), "crypto::aes_encrypt")?;
    let iv_hex = as_str(args.get(2).unwrap_or(&Value::Nil), "crypto::aes_encrypt")?;
    let key = match parse_key32(key_hex, "crypto::aes_encrypt") { Ok(k) => k, Err(e) => return Ok(err_value(e)) };
    let iv = match parse_iv16(iv_hex, "crypto::aes_encrypt") { Ok(v) => v, Err(e) => return Ok(err_value(e)) };
    let ct = aes256_cbc_encrypt(&plaintext, &key, &iv);
    Ok(ok_value(Value::Str(std::sync::Arc::new(base64_encode_bytes(&ct)))))
}

pub fn f_aes_decrypt(args: &[Value]) -> Result<Value, String> {
    Ok(match aes_buka(args, "crypto::aes_decrypt")? {
        Ok(bytes) => teks_atau_err(bytes, "crypto::aes_decrypt",
            "kunci atau IV-nya mungkin bukan yang dipakai menyandikan; isi biner diambil `crypto::aes_decrypt_bytes`"),
        Err(e) => err_value(e),
    })
}

// ─── seal/unseal: AES-256-CBC + HMAC-SHA256 (enkripsi lalu MAC) ──────────
// `aes_encrypt` tanpa MAC: sandi yang diubah tetap dibuka, dan bedanya galat padding dengan
// berhasil adalah oracle (kelas CVE-2016-2107). Kotak: IV 16 || sandi || tanda 32, base64.

fn kunci_kotak(key_hex: &str, nama: &str) -> Result<([u8; 32], [u8; 32]), String> {
    let k = parse_key32(key_hex, nama)?;
    Ok((hmac_sha256(&k, b"jrg-seal-enc"), hmac_sha256(&k, b"jrg-seal-mac")))
}

/// `crypto::seal(isi, kunci)` → `Result<string>`; IV acak dari OS, di dalam kotaknya.
pub fn f_seal(args: &[Value]) -> Result<Value, String> {
    const N: &str = "crypto::seal";
    let isi = isi_byte(args.get(0).unwrap_or(&Value::Nil), N)?;
    let key_hex = as_str(args.get(1).unwrap_or(&Value::Nil), N)?;
    let (ke, km) = match kunci_kotak(key_hex, N) { Ok(k) => k, Err(e) => return Ok(err_value(e)) };
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&acak_aman(16, N)?);
    let mut kotak = iv.to_vec();
    kotak.extend(aes256_cbc_encrypt(&isi, &ke, &iv));
    let tanda = hmac_sha256(&km, &kotak);
    kotak.extend_from_slice(&tanda);
    Ok(ok_value(Value::Str(std::sync::Arc::new(base64_encode_bytes(&kotak)))))
}

/// Tanda diperiksa (waktu tetap) SEBELUM didekripsi: kotak yang diubah satu bit tidak pernah dibuka.
fn buka_kotak(args: &[Value], nama: &str) -> Result<Result<Vec<u8>, String>, String> {
    let kotak_b64 = as_str(args.get(0).unwrap_or(&Value::Nil), nama)?;
    let key_hex = as_str(args.get(1).unwrap_or(&Value::Nil), nama)?;
    Ok((|| {
        let (ke, km) = kunci_kotak(key_hex, nama)?;
        let kotak = base64_decode_str(kotak_b64).map_err(|e| format!("{}: {}", nama, e))?;
        if kotak.len() < 16 + 16 + 32 {
            return Err(format!("{}: kotaknya terlalu pendek ({} byte) — bukan hasil `crypto::seal`", nama, kotak.len()));
        }
        let (isi, tanda) = kotak.split_at(kotak.len() - 32);
        let harus = hmac_sha256(&km, isi);
        if !sama_waktu_tetap(&harus, tanda) {
            return Err(format!("{}: tanda tangan tidak cocok — kuncinya lain, atau kotaknya diubah", nama));
        }
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&isi[..16]);
        aes256_cbc_decrypt(&isi[16..], &ke, &iv).map_err(|e| format!("{}: {}", nama, e))
    })())
}

pub fn f_unseal(args: &[Value]) -> Result<Value, String> {
    Ok(match buka_kotak(args, "crypto::unseal")? {
        Ok(b) => teks_atau_err(b, "crypto::unseal", "isi biner diambil `crypto::unseal_bytes`"),
        Err(e) => err_value(e),
    })
}

pub fn f_unseal_bytes(args: &[Value]) -> Result<Value, String> {
    Ok(match buka_kotak(args, "crypto::unseal_bytes")? { Ok(b) => ok_value(byte_ke_array(b)), Err(e) => err_value(e) })
}

/// `crypto::aes_decrypt_bytes`: isi yang dibuka sebagai array byte, teks atau bukan.
pub fn f_aes_decrypt_bytes(args: &[Value]) -> Result<Value, String> {
    Ok(match aes_buka(args, "crypto::aes_decrypt_bytes")? { Ok(b) => ok_value(byte_ke_array(b)), Err(e) => err_value(e) })
}

/// Galat argumen di luar, galat data (kunci, IV, sandi) di dalam: yang dalam jadi `Err`.
fn aes_buka(args: &[Value], nama: &str) -> Result<Result<Vec<u8>, String>, String> {
    let ct_b64 = as_str(args.get(0).unwrap_or(&Value::Nil), nama)?;
    let key_hex = as_str(args.get(1).unwrap_or(&Value::Nil), nama)?;
    let iv_hex = as_str(args.get(2).unwrap_or(&Value::Nil), nama)?;
    Ok((|| {
        let key = parse_key32(key_hex, nama)?;
        let iv = parse_iv16(iv_hex, nama)?;
        aes256_cbc_decrypt(&base64_decode_str(ct_b64)?, &key, &iv)
    })())
}

#[cfg(test)]
mod uji_base64 {
    use super::{base64_decode_str as b64, base64url_decode_str as url, base64url_encode_bytes as url_enc};

    #[test]
    fn satu_isi_satu_ejaan() {
        assert_eq!(b64("aGk=").unwrap(), b"hi");
        assert_eq!(b64(" aGk= \n").unwrap(), b"hi");
        assert_eq!(b64("aG\r\nk=").unwrap(), b"hi");
        assert_eq!(b64("").unwrap(), b"");
        assert_eq!(b64("aGk=aGk=").unwrap_err(), "base64_decode: padding di posisi 3 sebelum akhir data — dua isi yang disambung?");
        assert_eq!(b64("aGl=").unwrap_err(), "base64_decode: bit sisa di blok mulai posisi 0 bukan nol — ejaan kedua dari byte yang sama");
        assert_eq!(b64("YR==").unwrap_err(), "base64_decode: bit sisa di blok mulai posisi 0 bukan nol — ejaan kedua dari byte yang sama");
        assert_eq!(b64("YQ==").unwrap(), b"a");
        assert_eq!(b64("aG k=").unwrap_err(), "base64_decode: spasi di posisi 2 — di tengah data cuma ganti baris yang boleh");
        assert_eq!(b64("ab=c").unwrap_err(), "base64_decode: `=` di posisi 2 di tengah data — padding cuma boleh di ujung");
        assert_eq!(b64("a===").unwrap_err(), "base64_decode: `=` di posisi 1 di tengah data — padding cuma boleh di ujung");
        assert_eq!(b64("aGk").unwrap_err(), "base64_decode: panjang 3 bukan kelipatan 4 — padding `=` wajib");
        assert_eq!(b64("aGk-").unwrap_err(), "base64_decode: karakter `-` di posisi 3 bukan base64 (A-Z a-z 0-9 + /)");
        assert_eq!(b64("é").unwrap_err(), "base64_decode: karakter `é` di posisi 0 bukan base64 (A-Z a-z 0-9 + /)");
        assert_eq!(url_enc(&[251, 255]), "-_8");
        assert_eq!(url("-_8").unwrap(), [251, 255]);
        assert_eq!(url("aGk").unwrap(), b"hi");
        assert_eq!(url("").unwrap(), b"");
        assert_eq!(url("aGl").unwrap_err(), "base64url_decode: bit sisa di blok mulai posisi 0 bukan nol — ejaan kedua dari byte yang sama");
        assert!(url("aGk=").unwrap_err().contains("tanpa padding"));
        assert!(url("a+b").unwrap_err().contains("pakai `crypto::base64_decode`"));
        assert_eq!(url("a").unwrap_err(), "base64url_decode: panjang 1 tidak mungkin (sisa bagi 4 tidak boleh 1)");
        for n in 0..300usize {
            let isi: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(b64(&super::base64_encode_bytes(&isi)).unwrap(), isi);
            assert_eq!(url(&url_enc(&isi)).unwrap(), isi);
        }
    }
}

#[cfg(test)]
mod uji_acak {
    /// Rentang 3·2^126: dengan bias modulo sepertiga pertama keluar separuh waktu, tanpa bias sepertiga.
    #[test]
    fn random_int_tanpa_bias_modulo() {
        use crate::interpreter::Value;
        // `i128::MIN ..= 2^126 - 1` lebarnya 3·2^126; sepertiga pertamanya di bawah `-2^126`.
        let (lo, hi, batas) = (i128::MIN, (1i128 << 126) - 1, -(1i128 << 126));
        let (mut bawah, n) = (0, 3000);
        for _ in 0..n {
            match super::f_random_int(&[Value::Int(lo), Value::Int(hi)]).unwrap() {
                Value::Int(x) => { assert!(lo <= x && x <= hi); if x < batas { bawah += 1 } }
                lain => panic!("{:?}", lain),
            }
        }
        let f = bawah as f64 / n as f64;
        assert!((0.28..0.39).contains(&f), "pecahan sepertiga pertama {} (bias modulo memberi 0,5)", f);
        for (lo, hi) in [(5, 5), (i128::MIN, i128::MAX), (-3, 3)] {
            let Value::Int(x) = super::f_random_int(&[Value::Int(lo), Value::Int(hi)]).unwrap() else { panic!() };
            assert!(lo <= x && x <= hi);
        }
    }
}

#[cfg(test)]
mod uji_aes {
    use super::*;
    /// S-box hitungan sama persis dengan tabel FIPS-197, dua arah.
    #[test]
    fn sbox_hitung_sama_dengan_tabel() {
        for x in 0..=255u8 {
            assert_eq!(sb(x), sbox()[x as usize], "sbox {:#x}", x);
            assert_eq!(sb_balik(x), inv_sbox()[x as usize], "inv_sbox {:#x}", x);
            // Jalur SWAR: tiap lajur `u64` sama dengan rumus satu byte, di posisi mana pun.
            let lajur = u64::from_le_bytes(core::array::from_fn(|i| x.wrapping_add(37u8.wrapping_mul(i as u8))));
            let (h, hb) = (sb64(lajur).to_le_bytes(), sb_balik64(lajur).to_le_bytes());
            for i in 0..8 {
                let y = x.wrapping_add(37u8.wrapping_mul(i as u8));
                assert_eq!((h[i], hb[i]), (sbox()[y as usize], inv_sbox()[y as usize]), "lajur {} {:#x}", i, y);
            }
        }
    }
    /// FIPS-197 C.3 (AES-256), lewat dua jalur, plus blok acak: AES-NI dan hitungan wajib sama.
    #[test]
    fn aes256_fips197_dan_dua_jalur_sama() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let pt: [u8; 16] = core::array::from_fn(|i| (i as u8) * 0x11);
        let harap = [0x8e, 0xa2, 0xb7, 0xca, 0x51, 0x67, 0x45, 0xbf, 0xea, 0xfc, 0x49, 0x90, 0x4b, 0x49, 0x60, 0x89];
        let lunak = KunciAes::dengan(&key, false);
        assert_eq!(lunak.sandi(&pt), harap);
        assert_eq!(lunak.buka(&harap), pt);
        if ni::ada() {
            let keras = KunciAes::dengan(&key, true);
            assert_eq!(keras.sandi(&pt), harap);
            assert_eq!(keras.buka(&harap), pt);
            let mut x = 0x9e3779b97f4a7c15u64;
            for _ in 0..200 {
                let mut acak = || { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x as u8 };
                let k: [u8; 32] = core::array::from_fn(|_| acak());
                let b: [u8; 16] = core::array::from_fn(|_| acak());
                let (l, h) = (KunciAes::dengan(&k, false), KunciAes::dengan(&k, true));
                assert_eq!(l.sandi(&b), h.sandi(&b));
                assert_eq!(l.buka(&b), h.buka(&b));
            }
        }
    }
}

#[cfg(test)]
mod uji_hmac {
    use super::*;
    /// RFC 4231 kasus 1 dan 6 (kunci lebih panjang dari blok), dan `tanda32` sama dengan `tanda` umum.
    #[test]
    fn hmac_rfc4231_dan_jalur_pbkdf2_sama() {
        assert_eq!(hex_encode(&hmac_sha256(&[0x0b; 20], b"Hi There")),
                   "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        assert_eq!(hex_encode(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
                   "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
        assert_eq!(hex_encode(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // SHA-NI (kalau ada) sama dengan jalur skalar atas blok dan state acak.
        let mut x = 88172645463325252u64;
        for _ in 0..500 {
            let mut acak = || { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x };
            let h0: [u32; 8] = core::array::from_fn(|_| acak() as u32);
            let b: [u8; 64] = core::array::from_fn(|_| acak() as u8);
            let (mut p, mut q) = (h0, h0);
            sha256_kompres(&mut p, &b);
            sha256_kompres_lunak(&mut q, &b);
            assert_eq!(p, q);
        }
        let mac = HmacSha256::baru(b"kunci");
        let mut u = [7u8; 32];
        for _ in 0..50 { assert_eq!(mac.tanda32(&u), mac.tanda(&u)); u = mac.tanda(&u); }
    }
}
