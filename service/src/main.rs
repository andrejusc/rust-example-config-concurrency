use std::sync::Arc;
use std::{time, fmt};
use std::{env, str::FromStr};

use config::{Config, File, FileFormat};
use reqwest::Response;
use tokio::task::JoinSet;
use tracing::{debug, info, warn, metadata::LevelFilter};
use tracing_subscriber::{filter, prelude::*};

mod custom_tracing_layer;
use custom_tracing_layer::CustomTracingLayer;

/// Makes merged logging config. WIll panic if ENVROLE env variable is not set.
fn make_log_config() -> Config {
    let env_role = env::var("ENVROLE").unwrap();
    Config::builder()
        .add_source(File::new("env/logging", FileFormat::Yaml))
        .add_source(File::new(("env/logging-".to_string() + &env_role).as_str(), FileFormat::Yaml))
        .build()
        .unwrap()
}

/// Makes merged main config. WIll panic if ENVROLE env variable is not set.
fn make_config() -> Config {
    let env_role = env::var("ENVROLE").unwrap();
    Config::builder()
        .add_source(File::new("env/service", FileFormat::Yaml))
        .add_source(File::new(("env/service-".to_string() + &env_role).as_str(), FileFormat::Yaml))
        .build()
        .unwrap()
}
#[derive(Debug)]
struct CallResult {
    opid: u32,
    resp: Response
}

impl fmt::Display for CallResult {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "opid: {}", self.opid)
    }
}

/// Wrapper for underlying reqwest client call to have operation id assigned.
async fn background_op(id: u32, client: reqwest::Client, query: &str) -> CallResult {
    let call_result = CallResult {
        opid: id,
        resp: client.get(query).send().await.unwrap(),
    };
    // Those delays below are to prove that concurrency works in expected fashion
    if id == 4 {
        let twenty_secs = time::Duration::from_secs(20);
        info!("Request id: {}, Sleep 20 secs", id);
        tokio::time::sleep(twenty_secs).await;
    }
    if id == 6 {
        let fifteen_secs = time::Duration::from_secs(15);
        info!("Request id: {}, Sleep 15 secs", id);
        tokio::time::sleep(fifteen_secs).await;
    }
    call_result
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_config = make_log_config();
    // Enables spans and events with levels `INFO` and below:
    // let level_filter = LevelFilter::INFO;
    let level_str = log_config.get_string("level").unwrap();
    let level_filter:LevelFilter = match FromStr::from_str(&level_str) {
        Ok(filter) => {
            filter
        },
        Err(error) => {
            panic!("Problem parsing log level: {error:?}, supplied level: {level_str}");
        }
    };
    // Set up how `tracing-subscriber` will deal with tracing data.
    tracing_subscriber::registry().with(
        CustomTracingLayer.with_filter(level_filter)
            .with_filter(filter::filter_fn(|metadata| {
                metadata.target().eq("service")
            }))).init();

    info!("Starting logging at level: {}, for env role: {}", &level_str, env::var("ENVROLE").unwrap());
    let config = make_config();
    info!("Main entry point...");
    warn!("warning here");
    debug!("debug here");

    // Mock possible list of queries to perform
    let queries: [&str; 10] = [
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1",
        "https://petstore.swagger.io/v2/pet/1"
    ];

    let mut set = JoinSet::new();

    let concur_max = config.get_int("concurrency").unwrap();
    let data = Arc::new(async_mutex::Mutex::new(0));

    // Name your user agent after your app
    static APP_USER_AGENT: &str = concat!(
        env!("CARGO_PKG_NAME"),
        "/",
        env!("CARGO_PKG_VERSION"),
    );

    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        // .connection_verbose(true)
        .build()?;
    let mut i: u32 = 0;
    let timeout = time::Duration::from_secs(config.get_int("timeout").unwrap().try_into().unwrap());
    for query in queries {
        i += 1;
        info!("Request id: {}, Before calling to: {}", i, query);
        {
            // Lock to be gone when data will go out of scope
            let mut data = data.try_lock().unwrap();
            if *data >= concur_max {
                match tokio::time::timeout(timeout, set.join_next()).await {
                    Err(why) => panic!("! {why:?}"),
                    Ok(opt) => {
                        let call_result: CallResult = opt.unwrap().unwrap();
                        let resp = call_result.resp;
                        info!("Request id: {}, Status: {:#?}", call_result.opid, resp.status());
                        *data -= 1;
                        info!("join1 concur_current: {}", *data);
                    },
                }
            }
        }
        let fut = background_op(i, client.clone(), query);
        set.spawn(fut);
        {
            let mut data = data.try_lock().unwrap();
            *data += 1;
            info!("concur_current: {}", *data);
        }
    }

    // Allow every spawned task to complete without timeout constraint
    while let Some(res) = set.join_next().await {
        let call_result = res.unwrap();
        let resp = call_result.resp;
        info!("Request id: {}, Status: {:#?}", call_result.opid, resp.status());
        {
            let mut data = data.try_lock().unwrap();
            *data -= 1;
            info!("join2 concur_current: {}", *data);
        }
    }

    Ok(())
}
