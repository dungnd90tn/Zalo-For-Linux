//! Optional DTO bridge only. Does not instantiate the embedded engine or enable live E2EE.
use super::{Delta, Mutation, PeerActivation, RecordKey, Snapshot};
use anyhow::{anyhow, Result};
use zalo_signal_engine as engine;

fn key_from_engine(key: &engine::RecordKey) -> Result<RecordKey> {
    Ok(serde_json::from_slice(&serde_json::to_vec(key)?)?)
}
fn key_to_engine(key: &RecordKey) -> Result<engine::RecordKey> {
    let key: engine::RecordKey = serde_json::from_slice(&serde_json::to_vec(key)?)?;
    key.validate()
        .map_err(|_| anyhow!("Invalid engine record address"))?;
    Ok(key)
}
/// Values remain byte-for-byte opaque; only typed addresses cross the DTO bridge.
pub fn into_engine_snapshot(snapshot: Snapshot) -> Result<engine::Snapshot> {
    let account =
        engine::AccountId::new(snapshot.account).map_err(|_| anyhow!("Invalid engine account"))?;
    let records = snapshot
        .records
        .into_iter()
        .map(|record| {
            engine::Record::from_parts(key_to_engine(&record.key)?, record.value)
                .map_err(|_| anyhow!("Invalid stored engine record"))
        })
        .collect::<Result<Vec<_>>>()?;
    engine::Snapshot::new(account, snapshot.revision, records)
        .map_err(|_| anyhow!("Engine snapshot exceeds its resource policy"))
}
/// `allowed_keys` is the operation's planned read/write set, not keys inferred
/// blindly from engine output. Identity activations originate in verified trust.
/// Caller must discard the returned overlay/output on any SQL commit failure.
pub fn into_store_delta(
    result: engine::OperationResult,
    allowed_keys: Vec<RecordKey>,
    input_digest: [u8; 32],
    activate_peers: Vec<PeerActivation>,
) -> Result<(engine::Output, Delta)> {
    let (account, generation, operation_id, expected_revision, output, delta) = result.into_parts();
    let mutations = delta
        .into_mutations()
        .into_iter()
        .map(|mutation| match mutation {
            engine::Mutation::Put(record) => {
                let (key, value) = record.into_parts();
                Ok(Mutation::Put(super::Record {
                    key: key_from_engine(&key)?,
                    value,
                }))
            }
            engine::Mutation::Delete(key) => Ok(Mutation::Delete(key_from_engine(&key)?)),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        output,
        Delta {
            account: account.as_str().into(),
            expected_revision,
            generation,
            operation_id,
            input_digest,
            allowed_keys,
            activate_peers,
            mutations,
        },
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_key_codec_matches_every_engine_namespace_and_preserves_record_bytes() {
        let local = super::super::LocalNamespace {
            epoch: "1".into(),
            platform: super::super::Platform::Pc,
        };
        let peer = super::super::PeerNamespace {
            uid: "456".into(),
            epoch: "2".into(),
            platform: super::super::Platform::Mobile,
        };
        let keys = vec![
            RecordKey::LocalIdentity {
                local: local.clone(),
            },
            RecordKey::PreKey {
                local: local.clone(),
                id: 1,
            },
            RecordKey::SignedPreKey {
                local: local.clone(),
                id: 2,
            },
            RecordKey::Session {
                local: local.clone(),
                peer: peer.clone(),
            },
            RecordKey::PeerIdentity {
                local: local.clone(),
                peer: peer.clone(),
            },
            RecordKey::SessionTimestamp {
                local: local.clone(),
                peer: peer.clone(),
            },
            RecordKey::GroupSender {
                local,
                group: "789".into(),
                sender: peer,
            },
        ];
        for key in keys {
            let encoded = key_to_engine(&key).unwrap();
            assert!(key_from_engine(&encoded).unwrap() == key);
            let bytes = br#" { "version": 1, "opaque": [1,2,3] } "#.to_vec();
            let snapshot = into_engine_snapshot(Snapshot {
                account: "123".into(),
                revision: 9,
                records: vec![super::super::Record {
                    key,
                    value: bytes.clone(),
                }],
            })
            .unwrap();
            assert_eq!(snapshot.revision(), 9);
            assert_eq!(snapshot.account().as_str(), "123");
            assert_eq!(snapshot.records()[0].value(), bytes);
        }
    }
}
