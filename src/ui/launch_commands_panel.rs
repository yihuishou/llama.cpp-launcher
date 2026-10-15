use crate::engine::rpc::RpcManager;
use crate::engine::server::ServerManager;
use crate::i18n;
use crate::ui::widgets;
use fluent_egui::winui;

pub fn ui(
    ui: &mut egui::Ui,
    server: &ServerManager,
    rpc: &RpcManager,
    lang: &i18n::Language,
    accent: egui::Color32,
) {
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionLaunchCommands, lang),
        accent,
        |ui| {
            ui.label(i18n::t(i18n::Key::LabelServerCommand, lang));
            if let Some(ref cmd) = server.launch_command() {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.monospace(cmd);
                });
                if ui
                    .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnCopyToClipboard, lang)))
                    .clicked()
                {
                    ui.ctx().copy_text(cmd.to_string());
                }
            } else {
                ui.colored_label(
                    ui.visuals().weak_text_color(),
                    i18n::t(i18n::Key::HintNoCommand, lang),
                );
            }
            ui.separator();

            ui.label(i18n::t(i18n::Key::LabelRpcCommand, lang));
            if let Some(ref cmd) = rpc.launch_command() {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.monospace(cmd);
                });
                if ui
                    .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnCopyToClipboard, lang)))
                    .clicked()
                {
                    ui.ctx().copy_text(cmd.to_string());
                }
            } else {
                ui.colored_label(
                    ui.visuals().weak_text_color(),
                    i18n::t(i18n::Key::HintNoCommand, lang),
                );
            }
        },
    );
}
