use eframe::egui::{
    Align2, Button, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Theme, Ui, WidgetInfo,
    WidgetType, vec2,
};

struct Palette {
    canvas: Color32,
    surface: Color32,
    control: Color32,
    hover: Color32,
    pressed: Color32,
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
    hover: Color32::from_rgb(222, 233, 241),
    pressed: Color32::from_rgb(200, 221, 233),
    outline: Color32::from_rgb(204, 216, 226),
    text: Color32::from_rgb(19, 35, 49),
    muted: Color32::from_rgb(65, 82, 99),
    accent: Color32::from_rgb(25, 92, 110),
    on_accent: Color32::from_rgb(247, 252, 253),
};

const DARK: Palette = Palette {
    canvas: Color32::from_rgb(16, 23, 32),
    surface: Color32::from_rgb(25, 34, 46),
    control: Color32::from_rgb(38, 53, 68),
    hover: Color32::from_rgb(48, 67, 84),
    pressed: Color32::from_rgb(63, 87, 103),
    outline: Color32::from_rgb(72, 91, 111),
    text: Color32::from_rgb(234, 242, 248),
    muted: Color32::from_rgb(176, 195, 212),
    accent: Color32::from_rgb(115, 210, 205),
    on_accent: Color32::from_rgb(13, 42, 48),
};

pub const PREVIEW_SURFACE: Color32 = Color32::from_rgb(12, 18, 26);
pub const PREVIEW_TEXT: Color32 = Color32::from_rgb(180, 198, 213);

const REGULAR_FONT: &str = "fishing-text";
const BOLD_FONT: &str = "fishing-text-bold";

pub fn configure_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let regular = FontData::from_static(include_bytes!("../assets/fonts/NotoSansSC-Regular.otf"));
    let bold = FontData::from_static(include_bytes!("../assets/fonts/NotoSansSC-Bold.otf"));
    fonts.font_data.insert(REGULAR_FONT.into(), regular.into());
    fonts.font_data.insert(BOLD_FONT.into(), bold.into());
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, REGULAR_FONT.into());
    let mut emphasis = proportional.clone();
    emphasis.insert(0, BOLD_FONT.into());
    fonts
        .families
        .insert(FontFamily::Name(BOLD_FONT.into()), emphasis);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push(REGULAR_FONT.into());
    ctx.set_fonts(fonts);
}

pub fn bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FONT.into()))
}

pub fn configure_theme(ctx: &Context) {
    for (theme, colors) in [(Theme::Light, LIGHT), (Theme::Dark, DARK)] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = vec2(8.0, 7.0);
            style.spacing.button_padding = vec2(12.0, 6.0);
            style.spacing.interact_size.y = 32.0;
            style
                .text_styles
                .insert(TextStyle::Heading, bold_font(18.0));
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(15.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(14.0));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(13.0));

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
                (&mut widgets.active, colors.pressed),
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
    section_title(ui, title);
    surface(ui, content);
}

pub fn section_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(bold_font(16.0)));
}

pub fn surface(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            content(ui);
        });
}

pub fn picker_with_refresh(ui: &mut Ui, picker: impl FnOnce(&mut Ui, f32)) -> bool {
    const REFRESH_WIDTH: f32 = 60.0;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - REFRESH_WIDTH - ui.spacing().item_spacing.x).max(0.0);
        ui.scope(|ui| {
            // ComboBox::width is a minimum. Truncation uses the surrounding UI's width.
            ui.set_width(width);
            picker(ui, width);
        });
        ui.add_sized(
            [REFRESH_WIDTH, ui.spacing().interact_size.y],
            Button::new("刷新"),
        )
        .clicked()
    })
    .inner
}

pub fn switch(ui: &mut Ui, value: &mut bool, label: &str) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *value, label));

    let visuals = ui.visuals();
    let painter = ui.painter();
    let pressed = response.is_pointer_button_down_on();
    if pressed {
        painter.rect_filled(rect, 7.0, visuals.widgets.active.weak_bg_fill);
    } else if response.hovered() {
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
        rect.left_center() + vec2(8.0, 0.0),
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
    let knob_radius = if pressed { 6.0 } else { 6.5 };
    painter.circle_filled(track.center() + vec2(offset, 0.0), knob_radius, knob);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{CentralPanel, ComboBox, RawInput};

    #[test]
    fn picker_with_a_long_title_keeps_the_refresh_button_inside_its_row() {
        let ctx = Context::default();
        configure_theme(&ctx);
        let title = "Code · light-theme.less - ami-workspace - ".repeat(20);
        let _ = ctx.run(RawInput::default(), |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                ui.set_width(320.0);
                let right = ui.max_rect().right();
                picker_with_refresh(ui, |ui, width| {
                    ComboBox::from_id_salt("long-title")
                        .width(width)
                        .truncate()
                        .selected_text(&title)
                        .show_ui(ui, |_| {});
                });
                assert!(ui.min_rect().right() <= right + 0.5);
            });
        });
    }
}
