#[cfg(test)]
mod cip8_tests {
    use shadow_harvester_lib::cardano::*;
    use pallas::{
        crypto::key::ed25519::{SecretKey,PublicKey},
        ledger::{
            addresses::{Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart},
            traverse::ComputeHash,
        },
    };

    // --- TEST VECTORS EXTRACTED FROM UPLOADED FILES ---

    // The Message field from TsCsMSG.json
    const TC_MESSAGE: &str = "I agree to abide by the terms and conditions as described in version 1-0 of the Midnight scavenger mining process: 281ba5f69f4b943e3fb8a20390878a232787a04e4be22177f2472b63df01c200";

    // The CBOR hex content of the secret key from test.skey
    // cborHex: "5820e38c7887b3777c5204a38ce43f204c2c3aa9a0737ed8ee0fce0d4f993ec146b9"
    // The actual 32-byte key is the last 32 bytes after 5820.
    const SKEY_HEX: &str = "e38c7887b3777c5204a38ce43f204c2c3aa9a0737ed8ee0fce0d4f993ec146b9";

    // Expected outputs from signedTsCsMGS.json:
    const EXPECTED_PUBKEY_HEX: &str = "4497c0ef04fd9dd9b9d9abc2d8f19d8d09e69ae335c4355b7764c67e167d7f8e";
    const EXPECTED_ADDRESS_BECH32: &str = "addr1vxwce7p2uh9g0tjmxuyx3s7d96m7cq068pd863m8p3e0p9qjxpkqz";
    const EXPECTED_SIGNATURE_HEX: &str = "50832da3a87ff019c799a74c910e451271195b9d6a1273cf1d8a83caf4228228fe554a6aa8b89aa8f8ccf3e7bfc02c976c0514f28c5e5d97512af08186148c0e";


    // A helper function to derive the address from the secret key (matching the server's expected derivation)
    fn derive_address_from_skey(sk: &SecretKey) -> (pallas_crypto::key::ed25519::PublicKey, String) {
        let vk = sk.public_key();
        let addr = ShelleyAddress::new(
            Network::Mainnet,
            ShelleyPaymentPart::key_hash(vk.compute_hash()),
            ShelleyDelegationPart::Null
        );

        (vk, addr.to_bech32().unwrap())
    }

    #[test]
    /// Tests that the correct public key and base address (with null staking part) are derived from the secret key.
    fn test_address_derivation_from_skey() {
        let keypair = generate_cardano_key_pair_from_skey(&SKEY_HEX.to_string());

        let vk_hex = hex::encode(keypair.1.as_ref());

        let addr = keypair.2.to_bech32().unwrap().to_string();


        // 1. Verify Public Key
        assert_eq!(
            vk_hex,
            EXPECTED_PUBKEY_HEX,
            "Public Key mismatch. Expected: {}, Derived: {}",
            EXPECTED_PUBKEY_HEX,
            vk_hex
        );

        // 2. Verify Bech32 Address (Assuming Null staking part for address starting with 'addr1vxw')
        assert_eq!(
            addr,
            EXPECTED_ADDRESS_BECH32,
            "Bech32 Address mismatch. Expected: {}, Derived: {}",
            EXPECTED_ADDRESS_BECH32,
            addr
        );
    }

    #[test]
    /// Tests that the core Ed25519 signature component matches the expected value
    /// when signing the T&C message hash.
    fn test_cip8_core_signature_match() {
        let keypair = generate_cardano_key_pair_from_skey(&SKEY_HEX.to_string());

        let vk_hex = hex::encode(keypair.1.as_ref());

        // Sign the message hash using the SecretKey
        let signature = cip8_sign(&keypair, TC_MESSAGE);
        let signature_hex = hex::encode(signature.0);


        // We check if the unique signature component matches the value seen in the JS output.
        // The CIP-8 wrapper/CBOR is ignored here, focusing only on the raw signature bytes.
        assert_eq!(
            signature_hex,
            EXPECTED_SIGNATURE_HEX,
            "Core Ed25519 Signature mismatch. Expected: {}, Derived: {}",
            EXPECTED_SIGNATURE_HEX,
            signature_hex
        );
    }
}
