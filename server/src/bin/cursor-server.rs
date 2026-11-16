//! Starts the Cursor BYOK server executable.
use cursor_server::store::ExternalApiSettings;
use cursor_server::{App, Config, Result};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 加载当前目录下的 .env(若存在),与环境变量合并
    let _ = dotenvy::dotenv();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cursor_server=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = App::new(Config::from_env()?).await?;

    // 若通过 .env / 环境变量指定了外部访问 key,启动时写入存储并启用外部 API。
    // 这样无需打开管理面板即可一键配置 PROXY_ACCESS_KEY。
    if let Ok(key) = std::env::var("CURSOR_EXTERNAL_API_KEY") {
        if !key.is_empty() {
            tracing::info!("seeding external API key from CURSOR_EXTERNAL_API_KEY");
            app.store()
                .set_external_api_settings(ExternalApiSettings {
                    enabled: true,
                    api_key: key,
                })
                .await?;
        }
    }

    app.serve().await
}
