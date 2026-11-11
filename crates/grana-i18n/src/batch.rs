use crate::labels;

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
    paused => "Paused between analysis steps" / "在分析步骤之间暂停";
    running_protocol => "Running protocol across datasets…" / "正在对多个数据集运行协议…";
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
    capture_hint => "Capture the current region, objects and settings into a reusable protocol." / "将当前区域、对象与设置捕获为可复用协议。";
    datasets_hint => "Choose a folder of datasets that share this protocol." / "选择共享此协议的数据集文件夹。";
    output_hint => "Choose a destination folder for the exported tables." / "为导出的表格选择目标文件夹。";
    correction_hint => "The next run applies the correction to this dataset only." / "下次运行仅将修正应用于此数据集。";
    resume_hint => "Re-running the same protocol resumes where it stopped." / "重新运行同一协议时将从停止处继续。";
    comparison_hint => "Run the batch, or open a run record, to compare the reported groups." / "运行批处理或打开运行记录，以比较所报告的组。";
    comparison_note => "Objects are the samples and images are the clusters; the Welch test corrects for the clustering and the rank test compares image means. q is the Benjamini–Hochberg correction over the run. Click a condition for its numbers." / "对象是样本，图像是聚类；Welch 检验对聚类做了校正，秩检验比较各图像均值。q 是本次运行上的 Benjamini–Hochberg 校正。点击某个条件可查看其数值。";
    filter_empty => "No reported group matches this filter." / "没有符合此筛选的报告组。";
    dataset_fallback => "the dataset" / "该数据集";
    no_samples => "The run recorded no dataset samples." / "该运行未记录数据集样本。";
    metric_objects => "Objects" / "对象数";
    metric_volume => "Volume" / "体积";
    metric_area => "Area" / "面积";
    metric_size => "Size" / "尺寸";
    metric_intensity => "Intensity" / "强度";
}

use crate::Language;
use crate::messages;

messages! {
    reviewing(name: &str) => "Reviewing {name}. Edit its objects or parameters, then record the correction." / "正在审阅 {name}。编辑其对象或参数，然后记录修正。";
    significant(significant: usize, tests: usize) => "{significant}/{tests} significant" / "显著 {significant}/{tests}";
    concurrent(jobs: usize) => "{jobs} concurrent" / "并发 {jobs}";
    value_deviation(title: &str, mean: &str, deviation: &str) => "{title} · {mean} ± {deviation}" / "{title} · {mean} ± {deviation}";
    value_mean(title: &str, mean: &str) => "{title} · {mean}" / "{title} · {mean}";
    value_missing(title: &str) => "{title} · no value" / "{title} · 无值";
    objects_in_images(objects: usize, images: usize) => " · {objects} objects in {images} images" / " · {objects} 个对象，位于 {images} 张图像";
    q_value(q: f64, mark: &str) => " · q {q:.3} {mark}" / " · q {q:.3} {mark}";
    sample_value(dataset: &str, value: &str, objects: usize) => "{dataset}: {value} ({objects} objects)" / "{dataset}: {value} ({objects} 个对象)";
    sample_missing(dataset: &str) => "{dataset}: no value" / "{dataset}: 无值";
    more_samples(count: usize) => "{count} more" / "另有 {count} 项";
    test_groups(group: &str, other: &str, samples: usize, images: usize, p: &str, q: &str) => "{group} vs {other} · {samples} samples in {images} images · Welch p={p} q={q}" / "{group} 对比 {other} · {samples} 个样本，位于 {images} 张图像 · Welch p={p} q={q}";
    test_rank(rank_p: &str, rank_q: &str, effect: &str, delta: f64, interval: &str, mark: &str) => "image rank p={rank_p} q={rank_q} · g={effect} Δ={delta:.2} · CI {interval} {mark}" / "图像秩 p={rank_p} q={rank_q} · g={effect} Δ={delta:.2} · CI {interval} {mark}";
}

pub fn item_count(language: Language, count: usize) -> String {
    match language {
        Language::English => format!("{count} item{}", if count == 1 { "" } else { "s" }),
        Language::Chinese => format!("{count} 项"),
    }
}
