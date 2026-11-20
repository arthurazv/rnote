// Imports
use crate::RnAppWindow;
use crate::workspacebrowser::RnFileRow;
use crate::workspacebrowser::workspacesbar::RnWorkspaceListEntry;
use gettextrs::gettext;
use gtk4::{gio, glib, glib::clone, prelude::*};

/// Create a new `pin-notebook` action on a folder row to pin it as a Notebook in the sidebar.
pub(crate) fn pin_notebook(filerow: &RnFileRow, appwindow: &RnAppWindow) -> gio::SimpleAction {
    let action = gio::SimpleAction::new("pin-notebook", None);
    action.connect_activate(clone!(
        #[weak]
        filerow,
        #[weak]
        appwindow,
        move |_, _| {
            let Some(path) = filerow.current_file().and_then(|f| f.path()) else {
                return;
            };
            if !path.is_dir() {
                return;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| String::from("Notebook"));

            let entry = RnWorkspaceListEntry::from_parts(
                path,
                String::from("workspacelistentryicon-book-symbolic"),
                RnWorkspaceListEntry::COLOR_DEFAULT,
                name,
            );
            let workspacesbar = appwindow.sidebar().workspacebrowser().workspacesbar();
            let added = workspacesbar.add_or_select_workspace(entry);
            let _ = appwindow.save_to_settings();

            let msg = if added {
                gettext("Pinned as Notebook")
            } else {
                gettext("Notebook already pinned")
            };
            appwindow
                .overlays()
                .dispatch_toast_text(&msg, Some(std::time::Duration::from_secs(2)));
        }
    ));
    action
}
