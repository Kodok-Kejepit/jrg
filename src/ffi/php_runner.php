<?php
// php_runner.php — sisi PHP dari `php::`. Protokolnya sama dengan
// py_runner.py/js_runner.js: satu baris JSON masuk, satu baris keluar.

if (PHP_VERSION_ID < 80000) {
    fwrite(STDERR, "php:: butuh PHP 8.0 ke atas, yang terpasang " . PHP_VERSION . "\n");
    exit(2);
}

// stdout adalah KAWAT PROTOKOL: echo/print/var_dump milik pemakai dialihkan
// ke stderr per tulisan, supaya tetap terlihat dan tidak merusak pipa.
ob_start(static function (string $buf): string {
    if ($buf !== '') fwrite(STDERR, $buf);
    return '';
}, 1, PHP_OUTPUT_HANDLER_STDFLAGS & ~PHP_OUTPUT_HANDLER_REMOVABLE);

// Warning yang diam adalah hasil salah yang diam (`file_get_contents` → false):
// jadikan galat. Deprecation tidak mengubah hasil, jadi diabaikan.
error_reporting(E_ALL & ~E_DEPRECATED & ~E_USER_DEPRECATED);
ini_set('serialize_precision', '-1');
set_error_handler(static function (int $no, string $pesan, string $berkas = '', int $baris = 0): bool {
    if (!(error_reporting() & $no)) return false;   // `@`, atau jenis yang dimatikan
    throw new ErrorException($pesan, 0, $no, $berkas, $baris);
});

// php.ini CLI Debian memakai memory_limit=-1: rekursi tak berujung lalu tumbuh
// sampai OOM killer kernel. Diukur; batasnya bisa dinaikkan lewat ini_set.
if (ini_get('memory_limit') === '-1') ini_set('memory_limit', '1G');

if (!function_exists('array_is_list')) {
    function array_is_list(array $a): bool {
        $i = 0;
        foreach ($a as $k => $_) { if ($k !== $i++) return false; }
        return true;
    }
}

final class JrgPekerja {
    public static array $objs = [];
    public static int $nextId = 1;
    public static int $seq = 0;
    public static bool $menjawab = false;
    public static ?string $cadangan = null;
}

/** Dikodekan sebagai OBJEK JSON apa pun isinya — `{}` kosong tetap dict. */
final class JrgDict {
    public array $isi;
    public function __construct(array $isi) { $this->isi = $isi; }
}

final class JrgTakBisa extends Exception {}

// Kehabisan memori karena DATA dilaporkan lewat shutdown; cadangan ini dilepas
// dulu supaya laporannya sempat ditulis (rekursi tak berujung tetap tidak).
JrgPekerja::$cadangan = str_repeat("\0", 1 << 20);
register_shutdown_function(static function (): void {
    JrgPekerja::$cadangan = null;
    if (!JrgPekerja::$menjawab) return;
    $e = error_get_last();
    $fatal = E_ERROR | E_PARSE | E_CORE_ERROR | E_COMPILE_ERROR;
    if ($e !== null && ($e['type'] & $fatal)) {
        $pesan = 'fatal error PHP: ' . $e['message'];
        if (str_contains($e['message'], 'Allowed memory size')) {
            $pesan .= "\n  bantuan: batas memori worker PHP " . ini_get('memory_limit')
                . " — naikkan kalau memang perlu: php::exec(\"ini_set('memory_limit', '4G');\")";
        }
    } else {
        $pesan = 'kode PHP memanggil exit() — worker PHP ikut berhenti, dan objek yang '
            . 'dipegang handle hilang bersamanya; panggilan php:: berikutnya memulai worker baru';
    }
    // `berhenti` DULUAN: jrgc membacanya dari awal baris dan menunggu proses ini
    // selesai, supaya permintaan berikutnya tidak terkirim ke worker yang sekarat.
    jrg_kirim(['berhenti' => true, 'ok' => false, 'error' => $pesan]);
});

function jrg_kirim(array $pesan): void {
    fwrite(STDOUT, jrg_json($pesan) . "\n");
    fflush(STDOUT);
}

// ── JSON ────────────────────────────────────────────────────────────────────

function jrg_json(mixed $v): string {
    if ($v === null) return 'null';
    if (is_bool($v)) return $v ? 'true' : 'false';
    if (is_int($v)) return (string)$v;
    if (is_float($v)) {
        if (is_nan($v)) return 'NaN';
        if (is_infinite($v)) return $v > 0 ? 'Infinity' : '-Infinity';
        // Tanpa PRESERVE_ZERO_FRACTION `2.0` jadi `2`, lalu tiba di JRG sebagai int.
        return json_encode($v, JSON_PRESERVE_ZERO_FRACTION);
    }
    if (is_string($v)) {
        return json_encode($v, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE);
    }
    if ($v instanceof JrgDict) return jrg_json_objek($v->isi);
    if (is_array($v)) {
        if (!array_is_list($v)) return jrg_json_objek($v);
        $b = [];
        foreach ($v as $x) $b[] = jrg_json($x);
        return '[' . implode(',', $b) . ']';
    }
    return jrg_json((string)$v);
}

function jrg_json_objek(array $a): string {
    $b = [];
    foreach ($a as $k => $x) $b[] = jrg_json((string)$k) . ':' . jrg_json($x);
    return '{' . implode(',', $b) . '}';
}

/** `json_decode`, dengan jatuh ke pengurai sendiri untuk `NaN`/`Infinity`. */
function jrg_urai(string $s): mixed {
    $v = json_decode($s, true, 100000);
    if (json_last_error() === JSON_ERROR_NONE) return $v;
    $p = new JrgPengurai($s);
    $v = $p->nilai();
    $p->spasi();
    if ($p->i !== strlen($s)) throw new RuntimeException('JSON: ada sisa sesudah nilainya');
    return $v;
}

final class JrgPengurai {
    public int $i = 0;
    public function __construct(private string $s) {}

    public function spasi(): void {
        $n = strlen($this->s);
        while ($this->i < $n && strpos(" \t\r\n", $this->s[$this->i]) !== false) $this->i++;
    }

    public function nilai(): mixed {
        $this->spasi();
        $c = $this->s[$this->i] ?? '';
        if ($c === '{') return $this->objek();
        if ($c === '[') return $this->daftar();
        if ($c === '"') return $this->teks();
        foreach (['true' => true, 'false' => false, 'null' => null, 'NaN' => NAN,
                  'Infinity' => INF, '-Infinity' => -INF] as $kata => $isi) {
            if (substr_compare($this->s, $kata, $this->i, strlen($kata)) === 0) {
                $this->i += strlen($kata);
                return $isi;
            }
        }
        if (!preg_match('/-?\d+(\.\d+)?([eE][+-]?\d+)?/A', $this->s, $m, 0, $this->i)) {
            throw new RuntimeException('JSON: nilai tidak dikenal di posisi ' . $this->i);
        }
        $this->i += strlen($m[0]);
        if (isset($m[1]) || isset($m[2])) return (float)$m[0];
        $n = (int)$m[0];
        if ((string)$n !== ltrim($m[0], '+')) throw new RuntimeException('JSON: bilangan ' . $m[0] . ' tidak muat di int PHP');
        return $n;
    }

    private function teks(): string {
        if (($this->s[$this->i] ?? '') !== '"') throw new RuntimeException('JSON: kunci harus teks');
        $n = strlen($this->s);
        $j = $this->i + 1;
        while (true) {
            $j += strcspn($this->s, "\"\\", $j);
            if ($j >= $n) throw new RuntimeException('JSON: teks tidak ditutup');
            if ($this->s[$j] === '"') break;
            $j += 2;
        }
        $mentah = substr($this->s, $this->i, $j - $this->i + 1);
        $this->i = $j + 1;
        return json_decode($mentah, false, 2, JSON_THROW_ON_ERROR);
    }

    private function daftar(): array {
        $this->i++;
        $o = [];
        $this->spasi();
        if (($this->s[$this->i] ?? '') === ']') { $this->i++; return $o; }
        while (true) {
            $o[] = $this->nilai();
            $this->spasi();
            $c = $this->s[$this->i++] ?? '';
            if ($c === ']') return $o;
            if ($c !== ',') throw new RuntimeException('JSON: daftar butuh `,` atau `]`');
        }
    }

    private function objek(): array {
        $this->i++;
        $o = [];
        $this->spasi();
        if (($this->s[$this->i] ?? '') === '}') { $this->i++; return $o; }
        while (true) {
            $this->spasi();
            $k = $this->teks();
            $this->spasi();
            if (($this->s[$this->i++] ?? '') !== ':') throw new RuntimeException('JSON: objek butuh `:`');
            $o[$k] = $this->nilai();
            $this->spasi();
            $c = $this->s[$this->i++] ?? '';
            if ($c === '}') return $o;
            if ($c !== ',') throw new RuntimeException('JSON: objek butuh `,` atau `}`');
        }
    }
}

// ── Konversi nilai ──────────────────────────────────────────────────────────

/** Nilai PHP → nilai yang bisa menyeberang, atau `JrgTakBisa` (jadi handle). */
function jrg_konversi(mixed $v, int $dalam = 0): mixed {
    if ($dalam > 64) throw new JrgTakBisa('nilai bersarang lebih dari 64 tingkat');
    if ($v === null || is_bool($v) || is_int($v) || is_float($v)) return $v;
    if (is_string($v)) {
        if (preg_match('//u', $v) === 1) return $v;
        throw new JrgTakBisa('string biner (bukan UTF-8 yang sah)');
    }
    if (is_array($v)) {
        $o = [];
        if (array_is_list($v)) {
            foreach ($v as $x) $o[] = jrg_konversi($x, $dalam + 1);
            return $o;
        }
        foreach ($v as $k => $x) $o[$k] = jrg_konversi($x, $dalam + 1);
        return new JrgDict($o);
    }
    if ($v instanceof stdClass) {
        $o = [];
        foreach (get_object_vars($v) as $k => $x) $o[$k] = jrg_konversi($x, $dalam + 1);
        return new JrgDict($o);
    }
    throw new JrgTakBisa(is_object($v) ? 'objek `' . jrg_tipe($v) . '`' : 'nilai `' . jrg_tipe($v) . '`');
}

function jrg_tipe(mixed $v): string {
    if (is_object($v)) {
        $k = get_class($v);
        return str_contains($k, '@anonymous') ? 'class@anonymous' : $k;
    }
    if (is_resource($v)) return 'resource:' . get_resource_type($v);
    return get_debug_type($v);
}

function jrg_simpan(mixed $v): array {
    $id = JrgPekerja::$nextId++;
    JrgPekerja::$objs[$id] = $v;
    return ['ok' => true, 'ref' => $id, 'tipe' => jrg_tipe($v)];
}

/** Satu-satunya pintu yang memutuskan nilai lawan handle. */
function jrg_balas(mixed $v, bool $refSaja): array {
    if ($refSaja) return jrg_simpan($v);
    try {
        return ['ok' => true, 'value' => jrg_konversi($v)];
    } catch (JrgTakBisa $e) {
        return jrg_simpan($v);
    }
}

function jrg_cek_id(mixed $id): int {
    if (!is_int($id) || !array_key_exists($id, JrgPekerja::$objs)) {
        throw new RuntimeException('handle PHP #' . json_encode($id) . ' sudah tidak ada di worker ini '
            . '— sudah dilepas, atau workernya sempat berhenti');
    }
    return $id;
}

function jrg_ambil(array $req): mixed {
    return JrgPekerja::$objs[jrg_cek_id($req['id'] ?? null)];
}

function jrg_ambil_objek(array $req, string $untuk): object {
    $o = jrg_ambil($req);
    if (!is_object($o)) {
        throw new TypeError($untuk . ' butuh handle ke sebuah OBJEK PHP, dapat `' . jrg_tipe($o) . '`'
            . (is_array($o) ? ' — untuk array pakai php::item/php::set_item' : ''));
    }
    return $o;
}

/** Penanda dari jrgc (`__pyref__`, `__jrgfn__`) jadi objeknya lagi. */
function jrg_buka(mixed $v): mixed {
    if (!is_array($v)) return $v;
    if (count($v) === 1) {
        if (array_key_exists('__pyref__', $v)) return JrgPekerja::$objs[jrg_cek_id($v['__pyref__'])];
        if (array_key_exists('__jrgfn__', $v)) return jrg_fungsi($v['__jrgfn__']);
    }
    foreach ($v as $k => $x) $v[$k] = jrg_buka($x);
    return $v;
}

/** Argumen posisional lalu bernama. Handle di tingkat teratas dikirim sebagai
 *  REFERENSI ke slotnya, jadi parameter by-reference (`sort`) mengubah isinya. */
function jrg_args(array $req): array {
    $hasil = [];
    foreach ($req['args'] ?? [] as $i => $a) {
        if (is_array($a) && count($a) === 1 && array_key_exists('__pyref__', $a)) {
            $hasil[$i] = &JrgPekerja::$objs[jrg_cek_id($a['__pyref__'])];
        } else {
            $hasil[$i] = jrg_buka($a);
        }
    }
    foreach ($req['kwargs'] ?? [] as $k => $a) $hasil[(string)$k] = jrg_buka($a);
    return $hasil;
}

/** Fungsi JRG yang dipanggil dari PHP: satu pertukaran tulis-lalu-baca. */
/** Mati bersama closure pembungkusnya, lalu mengabarkan nomornya ke jrgc. */
final class JrgPenjagaFungsi {
    public static array $mati = [];
    public function __construct(private mixed $n) {}
    public function __destruct() { self::$mati[] = $this->n; }
}

function jrg_fungsi(mixed $n): Closure {
    $penjaga = new JrgPenjagaFungsi($n);
    return static function (...$args) use ($n, $penjaga) {
        foreach ($args as $k => $_) {
            if (is_string($k)) throw new ArgumentCountError(
                "fungsi JRG dipanggil dengan argumen bernama (`$k`) — parameter JRG cuma posisional");
        }
        $argl = [];
        foreach ($args as $a) {
            try { $argl[] = jrg_konversi($a); }
            catch (JrgTakBisa $e) {
                $d = jrg_simpan($a);
                $argl[] = new JrgDict(['__pyref__' => $d['ref'], '__pytipe__' => $d['tipe']]);
            }
        }
        $seq = ++JrgPekerja::$seq;
        jrg_kirim(['cb' => $n, 'seq' => $seq, 'args' => $argl]);
        $baris = fgets(STDIN);
        if ($baris === false) throw new RuntimeException('jrgc menutup pipa saat menunggu hasil callback');
        $jawab = jrg_urai(trim($baris));
        if (!is_array($jawab) || ($jawab['seq'] ?? null) !== $seq) {
            throw new RuntimeException("jawaban callback tertukar (menunggu #$seq) — protokol pipa kacau");
        }
        if (!($jawab['ok'] ?? false)) throw new RuntimeException('fungsi JRG gagal: ' . ($jawab['error'] ?? '?'));
        return jrg_buka($jawab['value'] ?? null);
    };
}

// ── Kode, berkas, templat ───────────────────────────────────────────────────

function jrg_kode(array $req, bool $ekspresi): string {
    $k = (string)($req['code'] ?? '');
    return $ekspresi ? 'return (' . rtrim($k, " \t\r\n;") . ');'
                     : preg_replace('/^\s*<\?php\b/', '', $k);
}

/** Jalur relatif dibaca terhadap direktori kerja program JRG-nya — bukan
 *  include_path, yang juga mencari di direktori runner ini. */
function jrg_berkas(mixed $p, string $untuk): string {
    if (!is_string($p) || $p === '') throw new TypeError("$untuk butuh jalur berkas");
    $abs = ($p[0] === '/' || preg_match('/^[A-Za-z]:[\\\\\/]/', $p)) ? $p : getcwd() . '/' . $p;
    if (!is_file($abs)) throw new RuntimeException("berkas `$p` tidak ada (dicari di `$abs`)");
    return $abs;
}

function jrg_render_dari(string $__jrg_sumber, bool $__jrg_berkas, array $__jrg_vars): string {
    foreach ($__jrg_vars as $k => $_) {
        if (!is_string($k) || !preg_match('/^[a-zA-Z_\x80-\xff][a-zA-Z0-9_\x80-\xff]*$/', $k)
            || $k === 'this' || str_starts_with($k, '__jrg_')) {
            throw new TypeError('kunci `' . $k . '` di data templat bukan nama variabel PHP yang sah '
                . '— datanya harus dict berkunci huruf, angka dan `_`');
        }
    }
    extract($__jrg_vars);
    ob_start();
    try {
        if ($__jrg_berkas) include $__jrg_sumber;
        else eval('?>' . $__jrg_sumber);
        return ob_get_clean();
    } catch (Throwable $t) {
        ob_end_clean();
        throw $t;
    }
}

// ── Penangan permintaan ─────────────────────────────────────────────────────

function jrg_panggil_callable(mixed $f, array $args): mixed {
    if (!is_callable($f)) {
        $nama = is_string($f) ? "`$f`" : '`' . jrg_tipe($f) . '`';
        throw new Error("$nama tidak bisa dipanggil: fungsi atau method statis itu tidak ada di PHP"
            . (is_string($f) ? ' (kalau ia dari pustaka, muat dulu berkasnya — php::require("vendor/autoload.php"))' : ''));
    }
    return call_user_func_array($f, $args);
}

function jrg_tangani(array $req): array {
    $ref = ($req['ref_saja'] ?? false) === true;

    switch ($req['op'] ?? null) {
        case 'call': {
            $f = is_array($req['func'] ?? null) ? jrg_buka($req['func']) : (string)$req['func'];
            return jrg_balas(jrg_panggil_callable($f, jrg_args($req)), $ref);
        }
        case 'new': {
            $k = (string)$req['class'];
            if (!class_exists($k)) {
                throw new Error("kelas `$k` tidak ada di PHP (kalau ia dari pustaka, muat dulu berkasnya "
                    . '— php::require("vendor/autoload.php"))');
            }
            return jrg_simpan((new ReflectionClass($k))->newInstanceArgs(jrg_args($req)));
        }
        case 'method': {
            $o = jrg_ambil_objek($req, 'php::method');
            $m = (string)$req['name'];
            if (!is_callable([$o, $m])) {
                throw new Error('objek `' . jrg_tipe($o) . "` tidak punya method `$m` yang bisa dipanggil dari luar");
            }
            return jrg_balas(call_user_func_array([$o, $m], jrg_args($req)), $ref);
        }
        case 'getattr': {
            $o = jrg_ambil_objek($req, 'php::get');
            $n = (string)$req['name'];
            return jrg_balas($o->$n, $ref);
        }
        case 'setattr': {
            $o = jrg_ambil_objek($req, 'php::set');
            $n = (string)$req['name'];
            $o->$n = jrg_buka($req['value'] ?? null);
            return ['ok' => true, 'value' => null];
        }
        case 'item': {
            $c = jrg_ambil($req);
            return jrg_balas($c[jrg_buka($req['key'] ?? null)], $ref);
        }
        case 'setitem': {
            $slot = &JrgPekerja::$objs[jrg_cek_id($req['id'] ?? null)];
            $k = jrg_buka($req['key'] ?? null);
            $slot[$k] = jrg_buka($req['value'] ?? null);
            return ['ok' => true, 'value' => null];
        }
        case 'delitem': {
            $slot = &JrgPekerja::$objs[jrg_cek_id($req['id'] ?? null)];
            unset($slot[jrg_buka($req['key'] ?? null)]);
            return ['ok' => true, 'value' => null];
        }
        case 'contains': {
            $c = jrg_ambil($req);
            $k = jrg_buka($req['key'] ?? null);
            if (is_array($c)) return ['ok' => true, 'value' => array_key_exists($k, $c)];
            if ($c instanceof ArrayAccess) return ['ok' => true, 'value' => $c->offsetExists($k)];
            if (is_string($c)) return ['ok' => true, 'value' => str_contains($c, (string)$k)];
            throw new TypeError('php::has butuh array, string, atau objek ArrayAccess, dapat `' . jrg_tipe($c) . '`');
        }
        case 'len': {
            $c = jrg_ambil($req);
            if (is_array($c) || $c instanceof Countable) return ['ok' => true, 'value' => count($c)];
            if (is_string($c)) return ['ok' => true, 'value' => strlen($c)];
            throw new TypeError('nilai `' . jrg_tipe($c) . '` tidak punya panjang');
        }
        case 'dir': {
            $c = jrg_ambil($req);
            if (is_array($c)) return ['ok' => true, 'value' => array_map('strval', array_keys($c))];
            if (!is_object($c)) throw new TypeError('php::dir butuh objek atau array, dapat `' . jrg_tipe($c) . '`');
            $nama = array_merge(get_class_methods($c), array_keys(get_object_vars($c)));
            $nama = array_values(array_unique(array_filter($nama, static fn($n) => !str_starts_with($n, '__'))));
            sort($nama);
            return ['ok' => true, 'value' => $nama];
        }
        case 'repr': {
            $c = jrg_ambil($req);
            if (is_string($c) && preg_match('//u', $c) !== 1) {
                $t = 'string biner ' . strlen($c) . ' byte: ' . bin2hex(substr($c, 0, 32))
                    . (strlen($c) > 32 ? '…' : '');
            } else {
                $t = (is_array($c) || is_object($c)) ? rtrim(print_r($c, true)) : var_export($c, true);
            }
            return ['ok' => true, 'value' => $t];
        }
        case 'to_jrg':
            try { return ['ok' => true, 'value' => jrg_konversi(jrg_ambil($req))]; }
            catch (JrgTakBisa $e) {
                $saran = str_starts_with($e->getMessage(), 'string biner')
                    ? ' — ubah dulu di sisi PHP: bin2hex(...) atau base64_encode(...)'
                    : ' — pakai method atau propertinya lewat handle ini';
                return ['ok' => false, 'error' => $e->getMessage() . ' tidak punya padanan di JRG' . $saran];
            }
        case 'render': {
            $vars = jrg_buka($req['vars'] ?? []);
            if (!is_array($vars)) throw new TypeError('data templat harus dict');
            if (($req['berkas'] ?? true) === true) {
                return jrg_balas(jrg_render_dari(jrg_berkas($req['path'] ?? null, 'php::render'), true, $vars), false);
            }
            return jrg_balas(jrg_render_dari((string)$req['template'], false, $vars), false);
        }
        case 'ping':   return ['ok' => true, 'value' => count(JrgPekerja::$objs)];
        default:       return ['ok' => false, 'error' => 'operasi tidak dikenal dari jrgc: ' . json_encode($req['op'] ?? null)];
    }
}

/** Galat PHP → satu kalimat. Warning menyebut jenisnya, bukan `ErrorException`. */
function jrg_pesan_galat(Throwable $t): string {
    $jenis = get_class($t);
    if ($t instanceof ErrorException) {
        $jenis = [E_WARNING => 'Warning', E_NOTICE => 'Notice', E_USER_WARNING => 'Warning',
                  E_USER_NOTICE => 'Notice', E_USER_ERROR => 'Error', E_RECOVERABLE_ERROR => 'Error',
                  E_CORE_WARNING => 'Warning', E_COMPILE_WARNING => 'Warning'][$t->getSeverity()] ?? 'Error';
    }
    $pesan = $jenis . ': ' . $t->getMessage();
    $berkas = $t->getFile();
    if ($berkas !== __FILE__ && !str_contains($berkas, "eval()'d code") && $berkas !== '') {
        $pesan .= ' (di ' . $berkas . ':' . $t->getLine() . ')';
    }
    if (str_contains($t->getMessage(), 'must be passed by reference, value given')) {
        $pesan .= "\n  bantuan: parameter by-reference cuma bisa diisi handle PHP (`php::ref(...)`) — "
            . 'nilai JRG yang dikirim adalah SALINAN, jadi perubahan PHP atasnya tidak akan pernah kembali';
    }
    return $pesan;
}

// ── Loop utama ──────────────────────────────────────────────────────────────
// eval/exec/require jalan DI SINI, di lingkup global sungguhan: `$x = 1` di
// satu `php::exec` adalah `global $x` bagi fungsi PHP mana pun sesudahnya.

while (($__jrg_baris = fgets(STDIN)) !== false) {
    $__jrg_baris = trim($__jrg_baris);
    if ($__jrg_baris === '') continue;
    JrgPekerja::$menjawab = true;
    try {
        $__jrg_req = jrg_urai($__jrg_baris);
        if (!is_array($__jrg_req) || !array_key_exists('op', $__jrg_req)) {
            // Jawaban callback yang pertukarannya sudah lewat: dibuang.
            JrgPekerja::$menjawab = false;
            continue;
        }
        foreach ($__jrg_req['lepas'] ?? [] as $__jrg_i) unset(JrgPekerja::$objs[$__jrg_i]);
        switch ($__jrg_req['op']) {
            case 'eval':
                $__jrg_teks = jrg_json(jrg_balas(eval(jrg_kode($__jrg_req, true)),
                                                 ($__jrg_req['ref_saja'] ?? false) === true));
                break;
            case 'exec':
                eval(jrg_kode($__jrg_req, false));
                $__jrg_teks = '{"ok":true,"value":null}';
                break;
            case 'require':
                $__jrg_teks = jrg_json(jrg_balas(
                    require_once jrg_berkas($__jrg_req['path'] ?? null, 'php::require'), false));
                break;
            default:
                $__jrg_teks = jrg_json(jrg_tangani($__jrg_req));
        }
    } catch (Throwable $__jrg_t) {
        $__jrg_teks = jrg_json(['ok' => false, 'error' => jrg_pesan_galat($__jrg_t)]);
    }
    JrgPekerja::$menjawab = false;
    if (JrgPenjagaFungsi::$mati && str_ends_with($__jrg_teks, '}')) {
        $__jrg_teks = substr($__jrg_teks, 0, -1) . ',"fn_lepas":' . json_encode(JrgPenjagaFungsi::$mati) . '}';
        JrgPenjagaFungsi::$mati = [];
    }
    fwrite(STDOUT, $__jrg_teks . "\n");
    fflush(STDOUT);
}
