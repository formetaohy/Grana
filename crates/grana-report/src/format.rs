pub(crate) fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}

pub(crate) fn scientific(value: f64) -> String {
    let text = format!("{value:.3e}");
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let exponent = exponent
        .parse::<i32>()
        .expect("a formatted exponent is an integer");
    format!("{mantissa}e{exponent:+03}")
}
