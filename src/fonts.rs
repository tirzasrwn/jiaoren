//! Locate and install the FiraCode Nerd Font faces used by the window.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Font family name as reported by `fc-list`; reused by the mermaid `fontFamily`
/// setting so diagrams and the window chrome agree.
pub const FAMILY: &str = "FiraCode Nerd Font";

const REGULAR: &str = "FiraCodeNerdFont-Regular";
const BOLD: &str = "FiraCodeNerdFont-Bold";

/// Family holding only the Bold face, so `**strong**` runs can ask for a real
/// bold weight. egui has no per-run weight flag: a face listed as a *fallback*
/// is only used for missing glyphs, so bold text needs its own family.
pub const BOLD_FAMILY: &str = "FiraCode Bold";

/// System fallbacks for scripts FiraCode lacks (CJK, Arabic). Loaded when
/// present so proxied documents do not render tofu; missing files simply keep
/// the egui defaults.
const FALLBACK_FILES: &[&str] = &[
    "NotoSansCJK-Regular.ttc",
    "NotoSansArabic-Regular.ttf",
    "NotoNaskhArabic-Regular.ttf",
];
const FALLBACK_DIRS: &[&str] = &[
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/truetype",
];

/// The font id for bold text at `size`.
pub fn bold_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD_FAMILY.into()))
}

/// Whether `font` asks for the bold family.
#[cfg(test)]
pub fn is_bold(font: &egui::FontId) -> bool {
    matches!(&font.family, egui::FontFamily::Name(name) if &**name == BOLD_FAMILY)
}

/// Load FiraCode Nerd Font from the first user font directory that has the
/// Regular face. Returns `None` (keeping egui defaults) when no copy is found,
/// so a missing font never prevents the app from starting.
pub fn load() -> Option<egui::FontDefinitions> {
    candidate_dirs()
        .into_iter()
        .find_map(|dir| load_from_dir(&dir))
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
    // List the Bold face as its own family so bold runs get a real bold
    // weight rather than the Regular face plus a darker colour.
    let faces = [REGULAR.to_owned()];
    // Registered even without the Bold file, so `bold_font` always resolves;
    // it then falls back to Regular.
    let mut bold_family = Vec::new();
    if let Some(b) = bold {
        defs.font_data
            .insert(BOLD.to_owned(), Arc::new(egui::FontData::from_owned(b)));
        bold_family.push(BOLD.to_owned());
    }
    // Script fallbacks (CJK, Arabic) sit after FiraCode so missing glyphs
    // resolve instead of rendering tofu; absent files keep egui's defaults.
    let mut fallbacks: Vec<String> = Vec::new();
    for file in FALLBACK_FILES {
        let Some(path) = FALLBACK_DIRS
            .iter()
            .map(|dir| Path::new(dir).join(file))
            .find(|path| path.is_file())
        else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = file.trim_end_matches(".ttc").trim_end_matches(".ttf");
        defs.font_data
            .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        fallbacks.push(name.to_owned());
    }
    bold_family.push(REGULAR.to_owned());
    bold_family.extend(fallbacks.iter().cloned());
    bold_family.extend(
        defs.families[&egui::FontFamily::Proportional]
            .iter()
            .cloned(),
    );
    defs.families
        .insert(egui::FontFamily::Name(BOLD_FAMILY.into()), bold_family);

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = defs.families.entry(family).or_default();
        let defaults = std::mem::take(list);
        list.extend(faces.iter().cloned());
        list.extend(fallbacks.iter().cloned());
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
                defs.families[&egui::FontFamily::Monospace].first().unwrap(),
                "FiraCodeNerdFont-Regular"
            );
        }

        #[test]
        fn fallbacks_cover_cjk_and_arabic() {
            let defs = load().expect("font installed");
            let proportional = &defs.families[&egui::FontFamily::Proportional];
            assert!(
                proportional.iter().any(|name| name.contains("CJK")),
                "CJK fallback missing: {proportional:?}"
            );
            assert!(
                proportional.iter().any(|name| name.contains("Arabic")),
                "Arabic fallback missing: {proportional:?}"
            );
        }

        #[test]
        fn missing_files_return_none_instead_of_panicking() {
            assert!(load_from_dir(std::path::Path::new("/nonexistent/fonts")).is_none());
        }

        #[test]
        fn bold_face_has_its_own_family_after_regular() {
            let defs = load().expect("font installed");
            let mono = &defs.families[&egui::FontFamily::Monospace];
            assert_eq!(mono[0], "FiraCodeNerdFont-Regular");
            let bold = &defs.families[&egui::FontFamily::Name(BOLD_FAMILY.into())];
            assert_eq!(bold[0], "FiraCodeNerdFont-Bold");
            assert_eq!(bold[1], "FiraCodeNerdFont-Regular");
        }

        #[test]
        fn bold_font_selects_the_bold_family() {
            assert!(is_bold(&bold_font(14.0)));
            assert!(!is_bold(&egui::FontId::proportional(14.0)));
            assert!(!is_bold(&egui::FontId::monospace(14.0)));
        }
    }
}
