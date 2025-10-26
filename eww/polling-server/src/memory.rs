/// Memory metrics collection
use crate::MemoryEntry;

/// Parse memory statistics from /proc/meminfo.
pub fn collect_memory(data: &[u8]) -> Option<MemoryEntry> {
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
    
    if total_kib == 0 || found_both != 3 {
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

/// Parse a number from a /proc line (e.g., "MemTotal:  16304284 kB" -> 16304284)
fn parse_number_from_line(line: &[u8]) -> u64 {
    let mut num = 0u64;
    let mut in_num = false;
    
    for &byte in line {
        if byte.is_ascii_digit() {
            num = num.wrapping_mul(10).wrapping_add((byte - b'0') as u64);
            in_num = true;
        } else if in_num {
            return num;
        }
    }
    
    num
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number_from_line_valid() {
        let line = b"MemTotal:       16304284 kB";
        assert_eq!(parse_number_from_line(line), 16304284);
    }

    #[test]
    fn test_parse_number_from_line_with_multiple_spaces() {
        let line = b"MemAvailable:    8967832 kB";
        assert_eq!(parse_number_from_line(line), 8967832);
    }

    #[test]
    fn test_parse_number_from_line_no_number() {
        let line = b"SomeLabel:";
        assert_eq!(parse_number_from_line(line), 0);
    }

    #[test]
    fn test_parse_number_from_line_zero() {
        let line = b"Value: 0 kB";
        assert_eq!(parse_number_from_line(line), 0);
    }

    #[test]
    fn test_collect_memory_valid() {
        let data = b"MemTotal:       16304284 kB\nMemFree:         8123456 kB\nMemAvailable:    8967832 kB";
        let result = collect_memory(data);
        
        assert!(result.is_some());
        let mem = result.unwrap();
        assert_eq!(mem.total_kib, 16304284);
        assert_eq!(mem.available_kib, 8967832);
        let used_kib = 16304284 - 8967832;
        let expected_percent = (used_kib as f64 * 100.0) / 16304284.0;
        assert!((mem.used_percent - expected_percent).abs() < 0.1);
    }

    #[test]
    fn test_collect_memory_missing_total() {
        let data = b"MemAvailable:    8967832 kB";
        assert!(collect_memory(data).is_none());
    }

    #[test]
    fn test_collect_memory_missing_available() {
        let data = b"MemTotal:       16304284 kB";
        assert!(collect_memory(data).is_none());
    }

    #[test]
    fn test_collect_memory_available_greater_than_total() {
        // Edge case: should still compute used_percent
        let data = b"MemTotal:       10000 kB\nMemAvailable:    20000 kB";
        let result = collect_memory(data);
        
        assert!(result.is_some());
        let mem = result.unwrap();
        assert_eq!(mem.total_kib, 10000);
        assert_eq!(mem.available_kib, 20000);
        // used_kib will be 0 due to saturating_sub
        assert_eq!(mem.used_percent, 0.0);
    }

    #[test]
    fn test_collect_memory_all_used() {
        let data = b"MemTotal:       10000 kB\nMemAvailable:    0 kB";
        let result = collect_memory(data);
        
        assert!(result.is_some());
        let mem = result.unwrap();
        assert_eq!(mem.used_percent, 100.0);
    }
}
