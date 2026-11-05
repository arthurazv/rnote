/// Flags returned to the UI widget that holds the engine.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WidgetFlags {
    /// Needs surface redrawing.
    pub redraw: bool,
    /// Needs surface resizing.
    pub resize: bool,
    /// Refresh the UI with the engine state.
    pub refresh_ui: bool,
    /// Indicates that the store was modified, i.e. new strokes inserted, modified, etc. .
    pub store_modified: bool,
    /// Update the current view offsets and size.
    pub view_modified: bool,
    /// Indicates that the camera has changed it's temporary zoom.
    pub zoomed_temporarily: bool,
    /// Indicates that the camera has changed it's permanent zoom.
    pub zoomed: bool,
    /// Deselect the elements of the global color picker.
    pub deselect_color_setters: bool,
    /// Is Some when undo button visibility should be changed. Is None if should not be changed.
    pub hide_undo: Option<bool>,
    /// Is Some when redo button visibility should be changed. Is None if should not be changed.
    pub hide_redo: Option<bool>,
    /// Changes whether text input in the UI toolkit should be enabled.
    pub enable_text_input: Option<bool>,
    /// Clipboard content to be placed into the system clipboard.
    pub clipboard_content: Option<Vec<(Vec<u8>, String)>>,
    /// Toast notification to display when clipboard content is set.
    pub clipboard_toast: Option<String>,
}

impl Default for WidgetFlags {
    fn default() -> Self {
        Self {
            redraw: false,
            resize: false,
            refresh_ui: false,
            store_modified: false,
            view_modified: false,
            zoomed_temporarily: false,
            zoomed: false,
            deselect_color_setters: false,
            hide_undo: None,
            hide_redo: None,
            enable_text_input: None,
            clipboard_content: None,
            clipboard_toast: None,
        }
    }
}

impl std::ops::BitOr for WidgetFlags {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self::Output {
        self |= rhs;
        self
    }
}

impl std::ops::BitOrAssign for WidgetFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.redraw |= rhs.redraw;
        self.resize |= rhs.resize;
        self.refresh_ui |= rhs.refresh_ui;
        self.store_modified |= rhs.store_modified;
        self.view_modified |= rhs.view_modified;
        self.zoomed_temporarily |= rhs.zoomed_temporarily;
        self.zoomed |= rhs.zoomed;
        self.deselect_color_setters |= rhs.deselect_color_setters;
        if rhs.hide_undo.is_some() {
            self.hide_undo = rhs.hide_undo
        }
        if rhs.hide_redo.is_some() {
            self.hide_redo = rhs.hide_redo;
        }
        if rhs.enable_text_input.is_some() {
            self.enable_text_input = rhs.enable_text_input;
        }
        if rhs.clipboard_content.is_some() {
            self.clipboard_content = rhs.clipboard_content;
        }
        if rhs.clipboard_toast.is_some() {
            self.clipboard_toast = rhs.clipboard_toast;
        }
    }
}
