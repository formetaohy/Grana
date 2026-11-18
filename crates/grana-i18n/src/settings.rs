use crate::labels;

labels! {
    title => "Settings" / "设置";
    appearance => "Appearance" / "外观";
    language => "Language" / "语言";
    language_detail => "Names, labels and messages follow this language." / "名称、标签与提示都随此语言切换。";
    close => "Close settings" / "关闭设置";
    about => "About" / "关于";
    version => "Version" / "版本";
    motion_detail => "Spring curves follow Material 3 Expressive." / "弹簧曲线遵循 Material 3 Expressive。";
}

use crate::messages;

messages! {
    frames_per_second(fps: f32) => "{fps:.1} frames per second" / "{fps:.1} 帧每秒";
    recent_count(count: usize) => "{count} entries" / "{count} 项";
}
