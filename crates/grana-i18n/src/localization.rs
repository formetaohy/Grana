use crate::labels;
use crate::messages;

labels! {
    title => "Super-Res" / "超分辨";
    render => "Render" / "渲染";
    export_image => "Export Image…" / "导出图像…";
    clear_image => "Clear Image" / "清除图像";
    pixel_size => "Pixel" / "像素";
    blur_width => "Blur" / "模糊";
    precision => "Precision" / "精度";
    resolution => "Resolution" / "分辨率";
    neighbours => "Neighbours" / "最近邻";
    correlation => "Fourier ring correlation" / "傅里叶环相关";
    resolution_missing => "No ring reaches the 1/7 threshold" / "没有环达到 1/7 阈值";
    stale => "The parameters changed since the last render" / "参数已不同于上次渲染";
    curve_data => "Measured" / "实测";
    curve_fit => "Fit" / "拟合";
    curve_smooth => "Smoothed" / "平滑";
    curve_raw => "Rings" / "各环";
    threshold => "1/7" / "1/7";
}

messages! {
    summary(pixel: f32) => "{pixel:.1} nm/px" / "{pixel:.1} nm/像素";
    span(start: u32, end: u32) => "frames {start}–{end}" / "第 {start}–{end} 帧";
    nanometres(value: f32) => "{value:.1} nm" / "{value:.1} nm";
    neighbour_pairs(pairs: f64) => "{pairs:.0} pairs" / "{pairs:.0} 对";
    localizations(count: u64) => "{count} localizations" / "{count} 个定位";
}
