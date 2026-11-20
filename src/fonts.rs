//! Locate and install the FiraCode Nerd Font faces used by the window.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Font family name as reported by `fc-list`; reused by the mermaid `fontFamily`
/// setting so diagrams and the window chrome agree.
pub const FAMILY: &str = "FiraCode Nerd Font";

const REGULAR: &str = "FiraCodeNerdFont-Regular";
const BOLD: &str = "FiraCodeNerdFont-Bold";

/// Load FiraCode Nerd Font from the first user font directory that has the
/// Regular face. Returns `None` (keeping egui defaults) when no copy is found,
/// so a missing font never prevents the app from starting.
pub fn load() -> Option<egui::FontDefinitions> {
    candidate_dirs().into_iter().find_map(|dir| load_from_dir(&dir))
}

/// Search order: `~/.fonts`, `$XDG_DATA_HOME/fonts`, `~/.local/share/fonts`,
/// `/usr/share/fonts/truetype`.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("HOME");
    if let Some(h) = &home {
        dirs.push(PathBuf::from(h).join(".fonts"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(xdg).join("fonts"));
    }
    if let Some(h) = &home {
        dirs.push(PathBuf::from(h).join(".local/share/fonts"));
    }
    dirs.push(PathBuf::from("/usr/share/fonts/truetype"));
    dirs
}

/// Build [`egui::FontDefinitions`] from the Regular (and, if present, Bold)
/// faces in `dir`. FiraCode becomes the primary for both `Proportional` and
/// `Monospace`; the built-in egui fonts stay as fallbacks. `None` when the
/// Regular face is missing or unreadable.
pub fn load_from_dir(dir: &Path) -> Option<egui::FontDefinitions> {
    let regular = std::fs::read(dir.join(format!("{REGULAR}.ttf"))).ok()?;
    let bold = std::fs::read(dir.join(format!("{BOLD}.ttf"))).ok();

    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert(
        REGULAR.to_owned(),
        Arc::new(egui::FontData::from_owned(regular)),
    );
    // List the Bold face so bold text renders as real bold rather than a
    // synthetic weight.
    let mut faces = vec![REGULAR.to_owned()];
    if let Some(b) = bold {
        defs.font_data
            .insert(BOLD.to_owned(), Arc::new(egui::FontData::from_owned(b)));
        faces.push(BOLD.to_owned());
    }

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = defs.families.entry(family).or_default();
        let defaults = std::mem::take(list);
        list.extend(faces.iter().cloned());
        list.extend(defaults);
    }
    Some(defs)
}

/// Install FiraCode once at startup. A no-op when the font is not installed,
/// leaving egui's defaults in place.
pub fn install(ctx: &egui::Context) {
    if let Some(defs) = load() {
        ctx.set_fonts(defs);
    }
}

#[cfg(test)]
mod tests {
    mod fonts {
        use super::super::*;

        #[test]
        fn finds_the_regular_face_on_this_machine() {
            let defs = load().expect("FiraCode Nerd Font is installed under ~/.fonts");
            assert!(defs.font_data.keys().any(|k| k.contains("FiraCode")));
            assert_eq!(
                defs.families[&egui::FontFamily::Proportional]
                    .first()
                    .unwrap(),
                "FiraCodeNerdFont-Regular"
            );
            assert_eq!(
                defs.families[&egui::FontFamily::Monospace]
                    .first()
                    .unwrap(),
                "FiraCodeNerdFont-Regular"
            );
        }

        #[test]
        fn missing_files_return_none_instead_of_panicking() {
            assert!(load_from_dir(std::path::Path::new("/nonexistent/fonts")).is_none());
        }

        #[test]
        fn bold_face_is_listed_after_regular() {
            let defs = load().expect("font installed");
            let mono = &defs.families[&egui::FontFamily::Monospace];
            assert_eq!(mono[0], "FiraCodeNerdFont-Regular");
            assert_eq!(mono[1], "FiraCodeNerdFont-Bold");
        }
    }
}
