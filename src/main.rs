mod mermaid;
mod doc;

use std::path::PathBuf;

pub struct Options {
    #[allow(dead_code)]
    path: PathBuf,
    #[allow(dead_code)]
    dark: bool,
}

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    const USAGE: &str = "usage: mdview [--dark] <file.md>";
    let mut dark = false;
    let mut path: Option<PathBuf> = None;
    for arg in args {
        match arg.as_str() {
            "--dark" => dark = true,
            "" => return Err(USAGE.to_string()),
            s => {
                if path.is_some() {
                    return Err(USAGE.to_string());
                }
                path = Some(PathBuf::from(s));
            }
        }
    }
    path.map(|path| Options { path, dark })
        .ok_or_else(|| USAGE.to_string())
}

#[allow(dead_code)] // scaffold: window shell reads opts in later tasks
struct App {
    opts: Options,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |_ui| {});
    }
}

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    eframe::run_native(
        "mdview",
        eframe::NativeOptions::default(),
        Box::new(move |_cc| Ok(Box::new(App { opts }))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn parses_path_and_dark_flag() {
        let o = parse_args(&a(&["--dark", "doc.md"])).unwrap();
        assert_eq!(o.path, PathBuf::from("doc.md"));
        assert!(o.dark);
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
