use super::*;
use crate::diagnostics::StorageOperation;
use crate::{
    app::directory::{DirectoryCategory, DirectoryFetcher, DirectorySink, DirectoryStage},
    protocol::messaging::{self as wire, DecodedFrame, FrameDecoder, Target, WireError},
    store::directory::DirectoryBatch,
    store::MessageStore,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    tungstenite::{self, client::IntoClientRequest, protocol::WebSocketConfig},
    MaybeTlsStream, WebSocketStream,
};
use url::Url;

#[cfg(feature = "signal-crypto")]
mod e2ee_init_devices;
#[cfg(feature = "signal-crypto")]
mod e2ee_init_peer;
mod e2ee_probe;
#[cfg(feature = "signal-crypto")]
mod e2ee_receive;
#[cfg(feature = "signal-crypto")]
mod e2ee_send;
#[cfg(feature = "signal-crypto")]
mod e2ee_send_probe;
mod e2ee_threads;
#[cfg(test)]
mod integration_tests;
mod privacy;

#[test]
fn outgoing_mismatch_diagnostics_report_only_closed_field_flags() {
    let mut store = crate::store::MessageStore::in_memory().unwrap();
    let id = store
        .prepare_outgoing("11", "d:22", "synthetic stored private body", 1000)
        .unwrap();
    store.record_outgoing_ack("11", id, "900").unwrap();
    store.accept_outgoing("11", id, "900", Some(1000)).unwrap();
    let message = crate::domain::Message {
        account_id: "11".into(),
        thread_id: "d:22".into(),
        id: "900".into(),
        sender: "11".into(),
        text: "synthetic changed private body".into(),
        sticker: None,
        media: None,
        sent_at_ms: 1001,
    };
    let error = store
        .receive_batch("11", &[(message, None)], None)
        .unwrap_err();
    let event = storage_failure_event(&error, StorageOperation::ReceiveBatch);
    let encoded = serde_json::to_string(&event).unwrap();
    let value: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(value["storage_mismatch_mask"], 2);
    assert_eq!(value["storage_failure"], "outgoing_mismatch");
    assert!(!encoded.contains("private body"));
    assert!(value.get("sqlite_code").is_none());
    for invalid in [0, 8, 255] {
        let value = serde_json::to_value(
            crate::diagnostics::Event::new(
                crate::diagnostics::Kind::Failure,
                crate::diagnostics::Stage::Storage,
                crate::diagnostics::Reason::Io,
            )
            .storage_mismatch(Some(invalid)),
        )
        .unwrap();
        assert!(value.get("storage_mismatch_mask").is_none());
    }
}
#[cfg(feature = "signal-crypto")]
pub(crate) mod signal_transport;

#[derive(Debug)]
struct NetworkFailure(LiveDiagnostic);
impl std::fmt::Display for NetworkFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for NetworkFailure {}
impl NetworkFailure {
    fn new(stage: LiveStage, category: LiveCategory) -> Self {
        Self(LiveDiagnostic {
            stage,
            category,
            http_status: None,
            server_code: None,
            parser_reason: None,
        })
    }
    fn socket(stage: LiveStage, error: tungstenite::Error) -> Self {
        let category = match &error {
            tungstenite::Error::Http(_) => LiveCategory::Http,
            tungstenite::Error::Tls(_) => LiveCategory::Tls,
            tungstenite::Error::Io(e)
                if stage == LiveStage::Connect
                    && matches!(e.kind(), std::io::ErrorKind::InvalidData) =>
            {
                LiveCategory::Tls
            }
            tungstenite::Error::Io(_) => LiveCategory::Network,
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                LiveCategory::Closed
            }
            tungstenite::Error::Capacity(_) | tungstenite::Error::WriteBufferFull(_) => {
                LiveCategory::Resource
            }
            _ => LiveCategory::Protocol,
        };
        let mut result = Self::new(stage, category);
        if let tungstenite::Error::Http(reply) = error {
            result.0.http_status = Some(reply.status().as_u16());
        }
        result
    }
}
#[derive(Debug)]
struct StorageContext(StorageOperation);
impl std::fmt::Display for StorageContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Local storage operation {:?} failed", self.0)
    }
}
impl std::error::Error for StorageContext {}
fn storage_result<T>(operation: StorageOperation, result: Result<T>) -> Result<T> {
    result.map_err(|error| error.context(StorageContext(operation)))
}
fn record_storage_failure(error: &anyhow::Error, operation: StorageOperation) {
    crate::diagnostics::record(storage_failure_event(error, operation));
}
fn storage_failure_event(
    error: &anyhow::Error,
    operation: StorageOperation,
) -> crate::diagnostics::Event {
    use crate::diagnostics::{Event, Kind, Reason, Stage, StorageFailure};
    use crate::store::FailureReason;
    let failure = match crate::store::classify_failure(error) {
        FailureReason::ConflictingMessage => StorageFailure::ConflictingMessage,
        FailureReason::ConflictingEcho => StorageFailure::ConflictingEcho,
        FailureReason::OutgoingMismatch => StorageFailure::OutgoingMismatch,
        FailureReason::ConflictingMetadata => StorageFailure::ConflictingMetadata,
        FailureReason::Validation => StorageFailure::Validation,
        FailureReason::Sqlite => StorageFailure::Sqlite,
        FailureReason::Other => StorageFailure::Other,
    };
    let sqlite_code = error.downcast_ref::<rusqlite::Error>().and_then(|error| {
        if let rusqlite::Error::SqliteFailure(code, _) = error {
            Some(code.extended_code)
        } else {
            None
        }
    });
    Event::new(Kind::Failure, Stage::Storage, Reason::Io)
        .storage(operation, failure, sqlite_code)
        .storage_mismatch(
            error
                .downcast_ref::<crate::store::OutgoingMismatchFields>()
                .map(|fields| fields.mask()),
        )
}
fn wire_error(error: &anyhow::Error) -> Option<&WireError> {
    error
        .downcast_ref::<wire::FrameFailure>()
        .map(|failure| &failure.error)
        .or_else(|| error.downcast_ref::<WireError>())
        .or_else(|| {
            error
                .downcast_ref::<wire::EligibilityFailure>()
                .map(|failure| &failure.error)
        })
}
fn diagnostic(error: &anyhow::Error, stage: LiveStage) -> LiveDiagnostic {
    if let Some(storage) = error.downcast_ref::<StorageContext>() {
        record_storage_failure(error, storage.0);
        return NetworkFailure::new(LiveStage::Storage, LiveCategory::Storage).0;
    }
    if let Some(network) = error.downcast_ref::<NetworkFailure>() {
        record_failure(network.0, None);
        return network.0;
    }
    let frame_failure = error.downcast_ref::<wire::FrameFailure>();
    let wire_error = wire_error(error);
    let category = match wire_error {
        Some(WireError::Invalid) => LiveCategory::Protocol,
        Some(WireError::Oversize) => LiveCategory::Resource,
        Some(WireError::Crypto) => LiveCategory::Crypto,
        Some(WireError::Unsupported) => LiveCategory::Unsupported,
        Some(WireError::Authentication) => LiveCategory::Authentication,
        Some(WireError::Service(_)) => LiveCategory::Server,
        None => LiveCategory::Storage,
    };
    let category = if error
        .downcast_ref::<wire::EligibilityFailure>()
        .is_some_and(|failure| failure.reason == wire::EligibilityReason::Timeout)
    {
        LiveCategory::Timeout
    } else {
        category
    };
    let result = LiveDiagnostic {
        stage,
        category,
        http_status: None,
        parser_reason: frame_failure.map(|failure| failure.reason),
        server_code: match wire_error {
            Some(WireError::Service(code)) => Some(*code),
            _ => None,
        },
    };
    if let Some(failure) = error.downcast_ref::<wire::EligibilityFailure>() {
        crate::diagnostics::record(eligibility_failure_event(failure));
    } else {
        record_failure(result, frame_failure.map(|failure| failure.context));
    }
    result
}
fn eligibility_failure_event(failure: &wire::EligibilityFailure) -> crate::diagnostics::Event {
    use crate::diagnostics::{CapabilityField as Field, Event, Kind, Reason, Stage};
    let reason = match failure.reason {
        wire::EligibilityReason::EncryptedTarget => Reason::CapabilityEncrypted,
        wire::EligibilityReason::Schema => Reason::CapabilitySchema,
        wire::EligibilityReason::Correlation => Reason::CapabilityCorrelation,
        wire::EligibilityReason::Timeout => Reason::CapabilityTimeout,
        wire::EligibilityReason::StateChanged => Reason::CapabilityChanged,
        wire::EligibilityReason::Pagination => Reason::CapabilityPagination,
    };
    let field = failure.field.map(|field| match field {
        wire::EligibilityField::Envelope => Field::Envelope,
        wire::EligibilityField::Request => Field::Request,
        wire::EligibilityField::Status => Field::Status,
        wire::EligibilityField::More => Field::More,
        wire::EligibilityField::Threads => Field::Threads,
        wire::EligibilityField::Waiting => Field::Waiting,
        wire::EligibilityField::Uids => Field::Uids,
        wire::EligibilityField::Pending => Field::Pending,
        wire::EligibilityField::Cursor => Field::Cursor,
        wire::EligibilityField::Group => Field::Group,
        wire::EligibilityField::Encryption => Field::Encryption,
        wire::EligibilityField::Visibility => Field::Visibility,
    });
    let mut event = Event::new(Kind::Failure, Stage::Send, reason)
        .capability_field(field)
        .field_type(Some(diagnostic_value_type(failure.value_type)))
        .server_code(match failure.error {
            WireError::Service(code) => Some(code),
            _ => None,
        });
    if let Some(command) = failure.command {
        event = event.frame(command, 0, None);
    }
    event
}
fn eligibility_message(reason: wire::EligibilityReason) -> &'static str {
    match reason {
        wire::EligibilityReason::EncryptedTarget=>"Hội thoại này đã bật mã hóa đầu cuối. Client chưa hỗ trợ gửi ở chế độ này; bản nháp được giữ lại.",
        wire::EligibilityReason::Schema=>"Chưa xác minh được trạng thái mã hóa do định dạng phản hồi máy chủ chưa được hỗ trợ. Bản nháp được giữ lại.",
        wire::EligibilityReason::Correlation=>"Phản hồi xác minh hội thoại không khớp yêu cầu. Hãy thử lại; bản nháp được giữ lại.",
        wire::EligibilityReason::Timeout=>"Quá thời gian xác minh trạng thái mã hóa. Hãy thử lại; bản nháp được giữ lại.",
        wire::EligibilityReason::StateChanged=>"Có sự kiện mã hóa trong lúc xác minh. Hãy thử lại; bản nháp được giữ lại.",
        wire::EligibilityReason::Pagination=>"Chưa lấy đủ trạng thái mã hóa của hội thoại. Hãy thử lại; bản nháp được giữ lại.",
    }
}
#[test]
fn eligibility_diagnostics_keep_only_fixed_reasons_fields_and_types() {
    use wire::{
        EligibilityFailure as Failure, EligibilityField as Field, EligibilityReason as Reason,
    };
    for (reason, name) in [
        (Reason::EncryptedTarget, "capability_encrypted"),
        (Reason::Schema, "capability_schema"),
        (Reason::Correlation, "capability_correlation"),
        (Reason::Timeout, "capability_timeout"),
        (Reason::StateChanged, "capability_changed"),
        (Reason::Pagination, "capability_pagination"),
    ] {
        let private = json!("synthetic private account message cookie URL");
        let mut failure = Failure::new(reason, Some(Field::Pending), Some(&private));
        failure.command = Some(563);
        let value = serde_json::to_value(eligibility_failure_event(&failure)).unwrap();
        assert_eq!(value["reason"], name);
        assert_eq!(value["capability_field"], "pending");
        assert_eq!(value["field_type"], "string");
        assert_eq!(value["command_id"], 563);
        assert!(!value.to_string().contains("private"));
        assert!(!format!("{failure:?}").contains("private"));
        assert!(value.as_object().unwrap().keys().all(|key| matches!(
            key.as_str(),
            "kind"
                | "stage"
                | "reason"
                | "capability_field"
                | "field_type"
                | "command"
                | "command_id"
                | "subcommand"
        )));
        let message = eligibility_message(reason);
        assert!(message.contains("bản nháp") || message.contains("Bản nháp"));
        if reason != Reason::EncryptedTarget {
            assert!(!message.contains("đã bật mã hóa đầu cuối"));
        }
    }
}
fn diagnostic_reason(reason: wire::FrameReason) -> crate::diagnostics::Reason {
    use crate::diagnostics::Reason;
    match reason {
        wire::FrameReason::MessageType => Reason::MessageType,
        wire::FrameReason::FrameLength => Reason::FrameLength,
        wire::FrameReason::OuterJson => Reason::OuterJson,
        wire::FrameReason::OuterEnvelope => Reason::OuterEnvelope,
        wire::FrameReason::AuthKey => Reason::AuthKey,
        wire::FrameReason::EncryptionMode => Reason::EncryptionMode,
        wire::FrameReason::OuterData => Reason::OuterData,
        wire::FrameReason::Base64 => Reason::Base64,
        wire::FrameReason::CipherIntegrity => Reason::CipherIntegrity,
        wire::FrameReason::Compression => Reason::Compression,
        wire::FrameReason::InnerJson => Reason::InnerJson,
        wire::FrameReason::InnerEnvelope => Reason::InnerEnvelope,
        wire::FrameReason::PlaintextAuth => Reason::PlaintextAuth,
        wire::FrameReason::QueueCommands => Reason::QueueCommands,
        wire::FrameReason::BatchEnvelope => Reason::BatchEnvelope,
        wire::FrameReason::BatchLimit => Reason::BatchLimit,
        wire::FrameReason::MessageTypeField => Reason::MessageTypeField,
        wire::FrameReason::MessageSender => Reason::MessageSender,
        wire::FrameReason::MessageRecipient => Reason::MessageRecipient,
        wire::FrameReason::MessageScope => Reason::MessageScope,
        wire::FrameReason::MessageTimestamp => Reason::MessageTimestamp,
        wire::FrameReason::MessageId => Reason::MessageId,
        wire::FrameReason::MessageContent => Reason::MessageContent,
        wire::FrameReason::MessageValidation => Reason::MessageValidation,
        wire::FrameReason::MessageClientId => Reason::MessageClientId,
        wire::FrameReason::QueueStatus => Reason::QueueStatus,
        wire::FrameReason::QueueKey => Reason::QueueKey,
        wire::FrameReason::QueueEvicted => Reason::QueueEvicted,
        wire::FrameReason::QueueCursor => Reason::QueueCursor,
        wire::FrameReason::QueueMore => Reason::QueueMore,
        wire::FrameReason::QueueContinuation => Reason::QueueContinuation,
        wire::FrameReason::QueueProgress => Reason::QueueProgress,
        wire::FrameReason::QueuePageLimit => Reason::QueuePageLimit,
    }
}
fn diagnostic_value_type(value: wire::FrameValueType) -> crate::diagnostics::ValueType {
    use crate::diagnostics::ValueType;
    match value {
        wire::FrameValueType::Absent => ValueType::Absent,
        wire::FrameValueType::Null => ValueType::Null,
        wire::FrameValueType::Integer => ValueType::Integer,
        wire::FrameValueType::Number => ValueType::Number,
        wire::FrameValueType::String => ValueType::String,
        wire::FrameValueType::Boolean => ValueType::Boolean,
        wire::FrameValueType::Object => ValueType::Object,
        wire::FrameValueType::Array => ValueType::Array,
    }
}
fn diagnostic_message_kind(value: wire::FrameMessageKind) -> crate::diagnostics::MessageKind {
    match value {
        wire::FrameMessageKind::Webchat => crate::diagnostics::MessageKind::Webchat,
        wire::FrameMessageKind::WebchatInfo => crate::diagnostics::MessageKind::WebchatInfo,
        wire::FrameMessageKind::Photo => crate::diagnostics::MessageKind::Photo,
        wire::FrameMessageKind::WebContent => crate::diagnostics::MessageKind::WebContent,
        wire::FrameMessageKind::Recommended => crate::diagnostics::MessageKind::Recommended,
        wire::FrameMessageKind::Poll => crate::diagnostics::MessageKind::Poll,
        wire::FrameMessageKind::File => crate::diagnostics::MessageKind::File,
        wire::FrameMessageKind::Sticker => crate::diagnostics::MessageKind::Sticker,
        wire::FrameMessageKind::Other => crate::diagnostics::MessageKind::Other,
    }
}
fn record_failure(detail: LiveDiagnostic, context: Option<wire::FrameContext>) {
    use crate::diagnostics::{record, Event, Kind, Reason, Stage};
    let stage = match detail.stage {
        LiveStage::Configuration | LiveStage::Connect => Stage::Connection,
        LiveStage::Authentication => Stage::Authentication,
        LiveStage::Send => Stage::Send,
        LiveStage::Receive => Stage::Receive,
        LiveStage::Synchronization => Stage::Synchronization,
        LiveStage::Heartbeat => Stage::Heartbeat,
        LiveStage::Storage => Stage::Storage,
    };
    let reason = match detail.parser_reason {
        Some(reason) => diagnostic_reason(reason),
        None => match detail.category {
            LiveCategory::Timeout => Reason::Timeout,
            LiveCategory::Http => Reason::HttpStatus,
            LiveCategory::Authentication | LiveCategory::Server => Reason::ServerCode,
            LiveCategory::Unsupported => Reason::Unsupported,
            LiveCategory::Protocol | LiveCategory::Crypto => Reason::Invalid,
            LiveCategory::Storage => Reason::Io,
            _ => Reason::Transport,
        },
    };
    let mut event = Event::new(Kind::Failure, stage, reason)
        .http_status(detail.http_status)
        .server_code(detail.server_code);
    if let Some(context) = context {
        event = event
            .bytes(context.bytes)
            .envelope_types(
                diagnostic_value_type(context.status_type),
                diagnostic_value_type(context.data_type),
            )
            .field_type(context.field_type.map(diagnostic_value_type))
            .field_bytes(context.field_bytes)
            .message_kind(context.message_kind.map(diagnostic_message_kind));
        if let (Some(command), Some(subcommand)) = (context.command, context.subcommand) {
            event = event.frame(command, subcommand, context.encryption)
        }
    }
    record(event);
}
fn trace(stage: crate::diagnostics::Stage, reason: crate::diagnostics::Reason) {
    crate::diagnostics::record(crate::diagnostics::Event::new(
        crate::diagnostics::Kind::Transition,
        stage,
        reason,
    ));
}
fn record_directory_failure(failure: &DirectoryFailure) {
    crate::diagnostics::record(directory_failure_event(failure));
}
fn directory_failure_event(failure: &DirectoryFailure) -> crate::diagnostics::Event {
    use crate::diagnostics::{DirectoryCategory as LogCategory, DirectoryStage as LogStage};
    use crate::diagnostics::{Event, Kind, Reason, Stage};
    let stage = match failure.stage {
        DirectoryStage::Account => LogStage::Account,
        DirectoryStage::Friends => LogStage::Friends,
        DirectoryStage::Groups => LogStage::Groups,
        DirectoryStage::Previews => LogStage::Previews,
        DirectoryStage::Storage => LogStage::Storage,
        DirectoryStage::Synchronization => LogStage::Synchronization,
    };
    let category = match failure.category {
        DirectoryCategory::Unavailable => LogCategory::Unavailable,
        DirectoryCategory::Network => LogCategory::Network,
        DirectoryCategory::Timeout => LogCategory::Timeout,
        DirectoryCategory::Http => LogCategory::Http,
        DirectoryCategory::Server => LogCategory::Server,
        DirectoryCategory::Crypto => LogCategory::Crypto,
        DirectoryCategory::Schema => LogCategory::Schema,
        DirectoryCategory::Resource => LogCategory::Resource,
        DirectoryCategory::Cancelled => LogCategory::Cancelled,
        DirectoryCategory::Storage => LogCategory::Storage,
    };
    let reason = match failure.category {
        DirectoryCategory::Unavailable => Reason::Unsupported,
        DirectoryCategory::Network => Reason::Transport,
        DirectoryCategory::Timeout => Reason::Timeout,
        DirectoryCategory::Http => Reason::HttpStatus,
        DirectoryCategory::Server => Reason::ServerCode,
        DirectoryCategory::Crypto | DirectoryCategory::Schema => Reason::Invalid,
        DirectoryCategory::Resource => Reason::FrameLength,
        DirectoryCategory::Cancelled => Reason::Cancelled,
        DirectoryCategory::Storage => Reason::Io,
    };
    let mut event = Event::new(
        Kind::Failure,
        Stage::Synchronization,
        failure
            .parser
            .map(|parser| diagnostic_reason(parser.reason))
            .unwrap_or(reason),
    )
    .directory(stage, category)
    .http_status(failure.http_status)
    .server_code(failure.server_code);
    if let Some(parser) = failure.parser {
        use crate::diagnostics::PreviewField as LogField;
        use crate::protocol::directory::PreviewField as WireField;
        let field = match parser.field {
            WireField::ResponseJson => LogField::ResponseJson,
            WireField::ResponseEnvelope => LogField::ResponseEnvelope,
            WireField::EncryptedData => LogField::EncryptedData,
            WireField::DecryptedJson => LogField::DecryptedJson,
            WireField::DecryptedEnvelope => LogField::DecryptedEnvelope,
            WireField::MessageCollections => LogField::MessageCollections,
            WireField::Msgs => LogField::Msgs,
            WireField::GroupMsgs => LogField::GroupMsgs,
            WireField::PageMsgs => LogField::PageMsgs,
            WireField::EeMsgs => LogField::EeMsgs,
        };
        event = event
            .preview_field(Some(field))
            .field_type(Some(diagnostic_value_type(parser.field_type)))
            .field_bytes(parser.field_bytes)
            .message_kind(parser.message_kind.map(diagnostic_message_kind));
    }
    event
}
/// Deterministic transport failures require an explicit retry, while the local
/// account database and command receiver remain available.
fn terminal_transport_state(error: &anyhow::Error) -> Option<LiveConnection> {
    if let Some(wire) = wire_error(error) {
        return Some(if *wire == WireError::Authentication {
            LiveConnection::Expired
        } else {
            LiveConnection::Offline
        });
    }
    let failure = error.downcast_ref::<NetworkFailure>()?;
    match failure.0.category {
        LiveCategory::Http => match failure.0.http_status {
            Some(401) => Some(LiveConnection::Expired),
            Some(300..=499) if failure.0.http_status != Some(429) => Some(LiveConnection::Offline),
            _ => None,
        },
        LiveCategory::Tls | LiveCategory::Protocol | LiveCategory::Resource => {
            Some(LiveConnection::Offline)
        }
        LiveCategory::Closed => match failure.0.server_code {
            Some(3000) => Some(LiveConnection::Expired),
            Some(3001..=3003) => Some(LiveConnection::Offline),
            _ => None,
        },
        _ => None,
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Job = Box<dyn FnOnce(&mut MessageStore) + Send>;
struct Database {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}
#[derive(Clone)]
struct DatabaseHandle {
    jobs: mpsc::Sender<Job>,
}
impl Database {
    async fn open(profile: Arc<Profile>, account: String) -> Result<Self> {
        let (tx, mut rx) = mpsc::channel::<Job>(16);
        let (ready, done) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("native-live-storage".into())
            .spawn(move || {
                let opened = profile
                    .bind_account(&account)
                    .and_then(|()| {
                        MessageStore::open(&profile.paths.data_dir.join("messages.sqlite"))
                    })
                    .and_then(|mut s| {
                        storage_result(
                            StorageOperation::RecoverOutgoing,
                            s.recover_outgoing(&account),
                        )?;
                        storage_result(
                            StorageOperation::ReconcileOutgoing,
                            s.reconcile_outgoing(&account),
                        )?;
                        Ok(s)
                    });
                let mut store = match opened {
                    Ok(s) => {
                        let _ = ready.send(Ok(()));
                        s
                    }
                    Err(error) => {
                        let _ =
                            ready.send(storage_result::<()>(StorageOperation::Open, Err(error)));
                        return;
                    }
                };
                while let Some(job) = rx.blocking_recv() {
                    job(&mut store)
                }
            })?;
        done.await??;
        Ok(Self {
            jobs: Some(tx),
            thread: Some(thread),
        })
    }
    async fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut MessageStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.handle()?.call(job).await
    }
    async fn call_with<T: Send + 'static>(
        &self,
        operation: StorageOperation,
        job: impl FnOnce(&mut MessageStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        storage_result(operation, self.call(job).await)
    }
    fn handle(&self) -> Result<DatabaseHandle> {
        Ok(DatabaseHandle {
            jobs: self
                .jobs
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Storage stopped"))?
                .clone(),
        })
    }
}
impl DatabaseHandle {
    async fn call_with<T: Send + 'static>(
        &self,
        operation: StorageOperation,
        job: impl FnOnce(&mut MessageStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        storage_result(operation, self.call(job).await)
    }
    async fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut MessageStore) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.jobs
            .send(Box::new(move |store| {
                let _ = tx.send(job(store));
            }))
            .await
            .map_err(|_| anyhow::anyhow!("Storage stopped"))?;
        rx.await.map_err(|_| anyhow::anyhow!("Storage stopped"))?
    }
}

fn directory_failure(category: DirectoryCategory) -> DirectoryFailure {
    DirectoryFailure {
        stage: DirectoryStage::Storage,
        category,
        http_status: None,
        server_code: None,
        parser: None,
    }
}
struct RuntimeDirectorySink {
    account: String,
    db: DatabaseHandle,
    updates: watch::Sender<u64>,
}
impl DirectorySink for RuntimeDirectorySink {
    async fn apply(&mut self, batch: DirectoryBatch) -> std::result::Result<(), DirectoryFailure> {
        if batch.account_id != self.account {
            return Err(directory_failure(DirectoryCategory::Storage));
        }
        self.db
            .call_with(StorageOperation::DirectoryWrite, move |store| {
                store.apply_directory_batch(&batch)?;
                Ok(())
            })
            .await
            .map_err(|_| directory_failure(DirectoryCategory::Storage))?;
        // Coalesce durable changes while the UI is slow. The directory task
        // never reserves critical WebSocket event capacity.
        self.updates
            .send_modify(|revision| *revision = revision.wrapping_add(1));
        Ok(())
    }
}
struct DirectoryRun {
    generation: u64,
    task: tokio::task::JoinHandle<std::result::Result<DirectoryReport, DirectoryFailure>>,
    cancel: watch::Sender<bool>,
}
impl Drop for DirectoryRun {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        self.task.abort();
    }
}
struct DirectorySync {
    running: Option<DirectoryRun>,
    pending: Option<u64>,
    updates_tx: watch::Sender<u64>,
    updates_rx: watch::Receiver<u64>,
    status: Option<(u64, bool, DirectoryReport)>,
    conversations_changed: bool,
}
impl DirectorySync {
    fn new() -> Self {
        let (updates_tx, updates_rx) = watch::channel(0);
        Self {
            running: None,
            pending: None,
            updates_tx,
            updates_rx,
            status: None,
            conversations_changed: false,
        }
    }
    fn request(
        &mut self,
        generation: u64,
        session: Arc<VerifiedSession>,
        db: DatabaseHandle,
        transports: &Transports,
    ) {
        if self.running.is_some() {
            self.pending = Some(generation);
            self.status = Some((generation, true, DirectoryReport::default()));
            return;
        }
        self.start(generation, session, db, transports);
    }
    fn start(
        &mut self,
        generation: u64,
        session: Arc<VerifiedSession>,
        db: DatabaseHandle,
        transports: &Transports,
    ) {
        let fetcher = {
            #[cfg(test)]
            {
                if transports.local.is_some() {
                    Ok(DirectoryFetcher::with_client(transports.http.clone()))
                } else {
                    DirectoryFetcher::new()
                }
            }
            #[cfg(not(test))]
            {
                let _ = transports;
                DirectoryFetcher::new()
            }
        };
        let fetcher = match fetcher {
            Ok(fetcher) => fetcher,
            Err(error) => {
                self.status = Some((
                    generation,
                    false,
                    DirectoryReport {
                        failures: vec![error],
                        ..Default::default()
                    },
                ));
                return;
            }
        };
        let (cancel, rx) = watch::channel(false);
        let mut sink = RuntimeDirectorySink {
            account: session.account_id().into(),
            db,
            updates: self.updates_tx.clone(),
        };
        let task = tokio::spawn(async move { fetcher.synchronize(&session, &mut sink, rx).await });
        self.running = Some(DirectoryRun {
            generation,
            task,
            cancel,
        });
        self.status = Some((generation, true, DirectoryReport::default()));
    }
    fn completed(
        &mut self,
        result: std::result::Result<
            std::result::Result<DirectoryReport, DirectoryFailure>,
            tokio::task::JoinError,
        >,
        session: Arc<VerifiedSession>,
        db: DatabaseHandle,
        transports: &Transports,
    ) {
        let generation = self
            .running
            .take()
            .expect("completed directory sync")
            .generation;
        let report = match result {
            Ok(Ok(report)) => report,
            Ok(Err(error)) => DirectoryReport {
                failures: vec![error],
                ..Default::default()
            },
            Err(_) => DirectoryReport {
                failures: vec![DirectoryFailure {
                    stage: DirectoryStage::Synchronization,
                    category: DirectoryCategory::Cancelled,
                    http_status: None,
                    server_code: None,
                    parser: None,
                }],
                ..Default::default()
            },
        };
        if let Some(generation) = self.pending.take() {
            self.start(generation, session, db, transports);
        } else {
            self.status = Some((generation, false, report));
        }
    }
    fn notifications_pending(&self) -> bool {
        self.status.is_some() || self.conversations_changed
    }
    async fn publish_notification(
        &mut self,
        permit: mpsc::Permit<'_, LiveEvent>,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        if let Some((generation, syncing, report)) = self.status.take() {
            for failure in &report.failures {
                record_directory_failure(failure);
            }
            permit.send(LiveEvent::DirectoryStatus {
                account_id: events.account.clone(),
                generation,
                syncing,
                report,
            });
        } else {
            let account = events.account.clone();
            // Read after obtaining UI capacity: earlier Received/Sent commits
            // are included, and later socket events cannot overtake this read.
            let conversations = db
                .call_with(StorageOperation::Conversations, move |store| {
                    store.conversations(&account, crate::domain::MAX_PAGE_SIZE)
                })
                .await?;
            self.conversations_changed = false;
            let event = LiveEvent::DirectoryChanged {
                account_id: events.account.clone(),
                conversations,
            };
            if let Some(event) = events
                .privacy
                .read()
                .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
                .event(event)
            {
                permit.send(event);
            }
        }
        (events.repaint)();
        Ok(())
    }
    fn changed(&mut self) {
        self.updates_rx.borrow_and_update();
        self.conversations_changed = true;
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        self.jobs.take();
        self.thread.take();
    }
}
struct Events {
    account: String,
    tx: mpsc::Sender<LiveEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    privacy: std::sync::RwLock<privacy::Projection>,
}
impl Events {
    async fn emit(&self, event: LiveEvent) -> Result<()> {
        let event = self
            .privacy
            .read()
            .map_err(|_| anyhow::anyhow!("Privacy state unavailable"))?
            .event(event);
        let Some(event) = event else {
            return Ok(());
        };
        (self.repaint)();
        self.tx
            .send(event)
            .await
            .map_err(|_| anyhow::anyhow!("UI stopped"))?;
        (self.repaint)();
        Ok(())
    }
    async fn connection(&self, state: LiveConnection) -> Result<()> {
        use crate::diagnostics::{Reason, Stage};
        trace(
            Stage::Connection,
            match state {
                LiveConnection::Connecting | LiveConnection::Reconnecting => Reason::Requested,
                LiveConnection::Online => Reason::Online,
                LiveConnection::Offline | LiveConnection::Expired => Reason::Offline,
            },
        );
        self.emit(LiveEvent::Connection {
            account_id: self.account.clone(),
            state,
        })
        .await
    }
    async fn diagnostic(&self, diagnostic: LiveDiagnostic) -> Result<()> {
        self.emit(LiveEvent::Diagnostic {
            account_id: self.account.clone(),
            diagnostic,
        })
        .await
    }
}
struct Transports {
    http: reqwest::Client,
    #[cfg(test)]
    local: Option<(std::net::SocketAddr, Arc<rustls::ClientConfig>)>,
}
impl Transports {
    async fn connect(&self, session: &VerifiedSession, index: usize) -> Result<Connection> {
        #[cfg(test)]
        if let Some((address, tls)) = &self.local {
            let request = Connection::handshake_request(session, index)?;
            let tcp = tokio::net::TcpStream::connect(address)
                .await
                .map_err(|_| NetworkFailure::new(LiveStage::Connect, LiveCategory::Network))?;
            let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
                request,
                tcp,
                Some(Connection::socket_config()),
                Some(tokio_tungstenite::Connector::Rustls(tls.clone())),
            )
            .await
            .map_err(|e| NetworkFailure::socket(LiveStage::Connect, e))?;
            return Ok(Connection::connected(socket, session.account_id()));
        }
        Connection::connect(session, index).await
    }
}
pub(super) async fn run(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    commands: mpsc::Receiver<LiveCommand>,
    events: mpsc::Sender<LiveEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    cancel: watch::Receiver<bool>,
) {
    let http = match client(session.client_type()) {
        Ok(client) => client,
        Err(_) => {
            let _ = events.try_send(LiveEvent::Fault {
                account_id: session.account_id().into(),
                error: "Cannot start messaging transport".into(),
            });
            repaint();
            return;
        }
    };
    let transports = Transports {
        http,
        #[cfg(test)]
        local: None,
    };
    run_with_transports(
        profile, session, commands, events, repaint, cancel, transports,
    )
    .await;
}
async fn run_with_transports(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    commands: mpsc::Receiver<LiveCommand>,
    events: mpsc::Sender<LiveEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    mut cancel: watch::Receiver<bool>,
    transports: Transports,
) {
    let account = session.account_id().to_owned();
    let event = Events {
        account,
        tx: events,
        repaint,
        privacy: std::sync::RwLock::new(privacy::Projection {
            status: LivePrivacyStatus::Checking,
            ..Default::default()
        }),
    };
    if *cancel.borrow() {
        return;
    }
    tokio::select! {biased; _=cancel.changed()=>{trace(crate::diagnostics::Stage::Shutdown,crate::diagnostics::Reason::Cancelled)},result=run_inner(profile,session,commands,&event,&transports)=>{if let Err(error)=result{
        let detail=diagnostic(&error,LiveStage::Storage);
        tokio::select!{biased;_=cancel.changed()=>{},_=event.diagnostic(detail)=>{}}
        tokio::select!{biased;_=cancel.changed()=>{},_=event.emit(LiveEvent::Fault{account_id:event.account.clone(),error:"Messaging stopped. Your local history and drafts are preserved.".into()})=>{}}
    }}}
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn client(client_type: crate::protocol::client::ClientType) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .user_agent(client_type.user_agent().ok_or(WireError::Unsupported)?)
        .build()?)
}
fn root_for(session: &VerifiedSession, name: &str) -> Result<Url> {
    session
        .config()
        .services
        .get(name)
        .and_then(|v| v.first())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("The server did not supply this messaging service"))
}
const MAX_OFFLINE_PAGES: u32 = 10_000;
#[derive(Default)]
struct OfflineProgress {
    last_id: u64,
    pages: u32,
}
fn safe_cursor(text: &str) -> Result<u64> {
    let value = text.parse::<u64>().map_err(|_| WireError::Invalid)?;
    if value == 0 || value > 9_007_199_254_740_991 {
        return Err(WireError::Invalid.into());
    }
    Ok(value)
}
struct Connection {
    // Fixed by VerifiedSession at connect, never by peer-provided message IDs.
    account: String,
    socket: Socket,
    decoder: FrameDecoder,
    queues: std::collections::HashSet<(u16, u8)>,
    offline_progress: std::collections::HashMap<(u16, u8), OfflineProgress>,
    request_id: u64,
    last_pong: tokio::time::Instant,
    last_ping: Option<i64>,
    authenticated_at: Option<tokio::time::Instant>,
    /// Ask for the E2EE conversation list after authenticating. On for real connections only, so
    /// scripted test sockets that expect an exact frame sequence are unaffected.
    list_encrypted: bool,
    /// Opt-in live decryption of pushed E2EE chat messages (report path); real connections only.
    #[cfg(feature = "signal-crypto")]
    e2ee_receive_report: Option<std::path::PathBuf>,
    #[cfg(feature = "signal-crypto")]
    e2ee_receive: Option<e2ee_receive::Receiver>,
    #[cfg(feature = "signal-crypto")]
    e2ee_http: Option<e2ee_send::E2eeHttp>,
}
impl Connection {
    async fn send(&mut self, cmd: u16, sub: u8, data: Value) -> Result<()> {
        let frame = wire::encode_frame(cmd, sub, &data)?;
        tokio::time::timeout(
            Duration::from_secs(5),
            self.socket.send(tungstenite::Message::Binary(frame)),
        )
        .await
        .map_err(|_| NetworkFailure::new(LiveStage::Receive, LiveCategory::Timeout))?
        .map_err(|e| NetworkFailure::socket(LiveStage::Receive, e))?;
        Ok(())
    }
    async fn next(&mut self) -> Result<Option<DecodedFrame>> {
        match self.socket.next().await {
            Some(Ok(tungstenite::Message::Binary(bytes))) => {
                Ok(Some(self.decoder.decode_detailed(&bytes)?))
            }
            Some(Ok(tungstenite::Message::Ping(bytes))) => {
                tokio::time::timeout(
                    Duration::from_secs(5),
                    self.socket.send(tungstenite::Message::Pong(bytes)),
                )
                .await
                .map_err(|_| NetworkFailure::new(LiveStage::Heartbeat, LiveCategory::Timeout))?
                .map_err(|e| NetworkFailure::socket(LiveStage::Heartbeat, e))?;
                Ok(None)
            }
            Some(Ok(tungstenite::Message::Pong(_))) => Ok(None),
            Some(Ok(tungstenite::Message::Close(frame))) => {
                let mut failure = NetworkFailure::new(LiveStage::Receive, LiveCategory::Closed);
                failure.0.server_code = frame.map(|frame| i64::from(u16::from(frame.code)));
                Err(failure.into())
            }
            None => Err(NetworkFailure::new(LiveStage::Receive, LiveCategory::Closed).into()),
            Some(Ok(_)) => Err(wire::FrameFailure::new(
                WireError::Invalid,
                wire::FrameReason::MessageType,
            )
            .into()),
            Some(Err(error)) => Err(NetworkFailure::socket(LiveStage::Receive, error).into()),
        }
    }
    fn handshake_request(
        session: &VerifiedSession,
        index: usize,
    ) -> Result<tungstenite::http::Request<()>> {
        let roots = &session.config().websocket;
        let root = roots
            .get(index % roots.len().max(1))
            .ok_or(WireError::Unsupported)?;
        let url = wire::socket_url(root, now_ms(), session.client_type())?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| WireError::Invalid)?;
        let mut cookie_url = url.clone();
        cookie_url
            .set_scheme("https")
            .map_err(|_| WireError::Invalid)?;
        if let Some(cookie) = session.authorization_cookie(&cookie_url) {
            request
                .headers_mut()
                .insert("Cookie", cookie.parse().map_err(|_| WireError::Invalid)?);
        }
        request.headers_mut().insert("Origin", "file://".parse()?);
        request.headers_mut().insert(
            "User-Agent",
            session
                .client_type()
                .user_agent()
                .ok_or(WireError::Unsupported)?
                .parse()?,
        );
        Ok(request)
    }
    fn socket_config() -> WebSocketConfig {
        WebSocketConfig {
            write_buffer_size: 32 * 1024,
            max_write_buffer_size: wire::MAX_FRAME + 32 * 1024,
            max_message_size: Some(wire::MAX_FRAME),
            max_frame_size: Some(wire::MAX_FRAME),
            accept_unmasked_frames: false,
            ..Default::default()
        }
    }
    async fn connect(session: &VerifiedSession, index: usize) -> Result<Self> {
        let request = Self::handshake_request(session, index)?;
        let config = Self::socket_config();
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(15),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| NetworkFailure::new(LiveStage::Connect, LiveCategory::Timeout))?
        .map_err(|e| NetworkFailure::socket(LiveStage::Connect, e))?;
        let mut connection = Self::connected(socket, session.account_id());
        connection.list_encrypted = true;
        #[cfg(feature = "signal-crypto")]
        {
            connection.e2ee_receive_report = e2ee_receive::target();
        }
        Ok(connection)
    }
    fn connected(socket: Socket, account: &str) -> Self {
        Self {
            account: account.to_owned(),
            socket,
            decoder: FrameDecoder::default(),
            queues: Default::default(),
            offline_progress: Default::default(),
            request_id: 0,
            last_pong: tokio::time::Instant::now(),
            last_ping: None,
            authenticated_at: None,
            list_encrypted: false,
            #[cfg(feature = "signal-crypto")]
            e2ee_receive_report: None,
            #[cfg(feature = "signal-crypto")]
            e2ee_receive: None,
            #[cfg(feature = "signal-crypto")]
            e2ee_http: None,
        }
    }
    fn request(&mut self) -> String {
        self.request_id = self.request_id.wrapping_add(1);
        format!("native_req_{}", self.request_id)
    }
    async fn offline(
        &mut self,
        db: &Database,
        account: &str,
        cmd: u16,
        sub: u8,
        first: bool,
        continuation: Option<&str>,
    ) -> Result<()> {
        let account = account.to_owned();
        let queue = format!("{cmd}_{sub}");
        let id = match continuation {
            Some(id) => id.to_owned(),
            None => {
                db.call_with(StorageOperation::ReceiveCursor, move |s| {
                    s.receive_cursor(&account, &queue)
                })
                .await?
            }
        };
        let failure = |error, reason| wire::FrameFailure {
            error,
            reason,
            context: wire::FrameContext {
                command: Some(cmd),
                subcommand: Some(sub),
                ..Default::default()
            },
        };
        let numeric = safe_cursor(&id)
            .map_err(|_| failure(WireError::Invalid, wire::FrameReason::QueueCursor))?;
        let progress = self.offline_progress.entry((cmd, sub)).or_default();
        if first {
            progress.pages = 0;
        } else if numeric <= progress.last_id {
            return Err(failure(WireError::Invalid, wire::FrameReason::QueueProgress).into());
        }
        if progress.pages >= MAX_OFFLINE_PAGES {
            return Err(failure(WireError::Oversize, wire::FrameReason::QueuePageLimit).into());
        }
        progress.last_id = numeric;
        progress.pages += 1;
        let req = self.request();
        self.send(
            cmd,
            sub,
            json!({"first":first,"reqId":req,"lastId":numeric,"preIds":[]}),
        )
        .await
    }
    async fn process(&mut self, frame: DecodedFrame, db: &Database, events: &Events) -> Result<()> {
        let failure = |error, reason| wire::FrameFailure {
            error,
            reason,
            context: wire::FrameContext {
                command: Some(frame.cmd),
                subcommand: Some(frame.sub_cmd),
                ..Default::default()
            },
        };
        let invalid = |reason| failure(WireError::Invalid, reason);
        if self.account != events.account {
            return Err(invalid(wire::FrameReason::MessageScope).into());
        }
        match frame.cmd {
            1 => {
                trace(
                    crate::diagnostics::Stage::Authentication,
                    crate::diagnostics::Reason::Ready,
                );
                self.authenticated_at = Some(tokio::time::Instant::now());
                let commands = frame
                    .payload
                    .pointer("/data/qCmds")
                    .and_then(Value::as_array)
                    .filter(|v| !v.is_empty() && v.len() <= 64)
                    .ok_or_else(|| {
                        wire::FrameFailure::new(
                            WireError::Invalid,
                            wire::FrameReason::QueueCommands,
                        )
                    })?;
                for command in commands {
                    let cmd = command.get("cmd").and_then(Value::as_u64).ok_or_else(|| {
                        wire::FrameFailure::new(
                            WireError::Invalid,
                            wire::FrameReason::QueueCommands,
                        )
                    })?;
                    let sub = command
                        .get("subCmd")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            wire::FrameFailure::new(
                                WireError::Invalid,
                                wire::FrameReason::QueueCommands,
                            )
                        })?;
                    if [510, 511].contains(&cmd) && sub <= 1 {
                        self.queues.insert((cmd as u16, sub as u8));
                    }
                }
                if self.queues.len() > 8 {
                    return Err(WireError::Oversize.into());
                }
                events.connection(LiveConnection::Online).await?;
                // Opt-in and read-only (E2EE_LIVE_PLAN.md stage 1); runs before any queue is requested.
                if let Some(path) = e2ee_probe::target().filter(|_| e2ee_probe::claim()) {
                    self.e2ee_probe(&path).await;
                }
                // Opt-in account change (E2EE_LIVE_PLAN.md stage 2): replace this account's PC key.
                #[cfg(feature = "signal-crypto")]
                if let Some(path) =
                    signal_transport::replace_target().filter(|_| signal_transport::claim_run())
                {
                    signal_transport::replace_pc_key(self, db, &events.account, &path).await;
                }
                // Session init with this account's other devices (what the original does after a new bundle).
                #[cfg(feature = "signal-crypto")]
                if let Some(path) =
                    e2ee_init_devices::target().filter(|_| e2ee_init_devices::claim())
                {
                    self.e2ee_init_subdevices(db, &path).await;
                }
                // Handshake with one contact (what the original does before the first message to a new peer).
                #[cfg(feature = "signal-crypto")]
                if let Some((path, peer)) =
                    e2ee_init_peer::target().filter(|_| e2ee_init_peer::claim())
                {
                    self.e2ee_init_peer(db, &path, peer.as_deref()).await;
                }
                // Send one real encrypted text (opt-in; the text goes to the chosen contact).
                #[cfg(feature = "signal-crypto")]
                if let Some((path, text, peer)) = e2ee_send::target().filter(|_| e2ee_send::claim())
                {
                    self.e2ee_send_text(db, events, &path, &text, peer.as_deref())
                        .await;
                }
                #[cfg(feature = "signal-crypto")]
                if let Some((path, peer)) =
                    e2ee_send_probe::target().filter(|_| e2ee_send_probe::claim())
                {
                    self.e2ee_send_probe(db, &path, peer.as_deref()).await;
                }
                #[cfg(not(feature = "signal-crypto"))]
                if let Some(path) =
                    e2ee_probe::replace_requested().filter(|_| e2ee_probe::claim_replace())
                {
                    e2ee_probe::note_unavailable(&path);
                }
                // Show end-to-end encrypted conversations (locked, unreadable here) instead of hiding them.
                if self.list_encrypted {
                    self.sync_encrypted_threads(db, events).await;
                }
                for (cmd, sub) in self.queues.clone() {
                    self.offline(db, &events.account, cmd, sub, true, None)
                        .await?;
                }
            }
            2 => {
                if frame
                    .payload
                    .get("eventId")
                    .and_then(Value::as_i64)
                    .is_some_and(|id| self.last_ping == Some(id))
                {
                    self.last_ping = None;
                    self.last_pong = tokio::time::Instant::now();
                    trace(
                        crate::diagnostics::Stage::Heartbeat,
                        crate::diagnostics::Reason::Ready,
                    );
                }
            }
            4 => {
                crate::diagnostics::record(
                    crate::diagnostics::Event::new(
                        crate::diagnostics::Kind::Debug,
                        crate::diagnostics::Stage::Heartbeat,
                        crate::diagnostics::Reason::Ready,
                    )
                    .frame(4, frame.sub_cmd, None),
                );
            }
            501 | 521 | 510 | 511 | 534 | 570 => {
                let parsed = self
                    .decoder
                    .incoming_messages(&self.account, &frame.payload)
                    .map_err(|mut error| {
                        error.context.command = Some(frame.cmd);
                        error.context.subcommand = Some(frame.sub_cmd);
                        error
                    })?;
                let messages: Vec<_> = parsed
                    .into_iter()
                    .map(|i| (i.message, i.client_id, i.info, i.media))
                    .collect();
                let account = events.account.clone();
                let mut cursors = Vec::new();
                if let Some(status) = frame.payload.pointer("/data/queueStatus") {
                    let map = status
                        .as_object()
                        .filter(|m| m.len() <= 64)
                        .ok_or_else(|| invalid(wire::FrameReason::QueueStatus))?;
                    for (queue, value) in map {
                        let (cmd, sub) = queue
                            .split_once('_')
                            .ok_or_else(|| invalid(wire::FrameReason::QueueKey))?;
                        let cmd = cmd
                            .parse::<u16>()
                            .map_err(|_| invalid(wire::FrameReason::QueueKey))?;
                        let sub = sub
                            .parse::<u8>()
                            .map_err(|_| invalid(wire::FrameReason::QueueKey))?;
                        if ![510, 511].contains(&cmd) || sub > 1 {
                            continue;
                        }
                        if value.get("evict").and_then(Value::as_u64) == Some(1) {
                            return Err(failure(
                                WireError::Unsupported,
                                wire::FrameReason::QueueEvicted,
                            )
                            .into());
                        }
                        let id = wire::id_string(
                            value
                                .get("lastId")
                                .ok_or_else(|| invalid(wire::FrameReason::QueueCursor))?,
                        )
                        .map_err(|error| failure(error, wire::FrameReason::QueueCursor))?;
                        cursors.push((queue.clone(), id));
                    }
                }
                let offline_queue = self.queues.contains(&(frame.cmd, frame.sub_cmd));
                let more = match frame.payload.pointer("/data/more") {
                    None => false,
                    Some(Value::Bool(value)) => *value,
                    Some(Value::Number(value)) if value.as_u64() == Some(0) => false,
                    Some(Value::Number(value)) if value.as_u64() == Some(1) => true,
                    _ => return Err(invalid(wire::FrameReason::QueueMore).into()),
                };
                let current_queue = format!("{}_{}", frame.cmd, frame.sub_cmd);
                // The PC client explicitly passes lastActionId to the next offline request.
                // queueStatus.lastId remains the normal reconnect cursor when available.
                let continuation = if offline_queue {
                    frame
                        .payload
                        .pointer("/data/lastActionId")
                        // The source consumes this field for pagination only while
                        // `more` is true. On a terminal page, null/zero means no
                        // cursor update. Preserve the existing positive fallback
                        // when queueStatus is absent; never persist a sentinel.
                        .filter(|value| {
                            more || !(value.is_null()
                                || value.as_u64() == Some(0)
                                || value.as_str() == Some("0"))
                        })
                        .map(|value| {
                            wire::id_string(value).map_err(|error| {
                                failure(error, wire::FrameReason::QueueContinuation)
                            })
                        })
                        .transpose()?
                        .or_else(|| {
                            cursors
                                .iter()
                                .find(|(queue, _)| queue == &current_queue)
                                .map(|(_, id)| id.clone())
                        })
                } else {
                    None
                };
                if offline_queue && more {
                    let next = safe_cursor(
                        continuation
                            .as_deref()
                            .ok_or_else(|| invalid(wire::FrameReason::QueueContinuation))?,
                    )
                    .map_err(|_| invalid(wire::FrameReason::QueueContinuation))?;
                    let previous = self
                        .offline_progress
                        .get(&(frame.cmd, frame.sub_cmd))
                        .ok_or_else(|| invalid(wire::FrameReason::QueueProgress))?;
                    if next <= previous.last_id {
                        return Err(invalid(wire::FrameReason::QueueProgress).into());
                    }
                    if previous.pages >= MAX_OFFLINE_PAGES {
                        return Err(failure(
                            WireError::Oversize,
                            wire::FrameReason::QueuePageLimit,
                        )
                        .into());
                    }
                }
                if offline_queue && !cursors.iter().any(|(queue, _)| queue == &current_queue) {
                    if let Some(id) = &continuation {
                        safe_cursor(id).map_err(|_| invalid(wire::FrameReason::QueueCursor))?;
                        cursors.push((current_queue, id.clone()));
                    }
                }
                let cursor = cursors.first().cloned();
                let stored = db
                    .call_with(StorageOperation::ReceiveBatch, move |s| {
                        s.receive_batch_with_media(
                            &account,
                            &messages,
                            cursor.as_ref().map(|(q, id)| (q.as_str(), id.as_str())),
                        )
                    })
                    .await?;
                for (queue, id) in cursors.into_iter().skip(1) {
                    let account = events.account.clone();
                    db.call_with(StorageOperation::ReceiveCursorWrite, move |s| {
                        s.receive_batch(&account, &[], Some((&queue, &id)))
                    })
                    .await?;
                }
                for (message, conversation) in stored {
                    trace(
                        crate::diagnostics::Stage::Receive,
                        crate::diagnostics::Reason::Ready,
                    );
                    events
                        .emit(LiveEvent::Received {
                            account_id: events.account.clone(),
                            message,
                            conversation,
                        })
                        .await?;
                }
                if offline_queue && more {
                    self.offline(
                        db,
                        &events.account,
                        frame.cmd,
                        frame.sub_cmd,
                        false,
                        continuation.as_deref(),
                    )
                    .await?;
                }
            }
            // Opt-in passive receive: decrypt and store pushed chat messages, acknowledge nothing.
            #[cfg(feature = "signal-crypto")]
            551 if self.e2ee_receive_report.is_some() => {
                self.e2ee_receive_push(&frame, db, events).await;
            }
            // These encrypted queues are intentionally not consumed or acknowledged.
            551 | 552 | 535 | 543 | 544 | 548 | 561 | 580 | 581 | 582 => {}
            _ => {}
        }
        Ok(())
    }
    async fn verify_direct(
        &mut self,
        target: &Target,
        db: &Database,
        events: &Events,
    ) -> Result<()> {
        let total_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut last = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let request = self.request();
            let data = if let Some(id) = &last {
                json!({"reqIid":1,"first":false,"lastId":id})
            } else {
                json!({"reqIid":1,"first":true})
            };
            self.send(563, 0, json!({"reqId":request,"data":data}))
                .await?;
            let deadline =
                (tokio::time::Instant::now() + Duration::from_secs(10)).min(total_deadline);
            loop {
                let frame = tokio::time::timeout_at(deadline, self.next())
                    .await
                    .map_err(|_| {
                        let mut failure = wire::EligibilityFailure::new(
                            wire::EligibilityReason::Timeout,
                            None,
                            None,
                        );
                        failure.command = Some(563);
                        failure
                    })??;
                let Some(frame) = frame else { continue };
                if frame.cmd == 563 {
                    let (allowed, next) = wire::legacy_direct_allowed_detailed(
                        &frame.payload,
                        &request,
                        &target.remote_id,
                    )
                    .map_err(|mut failure| {
                        failure.command = Some(563);
                        failure
                    })?;
                    if !allowed {
                        let mut failure = wire::EligibilityFailure::new(
                            wire::EligibilityReason::EncryptedTarget,
                            None,
                            None,
                        );
                        failure.command = Some(563);
                        return Err(failure.into());
                    }
                    match next {
                        None => return Ok(()),
                        Some(id) => {
                            if !seen.insert(id.clone()) {
                                let mut failure = wire::EligibilityFailure::new(
                                    wire::EligibilityReason::Pagination,
                                    Some(wire::EligibilityField::Cursor),
                                    None,
                                );
                                failure.command = Some(563);
                                return Err(failure.into());
                            }
                            last = Some(id);
                            break;
                        }
                    }
                }
                // Process all intervening data before checking the final capability snapshot.
                if [551, 552, 535, 543, 544, 548, 561, 580, 581, 582].contains(&frame.cmd) {
                    let mut failure = wire::EligibilityFailure::new(
                        wire::EligibilityReason::StateChanged,
                        None,
                        None,
                    );
                    failure.command = Some(frame.cmd);
                    return Err(failure.into());
                }
                self.process(frame, db, events).await?;
            }
        }
        let mut failure = wire::EligibilityFailure::new(
            wire::EligibilityReason::Pagination,
            Some(wire::EligibilityField::Cursor),
            None,
        );
        failure.command = Some(563);
        Err(failure.into())
    }
}
async fn read_response(
    mut response: reqwest::Response,
    session: &VerifiedSession,
) -> Result<Value> {
    if response.status() != reqwest::StatusCode::OK {
        anyhow::bail!("Messaging service returned an HTTP error")
    }
    if response
        .content_length()
        .is_some_and(|n| n > wire::MAX_FRAME as u64)
    {
        return Err(WireError::Oversize.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Messaging response interrupted"))?
    {
        if bytes.len() + chunk.len() > wire::MAX_FRAME {
            return Err(WireError::Oversize.into());
        }
        bytes.extend_from_slice(&chunk)
    }
    let outer: Value = serde_json::from_slice(&bytes).map_err(|_| WireError::Invalid)?;
    wire::service_code(&outer)?;
    let encrypted = outer
        .get("data")
        .and_then(Value::as_str)
        .ok_or(WireError::Invalid)?;
    let bytes = session.decrypt_payload(encrypted)?;
    if bytes.len() > wire::MAX_INFLATED {
        return Err(WireError::Oversize.into());
    }
    let inner = serde_json::from_slice(&bytes).map_err(|_| WireError::Invalid)?;
    wire::service_code(&inner)?;
    Ok(inner)
}
async fn authenticated_request(
    client: &reqwest::Client,
    session: &VerifiedSession,
    url: Url,
    body: Option<String>,
) -> Result<Value> {
    let cookie = session
        .authorization_cookie(&url)
        .ok_or_else(|| anyhow::anyhow!("Untrusted messaging origin"))?;
    let request = if let Some(body) = body {
        client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
    } else {
        client.get(url)
    };
    let response = request
        .header(
            "User-Agent",
            session
                .client_type()
                .user_agent()
                .ok_or(WireError::Unsupported)?,
        )
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Messaging request failed"))?;
    read_response(response, session).await
}
async fn verify_group(
    client: &reqwest::Client,
    session: &VerifiedSession,
    target: &Target,
) -> Result<u64> {
    let mut url = root_for(session, "group")?;
    wire::validate_origin(&url, "https")?;
    url.set_path("/api/group/getgi/v2");
    let params=session.encrypt_payload(&serde_json::to_vec(&json!({"grid":target.remote_id,"avatar_size":120,"member_avatar_size":120,"mpage":1,"mcount":100,"imei":session.imei()}))?)?;
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", session.client_type().api_type())
        .append_pair("params", &params);
    let payload = tokio::time::timeout(
        Duration::from_secs(15),
        authenticated_request(client, session, url, None),
    )
    .await
    .map_err(|_| wire::EligibilityFailure::new(wire::EligibilityReason::Timeout, None, None))??;
    Ok(wire::legacy_group_allowed_detailed(
        &payload,
        &target.remote_id,
    )?)
}
async fn send_text(
    client: &reqwest::Client,
    session: &VerifiedSession,
    target: &Target,
    text: &str,
    client_id: u64,
    visibility: Option<u64>,
) -> Result<wire::SendAck> {
    let root = root_for(
        session,
        match target.kind {
            ThreadKind::Direct => "chat",
            ThreadKind::Group => "group",
        },
    )?;
    let url = wire::send_url(&root, target.kind, session.client_type())?;
    let mut parameters = wire::text_parameters(target, text, client_id, session.imei())?;
    if target.kind == ThreadKind::Group {
        parameters["visibility"] = json!(visibility.ok_or(WireError::Invalid)?);
    }
    let params = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &params)
        .finish();
    let response = tokio::time::timeout(
        Duration::from_secs(15),
        authenticated_request(client, session, url, Some(body)),
    )
    .await??;
    Ok(wire::send_ack(&response)?)
}
async fn persist_ack(
    db: &Database,
    account: &str,
    id: u64,
    ack: wire::SendAck,
) -> Result<(Message, Conversation)> {
    let mut last = None;
    for attempt in 0..3 {
        let account = account.to_owned();
        let server = ack.server_id.clone();
        let timestamp = ack.timestamp_ms;
        let result = db
            .call_with(StorageOperation::PersistAck, move |s| {
                s.record_outgoing_ack(&account, id, &server)?;
                s.accept_outgoing(&account, id, &server, timestamp)
            })
            .await;
        match result {
            Ok(value) => return Ok(value),
            Err(error) => {
                let _ = diagnostic(&error, LiveStage::Storage);
                last = Some(error)
            }
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("Cannot store acknowledgement")))
}
enum CommandOutcome {
    Continue,
    PauseTransport,
    Shutdown,
}
async fn handle_command(
    command: LiveCommand,
    connection: &mut Option<Connection>,
    db: &Database,
    events: &Events,
    session: &VerifiedSession,
    client: &reqwest::Client,
) -> Result<CommandOutcome> {
    let requested = match &command {
        LiveCommand::RefreshMessages { thread, .. }
        | LiveCommand::ResolveSenders { thread, .. }
        | LiveCommand::Load { thread, .. }
        | LiveCommand::Overview { thread, .. }
        | LiveCommand::Send { thread, .. } => Some(thread.clone()),
        LiveCommand::Open {
            kind, remote_id, ..
        } => Some(Target::new(*kind, remote_id)?.thread_id()),
        _ => None,
    };
    if requested.as_deref().is_some_and(|thread| {
        !events
            .privacy
            .read()
            .is_ok_and(|state| state.permits(thread))
    }) {
        if let LiveCommand::Send {
            thread, request_id, ..
        } = command
        {
            events
                .emit(LiveEvent::SendSettled {
                    account_id: events.account.clone(),
                    thread,
                    request_id,
                    accepted: false,
                })
                .await?;
        }
        return Ok(CommandOutcome::Continue);
    }
    match command {
        LiveCommand::RefreshPrivacy
        | LiveCommand::RelockHidden
        | LiveCommand::SearchHidden { .. }
        | LiveCommand::UnlockHidden { .. } => return Ok(CommandOutcome::Continue),
        LiveCommand::Reconnect | LiveCommand::RefreshDirectory { .. } => {
            return Ok(CommandOutcome::Continue)
        }
        LiveCommand::Shutdown => return Ok(CommandOutcome::Shutdown),
        LiveCommand::RefreshMessages {
            generation,
            thread,
            message_ids,
        } => {
            let account = events.account.clone();
            let requested = thread.clone();
            let result = db
                .call_with(StorageOperation::LoadHistory, move |store| {
                    store.refresh_messages(&account, &requested, &message_ids)
                })
                .await;
            let event = match result {
                Ok(messages) => LiveEvent::MessagesRefreshed {
                    account_id: events.account.clone(),
                    generation,
                    thread,
                    messages,
                },
                Err(error) => {
                    record_storage_failure(&error, StorageOperation::LoadHistory);
                    LiveEvent::MessagesRefreshFailed {
                        account_id: events.account.clone(),
                        generation,
                        thread,
                    }
                }
            };
            events.emit(event).await?;
        }
        LiveCommand::ResolveAvatars {
            generation,
            thread_ids,
        } => {
            let account = events.account.clone();
            let avatars = db
                .call_with(StorageOperation::DirectoryRead, move |store| {
                    store.avatars(&account, &thread_ids)
                })
                .await;
            let avatars = match avatars {
                Ok(avatars) => avatars,
                Err(error) => {
                    record_storage_failure(&error, StorageOperation::DirectoryRead);
                    Vec::new()
                }
            };
            events
                .emit(LiveEvent::Avatars {
                    account_id: events.account.clone(),
                    generation,
                    avatars,
                })
                .await?;
        }
        LiveCommand::ResolveSenders {
            generation,
            thread,
            sender_ids,
        } => {
            let account = events.account.clone();
            let names = if Target::from_thread(&thread).is_ok() {
                match db
                    .call_with(StorageOperation::DirectoryRead, move |store| {
                        store.sender_names(&account, &sender_ids)
                    })
                    .await
                {
                    Ok(names) => names,
                    Err(error) => {
                        record_storage_failure(&error, StorageOperation::DirectoryRead);
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            events
                .emit(LiveEvent::SenderNames {
                    account_id: events.account.clone(),
                    generation,
                    thread,
                    names,
                })
                .await?;
        }
        LiveCommand::Directory {
            generation,
            is_group,
            query,
            after,
        } => {
            const PAGE_SIZE: usize = 100;
            let account = events.account.clone();
            let search = query.clone();
            let result = db
                .call_with(StorageOperation::DirectoryRead, move |store| {
                    store.directory_page(&account, is_group, &search, after.as_ref(), PAGE_SIZE + 1)
                })
                .await;
            let event = match result {
                Ok(mut entries) => {
                    let next = if entries.len() > PAGE_SIZE {
                        entries.pop();
                        entries.last().map(DirectoryCursor::from_record)
                    } else {
                        None
                    };
                    LiveEvent::DirectoryPage {
                        account_id: events.account.clone(),
                        generation,
                        is_group,
                        query,
                        entries,
                        next,
                    }
                }
                Err(_) => LiveEvent::DirectoryFailed {
                    account_id: events.account.clone(),
                    generation,
                    error: directory_failure(DirectoryCategory::Storage),
                },
            };
            events.emit(event).await?;
        }
        LiveCommand::Open {
            generation,
            kind,
            remote_id,
        } => {
            let target = Target::new(kind, &remote_id)?;
            let account = events.account.clone();
            let thread = target.thread_id();
            let title = match kind {
                ThreadKind::Direct => format!("Zalo ID {remote_id}"),
                ThreadKind::Group => format!("Nhóm {remote_id}"),
            };
            let result = db
                .call_with(StorageOperation::OpenConversation, move |s| {
                    let title = s
                        .directory_record(&account, &thread)?
                        .map(|entry| entry.display_name)
                        .unwrap_or(title);
                    s.open_conversation(&account, &thread, &title)
                })
                .await;
            match result {
                Ok(conversation) => {
                    events
                        .emit(LiveEvent::Opened {
                            account_id: events.account.clone(),
                            generation,
                            conversation,
                        })
                        .await?
                }
                Err(_) => {
                    events
                        .emit(LiveEvent::Fault {
                            account_id: events.account.clone(),
                            error: "Cannot open this conversation".into(),
                        })
                        .await?
                }
            }
        }
        LiveCommand::Load {
            generation,
            thread,
            before,
        } => {
            let account = events.account.clone();
            let requested = thread.clone();
            let result = db
                .call_with(StorageOperation::LoadHistory, move |s| {
                    s.page(
                        &account,
                        &requested,
                        before.as_ref(),
                        super::super::PAGE_SIZE + 1,
                    )
                })
                .await;
            let event = match result {
                Ok(mut messages) => {
                    let has_older = messages.len() > super::super::PAGE_SIZE;
                    if has_older {
                        messages.remove(0);
                    }
                    LiveEvent::Page {
                        account_id: events.account.clone(),
                        generation,
                        thread,
                        messages,
                        has_older,
                    }
                }
                Err(_) => LiveEvent::RequestFailed {
                    account_id: events.account.clone(),
                    generation,
                    thread,
                    error: "Cannot load this conversation".into(),
                },
            };
            events.emit(event).await?;
        }
        LiveCommand::Overview { generation, thread } => {
            let account = events.account.clone();
            let requested = thread.clone();
            let result = db
                .call_with(StorageOperation::LoadHistory, move |s| {
                    s.conversation_overview(
                        &account,
                        &requested,
                        crate::domain::OVERVIEW_ITEM_LIMIT,
                    )
                })
                .await;
            let event = match result {
                Ok(overview) => LiveEvent::Overview {
                    account_id: events.account.clone(),
                    generation,
                    thread,
                    overview: Box::new(overview),
                },
                Err(_) => LiveEvent::OverviewFailed {
                    account_id: events.account.clone(),
                    generation,
                    thread,
                },
            };
            events.emit(event).await?;
        }
        LiveCommand::Send {
            generation,
            thread,
            text,
            request_id,
        } => {
            trace(
                crate::diagnostics::Stage::Send,
                crate::diagnostics::Reason::Requested,
            );
            let target = Target::from_thread(&thread)?;
            let eligible = match connection.as_mut() {
                Some(c) if c.decoder.authenticated() => match target.kind {
                    ThreadKind::Direct => c.verify_direct(&target, db, events).await.map(|()| None),
                    ThreadKind::Group => verify_group(client, session, &target).await.map(Some),
                },
                _ => Err(anyhow::anyhow!("offline")),
            };
            let visibility = match eligible {
                Ok(visibility) => visibility,
                Err(error) => {
                    let storage_failed = error.downcast_ref::<StorageContext>().is_some();
                    let detail = diagnostic(&error, LiveStage::Send);
                    if storage_failed {
                        events.diagnostic(detail).await?;
                    }
                    let message = if storage_failed {
                        "Chưa lưu được dữ liệu nhận trên thiết bị. Kết nối đã tạm dừng; bản nháp được giữ lại."
                    } else if let Some(failure) = error.downcast_ref::<wire::EligibilityFailure>() {
                        eligibility_message(failure.reason)
                    } else if wire_error(&error)
                        .is_some_and(|e| matches!(e, WireError::Unsupported))
                    {
                        "Chưa xác minh được trạng thái mã hóa của hội thoại. Bản nháp được giữ lại."
                    } else {
                        "Chưa xác minh được hội thoại. Hãy kiểm tra kết nối; bản nháp được giữ lại."
                    };
                    events
                        .emit(LiveEvent::SendFailed {
                            account_id: events.account.clone(),
                            generation,
                            thread,
                            request_id,
                            error: message.into(),
                            unknown_delivery: false,
                            server_accepted: false,
                        })
                        .await?;
                    if storage_failed {
                        events.connection(LiveConnection::Offline).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                    if wire_error(&error) == Some(&WireError::Authentication) {
                        events
                            .diagnostic(diagnostic(&error, LiveStage::Authentication))
                            .await?;
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                    return Ok(CommandOutcome::Continue);
                }
            };
            let account = events.account.clone();
            let thread_copy = thread.clone();
            let text_copy = text.clone();
            let prepared = db
                .call_with(StorageOperation::PrepareOutgoing, move |s| {
                    s.prepare_outgoing(&account, &thread_copy, &text_copy, now_ms())
                })
                .await;
            let id = match prepared {
                Ok(id) => id,
                Err(error) => {
                    let _ = diagnostic(&error, LiveStage::Storage);
                    events.emit(LiveEvent::SendFailed{account_id:events.account.clone(),generation,thread,request_id,error:"Bản nháp đã có lượt gửi chưa xác định, hoặc không thể lưu khóa gửi an toàn. Không tự gửi lại.".into(),unknown_delivery:true,server_accepted:false}).await?;
                    return Ok(CommandOutcome::Continue);
                }
            };
            match send_text(client, session, &target, &text, id, visibility).await {
                Ok(ack) => {
                    trace(
                        crate::diagnostics::Stage::Send,
                        crate::diagnostics::Reason::Ready,
                    );
                    let account = events.account.clone();
                    let result = persist_ack(db, &account, id, ack).await;
                    match result {
                        Ok((message, conversation)) => {
                            events
                                .emit(LiveEvent::Sent {
                                    account_id: events.account.clone(),
                                    generation,
                                    thread,
                                    request_id,
                                    message,
                                    conversation,
                                })
                                .await?
                        }
                        Err(error) => {
                            events
                                .diagnostic(diagnostic(&error, LiveStage::Storage))
                                .await?;
                            events.emit(LiveEvent::SendFailed{account_id:events.account.clone(),generation,thread,request_id,error:"Zalo đã nhận nhưng chưa lưu được xác nhận trên thiết bị. Không gửi lại.".into(),unknown_delivery:true,server_accepted:true}).await?;
                            events.connection(LiveConnection::Offline).await?;
                            return Ok(CommandOutcome::PauseTransport);
                        }
                    }
                }
                Err(error) => {
                    let _ = diagnostic(&error, LiveStage::Send);
                    let known = wire_error(&error).is_some_and(|e| {
                        matches!(e, WireError::Service(_) | WireError::Authentication)
                    });
                    let expired = wire_error(&error) == Some(&WireError::Authentication);
                    let account = events.account.clone();
                    db.call_with(StorageOperation::FailOutgoing, move |s| {
                        s.fail_outgoing(&account, id, !known)
                    })
                    .await?;
                    events
                        .emit(LiveEvent::SendFailed {
                            account_id: events.account.clone(),
                            generation,
                            thread,
                            request_id,
                            error: if known {
                                "Zalo từ chối lượt gửi này. Bản nháp được giữ lại."
                            } else {
                                "Chưa có xác nhận từ Zalo. Không tự gửi lại."
                            }
                            .into(),
                            unknown_delivery: !known,
                            server_accepted: false,
                        })
                        .await?;
                    if expired {
                        events
                            .diagnostic(diagnostic(&error, LiveStage::Authentication))
                            .await?;
                        events.connection(LiveConnection::Expired).await?;
                        return Ok(CommandOutcome::PauseTransport);
                    }
                }
            }
        }
    }
    Ok(CommandOutcome::Continue)
}
fn retry_delay(
    connection: Option<&Connection>,
    backoff: &mut crate::reconnect::Backoff,
) -> Duration {
    if let Some(uptime) = connection
        .and_then(|c| c.authenticated_at)
        .map(|at| at.elapsed())
    {
        backoff.record_healthy_connection(uptime);
    }
    backoff.next_delay() + Duration::from_millis(u64::from(rand::random::<u8>()))
}
async fn run_inner(
    profile: Arc<Profile>,
    session: Arc<VerifiedSession>,
    mut commands: mpsc::Receiver<LiveCommand>,
    events: &Events,
    transports: &Transports,
) -> Result<()> {
    let db = Database::open(profile, events.account.clone()).await?;
    let account = events.account.clone();
    db.call(move |s| s.begin_live_privacy(&account)).await?;
    privacy::Privacy::announce(events).await?;
    let mut privacy = privacy::Privacy::new();
    privacy.start(session.clone(), transports);
    let client = transports.http.clone();
    let mut directory = DirectorySync::new();
    directory.request(0, session.clone(), db.handle()?, transports);
    let mut connection: Option<Connection> = None;
    let mut network_paused = false;
    let mut retries = 0_usize;
    let mut backoff = crate::reconnect::Backoff::default();
    let mut next_connect = tokio::time::Instant::now();
    let mut next_ping = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let privacy_deadline = privacy.next_check;
        let privacy_ready = events
            .privacy
            .read()
            .is_ok_and(|state| state.status == LivePrivacyStatus::Ready);
        tokio::select! {
            biased;
            result = async { (&mut directory.running.as_mut().expect("guarded directory sync").task).await }, if directory.running.is_some() => {
                directory.completed(result,session.clone(),db.handle()?,transports);
            },
            changed = directory.updates_rx.changed() => {
                changed.map_err(|_|anyhow::anyhow!("Directory stopped"))?;
                directory.changed();
            },
            permit = events.tx.reserve(), if directory.notifications_pending() => {
                directory.publish_notification(permit.map_err(|_|anyhow::anyhow!("UI stopped"))?,&db,events).await?;
            },
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()) };
                let Some(command) = privacy.command(command,&db,events,&session,transports).await? else { continue; };
                if let LiveCommand::RefreshDirectory{generation}=command {
                    directory.request(generation,session.clone(),db.handle()?,transports);
                    continue;
                }
                if matches!(command, LiveCommand::Reconnect) {
                    trace(crate::diagnostics::Stage::Connection,crate::diagnostics::Reason::Requested);
                    connection = None;
                    network_paused = false;
                    retries = 0;
                    backoff = crate::reconnect::Backoff::default();
                    next_connect = tokio::time::Instant::now();
                    continue;
                }
                let outcome = match handle_command(command, &mut connection, &db, events, &session, &client).await {
                    Ok(outcome) => outcome,
                    Err(error) if error.downcast_ref::<StorageContext>().is_some() => {
                        events.diagnostic(diagnostic(&error, LiveStage::Storage)).await?;
                        events.connection(LiveConnection::Offline).await?;
                        CommandOutcome::PauseTransport
                    }
                    Err(error) => return Err(error),
                };
                match outcome {
                    CommandOutcome::Shutdown => return Ok(()),
                    CommandOutcome::PauseTransport => { connection=None;network_paused=true; },
                    CommandOutcome::Continue => {},
                }
            },
            result = privacy.result(), if privacy.running() => { privacy.completed(result,&db,events).await?; },
            _ = tokio::time::sleep_until(privacy_deadline), if !privacy.running() => { privacy.start(session.clone(),transports); },
            _ = tokio::time::sleep_until(next_connect), if connection.is_none() && !network_paused && privacy_ready => {
                events.connection(if retries == 0 { LiveConnection::Connecting } else { LiveConnection::Reconnecting }).await?;
                let connecting = transports.connect(&session, retries);
                tokio::pin!(connecting);
                let result = loop {
                    let privacy_deadline=privacy.next_check;
                    tokio::select! {
                        biased;
                        result = async { (&mut directory.running.as_mut().expect("guarded directory sync").task).await }, if directory.running.is_some() => {
                            directory.completed(result,session.clone(),db.handle()?,transports);
                        },
                        changed = directory.updates_rx.changed() => {
                            changed.map_err(|_|anyhow::anyhow!("Directory stopped"))?;
                            directory.changed();
                        },
                        permit = events.tx.reserve(), if directory.notifications_pending() => {
                            directory.publish_notification(permit.map_err(|_|anyhow::anyhow!("UI stopped"))?,&db,events).await?;
                        },
                        result = &mut connecting => break Some(result),
                        command = commands.recv() => {
                            let Some(command) = command else { return Ok(()) };
                            let Some(command) = privacy.command(command,&db,events,&session,transports).await? else { continue; };
                            if let LiveCommand::RefreshDirectory{generation}=command {
                                directory.request(generation,session.clone(),db.handle()?,transports);
                                continue;
                            }
                            if matches!(command, LiveCommand::Reconnect) {
                                trace(crate::diagnostics::Stage::Connection,crate::diagnostics::Reason::Requested);
                                break None;
                            }
                            let outcome = match handle_command(command, &mut connection, &db, events, &session, &client).await {
                    Ok(outcome) => outcome,
                    Err(error) if error.downcast_ref::<StorageContext>().is_some() => {
                        events.diagnostic(diagnostic(&error, LiveStage::Storage)).await?;
                        events.connection(LiveConnection::Offline).await?;
                        CommandOutcome::PauseTransport
                    }
                    Err(error) => return Err(error),
                };
                match outcome {
                                CommandOutcome::Shutdown => return Ok(()),
                                CommandOutcome::PauseTransport => { network_paused=true;break None; },
                                CommandOutcome::Continue => {},
                            }
                        },
                        result = privacy.result(), if privacy.running() => { privacy.completed(result,&db,events).await?; },
                        _ = tokio::time::sleep_until(privacy_deadline), if !privacy.running() => { privacy.start(session.clone(),transports); },
                    }
                };
                let Some(result) = result else {
                    retries = 0;
                    backoff = crate::reconnect::Backoff::default();
                    next_connect = tokio::time::Instant::now();
                    continue;
                };
                match result {
                    Ok(c) => {
                        #[cfg(feature = "signal-crypto")]
                        let c = {
                            let mut c = c;
                            c.e2ee_http = Some(e2ee_send::E2eeHttp {
                                client: client.clone(),
                                session: session.clone(),
                            });
                            c
                        };
                        connection = Some(c);
                        next_ping = tokio::time::Instant::now() + Duration::from_secs(15);
                    },
                    Err(error) => {
                        events.diagnostic(diagnostic(&error, LiveStage::Configuration)).await?;
                        if let Some(state) = terminal_transport_state(&error) {
                            network_paused = true;
                            events.connection(state).await?;
                            continue;
                        }
                        retries = retries.saturating_add(1);
                        next_connect = tokio::time::Instant::now() + retry_delay(None, &mut backoff);
                        events.connection(LiveConnection::Offline).await?;
                    }
                }
            },
            frame = async { connection.as_mut().expect("guarded connection").next().await }, if connection.is_some() => {
                let stage = if frame.as_ref().ok().and_then(Option::as_ref).is_some_and(|frame| frame.cmd == 1) || !connection.as_ref().is_some_and(|c| c.decoder.authenticated()) {LiveStage::Authentication} else {LiveStage::Synchronization};
                let refresh_ping = frame.as_ref().ok().and_then(Option::as_ref).is_some_and(|frame| frame.cmd == 1 || (frame.cmd == 2 && frame.payload.get("eventId").and_then(Value::as_i64).is_some_and(|id| connection.as_ref().is_some_and(|c| c.last_ping == Some(id)))));
                let result = match frame {
                    Ok(Some(frame)) => connection.as_mut().unwrap().process(frame, &db, events).await,
                    Ok(None) => Ok(()),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    events.diagnostic(diagnostic(&error, stage)).await?;
                    if let Some(state) = terminal_transport_state(&error) {
                        connection = None;
                        network_paused = true;
                        events.connection(state).await?;
                        continue;
                    }
                    if error.downcast_ref::<StorageContext>().is_some() {
                        connection = None;
                        network_paused = true;
                        events.connection(LiveConnection::Offline).await?;
                        continue;
                    }
                    if error.downcast_ref::<NetworkFailure>().is_none() {
                        return Err(error);
                    }
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                    events.connection(LiveConnection::Reconnecting).await?;
                } else if refresh_ping && connection.as_ref().is_some_and(|c|c.last_ping.is_none()) {
                    next_ping = tokio::time::Instant::now() + Duration::from_secs(30);
                }
            },
            _ = tokio::time::sleep_until(next_ping), if connection.is_some() => {
                trace(crate::diagnostics::Stage::Heartbeat,crate::diagnostics::Reason::Requested);
                let c = connection.as_mut().unwrap();
                if !c.decoder.authenticated() || c.last_ping.is_some() {
                    let stage=if c.decoder.authenticated(){LiveStage::Heartbeat}else{LiveStage::Authentication};
                    events.diagnostic(NetworkFailure::new(stage,LiveCategory::Timeout).0).await?;
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                    events.connection(LiveConnection::Reconnecting).await?;
                    continue;
                }
                let now = now_ms();
                c.last_ping = Some(now);
                if let Err(error)=c.send(2, 1, json!({"eventId": now})).await {
                    events.diagnostic(diagnostic(&error,LiveStage::Heartbeat)).await?;
                    next_connect = tokio::time::Instant::now() + retry_delay(connection.as_ref(), &mut backoff);
                    connection = None;
                    retries = retries.saturating_add(1);
                }
                // A matching server pong schedules the normal 30-second interval.
                next_ping = tokio::time::Instant::now() + Duration::from_secs(5);
            }
        }
    }
}
