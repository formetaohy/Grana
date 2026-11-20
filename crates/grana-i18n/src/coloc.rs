use crate::labels;
use crate::messages;

labels! {
    two_channels_title => "Two Channels Required" / "需要两个通道";
    no_coloc_title => "No Colocalization" / "暂无共定位";
    pair => "Pair" / "配对";
    compute => "Compute" / "计算";
    export_csv => "Export CSV" / "导出 CSV";
    remove_result => "Remove result" / "移除结果";
    thresholds => "Thresholds" / "阈值";
    scope => "Scope" / "范围";
    overlap => "Overlap" / "重叠";
    voxels => "Voxels" / "体素";
    series => "Series" / "序列";
    method => "Method" / "方法";
    sampling => "Sampling" / "采样";
    shuffles => "Shuffles" / "置换";
    precision => "Precision" / "精度";
    precision_full => "Full" / "全量";
    precision_balanced => "Balanced" / "均衡";
    precision_fast => "Fast" / "快速";
    joint_histogram => "Joint Histogram" / "联合直方图";
}

messages! {
    samples(done: usize, total: usize) => "{done}/{total}" / "{done}/{total}";
    raw(a: f32, b: f32) => "{a:.0} · {b:.0}" / "{a:.0} · {b:.0}";
    costes(a: f32, b: f32) => "Costes {a:.3} · {b:.3}" / "Costes {a:.3} · {b:.3}";
    frame_row(time: u32) => "Frame {time}" / "第 {time} 帧";
    sampling_detail(coverage: f32, voxels: u64, frames: usize, blocks: u64, stride: u32) => "{coverage:.1}% · {voxels} vx · {frames}f · {blocks}b (s{stride})" / "{coverage:.1}% · {voxels} 体素 · {frames} 帧 · {blocks} 块 (s{stride})";
    frame_bins(time: u32, bins: usize) => "F{time} · {bins}" / "F{time} · {bins}";
}
