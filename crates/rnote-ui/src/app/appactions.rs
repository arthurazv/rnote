// Imports
use crate::RnApp;
use adw::prelude::*;
use gtk4::{gio, glib, glib::clone};

impl RnApp {
    pub(crate) fn setup_actions(&self) {
        let action_quit = gio::SimpleAction::new("quit", None);
        self.add_action(&action_quit);

        let action_color_scheme = gio::SimpleAction::new_stateful(
            "color-scheme",
            Some(&glib::VariantType::new("s").unwrap()),
            &String::from("system").to_variant(),
        );
        self.add_action(&action_color_scheme);

        let action_new_window = gio::SimpleAction::new("new-window", None);
        self.add_action(&action_new_window);

        action_color_scheme
            .bind_property("state", &self.style_manager(), "color-scheme")
            .transform_to(move |_, val: glib::Variant| {
                match val.get::<String>().unwrap().as_str() {
                    "default" => Some(adw::ColorScheme::Default),
                    "force-light" => Some(adw::ColorScheme::ForceLight),
                    "force-dark" => Some(adw::ColorScheme::ForceDark),
                    _ => None,
                }
            })
            .transform_from(move |_, val: adw::ColorScheme| match val {
                adw::ColorScheme::Default => Some(String::from("default").to_value()),
                adw::ColorScheme::ForceLight => Some(String::from("force-light").to_value()),
                adw::ColorScheme::ForceDark => Some(String::from("force-dark").to_value()),
                _ => None,
            })
            .sync_create()
            .bidirectional()
            .build();

        // Quit App
        action_quit.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                // Request closing all windows. They then get the chance to display a save dialog on unsaved changes
                for appwindow in app.windows() {
                    appwindow.close();
                }

                if app.windows().is_empty() {
                    app.quit();
                }
            }
        ));

        action_new_window.connect_activate(clone!(
            #[weak(rename_to = app)]
            self,
            move |_, _| {
                app.new_appwindow_init_show();
            }
        ));
    }

    // Accelerators / Keyboard Shortcuts
    pub(crate) fn setup_action_accels(&self) {
        #[cfg(target_os = "macos")]
        {
            self.set_accels_for_action("app.quit", &["<Meta>q", "<Primary>q", "<Ctrl>q"]);
            self.set_accels_for_action("app.new-window", &["<Meta>n", "<Primary>n", "<Ctrl>n"]);
            self.set_accels_for_action("win.open-doc-new-window", &["<Meta><Shift>o", "<Primary><Shift>o"]);
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.set_accels_for_action("app.quit", &["<Primary>q", "<Ctrl>q"]);
            self.set_accels_for_action("app.new-window", &["<Primary>n", "<Ctrl>n"]);
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn setup_macos_menubar(&self) {
        let menubar = gio::Menu::new();

        // File
        let file_menu = gio::Menu::new();
        let file_new_open = gio::Menu::new();
        file_new_open.append(Some("New Tab"), Some("win.new-tab"));
        file_new_open.append(Some("New Window"), Some("app.new-window"));
        file_new_open.append(Some("Open…"), Some("win.open-doc"));
        file_new_open.append(Some("Open in New Window…"), Some("win.open-doc-new-window"));
        file_menu.append_section(None, &file_new_open);

        let file_import_export = gio::Menu::new();
        file_import_export.append(Some("Import…"), Some("win.import-file"));
        let export_submenu = gio::Menu::new();
        export_submenu.append(Some("Export Document…"), Some("win.export-doc"));
        export_submenu.append(Some("Export Pages…"), Some("win.export-doc-pages"));
        export_submenu.append(Some("Export Selection…"), Some("win.export-selection"));
        file_import_export.append_submenu(Some("Export"), &export_submenu);
        file_menu.append_section(None, &file_import_export);

        let file_save_close = gio::Menu::new();
        file_save_close.append(Some("Save"), Some("win.save-doc"));
        file_save_close.append(Some("Save As…"), Some("win.save-doc-as"));
        file_save_close.append(Some("Print…"), Some("win.print-doc"));
        file_save_close.append(Some("Close Tab"), Some("win.active-tab-close"));
        file_menu.append_section(None, &file_save_close);

        menubar.append_submenu(Some("File"), &file_menu);

        // Edit
        let edit_menu = gio::Menu::new();
        edit_menu.append(Some("Undo"), Some("win.undo"));
        edit_menu.append(Some("Redo"), Some("win.redo"));

        let edit_clipboard = gio::Menu::new();
        edit_clipboard.append(Some("Cut"), Some("win.clipboard-cut"));
        edit_clipboard.append(Some("Copy"), Some("win.clipboard-copy"));
        edit_clipboard.append(Some("Paste"), Some("win.clipboard-paste"));
        edit_clipboard.append(Some("Select All"), Some("win.selection-select-all"));
        edit_menu.append_section(None, &edit_clipboard);

        let edit_doc = gio::Menu::new();
        edit_doc.append(Some("Find…"), Some("win.search"));
        edit_doc.append(Some("Clear Document"), Some("win.clear-doc"));
        edit_menu.append_section(None, &edit_doc);
        menubar.append_submenu(Some("Edit"), &edit_menu);

        // View
        let view_menu = gio::Menu::new();
        view_menu.append(Some("Zoom In"), Some("win.zoom-in"));
        view_menu.append(Some("Zoom Out"), Some("win.zoom-out"));
        view_menu.append(Some("Reset Zoom"), Some("win.zoom-reset"));

        let view_pages = gio::Menu::new();
        view_pages.append(Some("Pages Navigator"), Some("win.show-pages"));
        view_pages.append(Some("Next Page"), Some("win.go-to-next-page"));
        view_pages.append(Some("Previous Page"), Some("win.go-to-prev-page"));
        view_pages.append(Some("Return to Origin Page"), Some("win.return-origin-page"));
        view_menu.append_section(None, &view_pages);

        let view_ui = gio::Menu::new();
        view_ui.append(Some("Toggle Overview"), Some("win.toggle-overview"));
        view_ui.append(Some("Fullscreen"), Some("win.fullscreen"));
        view_menu.append_section(None, &view_ui);
        menubar.append_submenu(Some("View"), &view_menu);

        // Tools
        let tools_menu = gio::Menu::new();
        tools_menu.append(Some("Brush / Pen"), Some("win.pen-style::brush"));
        tools_menu.append(Some("Highlighter"), Some("win.toggle-highlighter"));
        tools_menu.append(Some("Shaper"), Some("win.pen-style::shaper"));
        tools_menu.append(Some("Typewriter"), Some("win.pen-style::typewriter"));
        tools_menu.append(Some("Eraser"), Some("win.pen-style::eraser"));
        tools_menu.append(Some("Selector"), Some("win.pen-style::selector"));
        tools_menu.append(Some("Tools"), Some("win.pen-style::tools"));
        menubar.append_submenu(Some("Tools"), &tools_menu);

        // Window Menu (macOS native special submenu)
        let window_menu = gio::Menu::new();
        let window_item = gio::MenuItem::new_submenu(Some("Window"), &window_menu);
        window_item.set_attribute_value("gtk-macos-special", Some(&"window-submenu".to_variant()));
        menubar.append_item(&window_item);

        self.set_menubar(Some(&menubar));
    }
}
