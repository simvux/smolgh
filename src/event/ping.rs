use serde::{Deserialize, Serialize};
use std::sync::mpsc;

#[derive(Debug, Deserialize, Serialize)]
pub struct Body {
    zen: String,
}

pub fn task(resc: mpsc::Receiver<Body>) {
    loop {
        let Ok(ping) = resc.recv() else {
            return;
        };

        println!("pong! {}", ping.zen);
    }
}
