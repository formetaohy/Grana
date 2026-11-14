use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::{self, Write};

use crate::Column;

const BLOCK: usize = 4096;

const FIELD: usize = 16;

const STEPS: usize = 256;

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

pub fn scientific(value: f64) -> String {
    let text = format!("{value:.3e}");
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let exponent = exponent
        .parse::<i32>()
        .expect("a formatted exponent is an integer");
    format!("{mantissa}e{exponent:+03}")
}

pub(crate) fn write(
    writer: &mut impl Write,
    columns: &[Column],
    rows: &[u32],
    values: impl Fn(usize) -> std::sync::Arc<Vec<f32>>,
    report: &mut dyn FnMut(usize, usize),
) -> io::Result<()> {
    let total = rows.len();
    let mut text = String::with_capacity(BLOCK * columns.len() * FIELD);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&escape(column.key()));
    }
    text.push('\n');
    writer.write_all(text.as_bytes())?;
    report(0, total);
    let stride = total.div_ceil(STEPS).max(BLOCK);
    let mut reported = 0;
    let mut start = 0;
    while start < total {
        let end = (start + BLOCK).min(total);
        let block = (0..columns.len()).map(&values).collect::<Vec<_>>();
        text.clear();
        for row in &rows[start..end] {
            let row = *row as usize;
            for (index, column) in columns.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                write!(text, "{:.*}", column.decimals() as usize, block[index][row])
                    .expect("a csv field writes into a string");
            }
            text.push('\n');
        }
        writer.write_all(text.as_bytes())?;
        start = end;
        if end - reported >= stride || end == total {
            report(end, total);
            reported = end;
        }
    }
    Ok(())
}

fn escape(value: &str) -> Cow<'_, str> {
    if value.contains([',', '"', '\n']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
}
