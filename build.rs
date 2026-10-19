/// 定位 MSVC 工具链中 rc.exe 所在目录（`<Windows Kits>/bin/<版本>/x64`）。
///
/// winres 默认通过 `reg query` 读注册表来定位 Windows SDK 安装路径；在无法访问
/// 注册表的环境（受限沙箱、部分 CI）下会拿到空路径，导致资源编译失败、exe 丢失
/// 图标与版本信息。这里先主动查找一次，命中即显式告知 winres；
/// 未命中则返回 None，交由 winres 按原有逻辑自行处理（不影响正常环境）。
fn find_msvc_rc_dir() -> Option<String> {
    use std::path::{Path, PathBuf};

    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1) 环境变量：WindowsSdkDir（通常带尾部分隔符）+ WindowsSDKVersion
    if let Ok(root) = std::env::var("WindowsSdkDir") {
        let ver = std::env::var("WindowsSDKVersion").unwrap_or_default();
        let ver = ver.trim_end_matches(|c| c == '\\' || c == '/');
        candidates.push(Path::new(&root).join("bin").join(ver).join("x64"));
    }

    // 2) 常见安装位置：按版本目录倒序，优先取较新的 SDK
    for base in [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ] {
        if let Ok(entries) = std::fs::read_dir(base) {
            let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            dirs.sort();
            dirs.reverse();
            for d in dirs {
                candidates.push(d.join("x64"));
            }
        }
    }

    candidates
        .into_iter()
        .find(|p| p.join("rc.exe").is_file())
        .map(|p| p.to_string_lossy().to_string())
}

/// 把 `CARGO_PKG_VERSION`（形如 `0.2.2`）打包成 VERSIONINFO 需要的 u64（四段各占 16 位）。
///
/// 必须与 Cargo.toml 同源，不要退回写死的字面量 —— 那样 exe 属性里的版本会再次漂移。
fn package_version_u64() -> u64 {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts = version.split('.').map(|part| {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u16>().unwrap_or(0)
    });
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    ((major as u64) << 48) | ((minor as u64) << 32) | ((patch as u64) << 16)
}

fn main() {
    // build.rs 运行在 host 上，不能用 #[cfg(windows)] 判断目标平台
    // 必须通过 CARGO_CFG_TARGET_OS 环境变量判断
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "windows" {
        let mut res = winres::WindowsResource::new();
        // 主动指定 rc.exe 目录，避免 winres 依赖注册表查询（受限环境下会失败）
        if let Some(dir) = find_msvc_rc_dir() {
            res.set_toolkit_path(&dir);
        }
        res.set_icon("assets/llama.ico");
        res.set("ProductName", "llama.cpp launcher");
        res.set("FileDescription", "llama.cpp launcher - GUI Launcher");
        res.set("LegalCopyright", "Copyright 2025");
        res.set("InternalName", "llama.cpp launcher");
        res.set("OriginalFilename", "llama_cpp_launcher.exe");
        let version = package_version_u64();
        res.set_version_info(winres::VersionInfo::FILEVERSION, version);
        res.set_version_info(winres::VersionInfo::PRODUCTVERSION, version);
        res.set_version_info(winres::VersionInfo::FILEOS, 0x40004u64);
        res.set_version_info(winres::VersionInfo::FILETYPE, 0x2u64);
        // 兜底：资源编译失败也不中断构建（仅丢失图标与版本信息，不影响功能）
        if let Err(e) = res.compile() {
            println!("cargo:warning=winres 资源编译已跳过（缺图标/版本信息）: {e}");
        }
    }

    // Linux: 将图标文件复制到 exe 同级目录
    if target_os == "linux" {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let icon_src = std::path::Path::new(&manifest_dir).join("llama-cpp-launcher.png");
        let icon_dst = std::path::Path::new(&out_dir)
            .join("../../..") // 回溯到 target/release 或 target/debug
            .join("llama-cpp-launcher.png");

        if icon_src.exists() {
            let _ = std::fs::copy(&icon_src, &icon_dst);
        }
    }
}
