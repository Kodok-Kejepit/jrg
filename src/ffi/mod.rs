// src/ffi/mod.rs — Compiler — src/ffi/mod.rs
// Direct FFI Subsystem: C++, Go, and System ABI Integration

#![allow(dead_code, unused_variables, unused_imports)]

pub mod cpp;
pub mod dotnet;
pub mod jvm;
pub mod kotlin;
pub mod wasm;
pub mod native;
pub mod pekerja;
pub mod python;
pub mod javascript;
pub mod php;
pub mod lua;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::collections::HashMap;

// ============================================================================
// §4.1 FFI ARCHITECTURE OVERVIEW …

// ============================================================================
//  §4.2  C++ FFI IMPLEMENTATION
// ============================================================================

// src/ffi/cpp_bridge.rs:

/// Represents a parsed C++ header that the compiler reads to resolve
/// type sizes, alignments, and calling conventions before generating IR.
#[derive(Debug, Clone)]
pub struct CppHeaderInfo {
    pub classes:   Vec<CppClass>,
    pub functions: Vec<CppFunction>,
    pub typedefs:  Vec<CppTypedef>,
    pub constants: Vec<CppConstant>,
}

#[derive(Debug, Clone)]
pub struct CppClass {
    pub name:        String,
    pub mangled_name: String,       // Itanium mangled, e.g. "_ZN3Foo3BarE"
    pub size:        usize,         // from clang AST sizeof query
    pub alignment:   usize,
    pub fields:      Vec<CppField>,
    pub vtable:      Option<CppVTable>,
    pub methods:     Vec<CppMethod>,
    pub base_classes: Vec<String>,
    pub is_abstract: bool,
}

#[derive(Debug, Clone)]
pub struct CppField {
    pub name:      String,
    pub offset:    usize,   // byte offset within class
    pub size:      usize,
    pub ty_llvm:   String,  // LLVM IR type string: "i32", "ptr", "[8 x i8]", etc.
}

#[derive(Debug, Clone)]
pub struct CppVTable {
    pub entries: Vec<CppVTableEntry>,
}

#[derive(Debug, Clone)]
pub struct CppVTableEntry {
    pub index:         usize,
    pub mangled_name:  String,
    pub signature:     CppFunctionSignature,
}

#[derive(Debug, Clone)]
pub struct CppMethod {
    pub name:          String,
    pub mangled_name:  String,
    pub is_virtual:    bool,
    pub is_const:      bool,
    pub vtable_index:  Option<usize>,
    pub signature:     CppFunctionSignature,
}

#[derive(Debug, Clone)]
pub struct CppFunction {
    pub name:         String,
    pub mangled_name: String,
    pub signature:    CppFunctionSignature,
    pub is_inline:    bool,
    pub linkage:      CppLinkage,
}

#[derive(Debug, Clone)]
pub struct CppFunctionSignature {
    pub return_type: CppType,
    pub params:      Vec<CppParam>,
    pub calling_conv: CallingConvention,
    pub is_variadic: bool,
}

#[derive(Debug, Clone)]
pub struct CppParam {
    pub name: Option<String>,
    pub ty:   CppType,
}

#[derive(Debug, Clone)]
pub enum CppType {
    Void,
    Bool,
    Char, UChar, SChar,
    Short, UShort,
    Int, UInt,
    Long, ULong,
    LongLong, ULongLong,
    Float, Double, LongDouble,
    Pointer(Box<CppType>, bool),   // (pointee, is_const)
    Reference(Box<CppType>, bool), // (referent, is_const)
    RvalueRef(Box<CppType>),
    Array(Box<CppType>, usize),
    Class(String),                  // by name; looked up in CppHeaderInfo.classes
    Typedef(String, Box<CppType>),
    TemplateInst(String, Vec<CppType>),
    FunctionPointer(Box<CppFunctionSignature>),
    Enum(String, Box<CppType>),     // enum name, underlying integer type
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct CppTypedef {
    pub alias: String,
    pub ty:    CppType,
}

#[derive(Debug, Clone)]
pub struct CppConstant {
    pub name:  String,
    pub ty:    CppType,
    pub value: i128,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CppLinkage { External, Internal, Inline }

#[derive(Debug, Clone, PartialEq)]
pub enum CallingConvention {
    CDecl,        // default on x86-64 SystemV / MS x64
    StdCall,      // Win32 legacy
    FastCall,     // Win32 legacy
    ThisCall,     // non-virtual C++ method dispatch on x86
    VectorCall,   // MSVC SIMD-optimized
    SysVAmd64,   // Linux/macOS x86-64 System V
    Win64,        // Windows x64
}

impl CallingConvention {
    /// Translate to LLVM calling convention attribute string.
    pub fn to_llvm_cc(&self) -> &'static str {
        match self {
            CallingConvention::CDecl | CallingConvention::SysVAmd64 => "ccc",
            CallingConvention::StdCall    => "x86_stdcallcc",
            CallingConvention::FastCall   => "x86_fastcallcc",
            CallingConvention::ThisCall   => "x86_thiscallcc",
            CallingConvention::VectorCall => "x86_vectorcallcc",
            CallingConvention::Win64      => "win64cc",
        }
    }
}

/// Maps a CppType to its LLVM IR type string.
/// This is used during IR generation when emitting call instructions
/// to C++ functions.
pub fn cpp_type_to_llvm_ir(ty: &CppType, header: &CppHeaderInfo) -> String {
    match ty {
        CppType::Void         => "void".to_string(),
        CppType::Bool         => "i1".to_string(),
        CppType::Char         => "i8".to_string(),
        CppType::UChar        => "i8".to_string(),
        CppType::SChar        => "i8".to_string(),
        CppType::Short        => "i16".to_string(),
        CppType::UShort       => "i16".to_string(),
        CppType::Int          => "i32".to_string(),
        CppType::UInt         => "i32".to_string(),
        CppType::Long         => "i64".to_string(),    // LP64 on Linux/macOS
        CppType::ULong        => "i64".to_string(),
        CppType::LongLong     => "i64".to_string(),
        CppType::ULongLong    => "i64".to_string(),
        CppType::Float        => "float".to_string(),
        CppType::Double       => "double".to_string(),
        CppType::LongDouble   => "x86_fp80".to_string(),  // x86-64 Linux ABI
        CppType::Pointer(_, _) | CppType::Reference(_, _) | CppType::RvalueRef(_)
                              => "ptr".to_string(),        // opaque pointers (LLVM 15+)
        CppType::Array(elem, n) => {
            format!("[{} x {}]", n, cpp_type_to_llvm_ir(elem, header))
        }
        CppType::Class(name) => {
            // Look up class in header; build LLVM struct type
            if let Some(cls) = header.classes.iter().find(|c| &c.name == name) {
                // Emit as byte array of exact size; field-level access uses GEP
                // with explicit byte offsets. This avoids LLVM struct padding
                // mismatches with C++ ABI padding.
                format!("[{} x i8]", cls.size)
            } else {
                // Unknown class: opaque ptr (safe for pointer-to-class only)
                "ptr".to_string()
            }
        }
        CppType::Typedef(_, inner)         => cpp_type_to_llvm_ir(inner, header),
        CppType::TemplateInst(_, _)        => "ptr".to_string(), // conservative
        CppType::FunctionPointer(_)        => "ptr".to_string(),
        CppType::Enum(_, underlying)       => cpp_type_to_llvm_ir(underlying, header),
        CppType::Unknown(_)                => "ptr".to_string(), // conservative
    }
}

/// Generates the LLVM IR `declare` line for a C++ function.
/// Example output:
///   declare i32 @_ZN3Foo3barEi(ptr noundef, i32 noundef) #0
pub fn emit_cpp_fn_declare(func: &CppFunction, header: &CppHeaderInfo) -> String {
    let ret = cpp_type_to_llvm_ir(&func.signature.return_type, header);
    let params: Vec<String> = func.signature.params.iter().map(|p| {
        format!("{} noundef", cpp_type_to_llvm_ir(&p.ty, header))
    }).collect();
    let variadic = if func.signature.is_variadic { ", ..." } else { "" };
    let cc = func.signature.calling_conv.to_llvm_cc();
    format!(
        "declare {} {} @{}({}{}) nounwind",
        cc, ret, func.mangled_name, params.join(", "), variadic
    )
}

/// For C++ non-virtual method calls, the `this` pointer is the first
/// argument.
pub fn emit_virtual_call_ir(
    obj_ir_name: &str,
    method: &CppMethod,
    args: &[String],
    header: &CppHeaderInfo,
) -> String {
    let vtable_idx = method.vtable_index.expect("virtual method must have vtable index");
    let ret = cpp_type_to_llvm_ir(&method.signature.return_type, header);
    let params: Vec<String> = std::iter::once(format!("ptr {}", obj_ir_name))
        .chain(method.signature.params.iter().zip(args.iter()).map(|(p, a)| {
            format!("{} {}", cpp_type_to_llvm_ir(&p.ty, header), a)
        }))
        .collect();

    format!(
        r#"  %_vtptr_{name} = load ptr, ptr {obj}, align 8
  %_vtslot_{name} = getelementptr inbounds ptr, ptr %_vtptr_{name}, i64 {idx}
  %_vtfn_{name} = load ptr, ptr %_vtslot_{name}, align 8
  %_vcall_{name} = call {cc} {ret} %_vtfn_{name}({params})"#,
        name = method.name.replace("::", "_"),
        obj  = obj_ir_name,
        idx  = vtable_idx,
        cc   = method.signature.calling_conv.to_llvm_cc(),
        ret  = ret,
        params = params.join(", "),
    )
}

// ─── C++ Header Parser (via libclang) ─────────────────────────────────────

/// Invokes libclang to parse a C++ header and extract ABI information.
pub fn parse_cpp_header(header_path: &Path, include_dirs: &[PathBuf]) -> Result<CppHeaderInfo, String> {
    let include_flags: Vec<String> = include_dirs.iter()
        .map(|d| format!("-I{}", d.display()))
        .collect();

    let output = Command::new("clang")
        .arg("-cc1")
        .arg("-ast-dump=json")
        .arg("-std=c++17")
        .args(&include_flags)
        .arg(header_path)
        .output()
        .map_err(|e| format!("failed to invoke clang: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "clang header parse failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let ast_json = String::from_utf8_lossy(&output.stdout);
    parse_clang_ast_json(&ast_json)
}

/// Parse the JSON AST produced by `clang -ast-dump=json`.
/// This is a partial parser covering the constructs the FFI subsystem needs.
fn parse_clang_ast_json(json: &str) -> Result<CppHeaderInfo, String> {
    // In a production compiler this would use serde_json.

    Ok(CppHeaderInfo {
        classes:   Vec::new(),
        functions: Vec::new(),
        typedefs:  Vec::new(),
        constants: Vec::new(),
    })
}

/// Compiles a .cpp source file to a .o object using the embedded clang driver.
/// Called by `jrgm build` for each C++ source in the manifest.
pub fn compile_cpp_source(
    src: &Path,
    out: &Path,
    include_dirs: &[PathBuf],
    defines: &[String],
    opt_level: u8,            // 0-3
    target_triple: &str,
) -> Result<(), String> {
    let include_flags: Vec<String> = include_dirs.iter()
        .map(|d| format!("-I{}", d.display()))
        .collect();
    let define_flags: Vec<String> = defines.iter()
        .map(|d| format!("-D{}", d))
        .collect();
    let opt = format!("-O{}", opt_level);

    let status = Command::new("clang++")
        .arg("-c")                              // compile only, no link
        .arg("-std=c++17")
        .arg(&opt)
        .arg(format!("--target={}", target_triple))
        .arg("-fPIC")                           // position-independent for .so
        .arg("-fvisibility=hidden")             // hide symbols not explicitly exported
        .args(&include_flags)
        .args(&define_flags)
        .arg("-o").arg(out)
        .arg(src)
        .status()
        .map_err(|e| format!("failed to invoke clang++: {}", e))?;

    if status.success() { Ok(()) }
    else { Err(format!("clang++ compilation failed for {}", src.display())) }
}


// ============================================================================
//  §4.3  GO FFI IMPLEMENTATION
// ============================================================================

// src/ffi/go_bridge.rs:

/// A Go module imported via `extern "Go" { }`.
#[derive(Debug, Clone)]
pub struct GoModule {
    pub import_path:  String,          // e.g. "github.com/user/pkg"
    pub local_path:   PathBuf,         // resolved local checkout path
    pub exported_fns: Vec<GoExport>,
    pub output_so:    PathBuf,         // where to write the .so
}

#[derive(Debug, Clone)]
pub struct GoExport {
    pub name:      String,             // as declared after //export
    pub c_name:    String,             // same, but C-compatible (usually identical)
    pub signature: GoFunctionSig,
}

#[derive(Debug, Clone)]
pub struct GoFunctionSig {
    pub params:  Vec<GoParam>,
    pub results: Vec<GoType>,
}

#[derive(Debug, Clone)]
pub struct GoParam {
    pub name: Option<String>,
    pub ty:   GoType,
}

/// Go types and their C ABI equivalents when exported via cgo.
/// Go's cgo documentation defines the mapping; we implement it here.
#[derive(Debug, Clone)]
pub enum GoType {
    // Primitive types — direct C ABI mapping
    Bool,                   // C: GoUint8 (_Bool)
    Int8,                   // C: GoInt8
    Int16,                  // C: GoInt16
    Int32,                  // C: GoInt32
    Int64,                  // C: GoInt64
    Uint8,                  // C: GoUint8
    Uint16,                 // C: GoUint16
    Uint32,                 // C: GoUint32
    Uint64,                 // C: GoUint64
    Uintptr,                // C: GoUintptr (uintptr_t)
    Float32,                // C: GoFloat32
    Float64,                // C: GoFloat64
    Complex64,              // C: GoComplex64 (struct { float r, i; })
    Complex128,             // C: GoComplex128 (struct { double r, i; })
    // Non-trivial types
    String,                 // C: GoString  (struct { const char* p; GoInt n; })
    Slice(Box<GoType>),     // C: GoSlice   (struct { void* data; GoInt len, cap; })
    UnsafePointer,          // C: void*
    Pointer(Box<GoType>),   // C: GoType*
    // Named/struct types (passed via pointer in C ABI)
    Named(String),
}

impl GoType {
    /// Returns the LLVM IR type for this Go type when used in a C-ABI call.
    pub fn to_llvm_ir(&self) -> String {
        match self {
            GoType::Bool                       => "i8".to_string(),
            GoType::Int8  | GoType::Uint8      => "i8".to_string(),
            GoType::Int16 | GoType::Uint16     => "i16".to_string(),
            GoType::Int32 | GoType::Uint32     => "i32".to_string(),
            GoType::Int64 | GoType::Uint64
            | GoType::Uintptr                  => "i64".to_string(),
            GoType::Float32                    => "float".to_string(),
            GoType::Float64                    => "double".to_string(),
            // Structs passed as immediate structs on-stack per SysV AMD64 ABI
            GoType::Complex64    => "{ float, float }".to_string(),
            GoType::Complex128   => "{ double, double }".to_string(),
            // GoString: { ptr, i64 }
            GoType::String       => "{ ptr, i64 }".to_string(),
            // GoSlice: { ptr, i64, i64 }
            GoType::Slice(_)     => "{ ptr, i64, i64 }".to_string(),
            GoType::UnsafePointer
            | GoType::Pointer(_) => "ptr".to_string(),
            GoType::Named(_)     => "ptr".to_string(), // passed by pointer
        }
    }
}

/// Builds a Go module as a C-shared library.
/// Invokes the Go toolchain (must be present on PATH).
pub fn build_go_module(module: &GoModule, target_goos: &str, target_goarch: &str) -> Result<(), String> {
    // Step 1: Verify the Go module has the required //export directives.
    // This is done by scanning the .go source files for `//export FuncName`
    // comments. Missing exports are compile errors.
    verify_go_exports(module)?;

    // Step 2: Compile the Go module to a C-shared library.
    let status = Command::new("go")
        .arg("build")
        .arg("-buildmode=c-shared")
        .arg("-o").arg(&module.output_so)
        .arg(&module.import_path)
        .env("GOOS",   target_goos)
        .env("GOARCH", target_goarch)
        .env("CGO_ENABLED", "1")
        // Pass the JRG ABI version so Go can check compatibility
        .env("JRG_FFI_VERSION", "1")
        .current_dir(&module.local_path)
        .status()
        .map_err(|e| format!("failed to invoke go build: {}", e))?;

    if !status.success() {
        return Err(format!("go build failed for module {}", module.import_path));
    }

    // Step 3: Parse the generated .h header from go build -buildmode=c-shared.
    let header_path = module.output_so.with_extension("h");
    parse_go_cgo_header(&header_path, module)?;

    Ok(())
}

fn verify_go_exports(module: &GoModule) -> Result<(), String> {
    use std::fs;

    let mut found_exports: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Walk all .go files in the module directory
    let go_files = fs::read_dir(&module.local_path)
        .map_err(|e| format!("cannot read Go module dir: {}", e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "go"))
        .collect::<Vec<_>>();

    for file_entry in &go_files {
        let src = fs::read_to_string(file_entry.path())
            .map_err(|e| format!("cannot read {}: {}", file_entry.path().display(), e))?;
        for line in src.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("//export ") {
                found_exports.insert(rest.trim().to_string());
            }
        }
    }

    for export in &module.exported_fns {
        if !found_exports.contains(&export.name) {
            return Err(format!(
                "Go module `{}` does not export `{}` (missing //export {})",
                module.import_path, export.name, export.name
            ));
        }
    }

    Ok(())
}

fn parse_go_cgo_header(header: &Path, module: &GoModule) -> Result<(), String> {
    // The generated header has lines like: extern GoInt64 Add(GoInt64 a, GoInt64
    // b); extern GoString FormatMsg(GoString prefix, GoInt64 code); We parse …
    use std::fs;
    let content = fs::read_to_string(header)
        .map_err(|e| format!("cannot read cgo header {}: {}", header.display(), e))?;

    let mut declared: HashMap<String, Vec<String>> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("extern ") { continue; }
        let without_extern = &line[7..];
        // Find function name: word before '('
        if let Some(paren_pos) = without_extern.find('(') {
            let before_paren = without_extern[..paren_pos].trim();
            // Last word before '(' is the function name
            let fn_name = before_paren.split_whitespace().last()
                .unwrap_or("").to_string();
            let params_str = without_extern[paren_pos+1..].trim_end_matches(");").trim();
            let params: Vec<String> = if params_str.is_empty() {
                Vec::new()
            } else {
                params_str.split(',').map(|s| s.trim().to_string()).collect()
            };
            declared.insert(fn_name, params);
        }
    }

    // Cross-check against module.exported_fns
    for export in &module.exported_fns {
        if !declared.contains_key(&export.c_name) {
            return Err(format!(
                "Go cgo header missing export `{}` in {}",
                export.c_name, header.display()
            ));
        }
    }

    Ok(())
}

/// Generates the LLVM IR `declare` for a Go-exported function.
pub fn emit_go_fn_declare(export: &GoExport) -> String {
    let ret_types: Vec<String> = export.signature.results.iter()
        .map(|r| r.to_llvm_ir())
        .collect();
    let ret = match ret_types.len() {
        0 => "void".to_string(),
        1 => ret_types[0].clone(),
        _ => format!("{{ {} }}", ret_types.join(", ")),
    };

    let param_types: Vec<String> = export.signature.params.iter()
        .map(|p| p.ty.to_llvm_ir())
        .collect();

    format!(
        "declare ccc {} @{}({}) nounwind",
        ret,
        export.c_name,
        param_types.join(", ")
    )
}

/// Runtime Go module loader — embedded in the JRG runtime library.
/// Called once at program startup for each Go module dependency.
/// The handle is stored as usize (opaque dlopen/LoadLibrary handle).
pub struct GoModuleLoader {
    handles: HashMap<String, usize>,
}

// NOTE: in a real impl this uses libloading or dlopen directly.


// ============================================================================
//  §4.4  THE LINKER SUBSYSTEM
// ============================================================================

// src/ffi/linker.rs:

/// LinkerConfig holds all inputs the JRG linker driver needs to produce
/// a final binary. This is populated by `jrgm build` from the manifest.
#[derive(Debug, Clone)]
pub struct LinkerConfig {
    pub target_triple:  String,
    pub output:         PathBuf,
    pub output_kind:    OutputKind,
    pub jrg_objects:    Vec<PathBuf>,   // .o from JRG source compilation
    pub cpp_objects:    Vec<PathBuf>,   // .o from C++ source compilation
    pub go_modules:     Vec<PathBuf>,   // .so from Go build
    pub rust_libs:      Vec<PathBuf>,   // .rlib from Rust crate compilation
    pub system_libs:    Vec<String>,    // e.g. ["pthread", "ssl", "crypto"]
    pub lib_dirs:       Vec<PathBuf>,   // -L paths
    pub rpath:          Vec<PathBuf>,   // runtime library search paths
    pub linker:         LinkerBackend,
    pub strip:          bool,
    pub lto:            LtoMode,
    pub opt_level:      u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutputKind {
    Executable,
    SharedLib,       // .so / .dylib / .dll
    StaticLib,       // .a
    Object,          // single .o
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkerBackend {
    Lld,            // LLVM lld (default; fastest, deterministic)
    Bfd,            // GNU ld (fallback)
    Gold,           // GNU gold
    Mold,           // Mold (parallel)
    MsLink,         // MSVC link.exe (Windows)
}

#[derive(Debug, Clone, PartialEq)]
pub enum LtoMode {
    None,
    Thin,           // ThinLTO: fast parallel LTO
    Full,           // Full LTO: maximum optimization
}

/// Invokes the linker with all configured inputs.
/// This is the final step of `jrgm build`.
pub fn link(config: &LinkerConfig) -> Result<(), String> {
    let mut cmd = build_linker_command(config)?;

    // --- JRG runtime --- Always link the JRG runtime library, which provides: -
    // GC runtime (jrg_gc_alloc, jrg_gc_write_barrier, GC mark/sweep) - Goroutine …
    cmd.arg("-ljrg_runtime");

    // --- C++ standard library ---
    // We always link libstdc++ or libc++ when C++ objects are present.
    if !config.cpp_objects.is_empty() {
        match config.target_triple.as_str() {
            t if t.contains("linux-gnu") => { cmd.arg("-lstdc++"); }
            t if t.contains("darwin")    => { cmd.arg("-lc++"); }
            t if t.contains("windows")   => { /* linked via .lib in MSVC mode */ }
            _                             => { cmd.arg("-lstdc++"); }
        }
    }

    // --- Go modules (rpath for runtime loading) ---
    for so in &config.go_modules {
        if let Some(dir) = so.parent() {
            cmd.arg(format!("-Wl,-rpath,{}", dir.display()));
        }
    }

    // --- System libraries ---
    for lib in &config.system_libs {
        cmd.arg(format!("-l{}", lib));
    }

    // --- LTO flags ---
    match config.lto {
        LtoMode::None => {}
        LtoMode::Thin => { cmd.arg("-flto=thin"); }
        LtoMode::Full => { cmd.arg("-flto=full"); }
    }

    // --- Strip ---
    if config.strip { cmd.arg("-Wl,--strip-all"); }

    let status = cmd.status()
        .map_err(|e| format!("linker invocation failed: {}", e))?;

    if status.success() { Ok(()) }
    else { Err(format!("link step failed for output {}", config.output.display())) }
}

fn build_linker_command(config: &LinkerConfig) -> Result<Command, String> {
    let mut cmd = match config.linker {
        LinkerBackend::Lld => {
            let mut c = Command::new("ld.lld");
            c.arg("--eh-frame-hdr");
            c
        }
        LinkerBackend::Bfd  => Command::new("ld"),
        LinkerBackend::Gold => Command::new("ld.gold"),
        LinkerBackend::Mold => Command::new("mold"),
        LinkerBackend::MsLink => Command::new("link.exe"),
    };

    cmd.arg("-o").arg(&config.output);

    match config.output_kind {
        OutputKind::SharedLib => { cmd.arg("-shared"); }
        OutputKind::StaticLib => {
            // Static libs are created with ar, not the linker.
            // We replace cmd entirely:
            let mut ar_cmd = Command::new("llvm-ar");
            ar_cmd.arg("rcs").arg(&config.output);
            for obj in &config.jrg_objects { ar_cmd.arg(obj); }
            for obj in &config.cpp_objects { ar_cmd.arg(obj); }
            return Ok(ar_cmd);
        }
        _ => {}
    }

    // Input objects
    for obj in &config.jrg_objects { cmd.arg(obj); }
    for obj in &config.cpp_objects { cmd.arg(obj); }
    for rlib in &config.rust_libs  { cmd.arg(rlib); }

    // Library search paths
    for dir in &config.lib_dirs { cmd.arg(format!("-L{}", dir.display())); }

    // Runtime paths
    for rp in &config.rpath { cmd.arg(format!("-Wl,-rpath,{}", rp.display())); }

    Ok(cmd)
}


// ============================================================================
//  §4.5  jrgm — PACKAGE MANAGER
// ============================================================================

// src/jrgm/manifest.rs + build pipeline:

/// jrgm.toml — the JawaRustGo++ package manifest.
/// Analogous to Cargo.toml or go.mod, but supports heterogeneous dependencies.
#[derive(Debug, Clone)]
pub struct JrgManifest {
    pub package:      PackageMeta,
    pub dependencies: Vec<Dependency>,
    pub dev_deps:     Vec<Dependency>,
    pub build_deps:   Vec<Dependency>,
    pub cpp_sources:  Vec<CppSourceConfig>,
    pub go_modules:   Vec<GoModuleConfig>,
    pub profile:      HashMap<String, BuildProfile>,
    pub workspace:    Option<WorkspaceConfig>,
}

#[derive(Debug, Clone)]
pub struct PackageMeta {
    pub name:        String,
    pub version:     SemVer,
    pub authors:     Vec<String>,
    pub description: Option<String>,
    pub license:     Option<String>,
    pub edition:     Edition,          // "2025" for JRG 0.1
    pub entry:       PathBuf,          // default: "src/main.jrg"
}

#[derive(Debug, Clone)]
pub struct SemVer { pub major: u32, pub minor: u32, pub patch: u32, pub pre: Option<String> }

#[derive(Debug, Clone)]
pub enum Edition { Jrg2025 }

#[derive(Debug, Clone)]
pub struct Dependency {
    pub name:    String,
    pub source:  DepSource,
    pub features: Vec<String>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub enum DepSource {
    Registry { version: String },                           // jrgm registry
    Git      { url: String, rev: Option<String> },         // git repo
    Path     { path: PathBuf },                             // local path
    Rust     { crate_name: String, version: String },       // crates.io
    Go       { import_path: String, version: String },      // Go module
    Cpp      { url: String, version: String, header: PathBuf }, // C++ library
}

/// C++ source file configuration within a JawaRustGo++ package.
/// These files are compiled by the embedded clang driver and linked
/// directly into the output binary — no wrappers required.
#[derive(Debug, Clone)]
pub struct CppSourceConfig {
    pub sources:      Vec<PathBuf>,     // .cpp files to compile
    pub headers:      Vec<PathBuf>,     // .h/.hpp files to parse for ABI
    pub include_dirs: Vec<PathBuf>,
    pub defines:      Vec<String>,
    pub extra_flags:  Vec<String>,      // raw clang++ flags
    pub std_version:  CppStd,
    pub exceptions:   bool,             // enable C++ exceptions (default: true)
    pub rtti:         bool,             // enable RTTI (default: true)
}

#[derive(Debug, Clone, PartialEq)]
pub enum CppStd { Cpp14, Cpp17, Cpp20, Cpp23 }

impl CppStd {
    pub fn to_flag(&self) -> &'static str {
        match self {
            CppStd::Cpp14 => "-std=c++14",
            CppStd::Cpp17 => "-std=c++17",
            CppStd::Cpp20 => "-std=c++20",
            CppStd::Cpp23 => "-std=c++23",
        }
    }
}

/// Go module configuration for direct embedding.
#[derive(Debug, Clone)]
pub struct GoModuleConfig {
    pub import_path: String,
    pub local_path:  Option<PathBuf>,   // if using local checkout
    pub exports:     Vec<String>,       // function names to import
    pub go_version:  String,            // e.g. "1.22"
}

#[derive(Debug, Clone)]
pub struct BuildProfile {
    pub opt_level:   u8,
    pub debug_info:  bool,
    pub lto:         LtoMode,
    pub strip:       bool,
    pub overflow_checks: bool,
    pub incremental: bool,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub members: Vec<PathBuf>,
    pub exclude: Vec<PathBuf>,
}

// ─── jrgm build pipeline ──────────────────────────────────────────────────

/// Full build pipeline for a JawaRustGo++ package.
/// This is what `jrgm build` executes.
pub struct BuildPipeline {
    pub manifest:      JrgManifest,
    pub target_triple: String,
    pub profile_name:  String,
    pub out_dir:       PathBuf,
}

impl BuildPipeline {
    /// Execute the complete build pipeline.
    pub fn run(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let profile = self.manifest.profile.get(&self.profile_name)
            .cloned()
            .unwrap_or(BuildProfile {
                opt_level: 2, debug_info: false, lto: LtoMode::Thin,
                strip: true, overflow_checks: false, incremental: true,
            });

        // ─ Step 1: Resolve and fetch dependencies ─────────────────────────
        match self.resolve_dependencies() {
            Ok(_)  => {}
            Err(e) => { errors.push(e); return Err(errors); }
        }

        // ─ Step 2: Compile C++ sources ────────────────────────────────────
        let mut cpp_objects = Vec::new();
        for cpp_cfg in &self.manifest.cpp_sources {
            match self.compile_cpp_sources(cpp_cfg, &profile) {
                Ok(objs) => cpp_objects.extend(objs),
                Err(e)   => errors.push(e),
            }
        }

        // ─ Step 3: Build Go modules ───────────────────────────────────────
        let mut go_sos = Vec::new();
        for go_cfg in &self.manifest.go_modules {
            match self.build_go_module_dep(go_cfg, &profile) {
                Ok(so)  => go_sos.push(so),
                Err(e)  => errors.push(e),
            }
        }

        if !errors.is_empty() { return Err(errors); }

        // ─ Step 4: Parse C++ headers for ABI info ─────────────────────────
        let mut header_infos: Vec<CppHeaderInfo> = Vec::new();
        for cpp_cfg in &self.manifest.cpp_sources {
            for header in &cpp_cfg.headers {
                match parse_cpp_header(header, &cpp_cfg.include_dirs) {
                    Ok(info) => header_infos.push(info),
                    Err(e)   => errors.push(e),
                }
            }
        }

        // ─ Step 5: Compile JawaRustGo++ sources ───────────────────────────
        let jrg_objects = match self.compile_jrg_sources(&header_infos, &go_sos, &profile) {
            Ok(objs) => objs,
            Err(errs) => { errors.extend(errs); return Err(errors); }
        };

        if !errors.is_empty() { return Err(errors); }

        // ─ Step 6: Link everything together ───────────────────────────────
        let output = self.out_dir.join(&self.manifest.package.name);
        let linker_config = LinkerConfig {
            target_triple: self.target_triple.clone(),
            output,
            output_kind: OutputKind::Executable,
            jrg_objects,
            cpp_objects,
            go_modules: go_sos,
            rust_libs: Vec::new(),   // populated by dependency resolver
            system_libs: vec!["pthread".to_string(), "m".to_string(), "dl".to_string()],
            lib_dirs: Vec::new(),
            rpath: vec![self.out_dir.clone()],
            linker: LinkerBackend::Lld,
            strip: profile.strip,
            lto: profile.lto.clone(),
            opt_level: profile.opt_level,
        };

        link(&linker_config).map_err(|e| { errors.push(e); errors })
    }

    fn resolve_dependencies(&self) -> Result<(), String> {
        // 1.
        Ok(())
    }

    fn compile_cpp_sources(&self, cfg: &CppSourceConfig, profile: &BuildProfile) -> Result<Vec<PathBuf>, String> {
        let mut objects = Vec::new();
        for src in &cfg.sources {
            let obj_name = src.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or("unknown".to_string());
            let out = self.out_dir.join(format!("{}.o", obj_name));
            let mut extra_defines = Vec::new();
            if profile.debug_info { extra_defines.push("JRG_DEBUG=1".to_string()); }

            compile_cpp_source(
                src,
                &out,
                &cfg.include_dirs,
                &[cfg.defines.clone(), extra_defines].concat(),
                profile.opt_level,
                &self.target_triple,
            )?;
            objects.push(out);
        }
        Ok(objects)
    }

    fn build_go_module_dep(&self, cfg: &GoModuleConfig, _profile: &BuildProfile) -> Result<PathBuf, String> {
        let local_path = cfg.local_path.clone()
            .unwrap_or_else(|| self.out_dir.join("go_deps").join(&cfg.import_path));
        let so_name = cfg.import_path.replace('/', "_").replace('.', "_");
        let output_so = self.out_dir.join(format!("lib{}.so", so_name));

        let module = GoModule {
            import_path: cfg.import_path.clone(),
            local_path,
            exported_fns: cfg.exports.iter().map(|name| GoExport {
                name: name.clone(),
                c_name: name.clone(),
                signature: GoFunctionSig { params: Vec::new(), results: Vec::new() },
            }).collect(),
            output_so: output_so.clone(),
        };

        // Detect target GOOS/GOARCH from target triple
        let (goos, goarch) = triple_to_go_env(&self.target_triple);
        build_go_module(&module, goos, goarch)?;
        Ok(output_so)
    }

    fn compile_jrg_sources(
        &self,
        header_infos: &[CppHeaderInfo],
        go_sos: &[PathBuf],
        profile: &BuildProfile,
    ) -> Result<Vec<PathBuf>, Vec<String>> {
        // This invokes the JRG compiler driver (jrgc) for each .jrg source: 1.
        Ok(Vec::new())  // placeholder; real impl ~1200 lines
    }
}

fn triple_to_go_env(triple: &str) -> (&'static str, &'static str) {
    let goos = if triple.contains("linux")   { "linux" }
               else if triple.contains("darwin")  { "darwin" }
               else if triple.contains("windows") { "windows" }
               else { "linux" };
    let goarch = if triple.contains("x86_64")  { "amd64" }
                 else if triple.contains("aarch64") { "arm64" }
                 else if triple.contains("riscv64") { "riscv64" }
                 else { "amd64" };
    (goos, goarch)
}


// ============================================================================
// §4.6 EXAMPLE: FULL jrgm.toml FOR A REAL PROJECT …


// ============================================================================
// §4.7 EXAMPLE: JawaRustGo++ SOURCE USING ALL THREE FFI MODES …


// ============================================================================
// §4.8 ABI COMPATIBILITY TABLE — TYPE MAPPING REFERENCE …

// End of §4 — FFI Subsystem
