use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_render::Detail;
use grana_scene::Scene;
use grana_volume::{BrickGrid, BrickRequest, PackedBrick, Pyramid};
use rayon::prelude::*;

use crate::plan::{self, Plan};
use crate::task::Bricks;

const CHUNK: usize = 64;

const CACHE_BYTES: u64 = 256 << 20;

pub(crate) struct Probe {
    pub(crate) detail: Detail,
    pub(crate) hollow: Vec<(u32, usize, Vec<u32>)>,
    pub(crate) cache: Cache,
}

pub(crate) struct Scan<'a> {
    pub(crate) plan: &'a Plan,
    pub(crate) pyramid: &'a Pyramid,
    pub(crate) scene: &'a Scene,
    pub(crate) bricks: &'a dyn Bricks,
    pub(crate) limit: usize,
    pub(crate) cancel: &'a AtomicBool,
    pub(crate) progress: &'a mut dyn FnMut(u32, u32),
}

pub(crate) fn scan(scan: Scan<'_>) -> Result<Option<Probe>, String> {
    let Scan {
        plan,
        pyramid,
        scene,
        bricks,
        limit,
        cancel,
        progress,
    } = scan;
    assert!(limit > 0, "an export that probes holds at least one brick");
    let size = plan.size.to_array();
    let ladders = plan
        .poses
        .iter()
        .map(|pose| {
            let camera = plan::camera(&pose.camera);
            (
                pose.time,
                grana_render::visible_bricks(pyramid, &camera, size, plan.region),
            )
        })
        .collect::<Vec<_>>();
    let total = ladders
        .iter()
        .flat_map(|(_, ladder)| ladder.iter().map(|visible| visible.bricks.len()))
        .sum::<usize>() as u32;
    let mut visited = 0u32;
    let mut known = HashMap::<(u32, usize, u32), bool>::new();
    let mut cache = Cache::new(CACHE_BYTES);
    let mut counts = Vec::with_capacity(ladders.len());
    let mut finest = pyramid.level_count() - 1;
    for (time, ladder) in &ladders {
        let warps = scene.drift_warps(*time);
        let mut count = 0usize;
        for visible in ladder {
            finest = finest.min(visible.level);
            let grid = BrickGrid::new(pyramid.level(visible.level).extent());
            for chunk in visible.bricks.chunks(CHUNK) {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let mut fresh = Vec::with_capacity(chunk.len());
                for index in chunk {
                    match known.get(&(*time, visible.level, *index)) {
                        Some(true) => {}
                        Some(false) => count += 1,
                        None => fresh.push(*index),
                    }
                }
                let read = fresh
                    .par_iter()
                    .map(|index| {
                        let request =
                            BrickRequest::new(*time, visible.level, grid.coord(*index as usize));
                        let data = bricks.read(request, &warps)?;
                        Ok((
                            *index,
                            request,
                            PackedBrick::of(&data, pyramid, request.channels),
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                for (index, request, packed) in read {
                    let hollow = packed
                        .packs()
                        .map(|(_, block)| block.maximum())
                        .fold(0.0f32, f32::max)
                        <= 0.0;
                    known.insert((*time, visible.level, index), hollow);
                    if hollow {
                        continue;
                    }
                    count += 1;
                    cache.insert(request, packed);
                }
                visited += chunk.len() as u32;
                progress(visited, total);
                if count > limit {
                    return Err(over(limit));
                }
            }
        }
        counts.push(count);
    }
    let mut grouped = BTreeMap::<(u32, usize), Vec<u32>>::new();
    for ((time, level, index), hollow) in &known {
        if *hollow {
            grouped.entry((*time, *level)).or_default().push(*index);
        }
    }
    Ok(Some(Probe {
        detail: Detail {
            level: finest,
            bricks: counts.iter().copied().max().unwrap_or(0),
        },
        hollow: grouped
            .into_iter()
            .map(|((time, level), indices)| (time, level, indices))
            .collect(),
        cache,
    }))
}

pub(crate) fn over(limit: usize) -> String {
    format!(
        "the volume holds more than {limit} bricks with content at the detail this export promises, more than the graphics device leaves for the export; lower the export resolution"
    )
}

pub(crate) struct Cache {
    entries: HashMap<BrickRequest, PackedBrick>,
    order: VecDeque<BrickRequest>,
    bytes: u64,
    limit: u64,
}

impl Cache {
    pub(crate) fn empty() -> Self {
        Self::new(0)
    }

    fn new(limit: u64) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    fn insert(&mut self, request: BrickRequest, packed: PackedBrick) {
        let bytes = packed.bytes();
        if bytes > self.limit || self.entries.contains_key(&request) {
            return;
        }
        self.bytes += bytes;
        self.entries.insert(request, packed);
        self.order.push_back(request);
        while self.bytes > self.limit {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(dropped) = self.entries.remove(&oldest) {
                self.bytes -= dropped.bytes();
            }
        }
    }

    pub(crate) fn take(&mut self, request: &BrickRequest) -> Option<PackedBrick> {
        let packed = self.entries.remove(request)?;
        self.bytes -= packed.bytes();
        Some(packed)
    }
}
