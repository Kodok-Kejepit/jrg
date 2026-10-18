// ===========================================================================
// js_runner.js — sisi JavaScript dari `js::`.

'use strict';


const fs = require('fs');

// fd 1 adalah KAWAT PROTOKOL, ditulis SINKRON (`fs.writeSync`) — callback
// menunggu jawabannya dengan membaca sinkron, jadi tulisannya harus sudah …
Object.defineProperty(process, 'stdout', { get: () => process.stderr, configurable: true });
const _format = require('util').format;
for (const m of ['log', 'info', 'warn', 'error', 'debug', 'trace', 'dir']) {
  console[m] = (...a) => process.stderr.write(_format(...a) + '\n');
}

function tidur(ms) { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms); }

function kirim(obj) {
  let b = Buffer.from(keJson(obj) + '\n');
  while (b.length > 0) {
    try { b = b.subarray(fs.writeSync(1, b)); }
    catch (e) { if (e.code === 'EAGAIN') tidur(1); else throw e; }
  }
}

/// Ubah sebuah galat JS jadi satu kalimat yang layak dibaca orang.
function pesanGalat(e) {
  if (!(e instanceof Error)) return String(e);
  return (e.name ? e.name + ': ' : '') + (e.message || String(e));
}

/// Serialisasi JSON sendiri, bukan `JSON.stringify`.
function keJson(v) {
  if (v === null || v === undefined) return 'null';
  const t = typeof v;
  if (t === 'boolean') return v ? 'true' : 'false';
  if (t === 'bigint') return v.toString();
  if (t === 'number') {
    if (Number.isNaN(v)) return 'NaN';
    if (v === Infinity) return 'Infinity';
    if (v === -Infinity) return '-Infinity';
    return JSON.stringify(v);
  }
  if (t === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map(keJson).join(',') + ']';
  if (t === 'object') {
    const bagian = [];
    for (const k of Object.keys(v)) {
      if (v[k] === undefined) continue;
      bagian.push(JSON.stringify(String(k)) + ':' + keJson(v[k]));
    }
    return '{' + bagian.join(',') + '}';
  }
  // Sisa (function, symbol) tidak pernah sampai ke sini: `konversi` sudah
  // melemparnya lebih dulu dan mengubahnya jadi handle.
  return JSON.stringify(String(v));
}

// ── Tabel objek
// ────────────────────────────────────────────────────────────
const _objs = new Map();
let _nextId = 1;
function simpan(v) { const id = _nextId++; _objs.set(id, v); return id; }
function ambilObj(req) {
  const id = req.id;
  if (!_objs.has(id)) throw new Error('handle js sudah tidak ada (id ' + id + ')');
  return _objs.get(id);
}

// Konteks global untuk `eval`/`exec`, hidup selama worker hidup — jadi
// `require` di satu `js::exec` terlihat oleh `js::eval` berikutnya. Disengaja.
const _konteks = Object.create(null);
_konteks.require = require;
_konteks.console = console;
_konteks.process = process;
_konteks.Buffer = Buffer;
_konteks.__jrg = {};

function jalankanKode(src, mode) {
  // `new Function` dipakai, bukan `vm.runInContext`: yang dibutuhkan cuma satu
  // lingkup yang bertahan, dan `vm` menambah isolasi yang justru bukan yang …
  const nama = Object.keys(_konteks);
  const nilai = nama.map((k) => _konteks[k]);
  const badan = mode === 'eval' ? 'return (' + src + ');' : src;
  const f = new Function(...nama, '"use strict";' + badan);
  return f(...nilai);
}

// ── Konversi nilai ─────────────────────────────────────────────────────────
class TakBisaDikonversi extends Error {}

/// Nilai JS → nilai yang bisa diseberangkan.
function konversi(v, dalam = 0) {
  if (dalam > 64) throw new TakBisaDikonversi('nilai bersarang lebih dari 64 tingkat');
  if (v === null || v === undefined) return null;
  const t = typeof v;
  if (t === 'boolean' || t === 'string') return v;
  if (t === 'number') {
    if (!Number.isFinite(v)) return v;      // inf/NaN diteruskan apa adanya
    return v;
  }
  // `BigInt` diteruskan apa adanya; `keJson` memancarkannya sebagai angka
  // telanjang, sama seperti `10**40` dari sisi Python. Yang tidak muat di
  // `int` 128-bit ditolak di sisi Rust dengan kalimatnya sendiri.
  if (t === 'bigint') return v;
  if (Array.isArray(v)) return v.map((x) => konversi(x, dalam + 1));
  if (v instanceof Date) return v.toISOString();
  if (v instanceof Map) {
    const o = {};
    for (const [k, val] of v) o[String(k)] = konversi(val, dalam + 1);
    return o;
  }
  if (v instanceof Set) return Array.from(v).map((x) => konversi(x, dalam + 1));
  if (t === 'object') {
    const proto = Object.getPrototypeOf(v);
    if (proto === Object.prototype || proto === null) {
      const o = {};
      for (const k of Object.keys(v)) o[k] = konversi(v[k], dalam + 1);
      return o;
    }
    throw new TakBisaDikonversi(
      'objek JavaScript bertipe `' + (v.constructor ? v.constructor.name : 'anonim') + '`');
  }
  throw new TakBisaDikonversi('nilai JavaScript bertipe `' + t + '`');
}

/// Jawab sebuah nilai: apa adanya kalau bisa dikonversi, sebagai handle kalau
/// tidak — atau selalu sebagai handle kalau `ref_saja`.
function balas(v, refSaja) {
  if (refSaja) return { ok: true, ref: simpan(v), tipe: namaTipe(v) };
  try {
    return { ok: true, value: konversi(v) };
  } catch (e) {
    if (e instanceof TakBisaDikonversi) return { ok: true, ref: simpan(v), tipe: namaTipe(v) };
    throw e;
  }
}

function namaTipe(v) {
  if (v === null) return 'null';
  if (Array.isArray(v)) return 'Array';
  const t = typeof v;
  if (t === 'object') return v.constructor ? v.constructor.name : 'Object';
  return t;
}

// ── Callback ke JRG
// ──────────────────────────────────────────────────────── SINKRON, seperti …
let _seq = 0;
let _sibuk = false;          // sedang melayani permintaan: tidak ada baca async
let _galatTertunda = null;   // dilaporkan di jawaban berikutnya

// Pembungkus yang sudah dikumpulkan GC dikabarkan di jawaban berikutnya,
// supaya jrgc bisa melepas fungsinya.
const _fnMati = [];
const _pengawasFungsi = new FinalizationRegistry((fid) => _fnMati.push(fid));

function bungkusFungsiJrg(fid) {
  const f = bungkusFungsiJrgMentah(fid);
  _pengawasFungsi.register(f, fid);
  return f;
}

function bungkusFungsiJrgMentah(fid) {
  return function (...args) {
    if (!_sibuk) {
      const pesan = 'fungsi JRG dipanggil sesudah permintaannya selesai (mis. dari '
        + '`setTimeout` atau Promise yang tidak ditunggu) — jrgc cuma melayani callback '
        + 'selama permintaan js:: masih berjalan';
      _galatTertunda = _galatTertunda || pesan;
      throw new Error(pesan);
    }
    const seq = ++_seq;
    const konv = args.map((a) => {
      // Argumen yang tidak bisa menyeberang jadi handle.
      try { return konversi(a); } catch { return { __pyref__: simpan(a) }; }
    });
    kirim({ cb: fid, seq, args: konv });
    for (;;) {
      const baris = bacaBarisSinkron();
      let jawab;
      try { jawab = JSON.parse(baris); } catch { continue; }
      if (!jawab || jawab.seq !== seq) continue;   // bukan untuk pertukaran ini
      if (jawab.ok) return bukaRef(jawab.value);
      throw new Error(String(jawab.error));
    }
  };
}

/// Ubah argumen dari jrgc: `{__pyref__: id}` jadi objeknya lagi,
/// `{__jrgfn__: id}` jadi fungsi async.
function bukaRef(v) {
  if (v === null || typeof v !== 'object') return v;
  if (Array.isArray(v)) return v.map(bukaRef);
  if (Object.prototype.hasOwnProperty.call(v, '__pyref__')) {
    const id = v.__pyref__;
    if (!_objs.has(id)) throw new Error('handle js sudah tidak ada (id ' + id + ')');
    return _objs.get(id);
  }
  if (Object.prototype.hasOwnProperty.call(v, '__jrgfn__')) return bungkusFungsiJrg(v.__jrgfn__);
  const o = {};
  for (const k of Object.keys(v)) o[k] = bukaRef(v[k]);
  return o;
}

// ── Penangan permintaan ────────────────────────────────────────────────────
function jalurAtribut(obj, jalur) {
  let t = obj;
  for (const b of String(jalur).split('.')) {
    if (t === null || t === undefined) throw new Error('`' + jalur + '`: bagian `' + b + '` tidak ada');
    t = t[b];
  }
  return t;
}

async function tangani(req) {
  const op = req.op;

  // Pembebasan dititipkan di permintaan mana pun, sama seperti `py::` — `Drop`
  // di sisi Rust tidak boleh melakukan I/O yang bisa gagal atau memblokir.
  if (Array.isArray(req.lepas)) for (const i of req.lepas) _objs.delete(i);

  const refSaja = req.ref_saja === true;

  switch (op) {
    case 'eval':   return balas(await jalankanKode(req.code, 'eval'), refSaja);
    case 'exec':   await jalankanKode(req.code, 'exec'); return { ok: true, value: null };
    case 'import': {
      // `require` DAN import ESM: paket modern hanya menyediakan yang kedua,
      // dan menolaknya berarti separuh ekosistem tidak terjangkau.
      let m;
      try { m = require(req.module); }
      catch (e) {
        if (e && (e.code === 'ERR_REQUIRE_ESM' || e.code === 'ERR_REQUIRE_ASYNC_MODULE')) {
          m = await import(req.module);
        } else { throw e; }
      }
      _konteks[namaAman(req.module)] = m;
      return { ok: true, value: null };
    }
    case 'call': {
      const m = _konteks[namaAman(req.module)] ?? require(req.module);
      const f = jalurAtribut(m, req.func);
      const induk = String(req.func).includes('.')
        ? jalurAtribut(m, String(req.func).split('.').slice(0, -1).join('.')) : m;
      return balas(await f.apply(induk, bukaRef(req.args || [])), refSaja);
    }
    case 'getattr': return balas(await jalurAtribut(ambilObj(req), req.name), refSaja);
    case 'method': {
      const o = ambilObj(req);
      const f = jalurAtribut(o, req.name);
      if (typeof f !== 'function') throw new Error('`' + req.name + '` bukan fungsi');
      const induk = String(req.name).includes('.')
        ? jalurAtribut(o, String(req.name).split('.').slice(0, -1).join('.')) : o;
      return balas(await f.apply(induk, bukaRef(req.args || [])), refSaja);
    }
    case 'item':    return balas(await ambilObj(req)[bukaRef(req.key)], refSaja);
    case 'to_jrg':
      try { return { ok: true, value: konversi(ambilObj(req)) }; }
      catch (e) {
        if (e instanceof TakBisaDikonversi) {
          return { ok: false, error:
            e.message + ' tidak punya padanan di JRG, jadi tidak bisa dijadikan nilai '
            + 'biasa — pakai method atau propertinya lewat handle ini' };
        }
        throw e;
      }
    case 'repr': {
      const v = ambilObj(req);
      let t;
      try { t = typeof v === 'string' ? v : JSON.stringify(v); } catch { t = String(v); }
      return { ok: true, value: t === undefined ? String(v) : t };
    }
    case 'setattr': ambilObj(req)[req.name] = bukaRef(req.value); return { ok: true, value: null };
    case 'setitem': ambilObj(req)[bukaRef(req.key)] = bukaRef(req.value); return { ok: true, value: null };
    case 'delitem': delete ambilObj(req)[bukaRef(req.key)]; return { ok: true, value: null };
    case 'len': {
      const v = ambilObj(req);
      if (v === null || v === undefined) throw new Error('nilai ini tidak punya panjang');
      if (typeof v.length === 'number') return { ok: true, value: v.length };
      if (typeof v.size === 'number') return { ok: true, value: v.size };
      if (typeof v === 'object') return { ok: true, value: Object.keys(v).length };
      throw new Error('nilai bertipe `' + typeof v + '` tidak punya panjang');
    }
    case 'dir': {
      const v = ambilObj(req);
      const keluar = new Set();
      for (let o = v; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) {
        for (const k of Object.getOwnPropertyNames(o)) {
          if (!k.startsWith('_') && k !== 'constructor') keluar.add(k);
        }
      }
      return { ok: true, value: Array.from(keluar).sort() };
    }
    case 'contains': {
      const o = ambilObj(req);
      const k = bukaRef(req.key);
      if (Array.isArray(o)) return { ok: true, value: o.includes(k) };
      if (o instanceof Set || o instanceof Map) return { ok: true, value: o.has(k) };
      if (typeof o === 'string') return { ok: true, value: o.includes(String(k)) };
      return { ok: true, value: Object.prototype.hasOwnProperty.call(o, k) };
    }
    case 'release': if (Array.isArray(req.ids)) for (const i of req.ids) _objs.delete(i);
                    return { ok: true, value: null };
    case 'ping':    return { ok: true, value: _objs.size };
    default:        return { ok: false, error: 'operasi tidak dikenal dari jrgc: ' + String(op) };
  }
}

function namaAman(m) { return '__mod_' + String(m).replace(/[^A-Za-z0-9_]/g, '_'); }

// ── Loop utama ─────────────────────────────────────────────────────────────
// Satu buffer untuk dua pembaca: loop utama membaca ASYNC hanya saat menunggu
// permintaan berikutnya, callback membaca SINKRON selama permintaan berjalan.
let _sisa = Buffer.alloc(0);
const _potong = Buffer.alloc(65536);

function ambilBaris() {
  const i = _sisa.indexOf(10);
  if (i < 0) return null;
  const baris = _sisa.subarray(0, i).toString('utf8');
  _sisa = _sisa.subarray(i + 1);
  return baris;
}
function tambah(n) {
  if (n === 0) process.exit(0);      // jrgc menutup pipa
  _sisa = Buffer.concat([_sisa, _potong.subarray(0, n)]);
}
function bacaBarisSinkron() {
  for (;;) {
    const b = ambilBaris();
    if (b !== null) return b;
    let n;
    try { n = fs.readSync(0, _potong, 0, _potong.length, null); }
    catch (e) { if (e.code === 'EAGAIN') { tidur(1); continue; } throw e; }
    tambah(n);
  }
}
function bacaBarisAsync() {
  const b = ambilBaris();
  if (b !== null) return Promise.resolve(b);
  return new Promise((res, rej) => {
    fs.read(0, _potong, 0, _potong.length, null, (e, n) => {
      if (e && e.code === 'EAGAIN') { setTimeout(() => bacaBarisAsync().then(res, rej), 1); return; }
      if (e) { rej(e); return; }
      tambah(n);
      bacaBarisAsync().then(res, rej);
    });
  });
}

// Galat dari kode yang jalan di luar permintaan tidak boleh mematikan worker
// diam-diam: ia dicetak, lalu dilaporkan sebagai `Err` di jawaban berikutnya.
function catatGalatLepas(e) {
  process.stderr.write('js:: galat di luar permintaan: ' + pesanGalat(e) + '\n');
  _galatTertunda = _galatTertunda || pesanGalat(e);
}
process.on('uncaughtException', catatGalatLepas);
process.on('unhandledRejection', catatGalatLepas);

(async () => {
  for (;;) {
    const t = (await bacaBarisAsync()).trim();
    if (!t) continue;
    let pesan;
    try { pesan = JSON.parse(t); }
    catch { kirim({ ok: false, error: 'baris dari jrgc bukan JSON: ' + t.slice(0, 200) }); continue; }
    if (!(pesan && typeof pesan === 'object' && 'op' in pesan)) continue;
    _sibuk = true;
    let jawab;
    try { jawab = await tangani(pesan); }
    catch (e) { jawab = { ok: false, error: pesanGalat(e) }; }
    _sibuk = false;
    if (_galatTertunda !== null && jawab.ok) {
      jawab = { ok: false, error: _galatTertunda };
    }
    _galatTertunda = null;
    if (_fnMati.length) jawab.fn_lepas = _fnMati.splice(0);
    kirim(jawab);
  }
})();
