//! The WebSocket back to Claw$h. Frames come in and go straight onto the
//! app's screen; when the socket drops, it retries on a timer.

use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{MessageEvent, WebSocket};

use crate::app::{App, Link};
use crate::pair;
use crate::protocol::{FromPhone, ToPhone};

const RETRY_MS: i32 = 1500;

type Shared<T> = Rc<RefCell<T>>;

#[derive(Clone)]
pub struct Socket {
    ws: Shared<Option<WebSocket>>,
    app: Shared<App>,
    url: String,
}

impl Socket {
    /// Connects to `/ws` on the page's own host. The browser sends the
    /// device cookie along with it.
    pub fn connect(app: Shared<App>) -> Self {
        let location = web_sys::window().unwrap().location();
        let scheme = if location.protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
        let host = location.host().unwrap_or_default();
        let socket = Self { ws: Rc::new(RefCell::new(None)), app, url: format!("{scheme}://{host}/ws") };
        socket.open();
        socket
    }

    fn open(&self) {
        self.app.borrow_mut().link = Link::Connecting;
        let Ok(ws) = WebSocket::new(&self.url) else {
            self.retry();
            return;
        };

        let onopen = Closure::<dyn FnMut()>::new({
            let app = self.app.clone();
            move || {
                let mut app = app.borrow_mut();
                app.link = Link::Open;
                // Send the size again on the next frame.
                app.sent_size = None;
            }
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new({
            let app = self.app.clone();
            move |e: MessageEvent| {
                let Some(text) = e.data().as_string() else { return };
                if let Ok(msg) = serde_json::from_str::<ToPhone>(&text) {
                    app.borrow_mut().apply(msg);
                }
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        // An error is always followed by a close, so only close retries.
        let onclose = Closure::<dyn FnMut()>::new({
            let socket = self.clone();
            // A socket refused for not being paired looks like any other
            // close, so ask before retrying. A removed device gets the
            // pairing form again.
            move || {
                socket.app.borrow_mut().link = Link::Lost;
                let socket = socket.clone();
                pair::check(move |status| match status {
                    pair::Status::NotPaired => {
                        let _ = web_sys::window().unwrap().location().reload();
                    }
                    _ => socket.retry(),
                });
            }
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        *self.ws.borrow_mut() = Some(ws);
    }

    fn retry(&self) {
        self.app.borrow_mut().link = Link::Lost;
        self.ws.borrow_mut().take();
        let again = Closure::once_into_js({
            let socket = self.clone();
            move || socket.open()
        });
        let _ = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(again.unchecked_ref(), RETRY_MS);
    }

    pub fn send(&self, msg: &FromPhone) {
        if let (Some(ws), Ok(text)) = (self.ws.borrow().as_ref(), serde_json::to_string(msg)) {
            let _ = ws.send_with_str(&text);
        }
    }
}
