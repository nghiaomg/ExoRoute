mod admin;
mod app;
mod config;
mod gateway;
mod infra;
mod protocol;
mod provider_adapters;
mod security;
mod state;
mod static_assets;
mod support;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The default runtime drop waits for blocking workers without a deadline.
    // A bounded shutdown is required because LMDB/file work can outlive the
    // async request that started it.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(app::async_main());
    runtime.shutdown_timeout(app::RUNTIME_SHUTDOWN_TIMEOUT);
    result
}
