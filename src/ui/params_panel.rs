use crate::config::settings::{
    is_server_binary_name, AppSettings, GpuLayersMode, KvCachePromiseWrapper,
    MaxContextPromiseWrapper,
};
use crate::i18n;
use crate::kv_cache;
use crate::ui::helper;
use crate::ui::widgets;
use poll_promise::Promise;

pub fn ui(ui: &mut egui::Ui, settings: &mut AppSettings, lang: &i18n::Language) {
    let accent = crate::theme::accent_color(&settings.accent_color);

    let server_path_valid = settings
        .server_path
        .file_name()
        .and_then(|f| f.to_str())
        .is_some_and(is_server_binary_name);
    let can_start = server_path_valid && !settings.model_path.as_os_str().is_empty();

    // ── 上下文与批次 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionContextBatch, lang),
        accent,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelNCtx, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.context)
                        .range(1..=1024)
                        .speed(1),
                );
                ui.label("k");
                ui.small(i18n::t(i18n::Key::HintKUnit, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpNCtx, lang));
            });
            ui.horizontal(|ui| {
                if ui
                    .button(i18n::t(i18n::Key::BtnSetMaxContextVram, lang))
                    .clicked()
                    && can_start
                    && settings.max_context_promise.0.is_none()
                {
                    // 克隆设置用于后台线程
                    let settings_clone = settings.clone();
                    let lang_clone = lang.clone();
                    settings.max_context_promise = MaxContextPromiseWrapper(Some(
                        Promise::spawn_thread("calc_max_context", move || {
                            kv_cache::calc_max_context_facade(&settings_clone, &lang_clone)
                        }),
                    ));
                }
                // 检查 Promise 状态并更新结果（显示在按钮右侧）
                if let Some(ref promise) = settings.max_context_promise.0 {
                    match promise.ready() {
                        Some(Ok(val)) => {
                            settings.context = *val;
                            settings.max_context_promise = MaxContextPromiseWrapper(None);
                        }
                        Some(Err(e)) => {
                            log::warn!("[params_panel] calc_max_context 失败: {}", e);
                            settings.max_context_promise = MaxContextPromiseWrapper(None);
                        }
                        None => {
                            ui.small(egui::RichText::new("计算中...").weak());
                        }
                    }
                }
            });
            // 批次大小 (--batch-size) (k)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelBatchSize, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.batch_size)
                        .range(0.0001..=16.0)
                        .speed(0.0001)
                        .fixed_decimals(4),
                ); // 0.0001k ~ 16k
                ui.label("k");
                ui.small(i18n::t(i18n::Key::HintKUnit, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpBatchSize, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_batch_size, "", accent);
                });
            });
            // 物理批次大小 (--ubatch-size) (k)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelUBatchSize, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.ubatch_size)
                        .range(0.0001..=16.0)
                        .speed(0.0001)
                        .fixed_decimals(4),
                ); // 0.0001k ~ 16k
                ui.label("k");
                ui.small(i18n::t(i18n::Key::HintKUnit, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpUBatchSize, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_ubatch_size, "", accent);
                });
            });

            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelSessionTimeout, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.session_timeout)
                        .range(60..=3600)
                        .speed(10),
                ); // 60~3600秒，步进10
                ui.label(i18n::t(i18n::Key::HintSUnit, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSessionTimeout, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_session_timeout, "", accent);
                });
            });

            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelKvCacheRatio, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.kv_cache_ratio)
                        .range(0.0..=1.0)
                        .speed(0.01),
                );
                ui.small(i18n::t(i18n::Key::HintKvCacheRatio, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpKvCacheRatio, lang));
            });

            ui.horizontal(|ui| {
                if ui
                    .button(i18n::t(i18n::Key::BtnCalcKvCache, lang))
                    .clicked()
                    && can_start
                    && settings.kv_cache_promise.0.is_none()
                {
                    // 克隆设置用于后台线程
                    let settings_clone = settings.clone();
                    let lang_clone = lang.clone();
                    settings.kv_cache_promise = KvCachePromiseWrapper(Some(Promise::spawn_thread(
                        "calc_kv_cache",
                        move || kv_cache::calc_and_format(&settings_clone, &lang_clone),
                    )));
                }
                // 检查 Promise 状态并更新结果
                if let Some(ref promise) = settings.kv_cache_promise.0 {
                    match promise.ready() {
                        Some(Ok(result)) => {
                            settings.kv_cache_result = Some(format!(
                                "{} {}",
                                i18n::t(i18n::Key::LabelKvCacheResult, lang),
                                result
                            ));
                            settings.kv_cache_promise = KvCachePromiseWrapper(None);
                        }
                        Some(Err(e)) => {
                            settings.kv_cache_result = Some(format!("⚠ {}", e));
                            settings.kv_cache_promise = KvCachePromiseWrapper(None);
                        }
                        None => {
                            ui.small(egui::RichText::new("计算中...").weak());
                        }
                    }
                } else if let Some(ref result) = settings.kv_cache_result {
                    ui.small(egui::RichText::new(result).weak());
                }
            });

            // ── Flash Attention（上下文与批次 子项）──
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelFlashAttn, lang));
                let fa_vals = ["on", "off", "auto"];
                let fa_labels = [
                    i18n::t(i18n::Key::FaModeOn, lang),
                    i18n::t(i18n::Key::FaModeOff, lang),
                    i18n::t(i18n::Key::FaModeAuto, lang),
                ];
                let mut fa_idx = fa_vals
                    .iter()
                    .position(|v| *v == settings.flash_attn)
                    .unwrap_or(2);
                widgets::segmented(ui, &fa_labels, &mut fa_idx, accent);
                settings.flash_attn = fa_vals[fa_idx].to_string();
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpFlashAttn, lang));
            });
            // ── 内存自动调优 --fit ──
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelFit, lang));
                let fit_vals = ["on", "off"];
                let fit_labels = [
                    i18n::t(i18n::Key::FitModeOn, lang),
                    i18n::t(i18n::Key::FitModeOff, lang),
                ];
                let mut fit_idx = fit_vals
                    .iter()
                    .position(|v| *v == settings.fit)
                    .unwrap_or(0);
                widgets::segmented(ui, &fit_labels, &mut fit_idx, accent);
                settings.fit = fit_vals[fit_idx].to_string();
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpFit, lang));
            });
            // ── 显存预留 --fit-target ──
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelFitTarget, lang));
                ui.add(egui::TextEdit::singleline(&mut settings.fit_target).desired_width(180.0));
                ui.label("MiB");
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpFitTarget, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_fit_target, "", accent);
                });
            });
            // ── 最小上下文 --fit-ctx ──
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelFitCtx, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.fit_ctx)
                        .range(1..=1024)
                        .speed(1),
                );
                ui.label("k");
                ui.small(i18n::t(i18n::Key::HintKUnit, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpFitCtx, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_fit_ctx, "", accent);
                });
            });
        },
    );

    // ── 思考与会话 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionThinkingConversation, lang),
        accent,
        |ui| {
            // 思考子区标题
            // ★ 不用 .strong()（浅色模式下 strong_text_color=白色→隐形），改用显式主文本色
            ui.label(
                egui::RichText::new(i18n::t(i18n::Key::SubSectionThinking, lang))
                    .color(ui.visuals().text_color())
                    .strong(),
            );

            // 推理模式 (--reasoning)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelReasoning, lang));
                let r_vals = ["auto", "on", "off"];
                let r_labels = [
                    i18n::t(i18n::Key::ReasoningModeAuto, lang),
                    i18n::t(i18n::Key::ReasoningModeOn, lang),
                    i18n::t(i18n::Key::ReasoningModeOff, lang),
                ];
                let mut r_idx = r_vals
                    .iter()
                    .position(|v| *v == settings.reasoning)
                    .unwrap_or(0);
                widgets::segmented(ui, &r_labels, &mut r_idx, accent);
                settings.reasoning = r_vals[r_idx].to_string();
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpReasoning, lang));
            });

            // 思考格式 (--reasoning-format)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelReasoningFormat, lang));
                let rf_vals = ["auto", "none", "deepseek", "deepseek-legacy"];
                let rf_labels = [
                    i18n::t(i18n::Key::ReasoningFormatAuto, lang),
                    i18n::t(i18n::Key::ReasoningFormatNone, lang),
                    i18n::t(i18n::Key::ReasoningFormatDeepseek, lang),
                    i18n::t(i18n::Key::ReasoningFormatDeepseekLegacy, lang),
                ];
                let mut rf_idx = rf_vals
                    .iter()
                    .position(|v| *v == settings.reasoning_format)
                    .unwrap_or(0);
                widgets::segmented(ui, &rf_labels, &mut rf_idx, accent);
                settings.reasoning_format = rf_vals[rf_idx].to_string();
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpReasoningFormat, lang));
            });

            // 推理强度 (--reasoning-effort)：纯字符串值；default = 不拼接
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelReasoningEffort, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpReasoningEffort, lang));
            });
            let effort_vals = [
                "default", "minimal", "low", "medium", "high", "xhigh", "max",
            ];
            let effort_labels = [
                i18n::t(i18n::Key::EffortDefault, lang),
                i18n::t(i18n::Key::EffortMinimal, lang),
                i18n::t(i18n::Key::EffortLow, lang),
                i18n::t(i18n::Key::EffortMedium, lang),
                i18n::t(i18n::Key::EffortHigh, lang),
                i18n::t(i18n::Key::EffortXhigh, lang),
                i18n::t(i18n::Key::EffortMax, lang),
            ];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                for (i, opt) in effort_vals.iter().enumerate() {
                    let selected = settings.reasoning_effort == *opt;
                    if ui.selectable_label(selected, effort_labels[i]).clicked() {
                        settings.reasoning_effort = opt.to_string();
                    }
                }
            });

            // 保留思考 (--reasoning-preserve)：模型默认 / 开启 / 关闭
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelReasoningPreserve, lang));
                let rp_vals: [Option<bool>; 3] = [None, Some(true), Some(false)];
                let rp_labels = [
                    i18n::t(i18n::Key::ReasoningPreserveDefault, lang),
                    i18n::t(i18n::Key::ReasoningPreserveOn, lang),
                    i18n::t(i18n::Key::ReasoningPreserveOff, lang),
                ];
                let mut rp_idx = rp_vals
                    .iter()
                    .position(|v| *v == settings.reasoning_preserve)
                    .unwrap_or(0);
                widgets::segmented(ui, &rp_labels, &mut rp_idx, accent);
                settings.reasoning_preserve = rp_vals[rp_idx];
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpReasoningPreserve, lang));
            });

            // 思考预算 (--reasoning-budget)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelReasoningBudget, lang));
                ui.add(
                    egui::DragValue::new(&mut settings.reasoning_budget)
                        .range(-1..=32768)
                        .speed(1),
                );
                ui.small(i18n::t(i18n::Key::HintReasoningBudget, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpReasoningBudget, lang));
            });

            ui.separator();

            // 会话子区标题
            // ★ 不用 .strong()（浅色模式下 strong_text_color=白色→隐形），改用显式主文本色
            ui.label(
                egui::RichText::new(i18n::t(i18n::Key::SubSectionChat, lang))
                    .color(ui.visuals().text_color())
                    .strong(),
            );

            // Jinja 对话模板引擎开关：标签 + ❓提示框 + 开关
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxJinja, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpJinja, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.jinja_enabled, "", accent);
                });
            });

            // 对话模板 (--chat-template)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelChatTemplate, lang));
                ui.text_edit_singleline(&mut settings.chat_template);
                ui.small(i18n::t(i18n::Key::HintChatTemplate, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpChatTemplate, lang));
            });

            // 对话模板文件 (--chat-template-file)
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelChatTemplateFile, lang));
                let mut file_str = settings.chat_template_file.to_string_lossy().to_string();
                let response = ui.text_edit_singleline(&mut file_str);
                if response.changed() {
                    settings.chat_template_file = std::path::PathBuf::from(&file_str);
                }
                if ui.button(i18n::t(i18n::Key::BtnBrowse, lang)).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(i18n::t(i18n::Key::DialogSelectChatTemplate, lang))
                        .add_filter(
                            i18n::t(i18n::Key::FilterTextFiles, lang),
                            &["txt", "jinja", "j2"],
                        )
                        .pick_file()
                    {
                        settings.chat_template_file = path;
                    }
                }
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpChatTemplateFile, lang));
            });
        },
    );

    // ── GPU 与设备分配 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionGpuDevice, lang),
        accent,
        |ui| {
            let mut gpu_layers = match settings.gpu_layers_mode {
                GpuLayersMode::Auto => 0usize,
                GpuLayersMode::All => 256usize,
                GpuLayersMode::Manual(n) => n,
            };

            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelGpuDevice, lang));
                let gm_labels = [
                    i18n::t(i18n::Key::GpuModeAuto, lang),
                    i18n::t(i18n::Key::GpuModeAll, lang),
                    i18n::t(i18n::Key::GpuModeManual, lang),
                ];
                let mut gm_idx = match settings.gpu_layers_mode {
                    GpuLayersMode::Auto => 0,
                    GpuLayersMode::All => 1,
                    GpuLayersMode::Manual(_) => 2,
                };
                widgets::segmented(ui, &gm_labels, &mut gm_idx, accent);
                match gm_idx {
                    0 => settings.gpu_layers_mode = GpuLayersMode::Auto,
                    1 => settings.gpu_layers_mode = GpuLayersMode::All,
                    2 => settings.gpu_layers_mode = GpuLayersMode::Manual(gpu_layers),
                    _ => {}
                }
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpGpuDevice, lang));
            });
            // 手动模式下显示层数输入
            if let GpuLayersMode::Manual(_) = settings.gpu_layers_mode {
                ui.indent("manual_gpu_layers_options", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelGpuDevice, lang));
                        ui.add(egui::DragValue::new(&mut gpu_layers).range(0..=256));
                        ui.small(i18n::t(i18n::Key::HintGpuDevice, lang));
                    });
                });
                settings.gpu_layers_mode = GpuLayersMode::Manual(gpu_layers);
            }
            // 延迟加载大型张量
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelTensorReadLazy, lang));
                let trl_vals = ["auto", "on", "off"];
                let trl_labels = [
                    i18n::t(i18n::Key::TensorReadLazyAuto, lang),
                    i18n::t(i18n::Key::TensorReadLazyOn, lang),
                    i18n::t(i18n::Key::TensorReadLazyOff, lang),
                ];
                let mut trl_idx = trl_vals
                    .iter()
                    .position(|v| *v == settings.tensor_read_lazy)
                    .unwrap_or(0);
                widgets::segmented(ui, &trl_labels, &mut trl_idx, accent);
                settings.tensor_read_lazy = trl_vals[trl_idx].to_string();
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTensorReadLazy, lang));
            });
            // 拆分模式
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelSplitMode, lang));
                let sm_vals = ["none", "layer", "tensor"];
                let sm_labels = [
                    i18n::t(i18n::Key::SplitModeNone, lang),
                    i18n::t(i18n::Key::SplitModeLayer, lang),
                    i18n::t(i18n::Key::SplitModeTensor, lang),
                ];
                let mut sm_idx = sm_vals
                    .iter()
                    .position(|v| *v == settings.split_mode)
                    .unwrap_or(0);
                widgets::segmented(ui, &sm_labels, &mut sm_idx, accent);
                settings.split_mode = sm_vals[sm_idx].to_string();
                ui.small(i18n::t(i18n::Key::HintSplitMode, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSplitMode, lang));
            });
            // 张量拆分比例
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelTensorSplit, lang));
                ui.text_edit_singleline(&mut settings.tensor_split);
                ui.small(i18n::t(i18n::Key::HintTensorSplit, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTensorSplit, lang));
            });
            // 张量拆分快捷参数按钮
            ui.indent("tensor_split_shortcuts", |ui| {
                ui.horizontal(|ui| {
                    let ratios = ["1,1", "1,2", "1,3", "1,4", "1,5", "1,6"];
                    let labels = ["1:1", "1:2", "1:3", "1:4", "1:5", "1:6"];
                    for (ratio, label) in ratios.iter().zip(labels.iter()) {
                        if ui
                            .add(widgets::rounded_button(label, Some(accent)))
                            .on_hover_text(*ratio)
                            .clicked()
                        {
                            settings.tensor_split = ratio.to_string();
                        }
                    }
                });
            });
            // CPU MoE（与 RPC 模式一致的缩进样式）
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxCpuMoe, lang));
                ui.small(i18n::t(i18n::Key::HintCpuMoe, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCpuMoe, lang));
                // ★ Toggle 新签名（行首已有标签，开关后不再重复文字）
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.cpu_moe, "", accent);
                });
            });
            // 关闭 cpu_moe 时才显示 n_cpu_moe 输入框
            if !settings.cpu_moe {
                ui.indent("cpu_moe_options", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelNCpuMoe, lang));
                        ui.add(egui::DragValue::new(&mut settings.n_cpu_moe).range(0..=256));
                        ui.small(i18n::t(i18n::Key::HintNCpuMoe, lang));
                    });
                });
            }
            // 指定特定张量到缓冲区
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelOverrideTensor, lang));
                ui.text_edit_singleline(&mut settings.override_tensor);
                ui.small(i18n::t(i18n::Key::HintOverrideTensor, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpOverrideTensor, lang));
            });
            // FFN / N-gram 卸载到CPU按钮（同行显示）
            ui.horizontal(|ui| {
                let ffn_set = settings.override_tensor == ".ffn_(gate|up|down).=CPU";
                if ui
                    .add_enabled(
                        !ffn_set,
                        egui::Button::new(i18n::t(i18n::Key::BtnFfnOffloadToCpu, lang)),
                    )
                    .clicked()
                {
                    settings.override_tensor = ".ffn_(gate|up|down).=CPU".to_string();
                }
                let ngram_set = settings
                    .override_tensor
                    .contains("per_layer_token_embd.weight=CPU");
                if ui
                    .add_enabled(
                        !ngram_set,
                        egui::Button::new(i18n::t(i18n::Key::BtnNGramOffloadToCpu, lang)),
                    )
                    .clicked()
                {
                    settings.override_tensor = "per_layer_token_embd.weight=CPU".to_string();
                }
                let tensor_empty = settings.override_tensor.is_empty();
                if ui
                    .add_enabled(
                        !tensor_empty,
                        egui::Button::new(i18n::t(i18n::Key::BtnClearOverrideTensor, lang)),
                    )
                    .clicked()
                {
                    settings.override_tensor.clear();
                }
            });
            // NUMA 模式
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelNuma, lang));
                let numa_vals = ["", "distribute", "isolate", "numactl"];
                let numa_labels = [
                    i18n::t(i18n::Key::LoadModeAuto, lang),
                    numa_vals[1],
                    numa_vals[2],
                    numa_vals[3],
                ];
                let mut numa_idx = numa_vals
                    .iter()
                    .position(|v| *v == settings.numa)
                    .unwrap_or(0);
                widgets::segmented(ui, &numa_labels, &mut numa_idx, accent);
                settings.numa = numa_vals[numa_idx].to_string();
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpNuma, lang));
            });
            // 主 GPU（多卡时指定）
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelMainGpu, lang));
                ui.add(egui::DragValue::new(&mut settings.main_gpu).range(0..=16));
                ui.small(i18n::t(i18n::Key::HintMainGpuFirst, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMainGpu, lang));
            });
            // 设备（多卡时指定）
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelServerDevice, lang));
                ui.text_edit_singleline(&mut settings.device);
                ui.small(i18n::t(i18n::Key::HintServerDevice, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpServerDevice, lang));
            });
            // 查看设备列表按钮
            let server_available = !settings.server_path.as_os_str().is_empty()
                && settings
                    .server_path
                    .file_name()
                    .map(|f| f.to_string_lossy().contains("llama-server"))
                    .unwrap_or(false);
            let btn = ui.add_enabled(
                server_available,
                egui::Button::new(i18n::t(i18n::Key::BtnViewServerDeviceList, lang)),
            );
            if btn.clicked() {
                if settings.show_server_device_list {
                    settings.show_server_device_list = false;
                } else {
                    // 执行 llama-server.exe --list-devices 获取设备列表
                    settings.server_device_list_output.clear();
                    let mut cmd = std::process::Command::new(&settings.server_path);
                    cmd.arg("--list-devices")
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped());
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x0800_0000u32);
                    }
                    match cmd.output() {
                        Ok(output) => {
                            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                            let raw = if !stdout.is_empty() {
                                stdout
                            } else if !stderr.is_empty() {
                                stderr
                            } else {
                                String::new()
                            };
                            // 只保留设备行：缩进且含 ":" 的行（如 "CUDA0: NVIDIA RTX 3090 (24576 MiB)"）
                            let devices: Vec<String> = raw
                                .lines()
                                .filter(|line| {
                                    line.starts_with(char::is_whitespace)
                                        && line.trim().contains(':')
                                        && !line.contains("Available devices")
                                })
                                .map(|line| line.trim().to_string())
                                .collect();
                            if devices.is_empty() {
                                settings.server_device_list_output =
                                    i18n::t(i18n::Key::HintServerDeviceListEmpty, lang).to_string();
                            } else {
                                settings.server_device_list_output = devices.join("\n");
                            }
                        }
                        Err(e) => {
                            settings.server_device_list_output = format!("执行失败: {}", e);
                        }
                    }
                    settings.show_server_device_list = true;
                }
            }
            // 设备列表输出区域
            if settings.show_server_device_list {
                ui.label(i18n::t(i18n::Key::LabelDeviceListTitle, lang));
                if settings.server_device_list_output.is_empty()
                    || settings.server_device_list_output
                        == i18n::t(i18n::Key::HintServerDeviceListEmpty, lang)
                {
                    ui.label(i18n::t(i18n::Key::HintServerDeviceListEmpty, lang));
                } else {
                    // 解析 RPC 节点列表，用于在设备后追加显示
                    let rpc_nodes: Vec<String> = settings
                        .rpc_endpoints
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    for line in settings.server_device_list_output.lines() {
                        if !line.is_empty() {
                            // 根据设备品牌设置圆点颜色
                            let dot_color = if line.contains("AMD") {
                                egui::Color32::from_rgb(220, 50, 50) // AMD 红色
                            } else if line.contains("NVIDIA") {
                                egui::Color32::from_rgb(50, 180, 50) // NVIDIA 绿色
                            } else if line.contains("Intel") || line.contains("INTEL") {
                                egui::Color32::from_rgb(50, 100, 220) // Intel 蓝色
                            } else {
                                accent // 其他使用主题色
                            };

                            // 显示设备行
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("●").color(dot_color).size(10.0));
                                ui.label(
                                    egui::RichText::new(line)
                                        .color(ui.visuals().text_color())
                                        .size(13.0),
                                );

                                // 添加至设备按钮（从格式 "CUDA0: ..." 提取设备 ID）
                                let device_entry =
                                    line.split(':').next().map(|s| s.trim().to_string());

                                if let Some(ref entry) = device_entry {
                                    let already_added =
                                        settings.device.split(',').any(|s| s.trim() == entry);
                                    let btn = ui.add_enabled(
                                        !already_added,
                                        egui::Button::new(i18n::t(i18n::Key::BtnAddToDevice, lang)),
                                    );
                                    if btn.clicked() {
                                        if settings.device.is_empty() {
                                            settings.device = entry.clone();
                                        } else {
                                            settings.device =
                                                format!("{},{}", settings.device, entry);
                                        }
                                    }
                                }
                            });
                        }
                    }

                    // 追加 RPC 节点列表（多卡节点展开为两个独立条目，紫色）
                    if !rpc_nodes.is_empty() {
                        let rpc_color = egui::Color32::from_rgb(180, 120, 255); // 紫色
                        let mut rpc_idx = 0usize;
                        for addr in rpc_nodes.iter() {
                            let is_multi_gpu = addr.ends_with('+');
                            let display_addr = addr.trim_end_matches('+');
                            // 多卡节点展开为两个条目
                            let count = if is_multi_gpu { 2 } else { 1 };
                            for offset in 0..count {
                                let idx = rpc_idx + offset;
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("●").color(rpc_color).size(10.0));
                                    let rpc_label = format!("RPC{}: {}", idx, display_addr);
                                    ui.label(
                                        egui::RichText::new(&rpc_label)
                                            .color(ui.visuals().text_color())
                                            .size(13.0),
                                    );
                                    // 多卡节点展开后的两个条目都显示 MULTI-GPU 标签
                                    if is_multi_gpu {
                                        let tag_color = egui::Color32::from_rgb(255, 165, 0);
                                        ui.label(
                                            egui::RichText::new("MULTI-GPU")
                                                .color(tag_color)
                                                .size(10.0),
                                        );
                                    }

                                    // 添加至设备按钮
                                    let rpc_entry = format!("RPC{}", idx);
                                    let already_added =
                                        settings.device.split(',').any(|s| s.trim() == rpc_entry);
                                    let btn = ui.add_enabled(
                                        !already_added,
                                        egui::Button::new(i18n::t(i18n::Key::BtnAddToDevice, lang)),
                                    );
                                    if btn.clicked() {
                                        if settings.device.is_empty() {
                                            settings.device = rpc_entry;
                                        } else {
                                            settings.device =
                                                format!("{},{}", settings.device, rpc_entry);
                                        }
                                    }
                                });
                            }
                            rpc_idx += count;
                        }
                    }
                }
            }
        },
    );

    // ── KV 缓存配置 ──
    widgets::card(ui, i18n::t(i18n::Key::SectionKvCache, lang), accent, |ui| {
        // 模型加载模式（--load-mode；auto 不拼接参数）
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelLoadMode, lang));
            let lm_vals = ["auto", "none", "mmap", "mlock", "mmap+mlock", "dio"];
            let lm_labels = [
                i18n::t(i18n::Key::LoadModeAuto, lang),
                i18n::t(i18n::Key::LoadModeNone, lang),
                lm_vals[2],
                lm_vals[3],
                lm_vals[4],
                lm_vals[5],
            ];
            let mut lm_idx = lm_vals
                .iter()
                .position(|v| *v == settings.load_mode)
                .unwrap_or(0);
            widgets::segmented(ui, &lm_labels, &mut lm_idx, accent);
            settings.load_mode = lm_vals[lm_idx].to_string();
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpLoadMode, lang));
        });
        // 长上下文 / 提示缓存（标签 + ❓提示框 + 行右侧开关）
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::CheckboxCachePrompt, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCachePrompt, lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::toggle(ui, &mut settings.cache_prompt, "", accent);
            });
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelCacheReuse, lang));
            ui.add(
                egui::DragValue::new(&mut settings.cache_reuse)
                    .range(0..=65536)
                    .speed(64),
            );
            ui.small(i18n::t(i18n::Key::HintCacheReuseDisabled, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCacheReuse, lang));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::CheckboxContextShift, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpContextShift, lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::toggle(ui, &mut settings.context_shift, "", accent);
            });
        });

        // KV 缓存开关统一样式（与「手动指定 GPU 层数」一致）：标签 + ❓提示框 + 开关
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::CheckboxKvOffload, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpKvOffload, lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::toggle(ui, &mut settings.kv_offload, "", accent);
            });
        });

        // K 缓存类型：标签 + ❓提示框同一行
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelCacheTypeK, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCacheTypeK, lang));
        });
        let k_types = [
            "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1", "turbo2",
            "turbo3", "turbo4",
        ];

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            for k_type in &k_types {
                let selected = settings.cache_type_k == *k_type;
                if ui.selectable_label(selected, *k_type).clicked() {
                    settings.cache_type_k = k_type.to_string();
                }
            }
        });

        // V 缓存类型：标签 + ❓提示框同一行
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelCacheTypeV, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCacheTypeV, lang));
        });
        let v_types = [
            "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1", "turbo2",
            "turbo3", "turbo4",
        ];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            for v_type in &v_types {
                let selected = settings.cache_type_v == *v_type;
                if ui.selectable_label(selected, *v_type).clicked() {
                    settings.cache_type_v = v_type.to_string();
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::CheckboxKvUnified, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpKvUnified, lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::toggle(ui, &mut settings.kv_unified, "", accent);
            });
        });

        // 完整滑动窗口 (--swa-full)，与「手动指定 GPU 层数」一致：标签 + ❓提示框 + 开关
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::CheckboxSwaFull, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSwaFull, lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::toggle(ui, &mut settings.swa_full, "", accent);
            });
        });
        // 上下文检查点
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelCtxCheckpoints, lang));
            ui.add(
                egui::DragValue::new(&mut settings.ctx_checkpoints)
                    .range(1..=256)
                    .speed(1),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCtxCheckpoints, lang));
        });
        // 最小检查点步长
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelCheckpointMinStep, lang));
            ui.add(
                egui::DragValue::new(&mut settings.checkpoint_min_step)
                    .range(64..=4096)
                    .speed(64),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpCheckpointMinStep, lang));
        });
    });

    // ── 推测解码 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionSpecDecoding, lang),
        accent,
        |ui| {
            // 算法类型：标签 + ❓提示框同一行
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::SpecTypeLabel, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecType, lang));
            });

            let spec_options = [
                ("none", i18n::t(i18n::Key::SpecTypeNone, lang)),
                (
                    "draft-simple",
                    i18n::t(i18n::Key::SpecTypeDraftSimple, lang),
                ),
                (
                    "draft-eagle3",
                    i18n::t(i18n::Key::SpecTypeDraftEagle3, lang),
                ),
                ("draft-mtp", i18n::t(i18n::Key::SpecTypeDraftMtp, lang)),
                (
                    "draft-dflash",
                    i18n::t(i18n::Key::SpecTypeDraftDflash, lang),
                ),
                (
                    "draft-dspark",
                    i18n::t(i18n::Key::SpecTypeDraftDspark, lang),
                ),
                (
                    "ngram-simple",
                    i18n::t(i18n::Key::SpecTypeNgramSimple, lang),
                ),
                ("ngram-map-k", i18n::t(i18n::Key::SpecTypeNgramMapK, lang)),
                (
                    "ngram-map-k4v",
                    i18n::t(i18n::Key::SpecTypeNgramMapK4V, lang),
                ),
                ("ngram-mod", i18n::t(i18n::Key::SpecTypeNgramMod, lang)),
                ("ngram-cache", i18n::t(i18n::Key::SpecTypeNgramCache, lang)),
            ];

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                for (value, label) in &spec_options {
                    let selected = settings.spec_type == *value;
                    if ui.selectable_label(selected, *label).clicked() {
                        settings.spec_type = value.to_string();
                    }
                }
            });
            // draft-* 算法参数（draft-simple / draft-eagle3 / draft-mtp / draft-dflash / draft-dspark）
            let is_draft = settings.spec_type.starts_with("draft-");
            if is_draft {
                // 最大推测数量 --spec-draft-n-max（DragValue + 启用开关）
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecDraftNMaxLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_draft_n_max).range(0..=64));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecDraftNMax, lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::toggle(ui, &mut settings.enable_spec_draft_n_max, "", accent);
                    });
                });
                // 最小推测数量 --spec-draft-n-min（DragValue + 启用开关）
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecDraftNMinLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_draft_n_min).range(0..=32));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecDraftNMin, lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::toggle(ui, &mut settings.enable_spec_draft_n_min, "", accent);
                    });
                });
                // 信任度 --spec-draft-p-min（Slider + 启用开关）
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecDraftPMinLabel, lang));
                    ui.add(
                        egui::Slider::new(&mut settings.spec_draft_p_min, 0.0..=1.0)
                            .smallest_positive(0.01)
                            .custom_formatter(|v, _| format!("{:.2}", v)),
                    );
                    ui.label(format!("{:.2}", settings.spec_draft_p_min));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecDraftPMin, lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::toggle(ui, &mut settings.enable_spec_draft_p_min, "", accent);
                    });
                });
                // 分裂概率 --spec-draft-p-split（Slider + 启用开关）
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecDraftPSplitLabel, lang));
                    ui.add(
                        egui::Slider::new(&mut settings.spec_draft_p_split, 0.0..=1.0)
                            .smallest_positive(0.01)
                            .custom_formatter(|v, _| format!("{:.2}", v)),
                    );
                    ui.label(format!("{:.2}", settings.spec_draft_p_split));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecDraftPSplit, lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::toggle(ui, &mut settings.enable_spec_draft_p_split, "", accent);
                    });
                });
                // 推测解码 KV 类型 K + 启用开关
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::LabelSpecDraftTypeK, lang));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecDraftTypeK, lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::toggle(ui, &mut settings.enable_spec_draft_type_k, "", accent);
                    });
                });
                let spec_k_types = [
                    "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1",
                ];
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for k_type in &spec_k_types {
                        let selected = settings.spec_draft_type_k == *k_type;
                        if ui.selectable_label(selected, *k_type).clicked() {
                            settings.spec_draft_type_k = k_type.to_string();
                        }
                    }
                });
                // 推测解码 KV 类型 V + 启用开关
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::LabelSpecDraftTypeV, lang));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecDraftTypeV, lang));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        widgets::toggle(ui, &mut settings.enable_spec_draft_type_v, "", accent);
                    });
                });
                let spec_v_types = [
                    "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1",
                ];
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for v_type in &spec_v_types {
                        let selected = settings.spec_draft_type_v == *v_type;
                        if ui.selectable_label(selected, *v_type).clicked() {
                            settings.spec_draft_type_v = v_type.to_string();
                        }
                    }
                });
            }
            // ngram-simple / ngram-map-k / ngram-map-k4v 共用参数（size-n / size-m / min-hits）
            let is_ngram_shared = matches!(
                settings.spec_type.as_str(),
                "ngram-simple" | "ngram-map-k" | "ngram-map-k4v"
            );
            if is_ngram_shared {
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecNgramSizeNLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_ngram_size_n).range(1..=256));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecNgramSizeN, lang));
                });
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecNgramSizeMLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_ngram_size_m).range(1..=256));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecNgramSizeM, lang));
                });
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecNgramMinHitsLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_ngram_min_hits).range(1..=128));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecNgramMinHits, lang));
                });
            }
            // ngram-mod 专用参数（n-min / n-max / n-match）
            if settings.spec_type == "ngram-mod" {
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecNgramModNMinLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_ngram_mod_n_min).range(1..=256));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecNgramModNMin, lang));
                });
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecNgramModNMaxLabel, lang));
                    ui.add(egui::DragValue::new(&mut settings.spec_ngram_mod_n_max).range(1..=256));
                    helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSpecNgramModNMax, lang));
                });
                ui.horizontal(|ui| {
                    ui.label(i18n::t(i18n::Key::SpecNgramModNMatchLabel, lang));
                    ui.add(
                        egui::DragValue::new(&mut settings.spec_ngram_mod_n_match).range(1..=128),
                    );
                    helper::help_button_inline(
                        ui,
                        i18n::t(i18n::Key::HelpSpecNgramModNMatch, lang),
                    );
                });
            }
        },
    );

    // ── 多模态 ──
    widgets::card(ui, i18n::t(i18n::Key::Multimodal, lang), accent, |ui| {
        // 多模态投影文件 --mmproj-auto / --no-mmproj
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelMmprojAuto, lang));
            let mm_vals = ["auto", "off"];
            let mm_labels = [
                i18n::t(i18n::Key::MmprojAuto, lang),
                i18n::t(i18n::Key::MmprojOff, lang),
            ];
            let mut mm_idx = if settings.mmproj_auto { 0 } else { 1 };
            widgets::segmented(ui, &mm_labels, &mut mm_idx, accent);
            settings.mmproj_auto = mm_vals[mm_idx] == "auto";
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMmprojAuto, lang));
        });
        // 投影 GPU 卸载 --mmproj-offload / --no-mmproj-offload
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelMmprojOffload, lang));
            let offload_vals = [true, false];
            let offload_labels = [
                i18n::t(i18n::Key::MmprojOffloadOn, lang),
                i18n::t(i18n::Key::MmprojOffloadOff, lang),
            ];
            let mut offload_idx = if settings.mmproj_offload { 0 } else { 1 };
            widgets::segmented(ui, &offload_labels, &mut offload_idx, accent);
            settings.mmproj_offload = offload_vals[offload_idx];
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMmprojOffload, lang));
        });
        // 投影设备 --mmproj-device
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelMmprojDevice, lang));
            ui.add(egui::TextEdit::singleline(&mut settings.mmproj_device).desired_width(120.0));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMmprojDevice, lang));
        });
        ui.separator();
        // 图片最小 Token --image-min-tokens
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelImageMinTokens, lang));
            ui.add(
                egui::DragValue::new(&mut settings.image_min_tokens)
                    .range(0..=32768)
                    .speed(1),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpImageTokens, lang));
        });
        // 图片最大 Token --image-max-tokens
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelImageMaxTokens, lang));
            ui.add(
                egui::DragValue::new(&mut settings.image_max_tokens)
                    .range(0..=32768)
                    .speed(1),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpImageTokens, lang));
        });
        // 批次最大 Token --mtmd-batch-max-tokens
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelMtmdBatchMaxTokens, lang));
            ui.add(
                egui::DragValue::new(&mut settings.mtmd_batch_max_tokens)
                    .range(1..=32768)
                    .speed(1),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMtmdBatch, lang));
        });
        ui.separator();
        // 视频帧率 --video-fps
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelVideoFps, lang));
            ui.add(
                egui::DragValue::new(&mut settings.video_fps)
                    .range(0.1..=60.0)
                    .speed(0.1)
                    .fixed_decimals(1),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpVideoFps, lang));
        });
        // 时间戳间隔 --video-timestamp-interval
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelVideoTimestampInterval, lang));
            ui.add(
                egui::DragValue::new(&mut settings.video_timestamp_interval)
                    .range(0..=60000)
                    .speed(100),
            );
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpVideoTimestamp, lang));
        });
        // FFmpeg 目录 --video-ffmpeg-dir
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelVideoFfmpegDir, lang));
            ui.add(egui::TextEdit::singleline(&mut settings.video_ffmpeg_dir).desired_width(200.0));
            if ui.button(i18n::t(i18n::Key::BtnBrowse, lang)).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Select ffmpeg directory")
                    .pick_folder()
                {
                    settings.video_ffmpeg_dir = path.to_string_lossy().to_string();
                }
            }
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpVideoFfmpegDir, lang));
        });
    });

    // ── 线程与生成长度 ──
    widgets::card(ui, i18n::t(i18n::Key::SectionThreads, lang), accent, |ui| {
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelThreads, lang));
            ui.add(egui::DragValue::new(&mut settings.threads).range(-1..=256));
            ui.small(i18n::t(i18n::Key::HintThreadsDefault, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpThreads, lang));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelThreadsBatch, lang));
            ui.add(egui::DragValue::new(&mut settings.threads_batch).range(-1..=256));
            ui.small(i18n::t(i18n::Key::HintThreadsBatchDefault, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpThreadsBatch, lang));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelNPredict, lang));
            ui.add(
                egui::DragValue::new(&mut settings.n_predict)
                    .range(-1..=65536)
                    .speed(128),
            );
            ui.small(i18n::t(i18n::Key::HintNPredictLimit, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpNPredict, lang));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelKeep, lang));
            ui.add(
                egui::DragValue::new(&mut settings.keep)
                    .range(0..=8192)
                    .speed(16),
            );
            ui.small(i18n::t(i18n::Key::HintKeepNone, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpKeep, lang));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelSeed, lang));
            ui.add(
                egui::DragValue::new(&mut settings.seed)
                    .range(-1..=i32::MAX as i64)
                    .speed(1),
            );
            ui.small(i18n::t(i18n::Key::HintSeedRandom, lang));
            helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSeed, lang));
        });
    });

    // ── 采样参数 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionSampling, lang),
        accent,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelTemperature, lang));
                ui.add(
                    egui::Slider::new(&mut settings.temperature, 0.0..=2.0)
                        .smallest_positive(0.01)
                        .custom_formatter(|v, _| format!("{:.2}", v)),
                );
                ui.label(format!("{:.2}", settings.temperature));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTemperature, lang));
                // 开关推到行最右侧（与顶栏 right_to_left 右对齐模式一致）
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_temperature, "", accent);
                });
            });
            // top_p
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelTopP, lang));
                ui.add(
                    egui::Slider::new(&mut settings.top_p, 0.0..=1.0)
                        .smallest_positive(0.01)
                        .custom_formatter(|v, _| format!("{:.2}", v)),
                );
                ui.label(format!("{:.2}", settings.top_p));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTopP, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_top_p, "", accent);
                });
            });
            // top_k
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelTopK, lang));
                ui.add(egui::DragValue::new(&mut settings.top_k).range(0..=1000));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTopK, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_top_k, "", accent);
                });
            });
            // 重复惩罚
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelRepeatPenalty, lang));
                ui.add(
                    egui::Slider::new(&mut settings.repeat_penalty, 0.0..=2.0)
                        .smallest_positive(0.01)
                        .custom_formatter(|v, _| format!("{:.2}", v)),
                );
                ui.label(format!("{:.2}", settings.repeat_penalty));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpRepeatPenalty, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_repeat_penalty, "", accent);
                });
            });
            // 存在惩罚
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelPresencePenalty, lang));
                ui.add(
                    egui::Slider::new(&mut settings.presence_penalty, -2.0..=2.0)
                        .smallest_positive(0.01)
                        .custom_formatter(|v, _| format!("{:.2}", v)),
                );
                ui.label(format!("{:.2}", settings.presence_penalty));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpPresencePenalty, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_presence_penalty, "", accent);
                });
            });
        },
    );

    // ── 采样器扩展 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionSamplers, lang),
        accent,
        |ui| {
            // Min-P
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxMinP, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMinP, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_min_p, "", accent);
                });
            });
            if settings.enable_min_p {
                ui.indent("min_p_opt", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelMinP, lang));
                        ui.add(
                            egui::Slider::new(&mut settings.min_p, 0.0..=1.0)
                                .smallest_positive(0.01)
                                .custom_formatter(|v, _| format!("{:.3}", v)),
                        );
                        ui.label(format!("{:.3}", settings.min_p));
                    });
                });
            }
            // Top-N-Sigma
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxTopNSigma, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTopNSigma, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_top_n_sigma, "", accent);
                });
            });
            if settings.enable_top_n_sigma {
                ui.indent("top_n_sigma_opt", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelTopNSigma, lang));
                        ui.add(
                            egui::Slider::new(&mut settings.top_n_sigma, 0.0..=3.0)
                                .smallest_positive(0.01)
                                .custom_formatter(|v, _| format!("{:.2}", v)),
                        );
                        ui.label(format!("{:.2}", settings.top_n_sigma));
                    });
                });
            }
            // XTC
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxXtc, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpXtc, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_xtc, "", accent);
                });
            });
            if settings.enable_xtc {
                ui.indent("xtc_opt", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelXtcProbability, lang));
                        ui.add(
                            egui::Slider::new(&mut settings.xtc_probability, 0.0..=1.0)
                                .smallest_positive(0.01)
                                .custom_formatter(|v, _| format!("{:.2}", v)),
                        );
                        ui.label(format!("{:.2}", settings.xtc_probability));
                    });
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelXtcThreshold, lang));
                        ui.add(
                            egui::Slider::new(&mut settings.xtc_threshold, 0.0..=1.0)
                                .smallest_positive(0.01)
                                .custom_formatter(|v, _| format!("{:.2}", v)),
                        );
                        ui.label(format!("{:.2}", settings.xtc_threshold));
                    });
                });
            }
            // Typical-P
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxTypicalP, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpTypicalP, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_typical_p, "", accent);
                });
            });
            if settings.enable_typical_p {
                ui.indent("typical_p_opt", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelTypicalP, lang));
                        ui.add(
                            egui::Slider::new(&mut settings.typical_p, 0.0..=1.0)
                                .smallest_positive(0.01)
                                .custom_formatter(|v, _| format!("{:.2}", v)),
                        );
                        ui.label(format!("{:.2}", settings.typical_p));
                    });
                });
            }
            // Mirostat
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxMirostat, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpMirostat, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_mirostat, "", accent);
                });
            });
            if settings.enable_mirostat {
                ui.indent("mirostat_opt", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelMirostat, lang));
                        let m_vals = [0, 1, 2];
                        let m_labels = ["0 = 关", "1", "2"];
                        let mut m_idx = m_vals
                            .iter()
                            .position(|v| *v == settings.mirostat)
                            .unwrap_or(0);
                        widgets::segmented(ui, &m_labels, &mut m_idx, accent);
                        settings.mirostat = m_vals[m_idx];
                    });
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelMirostatLr, lang));
                        ui.add(
                            egui::DragValue::new(&mut settings.mirostat_lr)
                                .range(0.0..=1.0)
                                .speed(0.01),
                        );
                        ui.label(format!("{:.2}", settings.mirostat_lr));
                    });
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelMirostatEnt, lang));
                        ui.add(
                            egui::DragValue::new(&mut settings.mirostat_ent)
                                .range(0.0..=20.0)
                                .speed(0.1),
                        );
                        ui.label(format!("{:.2}", settings.mirostat_ent));
                    });
                });
            }
            // 动态温度
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::CheckboxDynatemp, lang));
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpDynatemp, lang));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::toggle(ui, &mut settings.enable_dynatemp, "", accent);
                });
            });
            if settings.enable_dynatemp {
                ui.indent("dynatemp_opt", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelDynatempRange, lang));
                        ui.add(
                            egui::DragValue::new(&mut settings.dynatemp_range)
                                .range(0.0..=1.0)
                                .speed(0.05),
                        );
                        ui.label(format!("{:.2}", settings.dynatemp_range));
                    });
                    ui.horizontal(|ui| {
                        ui.label(i18n::t(i18n::Key::LabelDynatempExp, lang));
                        ui.add(
                            egui::DragValue::new(&mut settings.dynatemp_exp)
                                .range(0.0..=2.0)
                                .speed(0.05),
                        );
                        ui.label(format!("{:.2}", settings.dynatemp_exp));
                    });
                });
            }
            // 采样器序列
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelSamplerSeq, lang));
                ui.text_edit_singleline(&mut settings.sampler_seq);
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpSamplerSeq, lang));
            });
        },
    );

    // ── 结构化输出 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionStructuredOutput, lang),
        accent,
        |ui| {
            // JSON Schema
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelJsonSchema, lang));
                ui.text_edit_singleline(&mut settings.json_schema);
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpJsonSchema, lang));
            });
            // Grammar
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelGrammar, lang));
                ui.text_edit_singleline(&mut settings.grammar);
                helper::help_button_inline(ui, i18n::t(i18n::Key::HelpGrammar, lang));
            });
        },
    );
}
