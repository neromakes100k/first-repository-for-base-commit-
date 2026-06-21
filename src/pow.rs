use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

const HASH_BATCH_SIZE: u64 = 262_144;

#[derive(Debug, Clone)]
pub struct PowSolution {
    pub nonce: u64,
    pub hashes: u64,
}

pub fn default_threads() -> usize {
    thread::available_parallelism().map_or(1, usize::from)
}

pub fn search_pow(prefix: Vec<u8>, difficulty: u32, threads: usize) -> anyhow::Result<PowSolution> {
    validate_search_options(difficulty, threads)?;

    let found = Arc::new(AtomicBool::new(false));
    let total_hashes = Arc::new(AtomicU64::new(0));
    let solution = Arc::new(Mutex::new(None));

    thread::scope(|scope| {
        for thread_id in 0..threads {
            let found = Arc::clone(&found);
            let total_hashes = Arc::clone(&total_hashes);
            let solution = Arc::clone(&solution);
            let prefix = &prefix;

            scope.spawn(move || {
                search_worker(
                    prefix,
                    difficulty,
                    thread_id as u64,
                    threads as u64,
                    found,
                    total_hashes,
                    solution,
                );
            });
        }
    });

    let solution = solution
        .lock()
        .expect("solution mutex poisoned")
        .clone()
        .context("no solution found before nonce space was exhausted")?;
    Ok(solution)
}

pub fn parse_hex(value: &str) -> anyhow::Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        bail!("hex value must have an even number of characters");
    }

    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0])?;
            let low = hex_value(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

pub fn write_nonce_le(nonce: u64, output: &mut [u8]) {
    output.copy_from_slice(&nonce.to_le_bytes());
}

pub fn trailing_zero_bits(bytes: &[u8]) -> u32 {
    let mut zero_bits = 0;
    for byte in bytes.iter().rev() {
        if *byte == 0 {
            zero_bits += 8;
        } else {
            return zero_bits + byte.trailing_zeros();
        }
    }
    zero_bits
}

fn validate_search_options(difficulty: u32, threads: usize) -> anyhow::Result<()> {
    if threads == 0 {
        bail!("--threads must be greater than 0");
    }
    if difficulty > 256 {
        bail!("--difficulty must be between 0 and 256");
    }
    if threads as u128 > u64::MAX as u128 {
        bail!("--threads is too large");
    }
    Ok(())
}

fn search_worker(
    prefix: &[u8],
    difficulty: u32,
    mut nonce: u64,
    step: u64,
    found: Arc<AtomicBool>,
    total_hashes: Arc<AtomicU64>,
    solution: Arc<Mutex<Option<PowSolution>>>,
) {
    let mut input = Vec::with_capacity(prefix.len() + 8);
    input.extend_from_slice(prefix);
    input.resize(prefix.len() + 8, 0);

    let mut local_hashes = 0_u64;
    loop {
        if found.load(Ordering::Relaxed) {
            flush_hashes(local_hashes, &total_hashes);
            return;
        }

        write_nonce_le(nonce, &mut input[prefix.len()..]);
        let digest = Sha256::digest(&input);
        local_hashes += 1;

        if trailing_zero_bits(&digest) >= difficulty {
            let previous = total_hashes.fetch_add(local_hashes, Ordering::Relaxed);
            found.store(true, Ordering::Relaxed);
            let mut guard = solution.lock().expect("solution mutex poisoned");
            if guard.is_none() {
                *guard = Some(PowSolution {
                    nonce,
                    hashes: previous + local_hashes,
                });
            }
            return;
        }

        if local_hashes == HASH_BATCH_SIZE {
            flush_hashes(local_hashes, &total_hashes);
            local_hashes = 0;
        }

        let Some(next_nonce) = nonce.checked_add(step) else {
            flush_hashes(local_hashes, &total_hashes);
            return;
        };
        nonce = next_nonce;
    }
}

fn flush_hashes(hashes: u64, total_hashes: &AtomicU64) {
    if hashes > 0 {
        total_hashes.fetch_add(hashes, Ordering::Relaxed);
    }
}

fn hex_value(byte: u8) -> anyhow::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("hex value contains non-hex character: {}", byte as char),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_digest_has_256_trailing_zero_bits() {
        assert_eq!(trailing_zero_bits(&[0; 32]), 256);
    }

    #[test]
    fn low_bits_of_last_byte_are_counted() {
        let mut digest = [0xff; 32];
        digest[31] = 0b0001_0000;
        assert_eq!(trailing_zero_bits(&digest), 4);
    }

    #[test]
    fn one_in_last_byte_has_no_trailing_zero_bits() {
        let mut digest = [0xff; 32];
        digest[31] = 0b0000_0001;
        assert_eq!(trailing_zero_bits(&digest), 0);
    }

    #[test]
    fn zero_last_byte_continues_into_previous_byte() {
        let mut digest = [0xff; 32];
        digest[30] = 0b0000_1000;
        digest[31] = 0;
        assert_eq!(trailing_zero_bits(&digest), 11);
    }

    #[test]
    fn nonce_is_little_endian_u64() {
        let mut output = [0; 8];
        write_nonce_le(1, &mut output);
        assert_eq!(output, [1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn prefix_must_be_even_length_hex() {
        assert!(parse_hex("abc").is_err());
        assert!(parse_hex("xx").is_err());
        assert_eq!(parse_hex("deadBEEF").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }
}
