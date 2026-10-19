//! llama.cpp 编译面板 —— 编译环境检测 + 仓库/分支/Tag + 后端选择 + 克隆/编译入口
//!
//! 原为设置面板内的一张卡片，现独立为顶层导航项。

use crate::config::settings::AppSettings;
use crate::i18n;
use crate::ui::widgets;
use egui::RichText;
use fluent_egui::winui;
use std::borrow::Cow;

pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    lang: &i18n::Language,
    env_check: &crate::env_check::EnvCheckHandle,
    clone_handle: &crate::build::CloneHandle,
    build_handle: &crate::build::BuildHandle,
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

        // 编译选项（CUDA/ROCm 按工具包安装状态门控，网页客户端资源始终显示）
        ui.label(i18n::t(i18n::Key::BuildOptions, lang));
        ui.label(
            RichText::new(i18n::t(i18n::Key::BuildOptionsHint, lang))
                .small()
                .weak(),
        );
        render_cmake_options(ui, settings, env_check, lang);

        ui.add_space(8.0);

        ui.label(i18n::t(i18n::Key::BuildCmdPreview, lang));
        let command_preview = crate::build::cmake_command_preview(settings);
        ui.monospace(&command_preview);
        if ui
            .add(winui::FluentButton::new(i18n::t(
                i18n::Key::BtnCopyToClipboard,
                lang,
            )))
            .clicked()
        {
            ui.ctx().copy_text(command_preview.clone());
        }

        ui.add_space(8.0);

        // 操作按钮
        let build_enabled = can_build(env_check, settings, clone_handle);
        let building = build_handle.is_busy();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    build_enabled && !building,
                    widgets::rounded_button(i18n::t(i18n::Key::BuildBtnBuild, lang), None),
                )
                .clicked()
            {
                build_handle.start(settings);
            }
            if building
                && ui
                    .add(widgets::rounded_button(
                        i18n::t(i18n::Key::BuildBtnCancelBuild, lang),
                        None,
                    ))
                    .clicked()
            {
                build_handle.request_cancel();
            }
        });
        render_build_status(ui, build_handle, lang);
        if !build_enabled && !building {
            ui.label(
                RichText::new(i18n::t(i18n::Key::BuildBtnBuildHint, lang))
                    .small()
                    .weak(),
            );
        }
    });
}

/// 渲染编译状态：进行中（按阶段）/ 完成 / 失败（含英文原因）；空闲时不渲染。
fn render_build_status(
    ui: &mut egui::Ui,
    build_handle: &crate::build::BuildHandle,
    lang: &i18n::Language,
) {
    use crate::build::{BuildPhase, BuildState};

    let palette = winui::palette(widgets::current_theme(ui));
    let status = build_handle.snapshot();
    match &status.state {
        BuildState::Running => {
            let key = match status.phase {
                Some(BuildPhase::Compile) => i18n::Key::BuildStatusCompiling,
                _ => i18n::Key::BuildStatusConfiguring,
            };
            ui.label(RichText::new(i18n::t(key, lang)).weak());
            ui.label(
                RichText::new(i18n::t(i18n::Key::BuildLogHint, lang))
                    .small()
                    .weak(),
            );
        }
        BuildState::Done => {
            ui.label(
                RichText::new(i18n::t(i18n::Key::BuildStatusDone, lang))
                    .small()
                    .color(palette.success),
            );
        }
        BuildState::Failed(msg) => {
            ui.label(
                RichText::new(i18n::t(i18n::Key::BuildStatusError, lang))
                    .small()
                    .color(palette.danger),
            );
            ui.label(RichText::new(msg.as_str()).small().weak());
            ui.label(
                RichText::new(i18n::t(i18n::Key::BuildLogHint, lang))
                    .small()
                    .weak(),
            );
        }
        BuildState::Idle => {}
    }
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

/// 当前生效的检测条目：优先本次运行结果，否则回退到已保存结果。
fn env_entries<'a>(
    env_check: &crate::env_check::EnvCheckHandle,
    settings: &'a AppSettings,
) -> Cow<'a, [crate::env_check::EnvCheckEntry]> {
    match &env_check.snapshot().state {
        crate::env_check::EnvCheckState::Done(results) => {
            Cow::Owned(crate::env_check::to_entries(results))
        }
        _ => Cow::Borrowed(settings.env_check_result.as_slice()),
    }
}

/// 指定组件是否可用：优先本次运行结果，否则回退到已保存结果。
fn component_available(
    env_check: &crate::env_check::EnvCheckHandle,
    settings: &AppSettings,
    id: &str,
) -> bool {
    entry_found(&env_entries(env_check, settings), id)
}

/// 「克隆仓库」是否可点：git 可用 + 仓库地址、分支均非空。
fn can_clone(env_check: &crate::env_check::EnvCheckHandle, settings: &AppSettings) -> bool {
    component_available(env_check, settings, crate::env_check::ID_GIT)
        && !settings.build_git_repo.trim().is_empty()
        && !settings.build_branch.trim().is_empty()
}

/// 「开始编译」是否可点：环境必需项齐全 + CUDA/ROCm 工具包至少一个可用 + 源码目录非空；
/// 克隆 / 更新进行中时也不可点——源码树正在变动，此时编译必然失败。
fn can_build(
    env_check: &crate::env_check::EnvCheckHandle,
    settings: &AppSettings,
    clone_handle: &crate::build::CloneHandle,
) -> bool {
    if clone_handle.is_busy() {
        return false;
    }
    let entries = env_entries(env_check, settings);
    crate::env_check::all_required_found_entries(&entries)
        && (entry_found(&entries, crate::env_check::ID_CUDA)
            || entry_found(&entries, crate::env_check::ID_ROCM))
        && crate::build::source_dir_ready(settings)
}

/// 开关复选框的显示名：`选项名 (说明)`；spec 固定为 `=OFF` 的（勾选即禁用某特性）前置「禁用」
fn option_display_label(option: &str, desc: i18n::Key, lang: &i18n::Language) -> String {
    let name = crate::build::option_label(option);
    if crate::build::option_is_disable(option) {
        format!(
            "{} {} ({})",
            i18n::t(i18n::Key::BuildDisablePrefix, lang),
            name,
            i18n::t(desc, lang)
        )
    } else {
        format!("{} ({})", name, i18n::t(desc, lang))
    }
}

/// 按分组渲染编译开关复选框；`gated` 为 `Some(false)` 时隐藏该组并提示缺失的工具包。
/// 勾选结果写回 `settings.build_cmake_options`
fn render_cmake_options(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    env_check: &crate::env_check::EnvCheckHandle,
    lang: &i18n::Language,
) {
    let cuda_ok = component_available(env_check, settings, crate::env_check::ID_CUDA);
    let rocm_ok = component_available(env_check, settings, crate::env_check::ID_ROCM);

    for (group, gated, title, missing) in [
        (
            crate::build::OptionGroup::General,
            None,
            i18n::Key::BuildGeneral,
            None,
        ),
        (
            crate::build::OptionGroup::Cuda,
            Some(cuda_ok),
            i18n::Key::BuildCuda,
            Some(i18n::Key::BuildCudaMissing),
        ),
        (
            crate::build::OptionGroup::Hip,
            Some(rocm_ok),
            i18n::Key::BuildRocm,
            Some(i18n::Key::BuildRocmMissing),
        ),
        (
            crate::build::OptionGroup::Ui,
            None,
            i18n::Key::BuildUiAssets,
            None,
        ),
    ] {
        ui.add_space(4.0);
        ui.label(i18n::t(title, lang));
        if let Some(false) = gated {
            if let Some(missing) = missing {
                ui.label(RichText::new(i18n::t(missing, lang)).small().weak());
            }
            continue;
        }
        for &(option, desc) in crate::build::cmake_options(group) {
            let mut checked =
                crate::build::is_option_selected(&settings.build_cmake_options, option);
            let label = option_display_label(option, desc, lang);
            if ui
                .add(winui::FluentCheckBox::new(&mut checked, label))
                .changed()
            {
                crate::build::set_option_selected(
                    &mut settings.build_cmake_options,
                    option,
                    checked,
                );
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

    #[test]
    fn can_build_needs_required_gpu_and_source() {
        let env_handle = EnvCheckHandle::new();
        let clone_handle = crate::build::CloneHandle::default();
        let source_dir = std::env::temp_dir().join("llama_launcher_can_build_probe");
        let _ = std::fs::remove_dir_all(&source_dir);
        std::fs::create_dir_all(&source_dir).unwrap();

        let mut settings = AppSettings {
            env_check_result: vec![
                EnvCheckEntry {
                    id: ID_GIT.to_string(),
                    required: true,
                    found: true,
                    ..Default::default()
                },
                EnvCheckEntry {
                    id: crate::env_check::ID_CUDA.to_string(),
                    found: true,
                    ..Default::default()
                },
            ],
            build_work_dir: source_dir.display().to_string(),
            ..Default::default()
        };
        assert!(!can_build(&env_handle, &settings, &clone_handle));

        std::fs::write(source_dir.join("CMakeLists.txt"), "").unwrap();
        assert!(can_build(&env_handle, &settings, &clone_handle));

        settings
            .env_check_result
            .retain(|e| e.id != crate::env_check::ID_CUDA);
        assert!(!can_build(&env_handle, &settings, &clone_handle));

        settings.env_check_result.push(EnvCheckEntry {
            id: crate::env_check::ID_ROCM.to_string(),
            found: true,
            ..Default::default()
        });
        assert!(can_build(&env_handle, &settings, &clone_handle));

        settings.env_check_result[0].found = false;
        assert!(!can_build(&env_handle, &settings, &clone_handle));

        settings.env_check_result = Vec::new();
        assert!(!can_build(&env_handle, &settings, &clone_handle));

        let _ = std::fs::remove_dir_all(&source_dir);
    }

    #[test]
    fn disable_spec_gets_disabled_prefix_in_label() {
        let lang = i18n::Language::Zh;

        let disabled = option_display_label(
            "LLAMA_BUILD_TESTS=OFF",
            i18n::Key::BuildOptLlamaBuildTests,
            &lang,
        );
        assert!(
            disabled.starts_with("禁用 LLAMA_BUILD_TESTS ("),
            "{disabled}"
        );

        let plain = option_display_label("GGML_CUDA", i18n::Key::BuildOptCuda, &lang);
        assert!(plain.starts_with("GGML_CUDA ("), "{plain}");
        assert!(!plain.contains("禁用"));
    }
}
