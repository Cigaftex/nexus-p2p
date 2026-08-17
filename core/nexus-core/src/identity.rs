use std::fmt::{Display, Formatter};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::storage::Store;

#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct DeviceId(pub String);

impl Display for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicIdentity {
    pub device_id: DeviceId,
    pub display_name: String,
    pub signing_public_key: [u8; 32],
    pub exchange_public_key: [u8; 32],
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct SecretIdentity {
    signing_secret: [u8; 32],
    exchange_secret: [u8; 32],
}

pub struct Identity {
    public: PublicIdentity,
    signing: SigningKey,
    exchange: StaticSecret,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public", &self.public)
            .finish()
    }
}

impl Identity {
    pub fn load_or_create(store: &Store, display_name: &str) -> anyhow::Result<Self> {
        if let Some((public_json, secret_json)) = store.load_identity()? {
            let public: PublicIdentity = serde_json::from_str(&public_json)?;
            let encoded: String = serde_json::from_str(&secret_json)?;
            let bytes = STANDARD_NO_PAD.decode(encoded)?;
            let secret: SecretIdentity = bincode::deserialize(&bytes)?;
            return Ok(Self {
                public,
                signing: SigningKey::from_bytes(&secret.signing_secret),
                exchange: StaticSecret::from(secret.exchange_secret),
            });
        }

        let signing = SigningKey::generate(&mut OsRng);
        let exchange = StaticSecret::random_from_rng(OsRng);
        let signing_public_key = signing.verifying_key().to_bytes();
        let exchange_public_key = X25519PublicKey::from(&exchange).to_bytes();
        let id_hash = blake3::hash(&signing_public_key);
        let public = PublicIdentity {
            device_id: DeviceId(hex::encode(&id_hash.as_bytes()[..16])),
            display_name: display_name.to_owned(),
            signing_public_key,
            exchange_public_key,
        };
        let secret = SecretIdentity {
            signing_secret: signing.to_bytes(),
            exchange_secret: exchange.to_bytes(),
        };
        let secret_json =
            serde_json::to_string(&STANDARD_NO_PAD.encode(bincode::serialize(&secret)?))?;
        store.save_identity(&serde_json::to_string(&public)?, &secret_json)?;
        Ok(Self {
            public,
            signing,
            exchange,
        })
    }

    pub fn public(&self) -> &PublicIdentity {
        &self.public
    }

    pub fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        self.signing.sign(bytes).to_bytes().to_vec()
    }

    pub fn shared_secret(&self, peer_exchange_key: &[u8; 32]) -> [u8; 32] {
        self.exchange
            .diffie_hellman(&X25519PublicKey::from(*peer_exchange_key))
            .to_bytes()
    }
}

pub fn verify(public: &PublicIdentity, message: &[u8], signature: &[u8]) -> anyhow::Result<()> {
    use ed25519_dalek::{Signature, Verifier};
    let key = VerifyingKey::from_bytes(&public.signing_public_key)?;
    let signature = Signature::from_slice(signature)?;
    key.verify(message, &signature)?;
    Ok(())
}

pub fn validate_public_identity(public: &PublicIdentity) -> anyhow::Result<()> {
    VerifyingKey::from_bytes(&public.signing_public_key)?;
    let hash = blake3::hash(&public.signing_public_key);
    anyhow::ensure!(
        public.device_id.0 == hex::encode(&hash.as_bytes()[..16]),
        "device id does not match signing key"
    );
    anyhow::ensure!(!public.display_name.trim().is_empty(), "empty display name");
    Ok(())
}
