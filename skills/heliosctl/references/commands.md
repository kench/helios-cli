# heliosctl commands

Every command takes `--json` (always use it), and the settings `--endpoint` (or
`HELIOS_CONTROL_URL`), `--profile` (the AWS profile, default `helios-operator`, or
`AWS_PROFILE`) and `--region`. `<device>` is a thing name (`t-…`) or a device ID (`d-…`).
`heliosctl <command> --help` and `man heliosctl-<command>` say the same as this page.

## Devices

- `heliosctl --json devices list`: every device: `deviceId`, `thingName`, `role`, `status`,
  `heliosVersion`.
- `heliosctl --json devices get <device>`: one device, and its modules' states
  (`modules: {"net": "running", ...}`).

## Streams

Reads:

- `heliosctl --json streams list <device>`: every stream, each with `desired`, `reported`,
  `pending` and `rejected`; and `recording`, the device's recordings disk (`dir`, `mounted`,
  `full`, `totalBytes`, `freeBytes`, `segments`, `oldest`), if it reports one. Recordings are
  MPEG-TS segments on the device at `<dir>/<stream>/<run start>-<n>.ts`, fetched over SSH;
  the oldest are deleted when the disk fills.
- `heliosctl --json streams get <device> <stream>`: one stream.

Changes (each takes `--dry-run`, `--wait`, `--timeout <seconds>` (default 60) and
`--client-token <token>`):

- `heliosctl --json streams start <device> <stream>`: desired state live.
- `heliosctl --json streams stop <device> <stream>`: desired state stopped.
- `heliosctl --json streams update <device> <stream> [--source <source>]
  [--encoding-profile <name>] [--bitrate <kbps>|default] [--source-ceiling-percent <0-400>]
  [--record on|off]`:
  - `--source`: `camera:<video>[,audio=<device>]` (the video is `usb:<port>`, such as
    `usb:1-1.3`, or `<vid>:<pid>:<serial>`, as `inputs list` shows), `rtmp:<key>` (`ingest`,
    `ingest-2`, ...), or `test[:<width>x<height>][@<fps>]`.
  - `--encoding-profile`: a profile from `profiles list`. (`--profile` is the AWS profile.)
  - `--bitrate`: kbit/s, 100 to 100000, within the profile's range; `default` for the
    profile's own.
  - `--source-ceiling-percent`: adaptive RTMP streams: the bitrate ceiling as a percentage of
    the camera's.
  - `--record`: record the stream on the device as sent (`on`) or not (`off`). Switching
    restarts the stream's encoder, a few seconds' gap in the stream; ask before switching a
    live stream. The stream's `reported.recording.state` then says `recording`, `idle` (set to
    record, not running), `unavailable` (no recordings disk mounted), `disk_full` or `error`;
    `off` when it isn't set to record.
- `heliosctl --json streams set-destination <device> <stream> --url <url>` or
  `--url-file <file|->`: an `srt://` or `srtla://` URL, `host:port`, with only `streamid`,
  `latency` (20 to 30000 ms) and `passphrase` (10 to 79 characters) parameters. A URL with a
  passphrase must use `--url-file`.

A stream change returns `{"stream": {...}, "shadowVersion": n}` at once; with `--wait` it returns
once the device has applied it (`pending: false`), or fails with exit 4 (a new entry in
`rejected`) or 5 (time ran out).

## Inputs and profiles

- `heliosctl --json inputs list <device>`: cameras (`selector`, `port`, `product`,
  `hasAudio`), audio devices, RTMP ingest keys (and whether something is publishing), test
  patterns.
- `heliosctl --json profiles list <device>`: encoding profiles: `name`, `video` (encoder),
  `encoder`, `bitrateMode`, `bitrateKbps`, `minBitrateKbps`, `maxBitrateKbps`, `audio`.

## Location

- `heliosctl --json location <device>`: the device's GPS state (GetDeviceLocation): `gpsd`
  (`connected` or `unavailable`), `fix` (`mode`: `none`, `2d` or `3d`; with a fix, `time`,
  `latitude`, `longitude`, `altitudeM`, `speedMps`, `trackDeg`, `climbMps`,
  `horizontalErrorM`, `verticalErrorM`), `lastFix` (the last fix, while `fix` is `none`; its
  `time` says how old it is), `satellites` (`used`, `visible`, `hdop`, `pdop`), `receivers`,
  `since` (when the fix mode last changed), `configuredReceiver` and `reportedAt`. Exit 7
  (`DeviceStateUnavailable`) when the device has never reported GPS. **This is the device's
  exact position.**

## Uplinks

- `heliosctl --json uplinks <device> [--show-public-addresses]`: the device's network
  interfaces (GetDeviceUplinks), as `uplinks`, each with `interface`, `kind` (`ethernet`,
  `wifi`, `cellular`, `virtual`, `other`), `operState`, `carrier`, `ipv4`,
  `defaultRouteMetric` (lower is preferred), `sourceRouted`, `wifi` (`ssid`, `signalDbm`,
  `frequencyMhz`), `publicIpv4` (`sharesPublicAddressWith`, `checkedAt`, `error`, `failedAt`,
  and `address` only with `--show-public-addresses`; absent when not checked yet) and
  `bonding` (per running SRTLA output: `output`, `state` `bonded` or `standby`, and
  `sharesPublicAddressWith` for standby); plus `reportedAt`. Exit 7 (`DeviceStateUnavailable`)
  when the device has never reported its interfaces. **Public addresses are personal data.**

## Wi-Fi

- `heliosctl --json wifi list <device>`: the networks the device joins: `ssid`, `security`
  (`personal` or `open`), `hidden`. Never passphrases.
- `heliosctl --json wifi set <device> --ssid <ssid> --passphrase-file <file|->`, or `--open`
  for an open network; `--hidden` if it doesn't broadcast its SSID; `--dry-run` (shows the
  passphrase as `***`). The device applies it before the command returns.
- `heliosctl --json wifi remove <device> --ssid <ssid>`, with `--dry-run`. Returns
  `{"removed": true|false}`.

## Schema

- `heliosctl schema`: the Control API's OpenAPI description.
- `heliosctl schema <Operation>`: one operation's method, path, input and output, such as
  `heliosctl schema SetStreamDestination`.
