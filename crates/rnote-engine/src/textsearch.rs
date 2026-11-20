use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::hayro_syntax::page::Page;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, interpret_page,
};
use kurbo::{BezPath, Rect};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use rnote_compose::ext::{AabbExt, DAffine2Ext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextRun {
    pub text: String,
    /// Normalized bounding box [min_x, min_y, max_x, max_y] relative to page dimensions (0.0 .. 1.0)
    pub bbox: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    /// Bounding box of the match in document coordinates
    pub bounds: Aabb,
    /// The matching text snippet
    pub text: String,
    /// Center of the match for camera navigation
    pub center: Vector2,
}

struct TextExtractDevice {
    runs: Vec<TextRun>,
    current_word: String,
    current_min_x: f64,
    current_min_y: f64,
    current_max_x: f64,
    current_max_y: f64,
}

impl TextExtractDevice {
    fn new() -> Self {
        Self {
            runs: Vec::new(),
            current_word: String::new(),
            current_min_x: f64::MAX,
            current_min_y: f64::MAX,
            current_max_x: f64::MIN,
            current_max_y: f64::MIN,
        }
    }

    fn flush_word(&mut self) {
        if !self.current_word.trim().is_empty() {
            self.runs.push(TextRun {
                text: std::mem::take(&mut self.current_word),
                bbox: [
                    self.current_min_x,
                    self.current_min_y,
                    self.current_max_x,
                    self.current_max_y,
                ],
            });
        } else {
            self.current_word.clear();
        }
        self.current_min_x = f64::MAX;
        self.current_min_y = f64::MAX;
        self.current_max_x = f64::MIN;
        self.current_max_y = f64::MIN;
    }
}

impl<'a> Device<'a> for TextExtractDevice {
    fn draw_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, 'a>,
        props: DrawProps<'a>,
        _draw_mode: &DrawMode,
    ) {
        for glyph in glyph_run.glyphs() {
            if let Some(unicode) = glyph.as_unicode() {
                let s = match unicode {
                    BfString::Char(c) => c.to_string(),
                    BfString::String(s) => s,
                };

                let transform = props.transform * glyph.transform();
                let p0 = transform * kurbo::Point::new(0.0, 0.0);
                let adv = match &**glyph {
                    hayro::hayro_interpret::font::Glyph::Outline(og) => {
                        og.advance_width().unwrap_or(500.0) as f64 / 1000.0
                    }
                    hayro::hayro_interpret::font::Glyph::Type3(_) => 0.5,
                };
                let p1 = transform * kurbo::Point::new(adv, 1.0);

                let g_min_x = p0.x.min(p1.x);
                let g_max_x = p0.x.max(p1.x);
                let g_min_y = p0.y.min(p1.y);
                let g_max_y = p0.y.max(p1.y);

                for ch in s.chars() {
                    if ch.is_whitespace() {
                        self.flush_word();
                    } else {
                        self.current_word.push(ch);
                        self.current_min_x = self.current_min_x.min(g_min_x);
                        self.current_min_y = self.current_min_y.min(g_min_y);
                        self.current_max_x = self.current_max_x.max(g_max_x);
                        self.current_max_y = self.current_max_y.max(g_max_y);
                    }
                }
            }
        }
        self.flush_word();
    }

    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'_>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'_, '_>, _: ImageDrawProps<'_>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Extracts text words and their normalized bounding boxes [0..1] from a parsed PDF page.
pub fn extract_text_from_pdf_page(page: &Page<'_>) -> Vec<TextRun> {
    let mut device = TextExtractDevice::new();
    let cache = InterpreterCache::new();
    let settings = InterpreterSettings::default();

    let (w, h) = page.render_dimensions();
    let width = w as f64;
    let height = h as f64;
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }

    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, width, height),
        &cache,
        page.xref(),
        settings,
    );

    interpret_page(page, &mut context, &mut device);
    device.flush_word();

    device
        .runs
        .into_iter()
        .map(|mut run| {
            run.bbox[0] = (run.bbox[0] / width).clamp(0.0, 1.0);
            run.bbox[1] = (run.bbox[1] / height).clamp(0.0, 1.0);
            run.bbox[2] = (run.bbox[2] / width).clamp(0.0, 1.0);
            run.bbox[3] = (run.bbox[3] / height).clamp(0.0, 1.0);
            run
        })
        .collect()
}

/// Converts a normalized bounding box [min_x, min_y, max_x, max_y] (in 0..1 range)
/// to document coordinates using the stroke's Rectangle.
pub fn text_run_to_doc_bounds(
    run_bbox: &[f64; 4],
    rect: &rnote_compose::shapes::Rectangle,
) -> Aabb {
    let half_w = rect.cuboid.half_extents[0];
    let half_h = rect.cuboid.half_extents[1];

    let p_min_local = Vector2::new(
        (run_bbox[0] - 0.5) * 2.0 * half_w,
        (run_bbox[1] - 0.5) * 2.0 * half_h,
    );
    let p_max_local = Vector2::new(
        (run_bbox[2] - 0.5) * 2.0 * half_w,
        (run_bbox[3] - 0.5) * 2.0 * half_h,
    );

    let corners = [
        rect.affine.transform_point2(p_min_local),
        rect.affine.transform_point2(Vector2::new(p_max_local[0], p_min_local[1])),
        rect.affine.transform_point2(p_max_local),
        rect.affine.transform_point2(Vector2::new(p_min_local[0], p_max_local[1])),
    ];

    let min_x = corners.iter().map(|c| c[0]).fold(f64::INFINITY, f64::min);
    let min_y = corners.iter().map(|c| c[1]).fold(f64::INFINITY, f64::min);
    let max_x = corners.iter().map(|c| c[0]).fold(f64::NEG_INFINITY, f64::max);
    let max_y = corners.iter().map(|c| c[1]).fold(f64::NEG_INFINITY, f64::max);

    Aabb::new(Vector2::new(min_x, min_y), Vector2::new(max_x, max_y))
}

/// Searches runs for query matches, returning SearchMatch objects with document coordinates.
pub fn search_image_runs(
    runs: &[TextRun],
    rect: &rnote_compose::shapes::Rectangle,
    query: &str,
) -> Vec<SearchMatch> {
    if runs.is_empty() || query.trim().is_empty() {
        return Vec::new();
    }

    let mut full_text = String::new();
    let mut word_spans = Vec::with_capacity(runs.len());

    for (i, run) in runs.iter().enumerate() {
        if !full_text.is_empty() {
            full_text.push(' ');
        }
        let start = full_text.len();
        full_text.push_str(&run.text);
        let end = full_text.len();
        word_spans.push((start, end, i));
    }

    let lower_full = full_text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut matches = Vec::new();

    for (match_start, _) in lower_full.match_indices(&lower_query) {
        let match_end = match_start + lower_query.len();

        let mut min_norm_x = f64::INFINITY;
        let mut min_norm_y = f64::INFINITY;
        let mut max_norm_x = f64::NEG_INFINITY;
        let mut max_norm_y = f64::NEG_INFINITY;
        let mut matched_runs = Vec::new();

        for &(w_start, w_end, run_idx) in &word_spans {
            if w_end > match_start && w_start < match_end {
                matched_runs.push(run_idx);
                let bbox = &runs[run_idx].bbox;
                min_norm_x = min_norm_x.min(bbox[0]);
                min_norm_y = min_norm_y.min(bbox[1]);
                max_norm_x = max_norm_x.max(bbox[2]);
                max_norm_y = max_norm_y.max(bbox[3]);
            }
        }

        if !matched_runs.is_empty() {
            let combined_bbox = [min_norm_x, min_norm_y, max_norm_x, max_norm_y];
            let bounds = text_run_to_doc_bounds(&combined_bbox, rect);
            let snippet = full_text[match_start..match_end].to_string();
            let center = bounds.center();

            matches.push(SearchMatch {
                bounds,
                text: snippet,
                center,
            });
        }
    }

    matches
}

/// Searches a TextStroke for query matches, returning SearchMatch objects with document coordinates.
pub fn search_text_stroke(
    stroke: &crate::strokes::textstroke::TextStroke,
    query: &str,
) -> Vec<SearchMatch> {
    if stroke.text.trim().is_empty() || query.trim().is_empty() {
        return Vec::new();
    }

    let lower_text = stroke.text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut matches = Vec::new();

    let text_layout = match stroke
        .text_style
        .build_text_layout(&mut piet_cairo::CairoText::new(), stroke.text.clone())
    {
        Ok(layout) => layout,
        Err(_) => return Vec::new(),
    };

    for (match_start, _) in lower_text.match_indices(&lower_query) {
        let match_end = match_start + lower_query.len();
        use piet::TextLayout;
        let rects = text_layout.rects_for_range(match_start..match_end);
        if !rects.is_empty() {
            let mut union_bounds: Option<Aabb> = None;
            for r in rects {
                let aabb = stroke.affine.transform_aabb(Aabb::from_kurbo_rect(r));
                union_bounds = match union_bounds {
                    Some(u) => Some(u.merged(&aabb)),
                    None => Some(aabb),
                };
            }
            if let Some(bounds) = union_bounds {
                let snippet = stroke.text[match_start..match_end].to_string();
                let center = bounds.center();
                matches.push(SearchMatch {
                    bounds,
                    text: snippet,
                    center,
                });
            }
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use p2d::glamx::DAffine2;
    use rnote_compose::shapes::Rectangle;

    #[test]
    fn test_search_image_runs_single_and_multiword() {
        let runs = vec![
            TextRun {
                text: "Introdução".to_string(),
                bbox: [0.1, 0.1, 0.3, 0.15],
            },
            TextRun {
                text: "ao".to_string(),
                bbox: [0.32, 0.1, 0.36, 0.15],
            },
            TextRun {
                text: "Cálculo".to_string(),
                bbox: [0.38, 0.1, 0.55, 0.15],
            },
        ];

        let rect = Rectangle {
            cuboid: p2d::shape::Cuboid::new(Vector2::new(500.0, 700.0)),
            affine: DAffine2::from_translation(Vector2::new(500.0, 700.0)),
        };

        // Single word search (case-insensitive)
        let res = search_image_runs(&runs, &rect, "cálculo");
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].text, "Cálculo");

        // Multi-word search
        let res2 = search_image_runs(&runs, &rect, "ao cálculo");
        assert_eq!(res2.len(), 1);
        assert_eq!(res2[0].text, "ao Cálculo");

        // Partial word search
        let res3 = search_image_runs(&runs, &rect, "trod");
        assert_eq!(res3.len(), 1);
        assert_eq!(res3[0].text, "trod");

        // No match
        let res4 = search_image_runs(&runs, &rect, "álgebra");
        assert_eq!(res4.len(), 0);
    }
}
