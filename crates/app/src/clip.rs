//! Clipboard access (arboard). One long-lived instance: on X11 the owner must stay alive to serve the data.

use arboard::{Clipboard, ImageData};
use pinshot_core::output::to_rgba8;
use std::borrow::Cow;
use tiny_skia::Pixmap;

pub struct Clip {
    inner: Option<Clipboard>,
}

impl Clip {
    pub fn new() -> Self {
        let inner = Clipboard::new()
            .map_err(|e| log::warn!("clipboard unavailable: {e}"))
            .ok();
        Clip { inner }
    }

    fn cb(&mut self) -> Result<&mut Clipboard, String> {
        if self.inner.is_none() {
            self.inner = Some(Clipboard::new().map_err(|e| e.to_string())?);
        }
        self.inner
            .as_mut()
            .ok_or_else(|| "no clipboard".to_string())
    }

    pub fn set_image(&mut self, pm: &Pixmap) -> Result<(), String> {
        let img = ImageData {
            width: pm.width() as usize,
            height: pm.height() as usize,
            bytes: Cow::Owned(to_rgba8(pm)),
        };
        self.cb()?.set_image(img).map_err(|e| e.to_string())
    }

    pub fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.cb()?
            .set_text(text.to_string())
            .map_err(|e| e.to_string())
    }

    pub fn image(&mut self) -> Option<Pixmap> {
        let img = self.cb().ok()?.get_image().ok()?;
        pinshot_core::output::from_rgba8(img.width as u32, img.height as u32, &img.bytes)
    }

    pub fn text(&mut self) -> Option<String> {
        self.cb()
            .ok()?
            .get_text()
            .ok()
            .filter(|t| !t.trim().is_empty())
    }
}
