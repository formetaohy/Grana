use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use grana_animation::Timeline;
use grana_display::{PaneLayout, Panes};
use grana_engine::Panels;
use grana_offline::{Destination, Offline, Outcome, Plan, Pose, Task};
use grana_pipeline::Channels;
use grana_project::ViewState;
use grana_scene::Scene;
use grana_session::{Kind as MessageKind, Session};

use crate::bricks::ImageBricks;

const OPEN_LIMIT: Duration = Duration::from_secs(600);
const OPEN_POLL: Duration = Duration::from_millis(10);
const DEFAULT_WIDTH: u32 = 1920;
const ASPECT: [u32; 2] = [16, 9];

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
    Movie,
    Sequence,
}

pub fn run(request: &Request) -> Result<PathBuf, String> {
    let kind = classify(&request.output)?;
    if request.time.is_some() && kind != DestinationKind::Image {
        return Err("--time names one frame, so it only applies to a still image".to_string());
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
    let mut panes = Panes::new();
    let mut timeline = session.timeline().clone();
    if let Some(view) = &view {
        panes.restore(view.panes);
        panes.set_layout(view.layout);
        panes.set_split(view.split);
        panes.set_row(view.row);
        timeline = view.timeline.clone();
    } else {
        panes.set_layout(PaneLayout::Single);
    }
    let width = request.width.unwrap_or_else(|| default_width(&view, kind));
    let height = request.height.unwrap_or_else(|| default_height(width));
    if width == 0 || height == 0 {
        return Err("a render covers at least one pixel".to_string());
    }
    let panels = Panels::of(&panes, [width, height]);
    let regions = panels.export(width, kind == DestinationKind::Movie);
    if view.is_none() {
        panes.fit(
            scene.dataset.world_size(),
            scene.dataset.voxel_size(),
            std::array::from_fn(|index| {
                regions.panes[index].map_or(1.0, |pane| aspect(pane.region))
            }),
        );
    }
    let plan = Job {
        kind,
        output: &request.output,
        panes: &panes,
        timeline: &timeline,
        scene,
        request,
        panels,
        width,
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
    panes: &'a Panes,
    timeline: &'a Timeline,
    scene: &'a Scene,
    request: &'a Request,
    panels: Panels,
    width: u32,
}

impl Job<'_> {
    fn plan(&self) -> Result<Plan, String> {
        let views = self.panes.views();
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
                    vec![Pose { time, views }],
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
                let poses = (0..frames).map(|time| Pose { time, views }).collect();
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
            panels: self.panels,
            width: self.width,
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
        Some("mp4" | "mov") => Ok(DestinationKind::Movie),
        None => Ok(DestinationKind::Sequence),
        Some(other) => Err(format!(
            "a render writes PNG, OME-TIFF, MP4, MOV or a frame folder, not `.{other}`"
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
    view.as_ref().map_or(DEFAULT_WIDTH, |view| {
        let width = match kind {
            DestinationKind::Movie => view.export.movie,
            DestinationKind::Image | DestinationKind::Sequence => view.export.image,
        };
        if width == 0 { DEFAULT_WIDTH } else { width }
    })
}

fn default_height(width: u32) -> u32 {
    (u64::from(width) * u64::from(ASPECT[1]) / u64::from(ASPECT[0])).max(1) as u32
}

fn aspect(region: [u32; 4]) -> f32 {
    region[2].max(1) as f32 / region[3].max(1) as f32
}
