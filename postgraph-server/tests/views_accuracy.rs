use chrono::{Duration, NaiveDate, Utc};
use sqlx::PgPool;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Insert a row into daily_views.
async fn insert_daily_views(pool: &PgPool, date: NaiveDate, views: i64) {
    sqlx::query(
        "INSERT INTO daily_views (date, views, source, fetched_at) VALUES ($1, $2, 'test', NOW())",
    )
    .bind(date)
    .bind(views)
    .execute(pool)
    .await
    .unwrap();
}

/// Insert a post with a given id and timestamp.
async fn insert_post(pool: &PgPool, id: &str, days_ago: i64) {
    let ts = Utc::now() - Duration::days(days_ago);
    sqlx::query("INSERT INTO posts (id, text, timestamp, synced_at) VALUES ($1, $2, $3, NOW())")
        .bind(id)
        .bind(format!("Test post {id}"))
        .bind(ts)
        .execute(pool)
        .await
        .unwrap();
}

/// Insert an engagement snapshot attributed to a specific capture time.
async fn insert_snapshot(pool: &PgPool, post_id: &str, days_ago: i64, likes: i32) {
    let captured = Utc::now() - Duration::days(days_ago);
    sqlx::query(
        "INSERT INTO engagement_snapshots (id, post_id, captured_at, likes) VALUES (gen_random_uuid(), $1, $2, $3)",
    )
    .bind(post_id)
    .bind(captured)
    .bind(likes)
    .execute(pool)
    .await
    .unwrap();
}

// ── The production queries (mirrors analytics.rs) ────────────────────────────

/// Total views: SUM of all daily_views rows.
const TOTAL_VIEWS_QUERY: &str = "SELECT COALESCE(SUM(views), 0)::bigint FROM daily_views";

/// Single-range sum: views on or after $1 (a NaiveDate).
const RANGE_SUM_QUERY: &str =
    "SELECT COALESCE(SUM(CASE WHEN date >= $1 THEN views END), 0)::bigint FROM daily_views";

// ===========================================================================
// 1. Daily views upsert
// ===========================================================================

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn daily_views_insert(pool: PgPool) {
    let today = Utc::now().date_naive();
    insert_daily_views(&pool, today, 1_234).await;

    let (views,): (i64,) = sqlx::query_as("SELECT views FROM daily_views WHERE date = $1")
        .bind(today)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        views, 1_234,
        "inserted row should have the correct view count"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn daily_views_upsert_updates_existing_date(pool: PgPool) {
    let today = Utc::now().date_naive();

    // First insert
    insert_daily_views(&pool, today, 500).await;

    // Upsert with a new value — same as the production upsert_daily_views helper
    sqlx::query(
        r#"INSERT INTO daily_views (date, views, fetched_at)
           VALUES ($1, $2, NOW())
           ON CONFLICT (date) DO UPDATE SET views = $2, fetched_at = NOW()"#,
    )
    .bind(today)
    .bind(800_i64)
    .execute(&pool)
    .await
    .unwrap();

    let (views,): (i64,) = sqlx::query_as("SELECT views FROM daily_views WHERE date = $1")
        .bind(today)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        views, 800,
        "upsert should update views for an existing date"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn daily_views_upsert_does_not_duplicate_date(pool: PgPool) {
    let today = Utc::now().date_naive();

    for _ in 0..3 {
        sqlx::query(
            r#"INSERT INTO daily_views (date, views, fetched_at)
               VALUES ($1, $2, NOW())
               ON CONFLICT (date) DO UPDATE SET views = $2, fetched_at = NOW()"#,
        )
        .bind(today)
        .bind(100_i64)
        .execute(&pool)
        .await
        .unwrap();
    }

    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM daily_views WHERE date = $1")
            .bind(today)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        count, 1,
        "repeated upserts should not create duplicate rows"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn daily_views_empty_table_returns_zero(pool: PgPool) {
    let (total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(total, 0, "empty table should return 0 total views");
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn daily_views_total_sums_all_rows(pool: PgPool) {
    let today = Utc::now().date_naive();

    insert_daily_views(&pool, today - chrono::Duration::days(2), 1_000).await;
    insert_daily_views(&pool, today - chrono::Duration::days(1), 2_000).await;
    insert_daily_views(&pool, today, 3_000).await;

    let (total,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        total, 6_000,
        "total should be the sum of all daily_views rows"
    );
}

// ===========================================================================
// 2. Range sums monotonicity
// ===========================================================================

/// MOST IMPORTANT TEST: Prevents the class of bugs where all range buttons
/// show the same value. If this passes, range sums are working correctly.
#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sums_are_monotonically_decreasing(pool: PgPool) {
    let today = Utc::now().date_naive();

    // Spread views across 400 days to ensure every range band has data
    for offset in [0i64, 1, 5, 10, 20, 40, 60, 100, 180, 270, 365, 390] {
        insert_daily_views(&pool, today - chrono::Duration::days(offset), 100).await;
    }

    let (all_time,): (i64,) = sqlx::query_as(TOTAL_VIEWS_QUERY)
        .fetch_one(&pool)
        .await
        .unwrap();

    let boundaries: &[(&str, i64)] = &[
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

    let mut prev_value = all_time;
    let mut prev_label = "all";

    for (label, days) in boundaries {
        let boundary = today - chrono::Duration::days(*days);
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

    // Strict inequality: all-time must be strictly greater than 24h
    let boundary_1d = today - chrono::Duration::days(1);
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

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_excludes_data_outside_window(pool: PgPool) {
    let today = Utc::now().date_naive();

    // Only insert views older than 30 days
    insert_daily_views(&pool, today - chrono::Duration::days(60), 500).await;
    insert_daily_views(&pool, today - chrono::Duration::days(45), 500).await;

    let boundary_30d = today - chrono::Duration::days(30);
    let (d30,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary_30d)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        d30, 0,
        "data older than 30d should not appear in 30d range sum"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sum_includes_data_on_boundary_date(pool: PgPool) {
    let today = Utc::now().date_naive();

    // Insert a row exactly on the 30d boundary
    let boundary = today - chrono::Duration::days(30);
    insert_daily_views(&pool, boundary, 999).await;
    insert_daily_views(&pool, today, 1).await;

    let (d30,): (i64,) = sqlx::query_as(RANGE_SUM_QUERY)
        .bind(boundary)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        d30, 1_000,
        "boundary date row should be included (>= comparison)"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn range_sums_production_query_returns_all_columns(pool: PgPool) {
    let today = Utc::now().date_naive();

    // Spread data across the full year
    for offset in [0i64, 10, 30, 60, 90, 180, 270, 365] {
        insert_daily_views(&pool, today - chrono::Duration::days(offset), 50).await;
    }

    let b365 = today - chrono::Duration::days(365);
    let b270 = today - chrono::Duration::days(270);
    let b180 = today - chrono::Duration::days(180);
    let b90 = today - chrono::Duration::days(90);
    let b60 = today - chrono::Duration::days(60);
    let b30 = today - chrono::Duration::days(30);
    let b14 = today - chrono::Duration::days(14);
    let b7 = today - chrono::Duration::days(7);
    let b1 = today - chrono::Duration::days(1);

    // This mirrors the exact production query in get_views_range_sums
    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
               COALESCE(SUM(views), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $1 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $2 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $3 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $4 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $5 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $6 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $7 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $8 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $9 THEN views END), 0)::bigint
           FROM daily_views"#,
    )
    .bind(b365)
    .bind(b270)
    .bind(b180)
    .bind(b90)
    .bind(b60)
    .bind(b30)
    .bind(b14)
    .bind(b7)
    .bind(b1)
    .fetch_one(&pool)
    .await
    .unwrap();

    let values = [
        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9,
    ];

    // All 10 columns must be present and non-negative
    assert_eq!(
        values.len(),
        10,
        "production query must return 10 range columns"
    );
    for v in &values {
        assert!(*v >= 0, "no range sum should be negative");
    }

    // Monotonically non-increasing: all >= 365d >= 270d >= ... >= 24h
    for i in 0..values.len() - 1 {
        assert!(
            values[i] >= values[i + 1],
            "values[{i}]={} should be >= values[{}]={}",
            values[i],
            i + 1,
            values[i + 1]
        );
    }
}

// ===========================================================================
// 3. Engagement capture-time attribution
// ===========================================================================

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn engagement_deltas_attributed_to_captured_at(pool: PgPool) {
    // Post was published 60 days ago but snapshots were captured recently.
    // Engagement deltas should be bucketed by captured_at, not post timestamp.
    insert_post(&pool, "p1", 60).await;
    insert_snapshot(&pool, "p1", 10, 100).await; // first snapshot: 100 likes
    insert_snapshot(&pool, "p1", 5, 300).await; // +200 likes, captured 5d ago
    insert_snapshot(&pool, "p1", 1, 500).await; // +200 likes, captured 1d ago

    // Query using captured_at attribution (mirrors production get_engagement)
    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      MAX(es.likes) OVER (
                          PARTITION BY es.post_id
                          ORDER BY es.captured_at
                          ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
                      ) AS prev_likes
               FROM engagement_snapshots es
               WHERE es.post_id = $1
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta
               FROM ordered_snapshots
           )
           SELECT DATE(captured_at)::text AS date,
                  SUM(like_delta)::bigint AS total_likes
           FROM with_deltas
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .bind("p1")
    .fetch_all(&pool)
    .await
    .unwrap();

    // All deltas should be on capture dates, not the post's publish date (60d ago)
    let total_likes: i64 = rows.iter().map(|(_, v)| v).sum();
    assert_eq!(
        total_likes, 500,
        "total likes should equal final snapshot value"
    );

    let today = Utc::now().date_naive();
    let post_publish_date = (today - chrono::Duration::days(60)).to_string();

    for (date, _) in &rows {
        assert_ne!(
            date, &post_publish_date,
            "no engagement delta should be attributed to the post's publish date"
        );
    }
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn engagement_first_snapshot_attributed_to_captured_at_not_post_date(pool: PgPool) {
    // Critical distinction from old system: in the new system, the first
    // snapshot's delta is attributed to captured_at (when we observed it),
    // NOT backdated to the post's publish timestamp.
    insert_post(&pool, "p1", 30).await;
    insert_snapshot(&pool, "p1", 5, 400).await; // first and only snapshot

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      MAX(es.likes) OVER (
                          PARTITION BY es.post_id
                          ORDER BY es.captured_at
                          ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
                      ) AS prev_likes
               FROM engagement_snapshots es
               WHERE es.post_id = $1
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta
               FROM ordered_snapshots
           )
           SELECT DATE(captured_at)::text AS date,
                  SUM(like_delta)::bigint
           FROM with_deltas
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .bind("p1")
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 1, "should produce exactly one date bucket");

    let today = Utc::now().date_naive();
    let capture_date = (today - chrono::Duration::days(5)).to_string();
    let post_date = (today - chrono::Duration::days(30)).to_string();

    let (date, delta) = &rows[0];
    assert_eq!(
        date, &capture_date,
        "delta should be attributed to captured_at date ({capture_date}), not post date ({post_date})"
    );
    assert_eq!(
        *delta, 400,
        "first snapshot delta should equal the full likes count"
    );
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn engagement_api_glitch_does_not_produce_negative_delta(pool: PgPool) {
    // The API sometimes temporarily returns lower values; MAX-based window prevents
    // negative deltas from corrupting the sum.
    insert_post(&pool, "p1", 10).await;
    insert_snapshot(&pool, "p1", 9, 100).await;
    insert_snapshot(&pool, "p1", 5, 80).await; // glitch: lower than previous
    insert_snapshot(&pool, "p1", 1, 150).await;

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      MAX(es.likes) OVER (
                          PARTITION BY es.post_id
                          ORDER BY es.captured_at
                          ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
                      ) AS prev_likes
               FROM engagement_snapshots es
               WHERE es.post_id = $1
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta
               FROM ordered_snapshots
           )
           SELECT DATE(captured_at)::text AS date,
                  SUM(like_delta)::bigint
           FROM with_deltas
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .bind("p1")
    .fetch_all(&pool)
    .await
    .unwrap();

    let total: i64 = rows.iter().map(|(_, v)| v).sum();
    assert_eq!(
        total, 150,
        "MAX-based deltas should handle API glitches; total = final value"
    );

    for (_, delta) in &rows {
        assert!(*delta >= 0, "no delta should be negative");
    }
}

#[sqlx::test(migrations = "../postgraph-server/migrations")]
async fn engagement_empty_snapshots_returns_no_rows(pool: PgPool) {
    insert_post(&pool, "p1", 10).await;

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      MAX(es.likes) OVER (
                          PARTITION BY es.post_id
                          ORDER BY es.captured_at
                          ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
                      ) AS prev_likes
               FROM engagement_snapshots es
               WHERE es.post_id = $1
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta
               FROM ordered_snapshots
           )
           SELECT DATE(captured_at)::text AS date,
                  SUM(like_delta)::bigint
           FROM with_deltas
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .bind("p1")
    .fetch_all(&pool)
    .await
    .unwrap();

    assert!(
        rows.is_empty(),
        "post with no snapshots should produce no engagement rows"
    );
}
