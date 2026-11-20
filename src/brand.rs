use std::sync::{Arc, OnceLock};

use eframe::egui::{ColorImage, Context, IconData, TextureHandle, TextureOptions};

pub const NAME: &str = "fishing";

pub fn window_icon() -> Arc<IconData> {
    static ICON: OnceLock<Arc<IconData>> = OnceLock::new();
    Arc::clone(ICON.get_or_init(|| {
        let png = include_bytes!("../assets/fishing.png");
        Arc::new(eframe::icon_data::from_png_bytes(png).expect("embedded fishing PNG"))
    }))
}

pub fn texture(ctx: &Context) -> TextureHandle {
    let icon = window_icon();
    ctx.load_texture(
        "fishing-mark",
        ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba),
        TextureOptions::LINEAR,
    )
}
