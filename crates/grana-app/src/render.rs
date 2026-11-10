use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use grana_animation::Timeline;
use grana_display::{Camera, OutputSize};
use grana_offline::{Destination, Offline, Outcome, Plan, Pose, Task};
use grana_pipeline::Channels;
use grana_project::ViewState;
use grana_scene::Scene;
use grana_session::{Kind as MessageKind, Session};

use crate::bricks::ImageBricks;

const OPEN_LIMIT: Duration = Duration::from_secs(600);
const OPEN_POLL: Duration = Duration::from_millis(10);

pub struct Request {
    pub input: PathBuf,
    pub output: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub time: Option<u32>,
    pub view: Option<String>,
    pub transparent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DestinationKind {
    Image,
    Figure,
    Movie,
    Sequence,
}

pub fn run(request: &Request) -> Result<PathBuf, String> {
    let kind = classify(&request.output)?;
    if request.time.is_some() && !matches!(kind, DestinationKind::Image | DestinationKind::Figure) {
        return Err(
            "--time names one frame, so it only applies to a still image or a figure".to_string(),
        );
    }
    if request.output.exists() && kind == DestinationKind::Sequence {
        return Err(format!(
            "{} already exists, and a frame sequence writes a new folder",
            request.output.display()
        ));
    }
    if let Some(parent) = request.output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("failed to create the folder {}: {error}", parent.display())
        })?;
    }
    let mut session = open(&request.input)?;
    let view = named(&mut session, request.view.as_deref(), &request.input)?;
    let scene = session.scene().expect("the opened session holds a scene");
    let image = session.image().expect("the opened session holds an image");
    let mut camera = Camera::new(35.0);
    let mut timeline = session.timeline().clone();
    if let Some(view) = &view {
        camera.restore(view.camera);
        timeline = view.timeline.clone();
    }
    let width = request.width.unwrap_or_else(|| default_width(&view, kind));
    let even = kind == DestinationKind::Movie;
    let size = match request.height {
        Some(height) => OutputSize::of(width, height),
        None => OutputSize::of_width(width),
    };
    let size = if even { size.even() } else { size };
    if view.is_none() {
        camera.fit(scene.dataset.world_size(), aspect(size.to_array()));
    }
    let plan = Job {
        kind,
        output: &request.output,
        camera: &camera,
        timeline: &timeline,
        scene,
        request,
        size,
    }
    .plan()?;
    let channels = Channels::render(image, scene);
    let mut offline = Offline::headless()?;
    let task = Task {
        plan,
        scene: scene.snapshot().into_scene(),
        bricks: Arc::new(ImageBricks::new(channels)),
    };
    let cancel = AtomicBool::new(false);
    let mut shown = u32::MAX;
    let mut progress = |frame: u32, total: u32| {
        if total > 1 && frame != shown {
            shown = frame;
            eprint!("\rrendered {frame} of {total}");
        }
    };
    let outcome = offline.run(task, &cancel, &mut progress);
    if shown != u32::MAX {
        eprintln!();
    }
    match outcome {
        Outcome::Complete { path } => Ok(path),
        Outcome::Failed { detail } => Err(detail),
        Outcome::Cancelled => Err("the render was cancelled".to_string()),
    }
}

struct Job<'a> {
    kind: DestinationKind,
    output: &'a Path,
    camera: &'a Camera,
    timeline: &'a Timeline,
    scene: &'a Scene,
    request: &'a Request,
    size: OutputSize,
}

impl Job<'_> {
    fn plan(&self) -> Result<Plan, String> {
        let camera = self.camera.state();
        let (destination, poses, fps) = match self.kind {
            DestinationKind::Image => {
                let frames = self.scene.frame_count();
                let time = self.request.time.unwrap_or(self.scene.time());
                if time >= frames {
                    return Err(format!(
                        "frame {time} lies outside the dataset's {frames} frames"
                    ));
                }
                (
                    Destination::Image {
                        path: self.output.to_path_buf(),
                    },
                    vec![Pose { time, camera }],
                    0,
                )
            }
            DestinationKind::Figure => {
                let frames = self.scene.frame_count();
                let time = self.request.time.unwrap_or(self.scene.time());
                if time >= frames {
                    return Err(format!(
                        "frame {time} lies outside the dataset's {frames} frames"
                    ));
                }
                (
                    Destination::Figure {
                        path: self.output.to_path_buf(),
                    },
                    vec![Pose { time, camera }],
                    0,
                )
            }
            DestinationKind::Movie => {
                let poses = grana_offline::poses(self.timeline);
                if poses.is_empty() {
                    return Err("the timeline holds no keyframe to film".to_string());
                }
                (
                    Destination::Movie {
                        path: self.output.to_path_buf(),
                    },
                    poses,
                    self.timeline.fps(),
                )
            }
            DestinationKind::Sequence => {
                let frames = self.scene.frame_count();
                if frames == 0 {
                    return Err("the dataset holds no frame to render".to_string());
                }
                let poses = (0..frames).map(|time| Pose { time, camera }).collect();
                (
                    Destination::Sequence {
                        directory: self.output.to_path_buf(),
                        stem: grana_session::slug(self.scene.dataset.name()),
                    },
                    poses,
                    0,
                )
            }
        };
        Ok(Plan {
            destination,
            size: self.size,
            transparent: self.request.transparent,
            poses,
            fps,
            region: self.scene.region,
        })
    }
}

fn classify(output: &Path) -> Result<DestinationKind, String> {
    if output.is_dir() {
        return Ok(DestinationKind::Sequence);
    }
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png" | "tif" | "tiff") => Ok(DestinationKind::Image),
        Some("svg" | "pdf") => Ok(DestinationKind::Figure),
        Some("mp4" | "mov") => Ok(DestinationKind::Movie),
        None => Ok(DestinationKind::Sequence),
        Some(other) => Err(format!(
            "a render writes PNG, OME-TIFF, SVG, PDF, MP4, MOV or a frame folder, not `.{other}`"
        )),
    }
}

fn open(input: &Path) -> Result<Session, String> {
    if !input.exists() {
        return Err(format!("{} does not exist", input.display()));
    }
    let mut session = Session::new();
    let project = input
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("grana"));
    if project {
        session.begin_project(input.to_path_buf());
    } else {
        session.begin_load(input.to_path_buf());
    }
    let start = Instant::now();
    loop {
        session.poll();
        if session.scene().is_some() {
            return Ok(session);
        }
        if let Some(message) = session.message()
            && message.kind() == MessageKind::Error
        {
            return Err(message.text().to_string());
        }
        if start.elapsed() >= OPEN_LIMIT {
            return Err(format!(
                "{} did not open within {} seconds",
                input.display(),
                OPEN_LIMIT.as_secs()
            ));
        }
        std::thread::sleep(OPEN_POLL);
    }
}

fn named(
    session: &mut Session,
    name: Option<&str>,
    input: &Path,
) -> Result<Option<ViewState>, String> {
    match name {
        Some(name) => session
            .views()
            .iter()
            .find(|view| view.name == name)
            .map(|view| view.view.clone())
            .ok_or_else(|| format!("{} holds no view named `{name}`", input.display()))
            .map(Some),
        None => Ok(session.take_view()),
    }
}

fn default_width(view: &Option<ViewState>, kind: DestinationKind) -> u32 {
    view.as_ref().map_or(OutputSize::DEFAULT_WIDTH, |view| {
        let width = match kind {
            DestinationKind::Movie => view.export.movie,
            DestinationKind::Image | DestinationKind::Figure | DestinationKind::Sequence => {
                view.export.image
            }
        };
        if width == 0 {
            OutputSize::DEFAULT_WIDTH
        } else {
            width
        }
    })
}

fn aspect(size: [u32; 2]) -> f32 {
    size[0].max(1) as f32 / size[1].max(1) as f32
}
