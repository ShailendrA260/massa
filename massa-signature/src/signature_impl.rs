// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::MassaSignatureError;
use massa_hash::Hash;
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use secp256k1::{schnorr, Message, XOnlyPublicKey, SECP256K1};
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
    Deserialize,
};
use std::ops::Bound::Included;
use std::{convert::TryInto, str::FromStr};

/// Size of a public key
pub const PUBLIC_KEY_SIZE_BYTES: usize = 32;
/// Size of a keypair
pub const SECRET_KEY_SIZE_BYTES: usize = 32;
/// Size of a signature
pub const SIGNATURE_SIZE_BYTES: usize = 64;

const SIGNATURE_STRING_PREFIX: &str = "SIG";

/// `KeyPair` is used for signature and decrypting
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct KeyPair(secp256k1::KeyPair);

const SECRET_PREFIX: char = 'S';
const KEYPAIR_VERSION: u64 = 0;

impl std::fmt::Display for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        let mut bytes = Vec::new();
        u64_serializer
            .serialize(&KEYPAIR_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.secret_bytes());
        write!(
            f,
            "{}{}",
            SECRET_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for KeyPair {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == SECRET_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check =
                    bs58::decode(data)
                        .with_check(None)
                        .into_vec()
                        .map_err(|_| {
                            MassaSignatureError::ParsingError("Bad secret key bs58".to_owned())
                        })?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
                KeyPair::from_bytes(&rest.try_into().map_err(|_| {
                    MassaSignatureError::ParsingError("Secret key not long enough".to_string())
                })?)
            }
            _ => Err(MassaSignatureError::ParsingError(
                "Bad secret prefix".to_owned(),
            )),
        }
    }
}

impl KeyPair {
    /// Generate a new `KeyPair`
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// ```
    pub fn generate() -> KeyPair {
        use secp256k1::rand::rngs::OsRng;
        let mut rng = OsRng::new().expect("OsRng");
        KeyPair(secp256k1::KeyPair::new(SECP256K1, &mut rng))
    }

    /// Returns the Signature produced by signing
    /// data bytes with a PrivateKey.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    /// ```
    pub fn sign(&self, hash: &Hash) -> Result<Signature, MassaSignatureError> {
        let message = Message::from_slice(hash.to_bytes())?;
        Ok(Signature(SECP256K1.sign_schnorr(&message, &self.0)))
    }

    /// Return the bytes representing the keypair (should be a reference in the future)
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate();
    /// let bytes = keypair.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> [u8; SECRET_KEY_SIZE_BYTES] {
        self.0.secret_bytes()
    }

    /// Return the bytes representing the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate();
    /// let bytes = keypair.into_bytes();
    /// ```
    pub fn into_bytes(&self) -> [u8; SECRET_KEY_SIZE_BYTES] {
        self.0.secret_bytes()
    }

    /// Convert a byte array of size `SECRET_KEY_SIZE_BYTES` to a `KeyPair`
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate();
    /// let bytes = keypair.into_bytes();
    /// let keypair2 = KeyPair::from_bytes(&bytes).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8; SECRET_KEY_SIZE_BYTES]) -> Result<Self, MassaSignatureError> {
        secp256k1::KeyPair::from_seckey_slice(SECP256K1, &data[..])
            .map(Self)
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!("keypair bytes parsing error: {}", err))
            })
    }

    /// Get the public key of the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate();
    /// let public_key = keypair.get_public_key();
    /// ```
    pub fn get_public_key(&self) -> PublicKey {
        PublicKey(XOnlyPublicKey::from_keypair(&self.0))
    }

    /// Encode a keypair into his base58 form
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate();
    /// let bs58 = keypair.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Decode a base58 encoded keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate();
    /// let bs58 = keypair.to_bs58_check();
    /// let keypair2 = KeyPair::from_bs58_check(&bs58).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Self, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "keypair bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|key| {
                Ok(KeyPair(
                    secp256k1::KeyPair::from_seckey_slice(SECP256K1, key.as_slice()).map_err(
                        |err| {
                            MassaSignatureError::ParsingError(format!(
                                "keypair bs58_check parsing error: {:?}",
                                err
                            ))
                        },
                    )?,
                ))
            })
    }
}

impl ::serde::Serialize for KeyPair {
    /// `::serde::Serialize` trait for `PrivateKey`
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::KeyPair;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = KeyPair::generate();
    /// let serialized: String = serde_json::to_string(&private_key).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut keypair_serializer = s.serialize_struct("keypair", 2)?;
        keypair_serializer.serialize_field("secret_key", &self.to_string())?;
        keypair_serializer.serialize_field("public_key", &self.get_public_key().to_string())?;
        keypair_serializer.end()
    }
}

impl<'de> ::serde::Deserialize<'de> for KeyPair {
    /// `::serde::Deserialize` trait for `PrivateKey`
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{PrivateKey, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let serialized = serde_json::to_string(&private_key).unwrap();
    /// let deserialized: PrivateKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<KeyPair, D::Error> {
        enum Field {
            SecretKey,
            PublicKey,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("`secret_key` or `public_key`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "secret_key" => Ok(Field::SecretKey),
                            "public_key" => Ok(Field::PublicKey),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct KeyPairVisitor;

        impl<'de> Visitor<'de> for KeyPairVisitor {
            type Value = KeyPair;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("{'secret_key': 'xxx', 'public_key': 'xxx'}")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<KeyPair, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let secret = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let _: &str = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                KeyPair::from_str(secret).map_err(serde::de::Error::custom)
            }

            fn visit_map<V>(self, mut map: V) -> Result<KeyPair, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut secret = None;
                let mut public = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::SecretKey => {
                            if secret.is_some() {
                                return Err(serde::de::Error::duplicate_field("secret"));
                            }
                            secret = Some(map.next_value()?);
                        }
                        Field::PublicKey => {
                            if public.is_some() {
                                return Err(serde::de::Error::duplicate_field("public"));
                            }
                            public = Some(map.next_value()?);
                        }
                    }
                }
                let secret = secret.ok_or_else(|| serde::de::Error::missing_field("secret"))?;
                let _: &str = public.ok_or_else(|| serde::de::Error::missing_field("public"))?;
                KeyPair::from_str(secret).map_err(serde::de::Error::custom)
            }
        }

        const FIELDS: &[&str] = &["secret_key", "public_key"];
        d.deserialize_struct("KeyPair", FIELDS, KeyPairVisitor)
    }
}

/// Public key used to check if a message was encoded
/// by the corresponding `PublicKey`.
/// Generated from the `PrivateKey` using `SignatureEngine`
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicKey(secp256k1::XOnlyPublicKey);

const PUBLIC_PREFIX: char = 'P';

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        let mut bytes = Vec::new();
        u64_serializer
            .serialize(&KEYPAIR_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.serialize());
        write!(
            f,
            "{}{}",
            PUBLIC_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for PublicKey {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == PUBLIC_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check =
                    bs58::decode(data)
                        .with_check(None)
                        .into_vec()
                        .map_err(|_| {
                            MassaSignatureError::ParsingError("Bad public key bs58".to_owned())
                        })?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
                PublicKey::from_bytes(&rest.try_into().map_err(|_| {
                    MassaSignatureError::ParsingError("Public key not long enough".to_string())
                })?)
            }
            _ => Err(MassaSignatureError::ParsingError(
                "Bad public key prefix".to_owned(),
            )),
        }
    }
}

impl PublicKey {
    /// Checks if the `Signature` associated with data bytes
    /// was produced with the `PrivateKey` associated to given `PublicKey`
    pub fn verify_signature(
        &self,
        hash: &Hash,
        signature: &Signature,
    ) -> Result<(), MassaSignatureError> {
        let message = Message::from_slice(hash.to_bytes())?;
        Ok(SECP256K1.verify_schnorr(&signature.0, &message, &self.0)?)
    }

    /// Serialize a `PublicKey` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized: String = public_key.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a `PublicKey` as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialize = public_key.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_SIZE_BYTES] {
        self.0.serialize()
    }

    /// Serialize into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialize = public_key.to_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; PUBLIC_KEY_SIZE_BYTES] {
        self.0.serialize()
    }

    /// Deserialize a `PublicKey` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized: String = public_key.to_bs58_check();
    /// let deserialized: PublicKey = PublicKey::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<PublicKey, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "public key bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|key| {
                PublicKey::from_bytes(&key.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "public key bs58_check parsing error: {:?}",
                        err
                    ))
                })?)
            })
    }

    /// Deserialize a `PublicKey` from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized = public_key.into_bytes();
    /// let deserialized: PublicKey = PublicKey::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; PUBLIC_KEY_SIZE_BYTES],
    ) -> Result<PublicKey, MassaSignatureError> {
        secp256k1::XOnlyPublicKey::from_slice(&data[..])
            .map(PublicKey)
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "public key bytes parsing error: {}",
                    err
                ))
            })
    }
}

/// Serializer for `Signature`
#[derive(Default)]
pub struct PublicKeyDeserializer;

impl PublicKeyDeserializer {
    /// Creates a `SignatureDeserializer`
    pub fn new() -> Self {
        Self
    }
}

impl Deserializer<PublicKey> for PublicKeyDeserializer {
    /// ```
    /// use massa_signature::{PublicKey, PublicKeyDeserializer, derive_public_key, generate_random_private_key, sign};
    /// use massa_serialization::{DeserializeError, Deserializer};
    /// use massa_hash::Hash;
    ///
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    /// let serialized = public_key.to_bytes();
    /// let (rest, deser_public_key) = PublicKeyDeserializer::new().deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(public_key, deser_public_key);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PublicKey, E> {
        // Can't use try into directly because it fails if there is more data in the buffer
        if buffer.len() < PUBLIC_KEY_SIZE_BYTES {
            return Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::LengthValue,
            )));
        }
        let key =
            PublicKey::from_bytes(buffer[..PUBLIC_KEY_SIZE_BYTES].try_into().map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::LengthValue,
                ))
            })?)
            .map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Fail,
                ))
            })?;
        // Safe because the signature deserialization success
        Ok((&buffer[PUBLIC_KEY_SIZE_BYTES..], key))
    }
}

impl ::serde::Serialize for PublicKey {
    /// `::serde::Serialize` trait for `PublicKey`
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::{derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized: String = serde_json::to_string(&public_key).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(&self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for PublicKey {
    /// `::serde::Deserialize` trait for `PublicKey`
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{PublicKey, derive_public_key, generate_random_private_key};
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let public_key = derive_public_key(&private_key);
    ///
    /// let serialized = serde_json::to_string(&public_key).unwrap();
    /// let deserialized: PublicKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PublicKey, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = PublicKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        PublicKey::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PublicKey::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = PublicKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PublicKey::from_bytes(v.try_into().map_err(E::custom)?).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Signature generated from a message and a `PrivateKey`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Signature(schnorr::Signature);

impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", SIGNATURE_STRING_PREFIX, self.to_bs58_check())
        } else {
            write!(f, "{}", self.to_bs58_check())
        }
    }
}

impl FromStr for Signature {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Signature::from_bs58_check(s)
            } else if v[0] != SIGNATURE_STRING_PREFIX {
                Err(MassaSignatureError::WrongPrefix(
                    SIGNATURE_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Signature::from_bs58_check(v[1])
            }
        } else {
            Signature::from_bs58_check(s)
        }
    }
}

impl Signature {
    /// Serialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a Signature as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = signature.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> &[u8; SIGNATURE_SIZE_BYTES] {
        self.0.as_ref()
    }

    /// Serialize a Signature into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = signature.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; SIGNATURE_SIZE_BYTES] {
        *self.0.as_ref()
    }

    /// Deserialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// let deserialized: Signature = Signature::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Signature, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "signature bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|signature| {
                Signature::from_bytes(&signature.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "signature bs58_check parsing error: {:?}",
                        err
                    ))
                })?)
            })
    }

    /// Deserialize a Signature from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{generate_random_private_key, sign, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = signature.to_bytes();
    /// let deserialized: Signature = Signature::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8; SIGNATURE_SIZE_BYTES]) -> Result<Signature, MassaSignatureError> {
        schnorr::Signature::from_slice(&data[..])
            .map(Signature)
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!("signature bytes parsing error: {}", err))
            })
    }
}

impl ::serde::Serialize for Signature {
    /// `::serde::Serialize` trait for `Signature`
    /// if the serializer is human readable,
    /// serialization is done using `to_bs58_check`
    /// else, it uses `to_bytes`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::{generate_random_private_key, sign};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized: String = serde_json::to_string(&signature).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Signature {
    /// `::serde::Deserialize` trait for `Signature`
    /// if the deserializer is human readable,
    /// deserialization is done using `from_bs58_check`
    /// else, it uses `from_bytes`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{generate_random_private_key, sign, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    ///
    /// let serialized = serde_json::to_string(&signature).unwrap();
    /// let deserialized: Signature = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Signature, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Signature::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::from_bytes(v.try_into().map_err(E::custom)?).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Serializer for `Signature`
#[derive(Default)]
pub struct SignatureDeserializer;

impl SignatureDeserializer {
    /// Creates a `SignatureDeserializer`
    pub fn new() -> Self {
        Self
    }
}

impl Deserializer<Signature> for SignatureDeserializer {
    /// ```
    /// use massa_signature::{Signature, SignatureDeserializer, generate_random_private_key, sign};
    /// use massa_serialization::{DeserializeError, Deserializer};
    /// use massa_hash::Hash;
    ///
    /// let private_key = generate_random_private_key();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = sign(&data, &private_key).unwrap();
    /// let serialized = signature.into_bytes();
    /// let (rest, deser_signature) = SignatureDeserializer::new().deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(signature, deser_signature);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Signature, E> {
        // Can't use try into directly because it fails if there is more data in the buffer
        if buffer.len() < SIGNATURE_SIZE_BYTES {
            return Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::LengthValue,
            )));
        }
        let signature = Signature::from_bytes(buffer[..SIGNATURE_SIZE_BYTES].try_into().unwrap())
            .map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            ))
        })?;
        // Safe because the signature deserialization success
        Ok((&buffer[SIGNATURE_SIZE_BYTES..], signature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_example() {
        let keypair = KeyPair::generate();
        let message = "Hello World!".as_bytes();
        let hash = Hash::compute_from(message);
        let signature = keypair.sign(&hash).unwrap();
        assert!(keypair
            .get_public_key()
            .verify_signature(&hash, &signature)
            .is_ok())
    }

    #[test]
    #[serial]
    fn test_serde_keypair() {
        let keypair = KeyPair::generate();
        let serialized = serde_json::to_string(&keypair).expect("could not serialize keypair");
        println!("{}", serialized);
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize keypair");
        assert_eq!(keypair, deserialized);
    }

    #[test]
    #[serial]
    fn test_serde_public_key() {
        let keypair = KeyPair::generate();
        let public_key = keypair.get_public_key();
        let serialized =
            serde_json::to_string(&public_key).expect("Could not serialize public key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize public key");
        assert_eq!(public_key, deserialized);
    }

    #[test]
    #[serial]
    fn test_serde_signature() {
        let keypair = KeyPair::generate();
        let message = "Hello World!".as_bytes();
        let hash = Hash::compute_from(message);
        let signature = keypair.sign(&hash).unwrap();
        let serialized =
            serde_json::to_string(&signature).expect("could not serialize signature key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize signature key");
        assert_eq!(signature, deserialized);
    }
}
