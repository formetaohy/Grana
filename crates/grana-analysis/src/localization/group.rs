use std::collections::HashMap;

use grana_display::{CATEGORICAL, group_tint, srgb_to_linear};

use super::LocalizationFrame;

const TRACKED: usize = 32;

const NANOMETRES_PER_MICROMETRE: f64 = 1000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct GroupStat {
    group: Option<u32>,
    localizations: u64,
    precision: f32,
    photons: f32,
    tint: [f32; 3],
}

impl GroupStat {
    pub fn group(&self) -> Option<u32> {
        self.group
    }

    pub fn localizations(&self) -> u64 {
        self.localizations
    }

    pub fn precision(&self) -> f32 {
        self.precision
    }

    pub fn photons(&self) -> f32 {
        self.photons
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Groups {
    stats: Vec<GroupStat>,
    chromatic: bool,
}

impl Groups {
    pub fn stats(&self) -> &[GroupStat] {
        &self.stats
    }

    pub fn chromatic(&self) -> bool {
        self.chromatic
    }

    pub(super) fn tint_slot(&self, group: Option<u32>) -> u8 {
        self.stats
            .iter()
            .position(|stat| stat.group == group)
            .map_or(u8::MAX, |slot| slot as u8)
    }

    pub(super) fn slot_tint(&self, slot: u8) -> [f32; 3] {
        self.stats
            .get(slot as usize)
            .map_or(grana_display::GROUP_NEUTRAL, |stat| stat.tint)
    }

    pub(super) fn measure(frames: &[LocalizationFrame<'_>]) -> Self {
        let mut tally = Tally::default();
        for frame in frames {
            for (index, spot) in frame.spots().iter().enumerate() {
                if !frame.visible(index) {
                    continue;
                }
                let sigma = spot.sigma();
                tally.push(
                    spot.group(),
                    0.5 * (f64::from(sigma[0]) + f64::from(sigma[1])),
                    f64::from(spot.quality()),
                );
            }
        }
        tally.finish()
    }
}

#[derive(Default)]
struct Tally {
    entries: Vec<Entry>,
    index: HashMap<Option<u32>, usize>,
    held: Option<(Option<u32>, usize)>,
    overflow: bool,
}

struct Entry {
    group: Option<u32>,
    localizations: u64,
    sigma: f64,
    quality: f64,
}

impl Tally {
    fn push(&mut self, group: Option<u32>, sigma: f64, quality: f64) {
        let at = match self.held {
            Some((known, at)) if known == group => at,
            _ => {
                let at = match self.index.get(&group) {
                    Some(at) => *at,
                    None => {
                        if self.entries.len() >= TRACKED {
                            self.overflow = true;
                            return;
                        }
                        let at = self.entries.len();
                        self.entries.push(Entry {
                            group,
                            localizations: 0,
                            sigma: 0.0,
                            quality: 0.0,
                        });
                        self.index.insert(group, at);
                        at
                    }
                };
                self.held = Some((group, at));
                at
            }
        };
        let entry = &mut self.entries[at];
        entry.localizations += 1;
        entry.sigma += sigma;
        entry.quality += quality;
    }

    fn finish(mut self) -> Groups {
        if self.overflow {
            return Groups::default();
        }
        self.entries
            .sort_by_key(|entry| std::cmp::Reverse(entry.localizations));
        let chromatic = (2..=CATEGORICAL.len()).contains(&self.entries.len());
        let stats = self
            .entries
            .into_iter()
            .map(|entry| GroupStat {
                group: entry.group,
                localizations: entry.localizations,
                precision: (entry.sigma / entry.localizations as f64 * NANOMETRES_PER_MICROMETRE)
                    as f32,
                photons: (entry.quality / entry.localizations as f64) as f32,
                tint: srgb_to_linear(group_tint(entry.group)),
            })
            .collect();
        Groups { stats, chromatic }
    }
}
