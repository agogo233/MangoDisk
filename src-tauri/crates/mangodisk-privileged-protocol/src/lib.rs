//! Explicit capabilities shared by ordinary clients and the Windows service.
use serde::{Deserialize, Serialize};
use std::io;

pub const SCHEMA_VERSION: u32 = 1;
pub const SERVICE_NAME: &str = "MangoDiskPrivileged";
pub const PIPE_NAME: &str = r"\\.\pipe\MangoDisk.Privileged.v1";
pub const MAX_FRAME_BYTES: usize = 8 * 1024;
pub const SAMPLE_INTERVAL_MS: u64 = 4_000;
pub const SAMPLE_FRESHNESS_MS: u64 = 10_000;
pub const IDLE_RELEASE_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    CpuTemperature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Health,
    CpuTemperature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityStatus {
    Ready,
    UnsupportedHardware,
    DriverMissing,
    DriverAccessDenied,
    SensorUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub request_id: u64,
    pub operation: Operation,
}

impl Request {
    pub fn new(operation: Operation) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id: 1,
            operation,
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != SCHEMA_VERSION || self.request_id == 0 {
            return Err(invalid("unsupported request schema or identifier"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemperatureReading {
    pub celsius: f64,
    pub sampled_at_ms: u64,
}
impl TemperatureReading {
    pub fn validate(&self, now_ms: u64) -> io::Result<()> {
        if !self.celsius.is_finite()
            || !(0.0..=150.0).contains(&self.celsius)
            || self.celsius == 0.0
            || self.sampled_at_ms > now_ms
            || now_ms - self.sampled_at_ms > SAMPLE_FRESHNESS_MS
        {
            return Err(invalid("invalid or expired CPU temperature"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Reply {
    #[serde(rename_all = "camelCase")]
    Health {
        service_version: String,
        capabilities: Vec<Capability>,
        cpu_temperature: CapabilityStatus,
    },
    #[serde(rename_all = "camelCase")]
    CpuTemperature {
        status: CapabilityStatus,
        reading: Option<TemperatureReading>,
    },
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub schema_version: u32,
    pub request_id: u64,
    pub reply: Reply,
}
impl Response {
    pub fn new(request: &Request, reply: Reply) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id: request.request_id,
            reply,
        }
    }
    pub fn validate(&self, request: &Request) -> io::Result<()> {
        request.validate()?;
        if self.schema_version != SCHEMA_VERSION || self.request_id != request.request_id {
            return Err(invalid("unexpected response schema or identifier"));
        }
        match (&request.operation, &self.reply) {
            (Operation::Health, Reply::Health { .. }) => Ok(()),
            (Operation::CpuTemperature, Reply::CpuTemperature { status, reading })
                if (*status == CapabilityStatus::Ready) == reading.is_some() =>
            {
                Ok(())
            }
            _ => Err(invalid("unexpected capability response")),
        }
    }
}

pub fn encode<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|error| invalid(&error.to_string()))?;
    let length = frame_length(body.len())?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

pub fn frame_length(length: usize) -> io::Result<u32> {
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(invalid("privileged IPC frame exceeds bounds"));
    }
    Ok(length as u32)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Intel's package DTS reports a distance below TjMax, not absolute Celsius.
pub fn intel_package_celsius(target: u64, thermal_status: u64) -> Option<f64> {
    let tj_max = (target >> 16) & 0xff;
    let delta = (thermal_status >> 16) & 0x7f;
    if !(50..=125).contains(&tj_max) || thermal_status & (1 << 31) == 0 || delta >= tj_max {
        return None;
    }
    Some((tj_max - delta) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_reject_unknown_capabilities_paths_and_protocols() {
        for text in [
            r#"{"schemaVersion":1,"requestId":1,"operation":"execute"}"#,
            r#"{"schemaVersion":1,"requestId":1,"operation":"cpuTemperature","msr":412}"#,
            r#"{"schemaVersion":1,"requestId":1,"operation":"health","path":"file"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(text).is_err());
        }
        for version in [0, 2, u32::MAX] {
            let mut request = Request::new(Operation::Health);
            request.schema_version = version;
            assert!(request.validate().is_err());
        }
        let mut request = Request::new(Operation::Health);
        request.request_id = 0;
        assert!(request.validate().is_err());
    }
    #[test]
    fn frames_are_bounded_before_allocation() {
        for length in [0, MAX_FRAME_BYTES + 1, usize::MAX] {
            assert!(frame_length(length).is_err());
        }
        assert_eq!(
            frame_length(MAX_FRAME_BYTES).unwrap(),
            MAX_FRAME_BYTES as u32
        );
        assert!(encode(&"x".repeat(MAX_FRAME_BYTES)).is_err());
    }
    #[test]
    fn responses_reject_mismatched_identifiers_operations_and_versions() {
        let request = Request::new(Operation::CpuTemperature);
        let mut response = Response::new(
            &request,
            Reply::CpuTemperature {
                status: CapabilityStatus::SensorUnavailable,
                reading: None,
            },
        );
        assert!(response.validate(&request).is_ok());
        response.request_id = 2;
        assert!(response.validate(&request).is_err());
        response.request_id = 1;
        response.schema_version = 2;
        assert!(response.validate(&request).is_err());
        response.schema_version = 1;
        response.reply = Reply::Rejected;
        assert!(response.validate(&request).is_err());
    }
    #[test]
    fn transport_rejects_invalid_future_and_stale_readings() {
        let mut reading = TemperatureReading {
            celsius: 45.0,
            sampled_at_ms: 20_000,
        };
        assert!(reading.validate(20_000).is_ok());
        assert!(reading.validate(30_000).is_ok());
        assert!(reading.validate(30_001).is_err());
        assert!(reading.validate(19_999).is_err());
        for value in [f64::NAN, f64::INFINITY, 0.0, -1.0, 150.1] {
            reading.celsius = value;
            assert!(reading.validate(20_000).is_err());
        }
    }
    #[test]
    fn response_status_and_reading_must_agree() {
        let request = Request::new(Operation::CpuTemperature);
        for (status, reading) in [
            (CapabilityStatus::Ready, None),
            (
                CapabilityStatus::DriverMissing,
                Some(TemperatureReading {
                    celsius: 45.0,
                    sampled_at_ms: 1,
                }),
            ),
        ] {
            assert!(
                Response::new(&request, Reply::CpuTemperature { status, reading })
                    .validate(&request)
                    .is_err()
            );
        }
    }
    #[test]
    fn protocol_casing_and_frame_are_stable() {
        let request = Request::new(Operation::CpuTemperature);
        let frame = encode(&request).unwrap();
        let expected = br#"{"schemaVersion":1,"requestId":1,"operation":"cpuTemperature"}"#;
        assert_eq!(&frame[4..], expected);
        assert_eq!(
            u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize,
            expected.len()
        );
    }
    #[test]
    fn package_dts_checks_validity_target_and_delta_without_guessing() {
        assert_eq!(
            intel_package_celsius(100 << 16, (1 << 31) | (54 << 16)),
            Some(46.0)
        );
        assert_eq!(intel_package_celsius(100 << 16, 54 << 16), None);
        for target in [0, 49, 126, 255] {
            assert_eq!(intel_package_celsius(target << 16, 1 << 31), None);
        }
        for delta in [100, 127] {
            assert_eq!(
                intel_package_celsius(100 << 16, (1 << 31) | (delta << 16)),
                None
            );
        }
        assert_eq!(intel_package_celsius(125 << 16, 1 << 31), Some(125.0));
    }
}
