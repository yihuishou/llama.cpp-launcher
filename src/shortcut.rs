use std::path::PathBuf;

/// 根据系统语言获取快捷方式名称（国际化）
fn get_shortcut_name() -> &'static str {
    // 检测系统 locale：中文 → llama.cpp 启动器，其余默认英文
    let locale = sys_locale::get_locale().unwrap_or_default();
    if locale.starts_with("zh") {
        "llama.cpp 启动器"
    } else {
        "llama.cpp launcher"
    }
}

/// 在用户桌面创建指向 llama-lunch.exe 的 .lnk 快捷方式
#[cfg(target_os = "windows")]
pub fn create_desktop_shortcut() -> Result<(), String> {
    // 1. 获取 exe 路径
    let exe_path = std::env::current_exe().map_err(|e| format!("获取 exe 路径失败：{}", e))?;

    // 2. 获取桌面路径
    let desktop_dir = dirs::desktop_dir().ok_or_else(|| "无法获取桌面路径".to_string())?;

    // 3. 构造快捷方式目标路径（国际化名称）
    let name = get_shortcut_name();
    let shortcut_path: PathBuf = desktop_dir.join(format!("{}.lnk", name));

    // 4. 创建 ShellLink 快捷方式（自动设置工作目录为 exe 所在目录）
    let link = shortcuts_rs::ShellLink::new(&exe_path, None, None, None)
        .map_err(|e| format!("创建快捷方式对象失败：{}", e))?;
    link.create_lnk(&shortcut_path)
        .map_err(|e| format!("创建快捷方式失败：{}", e))?;

    Ok(())
}

/// 在用户桌面创建 .desktop 快捷方式（Linux）
#[cfg(target_os = "linux")]
pub fn create_desktop_shortcut() -> Result<(), String> {
    use std::fs;

    let exe_path = std::env::current_exe().map_err(|e| format!("获取 exe 路径失败：{}", e))?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| "无法获取 exe 所在目录".to_string())?;

    let desktop_dir = dirs::desktop_dir().ok_or_else(|| "无法获取桌面路径".to_string())?;

    let name = get_shortcut_name();
    let shortcut_path: PathBuf = desktop_dir.join(format!("{}.desktop", name));
    let icon_path = release_icon(exe_dir)?;

    let desktop_content = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nTerminal=false\n",
        name,
        desktop_quote(&exe_path),
        desktop_quote(&icon_path)
    );

    fs::write(&shortcut_path, &desktop_content)
        .map_err(|e| format!("创建 .desktop 文件失败：{}", e))?;
    set_desktop_mode(&shortcut_path).map_err(|e| format!("设置文件权限失败：{}", e))?;
    mark_trusted(&shortcut_path);

    Ok(())
}

/// Linux 启动自愈：把快捷方式与开机自启里的可执行文件路径修正到当前程序。
///
/// 发行包内的二进制名曾带版本号（`llama_cpp_launcher_v0.2.2_linux_x86_64`），
/// 升级后旧 `.desktop` 的 `Exec=` 会指向已不存在的文件；系统语言变化还会留下
/// 中/英两份快捷方式。这里幂等地重写 Exec/Icon、迁移文件名并同步 autostart。
#[cfg(target_os = "linux")]
pub fn ensure_linux_integration() {
    let Ok(exe_path) = std::env::current_exe() else {
        return;
    };
    let Some(exe_dir) = exe_path.parent() else {
        return;
    };
    let icon_path = release_icon(exe_dir).ok();
    if icon_path.is_none() {
        log::warn!("[shortcut] 释放图标失败，仅修正快捷方式路径");
    }

    let mut dirs = Vec::new();
    if let Some(desktop_dir) = dirs::desktop_dir() {
        dirs.push(desktop_dir);
    }
    if let Some(data_dir) = dirs::data_dir() {
        dirs.push(data_dir.join("applications"));
    }
    for dir in dirs.iter().filter(|d| d.is_dir()) {
        ensure_desktop_dir(dir, &exe_path, icon_path.as_deref());
    }

    ensure_autostart(&exe_path);
}

#[cfg(not(target_os = "linux"))]
pub fn ensure_linux_integration() {}

#[cfg(target_os = "linux")]
const SHORTCUT_NAMES: [&str; 2] = ["llama.cpp 启动器", "llama.cpp launcher"];

#[cfg(target_os = "linux")]
const ICON_FILENAME: &str = "llama-cpp-launcher.png";

#[cfg(target_os = "linux")]
fn release_icon(exe_dir: &std::path::Path) -> Result<PathBuf, String> {
    use std::fs;

    let icon_path = exe_dir.join(ICON_FILENAME);
    if !icon_path.exists() {
        const ICON_BYTES: &[u8] = include_bytes!("../llama-cpp-launcher.png");
        fs::write(&icon_path, ICON_BYTES).map_err(|e| format!("释放图标文件失败：{}", e))?;
    }
    Ok(icon_path)
}

/// .desktop 规范要求含空格的路径用双引号包裹。
#[cfg(target_os = "linux")]
fn desktop_quote(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value)
    } else {
        value
    }
}

#[cfg(target_os = "linux")]
fn set_desktop_mode(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

/// GNOME 桌面需要标记可信，否则双击只会打开文本编辑器（best-effort）。
#[cfg(target_os = "linux")]
fn mark_trusted(path: &std::path::Path) {
    use std::process::{Command, Stdio};

    let _ = Command::new("gio")
        .arg("set")
        .arg(path)
        .arg("metadata::trusted")
        .arg("true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// 解析 `Exec=` 行，返回（可执行文件路径, 其余参数）。
#[cfg(target_os = "linux")]
fn parse_exec(content: &str) -> Option<(PathBuf, String)> {
    let line = content
        .lines()
        .find(|l| l.trim_start().starts_with("Exec="))?;
    let value = line.trim_start().trim_start_matches("Exec=").trim();
    let (exec, rest) = match value.strip_prefix('"') {
        Some(quoted) => quoted
            .split_once('"')
            .map(|(exec, rest)| (exec, rest.trim_start()))
            .unwrap_or((quoted, "")),
        None => value
            .split_once(char::is_whitespace)
            .map(|(exec, rest)| (exec, rest.trim_start()))
            .unwrap_or((value, "")),
    };
    Some((PathBuf::from(exec), rest.to_string()))
}

#[cfg(target_os = "linux")]
fn is_launcher_binary(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("llama_cpp_launcher") || n.starts_with("llama-cpp-launcher"))
}

/// 历史发行包里带版本号的二进制名（`llama_cpp_launcher_v0.2.2_linux_x86_64`）。
#[cfg(target_os = "linux")]
fn is_legacy_binary(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("llama_cpp_launcher_v"))
}

/// 替换指定键（重复键丢弃），不存在则追加。
#[cfg(target_os = "linux")]
fn set_entry_line(content: &str, key: &str, value: &str) -> String {
    let mut out = String::with_capacity(content.len() + value.len() + 8);
    let mut replaced = false;
    for line in content.lines() {
        if line.trim_start().starts_with(&format!("{}=", key)) {
            if !replaced {
                out.push_str(&format!("{}={}\n", key, value));
                replaced = true;
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&format!("{}={}\n", key, value));
    }
    out
}

#[cfg(target_os = "linux")]
fn same_exec(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// 重写单个条目的 Exec/Icon；返回是否实际发生修改。
#[cfg(target_os = "linux")]
fn refresh_entry(
    path: &std::path::Path,
    exe: &std::path::Path,
    icon: Option<&std::path::Path>,
) -> std::io::Result<bool> {
    let content = std::fs::read_to_string(path)?;
    let Some((exec_path, rest)) = parse_exec(&content) else {
        return Ok(false);
    };
    // 只修已失效的路径与历史带版本号的文件名，避免抢占正常的另一份安装。
    if exec_path.exists() && !same_exec(&exec_path, exe) && !is_legacy_binary(&exec_path) {
        return Ok(false);
    }

    let mut exec_value = desktop_quote(exe);
    if !rest.is_empty() {
        exec_value.push(' ');
        exec_value.push_str(&rest);
    }
    let mut updated = set_entry_line(&content, "Exec", &exec_value);
    if let Some(icon) = icon {
        updated = set_entry_line(&updated, "Icon", &desktop_quote(icon));
    }
    if updated == content {
        return Ok(false);
    }

    std::fs::write(path, updated)?;
    let _ = set_desktop_mode(path);
    mark_trusted(path);
    Ok(true)
}

/// 目录内：找本程序的条目 → 迁移/去重 locale 文件名 → 刷新 Exec/Icon。
#[cfg(target_os = "linux")]
fn ensure_desktop_dir(
    dir: &std::path::Path,
    exe: &std::path::Path,
    icon: Option<&std::path::Path>,
) {
    use std::fs;

    let canonical_name = get_shortcut_name();
    let canonical_path = dir.join(format!("{}.desktop", canonical_name));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut ours = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let exec_is_ours = parse_exec(&content).is_some_and(|(p, _)| is_launcher_binary(&p));
        let name_is_ours = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| SHORTCUT_NAMES.contains(&s));
        if exec_is_ours || name_is_ours {
            ours.push(path);
        }
    }

    for path in &ours {
        let known_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| SHORTCUT_NAMES.contains(&s));
        if path == &canonical_path || !known_name {
            continue;
        }
        if !canonical_path.exists() {
            match fs::rename(path, &canonical_path) {
                Ok(()) => log::info!(
                    "[shortcut] 快捷方式已迁移: {} -> {}",
                    path.display(),
                    canonical_path.display()
                ),
                Err(e) => log::warn!("[shortcut] 迁移快捷方式失败: {}", e),
            }
        } else {
            let _ = fs::remove_file(path);
            log::info!("[shortcut] 已删除重复快捷方式: {}", path.display());
        }
    }

    let mut changed = false;
    for path in ours.iter().chain(std::iter::once(&canonical_path)) {
        if !path.is_file() {
            continue;
        }
        match refresh_entry(path, exe, icon) {
            Ok(true) => {
                changed = true;
                log::info!("[shortcut] 已修正快捷方式路径: {}", path.display());
            }
            Ok(false) => {}
            Err(e) => log::warn!("[shortcut] 更新快捷方式失败: {}: {}", path.display(), e),
        }
    }

    if changed {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// XDG autostart 同样存绝对路径，升级后需要同步（保留 `--minimized` 等参数）。
#[cfg(target_os = "linux")]
fn ensure_autostart(exe: &std::path::Path) {
    let Some(path) = dirs::config_dir().map(|d| d.join("autostart/llama-cpp-launcher.desktop"))
    else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let Some((exec_path, rest)) = parse_exec(&content) else {
        return;
    };
    if !is_launcher_binary(&exec_path) || same_exec(&exec_path, exe) {
        return;
    }
    if exec_path.exists() && !is_legacy_binary(&exec_path) {
        return;
    }

    let mut exec_value = desktop_quote(exe);
    if !rest.is_empty() {
        exec_value.push(' ');
        exec_value.push_str(&rest);
    }
    if std::fs::write(&path, set_entry_line(&content, "Exec", &exec_value)).is_ok() {
        log::info!("[shortcut] 已更新开机自启路径: {}", path.display());
    }
}

/// 在用户桌面创建指向可执行文件的符号链接（macOS）
///
/// macOS 没有 .lnk 概念；桌面符号链接在 Finder 中双击即可启动。
/// 若日后改为 .app 分发，链接指向 .app 内的可执行文件仍然有效。
#[cfg(target_os = "macos")]
pub fn create_desktop_shortcut() -> Result<(), String> {
    use std::os::unix::fs::symlink;

    // 1. 获取可执行文件路径
    let exe_path = std::env::current_exe().map_err(|e| format!("获取可执行文件路径失败：{}", e))?;

    // 2. 获取桌面路径
    let desktop_dir = dirs::desktop_dir().ok_or_else(|| "无法获取桌面路径".to_string())?;

    // 3. 构造快捷方式路径（国际化名称，无扩展名）
    let name = get_shortcut_name();
    let shortcut_path: PathBuf = desktop_dir.join(name);

    // 4. 先清理同名旧链接，避免 symlink 已存在时报错
    if shortcut_path.symlink_metadata().is_ok() {
        std::fs::remove_file(&shortcut_path).map_err(|e| format!("删除旧快捷方式失败：{}", e))?;
    }

    // 5. 创建符号链接
    symlink(&exe_path, &shortcut_path).map_err(|e| format!("创建快捷方式失败：{}", e))?;

    Ok(())
}
