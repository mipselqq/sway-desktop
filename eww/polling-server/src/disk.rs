/// Disk I/O metrics collection
use std::collections::HashMap;
use crate::{DiskCounters, DiskEntry, DISK_SECTOR_SIZE, constants::RATE_DECAY_TIME_SECS};

/// State tracking for disk device rate limiting and validity
#[derive(Clone, Copy)]
pub struct DiskDeviceState {
    /// Maximum rate seen so far
    pub max_rate: f64,
    /// Time since last rate >= max_rate / 2
    pub time_below_half_max: f64,
    /// Whether this device has ever had non-zero I/O
    pub has_had_io: bool,
}

impl DiskDeviceState {
    /// Create new device state with initial max rate of 1.0
    pub fn new() -> Self {
        DiskDeviceState {
            max_rate: 1.0,
            time_below_half_max: 0.0,
            has_had_io: false,
        }
    }
}

/// Check if device name should be skipped (partitions and pseudo-devices)
pub fn should_skip_device(name: &str) -> bool {
    // Skip pseudo-devices
    if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("dm-") {
        return true;
    }
    
    let last_char = name.chars().last().unwrap_or(' ');
    
    // If doesn't end with digit, it's a base device - keep it
    if !last_char.is_ascii_digit() {
        return false;
    }
    
    // Ends with digit - check if it's a partition
    // NVME partitions: nvme0n1p1 (has 'p' followed by digits)
    if let Some(p_pos) = name.rfind('p') {
        if p_pos < name.len() - 1 && name[p_pos + 1..].chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    
    // SD/HD/VD partitions: sda1, hdb2, vdc123 (start with sd/hd/vd and end with digit)
    if matches!(name.chars().next(), Some('s') | Some('h') | Some('v')) {
        return true;
    }
    
    false
}

/// Parse disk I/O counters from /proc/diskstats.
/// Returns HashMap of device names to (read_sectors, write_sectors).
pub fn parse_disks(data: &[u8]) -> HashMap<&'static str, (u64, u64)> {
    let mut result: HashMap<&'static str, (u64, u64)> = HashMap::with_capacity(16);
    
    for line in data.split(|&b| b == b'\n') {
        let fields: Vec<&[u8]> = line.split(|&b| b == b' ' || b == b'\t')
            .filter(|f| !f.is_empty())
            .collect();
        
        if fields.len() < 10 {
            continue;
        }
        
        let name_bytes = fields[2];
        let name = match std::str::from_utf8(name_bytes) {
            Ok(n) => n,
            Err(_) => continue,
        };
        
        if should_skip_device(name) {
            continue;
        }
        
        let read_sectors = parse_u64(fields.get(5).copied().unwrap_or(&[]));
        let write_sectors = parse_u64(fields.get(9).copied().unwrap_or(&[]));
        
        let name_static = Box::leak(name.to_string().into_boxed_str());
        result.insert(name_static, (read_sectors, write_sectors));
    }
    
    result
}

/// Parse a u64 from a byte slice
fn parse_u64(bytes: &[u8]) -> u64 {
    let mut num = 0u64;
    for &b in bytes {
        if b.is_ascii_digit() {
            num = num.wrapping_mul(10).wrapping_add((b - b'0') as u64);
        }
    }
    num
}

/// Calculate disk I/O throughput rates and populate entries.
pub fn calculate_disk_rates(
    elapsed: f64,
    parsed: HashMap<&'static str, (u64, u64)>,
    prev: &mut HashMap<&'static str, DiskCounters>,
    max_rates: &mut HashMap<&'static str, DiskDeviceState>,
    entries: &mut Vec<DiskEntry>,
) {
    let elapsed = elapsed.max(1e-8);
    
    for (name, (read_sectors, write_sectors)) in parsed {
        let counters = prev
            .entry(name)
            .or_insert(DiskCounters {
                read: read_sectors,
                write: write_sectors,
            });
        
        let (read_rate, write_rate) = calculate_disk_io_rates(
            read_sectors,
            write_sectors,
            counters.read,
            counters.write,
            elapsed,
        );
        
        counters.read = read_sectors;
        counters.write = write_sectors;
        
        // Update device state
        let state = max_rates.entry(name).or_insert(DiskDeviceState::new());
        let combined_rate = read_rate.max(write_rate);
        
        // Mark as having I/O if rate > 0
        if combined_rate > 0.0 {
            state.has_had_io = true;
        }
        
        // Update maximum and track time below half
        if combined_rate > state.max_rate {
            state.max_rate = combined_rate;
            state.time_below_half_max = 0.0;
        } else if combined_rate < state.max_rate / 2.0 {
            state.time_below_half_max += elapsed;
        } else {
            state.time_below_half_max = 0.0;
        }
        
        // Reset max to half if below half for RATE_DECAY_TIME_SECS
        if state.time_below_half_max >= RATE_DECAY_TIME_SECS {
            state.max_rate /= 2.0;
            state.time_below_half_max = 0.0;
        }
        
        // Only add entry if device has had I/O
        if state.has_had_io {
            entries.push(DiskEntry {
                device: name.to_string(),
                read_level: rate_to_level(read_rate, state.max_rate),
                write_level: rate_to_level(write_rate, state.max_rate),
                read_mib_s: read_rate / 1_048_576.0,
                write_mib_s: write_rate / 1_048_576.0,
            });
        }
    }
}

/// Calculate I/O rates from sector counters
fn calculate_disk_io_rates(
    read_sectors: u64,
    write_sectors: u64,
    prev_read: u64,
    prev_write: u64,
    elapsed: f64,
) -> (f64, f64) {
    let read_rate = if read_sectors >= prev_read {
        (read_sectors - prev_read) as f64 * DISK_SECTOR_SIZE as f64 / elapsed
    } else {
        0.0
    };
    
    let write_rate = if write_sectors >= prev_write {
        (write_sectors - prev_write) as f64 * DISK_SECTOR_SIZE as f64 / elapsed
    } else {
        0.0
    };
    
    (read_rate, write_rate)
}

/// Convert throughput rate to a 0-10 level indicator
fn rate_to_level(rate: f64, reference: f64) -> u8 {
    if rate <= 0.0 || reference <= 0.0 {
        return 0;
    }
    let ratio = (rate / reference).min(1.0);
    let level = (ratio * 10.0).ceil() as u8;
    level.min(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_device_loop() {
        assert!(should_skip_device("loop0"));
        assert!(should_skip_device("loop999"));
    }

    #[test]
    fn test_should_skip_device_ram() {
        assert!(should_skip_device("ram0"));
        assert!(should_skip_device("ram15"));
    }

    #[test]
    fn test_should_skip_device_dm() {
        assert!(should_skip_device("dm-0"));
        assert!(should_skip_device("dm-128"));
    }

    #[test]
    fn test_should_skip_device_nvme_partition() {
        assert!(should_skip_device("nvme0n1p1"));
        assert!(should_skip_device("nvme0n1p2"));
        assert!(should_skip_device("nvme1n1p100"));
    }

    #[test]
    fn test_should_skip_device_nvme_base() {
        assert!(!should_skip_device("nvme0n1"));
        assert!(!should_skip_device("nvme1n1"));
    }

    #[test]
    fn test_should_skip_device_sd_partition() {
        assert!(should_skip_device("sda1"));
        assert!(should_skip_device("sdb12"));
        assert!(should_skip_device("sdc999"));
    }

    #[test]
    fn test_should_skip_device_sd_base() {
        assert!(!should_skip_device("sda"));
        assert!(!should_skip_device("sdb"));
    }

    #[test]
    fn test_should_skip_device_hd_partition() {
        assert!(should_skip_device("hda1"));
        assert!(!should_skip_device("hda"));
    }

    #[test]
    fn test_should_skip_device_vd_partition() {
        assert!(should_skip_device("vda1"));
        assert!(!should_skip_device("vda"));
    }

    #[test]
    fn test_parse_u64_valid() {
        assert_eq!(parse_u64(b"12345"), 12345);
    }

    #[test]
    fn test_parse_u64_with_non_digits() {
        // parse_u64 skips non-digits and continues
        assert_eq!(parse_u64(b"123abc456"), 123456);
    }

    #[test]
    fn test_parse_u64_empty() {
        assert_eq!(parse_u64(b""), 0);
    }

    #[test]
    fn test_parse_u64_zero() {
        assert_eq!(parse_u64(b"0"), 0);
    }

    #[test]
    fn test_calculate_disk_io_rates_no_prev() {
        let (read_rate, write_rate) = calculate_disk_io_rates(1000, 2000, 0, 0, 1.0);
        assert_eq!(read_rate, 1000.0 * DISK_SECTOR_SIZE as f64);
        assert_eq!(write_rate, 2000.0 * DISK_SECTOR_SIZE as f64);
    }

    #[test]
    fn test_calculate_disk_io_rates_with_prev() {
        let (read_rate, write_rate) = calculate_disk_io_rates(2000, 4000, 1000, 2000, 1.0);
        let expected_read = 1000.0 * DISK_SECTOR_SIZE as f64;
        let expected_write = 2000.0 * DISK_SECTOR_SIZE as f64;
        assert_eq!(read_rate, expected_read);
        assert_eq!(write_rate, expected_write);
    }

    #[test]
    fn test_calculate_disk_io_rates_counter_reset() {
        let (read_rate, write_rate) = calculate_disk_io_rates(500, 1000, 1000, 2000, 1.0);
        assert_eq!(read_rate, 0.0);
        assert_eq!(write_rate, 0.0);
    }

    #[test]
    fn test_rate_to_level_zero() {
        assert_eq!(rate_to_level(0.0, 600_000_000.0), 0);
    }

    #[test]
    fn test_rate_to_level_reference() {
        let reference = 600_000_000.0;
        assert_eq!(rate_to_level(reference, reference), 10);
    }

    #[test]
    fn test_rate_to_level_quarter() {
        let reference = 600_000_000.0;
        assert_eq!(rate_to_level(reference / 4.0, reference), 3);
    }

    #[test]
    fn test_rate_to_level_over_reference() {
        let reference = 600_000_000.0;
        assert_eq!(rate_to_level(reference * 10.0, reference), 10);
    }
}
