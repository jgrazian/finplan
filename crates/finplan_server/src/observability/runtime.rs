use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;

use super::{Component, ErrorClass, JobKind, QueueSnapshot, Telemetry, metrics_router};
use crate::{analysis::AnalysisJobs, auth, billing, db::Db, state::AppState};

/// Owns observability/maintenance tasks. Dropping it aborts every task; normal
/// shutdown also drains the private listener. build() never opens a listener.
pub struct ObservabilityRuntime {
    stop: watch::Sender<bool>,
    tasks: JoinSet<io::Result<()>>,
    metrics_address: Option<SocketAddr>,
}

impl ObservabilityRuntime {
    pub async fn start(state: &AppState) -> io::Result<Self> {
        // Bind before spawning anything: an explicit occupied address fails startup.
        let listener = match state.config.metrics_bind {
            Some(address) => Some(tokio::net::TcpListener::bind(address).await.inspect_err(
                |error| {
                    tracing::error!(event = "server.listener_failed", listener = "metrics",
                    %address, class = ?error.kind(), os_error = error.raw_os_error());
                },
            )?),
            None => None,
        };
        let metrics_address = listener.as_ref().map(|l| l.local_addr()).transpose()?;
        let (stop, receive) = watch::channel(false);
        let mut tasks = JoinSet::new();
        if let Some(listener) = listener {
            let router = metrics_router(state.telemetry.clone());
            let shutdown = receive.clone();
            tasks.spawn(
                async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(stopped(shutdown))
                        .await
                }
                .with_current_subscriber(),
            );
        }
        let db = state.db.clone();
        let analyses = state.analyses.clone();
        let telemetry = state.telemetry.clone();
        let mut shutdown = receive.clone();
        tasks.spawn(
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                interval.tick().await; // build() already took the initial sample.
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => return Ok(()),
                        _ = interval.tick() => {
                            // Shutdown also cancels a blocked sample instead of waiting on SQLite.
                            tokio::select! {
                                _ = shutdown.changed() => return Ok(()),
                                _ = sample_sources(&db, &analyses, &telemetry) => {}
                            }
                        }
                    }
                }
            }
            .with_current_subscriber(),
        );
        let db = state.db.clone();
        let telemetry = state.telemetry.clone();
        let mut shutdown = receive;
        tasks.spawn(
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(3600));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                interval.tick().await;
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => return Ok(()),
                        _ = interval.tick() => {
                            tokio::select! {
                                _ = shutdown.changed() => return Ok(()),
                                _ = purge_sessions(&db, &telemetry) => {}
                            }
                        }
                    }
                }
            }
            .with_current_subscriber(),
        );
        Ok(Self {
            stop,
            tasks,
            metrics_address,
        })
    }

    pub fn metrics_address(&self) -> Option<SocketAddr> {
        self.metrics_address
    }

    /// A task returning while the server is still running is a service failure.
    pub async fn wait_for_failure(&mut self) -> io::Error {
        match self.tasks.join_next().await {
            Some(Ok(Err(error))) => error,
            Some(Err(_)) => io::Error::other("observability task panicked"),
            _ => io::Error::other("observability task stopped unexpectedly"),
        }
    }

    pub async fn shutdown(mut self) {
        let _ = self.stop.send(true);
        // No unbounded wait on a client that never finishes a scrape.
        if tokio::time::timeout(Duration::from_secs(5), async {
            while self.tasks.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            self.tasks.abort_all();
        }
    }
}

async fn stopped(mut shutdown: watch::Receiver<bool>) {
    if !*shutdown.borrow() {
        let _ = shutdown.changed().await;
    }
}

pub async fn sample(state: &AppState) {
    sample_sources(&state.db, &state.analyses, &state.telemetry).await;
}

async fn sample_sources(db: &Db, analyses: &AnalysisJobs, telemetry: &Telemetry) {
    // Persisted status includes dispatcher-held work and recovery backlog. Use
    // persisted second-resolution age so downtime is retained for recovered work.
    let result: Result<(i64, Option<f64>), sqlx::Error> = sqlx::query_as(
        "SELECT COUNT(*), CAST(unixepoch() - unixepoch(MIN(created_at)) AS REAL) FROM runs WHERE status = 'queued'"
    ).fetch_one(db).await;
    match result {
        Ok((queued, age)) => {
            let oldest_age_seconds = age.unwrap_or(0.0);
            if oldest_age_seconds < 0.0 {
                telemetry.recoverable_error(Component::QueueSampler, ErrorClass::Clock);
            } else {
                telemetry.recovered_error(Component::QueueSampler, ErrorClass::Clock);
            }
            let mut snapshots = analyses.queue_snapshot();
            snapshots.push(QueueSnapshot {
                kind: JobKind::Run,
                queued: queued as u64,
                oldest_age_seconds,
            });
            telemetry.set_snapshot(
                &snapshots,
                db.size(),
                db.num_idle(),
                billing::compute_admitted(),
            );
            telemetry.recovered_error(Component::QueueSampler, ErrorClass::Database);
        }
        Err(_) => {
            // Leave all cached values and freshness timestamp at the last good sample.
            telemetry.recoverable_error(Component::QueueSampler, ErrorClass::Database);
        }
    }
}

pub(crate) async fn purge_sessions(db: &Db, telemetry: &Telemetry) {
    match auth::session::purge_expired(db).await {
        Ok(count) => {
            telemetry.recovered_error(Component::Session, ErrorClass::Database);
            if count > 0 {
                tracing::info!(event = "maintenance.sessions_purged", count);
            }
        }
        Err(_) => telemetry.recoverable_error(Component::Session, ErrorClass::Database),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;

    fn config(metrics_bind: Option<SocketAddr>) -> ServerConfig {
        ServerConfig {
            log_format: Default::default(),
            metrics_bind,
            hosted: false,
            local_mail_sink: None,
            bind: "127.0.0.1:0".into(),
            database_url: "sqlite::memory:".into(),
            db_pool_size: 1,
            sim_workers: 1,
            max_iterations: 100,
            secure_cookies: false,
            cors_origins: vec!["http://localhost:3000".into()],
        }
    }

    #[tokio::test]
    async fn sampler_includes_persisted_backlog_and_keeps_last_good_values() {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE runs(status TEXT, created_at TEXT)")
            .execute(&db)
            .await
            .unwrap();
        let telemetry = Telemetry::new(1);
        let analyses = AnalysisJobs::new_with_telemetry(db.clone(), 1, telemetry.clone());
        for _ in 0..21 {
            sqlx::query("INSERT INTO runs VALUES ('queued',datetime('now','-2 minutes'))")
                .execute(&db)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO runs VALUES ('running',datetime('now','-1 hour'))")
            .execute(&db)
            .await
            .unwrap();
        sample_sources(&db, &analyses, &telemetry).await;
        let good = telemetry.encode().unwrap();
        assert!(good.contains("finplan_jobs_queued{kind=\"run\"} 21"));
        let age: f64 = good
            .lines()
            .find(|line| line.starts_with("finplan_job_oldest_queued_age_seconds{kind=\"run\"}"))
            .unwrap()
            .split_whitespace()
            .last()
            .unwrap()
            .parse()
            .unwrap();
        assert!((120.0..125.0).contains(&age));
        let timestamp = good
            .lines()
            .find(|line| line.starts_with("finplan_queue_snapshot_timestamp_seconds "))
            .unwrap();
        sqlx::query("DROP TABLE runs").execute(&db).await.unwrap();
        sample_sources(&db, &analyses, &telemetry).await;
        sample_sources(&db, &analyses, &telemetry).await;
        let failed = telemetry.encode().unwrap();
        assert!(failed.contains(timestamp));
        assert!(failed.contains("finplan_jobs_queued{kind=\"run\"} 21"));
        assert!(failed.contains(
            "finplan_server_errors_total{component=\"queue_sampler\",class=\"database\"} 2"
        ));
        sqlx::query("CREATE TABLE runs(status TEXT, created_at TEXT)")
            .execute(&db)
            .await
            .unwrap();
        sample_sources(&db, &analyses, &telemetry).await;
        let empty = telemetry.encode().unwrap();
        assert!(empty.contains("finplan_jobs_queued{kind=\"run\"} 0"));
        assert!(empty.contains("finplan_job_oldest_queued_age_seconds{kind=\"run\"} 0.0"));
        sqlx::query("INSERT INTO runs VALUES ('queued',datetime('now','+1 hour'))")
            .execute(&db)
            .await
            .unwrap();
        sample_sources(&db, &analyses, &telemetry).await;
        assert!(
            telemetry
                .encode()
                .unwrap()
                .contains("finplan_job_oldest_queued_age_seconds{kind=\"run\"} 0.0")
        );
        assert!(
            telemetry
                .encode()
                .unwrap()
                .contains("component=\"queue_sampler\",class=\"clock\"} 1")
        );
    }

    #[tokio::test]
    async fn listener_is_optional_and_shutdown_stops_managed_tasks() {
        let (_, state) = crate::build(config(None)).await.unwrap();
        let runtime = ObservabilityRuntime::start(&state).await.unwrap();
        assert_eq!(runtime.metrics_address(), None);
        let shutdown = runtime.stop.subscribe();
        runtime.shutdown().await;
        assert!(*shutdown.borrow());

        let (_, state) = crate::build(config(Some("127.0.0.1:0".parse().unwrap())))
            .await
            .unwrap();
        let runtime = ObservabilityRuntime::start(&state).await.unwrap();
        let address = runtime.metrics_address().unwrap();
        assert!(tokio::net::TcpStream::connect(address).await.is_ok());
        runtime.shutdown().await;
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn configured_bind_failure_is_visible_and_build_never_binds() {
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (_, state) = crate::build(config(Some(occupied.local_addr().unwrap())))
            .await
            .unwrap();
        assert!(ObservabilityRuntime::start(&state).await.is_err());
    }
}
