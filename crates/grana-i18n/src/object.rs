use crate::Language;
use crate::labels;
use crate::messages;

labels! {
    no_sets => "No Object Sets" / "暂无对象集";
    object_sets => "OBJECT SETS" / "对象集";
    import_title => "IMPORT" / "导入";
    import => "Import" / "导入";
    spots => "Spots" / "斑点";
    surfaces => "Surfaces" / "表面";
    filaments => "Filaments" / "丝状结构";
    cells => "Cells" / "细胞";
    spots_noun => "spots" / "斑点";
    objects_noun => "objects" / "对象";
    filaments_noun => "filaments" / "丝状结构";
    cells_noun => "cells" / "细胞";
    segments => "Segments" / "分割";
    import_labels => "Import Labels" / "导入标签";
    import_tracks => "Import Tracks" / "导入轨迹";
    import_localizations => "Import Localizations" / "导入定位表";
    import_geff => "GEFF Graph" / "GEFF 图谱";
    clear_objects => "Clear Objects" / "清空对象";
    hide_set => "Hide object set" / "隐藏对象集";
    show_set => "Show object set" / "显示对象集";
    object_commands => "Object commands" / "对象命令";
    rename => "Rename" / "重命名";
    duplicate => "Duplicate" / "复制";
    mask_channel => "Mask Channel" / "生成掩膜通道";
    colocalize_objects => "Colocalize Objects" / "对象共定位";
    cancel_object_coloc => "Cancel Object Colocalization" / "取消对象共定位";
    clear_object_coloc => "Clear Object Colocalization" / "清除对象共定位";
    paint_mask => "Paint Mask" / "绘制掩膜";
    clear_mask_edits => "Clear Mask Edits" / "清除掩膜编辑";
    export_mesh_obj => "Export Mesh OBJ…" / "导出网格 OBJ…";
    export_mesh_stl => "Export Mesh STL…" / "导出网格 STL…";
    export_mesh_glb => "Export Mesh GLB…" / "导出网格 GLB…";
    export_labels => "Export Labels…" / "导出标签…";
    export_geff => "Export GEFF…" / "导出 GEFF…";
    export_localizations => "Export Localizations…" / "导出定位表…";
    correct_localization_drift => "Correct Drift from Localizations" / "按定位表校正漂移";
    merge_into => "Merge Into" / "合并到";
    move_up => "Move Up" / "上移";
    move_down => "Move Down" / "下移";
    delete_set => "Delete Object Set" / "删除对象集";
    edit_objects => "Edit Objects" / "编辑对象";
    split_object => "Split Object" / "分裂对象";
    opacity => "Opacity" / "不透明度";
    roughness => "Roughness" / "粗糙度";
    metallic => "Metallic" / "金属度";
    object_labels => "Object Labels" / "对象标签";
    label_by => "Label by" / "标签依据";
    show_all => "Show All" / "全部显示";
    hide_selected => "Hide Selected" / "隐藏选中";
    target => "Target" / "目标";
    classifier_score => "Classifier score" / "分类器分数";
    label_color => "Object label" / "对象标签";
    coloring => "Coloring" / "着色";
    classifier => "Classifier" / "分类器";
    train_classifier => "Train Classifier" / "训练分类器";
    retrain_classifier => "Retrain Classifier" / "重新训练分类器";
    clear_classifier => "Clear Classifier" / "清除分类器";
    object_set => "Object Set" / "对象集";
    origin => "Origin" / "来源";
    curated => "Curated" / "已整理";
    detect => "Detect" / "检测";
    manual_seeds => "Manual Seeds" / "手动种子点";
    seeds => "Seeds" / "种子点";
    active_seeds => "Active seeds" / "生效的种子点";
    seed_source => "seed source" / "种子来源";
    place_seeds => "Place Seeds" / "放置种子点";
    stop_placing => "Stop Placing" / "停止放置";
    manual => "Manual" / "手动";
    clear_seeds => "Clear seeds" / "清除种子点";
    child => "Child" / "子对象";
    number => "Number" / "编号";
    children => "Children" / "子对象";
    selected_object => "Selected Object" / "选中对象";
    delete_object => "Delete Object" / "删除对象";
    count => "count" / "数量";
    overlap => "Overlap" / "重叠度";
    relations => "Relations" / "关系";
    open_pair_matrix => "Open pair matrix" / "打开配对矩阵";
    remove_relation => "Remove relation" / "移除关系";
    earlier_frame => "Earlier frame" / "前一帧";
    later_frame => "Later frame" / "后一帧";
    add_offset_relation => "Add offset relation" / "添加偏移关系";
    shift => "Shift" / "偏移";
    same_frame => "Same frame" / "同一帧";
    relate_to_set => "Relate to Object Set…" / "关联到对象集…";
}

messages! {
    unread(count: usize) => "{count}" / "{count}";
    more_notes(count: usize) => "{count} more" / "还有 {count} 条";
    labels_written(path: &str, objects: u32, frames: u32) => "{path} · {objects} ({frames}f)" / "{path} · {objects}（{frames} 帧）";
    header_frames(channel: &str, count: usize, noun: &str, detected: usize) => "{channel} · {count} {noun} · {detected}f" / "{channel} · {count} 个{noun} · {detected} 帧";
    header_single(channel: &str, count: usize, noun: &str) => "{channel} · {count} {noun}" / "{channel} · {count} 个{noun}";
    header_empty(channel: &str, frame: u32) => "{channel} · 0 on F{frame}" / "{channel} · F{frame} 无对象";
    ready_import(channel: &str) => "{channel} · Import" / "{channel} · 导入";
    ready_detect(channel: &str) => "{channel}" / "{channel}";
    hidden_suffix(count: usize) => " · {count} hidden" / " · {count} 隐藏";
    hidden_count(count: usize) => "{count} hidden" / "{count} 隐藏";
    classifier_fit(measurements: usize, kept: usize, excluded: usize, percent: f32) => "{measurements}M · +{kept}/−{excluded} · {percent:.0}%" / "{measurements} 项 · +{kept}/−{excluded} · {percent:.0}%";
    measurements(count: usize) => "{count} Measurements" / "{count} 个测量项";
    gate_rows(kept: usize, total: usize) => "{kept} / {total} gated" / "已门控 {kept} / {total}";
    imported_from(label: &str) => "{label}" / "{label}";
    attached_from(file: &str) => "{file}" / "{file}";
    filtered_from(name: &str) => "{name}" / "{name}";
    copy_of(name: &str) => "{name}" / "{name}";
    edited_from(name: &str, edits: usize) => "{name} · {edits} edits" / "{name} · {edits} 次编辑";
    painted_from(name: &str, strokes: usize) => "{name} · {strokes} strokes" / "{name} · {strokes} 笔";
    merged_from(names: &str) => "{names}" / "{names}";
    import_statistics(frames: usize, objects: usize) => "{frames}f · {objects} objects" / "{frames} 帧 · {objects} 个对象";
    import_sampled(frames: usize, objects: usize) => "{frames}f · {objects} objects" / "{frames} 帧 · {objects} 个对象";
    removed(count: usize) => "{count} removed" / "{count} 移除";
    track_edit(count: usize) => "{count} edit" / "{count} 编辑";
    track_edits(count: usize) => "{count} edits" / "{count} 编辑";
    pending(count: usize) => "{count} pending" / "{count} 待处理";
    dropped(count: usize) => "{count} dropped" / "{count} 丢弃";
    detect_root(root: &str) => "Detect {root}" / "检测 {root}";
    from_source(name: &str) => "From {name}" / "来自 {name}";
    active_seeds_detail(count: usize, name: &str) => "{count} · {name}" / "{count} · {name}";
    placed(count: usize) => "{count}" / "{count}";
    summary_count(count: usize, noun: &str, level: usize) => "{count} {noun} · L{level}" / "{count} 个{noun} · L{level}";
    summary_contrast(term: &str, mean: f32) => " · {term} {mean:.2}" / " · {term} {mean:.2}";
    no_objects_frame(time: u32, frames: u32) => "0 · F{time}/{frames}" / "0 · F{time}/{frames}";
    child_inside(name: &str, count: usize) => "{name} · {count}" / "{name} · {count}";
    nearest(name: &str) => "{name} · Nearest" / "{name} · 最近邻";
    target_shift(offset: &str) => "{offset}f" / "{offset}f";
    merge_selected(count: usize) => "Merge {count} Selected" / "合并选中的 {count} 个";
    merge_into_object(number: usize) => "Merge Into #{number}" / "合并到 #{number}";
}

pub fn property(language: Language, english: &str) -> String {
    if language == Language::English {
        return english.to_string();
    }
    if let Some(translated) = crate::measure::term(language, &english.to_lowercase()) {
        return translated;
    }
    let chinese = match english {
        "Object" => "对象",
        "Position" => "位置",
        "Hull volume" => "凸包体积",
        "Ellipsoid axes" => "椭球轴",
        "Centroid" => "质心",
        "Extent" => "范围",
        "Nucleus volume" => "细胞核体积",
        "Nucleus area" => "细胞核面积",
        "Nucleus sphericity" => "细胞核球形度",
        "Cytoplasm volume" => "细胞质体积",
        "Nuclear ratio" => "核质比",
        _ => english,
    };
    chinese.to_string()
}

pub fn parameter(language: Language, english: &str) -> String {
    if language == Language::English {
        return english.to_string();
    }
    let chinese = match english {
        "diameter" => "直径",
        "contrast" => "对比度",
        "threshold" => "阈值",
        "grain" => "粒度",
        "splitting" => "分裂度",
        "min volume" => "最小体积",
        "opening radius" => "开运算半径",
        "closing radius" => "闭运算半径",
        "min sphericity" => "最小球形度",
        "min mean intensity" => "最小平均强度",
        "min length" => "最小长度",
        "reach" => "延伸范围",
        "foreground" => "前景",
        "density" => "密度",
        "minimum size" => "最小尺寸",
        "minimum volume" => "最小体积",
        _ => english,
    };
    chinese.to_string()
}
