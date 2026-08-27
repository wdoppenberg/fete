//! One slot of the video wall: a thread, an `ffmpeg`, and a frame buffer.
//!
//! Decoding is done by piping raw RGBA out of an `ffmpeg` subprocess rather
//! than by linking libavcodec. That is a deliberate trade. Linking buys nothing
//! this crate needs — no seeking, no audio, no container introspection — and
//! costs a `pkg-config` build dependency, a pile of `unsafe`, and a much worse
//! story the evening before a show on a machine where the build breaks. The
//! subprocess costs one `Command`, and eight of them decoding 320x240 is a
//! rounding error next to the shader they feed.
//!
//! `-re` is what makes the whole thing simple: ffmpeg paces its output to the
//! clip's own frame rate, so a blocking `read_exact` on this side *is* the
//! playback clock. No timestamps, no drift correction, no A/V sync, and a
//! thread that costs nothing while it waits.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;

/// The newest frame a slot has decoded, and the state the uploader needs.
struct Latest {
    /// Tightly packed RGBA, `tile.x * tile.y * 4` bytes.
    pixels: Vec<u8>,
    /// A frame has arrived since the uploader last took one.
    fresh: bool,
    /// This slot has produced at least one frame, so its layer holds a picture
    /// rather than the black it was allocated with.
    live: bool,
}

/// State shared between a decoder thread and the main world.
struct Shared {
    latest: Mutex<Latest>,
    /// Set on teardown. The thread checks it between frames.
    stop: AtomicBool,
    /// The running `ffmpeg`, so teardown can kill it directly instead of
    /// waiting for a thread that is blocked reading from it.
    child: Mutex<Option<Child>>,
}

/// A decoder feeding one layer of the wall texture.
pub struct Slot {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Slot {
    /// Start decoding `playlist`, looping it forever, into `tile`-sized frames.
    pub fn start(playlist: Vec<PathBuf>, tile: UVec2) -> Self {
        let bytes = frame_bytes(tile);
        let shared = Arc::new(Shared {
            latest: Mutex::new(Latest {
                pixels: vec![0; bytes],
                fresh: false,
                live: false,
            }),
            stop: AtomicBool::new(false),
            child: Mutex::new(None),
        });

        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("fete-video decode".into())
                .spawn(move || run(&playlist, tile, &shared))
                .ok()
        };

        Self { shared, thread }
    }

    /// Whether a frame has arrived since the last [`take_frame_into`].
    ///
    /// Cheap, and worth asking before touching the texture at all: taking the
    /// image asset mutably re-uploads every layer, and at 25 fps into a 60 fps
    /// show most frames have nothing new on any slot.
    ///
    /// [`take_frame_into`]: Self::take_frame_into
    pub fn has_fresh_frame(&self) -> bool {
        self.shared.latest.lock().is_ok_and(|latest| latest.fresh)
    }

    /// Copy the newest frame into `layer` if one has arrived since the last
    /// call. Returns whether this slot is showing anything at all.
    ///
    /// A slot that is between clips reports live and copies nothing: the layer
    /// keeps the last frame of the outgoing clip rather than dropping to black
    /// for the tenth of a second it takes the next `ffmpeg` to start. On a wall
    /// of televisions a black set is a *statement* — it is the difference
    /// between a set that is off and one that is on — so it must never happen
    /// by accident.
    pub fn take_frame_into(&self, layer: &mut [u8]) -> bool {
        let Ok(mut latest) = self.shared.latest.lock() else {
            return false;
        };
        if latest.fresh && layer.len() == latest.pixels.len() {
            layer.copy_from_slice(&latest.pixels);
            latest.fresh = false;
        }
        latest.live
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Killed here rather than left to the thread: the thread spends
        // essentially all of its life blocked in `read_exact`, and closing the
        // pipe under it is what unblocks it. Left to notice the flag on its
        // own it would not do so until the next frame, and a wall of eight
        // would hold the show open for a visible moment on the way out.
        if let Ok(mut child) = self.shared.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Bytes in one decoded frame.
pub fn frame_bytes(tile: UVec2) -> usize {
    tile.x as usize * tile.y as usize * 4
}

/// Whether `ffmpeg` is on the path and runnable.
///
/// Checked once, up front, so a machine without it gets one warning and a show
/// that runs — rather than eight threads each failing to spawn on a loop.
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run(playlist: &[PathBuf], tile: UVec2, shared: &Arc<Shared>) {
    if playlist.is_empty() {
        return;
    }

    let mut buffer = vec![0u8; frame_bytes(tile)];
    let mut index = 0usize;

    while !shared.stop.load(Ordering::Relaxed) {
        let clip = &playlist[index % playlist.len()];
        index += 1;

        let Some(mut child) = spawn_ffmpeg(clip, tile) else {
            // A clip ffmpeg will not open — a truncated download, a codec it
            // was built without. Move on to the next rather than retrying: a
            // permanently bad clip must not spin this thread.
            warn!("video: could not decode {}", clip.display());
            continue;
        };
        let Some(mut stdout) = child.stdout.take() else {
            continue;
        };

        if let Ok(mut slot) = shared.child.lock() {
            *slot = Some(child);
        }

        // `read_exact` blocks until ffmpeg has paced out a whole frame, which
        // is the entire clock for this thread. It returns `Err` at end of
        // clip, which is how the loop advances.
        while !shared.stop.load(Ordering::Relaxed) && stdout.read_exact(&mut buffer).is_ok() {
            let Ok(mut latest) = shared.latest.lock() else {
                break;
            };
            // Swapped rather than copied: the decoder gets the uploader's old
            // buffer back and neither side allocates again for the rest of the
            // night.
            std::mem::swap(&mut latest.pixels, &mut buffer);
            latest.fresh = true;
            latest.live = true;
        }

        if let Ok(mut slot) = shared.child.lock()
            && let Some(mut child) = slot.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_ffmpeg(clip: &Path, tile: UVec2) -> Option<Child> {
    Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        // Pace the output to the clip's own frame rate. Without it ffmpeg
        // decodes the whole clip as fast as it can and this thread reads a
        // seventy-five second clip in about a second.
        .arg("-re")
        .args(["-i".as_ref(), clip.as_os_str()])
        .arg("-an")
        // Scaled and rate-converted here rather than at fetch time as well,
        // because the `--video` directory may hold anything at all.
        .args(["-vf", &format!("scale={}:{},fps=25", tile.x, tile.y)])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Dropped rather than inherited: ffmpeg's per-frame chatter would
        // scroll the operator HUD off the terminal, and an unread pipe would
        // eventually fill and stall the decoder.
        .stderr(Stdio::null())
        .spawn()
        .ok()
}
