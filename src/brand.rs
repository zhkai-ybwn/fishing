use std::sync::{Arc, OnceLock};

use eframe::egui::{ColorImage, Context, IconData, TextureHandle, TextureOptions};

pub fn window_icon() -> Arc<IconData> {
    static ICON: OnceLock<Arc<IconData>> = OnceLock::new();
    Arc::clone(ICON.get_or_init(|| {
        let png = include_bytes!("../assets/BossSensor.png");
        Arc::new(eframe::icon_data::from_png_bytes(png).expect("embedded BossSensor PNG"))
    }))
}

pub fn texture(ctx: &Context) -> TextureHandle {
    let icon = window_icon();
    ctx.load_texture(
        "boss-sensor-mark",
        ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba),
        TextureOptions::LINEAR,
    )
}
