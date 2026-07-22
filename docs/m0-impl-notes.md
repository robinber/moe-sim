# M0 implementation notes

## Delivered

Pure global-capacity validation lives in `moe-sim-core`:

- `CapacityError` (run-level feasibility; separate from `ManifestError`)
- `ModelManifest::validate_global_capacity`
- Unit tests covering design cases plus review P1 (first oversize key order,
  manifest pass before independent event failure) and review §6 extras

CLI is deferred for this PR.

## Residual (later adapter slice)

Choose JSONL events plus a simple TOML or JSON manifest, add `moe-sim-cli`,
wire `capacity check` to `ModelManifest::validate_global_capacity`, add one
valid and one failing fixture, and test the non-zero capacity-error exit path.
`trace inspect` remains optional.
