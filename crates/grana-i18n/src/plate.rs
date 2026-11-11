use crate::labels;

labels! {
    no_plate => "No plate loaded" / "未加载孔板";
    no_plate_detail => "Open an OME-Zarr plate to browse its wells and jump between fields of view." / "打开 OME-Zarr 孔板以浏览各孔并在视野间跳转。";
    every_well => "Every well shows the projection of its first field of view; choose a well to open it." / "每个孔显示其首个视野的投影；选择孔即可打开。";
    export_montage => "Export Montage" / "导出拼图";
    no_field => "This well holds no field of view." / "此孔没有视野。";
}

use crate::Language;
use crate::messages;

messages! {
    summary(fields: usize) => "{fields} fields" / "{fields} 个视野";
    summary_previews(fields: usize, previews: usize) => "{fields} fields · {previews} previews" / "{fields} 个视野 · {previews} 个预览";
    grid_detail(rows: usize, columns: usize, wells: usize, previews: &str) => "{rows} × {columns} wells · {wells} wells · {previews}" / "孔板 {rows} × {columns} · {wells} 孔 · {previews}";
    well_header(label: &str) => "Well {label}" / "{label} 孔";
    well_fields(label: &str, count: usize) => "{label} · {count} fields" / "{label} · {count} 个视野";
    field_name(index: usize) => "Field {index}" / "视野 {index}";
    field_suffix(suffix: &str) => "Field {suffix}" / "视野 {suffix}";
}

pub fn field_count(language: Language, count: usize) -> String {
    match language {
        Language::English => format!("{count} field{}", if count == 1 { "" } else { "s" }),
        Language::Chinese => format!("{count} 个视野"),
    }
}
