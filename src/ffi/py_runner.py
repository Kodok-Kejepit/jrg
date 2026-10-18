#!/usr/bin/env python3
# ============================================================================
# JawaRustGo++ — src/ffi/py_runner.py
import sys
import os
import queue
import threading
import json
import importlib

_proto_out = os.fdopen(os.dup(1), "w", buffering=1)

# Satu kunci untuk SELURUH lalu lintas pipa.
_pipa = threading.Lock()

# ── SATU pembaca untuk stdin, dan kenapa itu wajib
# ──────────────────────────
_antre_minta = queue.Queue()
_antre_jawab = queue.Queue()


def _pembaca_stdin():
    while True:
        baris = sys.stdin.readline()
        if not baris:
            _antre_minta.put(None)      # EOF: bangunkan loop utama
            _antre_jawab.put(None)      # dan siapa pun yang menunggu jawaban
            return
        baris = baris.strip()
        if not baris:
            continue
        try:
            pesan = json.loads(baris)
        except Exception:
            # Baris rusak diperlakukan sebagai permintaan supaya loop utama
            # yang melaporkannya — ia sudah punya kalimat untuk itu, dan
            # membuangnya diam-diam justru menghilangkan buktinya.
            _antre_minta.put(baris)
            continue
        if isinstance(pesan, dict) and "op" in pesan:
            _antre_minta.put(pesan)
        else:
            _antre_jawab.put(pesan)

# Nomor urut pertukaran callback.
_seq = 0
sys.stdout = sys.stderr

# Persists for the whole life of the worker process, so imports/variables
# from one py::exec are visible to a later py::eval/py::call in the same
# jrgc run — this is intentional, not a leak.
_globalns = {"__name__": "__jrg__"}
_modules = {}

# Objek yang ditinggal di sisi Python, dipegang sebuah id oleh JRG.
_objs = {}
_next_id = 1

# numpy/pandas TIDAK diimpor di sini.


def _modul_kalau_sudah_ada(nama):
    """Modul `nama` HANYA kalau sudah pernah diimpor; `None` kalau belum."""
    return sys.modules.get(nama)


class TakBisaDikonversi(Exception):
    """Nilai ini tidak punya padanan di JRG — panggilnya harus jadi handle."""


def _konversi(v):
    """Ubah jadi sesuatu yang bisa dibawa JSON, atau angkat `TakBisaDikonversi`."""
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    _np = _modul_kalau_sudah_ada("numpy")
    if _np is not None:
        if isinstance(v, _np.ndarray):
            return _konversi(v.tolist())      # bentuk/dtype apa pun, rekursif
        if isinstance(v, _np.generic):
            return _konversi(v.item())        # skalar numpy -> int/float/bool asli
    _pd = _modul_kalau_sudah_ada("pandas")
    if _pd is not None:
        if isinstance(v, _pd.DataFrame):
            return _konversi(v.to_dict(orient="records"))
        if isinstance(v, _pd.Series):
            return _konversi(v.tolist())
    if isinstance(v, (list, tuple)):
        return [_konversi(x) for x in v]
    if isinstance(v, dict):
        return {str(k): _konversi(x) for k, x in v.items()}
    try:
        json.dumps(v)
        return v
    except TypeError:
        raise TakBisaDikonversi(type(v).__name__)


def _simpan(v):
    """Taruh `v` di tabel objek, kembalikan deskripsinya untuk jrgc."""
    global _next_id
    i = _next_id
    _next_id += 1
    _objs[i] = v
    tipe = type(v).__name__
    try:
        modul = type(v).__module__
        if modul and modul not in ("builtins", "__main__"):
            tipe = "%s.%s" % (modul, tipe)
    except Exception:
        pass
    return {"ok": True, "ref": i, "tipe": tipe}


def _balas(v, paksa_ref=False):
    """Satu-satunya pintu yang memutuskan nilai vs handle."""
    if paksa_ref:
        return _simpan(v)
    try:
        return {"ok": True, "value": _konversi(v)}
    except TakBisaDikonversi:
        return _simpan(v)


def _ambil(req):
    """Objek yang ditunjuk `ref` di permintaan ini."""
    i = req.get("ref")
    if i not in _objs:
        raise KeyError(
            "handle Python #%s sudah tidak ada di worker ini — kemungkinan "
            "sudah dilepas, atau workernya sempat restart" % i)
    return _objs[i]


class _FungsiJrg:
    """Fungsi JRG yang dipanggil dari dalam Python."""

    __slots__ = ("_n",)

    def __init__(self, n):
        self._n = n

    def __call__(self, *args, **kwargs):
        if kwargs:
            raise TypeError(
                "fungsi JRG dipanggil dengan keyword argument (%s) — "
                "parameter JRG cuma posisional" % ", ".join(sorted(kwargs)))
        global _seq
        argl = [_konversi_arg(a) for a in args]
        # Tulis-lalu-baca dikunci sebagai SATU pertukaran. Lihat `_pipa`.
        with _pipa:
            _seq += 1
            n = _seq
            _proto_out.write(json.dumps({"cb": self._n, "seq": n, "args": argl}) + "\n")
            _proto_out.flush()
            jawab = _antre_jawab.get()
            if jawab is None:
                raise RuntimeError("jrgc menutup koneksi saat menunggu hasil callback")
        if jawab.get("seq") != n:
            raise RuntimeError(
                "jawaban callback tertukar (menunggu #%d, dapat #%s) — protokol pipa "
                "kacau" % (n, jawab.get("seq")))
        if not jawab.get("ok", False):
            raise RuntimeError("fungsi JRG gagal: %s" % jawab.get("error", "?"))
        return _buka_ref(jawab.get("value"))

    def __repr__(self):
        return "<fungsi JRG #%d>" % self._n

    def __del__(self):
        # Dikabarkan di jawaban berikutnya, supaya jrgc bisa melepas fungsinya.
        _fn_mati.append(self._n)


# Nomor pembungkus fungsi JRG yang sudah mati, menunggu dikabarkan.
_fn_mati = []


def _sertakan_fn_lepas(resp):
    n = len(_fn_mati)
    if n:
        resp["fn_lepas"] = _fn_mati[:n]
        del _fn_mati[:n]
    return resp


def _konversi_arg(v):
    """Argumen callback: Python -> JRG."""
    try:
        return _konversi(v)
    except TakBisaDikonversi:
        d = _simpan(v)
        # Penanda INLINE, karena argumen callback bisa berada di dalam
        # sebuah array — bentuk `{"ok":..,"ref":..}` cuma dikenali di
        # tingkat teratas respons.
        return {"__pyref__": d["ref"], "__pytipe__": d["tipe"]}


def _buka_ref(v):
    """Tukar penanda `{"__pyref__": id}` kembali jadi objeknya."""
    if isinstance(v, dict):
        # Fungsi JRG yang dikirim sebagai argumen.
        if len(v) == 1 and "__jrgfn__" in v:
            return _FungsiJrg(v["__jrgfn__"])
        if len(v) == 1 and "__pyref__" in v:
            i = v["__pyref__"]
            if i not in _objs:
                raise KeyError(
                    "handle Python #%s sudah tidak ada di worker ini — "
                    "kemungkinan sudah dilepas, atau workernya sempat restart" % i)
            return _objs[i]
        return {k: _buka_ref(x) for k, x in v.items()}
    if isinstance(v, list):
        return [_buka_ref(x) for x in v]
    return v


# Kode yang sudah dikompilasi, dipakai ulang antar-panggilan.
_KODE_MAKS = 512
_kode = {}


def _kompilasi(src, mode):
    kunci = (mode, src)
    ko = _kode.get(kunci)
    if ko is None:
        ko = compile(src, "<jrg>", mode)
        if len(_kode) >= _KODE_MAKS:
            _kode.clear()
        _kode[kunci] = ko
    return ko


def _resolve_module(name):
    mod = _modules.get(name)
    if mod is None:
        mod = importlib.import_module(name)
        _modules[name] = mod
    # Also make it reachable by bare name from eval/exec, exactly like a plain
    # `import name` statement would — otherwise `py::call("numpy", "sum", ...)` …
    top_level = name.split(".")[0]
    if top_level not in _globalns:
        _globalns[top_level] = sys.modules[top_level]
    return mod


def _handle(req):
    op = req.get("op")
    try:
        # Pembebasan dititipkan di permintaan mana pun, bukan operasi tersendiri:
        # jrgc mengantre id yang mati lalu menyiramnya di permintaan berikutnya …
        for i in req.get("lepas", ()):
            _objs.pop(i, None)

        if op == "eval":
            return _balas(eval(_kompilasi(req["code"], "eval"), _globalns),
                          req.get("ref_saja", False))

        if op == "exec":
            exec(_kompilasi(req["code"], "exec"), _globalns)
            return {"ok": True, "value": None}

        if op == "import":
            _resolve_module(req["module"])
            return {"ok": True, "value": None}

        if op == "call":
            target = _resolve_module(req["module"])
            for part in req["func"].split("."):
                target = getattr(target, part)
            hasil = target(*_buka_ref(req.get("args", [])),
                           **_buka_ref(req.get("kwargs", {})))
            return _balas(hasil, req.get("ref_saja", False))

        # ── Operasi di atas handle ──────────────────────────────────────
        if op == "getattr":
            return _balas(getattr(_ambil(req), req["name"]),
                          req.get("ref_saja", False))

        if op == "method":
            target = getattr(_ambil(req), req["name"])
            hasil = target(*_buka_ref(req.get("args", [])),
                           **_buka_ref(req.get("kwargs", {})))
            return _balas(hasil, req.get("ref_saja", False))

        if op == "item":
            # Indexing: `df["kolom"]`, `arr[3]`, `d["k"]`.
            return _balas(_ambil(req)[_buka_ref(req["key"])], req.get("ref_saja", False))

        if op == "to_jrg":
            try:
                return {"ok": True, "value": _konversi(_ambil(req))}
            except TakBisaDikonversi as e:
                return {"ok": False, "error":
                        "objek Python bertipe `%s` tidak punya padanan di JRG, "
                        "jadi tidak bisa dijadikan nilai biasa — pakai method "
                        "atau atributnya lewat handle ini" % e}

        if op == "repr":
            return {"ok": True, "value": repr(_ambil(req))}

        # ── Operasi yang MENGUBAH objek Python ──────────────────────────
        if op == "setattr":
            setattr(_ambil(req), req["name"], _buka_ref(req["value"]))
            return {"ok": True, "value": None}

        if op == "setitem":
            _ambil(req)[_buka_ref(req["key"])] = _buka_ref(req["value"])
            return {"ok": True, "value": None}

        if op == "delitem":
            del _ambil(req)[_buka_ref(req["key"])]
            return {"ok": True, "value": None}

        if op == "len":
            return {"ok": True, "value": len(_ambil(req))}

        if op == "dir":
            # Nama yang diawali `_` dibuang: ia bagian dalam objeknya, dan
            # daftar `dir()` mentah untuk sebuah DataFrame panjangnya
            # ratusan sehingga justru menutupi yang dicari.
            return {"ok": True,
                    "value": [n for n in dir(_ambil(req)) if not n.startswith("_")]}

        if op == "contains":
            return {"ok": True, "value": _buka_ref(req["key"]) in _ambil(req)}

        if op == "release":
            for i in req.get("ids", ()):
                _objs.pop(i, None)
            return {"ok": True, "value": None}

        if op == "ping":
            # Dipakai jrgc untuk menyiram antrean pelepasan tanpa
            # mengerjakan apa pun yang lain.
            return {"ok": True, "value": len(_objs)}

        return {"ok": False, "error": "operasi tidak dikenal dari jrgc: %r" % op}
    except TakBisaDikonversi as e:
        return {"ok": False, "error": "tidak bisa dikonversi: %s" % e}
    except Exception as e:
        return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}


def main():
    # stdin tidak lagi dibaca di sini sama sekali: satu thread pembaca yang
    # memilikinya, dan loop ini cuma mengambil dari antreannya. Lihat
    # `_pembaca_stdin` untuk balapan yang ditutupnya.
    threading.Thread(target=_pembaca_stdin, daemon=True).start()
    while True:
        req = _antre_minta.get()
        if req is None:
            break
        if isinstance(req, str):
            resp = {"ok": False, "error": "JSON dari jrgc tidak valid: %s" % req[:120]}
        else:
            resp = _handle(req)
        # Kunci yang sama dengan callback: balasan ini tidak boleh menyelip di
        # tengah pertukaran callback yang belum selesai.
        with _pipa:
            _proto_out.write(json.dumps(_sertakan_fn_lepas(resp)) + "\n")
            _proto_out.flush()


if __name__ == "__main__":
    main()
