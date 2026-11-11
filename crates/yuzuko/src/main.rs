use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use yuzuko::{
  PermissionFlag, Target, apply_permission_config, apply_permission_flags,
  bundled_launch, load_for_run, load_package, package, parse_permission_flag,
  permission_dir, reject_package_extension,
};
use yuzuko_core::{
  PermissionSpec, WatchMode, install_permissions, run_file, run_file_sandboxed,
  run_file_watch, run_repl, run_tests, set_args, set_build_standalone,
  status_of,
};

fn main() -> ExitCode {
  yuzuko_dom::register();
  yuzuko_winter::register();
  #[cfg(feature = "window")]
  yuzuko_window::register();
  #[cfg(feature = "image")]
  yuzuko_image::register();
  #[cfg(feature = "codec")]
  yuzuko_codec::register();
  #[cfg(feature = "canvas")]
  yuzuko_canvas::register();
  #[cfg(feature = "webgpu")]
  yuzuko_webgpu::register();
  #[cfg(feature = "audio")]
  yuzuko_audio::register();
  yuzuko::install_auto_update();
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  // A packaged executable always runs its entry. It does not dispatch
  // `run`, `test`, `package`, or `repl`. macOS Launch Services may insert
  // `-psn_` tokens in any position. Those are not script arguments.
  if let Some(bundle) = bundled_launch() {
    configure_window(false, false);
    #[cfg(feature = "window")]
    yuzuko_window::set_default_title(&bundle.name);
    #[cfg(feature = "window")]
    if let Some(identifier) = &bundle.identifier {
      yuzuko_window::set_app_identity(identifier, &bundle.resources);
    }
    #[cfg(feature = "fs")]
    yuzuko_core::set_process_bundle_root(&application_bundle_root(&bundle));
    set_build_standalone(true);
    yuzuko::enable_updater(bundle.resources);
    if let Some(value) = &bundle.permissions {
      let mut spec = PermissionSpec {
        prompt: false,
        ..PermissionSpec::default()
      };
      if let Err(error) = apply_permission_config(&mut spec, value, true)
        .and_then(|_| install_permissions(&spec, &bundle.sandbox))
      {
        eprintln!("{error}");
        return ExitCode::from(1);
      }
    }
    set_args(
      args
        .into_iter()
        .filter(|arg| !arg.starts_with("-psn_"))
        .collect(),
    );
    let code =
      match status_of(run_file_sandboxed(&bundle.entry, Some(&bundle.sandbox)))
      {
        Ok(code) => code,
        Err(error) => {
          eprintln!("{error}");
          1
        }
      };
    return ExitCode::from(code);
  }
  match dispatch(args) {
    Ok(code) => ExitCode::from(code),
    Err(error) => {
      eprintln!("{error}");
      ExitCode::from(1)
    }
  }
}

fn dispatch(args: Vec<String>) -> Result<u8, yuzuko_core::RuntimeError> {
  let Some(command) = args.first().map(String::as_str) else {
    return repl_command(&[]);
  };
  match command {
    "run" => run_command(&args[1..]),
    "test" => test_command(&args[1..]),
    "package" => package_command(&args[1..]),
    "repl" => repl_command(&args[1..]),
    "--help" | "-h" | "help" => {
      usage();
      Ok(0)
    }
    other => {
      eprintln!("unknown command {other}");
      usage();
      Ok(2)
    }
  }
}

fn run_command(args: &[String]) -> Result<u8, yuzuko_core::RuntimeError> {
  let mut entry = None;
  let mut index = 0;
  let mut allow_flag_entry = false;
  let mut watch = false;
  let mut hot = false;
  let mut no_clear = false;
  let mut flags = Vec::new();
  while index < args.len() && entry.is_none() {
    let token = args[index].as_str();
    if token == "--" {
      allow_flag_entry = true;
      index += 1;
      continue;
    }
    if !allow_flag_entry && token.starts_with('-') {
      match token {
        "--watch" => watch = true,
        "--watch-hmr" => hot = true,
        "--no-clear-screen" => no_clear = true,
        token => match classify_flag(token) {
          FlagStep::Flag(flag) => flags.push(flag),
          FlagStep::Bad => return Ok(2),
          FlagStep::Other => {
            eprintln!("unknown flag {token}");
            return Ok(2);
          }
        },
      }
      index += 1;
      continue;
    }
    entry = Some(PathBuf::from(token));
    index += 1;
  }
  if watch && hot {
    eprintln!("--watch and --watch-hmr cannot be combined");
    return Ok(2);
  }
  if no_clear && !watch && !hot {
    eprintln!("--no-clear-screen requires --watch or --watch-hmr");
    return Ok(2);
  }
  let Some(entry) = entry else {
    eprintln!("yuzuko run <entry.ts> [arg...]");
    return Ok(2);
  };
  if args.get(index).map(String::as_str) == Some("--") {
    index += 1;
  }
  set_args(args[index..].to_vec());
  if let Err(error) = install_run(&entry, &flags) {
    eprintln!("{error}");
    return Ok(1);
  }
  configure_window(false, false);
  if watch || hot {
    let mode = if hot {
      WatchMode::Hot
    } else {
      WatchMode::Restart
    };
    return run_file_watch(&entry, mode, !no_clear);
  }
  status_of(run_file(&entry))
}

fn test_command(args: &[String]) -> Result<u8, yuzuko_core::RuntimeError> {
  let mut filter = None;
  let mut paths = Vec::new();
  let mut flags = Vec::new();
  let mut index = 0;
  while index < args.len() {
    match args[index].as_str() {
      "--filter" => {
        index += 1;
        let Some(value) = args.get(index) else {
          eprintln!("--filter requires a value");
          return Ok(2);
        };
        filter = Some(value.clone());
        index += 1;
      }
      "--" => {
        paths.extend(args[index + 1..].iter().map(PathBuf::from));
        break;
      }
      flag if flag.starts_with('-') => match classify_flag(flag) {
        FlagStep::Flag(parsed) => {
          flags.push(parsed);
          index += 1;
        }
        FlagStep::Bad => return Ok(2),
        FlagStep::Other => {
          eprintln!("unknown flag {flag}");
          return Ok(2);
        }
      },
      _ => {
        paths.push(PathBuf::from(&args[index]));
        index += 1;
      }
    }
  }
  set_args(Vec::new());
  if let Err(error) = install_test(&flags) {
    eprintln!("{error}");
    return Ok(1);
  }
  configure_window(true, true);
  match run_tests(&paths, filter.as_deref()) {
    Ok(code) => Ok(u8::try_from(code).unwrap_or(1)),
    Err(error) => match error.exit_code {
      Some(code) => Ok(code),
      None => Err(error),
    },
  }
}

fn repl_command(args: &[String]) -> Result<u8, yuzuko_core::RuntimeError> {
  let mut flags = Vec::new();
  for token in args {
    if !token.starts_with('-') {
      eprintln!("yuzuko repl");
      return Ok(2);
    }
    match classify_flag(token) {
      FlagStep::Flag(flag) => flags.push(flag),
      FlagStep::Bad => return Ok(2),
      FlagStep::Other => {
        eprintln!("yuzuko repl");
        return Ok(2);
      }
    }
  }
  if let Err(error) = install_repl(&flags) {
    eprintln!("{error}");
    return Ok(1);
  }
  set_args(Vec::new());
  configure_window(false, false);
  status_of(run_repl())
}

fn configure_window(test_input: bool, headless: bool) {
  #[cfg(feature = "window")]
  {
    yuzuko_window::set_test_input(test_input);
    yuzuko_window::configure(headless);
  }
  #[cfg(not(feature = "window"))]
  let _ = (test_input, headless);
}

/// The signed bundle. On macOS that is the `.app`. On Windows and Linux it
/// is the directory that holds the executable and `yuzuko.bundle.json`.
#[cfg(feature = "fs")]
fn application_bundle_root(bundle: &yuzuko::BundleLaunch) -> PathBuf {
  let resources = &bundle.resources;
  let contents = resources.parent();
  let macos_app = resources.file_name().and_then(|name| name.to_str())
    == Some("Resources")
    && contents
      .and_then(|path| path.file_name())
      .and_then(|name| name.to_str())
      == Some("Contents");
  if macos_app {
    contents
      .and_then(|path| path.parent())
      .unwrap_or(resources)
      .to_path_buf()
  } else {
    resources.clone()
  }
}

fn install_run(entry: &Path, flags: &[PermissionFlag]) -> Result<(), String> {
  let config = load_for_run(entry).map_err(|error| error.to_string())?;
  if let Some(config) = &config {
    #[cfg(feature = "window")]
    yuzuko_window::set_default_title(&config.name);
  }
  let mut spec = PermissionSpec::default();
  if let Some(value) = config.as_ref().and_then(|item| item.permissions.clone())
  {
    apply_permission_config(&mut spec, &value, false)?;
  }
  let cwd = permission_dir()?;
  apply_permission_flags(&mut spec, flags, &cwd)?;
  spec.prompt = std::io::stdin().is_terminal()
    && !flags
      .iter()
      .any(|flag| matches!(flag, PermissionFlag::NoPrompt));
  let root = config
    .as_ref()
    .map(|item| item.directory.clone())
    .unwrap_or(cwd);
  install_permissions(&spec, &root)
}

fn install_test(flags: &[PermissionFlag]) -> Result<(), String> {
  let mut spec = PermissionSpec {
    prompt: false,
    ..PermissionSpec::default()
  };
  let cwd = permission_dir()?;
  apply_permission_flags(&mut spec, flags, &cwd)?;
  install_permissions(&spec, &cwd)
}

fn install_repl(flags: &[PermissionFlag]) -> Result<(), String> {
  let mut spec = PermissionSpec {
    prompt: false,
    ..PermissionSpec::default()
  };
  let cwd = permission_dir()?;
  apply_permission_flags(&mut spec, flags, &cwd)?;
  install_permissions(&spec, &cwd)
}

enum FlagStep {
  Flag(PermissionFlag),
  Bad,
  Other,
}

fn classify_flag(token: &str) -> FlagStep {
  match parse_permission_flag(token) {
    Some(Ok(flag)) => FlagStep::Flag(flag),
    Some(Err(message)) => {
      eprintln!("{message}");
      FlagStep::Bad
    }
    None => FlagStep::Other,
  }
}

fn package_command(args: &[String]) -> Result<u8, yuzuko_core::RuntimeError> {
  let mut entry = None;
  let mut config = None;
  let mut output = None;
  let mut index = 0;
  while index < args.len() {
    match args[index].as_str() {
      "--config" => {
        index += 1;
        let Some(value) = args.get(index) else {
          eprintln!("--config requires a value");
          return Ok(2);
        };
        config = Some(PathBuf::from(value));
        index += 1;
      }
      "--output" => {
        index += 1;
        let Some(value) = args.get(index) else {
          eprintln!("--output requires a value");
          return Ok(2);
        };
        output = Some(PathBuf::from(value));
        index += 1;
      }
      "--" => {
        index += 1;
        if entry.is_none() && index < args.len() {
          entry = Some(PathBuf::from(&args[index]));
        }
        break;
      }
      flag if flag.starts_with('-') => {
        eprintln!("unknown flag {flag}");
        return Ok(2);
      }
      _ => {
        if entry.is_some() {
          eprintln!("yuzuko package accepts one entry file");
          return Ok(2);
        }
        entry = Some(PathBuf::from(&args[index]));
        index += 1;
      }
    }
  }
  let Some(entry) = entry else {
    eprintln!("yuzuko package needs an entry path.");
    return Ok(1);
  };
  let loaded = load_package(&entry, config.as_deref());
  let loaded = match loaded {
    Ok(config) => config,
    Err(error) => {
      eprintln!("{error}");
      return Ok(1);
    }
  };
  let host = if cfg!(target_os = "macos") {
    Target::Macos
  } else if cfg!(target_os = "windows") {
    Target::Windows
  } else {
    Target::Linux
  };
  let output = if let Some(output) = output {
    if let Err(error) = reject_package_extension(&output.to_string_lossy()) {
      eprintln!("{error}");
      return Ok(1);
    }
    if matches!(host, Target::Macos)
      && !output
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
    {
      eprintln!("--output must end in .app.");
      return Ok(1);
    }
    output
  } else {
    let configured = match host {
      Target::Macos => loaded.output.macos.clone(),
      Target::Windows => loaded.output.windows.clone(),
      Target::Linux => loaded.output.linux.clone(),
    };
    let Some(output) = configured else {
      eprintln!("yuzuko package needs --output or an output path.");
      return Ok(1);
    };
    output
  };
  let runtime = match std::env::current_exe() {
    Ok(path) => path,
    Err(error) => {
      eprintln!("current executable: {error}");
      return Ok(1);
    }
  };
  match package(&loaded, host, &runtime, &output) {
    Ok(()) => Ok(0),
    Err(error) => {
      eprintln!("{error}");
      Ok(1)
    }
  }
}

fn usage() {
  eprintln!(
    "\
yuzuko run <entry.ts> [arg...]
yuzuko run --watch [--no-clear-screen] <entry.ts> [arg...]
yuzuko run --watch-hmr [--no-clear-screen] <entry.ts> [arg...]
yuzuko test [<path>...] [--filter <substring>]
yuzuko package <entry.ts> [--config <file>] [--output <path>]
yuzuko repl

Permission flags, before the entry: -A, --allow-all,
--allow-read[=<path>...] (-R), --allow-write[=<path>...] (-W),
--allow-net[=<host>...] (-N), --allow-env[=<name>...] (-E),
--allow-run[=<command>...], --allow-ffi[=<path>...],
and the matching --deny-* flags. --no-prompt skips the question."
  );
}
