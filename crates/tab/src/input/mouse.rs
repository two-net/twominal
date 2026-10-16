//! What a mouse or wheel event means (TERM-10, HW-02): a report for a program that asked for
//! the mouse, cursor keys for a full-screen program that did not, or a gesture or a scroll that
//! is Twominal's own.
//!
//! A gesture is given whole to the program or to Twominal, by its first press: to the program
//! if it asked for the mouse and neither Shift nor Meta is held, so Shift+drag still selects.
//! Meta is Cmd on macOS, which opens links, and xterm's reports cannot carry it. The reports
//! are xterm's, from `button.c`; the wheel's steps are xterm.js's. DESIGN.md §3 has the rules.
//!
//! The webview gives the DOM's numbers as they are, so any may arrive. One that is not finite
//! counts as 0, and no number makes the output grow without bound.

use terminal_core::{Modes, MouseTracking};

use super::encode;
use super::key::{Key, Mods, Named, Press};
use super::{
    Action, Button, DeltaMode, Grid, Kind, Modifiers, MouseEvent, MouseKind, Pointer, PointerKind,
    WheelEvent,
};

/// One tab's mouse: the grid it falls on, the gesture in progress and the wheel's part steps.
#[derive(Clone, Debug, Default)]
pub(crate) struct Mouse {
    grid: Option<Grid>,
    /// How far up the view's rows are drawn, in rows: the part of a row the view is scrolled
    /// past its top one (ANIM-06). A Pointer's row is the row drawn under it.
    offset: f64,
    gesture: Option<Gesture>,
    /// The cell of the last report, so a move is reported only in another: xterm's
    /// `mouse_row` and `mouse_col`.
    reported: Option<Cell>,
    /// Where the last Pointer action was, by cell and half, and with which modifiers, so a move
    /// that changes none of them is not one. Forgotten when the program is told of the mouse.
    pointed: Option<(i32, i32, bool, Modifiers)>,
    /// How far the wheel turned, in rows and columns, toward a step not yet taken.
    turned: [f64; 2],
    /// Where the wheel's last turn went. Turning it elsewhere starts the steps again.
    route: Option<Route>,
    /// Where the mouse last was, in CSS pixels from the top left of the first cell, until it left
    /// the terminal area. Its cell is found when asked, with the view's offset then, so the row
    /// under a mouse that does not move follows a view scrolled by a part of a row.
    at: Option<(f64, f64)>,
    /// The modifiers last held, by the last mouse, wheel or key event, for what a click where
    /// the mouse is would do.
    held: Modifiers,
    /// The release the last event gave was made up for one that was lost.
    made_up: bool,
}

/// The buttons pressed since all were up, and whose they are.
#[derive(Clone, Copy, Debug)]
struct Gesture {
    owner: Owner,
    /// The buttons down, as `Button::bit` has them.
    held: u8,
    /// The click count of the press that started it.
    clicks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Program,
    Twominal,
}

/// Where the wheel goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// Reported to the program.
    Program,
    /// The cursor keys, for a full-screen program that did not ask for the mouse (1007).
    Keys,
    /// Twominal's view.
    Twominal,
    /// Nowhere: the program asked for X10's reports, which have no wheel.
    Nowhere,
}

const VERTICAL: usize = 0;
const HORIZONTAL: usize = 1;

/// A cell, clamped to the grid: its row and column, from 0.
type Cell = (u16, u16);

/// Where a mouse event fell.
#[derive(Clone, Copy, Debug)]
struct Place {
    /// The row of the view drawn under it, which is scrolled `offset` rows up.
    row: i32,
    col: i32,
    right_half: bool,
    /// The cell of the grid, clamped to it as xterm clamps what it reports.
    cell: Cell,
}

impl Place {
    fn new(x: f64, y: f64, grid: Grid, offset: f64) -> Self {
        let x = finite(x) / grid.cell_width;
        let y = finite(y) / grid.cell_height;
        let (col, row) = (x.floor(), y.floor());
        let clamp = |at: f64, count: u16| at.clamp(0.0, f64::from(count.max(1) - 1)) as u16;
        Self {
            // `as` saturates, and makes NaN 0.
            row: (y + offset).floor() as i32,
            col: col as i32,
            right_half: x - col >= 0.5,
            cell: (clamp(row, grid.size.rows), clamp(col, grid.size.cols)),
        }
    }
}

impl Mouse {
    pub(crate) fn resize(&mut self, grid: Grid) {
        let size = |size: f64| size.is_finite() && size > 0.0;
        self.grid = (size(grid.cell_width) && size(grid.cell_height)).then_some(grid);
    }

    /// How far up the view's rows are drawn, in rows. Anything but a part of a row is none.
    pub(crate) fn set_offset(&mut self, offset: f64) {
        self.offset = if (0.0..1.0).contains(&offset) {
            offset
        } else {
            0.0
        };
    }

    /// The modifiers held now, as a key event says, or none when the window lost the focus and
    /// their release may never come.
    pub(crate) fn hold(&mut self, modifiers: Modifiers) {
        self.held = modifiers;
    }

    /// Whether the release the last event gave was made up for one that was lost, as when the
    /// button came up outside the window.
    pub(crate) fn made_up(&self) -> bool {
        self.made_up
    }

    /// The view's row and the column the mouse is over, and the modifiers held, while what it
    /// does there is Twominal's: not once it has left the terminal area, nor during a gesture
    /// that is the program's, nor while the program is told of every move, which neither Shift
    /// nor Meta keeps from it.
    pub(crate) fn hover(&self, modes: &Modes) -> Option<(i32, i32, Modifiers)> {
        let (x, y) = self.at?;
        let place = Place::new(x, y, self.grid?, self.offset);
        let ours = match self.gesture {
            Some(gesture) => gesture.owner == Owner::Twominal,
            None => modes.mouse != MouseTracking::Motion || bypasses(self.held),
        };
        ours.then_some((place.row, place.col, self.held))
    }

    /// How many rows the grid has, which is a page. None before there is a grid.
    pub(crate) fn page(&self) -> Option<f64> {
        self.grid.map(|grid| f64::from(grid.size.rows))
    }

    /// What a button going down or up, the mouse moving, or the mouse leaving the terminal
    /// area, means. Leaving means nothing but that the mouse is over no cell: a gesture goes on
    /// outside, and its buttons are looked at when the mouse comes back.
    pub(crate) fn event(&mut self, event: &MouseEvent, modes: &Modes) -> Option<Action> {
        self.made_up = false;
        if event.kind == MouseKind::Leave {
            self.at = None;
            self.pointed = None;
            return None;
        }
        let grid = self.grid?;
        let place = Place::new(event.x, event.y, grid, self.offset);
        self.at = Some((event.x, event.y));
        self.held = event.modifiers;
        let button = Button::from_dom(event.button);
        let pressed = button.map_or(0, Button::bit);
        let (held, up) = match event.kind {
            MouseKind::Down => (held(event.buttons) | pressed, 0),
            MouseKind::Up => (held(event.buttons) & !pressed, pressed),
            MouseKind::Move | MouseKind::Leave => (held(event.buttons), 0),
        };

        // A button of the gesture that is no longer held, or that goes down again, came up
        // where the window could not see it. Its release is told of, as it would have been.
        let mut lost = None;
        if let Some(mut gesture) = self.gesture {
            let again = if event.kind == MouseKind::Down {
                pressed
            } else {
                0
            };
            let gone = gesture.held & (!held | again) & !up;
            if gone != 0 {
                gesture.held &= !gone;
                self.gesture = (gesture.held != 0).then_some(gesture);
                lost = match gesture.owner {
                    Owner::Program => {
                        let mut bytes = Vec::new();
                        for button in Button::ALL.into_iter().filter(|b| gone & b.bit() != 0) {
                            bytes.extend(self.release(button, place, event.modifiers, modes));
                        }
                        (!bytes.is_empty()).then_some(Action::Send(bytes))
                    }
                    Owner::Twominal => {
                        self.made_up = true;
                        Some(self.point(
                            PointerKind::Release,
                            Button::lowest(gone),
                            place,
                            gesture.clicks,
                            event.modifiers,
                        ))
                    }
                };
            }
        }
        let Some(lost) = lost else {
            return self.meaning(event, button, place, modes);
        };

        // One action: the release, with what the event sends the program after it. An event
        // that means anything else is dropped, as if it had not come.
        let before = self.clone();
        match (lost, self.meaning(event, button, place, modes)) {
            (Action::Send(mut bytes), Some(Action::Send(more))) => {
                bytes.extend(more);
                Some(Action::Send(bytes))
            }
            (lost, None) => Some(lost),
            (lost, Some(_)) => {
                *self = before;
                Some(lost)
            }
        }
    }

    /// What the event means of itself.
    fn meaning(
        &mut self,
        event: &MouseEvent,
        button: Option<Button>,
        place: Place,
        modes: &Modes,
    ) -> Option<Action> {
        match (event.kind, button) {
            (MouseKind::Down, Some(button)) => self.down(button, event, place, modes),
            (MouseKind::Up, Some(button)) => self.up(button, event, place, modes),
            (MouseKind::Move | MouseKind::Leave, _) => self.moved(event, place, modes),
            // A button the DOM has no more of.
            (_, None) => None,
        }
    }

    fn down(
        &mut self,
        button: Button,
        event: &MouseEvent,
        place: Place,
        modes: &Modes,
    ) -> Option<Action> {
        let owner = if modes.mouse != MouseTracking::Off && !bypasses(event.modifiers) {
            Owner::Program
        } else {
            Owner::Twominal
        };
        let gesture = self.gesture.get_or_insert(Gesture {
            owner,
            held: 0,
            clicks: event.detail,
        });
        gesture.held |= button.bit();
        match gesture.owner {
            Owner::Program => self.press(button, place, event.modifiers, modes),
            Owner::Twominal => Some(self.point(
                PointerKind::Press,
                Some(button),
                place,
                event.detail,
                event.modifiers,
            )),
        }
    }

    fn up(
        &mut self,
        button: Button,
        event: &MouseEvent,
        place: Place,
        modes: &Modes,
    ) -> Option<Action> {
        // A button whose press was not seen, as one held when the mouse came in from outside.
        let mut gesture = self
            .gesture
            .filter(|gesture| gesture.held & button.bit() != 0)?;
        gesture.held &= !button.bit();
        self.gesture = (gesture.held != 0).then_some(gesture);
        match gesture.owner {
            Owner::Program => {
                let bytes = self.release(button, place, event.modifiers, modes);
                (!bytes.is_empty()).then_some(Action::Send(bytes))
            }
            Owner::Twominal => Some(self.point(
                PointerKind::Release,
                Some(button),
                place,
                gesture.clicks,
                event.modifiers,
            )),
        }
    }

    fn moved(&mut self, event: &MouseEvent, place: Place, modes: &Modes) -> Option<Action> {
        match self.gesture {
            Some(Gesture {
                owner: Owner::Program,
                held,
                ..
            }) => match modes.mouse {
                MouseTracking::Drag | MouseTracking::Motion => {
                    self.motion(Button::lowest(held), place, event.modifiers, modes)
                }
                // Mode 9 and 1000 report no moves, and the gesture is still the program's.
                MouseTracking::Press | MouseTracking::Click | MouseTracking::Off => None,
            },
            Some(Gesture {
                owner: Owner::Twominal,
                held,
                clicks,
            }) => self.point_moved(
                PointerKind::Drag,
                Button::lowest(held),
                place,
                clicks,
                event.modifiers,
            ),
            // Buttons held with no gesture came down outside the grid, so they count for
            // nothing here.
            None if modes.mouse == MouseTracking::Motion && !bypasses(event.modifiers) => {
                self.motion(None, place, event.modifiers, modes)
            }
            None => self.point_moved(PointerKind::Hover, None, place, 0, event.modifiers),
        }
    }

    // Reports

    fn press(
        &mut self,
        button: Button,
        place: Place,
        modifiers: Modifiers,
        modes: &Modes,
    ) -> Option<Action> {
        let code = match modes.mouse {
            MouseTracking::Off => return None,
            // X10's reports: presses of the first three buttons, with no modifiers.
            MouseTracking::Press => match button {
                Button::Left | Button::Middle | Button::Right => button.code(),
                Button::Back | Button::Forward => return None,
            },
            MouseTracking::Click | MouseTracking::Drag | MouseTracking::Motion => {
                button.code() | modifier_code(modifiers)
            }
        };
        Some(Action::Send(self.report(code, false, place, modes)))
    }

    fn release(
        &mut self,
        button: Button,
        place: Place,
        modifiers: Modifiers,
        modes: &Modes,
    ) -> Vec<u8> {
        match modes.mouse {
            MouseTracking::Off | MouseTracking::Press => Vec::new(),
            MouseTracking::Click | MouseTracking::Drag | MouseTracking::Motion => {
                let code = button.code() | modifier_code(modifiers);
                self.report(code, true, place, modes)
            }
        }
    }

    /// A move, with `button` held, in another cell than the last report's.
    fn motion(
        &mut self,
        button: Option<Button>,
        place: Place,
        modifiers: Modifiers,
        modes: &Modes,
    ) -> Option<Action> {
        if self.reported == Some(place.cell) {
            return None;
        }
        // 3 is no button, as for a release.
        let code = button.map_or(3, Button::code) | modifier_code(modifiers) | 32;
        Some(Action::Send(self.report(code, false, place, modes)))
    }

    fn report(&mut self, code: u8, release: bool, place: Place, modes: &Modes) -> Vec<u8> {
        self.reported = Some(place.cell);
        self.pointed = None;
        let (row, col) = place.cell;
        encode::mouse(code, release, row, col, modes.mouse_format)
    }

    // Twominal's own

    fn point(
        &mut self,
        kind: PointerKind,
        button: Option<Button>,
        place: Place,
        clicks: u32,
        modifiers: Modifiers,
    ) -> Action {
        self.pointed = Some((place.row, place.col, place.right_half, modifiers));
        Action::Pointer(Pointer {
            kind,
            button,
            row: place.row,
            col: place.col,
            right_half: place.right_half,
            clicks,
            modifiers,
        })
    }

    /// A move, if it left the cell or the half of it the last Pointer action was in, or the
    /// modifiers changed, as Cmd going down over a link.
    fn point_moved(
        &mut self,
        kind: PointerKind,
        button: Option<Button>,
        place: Place,
        clicks: u32,
        modifiers: Modifiers,
    ) -> Option<Action> {
        (self.pointed != Some((place.row, place.col, place.right_half, modifiers)))
            .then(|| self.point(kind, button, place, clicks, modifiers))
    }

    // The wheel

    /// What the wheel turning, or a trackpad scrolling, means. Only the axis it turned most
    /// counts, so a trackpad drifting sideways reports nothing.
    pub(crate) fn wheel(&mut self, event: &WheelEvent, modes: &Modes) -> Option<Action> {
        let grid = self.grid?;
        let place = Place::new(event.x, event.y, grid, self.offset);
        self.at = Some((event.x, event.y));
        self.held = event.modifiers;
        let (mut delta_y, mut delta_x) = (finite(event.delta_y), finite(event.delta_x));
        // macOS and Chromium turn a wheel sideways while Shift is held, and Shift scrolls the
        // view.
        if event.modifiers.shift && delta_y == 0.0 {
            (delta_y, delta_x) = (delta_x, 0.0);
        }
        // The axis is chosen by the deltas, which share a unit, where a cell does not.
        let (axis, delta) = if delta_y.abs() >= delta_x.abs() {
            (VERTICAL, delta_y)
        } else {
            (HORIZONTAL, delta_x)
        };
        let (cell, cells) = match axis {
            VERTICAL => (grid.cell_height, grid.size.rows),
            _ => (grid.cell_width, grid.size.cols),
        };
        let amount = match event.delta_mode {
            DeltaMode::Pixel => delta / cell,
            DeltaMode::Line => delta,
            DeltaMode::Page => delta * f64::from(cells),
        };
        // A delta over a cell a hair wide is no number either.
        if amount == 0.0 || !amount.is_finite() {
            return None;
        }

        let route = if bypasses(event.modifiers) {
            Route::Twominal
        } else {
            match modes.mouse {
                MouseTracking::Click | MouseTracking::Drag | MouseTracking::Motion => {
                    Route::Program
                }
                // xterm reports no wheel in X10's mode, and takes it.
                MouseTracking::Press => Route::Nowhere,
                MouseTracking::Off if modes.alternate_screen && modes.alternate_scroll => {
                    Route::Keys
                }
                MouseTracking::Off => Route::Twominal,
            }
        };
        if self.route != Some(route) {
            self.route = Some(route);
            self.turned = [0.0; 2];
        }
        match route {
            Route::Nowhere => None,
            // The view scrolls only up and down.
            Route::Twominal => (axis == VERTICAL).then_some(Action::Scroll(amount)),
            Route::Program => {
                let turned = self.turn(axis, amount);
                if turned.abs() < 1.0 {
                    return None;
                }
                // One report however far it turned, so a notch is one step for the program,
                // however many pixels the OS makes of it. What is left of a row is kept, but
                // no more: a flick is not owed afterwards.
                self.turned[axis] = turned.fract();
                let button = match (axis, turned < 0.0) {
                    (VERTICAL, true) => 64,
                    (VERTICAL, false) => 65,
                    (_, true) => 66,
                    (_, false) => 67,
                };
                let code = button | modifier_code(event.modifiers);
                Some(Action::Send(self.report(code, false, place, modes)))
            }
            Route::Keys if axis == VERTICAL => {
                let turned = self.turn(axis, amount);
                self.turned[axis] = turned.fract();
                // A key for each row, but no more than a screen of them.
                let steps = turned.abs().min(f64::from(grid.size.rows)) as usize;
                if steps == 0 {
                    return None;
                }
                let key = if turned < 0.0 { Named::Up } else { Named::Down };
                Some(Action::Send(cursor_key(key, modes).repeat(steps)))
            }
            Route::Keys => None,
        }
    }

    /// Adds `amount` to how far the wheel turned on `axis`, from nothing when it turned back.
    fn turn(&mut self, axis: usize, amount: f64) -> f64 {
        let turned = &mut self.turned[axis];
        if *turned != 0.0 && turned.is_sign_negative() != amount.is_sign_negative() {
            *turned = 0.0;
        }
        *turned += amount;
        // Ten turns of a tenth of a row make a row, which floating point misses by a hair.
        if (*turned - turned.round()).abs() < 1e-9 {
            *turned = turned.round();
        }
        *turned
    }
}

impl Button {
    /// Every button, the lowest-numbered first.
    const ALL: [Self; 5] = [
        Self::Left,
        Self::Middle,
        Self::Right,
        Self::Back,
        Self::Forward,
    ];

    /// The button the DOM's `button` names.
    fn from_dom(button: i16) -> Option<Self> {
        Some(match button {
            0 => Self::Left,
            1 => Self::Middle,
            2 => Self::Right,
            3 => Self::Back,
            4 => Self::Forward,
            _ => return None,
        })
    }

    /// Its number in xterm's reports: X's buttons 1 to 3, 8 and 9.
    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
            Self::Back => 128,
            Self::Forward => 129,
        }
    }

    /// A bit of its own, the lowest-numbered button's the lowest.
    fn bit(self) -> u8 {
        1 << self as u8
    }

    /// The lowest-numbered of `held`, which xterm reports a move with.
    fn lowest(held: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|button| held & button.bit() != 0)
    }
}

/// The DOM's `buttons` as `Button::bit` has them.
fn held(buttons: u16) -> u8 {
    [
        (1, Button::Left),
        (2, Button::Right),
        (4, Button::Middle),
        (8, Button::Back),
        (16, Button::Forward),
    ]
    .into_iter()
    .filter(|&(dom, _)| buttons & dom != 0)
    .fold(0, |held, (_, button)| held | button.bit())
}

/// Whether the modifiers keep the mouse from the program: Shift, so the user can select over a
/// program that has the mouse, and Meta, which xterm cannot report and Cmd+click needs.
pub(crate) fn bypasses(modifiers: Modifiers) -> bool {
    modifiers.shift || modifiers.meta
}

/// The modifiers in a report: xterm's Meta, which is Alt and Option, 8, and Ctrl 16. Shift
/// never reaches a report.
fn modifier_code(modifiers: Modifiers) -> u8 {
    u8::from(modifiers.alt) * 8 + u8::from(modifiers.ctrl) * 16
}

/// The cursor key the wheel stands for, as the key would send it in the modes the program
/// set: DECCKM, and the kitty keyboard protocol, which hears it come up too, as kitty sends it.
fn cursor_key(key: Named, modes: &Modes) -> Vec<u8> {
    let press = Press {
        key: Key::Named(key),
        typed: String::new(),
        mods: Mods::default(),
        kind: Kind::Press,
        us: None,
    };
    let release = Press {
        kind: Kind::Release,
        ..press.clone()
    };
    let mut bytes = encode::key(&press, modes);
    bytes.extend(encode::key(&release, modes));
    bytes
}

fn finite(number: f64) -> f64 {
    if number.is_finite() {
        number
    } else {
        0.0
    }
}
