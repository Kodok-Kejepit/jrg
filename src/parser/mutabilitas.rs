// src/parser/mutabilitas.rs — src/parser/mutabilitas.rs — `let` mengikat, `var` bisa diubah.

use std::collections::HashMap;

use super::{
    AssignOp, AssignStmt, Block, Expr, FnBody, FnDecl, FnParam, ForStmt, IfStmt, ImplItem, Item, MatchArm,
    MatchBody, Pattern, SelectCase, Span, Stmt, TemplatePart, TraitItem,
};

/// Bagaimana sebuah nama boleh diperlakukan.
#[derive(Clone, Copy, PartialEq)]
enum Ikat {
    /// Tidak boleh ditugasi ulang. Menyimpan kata kunci yang membuatnya,
    /// supaya pesannya bisa menyebut apa yang harus diubah pemakainya.
    Mengikat(&'static str),
    BisaUbah,
}

/// Satu nama di satu lingkup: aturannya, dan (untuk lokal ber-kata-kunci) tempat deklarasinya
/// serta apakah ia pernah dibaca — bahan `jrgm lint`.
struct Entri { ikat: Ikat, deklarasi: Option<Span>, dibaca: bool }

pub struct Pemeriksa {
    /// Tumpukan lingkup. Nama yang sama di lingkup lebih dalam MENUTUPI yang
    /// di luar (shadowing), jadi `let x = 1` di dalam blok tidak membuat
    /// `x` milik luar ikut terkunci.
    lingkup: Vec<HashMap<String, Entri>>,
    /// Lokal `let`/`var`/`const`/`:=` yang tidak pernah dibaca sebelum lingkupnya ditutup.
    pub tak_dibaca: Vec<(String, Span)>,
    /// Pola yang sedang diikat milik `let` (dicatat untuk lint), bukan parameter atau lengan.
    pola_lokal: bool,
    pub galat: Vec<(String, Span)>,
    /// Temuan "kata kunci ini tidak berpengaruh", dikumpulkan di penelusuran
    /// yang sama supaya tidak ada lintasan kedua.
    pub belum_ada: Vec<(String, Span)>,
    /// Temuan "builtin ini kebanyakan argumen" — jenis KETIGA dari
    /// penelusuran yang sama, dengan alasan yang sama.
    pub arity: Vec<(String, Span)>,
    /// Mati di berkas yang punya `import`. Lihat catatan di pemanggilnya.
    pub periksa_arity: bool,
    /// Fungsi tingkat atas berkas ini: (paling sedikit, paling banyak, nama parameter).
    pub fungsi_atas: HashMap<String, (usize, Option<usize>, Vec<String>)>,
    /// Panggilan langsung ke fungsi itu dengan jumlah argumen di luar batas — cuma untuk
    /// `--periksa`: saat jalan kalimatnya sama, dan baru muncul di baris itu.
    pub aritas_pemakai: Vec<(String, Span)>,
    /// Nama tipe yang dideklarasikan berkas ini; `E::A(1)` bukan builtin.
    pub tipe_sendiri: std::collections::HashSet<String>,
    /// Varian unit yang ditulis telanjang di pola: menguji, bukan mengikat.
    pub varian_unit: std::collections::HashSet<String>,
    /// Nama varian yang dimiliki lebih dari satu enum berkas ini, dengan pemiliknya.
    pub varian_ganda: HashMap<String, Vec<String>>,
}

impl Pemeriksa {
    pub fn baru() -> Self {
        Pemeriksa {
            lingkup: vec![HashMap::new()], tak_dibaca: Vec::new(), pola_lokal: false, galat: Vec::new(), belum_ada: Vec::new(),
            arity: Vec::new(), periksa_arity: false, fungsi_atas: HashMap::new(), aritas_pemakai: Vec::new(),
            tipe_sendiri: std::collections::HashSet::new(),
            varian_unit: ["None".to_string()].into_iter().collect(),
            varian_ganda: HashMap::new(),
        }
    }

    /// Varian yang ditulis telanjang padahal dua enum memilikinya: dulu dua mesin memilih satu
    /// diam-diam (nilai `K` enum terakhir, pola `K` dan `V { .. }` enum pertama).
    fn cek_ganda(&mut self, nama: &str, span: Span) {
        if let Some(pemilik) = self.varian_ganda.get(nama) {
            self.galat.push((format!(
                "`{}` ambigu — varian bernama itu ada di {}\n  bantuan: tulis lengkap, mis. `{}::{}`",
                nama, pemilik.join(" dan "), pemilik[0], nama), span));
        }
    }

    fn cek_ganda_tipe(&mut self, ty: &super::TypeExpr, span: Span) {
        if let super::TypeExpr::Named(n) = ty {
            if n.package.is_none() && n.modul.is_none() && !self.tipe_sendiri.contains(&n.name) {
                self.cek_ganda(&n.name.clone(), span);
            }
        }
    }

    fn masuk(&mut self) { self.lingkup.push(HashMap::new()); }
    fn keluar(&mut self) {
        if let Some(l) = self.lingkup.pop() {
            for (nama, e) in l { self.catat_tak_dibaca(nama, &e) }
        }
    }

    fn catat_tak_dibaca(&mut self, nama: String, e: &Entri) {
        if let (Some(sp), false) = (e.deklarasi, e.dibaca) {
            if !nama.starts_with('_') { self.tak_dibaca.push((nama, sp)) }
        }
    }

    fn ikat(&mut self, nama: &str, k: Ikat) { self.ikat_di(nama, k, None) }

    fn ikat_di(&mut self, nama: &str, k: Ikat, deklarasi: Option<Span>) {
        let lama = self.lingkup.last_mut()
            .and_then(|l| l.insert(nama.to_string(), Entri { ikat: k, deklarasi, dibaca: false }));
        if let Some(e) = lama { self.catat_tak_dibaca(nama.to_string(), &e) }
    }

    fn cari(&self, nama: &str) -> Option<Ikat> {
        self.lingkup.iter().rev().find_map(|l| l.get(nama).map(|e| e.ikat))
    }

    /// Tandai pembacaan; `false` kalau nama itu bukan milik lingkup mana pun.
    fn baca(&mut self, nama: &str) -> bool {
        match self.lingkup.iter_mut().rev().find_map(|l| l.get_mut(nama)) {
            Some(e) => { e.dibaca = true; true }
            None => false,
        }
    }

    fn lapor(&mut self, nama: &str, kata: &str, span: Span) {
        self.galat.push((
            format!(
                "`{}` dideklarasikan dengan `{}`, jadi ia tidak bisa ditugasi ulang.\n  \
                 bantuan: pakai `{}` kalau nilainya memang harus berubah\n  \
                 catatan: `let` dan `const` mengikat nilainya; `var` dan `:=` tidak",
                nama,
                kata,
                if kata == "const" { "var" } else { "var` atau `let mut" }
            ),
            span,
        ));
    }

    // ── Pola ──────────────────────────────────────────────────────────────

    /// Daftarkan tiap nama yang diikat sebuah pola.
    fn ikat_pola(&mut self, p: &Pattern, bawaan: Ikat) {
        match p {
            Pattern::Ident { name, is_mut, span } => {
                if self.varian_unit.contains(name) { self.cek_ganda(name, *span); }
                // Nama varian enum unit (`None`, `Ok`) ditulis seperti pengikat biasa tapi
                // tidak mengikat apa pun.
                let k = if *is_mut { Ikat::BisaUbah } else { bawaan };
                let d = self.pola_lokal.then_some(*span);
                self.ikat_di(name, k, d);
            }
            Pattern::Tuple { elems, .. } => for e in elems { self.ikat_pola(e, bawaan) },
            Pattern::Struct { ty, fields, span } => {
                self.cek_ganda_tipe(ty, *span);
                for f in fields { self.ikat_pola(&f.pat, bawaan) }
            }
            Pattern::Enum { ty, variant, inner, span } => {
                if matches!(ty, super::TypeExpr::Named(n) if n.name == *variant) { self.cek_ganda_tipe(ty, *span); }
                if let Some(i) = inner { self.ikat_pola(i, bawaan) }
            }
            Pattern::Ref { inner, .. } => self.ikat_pola(inner, bawaan),
            Pattern::Or { pats, span } => {
                self.cek_alternatif(pats, *span);
                for p in pats { self.ikat_pola(p, bawaan) }
            }
            Pattern::Array { before, rest, after, .. } => {
                for p in before { self.ikat_pola(p, bawaan) }
                if let Some(Some(nama)) = rest { self.ikat(nama, bawaan) }
                for p in after { self.ikat_pola(p, bawaan) }
            }
            Pattern::Binding { name, inner, .. } => {
                self.ikat(name, bawaan);
                self.ikat_pola(inner, bawaan);
            }
            Pattern::Literal { .. } | Pattern::Range { .. } | Pattern::Wildcard { .. } => {}
            _ => {}
        }
    }

    // ── Sasaran penugasan ─────────────────────────────────────────────────

    /// Nama akar sebuah jalur penugasan: `a`, `a[0]`, `a.f.g`, `a[i].f`.
    fn akar<'a>(e: &'a Expr) -> Option<(&'a str, Span)> {
        match e {
            Expr::Ident(name, span) => Some((name.as_str(), *span)),
            Expr::Index { obj, .. } => Self::akar(obj),
            Expr::Field { obj, .. } => Self::akar(obj),
            Expr::Slice { obj, .. } => Self::akar(obj),
            _ => None,
        }
    }

    fn periksa_sasaran(&mut self, e: &Expr) {
        let Some((nama, span)) = Self::akar(e) else { return };
        if let Some(Ikat::Mengikat(kata)) = self.cari(nama) {
            let nama = nama.to_string();
            self.lapor(&nama, kata, span);
        }
    }

    /// Nama yang diikat sebuah pola, tanpa varian unit dan `_`.
    fn nama_pola(&self, p: &Pattern, out: &mut std::collections::BTreeSet<String>) {
        match p {
            Pattern::Ident { name, .. } => {
                if name != "_" && !self.varian_unit.contains(name) { out.insert(name.clone()); }
            }
            Pattern::Binding { name, inner, .. } => {
                if name != "_" { out.insert(name.clone()); }
                self.nama_pola(inner, out);
            }
            Pattern::Tuple { elems, .. } => for e in elems { self.nama_pola(e, out) },
            Pattern::Struct { fields, .. } => for f in fields { self.nama_pola(&f.pat, out) },
            Pattern::Enum { inner: Some(i), .. } | Pattern::Ref { inner: i, .. } => self.nama_pola(i, out),
            // Alternatif bersarang sudah diperiksa sendiri; namanya cukup diambil dari yang pertama.
            Pattern::Or { pats, .. } => if let Some(p) = pats.first() { self.nama_pola(p, out) },
            Pattern::Array { before, rest, after, .. } => {
                for x in before.iter().chain(after) { self.nama_pola(x, out) }
                if let Some(Some(n)) = rest { out.insert(n.clone()); }
            }
            _ => {}
        }
    }

    /// `(a, 1) | (2, b) => b`: kalau alternatif pertama yang cocok, `b` tidak punya nilai.
    fn cek_alternatif(&mut self, alts: &[Pattern], span: Span) {
        let mut set: Vec<std::collections::BTreeSet<String>> = Vec::new();
        for a in alts {
            let mut s = std::collections::BTreeSet::new();
            self.nama_pola(a, &mut s);
            set.push(s);
        }
        let Some(pertama) = set.first() else { return };
        for (i, s) in set.iter().enumerate().skip(1) {
            if s == pertama { continue; }
            let (nama, ada) = match pertama.symmetric_difference(s).next() {
                Some(n) => (n.clone(), if pertama.contains(n) { 1 } else { i + 1 }),
                None => continue,
            };
            let tidak = if ada == 1 { i + 1 } else { 1 };
            self.galat.push((format!(
                "`{}` diikat di alternatif ke-{} tapi tidak di alternatif ke-{} dari pola `|` ini\n  \
                 bantuan: kalau alternatif yang lain yang cocok, `{}` tidak punya nilai — ikat nama \
                 yang sama di tiap alternatif, atau pisahkan jadi dua lengan",
                nama, ada, tidak, nama), span));
            return;
        }
    }

    // ── Penelusuran ───────────────────────────────────────────────────────

    pub fn berkas(&mut self, items: &[Item]) {
        // Dua lintasan atas item tingkat atas: nama modul-level harus sudah
        // terdaftar sebelum badan fungsi mana pun ditelusuri, karena sebuah
        // fungsi boleh memakai `const` yang dideklarasikan di bawahnya.
        for it in items {
            match it {
                Item::Const(c) => self.ikat(&c.name, Ikat::Mengikat(c.kata)),
                Item::Var(v) => self.ikat(&v.name, Ikat::BisaUbah),
                _ => {}
            }
        }
        for it in items {
            match it {
                // Nilai awal tingkat atas juga ekspresi: `const A = _` sama salahnya.
                Item::Const(c) => self.ekspr(&c.value),
                Item::Var(v) => if let Some(i) = &v.value { self.ekspr(i) },
                Item::Fn(f) => self.fungsi(f),
                Item::Impl(i) => for x in &i.items {
                    if let ImplItem::Method(m) = x { self.fungsi(m) }
                },
                Item::Trait(t) => for x in &t.items {
                    let (TraitItem::Method(m) | TraitItem::DefaultMethod(m)) = x;
                    self.fungsi(m);
                },
                _ => {}
            }
        }
    }

    fn fungsi(&mut self, f: &FnDecl) {
        if f.is_async {
            self.belum_ada.push((PESAN_ASYNC.to_string(), f.span));
        }
        self.masuk();
        self.parameter(&f.params);
        match &*f.body {
            FnBody::Block(b) => self.blok(b),
            FnBody::Expr(e) => self.ekspr(e),
            FnBody::Abstrak => {}
        }
        self.keluar();
    }

    fn blok(&mut self, b: &Block) {
        self.masuk();
        // `fn` bersarang di-hoist per blok, jadi namanya membayangi fungsi tingkat atas sejak awal blok.
        for st in &b.stmts { if let Stmt::Fn(f) = st { self.ikat(&f.name, Ikat::BisaUbah) } }
        for s in &b.stmts { self.stmt(s) }
        if let Some(t) = &b.tail { self.ekspr(t) }
        self.keluar();
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l) => {
                self.ekspr(&l.init);
                let k = if l.is_mut { Ikat::BisaUbah } else { Ikat::Mengikat("let") };
                self.pola_lokal = true;
                self.ikat_pola(&l.pattern, k);
                self.pola_lokal = false;
            }
            Stmt::Var(v) => {
                if let Some(i) = &v.init { self.ekspr(i) }
                self.ikat_di(&v.name, Ikat::BisaUbah, Some(v.span));
            }
            Stmt::Const(c) => {
                self.ekspr(&c.value);
                self.ikat_di(&c.name, Ikat::Mengikat("const"), Some(c.span));
            }
            Stmt::ShortDecl(sd) => {
                for v in &sd.values { self.ekspr(v) }
                for n in &sd.names { self.ikat_di(n, Ikat::BisaUbah, Some(sd.span)) }
            }
            Stmt::Assign(a) => self.penugasan(a),
            Stmt::Expr(e) => self.ekspr(e),
            Stmt::Return(r) => for v in &r.values { self.ekspr(v) },
            Stmt::If(i) => self.percabangan(i),
            Stmt::While(w) => { self.ekspr(&w.cond); self.blok(&w.body); }
            Stmt::Loop(l) => self.blok(&l.body),
            Stmt::For(f) => self.perulangan(f),
            Stmt::Match(m) => self.cocok(&m.scrutinee, &m.arms),
            Stmt::Block(b) => self.blok(b),
            Stmt::Spawn(sp) => {
                if sp.is_async {
                    self.belum_ada.push((PESAN_ASYNC.to_string(), sp.span));
                }
                self.masuk();
                if let Some(ps) = &sp.params { self.parameter(ps) }
                self.blok(&sp.body);
                self.keluar();
            }
            Stmt::Defer(d) => self.ekspr(&d.expr),
            Stmt::Select(sel) => for c in &sel.cases { self.pilih(c) },
            Stmt::Fn(f) => self.fungsi(f),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Error(_) => {}
        }
    }

    fn penugasan(&mut self, a: &AssignStmt) {
        for v in &a.values { self.ekspr(v) }
        for t in &a.targets {
            // `_ = …` membuang nilainya; `_ += 1` dan `_[0] = 1` MEMBACA `_`.
            if let Some(("_", span)) = Self::akar(t) {
                if !(matches!(t, Expr::Ident(..)) && matches!(a.op, AssignOp::Eq)) {
                    self.galat.push((PESAN_BACA_PEMBUANG.to_string(), span));
                }
                continue;
            }
            // Sisi kiri tetap ditelusuri sebagai ekspresi (indeksnya bisa
            // memuat pemanggilan), lalu akarnya diperiksa.
            self.ekspr_dalam_sasaran(t);
            self.periksa_sasaran(t);
            // `x += 1` membaca `x`; `x = 1` dan `x[0] = 1` cuma menulis.
            if !matches!(a.op, AssignOp::Eq) {
                if let Some((nama, _)) = Self::akar(t) { let n = nama.to_string(); self.baca(&n); }
            }
        }
    }

    /// Telusuri bagian sasaran yang benar-benar DIEVALUASI — indeks dan
    /// penerima — tanpa menganggap nama akarnya sebagai pembacaan biasa.
    fn ekspr_dalam_sasaran(&mut self, e: &Expr) {
        match e {
            Expr::Index { obj, idx, .. } => { self.ekspr_dalam_sasaran(obj); self.ekspr(idx); }
            Expr::Field { obj, .. } => self.ekspr_dalam_sasaran(obj),
            Expr::Slice { obj, lo, hi, .. } => {
                self.ekspr_dalam_sasaran(obj);
                if let Some(l) = lo { self.ekspr(l) }
                if let Some(h) = hi { self.ekspr(h) }
            }
            Expr::Ident(..) => {}
            lain => self.ekspr(lain),
        }
    }

    fn parameter(&mut self, ps: &[FnParam]) {
        for p in ps {
            // Parameter selalu bisa diubah: ia sudah berupa salinan, jadi
            // menguncinya tidak melindungi siapa pun. Alasan lengkapnya di
            // kepala berkas ini. `mut` di sini diterima dan tidak berpengaruh.
            self.ikat_pola(&p.pattern, Ikat::BisaUbah);
            if let Some(d) = &p.default { self.ekspr(d) }
        }
    }

    fn percabangan(&mut self, i: &IfStmt) {
        self.masuk();
        if let Some(s) = &i.init { self.stmt(s) }
        self.ekspr(&i.cond);
        self.blok(&i.then);
        for (init, cond, blk) in &i.else_ifs {
            self.masuk();
            if let Some(s) = init { self.stmt(s) }
            self.ekspr(cond);
            self.blok(blk);
            self.keluar();
        }
        if let Some(b) = &i.else_ { self.blok(b) }
        self.keluar();
    }

    fn pilih(&mut self, c: &SelectCase) {
        self.masuk();
        match c {
            SelectCase::Send { chan, val, body, .. } => {
                self.ekspr(chan); self.ekspr(val); self.blok(body);
            }
            SelectCase::Recv { names, is_decl, chan, body, span } => {
                self.ekspr(chan);
                // `case v := <-c` MENDEKLARASIKAN `v`; tanpa `:=` ia menugasi
                // nama yang sudah ada, jadi sasarannya harus diperiksa.
                if *is_decl {
                    for n in names { self.ikat(n, Ikat::BisaUbah) }
                } else {
                    for n in names {
                        if let Some(Ikat::Mengikat(kata)) = self.cari(n) {
                            let (n, kata) = (n.clone(), kata);
                            self.lapor(&n, kata, *span);
                        }
                    }
                }
                self.blok(body);
            }
            SelectCase::Let { stmt, body, .. } => {
                self.ekspr(&stmt.init);
                let k = if stmt.is_mut { Ikat::BisaUbah } else { Ikat::Mengikat("let") };
                self.ikat_pola(&stmt.pattern, k);
                self.blok(body);
            }
        }
        self.keluar();
    }

    fn perulangan(&mut self, f: &ForStmt) {
        self.masuk();
        match f {
            ForStmt::Range { vars, is_decl, iter, body, .. } => {
                self.ekspr(iter);
                // Variabel perulangan BISA DIUBAH: `for mut i` TIDAK parse, jadi menguncinya
                // berarti menutup pintu tanpa memberi kunci.
                let _ = is_decl;
                for v in vars { self.ikat(v, Ikat::BisaUbah) }
                self.blok(body);
            }
            ForStmt::Classic { init, cond, post, body, .. } => {
                if let Some(i) = init { self.stmt(i) }
                if let Some(c) = cond { self.ekspr(c) }
                if let Some(p) = post { self.stmt(p) }
                self.blok(body);
            }
        }
        self.keluar();
    }

    fn cocok(&mut self, scrut: &Expr, arms: &[MatchArm]) {
        self.ekspr(scrut);
        for a in arms {
            self.masuk();
            if a.patterns.len() > 1 {
                self.cek_alternatif(&a.patterns, a.span);
            }
            for p in &a.patterns {
                // Sama seperti parameter: pengikat pola tidak punya kata
                // kunci yang menjanjikan apa pun, dan nilainya sudah salinan.
                self.ikat_pola(p, Ikat::BisaUbah);
            }
            if let Some(g) = &a.guard { self.ekspr(g) }
            match &a.body {
                MatchBody::Expr(e) => self.ekspr(e),
                MatchBody::Block(b) => self.blok(b),
            }
            self.keluar();
        }
    }

    /// Builtin ber-namespace yang diberi argumen lebih banyak daripada yang
    /// dibacanya.
    fn cek_arity(&mut self, e: &Expr) {
        if !self.periksa_arity { return }
        let Expr::Call { func, args, span } = e else { return };
        let Expr::PathExpr { segments, .. } = func.as_ref() else { return };
        if segments.len() != 2 { return }
        if self.tipe_sendiri.contains(&segments[0]) { return }
        let penuh = segments.join("::");
        let Some(maks) = crate::interpreter::arity::maks(&penuh) else { return };
        if args.len() > maks {
            self.arity.push((
                crate::interpreter::arity::pesan_kebanyakan(&penuh, maks, args.len()),
                *span,
            ));
        }
    }

    /// `f(..)` atas fungsi tingkat atas yang tidak dibayangi nama lokal, parameter, atau `fn` bersarang.
    fn cek_aritas_pemakai(&mut self, e: &Expr) {
        let Expr::Call { func, args, span } = e else { return };
        let Expr::Ident(nama, _) = func.as_ref() else { return };
        if self.cari(nama).is_some() { return }
        let Some((min, maks, params)) = self.fungsi_atas.get(nama) else { return };
        let n = args.len();
        if let Some(m) = maks.filter(|m| n > *m) {
            self.aritas_pemakai.push((crate::interpreter::pesan_fungsi_kebanyakan(nama, m, n, params), *span));
        } else if n < *min {
            self.aritas_pemakai.push((crate::interpreter::pesan_parameter_wajib(nama, &params[n], n + 1), *span));
        }
    }

    fn ekspr(&mut self, e: &Expr) {
        self.cek_arity(e);
        self.cek_aritas_pemakai(e);
        match e {
            Expr::Closure { params, body, .. } => {
                self.masuk();
                self.parameter(params);
                self.ekspr(body);
                self.keluar();
            }
            Expr::Block(b) => self.blok(b),
            Expr::Match { scrutinee, arms, .. } => self.cocok(scrutinee, arms),
            Expr::If(i) => self.percabangan(i),
            Expr::Binary { lhs, rhs, .. } => { self.ekspr(lhs); self.ekspr(rhs) }
            Expr::Unary { expr, .. } => self.ekspr(expr),
            Expr::Ternary { cond, then, else_, .. } => { self.ekspr(cond); self.ekspr(then); self.ekspr(else_) }
            Expr::Pipe { lhs, rhs, .. } => { self.ekspr(lhs); self.ekspr(rhs) }
            Expr::Cast { expr, .. } => self.ekspr(expr),
            Expr::Field { obj, .. } => self.ekspr(obj),
            Expr::Method { obj, args, .. } => { self.ekspr(obj); for a in args { self.ekspr(a) } }
            Expr::Index { obj, idx, .. } => { self.ekspr(obj); self.ekspr(idx) }
            Expr::Slice { obj, lo, hi, .. } => {
                self.ekspr(obj);
                if let Some(l) = lo { self.ekspr(l) }
                if let Some(h) = hi { self.ekspr(h) }
            }
            Expr::Call { func, args, .. } => { self.ekspr(func); for a in args { self.ekspr(a) } }
            Expr::Await { expr, span } => {
                self.belum_ada.push((PESAN_AWAIT.to_string(), *span));
                self.ekspr(expr);
            }
            Expr::Propagate { expr, .. } => self.ekspr(expr),
            Expr::Struct { ty, fields, spread, span } => {
                self.cek_ganda_tipe(ty, *span);
                for f in fields { self.ekspr(&f.value) }
                if let Some(s) = spread { self.ekspr(s) }
            }
            Expr::Array { elems, .. } => for x in elems { self.ekspr(x) },
            Expr::ArrayRepeat { elem, count, .. } => { self.ekspr(elem); self.ekspr(count) }
            Expr::Tuple { elems, .. } => for x in elems { self.ekspr(x) },
            Expr::Range { lo, hi, .. } => {
                if let Some(l) = lo { self.ekspr(l) }
                if let Some(h) = hi { self.ekspr(h) }
            }
            Expr::MakeChan { .. } => {}
            Expr::Send { chan, val, .. } => { self.ekspr(chan); self.ekspr(val) }
            Expr::Recv { chan, .. } => self.ekspr(chan),
            Expr::Template { parts, .. } => for p in parts {
                if let TemplatePart::Expr(x) = p { self.ekspr(x) }
            },
            Expr::Ident(n, span) if n == "_" => self.galat.push((PESAN_BACA_PEMBUANG.to_string(), *span)),
            Expr::Ident(n, span) => if !self.baca(n) { self.cek_ganda(&n.clone(), *span) },
            Expr::Literal(..) | Expr::PathExpr { .. } | Expr::Error(..) => {}
        }
    }

}

// ─── Kata kunci yang diurai tapi tidak pernah berpengaruh
// ──────────────────

/// `_` membuang nilai di tiap posisi pengikat (`let _ = f()`, `_ = f()`, `for _, x in a`,
/// `fn f(_)`), jadi ia tidak pernah jadi variabel. Dulu `var _ = 1; _ = 2; wok(_)` menjawab
/// `2` di acuan dan LLVM tapi `1` di `--vm`, dan `let _ = 7; wok(_)` ditolak acuan saja.
pub(crate) const PESAN_BACA_PEMBUANG: &str = "\
`_` cuma untuk membuang nilai — ia tidak pernah jadi variabel, jadi tidak bisa dibaca \
(termasuk lewat `_ += …` atau `_[i] = …`)\n  \
bantuan: beri nama kalau nilainya perlu dipakai lagi, mis. `let sisa = …`";

// ── `async` / `await`: keputusan, bukan "belum"
// ────────────────────────────

const PESAN_ASYNC: &str = "\
`async` tidak ada di JRG, dan ini keputusan — bukan sesuatu yang belum sempat \
dikerjakan. Konkurensi JRG bergaya CSP dan satu thread OS per task, sementara \
`async` di bahasa mana pun menandakan task murah yang di-multiplex; memberi \
katanya tanpa model biayanya cuma memindahkan kejutannya ke waktu jalan.\n  \
bantuan: `Future::new(f)` lalu `future::await(f)` memberi bentuk yang sama \
persis (panggil sekarang, tunggu nanti), dan `spawn`/`go` dengan channel plus \
`select` untuk yang lebih bebas\n  \
catatan: hapus `async`-nya; fungsinya berjalan sama persis";

const PESAN_AWAIT: &str = "\
`await` tidak ada di JRG, dan ini keputusan — lihat `async`. Sebelum ini ia \
diterima tapi tidak melakukan apa pun, sampai-sampai `await 5` pun lolos.\n  \
bantuan: pakai `future::await(f)` untuk sebuah `Future::new(...)`, `task::join(t)` \
untuk sebuah `Task::spawn(...)`, atau `<-ch` untuk menunggu channel\n  \
catatan: hapus `await`-nya; nilainya sudah siap";

// `move` DULU ada di daftar ini, dan tidak lagi.
