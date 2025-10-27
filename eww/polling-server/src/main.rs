use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::{Duration, Instant};

extern crate libc;

mod cpu;
mod memory;
mod network;
mod disk;
mod constants;
mod temperature;

use cpu::collect_cpu;
use memory::collect_memory;
use network::{parse_network, calculate_network_rates, NetworkDeviceState};
use disk::{parse_disks, calculate_disk_rates, DiskDeviceState};
use temperature::collect_temperature;
use constants::*;

/// Collect network statistics: parse and calculate rates.
/// Wrapper for convenience - calls parse_network and calculate_network_rates.
#[inline]
fn collect_network(
    elapsed: f64,
    data: &[u8],
    prev: &mut HashMap<&'static str, NetCounters>,
    max_rates: &mut HashMap<&'static str, NetworkDeviceState>,
    entries: &mut Vec<NetworkEntry>,
) {
    let parsed = parse_network(data);
    calculate_network_rates(elapsed, parsed, prev, max_rates, entries);
}

/// Collect disk statistics: parse and calculate rates.
/// Wrapper for convenience - calls parse_disks and calculate_disk_rates.
#[inline]
fn collect_disks(
    elapsed: f64,
    data: &[u8],
    prev: &mut HashMap<&'static str, DiskCounters>,
    max_rates: &mut HashMap<&'static str, DiskDeviceState>,
    entries: &mut Vec<DiskEntry>,
) {
    let parsed = parse_disks(data);
    calculate_disk_rates(elapsed, parsed, prev, max_rates, entries);
}

/// Poll interval for system metric collection (default 3000ms, configurable via first argument in milliseconds)
fn get_poll_interval() -> Duration {
    let millis = env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u64>().ok())
        .unwrap_or(3000);
    Duration::from_millis(millis)
}

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
    let mut cpu_prev: Vec<Option<CpuCounters>> = vec![None; 64];
    let mut net_prev: HashMap<&'static str, NetCounters> = HashMap::with_capacity(16);
    let mut net_max_rates: HashMap<&'static str, NetworkDeviceState> = HashMap::with_capacity(16);
    let mut disk_prev: HashMap<&'static str, DiskCounters> = HashMap::with_capacity(16);
    let mut disk_max_rates: HashMap<&'static str, DiskDeviceState> = HashMap::with_capacity(16);
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
        collect_network(elapsed, &net_buf[..net_len], &mut net_prev, &mut net_max_rates, &mut net_entries);
        net_entries.sort_by(|a, b| a.iface.cmp(&b.iface));
        
        disk_entries.clear();
        let disk_len = pread_file(disk_fd, &mut disk_buf)?;
        collect_disks(elapsed, &disk_buf[..disk_len], &mut disk_prev, &mut disk_max_rates, &mut disk_entries);
        disk_entries.sort_by(|a, b| a.device.cmp(&b.device));

        let temp = collect_temperature();
        build_payload(&mut payload, &cpu_entries, memory.as_ref(), &net_entries, &disk_entries, temp);

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
    temp: u32,
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
    out.push_str("],\"t\":");
    itoa_u32(out, temp);
    out.push('}');
}

/// Write JSON payload to stdout with newline.
#[inline]
fn write_payload(payload: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    stdout.write_all(payload.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
