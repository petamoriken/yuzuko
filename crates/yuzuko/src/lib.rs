//! Shared packaging used by the `yuzuko` binary and the integration tests.

mod bundle;
mod permissions;

pub use bundle::{
  BundleLaunch, Config, ConfigError, Target, bundled_launch, discover,
  enable_updater, load_for_run, load_package, package,
  reject_package_extension,
};
pub use permissions::{
  PermissionFlag, apply_permission_config, apply_permission_flags,
  current_dir as permission_dir, parse_permission_flag,
};
