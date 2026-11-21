//! Command-line entry point: parse arguments, validate the input file, and
//! start the eframe window.

mod app;
mod doc;
mod fonts;
mod justify;
mod mermaid;
mod search;
mod theme;

use std::path::PathBuf;

/// Parsed command-line options.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    path: PathBuf,
    dark: bool,
}

/// A parsed command line: run the viewer, or print help/version and exit.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Open the viewer with the given options.
    Run(Options),
    /// Print the usage text and exit 0.
    Help,
    /// Print version information and exit 0.
    Version,
}

const USAGE: &str = "usage: jr [--dark] <file.md>";

/// The text printed by `jr --help`.
pub const HELP: &str = concat!(
    "jiaoren ",
    env!("CARGO_PKG_VERSION"),
    " — a zathura-like Markdown viewer with mermaid diagrams

Usage: jr [OPTIONS] <FILE>

Options:
      --dark     start with the dark theme
  -h, --help     print this help and exit
  -V, --version  print version information and exit

Keys: see the README (j/k, d/u, gg/G, +/-/a/0, t, r, q)."
);

/// Parse the command line. `--help` and `--version` win over other arguments.
/// Any usage error returns the message the caller prints before exiting with
/// status 2.
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Command::Help);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(Command::Version);
    }
    let mut dark = false;
    let mut path: Option<PathBuf> = None;
    for arg in args {
        match arg.as_str() {
            "--dark" => dark = true,
            "" => return Err(USAGE.to_string()),
            s if s.starts_with('-') && s != "-" => return Err(USAGE.to_string()),
            s => {
                if path.is_some() {
                    return Err(USAGE.to_string());
                }
                path = Some(PathBuf::from(s));
            }
        }
    }
    match path {
        Some(path) => Ok(Command::Run(Options { path, dark })),
        None => Err(USAGE.to_string()),
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(Command::Run(o)) => o,
        Ok(Command::Help) => {
            println!("{HELP}");
            return Ok(());
        }
        Ok(Command::Version) => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    // Read once, before any window exists; the app receives the text.
    let text = match doc::read_doc(&opts.path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    eframe::run_native(
        "jiaoren",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::App::new(
                opts.path,
                text,
                opts.dark,
                &cc.egui_ctx,
            )))
        }),
    )
    // `eframe::Error` is not `Sync`, so it cannot convert via `?`; the display
    // text is all the boundary needs.
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    mod parse_args {
        use super::super::*;

        fn a(v: &[&str]) -> Vec<String> {
            v.iter().map(|s| s.to_string()).collect()
        }

        #[test]
        fn parses_path_and_dark_flag() {
            let c = parse_args(&a(&["--dark", "doc.md"])).unwrap();
            assert_eq!(
                c,
                Command::Run(Options {
                    path: PathBuf::from("doc.md"),
                    dark: true,
                })
            );
        }

        #[test]
        fn long_help_returns_help() {
            assert_eq!(parse_args(&a(&["--help"])).unwrap(), Command::Help);
        }

        #[test]
        fn short_help_returns_help() {
            assert_eq!(parse_args(&a(&["-h"])).unwrap(), Command::Help);
        }

        #[test]
        fn long_version_returns_version() {
            assert_eq!(parse_args(&a(&["--version"])).unwrap(), Command::Version);
        }

        #[test]
        fn short_version_returns_version() {
            assert_eq!(parse_args(&a(&["-V"])).unwrap(), Command::Version);
        }

        #[test]
        fn help_wins_over_missing_file() {
            assert_eq!(parse_args(&a(&["--help"])).unwrap(), Command::Help);
        }

        #[test]
        fn help_wins_over_other_args() {
            assert_eq!(
                parse_args(&a(&["--dark", "--help", "a.md"])).unwrap(),
                Command::Help
            );
        }

        #[test]
        fn unknown_flag_is_an_error() {
            assert!(parse_args(&a(&["--nope"])).is_err());
        }

        #[test]
        fn rejects_missing_arg() {
            assert!(parse_args(&a(&[])).is_err());
        }

        #[test]
        fn rejects_two_paths() {
            assert!(parse_args(&a(&["a.md", "b.md"])).is_err());
        }
    }
}
