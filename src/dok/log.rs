// src/dok/log.rs — dok::log — logging leveled + berwarna + terstruktur, opsional ke file.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::interpreter::{err_value, ok_value, Value};

static LOG_LEVEL: AtomicU8 = AtomicU8::new(2); // default: info
static LOG_COLOR: AtomicBool = AtomicBool::new(true);

static LOG_FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
fn log_file() -> &'static Mutex<Option<std::fs::File>> { LOG_FILE.get_or_init(|| Mutex::new(None)) }

static LOG_NAME: OnceLock<Mutex<Option<String>>> = OnceLock::new();
fn log_name() -> &'static Mutex<Option<String>> { LOG_NAME.get_or_init(|| Mutex::new(None)) }

const LEVEL_NAMES: [&str; 6] = ["TRACE", "DEBUG", "INFO ", "WARN ", "ERROR", "FATAL"];
const LEVEL_COLORS: [&str; 6] = [
    "\x1b[90m",   // trace: abu-abu
    "\x1b[36m",   // debug: cyan
    "\x1b[32m",   // info:  hijau
    "\x1b[33m",   // warn:  kuning
    "\x1b[31m",   // error: merah
    "\x1b[1;31m", // fatal: merah tebal
];
const RESET: &str = "\x1b[0m";

fn parse_level(s: &str) -> Result<usize, String> {
    match s.to_lowercase().as_str() {
        "trace" => Ok(0),
        "debug" => Ok(1),
        "info" => Ok(2),
        "warn" | "warning" => Ok(3),
        "error" => Ok(4),
        "fatal" => Ok(5),
        other => Err(format!("dok::log: level tidak dikenal `{}` (pakai trace/debug/info/warn/error/fatal)", other)),
    }
}

use crate::interpreter::builtin::time_ext::civil_from_days;

fn now_parts() -> (i64, u32, u32, u32, u32, u32, u32) {
    let dur = crate::host::sejak_epoch().unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let ms = dur.subsec_millis();
    let days = secs.div_euclid(86400);
    let secs_of_day = secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let h = (secs_of_day / 3600) as u32;
    let mi = ((secs_of_day % 3600) / 60) as u32;
    let s = (secs_of_day % 60) as u32;
    (y, mo, d, h, mi, s, ms)
}

fn format_line(level: usize, msg: &str, fields: &str) -> String {
    let (y, mo, d, h, mi, s, ms) = now_parts();
    let name_prefix = match &*log_name().lock().unwrap_or_else(|p| p.into_inner()) {
        Some(n) => format!(" [{}]", n),
        None => String::new(),
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} [{}]{} {}{}",
        y, mo, d, h, mi, s, ms, LEVEL_NAMES[level], name_prefix, msg, fields
    )
}

fn emit(level: usize, line: &str) {
    if LOG_COLOR.load(Ordering::Relaxed) {
        crate::host::galat_baris(&format!("{}{}{}", LEVEL_COLORS[level], line, RESET));
    } else {
        crate::host::galat_baris(line);
    }
    if let Some(f) = log_file().lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        let _ = writeln!(f, "{}", line);
    }
}

fn do_log(level: usize, args: &[Value]) -> Result<Value, String> {
    if level < LOG_LEVEL.load(Ordering::Relaxed) as usize { return Ok(Value::Unit); }
    let msg = args.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ");
    emit(level, &format_line(level, &msg, ""));
    Ok(Value::Unit)
}

pub fn log_trace(args: &[Value]) -> Result<Value, String> { do_log(0, args) }
pub fn log_debug(args: &[Value]) -> Result<Value, String> { do_log(1, args) }
pub fn log_info(args: &[Value]) -> Result<Value, String> { do_log(2, args) }
pub fn log_warn(args: &[Value]) -> Result<Value, String> { do_log(3, args) }
pub fn log_error(args: &[Value]) -> Result<Value, String> { do_log(4, args) }

/// Log level FATAL lalu hentikan proses (kode keluar 1) — sesuai konvensi
/// umum "fatal" di kebanyakan logging library (zerolog, log4j, dst).
pub fn log_fatal(args: &[Value]) -> Result<Value, String> {
    do_log(5, args)?;
    crate::host::keluar(1);
}

pub fn log_with(args: &[Value]) -> Result<Value, String> {
    let level = match args.get(0) {
        Some(Value::Str(s)) => parse_level(s)?,
        _ => return Err("dok::log_with(level, msg, fields): argumen 1 harus string level".to_string()),
    };
    let msg = match args.get(1) { Some(Value::Str(s)) => s.clone(), Some(o) => format!("{}", o), None => String::new() };
    let fields_arr = match args.get(2) { Some(Value::Array(a)) => a.clone(), _ => Vec::new().into() };

    if level < LOG_LEVEL.load(Ordering::Relaxed) as usize { return Ok(Value::Unit); }

    let mut fstr = String::new();
    for item in &fields_arr {
        if let Value::Tuple(t) = item {
            if let (Some(Value::Str(k)), Some(v)) = (t.get(0), t.get(1)) {
                match v {
                    Value::Str(s) => fstr.push_str(&format!(" {}=\"{}\"", k, s)),
                    other => fstr.push_str(&format!(" {}={}", k, other)),
                }
            }
        }
    }
    emit(level, &format_line(level, &msg, &fstr));
    Ok(Value::Unit)
}

pub fn log_set_level(args: &[Value]) -> Result<Value, String> {
    let s = match args.get(0) { Some(Value::Str(s)) => s.clone(), _ => return Err("dok::log_set_level(level)".to_string()) };
    let lvl = parse_level(&s)?;
    LOG_LEVEL.store(lvl as u8, Ordering::Relaxed);
    Ok(Value::Unit)
}

pub fn log_to_file(args: &[Value]) -> Result<Value, String> {
    let path = match args.get(0) { Some(Value::Str(s)) => s.clone(), _ => return Err("dok::log_to_file(path)".to_string()) };
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            *log_file().lock().unwrap_or_else(|p| p.into_inner()) = Some(f);
            Ok(ok_value(Value::Nil))
        }
        Err(e) => Ok(err_value(format!("dok::log_to_file '{}': {}", path, crate::host::galat_io(&e)))),
    }
}

pub fn log_set_name(args: &[Value]) -> Result<Value, String> {
    let name = match args.get(0) { Some(Value::Str(s)) => s.clone(), _ => return Err("dok::log_set_name(name)".to_string()) };
    *log_name().lock().unwrap_or_else(|p| p.into_inner()) = Some(name);
    Ok(Value::Unit)
}

pub fn log_set_color(args: &[Value]) -> Result<Value, String> {
    let b = match args.get(0) { Some(Value::Bool(b)) => *b, _ => return Err("dok::log_set_color(bool)".to_string()) };
    LOG_COLOR.store(b, Ordering::Relaxed);
    Ok(Value::Unit)
}
