use crate::Language;
use crate::labels;
use crate::messages;

labels! {
    no_plate => "No Plate" / "未加载孔板";
    export_montage => "Export Montage" / "导出拼图";
    no_field => "No fields" / "无视野";
}

messages! {
    summary(fields: usize) => "{fields} fields" / "{fields} 个视野";
    summary_previews(fields: usize, previews: usize) => "{fields} fields · {previews}" / "{fields} 个视野 · {previews}";
    grid_detail(rows: usize, columns: usize, wells: usize, previews: &str) => "{rows}×{columns} · {wells}w · {previews}" / "{rows}×{columns} · {wells}孔 · {previews}";
    well_header(label: &str) => "Well {label}" / "{label} 孔";
    well_fields(label: &str, count: usize) => "{label} · {count}" / "{label} · {count}";
    field_name(index: usize) => "Field {index}" / "视野 {index}";
    field_suffix(suffix: &str) => "Field {suffix}" / "视野 {suffix}";
}

pub fn field_count(language: Language, count: usize) -> String {
    match language {
        Language::English => format!("{count}"),
        Language::Chinese => format!("{count}"),
    }
}
