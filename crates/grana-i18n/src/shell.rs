use crate::Language;
use crate::labels;
use crate::messages;

labels! {
    ome_zarr_folder => "OME-Zarr" / "OME-Zarr";
    remote_ome_zarr => "Remote" / "远程";
    project => "Project" / "项目";
    package => "Share" / "分享包";
    open_image => "Open Image…" / "打开图像…";
    tiles => "Tiles" / "瓦片";
    scanning_tiles => "Scanning tiles…" / "正在扫描瓦片…";
    stitch => "Stitch" / "拼接";
    channel => "Channel" / "通道";
    series => "Series" / "序列";
    grid => "Grid" / "网格";
    recover => "Recover" / "恢复";
    discard => "Discard" / "放弃";
    recent => "RECENT" / "最近";
    moments_ago => "moments ago" / "刚刚";
    minute_ago => "1m ago" / "1 分钟前";
    hour_ago => "1h ago" / "1 小时前";
    day_ago => "1d ago" / "1 天前";
}

messages! {
    steps_complete(done: usize, total: usize) => "{done}/{total}" / "{done}/{total}";
    tile_count(count: usize) => "{count} tiles" / "{count} 个瓦片";
    stitching(count: usize) => "Stitching {count} tiles…" / "正在拼接 {count} 个瓦片…";
    opening(label: &str) => "Opening {label}…" / "正在打开 {label}…";
    steps_stored(done: usize, total: usize) => "{done}/{total}" / "{done}/{total}";
    unsaved_analysis(name: &str) => "Unsaved · {name}" / "未保存 · {name}";
    saved_detail(detail: &str, saved: &str) => "{detail} · {saved}" / "{detail} · {saved}";
}

pub fn minutes_ago(language: Language, count: u64) -> String {
    match language {
        Language::English => format!("{count}m ago"),
        Language::Chinese => format!("{count} 分钟前"),
    }
}

pub fn hours_ago(language: Language, count: u64) -> String {
    match language {
        Language::English => format!("{count}h ago"),
        Language::Chinese => format!("{count} 小时前"),
    }
}

pub fn days_ago(language: Language, count: u64) -> String {
    match language {
        Language::English => format!("{count}d ago"),
        Language::Chinese => format!("{count} 天前"),
    }
}
