use crate::Language;
use crate::labels;
use crate::messages;

labels! {
    no_sets => "No Object Sets" / "暂无对象集";
    object_sets => "OBJECT SETS" / "对象集";
    add_object_set => "Add object set" / "添加对象集";
    import_title => "IMPORT" / "导入";
    import => "Import" / "导入";
    spots => "Spots" / "斑点";
    surfaces => "Surfaces" / "表面";
    filaments => "Filaments" / "丝状结构";
    spots_noun => "spots" / "斑点";
    objects_noun => "objects" / "对象";
    filaments_noun => "filaments" / "丝状结构";
    spots_description => "detects intensity maxima in" / "检测强度极大值";
    surfaces_description => "extracts connected surfaces of" / "提取连通表面";
    filaments_description => "traces centerlines through" / "追踪中心线";
    segments_unlicensed => "Segments · unlicensed" / "分割 · 未授权";
    import_labels => "Import Labels" / "导入标签";
    import_tracks => "Import Tracks" / "导入轨迹";
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
    export_tracks => "Export Tracks…" / "导出轨迹…";
    export_geff => "Export GEFF…" / "导出 GEFF…";
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
    coloring => "Coloring" / "着色";
    classifier => "Classifier" / "分类器";
    classifier_intro => "Trains on the objects kept by the statistics gate and scores every object of the set." / "以统计筛选保留的对象进行训练，并为该对象集的每个对象打分。";
    classifier_gate_hint => "Open Statistics for this set and add a filter or lasso to choose the kept objects." / "打开该对象集的统计，并添加筛选或套索以选择保留的对象。";
    train_classifier => "Train Classifier" / "训练分类器";
    retrain_classifier => "Retrain Classifier" / "重新训练分类器";
    clear_classifier => "Clear Classifier" / "清除分类器";
    object_set => "Object Set" / "对象集";
    origin => "Origin" / "来源";
    curated => "Curated" / "已整理";
    model => "Model" / "模型";
    detect => "Detect" / "检测";
    manual_seeds => "Manual Seeds" / "手动种子点";
    seeds => "Seeds" / "种子点";
    active_seeds => "Active seeds" / "生效的种子点";
    seed_source => "seed source" / "种子来源";
    place_seeds => "Place Seeds" / "放置种子点";
    stop_placing => "Stop Placing" / "停止放置";
    manual => "Manual" / "手动";
    clear_seeds => "Clear seeds" / "清除种子点";
    seed_hint => "Click an object or the plane to place a seed · Alt-click to remove." / "点击对象或平面放置种子点 · 按住 Alt 点击可移除。";
    no_segmentation_weights => "This build licenses no segmentation weights." / "此构建未授权任何分割权重。";
    no_model_weights => "This build licenses no weights for the model the object set was segmented with." / "此构建未授权该对象集所用模型的分割权重。";
    number => "Number" / "编号";
    child => "Child" / "子对象";
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
    no_sets_detail(channel: &str) => "Detect spots, surfaces or filaments in {channel} to measure structures." / "在 {channel} 中检测斑点、表面或丝状结构以测量结构。";
    unread(count: usize) => "{count} unread" / "{count} 条未读";
    more_notes(count: usize) => "{count} more" / "还有 {count} 条";
    segments(license: &str) => "Segments · {license}" / "分割 · {license}";
    labels_written(path: &str, objects: u32, frames: u32) => "Labels written to {path} · {objects} objects over {frames} frames." / "标签已写入 {path} · {objects} 个对象，共 {frames} 帧。";
    header_frames(channel: &str, count: usize, noun: &str, detected: usize) => "{channel} · {count} {noun} · {detected}f" / "{channel} · {count} 个{noun} · {detected} 帧";
    header_single(channel: &str, count: usize, noun: &str) => "{channel} · {count} {noun}" / "{channel} · {count} 个{noun}";
    header_empty(channel: &str, frame: u32) => "{channel} · 0 on frame {frame}" / "{channel} · 第 {frame} 帧无对象";
    ready_import(channel: &str) => "{channel} · Ready to import" / "{channel} · 可导入";
    ready_detect(channel: &str) => "{channel} · Ready to detect" / "{channel} · 可检测";
    hidden_suffix(count: usize) => " · {count} hidden" / " · {count} 个已隐藏";
    hidden_count(count: usize) => "{count} hidden" / "{count} 个已隐藏";
    classifier_fit(measurements: usize, kept: usize, excluded: usize, percent: f32) => "{measurements} measurements · {kept} kept / {excluded} excluded · {percent:.0}% balanced fit" / "{measurements} 个测量项 · 保留 {kept} / 排除 {excluded} · 平衡拟合度 {percent:.0}%";
    measurements(count: usize) => "{count} Measurements" / "{count} 个测量项";
    gate_rows(kept: usize, total: usize) => "The gate keeps {kept} of {total} rows." / "筛选保留 {kept}/{total} 行。";
    segmented(model: &str, license: &str) => "Segmented with {model} · {license} weights" / "使用 {model} 分割 · {license} 权重";
    imported_from(label: &str) => "Imported from {label}" / "从 {label} 导入";
    attached_from(file: &str) => "Attached from {file}" / "从 {file} 附加";
    filtered_from(name: &str) => "Filtered from {name}" / "从 {name} 筛选";
    copy_of(name: &str) => "Copy of {name}" / "复制自 {name}";
    edited_from(name: &str, edits: usize) => "Edited from {name} · {edits} edits" / "编辑自 {name} · {edits} 次编辑";
    painted_from(name: &str, strokes: usize) => "Painted from {name} · {strokes} strokes" / "绘制自 {name} · {strokes} 笔";
    merged_from(names: &str) => "Merged from {names}" / "合并自 {names}";
    import_statistics(frames: usize, objects: usize) => "{frames} frames · {objects} objects · intensities from the file statistics" / "{frames} 帧 · {objects} 个对象 · 强度来自文件统计";
    import_sampled(frames: usize, objects: usize) => "{frames} frames · {objects} objects · intensities sampled from the voxels inside" / "{frames} 帧 · {objects} 个对象 · 强度从内部体素采样";
    removed(count: usize) => "{count} removed" / "{count} 个已移除";
    track_edit(count: usize) => "{count} track edit" / "{count} 次轨迹编辑";
    track_edits(count: usize) => "{count} track edits" / "{count} 次轨迹编辑";
    pending(count: usize) => "{count} pending" / "{count} 个待处理";
    dropped(count: usize) => "{count} dropped" / "{count} 个已丢弃";
    model_weights(model: &str, license: &str) => "{model} · {license} weights" / "{model} · {license} 权重";
    detect_root(root: &str) => "Detect {root}" / "检测 {root}";
    from_source(name: &str) => "From {name}" / "来自 {name}";
    active_seeds_detail(count: usize, name: &str) => "{count} from {name}" / "来自 {name} 的 {count} 个";
    placed(count: usize) => "{count} placed" / "{count} 个已放置";
    summary_count(count: usize, noun: &str, level: usize) => "{count} {noun} · Level {level}" / "{count} 个{noun} · 层级 {level}";
    summary_contrast(term: &str, mean: f32) => " · {term} {mean:.2}" / " · {term} {mean:.2}";
    detect_summary(description: &str) => "{description} across all frames." / "在全部帧中{description}。";
    no_objects_frame(time: u32, frames: u32) => "No objects on frame {time} of {frames}." / "第 {time} 帧（共 {frames} 帧）没有对象。";
    child_inside(name: &str, count: usize) => "{name} · {count} inside" / "{name} · 内含 {count} 个";
    nearest(name: &str) => "{name} · Nearest Neighbour" / "{name} · 最近邻";
    target_shift(offset: &str) => "Target {offset}f" / "目标 {offset}f";
    merge_selected(count: usize) => "Merge {count} Selected Objects" / "合并选中的 {count} 个对象";
    merge_into_object(number: usize) => "Merge Into Object {number}" / "合并到对象 {number}";
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
        "cell probability" => "细胞概率",
        "flow error" => "光流误差",
        "minimum size" => "最小尺寸",
        "tile overlap" => "瓦片重叠",
        "slice overlap" => "切片重叠",
        _ => english,
    };
    chinese.to_string()
}
