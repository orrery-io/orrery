use sqlx::PgPool;
use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

#[tokio::test]
async fn test_migrations_run_on_connect() {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");

    // db::connect should run migrations without panicking
    let pool = orrery_server::db::connect(&db_url).await;

    // Verify migrations ran by checking a table created by 001_initial.sql
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'process_definitions')"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(exists, "process_definitions table should exist after migration");
}

#[tokio::test]
#[should_panic(expected = "Failed to run database migrations")]
async fn test_failed_migration_panics() {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");

    // Pre-create a table that conflicts with 001_initial.sql so migration fails
    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::query("CREATE TABLE process_definitions (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    // This should panic because migration 001 tries to CREATE TABLE process_definitions
    orrery_server::db::connect(&db_url).await;
}
