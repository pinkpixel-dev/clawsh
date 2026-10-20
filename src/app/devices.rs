//! The paired phones menu: seeing which phones can connect, and removing
//! one. A removed phone that's connected right then is cut off too.

use anyhow::Result;
use chrono::Utc;

use super::{App, Mode};
use crate::menu::Menu;
use crate::web::Device;

impl App {
    pub(super) fn open_devices_menu(&mut self) {
        let list = match self.devices.lock() {
            Ok(devices) => devices.list().iter().map(|d| (d.id.clone(), label(d))).collect(),
            Err(_) => Vec::new(),
        };
        self.mode = Mode::Menu(Menu::devices(self.menu_anchor(), list));
    }

    pub(super) fn confirm_remove_device(&mut self, id: &str) {
        let name = self.devices.lock().ok().and_then(|d| d.list().iter().find(|d| d.id == id).map(|d| d.name.clone()));
        if let Some(name) = name {
            self.mode = Mode::Menu(Menu::confirm_device(self.menu_anchor(), id.to_owned(), &name));
        }
    }

    pub(super) fn remove_device(&mut self, id: &str) -> Result<()> {
        let removed = match self.devices.lock() {
            Ok(mut devices) => {
                let name = devices.list().iter().find(|d| d.id == id).map(|d| d.name.clone());
                devices.remove(id)?;
                name
            }
            Err(_) => None,
        };
        self.disconnect_device(id)?;
        if let Some(name) = removed {
            self.notice = Some(format!("Removed {name}"));
        }
        Ok(())
    }
}

/// `iPhone · Oct 20 · seen 5m ago`, plus whether it's watch only.
fn label(d: &Device) -> String {
    let seen = match d.last_seen {
        None => "never connected".to_owned(),
        Some(t) => {
            let mins = (Utc::now() - t).num_minutes().max(0);
            match mins {
                0 => "seen just now".to_owned(),
                m if m < 60 => format!("seen {m}m ago"),
                m if m < 60 * 24 => format!("seen {}h ago", m / 60),
                m => format!("seen {}d ago", m / (60 * 24)),
            }
        }
    };
    let watch = if d.watch_only { " · watch only" } else { "" };
    format!("{} · {seen}{watch}", d.name)
}
