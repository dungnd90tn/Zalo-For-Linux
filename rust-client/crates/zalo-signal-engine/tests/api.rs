use zalo_signal_engine::*;

#[test]
fn identifiers_reject_noncanonical_deserialization() {
    for value in ["0", "00", "0012", "-1", "", "1x", "1.0", "1;throw"] {
        let json = serde_json::to_string(value).unwrap();
        assert!(serde_json::from_str::<AccountId>(&json).is_err());
        assert!(serde_json::from_str::<GroupId>(&json).is_err());
    }
    for value in ["0", "01", "+1", "-1", "1.0", "18446744073709551616"] {
        assert!(
            serde_json::from_str::<RegistrationEpoch>(&serde_json::to_string(value).unwrap())
                .is_err()
        );
    }
    assert!(serde_json::from_str::<RegistrationEpoch>("\"18446744073709551615\"").is_ok());
    assert!(serde_json::from_str::<AccountId>("\"12345678901234567890\"").is_ok());
}

#[test]
fn nested_record_keys_reject_invalid_scopes_and_unknown_fields() {
    for key in [
        r#"{"kind":"local_identity","local":{"epoch":"0","platform":"Pc"}}"#,
        r#"{"kind":"local_identity","local":{"epoch":"1","platform":"Pc","extra":1}}"#,
        r#"{"kind":"session","local":{"epoch":"1","platform":"Pc"},"peer":{"uid":"0012","platform":"Mobile","epoch":"1"}}"#,
        r#"{"kind":"group_sender","local":{"epoch":"1","platform":"Pc"},"group":"0","sender":{"uid":"12","platform":"Pc","epoch":"1"}}"#,
        r#"{"kind":"local_identity","local":{"epoch":"1","platform":"Pc"},"unknown":true}"#,
    ] {
        assert!(serde_json::from_str::<RecordKey>(key).is_err())
    }
}

fn local() -> LocalNamespace {
    LocalNamespace {
        epoch: RegistrationEpoch::new(1).unwrap(),
        platform: SignalPlatform::Pc,
    }
}

#[test]
fn opaque_records_and_snapshots_enforce_limits_and_safe_json() {
    let key = RecordKey::LocalIdentity { local: local() };
    for json in [
        br#"{"__proto__":{"x":1}}"#.to_vec(),
        br#"{"x":{"constructor":true}}"#.to_vec(),
        vec![0xff],
        Vec::new(),
    ] {
        assert!(Record::from_parts(key.clone(), json).is_err())
    }
    let record = || Record::from_parts(key.clone(), b"{}".to_vec()).unwrap();
    assert!(Snapshot::new(AccountId::new("12").unwrap(), 0, vec![record(), record()]).is_err());
    let too_many = (0..=MAX_RECORDS)
        .map(|id| {
            Record::from_parts(
                RecordKey::PreKey {
                    local: local(),
                    id: id as u32,
                },
                b"{}".to_vec(),
            )
            .unwrap()
        })
        .collect();
    assert!(Snapshot::new(AccountId::new("12").unwrap(), 0, too_many).is_err());
    assert!(Record::from_parts(key, vec![b' '; MAX_SNAPSHOT_BYTES + 1]).is_err());
}

#[cfg(feature = "embedded")]
mod embedded {
    use super::*;
    use std::{
        collections::BTreeMap,
        thread,
        time::{Duration, Instant},
    };

    struct Client {
        uid: AccountId,
        local: LocalNamespace,
        engine: Engine,
        records: BTreeMap<RecordKey, Vec<u8>>,
        revision: u64,
        operation: u64,
    }
    impl Client {
        fn new(uid: &str) -> Self {
            let uid = AccountId::new(uid).unwrap();
            let local = local();
            Self {
                engine: Engine::new(uid.clone(), 1, EngineOptions::default()).unwrap(),
                uid,
                local,
                records: BTreeMap::new(),
                revision: 0,
                operation: 0,
            }
        }
        fn snapshot(&self) -> Snapshot {
            Snapshot::new(
                self.uid.clone(),
                self.revision,
                self.records
                    .iter()
                    .map(|(key, value)| Record::from_parts(key.clone(), value.clone()).unwrap())
                    .collect(),
            )
            .unwrap()
        }
        fn request(&mut self, operation: Operation) -> Request {
            self.operation += 1;
            Request {
                account: self.uid.clone(),
                generation: 1,
                operation_id: self.operation,
                expected_revision: self.revision,
                operation,
                snapshot: self.snapshot(),
            }
        }
        fn run(&mut self, operation: Operation) -> Result<OperationResult, EngineError> {
            let request = self.request(operation);
            wait(self.engine.submit(request)?)
        }
        fn commit(&mut self, result: OperationResult) -> Output {
            let (account, _, _, revision, output, delta) = result.into_parts();
            assert!(account == self.uid);
            assert_eq!(revision, self.revision);
            for mutation in delta.into_mutations() {
                match mutation {
                    Mutation::Put(record) => {
                        let (key, value) = record.into_parts();
                        self.records.insert(key, value);
                    }
                    Mutation::Delete(key) => {
                        self.records.remove(&key);
                    }
                }
            }
            self.revision += 1;
            output
        }
        fn bundle(&mut self) -> PublicBundle {
            let result = self
                .run(Operation::CreateBundle {
                    local: self.local.clone(),
                    prekey_count: 3,
                })
                .unwrap();
            let Output::Bundle(bundle) = self.commit(result) else {
                panic!("bundle output")
            };
            bundle
        }
    }
    fn wait(mut ticket: OperationTicket) -> Result<OperationResult, EngineError> {
        let start = Instant::now();
        loop {
            match ticket.try_recv()? {
                Some(result) => return Ok(result),
                None => {
                    assert!(start.elapsed() < Duration::from_secs(10));
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
    fn namespace(client: &Client) -> PeerNamespace {
        PeerNamespace {
            uid: client.uid.clone(),
            platform: SignalPlatform::Pc,
            epoch: RegistrationEpoch::new(1).unwrap(),
        }
    }
    fn trust(peer: &Client, bundle: &PublicBundle) -> TrustDecision {
        TrustDecision {
            peer: namespace(peer),
            registration_id: bundle.registration_id,
            identity_key: bundle.identity_key,
            session_timestamp: 100,
            basis: TrustBasis::AuthenticatedFirstUse,
        }
    }
    fn bundle(public: &PublicBundle) -> PeerBundle {
        PeerBundle {
            registration_id: public.registration_id,
            identity_key: public.identity_key,
            signed_prekey_id: public.signed_prekey_id,
            signed_prekey: public.signed_prekey,
            signature: public.signature,
            prekey: public.prekeys.first().copied(),
        }
    }
    fn establish(a: &mut Client, b: &Client, bundle_b: &PublicBundle) {
        let result = a
            .run(Operation::ProcessBundle {
                local: a.local.clone(),
                trust: trust(b, bundle_b),
                bundle: bundle(bundle_b),
            })
            .unwrap();
        assert!(matches!(a.commit(result), Output::SessionReady));
    }
    fn encrypt(
        a: &mut Client,
        b: &Client,
        bundle_b: &PublicBundle,
        body: Vec<u8>,
    ) -> PersonalCiphertext {
        let result = a
            .run(Operation::EncryptPersonal {
                local: a.local.clone(),
                trust: trust(b, bundle_b),
                plaintext: body,
            })
            .unwrap();
        let Output::PersonalEncrypted { message, .. } = a.commit(result) else {
            panic!("cipher output")
        };
        message
    }
    fn decrypt(
        b: &mut Client,
        a: &Client,
        bundle_a: &PublicBundle,
        message: PersonalCiphertext,
    ) -> Result<OperationResult, EngineError> {
        b.run(Operation::DecryptPersonal {
            local: b.local.clone(),
            trust: trust(a, bundle_a),
            message,
            message_timestamp: 200,
        })
    }
    fn copy_cipher(message: &PersonalCiphertext) -> PersonalCiphertext {
        PersonalCiphertext {
            message_type: message.message_type,
            body: message.body.clone(),
        }
    }
    fn plaintext(output: Output) -> Vec<u8> {
        let Output::Plaintext(value) = output else {
            panic!("plaintext output")
        };
        value
    }

    #[test]
    fn independent_original_asm_fixture_decrypts_and_consumes_prekey_transactionally() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/e2ee/session.json"))
                .unwrap();
        let bytes = |name: &str| {
            let text = fixture[name].as_str().unwrap();
            (0..text.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
                .collect::<Vec<_>>()
        };
        let mut bob = Client::new("202");
        let pair = |public: &str, private: &str| serde_json::json!({"publicKey":bytes(public),"privateKey":bytes(private)});
        let mut identity = pair("bob_identity_public", "bob_identity_private");
        identity["registrationId"] = 2002.into();
        for (key, value) in [
            (
                RecordKey::LocalIdentity {
                    local: bob.local.clone(),
                },
                identity,
            ),
            (
                RecordKey::SignedPreKey {
                    local: bob.local.clone(),
                    id: 7,
                },
                pair("bob_signed_prekey_public", "bob_signed_prekey_private"),
            ),
            (
                RecordKey::PreKey {
                    local: bob.local.clone(),
                    id: 9,
                },
                pair("bob_one_time_public", "bob_one_time_private"),
            ),
        ] {
            bob.records.insert(key, serde_json::to_vec(&value).unwrap());
        }
        let trust = TrustDecision {
            peer: PeerNamespace {
                uid: AccountId::new("101").unwrap(),
                platform: SignalPlatform::Pc,
                epoch: RegistrationEpoch::new(1).unwrap(),
            },
            registration_id: 1001,
            identity_key: bytes("alice_identity_public").try_into().unwrap(),
            session_timestamp: 100,
            basis: TrustBasis::AuthenticatedFirstUse,
        };
        let operation = || Operation::DecryptPersonal {
            local: local(),
            trust: TrustDecision {
                peer: trust.peer.clone(),
                registration_id: trust.registration_id,
                identity_key: trust.identity_key,
                session_timestamp: trust.session_timestamp,
                basis: trust.basis,
            },
            message: PersonalCiphertext {
                message_type: PersonalMessageType::PreKey,
                body: bytes("initial_prekey_message"),
            },
            message_timestamp: 200,
        };
        let before = bob.records.clone();
        let result = bob.run(operation()).unwrap();
        drop(result);
        assert!(bob.records == before);
        let result = bob.run(operation()).unwrap();
        assert!(result
            .delta()
            .mutations()
            .iter()
            .any(|mutation| matches!(mutation, Mutation::Delete(RecordKey::PreKey { id: 9, .. }))));
        assert_eq!(
            plaintext(bob.commit(result)),
            fixture["plaintext"].as_str().unwrap().as_bytes()
        );
        assert!(!bob.records.contains_key(&RecordKey::PreKey {
            local: local(),
            id: 9
        }));
    }

    #[test]
    fn replenish_preserves_existing_identity_and_uses_only_reserved_ids() {
        let mut bob = Client::new("202");
        let mut bundle_b = bob.bundle();
        let before = bob.records.clone();
        let ids: Vec<u32> = (1_000_000..1_000_019).chain([u32::MAX]).collect();
        let operation = || Operation::GeneratePreKeys {
            local: local(),
            reserved_ids: ids.clone(),
        };
        let result = bob.run(operation()).unwrap();
        assert_eq!(result.delta().mutations().len(), 20);
        for mutation in result.delta().mutations() {
            assert!(
                matches!(mutation,Mutation::Put(record) if matches!(record.key(),RecordKey::PreKey{id,..} if ids.contains(id)))
            )
        }
        drop(result);
        assert!(bob.records == before);
        let result = bob.run(operation()).unwrap();
        let Output::PreKeys {
            registration_id,
            prekeys,
        } = bob.commit(result)
        else {
            panic!("prekeys output")
        };
        assert_eq!(registration_id, bundle_b.registration_id);
        assert!(prekeys.iter().map(|(id, _)| *id).eq(ids.iter().copied()));
        for (key, value) in before {
            assert!(bob.records.get(&key) == Some(&value))
        }
        assert!(matches!(
            bob.run(operation()),
            Err(EngineError::AlreadyProvisioned)
        ));
        assert!(matches!(
            bob.run(Operation::GeneratePreKeys {
                local: local(),
                reserved_ids: vec![77, 77]
            }),
            Err(EngineError::InvalidInput)
        ));
        bundle_b.prekeys = prekeys;
        let mut alice = Client::new("101");
        let bundle_a = alice.bundle();
        establish(&mut alice, &bob, &bundle_b);
        let message = encrypt(&mut alice, &bob, &bundle_b, b"replenished".to_vec());
        let result = decrypt(&mut bob, &alice, &bundle_a, message).unwrap();
        assert_eq!(plaintext(bob.commit(result)), b"replenished");
        assert!(!bob.records.contains_key(&RecordKey::PreKey {
            local: local(),
            id: ids[0]
        }));
    }

    #[test]
    fn input_boundaries_zero_registration_and_lossless_request_identifiers() {
        let mut alice = Client::new("101");
        let mut bob = Client::new("202");
        let ba = alice.bundle();
        let mut bb = bob.bundle();
        // Registration IDs are14-bit values, independent from nonzero account IDs.
        bb.registration_id = 0;
        let identity_key = RecordKey::LocalIdentity { local: local() };
        let mut identity: serde_json::Value =
            serde_json::from_slice(bob.records.get(&identity_key).unwrap()).unwrap();
        identity["registrationId"] = 0.into();
        bob.records
            .insert(identity_key, serde_json::to_vec(&identity).unwrap());
        establish(&mut alice, &bob, &bb);
        let message = encrypt(&mut alice, &bob, &bb, b"zero registration".to_vec());
        let result = decrypt(&mut bob, &alice, &ba, message).unwrap();
        assert_eq!(plaintext(bob.commit(result)), b"zero registration");
        assert!(matches!(
            alice.run(Operation::EncryptPersonal {
                local: local(),
                trust: trust(&bob, &bb),
                plaintext: vec![0; MAX_TEXT_BYTES + 1]
            }),
            Err(EngineError::LimitExceeded)
        ));
        let mut unsafe_timestamp = trust(&bob, &bb);
        unsafe_timestamp.session_timestamp = 1 << 53;
        assert!(matches!(
            alice.run(Operation::EncryptPersonal {
                local: local(),
                trust: unsafe_timestamp,
                plaintext: vec![]
            }),
            Err(EngineError::InvalidInput)
        ));
        let wrong_local = LocalNamespace {
            epoch: RegistrationEpoch::new(2).unwrap(),
            platform: SignalPlatform::Pc,
        };
        assert!(matches!(
            alice.run(Operation::CreateBundle {
                local: wrong_local,
                prekey_count: 1
            }),
            Err(EngineError::InvalidRecord)
        ));
        let mut fresh = Client::new("303");
        let mut request = fresh.request(Operation::CreateBundle {
            local: local(),
            prekey_count: 1,
        });
        request.operation_id = u64::MAX;
        request.expected_revision = u64::MAX;
        request.snapshot = Snapshot::new(fresh.uid.clone(), u64::MAX, vec![]).unwrap();
        let result = wait(fresh.engine.submit(request).unwrap()).unwrap();
        assert_eq!(result.operation_id(), u64::MAX);
        assert_eq!(result.expected_revision(), u64::MAX);
    }

    #[test]
    fn real_api_personal_bidirectional_out_of_order_replay_restart_and_failed_commit() {
        let mut a = Client::new("101");
        let mut b = Client::new("202");
        let ba = a.bundle();
        let bb = b.bundle();
        establish(&mut a, &b, &bb);
        let first = encrypt(&mut a, &b, &bb, "Xin chào".as_bytes().to_vec());
        let result = decrypt(&mut b, &a, &ba, first).unwrap();
        assert_eq!(plaintext(b.commit(result)), "Xin chào".as_bytes());
        let reply = encrypt(&mut b, &a, &ba, b"reply".to_vec());
        let result = decrypt(&mut a, &b, &bb, reply).unwrap();
        assert_eq!(plaintext(a.commit(result)), b"reply");
        let mut pending = Vec::new();
        for index in 0..4 {
            pending.push(encrypt(&mut a, &b, &bb, vec![index]))
        }
        for index in [3, 0, 2, 1] {
            let result = decrypt(&mut b, &a, &ba, copy_cipher(&pending[index])).unwrap();
            assert_eq!(plaintext(b.commit(result)), vec![index as u8]);
        }
        assert!(matches!(
            decrypt(&mut b, &a, &ba, copy_cipher(&pending[1])),
            Err(EngineError::Replay)
        ));
        let message = encrypt(&mut a, &b, &bb, b"failed caller commit".to_vec());
        let before = b.records.clone();
        let rejected = decrypt(&mut b, &a, &ba, copy_cipher(&message)).unwrap();
        drop(rejected);
        assert!(b.records == before);
        // Recreate the actual engine; only committed opaque records survive.
        b.engine.shutdown();
        b.engine = Engine::new(b.uid.clone(), 1, EngineOptions::default()).unwrap();
        let result = decrypt(&mut b, &a, &ba, message).unwrap();
        assert_eq!(plaintext(b.commit(result)), b"failed caller commit");
        let maximum = encrypt(&mut a, &b, &bb, vec![b'x'; MAX_TEXT_BYTES]);
        let result = decrypt(&mut b, &a, &ba, maximum).unwrap();
        assert_eq!(plaintext(b.commit(result)).len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn tamper_and_identity_change_never_commit_or_poison_next_valid_operation() {
        let mut a = Client::new("101");
        let mut b = Client::new("202");
        let ba = a.bundle();
        let bb = b.bundle();
        establish(&mut a, &b, &bb);
        let message = encrypt(&mut a, &b, &bb, b"tamper guarded".to_vec());
        let mut corrupt = copy_cipher(&message);
        let index = corrupt.body.len() - 5;
        corrupt.body[index] ^= 1;
        let before = b.records.clone();
        assert!(decrypt(&mut b, &a, &ba, corrupt).is_err());
        assert!(b.records == before);
        let result = decrypt(&mut b, &a, &ba, message).unwrap();
        assert_eq!(plaintext(b.commit(result)), b"tamper guarded");
        let mut wrong = trust(&b, &bb);
        wrong.identity_key[1] ^= 1;
        let error = a.run(Operation::EncryptPersonal {
            local: a.local.clone(),
            trust: wrong,
            plaintext: b"must fail".to_vec(),
        });
        assert!(matches!(error, Err(EngineError::IdentityChanged)));
        assert!(matches!(
            a.run(Operation::CreateBundle {
                local: a.local.clone(),
                prekey_count: 1
            }),
            Err(EngineError::AlreadyProvisioned)
        ));
    }

    #[test]
    fn authenticated_distribution_group_tamper_replay_and_failed_commit() {
        let mut a = Client::new("101");
        let mut b = Client::new("202");
        let ba = a.bundle();
        let bb = b.bundle();
        establish(&mut a, &b, &bb);
        let group = GroupId::new("303").unwrap();
        let result = a
            .run(Operation::CreateGroupDistribution {
                local: a.local.clone(),
                group: group.clone(),
                sender_epoch: a.local.epoch.clone(),
            })
            .unwrap();
        let Output::GroupDistribution(distribution) = a.commit(result) else {
            panic!("distribution output")
        };
        let packet = encrypt(&mut a, &b, &bb, distribution);
        let result = b
            .run(Operation::ProcessGroupDistribution {
                local: b.local.clone(),
                trust: trust(&a, &ba),
                group: group.clone(),
                message: packet,
                message_timestamp: 200,
            })
            .unwrap();
        assert!(matches!(b.commit(result), Output::GroupReady));
        let mut messages = Vec::new();
        for index in 0..4 {
            let result = a
                .run(Operation::EncryptGroup {
                    local: a.local.clone(),
                    group: group.clone(),
                    sender_epoch: a.local.epoch.clone(),
                    plaintext: vec![index],
                })
                .unwrap();
            let Output::GroupEncrypted(message) = a.commit(result) else {
                panic!("group cipher output")
            };
            messages.push(message);
        }
        let mut forged = messages[0].clone();
        let last = forged.len() - 1;
        forged[last] ^= 1;
        let before = b.records.clone();
        assert!(matches!(
            b.run(Operation::DecryptGroup {
                local: b.local.clone(),
                group: group.clone(),
                trust: trust(&a, &ba),
                ciphertext: forged
            }),
            Err(EngineError::InvalidCiphertext)
        ));
        assert!(b.records == before);
        let receiver_local = b.local.clone();
        for index in [3, 0, 2, 1] {
            let operation = || Operation::DecryptGroup {
                local: receiver_local.clone(),
                group: group.clone(),
                trust: trust(&a, &ba),
                ciphertext: messages[index].clone(),
            };
            let result = b.run(operation()).unwrap();
            if index == 3 {
                drop(result);
                let result = b.run(operation()).unwrap();
                assert_eq!(plaintext(b.commit(result)), vec![index as u8]);
            } else {
                assert_eq!(plaintext(b.commit(result)), vec![index as u8]);
            }
        }
        assert!(matches!(
            b.run(Operation::DecryptGroup {
                local: b.local.clone(),
                group: group.clone(),
                trust: trust(&a, &ba),
                ciphertext: messages[0].clone()
            }),
            Err(EngineError::Replay)
        ));
        // Each sender owns a separate chain. Install Bob's distribution through
        // the authenticated reverse personal session, then exercise CBC overhead.
        let result = b
            .run(Operation::CreateGroupDistribution {
                local: b.local.clone(),
                group: group.clone(),
                sender_epoch: b.local.epoch.clone(),
            })
            .unwrap();
        let Output::GroupDistribution(distribution) = b.commit(result) else {
            panic!("reverse distribution")
        };
        let packet = encrypt(&mut b, &a, &ba, distribution);
        let result = a
            .run(Operation::ProcessGroupDistribution {
                local: a.local.clone(),
                trust: trust(&b, &bb),
                group: group.clone(),
                message: packet,
                message_timestamp: 200,
            })
            .unwrap();
        assert!(matches!(a.commit(result), Output::GroupReady));
        let result = b
            .run(Operation::EncryptGroup {
                local: b.local.clone(),
                group: group.clone(),
                sender_epoch: b.local.epoch.clone(),
                plaintext: vec![b'x'; MAX_TEXT_BYTES],
            })
            .unwrap();
        let Output::GroupEncrypted(ciphertext) = b.commit(result) else {
            panic!("reverse ciphertext")
        };
        let result = a
            .run(Operation::DecryptGroup {
                local: a.local.clone(),
                group,
                trust: trust(&b, &bb),
                ciphertext,
            })
            .unwrap();
        assert_eq!(plaintext(a.commit(result)).len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn account_generation_operation_revision_and_backpressure_are_checked() {
        let mut client = Client::new("101");
        let operation = || Operation::CreateBundle {
            local: local(),
            prekey_count: 1,
        };
        let mut request = client.request(operation());
        request.account = AccountId::new("202").unwrap();
        assert!(matches!(
            client.engine.submit(request),
            Err(EngineError::WrongAccount)
        ));
        let mut request = client.request(operation());
        request.expected_revision = 1;
        assert!(matches!(
            client.engine.submit(request),
            Err(EngineError::StaleRevision)
        ));
        let mut request = client.request(operation());
        request.generation = 2;
        assert!(matches!(
            client.engine.submit(request),
            Err(EngineError::StaleGeneration)
        ));
        let mut tickets = Vec::new();
        for _ in 0..QUEUE_CAPACITY {
            let request = client.request(operation());
            tickets.push(client.engine.submit(request).unwrap())
        }
        let request = client.request(operation());
        assert!(matches!(
            client.engine.submit(request),
            Err(EngineError::QueueFull)
        ));
        client.engine.advance_generation(2).unwrap();
        for ticket in tickets {
            assert!(matches!(wait(ticket), Err(EngineError::StaleGeneration)))
        }
        let mut request = client.request(operation());
        request.generation = 2;
        request.operation_id = 1;
        assert!(matches!(
            client.engine.submit(request),
            Err(EngineError::StaleOperation)
        ));
    }

    #[test]
    fn deadline_cancel_shutdown_and_csprng_identity_generation() {
        let uid = AccountId::new("101").unwrap();
        let engine = Engine::new(
            uid.clone(),
            1,
            EngineOptions {
                operation_timeout: Duration::from_millis(1),
            },
        )
        .unwrap();
        let request = Request {
            account: uid.clone(),
            generation: 1,
            operation_id: 1,
            expected_revision: 0,
            operation: Operation::CreateBundle {
                local: local(),
                prekey_count: 100,
            },
            snapshot: Snapshot::new(uid, 0, vec![]).unwrap(),
        };
        assert!(matches!(
            wait(engine.submit(request).unwrap()),
            Err(EngineError::Deadline)
        ));
        let mut client = Client::new("101");
        let request = client.request(Operation::CreateBundle {
            local: local(),
            prekey_count: 100,
        });
        let ticket = client.engine.submit(request).unwrap();
        ticket.cancel();
        assert!(matches!(wait(ticket), Err(EngineError::Cancelled)));
        let mut first = Client::new("101");
        let mut second = Client::new("101");
        assert!(first.bundle().identity_key != second.bundle().identity_key);
        first.engine.shutdown();
        let request = first.request(Operation::CreateBundle {
            local: local(),
            prekey_count: 1,
        });
        assert!(matches!(
            first.engine.submit(request),
            Err(EngineError::Stopped)
        ));
    }
}
