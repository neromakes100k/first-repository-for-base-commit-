---
name: rpow2-authorized-miner
description: Use when a user wants help installing, cloning, setting up, or running the standalone RPOW2 authorized CLI miner from GitHub on Windows PowerShell, macOS, or Linux; generating safe commands for explicit RPOW2 cookie usage; checking Rust/Cargo and thread counts; running single-account production validation or conservative continuous mode; explaining base-unit wallet output, mint retry options, and TLS handshake EOF errors; without reading browser cookies, depending on another repository, or generating high-frequency/bulk mining workflows.
---

# RPOW2 Authorized Miner

Use this skill to help a user clone and run the standalone `rpow2-authorized-miner` repository:

```text
git@github.com:simple-YoungBadBoy/rpow2-authorized-miner.git
```

This skill is for the independent RPOW2 miner repository only.

## First-Time Setup

If the repository is not already present, give the user clone commands.

macOS/Linux:

```bash
git clone git@github.com:simple-YoungBadBoy/rpow2-authorized-miner.git
cd rpow2-authorized-miner
```

Windows PowerShell:

```powershell
git clone git@github.com:simple-YoungBadBoy/rpow2-authorized-miner.git
cd rpow2-authorized-miner
```

Check Rust and Cargo:

```bash
rustc --version
cargo --version
rustup toolchain install 1.88.0
```

Use the same commands in PowerShell if Rust is missing or toolchain `1.88.0` is not installed.

## Repository Checks

Before giving production run commands, verify the user is in the standalone miner repository:

macOS/Linux:

```bash
test -f Cargo.toml
test -f src/bin/rpow2_authorized_miner.rs
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- --help
```

Windows PowerShell:

```powershell
Test-Path Cargo.toml
Test-Path src/bin/rpow2_authorized_miner.rs
cargo +1.88.0 run --release --bin rpow2_authorized_miner -- --help
```

If a check fails, ask the user to clone the GitHub repository or `cd` into it before continuing.

## Cookie Handling

- Accept only an explicit cookie supplied by the user through `RPOW2_COOKIE` or `--cookie`.
- Do not read browser cookies, keychains, password managers, local browser profiles, or shell history.
- If the user pasted a real cookie, do not repeat the full value. Use `rpow_session=...`.
- Prefer temporary environment variables. Do not suggest persistent `setx` unless the user explicitly asks.

macOS/Linux:

```bash
export RPOW2_COOKIE='rpow_session=...'
```

Windows PowerShell:

```powershell
$env:RPOW2_COOKIE = 'rpow_session=...'
```

## Thread Count

Use the logical CPU count unless the user provides a specific value.

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

For an Apple M3 MacBook Air, `--threads 8` is expected. On other machines, replace `8` with the detected logical processor count.

## Run Commands

Single production validation, recommended first.

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

Conservative continuous production mode.

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

Local benchmark, no network access:

```bash
cargo +1.88.0 run --release --bin local_pow_bench -- --prefix deadbeef --difficulty 8
```

## Output Notes

- Startup success prints `logged_in_as`, `balance_base_units`, `balance_rpow`, `minted_base_units`, `minted_rpow`, `sent_base_units`, and `received_base_units`.
- RPOW uses 9 decimal places: `1000000000` base units equals `1` RPOW.
- Successful mint output includes `round`, `token_id`, `solution_nonce`, `average_mh_s`, and `mint_attempts`.

## Error Handling

- `/mint` transport errors without HTTP status are retried with the same `challenge_id` and `solution_nonce`.
- Default retry behavior is `--mint-retries 2 --mint-retry-delay-ms 750`.
- HTTP errors such as `401`, `429`, or other received statuses are not retried as mint transport failures.
- `/challenge` TLS handshake EOF is not retried by the miner; continuous mode waits `--min-delay-ms` and starts the next round.
- `session unauthorized` means the explicit cookie is invalid or expired.
- `https://api.rpow2.com requires --allow-production` means the production guard is working.

## Safety Policy

- Default to generating commands for the user to run.
- If the user asks Codex to execute production minting, use `--once` unless they clearly ask for continuous mode.
- Keep `--allow-production` explicit for `https://api.rpow2.com`.
- Keep `--min-delay-ms 5000` or higher for continuous production mode.
- Keep mint retry limits finite.
- Do not generate multi-account, zero-delay, high-frequency, stealth, proxy-rotation, browser-cookie-extraction, or rate-limit-bypass workflows.
