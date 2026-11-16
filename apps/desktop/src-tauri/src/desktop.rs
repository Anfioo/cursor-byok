use std::{
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::startup::{self, StartupDiagnostics};
use crate::tray;

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
const AUTOSTART_ARG: &str = "--autostart";
/// sidecar 就绪等待上限(秒)。
const SIDECAR_READY_TIMEOUT_SECS: u64 = 30;
/// 健康检查轮询间隔。
const SIDECAR_POLL_INTERVAL_MS: u64 = 150;

struct DesktopRuntime {
    /// sidecar 子句柄;退出时 kill 掉。
    child: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
    exiting: AtomicBool,
    /// 本地服务实际监听地址(由父进程选空闲端口后通过环境变量传给 sidecar)。
    server_addr: std::net::SocketAddr,
}

#[tauri::command]
fn open_terminal_with_command(command: String) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = command;
        Command::new("open").args(["-a", "Terminal"]).status()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "cmd", "/K", &command])
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        const TERMINALS: &[(&str, &[&str])] = &[
            ("x-terminal-emulator", &["-e"]),
            ("gnome-terminal", &["--"]),
            ("konsole", &["-e"]),
            ("xfce4-terminal", &["--execute"]),
            ("alacritty", &["-e"]),
            ("kitty", &[]),
        ];
        let script = format!("{command}; exec bash");
        for (terminal, separator) in TERMINALS {
            let mut process = Command::new(terminal);
            process.args(*separator);
            process.arg("bash").arg("-c").arg(&script);
            match process.spawn() {
                Ok(_) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(tauri::Error::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no supported terminal emulator found",
        )))
    }
}

fn create_main_window(
    app: &AppHandle,
    address: std::net::SocketAddr,
) -> tauri::Result<tauri::WebviewWindow> {
    let url = format!("http://{address}/__byok-api__/")
        .parse()
        .expect("local frontend URL");
    let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::External(url))
        .title("Cursor BYOK")
        .inner_size(820.0, 558.0)
        .min_inner_size(820.0, 558.0)
        .center()
        .background_color(tauri::webview::Color(20, 20, 20, 255))
        .decorations(cfg!(target_os = "macos"))
        .shadow(true)
        .resizable(true)
        .visible(false);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    builder.build()
}

/// 按需打开主窗口:webview 仅在需要界面时创建,关闭窗口即销毁释放内存。
pub(crate) fn open_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let address = app.state::<DesktopRuntime>().server_addr;
    let window = create_main_window(app, address)?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// 选一个本机空闲端口。存在极小的 TOCTOU 窗口(选完到子进程 bind 之间被抢走),
/// 命中时 sidecar 会 bind 失败退出,下方就绪等待会超时并报错,可接受。
fn pick_free_port() -> tauri::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

/// 轮询 TCP 连通,直到 sidecar 开始监听或超时。
fn wait_for_sidecar(addr: &std::net::SocketAddr, timeout: Duration) -> tauri::Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if std::net::TcpStream::connect(addr).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(SIDECAR_POLL_INTERVAL_MS));
    }
    Err(tauri::Error::from(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("cursor-server sidecar 未在 {} 秒内就绪", timeout.as_secs()),
    )))
}

pub fn run() -> std::process::ExitCode {
    let diagnostics = match StartupDiagnostics::initialize() {
        Ok(diagnostics) => diagnostics,
        Err(error) => {
            startup::report_logging_failure(error.as_ref());
            return std::process::ExitCode::FAILURE;
        }
    };
    #[cfg(unix)]
    {
        let open_file_limit = match crate::resource_limits::raise_open_file_limit() {
            Ok(limit) => limit,
            Err(error) => {
                diagnostics.report_fatal(&error);
                return std::process::ExitCode::FAILURE;
            }
        };
        tracing::info!(
            requested = crate::resource_limits::REQUESTED_OPEN_FILE_LIMIT,
            previous = open_file_limit.previous,
            effective = open_file_limit.effective,
            hard = open_file_limit.hard,
            "open file limit configured"
        );
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        architecture = std::env::consts::ARCH,
        log_directory = %diagnostics.log_directory().display(),
        "desktop starting (sidecar mode)"
    );

    let started_by_autostart = std::env::args_os().any(|arg| arg == AUTOSTART_ARG);

    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_terminal_with_command,
            crate::update::check_portable_update,
            crate::update::install_portable_update,
        ])
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|arg| arg == AUTOSTART_ARG) {
                let _ = open_main_window(app);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec![AUTOSTART_ARG]),
            ))?;

            // 1. 选空闲端口
            let port = pick_free_port()?;
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

            // 2. 组装 sidecar 环境变量
            let mut sidecar_cmd = app
                .shell()
                .sidecar("cursor-server")?
                .env("CURSOR_LISTEN_ADDR", format!("127.0.0.1:{port}"));

            // 开发模式:前端由 Vite(1420)提供,sidecar 反代过去;
            // 打包模式:前端 dist 作为资源随包分发,通过 CURSOR_CONSOLE_DIR 指给 sidecar。
            #[cfg(dev)]
            {
                sidecar_cmd =
                    sidecar_cmd.env("CURSOR_CONSOLE_PROXY", "http://127.0.0.1:1420");
            }
            #[cfg(not(dev))]
            {
                let resource_dir = app.path().resource_dir()?;
                let dist_dir = resource_dir.join("dist");
                tracing::info!(?dist_dir, "serving console from bundled dist");
                sidecar_cmd = sidecar_cmd
                    .env("CURSOR_CONSOLE_DIR", dist_dir.to_string_lossy().to_string());
            }

            // 3. 拉起 sidecar
            let (mut rx, child) = sidecar_cmd.spawn()?;
            tauri::async_runtime::spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        CommandEvent::Stdout(line) => {
                            tracing::info!(target: "cursor-server", "{}", String::from_utf8_lossy(&line));
                        }
                        CommandEvent::Stderr(line) => {
                            tracing::warn!(target: "cursor-server", "{}", String::from_utf8_lossy(&line));
                        }
                        CommandEvent::Error(err) => {
                            tracing::error!(target: "cursor-server", "sidecar error: {err}");
                        }
                        other => {
                            tracing::info!(target: "cursor-server", "sidecar event: {other:?}");
                        }
                    }
                }
            });

            // 4. 等 sidecar 就绪
            wait_for_sidecar(&addr, Duration::from_secs(SIDECAR_READY_TIMEOUT_SECS))?;
            tracing::info!(%addr, "cursor-server sidecar ready");

            app.manage(DesktopRuntime {
                child: Mutex::new(Some(child)),
                exiting: AtomicBool::new(false),
                server_addr: addr,
            });

            if started_by_autostart {
                tracing::info!("autostart launch; starting without the main window");
            } else {
                open_main_window(app.handle())?;
            }
            tray::create(app)?;
            crate::update::signal_ready_if_requested()?;
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            diagnostics.report_fatal(&error);
            return std::process::ExitCode::FAILURE;
        }
    };

    app.run(|app, event| match event {
        RunEvent::ExitRequested { code, api, .. } => match code {
            // code 为 None 表示所有窗口已被关闭(轻量模式),阻止退出,
            // sidecar 继续在托盘后台运行;code 为 Some 时是显式退出请求。
            None => api.prevent_exit(),
            Some(_) => {
                let runtime = app.state::<DesktopRuntime>();
                if !runtime.exiting.swap(true, Ordering::AcqRel) {
                    api.prevent_exit();
                    let app = app.clone();
                    let child = runtime.child.lock().expect("child lock poisoned").take();
                    tauri::async_runtime::spawn(async move {
                        if let Some(child) = child {
                            if let Err(error) = child.kill() {
                                tracing::warn!(%error, "failed to kill cursor-server sidecar");
                            }
                        }
                        app.exit(0);
                    });
                }
            }
        },
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => {
            let _ = open_main_window(app);
        }
        _ => {}
    });

    std::process::ExitCode::SUCCESS
}
