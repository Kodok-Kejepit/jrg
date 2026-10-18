// src/interpreter/galat_index.rs — src/interpreter/galat_index.rs — kalimat galat untuk operasi `obj[idx]`.

/// Batas jumlah nama yang disebut sebelum sisanya diringkas.
const MAKS_NAMA: usize = 12;

/// Wadah yang di-index dengan tipe yang tidak bisa dipakai untuk itu.
pub fn tipe_index_salah(wadah: &str, indeks: &str) -> String {
    format!(
        "tidak bisa index `{}` dengan `{}` — array, string dan tuple di-index \
         dengan int (boleh negatif, dihitung dari belakang), atau dipotong \
         dengan `a[awal:akhir]`",
        wadah, indeks
    )
}

/// Wadah yang memang tidak bisa di-index sama sekali (int, bool, nil, float).
pub fn bukan_wadah(wadah: &str) -> String {
    format!(
        "tidak bisa index `{}` — yang bisa di-index cuma array, string, tuple, \
         dict dan struct",
        wadah
    )
}

/// `a[0..2]`, yaitu Range yang sudah terlanjur jadi array biasa.
pub fn range_dikira_potongan(wadah: &str) -> String {
    format!(
        "tidak bisa index `{}` dengan `array` — kalau maksudnya potongan, \
         sintaksnya titik dua (`a[awal:akhir]`), bukan `..`: sebuah `..` bikin \
         Range yang dievaluasi jadi array biasa dulu, baru dipakai sebagai \
         index — itu sebabnya nyasar ke sini",
        wadah
    )
}

/// Key yang tidak ada di sebuah dict.
pub fn key_hilang(key: &str, tersedia: &[String]) -> String {
    format!("key `{}` tidak ada di dict ini{}", key, daftar("key", tersedia))
}

/// Field yang tidak ada di sebuah struct.
pub fn field_hilang(nama_struct: &str, field: &str, tersedia: &[String]) -> String {
    format!(
        "field `{}` tidak ditemukan di struct `{}`{}",
        field,
        nama_struct,
        daftar("field", tersedia)
    )
}

/// Bagian "— key/field yang ada: …", diurutkan supaya keluarannya tidak
/// bergantung pada urutan hash (yang di Rust diacak per proses, dan pernah …
fn daftar(sebutan: &str, tersedia: &[String]) -> String {
    if tersedia.is_empty() {
        return format!(" — belum ada {} sama sekali di sini", sebutan);
    }
    let mut nama: Vec<&str> = tersedia.iter().map(|s| s.as_str()).collect();
    nama.sort_unstable();
    if nama.len() <= MAKS_NAMA {
        format!(" — {} yang ada: {}", sebutan, nama.join(", "))
    } else {
        format!(
            " — {} yang ada: {}, dan {} lagi",
            sebutan,
            nama[..MAKS_NAMA].join(", "),
            nama.len() - MAKS_NAMA
        )
    }
}

/// Sebutan untuk satu anggota: sebuah dict punya **key**, sebuah struct punya
/// **field**.
pub fn sebutan_anggota(dict: bool) -> &'static str {
    if dict { "key" } else { "field" }
}

/// Key/field antara yang belum ada, di JALUR TULIS (`d["a"]["b"] = 1`).
pub fn belum_ada_untuk_ditulis(sebutan: &str, nama: &str) -> String {
    format!(
        "{} `{}` belum ada — bikin dulu isinya sebelum menulis ke dalamnya",
        sebutan, nama
    )
}

/// `.field` yang ditulis ke nilai bukan struct/dict, di langkah mana pun dari
/// jalur tulis. Dulu tiga kalimat: dua di interpreter, satu di VM.
pub fn bukan_pemilik_field(tipe: &str, field: &str) -> String {
    format!("`{}` tidak punya field — `.{}` cuma bisa ditulis ke struct atau dict{}",
            tipe, field, kurung_kosong(tipe))
}

/// Ekor untuk galat atas `()`: `var d = {}` adalah jebakan dari Python/JS, karena `{}` di JRG
/// blok kosong. Kosong untuk tipe lain.
pub fn kurung_kosong(tipe: &str) -> &'static str {
    if tipe == "unit" {
        "\n  bantuan: kalau `()` ini datang dari `{}` — di JRG `{}` adalah blok kosong, \
         bukan dict kosong; dict kosong ditulis `dict::new()`"
    } else { "" }
}

/// `[...]` yang ditulis ke nilai yang tidak bisa diubah lewat index. VM dulu
/// meneruskan tuple ke jalur BACA lalu menulis ke dalamnya lewat alias.
pub fn tak_bisa_ditulis_lewat_index(tipe: &str) -> String {
    format!("`{}` tidak bisa ditulis lewat `[...]` — yang bisa cuma array, dict, dan struct{}",
            tipe, kurung_kosong(tipe))
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn daftar_selalu_terurut() {
        let a = vec!["z".to_string(), "a".to_string(), "m".to_string()];
        assert!(key_hilang("q", &a).ends_with("key yang ada: a, m, z"));
    }

    #[test]
    fn daftar_panjang_diringkas() {
        let a: Vec<String> = (0..30).map(|i| format!("k{:02}", i)).collect();
        let p = key_hilang("q", &a);
        assert!(p.contains("dan 18 lagi"), "{}", p);
        assert!(!p.contains("k29"), "nama ke-30 seharusnya tidak ikut: {}", p);
    }

    #[test]
    fn wadah_kosong_bilang_kosong() {
        assert!(key_hilang("q", &[]).contains("belum ada key sama sekali"));
    }

    /// Membaca dan MENULIS bukan situasi yang sama, jadi kalimatnya tidak
    /// boleh ikut menyatu — yang menulis punya jalan keluar yang bisa
    /// disebut, yang membaca cuma perlu tahu apa yang ada.
    #[test]
    fn tulis_menawarkan_jalan_keluar_baca_tidak() {
        let tulis = belum_ada_untuk_ditulis("key", "b");
        assert!(tulis.contains("bikin dulu"), "{}", tulis);
        let baca = key_hilang("b", &["a".to_string()]);
        assert!(!baca.contains("bikin dulu"), "{}", baca);
        assert!(baca.contains("key yang ada: a"), "{}", baca);
    }

    /// Pesan harus menyebut apa yang SAH, bukan cuma apa yang salah.
    #[test]
    fn tipe_salah_menyebut_yang_sah() {
        let p = tipe_index_salah("array", "string");
        assert!(p.contains("`array`") && p.contains("`string`"));
        assert!(p.contains("int"), "harus menyebut int sebagai yang sah: {}", p);
        assert!(p.contains("a[awal:akhir]"), "harus menyebut potongan: {}", p);
    }
}

/// Batas panjang sebuah NILAI yang ikut dicetak di pesan galat.
const MAKS_NILAI: usize = 60;

/// Nilai yang siap dicetak di sebuah pesan galat: dipotong kalau panjang.
pub fn potong_nilai(v: &str) -> String {
    if v.chars().count() <= MAKS_NILAI { return v.to_string(); }
    let depan: String = v.chars().take(MAKS_NILAI).collect();
    format!("{}… ({} karakter)", depan, v.chars().count())
}

/// Pola field yang menyebut nama yang tidak ada. Dulu lengannya cuma DILEWATI,
/// jadi salah ketik satu huruf membuat cabangnya tidak pernah jalan dan yang
/// terlihat cuma cabang `_`.
pub fn field_pola_tak_ada(tipe: &str, field: &str, ada: &[String], saran: Option<&str>) -> String {
    let ekor = match saran {
        Some(s) => format!(" — maksudnya `{}`?", s),
        None => String::new(),
    };
    let daftar = if ada.is_empty() { String::from("(tidak ada)") } else { ada.join(", ") };
    format!(
        "pola `{}` menyebut field `{}` yang tidak ada{}\nbantuan: yang ada: {}. \
         Field yang tidak dideklarasikan membuat lengannya tidak pernah cocok, \
         jadi ia dilaporkan alih-alih dilewati diam-diam",
        tipe, field, ekor, daftar)
}

/// Menulis ke field yang tidak dideklarasikan sebuah struct.
/// Rekaman DATA milik pustaka yang memang ditulis sebagai literal tanpa deklarasi
/// (`Warna { r, g, b, a }` untuk `canvas::`); handle bawaan tidak ada di sini.
pub const STRUCT_DATA_PUSTAKA: &[&str] = &["Warna"];

/// Literal struct yang tidak mengisi field tanpa nilai default — satu kalimat di tiga mesin.
pub fn field_wajib(nama: &str, field: &str) -> String {
    format!(
        "struct `{}`: field `{}` wajib diisi (tidak ada nilai default)\nbantuan: tulis `{}: ...` \
         di literalnya, atau beri nilai default di deklarasi struct-nya (`{}: Tipe = nilai`)",
        nama, field, field, field)
}

/// Literal varian ber-field yang tidak mengisi salah satu field-nya (varian tidak punya default).
pub fn field_wajib_varian(varian: &str, field: &str) -> String {
    format!("varian `{}` butuh field `{}`\nbantuan: tulis `{}: ...` di literalnya — field \
             varian tidak punya nilai default", varian, field, field)
}

/// Basis `...` yang membawa field di luar deklarasi sasarannya; `sasaran` mis. "struct `B`".
/// Dulu `B { ...a }` melahirkan `B` yang memuat field milik tipe `a`.
pub fn sebar_field_liar(sasaran: &str, field: &str) -> String {
    format!("basis `...` membawa field `{}` yang tidak dideklarasikan {}\nbantuan: salin field yang \
             dimaksud satu per satu (`nama: sumber.nama`)", field, sasaran)
}

/// Field pertama (urut abjad) milik basis sebar yang tidak ada di `boleh`.
pub fn field_sebar_liar<'a>(ada: impl Iterator<Item = &'a String>, boleh: &[String]) -> Option<&'a String> {
    ada.filter(|k| !boleh.contains(k)).min()
}

/// Literal `Nama { .. }` atas nama yang tidak dideklarasikan program sebagai struct.
pub fn struct_tak_dideklarasikan(nama: &str, saran: Option<&str>) -> String {
    if crate::parser::TIPE_BAWAAN.contains(&nama) {
        return format!(
            "`{}` dibuat oleh pustaka, tidak bisa ditulis sebagai literal\nbantuan: nilai \
             bertipe ini datang dari fungsi yang membuatnya (mis. `ptr::new`, `Future::new`, \
             `dok::router_new`); isinya bukan sesuatu yang bisa disusun sendiri", nama);
    }
    let ekor = match saran {
        Some(s) => format!(" — maksudnya `{}`?", s),
        None => String::new(),
    };
    format!(
        "struct `{}` belum dideklarasikan{}\nbantuan: literal `{} {{ ... }}` butuh \
         `struct {}` yang dideklarasikan; untuk data yang bentuknya tidak tetap, pakai `dict`",
        nama, ekor, nama, nama)
}

/// Handle buatan pustaka: isinya nomor atau saluran yang cuma berarti bagi pustakanya.
pub const HANDLE_PUSTAKA: &[&str] = &["Ptr", "Future", "Task", "DokRouter", "TcpConn",
    "TcpListener", "UdpSock", "DokDb", "DokWs", "WasmModul", "DotnetFn", "JavaObj"];

/// Menulis ke field sebuah handle pustaka: dulu diterima, dan `h.id = 7` mengganti sasarannya.
pub fn tulis_handle(tipe: &str, field: &str) -> String {
    format!(
        "`{}` adalah handle buatan pustaka — field `{}` tidak bisa ditulis\nbantuan: isinya cuma \
         berarti bagi pustaka yang membuatnya; ubah lewat fungsi pustakanya, atau buat handle baru",
        tipe, field)
}

pub fn field_tak_dideklarasikan(tipe: &str, field: &str, saran: Option<&str>) -> String {
    let ekor = match saran {
        Some(s) => format!(" — maksudnya `{}`?", s),
        None => String::new(),
    };
    format!(
        "struct `{}` tidak punya field `{}`{}\nbantuan: field sebuah struct ditentukan \
         deklarasinya; menulis ke nama lain tidak menambah field. Untuk data yang \
         kuncinya baru diketahui saat jalan, pakai `dict`",
        tipe, field, ekor)
}

/// `x.m` tanpa `()` untuk sebuah METHOD: tidak dipanggil, tidak pula diambil
/// sebagai nilai. Menyebut dua jalan keluarnya, karena keduanya sah.
pub fn metode_tanpa_kurung(tipe: &str, metode: &str) -> String {
    format!(
        "`{m}` itu method milik `{t}`, dan di sini ditulis tanpa `()`\nbantuan: untuk \
         MEMANGGILNYA tulis `.{m}()`; untuk mengambil methodnya sebagai NILAI tulis \
         `{t}::{m}`\ncatatan: memanggilnya diam-diam akan membuat `typeof({t_kecil}.{m})` \
         punya efek samping, jadi bentuk tanpa kurung sengaja tidak diberi arti",
        m = metode, t = tipe, t_kecil = tipe.to_lowercase())
}

/// `h.nama` tanpa `()` atas handle asing (`py:list`): tidak dipanggil diam-diam,
/// dan atributnya punya pintunya sendiri.
pub fn field_handle_asing(tipe: &str, nama: &str) -> String {
    let ns = tipe.split(':').next().unwrap_or("py");
    format!(
        "`{n}` ditulis tanpa `()` di atas handle `{t}`\nbantuan: untuk MEMANGGIL method-nya \
         tulis `.{n}()`; untuk membaca atributnya tulis `{ns}::get(h, \"{n}\")`",
        n = nama, t = tipe, ns = ns)
}

/// Namespace tempat method BAWAAN sebuah penerima dicari, dari nama tipenya.
/// Satu definisi untuk dua mesin: pintu FIELD dan pintu METHOD harus sepakat
/// soal apa yang dihitung method, atau `x.m` tanpa kurung berarti dua hal.
pub fn ns_metode_bawaan(tipe: &str) -> Option<&'static str> {
    match tipe {
        "string" => Some("str"),
        "array" => Some("arr"),
        "channel" => Some("chan"),
        "canvas" => Some("canvas"),
        "dict" => Some("dict"),
        "enum:Option" => Some("Option"),
        "enum:Result" => Some("Result"),
        "atomic" => Some("atomic"),
        "mutex" => Some("mutex"),
        _ => None,
    }
}

/// Pseudo-field yang SENGAJA dibaca sebagai field, bukan method: keduanya
/// menjawab nilai di dua mesin sejak lama, jadi menolaknya akan memutus
/// program yang benar.
pub fn pseudo_field(tipe: &str, nama: &str) -> bool {
    matches!(tipe, "string" | "array") && matches!(nama, "len" | "is_empty")
}

/// Sebuah fungsi ditulis sendirian sebagai satu pernyataan. Dulu acuan
/// MEMANGGILNYA (dengan peringatan) sementara VM dan LLVM diam saja.
pub fn fungsi_sebagai_pernyataan(nama: &str) -> String {
    format!(
        "`{n}` itu fungsi, dan di sini ia berdiri sendiri sebagai satu pernyataan — \
         jadi tidak ada yang dikerjakan\nbantuan: kalau mau MEMANGGILNYA, tulis \
         `{n}()` lengkap dengan argumennya\ncatatan: kalau memang mau memakai `{n}` \
         sebagai NILAI, pakai di tempat yang menerima nilai — mis. `let f = {n}` \
         atau sebagai argumen sebuah HOF",
        n = nama)
}

/// `k.f(1)` di mana `f` FIELD bertipe fungsi, bukan method. `--vm` dulu
/// memanggil `f(k, 1)` — penerima yang tidak pernah ditulis — lalu BERHASIL,
/// dan kalimat acuan menyangkal dirinya sendiri ("tidak punya `f` … yang ada: f").
pub fn field_fungsi_bukan_metode(tipe: &str, nama: &str) -> String {
    format!(
        "`{n}` itu FIELD milik `{t}`, bukan method — jadi `.{n}(...)` tidak punya \
         arti\nbantuan: untuk memanggil fungsi yang disimpan di dalamnya, bungkus \
         dulu field-nya: `(x.{n})(...)`\ncatatan: memanggilnya sebagai method akan \
         menyisipkan penerimanya sebagai argumen pertama, yang tidak pernah ditulis",
        n = nama, t = tipe)
}

/// Nilai yang tidak punya padanan di JSON. Dulu `NaN`/`inf` ditulis apa
/// adanya — teks yang ditolak `json.loads` Python DAN `JSON.parse` Node — dan
/// sebuah fungsi jadi teks `"<function>"` yang tampak seperti data.
pub fn tidak_bisa_jadi_json(apa: &str, jalan: &str) -> String {
    crate::json_inti::pesan_tak_bisa(apa, jalan)
}
