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

/// The delta query used by both the chart (get_views_from_snapshots) and range sums.
/// Uses MAX high-water mark to handle API glitches (temporary view drops).
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

/// The authoritative total views query used by get_analytics() for the header.
const TOTAL_VIEWS_QUERY: &str = "SELECT COALESCE(SUM(views), 0)::bigint FROM posts";

/// The range-sum query used by get_views_range_sums — delta-based with conditional aggregation.
const RANGE_SUM_QUERY: &str = r#"
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
    SELECT COALESCE(SUM(CASE WHEN effective_date >= $1 THEN view_delta END), 0)::bigint
    FROM with_deltas
"#;

/// The chart query used by get_views_from_snapshots — returns daily deltas.
const CHART_QUERY: &str = r#"
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
    SELECT DATE(effective_date)::text AS date,
           SUM(view_delta)::bigint AS total_views
    FROM with_deltas
    GROUP BY DATE(effective_date)
    ORDER BY date
"#;

// ===========================================================================
// Layer 1: How views data is stored (sync → database)
// ===========================================================================

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn sync_stores_views_in_posts_table(pool: PgPool) {
    insert_post(&pool, "p1", 10, 0).await;

    // Simulate sync updating views
    sqlx::query("UPDATE posts SET views = 500 WHERE id = $1")
        .bind("p1")
        .execute(&pool)
        .await
        .unwrap();

    let (views,): (i32,) = sqlx::query_as("SELECT views FROM posts WHERE id = $1")
        .bind("p1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(views, 500, "sync should update posts.views");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn sync_creates_engagement_snapshot_with_views(pool: PgPool) {
    insert_post(&pool, "p1", 10, 500).await;
    insert_snapshot(&pool, "p1", 5, 500).await;

    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM engagement_snapshots WHERE post_id = $1")
            .bind("p1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "snapshot should be created");

    let (snap_views,): (i32,) =
        sqlx::query_as("SELECT views FROM engagement_snapshots WHERE post_id = $1")
            .bind("p1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(snap_views, 500, "snapshot should capture current views");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn multiple_snapshots_track_views_over_time(pool: PgPool) {
    insert_post(&pool, "p1", 30, 500).await;
    insert_snapshot(&pool, "p1", 25, 100).await;
    insert_snapshot(&pool, "p1", 15, 300).await;
    insert_snapshot(&pool, "p1", 5, 500).await;

    let rows: Vec<(i32,)> = sqlx::query_as(
        "SELECT views FROM engagement_snapshots WHERE post_id = $1 ORDER BY captured_at",
    )
    .bind("p1")
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, 100);
    assert_eq!(rows[1].0, 300);
    assert_eq!(rows[2].0, 500);
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn migration_004_zero_backfill_scenario(pool: PgPool) {
    // Simulates the state after migration 004: a post with an old snapshot
    // that was backfilled with views=0, followed by a real snapshot.
    insert_post(&pool, "p1", 60, 1000).await;
    insert_snapshot(&pool, "p1", 50, 0).await; // migration 004 backfill
    insert_snapshot(&pool, "p1", 5, 1000).await; // real snapshot post-migration

    let rows: Vec<(i32,)> = sqlx::query_as(
        "SELECT views FROM engagement_snapshots WHERE post_id = $1 ORDER BY captured_at",
    )
    .bind("p1")
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].0, 0,
        "old snapshot backfilled with 0 by migration 004"
    );
    assert_eq!(rows[1].0, 1000, "real snapshot has actual views");
}

// ===========================================================================
// Layer 2: How sums are calculated (delta queries)
// ===========================================================================

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_basic_sum(pool: PgPool) {
    insert_post(&pool, "p1", 10, 500).await;
    insert_snapshot(&pool, "p1", 9, 0).await;
    insert_snapshot(&pool, "p1", 5, 200).await;
    insert_snapshot(&pool, "p1", 1, 500).await;

    let (total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Row 1 (0):   prev=NULL, delta=0
    // Row 2 (200): prev=MAX(0)=0, delta=200
    // Row 3 (500): prev=MAX(0,200)=200, delta=300
    // Total: 500
    assert_eq!(total, 500, "delta sum should equal final views");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_handles_zero_backfilled_snapshots(pool: PgPool) {
    // KEY REGRESSION TEST: This is the exact scenario that broke the
    // boundary-based approach. Migration 004 backfilled old snapshots with
    // views=0. The delta approach correctly computes delta=0 for the zero
    // snapshot and delta=1000 for the real one.
    insert_post(&pool, "p1", 60, 1000).await;
    insert_snapshot(&pool, "p1", 50, 0).await; // migration 004 backfill
    insert_snapshot(&pool, "p1", 5, 1000).await; // real snapshot

    let (total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Row 1 (0):    prev=NULL, delta=0 (attributed to post timestamp, 60d ago)
    // Row 2 (1000): prev=MAX(0)=0, delta=1000 (attributed to post timestamp because prev=0)
    assert_eq!(
        total, 1000,
        "delta approach correctly handles zero-backfilled snapshots"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_handles_api_glitches(pool: PgPool) {
    // API sometimes returns temporarily lower values; MAX-based prev prevents
    // negative deltas from corrupting the sum.
    insert_post(&pool, "p1", 10, 150).await;
    insert_snapshot(&pool, "p1", 9, 100).await;
    insert_snapshot(&pool, "p1", 5, 80).await; // glitch
    insert_snapshot(&pool, "p1", 1, 150).await;

    let (total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    // Row 1 (100): prev=NULL, delta=100
    // Row 2 (80):  prev=MAX(100)=100, delta=GREATEST(80-100,0)=0
    // Row 3 (150): prev=MAX(100,80)=100, delta=50
    // Total: 150
    assert_eq!(total, 150, "MAX-based deltas handle API glitches correctly");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_attributes_initial_views_to_post_date(pool: PgPool) {
    // First snapshot's delta should be attributed to the post's creation date,
    // not the snapshot's capture date. This ensures views are correctly placed
    // in time ranges.
    insert_post(&pool, "p1", 30, 500).await;
    insert_snapshot(&pool, "p1", 25, 200).await; // first snapshot
    insert_snapshot(&pool, "p1", 5, 500).await;

    // Check with a 28-day boundary: should include the initial 200 views
    // because they're attributed to post date (30d ago), which is before 28d ago.
    // Wait — effective_date for first snapshot = post_timestamp (30d ago).
    // 28d boundary: effective_date (30d ago) < boundary (28d ago), so NOT included.
    let boundary = Utc::now() - Duration::days(28);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    // Only the 200→500 delta (captured 5d ago) is within the 28d range.
    assert_eq!(
        range_views, 300,
        "initial views attributed to post date, outside 28d range"
    );

    // But a 31-day boundary should include everything
    let boundary = Utc::now() - Duration::days(31);
    let (range_views,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        range_views, 500,
        "31d range includes initial views attributed to post date (30d ago)"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_empty_database(pool: PgPool) {
    let (total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    assert_eq!(total, 0);
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn delta_query_post_with_no_snapshots(pool: PgPool) {
    // Posts without snapshots contribute 0 to delta-based sums.
    // This is an acceptable trade-off: the authoritative total_views
    // (SUM(views) FROM posts) still counts them in the header.
    insert_post(&pool, "p1", 10, 500).await;

    let (delta,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    assert_eq!(delta, 0, "no snapshots means no deltas");

    let (auth,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        auth, 500,
        "authoritative total still includes posts without snapshots"
    );
}

// ===========================================================================
// Layer 3: Range sum calculation (the actual bug fix)
// ===========================================================================

/// MOST IMPORTANT TEST: Prevents the class of bugs where all range buttons
/// show the same value. If this test passes, ranges are working correctly
/// regardless of migration history or snapshot data quality.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sums_are_monotonically_decreasing(pool: PgPool) {
    // Post A: old post, gains views over time
    insert_post(&pool, "pA", 90, 1000).await;
    insert_snapshot(&pool, "pA", 80, 100).await;
    insert_snapshot(&pool, "pA", 40, 500).await;
    insert_snapshot(&pool, "pA", 10, 900).await;
    insert_snapshot(&pool, "pA", 0, 1000).await;

    // Post B: medium-age post
    insert_post(&pool, "pB", 20, 800).await;
    insert_snapshot(&pool, "pB", 15, 200).await;
    insert_snapshot(&pool, "pB", 5, 600).await;
    insert_snapshot(&pool, "pB", 0, 800).await;

    // Post C: very recent post
    insert_post(&pool, "pC", 3, 100).await;
    insert_snapshot(&pool, "pC", 2, 50).await;
    insert_snapshot(&pool, "pC", 0, 100).await;

    let now = Utc::now();
    let boundaries = [
        ("365d", 365),
        ("270d", 270),
        ("180d", 180),
        ("90d", 90),
        ("60d", 60),
        ("30d", 30),
        ("14d", 14),
        ("7d", 7),
        ("24h", 1),
    ];

    let (all_time,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();

    let mut prev_value = all_time;
    let mut prev_label = "all";

    for (label, days) in boundaries {
        let boundary = now - Duration::days(days);
        let (value,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
            .bind(boundary)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            value <= prev_value,
            "{label} ({value}) should be <= {prev_label} ({prev_value})"
        );
        prev_value = value;
        prev_label = label;
    }

    // Strict inequality: all-time must be greater than 24h
    let boundary_1d = now - Duration::days(1);
    let (d1,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_1d)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        all_time > d1,
        "all-time ({all_time}) must be strictly greater than 24h ({d1})"
    );
}

/// DIRECT REGRESSION TEST for migration 004: a post with a zero-backfilled
/// snapshot must NOT inflate range sums to equal the all-time total.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_migration_004_does_not_inflate(pool: PgPool) {
    insert_post(&pool, "p1", 60, 2000).await;
    insert_snapshot(&pool, "p1", 50, 0).await; // migration 004 backfill
    insert_snapshot(&pool, "p1", 5, 1500).await;
    insert_snapshot(&pool, "p1", 0, 2000).await;

    let (all_time,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();

    // 30d range should only count the 1500→2000 gain, NOT the full 2000
    let boundary_30d = Utc::now() - Duration::days(30);
    let (d30,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_30d)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(d30, 500, "30d range should only count gains within 30 days");
    assert!(
        d30 < all_time,
        "30d ({d30}) must be less than all-time ({all_time}) — this was the bug"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_new_post_counts_all_views(pool: PgPool) {
    // Post created 5 days ago — should be fully included in 30d and 7d ranges
    insert_post(&pool, "p1", 5, 800).await;
    insert_snapshot(&pool, "p1", 4, 200).await;
    insert_snapshot(&pool, "p1", 2, 600).await;
    insert_snapshot(&pool, "p1", 0, 800).await;

    let boundary_30d = Utc::now() - Duration::days(30);
    let (d30,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_30d)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(d30, 800, "post within 30d range counts all views");

    let boundary_7d = Utc::now() - Duration::days(7);
    let (d7,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_7d)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(d7, 800, "post within 7d range counts all views");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_old_post_only_counts_gains_in_range(pool: PgPool) {
    insert_post(&pool, "p1", 100, 900).await;
    insert_snapshot(&pool, "p1", 90, 500).await;
    insert_snapshot(&pool, "p1", 20, 800).await;
    insert_snapshot(&pool, "p1", 5, 900).await;

    // 30d: only the 800→900 delta (captured at 5d) is within range
    let boundary_30d = Utc::now() - Duration::days(30);
    let (d30,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_30d)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        d30, 100,
        "30d should only count the 800→900 gain captured at 5d ago"
    );

    // 95d: includes both the 500→800 delta (at 20d) and 800→900 delta (at 5d)
    // but NOT the initial 500 (attributed to post date 100d ago, outside 95d)
    let boundary_95d = Utc::now() - Duration::days(95);
    let (d95,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_95d)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(d95, 400, "95d range includes more deltas than 30d");
    assert!(d95 > d30, "wider range should include more views");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_empty_database(pool: PgPool) {
    let boundary = Utc::now() - Duration::days(30);
    let (range,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(range, 0);

    let (delta,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();
    assert_eq!(delta, 0);

    let (total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 0);
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_all_time_equals_delta_total(pool: PgPool) {
    insert_post(&pool, "p1", 100, 5000).await;
    insert_snapshot(&pool, "p1", 90, 2000).await;
    insert_snapshot(&pool, "p1", 30, 4000).await;
    insert_snapshot(&pool, "p1", 0, 5000).await;

    insert_post(&pool, "p2", 50, 3000).await;
    insert_snapshot(&pool, "p2", 40, 1000).await;
    insert_snapshot(&pool, "p2", 10, 3000).await;

    let (delta_total,): (i64,) = sqlx::query_as(DELTA_QUERY).fetch_one(&pool).await.unwrap();

    // Very old boundary = effectively "all time"
    let boundary = Utc::now() - Duration::days(3650);
    let (range_total,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        range_total, delta_total,
        "all-time range sum must equal delta total"
    );
}

// ===========================================================================
// Layer 4: Chart-to-button consistency (end-to-end)
// ===========================================================================

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn chart_sum_matches_range_button_for_30d(pool: PgPool) {
    // Create posts spanning 60 days so some data is inside and outside the range
    insert_post(&pool, "p1", 50, 1000).await;
    insert_snapshot(&pool, "p1", 45, 200).await;
    insert_snapshot(&pool, "p1", 20, 600).await;
    insert_snapshot(&pool, "p1", 5, 1000).await;

    insert_post(&pool, "p2", 10, 500).await;
    insert_snapshot(&pool, "p2", 8, 100).await;
    insert_snapshot(&pool, "p2", 2, 500).await;

    // Get chart data points (daily deltas)
    let chart_rows: Vec<(String, i64)> =
        sqlx::query_as(CHART_QUERY).fetch_all(&pool).await.unwrap();

    // Filter to last 30 days and sum
    let cutoff = (Utc::now() - Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let chart_sum: i64 = chart_rows
        .iter()
        .filter(|(date, _)| date.as_str() >= cutoff.as_str())
        .map(|(_, views)| views)
        .sum();

    // Get range button value for 30d
    let boundary_30d = Utc::now() - Duration::days(30);
    let (range_sum,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_30d)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        chart_sum, range_sum,
        "chart area sum for 30d must equal range button value"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn chart_sum_matches_range_button_for_7d(pool: PgPool) {
    insert_post(&pool, "p1", 20, 800).await;
    insert_snapshot(&pool, "p1", 15, 200).await;
    insert_snapshot(&pool, "p1", 5, 600).await;
    insert_snapshot(&pool, "p1", 1, 800).await;

    // Chart data
    let chart_rows: Vec<(String, i64)> =
        sqlx::query_as(CHART_QUERY).fetch_all(&pool).await.unwrap();

    let cutoff = (Utc::now() - Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let chart_sum: i64 = chart_rows
        .iter()
        .filter(|(date, _)| date.as_str() >= cutoff.as_str())
        .map(|(_, views)| views)
        .sum();

    // Range button value
    let boundary_7d = Utc::now() - Duration::days(7);
    let (range_sum,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_7d)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        chart_sum, range_sum,
        "chart area sum for 7d must equal range button value"
    );
}

// ===========================================================================
// Layer 5: Frontend contract (API response format)
// ===========================================================================

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sums_returns_all_expected_keys(pool: PgPool) {
    // Even with no data, the full query should return values for all ranges
    insert_post(&pool, "p1", 10, 100).await;
    insert_snapshot(&pool, "p1", 5, 100).await;

    let now = Utc::now();
    let boundaries: Vec<chrono::DateTime<Utc>> = [365, 270, 180, 90, 60, 30, 14, 7, 1]
        .iter()
        .map(|d| now - Duration::days(*d))
        .collect();

    // Run the full multi-column query (same as production get_views_range_sums)
    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at, es.post_id, es.views,
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
           SELECT
               COALESCE(SUM(view_delta), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $1 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $2 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $3 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $4 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $5 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $6 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $7 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $8 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $9 THEN view_delta END), 0)::bigint
           FROM with_deltas"#,
    )
    .bind(boundaries[0])
    .bind(boundaries[1])
    .bind(boundaries[2])
    .bind(boundaries[3])
    .bind(boundaries[4])
    .bind(boundaries[5])
    .bind(boundaries[6])
    .bind(boundaries[7])
    .bind(boundaries[8])
    .fetch_one(&pool)
    .await
    .unwrap();

    // All 10 columns should be returned (no NULLs, no errors)
    let keys = [
        "all", "365d", "270d", "180d", "90d", "60d", "30d", "14d", "7d", "24h",
    ];
    let values = [
        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9,
    ];
    let mut sums = std::collections::HashMap::new();
    for (key, val) in keys.iter().zip(values.iter()) {
        sums.insert(key.to_string(), *val);
    }
    assert_eq!(sums.len(), 10, "all 10 range keys must be present");
    for key in keys {
        assert!(sums.contains_key(key), "missing expected key: {key}");
    }
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn total_views_from_posts_table(pool: PgPool) {
    insert_post(&pool, "p1", 30, 1000).await;
    insert_post(&pool, "p2", 20, 2500).await;
    insert_post(&pool, "p3", 10, 500).await;

    let (total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 4000, "header total_views = SUM(views) FROM posts");
}
