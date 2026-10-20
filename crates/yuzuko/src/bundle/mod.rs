//! Package config, the bundle writer, and the updater.

mod config;
mod icon;
mod package;
mod update;

pub use config::{
  Config, ConfigError, discover, load_for_run, load_package,
  reject_package_extension,
};
pub use package::{BundleLaunch, Target, bundled_launch, package};
pub use update::enable_updater;
