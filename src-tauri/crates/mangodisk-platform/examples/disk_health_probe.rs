//! Read-only engineering evidence for ordinary-user storage and network capability.
use mangodisk_platform::system_resources::{disk, disk_health, network::NetworkReader};
use std::time::Instant;

fn main() {
    for volume in disk::list().expect("local volume catalogue") {
        let start = Instant::now();
        let result = disk_health::read(&volume);
        let outcome = match result {
            Ok(value) => serde_json::to_value(value).expect("serializable disk health"),
            Err(error) => {
                serde_json::json!({"unavailable":format!("{:?}",error.code()),"diagnostic":error.diagnostic()})
            }
        };
        println!(
            "{}",
            serde_json::json!({"volume":volume.name,"fileSystem":volume.file_system,"health":outcome,"elapsedUs":start.elapsed().as_micros()})
        );
    }
    let mut reader = NetworkReader::default();
    for sample in 0..3 {
        let start = Instant::now();
        let rows = reader.read().expect("native interface counters");
        println!(
            "{}",
            serde_json::json!({"sample":sample,"networkElapsedUs":start.elapsed().as_micros(),"links":rows.iter().filter(|row| row.interface.physical && row.interface.connected).map(|row| serde_json::json!({"name":row.interface.name,"receiveLinkBps":row.counters.and_then(|c|c.receive_link_bps),"transmitLinkBps":row.counters.and_then(|c|c.transmit_link_bps)})).collect::<Vec<_>>()})
        );
    }
}
