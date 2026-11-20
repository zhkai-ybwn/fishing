use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use boss_sensor_platform::{
    self as platform, CameraInfo, WindowActivation, WindowInfo, WindowTarget,
};
use eframe::egui::{self, Align, ColorImage, Context, Layout, RichText, TextureHandle, Ui, Vec2};

use crate::engine::{Engine, Job, Policy, Snapshot};
use crate::settings::{DEFAULT_NOTIFICATION_TEXT, Person, Settings, Storage};
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
    running: bool,
    job: Option<Job>,
    cameras: Vec<CameraInfo>,
    camera_list_error: Option<String>,
    windows: Vec<WindowInfo>,
    last_event: Option<Instant>,
    event_label: Option<String>,
    person_name: String,
    enrollment: Option<Enrollment>,
    enrollment_label: Option<String>,
    texture: Option<TextureHandle>,
    texture_sequence: u64,
    preview_position: Option<egui::Pos2>,
    logo: TextureHandle,
}

impl Fishing {
    pub fn new(ctx: &Context) -> Self {
        configure_ui(ctx);
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
        let mut app = Self {
            save_enabled: storage.is_some() && save_error.is_none(),
            settings,
            storage,
            engine,
            startup_error,
            save_error,
            running: false,
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
            preview_position: None,
            logo: brand::texture(ctx),
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
        let policy = if self.running && self.enrollment.is_none() {
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
                notification_text: self.settings.notification_message().map(str::to_owned),
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
        let preview = self.settings.preview && self.running && !importing;
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
        ui.horizontal(|ui| {
            let label = self.settings.camera_id.as_ref().map_or("默认摄像头", |id| {
                self.cameras
                    .iter()
                    .find(|camera| camera.id == *id)
                    .map_or("所选摄像头不可用", |camera| camera.name.as_str())
            });
            let picker_width = (ui.available_width() - 68.0).clamp(120.0, 380.0);
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
            if ui.button("刷新").clicked() {
                self.refresh_cameras();
            }
        });
        if let Some(error) = &self.camera_list_error {
            ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
        } else if self.cameras.is_empty() {
            ui.weak("未发现摄像头。连接摄像头后刷新列表。");
        }
        ui.separator();
        design::switch(ui, &mut self.settings.preview, "置顶实时预览");
    }

    fn rule_section(&mut self, ui: &mut Ui) {
        design::switch(ui, &mut self.settings.face_count_enabled, "按人脸数量触发");
        if self.settings.face_count_enabled {
            ui.indent("minimum-faces", |ui| {
                ui.horizontal(|ui| {
                    let label = ui.label("检测到至少");
                    ui.add(
                        egui::DragValue::new(&mut self.settings.minimum_faces)
                            .range(1..=u32::MAX)
                            .update_while_editing(false),
                    )
                    .labelled_by(label.id);
                    ui.label("张脸时触发");
                });
                ui.weak("按摄像头画面中的总人脸数计算。");
            });
        }
        ui.separator();
        design::switch(ui, &mut self.settings.recognize, "识别指定人员");
        if !self.settings.recognize {
            self.enrollment = None;
            return;
        }

        ui.indent("enrollment", |ui| {
            let mut remove = None;
            for (index, person) in self.settings.people.iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(&person.name);
                        if person.embedding().is_none() {
                            ui.weak("需要重新录入");
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("移除").clicked() {
                                remove = Some(index);
                            }
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
        if self.settings.notify {
            ui.indent("notification-text", |ui| {
                let label = ui.label("提示文字");
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.notification_text)
                        .hint_text(DEFAULT_NOTIFICATION_TEXT)
                        .char_limit(80)
                        .desired_width(ui.available_width().min(300.0)),
                )
                .labelled_by(label.id);
            });
        }
        ui.separator();
        if design::switch(ui, &mut self.settings.switch_window, "切到目标应用").changed()
            && self.settings.switch_window
        {
            self.windows = platform::windows();
        }
        if !self.settings.switch_window {
            return;
        }
        ui.indent("switch-target", |ui| {
            ui.horizontal(|ui| {
                let label = self
                    .settings
                    .target
                    .as_ref()
                    .map_or_else(|| "选择目标应用".into(), target_label);
                let picker_width = (ui.available_width() - 68.0).clamp(120.0, 380.0);
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
                if ui.button("刷新").clicked() {
                    self.windows = platform::windows();
                }
            });
            ui.weak("同一应用有多个窗口时，可以选择具体窗口。");
        });
    }

    fn preview(&mut self, ctx: &Context, snapshot: &Snapshot) {
        if !self.settings.preview
            || (!self.running && self.enrollment.is_none())
            || matches!(self.enrollment, Some(Enrollment::Photo { .. }))
        {
            self.texture = None;
            return;
        }
        if let Some(image) = snapshot
            .image
            .as_ref()
            .filter(|image| image.at.elapsed() < STALE_FRAME)
        {
            if self.texture.is_none() || self.texture_sequence != image.sequence {
                let frame = &image.frame;
                let pixels =
                    ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.rgb);
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
            }
        } else {
            self.texture = None;
        }

        let mut builder = egui::ViewportBuilder::default()
            .with_title("fishing 预览")
            .with_icon(brand::window_icon())
            .with_inner_size([320.0, 192.0])
            .with_min_inner_size([175.0, 120.0])
            .with_active(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_maximize_button(false)
            .with_minimize_button(false);
        if let Some(position) = self.preview_position {
            builder = builder.with_position(position);
        }
        let (closed, rect) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("camera-preview"),
            builder,
            |ctx, class| {
                let closed = ctx.input(|input| input.viewport().close_requested());
                let rect = ctx.input(|input| input.viewport().outer_rect);
                if class != egui::ViewportClass::Embedded {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().fill(design::PREVIEW_SURFACE))
                        .show(ctx, |ui| {
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
                        });
                }
                (closed, rect)
            },
        );
        if closed {
            self.preview_position = rect.map(|rect| rect.min);
            self.settings.preview = false;
        }
    }
}

impl eframe::App for Fishing {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let initial = self.settings.clone();
        let snapshot = self
            .engine
            .as_ref()
            .map_or_else(Snapshot::default, Engine::snapshot);
        self.observe(&snapshot);

        egui::TopBottomPanel::top("app-header")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(egui::Margin::symmetric(22, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.image((self.logo.id(), Vec2::splat(42.0)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(brand::NAME).size(25.0).strong());
                        ui.label(RichText::new("桌面摸鱼助手").small().weak());
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.running { "暂停" } else { "启动" };
                        let button = if self.running {
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
                            self.running = !self.running;
                            if !self.running {
                                self.enrollment = None;
                            }
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
                } else if !self.running {
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

        if self.save_error.is_some() || self.event_label.is_some() {
            egui::TopBottomPanel::bottom("app-status")
                .frame(
                    egui::Frame::side_top_panel(&ctx.style())
                        .inner_margin(egui::Margin::symmetric(22, 10)),
                )
                .show(ctx, |ui| {
                    if let Some(error) = &self.save_error {
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    design::section(ui, "摄像头", |ui| self.camera_section(ui));
                    ui.add_space(9.0);
                    design::section(ui, "触发条件", |ui| self.rule_section(ui));
                    ui.add_space(9.0);
                    design::section(ui, "触发动作", |ui| self.action_section(ui));
                });
            });
        self.preview(ctx, &snapshot);
        self.configure_engine();

        if initial != self.settings
            && self.save_enabled
            && let Some(storage) = &self.storage
        {
            self.save_error = storage
                .save(&self.settings)
                .err()
                .map(|error| format!("设置保存失败 · {error}"));
        }
        if self.job.is_some() || self.enrollment.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }
}

fn configure_ui(ctx: &Context) {
    design::configure_theme(ctx);
    let mut fonts = egui::FontDefinitions::default();
    let name = "noto-sans-sc".to_owned();
    let chinese = egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansSC.ttf"));
    fonts.font_data.insert(name.clone(), Arc::new(chinese));
    for family in fonts.families.values_mut() {
        family.push(name.clone());
    }
    ctx.set_fonts(fonts);
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
