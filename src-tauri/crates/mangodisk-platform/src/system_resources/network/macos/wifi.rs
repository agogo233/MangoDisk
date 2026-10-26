//! Read only non-location Wi-Fi metadata, bounded to a ten-second worker cache.
use super::super::{InterfaceSample, WifiDetails};
use objc2::rc::autoreleasepool;
use objc2_core_wlan::{CWChannelBand, CWChannelWidth, CWPHYMode, CWWiFiClient};
use objc2_foundation::NSString;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

type Metadata = (Option<u64>, WifiDetails);
#[derive(Default)]
pub(super) struct Reader {
    cached: HashMap<String, (bool, Instant, Option<Metadata>)>,
}
impl Reader {
    pub(super) fn read(&mut self, name: &str, connected: bool) -> Option<Metadata> {
        if let Some((before, at, value)) = self.cached.get(name) {
            if *before == connected && at.elapsed() < Duration::from_secs(10) {
                return value.clone();
            }
        }
        let value = connected.then(|| read(name)).flatten();
        self.cached
            .insert(name.into(), (connected, Instant::now(), value.clone()));
        value
    }
    pub(super) fn retain(&mut self, samples: &[InterfaceSample]) {
        self.cached
            .retain(|name, _| samples.iter().any(|sample| sample.interface.id == *name));
    }
}
fn bits_per_second(mbps: f64) -> Option<u64> {
    (mbps.is_finite() && mbps > 0.0 && mbps < u64::MAX as f64 / 1_000_000.0)
        .then(|| (mbps * 1_000_000.0).round() as u64)
        .filter(|rate| *rate > 0)
}
fn dbm(value: isize) -> Option<i16> {
    (-127..=-1).contains(&value).then_some(value as i16)
}
fn read(name: &str) -> Option<Metadata> {
    autoreleasepool(|_| {
        // CoreWLAN objects stay on the sampling worker. SSID/BSSID, scans and
        // location authorization are deliberately outside this read-only path.
        let interface = unsafe {
            CWWiFiClient::sharedWiFiClient().interfaceWithName(Some(&NSString::from_str(name)))
        }?;
        unsafe {
            let rate = bits_per_second(interface.transmitRate());
            let standard = match interface.activePHYMode() {
                mode if mode == CWPHYMode::Mode11a => Some("802.11a"),
                mode if mode == CWPHYMode::Mode11b => Some("802.11b"),
                mode if mode == CWPHYMode::Mode11g => Some("802.11g"),
                mode if mode == CWPHYMode::Mode11n => Some("802.11n"),
                mode if mode == CWPHYMode::Mode11ac => Some("802.11ac"),
                mode if mode == CWPHYMode::Mode11ax => Some("802.11ax"),
                _ => None,
            }
            .map(str::to_owned);
            let channel = interface.wlanChannel();
            let details = WifiDetails {
                standard,
                channel: channel
                    .as_ref()
                    .and_then(|value| u16::try_from(value.channelNumber()).ok())
                    .filter(|value| *value > 0),
                band_ghz: channel
                    .as_ref()
                    .and_then(|value| match value.channelBand() {
                        band if band == CWChannelBand::Band2GHz => Some(2.4),
                        band if band == CWChannelBand::Band5GHz => Some(5.0),
                        band if band == CWChannelBand::Band6GHz => Some(6.0),
                        _ => None,
                    }),
                channel_width_mhz: channel
                    .as_ref()
                    .and_then(|value| match value.channelWidth() {
                        width if width == CWChannelWidth::Width20MHz => Some(20),
                        width if width == CWChannelWidth::Width40MHz => Some(40),
                        width if width == CWChannelWidth::Width80MHz => Some(80),
                        width if width == CWChannelWidth::Width160MHz => Some(160),
                        _ => None,
                    }),
                signal_dbm: dbm(interface.rssiValue()),
                noise_dbm: dbm(interface.noiseMeasurement()),
            };
            Some((rate, details))
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warm_reads_reuse_metadata_and_disconnects_drop_the_previous_link() {
        let mut reader = Reader::default();
        reader.cached.insert(
            "test-interface".into(),
            (
                true,
                Instant::now(),
                Some((Some(2_401_000_000), WifiDetails::default())),
            ),
        );
        assert_eq!(
            reader.read("test-interface", true).unwrap().0,
            Some(2_401_000_000)
        );
        assert!(reader.read("test-interface", false).is_none());
        assert!(reader.cached["test-interface"].2.is_none());
        reader.retain(&[]);
        assert!(reader.cached.is_empty());
    }
    #[test]
    fn rate_conversion_uses_decimal_megabits_and_rejects_missing_or_overflow_values() {
        assert_eq!(bits_per_second(2401.0), Some(2_401_000_000));
        assert_eq!(bits_per_second(866.7), Some(866_700_000));
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY, u64::MAX as f64] {
            assert_eq!(bits_per_second(rate), None);
        }
        assert_eq!(dbm(0), None);
        assert_eq!(dbm(-200), None);
        assert_eq!(dbm(-41), Some(-41));
    }
}
