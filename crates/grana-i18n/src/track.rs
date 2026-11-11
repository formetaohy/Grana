use crate::labels;

labels! {
    tracking => "Tracking" / "轨迹追踪";
    track => "Track" / "追踪";
    lineage => "Lineage" / "谱系";
    cut => "Cut" / "截断";
    join => "Join" / "连接";
    cancel_join => "Cancel Join" / "取消连接";
    export => "Export" / "导出";
    recompute => "Recompute" / "重新计算";
    divide => "Divide" / "分裂";
    parent => "Parent" / "父轨迹";
    parent_action => "Parent" / "更改父级";
    detach => "Detach" / "断开";
    daughters => "Daughters" / "子轨迹";
    root => "Root" / "根轨迹";
    duration => "Duration" / "时长";
    length => "Length" / "长度";
    displacement => "Displacement" / "位移";
    speed => "Speed" / "速度";
    straightness => "Straightness" / "直线度";
    selected_track => "Selected Track" / "所选轨迹";
    set => "Set" / "对象集";
    parameters => "Parameters" / "参数";
    not_tracked_yet => "Not tracked yet" / "尚未追踪";
    click_track => "Click a track in the lineage to inspect metrics." / "点击谱系中的轨迹以查看指标。";
    select_join_target => "Select target track to join" / "选择要连接的目标轨迹";
    select_daughters => "Select two daughter tracks" / "选择两条子轨迹";
    select_parent => "Select the parent track" / "选择父轨迹";
    no_trackable_objects => "No Trackable Objects" / "无可追踪对象";
    no_trackable_objects_detail => "Add spots or surfaces, detect them, then follow their trajectories through time." / "添加斑点或表面并完成检测，即可追踪它们随时间的运动轨迹。";
    single_time_frame => "Single Time Frame" / "单一时间帧";
    single_time_frame_detail => "Tracking follows objects across the frames of a time-series dataset." / "轨迹追踪需要在时间序列数据集的多个帧之间跟踪对象。";
    no_trajectories => "No Trajectories Yet" / "尚无轨迹";
    no_trajectories_detail => "Detect objects in this set first, then press Track to link their lineage across frames." / "请先在此对象集中检测对象，再按“追踪”跨帧连接其谱系。";
    max_step => "Max step" / "最大步长";
    max_gap => "Max gap" / "最大间隔";
    allow_division => "Allow Division" / "允许分裂";
    division_range => "Div. range" / "分裂范围";
    show_tubes => "Show Tubes" / "显示管道";
    tube_width => "Tube width" / "管道宽度";
    tube_coloring => "Tube Coloring" / "管道着色";
    uniform => "Uniform" / "统一颜色";
    color_by => "Color by" / "着色依据";
    target => "Target" / "目标对象";
    palette => "Palette" / "色板";
    reverse => "Rev" / "反转";
    range => "Range" / "范围";
    auto => "Auto" / "自动";
    fix_range => "Fix Range" / "固定范围";
    no_values => "No measured values to map yet." / "暂无可映射的测量值。";
}

use crate::messages;

messages! {
    track_counts(tracks: usize, divisions: usize) => "{tracks} tracks · {divisions} divisions" / "{tracks} 条轨迹 · {divisions} 次分裂";
    track_summary(tracks: usize, divisions: usize, frames: usize) => "{tracks} tracks · {divisions} divisions · {frames} frames" / "{tracks} 条轨迹 · {divisions} 次分裂 · {frames} 帧";
    frame_count(frames: u32) => "{frames} frames" / "{frames} 帧";
    track_number(track: usize) => "Track #{track}" / "轨迹 #{track}";
    relation_metric(name: &str, field: &str, shift: &str, aggregate: &str) => "{name} {field}{shift} · {aggregate}" / "{name} {field}{shift} · {aggregate}";
    auto_range(minimum: &str, maximum: &str) => "Auto: {minimum} – {maximum}" / "自动：{minimum} – {maximum}";
}
