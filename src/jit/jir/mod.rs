//! JIR — IR SSA milik JRGJIT (`dokumentasi/jit/jir-rencana.md`): dibangun dari bytecode dan `layak::Rencana`,
//! diperiksa (`periksa`), diturunkan ke CLIF (`turun`) dengan status, deopt dan jaga yang sama dengan penurun
//! langsung (`super::turun`), yang tetap menurunkan unit di luar cakupannya.

pub mod bangun;
pub mod periksa;
pub mod turun;

use std::fmt;

/// Satu nilai SSA (didefinisikan tepat sekali: parameter blok atau hasil instruksi).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Nilai(pub u32);

/// Satu blok dasar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Blok(pub u32);

/// Tipe nilai: bulat 64 bit (juga `bool` sebagai 0/1), pecahan, atau bulat 128 bit (`Jenis::Lebar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipe { I, F, L }

/// Operasi atas bulat 128 bit. Banding, bagi dan sisa menghasilkan bulat 64 bit; geser kanan/kiri: operand kedua 64 bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpL { Tambah, Kurang, Kali, Dan, Atau, Xor, DanBukan, GeserKiri, GeserKanan, Cmp(Banding), Bagi, Sisa }

/// Perbandingan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banding { Lt, Le, Gt, Ge, Eq, Ne }

/// Operasi dua operand. `I*` atas bulat, `F*` atas pecahan; perbandingan menghasilkan bulat 0/1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biner {
    /// Tambah, kurang, kali bulat dengan luapan 64 bit dijaga.
    ITambah, IKurang, IKali,
    /// Bagi dan sisa bulat; pembagi 0 dan -1 dijaga (kalimat galat, `MIN / -1`, aturan VM).
    IBagi, ISisa,
    IDan, IAtau, IXor, IDanBukan,
    /// Geser kiri: geseran di atas 63 dan bit yang hilang dijaga.
    IGeserKiri,
    /// Geser kanan aritmetika: geseran 128 ke atas dijaga, 64–127 sama dengan 63.
    IGeserKanan,
    ICmp(Banding),
    FTambah, FKurang, FKali, FBagi,
    FCmp(Banding),
}

/// Operasi satu operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unar {
    /// Negasi bulat; `MIN` dijaga.
    INeg,
    /// Bit pecahan sebagai bulat, dan sebaliknya (slot payload berjenis campur di bayangan varian).
    BitF, DariBit,
    FNeg,
    /// `!` atas `bool` (0/1).
    Bukan,
    /// `~` bulat.
    IBukanBit,
    /// Kebenaran bulat/`bool`: bukan nol.
    KeBool,
    /// Bulat ke pecahan, seperti `i128 as f64` acuan untuk nilai 64 bit.
    KeF,
    /// Bulat 64 bit ke 128 bit, bertanda / tak bertanda (`math::wrap_*` menjawab 0 sampai 2^64-1).
    KeL, KeLU,
    /// `~` atas 128 bit; kebenaran 128 bit (bukan nol, hasil 64 bit); 64 bit bawahnya.
    BukanBitL, KeBoolL, Rendah,
}

/// Keadaan bingkai VM di titik deopt: instruksi tempat VM melanjutkan dan isi native tiap posisi stack (`None` =
/// posisi tanpa isi native — `nil`, `()` — yang dibangun `deopt::pulihkan` dari jenisnya).
#[derive(Clone, Debug, PartialEq)]
pub struct Keadaan {
    pub ip: usize,
    pub isi: Vec<Option<Nilai>>,
    /// Bingkai callee yang disisipkan (JIR-3): fungsi JIT-nya (`modul::info`), geser posisinya di stack pemanggil,
    /// dan keadaan pemanggil di `Call`-nya. `None` = bingkai unit ini sendiri.
    pub fid: Option<u32>,
    pub geser: usize,
    pub induk: Option<Box<Keadaan>>,
    /// Varian berbayangan yang isi stack VM-nya basi: diwujudkan sebelum deopt menulis rekamannya.
    pub maya: Vec<Maya>,
}

/// Varian di posisi `pos` yang isi stack VM-nya mungkin basi (bayangan varian, JIR-3b): penanda `tag` (indeks
/// `TabelVarian` di `tabel`), payload `muatan` (bit `pecahan` = pecahan), basi kalau bit penandanya ada di `basi`.
#[derive(Clone, Debug, PartialEq)]
/// `tuple`: yang ditulis cuma payload tuple-nya (`PayloadLokal` yang dilipat, langkah 105), bukan variannya.
pub struct Maya { pub pos: usize, pub tag: Nilai, pub muatan: Vec<Nilai>, pub basi: u64, pub pecahan: u64, pub tabel: i64, pub tuple: bool }

/// Jaga yang gagal = deopt di `keadaan`. `spek`: kegagalannya dicatat (`GAGAL_SPEK`), kompilasi berikutnya tidak
/// berspekulasi lagi di sana.
#[derive(Clone, Debug, PartialEq)]
pub struct Jaga {
    pub keadaan: Keadaan,
    pub spek: bool,
    /// Puncak keadaannya masih milik VM (hasil yang bukaannya gagal, `PUNCAK_VM`): isinya tidak dicatat.
    pub puncak_vm: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    KonstI { hasil: Nilai, nilai: i64 },
    KonstF { hasil: Nilai, nilai: f64 },
    /// `jaga` = kegagalan operasinya sendiri (luapan, pembagi, geseran); `None` = terbukti tidak gagal.
    Biner { hasil: Nilai, op: Biner, a: Nilai, b: Nilai, jaga: Option<Jaga> },
    Unar { hasil: Nilai, op: Unar, a: Nilai, jaga: Option<Jaga> },
    /// `jika ? ya : tidak`.
    Pilih { hasil: Nilai, jika: Nilai, ya: Nilai, tidak: Nilai },
    /// `kondisi` bukan nol = deopt.
    JagaJika { kondisi: Nilai, jaga: Jaga },
    /// Undian deopt acak (`JRG_JIT_STRES=deopt-acak`): 1 kira-kira sekali tiap 16.
    Undi { hasil: Nilai },
    /// Panggilan langsung ke callee bulat `Rencana::callee[callee]` (`tanda_tangan_bulat`). Callee yang belum mulai
    /// (`ULANG`) = deopt di `di` (VM yang memanggil); callee yang deopt = deopt di `sesudah` (VM melanjutkan callee-nya).
    Panggil { hasil: Nilai, callee: usize, args: Vec<Nilai>, di: Keadaan, sesudah: Keadaan },
    /// Lokal `lokal` milik VM disalin (`lambat::muat`) atau dipindah (`lambat::ambil`, lokal yang mati sesudahnya)
    /// ke posisi `ke`.
    Muat { lokal: usize, ke: usize, pindah: bool },
    /// Puncak `dari` (isi native `nilai` berkode jenis `kode`, atau milik VM) ke lokal `ke` (`lambat::simpan`).
    Simpan { ke: usize, dari: usize, nilai: Option<Nilai>, kode: i32, buang: bool },
    /// Posisi `pos` milik VM dilepas (`lambat::buang`).
    Buang { pos: usize },
    /// Kebenaran posisi `pos` milik VM (`lambat::benar`), 0/1; `lepas` = posisinya dilepas.
    Benar { hasil: Nilai, pos: usize, lepas: bool },
    /// Nilai VM di alamat `alamat` (konstanta chunk, callee rencana) disalin ke posisi `pos` (`lambat::taruh`).
    Taruh { pos: usize, alamat: i64 },
    /// VM menjalankan instruksi `ip` (`lambat::langkah`): isi native `tumpah` ke slot rekaman, posisi maya `taruh`
    /// ke stack VM dulu; `jenis` = alamat `Rencana::keadaan[ip]`, tinggi sesudahnya `sesudah`; galat = `GAGAL`.
    Langkah { ip: usize, jenis: i64, t: usize, tumpah: usize, sesudah: usize, isi: Vec<(usize, Nilai)>, taruh: Vec<(usize, i64)> },
    /// Posisi `pos` milik VM dibuka jadi angka berkode jenis `kode` (`lambat::buka`); jenis lain = deopt spekulasi.
    Buka { hasil: Nilai, pos: usize, kode: i32, jaga: Jaga },
    /// Fungsi bawaan `fungsi` atas `argc` argumen di puncak (`lambat::bawaan`, `argc` membawa bit `BAWAAN_POLOS`),
    /// hasilnya milik VM di posisi callee; galat = `GAGAL`. Tumpahan seperti `Langkah`.
    Bawaan { ip: usize, jenis: i64, t: usize, argc: i64, fungsi: i64, isi: Vec<(usize, Nilai)>, taruh: Vec<(usize, i64)> },
    /// Operator atas dua operand yang bukan dua angka native (`lambat::biner`, `tambah_nilai` untuk `+`): hasil
    /// aritmetika milik VM, hasil banding 0/1 di `hasil`; galat = `GAGAL`.
    BinerVm { hasil: Option<Nilai>, tambah: bool, ip: usize, jenis: i64, t: usize, isi: Vec<(usize, Nilai)>, taruh: Vec<(usize, i64)> },
    /// Tangkapan ke-`idx` ke posisi `ke` (`lambat::muat_upval`); bukan closure = deopt.
    MuatUpval { idx: usize, ke: usize, jaga: Jaga },
    /// Puncak `dari` ke tangkapan ke-`idx` (`lambat::simpan_upval`); bukan closure = deopt.
    SimpanUpval { idx: usize, dari: usize, nilai: Option<Nilai>, kode: i32, jaga: Jaga },
    /// `str(n)` atas bulat native ke posisi `pos` (`lambat::teks_bulat`).
    TeksBulat { pos: usize, n: Nilai },
    /// Posisi `dari` milik VM dipindah ke `ke` (`lambat::pindah`).
    Pindah { dari: usize, ke: usize },
    /// Isi native (atau `nil`/`()` tanpa isi) berkode jenis `kode` ditulis ke stack VM di posisi `pos`
    /// (`lambat::tulis_vm`): tepi ke titik temu yang posisinya dilebarkan jadi milik VM.
    TulisVm { pos: usize, nilai: Option<Nilai>, kode: i32 },
    KonstL { hasil: Nilai, nilai: i128 },
    /// `jaga` = luapan 128 bit (tambah/kurang) atau pembagi 0/-1; `jaga_muat` = operand bagi/sisa di luar 64 bit
    /// (spekulasi); geser dengan geseran tetap 1–63 tanpa jaga.
    BinerL { hasil: Nilai, op: OpL, a: Nilai, b: Nilai, jaga: Option<Jaga>, jaga_muat: Option<Jaga> },
    /// 128 bit ke 64 bit yang dispekulasikan muat; tidak muat = deopt `jaga`.
    Sempit { hasil: Nilai, a: Nilai, jaga: Jaga },
    /// Isi 128 bit ke stack VM di posisi `pos` (`lambat::tulis_lebar`).
    TulisLebar { pos: usize, nilai: Nilai },
    /// Posisi `pos` milik VM dibuka jadi 128 bit (`lambat::buka_lebar`); bukan bulat = deopt `jaga`.
    BukaLebar { hasil: Nilai, pos: usize, jaga: Jaga },
    /// `?` atas nilai milik VM di `pos` (`lambat::sebar`): `Ok`/`Some` dibuka di tempat; `Err`/`None` kembali dengan
    /// nilai itu kalau `pulang`, selain itu (dan bukan `Result`/`Option`) deopt `jaga` — VM yang menjalankannya.
    /// `keluarga` = arg opcode-nya (keluarga kembalian yang ditulis fungsinya; lain = VM yang melapor).
    Sebar { pos: usize, pulang: bool, keluarga: u32, jaga: Jaga },
    /// Lengan ke-`urutan` rantai `match` `rantai` atas lokal `lokal` (`turun::rantai_pola`): lengan pertama memanggil
    /// `lambat::pola_pilih` (alamat `situs`) dan menyimpan indeksnya di slot rantai; tiap lengan membandingkan indeksnya.
    /// `hasil` = cocok tidaknya (0/1); galat = deopt `jaga`; `panggil` = memanggil sendiri (lengan awal dilipat).
    Rantai { hasil: Nilai, rantai: usize, urutan: usize, lokal: usize, situs: i64, panggil: bool, jaga: Jaga },
    /// `l.m(..)` dengan `m` berumus (`Cara::UbahLokal`/`RumusLokal`, `Fungsi::rumus[rumus]`) atas lokal `lokal`:
    /// field dibaca dan diperiksa pembantu (gagal = deopt spekulasi `jaga`), rumusnya dihitung native dengan argumen
    /// `args` (pecahan, `None` = konstanta di rumusnya), lalu field ditulis (`tulis`) atau nilainya jadi `hasil`.
    Rumus { rumus: usize, lokal: usize, args: Vec<Option<Nilai>>, tulis: bool, hasil: Option<Nilai>, jaga: Jaga },
    /// Literal atas `n` posisi teratas (`lambat::bangun`, situs `SitusBangun` di alamat `situs`), hasilnya milik VM
    /// di posisi `t - n`. Tumpahan seperti `Langkah`.
    Bangun { situs: i64, jenis: i64, t: usize, n: usize, isi: Vec<(usize, Nilai)>, taruh: Vec<(usize, i64)> },
    /// Posisi `dari` milik VM disalin ke `ke` (`lambat::salin`).
    Salin { dari: usize, ke: usize },
    /// Dua posisi milik VM ditukar (`lambat::tukar`).
    Tukar { x: usize, y: usize },
    /// Label tipe nilai VM di `pos` harus `label` (alamat `Rencana::metode[n]`, `lambat::cek_metode`); lain = deopt
    /// spekulasi. Posisinya jadi callee maya.
    CekMetode { pos: usize, label: i64, jaga: Jaga },
    /// Iterator di posisi `slot` masih punya elemen (`lambat::iter_ada`), 0/1; galat (2) = deopt.
    IterAda { hasil: Nilai, slot: usize, jaga: Jaga },
    /// Indeks iterator di posisi `slot` maju satu (`lambat::maju`); gagal = deopt.
    Maju { slot: usize, jaga: Jaga },
    /// Panggilan langsung `Rencana::langsung[n]` (`masuk*`, kode native callee, `keluar*`): argumen native `args`,
    /// callee maya di alamat `fungsi` (0 = di stack VM); pemanggil basi = deopt di `basi`; hasil berspekulasi angka
    /// (kode jenis) ke `hasil`, dari register kalau statusnya cocok, selain itu dibuka berjaga.
    Langsung {
        n: usize, ip: usize, jenis: i64, t: usize, a: usize, fungsi: i64, isi: Vec<(usize, Nilai)>, taruh: Vec<(usize, i64)>,
        args: Vec<(usize, Nilai)>, basi: Keadaan, hasil: Option<(Nilai, i32, Jaga)>,
    },
    /// Panjang nilai VM di posisi `pos` (`lambat::panjang`, posisinya dilepas) atau lokal `lokal`
    /// (`lambat::panjang_lokal`, `Cara::PanjangLokal`); galat = deopt, VM yang melapor.
    Panjang { hasil: Nilai, pos: usize, lokal: bool, jaga: Jaga },
    /// Pembantu `pb` atas `args`; statusnya yang lolos `gagal` = deopt; `hasil` = statusnya sebagai bulat (0/1);
    /// `keluar` = kata yang ditulis pembantu ke penyangga (`Arg::Wadah`), bertipe nilainya.
    Pembantu { pb: Pb, args: Vec<Arg>, hasil: Option<Nilai>, gagal: Option<(Uji, Jaga)>, keluar: Option<Nilai> },
    /// Varian berbayangan ditulis ke stack VM kalau isinya basi (`lambat::wujud_varian`).
    Wujud(Maya),
}

/// Pembantu `lambat` yang dipanggil lewat `Inst::Pembantu`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pb { Field, Pola, PolaLokal, Payload, PayloadLokal, MuatGlobal, SimpanGlobal, Indeks, Baca, BacaPecahan, Tulis, JenisSama, PayloadJenisSama, PayloadPanjang, Karakter, KarakterLokal, PayloadAngkaLokal, FieldLokal }

/// Argumen `Inst::Pembantu`: konstanta 64/32 bit atau isi native (bit `i64`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg { I64(i64), I32(i32), Isi(Nilai), Wadah }

/// Status pembantu yang berarti gagal (deopt).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uji { BukanNol, Sama(i64), Negatif }

/// Status `Return` native (`super::turun::BERHASIL*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hasil { Bulat, Bool, Pecahan }

#[derive(Clone, Debug, PartialEq)]
pub enum Akhir {
    Lompat { ke: Blok, args: Vec<Nilai> },
    Cabang { kondisi: Nilai, ya: Blok, ya_args: Vec<Nilai>, tidak: Blok, tidak_args: Vec<Nilai> },
    Kembali { nilai: Nilai, hasil: Hasil },
    /// Hasil milik VM di posisi `pos` (`BERHASIL_NILAI`).
    KembaliNilai { pos: usize },
    /// `ReturnNil` (`BERHASIL_UNIT`).
    KembaliUnit,
    /// Titik keluar: VM menjalankan instruksi `keadaan.ip`.
    Keluar { keadaan: Keadaan },
    /// VM menjalankan instruksi bercabang `ip` (`lambat::langkah_cabang`): `hasil_bool` = `bool` di puncak memilih
    /// `tujuan[0]` (benar) atau `tujuan[1]`; selain itu `ip` berikutnya yang dijawab VM. Galat = `GAGAL`.
    CabangVm { ip: usize, jenis: i64, t: usize, tumpah: usize, hasil_bool: bool, isi: Vec<(usize, Nilai)>, taruh: Vec<(usize, i64)>,
        tujuan: [(Blok, Vec<Nilai>, usize); 2] },
}

#[derive(Clone, Debug)]
pub struct DataBlok {
    /// Instruksi bytecode tempat blok ini mulai.
    pub ip: usize,
    pub params: Vec<Nilai>,
    pub inst: Vec<Inst>,
    pub akhir: Option<Akhir>,
}

/// Satu titik masuk yang dikompilasi.
#[derive(Clone, Debug)]
pub struct Fungsi {
    pub blok: Vec<DataBlok>,
    /// Blok pertama: parameternya isi native posisi stack titik masuk (`Rencana::jenis_masuk`) yang skalar.
    pub masuk: Blok,
    /// Tipe tiap nilai, menurut nomornya.
    pub tipe: Vec<Tipe>,
    /// Posisi stack titik masuk yang menjadi parameter blok masuk, urut.
    pub posisi_masuk: Vec<usize>,
    /// Panjang stack terbesar di titik deopt mana pun (ukuran slot rekaman).
    pub maks: usize,
    /// Rumus method (`Rencana::ubah`) yang dirujuk `Inst::Rumus`.
    pub rumus: Vec<crate::jit::layak::Rumus>,
}

impl Fungsi {
    pub fn nilai_baru(&mut self, t: Tipe) -> Nilai {
        self.tipe.push(t);
        Nilai(self.tipe.len() as u32 - 1)
    }

    pub fn tipe(&self, v: Nilai) -> Tipe { self.tipe[v.0 as usize] }
}

impl fmt::Display for Nilai {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "v{}", self.0) }
}

impl fmt::Display for Blok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "b{}", self.0) }
}

fn daftar(v: &[Nilai]) -> String { v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ") }

impl fmt::Display for Keadaan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let isi: Vec<String> = self.isi.iter().map(|x| x.map_or("_".into(), |v| v.to_string())).collect();
        write!(f, "@{} [{}]", self.ip, isi.join(" "))?;
        for m in &self.maya { write!(f, " maya {}={}", m.pos, m.tag)?; }
        if let Some(i) = &self.induk { write!(f, " < fid {} +{} {i}", self.fid.unwrap_or(0), self.geser)?; }
        Ok(())
    }
}

impl fmt::Display for Fungsi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.blok.iter().enumerate() {
            let params: Vec<String> = b.params.iter().map(|p| format!("{p}:{:?}", self.tipe(*p))).collect();
            writeln!(f, "b{i} (ip {}) ({}):", b.ip, params.join(", "))?;
            for ins in &b.inst {
                let jaga = |j: &Option<Jaga>| j.as_ref().map_or(String::new(), |j| format!(" jaga{} {}", if j.spek { "-spek" } else { "" }, j.keadaan));
                match ins {
                    Inst::KonstI { hasil, nilai } => writeln!(f, "  {hasil} = {nilai}")?,
                    Inst::KonstF { hasil, nilai } => writeln!(f, "  {hasil} = {nilai:?}")?,
                    Inst::KonstL { hasil, nilai } => writeln!(f, "  {hasil} = {nilai}i128")?,
                    Inst::BinerL { hasil, op, a, b, jaga: j, jaga_muat } =>
                        writeln!(f, "  {hasil} = L{op:?} {a}, {b}{}{}", jaga(j), jaga(jaga_muat).replace("jaga", "jaga-muat"))?,
                    Inst::Sempit { hasil, a, jaga: j } => writeln!(f, "  {hasil} = sempit {a}{}", jaga(&Some(j.clone())))?,
                    Inst::TulisLebar { pos, nilai } => writeln!(f, "  tulis-lebar {nilai} -> {pos}")?,
                    Inst::Rumus { rumus, lokal, args, tulis, hasil, jaga: j } => writeln!(f, "  {}rumus #{rumus} lokal {lokal} {args:?}{}{}",
                        hasil.map_or(String::new(), |h| format!("{h} = ")), if *tulis { " tulis" } else { "" }, jaga(&Some(j.clone())))?,
                    Inst::Rantai { hasil, rantai, urutan, lokal, jaga: j, .. } =>
                        writeln!(f, "  {hasil} = rantai #{rantai}[{urutan}] lokal {lokal}{}", jaga(&Some(j.clone())))?,
                    Inst::Sebar { pos, pulang, jaga: j, .. } => writeln!(f, "  sebar {pos}{}{}", if *pulang { " pulang" } else { "" }, jaga(&Some(j.clone())))?,
                    Inst::BukaLebar { hasil, pos, jaga: j } => writeln!(f, "  {hasil} = buka-lebar {pos}{}", jaga(&Some(j.clone())))?,
                    Inst::Biner { hasil, op, a, b, jaga: j } => writeln!(f, "  {hasil} = {op:?} {a}, {b}{}", jaga(j))?,
                    Inst::Unar { hasil, op, a, jaga: j } => writeln!(f, "  {hasil} = {op:?} {a}{}", jaga(j))?,
                    Inst::Pilih { hasil, jika, ya, tidak } => writeln!(f, "  {hasil} = pilih {jika} ? {ya} : {tidak}")?,
                    Inst::JagaJika { kondisi, jaga: j } => writeln!(f, "  jaga {kondisi}{}", jaga(&Some(j.clone())))?,
                    Inst::Undi { hasil } => writeln!(f, "  {hasil} = undi")?,
                    Inst::Panggil { hasil, callee, args, di, sesudah } =>
                        writeln!(f, "  {hasil} = panggil #{callee}({}) ulang {di} deopt {sesudah}", daftar(args))?,
                    Inst::Muat { lokal, ke, pindah } => writeln!(f, "  {} lokal {lokal} -> {ke}", if *pindah { "ambil" } else { "muat" })?,
                    Inst::Simpan { ke, dari, nilai, kode, buang } =>
                        writeln!(f, "  simpan{} {dari}{} -> lokal {ke} (kode {kode})", if *buang { "-buang" } else { "" }, nilai.map_or(String::new(), |v| format!("={v}")))?,
                    Inst::Buang { pos } => writeln!(f, "  buang {pos}")?,
                    Inst::Benar { hasil, pos, lepas } => writeln!(f, "  {hasil} = benar {pos}{}", if *lepas { " lepas" } else { "" })?,
                    Inst::Taruh { pos, alamat } => writeln!(f, "  taruh {alamat:#x} -> {pos}")?,
                    Inst::Langkah { ip, t, tumpah, sesudah, isi, taruh, .. } =>
                        writeln!(f, "  langkah VM @{ip} t {t} tumpah {tumpah} -> {sesudah} isi {:?} taruh {}", isi, taruh.len())?,
                    Inst::Buka { hasil, pos, kode, jaga: j } => writeln!(f, "  {hasil} = buka {pos} (kode {kode}){}", jaga(&Some(j.clone())))?,
                    Inst::Bawaan { ip, t, argc, isi, .. } => writeln!(f, "  bawaan @{ip} t {t} argc {argc:#x} isi {isi:?}")?,
                    Inst::BinerVm { hasil, tambah, ip, isi, .. } =>
                        writeln!(f, "  {}biner-vm{} @{ip} isi {isi:?}", hasil.map_or(String::new(), |h| format!("{h} = ")), if *tambah { " +" } else { "" })?,
                    Inst::MuatUpval { idx, ke, jaga: j } => writeln!(f, "  muat tangkapan {idx} -> {ke}{}", jaga(&Some(j.clone())))?,
                    Inst::SimpanUpval { idx, dari, nilai, .. } =>
                        writeln!(f, "  simpan {dari}{} -> tangkapan {idx}", nilai.map_or(String::new(), |v| format!("={v}")))?,
                    Inst::TeksBulat { pos, n } => writeln!(f, "  teks {n} -> {pos}")?,
                    Inst::Pindah { dari, ke } => writeln!(f, "  pindah {dari} -> {ke}")?,
                    Inst::Bangun { t, n, isi, .. } => writeln!(f, "  bangun {n} dari {} isi {isi:?}", t - n)?,
                    Inst::Salin { dari, ke } => writeln!(f, "  salin {dari} -> {ke}")?,
                    Inst::Tukar { x, y } => writeln!(f, "  tukar {x} {y}")?,
                    Inst::CekMetode { pos, jaga: j, .. } => writeln!(f, "  cek-metode {pos}{}", jaga(&Some(j.clone())))?,
                    Inst::IterAda { hasil, slot, .. } => writeln!(f, "  {hasil} = iter-ada {slot}")?,
                    Inst::Maju { slot, .. } => writeln!(f, "  maju {slot}")?,
                    Inst::Wujud(m) => writeln!(f, "  wujud {} tag {} [{}] basi {:#x}", m.pos, m.tag, daftar(&m.muatan), m.basi)?,
                    Inst::Pembantu { pb, args, hasil, gagal, keluar } => writeln!(f, "  {}{pb:?} {args:?}{}{}",
                        hasil.map_or(String::new(), |h| format!("{h} = ")), gagal.as_ref().map_or(String::new(), |(u, _)| format!(" gagal {u:?}")),
                        keluar.map_or(String::new(), |k| format!(" -> {k}")))?,
                    Inst::Panjang { hasil, pos, lokal, .. } => writeln!(f, "  {hasil} = panjang {}{pos}", if *lokal { "lokal " } else { "" })?,
                    Inst::Langsung { n, ip, args, hasil, .. } => writeln!(f, "  {}langsung #{n} @{ip} args {args:?}",
                        hasil.as_ref().map_or(String::new(), |(h, k, _)| format!("{h} (kode {k}) = ")))?,
                    Inst::TulisVm { pos, nilai, kode } =>
                        writeln!(f, "  tulis-vm {}{pos} (kode {kode})", nilai.map_or(String::new(), |v| format!("{v} -> ")))?,
                }
            }
            match &b.akhir {
                Some(Akhir::Lompat { ke, args }) => writeln!(f, "  lompat {ke}({})", daftar(args))?,
                Some(Akhir::Cabang { kondisi, ya, ya_args, tidak, tidak_args }) =>
                    writeln!(f, "  cabang {kondisi} ? {ya}({}) : {tidak}({})", daftar(ya_args), daftar(tidak_args))?,
                Some(Akhir::Kembali { nilai, hasil }) => writeln!(f, "  kembali {nilai} ({hasil:?})")?,
                Some(Akhir::KembaliNilai { pos }) => writeln!(f, "  kembali posisi {pos}")?,
                Some(Akhir::KembaliUnit) => writeln!(f, "  kembali ()")?,
                Some(Akhir::CabangVm { ip, tujuan, .. }) => writeln!(f, "  cabang-vm @{ip} ? {}({}) : {}({})",
                    tujuan[0].0, daftar(&tujuan[0].1), tujuan[1].0, daftar(&tujuan[1].1))?,
                Some(Akhir::Keluar { keadaan }) => writeln!(f, "  keluar {keadaan}")?,
                None => writeln!(f, "  <tanpa akhir>")?,
            }
        }
        Ok(())
    }
}

/// Unit yang dicakup JIR diturunkan lewat JIR (bawaan sejak langkah 101); `JRG_JIT_JIR=mati` (`lama`, `off`) memakai
/// penurun langsung untuk semuanya. Yang ditolak JIR selalu lewat penurun langsung.
pub fn aktif() -> bool {
    static A: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *A.get_or_init(|| !std::env::var("JRG_JIT_JIR").is_ok_and(|v| matches!(v.as_str(), "mati" | "lama" | "off")))
}

/// Unit yang diturunkan lewat JIR di proses ini (untuk `JRG_JIT_LOG=ringkas` dan tes).
pub static UNIT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
