use crate::labels;

labels! {
    timeline => "Timeline" / "时间轴";
    preview => "Preview" / "预览";
    pause => "Pause" / "暂停";
    keyframe => "Keyframe" / "关键帧";
    delete_keyframe => "Delete keyframe" / "删除关键帧";
    add_keyframe => "Add a keyframe to animate the camera and time" / "添加关键帧以驱动相机与时间动画";
    movie => "Movie" / "影片";
    width => "Width" / "宽度";
    frame_rate => "Frame rate" / "帧率";
    duration => "Duration" / "时长";
    export_movie => "Export Movie (MP4)" / "导出影片 (MP4)";
    export_transparent => "Transparent Movie (MOV)" / "透明影片 (MOV)";
    still_sequence => "Still & Sequence" / "静帧与序列";
    transparent_background => "Transparent Background" / "透明背景";
    sequence => "Sequence" / "序列";
    save_image => "Save Image" / "保存图像";
    image_detail => "Eight bit PNG or sixteen bit OME-TIFF, chosen by the saved extension." / "8 位 PNG 或 16 位 OME-TIFF，由保存的扩展名决定。";
    save_figure => "Save Vector Figure" / "保存矢量图";
    figure_detail => "SVG or PDF with outlined text over the rendered volume." / "文字以轮廓形式叠加于渲染体之上的 SVG 或 PDF。";
    export_sequence => "Export Frame Sequence" / "导出帧序列";
    native_screen => "Native Screen" / "原生屏幕";
}

use crate::Language;
use crate::messages;

messages! {
    fps_frames(frames: u32) => "fps · {frames} frames" / "fps · {frames} 帧";
    duration_data(seconds: f32) => "s ({seconds:.1} s data)" / "s ({seconds:.1} s 数据)";
    exporting(frame: u32, total: u32) => "Exporting {frame}/{total}" / "正在导出 {frame}/{total}";
    png_frames(frames: u32) => "{frames} PNG frames" / "{frames} 个 PNG 帧";
}

pub fn keyframe_count(language: Language, count: usize) -> String {
    match language {
        Language::English => format!("{count} keyframe{}", if count == 1 { "" } else { "s" }),
        Language::Chinese => format!("{count} 个关键帧"),
    }
}
