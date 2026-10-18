// src/lsp/simbol.rs — src/lsp/simbol.rs — SATU indeks simbol, diturunkan dari AST.

use crate::lexer::Span;
use crate::parser::{EnumVariantKind, ImplItem, Item, ParsedFile, TraitItem};

/// Nilai `SymbolKind` LSP yang dipakai di sini.
pub mod jenis {
    pub const FUNGSI: u8 = 12;
    pub const METODE: u8 = 6;
    pub const STRUCT: u8 = 23;
    pub const ENUM: u8 = 10;
    pub const TRAIT: u8 = 11; // Interface
    pub const KONSTAN: u8 = 14;
    pub const VARIABEL: u8 = 13;
    pub const VARIAN: u8 = 22; // EnumMember
}

#[derive(Debug, Clone)]
pub struct Simbol {
    pub nama: String,
    pub jenis: u8,
    pub span: Span,
    /// Nama tipe pemilik, untuk metode di dalam `impl` atau `trait`.
    pub induk: Option<String>,
}

impl Simbol {
    /// Nama yang ditampilkan ke pengguna.
    pub fn nama_tampil(&self) -> String {
        match &self.induk {
            Some(t) => format!("{}::{}", t, self.nama),
            None => self.nama.clone(),
        }
    }
}

/// Kumpulkan tiap simbol yang dideklarasikan di berkas ini.
pub fn dari_ast(file: &ParsedFile) -> Vec<Simbol> {
    let mut out = Vec::new();

    for item in &file.items {
        match item {
            Item::Fn(f) => out.push(Simbol {
                nama: f.name.clone(),
                jenis: jenis::FUNGSI,
                span: f.span,
                induk: None,
            }),
            Item::Struct(s) => out.push(Simbol {
                nama: s.name.clone(),
                jenis: jenis::STRUCT,
                span: s.span,
                induk: None,
            }),
            Item::Enum(e) => {
                out.push(Simbol {
                    nama: e.name.clone(),
                    jenis: jenis::ENUM,
                    span: e.span,
                    induk: None,
                });
                for v in &e.variants {
                    // Varian bermuatan tetap didaftarkan sama seperti varian
                    // unit: keduanya nama yang bisa ditulis orang di `match`,
                    // dan yang mencarinya tidak peduli bentuk muatannya.
                    let _ = matches!(v.kind, EnumVariantKind::Unit);
                    out.push(Simbol {
                        nama: v.name.clone(),
                        jenis: jenis::VARIAN,
                        span: v.span,
                        induk: Some(e.name.clone()),
                    });
                }
            }
            Item::Trait(t) => {
                out.push(Simbol {
                    nama: t.name.clone(),
                    jenis: jenis::TRAIT,
                    span: t.span,
                    induk: None,
                });
                for it in &t.items {
                    let (TraitItem::Method(m) | TraitItem::DefaultMethod(m)) = it;
                    out.push(Simbol {
                        nama: m.name.clone(),
                        jenis: jenis::METODE,
                        span: m.span,
                        induk: Some(t.name.clone()),
                    });
                }
            }
            Item::Impl(i) => {
                let pemilik = crate::interpreter::eval_expr::nama_tipe_sasaran(&i.self_type);
                for it in &i.items {
                    match it {
                        ImplItem::Method(m) => out.push(Simbol {
                            nama: m.name.clone(),
                            jenis: jenis::METODE,
                            span: m.span,
                            induk: Some(pemilik.clone()),
                        }),
                        ImplItem::Const(c) => out.push(Simbol {
                            nama: c.name.clone(),
                            jenis: jenis::KONSTAN,
                            span: c.span,
                            induk: Some(pemilik.clone()),
                        }),
                        _ => {}
                    }
                }
            }
            Item::Const(c) => out.push(Simbol {
                nama: c.name.clone(),
                jenis: jenis::KONSTAN,
                span: c.span,
                induk: None,
            }),
            Item::Var(v) => out.push(Simbol {
                nama: v.name.clone(),
                jenis: jenis::VARIABEL,
                span: v.span,
                induk: None,
            }),
            // `TypeAlias` dan `Mod` sengaja dilewati: keduanya DITOLAK
            // parser sejak fase EKSPANSI-1, jadi mendaftarkannya berarti
            // menawarkan navigasi ke sesuatu yang tidak akan pernah jalan.
            _ => {}
        }
    }

    out
}

/// Cari simbol dengan nama PERSIS `nama`.
pub fn cari<'a>(simbol: &'a [Simbol], nama: &str) -> Option<&'a Simbol> {
    // Deklarasi tingkat atas didahulukan daripada metode. Kalau sebuah nama
    // dipakai untuk fungsi bebas DAN metode sebuah struct, yang dimaksud
    // orang saat mengetik nama telanjang hampir selalu yang pertama.
    simbol
        .iter()
        .find(|s| s.nama == nama && s.induk.is_none())
        .or_else(|| simbol.iter().find(|s| s.nama == nama))
}
