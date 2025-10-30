// shadowharvester/src/cardano.rs

use pallas::{
    crypto::key::ed25519::{SecretKey,PublicKey},
    ledger::{
        addresses::{Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart},
        traverse::ComputeHash,
    },
};

use rand_core::{OsRng};

type KeyPairAndAddress = (SecretKey, PublicKey, String, String);

pub fn generate_cardano_key_and_address() -> KeyPairAndAddress {
    let rng = OsRng;

    // Generate Ed25519 SecretKey
    let pay_sk = SecretKey::new(rng);
    let pay_vk = pay_sk.public_key();

    let pay_addr = ShelleyAddress::new(
        Network::Mainnet,
        ShelleyPaymentPart::key_hash(pay_vk.compute_hash()),
        ShelleyDelegationPart::Null
    );

    (pay_sk, pay_vk, pay_addr.to_bech32().unwrap(), hex::encode(pay_vk.as_ref()))


}
