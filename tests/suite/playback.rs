use std::time::{Duration, Instant};

use grana_display::Camera;
use grana_engine::{Display, Engine, SAMPLE_LIMIT};
use grana_session::Session;

use crate::support::gpu;

const PLAYBACK: Duration = Duration::from_millis(90);

fn play(display: Display, seconds: f64, advancing: bool) -> (u32, usize) {
    let Some(gpu) = gpu() else {
        return (0, 0);
    };
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets/data/zebrafish_neuromast.ome.tif");
    let mut session = Session::new();
    session.begin_load(path);
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
        if let Some(requests) = engine.poll_requests(gpu.device(), current)
            && session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time)
        {
            let mut bricks = requests.volume.clone();
            bricks.extend_from_slice(&requests.prefetch);
            session.request(requests.time, &bricks);
        }
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            engine.upload(gpu.queue(), &bricks);
        }
        if advancing
            && engine.info().samples > 0
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
                    display,
                    level_cap: 99,
                    overlays: &[],
                },
            )
            .expect("the viewport draws");
        draws += 1;
        std::thread::sleep(Duration::from_millis(2));
    }
    (time, draws)
}

#[test]
fn playback_advances_through_the_time_series() {
    let (time, draws) = play(Display::Playback, 6.0, true);
    assert!(
        time >= 8,
        "playback advanced {time} frames in six seconds with {draws} draws"
    );
}

#[test]
fn a_still_resolves_the_frame_it_shows() {
    let (time, draws) = play(Display::Still, 3.0, false);
    assert_eq!(
        time, 0,
        "a still moved away from its frame in {draws} draws"
    );
}

#[test]
fn an_interactive_frame_lands_at_the_full_viewport() {
    let Some(gpu) = gpu() else {
        return;
    };
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets/data/zebrafish_neuromast.ome.tif");
    let mut session = Session::new();
    session.begin_load(path);
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
    let scene = session.scene().expect("a scene");
    let mut engine = Engine::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    engine.set_dataset(gpu.device(), scene.dataset.pyramid());
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), 1.0);
    let size = [320, 320];
    let draw = |engine: &mut Engine, session: &mut Session, display: Display| {
        let current = session.scene().expect("a scene").time();
        if let Some(requests) = engine.poll_requests(gpu.device(), current) {
            let mut bricks = requests.volume.clone();
            bricks.extend_from_slice(&requests.prefetch);
            session.request(requests.time, &bricks);
        }
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            engine.upload(gpu.queue(), &bricks);
        }
        engine
            .draw(
                gpu.device(),
                gpu.queue(),
                grana_engine::View {
                    scene: session.scene().expect("a scene"),
                    camera: &camera,
                    size,
                    display,
                    level_cap: 99,
                    overlays: &[],
                },
            )
            .expect("the viewport draws");
    };
    draw(&mut engine, &mut session, Display::Interactive);
    assert_eq!(
        engine.info().samples,
        1,
        "an interactive frame renders one sample"
    );
    let revision = engine.image_revision();
    draw(&mut engine, &mut session, Display::Interactive);
    assert_eq!(
        engine.image_revision(),
        revision,
        "an unchanged interactive request renders nothing"
    );
    assert!(
        engine.request_pick(gpu.device(), gpu.queue(), [size[0] - 20, size[1] - 20]),
        "an interactive frame covers the whole viewport"
    );
    while engine.pick_pending() {
        engine.poll_pick(gpu.device());
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..SAMPLE_LIMIT * 4 {
        draw(&mut engine, &mut session, Display::Still);
        if engine.info().samples >= SAMPLE_LIMIT {
            break;
        }
    }
    assert!(
        engine.info().samples >= SAMPLE_LIMIT,
        "a still view keeps sampling to {} samples, reached {}",
        SAMPLE_LIMIT,
        engine.info().samples
    );
}
