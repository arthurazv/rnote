// Modules
mod laser;
mod offsetcamera;
mod snippet;
mod verticalspace;
mod zoom;

// Re-Exports
use laser::LaserTool;
use offsetcamera::OffsetCameraTool;
use snippet::SnippetTool;
use verticalspace::VerticalSpaceTool;
use zoom::ZoomTool;

// Imports
use super::PenBehaviour;
use super::PenStyle;
use super::pensconfig::toolsconfig::ToolStyle;
use crate::engine::{EngineView, EngineViewMut};
use crate::{DrawableOnDoc, WidgetFlags};
use futures::channel::oneshot;
use p2d::bounding_volume::Aabb;
use piet::RenderContext;
use rnote_compose::eventresult::EventResult;
use rnote_compose::penevent::{PenEvent, PenProgress};
use std::time::Instant;
use tracing::error;

#[derive(Debug, Clone, Copy)]
enum ToolsState {
    Idle,
    Active,
}

impl Default for ToolsState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Default)]
pub struct Tools {
    verticalspace_tool: VerticalSpaceTool,
    offsetcamera_tool: OffsetCameraTool,
    zoom_tool: ZoomTool,
    laser_tool: LaserTool,
    snippet_tool: SnippetTool,
}

impl PenBehaviour for Tools {
    fn init(&mut self, _engine_view: &EngineView) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn deinit(&mut self) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn style(&self) -> PenStyle {
        PenStyle::Tools
    }

    fn update_state(&mut self, _engine_view: &mut EngineViewMut) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn handle_event(
        &mut self,
        event: PenEvent,
        now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        match engine_view.config.pens_config.tools_config.style {
            ToolStyle::VerticalSpace => {
                self.verticalspace_tool
                    .handle_event(event, now, engine_view)
            }
            ToolStyle::OffsetCamera => self.offsetcamera_tool.handle_event(event, now, engine_view),
            ToolStyle::Zoom => self.zoom_tool.handle_event(event, now, engine_view),
            ToolStyle::Laser => self.laser_tool.handle_event(event, now, engine_view),
            ToolStyle::Snippet => self.snippet_tool.handle_event(event, now, engine_view),
        }
    }

    fn handle_animation_frame(&mut self, engine_view: &mut EngineViewMut) {
        match engine_view.config.pens_config.tools_config.style {
            ToolStyle::Laser => self.laser_tool.handle_animation_frame(engine_view),
            _ => {}
        }
    }

    fn fetch_clipboard_content(
        &self,
        engine_view: &EngineView,
    ) -> oneshot::Receiver<anyhow::Result<(Vec<(Vec<u8>, String)>, WidgetFlags)>> {
        let (sender, receiver) =
            oneshot::channel::<anyhow::Result<(Vec<(Vec<u8>, String)>, WidgetFlags)>>();

        let mut widget_flags = WidgetFlags::default();
        let mut clipboard_content = Vec::with_capacity(1);

        if let ToolStyle::Snippet = engine_view.config.pens_config.tools_config.style {
            if let Some(bounds) = self.snippet_tool.selected_bounds {
                let scale = engine_view.camera.scale_factor();
                if let Some((content, flags)) = engine_view.store.generate_area_snippet_clipboard(
                    engine_view.document.config.background,
                    bounds,
                    Some(scale),
                    true,
                ) {
                    widget_flags |= flags;
                    clipboard_content = content;
                }
            }
        }

        rayon::spawn(move || {
            if sender.send(Ok((clipboard_content, widget_flags))).is_err() {
                error!(
                    "Sending clipboard content in `Tools::fetch_clipboard_content()` failed, receiver already dropped."
                );
            }
        });

        receiver
    }
}

impl DrawableOnDoc for Tools {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        match engine_view.config.pens_config.tools_config.style {
            ToolStyle::VerticalSpace => self.verticalspace_tool.bounds_on_doc(engine_view),
            ToolStyle::OffsetCamera => self.offsetcamera_tool.bounds_on_doc(engine_view),
            ToolStyle::Zoom => self.zoom_tool.bounds_on_doc(engine_view),
            ToolStyle::Laser => self.laser_tool.bounds_on_doc(engine_view),
            ToolStyle::Snippet => self.snippet_tool.bounds_on_doc(engine_view),
        }
    }

    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        match &engine_view.config.pens_config.tools_config.style {
            ToolStyle::VerticalSpace => {
                self.verticalspace_tool.draw_on_doc(cx, engine_view)?;
            }
            ToolStyle::OffsetCamera => {
                self.offsetcamera_tool.draw_on_doc(cx, engine_view)?;
            }
            ToolStyle::Zoom => {
                self.zoom_tool.draw_on_doc(cx, engine_view)?;
            }
            ToolStyle::Laser => {
                self.laser_tool.draw_on_doc(cx, engine_view)?;
            }
            ToolStyle::Snippet => {
                self.snippet_tool.draw_on_doc(cx, engine_view)?;
            }
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}
