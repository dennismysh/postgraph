use chrono::{Duration, Utc};
use sqlx::PgPool;

/// Helper: insert a post with a given id, timestamp, and current views.
async fn insert_post(pool: &PgPool, id: &str, days_ago: i64, views: i32) {
    let ts = Utc::now() - Duration::days(days_ago);
    sqlx::query(
        "INSERT INTO posts (id, text, timestamp, views, synced_at) VALUES ($1, $2, $3, $4, NOW())",
    )
    .bind(id)
    .bind(format!("Test post {id}"))
    .bind(ts)
    .bind(views)
    .execute(pool)
    .await
    .unwrap();
}

/// Helper: insert an engagement snapshot at a specific time offset.
async fn insert_snapshot(pool: &PgPool, post_id: &str, days_ago: i64, views: i32) {
    let captured = Utc::now() - Duration::days(days_ago);
    sqlx::query(
        "INSERT INTO engagement_snapshots (id, post_id, captured_at, views) VALUES (gen_random_uuid(), $1, $2, $3)",
    )
    .bind(post_id)
    .bind(captured)
    .bind(views)
    .execute(pool)
    .await
    .unwrap();
}

/// The delta query used by get_views_from_snapshots (with the WHERE es.views > 0 removed).
const DELTA_QUERY: &str = r#"
    WITH ordered_snapshots AS (
        SELECT es.captured_at,
               es.post_id,
               es.views,
               LAG(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_views,
               p.timestamp AS post_timestamp
        FROM engagement_snapshots es
        JOIN posts p ON p.id = es.post_id
    ),
    with_deltas AS (
        SELECT CASE
                   WHEN prev_views IS NULL THEN post_timestamp
                   ELSE captured_at
               END AS effective_date,
               GREATEST(views - COALESCE(prev_views, 0), 0) AS view_delta
        FROM ordered_snapshots
    )
    SELECT COALESCE(SUM(view_delta), 0)::bigint AS total_views
    FROM with_deltas
"#;

/// The range-sum query used by get_views_range_sums.
const RANGE_SUM_QUERY: &str = r#"
    SELECT COALESCE(SUM(
        CASE
            WHEN p.timestamp >= $1 THEN p.views
            ELSE GREATEST(p.views - COALESCE(boundary.views, 0), 0)
        END
    ), 0)::bigint
    FROM posts p
    LEFT JOIN LATERAL (
        SELECT es.views FROM engagement_snapshots es
        WHERE es.post_id = p.id AND es.captured_at <= $1
        ORDER BY es.captured_at DESC LIMIT 1
    ) boundary ON TRUE
"#;

/// Verifies that snapshots with views=0 participate in the LAG window,
/// producing correct total deltas. The old WHERE es.views > 0 would have
/// excluded the first snapshot, making the total 70 instead of 120.
#[sqlx::test(migrations = "migrations")]
async fn delta_query_counts_all_views_including_zero_snapshots(pool: PgPool) {
    insert_post(&pool, "p1", 10, 120).await;
    // Snapshots: 0 → 50 → 120
    insert_snapshot(&pool, "p1", 9, 0).await;
    insert_snapshot(&pool, "p1", 5, 50).await;
    insert_snapshot(&pool, "p1", 1, 120).await;

    let (total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Deltas: 0 (first, 0-0), 50 (50-0), 70 (120-50) = 120
    assert_eq!(total, 120, "should count all views including zero-snapshot");
}

/// Verifies that SUM(posts.views) matches the expected authoritative total.
#[sqlx::test(migrations = "migrations")]
async fn total_views_matches_sum_of_post_views(pool: PgPool) {
    insert_post(&pool, "p1", 30, 100).await;
    insert_post(&pool, "p2", 20, 250).await;
    insert_post(&pool, "p3", 10, 650).await;

    let (total,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM posts")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(total, 1000);
}

/// Verifies range sums use the closest snapshot before the boundary.
/// Post created 60 days ago, snapshots at 40d (200) and 10d (400), current views=500.
/// For a 30-day range: boundary snapshot is at 40d (views=200), so range = 500-200 = 300.
#[sqlx::test(migrations = "migrations")]
async fn range_sums_uses_snapshot_boundaries(pool: PgPool) {
    insert_post(&pool, "p1", 60, 500).await;
    insert_snapshot(&pool, "p1", 40, 200).await;
    insert_snapshot(&pool, "p1", 10, 400).await;
    insert_snapshot(&pool, "p1", 0, 500).await;

    let boundary = Utc::now() - Duration::days(30);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_views, 300,
        "should use snapshot at 40d ago (closest before 30d boundary)"
    );
}

/// Verifies that posts created within the range count all their views.
#[sqlx::test(migrations = "migrations")]
async fn range_sums_counts_full_views_for_new_posts(pool: PgPool) {
    insert_post(&pool, "p1", 5, 800).await;
    insert_snapshot(&pool, "p1", 4, 200).await;
    insert_snapshot(&pool, "p1", 0, 800).await;

    let boundary = Utc::now() - Duration::days(30);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_views, 800,
        "post created within range should count all views"
    );
}

/// Documents that delta sums can OVER-count when views decrease then recover.
/// Snapshots: 100 → 80 (API glitch) → 150
/// Deltas: 100 + 0 + 70 = 170, but posts.views = 150.
/// This validates why range sums (using posts.views) are more accurate.
#[sqlx::test(migrations = "migrations")]
async fn delta_query_handles_decreasing_views(pool: PgPool) {
    insert_post(&pool, "p1", 10, 150).await;
    insert_snapshot(&pool, "p1", 9, 100).await;
    insert_snapshot(&pool, "p1", 5, 80).await;
    insert_snapshot(&pool, "p1", 1, 150).await;

    let (delta_total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Deltas: 100 (first), GREATEST(80-100,0)=0, GREATEST(150-80,0)=70 → 170
    assert_eq!(
        delta_total, 170,
        "delta sum overcounts after glitch+recovery"
    );

    // The authoritative total from posts.views is correct
    let (auth_total,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM posts")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(auth_total, 150, "posts.views is the authoritative source");

    // Range sum (all time) matches the authoritative total
    let boundary = Utc::now() - Duration::days(365);
    let (range_total,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    // Post created 10 days ago, within the 365d range, so all views count
    assert_eq!(
        range_total, 150,
        "range sum should match authoritative posts.views"
    );
}
