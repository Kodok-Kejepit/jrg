// src/lsp/completions.rs — src/lsp/completions.rs — Completion items + hover docs

use super::protocol::CompletionItem;

/// Item pelengkapan: kata kunci (tulis tangan — mereka sintaks, bukan
/// builtin) plus **setiap nama dari registry yang sedang berjalan**.
pub fn all_completions() -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for kw in &[
        "fn","let","var","const","if","else","while","loop","for","in",
        "return","break","continue","match","struct","impl","enum","trait",
        "pub","owned","shared","gc","ref","mut","async","await","spawn",
        "go","chan","select","defer","unsafe","use","import","package",
        "true","false","nil","self","super","type","extern","mod",
    ] {
        items.push(CompletionItem::keyword(kw));
    }

    for nama in crate::vm::daftar_builtin() {
        match crate::bantu::DOKUMEN.iter().find(|i| i.nama == nama) {
            Some(i) => items.push(CompletionItem::function(&nama, i.tanda_tangan, i.ringkas)),
            None => {
                let arity = match crate::interpreter::arity::maks(&nama) {
                    Some(n) => format!("paling banyak {} argumen", n),
                    None => "jumlah argumennya tidak dibatasi tabel".to_string(),
                };
                items.push(CompletionItem::function(
                    &nama, &nama, &format!("terdaftar, {} (belum ada penjelasan tertulis)", arity)));
            }
        }
    }

    // Bentuk sintaks yang bukan builtin (`for`, `match`, `??`, `"""`, …).
    for i in crate::bantu::SINTAKS {
        items.push(CompletionItem::function(i.nama, i.tanda_tangan, i.ringkas));
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    items
}

/// Dokumentasi hover — **satu definisi**, dipinjam dari `crate::bantu`.
pub fn hover_doc(word: &str) -> Option<String> {
    let teks = crate::bantu::jelaskan(word)?;
    let (tanda, sisa) = teks.split_once('\n').unwrap_or((teks.as_str(), ""));
    Some(format!("**{}**\n\n{}", tanda.trim(), sisa.trim()))
}
