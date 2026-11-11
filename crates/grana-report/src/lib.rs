mod chart;
mod comparison;
mod quality;
mod significance;

pub use chart::{Axis, Charted, Request, chart, group_names, set_names, sheet};
pub use comparison::{
    Comparison, ComparisonFrame, ComparisonReport, ComparisonSample, comparison, comparison_frames,
    comparison_of, samples,
};
pub use quality::{
    FrameValue, Metric, Quality, QualityDataset, QualityFrame, QualityInput, Summary, keys,
    of as quality_of, table,
};
pub use significance::{Significance, all as tests, frames, table as significance_table};
