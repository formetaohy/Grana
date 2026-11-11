use crate::labels;

labels! {
    measure => "Measure" / "测量";
    click_point => "Click a surface to drop a measurement point." / "点击表面放置测量点。";
    click_distance => "Click two surfaces to measure their distance." / "点击两个表面以测量它们之间的距离。";
    click_roi => "Click the volume to place the region of interest." / "点击体数据以放置感兴趣区域。";
    choose_tool => "Choose a tool, then click the volume." / "选择工具，然后点击体数据。";
    annotations => "Annotations" / "标注";
    nothing_measured => "Nothing is measured yet. Points and distances stay with the project and the protocol." / "尚未进行任何测量。点与距离会随项目和协议保留。";
    remove_all => "Remove All" / "全部移除";
    hide => "Hide" / "隐藏";
    show => "Show" / "显示";
    single_frame_scope => "single frame" / "单帧";
    every_frame_scope => "every frame" / "每一帧";
    single_frame_detail => "The dataset holds a single frame." / "数据集仅包含一帧。";
    shown_every_frame => "Shown on every frame." / "在所有帧上显示。";
    on_every_frame => "On every frame" / "在所有帧显示";
    intensity_profile => "Intensity Profile" / "强度剖面";
    empty_profile => "Sample the line to draw its intensity profile." / "采样该线段以绘制强度剖面。";
    resample => "Resample" / "重新采样";
    sample_profile => "Sample Profile" / "采样剖面";
    region_of_interest => "Region of Interest" / "感兴趣区域";
    region_detail => "Statistics, detection, drift and batch stay inside an enabled region. Click the volume to draw one." / "统计、检测、漂移校正和批处理都只作用于启用的区域。点击体数据即可绘制。";
    restrict_region => "Restrict to region" / "限制在区域内";
    whole_volume => "whole volume" / "整个体数据";
    paint_mask => "Paint Mask" / "绘制掩膜";
    tolerance => "Tolerance" / "容差";
    radius => "Radius" / "半径";
    open_remote => "Open Remote OME-Zarr" / "打开远程 OME-Zarr";
    http_range => "HTTP range access" / "HTTP 范围访问";
    remote_tip => "Paste the https url of an OME-Zarr dataset or plate" / "粘贴 OME-Zarr 数据集或孔板的 https URL";
    remote_hint => "Return opens the dataset · Escape cancels" / "回车打开数据集 · Esc 取消";
}

use crate::messages;

messages! {
    placed(count: usize) => "{count} placed" / "已放置 {count} 个";
    point_detail(scope: &str) => "point · {scope}" / "点 · {scope}";
    distance_detail(length: &str, scope: &str) => "distance · {length} · {scope}" / "距离 · {length} · {scope}";
    frame_scope(frame: u32, total: u32) => "frame {frame} of {total}" / "第 {frame} 帧，共 {total} 帧";
    placed_frame(frame: u32, total: u32) => "Placed on frame {frame} of {total}" / "放置于第 {frame} 帧，共 {total} 帧";
    profile_detail(samples: usize, channels: usize) => "{samples} samples · {channels} channels" / "{samples} 个采样 · {channels} 个通道";
    profile_span(span: f32, spacing: f32) => "0 – {span:.2} µm · every {spacing:.3} µm" / "0 – {span:.2} µm · 每 {spacing:.3} µm";
    recording(frame: u32, frames: u32) => "REC · frame {frame} of {frames}" / "REC · 第 {frame} 帧，共 {frames} 帧";
}
