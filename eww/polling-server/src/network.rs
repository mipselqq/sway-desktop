/// Network metrics collection
use std::collections::HashMap;
use crate::{NetCounters, NetworkEntry, constants::RATE_DECAY_TIME_SECS};

/// State tracking for network device rate limiting and validity
#[derive(Clone, Copy)]
pub struct NetworkDeviceState {
    /// Maximum rate seen so far
    pub max_rate: f64,
    /// Time since last rate >= max_rate / 2
    pub time_below_half_max: f64,
    /// Whether this interface has ever had non-zero traffic
    pub has_had_traffic: bool,
}

impl NetworkDeviceState {
    /// Create new device state with initial max rate of 1.0
    pub fn new() -> Self {
        NetworkDeviceState {
            max_rate: 1.0,
            time_below_half_max: 0.0,
            has_had_traffic: false,
        }
    }
}

/// Parse network interface counters from /proc/net/dev.
/// Returns HashMap of interface names to (rx_bytes, tx_bytes).
pub fn parse_network(data: &[u8]) -> HashMap<&'static str, (u64, u64)> {
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
            
            if let Some((iface, counters)) = parse_network_line(line) {
                result.insert(iface, counters);
            }
            
            line_start = i + 1;
        }
    }
    result
}

/// Parse a single network interface line
fn parse_network_line(line: &[u8]) -> Option<(&'static str, (u64, u64))> {
    let colon_pos = line.iter().position(|&b| b == b':')?;
    
    let iface_bytes = &line[..colon_pos];
    let iface = std::str::from_utf8(iface_bytes).ok()?.trim();
    
    if iface.is_empty() || iface.len() > 15 {
        return None;
    }
    
    // Skip certain interfaces
    match iface.chars().next()? {
        'l' if iface == "lo" => return None,
        'd' if iface.starts_with("docker") => return None,
        'v' if iface.starts_with("veth") => return None,
        _ => {}
    }
    
    let (rx_bytes, tx_bytes) = parse_network_counters(&line[colon_pos + 1..]);
    let iface_static = Box::leak(iface.to_string().into_boxed_str());
    
    Some((iface_static, (rx_bytes, tx_bytes)))
}

/// Parse RX and TX byte counters from network line data
fn parse_network_counters(data: &[u8]) -> (u64, u64) {
    let mut rx_bytes: u64 = 0;
    let mut tx_bytes: u64 = 0;
    let mut field = 0;
    let mut num = 0u64;
    let mut in_num = false;
    
    for &b in data {
        if b.is_ascii_digit() {
            num = num.wrapping_mul(10).wrapping_add((b - b'0') as u64);
            in_num = true;
        } else if in_num {
            match field {
                0 => rx_bytes = num,
                8 => {
                    tx_bytes = num;
                    return (rx_bytes, tx_bytes);
                }
                _ => {}
            }
            field += 1;
            num = 0;
            in_num = false;
        }
    }
    
    if in_num && field == 8 {
        tx_bytes = num;
    }
    
    (rx_bytes, tx_bytes)
}

/// Calculate network throughput rates and populate entries.
pub fn calculate_network_rates(
    elapsed: f64,
    parsed: HashMap<&'static str, (u64, u64)>,
    prev: &mut HashMap<&'static str, NetCounters>,
    max_rates: &mut HashMap<&'static str, NetworkDeviceState>,
    entries: &mut Vec<NetworkEntry>,
) {
    let elapsed = elapsed.max(1e-8);
    
    for (iface, (rx_bytes, tx_bytes)) in parsed {
        let counters = prev
            .entry(iface)
            .or_insert(NetCounters { rx: rx_bytes, tx: tx_bytes });
        
        let (rx_rate, tx_rate) = calculate_network_throughput(
            rx_bytes,
            tx_bytes,
            counters.rx,
            counters.tx,
            elapsed,
        );
        
        counters.rx = rx_bytes;
        counters.tx = tx_bytes;
        
        // Update device state
        let state = max_rates.entry(iface).or_insert(NetworkDeviceState::new());
        let combined_rate = rx_rate.max(tx_rate);
        
        // Mark as having traffic if rate > 0
        if combined_rate > 0.0 {
            state.has_had_traffic = true;
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
        
        // Only add entry if interface has had traffic
        if state.has_had_traffic {
            entries.push(NetworkEntry {
                iface: iface.to_string(),
                tx_level: rate_to_level(tx_rate, state.max_rate),
                rx_level: rate_to_level(rx_rate, state.max_rate),
                tx_mib_s: tx_rate / 1_048_576.0,
                rx_mib_s: rx_rate / 1_048_576.0,
            });
        }
    }
}

/// Calculate RX/TX throughput rates from byte counters
fn calculate_network_throughput(
    rx_bytes: u64,
    tx_bytes: u64,
    prev_rx: u64,
    prev_tx: u64,
    elapsed: f64,
) -> (f64, f64) {
    let rx_rate = if rx_bytes >= prev_rx {
        (rx_bytes - prev_rx) as f64 / elapsed
    } else {
        0.0
    };
    
    let tx_rate = if tx_bytes >= prev_tx {
        (tx_bytes - prev_tx) as f64 / elapsed
    } else {
        0.0
    };
    
    (rx_rate, tx_rate)
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
    fn test_parse_network_counters_valid() {
        let data = b" 1234567 1 0 0 0 0 0 0 9876543 1 0 0 0 0 0 0";
        let (rx, tx) = parse_network_counters(data);
        assert_eq!(rx, 1234567);
        assert_eq!(tx, 9876543);
    }

    #[test]
    fn test_parse_network_counters_zeros() {
        let data = b" 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let (rx, tx) = parse_network_counters(data);
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
    }

    #[test]
    fn test_parse_network_counters_empty() {
        let data = b"";
        let (rx, tx) = parse_network_counters(data);
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
    }

    #[test]
    fn test_parse_network_line_valid() {
        let line = b"   eth0: 1234567 1 0 0 0 0 0 0 9876543 1 0 0 0 0 0 0";
        let result = parse_network_line(line);
        assert!(result.is_some());
        let (iface, (rx, tx)) = result.unwrap();
        assert_eq!(iface, "eth0");
        assert_eq!(rx, 1234567);
        assert_eq!(tx, 9876543);
    }

    #[test]
    fn test_parse_network_line_skip_loopback() {
        let line = b"   lo: 100 0 0 0 0 0 0 0 100 0 0 0 0 0 0 0";
        assert!(parse_network_line(line).is_none());
    }

    #[test]
    fn test_parse_network_line_skip_docker() {
        let line = b"   docker0: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0";
        assert!(parse_network_line(line).is_none());
    }

    #[test]
    fn test_parse_network_line_skip_veth() {
        let line = b"   veth123abc: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0";
        assert!(parse_network_line(line).is_none());
    }

    #[test]
    fn test_parse_network_line_no_colon() {
        let line = b"   invalid_line";
        assert!(parse_network_line(line).is_none());
    }

    #[test]
    fn test_calculate_network_throughput_no_prev() {
        let (rx_rate, tx_rate) = calculate_network_throughput(1000, 2000, 0, 0, 1.0);
        assert_eq!(rx_rate, 1000.0);
        assert_eq!(tx_rate, 2000.0);
    }

    #[test]
    fn test_calculate_network_throughput_with_prev() {
        let (rx_rate, tx_rate) = calculate_network_throughput(2000, 4000, 1000, 2000, 1.0);
        assert_eq!(rx_rate, 1000.0);
        assert_eq!(tx_rate, 2000.0);
    }

    #[test]
    fn test_calculate_network_throughput_counter_reset() {
        // Simulate counter reset (counter wrapped around)
        let (rx_rate, tx_rate) = calculate_network_throughput(500, 1000, 1000, 2000, 1.0);
        assert_eq!(rx_rate, 0.0);
        assert_eq!(tx_rate, 0.0);
    }

    #[test]
    fn test_rate_to_level_zero() {
        assert_eq!(rate_to_level(0.0, 125_000_000.0), 0);
    }

    #[test]
    fn test_rate_to_level_reference() {
        let reference = 125_000_000.0;
        let level = rate_to_level(reference, reference);
        assert_eq!(level, 10);
    }

    #[test]
    fn test_rate_to_level_half() {
        let reference = 125_000_000.0;
        let level = rate_to_level(reference / 2.0, reference);
        assert_eq!(level, 5);
    }

    #[test]
    fn test_rate_to_level_over_reference() {
        let reference = 125_000_000.0;
        let level = rate_to_level(reference * 2.0, reference);
        assert_eq!(level, 10);
    }

    #[test]
    fn test_rate_to_level_invalid_reference() {
        assert_eq!(rate_to_level(1000.0, 0.0), 0);
        assert_eq!(rate_to_level(1000.0, -100.0), 0);
    }
}
