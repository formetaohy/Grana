use crate::labels;
use crate::messages;

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
    merge => "Merge" / "融合";
    cancel_merge => "Cancel Merge" / "取消融合";
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
    not_tracked_yet => "Untracked" / "未追踪";
    select_join_target => "Select target" / "选择目标";
    select_daughters => "Select 2 daughters" / "选择 2 条子轨迹";
    select_merge_parents => "Select 2 parents" / "选择 2 条父轨迹";
    select_merge_parent => "Select 2nd parent" / "选择第 2 条父轨迹";
    select_parent => "Select parent" / "选择父轨迹";
    no_trackable_objects => "No Trackable Objects" / "无可追踪对象";
    single_time_frame => "Single Time Frame" / "单一时间帧";
    no_trajectories => "No Trajectories" / "暂无轨迹";
    max_step => "Max step" / "最大步长";
    max_gap => "Max gap" / "最大间隔";
    allow_division => "Allow Division" / "允许分裂";
    division_range => "Div. range" / "分裂范围";
    allow_merge => "Allow Merge" / "允许融合";
    merge_range => "Merge range" / "融合范围";
    merge_overlap => "Overlap" / "重叠";
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
    no_values => "No measured values" / "暂无测量值";
}

messages! {
    track_counts(tracks: usize, divisions: usize, merges: usize) => "{tracks} · ÷{divisions} · +{merges}" / "{tracks} · ÷{divisions} · +{merges}";
    track_summary(tracks: usize, divisions: usize, merges: usize, frames: usize) => "{tracks} · ÷{divisions} · +{merges} · {frames}f" / "{tracks} · ÷{divisions} · +{merges} · {frames}帧";
    frame_count(frames: u32) => "{frames}f" / "{frames} 帧";
    track_number(track: usize) => "Track #{track}" / "轨迹 #{track}";
    relation_metric(name: &str, field: &str, shift: &str, aggregate: &str) => "{name} {field}{shift} · {aggregate}" / "{name} {field}{shift} · {aggregate}";
    auto_range(minimum: &str, maximum: &str) => "{minimum} – {maximum}" / "{minimum} – {maximum}";
}
