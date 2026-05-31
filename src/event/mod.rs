use super::Repository;
use rocket::http::Status;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock, mpsc};

pub mod ping;
pub mod repo;

pub struct EventChannels {
    runtime_directory: PathBuf,
    private_ssh_key: Option<String>,
    private_ssh_key_pw: Option<String>,

    ping: mpsc::Sender<ping::Body>,
    repositories: RwLock<HashMap<String, Arc<mpsc::Sender<repo::Event>>>>,
    // push: mpsc::Sender<push::Body>,
}

impl EventChannels {
    pub fn init_event_handlers(
        runtime_directory: PathBuf,
        private_ssh_key: Option<String>,
        private_ssh_key_pw: Option<String>,
    ) -> EventChannels {
        let ping = {
            let (sender, recv) = mpsc::channel();
            std::thread::spawn(move || ping::task(recv));
            sender
        };

        let repositories = RwLock::new(HashMap::new());

        EventChannels {
            private_ssh_key,
            private_ssh_key_pw,
            ping,
            repositories,
            runtime_directory,
        }
    }

    pub fn send_ping(&self, body: ping::Body) -> Status {
        match self.ping.send(body) {
            Ok(()) => Status::Ok,
            Err(_) => Status::InternalServerError,
        }
    }

    fn with_repo_sender<F>(&self, repo: Repository<'_>, f: F) -> Status
    where
        F: FnOnce(Arc<mpsc::Sender<repo::Event>>) -> Status,
    {
        // Task handling events for this repository is already running
        let slot = {
            let guard = self.repositories.read().unwrap();

            guard.get(repo.full_name).cloned()
        };
        if let Some(channel) = slot {
            return f(channel);
        }

        // Initialise the task handler for this repository

        let path = self
            .runtime_directory
            .join(sanitize_name(repo.full_name).as_ref());

        // Create the organization/user folder but not the repository folder
        let organization_folder = path.parent().unwrap();

        if let Err(err) = std::fs::create_dir_all(organization_folder) {
            eprintln!(
                "Could not create repository directory {}: {err}",
                repo.full_name
            );
            return Status::InternalServerError;
        }

        let (sender, recv) = mpsc::channel();

        self.repositories
            .write()
            .unwrap()
            .insert(repo.full_name.into(), Arc::new(sender));

        let ssh_url = repo.ssh_url.to_string();
        let https_url = repo.clone_url.to_string();

        let key = self
            .private_ssh_key
            .clone()
            .map(|key| (key, self.private_ssh_key_pw.clone()));

        let repository = match repo::open_or_clone(&path, key, ssh_url, https_url) {
            Ok(repo) => repo::OpenRepository::new(repo, path),
            Err(error) => {
                eprintln!("failed to open or create git repository: {error}");
                return Status::InternalServerError;
            }
        };

        std::thread::spawn(move || repo::task(repository, recv));

        // It should now exist.
        let channel = {
            let guard = self.repositories.read().unwrap();
            guard[repo.full_name].clone()
        };

        f(channel)
    }

    pub fn send_push(&self, repo: Repository<'_>, body: repo::Push) -> Status {
        self.with_repo_sender(repo, |sender| match sender.send(repo::Event::Push(body)) {
            Ok(()) => Status::Ok,
            Err(_) => {
                eprintln!("repository channel error");
                Status::InternalServerError
            }
        })
    }
}

fn sanitize_name(name: &str) -> Cow<'_, str> {
    let mut name = Cow::Borrowed(name);

    while name.starts_with('/') {
        name = Cow::Owned(format!("\\{}", &name[1..]));
    }

    if name.contains('.') {
        name = Cow::Owned(name.replace('.', "\\"));
    }

    name
}
