# heliosctl

The Helios Control API's command-line client, for operators and AI agents (HLD 7.3.4 and
7.3.4.1 in the Notion page "Helios - High-Level Design"). The Control API lives in
[helios-cloud](https://github.com/kench/helios-cloud) under `/v1/control`; its model is
[helios-contract](https://github.com/kench/helios-contract), a git submodule here.

```sh
heliosctl devices list
heliosctl --json devices get t-01M80KAX049HNYQ4K9NYP8SNFH
heliosctl schema SetWifiNetwork

heliosctl streams list t-01M80KAX049HNYQ4K9NYP8SNFH
heliosctl streams start t-01M80KAX049HNYQ4K9NYP8SNFH pocket --wait
heliosctl streams update t-01M80KAX… pocket --bitrate 3000 --dry-run
heliosctl streams update t-01M80KAX… pocket --source rtmp:ingest --encoding-profile hevc
heliosctl streams set-destination t-01M80KAX… pocket --url 'srtla://10.0.0.1:5000'
heliosctl streams set-destination t-01M80KAX… pocket --url-file - < destination.txt

heliosctl inputs list t-01M80KAX…
heliosctl profiles list t-01M80KAX…
heliosctl wifi list t-01M80KAX…
heliosctl wifi set t-01M80KAX… --ssid 'Venue Media' --passphrase-file - < passphrase.txt
heliosctl wifi set t-01M80KAX… --ssid 'Guest' --open
heliosctl wifi remove t-01M80KAX… --ssid 'Guest'
```

Sources: `camera:<video>[,audio=<device>]` (`usb:1-1.3` or `<vid>:<pid>:<serial>`),
`rtmp:<key>` (`ingest`, `ingest-<n>`), `test[:<WxH>][@<fps>]`. Bitrates: kbit/s, or `default`.
`--profile` is always the AWS profile; a stream's profile is `--encoding-profile`.

## For agents

- `--json` on every command prints exactly one JSON document on stdout: the API's response,
  with the model's member names. Errors then go to stderr as
  `{"error": {"type", "message", ...the error's members}}`.
- Never interactive: no prompts, no pager, no browser.
- Changes (`streams start|stop|update|set-destination`) carry a client token (`--client-token`,
  default random), so they're retried safely. `--dry-run` checks the input with the service's
  rules and prints `{"dryRun": true, "request": {...}}` without sending; for a device ID it needs
  no credentials. `--wait [--timeout 60]` polls until the device applied the change (0), refused
  it (4) or the time ran out (5).
- A Wi-Fi passphrase only comes from `--passphrase-file` (or `-` for stdin), never an argument,
  and is never printed (`--dry-run` shows `***`). `wifi set` needs `--passphrase-file` or `--open`.
- SRT passphrases are never printed (`passphrase=***`), and a destination with one is read from
  `--url-file` (or `-` for stdin), never the command line.
- `heliosctl schema [operation]` prints the API's OpenAPI description (the contract's release
  file, `schema/HeliosControl.openapi.json`, checked in tests against the pinned model).
- Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Done |
| 1 | Anything else: a network error, an internal error, a bad response |
| 2 | Invalid input: usage, `ValidationException`, `InvalidDestination` |
| 3 | Not found: `DeviceNotFound`, `StreamNotFound` |
| 4 | Refused: `SettingUnavailable`, `WifiNetworkRefused`, the device's `rejected` |
| 5 | The device didn't answer or apply in time: `DeviceUnreachable`, `--wait` timing out |
| 6 | Not authorized: no credentials, HTTP 403 |
| 7 | The device hasn't reported that: `DeviceStateUnavailable` |
| 8 | Throttled: `RateLimited`, after retries |

## Setup

**Endpoint** (not a secret): `--endpoint`, `HELIOS_CONTROL_URL`, or
`~/.config/heliosctl/config.toml`:

```toml
endpoint = "https://<api-id>.execute-api.us-west-2.amazonaws.com/dev"
profile = "helios-operator"   # the default
region = "us-west-2"          # else the profile's
```

**Credentials**: the AWS credential chain (`--profile`, `AWS_PROFILE`, default
`helios-operator`). heliosctl stores no secret. The profile should assume the role, so calls use
short-lived sessions:

```ini
# ~/.aws/config
[profile helios-operator]
role_arn = arn:aws:iam::<account>:role/helios-operator
source_profile = default
region = us-west-2
```

With `aws login` (a `login_session` profile), which the Rust AWS SDK can't read yet, bridge it
through the AWS CLI:

```ini
[profile helios-login]
credential_process = aws configure export-credentials --profile default --format process

[profile helios-operator]
role_arn = arn:aws:iam::<account>:role/helios-operator
source_profile = helios-login
region = us-west-2
```

## Development

```sh
git submodule update --init
cargo +nightly fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo llvm-cov --locked --workspace --fail-under-lines 90
```

The toolchain is pinned in `rust-toolchain.toml`; rustfmt runs on nightly. Conventional Commits
for commits and PR titles, squash merges. License: CC0-1.0.
