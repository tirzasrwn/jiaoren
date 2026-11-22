//! Small painted controls shared by the render hooks.

use egui::{Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind};

/// The same icon at a caller-chosen rect, for overlaying a block corner without
/// taking layout space. `id` must be unique per visible instance.
pub(crate) fn copy_button_at(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    tooltip: &str,
    backdrop: Color32,
) -> (bool, Rect) {
    let response = ui.interact(rect, id, Sense::CLICK);
    (
        paint_copy_button(ui, rect, response, tooltip, backdrop),
        rect,
    )
}

fn paint_copy_button(
    ui: &egui::Ui,
    rect: Rect,
    response: egui::Response,
    tooltip: &str,
    backdrop: Color32,
) -> bool {
    let response = response.on_hover_text(tooltip);
    let hovered = response.hovered();
    let visuals = ui.visuals();
    let hover_bg = visuals.widgets.hovered.weak_bg_fill;
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), hover_bg);
    }
    let color = if hovered {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    let stroke = Stroke::new(1.0, color);
    // A 14×14 icon: back sheet 9×9 at the top-left, front sheet 9×9 offset by
    // 4 px. The front is filled with the local background so the back outline
    // does not cut through it.
    let back = Rect::from_min_size(rect.min + egui::vec2(0.5, 0.5), egui::vec2(9.0, 9.0));
    let front = Rect::from_min_size(rect.min + egui::vec2(4.5, 4.5), egui::vec2(9.0, 9.0));
    let radius = CornerRadius::same(1);
    let painter = ui.painter();
    painter.rect_stroke(back, radius, stroke, StrokeKind::Inside);
    painter.rect_filled(front, radius, if hovered { hover_bg } else { backdrop });
    painter.rect_stroke(front, radius, stroke, StrokeKind::Inside);
    response.clicked()
}
