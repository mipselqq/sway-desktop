/// Temperature sensor reading in Celsius
use std::fs;

/// Read CPU temperature from thermal zone and return in Celsius (0-100+)
pub fn collect_temperature() -> u32 {
    // Try hwmon0 first (usually the main CPU sensor)
    if let Ok(content) = fs::read_to_string("/sys/class/hwmon/hwmon0/temp2_input") {
        if let Ok(millidegrees) = content.trim().parse::<u32>() {
            return millidegrees / 1000;
        }
    }
    
    if let Ok(content) = fs::read_to_string("/sys/class/hwmon/hwmon0/temp1_input") {
        if let Ok(millidegrees) = content.trim().parse::<u32>() {
            let temp = millidegrees / 1000;
            if temp > 0 {
                return temp;
            }
        }
    }
    
    // Try thermal_zone0
    if let Ok(content) = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        if let Ok(millidegrees) = content.trim().parse::<u32>() {
            let temp = millidegrees / 1000;
            if temp > 0 {
                return temp;
            }
        }
    }
    
    // Try /sys/devices/virtual/thermal/thermal_zone0/temp
    if let Ok(content) = fs::read_to_string("/sys/devices/virtual/thermal/thermal_zone0/temp") {
        if let Ok(millidegrees) = content.trim().parse::<u32>() {
            return millidegrees / 1000;
        }
    }
    
    // Try hwmon1
    if let Ok(content) = fs::read_to_string("/sys/class/hwmon/hwmon1/temp1_input") {
        if let Ok(millidegrees) = content.trim().parse::<u32>() {
            return millidegrees / 1000;
        }
    }
    
    0  // Default to 0 if not found
}
