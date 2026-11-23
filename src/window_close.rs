use eframe::egui::{
    Align, Button, Checkbox, Context, Frame, Id, Key, Layout, Margin, Modal, Modifiers,
};
use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    Exit,
    ToTray,
    #[default]
    Ask,
}

impl CloseBehavior {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exit => "直接退出",
            Self::ToTray => "收起到托盘",
            Self::Ask => "每次询问",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CloseDecision {
    Exit { remember: bool },
    ToTray { remember: bool },
    Cancel,
}

#[derive(Default)]
pub struct ClosePrompt {
    remember: bool,
}

impl ClosePrompt {
    pub fn show(
        &mut self,
        ctx: &Context,
        tray_available: bool,
        can_remember: bool,
    ) -> Option<CloseDecision> {
        let frame = Frame::popup(&ctx.style())
            .corner_radius(12)
            .inner_margin(Margin::same(18));
        let modal = Modal::new(Id::new("close-settings-window"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(340.0);
                ui.heading("关闭 fishing？");
                if tray_available {
                    ui.label("收起后，fishing 会在系统托盘继续运行。");
                } else {
                    ui.label("系统托盘不可用。可以退出或返回设置。");
                }
                ui.add_space(4.0);
                let remember = ui.add_enabled(
                    can_remember,
                    Checkbox::new(&mut self.remember, "记住这次操作"),
                );
                let remember_choice = can_remember && self.remember;
                ui.add_space(6.0);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let exit = ui.button("退出");
                    let to_tray = ui.add_enabled(tray_available, Button::new("收起到托盘"));
                    let cancel = ui.button("取消");

                    let decision = if exit.clicked() {
                        Some(CloseDecision::Exit {
                            remember: remember_choice,
                        })
                    } else if to_tray.clicked() {
                        Some(CloseDecision::ToTray {
                            remember: remember_choice,
                        })
                    } else if cancel.clicked() {
                        Some(CloseDecision::Cancel)
                    } else {
                        None
                    };
                    let control_focused = [exit, to_tray, cancel, remember]
                        .iter()
                        .any(|response| response.has_focus());
                    (decision, control_focused)
                })
                .inner
            });

        let decision = if let Some(decision) = modal.inner.0 {
            Some(decision)
        } else if modal.should_close() {
            Some(CloseDecision::Cancel)
        } else if modal.is_top_modal
            && !modal.any_popup_open
            && !modal.inner.1
            && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
        {
            Some(CloseDecision::Exit {
                remember: can_remember && self.remember,
            })
        } else {
            None
        };
        if decision.is_some() {
            self.remember = false;
        }
        decision
    }
}
