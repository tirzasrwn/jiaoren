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
    let bytes = std::fs::read(path).map_err(|e| format!("read svg: {e}"))?;
    let tree = resvg::usvg::Tree::from_data(&bytes, svg_options())
        .map_err(|e| format!("parse svg: {e}"))?;
    let pw = (w * scale).ceil().max(1.0) as u32;
    let ph = (h * scale).ceil().max(1.0) as u32;
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
        use super::super::rasterize;

        #[test]
        fn rasterize_produces_scaled_premultiplied_rgba() {
            // A 10x5 red rect rendered at scale 2 must be a 20x10 opaque red image.
            let p = std::env::temp_dir().join(format!("mdview-raster-{}.svg", std::process::id()));
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
