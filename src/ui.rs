use eframe::egui::{
    self, Align2, Color32, Context, FontId, Frame, Margin, Rect, Response, Sense, Stroke,
    StrokeKind, TextStyle, Theme, Ui, WidgetInfo, WidgetType, vec2,
};

struct Palette {
    canvas: Color32,
    surface: Color32,
    control: Color32,
    hover: Color32,
    outline: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    on_accent: Color32,
}

const LIGHT: Palette = Palette {
    canvas: Color32::from_rgb(243, 245, 248),
    surface: Color32::from_rgb(253, 254, 255),
    control: Color32::from_rgb(234, 239, 244),
    hover: Color32::from_rgb(224, 234, 241),
    outline: Color32::from_rgb(216, 225, 234),
    text: Color32::from_rgb(25, 43, 58),
    muted: Color32::from_rgb(76, 96, 115),
    accent: Color32::from_rgb(25, 92, 110),
    on_accent: Color32::from_rgb(247, 252, 253),
};

const DARK: Palette = Palette {
    canvas: Color32::from_rgb(16, 23, 32),
    surface: Color32::from_rgb(25, 34, 46),
    control: Color32::from_rgb(38, 53, 68),
    hover: Color32::from_rgb(48, 67, 84),
    outline: Color32::from_rgb(58, 74, 91),
    text: Color32::from_rgb(230, 239, 246),
    muted: Color32::from_rgb(162, 183, 202),
    accent: Color32::from_rgb(115, 210, 205),
    on_accent: Color32::from_rgb(13, 42, 48),
};

pub const PREVIEW_SURFACE: Color32 = Color32::from_rgb(12, 18, 26);
pub const PREVIEW_TEXT: Color32 = Color32::from_rgb(180, 198, 213);

pub fn configure_theme(ctx: &Context) {
    for (theme, colors) in [(Theme::Light, LIGHT), (Theme::Dark, DARK)] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = vec2(8.0, 8.0);
            style.spacing.button_padding = vec2(11.0, 6.0);
            style.spacing.interact_size.y = 30.0;
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(14.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(13.0));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(12.0));

            let visuals = &mut style.visuals;
            visuals.panel_fill = colors.canvas;
            visuals.window_fill = colors.surface;
            visuals.extreme_bg_color = colors.surface;
            visuals.window_stroke = Stroke::new(1.0, colors.outline);
            visuals.weak_text_color = Some(colors.muted);
            visuals.selection.bg_fill = colors.accent;
            visuals.selection.stroke = Stroke::new(1.0, colors.on_accent);

            let widgets = &mut visuals.widgets;
            widgets.noninteractive.fg_stroke.color = colors.text;
            widgets.noninteractive.bg_stroke.color = colors.outline;
            for (widget, fill) in [
                (&mut widgets.inactive, colors.control),
                (&mut widgets.hovered, colors.hover),
                (&mut widgets.active, colors.hover),
                (&mut widgets.open, colors.hover),
            ] {
                widget.weak_bg_fill = fill;
                widget.bg_fill = fill;
                widget.bg_stroke = Stroke::new(1.0, colors.outline);
                widget.fg_stroke.color = colors.text;
                widget.corner_radius = 7.into();
                widget.expansion = 0.0;
            }
            widgets.active.bg_stroke.color = colors.accent;
        });
    }
}

pub fn section(ui: &mut Ui, title: &str, content: impl FnOnce(&mut Ui)) {
    ui.label(egui::RichText::new(title).size(15.0).strong());
    Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            content(ui);
        });
}

pub fn switch(ui: &mut Ui, value: &mut bool, label: &str) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *value, label));

    let visuals = ui.visuals();
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 7.0, visuals.widgets.hovered.weak_bg_fill);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            7.0,
            Stroke::new(1.0, visuals.selection.bg_fill),
            StrokeKind::Inside,
        );
    }
    painter.text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );

    let track = Rect::from_center_size(rect.right_center() - vec2(24.0, 0.0), vec2(36.0, 20.0));
    let track_fill = if *value {
        visuals.selection.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    painter.rect(
        track,
        10.0,
        track_fill,
        visuals.window_stroke,
        StrokeKind::Inside,
    );
    let offset = 16.0 * ui.ctx().animate_bool(response.id, *value) - 8.0;
    let knob = if *value {
        visuals.selection.stroke.color
    } else {
        visuals.weak_text_color()
    };
    painter.circle_filled(track.center() + vec2(offset, 0.0), 6.5, knob);
    response
}
