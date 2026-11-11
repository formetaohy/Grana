use crate::labels;

labels! {
    two_channels_title => "Two Channels Required" / "需要两个通道";
    two_channels_detail => "Colocalization compares two channels across every frame in the region and analysis mask." / "共定位会在区域与分析掩膜的每一帧上比较两个通道。";
    no_coloc_title => "No Colocalization Yet" / "暂无共定位";
    no_coloc_streaming => "Streaming both channels over every frame of the current region and analysis mask." / "正在当前区域与分析掩膜的每一帧上流式读取两个通道。";
    no_coloc_detail => "Choose a channel pair, then compute Pearson overlap, Manders coefficients and a block permutation p-value." / "选择一对通道，然后计算 Pearson 重叠、Manders 系数与分块置换 p 值。";
    pair => "Pair" / "配对";
    compute => "Compute" / "计算";
    export_csv => "Export CSV" / "导出 CSV";
    remove_result => "Remove this result" / "移除此结果";
    thresholds => "Thresholds" / "阈值";
    scope => "Scope" / "范围";
    overlap => "Overlap" / "重叠";
    voxels => "Voxels" / "体素";
    series => "Series" / "序列";
    method => "Method" / "方法";
    sampling => "Sampling" / "采样";
    shuffles => "Shuffles" / "置换";
    precision => "Precision" / "精度";
    precision_full => "Full resolution" / "全分辨率";
    precision_balanced => "Balanced" / "均衡";
    precision_fast => "Fast" / "快速";
    joint_histogram => "Joint Histogram" / "联合直方图";
}

use crate::messages;

messages! {
    samples(done: usize, total: usize) => "{done}/{total} samples" / "{done}/{total} 个样本";
    raw(a: f32, b: f32) => "Raw {a:.0} · {b:.0}" / "原始 {a:.0} · {b:.0}";
    frame_row(time: u32) => "Frame {time}" / "第 {time} 帧";
    sampling_detail(coverage: f32, voxels: u64, frames: usize, blocks: u64, stride: u32) => "{coverage:.1}% · {voxels} vx · {frames}f · {blocks} blocks (s{stride})" / "{coverage:.1}% · {voxels} 体素 · {frames} 帧 · {blocks} 块 (s{stride})";
    all_frames_bins(bins: usize) => "All frames · {bins} bins" / "所有帧 · {bins} 组";
}
