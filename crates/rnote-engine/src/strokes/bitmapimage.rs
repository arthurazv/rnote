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
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;

#[derive(Clone)]
struct PageCacheEntry {
    bytes: glib::Bytes,
    pixel_width: u32,
    pixel_height: u32,
}

struct PdfPageLruCache {
    capacity: usize,
    entries: HashMap<(usize, usize), PageCacheEntry>,
    order: VecDeque<(usize, usize)>,
}

impl PdfPageLruCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &(usize, usize)) -> Option<PageCacheEntry> {
        if self.entries.contains_key(key) {
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(*key);
            self.entries.get(key).cloned()
        } else {
            None
        }
    }

    fn put(&mut self, key: (usize, usize), entry: PageCacheEntry) {
        if self.entries.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| *k == key) {
                self.order.remove(pos);
            }
        } else if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key);
        self.entries.insert(key, entry);
    }
}

static PDF_PAGE_CACHE: Lazy<Mutex<PdfPageLruCache>> =
    Lazy::new(|| Mutex::new(PdfPageLruCache::new(24)));

#[derive(Debug, Clone)]
pub struct PdfPageSource {
    pub pdf_bytes: Arc<Vec<u8>>,
    pub page_index: usize,
    pub scalefactor: f64,
    pub page_zoom_fit: f64,
    pub password: Option<String>,
}

impl PdfPageSource {
    pub fn get_or_render(&self) -> Result<(glib::Bytes, u32, u32), anyhow::Error> {
        let cache_key = (Arc::as_ptr(&self.pdf_bytes) as usize, self.page_index);
        {
            let mut cache = PDF_PAGE_CACHE.lock().unwrap();
            if let Some(entry) = cache.get(&cache_key) {
                return Ok((entry.bytes, entry.pixel_width, entry.pixel_height));
            }
        }

        let pdf = if let Some(ref password) = self.password {
            hayro_syntax::Pdf::new_with_password(Arc::clone(&self.pdf_bytes), password)
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        } else {
            hayro_syntax::Pdf::new(Arc::clone(&self.pdf_bytes))
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        };
        let pages = pdf.pages();
        let page = pages
            .get(self.page_index)
            .ok_or_else(|| anyhow!("no page at index '{}'", self.page_index))?;

        let interpreter_settings = hayro_interpret::InterpreterSettings::default();
        let render_cache = hayro::RenderCache::new();
        let render_settings = hayro::RenderSettings::default();
        let pixmap_settings = hayro::PixmapSettings {
            x_scale: (self.scalefactor * self.page_zoom_fit) as f32,
            y_scale: (self.scalefactor * self.page_zoom_fit) as f32,
            bg_color: vello_cpu::color::AlphaColor::WHITE,
        };

        let pixmap = hayro::render(
            page,
            &render_cache,
            &interpreter_settings,
            &render_settings,
            &pixmap_settings,
        );
        let pixel_width = u32::from(pixmap.width());
        let pixel_height = u32::from(pixmap.height());
        let bytes = glib::Bytes::from(pixmap.data_as_u8_slice());

        let entry = PageCacheEntry {
            bytes: bytes.clone(),
            pixel_width,
            pixel_height,
        };
        {
            let mut cache = PDF_PAGE_CACHE.lock().unwrap();
            cache.put(cache_key, entry);
        }

        Ok((bytes, pixel_width, pixel_height))
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
    #[serde(skip)]
    pub pdf_source: Option<Arc<PdfPageSource>>,
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
    fn draw(&self, cx: &mut impl piet::RenderContext, _image_scale: f64) -> anyhow::Result<()> {
        let (bytes, pixel_width, pixel_height) = if let Some(ref source) = self.pdf_source {
            source.get_or_render()?
        } else {
            (
                self.image.data.clone(),
                self.image.pixel_width,
                self.image.pixel_height,
            )
        };

        if bytes.is_empty() || pixel_width == 0 || pixel_height == 0 {
            return Ok(());
        }

        let piet_image_format = piet::ImageFormat::from(self.image.memory_format);

        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        cx.transform(self.rectangle.affine.to_kurbo());

        let piet_image = cx
            .make_image(
                pixel_width as usize,
                pixel_height as usize,
                &bytes,
                piet_image_format,
            )
            .map_err(|e| {
                anyhow::anyhow!("Make piet image in BitmapImage draw impl failed, Err: {e:?}")
            })?;
        let dest_rect = self.rectangle.cuboid.local_aabb().to_kurbo_rect();
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

    pub fn ensure_data(&mut self) -> Result<(), anyhow::Error> {
        if self.image.data.is_empty() {
            if let Some(ref source) = self.pdf_source {
                let (bytes, w, h) = source.get_or_render()?;
                self.image.data = bytes;
                self.image.pixel_width = w;
                self.image.pixel_height = h;
            }
        }
        Ok(())
    }

    pub fn from_pdf_bytes(
        to_be_read: &[u8],
        pdf_import_prefs: PdfImportPrefs,
        insert_pos: Vector2,
        page_range: Option<Range<usize>>,
        format: &Format,
        password: Option<String>,
    ) -> Result<Vec<Self>, anyhow::Error> {
        let data = Arc::new(to_be_read.to_vec());
        let pdf = if let Some(ref pw) = password {
            hayro_syntax::Pdf::new_with_password(Arc::clone(&data), pw)
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        } else {
            hayro_syntax::Pdf::new(Arc::clone(&data))
                .map_err(|err| anyhow!("Creating Pdf instance failed, Err: {err:?}"))?
        };
        let interpreter_settings = hayro_interpret::InterpreterSettings::default();
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

        let shared_data = Arc::clone(&data);
        page_layouts
            .into_par_iter()
            .map(|(page_i, image_pos, image_size)| {
                let page = pages
                    .get(page_i)
                    .ok_or_else(|| anyhow::anyhow!("no page at index '{page_i}'"))?;

                let pixel_width = ((image_size[0] * pdf_import_prefs.bitmap_scalefactor).round() as u32).max(1);
                let pixel_height = ((image_size[1] * pdf_import_prefs.bitmap_scalefactor).round() as u32).max(1);
                let image_bounds = Aabb::new(
                    Vector2::ZERO,
                    Vector2::new(pixel_width as f64, pixel_height as f64),
                );

                // Render page 0 eagerly so the initial view is instantly ready;
                // other pages are virtualized and lazy-loaded on viewport approach.
                let initial_data = if page_i == 0 {
                    let render_cache = hayro::RenderCache::new();
                    let render_settings = hayro::RenderSettings::default();
                    let pixmap_settings = hayro::PixmapSettings {
                        x_scale: (pdf_import_prefs.bitmap_scalefactor * page_zoom_fit) as f32,
                        y_scale: (pdf_import_prefs.bitmap_scalefactor * page_zoom_fit) as f32,
                        bg_color: vello_cpu::color::AlphaColor::WHITE,
                    };
                    let pixmap = hayro::render(
                        page,
                        &render_cache,
                        &interpreter_settings,
                        &render_settings,
                        &pixmap_settings,
                    );
                    let bytes = glib::Bytes::from(pixmap.data_as_u8_slice());
                    let mut cache = PDF_PAGE_CACHE.lock().unwrap();
                    let cache_key = (Arc::as_ptr(&shared_data) as usize, page_i);
                    cache.put(
                        cache_key,
                        PageCacheEntry {
                            bytes: bytes.clone(),
                            pixel_width,
                            pixel_height,
                        },
                    );
                    bytes
                } else {
                    glib::Bytes::from_static(&[])
                };

                let image = Image {
                    data: initial_data,
                    rectangle: Rectangle::from_p2d_aabb(image_bounds),
                    pixel_width,
                    pixel_height,
                    memory_format: crate::image::ImageMemoryFormat::R8g8b8a8Premultiplied,
                };
                let mut transform = DAffine2::IDENTITY;
                transform.append_translation_mut(image_pos + image_size * 0.5);
                let rectangle = Rectangle {
                    cuboid: p2d::shape::Cuboid::new(image_size * 0.5),
                    affine: transform,
                };
                let text_runs = crate::textsearch::extract_text_from_pdf_page(page);

                let pdf_source = Some(Arc::new(PdfPageSource {
                    pdf_bytes: Arc::clone(&shared_data),
                    page_index: page_i,
                    scalefactor: pdf_import_prefs.bitmap_scalefactor,
                    page_zoom_fit,
                    password: password.clone(),
                }));

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
