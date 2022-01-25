
#[macro_use]
extern crate diesel;

mod server;
mod cron;
mod bundle;
mod database;
mod types;
mod consts;

use std::collections::HashSet;
use std::iter::FromIterator;

use server::run_server;
use cron::run_crons;

#[actix_web::main]
async fn main() -> () {
    dotenv::dotenv().unwrap();

    let mut set = HashSet::new();
    for arg in std::env::args() {
        set.insert(arg);
    }

    if !set.contains("--no-cron") {
        tracing::info!("Running with cron");
        tokio::task::spawn_local(run_crons());
    } else {

    };

    if !set.contains("--no-server") {
        tracing::info!("Running with server");
        run_server().await.unwrap()
    } else {

    };
}