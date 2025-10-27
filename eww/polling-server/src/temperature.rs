/// Temperature sensor reading in Celsius
use std::fs;
use std::path::{Path, PathBuf};

/// Try to read a temperature file and return value in Celsius
fn read_temp_file(path: &Path) -> Option<u32> {
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(millidegrees) = content.trim().parse::<u32>() {
            let temp = millidegrees / 1000;
            if temp > 0 {
                return Some(temp);
            }
        }
    }
    None
}

/// Get label for a temperature sensor (e.g., "Package id 0", "Core 0", etc.)
fn get_temp_label(hwmon_path: &Path, temp_index: u32) -> Option<String> {
    let label_path = hwmon_path.join(format!("temp{}_label", temp_index));
    fs::read_to_string(label_path).ok().map(|s| s.trim().to_string())
}

/// Check if a label is likely to be CPU package temperature
fn is_package_temp_label(label: &str) -> bool {
    let label_lower = label.to_lowercase();
    label_lower.contains("package")
        || label_lower.contains("tdie")
        || label_lower.contains("soc")
}

/// Check if a label is likely to be any CPU temperature
fn is_cpu_temp_label(label: &str) -> bool {
    let label_lower = label.to_lowercase();
    label_lower.contains("package")
        || label_lower.contains("tdie")
        || label_lower.contains("cpu")
        || label_lower.contains("core")
        || label_lower.contains("soc")
}

/// Read CPU temperature from hwmon devices and return in Celsius (0-100+)
pub fn collect_temperature() -> u32 {
    // First priority: scan /sys/class/hwmon/ for temperature sensors
    // Similar to btop approach - look for Package/Tdie temperature first
    if let Ok(hwmon_entries) = fs::read_dir("/sys/class/hwmon") {
        let mut hwmon_paths: Vec<_> = hwmon_entries.flatten().map(|e| e.path()).collect();
        // Sort to get consistent order
        hwmon_paths.sort();
        
        // First pass: Look for Package/Tdie temperature with labels
        for hwmon_path in &hwmon_paths {
            for temp_idx in 0..20 {
                let temp_input = hwmon_path.join(format!("temp{}_input", temp_idx));
                if temp_input.exists() {
                    if let Some(label) = get_temp_label(hwmon_path, temp_idx) {
                        if is_package_temp_label(&label) {
                            if let Some(temp) = read_temp_file(&temp_input) {
                                return temp;
                            }
                        }
                    }
                }
            }
        }
        
        // Second pass: Look for any CPU-labeled temperature
        for hwmon_path in &hwmon_paths {
            for temp_idx in 0..20 {
                let temp_input = hwmon_path.join(format!("temp{}_input", temp_idx));
                if temp_input.exists() {
                    if let Some(label) = get_temp_label(hwmon_path, temp_idx) {
                        if is_cpu_temp_label(&label) {
                            if let Some(temp) = read_temp_file(&temp_input) {
                                return temp;
                            }
                        }
                    }
                }
            }
        }
        
        // Third pass: Try any temperature without label (fallback)
        for hwmon_path in &hwmon_paths {
            for temp_idx in 0..20 {
                let temp_input = hwmon_path.join(format!("temp{}_input", temp_idx));
                if let Some(temp) = read_temp_file(&temp_input) {
                    return temp;
                }
            }
        }
    }
    
    // Fallback: Try /sys/class/thermal/thermal_zone* (like btop does)
    for tz_idx in 0..10 {
        let thermal_path = PathBuf::from(format!("/sys/class/thermal/thermal_zone{}", tz_idx));
        let temp_file = thermal_path.join("temp");
        
        // Check the type to see if it's a CPU temperature
        if let Ok(temp_type) = fs::read_to_string(thermal_path.join("type")) {
            let type_lower = temp_type.to_lowercase();
            // Skip acpitz and other non-CPU thermal zones
            if type_lower.contains("acpi") || type_lower.contains("pch") {
                continue;
            }
        }
        
        if let Some(temp) = read_temp_file(&temp_file) {
            return temp;
        }
    }
    
    0  // Default to 0 if not found
}
