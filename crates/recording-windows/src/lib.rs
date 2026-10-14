mod capture;
mod encoder;
mod inspect;

pub use capture::{Recording, SharedMetrics, start};
pub use inspect::{Display, HardwareInfo, inspect};
