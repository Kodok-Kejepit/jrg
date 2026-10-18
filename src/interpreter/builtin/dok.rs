// src/interpreter/builtin/dok.rs — interpreter/builtin/dok.rs
// Dispatch untuk semua yang delegasi ke `src/dok/*` — net (tcp/udp),

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use super::NativeFnKind;

impl Interpreter {
    pub(super) fn dispatch_dok(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        let result: EvalResult = match kind {
            DokTcpConnect     => crate::dok::net::tcp_connect(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpSend        => crate::dok::net::tcp_send(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpRecv        => crate::dok::net::tcp_recv(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpRecvLine    => crate::dok::net::tcp_recv_line(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpRecvAll     => crate::dok::net::tcp_recv_all(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpClose       => crate::dok::net::tcp_close(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpSetTimeout  => crate::dok::net::tcp_set_timeout(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpLocalAddr   => crate::dok::net::tcp_local_addr(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpPeerAddr    => crate::dok::net::tcp_peer_addr(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpListen      => crate::dok::net::tcp_listen(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpAccept      => crate::dok::net::tcp_accept(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokTcpCloseListener => crate::dok::net::tcp_close_listener(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokUdpBind        => crate::dok::net::udp_bind(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokUdpSendTo      => crate::dok::net::udp_send_to(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokUdpRecvFrom    => crate::dok::net::udp_recv_from(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokUdpClose       => crate::dok::net::udp_close(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokResolve        => crate::dok::net::resolve(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            DokHttpGet     => crate::dok::http::http_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHttpPost    => crate::dok::http::http_post(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHttpPut     => crate::dok::http::http_put(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHttpDelete  => crate::dok::http::http_delete(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHttpRequest => crate::dok::http::http_request(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokUrlEncode   => crate::dok::http::url_encode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokUrlDecode   => crate::dok::http::url_decode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHeaderGet   => crate::dok::http::header_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokQueryGet    => crate::dok::http::query_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            DokRouterNew      => crate::dok::http::router_new(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterGet      => crate::dok::http::router_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterPost     => crate::dok::http::router_post(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterPut      => crate::dok::http::router_put(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterDelete   => crate::dok::http::router_delete(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterPatch    => crate::dok::http::router_patch(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterAny      => crate::dok::http::router_any(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterStatic   => crate::dok::http::router_static(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterNotFound => crate::dok::http::router_not_found(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokServe          => crate::dok::http::serve(self, args),
            DokRouterUse      => crate::dok::http::router_use(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterWs     => crate::dok::http::router_ws(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokDbOpen       => crate::dok::db::db_open(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokDbClose      => crate::dok::db::db_close(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokDbExec       => crate::dok::db::db_exec(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokDbQuery      => crate::dok::db::db_query(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokDbLastId     => crate::dok::db::db_last_id(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokDbTersedia   => crate::dok::db::db_tersedia(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokWsSend       => crate::dok::ws::ws_send(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokWsRecv       => crate::dok::ws::ws_recv(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokWsClose      => crate::dok::ws::ws_close(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokWsTerbuka    => crate::dok::ws::ws_terbuka(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRouterAfter    => crate::dok::http::router_after(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokCookieGet      => crate::dok::web::cookie_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokCookieSet      => crate::dok::web::cookie_set(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHeaderSet      => crate::dok::web::header_set(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokSessionEncode  => crate::dok::web::session_encode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokSessionDecode  => crate::dok::web::session_decode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokPasswordHash   => crate::dok::web::password_hash(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokPasswordVerify => crate::dok::web::password_verify(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRender         => crate::dok::web::render(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRenderFile     => crate::dok::web::render_file(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokEscapeHtml     => crate::dok::web::escape_html(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokResponse     => crate::dok::http::response(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokText         => crate::dok::http::text(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokHtml         => crate::dok::http::html(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokJsonResponse => crate::dok::http::json_response(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokRedirect     => crate::dok::http::redirect(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            DokJsonEncode => crate::dok::json::json_encode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokJsonDecode => crate::dok::json::json_decode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokJsonGet    => crate::dok::json::json_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokJsonSet    => crate::dok::json::json_set(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokJsonKeys   => crate::dok::json::json_keys(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            DokVecAdd       => crate::dok::numeric::vec_add(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecSub       => crate::dok::numeric::vec_sub(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecScale     => crate::dok::numeric::vec_scale(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecDot       => crate::dok::numeric::vec_dot(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecCross     => crate::dok::numeric::vec_cross(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecNorm      => crate::dok::numeric::vec_norm(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecNormalize => crate::dok::numeric::vec_normalize(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecLerp      => crate::dok::numeric::vec_lerp(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecDistance  => crate::dok::numeric::vec_distance(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokVecAngle     => crate::dok::numeric::vec_angle(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatNew       => crate::dok::numeric::mat_new(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatIdentity  => crate::dok::numeric::mat_identity(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatMul       => crate::dok::numeric::mat_mul(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatAdd       => crate::dok::numeric::mat_add(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatSub       => crate::dok::numeric::mat_sub(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatTranspose => crate::dok::numeric::mat_transpose(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatDet       => crate::dok::numeric::mat_det(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatInverse   => crate::dok::numeric::mat_inverse(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatGet       => crate::dok::numeric::mat_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokMatSet       => crate::dok::numeric::mat_set(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatCorrelation => crate::dok::numeric::stat_correlation(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatCovariance  => crate::dok::numeric::stat_covariance(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatPercentile  => crate::dok::numeric::stat_percentile(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatMode        => crate::dok::numeric::stat_mode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatLinreg      => crate::dok::numeric::stat_linreg(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatZscore      => crate::dok::numeric::stat_zscore(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatNormalPdf   => crate::dok::numeric::stat_normal_pdf(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokStatNormalCdf   => crate::dok::numeric::stat_normal_cdf(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumIsPrime      => crate::dok::numeric::num_is_prime(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumNextPrime    => crate::dok::numeric::num_next_prime(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumPrimeFactors => crate::dok::numeric::num_prime_factors(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumModPow       => crate::dok::numeric::num_mod_pow(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumToBase       => crate::dok::numeric::num_to_base(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumFromBase     => crate::dok::numeric::num_from_base(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokNumSolve        => crate::dok::numeric::num_solve(self, args),

            DokLogTrace    => crate::dok::log::log_trace(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogDebug    => crate::dok::log::log_debug(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogInfo     => crate::dok::log::log_info(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogWarn     => crate::dok::log::log_warn(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogError    => crate::dok::log::log_error(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogFatal    => crate::dok::log::log_fatal(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogWith     => crate::dok::log::log_with(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogSetLevel => crate::dok::log::log_set_level(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogToFile   => crate::dok::log::log_to_file(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogSetName  => crate::dok::log::log_set_name(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            DokLogSetColor => crate::dok::log::log_set_color(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            // ── ptr:: ───────────────────────────────────────────────────
            PtrNew    => crate::dok::ptr::ptr_new(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrNull   => crate::dok::ptr::ptr_null(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrIsNull => crate::dok::ptr::ptr_is_null(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrAddr   => crate::dok::ptr::ptr_addr(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrGet    => crate::dok::ptr::ptr_get(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrSet    => crate::dok::ptr::ptr_set(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrFree   => crate::dok::ptr::ptr_free(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrUpdate => crate::dok::ptr::ptr_update(self, args),
            MutexWithLock => self.mutex_with_lock(args),
            PtrInc    => crate::dok::ptr::ptr_inc(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            PtrDec    => crate::dok::ptr::ptr_dec(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            // ── crypto:: ────────────────────────────────────────────────
            CryptoMd5           => crate::dok::crypto::f_md5(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoSha1          => crate::dok::crypto::f_sha1(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoSha256        => crate::dok::crypto::f_sha256(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoSha512        => crate::dok::crypto::f_sha512(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoHmacSha256    => crate::dok::crypto::f_hmac_sha256(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoCrc32         => crate::dok::crypto::f_crc32(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoBase64Encode  => crate::dok::crypto::f_base64_encode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoBase64Decode  => crate::dok::crypto::f_base64_decode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoHexEncode     => crate::dok::crypto::f_hex_encode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoHexDecode     => crate::dok::crypto::f_hex_decode(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoRandomHex     => crate::dok::crypto::f_random_hex(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoRandomInt     => crate::dok::crypto::f_random_int(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoAesGenKey     => crate::dok::crypto::f_aes_gen_key(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoAesGenIv      => crate::dok::crypto::f_aes_gen_iv(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoAesEncrypt    => crate::dok::crypto::f_aes_encrypt(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            CryptoAesDecrypt    => crate::dok::crypto::f_aes_decrypt(args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            _ => return None,
        };
        // Sebuah pesan yang isinya CUMA tanda tangan bukan kalimat.
        Some(result.map_err(|sinyal| match sinyal {
            Signal::RuntimeError(e) => {
                let tipe: Vec<String> = args.iter().map(|v| v.type_name()).collect();
                Signal::RuntimeError(RuntimeError {
                    message: crate::vm::value::perjelas_pesan_native_tipe(e.message, &tipe),
                    ..e
                })
            }
            lain => lain,
        }))
    }
}

impl crate::interpreter::Interpreter {
    /// `mutex::with_lock(m, f)`: kunci, jalankan `f()`, lepas — juga kalau `f` gagal.
    pub(crate) fn mutex_with_lock(&mut self, args: &[Value]) -> EvalResult {
        use crate::interpreter::{RuntimeError, Signal};
        let galat = |e: String| Signal::RuntimeError(RuntimeError::new(e));
        let k = match args.first() {
            Some(Value::Sinkron(crate::sinkron::Pegangan::Kunci(k))) => k.clone(),
            v => return Err(galat(crate::sinkron::pesan_bukan("mutex::with_lock", "Mutex",
                &v.map_or_else(|| "nil".to_string(), |v| v.type_name())))),
        };
        let _terkunci = crate::sinkron::Terkunci::baru(&k, "mutex::with_lock").map_err(galat)?;
        self.call_fn(args.get(1).cloned().unwrap_or(Value::Nil), vec![])
    }
}
