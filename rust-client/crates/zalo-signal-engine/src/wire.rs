use crate::*;
use serde_json::{json, Value};

const MAX_SAFE_INTEGER: u64 = (1u64 << 53) - 1;
fn bad() -> EngineError {
    EngineError::InvalidInput
}
fn public_key(value: &[u8; 33]) -> Result<(), EngineError> {
    if value[0] != 5 {
        Err(bad())
    } else {
        Ok(())
    }
}

fn trust_value(trust: &TrustDecision) -> Result<Value, EngineError> {
    public_key(&trust.identity_key)?;
    if trust.registration_id > 16383 || trust.session_timestamp > MAX_SAFE_INTEGER {
        return Err(bad());
    }
    Ok(
        json!({"peer":trust.peer,"registrationId":trust.registration_id,"identityKey":trust.identity_key.as_slice(),"sessionTimestamp":trust.session_timestamp,"basis":trust.basis}),
    )
}
fn message_value(message: &PersonalCiphertext) -> Result<Value, EngineError> {
    if message.body.len() > MAX_CIPHERTEXT_BYTES
        || message.body.len() < 10
        || message.body[0] != 0x33
    {
        return Err(EngineError::InvalidCiphertext);
    }
    Ok(json!({"messageType":message.message_type,"body":message.body}))
}
fn text(value: &[u8]) -> Result<(), EngineError> {
    if value.len() > MAX_TEXT_BYTES {
        Err(EngineError::LimitExceeded)
    } else {
        Ok(())
    }
}

pub(crate) fn local(operation: &Operation) -> &LocalNamespace {
    match operation {
        Operation::CreateBundle { local, .. }
        | Operation::GeneratePreKeys { local, .. }
        | Operation::ProcessBundle { local, .. }
        | Operation::EncryptPersonal { local, .. }
        | Operation::DecryptPersonal { local, .. }
        | Operation::CreateGroupDistribution { local, .. }
        | Operation::ProcessGroupDistribution { local, .. }
        | Operation::EncryptGroup { local, .. }
        | Operation::DecryptGroup { local, .. } => local,
    }
}
pub(crate) fn allowed(operation: &Operation, key: &RecordKey, deleting: bool) -> bool {
    let current = local(operation);
    match (operation, key) {
        (
            Operation::CreateBundle { .. },
            RecordKey::LocalIdentity { local }
            | RecordKey::PreKey { local, .. }
            | RecordKey::SignedPreKey { local, .. },
        ) => local == current && !deleting,
        (Operation::GeneratePreKeys { reserved_ids, .. }, RecordKey::PreKey { local, id }) => {
            local == current && reserved_ids.contains(id) && !deleting
        }
        (
            Operation::ProcessBundle { trust, .. }
            | Operation::EncryptPersonal { trust, .. }
            | Operation::DecryptPersonal { trust, .. }
            | Operation::ProcessGroupDistribution { trust, .. },
            RecordKey::Session { local, peer }
            | RecordKey::PeerIdentity { local, peer }
            | RecordKey::SessionTimestamp { local, peer },
        ) => local == current && peer == &trust.peer && !deleting,
        (
            Operation::DecryptPersonal { .. } | Operation::ProcessGroupDistribution { .. },
            RecordKey::PreKey { local, .. },
        ) => local == current && deleting,
        (
            Operation::CreateGroupDistribution {
                group,
                sender_epoch,
                ..
            }
            | Operation::EncryptGroup {
                group,
                sender_epoch,
                ..
            },
            RecordKey::GroupSender {
                local,
                group: stored,
                sender,
            },
        ) => {
            local == current
                && stored == group
                && sender.epoch == *sender_epoch
                && sender.platform == current.platform
                && !deleting
        }
        (
            Operation::ProcessGroupDistribution { group, trust, .. }
            | Operation::DecryptGroup { group, trust, .. },
            RecordKey::GroupSender {
                local,
                group: stored,
                sender,
            },
        ) => local == current && stored == group && sender == &trust.peer && !deleting,
        _ => false,
    }
}

pub(crate) fn encode(request: &Request) -> Result<String, EngineError> {
    if request.snapshot.account != request.account {
        return Err(EngineError::WrongAccount);
    }
    if request.snapshot.revision != request.expected_revision {
        return Err(EngineError::StaleRevision);
    }
    if request.generation == 0 || request.operation_id == 0 {
        return Err(bad());
    }
    let local = local(&request.operation);
    let mut value = json!({"account":request.account,"generation":request.generation.to_string(),"operationId":request.operation_id.to_string(),"expectedRevision":request.expected_revision.to_string(),"local":local});
    let object = value.as_object_mut().unwrap();
    match &request.operation {
        Operation::CreateBundle { prekey_count, .. } => {
            if *prekey_count == 0 || *prekey_count > 100 {
                return Err(bad());
            }
            object.insert("operation".into(), json!("create_bundle"));
            object.insert("prekeyCount".into(), json!(prekey_count));
        }
        Operation::GeneratePreKeys { reserved_ids, .. } => {
            let unique: std::collections::BTreeSet<_> = reserved_ids.iter().collect();
            if reserved_ids.is_empty()
                || reserved_ids.len() > 100
                || unique.len() != reserved_ids.len()
            {
                return Err(bad());
            }
            object.insert("operation".into(), json!("generate_prekeys"));
            object.insert("reservedIds".into(), json!(reserved_ids));
        }
        Operation::ProcessBundle { trust, bundle, .. } => {
            public_key(&bundle.identity_key)?;
            public_key(&bundle.signed_prekey)?;
            if bundle.registration_id > 16383
                || bundle.registration_id != trust.registration_id
                || bundle.identity_key != trust.identity_key
            {
                return Err(EngineError::TrustRejected);
            }
            if let Some((_, key)) = &bundle.prekey {
                public_key(key)?;
            }
            object.insert("operation".into(), json!("process_bundle"));
            object.insert("trust".into(), trust_value(trust)?);
            object.insert("bundle".into(),json!({"registrationId":bundle.registration_id,"identityKey":bundle.identity_key.as_slice(),"signedPrekeyId":bundle.signed_prekey_id,"signedPrekey":bundle.signed_prekey.as_slice(),"signature":bundle.signature.as_slice(),"prekey":bundle.prekey.as_ref().map(|(id,key)|json!([id,key.as_slice()]))}));
        }
        Operation::EncryptPersonal {
            trust, plaintext, ..
        } => {
            text(plaintext)?;
            object.insert("operation".into(), json!("encrypt_personal"));
            object.insert("trust".into(), trust_value(trust)?);
            object.insert("plaintext".into(), json!(plaintext));
        }
        Operation::DecryptPersonal {
            trust,
            message,
            message_timestamp,
            ..
        }
        | Operation::ProcessGroupDistribution {
            trust,
            message,
            message_timestamp,
            ..
        } => {
            if *message_timestamp > MAX_SAFE_INTEGER {
                return Err(bad());
            }
            object.insert(
                "operation".into(),
                json!(
                    if matches!(request.operation, Operation::DecryptPersonal { .. }) {
                        "decrypt_personal"
                    } else {
                        "process_group_distribution"
                    }
                ),
            );
            object.insert("trust".into(), trust_value(trust)?);
            object.insert("message".into(), message_value(message)?);
            object.insert("messageTimestamp".into(), json!(message_timestamp));
            if let Operation::ProcessGroupDistribution { group, .. } = &request.operation {
                object.insert("group".into(), json!(group));
            }
        }
        Operation::CreateGroupDistribution {
            group,
            sender_epoch,
            ..
        }
        | Operation::EncryptGroup {
            group,
            sender_epoch,
            ..
        } => {
            if sender_epoch != &local.epoch {
                return Err(bad());
            }
            object.insert(
                "operation".into(),
                json!(
                    if matches!(request.operation, Operation::CreateGroupDistribution { .. }) {
                        "create_group_distribution"
                    } else {
                        "encrypt_group"
                    }
                ),
            );
            object.insert("group".into(), json!(group));
            object.insert(
                "sender".into(),
                json!(PeerNamespace {
                    uid: request.account.clone(),
                    platform: local.platform,
                    epoch: sender_epoch.clone()
                }),
            );
            if let Operation::EncryptGroup { plaintext, .. } = &request.operation {
                text(plaintext)?;
                object.insert("plaintext".into(), json!(plaintext));
            }
        }
        Operation::DecryptGroup {
            group,
            trust,
            ciphertext,
            ..
        } => {
            if ciphertext.len() > MAX_CIPHERTEXT_BYTES
                || ciphertext.len() <= 65
                || ciphertext[0] != 0x33
            {
                return Err(EngineError::InvalidCiphertext);
            }
            object.insert("operation".into(), json!("decrypt_group"));
            object.insert("group".into(), json!(group));
            object.insert("trust".into(), trust_value(trust)?);
            object.insert("ciphertext".into(), json!(ciphertext));
        }
    }
    let mut records = Vec::with_capacity(request.snapshot.records.len());
    for record in &request.snapshot.records {
        record.key().validate()?;
        let record_local = match record.key() {
            RecordKey::LocalIdentity { local }
            | RecordKey::PreKey { local, .. }
            | RecordKey::SignedPreKey { local, .. }
            | RecordKey::Session { local, .. }
            | RecordKey::PeerIdentity { local, .. }
            | RecordKey::SessionTimestamp { local, .. }
            | RecordKey::GroupSender { local, .. } => local,
        };
        if record_local != local {
            return Err(EngineError::InvalidRecord);
        }
        records.push(json!({"key":record.key(),"value":serde_json::from_slice::<Value>(record.value()).map_err(|_|EngineError::InvalidRecord)?}));
    }
    object.insert("records".into(), json!(records));
    let encoded = serde_json::to_string(&value).map_err(|_| bad())?;
    if encoded.len() > MAX_SNAPSHOT_BYTES {
        return Err(EngineError::LimitExceeded);
    }
    Ok(encoded)
}

fn bytes(value: &Value, max: usize) -> Result<Vec<u8>, EngineError> {
    let values = value.as_array().ok_or(EngineError::RuntimeFailure)?;
    if values.len() > max {
        return Err(EngineError::LimitExceeded);
    }
    values
        .iter()
        .map(|value| {
            value
                .as_u64()
                .filter(|v| *v <= 255)
                .map(|v| v as u8)
                .ok_or(EngineError::RuntimeFailure)
        })
        .collect()
}
fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], EngineError> {
    bytes(value, N)?
        .try_into()
        .map_err(|_| EngineError::RuntimeFailure)
}
fn number(value: &Value) -> Result<u32, EngineError> {
    value
        .as_u64()
        .and_then(|n| n.try_into().ok())
        .ok_or(EngineError::RuntimeFailure)
}

pub(crate) fn decode(request: &Request, encoded: &str) -> Result<OperationResult, EngineError> {
    if encoded.len() > MAX_SNAPSHOT_BYTES {
        return Err(EngineError::LimitExceeded);
    }
    let value: Value = serde_json::from_str(encoded).map_err(|_| EngineError::RuntimeFailure)?;
    if let Some(code) = value.get("error").and_then(Value::as_str) {
        return Err(match code {
            "invalid_input" => EngineError::InvalidInput,
            "invalid_record" => EngineError::InvalidRecord,
            "limit_exceeded" => EngineError::LimitExceeded,
            "trust_rejected" => EngineError::TrustRejected,
            "identity_changed" => EngineError::IdentityChanged,
            "missing_record" => EngineError::MissingRecord,
            "already_provisioned" => EngineError::AlreadyProvisioned,
            "invalid_ciphertext" => EngineError::InvalidCiphertext,
            "replay" => EngineError::Replay,
            "stale_operation" => EngineError::StaleOperation,
            _ => EngineError::RuntimeFailure,
        });
    }
    if value["account"].as_str() != Some(request.account.as_str()) {
        return Err(EngineError::WrongAccount);
    }
    if value["generation"].as_str() != Some(request.generation.to_string().as_str()) {
        return Err(EngineError::StaleGeneration);
    }
    if value["operationId"].as_str() != Some(request.operation_id.to_string().as_str()) {
        return Err(EngineError::StaleOperation);
    }
    if value["expectedRevision"].as_str() != Some(request.expected_revision.to_string().as_str()) {
        return Err(EngineError::StaleRevision);
    }
    let output = &value["output"];
    let kind = output["kind"].as_str().ok_or(EngineError::RuntimeFailure)?;
    let parsed = match (&request.operation, kind) {
        (Operation::CreateBundle { .. }, "bundle") => {
            let registration = number(&output["registrationId"])?;
            if registration > 16383 {
                return Err(EngineError::RuntimeFailure);
            }
            let identity_key = fixed(&output["identityKey"])?;
            let signed_prekey = fixed(&output["signedPrekey"])?;
            public_key(&identity_key)?;
            public_key(&signed_prekey)?;
            let mut prekeys = Vec::new();
            for item in output["prekeys"]
                .as_array()
                .ok_or(EngineError::RuntimeFailure)?
            {
                let public = fixed(&item[1])?;
                public_key(&public)?;
                prekeys.push((number(&item[0])?, public));
            }
            Output::Bundle(PublicBundle {
                registration_id: registration as u16,
                identity_key,
                signed_prekey_id: number(&output["signedPrekeyId"])?,
                signed_prekey,
                signature: fixed(&output["signature"])?,
                prekeys,
            })
        }
        (Operation::ProcessBundle { .. }, "session_ready") => Output::SessionReady,
        (Operation::GeneratePreKeys { reserved_ids, .. }, "prekeys") => {
            let registration = number(&output["registrationId"])?;
            if registration > 16383 {
                return Err(EngineError::RuntimeFailure);
            }
            let items = output["prekeys"]
                .as_array()
                .ok_or(EngineError::RuntimeFailure)?;
            if items.len() != reserved_ids.len() {
                return Err(EngineError::RuntimeFailure);
            }
            let mut prekeys = Vec::with_capacity(items.len());
            for (item, expected) in items.iter().zip(reserved_ids) {
                let id = number(&item[0])?;
                let key = fixed(&item[1])?;
                public_key(&key)?;
                if id != *expected {
                    return Err(EngineError::RuntimeFailure);
                }
                prekeys.push((id, key));
            }
            Output::PreKeys {
                registration_id: registration as u16,
                prekeys,
            }
        }
        (Operation::EncryptPersonal { .. }, "personal_encrypted") => {
            let message_type = serde_json::from_value(output["messageType"].clone())
                .map_err(|_| EngineError::RuntimeFailure)?;
            let body = bytes(&output["body"], MAX_CIPHERTEXT_BYTES)?;
            let registration = number(&output["remoteRegistrationId"])?;
            if registration > 16383 {
                return Err(EngineError::RuntimeFailure);
            }
            Output::PersonalEncrypted {
                message: PersonalCiphertext { message_type, body },
                remote_registration_id: registration as u16,
            }
        }
        (Operation::DecryptPersonal { .. } | Operation::DecryptGroup { .. }, "plaintext") => {
            Output::Plaintext(bytes(&output["body"], MAX_TEXT_BYTES)?)
        }
        (Operation::CreateGroupDistribution { .. }, "group_distribution") => {
            Output::GroupDistribution(bytes(&output["body"], MAX_CIPHERTEXT_BYTES)?)
        }
        (Operation::ProcessGroupDistribution { .. }, "group_ready") => Output::GroupReady,
        (Operation::EncryptGroup { .. }, "group_encrypted") => {
            Output::GroupEncrypted(bytes(&output["body"], MAX_CIPHERTEXT_BYTES)?)
        }
        _ => return Err(EngineError::RuntimeFailure),
    };
    let items = value["delta"]
        .as_array()
        .ok_or(EngineError::RuntimeFailure)?;
    if items.len() > MAX_RECORDS {
        return Err(EngineError::LimitExceeded);
    }
    let mut mutations = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    let mut size = 0usize;
    for item in items {
        let mutation = if let Some(put) = item.get("put") {
            let key: RecordKey = serde_json::from_value(put["key"].clone())
                .map_err(|_| EngineError::RuntimeFailure)?;
            if !allowed(&request.operation, &key, false) {
                return Err(EngineError::StorageBoundary);
            }
            if let (
                RecordKey::GroupSender { sender, .. },
                Operation::EncryptGroup { .. } | Operation::CreateGroupDistribution { .. },
            ) = (&key, &request.operation)
            {
                if sender.uid != request.account {
                    return Err(EngineError::StorageBoundary);
                }
            }
            let bytes =
                serde_json::to_vec(&put["value"]).map_err(|_| EngineError::RuntimeFailure)?;
            size += bytes.len();
            Mutation::Put(Record::from_parts(key, bytes)?)
        } else if let Some(delete) = item.get("delete") {
            let key: RecordKey =
                serde_json::from_value(delete.clone()).map_err(|_| EngineError::RuntimeFailure)?;
            if !allowed(&request.operation, &key, true) {
                return Err(EngineError::StorageBoundary);
            }
            Mutation::Delete(key)
        } else {
            return Err(EngineError::RuntimeFailure);
        };
        let key = match &mutation {
            Mutation::Put(record) => record.key(),
            Mutation::Delete(key) => key,
        };
        size += serde_json::to_vec(key)
            .map_err(|_| EngineError::RuntimeFailure)?
            .len()
            + 32;
        if !keys.insert(key.clone()) {
            return Err(EngineError::RuntimeFailure);
        }
        mutations.push(mutation);
    }
    if size > MAX_SNAPSHOT_BYTES {
        return Err(EngineError::LimitExceeded);
    }
    Ok(OperationResult {
        account: request.account.clone(),
        generation: request.generation,
        operation_id: request.operation_id,
        expected_revision: request.expected_revision,
        output: parsed,
        delta: MutationDelta { mutations },
    })
}
