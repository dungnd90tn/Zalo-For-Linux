//! Wasmi and its Rc heap never leave this bounded dedicated blocking worker.
use super::*;
use std::sync::mpsc::{sync_channel, SyncSender};
pub(super) struct TrustKeys {
    pub public: Vec<u8>,
    pub private: Vec<u8>,
    pub secret: Vec<u8>,
}
enum Command {
    Qr {
        token: String,
        now: u64,
        reply: oneshot::Sender<std::result::Result<String, LoginFailure>>,
    },
    Proof {
        data: Vec<u8>,
        mac: Vec<u8>,
        reply: oneshot::Sender<std::result::Result<CompanionProof, LoginFailure>>,
    },
    Verify {
        data: Vec<u8>,
        primary: Vec<u8>,
        now: u64,
        reply: oneshot::Sender<std::result::Result<(), LoginFailure>>,
    },
    Keys {
        reply: oneshot::Sender<std::result::Result<TrustKeys, LoginFailure>>,
    },
}
pub(super) struct TrustWorker {
    commands: SyncSender<Command>,
    pub public: Vec<u8>,
}
impl TrustWorker {
    pub async fn start(
        module: Option<PathBuf>,
        restored: Option<TrustKeys>,
    ) -> std::result::Result<Self, LoginFailure> {
        let (tx, rx) = sync_channel(1);
        let (ready, receive) = oneshot::channel();
        thread::Builder::new()
            .name("zalo-trust".into())
            .spawn(move || {
                let setup = (|| -> Result<_> {
                    let mut host = match module {
                        Some(path) => TrustHost::open(&path)?,
                        None => TrustHost::bundled()?,
                    };
                    let (identity, secret) = if let Some(saved) = restored {
                        (
                            IdentityKeys {
                                public_key: saved.public,
                                private_key: saved.private,
                            },
                            LinkingSecret(
                                saved
                                    .secret
                                    .try_into()
                                    .map_err(|_| anyhow::anyhow!("invalid saved trust key"))?,
                            ),
                        )
                    } else {
                        let identity = host.generate_identity()?;
                        let secret = host.generate_link_secret()?;
                        (identity, secret)
                    };
                    Ok((host, identity, secret))
                })();
                let Ok((mut host, identity, secret)) = setup else {
                    let _ = ready.send(Err(LoginFailure::Trust));
                    return;
                };
                if ready.send(Ok(identity.public_key.clone())).is_err() {
                    return;
                }
                let mut verified = false;
                let mut proof_primary = None;
                while let Ok(command) = rx.recv() {
                    match command {
                        Command::Qr { token, now, reply } => {
                            let result = host
                                .generate_linking_qr(&identity, &secret, &token, now)
                                .map(|qr| qr.text().to_owned())
                                .map_err(|_| LoginFailure::Trust);
                            let _ = reply.send(result);
                        }
                        Command::Proof { data, mac, reply } => {
                            verified = false;
                            let result = host
                                .create_companion_proof(&identity, &secret, &data, &mac)
                                .map_err(|_| LoginFailure::Trust);
                            if let Ok(proof) = &result {
                                proof_primary = Some(proof.primary_public_key.clone());
                            }
                            let _ = reply.send(result);
                        }
                        Command::Verify {
                            data,
                            primary,
                            now,
                            reply,
                        } => {
                            let result = if proof_primary
                                .as_ref()
                                .is_some_and(|expected| *expected != primary)
                            {
                                Err(LoginFailure::Trust)
                            } else {
                                host.verify_device_list_bytes(
                                    &data,
                                    &primary,
                                    now as i64,
                                    Some(&identity.public_key),
                                )
                                .map_err(|_| LoginFailure::Trust)
                            };
                            verified = result.is_ok();
                            let _ = reply.send(result);
                        }
                        Command::Keys { reply } => {
                            let result = if verified {
                                Ok(TrustKeys {
                                    public: identity.public_key.clone(),
                                    private: identity.private_key.clone(),
                                    secret: secret.0.to_vec(),
                                })
                            } else {
                                Err(LoginFailure::Trust)
                            };
                            let _ = reply.send(result);
                        }
                    }
                }
            })
            .map_err(|_| LoginFailure::Internal)?;
        let public = receive.await.map_err(|_| LoginFailure::Trust)??;
        Ok(Self {
            commands: tx,
            public,
        })
    }
    pub async fn qr(&self, token: String, now: u64) -> std::result::Result<String, LoginFailure> {
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(Command::Qr { token, now, reply })
            .map_err(|_| LoginFailure::Busy)?;
        rx.await.map_err(|_| LoginFailure::Trust)?
    }
    pub async fn proof(
        &self,
        data: Vec<u8>,
        mac: Vec<u8>,
    ) -> std::result::Result<CompanionProof, LoginFailure> {
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(Command::Proof { data, mac, reply })
            .map_err(|_| LoginFailure::Busy)?;
        rx.await.map_err(|_| LoginFailure::Trust)?
    }
    pub async fn verify(
        &self,
        data: Vec<u8>,
        primary: Vec<u8>,
        now: u64,
    ) -> std::result::Result<(), LoginFailure> {
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(Command::Verify {
                data,
                primary,
                now,
                reply,
            })
            .map_err(|_| LoginFailure::Busy)?;
        rx.await.map_err(|_| LoginFailure::Trust)?
    }
    pub async fn keys(&self) -> std::result::Result<TrustKeys, LoginFailure> {
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(Command::Keys { reply })
            .map_err(|_| LoginFailure::Busy)?;
        rx.await.map_err(|_| LoginFailure::Trust)?
    }
}
