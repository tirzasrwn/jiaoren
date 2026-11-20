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

/// Widths proportional to each column's desired width, fitting `available`
/// after `spacing` between columns, with a `min` floor. The floor shrinks when
/// the space cannot fit `min` in every column, so the sum never exceeds the
/// target. `desired` must be non-empty; values are clamped to >= 1.0 so no
/// column starves.
pub(crate) fn table_column_widths(
    desired: &[f32],
    available: f32,
    spacing: f32,
    min: f32,
) -> Vec<f32> {
    let n = desired.len();
    if n == 0 {
        return Vec::new();
    }
    let raw_total = available - spacing * (n - 1) as f32;
    let total = if raw_total.is_finite() && raw_total > 0.0 {
        raw_total
    } else {
        0.0
    };
    let min = if min.is_finite() { min.max(0.0) } else { 0.0 };
    let min = min.min(total / n as f32);
    if total == 0.0 {
        return vec![min; n];
    }
    let desire: Vec<f32> = desired
        .iter()
        .map(|d| if d.is_finite() { d.max(1.0) } else { 1.0 })
        .collect();
    let mut fixed = vec![false; n];
    loop {
        let active_sum: f32 = desire
            .iter()
            .zip(&fixed)
            .filter(|(_, f)| !**f)
            .map(|(d, _)| d)
            .sum();
        let fixed_sum = min * fixed.iter().filter(|f| **f).count() as f32;
        let scale = if active_sum > 0.0 {
            (total - fixed_sum) / active_sum
        } else {
            0.0
        };
        let mut changed = false;
        for (i, d) in desire.iter().enumerate() {
            if !fixed[i] && d * scale < min {
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed {
            return desire
                .iter()
                .zip(&fixed)
                .map(|(d, f)| if *f { min } else { d * scale })
                .collect();
        }
    }
}

/// Cap each width at `max`, redistributing the excess over the columns still
/// below the cap. The input sum is preserved when at least one column can grow;
/// if every column reaches the cap the result is simply `max` for each.
pub(crate) fn cap_column_widths(widths: &[f32], max: f32) -> Vec<f32> {
    let n = widths.len();
    if n == 0 {
        return Vec::new();
    }
    let max = if max.is_finite() { max.max(0.0) } else { 0.0 };
    let total: f32 = widths.iter().map(|w| w.max(0.0)).sum();
    let mut fixed = vec![false; n];
    loop {
        let fixed_sum = max * fixed.iter().filter(|f| **f).count() as f32;
        let active_sum: f32 = widths
            .iter()
            .zip(&fixed)
            .filter(|(_, f)| !**f)
            .map(|(w, _)| w.max(0.0))
            .sum();
        if active_sum <= 0.0 {
            return widths
                .iter()
                .zip(&fixed)
                .map(|(w, f)| if *f { max } else { w.max(0.0) })
                .collect();
        }
        let scale = (total - fixed_sum) / active_sum;
        let mut changed = false;
        for (i, w) in widths.iter().enumerate() {
            if !fixed[i] && w.max(0.0) * scale > max {
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed {
            return widths
                .iter()
                .zip(&fixed)
                .map(|(w, f)| if *f { max } else { w.max(0.0) * scale })
                .collect();
        }
    }
}

/// Keep a placeholder's top edge steady across a height change. Only content
/// whose placeholder started above the viewport is re-anchored; anything at or
/// below the fold is left alone. Pure so it is unit-testable.
pub(crate) fn reanchor(scroll_y: f32, viewport_top: f32, placeholder_top: f32, delta: f32) -> f32 {
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

/// Build the [`RasterKey`] for `id` at `bucket` and display density.
pub(crate) fn raster_key(id: &str, theme: Theme, bucket: u8, pixels_per_point: f32) -> RasterKey {
    (
        id.to_string(),
        theme.as_str(),
        bucket,
        (pixels_per_point * 100.0).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    mod geometry {
        use super::super::*;

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
        fn table_widths_scale_to_desired_content() {
            let w = table_column_widths(&[100.0, 100.0, 300.0], 900.0, 8.0, 80.0);
            assert!(((w.iter().sum::<f32>()) - 884.0).abs() < 0.01);
            assert!(w.iter().all(|x| *x >= 80.0));
            assert!((w[2] - 3.0 * w[0]).abs() < 0.5);
        }

        #[test]
        fn table_widths_floor_starved_columns() {
            let w = table_column_widths(&[10.0, 100.0, 100.0], 300.0, 8.0, 80.0);
            assert_eq!(w[0], 80.0);
            assert!((w[1] - w[2]).abs() < 0.01);
            assert!(((w.iter().sum::<f32>()) - 284.0).abs() < 0.01);
        }

        #[test]
        fn table_widths_shrink_floor_when_min_cannot_fit() {
            let w = table_column_widths(&[1.0; 5], 100.0, 8.0, 80.0);
            let floor = (100.0 - 8.0 * 4.0) / 5.0;
            assert!(w.iter().all(|x| (*x - floor).abs() < 0.01));
            assert!(w.iter().sum::<f32>() <= 68.0 + 0.01);
        }

        #[test]
        fn table_widths_cap_redistributes_surplus() {
            let w = table_column_widths(&[2000.0, 100.0], 900.0, 8.0, 80.0);
            let capped = cap_column_widths(&w, 0.6 * 892.0);
            assert!(capped[0] <= 0.6 * 892.0 + 0.01);
            assert!(capped[1] > 80.0);
            assert!((capped[0] + capped[1] - 892.0).abs() < 0.01);
        }

        #[test]
        fn table_widths_single_column_keeps_full_width() {
            let w = table_column_widths(&[2000.0], 900.0, 8.0, 80.0);
            let capped = cap_column_widths(&w, 900.0);
            assert!((capped[0] - 900.0).abs() < 0.01);
        }

        #[test]
        fn table_widths_empty_is_empty() {
            assert!(table_column_widths(&[], 900.0, 8.0, 80.0).is_empty());
        }

        #[test]
        fn table_widths_clamp_non_positive_desire() {
            let w = table_column_widths(&[0.0, -5.0, 100.0], 400.0, 8.0, 80.0);
            assert!(w.iter().all(|x| x.is_finite() && *x >= 80.0));
            assert!((w.iter().sum::<f32>() - 384.0).abs() < 0.01);
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
}
