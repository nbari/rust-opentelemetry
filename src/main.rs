use bigdecimal::BigDecimal;
use chrono::prelude::*;
use clap::{Arg, Command};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::time::Duration;
use warp::{http::HeaderMap, http::Response, Filter};

use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres, Row};

use opentelemetry::global::shutdown_tracer_provider;
use opentelemetry::sdk::Resource;
use opentelemetry::trace::Span;
use opentelemetry::trace::TraceError;
use opentelemetry::trace::Tracer;
use opentelemetry::{global, sdk::trace as sdktrace};
use opentelemetry_otlp::WithExportConfig;

const HTML: &str = r###"
<!DOCTYPE html>
<html>
<head>
<style>
    body {
        background-color: lightgreen;
    }
</style>
    <body>
    Hi: {}
    </body>
</html>"###;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub const GIT_COMMIT_HASH: &str = if let Some(hash) = built_info::GIT_COMMIT_HASH {
    hash
} else {
    ":-("
};

fn init_tracer() -> Result<sdktrace::Tracer, TraceError> {
    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_env())
        .with_trace_config(sdktrace::config().with_resource(Resource::default()))
        .install_batch(opentelemetry::runtime::Tokio)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let _ = init_tracer().unwrap();
    let matches = Command::new("demo")
        .version(format!("{} {}", env!("CARGO_PKG_VERSION"), GIT_COMMIT_HASH))
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .default_value("80")
                .help("listening port")
                .value_parser(clap::value_parser!(u16)),
        )
        .get_matches();

    let port: u16 = *matches.get_one("port").unwrap();

    let now = Utc::now();
    println!(
        "{} - Listening on *:{}",
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
        port
    );

    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::new(5, 0))
        .idle_timeout(Duration::new(60, 0))
        .max_connections(5)
        .connect(
            format!(
                "postgres://{}:{}@{}/demo",
                env!("DB_USER"),
                env!("DB_PASS"),
                env!("DB_HOST")
            )
            .as_ref(),
        )
        .await?;

    let db = warp::any().map(move || pool.clone());

    // define the routes to use
    let hello = warp::get().and(log_headers()).and_then(hello);
    let query = warp::get()
        .and(warp::path("query"))
        .and(db.clone())
        .and_then(query);
    let health = warp::any().and(warp::path("health")).and_then(health);

    // GET /*
    // ANY /health
    let routes = health.or(query).or(hello);

    // listen in both tcp46 falling back to IPv4
    let addr = match IpAddr::from_str("::0") {
        Ok(a) => a,
        Err(_) => IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
    };

    // start service
    warp::serve(routes).run((addr, port)).await;

    shutdown_tracer_provider();

    Ok(())
}

fn log_headers() -> impl Filter<Extract = (), Error = Infallible> + Copy {
    warp::header::headers_cloned()
        .map(|headers: HeaderMap| {
            let tracer = global::tracer("global_tracer");
            let mut header_hashmap: HashMap<String, String> = HashMap::new();
            for (k, v) in headers.iter() {
                let k = k.as_str().to_owned();
                let v = String::from_utf8_lossy(v.as_bytes()).into_owned();
                header_hashmap.entry(k).or_insert(v);
            }
            let parent_cx =
                global::get_text_map_propagator(|propagator| propagator.extract(&header_hashmap));
            let mut child = tracer
                .span_builder("log headers")
                .start_with_context(&tracer, &parent_cx);

            let j = serde_json::to_string(&header_hashmap).unwrap();
            println!("{}", j);
            child.end();
        })
        .untuple_one()
}

// GET  /*
async fn hello() -> Result<impl warp::Reply, warp::Rejection> {
    let resp = reqwest::get("https://httpbin.org/ip")
        .await
        .unwrap()
        .json::<HashMap<String, String>>()
        .await
        .unwrap();
    let rs = HTML.replace("{}", resp.get("origin").unwrap());
    Ok(warp::reply::html(rs))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Bookings {
    pub id: String,
    pub date: chrono::DateTime<chrono::Utc>,
    pub total: BigDecimal,
}

// GET  /query
async fn query(db: Pool<Postgres>) -> Result<impl warp::Reply, warp::Rejection> {
    let rows = sqlx::query(
        r#"
SELECT   *
FROM     bookings
ORDER BY total_amount desc
LIMIT    10
        "#,
    )
    .fetch_all(&db)
    .await
    .expect("some error");

    let x: Vec<Bookings> = rows
        .iter()
        .map(|r| Bookings {
            id: r.get::<String, _>("book_ref"),
            date: r.get::<chrono::DateTime<chrono::Utc>, _>("book_date"),
            total: r.get::<BigDecimal, _>("total_amount"),
        })
        .collect();

    Ok(warp::reply::json(&x))
}

// ANY /health
// return X-APP header and the commit in the body
async fn health() -> Result<impl warp::Reply, warp::Rejection> {
    let short_hash = if GIT_COMMIT_HASH.len() > 7 {
        &GIT_COMMIT_HASH[0..7]
    } else {
        ""
    };
    Ok(Response::builder()
        .header(
            "X-App",
            format!(
                "{}:{}:{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                short_hash
            ),
        )
        .body(GIT_COMMIT_HASH))
}
