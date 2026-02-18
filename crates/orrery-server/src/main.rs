use orrery_server::{build_app, db};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = db::connect(&database_url).await;

    let recovered = orrery_server::recovery::recover_orphaned_tasks(&pool)
        .await
        .expect("crash recovery failed");
    if recovered > 0 {
        tracing::warn!("crash recovery: created task rows for {recovered} orphaned instances");
    }

    // Start background timer scheduler (fires every 5s)
    let scheduler_pool = pool.clone();
    tokio::spawn(async move {
        orrery_server::scheduler::run(scheduler_pool, std::time::Duration::from_secs(5)).await;
    });

    let app = build_app(pool);

    let host = std::env::var("ORRERY_HOST").unwrap_or("0.0.0.0".to_string());
    let port = std::env::var("ORRERY_PORT").unwrap_or("3000".to_string());
    let addr = format!("{host}:{port}");

    tracing::info!("orrery-server listening on {addr}");
    tracing::info!("API docs at http://{addr}/docs");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
