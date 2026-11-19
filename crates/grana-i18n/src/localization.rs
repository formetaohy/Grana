use crate::labels;
use crate::messages;

labels! {
    title => "Super-Res" / "超分辨";
    render => "Render" / "渲染";
    export_image => "Export Image…" / "导出图像…";
    export_composite => "Export Composed…" / "导出合成图…";
    clear_image => "Clear Image" / "清除图像";
    pixel_size => "Pixel" / "像素";
    blur_width => "Blur" / "模糊";
    slice => "Slice" / "层";
    slice_depth => "Slice depth" / "层厚";
    projected => "Project frames" / "投影帧区间";
    precision => "Precision" / "精度";
    precision_missing => "—" / "—";
    resolution => "Resolution" / "分辨率";
    groups => "Groups" / "分组";
    ungrouped => "Ungrouped" / "未分组";
    neighbours => "Neighbours" / "最近邻";
    correlation => "Fourier ring correlation" / "傅里叶环相关";
    resolution_missing => "—" / "—";
    curve_data => "Measured" / "实测";
    curve_fit => "Fit" / "拟合";
    curve_smooth => "Smoothed" / "平滑";
    curve_raw => "Rings" / "各环";
    threshold => "1/7" / "1/7";
    molecules => "Molecules" / "分子";
    radius => "Radius" / "半径";
    dark => "Dark frames" / "暗帧";
    minimum => "Min locs" / "最少定位";
    cluster => "Cluster" / "聚类";
    estimate => "Estimate" / "估计";
    photons => "Min photons" / "最少光子";
    precision_limit => "Max precision" / "最大精度";
    color_by_group => "Color by group" / "按分组着色";
}

messages! {
    summary(pixel: f32) => "{pixel:.1} nm/px" / "{pixel:.1} nm/px";
    span(start: u32, end: u32) => "F{start}–{end}" / "F{start}–{end}";
    nanometres(value: f32) => "{value:.1} nm" / "{value:.1} nm";
    neighbour_pairs(pairs: f64) => "{pairs:.0}" / "{pairs:.0}";
    localizations(count: u64) => "{count} locs" / "{count} 个定位";
    molecule_count(count: usize) => "{count}" / "{count}";
    group_count(count: usize) => "{count}" / "{count}";
    groups_truncated(shown: usize, total: usize) => "{shown}/{total}" / "{shown}/{total}";
    group(index: u32) => "Group {index}" / "分组 {index}";
    group_detail(count: u64, precision: f32) => "{count} · {precision:.1} nm" / "{count} · {precision:.1} nm";
    slice_range(low: f32, high: f32) => "{low:.1}–{high:.1} nm" / "{low:.1}–{high:.1} nm";
}
