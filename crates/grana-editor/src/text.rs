use grana_i18n::Language;
use grana_session::Action;

pub(crate) fn action(language: Language, action: Action) -> &'static str {
    let (english, chinese) = match action {
        Action::AddObject => ("add an object set", "添加对象集"),
        Action::AddSegment => ("add a segmentation", "添加分割"),
        Action::RemoveObject => ("remove an object set", "移除对象集"),
        Action::RenameObject => ("rename an object set", "重命名对象集"),
        Action::ReorderObject => ("reorder object sets", "重排对象集"),
        Action::DeleteObject => ("delete an object", "删除对象"),
        Action::DeleteFiltered => ("delete excluded objects", "删除被排除的对象"),
        Action::ExtractFiltered => ("filtered object set", "筛选后的对象集"),
        Action::DuplicateObject => ("copy an object set", "复制对象集"),
        Action::MergeObjects => ("merge object sets", "合并对象集"),
        Action::EditObjects => ("edit surfaces", "编辑表面"),
        Action::MaskEdit => ("mask painting", "掩膜绘制"),
        Action::ClearMask => ("mask clearing", "清除掩膜"),
        Action::ClearObject => ("clear objects", "清空对象"),
        Action::Seed => ("seed points", "种子点"),
        Action::SeedSource => ("seed source", "种子来源"),
        Action::Detect => ("detection", "检测"),
        Action::Relation => ("object relation", "对象关系"),
        Action::Track => ("tracking", "轨迹追踪"),
        Action::ClearTracks => ("clear tracks", "清空轨迹"),
        Action::CutTrack => ("track cut", "截断轨迹"),
        Action::JoinTracks => ("track join", "连接轨迹"),
        Action::DivideTracks => ("track division", "轨迹分裂"),
        Action::MergeTracks => ("track merge", "轨迹融合"),
        Action::ReparentTrack => ("track parent", "轨迹父级"),
        Action::DetachTrack => ("track detach", "断开轨迹"),
        Action::ChannelEnabled => ("channel visibility", "通道可见性"),
        Action::ChannelOpacity => ("channel opacity", "通道不透明度"),
        Action::Window => ("intensity window", "强度窗口"),
        Action::ResetWindow => ("window reset", "重置窗口"),
        Action::Transfer => ("transfer function", "传递函数"),
        Action::VolumeMode => ("volume mode", "体渲染模式"),
        Action::Isovalue => ("isovalue", "等值"),
        Action::Derived => ("derived channel", "派生通道"),
        Action::ObjectVisible => ("object visibility", "对象可见性"),
        Action::ObjectLabels => ("object labels", "对象标签"),
        Action::ObjectHidden => ("hidden objects", "隐藏对象"),
        Action::ObjectColor => ("object color", "对象颜色"),
        Action::ObjectOpacity => ("object opacity", "对象不透明度"),
        Action::ObjectMaterial => ("object material", "对象材质"),
        Action::ObjectParameters => ("object parameters", "对象参数"),
        Action::ObjectColoring => ("object colors", "对象着色"),
        Action::Mask => ("analysis mask", "分析掩膜"),
        Action::Annotation => ("annotation", "标注"),
        Action::TrackVisible => ("track visibility", "轨迹可见性"),
        Action::TrackThickness => ("track width", "轨迹宽度"),
        Action::TrackColoring => ("track colors", "轨迹着色"),
        Action::TrackParameters => ("track parameters", "轨迹参数"),
        Action::Region => ("region", "区域"),
        Action::Clip => ("clipping plane", "裁剪平面"),
        Action::Exposure => ("exposure", "曝光"),
        Action::StepScale => ("sampling", "采样"),
        Action::Shading => ("lighting", "光照"),
        Action::ResetRendering => ("rendering reset", "重置渲染"),
        Action::Classifier => ("object classifier", "对象分类器"),
        Action::MeasureGroup => ("statistics group", "统计分组"),
        Action::MeasureAggregate => ("statistics aggregate", "统计聚合"),
        Action::MeasureScope => ("statistics range", "统计范围"),
        Action::MeasureSet => ("statistics object set", "统计对象集"),
        Action::MeasureColumns => ("statistics columns", "统计列"),
        Action::MeasureView => ("statistics view", "统计视图"),
        Action::MeasureAxes => ("statistics axes", "统计坐标轴"),
        Action::MeasureSort => ("statistics order", "统计排序"),
        Action::MeasureLasso => ("statistics lasso", "统计套索"),
        Action::MeasureFilter => ("statistics filter", "统计筛选"),
        Action::AddFilter => ("filter addition", "添加筛选"),
        Action::RemoveFilter => ("filter removal", "移除筛选"),
        Action::MeasureVisibility => ("statistics visibility", "统计可见性"),
        Action::ColocPair => ("colocalization channels", "共定位通道"),
        Action::ColocThresholds => ("colocalization thresholds", "共定位阈值"),
        Action::ColocSettings => ("colocalization settings", "共定位设置"),
        Action::ColocResult => ("colocalization result", "共定位结果"),
        Action::ComputeColoc => ("colocalization", "共定位计算"),
        Action::ObjectColoc => ("object colocalization", "对象共定位"),
        Action::ApplyProtocol => ("protocol replay", "重放协议"),
        Action::Drift => ("drift correction", "漂移校正"),
        Action::Keyframe => ("keyframes", "关键帧"),
        Action::TimelineFps => ("frame rate", "帧率"),
        Action::TimelineDuration => ("duration", "时长"),
    };
    match language {
        Language::English => english,
        Language::Chinese => chinese,
    }
}

pub(crate) fn command(language: Language, id: &str) -> &'static str {
    let (english, chinese) = match id {
        "palette.open" => ("Search Commands…", "搜索命令…"),
        "open.image" => ("Open Image…", "打开图像…"),
        "open.tiles" => ("Open Tile Folder…", "打开瓦片文件夹…"),
        "open.folder" => ("Open OME-Zarr Folder…", "打开 OME-Zarr 文件夹…"),
        "open.remote" => ("Open Remote OME-Zarr…", "打开远程 OME-Zarr…"),
        "open.project" => ("Open Project…", "打开项目…"),
        "save.project" => ("Save Project", "保存项目"),
        "save.as" => ("Save Project As…", "项目另存为…"),
        "snapshot" => ("Save Snapshot…", "保存快照…"),
        "figure" => ("Save Vector Figure…", "保存矢量图…"),
        "lineage" => ("Save Track Lineage…", "保存轨迹谱系…"),
        "labels" => ("Export Labels Only…", "仅导出标签…"),
        "undo" => ("Undo", "撤销"),
        "redo" => ("Redo", "重做"),
        "sidebar.toggle" => ("Toggle Sidebar", "切换侧边栏"),
        "language.english" => ("Language: English", "语言：English"),
        "language.chinese" => ("Language: Chinese", "语言：中文"),
        "appearance.contrast" => ("Toggle Increased Contrast", "切换增强对比度"),
        "appearance.transparency" => ("Toggle Reduced Transparency", "切换降低透明度"),
        "view.fit" => ("Frame the Volume", "适配体数据视图"),
        "panel.close" => ("Close Panel", "关闭面板"),
        "panel.statistics" => ("Show Statistics", "显示统计"),
        "panel.tracks" => ("Show Tracks", "显示轨迹"),
        "panel.colocalization" => ("Show Colocalization", "显示共定位"),
        "panel.relations" => ("Show Relations", "显示关系"),
        "panel.annotations" => ("Show Annotations", "显示标注"),
        "panel.plate" => ("Show Plate", "显示孔板"),
        "panel.batch" => ("Show Batch", "显示批处理"),
        "panel.animation" => ("Show Animation", "显示动画"),
        "camera.perspective" => ("Perspective Projection", "透视投影"),
        "camera.orthographic" => ("Orthographic Projection", "正交投影"),
        "camera.focus" => ("Focus Camera on Pivot", "聚焦到枢轴点"),
        "view.save" => ("Save View", "保存视图"),
        "keyframe.save" => ("Save Keyframe", "保存关键帧"),
        "keyframes.clear" => ("Clear Keyframes", "清除关键帧"),
        "protocol.capture" => ("Capture Protocol", "捕获协议"),
        "protocol.load" => ("Load Protocol…", "加载协议…"),
        "protocol.save" => ("Save Protocol…", "保存协议…"),
        "protocol.apply" => ("Apply Protocol", "应用协议"),
        "movie" => ("Export Movie…", "导出影片…"),
        "movie.transparent" => ("Export Transparent Movie…", "导出透明影片…"),
        "sequence" => ("Export Frame Sequence…", "导出帧序列…"),
        "measure.export" => ("Export Measurements…", "导出测量…"),
        "provenance.export" => ("Export Provenance Crate…", "导出溯源包…"),
        "dataset.export" => ("Export Dataset…", "导出数据集…"),
        "plate.export" => ("Export Plate…", "导出孔板…"),
        "batch.run" => ("Run Batch", "运行批处理"),
        "batch.pause" => ("Pause Batch", "暂停批处理"),
        "batch.resume" => ("Resume Batch", "继续批处理"),
        "batch.cancel" => ("Cancel Batch", "取消批处理"),
        "coloc.compute" => ("Compute Colocalization", "计算共定位"),
        "drift.compute" => ("Correct Drift", "校正漂移"),
        _ => panic!("every command carries a title"),
    };
    match language {
        Language::English => english,
        Language::Chinese => chinese,
    }
}

pub(crate) fn column(language: Language, label: &str) -> String {
    grana_i18n::measure::column(language, label)
}

pub(crate) fn legend(language: Language, title: &str) -> String {
    if language == Language::English {
        return title.to_string();
    }
    title
        .split(" · ")
        .map(|part| legend_part(language, part))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn legend_part(language: Language, part: &str) -> String {
    if let Some(name) = part.strip_suffix(" tracks") {
        return format!("{name} 轨迹");
    }
    if let Some(name) = part.strip_suffix(" intensity")
        && let Some(term) = grana_i18n::measure::term(language, "intensity")
    {
        return format!("{name} {term}");
    }
    if let Some(term) = grana_i18n::measure::term(language, part) {
        return term;
    }
    if let Some((name, rest)) = part.split_once(' ')
        && let Some(term) = grana_i18n::measure::term(language, name)
    {
        return format!("{term} {rest}");
    }
    column(language, part)
}
