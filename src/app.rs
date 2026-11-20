use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use boss_sensor_platform::{self as platform, CameraInfo, WindowInfo, WindowTarget};
use eframe::egui::{self, Align, ColorImage, Context, Layout, RichText, TextureHandle, Ui, Vec2};

use crate::engine::{Captured, Engine, Job, Policy, Snapshot};
use crate::settings::{Person, Settings, Storage};

const STALE_FRAME: Duration = Duration::from_secs(2);

struct Enrollment {
    name: String,
    started: Instant,
}

pub struct BossSensor {
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
}

impl BossSensor {
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
        let enrolling = self.enrollment.is_some();
        let policy = if self.running && !enrolling {
            Policy {
                second_face: self.settings.second_face,
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
                notify: self.settings.notify,
                target: self
                    .settings
                    .target
                    .clone()
                    .filter(|_| self.settings.switch_window),
            }
        } else {
            Policy::default()
        };
        let identify = enrolling || !policy.templates.is_empty();
        let analyze = identify || policy.second_face;
        let job = ((self.running && self.settings.preview) || analyze).then(|| Job {
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
            self.texture = None;
        }
    }

    fn observe(&mut self, snapshot: &Snapshot) {
        if let Some(event) = &snapshot.event
            && self.last_event != Some(event.at)
        {
            self.last_event = Some(event.at);
            let switch_detail = match &event.switched {
                Some(Ok(())) => " · 已切到目标窗口".into(),
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

        let Some(enrollment) = &self.enrollment else {
            return;
        };
        if enrollment.started.elapsed() > Duration::from_secs(10) {
            self.enrollment = None;
            self.enrollment_label =
                Some("录入超时。请让一张正面人脸出现在画面中，再试一次。".into());
            return;
        }
        if let Some(reading) = &snapshot.reading
            && reading.at > enrollment.started
            && reading.at.elapsed() < STALE_FRAME
            && reading.observation.face_count == 1
            && let [embedding] = reading.observation.embeddings.as_slice()
        {
            self.settings
                .people
                .push(Person::new(enrollment.name.clone(), embedding.clone()));
            self.enrollment_label = Some(format!("已录入 {}", enrollment.name));
            self.person_name.clear();
            self.enrollment = None;
        }
    }

    fn camera_section(&mut self, ui: &mut Ui) {
        ui.heading("摄像头与预览");
        ui.horizontal(|ui| {
            let label = self.settings.camera_id.as_ref().map_or("默认摄像头", |id| {
                self.cameras
                    .iter()
                    .find(|camera| camera.id == *id)
                    .map_or("所选摄像头不可用", |camera| camera.name.as_str())
            });
            egui::ComboBox::from_id_salt("camera-select")
                .width(300.0)
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.settings.camera_id, None, "默认摄像头");
                    for camera in &self.cameras {
                        ui.selectable_value(
                            &mut self.settings.camera_id,
                            Some(camera.id.clone()),
                            &camera.name,
                        );
                    }
                });
            if ui.button("刷新").clicked() {
                self.refresh_cameras();
            }
        });
        if let Some(error) = &self.camera_list_error {
            ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
        } else if self.cameras.is_empty() {
            ui.weak("未发现摄像头。连接摄像头后刷新列表。");
        }
        ui.checkbox(&mut self.settings.preview, "显示置顶预览");
    }

    fn rule_section(&mut self, ui: &mut Ui) {
        ui.heading("触发条件");
        ui.checkbox(&mut self.settings.second_face, "画面中出现第二张脸");
        if self.settings.second_face {
            ui.weak("保持你的脸在摄像头画面中。");
        }
        ui.checkbox(&mut self.settings.recognize, "识别指定人员");
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
            if self.enrollment.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("等待一张清晰的人脸…");
                    if ui.button("取消").clicked() {
                        self.enrollment = None;
                    }
                });
            } else {
                let name = self.person_name.trim().to_owned();
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.person_name)
                            .hint_text("人员名称")
                            .char_limit(48)
                            .desired_width(230.0),
                    );
                    if ui
                        .add_enabled(
                            !name.is_empty() && self.engine.is_some(),
                            egui::Button::new("录入"),
                        )
                        .clicked()
                    {
                        if self
                            .settings
                            .people
                            .iter()
                            .any(|person| person.name == name)
                        {
                            self.enrollment_label = Some("该名称已存在。".into());
                        } else {
                            self.enrollment = Some(Enrollment {
                                name,
                                started: Instant::now(),
                            });
                            self.enrollment_label = None;
                        }
                    }
                });
                if let Some(label) = &self.enrollment_label {
                    ui.weak(label);
                } else {
                    ui.weak("录入时让画面中只有一人。仅在本机保存识别特征。");
                }
            }
        });
    }

    fn action_section(&mut self, ui: &mut Ui) {
        ui.heading("触发动作");
        ui.checkbox(&mut self.settings.notify, "显示桌面提示");
        if ui
            .checkbox(&mut self.settings.switch_window, "切到目标应用")
            .changed()
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
                egui::ComboBox::from_id_salt("target-app")
                    .width(320.0)
                    .selected_text(label)
                    .show_ui(ui, |ui| {
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
                            ui.selectable_value(
                                &mut self.settings.target,
                                Some(target),
                                format!("    {}", window.title),
                            );
                        }
                    });
                if ui.button("刷新").clicked() {
                    self.windows = platform::windows();
                }
            });
            ui.weak("同一应用有多个窗口时，可以选择具体窗口。");
        });
    }

    fn preview(&mut self, ctx: &Context, image: Option<&Arc<Captured>>) {
        if !self.settings.preview || (!self.running && self.enrollment.is_none()) {
            self.texture = None;
            return;
        }
        if let Some(image) = image.filter(|image| image.at.elapsed() < STALE_FRAME) {
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
            .with_title("BossSensor 预览")
            .with_inner_size([320.0, 204.0])
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
                        .frame(egui::Frame::new().fill(egui::Color32::BLACK))
                        .show(ctx, |ui| {
                            ui.centered_and_justified(|ui| {
                                if let Some(texture) = &self.texture {
                                    let area = ui.available_size();
                                    let original = texture.size_vec2();
                                    let scale = (area.x / original.x).min(area.y / original.y);
                                    ui.image((texture.id(), original * scale));
                                } else {
                                    ui.label(
                                        RichText::new("等待摄像头画面")
                                            .color(egui::Color32::LIGHT_GRAY),
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

impl eframe::App for BossSensor {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let initial = self.settings.clone();
        let snapshot = self
            .engine
            .as_ref()
            .map_or_else(Snapshot::default, Engine::snapshot);
        self.observe(&snapshot);

        egui::TopBottomPanel::top("app-header")
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(16))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("BossSensor").size(23.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.running { "暂停" } else { "启动" };
                        let button = if self.running {
                            egui::Button::new(label)
                        } else {
                            egui::Button::new(label).fill(ui.visuals().selection.bg_fill)
                        };
                        if ui
                            .add_enabled(
                                self.engine.is_some(),
                                button.min_size(Vec2::new(72.0, 30.0)),
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
                ui.weak(status);
            });

        egui::TopBottomPanel::bottom("app-status").show(ctx, |ui| {
            if let Some(error) = &self.save_error {
                ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
            }
            if let Some(event) = &self.event_label {
                ui.label(event);
            }
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(16))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.camera_section(ui);
                    section_break(ui);
                    self.rule_section(ui);
                    section_break(ui);
                    self.action_section(ui);
                });
            });
        self.preview(ctx, snapshot.image.as_ref());
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
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }
}

fn configure_ui(ctx: &Context) {
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 9.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
    let mut fonts = egui::FontDefinitions::default();
    let name = "noto-sans-sc".to_owned();
    let chinese = egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansSC.ttf"));
    fonts.font_data.insert(name.clone(), Arc::new(chinese));
    for family in fonts.families.values_mut() {
        family.push(name.clone());
    }
    ctx.set_fonts(fonts);
}

fn section_break(ui: &mut Ui) {
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);
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
