//! Two throwaway 2048-bit RSA key pairs made with `openssl genrsa` for the tests, and the "private key" side that the
//! real game files were made with: a block of plain bytes is raised to the private exponent. No game key is in this file.
use crate::keys::RsaPublicKey;
use num_bigint::BigUint;

const KEYS: [(&str, &str); 2] = [
    (
        "c56e17e22c67e63839d4c790f4416af143023598c3591ac17502b40e8bada08c1c6485f8b6a1bd72b1f93364ca41288736dc2150608444d1eff9d945df4bee6fa24747d1fe151da88f947eed4a0c1efe2cb6f208b8b33f3b9ae2abbfa70304e24587170c878b2eb7e456c3e95383e402a30911b16a4dfe1dcddf397d72cc3a51aca83baa44908d505c45de23c82aecc1c8d301595edf61bcefdfd9fbfbe2695d3f3d2561f8cb55dd4b4ebbc3b9f55d416eb1d692d919eeae1a8ebb4afc86e8bdfd010e752c590e7cb4e2af406882a649befc3c9ab9502280ceeca80a1f2f62fabea82d4a529c583f17656b0edc9d1110874f086a3c1742639f665c5fab35ffb9",
        "dbdec1e67681b75842bf7f09259c7d16f7034ce1361cd3b94330a189d970a1eda7e923725b224133ac94d4f198757c47298779ea63b2aa5a0b217b215f8ed699a8662dcbe5656ac44032a63069589a638334b2d397aeb7eb0f9104a8c9bc8901dd609700b32b03fa73e7550352a8fd931a328d6bc1f1dde11f574c553335d27311c1853be47273320c86bff14e4812d5c668dbf022fc6ed84146d60736b441d68d095dab90683dc4a3febdd21385f158fbf2728cd89eebe848dbf8f2fbf951b2d148b07866888fa65af546021e2dab66141a34491b761416532305263c0babfc405b3b4f84fa48cd04c28cad8115096aa091927cafe58ad9b9a5e5a5384f841",
    ),
    (
        "c54a55efd8b720c08992e40ff07bb890a58a28d1ff475086b04487974e9cc8bc604b225a17ba2993c24b3652449c8872d848af7be8c34b6fb82e9d58fb110a5c1c375037633f3d0ba62775e62e67ccf31bc15c9894065e402d47dc43ea3ba87a87c398366e04cb006548919b8c4a4cb6ee9bf2721cba5e12bea22120199e97410e5acd059a6ce0a20254a432ebcec74cb27f61e0e14878954e6185d51aa66eee2fe71abc6b2b688a632380aa0a7d680c607abd2c13eb8a428aea099d7c556c3954d7dd00e881795ba66cfe3e19a3691cbcb3dd29583948131c0a1476aa060713bcdb5294e2d921f7b655a7278e2a93cae6390f8fdfb3d8e3b4c82393b56ea4cb",
        "dce16f7aaab471c41e428978c452953dc742f9eb6255d81f73941b5a43768a7c9cdd2c5b41854e01fd32c7b6837fdd244c6729a673357317b9558c6f3abd714e7ed34ec93421c644172f7172d686fae6938480c432608ed51087b660ddb5bd2d5c77ed1fc33d3529fee5fdb49b49f4143aeb4dc7d9c7588613f8360357ab941a174feabb53238ceba2467ac7a072a69091d1e1e328f94ef20507a22a1f829b53c2a58fe42f678324a4832d9ef9ec8e5608a5c46be5e1f117594a54a970d29eecaeacac4fc8b5d45af082a502aebb8534b8bc2e71fb0d87262ab0c9310042f7ccd06f2dfe3de32131bdcce286d83c1c8a36866a3eabd0b78e75f7487b6e3cb99",
    ),
];

/// Fingerprints of the two test keys.
pub const FINGERPRINTS: [&str; 2] = ["87febfc8", "b2969406"];

/// A test key pair.
#[derive(Clone)]
pub struct TestKey {
    pub public: RsaPublicKey,
    d: BigUint,
}

/// Test key `index` (0 or 1).
pub fn test_key(index: usize) -> TestKey {
    let (n, d) = KEYS[index % KEYS.len()];
    let n = BigUint::parse_bytes(n.as_bytes(), 16).expect("hex modulus");
    let d = BigUint::parse_bytes(d.as_bytes(), 16).expect("hex exponent");
    TestKey { public: RsaPublicKey::new(n, BigUint::from(65537u32)).expect("a valid key"), d }
}

impl TestKey {
    /// The public half as a `-----BEGIN RSA PUBLIC KEY-----` block.
    pub fn pem(&self) -> String {
        self.public.to_pem()
    }

    /// One block the way the game's files were made: up to `k - 1` plain bytes (left-padded with zeros) raised to the
    /// private exponent, as `k` bytes.
    pub fn encrypt_block(&self, plain: &[u8]) -> Vec<u8> {
        let k = self.public.modulus_len();
        assert!(plain.len() < k, "a block holds at most {} plain bytes", k - 1);
        let m = BigUint::from_bytes_be(plain);
        let c = m.modpow(&self.d, &self.public.n);
        let bytes = c.to_bytes_be();
        let mut out = vec![0u8; k - bytes.len()];
        out.extend(bytes);
        out
    }

    /// A whole header: `plain` is cut in blocks of `k - 1` bytes (the last one zero-padded) and every block encrypted.
    pub fn encrypt_header(&self, plain: &[u8]) -> Vec<u8> {
        let k = self.public.modulus_len();
        let mut out = Vec::new();
        for chunk in plain.chunks(k - 1) {
            let mut block = chunk.to_vec();
            block.resize(k - 1, 0);
            out.extend(self.encrypt_block(&block));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_keys_work_and_have_the_documented_fingerprints() {
        for (i, expected) in FINGERPRINTS.iter().enumerate() {
            let key = test_key(i);
            assert_eq!(key.public.fingerprint(), *expected);
            assert_eq!(key.public.bits(), 2048);
            let plain = b"BHD5 some plain text";
            let c = key.encrypt_block(plain);
            assert_eq!(c.len(), 256);
            // public operation gives the plain block back
            let back = crate::rsa::decrypt_block(&key.public, &c).unwrap();
            assert_eq!(back.len(), 255);
            assert!(back.ends_with(plain) && back[..255 - plain.len()].iter().all(|b| *b == 0));
        }
        assert_ne!(test_key(0).public, test_key(1).public);
    }
}
