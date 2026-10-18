// src/dok/crypto.rs — crypto:: — hashing (MD5/SHA-1/SHA-256/SHA-512), HMAC-SHA256, CRC32,
// Base64/Hex, random aman (OS RNG), dan AES-256-CBC. Semuanya ditulis

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

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

pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    let mut msg = input.to_vec();
    let ml = (input.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(temp1); d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        h[0]=h[0].wrapping_add(a); h[1]=h[1].wrapping_add(b); h[2]=h[2].wrapping_add(c); h[3]=h[3].wrapping_add(d);
        h[4]=h[4].wrapping_add(e); h[5]=h[5].wrapping_add(f); h[6]=h[6].wrapping_add(g); h[7]=h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for i in 0..8 { out[i*4..i*4+4].copy_from_slice(&h[i].to_be_bytes()); }
    out
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

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = if key.len() > BLOCK { sha256(key).to_vec() } else { key.to_vec() };
    key_block.resize(BLOCK, 0);
    let mut ipad = vec![0x36u8; BLOCK];
    let mut opad = vec![0x5cu8; BLOCK];
    for i in 0..BLOCK { ipad[i] ^= key_block[i]; opad[i] ^= key_block[i]; }
    let mut inner = ipad; inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);
    let mut outer = opad; outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

fn crc32_table() -> &'static [u32; 256] {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for i in 0..256u32 {
            let mut c = i;
            for _ in 0..8 { c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 }; }
            table[i as usize] = c;
        }
        table
    })
}

pub fn crc32(input: &[u8]) -> u32 {
    let table = crc32_table();
    let mut crc: u32 = 0xFFFFFFFF;
    for &b in input { crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8); }
    crc ^ 0xFFFFFFFF
}

pub fn hex_encode(input: &[u8]) -> String {
    let mut s = String::with_capacity(input.len() * 2);
    for b in input { s.push_str(&format!("{:02x}", b)); }
    s
}

// ─── Wrapper native (Value-level) untuk hashing ─────────────────────────

pub fn f_md5(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(hex_encode(&md5(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::md5")?))))
}
pub fn f_sha1(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(hex_encode(&sha1(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::sha1")?))))
}
pub fn f_sha256(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(hex_encode(&sha256(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::sha256")?))))
}
pub fn f_sha512(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Str(hex_encode(&sha512(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::sha512")?))))
}
pub fn f_hmac_sha256(args: &[Value]) -> Result<Value, String> {
    let key = isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::hmac_sha256")?;
    let msg = isi_byte(args.get(1).unwrap_or(&Value::Nil), "crypto::hmac_sha256")?;
    Ok(Value::Str(hex_encode(&hmac_sha256(&key, &msg))))
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

pub fn base64_decode_str(input: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62), b'/' => Some(63), _ => None,
        }
    }
    let clean: Vec<u8> = input.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() { return Ok(Vec::new()); }
    if clean.len() % 4 != 0 { return Err("base64_decode: panjang input tidak valid (harus kelipatan 4)".to_string()); }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let c0 = val(chunk[0]).ok_or("base64_decode: karakter tidak valid")?;
        let c1 = val(chunk[1]).ok_or("base64_decode: karakter tidak valid")?;
        let c2 = if chunk[2] == b'=' { 0 } else { val(chunk[2]).ok_or("base64_decode: karakter tidak valid")? };
        let c3 = if chunk[3] == b'=' { 0 } else { val(chunk[3]).ok_or("base64_decode: karakter tidak valid")? };
        let n = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
        out.push(((n >> 16) & 0xFF) as u8);
        if pad < 2 { out.push(((n >> 8) & 0xFF) as u8); }
        if pad < 1 { out.push((n & 0xFF) as u8); }
    }
    Ok(out)
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
    Ok(Value::Str(base64_encode_bytes(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::base64_encode")?)))
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
        Ok(s) => ok_value(Value::Str(s)),
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
    Ok(Value::Str(hex_encode(&isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::hex_encode")?)))
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
    Ok(Value::Str(hex_encode(&secure_random_bytes(n as usize))))
}

pub fn f_random_int(args: &[Value]) -> Result<Value, String> {
    let lo = match args.get(0) { Some(Value::Int(n)) => *n, _ => return Err("crypto::random_int(min, max)".to_string()) };
    let hi = match args.get(1) { Some(Value::Int(n)) => *n, _ => return Err("crypto::random_int(min, max)".to_string()) };
    if hi < lo { return Err("crypto::random_int: max harus >= min".to_string()); }
    let bytes = secure_random_bytes(16);
    let mut n: u128 = 0;
    for b in bytes { n = (n << 8) | b as u128; }
    Ok(Value::Int(crate::interpreter::acak_dalam_rentang(lo, hi, n)))
}

// ============================================================================
//  AES-256 (FIPS-197) — block cipher murni dari nol, mode CBC + padding PKCS7
// ============================================================================

fn sbox() -> &'static [u8; 256] {
    static SBOX: OnceLock<[u8; 256]> = OnceLock::new();
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

fn inv_sbox() -> &'static [u8; 256] {
    static INV: OnceLock<[u8; 256]> = OnceLock::new();
    INV.get_or_init(|| {
        let s = sbox();
        let mut inv = [0u8; 256];
        for i in 0..256 { inv[s[i] as usize] = i as u8; }
        inv
    })
}

fn xtime(a: u8) -> u8 { if a & 0x80 != 0 { (a << 1) ^ 0x1B } else { a << 1 } }

fn gmul(a: u8, b: u8) -> u8 {
    let (mut a, mut b) = (a, b);
    let mut p = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 { p ^= a; }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 { a ^= 0x1B; }
        b >>= 1;
    }
    p
}

fn rcon(i: usize) -> u8 {
    let mut c = 1u8;
    for _ in 1..i { c = xtime(c); }
    c
}

/// AES-256: kunci 8 word (32 byte) → 60 word key schedule (Nr=14, 15 round key @ 4 word).
fn key_expansion_256(key: &[u8; 32]) -> [[u8; 4]; 60] {
    let sbox = sbox();
    let mut w: [[u8; 4]; 60] = [[0; 4]; 60];
    for i in 0..8 { w[i] = [key[4*i], key[4*i+1], key[4*i+2], key[4*i+3]]; }
    for i in 8..60 {
        let mut temp = w[i-1];
        if i % 8 == 0 {
            temp = [temp[1], temp[2], temp[3], temp[0]];
            temp = [sbox[temp[0] as usize], sbox[temp[1] as usize], sbox[temp[2] as usize], sbox[temp[3] as usize]];
            temp[0] ^= rcon(i / 8);
        } else if i % 8 == 4 {
            temp = [sbox[temp[0] as usize], sbox[temp[1] as usize], sbox[temp[2] as usize], sbox[temp[3] as usize]];
        }
        for j in 0..4 { w[i][j] = w[i-8][j] ^ temp[j]; }
    }
    w
}

fn add_round_key(state: &mut [[u8; 4]; 4], w: &[[u8; 4]; 60], round: usize) {
    for c in 0..4 { for r in 0..4 { state[r][c] ^= w[round*4 + c][r]; } }
}

fn sub_bytes(state: &mut [[u8; 4]; 4]) {
    let s = sbox();
    for row in state.iter_mut() { for v in row.iter_mut() { *v = s[*v as usize]; } }
}
fn inv_sub_bytes(state: &mut [[u8; 4]; 4]) {
    let s = inv_sbox();
    for row in state.iter_mut() { for v in row.iter_mut() { *v = s[*v as usize]; } }
}
fn shift_rows(state: &mut [[u8; 4]; 4]) { for r in 1..4 { state[r].rotate_left(r); } }
fn inv_shift_rows(state: &mut [[u8; 4]; 4]) { for r in 1..4 { state[r].rotate_right(r); } }

fn mix_columns(state: &mut [[u8; 4]; 4]) {
    for c in 0..4 {
        let (a0, a1, a2, a3) = (state[0][c], state[1][c], state[2][c], state[3][c]);
        state[0][c] = gmul(a0,2) ^ gmul(a1,3) ^ a2 ^ a3;
        state[1][c] = a0 ^ gmul(a1,2) ^ gmul(a2,3) ^ a3;
        state[2][c] = a0 ^ a1 ^ gmul(a2,2) ^ gmul(a3,3);
        state[3][c] = gmul(a0,3) ^ a1 ^ a2 ^ gmul(a3,2);
    }
}
fn inv_mix_columns(state: &mut [[u8; 4]; 4]) {
    for c in 0..4 {
        let (a0, a1, a2, a3) = (state[0][c], state[1][c], state[2][c], state[3][c]);
        state[0][c] = gmul(a0,14) ^ gmul(a1,11) ^ gmul(a2,13) ^ gmul(a3,9);
        state[1][c] = gmul(a0,9)  ^ gmul(a1,14) ^ gmul(a2,11) ^ gmul(a3,13);
        state[2][c] = gmul(a0,13) ^ gmul(a1,9)  ^ gmul(a2,14) ^ gmul(a3,11);
        state[3][c] = gmul(a0,11) ^ gmul(a1,13) ^ gmul(a2,9)  ^ gmul(a3,14);
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

pub fn aes256_cbc_encrypt(plaintext: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let w = key_expansion_256(key);
    let mut padded = plaintext.to_vec();
    let pad_len = 16 - (padded.len() % 16);
    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    let mut out = Vec::with_capacity(padded.len());
    let mut prev = *iv;
    for chunk in padded.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        for i in 0..16 { block[i] ^= prev[i]; }
        let enc = encrypt_block(&block, &w);
        out.extend_from_slice(&enc);
        prev = enc;
    }
    out
}

pub fn aes256_cbc_decrypt(ciphertext: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err("aes_decrypt: ciphertext harus kelipatan 16 byte dan tidak kosong".to_string());
    }
    let w = key_expansion_256(key);
    let mut out = Vec::with_capacity(ciphertext.len());
    let mut prev = *iv;
    for chunk in ciphertext.chunks(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let dec = decrypt_block(&block, &w);
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

pub fn f_aes_gen_key(_args: &[Value]) -> Result<Value, String> { Ok(Value::Str(hex_encode(&secure_random_bytes(32)))) }
pub fn f_aes_gen_iv(_args: &[Value]) -> Result<Value, String> { Ok(Value::Str(hex_encode(&secure_random_bytes(16)))) }

pub fn f_aes_encrypt(args: &[Value]) -> Result<Value, String> {
    let plaintext = isi_byte(args.get(0).unwrap_or(&Value::Nil), "crypto::aes_encrypt")?;
    let key_hex = as_str(args.get(1).unwrap_or(&Value::Nil), "crypto::aes_encrypt")?;
    let iv_hex = as_str(args.get(2).unwrap_or(&Value::Nil), "crypto::aes_encrypt")?;
    let key = match parse_key32(key_hex, "crypto::aes_encrypt") { Ok(k) => k, Err(e) => return Ok(err_value(e)) };
    let iv = match parse_iv16(iv_hex, "crypto::aes_encrypt") { Ok(v) => v, Err(e) => return Ok(err_value(e)) };
    let ct = aes256_cbc_encrypt(&plaintext, &key, &iv);
    Ok(ok_value(Value::Str(base64_encode_bytes(&ct))))
}

pub fn f_aes_decrypt(args: &[Value]) -> Result<Value, String> {
    Ok(match aes_buka(args, "crypto::aes_decrypt")? {
        Ok(bytes) => teks_atau_err(bytes, "crypto::aes_decrypt",
            "kunci atau IV-nya mungkin bukan yang dipakai menyandikan; isi biner diambil `crypto::aes_decrypt_bytes`"),
        Err(e) => err_value(e),
    })
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
