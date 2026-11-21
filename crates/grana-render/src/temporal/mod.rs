mod convergence;
mod history;
mod resolve;

pub(crate) use convergence::Convergence;
pub(crate) use history::{History, STATISTICS_FORMAT};
pub(crate) use resolve::{Resolve, State};

pub(crate) const APERTURE_PHASE: u32 = 8;

pub(crate) const SAMPLE_BUDGET: u32 = 256;

pub(crate) const SAMPLE_FLOOR: u32 = 8;

pub(crate) const SAMPLE_STRIDE: u32 = 4;

pub(crate) const DRAW_TARGET: f32 = 32.0;

pub(crate) const ERROR_LIMIT: f32 = 2.0;
