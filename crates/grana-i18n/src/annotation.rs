use crate::labels;
use crate::messages;

labels! {
    measure => "Measure" / "测量";
    annotations => "Annotations" / "标注";
    nothing_measured => "No annotations" / "暂无标注";
    remove_all => "Remove All" / "全部移除";
    hide => "Hide" / "隐藏";
    show => "Show" / "显示";
    single_frame_scope => "1f" / "单帧";
    every_frame_scope => "all" / "全部帧";
    on_every_frame => "Every frame" / "在所有帧显示";
    intensity_profile => "Intensity Profile" / "强度剖面";
    profile_axis => "intensity" / "强度";
    profile_offset => "distance (µm)" / "距离 (µm)";
    empty_profile => "No profile sampled" / "未采样剖面";
    resample => "Resample" / "重新采样";
    sample_profile => "Sample Profile" / "采样剖面";
    region_of_interest => "Region of Interest" / "感兴趣区域";
    restrict_region => "Restrict to region" / "限制在区域内";
    whole_volume => "whole volume" / "整个体数据";
    paint_mask => "Paint Mask" / "绘制掩膜";
    tolerance => "Tolerance" / "容差";
    radius => "Radius" / "半径";
    open_remote => "Open Remote OME-Zarr" / "打开远程 OME-Zarr";
}

messages! {
    placed(count: usize) => "{count}" / "{count}";
    point_detail(scope: &str) => "{scope}" / "{scope}";
    distance_detail(length: &str, scope: &str) => "{length} · {scope}" / "{length} · {scope}";
    frame_scope(frame: u32, total: u32) => "F{frame}/{total}" / "F{frame}/{total}";
    profile_detail(samples: usize, channels: usize) => "{samples} · {channels}C" / "{samples} · {channels}C";
    profile_span(span: f32, spacing: f32) => "0 – {span:.2} µm · Δ{spacing:.3} µm" / "0 – {span:.2} µm · Δ{spacing:.3} µm";
    recording(frame: u32, frames: u32) => "REC · {frame}/{frames}" / "REC · {frame}/{frames}";
}
