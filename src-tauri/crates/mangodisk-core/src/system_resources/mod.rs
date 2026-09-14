//! System resource snapshots and explicit memory reclamation, independent of desktop UI.
pub mod cpu;
pub mod disk;
pub mod disk_io;
mod memory;
pub mod metrics;
pub mod models;
pub mod network;
pub mod readings;
pub mod release;
pub mod service;
