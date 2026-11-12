use crate::labels;

labels! {
    ome_zarr_folder => "OME-Zarr Folder" / "OME-Zarr 文件夹";
    remote_ome_zarr => "Remote OME-Zarr" / "远程 OME-Zarr";
    project => "Project" / "项目";
    package => "Share" / "分享包";
    open_image => "Open Image…" / "打开图像…";
    tiles => "Tiles" / "瓦片";
    stitch => "Stitch" / "拼接";
    channel => "Channel" / "通道";
    series => "Series" / "序列";
    grid => "Grid" / "网格";
    recover => "Recover" / "恢复";
    discard => "Discard" / "放弃";
    recent => "RECENT" / "最近";
    nothing_opened => "Nothing could be opened" / "无法打开";
    ready => "Ready" / "就绪";
    no_dataset => "No dataset open" / "未打开数据集";
    no_dataset_detail => "OME-Zarr, CZI, ND2, LIF and OME-TIFF data open here, locally or over HTTP." / "可在此打开本地或通过 HTTP 访问的 OME-Zarr、CZI、ND2、LIF 与 OME-TIFF 数据。";
    loading_detail => "The dataset streams in as it is explored, so the window stays live." / "数据集会在浏览时流式载入，窗口始终保持响应。";
    moments_ago => "moments ago" / "刚刚";
    minute_ago => "a minute ago" / "1 分钟前";
    hour_ago => "an hour ago" / "1 小时前";
    day_ago => "a day ago" / "1 天前";
}

use crate::Language;
use crate::messages;

messages! {
    steps_complete(done: usize, total: usize) => "{done} of {total} steps complete." / "已完成 {done}/{total} 步。";
    tile_count(count: usize) => "{count} tiles" / "{count} 个瓦片";
    stitching(count: usize) => "Stitching {count} tiles" / "正在拼接 {count} 个瓦片";
    opening(label: &str) => "Opening {label}" / "正在打开 {label}";
    steps_stored(done: usize, total: usize) => "{done} of {total} steps stored." / "已存储 {done}/{total} 步。";
    unsaved_analysis(name: &str) => "Unsaved analysis of {name}" / "{name} 的未保存分析";
    saved_detail(detail: &str, saved: &str) => "{detail} · saved {saved}" / "{detail} · 保存于 {saved}";
}

pub fn minutes_ago(language: Language, count: u64) -> String {
    match language {
        Language::English => format!("{count} minutes ago"),
        Language::Chinese => format!("{count} 分钟前"),
    }
}

pub fn hours_ago(language: Language, count: u64) -> String {
    match language {
        Language::English => format!("{count} hours ago"),
        Language::Chinese => format!("{count} 小时前"),
    }
}

pub fn days_ago(language: Language, count: u64) -> String {
    match language {
        Language::English => format!("{count} days ago"),
        Language::Chinese => format!("{count} 天前"),
    }
}
