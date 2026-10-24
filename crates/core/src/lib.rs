//! PinShot core: everything that can be written and tested without a window system.
//!
//! * [`geom`] / [`color`]: small value types
//! * [`annotate`] / [`history`]: the annotation model (shapes, hit testing, handles, undo/redo)
//! * [`text`] / [`render`]: text measurement and software rendering of annotations
//! * [`compose`]: stitching monitors into one image (scale-factor safe)
//! * [`config`] / [`hotkey`] / [`output`]: settings, shortcut strings, saving images

pub mod annotate;
pub mod color;
pub mod compose;
pub mod config;
pub mod geom;
pub mod history;
pub mod hotkey;
pub mod output;
pub mod render;
pub mod text;

pub use color::Rgba;
pub use geom::{Point, Rect};
