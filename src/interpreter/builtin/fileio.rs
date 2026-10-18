// src/interpreter/builtin/fileio.rs — interpreter/builtin/fileio.rs
// Dispatch untuk namespace `file::` — baca/tulis file lewat `std::fs`

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter, ok_value, err_value};
use crate::runtime_err;
use super::NativeFnKind;

impl Interpreter {
    pub(super) fn dispatch_file(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind,
            FileRead | FileWrite | FileAppend | FileExists | FileDelete |
            FileReadLines | FileListDir
        ) {
            return None;
        }
        Some(Self::dispatch_file_inner(kind, args))
    }

    pub fn dispatch_file_inner(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        fn fst(args: &[Value]) -> Value { args.get(0).cloned().unwrap_or(Value::Nil) }
        fn path_of(v: &Value, fname: &str) -> Result<String, Signal> {
            match v {
                Value::Str(s) => Ok(s.clone()),
                o => Err(Signal::RuntimeError(RuntimeError::new(format!(
                    "{}: path harus string, dapat `{}`", fname, o.type_name()
                )))),
            }
        }
        match kind {
            FileRead => {
                let path = path_of(&fst(args), "file::read")?;
                Ok(match std::fs::read_to_string(&path) {
                    Ok(s)  => ok_value(Value::Str(s)),
                    Err(e) => err_value(format!("file::read: gagal baca `{}` — {}", path, crate::host::galat_io(&e))),
                })
            }

            FileWrite => {
                if args.len() < 2 { return runtime_err!("file::write butuh (path, isi)"); }
                let path = path_of(&args[0], "file::write")?;
                let content = format!("{}", args[1]);
                Ok(match std::fs::write(&path, content) {
                    Ok(())  => ok_value(Value::Unit),
                    Err(e) => err_value(format!("file::write: gagal tulis `{}` — {}", path, crate::host::galat_io(&e))),
                })
            }

            FileAppend => {
                if args.len() < 2 { return runtime_err!("file::append butuh (path, isi)"); }
                let path = path_of(&args[0], "file::append")?;
                let content = format!("{}", args[1]);
                use std::io::Write;
                let result = std::fs::OpenOptions::new()
                    .create(true).append(true).open(&path)
                    .and_then(|mut f| f.write_all(content.as_bytes()));
                Ok(match result {
                    Ok(())  => ok_value(Value::Unit),
                    Err(e) => err_value(format!("file::append: gagal tambah isi ke `{}` — {}", path, crate::host::galat_io(&e))),
                })
            }

            FileExists => {
                let path = path_of(&fst(args), "file::exists")?;
                Ok(Value::Bool(std::path::Path::new(&path).exists()))
            }

            FileDelete => {
                let path = path_of(&fst(args), "file::delete")?;
                Ok(match std::fs::remove_file(&path) {
                    Ok(())  => ok_value(Value::Unit),
                    Err(e) => err_value(format!("file::delete: gagal hapus `{}` — {}", path, crate::host::galat_io(&e))),
                })
            }

            FileReadLines => {
                let path = path_of(&fst(args), "file::read_lines")?;
                Ok(match std::fs::read_to_string(&path) {
                    Ok(s) => ok_value(Value::Array(
                        s.lines().map(|l| Value::Str(l.to_string())).collect()
                    )),
                    Err(e) => err_value(format!("file::read_lines: gagal baca `{}` — {}", path, crate::host::galat_io(&e))),
                })
            }

            FileListDir => {
                let path = path_of(&fst(args), "file::list_dir")?;
                Ok(match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        let mut names: Vec<Value> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| Value::Str(e.file_name().to_string_lossy().into_owned()))
                            .collect();
                        names.sort_by(|a, b| format!("{}", a).cmp(&format!("{}", b)));
                        ok_value(Value::Array(names.into()))
                    }
                    Err(e) => err_value(format!("file::list_dir: gagal buka folder `{}` — {}", path, crate::host::galat_io(&e))),
                })
            }

            _ => runtime_err!("dispatch_file_inner: kind tidak dikenal"),
        }
    }
}
