// src/diag/kode.rs — kode error

// ─── Kode error
// ─────────────────────────────────────────────────────────────

pub mod codes {
    // ── parser ──
    /// Pemanggilan fungsi ditulis tanpa tanda kurung, dan argumennya
    /// ambigu sehingga tidak aman dipulihkan otomatis.
    pub const CALL_NEEDS_PARENS: &str = "E0101";
    /// Delimiter (`)`/`}`/`]`) tidak ditutup.
    pub const UNCLOSED_DELIMITER: &str = "E0102";
    /// Token yang tidak diharapkan di posisi ini.
    pub const UNEXPECTED_TOKEN: &str = "E0103";

    // ── peringatan ──
    /// Interpreter memulihkan pemanggilan tanpa kurung yang maknanya
    /// tidak ambigu (`wok "hai"` → `wok("hai")`).
    pub const RECOVERED_CALL: &str = "W0001";
}

#[cfg(test)]
mod tests {
    // `use super::*` di sini cuma membawa isi `kode`; nama-nama diagnostik
    // hidup satu tingkat di atas, dan `use` privat tidak ikut terwariskan.
    use crate::diag::*;
    use crate::lexer::{BytePos, Span, SourceFile};

    fn src(text: &str) -> SourceMap {
        SourceMap::with_file(SourceFile {
            id: 0,
            name: "main.jrg".to_string(),
            src: text.to_string(),
        })
    }

    fn span(lo: u32, hi: u32) -> Span {
        Span::new(BytePos(lo), BytePos(hi), 0)
    }

    #[test]
    fn renders_line_column_and_caret() {
        let map = src("package main\npub fn main() { wok(x) }\n");
        // `wok` ada di baris 2; cari offsetnya supaya test tidak rapuh
        // terhadap perubahan teks di atasnya.
        let lo = map.files[0].src.find("wok").unwrap() as u32;
        let d = Diagnostic::error("variabel `x` belum dikenal")
            .with_code("E0200")
            .with_span(span(lo, lo + 3))
            .with_label("di sini")
            .with_help("deklarasikan dulu: `let x = ...`");
        // Nada disuntikkan: yang diuji di sini TATA LETAKNYA, bukan
        // nadanya. Kalau dibiarkan ikut bawaan, mengubah nada bawaan akan
        // diam-diam mengubah apa yang test ini uji.
        let out = d.render_dengan_nada(&map, Nada::Sopan);

        assert!(out.starts_with("error[E0200]: variabel `x` belum dikenal"), "aktual:\n{}", out);
        assert!(out.contains("--> main.jrg:2:17"), "aktual:\n{}", out);
        assert!(out.contains("2 | pub fn main() { wok(x) }"), "aktual:\n{}", out);
        assert!(out.contains("^^^ di sini"), "aktual:\n{}", out);
        assert!(out.contains("= bantuan: deklarasikan dulu"), "aktual:\n{}", out);
    }

    /// Kolom dihitung per karakter, bukan per byte — kalau tidak, caret
    /// pada baris ber-Unicode akan meleset ke kanan.
    #[test]
    fn column_counts_characters_not_bytes() {
        let text = "let pesan = \"héllo\"\nwok(salah)\n";
        let map = src(text);
        let lo = text.find("salah").unwrap() as u32;
        let d = Diagnostic::error("tidak dikenal").with_span(span(lo, lo + 5));
        let out = d.render(&map);
        // baris 2, kolom 5 (w-o-k-'(' lalu 'salah')
        assert!(out.contains("--> main.jrg:2:5"), "aktual:\n{}", out);
    }

    /// Sebuah span yang membentang sampai baris berikutnya hanya ditandai
    /// pada baris pertamanya; caretnya tidak boleh meluber melewati akhir
    /// baris itu.
    #[test]
    fn multiline_span_is_clamped_to_first_line() {
        let text = "fn main() {\n    wok(1)\n";
        let map = src(text);
        let d = Diagnostic::error("blok belum ditutup").with_span(span(0, text.len() as u32));
        let out = d.render(&map);
        let caret_line = out.lines().find(|l| l.contains('^')).unwrap();
        assert!(caret_line.matches('^').count() <= "fn main() {".len(), "aktual:\n{}", out);
    }

    /// Tanpa span (atau file tidak ada di peta) pesan tetap tercetak utuh
    /// beserta help/note — tidak boleh ada jalur yang menelan diagnostik.
    #[test]
    fn renders_without_span() {
        let map = SourceMap::new();
        let d = Diagnostic::error("modul `foo` tidak ketemu")
            .with_help("cek nama file-nya")
            .with_note("dicari di direktori project");
        let out = d.render(&map);
        assert!(out.contains("error: modul `foo` tidak ketemu"));
        assert!(out.contains("= bantuan: cek nama file-nya"));
        assert!(out.contains("= catatan: dicari di direktori project"));
        assert!(!out.contains("-->"));
    }

    #[test]
    fn severity_labels_are_indonesian() {
        let map = SourceMap::new();
        assert!(Diagnostic::warning("hmm").render_dengan_nada(&map, Nada::Sopan).starts_with("peringatan: hmm"));
        assert!(Diagnostic::error("aduh").render_dengan_nada(&map, Nada::Sopan).starts_with("error: aduh"));
    }

    /// Nada galak MEMBUNGKUS, tidak MENGGANTI.
    #[test]
    fn nada_galak_menambah_baris_tanpa_mengubah_isi() {
        let text = "package main\npub fn main() {\n    wok(a[7])\n}\n";
        let map = src(text);
        let d = Diagnostic::error("index [7] di luar batas")
            .with_code("E0201")
            .with_span(span(34, 38))
            .with_help("index valid: 0..2");

        // Nada disuntikkan, bukan disetel global — lihat
        // `render_dengan_nada`. Test Rust jalan paralel.
        let sopan = d.render_dengan_nada(&map, Nada::Sopan);
        let galak = d.render_dengan_nada(&map, Nada::Galak);

        // Semua yang ada di nada sopan tetap ada, apa adanya.
        for potongan in ["error[E0201]: index [7] di luar batas", "-->", "^", "= bantuan: index valid: 0..2"] {
            assert!(galak.contains(potongan), "`{}` hilang di nada galak:\n{}", potongan, galak);
        }
        // Yang bertambah TEPAT satu baris, dan itu di paling atas.
        assert_eq!(
            galak.lines().count(), sopan.lines().count() + 1,
            "nada galak harus menambah tepat satu baris\nsopan:\n{}\ngalak:\n{}", sopan, galak
        );
        assert!(
            !galak.lines().next().unwrap().starts_with("error"),
            "baris omelan harus di ATAS baris judul:\n{}", galak
        );
        assert_eq!(galak.lines().skip(1).collect::<Vec<_>>(), sopan.lines().collect::<Vec<_>>());
    }

    /// Sapaan kompilasi dan sapaan runtime harus benar-benar berbeda —
    /// kalau sama, `Fase` tidak mengerjakan apa pun dan lebih baik dihapus.
    #[test]
    fn sapaan_kompilasi_beda_dari_sapaan_runtime() {
        for n in 1..12u32 {
            let k = omelan(Nada::Galak, Fase::Kompilasi, Some(5), 0xabc, n);
            let j = omelan(Nada::Galak, Fase::Jalan, Some(5), 0xabc, n);
            assert_ne!(k, j, "hitungan {}", n);
        }
        // Tanpa lokasi, sapaannya tidak boleh mengarang nomor baris.
        let tanpa = omelan(Nada::Galak, Fase::Jalan, None, 0xabc, 1);
        assert!(!tanpa.contains("baris"), "tidak boleh menyebut baris kalau lokasinya tidak diketahui: {}", tanpa);
    }

    /// Inti sistem gacha: acaknya DITURUNKAN, bukan diundi.
    #[test]
    fn sapaan_deterministik_untuk_error_dan_hitungan_yang_sama() {
        for n in 1..15u32 {
            let a = omelan(Nada::Galak, Fase::Kompilasi, Some(7), 0x1234, n);
            let b = omelan(Nada::Galak, Fase::Kompilasi, Some(7), 0x1234, n);
            assert_eq!(a, b, "hitungan {} tidak deterministik", n);
        }
        // Error yang BERBEDA tidak boleh selalu dapat kalimat yang sama —
        // kalau iya, benihnya tidak benar-benar dipakai.
        let beda: std::collections::HashSet<String> = (0..40u64)
            .map(|k| omelan(Nada::Galak, Fase::Kompilasi, Some(7), k * 2_654_435_761, 1))
            .collect();
        assert!(beda.len() > 1, "benih identitas error tidak berpengaruh sama sekali");
    }

    /// Naiknya hitungan harus benar-benar mengubah tingkatnya, dan
    /// tingkatnya harus naik searah — bukan lompat-lompat acak.
    #[test]
    fn tingkat_naik_seiring_pengulangan() {
        assert_eq!(tingkat_dari(1), Tingkat::Santai);
        assert_eq!(tingkat_dari(2), Tingkat::Menggoda);
        assert_eq!(tingkat_dari(3), Tingkat::Menggoda);
        assert_eq!(tingkat_dari(4), Tingkat::Gemas);
        assert_eq!(tingkat_dari(6), Tingkat::Gemas);
        assert_eq!(tingkat_dari(7), Tingkat::Ngamuk);
        assert_eq!(tingkat_dari(999), Tingkat::Ngamuk);

        // Dan kalimatnya memang ikut berganti saat tingkatnya berganti.
        let santai   = omelan(Nada::Galak, Fase::Jalan, Some(3), 0x55, 1);
        let menggoda = omelan(Nada::Galak, Fase::Jalan, Some(3), 0x55, 2);
        let gemas    = omelan(Nada::Galak, Fase::Jalan, Some(3), 0x55, 5);
        let ngamuk   = omelan(Nada::Galak, Fase::Jalan, Some(3), 0x55, 9);
        let semua = [&santai, &menggoda, &gemas, &ngamuk];
        for (i, a) in semua.iter().enumerate() {
            for b in semua.iter().skip(i + 1) {
                assert_ne!(a, b, "tingkat berbeda memberi kalimat yang sama:\n{}\n{}", a, b);
            }
        }
    }

    /// Semua nada, semua fase, semua tingkat, semua frasa — DIISI, lalu diperiksa
    /// hasilnya.
    #[test]
    fn tiap_frasa_terbaca_benar_sesudah_diisi() {
        // Kedua bentuk yang benar-benar dipakai `omelan`.
        const PENGGANTI: [&str; 2] = ["di baris 59", "di situ"];
        let mut jumlah = 0usize;
        let mut gagal: Vec<String> = Vec::new();

        for nada in [Nada::Galak, Nada::Judes, Nada::Hangat, Nada::Cool, Nada::Lawak] {
            let tabel = tabel_nada(nada).expect("nada bersapaan punya tabel");
            for (fase, pools) in [("kompilasi", &tabel.kompilasi), ("jalan", &tabel.jalan)] {
                for (t, pool) in pools.iter().enumerate() {
                    for frasa in pool.iter() {
                        jumlah += 1;
                        let mut catat = |apa: &str, hasil: &str| {
                            gagal.push(format!(
                                "{:?}/{}/T{}: {}\n      frasa : {:?}\n      hasil : {:?}",
                                nada, fase, t + 1, apa, frasa, hasil
                            ));
                        };
                        // Satu slot lokasi, tidak lebih — dua slot berarti
                        // nomor barisnya disebut dua kali.
                        if frasa.matches("{}").count() > 1 {
                            catat("lebih dari satu `{}`", frasa);
                        }
                        for isi in PENGGANTI {
                            let h = frasa.replace("{}", isi);
                            // Kata berulang yang MUNCUL karena penggantian.
                            let kata = |x: &str| -> Vec<String> {
                                x.replace(',', " ").replace('.', " ")
                                 .split_whitespace().map(|w| w.to_lowercase()).collect()
                            };
                            let hasil_kata = kata(&h);
                            let asli_kata = kata(&frasa.replace("{}", " \u{0} "));
                            for w in hasil_kata.windows(2) {
                                if w[0] == w[1]
                                    && !asli_kata.windows(2).any(|a| a[0] == w[0] && a[1] == w[1])
                                {
                                    catat(&format!("kata `{}` jadi dobel", w[0]), &h);
                                    break;
                                }
                            }
                            // "di situ" dua kali: frasa yang sudah menulis
                            // "di situ" jadi mengulang saat lokasinya tidak
                            // diketahui.
                            if h.matches("di situ").count() > 1 {
                                catat("`di situ` muncul dua kali", &h);
                            }
                            // Lebih dari satu em-dash dalam satu kalimat
                            // pendek selalu hasil tambalan, bukan tulisan.
                            if h.matches('—').count() > 1 {
                                catat("dua em-dash dalam satu kalimat", &h);
                            }
                            // Kerapian dasar: spasi dobel, spasi menggantung,
                            // dan tanda baca kembar.
                            if h.contains("  ") { catat("spasi dobel", &h); }
                            if h.trim() != h { catat("spasi di ujung", &h); }
                            for tanda in ["..", ",,", "!.", ".,", " ."] {
                                // `...` sengaja dipakai sebagai jeda, jadi
                                // yang dilarang cuma DUA titik — bukan tiga.
                                if tanda == ".." && h.contains("...") { continue; }
                                if h.contains(tanda) {
                                    catat(&format!("tanda baca kembar `{}`", tanda), &h);
                                }
                            }
                            // Diakhiri tanda baca, supaya tidak terbaca
                            // seperti kalimat yang terputus.
                            if !h.ends_with('.') && !h.ends_with('?') && !h.ends_with('!') {
                                catat("tidak diakhiri tanda baca", &h);
                            }
                        }
                    }
                }
            }
        }
        assert!(jumlah > 150, "tabel sapaan sepertinya tidak terbaca — cuma {} frasa", jumlah);
        assert!(
            gagal.is_empty(),
            "{} dari {} frasa salah baca sesudah diisi:\n  {}",
            gagal.len(), jumlah, gagal.join("\n  ")
        );
    }

    /// Frasa sapaan TIDAK boleh terdengar seperti templat.
    #[test]
    fn frasa_tidak_pakai_kalimat_templat() {
        // Frasa berkata-banyak dicocokkan sebagai substring; kata TUNGGAL harus
        // dicocokkan sebagai KATA UTUH.
        const TERLARANG_FRASA: [&str; 12] = [
            "mari kita", "perlu diperhatikan", "sebagai berikut",
            "dengan demikian", "oleh karena itu", "pastikan bahwa",
            "jangan khawatir", "semoga membantu", "terima kasih",
            "berikut ini", "dapat dilakukan", "sangat penting untuk",
        ];
        const TERLARANG_KATA: [&str; 4] = ["anda", "silakan", "mohon", "harap"];
        let mut gagal = Vec::new();
        for nada in [Nada::Galak, Nada::Judes, Nada::Hangat, Nada::Cool, Nada::Lawak] {
            let tabel = tabel_nada(nada).expect("nada bersapaan punya tabel");
            for (fase, pools) in [("kompilasi", &tabel.kompilasi), ("jalan", &tabel.jalan)] {
                for (t, pool) in pools.iter().enumerate() {
                    for frasa in pool.iter() {
                        let rendah = frasa.to_lowercase();
                        for buruk in TERLARANG_FRASA {
                            if rendah.contains(buruk) {
                                gagal.push(format!(
                                    "{:?}/{}/T{} memuat `{}`: {:?}",
                                    nada, fase, t + 1, buruk, frasa
                                ));
                            }
                        }
                        let kata: Vec<&str> = rendah
                            .split(|c: char| !c.is_alphanumeric() && c != '-')
                            .filter(|w| !w.is_empty())
                            .collect();
                        for buruk in TERLARANG_KATA {
                            if kata.iter().any(|w| *w == buruk) {
                                gagal.push(format!(
                                    "{:?}/{}/T{} memakai kata `{}`: {:?}",
                                    nada, fase, t + 1, buruk, frasa
                                ));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            gagal.is_empty(),
            "{} frasa terdengar seperti tulisan mesin:\n  {}",
            gagal.len(), gagal.join("\n  ")
        );
    }

    /// SEMUA nada harus serius pada kemunculan PERTAMA.
    #[test]
    fn kemunculan_pertama_selalu_serius_di_semua_nada() {
        const BERCANDA: [&str; 9] = [
            "!!!", "WOI", "HEH", "ASTAGA", "hadeh", "gemes", "ngomel", "kayak mantan", "wkwk",
        ];
        for nada in [Nada::Galak, Nada::Judes, Nada::Hangat, Nada::Cool, Nada::Lawak] {
            for fase in [Fase::Kompilasi, Fase::Jalan] {
                // Semua frasa di tingkat 1, bukan cuma yang kebetulan terpilih untuk satu
                // kunci — kalau salah satunya lolos, ia akan muncul untuk error tertentu …
                let tabel = tabel_nada(nada).expect("nada bersapaan punya tabel");
                let pool = match fase {
                    Fase::Kompilasi => tabel.kompilasi[0],
                    Fase::Jalan     => tabel.jalan[0],
                };
                assert!(!pool.is_empty(), "{:?}: tingkat pertama kosong", nada);
                for frasa in pool {
                    for b in BERCANDA {
                        assert!(
                            !frasa.contains(b),
                            "nada {:?} ({:?}) bercanda di kemunculan PERTAMA: {:?} memuat {:?}",
                            nada, fase, frasa, b
                        );
                    }
                    // Harus mengarahkan ke penjelasannya, bukan cuma
                    // mengeluh. Ini yang membedakan "serius" dari "datar".
                    let mengarah = ["bawah", "detail", "penjelasan", "keterangan",
                                    "sebab", "ini yang", "tunjukin"];
                    assert!(
                        mengarah.iter().any(|m| frasa.contains(m)),
                        "nada {:?} ({:?}) tidak mengarahkan pembaca ke penjelasan: {:?}",
                        nada, fase, frasa
                    );
                }
            }
        }
    }

    /// Kepribadian memang muncul, tapi baru dari pengulangan KEDUA.
    #[test]
    fn nada_berbeda_memberi_kalimat_berbeda_saat_berulang() {
        let mut terlihat = std::collections::HashSet::new();
        for nada in [Nada::Galak, Nada::Judes, Nada::Hangat, Nada::Cool, Nada::Lawak] {
            let k = omelan(nada, Fase::Jalan, Some(4), 0x99, 3);
            assert!(
                terlihat.insert(k.clone()),
                "nada {:?} memberi kalimat yang sama dengan nada lain: {:?}", nada, k
            );
        }
    }

    /// Badan diagnostik WAJIB identik di semua nada — yang berubah cuma satu
    /// baris sapaan di atasnya.
    #[test]
    fn badan_diagnostik_identik_di_semua_nada() {
        let sf = SourceFile { id: 0, name: "u.jrg".into(), src: "let a = 1
wok(b)
".into() };
        let map = SourceMap::with_file(sf);
        let d = Diagnostic::error("variabel `b` belum dikenal")
            .with_code("E0301")
            .with_span(Span { lo: BytePos(14), hi: BytePos(15), file_id: 0 })
            .with_help("deklarasikan dulu pakai `let b = ...`")
            .with_note("nama yang mirip: `a`")
            .saat_jalan();
        let badan = |n: Nada| -> String {
            let teks = d.render_dengan_nada(&map, n);
            // Buang baris sapaan (kalau ada) — sisanya harus sama persis.
            teks.lines().skip_while(|b| !b.starts_with("error")).collect::<Vec<_>>().join("\n")
        };
        let acuan = badan(Nada::Sopan);
        assert!(acuan.contains("bantuan:") && acuan.contains("catatan:") && acuan.contains('^'));
        for nada in [Nada::Galak, Nada::Judes, Nada::Hangat, Nada::Cool, Nada::Lawak] {
            assert_eq!(
                badan(nada), acuan,
                "badan diagnostik nada {:?} berbeda dari `sopan` — ada informasi yang \
                 cuma hidup di salah satu nada", nada
            );
        }
    }

    /// Nama nada bolak-balik lewat teks tanpa berubah.
    #[test]
    fn nama_nada_konsisten() {
        for n in Nada::SEMUA {
            let nada = Nada::dari_nama(n).unwrap_or_else(|| panic!("`{}` tidak dikenali", n));
            assert_eq!(nada.nama(), n);
        }
        assert!(Nada::dari_nama("HANGAT").is_some(), "nama nada harus tidak peka besar-kecil");
        assert!(Nada::dari_nama("hanget").is_none(), "salah ketik harus ditolak, bukan diterima");
    }

    /// Identitas error sengaja TIDAK memuat nomor baris: menyunting berkas
    /// menggeser baris, padahal kesalahannya itu-itu juga.
    #[test]
    fn identitas_error_tidak_bergantung_lokasi() {
        let a = kunci_error(Some("E0201"), "index [7] di luar batas");
        let b = kunci_error(Some("E0201"), "index [7] di luar batas");
        assert_eq!(a, b);
        assert_ne!(a, kunci_error(Some("E0201"), "index [9] di luar batas"));
        assert_ne!(a, kunci_error(None, "index [7] di luar batas"));
    }
}
