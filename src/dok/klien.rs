// src/dok/klien.rs — satu klien HTTP untuk `http::` dan `dok::http_*`: URL, sambungan polos atau TLS
// (`dok::tls`), badan berpotongan (`Transfer-Encoding: chunked`), dan `unduh` yang menulis badan langsung
// ke berkas (`<berkas>.sebagian`, diganti nama saat lengkap) sambil mengikuti pengalihan.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::tls::Tls;

#[derive(Debug)]
pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub jalur: String,
}

/// `http://h[:p]/j`, `https://...`, atau tanpa skema (= http).
pub fn urai_url(url: &str) -> Result<Url, String> {
    let url = url.trim();
    let (https, sisa) = if let Some(r) = url.strip_prefix("https://") { (true, r) }
        else if let Some(r) = url.strip_prefix("http://") { (false, r) }
        else if url.contains("://") { return Err(format!("skema URL `{}` tidak didukung; yang ada http:// dan https://", url.split("://").next().unwrap_or(""))); }
        else { (false, url) };
    let garis = sisa.find(['/', '?']).unwrap_or(sisa.len());
    let (host_port, jalur) = (&sisa[..garis], &sisa[garis..]);
    let jalur = if jalur.is_empty() { "/".to_string() } else if jalur.starts_with('?') { format!("/{}", jalur) } else { jalur.to_string() };
    let (host, port) = match host_port.rfind(':') {
        None => (host_port.to_string(), if https { 443 } else { 80 }),
        Some(_) => super::http::urai_host_port(host_port)?,
    };
    if host.is_empty() { return Err(format!("URL `{}` tanpa nama host", url)); }
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

pub fn sambung(u: &Url) -> Result<Sambungan, String> {
    let alamat = format!("{}:{}", u.host, u.port);
    let tcp = TcpStream::connect(&alamat)
        .map_err(|e| format!("tidak bisa tersambung ke {}: {}", alamat, crate::host::galat_io(&e)))?;
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
    let mut s = sambung(&u)?;
    kirim(&mut s, &u, metode, badan, kepala, agen)?;
    let mut jawab = Vec::new();
    s.read_to_end(&mut jawab).map_err(|e| format!("membaca jawaban dari {}: {}", u.host, crate::host::galat_io(&e)))?;
    Ok(jawab)
}

fn kirim(s: &mut Sambungan, u: &Url, metode: &str, badan: &[u8], kepala: &[(String, String)], agen: &str) -> Result<(), String> {
    let mut r = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: {}\r\n", metode, u.jalur, kepala_host(u), agen);
    if !badan.is_empty() { r.push_str(&format!("Content-Length: {}\r\n", badan.len())); }
    for (k, v) in kepala { r.push_str(&format!("{}: {}\r\n", k, v)); }
    r.push_str("\r\n");
    let mut semua = r.into_bytes();
    semua.extend_from_slice(badan);
    s.write_all(&semua).map_err(|e| format!("mengirim ke {}: {}", u.host, crate::host::galat_io(&e)))
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
        let mut baris = String::new();
        r.read_line(&mut baris).map_err(|e| format!("badan berpotongan: {}", crate::host::galat_io(&e)))?;
        if baris.is_empty() { return Err("badan berpotongan terputus sebelum potongan terakhir".into()); }
        let ukuran = baris.trim().split(';').next().unwrap_or("");
        let n = u64::from_str_radix(ukuran.trim(), 16)
            .map_err(|_| format!("badan berpotongan rusak: ukuran `{}` bukan heksadesimal", baris.trim()))?;
        if n == 0 { return Ok(total); }
        let disalin = std::io::copy(&mut r.by_ref().take(n), w).map_err(|e| crate::host::galat_io(&e))?;
        if disalin != n { return Err("badan berpotongan terputus di tengah potongan".into()); }
        total += n;
        let mut crlf = [0u8; 2];
        r.read_exact(&mut crlf).map_err(|_| "badan berpotongan terputus sesudah potongan".to_string())?;
    }
}

fn gabung(asal: &Url, lokasi: &str) -> String {
    if lokasi.starts_with("http://") || lokasi.starts_with("https://") { return lokasi.to_string(); }
    let skema = if asal.https { "https" } else { "http" };
    if lokasi.starts_with("//") { return format!("{}:{}", skema, lokasi); }
    let dasar = format!("{}://{}", skema, kepala_host(asal));
    if lokasi.starts_with('/') { return format!("{}{}", dasar, lokasi); }
    let dir = asal.jalur.split('?').next().unwrap_or("/");
    let dir = &dir[..dir.rfind('/').map(|i| i + 1).unwrap_or(1)];
    format!("{}{}{}", dasar, dir, lokasi)
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
        let mut s = sambung(&u)?;
        kirim(&mut s, &u, "GET", &[], &[], agen)?;
        let mut r = BufReader::new(s);
        let mut kepala = Vec::new();
        loop {
            let mut b = String::new();
            let n = r.read_line(&mut b).map_err(|e| format!("membaca kepala jawaban: {}", crate::host::galat_io(&e)))?;
            if n == 0 { return Err(format!("{} menutup sambungan sebelum menjawab", u.host)); }
            let b = b.trim_end().to_string();
            if b.is_empty() { break; }
            kepala.push(b);
        }
        let status: u16 = kepala.first().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("jawaban {} bukan HTTP", u.host))?;
        let cari = |nama: &str| kepala.iter().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim().eq_ignore_ascii_case(nama)).then(|| v.trim().to_string())
        });
        if (300..400).contains(&status) {
            let lokasi = cari("location").ok_or_else(|| format!("pengalihan {} tanpa Location", status))?;
            sekarang = gabung(&u, &lokasi);
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
    fn url_diurai_dan_digabung() {
        let u = urai_url("https://a.b/c/d?x=1").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.jalur.as_str()), (true, "a.b", 443, "/c/d?x=1"));
        let u = urai_url("h:8080").unwrap();
        assert_eq!((u.https, u.port, u.jalur.as_str()), (false, 8080, "/"));
        assert!(urai_url("ftp://x").unwrap_err().contains("ftp"));
        let a = urai_url("http://h:81/a/b.txt").unwrap();
        assert_eq!(gabung(&a, "c.txt"), "http://h:81/a/c.txt");
        assert_eq!(gabung(&a, "/z"), "http://h:81/z");
        assert_eq!(gabung(&a, "https://q/r"), "https://q/r");
    }

    #[test]
    fn badan_berpotongan() {
        assert_eq!(urai_berpotongan(b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
        assert!(urai_berpotongan(b"zz\r\nab\r\n").unwrap_err().contains("bukan heksadesimal"));
        assert!(urai_berpotongan(b"4\r\nWi").unwrap_err().contains("terputus"));
    }
}
