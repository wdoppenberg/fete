//! Bakes the glyph atlas Kanban's signs are drawn from.
//!
//! ```sh
//! tools/fetch-font.sh                 # Noto Sans CJK JP, once
//! cargo run -p fete-glyph-atlas       # writes src/glyphs.png and src/glyphs.txt
//! ```
//!
//! Kanban used to compose its characters out of hashed radicals, which meant
//! it could never spell anything. It now says real words, and real words need
//! real letterforms — so the characters come from a real Japanese font, baked
//! once into a signed distance field and committed. A distance field rather
//! than a picture of the characters, because the shader does not want coverage:
//! it wants to know how far the nearest stroke is, which is what makes the
//! tube, the core and the halo, and what lets one 128-pixel cell serve a sign
//! filling a third of the frame and a sign four pixels tall in the same frame.
//!
//! The font is not committed and nothing at runtime needs it. What ships is
//! `glyphs.png`: no outlines, no metrics, no tables — an image of distances.
//!
//! The vocabulary lives in the visual, not here. This tool includes
//! `lexicon.rs` directly rather than depending on the crate, which keeps a
//! build of the atlas from being a build of Bevy.

// Most of it is for the shader rather than for the bake.
#[allow(dead_code)]
#[path = "../../../visuals/fete-visual-kanban/src/lexicon.rs"]
mod lexicon;

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};

use lexicon::{ATLAS_CELL, ATLAS_COLS, ATLAS_EM, ATLAS_RANGE, atlas_rows, atlas_size, glyphs};

/// Where the outline is sampled, relative to the atlas cell.
///
/// The distance field is built from a hard inside/outside mask, so the outline
/// lands on a raster pixel edge rather than where it really is. At four times
/// the cell that error is an eighth of an output pixel, which is well under
/// what the shader's own antialiasing rounds off anyway.
const SUPERSAMPLE: u32 = 4;

/// Where the baseline sits below the top of the ideographic em square, in em.
///
/// Every character in the vocabulary is full-width, so its em square *is* its
/// design box, and centring that box in the cell is what keeps a column of
/// characters evenly spaced. Noto CJK puts the box between +0.88 and -0.12 em
/// about the baseline, which is the usual convention for the script.
const IDEOGRAPHIC_ASCENT: f32 = 0.88;

fn main() {
    let font_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_font_path);

    let data = std::fs::read(&font_path).unwrap_or_else(|err| {
        eprintln!(
            "cannot read {}: {err}\n\nRun tools/fetch-font.sh first, or pass a font: \
             cargo run -p fete-glyph-atlas -- /path/to/font.otf",
            font_path.display()
        );
        std::process::exit(1);
    });
    let font = FontVec::try_from_vec(data).expect("not a font file");

    let chars = glyphs();
    let (width, height) = atlas_size();
    let cell = ATLAS_CELL as usize;
    let mut atlas = vec![0u8; (width * height) as usize];

    for (index, ch) in chars.iter().enumerate() {
        let field = distance_field(&font, *ch);
        let col = index % ATLAS_COLS as usize;
        let row = index / ATLAS_COLS as usize;
        for y in 0..cell {
            let dst = (row * cell + y) * width as usize + col * cell;
            atlas[dst..dst + cell].copy_from_slice(&field[y * cell..(y + 1) * cell]);
        }
    }

    let out = manifest_dir().join("../../visuals/fete-visual-kanban/src");
    write_png(&out.join("glyphs.png"), &atlas, width, height);
    write_index(&out.join("glyphs.txt"), &font_path, &chars);

    println!(
        "{} characters, {}x{} cells of {ATLAS_CELL}px -> {width}x{height}",
        chars.len(),
        ATLAS_COLS,
        atlas_rows(),
    );
}

/// One cell of the atlas: the signed distance to `ch`, encoded so that mid-grey
/// is the outline and white is deep inside the stroke.
fn distance_field(font: &FontVec, ch: char) -> Vec<u8> {
    let cell = ATLAS_CELL as usize;
    let hi = cell * SUPERSAMPLE as usize;
    let em_px = ATLAS_CELL as f32 * ATLAS_EM * SUPERSAMPLE as f32;

    let id = font.glyph_id(ch);
    assert!(
        font.outline(id).is_some() || ch == ' ',
        "the font has no outline for `{ch}`",
    );

    // Calibrate the scale against the character's own advance rather than
    // trusting a pixel size to mean what we want it to. Every character here is
    // full-width, so its advance is exactly one em — measure it at an arbitrary
    // scale and correct.
    let probe = PxScale::from(em_px);
    let advance = font.as_scaled(probe).h_advance(id);
    let scale = PxScale::from(em_px * em_px / advance.max(1e-3));

    // The em square, centred in the cell.
    let top = (hi as f32 - em_px) * 0.5;
    let baseline = top + em_px * IDEOGRAPHIC_ASCENT;

    let mut inside = vec![false; hi * hi];
    if let Some(outlined) = font.outline_glyph(
        id.with_scale_and_position(scale, point((hi as f32 - em_px) * 0.5, baseline)),
    ) {
        let bounds = outlined.px_bounds();
        outlined.draw(|dx, dy, coverage| {
            if coverage < 0.5 {
                return;
            }
            let x = bounds.min.x as i32 + dx as i32;
            let y = bounds.min.y as i32 + dy as i32;
            if x >= 0 && y >= 0 && (x as usize) < hi && (y as usize) < hi {
                inside[y as usize * hi + x as usize] = true;
            }
        });
    }

    // Signed distance in high-resolution pixels: how far out of the character,
    // less how far into it.
    let out = euclidean(&inside, hi, false);
    let ink = euclidean(&inside, hi, true);
    let signed: Vec<f32> = (0..hi * hi).map(|i| out[i] - ink[i]).collect();

    // Down to the cell. Averaging a distance field is safe in a way averaging
    // coverage is not — halfway between two samples really is halfway between
    // two distances.
    let step = SUPERSAMPLE as usize;
    let mut field = vec![0u8; cell * cell];
    for y in 0..cell {
        for x in 0..cell {
            let mut sum = 0.0;
            for sy in 0..step {
                for sx in 0..step {
                    sum += signed[(y * step + sy) * hi + x * step + sx];
                }
            }
            let d_em = sum / (step * step) as f32 / em_px;
            let v = 0.5 - d_em / (2.0 * ATLAS_RANGE);
            field[y * cell + x] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    field
}

/// Exact Euclidean distance from every pixel to the nearest pixel of the other
/// kind, by Felzenszwalb & Huttenlocher's two-pass squared-distance transform.
///
/// `interior` picks which side is being measured: the distance out of the
/// character, or the distance in from its edge.
fn euclidean(mask: &[bool], size: usize, interior: bool) -> Vec<f32> {
    // Far, but not so far that it stops being a number: the envelope below
    // compares sums of this and of squared coordinates, and an f32 holding
    // 1e12 has no resolution left for the coordinates.
    let far = (size * size * 4) as f32;

    let mut grid: Vec<f32> = mask
        .iter()
        .map(|&on| if on == interior { far } else { 0.0 })
        .collect();

    let mut scratch = vec![0.0f32; size];
    // Columns, then rows: the 1d transform applied along each axis in turn is
    // the 2d transform, which is the whole reason this is linear time.
    for pass in 0..2 {
        for i in 0..size {
            for j in 0..size {
                scratch[j] = if pass == 0 {
                    grid[j * size + i]
                } else {
                    grid[i * size + j]
                };
            }
            let line = transform1d(&scratch);
            for j in 0..size {
                if pass == 0 {
                    grid[j * size + i] = line[j];
                } else {
                    grid[i * size + j] = line[j];
                }
            }
        }
    }

    grid.iter().map(|d| d.sqrt()).collect()
}

/// The lower envelope of the parabolas rooted at each sample — the 1d squared
/// distance transform.
fn transform1d(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut out = vec![0.0f32; n];
    // `v` holds the parabolas in the envelope, `z` the boundaries between them.
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f32; n + 1];
    let mut k = 0usize;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;

    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2 * q - 2 * p) as f32;
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }

    let mut k = 0usize;
    for (q, slot) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let d = (q as f32) - (p as f32);
        *slot = d * d + f[p];
    }
    out
}

fn write_png(path: &Path, data: &[u8], width: u32, height: u32) {
    let file = std::fs::File::create(path).expect("cannot write the atlas");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(data)
        .expect("png data");
}

/// The character list, in atlas order, beside the atlas.
///
/// Half provenance and half tripwire: the visual's tests compare this against
/// the vocabulary, so a word added without a re-bake fails the build rather
/// than quietly drawing the wrong characters.
fn write_index(path: &Path, font: &Path, chars: &[char]) {
    let name = font
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let body = format!(
        "# Generated by tools/glyph-atlas from {name}. Do not edit.\n\
         # Cell n of glyphs.png holds character n of this line, left to right\n\
         # then down, {ATLAS_COLS} cells across.\n\
         {}\n",
        chars.iter().collect::<String>(),
    );
    std::fs::write(path, body).expect("cannot write the character index");
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Where `tools/fetch-font.sh` leaves the font.
fn default_font_path() -> PathBuf {
    manifest_dir().join("NotoSansCJKjp-Light.otf")
}
