use std::fmt::Write as _;

use grana_chart::{Chart, DistributionStyle, Gates, Heading, Page};
use grana_measure::{Aggregate, Column, Mask, MeasureView, Table, Window};
use grana_ui::{Color, Theme};

const CHART_NAMESPACE: &str = "grana-report-";
const STATISTICS: usize = 5;
const TREND_FRAMES: usize = 3;

const PRIMARY: [&str; 12] = [
    "volume_um3",
    "length_um",
    "diameter_um",
    "area_um2",
    "max_diameter_um",
    "hull_volume_um3",
    "nucleus_volume_um3",
    "cytoplasm_volume_um3",
    "sphericity",
    "nucleus_sphericity",
    "contrast",
    "nuclear_ratio",
];

const POSITION: [&str; 3] = ["x_um", "y_um", "z_um"];

pub struct Report<'a> {
    pub title: &'a str,
    pub facts: &'a [(String, String)],
    pub thumbnail: Option<&'a [u8]>,
    pub sections: &'a [Section<'a>],
    pub steps: &'a [String],
    pub files: &'a [String],
    pub version: &'a str,
    pub created: &'a str,
}

pub struct Section<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub origin: &'a str,
    pub objects: usize,
    pub tracks: Option<usize>,
    pub table: &'a Table,
    pub file: &'a str,
}

pub fn html(report: &Report<'_>) -> Result<String, String> {
    let palette = Theme::dark().palette;
    let mut page = String::with_capacity(64 << 10);
    let _ = write!(
        page,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n{}</style>\n</head>\n<body>\n",
        escape(report.title),
        style(palette)
    );
    let _ = write!(
        page,
        "<header class=\"masthead\"><span class=\"mark\"></span><span class=\"wordmark\">Grana</span><span class=\"tag\">share report</span></header>\n<main>\n"
    );
    hero(&mut page, report);
    for (index, held) in report.sections.iter().enumerate() {
        section(&mut page, held, index)?;
    }
    if !report.steps.is_empty() {
        let _ = writeln!(
            page,
            "<section class=\"card\"><h2>How the analysis reproduces</h2><ol class=\"steps\">"
        );
        for step in report.steps {
            let _ = writeln!(page, "<li>{}</li>", escape(step));
        }
        let _ = writeln!(
            page,
            "</ol><p class=\"note\">protocol.json replays these steps over the dataset in this package.</p></section>"
        );
    }
    if !report.files.is_empty() {
        let _ = writeln!(
            page,
            "<section class=\"card\"><h2>What this package holds</h2><ul class=\"files\">"
        );
        for file in report.files {
            let _ = writeln!(page, "<li>{}</li>", escape(file));
        }
        page.push_str("</ul></section>\n");
    }
    let _ = write!(
        page,
        "</main>\n<footer>Grana {} wrote this report on {}. The recorded analysis opens in Grana from analysis.grana; every table is a plain CSV file beside it.</footer>\n</body>\n</html>\n",
        escape(report.version),
        escape(report.created)
    );
    Ok(page)
}

fn hero(page: &mut String, report: &Report<'_>) {
    page.push_str("<section class=\"hero\">\n");
    match report.thumbnail {
        Some(png) => {
            let _ = writeln!(
                page,
                "<figure class=\"preview\"><img alt=\"the recorded view\" src=\"data:image/png;base64,{}\"></figure>",
                grana_vector::encode_base64(png)
            );
        }
        None => page.push_str("<figure class=\"preview empty\">no recorded view</figure>\n"),
    }
    let _ = write!(
        page,
        "<div class=\"lede\">\n<h1>{}</h1>\n<dl class=\"facts\">\n",
        escape(report.title)
    );
    for (label, value) in report.facts {
        let _ = writeln!(
            page,
            "<div><dt>{}</dt><dd>{}</dd></div>",
            escape(label),
            escape(value)
        );
    }
    page.push_str("</dl>\n</div>\n</section>\n");
}

fn section(page: &mut String, section: &Section<'_>, index: usize) -> Result<(), String> {
    let _ = write!(
        page,
        "<section class=\"card\"><div class=\"head\"><h2>{}</h2><span class=\"chip\">{}</span><span class=\"chip\">{}</span></div>\n<p class=\"meta\">{} {}{}</p>\n",
        escape(section.name),
        escape(section.kind),
        escape(section.origin),
        count(section.objects),
        plural(section.objects, "object", "objects"),
        match section.tracks {
            Some(tracks) => format!(" · {} {}", count(tracks), plural(tracks, "track", "tracks")),
            None => String::new(),
        }
    );
    let primitive = primary(section.table);
    if let Some(column) = primitive {
        let name = label(&section.table.columns()[column]);
        let objects = plural(section.table.rows(), "object", "objects");
        let histogram_title = format!("{} in {}", name, section.name);
        let histogram_y = format!("Y: {objects}");
        let histogram_x = format!("X: {name}");
        let histogram_summary = format!("{} · {}", name, count(section.table.rows()));
        let trend_title = format!("{} over time", name);
        let trend_y = format!("Y: {name}");
        let trend_summary = format!("{} frames", frames(section.table));
        page.push_str("<div class=\"charts\">\n");
        page.push_str(&plot(Panel {
            table: section.table,
            column,
            view: MeasureView::Histogram,
            title: &histogram_title,
            y: &histogram_y,
            x: &histogram_x,
            summary: histogram_summary,
            namespace: index,
        })?);
        if frames(section.table) >= TREND_FRAMES {
            page.push_str(&plot(Panel {
                table: section.table,
                column,
                view: MeasureView::Trend,
                title: &trend_title,
                y: &trend_y,
                x: "X: frame",
                summary: trend_summary,
                namespace: index + 1000,
            })?);
        }
        page.push_str("</div>\n");
    }
    statistics(page, section.table, section.name);
    let _ = writeln!(
        page,
        "<p class=\"note\">every object, frame by frame: <a href=\"{}\">{}</a></p></section>",
        escape(section.file),
        escape(section.file)
    );
    Ok(())
}

struct Panel<'a> {
    table: &'a Table,
    column: usize,
    view: MeasureView,
    title: &'a str,
    y: &'a str,
    x: &'a str,
    summary: String,
    namespace: usize,
}

fn plot(panel: Panel<'_>) -> Result<String, String> {
    let gated = Mask::all(panel.table.rows());
    let Some(plot) = panel.table.plot(
        panel.view,
        [panel.column, panel.column],
        &gated,
        window(panel.table),
    ) else {
        return Ok(String::new());
    };
    let built = Chart {
        heading: Heading::new(panel.title, panel.y, panel.x)
            .with_summary(panel.summary)
            .with_empty("no observation to plot"),
        plot,
        gate: Gates::default(),
        highlight: None,
        distribution: DistributionStyle::Box,
    };
    let page = Page::Plot(Box::new(built));
    let canvas = grana_page::canvas(page.size(), &page.marks())?;
    Ok(format!(
        "<div class=\"chart\">{}</div>\n",
        grana_vector::write_svg_inline(&canvas, &format!("{CHART_NAMESPACE}{}-", panel.namespace))
    ))
}

fn statistics(page: &mut String, table: &Table, name: &str) {
    let aggregates = aggregates();
    let columns = table
        .columns()
        .iter()
        .enumerate()
        .skip(table.identity_columns())
        .filter(|(_, column)| !POSITION.contains(&column.key()))
        .filter_map(|(index, column)| {
            let values = (0..table.rows())
                .map(|row| table.value(row, index))
                .filter(|value| value.is_finite())
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some((column, values))
        })
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return;
    }
    let _ = write!(
        page,
        "<details class=\"statistics\"><summary>Statistics of {}</summary>\n<table>\n<thead><tr><th>measurement</th>",
        escape(name)
    );
    let _ = write!(page, "<th>unit</th>");
    for (label, _) in &aggregates {
        let _ = write!(page, "<th>{label}</th>");
    }
    page.push_str("<th>n</th></tr></thead>\n<tbody>\n");
    for (column, values) in columns {
        let decimals = column.decimals();
        let _ = write!(page, "<tr><td>{}</td>", escape(column.label()));
        let _ = write!(
            page,
            "<td class=\"unit\">{}</td>",
            escape(column.unit().suffix())
        );
        for (_, aggregate) in &aggregates {
            let value = aggregate.apply(&values);
            let _ = write!(page, "<td>{}</td>", number(value, decimals));
        }
        let _ = writeln!(page, "<td>{}</td></tr>", values.len());
    }
    let _ = write!(
        page,
        "</tbody>\n</table>\n</details>\n<p class=\"note\">statistics over every frame of {}</p>\n",
        escape(name)
    );
}

fn aggregates() -> [(&'static str, Aggregate); STATISTICS] {
    [
        ("mean", Aggregate::MEAN),
        ("SD", Aggregate::DEVIATION),
        (
            "median",
            Aggregate::quantile(50).expect("the median is a catalog quantile"),
        ),
        ("min", Aggregate::MINIMUM),
        ("max", Aggregate::MAXIMUM),
    ]
}

fn primary(table: &Table) -> Option<usize> {
    let columns = table.columns();
    for key in PRIMARY {
        if let Some(index) = table.column(key)
            && spread(table, index)
        {
            return Some(index);
        }
    }
    let intensity = (table.identity_columns()..columns.len())
        .find(|index| columns[*index].key().ends_with("_mean") && spread(table, *index));
    intensity.or_else(|| {
        (table.identity_columns()..columns.len())
            .find(|index| !POSITION.contains(&columns[*index].key()) && spread(table, *index))
    })
}

fn spread(table: &Table, column: usize) -> bool {
    table.extent(column).is_some_and(|(low, high)| high > low)
}

fn window(table: &Table) -> Window {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for (axis, key) in POSITION.iter().enumerate() {
        let Some(column) = table.column(key).and_then(|column| table.extent(column)) else {
            continue;
        };
        low[axis] = column.0;
        high[axis] = column.1;
    }
    if low.iter().chain(&high).all(|value| value.is_finite())
        && let Some(window) = Window::new(low, high)
    {
        return window;
    }
    Window::new([-1.0; 3], [1.0; 3]).expect("the fallback window spans a unit box")
}

fn frames(table: &Table) -> usize {
    let mut times = (0..table.rows())
        .filter_map(|row| table.observation(row).time())
        .collect::<Vec<_>>();
    times.sort_unstable();
    times.dedup();
    times.len()
}

fn label(column: &Column) -> String {
    match column.unit().suffix() {
        "" => column.label().to_string(),
        suffix => format!("{} ({suffix})", column.label()),
    }
}

fn count(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

fn plural(value: usize, one: &str, many: &str) -> String {
    if value == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

fn number(value: f32, decimals: u8) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    let magnitude = value.abs();
    if magnitude != 0.0 && !(0.001..100_000.0).contains(&magnitude) {
        return format!("{value:.3e}");
    }
    format!("{value:.*}", usize::from(decimals))
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn hex(color: Color) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.red),
        channel(color.green),
        channel(color.blue)
    )
}

fn style(palette: grana_ui::Palette) -> String {
    format!(
        ":root{{color-scheme:dark;--surface:{surface};--surface-low:{surface_low};--surface-high:{surface_high};--on-surface:{on_surface};--variant:{on_surface_variant};--outline-variant:{outline_variant};--primary:{primary};--primary-container:{primary_container};--on-primary-container:{on_primary_container};--tertiary:{tertiary}}}\n\
*{{box-sizing:border-box}}\n\
html,body{{margin:0;padding:0}}\n\
body{{background:var(--surface);color:var(--on-surface);font-family:'Segoe UI Variable Display','Segoe UI',Roboto,'Helvetica Neue',system-ui,sans-serif;font-size:15px;line-height:1.55;-webkit-font-smoothing:antialiased}}\n\
.masthead{{display:flex;align-items:center;gap:12px;padding:20px clamp(20px,5vw,72px) 8px;color:var(--variant);font-size:13px;letter-spacing:.08em;text-transform:uppercase}}\n\
.mark{{width:22px;height:22px;border-radius:7px;background:var(--primary-container);box-shadow:inset 0 0 0 1px var(--outline-variant);position:relative}}\n\
.mark::after{{content:'';position:absolute;left:6px;right:6px;top:10px;height:2px;border-radius:1px;background:var(--primary)}}\n\
.wordmark{{color:var(--on-surface);font-weight:600;letter-spacing:.16em}}\n\
.tag{{margin-left:auto}}\n\
main{{padding:8px clamp(20px,5vw,72px) 56px;display:flex;flex-direction:column;gap:20px}}\n\
.hero{{display:grid;grid-template-columns:minmax(0,1.35fr) minmax(0,1fr);gap:24px;align-items:stretch}}\n\
@media (max-width:820px){{.hero{{grid-template-columns:1fr}}}}\n\
.preview{{margin:0;border-radius:28px;overflow:hidden;background:var(--surface-low);border:1px solid var(--outline-variant);min-height:220px;display:flex;align-items:center;justify-content:center}}\n\
.preview img{{display:block;width:100%;height:100%;object-fit:cover}}\n\
.preview.empty{{color:var(--variant);font-size:13px}}\n\
.lede{{display:flex;flex-direction:column;justify-content:center;gap:14px}}\n\
h1{{margin:0;font-size:clamp(24px,3vw,36px);line-height:1.15;font-weight:600;letter-spacing:-.01em}}\n\
h2{{margin:0;font-size:17px;font-weight:600;letter-spacing:.01em}}\n\
.facts{{margin:0;display:flex;flex-direction:column;gap:2px;font-size:14px}}\n\
.facts div{{display:flex;justify-content:space-between;gap:16px;padding:5px 0;border-bottom:1px solid var(--outline-variant)}}\n\
.facts div:last-child{{border-bottom:0}}\n\
.facts dt{{color:var(--variant)}}\n\
.facts dd{{margin:0;text-align:right;font-variant-numeric:tabular-nums}}\n\
.card{{background:var(--surface-low);border:1px solid var(--outline-variant);border-radius:24px;padding:22px clamp(18px,3vw,30px) 20px;display:flex;flex-direction:column;gap:14px}}\n\
.head{{display:flex;align-items:center;gap:10px;flex-wrap:wrap}}\n\
.chip{{padding:3px 12px;border-radius:999px;background:var(--surface-high);color:var(--variant);font-size:12px;letter-spacing:.03em}}\n\
.head .chip:first-of-type{{background:var(--primary-container);color:var(--on-primary-container)}}\n\
.meta{{margin:0;color:var(--variant);font-size:13.5px}}\n\
.charts{{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:16px}}\n\
.chart{{border-radius:18px;overflow:hidden;background:var(--surface);min-width:0}}\n\
.chart svg{{display:block;width:100%;height:auto}}\n\
.statistics{{border-top:1px solid var(--outline-variant);padding-top:12px}}\n\
.statistics summary{{cursor:pointer;color:var(--primary);font-size:13.5px;letter-spacing:.02em}}\n\
table{{width:100%;border-collapse:collapse;margin-top:10px;font-size:13px;font-variant-numeric:tabular-nums}}\n\
th,td{{text-align:right;padding:6px 8px;border-bottom:1px solid var(--outline-variant);white-space:nowrap}}\n\
th{{color:var(--variant);font-weight:500;font-size:12px;letter-spacing:.04em;text-transform:uppercase}}\n\
th:first-child,td:first-child{{text-align:left;white-space:normal}}\n\
td.unit{{color:var(--variant);text-align:left;width:1%}}\n\
tbody tr:last-child td{{border-bottom:0}}\n\
.note{{margin:0;color:var(--variant);font-size:13px}}\n\
a{{color:var(--tertiary);text-decoration:none;border-bottom:1px solid var(--outline-variant)}}\n\
a:hover{{border-bottom-color:var(--tertiary)}}\n\
.steps,.files{{margin:0;padding-left:22px;color:var(--on-surface);font-size:14px}}\n\
.steps li,.files li{{padding:3px 0}}\n\
.files{{column-count:2;column-gap:28px;padding-left:20px}}\n\
@media (max-width:720px){{.files{{column-count:1}}}}\n\
footer{{padding:0 clamp(20px,5vw,72px) 48px;color:var(--variant);font-size:12.5px;line-height:1.7;max-width:1100px}}\n",
        surface = hex(palette.surface),
        surface_low = hex(palette.surface_container_low),
        surface_high = hex(palette.surface_container_high),
        on_surface = hex(palette.on_surface),
        on_surface_variant = hex(palette.on_surface_variant),
        outline_variant = hex(palette.outline_variant),
        primary = hex(palette.primary),
        primary_container = hex(palette.primary_container),
        on_primary_container = hex(palette.on_primary_container),
        tertiary = hex(palette.tertiary),
    )
}
