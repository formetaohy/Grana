use crate::labels;

labels! {
    no_relations_title => "No Relations Yet" / "暂无关系";
    no_relations_detail => "Add a relation between two object sets in the Objects sidebar to compare their pairs here." / "在“对象”侧边栏为两个对象集添加关系，即可在此比较它们的配对。";
    object_set => "Object Set" / "对象集";
    metric => "Metric" / "指标";
    filter => "Filter" / "筛选";
    export_pairs => "Export Pairs…" / "导出配对…";
    no_frame_measured => "This relation measures no frame yet." / "该关系尚未测量任何帧。";
    exports_every_pair => "Exports every measured pair as CSV." / "将所有已测量的配对导出为 CSV。";
    exports_measured_pairs => "Exports the pairs measured so far as CSV." / "将目前测得的配对导出为 CSV。";
    pair_matrix => "Pair Matrix" / "配对矩阵";
    no_pair_filtered => "No pair passes the filter at this frame." / "当前帧没有配对通过筛选。";
    no_pairs_frame => "This relation holds no pairs at this frame." / "该关系在当前帧没有配对。";
    pairs => "Pairs" / "配对";
    no_pairs_list => "No pairs to list." / "没有可列出的配对。";
    source_target => "src / tgt" / "源/目标";
}

use crate::messages;

messages! {
    measuring_frame(done: usize, total: usize) => "Measuring frame {done} of {total}." / "正在测量第 {done} 帧，共 {total} 帧。";
    frame_not_measured(done: usize, total: usize, frame: u32) => "Measured on {done} of {total} frames. This relation holds no measurement for frame {frame} yet." / "已测量 {done}/{total} 帧。该关系尚无第 {frame} 帧的测量结果。";
    matrix_scope(sources: usize, targets: usize) => "{sources} sources × {targets} targets" / "{sources} 个源 × {targets} 个目标";
    strongest(shown: usize, total: usize) => "{shown} strongest of {total}" / "最强 {shown} 对，共 {total} 对";
    pair_count(count: usize) => "{count} pairs" / "{count} 对";
    uniform_strength(low: &str) => "{low} for every pair" / "{low}（所有配对相同）";
    near_far(low: &str, high: &str) => "{low} near · {high} far" / "{low} 近 · {high} 远";
    low_high(low: &str, high: &str) => "{low} low · {high} high" / "{low} 低 · {high} 高";
}
