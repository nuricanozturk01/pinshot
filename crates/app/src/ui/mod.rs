//! A small software-rendered widget kit: everything is drawn into a pixmap, so it looks the same on every OS and
//! needs no GUI toolkit.

pub mod canvas;
pub mod icons;
pub mod menu;
pub mod theme;

pub use canvas::Canvas;
