use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::{Link, TrackParams};

use super::assign::{self, Edge};
use super::chain::Chains;
use super::detection::{Detections, Grid};

const DUMMY_COST: f64 = 1.0;
const GAP_COST: f64 = 0.25;

struct Linker {
    chains: Chains,
    velocity: Vec<Vec3>,
    open: Vec<usize>,
}

impl Linker {
    fn new() -> Self {
        Self {
            chains: Chains::new(),
            velocity: Vec::new(),
            open: Vec::new(),
        }
    }

    fn push(&mut self, point: usize, frame: u32, velocity: Vec3) {
        let index = self.chains.push(point, frame);
        self.velocity.push(velocity);
        self.open.push(index);
    }

    fn extend(&mut self, index: usize, point: usize, frame: u32, detections: &Detections) {
        let previous = *self
            .chains
            .points(index)
            .last()
            .expect("a chain holds points");
        let elapsed = (frame - detections.frame(previous)) as f32;
        self.chains.extend(index, point, frame);
        self.velocity[index] =
            (detections.centroid(point) - detections.centroid(previous)) / elapsed;
    }

    fn attach_gap(&mut self, parent: usize, point: usize, frame: u32, detections: &Detections) {
        let child = self.chains.push(point, frame);
        let previous = *self
            .chains
            .points(parent)
            .last()
            .expect("a chain holds points");
        let elapsed = (frame - detections.frame(previous)) as f32;
        self.velocity
            .push((detections.centroid(point) - detections.centroid(previous)) / elapsed);
        self.chains.attach(parent, child, Link::Gap);
        self.open.push(child);
    }

    fn advance(&mut self, frame: usize, span: usize) {
        self.open.retain(|&index| {
            let chain = self.chains.chain(index);
            chain.outgoing() == 0
                && chain.end() < frame as u32
                && chain.end() as usize + span >= frame
        });
    }

    fn rows(
        &self,
        detections: &Detections,
        frame: usize,
        step: f32,
        grids: &[Grid<'_>],
    ) -> Vec<Vec<Edge>> {
        let targets = detections.span(frame);
        let count = targets.len();
        let mut rows = Vec::with_capacity(self.open.len());
        for (row, &index) in self.open.iter().enumerate() {
            let chain = self.chains.chain(index);
            let last = *chain.points().last().expect("a chain holds points");
            let reach = frame as u32 - chain.end();
            let predicted = detections.centroid(last) + self.velocity[index] * reach as f32;
            let grid = &grids[reach as usize - 1];
            let gate = detections.gate(last, reach as f32 * step);
            let mut edges = Vec::new();
            grid.near(predicted, gate, |target, distance| {
                let closeness = 1.0 - distance;
                if closeness <= 0.0 {
                    return;
                }
                edges.push(Edge {
                    column: target - targets.start,
                    cost: 1.0 - f64::from(closeness) + GAP_COST * f64::from(reach - 1),
                });
            });
            edges.push(Edge {
                column: count + row,
                cost: DUMMY_COST,
            });
            rows.push(edges);
        }
        rows
    }
}

pub(super) fn link(
    detections: &Detections,
    params: TrackParams,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Option<Chains> {
    let frames = detections.frame_count();
    let total = 2 * frames;
    let span = params.max_gap() as usize + 1;
    let step = params.max_step();
    let mut linker = Linker::new();
    for frame in 0..frames {
        progress(frame, total);
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        linker.advance(frame, span);
        let targets = detections.span(frame);
        let count = targets.len();
        if linker.open.is_empty() || count == 0 {
            for point in targets {
                linker.push(point, frame as u32, Vec3::ZERO);
            }
            continue;
        }
        let rows = {
            let grids = (1..=span)
                .map(|reach| detections.grid(frame, step * reach as f32))
                .collect::<Vec<_>>();
            linker.rows(detections, frame, step, &grids)
        };
        let sources = linker.open.clone();
        let assignment = assign::solve(&rows, count + sources.len(), cancel)?;
        let mut claimed = vec![false; count];
        for (row, &column) in assignment.iter().enumerate() {
            if column >= count {
                continue;
            }
            claimed[column] = true;
            let source = sources[row];
            let point = targets.start + column;
            if linker.chains.chain(source).end() + 1 == frame as u32 {
                linker.extend(source, point, frame as u32, detections);
            } else {
                linker.attach_gap(source, point, frame as u32, detections);
            }
        }
        for (column, point) in targets.enumerate() {
            if !claimed[column] {
                linker.push(point, frame as u32, Vec3::ZERO);
            }
        }
    }
    Some(linker.chains)
}
