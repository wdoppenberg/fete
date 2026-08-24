//! `fete-app` — the shell a show runs inside.
//!
//! `fete-core` knows how to drive visuals; this crate knows how to put them in
//! front of an audience. It configures the window for a projector, wires the
//! live keyboard control surface, draws the operator HUD, and picks a visual to
//! start on.
//!
//! A standalone visual is then a `main.rs` this short:
//!
//! ```ignore
//! fn main() -> AppExit {
//!     fete_app::show(ShowConfig::new("Sprawl"))
//!         .add_plugins(SprawlPlugin)
//!         .run()
//! }
//! ```

pub mod capture;
pub mod control;
pub mod hud;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode, WindowResolution};
use fete_core::FeteCorePlugin;
use fete_core::prelude::*;

use crate::capture::{ScheduledCapture, handle_capture_key, run_scheduled_capture};
use crate::control::{
    Blackout, HudVisible, handle_autopilot_key, handle_macro_keys, handle_palette_keys,
    handle_show_keys, handle_window_keys,
};
use crate::hud::{spawn_hud, update_hud};

/// How the show window is set up.
#[derive(Resource, Debug, Clone)]
pub struct ShowConfig {
    pub title: String,
    /// Windowed size in physical pixels. Ignored when starting fullscreen.
    pub resolution: UVec2,
    pub fullscreen: bool,
    /// Which display to take over when fullscreen.
    pub monitor: MonitorSelection,
    /// Cap the frame rate to the display refresh. Leave on for projection:
    /// tearing is far more visible on a large, dim image than on a monitor.
    pub vsync: bool,
    /// Draw the operator HUD at startup.
    pub hud: bool,
    /// Starting tempo.
    pub bpm: f32,
    /// Visual to open on, by [`VisualId`]. Falls back to the first registered
    /// visual when absent or unknown.
    pub start_with: Option<String>,
    /// Output shape as width/height, masked to black outside.
    ///
    /// `None` — the default — fills whatever window it is given. Visuals are
    /// written to compose at any aspect, so masking is only worth doing when
    /// the projector's shape genuinely differs from the output's and the bars
    /// would otherwise land on the wall.
    pub aspect: Option<f32>,
}

impl Default for ShowConfig {
    fn default() -> Self {
        Self {
            title: "fete".to_string(),
            // 4:3 by default, matching the screen this was built for, but
            // nothing depends on it — the visuals compose at any aspect and
            // fullscreen takes whatever the projector gives.
            resolution: UVec2::new(1440, 1080),
            fullscreen: false,
            monitor: MonitorSelection::Current,
            vsync: true,
            hud: true,
            bpm: 128.0,
            start_with: None,
            aspect: None,
        }
    }
}

impl ShowConfig {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..default()
        }
    }

    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.resolution = UVec2::new(width, height);
        self
    }

    pub fn fullscreen(mut self, monitor: MonitorSelection) -> Self {
        self.fullscreen = true;
        self.monitor = monitor;
        self
    }

    pub fn with_bpm(mut self, bpm: f32) -> Self {
        self.bpm = bpm;
        self
    }

    pub fn without_hud(mut self) -> Self {
        self.hud = false;
        self
    }

    pub fn starting_with(mut self, id: impl Into<String>) -> Self {
        self.start_with = Some(id.into());
        self
    }

    /// Set the output shape as width/height, e.g. `16.0 / 9.0`.
    pub fn with_aspect(mut self, aspect: f32) -> Self {
        self.aspect = Some(aspect);
        self
    }

    /// Fill whatever window the show gets, with no aspect mask.
    pub fn filling_window(mut self) -> Self {
        self.aspect = None;
        self
    }
}

/// Build an [`App`] with the window, framework and control surface in place.
///
/// Register visuals on the returned app, then `run()` it.
pub fn show(config: ShowConfig) -> App {
    let mut app = App::new();

    let window = Window {
        title: config.title.clone(),
        resolution: WindowResolution::from(config.resolution.to_array()),
        mode: if config.fullscreen {
            WindowMode::BorderlessFullscreen(config.monitor)
        } else {
            WindowMode::Windowed
        },
        present_mode: if config.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        },
        ..default()
    };

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            .set(ImagePlugin::default_linear()),
    )
    .add_plugins((FeteCorePlugin, ShowShellPlugin))
    .insert_resource(config);

    // Scripted preview capture, for generating stills without a person at the
    // keyboard. Absent in normal use.
    if let Some(capture) = ScheduledCapture::from_env() {
        app.insert_resource(capture);
    }

    app
}

/// Control surface, HUD, and start-up visual selection.
///
/// Separate from [`show`] so an app that builds its own window (a VJ tool
/// embedding the show in a larger UI, say) can still get the shell.
pub struct ShowShellPlugin;

impl Plugin for ShowShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowConfig>()
            .init_resource::<HudVisible>()
            .init_resource::<Blackout>();

        app.add_systems(Startup, (apply_config, spawn_hud));

        // Start on the first registered visual. Deferred to `PostStartup` so
        // every `add_visual` call — wherever in the plugin graph it happened —
        // has already landed in the registry.
        app.add_systems(PostStartup, select_opening_visual);

        app.add_systems(
            Update,
            (
                handle_show_keys,
                handle_macro_keys,
                handle_palette_keys,
                handle_window_keys,
                handle_capture_key,
                handle_autopilot_key,
            )
                // Ahead of the framework's own update so a key press this frame
                // is reflected in this frame's picture.
                .before(VisualSystems::Prepare),
        );

        app.add_systems(
            Update,
            (update_hud, run_scheduled_capture).after(VisualSystems::Animate),
        );
    }
}

fn apply_config(
    config: Res<ShowConfig>,
    mut clock: ResMut<ShowClock>,
    mut hud: ResMut<HudVisible>,
) {
    clock.bpm = config.bpm;
    hud.0 = config.hud;
}

fn select_opening_visual(
    config: Res<ShowConfig>,
    registry: Res<VisualRegistry>,
    current: Res<State<ActiveVisual>>,
    mut requests: MessageWriter<VisualRequest>,
) {
    if current.get().0.is_some() {
        return;
    }

    let requested = config.start_with.as_deref().and_then(|wanted| {
        let found = registry.info_by_str(wanted);
        if found.is_none() {
            warn!("no visual named `{wanted}`; opening on the first one instead");
        }
        found
    });

    match requested.or_else(|| registry.entries().first().map(|info| info.id)) {
        Some(id) => {
            requests.write(VisualRequest::Show(id));
        }
        None => warn!("no visuals registered — the show will render an empty stage"),
    }
}

/// The pieces a standalone visual binary needs.
pub mod prelude {
    pub use crate::capture::ScheduledCapture;
    pub use crate::{ShowConfig, ShowShellPlugin, show};
    pub use bevy::window::MonitorSelection;
}
