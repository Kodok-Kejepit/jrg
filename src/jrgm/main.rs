// src/jrgm/main.rs — biner `jrgm`: badannya `jrgc::jrgm` di crate lib, satu salinan dengan `jrgc --jrgm`.

fn main() {
    jrgc::term::pasang_pengait_panik();
    jrgc::jrgm::utama(std::env::args().collect());
}
