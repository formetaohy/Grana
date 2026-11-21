use crate::labels;
use crate::messages;

labels! {
    show_sidebar => "Show sidebar" / "显示侧边栏";
    hide_sidebar => "Hide sidebar" / "隐藏侧边栏";
    settings => "Settings" / "设置";
    analysis => "Analysis" / "分析";
    frame_volume => "Frame volume" / "适配视图";
    nothing_to_undo => "Nothing to undo" / "无可撤销";
    nothing_to_redo => "Nothing to redo" / "无可重做";
    minimize_window => "Minimize" / "最小化";
    maximize_window => "Maximize" / "最大化";
    restore_window => "Restore" / "还原";
    close_window => "Close" / "关闭";
}

messages! {
    undo_action(action: &str) => "Undo {action}" / "撤销{action}";
    redo_action(action: &str) => "Redo {action}" / "重做{action}";
    network_transfer(rate: &str, resumed: &str) => "{rate} · {resumed} reused" / "{rate} · 复用{resumed}";
    network_detail(fetched: &str, resumed: &str) => "{fetched} read over the network, {resumed} reused after an interrupted transfer" / "网络只取回{fetched}，中断后续传复用{resumed}";
}
