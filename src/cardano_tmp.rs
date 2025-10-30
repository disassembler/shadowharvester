// shadowharvester/src/cardano.rs

use pallas_addresses::{Address, Network, ShelleyAddress, ShelleyPaymentPart, ShelleyDelegationPart};
use pallas_crypto::key::ed25519::{SecretKey, PublicKey};
// FIX 1: Import the VerificationKey trait (provides .hash())
// FIX 2: Import the ToBytes trait (provides .to_bytes())
use pallas_crypto::key::VerificationKey;
use pallas_crypto::key::ToBytes;
use pallas_crypto::hash::Hash;
use rand_core::{OsRng, RngCore};
use hex;


/// Generates a Cardano key pair and prints the secret key, public key, and payment address.
///
/// This function generates a random Ed25519 key pair and constructs a Testnet (preprod)
/// Shelley Base Address using the public key hash for both payment and staking parts.
pub fn generate_cardano_key_and_address() {
    let mut rng = OsRng;

    // Generate Ed25519 SecretKey
    let sk = SecretKey::new(&mut rng);

    // Use public_key()
    let vk = sk.public_key();

    // FIX 1 & 2: Use the hash() method. The Hash type is now imported.
    // vk.hash() returns Hash<28>, which satisfies the constraints of ShelleyPaymentPart::Key
    let vk_hash: Hash<28> = vk.hash();

    // Construct the Shelley Base Address parts (Testnet is used here)
    let payment_part = ShelleyPaymentPart::Key(vk_hash);
    let delegation_part = ShelleyDelegationPart::Key(vk_hash);

    let shelley_address = ShelleyAddress::new(
        Network::Testnet,
        payment_part,
        delegation_part,
    );

    let payment_address = Address::Shelley(shelley_address);

    // FIX 3 & 4: Use to_bytes().to_vec() for key byte access to avoid missing ToVec trait error.
    let sk_bytes = sk.to_bytes().to_vec();
    let vk_bytes = vk.to_bytes().to_vec();

    // The server expects a 64-character hex string for the public key
    let pub_key_for_registration = hex::encode(&vk_bytes);

    println!("\n💳 Cardano Key Pair Generated (Testnet)");
    println!("--------------------------------------------------");
    println!("⚠️ SECRET KEY (Keep this safe!):");
    println!("{}", hex::encode(&sk_bytes));
    println!("\n✅ Public Key (64-character hex for registration):");
    println!("{}", pub_key_for_registration);
    println!("\n📍 Payment Address (Bech32 for mining/receiving rewards):");
    println!("{}", payment_address.to_bech32().unwrap());
    println!("--------------------------------------------------");
}
