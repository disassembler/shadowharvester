// shadowharvester/src/cardano.rs

use pallas::{
    crypto::key::ed25519::{SecretKey,PublicKey,SecretKeyExtended,Signature},
    ledger::{
        addresses::{Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart},
        traverse::ComputeHash,
    },
};
use cryptoxide::{hmac::Hmac, pbkdf2::pbkdf2, sha2::Sha512};
use minicbor::*;

use rand_core::{OsRng};
use bip39::Mnemonic;
use ed25519_bip32::{self, XPrv, XPRV_SIZE};

pub enum FlexibleSecretKey {
    Standard(SecretKey),
    Extended(SecretKeyExtended),
}

// 2. Define the new type alias using the enum
pub type KeyPairAndAddress = (FlexibleSecretKey, PublicKey, ShelleyAddress);

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

    let sk_flex: FlexibleSecretKey = FlexibleSecretKey::Standard(sk);
    (sk_flex, vk, addr)
}

pub fn harden_index(index: u32) -> u32 {
    // The constant 0x80000000 is 2^31, which sets the most significant bit.
    index | 0x80000000
}

pub fn derive_key_pair_from_mnemonic(mnemonic: &str, account: u32, index: u32) -> KeyPairAndAddress {
    // NOTE: This is a simplified, non-compliant derivation for demonstration purposes.
    // A real Cardano application MUST use BIP39/BIP44-compliant HD derivation.
    let bip39 = Mnemonic::parse(mnemonic).expect("Need a valid mnemonic");
    let entropy = bip39.clone().to_entropy();
    let mut pbkdf2_result = [0; XPRV_SIZE];
    const ITER: u32 = 4096;
    let mut mac = Hmac::new(Sha512::new(), "".as_bytes());
    pbkdf2(&mut mac, &entropy, ITER, &mut pbkdf2_result);
    let xprv = XPrv::normalize_bytes_force3rd(pbkdf2_result);

    // payment key 1852'/1815'/<account>'/0/<index>
    let pay_xprv = &xprv
        .derive(ed25519_bip32::DerivationScheme::V2, harden_index(1852))
        .derive(ed25519_bip32::DerivationScheme::V2, harden_index(1815))
        .derive(ed25519_bip32::DerivationScheme::V2, harden_index(account))
        .derive(ed25519_bip32::DerivationScheme::V2, 0)
        .derive(ed25519_bip32::DerivationScheme::V2, index)
        .extended_secret_key();
    unsafe {
        let sk = SecretKeyExtended::from_bytes_unchecked(*pay_xprv);
        let vk = sk.public_key();

        // Cardano (Shelley) address derivation
        let addr = ShelleyAddress::new(
            Network::Mainnet, // Assuming Mainnet environment
            ShelleyPaymentPart::key_hash(vk.compute_hash()),
            ShelleyDelegationPart::Null
        );
        let sk_flex: FlexibleSecretKey = FlexibleSecretKey::Extended(sk);

        (sk_flex, vk, addr)
    }

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

    let sk_flex: FlexibleSecretKey = FlexibleSecretKey::Standard(sk);
    (sk_flex, vk, addr)
}

#[derive(Debug)]
pub struct CoseProtHeader {
    address: Vec<u8>,
}

impl<C> Encode<C> for CoseProtHeader
where
    C: Default,
{
    fn encode<W: minicbor::encode::Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.map(2)?;

        e.i64(1)?;
        e.i64(-8)?;

        e.str("address")?;
        e.bytes(&self.address)?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct CoseSignData<'a> {
    pub label: &'a str, // Use a slice for zero-copy encoding
    pub protected_header: &'a [u8],
    pub external_aad: &'a [u8],
    pub payload: &'a [u8],
}

impl<C> Encode<C> for CoseSignData<'_>
where
    C: Default,
{
    fn encode<W: minicbor::encode::Write>(&self, e: &mut minicbor::Encoder<W>, _ctx: &mut C) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(4)?;
        e.str(self.label)?;
        e.bytes(self.protected_header)?;
        e.bytes(self.external_aad)?;
        e.bytes(self.payload)?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct CoseSign1<'a> {
    pub protected_header: &'a [u8],
    pub payload: &'a [u8],
    pub signature: &'a [u8],
}

impl<C> Encode<C> for CoseSign1<'_>
where
    C: Default,
{
    fn encode<W: encode::Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), encode::Error<W::Error>> {
        e.array(4)?;
        e.bytes(self.protected_header)?;
        e.map(1)?;

        e.str("hashed")?;
        e.bool(false)?;

        e.bytes(self.payload)?;
        e.bytes(self.signature)?;

        Ok(())
    }
}



/// Creates a placeholder hex string simulating a CIP-8 signed message payload.
/// NOTE: The actual CIP-8 structure (CBOR headers/map) is not dynamically built here,
/// but the signature and public key components are guaranteed to be unique.
pub fn cip8_sign(kp: &KeyPairAndAddress, message: &str) -> (String, String) {

    let pubkey = hex::encode(kp.1.as_ref());
    let prot_header = CoseProtHeader {
        address: kp.2.to_vec(),
    };
    let cose_prot_cbor = pallas::codec::minicbor::to_vec(&prot_header).unwrap();
    let to_sign = CoseSignData {
        label: "Signature1",
        protected_header: &cose_prot_cbor,
        external_aad: b"" ,
        payload: message.as_bytes(),
    };
    let to_sign_cbor = pallas::codec::minicbor::to_vec(&to_sign).unwrap();
    // The specific error type the compiler couldn't figure out. Let's use &'static str
    type SignResult = Result<Signature, &'static str>;

    // Assume kp.0 is &FlexibleSecretKey
    let signature_result: SignResult = match &kp.0 {
        FlexibleSecretKey::Standard(sk) => {
            // Now the compiler knows the full result type is Result<Signature, &'static str>
            Ok(sk.sign(&to_sign_cbor))
        }
        FlexibleSecretKey::Extended(ske) => {
            // All match arms must return SignResult
            Ok(ske.sign(&to_sign_cbor))
        }
    };

    let sig = signature_result.expect("Failed to sign the message.");

    let cose_struct = CoseSign1 {
        protected_header: &cose_prot_cbor,
        payload: message.as_bytes(),
        signature: sig.as_ref(),
    };
    let cose_sign1_cbor = pallas::codec::minicbor::to_vec(&cose_struct).unwrap();

    (hex::encode(&cose_sign1_cbor).to_string(), hex::encode(pubkey).to_string())
}
