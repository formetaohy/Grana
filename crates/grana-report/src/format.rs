pub(crate) fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}

pub(crate) fn scientific(value: f64) -> String {
    grana_measure::csv::scientific(value)
}
