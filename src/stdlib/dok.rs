// src/stdlib/dok.rs — src/stdlib/dok.rs — `dok::` di backend VM.

use crate::vm::vm::Vm;

use super::sistem::lewat_hasil;

pub fn register(vm: &mut Vm) {
    // `dok::num_solve` didaftarkan supaya NAMANYA ada, tapi badan ini tidak
    // pernah berjalan: ia dicegat lebih dulu di situs pemanggilan native (lihat …
    vm.def("dok::num_solve", |_| {
        Err("dok::num_solve dipanggil lewat jalur yang tidak dicegat — ini bug \
             internal, bukan kesalahan program".to_string())
    });


    // ── klien HTTP, router, dan pembantu respons ──
    vm.def("dok::header_get",          |a| lewat_hasil(a, crate::dok::http::header_get));
    vm.def("dok::html",                |a| lewat_hasil(a, crate::dok::http::html));
    vm.def("dok::http_delete",         |a| lewat_hasil(a, crate::dok::http::http_delete));
    vm.def("dok::http_get",            |a| lewat_hasil(a, crate::dok::http::http_get));
    vm.def("dok::http_post",           |a| lewat_hasil(a, crate::dok::http::http_post));
    vm.def("dok::http_put",            |a| lewat_hasil(a, crate::dok::http::http_put));
    vm.def("dok::http_request",        |a| lewat_hasil(a, crate::dok::http::http_request));
    vm.def("dok::json_response",       |a| lewat_hasil(a, crate::dok::http::json_response));
    vm.def("dok::query_get",           |a| lewat_hasil(a, crate::dok::http::query_get));
    vm.def("dok::redirect",            |a| lewat_hasil(a, crate::dok::http::redirect));
    vm.def("dok::response",            |a| lewat_hasil(a, crate::dok::http::response));
    vm.def("dok::router_any",          |a| lewat_hasil(a, crate::dok::http::router_any));
    vm.def("dok::router_delete",       |a| lewat_hasil(a, crate::dok::http::router_delete));
    // Tujuh fungsi di bawah menerima FUNGSI, dan sebuah fungsi tidak bisa
    // menyeberang.
    vm.def("dok::serve", |_| {
        Err("dok::serve dipanggil lewat jalur yang tidak dicegat — ini bug di \
             jrgc, bukan di programmu"
            .to_string())
    });
    vm.def("dok::router_get",          |a| lewat_hasil(a, crate::dok::http::router_get));
    vm.def("dok::cookie_get",           |a| lewat_hasil(a, crate::dok::web::cookie_get));
    vm.def("dok::cookie_set",           |a| lewat_hasil(a, crate::dok::web::cookie_set));
    vm.def("dok::header_set",           |a| lewat_hasil(a, crate::dok::web::header_set));
    vm.def("dok::session_encode",       |a| lewat_hasil(a, crate::dok::web::session_encode));
    vm.def("dok::session_decode",       |a| lewat_hasil(a, crate::dok::web::session_decode));
    vm.def("dok::password_hash",        |a| lewat_hasil(a, crate::dok::web::password_hash));
    vm.def("dok::password_verify",      |a| lewat_hasil(a, crate::dok::web::password_verify));
    vm.def("dok::render",               |a| lewat_hasil(a, crate::dok::web::render));
    vm.def("dok::render_file",          |a| lewat_hasil(a, crate::dok::web::render_file));
    vm.def("dok::escape_html",          |a| lewat_hasil(a, crate::dok::web::escape_html));
    // `router_use`/`router_after` menerima FUNGSI, jadi keduanya dicegat di
    // situs pemanggilan seperti `router_get` — badan di bawah tidak pernah …
    vm.def("dok::ws_send",          |a| lewat_hasil(a, crate::dok::ws::ws_send));
    vm.def("dok::ws_recv",          |a| lewat_hasil(a, crate::dok::ws::ws_recv));
    vm.def("dok::ws_close",         |a| lewat_hasil(a, crate::dok::ws::ws_close));
    vm.def("dok::ws_terbuka",       |a| lewat_hasil(a, crate::dok::ws::ws_terbuka));
    vm.def("dok::db_open",          |a| lewat_hasil(a, crate::dok::db::db_open));
    vm.def("dok::db_close",         |a| lewat_hasil(a, crate::dok::db::db_close));
    vm.def("dok::db_exec",          |a| lewat_hasil(a, crate::dok::db::db_exec));
    vm.def("dok::db_query",         |a| lewat_hasil(a, crate::dok::db::db_query));
    vm.def("dok::db_last_id",       |a| lewat_hasil(a, crate::dok::db::db_last_id));
    vm.def("dok::db_tersedia",      |a| lewat_hasil(a, crate::dok::db::db_tersedia));
    vm.def("dok::router_ws", |_| Err(
        "dok::router_ws dipanggil lewat jalur yang tidak dicegat — ini bug di jrgc, \
         bukan di programmu".to_string()));
    vm.def("dok::router_use", |_| Err(
        "dok::router_use dipanggil lewat jalur yang tidak dicegat — ini bug di jrgc, \
         bukan di programmu".to_string()));
    vm.def("dok::router_after", |_| Err(
        "dok::router_after dipanggil lewat jalur yang tidak dicegat — ini bug di jrgc, \
         bukan di programmu".to_string()));
    vm.def("dok::router_new",          |a| lewat_hasil(a, crate::dok::http::router_new));
    vm.def("dok::router_not_found",    |a| lewat_hasil(a, crate::dok::http::router_not_found));
    vm.def("dok::router_patch",        |a| lewat_hasil(a, crate::dok::http::router_patch));
    vm.def("dok::router_post",         |a| lewat_hasil(a, crate::dok::http::router_post));
    vm.def("dok::router_put",          |a| lewat_hasil(a, crate::dok::http::router_put));
    vm.def("dok::router_static",       |a| lewat_hasil(a, crate::dok::http::router_static));
    vm.def("dok::text",                |a| lewat_hasil(a, crate::dok::http::text));
    vm.def("dok::binary",              |a| lewat_hasil(a, crate::dok::http::binary));
    vm.def("dok::udp_local_addr",      |a| lewat_hasil(a, crate::dok::net::udp_local_addr));
    vm.def("dok::url_decode",          |a| lewat_hasil(a, crate::dok::http::url_decode));
    vm.def("dok::url_encode",          |a| lewat_hasil(a, crate::dok::http::url_encode));

    // ── JSON ──
    vm.def("dok::json_decode",         |a| lewat_hasil(a, crate::dok::json::json_decode));
    vm.def("dok::json_encode",         |a| lewat_hasil(a, crate::dok::json::json_encode));
    vm.def("dok::json_get",            |a| lewat_hasil(a, crate::dok::json::json_get));
    vm.def("dok::json_keys",           |a| lewat_hasil(a, crate::dok::json::json_keys));
    vm.def("dok::json_set",            |a| lewat_hasil(a, crate::dok::json::json_set));

    // ── log berjenjang ──
    vm.def("dok::log_debug",           |a| lewat_hasil(a, crate::dok::log::log_debug));
    vm.def("dok::log_error",           |a| lewat_hasil(a, crate::dok::log::log_error));
    vm.def("dok::log_fatal",           |a| lewat_hasil(a, crate::dok::log::log_fatal));
    vm.def("dok::log_info",            |a| lewat_hasil(a, crate::dok::log::log_info));
    vm.def("dok::log_set_color",       |a| lewat_hasil(a, crate::dok::log::log_set_color));
    vm.def("dok::log_set_level",       |a| lewat_hasil(a, crate::dok::log::log_set_level));
    vm.def("dok::log_set_name",        |a| lewat_hasil(a, crate::dok::log::log_set_name));
    vm.def("dok::log_to_file",         |a| lewat_hasil(a, crate::dok::log::log_to_file));
    vm.def("dok::log_trace",           |a| lewat_hasil(a, crate::dok::log::log_trace));
    vm.def("dok::log_warn",            |a| lewat_hasil(a, crate::dok::log::log_warn));
    vm.def("dok::log_with",            |a| lewat_hasil(a, crate::dok::log::log_with));

    // ── TCP/UDP dan DNS ──
    vm.def("dok::resolve",             |a| lewat_hasil(a, crate::dok::net::resolve));
    vm.def("dok::tcp_accept",          |a| lewat_hasil(a, crate::dok::net::tcp_accept));
    vm.def("dok::tcp_close",           |a| lewat_hasil(a, crate::dok::net::tcp_close));
    vm.def("dok::tcp_close_listener",  |a| lewat_hasil(a, crate::dok::net::tcp_close_listener));
    vm.def("dok::tcp_connect",         |a| lewat_hasil(a, crate::dok::net::tcp_connect));
    vm.def("dok::tcp_listen",          |a| lewat_hasil(a, crate::dok::net::tcp_listen));
    vm.def("dok::tcp_local_addr",      |a| lewat_hasil(a, crate::dok::net::tcp_local_addr));
    vm.def("dok::tcp_peer_addr",       |a| lewat_hasil(a, crate::dok::net::tcp_peer_addr));
    vm.def("dok::tcp_recv",            |a| lewat_hasil(a, crate::dok::net::tcp_recv));
    vm.def("dok::tcp_recv_bytes",      |a| lewat_hasil(a, crate::dok::net::tcp_recv_bytes));
    vm.def("dok::udp_recv_bytes_from", |a| lewat_hasil(a, crate::dok::net::udp_recv_bytes_from));
    vm.def("dok::tcp_recv_all",        |a| lewat_hasil(a, crate::dok::net::tcp_recv_all));
    vm.def("dok::tcp_recv_line",       |a| lewat_hasil(a, crate::dok::net::tcp_recv_line));
    vm.def("dok::tcp_send",            |a| lewat_hasil(a, crate::dok::net::tcp_send));
    vm.def("dok::tcp_set_timeout",     |a| lewat_hasil(a, crate::dok::net::tcp_set_timeout));
    vm.def("dok::udp_bind",            |a| lewat_hasil(a, crate::dok::net::udp_bind));
    vm.def("dok::udp_close",           |a| lewat_hasil(a, crate::dok::net::udp_close));
    vm.def("dok::udp_recv_from",       |a| lewat_hasil(a, crate::dok::net::udp_recv_from));
    vm.def("dok::udp_send_to",         |a| lewat_hasil(a, crate::dok::net::udp_send_to));

    // ── matriks, vektor, statistik, dan teori bilangan ──
    vm.def("dok::mat_add",             |a| lewat_hasil(a, crate::dok::numeric::mat_add));
    vm.def("dok::mat_det",             |a| lewat_hasil(a, crate::dok::numeric::mat_det));
    vm.def("dok::mat_get",             |a| lewat_hasil(a, crate::dok::numeric::mat_get));
    vm.def("dok::mat_identity",        |a| lewat_hasil(a, crate::dok::numeric::mat_identity));
    vm.def("dok::mat_inverse",         |a| lewat_hasil(a, crate::dok::numeric::mat_inverse));
    vm.def("dok::mat_mul",             |a| lewat_hasil(a, crate::dok::numeric::mat_mul));
    vm.def("dok::mat_new",             |a| lewat_hasil(a, crate::dok::numeric::mat_new));
    vm.def("dok::mat_set",             |a| lewat_hasil(a, crate::dok::numeric::mat_set));
    vm.def("dok::mat_sub",             |a| lewat_hasil(a, crate::dok::numeric::mat_sub));
    vm.def("dok::mat_transpose",       |a| lewat_hasil(a, crate::dok::numeric::mat_transpose));
    vm.def("dok::num_from_base",       |a| lewat_hasil(a, crate::dok::numeric::num_from_base));
    vm.def("dok::num_is_prime",        |a| lewat_hasil(a, crate::dok::numeric::num_is_prime));
    vm.def("dok::num_mod_pow",         |a| lewat_hasil(a, crate::dok::numeric::num_mod_pow));
    vm.def("dok::num_next_prime",      |a| lewat_hasil(a, crate::dok::numeric::num_next_prime));
    vm.def("dok::num_prime_factors",   |a| lewat_hasil(a, crate::dok::numeric::num_prime_factors));
    vm.def("dok::num_to_base",         |a| lewat_hasil(a, crate::dok::numeric::num_to_base));
    vm.def("dok::stat_correlation",    |a| lewat_hasil(a, crate::dok::numeric::stat_correlation));
    vm.def("dok::stat_covariance",     |a| lewat_hasil(a, crate::dok::numeric::stat_covariance));
    vm.def("dok::stat_linreg",         |a| lewat_hasil(a, crate::dok::numeric::stat_linreg));
    vm.def("dok::stat_mode",           |a| lewat_hasil(a, crate::dok::numeric::stat_mode));
    vm.def("dok::stat_normal_cdf",     |a| lewat_hasil(a, crate::dok::numeric::stat_normal_cdf));
    vm.def("dok::stat_normal_pdf",     |a| lewat_hasil(a, crate::dok::numeric::stat_normal_pdf));
    vm.def("dok::stat_percentile",     |a| lewat_hasil(a, crate::dok::numeric::stat_percentile));
    vm.def("dok::stat_zscore",         |a| lewat_hasil(a, crate::dok::numeric::stat_zscore));
    vm.def("dok::vec_add",             |a| lewat_hasil(a, crate::dok::numeric::vec_add));
    vm.def("dok::vec_angle",           |a| lewat_hasil(a, crate::dok::numeric::vec_angle));
    vm.def("dok::vec_cross",           |a| lewat_hasil(a, crate::dok::numeric::vec_cross));
    vm.def("dok::vec_distance",        |a| lewat_hasil(a, crate::dok::numeric::vec_distance));
    vm.def("dok::vec_dot",             |a| lewat_hasil(a, crate::dok::numeric::vec_dot));
    vm.def("dok::vec_lerp",            |a| lewat_hasil(a, crate::dok::numeric::vec_lerp));
    vm.def("dok::vec_norm",            |a| lewat_hasil(a, crate::dok::numeric::vec_norm));
    vm.def("dok::vec_normalize",       |a| lewat_hasil(a, crate::dok::numeric::vec_normalize));
    vm.def("dok::vec_scale",           |a| lewat_hasil(a, crate::dok::numeric::vec_scale));
    vm.def("dok::vec_sub",             |a| lewat_hasil(a, crate::dok::numeric::vec_sub));
}

// ─── Tabel handler milik VM ──────────────────────────────────────────────
static HANDLER_VM: std::sync::OnceLock<std::sync::Mutex<Vec<crate::vm::value::VmValue>>> =
    std::sync::OnceLock::new();

fn handler_vm() -> &'static std::sync::Mutex<Vec<crate::vm::value::VmValue>> {
    HANDLER_VM.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Simpan sebuah handler VM, kembalikan nomornya.
pub fn simpan_handler(f: crate::vm::value::VmValue) -> u64 {
    let mut g = handler_vm().lock().unwrap_or_else(|p| p.into_inner());
    g.push(f);
    (g.len() - 1) as u64
}

/// Ambil kembali handler bernomor `id`.
pub fn ambil_handler(id: u64) -> Option<crate::vm::value::VmValue> {
    handler_vm()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(id as usize)
        .cloned()
}
