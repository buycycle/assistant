use rust_bot::assistant::{assistant_chat_handler, create_assistant, DB};
use sqlx::{Executor, SqlitePool};
async fn setup_test_db() -> SqlitePool {
    let database_url = "sqlite::memory:";
    let pool = SqlitePool::connect(database_url).await.unwrap();
    // Run migrations or setup schema here if necessary
    pool
}
#[tokio::test]
async fn test_create_assistant() {
    let db_pool = setup_test_db().await;
    // Call the create_assistant function and assert that it works.
    let result = create_assistant(
        "Test Assistant",
        "gpt-4",
        "Your instructions here",
        "/context",
    )
    .await;
    assert!(result.is_ok()); // Use assert! to check if the result is Ok
                             // Optionally, check if the assistant was correctly inserted into the database.
    let assistant_id = result.unwrap().id;
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM assistants WHERE id = ?")
        .bind(&assistant_id)
        .fetch_one(&db_pool)
        .await
        .unwrap();
    assert_eq!(row.0, 1, "Assistant was not inserted into the database");
}
