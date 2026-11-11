use crate::labels;

labels! {
    file => "File" / "文件";
    edit => "Edit" / "编辑";
    view => "View" / "视图";
    time => "Time" / "时间";
    protocol => "Protocol" / "协议";
    export => "Export" / "导出";
    batch => "Batch" / "批处理";
    analysis => "Analysis" / "分析";
    play => "Play" / "播放";
    pause_playback => "Pause Playback" / "暂停播放";
    no_matches => "No command matches" / "没有匹配的命令";
    footer_hint => "↑↓ choose · ↩ run · esc close" / "↑↓ 选择 · ↩ 运行 · esc 关闭";
}

use crate::messages;

messages! {
    open_recent(name: &str) => "Open {name}" / "打开 {name}";
    apply_view(name: &str) => "Apply View {name}" / "应用视图 {name}";
}
