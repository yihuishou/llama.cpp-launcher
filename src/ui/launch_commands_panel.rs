use crate::engine::rpc::RpcManager;
use crate::engine::server::ServerManager;
use crate::i18n;
use crate::ui::widgets;
use fluent_egui::winui;

/// 启动命令后处理：exe 路径之后，每 6 个逻辑参数以 " /" 换行，末行不补 "/"。
/// 逻辑参数 = 以 "--" 开头的 flag 及其后续值 token 整体（如 `--model a.gguf` 算 1 个参数）；
/// 含空格的值 token 不触发新参数，始终归入当前参数，因此 flag/值成对不被拆散。
fn wrap_launch_command(cmd: &str) -> String {
    let mut tokens = cmd.split_whitespace();
    let Some(exe) = tokens.next() else {
        return String::new();
    };
    let mut args: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    for tok in tokens {
        if tok.starts_with("--") {
            if let Some(c) = cur.take() {
                args.push(c);
            }
            cur = Some(tok.to_string());
        } else if let Some(c) = cur.as_mut() {
            c.push(' ');
            c.push_str(tok);
        }
    }
    if let Some(c) = cur {
        args.push(c);
    }
    let groups = args.chunks(6).count().max(1);
    let mut out = String::from(exe);
    for (i, chunk) in args.chunks(6).enumerate() {
        if i > 0 {
            out.push('\n');
        } else {
            out.push(' ');
        }
        out.push_str(&chunk.join(" "));
        if i + 1 < groups {
            out.push_str(" /");
        }
    }
    out
}

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
                let wrapped = wrap_launch_command(cmd);
                ui.monospace(&wrapped);
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnCopyToClipboard,
                        lang,
                    )))
                    .clicked()
                {
                    ui.ctx().copy_text(wrapped.clone());
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
                let wrapped = wrap_launch_command(cmd);
                ui.monospace(&wrapped);
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnCopyToClipboard,
                        lang,
                    )))
                    .clicked()
                {
                    ui.ctx().copy_text(wrapped.clone());
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

#[cfg(test)]
mod tests {
    use super::wrap_launch_command;

    /// 空串 → 空串
    #[test]
    fn wrap_empty() {
        assert_eq!(wrap_launch_command(""), "");
    }

    /// 仅 exe 无参数 → 原样返回
    #[test]
    fn wrap_exe_only() {
        assert_eq!(wrap_launch_command("llama-server.exe"), "llama-server.exe");
    }

    /// 参数不足 6 个 → 不换行
    #[test]
    fn wrap_fewer_than_six_args() {
        assert_eq!(
            wrap_launch_command("exe --a 1 --b 2 --c 3 --d 4 --e 5"),
            "exe --a 1 --b 2 --c 3 --d 4 --e 5"
        );
    }

    /// 恰好 6 个参数 → 单行不换行（末行不补 /）
    #[test]
    fn wrap_exactly_six_args_no_trailing_slash() {
        assert_eq!(
            wrap_launch_command("exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6"),
            "exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6"
        );
    }

    /// 7 个参数 → 前 6 个后换行
    #[test]
    fn wrap_seven_args_breaks_after_six() {
        assert_eq!(
            wrap_launch_command("exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6 --g 7"),
            "exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6 /\n--g 7"
        );
    }

    /// 12 个参数 → 两组各 6 个，末行无 /
    #[test]
    fn wrap_twelve_args_two_groups() {
        assert_eq!(
            wrap_launch_command(
                "exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6 --g 7 --h 8 --i 9 --j 10 --k 11 --l 12"
            ),
            "exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6 /\n--g 7 --h 8 --i 9 --j 10 --k 11 --l 12"
        );
    }

    /// 13 个参数 → 三组，仅前两行带 /
    #[test]
    fn wrap_thirteen_args_three_groups() {
        assert_eq!(
            wrap_launch_command("exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6 --g 7 --h 8 --i 9 --j 10 --k 11 --l 12 --m 13"),
            "exe --a 1 --b 2 --c 3 --d 4 --e 5 --f 6 /\n--g 7 --h 8 --i 9 --j 10 --k 11 --l 12 /\n--m 13"
        );
    }
}
