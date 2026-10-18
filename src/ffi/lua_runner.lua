-- src/ffi/lua_runner.lua — worker `lua::`: satu baris JSON masuk, satu baris JSON keluar,
-- protokol yang sama dengan py_runner.py/js_runner.js/php_runner.php. Tahap 1 berbasis
-- NILAI: hasil yang tidak bisa dikonversi (fungsi, userdata) ditolak, bukan dijadikan handle.

local proto_in, proto_out = io.stdin, io.stdout
-- Kawat protokol milik stdout; keluaran pemakai (`print`, `io.write`) ke stderr.
io.stdout = io.stderr
io.output(io.stderr)
print = function(...)
  local n = select("#", ...)
  local bagian = {}
  for i = 1, n do bagian[i] = tostring((select(i, ...))) end
  io.stderr:write(table.concat(bagian, "\t"), "\n")
end

-- ── JSON ───────────────────────────────────────────────────────────────────

local function galat_json(s, pos, apa)
  error({ json = true, pesan = string.format("JSON tidak valid di posisi %d: %s", pos, apa) }, 0)
end

local function lewati_spasi(s, pos)
  return (s:find("[^ \t\r\n]", pos)) or (#s + 1)
end

local function utf8_dari(kode)
  return utf8.char(kode)
end

local baca_nilai
-- Panjang array JSON (termasuk `null` di dalamnya) disimpan di SAMPING tabelnya, supaya
-- argumen `nil` tidak hilang tanpa memakai kunci yang bisa bentrok dengan data pemakai.
local panjang = setmetatable({}, { __mode = "k" })

local function baca_string(s, pos)
  local bagian, i = {}, pos + 1
  while true do
    local c = s:sub(i, i)
    if c == "" then galat_json(s, i, "string tidak ditutup") end
    if c == '"' then return table.concat(bagian), i + 1 end
    if c == "\\" then
      local e = s:sub(i + 1, i + 1)
      local peta = { ['"'] = '"', ["\\"] = "\\", ["/"] = "/", b = "\b", f = "\f", n = "\n", r = "\r", t = "\t" }
      if peta[e] then
        bagian[#bagian + 1] = peta[e]; i = i + 2
      elseif e == "u" then
        local kode = tonumber(s:sub(i + 2, i + 5), 16)
        if not kode then galat_json(s, i, "escape \\u rusak") end
        i = i + 6
        if kode >= 0xD800 and kode <= 0xDBFF and s:sub(i, i + 1) == "\\u" then
          local bawah = tonumber(s:sub(i + 2, i + 5), 16)
          if bawah and bawah >= 0xDC00 and bawah <= 0xDFFF then
            kode = 0x10000 + (kode - 0xD800) * 0x400 + (bawah - 0xDC00)
            i = i + 6
          end
        end
        bagian[#bagian + 1] = utf8_dari(kode)
      else
        galat_json(s, i, "escape tidak dikenal")
      end
    else
      local j = s:find('[\\"]', i)
      if not j then galat_json(s, i, "string tidak ditutup") end
      bagian[#bagian + 1] = s:sub(i, j - 1)
      i = j
    end
  end
end

local function baca_angka(s, pos)
  for _, kata in ipairs({ "NaN", "Infinity", "-Infinity" }) do
    if s:sub(pos, pos + #kata - 1) == kata then
      local v = (kata == "NaN") and (0 / 0) or ((kata == "Infinity") and math.huge or -math.huge)
      return v, pos + #kata
    end
  end
  local teks = s:match("^-?%d+%.?%d*[eE]?[-+]?%d*", pos)
  if not teks or teks == "" or teks == "-" then galat_json(s, pos, "angka rusak") end
  local v
  if teks:find("[%.eE]") then v = tonumber(teks) + 0.0 else v = math.tointeger(tonumber(teks)) or tonumber(teks) end
  return v, pos + #teks
end

baca_nilai = function(s, pos)
  pos = lewati_spasi(s, pos)
  local c = s:sub(pos, pos)
  if c == "{" then
    local obj = {}
    pos = lewati_spasi(s, pos + 1)
    if s:sub(pos, pos) == "}" then return obj, pos + 1 end
    while true do
      pos = lewati_spasi(s, pos)
      if s:sub(pos, pos) ~= '"' then galat_json(s, pos, "kunci object harus string") end
      local k; k, pos = baca_string(s, pos)
      pos = lewati_spasi(s, pos)
      if s:sub(pos, pos) ~= ":" then galat_json(s, pos, "kurang ':'") end
      local v; v, pos = baca_nilai(s, pos + 1)
      obj[k] = v
      pos = lewati_spasi(s, pos)
      local d = s:sub(pos, pos)
      if d == "}" then return obj, pos + 1 end
      if d ~= "," then galat_json(s, pos, "kurang ',' atau '}'") end
      pos = pos + 1
    end
  elseif c == "[" then
    local arr, n = {}, 0
    pos = lewati_spasi(s, pos + 1)
    if s:sub(pos, pos) == "]" then panjang[arr] = 0; return arr, pos + 1 end
    while true do
      local v; v, pos = baca_nilai(s, pos)
      n = n + 1; arr[n] = v
      pos = lewati_spasi(s, pos)
      local d = s:sub(pos, pos)
      if d == "]" then panjang[arr] = n; return arr, pos + 1 end
      if d ~= "," then galat_json(s, pos, "kurang ',' atau ']'") end
      pos = pos + 1
    end
  elseif c == '"' then
    return baca_string(s, pos)
  elseif s:sub(pos, pos + 3) == "true" then return true, pos + 4
  elseif s:sub(pos, pos + 4) == "false" then return false, pos + 5
  elseif s:sub(pos, pos + 3) == "null" then return nil, pos + 4
  else
    return baca_angka(s, pos)
  end
end

local function decode(s)
  local v, pos = baca_nilai(s, 1)
  pos = lewati_spasi(s, pos)
  if pos <= #s then galat_json(s, pos, "sisa teks sesudah nilai") end
  return v
end

local function tulis_string(s, out)
  out[#out + 1] = '"'
  out[#out + 1] = (s:gsub('[%c"\\]', function(ch)
    if ch == '"' then return '\\"' end
    if ch == "\\" then return "\\\\" end
    if ch == "\n" then return "\\n" end
    if ch == "\t" then return "\\t" end
    if ch == "\r" then return "\\r" end
    return string.format("\\u%04x", ch:byte())
  end))
  out[#out + 1] = '"'
end

local function tulis_angka(v, out)
  if math.type(v) == "integer" then out[#out + 1] = string.format("%d", v); return end
  if v ~= v then out[#out + 1] = "NaN"; return end
  if v == math.huge then out[#out + 1] = "Infinity"; return end
  if v == -math.huge then out[#out + 1] = "-Infinity"; return end
  local s = string.format("%.15g", v)
  if tonumber(s) ~= v then s = string.format("%.17g", v) end
  -- Pecahan tetap pecahan di JRG: `1.0` bukan `1`.
  if not s:find("[%.eEn]") then s = s .. ".0" end
  out[#out + 1] = s
end

local function adalah_deret(t)
  local n, jumlah = 0, 0
  for k in pairs(t) do
    if math.type(k) ~= "integer" or k < 1 then return false end
    if k > n then n = k end
    jumlah = jumlah + 1
  end
  if jumlah ~= n then return false end
  return true, n
end

local tulis_nilai
tulis_nilai = function(v, out, lihat)
  local t = type(v)
  if v == nil then out[#out + 1] = "null"
  elseif t == "boolean" then out[#out + 1] = v and "true" or "false"
  elseif t == "number" then tulis_angka(v, out)
  elseif t == "string" then tulis_string(v, out)
  elseif t == "table" then
    if lihat[v] then error("tabel Lua yang memuat dirinya sendiri tidak bisa dikirim ke JRG", 0) end
    lihat[v] = true
    local deret, n = adalah_deret(v)
    if deret then
      out[#out + 1] = "["
      for i = 1, n do
        if i > 1 then out[#out + 1] = "," end
        tulis_nilai(v[i], out, lihat)
      end
      out[#out + 1] = "]"
    else
      local kunci = {}
      for k in pairs(v) do kunci[#kunci + 1] = k end
      table.sort(kunci, function(a, b) return tostring(a) < tostring(b) end)
      out[#out + 1] = "{"
      for i, k in ipairs(kunci) do
        if i > 1 then out[#out + 1] = "," end
        tulis_string(tostring(k), out)
        out[#out + 1] = ":"
        tulis_nilai(v[k], out, lihat)
      end
      out[#out + 1] = "}"
    end
    lihat[v] = nil
  else
    error(string.format("nilai Lua bertipe `%s` tidak bisa dikonversi ke JRG — `lua::` tahap ini " ..
      "cuma mengirim nilai (angka, teks, bool, nil, tabel)", t), 0)
  end
end

local function encode(v)
  local out = {}
  tulis_nilai(v, out, {})
  return table.concat(out)
end

-- ── Fungsi JRG sebagai callback ────────────────────────────────────────────

local seq = 0

local function kirim_baris(s)
  proto_out:write(s, "\n")
  proto_out:flush()
end

local function fungsi_jrg(n)
  return function(...)
    local args = table.pack(...)
    local isi = {}
    for i = 1, args.n do isi[i] = args[i] end
    seq = seq + 1
    local nomor = seq
    local body = encode(isi)
    kirim_baris(string.format('{"cb":%d,"seq":%d,"args":%s}', n, nomor, body))
    local baris = proto_in:read("l")
    if not baris then error("jrgc menutup pipa di tengah callback", 0) end
    local jawab = decode(baris)
    if jawab.seq ~= nomor then
      error(string.format("jawaban callback #%d datang untuk pertukaran #%s — protokol kacau",
        nomor, tostring(jawab.seq)), 0)
    end
    if not jawab.ok then error(jawab.error or "callback JRG gagal", 0) end
    return jawab.value
  end
end

-- Penanda `{"__jrgfn__": n}` dari jrgc ditukar jadi fungsi Lua, di kedalaman mana pun.
local function buka(v)
  if type(v) ~= "table" then return v end
  if v.__jrgfn__ ~= nil then
    local jumlah = 0
    for _ in pairs(v) do jumlah = jumlah + 1 end
    if jumlah == 1 then return fungsi_jrg(v.__jrgfn__) end
  end
  for k, x in pairs(v) do v[k] = buka(x) end
  return v
end

-- ── Operasi ────────────────────────────────────────────────────────────────

local function hasil_dari(n, ...)
  if n == 0 then return nil end
  if n == 1 then return (...) end
  local t = table.pack(...)
  t.n = nil
  return t
end

local function cari_fungsi(nama)
  local f = _G
  for bagian in nama:gmatch("[^%.]+") do
    if type(f) ~= "table" then f = nil; break end
    f = f[bagian]
  end
  if type(f) ~= "function" and not (type(f) == "table" and getmetatable(f) and getmetatable(f).__call) then
    error(string.format("`%s` bukan fungsi Lua yang dikenal (dapat `%s`)", nama, type(f)), 0)
  end
  return f
end

local function muat(kode, nama)
  local f, salah = load(kode, nama, "t", _G)
  if not f then error(salah, 0) end
  return f
end

local function tangani(req)
  local op = req.op
  if op == "eval" then
    local f = load("return " .. req.code, "=lua::eval", "t", _G) or muat(req.code, "=lua::eval")
    local r = table.pack(f())
    return hasil_dari(r.n, table.unpack(r, 1, r.n))
  elseif op == "exec" then
    muat(req.code, "=lua::exec")()
    return nil
  elseif op == "call" then
    local f = cari_fungsi(req.func)
    local args = req.args or {}
    local n = panjang[args] or #args
    args = buka(args)
    local hasil = table.pack(f(table.unpack(args, 1, n)))
    return hasil_dari(hasil.n, table.unpack(hasil, 1, hasil.n))
  elseif op == "require" then
    local m = require(req.name)
    _G[req.name:match("([^%.]+)$")] = m
    return nil
  elseif op == "ping" then
    return 0
  end
  error("operasi tidak dikenal dari jrgc: " .. tostring(op), 0)
end

local function jawab(req)
  local ok, v = pcall(tangani, req)
  if not ok then
    local pesan = type(v) == "table" and (v.pesan or tostring(v)) or tostring(v)
    return '{"ok":false,"error":' .. encode(pesan) .. "}"
  end
  local ok2, teks = pcall(encode, v)
  if not ok2 then return '{"ok":false,"error":' .. encode(tostring(teks)) .. "}" end
  return '{"ok":true,"value":' .. teks .. "}"
end

package.path = "./?.lua;./?/init.lua;" .. package.path

while true do
  local baris = proto_in:read("l")
  if not baris then break end
  if baris:match("%S") then
    local ok, req = pcall(decode, baris)
    if not ok or type(req) ~= "table" then
      kirim_baris('{"ok":false,"error":' .. encode("JSON dari jrgc tidak valid: " .. baris:sub(1, 120)) .. "}")
    else
      kirim_baris(jawab(req))
    end
  end
end
