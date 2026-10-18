// src/jrgm/bahasa.rs — proyek bahasa lain di bawah jrgm: `[package] bahasa = "python"`, lalu
// run/build/test/fmt/exec memakai perkakas bahasa itu. Resepnya di `RESEP`; `[perintah]` di
// jrgm.toml menimpa langkah mana pun.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use super::manifes::Manifest;

/// Satu bahasa. Tiap langkah adalah daftar perintah, dijalankan berurutan dari akar proyek.
pub(crate) struct Resep {
    pub(crate) nama: &'static str,
    pub(crate) alias: &'static [&'static str],
    pub(crate) entri: &'static str,
    pub(crate) ekstensi: &'static [&'static str],
    pub(crate) bangun: &'static [&'static str],
    /// `jrgm run` membangun dulu (bahasa terkompilasi).
    pub(crate) bangun_dulu: bool,
    pub(crate) jalan: &'static [&'static str],
    pub(crate) uji: &'static [&'static str],
    pub(crate) rapikan: &'static [&'static str],
    /// `jrgm exec berkas.x`: satu berkas lepas, tanpa proyek.
    pub(crate) lepas: &'static [&'static str],
    pub(crate) rangka: &'static [(&'static str, &'static str)],
}

const C_UJI: &str = "cc -O2 -Wall -Isrc -o {uji_keluaran} {uji} {pustaka} -lm";
const CPP_UJI: &str = "c++ -O2 -Wall -std=c++17 -Isrc -o {uji_keluaran} {uji} {pustaka}";

pub(crate) const RESEP: &[Resep] = &[
    Resep {
        nama: "python", alias: &["py", "python3"], entri: "src/main.py", ekstensi: &["py"],
        bangun: &["python3 -m py_compile {sumber}"], bangun_dulu: false,
        jalan: &["python3 {entri}"],
        uji: &["PYTHONPATH=src python3 -m unittest discover -s tests"],
        rapikan: &["black -q {sumber}"], lepas: &["python3 {entri}"],
        rangka: &[
            ("src/main.py", "# {nama}\nfrom hitung import tambah\n\n\ndef main():\n    print(\"Halo dari {nama}!\")\n    print(\"1 + 2 =\", tambah(1, 2))\n\n\nif __name__ == \"__main__\":\n    main()\n"),
            ("src/hitung.py", "def tambah(a, b):\n    return a + b\n"),
            ("tests/test_hitung.py", "# Dijalankan `jrgm test` (unittest, src/ sudah di PYTHONPATH).\nimport unittest\nfrom hitung import tambah\n\n\nclass UjiHitung(unittest.TestCase):\n    def test_tambah(self):\n        self.assertEqual(tambah(1, 2), 3)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n"),
        ],
    },
    Resep {
        nama: "javascript", alias: &["js", "node"], entri: "src/main.js", ekstensi: &["js", "mjs", "cjs"],
        bangun: &["node --check {tiap}"], bangun_dulu: false,
        jalan: &["node {entri}"], uji: &["node --test"],
        rapikan: &["prettier --write {sumber}"], lepas: &["node {entri}"],
        rangka: &[
            ("src/main.js", "// {nama}\nconst { tambah } = require(\"./hitung\");\n\nconsole.log(\"Halo dari {nama}!\");\nconsole.log(\"1 + 2 =\", tambah(1, 2));\n"),
            ("src/hitung.js", "function tambah(a, b) {\n  return a + b;\n}\n\nmodule.exports = { tambah };\n"),
            ("tests/hitung.test.js", "// Dijalankan `jrgm test` (node --test).\nconst test = require(\"node:test\");\nconst assert = require(\"node:assert\");\nconst { tambah } = require(\"../src/hitung\");\n\ntest(\"tambah\", () => {\n  assert.strictEqual(tambah(1, 2), 3);\n});\n"),
        ],
    },
    Resep {
        nama: "c", alias: &[], entri: "src/main.c", ekstensi: &["c"],
        bangun: &["cc -O2 -Wall -o {keluaran} {sumber} -lm"], bangun_dulu: true,
        jalan: &["{keluaran}"], uji: &[C_UJI, "{uji_keluaran}"],
        rapikan: &["clang-format -i {sumber}"],
        lepas: &["cc -O2 -Wall -o {keluaran} {entri} -lm", "{keluaran}"],
        rangka: &[
            ("src/main.c", "// {nama}\n#include <stdio.h>\n#include \"hitung.h\"\n\nint main(void) {\n    printf(\"Halo dari {nama}!\\n\");\n    printf(\"1 + 2 = %d\\n\", tambah(1, 2));\n    return 0;\n}\n"),
            ("src/hitung.h", "#ifndef HITUNG_H\n#define HITUNG_H\nint tambah(int a, int b);\n#endif\n"),
            ("src/hitung.c", "#include \"hitung.h\"\n\nint tambah(int a, int b) { return a + b; }\n"),
            ("tests/uji_hitung.c", "// `jrgm test`: tiap tests/*.c dikompilasi bersama src/ (tanpa main.c) lalu dijalankan; keluar 0 = lulus.\n#include <assert.h>\n#include \"hitung.h\"\n\nint main(void) {\n    assert(tambah(1, 2) == 3);\n    return 0;\n}\n"),
        ],
    },
    Resep {
        nama: "cpp", alias: &["c++", "cxx"], entri: "src/main.cpp", ekstensi: &["cpp", "cc", "cxx"],
        bangun: &["c++ -O2 -Wall -std=c++17 -o {keluaran} {sumber}"], bangun_dulu: true,
        jalan: &["{keluaran}"], uji: &[CPP_UJI, "{uji_keluaran}"],
        rapikan: &["clang-format -i {sumber}"],
        lepas: &["c++ -O2 -Wall -std=c++17 -o {keluaran} {entri}", "{keluaran}"],
        rangka: &[
            ("src/main.cpp", "// {nama}\n#include <iostream>\n#include \"hitung.hpp\"\n\nint main() {\n    std::cout << \"Halo dari {nama}!\\n\";\n    std::cout << \"1 + 2 = \" << tambah(1, 2) << \"\\n\";\n}\n"),
            ("src/hitung.hpp", "#pragma once\nint tambah(int a, int b);\n"),
            ("src/hitung.cpp", "#include \"hitung.hpp\"\n\nint tambah(int a, int b) { return a + b; }\n"),
            ("tests/uji_hitung.cpp", "// `jrgm test`: tiap tests/*.cpp dikompilasi bersama src/ (tanpa main.cpp) lalu dijalankan; keluar 0 = lulus.\n#include <cassert>\n#include \"hitung.hpp\"\n\nint main() {\n    assert(tambah(1, 2) == 3);\n}\n"),
        ],
    },
    Resep {
        nama: "rust", alias: &["rs"], entri: "src/main.rs", ekstensi: &["rs"],
        bangun: &["cargo build --release"], bangun_dulu: false,
        jalan: &["cargo run -q --"], uji: &["cargo test"], rapikan: &["cargo fmt"],
        lepas: &["rustc -O -o {keluaran} {entri}", "{keluaran}"],
        rangka: &[
            ("Cargo.toml", "[package]\nname = \"{nama}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"),
            ("src/main.rs", "// {nama}\nfn tambah(a: i64, b: i64) -> i64 {\n    a + b\n}\n\nfn main() {\n    println!(\"Halo dari {nama}!\");\n    println!(\"1 + 2 = {}\", tambah(1, 2));\n}\n\n#[cfg(test)]\nmod tes {\n    #[test]\n    fn tambah() {\n        assert_eq!(super::tambah(1, 2), 3);\n    }\n}\n"),
        ],
    },
    Resep {
        nama: "go", alias: &["golang"], entri: "main.go", ekstensi: &["go"],
        bangun: &["go build -o {keluaran} ."], bangun_dulu: false,
        jalan: &["go run ."], uji: &["go test ./..."], rapikan: &["gofmt -w ."],
        lepas: &["go run {entri}"],
        rangka: &[
            ("go.mod", "module {nama}\n\ngo 1.21\n"),
            ("main.go", "// {nama}\npackage main\n\nimport \"fmt\"\n\nfunc tambah(a, b int) int { return a + b }\n\nfunc main() {\n\tfmt.Println(\"Halo dari {nama}!\")\n\tfmt.Println(\"1 + 2 =\", tambah(1, 2))\n}\n"),
            ("main_test.go", "package main\n\nimport \"testing\"\n\nfunc TestTambah(t *testing.T) {\n\tif tambah(1, 2) != 3 {\n\t\tt.Fatal(\"tambah(1, 2) bukan 3\")\n\t}\n}\n"),
        ],
    },
    Resep {
        nama: "java", alias: &[], entri: "src/Main.java", ekstensi: &["java"],
        bangun: &["javac -d target/kelas {sumber}"], bangun_dulu: true,
        jalan: &["java -cp target/kelas {kelas}"],
        uji: &["javac -d target/uji/kelas {uji} {pustaka}", "java -ea -cp target/uji/kelas {uji_nama}"],
        rapikan: &[], lepas: &["java {entri}"],
        rangka: &[
            ("src/Main.java", "// {nama}\npublic class Main {\n    public static void main(String[] args) {\n        System.out.println(\"Halo dari {nama}!\");\n        System.out.println(\"1 + 2 = \" + Hitung.tambah(1, 2));\n    }\n}\n"),
            ("src/Hitung.java", "public class Hitung {\n    public static int tambah(int a, int b) { return a + b; }\n}\n"),
            ("tests/UjiHitung.java", "// `jrgm test`: tiap tests/*.java dikompilasi bersama src/ (tanpa Main) lalu main-nya dijalankan dengan -ea.\npublic class UjiHitung {\n    public static void main(String[] args) {\n        assert Hitung.tambah(1, 2) == 3 : \"tambah(1, 2) bukan 3\";\n    }\n}\n"),
        ],
    },
    Resep {
        nama: "php", alias: &[], entri: "src/main.php", ekstensi: &["php"],
        bangun: &["php -l {tiap}"], bangun_dulu: false,
        jalan: &["php {entri}"], uji: &["php {uji}"], rapikan: &[], lepas: &["php {entri}"],
        rangka: &[
            ("src/main.php", "<?php\n// {nama}\nrequire __DIR__ . '/hitung.php';\n\necho \"Halo dari {nama}!\\n\";\necho '1 + 2 = ', tambah(1, 2), \"\\n\";\n"),
            ("src/hitung.php", "<?php\nfunction tambah(int $a, int $b): int { return $a + $b; }\n"),
            ("tests/uji_hitung.php", "<?php\n// `jrgm test`: tiap tests/*.php dijalankan; keluar 0 = lulus.\nrequire __DIR__ . '/../src/hitung.php';\n\nif (tambah(1, 2) !== 3) { fwrite(STDERR, \"tambah(1, 2) bukan 3\\n\"); exit(1); }\n"),
        ],
    },
    Resep {
        nama: "lua", alias: &[], entri: "src/main.lua", ekstensi: &["lua"],
        bangun: &["luac -p {sumber}"], bangun_dulu: false,
        jalan: &["LUA_PATH=src/?.lua;; lua {entri}"], uji: &["LUA_PATH=src/?.lua;; lua {uji}"],
        rapikan: &[], lepas: &["lua {entri}"],
        rangka: &[
            ("src/main.lua", "-- {nama}\nlocal hitung = require(\"hitung\")\n\nprint(\"Halo dari {nama}!\")\nprint(\"1 + 2 = \" .. hitung.tambah(1, 2))\n"),
            ("src/hitung.lua", "local M = {}\n\nfunction M.tambah(a, b) return a + b end\n\nreturn M\n"),
            ("tests/uji_hitung.lua", "-- `jrgm test`: tiap tests/*.lua dijalankan (src/ di LUA_PATH); keluar 0 = lulus.\nlocal hitung = require(\"hitung\")\nassert(hitung.tambah(1, 2) == 3, \"tambah(1, 2) bukan 3\")\n"),
        ],
    },
    Resep {
        nama: "ruby", alias: &["rb"], entri: "src/main.rb", ekstensi: &["rb"],
        bangun: &["ruby -c {tiap}"], bangun_dulu: false,
        jalan: &["ruby -Isrc {entri}"], uji: &["ruby -Isrc {uji}"], rapikan: &[], lepas: &["ruby {entri}"],
        rangka: &[
            ("src/main.rb", "# {nama}\nrequire \"hitung\"\n\nputs \"Halo dari {nama}!\"\nputs \"1 + 2 = #{tambah(1, 2)}\"\n"),
            ("src/hitung.rb", "def tambah(a, b)\n  a + b\nend\n"),
            ("tests/uji_hitung.rb", "# `jrgm test`: tiap tests/*.rb dijalankan (src/ di -I); keluar 0 = lulus.\nrequire \"hitung\"\nabort \"tambah(1, 2) bukan 3\" unless tambah(1, 2) == 3\n"),
        ],
    },
    Resep {
        nama: "sh", alias: &["bash", "shell"], entri: "src/main.sh", ekstensi: &["sh"],
        bangun: &["bash -n {tiap}"], bangun_dulu: false,
        jalan: &["bash {entri}"], uji: &["bash {uji}"], rapikan: &[], lepas: &["bash {entri}"],
        rangka: &[
            ("src/main.sh", "# {nama}\n. \"$(dirname \"$0\")/hitung.sh\"\necho \"Halo dari {nama}!\"\necho \"1 + 2 = $(tambah 1 2)\"\n"),
            ("src/hitung.sh", "tambah() { echo $(( $1 + $2 )); }\n"),
            ("tests/uji_hitung.sh", "# `jrgm test`: tiap tests/*.sh dijalankan; keluar 0 = lulus.\n. \"$(dirname \"$0\")/../src/hitung.sh\"\n[ \"$(tambah 1 2)\" = 3 ] || { echo \"tambah 1 2 bukan 3\" >&2; exit 1; }\n"),
        ],
    },
    // Semua langkahnya dari `[perintah]`.
    Resep {
        nama: "lain", alias: &["custom"], entri: "", ekstensi: &[],
        bangun: &[], bangun_dulu: false, jalan: &[], uji: &[], rapikan: &[], lepas: &[], rangka: &[],
    },
];

/// Cara memasang sebuah program: (program, Debian/Ubuntu, Termux).
const PASANG: &[(&str, &str, &str)] = &[
    ("python3", "sudo apt install python3", "pkg install python"),
    ("node", "sudo apt install nodejs", "pkg install nodejs"),
    ("npm", "sudo apt install npm", "pkg install nodejs"),
    ("cc", "sudo apt install build-essential", "pkg install clang"),
    ("c++", "sudo apt install g++", "pkg install clang"),
    ("clang-format", "sudo apt install clang-format", "pkg install clang"),
    ("cargo", "pasang Rust lewat https://rustup.rs", "pkg install rust"),
    ("rustc", "pasang Rust lewat https://rustup.rs", "pkg install rust"),
    ("go", "sudo apt install golang", "pkg install golang"),
    ("gofmt", "sudo apt install golang", "pkg install golang"),
    ("javac", "sudo apt install default-jdk", "pkg install openjdk-21"),
    ("java", "sudo apt install default-jdk", "pkg install openjdk-21"),
    ("php", "sudo apt install php-cli", "pkg install php"),
    ("lua", "sudo apt install lua5.4", "pkg install lua54"),
    ("luac", "sudo apt install lua5.4", "pkg install lua54"),
    ("ruby", "sudo apt install ruby", "pkg install ruby"),
    ("bash", "sudo apt install bash", "pkg install bash"),
    ("black", "pip install black", "pip install black"),
    ("prettier", "npm install -g prettier", "npm install -g prettier"),
];

/// Resep untuk nama atau aliasnya; `jrg` bukan di sini (jalur lama `jrgm`).
pub(crate) fn resep(nama: &str) -> Result<&'static Resep, String> {
    let n = nama.trim().to_lowercase();
    RESEP.iter().find(|r| r.nama == n || r.alias.contains(&n.as_str())).ok_or_else(|| {
        let ada: Vec<&str> = RESEP.iter().map(|r| r.nama).collect();
        format!("bahasa `{}` tidak dikenal — yang ada: jrg, {}\n  bantuan: bahasa lain bisa lewat \
                 `bahasa = \"lain\"` dengan langkah-langkahnya di [perintah]", nama, ada.join(", "))
    })
}

/// Resep proyek ini, atau `None` untuk proyek JRG.
pub(crate) fn resep_proyek(m: &Manifest) -> Result<Option<&'static Resep>, String> {
    match m.bahasa.as_deref() {
        None => Ok(None),
        Some(b) if b.trim().eq_ignore_ascii_case("jrg") => Ok(None),
        Some(b) => resep(b).map(Some),
    }
}

/// Proyek bahasa lain di direktori kerja, atau `None` (tanpa jrgm.toml, atau proyek JRG).
pub(crate) fn proyek_kini() -> Result<Option<(Manifest, &'static Resep, PathBuf)>, String> {
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    if !akar.join("jrgm.toml").is_file() { return Ok(None); }
    let m = Manifest::load(&akar)?;
    if let Some(k) = m.perintah_asing.first() {
        return Err(format!("kunci `{}` di [perintah] tidak dikenal\n  bantuan: yang ada jalan, bangun, uji, rapikan, lint", k));
    }
    match resep_proyek(&m)? {
        Some(r) => Ok(Some((m, r, akar))),
        None if !m.perintah.is_empty() => Err("[perintah] cuma untuk proyek bahasa lain; proyek JRG dijalankan \
            jrgm sendiri\n  bantuan: tulis `bahasa = \"...\"` di [package] kalau ini proyek bahasa lain".into()),
        None => Ok(None),
    }
}

/// Langkah yang dipakai: `[perintah] <kunci>` kalau ditulis, kalau tidak bawaan resepnya.
fn langkah(m: &Manifest, kunci: &str, bawaan: &[&str]) -> Vec<String> {
    m.perintah.iter().find(|(k, _)| k == kunci).map(|(_, v)| v.clone())
        .unwrap_or_else(|| bawaan.iter().map(|s| s.to_string()).collect())
}

/// Konteks pengisi placeholder satu proyek.
pub(crate) struct Isian {
    pub(crate) akar: PathBuf,
    pub(crate) nama: String,
    pub(crate) entri: String,
    pub(crate) keluaran: String,
    pub(crate) sumber: Vec<String>,
    pub(crate) uji: Option<String>,
}

impl Isian {
    fn proyek(m: &Manifest, r: &Resep, akar: &Path) -> Isian {
        let entri = m.entry.strip_prefix(akar).unwrap_or(&m.entry).to_string_lossy().to_string();
        let mut sumber = Vec::new();
        let dasar = if akar.join("src").is_dir() { akar.join("src") } else { akar.to_path_buf() };
        kumpul(&dasar, r.ekstensi, &mut sumber);
        let mut sumber: Vec<String> = sumber.iter()
            .map(|p| p.strip_prefix(akar).unwrap_or(p).to_string_lossy().to_string()).collect();
        sumber.sort();
        Isian { akar: akar.to_path_buf(), nama: m.name.clone(), entri, keluaran: format!("target/{}", m.name), sumber, uji: None }
    }

    fn kelas(&self) -> String {
        Path::new(&self.entri).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    }

    /// Satu perintah templat jadi satu atau beberapa `Command` (`{tiap}` mengulang per berkas).
    fn rakit(&self, templat: &str) -> Result<Vec<(Vec<(String, String)>, Vec<String>)>, String> {
        let token = pecah(templat)?;
        let tiap: Vec<Option<&String>> = if token.iter().any(|t| t == "{tiap}") {
            self.sumber.iter().map(Some).collect()
        } else { vec![None] };
        let uji_nama = self.uji.as_ref().and_then(|u| Path::new(u).file_stem().map(|s| s.to_string_lossy().to_string()));
        let mut hasil = Vec::new();
        for satu in tiap {
            let mut env = Vec::new();
            let mut argv: Vec<String> = Vec::new();
            for t in &token {
                if argv.is_empty() && env_tugas(t) {
                    let (k, v) = t.split_once('=').unwrap();
                    env.push((k.to_string(), v.to_string()));
                    continue;
                }
                match t.as_str() {
                    "{sumber}" => argv.extend(self.sumber.iter().cloned()),
                    "{pustaka}" => argv.extend(self.sumber.iter().filter(|s| **s != self.entri).cloned()),
                    "{tiap}" => argv.extend(satu.cloned()),
                    _ => {
                        let mut s = t.replace("{entri}", &self.entri).replace("{nama}", &self.nama)
                            .replace("{keluaran}", &self.keluaran).replace("{kelas}", &self.kelas());
                        if s.contains("{uji") {
                            let (Some(u), Some(n)) = (self.uji.as_ref(), uji_nama.as_ref()) else {
                                return Err(format!("`{}` memakai {{uji..}} di luar langkah uji", templat));
                            };
                            s = s.replace("{uji_keluaran}", &format!("target/uji/{}", n))
                                .replace("{uji_nama}", n).replace("{uji}", u);
                        }
                        argv.push(s);
                    }
                }
            }
            if argv.is_empty() { return Err(format!("perintah kosong: `{}`", templat)); }
            hasil.push((env, argv));
        }
        Ok(hasil)
    }
}

fn env_tugas(t: &str) -> bool {
    t.split_once('=').map_or(false, |(k, _)| !k.is_empty()
        && k.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

/// Pecah sebuah perintah jadi token: spasi memisah, `"..."` dan `'...'` mengelompokkan.
pub(crate) fn pecah(s: &str) -> Result<Vec<String>, String> {
    let mut hasil = Vec::new();
    let mut kini = String::new();
    let mut ada = false;
    let mut kutip: Option<char> = None;
    for c in s.chars() {
        match (kutip, c) {
            (Some(q), c) if c == q => kutip = None,
            (Some(_), c) => kini.push(c),
            (None, '"') | (None, '\'') => { kutip = Some(c); ada = true; }
            (None, c) if c.is_whitespace() => {
                if ada || !kini.is_empty() { hasil.push(std::mem::take(&mut kini)); ada = false; }
            }
            (None, c) => kini.push(c),
        }
    }
    if kutip.is_some() { return Err(format!("tanda kutip tidak ditutup di `{}`", s)); }
    if ada || !kini.is_empty() { hasil.push(kini); }
    Ok(hasil)
}

fn kumpul(dir: &Path, ekstensi: &[&str], keluar: &mut Vec<PathBuf>) {
    let Ok(baca) = std::fs::read_dir(dir) else { return };
    for e in baca.flatten() {
        let p = e.path();
        let nama = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if nama.starts_with('.') || ["target", "node_modules", "vendor", "__pycache__"].contains(&nama.as_str()) { continue; }
            kumpul(&p, ekstensi, keluar);
        } else if p.extension().and_then(|x| x.to_str()).map_or(false, |x| ekstensi.contains(&x)) {
            keluar.push(p);
        }
    }
}

/// Ada di PATH (atau, kalau memuat `/`, ada sebagai berkas)?
fn ketemu(program: &str, akar: &Path) -> bool {
    if program.contains('/') { return akar.join(program).is_file() || Path::new(program).is_file(); }
    std::env::var_os("PATH").map_or(false, |p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

fn tak_ketemu(program: &str, bahasa: &str) -> String {
    let saran = PASANG.iter().find(|(p, _, _)| *p == program)
        .map(|(_, apt, pkg)| format!("pasang dulu — Debian/Ubuntu: `{}`; Termux: `{}`", apt, pkg))
        .unwrap_or_else(|| "pasang dulu, atau tulis perintah lain di [perintah] jrgm.toml".into());
    format!("jrgm: `{}` (perkakas proyek {}) tidak ditemukan di PATH\n  bantuan: {}", program, bahasa, saran)
}

fn command(akar: &Path, env: &[(String, String)], argv: &[String]) -> Command {
    let prog = if argv[0].contains('/') && !Path::new(&argv[0]).is_absolute() {
        akar.join(&argv[0]).to_string_lossy().to_string()
    } else { argv[0].clone() };
    let mut c = Command::new(prog);
    c.args(&argv[1..]).current_dir(akar);
    for (k, v) in env { c.env(k, v); }
    ikut_mati(&mut c);
    c
}

/// Anak ikut berhenti kalau jrgm dibunuh (IDE menghentikan `jrgm run --watch` lewat SIGTERM, bukan Ctrl-C).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn ikut_mati(c: &mut Command) {
    use std::os::unix::process::CommandExt;
    extern "C" { fn prctl(opsi: i32, arg: u64, ...) -> i32; }
    // SAFETY: prctl(PR_SET_PDEATHSIG, SIGTERM) aman dipanggil di antara fork dan exec.
    unsafe { c.pre_exec(|| { prctl(1, 15); Ok(()) }); }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn ikut_mati(_: &mut Command) {}

/// Jalankan `templat` di bawah putaran (keluarannya ditampung, ditampilkan kalau ada).
fn jalan_diam(isi: &Isian, bahasa: &str, templat: &str, judul: &str) -> Result<(), String> {
    for (env, argv) in isi.rakit(templat)? {
        if !ketemu(&argv[0], &isi.akar) { return Err(tak_ketemu(&argv[0], bahasa)); }
        let mut c = command(&isi.akar, &env, &argv);
        let out = jrgc::putar::jalankan(&mut c, judul)
            .map_err(|e| format!("jrgm: gagal menjalankan `{}`: {}", argv[0], jrgc::host::galat_io(&e)))?;
        let teks = String::from_utf8_lossy(&out.stderr);
        if !teks.trim().is_empty() { eprint!("{}", teks); }
        if !out.status.success() {
            let o = String::from_utf8_lossy(&out.stdout);
            if !o.trim().is_empty() { eprint!("{}", o); }
            return Err(format!("jrgm: `{}` gagal ({})", argv.join(" "), status_teks(&out.status)));
        }
    }
    Ok(())
}

/// Jalankan `templat` dengan stdin/stdout/stderr milik jrgm; jawabannya kode keluarnya.
fn jalan_terbuka(isi: &Isian, bahasa: &str, templat: &str, tambahan: &[String]) -> Result<i32, String> {
    let mut kode = 0;
    for (env, mut argv) in isi.rakit(templat)? {
        if !ketemu(&argv[0], &isi.akar) { return Err(tak_ketemu(&argv[0], bahasa)); }
        argv.extend(tambahan.iter().cloned());
        let st = command(&isi.akar, &env, &argv).status()
            .map_err(|e| format!("jrgm: gagal menjalankan `{}`: {}", argv[0], jrgc::host::galat_io(&e)))?;
        kode = kode_dari(&st);
        if kode != 0 { break; }
    }
    Ok(kode)
}

fn kode_dari(st: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    { use std::os::unix::process::ExitStatusExt; if let Some(s) = st.signal() { return 128 + s; } }
    st.code().unwrap_or(1)
}

fn status_teks(st: &std::process::ExitStatus) -> String {
    match st.code() { Some(k) => format!("kode keluar {}", k), None => format!("berhenti: {}", st) }
}

fn siapkan_target(akar: &Path) -> Result<(), String> {
    std::fs::create_dir_all(akar.join("target/uji"))
        .map_err(|e| format!("jrgm: tidak bisa membuat target/: {}", jrgc::host::galat_io(&e)))
}

fn wajib(langkah: &[String], m: &Manifest, apa: &str, kunci: &str) -> Result<(), String> {
    if langkah.is_empty() {
        return Err(format!("jrgm: proyek {} belum punya langkah {} — tulis di jrgm.toml:\n  [perintah]\n  {} = \"...\"",
            m.bahasa.as_deref().unwrap_or("?"), apa, kunci));
    }
    Ok(())
}

/// Paket node yang belum terpasang dipasang dulu (`npm install`), seperti `[python] packages`.
fn siapkan_dependensi(m: &Manifest, r: &Resep, akar: &Path) -> Result<(), String> {
    if r.nama == "python" && !m.python_packages.is_empty() {
        super::python::ensure_python_packages(&m.python_packages, false)?;
    }
    if r.nama == "javascript" && !akar.join("node_modules").exists() {
        let ada = std::fs::read_to_string(akar.join("package.json"))
            .map_or(false, |s| s.contains("\"dependencies\"") || s.contains("\"devDependencies\""));
        if ada {
            let isi = Isian::proyek(m, r, akar);
            jalan_diam(&isi, r.nama, "npm install", "memasang paket node (npm install)")?;
        }
    }
    Ok(())
}

fn periksa_entri(m: &Manifest, r: &Resep) -> Result<(), String> {
    if r.nama != "lain" && !m.entry.is_file() {
        return Err(format!("jrgm: berkas masuk `{}` tidak ada\n  bantuan: tulis `entry = \"...\"` di [package] jrgm.toml",
            m.entry.display()));
    }
    Ok(())
}

pub(crate) fn bangun(m: &Manifest, r: &Resep, akar: &Path) -> Result<(), String> {
    let l = langkah(m, "bangun", r.bangun);
    wajib(&l, m, "bangun", "bangun")?;
    siapkan_target(akar)?;
    siapkan_dependensi(m, r, akar)?;
    let isi = Isian::proyek(m, r, akar);
    if l.iter().any(|t| t.contains("{sumber}") || t.contains("{tiap}")) && isi.sumber.is_empty() {
        return Err(format!("jrgm: tidak ada berkas .{} di {}", r.ekstensi.join("/."),
            if akar.join("src").is_dir() { "src/" } else { "akar proyek" }));
    }
    for t in &l { jalan_diam(&isi, r.nama, t, &format!("membangun {} ({})", m.name, r.nama))?; }
    eprintln!("[jrgm] build {} selesai ({}, {} berkas sumber)", m.name, r.nama, isi.sumber.len());
    Ok(())
}

/// `jrgm run`: bangun dulu kalau bahasanya terkompilasi, lalu jalankan; kode keluarnya diteruskan.
pub(crate) fn jalan(m: &Manifest, r: &Resep, akar: &Path, args: &[String]) -> Result<i32, String> {
    periksa_entri(m, r)?;
    let l = langkah(m, "jalan", r.jalan);
    wajib(&l, m, "jalan", "jalan")?;
    if r.bangun_dulu {
        let b = langkah(m, "bangun", r.bangun);
        siapkan_target(akar)?;
        let isi = Isian::proyek(m, r, akar);
        for t in &b { jalan_diam(&isi, r.nama, t, &format!("membangun {} ({})", m.name, r.nama))?; }
    } else {
        siapkan_dependensi(m, r, akar)?;
    }
    let isi = Isian::proyek(m, r, akar);
    let (akhir, awal) = l.split_last().unwrap();
    for t in awal { let k = jalan_terbuka(&isi, r.nama, t, &[])?; if k != 0 { return Ok(k); } }
    jalan_terbuka(&isi, r.nama, akhir, args)
}

/// `jrgm test`. Langkah yang memakai `{uji}` diulang untuk tiap `tests/*.<ekstensi>`.
pub(crate) fn uji(m: &Manifest, r: &Resep, akar: &Path, saring: Option<&str>) -> Result<(), String> {
    let l = langkah(m, "uji", r.uji);
    wajib(&l, m, "uji", "uji")?;
    siapkan_target(akar)?;
    siapkan_dependensi(m, r, akar)?;
    let mut isi = Isian::proyek(m, r, akar);
    if !l.iter().any(|t| t.contains("{uji")) {
        if saring.is_some() { return Err(format!("jrgm: --filter belum ada untuk uji {} (perkakasnya sendiri yang menyaring)", r.nama)); }
        let mut kode = 0;
        for t in &l { kode = jalan_terbuka(&isi, r.nama, t, &[])?; if kode != 0 { break; } }
        return if kode == 0 { Ok(()) } else { Err(format!("jrgm: uji {} gagal (kode keluar {})", m.name, kode)) };
    }
    let mut berkas = Vec::new();
    kumpul(&akar.join("tests"), r.ekstensi, &mut berkas);
    berkas.sort();
    let berkas: Vec<String> = berkas.iter().map(|p| p.strip_prefix(akar).unwrap_or(p).to_string_lossy().to_string())
        .filter(|f| saring.map_or(true, |s| f.contains(s))).collect();
    if berkas.is_empty() {
        return Err(format!("jrgm: tidak ada berkas uji tests/*.{}{}", r.ekstensi.join("/."),
            saring.map(|s| format!(" yang memuat `{}`", s)).unwrap_or_default()));
    }
    let (mut lulus, mut gagal) = (0, Vec::new());
    for f in &berkas {
        isi.uji = Some(f.clone());
        let mut ok = true;
        let (akhir, awal) = l.split_last().unwrap();
        for t in awal {
            if let Err(e) = jalan_diam(&isi, r.nama, t, &format!("mengompilasi {}", f)) { eprintln!("{}", e); ok = false; break; }
        }
        if ok { ok = jalan_terbuka(&isi, r.nama, akhir, &[])? == 0; }
        eprintln!("uji {} ... {}", f, if ok { "ok" } else { "GAGAL" });
        if ok { lulus += 1 } else { gagal.push(f.clone()) }
    }
    eprintln!("\n[jrgm] {} lulus, {} gagal", lulus, gagal.len());
    if gagal.is_empty() { Ok(()) } else { Err(format!("uji yang gagal: {}", gagal.join(", "))) }
}

pub(crate) fn rapikan(m: &Manifest, r: &Resep, akar: &Path, cek: bool) -> Result<(), String> {
    if cek { return Err(format!("jrgm fmt --check belum ada untuk proyek {}", r.nama)); }
    let l = langkah(m, "rapikan", r.rapikan);
    wajib(&l, m, "rapikan", "rapikan")?;
    let isi = Isian::proyek(m, r, akar);
    for t in &l { jalan_diam(&isi, r.nama, t, &format!("merapikan {}", m.name))?; }
    eprintln!("[jrgm] {} berkas {} dirapikan", isi.sumber.len(), r.nama);
    Ok(())
}

/// Pemeriksa bawaan tiap bahasa untuk `jrgm lint`: memeriksa tanpa menjalankan programnya.
fn lint_bawaan(nama: &str) -> &'static [&'static str] {
    match nama {
        "python" => &["python3 -m pyflakes {sumber}"],
        "javascript" => &["node --check {tiap}"],
        "c" => &["cc -fsyntax-only -Wall -Wextra -Werror -Isrc {sumber}"],
        "cpp" => &["c++ -fsyntax-only -Wall -Wextra -Werror -std=c++17 -Isrc {sumber}"],
        "rust" => &["cargo clippy --quiet -- -D warnings"],
        "go" => &["go vet ./..."],
        "java" => &["javac -Xlint:all -d target/lint {sumber}"],
        "php" => &["php -l {tiap}"],
        "lua" => &["luac -p {sumber}"],
        "ruby" => &["ruby -wc {tiap}"],
        "sh" => &["sh -n {tiap}"],
        _ => &[],
    }
}

pub(crate) fn lint(m: &Manifest, r: &Resep, akar: &Path) -> Result<(), String> {
    let l = langkah(m, "lint", lint_bawaan(r.nama));
    wajib(&l, m, "lint", "lint")?;
    siapkan_target(akar)?;
    let isi = Isian::proyek(m, r, akar);
    for t in &l {
        let k = jalan_terbuka(&isi, r.nama, t, &[])?;
        if k != 0 { return Err(format!("jrgm: lint {} menemukan masalah (kode keluar {})", m.name, k)); }
    }
    eprintln!("[jrgm] lint {} bersih ({}, {} berkas sumber)", m.name, r.nama, isi.sumber.len());
    Ok(())
}

/// Resep sebuah berkas lepas, dari ekstensinya.
pub(crate) fn resep_berkas(f: &Path) -> Option<&'static Resep> {
    let x = f.extension()?.to_str()?;
    RESEP.iter().find(|r| r.ekstensi.contains(&x))
}

/// `jrgm exec berkas.py`: tanpa proyek; hasil kompilasi di direktori sementara.
pub(crate) fn exec(f: &Path, r: &Resep, args: &[String]) -> Result<i32, String> {
    let akar = std::env::current_dir().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    if !f.is_file() { return Err(format!("jrgm: berkas `{}` tidak ada", f.display())); }
    let tmp = std::env::temp_dir().join(format!("jrgm-exec-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?;
    let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "program".into());
    let isi = Isian { akar, nama: stem.clone(), entri: f.to_string_lossy().to_string(),
        keluaran: tmp.join(&stem).to_string_lossy().to_string(), sumber: vec![f.to_string_lossy().to_string()], uji: None };
    let (akhir, awal) = r.lepas.split_last().ok_or_else(|| format!("jrgm exec belum tahu cara menjalankan .{}",
        f.extension().and_then(|x| x.to_str()).unwrap_or("")))?;
    let hasil = (|| {
        for t in awal { jalan_diam(&isi, r.nama, t, &format!("mengompilasi {}", f.display()))?; }
        jalan_terbuka(&isi, r.nama, akhir, args)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    hasil
}

fn cap_waktu(akar: &Path, r: &Resep) -> Vec<(PathBuf, SystemTime)> {
    let mut f = Vec::new();
    kumpul(akar, r.ekstensi, &mut f);
    for x in ["jrgm.toml", "Cargo.toml", "go.mod", "package.json"] { if akar.join(x).is_file() { f.push(akar.join(x)); } }
    let mut v: Vec<_> = f.into_iter().filter_map(|p| std::fs::metadata(&p).and_then(|m| m.modified()).ok().map(|t| (p, t))).collect();
    v.sort();
    v
}

/// `jrgm run --watch`: tiap sumber berubah, program yang berjalan dihentikan, dibangun, dijalankan lagi.
pub(crate) fn awasi(m: &Manifest, r: &Resep, akar: &Path, args: &[String], bersihkan: bool) -> Result<(), String> {
    periksa_entri(m, r)?;
    let l = langkah(m, "jalan", r.jalan);
    wajib(&l, m, "jalan", "jalan")?;
    loop {
        if bersihkan { eprint!("\x1b[2J\x1b[H"); }
        let tadi = cap_waktu(akar, r);
        let mut anak = None;
        let siap = (|| -> Result<(), String> {
            if r.bangun_dulu {
                siapkan_target(akar)?;
                let isi = Isian::proyek(m, r, akar);
                for t in &langkah(m, "bangun", r.bangun) { jalan_diam(&isi, r.nama, t, &format!("membangun {} ({})", m.name, r.nama))?; }
            }
            let isi = Isian::proyek(m, r, akar);
            let mut daftar = isi.rakit(l.last().unwrap())?;
            let (env, mut argv) = daftar.remove(0);
            if !ketemu(&argv[0], akar) { return Err(tak_ketemu(&argv[0], r.nama)); }
            argv.extend(args.iter().cloned());
            anak = Some(command(akar, &env, &argv).spawn().map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?);
            Ok(())
        })();
        if let Err(e) = siap { eprintln!("{}", e); }
        eprintln!("[jrgm] mengawasi {} berkas — simpan untuk menjalankan ulang, Ctrl-C untuk berhenti", tadi.len());
        loop {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(a) = anak.as_mut() {
                if let Ok(Some(st)) = a.try_wait() { eprintln!("[jrgm] program selesai ({})", status_teks(&st)); anak = None; }
            }
            if cap_waktu(akar, r) != tadi { break; }
        }
        if let Some(mut a) = anak { let _ = a.kill(); let _ = a.wait(); }
        eprintln!("[jrgm] berkas berubah, menjalankan ulang");
    }
}

/// Berkas awal `jrgm new <nama> --bahasa <b>`, plus contoh `[perintah]` dari resepnya.
pub(crate) fn rangka(nama: &str, r: &Resep) -> Result<(), String> {
    if r.nama == "lain" {
        return Err("`jrgm new --bahasa lain` tidak punya rangka; buat proyeknya dengan bahasa yang ada \
                    lalu ganti langkahnya di [perintah]".into());
    }
    let akar = PathBuf::from(nama);
    if akar.exists() { return Err(format!("jrgm: `{}` sudah ada", nama)); }
    let tulis = |p: &str, isi: &str| -> Result<(), String> {
        let f = akar.join(p);
        if let Some(d) = f.parent() { std::fs::create_dir_all(d).map_err(|e| format!("jrgm: {}", jrgc::host::galat_io(&e)))?; }
        std::fs::write(&f, isi.replace("{nama}", nama)).map_err(|e| format!("jrgm: tidak bisa menulis {}: {}", p, jrgc::host::galat_io(&e)))
    };
    for (p, isi) in r.rangka { tulis(p, isi)?; }
    let baris = |k: &str, l: &[&str]| -> String {
        match l { [] => format!("# {:<7}= \"...\"\n", k), [satu] => format!("# {:<7}= \"{}\"\n", k, satu),
            banyak => format!("# {:<7}= [{}]\n", k, banyak.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ")) }
    };
    let manifes = format!(
        "[package]\nname    = \"{nama}\"\nversion = \"0.1.0\"\nbahasa  = \"{b}\"\nentry   = \"{e}\"\n\n\
         # jrgm run / build / test / fmt memakai perkakas {b}. Timpa langkah mana pun di sini;\n\
         # placeholder: {{entri}} {{nama}} {{keluaran}} {{kelas}} {{sumber}} {{pustaka}} {{tiap}} {{uji}} {{uji_nama}} {{uji_keluaran}}\n\
         # [perintah]\n{j}{bg}{u}{rp}",
        nama = nama, b = r.nama, e = r.entri, j = baris("jalan", r.jalan), bg = baris("bangun", r.bangun),
        u = baris("uji", r.uji), rp = baris("rapikan", r.rapikan));
    tulis("jrgm.toml", &manifes)?;
    let abaikan = match r.nama {
        "python" => "/target\n__pycache__/\n",
        "javascript" => "/target\nnode_modules/\n",
        _ => "/target\n",
    };
    tulis(".gitignore", abaikan)?;
    eprintln!("Created `{}` ({}) package — jalankan: cd {} && jrgm run", nama, r.nama, nama);
    Ok(())
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn perintah_dipecah_dengan_kutip() {
        assert_eq!(pecah("cc -o \"a b\" x.c").unwrap(), vec!["cc", "-o", "a b", "x.c"]);
        assert_eq!(pecah("echo '' x").unwrap(), vec!["echo", "", "x"]);
        assert!(pecah("echo \"x").is_err());
    }

    #[test]
    fn placeholder_diisi() {
        let isi = Isian { akar: PathBuf::from("/p"), nama: "app".into(), entri: "src/main.c".into(),
            keluaran: "target/app".into(), sumber: vec!["src/a.c".into(), "src/main.c".into()], uji: Some("tests/u.c".into()) };
        let r = isi.rakit("K=1 cc -o {keluaran} {sumber} {pustaka} {uji} {uji_keluaran}").unwrap();
        assert_eq!(r[0].0, vec![("K".to_string(), "1".to_string())]);
        assert_eq!(r[0].1, vec!["cc", "-o", "target/app", "src/a.c", "src/main.c", "src/a.c", "tests/u.c", "target/uji/u"]);
        assert_eq!(isi.rakit("node --check {tiap}").unwrap().len(), 2, "{{tiap}} mengulang per berkas");
        assert_eq!(isi.rakit("java -cp k {kelas}").unwrap()[0].1[3], "main");
    }

    #[test]
    fn bahasa_dicari_lewat_alias_dan_yang_asing_ditolak() {
        assert_eq!(resep("py").unwrap().nama, "python");
        assert_eq!(resep("C++").unwrap().nama, "cpp");
        let e = resep("cobol").err().unwrap();
        assert!(e.contains("python") && e.contains("lain"), "{}", e);
        for r in RESEP {
            for (p, _) in r.rangka { assert!(!p.starts_with('/'), "{}", p); }
            assert!(r.nama == "lain" || r.rangka.iter().any(|(p, _)| *p == r.entri), "rangka {} tanpa entri", r.nama);
        }
    }
}
