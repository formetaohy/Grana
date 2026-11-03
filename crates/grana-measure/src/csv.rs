use std::borrow::Cow;

use crate::Column;

pub fn line(fields: &[&str]) -> String {
    let mut text = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&escape(field));
    }
    text.push('\n');
    text
}

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
        text.push_str(&escape(column.key()));
    }
    text.push('\n');
    for row in 0..rows {
        for (index, column) in columns.iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            text.push_str(&escape(&column.format(value(row, index))));
        }
        text.push('\n');
    }
    text
}

fn escape(value: &str) -> Cow<'_, str> {
    if value.contains([',', '"', '\n']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
}
