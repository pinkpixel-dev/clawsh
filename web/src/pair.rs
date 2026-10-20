//! Whether this browser is paired, and the pairing form when it isn't.
//!
//! The device token lives in an `HttpOnly` cookie this code can't read, so
//! the only way to know is to ask the server (`/api/me`).

use std::{cell::RefCell, rc::Rc};

use serde::Deserialize;
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{HtmlElement, Response};

#[derive(Deserialize)]
pub struct Me {
    #[allow(dead_code)]
    pub name: String,
    pub watch_only: bool,
}

pub enum Status {
    Paired(Me),
    NotPaired,
    /// No answer at all, usually because phone access is off.
    Unreachable,
}

type Done = Box<dyn FnOnce(Status)>;

/// Asks the server, then calls `done` once with the answer.
pub fn check(done: impl FnOnce(Status) + 'static) {
    // The answer can come from either the success or the error callback,
    // but only one of them runs it.
    let done: Rc<RefCell<Option<Done>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let finish = {
        let done = done.clone();
        move |status: Status| {
            if let Some(done) = done.borrow_mut().take() {
                done(status);
            }
        }
    };
    let window = web_sys::window().unwrap();
    let on_response = Closure::<dyn FnMut(JsValue)>::once({
        let finish = finish.clone();
        move |value: JsValue| {
            let response: Response = value.unchecked_into();
            if response.status() == 401 {
                return finish(Status::NotPaired);
            }
            let text = match response.text() {
                Ok(text) if response.ok() => text,
                _ => return finish(Status::Unreachable),
            };
            let on_text = Closure::<dyn FnMut(JsValue)>::once(move |body: JsValue| {
                let me = body.as_string().and_then(|s| serde_json::from_str::<Me>(&s).ok());
                finish(me.map_or(Status::Unreachable, Status::Paired));
            });
            let _ = text.then(&on_text);
            on_text.forget();
        }
    });
    let on_error = Closure::<dyn FnMut(JsValue)>::once(move |_: JsValue| finish(Status::Unreachable));
    let _ = window.fetch_with_str("/api/me").then2(&on_response, &on_error);
    on_response.forget();
    on_error.forget();
}

/// Shows the pairing form instead of the terminal.
pub fn show_form() {
    let document = web_sys::window().unwrap().document().unwrap();
    let failed = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|q| q.contains("pair=failed"));
    if failed && let Some(msg) = document.get_element_by_id("pair-msg") {
        msg.set_text_content(Some("That code didn't work. Press n in the popup at your desk for a new one."));
        let _ = msg.set_attribute("class", "error");
    }
    if let Some(form) = document.get_element_by_id("pair") {
        let _ = form.remove_attribute("hidden");
    }
    if let Some(input) = document.get_element_by_id("code").and_then(|e| e.dyn_into::<HtmlElement>().ok()) {
        let _ = input.focus();
    }
}

/// Shows the terminal, marked watch-only when this device is.
pub fn show_app(watch_only: bool) {
    let document = web_sys::window().unwrap().document().unwrap();
    if let Some(app) = document.get_element_by_id("app") {
        let _ = app.remove_attribute("hidden");
        if watch_only {
            let _ = app.set_attribute("data-watch", "true");
        }
    }
}
