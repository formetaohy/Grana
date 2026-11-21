mod apply;
mod assist;
mod error;
mod manifest;
mod query;
mod signature;
mod source;
mod stage;
mod target;

use semver::Version;

pub use apply::{hand_over, open, relaunch};
pub use assist::assist;
pub use error::{Error, Failure};
pub use manifest::{Artifact, Offer};
pub use query::check;
pub use source::Source;
pub use stage::{Staged, StagedArtifact, Staging};
pub use target::{Target, detect};

pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is semver")
}
