//! Ratzilla spike for the Claw$h phone client.
//!
//! Ratzilla only paints. Input is handled here instead of through Ratzilla's
//! `on_key_event`/`on_mouse_event`, because those attach to a grid element the
//! DOM backend rebuilds on every resize, and a focusable div doesn't bring up
//! a phone keyboard. A hidden textarea catches typing, the key bar covers what
//! a phone keyboard can't send, and pointer events handle taps and scrolling.

mod app;
mod ui;

use std::{cell::RefCell, rc::Rc};

use ratzilla::{
    DomBackend,
    ratatui::{Terminal, TerminalOptions, Viewport, backend::Backend, layout::{Position, Rect}},
};
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{
    CompositionEvent, Element, Event, EventTarget, HtmlElement, HtmlTextAreaElement, InputEvent,
    KeyboardEvent, PointerEvent, WheelEvent,
};

use app::{App, Key};

type Shared<T> = Rc<RefCell<T>>;

/// Kept in the textarea so Android keyboards have something to delete;
/// with an empty field, backspace often sends nothing at all.
const SENTINEL: &str = "  ";

fn main() {
    let window = web_sys::window().expect("window");
    let document = window.document().expect("document");
    let term_el = document.get_element_by_id("term").expect("#term");
    let sink: HtmlTextAreaElement = document.get_element_by_id("sink").expect("#sink").dyn_into().unwrap();
    reset_sink(&sink);

    let backend = DomBackend::new_by_id("term").expect("dom backend");
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport: Viewport::Fixed(Rect::ZERO) }).unwrap();
    let size = terminal.backend_mut().window_size().unwrap();
    terminal.resize(Rect::new(0, 0, size.columns_rows.width, size.columns_rows.height)).unwrap();

    let app: Shared<App> = Rc::new(RefCell::new(App::new()));
    let terminal = Rc::new(RefCell::new(terminal));
    let resized = Rc::new(RefCell::new(false));

    listen(&window, "resize", { let resized = resized.clone(); move |_: Event| *resized.borrow_mut() = true });
    wire_viewport(&window);
    wire_sink(&sink, &app);
    wire_keys(&document, &sink, &app);
    wire_pointer(&term_el, &sink, &app);

    render_loop(terminal, app, resized);
}

fn render_loop(terminal: Shared<Terminal<DomBackend>>, app: Shared<App>, resized: Shared<bool>) {
    let frame_cb: Shared<Option<Closure<dyn FnMut()>>> = Rc::new(RefCell::new(None));
    let next = frame_cb.clone();
    *frame_cb.borrow_mut() = Some(Closure::new(move || {
        let mut terminal = terminal.borrow_mut();
        if std::mem::take(&mut *resized.borrow_mut()) {
            // The backend re-measures its grid inside draw, but the terminal's
            // buffer is still the old size. An empty draw lets the backend
            // rebuild without indexing past its new cells, then we resize to match.
            terminal.resize(Rect::ZERO).unwrap();
            terminal.draw(|_| {}).unwrap();
            let size = terminal.backend_mut().window_size().unwrap();
            terminal.resize(Rect::new(0, 0, size.columns_rows.width, size.columns_rows.height)).unwrap();
        }
        let size = terminal.backend_mut().window_size().unwrap();
        let mut app = app.borrow_mut();
        if size.columns_rows.width > 0 && size.columns_rows.height > 0 {
            app.cell = (
                size.pixels.width as f64 / size.columns_rows.width as f64,
                size.pixels.height as f64 / size.columns_rows.height as f64,
            );
        }
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        sync_toggles(&app);
        request_frame(next.borrow().as_ref().unwrap());
    }));
    request_frame(frame_cb.borrow().as_ref().unwrap());
}

fn request_frame(cb: &Closure<dyn FnMut()>) {
    web_sys::window().unwrap().request_animation_frame(cb.as_ref().unchecked_ref()).unwrap();
}

/// Keeps #app sized to the visible viewport so the key bar sits above the
/// phone keyboard. iOS doesn't fire a window resize when the keyboard opens,
/// so we send one ourselves to make Ratzilla re-measure.
fn wire_viewport(window: &web_sys::Window) {
    let Some(viewport) = window.visual_viewport() else { return };
    let apply = {
        let window = window.clone();
        let viewport = viewport.clone();
        move |_: Event| {
            let app: HtmlElement = window.document().unwrap().get_element_by_id("app").unwrap().dyn_into().unwrap();
            let style = app.style();
            style.set_property("height", &format!("{}px", viewport.height())).unwrap();
            style.set_property("transform", &format!("translateY({}px)", viewport.offset_top())).unwrap();
            window.dispatch_event(&Event::new("resize").unwrap()).unwrap();
        }
    };
    listen(&viewport, "resize", apply.clone());
    listen(&viewport, "scroll", apply);
}

fn wire_sink(sink: &HtmlTextAreaElement, app: &Shared<App>) {
    listen(sink, "beforeinput", {
        let app = app.clone();
        let sink = sink.clone();
        move |e: InputEvent| {
            let mut app = app.borrow_mut();
            let kind = e.input_type();
            app.log(format!("beforeinput {kind} {:?}{}", e.data().unwrap_or_default(), if e.is_composing() { " (composing)" } else { "" }));
            if e.is_composing() {
                return;
            }
            match kind.as_str() {
                "insertText" | "insertReplacementText" => {
                    for c in e.data().unwrap_or_default().chars() {
                        app.key(Key::Char(c));
                    }
                }
                "insertLineBreak" | "insertParagraph" => app.key(Key::Enter),
                "deleteContentBackward" => app.key(Key::Backspace),
                _ => return,
            }
            e.prevent_default();
            reset_sink(&sink);
        }
    });

    listen(sink, "compositionupdate", {
        let app = app.clone();
        move |e: CompositionEvent| app.borrow_mut().preview = e.data().unwrap_or_default()
    });
    listen(sink, "compositionend", {
        let app = app.clone();
        let sink = sink.clone();
        move |e: CompositionEvent| {
            let mut app = app.borrow_mut();
            let data = e.data().unwrap_or_default();
            app.log(format!("compositionend {data:?}"));
            app.preview.clear();
            for c in data.chars() {
                app.key(Key::Char(c));
            }
            reset_sink(&sink);
        }
    });

    listen(sink, "paste", {
        let app = app.clone();
        move |e: Event| {
            let e: web_sys::ClipboardEvent = e.unchecked_into();
            let text = e.clipboard_data().and_then(|d| d.get_data("text").ok()).unwrap_or_default();
            e.prevent_default();
            let mut app = app.borrow_mut();
            app.log(format!("paste {} chars", text.chars().count()));
            app.input.push_str(&text);
        }
    });

    listen(sink, "keydown", {
        let app = app.clone();
        move |e: KeyboardEvent| {
            let mut app = app.borrow_mut();
            app.log(format!("keydown {:?} code={:?} kc={}", e.key(), e.code(), e.key_code()));
            if e.is_composing() || e.key_code() == 229 {
                return;
            }
            let key = e.key();
            if e.ctrl_key() && key.chars().count() == 1 {
                app.ctrl_char(key.chars().next().unwrap());
                e.prevent_default();
                return;
            }
            let special = match key.as_str() {
                "Enter" => Key::Enter,
                "Backspace" => Key::Backspace,
                "Tab" => Key::Tab,
                "Escape" => Key::Esc,
                "ArrowUp" => Key::Up,
                "ArrowDown" => Key::Down,
                "ArrowLeft" => Key::Left,
                "ArrowRight" => Key::Right,
                _ => return,
            };
            app.key(special);
            e.prevent_default();
        }
    });
}

fn wire_keys(document: &web_sys::Document, sink: &HtmlTextAreaElement, app: &Shared<App>) {
    let buttons = document.query_selector_all("#keys button").unwrap();
    for i in 0..buttons.length() {
        let button: Element = buttons.item(i).unwrap().dyn_into().unwrap();
        // Stop the button from taking focus, which would close the keyboard.
        for kind in ["pointerdown", "mousedown"] {
            listen(&button, kind, |e: Event| e.prevent_default());
        }
        listen(&button, "click", {
            let app = app.clone();
            let sink = sink.clone();
            let button = button.clone();
            move |_: Event| {
                let mut app = app.borrow_mut();
                let name = button.get_attribute("data-k").unwrap_or_default();
                match name.as_str() {
                    "ctrl" => app.ctrl = !app.ctrl,
                    "prefix" => app.prefix = !app.prefix,
                    "esc" => app.key(Key::Esc),
                    "tab" => app.key(Key::Tab),
                    "left" => app.key(Key::Left),
                    "up" => app.key(Key::Up),
                    "down" => app.key(Key::Down),
                    "right" => app.key(Key::Right),
                    "ctrlc" => app.ctrl_char('c'),
                    "kbd" => {
                        if sink_focused(&sink) { sink.blur().unwrap() } else { sink.focus().unwrap() }
                    }
                    _ => {}
                }
                app.log(format!("key bar {name} (keyboard {})", if sink_focused(&sink) { "kept" } else { "closed" }));

            }
        });
    }
}

fn sync_toggles(app: &App) {
    let document = web_sys::window().unwrap().document().unwrap();
    for (name, on) in [("ctrl", app.ctrl), ("prefix", app.prefix)] {
        if let Some(el) = document.query_selector(&format!("#keys [data-k={name}]")).unwrap() {
            el.set_attribute("aria-pressed", if on { "true" } else { "false" }).unwrap();
        }
    }
}

/// Taps pick a tab or open the keyboard; drags scroll the pane.
fn wire_pointer(term: &Element, sink: &HtmlTextAreaElement, app: &Shared<App>) {
    #[derive(Default)]
    struct Drag { start: (f64, f64), last_y: f64, carry: f64, moved: bool, down: bool }
    let drag: Shared<Drag> = Rc::new(RefCell::new(Drag::default()));

    // Pressing the grid would blur the textarea and close the keyboard.
    listen(term, "mousedown", |e: Event| e.prevent_default());
    listen(term, "pointerdown", {
        let drag = drag.clone();
        move |e: PointerEvent| {
            e.prevent_default();
            let (x, y) = (e.client_x() as f64, e.client_y() as f64);
            *drag.borrow_mut() = Drag { start: (x, y), last_y: y, down: true, ..Default::default() };
        }
    });
    listen(term, "pointermove", {
        let drag = drag.clone();
        let app = app.clone();
        move |e: PointerEvent| {
            let mut d = drag.borrow_mut();
            if !d.down {
                return;
            }
            let (x, y) = (e.client_x() as f64, e.client_y() as f64);
            if (x - d.start.0).hypot(y - d.start.1) > 8.0 {
                d.moved = true;
            }
            let mut app = app.borrow_mut();
            d.carry += y - d.last_y;
            d.last_y = y;
            let lines = (d.carry / app.cell.1).trunc();
            if lines != 0.0 {
                d.carry -= lines * app.cell.1;
                app.scroll_by(lines as isize);
            }
        }
    });
    listen(term, "pointerup", {
        let drag = drag.clone();
        let app = app.clone();
        let sink = sink.clone();
        let term = term.clone();
        move |e: PointerEvent| {
            let mut d = drag.borrow_mut();
            d.down = false;
            if d.moved {
                return;
            }
            let rect = term.get_bounding_client_rect();
            let mut app = app.borrow_mut();
            let col = ((e.client_x() as f64 - rect.left()) / app.cell.0) as u16;
            let row = ((e.client_y() as f64 - rect.top()) / app.cell.1) as u16;
            let hit = app.hits.iter().find(|(r, _)| r.contains(Position::new(col, row))).map(|(_, i)| *i);
            if let Some(index) = hit {
                app.select(index);
            }
            sink.focus().unwrap();
            app.log(format!("tap {} at {col},{row} -> {} (keyboard {})", e.pointer_type(), hit.map_or("pane".into(), |i| format!("tab {}", i + 1)), if sink_focused(&sink) { "up" } else { "not up" }));
        }
    });
    listen(term, "pointercancel", move |_: Event| drag.borrow_mut().down = false);

    listen(term, "wheel", {
        let app = app.clone();
        move |e: WheelEvent| app.borrow_mut().scroll_by(if e.delta_y() > 0.0 { -3 } else { 3 })
    });
}

fn sink_focused(sink: &HtmlTextAreaElement) -> bool {
    let active = web_sys::window().unwrap().document().unwrap().active_element();
    active.is_some_and(|el| el == ***sink)
}

fn reset_sink(sink: &HtmlTextAreaElement) {
    sink.set_value(SENTINEL);
    let end = SENTINEL.len() as u32;
    let _ = sink.set_selection_range(end, end);
}

fn listen<E, F>(target: &EventTarget, kind: &str, handler: F)
where
    E: JsCast + 'static,
    F: FnMut(E) + 'static,
{
    let mut handler = handler;
    let closure = Closure::<dyn FnMut(Event)>::new(move |e: Event| handler(e.unchecked_into()));
    target.add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref()).unwrap();
    closure.forget();
}
