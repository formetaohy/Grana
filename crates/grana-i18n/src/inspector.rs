use crate::labels;

labels! {
    channels => "Channels" / "通道";
    display => "Display" / "显示";
    camera => "Camera" / "相机";
    views => "Views" / "视图";
    region_of_interest => "Region of Interest" / "感兴趣区域";
    drift_correction => "Drift Correction" / "漂移校正";
    channel_registration => "Channel Registration" / "通道配准";
    analysis_mask => "Analysis Mask" / "分析掩膜";
    slice_view => "Slice View" / "切片视图";
    clipping_plane => "Clipping Plane" / "裁剪平面";
    rendering => "Rendering" / "渲染";
    surface_lighting => "Surface Lighting" / "表面光照";
    transfer_curve => "Transfer Curve" / "传递曲线";
    bead_calibration => "Bead Calibration" / "微球校准";

    projection => "Projection" / "投影";
    perspective => "Perspective" / "透视";
    orthographic => "Orthographic" / "正交";
    aperture => "Aperture" / "孔径";
    focus => "Focus" / "对焦";
    focus_pivot => "Focus Pivot" / "聚焦到枢轴";
    save_current_view => "Save Current View" / "保存当前视图";
    remove_view => "Remove view" / "移除视图";
    browse_wells => "Browse Wells" / "浏览孔位";
    no_field_of_view => "no field of view" / "无视野";
    series => "Series" / "序列";
    scene => "Scene" / "场景";

    iterations => "Iterations" / "迭代次数";
    emission => "Emission" / "发射波长";
    immersion => "Immersion" / "浸没";
    specimen => "Specimen" / "样本";
    depth => "Depth" / "深度";
    theoretical_model => "Theoretical Model" / "理论模型";
    opacity => "Opacity" / "不透明度";
    iso_value => "Iso Value" / "等值";
    blend => "Blend" / "混合";
    maximum_projection => "MIP" / "最大投影";
    isosurface => "Iso" / "等值面";
    auto => "Auto" / "自动";
    tinted => "Tinted" / "通道着色";
    gray => "Gray" / "灰度";
    sigma => "Sigma" / "σ";
    radius => "Radius" / "半径";
    smooth => "Smooth" / "平滑";
    background => "Background" / "背景";
    ratio => "Ratio" / "比值";
    bleach => "Bleach" / "漂白";
    channel_operators => "Channel operators" / "通道算子";
    export_dataset => "Export Dataset" / "导出数据集";
    remove_channel => "Remove Channel" / "移除通道";
    bead_threshold => "Bead Threshold" / "微球阈值";
    bead_spacing => "Bead Spacing" / "微球间距";
    deconvolve => "Deconvolve" / "去卷积";
    from_beads => "From Beads" / "从微球";

    cut_volume => "Cut Volume" / "剖切体数据";
    azimuth => "Azimuth" / "方位角";
    elevation => "Elevation" / "仰角";
    offset => "Offset" / "偏移";
    flip_plane => "Flip Plane" / "翻转平面";
    exposure => "Exposure" / "曝光";
    shadows => "Shadows" / "阴影";
    ambient => "Ambient" / "环境光";
    diffuse => "Diffuse" / "漫反射";
    reset_rendering => "Reset Rendering" / "重置渲染";

    limit_analysis => "Limit Analysis and Export" / "限制分析与导出";
    region_crops_export => "Exported datasets hold only this box." / "导出只写入此框内的数据。";
    align_to_axes => "Align to Axes" / "对齐坐标轴";
    fill_volume => "Fill Volume" / "填满体数据";
    active => "Active" / "已启用";
    inactive => "Off" / "已关闭";
    aperture_off => "off" / "关";
    aligning => "Aligning…" / "对齐中…";
    aligning_frames => "Aligning frames…" / "正在对齐帧…";
    correct_drift => "Correct Drift" / "校正漂移";
    registering => "Registering…" / "正在配准…";
    clear_drift => "Clear Drift" / "清除漂移";
    register_channels => "Register Channels" / "配准通道";
    from_localizations => "From Localizations" / "由定位表";
    clear_registration => "Clear Registration" / "清除配准";
    reference => "Reference" / "参考帧";
    frame_unit => "frame" / "帧";
    reference_suffix => " (ref)" / "（参考）";
    reference_fixed => "Reference frame is fixed at zero offset." / "参考帧固定为零偏移。";

    source => "Source" / "来源";
    grow_by => "Grow by" / "扩展";
    any_relation => "Any Relation" / "任意关系";
    relation => "Relation" / "关系";
    field => "Field" / "字段";
    relation_target => "the relation target" / "关系目标";
    detects_all => "Detects across the entire dataset." / "在整个数据集范围内检测。";

    uniform => "Uniform" / "统一";
    color_by => "Color by" / "着色依据";
    reverse => "Rev" / "反转";
    palette => "Palette" / "色板";
    range => "Range" / "范围";
    fix_range => "Fix Range" / "固定范围";
    no_measured => "No measured values to map yet." / "尚无可映射的测量值。";
    add_stop => "Add Stop" / "添加色标";
    remove_stop => "Remove stop" / "移除色标";

    slice_caption => "A slice view samples one plane orthographically." / "切片视图以正交投影采样单个平面。";
    parallel_caption => "Parallel rays hold one scale across depth." / "平行光线在纵深方向保持同一尺度。";
    slice_help => "One unlit voxel layer; drag pans, scroll zooms, ↑↓ or Ctrl-scroll steps." / "单个无光照体素层；拖动平移，滚轮缩放，↑↓ 或 Ctrl+滚轮逐层切换。";
}

use crate::messages;

messages! {
    visible_channels(enabled: usize, total: usize) => "{enabled}/{total} visible" / "{enabled}/{total} 可见";
    saved_views(count: usize) => "{count} saved" / "已保存 {count} 个";
    plate_summary(rows: usize, columns: usize, fields: usize) => "{rows} × {columns} wells · {fields} fields" / "{rows} × {columns} 孔 · {fields} 个视野";
    well_single_field(well: &str, fields: usize) => "Well {well} · {fields} field" / "{well} 孔 · {fields} 个视野";
    well_fields(well: &str, fields: usize) => "Well {well} · {fields} fields" / "{well} 孔 · {fields} 个视野";
    minimum_readout(start: f32) => "Min {start:.0}" / "最小 {start:.0}";
    maximum_readout(maximum: f32) => "Max {maximum:.0}" / "最大 {maximum:.0}";
    measured_psf(layers: u32, x: u32, y: u32, z: u32) => "Measured from beads · {layers} depth bands · {x}×{y}×{z}" / "由微球测得 · {layers} 个深度层 · {x}×{y}×{z}";
    divides(numerator: &str, denominator: &str) => "Divides {numerator} by {denominator}" / "{numerator} 除以 {denominator}";
    center(axis: &str) => "Center {axis}" / "中心 {axis}";
    size(axis: &str) => "Size {axis}" / "尺寸 {axis}";
    offset_axis(axis: &str) => "Offset {axis}" / "偏移 {axis}";
    aligning_progress(done: usize, total: usize) => "Aligning · {done}/{total} frames" / "对齐中 · {done}/{total} 帧";
    drift_frames(frames: u32) => "{frames} frames" / "{frames} 帧";
    aligns_channel(channel: &str) => "Aligns {channel} across all time frames." / "在所有时间帧上对齐 {channel}。";
    drift_summary(frames: u32, maximum: f32) => "{frames} frames aligned · max {maximum:.2} µm" / "已对齐 {frames} 帧 · 最大 {maximum:.2} µm";
    drift_deformation(maximum: f32) => "Deformation up to {maximum:.2} µm" / "形变达 {maximum:.2} µm";
    registering_channels(done: usize, total: usize) => "Channel {done} of {total}" / "第 {done} 个通道，共 {total} 个";
    registration_summary(channels: usize, maximum: f32) => "{channels} channels aligned · max {maximum:.2} µm" / "已对齐 {channels} 个通道 · 最大 {maximum:.2} µm";
    registration_deformation(maximum: f32) => "Deformation up to {maximum:.2} µm" / "形变达 {maximum:.2} µm";
    registration_hint(name: &str) => "Aligns every channel onto {name}." / "把所有通道对齐到 {name}。";
    frame_number(frame: u32) => "Frame {frame}" / "第 {frame} 帧";
    detects_relation(field: &str, minimum: f32, maximum: f32, suffix: &str) => "Detects only where the mask relation keeps objects whose {field} lies in {minimum:.2}{suffix} to {maximum:.2}{suffix}." / "仅检测掩膜关系中 {field} 位于 {minimum:.2}{suffix} 至 {maximum:.2}{suffix} 的对象。";
    detects_filtered(name: &str) => "Detects inside filtered objects of {name}." / "在 {name} 的筛选后对象内部检测。";
    detects_inside(name: &str) => "Detects inside {name} objects only." / "仅在 {name} 的对象内部检测。";
    target_without_mask(target: &str) => "{target} is detected without the mask so its relation can limit the mask." / "检测 {target} 时不应用掩膜，使其关系可限制掩膜。";
    auto_range(minimum: &str, maximum: &str) => "Auto: {minimum} – {maximum}" / "自动：{minimum} – {maximum}";
}
