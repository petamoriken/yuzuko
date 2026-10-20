use std::path::{Path, PathBuf};
use std::process::ExitCode;

use yuzuko::{
  ConfigError, Target, bundled_launch, load_for_run, load_package, package,
  reject_package_extension,
};
use yuzuko_core::{run_file, run_file_sandboxed, run_repl, run_tests};

fn main() -> ExitCode {
  yuzuko_dom::register();
  yuzuko_winter::register();
  yuzuko_window::register();
  yuzuko_image::register();
  yuzuko_codec::register();
  yuzuko_canvas::register();
  yuzuko_webgpu::register();
  yuzuko_audio::register();
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  if bundled_args(&args)
    && let Some(bundle) = bundled_launch()
  {
    yuzuko_window::set_test_input(false);
    yuzuko_window::configure(false);
    yuzuko_window::set_default_title(&bundle.name);
    if let Some(identifier) = &bundle.identifier {
      yuzuko_window::set_app_identity(identifier, &bundle.resources);
    }
    yuzuko::enable_updater(bundle.resources);
    let code = match run_file_sandboxed(&bundle.entry, Some(&bundle.sandbox)) {
      Ok(()) => 0,
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
  while index < args.len() {
    match args[index].as_str() {
      "--" => {
        index += 1;
        break;
      }
      flag if flag.starts_with('-') => {
        eprintln!("unknown flag {flag}");
        return Ok(2);
      }
      _ => {
        if entry.is_some() {
          eprintln!("yuzuko run accepts one entry file");
          return Ok(2);
        }
        entry = Some(PathBuf::from(&args[index]));
        index += 1;
      }
    }
  }
  if index < args.len() && entry.is_none() {
    entry = Some(PathBuf::from(&args[index]));
  }
  let Some(entry) = entry else {
    eprintln!("yuzuko run <entry.ts>");
    return Ok(2);
  };
  if let Err(error) = apply_run_title(&entry) {
    eprintln!("{error}");
    return Ok(1);
  }
  yuzuko_window::set_test_input(false);
  yuzuko_window::configure(false);
  match run_file(&entry) {
    Ok(()) => Ok(0),
    Err(error) => {
      eprintln!("{error}");
      Ok(1)
    }
  }
}

fn test_command(args: &[String]) -> Result<u8, yuzuko_core::RuntimeError> {
  let mut filter = None;
  let mut paths = Vec::new();
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
      flag if flag.starts_with('-') => {
        eprintln!("unknown flag {flag}");
        return Ok(2);
      }
      _ => {
        paths.push(PathBuf::from(&args[index]));
        index += 1;
      }
    }
  }
  yuzuko_window::set_test_input(true);
  yuzuko_window::configure(true);
  run_tests(&paths, filter.as_deref())
    .map(|code| u8::try_from(code).unwrap_or(1))
}

fn repl_command(args: &[String]) -> Result<u8, yuzuko_core::RuntimeError> {
  if !args.is_empty() {
    eprintln!("yuzuko repl");
    return Ok(2);
  }
  yuzuko_window::set_test_input(false);
  yuzuko_window::configure(false);
  match run_repl() {
    Ok(()) => Ok(0),
    Err(error) => {
      eprintln!("{error}");
      Ok(1)
    }
  }
}

fn bundled_args(args: &[String]) -> bool {
  args.iter().all(|arg| arg.starts_with("-psn_"))
}

fn apply_run_title(entry: &Path) -> Result<(), ConfigError> {
  if let Some(config) = load_for_run(entry)? {
    yuzuko_window::set_default_title(&config.name);
  }
  Ok(())
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
yuzuko run <entry.ts>
yuzuko test [<path>...] [--filter <substring>]
yuzuko package <entry.ts> [--config <file>] [--output <path>]
yuzuko repl"
  );
}
