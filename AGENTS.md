# AGENTS.md

`heliosctl`, the Helios Control API's command-line client (HLD 7.3.4.1). Start with the Notion
page "Helios - Project Status and Handoff"; open work is the Notion board "Helios Tasks".

## Layout

```
src/cli.rs      parsing, commands, running a command and its exit code
src/client.rs   HTTPS with SigV4 (execute-api), retries, the API's errors
src/streams.rs  streams: changes with client tokens, --dry-run, --wait; passphrase redaction
src/inventory.rs  inputs, profiles, wifi (passphrase from a file or stdin only)
src/destination.rs  srt:// and srtla:// checks, ported from helios-cloud's Destinations.java
src/config.rs   endpoint, profile and region: flag, environment, ~/.config/heliosctl/config.toml
src/error.rs    failures and the documented exit codes
src/output.rs   --json (the API's response) or a summary for people
schema/         the contract release's HeliosControl.openapi.json (heliosctl schema)
contract/       git submodule: helios-contract, pinned to the tag the schema came from
tests/          the binary end to end against a local server; manpages.rs checks man/
examples/manpages.rs  generates man/ from the clap definitions (cargo run --example manpages)
man/            the generated manual pages (checked in; never edit by hand)
skills/heliosctl/  the agent skill (Agent Skills format: SKILL.md + references/); tests/skill.rs
                checks it against the CLI. Update it with any change to commands or flags
```

## Rules

- Same as helios: `unsafe_code = "deny"`, no `unwrap` (denied), clippy `-D warnings`, nightly
  rustfmt, at least 90% line coverage, Conventional Commits, squash merges, branch first.
- Agent-facing behaviour is a contract: `--json` prints one document on stdout and errors as JSON
  on stderr; exit codes as in the README; never interactive (no prompts, ever). Change them only
  with the owner, and note it in HLD 7.3.4.1.
- Credentials only from the AWS credential chain; never store or print a secret, never take one
  as an argument (a Wi-Fi passphrase comes from stdin or a file).
- `destination.rs` must refuse exactly what the service refuses: change it together with
  helios-cloud's `Destinations.java` and its test cases.
- When the contract moves to a new tag, update the submodule and `schema/` together (a test
  checks they match).
- Don't push tags or change repository settings without the owner's OK; an OK for a release
  covers that one tag.
