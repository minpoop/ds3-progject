//! The raw RSA public operation Dark Souls III's archive headers (`Data0.bhd` ...) are protected with.
//!
//! The headers are not encrypted for secrecy: the game's own files hold the public key as plain text and the header was
//! made with the private one. Every block of `k` bytes (`k` = the modulus length, 256 for 2048 bits) is read as a
//! big-endian number `c`, raised to the public exponent modulo the modulus, and the result is written as exactly `k - 1`
//! bytes (left-padded with zeros). The plain text of a correct key starts with the ASCII bytes `BHD5`.
use crate::keys::RsaPublicKey;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RsaError {
    /// `decrypt_block` was given something else than exactly one block.
    BlockLength { expected: usize, found: usize },
    /// The data ends in the middle of a block.
    PartialBlock { trailing: usize },
    /// A block is not smaller than the modulus, so it cannot have been made with this key.
    BlockTooLarge { block: usize },
    /// The result does not fit in `k - 1` bytes, so the key is not the one the data was made with.
    ResultTooLarge { block: usize },
    /// A helper thread failed.
    Worker,
}

impl fmt::Display for RsaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RsaError::BlockLength { expected, found } => write!(f, "one block is {expected} bytes, not {found}"),
            RsaError::PartialBlock { trailing } => write!(f, "the data ends in the middle of a block ({trailing} bytes left over)"),
            RsaError::BlockTooLarge { block } => write!(f, "block {block} is too large for this key"),
            RsaError::ResultTooLarge { block } => write!(f, "block {block} does not decrypt to a plain block with this key"),
            RsaError::Worker => write!(f, "a helper thread failed"),
        }
    }
}

impl std::error::Error for RsaError {}

fn decrypt_into(key: &RsaPublicKey, block: &[u8], out: &mut [u8], index: usize) -> Result<(), RsaError> {
    let c = num_bigint::BigUint::from_bytes_be(block);
    if &c >= key.n() {
        return Err(RsaError::BlockTooLarge { block: index });
    }
    let bytes = c.modpow(key.e(), key.n()).to_bytes_be();
    let Some(start) = out.len().checked_sub(bytes.len()) else {
        return Err(RsaError::ResultTooLarge { block: index });
    };
    out.fill(0);
    out[start..].copy_from_slice(&bytes);
    Ok(())
}

/// Decrypts one block (exactly `key.modulus_len()` bytes) to `modulus_len() - 1` bytes.
pub fn decrypt_block(key: &RsaPublicKey, block: &[u8]) -> Result<Vec<u8>, RsaError> {
    let k = key.modulus_len();
    if block.len() != k {
        return Err(RsaError::BlockLength { expected: k, found: block.len() });
    }
    let mut out = vec![0u8; k - 1];
    decrypt_into(key, block, &mut out, 0)?;
    Ok(out)
}

/// Does the first block of `data` decrypt, with this key, to something that starts with `magic`? This is how the key of an
/// archive is found: the candidates are tried on the first block only.
pub fn first_block_starts_with(key: &RsaPublicKey, data: &[u8], magic: &[u8]) -> bool {
    let Some(block) = data.get(..key.modulus_len()) else { return false };
    decrypt_block(key, block).is_ok_and(|plain| plain.starts_with(magic))
}

fn thread_count(blocks: usize) -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    cores.min(8).min(blocks / 64 + 1).max(1)
}

/// Decrypts every complete block of `data` (several threads for a big header). Returns the plain bytes and how many bytes at
/// the end of `data` did not make up a whole block (those are not decrypted).
pub fn decrypt_complete_blocks(key: &RsaPublicKey, data: &[u8]) -> Result<(Vec<u8>, usize), RsaError> {
    decrypt_with_threads(key, data, thread_count(data.len() / key.modulus_len()))
}

fn decrypt_with_threads(key: &RsaPublicKey, data: &[u8], threads: usize) -> Result<(Vec<u8>, usize), RsaError> {
    let k = key.modulus_len();
    let blocks = data.len() / k;
    let trailing = data.len() - blocks * k;
    let mut out = vec![0u8; blocks * (k - 1)];
    if blocks == 0 {
        return Ok((out, trailing));
    }
    let threads = threads.clamp(1, blocks);
    let per_thread = blocks.div_ceil(threads);
    let input = &data[..blocks * k];
    if threads == 1 {
        for (i, (block, plain)) in input.chunks_exact(k).zip(out.chunks_exact_mut(k - 1)).enumerate() {
            decrypt_into(key, block, plain, i)?;
        }
        return Ok((out, trailing));
    }
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (t, (inp, outp)) in input.chunks(per_thread * k).zip(out.chunks_mut(per_thread * (k - 1))).enumerate() {
            handles.push(scope.spawn(move || -> Result<(), RsaError> {
                for (i, (block, plain)) in inp.chunks_exact(k).zip(outp.chunks_exact_mut(k - 1)).enumerate() {
                    decrypt_into(key, block, plain, t * per_thread + i)?;
                }
                Ok(())
            }));
        }
        for handle in handles {
            handle.join().map_err(|_| RsaError::Worker)??;
        }
        Ok::<(), RsaError>(())
    })?;
    Ok((out, trailing))
}

/// Decrypts a whole header. A trailing partial block is an error.
pub fn decrypt_header(key: &RsaPublicKey, data: &[u8]) -> Result<Vec<u8>, RsaError> {
    let (plain, trailing) = decrypt_complete_blocks(key, data)?;
    if trailing != 0 {
        return Err(RsaError::PartialBlock { trailing });
    }
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::keys::test_key;

    #[test]
    fn a_header_made_with_the_private_key_reads_back_with_the_public_one() {
        let key = test_key(0);
        let mut plain = b"BHD5".to_vec();
        plain.extend((0..1000u32).map(|i| (i * 13 + 5) as u8));
        let encrypted = key.encrypt_header(&plain);
        assert_eq!(encrypted.len(), 256 * plain.len().div_ceil(255));
        let back = decrypt_header(&key.public, &encrypted).unwrap();
        assert_eq!(back.len(), 255 * 4);
        assert_eq!(&back[..plain.len()], &plain[..]);
        assert!(back[plain.len()..].iter().all(|b| *b == 0), "the last block was padded with zeros");
    }

    #[test]
    fn only_the_right_key_gives_bhd5() {
        let (a, b) = (test_key(0), test_key(1));
        let encrypted = a.encrypt_header(b"BHD5 and then some more");
        assert!(first_block_starts_with(&a.public, &encrypted, b"BHD5"));
        assert!(!first_block_starts_with(&b.public, &encrypted, b"BHD5"));
        assert!(!first_block_starts_with(&a.public, &encrypted[..255], b"BHD5"), "not a whole block");
        assert!(!first_block_starts_with(&a.public, &[], b"BHD5"));
        assert!(!first_block_starts_with(&a.public, &[0xFF; 256], b"BHD5"), "larger than the modulus");
        // the wrong key either fails or gives something else, never a panic
        for key in [&a.public, &b.public] {
            let _ = decrypt_block(key, &encrypted[..256]);
        }
    }

    #[test]
    fn many_blocks_use_several_threads_and_keep_their_order() {
        let key = test_key(1);
        let plain: Vec<u8> = (0..255 * 40u32).map(|i| (i % 253) as u8 | 1).collect();
        let encrypted = key.encrypt_header(&plain);
        let (back, trailing) = decrypt_complete_blocks(&key.public, &encrypted).unwrap();
        assert_eq!((trailing, &back), (0, &plain));
        for threads in [1, 2, 3, 7, 40, 1000] {
            let (back, trailing) = decrypt_with_threads(&key.public, &encrypted, threads).unwrap();
            assert_eq!((trailing, &back), (0, &plain), "{threads} threads");
        }
        // an error in one thread's blocks is reported with the right block number
        let mut broken = encrypted.clone();
        broken[30 * 256..31 * 256].fill(0xFF);
        assert_eq!(decrypt_with_threads(&key.public, &broken, 4), Err(RsaError::BlockTooLarge { block: 30 }));
        assert_eq!(thread_count(0), 1);
        assert!(thread_count(1_000_000) <= 8);
    }

    #[test]
    fn a_partial_block_is_an_error_for_headers_but_reported_by_the_lenient_function() {
        let key = test_key(0);
        let mut encrypted = key.encrypt_header(b"BHD5 something");
        encrypted.extend([1, 2, 3]);
        assert_eq!(decrypt_header(&key.public, &encrypted), Err(RsaError::PartialBlock { trailing: 3 }));
        let (plain, trailing) = decrypt_complete_blocks(&key.public, &encrypted).unwrap();
        assert_eq!((plain.len(), trailing), (255, 3));
        assert!(plain.starts_with(b"BHD5 something"));
        let (empty, trailing) = decrypt_complete_blocks(&key.public, &[9; 100]).unwrap();
        assert_eq!((empty.len(), trailing), (0, 100));
        assert!(decrypt_header(&key.public, &[]).unwrap().is_empty());
    }

    #[test]
    fn blocks_that_cannot_be_ciphertext_are_refused() {
        let key = test_key(0);
        assert_eq!(decrypt_block(&key.public, &[0xFF; 256]), Err(RsaError::BlockTooLarge { block: 0 }));
        assert_eq!(decrypt_block(&key.public, &[1; 100]), Err(RsaError::BlockLength { expected: 256, found: 100 }));
        let mut data = key.encrypt_header(&[0x42; 600]);
        let k = 256;
        data[2 * k..3 * k].fill(0xFF);
        assert_eq!(decrypt_header(&key.public, &data), Err(RsaError::BlockTooLarge { block: 2 }));
        // all zero is a legal block (the number 0)
        assert_eq!(decrypt_block(&key.public, &[0; 256]).unwrap(), vec![0; 255]);
        // a value that decrypts to more than k - 1 bytes belongs to another key
        let other = test_key(1);
        let c = other.encrypt_block(&[0x42; 255]);
        let r = decrypt_block(&key.public, &c);
        assert!(matches!(r, Err(RsaError::ResultTooLarge { .. }) | Err(RsaError::BlockTooLarge { .. }) | Ok(_)));
    }

    /// Timing of the public operation: `cargo test -p ashen-ds3data --release -- --ignored --nocapture rsa_speed`
    #[test]
    #[ignore]
    fn rsa_speed() {
        let key = test_key(0);
        // 16 real blocks, repeated: the time of the public operation does not depend on the content
        let sixteen = key.encrypt_header(&(0..255 * 16u32).map(|i| (i % 253) as u8 | 1).collect::<Vec<u8>>());
        let data: Vec<u8> = sixteen.iter().copied().cycle().take(sixteen.len() * 125).collect();
        let started = std::time::Instant::now();
        let (plain, _) = decrypt_complete_blocks(&key.public, &data).unwrap();
        println!("2000 blocks in {:?} with {} threads ({} plain bytes)", started.elapsed(), thread_count(2000), plain.len());
    }
}
