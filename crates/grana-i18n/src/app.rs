use crate::labels;

labels! {
    unsaved_analysis => "Unsaved analysis" / "未保存的分析";
    unsaved_detail => "The analysis has unsaved changes. Save them before continuing?" / "分析中有未保存的更改。是否在继续前保存？";
    device_lost => "Graphics device lost" / "图形设备丢失";
    device_lost_detail => "The graphics device was lost, so grana stopped. The analysis was written to the recovery store; reopen grana to restore it." / "图形设备已丢失，Grana 已停止。分析已写入恢复存储，重新打开 Grana 即可恢复。";
    open_anything => "Open a dataset, project or image" / "打开数据集、项目或图像";
    open_folder => "Open a folder" / "打开文件夹";
    supported_files => "Datasets, projects and images" / "数据集、项目与图像";
    open_tile_folder => "Open a tile folder" / "打开瓦片文件夹";
    open_microscopy_image => "Open a microscopy image" / "打开显微图像";
    microscopy_image => "Microscopy image" / "显微图像";
    grana_project => "grana project" / "Grana 项目";
    open_grana_project => "Open a grana project" / "打开 Grana 项目";
    open_grana_package => "Open a share package" / "打开分享包";
    save_grana_project => "Save a grana project" / "保存 Grana 项目";
    save_image => "Save an image" / "保存图像";
    png_8 => "PNG (8-bit)" / "PNG（8 位）";
    ome_tiff_16 => "OME-TIFF (16-bit)" / "OME-TIFF（16 位）";
    save_figure => "Save a vector figure" / "保存矢量图";
    svg_vector => "SVG (vector)" / "SVG（矢量）";
    pdf_vector => "PDF (vector)" / "PDF（矢量）";
    save_statistics_chart => "Save a statistics chart" / "保存统计图";
    save_comparison_chart => "Save a comparison chart" / "保存比较图";
    save_lineage => "Save a track lineage" / "保存轨迹谱系";
    save_movie => "Save a movie" / "保存影片";
    save_transparent_movie => "Save a transparent movie" / "保存透明影片";
    choose_sequence_folder => "Choose a folder for the frame sequence" / "选择帧序列的保存文件夹";
    export_running => "Export running" / "导出进行中";
    export_running_detail => "An export is still writing its file. Cancel it before closing grana." / "导出仍在写入文件。请在关闭 Grana 前取消导出。";
    read_only => "Read-only" / "只读";
    read_only_detail => "This viewer shows a recorded analysis. Open the project in grana to edit it." / "查看器仅展示已记录的分析，请在 Grana 中打开项目以编辑。";
    viewer => "Viewer" / "查看器";
    unreadable_detail => "Read these regions again" / "重新读取这些区域";
}

use crate::messages;

messages! {
    saved(path: &str) => "saved {path}" / "已保存 {path}";
    unreadable(blocks: usize) => "{blocks} regions unreadable" / "{blocks} 块区域读不出";
    unsaved_field(name: &str) => "The analysis of {name} has unsaved changes. Save it in a project before continuing?" / "{name} 的分析有未保存的更改。是否在继续前保存为项目？";
    unsaved_unsavable(name: &str) => "The analysis of {name} has unsaved changes, and the dataset carries no file source to record them in a project. Discard it?" / "{name} 的分析有未保存的更改，且数据集没有可写入项目的文件来源。是否放弃该分析？";
}

labels! {
    export_cancelled => "the export was cancelled" / "导出已取消";
    export_worker_stopped => "the export worker stopped" / "导出线程已停止";
    wait_export => "wait for the running export to finish" / "请等待当前导出完成";
    dataset_opening => "the dataset is still opening" / "数据集仍在打开";
    wait_analysis => "wait for the running analysis before exporting" / "请等待当前分析完成后再导出";
    open_dataset_first => "open a dataset before exporting" / "请先打开数据集再导出";
    open_dataset_chart => "open a dataset before exporting a chart" / "请先打开数据集再导出图表";
    choose_plot => "choose a statistics plot before exporting its chart" / "请先选择统计图视图再导出";
    choose_condition => "choose a compared condition before exporting its comparison" / "请先选择比较条件再导出";
    open_tracked_set => "open a tracked object set before exporting its lineage" / "请先打开已追踪的对象集再导出谱系";
    set_keyframe => "set a keyframe before exporting" / "请先设置关键帧再导出";
    no_time_point => "the dataset has no time point to export" / "数据集没有可导出的时间点";
    export_running_error => "an export is already running" / "已有导出正在运行";
}
