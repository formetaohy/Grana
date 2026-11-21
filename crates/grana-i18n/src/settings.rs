use crate::labels;

labels! {
    title => "Settings" / "设置";
    general => "General" / "通用";
    updates => "Updates" / "更新";
    about => "About" / "关于";
    language => "Language" / "语言";
    interface => "Interface" / "界面";
    files => "Files" / "文件";
    autosave => "Autosave" / "自动保存";
    cache => "Derived cache" / "派生缓存";
    data => "Data folder" / "数据目录";
    clearing => "Clearing" / "正在清除";
    graphics => "Graphics" / "图形";
    license => "License" / "许可";
    project_page => "Project page" / "项目主页";
    close => "Close" / "关闭";
    version => "Version" / "版本";
    check => "Check for updates" / "检查更新";
    checking => "Checking for updates" / "正在检查更新";
    up_to_date => "Up to date" / "已是最新";
    download => "Download" / "下载";
    downloading => "Downloading" / "下载中";
    ready => "Ready, restart to update" / "已就绪，重启后完成更新";
    restart => "Restart and update" / "重启并更新";
    notes => "Release notes" / "发布说明";
    automatic => "Check for updates automatically" / "自动检查更新";
    offline => "The release cannot be reached" / "无法连接发布服务器";
    corrupt => "The download does not match the release" / "下载内容与发布不符";
    signature => "The release is not signed by the publisher" / "发布包的签名与发布者不符";
    storage_failure => "The update cannot be written" / "无法写入更新文件";
    installer => "The installer did not finish" / "安装程序未完成";
    failure => "The update failed" / "更新失败";
    open => "Show the update" / "查看更新";
}

use crate::messages;

messages! {
    freed(bytes: &str) => "Freed {bytes}" / "已释放 {bytes}";
}
