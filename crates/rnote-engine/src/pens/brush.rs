// Imports
use super::PenBehaviour;
use super::PenStyle;
use super::pensconfig::brushconfig::BrushStyle;
use crate::engine::{EngineView, EngineViewMut};
use crate::store::StrokeKey;
use crate::strokes::BrushStroke;
use crate::strokes::Stroke;
use crate::{DrawableOnDoc, WidgetFlags};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::Constraints;
use rnote_compose::Style;
use rnote_compose::builders::buildable::{Buildable, BuilderCreator, BuilderProgress};
use rnote_compose::builders::{
    PenPathBuilderType, PenPathCurvedBuilder, PenPathModeledBuilder, PenPathSimpleBuilder,
};
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::penevent::{ModifierKey, PenEvent, PenProgress};
use rnote_compose::penpath::{Element, Segment};
use rnote_compose::shapes::Shape;
use crate::strokes::ShapeStroke;
use std::time::{Duration, Instant};

#[derive(Debug)]
enum BrushState {
    Idle,
    Drawing {
        path_builder: Box<dyn Buildable<Emit = Segment>>,
        current_stroke_key: StrokeKey,
        preview_style: Style,
        start_pos: Vector2,
        raw_points: Vec<Vector2>,
        last_move_pos: Vector2,
        last_move_time: Instant,
        snapped_shape: Option<Shape>,
    },
}

#[derive(Debug)]
pub struct Brush {
    state: BrushState,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            state: BrushState::Idle,
        }
    }
}

impl PenBehaviour for Brush {
    fn init(&mut self, _engine_view: &EngineView) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn deinit(&mut self) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn style(&self) -> PenStyle {
        PenStyle::Brush
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
        let mut widget_flags = WidgetFlags::default();

        let event_result = match (&mut self.state, event) {
            (BrushState::Idle, PenEvent::Down { element, .. }) => {
                if !element.filter_by_bounds(
                    engine_view
                        .document
                        .bounds()
                        .loosened(Self::INPUT_OVERSHOOT),
                ) {
                    #[cfg(feature = "ui")]
                    {
                        if engine_view.config.pens_config.brush_config.style == BrushStyle::Marker {
                            play_marker_sound(engine_view);
                        } else {
                            trigger_brush_sound(engine_view);
                        }
                    }

                    engine_view
                        .config
                        .pens_config
                        .brush_config
                        .new_style_seeds();

                    let preview_style = Self::get_preview_style(&engine_view.as_im());
                    let brushstroke =
                        Stroke::BrushStroke(BrushStroke::new(element, preview_style.clone()));

                    let current_stroke_key = engine_view.store.insert_stroke(
                        brushstroke,
                        Some(
                            engine_view
                                .config
                                .pens_config
                                .brush_config
                                .layer_for_current_options(),
                        ),
                    );

                    engine_view.store.regenerate_rendering_for_stroke(
                        current_stroke_key,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    self.state = BrushState::Drawing {
                        path_builder: new_builder(
                            engine_view.config.pens_config.brush_config.builder_type,
                            element,
                            now,
                        ),
                        current_stroke_key,
                        preview_style,
                        start_pos: element.pos,
                        raw_points: vec![element.pos],
                        last_move_pos: element.pos,
                        last_move_time: now,
                        snapped_shape: None,
                    };

                    EventResult {
                        handled: true,
                        propagate: EventPropagation::Stop,
                        progress: PenProgress::InProgress,
                    }
                } else {
                    EventResult {
                        handled: false,
                        propagate: EventPropagation::Proceed,
                        progress: PenProgress::Idle,
                    }
                }
            }
            (BrushState::Idle, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (
                BrushState::Drawing {
                    current_stroke_key,
                    snapped_shape,
                    ..
                },
                PenEvent::Cancel,
            ) => {
                if snapped_shape.is_none() {
                    if let Some(Stroke::BrushStroke(brushstroke)) =
                        engine_view.store.get_stroke_mut(*current_stroke_key)
                    {
                        brushstroke.style = engine_view
                            .config
                            .pens_config
                            .brush_config
                            .style_for_current_options();
                    }
                } else if let Some(Stroke::ShapeStroke(shapestroke)) =
                    engine_view.store.get_stroke_mut(*current_stroke_key)
                {
                    shapestroke.style = engine_view
                        .config
                        .pens_config
                        .brush_config
                        .style_for_current_options();
                }

                // Finish up the last stroke
                engine_view
                    .store
                    .update_geometry_for_stroke(*current_stroke_key);
                engine_view.store.regenerate_rendering_for_stroke_threaded(
                    engine_view.tasks_tx.clone(),
                    *current_stroke_key,
                    engine_view.camera.viewport(),
                    engine_view.camera.image_scale(),
                );
                widget_flags |= engine_view
                    .document
                    .resize_autoexpand(engine_view.store, engine_view.camera);

                self.state = BrushState::Idle;

                widget_flags |= engine_view.store.record(Instant::now());
                widget_flags.store_modified = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (
                BrushState::Drawing {
                    path_builder,
                    current_stroke_key,
                    preview_style,
                    start_pos,
                    raw_points,
                    last_move_pos,
                    last_move_time,
                    snapped_shape,
                },
                mut pen_event,
            ) => {
                // If Shift key is pressed, constrain movement to horizontal or vertical axis from start_pos
                match &mut pen_event {
                    PenEvent::Down {
                        element,
                        modifier_keys,
                    }
                    | PenEvent::Up {
                        element,
                        modifier_keys,
                    } => {
                        if modifier_keys.contains(&ModifierKey::KeyboardShift) {
                            let delta = element.pos - *start_pos;
                            if delta.x.abs() >= delta.y.abs() {
                                element.pos.y = start_pos.y;
                            } else {
                                element.pos.x = start_pos.x;
                            }
                        }
                    }
                    _ => {}
                }

                // Track motion points for hold-to-shape recognition
                if let PenEvent::Down { element, .. } = &pen_event {
                    raw_points.push(element.pos);
                    let dist = (element.pos - *last_move_pos).length();
                    if dist > 8.0 {
                        *last_move_pos = element.pos;
                        *last_move_time = now;
                    }
                }

                // If held still for >= 420ms while pen is down, snap live into shape
                if snapped_shape.is_none()
                    && raw_points.len() >= 6
                    && now.duration_since(*last_move_time) >= Duration::from_millis(420)
                {
                    if let Some(shape) = crate::pens::shaperecognizer::recognize_shape(raw_points) {
                        *snapped_shape = Some(shape.clone());

                        #[cfg(feature = "ui")]
                        trigger_brush_sound(engine_view);

                        if let Some(stroke) = engine_view.store.get_stroke_mut(*current_stroke_key) {
                            *stroke = Stroke::ShapeStroke(ShapeStroke::new(shape, preview_style.clone()));
                            widget_flags.store_modified = true;
                        }

                        engine_view.store.update_geometry_for_stroke(*current_stroke_key);
                        engine_view.store.regenerate_rendering_for_stroke_threaded(
                            engine_view.tasks_tx.clone(),
                            *current_stroke_key,
                            engine_view.camera.viewport(),
                            engine_view.camera.image_scale(),
                        );
                    }
                }

                let builder_result =
                    path_builder.handle_event(pen_event, now, Constraints::default());
                let handled = builder_result.handled;
                let propagate = builder_result.propagate;

                let progress = match builder_result.progress {
                    BuilderProgress::InProgress => {
                        #[cfg(feature = "ui")]
                        {
                            if engine_view.config.pens_config.brush_config.style
                                != BrushStyle::Marker
                            {
                                trigger_brush_sound(engine_view);
                            }
                        }

                        PenProgress::InProgress
                    }
                    BuilderProgress::EmitContinue(segments) => {
                        #[cfg(feature = "ui")]
                        {
                            if engine_view.config.pens_config.brush_config.style
                                != BrushStyle::Marker
                            {
                                trigger_brush_sound(engine_view);
                            }
                        }

                        let n_segments = segments.len();

                        // Only continue extending the brush path if we have not snapped to a shape
                        if snapped_shape.is_none() && n_segments != 0 {
                            if let Some(Stroke::BrushStroke(brushstroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                brushstroke.extend_w_segments(segments);
                                widget_flags.store_modified = true;
                            }

                            engine_view.store.append_rendering_last_segments(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                n_segments,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        PenProgress::InProgress
                    }
                    BuilderProgress::Finished(segments) => {
                        let final_style = engine_view
                            .config
                            .pens_config
                            .brush_config
                            .style_for_current_options();

                        // If not already snapped during Down hold, check if held at release (>= 380ms)
                        if snapped_shape.is_none()
                            && raw_points.len() >= 6
                            && now.duration_since(*last_move_time) >= Duration::from_millis(380)
                        {
                            if let Some(shape) = crate::pens::shaperecognizer::recognize_shape(raw_points) {
                                *snapped_shape = Some(shape.clone());

                                if let Some(stroke) = engine_view.store.get_stroke_mut(*current_stroke_key) {
                                    *stroke = Stroke::ShapeStroke(ShapeStroke::new(shape, final_style.clone()));
                                    widget_flags.store_modified = true;
                                }
                            }
                        }

                        if snapped_shape.is_some() {
                            if let Some(Stroke::ShapeStroke(shapestroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                shapestroke.style = final_style;
                                widget_flags.store_modified = true;
                            }
                        } else {
                            let n_segments = segments.len();

                            if n_segments != 0 {
                                if let Some(Stroke::BrushStroke(brushstroke)) =
                                    engine_view.store.get_stroke_mut(*current_stroke_key)
                                {
                                    brushstroke.extend_w_segments(segments);
                                    widget_flags.store_modified = true;
                                }

                                engine_view.store.append_rendering_last_segments(
                                    engine_view.tasks_tx.clone(),
                                    *current_stroke_key,
                                    n_segments,
                                    engine_view.camera.viewport(),
                                    engine_view.camera.image_scale(),
                                );
                            }

                            if let Some(Stroke::BrushStroke(brushstroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                brushstroke.style = final_style;
                            }
                        }

                        // Finish up the stroke
                        engine_view
                            .store
                            .update_geometry_for_stroke(*current_stroke_key);
                        engine_view.store.regenerate_rendering_for_stroke_threaded(
                            engine_view.tasks_tx.clone(),
                            *current_stroke_key,
                            engine_view.camera.viewport(),
                            engine_view.camera.image_scale(),
                        );
                        widget_flags |= engine_view
                            .document
                            .resize_autoexpand(engine_view.store, engine_view.camera);

                        self.state = BrushState::Idle;

                        widget_flags |= engine_view.store.record(Instant::now());
                        widget_flags.store_modified = true;

                        PenProgress::Finished
                    }
                };

                EventResult {
                    handled,
                    propagate,
                    progress,
                }
            }
        };

        (event_result, widget_flags)
    }
}

impl DrawableOnDoc for Brush {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        let style = engine_view
            .config
            .pens_config
            .brush_config
            .style_for_current_options();

        match &self.state {
            BrushState::Idle => None,
            BrushState::Drawing {
                path_builder,
                snapped_shape,
                ..
            } => {
                if snapped_shape.is_some() {
                    None
                } else {
                    path_builder.bounds(&style, engine_view.camera.zoom())
                }
            }
        }
    }

    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        match &self.state {
            BrushState::Idle => {}
            BrushState::Drawing {
                path_builder,
                preview_style,
                snapped_shape,
                ..
            } => {
                if snapped_shape.is_none() {
                    match engine_view.config.pens_config.brush_config.style {
                        BrushStyle::Marker => {
                            // Don't draw the marker, as the pen would render on top of other strokes, while the stroke itself would render underneath them.
                        }
                        BrushStyle::Solid | BrushStyle::Textured => {
                            path_builder.draw_styled(
                                cx,
                                preview_style,
                                engine_view.camera.total_zoom(),
                            );
                        }
                    }
                }
            }
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

impl Brush {
    const INPUT_OVERSHOOT: f64 = 30.0;

    fn get_preview_style(engine_view: &EngineView) -> Style {
        let mut style = engine_view
            .config
            .pens_config
            .brush_config
            .style_for_current_options();

        if engine_view.config.pens_config.brush_config.style == BrushStyle::Marker {
            if let Some(mut stroke_color) = style.stroke_color() {
                stroke_color.a = 0.45;
                style.set_stroke_color(stroke_color);
            }
        } else if let Some(mut stroke_color) = style.stroke_color() {
            stroke_color.a = 1.0;
            style.set_stroke_color(stroke_color);
        }

        style
    }
}

#[cfg(feature = "ui")]
fn play_marker_sound(engine_view: &mut EngineViewMut) {
    if let Some(audioplayer) = engine_view.audioplayer {
        audioplayer.play_random_marker_sound();
    }
}

#[cfg(feature = "ui")]
fn trigger_brush_sound(engine_view: &mut EngineViewMut) {
    if let Some(audioplayer) = engine_view.audioplayer.as_mut() {
        audioplayer.trigger_random_brush_sound();
    }
}

fn new_builder(
    builder_type: PenPathBuilderType,
    element: Element,
    now: Instant,
) -> Box<dyn Buildable<Emit = Segment>> {
    match builder_type {
        PenPathBuilderType::Simple => Box::new(PenPathSimpleBuilder::start(element, now)),
        PenPathBuilderType::Curved => Box::new(PenPathCurvedBuilder::start(element, now)),
        PenPathBuilderType::Modeled => Box::new(PenPathModeledBuilder::start(element, now)),
    }
}
