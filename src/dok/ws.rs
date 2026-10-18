//! `dok::ws_*` — WebSocket (RFC 6455) di atas server HTTP yang sudah ada.

use crate::interpreter::{err_value, ok_value, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};

use super::crypto::{base64_encode_bytes, sha1};

/// GUID dari RFC 6455 §1.3. Bukan rahasia — ia ada supaya sebuah server yang
/// tidak paham WebSocket tidak bisa kebetulan menghasilkan jawaban yang sah.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Batas ukuran satu pesan (termasuk hasil penggabungan fragmen).
const MAKS_PESAN: usize = 16 * 1024 * 1024;

// ─────────────────────────────────────────────────────────────────────────
// Registri koneksi
// ─────────────────────────────────────────────────────────────────────────

/// Sisi baca dan sisi tulis dikunci TERPISAH, dan registrinya cuma dikunci untuk mengambil
/// pegangan: dulu `ws_recv` yang menunggu memegang satu kunci untuk semua koneksi, jadi
/// `ws_send` dari goroutine lain (dan koneksi lain) tertahan sampai klien mengirim sesuatu.
struct Koneksi {
    /// Aliran untuk membaca, dan sisa byte yang sudah terbaca tapi belum jadi frame utuh.
    baca: Mutex<(TcpStream, Vec<u8>)>,
    tulis: Mutex<TcpStream>,
    tutup: std::sync::atomic::AtomicBool,
}

impl Koneksi {
    fn tertutup(&self) -> bool { self.tutup.load(std::sync::atomic::Ordering::SeqCst) }
    fn tandai_tutup(&self) { self.tutup.store(true, std::sync::atomic::Ordering::SeqCst); }
    fn kirim(&self, f: &[u8]) -> std::io::Result<()> {
        let mut t = self.tulis.lock().unwrap_or_else(|p| p.into_inner());
        t.write_all(f).and_then(|_| t.flush())
    }
}

static KONEKSI: OnceLock<Mutex<HashMap<u64, std::sync::Arc<Koneksi>>>> = OnceLock::new();
fn koneksi() -> &'static Mutex<HashMap<u64, std::sync::Arc<Koneksi>>> {
    KONEKSI.get_or_init(|| Mutex::new(HashMap::new()))
}
fn ambil(id: u64) -> Option<std::sync::Arc<Koneksi>> {
    koneksi().lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned()
}
static URUT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn handle(id: u64) -> Value {
    let mut f = HashMap::new();
    f.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: "DokWs".to_string(), fields: f.into() }
}

fn id_dari(v: Option<&Value>, fname: &str) -> Result<u64, String> {
    match v {
        Some(Value::Struct { name, fields }) if name == "DokWs" => match fields.get("id") {
            Some(Value::Int(n)) if u64::try_from(*n).is_ok() => Ok(*n as u64),
            _ => Err(format!("{}: handle WebSocket rusak", fname)),
        },
        Some(lain) => Err(format!(
            "{}: butuh koneksi WebSocket (argumen handler `dok::router_ws`), dapat `{}`",
            fname, lain.type_name())),
        None => Err(format!("{}: butuh koneksi WebSocket-nya", fname)),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Jabat tangan
// ─────────────────────────────────────────────────────────────────────────

/// Kunci jawaban RFC 6455: base64(sha1(kunci_klien + GUID)).
pub fn kunci_jawaban(kunci_klien: &str) -> String {
    let mut buf = kunci_klien.trim().to_string();
    buf.push_str(GUID);
    base64_encode_bytes(&sha1(buf.as_bytes()))
}

/// Apakah header-header ini meminta upgrade ke WebSocket?
pub fn minta_upgrade(headers: &[(String, String)]) -> Option<String> {
    let cari = |n: &str| headers.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(n))
        .map(|(_, v)| v.as_str());
    let up = cari("upgrade")?;
    if !up.eq_ignore_ascii_case("websocket") { return None; }
    let conn = cari("connection")?;
    if !conn.to_ascii_lowercase().contains("upgrade") { return None; }
    // Versi 13 saja. Versi lain punya jabat tangan yang berbeda, dan menjawab
    // seolah paham berarti kedua sisi bicara protokol yang berbeda.
    match cari("sec-websocket-version") {
        Some(v) if v.trim() == "13" => {}
        _ => return None,
    }
    cari("sec-websocket-key").map(|k| k.to_string())
}

/// Jawab jabat tangannya di atas `stream`, lalu daftarkan koneksinya.
pub fn terima(mut stream: TcpStream, kunci: &str) -> Result<u64, String> {
    let jawab = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        kunci_jawaban(kunci));
    stream.write_all(jawab.as_bytes()).map_err(|e| format!("ws: gagal jabat tangan: {}", crate::host::galat_io(&e)))?;
    stream.flush().map_err(|e| format!("ws: gagal jabat tangan: {}", crate::host::galat_io(&e)))?;
    let id = URUT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let tulis = stream.try_clone().map_err(|e| format!("ws: gagal menyiapkan koneksi: {}", crate::host::galat_io(&e)))?;
    koneksi().lock().unwrap_or_else(|p| p.into_inner()).insert(id, std::sync::Arc::new(Koneksi {
        baca: Mutex::new((stream, Vec::new())), tulis: Mutex::new(tulis), tutup: Default::default() }));
    Ok(id)
}

/// Buang koneksi dari registri (dipanggil sesudah handler selesai).
pub fn lepas(id: u64) {
    koneksi().lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
}

pub fn nilai_handle(id: u64) -> Value { handle(id) }

// ─────────────────────────────────────────────────────────────────────────
// Frame
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Jenis { Lanjutan, Teks, Biner, Tutup, Ping, Pong }

fn jenis_dari(op: u8) -> Option<Jenis> {
    Some(match op {
        0x0 => Jenis::Lanjutan, 0x1 => Jenis::Teks, 0x2 => Jenis::Biner,
        0x8 => Jenis::Tutup, 0x9 => Jenis::Ping, 0xA => Jenis::Pong,
        _ => return None,
    })
}

struct Frame { fin: bool, jenis: Jenis, muatan: Vec<u8> }

/// Coba baca satu frame dari `buf`. `Ok(None)` berarti byte-nya belum cukup —
/// bukan galat, dan membedakan keduanya adalah seluruh gunanya fungsi ini.
fn baca_frame(buf: &[u8]) -> Result<Option<(Frame, usize)>, String> {
    if buf.len() < 2 { return Ok(None); }
    let fin = buf[0] & 0x80 != 0;
    if buf[0] & 0x70 != 0 {
        // RSV1..3 dipakai ekstensi.
        return Err("ws: frame memakai bit RSV, padahal tidak ada ekstensi yang disepakati".into());
    }
    let Some(jenis) = jenis_dari(buf[0] & 0x0F) else {
        return Err(format!("ws: opcode {} tidak dikenal", buf[0] & 0x0F));
    };
    let masked = buf[1] & 0x80 != 0;
    let len7 = (buf[1] & 0x7F) as usize;
    let mut i = 2usize;
    let panjang = match len7 {
        126 => {
            if buf.len() < i + 2 { return Ok(None); }
            let n = u16::from_be_bytes([buf[i], buf[i + 1]]) as usize; i += 2; n
        }
        127 => {
            if buf.len() < i + 8 { return Ok(None); }
            let mut a = [0u8; 8]; a.copy_from_slice(&buf[i..i + 8]); i += 8;
            let n = u64::from_be_bytes(a);
            // Panjang ini datang dari JARINGAN. Tanpa batas, sebuah header
            // 8 byte bisa meminta alokasi 8 exabyte dari siapa pun yang bisa
            // membuka soket.
            if n > MAKS_PESAN as u64 {
                return Err(format!(
                    "ws: frame {} byte melebihi batas {} byte", n, MAKS_PESAN));
            }
            n as usize
        }
        n => n,
    };
    if panjang > MAKS_PESAN {
        return Err(format!("ws: frame {} byte melebihi batas {} byte", panjang, MAKS_PESAN));
    }
    // Frame kontrol wajib pendek dan tidak boleh terfragmentasi (RFC 6455 §5.5).
    if matches!(jenis, Jenis::Tutup | Jenis::Ping | Jenis::Pong) {
        if panjang > 125 { return Err("ws: frame kontrol lebih dari 125 byte".into()); }
        if !fin { return Err("ws: frame kontrol tidak boleh terfragmentasi".into()); }
    }
    let mask = if masked {
        if buf.len() < i + 4 { return Ok(None); }
        let m = [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]; i += 4; Some(m)
    } else { None };
    if buf.len() < i + panjang { return Ok(None); }
    let mut muatan = buf[i..i + panjang].to_vec();
    if let Some(m) = mask {
        for (k, b) in muatan.iter_mut().enumerate() { *b ^= m[k % 4]; }
    }
    Ok(Some((Frame { fin, jenis, muatan }, i + panjang)))
}

/// Rakit satu frame untuk dikirim. Server TIDAK memasang mask (RFC 6455 §5.1).
fn tulis_frame(jenis: Jenis, muatan: &[u8]) -> Vec<u8> {
    let op = match jenis {
        Jenis::Lanjutan => 0x0, Jenis::Teks => 0x1, Jenis::Biner => 0x2,
        Jenis::Tutup => 0x8, Jenis::Ping => 0x9, Jenis::Pong => 0xA,
    };
    let mut out = vec![0x80 | op];
    let n = muatan.len();
    if n < 126 { out.push(n as u8); }
    else if n <= u16::MAX as usize { out.push(126); out.extend_from_slice(&(n as u16).to_be_bytes()); }
    else { out.push(127); out.extend_from_slice(&(n as u64).to_be_bytes()); }
    out.extend_from_slice(muatan);
    out
}

// ─────────────────────────────────────────────────────────────────────────
// API
// ─────────────────────────────────────────────────────────────────────────

/// `dok::ws_recv(c)` → `Option`: teks untuk pesan teks, array byte untuk pesan biner.
pub fn ws_recv(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::ws_recv")?;
    let Some(k) = ambil(id) else {
        return Err("dok::ws_recv: koneksi sudah tidak ada".to_string());
    };
    if k.tertutup() { return Ok(none()); }
    let mut baca = k.baca.lock().unwrap_or_else(|p| p.into_inner());
    let (aliran, sisa) = &mut *baca;

    let mut gabung: Vec<u8> = Vec::new();
    let mut jenis_awal: Option<Jenis> = None;
    let mut buf = [0u8; 8192];
    loop {
        match baca_frame(sisa) {
            Err(e) => { k.tandai_tutup(); return Err(e); }
            Ok(Some((f, n))) => {
                sisa.drain(..n);
                match f.jenis {
                    Jenis::Ping => {
                        let _ = k.kirim(&tulis_frame(Jenis::Pong, &f.muatan));
                        continue;
                    }
                    Jenis::Pong => continue,
                    Jenis::Tutup => {
                        let _ = k.kirim(&tulis_frame(Jenis::Tutup, &[]));
                        k.tandai_tutup();
                        return Ok(none());
                    }
                    j => {
                        if jenis_awal.is_none() { jenis_awal = Some(j); }
                        if gabung.len() + f.muatan.len() > MAKS_PESAN {
                            k.tandai_tutup();
                            return Err(format!(
                                "ws: pesan gabungan melebihi batas {} byte", MAKS_PESAN));
                        }
                        gabung.extend_from_slice(&f.muatan);
                        if f.fin {
                            if matches!(jenis_awal, Some(Jenis::Biner)) {
                                return Ok(some(super::crypto::byte_ke_array(gabung)));
                            }
                            // RFC 6455 §8.1: pesan teks yang bukan UTF-8 menutup koneksi (1007).
                            let Ok(teks) = String::from_utf8(gabung) else {
                                let _ = k.kirim(&tulis_frame(Jenis::Tutup, &[0x03, 0xEF]));
                                k.tandai_tutup();
                                return Err("ws: pesan teks dari klien bukan UTF-8 yang sah; koneksinya ditutup (kode 1007)".to_string());
                            };
                            return Ok(some(Value::Str(teks)));
                        }
                        continue;
                    }
                }
            }
            Ok(None) => {}
        }
        match aliran.read(&mut buf) {
            Ok(0) => { k.tandai_tutup(); return Ok(none()); }
            Ok(n) => sisa.extend_from_slice(&buf[..n]),
            Err(e) => { k.tandai_tutup(); return Err(format!("ws: gagal baca: {}", crate::host::galat_io(&e))); }
        }
    }
}

/// `dok::ws_send(c, isi)` → `Result<(), string>`; array byte jadi pesan biner.
pub fn ws_send(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::ws_send")?;
    // Array byte dikirim sebagai pesan BINER; selain itu teks (nilai lain ditampilkan).
    let (jenis, teks) = match args.get(1) {
        Some(Value::Str(s)) => (Jenis::Teks, s.clone().into_bytes()),
        Some(Value::Array(a)) => (Jenis::Biner, super::crypto::array_ke_byte(a, "dok::ws_send")?),
        Some(lain) => (Jenis::Teks, format!("{}", lain).into_bytes()),
        None => return Err("dok::ws_send: butuh pesannya".to_string()),
    };
    if teks.len() > MAKS_PESAN {
        return Ok(err_value(format!(
            "dok::ws_send: pesan {} byte melebihi batas {} byte", teks.len(), MAKS_PESAN)));
    }
    let Some(k) = ambil(id) else {
        return Ok(err_value("koneksi sudah tidak ada"));
    };
    if k.tertutup() { return Ok(err_value("koneksi sudah ditutup")); }
    match k.kirim(&tulis_frame(jenis, &teks)) {
        Ok(()) => Ok(ok_value(Value::Unit)),
        Err(e) => { k.tandai_tutup(); Ok(err_value(format!("gagal kirim: {}", crate::host::galat_io(&e)))) }
    }
}

/// `dok::ws_close(c)` — kirim frame close lalu tandai tertutup.
pub fn ws_close(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::ws_close")?;
    if let Some(k) = ambil(id) {
        if !k.tertutup() {
            let _ = k.kirim(&tulis_frame(Jenis::Tutup, &[]));
            k.tandai_tutup();
        }
    }
    Ok(Value::Unit)
}

/// `dok::ws_terbuka(c)` → bool.
pub fn ws_terbuka(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::ws_terbuka")?;
    Ok(Value::Bool(ambil(id).map(|k| !k.tertutup()).unwrap_or(false)))
}

fn some(v: Value) -> Value {
    Value::Enum { type_name: "Option".into(), variant: "Some".into(), payload: Some(Box::new(v)) }
}
fn none() -> Value {
    Value::Enum { type_name: "Option".into(), variant: "None".into(), payload: None }
}

#[cfg(test)]
mod tes {
    use super::*;

    /// Vektor uji dari RFC 6455 §1.3 — kalau ini meleset, tidak ada klien yang
    /// akan mau tersambung, dan gejalanya cuma "koneksi ditolak".
    #[test]
    fn kunci_jawaban_sesuai_rfc6455() {
        assert_eq!(kunci_jawaban("dGhlIHNhbXBsZSBub25jZQ=="),
                   "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn frame_bolak_balik() {
        // Frame dari server tidak bermask; frame dari klien bermask.
        let f = tulis_frame(Jenis::Teks, b"halo");
        assert_eq!(f[0], 0x81);
        assert_eq!(f[1], 4);
        assert_eq!(&f[2..], b"halo");

        // Bikin frame klien (bermask) dan baca balik.
        let mut klien = vec![0x81, 0x80 | 4, 1, 2, 3, 4];
        for (i, b) in b"halo".iter().enumerate() { klien.push(b ^ [1, 2, 3, 4][i % 4]); }
        let (fr, n) = baca_frame(&klien).unwrap().unwrap();
        assert_eq!(n, klien.len());
        assert_eq!(fr.muatan, b"halo");
        assert!(fr.fin);
    }

    #[test]
    fn byte_belum_lengkap_bukan_galat() {
        // Ini yang membedakan "tunggu lagi" dari "rusak". Kalau keduanya
        // disamakan, sebuah pesan yang kebetulan terpotong di tengah TCP
        // segment akan memutus koneksi yang sehat.
        assert!(baca_frame(&[0x81]).unwrap().is_none());
        assert!(baca_frame(&[0x81, 0x84, 1, 2]).unwrap().is_none());
        assert!(baca_frame(&[]).unwrap().is_none());
    }

    #[test]
    fn frame_kegedean_ditolak_bukan_dialokasikan() {
        // Panjangnya datang dari jaringan: 8 byte header bisa meminta 8
        // exabyte. Yang diuji di sini adalah ia DITOLAK sebelum satu byte pun
        // dialokasikan.
        let mut h = vec![0x81, 127];
        h.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(baca_frame(&h).is_err());
    }

    #[test]
    fn bit_rsv_dan_opcode_asing_ditolak() {
        assert!(baca_frame(&[0xC1, 0x00]).is_err());   // RSV1 menyala
        assert!(baca_frame(&[0x83, 0x00]).is_err());   // opcode 3 tidak ada
    }

    #[test]
    fn frame_kontrol_wajib_pendek_dan_utuh() {
        assert!(baca_frame(&[0x89, 126, 0, 200]).is_err());  // ping > 125
        assert!(baca_frame(&[0x09, 0x00]).is_err());         // ping terfragmentasi
    }
}
