// src/dok/klien.rs — satu klien HTTP untuk `http::` dan `dok::http_*`: URL, sambungan polos atau TLS
// (`dok::tls`), badan berpotongan (`Transfer-Encoding: chunked`), dan `unduh` yang menulis badan langsung
// ke berkas (`<berkas>.sebagian`, diganti nama saat lengkap) sambil mengikuti pengalihan.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::tls::Tls;

#[derive(Debug)]
pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub jalur: String,
}

/// `http://h[:p]/j`, `https://...`, atau tanpa skema (= http) — lewat pengurai `url::` yang sama dengan pemakai.
pub fn urai_url(url: &str) -> Result<Url, String> {
    let lengkap = if url.contains("://") { url.to_string() } else { format!("http://{}", url) };
    let u = crate::stdlib::url::urai(&lengkap)?;
    let https = match u.skema.as_str() {
        "https" => true,
        "http" => false,
        lain => return Err(format!("skema URL `{}` tidak didukung; yang ada http:// dan https://", lain)),
    };
    if u.pengguna.is_some() {
        return Err(format!("URL `{}` memuat kredensial (`nama@`) — tidak dikirim; pasang kepala `Authorization`",
            crate::kutip_inti::kutip(crate::stdlib::url::samarkan_userinfo(url))));
    }
    let host = u.host.clone().unwrap_or_default();
    let port = u.port.unwrap_or(if https { 443 } else { 80 });
    // Fragmen tidak pernah dikirim ke server (RFC 9110 §4.2.4).
    let jalur = match &u.query { Some(q) => format!("{}?{}", u.jalur, q), None => u.jalur.clone() };
    Ok(Url { https, host, port, jalur })
}

pub enum Sambungan {
    Polos(TcpStream),
    Tls(Box<Tls>),
}

impl Read for Sambungan {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self { Sambungan::Polos(s) => s.read(b), Sambungan::Tls(t) => t.read(b) }
    }
}

impl Write for Sambungan {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self { Sambungan::Polos(s) => s.write(b), Sambungan::Tls(t) => t.write(b) }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self { Sambungan::Polos(s) => s.flush(), Sambungan::Tls(t) => t.flush() }
    }
}

static HANYA_PUBLIK: AtomicBool = AtomicBool::new(false);

/// `http::hanya_publik(aktif)`: sesudahnya klien menolak alamat internal, juga di tiap pengalihan.
pub fn atur_hanya_publik(aktif: bool) { HANYA_PUBLIK.store(aktif, Ordering::SeqCst) }

/// Golongan alamat yang tidak boleh dicapai di bawah `hanya_publik` — `None` kalau alamatnya publik.
pub fn alamat_internal(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(a) => internal_v4(a),
        IpAddr::V6(a) => {
            let s = a.segments();
            // Alamat IPv4 yang dibungkus IPv6 dinilai sebagai IPv4-nya: mapped, compatible, NAT64, 6to4.
            let v4 = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
            if let Some(m) = a.to_ipv4_mapped() { return internal_v4(m); }
            if a.is_loopback() { return Some("loopback (::1)"); }
            if a.is_unspecified() { return Some("tak bertentu (::)"); }
            if s[..6] == [0; 6] { return internal_v4(v4(s[6], s[7])); }
            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] { return internal_v4(v4(s[6], s[7])); }
            if s[0] == 0x2002 { return internal_v4(v4(s[1], s[2])); }
            if s[0] & 0xfe00 == 0xfc00 { return Some("privat IPv6 (fc00::/7)"); }
            if s[0] & 0xffc0 == 0xfe80 { return Some("link-local IPv6 (fe80::/10)"); }
            if s[0] & 0xffc0 == 0xfec0 { return Some("site-local IPv6 (fec0::/10)"); }
            if s[0] >> 8 == 0xff { return Some("multicast IPv6"); }
            None
        }
    }
}

fn internal_v4(a: Ipv4Addr) -> Option<&'static str> {
    let o = a.octets();
    if a.is_loopback() { return Some("loopback (127.0.0.0/8)"); }
    if o[0] == 0 { return Some("tak bertentu (0.0.0.0/8)"); }
    if a.is_private() { return Some("privat (10/8, 172.16/12, 192.168/16)"); }
    if a.is_link_local() { return Some("link-local (169.254.0.0/16, termasuk metadata awan 169.254.169.254)"); }
    if o[0] == 100 && o[1] & 0xc0 == 64 { return Some("CGNAT (100.64.0.0/10)"); }
    if o[0] == 192 && o[1] == 0 && o[2] == 0 { return Some("cadangan IETF (192.0.0.0/24)"); }
    if o[0] == 198 && o[1] & 0xfe == 18 { return Some("uji jaringan (198.18.0.0/15)"); }
    if o[0] >= 224 { return Some("multicast, cadangan, atau broadcast (224.0.0.0/3)"); }
    None
}

/// Nama diurai SEKALI dan yang disambungi alamat yang diperiksa itu juga — dua kali mengurai bisa menjawab
/// dua alamat (DNS rebinding). Satu alamat internal di antara jawabannya cukup untuk menolak semuanya.
pub fn sambung(u: &Url) -> Result<Sambungan, String> {
    let alamat = format!("{}:{}", u.host, u.port);
    let gagal = |e: &std::io::Error| format!("tidak bisa tersambung ke {}: {}", alamat, crate::host::galat_io(e));
    let daftar: Vec<SocketAddr> = alamat.to_socket_addrs().map_err(|e| gagal(&e))?.collect();
    if HANYA_PUBLIK.load(Ordering::SeqCst) {
        if let Some((a, golongan)) = daftar.iter().find_map(|a| alamat_internal(a.ip()).map(|g| (a, g))) {
            let tunjuk = if u.host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() { String::new() }
                         else { format!("{} menunjuk ", u.host) };
            return Err(format!("{}{}, alamat {} — ditolak karena `http::hanya_publik(true)` (kelas SSRF)",
                               tunjuk, a.ip(), golongan));
        }
    }
    let mut akhir = std::io::Error::new(std::io::ErrorKind::NotFound, "nama host tidak punya alamat");
    let mut tcp = None;
    for a in &daftar {
        match TcpStream::connect_timeout(a, Duration::from_secs(10)) {
            Ok(t) => { tcp = Some(t); break; }
            Err(e) => akhir = e,
        }
    }
    let tcp = tcp.ok_or_else(|| gagal(&akhir))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(30))).ok();
    if u.https { Ok(Sambungan::Tls(Box::new(Tls::sambung(tcp, &u.host)?))) } else { Ok(Sambungan::Polos(tcp)) }
}

fn kepala_host(u: &Url) -> String {
    let bawaan = if u.https { 443 } else { 80 };
    if u.port == bawaan { u.host.clone() } else { format!("{}:{}", u.host, u.port) }
}

/// Kirim permintaan dan baca jawaban utuhnya (kepala + badan mentah, berpotongan kalau server begitu).
pub fn minta(metode: &str, url: &str, badan: &[u8], kepala: &[(String, String)], agen: &str) -> Result<Vec<u8>, String> {
    let u = urai_url(url)?;
    periksa_permintaan(&u, metode, kepala, agen)?;
    let mut s = sambung(&u)?;
    kirim(&mut s, &u, metode, badan, kepala, agen)?;
    baca_berbatas(&mut s, &u.host, MAKS_JAWABAN)
}

/// Jawaban yang disimpan di memori berbatas; yang lebih besar diunduh ke berkas (`http::unduh`).
pub const MAKS_JAWABAN: u64 = 256 << 20;

/// Server yang menjawab tanpa henti dulu menghabiskan memori proses sebelum ada galat.
fn baca_berbatas(s: &mut impl Read, host: &str, maks: u64) -> Result<Vec<u8>, String> {
    let mut jawab = Vec::new();
    s.take(maks + 1).read_to_end(&mut jawab).map_err(|e| format!("membaca jawaban dari {}: {}", host, crate::host::galat_io(&e)))?;
    if jawab.len() as u64 > maks {
        return Err(format!("jawaban dari {} melewati {} MiB — unduh ke berkas lewat `http::unduh`", host, maks >> 20));
    }
    Ok(jawab)
}

/// Teks yang ditulis ke baris permintaan atau kepala tidak boleh memotong barisnya (CVE-2019-9740 di Python).
fn periksa_permintaan(u: &Url, metode: &str, kepala: &[(String, String)], agen: &str) -> Result<(), String> {
    let kendali = |t: &str| t.bytes().any(|b| b < 0x20 || b == 0x7f);
    if !super::http::nama_kepala_sah(metode) { return Err(format!("metode HTTP {:?} tidak sah", metode)); }
    if kendali(&u.jalur) || u.jalur.contains(' ') {
        return Err(format!("jalur URL {:?} memuat spasi atau karakter kendali — loloskan dulu (`http::url_encode`)", u.jalur));
    }
    if kendali(&u.host) || u.host.contains(' ') { return Err(format!("nama host {:?} tidak sah", u.host)); }
    for (k, v) in kepala.iter().map(|(k, v)| (k.as_str(), v.as_str())).chain(std::iter::once(("User-Agent", agen))) {
        if !super::http::nama_kepala_sah(k) { return Err(format!("nama kepala {:?} tidak sah", k)); }
        if v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
            return Err(format!("kepala `{}` memuat CR, LF atau NUL, yang akan menyisipkan kepala lain", k));
        }
        // Pembingkaian diatur klien: kepala kedua membuat server dan proxy membaca batas permintaan berbeda
        // (penyelundupan permintaan, CL.CL / TE.CL).
        if ["content-length", "transfer-encoding", "connection"].iter().any(|n| k.eq_ignore_ascii_case(n)) {
            return Err(format!("kepala `{}` diatur klien sendiri — menulisnya lagi membuat permintaan dibaca dua cara \
                (penyelundupan permintaan); buang kepala itu", k));
        }
    }
    if kepala.iter().filter(|(k, _)| k.eq_ignore_ascii_case("host")).count() > 1 {
        return Err("kepala `Host` ditulis dua kali — ambigu, ditolak".into());
    }
    Ok(())
}

fn kirim(s: &mut Sambungan, u: &Url, metode: &str, badan: &[u8], kepala: &[(String, String)], agen: &str) -> Result<(), String> {
    // `Host` dari pemakai MENGGANTIKAN yang otomatis (virtual host), tidak pernah jadi yang kedua.
    let host = kepala.iter().find(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.clone())
        .unwrap_or_else(|| kepala_host(u));
    let mut r = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: {}\r\n", metode, u.jalur, host, agen);
    if !badan.is_empty() { r.push_str(&format!("Content-Length: {}\r\n", badan.len())); }
    for (k, v) in kepala.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("host")) { r.push_str(&format!("{}: {}\r\n", k, v)); }
    r.push_str("\r\n");
    let mut semua = r.into_bytes();
    semua.extend_from_slice(badan);
    s.write_all(&semua).map_err(|e| format!("mengirim ke {}: {}", u.host, crate::host::galat_io(&e)))
}

/// Batas baris kepala dan jumlah kepala jawaban di klien (sepadan dengan `dok::serve`).
const MAKS_BARIS_KEPALA: usize = 8 * 1024;
const MAKS_KEPALA: usize = 100;

fn baca_baris_terbatas<R: BufRead>(r: &mut R, konteks: &str) -> Result<String, String> {
    let mut buf = Vec::new();
    let n = r.take(MAKS_BARIS_KEPALA as u64 + 1).read_until(b'\n', &mut buf)
        .map_err(|e| format!("{}: {}", konteks, crate::host::galat_io(&e)))?;
    if n == 0 { return Ok(String::new()); }
    if buf.len() > MAKS_BARIS_KEPALA {
        return Err(format!("{}: baris melewati {} byte", konteks, MAKS_BARIS_KEPALA));
    }
    String::from_utf8(buf).map_err(|_| format!("{}: baris bukan teks UTF-8 yang sah", konteks))
}

fn baca_kepala_jawaban<R: BufRead>(r: &mut R, host: &str) -> Result<Vec<String>, String> {
    let mut kepala = Vec::new();
    loop {
        let b = baca_baris_terbatas(r, "membaca kepala jawaban")?;
        if b.is_empty() { return Err(format!("{} menutup sambungan sebelum menjawab", host)); }
        let b = b.trim_end_matches(['\r', '\n']).to_string();
        if b.is_empty() { break; }
        if kepala.len() > MAKS_KEPALA {
            return Err(format!("jawaban {} memiliki lebih dari {} kepala", host, MAKS_KEPALA));
        }
        kepala.push(b);
    }
    Ok(kepala)
}

/// Badan `Transfer-Encoding: chunked` jadi badan utuh; yang rusak DITOLAK, bukan dipotong diam-diam.
pub fn urai_berpotongan(b: &[u8]) -> Result<Vec<u8>, String> {
    let mut keluar = Vec::new();
    let mut r = BufReader::new(b);
    salin_berpotongan(&mut r, &mut keluar).map(|_| keluar)
}

fn salin_berpotongan<R: BufRead, W: Write>(r: &mut R, w: &mut W) -> Result<u64, String> {
    let mut total = 0u64;
    loop {
        let baris = baca_baris_terbatas(r, "badan berpotongan")?;
        if baris.is_empty() { return Err("badan berpotongan terputus sebelum potongan terakhir".into()); }
        let ukuran = baris.trim().split(';').next().unwrap_or("");
        let n = u64::from_str_radix(ukuran.trim(), 16)
            .map_err(|_| format!("badan berpotongan rusak: ukuran `{}` bukan heksadesimal", baris.trim()))?;
        if n == 0 { return Ok(total); }
        let disalin = std::io::copy(&mut r.by_ref().take(n), w).map_err(|e| crate::host::galat_io(&e))?;
        if disalin != n { return Err("badan berpotongan terputus di tengah potongan".into()); }
        total = total.checked_add(n).ok_or_else(|| "badan berpotongan meluap".to_string())?;
        let mut crlf = [0u8; 2];
        r.read_exact(&mut crlf).map_err(|_| "badan berpotongan terputus sesudah potongan".to_string())?;
        if crlf != *b"\r\n" {
            return Err("badan berpotongan rusak: potongan tidak ditutup CRLF".into());
        }
    }
}

fn gabung(asal: &Url, lokasi: &str) -> Result<String, String> {
    let skema = if asal.https { "https" } else { "http" };
    let dasar = format!("{}://{}{}", skema, kepala_host(asal), asal.jalur);
    crate::stdlib::url::gabung(&dasar, lokasi).map_err(|e| format!("Location `{}` tidak sah: {}", lokasi, e))
}

/// Pengalihan dari https ke http membuka isi unduhan bagi siapa pun di jalurnya.
fn periksa_alih(asal: &Url, tujuan: &str) -> Result<(), String> {
    if asal.https && !tujuan.starts_with("https://") {
        return Err(format!("pengalihan dari https://{} ke {} ditolak — turun ke http membuka isinya di jalan",
                           kepala_host(asal), tujuan));
    }
    Ok(())
}

pub struct HasilUnduh {
    pub status: u16,
    pub ukuran: u64,
    pub url: String,
}

/// GET `url` ke `berkas`, mengikuti sampai 10 pengalihan. Selagi berjalan isinya di `<berkas>.sebagian`
/// (ukurannya bisa dibaca untuk kemajuan); jawaban yang bukan 2xx tidak menulis apa pun.
pub fn unduh(url: &str, berkas: &str, agen: &str) -> Result<HasilUnduh, String> {
    let mut sekarang = url.to_string();
    for _ in 0..10 {
        let u = urai_url(&sekarang)?;
        periksa_permintaan(&u, "GET", &[], agen)?;
        let mut s = sambung(&u)?;
        kirim(&mut s, &u, "GET", &[], &[], agen)?;
        let mut r = BufReader::new(s);
        let kepala = baca_kepala_jawaban(&mut r, &u.host)?;
        let status: u16 = kepala.first().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("jawaban {} bukan HTTP", u.host))?;
        let cari = |nama: &str| kepala.iter().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim().eq_ignore_ascii_case(nama)).then(|| v.trim().to_string())
        });
        if (300..400).contains(&status) {
            let lokasi = cari("location").ok_or_else(|| format!("pengalihan {} tanpa Location", status))?;
            sekarang = gabung(&u, &lokasi)?;
            periksa_alih(&u, &sekarang)?;
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(format!("{} menjawab {}", sekarang, kepala[0].splitn(2, ' ').nth(1).unwrap_or("")));
        }
        let sebagian = format!("{}.sebagian", berkas);
        let mut f = std::fs::File::create(&sebagian).map_err(|e| format!("{}: {}", sebagian, crate::host::galat_io(&e)))?;
        let hasil = (|| -> Result<u64, String> {
            if cari("transfer-encoding").map_or(false, |v| v.to_ascii_lowercase().contains("chunked")) {
                return salin_berpotongan(&mut r, &mut f);
            }
            match cari("content-length").and_then(|v| v.parse::<u64>().ok()) {
                Some(n) => {
                    let d = std::io::copy(&mut r.by_ref().take(n), &mut f).map_err(|e| crate::host::galat_io(&e))?;
                    if d != n { return Err(format!("unduhan terputus: {} dari {} byte", d, n)); }
                    Ok(d)
                }
                None => std::io::copy(&mut r, &mut f).map_err(|e| crate::host::galat_io(&e)),
            }
        })();
        drop(f);
        return match hasil {
            Ok(ukuran) => {
                std::fs::rename(&sebagian, berkas).map_err(|e| format!("{}: {}", berkas, crate::host::galat_io(&e)))?;
                Ok(HasilUnduh { status, ukuran, url: sekarang })
            }
            Err(e) => Err(format!("mengunduh {}: {} (yang sudah sampai ada di {})", sekarang, e, sebagian)),
        };
    }
    Err(format!("lebih dari 10 pengalihan mulai dari {}", url))
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn permintaan_tidak_bisa_disisipi() {
        // CVE-2019-9740 di Python: jalur URL ber-CRLF menyisipkan baris permintaan.
        assert!(urai_url("http://a.b/x HTTP/1.1\r\nX-Sisip: 1").unwrap_err().contains("spasi"));
        let u = urai_url("http://a.b/x").unwrap();
        assert!(periksa_permintaan(&u, "GET", &[("X-A".into(), "1\r\nX-B: 2".into())], "jrg").is_err());
        assert!(periksa_permintaan(&u, "GE T", &[], "jrg").is_err());
        assert!(periksa_permintaan(&u, "GET", &[("X-A".into(), "1".into())], "jrg").is_ok());
    }

    #[test]
    fn alamat_internal_dikenali_dalam_tiap_bungkusnya() {
        for ip in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1",
                   "224.0.0.1", "255.255.255.255", "::1", "::", "::ffff:127.0.0.1", "::ffff:169.254.169.254",
                   "::127.0.0.1", "64:ff9b::a9fe:a9fe", "2002:7f00:1::", "fd00::1", "fe80::1", "ff02::1"] {
            assert!(alamat_internal(ip.parse().unwrap()).is_some(), "{} harus internal", ip);
        }
        for ip in ["8.8.8.8", "1.1.1.1", "172.32.0.1", "100.128.0.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert!(alamat_internal(ip.parse().unwrap()).is_none(), "{} harus publik", ip);
        }
    }

    #[test]
    fn pengalihan_tidak_turun_ke_http() {
        let aman = urai_url("https://a.test/x").unwrap();
        assert!(periksa_alih(&aman, "https://b.test/y").is_ok());
        assert!(periksa_alih(&aman, "http://a.test/y").unwrap_err().contains("ditolak"));
        assert!(periksa_alih(&urai_url("http://a.test/").unwrap(), "http://b.test/").is_ok());
        let v6 = urai_url("http://[::1]/a").unwrap();
        assert_eq!((v6.host.as_str(), v6.port), ("[::1]", 80));
        assert_eq!(urai_url("https://[::1]:8443/").unwrap().port, 8443);
    }

    #[test]
    fn url_diurai_dan_digabung() {
        let u = urai_url("https://a.b/c/d?x=1").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.jalur.as_str()), (true, "a.b", 443, "/c/d?x=1"));
        let u = urai_url("h:8080").unwrap();
        assert_eq!((u.https, u.port, u.jalur.as_str()), (false, 8080, "/"));
        assert!(urai_url("ftp://x").unwrap_err().contains("ftp"));
        let a = urai_url("http://h:81/a/b.txt").unwrap();
        assert_eq!(gabung(&a, "c.txt").unwrap(), "http://h:81/a/c.txt");
        assert_eq!(gabung(&a, "/z").unwrap(), "http://h:81/z");
        assert_eq!(gabung(&a, "HTTPS://q/r").unwrap(), "https://q/r");
        assert_eq!(gabung(&a, "../x?q#f").unwrap(), "http://h:81/x?q#f");
        assert!(urai_url("http://u:p@h/").unwrap_err().contains("kredensial"));
        assert_eq!(urai_url("http://h/a#frag").unwrap().jalur, "/a");
    }

    #[test]
    fn jawaban_berbatas_ukurannya() {
        let pas = vec![b'x'; 2 << 20];
        assert_eq!(baca_berbatas(&mut &pas[..], "h", 2 << 20).unwrap().len(), 2 << 20);
        let lebih = vec![b'x'; (2 << 20) + 1];
        let g = baca_berbatas(&mut &lebih[..], "h", 2 << 20).unwrap_err();
        assert!(g.contains("melewati 2 MiB") && g.contains("http::unduh"), "{}", g);
        // Server tanpa henti: berhenti membaca sesudah batasnya, tidak menunggu akhir.
        let g = baca_berbatas(&mut std::io::repeat(b'y'), "h", 1 << 20).unwrap_err();
        assert!(g.contains("melewati 1 MiB"), "{}", g);
    }

    #[test]
    fn badan_berpotongan() {
        assert_eq!(urai_berpotongan(b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
        assert!(urai_berpotongan(b"zz\r\nab\r\n").unwrap_err().contains("bukan heksadesimal"));
        assert!(urai_berpotongan(b"4\r\nWi").unwrap_err().contains("terputus"));
        // Potongan yang tidak ditutup CRLF ditolak, bukan memakan 2 byte badan berikutnya.
        assert!(urai_berpotongan(b"4\r\nWikiXX0\r\n\r\n").unwrap_err().contains("CRLF"));
        let raksasa = format!("4;{}\r\nWiki\r\n0\r\n\r\n", "a".repeat(MAKS_BARIS_KEPALA + 10));
        assert!(urai_berpotongan(raksasa.as_bytes()).unwrap_err().contains("melewati"));
    }

    #[test]
    fn kepala_jawaban_unduh_berbatas() {
        let panjang = format!("HTTP/1.1 200 OK\r\nX: {}\r\n\r\n", "a".repeat(MAKS_BARIS_KEPALA));
        assert!(baca_kepala_jawaban(&mut BufReader::new(panjang.as_bytes()), "h").unwrap_err().contains("melewati"));
        let banyak = format!("HTTP/1.1 200 OK\r\n{}\r\n", "X: 1\r\n".repeat(MAKS_KEPALA + 1));
        assert!(baca_kepala_jawaban(&mut BufReader::new(banyak.as_bytes()), "h").unwrap_err().contains("lebih dari"));
        let sah = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        assert_eq!(baca_kepala_jawaban(&mut BufReader::new(&sah[..]), "h").unwrap().len(), 2);
    }
}
