//! SQLite persistence (sessions, laps, stints, radio events, track outlines).

use crate::state::{LapRow, SessionRow};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListItem {
    pub id: i64,
    pub session_uid: String,
    pub game_year: i64,
    pub track_id: i64,
    pub track_name: String,
    pub session_type: String,
    pub total_laps: i64,
    pub started_at: String,
    pub lap_count: i64,
    /// The player's car (unknown for sessions recorded before it was stored).
    pub player_car: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioRow {
    pub at_ms: i64,
    pub priority: String,
    pub source: String,
    pub text: String,
    pub context: Option<String>,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackOutline {
    pub track_id: i64,
    pub length_m: f64,
    pub points: Vec<[f32; 2]>,
}

impl Db {
    /// Open (creating if needed) the database file and run migrations.
    /// Pass `":memory:"` for an in-memory database.
    pub async fn open(path: &str) -> anyhow::Result<Self> {
        let opts = if path == ":memory:" {
            SqliteConnectOptions::from_str("sqlite::memory:")?
        } else {
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            SqliteConnectOptions::new().filename(path).create_if_missing(true).journal_mode(SqliteJournalMode::Wal)
        };
        let max = if path == ":memory:" { 1 } else { 4 };
        let pool = SqlitePoolOptions::new().max_connections(max).connect_with(opts).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn create_session(&self, s: &SessionRow) -> anyhow::Result<i64> {
        // Re-use the row if the same session UID was seen before (e.g. daemon restart mid-session).
        if let Some(row) = sqlx::query("SELECT id FROM sessions WHERE session_uid = ? AND session_type = ?")
            .bind(&s.session_uid)
            .bind(&s.session_type)
            .fetch_optional(&self.pool)
            .await?
        {
            return Ok(row.get(0));
        }
        let id = sqlx::query(
            "INSERT INTO sessions (session_uid, game_year, track_id, track_name, session_type, total_laps, started_at, player_car)
             VALUES (?, ?, ?, ?, ?, ?, datetime('now'), ?)",
        )
        .bind(&s.session_uid)
        .bind(s.game_year as i64)
        .bind(s.track_id as i64)
        .bind(&s.track_name)
        .bind(&s.session_type)
        .bind(s.total_laps as i64)
        .bind(s.player_car as i64)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    pub async fn insert_lap(&self, session_id: i64, r: &LapRow) -> anyhow::Result<()> {
        let w = r.wear.unwrap_or([f32::NAN; 4]);
        let wear = |i: usize| if r.wear.is_some() { Some(w[i] as f64) } else { None };
        sqlx::query(
            "INSERT OR REPLACE INTO laps (session_id, car_index, driver, lap, lap_ms, s1_ms, s2_ms, s3_ms, compound, tyre_age,
               wear_fl, wear_fr, wear_rl, wear_rr, fuel_kg, ers_harvest_j, ers_deploy_j, valid, position, session_time)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(r.car_index as i64)
        .bind(&r.driver)
        .bind(r.lap as i64)
        .bind(r.lap_ms as i64)
        .bind(r.s1_ms.map(i64::from))
        .bind(r.s2_ms.map(i64::from))
        .bind(r.s3_ms.map(i64::from))
        .bind(&r.compound)
        .bind(r.tyre_age.map(i64::from))
        .bind(wear(0))
        .bind(wear(1))
        .bind(wear(2))
        .bind(wear(3))
        .bind(r.fuel_kg.map(f64::from))
        .bind(r.ers_harvest_j.map(f64::from))
        .bind(r.ers_deploy_j.map(f64::from))
        .bind(r.valid)
        .bind(r.position as i64)
        .bind(r.session_time as f64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Flashback: remove laps at or after `from_lap`.
    pub async fn delete_laps_from(&self, session_id: i64, from_lap: u8) -> anyhow::Result<u64> {
        Ok(sqlx::query("DELETE FROM laps WHERE session_id = ? AND lap >= ?")
            .bind(session_id)
            .bind(from_lap as i64)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    pub async fn laps(&self, session_id: i64) -> anyhow::Result<Vec<LapRow>> {
        let rows = sqlx::query(
            "SELECT car_index, driver, lap, lap_ms, s1_ms, s2_ms, s3_ms, compound, tyre_age, wear_fl, wear_fr, wear_rl, wear_rr,
                    fuel_kg, ers_harvest_j, ers_deploy_j, valid, position, session_time
             FROM laps WHERE session_id = ? ORDER BY lap, car_index",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let wfl: Option<f64> = r.get("wear_fl");
                let wear = wfl.map(|fl| {
                    [fl as f32, r.get::<Option<f64>, _>("wear_fr").unwrap_or(0.0) as f32, r.get::<Option<f64>, _>("wear_rl").unwrap_or(0.0) as f32, r.get::<Option<f64>, _>("wear_rr").unwrap_or(0.0) as f32]
                });
                LapRow {
                    car_index: r.get::<i64, _>("car_index") as u8,
                    driver: r.get("driver"),
                    lap: r.get::<i64, _>("lap") as u8,
                    lap_ms: r.get::<i64, _>("lap_ms") as u32,
                    s1_ms: r.get::<Option<i64>, _>("s1_ms").map(|v| v as u32),
                    s2_ms: r.get::<Option<i64>, _>("s2_ms").map(|v| v as u32),
                    s3_ms: r.get::<Option<i64>, _>("s3_ms").map(|v| v as u32),
                    compound: r.get("compound"),
                    tyre_age: r.get::<Option<i64>, _>("tyre_age").map(|v| v as u8),
                    wear,
                    fuel_kg: r.get::<Option<f64>, _>("fuel_kg").map(|v| v as f32),
                    ers_harvest_j: r.get::<Option<f64>, _>("ers_harvest_j").map(|v| v as f32),
                    ers_deploy_j: r.get::<Option<f64>, _>("ers_deploy_j").map(|v| v as f32),
                    valid: r.get("valid"),
                    position: r.get::<i64, _>("position") as u8,
                    session_time: r.get::<f64, _>("session_time") as f32,
                }
            })
            .collect())
    }

    pub async fn list_sessions(&self, limit: i64) -> anyhow::Result<Vec<SessionListItem>> {
        self.sessions_where("1 = 1", -1, limit).await
    }

    pub async fn session(&self, id: i64) -> anyhow::Result<Option<SessionListItem>> {
        Ok(self.sessions_where("s.id = ?", id, 1).await?.into_iter().next())
    }

    async fn sessions_where(&self, cond: &str, id: i64, limit: i64) -> anyhow::Result<Vec<SessionListItem>> {
        let sql = format!(
            "SELECT s.id, s.session_uid, s.game_year, s.track_id, s.track_name, s.session_type, s.total_laps, s.started_at, s.player_car,
                    (SELECT COUNT(DISTINCT lap) FROM laps l WHERE l.session_id = s.id) AS lap_count
             FROM sessions s WHERE {cond} ORDER BY s.id DESC LIMIT ?"
        );
        let mut q = sqlx::query(&sql);
        if id >= 0 {
            q = q.bind(id);
        }
        let rows = q.bind(limit).fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| SessionListItem {
                id: r.get("id"),
                session_uid: r.get("session_uid"),
                game_year: r.get("game_year"),
                track_id: r.get("track_id"),
                track_name: r.get("track_name"),
                session_type: r.get("session_type"),
                total_laps: r.get("total_laps"),
                started_at: r.get("started_at"),
                lap_count: r.get("lap_count"),
                player_car: r.get("player_car"),
            })
            .collect())
    }

    pub async fn insert_radio(&self, session_id: Option<i64>, r: &RadioRow) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO radio_events (session_id, at_ms, priority, source, text, context, outcome) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(session_id)
            .bind(r.at_ms)
            .bind(&r.priority)
            .bind(&r.source)
            .bind(&r.text)
            .bind(&r.context)
            .bind(&r.outcome)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn radio_events(&self, session_id: i64) -> anyhow::Result<Vec<RadioRow>> {
        let rows = sqlx::query("SELECT at_ms, priority, source, text, context, outcome FROM radio_events WHERE session_id = ? ORDER BY id")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| RadioRow {
                at_ms: r.get("at_ms"),
                priority: r.get("priority"),
                source: r.get("source"),
                text: r.get("text"),
                context: r.get("context"),
                outcome: r.get("outcome"),
            })
            .collect())
    }

    pub async fn upsert_outline(&self, track_id: i8, length_m: f32, points: &[[f32; 2]]) -> anyhow::Result<()> {
        let json = serde_json::to_string(points)?;
        sqlx::query("INSERT OR REPLACE INTO track_outlines (track_id, length_m, points, updated_at) VALUES (?, ?, ?, datetime('now'))")
            .bind(track_id as i64)
            .bind(length_m as f64)
            .bind(json)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn outline(&self, track_id: i64) -> anyhow::Result<Option<TrackOutline>> {
        let row = sqlx::query("SELECT track_id, length_m, points FROM track_outlines WHERE track_id = ?").bind(track_id).fetch_optional(&self.pool).await?;
        Ok(match row {
            Some(r) => Some(TrackOutline {
                track_id: r.get("track_id"),
                length_m: r.get("length_m"),
                points: serde_json::from_str(r.get::<&str, _>("points"))?,
            }),
            None => None,
        })
    }

    pub async fn delete_outline(&self, track_id: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM track_outlines WHERE track_id = ?").bind(track_id).execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(car: u8, lap: u8) -> LapRow {
        LapRow {
            car_index: car,
            driver: "TST".into(),
            lap,
            lap_ms: 90_000 + lap as u32,
            s1_ms: Some(30_000),
            s2_ms: None,
            s3_ms: Some(30_000),
            compound: Some("M".into()),
            tyre_age: Some(lap),
            wear: Some([1.0, 2.0, 3.0, 4.0]),
            fuel_kg: Some(50.0),
            ers_harvest_j: None,
            ers_deploy_j: Some(1.0),
            valid: true,
            position: 3,
            session_time: 100.0,
        }
    }

    #[tokio::test]
    async fn session_laps_flashback_outline() {
        let db = Db::open(":memory:").await.unwrap();
        let s = SessionRow { session_uid: "abc".into(), game_year: 24, track_id: 3, track_name: "Sakhir".into(), session_type: "Race".into(), total_laps: 10, player_car: 3 };
        let id = db.create_session(&s).await.unwrap();
        assert_eq!(db.create_session(&s).await.unwrap(), id, "same uid re-uses row");
        for l in 1..=5 {
            db.insert_lap(id, &lap(0, l)).await.unwrap();
            db.insert_lap(id, &lap(1, l)).await.unwrap();
        }
        db.insert_lap(id, &lap(0, 5)).await.unwrap(); // replace, not duplicate
        let laps = db.laps(id).await.unwrap();
        assert_eq!(laps.len(), 10);
        assert_eq!(laps[0], lap(0, 1));
        assert_eq!(db.delete_laps_from(id, 4).await.unwrap(), 4);
        assert_eq!(db.laps(id).await.unwrap().len(), 6);
        let sessions = db.list_sessions(10).await.unwrap();
        assert_eq!(sessions[0].lap_count, 3);

        assert!(db.outline(3).await.unwrap().is_none());
        db.upsert_outline(3, 5400.0, &[[0.0, 1.0], [2.0, 3.0]]).await.unwrap();
        assert_eq!(db.outline(3).await.unwrap().unwrap().points.len(), 2);

        db.insert_radio(Some(id), &RadioRow { at_ms: 5, priority: "P0".into(), source: "rule".into(), text: "Safety car".into(), context: None, outcome: "spoken".into() })
            .await
            .unwrap();
        assert_eq!(db.radio_events(id).await.unwrap().len(), 1);
    }
}
