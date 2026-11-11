use crate::labels;

labels! {
    show_sidebar => "Show sidebar" / "显示侧边栏";
    hide_sidebar => "Hide sidebar" / "隐藏侧边栏";
    settings => "Settings" / "设置";
    open => "Open" / "打开";
    analysis => "Analysis" / "分析";
    frame_volume => "Frame the volume" / "适配体数据视图";
    nothing_to_undo => "Nothing to undo" / "没有可撤销的操作";
    nothing_to_redo => "Nothing to redo" / "没有可重做的操作";
    recent => "Recent" / "最近";
}

use crate::messages;

messages! {
    search_commands(shortcut: &str) => "Search commands ({shortcut})" / "搜索命令（{shortcut}）";
    undo_action(action: &str) => "Undo {action}" / "撤销{action}";
    redo_action(action: &str) => "Redo {action}" / "重做{action}";
}
