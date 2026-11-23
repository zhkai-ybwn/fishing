use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use boss_sensor_platform::{
    self as platform, CameraInfo, Tray, TrayAction, WindowActivation, WindowInfo, WindowTarget,
};
use eframe::egui::{self, Align, ColorImage, Context, Layout, RichText, TextureHandle, Ui, Vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::engine::{Engine, Job, Policy, Reminder, Snapshot};
use crate::settings::{
    DEFAULT_NOTIFICATION_TEXT, DEFAULT_PREVIEW_BRIGHTNESS, PREVIEW_BRIGHTNESS_RANGE, Person,
    Settings, Storage, render_reminder,
};
use crate::update_ui::Updater;
use crate::window_close::{CloseBehavior, CloseDecision, ClosePrompt};
use crate::{brand, ui as design};

const STALE_FRAME: Duration = Duration::from_secs(2);

enum Enrollment {
    Camera {
        name: String,
        started: Instant,
    },
    Photo {
        name: String,
        result: Receiver<Result<Vec<f32>, String>>,
    },
}

pub struct Fishing {
    settings: Settings,
    storage: Option<Storage>,
    save_enabled: bool,
    engine: Option<Engine>,
    startup_error: Option<String>,
    save_error: Option<String>,
    running_since: Option<Instant>,
    tray: Option<Tray>,
    tray_error: Option<String>,
    in_tray: bool,
    allow_exit: bool,
    close_prompt: Option<ClosePrompt>,
    updater: Updater,
    job: Option<Job>,
    cameras: Vec<CameraInfo>,
    camera_list_error: Option<String>,
    windows: Vec<WindowInfo>,
    last_event: Option<Instant>,
    event_label: Option<String>,
    person_name: String,
    minimum_faces_input: String,
    enrollment: Option<Enrollment>,
    enrollment_label: Option<String>,
    texture: Option<TextureHandle>,
    texture_sequence: u64,
    texture_brightness: u16,
    preview_position: Option<egui::Pos2>,
    preview_open: bool,
    logo: TextureHandle,
    show_preferences: bool,
}

impl Fishing {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        let ctx = &context.egui_ctx;
        design::configure_fonts(ctx);
        design::configure_theme(ctx);
        let (settings, storage, save_error, mut startup_error) = match Storage::open() {
            Ok(storage) => match storage.load() {
                Ok(settings) => (settings, Some(storage), None, None),
                Err(error) => (
                    Settings::default(),
                    Some(storage),
                    Some(format!("设置读取失败，当前更改仅用于本次运行。{error}")),
                    None,
                ),
            },
            Err(error) => (Settings::default(), None, None, Some(error)),
        };
        let engine = if startup_error.is_none() {
            match Engine::start() {
                Ok(engine) => Some(engine),
                Err(error) => {
                    startup_error = Some(format!("无法启动摄像头与识别线程。{error}"));
                    None
                }
            }
        } else {
            None
        };
        let (tray, tray_error) = if engine.is_some() {
            match create_tray(context) {
                Ok(tray) => (Some(tray), None),
                Err(error) => (None, Some(error)),
            }
        } else {
            (None, None)
        };
        let mut app = Self {
            save_enabled: storage.is_some() && save_error.is_none(),
            minimum_faces_input: settings.minimum_faces.to_string(),
            settings,
            storage,
            engine,
            startup_error,
            save_error,
            running_since: None,
            tray,
            tray_error,
            in_tray: false,
            allow_exit: false,
            close_prompt: None,
            updater: Updater::default(),
            job: None,
            cameras: Vec::new(),
            camera_list_error: None,
            windows: Vec::new(),
            last_event: None,
            event_label: None,
            person_name: String::new(),
            enrollment: None,
            enrollment_label: None,
            texture: None,
            texture_sequence: 0,
            texture_brightness: DEFAULT_PREVIEW_BRIGHTNESS,
            preview_position: None,
            preview_open: false,
            logo: brand::texture(ctx),
            show_preferences: false,
        };
        app.refresh_cameras();
        if app.settings.switch_window {
            app.windows = platform::windows();
        }
        app
    }

    fn refresh_cameras(&mut self) {
        match platform::cameras() {
            Ok(cameras) => {
                self.cameras = cameras;
                self.camera_list_error = None;
            }
            Err(error) => self.camera_list_error = Some(error),
        }
    }

    fn configure_engine(&mut self) {
        let recording = matches!(self.enrollment, Some(Enrollment::Camera { .. }));
        let importing = matches!(self.enrollment, Some(Enrollment::Photo { .. }));
        let policy = if let Some(started) = self.running_since.filter(|_| self.enrollment.is_none())
        {
            Policy {
                minimum_faces: self
                    .settings
                    .face_count_enabled
                    .then_some(self.settings.minimum_faces),
                templates: if self.settings.recognize {
                    self.settings
                        .people
                        .iter()
                        .filter_map(Person::embedding)
                        .map(<[f32]>::to_vec)
                        .collect()
                } else {
                    Vec::new()
                },
                reminder: self
                    .settings
                    .notification_message()
                    .map(|message| Reminder {
                        template: message.into(),
                        started,
                    }),
                target: self
                    .settings
                    .target
                    .clone()
                    .filter(|_| self.settings.switch_window),
            }
        } else {
            Policy::default()
        };
        let identify = recording || !policy.templates.is_empty();
        let analyze = identify || policy.minimum_faces.is_some();
        let preview =
            self.settings.preview && self.running_since.is_some() && !importing && !self.in_tray;
        let job = (preview || analyze).then(|| Job {
            camera: self.settings.camera_id.clone().unwrap_or_default(),
            analyze,
            identify,
            policy,
        });
        if self.job != job {
            if let Some(engine) = &self.engine {
                engine.configure(job.clone());
            }
            self.job = job;
        }
    }

    fn toggle_running(&mut self) {
        self.running_since = if self.running_since.is_some() {
            self.enrollment = None;
            None
        } else {
            Some(Instant::now())
        };
        if let Some(tray) = &self.tray {
            tray.set_running(self.running_since.is_some());
        }
    }

    fn minimize_to_tray(&mut self, ctx: &Context) -> bool {
        if let Some(tray) = &self.tray {
            match tray.set_taskbar_visible(false) {
                Ok(()) => {
                    self.in_tray = true;
                    self.tray_error = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    return true;
                }
                Err(error) => self.tray_error = Some(error),
            }
        }
        false
    }

    fn begin_exit(&mut self) {
        self.allow_exit = true;
        self.close_prompt = None;
        if let Some(engine) = &self.engine {
            engine.configure(None);
        }
    }

    fn request_exit(&mut self, ctx: &Context) {
        self.begin_exit();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_close_request(&mut self, ctx: &Context) {
        if self.allow_exit || !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        if self.settings.close_behavior == CloseBehavior::Exit {
            self.begin_exit();
            return;
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if self.settings.close_behavior != CloseBehavior::ToTray || !self.minimize_to_tray(ctx) {
            self.close_prompt.get_or_insert_with(ClosePrompt::default);
        }
    }

    fn draw_close_prompt(&mut self, ctx: &Context) -> bool {
        let decision = self.close_prompt.as_mut().and_then(|prompt| {
            prompt.show(
                ctx,
                self.tray.is_some() && self.tray_error.is_none(),
                self.save_enabled,
            )
        });
        match decision {
            Some(CloseDecision::Exit { remember }) => {
                if remember {
                    self.settings.close_behavior = CloseBehavior::Exit;
                }
                self.close_prompt = None;
                true
            }
            Some(CloseDecision::ToTray { remember }) => {
                if self.minimize_to_tray(ctx) {
                    if remember {
                        self.settings.close_behavior = CloseBehavior::ToTray;
                    }
                    self.close_prompt = None;
                }
                false
            }
            Some(CloseDecision::Cancel) => {
                self.close_prompt = None;
                false
            }
            None => false,
        }
    }

    fn restore_from_tray(&mut self) {
        if self.in_tray
            && let Some(tray) = &self.tray
        {
            self.tray_error = tray.set_taskbar_visible(true).err();
        }
        self.in_tray = false;
    }

    fn handle_tray_actions(&mut self, ctx: &Context) {
        while let Some(action) = self.tray.as_ref().and_then(Tray::next_action) {
            match action {
                TrayAction::Open => self.restore_from_tray(),
                TrayAction::Toggle => self.toggle_running(),
                TrayAction::Quit => {
                    self.request_exit(ctx);
                    return;
                }
            }
        }
        if self.in_tray
            && ctx.input(|input| {
                input.viewport().minimized == Some(false) && input.viewport().focused == Some(true)
            })
        {
            self.restore_from_tray();
        }
    }

    fn observe(&mut self, snapshot: &Snapshot) {
        if let Some(event) = &snapshot.event
            && self.last_event != Some(event.at)
        {
            self.last_event = Some(event.at);
            let switch_detail = match &event.switched {
                Some(Ok(WindowActivation::Foreground)) => " · 已切到目标窗口".into(),
                Some(Ok(WindowActivation::Raised)) => {
                    " · 目标窗口已浮到前面，键盘焦点留在原窗口".into()
                }
                Some(Err(error)) => format!(" · 切屏失败，{error}"),
                None => String::new(),
            };
            let notification_detail = match &event.notification {
                Some(Err(error)) => format!(" · 提示失败，{error}"),
                _ => String::new(),
            };
            self.event_label = Some(format!(
                "{}{switch_detail}{notification_detail}",
                event.matched.label()
            ));
        }

        let (name, result) = match &self.enrollment {
            Some(Enrollment::Camera { name, started }) => {
                if started.elapsed() > Duration::from_secs(10) {
                    (
                        name,
                        Err("录入超时。请让一张正面人脸出现在画面中，再试一次。".into()),
                    )
                } else if let Some(reading) = &snapshot.reading
                    && reading.at > *started
                    && reading.at.elapsed() < STALE_FRAME
                    && reading.observation.face_count == 1
                    && let [embedding] = reading.observation.embeddings.as_slice()
                {
                    (name, Ok(embedding.clone()))
                } else {
                    return;
                }
            }
            Some(Enrollment::Photo { name, result }) => match result.try_recv() {
                Ok(result) => (name, result),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => (name, Err("照片识别中断，请重试。".into())),
            },
            None => return,
        };
        self.enrollment_label = match result {
            Ok(embedding) => {
                self.settings
                    .people
                    .push(Person::new(name.clone(), embedding));
                self.person_name.clear();
                Some(format!("已录入 {name}"))
            }
            Err(error) => Some(error),
        };
        self.enrollment = None;
    }

    fn begin_enrollment(&mut self, name: String, from_photo: bool) {
        if self
            .settings
            .people
            .iter()
            .any(|person| person.name == name)
        {
            self.enrollment_label = Some("该名称已存在。".into());
            return;
        }
        if !from_photo {
            self.enrollment = Some(Enrollment::Camera {
                name,
                started: Instant::now(),
            });
            self.enrollment_label = None;
            return;
        }

        // Suspend auto-switching while the native file picker has focus.
        if let Some(engine) = &self.engine {
            engine.configure(None);
        }
        self.job = None;
        self.texture = None;
        match platform::choose_portrait() {
            Ok(Some(path)) => {
                let (sender, result) = mpsc::channel();
                match thread::Builder::new()
                    .name("photo-enrollment".into())
                    .spawn(move || {
                        let _ = sender.send(boss_sensor_vision::enroll_photo(&path));
                    }) {
                    Ok(_) => {
                        self.enrollment = Some(Enrollment::Photo { name, result });
                        self.enrollment_label = None;
                    }
                    Err(error) => {
                        self.enrollment_label = Some(format!("无法启动照片识别。{error}"))
                    }
                }
            }
            Ok(None) => {}
            Err(error) => self.enrollment_label = Some(error),
        }
    }

    fn camera_section(&mut self, ui: &mut Ui) {
        if design::picker_with_refresh(ui, |ui, picker_width| {
            let label = self.settings.camera_id.as_ref().map_or("默认摄像头", |id| {
                self.cameras
                    .iter()
                    .find(|camera| camera.id == *id)
                    .map_or("所选摄像头不可用", |camera| camera.name.as_str())
            });
            egui::ComboBox::from_id_salt("camera-select")
                .width(picker_width)
                .truncate()
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.set_max_width(picker_width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui.selectable_value(&mut self.settings.camera_id, None, "默认摄像头");
                    for camera in &self.cameras {
                        ui.selectable_value(
                            &mut self.settings.camera_id,
                            Some(camera.id.clone()),
                            &camera.name,
                        );
                    }
                })
                .response
                .on_hover_text(label);
        }) {
            self.refresh_cameras();
        }
        if let Some(error) = &self.camera_list_error {
            ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
        } else if self.cameras.is_empty() {
            ui.weak("未发现摄像头。连接摄像头后刷新列表。");
        }
        ui.separator();
        design::switch(ui, &mut self.settings.preview_frameless, "无边框预览");
        ui.horizontal(|ui| {
            let label = ui.label("预览亮度");
            ui.add(
                egui::Slider::new(
                    &mut self.settings.preview_brightness,
                    PREVIEW_BRIGHTNESS_RANGE,
                )
                .suffix("%"),
            )
            .labelled_by(label.id);
            if ui
                .button("还原")
                .on_hover_text("还原为摄像头的原始亮度")
                .clicked()
            {
                self.settings.preview_brightness = DEFAULT_PREVIEW_BRIGHTNESS;
            }
        });
    }

    fn rule_section(&mut self, ui: &mut Ui) {
        design::switch(ui, &mut self.settings.preview, "置顶实时预览");
        ui.separator();
        design::switch(ui, &mut self.settings.face_count_enabled, "按人脸数量触发");
        if self.settings.face_count_enabled {
            ui.indent("minimum-faces", |ui| {
                let valid = ui
                    .horizontal(|ui| {
                        let label = ui.label("检测到至少");
                        let input = ui
                            .add(
                                egui::TextEdit::singleline(&mut self.minimum_faces_input)
                                    .desired_width(68.0)
                                    .horizontal_align(Align::Center)
                                    .char_limit(10),
                            )
                            .labelled_by(label.id);
                        ui.label("张脸时触发");
                        let value = self
                            .minimum_faces_input
                            .trim()
                            .parse::<u32>()
                            .ok()
                            .filter(|value| *value > 0);
                        if input.lost_focus()
                            && let Some(value) = value
                        {
                            self.settings.minimum_faces = value;
                            self.minimum_faces_input = value.to_string();
                        }
                        value.is_some()
                            || (input.has_focus() && self.minimum_faces_input.is_empty())
                    })
                    .inner;
                if valid {
                    ui.weak("按摄像头画面中的总人脸数计算。");
                } else {
                    ui.label(
                        RichText::new(format!(
                            "请输入正整数。正在使用阈值 {}。",
                            self.settings.minimum_faces
                        ))
                        .color(ui.visuals().warn_fg_color),
                    );
                }
            });
        }
        ui.separator();
        if design::switch(ui, &mut self.settings.recognize, "识别指定人员").changed()
            && !self.settings.recognize
        {
            self.enrollment = None;
        }
        if self.settings.recognize && self.settings.people.is_empty() {
            ui.indent("people-empty", |ui| {
                if ui.small_button("录入人员…").clicked() {
                    self.show_preferences = true;
                }
            });
        }
    }

    fn people_section(&mut self, ui: &mut Ui) {
        ui.scope(|ui| {
            let mut remove = None;
            for (index, person) in self.settings.people.iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("移除").clicked() {
                            remove = Some(index);
                        }
                        if person.embedding().is_none() {
                            ui.weak("需要重新录入");
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(&person.name).truncate())
                                .on_hover_text(&person.name);
                        });
                    });
                });
            }
            if let Some(index) = remove {
                self.settings.people.remove(index);
            }
            if let Some(enrollment) = &self.enrollment {
                let label = match enrollment {
                    Enrollment::Camera { .. } => "等待一张清晰的人脸…",
                    Enrollment::Photo { .. } => "正在识别照片…",
                };
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(label);
                    if ui.button("取消").clicked() {
                        self.enrollment = None;
                    }
                });
            } else {
                let input_width = ui.available_width().min(300.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.person_name)
                        .hint_text("人员名称")
                        .char_limit(48)
                        .desired_width(input_width),
                );
                let name = self.person_name.trim().to_owned();
                let can_enroll = !name.is_empty() && self.engine.is_some();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_enroll, egui::Button::new("从照片录入…"))
                        .clicked()
                    {
                        self.begin_enrollment(name.clone(), true);
                    }
                    if ui
                        .add_enabled(can_enroll, egui::Button::new("摄像头录入"))
                        .clicked()
                    {
                        self.begin_enrollment(name, false);
                    }
                });
                if let Some(label) = &self.enrollment_label {
                    ui.weak(label);
                } else {
                    ui.weak("选一张只有目标人物的清晰正脸照，照片仅用于提取特征。");
                }
            }
        });
    }

    fn action_section(&mut self, ui: &mut Ui) {
        design::switch(ui, &mut self.settings.notify, "显示桌面提示");
        ui.separator();
        self.switch_target_section(ui);
    }

    fn notification_section(&mut self, ui: &mut Ui) {
        let label = ui.label("提示文字");
        let width = ui.available_width();
        ui.add(
            egui::TextEdit::singleline(&mut self.settings.notification_text)
                .hint_text(DEFAULT_NOTIFICATION_TEXT)
                .char_limit(80)
                .desired_width(width.min(440.0)),
        )
        .labelled_by(label.id);
        ui.weak("{时长} 会替换为自本次启动检测以来的时间。");
        ui.horizontal(|ui| {
            if ui.button("恢复默认").clicked() {
                self.settings.notification_text = DEFAULT_NOTIFICATION_TEXT.into();
            }
            if ui.button("预览提醒").clicked() {
                let template = self.settings.notification_template();
                let elapsed = self
                    .running_since
                    .map_or(Duration::ZERO, |started| started.elapsed());
                let message = render_reminder(template, elapsed);
                if let Err(error) = platform::notify(&message) {
                    self.event_label = Some(error);
                }
            }
        });
    }

    fn switch_target_section(&mut self, ui: &mut Ui) {
        if design::switch(ui, &mut self.settings.switch_window, "切到目标应用").changed()
            && self.settings.switch_window
        {
            self.windows = platform::windows();
        }
        if !self.settings.switch_window {
            return;
        }
        if design::picker_with_refresh(ui, |ui, picker_width| {
            let label = self
                .settings
                .target
                .as_ref()
                .map_or_else(|| "选择目标应用".into(), target_label);
            egui::ComboBox::from_id_salt("target-app")
                .width(picker_width)
                .truncate()
                .selected_text(&label)
                .show_ui(ui, |ui| {
                    ui.set_max_width(picker_width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui.selectable_value(&mut self.settings.target, None, "清除目标");
                    if self.windows.is_empty() {
                        ui.weak("打开应用窗口后，刷新此列表。");
                    }
                    let mut previous = None;
                    for window in &self.windows {
                        if previous != Some(window.path.as_str()) {
                            let app = WindowTarget {
                                title: None,
                                path: window.path.clone(),
                            };
                            ui.selectable_value(
                                &mut self.settings.target,
                                Some(app),
                                executable_name(&window.path),
                            );
                            previous = Some(window.path.as_str());
                        }
                        let target = WindowTarget {
                            title: Some(window.title.clone()),
                            path: window.path.clone(),
                        };
                        ui.indent((&window.path, &window.title), |ui| {
                            ui.selectable_value(
                                &mut self.settings.target,
                                Some(target),
                                &window.title,
                            )
                            .on_hover_text(&window.title);
                        });
                    }
                })
                .response
                .on_hover_text(label);
        }) {
            self.windows = platform::windows();
        }
    }

    fn preview(&mut self, ctx: &Context, snapshot: &Snapshot) {
        if !self.settings.preview
            || (self.running_since.is_none() && self.enrollment.is_none())
            || matches!(self.enrollment, Some(Enrollment::Photo { .. }))
            || self.in_tray
        {
            self.texture = None;
            self.preview_open = false;
            return;
        }
        if let Some(image) = snapshot
            .image
            .as_ref()
            .filter(|image| image.at.elapsed() < STALE_FRAME)
        {
            let brightness = self.settings.preview_brightness;
            if self.texture.is_none()
                || self.texture_sequence != image.sequence
                || self.texture_brightness != brightness
            {
                let frame = &image.frame;
                let mut pixels =
                    ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.rgb);
                if brightness != DEFAULT_PREVIEW_BRIGHTNESS {
                    let adjust =
                        |channel: u8| ((u16::from(channel) * brightness + 50) / 100).min(255) as u8;
                    for pixel in &mut pixels.pixels {
                        *pixel = egui::Color32::from_rgb(
                            adjust(pixel.r()),
                            adjust(pixel.g()),
                            adjust(pixel.b()),
                        );
                    }
                }
                if let Some(texture) = &mut self.texture {
                    texture.set(pixels, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ctx.load_texture(
                        "camera-preview",
                        pixels,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                self.texture_sequence = image.sequence;
                self.texture_brightness = brightness;
            }
        } else {
            self.texture = None;
        }

        let frameless = self.settings.preview_frameless;
        let mut builder = egui::ViewportBuilder::default()
            .with_title("fishing 预览")
            .with_icon(brand::window_icon())
            .with_decorations(!frameless)
            .with_inner_size([320.0, 180.0])
            .with_min_inner_size([175.0, 120.0])
            .with_active(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_maximize_button(false)
            .with_minimize_button(false);
        if !self.preview_open
            && let Some(position) = self.preview_position
        {
            builder = builder.with_position(position);
        }
        self.preview_open = true;
        let (closed, rect) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("camera-preview"),
            builder,
            |ctx, class| {
                let mut closed = ctx.input(|input| input.viewport().close_requested());
                let rect = ctx.input(|input| input.viewport().outer_rect);
                if class != egui::ViewportClass::Embedded {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().fill(design::PREVIEW_SURFACE))
                        .show(ctx, |ui| {
                            let area = ui.available_rect_before_wrap();
                            ui.centered_and_justified(|ui| {
                                if let Some(texture) = &self.texture {
                                    let area = ui.available_size();
                                    let original = texture.size_vec2();
                                    let scale = (area.x / original.x).min(area.y / original.y);
                                    ui.image((texture.id(), original * scale));
                                } else {
                                    ui.label(
                                        RichText::new(if snapshot.camera_error.is_some() {
                                            "摄像头暂不可用"
                                        } else {
                                            "正在连接摄像头…"
                                        })
                                        .color(design::PREVIEW_TEXT),
                                    );
                                }
                            });
                            if frameless {
                                closed |= preview_window_controls(ui, area);
                            }
                        });
                }
                (closed, rect)
            },
        );
        if let Some(rect) = rect {
            self.preview_position = Some(rect.min);
        }
        if closed {
            self.preview_open = false;
            self.settings.preview = false;
            ctx.request_repaint();
        }
    }
}

fn preview_window_controls(ui: &mut Ui, area: egui::Rect) -> bool {
    let mouse_pressed =
        ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
    let surface = ui.interact(
        area,
        ui.id().with("preview-surface"),
        egui::Sense::click_and_drag(),
    );
    let resize_area =
        egui::Rect::from_min_max(area.right_bottom() - Vec2::splat(16.0), area.right_bottom());
    let resize = ui.interact(
        resize_area,
        ui.id().with("preview-resize"),
        egui::Sense::drag(),
    );
    if resize.hovered() || resize.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        ui.painter()
            .rect_filled(resize_area, 0.0, design::PREVIEW_SURFACE);
        for inset in [5.0, 9.0, 13.0] {
            let corner = resize_area.right_bottom() - Vec2::splat(3.0);
            ui.painter().line_segment(
                [
                    corner - egui::vec2(inset, 0.0),
                    corner - egui::vec2(0.0, inset),
                ],
                egui::Stroke::new(1.0, design::PREVIEW_TEXT),
            );
        }
    }
    if mouse_pressed {
        if resize.is_pointer_button_down_on() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::BeginResize(
                    egui::ResizeDirection::SouthEast,
                ));
        } else if surface.is_pointer_button_down_on() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
    let mut closed = false;
    surface.context_menu(|ui| {
        if ui.button("关闭预览").clicked() {
            closed = true;
            ui.close();
        }
    });
    closed
}

impl Fishing {
    fn application_settings(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let label = ui.label("关闭窗口");
            egui::ComboBox::from_id_salt("close-behavior")
                .selected_text(self.settings.close_behavior.label())
                .width(138.0)
                .show_ui(ui, |ui| {
                    for behavior in [
                        CloseBehavior::Exit,
                        CloseBehavior::ToTray,
                        CloseBehavior::Ask,
                    ] {
                        ui.selectable_value(
                            &mut self.settings.close_behavior,
                            behavior,
                            behavior.label(),
                        );
                    }
                })
                .response
                .labelled_by(label.id);
        });
        ui.separator();
        let directory = self.storage.as_ref().map(Storage::updates_directory);
        self.updater.show(ui, directory.as_deref());
    }

    fn draw_home(&mut self, ui: &mut Ui) {
        design::surface(ui, |ui| {
            if ui.available_width() >= 610.0 {
                ui.columns(2, |columns| {
                    design::section_title(&mut columns[0], "监看与触发");
                    self.rule_section(&mut columns[0]);
                    design::section_title(&mut columns[1], "触发动作");
                    self.action_section(&mut columns[1]);
                });
            } else {
                design::section_title(ui, "监看与触发");
                self.rule_section(ui);
                ui.add_space(12.0);
                design::section_title(ui, "触发动作");
                self.action_section(ui);
            }
        });
    }

    fn draw_preferences(&mut self, ui: &mut Ui) {
        if ui.available_width() >= 680.0 {
            ui.columns(2, |columns| {
                design::section(&mut columns[0], "人员录入", |ui| {
                    self.people_section(ui)
                });
                columns[0].add_space(10.0);
                design::section(&mut columns[0], "应用", |ui| {
                    self.application_settings(ui)
                });
                design::section(&mut columns[1], "摄像头与预览", |ui| {
                    self.camera_section(ui)
                });
                columns[1].add_space(10.0);
                design::section(&mut columns[1], "提示文字", |ui| {
                    self.notification_section(ui)
                });
            });
        } else {
            design::section(ui, "人员录入", |ui| self.people_section(ui));
            ui.add_space(10.0);
            design::section(ui, "摄像头与预览", |ui| self.camera_section(ui));
            ui.add_space(10.0);
            design::section(ui, "提示文字", |ui| self.notification_section(ui));
            ui.add_space(10.0);
            design::section(ui, "应用", |ui| self.application_settings(ui));
        }
    }

    fn draw_main_window(&mut self, ctx: &Context, snapshot: &Snapshot) {
        egui::TopBottomPanel::top("app-header")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.image((self.logo.id(), Vec2::splat(38.0)));
                    ui.label(RichText::new(brand::NAME).font(design::bold_font(25.0)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let running = self.running_since.is_some();
                        let label = if running { "暂停" } else { "启动" };
                        let button = if running {
                            egui::Button::new(label)
                        } else {
                            egui::Button::new(
                                RichText::new(label)
                                    .strong()
                                    .color(ui.visuals().selection.stroke.color),
                            )
                            .fill(ui.visuals().selection.bg_fill)
                        };
                        if ui
                            .add_enabled(
                                self.engine.is_some(),
                                button.min_size(Vec2::new(78.0, 34.0)),
                            )
                            .clicked()
                        {
                            self.toggle_running();
                        }
                        if ui
                            .add_enabled(self.tray.is_some(), egui::Button::new("收起到托盘"))
                            .on_hover_text("收起窗口，保持当前的检测状态")
                            .clicked()
                        {
                            self.minimize_to_tray(ctx);
                        }
                        let page = if self.show_preferences {
                            "返回首页"
                        } else {
                            "设置"
                        };
                        if ui.button(page).clicked() {
                            self.show_preferences = !self.show_preferences;
                        }
                    });
                });
                let status = if let Some(error) = &self.startup_error {
                    error.clone()
                } else if let Some(error) = &snapshot.camera_error {
                    format!("摄像头不可用 · {error}")
                } else if let Some(error) = &snapshot.vision_error {
                    format!("识别不可用 · {error}")
                } else if matches!(self.enrollment, Some(Enrollment::Photo { .. })) {
                    "正在录入照片…".into()
                } else if self.enrollment.is_some() {
                    "录入中 · 等待一张人脸".into()
                } else if self.running_since.is_none() {
                    "已暂停".into()
                } else if self.job.is_none() {
                    "等待开启预览或触发条件".into()
                } else if let Some(reading) = snapshot
                    .reading
                    .as_ref()
                    .filter(|reading| reading.at.elapsed() < STALE_FRAME)
                {
                    format!("识别中 · {} 张脸", reading.observation.face_count)
                } else if self.job.as_ref().is_some_and(|job| job.analyze) {
                    "正在启动识别…".into()
                } else {
                    "预览中".into()
                };
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let has_error = self.startup_error.is_some()
                        || snapshot.camera_error.is_some()
                        || snapshot.vision_error.is_some();
                    let color = if has_error {
                        ui.visuals().warn_fg_color
                    } else if self.job.is_some() {
                        ui.visuals().selection.bg_fill
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, color);
                    ui.label(RichText::new(status).small().weak());
                });
            });

        if self.save_error.is_some() || self.event_label.is_some() || self.tray_error.is_some() {
            egui::TopBottomPanel::bottom("app-status")
                .frame(
                    egui::Frame::side_top_panel(&ctx.style())
                        .inner_margin(egui::Margin::symmetric(22, 10)),
                )
                .show(ctx, |ui| {
                    if let Some(error) = &self.save_error {
                        ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
                    }
                    if let Some(error) = &self.tray_error {
                        ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
                    }
                    if let Some(event) = &self.event_label {
                        ui.label(RichText::new(format!("最近触发 · {event}")).small());
                    }
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(22, 18)),
            )
            .show(ctx, |ui| {
                let preferences = self.show_preferences;
                egui::ScrollArea::vertical()
                    .id_salt(("main-page", preferences))
                    .show(ui, |ui| {
                        if preferences {
                            self.draw_preferences(ui);
                        } else {
                            self.draw_home(ui);
                        }
                    });
            });
    }
}

impl eframe::App for Fishing {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let was_in_tray = self.in_tray;
        let initial = self.settings.clone();
        let snapshot = self
            .engine
            .as_ref()
            .map_or_else(Snapshot::default, Engine::snapshot);
        self.handle_tray_actions(ctx);
        self.handle_close_request(ctx);
        if !self.allow_exit {
            if self.updater.poll() {
                self.request_exit(ctx);
            } else {
                self.observe(&snapshot);
            }
        }
        // eframe presents the last update before applying close/minimize. Keep
        // the settings surface composed through that transition.
        if !self.in_tray || !was_in_tray {
            self.draw_main_window(ctx, &snapshot);
        }
        let close_confirmed = self.draw_close_prompt(ctx);
        self.preview(ctx, &snapshot);
        if !self.allow_exit {
            self.configure_engine();
        }

        if initial != self.settings
            && self.save_enabled
            && let Some(storage) = &self.storage
        {
            self.save_error = storage
                .save(&self.settings)
                .err()
                .map(|error| format!("设置保存失败 · {error}"));
        }
        if close_confirmed {
            if initial.close_behavior != self.settings.close_behavior && self.save_error.is_some() {
                self.settings.close_behavior = initial.close_behavior;
            } else {
                self.request_exit(ctx);
            }
        }
        if (!self.in_tray && self.job.is_some()) || self.enrollment.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        } else if self.updater.pending() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }
}

fn create_tray(context: &eframe::CreationContext<'_>) -> Result<Tray, String> {
    let handle = context
        .window_handle()
        .map_err(|error| format!("系统托盘不可用，无法取得主窗口。{error}"))?;
    let RawWindowHandle::Win32(window) = handle.as_raw() else {
        return Err("系统托盘不可用，找不到 Windows 主窗口。".into());
    };
    let ctx = context.egui_ctx.clone();
    Tray::new(window.hwnd.get(), move || {
        ctx.request_repaint_of(egui::ViewportId::ROOT)
    })
}

fn executable_name(path: &str) -> &str {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(path)
}

fn target_label(target: &WindowTarget) -> String {
    let app = executable_name(&target.path);
    target
        .title
        .as_ref()
        .map_or_else(|| app.into(), |title| format!("{app} · {title}"))
}
