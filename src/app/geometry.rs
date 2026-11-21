//! Pure geometry helpers: reading-column widths, scroll re-anchoring, and the
//! resolution buckets that key diagram rasters.

use crate::mermaid::Theme;

/// Identity of one raster: diagram, theme, resolution bucket and display
/// density (rounded to two decimals) so a DPI change never serves a stale
/// image. Matches the `(id, theme, bucket, ppp_rounded_2dp)` interface.
pub(crate) type RasterKey = (String, &'static str, u16, i32);

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

/// Displayed size of a diagram: small diagrams keep their intrinsic size
/// (times app zoom) and are centred in the column; wide ones are capped to the
/// column. Height always follows the intrinsic aspect ratio. A non-finite or
/// non-positive intrinsic width falls back to a 2:1 box no wider than 320 × zoom.
pub(crate) fn diagram_display_size(intrinsic: (f32, f32), column: f32, zoom: f32) -> (f32, f32) {
    let (iw, ih) = intrinsic;
    if !iw.is_finite() || !ih.is_finite() || iw <= 0.0 {
        let w = column.min(320.0 * zoom);
        return (w, w * 0.5);
    }
    let w = (iw * zoom).min(column);
    (w, w * ih / iw)
}

/// Resolution bucket that covers `scale` ([`diagram_display_size`]'s width in
/// points divided by the intrinsic width), clamped to the raster budget in
/// device pixels: each side ≤ 8192 px and ≤ 16 Mpx of pixels. `ppp` converts
/// the SVG's CSS units to device pixels, so it must not also appear in
/// `scale` — one `ppp` factor, at texture creation ([`bucket_scale`]).
/// Rounding up keeps the raster at least as sharp as the display. Non-finite
/// input means bucket 1.
pub(crate) fn bucket_for_scale(scale: f32, iw: f32, ih: f32, ppp: f32) -> u16 {
    if !scale.is_finite()
        || !iw.is_finite()
        || !ih.is_finite()
        || !ppp.is_finite()
        || iw <= 0.0
        || ih <= 0.0
        || ppp <= 0.0
    {
        return 1;
    }
    let (pw, ph) = (iw * ppp, ih * ppp);
    let side = (8192.0 / pw).floor().min((8192.0 / ph).floor());
    // `pw * ph` is finite and positive here, so the area cap is defined.
    let area = (16_000_000.0 / (pw * ph)).sqrt().floor();
    let bucket = scale.ceil().max(1.0).min(side).min(area).max(1.0);
    // The budget caps keep this below u16::MAX (≤ 4000 even for a 1×1 SVG).
    bucket as u16
}

/// Whether a raster bucket stays when the diagram is drawn at `bucket`: that
/// bucket and the next one down (the best-ready fallback), nothing else.
pub(crate) fn keep_bucket(bucket: u16, other: u16) -> bool {
    other == bucket || (other > 0 && other + 1 == bucket)
}

/// Zoom factor in the diagram popup after one step: `0.25` per step, clamped to
/// `1.0..=3.0`. Non-finite input resets to `1.0` so the overlay never draws a
/// NaN-sized image.
pub(crate) fn popup_zoom_step(zoom: f32, delta: i32) -> f32 {
    if !zoom.is_finite() {
        return 1.0;
    }
    (zoom + delta as f32 * 0.25).clamp(1.0, 3.0)
}

/// New pan that keeps the image point under `anchor` fixed while the image
/// scales from `old_size` to `new_size`.
///
/// `pan` is the image's top-left relative to the view's top-left and `anchor`
/// is relative to the same origin. The image point under the anchor is
/// `content = anchor - pan`; after scaling by `new_size / old_size` it must sit
/// under the anchor again, so `pan' = anchor - content * k`. A non-finite
/// pan/anchor/size returns `pan` unchanged so the overlay never draws a
/// NaN-sized image.
pub(crate) fn zoom_anchored(
    pan: egui::Vec2,
    anchor: egui::Vec2,
    old_size: egui::Vec2,
    new_size: egui::Vec2,
) -> egui::Vec2 {
    if !pan.is_finite()
        || !anchor.is_finite()
        || !old_size.is_finite()
        || !new_size.is_finite()
        || old_size.min_elem() <= 0.0
        || new_size.min_elem() <= 0.0
    {
        return pan;
    }
    let k = new_size / old_size;
    anchor - (anchor - pan) * k
}

/// Clamp the image's top-left so its edges can reach the middle of the view
/// but no further: `pan` in `[view * 0.5 - image, view * 0.5]` per axis. The
/// range is symmetric around the centred position `(view - image) * 0.5`, so
/// it holds for images smaller than the view too. Non-finite input falls back
/// to the centred position (or zero when even that is not finite).
pub(crate) fn clamp_pan(pan: egui::Vec2, image: egui::Vec2, view: egui::Vec2) -> egui::Vec2 {
    let centre = (view - image) * 0.5;
    if !pan.is_finite() || !image.is_finite() || !view.is_finite() {
        return if centre.is_finite() {
            centre
        } else {
            egui::Vec2::ZERO
        };
    }
    let half = view * 0.5;
    pan.clamp(half - image, half)
}

/// Pixel scale for `bucket` at a given display density. Textures are rendered
/// at `bucket × pixels_per_point` texels per SVG unit.
pub(crate) fn bucket_scale(bucket: u16, pixels_per_point: f32) -> f32 {
    bucket as f32 * pixels_per_point
}

/// Build the [`RasterKey`] for `id` at `bucket` and display density.
pub(crate) fn raster_key(id: &str, theme: Theme, bucket: u16, pixels_per_point: f32) -> RasterKey {
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
        fn display_size_keeps_small_diagrams_small() {
            assert_eq!(
                diagram_display_size((87.0, 160.0), 900.0, 1.0),
                (87.0, 160.0)
            );
            // Zoom doubles the intrinsic size, still under the column.
            assert_eq!(
                diagram_display_size((87.0, 160.0), 900.0, 2.0),
                (174.0, 320.0)
            );
        }

        #[test]
        fn display_size_caps_wide_diagrams_to_the_column() {
            let (w, h) = diagram_display_size((1400.0, 300.0), 900.0, 1.0);
            assert_eq!(w, 900.0);
            assert!((h - 900.0 * 300.0 / 1400.0).abs() < 0.01);
            assert!((h - 192.857).abs() < 0.01);
            // Zoom widens the column elsewhere, but the cap still applies.
            assert_eq!(diagram_display_size((1400.0, 300.0), 900.0, 2.0).0, 900.0);
        }

        #[test]
        fn display_size_guards_zero_width() {
            assert_eq!(
                diagram_display_size((0.0, 160.0), 900.0, 1.0),
                (320.0, 160.0)
            );
            assert_eq!(
                diagram_display_size((0.0, 160.0), 100.0, 1.0),
                (100.0, 50.0)
            );
        }

        #[test]
        fn bucket_covers_the_scale_needed_for_the_column() {
            // 900 pt column over an 87 SVG-unit wide diagram needs ~10.34x.
            let scale = 900.0 / 87.0;
            assert_eq!(bucket_for_scale(scale, 87.0, 160.0, 1.0), 11);
        }

        #[test]
        fn bucket_is_one_for_an_unscaled_diagram_at_any_density() {
            // 1:1 points: bucket 1 must already give a device-pixel-sharp raster
            // (bucket_scale then multiplies by ppp only once).
            assert_eq!(bucket_for_scale(1.0, 86.0, 166.0, 1.0), 1);
            assert_eq!(bucket_for_scale(1.0, 86.0, 166.0, 1.083_333_4), 1);
            assert_eq!(bucket_for_scale(1.0, 86.0, 166.0, 2.0), 1);
        }

        #[test]
        fn bucket_is_one_when_intrinsic_is_already_wide() {
            let scale = 900.0 / 2000.0;
            assert_eq!(bucket_for_scale(scale, 2000.0, 1500.0, 1.0), 1);
        }

        #[test]
        fn bucket_clamps_to_the_pixel_budget() {
            // 1000x over 10x10 would be 10000x10000 (area 1e8); the 16 Mpx
            // budget caps it at sqrt(16_000_000 / 100) = 400.
            assert_eq!(bucket_for_scale(1000.0, 10.0, 10.0, 1.0), 400);
            // Side cap wins for a long thin diagram.
            assert_eq!(bucket_for_scale(1000.0, 100.0, 4000.0, 1.0), 2);
            // Density counts: at ppp 2 the same 10x10 SVG is 20x20 device px,
            // so the area cap is sqrt(16_000_000 / 400) = 200.
            assert_eq!(bucket_for_scale(1000.0, 10.0, 10.0, 2.0), 200);
            // A large intrinsic at ppp 2 hits the 8192 px side cap immediately.
            assert_eq!(bucket_for_scale(4.0, 3000.0, 1000.0, 2.0), 1);
        }

        #[test]
        fn bucket_handles_non_finite_input() {
            assert_eq!(bucket_for_scale(f32::NAN, 87.0, 160.0, 1.0), 1);
            assert_eq!(bucket_for_scale(f32::INFINITY, 87.0, 160.0, 1.0), 1);
            assert_eq!(bucket_for_scale(1.0, 0.0, 160.0, 1.0), 1);
            assert_eq!(bucket_for_scale(1.0, 87.0, 160.0, 0.0), 1);
            assert_eq!(bucket_for_scale(1.0, 87.0, 160.0, f32::NAN), 1);
        }

        #[test]
        fn keep_bucket_keeps_current_and_one_below() {
            assert!(keep_bucket(5, 5));
            assert!(keep_bucket(5, 4));
            assert!(!keep_bucket(5, 3));
            assert!(!keep_bucket(5, 6));
            assert!(keep_bucket(1, 1));
            assert!(!keep_bucket(1, 0));
        }

        #[test]
        fn popup_zoom_steps_by_quarter_and_clamps() {
            assert_eq!(popup_zoom_step(1.0, 1), 1.25);
            assert_eq!(popup_zoom_step(3.0, 1), 3.0);
            assert_eq!(popup_zoom_step(1.0, -1), 1.0);
            assert_eq!(popup_zoom_step(2.0, -2), 1.5);
            assert_eq!(popup_zoom_step(2.875, 1), 3.0);
            assert_eq!(popup_zoom_step(f32::NAN, 1), 1.0);
        }

        #[test]
        fn zoom_anchor_centred_image_stays_centred() {
            // Centred 800x800 in a 1000x1000 view: pan 100. At 2x the same
            // centre anchor lands on the centred position for 1600x1600.
            let pan = zoom_anchored(
                egui::vec2(100.0, 100.0),
                egui::vec2(500.0, 500.0),
                egui::vec2(800.0, 800.0),
                egui::vec2(1600.0, 1600.0),
            );
            assert_eq!(pan, egui::vec2(-300.0, -300.0));
        }

        #[test]
        fn zoom_anchor_at_view_min_keeps_the_top_left_content_point() {
            // anchor 0 sits on image-local point -pan; after scaling it must
            // still be at 0, i.e. pan' = pan * k.
            let pan = zoom_anchored(
                egui::vec2(100.0, 100.0),
                egui::Vec2::ZERO,
                egui::vec2(800.0, 800.0),
                egui::vec2(1600.0, 1600.0),
            );
            assert_eq!(pan, egui::vec2(200.0, 200.0));
        }

        #[test]
        fn zoom_out_from_2x_recentres_when_the_image_fits() {
            let pan = zoom_anchored(
                egui::vec2(-300.0, -300.0),
                egui::vec2(500.0, 500.0),
                egui::vec2(1600.0, 1600.0),
                egui::vec2(800.0, 800.0),
            );
            assert_eq!(pan, egui::vec2(100.0, 100.0));
        }

        #[test]
        fn zoom_anchor_guards_bad_values() {
            let pan = egui::vec2(100.0, 50.0);
            let p = egui::vec2(10.0, 20.0);
            let size = egui::vec2(300.0, 200.0);
            assert_eq!(zoom_anchored(pan, p, size, egui::Vec2::ZERO), pan);
            assert_eq!(zoom_anchored(pan, p, egui::Vec2::ZERO, size), pan);
            assert_eq!(zoom_anchored(pan, p, egui::vec2(-1.0, 200.0), size), pan);
            assert_eq!(
                zoom_anchored(pan, p, size, egui::vec2(f32::NAN, 200.0)),
                pan
            );
            let nan_pan = zoom_anchored(egui::vec2(f32::NAN, 0.0), p, size, size);
            assert!(nan_pan.x.is_nan() && nan_pan.y == 0.0);
            assert_eq!(
                zoom_anchored(pan, egui::vec2(f32::INFINITY, 0.0), size, size),
                pan
            );
        }

        #[test]
        fn clamp_pan_allows_half_a_view_of_overscroll_for_a_small_image() {
            // view 1000, image 200: left edge may reach the middle (500) or
            // the right edge may (300 = 500 - 200); centred 400 stays put.
            let view = egui::vec2(1000.0, 1000.0);
            let image = egui::vec2(200.0, 200.0);
            assert_eq!(
                clamp_pan(egui::vec2(400.0, 400.0), image, view),
                egui::vec2(400.0, 400.0)
            );
            assert_eq!(
                clamp_pan(egui::vec2(5000.0, -5000.0), image, view),
                egui::vec2(500.0, 300.0)
            );
        }

        #[test]
        fn clamp_pan_allows_an_overflowing_image_to_reach_the_middle() {
            // image 1400 in a 1000 view: right edge at the middle is
            // 500 - 1400 = -900; left edge at the middle is 500.
            let view = egui::vec2(1000.0, 1000.0);
            let image = egui::vec2(1400.0, 1400.0);
            assert_eq!(clamp_pan(egui::Vec2::ZERO, image, view), egui::Vec2::ZERO);
            assert_eq!(
                clamp_pan(egui::vec2(-5000.0, 5000.0), image, view),
                egui::vec2(-900.0, 500.0)
            );
        }

        #[test]
        fn clamp_pan_is_per_axis() {
            let view = egui::vec2(1000.0, 1000.0);
            // Wide and short: x overflows, y is small.
            assert_eq!(
                clamp_pan(egui::vec2(5000.0, 5000.0), egui::vec2(1400.0, 200.0), view),
                egui::vec2(500.0, 500.0)
            );
            // Narrow and tall: x is small, y overflows.
            assert_eq!(
                clamp_pan(
                    egui::vec2(-5000.0, -5000.0),
                    egui::vec2(200.0, 1400.0),
                    view
                ),
                egui::vec2(300.0, -900.0)
            );
        }

        #[test]
        fn clamp_pan_guards_non_finite_values_with_the_centred_position() {
            let image = egui::vec2(200.0, 200.0);
            let view = egui::vec2(1000.0, 1000.0);
            assert_eq!(
                clamp_pan(egui::vec2(f32::NAN, 10.0), image, view),
                egui::vec2(400.0, 400.0)
            );
            assert_eq!(
                clamp_pan(
                    egui::vec2(10.0, 10.0),
                    egui::vec2(f32::INFINITY, 200.0),
                    view
                ),
                egui::Vec2::ZERO
            );
            assert_eq!(
                clamp_pan(egui::vec2(10.0, 10.0), image, egui::vec2(f32::NAN, 1000.0)),
                egui::Vec2::ZERO
            );
        }

        #[test]
        fn bucket_scale_multiplies_by_dpi() {
            assert_eq!(bucket_scale(2, 1.0), 2.0);
            assert_eq!(bucket_scale(1, 1.25), 1.25);
        }
    }
}
