use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use eframe::egui::{self, RichText, Ui};
use fishing_update::Update;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub enum Updater {
    #[default]
    Idle,
    Checking(Receiver<Result<Option<Update>, String>>),
    Current,
    Available(Update),
    Downloading(Receiver<Result<(Update, PathBuf), String>>),
    Ready(Update, PathBuf),
    Launching(Receiver<Result<(), String>>),
    Failed(String),
}

impl Updater {
    /// Return true after the user has launched the update installer.
    pub fn poll(&mut self) -> bool {
        let next = match self {
            Self::Checking(result) => receive(result).map(|result| match result {
                Ok(Some(update)) => Self::Available(update),
                Ok(None) => Self::Current,
                Err(error) => Self::Failed(error),
            }),
            Self::Downloading(result) => receive(result).map(|result| match result {
                Ok((update, installer)) => Self::Ready(update, installer),
                Err(error) => Self::Failed(error),
            }),
            Self::Launching(result) => match receive(result) {
                Some(Ok(())) => return true,
                Some(Err(error)) => Some(Self::Failed(error)),
                None => None,
            },
            _ => None,
        };
        if let Some(next) = next {
            *self = next;
        }
        false
    }

    pub fn pending(&self) -> bool {
        matches!(
            self,
            Self::Checking(_) | Self::Downloading(_) | Self::Launching(_)
        )
    }

    pub fn show(&mut self, ui: &mut Ui, directory: Option<&Path>) {
        match self {
            Self::Checking(_) => progress(ui, "正在检查更新…"),
            Self::Downloading(_) => progress(ui, "正在下载安装包并校验…"),
            Self::Launching(_) => progress(ui, "正在打开更新安装程序…"),
            Self::Available(update) => {
                let clicked = ui
                    .horizontal(|ui| {
                        ui.label(format!("发现 v{}", update.version));
                        ui.add_enabled(directory.is_some(), egui::Button::new("下载更新"))
                            .clicked()
                    })
                    .inner;
                if !update.notes.trim().is_empty() {
                    ui.collapsing("查看更新内容", |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(110.0)
                            .show(ui, |ui| {
                                ui.label(&update.notes);
                            });
                    });
                }
                if clicked
                    && let Some(directory) = directory
                    && let Self::Available(update) = std::mem::take(self)
                {
                    let directory = directory.to_owned();
                    *self = match start(move || {
                        let installer = fishing_update::download(&update, &directory)?;
                        Ok((update, installer))
                    }) {
                        Ok(receiver) => Self::Downloading(receiver),
                        Err(error) => Self::Failed(error),
                    };
                }
            }
            Self::Ready(update, _) => {
                let clicked = ui
                    .horizontal(|ui| {
                        ui.label(format!("v{} 已下载", update.version));
                        ui.button("退出并安装").clicked()
                    })
                    .inner;
                if clicked && let Self::Ready(update, installer) = std::mem::take(self) {
                    *self = match start(move || {
                        let _verified = fishing_update::verified_installer(&update, &installer)?;
                        Command::new(installer)
                            .spawn()
                            .map(|_| ())
                            .map_err(|error| format!("无法启动更新安装程序。{error}"))
                    }) {
                        Ok(receiver) => Self::Launching(receiver),
                        Err(error) => Self::Failed(error),
                    };
                }
            }
            Self::Idle | Self::Current | Self::Failed(_) => {
                let clicked = ui
                    .horizontal(|ui| {
                        ui.weak(format!("当前 v{CURRENT_VERSION}"));
                        ui.add_enabled(directory.is_some(), egui::Button::new("检查更新"))
                            .clicked()
                    })
                    .inner;
                match self {
                    Self::Current => {
                        ui.weak("已是最新版本。");
                    }
                    Self::Failed(error) => {
                        ui.label(RichText::new(error.as_str()).color(ui.visuals().warn_fg_color));
                    }
                    _ => {}
                }
                if clicked {
                    *self = match start(|| fishing_update::check(CURRENT_VERSION)) {
                        Ok(receiver) => Self::Checking(receiver),
                        Err(error) => Self::Failed(error),
                    };
                }
            }
        }
    }
}

fn progress(ui: &mut Ui, label: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(label);
    });
}

fn start<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Receiver<Result<T, String>>, String> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("fishing-update".into())
        .spawn(move || {
            let _ = sender.send(task());
        })
        .map_err(|error| format!("无法启动更新任务。{error}"))?;
    Ok(receiver)
}

fn receive<T>(receiver: &Receiver<Result<T, String>>) -> Option<Result<T, String>> {
    match receiver.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("更新任务中断，请重试。".into())),
    }
}
