use auth::Auth;
use event::{EventChannels, ping, repo};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::{Request, State, launch, post, routes};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

mod auth;
mod event;

#[derive(Deserialize)]
struct Config {
    port: i32,
    secret: String,
    runtime_directory: PathBuf,

    private_ssh_key: Option<PathBuf>,
    private_ssh_key_pw: Option<String>,
}

// Create an endpoint receiver which reads a string from a header.
macro_rules! string_header {
    ($ty:ident, $str:literal) => {
        #[derive(Debug)]
        struct $ty<'a>(&'a str);

        #[rocket::async_trait]
        impl<'r> FromRequest<'r> for $ty<'r> {
            type Error = String;

            async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
                request
                    .headers()
                    .get_one($str)
                    .map($ty)
                    .map(Outcome::Success)
                    .unwrap_or_else(|| {
                        Outcome::Error((Status::BadRequest, format!("missing {} header", $str)))
                    })
            }
        }
    };
}

string_header!(GithubEvent, "X-Github-Event");
string_header!(Secret, "X-Hub-Signature-256");

#[launch]
async fn rocket() -> _ {
    let config: Config = {
        let file = std::fs::File::open("config.ron").unwrap();
        ron::de::from_reader(file).unwrap()
    };

    let mut private_ssh_key = None;
    if let Some(path) = config.private_ssh_key {
        match std::fs::read_to_string(path) {
            Ok(key) => private_ssh_key = Some(key),
            Err(error) => panic!("Could not read private ssh key: {error}"),
        }
    }

    let auth = Arc::new(Auth::new(config.secret));
    let event_channels = Arc::new(EventChannels::init_event_handlers(
        config.runtime_directory,
        private_ssh_key,
        config.private_ssh_key_pw,
    ));

    rocket::build()
        .configure(
            rocket::Config::figment()
                .merge(("port", config.port))
                .merge(("limits.string", "10MiB"))
                .merge(("limits.bytes", "10MiB")),
        )
        .manage(auth)
        .manage(event_channels)
        .mount("/", routes![handle])
}

#[derive(Debug, Deserialize, Serialize)]
struct Repository<'a> {
    name: &'a str,
    full_name: &'a str,

    url: &'a str,
    ssh_url: &'a str,
    clone_url: &'a str,
}

#[derive(Serialize, Deserialize)]
struct Body<'a, T> {
    repository: Repository<'a>,

    #[serde(default)]
    #[serde(borrow)]
    _hack: PhantomData<&'a ()>,

    #[serde(flatten)]
    kind: T,
}

#[post("/cicd", data = "<json>")]
async fn handle(
    auth: &State<Arc<Auth>>,
    channels: &State<Arc<EventChannels>>,
    event: GithubEvent<'_>,
    secret: Secret<'_>,
    json: &str,
) -> Status {
    let auth = Arc::clone(auth);

    println!("sending {event:?} to handlers");

    match event {
        GithubEvent("ping") => match auth.verified_decode::<ping::Body>(json, secret.0) {
            Ok(json) => channels.send_ping(json.kind),
            Err(status) => status,
        },
        GithubEvent("push") => match auth.verified_decode::<repo::Push>(json, secret.0) {
            Ok(json) => channels.send_push(json.repository, json.kind),
            Err(status) => status,
        },
        _ => {
            println!("Unhandled: {event:?}");
            Status::Ok
        }
    }
}
