// src/diag/nada.rs — tabel nada, tingkat eskalasi, dan hitungan pengulangan

use super::*;

/// Baris omelan untuk nada galak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tingkat { Santai, Menggoda, Gemas, Ngamuk }

pub(crate) fn tingkat_dari(hitungan: u32) -> Tingkat {
    match hitungan {
        0 | 1  => Tingkat::Santai,
        2..=3  => Tingkat::Menggoda,
        4..=6  => Tingkat::Gemas,
        _      => Tingkat::Ngamuk,
    }
}

/// Sapaan per (fase, tingkat).
pub(crate) struct TabelNada {
    pub(crate) kompilasi: [&'static [&'static str]; 4],
    pub(crate) jalan:     [&'static [&'static str]; 4],
}

// ── ATURAN YANG MENGIKAT SELURUH TABEL DI BAWAH ─────────────────────────

pub(crate) const GALAK: TabelNada = TabelNada {
    kompilasi: [
        &[
            "ada yang belum beres {}. gw tunjukin persisnya di bawah.",
            "belum bisa dikompilasi, nyangkut {}. keterangannya nyusul di bawah.",
            "nemu satu {}. detailnya gw taro di bawah.",
            "berhenti {} — ini yang bikin macet, lengkapnya di bawah.",
        ],
        &[
            "loh, {} lagi. gw baru merem bentar tadi.",
            "{} lagi? kita udah kayak naik angkot yang sama tiap pagi.",
            "eh, balik lagi {}. gw belum pindah dari situ dari kemarin.",
            "yang tadi kebaca ga? {} isinya masih sama huruf per huruf.",
            "{} nya belum bergeser sejengkal pun tuh.",
            "gw ga marah. gw cuma... {} lagi ya. oke.",
        ],
        &[
            "woi. {}. gw udah hafal nomor barisnya kayak nomor rumah sendiri.",
            "hadeh! {} lagi. gw ketik penjelasannya capek-capek loh di bawah.",
            "eh eh eh, {} MASIH? bawah tuh, bukan buat pajangan.",
            "seriusan, {}? gw mulai gemes sampe kipas laptop ikut bunyi.",
            "aduh, {}. gw tungguin dari tadi, isinya kayak kaset rusak.",
            "{} lagi... yaudah sini, gw bacain bareng, pelan-pelan, kata per kata.",
        ],
        &[
            "WOI, WOIIIIII!!! {}! gw teriak dari dalam CPU nih! benerin dulu sono!",
            "STOP. {}. gw ga bakal diem sampe ini kelar. baca. yang. bawah.",
            "YA AMPUN, {} LAGI?! gw sampe apal spasinya. sini gw eja.",
            "udah cukup. {}. gw ga marah, gw kecewa... bohong, gw marah.",
            "{}!!! gw udah kehabisan cara nunjukinnya. bawah tuh, udah gede-gede.",
            "HEH! {}. yaudah gw temenin sampe bener. tapi ya dibaca dong, sekali aja.",
        ],
    ],
    jalan: [
        &[
            "program berhenti {}. sebabnya gw tulis di bawah.",
            "kepentok {} waktu jalan. keterangannya di bawah.",
            "jalannya mentok {} — ini yang terjadi, lengkapnya di bawah.",
            "berhenti {}. gw tunjukin apa yang salah, di bawah.",
        ],
        &[
            "loh, {} lagi? gw kira tadi udah salaman.",
            "hmm, {} lagi. ada yang belum kesentuh kayaknya.",
            "kita ketemu lagi {}. gw mulai kenal baris ini secara pribadi.",
            "eh, {} balik. yakin udah dibenerin, atau cuma disimpen?",
            "{} lagi. dicek dong yang bawah, beneran dibaca bukan dilewatin.",
            "gw ga bosen kok. tapi ini {} lagi loh, ya.",
        ],
        &[
            "woi, {} MASIH. ini udah beberapa putaran.",
            "hadeh, {}. gw udah bilangin sampe suara gw serak.",
            "seriusan {} lagi? gw mulai gemes sendiri.",
            "eh! {}. yang di bawah itu dibaca, bukan dipandangin.",
            "{} lagi... gw udah ga tau harus pake huruf apa lagi.",
            "aduh, {}. ayo dong, tinggal seincik lagi juga kelar.",
        ],
        &[
            "HEH! LU, NGODING? {} lagi! kalo ga niat, mending gw yang ngoding deh.",
            "WOI, {}!!! gw teriak nih ya. BACA. YANG. BAWAH.",
            "ASTAGA, {} LAGI. gw sampe pengen ngambil alih keyboardnya.",
            "udah ya, {}. gw ga bakal berhenti ngomel sampe ini beres.",
            "{}!!! oke, fine. gw temenin. tapi kali ini beneran dibaca ya.",
            "STOP DULU. {}. duduk. tarik napas. sekarang baca yang bawah, pelan.",
        ],
    ],
};

pub(crate) const JUDES: TabelNada = TabelNada {
    kompilasi: [
        &[
            "ada masalah {}. detailnya di bawah.",
            "{} belum beres. bawah, baca.",
            "gagal dikompilasi, sumbernya {} — keterangannya di bawah.",
            "berhenti {}. penjelasannya di bawah.",
        ],
        &[
            "{} lagi. gw ga bakal tepuk tangan.",
            "sama persis kayak tadi. {}. ga ada yang geser.",
            "ini yang kedua, {}. gw itung loh.",
            "{}. yang tadi kebaca apa dilewatin?",
        ],
        &[
            "{}. lagi. dan lagi. gw ga bakal ganti kalimat.",
            "yang di bawah itu bukan wallpaper. {}.",
            "{} — udah beberapa kali, isinya ga bergerak seinci.",
            "gw taro penjelasannya di bawah bukan buat dilewatin. {}.",
        ],
        &[
            "{}. oke. gw diem. yang di bawah udah gw perjelas.",
            "terserah. {}. penjelasannya tetep gw taro di bawah.",
            "{}. gw ga capek. gw kan program.",
            "ini yang keberapa? {}. yaudah, gw tambahin bantuannya sekalian.",
        ],
    ],
    jalan: [
        &[
            "program berhenti {}. sebabnya di bawah.",
            "mati {}. detailnya di bawah.",
            "kepentok {}. ini penjelasannya.",
            "{} — jalannya berhenti. sebabnya di bawah.",
        ],
        &[
            "{} lagi. serius?",
            "berhenti di tempat yang sama. {}. persis.",
            "{}. kedua kalinya, kalo lu ga ngitung.",
            "kayaknya yang tadi ga kebaca ya. {}.",
        ],
        &[
            "{}. tempat sama, error sama, hasil sama.",
            "gw hafal yang ini. {}. sampe koma-komanya.",
            "{} — beberapa kali sudah. tidak ada perkembangan.",
            "yaudah. {}. gw taro lagi penjelasannya di bawah.",
        ],
        &[
            "{}. gw berhenti ngomel. bacanya aja yang bawah.",
            "sudah sekian kali, {}. gw tambahin petunjuk sekalian.",
            "{}. gw ga bosen, tapi lu harusnya iya.",
            "oke. {}. gw bacain bawahnya, lu simak.",
        ],
    ],
};

pub(crate) const HANGAT: TabelNada = TabelNada {
    kompilasi: [
        &[
            "ada yang perlu dibenerin {}. tenang, keterangannya lengkap di bawah.",
            "belum bisa jalan karena {}. yuk lihat detailnya di bawah.",
            "ketemu satu masalah {}. penjelasannya ada di bawah ya.",
            "berhenti {} — bukan hal besar. ini detailnya.",
        ],
        &[
            "muncul lagi {}. gapapa, gw juga sering kelewat baris yang sama.",
            "yang ini balik lagi ya, {}. coba baca ulang yang di bawah.",
            "hmm, {} lagi. mungkin ada yang kelewat sedikit. sini gw temenin.",
            "sabar ya, {} masih ada. ini bagian normal dari belajar, beneran.",
        ],
        &[
            "masih ada masalah {}. serius, ini wajar — gw bantu lagi ya.",
            "udah beberapa kali {}. jangan nyerah, kita lihat lagi bareng.",
            "masih {}. tarik napas dulu. seriusan, tarik napas dulu.",
            "kalau {} terus, mungkin tulisan gw yang belum jelas. gw ulang.",
        ],
        &[
            "{} sudah sering. gw tambahin petunjuk supaya lebih terang.",
            "kayaknya gw yang kurang jelas nulisnya, bukan lu yang lambat. {} lagi.",
            "yang ini memang bandel, {}. jangan nyerah ya, deket kok.",
            "gw temenin sampai beres. {}. ga usah buru-buru, ga ada yang ngejar.",
        ],
    ],
    jalan: [
        &[
            "programnya berhenti {}. tenang, ini sebabnya.",
            "sampai {} lalu mentok. penjelasannya di bawah ya.",
            "berhenti {} — bukan gagal, cuma belum lolos. ini detailnya.",
            "ada yang bikin nyangkut {}. sini gw tunjukin.",
        ],
        &[
            "mentok lagi {}. gapapa, kita lihat sekali lagi.",
            "yang ini muncul lagi ya, {}. coba cek yang di bawah.",
            "berhenti di tempat yang sama, {}. mungkin ada yang kelewat.",
            "sabar, {} lagi. kita betulin pelan-pelan aja.",
        ],
        &[
            "masih terjadi {}. tetap tenang ya, gw bantu lagi.",
            "udah beberapa kali {}. yuk baca ulang bareng, dari atas.",
            "masih mentok {}. ini bukan tanda lu ga bisa — cuma belum ketemu.",
            "kalau {} terus, mungkin ada anggapan yang perlu dicek ulang.",
        ],
        &[
            "sudah sering banget {}. gw tambahin petunjuk supaya lebih terang.",
            "gw rasa penjelasannya yang kurang jelas, {}. ini tambahannya.",
            "yang ini memang keras kepala, {}. jangan nyerah ya, hampir.",
            "gw temenin terus sampai kelar, {}. beneran.",
        ],
    ],
};

pub(crate) const COOL: TabelNada = TabelNada {
    kompilasi: [
        &[
            "{} — ada yang belum pas. detail di bawah.",
            "kompilasi berhenti {}. rinciannya di bawah.",
            "satu masalah {}. selengkapnya di bawah.",
            "belum lolos {}. ini keterangannya.",
        ],
        &[
            "{}. persis seperti tadi.",
            "{} — kedua kali.",
            "berulang {}. tidak ada yang bergeser.",
            "{}. sama. lanjut.",
        ],
        &[
            "{}. beberapa kali sekarang.",
            "masih {}. keadaannya tidak berubah.",
            "{} — polanya rapi sekali, sayangnya.",
            "berulang lagi {}. mungkin anggapannya yang perlu dicek.",
        ],
        &[
            "{}. sudah sering. gw tambahkan petunjuk di bawah.",
            "berulang terus {}. arahan tambahan ada di bawah.",
            "{} — sepertinya keterangan gw kurang. ini tambahannya.",
            "masih {}. gw coba jelaskan dari sudut lain.",
        ],
    ],
    jalan: [
        &[
            "berhenti {}. rinciannya di bawah.",
            "{} — eksekusi terhenti. detailnya di bawah.",
            "mentok {}. ini keterangannya.",
            "jalan sampai {}, lalu berhenti. keterangannya di bawah.",
        ],
        &[
            "{}. lagi.",
            "titik yang sama, {}.",
            "{} — berulang.",
            "berhenti di titik itu lagi, {}.",
        ],
        &[
            "{}. konsisten berulang.",
            "masih {}. kondisinya tetap.",
            "{} — beberapa kali sekarang.",
            "berulang di titik yang sama, {}. rapi.",
        ],
        &[
            "{}. gw tambahkan petunjuk di bawah.",
            "berulang terus, {}. ini arahan tambahannya.",
            "{} — coba dari sudut lain, ada di bawah.",
            "masih sama, {}. petunjuk tambahan menyusul.",
        ],
    ],
};

pub(crate) const LAWAK: TabelNada = TabelNada {
    kompilasi: [
        &[
            "oke, ada error {}. seriusan dulu — detailnya di bawah.",
            "ada yang salah {}. becandanya nanti — detailnya dulu, di bawah.",
            "nemu masalah {}. ini penjelasannya, baca dulu.",
            "belum bisa jalan, nyangkut {}. detail di bawah.",
        ],
        &[
            "{} lagi. wah, kita jadi sering ketemu di halte yang sama.",
            "eh {}. gw kira tadi udah dadah-dadahan.",
            "{}? baris itu kayaknya punya wifi sendiri.",
            "dua kali {}. mulai kayak langganan koran.",
        ],
        &[
            "{} lagi?! ini error apa alarm subuh, bunyi tiap hari.",
            "gw sama baris itu udah tuker resep rendang. {}.",
            "{}. gw itungin ya, ini udah... banyak. bawah tuh, dibaca.",
            "{} lagi. gw curiga baris itu punya agenda politik sendiri.",
        ],
        &[
            "{}!!! oke gw nyerah becanda. petunjuk beneran gw taro di bawah.",
            "gw sama {} udah tuker nomor telepon. yaudah, ini bantuannya.",
            "{}. lawakan gw habis, jadi gw kasih petunjuk tambahan aja.",
            "{} lagi. gw serius sekarang: yang di bawah udah gw perjelas.",
        ],
    ],
    jalan: [
        &[
            "program berhenti {}. seriusan dulu — ini sebabnya.",
            "mentok {}. gw serius dulu ya — penjelasannya di bawah.",
            "berhenti {}. detailnya di bawah, baca dulu.",
            "jalannya nyangkut {}. ini keterangannya.",
        ],
        &[
            "{} lagi. programnya kayak lagi jogging bolak-balik.",
            "eh, mentok {} lagi. tempat favorit ya?",
            "{}? gw kira tadi udah lewat pos itu.",
            "dua kali berhenti {}. mulai kebiasaan nih.",
        ],
        &[
            "{} lagi. programnya lebih konsisten daripada niat gw olahraga.",
            "gw sama baris itu udah sahabat pena. {}.",
            "{}. ini bukan bug, ini tradisi keluarga. bawah tuh, dibaca.",
            "{} terus. gw sampe apal nomor barisnya kayak nomor plat.",
        ],
        &[
            "{}!!! oke serius. petunjuk beneran gw taro di bawah.",
            "bahan lawakan gw habis, {}. ini bantuan tambahannya.",
            "{}. yaudah, gw berhenti becanda dan mulai bantuin beneran.",
            "{} lagi. gw perjelas dari sudut lain di bawah.",
        ],
    ],
};

pub(crate) fn tabel_nada(n: Nada) -> Option<&'static TabelNada> {
    match n {
        Nada::Sopan => None,
        Nada::Galak => Some(&GALAK),
        Nada::Judes => Some(&JUDES),
        Nada::Hangat => Some(&HANGAT),
        Nada::Cool => Some(&COOL),
        Nada::Lawak => Some(&LAWAK),
    }
}

/// Baris bantuan yang HANYA muncul di tingkat `Ngamuk`.
pub(crate) fn bantuan_tambahan(kunci: u64) -> &'static str {
    const SARAN: &[&str] = &[
        "beneran nih: baca baris `bantuan:` di atas pelan-pelan, satu kata satu kata. \
         biasanya jawabannya udah ada di situ.",
        "coba tunjukin kodenya ke orang lain, atau baca keras-keras. mata kedua \
         (atau kuping sendiri) sering langsung nemu yang matanya sendiri lewatin.",
        "kalau masih buntu: hapus dulu baris yang error, pastiin sisanya jalan, \
         baru tulis ulang pelan-pelan. lebih cepat daripada nebak-nebak.",
        "tanda `^` di bawah nunjuk TEPAT ke bagian yang bermasalah. mulai dari situ, \
         jangan dari awal baris.",
    ];
    SARAN[(kunci % SARAN.len() as u64) as usize]
}

/// Satu baris sapaan untuk nada galak.
pub(crate) fn fnv1a(data: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Identitas sebuah error, untuk menghitung pengulangan.
pub(crate) fn kunci_error(kode: Option<&str>, pesan: &str) -> u64 {
    fnv1a(&format!("{}|{}", kode.unwrap_or(""), pesan))
}

/// Sapaan lengkap: tingkat dipilih dari hitungan, varian di-gacha dari
/// (identitas error, hitungan).
pub(crate) fn omelan(nada: Nada, fase: Fase, baris: Option<usize>, kunci: u64, hitungan: u32) -> String {
    let di_baris = match baris {
        Some(n) => format!("di baris {}", n),
        // Tanpa lokasi, tidak mengarang nomor — cukup menunjuk kabur.
        None => "di situ".to_string(),
    };
    let Some(tabel) = tabel_nada(nada) else { return String::new() };
    let per_fase = match fase {
        Fase::Kompilasi => &tabel.kompilasi,
        Fase::Jalan     => &tabel.jalan,
    };
    let pool = per_fase[match tingkat_dari(hitungan) {
        Tingkat::Santai   => 0,
        Tingkat::Menggoda => 1,
        Tingkat::Gemas    => 2,
        Tingkat::Ngamuk   => 3,
    }];
    // Variasinya DITURUNKAN dari (identitas error, hitungan, nada), bukan
    // diundi.
    let undi = fnv1a(&format!("{:x}#{}#{}", kunci, hitungan, nada.nama())) as usize;
    pool[undi % pool.len()].replace("{}", &di_baris)
}

// ─── Hitungan pengulangan ───────────────────────────────────────────────────

/// Berapa kali error yang sama sudah muncul, dalam SATU proses ini.
pub(crate) static HITUNGAN_PROSES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, u32>>> =
    std::sync::OnceLock::new();

/// Sesudah selang ini, hitungan dianggap mulai dari nol lagi.
pub(crate) const LURUH_DETIK: u64 = 6 * 60 * 60;

/// Berkas kecil tempat hitungan antar-run disimpan.
pub(crate) fn berkas_hitungan() -> Option<std::path::PathBuf> {
    if !ingat_antar_run() { return None; }
    let dasar = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(dasar.join("jrgc").join("ulangan"))
}

/// Bolehkah hitungan pengulangan diingat ANTAR-RUN?
pub(crate) fn ingat_antar_run() -> bool {
    if matches!(std::env::var("JRG_ULANGAN"), Ok(v) if v == "1") { return true; }
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// Naikkan hitungan untuk `kunci`, lalu kembalikan nilainya sesudah naik.
pub(crate) fn naikkan_hitungan(kunci: u64) -> u32 {
    let dalam_proses = {
        let peta = HITUNGAN_PROSES.get_or_init(|| std::sync::Mutex::new(Default::default()));
        let mut g = peta.lock().unwrap_or_else(|p| p.into_inner());
        let e = g.entry(kunci).or_insert(0);
        *e += 1;
        *e
    };
    // Yang antar-run cuma dihitung sekali per proses — kalau tidak, satu berkas
    // dengan sepuluh error parse akan menaikkan hitungan tersimpan sepuluh kali …
    if dalam_proses == 1 {
        if let Some(tersimpan) = naikkan_tersimpan(kunci) {
            return tersimpan;
        }
    }
    dalam_proses
}

pub(crate) fn sekarang_detik() -> u64 {
    crate::host::sejak_epoch()
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Baca-naikkan-tulis berkas hitungan. `None` kalau apa pun gagal.
pub(crate) fn naikkan_tersimpan(kunci: u64) -> Option<u32> {
    let path = berkas_hitungan()?;
    let sekarang = sekarang_detik();

    // (kunci, hitungan, terakhir)
    let mut entri: Vec<(u64, u32, u64)> = std::fs::read_to_string(&path)
        .ok()
        .map(|isi| {
            isi.lines().filter_map(|b| {
                let mut it = b.split_whitespace();
                let k = u64::from_str_radix(it.next()?, 16).ok()?;
                let n: u32 = it.next()?.parse().ok()?;
                let t: u64 = it.next()?.parse().ok()?;
                Some((k, n, t))
            }).collect()
        })
        .unwrap_or_default();

    let mut hasil = 1u32;
    match entri.iter_mut().find(|(k, _, _)| *k == kunci) {
        Some((_, n, t)) => {
            // Peluruhan: sudah lama tidak ketemu error ini, mulai lagi dari awal.
            *n = if sekarang.saturating_sub(*t) > LURUH_DETIK { 1 } else { n.saturating_add(1) };
            *t = sekarang;
            hasil = *n;
        }
        None => entri.push((kunci, 1, sekarang)),
    }

    // Buang yang sudah luruh, lalu batasi ukurannya. Berkas ini hiasan;
    // ia tidak boleh tumbuh tanpa batas di direktori cache orang.
    entri.retain(|(_, _, t)| sekarang.saturating_sub(*t) <= LURUH_DETIK);
    if entri.len() > 200 {
        entri.sort_unstable_by_key(|(_, _, t)| std::cmp::Reverse(*t));
        entri.truncate(200);
    }

    let isi: String = entri.iter()
        .map(|(k, n, t)| format!("{:x} {} {}\n", k, n, t))
        .collect();
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, isi).ok()?;
    Some(hasil)
}
