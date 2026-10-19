//! Shared packaging used by the `yuzuko` binary and the integration tests.

mod bundle;

pub use bundle::{
  BundleLaunch, Config, ConfigError, Target, bundled_launch, discover,
  enable_updater, load_for_run, load_package, package,
  reject_package_extension,
};
