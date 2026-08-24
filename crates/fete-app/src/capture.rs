//! Stills from the show's own framebuffer.
//!
//! Two uses. The obvious one is grabbing a frame you liked during a set —
//! `F12`, and it lands in `captures/`. The other is scripted: set
//! `FETE_CAPTURE` and the app renders for a fixed time, writes one frame and
//! exits, which is how preview images get generated without a person watching.
//!
//! This captures the render target, not the desktop, so the HUD is included but
//! nothing else on the machine ever is.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// A scripted one-shot capture.
#[derive(Resource, Debug, Clone)]
pub struct ScheduledCapture {
    pub path: PathBuf,
    /// Seconds to let the visual settle before capturing. Simulations need a
    /// few seconds before they show anything worth looking at.
    pub after: f32,
    /// Quit once the frame is written.
    pub then_exit: bool,
    taken: bool,
}

impl ScheduledCapture {
    pub fn new(path: impl Into<PathBuf>, after: f32) -> Self {
        Self {
            path: path.into(),
            after,
            then_exit: true,
            taken: false,
        }
    }

    /// Parse `FETE_CAPTURE`, formatted `path.png` or `path.png@seconds`.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("FETE_CAPTURE").ok()?;
        let (path, after) = match raw.rsplit_once('@') {
            Some((path, secs)) => (path, secs.parse().unwrap_or(5.0)),
            None => (raw.as_str(), 5.0),
        };
        Some(Self::new(path, after))
    }
}

/// Directory for `F12` grabs, relative to the working directory.
const CAPTURE_DIR: &str = "captures";

pub fn handle_capture_key(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    // `Period` as well as `F12`: on macOS the function keys are media keys by
    // default, so an F-key-only binding is unreachable on most laptops.
    if !keys.just_pressed(KeyCode::F12) && !keys.just_pressed(KeyCode::Period) {
        return;
    }

    if let Err(err) = std::fs::create_dir_all(CAPTURE_DIR) {
        warn!("could not create `{CAPTURE_DIR}`: {err}");
        return;
    }

    // Unix seconds rather than a counter: a counter would overwrite last
    // night's grabs the next time the app starts.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = format!("{CAPTURE_DIR}/fete-{stamp}.png");

    info!("capturing {path}");
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

pub fn run_scheduled_capture(
    mut commands: Commands,
    time: Res<Time>,
    capture: Option<ResMut<ScheduledCapture>>,
    mut exit: MessageWriter<AppExit>,
    mut frames_since: Local<u32>,
) {
    let Some(mut capture) = capture else {
        return;
    };

    if capture.taken {
        // A few frames of grace before quitting. The screenshot is resolved in
        // the render world and read back asynchronously; exiting on the same
        // frame the request is made would tear down the device first and the
        // file would never be written.
        //
        // Note: scripted captures are unreliable on macOS when the window is
        // not frontmost — roughly half come out black. Raising this count does
        // not help (tried 30, which was worse), so the cause is elsewhere.
        // Retry until a capture is non-black, or grab stills with F12 / `.`
        // from a window you can see.
        if capture.then_exit {
            *frames_since += 1;
            if *frames_since > 4 {
                exit.write(AppExit::Success);
            }
        }
        return;
    }

    if time.elapsed_secs() < capture.after {
        return;
    }
    capture.taken = true;

    let path = capture.path.clone();
    info!("capturing {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}
