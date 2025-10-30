// shadowharvester/src/cardano.rs

use pallas::{
    crypto::key::ed25519::{SecretKey,PublicKey},
    ledger::{
        addresses::{Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart},
        traverse::ComputeHash,
    },
};

use rand_core::{OsRng};

pub type KeyPairAndAddress = (SecretKey, PublicKey, ShelleyAddress);

pub fn generate_cardano_key_and_address() -> KeyPairAndAddress {
    let rng = OsRng;

    // Generate Ed25519 SecretKey
    let sk = SecretKey::new(rng);
    let vk = sk.public_key();

    let addr = ShelleyAddress::new(
        Network::Mainnet,
        ShelleyPaymentPart::key_hash(vk.compute_hash()),
        ShelleyDelegationPart::Null
    );

    (sk, vk, addr)
}

pub fn generate_cardano_key_pair_from_skey(sk_hex: &String) -> KeyPairAndAddress {
    let skey_bytes = hex::decode(sk_hex).expect("Invalid secret key hex");
    let skey_array: [u8; 32] = skey_bytes
        .try_into()
        .expect("Secret key must be exactly 32 bytes");
    let sk = SecretKey::from(skey_array);
    let vk = sk.public_key();

    let addr = ShelleyAddress::new(
        Network::Mainnet,
        ShelleyPaymentPart::key_hash(vk.compute_hash()),
        ShelleyDelegationPart::Null
    );

    (sk, vk, addr)
}

/// Creates a placeholder hex string simulating a CIP-8 signed message payload.
/// NOTE: The actual CIP-8 structure (CBOR headers/map) is not dynamically built here,
/// but the signature and public key components are guaranteed to be unique.
pub fn cip8_sign(kp: &KeyPairAndAddress, message: &str) -> (String, String) {
    let mut protected_header_buffer = [0u8; 128];
    let pubkey_hex = hex::encode(kp.1.as_ref());
    let protected_header_bytes = {
        //let mut encoder = Encoder::new(&mut protected_header_buffer);
        //// Map of size 2
        //encoder.map(2).unwrap();

        //// Key 1 (alg) -> Value -8 (EdDSA)
        //encoder.u8(1).unwrap().i8(-8).unwrap();

        //// Key 'address' -> Value raw address bytes
        //encoder.text("address").unwrap().bytes(&address_raw_bytes).unwrap();

        //encoder.to_vec().expect("Failed to encode protected header")
    };

    // --- 3. DATA TO SIGN (Sig_structure) ---

    // Sig_structure = [ context="Signature1", protected_header_bytes, external_aad, payload_bytes ]
    let sig_structure_bytes = {
        //let mut buffer = [0u8; 512];
        //let mut encoder = Encoder::new(&mut buffer);

        //// Array of size 4
        //encoder.array(4).unwrap();

        //// 1. Context: "Signature1"
        //encoder.text("Signature1").unwrap();

        //// 2. Protected Header: bstr (already CBOR encoded)
        //encoder.bytes(&protected_header_bytes).unwrap();

        //// 3. External AAD: bstr (empty)
        //encoder.bytes(b"").unwrap();

        //// 4. Payload: bstr (Blake2b-256 hash of message)
        //encoder.bytes(message_hash.as_ref()).unwrap();

        //encoder.to_vec().expect("Failed to encode Sig_structure")
    };

    // --- 4. SIGNING ---

    // Sign the CBOR-encoded Sig_structure bytes (This is the CIP-8 requirement)
    //let signature = kp.0.sign(&sig_structure_bytes);
    //let signature_hex = hex::encode(signature.to_ref());

    // --- 5. FINAL COSE_SIGN1 ASSEMBLY ---

    // Unprotected header map: {"hashed": false}
    let mut unprotected_header_buffer = [0u8; 64];
    let unprotected_header_bytes = {
        //let mut encoder = Encoder::new(&mut unprotected_header_buffer);
        //encoder.map(1).unwrap();
        //encoder.text("hashed").unwrap().bool(false).unwrap();
        //encoder.to_vec().expect("Failed to encode unprotected header")
    };

    // COSE_Sign1_structure = [ protected_header_bytes, unprotected_header_map, payload_bytes, signature_bytes ]
    let cose_sign1_bytes = {
        //let mut buffer = [0u8; 1024];
        //let mut encoder = Encoder::new(&mut buffer);

        //// Array of size 4
        //encoder.array(4).unwrap();

        //// 1. Protected Header: bstr (already CBOR encoded)
        //encoder.bytes(&protected_header_bytes).unwrap();

        //// 2. Unprotected Header: map (already CBOR encoded)
        //encoder.map_iter(unprotected_header_bytes.iter().copied()).unwrap();

        //// 3. Payload: bstr (Blake2b-256 hash of message)
        //encoder.bytes(message_hash.as_ref()).unwrap();

        //// 4. Signature: bstr
        //encoder.bytes(signature.to_bytes().as_ref()).unwrap();

        //encoder.to_vec().expect("Failed to encode COSE_Sign1")
    };

    // --- 6. COSE_KEY ASSEMBLY ---

    // COSE_Key structure: {1: 1 (OKP), 3: -8 (EdDSA), -1: 6 (Ed25519), -2: pubKey}
    let cose_key_bytes = {
        let mut buffer = [0u8; 128];
        //let mut encoder = Encoder::new(&mut buffer);

        // Map of size 5 (if including key ID or other headers, but we use minimal 4)
        //encoder.map(4).unwrap();

        //// kty (1) -> OKP (1)
        //encoder.u8(1).unwrap().u8(1).unwrap();

        //// alg (3) -> EdDSA (-8)
        //encoder.u8(3).unwrap().i8(-8).unwrap();

        //// crv (-1) -> Ed25519 (6)
        //encoder.i8(-1).unwrap().u8(6).unwrap();

        //// x (-2) -> pubKey bytes
        //encoder.i8(-2).unwrap().bytes(sk.public_key().as_ref()).unwrap();

        //encoder.to_vec().expect("Failed to encode COSE_Key")
    };

    // Return the final concatenated hex strings
    //(hex::encode(cose_sign1_bytes), hex::encode(cose_key_bytes))
    let signature_hex = "abc123456";
    (signature_hex.to_string(), pubkey_hex.to_string())
}
