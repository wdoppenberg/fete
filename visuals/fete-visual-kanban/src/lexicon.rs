//! What the signs say.
//!
//! Every sign in the frame carries one entry from [`WORDS`] — a real word, in
//! real characters, of the kind that is actually lit up somewhere in Tokyo
//! tonight. Nothing here is invented and nothing is decorative: the list is
//! drink, food, baths, pachinko, place names, the two or three phrases a
//! shopfront uses to say it is open, and the vocabulary of 酉の市 itself.
//!
//! This module is the single source of truth for both halves of the visual.
//! The shader indexes glyphs by their position in [`glyphs()`], and
//! `tools/glyph-atlas` draws the atlas in exactly that order — so adding a word
//! here means regenerating the atlas, and the test at the bottom of this file
//! fails until that happens.
//!
//! Deliberately free of Bevy: `tools/glyph-atlas` includes this file directly.

/// One sign's worth of text.
pub struct Word {
    /// What the sign says. One to [`MAX_CHARS`] characters — the length of a
    /// column that still reads across a dark room.
    pub text: &'static str,
    /// The reading, and what it means. Kept beside the text so the list can be
    /// checked by someone who does not read the script.
    pub gloss: &'static str,
    /// How many draw slots this takes, and so how often it comes up against
    /// the rest of the list.
    pub weight: u8,
}

const fn word(text: &'static str, gloss: &'static str) -> Word {
    Word {
        text,
        gloss,
        weight: 1,
    }
}

/// The longest sign the layout draws. A column of five is taller than the
/// cell it has to fit in and comes out too small to read.
pub const MAX_CHARS: usize = 4;

/// The vocabulary.
///
/// Weighted entries are the exception rather than the rule — the field reads
/// as a street because no one message dominates it. 酉の市 is the one thing
/// here the night is actually about, so it carries the only weight above one:
/// often enough that it comes round several times a minute somewhere in the
/// frame, rare enough that it is still a thing you notice rather than the
/// wallpaper.
pub const WORDS: &[Word] = &[
    // --- 酉の市 ------------------------------------------------------------
    Word {
        text: "酉の市",
        gloss: "Tori no Ichi — the Festival of the Rooster",
        weight: 4,
    },
    word("熊手", "kumade — the lucky rake sold at the festival"),
    word("開運", "kaiun — opening of fortune"),
    word("商売繁盛", "shobai hanjo — thriving trade"),
    word("大吉", "daikichi — great luck"),
    word("祭", "matsuri — festival"),
    // --- drink -------------------------------------------------------------
    word("居酒屋", "izakaya"),
    word("酒", "sake"),
    word("大衆酒場", "taishu sakaba — a tavern for everyone"),
    word("生ビール", "nama biiru — draught beer"),
    word("焼酎", "shochu"),
    word("立呑み", "tachinomi — standing bar"),
    word("バー", "baa — bar"),
    word("スナック", "sunakku — hostess bar"),
    // --- food --------------------------------------------------------------
    word("ラーメン", "raamen"),
    word("中華そば", "chuka soba — the older name for ramen"),
    word("餃子", "gyoza"),
    word("焼鳥", "yakitori"),
    word("焼肉", "yakiniku"),
    word("寿司", "sushi"),
    word("天ぷら", "tempura"),
    word("とんかつ", "tonkatsu"),
    word("うどん", "udon"),
    word("そば", "soba"),
    word("おでん", "oden"),
    word("カレー", "karee — curry"),
    word("定食", "teishoku — set meal"),
    word("丼", "donburi — a rice bowl"),
    word("麺", "men — noodles"),
    word("大盛", "omori — large portion"),
    word("氷", "kori — ice, the shaved-ice sign"),
    // --- the rest of the street --------------------------------------------
    word("喫茶", "kissa — coffee house"),
    word("珈琲", "kohii — coffee, in the old characters"),
    word("コーヒー", "kohii — coffee"),
    word("カラオケ", "karaoke"),
    word("パチンコ", "pachinko"),
    word("サウナ", "sauna"),
    word("ホテル", "hoteru — hotel"),
    word("銭湯", "sento — public bath"),
    word("ゆ", "yu — hot water, the character on a bathhouse curtain"),
    word("温泉", "onsen — hot spring"),
    word("質屋", "shichiya — pawnshop"),
    word("薬", "kusuri — medicine, a pharmacy"),
    word("電気", "denki — electrical goods"),
    word("たばこ", "tabako — tobacco"),
    word("横丁", "yokocho — the alley"),
    // --- what a shopfront says about itself ---------------------------------
    word("営業中", "eigyochu — open"),
    word("年中無休", "nenju mukyu — open every day of the year"),
    word("深夜営業", "shinya eigyo — open late"),
    word("一番", "ichiban — the best"),
    word("元祖", "ganso — the original"),
    // --- where ---------------------------------------------------------------
    word("東京", "Tokyo"),
    word("新宿", "Shinjuku"),
    word("渋谷", "Shibuya"),
    word("上野", "Ueno"),
    word("浅草", "Asakusa"),
    word("銀座", "Ginza"),
    word("歌舞伎町", "Kabukicho"),
];

/// The long vowel mark. Written across in horizontal text and down the column
/// in vertical text, which is the one character in the list whose *shape*
/// depends on how the sign is set — so the shader turns it a quarter turn and
/// needs to know which glyph to do that to.
pub const CHOONPU: char = 'ー';

/// Every character used by [`WORDS`], in first-appearance order, deduplicated.
///
/// This ordering *is* the atlas layout. Cell `n` of the atlas holds
/// `glyphs()[n]`, counting left to right and then down.
pub fn glyphs() -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for word in WORDS {
        for ch in word.text.chars() {
            if !out.contains(&ch) {
                out.push(ch);
            }
        }
    }
    out
}

/// Index of `ch` in the atlas, or `None` if it is not in the vocabulary.
pub fn glyph_index(ch: char) -> Option<usize> {
    glyphs().iter().position(|&g| g == ch)
}

// --- the atlas ---------------------------------------------------------------

/// Side of one atlas cell, in pixels.
///
/// The signed distance field is smooth, so this is not the resolution the
/// characters are drawn at — it is the resolution the *distance* is sampled
/// at, and a sign three times this size on screen still has clean edges. It is
/// the small strokes inside a dense character — 繁, 舞 — that set the floor.
pub const ATLAS_CELL: u32 = 128;

/// Cells across the atlas. Rows follow from the vocabulary size.
pub const ATLAS_COLS: u32 = 12;

/// How much of a cell the character's em square covers.
///
/// The rest is margin, and the margin is not padding: it is where the distance
/// field lives. A character drawn edge to edge in its cell has nowhere to
/// record *how far away* it is, and the glow around it would stop dead at the
/// cell boundary.
pub const ATLAS_EM: f32 = 0.62;

/// The distance stored at full black and full white, in em.
///
/// Wide enough that the whole cell is inside the range — the encoding never
/// clips, so the field keeps falling away right to the corner and the halo has
/// no square edge in it anywhere.
pub const ATLAS_RANGE: f32 = 0.8;

/// Rows the atlas needs, and its size in pixels.
pub fn atlas_rows() -> u32 {
    glyphs().len().div_ceil(ATLAS_COLS as usize) as u32
}

/// `(width, height)` of the atlas image, in pixels.
pub fn atlas_size() -> (u32, u32) {
    (ATLAS_COLS * ATLAS_CELL, atlas_rows() * ATLAS_CELL)
}

// --- the draw table ----------------------------------------------------------

/// Room in the uniform for the weight-expanded vocabulary. Must match
/// `MAX_SLOTS` in `kanban.wgsl`.
pub const MAX_SLOTS: usize = 128;

/// The vocabulary as the shader sees it: one row per draw slot, holding up to
/// [`MAX_CHARS`] glyph indices with `-1.0` for the unused tail.
///
/// Weights are expanded here rather than in the shader. A sign picks a row at
/// random and a weighted word is simply a word occupying several rows, which
/// costs the shader nothing — no table of cumulative probabilities to walk,
/// no second lookup.
pub fn draw_slots() -> Vec<[f32; MAX_CHARS]> {
    let glyphs = glyphs();
    let mut slots = Vec::new();

    for word in WORDS {
        let mut row = [-1.0f32; MAX_CHARS];
        let chars: Vec<char> = word.text.chars().collect();
        assert!(
            chars.len() <= MAX_CHARS,
            "`{}` is {} characters; the layout draws at most {MAX_CHARS}",
            word.text,
            chars.len(),
        );
        for (i, ch) in chars.iter().enumerate() {
            let index = glyphs
                .iter()
                .position(|g| g == ch)
                .expect("every character of every word is in `glyphs()`");
            row[i] = index as f32;
        }
        for _ in 0..word.weight.max(1) {
            slots.push(row);
        }
    }

    assert!(
        slots.len() <= MAX_SLOTS,
        "the vocabulary expands to {} draw slots; the uniform holds {MAX_SLOTS}",
        slots.len(),
    );
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_fits_and_packs() {
        // Panics on a word too long or a vocabulary too large for the uniform.
        let slots = draw_slots();
        assert_eq!(
            slots.len(),
            WORDS
                .iter()
                .map(|w| w.weight.max(1) as usize)
                .sum::<usize>(),
        );
    }

    #[test]
    fn the_long_vowel_mark_is_in_the_vocabulary() {
        // The shader turns this glyph a quarter turn in vertical text. If the
        // vocabulary ever loses it the index goes to -1, which the shader reads
        // as "no character here" rather than as a mistake.
        assert!(glyph_index(CHOONPU).is_some());
    }

    /// The atlas is generated, committed, and read back by the shader by
    /// position, so a vocabulary change that has not been followed by a re-run
    /// of `tools/glyph-atlas` silently draws the wrong characters — a word
    /// spelled out of whatever happens to sit at those indices now.
    ///
    /// The tool writes the character list beside the atlas for exactly this.
    #[test]
    fn the_committed_atlas_matches_the_vocabulary() {
        let stale = "`src/glyphs.png` is stale — re-run `cargo run -p fete-glyph-atlas`";

        let index = include_str!("glyphs.txt");
        let baked = index
            .lines()
            .find(|line| !line.starts_with('#'))
            .expect("the character index has a character line");
        assert_eq!(baked.chars().collect::<Vec<_>>(), glyphs(), "{stale}");

        // And that the image really holds a cell for each of them. `IHDR` puts
        // the dimensions at a fixed offset, so this needs no image decoder.
        let png = include_bytes!("glyphs.png");
        let dim = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
        assert_eq!((dim(16), dim(20)), atlas_size(), "{stale}");
    }
}
