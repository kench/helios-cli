# heliosctl errors

With `--json`, a failure prints one JSON document on stderr:

```json
{"error": {"type": "WifiNetworkRefused", "message": "...", "reason": "no_adapter", "status": 409}}
```

`type` is the API's error, or one of heliosctl's own (`InvalidInput`, `NoCredentials`,
`TimedOut`, `Refused`, `Error`). The exit code groups them:

| Exit | Types | Meaning |
| --- | --- | --- |
| 1 | `Error`, `HTTP5xx`, `InternalError` | A network problem or a fault in the service. Retry once, with the same client token for a change |
| 2 | `InvalidInput`, `ValidationException`, `InvalidDestination` (with `part`: `scheme`, `host`, `port`, `streamid`, `latency`, `passphrase`, ...) | The input is wrong; the message says how |
| 3 | `DeviceNotFound`, `StreamNotFound` | No such device or stream |
| 4 | `SettingUnavailable` (no such profile or source on the device), `StreamChangeRefused` (the device refused a stream change and changed nothing; `refused`: each `setting` and `reason`), `WifiNetworkRefused` (`reason`: `no_adapter`, `too_many`, `invalid_ssid`, `invalid_passphrase`, `apply_failed`, `no_secrets_key`), `Refused` (the device rejected a change while `--wait` waited: see the stream's `rejected`) | Refused |
| 5 | `DeviceUnreachable`, `TimedOut` | The device didn't answer or apply in time. It may still: read the state before retrying |
| 6 | `NoCredentials`, `AccessDeniedException` (HTTP 403) | Not signed in, or not allowed |
| 7 | `DeviceStateUnavailable` | The device hasn't reported that part of its state yet |
| 8 | `RateLimited` | Throttled even after heliosctl's own retries |

A stream's `rejected` list holds the device's own refusals: `{"setting", "reason", "at"}`, such
as a camera that isn't plugged in.
