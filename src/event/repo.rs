use git2::{self as git, RemoteCallbacks};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;

pub enum Event {
    Push(Push),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Push {
    r#ref: String,
    before: String,
    after: String,
    pusher: Pusher,
    commits: Vec<Commit>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Pusher {
    name: String,
    email: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Commit {
    id: String,
    message: String,
    timestamp: String,
    url: String,
}

pub fn open_or_clone(
    // TODO: Cleanup how we handle credentials
    dir: &Path,
    privkey: Option<(String, Option<String>)>,
    ssh_url: String,
    https_url: String,
) -> Result<git::Repository, String> {
    if dir.exists() {
        match git::Repository::open(&dir) {
            Ok(repository) => Ok(repository),
            Err(error) => Err(format!(
                "{} is not a valid, delete it if you want proceed\n  {error}",
                dir.display()
            )),
        }
    } else {
        let mut used_url = &https_url;

        let creation = match privkey {
            Some((key, pw)) => {
                used_url = &ssh_url;

                let mut callbacks = RemoteCallbacks::new();
                callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                    git::Cred::ssh_key_from_memory(
                        username_from_url.unwrap_or(""),
                        None,
                        &key,
                        pw.as_deref(),
                    )
                });

                // git::Repository::clone(ssh, git_directory);
                let mut fo = git::FetchOptions::new();
                fo.remote_callbacks(callbacks);

                let mut builder = git::build::RepoBuilder::new();
                builder.fetch_options(fo);

                builder.clone(&ssh_url, &dir)
            }
            None => git::Repository::clone(&https_url, &dir),
        };

        match creation {
            Ok(repository) => Ok(repository),
            Err(error) => Err(format!("Could not clone {used_url} {error}")),
        }
    }
}

pub fn task(mut repository: OpenRepository, resc: mpsc::Receiver<Event>) {
    println!(
        "Initialised task handler for repository {}",
        repository.path.display()
    );

    loop {
        let Ok(event) = resc.recv() else {
            return;
        };

        match event {
            Event::Push(p) => {
                println!(
                    "handling on_push {} in {}",
                    p.r#ref,
                    repository.path.display()
                );
                repository.on_push(p)
            }
        }
    }
}

pub struct OpenRepository {
    repo: git::Repository,
    path: PathBuf,
}

fn leading_number(name: &OsStr) -> Option<i64> {
    let name = name.to_str()?;

    name.as_bytes()
        .iter()
        .position(|&c| c == b'_')
        .and_then(|separator| name[..separator].parse().ok())
}

impl OpenRepository {
    pub fn new(repo: git::Repository, path: PathBuf) -> Self {
        Self { repo, path }
    }

    fn get_scripts_in(&self, path: &Path) -> Result<Vec<(Option<i64>, PathBuf)>, io::Error> {
        println!("running scripts in {}", path.display());

        let dir = std::fs::read_dir(path)?;

        let mut paths = dir
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let type_ = entry.file_type().ok()?;

                if !type_.is_file() {
                    return None;
                }

                let path = entry.path();
                let leading_number = path.file_name().and_then(leading_number);

                let path = path.canonicalize().ok()?;

                Some((leading_number, path))
            })
            .collect::<Vec<_>>();

        paths.sort_by(|(leading_lhs, _), (leading_rhs, _)| {
            leading_lhs.unwrap_or(-1).cmp(&leading_rhs.unwrap_or(-1))
        });

        Ok(paths)
    }

    pub fn on_push(&mut self, push: Push) {
        let branch_name = push.r#ref.split('/').last().unwrap();

        let mut remote = self.repo.find_remote("origin").unwrap();

        remote
            .fetch(&[branch_name], None, None)
            .expect("could not fetch remote branch");

        let remote_ref = format!("refs/remotes/origin/{branch_name}");
        let oid = self
            .repo
            .refname_to_id(&remote_ref)
            .expect("could not find fetched remote branch");

        // TODO: If this is an old event that was queued due to the worker being busy, then fetching
        // the last commit of the branch can be incorrect. We need to checkout the specific commit
        // in a detached state instead.

        let object = self
            .repo
            .find_object(oid, None)
            .expect("could not find fetched commit");

        self.repo
            .checkout_tree(&object, Some(git::build::CheckoutBuilder::new().safe()))
            .expect("could not checkout branch");

        self.repo
            .reference(&push.r#ref, oid, true, "update local branch")
            .expect("could not update local branch");

        self.repo
            .set_head(&push.r#ref)
            .expect("could not set branch HEAD");

        let on_push_path = {
            let mut path = self.path.clone();
            path.push(".smolgh");
            path.push(branch_name);
            path.push("on_push");
            path
        };

        match self.get_scripts_in(&on_push_path) {
            Err(err) => {
                eprintln!("Could not open {}: {err}", on_push_path.display());
            }
            Ok(scripts) => {
                let mut concurrent_handles = vec![];

                for (leading_number, script) in scripts {
                    let mut command = Command::new(&script);

                    println!(
                        " >> Running command {:?}",
                        script.file_name().unwrap_or(OsStr::new("???"))
                    );

                    command
                        .current_dir(&self.path)
                        .env("SMOLGH_REF", &push.r#ref)
                        .env("SMOLGH_BRANCH", branch_name)
                        .env("SMOLGH_COMMIT_BEFORE", &push.before)
                        .env("SMOLGH_COMMIT_AFTER", &push.after)
                        .env("SMOLGH_PUSHER_NAME", &push.pusher.name)
                        .env("SMOLGH_PUSHER_EMAIL", &push.pusher.email);

                    // For now at least, only attach the latest commit
                    if let Some(commit) = push.commits.last() {
                        command
                            .env("SMOLGH_COMMIT_ID", &commit.id)
                            .env("SMOLGH_COMMIT_MESSAGE", &commit.message)
                            .env("SMOLGH_COMMIT_TIMESTAMP", &commit.timestamp);
                    }

                    // TODO: Stream the stdout/stderr to some website
                    command.stdout(std::io::stdout());

                    match command.spawn() {
                        Err(error) => {
                            eprintln!("could not run {}: {error}", script.display());
                        }
                        Ok(child) if leading_number.is_none() => concurrent_handles.push(child),
                        Ok(child) => wait_child(child),
                    }
                }

                // Wait for all scripts that didn't need a specific order and thus were spawned concurrently.
                concurrent_handles.into_iter().for_each(wait_child);
            }
        }
    }
}

fn wait_child(mut child: std::process::Child) {
    match child.wait() {
        Err(error) => eprintln!("child process failed: {error}"),
        Ok(status) => {
            if status.code().is_some_and(|code| code != 0) {
                eprintln!("child process exited with non-zero code: {status}")
            }
        }
    }
}
