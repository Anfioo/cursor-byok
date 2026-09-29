# Sidecar 模式说明（dev 分支）

本分支把桌面壳（`apps/desktop/src-tauri`）从「把 `cursor-server` 链进同一个进程」
改成了「把 `cursor-server` 作为外部 sidecar 二进制随包分发、运行时拉起」。

## 为什么这么做

- **解耦升级**：升级模型网关只需替换 `binaries/cursor-server-<triple>` 这个文件，
  不用重新编译整个 Tauri 壳、不用出新版安装包。
- **可独立复用**：这个 sidecar 二进制本身就是普通的 `cursor-server`，
  你自己的 Tauri 项目（或任意进程）也可以直接拉起它、再把 webview 指过去。
- **壳变薄**：`src-tauri` 不再依赖 server 的内部库，只负责选端口、拉起子进程、开窗口。

## 架构

```
Tauri 主进程（桌面壳，Rust）
  ├── setup() 选一个空闲端口 127.0.0.1:0
  ├── sidecar("cursor-server").spawn()
  │     env: CURSOR_LISTEN_ADDR=127.0.0.1:<port>
  │     env: CURSOR_CONSOLE_DIR=<resource_dir>/dist   (打包)
  │          或 CURSOR_CONSOLE_PROXY=http://127.0.0.1:1420  (开发)
  ├── 轮询 TCP 连通，等 sidecar 就绪
  └── WebviewWindow 打开 http://127.0.0.1:<port>/__byok-api__/
```

## 构建步骤（在你自己的机器上）

```bash
# 1. 装前端依赖
npm --prefix apps/desktop install

# 2. 先把 sidecar 编出来（debug，供 tauri dev 用）
make build-sidecar

# 3. 跑桌面开发（会同时起 Vite + 拉起 sidecar + 开窗口）
make dev-desktop
```

打正式包前：

```bash
make build-sidecar SIDE_PROFILE=release   # release 版 sidecar
make build-web                            # 前端 dist
make build-desktop                        # 出安装包
```

## sidecar 二进制放哪

Tauri 约定 sidecar 文件名要带 target triple：

```
apps/desktop/src-tauri/binaries/
  cursor-server-x86_64-unknown-linux-gnu
  cursor-server-aarch64-apple-darwin
  cursor-server-x86_64-pc-windows-msvc.exe
  ...
```

`make build-sidecar` 会自动按当前机器的 triple 命名并拷贝。
跨平台打包时需要在对应平台上各跑一次 `make build-sidecar SIDE_PROFILE=release`。

## 已知差距（相对原来的同进程模式）

1. **`/api/desktop/open-external-url` 暂时 404**。原来这个路由由桌面壳自己的 axum
   路由提供（用于在系统浏览器里打开外链）。sidecar 里没有。影响范围：前端里
   「在外部浏览器打开」类按钮。后续可以：(a) 给 standalone server 补这个路由；
   (b) 在 Tauri 窗口上拦截新窗口跳转，用 opener 插件打开。
2. **启动时的 `harness.cleanup_stale_settings()` 没了**。原来桌面壳会在启动时
   清理残留的 Cursor 配置；sidecar 自己的 `serve()` 不做这件事。影响很小，
   需要的话可以在 server 的 `serve()` 里补上。
3. **macOS Dock 图标开关、静默启动** 这些桌面偏好，原来直接调 in-process API 读取；
   sidecar 模式下暂时跳过，后续可以在 sidecar 就绪后通过 HTTP 读 `/api/settings/desktop` 补上。

## 如果你想在【自己的另一个 Tauri 项目】里复用这个看板

1. 把编出来的 `cursor-server-<triple>` 当成普通 sidecar 加进你项目的 `externalBin`；
2. 运行时给它 `CURSOR_CONSOLE_DIR` 指到本仓库 `apps/desktop/dist`（或你自己 fork 的前端构建产物）；
3. 选个空闲端口传 `CURSOR_LISTEN_ADDR`，等就绪后 webview/iframe 指过去即可。
