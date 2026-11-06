use super::*;

impl Session {
    pub(super) fn poll_analysis(&mut self) -> bool {
        enum Update {
            Progress(u32, usize, usize),
            Frame(u32, ObjectFrame),
            Tracks(TrackSet),
            Relation(u32, grana_object::RelationFrame),
            Coloc(Box<grana_measure::ColocSeries>, Region),
            Drift(grana_scene::Drift),
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
                    Poll::Event(Event::Tracks(set)) => {
                        updates.push((index, job, Update::Tracks(set)));
                    }
                    Poll::Event(Event::Relation { time, frame }) => {
                        updates.push((index, job, Update::Relation(time, frame)));
                    }
                    Poll::Event(Event::Coloc { series, region }) => {
                        updates.push((index, job, Update::Coloc(series, region)));
                    }
                    Poll::Event(Event::Drift(drift)) => {
                        updates.push((index, job, Update::Drift(drift)));
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
                Update::Tracks(tracks) => {
                    if let (Job::Track(set), Some(scene)) = (job, self.scene.as_mut()) {
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
                Update::Drift(drift) => {
                    if let (Job::Drift, Some(scene)) = (job, self.scene.as_mut()) {
                        scene.set_drift(drift);
                    }
                    self.realign();
                }
                Update::Done => {
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    finished.push(job);
                    match job {
                        Job::Detect(set) => detected.push(set),
                        Job::Import(set) => imported.push(set),
                        Job::Recut(set) => recutted.push(set),
                        Job::Track(_) | Job::Relation { .. } | Job::Coloc | Job::Drift => {}
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
            self.start_tracks(set);
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
            gate,
            0,
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
        let Some(reader) = self.source.as_ref().and_then(Loader::reader) else {
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
        let params = object.params();
        let region = scene.region;
        let drift = scene.drift().clone();
        let seed_source = scene.seed_source(set);
        let seeds = scene.seed_frames(set);
        if let Some(source) = seed_source
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
        if region.enabled()
            && region
                .voxel_range(scene.dataset.pyramid().level(0))
                .is_none()
        {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        self.message = None;
        self.record_progress(Job::Detect(set), 0, 0, 0);
        let policy = match seed_source {
            Some(_) => grana_pipeline::SeedPolicy::Sourced,
            None if seeds.iter().any(|frame| !frame.is_empty()) => {
                grana_pipeline::SeedPolicy::Strict
            }
            None => grana_pipeline::SeedPolicy::Unseeded,
        };
        self.analysis.push(crate::detect(
            Job::Detect(set),
            reader,
            DetectRequest::new(channel, params, region, roi, drift, 0, self.store())
                .with_seeds(seeds, policy),
        ));
    }

    pub(super) fn recut(&mut self, set: ObjectSetId, previous: ObjectParams, params: ObjectParams) {
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
            detections[time as usize] = frame.centroids();
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
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Coloc)
        {
            return;
        }
        let Some(reader) = self.source.as_ref().and_then(Loader::reader) else {
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
        if region.enabled()
            && region
                .voxel_range(scene.dataset.pyramid().level(0))
                .is_none()
        {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        let settings = scene.coloc.settings();
        let drift = scene.drift().clone();
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
            reader,
            grana_pipeline::ColocRequest::new(
                pair,
                region,
                roi,
                mask.map(|set| set.raw()),
                settings,
                drift,
                0,
            ),
        ));
    }

    pub(super) fn start_drift(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let reference = scene.drift().reference();
        let channel = scene.selected;
        self.align_drift(channel, reference);
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
        let Some(reader) = self.source.as_ref().and_then(Loader::reader) else {
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
            reader,
            grana_pipeline::DriftRequest::new(channel, reference),
        ));
    }

    pub(super) fn set_drift_reference(&mut self, reference: u32) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let drift = scene.drift().clone();
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
        let drift = scene.drift().clone();
        if drift.is_empty() || time >= drift.frames() || time == drift.reference() {
            return;
        }
        if !offset.iter().all(|value| value.is_finite()) {
            return;
        }
        let offset =
            offset.map(|value| value.clamp(-grana_scene::Drift::LIMIT, grana_scene::Drift::LIMIT));
        if scene.set_drift(drift.with_offset(time, offset)) {
            self.realign();
        }
    }

    pub(super) fn clear_drift(&mut self) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if scene.set_drift(grana_scene::Drift::default()) {
            self.realign();
        }
    }

    pub(super) fn realign(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let drift = scene.drift().clone();
        if let Some(source) = self.source.as_ref() {
            source.set_drift(drift);
        }
        self.in_flight.clear();
        self.bricks.clear();
        self.reload += 1;
    }
}
