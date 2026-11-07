// Imports
use super::ToolsState;
use crate::engine::{EngineView, EngineViewMut};
use crate::WidgetFlags;
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::color;
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::ext::AabbExt;
use rnote_compose::penevent::{PenEvent, PenProgress};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct SnippetTool {
    state: ToolsState,
    start_pos: Vector2,
    current_pos: Vector2,
    pub(crate) selected_bounds: Option<Aabb>,
}

impl Default for SnippetTool {
    fn default() -> Self {
        Self {
            state: ToolsState::default(),
            start_pos: Vector2::ZERO,
            current_pos: Vector2::ZERO,
            selected_bounds: None,
        }
    }
}

impl SnippetTool {
    pub fn reset(&mut self) {
        self.state = ToolsState::Idle;
        self.start_pos = Vector2::ZERO;
        self.current_pos = Vector2::ZERO;
        self.selected_bounds = None;
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
                self.selected_bounds = None;
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
            (ToolsState::Active, PenEvent::Up { element, modifier_keys: _ }) => {
                self.current_pos = element.pos;
                self.state = ToolsState::Idle;
                widget_flags.redraw = true;

                let mut bounds = Aabb::new(self.start_pos, self.current_pos);
                bounds.ensure_positive();

                if bounds.extents()[0] >= 5.0 && bounds.extents()[1] >= 5.0 {
                    if let Some((_, flags)) = engine_view.store.generate_area_snippet_clipboard(
                        engine_view.document.config.background,
                        bounds,
                    ) {
                        widget_flags |= flags;
                        self.selected_bounds = Some(bounds);
                    }
                }

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
        } else if let Some(bounds) = self.selected_bounds {
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
        let draw_bounds = if let ToolsState::Active = self.state {
            let mut bounds = Aabb::new(self.start_pos, self.current_pos);
            bounds.ensure_positive();
            Some(bounds)
        } else {
            self.selected_bounds
        };

        if let Some(bounds) = draw_bounds {

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
