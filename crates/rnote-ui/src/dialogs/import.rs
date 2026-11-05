//adw::ToolbarView is a replacement for adw::Dialog but not suitable for an async flow

// Imports
use crate::canvas::RnCanvas;
use crate::{RnAppWindow, config};
use adw::prelude::*;
use anyhow::anyhow;
use futures::StreamExt;
use gettextrs::gettext;
use gtk4::{Builder, Button, FileDialog, FileFilter, Label, ToggleButton, gio, glib, glib::clone};
use gtk4::{graphene, gsk};
use hayro::hayro_syntax;
use num_traits::ToPrimitive;
use p2d::math::Vector2;
use rnote_engine::engine::import::{PdfImportPageSpacing, PdfImportPagesType};
use std::sync::Arc;
use tracing::{debug, error};

fn build_all_supported_filter() -> FileFilter {
    let all_filter = FileFilter::new();
    all_filter.set_name(Some(&gettext("All Supported Files")));
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        for pat in [
            "*.rnote", "*.pdf", "*.svg", "*.png", "*.jpg", "*.jpeg", "*.webp", "*.bmp", "*.tiff",
            "*.tif", "*.gif", "*.txt", "*.xopp",
        ] {
            all_filter.add_pattern(pat);
        }
    } else {
        all_filter.add_mime_type("application/x-xopp");
        all_filter.add_mime_type("application/pdf");
        all_filter.add_mime_type("image/svg+xml");
        all_filter.add_mime_type("image/png");
        all_filter.add_mime_type("image/jpeg");
        all_filter.add_mime_type("image/*");
        all_filter.add_mime_type("text/plain");
        all_filter.add_mime_type("application/rnote");
    }
    if cfg!(target_os = "macos") {
        for suffix in [
            "rnote", "pdf", "svg", "png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif",
            "txt", "xopp",
        ] {
            all_filter.add_suffix(suffix);
        }
    }
    all_filter
}

pub(crate) fn default_initial_folder(appwindow: &RnAppWindow) -> gio::File {
    #[cfg(target_os = "macos")]
    {
        let icloud_base = glib::home_dir().join("Library/Mobile Documents/com~apple~CloudDocs");
        let icloud_rnote = icloud_base.join("Rnote");
        let _ = std::fs::create_dir_all(&icloud_rnote);
        if icloud_rnote.exists() {
            return gio::File::for_path(icloud_rnote);
        }
        if icloud_base.exists() {
            return gio::File::for_path(icloud_base);
        }
    }

    if let Some(dir) = appwindow.sidebar().workspacebrowser().dir_list_dir() {
        if dir != std::path::Path::new("/")
            && dir != std::path::Path::new(".")
            && dir != std::path::Path::new("./")
            && dir.exists()
        {
            return gio::File::for_path(dir);
        }
    }

    if let Some(doc_dir) = glib::user_special_dir(glib::UserDirectory::Documents) {
        let rnote_dir = doc_dir.join("Rnote");
        let _ = std::fs::create_dir_all(&rnote_dir);
        if rnote_dir.exists() {
            return gio::File::for_path(rnote_dir);
        }
        return gio::File::for_path(doc_dir);
    }
    gio::File::for_path(glib::home_dir())
}

/// Opens a new rnote save file in a new tab
pub(crate) async fn filedialog_open_doc(appwindow: &RnAppWindow) {
    let filter = FileFilter::new();
    // note : mimetypes are not supported with the native file picker on windows
    // See the limitations on FileChooserNative
    // https://gtk-rs.org/gtk3-rs/stable/latest/docs/gtk/struct.FileChooserNative.html#win32-details--gtkfilechooserdialognative-win32
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        filter.add_pattern("*.rnote");
    } else {
        filter.add_mime_type("application/rnote");
    }
    filter.add_suffix("rnote");
    filter.set_name(Some(&gettext(".rnote")));

    let all_supported_filter = build_all_supported_filter();

    let filter_list = gio::ListStore::new::<FileFilter>();
    filter_list.append(&all_supported_filter);
    filter_list.append(&filter);

    let filedialog = FileDialog::builder()
        .title(gettext("Open File"))
        .modal(true)
        .accept_label(gettext("Open"))
        .filters(&filter_list)
        .default_filter(&all_supported_filter)
        .build();

    filedialog.set_initial_folder(Some(&default_initial_folder(appwindow)));

    match filedialog.open_future(Some(appwindow)).await {
        Ok(selected_file) => {
            appwindow
                .open_file_w_dialogs(selected_file, None, true)
                .await;
        }
        Err(e) => {
            debug!("Did not open document (Error or dialog dismissed by user), Err: {e:?}");
        }
    }
}

pub(crate) async fn filedialog_import_file(appwindow: &RnAppWindow) {
    let filter_list = gio::ListStore::new::<FileFilter>();

    let all_filter = build_all_supported_filter();
    filter_list.append(&all_filter);

    let pdf_filter = FileFilter::new();
    pdf_filter.set_name(Some(&gettext("PDF Documents (*.pdf)")));
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        pdf_filter.add_pattern("*.pdf");
    } else {
        pdf_filter.add_mime_type("application/pdf");
    }
    if cfg!(target_os = "macos") {
        pdf_filter.add_suffix("pdf");
    }
    filter_list.append(&pdf_filter);

    let img_filter = FileFilter::new();
    img_filter.set_name(Some(&gettext("Images (*.png, *.jpg, *.jpeg, *.svg, ...)")));
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        for pat in [
            "*.png", "*.jpg", "*.jpeg", "*.svg", "*.webp", "*.bmp", "*.tiff", "*.tif", "*.gif",
        ] {
            img_filter.add_pattern(pat);
        }
    } else {
        img_filter.add_mime_type("image/*");
    }
    if cfg!(target_os = "macos") {
        for suffix in [
            "png", "jpg", "jpeg", "svg", "webp", "bmp", "tiff", "tif", "gif",
        ] {
            img_filter.add_suffix(suffix);
        }
    }
    filter_list.append(&img_filter);

    let any_filter = FileFilter::new();
    any_filter.set_name(Some(&gettext("All Files (*)")));
    any_filter.add_pattern("*");
    filter_list.append(&any_filter);

    let dialog = FileDialog::builder()
        .title(gettext("Import File"))
        .modal(true)
        .accept_label(gettext("Import"))
        .filters(&filter_list)
        .default_filter(&all_filter)
        .build();

    dialog.set_initial_folder(Some(&default_initial_folder(appwindow)));

    match dialog.open_future(Some(appwindow)).await {
        Ok(selected_file) => {
            appwindow
                .open_file_w_dialogs(selected_file, None, true)
                .await;
        }
        Err(e) => {
            debug!("Did not import file (Error or dialog dismissed by user), Err: {e:?}");
        }
    }
}

/// Check for a pdf encryption and request a password if needed from the user
///
/// Returns the parsed Pdf instance, password Option, and a boolean whether the user canceled the file import or not
pub(crate) async fn pdf_encryption_check_and_dialog(
    appwindow: &RnAppWindow,
    input_file: &gio::File,
    pdf_data: Arc<Vec<u8>>,
) -> (Option<hayro_syntax::Pdf>, Option<String>, bool) {
    let builder = Builder::from_resource(
        (String::from(config::APP_IDPATH) + "ui/dialogs/import.ui").as_str(),
    );

    let dialog_import_pdf_password: adw::AlertDialog =
        builder.object("dialog_import_pdf_password").unwrap();
    let pdf_password_entry: adw::PasswordEntryRow = builder.object("pdf_password_entry").unwrap();
    let pdf_password_entry_box: gtk4::ListBox = builder.object("pdf_password_entry_box").unwrap();

    let target = adw::CallbackAnimationTarget::new(clone!(
        #[weak]
        pdf_password_entry_box,
        move |value| {
            let x = adw::lerp(0., 40.0, value);
            let p = graphene::Point::new(x as f32, 0.);
            let transform = gsk::Transform::new().translate(&p);
            pdf_password_entry_box.allocate(
                pdf_password_entry_box.width(),
                pdf_password_entry_box.height(),
                -1,
                Some(transform),
            );
        }
    ));

    let animation = adw::SpringAnimation::builder()
        .widget(&pdf_password_entry_box)
        .value_from(0.0)
        .value_to(0.0)
        .spring_params(&adw::SpringParams::new(0.2, 0.5, 500.0))
        .target(&target)
        .initial_velocity(10.0)
        .epsilon(0.001) // If amplitude of oscillation < epsilon, animation stops
        .clamp(false)
        .build();

    let (tx, mut rx) = futures::channel::mpsc::unbounded::<(Option<String>, bool)>();

    dialog_import_pdf_password.connect_response(
        Some("unlock"),
        clone!(
            #[weak]
            pdf_password_entry,
            #[strong]
            tx,
            move |_, _| {
                tx.unbounded_send((Some(pdf_password_entry.text().to_string()), false))
                    .unwrap();
            }
        ),
    );

    dialog_import_pdf_password.connect_response(
        Some("cancel"),
        clone!(
            #[strong]
            tx,
            move |_, _| {
                tx.unbounded_send((None, true)).unwrap();
            }
        ),
    );

    let file_name = input_file.basename().map_or_else(
        || gettext("- no file name -"),
        |s| s.to_string_lossy().to_string(),
    );
    let dialog_body = dialog_import_pdf_password.body();
    let dialog_body = file_name.clone() + " " + &dialog_body;
    dialog_import_pdf_password.set_body(&dialog_body);

    let mut password: Option<String> = None;

    loop {
        let pdf_res = if let Some(password) = password.as_ref() {
            hayro_syntax::Pdf::new_with_password(pdf_data.clone(), password)
        } else {
            hayro_syntax::Pdf::new(pdf_data.clone())
        };
        match pdf_res {
            Ok(pdf) => return (Some(pdf), password, false),
            Err(hayro_syntax::LoadPdfError::Decryption(
                hayro_syntax::DecryptionError::PasswordProtected,
            )) => {
                appwindow.overlays().progressbar_abort();
                dialog_import_pdf_password.present(appwindow.root().as_ref());
                pdf_password_entry.grab_focus();

                match rx.next().await {
                    Some((new_password, cancel)) => {
                        if cancel {
                            return (None, None, true);
                        }
                        password = new_password;
                    }
                    None => {
                        return (None, None, true);
                    }
                }
                animation.play();
                pdf_password_entry.set_text("");
            }
            Err(err) => {
                error!("Creating Pdf instance failed, Err: {err:?}");
                return (None, None, true);
            }
        }
    }
}

/// Imports the file as Pdf with an import dialog.
///
/// Returns true when the file was imported, else false.
pub(crate) async fn dialog_import_pdf_w_prefs(
    appwindow: &RnAppWindow,
    canvas: &RnCanvas,
    input_file: gio::File,
    target_pos: Option<Vector2>,
) -> anyhow::Result<bool> {
    let pdf_data = Arc::new(input_file.load_bytes_future().await?.0.to_vec());
    let (pdf, password, cancel) =
        pdf_encryption_check_and_dialog(appwindow, &input_file, pdf_data.clone()).await;
    if cancel {
        return Ok(false);
    }
    let pdf = pdf.ok_or_else(|| anyhow!("Creating Pdf instance failed"))?;

    let builder = Builder::from_resource(
        (String::from(config::APP_IDPATH) + "ui/dialogs/import.ui").as_str(),
    );
    let dialog: adw::Dialog = builder.object("dialog_import_pdf_w_prefs").unwrap();
    let pdf_page_start_row: adw::SpinRow = builder.object("pdf_page_start_row").unwrap();
    let pdf_page_end_row: adw::SpinRow = builder.object("pdf_page_end_row").unwrap();
    let pdf_import_destination_row: adw::ComboRow =
        builder.object("pdf_import_destination_row").unwrap();
    let pdf_import_page_format_row: adw::ComboRow =
        builder.object("pdf_import_page_format_row").unwrap();
    let pdf_info_label: Label = builder.object("pdf_info_label").unwrap();
    let pdf_import_width_row: adw::SpinRow = builder.object("pdf_import_width_row").unwrap();
    let pdf_import_page_spacing_row: adw::ComboRow =
        builder.object("pdf_import_page_spacing_row").unwrap();
    let pdf_import_as_bitmap_toggle: ToggleButton =
        builder.object("pdf_import_as_bitmap_toggle").unwrap();
    let pdf_import_as_vector_toggle: ToggleButton =
        builder.object("pdf_import_as_vector_toggle").unwrap();
    let pdf_import_bitmap_scalefactor_row: adw::SpinRow =
        builder.object("pdf_import_bitmap_scalefactor_row").unwrap();
    let pdf_import_adjust_document_row: adw::SwitchRow =
        builder.object("pdf_import_adjust_document_row").unwrap();
    let import_pdf_button_cancel: Button = builder.object("import_pdf_button_cancel").unwrap();
    let import_pdf_button_confirm: Button = builder.object("import_pdf_button_confirm").unwrap();

    pdf_import_adjust_document_row
        .bind_property("active", &pdf_import_width_row, "sensitive")
        .invert_boolean()
        .sync_create()
        .build();

    let pdf_import_prefs = appwindow
        .engine_config()
        .read()
        .import_prefs
        .pdf_import_prefs;

    // Set the widget state from the pdf import prefs
    pdf_import_width_row.set_value(pdf_import_prefs.page_width_perc);
    match pdf_import_prefs.pages_type {
        PdfImportPagesType::Bitmap => {
            pdf_import_as_bitmap_toggle.set_active(true);
            pdf_import_bitmap_scalefactor_row.set_sensitive(true);
        }
        PdfImportPagesType::Vector => {
            pdf_import_as_vector_toggle.set_active(true);
            pdf_import_bitmap_scalefactor_row.set_sensitive(false);
        }
    }
    pdf_import_page_spacing_row.set_selected(pdf_import_prefs.page_spacing.to_u32().unwrap());
    pdf_import_bitmap_scalefactor_row.set_value(pdf_import_prefs.bitmap_scalefactor);
    pdf_import_adjust_document_row.set_active(pdf_import_prefs.adjust_document);

    pdf_page_start_row
        .bind_property("value", &pdf_page_end_row.adjustment(), "lower")
        .sync_create()
        .build();
    pdf_page_end_row
        .bind_property("value", &pdf_page_start_row.adjustment(), "upper")
        .sync_create()
        .build();

    // Update preferences
    pdf_import_as_vector_toggle.connect_toggled(clone!(
        #[weak]
        pdf_import_bitmap_scalefactor_row,
        #[weak]
        appwindow,
        move |toggle| {
            if !toggle.is_active() {
                return;
            }
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .pdf_import_prefs
                .pages_type = PdfImportPagesType::Vector;
            pdf_import_bitmap_scalefactor_row.set_sensitive(false);
        }
    ));

    pdf_import_as_bitmap_toggle.connect_toggled(clone!(
        #[weak]
        pdf_import_bitmap_scalefactor_row,
        #[weak]
        appwindow,
        move |toggle| {
            if !toggle.is_active() {
                return;
            }
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .pdf_import_prefs
                .pages_type = PdfImportPagesType::Bitmap;
            pdf_import_bitmap_scalefactor_row.set_sensitive(true);
        }
    ));

    pdf_import_bitmap_scalefactor_row.connect_value_notify(clone!(
        #[weak]
        appwindow,
        move |row| {
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .pdf_import_prefs
                .bitmap_scalefactor = row.value();
        }
    ));

    pdf_import_page_spacing_row.connect_selected_notify(clone!(
        #[weak]
        appwindow,
        move |row| {
            let page_spacing = PdfImportPageSpacing::try_from(row.selected()).unwrap();
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .pdf_import_prefs
                .page_spacing = page_spacing;
        }
    ));

    pdf_import_width_row.connect_value_notify(clone!(
        #[weak]
        appwindow,
        move |row| {
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .pdf_import_prefs
                .page_width_perc = row.value();
        }
    ));

    pdf_import_adjust_document_row.connect_active_notify(clone!(
        #[weak]
        appwindow,
        move |row| {
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .pdf_import_prefs
                .adjust_document = row.is_active();
        }
    ));

    let pdf_metadata = pdf.metadata();

    let file_name = input_file.basename().map_or_else(
        || gettext("- no file name -"),
        |s| s.to_string_lossy().to_string(),
    );
    let title = pdf_metadata
        .title
        .to_owned()
        .and_then(|s| String::from_utf8(s).ok())
        .unwrap_or_else(|| gettext("- no title -"));
    let author = pdf_metadata
        .author
        .to_owned()
        .and_then(|s| String::from_utf8(s).ok())
        .unwrap_or_else(|| gettext("- no author -"));
    let mod_date = pdf_metadata
        .modification_date
        .and_then(|dt| {
            let dt = rnote_engine::utils::chrono_dt_from_hayro(dt)?;
            Some(dt.to_rfc3339())
        })
        .unwrap_or_else(|| gettext("- no modification date -"));
    let n_pages = pdf.pages().len();

    // pdf info
    pdf_info_label.set_label(
            &format!("<b>{}  </b>{file_name}\n<b>{}  </b>{title}\n<b>{}  </b>{author}\n<b>{}  </b>{mod_date}\n<b>{}  </b>{n_pages}\n",
                gettext("File name:"),
                gettext("Title:"),
                gettext("Author:"),
                gettext("Modification date:"),
                gettext("Pages:"))
        );

    // Configure pages spinners
    pdf_page_start_row.set_range(1.into(), n_pages as f64);
    pdf_page_start_row.set_value(1.into());

    pdf_page_end_row.set_range(1.into(), n_pages as f64);
    pdf_page_end_row.set_value(n_pages as f64);

    // Listen to responses

    let (tx, mut rx_confirm) = futures::channel::mpsc::unbounded::<(bool, bool)>();
    let tx_cancel = tx.clone();
    let tx_confirm = tx.clone();
    let tx_close = tx.clone();

    import_pdf_button_cancel.connect_clicked(clone!(move |_| {
        if let Err(e) = tx_cancel.unbounded_send((false, false)) {
            error!(
                "PDF import dialog closed, but failed to send signal through channel. Err: {e:?}"
            );
        }
    }));

    import_pdf_button_confirm.connect_clicked(clone!(move |_| {
        if let Err(e) = tx_confirm.unbounded_send((true, false)) {
            error!("PDF file imported, but failed to send signal through channel. Err: {e:?}");
        }
    }));

    // Send a cancel response when the dialog is closed
    dialog.connect_closed(clone!(move |_| {
        if let Err(e) = tx_close.unbounded_send((false, true)) {
            error!(
                "PDF import dialog closed, but failed to send signal through channel. Err: {e:?}"
            );
        }
    }));

    // set focus on enter button for quick confirmation
    dialog.set_focus(Some(&import_pdf_button_confirm));

    // Drop parsed Pdf handle so `pdf_data` can be unwrapped without cloning when confirmed
    drop(pdf);

    // Stop progressbar pulsing while waiting for user confirmation in the dialog
    appwindow.overlays().progressbar_abort();

    // Present then wait for a response from the dialog
    dialog.present(appwindow.root().as_ref());

    match rx_confirm.next().await {
        Some((confirm, dialog_closed)) => {
            let page_range = (pdf_page_start_row.value() as usize).saturating_sub(1)
                ..pdf_page_end_row.value() as usize;
            let destination = pdf_import_destination_row.selected();
            let page_format = pdf_import_page_format_row.selected();
            if !dialog_closed {
                dialog.close();
            }
            if confirm {
                appwindow.overlays().progressbar_start_pulsing();

                if destination == 1 {
                    // Each Page in a New Tab
                    let pdf_data_vec = Arc::unwrap_or_clone(pdf_data);
                    for page_idx in page_range {
                        let wrapper = appwindow.new_canvas_wrapper();
                        let page_canvas = wrapper.canvas();

                        let parsed_dims = if let Ok(pdf_parsed) = if let Some(ref pw) = password {
                            hayro_syntax::Pdf::new_with_password(Arc::new(pdf_data_vec.clone()), pw)
                        } else {
                            hayro_syntax::Pdf::new(Arc::new(pdf_data_vec.clone()))
                        } {
                            pdf_parsed.pages().get(page_idx).map(|p| p.render_dimensions())
                        } else {
                            None
                        };

                        match page_format {
                            0 => {
                                // Original Size
                                if let Some((pw, ph)) = parsed_dims {
                                    let mut engine = page_canvas.engine_mut();
                                    engine.document.config.format.set_width(pw as f64);
                                    engine.document.config.format.set_height(ph as f64);
                                    let _ = engine.set_doc_layout(rnote_engine::document::Layout::FixedSize);
                                }
                            }
                            1 => {
                                // Fixed A4
                                let orientation = if let Some((pw, ph)) = parsed_dims {
                                    if pw > ph {
                                        rnote_engine::document::format::Orientation::Landscape
                                    } else {
                                        rnote_engine::document::format::Orientation::Portrait
                                    }
                                } else {
                                    rnote_engine::document::format::Orientation::Portrait
                                };
                                let (w_mm, h_mm) = if orientation == rnote_engine::document::format::Orientation::Landscape {
                                    (297.0, 210.0)
                                } else {
                                    (210.0, 297.0)
                                };
                                let mut engine = page_canvas.engine_mut();
                                let dpi = engine.document.config.format.dpi();
                                let w_px = (w_mm / rnote_engine::document::format::MeasureUnit::AMOUNT_MM_IN_INCH) * dpi;
                                let h_px = (h_mm / rnote_engine::document::format::MeasureUnit::AMOUNT_MM_IN_INCH) * dpi;
                                engine.document.config.format.set_width(w_px);
                                engine.document.config.format.set_height(h_px);
                                let _ = engine.set_doc_layout(rnote_engine::document::Layout::FixedSize);
                            }
                            _ => {
                                // Custom / Current format from default preset
                            }
                        }

                        page_canvas
                            .load_in_pdf_bytes(
                                appwindow,
                                pdf_data_vec.clone(),
                                target_pos,
                                Some(page_idx..page_idx + 1),
                                password.clone(),
                            )
                            .await?;

                        let tab_page = appwindow.append_wrapper_new_tab(&wrapper);
                        tab_page.set_title(&format!("{file_name} - {}", page_idx + 1));
                    }
                } else {
                    // Single Document in current canvas
                    if page_format == 1 {
                        // Fixed A4
                        let mut engine = canvas.engine_mut();
                        let orientation = engine.document.config.format.orientation();
                        let (w_mm, h_mm) = if orientation == rnote_engine::document::format::Orientation::Landscape {
                            (297.0, 210.0)
                        } else {
                            (210.0, 297.0)
                        };
                        let dpi = engine.document.config.format.dpi();
                        let w_px = (w_mm / rnote_engine::document::format::MeasureUnit::AMOUNT_MM_IN_INCH) * dpi;
                        let h_px = (h_mm / rnote_engine::document::format::MeasureUnit::AMOUNT_MM_IN_INCH) * dpi;
                        engine.document.config.format.set_width(w_px);
                        engine.document.config.format.set_height(h_px);
                    }
                    canvas
                        .load_in_pdf_bytes(
                            appwindow,
                            Arc::unwrap_or_clone(pdf_data),
                            target_pos,
                            Some(page_range),
                            password,
                        )
                        .await?;
                }
                Ok(true)
            } else {
                Ok(false)
            }
        }
        None => Err(anyhow::anyhow!(
            "Channel closed before receiving a response from dialog."
        )),
    }
}

/// Imports the file as Xopp with an import dialog.
///
/// Returns true when the file was imported, else false.
pub(crate) async fn dialog_import_xopp_w_prefs(
    appwindow: &RnAppWindow,
    canvas: &RnCanvas,
    input_file: gio::File,
) -> anyhow::Result<bool> {
    let builder = Builder::from_resource(
        (String::from(config::APP_IDPATH) + "ui/dialogs/import.ui").as_str(),
    );
    let dialog: adw::Dialog = builder.object("dialog_import_xopp_w_prefs").unwrap();
    let dpi_row: adw::SpinRow = builder.object("xopp_import_dpi_row").unwrap();
    let xopp_import_prefs = appwindow
        .engine_config()
        .read()
        .import_prefs
        .xopp_import_prefs;
    let import_xopp_button_cancel: Button = builder.object("import_xopp_button_cancel").unwrap();
    let import_xopp_button_confirm: Button = builder.object("import_xopp_button_confirm").unwrap();

    // Set initial widget state for preference
    dpi_row.set_value(xopp_import_prefs.dpi);

    // Update preferences
    dpi_row.connect_value_notify(clone!(
        #[weak]
        appwindow,
        move |row| {
            appwindow
                .engine_config()
                .write()
                .import_prefs
                .xopp_import_prefs
                .dpi = row.value();
        }
    ));

    // Listen to responses

    let (tx, mut rx_confirm) = futures::channel::mpsc::unbounded::<(bool, bool)>();
    let tx_cancel = tx.clone();
    let tx_confirm = tx.clone();
    let tx_close = tx.clone();

    import_xopp_button_cancel.connect_clicked(clone!(move |_| {
        if let Err(e) = tx_cancel.unbounded_send((false, false)) {
            error!(
                "XOPP import dialog cancelled, but failed to send signal through channel. Err: {e:?}"
            );
        }
    }));

    import_xopp_button_confirm.connect_clicked(clone!(move |_| {
        if let Err(e) = tx_confirm.unbounded_send((true, false)) {
            error!(
                "Xopp import dialog confirmed, but failed to send signal through channel. Err: {e:?}"
            );
        }
    }));

    // Send a cancel response when the dialog is closed
    dialog.connect_closed(clone!(move |_| {
        if let Err(e) = tx_close.unbounded_send((false, true)) {
            error!(
                "XOPP import dialog closed, but failed to send signal through channel. Err: {e:?}"
            );
        }
    }));

    dialog.set_focus(Some(&import_xopp_button_confirm));

    appwindow.overlays().progressbar_abort();

    // Present then wait for a response from the dialog
    dialog.present(appwindow.root().as_ref());

    match rx_confirm.next().await {
        Some((confirm, dialog_closed)) => {
            if !dialog_closed {
                dialog.close();
            }
            if confirm {
                appwindow.overlays().progressbar_start_pulsing();
                let (bytes, _) = input_file.load_bytes_future().await?;
                canvas.load_in_xopp_bytes(appwindow, bytes.to_vec()).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        None => Err(anyhow::anyhow!(
            "Channel closed before receiving a response from dialog."
        )),
    }
}
