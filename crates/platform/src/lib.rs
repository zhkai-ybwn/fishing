//! Windows camera and desktop window access for BossSensor.

#![cfg(windows)]

mod camera;
mod desktop;
mod notification;

pub use camera::{Camera, CameraInfo, Frame, cameras};
pub use desktop::{WindowInfo, WindowTarget, activate, windows};
pub use notification::notify;
