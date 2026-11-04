use super::*;

enum Next {
    Detect {
        set: ObjectSetId,
        mask: Option<ObjectSetId>,
    },
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
    },
    Coloc {
        pair: ColocPair,
        thresholds: ColocThresholds,
        mask: Option<ObjectSetId>,
    },
    Present,
}

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
        self.recipe = Some(Recipe::new(protocol.steps(), sets, pairs, settings));
        self.message = None;
        self.drive_recipe();
    }

    pub(super) fn drive_recipe(&mut self) {
        let Some(recipe) = self.recipe.as_ref() else {
            return;
        };
        let settings = recipe.settings();
        let next = match recipe.step() {
            Step::Detect { set, mask } => Next::Detect {
                set: recipe.set(set),
                mask: recipe.mask(mask),
            },
            Step::Relation { source, target } => Next::Relation {
                source: recipe.set(source),
                target: recipe.set(target),
            },
            Step::Coloc { pair, mask } => {
                let (pair, thresholds) = recipe.pair(pair);
                Next::Coloc {
                    pair,
                    thresholds,
                    mask: recipe.mask(mask),
                }
            }
            Step::Present => Next::Present,
        };
        match next {
            Next::Detect { set, mask } => {
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_mask(mask);
                }
                self.start_detect(set);
            }
            Next::Relation { source, target } => self.start_relation(source, target),
            Next::Coloc {
                pair,
                thresholds,
                mask,
            } => {
                self.coloc_thresholds = Some((pair, thresholds));
                if let Some(scene) = self.scene.as_mut() {
                    scene.coloc.set_settings(settings);
                    scene.coloc.set_pair(pair);
                    scene.set_mask(mask);
                }
                self.start_coloc();
            }
            Next::Present => {
                self.present_recipe();
                return;
            }
        }
        let running = self.recipe.as_ref().is_some_and(|recipe| {
            self.analysis
                .iter()
                .any(|analysis| Some(analysis.job()) == recipe.job())
        });
        if !running && self.recipe.is_some() {
            self.abort_recipe();
            if self.message.is_none() {
                self.message = Some(Message::error("the protocol could not start its next step"));
            }
        }
    }

    pub(super) fn advance_recipe(&mut self, job: Job) {
        let Some(recipe) = self.recipe.as_mut() else {
            return;
        };
        if recipe.job() != Some(job) {
            return;
        }
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
