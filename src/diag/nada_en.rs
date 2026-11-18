// src/diag/nada_en.rs — sapaan nada dalam bahasa Inggris; aturannya sama dengan tabel Indonesia di nada.rs
//
// Tingkat pertama selalu serius. Menggoda boleh, merendahkan tidak. `{}` diisi "on line N" atau "around there".

use super::nada::TabelNada;

pub(crate) const GALAK: TabelNada = TabelNada {
    kompilasi: [
        &[
            "something is not right {}. the details are below.",
            "it does not compile yet, stuck {}. the explanation follows.",
            "found one problem {}. the details are below.",
            "stopped {} — this is what is blocking it, in full below.",
        ],
        &[
            "oh, {} again. i only blinked for a second.",
            "{} again? we are like two people on the same bus every morning.",
            "back {} again. i have not moved from that spot since yesterday.",
            "did you read the last one? {} it says the same thing, letter for letter.",
            "it has not moved an inch {}.",
            "i am not angry. it is just... {} again. okay.",
        ],
        &[
            "hey. {}. i know that line number like my own house number by now.",
            "really? {} again. i typed that explanation below for a reason.",
            "wait, {} STILL? the part below is not decoration.",
            "seriously, {}? my laptop fan is starting to spin out of frustration.",
            "oh no, {}. i have been waiting, and it sounds like a broken record.",
            "{} again... fine, come here, we will read it together, word by word.",
        ],
        &[
            "HEY, HEEEY!!! {}! i am shouting from inside the CPU! fix it first!",
            "STOP. {}. i will not be quiet until this is done. read. the. part. below.",
            "OH MY, {} AGAIN?! i know its spacing by heart. let me spell it out.",
            "that is enough. {}. i am not angry, i am disappointed... no, i am angry.",
            "{}!!! i have run out of ways to point at it. it is right below, in big letters.",
            "HEY! {}. fine, i will stay with you until it works. but please read it, just once.",
        ],
    ],
    jalan: [
        &[
            "the program stopped {}. the reason is below.",
            "it hit a wall {} while running. the explanation is below.",
            "the run got stuck {} — here is what happened, in full below.",
            "stopped {}. i will show you what went wrong, below.",
        ],
        &[
            "oh, {} again? i thought we had already shaken hands.",
            "hmm, {} again. something has not been touched yet, it seems.",
            "we meet again {}. i am getting to know this line personally.",
            "hey, {} is back. are you sure it was fixed, or just saved?",
            "{} again. check the part below — really read it, not skim it.",
            "i am not bored. but this is {} again, you know.",
        ],
        &[
            "hey, {} STILL. this is a few rounds now.",
            "oh dear, {}. i have said it until my voice went hoarse.",
            "seriously, {} again? i am getting worked up on my own.",
            "hey! {}. the part below is for reading, not for staring at.",
            "{} again... i do not know which letters to use anymore.",
            "oh, {}. come on, you are one tiny step away from done.",
        ],
        &[
            "HEY! YOU, CODING? {} again! if you are not into it, i can write it myself.",
            "HEY, {}!!! i am yelling now. READ. THE. PART. BELOW.",
            "GOODNESS, {} AGAIN. i am this close to grabbing the keyboard.",
            "okay, {}. i will not stop nagging until this is sorted.",
            "{}!!! okay, fine. i am with you. but this time, really read it.",
            "STOP FOR A MOMENT. {}. sit. breathe. now read the part below, slowly.",
        ],
    ],
};

pub(crate) const JUDES: TabelNada = TabelNada {
    kompilasi: [
        &[
            "there is a problem {}. the details are below.",
            "{} is not done yet. below. read it.",
            "it failed to compile, the source is {} — the explanation is below.",
            "stopped {}. the explanation is below.",
        ],
        &[
            "{} again. i am not going to clap.",
            "exactly like before. {}. nothing moved.",
            "this is the second time, {}. i am counting.",
            "{}. was the last one read or skipped?",
        ],
        &[
            "{}. again. and again. i will not change my sentence.",
            "the part below is not wallpaper. {}.",
            "{} — several times now, and it has not moved an inch.",
            "i put the explanation below so it would be read. {}.",
        ],
        &[
            "{}. fine. i will be quiet. the part below is clearer now.",
            "whatever. {}. the explanation is still below.",
            "{}. i do not get tired. i am a program.",
            "which time is this? {}. fine, i added more help while i was at it.",
        ],
    ],
    jalan: [
        &[
            "the program stopped {}. the reason is below.",
            "it died {}. the details are below.",
            "it hit a wall {}. here is the explanation.",
            "{} — the run stopped. the reason is below.",
        ],
        &[
            "{} again. seriously?",
            "stopped in the same place. {}. exactly.",
            "{}. the second time, in case you are not counting.",
            "it seems the last one was not read. {}.",
        ],
        &[
            "{}. same place, same error, same result.",
            "i know this one by heart. {}. down to the commas.",
            "{} — several times already. no progress.",
            "fine. {}. i put the explanation below again.",
        ],
        &[
            "{}. i will stop nagging. just read the part below.",
            "this many times, {}. i added a hint while i was at it.",
            "{}. i am not bored, but you should be.",
            "okay. {}. i will read the part below to you, you listen.",
        ],
    ],
};

pub(crate) const HANGAT: TabelNada = TabelNada {
    kompilasi: [
        &[
            "something needs fixing {}. don't worry, the full explanation is below.",
            "it can't run yet because of something {}. let's look at the details below.",
            "found one problem {}. the explanation is below.",
            "stopped {} — nothing big. here are the details.",
        ],
        &[
            "it showed up again {}. that's fine, i skip the same line all the time too.",
            "this one is back, {}. try reading the part below once more.",
            "hmm, {} again. maybe something small was missed. i'm right here with you.",
            "hang in there, {} is still there. this is a normal part of learning, really.",
        ],
        &[
            "there is still a problem {}. honestly, this is normal — let me help again.",
            "a few times now {}. don't give up, let's look at it together.",
            "still {}. take a breath first. really, take a breath.",
            "if it keeps happening {}, maybe my wording was not clear. let me say it again.",
        ],
        &[
            "{} has come up a lot. i added a hint to make it clearer.",
            "i think i'm the one who wasn't clear, not you being slow. {} again.",
            "this one really is stubborn, {}. don't give up, you're close.",
            "i'll stay until it's done. {}. no need to rush, nobody is chasing you.",
        ],
    ],
    jalan: [
        &[
            "the program stopped {}. don't worry, here is why.",
            "it got as far as {} and then got stuck. the explanation is below.",
            "stopped {} — not a failure, it just did not get through yet. here are the details.",
            "something got caught {}. let me show you.",
        ],
        &[
            "stuck again {}. that's okay, let's look once more.",
            "this one is back, {}. try checking the part below.",
            "stopped in the same place, {}. maybe something was missed.",
            "patience, {} again. let's fix it slowly.",
        ],
        &[
            "it is still happening {}. stay calm, i'll help again.",
            "a few times now {}. let's read it again together, from the top.",
            "still stuck {}. this is not a sign you can't do it — you just haven't found it yet.",
            "if it keeps happening {}, maybe an assumption needs a second look.",
        ],
        &[
            "it has happened a lot {}. i added a hint to make it clearer.",
            "i think the explanation was not clear enough, {}. here is more.",
            "this one really is stubborn, {}. don't give up, almost there.",
            "i'll stay with you until it's done, {}. really.",
        ],
    ],
};

pub(crate) const COOL: TabelNada = TabelNada {
    kompilasi: [
        &[
            "{} — something is not quite right. details below.",
            "compilation stopped {}. the details are below.",
            "one problem {}. more below.",
            "not through yet {}. here is the explanation.",
        ],
        &[
            "{}. exactly as before.",
            "{} — the second time.",
            "again {}. nothing has shifted.",
            "{}. same. moving on.",
        ],
        &[
            "{}. several times now.",
            "still {}. the situation has not changed.",
            "{} — a very tidy pattern, unfortunately.",
            "again {}. maybe the assumption needs checking.",
        ],
        &[
            "{}. it keeps coming back. i added a hint below.",
            "again and again {}. extra guidance is below.",
            "{} — it seems my explanation was not enough. here is more.",
            "still {}. let me explain it from another angle.",
        ],
    ],
    jalan: [
        &[
            "stopped {}. the details are below.",
            "the run ended {}. the explanation is below.",
            "one problem while running, {}. more below.",
            "it halted {}. here is what happened.",
        ],
        &[
            "{}. same as before.",
            "{} — the second time.",
            "stopped in the same place, {}.",
            "{}. nothing has changed.",
        ],
        &[
            "{}. several times now.",
            "still {}. same place, same cause.",
            "{} — consistent, at least.",
            "again {}. maybe an assumption needs checking.",
        ],
        &[
            "{}. it keeps coming back. i added a hint below.",
            "again and again {}. extra guidance is below.",
            "{} — let me explain it from another angle.",
            "still {}. here is more help.",
        ],
    ],
};

pub(crate) const LAWAK: TabelNada = TabelNada {
    kompilasi: [
        &[
            "something is wrong {}. the details are below.",
            "it does not compile yet, the problem is {}. read below.",
            "found a problem {}. the explanation is below.",
            "stopped {}. here is what is in the way.",
        ],
        &[
            "{} again. we really have to stop meeting like this.",
            "{}? i thought we had already passed that checkpoint.",
            "twice now {}. it is becoming a habit.",
            "{} again. at least it is consistent.",
        ],
        &[
            "{} again. this program is more consistent than my gym plans.",
            "that line and i are pen pals now. {}.",
            "{}. this is not a bug anymore, it is a family tradition. read below.",
            "{} again and again. i know that line number like a licence plate.",
        ],
        &[
            "{}!!! okay, seriously now. a real hint is below.",
            "i am out of jokes, {}. here is extra help.",
            "{}. fine, i will stop joking and start helping for real.",
            "{} again. i will explain it from another angle below.",
        ],
    ],
    jalan: [
        &[
            "the program stopped {}. the reason is below.",
            "it ran into something {}. the details are below.",
            "the run ended {}. here is why.",
            "stopped {}. this is what happened.",
        ],
        &[
            "{} again. we really have to stop meeting like this.",
            "{}? i thought we had already passed that checkpoint.",
            "stopped {} twice now. it is becoming a habit.",
            "{} again. at least it is consistent.",
        ],
        &[
            "{} again. this program is more consistent than my gym plans.",
            "that line and i are pen pals now. {}.",
            "{}. this is not a bug anymore, it is a family tradition. read below.",
            "{} again and again. i know that line number like a licence plate.",
        ],
        &[
            "{}!!! okay, seriously now. a real hint is below.",
            "i am out of jokes, {}. here is extra help.",
            "{}. fine, i will stop joking and start helping for real.",
            "{} again. i will explain it from another angle below.",
        ],
    ],
};

pub(crate) const BANTUAN_TAMBAHAN: &[&str] = &[
    "really: read the `help:` line above slowly, one word at a time. the answer is usually already there.",
    "show the code to someone else, or read it aloud. a second pair of eyes (or your own ears) often spots \
     what your own eyes skipped.",
    "if you are still stuck: delete the line with the error, make sure the rest runs, then write it again \
     slowly. faster than guessing.",
    "the `^` below points EXACTLY at the part that is wrong. start there, not at the beginning of the line.",
];

#[cfg(test)]
mod tests {
    use super::*;

    const SEMUA: [(&str, &TabelNada); 5] =
        [("galak", &GALAK), ("judes", &JUDES), ("hangat", &HANGAT), ("cool", &COOL), ("lawak", &LAWAK)];

    /// Aturan yang sama dengan tabel Indonesia: kemunculan pertama serius dan menunjuk penjelasannya.
    #[test]
    fn tingkat_pertama_inggris_serius_dan_menunjuk_penjelasan() {
        for (nama, t) in SEMUA {
            for pool in [t.kompilasi[0], t.jalan[0]] {
                assert!(!pool.is_empty(), "{nama}: tingkat pertama kosong");
                for f in pool {
                    assert!(!f.contains("!!!") && !f.contains("HEY") && f.to_lowercase() == *f,
                        "{nama}: bercanda atau berteriak di kemunculan pertama: {f:?}");
                    assert!(["below", "details", "explanation", "reason", "here is", "show you", "what happened"]
                        .iter().any(|m| f.contains(m)), "{nama}: tidak menunjuk penjelasan: {f:?}");
                }
            }
        }
    }

    /// Tiap frasa satu `{}`, dan sesudah diisi tidak ada kata dobel ("on on line").
    #[test]
    fn frasa_inggris_terbaca_benar_sesudah_diisi() {
        for (nama, t) in SEMUA {
            for pool in t.kompilasi.iter().chain(t.jalan.iter()) {
                for f in pool.iter() {
                    assert_eq!(f.matches("{}").count(), 1, "{nama}: {f:?}");
                    // Cuma kata dobel yang LAHIR dari isiannya ("on on line"), seperti tes tabel Indonesia.
                    let kata = |x: &str| x.to_lowercase().split(|c: char| !c.is_alphanumeric())
                        .filter(|w| !w.is_empty()).map(String::from).collect::<Vec<_>>();
                    let asli = kata(&f.replace("{}", " \u{0} "));
                    for isi in ["on line 59", "around there"] {
                        let h = f.replace("{}", isi);
                        let lahir = kata(&h).windows(2).any(|w| w[0] == w[1]
                            && !asli.windows(2).any(|a| a[0] == w[0] && a[1] == w[1]));
                        assert!(!lahir, "{nama}: kata dobel di {h:?}");
                    }
                }
            }
        }
    }
}
