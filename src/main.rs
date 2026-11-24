//! 启动宿主管理页并注册查询与应用入口

mod gateway;
mod management;
mod ranking;

use std::sync::Arc;

use gateway_plugin_sdk::{
    CallBudgets, Stage,
    client::{PluginBuilder, PluginSession, SessionConfig},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = PluginSession::accept(
        tokio::io::stdin(),
        tokio::io::stdout(),
        SessionConfig::default(),
    )
    .await?;
    session.set_call_budgets(CallBudgets::from([(Stage::Management, 170_000)]))?;
    let handshake = session.handshake();
    let configuration = serde_json::from_value(handshake.configuration.clone())?;
    let app = Arc::new(management::App::new(
        configuration,
        handshake.artifact_sha256.clone(),
    ));
    let plugin = PluginBuilder::from_json(include_bytes!("../plugin.json"))?
        .management(management::registration(), move |call| {
            let app = Arc::clone(&app);
            async move { app.handle(call).await }
        })?
        .build()?;
    session.run(plugin).await?;
    Ok(())
}
