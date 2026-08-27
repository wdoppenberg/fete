//! Rendering the stage smaller than the window, then blowing it up.
//!
//! Shader cost falls with the square of resolution, which makes this the
//! largest single lever in the framework: at half scale a visual does a quarter
//! of the work, and every pass the rig owns — bleed, bloom, tonemapping, grade —
//! gets a quarter cheaper along with it, for free and without touching a
//! shader.
//!
//! It is also the lever these visuals mind least. They are soft, mostly-black,
//! bloomed procedural fields with no text and no hard edges, seen from several
//! metres away on a four-metre screen. The grade already ends in a tilt-shift
//! blur and a vignette. Halving the sample rate of that costs less than the
//! same halving would cost almost any other kind of image.
//!
//! # Shape
//!
//! At [`render_scale`](crate::quality::Quality::render_scale) `1.0` none of
//! this exists: no image, no second camera, no extra blit, and the pipeline is
//! exactly what it was before this module was written. That is deliberate —
//! the laptop path is the reference the low tier is judged against, and it
//! should not quietly acquire a fullscreen copy to support hardware it is not
//! running on.
//!
//! Below `1.0` the stage camera renders into [`StageTarget`]'s image and a
//! second camera draws that image across the window. The two are kept apart by
//! render layers rather than by ordering, because a 2d camera otherwise sees
//! every 2d entity in the world — including, without this, the sprite showing
//! its own output.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

use crate::quality::Quality;

/// Marker for the camera that puts the scaled stage on screen.
///
/// Only exists while the stage is rendered at reduced resolution.
#[derive(Component, Debug)]
pub struct PresentCamera;

/// Marker for the sprite the [`PresentCamera`] draws.
#[derive(Component, Debug)]
pub struct PresentSurface;

/// The layer the present pass lives on.
///
/// Layer 0 — the default every visual and the stage camera land on — is the
/// show. Keeping the present sprite off it is what stops the stage camera
/// rendering last frame's output back into this frame's.
pub const PRESENT_LAYER: Layer = 1;

/// Bevy's render-layer id type, re-exported so callers need not reach for it.
pub type Layer = usize;

/// The image the stage renders into, when it is not rendering into the window.
///
/// Absent at full scale, which is the flag the rest of the framework tests.
#[derive(Resource, Debug, Clone)]
pub struct StageTarget {
    pub image: Handle<Image>,
    /// Size of `image` in physical pixels.
    pub physical: UVec2,
    /// Physical texels per logical unit in `image`.
    ///
    /// The window's own scale factor times the render scale, which is what
    /// makes the image's *logical* size identical to the window's. Without it
    /// the stage camera would map one world unit to one texel, the viewport
    /// quad — sized in logical units — would cover only part of the frame, and
    /// the show would render into the middle of a black border.
    pub scale_factor: f32,
}

impl StageTarget {
    fn render_target(&self) -> RenderTarget {
        RenderTarget::Image(ImageRenderTarget {
            handle: self.image.clone(),
            scale_factor: self.scale_factor,
        })
    }
}

/// The stage's size in the logical units the rest of the framework speaks.
///
/// This is the seam the feature turns on. Before it existed, three separate
/// systems — globals, grade and bleed — each queried `Window` directly and each
/// assumed the stage filled it. They read this instead, so there is one place
/// that knows how big the stage is.
///
/// Note what it is *not*: the texel count. Render scale deliberately leaves
/// this alone, because `resolution` has always meant logical pixels — a retina
/// window already renders four texels for every unit reported here, and
/// scanlines, grain and dither are all sized against it so that they look the
/// same on a laptop preview and on a projector feed. Scaling this down as well
/// would resize every one of those effects, which is a change to the look
/// rather than to the cost. Render scale is a sampling-rate change and nothing
/// else — the same thing as moving to a lower-DPI screen.
#[derive(Resource, Debug, Clone, Copy)]
pub struct StageResolution(pub Vec2);

impl Default for StageResolution {
    /// Not zero. Nothing should ever read the default — the startup systems
    /// set it before `First` runs — but a headless app with no window never
    /// gets that far, and a zero here turns every `uv / resolution` in every
    /// shader into a NaN rather than into a visibly wrong picture.
    fn default() -> Self {
        Self(Vec2::new(1920.0, 1080.0))
    }
}

/// Creates the render target, if the quality setting calls for one.
///
/// Runs in `Startup` between the adapter probe and the camera rig: the probe
/// may have lowered the tier, and the camera needs to know where to point.
pub fn setup_stage_target(
    mut commands: Commands,
    quality: Res<Quality>,
    windows: Query<&Window>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };

    let logical = Vec2::new(window.width(), window.height());
    commands.insert_resource(StageResolution(logical));

    if quality.render_scale >= 1.0 {
        return;
    }

    let physical = quality.stage_size(window.physical_size());
    let scale_factor = window.scale_factor() * quality.render_scale;
    let image = images.add(stage_image(physical));

    commands.spawn((
        Name::new("present camera"),
        PresentCamera,
        Camera2d,
        Camera {
            // After the stage, and the only camera left pointing at the window
            // — which is also what makes it the default UI camera, so the HUD
            // follows it here without being told.
            order: 1,
            ..default()
        },
        // Nothing on this layer has an edge that is not already a texel
        // boundary, so there is nothing for MSAA to do but cost memory.
        Msaa::Off,
        RenderLayers::layer(PRESENT_LAYER),
    ));

    commands.spawn((
        Name::new("present surface"),
        PresentSurface,
        Sprite {
            image: image.clone(),
            custom_size: Some(logical),
            ..default()
        },
        RenderLayers::layer(PRESENT_LAYER),
    ));

    commands.insert_resource(StageTarget {
        image,
        physical,
        scale_factor,
    });
}

/// Points the stage camera at the render target, when there is one.
///
/// A separate system rather than a branch inside the camera spawner so that
/// `stage.rs` stays about the look of the rig and knows nothing about scaling.
pub fn retarget_stage_camera(
    mut commands: Commands,
    target: Option<Res<StageTarget>>,
    cameras: Query<Entity, With<crate::stage::StageCamera>>,
) {
    let Some(target) = target else {
        return;
    };
    for camera in &cameras {
        // `RenderTarget` is a component of its own here, not a field of
        // `Camera`, so this is an insert rather than an assignment.
        commands.entity(camera).insert(target.render_target());
    }
}

/// Follows the window: resizes the target and the sprite, and republishes
/// [`StageResolution`].
///
/// Runs every frame in `First` so that everything downstream — globals, grade,
/// bleed, the quad fit — sees one consistent size for the frame, including the
/// frame a window is resized on.
pub fn track_stage_resolution(
    quality: Res<Quality>,
    windows: Query<&Window>,
    mut resolution: ResMut<StageResolution>,
    target: Option<ResMut<StageTarget>>,
    mut images: ResMut<Assets<Image>>,
    mut surfaces: Query<&mut Sprite, With<PresentSurface>>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };

    let logical = Vec2::new(window.width(), window.height());
    // Hold the last good size rather than publishing zero. A minimised window
    // reports zero, and a zero resolution turns every `uv / resolution` in
    // every shader into a NaN — which survives into the bleed's history
    // texture and stays on screen after the window comes back.
    if logical.x > 0.0 && logical.y > 0.0 {
        resolution.0 = logical;
    }

    let Some(mut target) = target else {
        return;
    };

    let physical = quality.stage_size(window.physical_size());
    if physical == target.physical {
        return;
    }

    // Reallocating beats resizing in place: `Image` has no in-place resize that
    // preserves the GPU texture, and a stage resize is a once-per-window-drag
    // event, not a per-frame one.
    let handle = images.add(stage_image(physical));
    for mut sprite in &mut surfaces {
        sprite.image = handle.clone();
        sprite.custom_size = Some(logical);
    }
    target.image = handle;
    target.physical = physical;
    target.scale_factor = window.scale_factor() * quality.render_scale;
}

/// Repoints the stage camera after [`track_stage_resolution`] reallocates.
///
/// Split out because the resize path runs in `First` every frame and only
/// rarely has anything to do, while the change it makes has to reach a camera
/// that lives in another query.
pub fn follow_stage_target(
    mut commands: Commands,
    target: Option<Res<StageTarget>>,
    cameras: Query<Entity, With<crate::stage::StageCamera>>,
) {
    let Some(target) = target else {
        return;
    };
    if !target.is_changed() {
        return;
    }
    for camera in &cameras {
        commands.entity(camera).insert(target.render_target());
    }
}

/// The target the stage renders into.
///
/// Eight-bit, not `Rgba16Float`: the grade runs after tonemapping, so what
/// lands here is already display-referred and the extra range would be spent
/// on nothing. Halving the bytes per texel matters on hardware that has no
/// dedicated video memory and reads this texture straight back for the upscale.
fn stage_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    // Linear, so the upscale is a smooth stretch rather than visible texels.
    // On this material — glow and haze — bilinear is all the reconstruction
    // the image needs, and anything sharper would only bring back the aliasing
    // that rendering small avoided.
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::Tier;

    #[test]
    fn full_scale_allocates_nothing() {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default())
            .init_asset::<Image>()
            .insert_resource(Quality::new(Tier::High))
            .add_systems(Startup, setup_stage_target);
        app.world_mut().spawn(Window::default());
        app.update();

        assert!(
            app.world().get_resource::<StageTarget>().is_none(),
            "the high tier must keep the original single-camera pipeline"
        );
    }

    #[test]
    fn reduced_scale_spawns_one_camera_and_one_surface() {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default())
            .init_asset::<Image>()
            .insert_resource(Quality::new(Tier::Low))
            .add_systems(Startup, setup_stage_target);
        app.world_mut().spawn(Window::default());
        app.update();

        let target = app
            .world()
            .get_resource::<StageTarget>()
            .expect("a reduced scale needs somewhere to render");
        let window = Window::default();
        let expected = Quality::new(Tier::Low).stage_size(window.physical_size());
        assert_eq!(target.physical, expected);

        assert_eq!(
            app.world_mut()
                .query::<&PresentCamera>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(
            app.world_mut()
                .query::<&PresentSurface>()
                .iter(app.world())
                .count(),
            1
        );
    }

    /// Render scale must not move `StageResolution`: it changes how many
    /// texels the stage has, not how many logical pixels it claims, so every
    /// effect sized against `resolution` keeps the size it had.
    #[test]
    fn render_scale_leaves_the_logical_size_alone() {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default())
            .init_asset::<Image>()
            .insert_resource(Quality::new(Tier::Low))
            .add_systems(Startup, setup_stage_target);
        let window = Window::default();
        let logical = Vec2::new(window.width(), window.height());
        app.world_mut().spawn(window);
        app.update();

        assert_eq!(app.world().resource::<StageResolution>().0, logical);
    }

    /// The image must present at the window's logical size, or the viewport
    /// quad covers only part of it and the show renders inside a black border.
    #[test]
    fn the_target_keeps_the_windows_logical_size() {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default())
            .init_asset::<Image>()
            .insert_resource(Quality::new(Tier::Low))
            .add_systems(Startup, setup_stage_target);
        let window = Window::default();
        let logical = Vec2::new(window.width(), window.height());
        app.world_mut().spawn(window);
        app.update();

        let target = app.world().resource::<StageTarget>();
        let target_logical = target.physical.as_vec2() / target.scale_factor;
        assert!(
            (target_logical - logical).abs().max_element() <= 1.0,
            "{target_logical:?} should match the window's {logical:?}"
        );
    }
}
