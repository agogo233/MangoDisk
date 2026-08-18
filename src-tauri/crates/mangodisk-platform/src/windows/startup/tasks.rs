use std::path::PathBuf;
use std::time::Instant;

use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IAction, IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TaskScheduler,
    TASK_ACTION_EXEC, TASK_ENUM_HIDDEN, TASK_STATE_RUNNING, TASK_TRIGGER_BOOT, TASK_TRIGGER_DAILY,
    TASK_TRIGGER_EVENT, TASK_TRIGGER_LOGON, TASK_TRIGGER_MONTHLY, TASK_TRIGGER_MONTHLYDOW,
    TASK_TRIGGER_TIME, TASK_TRIGGER_TYPE2, TASK_TRIGGER_WEEKLY,
};
use windows::Win32::System::Variant::VARIANT;
use windows_core::{Interface, BSTR};

use crate::{
    PlatformCancellation, PlatformError, PlatformErrorCode, PlatformResult,
    PlatformStartupArtifact, PlatformStartupChangeRequest, PlatformStartupChangeResult,
    PlatformStartupConfiguredState, PlatformStartupControlCapability,
    PlatformStartupCoverageReason, PlatformStartupCoverageStatus, PlatformStartupDesiredState,
    PlatformStartupDiagnosticCode, PlatformStartupIdentityConfidence, PlatformStartupOwner,
    PlatformStartupRuntimeState, PlatformStartupScope, PlatformStartupSourceKind,
    PlatformStartupSourceResult, PlatformStartupSummarySource, PlatformStartupTarget,
    PlatformStartupTargetKind, PlatformStartupTrigger,
};

use super::metadata::{file_version_metadata, startup_trust};
use super::registry::{
    expand_environment_variables, normalized_path, split_command_line, target_kind,
};

struct ComGuard;

impl ComGuard {
    fn initialize() -> windows_core::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub(super) fn scan(cancellation: &PlatformCancellation) -> PlatformStartupSourceResult {
    let started = Instant::now();
    let Ok(_com) = ComGuard::initialize() else {
        return unavailable(started, PlatformStartupCoverageReason::ApiUnavailable);
    };
    let scheduler: ITaskService =
        match unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) } {
            Ok(scheduler) => scheduler,
            Err(error) => return unavailable(started, coverage_reason(&error)),
        };
    let empty = VARIANT::default();
    if let Err(error) = unsafe { scheduler.Connect(&empty, &empty, &empty, &empty) } {
        return unavailable(started, coverage_reason(&error));
    }
    let root = match unsafe { scheduler.GetFolder(&BSTR::from("\\")) } {
        Ok(root) => root,
        Err(error) => return unavailable(started, coverage_reason(&error)),
    };
    let mut items = Vec::new();
    let mut partial_reason = None;
    visit_folder(&root, cancellation, &mut items, &mut partial_reason);
    if cancellation.is_cancelled() {
        return result(
            items,
            PlatformStartupCoverageStatus::Cancelled,
            Some(PlatformStartupCoverageReason::Cancelled),
            started,
        );
    }
    result(
        items,
        if partial_reason.is_some() {
            PlatformStartupCoverageStatus::Partial
        } else {
            PlatformStartupCoverageStatus::Complete
        },
        partial_reason,
        started,
    )
}

pub(super) fn change(
    request: &PlatformStartupChangeRequest,
) -> PlatformResult<PlatformStartupChangeResult> {
    let _com = ComGuard::initialize().map_err(task_platform_error)?;
    let scheduler: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
            .map_err(task_platform_error)?;
    let empty = VARIANT::default();
    unsafe { scheduler.Connect(&empty, &empty, &empty, &empty) }.map_err(task_platform_error)?;
    let root = unsafe { scheduler.GetFolder(&BSTR::from("\\")) }.map_err(task_platform_error)?;
    let (task, current) = find_task(&root, &request.provider_item_id)?
        .ok_or_else(|| PlatformError::item_changed("scheduled task no longer exists"))?;
    if current != request.expected_artifact {
        return Err(PlatformError::item_changed(
            "scheduled task changed after preflight",
        ));
    }
    if current.control_capability != PlatformStartupControlCapability::Toggleable {
        return Err(PlatformError::new(
            PlatformErrorCode::Unsupported,
            "scheduled task is not available for current-user control",
        ));
    }
    if request.desired_state == PlatformStartupDesiredState::Removed {
        return Err(PlatformError::new(
            PlatformErrorCode::Unsupported,
            "scheduled tasks cannot be removed by startup management",
        ));
    }
    let desired_enabled = request.desired_state == PlatformStartupDesiredState::Enabled;
    if (current.configured_state == PlatformStartupConfiguredState::Enabled) != desired_enabled {
        unsafe { task.SetEnabled(desired_enabled.into()) }.map_err(task_platform_error)?;
    }
    let verified = inspect_task(&task)
        .map_err(|reason| {
            PlatformError::new(
                PlatformErrorCode::OperationFailed,
                format!("verify scheduled task: {reason:?}"),
            )
        })?
        .ok_or_else(|| PlatformError::item_changed("scheduled task trigger changed"))?;
    let desired = if desired_enabled {
        PlatformStartupConfiguredState::Enabled
    } else {
        PlatformStartupConfiguredState::Disabled
    };
    Ok(PlatformStartupChangeResult {
        previous_state: current.configured_state,
        configured_state: verified.configured_state,
        verified: verified.configured_state == desired,
    })
}

fn find_task(
    folder: &ITaskFolder,
    provider_item_id: &str,
) -> PlatformResult<Option<(IRegisteredTask, PlatformStartupArtifact)>> {
    let tasks = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) }.map_err(task_platform_error)?;
    let count = unsafe { tasks.Count() }.map_err(task_platform_error)?;
    for index in 1..=count {
        let task = unsafe { tasks.get_Item(&VARIANT::from(index)) }.map_err(task_platform_error)?;
        if let Some(artifact) = inspect_task(&task).map_err(|reason| {
            PlatformError::new(
                PlatformErrorCode::OperationFailed,
                format!("inspect scheduled task: {reason:?}"),
            )
        })? {
            if artifact.provider_item_id == provider_item_id {
                return Ok(Some((task, artifact)));
            }
        }
    }
    let folders = unsafe { folder.GetFolders(0) }.map_err(task_platform_error)?;
    let count = unsafe { folders.Count() }.map_err(task_platform_error)?;
    for index in 1..=count {
        let child =
            unsafe { folders.get_Item(&VARIANT::from(index)) }.map_err(task_platform_error)?;
        if let Some(found) = find_task(&child, provider_item_id)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn task_platform_error(error: windows_core::Error) -> PlatformError {
    let code = if error.code().0 as u32 == 0x8007_0005 {
        PlatformErrorCode::AccessDenied
    } else {
        PlatformErrorCode::OperationFailed
    };
    PlatformError::new(code, format!("task scheduler operation failed: {error}"))
}

fn visit_folder(
    folder: &ITaskFolder,
    cancellation: &PlatformCancellation,
    items: &mut Vec<PlatformStartupArtifact>,
    partial_reason: &mut Option<PlatformStartupCoverageReason>,
) {
    if cancellation.is_cancelled() {
        return;
    }
    match unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) } {
        Ok(tasks) => {
            let count = unsafe { tasks.Count() }.unwrap_or_default();
            for index in 1..=count {
                if cancellation.is_cancelled() {
                    return;
                }
                let task = match unsafe { tasks.get_Item(&VARIANT::from(index)) } {
                    Ok(task) => task,
                    Err(error) => {
                        partial_reason.get_or_insert(coverage_reason(&error));
                        continue;
                    }
                };
                match inspect_task(&task) {
                    Ok(Some(item)) => items.push(item),
                    Ok(None) => {}
                    Err(reason) => {
                        partial_reason.get_or_insert(reason);
                    }
                }
            }
        }
        Err(error) => {
            partial_reason.get_or_insert(coverage_reason(&error));
        }
    }
    match unsafe { folder.GetFolders(0) } {
        Ok(folders) => {
            let count = unsafe { folders.Count() }.unwrap_or_default();
            for index in 1..=count {
                if cancellation.is_cancelled() {
                    return;
                }
                match unsafe { folders.get_Item(&VARIANT::from(index)) } {
                    Ok(child) => visit_folder(&child, cancellation, items, partial_reason),
                    Err(error) => {
                        partial_reason.get_or_insert(coverage_reason(&error));
                    }
                }
            }
        }
        Err(error) => {
            partial_reason.get_or_insert(coverage_reason(&error));
        }
    }
}

fn inspect_task(
    task: &IRegisteredTask,
) -> Result<Option<PlatformStartupArtifact>, PlatformStartupCoverageReason> {
    let definition = unsafe { task.Definition() }.map_err(|error| coverage_reason(&error))?;
    let triggers = task_triggers(&definition)?;
    if !triggers.iter().any(|trigger| {
        matches!(
            trigger,
            PlatformStartupTrigger::Boot | PlatformStartupTrigger::UserLogon
        )
    }) {
        return Ok(None);
    }
    let task_path = unsafe { task.Path() }
        .map(|value| value.to_string())
        .map_err(|error| coverage_reason(&error))?;
    let task_name = unsafe { task.Name() }
        .map(|value| value.to_string())
        .unwrap_or_else(|_| task_path.clone());
    let enabled = unsafe { task.Enabled() }.map(bool::from).unwrap_or(false);
    let running = unsafe { task.State() }
        .map(|state| state == TASK_STATE_RUNNING)
        .unwrap_or(false);
    let (target, action_count, unsupported_action) = task_target(&definition, &task_path)?;
    let registration = unsafe { definition.RegistrationInfo() }.ok();
    let description = registration
        .as_ref()
        .and_then(|info| output_bstr(|value| unsafe { info.Description(value) }));
    let author = registration
        .as_ref()
        .and_then(|info| output_bstr(|value| unsafe { info.Author(value) }));
    let system_item = task_path
        .to_ascii_lowercase()
        .starts_with("\\microsoft\\windows\\");
    let mut diagnostics = Vec::new();
    if unsupported_action || action_count > 1 {
        diagnostics.push(PlatformStartupDiagnosticCode::UnsupportedFormat);
    }
    if target.path.as_deref().is_some_and(|path| !path.exists()) {
        diagnostics.push(PlatformStartupDiagnosticCode::MissingTarget);
    }
    let scope = if triggers.contains(&PlatformStartupTrigger::Boot) {
        PlatformStartupScope::Machine
    } else {
        PlatformStartupScope::User
    };
    let icon_path = target.path.clone().filter(|path| path.exists());
    let trust = startup_trust(target.path.as_deref(), system_item);
    let version_metadata = target
        .path
        .as_deref()
        .and_then(file_version_metadata)
        .unwrap_or_default();
    let owner_identity = target.identity_key.clone();
    let task_description_available = description.is_some();
    let summary = description.or(version_metadata.description);
    Ok(Some(PlatformStartupArtifact {
        provider_item_id: format!("task:{}", task_path.to_ascii_lowercase()),
        source_kind: PlatformStartupSourceKind::ScheduledTask,
        scope,
        triggers,
        display_name: task_name.clone(),
        configuration_path: None,
        target,
        owner: PlatformStartupOwner {
            identity_key: Some(owner_identity),
            name: version_metadata.product_name.or(Some(task_name)),
            publisher: author.or(version_metadata.company_name),
            summary: summary.clone(),
            summary_source: if task_description_available {
                PlatformStartupSummarySource::TaskDescription
            } else if summary.is_some() {
                PlatformStartupSummarySource::VersionInfo
            } else {
                PlatformStartupSummarySource::SourceLabel
            },
            version: version_metadata.product_version,
            icon_path,
            confidence: PlatformStartupIdentityConfidence::Strong,
        },
        configured_state: if enabled {
            PlatformStartupConfiguredState::Enabled
        } else {
            PlatformStartupConfiguredState::Disabled
        },
        runtime_state: if running {
            PlatformStartupRuntimeState::Running
        } else {
            PlatformStartupRuntimeState::Stopped
        },
        control_capability: if system_item {
            PlatformStartupControlCapability::SystemManaged
        } else if scope == PlatformStartupScope::User {
            PlatformStartupControlCapability::Toggleable
        } else {
            PlatformStartupControlCapability::ElevationRequired
        },
        trust,
        modified_at_ms: None,
        diagnostics,
    }))
}

fn task_triggers(
    definition: &windows::Win32::System::TaskScheduler::ITaskDefinition,
) -> Result<Vec<PlatformStartupTrigger>, PlatformStartupCoverageReason> {
    let collection = unsafe { definition.Triggers() }.map_err(|error| coverage_reason(&error))?;
    let mut count = 0;
    unsafe { collection.Count(&mut count) }.map_err(|error| coverage_reason(&error))?;
    let mut triggers = Vec::new();
    for index in 1..=count {
        let trigger =
            unsafe { collection.get_Item(index) }.map_err(|error| coverage_reason(&error))?;
        let mut kind = TASK_TRIGGER_TYPE2::default();
        unsafe { trigger.Type(&mut kind) }.map_err(|error| coverage_reason(&error))?;
        let mapped = if kind == TASK_TRIGGER_BOOT {
            PlatformStartupTrigger::Boot
        } else if kind == TASK_TRIGGER_LOGON {
            PlatformStartupTrigger::UserLogon
        } else if [
            TASK_TRIGGER_TIME,
            TASK_TRIGGER_DAILY,
            TASK_TRIGGER_WEEKLY,
            TASK_TRIGGER_MONTHLY,
            TASK_TRIGGER_MONTHLYDOW,
        ]
        .contains(&kind)
        {
            PlatformStartupTrigger::Scheduled
        } else if kind == TASK_TRIGGER_EVENT {
            PlatformStartupTrigger::Event
        } else {
            PlatformStartupTrigger::Unknown
        };
        if !triggers.contains(&mapped) {
            triggers.push(mapped);
        }
    }
    Ok(triggers)
}

fn task_target(
    definition: &windows::Win32::System::TaskScheduler::ITaskDefinition,
    task_path: &str,
) -> Result<(PlatformStartupTarget, i32, bool), PlatformStartupCoverageReason> {
    let actions = unsafe { definition.Actions() }.map_err(|error| coverage_reason(&error))?;
    let mut count = 0;
    unsafe { actions.Count(&mut count) }.map_err(|error| coverage_reason(&error))?;
    let mut unsupported = false;
    for index in 1..=count {
        let action: IAction =
            unsafe { actions.get_Item(index) }.map_err(|error| coverage_reason(&error))?;
        let mut kind = Default::default();
        unsafe { action.Type(&mut kind) }.map_err(|error| coverage_reason(&error))?;
        if kind != TASK_ACTION_EXEC {
            unsupported = true;
            continue;
        }
        let Ok(exec) = action.cast::<IExecAction>() else {
            unsupported = true;
            continue;
        };
        let path = output_bstr(|value| unsafe { exec.Path(value) }).unwrap_or_default();
        let path = PathBuf::from(expand_environment_variables(&path));
        let arguments = output_bstr(|value| unsafe { exec.Arguments(value) })
            .map(|value| split_command_line(&value))
            .unwrap_or_default();
        let identity = normalized_path(&path);
        return Ok((
            PlatformStartupTarget {
                kind: target_kind(Some(&path)),
                identity_key: format!("path:{identity}"),
                path: Some(path.clone()),
                executable_name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned),
                arguments,
            },
            count,
            unsupported,
        ));
    }
    Ok((
        PlatformStartupTarget {
            kind: PlatformStartupTargetKind::Task,
            identity_key: format!("task:{}", task_path.to_ascii_lowercase()),
            path: None,
            executable_name: None,
            arguments: Vec::new(),
        },
        count,
        true,
    ))
}

fn output_bstr(reader: impl FnOnce(*mut BSTR) -> windows_core::Result<()>) -> Option<String> {
    let mut value = BSTR::new();
    reader(&mut value).ok()?;
    let value = value.to_string();
    (!value.trim().is_empty()).then_some(value)
}

fn coverage_reason(error: &windows_core::Error) -> PlatformStartupCoverageReason {
    if error.code().0 as u32 == 0x8007_0005 {
        PlatformStartupCoverageReason::AccessDenied
    } else {
        PlatformStartupCoverageReason::ApiUnavailable
    }
}

fn unavailable(
    started: Instant,
    reason: PlatformStartupCoverageReason,
) -> PlatformStartupSourceResult {
    result(
        Vec::new(),
        PlatformStartupCoverageStatus::Unavailable,
        Some(reason),
        started,
    )
}

fn result(
    items: Vec<PlatformStartupArtifact>,
    status: PlatformStartupCoverageStatus,
    reason: Option<PlatformStartupCoverageReason>,
    started: Instant,
) -> PlatformStartupSourceResult {
    PlatformStartupSourceResult {
        source_id: "windows.scheduled_tasks".to_string(),
        required: true,
        status,
        reason,
        items,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}
