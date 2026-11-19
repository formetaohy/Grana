#[macro_export]
macro_rules! error {
    ($($event:tt)*) => {
        $crate::__tracing::error!($($event)*)
    };
}

#[macro_export]
macro_rules! info {
    ($($event:tt)*) => {
        $crate::__tracing::info!($($event)*)
    };
}

#[macro_export]
macro_rules! warn {
    ($($event:tt)*) => {
        $crate::__tracing::warn!($($event)*)
    };
}
