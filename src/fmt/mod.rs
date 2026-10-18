// src/fmt/mod.rs — src/fmt/mod.rs — formatter untuk `jrgm fmt`

use crate::lexer::{lex, lex_dengan_komentar, Komentar, SourceFile, Token, TokenKind};

/// Empat spasi. Bukan selera: diukur dari 40 berkas `.jrg` di pohon ini —
/// 2578 baris berindentasi kelipatan empat, nol baris memakai tab, nol baris
/// berindentasi kelipatan dua saja.
pub const LEBAR_INDENTASI: usize = 4;

// ─── Pintu masuk ───────────────────────────────────────────────────────────

/// Memformat satu berkas sumber.
pub fn format_sumber(sf: &SourceFile) -> Result<String, String> {
    let (tokens, komentar) = lex_dengan_komentar(sf)
        .map_err(|e| format!("tidak bisa dibaca sebagai JRG: {}", e.msg))?;

    let hasil = rakit(&sf.src, &tokens, &komentar);
    periksa_setara(sf, &hasil)?;
    Ok(hasil)
}

/// Menuntut hasil format melex jadi aliran token yang **identik** dengan
/// aslinya.
fn periksa_setara(asli: &SourceFile, hasil: &str) -> Result<(), String> {
    // Yang dibandingkan adalah masukan DENGAN akhiran berkas yang sudah
    // dinormalkan, bukan masukan mentah.
    let mut src_norm = asli.src.trim_end().to_string();
    src_norm.push('\n');
    let asli_norm = SourceFile { id: asli.id, name: asli.name.clone(), src: src_norm };

    let sebelum = lex(&asli_norm)
        .map_err(|e| format!("tidak bisa dibaca sebagai JRG: {}", e.msg))?;
    let sesudah_sf = SourceFile {
        id: asli.id,
        name: asli.name.clone(),
        src: hasil.to_string(),
    };
    let sesudah = lex(&sesudah_sf).map_err(|e| {
        format!(
            "BUG formatter: hasilnya sendiri tidak bisa dilex lagi ({}). \
             Berkas TIDAK diubah",
            e.msg
        )
    })?;

    let a: Vec<&TokenKind> = sebelum.iter().map(|t| &t.kind).collect();
    let b: Vec<&TokenKind> = sesudah.iter().map(|t| &t.kind).collect();
    if a == b {
        return Ok(());
    }

    // Sebutkan token pertama yang berbeda. Sebuah laporan yang cuma bilang
    // "tidak setara" memaksa orang berikutnya mengulang seluruh pencarian
    // yang barusan gagal.
    let n = a.len().min(b.len());
    let i = (0..n).find(|&i| a[i] != b[i]).unwrap_or(n);
    Err(format!(
        "BUG formatter: hasilnya tidak setara dengan masukan — token ke-{} \
         berubah dari {:?} jadi {:?} (asli {} token, hasil {} token). \
         Berkas TIDAK diubah",
        i,
        a.get(i),
        b.get(i),
        a.len(),
        b.len()
    ))
}

// ─── Perakitan ─────────────────────────────────────────────────────────────

/// Satu hal yang harus dicetak: sebuah token, atau sebuah komentar.
#[derive(Clone, Copy)]
struct Butir {
    lo: usize,
    hi: usize,
    kind: Option<usize>, // indeks ke `tokens`; None = komentar
    baris_penuh: bool,   // komentar `//`, yang selalu mengakhiri barisnya
}

fn rakit(src: &str, tokens: &[Token], komentar: &[Komentar]) -> String {
    let mut butir: Vec<Butir> = Vec::with_capacity(tokens.len() + komentar.len());

    let mut i = 0usize;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.kind == TokenKind::Eof {
            i += 1;
            continue;
        }
        let (lo, hi) = (t.span.lo.0 as usize, t.span.hi.0 as usize);

        // Titik koma yang DISISIPKAN otomatis tidak ada di sumber — rentangnya
        // menunjuk ke newline. Mencetaknya akan menambahkan `;` yang tidak
        // pernah ditulis siapa pun ke seluruh berkas.
        if t.kind == TokenKind::Semi && src.get(lo..hi) != Some(";") {
            i += 1;
            continue;
        }

        // Template literal disalin UTUH dari `«` sampai `»` pasangannya.
        if t.kind == TokenKind::TemplateLitStart {
            let mut dalam = 0usize;
            let mut j = i;
            while j < tokens.len() {
                match tokens[j].kind {
                    TokenKind::TemplateLitStart => dalam += 1,
                    TokenKind::TemplateLitEnd => {
                        dalam -= 1;
                        if dalam == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let akhir = tokens.get(j).map(|t| t.span.hi.0 as usize).unwrap_or(hi);
            butir.push(Butir { lo, hi: akhir, kind: Some(i), baris_penuh: false });
            i = j + 1;
            continue;
        }

        butir.push(Butir { lo, hi, kind: Some(i), baris_penuh: false });
        i += 1;
    }

    for k in komentar {
        butir.push(Butir {
            lo: k.span.lo.0 as usize,
            hi: k.span.hi.0 as usize,
            kind: None,
            baris_penuh: !k.blok,
        });
    }
    butir.sort_by_key(|b| b.lo);

    // ── cetak ──
    let mut out = String::with_capacity(src.len() + src.len() / 8);
    let mut depth: usize = 0;
    let mut prev_hi: Option<usize> = None;
    let mut prev_kind: Option<TokenKind> = None;
    let mut prev_baris_penuh = false;

    for b in &butir {
        // Butir yang SUDAH tercakup oleh sesuatu yang dipancarkan sebelumnya
        // dilewati.
        if prev_hi.map_or(false, |p| b.lo < p) {
            continue;
        }

        let teks = &src[b.lo..b.hi];
        let kind = b.kind.map(|i| tokens[i].kind.clone());

        // Penutup mundur SEBELUM barisnya diberi indentasi, supaya `}` sejajar
        // dengan pembuka bloknya dan bukan dengan isinya.
        if matches!(
            kind,
            Some(TokenKind::RBrace) | Some(TokenKind::RParen) | Some(TokenKind::RBracket)
        ) {
            depth = depth.saturating_sub(1);
        }

        let baris_baru = match prev_hi {
            None => false,
            Some(p) => {
                // Komentar `//` menelan sisa barisnya, jadi apa pun sesudahnya WAJIB turun
                // baris — kalau tidak, kode berikutnya ikut termakan komentar dan hilang.
                prev_baris_penuh || src[p..b.lo].contains('\n')
            }
        };

        if baris_baru {
            let kosong = src[prev_hi.unwrap()..b.lo].matches('\n').count();
            // Beberapa baris kosong beruntun mengerut jadi satu. Nol berarti
            // komentar `//` yang memaksa turun baris tanpa ada newline nyata.
            out.push('\n');
            if kosong >= 2 {
                out.push('\n');
            }
            for _ in 0..depth * LEBAR_INDENTASI {
                out.push(' ');
            }
        } else if let Some(p) = prev_hi {
            // `Pertahankan` melihat sumber aslinya: kalau di sana ada spasi, hasilnya
            // satu spasi; kalau tidak ada, tetap tidak ada.
            let ada_spasi_asli = !src[p..b.lo].is_empty();
            match aturan_celah(prev_kind.as_ref(), kind.as_ref(), b.kind.is_none()) {
                Celah::Paksa => out.push(' '),
                Celah::Larang => {}
                Celah::Pertahankan => {
                    if ada_spasi_asli {
                        out.push(' ');
                    }
                }
            }
        }

        out.push_str(teks);

        if matches!(
            kind,
            Some(TokenKind::LBrace) | Some(TokenKind::LParen) | Some(TokenKind::LBracket)
        ) {
            depth += 1;
        }

        prev_hi = Some(b.hi);
        prev_kind = kind;
        prev_baris_penuh = b.baris_penuh;
    }

    // Tepat satu newline di akhir berkas, tidak nol dan tidak dua.
    while out.ends_with('\n') || out.ends_with(' ') {
        out.pop();
    }
    out.push('\n');
    out
}

// ─── Aturan jarak ──────────────────────────────────────────────────────────

/// Apa yang harus terjadi pada celah antara dua token yang sebaris.
#[derive(PartialEq, Clone, Copy)]
enum Celah {
    Paksa,
    Larang,
    Pertahankan,
}

/// Aturan di sini sengaja cuma mencakup token yang artinya **tidak ambigu
/// dari tokennya sendiri**.
fn aturan_celah(prev: Option<&TokenKind>, cur: Option<&TokenKind>, cur_komentar: bool) -> Celah {
    use TokenKind::*;

    // Komentar di ujung baris kode selalu dipisahkan satu spasi.
    if cur_komentar {
        return Celah::Paksa;
    }
    let (p, c) = match (prev, cur) {
        (Some(p), Some(c)) => (p, c),
        // Kode tepat sesudah komentar blok: `/* a */x` dibaca susah.
        _ => return Celah::Paksa,
    };

    // `::` AWALAN (`f(a, ::len(x))`, `let f = ::abs`) memulai sebuah nama bawaan, bukan
    // menyambung jalur: celah di depannya diatur seperti di depan sebuah nama.
    if matches!(c, ColonColon) && !kepala_jalur(p) {
        return aturan_celah(prev, Some(&Ident(String::new())), false);
    }
    // Tidak pernah ada spasi di sekitar penyambung nama.
    if matches!(p, Dot | ColonColon | DotDotDot) || matches!(c, Dot | ColonColon) {
        return Celah::Larang;
    }
    // Tidak pernah ada spasi tepat di dalam kurung, atau sebelum pemisah.
    if matches!(p, LParen | LBracket) {
        return Celah::Larang;
    }
    if matches!(c, RParen | RBracket | Comma | Semi) {
        return Celah::Larang;
    }
    // `f(x)` dan `a[i]`: pemanggilan dan indeks menempel ke yang di depannya.
    // Literal juga, supaya `"abc"[0]` tidak merenggang.
    if matches!(c, LParen | LBracket)
        && matches!(
            p,
            Ident(_) | RParen | RBracket | KwSelf | StringLit(_) | RawStringLit(_)
        )
    {
        return Celah::Larang;
    }
    // Sesudah `,` selalu tepat satu spasi.
    if matches!(p, Comma) {
        return Celah::Paksa;
    }
    // `:` menempel ke kiri, berjarak satu ke kanan — `x: int`, `{a: 1}`, `struct
    // Anjing: Hewan`.
    if matches!(c, Colon) {
        return Celah::Larang;
    }
    if matches!(p, Colon) {
        return Celah::Paksa;
    }
    // `{}` kosong tetap rapat. Diukur: 4 di pohon ini, dan `Titik {}` adalah
    // bentuk sah yang berarti "semua field pakai nilai bawaan" — merenggangkan
    // jadi `Titik { }` membuatnya terbaca seperti blok kosong.
    if matches!(p, LBrace) && matches!(c, RBrace) {
        return Celah::Larang;
    }
    // `{` selalu didahului satu spasi (diukur: 186 lawan 3), dan isi yang
    // sebaris selalu berjarak satu spasi di kedua sisi (diukur: 128 dari 128,
    // nol yang rapat).
    if matches!(c, LBrace) || matches!(p, LBrace) || matches!(c, RBrace) {
        return Celah::Paksa;
    }
    // Operator yang TIDAK MUNGKIN jadi awalan, jadi aman selalu dikelilingi
    // spasi.
    if tak_mungkin_awalan(p) || tak_mungkin_awalan(c) {
        return Celah::Paksa;
    }
    // Kata kunci selalu berdiri sendiri: `let x`, `return v`, `if c`.
    if kata_kunci(p) || kata_kunci(c) {
        return Celah::Paksa;
    }

    Celah::Pertahankan
}

/// Sesudah token ini `::` MENYAMBUNG jalur (`a::b`, `chan::len`); di tempat lain ia
/// awalan `::nama`.
fn kepala_jalur(k: &TokenKind) -> bool {
    use TokenKind::*;
    matches!(k, Ident(_) | KwChan | KwSelf)
}

/// Token yang tidak pernah bisa muncul di posisi awalan sebuah ekspresi, jadi
/// aman untuk selalu dikelilingi spasi.
fn tak_mungkin_awalan(k: &TokenKind) -> bool {
    use TokenKind::*;
    matches!(
        k,
        Eq | EqEq | BangEq | LtEq | GtEq
            | AndAnd | OrOr
            | FatArrow | Arrow
            | PlusEq | MinusEq | StarEq | SlashEq | PercentEq
            | AndEq | OrEq | CaretEq | ShlEq | ShrEq
            | ColonEq | QuestionQuestion | Pipe | Dollar
            // Lolos kedua saringan di atas.
            | Plus | Slash | Percent | Caret | Shl | Shr
    )
}

/// Kata kunci selalu dipisahkan spasi dari apa pun di sebelahnya.
fn kata_kunci(k: &TokenKind) -> bool {
    use TokenKind::*;
    if matches!(k, KwSelf) {
        return false;
    }
    // Nama varian dibangkitkan oleh lexer dengan awalan `Kw`, jadi ini
    // diturunkan dari bentuk Debug-nya alih-alih ditulis ulang satu per satu.
    format!("{:?}", k).starts_with("Kw")
}
