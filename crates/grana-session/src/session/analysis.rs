use super::*;

use grana_calibration::Settings;
use grana_pipeline::run::{GrowRequest, SegmentRequest};

const CALIBRATION_ITERATIONS: u32 = 10;

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
            Coloc(Box<grana_measure::ColocSeries>, Region),
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
                    Poll::Event(Event::Coloc { series, region }) => {
                        updates.push((index, job, Update::Coloc(series, region)));
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
        let mut detected = Vec::new();
        let mut imported = Vec::new();
        let mut recutted = Vec::new();
        let mut finished = Vec::new();
        for (index, job, update) in updates {
            repaint = true;
            match update {
                Update::Progress(time, done, total) => {
                    self.record_progress(job, time, done, total);
                }
                Update::Frame(time, frame) => {
                    if let (Job::Detect(set) | Job::Import(set) | Job::Recut(set), Some(scene)) =
                        (job, self.scene.as_mut())
                    {
                        scene.install(set, time, frame);
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
                    if let (Job::Import(set), Some(scene)) = (job, self.scene.as_mut()) {
                        let spec = grana_object::LocalizationSpec::cover(scene.frame_count());
                        scene.set_localization(
                            set,
                            grana_scene::LocalizationSource::new(
                                pixel, width, height, volume, photons, spec, alignment,
                            ),
                        );
                    }
                }
                Update::Tracks(tracks) => {
                    if let (Job::Track(set) | Job::Import(set), Some(scene)) =
                        (job, self.scene.as_mut())
                    {
                        scene.install_tracks(set, tracks);
                    }
                }
                Update::Relation(time, frame) => {
                    if let (
                        Job::Relation {
                            source,
                            target,
                            shift,
                        },
                        Some(scene),
                    ) = (job, self.scene.as_mut())
                    {
                        scene.install_relation(source, target, shift, time, frame);
                    }
                }
                Update::Coloc(series, region) => {
                    let pair = series.pair();
                    if let Some(scene) = self.scene.as_mut()
                        && scene.coloc.pair() == Some(pair)
                        && scene.region == region
                        && series.mask() == scene.mask.map(|set| set.raw())
                    {
                        scene.coloc.install(*series);
                        if let Some((current, thresholds)) = self.coloc_thresholds
                            && current == pair
                        {
                            scene.coloc.set_thresholds(thresholds);
                        }
                    }
                }
                Update::ObjectColoc(coloc) => {
                    if let (Job::ObjectColoc(set), Some(scene)) = (job, self.scene.as_mut()) {
                        scene.install_object_coloc(set, *coloc);
                    }
                }
                Update::LocalizationReport {
                    set,
                    revision,
                    report,
                } => {
                    let current = self
                        .scene
                        .as_ref()
                        .and_then(|scene| scene.objects.get(set))
                        .map(|object| object.revision());
                    if let (Job::Localizations(held), Some(current)) = (job, current)
                        && held == set
                        && revision == current
                    {
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
                    if let (Job::Drift, Some(scene)) = (job, self.scene.as_mut()) {
                        scene.set_drift(drift);
                    }
                    self.realign();
                }
                Update::Registration(registration) => {
                    if let (Job::Register, Some(scene)) = (job, self.scene.as_mut()) {
                        scene.set_registration(registration);
                    }
                    self.install_channels();
                    self.realign();
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
                    if job == Job::Calibrate {
                        let before = self.scene.as_ref().map_or(0, |scene| scene.derived().len());
                        self.add_restore_channel(source, Psf::Measured(*psf), iterations);
                        let after = self.scene.as_ref().map_or(0, |scene| scene.derived().len());
                        if after > before {
                            self.message = Some(Message::info(format!(
                                "a measured point spread function was calibrated from {beads} beads"
                            )));
                        }
                    }
                }
                Update::Done => {
                    self.progress.retain(|progress| progress.job != job);
                    let mut molecule = Ok(false);
                    if let (Job::Import(set), Some(scene)) = (job, self.scene.as_mut()) {
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
                    removed.push(index);
                    finished.push(job);
                    match job {
                        Job::Detect(set) => detected.push(set),
                        Job::Import(set) => imported.push(set),
                        Job::Recut(set) => recutted.push(set),
                        Job::Track(_)
                        | Job::Relation { .. }
                        | Job::Coloc
                        | Job::ObjectColoc(_)
                        | Job::Localizations(_)
                        | Job::Clusters(_)
                        | Job::Drift
                        | Job::Register => {}
                        Job::Calibrate => {}
                    }
                }
                Update::Failed(detail) => {
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
                    self.message = Some(Message::error(detail));
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
        if let Some((set, cut)) = self.recut.take()
            && !self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Recut(set))
        {
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
                if recut {
                    scene.reapply_hidden(set);
                } else {
                    scene.apply_curation(set);
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
        let rows = self.measure.rows().to_vec();
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

    pub(super) fn cancel(&mut self, job: Job) {
        if self
            .recipe
            .as_ref()
            .is_some_and(|recipe| recipe.job() == Some(job))
        {
            self.abort_recipe();
        }
        self.analysis.retain(|analysis| analysis.job() != job);
        self.progress.retain(|progress| progress.job != job);
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
            if existing {
                self.start_relation(source, target, shift);
            } else {
                self.add_relation(source, target, shift);
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

    pub(super) fn add_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) {
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
            return;
        }
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if scene
            .objects
            .get(source)
            .is_some_and(|set| set.detected() == 0)
        {
            self.message = Some(Message::error(
                "the object set holds no objects to relate from",
            ));
            return;
        }
        if scene
            .objects
            .get(target)
            .is_some_and(|set| set.detected() == 0)
        {
            self.message = Some(Message::error(
                "the target object set holds no objects to relate to",
            ));
            return;
        }
        if !scene.add_relation(source, target, shift) {
            return;
        }
        self.start_relation(source, target, shift);
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
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_relation(source, target, shift);
        }
    }

    pub(super) fn start_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) {
        let job = Job::Relation {
            source,
            target,
            shift,
        };
        if self.loading.is_some() || self.analysis.iter().any(|analysis| analysis.job() == job) {
            return;
        }
        let prepared = {
            let Some(scene) = self.scene.as_ref() else {
                return;
            };
            let (Some(from), Some(to)) = (scene.objects.get(source), scene.objects.get(target))
            else {
                return;
            };
            if to.detected() == 0 {
                self.message = Some(Message::error(
                    "the target object set holds no objects to relate to",
                ));
                return;
            }
            let frames = scene.frame_count();
            let gate = scene.relation_gate(source, target);
            let source_frames = (0..frames)
                .map(|time| from.frame_shared(time))
                .collect::<Vec<_>>();
            let target_frames = (0..frames)
                .map(|time| to.frame_shared(time))
                .collect::<Vec<_>>();
            (source_frames, target_frames, gate, frames as usize)
        };
        let (source_frames, target_frames, gate, total) = prepared;
        self.message = None;
        self.record_progress(job, 0, 0, total);
        self.analysis.push(crate::relation(
            job,
            source_frames,
            target_frames,
            shift,
            source == target,
            gate,
            0,
        ));
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
        self.cancel(Job::Recut(set));
        if self.recut.is_some_and(|(pending, _)| pending == set) {
            self.recut = None;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::Detect(set))
        {
            return;
        }
        self.cancel(Job::Track(set));
        if self.recipe.is_none() {
            self.defer_relations(set);
            self.clear_relations(set);
        }
        if let Some(scene) = self.scene.as_mut() {
            scene.clear_tracks(set);
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
        let channel = object.channel();
        if channel >= scene.base_channels() {
            let spec = scene.derived()[channel - scene.base_channels()].spec();
            if let Some(set) = spec.mask() {
                let ready = scene.objects.get(set).is_some_and(|set| set.detected() > 0);
                if !ready {
                    self.message = Some(Message::error(
                        "the mask channel needs its object set detected",
                    ));
                    return;
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
            return;
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
                        return;
                    }
                    let detected = scene
                        .objects
                        .get(source)
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
            }
        };
        if scene.dataset.channels()[channel].intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to detect",
            ));
            return;
        }
        if region.enabled() && region.voxel_range(channels.pyramid().level(0)).is_none() {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        if let ObjectParams::Cells(params) = params {
            let Some(source) = seed_source else {
                self.message = Some(Message::error("the cells grow from a nucleus object set"));
                return;
            };
            let Some(nuclei) = scene.objects.get(source) else {
                return;
            };
            if nuclei.detected() == 0 {
                self.message = Some(Message::error(
                    "the nucleus object set holds no detected objects",
                ));
                return;
            }
            let sources = (0..scene.frame_count())
                .map(|time| nuclei.frame_shared(time))
                .collect::<Vec<_>>();
            self.message = None;
            self.record_progress(Job::Detect(set), 0, 0, 0);
            if let Some(scene) = self.scene.as_mut() {
                scene.clear_object(set);
            }
            self.analysis.push(crate::grow(
                Job::Detect(set),
                Arc::clone(&channels),
                GrowRequest::new(channel, params, region, drifts, roi, sources),
            ));
            return;
        }
        self.message = None;
        self.record_progress(Job::Detect(set), 0, 0, 0);
        if let Some(params) = segment {
            let request = SegmentRequest::new(channel, params, region, drifts, roi);
            let request = if seeds.iter().any(|frame| !frame.is_empty()) {
                request.with_seeds(seeds)
            } else {
                request
            };
            self.analysis.push(crate::segment(
                Job::Detect(set),
                Arc::clone(&channels),
                request,
            ));
            return;
        }
        let policy = match seed_source {
            Some(_) => grana_pipeline::SeedPolicy::Sourced,
            None if seeds.iter().any(|frame| !frame.is_empty()) => {
                grana_pipeline::SeedPolicy::Strict
            }
            None => grana_pipeline::SeedPolicy::Unseeded,
        };
        self.analysis.push(crate::detect(
            Job::Detect(set),
            channels,
            DetectRequest::new(channel, params, region, roi, drifts, 0, self.store())
                .with_seeds(seeds, policy),
        ));
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
            return;
        };
        if previous == params
            || previous.threshold() != params.threshold()
            || previous.grain() != params.grain()
            || previous.opening() != params.opening()
            || previous.closing() != params.closing()
        {
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
                object.frame(time).is_none_or(|frame| match frame {
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
        let frames = (0..scene.frame_count())
            .map(|time| object.frame_shared(time))
            .collect::<Vec<_>>();
        if frames.iter().all(Option::is_none) {
            return;
        }
        self.message = None;
        self.record_progress(Job::Recut(set), 0, 0, frames.len());
        self.analysis
            .push(crate::recut(Job::Recut(set), frames, cut, 0, self.store()));
    }

    pub(super) fn store(&self) -> Arc<GeometryStore> {
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
        self.analysis.push(crate::track(
            Job::Track(set),
            detections,
            params,
            frame_interval,
        ));
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
        self.analysis.push(crate::coloc(
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
        ));
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
        let frames = (0..scene.frame_count())
            .map(|time| object.frame_shared(time))
            .collect::<Vec<_>>();
        let drifts = scene.drifts().clone();
        self.message = None;
        self.record_progress(Job::ObjectColoc(set), 0, 0, 0);
        self.analysis.push(crate::object_coloc(
            Job::ObjectColoc(set),
            channels,
            grana_pipeline::run::ObjectColocRequest::new(pair, region, frames, drifts, 0),
        ));
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
        let mut frames = Vec::with_capacity(scene.frame_count() as usize);
        for time in 0..scene.frame_count() {
            let mut points = Vec::new();
            if let Some(frame) = object.frame(time) {
                for (index, spot) in frame.spots_of().iter().enumerate() {
                    if object.visible_object(time, index) {
                        points.push(grana_analysis::LocalizationPoint::new(
                            spot.center(),
                            spot.sigma(),
                        ));
                    }
                }
            }
            frames.push(points);
        }
        self.message = None;
        self.record_progress(Job::Drift, 0, 0, 0);
        self.analysis.push(crate::localization_drift(
            Job::Drift,
            grana_pipeline::run::LocalizationDriftRequest::new(frames, reference, channel, base),
        ));
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
        self.analysis.push(crate::drift(
            Job::Drift,
            channels,
            grana_pipeline::DriftRequest::new(channel, reference),
        ));
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
        self.analysis.push(crate::fiducial(
            Job::Register,
            grana_pipeline::run::FiducialRequest::new(reference, moving, channels, scale),
        ));
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
        self.analysis.push(crate::register(
            Job::Register,
            channels,
            grana_pipeline::RegisterRequest::new(reference, stored.1),
        ));
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

    pub(super) fn realign(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let drifts = scene.drifts().clone();
        if let Some(source) = self.source.as_ref() {
            source.set_drifts(drifts);
        }
        self.in_flight.clear();
        self.bricks.clear();
        self.reloaded();
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
            let Some(frame) = object.frame(time) else {
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
        let Some(frame) = object.frame(time) else {
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
