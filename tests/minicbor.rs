// In tests/minicbor.rs
#[cfg(test)]
mod tests {
    use pallas_codec::minicbor::*;
    use pallas::codec::minicbor::encode::{Error as EncodeError, write::EndOfSlice};

    const EXPECTED_CBOR_HEX: &str = "a30065416c69636501182002f5";

    #[derive(Clone, Encode, Decode, Debug, PartialEq)]
    #[cbor(map)]
    pub struct UserProfile {
        #[n(0)] // Index for the "name" field
        pub name: String,
        #[n(1)] // Index for the "age" field
        pub age: u8,
        #[n(2)] // Index for the "is_active" field
        pub is_active: bool,
    }
    #[test]
    fn test_encode_simple_string() -> Result<(), EncodeError<EndOfSlice>> {

        let user_profile = UserProfile {
            name: "Alice".to_string(),
            age: 32,
            is_active: true,
        };

        let cbor = pallas::codec::minicbor::to_vec(user_profile).unwrap();
        println!("{:?}", hex::encode(&cbor));

        // Assertions
        assert_eq!(
            hex::encode(&cbor),
            EXPECTED_CBOR_HEX,
            "The encoded CBOR bytes do not match the expected value."
        );
        assert_eq!(cbor.len(), 13, "The resulting slice should be 13 bytes long.");
        Ok(())
    }
}
