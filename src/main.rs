//! This example illustrates how to make a headless renderer.
//! Derived from: <https://sotrh.github.io/learn-wgpu/showcase/windowless/#a-triangle-without-a-window>
//! It follows these steps:
//!
//! 1. Render from camera to gpu-image render target
//! 2. Copy from gpu image to buffer using `ImageCopyDriver` node in `RenderGraph`
//! 3. Copy from buffer to channel using `receive_image_from_buffer` after `RenderSystems::Render`
//! 4. Save from channel to random named file using `scene::update` at `PostUpdate` in `MainWorld`
//! 5. Exit if `single_image` setting is set
//!
//! If your goal is to capture a single “screenshot” as opposed to every single rendered frame
//! without gaps, it is simpler to use [`bevy::render::view::window::screenshot::Screenshot`]
//! than this approach.

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    core_pipeline::tonemapping::Tonemapping,
    image::TextureFormatPixelInfo,
    prelude::*,
    render::renderer::RenderDevice,
    window::ExitCondition,
};
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    time::Duration,
};

mod scene;
use scene::{SceneController, SceneState};
mod image_grab;
use image_grab::{ImageCopyPlugin, ImageToSave, MainWorldReceiver};

// Parameters of resulting image
struct AppConfig {
    width: u32,
    height: u32,
    single_image: bool,
}

fn main() {
    let config = AppConfig {
        width: 1920,
        height: 1080,
        single_image: true,
    };

    // setup frame capture
    App::new()
        .insert_resource(SceneController::new(
            config.width,
            config.height,
            config.single_image,
        ))
        .insert_resource(ClearColor(Color::srgb_u8(0, 0, 0)))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                // Not strictly necessary, as the inclusion of ScheduleRunnerPlugin below
                // replaces the bevy_winit app runner and so a window is never created.
                .set(WindowPlugin {
                    primary_window: None,
                    // Don’t automatically exit due to having no windows.
                    // Instead, the code in `update()` will explicitly produce an `AppExit` event.
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                }),
        )
        .add_plugins(ImageCopyPlugin)
        // ScheduleRunnerPlugin provides an alternative to the default bevy_winit app runner, which
        // manages the loop without creating a window.
        .add_plugins(ScheduleRunnerPlugin::run_loop(
            // Run 60 times per second.
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .init_resource::<SceneController>()
        .add_systems(Startup, setup)
        .add_systems(PostUpdate, save_frame)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    render_device: Res<RenderDevice>,
) {
    let render_target = ImageCopyPlugin::setup_render_target(
        &mut commands,
        &mut images,
        &render_device,
        &mut scene_controller,
        // pre_roll_frames should be big enough for full scene render,
        // but the bigger it is, the longer example will run.
        // To visualize stages of scene rendering change this param to 0
        // and change AppConfig::single_image to false in main
        // Stages are:
        // 1. Transparent image
        // 2. Few black box images
        // 3. Fully rendered scene images
        // Exact number depends on device speed, device load and scene size
        40,
        "main_scene".into(),
    );

    // Scene example for non black box picture
    // circular base
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    // cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    // light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.spawn((
        Camera3d::default(),
        render_target,
        Tonemapping::None,
        Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

// Takes from channel image content sent from render world and saves it to disk
fn save_frame(
    images_to_save: Query<&ImageToSave>,
    receiver: Res<MainWorldReceiver>,
    mut images: ResMut<Assets<Image>>,
    mut scene_controller: ResMut<SceneController>,
    mut app_exit_writer: MessageWriter<AppExit>,
    mut file_number: Local<u32>,
) {
    if let SceneState::Render(n) = scene_controller.state {
        if n < 1 {
            // We don't want to block the main world on this,
            // so we use try_recv which attempts to receive without blocking
            let mut image_data = Vec::new();
            while let Ok(data) = receiver.try_recv() {
                // image generation could be faster than saving to fs,
                // that's why use only last of them
                image_data = data;
            }
            if !image_data.is_empty() {
                for image in images_to_save.iter() {
                    // Fill correct data from channel to image
                    let img_bytes = images.get_mut(image.id()).unwrap();

                    // We need to ensure that this works regardless of the image dimensions
                    // If the image became wider when copying from the texture to the buffer,
                    // then the data is reduced to its original size when copying from the buffer to the image.
                    let row_bytes = img_bytes.width() as usize
                        * img_bytes.texture_descriptor.format.pixel_size().unwrap();
                    let aligned_row_bytes = RenderDevice::align_copy_bytes_per_row(row_bytes);
                    if row_bytes == aligned_row_bytes {
                        img_bytes.data.as_mut().unwrap().clone_from(&image_data);
                    } else {
                        // shrink data to original image size
                        img_bytes.data = Some(
                            image_data
                                .chunks(aligned_row_bytes)
                                .take(img_bytes.height() as usize)
                                .flat_map(|row| &row[..row_bytes.min(row.len())])
                                .cloned()
                                .collect(),
                        );
                    }

                    // Create RGBA Image Buffer
                    let img = match img_bytes.clone().try_into_dynamic() {
                        Ok(img) => img.to_rgba8(),
                        Err(e) => panic!("Failed to create image buffer {e:?}"),
                    };

                    // Prepare directory for images, test_images in bevy folder is used here for example
                    // You should choose the path depending on your needs
                    let images_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images");
                    info!("Saving image to: {images_dir:?}");
                    std::fs::create_dir_all(&images_dir).unwrap();

                    // Choose filename starting from 000.png
                    let image_path = images_dir.join(format!("{:03}.png", file_number.deref()));
                    *file_number.deref_mut() += 1;

                    // Finally saving image to file, this heavy blocking operation is kept here
                    // for example simplicity, but in real app you should move it to a separate task
                    if let Err(e) = img.save(image_path) {
                        panic!("Failed to save image: {e}");
                    };
                }
                if scene_controller.single_image {
                    app_exit_writer.write(AppExit::Success);
                }
            }
        } else {
            // clears channel for skipped frames
            while receiver.try_recv().is_ok() {}
            scene_controller.state = SceneState::Render(n - 1);
        }
    }
}
