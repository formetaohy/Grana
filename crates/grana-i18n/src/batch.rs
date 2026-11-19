use crate::Language;
use crate::labels;
use crate::messages;

labels! {
    capture => "Capture" / "捕获";
    load => "Load" / "加载";
    provenance => "Provenance" / "溯源";
    copy => "Copy" / "复制";
    protocol => "Protocol" / "协议";
    ready => "Ready" / "就绪";
    datasets => "Datasets" / "数据集";
    add_folder => "Add Folder…" / "添加文件夹…";
    choose_folder => "Choose Folder…" / "选择文件夹…";
    open_run => "Open Run" / "打开运行记录";
    workers => "Workers" / "工作线程";
    save_project => "Save Project per Dataset" / "为每个数据集保存项目";
    export_channels => "Export Derived Channels" / "导出派生通道";
    paused => "Paused" / "已暂停";
    running_protocol => "Running…" / "运行中…";
    resume => "Resume" / "继续";
    pause => "Pause" / "暂停";
    run_batch => "Run Batch" / "运行批处理";
    comparison => "Comparison" / "比较";
    conditions => "Conditions" / "条件";
    frames => "Frames" / "帧";
    group => "Group" / "组";
    set => "Set" / "对象集";
    all_groups => "All groups" / "所有组";
    all_sets => "All sets" / "所有对象集";
    no_group_test => "no group test" / "无分组检验";
    complete => "Complete" / "完成";
    resumed => "Resumed" / "已继续";
    corrected => "Corrected" / "已修正";
    failed => "Failed" / "失败";
    cancelled => "Cancelled" / "已取消";
    running_state => "Running" / "运行中";
    pending => "Pending" / "待处理";
    queued => "Queued" / "排队中";
    export_comparison => "Export comparison (SVG/PDF)" / "导出比较 (SVG/PDF)";
    output_execution => "Output & Execution" / "输出与执行";
    record_correction => "Record Correction" / "记录修正";
    clear_correction => "Clear Correction" / "清除修正";
    filter_empty => "No matching group" / "无匹配分组";
    dataset_fallback => "dataset" / "数据集";
    no_samples => "No samples" / "无样本";
}

messages! {
    reviewing(name: &str) => "{name}" / "{name}";
    significant(significant: usize, tests: usize) => "{significant}/{tests}" / "{significant}/{tests}";
    concurrent(jobs: usize) => "{jobs}" / "{jobs}";
    value_deviation(title: &str, mean: &str, deviation: &str) => "{title} · {mean} ± {deviation}" / "{title} · {mean} ± {deviation}";
    value_mean(title: &str, mean: &str) => "{title} · {mean}" / "{title} · {mean}";
    value_missing(title: &str) => "{title} · —" / "{title} · —";
    objects_in_images(objects: usize, images: usize) => " · {objects} ({images})" / " · {objects}（{images}）";
    q_value(q: f64, mark: &str) => " · q {q:.3} {mark}" / " · q {q:.3} {mark}";
    sample_value(dataset: &str, value: &str, objects: usize) => "{dataset}: {value} ({objects})" / "{dataset}: {value} ({objects})";
    sample_missing(dataset: &str) => "{dataset}: —" / "{dataset}: —";
    more_samples(count: usize) => "+{count}" / "+{count}";
    test_groups(group: &str, other: &str, samples: usize, images: usize, p: &str, q: &str) => "{group} vs {other} · n={samples}/{images} · p={p} q={q}" / "{group} vs {other} · n={samples}/{images} · p={p} q={q}";
    test_rank(rank_p: &str, rank_q: &str, effect: &str, delta: f64, interval: &str, mark: &str) => "rank p={rank_p} q={rank_q} · g={effect} Δ={delta:.2} · CI {interval} {mark}" / "秩 p={rank_p} q={rank_q} · g={effect} Δ={delta:.2} · CI {interval} {mark}";
}

pub fn item_count(language: Language, count: usize) -> String {
    let _ = language;
    format!("{count}")
}
