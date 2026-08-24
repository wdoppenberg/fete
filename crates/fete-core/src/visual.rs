//! The [`Visual`] trait, the registry, and the machinery that puts exactly one
//! visual on screen at a time.
//!
//! A visual is a `Material2d` painted onto a quad that covers the viewport.
//! That is a deliberately narrow definition: it means adding a visual is a WGSL
//! file plus a struct, and it means every visual automatically inherits HDR,
//! bloom and tonemapping from the shared camera rig. Visuals that need more —
//! a compute simulation feeding a texture, extra meshes — add their own plugin
//! alongside, and still present through the same fullscreen material.

use core::hash::Hash;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;
use bevy::sprite_render::{Material2d, Material2dPlugin, MeshMaterial2d};

use crate::bleed::Transition;
use crate::globals::{FeteGlobals, Frame, ShowOutput};
use crate::palette::Palette;
use crate::signal::{Audio, Macros};

/// Identifies a visual. Stable across runs — used for presets and MIDI mapping.
pub type VisualId = &'static str;

/// A full-screen look.
///
/// Implementors are ordinary Bevy materials, so `#[derive(AsBindGroup)]` gives
/// access to uniforms, textures and storage buffers. The only requirements the
/// framework adds are an identity and a way to reach the shared globals block.
/// The `Data` bound is what `Material2dPlugin` requires for pipeline
/// specialisation keys. A material using `#[derive(AsBindGroup)]` without a
/// `#[bind_group_data]` attribute has `Data = ()`, which satisfies it for free.
pub trait Visual: Material2d<Data: PartialEq + Eq + Hash + Clone> + Default {
    /// Stable identifier, e.g. `"sprawl"`.
    const ID: VisualId;
    /// Name shown in the overlay.
    const NAME: &'static str;
    /// Free-form tags for grouping and future auto-setlists.
    const TAGS: &'static [&'static str] = &[];

    /// Hand the framework the globals block inside this material.
    ///
    /// Every visual stores a [`FeteGlobals`] as its first uniform; this is how
    /// the driver system finds it without reflection.
    fn globals_mut(&mut self) -> &mut FeteGlobals;

    /// Update visual-specific parameters for this frame.
    ///
    /// Globals are written before this is called, so `self.globals` is already
    /// current. Override to map macro knobs onto your own uniforms.
    fn animate(&mut self, frame: &Frame) {
        let _ = frame;
    }
}

/// What the overlay and the cycling logic know about a registered visual.
#[derive(Debug, Clone)]
pub struct VisualInfo {
    pub id: VisualId,
    pub name: &'static str,
    pub tags: &'static [&'static str],
}

/// Every visual registered with the app, in registration order.
#[derive(Resource, Debug, Clone, Default)]
pub struct VisualRegistry {
    entries: Vec<VisualInfo>,
}

impl VisualRegistry {
    pub fn entries(&self) -> &[VisualInfo] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn index_of(&self, id: VisualId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    pub fn get(&self, index: usize) -> Option<&VisualInfo> {
        self.entries.get(index)
    }

    pub fn info(&self, id: VisualId) -> Option<&VisualInfo> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Look up a visual by a runtime string — a command-line argument, a
    /// config file — and recover its `'static` id.
    pub fn info_by_str(&self, id: &str) -> Option<VisualId> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.id)
    }

    /// The visual `offset` places after `current`, wrapping. Returns `None`
    /// when nothing is registered.
    pub fn cycle(&self, current: Option<VisualId>, offset: isize) -> Option<VisualId> {
        if self.entries.is_empty() {
            return None;
        }
        let len = self.entries.len() as isize;
        let base = current.and_then(|id| self.index_of(id)).unwrap_or(0) as isize;
        let next = (base + offset).rem_euclid(len) as usize;
        Some(self.entries[next].id)
    }

    fn register(&mut self, info: VisualInfo) {
        if self.entries.iter().any(|entry| entry.id == info.id) {
            warn!(
                "visual `{}` registered twice; ignoring the second registration",
                info.id
            );
            return;
        }
        self.entries.push(info);
    }
}

/// Which visual is on screen. `None` means a black stage.
///
/// Modelled as a state so visuals get `OnEnter`/`OnExit` and
/// [`DespawnOnExit`] for free — entities a visual spawns are cleaned up
/// automatically when the show moves on.
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ActiveVisual(pub Option<VisualId>);

impl ActiveVisual {
    pub fn of<V: Visual>() -> Self {
        Self(Some(V::ID))
    }
}

/// Marker for the viewport-filling quad a visual is painted on.
#[derive(Component, Debug)]
pub struct VisualSurface;

/// The unit quad shared by every visual, scaled to the viewport each frame.
#[derive(Resource, Debug)]
pub struct VisualQuad(pub Handle<Mesh>);

impl FromWorld for VisualQuad {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        Self(meshes.add(Rectangle::new(1.0, 1.0)))
    }
}

/// Registers visuals with the app.
pub trait VisualAppExt {
    /// Add a visual: its material plugin, its registry entry, and the systems
    /// that spawn, drive and tear it down when it becomes active.
    fn add_visual<V: Visual>(&mut self) -> &mut Self;

    /// Add systems that only run while `V` is the active visual.
    ///
    /// Use this for anything a visual needs beyond its material — driving a
    /// compute simulation, spawning extra geometry, reacting to beats.
    fn add_visual_systems<V: Visual, M>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;
}

impl VisualAppExt for App {
    fn add_visual<V: Visual>(&mut self) -> &mut Self {
        if !self.is_plugin_added::<Material2dPlugin<V>>() {
            self.add_plugins(Material2dPlugin::<V>::default());
        }

        self.world_mut()
            .resource_mut::<VisualRegistry>()
            .register(VisualInfo {
                id: V::ID,
                name: V::NAME,
                tags: V::TAGS,
            });

        let state = ActiveVisual::of::<V>();
        self.add_systems(OnEnter(state.clone()), spawn_visual::<V>)
            .add_systems(
                Update,
                animate_visual::<V>
                    .in_set(VisualSystems::Animate)
                    .run_if(in_state(state)),
            )
    }

    fn add_visual_systems<V: Visual, M>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_systems(schedule, systems.run_if(in_state(ActiveVisual::of::<V>())))
    }
}

/// Ordering hooks for the per-frame visual update.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum VisualSystems {
    /// [`FeteGlobals`] and friends are refreshed here.
    Prepare,
    /// Materials are written here. Runs after [`Prepare`](Self::Prepare).
    Animate,
}

/// Spawns the fullscreen quad carrying `V`'s material.
///
/// [`DespawnOnExit`] ties the entity's lifetime to the state, so switching
/// visuals needs no matching teardown system.
fn spawn_visual<V: Visual>(
    mut commands: Commands,
    quad: Res<VisualQuad>,
    mut materials: ResMut<Assets<V>>,
    globals: Res<FeteGlobals>,
) {
    let mut material = V::default();
    // Seed the globals immediately: the quad is rendered before `Update` runs
    // again, and an unwritten uniform means one frame of garbage.
    *material.globals_mut() = *globals;

    commands.spawn((
        Name::new(V::NAME),
        VisualSurface,
        Mesh2d(quad.0.clone()),
        MeshMaterial2d(materials.add(material)),
        // Slightly behind the default plane so overlay UI drawn in 2d still
        // sorts on top.
        Transform::from_xyz(0.0, 0.0, -1.0),
        DespawnOnExit(ActiveVisual::of::<V>()),
    ));
}

/// Copies globals into the active material and lets the visual animate itself.
fn animate_visual<V: Visual>(
    mut materials: ResMut<Assets<V>>,
    surfaces: Query<&MeshMaterial2d<V>, With<VisualSurface>>,
    globals: Res<FeteGlobals>,
    clock: Res<crate::clock::ShowClock>,
    macros: Res<Macros>,
    audio: Res<Audio>,
    palette: Res<Palette>,
) {
    let frame = Frame {
        globals: &globals,
        clock: &clock,
        macros: &macros,
        audio: &audio,
        palette: &palette,
    };

    for handle in &surfaces {
        // `get_mut` flags the asset as changed, which is what schedules the
        // uniform re-upload for this frame.
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        *material.globals_mut() = *globals;
        material.animate(&frame);
    }
}

/// Keeps the quad covering the viewport.
///
/// The 2d camera maps one world unit to one pixel, so scaling the unit quad by
/// the window size is an exact fit at any resolution.
pub fn fit_surface_to_viewport(
    globals: Res<FeteGlobals>,
    mut surfaces: Query<&mut Transform, With<VisualSurface>>,
) {
    let size = globals.resolution;
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    for mut transform in &mut surfaces {
        transform.scale = Vec3::new(size.x, size.y, 1.0);
    }
}

/// Requests a visual change. Handled by [`apply_visual_requests`].
#[derive(Message, Debug, Clone)]
pub enum VisualRequest {
    /// Show a specific visual.
    Show(VisualId),
    /// Move `offset` places through the registry.
    Cycle(isize),
    /// Fade to nothing.
    Blackout,
}

/// Applies [`VisualRequest`]s, re-seeds the incoming visual, and starts the
/// transition that covers the change.
pub fn apply_visual_requests(
    mut requests: MessageReader<VisualRequest>,
    registry: Res<VisualRegistry>,
    current: Res<State<ActiveVisual>>,
    mut next: ResMut<NextState<ActiveVisual>>,
    mut output: ResMut<ShowOutput>,
    mut transition: ResMut<Transition>,
    clock: Res<crate::clock::ShowClock>,
) {
    for request in requests.read() {
        let target = match request {
            VisualRequest::Show(id) => Some(*id),
            VisualRequest::Cycle(offset) => registry.cycle(current.get().0, *offset),
            VisualRequest::Blackout => None,
        };

        if target == current.get().0 {
            continue;
        }

        // A fresh seed per activation: the same visual should not look
        // identical every time it comes back up during a set.
        output.seed = (clock.elapsed * 1000.0).fract() as f32;
        next.set(ActiveVisual(target));

        // Started here rather than by whoever raised the request, so every
        // route into a visual change — key, autopilot, blackout, a control app
        // that does not exist yet — gets the same handover for free. The frame
        // still on screen is the one it keeps.
        transition.start(&clock);

        match target {
            Some(id) => info!("visual -> {id}"),
            None => info!("visual -> blackout"),
        }
    }
}
