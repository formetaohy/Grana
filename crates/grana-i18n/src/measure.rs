use crate::{Language, labels, messages};

pub fn term(language: Language, english: &str) -> Option<String> {
    let translated = match english {
        "frame" => "帧",
        "time" => "时间",
        "object" => "对象",
        "objects" => "对象数",
        "track" => "轨迹",
        "x" => "X",
        "y" => "Y",
        "z" => "Z",
        "diameter" => "直径",
        "contrast" => "对比度",
        "volume" => "体积",
        "area" => "面积",
        "sphericity" => "球形度",
        "max diameter" => "最大直径",
        "convex hull volume" => "凸包体积",
        "voxels" => "体素",
        "length" => "长度",
        "branches" => "分支数",
        "terminals" => "末端数",
        "junctions" => "连接点数",
        "nodes" => "节点数",
        "first frame" => "首帧",
        "last frame" => "末帧",
        "daughters" => "子轨迹数",
        "parent" => "父轨迹",
        "frames" => "帧数",
        "duration" => "时长",
        "displacement" => "位移",
        "speed" => "速度",
        "straightness" => "直线度",
        "mean" => "均值",
        "minimum" => "最小值",
        "maximum" => "最大值",
        "deviation" => "标准差",
        "sum" => "总和",
        "median" => "中位数",
        "quantile" => "分位数",
        "SD (σ)" => "标准差 (σ)",
        "distance" => "距离",
        "nearest neighbour" => "最近邻",
        "center distance" => "中心距离",
        "inside" => "内部占比",
        "partner" => "配对对象",
        "contact" => "接触面积",
        "partners" => "配对数量",
        "intensity" => "强度",
        "classifier" => "分类器",
        "target" => "目标",
        "children" => "子对象",
        "Pearson" => "皮尔逊",
        "Manders" => "曼德斯",
        "Costes" => "科斯特斯",
        "Overlap" => "重叠",
        "Full resolution" => "全分辨率",
        "Balanced" => "均衡",
        "Fast" => "快速",
        _ => return None,
    };
    Some(match language {
        Language::English => english.to_string(),
        Language::Chinese => translated.to_string(),
    })
}

labels! {
    set => "Set" / "对象集";
    view => "View" / "视图";
    table => "Table" / "表格";
    histogram => "Histogram" / "直方图";
    scatter => "Scatter" / "散点图";
    distribution => "Distribution" / "分布";
    trend => "Trend" / "趋势";
    ripley => "Ripley" / "Ripley";
    ripley_title => "Ripley L" / "Ripley L";
    ripley_curve => "L(r) − r" / "L(r) − r";
    no_spatial => "No spatial frames" / "无可分析的空间帧";
    box_plot => "Box" / "箱线图";
    violin_plot => "Violin" / "小提琴图";
    value => "Value" / "数值";
    style => "Style" / "样式";
    x_axis => "X Axis" / "X 轴";
    y_axis => "Y Axis" / "Y 轴";
    add_filter => "Add filter" / "添加筛选";
    remove_filter => "Remove filter" / "移除筛选";
    choose_columns => "Choose columns" / "选择列";
    export_csv => "Export CSV" / "导出 CSV";
    export_plot => "Export plot (SVG/PDF)" / "导出图表 (SVG/PDF)";
    show_all_columns => "Show All Columns" / "显示全部列";
    hide_filtered_rows => "Hide Filtered Rows" / "隐藏已筛选行";
    extract_filtered => "Extract Filtered" / "提取筛选结果";
    delete_excluded => "Delete Excluded" / "删除被排除项";
    filter => "Filter" / "筛选";
    clear_gate => "Clear Gate" / "清除门控";
    no_measurements => "No Measurements Yet" / "暂无测量";
    no_measurements_detail => "Add and detect an object set to build a measurement table, then filter, sort and export it." / "添加并检测对象集以生成测量表，然后进行筛选、排序和导出。";
    no_set_selected => "No Object Set Selected" / "未选择对象集";
    no_set_selected_detail => "Choose an object set to inspect its measurements." / "选择对象集以查看其测量。";
    measuring => "Measuring" / "测量中";
    measuring_detail => "The measurements are computed in the background and appear here when they settle." / "测量在后台计算，完成后会显示在此处。";
    no_detection => "No Detection Yet" / "尚未检测";
    no_detection_detail => "Detect this object set to build its measurement table." / "检测此对象集以生成其测量表。";
    no_rows => "No Rows" / "无数据行";
    no_rows_track_detail => "Track the objects to summarise their trajectories." / "追踪对象以汇总其轨迹。";
    no_rows_detail => "Detect objects in the region to build a measurement table." / "检测区域内的对象以生成测量表。";
    no_visible_columns => "No Visible Columns" / "无可见列";
    no_visible_columns_detail => "Choose at least one column from the columns menu." / "请从列菜单中至少选择一列。";
    no_objects => "No objects to plot" / "无可绘制对象";
    no_frames_trend => "No frames to trend" / "无可绘制趋势的帧";
    no_groups_compare => "No groups to compare" / "无可比较的组";
    no_gated_samples => "no gated samples" / "无已门控样本";
    no_samples => "no samples" / "无样本";
    ungated => "Ungated" / "未门控";
    scatter_hint => "Drag: Lasso · Shift-Drag: Box · Click: Locate" / "拖动：套索 · Shift+拖动：框选 · 单击：定位";
    gate_hint => "Drag: Gate · Click: Locate" / "拖动：门控 · 单击：定位";
    y_objects => "Y: Objects" / "Y: 对象数";
    x_groups => "X: Groups" / "X: 组";
    x_frame => "X: Frame" / "X: 帧";
    x_radius => "X: r (µm)" / "X: r (µm)";
    y_ripley => "Y: L − r (µm)" / "Y: L − r (µm)";
    x_conditions => "X: Conditions" / "X: 条件";
    x_frames => "X: Frames" / "X: 帧";
    aggregate_mean => "Mean" / "均值";
    aggregate_sum => "Sum" / "总和";
    aggregate_minimum => "Min" / "最小值";
    aggregate_maximum => "Max" / "最大值";
    aggregate_deviation => "SD (σ)" / "标准差 (σ)";
    aggregate_median => "Median" / "中位数";
    all_objects => "All objects" / "所有对象";
    all_tracks => "All tracks" / "所有轨迹";
}

pub fn distribution_group(language: Language, label: &str) -> String {
    match label {
        "All objects" => all_objects(language).to_string(),
        "All tracks" => all_tracks(language).to_string(),
        _ => match label.strip_prefix("Frame ") {
            Some(number) => match number.parse::<u32>() {
                Ok(number) => frame(language, number),
                Err(_) => label.to_string(),
            },
            None => label.to_string(),
        },
    }
}

messages! {
    rows(shown: usize, total: usize) => "{shown}/{total} rows" / "{shown}/{total} 行";
    rows_roi(shown: usize, total: usize) => "{shown}/{total} rows · ROI" / "{shown}/{total} 行 · ROI";
    objects(total: u64) => "{total} objects" / "{total} 个对象";
    gated_objects(gated: u64, total: u64) => "{gated} of {total} objects gated" / "{gated}/{total} 个对象已门控";
    groups(count: usize) => "{count} groups" / "{count} 个组";
    values_over_frames(count: u64, frames: u32) => "{count} values over {frames} frames" / "{frames} 帧中的 {count} 个数值";
    gated_values_over_frames(gated: u64, total: u64, frames: u32) => "{gated} of {total} values over {frames} frames" / "{frames} 帧的 {total} 个数值中有 {gated} 个";
    gated_count(gated: u64, total: u64) => "{gated} of {total} gated" / "{gated}/{total} 已门控";
    frame(number: u32) => "Frame {number}" / "第 {number} 帧";
    median(value: &str, unit: &str) => "Median {value} {unit}" / "中位数 {value} {unit}";
    mean(value: &str, unit: &str) => "Mean {value} {unit}" / "均值 {value} {unit}";
    bin_readout(low: &str, high: &str, unit: &str, gated: u64, total: u64) => "{low} – {high} {unit} · {gated} of {total}" / "{low} – {high} {unit} · {gated}/{total}";
    located_frame(frame: u32) => " · Frame {frame}" / " · 第 {frame} 帧";
    located_object(frame: u32, object: usize) => " · Frame {frame} · Object {object}" / " · 第 {frame} 帧 · 对象 {object}";
    located_track(track: usize) => " · Track {track}" / " · 轨迹 {track}";
    histogram_title(column: &str) => "Histogram of {column}" / "{column} 直方图";
    scatter_title(vertical: &str, horizontal: &str) => "{vertical} against {horizontal}" / "{vertical} 对 {horizontal}";
    distribution_title(column: &str) => "Distribution of {column}" / "{column} 分布";
    trend_title(column: &str) => "Trend of {column}" / "{column} 趋势";
    ripley_summary(frames: u32, objects: u64) => "{frames} frames · {objects} objects · L − r against the radius" / "{frames} 帧 · {objects} 个对象 · L − r 随半径变化";
    ripley_envelope(simulations: u32) => "CSR envelope · {simulations} simulations" / "CSR 包络 · {simulations} 次模拟";
    radial_readout(radius: &str, value: &str, low: &str, high: &str, frames: u64) => "r {radius} µm · L − r {value} µm · 95% CI {low}–{high} · {frames} frames" / "r {radius} µm · L − r {value} µm · 95% CI {low}–{high} · {frames} 帧";
    comparison_title(metric: &str) => "Comparison of {metric}" / "{metric}比较";
    comparison_summary(count: usize) => "{count} categories · images are the samples" / "{count} 个类别 · 图像为样本";
    comparison_frames_summary(count: usize, frames: u32) => "{count} series over {frames} frames · band is the 95% CI across images" / "{count} 条曲线，跨越 {frames} 帧 · 波带为跨图像的 95% CI";
}
