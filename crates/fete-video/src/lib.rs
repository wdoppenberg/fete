//! `fete-video` — a handful of clips, decoded into one array texture.
//!
//! Every visual in this show is a pure function of time. This crate is the one
//! exception, and it exists for one visual: Terebi's wall of televisions, where
//! a set showing actual footage among sets showing synthesised footage is the
//! whole joke. Nothing else in the set has a frame for it.
//!
//! The shape is deliberately small. `N` slots, each an `ffmpeg` subprocess
//! decoding its own playlist of clips on its own thread, all writing into the
//! layers of one `2d_array` texture that a material binds. There is no seeking,
//! no scrubbing, no transport, no audio and no synchronisation between slots —
//! a wall of televisions wants none of those, and every one of them would be a
//! thing to go wrong at a venue.
//!
//! # Failure is the normal case
//!
//! There is no `ffmpeg` on some machines, no clips in the directory on most,
//! and no directory at all by default. All three are ordinary: the plugin logs
//! one line and installs nothing, [`VideoWall`] is absent, and the visual that
//! wanted it draws the picture it always drew. A show that cannot start because
//! a video file is missing is worse than a show without video.
//!
//! ```ignore
//! app.add_plugins(VideoPlugin::from_dir("video"));
//! ```

pub mod decode;

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use rand::seq::SliceRandom;

use crate::decode::{Slot, frame_bytes};

/// How many independent feeds the wall can be showing at once.
///
/// Every set that is switched on is now showing footage — there are no
/// synthesised programmes left to take up the slack — so this is effectively
/// "how many televisions can be showing different things", and the wall carries
/// somewhere north of twenty cabinets at the usual set size. Twenty-four covers
/// it, dealt by position rather than by hash so the ones that do coincide are
/// never near each other. See `rank` in the shader.
///
/// The decode cost is linear and far smaller than it looks: sixteen `ffmpeg`s
/// at this size measured at 3.6% of one core on an M4 Pro, because `-re` means
/// each one spends nearly all of its time asleep. The texture is the real cost
/// at 7.4 MB, re-uploaded whole on any frame where any layer changed — fine on
/// a desktop GPU, and the first thing to cut on a Pi.
pub const SLOTS: u32 = 24;

/// Decoded frame size, per slot.
///
/// A television on Terebi's wall lands somewhere between a tenth and a third of
/// the screen height, so at 1080p a set is roughly 110 to 320 pixels tall. 240
/// sits at the top of that: near enough 1:1 on the largest sets, mild
/// minification on the smallest. Going higher would only buy detail that the
/// scan lines and the tube curvature immediately take away again.
pub const TILE: UVec2 = UVec2::new(320, 240);

/// The wall's texture and the decoders filling it.
///
/// Present only when there was something to play. A visual should treat its
/// absence as the normal case — see the module docs.
#[derive(Resource)]
pub struct VideoWall {
    /// A `2d_array` of [`SLOTS`] layers, one per decoder.
    pub texture: Handle<Image>,
    /// Which clip file each slot started on, for the log and the HUD.
    pub playlists: Vec<Vec<PathBuf>>,
    slots: Vec<Slot>,
    live: u32,
}

impl VideoWall {
    /// How many layers are holding a picture.
    ///
    /// Zero until the first frames arrive — about a tenth of a second after
    /// startup — so a shader must gate on it rather than on the texture being
    /// bound. The layers themselves are allocated black, which is exactly what
    /// a television showing nothing looks like, so the window is invisible.
    pub fn live_slots(&self) -> u32 {
        self.live
    }
}

/// Adds a video wall, if there is anything to put on it.
pub struct VideoPlugin {
    /// Directory scanned for clips, non-recursively.
    pub dir: PathBuf,
}

impl VideoPlugin {
    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        let clips = scan(&self.dir);
        if clips.is_empty() {
            info!(
                "video: no clips in {} — televisions will play the synthesised channels only",
                self.dir.display()
            );
            return;
        }
        if !decode::ffmpeg_available() {
            warn!(
                "video: found {} clips in {} but no runnable `ffmpeg` — skipping the video channel",
                clips.len(),
                self.dir.display()
            );
            return;
        }

        let wall = build(&mut app.world_mut().resource_mut::<Assets<Image>>(), clips);
        info!(
            "video: {} clips across {} televisions",
            wall.playlists.iter().map(Vec::len).sum::<usize>(),
            wall.playlists.len(),
        );

        app.insert_resource(wall)
            // `First`, ahead of everything: a visual's own systems read
            // `live_slots` in `Update` to decide whether to show video at all,
            // and reading it a frame stale means one frame of a set tuned to a
            // channel that has not arrived yet.
            .add_systems(First, upload_frames);
    }
}

/// Clip files in `dir`, sorted so a given directory always deals the same way.
fn scan(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut clips: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|ext| {
                    let ext = ext.to_ascii_lowercase();
                    // Everything ffmpeg is likely to be handed here. Not a
                    // whitelist of what it can read — it reads far more — but
                    // enough to keep a stray `.DS_Store` or a README out of a
                    // decoder that would then log a failure for it.
                    ["mp4", "mkv", "mov", "avi", "webm", "m4v", "mpg", "mpeg"]
                        .iter()
                        .any(|known| ext == *known)
                })
        })
        .collect();
    clips.sort();
    clips
}

fn build(images: &mut Assets<Image>, clips: Vec<PathBuf>) -> VideoWall {
    let layer = frame_bytes(TILE);
    let mut image = Image::new(
        Extent3d {
            width: TILE.x,
            height: TILE.y,
            depth_or_array_layers: SLOTS,
        },
        TextureDimension::D2,
        vec![0; layer * SLOTS as usize],
        // Srgb, matching how the clips were encoded. The wrong choice here does
        // not look like a bug — it looks like footage that is slightly too
        // bright, which is exactly the kind of thing that gets tuned around for
        // an hour before anyone checks the texture format.
        TextureFormat::Rgba8UnormSrgb,
        // The CPU copy is not a cache — it is where the decoders write, and it
        // is re-uploaded from every frame.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    // Layers are only layers if the view says so.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = ImageSampler::linear();

    // Deal the clips round-robin into per-slot playlists. Shuffled first, so
    // two televisions do not run the same programme in the same order every
    // night, and so a cache holding six commercials and two game shows does not
    // reliably put both game shows on adjacent sets.
    let mut shuffled = clips;
    shuffled.shuffle(&mut rand::rng());

    let mut playlists = vec![Vec::new(); SLOTS as usize];
    let count = playlists.len();
    for (index, clip) in shuffled.into_iter().enumerate() {
        playlists[index % count].push(clip);
    }
    // Fewer clips than slots: the empty ones get nothing and stay black rather
    // than doubling up. A wall where two sets are visibly in lockstep is worse
    // than a wall with fewer video sets on it.
    playlists.retain(|playlist| !playlist.is_empty());

    let slots = playlists
        .iter()
        .map(|playlist| Slot::start(playlist.clone(), TILE))
        .collect();

    VideoWall {
        texture: images.add(image),
        playlists,
        slots,
        live: 0,
    }
}

/// Copies whatever the decoders have produced into the array texture.
fn upload_frames(mut wall: ResMut<VideoWall>, mut images: ResMut<Assets<Image>>) {
    // `get_mut` is what schedules the re-upload, and it re-uploads the whole
    // array — a megabyte and a bit — so it is worth not touching the asset at
    // all on a frame where nothing arrived. At 25 fps into a 60 fps show that
    // is more than half of them. Nothing fresh also means nothing can have
    // come live, so the count below cannot have changed either.
    if !wall.slots.iter().any(Slot::has_fresh_frame) {
        return;
    }

    let layer = frame_bytes(TILE);
    let Some(mut image) = images.get_mut(&wall.texture) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };

    let mut live = 0;
    for (index, slot) in wall.slots.iter().enumerate() {
        let start = index * layer;
        let Some(target) = data.get_mut(start..start + layer) else {
            continue;
        };
        // Layer-major, so one layer is one contiguous run and the copy is a
        // single memcpy rather than a row-by-row blit into a subrectangle.
        // That is most of why this is an array texture and not an atlas — the
        // rest of it is that mip levels of an atlas bleed across tiles.
        if slot.take_frame_into(target) {
            live += 1;
        }
    }
    wall.live = live;
}
