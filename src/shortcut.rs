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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    // 1. 获取 exe 路径和所在目录
    let exe_path = std::env::current_exe().map_err(|e| format!("获取 exe 路径失败：{}", e))?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| "无法获取 exe 所在目录".to_string())?;

    // 2. 获取桌面路径
    let desktop_dir = dirs::desktop_dir().ok_or_else(|| "无法获取桌面路径".to_string())?;

    // 3. 构造 .desktop 文件路径（国际化名称）
    let name = get_shortcut_name();
    let shortcut_path: PathBuf = desktop_dir.join(format!("{}.desktop", name));

    // 4. 释放图标到 exe 同级目录（如果不存在）
    // 图标在编译时嵌入二进制文件，运行时释放
    let icon_filename = "llama-cpp-launcher.png";
    let icon_path = exe_dir.join(icon_filename);

    if !icon_path.exists() {
        // 从编译时嵌入的字节数据释放图标
        const ICON_BYTES: &[u8] = include_bytes!("../llama-cpp-launcher.png");
        fs::write(&icon_path, ICON_BYTES).map_err(|e| format!("释放图标文件失败：{}", e))?;
    }

    // 5. 创建 .desktop 文件内容（使用绝对路径引用图标）
    let icon_absolute = icon_path
        .to_str()
        .ok_or_else(|| "图标路径无效".to_string())?;
    let desktop_content = format!(
        r#"[Desktop Entry]
Type=Application
Name={}
Exec={}
Icon={}
Terminal=false
"#,
        name,
        exe_path.display(),
        icon_absolute
    );

    // 6. 写入文件
    fs::write(&shortcut_path, &desktop_content)
        .map_err(|e| format!("创建 .desktop 文件失败：{}", e))?;

    // 7. 设置可执行权限（Linux .desktop 文件需要）
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&shortcut_path)
            .map_err(|e| format!("获取文件权限失败：{}", e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shortcut_path, perms)
            .map_err(|e| format!("设置文件权限失败：{}", e))?;
    }

    Ok(())
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
