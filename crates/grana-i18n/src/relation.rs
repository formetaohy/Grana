use crate::labels;
use crate::messages;

labels! {
    no_relations_title => "No Relations" / "暂无关系";
    object_set => "Object Set" / "对象集";
    metric => "Metric" / "指标";
    filter => "Filter" / "筛选";
    export_pairs => "Export CSV" / "导出 CSV";
    pair_matrix => "Pair Matrix" / "配对矩阵";
    no_pair_filtered => "No matching pairs" / "无匹配配对";
    no_pairs_frame => "No pairs" / "无配对";
    pairs => "Pairs" / "配对";
    no_pairs_list => "No pairs" / "无配对";
    source_target => "src / tgt" / "源/目标";
}

messages! {
    measuring_frame(done: usize, total: usize) => "{done}/{total} frames" / "{done}/{total} 帧";
    frame_not_measured(done: usize, total: usize, frame: u32) => "{done}/{total}f · F{frame}" / "{done}/{total} 帧 · F{frame}";
    matrix_scope(sources: usize, targets: usize) => "{sources} × {targets}" / "{sources} × {targets}";
    strongest(shown: usize, total: usize) => "{shown}/{total}" / "{shown}/{total}";
    pair_count(count: usize) => "{count}" / "{count}";
    uniform_strength(low: &str) => "{low}" / "{low}";
    near_far(low: &str, high: &str) => "{low} – {high}" / "{low} – {high}";
    low_high(low: &str, high: &str) => "{low} – {high}" / "{low} – {high}";
}
