mod language;

pub mod animation;
pub mod annotation;
pub mod app;
pub mod batch;
pub mod coloc;
pub mod common;
pub mod header;
pub mod inspector;
pub mod measure;
pub mod object;
pub mod palette;
pub mod panel;
pub mod plate;
pub mod relation;
pub mod shell;
pub mod timeline;
pub mod track;

pub use language::Language;

#[macro_export]
macro_rules! labels {
    ($($name:ident => $english:literal / $chinese:literal;)*) => {
        $(
            pub fn $name(language: $crate::Language) -> &'static str {
                match language {
                    $crate::Language::English => $english,
                    $crate::Language::Chinese => $chinese,
                }
            }
        )*
    };
}

#[macro_export]
macro_rules! messages {
    ($($name:ident ($($argument:ident : $kind:ty),* $(,)?) => $english:literal / $chinese:literal;)*) => {
        $(
            pub fn $name(language: $crate::Language, $($argument: $kind),*) -> String {
                match language {
                    $crate::Language::English => format!($english, $($argument = $argument),*),
                    $crate::Language::Chinese => format!($chinese, $($argument = $argument),*),
                }
            }
        )*
    };
}
