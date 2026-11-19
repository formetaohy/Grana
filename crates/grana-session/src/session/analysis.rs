use super::*;

use super::localization::still_current;
use grana_calibration::Settings;
use grana_pipeline::run::{GrowRequest, SegmentRequest};

const CALIBRATION_ITERATIONS: u32 = 10;

enum DetectPlan {
    Grow {
        channels: Arc<grana_pipeline::Channels>,
        request: GrowRequest,
    },
    Segment {
        channels: Arc<grana_pipeline::Channels>,
        request: SegmentRequest,
    },
    Detect {
        channels: Arc<grana_pipeline::Channels>,
        request: DetectRequest,
    },
}

impl DetectPlan {
    fn clears_objects(&self) -> bool {
        matches!(self, Self::Grow { .. })
    }

    fn into_analysis(self, set: ObjectSetId) -> Analysis {
        let job = Job::Detect(set);
        match self {
            Self::Grow { channels, request } => crate::grow(job, channels, request),
            Self::Segment { channels, request } => crate::segment(job, channels, request),
            Self::Detect { channels, request } => crate::detect(job, channels, request),
        }
    }
}

impl Session {
    pub(super) fn sample_profile(&mut self, index: usize) {
        let prepared = self.scene.as_ref().and_then(|scene| {
            let annotation = scene.annotations.get(index)?;
            let end = annotation.kind().end()?;
            let line = grana_profile::Line::new(annotation.kind().anchor(), end);
            let names = scene
                .dataset
                .channels()
                .iter()
                .map(|channel| channel.name().to_string())
                .collect::<Vec<_>>();
            let spacing = scene.dataset.voxel_size().min_element().max(1.0e-4);
            Some((line, names, spacing, scene.time()))
        });
        let Some((line, names, spacing, time)) = prepared else {
            self.message = Some(Message::error(
                "choose a distance annotation to sample a profile",
            ));
            return;
        };
        let Some(channels) = self.analysis_channels() else {
            self.message = Some(Message::error("open a dataset before sampling a profile"));
            return;
        };
        let source = grana_pipeline::ProfileSource::new(&channels, time);
        match source.profile(line, spacing, &names) {
            Ok(profile) => {
                let samples = profile.samples().len();
                if let Some(scene) = self.scene.as_mut() {
                    scene.annotations.set_profile(index, Some(profile));
                    self.message = Some(Message::info(format!(
                        "sampled {samples} points of {} channels along {:.2} µm",
                        names.len(),
                        line.length(),
                    )));
                }
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn analysable(&mut self) -> bool {
        if self.scene.is_none() {
            self.message = Some(Message::error("open a dataset before analysing it"));
            return false;
        }
        true
    }

    pub(super) fn poll_analysis(&mut self) -> bool {
        enum Update {
            Progress(u32, usize, usize),
            Frame(u32, ObjectFrame),
            Localization {
                pixel: f32,
                width: u32,
                height: u32,
                volume: bool,
                photons: bool,
                alignment: grana_scene::LocalizationAlignment,
            },
            Tracks(TrackSet),
            Relation(u32, grana_object::RelationFrame),
            Coloc(Box<grana_measure::ColocSeries>),
            ObjectColoc(Box<grana_measure::ObjectColoc>),
            LocalizationReport {
                set: ObjectSetId,
                revision: Revision,
                report: Box<grana_analysis::localization::LocalizationReport>,
            },
            LocalizationClusters {
                set: ObjectSetId,
                revision: Revision,
                clusters: Box<grana_analysis::localization::Clusters>,
            },
            Drift(grana_scene::Drift),
            DriftSummary {
                segments: usize,
                fallback: usize,
            },
            Registration(grana_scene::Registration),
            Fiducial {
                channels: usize,
                pairs: usize,
                residual: f32,
            },
            Calibration {
                source: usize,
                psf: Box<grana_optics::MeasuredPsf>,
                beads: usize,
                iterations: u32,
            },
            Done,
            Failed(String),
        }
        let mut repaint = false;
        let mut updates: Vec<(usize, Job, Update)> = Vec::new();
        for (index, analysis) in self.analysis.iter().enumerate() {
            let job = analysis.job();
            loop {
                match analysis.poll() {
                    Poll::Event(Event::Progress { time, done, total }) => {
                        updates.push((index, job, Update::Progress(time, done, total)));
                    }
                    Poll::Event(Event::Frame { time, frame }) => {
                        updates.push((index, job, Update::Frame(time, frame)));
                    }
                    Poll::Event(Event::Localization {
                        pixel,
                        width,
                        height,
                        volume,
                        photons,
                        alignment,
                    }) => {
                        updates.push((
                            index,
                            job,
                            Update::Localization {
                                pixel,
                                width,
                                height,
                                volume,
                                photons,
                                alignment,
                            },
                        ));
                    }
                    Poll::Event(Event::Tracks(set)) => {
                        updates.push((index, job, Update::Tracks(set)));
                    }
                    Poll::Event(Event::Relation { time, frame }) => {
                        updates.push((index, job, Update::Relation(time, frame)));
                    }
                    Poll::Event(Event::Coloc { series, .. }) => {
                        updates.push((index, job, Update::Coloc(series)));
                    }
                    Poll::Event(Event::ObjectColoc { coloc }) => {
                        updates.push((index, job, Update::ObjectColoc(coloc)));
                    }
                    Poll::Event(Event::LocalizationReport {
                        set,
                        revision,
                        report,
                    }) => {
                        updates.push((
                            index,
                            job,
                            Update::LocalizationReport {
                                set,
                                revision,
                                report,
                            },
                        ));
                    }
                    Poll::Event(Event::LocalizationClusters {
                        set,
                        revision,
                        clusters,
                    }) => {
                        updates.push((
                            index,
                            job,
                            Update::LocalizationClusters {
                                set,
                                revision,
                                clusters,
                            },
                        ));
                    }
                    Poll::Event(Event::Drift(drift)) => {
                        updates.push((index, job, Update::Drift(drift)));
                    }
                    Poll::Event(Event::DriftSummary { segments, fallback }) => {
                        updates.push((index, job, Update::DriftSummary { segments, fallback }));
                    }
                    Poll::Event(Event::Registration(registration)) => {
                        updates.push((index, job, Update::Registration(registration)));
                    }
                    Poll::Event(Event::Fiducial {
                        channels,
                        pairs,
                        residual,
                    }) => {
                        updates.push((
                            index,
                            job,
                            Update::Fiducial {
                                channels,
                                pairs,
                                residual,
                            },
                        ));
                    }
                    Poll::Event(Event::Calibration {
                        source,
                        psf,
                        beads,
                        iterations,
                    }) => {
                        updates.push((
                            index,
                            job,
                            Update::Calibration {
                                source,
                                psf,
                                beads,
                                iterations,
                            },
                        ));
                    }
                    Poll::Event(Event::Done) => {
                        updates.push((index, job, Update::Done));
                        break;
                    }
                    Poll::Event(Event::Failed(detail)) => {
                        updates.push((index, job, Update::Failed(detail)));
                        break;
                    }
                    Poll::Empty => break,
                    Poll::Closed => {
                        updates.push((
                            index,
                            job,
                            Update::Failed("the analysis worker stopped".to_string()),
                        ));
                        break;
                    }
                }
            }
        }
        let mut removed = Vec::new();
        let mut failed = Vec::new();
        let mut rolled_back = Vec::new();
        let mut detected = Vec::new();
        let mut imported = Vec::new();
        let mut recutted = Vec::new();
        let mut finished = Vec::new();
        let mut restarted = Vec::new();
        let mut restarted_imports = Vec::new();
        let mut restarted_relations = Vec::new();
        let mut retracked = Vec::new();
        let inputs = self
            .analysis
            .iter()
            .map(|analysis| analysis.input().cloned())
            .collect::<Vec<_>>();
        for (index, job, update) in updates {
            repaint = true;
            match update {
                Update::Progress(time, done, total) => {
                    self.record_progress(job, time, done, total);
                }
                Update::Frame(time, frame) => {
                    if let Job::Detect(set) | Job::Import(set) | Job::Recut(set) = job {
                        let stale = match job {
                            Job::Detect(_) => {
                                self.detect_input(set).as_ref() != inputs[index].as_ref()
                            }
                            Job::Import(_) => {
                                self.import_input(set).as_ref() != inputs[index].as_ref()
                            }
                            _ => false,
                        };
                        if !stale && let Some(scene) = self.scene.as_mut() {
                            scene.install(set, time, frame);
                        }
                    }
                }
                Update::Localization {
                    pixel,
                    width,
                    height,
                    volume,
                    photons,
                    alignment,
                } => {
                    if let Job::Import(set) = job {
                        if self.import_input(set).as_ref() != inputs[index].as_ref() {
                            continue;
                        }
                        if let Some(scene) = self.scene.as_mut() {
                            let spec = grana_object::LocalizationSpec::cover(scene.frame_count());
                            scene.set_localization(
                                set,
                                grana_scene::LocalizationSource::new(
                                    pixel, width, height, volume, photons, spec, alignment,
                                ),
                            );
                        }
                    }
                }
                Update::Tracks(tracks) => {
                    let set = match job {
                        Job::Track(set) | Job::Import(set) => set,
                        _ => continue,
                    };
                    if matches!(job, Job::Track(_))
                        && self.track_input(set).as_ref() != inputs[index].as_ref()
                    {
                        retracked.push(set);
                        continue;
                    }
                    if matches!(job, Job::Import(_))
                        && self.import_input(set).as_ref() != inputs[index].as_ref()
                    {
                        continue;
                    }
                    if let Some(scene) = self.scene.as_mut() {
                        scene.install_tracks(set, tracks);
                    }
                }
                Update::Relation(time, frame) => {
                    if let Job::Relation {
                        source,
                        target,
                        shift,
                    } = job
                        && self.relation_input(source, target, shift).as_ref()
                            == inputs[index].as_ref()
                        && let Some(scene) = self.scene.as_mut()
                    {
                        scene.install_relation(source, target, shift, time, frame);
                    }
                }
                Update::Coloc(series) => {
                    let pair = series.pair();
                    if self.coloc_input().as_ref() != inputs[index].as_ref() {
                        self.message = Some(Message::info(
                            "the colocalization finished after the channels, drift, pair or mask it measured changed, so its result was discarded",
                        ));
                    } else if let Some(scene) = self.scene.as_mut() {
                        scene.coloc.install(*series);
                        if let Some((current, thresholds)) = self.coloc_thresholds
                            && current == pair
                        {
                            scene.coloc.set_thresholds(thresholds);
                        }
                    }
                }
                Update::ObjectColoc(coloc) => {
                    let Job::ObjectColoc(set) = job else {
                        continue;
                    };
                    if self.object_coloc_input(set).as_ref() != inputs[index].as_ref() {
                        self.message = Some(Message::info(
                            "the object colocalization finished after the objects or channels it measured changed, so its result was discarded",
                        ));
                    } else if let Some(scene) = self.scene.as_mut() {
                        scene.install_object_coloc(set, *coloc);
                    }
                }
                Update::LocalizationReport {
                    set,
                    revision,
                    report,
                } => {
                    let placed = match (job, self.scene.as_ref()) {
                        (Job::Localizations(held), Some(scene)) if held == set => {
                            still_current(scene, set, revision, &report)
                        }
                        _ => false,
                    };
                    if !placed {
                        self.message = Some(Message::info(
                            "the super-resolution image finished after the localizations, parameters or region it rendered changed, so it was discarded",
                        ));
                    } else {
                        self.localization.install(set, revision, *report);
                    }
                }
                Update::LocalizationClusters {
                    set,
                    revision,
                    clusters,
                } => {
                    let current = self
                        .scene
                        .as_ref()
                        .and_then(|scene| scene.objects.get(set))
                        .map(|object| object.revision());
                    if let (Job::Clusters(held), Some(current)) = (job, current)
                        && held == set
                        && revision == current
                    {
                        self.install_clusters(set, &clusters);
                    }
                }
                Update::Drift(drift) => {
                    let current = match inputs[index].as_ref() {
                        Some(Input::Drift(held)) => self.drift_current(held),
                        _ => false,
                    };
                    if current {
                        self.install_drift(drift);
                    } else {
                        self.message = Some(Message::info(
                            "the drift correction finished after the localizations or image it aligned changed, so its result was discarded",
                        ));
                    }
                }
                Update::DriftSummary { segments, fallback } => {
                    self.message = Some(Message::info(format!(
                        "the drift correction aligned {segments} segments and filled {fallback} of them from the resampled reference"
                    )));
                }
                Update::Registration(registration) => {
                    let current = match inputs[index].as_ref() {
                        Some(Input::Register(held)) => self.register_current(held),
                        _ => false,
                    };
                    if current {
                        self.install_registration(registration);
                    } else {
                        self.message = Some(Message::info(
                            "the channel registration finished after the localizations or channels it aligned changed, so its result was discarded",
                        ));
                    }
                }
                Update::Fiducial {
                    channels,
                    pairs,
                    residual,
                } => {
                    self.message = Some(Message::info(format!(
                        "registered {channels} channels from {pairs} bead pairs · residual {:.0} nm",
                        residual * 1000.0
                    )));
                }
                Update::Calibration {
                    source,
                    psf,
                    beads,
                    iterations,
                } => {
                    if job == Job::Calibrate
                        && self.add_restore_channel(source, Psf::Measured(*psf), iterations)
                    {
                        self.message = Some(Message::info(format!(
                            "a measured point spread function was calibrated from {beads} beads"
                        )));
                    }
                }
                Update::Done => {
                    grana_log::info!(target: "grana::analysis", job = ?job, "an analysis job finished");
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    match job {
                        Job::Detect(set) => {
                            finished.push(job);
                            if self.recipe.is_none()
                                && self.detect_input(set).as_ref() != inputs[index].as_ref()
                            {
                                restarted.push(set);
                            } else {
                                detected.push(set);
                            }
                        }
                        Job::Import(set) => {
                            if self.import_input(set).as_ref() != inputs[index].as_ref() {
                                restarted_imports.push((set, self.analysis[index].take_fallback()));
                            } else {
                                finished.push(job);
                                let mut molecule = Ok(false);
                                if let Some(scene) = self.scene.as_mut() {
                                    scene.realign_localization(set);
                                    if scene
                                        .objects
                                        .get(set)
                                        .is_some_and(|object| object.localization().is_some())
                                    {
                                        molecule = scene.rebuild_molecule_tracks(set);
                                    }
                                }
                                if let Err(detail) = molecule {
                                    self.message = Some(Message::error(detail));
                                }
                                imported.push(set);
                            }
                        }
                        Job::Relation {
                            source,
                            target,
                            shift,
                        } => {
                            finished.push(job);
                            if self.relation_input(source, target, shift).as_ref()
                                != inputs[index].as_ref()
                                && self.relation_present(source, target, shift)
                            {
                                restarted_relations.push((source, target, shift));
                            }
                        }
                        Job::Recut(set) => {
                            finished.push(job);
                            recutted.push(set);
                        }
                        Job::Track(_)
                        | Job::Coloc
                        | Job::ObjectColoc(_)
                        | Job::Localizations(_)
                        | Job::Clusters(_)
                        | Job::Drift
                        | Job::Register
                        | Job::Calibrate => finished.push(job),
                    }
                }
                Update::Failed(detail) => {
                    grana_log::error!(target: "grana::analysis", job = ?job, %detail, "an analysis job failed");
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    if self
                        .recipe
                        .as_ref()
                        .is_some_and(|recipe| recipe.matches(job))
                    {
                        self.abort_recipe();
                    }
                    if let Job::Import(set) = job {
                        failed.push(set);
                    }
                    if matches!(job, Job::Detect(_) | Job::Recut(_))
                        && let Some(state) = self.analysis[index].take_fallback()
                    {
                        rolled_back.push(state);
                        self.message = Some(Message::error(format!(
                            "{detail}; the object set kept the geometry, curation and tracks it held before the analysis"
                        )));
                    } else {
                        self.message = Some(Message::error(detail));
                    }
                }
            }
        }
        removed.sort_unstable();
        removed.dedup();
        for index in removed.into_iter().rev() {
            self.analysis.remove(index);
        }
        for set in failed {
            self.remove_object(set);
        }
        for state in rolled_back {
            self.rollback(state);
        }
        for (set, fallback) in restarted_imports {
            if let Some(state) = fallback {
                self.rollback(state);
            }
            self.start_import(set);
            if self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Import(set))
            {
                self.message = Some(Message::info(
                    "the import finished after the drift or registration it applied changed, so it restarted",
                ));
            } else {
                self.message = Some(Message::error(
                    "the import finished after the drift or registration it applied changed, so it was discarded and cannot restart while the dataset is busy",
                ));
            }
        }
        for (source, target, shift) in restarted_relations {
            if !self.resume.contains(&(source, target, shift)) {
                self.resume.push((source, target, shift));
            }
            self.resume_relations(source);
            self.message = Some(Message::info(
                "the relation finished after the objects it related changed, so its result was discarded",
            ));
        }
        let startable = self.recut.is_some_and(|(set, _)| {
            !self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Recut(set))
        });
        if startable && let Some((set, cut)) = self.recut.take() {
            self.start_recut(set, cut);
        }
        for (set, recut) in detected
            .into_iter()
            .map(|set| (set, false))
            .chain(imported.into_iter().map(|set| (set, false)))
            .chain(recutted.into_iter().map(|set| (set, true)))
        {
            if self.recut.is_some_and(|(pending, _)| pending == set)
                || self
                    .analysis
                    .iter()
                    .any(|analysis| analysis.job() == Job::Recut(set))
            {
                continue;
            }
            if recut {
                self.defer_relations(set);
            }
            if let Some(scene) = self.scene.as_mut() {
                let applied = if recut {
                    Ok(scene.reapply_hidden(set))
                } else {
                    scene.apply_curation(set)
                };
                if let Err(detail) = applied {
                    self.message = Some(Message::error(detail));
                    continue;
                }
            }
            let carried_tracks = self
                .scene
                .as_ref()
                .and_then(|scene| scene.objects.get(set))
                .is_some_and(|object| object.tracks().is_some());
            if !carried_tracks {
                self.start_tracks(set);
            }
            self.resume_relations(set);
            if self.recipe.is_none() {
                self.refresh_dependents(set);
            }
        }
        for set in restarted {
            self.start_detect(set);
        }
        for set in retracked {
            self.start_tracks(set);
        }
        for job in finished {
            self.advance_recipe(job);
        }
        repaint
    }

    pub(super) fn train_classifier(
        &mut self,
        set: ObjectSetId,
        features: Vec<grana_measure::Measurement>,
    ) {
        if self.loading.is_some() || self.scene.is_none() {
            return;
        }
        if self.measure.set() != Some(set) {
            self.message = Some(Message::error(
                "select the object set in the statistics panel before training",
            ));
            return;
        }
        if self.measure.group() != Group::Object {
            self.message = Some(Message::error(
                "switch the statistics table to objects before training",
            ));
            return;
        }
        let Some(rows) = self.measure.current_rows().map(|rows| rows.rows().to_vec()) else {
            self.message = Some(Message::error("the object set holds no measurement table"));
            return;
        };
        if rows.is_empty() {
            self.message = Some(Message::error(
                "gate the objects with statistics filters before training",
            ));
            return;
        }
        if features.is_empty() {
            self.message = Some(Message::error(
                "choose at least one measurement to train the classifier",
            ));
            return;
        }
        let outcome = self
            .scene
            .as_mut()
            .expect("the classifier names a scene")
            .train_classifier(set, &features, &rows);
        match outcome {
            Ok(()) => {
                let summary = self
                    .scene
                    .as_ref()
                    .and_then(|scene| scene.objects.get(set))
                    .and_then(|set| set.classifier())
                    .map(|classifier| {
                        format!(
                            "trained a classifier on {} measurements · {:.0}% balanced fit",
                            classifier.features().len(),
                            classifier.accuracy() * 100.0
                        )
                    });
                self.message = summary.map(Message::info);
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn record_progress(&mut self, job: Job, time: u32, done: usize, total: usize) {
        let progress = Progress {
            job,
            time,
            done,
            total,
        };
        if let Some(existing) = self
            .progress
            .iter_mut()
            .find(|progress| progress.job == job)
        {
            *existing = progress;
        } else {
            self.progress.push(progress);
        }
    }

    pub(super) fn cancel(&mut self, job: Job) -> bool {
        let fallback = self.halt(job);
        fallback.is_some_and(|state| self.rollback(state))
    }

    pub(super) fn discard_analysis(&mut self) {
        let jobs = self.analysis.iter().map(Analysis::job).collect::<Vec<_>>();
        for job in jobs {
            self.cancel(job);
        }
        self.progress.clear();
        self.resume.clear();
        self.recut = None;
        self.abort_recipe();
    }

    pub(super) fn halt(&mut self, job: Job) -> Option<grana_scene::SetState> {
        if self
            .recipe
            .as_ref()
            .is_some_and(|recipe| recipe.job() == Some(job))
        {
            self.abort_recipe();
        }
        let fallback = self
            .analysis
            .iter_mut()
            .find(|analysis| analysis.job() == job)
            .and_then(Analysis::take_fallback);
        self.analysis.retain(|analysis| analysis.job() != job);
        self.progress.retain(|progress| progress.job != job);
        fallback
    }

    fn rollback(&mut self, state: grana_scene::SetState) -> bool {
        let Some(set) = ObjectSetId::from_raw(state.id) else {
            return false;
        };
        if !self
            .scene
            .as_mut()
            .is_some_and(|scene| scene.restore_object(state))
        {
            return false;
        }
        self.resume_relations(set);
        true
    }

    pub(super) fn defer_relations(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let mut pairs = Vec::new();
        for object in scene.objects.iter() {
            for relation in object.relations() {
                let pair = (object.id(), relation.target(), relation.shift());
                if (pair.0 == set || pair.1 == set) && !self.resume.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
        self.resume.extend(pairs);
    }

    pub(super) fn resume_relations(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let busy = |pair: &(ObjectSetId, ObjectSetId, i32)| {
            self.analysis.iter().any(|analysis| {
                matches!(
                    analysis.job(),
                    Job::Detect(id) | Job::Recut(id) if id == pair.0 || id == pair.1
                )
            })
        };
        let ready = self
            .resume
            .iter()
            .copied()
            .filter(|pair| (pair.0 == set || pair.1 == set) && !busy(pair))
            .filter(|pair| {
                scene
                    .objects
                    .get(pair.0)
                    .is_some_and(|object| object.detected() > 0)
                    && scene
                        .objects
                        .get(pair.1)
                        .is_some_and(|object| object.detected() > 0)
            })
            .collect::<Vec<_>>();
        let existing = ready
            .iter()
            .map(|(source, target, shift)| {
                scene
                    .objects
                    .get(*source)
                    .is_some_and(|object| object.relation(*target, *shift).is_some())
            })
            .collect::<Vec<_>>();
        self.resume.retain(|candidate| !ready.contains(candidate));
        for ((source, target, shift), existing) in ready.into_iter().zip(existing) {
            let started = if existing {
                self.start_relation(source, target, shift)
            } else {
                self.add_relation(source, target, shift)
            };
            if !started {
                self.resume.push((source, target, shift));
            }
        }
    }

    pub(super) fn clear_relations(&mut self, set: ObjectSetId) {
        self.analysis.retain(|analysis| {
                !matches!(analysis.job(), Job::Relation { .. } if analysis.job().involves(set))
            });
        self.progress.retain(
            |progress| !matches!(progress.job, Job::Relation { .. } if progress.job.involves(set)),
        );
        if let Some(scene) = self.scene.as_mut() {
            scene.clear_relations(set);
        }
    }

    pub(super) fn add_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    ) -> bool {
        let busy = self.analysis.iter().any(|analysis| {
            matches!(
                analysis.job(),
                Job::Detect(id) | Job::Recut(id) if id == source || id == target
            )
        });
        if busy {
            self.message = Some(Message::error(
                "the object sets are being detected before they relate",
            ));
            return false;
        }
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        if scene
            .objects
            .get(source)
            .is_some_and(|set| set.detected() == 0)
        {
            self.message = Some(Message::error(
                "the object set holds no objects to relate from",
            ));
            return false;
        }
        if scene
            .objects
            .get(target)
            .is_some_and(|set| set.detected() == 0)
        {
            self.message = Some(Message::error(
                "the target object set holds no objects to relate to",
            ));
            return false;
        }
        if !scene.add_relation(source, target, shift) {
            return false;
        }
        self.start_relation(source, target, shift)
    }

    pub(super) fn move_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        from: i32,
        to: i32,
    ) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if !scene.move_relation(source, target, from, to) {
            return;
        }
        self.resume.retain(|pair| *pair != (source, target, from));
        self.cancel(Job::Relation {
            source,
            target,
            shift: from,
        });
        self.start_relation(source, target, to);
    }

    pub(super) fn remove_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) {
        self.cancel(Job::Relation {
            source,
            target,
            shift,
        });
        self.resume.retain(|pair| *pair != (source, target, shift));
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_relation(source, target, shift);
        }
    }

    pub(super) fn start_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    ) -> bool {
        let job = Job::Relation {
            source,
            target,
            shift,
        };
        if self.analysis.iter().any(|analysis| analysis.job() == job) {
            return true;
        }
        if self.loading.is_some() {
            return false;
        }
        let same = source == target;
        let prepared = {
            let Some(scene) = self.scene.as_ref() else {
                return false;
            };
            let (Some(from), Some(to)) = (scene.objects.get(source), scene.objects.get(target))
            else {
                return false;
            };
            if to.detected() == 0 {
                self.message = Some(Message::error(
                    "the target object set holds no objects to relate to",
                ));
                return false;
            }
            let gate = scene.relation_gate(source, target);
            let total = scene.frame_count() as usize;
            (from.source(), to.source(), gate, total)
        };
        let Some(input) = self.relation_input(source, target, shift) else {
            return false;
        };
        let (from, to, gate, total) = prepared;
        self.message = None;
        self.record_progress(job, 0, 0, total);
        self.analysis
            .push(crate::relation(job, from, to, shift, same, gate, 0).keyed(input));
        true
    }

    fn relation_present(&self, source: ObjectSetId, target: ObjectSetId, shift: i32) -> bool {
        self.scene
            .as_ref()
            .and_then(|scene| scene.objects.get(source))
            .is_some_and(|set| set.relation(target, shift).is_some())
    }

    pub(super) fn begin_calibration(&mut self, source: usize, threshold: f32, separation: f32) {
        if !self.analysable() {
            return;
        }
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay calibrates the point spread function it records",
            ));
            return;
        }
        if self.loading.is_some() {
            return;
        }
        let Some(channels) = self.analysis_channels() else {
            self.message = Some(Message::error("open a dataset before calibrating it"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if source >= scene.base_channels() {
            self.message = Some(Message::error(
                "a point spread function is calibrated from a stored channel",
            ));
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::Calibrate)
        {
            self.message = Some(Message::error(
                "a point spread function calibration is already running",
            ));
            return;
        }
        let image = channels.image_shared();
        let defaults = Settings::for_image(&image, source, scene.time());
        let settings = Settings::new(
            source,
            scene.time(),
            threshold.clamp(1.0, 64.0),
            separation
                .max(
                    2.0 * scene
                        .dataset
                        .voxel_size()
                        .x
                        .min(scene.dataset.voxel_size().y),
                )
                .min(1.0e3),
            defaults.half(),
            defaults.bands(),
        );
        self.message = None;
        self.record_progress(Job::Calibrate, 0, 0, 1);
        self.analysis.push(crate::calibration(
            Job::Calibrate,
            image,
            settings,
            CALIBRATION_ITERATIONS,
        ));
    }

    pub(super) fn detection_root(&self, set: ObjectSetId) -> Option<ObjectSetId> {
        self.scene.as_ref()?.detection_root(set)
    }

    pub(super) fn detect_root(&mut self, set: ObjectSetId) {
        let Some(root) = self.detection_root(set) else {
            return;
        };
        let merged = self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(root))
            .is_some_and(|object| object.provenance().is_merged());
        if merged {
            self.message = Some(Message::error(
                "a merged object set carries no detection to run",
            ));
            return;
        }
        self.start_detect(root);
    }

    pub(super) fn start_detect(&mut self, set: ObjectSetId) {
        if !self.analysable() {
            return;
        }
        if self.loading.is_some() {
            return;
        }
        if self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(set))
            .is_some_and(|object| object.provenance().is_imported())
        {
            self.message = Some(Message::error(
                "an imported object set is measured from its label image",
            ));
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::Detect(set))
        {
            return;
        }
        let Some(plan) = self.detect_plan(set) else {
            return;
        };
        let fallback = self
            .scene
            .as_ref()
            .and_then(|scene| scene.object_state(set));
        let _ = self.halt(Job::Recut(set));
        if self.recut.is_some_and(|(pending, _)| pending == set) {
            self.recut = None;
        }
        let _ = self.halt(Job::Track(set));
        if self.recipe.is_none() {
            self.defer_relations(set);
            self.clear_relations(set);
        }
        if let Some(scene) = self.scene.as_mut() {
            scene.clear_tracks(set);
        }
        self.message = None;
        self.record_progress(Job::Detect(set), 0, 0, 0);
        if plan.clears_objects()
            && let Some(scene) = self.scene.as_mut()
        {
            scene.clear_object(set);
        }
        let Some(input) = self.detect_input(set) else {
            return;
        };
        self.analysis
            .push(plan.into_analysis(set).keyed(input).with_fallback(fallback));
    }

    fn detect_plan(&mut self, set: ObjectSetId) -> Option<DetectPlan> {
        let Some(channels) = self.analysis_channels() else {
            self.message = Some(Message::error("the dataset is still opening"));
            return None;
        };
        let scene = self.scene.as_ref()?;
        let object = scene.objects.get(set)?;
        let channel = object.channel();
        if channel >= scene.base_channels() {
            let spec = scene.derived()[channel - scene.base_channels()].spec();
            if let Some(set) = spec.mask() {
                let ready = scene.objects.get(set).is_some_and(|set| set.detected() > 0);
                if !ready {
                    self.message = Some(Message::error(
                        "the mask channel needs its object set detected",
                    ));
                    return None;
                }
            }
        }
        let params = object.params();
        let segment = object.provenance().segment().cloned();
        let region = scene.region;
        let drifts = scene.drifts().clone();
        let seed_source = scene.seed_source(set);
        let seeds = scene.seed_frames(set);
        if let Some(source) = seed_source
            && !matches!(params, ObjectParams::Cells(_))
            && seeds.iter().all(|frame| frame.is_empty())
        {
            let name = scene
                .objects
                .get(source)
                .map_or("the seed source", |set| set.name());
            self.message = Some(Message::error(format!(
                "the seed source {name} detected no objects"
            )));
            return None;
        }
        let filter_target = scene
            .mask_filter
            .is_some_and(|filter| filter.target() == set);
        let roi = if filter_target {
            None
        } else {
            match scene.mask {
                None => None,
                Some(source) => {
                    if source == set {
                        self.message = Some(Message::error(
                            "an object set cannot be limited to its own objects",
                        ));
                        return None;
                    }
                    let detected = scene
                        .objects
                        .get(source)
                        .is_some_and(|set| set.detected() > 0);
                    if !detected {
                        self.message = Some(Message::error(
                            "the analysis mask holds no detected objects",
                        ));
                        return None;
                    }
                    match scene.mask_frames() {
                        Ok(Some(frames)) => Some(frames),
                        Ok(None) => {
                            self.message = Some(Message::error(
                                "the analysis mask holds no detected objects",
                            ));
                            return None;
                        }
                        Err(detail) => {
                            self.message = Some(Message::error(detail));
                            return None;
                        }
                    }
                }
            }
        };
        if scene.dataset.channels()[channel].intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to detect",
            ));
            return None;
        }
        if region.enabled() && region.voxel_range(channels.pyramid().level(0)).is_none() {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return None;
        }
        Some(match (params, segment) {
            (ObjectParams::Cells(params), _) => {
                let Some(source) = seed_source else {
                    self.message = Some(Message::error("the cells grow from a nucleus object set"));
                    return None;
                };
                let nuclei = scene.objects.get(source)?;
                if nuclei.detected() == 0 {
                    self.message = Some(Message::error(
                        "the nucleus object set holds no detected objects",
                    ));
                    return None;
                }
                DetectPlan::Grow {
                    channels,
                    request: GrowRequest::new(
                        channel,
                        params,
                        region,
                        drifts,
                        roi,
                        nuclei.source(),
                    ),
                }
            }
            (_, Some(segment)) => {
                let request = SegmentRequest::new(channel, segment, region, drifts, roi);
                DetectPlan::Segment {
                    channels,
                    request: if seeds.iter().any(|frame| !frame.is_empty()) {
                        request.with_seeds(seeds)
                    } else {
                        request
                    },
                }
            }
            (params, None) => {
                let policy = match seed_source {
                    Some(_) => grana_pipeline::SeedPolicy::Sourced,
                    None if seeds.iter().any(|frame| !frame.is_empty()) => {
                        grana_pipeline::SeedPolicy::Strict
                    }
                    None => grana_pipeline::SeedPolicy::Unseeded,
                };
                DetectPlan::Detect {
                    channels,
                    request: DetectRequest::new(
                        channel,
                        params,
                        region,
                        roi,
                        drifts,
                        grana_analysis::Budget::new(0),
                        self.store(),
                    )
                    .with_seeds(seeds, policy),
                }
            }
        })
    }

    pub(super) fn recut(&mut self, set: ObjectSetId, previous: ObjectParams, params: ObjectParams) {
        if previous == params {
            return;
        }
        if matches!(params, ObjectParams::Cells(_)) {
            if self.scene.is_some() {
                self.start_detect(set);
            }
            return;
        }
        let restore = previous;
        let (ObjectParams::Surfaces(previous), ObjectParams::Surfaces(params)) = (previous, params)
        else {
            self.message = Some(Message::info(
                "detect the object set to apply the new detection settings",
            ));
            return;
        };
        if previous == params {
            return;
        }
        if previous.threshold() != params.threshold()
            || previous.grain() != params.grain()
            || previous.opening() != params.opening()
            || previous.closing() != params.closing()
        {
            self.message = Some(Message::info(
                "detect the object set to apply the new surface settings",
            ));
            return;
        }
        let cut = params.cut();
        if !self.cuttable(set) {
            let merged = self
                .scene
                .as_ref()
                .and_then(|scene| scene.objects.get(set))
                .is_some_and(|object| object.provenance().is_merged());
            if merged {
                if let Some(target) = self.object_mut(set) {
                    target.set_params(restore);
                }
                self.message = Some(Message::error(
                    "a merged object set only changes its cut filters",
                ));
                return;
            }
            let detected = self
                .scene
                .as_ref()
                .and_then(|scene| scene.objects.get(set))
                .is_some_and(|object| object.detected() > 0);
            let detecting = self
                .analysis
                .iter()
                .any(|analysis| matches!(analysis.job(), Job::Detect(id) if id == set));
            if detected && !detecting {
                self.start_detect(set);
            }
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| matches!(analysis.job(), Job::Detect(id) if id == set))
        {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::Recut(set))
        {
            self.recut = Some((set, cut));
            return;
        }
        self.start_recut(set, cut);
    }

    pub(super) fn cuttable(&self, set: ObjectSetId) -> bool {
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        let Some(object) = scene.objects.get(set) else {
            return false;
        };
        object.detected() > 0
            && (0..scene.frame_count()).all(|time| {
                object
                    .frame_shared(time)
                    .is_none_or(|frame| match frame.as_ref() {
                        ObjectFrame::Surfaces(surface) => surface.cuttable(),
                        _ => false,
                    })
            })
    }

    pub(super) fn start_recut(&mut self, set: ObjectSetId, cut: SurfaceCut) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let source = object.source();
        if source.counts().iter().all(Option::is_none) {
            return;
        }
        let fallback = scene.object_state(set);
        self.message = None;
        self.record_progress(Job::Recut(set), 0, 0, source.frames() as usize);
        self.analysis.push(
            crate::recut(Job::Recut(set), source, cut, 0, self.store()).with_fallback(fallback),
        );
    }

    pub(super) fn store(&self) -> Arc<SpillStore> {
        Arc::clone(&self.store)
    }

    pub(super) fn start_tracks(&mut self, set: ObjectSetId) {
        if !self.analysable() {
            return;
        }
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job().installs(set))
        {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.frame_count() < 2 {
            return;
        }
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        if !object.kind().trackable() {
            return;
        }
        if object.detected() == 0 {
            return;
        }
        let mut detections = vec![Vec::new(); scene.frame_count() as usize];
        for (time, frame) in object.frames() {
            detections[time as usize] = frame.detections();
        }
        let params = object
            .track_style()
            .expect("a trackable object carries track parameters")
            .params();
        let frame_interval = scene.dataset.frame_interval();
        self.message = None;
        self.record_progress(Job::Track(set), 0, 0, detections.len());
        let Some(input) = self.track_input(set) else {
            return;
        };
        self.analysis
            .push(crate::track(Job::Track(set), detections, params, frame_interval).keyed(input));
    }

    pub(super) fn start_coloc(&mut self) {
        if !self.analysable() {
            return;
        }
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Coloc)
        {
            return;
        }
        let Some(channels) = self.analysis_channels() else {
            self.message = Some(Message::error("the dataset is still opening"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(pair) = scene.coloc.pair() else {
            self.message = Some(Message::error(
                "the dataset needs two channels to colocalize",
            ));
            return;
        };
        if [pair.a(), pair.b()]
            .iter()
            .any(|channel| scene.dataset.channels()[*channel].intensity().span() <= 0.0)
        {
            self.message = Some(Message::error(
                "the selected channels have no intensity range to colocalize",
            ));
            return;
        }
        let region = scene.region;
        if region.enabled() && region.voxel_range(channels.pyramid().level(0)).is_none() {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        let settings = scene.coloc.settings();
        let drifts = scene.drifts().clone();
        let mask = scene.mask;
        let roi = match mask {
            None => None,
            Some(mask) => {
                let detected = scene
                    .objects
                    .get(mask)
                    .is_some_and(|set| set.detected() > 0);
                if !detected {
                    self.message = Some(Message::error(
                        "the analysis mask holds no detected objects",
                    ));
                    return;
                }
                match scene.mask_frames() {
                    Ok(Some(frames)) => Some(frames),
                    Ok(None) => {
                        self.message = Some(Message::error(
                            "the analysis mask holds no detected objects",
                        ));
                        return;
                    }
                    Err(detail) => {
                        self.message = Some(Message::error(detail));
                        return;
                    }
                }
            }
        };
        self.message = None;
        self.record_progress(Job::Coloc, 0, 0, 0);
        let Some(input) = self.coloc_input() else {
            return;
        };
        self.analysis.push(
            crate::coloc(
                Job::Coloc,
                channels,
                grana_pipeline::ColocRequest::new(
                    pair,
                    region,
                    roi,
                    mask.map(|set| set.raw()),
                    settings,
                    drifts,
                    0,
                ),
            )
            .keyed(input),
        );
    }

    pub(super) fn start_object_coloc(&mut self, set: ObjectSetId) {
        if !self.analysable() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::ObjectColoc(set))
        {
            return;
        }
        let Some(channels) = self.analysis_channels() else {
            self.message = Some(Message::error("the dataset is still opening"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        if object.detected() == 0 {
            self.message = Some(Message::error(format!(
                "{} holds no objects to colocalize",
                object.name()
            )));
            return;
        }
        let Some(pair) = scene.coloc.pair() else {
            self.message = Some(Message::error(
                "the dataset needs two channels to colocalize",
            ));
            return;
        };
        let measured = object.measured_channels();
        if pair.a() >= measured || pair.b() >= measured {
            self.message = Some(Message::error(format!(
                "{} measured {measured} channels and the pair names channels beyond them",
                object.name()
            )));
            return;
        }
        if [pair.a(), pair.b()]
            .iter()
            .any(|channel| scene.dataset.channels()[*channel].intensity().span() <= 0.0)
        {
            self.message = Some(Message::error(
                "the selected channels have no intensity range to colocalize",
            ));
            return;
        }
        let region = scene.region;
        if region.enabled() && region.voxel_range(channels.pyramid().level(0)).is_none() {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        let source = object.source();
        let drifts = scene.drifts().clone();
        self.message = None;
        self.record_progress(Job::ObjectColoc(set), 0, 0, 0);
        let Some(input) = self.object_coloc_input(set) else {
            return;
        };
        self.analysis.push(
            crate::object_coloc(
                Job::ObjectColoc(set),
                channels,
                grana_pipeline::run::ObjectColocRequest::new(pair, region, source, drifts, 0),
            )
            .keyed(input),
        );
    }

    pub(super) fn start_drift(&mut self) {
        if !self.analysable() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let channel = scene.selected;
        let reference = scene
            .drift(channel)
            .map_or(0, grana_scene::Drift::reference);
        self.align_drift(channel, reference);
    }

    pub(super) fn start_localization_drift(&mut self, set: ObjectSetId) {
        if !self.analysable() {
            return;
        }
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Drift)
        {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        if object.kind() != grana_scene::ObjectKind::Spots {
            self.message = Some(Message::error(
                "localization drift aligns a spot object set",
            ));
            return;
        }
        if object.localization().is_none() {
            self.message = Some(Message::error(
                "the object set carries no localization table to align",
            ));
            return;
        }
        if scene.frame_count() < 2 {
            self.message = Some(Message::error("the dataset holds one frame to align"));
            return;
        }
        let channel = object.channel();
        let base = scene.drift(channel).cloned().unwrap_or_default();
        let reference = base.reference();
        let source = object.source();
        let Some(input) = self.localization_drift_input(set) else {
            return;
        };
        self.message = None;
        self.record_progress(Job::Drift, 0, 0, 0);
        self.analysis.push(
            crate::localization_drift(
                Job::Drift,
                grana_pipeline::run::LocalizationDriftRequest::new(
                    source, reference, channel, base,
                ),
            )
            .keyed(input),
        );
    }

    pub(super) fn align_drift(&mut self, channel: usize, reference: u32) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Drift)
        {
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error("the dataset is still opening"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.frame_count() < 2 {
            self.message = Some(Message::error("the dataset holds one frame to align"));
            return;
        }
        if channel >= scene.dataset.channels().len() {
            self.message = Some(Message::error(
                "the protocol aligns a channel the dataset does not hold",
            ));
            return;
        }
        if channel >= scene.base_channels() {
            self.message = Some(Message::error(
                "the drift correction aligns a stored channel",
            ));
            return;
        }
        if reference >= scene.frame_count() {
            self.message = Some(Message::error(
                "the protocol aligns to a frame the dataset does not hold",
            ));
            return;
        }
        if scene.dataset.channels()[channel].intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to align",
            ));
            return;
        }
        self.message = None;
        self.record_progress(Job::Drift, 0, 0, scene.frame_count() as usize);
        let input = self.drift_input(channel, reference);
        self.analysis.push(
            crate::drift(
                Job::Drift,
                channels,
                grana_pipeline::DriftRequest::new(channel, reference),
            )
            .keyed(input),
        );
    }

    pub(super) fn set_drift_reference(&mut self, reference: u32) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let Some(drift) = scene.drift(scene.selected).cloned() else {
            return;
        };
        if drift.is_empty() || reference >= drift.frames() || reference == drift.reference() {
            return;
        }
        if scene.set_drift(drift.referenced_at(reference)) {
            self.realign();
        }
    }

    pub(super) fn set_drift_offset(&mut self, time: u32, offset: [f32; 3]) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let Some(drift) = scene.drift(scene.selected).cloned() else {
            return;
        };
        if drift.is_empty() || time >= drift.frames() || time == drift.reference() {
            return;
        }
        if !offset.iter().all(|value| value.is_finite()) {
            return;
        }
        let offset =
            offset.map(|value| value.clamp(-grana_scene::Drift::LIMIT, grana_scene::Drift::LIMIT));
        if scene.set_drift(drift.placed(time, offset)) {
            self.realign();
        }
    }

    pub(super) fn clear_drift(&mut self) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if scene.clear_drift(scene.selected) {
            self.realign();
        }
    }

    pub(super) fn start_fiducial_registration(&mut self, reference: usize) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Register)
        {
            return;
        }
        if !self.analysable() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.base_channels() < 2 {
            self.message = Some(Message::error("the dataset holds one channel to register"));
            return;
        }
        if reference >= scene.base_channels() {
            self.message = Some(Message::error(
                "the channel registration aligns a stored channel",
            ));
            return;
        }
        let fiducial = scene
            .objects
            .iter()
            .filter(|set| {
                set.kind() == grana_scene::ObjectKind::Spots
                    && set.channel() < scene.base_channels()
                    && set.localization().is_some()
            })
            .map(|set| (set.id(), set.channel()))
            .collect::<Vec<_>>();
        let Some(source) = fiducial
            .iter()
            .find(|(_, channel)| *channel == reference)
            .map(|(set, _)| *set)
        else {
            self.message = Some(Message::error(
                "the selected channel carries no localization table to register from",
            ));
            return;
        };
        let mut moving = Vec::new();
        let mut channels = Vec::new();
        for (set, channel) in &fiducial {
            if *channel == reference || channels.contains(channel) {
                continue;
            }
            channels.push(*channel);
            moving.push(*set);
        }
        if moving.is_empty() {
            self.message = Some(Message::error(
                "another channel needs a localization table to register",
            ));
            return;
        }
        let scale = scene
            .dataset
            .pyramid()
            .level(0)
            .voxel()
            .into_iter()
            .fold(f32::INFINITY, f32::min);
        let fiducial_sets = std::iter::once(source)
            .chain(moving.iter().copied())
            .collect::<Vec<_>>();
        let Some(input) = self.fiducial_input(reference, &fiducial_sets) else {
            return;
        };
        let reference =
            grana_pipeline::run::FiducialChannel::new(reference, fiducial_points(scene, source));
        let moving = moving
            .into_iter()
            .zip(channels)
            .map(|(set, channel)| {
                grana_pipeline::run::FiducialChannel::new(channel, fiducial_points(scene, set))
            })
            .collect::<Vec<_>>();
        let total = moving.len();
        let channels = scene.base_channels();
        self.message = None;
        self.record_progress(Job::Register, 0, 0, total);
        self.analysis.push(
            crate::fiducial(
                Job::Register,
                grana_pipeline::run::FiducialRequest::new(reference, moving, channels, scale),
            )
            .keyed(input),
        );
    }

    pub(super) fn start_registration(&mut self, reference: usize) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Register)
        {
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error("the dataset is still opening"));
            return;
        };
        let stored = {
            let Some(scene) = self.scene.as_ref() else {
                return;
            };
            if scene.base_channels() < 2 {
                self.message = Some(Message::error("the dataset holds one channel to register"));
                return;
            }
            if reference >= scene.base_channels() {
                self.message = Some(Message::error(
                    "the channel registration aligns a stored channel",
                ));
                return;
            }
            (scene.base_channels(), scene.time())
        };
        self.message = None;
        self.record_progress(Job::Register, 0, 0, stored.0);
        let input = self.register_input(reference);
        self.analysis.push(
            crate::register(
                Job::Register,
                channels,
                grana_pipeline::RegisterRequest::new(reference, stored.1),
            )
            .keyed(input),
        );
    }

    pub(super) fn clear_registration(&mut self) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if scene.set_registration(grana_scene::Registration::default()) {
            self.install_channels();
            self.realign();
        }
    }

    fn install_drift(&mut self, drift: grana_scene::Drift) {
        let Some(recording) = self.begin_change(Edit::new(Action::Drift)) else {
            self.message = Some(Message::info(
                "the drift correction finished while the document was loading, so its result was discarded",
            ));
            return;
        };
        if let Some(scene) = self.scene.as_mut() {
            scene.set_drift(drift);
        }
        self.realign();
        self.commit_edit(recording);
    }

    fn install_registration(&mut self, registration: grana_scene::Registration) {
        let Some(recording) = self.begin_change(Edit::new(Action::Registration)) else {
            self.message = Some(Message::info(
                "the channel registration finished while the document was loading, so its result was discarded",
            ));
            return;
        };
        if let Some(scene) = self.scene.as_mut() {
            scene.set_registration(registration);
        }
        self.install_channels();
        self.realign();
        self.commit_edit(recording);
    }

    pub(super) fn realign(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let drifts = scene.drifts().clone();
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let rebuilt = source.drifts().digest() != drifts.digest();
        source.set_drifts(drifts);
        if rebuilt {
            self.install_channels();
        }
        self.reload_streamed();
    }
}

fn fiducial_points(scene: &Scene, set: ObjectSetId) -> Vec<[f32; 3]> {
    let Some(object) = scene.objects.get(set) else {
        return Vec::new();
    };
    let grouped = object
        .localization()
        .is_some_and(|source| source.clustered());
    if !grouped {
        let mut points = Vec::new();
        for time in 0..object.frame_count() {
            let Some(frame) = object.frame_shared(time) else {
                continue;
            };
            for (index, spot) in frame.spots_of().iter().enumerate() {
                if object.visible_object(time, index) {
                    points.push(spot.center());
                }
            }
        }
        return points;
    }
    let mut groups = std::collections::BTreeMap::<u32, (u32, [f64; 3])>::new();
    for time in 0..object.frame_count() {
        let Some(frame) = object.frame_shared(time) else {
            continue;
        };
        for (index, spot) in frame.spots_of().iter().enumerate() {
            let Some(group) = spot.group() else {
                continue;
            };
            if !object.visible_object(time, index) {
                continue;
            }
            let entry = groups.entry(group).or_insert((0, [0.0; 3]));
            entry.0 += 1;
            for (axis, value) in spot.center().iter().enumerate() {
                entry.1[axis] += f64::from(*value);
            }
        }
    }
    groups
        .values()
        .map(|(count, sum)| [0, 1, 2].map(|axis| (sum[axis] / f64::from(*count)) as f32))
        .collect()
}
