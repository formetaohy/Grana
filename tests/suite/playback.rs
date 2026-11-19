use std::time::{Duration, Instant};

use glam::Vec3;
use grana_display::Camera;
use grana_engine::{Cache, Engine, Lens};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet};
use grana_render::Delivery;
use grana_scene::{Dataset, ObjectFrame, ObjectKind, Scene};
use grana_session::Session;
use grana_volume::{ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing, VoxelFormat};

use crate::support::gpu;

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
        gpu.pipelines(),
    );
    engine.set_dataset(
        gpu.device(),
        session.scene().expect("a scene").dataset.pyramid(),
    );
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
            .poll_requests(gpu.device(), current)
            .expect("the demand polls")
            && session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time)
        {
            session.request(requests.time, &requests.volume, &requests.prefetch);
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
                    level_cap: 99,
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
    let level = Level::new(Extent::new(160, 160, 8), Spacing::new(1.0, 1.0, 1.0));
    let intensity = Intensity::new(0.0, 1.0);
    let channels = vec![ChannelInfo::new(
        "signal",
        [1.0, 1.0, 1.0],
        intensity,
        intensity,
        VoxelFormat::F32,
    )];
    let pyramid = Pyramid::new(vec![level], 1, channels, 2, 1.0);
    let mut scene = Scene::new(Dataset::synthetic("proxy", pyramid));
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..2 {
        scene.install(set, time, ObjectFrame::from_spots(crowd(time)));
    }
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
        let rendered =
            grana_engine::frame(&scene, &targets, &mut cache).expect("the proxy scene renders");
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
    let statistics = vec![SampleStatistics::new(20.0, 10.0, 30.0, 0.0, 20.0); spots.len()];
    SpotSet::new(0, spots, IntensityTable::new(1, statistics))
}

fn load(session: &mut Session, path: std::path::PathBuf) {
    assert!(
        matches!(session.begin_load(path), grana_session::Transfer::Started),
        "the session starts opening the dataset"
    );
}
