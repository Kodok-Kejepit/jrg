//! REPL — mengetik JRG sepotong-sepotong dan langsung melihat jawabannya.


use crate::interpreter::{Env, Interpreter, Value};
use crate::lexer::{lex, SourceFile};
use crate::parser::{parse, Item};

/// Keadaan satu sesi REPL.
struct Sesi {
    interp: Interpreter,
    env: Env,
    /// Teks tiap masukan yang BERHASIL, plus jenisnya. Jenis itu menentukan
    /// ke mana ia ditulis oleh `:simpan` — dan untuk EKSPRESI, apakah ia perlu
    /// dibungkus `wok(...)`. Lihat `sebagai_program`.
    riwayat: Vec<(String, Jenis)>,
    /// Teks tiap DEKLARASI yang diterima, dipakai sebagai pendahuluan waktu
    /// masukan berikutnya diparse.
    deklarasi: Vec<String>,
}

/// Jenis sebuah masukan yang sudah diterima, untuk `:simpan`.
#[derive(Clone, Copy, PartialEq)]
enum Jenis {
    /// `fn`/`struct`/`enum`/`trait`/`impl`/`extern`/`import` — top level.
    Deklarasi,
    /// Menghasilkan nilai yang REPL cetak. Di dalam berkas, sebuah ekspresi
    /// telanjang di tengah fungsi tidak mencetak apa pun dan tidak melakukan
    /// apa pun, jadi ia dibungkus `wok(...)`.
    Ekspresi,
    /// Sisanya: `let`, penugasan, `wok(...)` yang sudah mencetak sendiri.
    Pernyataan,
}

/// Hasil satu masukan.
enum Hasil {
    /// Sebuah nilai yang perlu dicetak.
    Nilai(Value),
    /// Berhasil, tapi tidak menghasilkan apa-apa untuk dilihat
    /// (`wok(...)` yang sudah mencetak sendiri, penugasan).
    Sunyi,
    /// Sebuah deklarasi. Dibedakan dari `Sunyi` karena `:simpan` harus
    /// menaruhnya di top level, bukan di dalam `main`.
    Deklarasi,
}

const SAPAAN: &str = "\
JRG REPL. Ketik ekspresi lalu Enter; `:bantuan` untuk perintah, Ctrl-D keluar.
Di sini sebuah nama BOLEH didefinisikan ulang — di dalam berkas tidak boleh,
dan `:simpan` akan memberi tahu kalau sesi ini memuatnya.";

const BANTUAN: &str = "  :bantuan            tampilkan ini
  :keluar             keluar (sama dengan Ctrl-D)
  :simpan <berkas>    tulis sesi ini sebagai program .jrg yang bisa dijalankan
  :riwayat            tampilkan masukan yang sudah diterima
  :hapus              lupakan semua binding dan mulai dari awal
  :tipe <ekspresi>    tampilkan typeof-nya tanpa mencetak nilainya
  :cari <kata>        cari builtin yang namanya memuat <kata>
  :doc <nama>         penjelasan sebuah builtin
  :daftar [namespace] daftar isi sebuah namespace, atau semua namespace

Penyuntingan baris: panah kiri/kanan pindah kursor, atas/bawah menelusuri
riwayat sesi ini, Home/Ctrl-A dan End/Ctrl-E ke ujung, Ctrl-W hapus satu kata
ke kiri, Ctrl-U hapus dari awal baris sampai kursor, Ctrl-K hapus dari kursor
sampai akhir, Ctrl-L bersihkan layar, Ctrl-C batalkan baris (sesi tetap
hidup), Ctrl-D keluar. Di dalam pipa penyuntingan ini mati sendiri, karena di
sana ia tidak berarti.

Blok yang belum tertutup dilanjutkan otomatis: ketik `fn f() {` lalu Enter,
promptnya berubah jadi `  ...` sampai kurungnya seimbang. Baris kosong di
tengah blok membatalkan masukannya.";

/// Pesan sebuah sinyal untuk dicetak sesudah `error: ` — `Display` milik `Signal` menambah
/// awalan `runtime error:`, jadi REPL dulu mencetak "error: runtime error: …".
fn pesan_sinyal(sig: crate::interpreter::Signal) -> String {
    match sig {
        crate::interpreter::Signal::RuntimeError(e) => e.message,
        lain => format!("{}", lain),
    }
}

impl Sesi {
    fn baru() -> Self {
        let mut interp = Interpreter::new();
        // `import "bantu"` mencari di direktori kerja — dulu daftarnya KOSONG, jadi modul di
        // sebelahnya pun dijawab "tidak ketemu. Yang sudah dicari: []".
        interp.search_paths = vec![std::path::PathBuf::from(".")];
        // Env REPL memakai globals yang sama dengan interpreternya, lalu
        // membuka SATU scope yang hidup sepanjang sesi. Tanpa scope ini
        // `let` di masukan pertama tidak punya tempat tinggal.
        let mut env = Env::with_globals(interp.env.globals.clone());
        env.push_scope();
        // `defer` di masukan REPL perlu frame, sama seperti `main` di
        // `run_file` — tanpa ini `defer` menjawab "cuma bisa di dalam fungsi"
        // untuk sesuatu yang jelas sedang dijalankan.
        interp.defer_frames.push(Vec::new());
        Sesi {
            interp,
            env,
            riwayat: Vec::new(),
            deklarasi: Vec::new(),
        }
    }

    /// Apakah masukannya sudah lengkap secara kurung?
    fn lengkap(teks: &str) -> bool {
        let sf = SourceFile { id: 0, name: "<repl>".into(), src: teks.to_string() };
        let Ok(tok) = lex(&sf) else { return false };
        use crate::lexer::TokenKind as T;
        let mut dalam: i32 = 0;
        for t in &tok {
            match t.kind {
                T::LBrace | T::LParen | T::LBracket => dalam += 1,
                T::RBrace | T::RParen | T::RBracket => dalam -= 1,
                _ => {}
            }
        }
        dalam <= 0
    }


    /// Sesi ini sebagai satu program `.jrg` yang utuh.
    fn sebagai_program(&self) -> String {
        let badan: Vec<String> = self
            .riwayat
            .iter()
            .filter(|(_, j)| *j != Jenis::Deklarasi)
            .map(|(r, j)| {
                // Sebuah EKSPRESI dibungkus `wok(...)`, karena REPL mencetak nilainya dan
                // sebuah berkas tidak.
                let t = if *j == Jenis::Ekspresi {
                    format!("wok({})", r)
                } else {
                    r.clone()
                };
                t.lines().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n")
            })
            .collect();
        format!(
            "package main\n\n{}{}pub fn main() {{\n{}\n}}\n",
            self.deklarasi.join("\n\n"),
            if self.deklarasi.is_empty() { "" } else { "\n\n" },
            badan.join("\n"))
    }

    /// Jalankan satu masukan.
    fn jalankan(&mut self, teks: &str) -> Result<Hasil, String> {
        if let Some(h) = self.coba_deklarasi(teks)? {
            return Ok(h);
        }
        self.coba_pernyataan(teks)
    }

    /// Coba baca sebagai deklarasi top-level. `Ok(None)` berarti "ini bukan
    /// deklarasi, coba jalur lain" — bukan "gagal".
    fn coba_deklarasi(&mut self, teks: &str) -> Result<Option<Hasil>, String> {
        // Diparse BERSAMA deklarasi sebelumnya, bukan sendirian.
        let nama_baru = Self::nama_item(teks);
        let sisa: Vec<&String> = self
            .deklarasi
            .iter()
            .filter(|d| {
                let n = Self::nama_item(d);
                n.is_empty() || !n.iter().any(|x| nama_baru.contains(x))
            })
            .collect();
        let pendahuluan: String =
            sisa.iter().map(|d| format!("{}\n", d)).collect::<Vec<_>>().join("");
        let src = format!("package main\n{}{}\n", pendahuluan, teks);
        let sf = SourceFile { id: 0, name: "<repl>".into(), src };
        let Ok(tok) = lex(&sf) else { return Ok(None) };
        let (file, galat) = parse(tok, false);
        if !galat.is_empty() {
            return Ok(None);
        }
        // Hanya bentuk yang memang HARUS top-level. `Const`/`Var` sengaja
        // TIDAK di sini: di REPL `let x = 1` harus bisa diketik ulang, dan
        // sebagai item ia akan kena aturan nama-ganda Fase NAMA.
        let deklarasi = !file.items.is_empty()
            && file.items.iter().all(|i| matches!(i,
                Item::Fn(_) | Item::Struct(_) | Item::Enum(_)
                | Item::Trait(_) | Item::Impl(_) | Item::Extern(_)));
        if !deklarasi && file.imports.is_empty() {
            return Ok(None);
        }
        for imp in &file.imports {
            self.interp
                .load_module(&imp.path, imp.alias.as_deref())
                .map_err(|e| e.message.clone())?;
        }
        // Seluruh berkasnya di-hoist, termasuk deklarasi lama. Itu idempoten
        // (`define` menimpa) dan jauh lebih sederhana daripada memilah item
        // mana yang baru — dan memilah adalah tempat yang bagus untuk salah.
        let file = Self::lewat_ruang(&self.interp, file)?;
        self.interp.hoist_top_level(&file).map_err(|e| e.message.clone())?;
        let sisa: Vec<String> = sisa.into_iter().cloned().collect();
        self.deklarasi = sisa;
        self.deklarasi.push(teks.to_string());
        Ok(Some(Hasil::Deklarasi))
    }

    /// Nama tipe ditulis ulang seperti berkas utama (`crate::ruang_tipe`): tipe milik REPL tidak
    /// menimpa tipe bernama sama milik modul yang di-`import` lebih dulu.
    fn lewat_ruang(interp: &Interpreter, mut file: crate::parser::ParsedFile) -> Result<crate::parser::ParsedFile, String> {
        use crate::ruang_tipe::UNIT_UTAMA;
        let mut r = interp.ruang.lock().unwrap_or_else(|p| p.into_inner());
        r.klaim(UNIT_UTAMA, UNIT_UTAMA, &file);
        r.olah(UNIT_UTAMA, UNIT_UTAMA, &mut file, &interp.impor_tipe).map_err(|(pesan, _)| pesan)?;
        Ok(file)
    }

    /// Nama top-level yang didefinisikan sebuah teks deklarasi.
    fn nama_item(teks: &str) -> Vec<String> {
        let sf = SourceFile {
            id: 0,
            name: "<repl>".into(),
            src: format!("package main\n{}\n", teks),
        };
        let Ok(tok) = lex(&sf) else { return Vec::new() };
        let (file, _) = parse(tok, true);
        file.items
            .iter()
            .filter_map(|i| match i {
                Item::Fn(d) => Some(format!("fn {}", d.name)),
                Item::Struct(d) => Some(format!("tipe {}", d.name)),
                Item::Enum(d) => Some(format!("tipe {}", d.name)),
                Item::Trait(d) => Some(format!("tipe {}", d.name)),
                Item::Impl(d) => Some(format!("impl {:?}", d.self_type)),
                _ => None,
            })
            .collect()
    }

    /// Jalankan sebagai pernyataan/ekspresi di dalam env yang menetap.
    fn coba_pernyataan(&mut self, teks: &str) -> Result<Hasil, String> {
        let src = format!("package main\npub fn __repl__() {{\n{}\n}}\n", teks);
        let sf = SourceFile { id: 0, name: "<repl>".into(), src };
        let tok = lex(&sf).map_err(|e| e.msg.clone())?;
        let (file, galat) = parse(tok, false);
        if let Some(g) = galat.first() {
            return Err(g.msg.clone());
        }
        let file = Self::lewat_ruang(&self.interp, file)?;
        let Some(Item::Fn(f)) = file.items.first() else {
            return Err("masukan ini tidak bisa dibaca sebagai pernyataan".into());
        };
        let crate::parser::FnBody::Block(body) = &*f.body else {
            return Err("masukan ini tidak bisa dibaca sebagai pernyataan".into());
        };

        for s in &body.stmts {
            self.interp
                .exec_stmt(s, &mut self.env)
                .map_err(pesan_sinyal)?;
        }
        match &body.tail {
            Some(e) => {
                let v = self
                    .interp
                    .eval_expr(e, &mut self.env)
                    .map_err(pesan_sinyal)?;
                Ok(match v {
                    // `()` adalah nilai yang benar untuk `wok(...)` dan
                    // untuk penugasan; mencetaknya cuma jadi derau.
                    Value::Unit => Hasil::Sunyi,
                    lain => Hasil::Nilai(lain),
                })
            }
            None => Ok(Hasil::Sunyi),
        }
    }
}

/// Perintah `:...`. Mengembalikan `false` kalau sesi harus berakhir.
fn perintah(s: &mut Sesi, baris: &str) -> bool {
    let mut bagian = baris.trim().splitn(2, char::is_whitespace);
    let cmd = bagian.next().unwrap_or("");
    let arg = bagian.next().unwrap_or("").trim();
    match cmd {
        ":keluar" | ":q" => return false,
        ":bantuan" | ":h" => println!("{}", BANTUAN),
        ":riwayat" => {
            for (i, (r, _)) in s.riwayat.iter().enumerate() {
                println!("{:>3}  {}", i + 1, r.replace('\n', "\n     "));
            }
        }
        ":hapus" => {
            *s = Sesi::baru();
            println!("sesi dikosongkan");
        }
        ":tipe" => {
            if arg.is_empty() {
                println!("pakai: :tipe <ekspresi>");
            } else {
                match s.jalankan(&format!("typeof({})", arg)) {
                    Ok(Hasil::Nilai(v)) => println!("{}", v),
                    Ok(_) => println!("(tidak menghasilkan nilai)"),
                    Err(e) => println!("error: {}", e),
                }
            }
        }
        ":cari" => {
            if arg.is_empty() {
                println!("pakai: :cari <kata>");
                return true;
            }
            let hasil = crate::bantu::cari(arg);
            if hasil.is_empty() {
                println!("tidak ada builtin yang namanya memuat `{}`", arg);
            } else {
                // Dibatasi supaya `:cari a` tidak menumpahkan ratusan baris
                // dan mendorong prompt-nya keluar layar. Jumlah penuhnya tetap
                // disebut, jadi yang dipotong tidak disembunyikan.
                let n = hasil.len();
                for h in hasil.iter().take(40) {
                    println!("  {}", h);
                }
                if n > 40 {
                    println!("  … dan {} lagi (persempit katanya)", n - 40);
                }
                println!("{} nama", n);
            }
        }
        ":doc" => {
            if arg.is_empty() {
                println!("pakai: :doc <nama>");
                return true;
            }
            match crate::bantu::jelaskan(arg) {
                Some(t) => println!("{}", t),
                None => {
                    // Bukan cuma "tidak ada": tawarkan yang paling mirip,
                    // lewat definisi saran nama yang sama dengan yang dipakai
                    // diagnostik di seluruh bahasa ini.
                    let semua = crate::vm::daftar_builtin();
                    let rujuk: Vec<&str> = semua.iter().map(|s| s.as_str()).collect();
                    match crate::diag::saran_nama_terdekat(arg, rujuk.iter().copied()) {
                        Some(m) => println!("`{}` tidak terdaftar — maksudnya `{}`?", arg, m),
                        None => println!("`{}` tidak terdaftar. Coba `:cari {}`.", arg, arg),
                    }
                }
            }
        }
        ":daftar" => {
            if arg.is_empty() {
                for (ns, n) in crate::bantu::namespace_tersedia() {
                    println!("  {:<10} {} fungsi", ns, n);
                }
            } else {
                let isi = crate::bantu::daftar_namespace(arg);
                if isi.is_empty() {
                    println!("namespace `{}` tidak ada — `:daftar` tanpa argumen memberi daftarnya", arg);
                } else {
                    for i in &isi { println!("  {}", i); }
                    println!("{} nama di `{}::`", isi.len(), arg);
                }
            }
        }
        ":simpan" => {
            if arg.is_empty() {
                println!("pakai: :simpan <berkas.jrg>");
                return true;
            }
            let isi = s.sebagai_program();
            // Diperiksa dengan PARSER yang sama, bukan dengan tebakan.
            let sf = SourceFile { id: 0, name: arg.to_string(), src: isi.clone() };
            let galat = match lex(&sf) {
                Err(e) => Some(e.msg.clone()),
                Ok(tok) => parse(tok, false).1.first().map(|g| g.msg.clone()),
            };
            if let Some(g) = galat {
                // Pesan parsernya diteruskan APA ADANYA.
                println!("tidak ditulis: sesi ini tidak sah sebagai berkas — {}", g);
                return true;
            }
            match std::fs::write(arg, isi) {
                Ok(()) => println!("ditulis ke {}", arg),
                Err(e) => println!("gagal menulis {}: {}", arg, crate::host::galat_io(&e)),
            }
        }
        lain => println!("perintah tidak dikenal: {} — coba :bantuan", lain),
    }
    true
}

/// Jalankan REPL sampai EOF atau `:keluar`. Mengembalikan exit code.
pub fn jalankan() -> i32 {
    let stdin = std::io::stdin();
    let interaktif = std::io::IsTerminal::is_terminal(&stdin);
    if interaktif {
        println!("{}", SAPAAN);
    }
    let mut s = Sesi::baru();
    let mut buf = String::new();
    // Lock stdin diambil HANYA di jalur non-interaktif.
    let mut sunting = if interaktif { Some(crate::baris::Penyunting::baru()) } else { None };
    loop {
        let prompt = if buf.is_empty() { "jrg> " } else { "  ... " };
        let baris = match sunting.as_mut() {
            Some(p) => match p.baca(prompt) {
                crate::baris::Hasil::Baris(b) => { p.ingat(&b); b }
                crate::baris::Hasil::Selesai => return 0,
                // Ctrl-C membuang baris DAN blok yang belum selesai — kalau
                // hanya barisnya, sesi tetap terkunci di dalam blok itu.
                crate::baris::Hasil::Batal => { buf.clear(); continue; }
            },
            None => {
                let mut b = String::new();
                match crate::host::baca_baris(&mut b) {
                    Ok(0) => return 0,
                    Ok(_) => {}
                    Err(e) => { eprintln!("gagal membaca masukan: {}", e); return 1; }
                }
                b
            }
        };
        let potong = baris.trim_end_matches(['\n', '\r']);

        // Baris kosong di tengah blok = batal. Tanpa ini, satu kurung yang
        // salah ketik mengunci sesi dan satu-satunya jalan keluar Ctrl-C.
        if !buf.is_empty() && potong.trim().is_empty() {
            buf.clear();
            continue;
        }
        if buf.is_empty() {
            let t = potong.trim();
            if t.is_empty() {
                continue;
            }
            if t.starts_with(':') {
                if !perintah(&mut s, t) {
                    return 0;
                }
                continue;
            }
        }

        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(potong);
        if !Sesi::lengkap(&buf) {
            continue;
        }

        let teks = std::mem::take(&mut buf);
        match s.jalankan(&teks) {
            Ok(h) => {
                if let Hasil::Nilai(v) = &h {
                    println!("{}", s.interp.tampil(v).unwrap_or_else(|_| format!("{}", v)));
                }
                s.riwayat.push((teks, match h {
                    Hasil::Deklarasi => Jenis::Deklarasi,
                    Hasil::Nilai(_)  => Jenis::Ekspresi,
                    Hasil::Sunyi     => Jenis::Pernyataan,
                }));
            }
            Err(e) => {
                // Masukan yang gagal di tengah jalan tidak boleh meninggalkan sesi di raw
                // mode atau di layar alternatif.
                crate::term::pulihkan();
                // Satu baris untuk masukan satu baris: kutipan sumber dan caret cuma akan
                // mengulang baris yang barusan diketik dan masih terlihat di layar, jadi ia …
                if teks.contains('\n') {
                    eprintln!("error: {}", e);
                    for (i, l) in teks.lines().enumerate() {
                        eprintln!("{:>3} | {}", i + 1, l);
                    }
                } else {
                    eprintln!("error: {}", e);
                }
            }
        }
    }
}
