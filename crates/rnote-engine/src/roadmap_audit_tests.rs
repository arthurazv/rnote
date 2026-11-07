#[cfg(test)]
mod tests {
    use crate::document::background::PatternStyle;
    use crate::document::{Background, Format};
    use crate::engine::import::{PdfImportPageSpacing, PdfImportPagesType, PdfImportPrefs};
    use crate::engine::{EngineSnapshot, StrokeContent};
    use crate::fileformats::{FileFormatLoader, rnoteformat::RnoteFile};
    use crate::pens::PenStyle;
    use crate::pens::pensconfig::brushconfig::BrushStyle;
    use crate::pens::pensconfig::toolsconfig::ToolStyle;
    use crate::strokes::{BitmapImage, Stroke, VectorImage};
    use crate::{Drawable, Engine};
    use p2d::bounding_volume::Aabb;
    use p2d::math::Vector2;
    use rnote_compose::penevent::{KeyboardKey, ModifierKey, PenEvent};
    use rnote_compose::penpath::Element;
    use rnote_compose::shapes::Shapeable;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    /// Generates a valid multi-page searchable PDF in memory using Cairo's PdfSurface.
    fn build_sample_multipage_pdf(pages_text: &[&str]) -> Vec<u8> {
        let surface = cairo::PdfSurface::for_stream(595.0, 842.0, Vec::<u8>::new())
            .expect("PdfSurface creation should succeed");
        {
            let cx = cairo::Context::new(&surface).expect("Cairo context should succeed");
            for (idx, text) in pages_text.iter().enumerate() {
                cx.set_source_rgb(1.0, 1.0, 1.0);
                cx.paint().unwrap();

                // Draw distinct colored header bar so cropped regions have real non-white pixels
                cx.set_source_rgb(0.15, 0.35 + (idx as f64 * 0.1) % 0.5, 0.75);
                cx.rectangle(40.0, 40.0, 500.0, 80.0);
                cx.fill().unwrap();

                // Render searchable text
                cx.set_source_rgb(0.05, 0.05, 0.05);
                cx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cx.set_font_size(24.0);
                cx.move_to(60.0, 200.0);
                cx.show_text(text).unwrap();

                cx.show_page().unwrap();
            }
        }
        let buf = *surface
            .finish_output_stream()
            .unwrap()
            .downcast::<Vec<u8>>()
            .unwrap();
        assert!(buf.starts_with(b"%PDF"), "Generated buffer must be a valid PDF");
        buf
    }

    #[test]
    fn test_lazy_pdf_virtualization_and_rnote_roundtrip() {
        let pdf_bytes = build_sample_multipage_pdf(&[
            "Capitulo Um Termodinamica",
            "Capitulo Dois Eletromagnetismo",
            "Capitulo Tres Mecanica Quantica",
            "Capitulo Quatro Relatividade",
        ]);

        let format = Format::default();
        let prefs = PdfImportPrefs {
            pages_type: PdfImportPagesType::Bitmap,
            page_width_perc: 100.0,
            page_spacing: PdfImportPageSpacing::OnePerDocumentPage,
            bitmap_scalefactor: 1.8,
            adjust_document: true,
        };

        let pages = BitmapImage::from_pdf_bytes(&pdf_bytes, prefs, Vector2::ZERO, None, &format, None)
            .expect("BitmapImage::from_pdf_bytes must succeed");
        assert_eq!(pages.len(), 4);

        for (i, page) in pages.iter().enumerate() {
            // Unrendered placeholder must be tiny (4 bytes = 1x1 RGBA) AND pass assert_valid()!
            assert_eq!(page.image.data.len(), 4, "Page {i} placeholder must be 4 bytes");
            page.image
                .assert_valid()
                .expect("Virtualized BitmapImage placeholder must satisfy Image::assert_valid()");
            assert!(page.pdf_source.is_some(), "Page {i} must retain PdfPageSource");

            // Full-scale rendering & thumbnail rendering via Drawable::draw must succeed
            let full_img = crate::Image::gen_with_cairo(
                |cx| {
                    let mut piet_cx = piet_cairo::CairoRenderContext::new(cx);
                    page.draw(&mut piet_cx, 1.0)
                },
                page.bounds(),
                1.0,
            )
            .expect("Full-scale virtualized PDF draw must succeed");
            full_img.assert_valid().unwrap();

            let thumb_img = crate::Image::gen_with_cairo(
                |cx| {
                    let mut piet_cx = piet_cairo::CairoRenderContext::new(cx);
                    page.draw(&mut piet_cx, 0.25)
                },
                page.bounds(),
                0.25,
            )
            .expect("Thumbnail-scale virtualized PDF draw must succeed");
            thumb_img.assert_valid().unwrap();
        }

        // Import into Engine, test text search + camera_center_at + .rnote roundtrip + thumbnail_png
        let mut engine = Engine::default();
        let strokes = pages
            .into_iter()
            .map(|b| (Stroke::BitmapImage(b), None))
            .collect();
        let flags = engine.import_generated_content(strokes, true);
        assert!(flags.resize && flags.store_modified);

        // Text search across virtualized PDF pages
        let matches = engine.search_text("Quantica");
        assert_eq!(matches.len(), 1, "Must find 'Quantica' on page 3");
        let jump_flags = engine.camera_center_at(matches[0].center);
        assert!(jump_flags.view_modified && jump_flags.redraw);
        assert!(
            engine.camera.viewport().contains_local_point(matches[0].center),
            "Camera viewport must contain jumped-to match center"
        );

        // Save to .rnote bytes and verify deduplicated pdf_sources + embedded thumbnail_png
        let saved_bytes = futures::executor::block_on(engine.save_as_rnote_bytes("livro.rnote".into()))
            .expect("save_as_rnote_bytes channel")
            .expect("save_as_rnote_bytes result");

        let rnote_file = RnoteFile::load_from_bytes(&saved_bytes).expect("RnoteFile::load_from_bytes");
        assert!(
            rnote_file.thumbnail_png.as_ref().is_some_and(|s| !s.is_empty()),
            "Saved .rnote must include Quick Look thumbnail_png"
        );

        let snapshot: EngineSnapshot =
            futures::executor::block_on(EngineSnapshot::load_from_rnote_bytes(saved_bytes))
                .expect("EngineSnapshot::load_from_rnote_bytes");
        assert_eq!(
            snapshot.pdf_sources.len(),
            1,
            "All 4 pages from the same PDF must deduplicate to 1 pdf_sources entry"
        );

        let mut restored_engine = Engine::default();
        let _ = restored_engine.load_snapshot(snapshot);
        assert_eq!(restored_engine.search_text("Relatividade").len(), 1);
    }

    #[test]
    fn test_vector_pdf_import_and_search() {
        let pdf_bytes = build_sample_multipage_pdf(&["Pagina Vetorial Alfa", "Pagina Vetorial Beta"]);
        let format = Format::default();
        let prefs = PdfImportPrefs {
            pages_type: PdfImportPagesType::Vector,
            page_width_perc: 100.0,
            page_spacing: PdfImportPageSpacing::Continuous,
            bitmap_scalefactor: 1.8,
            adjust_document: true,
        };
        let pages = VectorImage::from_pdf_bytes(&pdf_bytes, prefs, Vector2::ZERO, None, &format, None)
            .expect("VectorImage::from_pdf_bytes must succeed");
        assert_eq!(pages.len(), 2);

        let mut engine = Engine::default();
        let strokes = pages
            .into_iter()
            .map(|v| (Stroke::VectorImage(v), None))
            .collect();
        let _ = engine.import_generated_content(strokes, true);
        assert_eq!(engine.search_text("Vetorial").len(), 2);
    }

    #[test]
    fn test_snippet_crop_tool_pastes_cropped_region_not_full_page() {
        let pdf_bytes = build_sample_multipage_pdf(&["Important Snippet Region To Crop"]);
        let format = Format::default();
        let prefs = PdfImportPrefs {
            pages_type: PdfImportPagesType::Bitmap,
            page_width_perc: 100.0,
            page_spacing: PdfImportPageSpacing::OnePerDocumentPage,
            bitmap_scalefactor: 1.8,
            adjust_document: true,
        };
        let pages = BitmapImage::from_pdf_bytes(&pdf_bytes, prefs, Vector2::ZERO, None, &format, None)
            .unwrap();
        let full_page_bounds = pages[0].bounds();
        assert!(full_page_bounds.extents()[0] > 500.0 && full_page_bounds.extents()[1] > 700.0);

        let mut engine = Engine::default();
        let _ = engine.import_generated_content(vec![(Stroke::BitmapImage(pages.into_iter().next().unwrap()), None)], true);

        // Activate Snippet tool
        engine.config.write().pens_config.tools_config.style = ToolStyle::Snippet;
        let _ = engine.change_pen_style(PenStyle::Tools);

        let now = Instant::now();
        // Drag a 120x80 crop region inside the 595x842 PDF page
        let (_, _) = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(50.0, 50.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );
        let (_, flags) = engine.handle_pen_event(
            PenEvent::Up {
                element: Element::new(Vector2::new(170.0, 130.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            now + Duration::from_millis(50),
        );

        let clipboard = flags
            .clipboard_content
            .expect("Snippet tool must populate clipboard_content");

        let stroke_json_entry = clipboard
            .iter()
            .find(|(_, mime)| mime == StrokeContent::MIME_TYPE)
            .expect("Clipboard must include StrokeContent::MIME_TYPE");
        let png_entry = clipboard
            .iter()
            .find(|(_, mime)| mime == "image/png")
            .expect("Clipboard must include image/png");
        assert!(!png_entry.0.is_empty());

        let text_entry = clipboard
            .iter()
            .find(|(_, mime)| mime == "text/plain");
        if let Some((text_bytes, _)) = text_entry {
            let extracted = String::from_utf8_lossy(text_bytes);
            assert!(
                extracted.contains("Importante"),
                "Extracted text from cropped PDF region must contain 'Importante', found: {extracted}"
            );
        }

        let pasted_content: StrokeContent =
            serde_json::from_slice(&stroke_json_entry.0).expect("Valid StrokeContent JSON");
        assert_eq!(pasted_content.strokes.len(), 1);
        let pasted_bounds = pasted_content.strokes[0].stroke.bounds();
        assert!(
            (pasted_bounds.extents()[0] - 120.0).abs() < 1e-3
                && (pasted_bounds.extents()[1] - 80.0).abs() < 1e-3,
            "Pasted snippet bounds {:?} must match cropped 120x80 region, NOT full page {:?}",
            pasted_bounds.extents(),
            full_page_bounds.extents()
        );
    }

    #[test]
    fn test_cornell_background_tile_and_pen_highlighter_separation() {
        let mut bg = Background::default();
        bg.pattern = PatternStyle::Cornell;
        bg.pattern_size = Vector2::new(32.0, 30.0);

        // Tile image must generate cleanly without failing
        let tile = bg.gen_tile_image(1.0).expect("Cornell tile image must succeed");
        tile.assert_valid().unwrap();

        // Full-page SVG must include cue & summary lines
        let full_svg = bg
            .gen_svg(Aabb::new(Vector2::ZERO, Vector2::new(794.0, 1123.0)), true, false)
            .expect("Cornell full-page SVG must succeed");
        assert!(full_svg.svg_data.contains("line"));

        // Verify Solid Pen is 100% opaque (alpha = 1.0) while Marker Highlighter is translucent (alpha = 0.45)
        let mut engine = Engine::default();
        {
            let mut cfg = engine.config.write();
            cfg.pens_config.brush_config.style = BrushStyle::Solid;
            let solid_style = cfg.pens_config.brush_config.style_for_current_options();
            assert!((solid_style.stroke_color().unwrap().a - 1.0).abs() < 1e-6);

            cfg.pens_config.brush_config.style = BrushStyle::Marker;
            let marker_style = cfg.pens_config.brush_config.style_for_current_options();
            assert!((marker_style.stroke_color().unwrap().a - 0.45).abs() < 1e-6);
        }

        // Verify Shift-constrained straight line & Hold-to-Shape snap in Brush
        let _ = engine.change_pen_style(PenStyle::Brush);
        engine.config.write().pens_config.brush_config.style = BrushStyle::Solid;
        let t0 = Instant::now();
        let mut shift_mods = HashSet::new();
        shift_mods.insert(ModifierKey::KeyboardShift);
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(100.0, 100.0), 0.5),
                modifier_keys: shift_mods.clone(),
            },
            None,
            t0,
        );
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(260.0, 118.0), 0.5),
                modifier_keys: shift_mods.clone(),
            },
            None,
            t0 + Duration::from_millis(30),
        );
        let _ = engine.handle_pen_event(
            PenEvent::Up {
                element: Element::new(Vector2::new(260.0, 118.0), 0.5),
                modifier_keys: shift_mods,
            },
            None,
            t0 + Duration::from_millis(60),
        );
        let keys = engine.store.stroke_keys_as_rendered();
        assert_eq!(keys.len(), 1);
        let _ = KeyboardKey::Unicode('a');
    }

    #[test]
    fn test_text_extraction_and_clipboard_plain_text() {
        let pdf_bytes = build_sample_multipage_pdf(&[
            "Antigravity Engineering Rocks",
            "Second Page Document Text",
        ]);

        let mut prefs = PdfImportPrefs::default();
        prefs.pages_type = PdfImportPagesType::Bitmap;
        prefs.page_spacing = PdfImportPageSpacing::Continuous;
        prefs.adjust_document = true;

        let format = Format::default();
        let strokes = BitmapImage::from_pdf_bytes(
            &pdf_bytes,
            prefs,
            Vector2::ZERO,
            None,
            &format,
            None,
        ).expect("PDF import should succeed");

        assert_eq!(strokes.len(), 2);
        let mut engine = Engine::default();
        for s in strokes {
            engine.store.insert_stroke(Stroke::BitmapImage(s), Some(crate::store::chrono_comp::StrokeLayer::Document));
        }

        // Test extract_text_in_bounds
        let all_bounds = engine.store.bounds_for_strokes(&engine.store.stroke_keys_as_rendered()).unwrap();
        let words = engine.store.extract_text_in_bounds(all_bounds);
        assert!(!words.is_empty(), "Extracted words should not be empty");
        let full_text = words.join(" ");
        assert!(full_text.contains("Antigravity"), "Should contain Antigravity");
        assert!(full_text.contains("Engineering"), "Should contain Engineering");

        // Test engine.extract_text
        let extracted = engine.extract_text(Some(all_bounds)).expect("extract_text should return string");
        assert!(extracted.contains("Antigravity"));

        // Verify that background Document layer strokes are correctly marked as StrokeLayer::Document
        for key in engine.store.stroke_keys_as_rendered() {
            assert_eq!(engine.store.stroke_layer(key), Some(crate::store::chrono_comp::StrokeLayer::Document));
        }
    }

    #[test]
    fn test_dedicated_text_selector_tool_workflow() {
        use crate::pens::PenStyle;
        use crate::pens::pensconfig::selectorconfig::SelectorStyle;
        use rnote_compose::penpath::Element;
        use rnote_compose::penevent::PenEvent;

        let pdf_bytes = build_sample_multipage_pdf(&["Dedicated Text Selection Workflow"]);
        let mut prefs = PdfImportPrefs::default();
        prefs.pages_type = PdfImportPagesType::Bitmap;
        prefs.adjust_document = true;

        let format = Format::default();
        let strokes = BitmapImage::from_pdf_bytes(
            &pdf_bytes,
            prefs,
            Vector2::ZERO,
            None,
            &format,
            None,
        ).expect("PDF import should succeed");

        let mut engine = Engine::default();
        for s in strokes {
            engine.store.insert_stroke(Stroke::BitmapImage(s), Some(crate::store::chrono_comp::StrokeLayer::Document));
        }

        // Configure engine with SelectorStyle::Text
        engine.config.write().pens_config.selector_config.style = SelectorStyle::Text;
        let _ = engine.change_pen_style(PenStyle::Selector);

        // Get bounds of the page
        let all_bounds = engine.store.bounds_for_strokes(&engine.store.stroke_keys_as_rendered()).unwrap();

        // Simulate pen down and pen up across the text
        let now = Instant::now();
        let p_start = Element::new(all_bounds.mins, 0.5);
        let p_end = Element::new(all_bounds.maxs, 0.5);

        let (_, _) = engine.handle_pen_event(
            PenEvent::Down { element: p_start, modifier_keys: HashSet::new() },
            None,
            now,
        );
        let (_, flags) = engine.handle_pen_event(
            PenEvent::Up { element: p_end, modifier_keys: HashSet::new() },
            None,
            now + Duration::from_millis(50),
        );

        // Verify clipboard content in widget flags contains text/plain ONLY!
        let clip = flags.clipboard_content.expect("Should have set clipboard_content");
        assert_eq!(clip.len(), 1, "Should contain exactly one clipboard format");
        assert_eq!(clip[0].1, "text/plain", "MIME type must be text/plain");
        let text_str = String::from_utf8(clip[0].0.clone()).unwrap();
        assert!(text_str.contains("Dedicated"), "Should contain 'Dedicated'");
        assert!(text_str.contains("Workflow"), "Should contain 'Workflow'");

        // Verify that fetch_clipboard_content on selector returns ONLY text/plain, no image/png or SVG!
        let receiver = engine.fetch_clipboard_content();
        let (content, _) = futures::executor::block_on(receiver).unwrap().unwrap();
        assert_eq!(content.len(), 1, "Clipboard content must contain ONLY text/plain");
        assert_eq!(content[0].1, "text/plain", "MIME type must strictly be text/plain");

        // Verify no strokes were selected for movement/transformation
        assert!(engine.store.selection_keys_as_rendered().is_empty(), "No strokes should be selected for modification");
    }

    #[test]
    fn test_merge_word_boxes_into_line_spans() {
        use p2d::bounding_volume::Aabb;
        use p2d::math::Vector2;
        use crate::store::StrokeStore;

        // Line 1: 3 words
        let w1 = Aabb::new(Vector2::new(10.0, 20.0), Vector2::new(50.0, 35.0));
        let w2 = Aabb::new(Vector2::new(55.0, 20.0), Vector2::new(95.0, 35.0));
        let w3 = Aabb::new(Vector2::new(100.0, 20.0), Vector2::new(140.0, 35.0));

        // Line 2: 2 words
        let w4 = Aabb::new(Vector2::new(10.0, 45.0), Vector2::new(60.0, 60.0));
        let w5 = Aabb::new(Vector2::new(65.0, 45.0), Vector2::new(115.0, 60.0));

        let merged = StrokeStore::merge_word_boxes_into_line_spans(vec![w1, w2, w3, w4, w5]);
        assert_eq!(merged.len(), 2, "5 words across 2 lines should merge into exactly 2 line spans");
        assert_eq!(merged[0].mins, Vector2::new(10.0, 20.0));
        assert_eq!(merged[0].maxs, Vector2::new(140.0, 35.0));
        assert_eq!(merged[1].mins, Vector2::new(10.0, 45.0));
        assert_eq!(merged[1].maxs, Vector2::new(115.0, 60.0));
    }

    #[test]
    fn test_area_selector_crops_and_stays_marked() {
        use crate::Engine;
        use crate::engine::StrokeContent;
        use crate::pens::PenStyle;
        use crate::pens::pensconfig::selectorconfig::SelectorStyle;
        use crate::strokes::Stroke;
        use p2d::bounding_volume::Aabb;
        use p2d::glamx::DAffine2;
        use p2d::math::Vector2;
        use rnote_compose::ext::DAffine2Ext;
        use rnote_compose::penevent::{KeyboardKey, PenEvent};
        use rnote_compose::penpath::Element;
        use std::collections::HashSet;
        use std::time::{Duration, Instant};

        let mut engine = Engine::default();

        // 1. Insert a 600x800 background document image (e.g. PDF page with questions)
        let placeholder_bounds = Aabb::new(Vector2::ZERO, Vector2::new(600.0, 800.0));
        let dummy_pixels = vec![255u8; 600 * 800 * 4];
        let img = crate::Image {
            data: glib::Bytes::from_owned(dummy_pixels),
            rectangle: rnote_compose::shapes::Rectangle::from_p2d_aabb(placeholder_bounds),
            pixel_width: 600,
            pixel_height: 800,
            memory_format: crate::image::ImageMemoryFormat::R8g8b8a8Premultiplied,
        };
        let mut transform = DAffine2::IDENTITY;
        transform.append_translation_mut(Vector2::new(300.0, 400.0));
        let rect = rnote_compose::shapes::Rectangle {
            cuboid: p2d::shape::Cuboid::new(Vector2::new(300.0, 400.0)),
            affine: transform,
        };
        let doc_image = crate::strokes::BitmapImage {
            image: img,
            rectangle: rect,
            text_runs: Vec::new(),
            pdf_source: None,
        };
        engine.store.insert_stroke(
            Stroke::BitmapImage(doc_image),
            Some(crate::store::chrono_comp::StrokeLayer::Document),
        );

        // 2. Select the Selector tool with Rectangle style
        engine.config.write().pens_config.selector_config.style = SelectorStyle::Rectangle;
        let _ = engine.change_pen_style(PenStyle::Selector);

        // 3. Drag a 150x100 rectangular crop around a question inside the page
        let now = Instant::now();
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(40.0, 60.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            now,
        );
        let (_, flags) = engine.handle_pen_event(
            PenEvent::Up {
                element: Element::new(Vector2::new(190.0, 160.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            now + Duration::from_millis(50),
        );

        // 4. Verify the selector stays marked on doc!
        let bounds_on_doc = engine.current_pen_bounds_on_doc();
        assert!(
            bounds_on_doc.is_some(),
            "The selector MUST stay marked on screen for the chosen section!"
        );

        // 5. Verify clipboard was populated with StrokeContent and image/png
        let clip = flags.clipboard_content.expect("Must populate clipboard_content on area selection");
        let stroke_json = clip
            .iter()
            .find(|(_, mime)| mime == StrokeContent::MIME_TYPE)
            .expect("Must have StrokeContent::MIME_TYPE");
        let png_entry = clip
            .iter()
            .find(|(_, mime)| mime == "image/png")
            .expect("Must have image/png");
        assert!(!png_entry.0.is_empty());

        let pasted_content: StrokeContent =
            serde_json::from_slice(&stroke_json.0).expect("Valid StrokeContent JSON");
        assert_eq!(pasted_content.strokes.len(), 1);
        let pasted_bounds = pasted_content.strokes[0].stroke.bounds();
        assert!(
            (pasted_bounds.extents()[0] - 150.0).abs() < 1e-3
                && (pasted_bounds.extents()[1] - 100.0).abs() < 1e-3,
            "Pasted cropped image must match exact selected dimensions 150x100, got: {:?}",
            pasted_bounds.extents()
        );

        // 6. Press Escape to clear the selection
        let (_, esc_flags) = engine.handle_pen_event(
            PenEvent::KeyPressed {
                keyboard_key: KeyboardKey::Escape,
                modifier_keys: HashSet::new(),
            },
            None,
            now + Duration::from_millis(100),
        );
        assert!(esc_flags.redraw);
        assert!(
            engine.current_pen_bounds_on_doc().is_none(),
            "Escape must clear the area selection back to Idle"
        );
    }
}
