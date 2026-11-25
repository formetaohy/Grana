use crate::{Language, labels, messages};

pub fn term(language: Language, english: &str) -> Option<String> {
    let translated = match english {
        "frame" => "帧",
        "time" => "时间",
        "object" => "对象",
        "objects" => "对象数",
        "track" => "轨迹",
        "molecule" => "分子",
        "localizations" => "定位数",
        "spread x" => "离散 X",
        "spread y" => "离散 Y",
        "spread z" => "离散 Z",
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
    ripley_outside => "outside" / "越界";
    no_spatial => "No spatial frames" / "无可分析的空间帧";
    box_plot => "Box" / "箱线图";
    violin_plot => "Violin" / "小提琴图";
    value => "Value" / "数值";
    style => "Style" / "样式";
    x_axis => "X" / "X";
    y_axis => "Y" / "Y";
    add_filter => "Add filter" / "添加筛选";
    remove_filter => "Remove filter" / "移除筛选";
    choose_columns => "Choose columns" / "选择列";
    export_csv => "Export CSV" / "导出 CSV";
    export_plot => "Export plot (SVG/PDF)" / "导出图表 (SVG/PDF)";
    show_all_columns => "Show All Columns" / "显示全部列";
    hide_filtered_rows => "Hide Filtered" / "隐藏已筛选";
    extract_filtered => "Extract Filtered" / "提取筛选结果";
    delete_excluded => "Delete Excluded" / "删除被排除项";
    filter => "Filter" / "筛选";
    clear_gate => "Clear Gate" / "清除门控";
    no_measurements => "No Measurements" / "暂无测量";
    no_set_selected => "No Object Set Selected" / "未选择对象集";
    measuring => "Measuring…" / "测量中…";
    no_detection => "No Detection" / "尚未检测";
    no_rows => "No Rows" / "无数据行";
    no_visible_columns => "No Visible Columns" / "无可见列";
    no_objects => "No objects" / "无对象";
    no_frames_trend => "No frames" / "无帧数据";
    no_groups_compare => "No groups to compare" / "无可比较的组";
    no_gated_samples => "no gated samples" / "无已门控样本";
    no_samples => "no samples" / "无样本";
    ungated => "Ungated" / "未门控";
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

pub fn column(language: Language, label: &str) -> String {
    if let Some(translated) = term(language, label) {
        return translated;
    }
    if let Some((head, tail)) = label.rsplit_once(" · ") {
        let head = phrase(language, head).unwrap_or_else(|| head.to_string());
        let tail = term(language, tail).unwrap_or_else(|| tail.to_string());
        return format!("{head} · {tail}");
    }
    phrase(language, label).unwrap_or_else(|| label.to_string())
}

fn phrase(language: Language, label: &str) -> Option<String> {
    if let Some(translated) = term(language, label) {
        return Some(translated);
    }
    if let Some(rest) = label.strip_prefix("classifier ") {
        let name = term(language, "classifier")?;
        if let Some(aggregate) = term(language, rest) {
            return Some(format!("{name} {aggregate}"));
        }
    }
    if let Some(rest) = label.strip_prefix("ellipsoid axis ")
        && let Ok(axis) = rest.parse::<usize>()
    {
        return Some(match language {
            Language::English => label.to_string(),
            Language::Chinese => format!("椭球轴 {axis}"),
        });
    }
    if let Some((target, field)) = label.rsplit_once(' ')
        && let Some(field) = term(language, field)
    {
        return Some(format!("{target} {field}"));
    }
    None
}

messages! {
    rows(shown: usize, total: usize) => "{shown}/{total}" / "{shown}/{total}";
    rows_roi(shown: usize, total: usize) => "{shown}/{total} · ROI" / "{shown}/{total} · ROI";
    objects(total: u64) => "{total} objects" / "{total} 个对象";
    gated_objects(gated: u64, total: u64) => "{gated}/{total} objects" / "{gated}/{total} 个对象";
    groups(count: usize) => "{count} groups" / "{count} 个组";
    values_over_frames(count: u64, frames: u32) => "{count} · {frames}f" / "{count} · {frames} 帧";
    gated_values_over_frames(gated: u64, total: u64, frames: u32) => "{gated}/{total} · {frames}f" / "{gated}/{total} · {frames} 帧";
    gated_count(gated: u64, total: u64) => "{gated}/{total}" / "{gated}/{total}";
    frame(number: u32) => "Frame {number}" / "第 {number} 帧";
    median(value: &str, unit: &str) => "Median {value} {unit}" / "中位数 {value} {unit}";
    mean(value: &str, unit: &str) => "Mean {value} {unit}" / "均值 {value} {unit}";
    bin_readout(low: &str, high: &str, unit: &str, gated: u64, total: u64) => "{low} – {high} {unit} · {gated}/{total}" / "{low} – {high} {unit} · {gated}/{total}";
    located_frame(frame: u32) => " · F{frame}" / " · F{frame}";
    located_object(frame: u32, object: usize) => " · F{frame} · #{object}" / " · F{frame} · #{object}";
    located_track(track: usize) => " · T{track}" / " · T{track}";
    histogram_title(column: &str) => "Histogram of {column}" / "{column} 直方图";
    scatter_title(vertical: &str, horizontal: &str) => "{vertical} against {horizontal}" / "{vertical} 对 {horizontal}";
    distribution_title(column: &str) => "Distribution of {column}" / "{column} 分布";
    trend_title(column: &str) => "Trend of {column}" / "{column} 趋势";
    ripley_summary(frames: u32, objects: u64) => "{frames}f · {objects} objects" / "{frames} 帧 · {objects} 个对象";
    ripley_single(single: u32) => "{single} single-object frames" / "{single} 帧仅一个对象";
    ripley_envelope(simulations: u32) => "CSR 95% · {simulations}" / "CSR 95% · {simulations}";
    ripley_test(p: &str, beyond: u32) => "p {p} · {beyond}" / "p {p} · {beyond}";
    radial_readout(radius: &str, value: &str, low: &str, high: &str, frames: u64) => "r {radius} µm · L − r {value} µm · CI {low}–{high} · {frames}f" / "r {radius} µm · L − r {value} µm · CI {low}–{high} · {frames} 帧";
    comparison_title(metric: &str) => "Comparison of {metric}" / "{metric}比较";
    comparison_summary(count: usize) => "{count} categories" / "{count} 个类别";
    comparison_frames_summary(count: usize, frames: u32) => "{count} series · {frames}f · 95% CI" / "{count} 条曲线 · {frames} 帧 · 95% CI";
    trend_interaction(faster: &str, slower: &str, q: &str) => "{faster} > {slower} · q {q}" / "{faster} > {slower} · q {q}";
    trend_interaction_none(q: &str) => "min q {q}" / "最小 q {q}";
}
