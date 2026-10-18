// Aktivitas.java — tampilan "web" APK JRG: program JRG jalan sebagai proses (jrgc asli dari
// lib/<abi>/libjrgc.so) dan melayani HTTP di loopback; WebView ini menampilkannya.
package jrg.apk;

import android.app.Activity;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.content.res.AssetManager;
import android.os.Bundle;
import android.util.Log;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;

public class Aktivitas extends Activity {
    private WebView web;
    // Pelayan JRG milik proses aplikasi, bukan milik aktivitas: selagi Layanan (latar) jalan, aktivitas
    // yang ditutup lalu dibuka lagi memakai pelayan yang sama, jadi sesi terminal tidak hilang.
    static Process proses;
    static Aktivitas kini;
    private final StringBuilder keluaran = new StringBuilder();
    private volatile boolean berhenti;
    private final java.util.Map<Integer, String> permintaan = new java.util.HashMap<Integer, String>();
    private int nomorPermintaan = 100;
    // Berkas dan folder di penyimpanan HP lewat pemilih sistem (Storage Access Framework): tidak
    // butuh izin penyimpanan, pemakai sendiri yang memilih apa yang dibuka atau ke mana disimpan.
    private static final int KODE_PILIH = 7001, KODE_SIMPAN = 7002, KODE_FOLDER_MASUK = 7003, KODE_FOLDER_KELUAR = 7004;
    private android.webkit.ValueCallback<android.net.Uri[]> pilihBerkas;
    private File sasaranSaf;

    // Nama izin ramah (sama dengan `[android] izin` di jrgm.toml) ke nama Android-nya.
    private static final String[][] IZIN = {
        {"kamera", "CAMERA"}, {"mikrofon", "RECORD_AUDIO"}, {"lokasi", "ACCESS_FINE_LOCATION"}, {"kontak", "READ_CONTACTS"},
        {"penyimpanan", "READ_EXTERNAL_STORAGE"}, {"telepon", "CALL_PHONE"}, {"sms", "SEND_SMS"}, {"kalender", "READ_CALENDAR"},
    };

    private static String izinAndroid(String ramah) {
        for (String[] p : IZIN) if (p[0].equals(ramah)) return "android.permission." + p[1];
        return null;
    }

    private boolean diManifes(String izin) {
        try {
            String[] d = getPackageManager().getPackageInfo(getPackageName(), PackageManager.GET_PERMISSIONS).requestedPermissions;
            if (d != null) for (String x : d) if (x.equals(izin)) return true;
        } catch (Exception e) { /* dianggap tidak ada */ }
        return false;
    }

    private boolean diberi(String izin) { return checkSelfPermission(izin) == PackageManager.PERMISSION_GRANTED; }

    // Pintu halaman ke Android: `JRGAndroid.izin(nama)` dan `JRGAndroid.mintaIzin(nama)`; jawaban
    // dialognya datang sebagai peristiwa `jrg-izin` ({nama, diberi}). Rancangannya: apk.md
    private class Jembatan {
        @android.webkit.JavascriptInterface
        public String izin(String nama) {
            String a = izinAndroid(nama);
            if (a == null) return "tidak_dikenal";
            if (!diManifes(a)) return "tidak_di_manifes";
            return diberi(a) ? "diberi" : "belum";
        }

        @android.webkit.JavascriptInterface
        public String mintaIzin(final String nama) {
            final String a = izinAndroid(nama);
            String sekarang = izin(nama);
            if (!sekarang.equals("belum")) return sekarang;
            // Penyimpanan: baca dan tulis satu grup izin, jadi satu dialog memberi keduanya (targetSdk 28).
            final String[] minta = a.equals("android.permission.READ_EXTERNAL_STORAGE") && diManifes("android.permission.WRITE_EXTERNAL_STORAGE")
                ? new String[] { a, "android.permission.WRITE_EXTERNAL_STORAGE" } : new String[] { a };
            runOnUiThread(new Runnable() { public void run() {
                int kode = ++nomorPermintaan;
                permintaan.put(kode, nama);
                requestPermissions(minta, kode);
            } });
            return "ditanyakan";
        }

        // Tetap jalan di latar dengan notifikasi, seperti Termux: `teks` isi notifikasinya ("2 sesi berjalan"),
        // "" menghentikannya. Jawabannya `ok`, `tidak_di_manifes` (butuh `latar` di [android] izin) atau sebabnya.
        @android.webkit.JavascriptInterface
        public String latar(String teks) {
            if (!diManifes("android.permission.FOREGROUND_SERVICE")) return "tidak_di_manifes";
            try {
                android.content.Intent i = new android.content.Intent(Aktivitas.this, Layanan.class);
                if (teks == null || teks.isEmpty()) { stopService(i); return "ok"; }
                if (android.os.Build.VERSION.SDK_INT >= 33 && !diberi("android.permission.POST_NOTIFICATIONS") && !Layanan.ditanya) {
                    Layanan.ditanya = true;
                    runOnUiThread(new Runnable() { public void run() { requestPermissions(new String[] { "android.permission.POST_NOTIFICATIONS" }, 7100); } });
                }
                i.putExtra("teks", teks);
                if (android.os.Build.VERSION.SDK_INT >= 26) startForegroundService(i); else startService(i);
                return "ok";
            } catch (Exception e) {
                return String.valueOf(e);
            }
        }

        // `jalur` mutlak di bawah direktori berkas aplikasi; jawabannya `ditanyakan` atau sebabnya,
        // hasilnya datang sebagai peristiwa `jrg-berkas` ({aksi, ok, jumlah, nama, galat}).
        @android.webkit.JavascriptInterface
        public String ekspor(String jalur) {
            File f = dalamAplikasi(jalur);
            if (f == null || !f.isFile()) return "berkas tidak ada di direktori aplikasi";
            android.content.Intent i = new android.content.Intent(android.content.Intent.ACTION_CREATE_DOCUMENT);
            i.addCategory(android.content.Intent.CATEGORY_OPENABLE);
            i.setType("application/octet-stream");
            i.putExtra(android.content.Intent.EXTRA_TITLE, f.getName());
            return tanyaSaf(i, KODE_SIMPAN, f);
        }

        @android.webkit.JavascriptInterface
        public String eksporFolder(String jalur) {
            File f = dalamAplikasi(jalur);
            if (f == null || !f.isDirectory()) return "folder tidak ada di direktori aplikasi";
            return tanyaSaf(new android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT_TREE), KODE_FOLDER_KELUAR, f);
        }

        // Folder dari penyimpanan disalin UTUH ke `jalur` (folder tujuan di direktori aplikasi).
        @android.webkit.JavascriptInterface
        public String imporFolder(String jalur) {
            File f = dalamAplikasi(jalur);
            if (f == null || !f.isDirectory()) return "folder tujuan tidak ada di direktori aplikasi";
            return tanyaSaf(new android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT_TREE), KODE_FOLDER_MASUK, f);
        }

        // Pintasan bersemat di layar utama dengan ikon PNG sendiri (base64); sistem menampilkan dialognya.
        // Jawabannya `ditanyakan`, `tidak_didukung` (Android < 8 atau peluncur tanpa pintasan), atau sebabnya.
        @android.webkit.JavascriptInterface
        public String pintasan(String label, String pngBase64) {
            if (android.os.Build.VERSION.SDK_INT < 26) return "tidak_didukung";
            try {
                android.content.pm.ShortcutManager sm = getSystemService(android.content.pm.ShortcutManager.class);
                if (sm == null || !sm.isRequestPinShortcutSupported()) return "tidak_didukung";
                byte[] b = android.util.Base64.decode(pngBase64, android.util.Base64.DEFAULT);
                android.graphics.Bitmap bmp = android.graphics.BitmapFactory.decodeByteArray(b, 0, b.length);
                if (bmp == null) return "ikon bukan PNG/JPEG yang sah";
                android.content.Intent buka = new android.content.Intent(Aktivitas.this, Aktivitas.class);
                buka.setAction(android.content.Intent.ACTION_MAIN);
                String nama = (label == null || label.trim().isEmpty()) ? "JRG" : label.trim();
                android.content.pm.ShortcutInfo si = new android.content.pm.ShortcutInfo.Builder(Aktivitas.this, "jrg-" + nama)
                    .setShortLabel(nama).setIcon(android.graphics.drawable.Icon.createWithBitmap(bmp)).setIntent(buka).build();
                return sm.requestPinShortcut(si, null) ? "ditanyakan" : "tidak_didukung";
            } catch (Exception e) {
                return String.valueOf(e);
            }
        }

        // `pola`: milidetik getar,jeda,getar,... seperti `navigator.vibrate`, yang di WebView diam
        // saat HP senyap dan tidak pernah bilang kenapa. Jawabannya menyebut apa yang terjadi.
        @android.webkit.JavascriptInterface
        public String getar(String pola) {
            if (!diManifes("android.permission.VIBRATE")) return "tidak_di_manifes";
            android.os.Vibrator v = (android.os.Vibrator) getSystemService(VIBRATOR_SERVICE);
            if (v == null || !v.hasVibrator()) return "tidak_ada";
            try {
                String[] bagian = pola.split(",");
                long[] waktu = new long[bagian.length + 1];
                long total = 0;
                for (int i = 0; i < bagian.length; i++) {
                    waktu[i + 1] = Math.max(0, Math.min(5000, Long.parseLong(bagian[i].trim())));
                    total += waktu[i + 1];
                }
                if (bagian.length > 40 || total > 15000) return "pola terlalu panjang (paling lama 15 detik)";
                if (android.os.Build.VERSION.SDK_INT >= 26) v.vibrate(android.os.VibrationEffect.createWaveform(waktu, -1));
                else v.vibrate(waktu, -1);
                return "ok";
            } catch (Exception e) {
                return String.valueOf(e);
            }
        }
    }

    // Jalur dari halaman dipercaya HANYA kalau ia di bawah direktori berkas aplikasi.
    private File dalamAplikasi(String jalur) {
        try {
            File f = new File(jalur).getCanonicalFile();
            String akar = getFilesDir().getCanonicalPath() + File.separator;
            return (f.getPath() + File.separator).startsWith(akar) ? f : null;
        } catch (Exception e) { return null; }
    }

    private String tanyaSaf(final android.content.Intent i, final int kode, File sasaran) {
        sasaranSaf = sasaran;
        runOnUiThread(new Runnable() { public void run() {
            try { startActivityForResult(i, kode); }
            catch (Exception e) { kabarBerkas(kode, false, 0, "", "pemilih berkas tidak tersedia: " + e); }
        } });
        return "ditanyakan";
    }

    private void kabarBerkas(int kode, boolean ok, int jumlah, String nama, String galat) {
        String aksi = kode == KODE_SIMPAN ? "ekspor" : kode == KODE_FOLDER_KELUAR ? "ekspor_folder" : "impor_folder";
        final String js = "window.dispatchEvent(new CustomEvent('jrg-berkas',{detail:{aksi:" + kutip(aksi) + ",ok:" + ok + ",jumlah:" + jumlah
            + ",nama:" + kutip(nama) + ",galat:" + kutip(galat) + "}}))";
        runOnUiThread(new Runnable() { public void run() { web.evaluateJavascript(js, null); } });
    }

    private static String kutip(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (char c : s.toCharArray()) {
            if (c == '"' || c == '\\') b.append('\\').append(c);
            else if (c < 32 || c == 0x2028 || c == 0x2029) b.append(String.format("\\u%04x", (int) c));
            else b.append(c);
        }
        return b.append('"').toString();
    }

    @Override
    protected void onActivityResult(final int kode, int hasil, final android.content.Intent data) {
        if (kode == KODE_PILIH) {
            android.net.Uri[] u = null;
            if (hasil == RESULT_OK && data != null) {
                android.content.ClipData c = data.getClipData();
                if (c != null) {
                    u = new android.net.Uri[c.getItemCount()];
                    for (int k = 0; k < u.length; k++) u[k] = c.getItemAt(k).getUri();
                } else if (data.getData() != null) u = new android.net.Uri[] { data.getData() };
            }
            if (pilihBerkas != null) pilihBerkas.onReceiveValue(u);
            pilihBerkas = null;
            return;
        }
        if (kode != KODE_SIMPAN && kode != KODE_FOLDER_MASUK && kode != KODE_FOLDER_KELUAR) { super.onActivityResult(kode, hasil, data); return; }
        final File sasaran = sasaranSaf;
        sasaranSaf = null;
        if (hasil != RESULT_OK || data == null || data.getData() == null || sasaran == null) { kabarBerkas(kode, false, 0, "", "dibatalkan"); return; }
        // Menyalin bisa lama (folder besar): di luar utas tampilan.
        new Thread(new Runnable() { public void run() {
            android.net.Uri uri = data.getData();
            try {
                if (kode == KODE_SIMPAN) {
                    salinAliran(new java.io.FileInputStream(sasaran), getContentResolver().openOutputStream(uri, "wt"));
                    kabarBerkas(kode, true, 1, sasaran.getName(), "");
                } else if (kode == KODE_FOLDER_KELUAR) {
                    android.net.Uri induk = android.provider.DocumentsContract.buildDocumentUriUsingTree(uri, android.provider.DocumentsContract.getTreeDocumentId(uri));
                    int[] n = { 0 };
                    tulisKePohon(induk, sasaran, n);
                    kabarBerkas(kode, true, n[0], sasaran.getName(), "");
                } else {
                    String akar = android.provider.DocumentsContract.getTreeDocumentId(uri);
                    String nama = namaDokumen(android.provider.DocumentsContract.buildDocumentUriUsingTree(uri, akar));
                    File tujuan = new File(sasaran, nama.isEmpty() ? "impor" : nama);
                    int[] n = { 0 };
                    bacaDariPohon(uri, akar, tujuan, n, 0);
                    kabarBerkas(kode, true, n[0], tujuan.getName(), "");
                }
            } catch (Exception e) {
                kabarBerkas(kode, false, 0, "", String.valueOf(e));
            }
        } }, "jrg-berkas").start();
    }

    private static void salinAliran(InputStream i, OutputStream o) throws Exception {
        try {
            byte[] buf = new byte[65536];
            int n;
            while ((n = i.read(buf)) > 0) o.write(buf, 0, n);
        } finally { i.close(); o.close(); }
    }

    private String namaDokumen(android.net.Uri u) {
        android.database.Cursor c = getContentResolver().query(u, new String[] { android.provider.DocumentsContract.Document.COLUMN_DISPLAY_NAME }, null, null, null);
        try { return c != null && c.moveToFirst() ? c.getString(0) : ""; } finally { if (c != null) c.close(); }
    }

    // Nama dari penyedia dokumen disaring: tanpa garis miring, tanpa `.`/`..`.
    private static boolean namaAman(String n) { return n != null && !n.isEmpty() && !n.equals(".") && !n.equals("..") && n.indexOf('/') < 0 && n.indexOf(0) < 0; }

    private void bacaDariPohon(android.net.Uri pohon, String id, File ke, int[] n, int dalam) throws Exception {
        if (dalam > 24) return;
        ke.mkdirs();
        android.net.Uri anak = android.provider.DocumentsContract.buildChildDocumentsUriUsingTree(pohon, id);
        String[] kolom = { android.provider.DocumentsContract.Document.COLUMN_DOCUMENT_ID, android.provider.DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            android.provider.DocumentsContract.Document.COLUMN_MIME_TYPE };
        android.database.Cursor c = getContentResolver().query(anak, kolom, null, null, null);
        if (c == null) return;
        try {
            while (c.moveToNext()) {
                String did = c.getString(0), nama = c.getString(1), jenis = c.getString(2);
                if (!namaAman(nama)) continue;
                if (n[0] >= 5000) throw new Exception("lebih dari 5000 berkas; pilih folder yang lebih kecil");
                if (android.provider.DocumentsContract.Document.MIME_TYPE_DIR.equals(jenis)) bacaDariPohon(pohon, did, new File(ke, nama), n, dalam + 1);
                else {
                    salinAliran(getContentResolver().openInputStream(android.provider.DocumentsContract.buildDocumentUriUsingTree(pohon, did)), new FileOutputStream(new File(ke, nama)));
                    n[0]++;
                }
            }
        } finally { c.close(); }
    }

    private void tulisKePohon(android.net.Uri induk, File dari, int[] n) throws Exception {
        if (dari.isDirectory()) {
            android.net.Uri d = android.provider.DocumentsContract.createDocument(getContentResolver(), induk, android.provider.DocumentsContract.Document.MIME_TYPE_DIR, dari.getName());
            if (d == null) throw new Exception("folder " + dari.getName() + " tidak bisa dibuat di sana");
            File[] isi = dari.listFiles();
            if (isi != null) for (File x : isi) tulisKePohon(d, x, n);
            return;
        }
        android.net.Uri f = android.provider.DocumentsContract.createDocument(getContentResolver(), induk, "application/octet-stream", dari.getName());
        if (f == null) throw new Exception("berkas " + dari.getName() + " tidak bisa dibuat di sana");
        salinAliran(new java.io.FileInputStream(dari), getContentResolver().openOutputStream(f, "wt"));
        n[0]++;
    }

    @Override
    public void onRequestPermissionsResult(int kode, String[] izin, int[] hasil) {
        String nama = permintaan.remove(kode);
        if (nama == null) return;
        boolean ya = hasil.length > 0 && hasil[0] == PackageManager.PERMISSION_GRANTED;
        web.evaluateJavascript("window.dispatchEvent(new CustomEvent('jrg-izin',{detail:{nama:'" + nama + "',diberi:" + ya + "}}))", null);
    }

    @Override
    protected void onCreate(Bundle b) {
        super.onCreate(b);
        kini = this;
        web = new WebView(this);
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setGeolocationEnabled(true);
        s.setMediaPlaybackRequiresUserGesture(false);
        // Galat halaman TIDAK ditelan: konsol dan kegagalan muat masuk logcat (tag `jrg-web`).
        web.setWebViewClient(new WebViewClient() {
            // Halaman "Menyiapkan…" tidak boleh tersisa di riwayat: tombol kembali dulu membawa ke sana dan berhenti.
            @Override
            public void onPageFinished(WebView v, String url) {
                if (url != null && url.startsWith("http")) v.clearHistory();
            }
            @Override
            public void onReceivedError(WebView v, android.webkit.WebResourceRequest r, android.webkit.WebResourceError e) {
                Log.e("jrg-web", "gagal memuat " + r.getUrl() + ": " + e.getDescription());
            }
            @Override
            public void onReceivedHttpError(WebView v, android.webkit.WebResourceRequest r, android.webkit.WebResourceResponse p) {
                Log.e("jrg-web", "HTTP " + p.getStatusCode() + " untuk " + r.getUrl());
            }
        });
        web.setWebChromeClient(new android.webkit.WebChromeClient() {
            @Override
            public boolean onConsoleMessage(android.webkit.ConsoleMessage m) {
                Log.i("jrg-web", m.messageLevel() + " " + m.message() + " (" + m.sourceId() + ":" + m.lineNumber() + ")");
                return true;
            }
            // Kamera/mikrofon halaman (getUserMedia) diberi HANYA kalau aplikasinya sudah memegang izinnya.
            @Override
            public void onPermissionRequest(final android.webkit.PermissionRequest r) {
                runOnUiThread(new Runnable() { public void run() {
                    java.util.List<String> boleh = new java.util.ArrayList<String>();
                    for (String s : r.getResources()) {
                        if (s.equals(android.webkit.PermissionRequest.RESOURCE_VIDEO_CAPTURE) && diberi("android.permission.CAMERA")) boleh.add(s);
                        if (s.equals(android.webkit.PermissionRequest.RESOURCE_AUDIO_CAPTURE) && diberi("android.permission.RECORD_AUDIO")) boleh.add(s);
                    }
                    if (boleh.isEmpty()) r.deny(); else r.grant(boleh.toArray(new String[0]));
                } });
            }
            // `<input type="file">` halaman: pemilih dokumen sistem (tanpa ini WebView mengabaikan ketukannya).
            @Override
            public boolean onShowFileChooser(WebView w, android.webkit.ValueCallback<android.net.Uri[]> cb, FileChooserParams p) {
                if (pilihBerkas != null) pilihBerkas.onReceiveValue(null);
                pilihBerkas = cb;
                android.content.Intent i = new android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT);
                i.addCategory(android.content.Intent.CATEGORY_OPENABLE);
                // Jenis dari `accept` halaman (mis. image/*,video/* untuk latar); kosong = apa saja.
                java.util.List<String> jenis = new java.util.ArrayList<String>();
                for (String a : p.getAcceptTypes()) for (String x : a.split(",")) if (x.trim().contains("/")) jenis.add(x.trim());
                i.setType(jenis.size() == 1 ? jenis.get(0) : "*/*");
                if (jenis.size() > 1) i.putExtra(android.content.Intent.EXTRA_MIME_TYPES, jenis.toArray(new String[0]));
                if (p.getMode() == FileChooserParams.MODE_OPEN_MULTIPLE) i.putExtra(android.content.Intent.EXTRA_ALLOW_MULTIPLE, true);
                try { startActivityForResult(i, KODE_PILIH); }
                catch (Exception e) { pilihBerkas = null; return false; }
                return true;
            }
            @Override
            public void onGeolocationPermissionsShowPrompt(String asal, android.webkit.GeolocationPermissions.Callback cb) {
                cb.invoke(asal, diberi("android.permission.ACCESS_FINE_LOCATION"), false);
            }
        });
        web.addJavascriptInterface(new Jembatan(), "JRGAndroid");
        // Aplikasi bertanda tangan debug bisa diperiksa lewat chrome://inspect.
        if ((getApplicationInfo().flags & ApplicationInfo.FLAG_DEBUGGABLE) != 0) WebView.setWebContentsDebuggingEnabled(true);
        setContentView(web);
        pesan("Menyiapkan…", "");
        new Thread(new Runnable() { public void run() { mulai(); } }, "jrg-mulai").start();
    }

    private String meta(String kunci, String bawaan) {
        try {
            ApplicationInfo ai = getPackageManager().getApplicationInfo(getPackageName(), PackageManager.GET_META_DATA);
            Object v = ai.metaData == null ? null : ai.metaData.get(kunci);
            return v == null ? bawaan : String.valueOf(v);
        } catch (Exception e) { return bawaan; }
    }

    private void mulai() {
        try {
            final int port = Integer.parseInt(meta("jrg.port", "8080"));
            String entri = meta("jrg.entri", "src/main.jrg");
            String mesin = meta("jrg.mesin", "interp");
            File akar = new File(getFilesDir(), "proyek");
            salinProyek(akar);
            String jrgc = getApplicationInfo().nativeLibraryDir + "/libjrgc.so";
            java.util.List<String> perintah = new java.util.ArrayList<String>();
            perintah.add(jrgc);
            perintah.add(new File(akar, entri).getPath());
            if (mesin.equals("vm")) perintah.add("--vm");
            for (String d : meta("jrg.impor", "").split(",")) {
                if (d.length() > 0) { perintah.add("-I"); perintah.add(new File(akar, d).getPath()); }
            }
            // Aktivitas dibuat ulang selagi pelayannya masih hidup (Layanan latar): langsung tampilkan.
            if (proses != null && hidup() && tersambung(port)) { muat("http://127.0.0.1:" + port + "/"); return; }
            ProcessBuilder pb = new ProcessBuilder(perintah);
            pb.directory(akar);
            pb.redirectErrorStream(true);
            java.util.Map<String, String> env = pb.environment();
            env.put("HOME", getFilesDir().getPath());
            env.put("TMPDIR", getCacheDir().getPath());
            env.put("PORT", String.valueOf(port));
            env.put("JRG_APK", "1");
            env.put("JRG_JRGC", jrgc);
            env.put("JRG_PUSTAKA", getApplicationInfo().nativeLibraryDir);
            // `extern "C" "libx.so"` dicari lewat nama: pustaka proyek ada di direktori ini.
            env.put("LD_LIBRARY_PATH", getApplicationInfo().nativeLibraryDir);
            proses = pb.start();
            new Thread(new Runnable() { public void run() { bacaKeluaran(); } }, "jrg-keluaran").start();
            // Tunggu pelayannya menerima sambungan; program yang mati lebih dulu DILAPORKAN.
            for (int i = 0; i < 300 && !berhenti; i++) {
                if (tersambung(port)) { muat("http://127.0.0.1:" + port + "/"); return; }
                if (!hidup()) { pesan("Program berhenti sebelum melayani port " + port, teks()); return; }
                Thread.sleep(100);
            }
            if (!berhenti) pesan("Program tidak melayani port " + port + " dalam 30 detik", teks());
        } catch (Exception e) {
            pesan("Aplikasi gagal dimulai", String.valueOf(e) + "\n" + teks());
        }
    }

    private boolean hidup() {
        try { proses.exitValue(); return false; } catch (IllegalThreadStateException e) { return true; }
    }

    private boolean tersambung(int port) {
        try {
            Socket s = new Socket();
            s.connect(new InetSocketAddress("127.0.0.1", port), 200);
            s.close();
            return true;
        } catch (Exception e) { return false; }
    }

    private void bacaKeluaran() {
        try {
            BufferedReader r = new BufferedReader(new InputStreamReader(proses.getInputStream(), "UTF-8"));
            String baris;
            while ((baris = r.readLine()) != null) {
                Log.i("jrg", baris);
                synchronized (keluaran) {
                    if (keluaran.length() < 20000) keluaran.append(baris).append('\n');
                }
            }
        } catch (Exception e) { /* prosesnya ditutup */ }
    }

    private String teks() { synchronized (keluaran) { return keluaran.toString(); } }

    // Salin aset `proyek/` ke berkas aplikasi tiap versi berganti; data pemakai di luar `proyek/` tidak disentuh.
    private void salinProyek(File akar) throws Exception {
        long kode = getPackageManager().getPackageInfo(getPackageName(), 0).lastUpdateTime;
        File tanda = new File(getFilesDir(), "proyek.versi");
        String lama = "";
        if (tanda.exists()) { InputStream i = new java.io.FileInputStream(tanda); byte[] b = new byte[64]; int n = i.read(b); i.close(); lama = n > 0 ? new String(b, 0, n) : ""; }
        if (lama.equals(String.valueOf(kode)) && akar.isDirectory()) return;
        hapus(akar);
        salin(getAssets(), "proyek", akar);
        OutputStream o = new FileOutputStream(tanda);
        o.write(String.valueOf(kode).getBytes());
        o.close();
    }

    private void hapus(File f) {
        File[] isi = f.listFiles();
        if (isi != null) for (File x : isi) hapus(x);
        f.delete();
    }

    private void salin(AssetManager am, String dari, File ke) throws Exception {
        String[] isi = am.list(dari);
        if (isi == null || isi.length == 0) {
            InputStream i = am.open(dari);
            ke.getParentFile().mkdirs();
            OutputStream o = new FileOutputStream(ke);
            byte[] buf = new byte[65536];
            int n;
            while ((n = i.read(buf)) > 0) o.write(buf, 0, n);
            o.close();
            i.close();
            return;
        }
        ke.mkdirs();
        for (String nama : isi) salin(am, dari + "/" + nama, new File(ke, nama));
    }

    private void muat(final String url) {
        runOnUiThread(new Runnable() { public void run() { web.loadUrl(url); } });
    }

    private void pesan(final String judul, final String rinci) {
        final String aman = rinci.replace("&", "&amp;").replace("<", "&lt;");
        runOnUiThread(new Runnable() { public void run() {
            web.loadDataWithBaseURL(null, "<meta name=viewport content='width=device-width'>"
                + "<body style='font-family:sans-serif;background:#111;color:#eee;padding:16px'><h3>" + judul
                + "</h3><pre style='white-space:pre-wrap;font-size:12px'>" + aman + "</pre>", "text/html", "UTF-8", null);
        } });
    }

    @Override
    public void onBackPressed() {
        // Halaman dulu yang ditanya (`window.jrgKembali()` menutup panel/lembar dan menjawab true);
        // kalau tidak ada yang ditutup, aplikasi ke latar belakang, pelayannya tetap hidup.
        web.evaluateJavascript("(function(){try{return !!(window.jrgKembali&&window.jrgKembali())}catch(e){return false}})()",
            new android.webkit.ValueCallback<String>() { public void onReceiveValue(String v) {
                if ("true".equals(v)) return;
                if (web.canGoBack()) web.goBack(); else moveTaskToBack(true);
            } });
    }

    @Override
    protected void onDestroy() {
        berhenti = true;
        if (kini == this) kini = null;
        // Selagi Layanan jalan, pelayan dan sesinya tetap hidup (keluar lewat notifikasinya).
        if (proses != null && !Layanan.jalan) { proses.destroy(); proses = null; }
        super.onDestroy();
    }

    // Layanan latar (foreground service) dengan notifikasi bergaya Termux: "N sesi berjalan", Keluar, Wake lock.
    // Ketukan notifikasinya membuka aplikasi lagi; Keluar menghentikan pelayan JRG beserta sesinya.
    public static class Layanan extends android.app.Service {
        static volatile boolean jalan, ditanya;
        private static final String SALURAN = "jrg-latar";
        private String teks = "";
        private android.os.PowerManager.WakeLock kunci;

        @Override
        public android.os.IBinder onBind(android.content.Intent i) { return null; }

        @Override
        public int onStartCommand(android.content.Intent i, int bendera, int nomor) {
            String aksi = i == null ? null : i.getAction();
            if ("keluar".equals(aksi)) { keluar(); return START_NOT_STICKY; }
            if ("kunci".equals(aksi)) aturKunci(kunci == null);
            if (i != null && i.hasExtra("teks")) teks = i.getStringExtra("teks");
            jalan = true;
            startForeground(1, notifikasi());
            return START_NOT_STICKY;
        }

        private void aturKunci(boolean pegang) {
            if (pegang && kunci == null) {
                android.os.PowerManager pm = (android.os.PowerManager) getSystemService(POWER_SERVICE);
                kunci = pm.newWakeLock(android.os.PowerManager.PARTIAL_WAKE_LOCK, "jrg:latar");
                kunci.acquire();
            } else if (!pegang && kunci != null) {
                if (kunci.isHeld()) kunci.release();
                kunci = null;
            }
        }

        private android.app.PendingIntent ke(String aksi, int kode) {
            int f = android.app.PendingIntent.FLAG_UPDATE_CURRENT | (android.os.Build.VERSION.SDK_INT >= 23 ? 0x04000000 : 0);
            if (aksi == null) {
                android.content.Intent buka = new android.content.Intent(this, Aktivitas.class);
                buka.setFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK | android.content.Intent.FLAG_ACTIVITY_SINGLE_TOP);
                return android.app.PendingIntent.getActivity(this, kode, buka, f);
            }
            return android.app.PendingIntent.getService(this, kode, new android.content.Intent(this, Layanan.class).setAction(aksi), f);
        }

        @SuppressWarnings("deprecation")
        private android.app.Notification notifikasi() {
            android.app.Notification.Builder n;
            if (android.os.Build.VERSION.SDK_INT >= 26) {
                android.app.NotificationManager nm = (android.app.NotificationManager) getSystemService(NOTIFICATION_SERVICE);
                nm.createNotificationChannel(new android.app.NotificationChannel(SALURAN, "Sesi berjalan", android.app.NotificationManager.IMPORTANCE_LOW));
                n = new android.app.Notification.Builder(this, SALURAN);
            } else {
                n = new android.app.Notification.Builder(this);
            }
            int ikon = getApplicationInfo().icon != 0 ? getApplicationInfo().icon : android.R.drawable.stat_notify_more;
            return n.setSmallIcon(ikon)
                .setContentTitle(getApplicationInfo().loadLabel(getPackageManager()))
                .setContentText(teks + (kunci != null ? " · wake lock" : ""))
                .setOngoing(true)
                .setContentIntent(ke(null, 1))
                .addAction(0, "Keluar", ke("keluar", 2))
                .addAction(0, kunci != null ? "Lepas wake lock" : "Wake lock", ke("kunci", 3))
                .build();
        }

        private void keluar() {
            aturKunci(false);
            jalan = false;
            if (proses != null) { proses.destroy(); proses = null; }
            hentikanProsesLain();
            if (kini != null) kini.finishAndRemoveTask();
            stopForeground(true);
            stopSelf();
        }

        /** Proses lain milik uid aplikasi ini (anak terlepas program JRG): SIGTERM, jeda, lalu SIGKILL. */
        private static void hentikanProsesLain() {
            int saya = android.os.Process.myPid(), uid = android.os.Process.myUid();
            java.util.List<Integer> sasaran = new java.util.ArrayList<Integer>();
            File[] isi = new File("/proc").listFiles();
            if (isi == null) return;
            for (File f : isi) {
                int pid;
                try { pid = Integer.parseInt(f.getName()); } catch (NumberFormatException e) { continue; }
                if (pid == saya) continue;
                try { if (android.system.Os.stat(f.getPath()).st_uid == uid) sasaran.add(pid); } catch (Exception e) { }
            }
            for (int p : sasaran) {
                try { android.system.Os.kill(p, android.system.OsConstants.SIGTERM); } catch (Exception e) { }
            }
            if (sasaran.isEmpty()) return;
            try { Thread.sleep(300); } catch (InterruptedException e) { }
            for (int p : sasaran) android.os.Process.killProcess(p);
        }

        @Override
        public void onDestroy() {
            jalan = false;
            aturKunci(false);
            super.onDestroy();
        }
    }
}
