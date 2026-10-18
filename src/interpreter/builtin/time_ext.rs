// src/interpreter/builtin/time_ext.rs — interpreter/builtin/time_ext.rs
// Perluasan namespace `time::` — di atas `TimeSleepMs`/`TimeNowMs` yang

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use crate::runtime_err;
use super::NativeFnKind;

/// Howard Hinnant's `civil_from_days`: hari-sejak-epoch (1970-01-01 = 0) ->
/// (tahun, bulan 1..=12, tanggal 1..=31).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;                                          // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;      // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);                   // [0, 365]
    let mp = (5 * doy + 2) / 153;                                        // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;                       // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;              // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const HARI: [&str; 7] = ["Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu", "Minggu"];

fn breakdown(ms: i128) -> (i64, u32, u32, u32, u32, u32, u32, &'static str) {
    let secs_total = ms.div_euclid(1000) as i64;
    let days = secs_total.div_euclid(86400);
    let secs_of_day = secs_total.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let hh = (secs_of_day / 3600) as u32;
    let mm = ((secs_of_day % 3600) / 60) as u32;
    let ss = (secs_of_day % 60) as u32;
    let wd_mon0 = (((days % 7) + 10) % 7) as usize;
    (y, mo, d, hh, mm, ss, ms.rem_euclid(1000) as u32, HARI[wd_mon0])
}

/// Terapkan pola gaya `strftime` atas sebuah cap waktu milidetik.
fn format_waktu(ms: i128, pola: &str) -> Result<String, Signal> {
    let (y, mo, d, hh, mm, ss, milli, hari) = breakdown(ms);
    let mut keluar = String::with_capacity(pola.len() + 16);
    let mut c = pola.chars().peekable();
    while let Some(ch) = c.next() {
        if ch != '%' {
            keluar.push(ch);
            continue;
        }
        let Some(arahan) = c.next() else {
            return Err(Signal::RuntimeError(RuntimeError::new(
                "time::format: pola berakhir dengan `%` menggantung".to_string())));
        };
        match arahan {
            'Y' => keluar.push_str(&format!("{:04}", y)),
            'y' => keluar.push_str(&format!("{:02}", (y % 100 + 100) % 100)),
            'm' => keluar.push_str(&format!("{:02}", mo)),
            'd' => keluar.push_str(&format!("{:02}", d)),
            'H' => keluar.push_str(&format!("{:02}", hh)),
            'M' => keluar.push_str(&format!("{:02}", mm)),
            'S' => keluar.push_str(&format!("{:02}", ss)),
            'L' => keluar.push_str(&format!("{:03}", milli)),
            'A' => keluar.push_str(hari),
            'B' => keluar.push_str(BULAN[(mo as usize).clamp(1, 12) - 1]),
            '%' => keluar.push('%'),
            lain => {
                return Err(Signal::RuntimeError(RuntimeError::new(format!(
                    "time::format: arahan `%{}` tidak dikenal.\n  \
                     bantuan: yang ada — %Y tahun, %y dua digit, %m bulan, %d tanggal, \
                     %H jam, %M menit, %S detik, %L milidetik, %A nama hari, \
                     %B nama bulan, %% tanda persen", lain))))
            }
        }
    }
    Ok(keluar)
}

/// Nama bulan, sejajar dengan `HARI` yang sudah ada.
/// Cap waktu terjauh yang diterima `time::`, dalam milidetik dari 1970.
const BATAS_MS: i128 = 8_640_000_000_000_000;

const BULAN: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni",
    "Juli", "Agustus", "September", "Oktober", "November", "Desember",
];

impl Interpreter {
    pub(super) fn dispatch_time_ext(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind,
            TimeNow | TimeSleep | TimeSleepMs | TimeYear | TimeMonth | TimeDay |
            TimeHour | TimeMinute | TimeSecond | TimeWeekday | TimeFormat
        ) {
            return None;
        }
        Some(Self::dispatch_time_ext_inner(kind, args))
    }

    pub fn dispatch_time_ext_inner(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        fn fst(args: &[Value]) -> Value { args.get(0).cloned().unwrap_or(Value::Nil) }
        // `time::now()` dipakai sebagai default kalau argumen ms nggak dikasih,
        // jadi `time::year()` (tanpa arg) berarti "tahun sekarang".
        fn now_ms() -> i128 {
            crate::host::sejak_epoch()
                .map(|d| d.as_millis() as i128)
                .unwrap_or(0)
        }
        /// Argumen waktu, dalam MILIDETIK.
        fn ms_arg(args: &[Value]) -> Result<i128, Signal> {
            match args.first() {
                None => Ok(now_ms()),
                Some(Value::Float(_)) => runtime_err!(
                    "time:: menerima MILIDETIK (bilangan bulat), dapat pecahan.\n  \
                     bantuan: `time::now()` menjawab DETIK — pakai `time::now_ms()`"),
                Some(_) => {
                    let ms = crate::interpreter::eval_expr::nilai_ke_i128(&args[0])?;
                    // Batas yang sama dengan `Date` JavaScript: di luar itu hitungan kalendernya
                    // tidak muat 64 bit dan dulu menjawab tahun negatif.
                    if ms.unsigned_abs() > BATAS_MS as u128 {
                        return runtime_err!("time:: cap waktu `{}` milidetik di luar jangkauan — \
                            yang sah -{b} sampai {b} (sekitar 275 ribu tahun dari 1970)", ms, b = BATAS_MS);
                    }
                    Ok(ms)
                }
            }
        }
        match kind {
            TimeNow => Ok(Value::Float(now_ms() as f64 / 1000.0)),

            TimeSleep => {
                let secs = crate::interpreter::eval_expr::nilai_ke_f64(&fst(args))?;
                if !secs.is_finite() || secs > 315_360_000.0 /* ~10 tahun, batas akal sehat */ {
                    return runtime_err!("time::sleep: durasi `{}` detik tidak masuk akal — pakai angka detik yang wajar (0 untuk tanpa delay)", secs);
                }
                if secs > 0.0 { crate::host::tidur(std::time::Duration::from_secs_f64(secs)); }
                Ok(Value::Unit)
            }

            // SATU definisi untuk dua mesin. Durasi negatif dijepit ke 0 (sisa waktu
            // sebuah bingkai bisa negatif); yang bukan angka dulu diam-diam 0 ms.
            TimeSleepMs => {
                let ms = match args.first() {
                    Some(Value::Int(n)) => *n as f64,
                    Some(Value::Float(f)) if !f.is_nan() => *f,
                    Some(lain) => return runtime_err!(
                        "time::sleep_ms: butuh angka (milidetik), dapat `{}`", lain.type_name()),
                    None => 0.0,
                };
                if ms > 315_360_000_000.0 /* ~10 tahun, batas yang sama dengan `time::sleep` */ {
                    return runtime_err!("time::sleep_ms: durasi `{}` milidetik tidak masuk akal — \
                                         pakai angka yang wajar (0 untuk tanpa delay)", ms);
                }
                if ms > 0.0 { crate::host::tidur(std::time::Duration::from_secs_f64(ms / 1000.0)); }
                Ok(Value::Unit)
            }

            TimeYear   => { let ms = ms_arg(args)?; Ok(Value::Int(breakdown(ms).0 as i128)) }
            TimeMonth  => { let ms = ms_arg(args)?; Ok(Value::Int(breakdown(ms).1 as i128)) }
            TimeDay    => { let ms = ms_arg(args)?; Ok(Value::Int(breakdown(ms).2 as i128)) }
            TimeHour   => { let ms = ms_arg(args)?; Ok(Value::Int(breakdown(ms).3 as i128)) }
            TimeMinute => { let ms = ms_arg(args)?; Ok(Value::Int(breakdown(ms).4 as i128)) }
            TimeSecond => { let ms = ms_arg(args)?; Ok(Value::Int(breakdown(ms).5 as i128)) }
            TimeWeekday => { let ms = ms_arg(args)?; Ok(Value::Str(breakdown(ms).7.to_string())) }

            TimeFormat => {
                let ms = ms_arg(args)?;
                match args.get(1) {
                    None => {
                        let (y, mo, d, hh, mm, ss, _, _) = breakdown(ms);
                        Ok(Value::Str(format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, hh, mm, ss)))
                    }
                    Some(Value::Str(pola)) => Ok(Value::Str(format_waktu(ms, pola)?)),
                    Some(lain) => runtime_err!(
                        "time::format: polanya harus string, dapat `{}`", lain.type_name()),
                }
            }

            _ => runtime_err!("dispatch_time_ext_inner: kind tidak dikenal"),
        }
    }
}
