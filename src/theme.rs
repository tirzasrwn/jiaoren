use std::sync::LazyLock;

use crate::mermaid::Theme;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

impl From<Rgb> for egui::Color32 {
    fn from(c: Rgb) -> Self {
        egui::Color32::from_rgb(c.0, c.1, c.2)
    }
}

// Some roles (subtext, overlay, accent) are only consumed by the mermaid
// theme variables; keep the full Catppuccin set here as the single source.
#[allow(dead_code)]
pub struct Palette {
    pub base: Rgb,
    pub mantle: Rgb,
    pub crust: Rgb,
    pub text: Rgb,
    pub subtext1: Rgb,
    pub subtext0: Rgb,
    pub overlay2: Rgb,
    pub overlay1: Rgb,
    pub overlay0: Rgb,
    pub surface2: Rgb,
    pub surface1: Rgb,
    pub surface0: Rgb,
    pub blue: Rgb,
    pub lavender: Rgb,
    pub yellow: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub peach: Rgb,
    pub mauve: Rgb,
    pub teal: Rgb,
}

pub const LATTE: Palette = Palette {
    base: Rgb(0xef, 0xf1, 0xf5),
    mantle: Rgb(0xe6, 0xe9, 0xef),
    crust: Rgb(0xdc, 0xe0, 0xe8),
    text: Rgb(0x4c, 0x4f, 0x69),
    subtext1: Rgb(0x5c, 0x5f, 0x77),
    subtext0: Rgb(0x6c, 0x6f, 0x85),
    overlay2: Rgb(0x7c, 0x7f, 0x93),
    overlay1: Rgb(0x8c, 0x8f, 0xa1),
    overlay0: Rgb(0x9c, 0xa0, 0xb0),
    surface2: Rgb(0xac, 0xb0, 0xbe),
    surface1: Rgb(0xbc, 0xc0, 0xcc),
    surface0: Rgb(0xcc, 0xd0, 0xda),
    blue: Rgb(0x1e, 0x66, 0xf5),
    lavender: Rgb(0x72, 0x87, 0xfd),
    yellow: Rgb(0xdf, 0x8e, 0x1d),
    red: Rgb(0xd2, 0x0f, 0x39),
    green: Rgb(0x40, 0xa0, 0x2b),
    peach: Rgb(0xfe, 0x64, 0x0b),
    mauve: Rgb(0x88, 0x39, 0xef),
    teal: Rgb(0x17, 0x92, 0x99),
};

pub const MACCHIATO: Palette = Palette {
    base: Rgb(0x24, 0x27, 0x3a),
    mantle: Rgb(0x1e, 0x20, 0x30),
    crust: Rgb(0x18, 0x19, 0x26),
    text: Rgb(0xca, 0xd3, 0xf5),
    subtext1: Rgb(0xb8, 0xc0, 0xe0),
    subtext0: Rgb(0xa5, 0xad, 0xcb),
    overlay2: Rgb(0x93, 0x9a, 0xb7),
    overlay1: Rgb(0x80, 0x87, 0xa2),
    overlay0: Rgb(0x6e, 0x73, 0x8d),
    surface2: Rgb(0x5b, 0x60, 0x78),
    surface1: Rgb(0x49, 0x4d, 0x64),
    surface0: Rgb(0x36, 0x3a, 0x4f),
    blue: Rgb(0x8a, 0xad, 0xf4),
    lavender: Rgb(0xb7, 0xbd, 0xf8),
    yellow: Rgb(0xee, 0xd4, 0x9f),
    red: Rgb(0xed, 0x87, 0x96),
    green: Rgb(0xa6, 0xda, 0x95),
    peach: Rgb(0xf5, 0xa9, 0x7f),
    mauve: Rgb(0xc6, 0xa0, 0xf6),
    teal: Rgb(0x8b, 0xd5, 0xca),
};

pub fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Light => &LATTE,
        Theme::Dark => &MACCHIATO,
    }
}

pub fn visuals(theme: Theme) -> egui::Visuals {
    let p = palette(theme);
    let dark = matches!(theme, Theme::Dark);
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.dark_mode = dark;
    v.panel_fill = p.base.into();
    v.window_fill = p.base.into();
    v.faint_bg_color = p.mantle.into();
    v.extreme_bg_color = p.crust.into();
    v.code_bg_color = p.crust.into();
    v.text_edit_bg_color = Some(p.mantle.into());
    v.hyperlink_color = p.blue.into();
    v.warn_fg_color = p.yellow.into();
    v.error_fg_color = p.red.into();
    v.selection.bg_fill =
        egui::Color32::from_rgba_unmultiplied(p.lavender.0, p.lavender.1, p.lavender.2, 0x59);
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from(p.lavender));
    let stroke = egui::Stroke::new(1.0, egui::Color32::from(p.overlay0));
    v.widgets.noninteractive.bg_stroke = stroke;
    v.widgets.inactive.bg_stroke = stroke;
    v.widgets.hovered.bg_stroke = stroke;
    v.widgets.active.bg_stroke = stroke;
    v.widgets.noninteractive.bg_fill = p.surface0.into();
    v.widgets.noninteractive.weak_bg_fill = p.surface0.into();
    v.widgets.noninteractive.fg_stroke.color = p.text.into();
    v.widgets.inactive.bg_fill = p.surface0.into();
    v.widgets.inactive.weak_bg_fill = p.surface0.into();
    v.widgets.hovered.bg_fill = p.surface1.into();
    v.widgets.hovered.weak_bg_fill = p.surface1.into();
    v.widgets.active.bg_fill = p.surface2.into();
    v.widgets.active.weak_bg_fill = p.surface2.into();
    v
}

/// Mermaid `themeVariables` fragment (no outer braces) keyed to the palette.
static MERMAID_VARS: LazyLock<[String; 2]> =
    LazyLock::new(|| [mermaid_vars(&LATTE), mermaid_vars(&MACCHIATO)]);

fn mermaid_vars(p: &Palette) -> String {
    let h = |c: Rgb| c.to_hex();
    format!(
        "background:\"{}\",primaryColor:\"{}\",primaryTextColor:\"{}\",primaryBorderColor:\"{}\",lineColor:\"{}\",secondaryColor:\"{}\",tertiaryColor:\"{}\",textColor:\"{}\",mainBkg:\"{}\",nodeBorder:\"{}\",clusterBkg:\"{}\",clusterBorder:\"{}\",titleColor:\"{}\",edgeLabelBackground:\"{}\",actorBkg:\"{}\",actorBorder:\"{}\",actorTextColor:\"{}\",signalColor:\"{}\",signalTextColor:\"{}\",labelBoxBkgColor:\"{}\",labelBoxBorderColor:\"{}\",noteBkgColor:\"{}\",noteTextColor:\"{}\",noteBorderColor:\"{}\"",
        h(p.base), h(p.surface0), h(p.text), h(p.overlay1), h(p.overlay2),
        h(p.mantle), h(p.crust), h(p.text), h(p.surface0), h(p.overlay1),
        h(p.mantle), h(p.surface1), h(p.text), h(p.base), h(p.surface0),
        h(p.overlay1), h(p.text), h(p.text), h(p.text), h(p.surface0),
        h(p.overlay1), h(p.yellow), h(p.base), h(p.yellow),
    )
}

pub fn mermaid_variables(theme: Theme) -> &'static str {
    match theme {
        Theme::Light => MERMAID_VARS[0].as_str(),
        Theme::Dark => MERMAID_VARS[1].as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_carry_the_documented_base_and_text() {
        assert_eq!(palette(Theme::Light).base.0, 0xef);
        assert_eq!(palette(Theme::Light).base.to_hex(), "#eff1f5");
        assert_eq!(palette(Theme::Dark).base.to_hex(), "#24273a");
        assert_eq!(palette(Theme::Dark).text.to_hex(), "#cad3f5");
    }

    #[test]
    fn visuals_are_light_for_latte_and_dark_for_macchiato() {
        let l = visuals(Theme::Light);
        assert!(!l.dark_mode);
        assert_eq!(l.panel_fill, palette(Theme::Light).base.into());
        let d = visuals(Theme::Dark);
        assert!(d.dark_mode);
        assert_eq!(d.panel_fill, palette(Theme::Dark).base.into());
    }

    #[test]
    fn mermaid_variables_use_their_palette_and_differ() {
        let l = mermaid_variables(Theme::Light);
        let d = mermaid_variables(Theme::Dark);
        assert!(l.contains(&palette(Theme::Light).base.to_hex()));
        assert!(l.contains(&palette(Theme::Light).text.to_hex()));
        assert!(d.contains(&palette(Theme::Dark).base.to_hex()));
        assert!(d.contains(&palette(Theme::Dark).text.to_hex()));
        assert_ne!(l, d);
    }
}
