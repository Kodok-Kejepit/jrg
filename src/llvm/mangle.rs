// src/llvm/mangle.rs — jrgc LLVM backend — src/llvm/mangle.rs
// Name mangling + LLVM textual-IR escaping helpers.

use super::types::JrgType;

/// Escape a string for use *inside* a quoted LLVM identifier (`%"here"` /
/// `@"here"`) or a `c"here"` string constant: LLVM's textual IR escapes any …
pub fn llvm_escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\22"),
            b'\\' => out.push_str("\\5C"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{:02X}", b)),
        }
    }
    out
}

pub fn llvm_quoted_ident(name: &str) -> String {
    format!("\"{}\"", llvm_escape(name.as_bytes()))
}

/// Mangled name for a monomorphized free function / static method.
pub fn mangle_fn(base: &str, type_args: &[JrgType]) -> String {
    if type_args.is_empty() {
        base.to_string()
    } else {
        let frags: Vec<String> = type_args.iter().map(|t| t.mangle_frag()).collect();
        format!("{}${}", base, frags.join("$"))
    }
}

/// Mangled name for an instance method, `Type.method`.
pub fn mangle_method(self_ty: &str, method: &str) -> String {
    format!("{}.{}", self_ty, method)
}

/// Mangled name for a monomorphized struct/enum instantiation.
pub fn mangle_generic_type(base: &str, type_args: &[JrgType]) -> String {
    if type_args.is_empty() {
        base.to_string()
    } else {
        let frags: Vec<String> = type_args.iter().map(|t| t.mangle_frag()).collect();
        format!("{}${}", base, frags.join("$"))
    }
}

pub fn mangle_option(t: &JrgType) -> String {
    format!("Option${}", t.mangle_frag())
}

pub fn mangle_result(t: &JrgType, e: &JrgType) -> String {
    format!("Result${}${}", t.mangle_frag(), e.mangle_frag())
}
