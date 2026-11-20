// Imports
use crate::{RnAppWindow, RnCanvas};
use gettextrs::gettext;
use gtk4::{
    Align, Label, ListBox, Orientation, Picture, PolicyType, ScrolledWindow, SelectionMode,
    Widget, gdk, glib, glib::clone, prelude::*, subclass::prelude::*,
};
use p2d::bounding_volume::Aabb;
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// Logical width of the page thumbnails.
const THUMBNAIL_WIDTH: f64 = 168.0;
/// Debounce timeout before regenerating thumbnails after the document was modified.
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(700);

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub(crate) struct RnPagesNavigator {
        pub(crate) appwindow: glib::WeakRef<RnAppWindow>,
        pub(crate) header_label: Label,
        pub(crate) scroller: ScrolledWindow,
        pub(crate) listbox: ListBox,
        pub(crate) pictures: RefCell<Vec<Picture>>,
        pub(crate) pages_bounds: RefCell<Vec<Aabb>>,
        pub(crate) refresh_source: RefCell<Option<glib::SourceId>>,
        pub(crate) needs_refresh: Cell<bool>,
        pub(crate) generation: Cell<u64>,
        pub(crate) current_page: Cell<Option<usize>>,
        pub(crate) update_pending: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnPagesNavigator {
        const NAME: &'static str = "RnPagesNavigator";
        type Type = super::RnPagesNavigator;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk4::BinLayout>();
            klass.set_css_name("pagesnavigator");
        }
    }

    impl ObjectImpl for RnPagesNavigator {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let vbox = gtk4::Box::new(Orientation::Vertical, 0);

            self.header_label.set_halign(Align::Center);
            self.header_label.set_margin_top(9);
            self.header_label.set_margin_bottom(6);
            self.header_label.add_css_class("dim-label");
            self.header_label.add_css_class("caption-heading");
            vbox.append(&self.header_label);

            self.listbox.set_selection_mode(SelectionMode::Single);
            self.listbox.set_activate_on_single_click(true);
            self.listbox.add_css_class("navigation-sidebar");
            self.listbox.add_css_class("pagesnavigator-list");

            self.scroller.set_hscrollbar_policy(PolicyType::Never);
            self.scroller.set_vscrollbar_policy(PolicyType::Automatic);
            self.scroller.set_vexpand(true);
            self.scroller.set_child(Some(&self.listbox));
            vbox.append(&self.scroller);

            vbox.set_parent(&*obj);

            self.listbox.connect_row_activated(clone!(
                #[weak]
                obj,
                move |_, row| {
                    let index = row.index();
                    if index >= 0 {
                        obj.go_to_page(index as usize);
                    }
                }
            ));

            obj.connect_map(|obj| {
                obj.refresh();
            });
        }

        fn dispose(&self) {
            if let Some(source) = self.refresh_source.take() {
                source.remove();
            }
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RnPagesNavigator {}
}

glib::wrapper! {
    pub(crate) struct RnPagesNavigator(ObjectSubclass<imp::RnPagesNavigator>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnPagesNavigator {
    fn default() -> Self {
        Self::new()
    }
}

impl RnPagesNavigator {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        self.imp().appwindow.set(Some(appwindow));
    }

    fn active_canvas(&self) -> Option<RnCanvas> {
        self.imp().appwindow.upgrade()?.active_tab_canvas()
    }

    /// Schedule a (debounced) regeneration of the thumbnails.
    ///
    /// Thumbnails are only generated when the navigator is visible.
    pub(crate) fn schedule_refresh(&self) {
        let imp = self.imp();
        if !self.is_mapped() {
            imp.needs_refresh.set(true);
            return;
        }
        if let Some(source) = imp.refresh_source.take() {
            source.remove();
        }
        let source = glib::timeout_add_local_once(
            REFRESH_DEBOUNCE,
            clone!(
                #[weak(rename_to=navigator)]
                self,
                move || {
                    // The source is finished at this point, so it must not be removed again.
                    let _ = navigator.imp().refresh_source.take();
                    navigator.refresh();
                }
            ),
        );
        imp.refresh_source.replace(Some(source));
    }

    /// Regenerate the thumbnails immediately.
    pub(crate) fn refresh(&self) {
        let imp = self.imp();
        imp.needs_refresh.set(false);
        let Some(canvas) = self.active_canvas() else {
            self.rebuild_rows(&[]);
            imp.pages_bounds.replace(vec![]);
            self.update_header();
            return;
        };

        let pages_bounds = canvas.engine_ref().doc_navigation_pages_bounds();
        let width_px = THUMBNAIL_WIDTH * self.scale_factor().max(1) as f64;
        let receiver = canvas
            .engine_ref()
            .gen_pages_thumbnails(pages_bounds.clone(), width_px);

        // Lay out the rows right away (with the correct aspect ratio), the textures follow once rendered.
        if pages_bounds.len() != imp.pictures.borrow().len()
            || *imp.pages_bounds.borrow() != pages_bounds
        {
            self.rebuild_rows(&pages_bounds);
        }
        imp.pages_bounds.replace(pages_bounds);
        imp.current_page.set(None);
        self.update_current_page();

        let generation = imp.generation.get().wrapping_add(1);
        imp.generation.set(generation);

        glib::spawn_future_local(clone!(
            #[weak(rename_to=navigator)]
            self,
            async move {
                let Ok(thumbnails) = receiver.await else {
                    return;
                };
                if navigator.imp().generation.get() != generation {
                    // A newer refresh was requested in the meantime.
                    return;
                }
                let pictures = navigator.imp().pictures.borrow();
                for (picture, thumbnail) in pictures.iter().zip(thumbnails) {
                    let Some(thumbnail) = thumbnail else {
                        continue;
                    };
                    match thumbnail.to_memtexture() {
                        Ok(texture) => picture.set_paintable(Some(&texture.upcast::<gdk::Texture>())),
                        Err(e) => tracing::error!("Converting page thumbnail to texture failed, Err: {e:?}"),
                    }
                }
            }
        ));
    }

    fn rebuild_rows(&self, pages_bounds: &[Aabb]) {
        let imp = self.imp();
        imp.listbox.remove_all();
        let mut pictures = Vec::with_capacity(pages_bounds.len());

        for (i, bounds) in pages_bounds.iter().enumerate() {
            let extents = bounds.extents();
            let aspect = if extents[0] > 0.0 {
                extents[1] / extents[0]
            } else {
                1.0
            };
            let height = (THUMBNAIL_WIDTH * aspect).clamp(24.0, THUMBNAIL_WIDTH * 4.0);

            let picture = Picture::new();
            picture.set_content_fit(gtk4::ContentFit::Contain);
            picture.set_can_shrink(true);
            picture.set_size_request(THUMBNAIL_WIDTH as i32, height as i32);
            picture.set_halign(Align::Center);
            picture.add_css_class("pagesnavigator-thumbnail");

            let frame = gtk4::Frame::new(None);
            frame.set_child(Some(&picture));
            frame.set_halign(Align::Center);

            let label = Label::new(Some(&(i + 1).to_string()));
            label.add_css_class("caption");
            label.add_css_class("dim-label");

            let vbox = gtk4::Box::new(Orientation::Vertical, 4);
            vbox.set_margin_top(6);
            vbox.set_margin_bottom(6);
            vbox.append(&frame);
            vbox.append(&label);

            let row = gtk4::ListBoxRow::new();
            row.set_child(Some(&vbox));
            row.set_tooltip_text(Some(&format!("{} {}", gettext("Page"), i + 1)));
            imp.listbox.append(&row);
            pictures.push(picture);
        }
        imp.pictures.replace(pictures);
    }

    fn update_header(&self) {
        let imp = self.imp();
        let n_pages = imp.pages_bounds.borrow().len();
        let text = match imp.current_page.get() {
            Some(current) if n_pages > 0 => {
                format!("{} {} / {}", gettext("Page"), current + 1, n_pages)
            }
            _ => format!("{} {}", n_pages, gettext("Pages")),
        };
        imp.header_label.set_label(&text);
    }

    /// Queue an update of the highlighted page on idle.
    ///
    /// Coalesces many view changes (e.g. while scrolling) into a single update and
    /// avoids borrowing the engine while it might still be mutably borrowed by the caller.
    pub(crate) fn queue_update_current_page(&self) {
        let imp = self.imp();
        if !self.is_mapped() || imp.update_pending.get() {
            return;
        }
        imp.update_pending.set(true);
        glib::idle_add_local_once(clone!(
            #[weak(rename_to=navigator)]
            self,
            move || {
                navigator.imp().update_pending.set(false);
                navigator.update_current_page();
            }
        ));
    }

    /// Update the highlighted page to match the current viewport.
    pub(crate) fn update_current_page(&self) {
        let imp = self.imp();
        if !self.is_mapped() {
            return;
        }
        let Some(canvas) = self.active_canvas() else {
            return;
        };
        let current = canvas
            .engine_ref()
            .current_page_index(&imp.pages_bounds.borrow());
        if current == imp.current_page.get() {
            return;
        }
        imp.current_page.set(current);
        self.update_header();

        let Some(current) = current else {
            imp.listbox.unselect_all();
            return;
        };
        let Some(row) = imp.listbox.row_at_index(current as i32) else {
            return;
        };
        imp.listbox.select_row(Some(&row));
        self.scroll_row_into_view(&row);
    }

    fn scroll_row_into_view(&self, row: &gtk4::ListBoxRow) {
        let imp = self.imp();
        let Some(bounds) = row.compute_bounds(&imp.listbox) else {
            return;
        };
        let vadj = imp.scroller.vadjustment();
        let top = bounds.y() as f64;
        let bottom = top + bounds.height() as f64;
        let view_top = vadj.value();
        let view_bottom = view_top + vadj.page_size();
        if top < view_top {
            vadj.set_value(top);
        } else if bottom > view_bottom {
            vadj.set_value(bottom - vadj.page_size());
        }
    }

    /// Navigate the active canvas to the page with the given index.
    pub(crate) fn go_to_page(&self, index: usize) {
        let Some(appwindow) = self.imp().appwindow.upgrade() else {
            return;
        };
        let Some(canvas) = appwindow.active_tab_canvas() else {
            return;
        };
        let Some(page_bounds) = self.imp().pages_bounds.borrow().get(index).copied() else {
            return;
        };
        let widget_flags = canvas.engine_mut().camera_go_to_page(page_bounds);
        appwindow.handle_widget_flags(widget_flags, &canvas);
    }
}

/// Navigate the canvas by one page forward (`forward == true`) or backward.
pub(crate) fn canvas_go_to_adjacent_page(
    appwindow: &RnAppWindow,
    canvas: &RnCanvas,
    forward: bool,
) {
    let target = {
        let engine = canvas.engine_ref();
        let pages = engine.doc_navigation_pages_bounds();
        if pages.is_empty() {
            return;
        }
        let viewport = engine.camera.viewport();
        let current = engine.current_page_index(&pages).unwrap_or(0);
        let index = if forward {
            (current + 1).min(pages.len() - 1)
        } else if viewport.mins[1] > pages[current].mins[1] + 1.0 {
            // When scrolled into the current page, first go back to its top.
            current
        } else {
            current.saturating_sub(1)
        };
        pages[index]
    };
    let widget_flags = canvas.engine_mut().camera_go_to_page(target);
    appwindow.handle_widget_flags(widget_flags, canvas);
}
