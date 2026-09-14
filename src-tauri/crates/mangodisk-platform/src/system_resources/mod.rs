//! Native resource sampling. Each sensor owns its OS handles and refresh policy;
//! product aggregation and desktop scheduling stay in their respective layers.
pub mod cpu;
pub mod disk;
pub mod disk_io;
pub mod memory;
pub mod network;
pub mod release;
