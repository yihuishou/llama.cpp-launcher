//! 网络工具（系统代理 + 共享 ureq Agent 构建）
//!
//! 供 updater（软件自更新）与 downloader（llama.cpp 下载）共用，
//! 避免两个模块各自复制 Agent 构建与超时/UA 常量。
//!
//! ## 系统代理
//! ureq 默认只读环境变量代理（需 feature）且**不读 Windows 系统代理注册表**。
//! 国内用户普遍通过 Clash 等代理工具访问 GitHub——它们把代理配置写在
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`
//! （ProxyEnable=1, ProxyServer=127.0.0.1:7890），并开启"系统代理"。
//! 本模块读取该注册表项，把系统代理应用到 ureq Agent。
//!
//! ## 超时策略（关键）
//! 只设 `timeout_connect`（连接建立上限），不设 `timeout_recv_body` / `.timeout()`。
//! 原因：ureq 的 `.timeout()` 与 `timeout_recv_body` 都是"总预算"型超时
//! （`timeout_recv_body` 的起点是收到响应头那一刻，且读取期间不逐次重置），
//! 大文件下载（数 MB~GB）必然中途被掐断并触发镜像源重新下载（进度条读两次）。
//! 连接超时只防卡死，不限制下载总时长。

/// 连接建立超时（不限制下载总时长）
pub const NETWORK_TIMEOUT_SECS: u64 = 30;
/// 请求 User-Agent（GitHub API 必需）
pub const USER_AGENT: &str = "llama-cpp-launcher";
/// 下载块大小（字节）
pub const CHUNK_SIZE: usize = 8192;
/// 全部源都失败时统一返回的错误标记（UI 展示"获取失败：网络错误"）
pub const ERR_NETWORK: &str = "network-error";

/// 构建共享 ureq Agent：带超时 + User-Agent + 系统代理（有则用）。
/// updater/downloader 统一调用，避免重复实现。
pub fn build_agent() -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(NETWORK_TIMEOUT_SECS)));
    if let Some(proxy) = system_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    builder.build().into()
}

/// 读取 Windows 系统代理（注册表），返回 ureq Proxy（调试/堆积时返回 None）
#[cfg(target_os = "windows")]
pub fn system_proxy() -> Option<ureq::Proxy> {
    let mut enable: Option<u32> = None;
    let mut server: Option<String> = None;

    // 读取 ProxyEnable / ProxyServer（子项：Internet Settings）
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("reg")
            .args([
                "query",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            ])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let lower = line.to_lowercase();
            if lower.contains("proxyenable") {
                enable = line
                    .split_whitespace()
                    .last()
                    .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());
            } else if lower.contains("proxyserver") {
                server = line.split_whitespace().last().map(str::to_string);
            }
        }
    }

    // 仅当系统代理开启且地址存在时应用
    if enable == Some(1) {
        if let Some(addr) = server {
            if !addr.is_empty() {
                // Clash 类工具通常填 `127.0.0.1:7890`（无协议前缀），补 http://
                let proxy_url = if addr.contains("://") {
                    addr
                } else {
                    format!("http://{}", addr)
                };
                match ureq::Proxy::new(&proxy_url) {
                    Ok(p) => {
                        log::info!("[net_proxy] 使用系统代理: {}", proxy_url);
                        return Some(p);
                    }
                    Err(e) => log::warn!("[net_proxy] 系统代理解析失败: {}", e),
                }
            }
        }
    }
    None
}

/// 非 Windows 平台：无系统代理注册表，返回 None
#[cfg(not(target_os = "windows"))]
pub fn system_proxy() -> Option<ureq::Proxy> {
    None
}
