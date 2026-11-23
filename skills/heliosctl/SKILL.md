---
name: heliosctl
description: Operate Helios streaming devices (Orange Pi and Raspberry Pi transcoders and relays, PROXmobil3 scanners) through the Helios Control API with the heliosctl command-line tool. Use when asked to check a Helios device, list or inspect its streams, start or stop a stream, change a stream's source, profile, bitrate or destination, look at a device's cameras and inputs or encoding profiles, or add, list or remove its Wi-Fi networks.
---

# Operating Helios devices with heliosctl

`heliosctl` talks to the Helios Control API, which changes and reads devices through AWS IoT.
Changes are real: they affect live video streams and a device's network. Work carefully, in the
order below, and ask the user before anything disruptive.

## Before you start

1. Check the tool and its access: `heliosctl --version`, then `heliosctl --json devices list`.
   - Exit 2 with "no Control API endpoint": the user must set `HELIOS_CONTROL_URL` or
     `~/.config/heliosctl/config.toml`. Don't guess the URL.
   - Exit 6: no AWS credentials, or not allowed. The user signs in (for example `aws login`);
     never ask them to paste keys into the conversation.
2. Always pass `--json`. Parse stdout; on failure read the JSON error on stderr. Don't parse the
   human tables.
3. Name a device by its thing name (`t-…`) or device ID (`d-…`) from `devices list`. If the user
   says "the Orange Pi" or "the backpack", find it in `devices list` (role `transcoder` or
   `relay`, Helios version) and confirm with them if more than one could match.

## The order for every change

1. **Read first.** `heliosctl --json streams get <device> <stream>` (or `streams list`,
   `inputs list`, `profiles list`, `wifi list`). Know the current state before changing it, and
   check that what you're about to set exists: a source in `inputs list`, a profile in
   `profiles list`.
2. **Dry run.** Add `--dry-run` to the change. It checks the input with the service's own rules
   and prints the exact request without sending it. Fix anything it refuses (exit 2) before going
   on.
3. **Ask when it's disruptive** (see below). Show the user what will change.
4. **Make the change.** For a stream change, add a client token and `--wait`, for example
   `heliosctl --json streams update <device> <stream> --bitrate 3000 --client-token <token> --wait`.
   Make up a token per change (letters, digits, `-`, `_`), and reuse the same token if you retry
   that change: the API then returns the first result instead of applying it twice. Wi-Fi changes
   (`wifi set`, `wifi remove`) take neither: the device applies them before the command returns.
5. **Check the result** from the exit code and the returned stream (`desired`, `reported`,
   `pending`, `rejected`).

## Ask the user first

- Stopping a stream (`streams stop`) or starting one: it goes on or off air.
- Changing a live stream's source, profile or destination: viewers see a gap, and a wrong
  destination sends the video elsewhere.
- Removing a Wi-Fi network (`wifi remove`): if the device is on that network, it can drop off the
  internet and stop answering.
- Anything the user didn't ask for. Reading is always fine.

## Secrets

- A Wi-Fi passphrase is read only from a file or standard input:
  `heliosctl --json wifi set <device> --ssid <ssid> --passphrase-file -` with the passphrase on
  stdin. There is no `--passphrase` flag; never put one in a command line, a file name or the
  conversation. If the user hasn't provided the passphrase in a file, ask them to run the command
  themselves.
- An SRT destination URL with `passphrase=` must come from `--url-file` (or `-`), never `--url`.
  heliosctl shows passphrases as `***`; don't try to recover them.
- Never print, log or repeat a passphrase, PIN or AWS key.
- `location` returns the device's exact position. Answer the user directly when they ask where
  it is, but don't copy coordinates into notes, issues, commits, logs or anything shared.

## Exit codes

| Code | Meaning | What to do |
| --- | --- | --- |
| 0 | Done (with `--wait`: applied) | Report the result |
| 1 | Network or internal error | Retry once with the same client token, then report |
| 2 | Invalid input | Fix the input (the error names the part); don't retry as is |
| 3 | Device or stream not found | Recheck names with `devices list` / `streams list` |
| 4 | Refused by the service or the device | Read `error.type`, `reason` or `rejected`; report it |
| 5 | Timed out (`DeviceUnreachable`, or `--wait` ran out) | The device may still apply it: read it again before retrying |
| 6 | Not authorized | The user's credentials; don't work around it |
| 7 | The device hasn't reported that yet | Wait, or check it's online, then read again |
| 8 | Throttled, after retries | Wait a minute, then retry with the same token |

More in [references/errors.md](references/errors.md).

## Commands

The full list, with every flag, is in [references/commands.md](references/commands.md). The
input and output shapes of each operation: `heliosctl schema <Operation>` (for example
`heliosctl schema UpdateStream`).

Common tasks:

- What's on the device: `heliosctl --json devices get <device>`, then
  `heliosctl --json streams list <device>`.
- Lower a stream's bitrate: read it, then
  `heliosctl --json streams update <device> <stream> --bitrate 3000 --dry-run`, then the same with
  `--client-token <token> --wait` instead of `--dry-run`. Back to the profile's own bitrate:
  `--bitrate default`.
- Switch a stream to a camera: `heliosctl --json inputs list <device>` for the camera's
  selector, then `streams update … --source camera:<selector>` (dry run first).
- Send a stream somewhere new: `streams set-destination <device> <stream> --url srt://host:port?streamid=...`
  (dry run first; `--url-file -` if it has a passphrase).
- Where is it: `heliosctl --json location <device>`. With no current fix, say so and give
  `lastFix` with its time (how old it is), not as if it were current.
- Add a Wi-Fi network: ask the user to run
  `heliosctl --json wifi set <device> --ssid <ssid> --passphrase-file -` themselves with the
  passphrase on stdin, unless they've given you a file holding it.

## Reporting back

Say what you changed, on which device and stream, and what the device reports now
(`reported.state`, bitrate, destination with passphrases hidden). If something was refused or
timed out, say so plainly, with the error type and reason.
