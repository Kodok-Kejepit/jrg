// src/stdlib/gzip.rs — `gzip::`: format gzip (RFC 1952) di atas DEFLATE milik `src/kompres.rs`.
// Kelas celah pembongkar gzip bahasa lain ditutup sejak lahir: bom, kepala yang meluber, CRC/ISIZE bohong, ekor liar.

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// Hasil bongkaran paling besar, seluruh anggota dijumlah — di atasnya dianggap bom gzip.
pub const MAKS_HASIL: usize = 256 << 20;

const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;

pub fn register(vm: &mut Vm) {
    vm.def("gzip::compress", |a| crate::stdlib::sistem::lewat_hasil(a, g_compress));
    vm.def("gzip::decompress", |a| crate::stdlib::sistem::lewat_hasil(a, |a| g_decompress(a, false)));
    vm.def("gzip::decompress_text", |a| crate::stdlib::sistem::lewat_hasil(a, |a| g_decompress(a, true)));
}

/// Satu anggota gzip; MTIME nol dan tanpa nama berkas, jadi isi yang sama selalu jadi byte yang sama.
pub fn mampatkan(data: &[u8]) -> Vec<u8> {
    let mut o = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    o.extend_from_slice(&crate::kompres::deflate(data));
    o.extend_from_slice(&crate::kompres::crc32(data).to_le_bytes());
    o.extend_from_slice(&(data.len() as u32).to_le_bytes());
    o
}

fn u16_le(b: &[u8], i: usize) -> u16 { b[i] as u16 | (b[i + 1] as u16) << 8 }
fn u32_le(b: &[u8], i: usize) -> u32 { u16_le(b, i) as u32 | (u16_le(b, i + 2) as u32) << 16 }

/// Bongkar semua anggota berurutan (RFC 1952 §2.2); hasil di atas `batas` byte ditolak.
pub fn bongkar(data: &[u8], batas: usize) -> Result<Vec<u8>, String> {
    if data.is_empty() { return Err("data kosong, bukan gzip".into()); }
    let mut hasil = Vec::new();
    let mut pos = 0;
    let mut ke = 1;
    while pos < data.len() {
        let b = &data[pos..];
        if b.len() < 2 || b[0] != 0x1f || b[1] != 0x8b {
            return Err(if ke == 1 { "bukan gzip: dua byte pertamanya bukan 1f 8b".into() } else {
                format!("ada {} byte sesudah anggota gzip ke-{} yang bukan anggota gzip — ditolak, bukan dibuang \
                         diam-diam", b.len(), ke - 1)
            });
        }
        if b.len() < 18 { return Err(format!("anggota gzip ke-{} terpotong", ke)); }
        if b[2] != 8 { return Err(format!("metode pemampatan {} tidak dikenal, gzip cuma punya DEFLATE (8)", b[2])); }
        let flg = b[3];
        if flg & 0xe0 != 0 {
            return Err(format!("bit cadangan kepala gzip terpasang (FLG {:#04x}) — RFC 1952 mewajibkan ditolak", flg));
        }
        let terpotong = || format!("kepala anggota gzip ke-{} terpotong", ke);
        let mut i = 10;
        if flg & FEXTRA != 0 {
            if i + 2 > b.len() { return Err(terpotong()); }
            let xlen = u16_le(b, i) as usize;
            i += 2;
            if i + xlen > b.len() { return Err(terpotong()); }
            i += xlen;
        }
        for (bit, apa) in [(FNAME, "nama berkas"), (FCOMMENT, "komentar")] {
            if flg & bit == 0 { continue; }
            match b[i.min(b.len())..].iter().position(|&c| c == 0) {
                Some(n) => i += n + 1,
                None => return Err(format!("{} di kepala anggota gzip ke-{} tidak diakhiri NUL", apa, ke)),
            }
        }
        if flg & FHCRC != 0 {
            if i + 2 > b.len() { return Err(terpotong()); }
            if crate::kompres::crc32(&b[..i]) as u16 != u16_le(b, i) {
                return Err(format!("CRC kepala anggota gzip ke-{} tidak cocok", ke));
            }
            i += 2;
        }
        let (isi, dipakai) = crate::kompres::inflate_sisa(&b[i..], batas - hasil.len())
            .map_err(|e| format!("anggota gzip ke-{}: {}", ke, e))?;
        i += dipakai;
        if i + 8 > b.len() { return Err(format!("ekor anggota gzip ke-{} (CRC32 dan ukuran) terpotong", ke)); }
        if crate::kompres::crc32(&isi) != u32_le(b, i) {
            return Err(format!("CRC32 anggota gzip ke-{} tidak cocok — isinya rusak atau diubah", ke));
        }
        if isi.len() as u32 != u32_le(b, i + 4) {
            return Err(format!("ukuran anggota gzip ke-{} tercatat {} byte, isinya {}", ke, u32_le(b, i + 4), isi.len()));
        }
        hasil.extend_from_slice(&isi);
        pos += i + 8;
        ke += 1;
    }
    Ok(hasil)
}

fn byte_arg(a: &[Value], nama: &str, teks_boleh: bool) -> Result<Vec<u8>, String> {
    match a.first() {
        Some(Value::Str(s)) if teks_boleh => Ok(s.as_bytes().to_vec()),
        Some(Value::Array(x)) => crate::dok::crypto::array_ke_byte(x, nama),
        Some(lain) => Err(format!("{}: argumen ke-1 harus {}array byte, dapat `{}`", nama,
                                  if teks_boleh { "string atau " } else { "" }, lain.type_name())),
        None => Err(format!("{}: argumen ke-1 wajib diisi", nama)),
    }
}

/// `gzip::compress(isi)` → `array` byte; `isi` teks (UTF-8) atau array byte.
fn g_compress(a: &[Value]) -> Result<Value, String> {
    Ok(crate::dok::crypto::byte_ke_array(mampatkan(&byte_arg(a, "gzip::compress", true)?)))
}

/// `gzip::decompress(byte)` → `Result<array>`; `gzip::decompress_text` → `Result<string>`.
fn g_decompress(a: &[Value], teks: bool) -> Result<Value, String> {
    let nama = if teks { "gzip::decompress_text" } else { "gzip::decompress" };
    let data = byte_arg(a, nama, false)?;
    Ok(match bongkar(&data, MAKS_HASIL) {
        Err(e) if e.contains("melewati batas") =>
            err_value(format!("{}: hasil bongkaran melewati {} MiB — dianggap bom gzip", nama, MAKS_HASIL >> 20)),
        Err(e) => err_value(format!("{}: {}", nama, e)),
        Ok(isi) if teks => match String::from_utf8(isi) {
            Ok(s) => ok_value(Value::teks(s)),
            Err(_) => err_value(format!("{}: isinya bukan teks UTF-8 — baca byte-nya dengan `gzip::decompress`", nama)),
        },
        Ok(isi) => ok_value(crate::dok::crypto::byte_ke_array(isi)),
    })
}

#[cfg(test)]
mod tes {
    use super::*;

    /// `gzip -c halo.txt`: FNAME terpasang, MTIME bukan nol — keluaran program lain, bukan milik kita.
    const HALO: [u8; 39] = [
        0x1f, 0x8b, 0x08, 0x08, 0x85, 0x6f, 0xe5, 0x6a, 0x00, 0x03, 0x68, 0x61, 0x6c, 0x6f, 0x2e, 0x74, 0x78, 0x74,
        0x00, 0xcb, 0x48, 0xcc, 0xc9, 0x57, 0x48, 0xaf, 0xca, 0x2c, 0xe0, 0x02, 0x00, 0x39, 0xbe, 0xc8, 0x30, 0x0a,
        0x00, 0x00, 0x00,
    ];

    #[test]
    fn bolak_balik_dan_keluaran_gzip_lain() {
        assert_eq!(bongkar(&HALO, MAKS_HASIL).unwrap(), b"halo gzip\n");
        for isi in [&b""[..], b"a", &[7u8; 100_000][..], "teks ✓ panjang".repeat(500).as_bytes()] {
            assert_eq!(bongkar(&mampatkan(isi), MAKS_HASIL).unwrap(), isi);
        }
        // Anggota bersambung (`cat a.gz b.gz`) adalah satu berkas gzip yang sah.
        let mut dua = mampatkan(b"satu ");
        dua.extend_from_slice(&HALO);
        assert_eq!(bongkar(&dua, MAKS_HASIL).unwrap(), b"satu halo gzip\n");
    }

    #[test]
    fn data_jahat_ditolak() {
        let galat = |b: &[u8]| bongkar(b, MAKS_HASIL).unwrap_err();
        assert!(galat(&[]).contains("kosong"));
        assert!(galat(b"PK\x03\x04").contains("bukan gzip"));
        // Bom: hasilnya dibatasi SAAT dibongkar, bukan sesudah.
        assert!(bongkar(&mampatkan(&[0u8; 5000]), 4096).unwrap_err().contains("melewati batas"));
        let mut b = HALO; b[3] |= 0x20;
        assert!(galat(&b).contains("cadangan"));
        let mut b = HALO; b[2] = 9;
        assert!(galat(&b).contains("metode"));
        // Nama berkas tanpa NUL sampai akhir data, dan FEXTRA yang mengaku lebih panjang dari datanya.
        assert!(galat(&HALO[..18]).contains("terpotong") || galat(&HALO[..18]).contains("NUL"));
        let mut b = HALO.to_vec(); b[3] = FEXTRA; b[10] = 0xff; b[11] = 0xff;
        assert!(galat(&b).contains("terpotong"));
        let mut b = HALO; b[31] ^= 1;
        assert!(galat(&b).contains("CRC32"));
        let mut b = HALO; b[35] = 11;
        assert!(galat(&b).contains("ukuran"));
        assert!(galat(&HALO[..33]).contains("terpotong"));
        let mut ekor = HALO.to_vec(); ekor.extend_from_slice(&[0, 0]);
        assert!(galat(&ekor).contains("sesudah anggota gzip ke-1"));
        // FHCRC: CRC16 kepala diperiksa.
        let mut h = vec![0x1f, 0x8b, 8, FHCRC, 0, 0, 0, 0, 0, 255];
        let c = crate::kompres::crc32(&h) as u16;
        h.extend_from_slice(&c.to_le_bytes());
        h.extend_from_slice(&mampatkan(b"x")[10..]);
        assert_eq!(bongkar(&h, MAKS_HASIL).unwrap(), b"x");
        h[10] ^= 1;
        assert!(galat(&h).contains("CRC kepala"));
    }
}
