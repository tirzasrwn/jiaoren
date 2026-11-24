//! Command-line entry point: parse arguments, validate the input file, and
//! start the eframe window.

mod app;
mod doc;
mod download;
mod fonts;
mod justify;
mod mermaid;
mod prefs;
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
    /// Print the keyboard shortcuts and exit 0.
    Keys,
    /// Delete cached diagram renders; `all` also removes the managed Chromium.
    CleanCache { all: bool },
}

const USAGE: &str = "usage: jr [--dark] <file.md>";

/// The keyboard shortcuts, one per line. Shared by `jr --keys` and the `?`
/// overlay so the two can never drift apart.
pub const KEYS_HELP: &str = "\
j / ↓            scroll down one line
k / ↑            scroll up one line
d / u            scroll half a page down / up
space / b        scroll a page down / up
gg / G           jump to the top / bottom
h / l            pan left / right (shift+←/→ too)
+ / = / -        zoom in / out
a / 0            reset zoom (a also pans back left)
t                toggle light/dark theme
r                reload from disk
] / [            next / previous Markdown file in this folder
o                open the link under the pointer in the browser
ctrl+g           show the file path
drag             select text (releasing copies; on empty space it pans)
y                copy the selection, yy copies the whole file
/                search, Enter confirms
n / N            next / previous match
tab              heading index (depth 3): j/k move, h/l fold, enter jumps
?                this list (esc, q or ctrl+c closes it)
esc / ctrl+c     copy selection, else cancel prompt, else clear search, else quit
q                quit";

/// The text printed by `jr --help`.
pub const HELP: &str = concat!(
    "jiaoren ",
    env!("CARGO_PKG_VERSION"),
    " — a zathura-like Markdown viewer with mermaid diagrams

Usage: jr [OPTIONS] <FILE>

Options:
      --dark         start with the dark theme
      --clean-cache  delete cached diagram renders and exit
      --all          with --clean-cache, also delete Chromium and cached fonts
      --keys         print the keyboard shortcuts and exit
  -h, --help         print this help and exit
  -V, --version      print version information and exit

Keys: jr --keys, or press ? in the viewer."
);

/// One-line-per-component version output for `jr --version`.
pub fn version_text() -> String {
    // The managed build is the default; an explicit JIAOREN_CHROMIUM override
    // is what actually runs, so report the override as well.
    let chromium = match std::env::var("JIAOREN_CHROMIUM") {
        Ok(path) => format!("{} (override: {path})", mermaid::MANAGED_VERSION),
        Err(_) => format!("{} (managed)", mermaid::MANAGED_VERSION),
    };
    format!(
        "{} {}\nmermaid {}\nchromium {}\nfont {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        mermaid::MERMAID_VERSION,
        chromium,
        fonts::version_line(&mermaid::cache_root())
    )
}

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
    if args.iter().any(|a| a == "--keys") {
        let other = args.iter().any(|a| a != "--keys");
        if other {
            return Err(USAGE.to_string());
        }
        return Ok(Command::Keys);
    }
    if args.iter().any(|a| a == "--clean-cache") {
        let all = args.iter().any(|a| a == "--all");
        let other = args.iter().any(|a| a != "--clean-cache" && a != "--all");
        if other {
            return Err(USAGE.to_string());
        }
        return Ok(Command::CleanCache { all });
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
            println!("{}", version_text());
            return Ok(());
        }
        Ok(Command::Keys) => {
            println!("{KEYS_HELP}");
            return Ok(());
        }
        Ok(Command::CleanCache { all }) => {
            match mermaid::clean_cache(all) {
                Ok((bytes, files)) => {
                    println!("removed {files} cache entries ({} MB)", bytes / 1_000_000);
                    if !all {
                        println!("kept Chromium and fonts; add --all to remove them too");
                    }
                }
                Err(e) => {
                    eprintln!("clean cache: {e}");
                    std::process::exit(1);
                }
            }
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
        fn clean_cache_flag_parses_with_optional_all() {
            assert_eq!(
                parse_args(&a(&["--clean-cache"])).unwrap(),
                Command::CleanCache { all: false }
            );
            assert_eq!(
                parse_args(&a(&["--clean-cache", "--all"])).unwrap(),
                Command::CleanCache { all: true }
            );
            assert!(parse_args(&a(&["--clean-cache", "doc.md"])).is_err());
            assert!(parse_args(&a(&["--all"])).is_err());
        }

        #[test]
        fn version_text_names_mermaid_and_chromium() {
            let text = version_text();
            assert!(text.contains(&format!("jiaoren {}", env!("CARGO_PKG_VERSION"))));
            assert!(text.contains(&format!("mermaid {}", crate::mermaid::MERMAID_VERSION)));
            assert!(text.contains(&format!("chromium {}", crate::mermaid::MANAGED_VERSION)));
            assert!(text.contains("(managed)") || text.contains("(override:"));
        }

        #[test]
        fn version_text_names_the_font() {
            let text = version_text();
            assert!(
                text.lines().any(|l| l.starts_with("font ")),
                "font line missing: {text}"
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
        fn keys_flag_returns_keys_and_takes_nothing_else() {
            assert_eq!(parse_args(&a(&["--keys"])).unwrap(), Command::Keys);
            assert!(parse_args(&a(&["--keys", "doc.md"])).is_err());
            assert!(KEYS_HELP.lines().any(|l| l.starts_with("q")));
            assert!(HELP.contains("--keys"));
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
