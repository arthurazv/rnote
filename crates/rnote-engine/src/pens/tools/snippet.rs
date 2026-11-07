// Imports
use super::ToolsState;
use crate::engine::{EngineView, EngineViewMut};
use crate::engine::StrokeContent;
use crate::{Svg, WidgetFlags};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::color;
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::ext::AabbExt;
use rnote_compose::penevent::{ModifierKey, PenEvent, PenProgress};
use rnote_compose::shapes::Shapeable;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct SnippetTool {
    state: ToolsState,
    start_pos: Vector2,
    current_pos: Vector2,
}

impl Default for SnippetTool {
    fn default() -> Self {
        Self {
            state: ToolsState::default(),
            start_pos: Vector2::ZERO,
            current_pos: Vector2::ZERO,
        }
    }
}

impl SnippetTool {
    pub fn reset(&mut self) {
        self.state = ToolsState::Idle;
        self.start_pos = Vector2::ZERO;
        self.current_pos = Vector2::ZERO;
    }

    pub fn handle_event(
        &mut self,
        event: PenEvent,
        _now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        let result = match (&mut self.state, event) {
            (ToolsState::Idle, PenEvent::Down { element, .. }) => {
                self.start_pos = element.pos;
                self.current_pos = element.pos;
                self.state = ToolsState::Active;
                widget_flags.redraw = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (ToolsState::Idle, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (ToolsState::Active, PenEvent::Down { element, .. }) => {
                self.current_pos = element.pos;
                widget_flags.redraw = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (ToolsState::Active, PenEvent::Up { element, modifier_keys }) => {
                self.current_pos = element.pos;
                self.state = ToolsState::Idle;
                widget_flags.redraw = true;

                let mut bounds = Aabb::new(self.start_pos, self.current_pos);
                bounds.ensure_positive();

                if bounds.extents()[0] >= 5.0 && bounds.extents()[1] >= 5.0 {
                    let keys = engine_view
                        .store
                        .stroke_keys_as_rendered_intersecting_bounds(bounds);
                    if !keys.is_empty() {
                        let stroke_content = engine_view
                            .store
                            .fetch_stroke_content(&keys)
                            .with_background(engine_view.document.config.background)
                            .with_bounds(bounds);

                        let is_cut = modifier_keys.contains(&ModifierKey::KeyboardShift);
                        let image_scale = 2.0;

                        if let Ok(cropped_image) = crate::Image::gen_with_cairo(
                            |cairo_cx| {
                                stroke_content.draw_to_cairo(
                                    cairo_cx,
                                    true,
                                    false,
                                    false,
                                    0.0,
                                    image_scale,
                                )
                            },
                            bounds,
                            image_scale,
                        ) {
                            if let Ok(png_bytes) = cropped_image
                                .clone()
                                .into_encoded_bytes(image::ImageFormat::Png, None)
                            {
                                let mut clipboard_content = Vec::new();

                                // Build a cropped BitmapImage stroke at exact 1:1 document bounds
                                // with 2x Retina pixel density so pasting inside Rnote (⌘V) pastes
                                // ONLY the cropped region rather than the entire uncropped PDF page!
                                let logical_size = bounds.extents();
                                let affine =
                                    p2d::glamx::DAffine2::from_translation(logical_size * 0.5);
                                let cropped_bitmap = crate::strokes::BitmapImage {
                                    image: cropped_image,
                                    rectangle: rnote_compose::shapes::Rectangle {
                                        cuboid: p2d::shape::Cuboid::new(logical_size * 0.5),
                                        affine,
                                    },
                                    text_runs: Vec::new(),
                                    pdf_source: None,
                                };
                                let cropped_stroke_content = StrokeContent::default()
                                    .with_strokes(vec![crate::engine::strokecontent::LayeredStroke {
                                        stroke: std::sync::Arc::new(
                                            crate::strokes::Stroke::BitmapImage(cropped_bitmap),
                                        ),
                                        layer: crate::store::chrono_comp::StrokeLayer::UserLayer(0),
                                    }])
                                    .with_bounds(Aabb::new(Vector2::ZERO, logical_size));

                                if let Ok(json) = serde_json::to_string(&cropped_stroke_content) {
                                    clipboard_content.push((
                                        json.into_bytes(),
                                        StrokeContent::MIME_TYPE.to_string(),
                                    ));
                                }
                                if let Ok(Some(svg)) =
                                    stroke_content.gen_svg(true, false, false, 0.0)
                                {
                                    clipboard_content.push((
                                        svg.to_document().into_bytes(),
                                        Svg::MIME_TYPE.to_string(),
                                    ));
                                }
                                clipboard_content.push((png_bytes, "image/png".to_string()));

                                // Extract any text embedded inside the cropped bounds (e.g. from PDFs or TextStrokes)
                                // and place it on the clipboard as "text/plain" so users can paste clean text into any app!
                                let mut extracted_text_pieces = Vec::new();
                                for &k in &keys {
                                    if let Some(stroke) = engine_view.store.get_stroke_arc(k) {
                                        match &*stroke {
                                            crate::strokes::Stroke::BitmapImage(b) => {
                                                for run in &b.text_runs {
                                                    let run_bounds = crate::textsearch::text_run_to_doc_bounds(&run.bbox, &b.rectangle);
                                                    if bounds.intersects(&run_bounds) {
                                                        extracted_text_pieces.push(run.text.clone());
                                                    }
                                                }
                                            }
                                            crate::strokes::Stroke::VectorImage(v) => {
                                                for run in &v.text_runs {
                                                    let run_bounds = crate::textsearch::text_run_to_doc_bounds(&run.bbox, &v.rectangle);
                                                    if bounds.intersects(&run_bounds) {
                                                        extracted_text_pieces.push(run.text.clone());
                                                    }
                                                }
                                            }
                                            crate::strokes::Stroke::TextStroke(t) => {
                                                if bounds.intersects(&t.bounds()) {
                                                    extracted_text_pieces.push(t.text.clone());
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                if !extracted_text_pieces.is_empty() {
                                    let joined_text = extracted_text_pieces.join(" ");
                                    clipboard_content.push((
                                        joined_text.into_bytes(),
                                        "text/plain".to_string(),
                                    ));
                                }

                                widget_flags.clipboard_content = Some(clipboard_content);

                                if is_cut {
                                    let trash_keys = keys
                                        .into_iter()
                                        .filter(|&k| {
                                            if let Some(stroke) =
                                                engine_view.store.get_stroke_arc(k)
                                            {
                                                bounds.contains(&stroke.bounds())
                                            } else {
                                                false
                                            }
                                        })
                                        .collect::<Vec<_>>();

                                    if !trash_keys.is_empty() {
                                        engine_view.store.set_trashed_keys(&trash_keys, true);
                                        widget_flags |= engine_view.store.record(Instant::now());
                                        widget_flags.store_modified = true;
                                    }
                                    widget_flags.clipboard_toast = Some(
                                        "Trecho recortado e copiado! Cole com ⌘V".to_string(),
                                    );
                                } else {
                                    if !extracted_text_pieces.is_empty() {
                                        let count = extracted_text_pieces.len();
                                        widget_flags.clipboard_toast = Some(
                                            format!("Trecho copiado (imagem + {count} palavras)! Cole com ⌘V"),
                                        );
                                    } else {
                                        widget_flags.clipboard_toast = Some(
                                            "Trecho copiado para a área de transferência! Cole com ⌘V"
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                self.reset();

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (ToolsState::Active, PenEvent::Cancel) => {
                self.reset();
                widget_flags.redraw = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (ToolsState::Active, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::InProgress,
            },
        };

        (result, widget_flags)
    }

    pub fn bounds_on_doc(&self, _engine_view: &EngineView) -> Option<Aabb> {
        if let ToolsState::Active = self.state {
            let mut bounds = Aabb::new(self.start_pos, self.current_pos);
            bounds.ensure_positive();
            Some(bounds.loosened(10.0))
        } else {
            None
        }
    }

    pub fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        if let ToolsState::Active = self.state {
            let mut bounds = Aabb::new(self.start_pos, self.current_pos);
            bounds.ensure_positive();

            let total_zoom = engine_view.camera.total_zoom();
            let line_width = 1.5 / total_zoom;

            let rect = piet::kurbo::Rect::new(
                bounds.mins[0],
                bounds.mins[1],
                bounds.maxs[0],
                bounds.maxs[1],
            );

            // Semi-transparent fill
            let fill_color = piet::Color::rgba(0.21, 0.52, 0.89, 0.15);
            cx.fill(rect, &fill_color);

            // Dashed border
            let stroke_color = color::GNOME_BLUES[3];
            const DASH_PATTERN: &[f64] = &[6.0, 4.0];
            let stroke_style = piet::StrokeStyle::new()
                .dash_pattern(DASH_PATTERN)
                .line_cap(piet::LineCap::Round)
                .line_join(piet::LineJoin::Round);

            cx.stroke_styled(rect, &stroke_color, line_width, &stroke_style);

            // Corner accents (4 corners)
            let corner_len = (10.0 / total_zoom).min(bounds.extents()[0] * 0.3).min(bounds.extents()[1] * 0.3);
            let corner_style = piet::StrokeStyle::new()
                .line_cap(piet::LineCap::Square)
                .line_join(piet::LineJoin::Round);
            let corner_color = color::GNOME_BLUES[4];
            let corner_width = 2.5 / total_zoom;

            // Top-left
            let mut p = piet::kurbo::BezPath::new();
            p.move_to((bounds.mins[0], bounds.mins[1] + corner_len));
            p.line_to((bounds.mins[0], bounds.mins[1]));
            p.line_to((bounds.mins[0] + corner_len, bounds.mins[1]));
            cx.stroke_styled(p, &corner_color, corner_width, &corner_style);

            // Top-right
            let mut p = piet::kurbo::BezPath::new();
            p.move_to((bounds.maxs[0] - corner_len, bounds.mins[1]));
            p.line_to((bounds.maxs[0], bounds.mins[1]));
            p.line_to((bounds.maxs[0], bounds.mins[1] + corner_len));
            cx.stroke_styled(p, &corner_color, corner_width, &corner_style);

            // Bottom-right
            let mut p = piet::kurbo::BezPath::new();
            p.move_to((bounds.maxs[0], bounds.maxs[1] - corner_len));
            p.line_to((bounds.maxs[0], bounds.maxs[1]));
            p.line_to((bounds.maxs[0] - corner_len, bounds.maxs[1]));
            cx.stroke_styled(p, &corner_color, corner_width, &corner_style);

            // Bottom-left
            let mut p = piet::kurbo::BezPath::new();
            p.move_to((bounds.mins[0] + corner_len, bounds.maxs[1]));
            p.line_to((bounds.mins[0], bounds.maxs[1]));
            p.line_to((bounds.mins[0], bounds.maxs[1] - corner_len));
            cx.stroke_styled(p, &corner_color, corner_width, &corner_style);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet_tool_lifecycle() {
        let mut tool = SnippetTool::default();
        assert!(matches!(tool.state, ToolsState::Idle));
        assert_eq!(tool.start_pos, Vector2::ZERO);

        tool.start_pos = Vector2::new(10.0, 20.0);
        tool.current_pos = Vector2::new(100.0, 150.0);
        tool.state = ToolsState::Active;

        let bounds = Aabb::new(tool.start_pos, tool.current_pos);
        assert_eq!(bounds.mins, Vector2::new(10.0, 20.0));
        assert_eq!(bounds.maxs, Vector2::new(100.0, 150.0));
        assert_eq!(bounds.extents(), Vector2::new(90.0, 130.0));

        tool.reset();
        assert!(matches!(tool.state, ToolsState::Idle));
        assert_eq!(tool.start_pos, Vector2::ZERO);
    }
}
