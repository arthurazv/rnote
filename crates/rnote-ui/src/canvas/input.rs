// Imports
use super::RnCanvas;
use gtk4::{Native, gdk, glib, graphene, prelude::*, subclass::prelude::*};
use p2d::math::Vector2;
use rnote_compose::penevent::{KeyboardKey, ModifierKey, PenEvent, PenState, ShortcutKey};
use rnote_compose::penpath::Element;
use rnote_engine::WidgetFlags;
use rnote_engine::ext::EventPropagationExt;
use rnote_engine::ext::GraphenePointExt;
use rnote_engine::pens::PenMode;
use rnote_engine::pens::penholder::BacklogPolicy;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tracing::trace;

/// Handles a pointer controller event.
///
/// Returns:
/// - the propagation of the event, which is `glib::Propagation::Stop` when the event
///   should be inhibited from propagating
/// - the new pen state
/// - whether a stylus input session is active
/// - accumulated widget flags to apply after `pen_state` has been updated
pub(crate) fn handle_pointer_controller_event(
    canvas: &RnCanvas,
    event: &gdk::Event,
    mut pen_state: PenState,
    mut stylus_active: bool,
) -> (glib::Propagation, PenState, bool, WidgetFlags) {
    let now = Instant::now();
    let mut widget_flags = WidgetFlags::default();
    let touch_drawing = canvas.touch_drawing();
    let gdk_event_type = event.event_type();
    let gdk_modifiers = event.modifier_state();
    let _gdk_device = event.device().unwrap();
    let backlog_policy = canvas.engine_ref().penholder.backlog_policy();
    let is_stylus = event_is_stylus(event);
    let any_mouse_button_mask = gdk::ModifierType::BUTTON1_MASK
        | gdk::ModifierType::BUTTON2_MASK
        | gdk::ModifierType::BUTTON3_MASK;

    //std::thread::sleep(std::time::Duration::from_millis(100));
    //super::input::debug_gdk_event(event);

    if !is_stylus && stylus_active && event_is_touch_or_hand(event) {
        // Palm/touch rejection: never let a hand/finger touch draw or steal focus while stylus is active
        return (
            glib::Propagation::Proceed,
            pen_state,
            stylus_active,
            widget_flags,
        );
    }

    if !is_stylus && reject_pointer_input(event, touch_drawing) {
        return (
            glib::Propagation::Proceed,
            pen_state,
            stylus_active,
            widget_flags,
        );
    }

    // GTK can deliver mouse/trackpad events while a stylus is hovering or drawing.
    // Identify hardware source: stylus and trackpad/mouse each remember their own tool,
    // switching automatically when alternating input devices.
    if is_stylus {
        stylus_active = !matches!(
            gdk_event_type,
            gdk::EventType::ProximityOut | gdk::EventType::ButtonRelease
        );
        canvas.imp().stylus_detected.set(true);
        if gdk_event_type == gdk::EventType::ButtonPress
            || (gdk_event_type == gdk::EventType::MotionNotify
                && gdk_modifiers.contains(gdk::ModifierType::BUTTON1_MASK))
        {
            widget_flags |= route_device_tool(canvas, true);
        }
    } else {
        // Protect active stylus strokes from accidental palm/trackpad brushes,
        // while never swallowing ButtonRelease or motion once buttons are released.
        if gdk_event_type == gdk::EventType::ButtonRelease {
            stylus_active = false;
        } else if stylus_active && pen_state == PenState::Down {
            if gdk_event_type == gdk::EventType::MotionNotify
                && !gdk_modifiers.intersects(any_mouse_button_mask)
            {
                stylus_active = false;
            } else {
                return (
                    glib::Propagation::Proceed,
                    pen_state,
                    stylus_active,
                    widget_flags,
                );
            }
        } else {
            stylus_active = false;
        }

        if gdk_event_type == gdk::EventType::ButtonPress
            && let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
            && button_event.button() == gdk::BUTTON_PRIMARY
        {
            widget_flags |= route_device_tool(canvas, false);
        }
    }

    let mut handle_pen_event = false;
    let mut propagation = glib::Propagation::Proceed;

    match gdk_event_type {
        gdk::EventType::MotionNotify => {
            trace!(
                "canvas event MotionNotify - gdk_modifiers: {gdk_modifiers:?}, is_stylus: {is_stylus}"
            );
            handle_pen_event = true;

            if is_stylus {
                // like in gtk4 'gesturestylus.c:120' stylus proximity is detected this way,
                // in case ProximityIn & ProximityOut is not reported.
                if gdk_modifiers.contains(gdk::ModifierType::BUTTON1_MASK) {
                    pen_state = PenState::Down;
                } else if pen_state == PenState::Down {
                    pen_state = PenState::Up;
                } else {
                    pen_state = PenState::Proximity;
                }
            } else if pen_state == PenState::Down
                && !gdk_modifiers.intersects(any_mouse_button_mask)
            {
                pen_state = PenState::Up;
            }
        }
        gdk::EventType::ButtonPress => {
            let button_event = event.downcast_ref::<gdk::ButtonEvent>().unwrap();
            let gdk_button = button_event.button();
            let mut handle_shortcut_key = false;

            trace!("canvas event ButtonPress - gdk_button: {gdk_button}, is_stylus: {is_stylus}");

            if is_stylus {
                if gdk_button == gdk::BUTTON_PRIMARY
                    || gdk_button == gdk::BUTTON_SECONDARY
                    || gdk_button == gdk::BUTTON_MIDDLE
                {
                    handle_pen_event = true;
                    handle_shortcut_key = true;
                }
                if gdk_button == gdk::BUTTON_PRIMARY {
                    pen_state = PenState::Down;
                }
            } else {
                #[allow(clippy::collapsible_else_if)]
                if gdk_button == gdk::BUTTON_PRIMARY || gdk_button == gdk::BUTTON_SECONDARY {
                    handle_pen_event = true;
                    handle_shortcut_key = true;
                    pen_state = PenState::Down;
                }
            }

            if handle_shortcut_key {
                let shortcut_key = retrieve_button_shortcut_key(gdk_button, is_stylus);

                if let Some(shortcut_key) = shortcut_key {
                    let (ep, wf) = canvas
                        .engine_mut()
                        .handle_pressed_shortcut_key(shortcut_key, now);
                    widget_flags |= wf;
                    propagation = ep.into_glib();
                }
            }
        }
        gdk::EventType::ButtonRelease => {
            let button_event = event.downcast_ref::<gdk::ButtonEvent>().unwrap();
            let gdk_button = button_event.button();

            trace!("canvas event ButtonRelease - gdk_button: {gdk_button}, is_stylus: {is_stylus}");

            if is_stylus {
                if gdk_button == gdk::BUTTON_PRIMARY
                    || gdk_button == gdk::BUTTON_SECONDARY
                    || gdk_button == gdk::BUTTON_MIDDLE
                {
                    handle_pen_event = true;
                }

                // again, this is the method to detect proximity on stylus.
                if gdk_button == gdk::BUTTON_PRIMARY {
                    pen_state = PenState::Up;
                } else {
                    // Workaround for https://github.com/flxzt/rnote/issues/785
                    // On window only one button release event is sent when the
                    // pen leaves the screen, and if the button is pressed this
                    // is not a gdk::BUTTON_PRIMARY
                    #[allow(clippy::collapsible_else_if)]
                    if cfg!(target_os = "windows") {
                        pen_state = PenState::Up;
                    } else {
                        pen_state = PenState::Proximity;
                    }
                }
            } else {
                #[allow(clippy::collapsible_else_if)]
                if gdk_button == gdk::BUTTON_PRIMARY || gdk_button == gdk::BUTTON_SECONDARY {
                    pen_state = PenState::Up;
                    handle_pen_event = true;
                }
            };
        }
        gdk::EventType::ProximityIn => {
            pen_state = PenState::Proximity;
            handle_pen_event = true;
        }
        gdk::EventType::ProximityOut => {
            pen_state = PenState::Up;
            handle_pen_event = true;
        }
        // We early-returned when detecting touch input and touch-drawing is not enabled,
        // so it is fine to always handle it here.
        gdk::EventType::TouchBegin => {
            pen_state = PenState::Down;
            handle_pen_event = true;
        }
        gdk::EventType::TouchUpdate => {
            pen_state = PenState::Down;
            handle_pen_event = true;
        }
        gdk::EventType::TouchEnd => {
            pen_state = PenState::Up;
            handle_pen_event = true;
        }
        gdk::EventType::TouchCancel => {
            pen_state = PenState::Up;
            handle_pen_event = true;
        }
        _ => {}
    };

    if handle_pen_event {
        let Some(elements) = retrieve_pointer_elements(canvas, now, event, backlog_policy) else {
            return (
                glib::Propagation::Proceed,
                pen_state,
                stylus_active,
                widget_flags,
            );
        };
        let modifier_keys = retrieve_modifier_keys(event.modifier_state());
        let pen_mode = retrieve_pen_mode(event);

        for (element, event_time) in elements {
            trace!(?element, ?pen_state, ?modifier_keys, ?pen_mode, event_time_delta=?now.duration_since(event_time), msg="handle pen event element");

            // Workaround for https://github.com/flxzt/rnote/issues/785
            // only one event is sent when the pen approaches the screen
            // on Windows whereas rnote expects 2 (switch to proximity
            // then down). This forces the pen to be down when on the
            // screen
            #[cfg(target_os = "windows")]
            {
                if element.pressure > 0.0 && is_stylus {
                    pen_state = PenState::Down;
                }
            }

            match pen_state {
                PenState::Up => {
                    canvas.enable_drawing_cursor(false);

                    let (ep, wf) = canvas.engine_mut().handle_pen_event(
                        PenEvent::Up {
                            element,
                            modifier_keys: modifier_keys.clone(),
                        },
                        pen_mode,
                        event_time,
                    );
                    widget_flags |= wf;
                    propagation = ep.into_glib();
                }
                PenState::Proximity => {
                    canvas.enable_drawing_cursor(false);

                    let (ep, wf) = canvas.engine_mut().handle_pen_event(
                        PenEvent::Proximity {
                            element,
                            modifier_keys: modifier_keys.clone(),
                        },
                        pen_mode,
                        event_time,
                    );
                    widget_flags |= wf;
                    propagation = ep.into_glib();
                }
                PenState::Down => {
                    canvas.grab_focus();
                    canvas.enable_drawing_cursor(true);

                    let (ep, wf) = canvas.engine_mut().handle_pen_event(
                        PenEvent::Down {
                            element,
                            modifier_keys: modifier_keys.clone(),
                        },
                        pen_mode,
                        event_time,
                    );
                    widget_flags |= wf;
                    propagation = ep.into_glib();
                }
            }
        }
    }

    (propagation, pen_state, stylus_active, widget_flags)
}

/// Synchronizes and switches independent per-device tool memory between Stylus and Trackpad/Mouse.
fn route_device_tool(canvas: &RnCanvas, device_is_stylus: bool) -> WidgetFlags {
    let imp = canvas.imp();
    if !imp.pen_mouse_split.get() {
        return WidgetFlags::default();
    }
    let current_style = canvas.engine_ref().current_pen_style();
    let (active_tool, other_tool) = if device_is_stylus {
        (&imp.stylus_tool, &imp.mouse_tool)
    } else {
        (&imp.mouse_tool, &imp.stylus_tool)
    };
    let mut widget_flags = WidgetFlags::default();
    if imp.last_input_is_stylus.get() == Some(!device_is_stylus) {
        if current_style != other_tool.get() {
            // User manually picked a new tool in the toolbar before using this device
            active_tool.set(current_style);
        } else {
            let target = active_tool.get();
            if current_style != target {
                widget_flags |= canvas.engine_mut().change_pen_style(target);
            }
        }
    } else {
        active_tool.set(current_style);
    }
    imp.last_input_is_stylus.set(Some(device_is_stylus));
    widget_flags
}

pub(crate) fn handle_key_controller_key_pressed(
    canvas: &RnCanvas,
    gdk_key: gdk::Key,
    gdk_modifiers: gdk::ModifierType,
) -> glib::Propagation {
    trace!("canvas event key pressed - gdk_key: {gdk_key:?}, gdk_modifiers: {gdk_modifiers:?}");
    canvas.grab_focus();

    let now = Instant::now();
    let keyboard_key = retrieve_keyboard_key(gdk_key);
    let modifier_keys = retrieve_modifier_keys(gdk_modifiers);
    let shortcut_key = retrieve_keyboard_shortcut_key(gdk_key, gdk_modifiers);

    let (propagation, widget_flags) = if let Some(shortcut_key) = shortcut_key {
        canvas
            .engine_mut()
            .handle_pressed_shortcut_key(shortcut_key, now)
    } else {
        canvas.engine_mut().handle_pen_event(
            PenEvent::KeyPressed {
                keyboard_key,
                modifier_keys,
            },
            None,
            now,
        )
    };

    canvas.emit_handle_widget_flags(widget_flags);
    propagation.into_glib()
}

pub(crate) fn handle_key_controller_key_released(
    _canvas: &RnCanvas,
    gdk_key: gdk::Key,
    gdk_modifiers: gdk::ModifierType,
) {
    trace!("canvas event key released - gdk_key: {gdk_key:?}, gdk_modifiers: {gdk_modifiers:?}");
}

pub(crate) fn handle_imcontext_text_commit(canvas: &RnCanvas, text: &str) {
    let now = Instant::now();

    let (_ep, widget_flags) = canvas.engine_mut().handle_pen_event(
        PenEvent::Text {
            text: text.to_string(),
        },
        None,
        now,
    );
    canvas.emit_handle_widget_flags(widget_flags);
}

#[allow(unused)]
fn trace_gdk_event(event: &gdk::Event) {
    let pos = event
        .position()
        .map(|(x, y)| format!("x: {x:.1}, y: {y:.1}"));
    trace!(
        msg="Gdk event",
        pos=?pos,
        device=?event.device(),
        modifier=?event.modifier_state(),
        event_type=?event.event_type(),
        tool_type=?event.device_tool().map(|t| t.tool_type()),
        input_source=?event.device().map(|d| d.source())
    );
}

/// Returns true if input is from a finger/touchscreen/hand rather than a stylus or physical mouse.
pub(crate) fn event_is_touch_or_hand(event: &gdk::Event) -> bool {
    let event_type = event.event_type();
    event.is_pointer_emulated()
        || matches!(
            event_type,
            gdk::EventType::TouchBegin
                | gdk::EventType::TouchUpdate
                | gdk::EventType::TouchEnd
                | gdk::EventType::TouchCancel
        )
        || event
            .device()
            .is_some_and(|d| d.source() == gdk::InputSource::Touchscreen)
}

/// Returns true if input should be rejected from drawing (hand/touch when touch_drawing is off, or multi-touch).
pub(crate) fn reject_pointer_input(event: &gdk::Event, touch_drawing: bool) -> bool {
    if event_is_stylus(event) {
        return false;
    }
    if touch_drawing {
        if event.device().is_some_and(|d| d.num_touches() > 1) {
            return true;
        }
    } else if event_is_touch_or_hand(event) {
        return true;
    }
    false
}

#[cfg(target_os = "macos")]
fn macos_current_nsevent_stylus_info(event: &gdk::Event) -> Option<(f64, Option<PenMode>)> {
    let gdk_event_type = event.event_type();
    if !matches!(
        gdk_event_type,
        gdk::EventType::MotionNotify | gdk::EventType::ButtonPress | gdk::EventType::ButtonRelease
    ) {
        return None;
    }

    use std::ffi::{c_char, c_void};
    unsafe {
        unsafe extern "C" {
            fn dlopen(filename: *const c_char, flag: i32) -> *mut c_void;
            fn sel_registerName(name: *const c_char) -> *mut c_void;
            fn objc_msgSend(receiver: *mut c_void, op: *mut c_void, ...) -> *mut c_void;
            fn objc_getClass(name: *const c_char) -> *mut c_void;
        }
        dlopen(
            b"/System/Library/Frameworks/AppKit.framework/AppKit\0".as_ptr() as *const _,
            1,
        );
        let ns_app_cls = objc_getClass(b"NSApplication\0".as_ptr() as *const _);
        if ns_app_cls.is_null() {
            return None;
        }
        let msg_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const ());
        let msg_usize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
            std::mem::transmute(objc_msgSend as *const ());
        let msg_i16: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i16 =
            std::mem::transmute(objc_msgSend as *const ());
        let msg_f32: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f32 =
            std::mem::transmute(objc_msgSend as *const ());
        let msg_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 =
            std::mem::transmute(objc_msgSend as *const ());

        let app = msg_ptr(
            ns_app_cls,
            sel_registerName(b"sharedApplication\0".as_ptr() as *const _),
        );
        if app.is_null() {
            return None;
        }
        let ns_event = msg_ptr(
            app,
            sel_registerName(b"currentEvent\0".as_ptr() as *const _),
        );
        if ns_event.is_null() {
            return None;
        }

        let gdk_time = event.time();
        if gdk_time > 0 {
            let ns_ts = msg_f64(
                ns_event,
                sel_registerName(b"timestamp\0".as_ptr() as *const _),
            );
            let ns_ts_ms = ((ns_ts * 1000.0) as u64) as u32;
            let diff = ns_ts_ms.wrapping_sub(gdk_time).min(gdk_time.wrapping_sub(ns_ts_ms));
            if diff > 15 {
                return None;
            }
        }

        let ev_type = msg_usize(ns_event, sel_registerName(b"type\0".as_ptr() as *const _));
        let subtype = || {
            msg_i16(
                ns_event,
                sel_registerName(b"subtype\0".as_ptr() as *const _),
            )
        };
        let tablet_pressure = || {
            let pressure = msg_f32(
                ns_event,
                sel_registerName(b"pressure\0".as_ptr() as *const _),
            );
            Some(((pressure as f64).clamp(0.01, 1.0), None))
        };

        match (gdk_event_type, ev_type) {
            (gdk::EventType::MotionNotify, 23) => tablet_pressure(),
            (gdk::EventType::MotionNotify, 5..=7 | 27) if subtype() == 1 => tablet_pressure(),
            (gdk::EventType::ButtonPress, 1 | 3 | 25) if subtype() == 1 => tablet_pressure(),
            (gdk::EventType::ButtonRelease, 2 | 4 | 26) if subtype() == 1 => tablet_pressure(),
            _ => None,
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn macos_current_nsevent_stylus_info(_event: &gdk::Event) -> Option<(f64, Option<PenMode>)> {
    None
}

fn event_is_stylus(event: &gdk::Event) -> bool {
    // Detect stylus via device tool (GTK4 standard), input source (Pen),
    // or matching native macOS NSEvent tablet subtype (for gdkmacos core-pointer tablet routing)
    event.device_tool().is_some()
        || event
            .device()
            .is_some_and(|d| d.source() == gdk::InputSource::Pen)
        || macos_current_nsevent_stylus_info(event).is_some()
}

fn retrieve_pointer_elements(
    canvas: &RnCanvas,
    now: Instant,
    event: &gdk::Event,
    backlog_policy: BacklogPolicy,
) -> Option<Vec<(Element, Instant)>> {
    // Retrieve the transform directly from the event, just like in `gtkgesturestylus.c`'s `get_backlog()`
    let event_native = Native::for_surface(&event.surface()?)?;
    let (surface_trans_x, surface_trans_y) = event_native.surface_transform();
    // retrieving the pressure only works when the event has a device tool,
    // else we get SIGSEGV when trying to access (TODO: report this to gtk-rs)
    let has_device_tool = event.device_tool().is_some();
    let event_time = event.time();

    let mut elements = Vec::with_capacity(1);

    // Transforms the pos given in surface coordinate space to the canvas document coordinate space
    let transform_pos = |pos: Vector2| -> Vector2 {
        event_native
            .compute_point(
                canvas,
                &graphene::Point::from_p2d_vec(
                    pos - Vector2::new(surface_trans_x, surface_trans_y),
                ),
            )
            .map(|p| {
                canvas
                    .engine_ref()
                    .camera
                    .transform()
                    .inverse()
                    .transform_point2(p.to_p2d_vec())
            })
            .unwrap()
    };

    if event.event_type() == gdk::EventType::MotionNotify
        && backlog_policy != BacklogPolicy::Disable
    {
        let mut prev_delta = Duration::ZERO;

        let mut entries = vec![];
        for entry in event.history().into_iter().rev() {
            let available_axes = entry.flags();
            if !(available_axes.contains(gdk::AxisFlags::X)
                && available_axes.contains(gdk::AxisFlags::Y))
            {
                continue;
            }

            let entry_delta = Duration::from_millis(event_time.saturating_sub(entry.time()) as u64);
            let Some(entry_time) = now.checked_sub(entry_delta) else {
                continue;
            };

            if let BacklogPolicy::Limit(delta_limit) = backlog_policy {
                // We go back in time, so `entry_delta` will increase
                //
                // If the backlog input rate is higher than the limit, filter it out
                if entry_delta.saturating_sub(prev_delta) < delta_limit {
                    continue;
                }
            }
            prev_delta = entry_delta;

            let axes = entry.axes();
            let pos = transform_pos(Vector2::new(
                axes[crate::utils::axis_use_idx(gdk::AxisUse::X)],
                axes[crate::utils::axis_use_idx(gdk::AxisUse::Y)],
            ));
            let pressure = if has_device_tool {
                axes[crate::utils::axis_use_idx(gdk::AxisUse::Pressure)]
            } else {
                Element::PRESSURE_DEFAULT
            };

            entries.push((Element::new(pos, pressure), entry_time));
        }

        elements.extend(entries.into_iter().rev());
    }

    let pos = event
        .position()
        .map(|(x, y)| transform_pos(Vector2::new(x, y)))?;

    let pressure = if has_device_tool {
        event.axis(gdk::AxisUse::Pressure).unwrap_or(Element::PRESSURE_DEFAULT)
    } else if let Some((mac_p, _)) = macos_current_nsevent_stylus_info(event) {
        mac_p
    } else {
        Element::PRESSURE_DEFAULT
    };

    elements.push((Element::new(pos, pressure), now));

    Some(elements)
}

pub(crate) fn retrieve_button_shortcut_key(
    gdk_button: u32,
    is_stylus: bool,
) -> Option<ShortcutKey> {
    match (is_stylus, gdk_button) {
        (_, gdk::BUTTON_PRIMARY) => None,
        (false, gdk::BUTTON_SECONDARY) => Some(ShortcutKey::MouseSecondaryButton),
        (true, gdk::BUTTON_SECONDARY) => Some(ShortcutKey::StylusPrimaryButton),
        (true, gdk::BUTTON_MIDDLE) => Some(ShortcutKey::StylusSecondaryButton),
        (_, _) => None,
    }
}

pub(crate) fn retrieve_modifier_keys(modifier: gdk::ModifierType) -> HashSet<ModifierKey> {
    let mut keys = HashSet::new();

    if modifier.contains(gdk::ModifierType::SHIFT_MASK) {
        keys.insert(ModifierKey::KeyboardShift);
    }
    if modifier.contains(gdk::ModifierType::CONTROL_MASK) {
        keys.insert(ModifierKey::KeyboardCtrl);
    }
    if modifier.contains(gdk::ModifierType::ALT_MASK) {
        keys.insert(ModifierKey::KeyboardAlt);
    }

    keys
}

fn retrieve_pen_mode(event: &gdk::Event) -> Option<PenMode> {
    if let Some(device_tool) = event.device_tool() {
        return match device_tool.tool_type() {
            gdk::DeviceToolType::Pen => Some(PenMode::Pen),
            gdk::DeviceToolType::Eraser => Some(PenMode::Eraser),
            _ => None,
        };
    }
    macos_current_nsevent_stylus_info(event).and_then(|(_, mode)| mode)
}

pub(crate) fn retrieve_keyboard_shortcut_key(
    gdk_key: gdk::Key,
    modifier: gdk::ModifierType,
) -> Option<ShortcutKey> {
    match gdk_key {
        gdk::Key::space if modifier.contains(gdk::ModifierType::CONTROL_MASK) => {
            Some(ShortcutKey::KeyboardCtrlSpace)
        }
        _ => None,
    }
}

pub(crate) fn retrieve_keyboard_key(gdk_key: gdk::Key) -> KeyboardKey {
    if let Some(keychar) = gdk_key.to_unicode() {
        KeyboardKey::Unicode(keychar).filter_convert_unicode_control_chars()
    } else {
        // Mapping based on https://developer.mozilla.org/en-US/docs/Web/API/UI_Events/Keyboard_event_key_values
        match gdk_key {
            gdk::Key::BackSpace => KeyboardKey::BackSpace,
            gdk::Key::Tab => KeyboardKey::HorizontalTab,
            gdk::Key::KP_Tab => KeyboardKey::HorizontalTab,
            gdk::Key::ISO_Left_Tab => KeyboardKey::HorizontalTab,
            gdk::Key::Linefeed => KeyboardKey::Linefeed,
            gdk::Key::Return => KeyboardKey::CarriageReturn,
            gdk::Key::KP_Enter => KeyboardKey::CarriageReturn,
            gdk::Key::ISO_Enter => KeyboardKey::CarriageReturn,
            gdk::Key::_3270_Enter => KeyboardKey::CarriageReturn,
            gdk::Key::Escape => KeyboardKey::Escape,
            gdk::Key::Delete => KeyboardKey::Delete,
            gdk::Key::KP_Delete => KeyboardKey::Delete,
            gdk::Key::Down => KeyboardKey::NavDown,
            gdk::Key::KP_Down => KeyboardKey::NavDown,
            gdk::Key::Up => KeyboardKey::NavUp,
            gdk::Key::KP_Up => KeyboardKey::NavUp,
            gdk::Key::Left => KeyboardKey::NavLeft,
            gdk::Key::KP_Left => KeyboardKey::NavLeft,
            gdk::Key::Right => KeyboardKey::NavRight,
            gdk::Key::KP_Right => KeyboardKey::NavRight,
            gdk::Key::Shift_L => KeyboardKey::ShiftLeft,
            gdk::Key::Shift_R => KeyboardKey::ShiftRight,
            gdk::Key::Control_L => KeyboardKey::CtrlLeft,
            gdk::Key::Control_R => KeyboardKey::CtrlRight,
            gdk::Key::Home => KeyboardKey::Home,
            gdk::Key::KP_Home => KeyboardKey::Home,
            gdk::Key::End => KeyboardKey::End,
            gdk::Key::KP_End => KeyboardKey::End,
            _ => KeyboardKey::Unsupported,
        }
    }
}
