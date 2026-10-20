//! Compose mode: a real text field above the key bar, so autocorrect,
//! dictation, and editing all work. Send (or Return) delivers it as one paste
//! followed by Enter. Keys mode hides it and goes back to sending every key
//! as it's typed.

use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlElement, HtmlTextAreaElement, InputEvent, KeyboardEvent};

use crate::app::App;
use crate::{Shared, listen};

const MODE_KEY: &str = "clawsh.mode";

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn draft() -> HtmlTextAreaElement {
    document().get_element_by_id("draft").unwrap().dyn_into().unwrap()
}

fn sink() -> HtmlTextAreaElement {
    document().get_element_by_id("sink").unwrap().dyn_into().unwrap()
}

/// The field that takes typing in the current mode.
pub fn input(app: &App) -> HtmlTextAreaElement {
    if app.compose { draft() } else { sink() }
}

pub fn focused(el: &HtmlTextAreaElement) -> bool {
    document().active_element().is_some_and(|a| a == ***el)
}

/// Ratzilla only re-measures the grid on a window resize, so anything that
/// changes how much room the terminal gets has to send one.
fn remeasure() {
    let window = web_sys::window().unwrap();
    let _ = window.dispatch_event(&Event::new("resize").unwrap());
}

/// Picks up the mode the phone used last, if the browser kept it.
pub fn restore(app: &mut App) {
    let saved = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(MODE_KEY).ok().flatten());
    if saved.as_deref() == Some("keys") {
        set_mode(app, false);
    }
}

pub fn set_mode(app: &mut App, compose: bool) {
    let keyboard_up = focused(&input(app));
    app.compose = compose;
    let mode = if compose { "compose" } else { "keys" };
    if let Some(el) = document().get_element_by_id("app") {
        let _ = el.set_attribute("data-mode", mode);
    }
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(MODE_KEY, mode);
    }
    // Keep the keyboard up across the switch if it was up.
    if keyboard_up {
        let _ = input(app).focus();
    }
    remeasure();
}

/// Grows the field with its text, up to the CSS max-height.
fn fit(draft: &HtmlTextAreaElement) {
    let el: &HtmlElement = draft;
    let before = el.offset_height();
    let style = el.style();
    let _ = style.set_property("height", "auto");
    let _ = style.set_property("height", &format!("{}px", draft.scroll_height() + 2));
    if el.offset_height() != before {
        remeasure();
    }
}

fn send(app: &Shared<App>, draft: &HtmlTextAreaElement) {
    app.borrow_mut().submit(&draft.value());
    draft.set_value("");
    fit(draft);
}

pub fn wire(app: &Shared<App>) {
    let draft = draft();

    listen(&draft, "input", {
        let draft = draft.clone();
        move |_: Event| fit(&draft)
    });

    listen(&draft, "beforeinput", {
        let app = app.clone();
        let draft = draft.clone();
        move |e: InputEvent| {
            if e.is_composing() {
                return;
            }
            let kind = e.input_type();
            // Return on a phone keyboard sends, like Claude's own prompt.
            if matches!(kind.as_str(), "insertLineBreak" | "insertParagraph") {
                e.prevent_default();
                send(&app, &draft);
                return;
            }
            // Sticky ctrl from the key bar turns the next letter into a
            // ctrl key instead of text.
            let data = e.data().unwrap_or_default();
            let mut app = app.borrow_mut();
            if app.ctrl && kind == "insertText" && data.chars().count() == 1 {
                e.prevent_default();
                app.ctrl_char(data.chars().next().unwrap());
            }
        }
    });

    // A hardware keyboard: Enter sends, shift+Enter is a new line.
    listen(&draft, "keydown", {
        let app = app.clone();
        let draft = draft.clone();
        move |e: KeyboardEvent| {
            if e.key() == "Enter" && !e.shift_key() && !e.is_composing() && e.key_code() != 229 {
                e.prevent_default();
                send(&app, &draft);
            }
        }
    });

    let button = document().get_element_by_id("send").unwrap();
    // Keep focus in the field so the keyboard stays up.
    for kind in ["pointerdown", "mousedown"] {
        listen(&button, kind, |e: Event| e.prevent_default());
    }
    listen(&button, "click", {
        let app = app.clone();
        move |_: Event| send(&app, &draft)
    });
}
