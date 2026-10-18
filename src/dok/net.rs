// src/dok/net.rs — dok::net — raw TCP / UDP networking (bagian dari library `dok`)

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::interpreter::{err_value, ok_value, Value};

// ─── Registries ─────────────────────────────────────────────────────────

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 { NEXT_ID.fetch_add(1, Ordering::SeqCst) }

type StreamCell = Arc<Mutex<BufReader<TcpStream>>>;

static TCP_STREAMS: OnceLock<Mutex<HashMap<u64, StreamCell>>> = OnceLock::new();
fn tcp_streams() -> &'static Mutex<HashMap<u64, StreamCell>> {
    TCP_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}
/// Kunci registry HANYA untuk clone-out `Arc` lalu langsung dilepas —
/// lihat catatan locking di atas modul.
fn get_stream(id: u64) -> Option<StreamCell> {
    tcp_streams().lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned()
}

static TCP_LISTENERS: OnceLock<Mutex<HashMap<u64, TcpListener>>> = OnceLock::new();
fn tcp_listeners() -> &'static Mutex<HashMap<u64, TcpListener>> {
    TCP_LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

static UDP_SOCKETS: OnceLock<Mutex<HashMap<u64, Arc<UdpSocket>>>> = OnceLock::new();
fn udp_sockets() -> &'static Mutex<HashMap<u64, Arc<UdpSocket>>> {
    UDP_SOCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn get_udp(id: u64) -> Option<Arc<UdpSocket>> {
    udp_sockets().lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned()
}

// ─── Handle helpers ─────────────────────────────────────────────────────

fn make_handle(type_name: &str, id: u64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: type_name.to_string(), fields: fields.into() }
}

/// `id` sebuah handle jenis `jenis`. NAMA struct-nya diperiksa: dulu field `id`
/// saja yang dibaca, jadi `Pengguna { id: 1 }` diam-diam dipakai sebagai soket 1.
fn handle_id(v: &Value, fname: &str, jenis: &str) -> Result<u64, String> {
    crate::dok::http::id_handle(v, fname, jenis)
}

fn as_str<'a>(v: &'a Value, fname: &str, arg_pos: usize) -> Result<&'a str, String> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        other => Err(format!("{}: argumen {} harus string, dapat `{}`", fname, arg_pos, other.type_name())),
    }
}

fn as_port(v: &Value, fname: &str) -> Result<u16, String> {
    match v {
        Value::Int(n) if *n >= 0 && *n <= 65535 => Ok(*n as u16),
        Value::Int(n) => Err(format!("{}: port {} di luar jangkauan 0..65535", fname, n)),
        other => Err(format!("{}: port harus int, dapat `{}`", fname, other.type_name())),
    }
}

/// Penampung sekali baca: `maks` berarti PALING BANYAK, jadi permintaan di atas ini tetap sah —
/// yang tidak boleh adalah memesan memori sebesar angka yang ditulis (2^63 mematikan proses).
const MAKS_SEKALI_BACA: usize = 16 * 1024 * 1024;
const MAKS_DATAGRAM: usize = 65_535;

fn as_usize(v: &Value, fname: &str) -> Result<usize, String> {
    match v {
        Value::Int(n) if *n >= 0 => Ok(usize::try_from(*n).unwrap_or(usize::MAX)),
        Value::Int(n) => Err(format!("{}: butuh int non-negatif, dapat {}", fname, n)),
        other => Err(format!("{}: butuh int non-negatif, dapat `{}`", fname, other.type_name())),
    }
}

// ─── TCP: client ────────────────────────────────────────────────────────

pub fn tcp_connect(args: &[Value]) -> Result<Value, String> {
    let host = as_str(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_connect", 1)?;
    let port = as_port(args.get(1).unwrap_or(&Value::Nil), "dok::tcp_connect")?;
    // Batas waktu yang salah tipe dulu diam-diam berarti "tanpa batas waktu".
    let timeout_ms = match args.get(2) {
        None | Some(Value::Int(0)) => None,
        Some(Value::Int(n)) if *n > 0 => Some((*n).min(u64::MAX as i128) as u64),
        Some(lain) => return Err(format!(
            "dok::tcp_connect: argumen ke-3 (batas waktu, milidetik) harus bilangan bulat \
             non-negatif, dapat {}",
            match lain { Value::Int(n) => n.to_string(), l => format!("`{}`", l.type_name()) })),
    };

    let addr = format!("{}:{}", host, port);
    let connect_result = match timeout_ms {
        Some(ms) => {
            let mut addrs = match addr.to_socket_addrs() {
                Ok(a) => a,
                Err(e) => return Ok(err_value(format!("dok::tcp_connect '{}': {}", addr, crate::host::galat_io(&e)))),
            };
            match addrs.next() {
                Some(sockaddr) => TcpStream::connect_timeout(&sockaddr, Duration::from_millis(ms)),
                None => return Ok(err_value(format!("dok::tcp_connect '{}': tidak ada alamat", addr))),
            }
        }
        None => TcpStream::connect(&addr),
    };

    match connect_result {
        Ok(stream) => {
            let id = next_id();
            tcp_streams().lock().unwrap_or_else(|p| p.into_inner())
                .insert(id, Arc::new(Mutex::new(BufReader::new(stream))));
            Ok(ok_value(make_handle("TcpConn", id)))
        }
        Err(e) => Ok(err_value(format!("dok::tcp_connect '{}': {}", addr, crate::host::galat_io(&e)))),
    }
}

pub fn tcp_send(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_send", "TcpConn")?;
    let data = data_kirim(args.get(1).unwrap_or(&Value::Nil), "dok::tcp_send", 2)?;

    match get_stream(id) {
        Some(cell) => {
            let mut reader = cell.lock().unwrap_or_else(|p| p.into_inner());
            match reader.get_mut().write_all(&data) {
                Ok(()) => Ok(ok_value(Value::Int(data.len() as i128))),
                Err(e) => Ok(err_value(format!("dok::tcp_send: {}", crate::host::galat_io(&e)))),
            }
        }
        None => Ok(err_value("dok::tcp_send: koneksi tidak ditemukan (sudah ditutup?)".to_string())),
    }
}

/// Yang ditulis ke soket: teks, atau array byte (bilangan 0 sampai 255).
fn data_kirim(v: &Value, fname: &str, posisi: usize) -> Result<Vec<u8>, String> {
    let Value::Array(isi) = v else { return Ok(as_str(v, fname, posisi)?.as_bytes().to_vec()) };
    super::crypto::array_ke_byte(isi, fname)
}

/// Kalimat untuk byte yang datang dan bukan teks; `jalan` nama pintu yang mengambil byte-nya.
fn bukan_teks(fname: &str, jalan: &str) -> Value {
    err_value(format!("{}: data yang datang bukan teks UTF-8 yang sah; pakai `{}` untuk mengambil byte-nya", fname, jalan))
}

/// Lebar sebuah karakter UTF-8 dari byte pertamanya (0 kalau bukan byte pembuka).
fn lebar_utf8(b: u8) -> usize {
    match b { 0xC2..=0xDF => 2, 0xE0..=0xEF => 3, 0xF0..=0xF4 => 4, _ => 0 }
}

/// `dok::tcp_recv`: paling banyak `maks` byte, tapi tidak pernah membelah satu karakter —
/// potongan yang belum lengkap ditinggal di penampung untuk bacaan berikutnya.
pub fn tcp_recv(args: &[Value]) -> Result<Value, String> {
    const NAMA: &str = "dok::tcp_recv";
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), NAMA, "TcpConn")?;
    let max = as_usize(args.get(1).unwrap_or(&Value::Int(4096)), NAMA)?.clamp(1, MAKS_SEKALI_BACA);
    let Some(cell) = get_stream(id) else {
        return Ok(err_value("dok::tcp_recv: koneksi tidak ditemukan (sudah ditutup?)".to_string()));
    };
    let mut reader = cell.lock().unwrap_or_else(|p| p.into_inner());
    let (teks, pakai, kurang) = match reader.fill_buf() {
        Err(e) => return Ok(err_value(format!("{}: {}", NAMA, crate::host::galat_io(&e)))),
        Ok(ada) => {
            let b = &ada[..ada.len().min(max)];
            let sah = match std::str::from_utf8(b) {
                Ok(_) => b.len(),
                Err(e) if e.valid_up_to() > 0 => e.valid_up_to(),
                Err(e) if e.error_len().is_none() => 0,
                Err(_) => return Ok(bukan_teks(NAMA, "dok::tcp_recv_bytes")),
            };
            let kurang = if sah == 0 && !b.is_empty() { lebar_utf8(b[0]) } else { 0 };
            (String::from_utf8_lossy(&b[..sah]).to_string(), sah, kurang)
        }
    };
    reader.consume(pakai);
    if kurang == 0 { return Ok(ok_value(Value::Str(teks))); }
    let mut satu = vec![0u8; kurang];
    match reader.read_exact(&mut satu) {
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            Ok(err_value(format!("{}: koneksi ditutup di tengah sebuah karakter", NAMA))),
        Err(e) => Ok(err_value(format!("{}: {}", NAMA, crate::host::galat_io(&e)))),
        Ok(()) => Ok(match String::from_utf8(satu) {
            Ok(t) => ok_value(Value::Str(t)),
            Err(_) => bukan_teks(NAMA, "dok::tcp_recv_bytes"),
        }),
    }
}

/// `dok::tcp_recv_bytes(sock, maks)`: byte apa pun, sebagai array bilangan 0 sampai 255.
pub fn tcp_recv_bytes(args: &[Value]) -> Result<Value, String> {
    const NAMA: &str = "dok::tcp_recv_bytes";
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), NAMA, "TcpConn")?;
    let max = as_usize(args.get(1).unwrap_or(&Value::Int(4096)), NAMA)?.clamp(1, MAKS_SEKALI_BACA);
    let Some(cell) = get_stream(id) else {
        return Ok(err_value("dok::tcp_recv_bytes: koneksi tidak ditemukan (sudah ditutup?)".to_string()));
    };
    let mut reader = cell.lock().unwrap_or_else(|p| p.into_inner());
    let b = match reader.fill_buf() {
        Ok(ada) => ada[..ada.len().min(max)].to_vec(),
        Err(e) => return Ok(err_value(format!("{}: {}", NAMA, crate::host::galat_io(&e)))),
    };
    reader.consume(b.len());
    Ok(ok_value(super::crypto::byte_ke_array(b)))
}

pub fn tcp_recv_line(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_recv_line", "TcpConn")?;

    match get_stream(id) {
        Some(cell) => {
            let mut reader = cell.lock().unwrap_or_else(|p| p.into_inner());
            // Baris tanpa akhir dari lawan bicara tidak boleh menghabiskan memori.
            let mut mentah = Vec::new();
            let hasil = (&mut *reader).take(MAKS_SEKALI_BACA as u64 + 1).read_until(b'\n', &mut mentah);
            if mentah.len() > MAKS_SEKALI_BACA {
                return Ok(err_value(format!("dok::tcp_recv_line: baris lebih dari {} byte tanpa akhir baris", MAKS_SEKALI_BACA)));
            }
            let mut line = match String::from_utf8(mentah) {
                Ok(t) => t,
                Err(_) => return Ok(bukan_teks("dok::tcp_recv_line", "dok::tcp_recv_bytes")),
            };
            match hasil {
                Ok(0) => Ok(err_value("dok::tcp_recv_line: EOF (koneksi ditutup peer)".to_string())),
                Ok(_) => {
                    while line.ends_with('\n') || line.ends_with('\r') { line.pop(); }
                    Ok(ok_value(Value::Str(line)))
                }
                Err(e) => Ok(err_value(format!("dok::tcp_recv_line: {}", crate::host::galat_io(&e)))),
            }
        }
        None => Ok(err_value("dok::tcp_recv_line: koneksi tidak ditemukan".to_string())),
    }
}

pub fn tcp_recv_all(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_recv_all", "TcpConn")?;

    match get_stream(id) {
        Some(cell) => {
            let mut reader = cell.lock().unwrap_or_else(|p| p.into_inner());
            let mut buf = Vec::new();
            match reader.read_to_end(&mut buf) {
                Ok(_) => Ok(match String::from_utf8(buf) {
                    Ok(t) => ok_value(Value::Str(t)),
                    Err(_) => bukan_teks("dok::tcp_recv_all", "dok::tcp_recv_bytes"),
                }),
                Err(e) => Ok(err_value(format!("dok::tcp_recv_all: {}", crate::host::galat_io(&e)))),
            }
        }
        None => Ok(err_value("dok::tcp_recv_all: koneksi tidak ditemukan".to_string())),
    }
}

pub fn tcp_close(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_close", "TcpConn")?;
    tcp_streams().lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    Ok(Value::Unit)
}

pub fn tcp_set_timeout(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_set_timeout", "TcpConn")?;
    let ms = as_usize(args.get(1).unwrap_or(&Value::Nil), "dok::tcp_set_timeout")?;

    match get_stream(id) {
        Some(cell) => {
            let reader = cell.lock().unwrap_or_else(|p| p.into_inner());
            let dur = if ms == 0 { None } else { Some(Duration::from_millis(ms as u64)) };
            let r1 = reader.get_ref().set_read_timeout(dur);
            let r2 = reader.get_ref().set_write_timeout(dur);
            match (r1, r2) {
                (Ok(()), Ok(())) => Ok(ok_value(Value::Nil)),
                (Err(e), _) | (_, Err(e)) => Ok(err_value(format!("dok::tcp_set_timeout: {}", crate::host::galat_io(&e)))),
            }
        }
        None => Ok(err_value("dok::tcp_set_timeout: koneksi tidak ditemukan".to_string())),
    }
}

pub fn tcp_local_addr(args: &[Value]) -> Result<Value, String> {
    const F: &str = "dok::tcp_local_addr";
    let v = args.get(0).unwrap_or(&Value::Nil);
    // Listener juga punya alamat lokal — itu satu-satunya cara tahu port yang dipilih
    // OS sesudah bind ke port 0. Dulu dijawab teks KOSONG, begitu juga koneksi yang ditutup.
    if matches!(v, Value::Struct { name, .. } if name == "TcpListener") {
        let id = handle_id(v, F, "TcpListener")?;
        let guard = tcp_listeners().lock().unwrap_or_else(|p| p.into_inner());
        let l = guard.get(&id).ok_or_else(|| format!("{}: listener ini sudah ditutup", F))?;
        return l.local_addr().map(|a| Value::Str(a.to_string())).map_err(|e| format!("{}: {}", F, crate::host::galat_io(&e)));
    }
    alamat(v, F, |s| s.local_addr())
}

/// `dok::udp_local_addr(sock)`: satu-satunya cara tahu port yang dipilih OS sesudah bind ke 0.
pub fn udp_local_addr(args: &[Value]) -> Result<Value, String> {
    const F: &str = "dok::udp_local_addr";
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), F, "UdpSock")?;
    let sock = get_udp(id).ok_or_else(|| format!("{}: socket ini sudah ditutup", F))?;
    sock.local_addr().map(|a| Value::Str(a.to_string())).map_err(|e| format!("{}: {}", F, crate::host::galat_io(&e)))
}

pub fn tcp_peer_addr(args: &[Value]) -> Result<Value, String> {
    alamat(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_peer_addr", |s| s.peer_addr())
}

/// Alamat sebuah koneksi yang MASIH terbuka; yang sudah ditutup ditolak, bukan teks kosong.
fn alamat(v: &Value, f: &str, ambil: impl Fn(&TcpStream) -> std::io::Result<std::net::SocketAddr>)
    -> Result<Value, String> {
    let id = handle_id(v, f, "TcpConn")?;
    let cell = get_stream(id).ok_or_else(|| format!("{}: koneksi ini sudah ditutup", f))?;
    let reader = cell.lock().unwrap_or_else(|p| p.into_inner());
    ambil(reader.get_ref()).map(|a| Value::Str(a.to_string())).map_err(|e| format!("{}: {}", f, crate::host::galat_io(&e)))
}

// ─── TCP: server-side primitives (dipakai juga oleh dok::http server) ───

pub fn tcp_listen(args: &[Value]) -> Result<Value, String> {
    let host = as_str(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_listen", 1)?;
    let port = as_port(args.get(1).unwrap_or(&Value::Nil), "dok::tcp_listen")?;
    let addr = format!("{}:{}", host, port);
    match TcpListener::bind(&addr) {
        Ok(listener) => {
            let id = next_id();
            tcp_listeners().lock().unwrap_or_else(|p| p.into_inner()).insert(id, listener);
            Ok(ok_value(make_handle("TcpListener", id)))
        }
        Err(e) => Ok(err_value(format!("dok::tcp_listen '{}': {}", addr, crate::host::galat_io(&e)))),
    }
}

pub fn tcp_accept(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_accept", "TcpListener")?;
    // Kunci sebentar hanya untuk clone-able check; `accept()` sendiri
    // dipanggil di luar lock supaya listener lain (dan koneksi lain)
    // tidak ikut terblokir selama menunggu koneksi masuk.
    let listener_ref = {
        let guard = tcp_listeners().lock().unwrap_or_else(|p| p.into_inner());
        match guard.get(&id) {
            Some(l) => l.try_clone(),
            None => return Ok(err_value("dok::tcp_accept: listener tidak ditemukan".to_string())),
        }
    };
    match listener_ref {
        Ok(listener) => match listener.accept() {
            Ok((stream, _addr)) => {
                let cid = next_id();
                tcp_streams().lock().unwrap_or_else(|p| p.into_inner())
                    .insert(cid, Arc::new(Mutex::new(BufReader::new(stream))));
                Ok(ok_value(make_handle("TcpConn", cid)))
            }
            Err(e) => Ok(err_value(format!("dok::tcp_accept: {}", crate::host::galat_io(&e)))),
        },
        Err(e) => Ok(err_value(format!("dok::tcp_accept: {}", crate::host::galat_io(&e)))),
    }
}

pub fn tcp_close_listener(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::tcp_close_listener", "TcpListener")?;
    tcp_listeners().lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    Ok(Value::Unit)
}

// ─── UDP ────────────────────────────────────────────────────────────────
// `UdpSocket::send_to`/`recv_from` menerima `&self` — sudah thread-safe …

pub fn udp_bind(args: &[Value]) -> Result<Value, String> {
    let host = as_str(args.get(0).unwrap_or(&Value::Nil), "dok::udp_bind", 1)?;
    let port = as_port(args.get(1).unwrap_or(&Value::Nil), "dok::udp_bind")?;
    let addr = format!("{}:{}", host, port);
    match UdpSocket::bind(&addr) {
        Ok(sock) => {
            let id = next_id();
            udp_sockets().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(sock));
            Ok(ok_value(make_handle("UdpSock", id)))
        }
        Err(e) => Ok(err_value(format!("dok::udp_bind '{}': {}", addr, crate::host::galat_io(&e)))),
    }
}

pub fn udp_send_to(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::udp_send_to", "UdpSock")?;
    let host = as_str(args.get(1).unwrap_or(&Value::Nil), "dok::udp_send_to", 2)?;
    let port = as_port(args.get(2).unwrap_or(&Value::Nil), "dok::udp_send_to")?;
    let data = data_kirim(args.get(3).unwrap_or(&Value::Nil), "dok::udp_send_to", 4)?;

    match get_udp(id) {
        Some(sock) => match sock.send_to(&data, (host, port)) {
            Ok(n) => Ok(ok_value(Value::Int(n as i128))),
            Err(e) => Ok(err_value(format!("dok::udp_send_to: {}", crate::host::galat_io(&e)))),
        },
        None => Ok(err_value("dok::udp_send_to: socket tidak ditemukan".to_string())),
    }
}

pub fn udp_recv_from(args: &[Value]) -> Result<Value, String> { udp_terima(args, "dok::udp_recv_from", false) }

/// `dok::udp_recv_bytes_from(sock, maks)`: seperti `udp_recv_from`, `data`-nya array byte.
pub fn udp_recv_bytes_from(args: &[Value]) -> Result<Value, String> { udp_terima(args, "dok::udp_recv_bytes_from", true) }

/// Datagram teks yang rusak ditolak SEBELUM diambil (`peek_from`), jadi pintu byte masih
/// bisa membacanya; karakter yang terpotong oleh `maks` sendiri dibuang, bukan dirusak.
fn udp_terima(args: &[Value], nama: &str, byte: bool) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), nama, "UdpSock")?;
    let max = as_usize(args.get(1).unwrap_or(&Value::Int(4096)), nama)?.clamp(1, MAKS_DATAGRAM);
    let Some(sock) = get_udp(id) else {
        return Ok(err_value(format!("{}: socket tidak ditemukan", nama)));
    };
    let teks = |b: &[u8]| match std::str::from_utf8(b) {
        Ok(t) => Some(t.to_string()),
        Err(e) if b.len() == max && e.error_len().is_none() =>
            Some(String::from_utf8_lossy(&b[..e.valid_up_to()]).to_string()),
        Err(_) => None,
    };
    let mut buf = vec![0u8; max];
    if !byte {
        match sock.peek_from(&mut buf) {
            Ok((n, _)) if teks(&buf[..n]).is_none() => return Ok(bukan_teks(nama, "dok::udp_recv_bytes_from")),
            Ok(_) => {}
            Err(e) => return Ok(err_value(format!("{}: {}", nama, crate::host::galat_io(&e)))),
        }
    }
    match sock.recv_from(&mut buf) {
        Ok((n, from)) => {
            let data = if byte { super::crypto::byte_ke_array(buf[..n].to_vec()) } else {
                match teks(&buf[..n]) { Some(t) => Value::Str(t), None => return Ok(bukan_teks(nama, "dok::udp_recv_bytes_from")) }
            };
            let mut fields = HashMap::new();
            fields.insert("data".to_string(), data);
            fields.insert("host".to_string(), Value::Str(from.ip().to_string()));
            fields.insert("port".to_string(), Value::Int(from.port() as i128));
            Ok(ok_value(Value::Struct { name: "UdpPacket".to_string(), fields: fields.into() }))
        }
        Err(e) => Ok(err_value(format!("{}: {}", nama, crate::host::galat_io(&e)))),
    }
}

pub fn udp_close(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::udp_close", "UdpSock")?;
    udp_sockets().lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    Ok(Value::Unit)
}

// ─── DNS ────────────────────────────────────────────────────────────────

pub fn resolve(args: &[Value]) -> Result<Value, String> {
    let host = as_str(args.get(0).unwrap_or(&Value::Nil), "dok::resolve", 1)?;
    // `ToSocketAddrs` butuh port; pakai 0 sebagai placeholder karena kita
    // hanya peduli alamat IP-nya.
    match (host, 0u16).to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(a) => Ok(ok_value(Value::Str(a.ip().to_string()))),
            None => Ok(err_value(format!("dok::resolve '{}': tidak ada alamat", host))),
        },
        Err(e) => Ok(err_value(format!("dok::resolve '{}': {}", host, crate::host::galat_io(&e)))),
    }
}
