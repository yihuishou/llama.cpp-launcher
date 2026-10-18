//! llama.cpp 编译面板 —— 编译环境检测 + 仓库/分支/Tag + 后端选择 + 克隆/编译入口
//!
//! 原为设置面板内的一张卡片，现独立为顶层导航项。

use crate::config::settings::AppSettings;
use crate::i18n;
use crate::ui::widgets;
use egui::RichText;
use fluent_egui::winui;

pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    lang: &i18n::Language,
    env_check: &crate::env_check::EnvCheckHandle,
    clone_handle: &crate::build::CloneHandle,
) {
    let accent = crate::theme::accent_color(&settings.accent_color);

    // ── llama.cpp 编译 ──
    widgets::card(ui, i18n::t(i18n::Key::SettingsBuild, lang), accent, |ui| {
        ui.add_space(8.0);

        // 编译环境检测（卡片第一项）
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::t(i18n::Key::EnvCheckTitle, lang));
            if ui
                .add_enabled(
                    !env_check.is_busy(),
                    widgets::rounded_button(i18n::t(i18n::Key::EnvCheckBtnRun, lang), Some(accent)),
                )
                .clicked()
            {
                env_check.run();
            }
        });

        match &env_check.snapshot().state {
            crate::env_check::EnvCheckState::Running => {
                ui.label(RichText::new(i18n::t(i18n::Key::EnvCheckStatusRunning, lang)).weak());
            }
            crate::env_check::EnvCheckState::Done(results) => {
                ui.label(RichText::new(i18n::t(i18n::Key::EnvCheckStatusDone, lang)).weak());
                let entries = crate::env_check::to_entries(results);
                render_env_entries(ui, &entries, lang);
            }
            crate::env_check::EnvCheckState::Idle => {
                if settings.env_check_result.is_empty() {
                    ui.label(RichText::new(i18n::t(i18n::Key::EnvCheckStatusIdle, lang)).weak());
                } else {
                    render_env_entries(ui, &settings.env_check_result, lang);
                }
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // Git 仓库
        ui.label(i18n::t(i18n::Key::BuildGitRepo, lang));
        ui.add(winui::FluentTextEdit::new(&mut settings.build_git_repo));

        // 分支
        ui.label(i18n::t(i18n::Key::BuildBranch, lang));
        ui.add(winui::FluentTextEdit::new(&mut settings.build_branch));

        // Tag
        ui.label(i18n::t(i18n::Key::BuildTag, lang));
        ui.add(winui::FluentTextEdit::new(&mut settings.build_tag));

        ui.add_space(4.0);
        let action_enabled = can_clone(env_check, settings) && !clone_handle.is_busy();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    action_enabled,
                    widgets::rounded_button(i18n::t(i18n::Key::BuildBtnClone, lang), None),
                )
                .clicked()
            {
                clone_handle.start_clone(
                    settings.build_git_repo.trim().to_string(),
                    settings.build_branch.trim().to_string(),
                    settings.build_tag.trim().to_string(),
                );
            }
            if ui
                .add_enabled(
                    action_enabled,
                    widgets::rounded_button(i18n::t(i18n::Key::BuildBtnUpdate, lang), None),
                )
                .clicked()
            {
                clone_handle.start_update(
                    settings.build_branch.trim().to_string(),
                    settings.build_tag.trim().to_string(),
                );
            }
        });
        render_clone_status(ui, clone_handle, lang);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // 后端编译开关（按工具包安装状态显示）
        ui.label(i18n::t(i18n::Key::BuildBackend, lang));
        ui.label(
            RichText::new(i18n::t(i18n::Key::BuildGgmlHint, lang))
                .small()
                .weak(),
        );
        render_ggml_options(ui, settings, env_check, lang);

        ui.add_space(8.0);

        // 操作按钮
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(widgets::rounded_button(
                    i18n::t(i18n::Key::BuildBtnBuild, lang),
                    None,
                ))
                .clicked()
            {
                // TODO: 编译逻辑
            }
        });
    });
}

/// 渲染克隆 / 更新状态：进行中（含进度条）/ 完成（含源码目录）/ 失败（含英文原因）/ 空闲（提示目标目录）。
fn render_clone_status(
    ui: &mut egui::Ui,
    clone_handle: &crate::build::CloneHandle,
    lang: &i18n::Language,
) {
    use crate::build::{CloneState, SourceOp};

    let palette = winui::palette(widgets::current_theme(ui));
    let status = clone_handle.snapshot();
    let is_update = status.op == Some(SourceOp::Update);
    match &status.state {
        CloneState::Running => {
            let key = if is_update {
                i18n::Key::BuildStatusUpdating
            } else {
                i18n::Key::BuildStatusCloning
            };
            ui.label(RichText::new(i18n::t(key, lang)).weak());
            if let Some(percent) = status.percent {
                ui.add(winui::FluentProgressBar::new(percent as f32 / 100.0).show_percentage(true));
            }
        }
        CloneState::Done(path) => {
            let key = if is_update {
                i18n::Key::BuildUpdateDone
            } else {
                i18n::Key::BuildCloneDone
            };
            ui.label(
                RichText::new(i18n::t(key, lang))
                    .small()
                    .color(palette.success),
            );
            ui.label(RichText::new(path.display().to_string()).small().weak());
        }
        CloneState::Failed(msg) => {
            let key = if is_update {
                i18n::Key::BuildUpdateFailed
            } else {
                i18n::Key::BuildCloneFailed
            };
            ui.label(
                RichText::new(i18n::t(key, lang))
                    .small()
                    .color(palette.danger),
            );
            ui.label(RichText::new(msg.as_str()).small().weak());
        }
        CloneState::Idle => {
            if let Some(dir) = crate::build::source_code_dir() {
                ui.label(RichText::new(dir.display().to_string()).small().weak());
            }
        }
    }
}

/// 检测结果条目里指定组件是否可用
fn entry_found(entries: &[crate::env_check::EnvCheckEntry], id: &str) -> bool {
    entries.iter().any(|e| e.id == id && e.found)
}

/// 指定组件是否可用：优先本次运行结果，否则回退到已保存结果。
fn component_available(
    env_check: &crate::env_check::EnvCheckHandle,
    settings: &AppSettings,
    id: &str,
) -> bool {
    match &env_check.snapshot().state {
        crate::env_check::EnvCheckState::Done(results) => {
            entry_found(&crate::env_check::to_entries(results), id)
        }
        _ => entry_found(&settings.env_check_result, id),
    }
}

/// 「克隆仓库」是否可点：git 可用 + 仓库地址、分支均非空。
fn can_clone(env_check: &crate::env_check::EnvCheckHandle, settings: &AppSettings) -> bool {
    component_available(env_check, settings, crate::env_check::ID_GIT)
        && !settings.build_git_repo.trim().is_empty()
        && !settings.build_branch.trim().is_empty()
}

/// 按工具包安装状态渲染 GGML_* 编译开关复选框；勾选结果写回 `settings.build_ggml_options`
fn render_ggml_options(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    env_check: &crate::env_check::EnvCheckHandle,
    lang: &i18n::Language,
) {
    let cuda_ok = component_available(env_check, settings, crate::env_check::ID_CUDA);
    let rocm_ok = component_available(env_check, settings, crate::env_check::ID_ROCM);

    for (group, available, title, missing) in [
        (
            crate::build::GgmlGroup::Cuda,
            cuda_ok,
            i18n::Key::BuildCuda,
            i18n::Key::BuildCudaMissing,
        ),
        (
            crate::build::GgmlGroup::Hip,
            rocm_ok,
            i18n::Key::BuildRocm,
            i18n::Key::BuildRocmMissing,
        ),
    ] {
        ui.add_space(4.0);
        ui.label(i18n::t(title, lang));
        if !available {
            ui.label(RichText::new(i18n::t(missing, lang)).small().weak());
            continue;
        }
        for &(name, desc) in crate::build::ggml_options(group) {
            let mut checked =
                crate::build::is_ggml_option_enabled(&settings.build_ggml_options, name);
            let label = format!("{}({})", name, i18n::t(desc, lang));
            if ui
                .add(winui::FluentCheckBox::new(&mut checked, label))
                .changed()
            {
                crate::build::set_ggml_option(&mut settings.build_ggml_options, name, checked);
            }
        }
    }
}

/// 渲染环境检测结果列表：逐行显示「组件名 + 必需/可选 + 已安装/未安装 + 版本」。
fn render_env_entries(
    ui: &mut egui::Ui,
    entries: &[crate::env_check::EnvCheckEntry],
    lang: &i18n::Language,
) {
    let palette = winui::palette(widgets::current_theme(ui));
    for entry in entries {
        let Some(key) = crate::env_check::id_to_key(&entry.id) else {
            continue;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::t(key, lang));

            let required = if entry.required {
                i18n::Key::EnvCheckTagRequired
            } else {
                i18n::Key::EnvCheckTagOptional
            };
            ui.label(RichText::new(i18n::t(required, lang)).small().weak());

            let (tag, color) = if entry.found {
                (i18n::Key::EnvCheckTagFound, palette.success)
            } else {
                (i18n::Key::EnvCheckTagMissing, palette.danger)
            };
            ui.label(RichText::new(i18n::t(tag, lang)).small().color(color));

            if !entry.version.is_empty() {
                ui.label(RichText::new(&entry.version).small().weak());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env_check::{EnvCheckEntry, EnvCheckHandle, ID_GIT};

    fn git_entry(found: bool) -> EnvCheckEntry {
        EnvCheckEntry {
            id: ID_GIT.to_string(),
            found,
            ..Default::default()
        }
    }

    #[test]
    fn entry_found_requires_id_and_found() {
        assert!(entry_found(&[git_entry(true)], ID_GIT));
        assert!(!entry_found(&[git_entry(false)], ID_GIT));
        assert!(!entry_found(&[], ID_GIT));
        assert!(!entry_found(&[git_entry(true)], crate::env_check::ID_CUDA));
    }

    #[test]
    fn can_clone_needs_git_and_nonempty_repo_and_branch() {
        let handle = EnvCheckHandle::new();
        let mut settings = AppSettings {
            env_check_result: vec![git_entry(true)],
            build_git_repo: "https://github.com/ggml-org/llama.cpp.git".to_string(),
            build_branch: "master".to_string(),
            ..Default::default()
        };
        assert!(can_clone(&handle, &settings));

        settings.build_git_repo = "   ".to_string();
        assert!(!can_clone(&handle, &settings));

        settings.build_git_repo = "https://github.com/ggml-org/llama.cpp.git".to_string();
        settings.build_branch = String::new();
        assert!(!can_clone(&handle, &settings));

        settings.build_branch = "master".to_string();
        settings.env_check_result = vec![git_entry(false)];
        assert!(!can_clone(&handle, &settings));

        settings.env_check_result = Vec::new();
        assert!(!can_clone(&handle, &settings));
    }
}
