use crate::Language;
use crate::labels;
use crate::messages;

labels! {
    timeline => "Timeline" / "时间轴";
    preview => "Preview" / "预览";
    pause => "Pause" / "暂停";
    keyframe => "Keyframe" / "关键帧";
    delete_keyframe => "Delete keyframe" / "删除关键帧";
    add_keyframe => "No keyframes" / "暂无关键帧";
    movie => "Movie" / "影片";
    width => "Width" / "宽度";
    frame_rate => "Frame rate" / "帧率";
    duration => "Duration" / "时长";
    export_movie => "Export Movie (MP4)" / "导出影片 (MP4)";
    export_transparent => "Transparent Movie (MOV)" / "透明影片 (MOV)";
    still_sequence => "Still & Sequence" / "静帧与序列";
    transparent_background => "Transparent Background" / "透明背景";
    save_image => "Save Image" / "保存图像";
    save_figure => "Save Vector Figure" / "保存矢量图";
    export_sequence => "Export Frame Sequence" / "导出帧序列";
    native_screen => "Native Screen" / "原生屏幕";
}

messages! {
    fps_frames(frames: u32) => "fps · {frames}f" / "fps · {frames} 帧";
    duration_data(seconds: f32) => "s ({seconds:.1}s)" / "s ({seconds:.1}s)";
    exporting(frame: u32, total: u32) => "{frame}/{total}" / "{frame}/{total}";
}

pub fn keyframe_count(language: Language, count: usize) -> String {
    let _ = language;
    format!("{count}")
}
