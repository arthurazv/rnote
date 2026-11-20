// Imports
use crate::workspacebrowser::workspacesbar::RnWorkspaceListEntry;
use crate::{RnAppWindow, RnWorkspaceBrowser};
use gettextrs::gettext;
use gtk4::{
    Align, CheckButton, DrawingArea, Entry, Label, Popover, PositionType, ToggleButton, gio,
    glib, glib::clone, pango, prelude::*,
};
use rnote_engine::ext::GdkRGBAExt;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{debug, warn};

const NOTEBOOK_ICONS: &[(&str, &str)] = &[
    ("workspacelistentryicon-book-symbolic", "Notebook"),
    ("workspacelistentryicon-math-symbolic", "Math"),
    ("workspacelistentryicon-science-symbolic", "Science"),
    ("workspacelistentryicon-code-symbolic", "Code"),
    ("workspacelistentryicon-folder-symbolic", "Folder"),
];

const NOTEBOOK_COLORS: &[&str] = &[
    "#3584e4", // Blue
    "#2ec27e", // Green
    "#e66100", // Orange
    "#9141ac", // Purple
    "#e01b24", // Red
    "#2190a4", // Teal
];

/// Create a new `create-notebook` action that creates a subject folder ("Caderno") inside the active
/// cloud/local workspace directory and optionally pins it to the sidebar.
pub(crate) fn create_notebook(
    workspacebrowser: &RnWorkspaceBrowser,
    appwindow: &RnAppWindow,
) -> gio::SimpleAction {
    let action = gio::SimpleAction::new("create-notebook", None);

    action.connect_activate(clone!(
        #[weak]
        workspacebrowser,
        #[weak]
        appwindow,
        move |_, _| {
            let Some(parent_path) = workspacebrowser.dir_list_file().and_then(|f| f.path()) else {
                warn!("Can't create new notebook when there is no active workspace directory");
                return;
            };

            let title_label = Label::builder()
                .label(gettext("New Notebook"))
                .halign(Align::Center)
                .build();
            title_label.add_css_class("title-4");

            let dest_label = Label::builder()
                .label(parent_path.to_string_lossy().as_ref())
                .ellipsize(pango::EllipsizeMode::Start)
                .max_width_chars(32)
                .halign(Align::Center)
                .build();
            dest_label.add_css_class("caption");
            dest_label.add_css_class("dim-label");

            let name_entry = Entry::builder()
                .placeholder_text(gettext("Subject or notebook name"))
                .hexpand(true)
                .build();

            let selected_icon = Rc::new(RefCell::new(String::from(NOTEBOOK_ICONS[0].0)));
            let icons_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(6)
                .halign(Align::Center)
                .build();
            let mut first_icon_btn: Option<ToggleButton> = None;
            for (idx, (icon_name, _desc)) in NOTEBOOK_ICONS.iter().enumerate() {
                let btn = ToggleButton::builder()
                    .icon_name(*icon_name)
                    .active(idx == 0)
                    .build();
                btn.add_css_class("circular");
                if let Some(ref group_head) = first_icon_btn {
                    btn.set_group(Some(group_head));
                } else {
                    first_icon_btn = Some(btn.clone());
                }
                let icon_str = (*icon_name).to_string();
                let selected_icon_ref = selected_icon.clone();
                btn.connect_toggled(move |b| {
                    if b.is_active() {
                        *selected_icon_ref.borrow_mut() = icon_str.clone();
                    }
                });
                icons_box.append(&btn);
            }

            let selected_color = Rc::new(RefCell::new(
                gtk4::gdk::RGBA::parse(NOTEBOOK_COLORS[0]).unwrap_or_else(|_| {
                    gtk4::gdk::RGBA::from_compose_color(RnWorkspaceListEntry::COLOR_DEFAULT.into())
                }),
            ));
            let colors_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(6)
                .halign(Align::Center)
                .build();
            let mut first_color_btn: Option<ToggleButton> = None;
            for (idx, hex) in NOTEBOOK_COLORS.iter().enumerate() {
                let rgba = gtk4::gdk::RGBA::parse(*hex).unwrap_or_else(|_| {
                    gtk4::gdk::RGBA::from_compose_color(RnWorkspaceListEntry::COLOR_DEFAULT.into())
                });
                let swatch = DrawingArea::builder()
                    .content_width(18)
                    .content_height(18)
                    .build();
                swatch.set_draw_func(move |_, cr, w, h| {
                    let radius = (w.min(h) as f64) * 0.45;
                    let cx = (w as f64) * 0.5;
                    let cy = (h as f64) * 0.5;
                    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
                    cr.set_source_rgba(
                        rgba.red() as f64,
                        rgba.green() as f64,
                        rgba.blue() as f64,
                        rgba.alpha() as f64,
                    );
                    let _ = cr.fill();
                });

                let btn = ToggleButton::builder()
                    .child(&swatch)
                    .active(idx == 0)
                    .build();
                btn.add_css_class("circular");
                if let Some(ref group_head) = first_color_btn {
                    btn.set_group(Some(group_head));
                } else {
                    first_color_btn = Some(btn.clone());
                }
                let selected_color_ref = selected_color.clone();
                btn.connect_toggled(move |b| {
                    if b.is_active() {
                        *selected_color_ref.borrow_mut() = rgba;
                    }
                });
                colors_box.append(&btn);
            }

            let pin_check = CheckButton::builder()
                .label(gettext("Pin notebook to sidebar"))
                .active(true)
                .build();
            let new_tab_check = CheckButton::builder()
                .label(gettext("Open new tab for this notebook"))
                .active(false)
                .build();

            let cancel_btn = gtk4::Button::builder()
                .label(gettext("Cancel"))
                .halign(Align::Start)
                .build();
            let create_btn = gtk4::Button::builder()
                .label(gettext("Create"))
                .halign(Align::End)
                .sensitive(false)
                .build();
            create_btn.add_css_class("suggested-action");

            let buttons_row = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(12)
                .build();
            let spacer = gtk4::Box::builder().hexpand(true).build();
            buttons_row.append(&cancel_btn);
            buttons_row.append(&spacer);
            buttons_row.append(&create_btn);

            let content = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Vertical)
                .spacing(10)
                .margin_top(10)
                .margin_bottom(10)
                .margin_start(10)
                .margin_end(10)
                .build();
            content.append(&title_label);
            content.append(&dest_label);
            content.append(&name_entry);
            content.append(&icons_box);
            content.append(&colors_box);
            content.append(&pin_check);
            content.append(&new_tab_check);
            content.append(&buttons_row);

            let popover = Popover::builder()
                .autohide(true)
                .has_arrow(true)
                .position(PositionType::Bottom)
                .child(&content)
                .build();
            let actions_box = workspacebrowser.dir_controls_actions_box();
            actions_box.append(&popover);

            popover.connect_closed(clone!(
                #[weak]
                actions_box,
                move |pop| {
                    if pop.parent().is_some_and(|p| p == actions_box) {
                        pop.unparent();
                    }
                }
            ));

            cancel_btn.connect_clicked(clone!(
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                }
            ));

            name_entry.connect_changed(clone!(
                #[weak]
                create_btn,
                #[strong]
                parent_path,
                move |entry| {
                    let raw = entry.text();
                    let name = raw.trim();
                    let invalid = name.is_empty()
                        || name.contains('/')
                        || name.contains('\\')
                        || name == "."
                        || name == ".."
                        || parent_path.join(name).exists();
                    if invalid {
                        create_btn.set_sensitive(false);
                        if !raw.is_empty() {
                            entry.add_css_class("error");
                        } else {
                            entry.remove_css_class("error");
                        }
                    } else {
                        create_btn.set_sensitive(true);
                        entry.remove_css_class("error");
                    }
                }
            ));

            name_entry.connect_activate(clone!(
                #[weak]
                create_btn,
                move |_| {
                    if create_btn.is_sensitive() {
                        create_btn.emit_clicked();
                    }
                }
            ));

            create_btn.connect_clicked(clone!(
                #[weak]
                popover,
                #[weak]
                name_entry,
                #[weak]
                pin_check,
                #[weak]
                new_tab_check,
                #[weak]
                workspacebrowser,
                #[weak]
                appwindow,
                #[strong]
                parent_path,
                move |_| {
                    let name = name_entry.text().trim().to_string();
                    if name.is_empty()
                        || name.contains('/')
                        || name.contains('\\')
                        || name == "."
                        || name == ".."
                    {
                        return;
                    }
                    let notebook_path = parent_path.join(&name);
                    if notebook_path.exists() {
                        appwindow
                            .overlays()
                            .dispatch_toast_error(&gettext("Notebook folder already exists"));
                        return;
                    }
                    if let Err(e) = std::fs::create_dir_all(&notebook_path) {
                        appwindow
                            .overlays()
                            .dispatch_toast_error(&gettext("Creating notebook failed"));
                        debug!("Failed to create notebook folder {notebook_path:?}: {e:?}");
                        return;
                    }

                    if pin_check.is_active() {
                        let color = piet::Color::from((*selected_color.borrow()).into_compose_color());
                        let entry = RnWorkspaceListEntry::from_parts(
                            notebook_path.clone(),
                            selected_icon.borrow().clone(),
                            color,
                            name.clone(),
                        );
                        workspacebrowser.workspacesbar().add_or_select_workspace(entry);
                        let _ = appwindow.save_to_settings();
                    } else {
                        workspacebrowser
                            .set_dir_list_file(Some(&gtk4::gio::File::for_path(&notebook_path)));
                    }

                    if new_tab_check.is_active() {
                        let wrapper = appwindow.new_canvas_wrapper();
                        appwindow.append_wrapper_new_tab(&wrapper);
                    }

                    let msg = format!("{}: {name}", gettext("Notebook created"));
                    appwindow
                        .overlays()
                        .dispatch_toast_text(&msg, Some(std::time::Duration::from_secs(2)));
                    popover.popdown();
                }
            ));

            popover.popup();
            name_entry.grab_focus();
        }
    ));

    action
}
