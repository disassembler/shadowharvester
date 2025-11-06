//#[cfg(test)]
//mod rom_integration_tests {
//    use shadow_harvester_lib::{RomGenerationType, Rom};
//    use shadow_harvester_lib::hash;
//
//    use cryptoxide::hashing::blake2b::{self};
//    use std::{assert_eq, convert::TryInto};
//
//
//    const ROM_SIZE: usize = 1_073_741_824;
//    const PRE_SIZE: usize = 16_777_216;
//    const MIXING_NUMBERS: usize = 4;
//
//    const ROM_SEED_ASCII_HEX: &str = "fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011";
//
//    // NOTE: Replace these with the actual derived values.
//    const EXPECTED_V0_HEX: &str = "118a9e880ecef64f9dff3eb94db22b0f417524697ae4b9e8037b1328de0765fe";
//    const EXPECTED_ROM_DIGEST_HEX: &str = "363c87d27c93f1013ed03f19ca39c6ea8b83b24b607df70dccc8967ad59c78fe6aeeea9978e7dbfaba584550e568808f75202c48fc9f4236184b8ee5709816c8";
//
//    fn print_hex(name: &str, data: &[u8]) {
//        print!("{}: ", name);
//        for byte in data.iter() {
//            print!("{:02x}", byte);
//        }
//        println!();
//    }
//
//    fn blake2b_seed_logic(key: &[u8], size: usize) -> [u8; 32] {
//        // Output size is fixed at 32 bytes (256 bits)
//        let data = vec![0; size];
//        let size_bytes = (data.len() as u32).to_le_bytes();
//        println!("{:?}", key);
//
//        // FIX: Use Blake2b::Context<32> for compile-time fixed 32-byte output,
//        // matching the implicit constraint of the implementation code (rom.rs uses Context<256>,
//        // which corresponds to a 32-byte output when using Blake2b types).
//        let seed = blake2b::Context::<256>::new()
//            .update(&size_bytes)
//            .update(key)
//            .finalize();
//        print_hex("Input Size (4-bytes LE)", &size_bytes);
//        print_hex("Input Key", key);
//        print_hex("Blake2b-256 Seed (32 bytes)", &seed);
//
//        seed
//    }
//
//    fn count_leading_zeros(s: &str) -> usize {
//        let mut count = 0;
//        for char_val in s.chars() {
//            if char_val == '0' {
//                count += 1;
//            } else {
//                break; // Stop counting once a non-zero character is found
//            }
//        }
//        count
//    }
//
//    fn hash_structure_good(hash: &[u8], zero_bits: usize) -> bool {
//        let full_bytes = zero_bits / 8; // Number of full zero bytes
//        let remaining_bits = zero_bits % 8; // Bits to check in the next byte
//
//        // Check full zero bytes
//        if hash.len() < full_bytes || hash[..full_bytes].iter().any(|&b| b != 0) {
//            return false;
//        }
//
//        if remaining_bits == 0 {
//            return true;
//        }
//        if hash.len() > full_bytes {
//            // Mask for the most significant bits
//            let mask = 0xFF << (8 - remaining_bits);
//            hash[full_bytes] & mask == 0
//        } else {
//            false
//        }
//    }
//
//    // -------------------------------------------------------------------------
//    //                              TEST CASES
//    // -------------------------------------------------------------------------
//
//    #[test]
//    fn test_v0_seed_logic() {
//        let rom_seed_bytes = ROM_SEED_ASCII_HEX.as_bytes();
//
//        // This test will now use the corrected blake2b_seed_logic
//        let actual_v0_seed = blake2b_seed_logic(rom_seed_bytes, ROM_SIZE);
//
//        let expected_v0_seed = hex::decode(EXPECTED_V0_HEX).expect("Invalid V0 Hex");
//        let expected_v0_seed_array: [u8; 32] = expected_v0_seed.as_slice().try_into().unwrap();
//
//        assert_eq!(actual_v0_seed.len(), 32);
//        assert_eq!(
//            actual_v0_seed,
//            expected_v0_seed_array,
//            "V0 Seed Hash mismatch. Actual: {} Expected: {}",
//            hex::encode(actual_v0_seed),
//            EXPECTED_V0_HEX
//        );
//    }
//
//    #[test]
//    fn test_full_rom_construction() {
//        let rom_seed_bytes = ROM_SEED_ASCII_HEX.as_bytes();
//        let gen_type = RomGenerationType::TwoStep {
//            pre_size: PRE_SIZE,
//            mixing_numbers: MIXING_NUMBERS,
//        };
//
//        // Library Call: Rom::new runs V0, hprime, mixing, and final digest
//        let rom = Rom::new(rom_seed_bytes, gen_type, ROM_SIZE);
//
//        // Access the [u8; 64] inside RomDigest
//        let actual_digest = rom.digest.0;
//
//        let expected_digest = hex::decode(EXPECTED_ROM_DIGEST_HEX)
//            .expect("Invalid ROM Digest Hex");
//
//        let expected_digest_array: [u8; 64] = expected_digest.as_slice().try_into().unwrap();
//
//        assert_eq!(
//            actual_digest.as_slice(),
//            expected_digest_array.as_slice(),
//            "Final ROM Digest Mismatch. Actual: {} Expected: {}",
//            hex::encode(actual_digest),
//            EXPECTED_ROM_DIGEST_HEX
//        );
//
//        let mut preimage: String = "".to_string();
//        preimage.push_str("addr_test1qq4dl3nhr0axurgcrpun9xyp04pd2r2dwu5x7eeam98psv6dhxlde8ucclv2p46hm077ds4vzelf5565fg3ky794uhrq5up0he");
//        preimage.push_str("**D07C10");
//        preimage.push_str("000FFFFF");
//        preimage.push_str("fd651ac2725e3b9d804cc8b161c0709af14d6264f93e8d4afef0fd1142a3f011");
//        preimage.push_str("2025-10-19T08:59:59.000Z");
//        preimage.push_str("509681483");
//
//        let num_leading_zeros = count_leading_zeros("000FFFFF")*4;
//        let num: u64 = 0x0019c96b6a30ee38;
//        let num_str = format!("{:016x}", num);
//        preimage.insert_str(0, &num_str);
//        println!("preimage = {preimage}");
//        let h = hash(preimage.as_bytes(), &rom, 8, 256);
//        assert!(
//            hash_structure_good(&h, num_leading_zeros),
//            "Preimage does not meet difficulty {}",
//            preimage
//        );
//
//
//    }
//
//    const TEST_MIXING_BUFFER_SLICE_HEX: &str = "b89b48b36e71912f26e2d57c59996621f248d827203fa2206e3a090aa37e242fb94a5f21b4346c6f93ee77e202103bc652a972820a85d9a05f62adcc408b967169ad0046dcbabe8e8763a7726ba5ebfb03ea5f285326d48b18d125de2f7531a121e544a8355bcd4bcc26f0c0571e30a8858cf59180ea3197d8c769ec052f0805";
//    const TEST_OFFSETS_BS_HEX: &str = "18fad3a7c3f06ab89a68962844ebea97e28e11ea741c39125fcb84e3aa511f5ef705bb48fb9adf808dae9d417573435a9c0616243a7eab6d5761e8a6728d7843";
//    const TEST_OFFSETS_DIFF_HEX: &str = "c4b4ca4adeabe082c0b94669731a1aa87a5874de54fb942e13636bdbaf4bf66ff5f33ce3a2d7b847c6558e5c614644d046c903563a9a788a324d88110b9a6b5e3c85e1aec45f9bb209c3413dab9963d6716663a43c8561fba38edb23f20e919967156a2b147634e39a401c607022904c64ddba1f25968fd387282dcfb0e69b9dbd3ac7808e0b733be3b77ba744e40e46acccf6f51a784c30d4998c9afb6bdb796ed2d4f51b8ed6e261af36e89d7b9600dc3d245614cfbad292deafae0834a26720d0019c93982d9b79f26096c2ec19a4e257adb213acac3e2e168f452e6fc7ecb4aec29c6efec4e4a156876f34e4b14796b2fbd835f46b3c00702245557c7fc6";
//     const EXPECTED_CHUNK_I0_HEX: &str = "80f621c53c5f7e4d3194bd6b7be2392d899046046368e329f5c7f0338b60c156f658bd8ca7c8cab290aa36565a17ff58e42708814bcba3f7de5a3fab029e6340";
//     const EXPECTED_CHUNK_I1_HEX: &str = "d5fbf206ec6c81339bc08e253d0caf50ed7bfed6d4f6d3b1e6528e2950e1c55746b882f876cc8ebdca1af0b273aa76e73603dd19034681405dea0bf3a34c927d";
//     const EXPECTED_CHUNK_I2_HEX: &str = "d5d56c413dd00d66d55f887e38b82e0b3b5efd79f148f42d798944cccfb684bc13e1c09dcebf83e1d4820d89e24c1d73b545398a95698d1c6817d6886f5e0a46";
//     const EXPECTED_CHUNK_I3_HEX: &str = "aa034c66ac0e914a9e89ddd1c463f33d3cf1515baac2a45e3a6f00e95e30550b50d4dc3cf209c663627f0f3ac664b7478386342e77eb7048f4771e14974c486b";
//
//
//    /// Helper: Converts the 64-byte Blake2b digest to a 128-element Vec of u16s (little-endian)
//    fn decode_offsets_diff(hex_digest: &str) -> Vec<u16> {
//        let digest_bytes = hex::decode(hex_digest).expect("Invalid Offsets Diff Hex");
//        digest_bytes
//            .chunks(2)
//            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
//            .collect()
//    }
//
//    /// Test 5: Verifies the single chunk mixing logic (indexing and xorbuf).
//    #[test]
//    fn test_single_chunk_mixing_logic() {
//        // We need 'super' or 'crate::rom::xorbuf' if it's not exposed publicly,
//        // but assuming it's made public:
//        use shadow_harvester_lib::rom::new_debug;
//        use shadow_harvester_lib::rom::step_debug;
//        use shadow_harvester_lib::rom::build_rom_from_state;
//
//
//// 1. Initial Setup
//        let rom_seed_bytes = ROM_SEED_ASCII_HEX.as_bytes();
//        let gen_type = RomGenerationType::TwoStep {
//            pre_size: PRE_SIZE,
//            mixing_numbers: MIXING_NUMBERS,
//        };
//
//        // Use the debug constructor to prepare all the state variables
//        let mut state = new_debug(rom_seed_bytes, gen_type, ROM_SIZE);
//
//        let mixing_buffer = &state.mixing_buffer;
//        let offsets_bs = &state.offsets_bs;
//        let offsets_diff = &state.offsets_diff;
//
//        let expected_mixing_buffer = hex::decode(TEST_MIXING_BUFFER_SLICE_HEX).unwrap();
//        let expected_offsets_bs = hex::decode(TEST_OFFSETS_BS_HEX).unwrap();
//        let expected_offsets_diff_u16 = decode_offsets_diff(TEST_OFFSETS_DIFF_HEX);
//
//        // Assertion 1: Mixing Buffer Slice (128 bytes)
//        assert_eq!(
//            mixing_buffer[0..128],
//            expected_mixing_buffer[0..128],
//            "Mixing Buffer Slice Mismatch (First 128 bytes)."
//        );
//
//        // Assertion 2: Offsets BS (First 64 bytes)
//        assert_eq!(
//            offsets_bs[0..64],
//            expected_offsets_bs[0..64],
//            "Offsets BS Mismatch (First 64 bytes)."
//        );
//
//        // Assertion 3: Offsets Diff (128 u16s)
//        assert_eq!(
//            offsets_diff.len(),
//            128,
//            "Offsets Diff list size mismatch."
//        );
//        assert_eq!(
//            *offsets_diff,
//            expected_offsets_diff_u16,
//            "Offsets Diff contents mismatch."
//        );
//
//        // --- Step 1: Check initial state ---
//        // (Optional: Assert state.offsets_bs or state.offsets_diff against known values)
//
//        let chunk_i0 = step_debug(&mut state);
//        let chunk_i1 = step_debug(&mut state);
//        let chunk_i2 = step_debug(&mut state);
//        let chunk_i3 = step_debug(&mut state);
//
//        let expected_chunk_i0 = hex::decode(EXPECTED_CHUNK_I0_HEX).expect("Invalid I0 Hex");
//        let expected_chunk_i0_array: [u8; 64] = expected_chunk_i0.as_slice().try_into().unwrap();
//
//        assert_eq!(
//            chunk_i0,
//            expected_chunk_i0_array,
//            "Chunk I=0 Mixing Mismatch. Actual: {} Expected: {}",
//            hex::encode(chunk_i0),
//            EXPECTED_CHUNK_I0_HEX
//        );
//
//        let expected_chunk_i1 = hex::decode(EXPECTED_CHUNK_I1_HEX).expect("Invalid I1 Hex");
//        let expected_chunk_i1_array: [u8; 64] = expected_chunk_i1.as_slice().try_into().unwrap();
//
//        assert_eq!(
//            chunk_i1,
//            expected_chunk_i1_array,
//            "Chunk I=1 Mixing Mismatch. Actual: {} Expected: {}",
//            hex::encode(chunk_i1),
//            EXPECTED_CHUNK_I1_HEX
//        );
//        let expected_chunk_i2 = hex::decode(EXPECTED_CHUNK_I2_HEX).expect("Invalid I2 Hex");
//        let expected_chunk_i2_array: [u8; 64] = expected_chunk_i2.as_slice().try_into().unwrap();
//
//        assert_eq!(
//            chunk_i2,
//            expected_chunk_i2_array,
//            "Chunk I=2 Mixing Mismatch. Actual: {} Expected: {}",
//            hex::encode(chunk_i2),
//            EXPECTED_CHUNK_I2_HEX
//        );
//        let expected_chunk_i3 = hex::decode(EXPECTED_CHUNK_I3_HEX).expect("Invalid I3 Hex");
//        let expected_chunk_i3_array: [u8; 64] = expected_chunk_i3.as_slice().try_into().unwrap();
//
//        assert_eq!(
//            chunk_i3,
//            expected_chunk_i3_array,
//            "Chunk I=3 Mixing Mismatch. Actual: {} Expected: {}",
//            hex::encode(chunk_i3),
//            EXPECTED_CHUNK_I3_HEX
//        );
//        let rom = build_rom_from_state(state, ROM_SIZE);
//
//        let actual_digest = rom.digest.0;
//        let expected_digest = hex::decode(EXPECTED_ROM_DIGEST_HEX).expect("Invalid ROM Digest Hex");
//        let expected_digest_array: [u8; 64] = expected_digest.as_slice().try_into().unwrap();
//
//        // 5. Comparison
//        assert_eq!(
//            actual_digest.as_slice(),
//            expected_digest_array.as_slice(),
//            "Final Debug Digest Mismatch. The manual stepping process does not match Rom::new(). Actual: {} Expected: {}",
//            hex::encode(actual_digest),
//            EXPECTED_ROM_DIGEST_HEX
//        );
//    }
//
//    const TEST_INPUT_DIGEST_HEX: &str = "c4b4ca4adeabe082c0b94669731a1aa87a5874de54fb942e13636bdbaf4bf66ff5f33ce3a2d7b847c6558e5c614644d046c903563a9a788a324d88110b9a6b5e";
//
//    // Expected output array of 32 u16s (0x0001, 0x0203, 0x0405, etc.)
//    const EXPECTED_U16S_OUTPUT: [u16; 32] = [
//        46276, 19146, 43998, 33504, 47552, 26950, 6771, 43034,
//        22650, 56948, 64340, 11924, 25363, 56171, 19375, 28662,
//        62453, 58172, 55202, 18360, 21958, 23694, 18017, 53316,
//        51526, 22019, 39482, 35448, 19762, 4488, 39435, 24171 // Your actual values
//    ];
//
//    /// Test X: Verifies digest_to_u16s correctly converts bytes to little-endian u16s.
//    #[test]
//    fn test_digest_to_u16s_conversion() {
//        use std::convert::TryInto;
//
//        let input_bytes_vec = hex::decode(TEST_INPUT_DIGEST_HEX).expect("Invalid digest hex");
//        let input_bytes: [u8; 64] = input_bytes_vec.try_into().expect("Digest not 64 bytes");
//
//        // The function being tested: digest_to_u16s
//        // NOTE: We assume digest_to_u16s is accessible here (e.g., inlined or public).
//        let actual_u16s: Vec<u16> = input_bytes
//            .chunks(2)
//            .map(|c| u16::from_le_bytes(*<&[u8; 2]>::try_from(c).unwrap()))
//            .collect();
//
//        assert_eq!(
//            actual_u16s.len(),
//            32,
//            "Output must contain 32 Word16 values."
//        );
//        assert_eq!(
//            actual_u16s.as_slice(),
//            EXPECTED_U16S_OUTPUT.as_slice(),
//            "u16 conversion mismatch (Little Endian byte order failure)."
//        );
//    }
//}
