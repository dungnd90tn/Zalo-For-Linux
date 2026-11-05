use crate::*;
use futures_channel::oneshot;
use rquickjs::{context::EvalOptions, Context, Function, Promise, Runtime};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    task::{Context as TaskContext, Poll},
    time::{Duration, Instant},
};

#[path = "crypto.rs"]
mod crypto;
#[path = "wire.rs"]
mod wire;

struct Control {
    account: AccountId,
    generation: AtomicU64,
    highest_operation: AtomicU64,
    stopped: AtomicBool,
    outstanding: AtomicUsize,
}
struct Inner {
    sender: mpsc::SyncSender<Command>,
    control: Arc<Control>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.control.stopped.store(true, Ordering::Release);
        let _ = self.sender.try_send(Command::Stop);
    }
}
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}
enum Command {
    Run(Box<Job>),
    Stop,
}
struct Job {
    request: Request,
    cancel: Arc<AtomicBool>,
    result: oneshot::Sender<Result<OperationResult, EngineError>>,
}

pub struct OperationTicket {
    receiver: oneshot::Receiver<Result<OperationResult, EngineError>>,
    control: Arc<Control>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    complete: bool,
}
impl OperationTicket {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release)
    }
    fn finish(&mut self) {
        if !self.complete {
            self.complete = true;
            self.control.outstanding.fetch_sub(1, Ordering::AcqRel);
        }
    }
    pub fn try_recv(&mut self) -> Result<Option<OperationResult>, EngineError> {
        if self.complete {
            return Err(EngineError::Stopped);
        }
        match self.receiver.try_recv() {
            Ok(Some(result)) => {
                self.finish();
                self.check_current()?;
                result.map(Some)
            }
            Ok(None) => {
                if let Err(error) = self.check_current() {
                    self.finish();
                    return Err(error);
                }
                Ok(None)
            }
            Err(_) => {
                self.finish();
                Err(EngineError::Stopped)
            }
        }
    }
    fn check_current(&self) -> Result<(), EngineError> {
        if self.cancel.load(Ordering::Acquire) {
            Err(EngineError::Cancelled)
        } else if self.control.stopped.load(Ordering::Acquire) {
            Err(EngineError::Stopped)
        } else if self.control.generation.load(Ordering::Acquire) != self.generation {
            Err(EngineError::StaleGeneration)
        } else {
            Ok(())
        }
    }
}
impl Future for OperationTicket {
    type Output = Result<OperationResult, EngineError>;
    fn poll(mut self: Pin<&mut Self>, ctx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        if self.complete {
            return Poll::Ready(Err(EngineError::Stopped));
        }
        if let Err(error) = self.check_current() {
            self.finish();
            return Poll::Ready(Err(error));
        }
        match Pin::new(&mut self.receiver).poll(ctx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(value) => {
                self.finish();
                Poll::Ready(
                    self.check_current()
                        .and_then(|()| value.map_err(|_| EngineError::Stopped)?),
                )
            }
        }
    }
}
impl Drop for OperationTicket {
    fn drop(&mut self) {
        if !self.complete {
            self.cancel.store(true, Ordering::Release);
            self.control.outstanding.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Engine {
    pub fn new(
        account: AccountId,
        generation: u64,
        options: EngineOptions,
    ) -> Result<Self, EngineError> {
        if generation == 0
            || options.operation_timeout < Duration::from_millis(1)
            || options.operation_timeout > Duration::from_secs(60)
        {
            return Err(EngineError::InvalidInput);
        }
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let control = Arc::new(Control {
            account,
            generation: AtomicU64::new(generation),
            highest_operation: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            outstanding: AtomicUsize::new(0),
        });
        let worker_control = control.clone();
        std::thread::Builder::new()
            .name("zalo-signal-engine".into())
            .stack_size(4 * 1024 * 1024)
            .spawn(move || worker(receiver, worker_control, options))
            .map_err(|_| EngineError::RuntimeFailure)?;
        Ok(Self {
            inner: Arc::new(Inner { sender, control }),
        })
    }
    pub fn submit(&self, request: Request) -> Result<OperationTicket, EngineError> {
        let control = &self.inner.control;
        if control.stopped.load(Ordering::Acquire) {
            return Err(EngineError::Stopped);
        }
        if request.account != control.account || request.snapshot.account != control.account {
            return Err(EngineError::WrongAccount);
        }
        if request.generation != control.generation.load(Ordering::Acquire) {
            return Err(EngineError::StaleGeneration);
        }
        if request.expected_revision != request.snapshot.revision {
            return Err(EngineError::StaleRevision);
        }
        if request.operation_id == 0 {
            return Err(EngineError::InvalidInput);
        }
        control
            .outstanding
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < QUEUE_CAPACITY).then_some(count + 1)
            })
            .map_err(|_| EngineError::QueueFull)?;
        if control
            .highest_operation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                (request.operation_id > old).then_some(request.operation_id)
            })
            .is_err()
        {
            control.outstanding.fetch_sub(1, Ordering::AcqRel);
            return Err(EngineError::StaleOperation);
        }
        let generation = request.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, receiver) = oneshot::channel();
        let ticket = OperationTicket {
            receiver,
            control: control.clone(),
            cancel: cancel.clone(),
            generation,
            complete: false,
        };
        match self.inner.sender.try_send(Command::Run(Box::new(Job {
            request,
            cancel,
            result: tx,
        }))) {
            Ok(()) => Ok(ticket),
            Err(mpsc::TrySendError::Full(_)) => Err(EngineError::QueueFull),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(EngineError::Stopped),
        }
    }
    /// Invalidate queued, running and completed-but-uncollected old results.
    pub fn advance_generation(&self, generation: u64) -> Result<(), EngineError> {
        self.inner
            .control
            .generation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                (generation > old).then_some(generation)
            })
            .map(|_| ())
            .map_err(|_| EngineError::StaleGeneration)
    }
    pub fn shutdown(&self) {
        self.inner.control.stopped.store(true, Ordering::Release);
        let _ = self.inner.sender.try_send(Command::Stop);
    }
}

struct Budget {
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    generation: u64,
}
struct Host {
    context: Context,
    _runtime: Runtime,
    budget: Rc<RefCell<Budget>>,
    control: Arc<Control>,
}
impl Host {
    fn new(
        control: Arc<Control>,
        cancel: Arc<AtomicBool>,
        generation: u64,
        deadline: Instant,
    ) -> Result<Self, EngineError> {
        if format!("{:x}", Sha256::digest(crypto::LIBRARY.as_bytes())) != crypto::LIBRARY_SHA256 {
            return Err(EngineError::RuntimeFailure);
        }
        if format!(
            "{:x}",
            Sha256::digest(include_bytes!(concat!(env!("OUT_DIR"), "/group.js")))
        ) != "cd00830ac06f57ecf23c1162b650dbfa738af85a806b8b6a184942d63b502427"
        {
            return Err(EngineError::RuntimeFailure);
        }
        let runtime = Runtime::new().map_err(|_| EngineError::RuntimeFailure)?;
        runtime.set_memory_limit(64 * 1024 * 1024);
        runtime.set_max_stack_size(2 * 1024 * 1024);
        let budget = Rc::new(RefCell::new(Budget {
            deadline,
            cancel,
            generation,
        }));
        let interrupt = budget.clone();
        let current = control.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let value = interrupt.borrow();
            current.stopped.load(Ordering::Acquire)
                || current.generation.load(Ordering::Acquire) != value.generation
                || value.cancel.load(Ordering::Acquire)
                || Instant::now() >= value.deadline
        })));
        let context = Context::full(&runtime).map_err(|_| EngineError::RuntimeFailure)?;
        let loaded = context.with(|ctx| -> rquickjs::Result<()> {
            ctx.globals().set(
                "__nativeCrypto",
                Function::new(ctx.clone(), crypto::bridge_crypto)?,
            )?;
            ctx.globals().set(
                "__nativeRandom",
                Function::new(ctx.clone(), crypto::bridge_random)?,
            )?;
            ctx.globals().set(
                "__nativeCurve",
                Function::new(ctx.clone(), crypto::bridge_curve)?,
            )?;
            ctx.eval::<(), _>(include_str!("bridge.js"))?;
            const ANCHOR: &str = "Internal.Curve = n(Internal.curve25519)";
            if crypto::LIBRARY.matches(ANCHOR).count() != 1 {
                return Err(rquickjs::Error::Unknown);
            }
            let source = crypto::LIBRARY.replacen(
                ANCHOR,
                &format!("{}\n{ANCHOR}", include_str!("curve-glue.js")),
                1,
            );
            let mut options = EvalOptions::default();
            options.strict = false;
            ctx.eval_with_options::<(), _>(source, options)?;
            ctx.eval::<(), _>(include_str!(concat!(env!("OUT_DIR"), "/group.js")))?;
            ctx.eval::<(), _>(include_str!("group-glue.js"))?;
            ctx.eval::<(), _>(include_str!("engine.js"))?;
            Ok(())
        });
        let host = Self {
            context,
            _runtime: runtime,
            budget,
            control,
        };
        loaded.map_err(|_| host.interrupted().unwrap_or(EngineError::RuntimeFailure))?;
        Ok(host)
    }
    fn interrupted(&self) -> Option<EngineError> {
        let value = self.budget.borrow();
        if self.control.stopped.load(Ordering::Acquire) {
            Some(EngineError::Stopped)
        } else if self.control.generation.load(Ordering::Acquire) != value.generation {
            Some(EngineError::StaleGeneration)
        } else if value.cancel.load(Ordering::Acquire) {
            Some(EngineError::Cancelled)
        } else if Instant::now() >= value.deadline {
            Some(EngineError::Deadline)
        } else {
            None
        }
    }
    fn execute(
        &mut self,
        request: &Request,
        encoded: String,
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<OperationResult, EngineError> {
        *self.budget.borrow_mut() = Budget {
            deadline,
            cancel,
            generation: request.generation,
        };
        if let Some(error) = self.interrupted() {
            return Err(error);
        }
        let output = self
            .context
            .with(|ctx| -> rquickjs::Result<String> {
                ctx.globals().set("__requestJson", encoded)?;
                let promise: Promise = ctx.eval("__engineExecute(JSON.parse(__requestJson))")?;
                let mut count = 0usize;
                let result = loop {
                    if self.interrupted().is_some() {
                        return Err(rquickjs::Error::Unknown);
                    }
                    if let Some(value) = promise.result::<String>() {
                        break value?;
                    }
                    count += 1;
                    if count > 200_000 {
                        return Err(rquickjs::Error::Unknown);
                    }
                    if !ctx.execute_pending_job() {
                        return Err(rquickjs::Error::WouldBlock);
                    }
                };
                ctx.globals().set("__requestJson", "")?;
                // Drain original SessionLock cleanup promises before reusing a runtime.
                for _ in 0..200_000 {
                    if self.interrupted().is_some() {
                        return Err(rquickjs::Error::Unknown);
                    }
                    if !ctx.execute_pending_job() {
                        return Ok(result);
                    }
                }
                Err(rquickjs::Error::Unknown)
            })
            .map_err(|_| self.interrupted().unwrap_or(EngineError::RuntimeFailure))?;
        if let Some(error) = self.interrupted() {
            return Err(error);
        }
        wire::decode(request, &output)
    }
}

fn worker(receiver: mpsc::Receiver<Command>, control: Arc<Control>, options: EngineOptions) {
    let mut host = None;
    while !control.stopped.load(Ordering::Acquire) {
        let Ok(command) = receiver.recv() else { break };
        let Command::Run(job) = command else { break };
        let deadline = Instant::now() + options.operation_timeout;
        let current = || {
            if control.stopped.load(Ordering::Acquire) {
                Some(EngineError::Stopped)
            } else if job.cancel.load(Ordering::Acquire) {
                Some(EngineError::Cancelled)
            } else if control.generation.load(Ordering::Acquire) != job.request.generation {
                Some(EngineError::StaleGeneration)
            } else {
                None
            }
        };
        let result = (|| {
            if let Some(error) = current() {
                return Err(error);
            }
            let encoded = wire::encode(&job.request)?;
            if let Some(error) = current() {
                return Err(error);
            }
            if host.is_none() {
                host = Some(Host::new(
                    control.clone(),
                    job.cancel.clone(),
                    job.request.generation,
                    deadline,
                )?)
            }
            host.as_mut()
                .unwrap()
                .execute(&job.request, encoded, job.cancel.clone(), deadline)
        })();
        // Domain errors also discard the runtime: original SessionLock retains a
        // rejected cleanup chain, which must never retain an uncommitted overlay.
        if result.is_err() {
            host = None
        }
        let result = if let Some(error) = current() {
            Err(error)
        } else {
            result
        };
        let _ = job.result.send(result);
    }
    control.stopped.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn new_host() -> Host {
        let control = Arc::new(Control {
            account: AccountId::new("101").unwrap(),
            generation: AtomicU64::new(1),
            highest_operation: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            outstanding: AtomicUsize::new(0),
        });
        Host::new(
            control,
            Arc::default(),
            1,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap()
    }
    fn worker_test(test: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(4 * 1024 * 1024)
            .spawn(test)
            .unwrap()
            .join()
            .unwrap()
    }
    #[test]
    fn actual_host_enforces_heap_and_stack_limits() {
        worker_test(|| {
            let host = new_host();
            host.context.with(|ctx| {
                assert!(ctx.eval::<(), _>("new ArrayBuffer(65*1024*1024)").is_err());
                let _ = ctx.catch();
            });
            drop(host);
            let host = new_host();
            host.context.with(|ctx| {
                assert!(ctx
                    .eval::<(), _>("(function recurse(){return 1+recurse()})()")
                    .is_err());
                let _ = ctx.catch();
            });
        })
    }
    #[test]
    fn actual_host_interrupts_busy_javascript_on_deadline_and_operator_cancel() {
        worker_test(|| {
            for cancel_operation in [false, true] {
                let host = new_host();
                let token = host.budget.borrow().cancel.clone();
                let notifier = if cancel_operation {
                    Some(std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(20));
                        token.store(true, Ordering::Release)
                    }))
                } else {
                    host.budget.borrow_mut().deadline = Instant::now() + Duration::from_millis(20);
                    None
                };
                let started = Instant::now();
                host.context.with(|ctx| {
                    assert!(ctx.eval::<(), _>("while(true){}").is_err());
                    let _ = ctx.catch();
                });
                assert!(started.elapsed() < Duration::from_secs(1));
                assert_eq!(
                    host.interrupted(),
                    Some(if cancel_operation {
                        EngineError::Cancelled
                    } else {
                        EngineError::Deadline
                    })
                );
                if let Some(notifier) = notifier {
                    notifier.join().unwrap()
                }
                // A failed Host is deliberately dropped, never reset and reused.
            }
        })
    }
    #[test]
    fn actual_host_exposes_only_bounded_crypto_and_data_only_inputs() {
        worker_test(|| {
            let host = new_host();
            host.context.with(|ctx|{
            ctx.globals().set("__requestJson",r#"{"text":"');globalThis.injected=true;//"}"#).unwrap();
            ctx.eval::<(),_>("for(const name of ['require','process','std','os','fetch','WebSocket','XMLHttpRequest','Deno','Bun','__nativeCrypto','__nativeCurve','__nativeRandom'])if(typeof globalThis[name]!=='undefined')throw new Error('host binding');const data=JSON.parse(__requestJson);if(typeof injected!=='undefined')throw new Error('data executed');").unwrap();
            let promise:Promise=ctx.eval(r#"(async()=>{
                const hex=x=>Array.from(new Uint8Array(x),v=>v.toString(16).padStart(2,'0')).join('');
                const raw=new Uint8Array([99,97,98,99,99]);
                if(hex(await crypto.subtle.digest('SHA-256',raw.subarray(1,4)))!=='ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')throw new Error('offset digest');
                const output=new Uint8Array(66);output.fill(7);crypto.getRandomValues(output.subarray(1,65));if(output[0]!==7||output[65]!==7||output.subarray(1,65).every(x=>x===7))throw new Error('RNG offset');
                for(const invalid of [new Float64Array(4),new Uint8Array(65537)]){let rejected=false;try{crypto.getRandomValues(invalid)}catch(error){rejected=true}if(!rejected)throw new Error('RNG quota');}
                let rejected=false;try{await crypto.subtle.digest('SHA-256',new ArrayBuffer(131073))}catch(error){rejected=true}if(!rejected)throw new Error('primitive input quota');
            })()"#).unwrap();promise.finish::<()>().unwrap();
        });
        })
    }
}
