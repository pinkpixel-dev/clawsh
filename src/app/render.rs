//! Draws one screen's worth of Claw$h. The desktop terminal and the phone
//! each get their own layout, so this runs once per screen with that
//! screen's areas.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};

use crate::pane::{CursorState, Pane};
use crate::ui::drawer::{self, DividerView, Side};
use crate::ui::{self, Areas, Hits, View};
use crate::workspace::Workspaces;

pub(super) struct Screen<'a> {
    pub areas: &'a Areas,
    pub side: Side,
    pub percent: u16,
    pub reviewing: bool,
    /// Normal mode, where the pane's own cursor is shown.
    pub normal: bool,
}

pub(super) struct Drawn {
    pub hits: Hits,
    /// Where the cursor ended up on this screen, if anywhere.
    pub cursor: Option<Position>,
    /// The focused pane's cursor shape, for the host terminal.
    pub pane_cursor: Option<CursorState>,
}

/// `remap` turns a pane's image id into the host's; the phone passes one
/// that always says no, which blanks image cells.
pub(super) fn render(
    frame: &mut Frame,
    spaces: &mut Workspaces<Pane>,
    view: &mut View,
    screen: &Screen,
    remap: &dyn Fn(usize, u32) -> Option<u32>,
) -> Result<Drawn> {
    let split = drawer::split(screen.areas.pane, screen.side, screen.percent);
    let tab_drawer = spaces.active_tab().and_then(|p| p.drawer.as_ref());
    view.terminal = tab_drawer.map(|d| d.shown);
    view.divider = match (tab_drawer, split) {
        (Some(d), Some(s)) if d.shown => Some(DividerView { rect: s.divider, side: screen.side, focused: d.focused }),
        _ => None,
    };

    let mut pane_cursor: Option<(CursorState, Rect)> = None;
    let mut review_cursor = None;
    if let Some(pane) = spaces.active_tab_mut() {
        let split = split.filter(|_| pane.drawer.as_ref().is_some_and(|d| d.shown));
        let main = split.map_or(screen.areas.pane, |s| s.main);
        let main_cursor = if screen.reviewing && let Some(review) = pane.review.as_mut() {
            review_cursor = ui::review::draw(frame, main, review);
            None
        } else {
            let id = pane.id();
            pane.render(frame.buffer_mut(), main, |img| remap(id, img))?.map(|c| (c, main))
        };
        // The cursor belongs to whichever pane has the keyboard.
        pane_cursor = match (pane.drawer.as_mut(), split) {
            (Some(d), Some(s)) => {
                let id = d.pane.id();
                let drawer_cursor =
                    d.pane.render(frame.buffer_mut(), s.drawer, |img| remap(id, img))?.map(|c| (c, s.drawer));
                if d.focused { drawer_cursor } else { main_cursor }
            }
            _ => main_cursor,
        };
    }

    let (hits, picker_cursor) = ui::draw_chrome(frame, screen.areas, view);
    let cursor = picker_cursor.or(review_cursor).or_else(|| {
        let (c, area) = pane_cursor.as_ref().filter(|_| screen.normal)?;
        let at = Position::new(area.x + c.position.x, area.y + c.position.y);
        // A cropped pane can have its cursor off this screen.
        area.contains(at).then_some(at)
    });
    if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
    Ok(Drawn { hits, cursor, pane_cursor: pane_cursor.map(|(c, _)| c) })
}
