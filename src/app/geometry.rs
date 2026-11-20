//! Pure geometry helpers: reading-column widths, scroll re-anchoring, and the
//! resolution buckets that key diagram rasters.

use crate::mermaid::Theme;

/// Identity of one raster: diagram, theme, resolution bucket and display
/// density (rounded to two decimals) so a DPI change never serves a stale
/// image. Matches the `(id, theme, bucket, ppp_rounded_2dp)` interface.
pub(crate) type RasterKey = (String, &'static str, u8, i32);

/// Width of the reading column: the window minus 64 px of margins, capped at
/// 900 px scaled by zoom. Floored at 0 so a tiny window never goes negative.
pub(crate) fn content_column(available: f32, zoom: f32) -> f32 {
    (available - 64.0).min(900.0 * zoom).max(0.0)
}

/// Per-column width for a table spanning `available` points with `ncols`
/// columns and `spacing` between them. Exact division, floored at 80 px.
pub(crate) fn column_widths(available: f32, spacing: f32, ncols: usize) -> f32 {
    if ncols == 0 {
        return available;
    }
    let w = (available - spacing * (ncols - 1) as f32) / ncols as f32;
    w.max(80.0)
}

/// Keep a placeholder's top edge steady across a height change. Only content
/// whose placeholder started above the viewport is re-anchored; anything at or
/// below the fold is left alone. Pure so it is unit-testable.
pub(crate) fn reanchor(
    scroll_y: f32,
    viewport_top: f32,
    placeholder_top: f32,
    delta: f32,
) -> f32 {
    if placeholder_top < viewport_top {
        (scroll_y + delta).max(0.0)
    } else {
        scroll_y.max(0.0)
    }
}

/// Trailing space after the document so the last line can rise to mid-window
/// when scrolled to the end. Zero for a zero-height viewport.
pub(crate) fn bottom_pad(viewport_height: f32) -> f32 {
    viewport_height * 0.5
}

/// Resolution bucket covering a zoom factor: the smallest of `1..=3` whose
/// scale (bucket × DPI) is at least the on-screen zoom. Zoom is clamped to
/// `0.5..=3.0`, so this never exceeds 3.
pub(crate) fn bucket_for(zoom: f32) -> u8 {
    (zoom.ceil() as u8).clamp(1, 3)
}

/// Pixel scale for `bucket` at a given display density. Textures are rendered
/// at `bucket × pixels_per_point` texels per SVG unit.
pub(crate) fn bucket_scale(bucket: u8, pixels_per_point: f32) -> f32 {
    bucket as f32 * pixels_per_point
}

pub(crate) fn raster_key(
    id: &str,
    theme: Theme,
    bucket: u8,
    pixels_per_point: f32,
) -> RasterKey {
    (
        id.to_string(),
        theme.as_str(),
        bucket,
        (pixels_per_point * 100.0).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_grows_with_zoom() {
        assert_eq!(content_column(1800.0, 1.0), 900.0);
        assert_eq!(content_column(1800.0, 2.0), 1736.0);
        assert_eq!(content_column(1800.0, 3.0), 1736.0);
        assert_eq!(content_column(600.0, 1.0), 536.0);
        assert_eq!(content_column(1800.0, 0.5), 450.0);
        assert_eq!(content_column(40.0, 1.0), 0.0);
    }

    #[test]
    fn column_width_leaves_room_for_spacing() {
        assert!((column_widths(900.0, 8.0, 3) - 294.666_7).abs() < 0.01);
        assert_eq!(column_widths(100.0, 8.0, 10), 80.0);
    }

    #[test]
    fn bottom_pad_is_half_the_viewport() {
        assert_eq!(bottom_pad(800.0), 400.0);
        assert_eq!(bottom_pad(0.0), 0.0);
    }

    #[test]
    fn placeholder_above_viewport_is_reanchored() {
        assert_eq!(reanchor(500.0, 100.0, 50.0, 200.0), 700.0);
    }

    #[test]
    fn placeholder_below_viewport_is_not_reanchored() {
        assert_eq!(reanchor(500.0, 100.0, 400.0, 200.0), 500.0);
    }

    #[test]
    fn partially_visible_placeholder_is_reanchored() {
        // Top above the fold, bottom still visible.
        assert_eq!(reanchor(500.0, 100.0, 50.0, -120.0), 380.0);
    }

    #[test]
    fn zero_and_negative_delta_are_clamped() {
        assert_eq!(reanchor(500.0, 100.0, 50.0, 0.0), 500.0);
        assert_eq!(reanchor(50.0, 100.0, 20.0, -100.0), 0.0);
    }

    #[test]
    fn buckets_cover_the_zoom() {
        assert_eq!(bucket_for(0.5), 1);
        assert_eq!(bucket_for(1.0), 1);
        assert_eq!(bucket_for(1.4), 2);
        assert_eq!(bucket_for(2.6), 3);
        assert_eq!(bucket_for(3.0), 3);
    }

    #[test]
    fn bucket_scale_multiplies_by_dpi() {
        assert_eq!(bucket_scale(2, 1.0), 2.0);
        assert_eq!(bucket_scale(1, 1.25), 1.25);
    }
}
