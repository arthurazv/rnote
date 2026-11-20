// Imports
use super::resize::{ImageSizeOption, calculate_resize_ratio};
use super::{Content, Stroke};
use crate::Drawable;
use crate::Image;
use crate::document::Format;
use crate::engine::import::{PdfImportPageSpacing, PdfImportPrefs};
use anyhow::anyhow;
use hayro::{hayro_interpret, hayro_syntax, vello_cpu};
use kurbo::Shape;
use p2d::bounding_volume::Aabb;
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rnote_compose::Transformable;
use rnote_compose::ext::{AabbExt, DAffine2Ext};
use rnote_compose::shapes::Rectangle;
use rnote_compose::shapes::Shapeable;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Mutex};

const PDF_DOC_REGISTRY_CAP: usize = 12;
const PDF_PAGE_CACHE_CAP: usize = 32;
const PDF_THUMB_CACHE_CAP: usize = 128;

static PDF_DOC_REGISTRY: Lazy<Mutex<VecDeque<(u64, Arc<Vec<u8>>)>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(PDF_DOC_REGISTRY_CAP)));

type RenderedPageEntry = ((u64, usize, u32), (u32, u32, glib::Bytes));

static PDF_PAGE_CACHE: Lazy<Mutex<VecDeque<RenderedPageEntry>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(PDF_PAGE_CACHE_CAP)));

static PDF_THUMB_CACHE: Lazy<Mutex<VecDeque<RenderedPageEntry>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(PDF_THUMB_CACHE_CAP)));

pub fn compute_pdf_doc_hash(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    // Hash up to first 64 KB + middle 64 KB + last 64 KB for O(1) speed on 200 MB books,
    // or full slice if <= 192 KB.
    if bytes.len() <= 196_608 {
        bytes.hash(&mut hasher);
    } else {
        bytes[..65_536].hash(&mut hasher);
        let mid = bytes.len() / 2;
        bytes[mid..mid + 65_536].hash(&mut hasher);
        bytes[bytes.len() - 65_536..].hash(&mut hasher);
    }
    hasher.finish()
}

pub fn register_pdf_doc_bytes(doc_hash: u64, bytes: Arc<Vec<u8>>) {
    let mut reg = PDF_DOC_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = reg.iter().position(|(h, _)| *h == doc_hash) {
        let entry = reg.remove(pos).unwrap();
        reg.push_front(entry);
        return;
    }
    if reg.len() >= PDF_DOC_REGISTRY_CAP {
        reg.pop_back();
    }
    reg.push_front((doc_hash, bytes));
}

pub fn lookup_pdf_doc_bytes(doc_hash: u64) -> Option<Arc<Vec<u8>>> {
    let mut reg = PDF_DOC_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = reg.iter().position(|(h, _)| *h == doc_hash) {
        let entry = reg.remove(pos).unwrap();
        let bytes = Arc::clone(&entry.1);
        reg.push_front(entry);
        return Some(bytes);
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfPageSource {
    pub doc_hash: u64,
    #[serde(skip)]
    pub pdf_bytes: Option<Arc<Vec<u8>>>,
    pub page_index: usize,
    pub x_scale: f32,
    pub y_scale: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl PdfPageSource {
    pub fn resolved_bytes(&self) -> Option<Arc<Vec<u8>>> {
        self.pdf_bytes
            .as_ref()
            .cloned()
            .or_else(|| lookup_pdf_doc_bytes(self.doc_hash))
    }

    pub fn render_at_factor(&self, scale_multiplier: f32, is_thumbnail: bool) -> anyhow::Result<(u32, u32, glib::Bytes)> {
        let eff_x = (self.x_scale * scale_multiplier).clamp(0.12, 8.0);
        let eff_y = (self.y_scale * scale_multiplier).clamp(0.12, 8.0);
        let scale_key = (eff_x * 1000.0).round() as u32;
        let cache_key = (self.doc_hash, self.page_index, scale_key);

        let cache = if is_thumbnail {
            &PDF_THUMB_CACHE
        } else {
            &PDF_PAGE_CACHE
        };
        let cap = if is_thumbnail {
            PDF_THUMB_CACHE_CAP
        } else {
            PDF_PAGE_CACHE_CAP
        };

        {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(pos) = guard.iter().position(|(k, _)| *k == cache_key) {
                let entry = guard.remove(pos).unwrap();
                let res = entry.1.clone();
                guard.push_front(entry);
                return Ok(res);
            }
        }

        let pdf_data = self
            .resolved_bytes()
            .ok_or_else(|| anyhow!("PDF source bytes unavailable for doc_hash {:016x}", self.doc_hash))?;

        let pdf = if let Some(ref pw) = self.password {
            hayro_syntax::Pdf::new_with_password(pdf_data, pw)
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        } else {
            hayro_syntax::Pdf::new(pdf_data)
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        };
        let page = pdf
            .pages()
            .get(self.page_index)
            .ok_or_else(|| anyhow!("No PDF page at index {}", self.page_index))?;

        let render_cache = hayro::RenderCache::new();
        let interpreter_settings = hayro_interpret::InterpreterSettings::default();
        let render_settings = hayro::RenderSettings::default();
        let pixmap_settings = hayro::PixmapSettings {
            x_scale: eff_x,
            y_scale: eff_y,
            bg_color: vello_cpu::color::AlphaColor::WHITE,
        };
        let pixmap = hayro::render(
            page,
            &render_cache,
            &interpreter_settings,
            &render_settings,
            &pixmap_settings,
        );
        let pw = u32::from(pixmap.width()).max(1);
        let ph = u32::from(pixmap.height()).max(1);
        let bytes = glib::Bytes::from(pixmap.data_as_u8_slice());
        let rendered = (pw, ph, bytes);

        {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(pos) = guard.iter().position(|(k, _)| *k == cache_key) {
                guard.remove(pos);
            } else if guard.len() >= cap {
                guard.pop_back();
            }
            guard.push_front((cache_key, rendered.clone()));
        }

        Ok(rendered)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "bitmapimage")]
pub struct BitmapImage {
    /// The bitmap image.
    ///
    /// The bounds field of the image should not be used to determine the stroke bounds.
    /// Use rectangle.bounds() instead.
    #[serde(rename = "image")]
    pub image: Image,
    #[serde(rename = "rectangle")]
    pub rectangle: Rectangle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_runs: Vec<crate::textsearch::TextRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_source: Option<PdfPageSource>,
}

impl Default for BitmapImage {
    fn default() -> Self {
        Self {
            image: Image::default(),
            rectangle: Rectangle::default(),
            text_runs: Vec::new(),
            pdf_source: None,
        }
    }
}

impl Content for BitmapImage {
    fn update_geometry(&mut self) {}
}

impl Drawable for BitmapImage {
    fn draw(&self, cx: &mut impl piet::RenderContext, image_scale: f64) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        cx.transform(self.rectangle.affine.to_kurbo());
        let dest_rect = self.rectangle.cuboid.local_aabb().to_kurbo_rect();

        if let Some(ref src) = self.pdf_source {
            let is_thumb = image_scale < 0.45;
            let mult = if is_thumb {
                // Render thumbnail directly at small scale without polluting full-res LRU cache
                (image_scale as f32).clamp(0.12, 0.45)
            } else if image_scale > 1.5 {
                // Boost crispness slightly when zoomed in deeply, capped for memory safety
                (image_scale as f32 / 1.5).clamp(1.0, 2.0)
            } else {
                1.0
            };
            if let Ok((pw, ph, data)) = src.render_at_factor(mult, is_thumb) {
                let piet_image = cx
                    .make_image(
                        pw as usize,
                        ph as usize,
                        &data,
                        piet::ImageFormat::RgbaPremul,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Make piet image from virtualized PDF page failed, Err: {e:?}"
                        )
                    })?;
                cx.draw_image(&piet_image, dest_rect, piet::InterpolationMode::Bilinear);
                cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
                return Ok(());
            }
        }

        let piet_image_format = piet::ImageFormat::from(self.image.memory_format);
        let piet_image = cx
            .make_image(
                self.image.pixel_width as usize,
                self.image.pixel_height as usize,
                &self.image.data,
                piet_image_format,
            )
            .map_err(|e| {
                anyhow::anyhow!("Make piet image in BitmapImage draw impl failed, Err: {e:?}")
            })?;
        cx.draw_image(&piet_image, dest_rect, piet::InterpolationMode::Bilinear);
        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(())
    }
}

impl Shapeable for BitmapImage {
    fn bounds(&self) -> Aabb {
        self.rectangle.bounds()
    }

    fn hitboxes(&self) -> Vec<Aabb> {
        vec![self.bounds()]
    }

    fn outline_path(&self) -> kurbo::BezPath {
        self.bounds().to_kurbo_rect().to_path(0.25)
    }
}

impl Transformable for BitmapImage {
    fn translate(&mut self, offset: Vector2) {
        self.rectangle.translate(offset);
    }

    fn rotate(&mut self, angle: f64, center: Vector2) {
        self.rectangle.rotate(angle, center);
    }

    fn scale(&mut self, scale: Vector2) {
        self.rectangle.scale(scale);
    }
}

impl BitmapImage {
    pub fn from_image_bytes(
        bytes: &[u8],
        pos: Vector2,
        size_option: ImageSizeOption,
    ) -> Result<Self, anyhow::Error> {
        let image = Image::try_from_encoded_bytes(bytes)?;
        let initial_size = Vector2::new(image.pixel_width as f64, image.pixel_height as f64);
        let (size, resize_ratio) = match size_option {
            ImageSizeOption::RespectOriginalSize => (initial_size, 1.0f64),
            ImageSizeOption::ImposeSize(given_size) => (given_size, 1.0f64),
            ImageSizeOption::ResizeImage(resize_struct) => (
                initial_size,
                calculate_resize_ratio(resize_struct, initial_size, pos),
            ),
        };
        let mut transform = DAffine2::IDENTITY;
        transform.append_scale_mut(Vector2::splat(resize_ratio));
        transform.append_translation_mut(pos + size * resize_ratio * 0.5);
        let rectangle = Rectangle {
            cuboid: p2d::shape::Cuboid::new(size * 0.5),
            affine: transform,
        };

        Ok(Self {
            image,
            rectangle,
            text_runs: Vec::new(),
            pdf_source: None,
        })
    }

    pub fn from_pdf_bytes(
        to_be_read: &[u8],
        pdf_import_prefs: PdfImportPrefs,
        insert_pos: Vector2,
        page_range: Option<Range<usize>>,
        format: &Format,
        password: Option<String>,
    ) -> Result<Vec<Self>, anyhow::Error> {
        let doc_hash = compute_pdf_doc_hash(to_be_read);
        let data = Arc::new(to_be_read.to_vec());
        register_pdf_doc_bytes(doc_hash, Arc::clone(&data));
        let pdf = if let Some(ref pw) = password {
            hayro_syntax::Pdf::new_with_password(Arc::clone(&data), pw)
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        } else {
            hayro_syntax::Pdf::new(Arc::clone(&data))
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        };
        let pages = pdf.pages();
        let page_range = page_range.unwrap_or(0..pages.len());
        let page_width = if pdf_import_prefs.adjust_document {
            format.width()
        } else {
            format.width() * (pdf_import_prefs.page_width_perc / 100.0)
        };

        // Uniform zoom based on the widest page, keeps relative page sizes.
        let selected_pages: Vec<_> = page_range
            .clone()
            .filter_map(|page_i| pages.get(page_i))
            .collect();
        if selected_pages.is_empty() {
            return Ok(vec![]);
        }
        let max_intrinsic_width = selected_pages
            .iter()
            .map(|page| page.render_dimensions().0 as f64)
            .fold(0.0f64, f64::max);
        if max_intrinsic_width <= 0.0 {
            return Err(anyhow!("PDF pages have zero intrinsic width"));
        }
        let page_zoom_fit = page_width / max_intrinsic_width;

        // Stride for OnePerDocumentPage: start each page on a document page boundary.
        let max_rendered_height = selected_pages
            .iter()
            .map(|page| page.render_dimensions().1 as f64 * page_zoom_fit)
            .fold(0.0f64, f64::max);

        let x = insert_pos[0];
        let mut y = insert_pos[1];

        let page_layouts = page_range
            .map(|page_i| {
                let page = pages
                    .get(page_i)
                    .ok_or_else(|| anyhow::anyhow!("no page at index '{page_i}'"))?;
                let (intrinsic_width, intrinsic_height) = {
                    let dimensions = page.render_dimensions();
                    (dimensions.0 as f64, dimensions.1 as f64)
                };
                let width = intrinsic_width * page_zoom_fit;
                let height = intrinsic_height * page_zoom_fit;

                // Center narrower pages within the import width when adjusting the document.
                let page_x = if pdf_import_prefs.adjust_document {
                    x + (page_width - width) * 0.5
                } else {
                    x
                };
                let image_pos = Vector2::new(page_x, y);
                let image_size = Vector2::new(width, height);

                y += match pdf_import_prefs.page_spacing {
                    PdfImportPageSpacing::Continuous => {
                        if pdf_import_prefs.adjust_document {
                            height
                        } else {
                            height + Stroke::IMPORT_OFFSET_DEFAULT[1] * 0.5
                        }
                    }
                    PdfImportPageSpacing::OnePerDocumentPage => {
                        if pdf_import_prefs.adjust_document {
                            max_rendered_height
                        } else {
                            format.height()
                        }
                    }
                };

                Ok((page_i, image_pos, image_size))
            })
            .collect::<anyhow::Result<Vec<(usize, Vector2, Vector2)>>>()?;

        let base_scale = (pdf_import_prefs.bitmap_scalefactor * page_zoom_fit) as f32;
        static PLACEHOLDER_WHITE_RGBA: [u8; 4] = [255, 255, 255, 255];

        page_layouts
            .into_par_iter()
            .map(|(page_i, image_pos, image_size)| {
                let page = pages
                    .get(page_i)
                    .ok_or_else(|| anyhow::anyhow!("no page at index '{page_i}'"))?;

                // Lightweight 4-byte valid 1x1 white placeholder so `Image::assert_valid()`
                // always succeeds without rasterizing hundreds of pages upfront!
                let placeholder_bounds = Aabb::new(Vector2::ZERO, Vector2::new(1.0, 1.0));
                let image = Image {
                    data: glib::Bytes::from_static(&PLACEHOLDER_WHITE_RGBA),
                    rectangle: Rectangle::from_p2d_aabb(placeholder_bounds),
                    pixel_width: 1,
                    pixel_height: 1,
                    memory_format: crate::image::ImageMemoryFormat::R8g8b8a8Premultiplied,
                };
                let mut transform = DAffine2::IDENTITY;
                transform.append_translation_mut(image_pos + image_size * 0.5);
                let rectangle = Rectangle {
                    cuboid: p2d::shape::Cuboid::new(image_size * 0.5),
                    affine: transform,
                };
                let text_runs = crate::textsearch::extract_text_from_pdf_page(page);
                let pdf_source = Some(PdfPageSource {
                    doc_hash,
                    pdf_bytes: Some(Arc::clone(&data)),
                    page_index: page_i,
                    x_scale: base_scale,
                    y_scale: base_scale,
                    password: password.clone(),
                });

                Ok(Self {
                    image,
                    rectangle,
                    text_runs,
                    pdf_source,
                })
            })
            .collect()
    }
}
