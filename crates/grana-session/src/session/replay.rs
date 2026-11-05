use super::*;

impl Session {
    pub fn set_protocol(&mut self, protocol: Protocol) {
        self.protocol_text = Some(protocol.summary());
        self.protocol = Some(protocol);
        self.reset_batch_rows();
        self.batch_message = None;
    }

    pub fn replaying(&self) -> bool {
        self.recipe.is_some()
    }

    pub(super) fn capture_protocol(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let time = scene.time();
        let view = self
            .current_view
            .as_ref()
            .map(|view| ViewSpec::of(view, time));
        match Protocol::capture(scene, &self.measure, view) {
            Ok(protocol) => self.set_protocol(protocol),
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    pub(super) fn apply_protocol(&mut self) {
        let Some(protocol) = self.protocol.clone() else {
            self.message = Some(Message::error("capture or load a protocol first"));
            return;
        };
        if self.loading.is_some() {
            return;
        }
        if self.source.is_none() || self.scene.is_none() {
            self.message = Some(Message::error("open a dataset before applying a protocol"));
            return;
        }
        self.abort_recipe();
        self.analysis.clear();
        self.progress.clear();
        self.resume.clear();
        self.recut = None;
        if let Some(scene) = self.scene.as_ref()
            && let Err(error) = protocol.validate(scene)
        {
            self.message = Some(Message::error(error.to_string()));
            return;
        }
        let staged = {
            let Some(scene) = self.scene.as_mut() else {
                return;
            };
            scene.clear_objects();
            match protocol.stage(scene) {
                Ok(staged) => staged,
                Err(error) => {
                    self.message = Some(Message::error(error.to_string()));
                    return;
                }
            }
        };
        let sets = staged.sets().to_vec();
        let steps = staged.steps().to_vec();
        self.realign();
        let pairs = match protocol.coloc() {
            Some(coloc) => staged
                .pairs()
                .iter()
                .zip(&coloc.pairs)
                .map(|(pair, spec)| {
                    (
                        *pair,
                        ColocThresholds::new(spec.thresholds[0], spec.thresholds[1]),
                    )
                })
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let settings = protocol
            .coloc()
            .map_or_else(Default::default, |coloc| coloc.settings());
        self.recipe = Some(Recipe::new(steps, sets, pairs, settings));
        self.message = None;
        self.drive_recipe();
    }

    pub(super) fn drive_recipe(&mut self) {
        loop {
            let Some(recipe) = self.recipe.as_ref() else {
                return;
            };
            let step = recipe.step().clone();
            let settings = recipe.settings();
            match step {
                Step::Present => {
                    self.present_recipe();
                    return;
                }
                Step::Drift {
                    channel,
                    reference,
                    offsets,
                } => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.set_drift(grana_scene::Drift::new(channel, reference, offsets));
                    }
                    self.realign();
                    self.advance_step();
                    continue;
                }
                Step::Detect { set, mask } => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.set_mask(mask);
                    }
                    self.start_detect(set);
                }
                Step::Relation {
                    source,
                    target,
                    shift,
                } => self.start_relation(source, target, shift),
                Step::Coloc { pair, mask } => {
                    let Some(recipe) = self.recipe.as_ref() else {
                        return;
                    };
                    let (pair, thresholds) = recipe.pair(pair);
                    self.coloc_thresholds = Some((pair, thresholds));
                    if let Some(scene) = self.scene.as_mut() {
                        scene.coloc.set_settings(settings);
                        scene.coloc.set_pair(pair);
                        scene.set_mask(mask);
                    }
                    self.start_coloc();
                }
                Step::Filter { set, .. } => match self.reload_filtered(set) {
                    Ok(()) => {
                        self.derive_settled(set, true);
                        self.advance_step();
                        continue;
                    }
                    Err(detail) => {
                        self.abort_recipe();
                        self.message = Some(Message::error(detail));
                        return;
                    }
                },
                Step::Copy { set, source } => match self.copy_frames(set, source) {
                    Ok(()) => {
                        self.derive_settled(set, false);
                        self.advance_step();
                        continue;
                    }
                    Err(detail) => {
                        self.abort_recipe();
                        self.message = Some(Message::error(detail));
                        return;
                    }
                },
                Step::Merge { set, part } => {
                    let waiting = self.analysis.iter().any(|analysis| {
                        analysis.job().involves(set) || analysis.job().involves(part)
                    });
                    if waiting {
                        return;
                    }
                    match self.merge_sets(set, part) {
                        Ok(()) => {
                            self.advance_step();
                            continue;
                        }
                        Err(detail) => {
                            self.abort_recipe();
                            self.message = Some(Message::error(detail));
                            return;
                        }
                    }
                }
            }
            let running = self.recipe.as_ref().is_some_and(|recipe| {
                self.analysis
                    .iter()
                    .any(|analysis| recipe.matches(analysis.job()))
            });
            if !running && self.recipe.is_some() {
                self.abort_recipe();
                if self.message.is_none() {
                    self.message =
                        Some(Message::error("the protocol could not start its next step"));
                }
            }
            return;
        }
    }

    pub(super) fn advance_step(&mut self) {
        if let Some(recipe) = self.recipe.as_mut() {
            recipe.advance();
        }
    }

    fn derive_settled(&mut self, set: ObjectSetId, track: bool) {
        self.defer_relations(set);
        if self.recipe.is_none() {
            self.resume_relations(set);
        }
        if track {
            self.start_tracks(set);
        }
    }

    pub(super) fn advance_recipe(&mut self, job: Job) {
        let Some(recipe) = self.recipe.as_ref() else {
            return;
        };
        if recipe.job().is_none() {
            self.drive_recipe();
            return;
        }
        if !recipe.matches(job) {
            return;
        }
        let waiting = self.analysis.iter().any(|analysis| {
            recipe
                .step_sets()
                .iter()
                .any(|set| analysis.job().involves(*set))
        });
        if waiting {
            return;
        }
        let Some(recipe) = self.recipe.as_mut() else {
            return;
        };
        recipe.advance();
        self.drive_recipe();
    }

    pub(super) fn present_recipe(&mut self) {
        let Some(recipe) = self.recipe.take() else {
            return;
        };
        self.coloc_thresholds = None;
        let Some(protocol) = self.protocol.clone() else {
            return;
        };
        if let Some(view) = protocol.view() {
            if let Some(scene) = self.scene.as_mut() {
                let time = view.time.min(scene.frame_count().saturating_sub(1));
                scene.set_time(time);
                self.in_flight.retain(|request| request.time >= time);
                if let Some(source) = self.source.as_ref() {
                    source.set_time(time);
                }
            }
            if let Some(timeline) = view.timeline() {
                self.timeline = timeline;
            }
            if let Some(export) = view.export() {
                self.export = export;
            }
            self.view = Some(ViewState {
                camera: view.camera(),
                section: view.section(),
                split: view.split,
                timeline: self.timeline.clone(),
                export: self.export,
            });
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        protocol.present_measure(recipe.sets(), &mut self.measure, scene);
    }

    pub(super) fn abort_recipe(&mut self) {
        self.recipe = None;
        self.coloc_thresholds = None;
    }

    pub(super) fn load_protocol(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open an analysis protocol")
            .add_filter("grana protocol", &["json"])
            .pick_file()
        else {
            return;
        };
        match Protocol::read(&path) {
            Ok(protocol) => self.set_protocol(protocol),
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    pub(super) fn save_protocol(&mut self) {
        if self.protocol.is_none() {
            self.capture_protocol();
        }
        let Some(protocol) = self.protocol.as_ref() else {
            return;
        };
        let suggested = format!(
            "{}-protocol.json",
            crate::slug(
                self.scene
                    .as_ref()
                    .map_or("grana", |scene| scene.dataset.name())
            )
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save the analysis protocol")
            .set_file_name(suggested)
            .add_filter("grana protocol", &["json"])
            .save_file()
        else {
            return;
        };
        match protocol.write(&path) {
            Ok(()) => self.batch_message = None,
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }
}
