use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::{Duration, Instant};

extern crate libc;

/// Poll interval for system metric collection (default 3000ms, configurable via first argument in milliseconds)
fn get_poll_interval() -> Duration {
    let millis = env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u64>().ok())
        .unwrap_or(3000);
    Duration::from_millis(millis)
}
/// Path to /proc/stat for CPU metrics
const PROC_STAT_PATH: &str = "/proc/stat";
/// Path to /proc/meminfo for memory metrics
const MEMINFO_PATH: &str = "/proc/meminfo";
/// Path to /proc/net/dev for network metrics
const NET_DEV_PATH: &str = "/proc/net/dev";
/// Path to /proc/diskstats for disk metrics
const DISKSTATS_PATH: &str = "/proc/diskstats";
/// Initial capacity for JSON payload buffer
const PAYLOAD_CAPACITY: usize = 4096;
/// Reference bandwidth for network level calculation (125 Mbps)
const NET_REF_BPS: f64 = 125_000_000.0;
/// Reference bandwidth for disk level calculation (600 Mbps)
const DISK_REF_BPS: f64 = 600_000_000.0;
/// Disk sector size in bytes
const DISK_SECTOR_SIZE: u64 = 512;
/// Minimum elapsed time to avoid division by zero
const MIN_ELAPSED: f64 = 1e-8;

#[derive(Clone, Copy)]
/// CPU counter values from /proc/stat (user, nice, system, idle, etc.)
struct CpuCounters {
    /// Total ticks (sum of all modes)
    total: u64,
    /// Idle ticks
    idle: u64,
}

#[derive(Clone, Copy)]
/// Network interface counter values
struct NetCounters {
    /// Bytes received
    rx: u64,
    /// Bytes transmitted
    tx: u64,
}

#[derive(Clone, Copy)]
/// Disk counter values
struct DiskCounters {
    /// Bytes read
    read: u64,
    /// Bytes written
    write: u64,
}

/// CPU metric entry for output
struct CpuEntry {
    /// CPU identifier (e.g., "cpu0", "cpu1")
    id: String,
    /// Usage percentage (0-100)
    usage: u32,
}

/// Memory metric entry for output
struct MemoryEntry {
    /// Total memory in KiB
    total_kib: u64,
    /// Available memory in KiB
    available_kib: u64,
    /// Used percentage (0-100.0)
    used_percent: f64,
}

/// Network interface entry for output
struct NetworkEntry {
    /// Interface name
    iface: String,
    /// TX level (0-10)
    tx_level: u8,
    /// RX level (0-10)
    rx_level: u8,
    /// TX rate in MiB/s
    tx_mib_s: f64,
    /// RX rate in MiB/s
    rx_mib_s: f64,
}

/// Disk device entry for output
struct DiskEntry {
    /// Device name
    device: String,
    /// Read level (0-10)
    read_level: u8,
    /// Write level (0-10)
    write_level: u8,
    /// Read rate in MiB/s
    read_mib_s: f64,
    /// Write rate in MiB/s
    write_mib_s: f64,
}

fn main() -> io::Result<()> {
    let poll_interval = get_poll_interval();
    
    // Use Vec instead of HashMap for CPU cores - O(1) lookup instead of O(hash)
    // Max 256 cores, usually ~16. Much faster than String-keyed HashMap
    let mut cpu_prev: Vec<Option<CpuCounters>> = vec![None; 256];
    let mut net_prev: HashMap<&'static str, NetCounters> = HashMap::with_capacity(16);
    let mut disk_prev: HashMap<&'static str, DiskCounters> = HashMap::with_capacity(16);
    let mut payload = String::with_capacity(PAYLOAD_CAPACITY);
    let mut cpu_entries = Vec::with_capacity(256);
    let mut net_entries = Vec::with_capacity(16);
    let mut disk_entries = Vec::with_capacity(16);
    let mut last_instant = Instant::now();
    
    // Pre-allocate read buffers - just enough for actual /proc file sizes
    // /proc/stat: ~5.5KB, /proc/meminfo: ~1.6KB, /proc/net/dev: ~1KB, /proc/diskstats: ~300B
    let mut stat_buf = vec![0u8; 8192];
    let mut meminfo_buf = vec![0u8; 4096];
    let mut net_buf = vec![0u8; 4096];
    let mut disk_buf = vec![0u8; 4096];

    // Open files ONCE at startup, reuse with pread() - avoids repeated open() syscalls
    let stat_file = File::open(PROC_STAT_PATH)?;
    let meminfo_file = File::open(MEMINFO_PATH)?;
    let net_file = File::open(NET_DEV_PATH)?;
    let disk_file = File::open(DISKSTATS_PATH)?;

    let stat_fd = stat_file.as_raw_fd();
    let meminfo_fd = meminfo_file.as_raw_fd();
    let net_fd = net_file.as_raw_fd();
    let disk_fd = disk_file.as_raw_fd();

    loop {
        let loop_start = Instant::now();
        let elapsed = loop_start.duration_since(last_instant).as_secs_f64();
        last_instant = loop_start;

        cpu_entries.clear();
        let stat_len = pread_file(stat_fd, &mut stat_buf)?;
        collect_cpu(&stat_buf[..stat_len], &mut cpu_prev, &mut cpu_entries);
        
        let meminfo_len = pread_file(meminfo_fd, &mut meminfo_buf)?;
        let memory = collect_memory(&meminfo_buf[..meminfo_len]);
        
        net_entries.clear();
        let net_len = pread_file(net_fd, &mut net_buf)?;
        collect_network(elapsed, &net_buf[..net_len], &mut net_prev, &mut net_entries);
        net_entries.sort_by(|a, b| a.iface.cmp(&b.iface));
        
        disk_entries.clear();
        let disk_len = pread_file(disk_fd, &mut disk_buf)?;
        collect_disks(elapsed, &disk_buf[..disk_len], &mut disk_prev, &mut disk_entries);
        disk_entries.sort_by(|a, b| a.device.cmp(&b.device));

        build_payload(&mut payload, &cpu_entries, memory.as_ref(), &net_entries, &disk_entries);

        if let Err(err) = write_payload(&payload) {
            if err.kind() == io::ErrorKind::BrokenPipe {
                break;
            }
            return Err(err);
        }

        let loop_duration = loop_start.elapsed();
        if loop_duration < poll_interval {
            thread::sleep(poll_interval - loop_duration);
        }
    }
    
    Ok(())
}

/// Read file contents using pread64 syscall with no file pointer changes.
/// This avoids repeated open/close syscalls by reusing file descriptors.
///
/// # Arguments
/// * `fd` - Open file descriptor (must be kept open by caller)
/// * `buf` - Buffer to read into (sized appropriately)
///
/// # Returns
/// Number of bytes read, or io::Error on failure
#[inline]
fn pread_file(fd: i32, buf: &mut [u8]) -> io::Result<usize> {
    // Direct libc::pread64 - zero overhead wrapper
    let bytes_read = unsafe {
        libc::pread64(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0)
    };
    
    if bytes_read < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(bytes_read as usize)
    }
}

/// Parse CPU statistics from /proc/stat and calculate usage percentages.
/// Uses Vec-based O(1) storage indexed by CPU number for fast lookups.
#[inline]
fn collect_cpu(
    data: &[u8],
    prev: &mut [Option<CpuCounters>],
    entries: &mut Vec<CpuEntry>,
) {
    let mut line_start = 0;
    
    for (i, &byte) in data.iter().enumerate() {
        if byte == b'\n' || i == data.len() - 1 {
            let end = if byte == b'\n' { i } else { i + 1 };
            let line = &data[line_start..end];
            
            if !line.starts_with(b"cpu") {
                line_start = i + 1;
                continue;
            }
            
            if line.len() < 5 || !line[3].is_ascii_digit() {
                line_start = i + 1;
                continue;
            }
            
            // Extract cpu number - cpu0, cpu1, etc.
            // Fast path: parse as u8 directly
            let mut cpu_idx = 0usize;
            let mut pos = 3;
            while pos < line.len() && line[pos].is_ascii_digit() {
                cpu_idx = cpu_idx * 10 + (line[pos] - b'0') as usize;
                pos += 1;
            }
            
            if cpu_idx >= 256 {
                line_start = i + 1;
                continue;
            }
            
            // Skip to first space
            while pos < line.len() && line[pos] != b' ' && line[pos] != b'\t' {
                pos += 1;
            }
            
            // Parse numbers
            let mut total: u64 = 0;
            let mut idle: u64 = 0;
            let mut field = 0;
            let mut num = 0u64;
            let mut in_num = false;
            
            for &b in &line[pos..] {
                if b.is_ascii_digit() {
                    num = num.wrapping_mul(10).wrapping_add((b - b'0') as u64);
                    in_num = true;
                } else if in_num {
                    total += num;
                    if field == 3 {
                        idle = num;
                    }
                    if field > 8 {
                        break;
                    }
                    field += 1;
                    num = 0;
                    in_num = false;
                }
            }
            
            // O(1) lookup instead of O(hash) HashMap lookup
            let usage = if let Some(prev_sample) = prev[cpu_idx] {
                let total_diff = total.saturating_sub(prev_sample.total);
                if total_diff == 0 {
                    0
                } else {
                    let idle_diff = idle.saturating_sub(prev_sample.idle);
                    let active = total_diff.saturating_sub(idle_diff);
                    (100 * active / total_diff) as u32
                }
            } else {
                0
            };
            
            prev[cpu_idx] = Some(CpuCounters { total, idle });
            
            // Build cpu ID string manually without format! macro overhead
            let mut cpu_id = String::with_capacity(8);
            cpu_id.push_str("cpu");
            itoa_usize(&mut cpu_id, cpu_idx);
            
            entries.push(CpuEntry { id: cpu_id, usage });
            
            line_start = i + 1;
        }
    }
}

/// Parse memory statistics from /proc/meminfo.
#[inline]
fn collect_memory(data: &[u8]) -> Option<MemoryEntry> {
    let mut total_kib = 0u64;
    let mut available_kib = 0u64;
    let mut found_both = 0u8;
    
    let mut line_start = 0;
    for (i, &byte) in data.iter().enumerate() {
        if byte == b'\n' || i == data.len() - 1 {
            let end = if byte == b'\n' { i } else { i + 1 };
            let line = &data[line_start..end];
            
            if line.starts_with(b"MemTotal:") && found_both & 1 == 0 {
                total_kib = parse_number_from_line(line);
                found_both |= 1;
            } else if line.starts_with(b"MemAvailable:") && found_both & 2 == 0 {
                available_kib = parse_number_from_line(line);
                found_both |= 2;
                if found_both == 3 {
                    break;
                }
            }
            
            line_start = i + 1;
        }
    }
    
    if total_kib == 0 {
        return None;
    }
    
    let used_kib = total_kib.saturating_sub(available_kib);
    let used_percent = (used_kib as f64 * 100.0) / total_kib as f64;
    
    Some(MemoryEntry {
        total_kib,
        available_kib,
        used_percent,
    })
}

#[inline]
fn parse_number_from_line(line: &[u8]) -> u64 {
    let mut num = 0u64;
    let mut in_num = false;
    for &byte in line {
        if byte.is_ascii_digit() {
            num = num.wrapping_mul(10).wrapping_add((byte - b'0') as u64);
            in_num = true;
        } else if in_num {
            // Found first number, return it
            return num;
        }
        // Skip non-digits until we find a number
    }
    num
}

/// Parse network interface counters from /proc/net/dev.
/// Returns HashMap of interface names to byte counters.
#[inline]
fn parse_network(data: &[u8]) -> HashMap<&'static str, (u64, u64)> {
    let mut result: HashMap<&'static str, (u64, u64)> = HashMap::with_capacity(16);
    let mut line_start = 0;
    let mut skip_count = 0;
    
    for (i, &byte) in data.iter().enumerate() {
        if byte == b'\n' || i == data.len() - 1 {
            let end = if byte == b'\n' { i } else { i + 1 };
            let line = &data[line_start..end];
            
            if skip_count < 2 {
                skip_count += 1;
                line_start = i + 1;
                continue;
            }
            
            // Find colon
            let colon_pos = match line.iter().position(|&b| b == b':') {
                Some(p) => p,
                None => {
                    line_start = i + 1;
                    continue;
                }
            };
            
            let iface_bytes = &line[..colon_pos];
            let iface = std::str::from_utf8(iface_bytes).unwrap_or("").trim();
            
            if iface.is_empty() || iface.len() > 15 {
                line_start = i + 1;
                continue;
            }
            
            // Skip certain interfaces
            match iface.as_bytes().first() {
                Some(&b'l') if iface == "lo" => {
                    line_start = i + 1;
                    continue;
                },
                Some(&b'd') if iface.starts_with("docker") => {
                    line_start = i + 1;
                    continue;
                },
                Some(&b'v') if iface.starts_with("veth") => {
                    line_start = i + 1;
                    continue;
                },
                _ => {}
            }
            
            // Parse numbers after colon
            let mut rx_bytes: u64 = 0;
            let mut tx_bytes: u64 = 0;
            let mut field = 0;
            let mut num = 0u64;
            let mut in_num = false;
            
            for &b in &line[colon_pos + 1..] {
                if b.is_ascii_digit() {
                    num = num.wrapping_mul(10).wrapping_add((b - b'0') as u64);
                    in_num = true;
                } else if in_num {
                    if field == 0 {
                        rx_bytes = num;
                    } else if field == 8 {
                        tx_bytes = num;
                    }
                    field += 1;
                    num = 0;
                    in_num = false;
                    if field > 8 {
                        break;
                    }
                }
            }
            if in_num && field == 8 {
                tx_bytes = num;
            }
            
            let iface_static = Box::leak(iface.to_string().into_boxed_str());
            result.insert(iface_static, (rx_bytes, tx_bytes));
            
            line_start = i + 1;
        }
    }
    result
}

/// Calculate network throughput rates and populate entries.
/// Requires previous counters for rate calculation.
#[inline]
fn calculate_network_rates(
    elapsed: f64,
    parsed: HashMap<&'static str, (u64, u64)>,
    prev: &mut HashMap<&'static str, NetCounters>,
    entries: &mut Vec<NetworkEntry>,
) {
    let elapsed = elapsed.max(MIN_ELAPSED);
    
    for (iface, (rx_bytes, tx_bytes)) in parsed {
        let counters = prev
            .entry(iface)
            .or_insert(NetCounters { rx: rx_bytes, tx: tx_bytes });
        
        let rx_rate = if rx_bytes >= counters.rx {
            (rx_bytes - counters.rx) as f64 / elapsed
        } else {
            0.0
        };
        let tx_rate = if tx_bytes >= counters.tx {
            (tx_bytes - counters.tx) as f64 / elapsed
        } else {
            0.0
        };
        
        counters.rx = rx_bytes;
        counters.tx = tx_bytes;
        
        entries.push(NetworkEntry {
            iface: iface.to_string(),
            tx_level: rate_to_level(tx_rate, NET_REF_BPS),
            rx_level: rate_to_level(rx_rate, NET_REF_BPS),
            tx_mib_s: tx_rate / 1_048_576.0,
            rx_mib_s: rx_rate / 1_048_576.0,
        });
    }
}

/// Collect network statistics: parse and calculate rates.
/// Wrapper for convenience - calls parse_network and calculate_network_rates.
#[inline]
fn collect_network(
    elapsed: f64,
    data: &[u8],
    prev: &mut HashMap<&'static str, NetCounters>,
    entries: &mut Vec<NetworkEntry>,
) {
    let parsed = parse_network(data);
    calculate_network_rates(elapsed, parsed, prev, entries);
}

/// Parse disk I/O counters from /proc/diskstats.
/// Returns HashMap of device names to (read_sectors, write_sectors).
#[inline]
fn parse_disks(data: &[u8]) -> HashMap<&'static str, (u64, u64)> {
    let mut result: HashMap<&'static str, (u64, u64)> = HashMap::with_capacity(16);
    
    let mut line_start = 0;
    
    for (i, &byte) in data.iter().enumerate() {
        if byte == b'\n' || i == data.len() - 1 {
            let end = if byte == b'\n' { i } else { i + 1 };
            let line = &data[line_start..end];
            
            // Parse fields: skip first two, then name, then fields
            let mut field = 0;
            let mut num = 0u64;
            let mut in_num = false;
            let mut name_start = 0;
            let mut name_len = 0;
            let mut read_sectors: u64 = 0;
            let mut write_sectors: u64 = 0;
            
            for (j, &b) in line.iter().enumerate() {
                if b.is_ascii_digit() {
                    if !in_num && field == 2 {
                        name_start = j;
                    }
                    num = num.wrapping_mul(10).wrapping_add((b - b'0') as u64);
                    in_num = true;
                } else if in_num {
                    match field {
                        2 => {
                            name_len = j - name_start;
                        },
                        5 => read_sectors = num,
                        9 => {
                            write_sectors = num;
                            break;
                        },
                        _ => {}
                    }
                    field += 1;
                    num = 0;
                    in_num = false;
                }
            }
            
            if field < 9 && in_num {
                if field == 9 {
                    write_sectors = num;
                } else if field == 5 {
                    read_sectors = num;
                }
            }
            
            if name_len == 0 {
                line_start = i + 1;
                continue;
            }
            
            let name_bytes = &line[name_start..name_start + name_len];
            let name = std::str::from_utf8(name_bytes).unwrap_or("");
            let name_bytes = name.as_bytes();
            let last_byte = *name_bytes.last().unwrap_or(&0);
            
            // Skip pseudo-devices
            match name.as_bytes().first() {
                Some(&b'l') if name.starts_with("loop") => {
                    line_start = i + 1;
                    continue;
                },
                Some(&b'r') if name.starts_with("ram") => {
                    line_start = i + 1;
                    continue;
                },
                Some(&b'd') if name.starts_with("dm-") => {
                    line_start = i + 1;
                    continue;
                },
                _ => {}
            }
            
            // Skip partitions (ends with digit and contains p or starts with s/h/v)
            if last_byte.is_ascii_digit() && 
               (name.contains('p') || matches!(name.as_bytes().first(), Some(&b's') | Some(&b'h') | Some(&b'v'))) {
                line_start = i + 1;
                continue;
            }
            
            let name_static = Box::leak(name.to_string().into_boxed_str());
            result.insert(name_static, (read_sectors, write_sectors));
            
            line_start = i + 1;
        }
    }
    result
}

/// Calculate disk I/O throughput rates and populate entries.
/// Requires previous counters for rate calculation.
#[inline]
fn calculate_disk_rates(
    elapsed: f64,
    parsed: HashMap<&'static str, (u64, u64)>,
    prev: &mut HashMap<&'static str, DiskCounters>,
    entries: &mut Vec<DiskEntry>,
) {
    let elapsed = elapsed.max(MIN_ELAPSED);
    
    for (name, (read_sectors, write_sectors)) in parsed {
        let counters = prev
            .entry(name)
            .or_insert(DiskCounters {
                read: read_sectors,
                write: write_sectors,
            });
        
        let read_rate = if read_sectors >= counters.read {
            (read_sectors - counters.read) as f64 * DISK_SECTOR_SIZE as f64 / elapsed
        } else {
            0.0
        };
        let write_rate = if write_sectors >= counters.write {
            (write_sectors - counters.write) as f64 * DISK_SECTOR_SIZE as f64 / elapsed
        } else {
            0.0
        };
        
        counters.read = read_sectors;
        counters.write = write_sectors;
        
        entries.push(DiskEntry {
            device: name.to_string(),
            read_level: rate_to_level(read_rate, DISK_REF_BPS),
            write_level: rate_to_level(write_rate, DISK_REF_BPS),
            read_mib_s: read_rate / 1_048_576.0,
            write_mib_s: write_rate / 1_048_576.0,
        });
    }
}

/// Collect disk statistics: parse and calculate rates.
/// Wrapper for convenience - calls parse_disks and calculate_disk_rates.
#[inline]
fn collect_disks(
    elapsed: f64,
    data: &[u8],
    prev: &mut HashMap<&'static str, DiskCounters>,
    entries: &mut Vec<DiskEntry>,
) {
    let parsed = parse_disks(data);
    calculate_disk_rates(elapsed, parsed, prev, entries);
}

// Extreme optimization: inline number-to-string conversions
#[inline]
fn itoa_u8(s: &mut String, mut n: u8) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [b'0'; 3];
    let mut i = 3;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10);
        n /= 10;
    }
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..]) });
}

#[inline]
fn itoa_usize(s: &mut String, mut n: usize) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [b'0'; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..]) });
}

#[inline]
fn itoa_u32(s: &mut String, mut n: u32) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [b'0'; 10];
    let mut i = 10;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..]) });
}

#[inline]
fn itoa_u64(s: &mut String, mut n: u64) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [b'0'; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[i..]) });
}

#[inline]
fn ftoa_f64(s: &mut String, mut n: f64, prec: usize) {
    if n < 0.0 {
        s.push('-');
        n = -n;
    }
    let int_part = n as u64;
    itoa_u64(s, int_part);
    s.push('.');
    let mut frac = n - int_part as f64;
    for _ in 0..prec {
        frac *= 10.0;
        let digit = frac as u8;
        s.push((b'0' + digit) as char);
        frac -= digit as f64;
    }
}

/// Build JSON payload from collected metrics using optimized number formatting.
/// Avoids format! macro overhead by using inlined itoa_* and ftoa_* functions.
#[inline]
fn build_payload(
    out: &mut String,
    cpu: &[CpuEntry],
    memory: Option<&MemoryEntry>,
    network: &[NetworkEntry],
    disks: &[DiskEntry],
) {
    out.clear();
    out.reserve(PAYLOAD_CAPACITY);
    
    // Extreme optimization: pre-write static strings, use itoa for numbers
    out.push_str("{\"c\":[");
    for (idx, entry) in cpu.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("[\"");
        out.push_str(&entry.id);
        out.push_str("\",");
        itoa_u32(out, entry.usage);
        out.push(']');
    }
    out.push_str("],\"m\":");
    if let Some(mem) = memory {
        out.push('[');
        itoa_u64(out, mem.total_kib);
        out.push(',');
        itoa_u64(out, mem.available_kib);
        out.push(',');
        ftoa_f64(out, mem.used_percent, 1);
        out.push(']');
    } else {
        out.push_str("null");
    }

    out.push_str(",\"n\":[");
    for (idx, entry) in network.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("[\"");
        out.push_str(&entry.iface);
        out.push_str("\",");
        itoa_u8(out, entry.tx_level);
        out.push(',');
        itoa_u8(out, entry.rx_level);
        out.push(',');
        ftoa_f64(out, entry.tx_mib_s, 2);
        out.push(',');
        ftoa_f64(out, entry.rx_mib_s, 2);
        out.push(']');
    }
    out.push_str("],\"d\":[");
    for (idx, entry) in disks.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("[\"");
        out.push_str(&entry.device);
        out.push_str("\",");
        itoa_u8(out, entry.read_level);
        out.push(',');
        itoa_u8(out, entry.write_level);
        out.push(',');
        ftoa_f64(out, entry.read_mib_s, 2);
        out.push(',');
        ftoa_f64(out, entry.write_mib_s, 2);
        out.push(']');
    }
    out.push_str("]}");
}

/// Convert throughput rate to a 0-10 level indicator relative to reference.
#[inline]
fn rate_to_level(rate: f64, reference: f64) -> u8 {
    if rate <= 0.0 || reference <= 0.0 {
        return 0;
    }
    let ratio = (rate / reference).min(1.0);
    let level = (ratio * 10.0).ceil() as u8;
    level.min(10)
}

/// Write JSON payload to stdout with newline.
#[inline]
fn write_payload(payload: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    stdout.write_all(payload.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
