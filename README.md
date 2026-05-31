# smolgh

A small GitHub CI/CD for smaller projects designed to be as simple to self-host as possible.

`smolgh` uses GitHub webhooks to run scripts in a directory structure on events.

The scripts will be ran in a directory that contains the repository checked out to the correct branch/commit the event is for. Various environment variables will be set and are available for the scripts. 

The directory for event handlers should be placed at the root of the repository and look like this:
```
.smolgh
├── main
│   └── on_push
│       ├── 00_runs_first.sh
│       ├── 10_runs_second.sh
│       ├── build-docs.sh
│       └── run-tests.sh
├── on_issue
│   └── send-notification.py
└── on_pr
    └── verify-valid-pr.rb
```

Any kind of exec'able script/program made in whichever language can be used.

Ensure they have the executable permission set.

Script names prefixed by `<NUM>_` will run synchronously in that order. While scripts without an explicit order will be ran concurrently right away.

## Build and Install

```rs
cargo build --release
sudo install -m 755 target/release/smolgh /usr/bin/smolgh
```

## Configuration

Create a `config.ron` with basic configuration.

```ron
Config(
    // required arguments
    port: 7818,
    secret: "<WEBHOOK-SECRET>",
    runtime_directory: "workdir"

    // optional arguments for fetching remote repositories with `ssh` instead of `https`
    private_ssh_key: "/home/deploy/.ssh/id_ed25519",
    private_ssh_key_pw: None, // TODO: pass this through in a more secure way
)
```

Go to your repository and create a webhook in `Settings -> Webhooks -> Add webhook`

Change the `Payload URL` to the server where your `smolgh` instance is running.

Change the `Content type` to `application/json`

Check `Let me select individual events` and add the events you need.

Currently supported events are:

- Ping
- Push

Further planned events are:

- Issues
- Pull Requests
- Stars

## Script Environment

Push scripts receive these environment variables:

- `SMOLGH_REF`: Full Git ref, for example `refs/heads/main`.
- `SMOLGH_BRANCH`: Branch name extracted from the ref.
- `SMOLGH_COMMIT_BEFORE`: Previous commit SHA from the push payload.
- `SMOLGH_COMMIT_AFTER`: New commit SHA from the push payload.
- `SMOLGH_PUSHER_NAME`: GitHub pusher name.
- `SMOLGH_PUSHER_EMAIL`: GitHub pusher email.
- `SMOLGH_COMMIT_ID`: Latest commit ID from the push payload, when present.
- `SMOLGH_COMMIT_MESSAGE`: Latest commit message, when present.
- `SMOLGH_COMMIT_TIMESTAMP`: Latest commit timestamp, when present.

## TODO

- Create planned events
- Add ssh/webview for observing the output of scripts without checking `smolgh`s stdout/stderr
- Create various templates and example setups for common use-cases
- Figure out how to use a custom `tracing` subscriber without messing up the stdout output of `Rocket`

## License

This project is licensed under the Mozilla Public License 2.0. See
[`LICENSE`](LICENSE).
