use crate::config::settings::{AppSettings, SettingsManager};
use crate::engine::server::ServerManager;
use crate::i18n;
use crate::ui::helper;
use crate::ui::widgets;
use fluent_egui::winui;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;

pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    settings_manager: &SettingsManager,
    lang: &i18n::Language,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] server_manager: &ServerManager,
    downloader: &crate::downloader::DownloadHandle,
    check_update: &mut Option<poll_promise::Promise<(String, Option<String>)>>,
) {
    // 下载成功时回写 server_path（幂等）
    let snapshot = downloader.snapshot();
    if let crate::downloader::DownloadState::Success(path) = &snapshot.state {
        let new_path = PathBuf::from(path.as_str());
        if settings.server_path != new_path {
            settings.server_path = new_path;
        }
    }

    let accent = crate::theme::accent_color(&settings.accent_color);

    // ── Server 路径 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::PanelServerTitle, lang),
        accent,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelServerPath, lang));
                let mut server_path_str = settings.server_path.to_string_lossy().to_string();
                let response = ui.add(winui::FluentTextEdit::new(&mut server_path_str));
                if response.changed() {
                    settings.server_path = std::path::PathBuf::from(&server_path_str);
                }
            });

            ui.horizontal(|ui| {
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnBrowse,
                        lang,
                    )))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(i18n::t(i18n::Key::DialogSelectServer, lang))
                        .add_filter(i18n::t(i18n::Key::FilterExecutable, lang), &["exe"])
                        .pick_file()
                    {
                        settings.server_path = path;
                    }
                }
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnAutoDetect,
                        lang,
                    )))
                    .clicked()
                {
                    if let Some(path) = settings_manager.auto_detect_server_path() {
                        settings.server_path = path;
                    } else {
                        settings.server_path = std::path::PathBuf::from("");
                    }
                }

                #[cfg(target_os = "linux")]
                {
                    let server_exists = server_manager.check_server(&settings.server_path);
                    let btn = egui::Button::new(i18n::t(i18n::Key::BtnAutoAuthorize, lang))
                        .min_size(egui::vec2(70.0, 20.0));
                    let btn = if server_exists {
                        btn
                    } else {
                        btn.sense(egui::Sense::hover())
                    };
                    if ui.add(btn).clicked() {
                        if let Err(e) = server_manager.authorize_server(&settings.server_path) {
                            log::error!("自动授权失败: {}", e);
                        }
                    }
                }

                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnCheckVersion,
                        lang,
                    )))
                    .clicked()
                {
                    settings.llama_version = get_local_llama_version(&settings.server_path);
                }

                if !settings.llama_version.is_empty() {
                    ui.small(egui::RichText::new(&settings.llama_version).weak());
                }
                if settings.update_available == Some(true) {
                    let text = if let Some(ref tag) = settings.new_version_tag {
                        format!("{} {}", i18n::t(i18n::Key::StatusNewVersion, lang), tag)
                    } else {
                        i18n::t(i18n::Key::StatusNewVersion, lang).to_string()
                    };
                    ui.small(egui::RichText::new(text).weak());
                }
            });

            ui.separator();

            // llama.cpp 下载子区标题（与参数面板"思考 (Reasoning)"同风格：显式主文本色 + strong）
            ui.label(
                egui::RichText::new(i18n::t(i18n::Key::SubSectionLlamaCppDownload, lang))
                    .color(ui.visuals().text_color())
                    .strong(),
            );

            // 变体选择（按平台展示对应选项）
            let platform_supported = cfg!(target_os = "windows") || cfg!(target_os = "linux");
            let busy = downloader.is_busy();
            let is_linux = cfg!(target_os = "linux");

            // 版本分支选择（在变体选择之前）
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::BranchLabel, lang));
                ui.selectable_value(
                    &mut settings.llama_branch,
                    "main".to_string(),
                    i18n::t(i18n::Key::BranchMain, lang),
                );
                ui.selectable_value(
                    &mut settings.llama_branch,
                    "lemonade_rocm".to_string(),
                    i18n::t(i18n::Key::BranchLemonadeRocm, lang),
                );
                ui.selectable_value(
                    &mut settings.llama_branch,
                    "turboquant".to_string(),
                    i18n::t(i18n::Key::BranchTurboQuant, lang),
                );
            });

            // Lemonade ROCm 分支：固定为 rocm_lemonade 变体 + 显示 GPU 目标选择
            let is_lemonade_branch = settings.llama_branch == "lemonade_rocm";
            let is_turboquant_branch = settings.llama_branch == "turboquant";
            if is_lemonade_branch {
                settings.download_variant = "rocm_lemonade".to_string();
                // Lemonade ROCm 只有 preview 通道
                if settings.release_channel != "preview" {
                    settings.release_channel = "preview".to_string();
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::GpuTargetLabel, lang));
                    let gpu_targets = [
                        ("gfx103X", i18n::t(i18n::Key::VariantGpuGfx103X, lang)),
                        ("gfx110X", i18n::t(i18n::Key::VariantGpuGfx110X, lang)),
                        ("gfx1150", i18n::t(i18n::Key::VariantGpuGfx1150, lang)),
                        ("gfx1151", i18n::t(i18n::Key::VariantGpuGfx1151, lang)),
                        ("gfx120X", i18n::t(i18n::Key::VariantGpuGfx120X, lang)),
                        ("gfx908", i18n::t(i18n::Key::VariantGpuGfx908, lang)),
                        ("gfx90a", i18n::t(i18n::Key::VariantGpuGfx90a, lang)),
                    ];
                    let gpu_labels: Vec<&str> =
                        gpu_targets.iter().map(|(_, label)| *label).collect();
                    let mut gpu_idx = gpu_targets
                        .iter()
                        .position(|(key, _)| *key == settings.rocm_gpu_target)
                        .unwrap_or(0);
                    if widgets::segmented(ui, &gpu_labels, &mut gpu_idx, accent) {
                        settings.rocm_gpu_target = gpu_targets[gpu_idx].0.to_string();
                    }
                });
            } else {
                // 兼容旧版：把历史 "gpu" 归一化为当前平台默认 GPU 变体（幂等）
                if settings.download_variant == "gpu"
                    || settings.download_variant == "rocm_lemonade"
                {
                    settings.download_variant = if is_linux {
                        "vulkan".to_string()
                    } else {
                        "cuda124".to_string()
                    };
                }

                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut settings.download_variant,
                        "cpu".to_string(),
                        i18n::t(i18n::Key::VariantCpu, lang),
                    );
                    if !is_linux {
                        ui.selectable_value(
                            &mut settings.download_variant,
                            "cuda124".to_string(),
                            i18n::t(i18n::Key::VariantGpuCuda, lang),
                        );
                        ui.selectable_value(
                            &mut settings.download_variant,
                            "cuda133".to_string(),
                            i18n::t(i18n::Key::VariantGpuCuda133, lang),
                        );
                    }
                    // TurboQuant 分支在 Windows 下不显示 ROCm10 选项
                    if !(is_turboquant_branch && !is_linux) {
                        ui.selectable_value(
                            &mut settings.download_variant,
                            "rocm10".to_string(),
                            i18n::t(i18n::Key::VariantGpuRocm10, lang),
                        );
                    }
                    ui.selectable_value(
                        &mut settings.download_variant,
                        "vulkan".to_string(),
                        i18n::t(i18n::Key::VariantGpuVulkan, lang),
                    );
                    // SYCL / OpenVINO 选项（仅主分支显示，TurboQuant 分支不显示）
                    if !is_turboquant_branch {
                        ui.selectable_value(
                            &mut settings.download_variant,
                            "sycl".to_string(),
                            i18n::t(i18n::Key::VariantGpuSycl, lang),
                        );
                        ui.selectable_value(
                            &mut settings.download_variant,
                            "openvino".to_string(),
                            i18n::t(i18n::Key::VariantGpuOpenVino, lang),
                        );
                    }
                });
            }

            // 解析当前选中配置对应的下载变体（平台感知 + 兜底，使用用户选择的 GPU 目标）
            let variant = crate::downloader::DownloadVariant::from_settings_with_gpu_target(
                &settings.download_variant,
                &settings.rocm_gpu_target,
            );

            // 发布通道选择（stable/preview）— Lemonade ROCm 分支和 TurboQuant 分支不需要
            if !is_lemonade_branch && !is_turboquant_branch {
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::ReleaseChannelLabel, lang));
                    ui.selectable_value(
                        &mut settings.release_channel,
                        "stable".to_string(),
                        i18n::t(i18n::Key::ReleaseChannelStable, lang),
                    );
                    ui.selectable_value(
                        &mut settings.release_channel,
                        "preview".to_string(),
                        i18n::t(i18n::Key::ReleaseChannelPreview, lang),
                    );
                });
            }

            // 下载内嵌 CUDA 库开关：仅 Windows + CUDA 12/13 变体时显示，TurboQuant 分支不显示
            if cfg!(target_os = "windows")
                && !is_turboquant_branch
                && (settings.download_variant == "cuda124"
                    || settings.download_variant == "cuda133")
            {
                ui.horizontal(|ui| {
                    ui.add(winui::FluentCheckBox::new(
                        &mut settings.download_cuda_lib,
                        i18n::t(i18n::Key::CudaLibDownloadLabel, lang),
                    ));
                });
            }

            // 下载 llama.cpp + 检查更新：单独一行
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        platform_supported && !busy,
                        winui::FluentButton::new(i18n::t(i18n::Key::BtnDownloadLlamaCpp, lang)),
                    )
                    .clicked()
                {
                    downloader.start_download(
                        settings_manager.config_dir().to_path_buf(),
                        variant,
                        settings.release_channel.clone(),
                        settings.download_cuda_lib,
                        settings.llama_branch.clone(),
                    );
                }

                // 检查更新：llama-server 路径非空时可用
                let checking = check_update.is_some();
                if ui
                    .add_enabled(
                        !settings.server_path.to_string_lossy().is_empty() && !checking,
                        winui::FluentButton::new(if checking {
                            i18n::t(i18n::Key::StatusChecking, lang)
                        } else {
                            i18n::t(i18n::Key::BtnCheckUpdate, lang)
                        }),
                    )
                    .clicked()
                {
                    let variant = crate::downloader::DownloadVariant::from_settings_with_gpu_target(
                        &settings.download_variant,
                        &settings.rocm_gpu_target,
                    );
                    let server_path = settings.server_path.clone();
                    let release_channel = settings.release_channel.clone();
                    let llama_branch = settings.llama_branch.clone();
                    *check_update = Some(poll_promise::Promise::spawn_thread(
                        "check-update",
                        move || {
                            let llama_version =
                                super::server_panel::get_local_llama_version(&server_path);
                            let result = crate::downloader::fetch_latest_tag(
                                variant,
                                &release_channel,
                                &llama_branch,
                            );
                            match result {
                                Ok(latest) => {
                                    let build_tag =
                                        super::server_panel::extract_build_tag(&llama_version);
                                    let up_to_date = build_tag == Some(latest.clone());
                                    (llama_version, if up_to_date { None } else { Some(latest) })
                                }
                                Err(e) => {
                                    log::error!("检查更新失败: {}", e);
                                    (llama_version, None)
                                }
                            }
                        },
                    ));
                }
            });

            // 进度/状态行（仅非 Idle 时渲染）
            match &snapshot.state {
                crate::downloader::DownloadState::Running => {
                    let phase_key = match snapshot.phase {
                        crate::downloader::Phase::FetchingRelease => i18n::Key::DlPhaseFetching,
                        crate::downloader::Phase::Downloading => i18n::Key::DlPhaseDownloading,
                        crate::downloader::Phase::Extracting => i18n::Key::DlPhaseExtracting,
                        crate::downloader::Phase::LocatingServer => i18n::Key::DlPhaseLocating,
                    };
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(phase_key, lang));
                        let ratio = snapshot
                            .total
                            .map(|t| {
                                if t > 0 {
                                    snapshot.done as f32 / t as f32
                                } else {
                                    0.0
                                }
                            })
                            .unwrap_or(0.0);
                        ui.small(format!("{:.1}%", ratio * 100.0));
                        ui.add(winui::FluentProgressBar::new(ratio).show_percentage(false));
                    });
                }
                crate::downloader::DownloadState::Success(_) => {
                    ui.label(i18n::t(i18n::Key::DlSuccess, lang));
                }
                crate::downloader::DownloadState::Error(message) => {
                    let display = if message == crate::downloader::ERR_NETWORK {
                        i18n::t(i18n::Key::UpdNetworkError, lang).to_string()
                    } else {
                        format!("{}: {}", i18n::t(i18n::Key::DlFailed, lang), message)
                    };
                    ui.label(egui::RichText::new(display).color(ui.visuals().error_fg_color));
                }
                crate::downloader::DownloadState::Idle => {}
            }

            // 更新清空目录警告
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(i18n::t(i18n::Key::WarningLlamaCppUpdate, lang))
                    .color(egui::Color32::from_rgb(220, 50, 50))
                    .size(11.0),
            );
        },
    );

    // ── 网络 ──
    widgets::card(ui, i18n::t(i18n::Key::SectionNetwork, lang), accent, |ui| {
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelHost, lang));
            ui.add(winui::FluentTextEdit::new(&mut settings.host));
            ui.label(i18n::t(i18n::Key::LabelPort, lang));
            let mut port = settings.port as f64;
            ui.add(
                winui::NumberBox::new(&mut port)
                    .range(1.0..=65535.0)
                    .width(90.0)
                    .spin_button_placement(winui::NumberBoxSpinButtonPlacement::Compact),
            );
            settings.port = port as _;
        });

        ui.horizontal(|ui| {
            if ui
                .add(
                    winui::FluentButton::new(i18n::t(i18n::Key::BtnHostLocal, lang))
                        .min_width(40.0),
                )
                .clicked()
            {
                settings.host = "127.0.0.1".to_string();
            }
            if ui
                .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnHostAny, lang)).min_width(50.0))
                .clicked()
            {
                settings.host = "0.0.0.0".to_string();
            }
        });

        // 并行槽位
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelParallelSlots, lang));
            let mut slots = settings.parallel_slots as f64;
            ui.add(
                winui::NumberBox::new(&mut slots)
                    .range(-1.0..=1024.0)
                    .width(90.0)
                    .spin_button_placement(winui::NumberBoxSpinButtonPlacement::Compact),
            );
            settings.parallel_slots = slots as _;
            if settings.parallel_slots == 0 {
                settings.parallel_slots = 1;
            }
        });

        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelAlias, lang));
            ui.add(winui::FluentTextEdit::new(&mut settings.alias));
        });
    });

    // ── 功能开关（Toggle 在左，标签在右）──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionFeatures, lang),
        accent,
        |ui| {
            // ★ 新 toggle 签名：toggle(ui, &mut bool, "标签", accent) —— 内部渲染 [开关][标签]
            widgets::toggle(
                ui,
                &mut settings.verbose,
                i18n::t(i18n::Key::CheckboxVerbose, lang),
                accent,
            );
            widgets::toggle(
                ui,
                &mut settings.log_timestamps,
                i18n::t(i18n::Key::CheckboxLogTimestamps, lang),
                accent,
            );
            widgets::toggle(
                ui,
                &mut settings.offline_mode,
                i18n::t(i18n::Key::CheckboxOfflineMode, lang),
                accent,
            );
            widgets::toggle(
                ui,
                &mut settings.rpc_mode,
                i18n::t(i18n::Key::CheckboxRpcMode, lang),
                accent,
            );
            if settings.rpc_mode {
                ui.indent("rpc_endpoints", |ui| {
                    // 标题行
                    ui.label(i18n::t(i18n::Key::LabelRpcEndpoints, lang));

                    // 解析并显示已有的节点地址
                    let endpoints: Vec<String> = settings
                        .rpc_endpoints
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    // 节点列表（表格形式）
                    if endpoints.is_empty() {
                        ui.label(i18n::t(i18n::Key::HintRpcEndpointsEmpty, lang));
                    } else {
                        egui::Grid::new("rpc_endpoints_grid")
                            .striped(true)
                            .show(ui, |ui| {
                                // 表头
                                ui.label(i18n::t(i18n::Key::LabelRpcEndpointIndex, lang));
                                ui.label(i18n::t(i18n::Key::LabelRpcEndpointAddress, lang));
                                ui.label(""); // 删除按钮列
                                ui.end_row();

                                // 节点列表
                                let mut indices_to_remove: Vec<usize> = Vec::new();
                                for (i, addr) in endpoints.iter().enumerate() {
                                    ui.label(format!("{}", i + 1));
                                    // 判断是否多卡节点（末尾带 +）
                                    let is_multi_gpu = addr.ends_with('+');
                                    let display_addr = addr.trim_end_matches('+');
                                    ui.horizontal(|ui| {
                                        ui.label(display_addr);
                                        if is_multi_gpu {
                                            ui.label(
                                                egui::RichText::new("MULTI-GPU")
                                                    .color(egui::Color32::from_rgb(255, 165, 0))
                                                    .small(),
                                            );
                                        }
                                    });
                                    if ui
                                        .small(egui::RichText::new("×").color(egui::Color32::RED))
                                        .on_hover_text(i18n::t(i18n::Key::BtnRemoveEndpoint, lang))
                                        .clicked()
                                    {
                                        indices_to_remove.push(i);
                                    }
                                    ui.end_row();
                                }

                                // 删除操作（从后往前删避免索引偏移）
                                if !indices_to_remove.is_empty() {
                                    let mut new_endpoints = endpoints.clone();
                                    for &i in indices_to_remove.iter().rev() {
                                        new_endpoints.remove(i);
                                    }
                                    settings.rpc_endpoints = new_endpoints.join(",");
                                }
                            });
                    }

                    // 添加新节点地址
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelAddEndpoint, lang));
                        let response =
                            ui.add(winui::FluentTextEdit::new(&mut settings.rpc_endpoint_input));
                        if ui
                            .add(winui::FluentButton::new(i18n::t(
                                i18n::Key::BtnAddEndpoint,
                                lang,
                            )))
                            .clicked()
                            || (response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            let trimmed = settings.rpc_endpoint_input.trim().to_string();
                            if !trimmed.is_empty() {
                                // 根据多卡节点勾选状态拼接 + 标记
                                let addr = if settings.rpc_endpoint_multi_gpu {
                                    format!("{}+", trimmed)
                                } else {
                                    trimmed
                                };
                                // 检查是否已存在（比较时忽略末尾 +）
                                let exists = endpoints
                                    .iter()
                                    .any(|e| e.trim_end_matches('+') == addr.trim_end_matches('+'));
                                if !exists {
                                    if settings.rpc_endpoints.is_empty() {
                                        settings.rpc_endpoints = addr;
                                    } else {
                                        settings.rpc_endpoints =
                                            format!("{},{}", settings.rpc_endpoints, addr);
                                    }
                                    // 添加成功后清空输入框
                                    settings.rpc_endpoint_input.clear();
                                }
                            }
                        }
                        ui.add(winui::FluentCheckBox::new(
                            &mut settings.rpc_endpoint_multi_gpu,
                            i18n::t(i18n::Key::CheckboxMultiGpuNode, lang),
                        ));
                    });

                    // 添加本机 RPC 客户端按钮
                    let local_addr = format!("127.0.0.1:{}", settings.rpc_port);
                    let already_added = endpoints.iter().any(|s| s == &local_addr);
                    if ui
                        .add_enabled(
                            !already_added,
                            winui::FluentButton::new(i18n::t(
                                i18n::Key::BtnAddLocalRpcClient,
                                lang,
                            )),
                        )
                        .clicked()
                    {
                        if settings.rpc_endpoints.is_empty() {
                            settings.rpc_endpoints = local_addr.clone();
                        } else {
                            settings.rpc_endpoints =
                                format!("{},{}", settings.rpc_endpoints, local_addr);
                        }
                    }
                });
            }
            widgets::toggle(
                ui,
                &mut settings.web_ui_enabled,
                i18n::t(i18n::Key::CheckboxEnableWebClient, lang),
                accent,
            );

            // 日志级别（--log-verbosity）分段控件
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelLogVerbosity, lang));
                let lv_vals = [0usize, 1, 2, 3, 4, 5];
                let lv_labels = [
                    i18n::t(i18n::Key::LogVerbosity0, lang),
                    i18n::t(i18n::Key::LogVerbosity1, lang),
                    i18n::t(i18n::Key::LogVerbosity2, lang),
                    i18n::t(i18n::Key::LogVerbosity3, lang),
                    i18n::t(i18n::Key::LogVerbosity4, lang),
                    i18n::t(i18n::Key::LogVerbosity5, lang),
                ];
                let mut lv_idx = lv_vals
                    .iter()
                    .position(|v| *v == settings.log_verbosity)
                    .unwrap_or(3);
                widgets::segmented(ui, &lv_labels, &mut lv_idx, accent);
                settings.log_verbosity = lv_vals[lv_idx];
            });
        },
    );

    // ── API 安全 / 部署 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionApiSecurity, lang),
        accent,
        |ui| {
            // API Key
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelApiKey, lang));
                ui.add(winui::FluentTextEdit::new(&mut settings.api_key));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpApiKey, lang));
            });
            // API Prefix
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelApiPrefix, lang));
                ui.add(winui::FluentTextEdit::new(&mut settings.api_prefix));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpApiPrefix, lang));
            });
            // API Key File
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::AdvApiKeyFile, lang));
                ui.add(winui::FluentTextEdit::new(&mut settings.api_key_file));
                helper::help_button_inline(ui, i18n::t(i18n::Key::AdvApiKeyFileHint, lang));
            });
            // CORS Origins
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelCorsOrigins, lang));
                ui.add(winui::FluentTextEdit::new(&mut settings.cors_origins));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCorsOrigins, lang));
            });
            // SSL 证书 / 私钥
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelSslCertFile, lang));
                let mut cert_str = settings.ssl_cert_file.to_string_lossy().to_string();
                let resp = ui.add(winui::FluentTextEdit::new(&mut cert_str));
                if resp.changed() {
                    settings.ssl_cert_file = PathBuf::from(&cert_str);
                }
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnBrowseCert,
                        lang,
                    )))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(i18n::t(i18n::Key::LabelSslCertFile, lang))
                        .add_filter("PEM", &["pem", "crt", "cer"])
                        .pick_file()
                    {
                        settings.ssl_cert_file = path;
                    }
                }
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSsl, lang));
            });
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelSslKeyFile, lang));
                let mut key_str = settings.ssl_key_file.to_string_lossy().to_string();
                let resp = ui.add(winui::FluentTextEdit::new(&mut key_str));
                if resp.changed() {
                    settings.ssl_key_file = PathBuf::from(&key_str);
                }
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnBrowseCert,
                        lang,
                    )))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(i18n::t(i18n::Key::LabelSslKeyFile, lang))
                        .add_filter("PEM", &["pem", "key"])
                        .pick_file()
                    {
                        settings.ssl_key_file = path;
                    }
                }
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSsl, lang));
            });
            // 端口复用（开关在最左面）
            ui.horizontal(|ui| {
                widgets::toggle(
                    ui,
                    &mut settings.reuse_port,
                    i18n::t(i18n::Key::CheckboxReusePort, lang),
                    accent,
                );
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpReusePort, lang));
            });
        },
    );
}

/// 运行 llama-server --version，返回解析出的版本字符串
pub fn get_local_llama_version(server_path: &std::path::Path) -> String {
    let mut cmd = std::process::Command::new(server_path);
    cmd.arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000u32); // CREATE_NO_WINDOW
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let all_lines: Vec<&str> = stdout.lines().chain(stderr.lines()).collect();
            // 兼容 "version: xxx" 与 llama.cpp 的 "version = xxx (bxxx, hash)" 两种格式
            let version_line = all_lines
                .iter()
                .find(|line| line.contains("version:"))
                .or_else(|| {
                    all_lines
                        .iter()
                        .find(|line| line.trim_start().starts_with("version"))
                });
            version_line
                .and_then(|line| {
                    // 去掉 "version:" / "version =" 前缀，保留版本号部分
                    let value = if let Some(v) = line.split_once("version:") {
                        v.1
                    } else if let Some(v) = line.split_once("version") {
                        v.1
                    } else {
                        return None;
                    };
                    let trimmed = value.trim().trim_start_matches('=').trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .unwrap_or_else(|| "未知版本".to_string())
        }
        Err(e) => format!("获取失败: {}", e),
    }
}

/// 从版本字符串中提取 build 标签（如 "b10549" 或 "build 10621"）
pub fn extract_build_tag(version: &str) -> Option<String> {
    // 先尝试匹配 "build 10621" 格式
    if let Some(pos) = version.find("build ") {
        let start = pos + 6; // "build " 长度
        let bytes = version.as_bytes();
        if start < bytes.len()
            && bytes[start..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count()
                > 0
        {
            let end = start
                + bytes[start..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
            return Some(format!("b{}", &version[start..end]));
        }
    }
    // 再尝试匹配 "b10621" 格式
    let bytes = version.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'b' && bytes[i + 1].is_ascii_digit() {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            return Some(format!("b{}", &version[start..end]));
        }
        i += 1;
    }
    None
}
