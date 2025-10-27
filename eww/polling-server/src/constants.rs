/// Path to /proc/stat for CPU metrics
pub const PROC_STAT_PATH: &str = "/proc/stat";

/// Path to /proc/meminfo for memory metrics
pub const MEMINFO_PATH: &str = "/proc/meminfo";

/// Path to /proc/net/dev for network metrics
pub const NET_DEV_PATH: &str = "/proc/net/dev";

/// Path to /proc/diskstats for disk metrics
pub const DISKSTATS_PATH: &str = "/proc/diskstats";

/// Initial capacity for JSON payload buffer
pub const PAYLOAD_CAPACITY: usize = 270;

/// Disk sector size in bytes
pub const DISK_SECTOR_SIZE: u64 = 512;

/// Time in seconds below half maximum before resetting max rate
pub const RATE_DECAY_TIME_SECS: f64 = 10.0;
