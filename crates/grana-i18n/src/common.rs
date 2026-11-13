use crate::Language;
use crate::labels;

labels! {
    cancel => "Cancel" / "取消";
    close => "Close" / "关闭";
    done => "Done" / "完成";
    clear => "Clear" / "清除";
    remove => "Remove" / "移除";
    save => "Save" / "保存";
    open => "Open" / "打开";
    apply => "Apply" / "应用";
    reset => "Reset" / "重置";
    none => "None" / "无";
    all => "All" / "全部";
    current => "Current" / "当前";
    minimum => "Minimum" / "最小值";
    maximum => "Maximum" / "最大值";
    frame => "Frame" / "帧";
    frames => "Frames" / "帧";
    time => "Time" / "时间";
    track => "Track" / "轨迹";
    tracks => "Tracks" / "轨迹";
    object => "Object" / "对象";
    objects => "Objects" / "对象";
    every_frame => "Every frame" / "每一帧";
    single_frame => "Single frame" / "单帧";
    whole_volume => "Whole volume" / "整个体数据";
    whole_volume_title => "Whole Volume" / "整个体数据";
    untitled => "Untitled" / "未命名";
    blocks => "blocks" / "块";
    tiles => "tiles" / "瓦片";
    beads => "beads" / "微球";
    steps => "steps" / "步";
    preparing => "Preparing" / "准备中";
}

use crate::messages;

messages! {
    frame_prefix(time: u32) => "Frame {time} · " / "第 {time} 帧 · ";
    progress(done: usize, total: usize, unit: &str) => "{done}/{total} {unit}" / "{done}/{total} {unit}";
    saving(name: &str) => "Saving {name}" / "正在保存 {name}";
    exporting_statistics(name: &str) => "Exporting {name} statistics" / "正在导出 {name} 统计";
}

labels! {
    exporting_labels => "Exporting labels" / "正在导出标签";
    plate_montage => "Plate montage" / "孔板拼图";
    exporting_dataset => "Exporting the dataset" / "正在导出数据集";
    exporting_provenance => "Exporting the provenance crate" / "正在导出溯源包";
    exporting_package => "Exporting the share package" / "正在导出分享包";
}

pub fn task(language: Language, label: &str) -> String {
    if language == Language::English {
        return label.to_string();
    }
    if let Some(name) = label.strip_prefix("Saving ") {
        return saving(language, name);
    }
    if let Some(name) = label.strip_prefix("Opening ") {
        return crate::shell::opening(language, name);
    }
    if let Some(name) = label.strip_prefix("Exporting ")
        && let Some(name) = name.strip_suffix(" statistics")
    {
        return exporting_statistics(language, name);
    }
    match label {
        "Exporting labels" => exporting_labels(language).to_string(),
        "Plate montage" => plate_montage(language).to_string(),
        "Exporting the dataset" => exporting_dataset(language).to_string(),
        "Exporting the provenance crate" => exporting_provenance(language).to_string(),
        "Exporting the share package" => exporting_package(language).to_string(),
        _ => label.to_string(),
    }
}
