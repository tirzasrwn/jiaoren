//! SVG rasterization for diagrams: turn a cached SVG file into a premultiplied
//! RGBA image on a worker thread, and the state a raster has on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use super::geometry::RasterKey;

/// State of one raster in [`super::diagram::DiagramUi`]. `Ready` holds the GPU
/// texture; the UI thread is the only place a texture is created or dropped.
pub(crate) enum Raster {
    Ready(egui::TextureHandle),
    Pending,
    Failed(String),
}

/// A rasterization job handed to a worker thread.
pub(crate) struct RasterJob {
    pub(crate) key: RasterKey,
    pub(crate) path: PathBuf,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) scale: f32,
}

/// Longest side the GPU texture accepts; egui rejects (and panics on) larger
/// images. Huge diagrams are scaled down instead of uploaded at full size.
pub(crate) const MAX_TEXTURE_SIDE: u32 = 16384;

/// Total RGBA pixels allowed per raster (32 MiB at 4 bytes per pixel). The side
/// cap alone still allows a 16384×16384 = 1 GiB texture.
pub(crate) const MAX_TEXTURE_PIXELS: f32 = 32_000_000.0;

/// `scale` reduced so neither side exceeds [`MAX_TEXTURE_SIDE`] and the pixel
/// budget is respected.
pub(crate) fn capped_scale(w: f32, h: f32, scale: f32) -> f32 {
    let longest = w.max(h) * scale;
    let mut scale = if longest <= MAX_TEXTURE_SIDE as f32 {
        scale
    } else {
        scale * (MAX_TEXTURE_SIDE as f32 / longest)
    };
    let pixels = w * h * scale * scale;
    if pixels > MAX_TEXTURE_PIXELS {
        scale *= (MAX_TEXTURE_PIXELS / pixels).sqrt();
    }
    scale
}

/// Some SVGs (Mermaid C4/class links) use `xlink:href` without declaring the
/// `xlink` namespace, which `usvg` rejects. Declare it on the root `<svg>`.
/// Invalid UTF-8 is returned unchanged, so `usvg` reports the real cause.
fn ensure_xlink_namespace(bytes: Vec<u8>) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return bytes;
    };
    if !text.contains("xlink:") || text.contains("xmlns:xlink") {
        return bytes;
    }
    let Some(start) = text.find("<svg") else {
        return bytes;
    };
    let Some(offset) = text[start..].find('>') else {
        return bytes;
    };
    let at = start + offset;
    let mut out = String::with_capacity(bytes.len() + 44);
    out.push_str(&text[..at]);
    out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
    out.push_str(&text[at..]);
    out.into_bytes()
}

/// `usvg` options built once with system fonts loaded; without them `<text>`
/// labels vanish from every diagram.
fn svg_options() -> &'static resvg::usvg::Options<'static> {
    static OPTS: LazyLock<resvg::usvg::Options<'static>> = LazyLock::new(|| {
        let mut opts = resvg::usvg::Options::default();
        opts.fontdb_mut().load_system_fonts();
        opts
    });
    &OPTS
}

/// Rasterize `path` at `scale` into a premultiplied RGBA image. Runs on a
/// worker thread; never touches egui state.
pub(crate) fn rasterize(
    path: &Path,
    w: f32,
    h: f32,
    scale: f32,
) -> Result<egui::ColorImage, String> {
    let bytes = ensure_xlink_namespace(std::fs::read(path).map_err(|e| format!("read svg: {e}"))?);
    let tree = resvg::usvg::Tree::from_data(&bytes, svg_options())
        .map_err(|e| format!("parse svg: {e}"))?;
    let scale = capped_scale(w, h, scale);
    let pw = ((w * scale).ceil().max(1.0) as u32).min(MAX_TEXTURE_SIDE);
    let ph = ((h * scale).ceil().max(1.0) as u32).min(MAX_TEXTURE_SIDE);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pw, ph)
        .ok_or_else(|| format!("bad raster size {pw}x{ph}"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [pw as usize, ph as usize],
        pixmap.data(),
    ))
}

#[cfg(test)]
mod tests {
    mod raster {
        use super::super::ensure_xlink_namespace;
        use super::super::rasterize;

        #[test]
        fn non_utf8_svg_bytes_are_returned_unchanged() {
            // Not text, so there is no namespace to patch; passing it through
            // lets `usvg` report the encoding instead of an empty-input parse.
            let raw = vec![0xff, 0xfe, 0x3c, 0x73, 0x76, 0x67];
            assert_eq!(ensure_xlink_namespace(raw.clone()), raw);
        }

        #[test]
        fn undeclared_xlink_namespace_is_added() {
            // Mermaid emits `xlink:href` for C4/class links but some SVGs omit
            // the namespace declaration; usvg rejects that outright.
            let p = std::env::temp_dir().join(format!("jiaoren-xlink-{}.svg", std::process::id()));
            std::fs::write(
                &p,
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 5"><use xlink:href="#r"/></svg>"##,
            )
            .unwrap();
            let img = rasterize(&p, 10.0, 5.0, 1.0);
            assert!(img.is_ok(), "xlink svg must parse: {img:?}");
            let _ = std::fs::remove_file(&p);
        }

        #[test]
        fn huge_svg_is_capped_below_the_texture_limit() {
            // 1 x 100000 at scale 2 would be 200000 px tall, far past the GPU
            // texture limit. The raster must be scaled down to fit.
            let p = std::env::temp_dir().join(format!("jiaoren-huge-{}.svg", std::process::id()));
            std::fs::write(
                &p,
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 100000"><rect width="1" height="100000" fill="#ff0000"/></svg>"##,
            )
            .unwrap();
            let img = rasterize(&p, 1.0, 100000.0, 2.0).unwrap();
            assert_eq!(img.size[1], super::super::MAX_TEXTURE_SIDE as usize);
            assert!(img.size[0] >= 1);
            let _ = std::fs::remove_file(&p);
        }

        #[test]
        fn rasterize_produces_scaled_premultiplied_rgba() {
            // A 10x5 red rect rendered at scale 2 must be a 20x10 opaque red image.
            let p = std::env::temp_dir().join(format!("jiaoren-raster-{}.svg", std::process::id()));
            std::fs::write(
                &p,
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 5"><rect width="10" height="5" fill="#ff0000"/></svg>"##,
            )
            .unwrap();
            let img = rasterize(&p, 10.0, 5.0, 2.0).unwrap();
            assert_eq!(img.size, [20, 10]);
            assert_eq!(
                img.pixels[0],
                egui::Color32::from_rgba_premultiplied(255, 0, 0, 255)
            );
            let _ = std::fs::remove_file(&p);
        }
    }
}

#[cfg(test)]
mod pixel_budget {
    use super::{capped_scale, MAX_TEXTURE_PIXELS};

    #[test]
    fn huge_square_is_clipped_to_the_pixel_budget() {
        let scale = capped_scale(16384.0, 16384.0, 1.0);
        assert!(16384.0 * 16384.0 * scale * scale <= MAX_TEXTURE_PIXELS * 1.001);
    }

    #[test]
    fn small_diagram_keeps_its_scale() {
        assert_eq!(capped_scale(800.0, 600.0, 2.0), 2.0);
    }
}
