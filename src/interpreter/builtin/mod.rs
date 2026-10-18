// src/interpreter/builtin/mod.rs — interpreter/builtin/mod.rs
// Rumah semua fungsi native JRG: daftar ID-nya (`NativeFnKind`),

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, Write};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter,
    ok_value, err_value, some_value, none_value,
};
use crate::runtime_err;

mod math;
mod arr;
pub(crate) mod str_ops;
// Dipakai backend LLVM: ia mem-parsing template `str::format` saat kompilasi,
// jadi ia tidak bisa memanggil implementasi yang bekerja atas `Value` — tapi
// ATURAN "apa yang dianggap placeholder" harus tetap satu.
pub use str_ops::SegmenFormat;
mod dok;
pub(crate) mod heap;
pub(crate) mod dict;
mod rand;
pub(crate) mod time_ext;
mod fileio;
mod sysenv;
mod php;
mod lua;
pub mod ffi_pintu;
pub(crate) mod jembatan_stdlib;
pub(crate) mod aritas;

pub(crate) use arr::{fisher_yates_shuffle, Pasti};
pub(crate) use math::jenis_metode_math;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NativeFnKind {
    // fmt / IO
    FmtWok, FmtWokNoNl, FmtTok, FmtEprint, FmtSprint,
    // type conversion
    ToInt, ToFloat, ToBool, ToStr, ToList, ToUint, ToRune,
    // builtins
    Len, ParseInt, ParseFloat, Typeof, Assert, AssertEq, Panik, NumCpus,
    /// `terap(f, args)` — panggil `f` dengan isi array `args` sebagai
    /// argumennya. Ini mesin di balik gula `f(...v)`.
    Terap,
    /// `terap_bernama(f, posisional, nama, nilai)` — mesin di balik gula
    /// `f(a, b: 2)`.
    TerapBernama,
    Range, RangeInclusive, Exit,
    ArrCountIf, ArrMaxBy, ArrMinBy, ArrSortByKey, ArrFold,
    // math::
    MathAbs, MathSqrt, MathPow, MathFloor, MathCeil, MathRound,
    MathSin, MathCos, MathTan, MathAsin, MathAcos, MathAtan, MathAtan2,
    MathLn, MathLog10, MathLog2, MathExp,
    MathMin, MathMax, MathClamp, MathIsNan, MathIsInf, MathSign,
    MathFact, MathGcd, MathLcm, MathPi, MathE,
    // arr::
    ArrMap, ArrFilter, ArrReduce, ArrAny, ArrAll, ArrFind, ArrFindIdx,
    ArrEach,
    ChanNew,
    ArrFlatten, ArrZip, ArrUnique, ArrSort, ArrSortBy,
    ArrReverse, ArrSum, ArrProduct, ArrMean, ArrMedian, ArrVariance,
    ArrStdDev, ArrMin, ArrMax, ArrContains, ArrIndexOf, ArrJoin,
    ArrPush, ArrPop, ArrInsert, ArrRemove, ArrTake, ArrSkip,
    ArrChunks, ArrWindows, ArrRepeat, ArrFill, ArrEnumerate,
    // str::
    StrFormat, StrPadLeft, StrPadRight, StrCenter, StrRepeat,
    StrWords, StrLines, StrIsNumeric, StrIsAlpha, StrIsAlphaNum,
    StrTitle, StrSnakeCase, StrCamelCase, StrReverse, StrCount,
    StrStartsWith, StrEndsWith, StrContains, StrSplit, StrJoin,
    StrTrim, StrUpper, StrLower, StrReplace, StrSlice, StrCharAt,
    StrBytes, StrChars, StrFind,
    // canvas::
    CanvasNew,
    // time:: — a small, natural complement to goroutines/channels: realistic
    // concurrent examples (worker pools, polling `select` loops, simple rate …
    TimeSleepMs, TimeNowMs,
    // py:: — Python interop via a persistent subprocess worker (see
    // crate::ffi::python).
    PyEval, PyExec, PyCall, PyImport, PyCallKw,
    PyRef, PyGet, PyMethod, PyItem, PyToJrg, PyRepr, PyRefs,
    // Saudara "pertahankan handle" dari empat di atas. Lihat komentar di
    // arm-nya untuk kenapa keempatnya perlu ada.
    PyRefGet, PyRefMethod, PyRefItem, PyRefCall,
    PyMethodKw, PyRefMethodKw, PyLen, PyDir, PySet, PySetItem, PyDelItem, PyHas,
    // js:: — FFI JavaScript (crate::ffi::javascript).
    JsEval, JsExec, JsImport, JsCall, JsRefE,
    JsGet, JsMethod, JsItem, JsToJrg, JsRepr, JsRefs,
    JsRefGet, JsRefMethod, JsRefItem, JsRefCall,
    JsLen, JsDir, JsSet, JsSetItem, JsDelItem, JsHas,
    // php:: — FFI PHP (crate::ffi::php); badannya di `builtin/php.rs`.
    PhpEval, PhpExec, PhpRef, PhpCall, PhpRefCall, PhpCallKw, PhpNew, PhpNewKw,
    PhpMethod, PhpRefMethod, PhpMethodKw, PhpGet, PhpRefGet, PhpSet,
    PhpItem, PhpRefItem, PhpSetItem, PhpDelItem, PhpHas, PhpLen, PhpDir,
    PhpRepr, PhpToJrg, PhpRefs, PhpRequire, PhpRender, PhpRenderStr, PhpTersedia,
    // lua:: — FFI Lua (crate::ffi::lua); badannya di `builtin/lua.rs`.
    LuaEval, LuaExec, LuaCall, LuaRequire, LuaTersedia,
    // `Future::`/`Task::` — lihat `interpreter/tugas.rs` untuk kenapa
    // interpreter punya implementasinya sendiri alih-alih menjembatani.
    FutureNew, FutureAwait, FutureIsDone, TaskSpawn, TaskJoin,
    FutureAll, FutureRace, FutureTimeout, FutureTryGet,

    // ── dok:: — library "full power": networking, HTTP, JSON, numerik tajam,
    // logging.
    DokTcpConnect, DokTcpSend, DokTcpRecv, DokTcpRecvLine, DokTcpRecvAll,
    DokTcpClose, DokTcpSetTimeout, DokTcpLocalAddr, DokTcpPeerAddr,
    DokTcpListen, DokTcpAccept, DokTcpCloseListener,
    DokUdpBind, DokUdpSendTo, DokUdpRecvFrom, DokUdpClose, DokResolve,

    DokHttpGet, DokHttpPost, DokHttpPut, DokHttpDelete, DokHttpRequest,
    DokUrlEncode, DokUrlDecode, DokHeaderGet, DokQueryGet,

    DokRouterNew, DokRouterGet, DokRouterPost, DokRouterPut, DokRouterDelete,
    DokRouterPatch, DokRouterAny, DokRouterStatic, DokRouterNotFound, DokServe,
    DokResponse, DokText, DokHtml, DokJsonResponse, DokRedirect,
    // Fase WEB-3: driver SQLite.
    DokDbOpen, DokDbClose, DokDbExec, DokDbQuery, DokDbLastId, DokDbTersedia,
    // Fase WEB-2: WebSocket.
    DokRouterWs, DokWsSend, DokWsRecv, DokWsClose, DokWsTerbuka,
    // Fase WEB-1: middleware, cookie, sesi, kata sandi, template.
    DokRouterUse, DokRouterAfter, DokCookieGet, DokCookieSet, DokHeaderSet, DokSessionEncode, DokSessionDecode, DokPasswordHash, DokPasswordVerify, DokRender, DokRenderFile, DokEscapeHtml,

    DokJsonEncode, DokJsonDecode, DokJsonGet, DokJsonSet, DokJsonKeys,

    DokVecAdd, DokVecSub, DokVecScale, DokVecDot, DokVecCross, DokVecNorm,
    DokVecNormalize, DokVecLerp, DokVecDistance, DokVecAngle,
    DokMatNew, DokMatIdentity, DokMatMul, DokMatAdd, DokMatSub, DokMatTranspose,
    DokMatDet, DokMatInverse, DokMatGet, DokMatSet,
    DokStatCorrelation, DokStatCovariance, DokStatPercentile, DokStatMode,
    DokStatLinreg, DokStatZscore, DokStatNormalPdf, DokStatNormalCdf,
    DokNumIsPrime, DokNumNextPrime, DokNumPrimeFactors, DokNumModPow,
    DokNumToBase, DokNumFromBase, DokNumSolve,

    DokLogTrace, DokLogDebug, DokLogInfo, DokLogWarn, DokLogError, DokLogFatal,
    DokLogWith, DokLogSetLevel, DokLogToFile, DokLogSetName, DokLogSetColor,

    // ptr:: — referensi/pointer sungguhan (shared mutable cell).
    PtrNew, PtrNull, PtrIsNull, PtrAddr, PtrGet, PtrSet, PtrFree, PtrUpdate, PtrInc, PtrDec, MutexWithLock,

    // crypto:: — hashing, HMAC, CRC32, base64/hex, random aman, AES-256-CBC.
    CryptoMd5, CryptoSha1, CryptoSha256, CryptoSha512, CryptoHmacSha256, CryptoCrc32,
    CryptoBase64Encode, CryptoBase64Decode, CryptoHexEncode, CryptoHexDecode,
    CryptoRandomHex, CryptoRandomInt,
    CryptoAesGenKey, CryptoAesGenIv, CryptoAesEncrypt, CryptoAesDecrypt,

    // new:: / delete:: — manajemen memori manual (lihat src/dok/heap.rs).
    NewAlloc, NewGet, NewSet,
    NewArray, NewArrayGet, NewArraySet, NewArrayPush, NewArrayPop, NewArrayLen, NewArrayToList,
    NewBuffer, NewBufferFromHex, NewBufferGet, NewBufferSet, NewBufferLen, NewBufferFill, NewBufferToHex,
    NewIsValid, NewSizeof, NewStats,
    DeleteFree,

    // ── arr:: (tambahan) — helper koleksi sehari-hari yang tadinya belum ada:
    // ambil ujung, cek kosong, gabung, transformasi + flatten sekaligus …
    ArrFirst, ArrLast, ArrIsEmpty, ArrConcat, ArrSebar, ArrFlatMap, ArrGroupBy,
    ArrPartition, ArrZipWith, ArrShuffle, ArrSample, ArrSwap, ArrRotate,

    // ── str:: (tambahan)
    StrCapitalize, StrIsEmpty, StrStripPrefix, StrStripSuffix, StrSubstr,
    StrIsUpper, StrIsLower, StrSwapCase,

    // ── math:: (tambahan) — konstanta tambahan + fungsi yang lumrah
    // dibutuhkan tapi belum ada (jarak 2D, log basis-bebas, konversi
    // sudut, pembulatan ke arah nol, salin-tanda).
    MathTau, MathInf, MathNan, MathHypot, MathLog, MathDegrees, MathRadians,
    MathTrunc, MathCopysign, MathWrapAdd, MathWrapSub, MathWrapMul,

    // ── dict:: — peta key/value dinamis ala `dict` Python (lihat
    // `builtin/dict.rs`); dibangun di atas `Value::Struct` yang sudah
    // ada, jadi TIDAK butuh varian `Value` baru.
    DictNew, DictFromPairs, DictSet, DictGet, DictGetOr, DictHas, DictRemove,
    DictKeys, DictValues, DictItems, DictLen, DictIsEmpty, DictMerge, DictIsDict,

    // ── rand:: — bungkusan ramah-pemula di atas RNG aman yang sama
    // dengan `crypto::random_*` (lihat `builtin/rand.rs`).
    RandInt, RandFloat, RandBool, RandChoice, RandShuffle, RandSample,

    // ── time:: (tambahan) — pemecahan kalender dari epoch-ms, dihitung
    // sendiri tanpa crate tanggal (lihat `builtin/time_ext.rs`).
    // `TimeSleepMs`/`TimeNowMs` yang sudah ada TIDAK disentuh.
    TimeNow, TimeSleep, TimeYear, TimeMonth, TimeDay, TimeHour, TimeMinute,
    TimeSecond, TimeWeekday, TimeFormat,

    // ── file:: — baca/tulis file lewat `std::fs`, tiap operasi balik
    // `Result` (lihat `builtin/fileio.rs`).
    FileRead, FileWrite, FileAppend, FileExists, FileDelete, FileReadLines, FileListDir,

    // ── env:: — environment variable & argumen CLI (lihat `builtin/sysenv.rs`).
    EnvGet, EnvGetOr, EnvSet, EnvArgs, EnvExe,

    /// Builtin yang implementasinya tinggal di stdlib VM (`src/stdlib/`),
    /// dipanggil interpreter lewat jembatan nilai.
    DariStdlibVm(&'static str),

    /// `chan::<nama>(c, ...)` — bentuk namespace method channel; namanya nama method.
    ChanNs(&'static str),
}

// ---------------------------------------------------------------------------
// Registrasi builtin
// ---------------------------------------------------------------------------

impl Env {
    /// Nama builtin FFI (`py::`, `js::`, `php::`, `lua::`). DITURUNKAN dengan
    /// mendaftarkannya ke sebuah `Env` kosong, bukan ditulis tangan.
    pub fn nama_ffi() -> Vec<String> {
        let mut e = Env::new();
        e.register_builtins();
        let g = e.globals.lock().unwrap_or_else(|p| p.into_inner());
        let mut v: Vec<String> = g.keys()
            .filter(|n| ffi_pintu::NAMESPACE_FFI.iter().any(|awalan| n.starts_with(awalan)))
            .cloned().collect();
        v.sort();
        v
    }

    fn n(&mut self, name: &str, k: NativeFnKind) {
        self.globals.lock().unwrap_or_else(|p| p.into_inner()).insert(name.to_string(), Value::NativeFn(k));
    }

    /// Like `n`, but for any value — used below to register the built-in
    /// `Result`/`Option` constructors (`Value::EnumCtor` / `Value::Enum`
    /// values), not just native functions.
    fn v(&mut self, name: &str, val: Value) {
        self.globals.lock().unwrap_or_else(|p| p.into_inner()).insert(name.to_string(), val);
    }


    /// Daftarkan semua fungsi native + shortcut namanya ke global env.
    /// Dipanggil sekali dari `Env::new()`.
    pub(crate) fn register_builtins(&mut self) {
        use NativeFnKind::*;
        // fmt / IO — full qualified + short aliases (using namespace fmt)
        self.n("fmt::wok",    FmtWok);    self.n("wok",    FmtWok);
        self.n("fmt::wok_",   FmtWokNoNl);self.n("wok_",   FmtWokNoNl);
        self.n("fmt::tok",    FmtTok);    self.n("tok",    FmtTok);
        self.n("fmt::eprint", FmtEprint); self.n("eprint", FmtEprint);
        self.n("fmt::sprint", FmtSprint); self.n("sprint", FmtSprint);
        // type conversion
        self.n("int",   ToInt);   self.n("float", ToFloat);
        self.n("bool",  ToBool);  self.n("str",   ToStr);
        // Empat nama yang dulu CUMA hidup di `--vm`, terdaftar di registry (jadi
        // muncul di `:cari`) dan punya entri arity, tapi ditolak interpreter sebagai …
        self.n("uint",  ToUint);  self.n("rune",  ToRune);
        self.n("range_inclusive", RangeInclusive);
        // `sleep` adalah nama pendek `time::sleep` (DETIK): dulu salinan kedua membaca
        // bilangan bulat sebagai MILIDETIK, jadi `sleep(2)` tidur 2 ms dan `sleep(-1)` selamanya.
        self.n("sleep", TimeSleep);
        self.n("list",  ToList);
        // builtins
        self.n("len",         Len);
        self.n("parse_int",   ParseInt);
        self.n("parse_float", ParseFloat);
        self.n("typeof",      Typeof);
        self.n("terap",       Terap);
        self.n("terap_bernama", TerapBernama);
        self.n("assert",      Assert);
        self.n("panik",       Panik);
        self.n("assert_eq",   AssertEq);
        self.n("num_cpus",    NumCpus);
        self.n("range",       Range);
        // Alias, bukan salinan: dulu jenis sendiri yang cuma beda kalimat galat.
        self.n("enumerate",   ArrEnumerate);
        self.n("zip",         ArrZip);
        self.n("exit",        Exit); self.n("std::exit", Exit);
        // math — qualified + short
        self.n("math::abs",       MathAbs);       self.n("abs",   MathAbs);
        self.n("math::sqrt",      MathSqrt);      self.n("sqrt",  MathSqrt);
        self.n("math::pow",       MathPow);       self.n("pow",   MathPow);
        self.n("math::floor",     MathFloor);     self.n("floor", MathFloor);
        self.n("math::ceil",      MathCeil);      self.n("ceil",  MathCeil);
        self.n("math::round",     MathRound);     self.n("round", MathRound);
        self.n("math::sin",       MathSin);       self.n("sin",   MathSin);
        self.n("math::cos",       MathCos);       self.n("cos",   MathCos);
        self.n("math::tan",       MathTan);       self.n("tan",   MathTan);
        self.n("math::asin",      MathAsin);
        self.n("math::acos",      MathAcos);
        self.n("math::atan",      MathAtan);
        self.n("math::atan2",     MathAtan2);
        self.n("math::ln",        MathLn);        self.n("ln",    MathLn);
        self.n("math::log10",     MathLog10);     self.n("log10", MathLog10);
        self.n("math::log2",      MathLog2);      self.n("log2",  MathLog2);
        self.n("math::exp",       MathExp);       self.n("exp",   MathExp);
        self.n("math::min",       MathMin);
        self.n("math::max",       MathMax);
        self.n("math::clamp",     MathClamp);     self.n("clamp", MathClamp);
        self.n("math::is_nan",    MathIsNan);
        self.n("math::is_inf",    MathIsInf);
        self.n("math::sign",      MathSign);
        self.n("math::factorial", MathFact);
        self.n("math::gcd",       MathGcd);
        self.n("math::lcm",       MathLcm);
        self.n("math::PI",        MathPi);
        self.n("math::E",         MathE);
        // arr::
        self.n("arr::map",       ArrMap);       self.n("arr::filter",    ArrFilter);
        // `arr::each` — satu-satunya HOF yang bentuk NAMESPACE-nya hilang di sini
        // sementara ada di VM.
        self.n("arr::each",      ArrEach);
        // `Chan::new(n)` — bentuk FUNGSI untuk membuat channel.
        self.n("Chan::new",      ChanNew);
        // `chan::make` alias `Chan::new`; sisanya bentuk namespace method channel.
        self.n("chan::make",     ChanNew);
        for m in ["send", "recv", "try_send", "try_recv", "close", "len", "cap", "is_closed", "has_value"] {
            self.n(&format!("chan::{}", m), ChanNs(m));
        }
        self.n("arr::reduce",    ArrReduce);    self.n("arr::any",       ArrAny);
        self.n("arr::fold",      ArrFold);
        self.n("arr::all",       ArrAll);       self.n("arr::find",      ArrFind);
        self.n("arr::find_idx",  ArrFindIdx);   self.n("arr::count",     ArrCountIf);
        self.n("arr::flatten",   ArrFlatten);   self.n("arr::zip",       ArrZip);
        self.n("arr::unique",    ArrUnique);    self.n("arr::sort",      ArrSort);
        // Empat nama yang selama ini cuma punya bentuk METHOD, sementara
        // registry sudah mengiklankannya sebagai `arr::…`.
        self.n("arr::count_if",  ArrCountIf);   self.n("arr::max_by",    ArrMaxBy);
        self.n("arr::min_by",    ArrMinBy);     self.n("arr::sort_by_key", ArrSortByKey);
        self.n("arr::sort_by",   ArrSortBy);    self.n("arr::reverse",   ArrReverse);
        self.n("arr::sum",       ArrSum);       self.n("arr::product",   ArrProduct);
        self.n("arr::mean",      ArrMean);      self.n("arr::median",    ArrMedian);
        self.n("arr::variance",  ArrVariance);  self.n("arr::std_dev",   ArrStdDev);
        self.n("arr::min",       ArrMin);       self.n("arr::max",       ArrMax);
        self.n("arr::contains",  ArrContains);  self.n("arr::index_of",  ArrIndexOf);
        self.n("arr::join",      ArrJoin);      self.n("arr::push",      ArrPush);
        self.n("arr::pop",       ArrPop);       self.n("arr::insert",    ArrInsert);
        self.n("arr::remove",    ArrRemove);    self.n("arr::take",      ArrTake);
        self.n("arr::skip",      ArrSkip);      self.n("arr::chunks",    ArrChunks);
        self.n("arr::windows",   ArrWindows);   self.n("arr::repeat",    ArrRepeat);
        self.n("arr::fill",      ArrFill);      self.n("arr::enumerate", ArrEnumerate);
        // str::
        self.n("str::format",          StrFormat);
        self.n("str::pad_left",        StrPadLeft);
        self.n("str::pad_right",       StrPadRight);
        self.n("str::center",          StrCenter);
        self.n("str::repeat",          StrRepeat);
        self.n("str::words",           StrWords);
        self.n("str::lines",           StrLines);
        self.n("str::is_numeric",      StrIsNumeric);
        self.n("str::is_alpha",        StrIsAlpha);
        self.n("str::is_alphanumeric", StrIsAlphaNum);
        self.n("str::title",           StrTitle);
        self.n("str::snake_case",      StrSnakeCase);
        self.n("str::camel_case",      StrCamelCase);
        self.n("str::reverse",         StrReverse);
        self.n("str::count",           StrCount);
        self.n("str::starts_with",     StrStartsWith);
        self.n("str::ends_with",       StrEndsWith);
        self.n("str::contains",        StrContains);
        self.n("str::split",           StrSplit);
        self.n("str::join",            StrJoin);
        self.n("str::trim",            StrTrim);
        self.n("str::upper",           StrUpper);
        self.n("str::lower",           StrLower);
        self.n("str::replace",         StrReplace);
        self.n("str::slice",           StrSlice);
        self.n("str::char_at",         StrCharAt);
        self.n("str::bytes",           StrBytes);
        self.n("str::chars",           StrChars);
        self.n("str::find",            StrFind);
        // canvas::
        self.n("Canvas::new", CanvasNew);

        // time::
        self.n("time::sleep_ms", TimeSleepMs);
        self.n("time::now_ms",   TimeNowMs);

        // py:: — Python FFI (crate::ffi::python). See the NativeFnKind
        // match arms in call_native for argument shapes.
        self.n("py::eval",   PyEval);
        self.n("py::exec",   PyExec);
        self.n("py::call",   PyCall);
        self.n("py::import", PyImport);
        self.n("py::call_kw", PyCallKw);
        // Operasi di atas HANDLE — objek yang sengaja ditinggal di
        // sisi Python. Lihat `Value::PyObj`.
        self.n("py::ref",    PyRef);
        self.n("py::get",    PyGet);
        self.n("py::method", PyMethod);
        // Empat ini menjaga hasilnya tetap di sisi Python.
        self.n("py::ref_get",    PyRefGet);
        self.n("py::ref_method", PyRefMethod);
        self.n("py::ref_item",   PyRefItem);
        self.n("py::ref_call",   PyRefCall);
        // Keyword argument PADA METODE.
        self.n("js::eval",              JsEval);
        self.n("js::exec",              JsExec);
        self.n("js::import",            JsImport);
        self.n("js::call",              JsCall);
        self.n("js::ref",               JsRefE);
        self.n("js::get",               JsGet);
        self.n("js::method",            JsMethod);
        self.n("js::item",              JsItem);
        self.n("js::to_jrg",            JsToJrg);
        self.n("js::repr",              JsRepr);
        self.n("js::refs",              JsRefs);
        self.n("js::ref_get",           JsRefGet);
        self.n("js::ref_method",        JsRefMethod);
        self.n("js::ref_item",          JsRefItem);
        self.n("js::ref_call",          JsRefCall);
        self.n("js::len",               JsLen);
        self.n("js::dir",               JsDir);
        self.n("js::set",               JsSet);
        self.n("js::set_item",          JsSetItem);
        self.n("js::del_item",          JsDelItem);
        self.n("js::has",               JsHas);
        // php:: — FFI PHP (crate::ffi::php).
        self.n("php::eval",           PhpEval);
        self.n("php::exec",           PhpExec);
        self.n("php::ref",            PhpRef);
        self.n("php::call",           PhpCall);
        self.n("php::ref_call",       PhpRefCall);
        self.n("php::call_kw",        PhpCallKw);
        self.n("php::new",            PhpNew);
        self.n("php::new_kw",         PhpNewKw);
        self.n("php::method",         PhpMethod);
        self.n("php::ref_method",     PhpRefMethod);
        self.n("php::method_kw",      PhpMethodKw);
        self.n("php::get",            PhpGet);
        self.n("php::ref_get",        PhpRefGet);
        self.n("php::set",            PhpSet);
        self.n("php::item",           PhpItem);
        self.n("php::ref_item",       PhpRefItem);
        self.n("php::set_item",       PhpSetItem);
        self.n("php::del_item",       PhpDelItem);
        self.n("php::has",            PhpHas);
        self.n("php::len",            PhpLen);
        self.n("php::dir",            PhpDir);
        self.n("php::repr",           PhpRepr);
        self.n("php::to_jrg",         PhpToJrg);
        self.n("php::refs",           PhpRefs);
        self.n("php::require",        PhpRequire);
        self.n("php::render",         PhpRender);
        self.n("php::render_str",     PhpRenderStr);
        self.n("php::tersedia",       PhpTersedia);
        self.n("lua::eval",           LuaEval);
        self.n("lua::exec",           LuaExec);
        self.n("lua::call",           LuaCall);
        self.n("lua::require",        LuaRequire);
        self.n("lua::tersedia",       LuaTersedia);
        self.n("py::method_kw",     PyMethodKw);
        self.n("py::ref_method_kw", PyRefMethodKw);
        // Membaca dan MENGUBAH objek Python di tempat.
        self.n("py::len",      PyLen);
        self.n("py::dir",      PyDir);
        self.n("py::set",      PySet);
        self.n("py::set_item", PySetItem);
        self.n("py::del_item", PyDelItem);
        self.n("py::has",      PyHas);
        self.n("Future::new",     FutureNew);
        self.n("future::await",   FutureAwait);
        self.n("future::is_done", FutureIsDone);
        self.n("Task::spawn",     TaskSpawn);
        self.n("task::join",      TaskJoin);
        // Empat ini dulu cuma ada di VM, dan alasan yang tercatat untuk itu ("butuh
        // sel bersama milik VM") tidak pernah berlaku bagi mereka: keempatnya murni …
        self.n("future::all",     FutureAll);
        self.n("future::race",    FutureRace);
        self.n("future::timeout", FutureTimeout);
        self.n("future::try_get", FutureTryGet);
        self.n("py::item",   PyItem);
        self.n("py::to_jrg", PyToJrg);
        self.n("py::repr",   PyRepr);
        self.n("py::refs",   PyRefs);

        // dok:: — library "full power" (lihat src/dok/mod.rs untuk overview).
        self.n("dok::tcp_connect",     DokTcpConnect);
        self.n("dok::tcp_send",        DokTcpSend);
        self.n("dok::tcp_recv",        DokTcpRecv);
        self.n("dok::tcp_recv_line",   DokTcpRecvLine);
        self.n("dok::tcp_recv_all",    DokTcpRecvAll);
        self.n("dok::tcp_close",       DokTcpClose);
        self.n("dok::tcp_set_timeout", DokTcpSetTimeout);
        self.n("dok::tcp_local_addr",  DokTcpLocalAddr);
        self.n("dok::tcp_peer_addr",   DokTcpPeerAddr);
        self.n("dok::tcp_listen",      DokTcpListen);
        self.n("dok::tcp_accept",      DokTcpAccept);
        self.n("dok::tcp_close_listener", DokTcpCloseListener);
        self.n("dok::udp_bind",        DokUdpBind);
        self.n("dok::udp_send_to",     DokUdpSendTo);
        self.n("dok::udp_recv_from",   DokUdpRecvFrom);
        self.n("dok::udp_close",       DokUdpClose);
        self.n("dok::resolve",         DokResolve);

        self.n("dok::http_get",     DokHttpGet);
        self.n("dok::http_post",    DokHttpPost);
        self.n("dok::http_put",     DokHttpPut);
        self.n("dok::http_delete",  DokHttpDelete);
        self.n("dok::http_request", DokHttpRequest);
        self.n("dok::url_encode",   DokUrlEncode);
        self.n("dok::url_decode",   DokUrlDecode);
        self.n("dok::header_get",   DokHeaderGet);
        self.n("dok::query_get",    DokQueryGet);

        self.n("dok::router_new",       DokRouterNew);
        self.n("dok::router_get",       DokRouterGet);
        self.n("dok::router_post",      DokRouterPost);
        self.n("dok::router_put",       DokRouterPut);
        self.n("dok::router_delete",    DokRouterDelete);
        self.n("dok::router_patch",     DokRouterPatch);
        self.n("dok::router_any",       DokRouterAny);
        self.n("dok::router_static",    DokRouterStatic);
        self.n("dok::router_not_found", DokRouterNotFound);
        self.n("dok::serve",            DokServe);
        self.n("dok::db_open",            DokDbOpen);
        self.n("dok::db_close",           DokDbClose);
        self.n("dok::db_exec",            DokDbExec);
        self.n("dok::db_query",           DokDbQuery);
        self.n("dok::db_last_id",         DokDbLastId);
        self.n("dok::db_tersedia",        DokDbTersedia);
        self.n("dok::router_ws",          DokRouterWs);
        self.n("dok::ws_send",            DokWsSend);
        self.n("dok::ws_recv",            DokWsRecv);
        self.n("dok::ws_close",           DokWsClose);
        self.n("dok::ws_terbuka",         DokWsTerbuka);
        self.n("dok::router_use",         DokRouterUse);
        self.n("dok::router_after",       DokRouterAfter);
        self.n("dok::cookie_get",         DokCookieGet);
        self.n("dok::cookie_set",         DokCookieSet);
        self.n("dok::header_set",         DokHeaderSet);
        self.n("dok::session_encode",     DokSessionEncode);
        self.n("dok::session_decode",     DokSessionDecode);
        self.n("dok::password_hash",      DokPasswordHash);
        self.n("dok::password_verify",    DokPasswordVerify);
        self.n("dok::render",             DokRender);
        self.n("dok::render_file",        DokRenderFile);
        self.n("dok::escape_html",        DokEscapeHtml);
        self.n("dok::response",      DokResponse);
        self.n("dok::text",          DokText);
        self.n("dok::html",          DokHtml);
        self.n("dok::json_response", DokJsonResponse);
        self.n("dok::redirect",      DokRedirect);

        self.n("dok::json_encode", DokJsonEncode);
        self.n("dok::json_decode", DokJsonDecode);
        self.n("dok::json_get",    DokJsonGet);
        self.n("dok::json_set",    DokJsonSet);
        self.n("dok::json_keys",   DokJsonKeys);

        self.n("dok::vec_add",      DokVecAdd);
        self.n("dok::vec_sub",      DokVecSub);
        self.n("dok::vec_scale",    DokVecScale);
        self.n("dok::vec_dot",      DokVecDot);
        self.n("dok::vec_cross",    DokVecCross);
        self.n("dok::vec_norm",     DokVecNorm);
        self.n("dok::vec_normalize", DokVecNormalize);
        self.n("dok::vec_lerp",     DokVecLerp);
        self.n("dok::vec_distance", DokVecDistance);
        self.n("dok::vec_angle",    DokVecAngle);
        self.n("dok::mat_new",       DokMatNew);
        self.n("dok::mat_identity",  DokMatIdentity);
        self.n("dok::mat_mul",       DokMatMul);
        self.n("dok::mat_add",       DokMatAdd);
        self.n("dok::mat_sub",       DokMatSub);
        self.n("dok::mat_transpose", DokMatTranspose);
        self.n("dok::mat_det",       DokMatDet);
        self.n("dok::mat_inverse",   DokMatInverse);
        self.n("dok::mat_get",       DokMatGet);
        self.n("dok::mat_set",       DokMatSet);
        self.n("dok::stat_correlation", DokStatCorrelation);
        self.n("dok::stat_covariance",  DokStatCovariance);
        self.n("dok::stat_percentile",  DokStatPercentile);
        self.n("dok::stat_mode",        DokStatMode);
        self.n("dok::stat_linreg",      DokStatLinreg);
        self.n("dok::stat_zscore",      DokStatZscore);
        self.n("dok::stat_normal_pdf",  DokStatNormalPdf);
        self.n("dok::stat_normal_cdf",  DokStatNormalCdf);
        self.n("dok::num_is_prime",     DokNumIsPrime);
        self.n("dok::num_next_prime",   DokNumNextPrime);
        self.n("dok::num_prime_factors", DokNumPrimeFactors);
        self.n("dok::num_mod_pow",      DokNumModPow);
        self.n("dok::num_to_base",      DokNumToBase);
        self.n("dok::num_from_base",    DokNumFromBase);
        self.n("dok::num_solve",        DokNumSolve);

        self.n("dok::log_trace", DokLogTrace);
        self.n("dok::log_debug", DokLogDebug);
        self.n("dok::log_info",  DokLogInfo);
        self.n("dok::log_warn",  DokLogWarn);
        self.n("dok::log_error", DokLogError);
        self.n("dok::log_fatal", DokLogFatal);
        self.n("dok::log_with",      DokLogWith);
        self.n("dok::log_set_level", DokLogSetLevel);
        self.n("dok::log_to_file",   DokLogToFile);
        self.n("dok::log_set_name",  DokLogSetName);
        self.n("dok::log_set_color", DokLogSetColor);

        // ptr:: — referensi/pointer sungguhan.
        self.n("ptr::new",     PtrNew);
        self.n("ptr::null",    PtrNull);
        self.n("ptr::is_null", PtrIsNull);
        self.n("ptr::addr",    PtrAddr);
        self.n("ptr::get",     PtrGet);
        self.n("ptr::deref",   PtrGet);
        self.n("ptr::set",     PtrSet);
        self.n("ptr::free",    PtrFree);
        self.n("ptr::update",  PtrUpdate);
        self.n("mutex::with_lock", MutexWithLock);
        self.n("ptr::inc",     PtrInc);
        self.n("ptr::dec",     PtrDec);

        // crypto:: — hashing, HMAC, CRC32, base64/hex, random, AES-256-CBC.
        self.n("crypto::md5",            CryptoMd5);
        self.n("crypto::sha1",           CryptoSha1);
        self.n("crypto::sha256",         CryptoSha256);
        self.n("crypto::hash",           CryptoSha256); // alias: default hash paling umum dipake
        self.n("dok::get",  DokHttpGet);   // alias pendek buat http_get/post/put/delete
        self.n("dok::post", DokHttpPost);
        self.n("dok::put",  DokHttpPut);
        self.n("dok::delete", DokHttpDelete);
        self.n("crypto::sha512",         CryptoSha512);
        self.n("crypto::hmac_sha256",    CryptoHmacSha256);
        self.n("crypto::crc32",          CryptoCrc32);
        self.n("crypto::base64_encode",  CryptoBase64Encode);
        self.n("crypto::base64_decode",  CryptoBase64Decode);
        self.n("crypto::hex_encode",     CryptoHexEncode);
        self.n("crypto::hex_decode",     CryptoHexDecode);
        self.n("crypto::random_hex",     CryptoRandomHex);
        self.n("crypto::random_int",     CryptoRandomInt);
        self.n("crypto::aes_gen_key",    CryptoAesGenKey);
        self.n("crypto::aes_gen_iv",     CryptoAesGenIv);
        self.n("crypto::aes_encrypt",    CryptoAesEncrypt);
        self.n("crypto::aes_decrypt",    CryptoAesDecrypt);

        // jawa:: / jawir:: — manajemen memori manual.
        self.n("jawa::alloc", NewAlloc);
        self.n("jawa::get",   NewGet);
        self.n("jawa::set",   NewSet);
        self.n("jawa::array",         NewArray);
        self.n("jawa::array_get",     NewArrayGet);
        self.n("jawa::array_set",     NewArraySet);
        self.n("jawa::array_push",    NewArrayPush);
        self.n("jawa::array_pop",     NewArrayPop);
        self.n("jawa::array_len",     NewArrayLen);
        self.n("jawa::array_to_list", NewArrayToList);
        self.n("jawa::buffer",          NewBuffer);
        self.n("jawa::buffer_from_hex", NewBufferFromHex);
        self.n("jawa::buffer_get",      NewBufferGet);
        self.n("jawa::buffer_set",      NewBufferSet);
        self.n("jawa::buffer_len",      NewBufferLen);
        self.n("jawa::buffer_fill",     NewBufferFill);
        self.n("jawa::buffer_to_hex",   NewBufferToHex);
        self.n("jawa::is_valid", NewIsValid);
        self.n("jawa::sizeof",   NewSizeof);
        self.n("jawa::stats",    NewStats);
        self.n("jawir::free",  DeleteFree);
        // Bentuk telanjang untuk dua operasi yang paling sering dipakai:
        // `jawa(42)` sama dengan `jawa::alloc(42)`, `jawir(p)` sama dengan
        // `jawir::free(p)`. Sisanya tetap lewat `jawa::`.
        self.n("jawa",  NewAlloc);
        self.n("jawir", DeleteFree);

        // ── arr:: (tambahan) ────────────────────────────────────────────
        self.n("arr::first",     ArrFirst);     self.n("arr::last",   ArrLast);
        self.n("arr::is_empty",  ArrIsEmpty);   self.n("arr::concat", ArrConcat);
        // Turunan parser untuk `[...x]` dan `f(...x)`; nama internal, jadi tidak diiklankan.
        self.n("arr::__sebar", ArrSebar);
        self.n("arr::flat_map",  ArrFlatMap);   self.n("arr::group_by", ArrGroupBy);
        self.n("arr::partition", ArrPartition); self.n("arr::zip_with", ArrZipWith);
        self.n("arr::shuffle",   ArrShuffle);   self.n("arr::sample", ArrSample);
        self.n("arr::swap",      ArrSwap);      self.n("arr::rotate", ArrRotate);

        // ── str:: (tambahan) ────────────────────────────────────────────
        self.n("str::capitalize",    StrCapitalize);
        self.n("str::is_empty",      StrIsEmpty);
        self.n("str::strip_prefix",  StrStripPrefix);
        self.n("str::strip_suffix",  StrStripSuffix);
        self.n("str::substr",        StrSubstr);
        self.n("str::is_upper",      StrIsUpper);
        self.n("str::is_lower",      StrIsLower);
        self.n("str::swap_case",     StrSwapCase);
        // Alias — nama yang lebih familiar buat yang datang dari Python/JS,
        // tanpa duplikasi logika: langsung ke NativeFnKind yang sudah ada.
        self.n("str::ljust",     StrPadRight); // ljust Python = pad di kanan
        self.n("str::rjust",     StrPadLeft);  // rjust Python = pad di kiri
        self.n("str::index_of",  StrFind);
        self.n("str::to_int",    ParseInt);
        self.n("str::to_float",  ParseFloat);

        // ── math:: (tambahan) ───────────────────────────────────────────
        self.n("math::TAU",       MathTau);
        self.n("math::INF",       MathInf);
        self.n("math::NAN",       MathNan);
        self.n("math::hypot",     MathHypot);
        self.n("math::log",       MathLog);
        self.n("math::degrees",   MathDegrees);
        self.n("math::radians",   MathRadians);
        self.n("math::trunc",     MathTrunc);
        self.n("math::wrap_add",  MathWrapAdd);
        self.n("math::wrap_sub",  MathWrapSub);
        self.n("math::wrap_mul",  MathWrapMul);
        self.n("math::copysign",  MathCopysign);

        // ── dict:: — peta key/value ala Python `dict` ───────────────────
        self.n("dict::new",         DictNew);
        self.n("dict::from_pairs",  DictFromPairs);
        self.n("dict::set",         DictSet);
        self.n("dict::get",         DictGet);
        self.n("dict::get_or",      DictGetOr);
        self.n("dict::has",         DictHas);
        self.n("dict::remove",      DictRemove);
        self.n("dict::keys",        DictKeys);
        self.n("dict::values",      DictValues);
        self.n("dict::items",       DictItems);
        self.n("dict::len",         DictLen);
        self.n("dict::is_empty",    DictIsEmpty);
        self.n("dict::merge",       DictMerge);
        self.n("dict::is_dict",     DictIsDict);

        // ── rand:: — angka acak ramah-pemula ─────────────────────────────
        self.n("rand::int",     RandInt);
        self.n("rand::float",   RandFloat);
        self.n("rand::bool",    RandBool);
        self.n("rand::choice",  RandChoice);
        self.n("rand::shuffle", RandShuffle);
        self.n("rand::sample",  RandSample);

        // ── time:: (tambahan) ────────────────────────────────────────────
        self.n("time::now",     TimeNow);
        self.n("time::sleep",   TimeSleep);
        self.n("time::year",    TimeYear);
        self.n("time::month",   TimeMonth);
        self.n("time::day",     TimeDay);
        self.n("time::hour",    TimeHour);
        self.n("time::minute",  TimeMinute);
        self.n("time::second",  TimeSecond);
        self.n("time::weekday", TimeWeekday);
        self.n("time::format",  TimeFormat);

        // ── file:: — baca/tulis file ─────────────────────────────────────
        self.n("file::read",        FileRead);
        self.n("file::write",       FileWrite);
        self.n("file::append",      FileAppend);
        self.n("file::exists",      FileExists);
        self.n("file::delete",      FileDelete);
        self.n("file::read_lines",  FileReadLines);
        self.n("file::list_dir",    FileListDir);

        // ── env:: — environment variable & argumen CLI ───────────────────
        self.n("env::get",     EnvGet);
        self.n("env::get_or",  EnvGetOr);
        self.n("env::set",     EnvSet);
        self.n("env::args",    EnvArgs);
        self.n("env::exe",     EnvExe);

        // Result<T, E> / Option<T> — built-in tagged-union types so a fallible
        // operation can hand back an explicit, checkable value instead of a silent …
        self.v("Some", Value::EnumCtor { type_name: "Option".to_string(), variant: "Some".to_string(), arity: 1 });
        self.v("Option::Some", Value::EnumCtor { type_name: "Option".to_string(), variant: "Some".to_string(), arity: 1 });
        self.v("None", none_value());
        self.v("Option::None", none_value());
        self.v("Ok", Value::EnumCtor { type_name: "Result".to_string(), variant: "Ok".to_string(), arity: 1 });
        self.v("Result::Ok", Value::EnumCtor { type_name: "Result".to_string(), variant: "Ok".to_string(), arity: 1 });
        self.v("Err", Value::EnumCtor { type_name: "Result".to_string(), variant: "Err".to_string(), arity: 1 });
        self.v("Result::Err", Value::EnumCtor { type_name: "Result".to_string(), variant: "Err".to_string(), arity: 1 });
        // Bare type-name placeholders, exactly like a plain `struct`'s name is
        // registered (see `hoist_top_level`'s `Item::Struct` case) — so referencing …
        self.v("Result", Value::Nil);
        self.v("Option", Value::Nil);

        // ── Alias gaya Indonesia — sinonim, bukan pengganti ───────────── Nama asli
        // (wok/wok_/tok/eprint/sprint) tetap ada persis seperti sebelumnya; ini cuma …
        self.n("cetak",       FmtWok);
        self.n("cetak_",      FmtWokNoNl);
        self.n("baca",        FmtTok);
        self.n("cetak_galat", FmtEprint);
        self.n("teks",        FmtSprint);
    }
}


mod dispatch;
