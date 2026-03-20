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

/// The delta query used by get_views_from_snapshots (chart), using MAX high-water mark.
const DELTA_QUERY: &str = r#"
    WITH ordered_snapshots AS (
        SELECT es.captured_at,
               es.post_id,
               es.views,
               MAX(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_views,
               p.timestamp AS post_timestamp
        FROM engagement_snapshots es
        JOIN posts p ON p.id = es.post_id
    ),
    with_deltas AS (
        SELECT CASE
                   WHEN prev_views IS NULL OR prev_views = 0 THEN post_timestamp
                   ELSE captured_at
               END AS effective_date,
               GREATEST(views - COALESCE(prev_views, 0), 0) AS view_delta
        FROM ordered_snapshots
    )
    SELECT COALESCE(SUM(view_delta), 0)::bigint AS total_views
    FROM with_deltas
"#;

/// The authoritative total views query used by get_analytics().
const TOTAL_VIEWS_QUERY: &str = "SELECT COALESCE(SUM(views), 0)::bigint FROM posts";

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

// ---------------------------------------------------------------------------
// Delta query tests (validates the chart's snapshot-based approach)
// ---------------------------------------------------------------------------

/// Verifies that snapshots with views=0 participate in the MAX window,
/// producing correct total deltas.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_counts_all_views_including_zero_snapshots(pool: PgPool) {
    insert_post(&pool, "p1", 10, 120).await;
    // Snapshots: 0 → 50 → 120
    insert_snapshot(&pool, "p1", 9, 0).await;
    insert_snapshot(&pool, "p1", 5, 50).await;
    insert_snapshot(&pool, "p1", 1, 120).await;

    let (total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Row 1 (0):   prev=NULL, delta=0
    // Row 2 (50):  prev=MAX(0)=0, delta=50
    // Row 3 (120): prev=MAX(0,50)=50, delta=70
    // Total: 120
    assert_eq!(total, 120, "should count all views including zero-snapshot");
}

/// Verifies that SUM(posts.views) matches the expected authoritative total.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn total_views_matches_sum_of_post_views(pool: PgPool) {
    insert_post(&pool, "p1", 30, 100).await;
    insert_post(&pool, "p2", 20, 250).await;
    insert_post(&pool, "p3", 10, 650).await;

    let (total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 1000);
}

/// Verifies range sums use the closest snapshot before the boundary.
/// Post created 60 days ago, snapshots at 40d (200) and 10d (400), current views=500.
/// For a 30-day range: boundary snapshot is at 40d (views=200), so range = 500-200 = 300.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
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
#[sqlx::test(migrations = "../postgraph-server/migrations")]
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

/// With MAX high-water mark, API glitches (temporary drops) don't cause
/// overcounting. Snapshots: 100 → 80 (glitch) → 150.
/// MAX-based deltas: 100 + 0 + 50 = 150, matching posts.views exactly.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_handles_decreasing_views(pool: PgPool) {
    insert_post(&pool, "p1", 10, 150).await;
    insert_snapshot(&pool, "p1", 9, 100).await;
    insert_snapshot(&pool, "p1", 5, 80).await;
    insert_snapshot(&pool, "p1", 1, 150).await;

    let (delta_total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Row 1 (100): prev=NULL, delta=100
    // Row 2 (80):  prev=MAX(100)=100, delta=GREATEST(80-100,0)=0
    // Row 3 (150): prev=MAX(100,80)=100, delta=GREATEST(150-100,0)=50
    // Total: 150
    assert_eq!(
        delta_total, 150,
        "MAX-based deltas handle API glitches correctly"
    );

    // The authoritative total from posts.views matches
    let (auth_total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
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
    assert_eq!(
        range_total, 150,
        "range sum should match authoritative posts.views"
    );
}

// ---------------------------------------------------------------------------
// Tests for the root cause: posts without proper snapshots
// ---------------------------------------------------------------------------

/// Posts with views but NO engagement snapshots should still be counted
/// in total_views (SUM from posts), even though the delta query misses them.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn post_with_no_snapshots_counted_in_total(pool: PgPool) {
    insert_post(&pool, "p1", 30, 500).await;
    // No snapshots at all for this post

    // Authoritative total includes the post
    let (auth_total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        auth_total, 500,
        "SUM(views) should include posts without snapshots"
    );

    // Delta query returns 0 (no snapshots to compute deltas from)
    let (delta_total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    assert_eq!(
        delta_total, 0,
        "delta query misses posts without snapshots (documents the bug)"
    );

    // Range sum for "all" range should include the post
    let boundary = Utc::now() - Duration::days(3650);
    let (range_total,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_total, 500,
        "range sum should include posts without snapshots"
    );
}

/// Posts with only zero-value snapshots (from migration 004 backfill)
/// should still have their views counted via the boundary-based range query.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn post_with_only_zero_snapshots_counted_in_range(pool: PgPool) {
    insert_post(&pool, "p1", 20, 300).await;
    // Only a zero-value snapshot exists (simulates migration 004 backfill)
    insert_snapshot(&pool, "p1", 15, 0).await;

    // For a 30-day range: post was created within range, so all 300 views count
    let boundary = Utc::now() - Duration::days(30);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_views, 300,
        "post created within range counts all views even with only zero snapshots"
    );

    // For a 10-day range: post was created before range, boundary snapshot is at 15d (views=0)
    // So range = 300 - 0 = 300
    let boundary = Utc::now() - Duration::days(10);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_views, 300,
        "older post with zero boundary snapshot counts all current views"
    );
}

/// Multiple posts across different time ranges with varying snapshot coverage.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn multiple_posts_range_sum(pool: PgPool) {
    // Post A: created 60 days ago, has snapshots, current views=1000
    insert_post(&pool, "pA", 60, 1000).await;
    insert_snapshot(&pool, "pA", 40, 400).await;
    insert_snapshot(&pool, "pA", 10, 800).await;

    // Post B: created 5 days ago, current views=200
    insert_post(&pool, "pB", 5, 200).await;
    insert_snapshot(&pool, "pB", 3, 100).await;

    // For a 30-day range:
    // Post A: created before range, boundary snapshot at 40d (views=400) → 1000-400 = 600
    // Post B: created within range → all 200 views
    // Total = 800
    let boundary = Utc::now() - Duration::days(30);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(range_views, 800, "mixed posts across range boundary");
}

/// "All time" range sum must match SUM(views) FROM posts exactly.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_all_matches_posts_sum(pool: PgPool) {
    insert_post(&pool, "p1", 100, 5000).await;
    insert_post(&pool, "p2", 50, 3000).await;
    insert_post(&pool, "p3", 10, 1000).await;
    // Only some posts have snapshots
    insert_snapshot(&pool, "p1", 80, 2000).await;
    insert_snapshot(&pool, "p3", 5, 500).await;

    let (auth_total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(auth_total, 9000);

    // Using a very old boundary effectively means "all time"
    let boundary = Utc::now() - Duration::days(3650);
    let (range_total,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_total, auth_total,
        "all-time range sum must equal SUM(views) FROM posts"
    );
}

/// Posts with no snapshots but outside the time range should still contribute
/// their full views (no boundary snapshot means COALESCE gives 0).
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_old_post_no_snapshots(pool: PgPool) {
    // Post created 90 days ago with 2000 views, no snapshots at all
    insert_post(&pool, "p1", 90, 2000).await;

    // 30-day range: post is older, no boundary snapshot → GREATEST(2000 - 0, 0) = 2000
    let boundary = Utc::now() - Duration::days(30);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_views, 2000,
        "old post with no snapshots: all views attributed to range (conservative)"
    );
}

/// Empty database should return 0 for both total and range queries.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn empty_database_returns_zero(pool: PgPool) {
    let (total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 0);

    let (delta,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    assert_eq!(delta, 0);

    let boundary = Utc::now() - Duration::days(30);
    let (range,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(range, 0);
}
