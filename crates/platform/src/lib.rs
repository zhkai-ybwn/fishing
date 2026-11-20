//! Windows camera, file picker, and desktop window access for fishing.

#![cfg(windows)]

mod camera;
mod desktop;
mod dialog;
mod notification;
mod tray;

pub use camera::{Camera, CameraInfo, Frame, cameras};
pub use desktop::{WindowActivation, WindowInfo, WindowTarget, activate, windows};
pub use dialog::choose_portrait;
pub use notification::notify;
pub use tray::{Tray, TrayAction};
