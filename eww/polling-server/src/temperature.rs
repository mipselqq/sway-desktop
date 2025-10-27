/// Temperature sensor reading in Celsius
use std::fs;
use std::path::{Path, PathBuf};
use std::fs::File;

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

/// Find the temperature sensor file path (called once at startup)
fn find_temp_file_path() -> Option<PathBuf> {
    // Try to find in /sys/class/hwmon/ with priority for Package/Tdie temps
    if let Some(path) = find_hwmon_temp() {
        return Some(path);
    }
    
    // Fallback: Try /sys/class/thermal/thermal_zone*
    find_thermal_temp()
}

/// Search /sys/class/hwmon for temperature sensor files
fn find_hwmon_temp() -> Option<PathBuf> {
    let hwmon_entries = fs::read_dir("/sys/class/hwmon").ok()?;
    let mut hwmon_paths: Vec<_> = hwmon_entries.flatten().map(|e| e.path()).collect();
    hwmon_paths.sort();
    
    // Helper: Collect available temp files in a hwmon directory (avoids repeated stat calls)
    fn get_temp_files(hwmon_path: &Path) -> Vec<(u32, PathBuf)> {
        let mut temps = Vec::new();
        if let Ok(entries) = fs::read_dir(hwmon_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("temp") && name.ends_with("_input") {
                        // Extract temperature index from filename (e.g., "temp0_input" -> 0)
                        if let Ok(idx) = name[4..].trim_end_matches("_input").parse::<u32>() {
                            temps.push((idx, path));
                        }
                    }
                }
            }
        }
        temps.sort_by_key(|t| t.0);
        temps
    }
    
    // First priority: Package/Tdie with label
    for hwmon_path in &hwmon_paths {
        for (temp_idx, temp_input) in get_temp_files(hwmon_path) {
            if let Some(label) = get_temp_label(hwmon_path, temp_idx) {
                if is_package_temp_label(&label) {
                    return Some(temp_input);
                }
            }
        }
    }
    
    // Second priority: Any CPU-labeled temp
    for hwmon_path in &hwmon_paths {
        for (temp_idx, temp_input) in get_temp_files(hwmon_path) {
            if let Some(label) = get_temp_label(hwmon_path, temp_idx) {
                if is_cpu_temp_label(&label) {
                    return Some(temp_input);
                }
            }
        }
    }
    
    // Third priority: Any temp file
    for hwmon_path in &hwmon_paths {
        if let Some((_, temp_input)) = get_temp_files(hwmon_path).first() {
            return Some(temp_input.clone());
        }
    }
    
    None
}

/// Search /sys/class/thermal for temperature sensor files
fn find_thermal_temp() -> Option<PathBuf> {
    for tz_idx in 0..10 {
        let thermal_path = PathBuf::from(format!("/sys/class/thermal/thermal_zone{}", tz_idx));
        let temp_file = thermal_path.join("temp");
        
        // Skip non-CPU thermal zones
        if let Ok(temp_type) = fs::read_to_string(thermal_path.join("type")) {
            let type_lower = temp_type.to_lowercase();
            if type_lower.contains("acpi") || type_lower.contains("pch") {
                continue;
            }
        }
        
        if temp_file.exists() {
            return Some(temp_file);
        }
    }
    None
}

/// Read temperature from already-open file descriptor using pread
pub fn read_temperature_from_fd(fd: i32, buf: &mut [u8]) -> u32 {
    let n = unsafe {
        libc::pread(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0)
    };
    
    if n <= 0 {
        return 0;
    }
    
    let Ok(content) = std::str::from_utf8(&buf[..n as usize]) else {
        return 0;
    };
    
    let Ok(millidegrees) = content.trim().parse::<u32>() else {
        return 0;
    };
    
    let temp = millidegrees / 1000;
    if temp > 0 { temp } else { 0 }
}

/// Find and initialize temperature file at startup
/// Returns (File, Vec buffer) tuple for efficient repeated reading
pub fn init_temperature() -> Option<(File, Vec<u8>)> {
    if let Some(temp_path) = find_temp_file_path() {
        if let Ok(file) = File::open(temp_path) {
            // Pre-allocate buffer for temperature reading (64 bytes is enough for any temp file)
            let buf = vec![0u8; 64];
            return Some((file, buf));
        }
    }
    None
}
