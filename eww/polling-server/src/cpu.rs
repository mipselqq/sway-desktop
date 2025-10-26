/// CPU metrics collection and parsing
use crate::{CpuCounters, CpuEntry};

/// Parse CPU statistics from /proc/stat and calculate usage percentages.
pub fn collect_cpu(
    data: &[u8],
    prev: &mut [Option<CpuCounters>],
    entries: &mut Vec<CpuEntry>,
) {
    let mut line_start = 0;
    
    for (i, &byte) in data.iter().enumerate() {
        if byte == b'\n' || i == data.len() - 1 {
            let end = if byte == b'\n' { i } else { i + 1 };
            let line = &data[line_start..end];
            
            process_cpu_line(line, prev, entries);
            line_start = i + 1;
        }
    }
}

/// Process a single CPU line from /proc/stat
fn process_cpu_line(line: &[u8], prev: &mut [Option<CpuCounters>], entries: &mut Vec<CpuEntry>) {
    if !line.starts_with(b"cpu") || line.len() < 5 || !line[3].is_ascii_digit() {
        return;
    }
    
    // Extract cpu number - cpu0, cpu1, etc.
    let mut cpu_idx = 0usize;
    let mut pos = 3;
    while pos < line.len() && line[pos].is_ascii_digit() {
        cpu_idx = cpu_idx * 10 + (line[pos] - b'0') as usize;
        pos += 1;
    }
    
    if cpu_idx >= 256 {
        return;
    }
    
    // Skip to first space
    while pos < line.len() && line[pos] != b' ' && line[pos] != b'\t' {
        pos += 1;
    }
    
    // Parse numbers
    let (total, idle) = parse_cpu_counters(&line[pos..]);
    
    // Calculate usage
    let usage = calculate_cpu_usage(prev[cpu_idx], total, idle);
    
    prev[cpu_idx] = Some(CpuCounters { total, idle });
    
    // Build cpu ID string manually without format! macro overhead
    let mut cpu_id = String::with_capacity(8);
    cpu_id.push_str("cpu");
    itoa_usize(&mut cpu_id, cpu_idx);
    
    entries.push(CpuEntry { id: cpu_id, usage });
}

/// Parse total and idle ticks from CPU line
fn parse_cpu_counters(data: &[u8]) -> (u64, u64) {
    let mut total: u64 = 0;
    let mut idle: u64 = 0;
    let mut field = 0;
    let mut num = 0u64;
    let mut in_num = false;
    
    for &b in data {
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
    
    (total, idle)
}

/// Calculate CPU usage percentage
fn calculate_cpu_usage(prev: Option<CpuCounters>, total: u64, idle: u64) -> u32 {
    let prev_sample = match prev {
        Some(p) => p,
        None => return 0,
    };
    
    let total_diff = total.saturating_sub(prev_sample.total);
    if total_diff == 0 {
        return 0;
    }
    
    let idle_diff = idle.saturating_sub(prev_sample.idle);
    let active = total_diff.saturating_sub(idle_diff);
    (100 * active / total_diff) as u32
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu_counters_valid() {
        let data = b" 2255 34 24 22625563 6290 127 456 0 0 0";
        let (total, idle) = parse_cpu_counters(data);
        assert_eq!(idle, 22625563);
        assert!(total > idle);
    }

    #[test]
    fn test_parse_cpu_counters_empty() {
        let data = b"";
        let (total, idle) = parse_cpu_counters(data);
        assert_eq!(total, 0);
        assert_eq!(idle, 0);
    }

    #[test]
    fn test_calculate_cpu_usage_no_previous() {
        let usage = calculate_cpu_usage(None, 1000, 500);
        assert_eq!(usage, 0);
    }

    #[test]
    fn test_calculate_cpu_usage_50_percent() {
        let prev = Some(CpuCounters {
            total: 1000,
            idle: 100,
        });
        let usage = calculate_cpu_usage(prev, 2000, 600);
        assert_eq!(usage, 50);
    }

    #[test]
    fn test_calculate_cpu_usage_100_percent() {
        let prev = Some(CpuCounters {
            total: 1000,
            idle: 100,
        });
        let usage = calculate_cpu_usage(prev, 2000, 100);
        assert_eq!(usage, 100);
    }

    #[test]
    fn test_calculate_cpu_usage_zero_percent() {
        let prev = Some(CpuCounters {
            total: 1000,
            idle: 100,
        });
        let usage = calculate_cpu_usage(prev, 2000, 1100);
        assert_eq!(usage, 0);
    }

    #[test]
    fn test_process_cpu_line_valid() {
        let line = b"cpu0 2255 34 24 22625563 6290 127 456 0 0 0";
        let mut prev = vec![None; 256];
        let mut entries = Vec::new();
        
        process_cpu_line(line, &mut prev, &mut entries);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "cpu0");
        assert_eq!(entries[0].usage, 0);
    }

    #[test]
    fn test_process_cpu_line_invalid_line() {
        let line = b"invalid";
        let mut prev = vec![None; 256];
        let mut entries = Vec::new();
        
        process_cpu_line(line, &mut prev, &mut entries);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_process_cpu_line_cpu_too_high() {
        let line = b"cpu300 2255 34 24 22625563 6290 127 456 0 0 0";
        let mut prev = vec![None; 256];
        let mut entries = Vec::new();
        
        process_cpu_line(line, &mut prev, &mut entries);
        assert_eq!(entries.len(), 0);
    }
}
