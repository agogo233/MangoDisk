use super::{cpu, security};
use mangodisk_privileged_protocol::{
    self as protocol, Capability, CapabilityStatus, Operation, Reply, Request, Response,
    SERVICE_NAME,
};
use std::{
    cell::RefCell,
    ffi::OsString,
    io,
    rc::Rc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::watch,
};
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}
define_windows_service!(ffi_service_main, service_main);
fn status(state: ServiceState, code: u32) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: ServiceExitCode::Win32(code),
        checkpoint: 0,
        wait_hint: Duration::ZERO,
        process_id: None,
    }
}
fn service_main(_arguments: Vec<OsString>) {
    let (stop, receiver) = watch::channel(false);
    let handle = match service_control_handler::register(SERVICE_NAME, move |control| match control
    {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = stop.send(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    }) {
        Ok(handle) => handle,
        Err(_) => return,
    };
    let result = (|| {
        security::allow_process_identity_query()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        tokio::task::LocalSet::new().block_on(&runtime, async {
            let pipe = create_pipe(true)?;
            handle
                .set_service_status(status(ServiceState::Running, 0))
                .map_err(io::Error::other)?;
            super::diagnostics::info(format!(
                "privileged_service_started version={} protocol={}",
                env!("CARGO_PKG_VERSION"),
                protocol::SCHEMA_VERSION
            ));
            serve(pipe, receiver).await
        })
    })();
    if let Err(error) = &result {
        super::diagnostics::error(format!("privileged_runtime_failed error={error}"));
    }
    let code = result.as_ref().err().map_or(0, |_| 1);
    super::diagnostics::info(format!("privileged_service_stopped exit={code}"));
    let _ = handle.set_service_status(status(ServiceState::Stopped, code));
}
fn create_pipe(first: bool) -> io::Result<NamedPipeServer> {
    // FILE_GENERIC_WRITE includes CREATE_PIPE_INSTANCE. Interactive clients get
    // only data/attribute IO, never permission to create a service pipe instance.
    let descriptor = security::Descriptor::new("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x120183;;;IU)")?;
    let mut attributes = descriptor.attributes();
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(9)
            .create_with_security_attributes_raw(
                protocol::PIPE_NAME,
                (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
            )
    }
}

#[derive(Default)]
struct CpuState {
    reader: Option<cpu::Reader>,
    last_status: Option<CapabilityStatus>,
    cache: crate::cpu_cache::CpuCache,
}
impl CpuState {
    fn release(&mut self) {
        if self.reader.is_some() {
            super::diagnostics::info("cpu_temperature_idle collector=released");
        }
        self.last_status = None;
        self.reader = None;
        self.cache.release();
    }
    fn read(&mut self) -> Reply {
        let reply = self.cache.read(Instant::now(), || {
            if self.reader.is_none() {
                self.reader = Some(cpu::Reader::new()?);
            }
            let result = self.reader.as_mut().expect("CPU reader initialized").read();
            if result.is_err() {
                self.reader = None;
            }
            result
        });
        if let Reply::CpuTemperature { status, .. } = &reply {
            if self.last_status != Some(*status) {
                super::diagnostics::info(format!(
                    "cpu_temperature_status previous={:?} current={status:?}",
                    self.last_status
                ));
                self.last_status = Some(*status);
            }
        }
        reply
    }
}
async fn idle(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending::<()>().await,
    }
}
async fn serve(mut pipe: NamedPipeServer, mut stop: watch::Receiver<bool>) -> io::Result<()> {
    let cpu = Rc::new(RefCell::new(CpuState::default()));
    let mut peers = tokio::task::JoinSet::new();
    loop {
        let deadline = cpu.borrow().cache.idle;
        tokio::select! {
            _ = stop.changed() => break,
            _ = idle(deadline) => { cpu.borrow_mut().release(); },
            _ = peers.join_next(), if !peers.is_empty() => {},
            connected = pipe.connect(), if peers.len() < 8 => {
                connected?;
                let next = create_pipe(false)?;
                let mut connected = std::mem::replace(&mut pipe, next);
                let cpu = cpu.clone();
                // Keep one listener and at most eight bounded peers. A slow
                // client cannot serialize unrelated clients behind its frame.
                peers.spawn_local(async move {
                    let _ = tokio::time::timeout(Duration::from_millis(750), exchange(&mut connected, &cpu)).await;
                });
            }
        }
    }
    peers.abort_all();
    while peers.join_next().await.is_some() {}
    Ok(())
}
async fn exchange(pipe: &mut NamedPipeServer, cpu: &RefCell<CpuState>) -> io::Result<()> {
    let length = pipe.read_u32_le().await? as usize;
    protocol::frame_length(length)?;
    let mut body = vec![0; length];
    pipe.read_exact(&mut body).await?;
    let request: Request = serde_json::from_slice(&body).map_err(io::Error::other)?;
    request.validate()?;
    let reply = match request.operation {
        Operation::Health => Reply::Health {
            service_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![Capability::CpuTemperature],
            cpu_temperature: if !cpu::supported() {
                CapabilityStatus::UnsupportedHardware
            } else if !super::deployment::driver_available() {
                CapabilityStatus::DriverMissing
            } else {
                CapabilityStatus::Ready
            },
        },
        Operation::CpuTemperature => cpu.borrow_mut().read(),
    };
    pipe.write_all(&protocol::encode(&Response::new(&request, reply))?)
        .await?;
    Ok(())
}
