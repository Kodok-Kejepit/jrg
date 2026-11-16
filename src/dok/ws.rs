//! `dok::ws_*` — WebSocket (RFC 6455) di atas server HTTP yang sudah ada.

use crate::interpreter::{err_value, ok_value, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::crypto::{base64_encode_bytes, sha1};

/// GUID dari RFC 6455 §1.3. Bukan rahasia — ia ada supaya sebuah server yang
/// tidak paham WebSocket tidak bisa kebetulan menghasilkan jawaban yang sah.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Batas ukuran satu pesan (termasuk hasil penggabungan fragmen).
const MAKS_PESAN: usize = 16 * 1024 * 1024;
/// Klien yang diam selama `DIAM` ditanya `ping`; yang tidak menjawab dalam `JAWAB_PING` dilepas. Satu pesan
/// harus utuh dalam `TENGGAT_PESAN` ditambah 1 detik per `LAJU_MIN` byte — dulu sambungan mati atau yang
/// menetes memegang utas dan slot `MAKS_SAMBUNGAN` selamanya.
const DIAM: Duration = Duration::from_secs(60);
const JAWAB_PING: Duration = Duration::from_secs(30);
const TENGGAT_PESAN: Duration = Duration::from_secs(30);
const LAJU_MIN: u64 = 16 * 1024;

fn tenggat_pesan(diterima: usize) -> Duration { TENGGAT_PESAN + Duration::from_secs(diterima as u64 / LAJU_MIN) }

// ─────────────────────────────────────────────────────────────────────────
// Registri koneksi
// ─────────────────────────────────────────────────────────────────────────

/// Sisi baca dan sisi tulis dikunci TERPISAH, dan registrinya cuma dikunci untuk mengambil
/// pegangan: dulu `ws_recv` yang menunggu memegang satu kunci untuk semua koneksi, jadi
/// `ws_send` dari goroutine lain (dan koneksi lain) tertahan sampai klien mengirim sesuatu.
struct Koneksi {
    baca: Mutex<Baca>,
    tulis: Mutex<TcpStream>,
    tutup: std::sync::atomic::AtomicBool,
}

/// Aliran untuk membaca, sisa byte yang belum jadi pesan, dan bagiannya di anggaran byte klien yang sama dengan
/// badan HTTP (`MAKS_BADAN_SERENTAK`): dulu 512 sambungan masing-masing memegang pesan setengah jadi 16 MiB (8 GiB).
struct Baca { aliran: TcpStream, sisa: Vec<u8>, pesanan: super::http::Pesanan }

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

/// CSWSH (CWE-1385): peramban selalu mengirim `Origin`, dan host-nya harus host `Host` — port boleh beda,
/// seperti cookie. Klien bukan peramban (tanpa `Origin`) diterima; `Origin: null` dan domain lain tidak.
pub fn origin_sah(headers: &[(String, String)]) -> bool {
    let cari = |n: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.trim());
    let Some(o) = cari("origin") else { return true };
    let host = |v: &str| crate::stdlib::url::urai_kepala_host(v).ok().map(|(h, _)| h.to_ascii_lowercase());
    match (o.split_once("://").and_then(|(_, sisa)| host(sisa)), cari("host").and_then(host)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Jawab jabat tangannya di atas `stream`, lalu daftarkan koneksinya.
pub fn terima(mut stream: TcpStream, kunci: &str) -> Result<u64, String> {
    // Klien yang berhenti membaca tidak boleh menahan `ws_send` selamanya: tiap tulisan bertenggat 30 detik.
    stream.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();
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
        baca: Mutex::new(Baca { aliran: stream, sisa: Vec::new(),
            pesanan: super::http::Pesanan::baru(&super::http::BADAN_TERPAKAI) }), tulis: Mutex::new(tulis), tutup: Default::default() }));
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

#[derive(Debug)]
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
    if !masked {
        return Err("ws: frame dari klien wajib bermask (RFC 6455 §5.1)".into());
    }
    if buf.len() < i + 4 { return Ok(None); }
    let m = [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]; i += 4;
    if buf.len() < i + panjang { return Ok(None); }
    let mut muatan = buf[i..i + panjang].to_vec();
    for (k, b) in muatan.iter_mut().enumerate() { *b ^= m[k % 4]; }
    Ok(Some((Frame { fin, jenis, muatan }, i + panjang)))
}

/// Rakit fragmen data sesuai urutan RFC 6455 §5.4; mengembalikan kode tutup protokol bila melanggar.
fn gabung_fragmen(
    jenis_awal: &mut Option<Jenis>, gabung: &mut Vec<u8>, f: Frame,
) -> Result<Option<(Jenis, Vec<u8>)>, (u16, String)> {
    match (*jenis_awal, f.jenis) {
        (None, Jenis::Lanjutan) =>
            return Err((1002, "ws: frame lanjutan tanpa frame pembuka (kode 1002)".into())),
        (Some(_), Jenis::Teks | Jenis::Biner) =>
            return Err((1002, "ws: frame data baru di tengah pesan terfragmentasi (kode 1002)".into())),
        (None, j) => *jenis_awal = Some(j),
        (Some(_), Jenis::Lanjutan) => {}
        _ => {}
    }
    if gabung.len() + f.muatan.len() > MAKS_PESAN {
        return Err((1009, format!("ws: pesan gabungan melebihi batas {} byte", MAKS_PESAN)));
    }
    gabung.extend_from_slice(&f.muatan);
    if !f.fin { return Ok(None); }
    let jenis = jenis_awal.take().unwrap_or(Jenis::Teks);
    Ok(Some((jenis, std::mem::take(gabung))))
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
    let Baca { aliran, sisa, pesanan } = &mut *baca;
    let hasil = terima_pesan(&k, aliran, sisa, pesanan);
    pesanan.sisakan(sisa.len());
    hasil
}

fn tutup_dengan(k: &Koneksi, kode: u16) {
    let _ = k.kirim(&tulis_frame(Jenis::Tutup, &kode.to_be_bytes()));
    k.tandai_tutup();
}

fn terima_pesan(k: &Koneksi, aliran: &mut TcpStream, sisa: &mut Vec<u8>, pesanan: &mut super::http::Pesanan)
    -> Result<Value, String> {
    let mut gabung: Vec<u8> = Vec::new();
    let mut jenis_awal: Option<Jenis> = None;
    let mut buf = [0u8; 8192];
    // Tenggat pesan berjalan sejak byte pertamanya ada; sebelum itu klien boleh diam, tapi ditanya `ping`.
    let mut mulai: Option<Instant> = (!sisa.is_empty()).then(Instant::now);
    let mut diterima = sisa.len();
    let mut ping_sejak: Option<Instant> = None;
    loop {
        match baca_frame(sisa) {
            Err(e) => { k.tandai_tutup(); return Err(e); }
            Ok(Some((f, n))) => {
                sisa.drain(..n);
                match f.jenis {
                    Jenis::Ping | Jenis::Pong => {
                        if f.jenis == Jenis::Ping { let _ = k.kirim(&tulis_frame(Jenis::Pong, &f.muatan)); }
                        if sisa.is_empty() && gabung.is_empty() { mulai = None; diterima = 0; }
                        continue;
                    }
                    Jenis::Tutup => {
                        let _ = k.kirim(&tulis_frame(Jenis::Tutup, &[]));
                        k.tandai_tutup();
                        return Ok(none());
                    }
                    _ => {
                        match gabung_fragmen(&mut jenis_awal, &mut gabung, f) {
                            Err((kode, pesan)) => { tutup_dengan(k, kode); return Err(pesan); }
                            Ok(None) => continue,
                            Ok(Some((Jenis::Biner, isi))) => {
                                return Ok(some(super::crypto::byte_ke_array(isi)));
                            }
                            Ok(Some((_, isi))) => {
                                // RFC 6455 §8.1: pesan teks yang bukan UTF-8 menutup koneksi (1007).
                                let Ok(teks) = String::from_utf8(isi) else {
                                    tutup_dengan(k, 1007);
                                    return Err("ws: pesan teks dari klien bukan UTF-8 yang sah; koneksinya ditutup (kode 1007)".to_string());
                                };
                                return Ok(some(Value::Str(std::sync::Arc::new(teks))));
                            }
                        }
                    }
                }
            }
            Ok(None) => {}
        }
        let tunggu = match (mulai, ping_sejak) {
            (Some(m), _) => match tenggat_pesan(diterima).checked_sub(m.elapsed()) {
                Some(t) if !t.is_zero() => t,
                _ => {
                    tutup_dengan(k, 1008);
                    return Err(format!("ws: pesan dari klien tidak utuh dalam {} detik; koneksinya ditutup (kode 1008)",
                        tenggat_pesan(diterima).as_secs()));
                }
            },
            (None, None) => DIAM,
            (None, Some(p)) => match JAWAB_PING.checked_sub(p.elapsed()) {
                Some(t) if !t.is_zero() => t,
                // Klien yang tidak menjawab `ping` dianggap pergi, sama dengan sambungan yang ditutup.
                _ => { tutup_dengan(k, 1001); return Ok(none()); }
            },
        };
        aliran.set_read_timeout(Some(tunggu)).ok();
        match aliran.read(&mut buf) {
            Ok(0) => { k.tandai_tutup(); return Ok(none()); }
            Ok(n) => {
                if !pesanan.tambah(n, super::http::MAKS_BADAN_SERENTAK) {
                    tutup_dengan(k, 1013);
                    return Err(format!("ws: server sedang memegang {} MiB data dari klien; koneksinya ditutup (kode 1013)",
                        super::http::MAKS_BADAN_SERENTAK >> 20));
                }
                sisa.extend_from_slice(&buf[..n]);
                diterima += n;
                ping_sejak = None;
                if mulai.is_none() { mulai = Some(Instant::now()); }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                if mulai.is_none() && ping_sejak.is_none() {
                    let _ = k.kirim(&tulis_frame(Jenis::Ping, &[]));
                    ping_sejak = Some(Instant::now());
                }
            }
            Err(e) => { k.tandai_tutup(); return Err(format!("ws: gagal baca: {}", crate::host::galat_io(&e))); }
        }
    }
}

/// `dok::ws_send(c, isi)` → `Result<(), string>`; array byte jadi pesan biner.
pub fn ws_send(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::ws_send")?;
    // Array byte dikirim sebagai pesan BINER; selain itu teks (nilai lain ditampilkan).
    let (jenis, teks) = match args.get(1) {
        Some(Value::Str(s)) => (Jenis::Teks, s.as_bytes().to_vec()),
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

    #[test]
    fn frame_tanpa_mask_dan_fragmen_kacau_ditolak() {
        assert!(baca_frame(&[0x81, 0x04, b'h', b'a', b'l', b'o']).unwrap_err().contains("bermask"));
        let (mut awal, mut gabung) = (None, Vec::new());
        let lanjutan_yatim = Frame { fin: true, jenis: Jenis::Lanjutan, muatan: b"x".to_vec() };
        assert_eq!(gabung_fragmen(&mut awal, &mut gabung, lanjutan_yatim).unwrap_err().0, 1002);
        let buka = Frame { fin: false, jenis: Jenis::Teks, muatan: b"ha".to_vec() };
        assert!(gabung_fragmen(&mut awal, &mut gabung, buka).unwrap().is_none());
        let tabrak = Frame { fin: true, jenis: Jenis::Biner, muatan: b"lo".to_vec() };
        assert_eq!(gabung_fragmen(&mut awal, &mut gabung, tabrak).unwrap_err().0, 1002);
    }
}

#[cfg(test)]
mod uji_origin {
    use super::origin_sah;
    fn k(o: Option<&str>, h: &str) -> Vec<(String, String)> {
        let mut v = vec![("Host".to_string(), h.to_string())];
        if let Some(o) = o { v.push(("Origin".to_string(), o.to_string())); }
        v
    }
    #[test]
    fn origin_websocket_harus_host_yang_sama() {
        assert!(origin_sah(&k(None, "127.0.0.1:8080")));
        assert!(origin_sah(&k(Some("http://localhost:5173"), "localhost:8080")));
        assert!(origin_sah(&k(Some("https://JRG-lang.org"), "jrg-lang.org")));
        assert!(origin_sah(&k(Some("http://[::1]:3000"), "[::1]:8080")));
        for o in ["https://jahat.example", "null", "http://localhost.jahat.example", "localhost", "http://a@localhost"] {
            assert!(!origin_sah(&k(Some(o), "localhost:8080")), "{}", o);
        }
    }
}
