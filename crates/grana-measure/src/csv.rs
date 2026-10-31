use crate::Column;

pub(crate) fn write(
    columns: &[Column],
    rows: usize,
    value: impl Fn(usize, usize) -> f32,
) -> String {
    let mut text = String::new();
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(column.key());
    }
    text.push('\n');
    for row in 0..rows {
        for (index, column) in columns.iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            text.push_str(&column.format(value(row, index)));
        }
        text.push('\n');
    }
    text
}
