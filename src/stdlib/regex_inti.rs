// src/stdlib/regex_inti.rs — mesin regex JRG: pola diurai jadi program, dijalankan sebagai Pike VM.
// Waktunya linear (panjang teks × ukuran program), jadi pola dari pemakai tidak bisa membuatnya meledak
// (ReDoS). Semantiknya leftmost-first seperti Perl/Python: alternatif dan pengulangan dicoba urut prioritas.

/// Batas pengulangan `{n,m}` dan ukuran program; di atasnya pola ditolak, bukan dijalankan lambat.
pub const MAKS_ULANG: usize = 1000;
pub const MAKS_PROGRAM: usize = 100_000;
/// Batas sarang grup, seperti Go sesudah CVE-2022-24921: pengurainya rekursif per `(`.
pub const MAKS_SARANG: usize = 1000;

#[derive(Debug, Clone)]
enum Node {
    Literal(char),
    AnyChar,
    Class(Vec<(char, char)>, bool),
    Awal,
    Akhir,
    BatasKata(bool),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Node>, usize, Option<usize>, bool),
    Empty,
}

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Any,
    Class(Vec<(char, char)>, bool),
    Awal,
    Akhir,
    BatasKata(bool),
    Split(usize, usize),
    Jmp(usize),
    Match,
}

/// Pola yang sudah dikompilasi.
#[derive(Debug, Clone)]
pub struct Regex { prog: Vec<Inst> }

const DIGIT: &[(char, char)] = &[('0', '9')];
const KATA: &[(char, char)] = &[('0', '9'), ('A', 'Z'), ('_', '_'), ('a', 'z')];
const SPASI: &[(char, char)] = &[('\t', '\r'), (' ', ' ')];

struct Urai<'a> { c: &'a [char], p: usize, sarang: usize }

impl<'a> Urai<'a> {
    fn lihat(&self) -> Option<char> { self.c.get(self.p).copied() }

    fn alt(&mut self) -> Result<Node, String> {
        let mut alts = vec![self.concat()?];
        while self.lihat() == Some('|') {
            self.p += 1;
            alts.push(self.concat()?);
        }
        Ok(if alts.len() == 1 { alts.pop().unwrap() } else { Node::Alt(alts) })
    }

    fn concat(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(c) = self.lihat() {
            if c == '|' || c == ')' { break; }
            if matches!(c, '*' | '+' | '?') || (c == '{' && self.kuantor_kurawal_di(self.p)) {
                return Err(format!("`{}` di posisi {} tidak punya sesuatu untuk diulang", c, self.p));
            }
            let atom = self.atom()?;
            items.push(self.kuantor(atom)?);
        }
        Ok(if items.len() == 1 { items.pop().unwrap() } else if items.is_empty() { Node::Empty } else { Node::Concat(items) })
    }

    /// `{` memulai pengulangan cuma kalau bentuknya `{n}`, `{n,}` atau `{n,m}`; selain itu huruf biasa.
    fn kuantor_kurawal_di(&self, p: usize) -> bool {
        let mut i = p + 1;
        let digit = |i: &mut usize| { let a = *i; while self.c.get(*i).map_or(false, |c| c.is_ascii_digit()) { *i += 1; } *i > a };
        let ada_min = digit(&mut i);
        let mut ada_max = false;
        if self.c.get(i) == Some(&',') { i += 1; ada_max = digit(&mut i); }
        else if !ada_min { return false; }
        (ada_min || ada_max) && self.c.get(i) == Some(&'}')
    }

    fn angka(&mut self) -> Result<usize, String> {
        let awal = self.p;
        while self.lihat().map_or(false, |c| c.is_ascii_digit()) { self.p += 1; }
        let t: String = self.c[awal..self.p].iter().collect();
        t.parse::<usize>().ok().filter(|n| *n <= MAKS_ULANG)
            .ok_or_else(|| format!("jumlah pengulangan `{}` di atas batas {}", t, MAKS_ULANG))
    }

    fn kuantor(&mut self, atom: Node) -> Result<Node, String> {
        let posisi = self.p;
        let (min, max) = match self.lihat() {
            Some('*') => { self.p += 1; (0, None) }
            Some('+') => { self.p += 1; (1, None) }
            Some('?') => { self.p += 1; (0, Some(1)) }
            Some('{') if self.kuantor_kurawal_di(self.p) => {
                self.p += 1;
                let min = if self.lihat() == Some(',') { 0 } else { self.angka()? };
                let max = if self.lihat() == Some(',') {
                    self.p += 1;
                    if self.lihat() == Some('}') { None } else { Some(self.angka()?) }
                } else { Some(min) };
                self.p += 1;
                if let Some(m) = max {
                    if m < min { return Err(format!("pengulangan `{{{},{}}}` terbalik: batas atasnya lebih kecil", min, m)); }
                }
                (min, max)
            }
            _ => return Ok(atom),
        };
        if matches!(atom, Node::Awal | Node::Akhir | Node::BatasKata(_)) {
            return Err(format!("jangkar di posisi {} tidak bisa diulang", posisi));
        }
        let rakus = if self.lihat() == Some('?') { self.p += 1; false } else { true };
        if let Some(c) = self.lihat() {
            if matches!(c, '*' | '+' | '?') || (c == '{' && self.kuantor_kurawal_di(self.p)) {
                return Err(format!("pengulangan bertumpuk di posisi {} — bungkus dengan `(?:...)` kalau memang dimaksud", self.p));
            }
        }
        Ok(Node::Repeat(Box::new(atom), min, max, rakus))
    }

    fn atom(&mut self) -> Result<Node, String> {
        let c = self.lihat().unwrap();
        self.p += 1;
        Ok(match c {
            '.' => Node::AnyChar,
            '^' => Node::Awal,
            '$' => Node::Akhir,
            '(' => {
                if self.lihat() == Some('?') {
                    if self.c.get(self.p + 1) == Some(&':') { self.p += 2; }
                    else {
                        let s: String = self.c[self.p - 1..(self.p + 2).min(self.c.len())].iter().collect();
                        return Err(format!("konstruksi `{}` tidak didukung — yang ada cuma grup `(...)` dan `(?:...)`", s));
                    }
                }
                if self.sarang >= MAKS_SARANG {
                    return Err(format!("grup di posisi {} bersarang lebih dari {} tingkat", self.p - 1, MAKS_SARANG));
                }
                self.sarang += 1;
                let isi = self.alt()?;
                self.sarang -= 1;
                if self.lihat() != Some(')') { return Err("grup `(` tidak pernah ditutup `)`".to_string()); }
                self.p += 1;
                // `(\b)?` sah, `\b?` tidak: jangkar di dalam grup dibungkus supaya boleh diulang.
                if matches!(isi, Node::Awal | Node::Akhir | Node::BatasKata(_)) { Node::Concat(vec![isi]) } else { isi }
            }
            '[' => self.kelas()?,
            '\\' => self.escape_luar()?,
            c => Node::Literal(c),
        })
    }

    fn escape_luar(&mut self) -> Result<Node, String> {
        let c = self.lihat().ok_or("pola berakhir dengan `\\` menggantung")?;
        self.p += 1;
        Ok(match c {
            'd' => Node::Class(DIGIT.to_vec(), false),
            'D' => Node::Class(DIGIT.to_vec(), true),
            'w' => Node::Class(KATA.to_vec(), false),
            'W' => Node::Class(KATA.to_vec(), true),
            's' => Node::Class(SPASI.to_vec(), false),
            'S' => Node::Class(SPASI.to_vec(), true),
            'b' => Node::BatasKata(true),
            'B' => Node::BatasKata(false),
            'A' => Node::Awal,
            'z' => Node::Akhir,
            c => Node::Literal(Self::escape_huruf(c)?),
        })
    }

    /// Escape yang berarti satu karakter; huruf dan angka lain DITOLAK, bukan dibaca sebagai dirinya.
    fn escape_huruf(c: char) -> Result<char, String> {
        Ok(match c {
            'n' => '\n', 'r' => '\r', 't' => '\t', 'f' => '\x0c', 'v' => '\x0b', '0' => '\0',
            c if c.is_ascii_digit() => return Err(format!(
                "rujukan balik `\\{}` tidak didukung — mesin regex JRG berwaktu linear", c)),
            c if c.is_alphanumeric() => return Err(format!("escape `\\{}` tidak dikenal", c)),
            c => c,
        })
    }

    fn kelas(&mut self) -> Result<Node, String> {
        let neg = self.lihat() == Some('^');
        if neg { self.p += 1; }
        let mut items: Vec<(char, char)> = Vec::new();
        let mut pertama = true;
        loop {
            let c = self.lihat().ok_or("kelas karakter `[` tidak pernah ditutup `]`")?;
            if c == ']' && !pertama { self.p += 1; break; }
            pertama = false;
            let lo = match self.butir_kelas()? {
                Ok(ch) => ch,
                Err(set) => { items.extend_from_slice(set); continue; }
            };
            if self.lihat() == Some('-') && self.c.get(self.p + 1).map_or(false, |c| *c != ']') {
                self.p += 1;
                let hi = match self.butir_kelas()? {
                    Ok(ch) => ch,
                    Err(_) => return Err("rentang kelas tidak bisa berujung `\\d`/`\\w`/`\\s`".to_string()),
                };
                if hi < lo { return Err(format!("rentang kelas `{}-{}` terbalik", lo, hi)); }
                items.push((lo, hi));
            } else {
                items.push((lo, lo));
            }
        }
        Ok(Node::Class(items, neg))
    }

    /// Satu butir di dalam `[...]`: sebuah karakter, atau himpunan `\d`/`\w`/`\s`.
    fn butir_kelas(&mut self) -> Result<Result<char, &'static [(char, char)]>, String> {
        let c = self.lihat().unwrap();
        self.p += 1;
        if c != '\\' { return Ok(Ok(c)); }
        let e = self.lihat().ok_or("pola berakhir dengan `\\` menggantung")?;
        self.p += 1;
        Ok(match e {
            'd' => Err(DIGIT),
            'w' => Err(KATA),
            's' => Err(SPASI),
            'D' | 'W' | 'S' | 'b' | 'B' => return Err(format!("`\\{}` tidak didukung di dalam `[...]`", e)),
            e => Ok(Self::escape_huruf(e)?),
        })
    }
}

struct Kompil { prog: Vec<Inst> }

impl Kompil {
    fn tambah(&mut self, i: Inst) -> Result<usize, String> {
        if self.prog.len() >= MAKS_PROGRAM {
            return Err(format!("polanya terlalu besar sesudah pengulangannya dibuka (lebih dari {} langkah)", MAKS_PROGRAM));
        }
        self.prog.push(i);
        Ok(self.prog.len() - 1)
    }

    fn node(&mut self, n: &Node) -> Result<(), String> {
        match n {
            Node::Literal(c) => { self.tambah(Inst::Char(*c))?; }
            Node::AnyChar => { self.tambah(Inst::Any)?; }
            Node::Class(r, neg) => { self.tambah(Inst::Class(r.clone(), *neg))?; }
            Node::Awal => { self.tambah(Inst::Awal)?; }
            Node::Akhir => { self.tambah(Inst::Akhir)?; }
            Node::BatasKata(b) => { self.tambah(Inst::BatasKata(*b))?; }
            Node::Empty => {}
            Node::Concat(xs) => for x in xs { self.node(x)?; },
            Node::Alt(xs) => {
                let mut lompat = Vec::new();
                for (i, x) in xs.iter().enumerate() {
                    if i + 1 < xs.len() {
                        let s = self.tambah(Inst::Split(0, 0))?;
                        self.node(x)?;
                        lompat.push(self.tambah(Inst::Jmp(0))?);
                        let lanjut = self.prog.len();
                        self.prog[s] = Inst::Split(s + 1, lanjut);
                    } else {
                        self.node(x)?;
                    }
                }
                let akhir = self.prog.len();
                for j in lompat { self.prog[j] = Inst::Jmp(akhir); }
            }
            Node::Repeat(x, min, max, rakus) => {
                for _ in 0..*min { self.node(x)?; }
                match max {
                    None => {
                        let s = self.tambah(Inst::Split(0, 0))?;
                        self.node(x)?;
                        self.tambah(Inst::Jmp(s))?;
                        let keluar = self.prog.len();
                        self.prog[s] = if *rakus { Inst::Split(s + 1, keluar) } else { Inst::Split(keluar, s + 1) };
                    }
                    Some(m) => {
                        let mut celah = Vec::new();
                        for _ in *min..*m {
                            celah.push(self.tambah(Inst::Split(0, 0))?);
                            self.node(x)?;
                        }
                        let keluar = self.prog.len();
                        for s in celah {
                            self.prog[s] = if *rakus { Inst::Split(s + 1, keluar) } else { Inst::Split(keluar, s + 1) };
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn kata(c: Option<&char>) -> bool { c.map_or(false, |c| c.is_ascii_alphanumeric() || *c == '_') }

impl Regex {
    pub fn baru(pola: &str) -> Result<Regex, String> {
        let c: Vec<char> = pola.chars().collect();
        let mut u = Urai { c: &c, p: 0, sarang: 0 };
        let n = u.alt()?;
        if u.p < c.len() {
            return Err(format!("ada `{}` di posisi {} yang tidak punya pembuka", c[u.p], u.p));
        }
        let mut k = Kompil { prog: Vec::new() };
        k.node(&n)?;
        k.tambah(Inst::Match)?;
        Ok(Regex { prog: k.prog })
    }

    /// Tambah utas di `pc` beserta semua langkah tanpa karakter, urut prioritas.
    fn tambah_utas(&self, daftar: &mut Vec<(usize, usize)>, ada: &mut [usize], tanda: usize,
                   pc: usize, mulai: usize, t: &[char], pos: usize, tumpuk: &mut Vec<usize>) {
        tumpuk.clear();
        tumpuk.push(pc);
        while let Some(pc) = tumpuk.pop() {
            if ada[pc] == tanda { continue; }
            ada[pc] = tanda;
            match &self.prog[pc] {
                Inst::Jmp(x) => tumpuk.push(*x),
                Inst::Split(x, y) => { tumpuk.push(*y); tumpuk.push(*x); }
                Inst::Awal => if pos == 0 { tumpuk.push(pc + 1) },
                Inst::Akhir => if pos == t.len() { tumpuk.push(pc + 1) },
                Inst::BatasKata(b) => {
                    let batas = kata(if pos > 0 { t.get(pos - 1) } else { None }) != kata(t.get(pos));
                    if batas == *b { tumpuk.push(pc + 1) }
                }
                _ => daftar.push((pc, mulai)),
            }
        }
    }

    /// Kecocokan paling kiri yang mulai di `dari` atau sesudahnya, dalam indeks karakter.
    pub fn cari_dari(&self, t: &[char], dari: usize) -> Option<(usize, usize)> { self.cari(t, dari, None) }

    /// `tolak`: kecocokan KOSONG di posisi itu tidak dihitung (aturan `finditer` Python sejak 3.7).
    fn cari(&self, t: &[char], dari: usize, tolak: Option<usize>) -> Option<(usize, usize)> {
        let n = self.prog.len();
        let mut kini: Vec<(usize, usize)> = Vec::new();
        let mut depan: Vec<(usize, usize)> = Vec::new();
        let mut ada = vec![usize::MAX; n];
        let mut tumpuk = Vec::new();
        let mut hasil: Option<(usize, usize)> = None;
        let mut pos = dari;
        loop {
            let tanda = pos * 2;
            if hasil.is_none() {
                self.tambah_utas(&mut kini, &mut ada, tanda, 0, pos, t, pos, &mut tumpuk);
            }
            if kini.is_empty() && (hasil.is_some() || pos >= t.len()) { break; }
            let tanda_depan = pos * 2 + 2;
            depan.clear();
            for i in 0..kini.len() {
                let (pc, mulai) = kini[i];
                let lolos = match &self.prog[pc] {
                    Inst::Match if tolak == Some(pos) && mulai == pos => false,
                    Inst::Match => { hasil = Some((mulai, pos)); break; }
                    Inst::Char(c) => t.get(pos) == Some(c),
                    Inst::Any => t.get(pos).map_or(false, |c| *c != '\n'),
                    Inst::Class(r, neg) => t.get(pos).map_or(false, |c| r.iter().any(|(a, b)| a <= c && c <= b) != *neg),
                    _ => false,
                };
                if lolos {
                    self.tambah_utas(&mut depan, &mut ada, tanda_depan, pc + 1, mulai, t, pos + 1, &mut tumpuk);
                }
            }
            if pos >= t.len() {
                // Utas yang tersisa cuma bisa selesai di `Match` pada posisi ini; sudah diperiksa di atas.
                break;
            }
            std::mem::swap(&mut kini, &mut depan);
            pos += 1;
        }
        hasil
    }

    /// Semua kecocokan tak bertumpuk; sesudah kecocokan kosong di `p`, yang tak-kosong di `p` masih boleh.
    pub fn semua(&self, t: &[char]) -> Vec<(usize, usize)> {
        let mut keluar = Vec::new();
        let mut pos = 0;
        let mut tolak: Option<usize> = None;
        while pos <= t.len() {
            match self.cari(t, pos, tolak) {
                Some((a, b)) => { keluar.push((a, b)); pos = b; tolak = if b > a { None } else { Some(b) }; }
                None => break,
            }
        }
        keluar
    }
}

#[cfg(test)]
mod uji {
    use super::{Regex, MAKS_SARANG};

    fn semua(p: &str, t: &str) -> Vec<(usize, usize)> {
        let c: Vec<char> = t.chars().collect();
        Regex::baru(p).unwrap().semua(&c)
    }
    fn cocok(p: &str, t: &str) -> bool { !semua(p, t).is_empty() }

    #[test]
    fn mundur_ke_dalam_pengulangan_dan_alternatif() {
        // Mesin lama tidak pernah mundur: keempatnya dulu `false`.
        assert!(cocok("a*a", "aaa"));
        assert!(cocok("^(a|ab)c$", "abc"));
        assert!(cocok("^.*x$", "abx"));
        assert!(cocok("^[0-9]+0$", "100"));
    }

    #[test]
    fn malas_dan_rakus() {
        assert_eq!(semua("a+?", "aaa"), vec![(0, 1), (1, 2), (2, 3)]);
        assert_eq!(semua("a+", "aaa"), vec![(0, 3)]);
        assert_eq!(semua("<.*?>", "<a><b>"), vec![(0, 3), (3, 6)]);
    }

    #[test]
    fn escape_bermakna_atau_ditolak() {
        assert!(cocok("a\\nb", "a\nb"));
        assert!(cocok("^[\\d]$", "7") && !cocok("^[\\d]$", "d"));
        assert_eq!(semua("\\bab\\b", "ab xab ab"), vec![(0, 2), (7, 9)]);
        for salah in ["\\q", "\\1", "a**", "*a", "a{3,1}", "[z-a]", "(?=a)", "(a", "a)", "a{1001}"] {
            assert!(Regex::baru(salah).is_err(), "{}", salah);
        }
        assert!(cocok("^(?:ab)+$", "abab"));
        assert!(cocok("^a{,2}$", "aa") && cocok("a{x}", "a{x}"));
    }

    #[test]
    fn kecocokan_kosong_seperti_python() {
        assert_eq!(semua("x*", "abc"), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
        assert_eq!(semua("a*", "baaac"), vec![(0, 0), (1, 4), (4, 4), (5, 5)]);
        assert_eq!(semua("$", "ab"), vec![(2, 2)]);
    }

    #[test]
    fn pola_redos_tetap_linear() {
        // Di mesin mundur-eksponensial ini tidak selesai; di sini satu lintasan.
        let t: String = "a".repeat(20000) + "b";
        assert!(!cocok("^(a+)+$", &t));
        assert!(!cocok("^(a|a)*$", &t));
        assert!(Regex::baru("((a{1000}){1000}){1000}").is_err());
    }

    #[test]
    fn sarang_dalam_ditolak_tanpa_menghabiskan_stack() {
        // Stack 64 MB seperti thread program (256 MB); frame debug tak muat di 2 MB bawaan tes.
        std::thread::Builder::new().stack_size(64 << 20).spawn(|| {
            let dalam = |n: usize| "(".repeat(n) + "a" + &")".repeat(n);
            assert!(cocok(&dalam(MAKS_SARANG), "a"));
            let e = Regex::baru(&dalam(1_000_000)).unwrap_err();
            assert!(e.contains("bersarang lebih dari 1000 tingkat"), "{}", e);
        }).unwrap().join().unwrap();
    }
}
