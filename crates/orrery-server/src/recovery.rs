use sqlx::PgPool;
use uuid::Uuid;

/// On startup, ensure every WAITING_FOR_TASK instance has a corresponding task row.
/// This is idempotent — safe to call every time the server starts.
pub async fn recover_orphaned_tasks(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let orphans = sqlx::query!(
        r#"
        SELECT pi.id, pi.active_element_ids
        FROM process_instances pi
        WHERE pi.state = 'WAITING_FOR_TASK'
          AND NOT EXISTS (
            SELECT 1 FROM tasks t
            WHERE t.process_instance_id = pi.id
              AND t.state IN ('CREATED', 'CLAIMED')
          )
        "#,
    )
    .fetch_all(pool)
    .await?;

    let count = orphans.len() as u64;

    for row in orphans {
        let active: Vec<String> =
            serde_json::from_value(row.active_element_ids).unwrap_or_default();

        if let Some(element_id) = active.into_iter().next() {
            let task_id = Uuid::new_v4().to_string();
            sqlx::query!(
                r#"
                INSERT INTO tasks (id, process_instance_id, element_id, element_type, state, variables)
                VALUES ($1, $2, $3, 'SERVICE_TASK', 'CREATED', '{}')
                ON CONFLICT DO NOTHING
                "#,
                task_id,
                row.id,
                element_id,
            )
            .execute(pool)
            .await?;
        }
    }

    Ok(count)
}
