use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use rpow2_authorized_miner::pow::{default_threads, parse_hex, search_pow};

#[derive(Debug, Parser)]
#[command(name = "local_pow_bench")]
#[command(about = "Local SHA-256 proof-of-work benchmark")]
struct Cli {
    #[arg(long)]
    prefix: String,

    #[arg(long)]
    difficulty: u32,

    #[arg(long)]
    threads: Option<usize>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let prefix = parse_hex(&cli.prefix).context("--prefix must be even-length hex")?;
    let threads = cli.threads.unwrap_or_else(default_threads);

    let started_at = Instant::now();
    let solution = search_pow(prefix, cli.difficulty, threads)?;
    let elapsed = started_at.elapsed();
    let seconds = elapsed.as_secs_f64();
    let mh_per_second = if seconds > 0.0 {
        solution.hashes as f64 / 1_000_000.0 / seconds
    } else {
        0.0
    };

    println!("solution_nonce: {}", solution.nonce);
    println!("total_hashes: {}", solution.hashes);
    println!("elapsed_secs: {:.6}", seconds);
    println!("average_mh_s: {:.2}", mh_per_second);
    println!("threads: {}", threads);

    Ok(())
}
