# RPOW2 Authorized Miner

Standalone Rust CLI tools for local RPOW2 proof-of-work benchmarking and authorized single-account mint testing.

This repository contains:

- `local_pow_bench`: local-only SHA-256 PoW benchmark.
- `rpow2_authorized_miner`: authorized CLI miner that verifies `/me`, requests `/challenge`, computes the nonce locally, and submits `/mint`.
- `skills/rpow2-authorized-miner`: Codex skill instructions for generating safe macOS/Linux/Windows run commands.

## Safety Boundaries

- The miner does not read browser cookies, keychains, password managers, or local browser profiles.
- The miner only accepts an explicit `--cookie` argument or `RPOW2_COOKIE` environment variable.
- `https://api.rpow2.com` requires explicit `--allow-production`.
- Continuous mode is single-account and should keep a conservative `--min-delay-ms`.
- The miner does not implement multi-account, proxy rotation, stealth behavior, zero-delay loops, or rate-limit bypass.

## Requirements

Install Rust toolchain `1.88.0`:

```bash
rustup toolchain install 1.88.0
```

Check the CLI:

```bash
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- --help
```

## Cookie Setup

Use a temporary environment variable. Replace the placeholder with your explicit session cookie.

macOS/Linux:

```bash
export RPOW2_COOKIE='rpow_session=...'
```

Windows PowerShell:

```powershell
$env:RPOW2_COOKIE = 'rpow_session=...'
```

Do not paste real cookies into public issues, logs, README files, or shell history you plan to share.

## Thread Count

Use your logical CPU count unless you want to benchmark a smaller value.

macOS:

```bash
sysctl -n hw.ncpu
```

Linux:

```bash
nproc
```

Windows PowerShell:

```powershell
(Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
```

For an Apple M3 MacBook Air, `--threads 8` is expected.

## Local Benchmark

This does not contact RPOW2 APIs:

```bash
cargo +1.88.0 run --release --bin local_pow_bench -- --prefix deadbeef --difficulty 8
```

Higher difficulty example:

```bash
cargo +1.88.0 run --release --bin local_pow_bench -- --prefix deadbeefcafebabe --difficulty 25
```

## Production Single Validation

Recommended first production check. This performs at most one mint attempt round.

macOS/Linux:

```bash
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- \
  --api-base https://api.rpow2.com \
  --allow-production \
  --threads 8 \
  --once \
  --mint-retries 2 \
  --mint-retry-delay-ms 750
```

Windows PowerShell:

```powershell
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- `
  --api-base https://api.rpow2.com `
  --allow-production `
  --threads 8 `
  --once `
  --mint-retries 2 `
  --mint-retry-delay-ms 750
```

## Conservative Continuous Mode

macOS/Linux:

```bash
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- \
  --api-base https://api.rpow2.com \
  --allow-production \
  --threads 8 \
  --min-delay-ms 5000 \
  --mint-retries 2 \
  --mint-retry-delay-ms 750
```

Windows PowerShell:

```powershell
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- `
  --api-base https://api.rpow2.com `
  --allow-production `
  --threads 8 `
  --min-delay-ms 5000 `
  --mint-retries 2 `
  --mint-retry-delay-ms 750
```

## Output Notes

Startup success prints:

```text
logged_in_as: owner@example.com balance_base_units: 1000000000 balance_rpow: 1 minted_base_units: 2000000 minted_rpow: 0.002 sent_base_units: 0 received_base_units: 0
```

RPOW uses 9 decimal places. `1000000000` base units equals `1` RPOW.

## Error Notes

- `session unauthorized`: the explicit cookie is invalid or expired.
- `https://api.rpow2.com requires --allow-production`: the production guard is working.
- `tls handshake eof` on `/me`: startup verification failed before any mint happened.
- `tls handshake eof` on `/challenge`: the round failed before mining; continuous mode waits `--min-delay-ms` before the next round.
- `tls handshake eof` on `/mint`: the nonce was found locally, then submission hit a retryable transport error. The miner retries the same `challenge_id` and `solution_nonce` up to `--mint-retries`.
- `mint_attempts: 1`: `/mint` succeeded first try.
- `mint_attempts: 2` or `3`: `/mint` had retryable transport failures before succeeding.

## Development

Run tests:

```bash
cargo +1.88.0 test
```

Run help:

```bash
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- --help
```
