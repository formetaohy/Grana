use std::time::{Duration, Instant};

use glam::Vec3;
use grana_display::Camera;
use grana_engine::{Cache, Engine, Lens};
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{
    IntensityTable, SampleStatistics, Spot, SpotSet, SurfaceSet, SurfaceStatistics, Truncation,
};
use grana_render::Delivery;
use grana_scene::{Dataset, ObjectFrame, ObjectKind, Scene};
use grana_session::Session;
use grana_volume::{
    Calibration, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing, Statistics, VoxelFormat,
    VoxelSemantics,
};

use crate::support::{budget, gpu, spot_set};

const PLAYBACK: Duration = Duration::from_millis(90);

fn play(delivery: Delivery, seconds: f64, advancing: bool) -> Option<(u32, usize)> {
    let gpu = gpu()?;
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets/data/zebrafish_neuromast.ome.tif");
    let mut session = Session::new();
    load(&mut session, path);
    let mut waited = Duration::ZERO;
    while session.scene().is_none() {
        session.poll();
        std::thread::sleep(Duration::from_millis(10));
        waited += Duration::from_millis(10);
        assert!(
            waited < Duration::from_secs(120),
            "the dataset never opened"
        );
    }
    let mut engine = Engine::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        256 << 20,
        gpu.pipelines(),
    );
    engine
        .set_dataset(
            gpu.device(),
            session.scene().expect("a scene").dataset.pyramid(),
            budget(256 << 20, [320, 320]),
            [320, 320],
        )
        .expect("the test budget holds the series");
    let mut camera = Camera::new(35.0);
    camera.fit(session.scene().expect("a scene").dataset.world_size(), 1.0);
    let frames = session.scene().expect("a scene").frame_count();
    let start = Instant::now();
    let mut last_advance = Some(Instant::now());
    let mut time = 0u32;
    let mut draws = 0usize;
    while start.elapsed().as_secs_f64() < seconds {
        session.poll();
        let current = session.scene().expect("a scene").time();
        if let Some(requests) = engine
            .poll_requests(gpu.device(), current, session.pending_requests())
            .expect("the demand polls")
            && session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time)
        {
            session.request(
                requests.time,
                requests.epoch,
                &requests.volume,
                &requests.prefetch,
            );
        }
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            engine.upload(&bricks);
        }
        if advancing
            && engine.info().drawn
            && last_advance.is_none_or(|last| last.elapsed() >= PLAYBACK)
        {
            time = (current + 1) % frames;
            session.set_time(time);
            last_advance = Some(Instant::now());
        }
        engine
            .draw(
                gpu.device(),
                gpu.queue(),
                grana_engine::View {
                    scene: session.scene().expect("a scene"),
                    camera: &camera,
                    size: [320, 320],
                    delivery,
                    overlays: &[],
                },
            )
            .expect("the viewport draws");
        draws += 1;
        std::thread::sleep(Duration::from_millis(2));
    }
    Some((time, draws))
}

#[test]
fn playback_advances_through_the_time_series() {
    let Some((time, draws)) = play(Delivery::Awaited, 6.0, true) else {
        return;
    };
    assert!(
        time >= 8,
        "playback advanced {time} frames in six seconds with {draws} draws"
    );
}

#[test]
fn a_zoomed_out_proxy_prepares_only_the_level_it_draws() {
    let mut scene = proxy_scene(2);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::new(400.0, 400.0, 8.0), 1.0);
    let mut cache = Cache::new();
    for time in 0..2 {
        scene.set_time(time);
        let targets = grana_engine::plan_with(
            &scene,
            Some(Lens {
                camera: &camera,
                height: 320,
            }),
            &mut cache,
        );
        let rendered = grana_engine::frame(&scene, &targets, &mut cache, 8 << 20)
            .expect("the proxy scene renders");
        assert_eq!(
            rendered.splats.len(),
            1,
            "frame {time} builds every proxy level instead of the one it draws"
        );
        assert!(rendered.splats[0].active(), "the drawn level is active");
        assert!(
            rendered.splats[0].level() >= 1,
            "a zoomed-out aggregated set draws a proxy level, not the raw spots"
        );
        assert!(!rendered.splats[0].is_empty(), "the proxy holds cells");
    }
}

#[test]
fn a_tight_splat_residency_coarsens_the_drawn_tier() {
    let scene = proxy_scene(1);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::new(400.0, 400.0, 8.0), 1.0);
    let lens = Lens {
        camera: &camera,
        height: 320,
    };
    let mut cache = Cache::new();
    let targets = grana_engine::plan_with(&scene, Some(lens), &mut cache);
    let whole = grana_engine::frame(&scene, &targets, &mut cache, u64::MAX)
        .expect("the unconstrained proxy renders");
    let wanted = whole.splats[0].level();
    let bytes = whole.splats[0].bytes();
    let mut cache = Cache::new();
    let targets = grana_engine::plan_with(&scene, Some(lens), &mut cache);
    let fitted = grana_engine::frame(&scene, &targets, &mut cache, bytes / 2)
        .expect("the constrained proxy renders");
    assert!(
        fitted.splats[0].level() > wanted,
        "a residency of {} bytes keeps the wanted level {wanted}",
        bytes / 2
    );
    assert!(
        fitted.splats[0].bytes() <= bytes / 2,
        "the clamped level holds {} bytes over the {} it may",
        fitted.splats[0].bytes(),
        bytes / 2
    );
}

#[test]
fn a_budget_change_that_keeps_the_mesh_level_keeps_the_topology() {
    let scene = cube_scene();
    let mut cache = Cache::new();
    let views = [4.0, 5.0].map(|distance| {
        let mut camera = Camera::new(35.0);
        let mut state = camera.state();
        state.distance = distance;
        camera.restore(state);
        let targets = grana_engine::plan_with(
            &scene,
            Some(Lens {
                camera: &camera,
                height: 320,
            }),
            &mut cache,
        );
        let rendered =
            grana_engine::frame(&scene, &targets, &mut cache, u64::MAX).expect("the cube renders");
        (targets, rendered.versions)
    });
    assert_ne!(
        views[0].0, views[1].0,
        "the two distances name one triangle budget"
    );
    assert_eq!(
        views[0].1.topology(),
        views[1].1.topology(),
        "a budget change that keeps the mesh level renames the topology"
    );
}

fn cube_scene() -> Scene {
    let level = Level::new(Extent::new(8, 8, 8), Spacing::new(1.0, 1.0, 1.0));
    let intensity = Intensity::new(0.0, 1.0);
    let channels = vec![ChannelInfo::new(
        "signal",
        [1.0, 1.0, 1.0],
        Statistics::declared(intensity, VoxelSemantics::Scalar),
        VoxelFormat::U8,
    )];
    let pyramid = Pyramid::new(vec![level], 1, channels, 1, 1.0, Calibration::Recorded);
    let vertices = (0..8u32)
        .map(|bits| {
            let at = [0, 1, 2].map(|axis| if (bits >> axis) & 1 == 0 { -0.5 } else { 0.5 });
            MeshVertex::new(at, [0.0, 0.0, 1.0], 0)
        })
        .collect::<Vec<_>>();
    let indices = vec![
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6,
        1, 3, 5, 3, 7, 5,
    ];
    let statistics = SurfaceStatistics::new(
        8,
        1.0,
        6.0,
        [0.0; 3],
        Bounds::new([-0.5; 3], [0.5; 3]),
        None,
        Truncation::Complete,
    );
    let surfaces = SurfaceSet::new(
        0,
        crate::support::ids(1),
        Mesh::new(vertices, indices),
        vec![statistics],
        IntensityTable::new(
            1,
            vec![Some(SampleStatistics::new(0.5, 0.0, 1.0, 0.0, 1.0))],
        ),
    );
    let mut scene = Scene::new(Dataset::synthetic("cube", pyramid));
    let set = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(set, 0, ObjectFrame::Surfaces(surfaces));
    scene
}

fn proxy_scene(frames: u32) -> Scene {
    let level = Level::new(Extent::new(160, 160, 8), Spacing::new(1.0, 1.0, 1.0));
    let intensity = Intensity::new(0.0, 1.0);
    let channels = vec![ChannelInfo::new(
        "signal",
        [1.0, 1.0, 1.0],
        Statistics::declared(intensity, VoxelSemantics::Scalar),
        VoxelFormat::F32,
    )];
    let pyramid = Pyramid::new(vec![level], 1, channels, frames, 1.0, Calibration::Recorded);
    let mut scene = Scene::new(Dataset::synthetic("proxy", pyramid));
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..frames {
        scene.install(set, time, ObjectFrame::from_spots(crowd(time)));
    }
    scene
}

fn crowd(time: u32) -> SpotSet {
    let spots = (0..600u32)
        .map(|index| {
            let center = [
                (index % 30) as f32 * 4.0 + 2.0,
                (index / 30) as f32 * 4.0 + 2.0,
                4.0 + time as f32,
            ];
            Spot::new(center, 1.0, 0.5)
        })
        .collect::<Vec<_>>();
    let statistics = vec![Some(SampleStatistics::new(20.0, 10.0, 30.0, 0.0, 20.0)); spots.len()];
    spot_set(0, spots, IntensityTable::new(1, statistics))
}

fn load(session: &mut Session, path: std::path::PathBuf) {
    assert!(
        matches!(session.begin_load(path), grana_session::Transfer::Started),
        "the session starts opening the dataset"
    );
}
