/*! API wrapper for talking to the X server using XCB
 *
 *  The crate used by penrose for talking to the X server is rust-xcb, which
 *  is a set of bindings for the C level XCB library that are autogenerated
 *  from an XML spec. The XML files can be found
 *  [here](https://github.com/rtbo/rust-xcb/tree/master/xml) and are useful
 *  as reference for how the API works. Sections have been converted and added
 *  to the documentation of the method calls and enums present in this module.
 *
 *  [EWMH](https://specifications.freedesktop.org/wm-spec/wm-spec-1.3.html)
 *  [Xlib manual](https://tronche.com/gui/x/xlib/)
 */
use crate::data_types::{KeyBindings, KeyCode, Point, Region, WinId};
use crate::screen::Screen;
use std::cell::Cell;
use std::collections::HashMap;
use xcb;

const WM_NAME: &'static str = "penrose";

/*
 * pulling out bitmasks to make the following xcb / xrandr calls easier to parse visually
 */
const WINDOW_CLASS_INPUT_ONLY: u16 = xcb::xproto::WINDOW_CLASS_INPUT_ONLY as u16;
const NOTIFY_MASK: u16 = xcb::randr::NOTIFY_MASK_CRTC_CHANGE as u16;
const GRAB_MODE_ASYNC: u8 = xcb::GRAB_MODE_ASYNC as u8;
const INPUT_FOCUS_PARENT: u8 = xcb::INPUT_FOCUS_PARENT as u8;
const PROP_MODE_REPLACE: u8 = xcb::PROP_MODE_REPLACE as u8;
const ATOM_WINDOW: u32 = xcb::xproto::ATOM_WINDOW;
const STACK_ABOVE: u32 = xcb::STACK_MODE_ABOVE as u32;
const STACK_MODE: u16 = xcb::CONFIG_WINDOW_STACK_MODE as u16;
const WIN_BORDER: u16 = xcb::CONFIG_WINDOW_BORDER_WIDTH as u16;
const WIN_HEIGHT: u16 = xcb::CONFIG_WINDOW_HEIGHT as u16;
const WIN_WIDTH: u16 = xcb::CONFIG_WINDOW_WIDTH as u16;
const WIN_X: u16 = xcb::CONFIG_WINDOW_X as u16;
const WIN_Y: u16 = xcb::CONFIG_WINDOW_Y as u16;
const NEW_WINDOW_MASK: &[(u32, u32)] = &[(
    xcb::CW_EVENT_MASK,
    xcb::EVENT_MASK_ENTER_WINDOW | xcb::EVENT_MASK_LEAVE_WINDOW,
)];
const MOUSE_MASK: u16 = (xcb::EVENT_MASK_BUTTON_PRESS
    | xcb::EVENT_MASK_BUTTON_RELEASE
    | xcb::EVENT_MASK_POINTER_MOTION) as u16;
const EVENT_MASK: &[(u32, u32)] = &[(
    xcb::CW_EVENT_MASK,
    xcb::EVENT_MASK_SUBSTRUCTURE_NOTIFY as u32,
)];

// TODO: this list has been copied from atoms used in other WMs, not using everything
//       yet so work out which ones we need to keep and which we can drop.
const ATOMS: &[&'static str] = &[
    "MANAGER",
    "UTF8_STRING",
    "WM_CLASS",
    "WM_DELETE_WINDOW",
    "WM_PROTOCOLS",
    "WM_STATE",
    "WM_NAME",
    "WM_TAKE_FOCUS",
    "_NET_ACTIVE_WINDOW",
    "_NET_CLIENT_LIST",
    "_NET_CURRENT_DESKTOP",
    "_NET_DESKTOP_NAMES",
    "_NET_NUMBER_OF_DESKTOPS",
    "_NET_SUPPORTED",
    "_NET_SUPPORTING_WM_CHECK",
    "_NET_SYSTEM_TRAY_OPCODE",
    "_NET_SYSTEM_TRAY_ORIENTATION",
    "_NET_SYSTEM_TRAY_ORIENTATION_HORZ",
    "_NET_SYSTEM_TRAY_S0",
    "_NET_WM_DESKTOP",
    "_NET_WM_NAME",
    "_NET_WM_STATE",
    "_NET_WM_STATE_FULLSCREEN",
    "_NET_WM_WINDOW_TYPE",
    "_NET_WM_WINDOW_TYPE_DIALOG",
    "_XEMBED",
    "_XEMBED_INFO",
    // window types
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_WM_WINDOW_TYPE_DOCK",
    "_NET_WM_WINDOW_TYPE_TOOLBAR",
    "_NET_WM_WINDOW_TYPE_MENU",
    "_NET_WM_WINDOW_TYPE_UTILITY",
    "_NET_WM_WINDOW_TYPE_SPLASH",
    "_NET_WM_WINDOW_TYPE_DIALOG",
    "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
    "_NET_WM_WINDOW_TYPE_POPUP_MENU",
    "_NET_WM_WINDOW_TYPE_NOTIFICATION",
    "_NET_WM_WINDOW_TYPE_COMBO",
    "_NET_WM_WINDOW_TYPE_DND",
    "_NET_WM_WINDOW_TYPE_NORMAL",
];

const AUTO_FLOAT_WINDOW_TYPES: &[&'static str] = &[
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_WM_WINDOW_TYPE_DIALOG",
    "_NET_WM_WINDOW_TYPE_DOCK",
    "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
    "_NET_WM_WINDOW_TYPE_MENU",
    "_NET_WM_WINDOW_TYPE_NOTIFICATION",
    "_NET_WM_WINDOW_TYPE_POPUP_MENU",
    "_NET_WM_WINDOW_TYPE_SPLASH",
    "_NET_WM_WINDOW_TYPE_TOOLBAR",
    "_NET_WM_WINDOW_TYPE_UTILITY",
];

/**
 * Wrapper around the low level XCB event types that require casting to work with.
 * Not all event fields are extracted so check the XCB documentation and update
 * accordingly if you need access to something that isn't currently passed through
 * to the WindowManager event loop.
 *
 * https://tronche.com/gui/x/xlib/events/types.html
 * https://github.com/rtbo/rust-xcb/xml/xproto.xml
 *
 * ### XCB Level events
 *
 * *MapNotify* - a window was mapped
 *   - _event_ (WinId):
 *     The window which was mapped or its parent, depending on
 *     whether `StructureNotify` or `SubstructureNotify` was selected.
 *   - _window_ (WinId):
 *     The window that was mapped.
 *   - _override_redirect_ (bool):
 *     We should ignore this window if true
 *
 * *UnmapNotify* - a window was unmapped
 *   - _event_ (WinId):
 *     The window which was unmapped or its parent, depending on
 *     whether `StructureNotify` or `SubstructureNotify` was selected.
 *   - _window_ (WinId):
 *     The window that was unmapped.
 *   - _from-configure_ (bool):
 *     - 'true' if the event was generated as a result of a resizing of
 *       the window's parent when `window` had a win_gravity of `UnmapGravity`.
 *
 * *EnterNotify* - the pointer is now in a different window
 *   - _event_ (WinId):
 *     The window on which the event was generated.
 *   - _child_ (WinId):
 *     If the window has sub-windows then this is the ID of the window
 *     that the pointer ended on, XCB_WINDOW_NONE otherwise.
 *   - _root_ (WinId):
 *     The root window for the final cursor position.
 *   - _root-x, root-y_ (i16, i16):
 *     The coordinates of the pointer relative to 'root's origin.
 *   - _event-x, event-y_ (i16, i16):
 *     The coordinates of the pointer relative to the event window's origin.
 *   - _mode_ (NotifyMode enum)
 *     - Normal, Grab, Ungrab, WhileGrabbed
 *
 * *LeaveNotify* - the pointer has left a window
 *   - Same fields as *EnterNotify*
 *
 * *DestroyNotify* - a window has been destroyed
 *   - _event_ (WinId):
 *     The reconfigured window or its parent, depending on whether
 *     `StructureNotify` or `SubstructureNotify` was selected.
 *   - _window_ (WinId):
 *     The window that was destroyed.
 *
 * *KeyPress* - a keyboard key was pressed / released
 *   - _detail_ (u8):
 *     Keycode of the key that was pressed
 *   - _event_ (u16):
 *     The modifier masks being held when the key was pressed
 *   - _child_ (WinId):
 *     If the window has sub-windows then this is the ID of the window
 *     that the pointer ended on, XCB_WINDOW_NONE otherwise.
 *   - _root_ (WinId):
 *     The root window for the final cursor position.
 *   - _root-x, root-y_ (i16, i16):
 *     The coordinates of the pointer relative to 'root's origin.
 *   - _event-x, event-y_ (i16, i16):
 *     The coordinates of the pointer relative to the event window's origin.
 *
 * *ButtonPress* - a mouse button was pressed
 *   - _detail_ (u8):
 *     The button that was pressed
 *   - _event_ (u16):
 *     The modifier masks being held when the button was pressed
 *   - _child_ (WinId):
 *     If the window has sub-windows then this is the ID of the window
 *     that the pointer ended on, XCB_WINDOW_NONE otherwise.
 *   - _root_ (WinId):
 *     The root window for the final cursor position.
 *   - _root-x, root-y_ (i16, i16):
 *     The coordinates of the pointer relative to 'root's origin.
 *   - _event-x, event-y_ (i16, i16):
 *     The coordinates of the pointer relative to the event window's origin.
 *
 * *ButtonRelease* - a mouse button was released
 *   - same fields as *ButtonPress*
 */
#[derive(Debug, Copy, Clone)]
pub enum XEvent {
    /// xcb docs: https://www.mankier.com/3/xcb_input_raw_button_press_event_t
    ButtonPress,

    /// xcb docs: https://www.mankier.com/3/xcb_input_raw_button_press_event_t
    ButtonRelease,

    /// xcb docs: https://www.mankier.com/3/xcb_input_device_key_press_event_t
    KeyPress {
        /// The X11 key code that was received along with any modifiers that were held
        code: KeyCode,
    },

    /// MapNotifyEvent
    /// xcb docs: https://www.mankier.com/3/xcb_xkb_map_notify_event_t
    Map {
        /// The ID of the window being mapped
        id: WinId,
        /// Whether or not the WindowManager should handle this window.
        ignore: bool,
    },

    /// xcb docs: https://www.mankier.com/3/xcb_enter_notify_event_t
    Enter {
        /// The ID of the window that was entered
        id: WinId,
        /// Absolute coordinate of the event
        rpt: Point,
        /// Coordinate of the event relative to top-left of the window itself
        wpt: Point,
    },

    /// xcb docs: https://www.mankier.com/3/xcb_enter_notify_event_t
    Leave {
        /// The ID of the window that was left
        id: WinId,
        /// Absolute coordinate of the event
        rpt: Point,
        /// Coordinate of the event relative to top-left of the window itself
        wpt: Point,
    },

    /// xcb docs: https://www.mankier.com/3/xcb_focus_in_event_t
    FocusIn {
        /// The ID of the window that gained focus
        id: WinId,
    },

    /// xcb docs: https://www.mankier.com/3/xcb_focus_out_event_t
    FocusOut {
        /// The ID of the window that lost focus
        id: WinId,
    },

    /// MapNotifyEvent
    /// xcb docs: https://www.mankier.com/3/xcb_destroy_notify_event_t
    Destroy {
        /// The ID of the window being destroyed
        id: WinId,
    },

    /// xcb docs: https://www.mankier.com/3/xcb_randr_screen_change_notify_event_t
    ScreenChange,

    /// xcb docs: https://www.mankier.com/3/xcb_randr_notify_event_t
    RandrNotify,
}

/// A handle on a running X11 connection that we can use for issuing X requests
pub trait XConn {
    /// Flush pending actions to the X event loop
    fn flush(&self) -> bool;

    /// Wait for the next event from the X server and return it as an XEvent
    fn wait_for_event(&self) -> Option<XEvent>;

    /// Determine the currently connected CRTCs and return their details
    fn current_outputs(&self) -> Vec<Screen>;

    /// Determine the current (x,y) position of the cursor relative to the root window.
    fn cursor_position(&self) -> Point;

    /// Reposition the window identified by 'id' to the specifed region
    fn position_window(&self, id: WinId, r: Region, border: u32);

    /// Mark the given window as newly created
    fn mark_new_window(&self, id: WinId);

    /// Map a window to the display. Called each time a map_notify event is received
    fn map_window(&self, id: WinId);

    /// Unmap a window from the display. Called each time an unmap_notify event is received
    fn unmap_window(&self, id: WinId);

    /// Send an X event to the target window
    fn send_client_event(&self, id: WinId, atom_name: &str);

    /// Return the client ID of the Client that currently holds X focus
    fn focused_client(&self) -> WinId;

    /// Mark the given client as having focus
    fn focus_client(&self, id: WinId);

    /// Change the border color for the given client
    fn set_client_border_color(&self, id: WinId, color: u32);

    /**
     * Notify the X server that we are intercepting the user specified key bindings
     * and prevent them being passed through to the underlying applications. This
     * is what determines which key press events end up being sent through in the
     * main event loop for the WindowManager.
     */
    fn grab_keys(&self, key_bindings: &KeyBindings);

    /// Set required EWMH properties to ensure compatability with external programs
    fn set_wm_properties(&self, workspaces: &[&str]);

    /// Update the root window properties with the current desktop details
    fn update_desktops(&self, workspaces: &[&str]);

    /// Update which desktop is currently focused
    fn set_current_workspace(&self, wix: usize);

    /// Set the WM_NAME prop of the root window
    fn set_root_window_name(&self, name: &str);

    /// Update which desktop a client is currently on
    fn set_client_workspace(&self, id: WinId, wix: usize);

    /// Determine whether the target window should be tiled or allowed to float
    fn window_should_float(&self, id: WinId, floating_classes: &[&str]) -> bool;

    /**
     * Warp the cursor to be within the specified window. If win_id == None then behaviour is
     * definined by the implementor (e.g. warp cursor to active window, warp to center of screen)
     */
    fn warp_cursor(&self, win_id: Option<WinId>, screen: &Screen);

    /// Run on startup/restart to determine already running windows that we need to track
    fn query_for_active_windows(&self) -> Vec<WinId>;

    /**
     * Use the xcb api to query a string property for a window by window ID and poperty name.
     * Can fail if the property name is invalid or we get a malformed response from xcb.
     */
    fn str_prop(&self, id: u32, name: &str) -> Result<String, String>;

    /// Fetch an atom prop by name for a particular window ID
    fn atom_prop(&self, id: u32, name: &str) -> Result<u32, String>;

    /// Perform any state cleanup required prior to shutting down the window manager
    fn cleanup(&self);
}

/// Handles communication with an X server via xcb
pub struct XcbConnection {
    conn: xcb::Connection,
    root: WinId,
    check_win: WinId,
    atoms: HashMap<&'static str, u32>,
    auto_float_types: Vec<u32>,
    randr_base: u8,
}

impl XcbConnection {
    /// Establish a new connection to the running X server. Fails if unable to connect
    pub fn new() -> XcbConnection {
        let (conn, _) = match xcb::Connection::connect(None) {
            Err(e) => panic!("unable to establish connection to X server: {}", e),
            Ok(conn) => conn,
        };

        let root = match conn.get_setup().roots().nth(0) {
            None => panic!("unable to get handle for screen"),
            Some(s) => s.root(),
        };

        // https://www.mankier.com/3/xcb_intern_atom
        let atoms: HashMap<&'static str, u32> = ATOMS
            .iter()
            .map(|atom| {
                // false == always return the atom, even if exists already
                let val = xcb::intern_atom(&conn, false, atom)
                    .get_reply()
                    .expect(&format!("unable to intern xcb atom '{}'", atom))
                    .atom();

                (*atom, val)
            })
            .collect();

        let auto_float_types: Vec<u32> = AUTO_FLOAT_WINDOW_TYPES
            .iter()
            .map(|t| *atoms.get(t).unwrap())
            .collect();

        let check_win = conn.generate_id();

        // xcb docs: https://www.mankier.com/3/xcb_create_window
        xcb::create_window(
            &conn,                   // xcb connection to X11
            0,                       // new window's depth
            check_win,               // ID to be used for referring to the window
            root,                    // parent window
            0,                       // x-coordinate
            0,                       // y-coordinate
            1,                       // width
            1,                       // height
            0,                       // border width
            WINDOW_CLASS_INPUT_ONLY, // class (i _think_ 0 == COPY_FROM_PARENT?)
            0,                       // visual (i _think_ 0 == COPY_FROM_PARENT?)
            &[],                     // value list? (value mask? not documented either way...)
        );

        let randr_base = conn
            .get_extension_data(&mut xcb::randr::id())
            .unwrap()
            .first_event();

        // xcb docs: https://www.mankier.com/3/xcb_randr_select_input
        if let Err(e) = xcb::randr::select_input(&conn, root, NOTIFY_MASK).request_check() {
            panic!("xrandr error: {}", e);
        }

        XcbConnection {
            conn,
            root,
            check_win,
            atoms,
            auto_float_types,
            randr_base,
        }
    }

    fn atom(&self, name: &str) -> u32 {
        *self
            .atoms
            .get(name)
            .expect(&format!("{} is not a known atom", name))
    }

    fn window_geometry(&self, id: WinId) -> Result<Region, String> {
        let cookie = xcb::get_geometry(&self.conn, id);

        match cookie.get_reply() {
            Err(e) => Err(format!("unable to fetch window property: {}", e)),
            Ok(r) => Ok(Region::new(
                r.x() as u32,
                r.y() as u32,
                r.width() as u32,
                r.height() as u32,
            )),
        }
    }

    fn window_has_type_in(&self, id: WinId, win_types: &Vec<u32>) -> bool {
        // xcb docs: https://www.mankier.com/3/xcb_get_property
        let cookie = xcb::get_property(
            &self.conn,                       // xcb connection to X11
            false,                            // should the property be deleted
            id,                               // target window to query
            self.atom("_NET_WM_WINDOW_TYPE"), // the property we want
            xcb::ATOM_ANY,                    // the type of the property
            0,                                // offset in the property to retrieve data from
            2048,                             // how many 32bit multiples of data to retrieve
        );

        match cookie.get_reply() {
            Err(_) => false,
            Ok(types) => types.value().iter().any(|t| win_types.contains(t)),
        }
    }
}

impl XConn for XcbConnection {
    fn flush(&self) -> bool {
        self.conn.flush()
    }

    fn wait_for_event(&self) -> Option<XEvent> {
        self.conn.wait_for_event().and_then(|event| {
            let etype = event.response_type();
            // Need to apply the randr_base mask as well which doesn't seem to work in 'match'
            if etype == self.randr_base + xcb::randr::NOTIFY {
                return Some(XEvent::RandrNotify);
            }

            match etype {
                xcb::BUTTON_PRESS => None,

                xcb::BUTTON_RELEASE => None,

                xcb::KEY_PRESS => {
                    let e: &xcb::KeyPressEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::KeyPress {
                        code: KeyCode::from_key_press(e),
                    })
                }

                xcb::MAP_NOTIFY => {
                    let e: &xcb::MapNotifyEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::Map {
                        id: e.window(),
                        ignore: e.override_redirect(),
                    })
                }

                xcb::ENTER_NOTIFY => {
                    let e: &xcb::EnterNotifyEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::Enter {
                        id: e.event(),
                        rpt: Point::new(e.root_x() as u32, e.root_y() as u32),
                        wpt: Point::new(e.event_x() as u32, e.event_y() as u32),
                    })
                }

                xcb::LEAVE_NOTIFY => {
                    let e: &xcb::LeaveNotifyEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::Leave {
                        id: e.event(),
                        rpt: Point::new(e.root_x() as u32, e.root_y() as u32),
                        wpt: Point::new(e.event_x() as u32, e.event_y() as u32),
                    })
                }

                xcb::FOCUS_IN => {
                    let e: &xcb::FocusInEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::FocusIn { id: e.event() })
                }

                xcb::FOCUS_OUT => {
                    let e: &xcb::FocusOutEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::FocusOut { id: e.event() })
                }

                xcb::DESTROY_NOTIFY => {
                    let e: &xcb::MapNotifyEvent = unsafe { xcb::cast_event(&event) };
                    Some(XEvent::Destroy { id: e.window() })
                }

                xcb::randr::SCREEN_CHANGE_NOTIFY => Some(XEvent::ScreenChange),

                // NOTE: ignoring other event types
                _ => None,
            }
        })
    }

    fn current_outputs(&self) -> Vec<Screen> {
        // xcb docs: https://www.mankier.com/3/xcb_randr_get_screen_resources
        let resources = xcb::randr::get_screen_resources(&self.conn, self.check_win);

        // xcb docs: https://www.mankier.com/3/xcb_randr_get_crtc_info
        match resources.get_reply() {
            Err(e) => panic!("error reading X screen resources: {}", e),
            Ok(reply) => reply
                .crtcs()
                .iter()
                .flat_map(|c| xcb::randr::get_crtc_info(&self.conn, *c, 0).get_reply())
                .enumerate()
                .map(|(i, r)| Screen::from_crtc_info_reply(r, i))
                .filter(|s| {
                    let (_, _, w, _) = s.true_region.values();
                    w > 0
                })
                .collect(),
        }
    }

    fn cursor_position(&self) -> Point {
        let cookie = xcb::query_pointer(&self.conn, self.root);
        match cookie.get_reply() {
            Err(_) => Point::new(0, 0),
            Ok(reply) => Point::new(reply.root_x() as u32, reply.root_y() as u32),
        }
    }

    fn position_window(&self, id: WinId, r: Region, border: u32) {
        let (x, y, w, h) = r.values();

        // xcb docs: https://www.mankier.com/3/xcb_configure_window
        xcb::configure_window(
            &self.conn,
            id,
            &[
                (WIN_X, x),
                (WIN_Y, y),
                (WIN_WIDTH, w),
                (WIN_HEIGHT, h),
                (WIN_BORDER, border),
                (STACK_MODE, STACK_ABOVE),
            ],
        );
    }

    fn mark_new_window(&self, id: WinId) {
        // xcb docs: https://www.mankier.com/3/xcb_change_window_attributes
        xcb::change_window_attributes(&self.conn, id, NEW_WINDOW_MASK);
    }

    fn map_window(&self, id: WinId) {
        xcb::map_window(&self.conn, id);
    }

    fn unmap_window(&self, id: WinId) {
        xcb::unmap_window(&self.conn, id);
    }

    fn send_client_event(&self, id: WinId, atom_name: &str) {
        let atom = self.atom(atom_name);
        let wm_protocols = self.atom("WM_PROTOCOLS");
        let data = xcb::ClientMessageData::from_data32([atom, xcb::CURRENT_TIME, 0, 0, 0]);
        let event = xcb::ClientMessageEvent::new(32, id, wm_protocols, data);
        xcb::send_event(&self.conn, false, id, xcb::EVENT_MASK_NO_EVENT, &event);
    }

    fn focused_client(&self) -> WinId {
        // xcb docs: https://www.mankier.com/3/xcb_get_input_focus
        match xcb::get_input_focus(&self.conn).get_reply() {
            Err(_) => 0,
            Ok(resp) => resp.focus(),
        }
    }

    fn focus_client(&self, id: WinId) {
        let prop = self.atom("_NET_ACTIVE_WINDOW");

        // xcb docs: https://www.mankier.com/3/xcb_set_input_focus
        xcb::set_input_focus(
            &self.conn,         // xcb connection to X11
            INPUT_FOCUS_PARENT, // focus the parent when focus is lost
            id,                 // window to focus
            0,                  // current time to avoid network race conditions (0 == current time)
        );

        // xcb docs: https://www.mankier.com/3/xcb_change_property
        xcb::change_property(
            &self.conn,        // xcb connection to X11
            PROP_MODE_REPLACE, // discard current prop and replace
            self.root,         // window to change prop on
            prop,              // prop to change
            ATOM_WINDOW,       // type of prop
            32,                // data format (8/16/32-bit)
            &[id],             // data
        );
    }

    fn set_client_border_color(&self, id: WinId, color: u32) {
        xcb::change_window_attributes(&self.conn, id, &[(xcb::CW_BORDER_PIXEL, color)]);
    }

    fn grab_keys(&self, key_bindings: &KeyBindings) {
        for k in key_bindings.keys() {
            // xcb docs: https://www.mankier.com/3/xcb_grab_key
            xcb::grab_key(
                &self.conn,      // xcb connection to X11
                false,           // don't pass grabbed events through to the client
                self.root,       // the window to grab: in this case the root window
                k.mask,          // modifiers to grab
                k.code,          // keycode to grab
                GRAB_MODE_ASYNC, // don't lock pointer input while grabbing
                GRAB_MODE_ASYNC, // don't lock keyboard input while grabbing
            );
        }

        // TODO: this needs to be more configurable by the user
        for mouse_button in &[1, 3] {
            // xcb docs: https://www.mankier.com/3/xcb_grab_button
            xcb::grab_button(
                &self.conn,             // xcb connection to X11
                false,                  // don't pass grabbed events through to the client
                self.root,              // the window to grab: in this case the root window
                MOUSE_MASK,             // which events are reported to the client
                GRAB_MODE_ASYNC,        // don't lock pointer input while grabbing
                GRAB_MODE_ASYNC,        // don't lock keyboard input while grabbing
                xcb::NONE,              // don't confine the cursor to a specific window
                xcb::NONE,              // don't change the cursor type
                *mouse_button,          // the button to grab
                xcb::MOD_MASK_4 as u16, // modifiers to grab
            );
        }

        // xcb docs: https://www.mankier.com/3/xcb_change_window_attributes
        xcb::change_window_attributes(&self.conn, self.root, EVENT_MASK);
        &self.conn.flush();
    }

    fn set_wm_properties(&self, workspaces: &[&str]) {
        // xcb docs: https://www.mankier.com/3/xcb_change_property
        xcb::change_property(
            &self.conn,                            // xcb connection to X11
            PROP_MODE_REPLACE,                     // discard current prop and replace
            self.check_win,                        // window to change prop on
            self.atom("_NET_SUPPORTING_WM_CHECK"), // prop to change
            ATOM_WINDOW,                           // type of prop
            32,                                    // data format (8/16/32-bit)
            &[self.check_win],                     // data
        );
        xcb::change_property(
            &self.conn,                // xcb connection to X11
            PROP_MODE_REPLACE,         // discard current prop and replace
            self.check_win,            // window to change prop on
            self.atom("_NET_WM_NAME"), // prop to change
            self.atom("UTF8_STRING"),  // type of prop
            8,                         // data format (8/16/32-bit)
            WM_NAME.as_bytes(),        // data
        );
        xcb::change_property(
            &self.conn,                            // xcb connection to X11
            PROP_MODE_REPLACE,                     // discard current prop and replace
            self.root,                             // window to change prop on
            self.atom("_NET_SUPPORTING_WM_CHECK"), // prop to change
            ATOM_WINDOW,                           // type of prop
            32,                                    // data format (8/16/32-bit)
            &[self.check_win],                     // data
        );
        xcb::change_property(
            &self.conn,                // xcb connection to X11
            PROP_MODE_REPLACE,         // discard current prop and replace
            self.root,                 // window to change prop on
            self.atom("_NET_WM_NAME"), // prop to change
            self.atom("UTF8_STRING"),  // type of prop
            8,                         // data format (8/16/32-bit)
            WM_NAME.as_bytes(),        // data
        );

        // EWMH support
        let supported: Vec<u32> = ATOMS.iter().map(|a| self.atom(a)).collect();
        xcb::change_property(
            &self.conn,                  // xcb connection to X11
            PROP_MODE_REPLACE,           // discard current prop and replace
            self.root,                   // window to change prop on
            self.atom("_NET_SUPPORTED"), // prop to change
            xcb::xproto::ATOM_ATOM,      // type of prop
            32,                          // data format (8/16/32-bit)
            &supported,                  // data
        );
        self.update_desktops(workspaces);

        xcb::delete_property(&self.conn, self.root, self.atom("_NET_CLIENT_LIST"));
    }

    fn update_desktops(&self, workspaces: &[&str]) {
        xcb::change_property(
            &self.conn,                           // xcb connection to X11
            PROP_MODE_REPLACE,                    // discard current prop and replace
            self.root,                            // window to change prop on
            self.atom("_NET_NUMBER_OF_DESKTOPS"), // prop to change
            xcb::xproto::ATOM_CARDINAL,           // type of prop
            32,                                   // data format (8/16/32-bit)
            &[workspaces.len() as u32],           // data
        );
        xcb::change_property(
            &self.conn,                       // xcb connection to X11
            PROP_MODE_REPLACE,                // discard current prop and replace
            self.root,                        // window to change prop on
            self.atom("_NET_DESKTOP_NAMES"),  // prop to change
            self.atom("UTF8_STRING"),         // type of prop
            8,                                // data format (8/16/32-bit)
            workspaces.join("\0").as_bytes(), // data
        );
    }

    fn set_current_workspace(&self, wix: usize) {
        xcb::change_property(
            &self.conn,                        // xcb connection to X11
            PROP_MODE_REPLACE,                 // discard current prop and replace
            self.root,                         // window to change prop on
            self.atom("_NET_CURRENT_DESKTOP"), // prop to change
            xcb::xproto::ATOM_CARDINAL,        // type of prop
            32,                                // data format (8/16/32-bit)
            &[wix as u32],                     // data
        );
    }

    fn set_root_window_name(&self, name: &str) {
        xcb::change_property(
            &self.conn,               // xcb connection to X11
            PROP_MODE_REPLACE,        // discard current prop and replace
            self.root,                // window to change prop on
            self.atom("WM_NAME"),     // prop to change
            self.atom("UTF8_STRING"), // type of prop
            8,                        // data format (8/16/32-bit)
            name.as_bytes(),          // data
        );
    }

    fn set_client_workspace(&self, id: WinId, wix: usize) {
        xcb::change_property(
            &self.conn,                   // xcb connection to X11
            PROP_MODE_REPLACE,            // discard current prop and replace
            id,                           // window to change prop on
            self.atom("_NET_WM_DESKTOP"), // prop to change
            xcb::xproto::ATOM_CARDINAL,   // type of prop
            32,                           // data format (8/16/32-bit)
            &[wix as u32],                // data
        );
    }

    fn window_should_float(&self, id: WinId, floating_classes: &[&str]) -> bool {
        match self.str_prop(id, "WM_CLASS") {
            Ok(s) => {
                if s.split("\0").any(|c| floating_classes.contains(&c)) {
                    return true;
                }
            }
            Err(_) => (), // no WM_CLASS set
        };

        // self.window_has_type_in(id, &self.auto_float_types)
        // xcb docs: https://www.mankier.com/3/xcb_get_property
        let cookie = xcb::get_property(
            &self.conn,                       // xcb connection to X11
            false,                            // should the property be deleted
            id,                               // target window to query
            self.atom("_NET_WM_WINDOW_TYPE"), // the property we want
            xcb::ATOM_ANY,                    // the type of the property
            0,                                // offset in the property to retrieve data from
            2048,                             // how many 32bit multiples of data to retrieve
        );

        match cookie.get_reply() {
            Err(_) => false,
            Ok(types) => types
                .value()
                .iter()
                .any(|t| self.auto_float_types.contains(t)),
        }
    }

    fn warp_cursor(&self, win_id: Option<WinId>, screen: &Screen) {
        let (x, y, id) = match win_id {
            Some(id) => {
                let (_, _, w, h) = self.window_geometry(id).unwrap().values();
                ((w / 2) as i16, (h / 2) as i16, id)
            }
            None => {
                let (x, y, w, h) = screen.effective_region.values();
                ((x + w / 2) as i16, (y + h / 2) as i16, self.root)
            }
        };

        xcb::warp_pointer(
            &self.conn, // xcb connection to X11
            0,          // source window
            id,         // destination window
            0,          // source x
            0,          // source y
            0,          // source width
            0,          // source height
            x,          // destination x
            y,          // destination y
        );
    }

    fn query_for_active_windows(&self) -> Vec<WinId> {
        let all_ids = match xcb::query_tree(&self.conn, self.root).get_reply() {
            Err(_) => Vec::new(),
            Ok(reply) => reply.children().into(),
        };

        let dont_manage: Vec<u32> = ["_NET_WM_WINDOW_TYPE_DOCK", "_NET_WM_WINDOW_TYPE_TOOLBAR"]
            .iter()
            .map(|t| self.atom(t))
            .collect();

        all_ids
            .iter()
            .filter(|id| !self.window_has_type_in(**id, &dont_manage))
            .cloned()
            .collect()
    }

    fn str_prop(&self, id: u32, name: &str) -> Result<String, String> {
        // xcb docs: https://www.mankier.com/3/xcb_get_property
        let cookie = xcb::get_property(
            &self.conn,      // xcb connection to X11
            false,           // should the property be deleted
            id,              // target window to query
            self.atom(name), // the property we want
            xcb::ATOM_ANY,   // the type of the property
            0,               // offset in the property to retrieve data from
            1024,            // how many 32bit multiples of data to retrieve
        );

        match cookie.get_reply() {
            Err(e) => Err(format!("unable to fetch window property: {}", e)),
            Ok(reply) => match String::from_utf8(reply.value().to_vec()) {
                Err(e) => Err(format!("invalid utf8 resonse from xcb: {}", e)),
                Ok(s) => Ok(s),
            },
        }
    }

    fn atom_prop(&self, id: u32, name: &str) -> Result<u32, String> {
        // xcb docs: https://www.mankier.com/3/xcb_get_property
        let cookie = xcb::get_property(
            &self.conn,      // xcb connection to X11
            false,           // should the property be deleted
            id,              // target window to query
            self.atom(name), // the property we want
            xcb::ATOM_ANY,   // the type of the property
            0,               // offset in the property to retrieve data from
            1024,            // how many 32bit multiples of data to retrieve
        );

        match cookie.get_reply() {
            Err(e) => Err(format!("unable to fetch window property: {}", e)),
            Ok(reply) => {
                if reply.value_len() <= 0 {
                    Err(format!("property '{}' was empty for id: {}", name, id))
                } else {
                    Ok(reply.value()[0])
                }
            }
        }
    }

    // - Release all of the keybindings we are holding on to
    // - destroy the check window
    // - mark ourselves as no longer being the active root window
    // - TODO: tidy embedded bar / systray once this is a thing
    // - TODO: tidy up any graphics context we have
    fn cleanup(&self) {
        // xcb docs: https://www.mankier.com/3/xcb_ungrab_key
        xcb::ungrab_key(
            &self.conn, // xcb connection to X11
            xcb::GRAB_ANY as u8,
            self.root, // the window to ungrab keys for
            xcb::MOD_MASK_ANY as u16,
        );
        xcb::destroy_window(&self.conn, self.check_win);
        xcb::delete_property(&self.conn, self.root, self.atom("_NET_ACTIVE_WINDOW"));
    }
}

/// A dummy XConn implementation for testing
pub struct MockXConn {
    screens: Vec<Screen>,
    events: Cell<Vec<XEvent>>,
    focused: Cell<WinId>,
}

impl MockXConn {
    /// Set up a new MockXConn with pre-defined Screens and an event stream to pull from
    pub fn new(screens: Vec<Screen>, events: Vec<XEvent>) -> Self {
        MockXConn {
            screens,
            events: Cell::new(events),
            focused: Cell::new(0),
        }
    }
}

impl XConn for MockXConn {
    fn flush(&self) -> bool {
        true
    }
    fn wait_for_event(&self) -> Option<XEvent> {
        let mut remaining = self.events.replace(vec![]);
        if remaining.len() == 0 {
            return None;
        }
        let next = remaining.remove(0);
        self.events.set(remaining);
        Some(next)
    }
    fn current_outputs(&self) -> Vec<Screen> {
        self.screens.clone()
    }
    fn cursor_position(&self) -> Point {
        Point::new(0, 0)
    }
    fn position_window(&self, _: WinId, _: Region, _: u32) {}
    fn mark_new_window(&self, _: WinId) {}
    fn map_window(&self, _: WinId) {}
    fn unmap_window(&self, _: WinId) {}
    fn send_client_event(&self, _: WinId, _: &str) {}
    fn focused_client(&self) -> WinId {
        self.focused.get()
    }
    fn focus_client(&self, id: WinId) {
        self.focused.replace(id);
    }
    fn set_client_border_color(&self, _: WinId, _: u32) {}
    fn grab_keys(&self, _: &KeyBindings) {}
    fn set_wm_properties(&self, _: &[&str]) {}
    fn update_desktops(&self, _: &[&str]) {}
    fn set_current_workspace(&self, _: usize) {}
    fn set_root_window_name(&self, _: &str) {}
    fn set_client_workspace(&self, _: WinId, _: usize) {}
    fn window_should_float(&self, _: WinId, _: &[&str]) -> bool {
        true
    }
    fn warp_cursor(&self, _: Option<WinId>, _: &Screen) {}
    fn query_for_active_windows(&self) -> Vec<WinId> {
        Vec::new()
    }
    fn str_prop(&self, _: u32, name: &str) -> Result<String, String> {
        Ok(String::from(name))
    }
    fn atom_prop(&self, id: u32, _: &str) -> Result<u32, String> {
        Ok(id)
    }
    fn cleanup(&self) {}
}