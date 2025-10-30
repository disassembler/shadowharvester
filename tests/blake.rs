#[cfg(test)]
mod tests {
    //use cryptoxide::digest::Digest;
    use cryptoxide::{
        hashing::blake2b::{self, Blake2b, Context},
        kdf::argon2,
    };
    use hex_literal::hex;

    // Helper function to convert a byte slice to a hex String for clean error messages
    fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes.iter()
             // Format each byte as two hexadecimal digits, padding with zero if needed
             .map(|b| format!("{:02x}", b))
             .collect()
    }

    fn print_hex(name: &str, data: &[u8]) {
        print!("{}: ", name);
        for byte in data.iter() {
            print!("{:02x}", byte);
        }
        println!();
    }

    fn blake2b_seed_logic(key: &[u8], size: usize) -> [u8; 32] {
        // Output size is fixed at 32 bytes (256 bits)
        let data = vec![0; size];
        let size_bytes = (data.len() as u32).to_le_bytes();
        println!("{:?}", key);

        // FIX: Use Blake2b::Context<32> for compile-time fixed 32-byte output,
        // matching the implicit constraint of the implementation code (rom.rs uses Context<256>,
        // which corresponds to a 32-byte output when using Blake2b types).
        let seed = blake2b::Context::<256>::new()
            .update(&size_bytes)
            .update(key)
            .finalize();
        print_hex("Input Size (4-bytes LE)", &size_bytes);
        print_hex("Input Key", key);
        print_hex("Blake2b-256 Seed (32 bytes)", &seed);

        seed
    }

    #[test]
    fn test_blake2b_256_seed_compatibility() {
        // Parameters extracted from hash.rs main() function:
        const MB: usize = 1024 * 1024;
        const GB: usize = 1024 * MB;
        const ROM_SIZE: usize = 1 * GB;

        // key = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011".as_bytes()
        let key_bytes = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011".as_bytes();

        // The expected hash (the "Blake2b-256 Seed (32 bytes)" printed in hash.rs)
        // Blake2b-256 Seed: 118a9e880ecef64f9dff3eb94db22b0f417524697ae4b9e8037b1328de0765fe
        let expected_seed: [u8; 32] = hex!("118a9e880ecef64f9dff3eb94db22b0f417524697ae4b9e8037b1328de0765fe");

        // --- ACTUAL CALCULATION ---
        let actual_seed = blake2b_seed_logic(&key_bytes, ROM_SIZE);

        // 4. Custom assertion: Print hex on failure.
        assert!(
            actual_seed == expected_seed,
            "Implementation Logic Check Failed. The calculated seed does not match the expected value from the ROM generation printout.\n\
             Expected: {}\n\
             Actual:   {}",
            bytes_to_hex(&expected_seed),
            bytes_to_hex(&actual_seed)
        );
    }

    #[test]
    fn test_hprime_with_key_bytes_hash() {
        // Parameters extracted from hash.rs main() function:
        const MB: usize = 1024 * 1024;
        const GB: usize = 1024 * MB;
        const ROM_SIZE: usize = 1 * GB;

        let pre_size = 16 * MB;

        // key = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011".as_bytes()
        let key_bytes = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011".as_bytes();

        let expected_digest = hex!("b89b48b36e71912f26e2d57c59996621f248d827203fa2206e3a090aa37e24");

        let mut mixing_buffer = vec![0; pre_size];

        // --- ACTUAL CALCULATION ---
        let seed_hash = blake2b_seed_logic(&key_bytes, ROM_SIZE);
        argon2::hprime(&mut mixing_buffer, &seed_hash);


        // 4. Custom assertion: Print hex on failure.
        assert!(
            mixing_buffer[0..31] == expected_digest,
            "hprime key bytes small failed\n\
             Expected: {}\n\
             Actual:   {}",
            bytes_to_hex(&expected_digest),
            bytes_to_hex(&mixing_buffer[0..31])
        );
    }

    #[test]
    fn test_hprime_with_large_bytes_hash() {
        // Parameters extracted from hash.rs main() function:
        const MB: usize = 1024 * 1024;
        const GB: usize = 1024 * MB;
        const ROM_SIZE: usize = 1 * GB;

        let pre_size = 16 * MB;

        // key = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011".as_bytes()
        let key_bytes = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40".as_bytes();

        let expected_digest = hex!("a45474211bc6322703a8c9bceb47fa1feb4895ed76e55f695c594fdbe3c99c");

        let mut mixing_buffer = vec![0; pre_size];

        // --- ACTUAL CALCULATION ---
        argon2::hprime(&mut mixing_buffer, &key_bytes);

        print_hex("large prime", &mixing_buffer[0..31]);


        // 4. Custom assertion: Print hex on failure.
        assert!(
            mixing_buffer[0..31] == expected_digest,
            "hprime large bytes failed\n\
             Expected: {}\n\
             Actual:   {}",
            bytes_to_hex(&expected_digest),
            bytes_to_hex(&mixing_buffer[0..31])
        );
    }

    use std::convert::TryInto;

    // --- CONSTANTS FOR BLAKE2B-512 ISOLATION TEST ---

    // A generic 64-byte seed
    const BLAKE512_TEST_SEED_HEX: &str = "112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00";

    // The fixed string constant used in the KDF initialization
    const INPUT_STRING: &[u8] = b"generation offset base";

    // NOTE: YOU MUST RUN A RUST SNIPPET WITH THE ABOVE INPUTS TO GET THIS HASH
    // (Seed | INPUT_STRING) -> Blake2b-512 Output (64 bytes)
    const EXPECTED_BLAKE512_OUTPUT_HEX: &str = "e40906cd803e58934d39caeba810eab345e4c1903afd9d1491397152ee2502de39bc30f2fe62ec7345e573ba14745258fa317f3d987f2d82a25f2a1deab8b8c9";


    /// Test X: Verifies the integrity of the Blake2b-512 context and concatenation order.
    #[test]
    fn test_blake512_context_integrity() {
        let seed = hex::decode(BLAKE512_TEST_SEED_HEX).expect("Invalid test seed hex");
        let actual_digest_ctx = Context::<512>::new();
        let final_context = actual_digest_ctx
        .update(&seed)
        .update(INPUT_STRING);

        let actual_digest_vec = final_context.finalize();
        let actual_digest_array: [u8; 64] = actual_digest_vec.as_slice().try_into().unwrap();

        let expected_digest = hex::decode(EXPECTED_BLAKE512_OUTPUT_HEX).expect("Invalid expected hash");
        let expected_digest_array: [u8; 64] = expected_digest.as_slice().try_into().unwrap();

        assert_eq!(
            actual_digest_array,
            expected_digest_array,
            "Blake2b-512 Context Mismatch. Actual: {} Expected: {}",
            hex::encode(actual_digest_array),
            EXPECTED_BLAKE512_OUTPUT_HEX
        );
    }
}

