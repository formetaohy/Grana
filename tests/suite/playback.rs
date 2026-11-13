use std::time::{Duration, Instant};

use grana_display::Camera;
use grana_engine::{Display, Engine};
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
    let mut engine = Engine::new(gpu.device(), gpu.queue(), wgpu::TextureFormat::Rgba8Unorm);
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
