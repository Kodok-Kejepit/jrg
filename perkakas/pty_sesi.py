#!/usr/bin/env python3
"""Menjalankan sebuah program di dalam PTY sungguhan dan mengetik ke dalamnya.

Ada karena penyuntingan baris hanya bisa diuji lewat terminal sungguhan: mode
raw, urutan escape, dan penempatan kursor semuanya tidak ada di dalam pipa.
Rust di sini tidak punya crate PTY (dan proyek ini tidak menambah crate), jadi
pengemudinya di luar — bentuk yang sama dengan `py_runner.py`.

Pemakaian:  pty_sesi.py <program> [arg...]   <  skrip
Skrip, satu arahan per baris:
    tunggu <ms>         jeda, lalu tiriskan apa pun yang sudah keluar
    sampai <ms> <teks>  tunggu sampai keluaran memuat <teks>, paling lama <ms>
    diam <ms> <batas>   tunggu sampai tak ada byte baru selama <ms>, maks <batas>
    kirim <hex>         tulis byte-byte itu ke terminal
    akhir <ms>          tunggu programnya selesai, lalu catat `[pty keluar=<kode>]`
    tutup <ms>          tutup terminalnya (seperti menutup jendela), lalu seperti `akhir`
    mode                catat mode terminalnya: `[pty icanon=1 echo=1 isig=1]`
    kolom <n>       lebar terminal (harus sebelum arahan lain)
    baris <n>       tinggi terminal
    ukuran <kol> <bar>  ubah ukuran jendela di tengah sesi (programnya menerima SIGWINCH)
Keluarannya byte PTY apa adanya di stdout, supaya pemanggilnya yang memutuskan
apakah escape-nya penting.
"""
import os, sys, pty, time, select, fcntl, termios, struct

def ukuran(fd, kolom, baris):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", baris, kolom, 0, 0))

def main():
    if len(sys.argv) < 2:
        sys.stderr.write("pty_sesi.py: butuh nama program\n"); return 2
    skrip = sys.stdin.read().splitlines()
    kolom, baris = 80, 24
    for l in skrip:
        k = l.split()
        if len(k) == 2 and k[0] == "kolom": kolom = int(k[1])
        if len(k) == 2 and k[0] == "baris": baris = int(k[1])

    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm"
        os.environ["COLUMNS"] = str(kolom)
        os.environ["LINES"] = str(baris)
        try:
            os.execv(sys.argv[1], sys.argv[1:])
        except OSError as e:
            sys.stderr.write("exec gagal: %s\n" % e)
            os._exit(127)

    ukuran(fd, kolom, baris)
    keluar = bytearray()
    status = [None]
    terbuka = [True]

    def catat_keluar():
        if status[0] is None:
            kode = "belum"
        elif os.WIFEXITED(status[0]):
            kode = str(os.WEXITSTATUS(status[0]))
        else:
            kode = "sinyal%d" % os.WTERMSIG(status[0])
        keluar.extend(("\n[pty keluar=%s]\n" % kode).encode())

    def tuai():
        try:
            p, st = os.waitpid(pid, os.WNOHANG)
        except (ChildProcessError, OSError):
            return True
        if p == pid:
            status[0] = st
        return status[0] is not None

    def tiris(detik):
        batas = time.time() + detik
        while time.time() < batas:
            sisa = max(0.0, batas - time.time())
            if not select.select([fd], [], [], sisa)[0]:
                continue
            try:
                b = os.read(fd, 1 << 20)
            except OSError:
                return
            if not b:
                return
            keluar.extend(b)

    for l in skrip:
        k = l.split(None, 1)
        if not k: continue
        if k[0] == "tunggu":
            tiris(int(k[1]) / 1000.0)
        elif k[0] == "sampai":
            batas_ms, penanda = k[1].split(None, 1)
            akhir = time.time() + int(batas_ms) / 1000.0
            while time.time() < akhir:
                tiris(0.05)
                if penanda.encode() in bytes(keluar):
                    break
        elif k[0] == "diam":
            hening_ms, batas_ms = k[1].split()
            hening = int(hening_ms) / 1000.0
            akhir = time.time() + int(batas_ms) / 1000.0
            terakhir = time.time()
            n = len(keluar)
            while time.time() < akhir:
                tiris(0.03)
                if len(keluar) != n:
                    n = len(keluar)
                    terakhir = time.time()
                elif time.time() - terakhir >= hening:
                    break
        elif k[0] == "kirim":
            try:
                os.write(fd, bytes.fromhex(k[1].strip()))
            except OSError:
                break
            tiris(0.05)
        elif k[0] == "ukuran":
            kb = k[1].split()
            ukuran(fd, int(kb[0]), int(kb[1]))
            tiris(0.05)
        elif k[0] == "akhir":
            akhir = time.time() + int(k[1]) / 1000.0
            while time.time() < akhir:
                tiris(0.03)
                if tuai():
                    break
            tiris(0.1)
            catat_keluar()
        elif k[0] == "tutup":
            # Sisi master ditutup: bagi programnya terminalnya PUTUS, dan kernel mengirim
            # SIGHUP. Yang diukur: apakah program yang selamat dari sinyal itu berhenti sendiri.
            tiris(0.1)
            try: os.close(fd)
            except OSError: pass
            terbuka[0] = False
            akhir = time.time() + int(k[1]) / 1000.0
            while time.time() < akhir:
                if tuai():
                    break
                time.sleep(0.03)
            catat_keluar()
            break
        elif k[0] == "mode":
            # Dibaca dari sisi master: yang dijawab adalah mode yang DITINGGALKAN programnya.
            try:
                l = termios.tcgetattr(fd)[3]
                keluar.extend(("\n[pty icanon=%d echo=%d isig=%d]\n" % (
                    bool(l & termios.ICANON), bool(l & termios.ECHO), bool(l & termios.ISIG))).encode())
            except termios.error as e:
                keluar.extend(("\n[pty mode tidak terbaca: %s]\n" % e).encode())

    if terbuka[0]:
        tiris(0.3)
        try: os.close(fd)
        except OSError: pass
    # Proses anaknya dimatikan, bukan ditunggu: sebuah editor yang menolak
    # keluar karena ada perubahan belum tersimpan memang TIDAK akan selesai,
    # dan itu justru salah satu yang diuji.
    if status[0] is None:
        try: os.kill(pid, 9)
        except OSError: pass
        try: os.waitpid(pid, 0)
        except (ChildProcessError, OSError): pass

    sys.stdout.buffer.write(bytes(keluar))
    sys.stdout.buffer.flush()
    return 0

if __name__ == "__main__":
    sys.exit(main())
