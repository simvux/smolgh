use super::Body;
use hmac::{HmacReset, KeyInit, Mac};
use rocket::http::Status;
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Arc;
use std::sync::Mutex;

pub struct Auth {
    mac: Mutex<HmacSha256>,
}

type HmacSha256 = HmacReset<Sha256>;

impl Auth {
    pub fn new(secret: String) -> Auth {
        let mac = HmacSha256::new_from_slice(secret.as_bytes())
            .expect("failed to initialize HMAC verifier");

        Auth {
            mac: Mutex::new(mac),
        }
    }

    pub fn verified_decode<'a, Kind>(
        self: Arc<Self>,
        data: &'a str,
        header: &str,
    ) -> Result<Body<'a, Kind>, Status>
    where
        Kind: Deserialize<'a>,
    {
        let Ok(mut mac) = self.mac.lock() else {
            eprintln!("hmac lock has been poisoned");
            return Err(Status::InternalServerError);
        };

        let Some(sig_sep) = header.strip_prefix("sha256=") else {
            eprintln!("webhook signature header has invalid format");
            return Err(Status::BadRequest);
        };

        let Ok(decoded) = hex::decode(sig_sep) else {
            eprintln!("webhook signature is not valid hex");
            return Err(Status::Unauthorized);
        };

        mac.update(data.as_bytes());

        let Ok(()) = mac.verify_slice_reset(&decoded) else {
            eprintln!("webhook signature verification failed");
            mac.reset();
            return Err(Status::Unauthorized);
        };

        println!("webhook signature verified");

        match serde_json::de::from_str(data) {
            Ok(json) => Ok(json),
            Err(error) => {
                eprintln!("webhook payload is not valid JSON: {error}");
                Err(Status::BadRequest)
            }
        }
    }
}
