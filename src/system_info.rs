use crate::helpers::{avg_vecu32, nvd_r2u64, pop_4u8};
use serde::Serialize;
use sysinfo::MemoryRefreshKind;


/*
typedef struct {
    uint8_t cpu_usage;
    uint16_t ram_max;
    uint8_t ram_usage;
    char ram_unit[4];
    uint8_t gpu_usage;
    uint16_t vram_max;
    uint8_t vram_usage;
    char vram_unit[4];
} DataStruct;
*/

#[derive(Serialize, Debug, Clone)]
pub struct SystemInfo {
    pub cpu_usage: u8,
    pub ram_max: u16,
    pub ram_usage: u8,
    pub ram_unit: [u8; 4],
    pub gpu_usage: u8,
    pub vram_max: u16,
    pub vram_usage: u8,
    pub vram_unit: [u8; 4],
    pub cpu_temp: u8,   // degrees C, 255 = N/A
    pub gpu_temp: u8,   // degrees C, 255 = N/A
}

impl SystemInfo {
    fn get_unit(exp: u32) -> String {
        match exp {
            0 => "B",
            1 => "KB",
            2 => "MB",
            3 => "GB",
            4 => "TB",
            _ => "UB",
        }
        .to_owned()
    }

    fn get_exp(num: u64, base: u64) -> u32 {
        match num {
            x if x > u64::pow(base, 4) => 4,
            x if x > u64::pow(base, 3) => 3,
            x if x > u64::pow(base, 2) => 2,
            x if x > base => 1,
            _ => 0,
        }
    }

    pub fn get_system_info(
        system_info: &mut sysinfo::System,
        components: &mut sysinfo::Components,
    ) -> Self {
        // Need to refresh only CPU and RAM => big boost when combined with reusing system_info
        // system_info.refresh_all();
        system_info.refresh_memory_specifics(MemoryRefreshKind::new().with_ram());
        let base = 1024;

        let ram_max = system_info.total_memory();
        let ram_exp = Self::get_exp(ram_max, base);

        let gpu_info = GpuInfo::get_gpu_info();
        let vram_mult = u64::pow(base, 2); // MiB

        let vram_max = match &gpu_info {
            Some(gi) => gi.vram_max * vram_mult,
            None => 0,
        };
        let vram_exp = Self::get_exp(vram_max, base);

        // Refresh only CPU usage before reading
        system_info.refresh_cpu_usage();

        // CPU temperature:
        // Sources prioritized:
        // 1. LibreHardwareMonitor / OpenHardwareMonitor WMI (CPU Package, Core Max, hottest CPU sensor)
        // 2. Win32_PerfFormattedData_Counters_ThermalZoneInformation (ROOT\CIMV2)
        // 3. MSAcpi_ThermalZoneTemperature (ROOT\WMI)
        // Fallback to sysinfo Components with prioritized picking (works natively on Linux).
        #[cfg(target_os = "windows")]
        let (cpu_temp_override, gpu_temp_override) = windows_hardware::get_windows_cpu_temp();
        #[cfg(not(target_os = "windows"))]
        let (cpu_temp_override, gpu_temp_override): (Option<u8>, Option<u8>) = (None, None);

        components.refresh();
        let cpu_temp: u8 = cpu_temp_override.unwrap_or_else(|| {
            // Prioritized picking:
            // Priority 3: package, tctl, tdie
            // Priority 2: coretemp, k10temp, cpu
            // Priority 1: core
            let mut best: Option<(u8, f32)> = None;
            for c in components.iter() {
                let temp = c.temperature();
                if !(5.0..=125.0).contains(&temp) {
                    continue;
                }
                let l = c.label().to_ascii_lowercase();
                let prio = if l.contains("package") || l.contains("tctl") || l.contains("tdie") {
                    3
                } else if l.contains("coretemp") || l.contains("k10temp") || l.contains("cpu") {
                    2
                } else if l.contains("core") {
                    1
                } else {
                    continue;
                };
                best = match best {
                    Some((p, v)) if p > prio || (p == prio && v >= temp) => Some((p, v)),
                    _ => Some((prio, temp)),
                };
            }
            best.map(|(_, t)| t.round() as u8).unwrap_or(u8::MAX)
        });

        SystemInfo {
            cpu_usage: avg_vecu32(
                system_info
                    .cpus()
                    .iter()
                    .map(|c| c.cpu_usage() as u32)
                    .collect(),
            ) as u8,
            // cpu_usage: system_info.cpus().first().unwrap().cpu_usage() as u8,
            ram_max: (ram_max as f64 / u64::pow(base, ram_exp) as f64 * 10.0) as u16,
            ram_usage: (system_info.used_memory() as f64 / ram_max as f64 * 100.0) as u8,
            ram_unit: pop_4u8(Self::get_unit(ram_exp).as_bytes()),
            gpu_usage: match &gpu_info {
                Some(gi) => gi.gpu_usage as u8,
                None => u8::MAX,
            },
            vram_max: (vram_max as f64 / u64::pow(base, vram_exp) as f64 * 10.0) as u16,
            vram_usage: match &gpu_info {
                Some(gi) => {
                    (gi.vram_used as f64 * vram_mult as f64 / vram_max as f64 * 100.0) as u8
                }
                None => u8::MAX,
            },
            vram_unit: pop_4u8(Self::get_unit(vram_exp).as_bytes()),
            cpu_temp,
            gpu_temp: gpu_temp_override
                .or_else(|| gpu_info.as_ref().map(|gi| gi.gpu_temp as u8))
                .unwrap_or(u8::MAX),
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct GpuInfo {
    pub gpu_usage: u64,
    pub vram_max: u64,
    pub vram_used: u64,
    pub gpu_temp: u64,  // degrees C, 0 = N/A
}

impl GpuInfo {
    pub fn get_gpu_info() -> Option<Self> {
        // TODO: AMD support
        // TODO: AMD support
        // Using std::process instead of tokio::process
        let Ok(cmd) = std::process::Command::new("nvidia-smi")
            .arg("-q")
            .arg("-x")
            .stdout(std::process::Stdio::piped())
            .spawn()
        else {
            return None;
        };
        
        // No need for async read
        let output = cmd.wait_with_output().ok()?;
        let stdout_str = String::from_utf8_lossy(&output.stdout);

        match xmltojson::to_json(&stdout_str) {
            Ok(json) => {
                let g = json["nvidia_smi_log"]["gpu"].to_owned();

                let Some(gpu_usage) = nvd_r2u64(g["utilization"]["gpu_util"].to_string()) else {
                    return None;
                };
                let Some(vram_max) = nvd_r2u64(g["fb_memory_usage"]["total"].to_string()) else {
                    return None;
                };
                let Some(vram_used) = nvd_r2u64(g["fb_memory_usage"]["used"].to_string()) else {
                    return None;
                };
                // gpu_temp may be absent on some cards, default to 0
                let gpu_temp = nvd_r2u64(g["temperature"]["gpu_temp"].to_string()).unwrap_or(0);

                Some(GpuInfo {
                    gpu_usage,
                    vram_max,
                    vram_used,
                    gpu_temp,
                })
            }
            Err(_) => None,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_hardware {
    use serde::Deserialize;
    use wmi::{COMLibrary, WMIConnection};

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Sensor")]
    #[serde(rename_all = "PascalCase")]
    #[allow(dead_code)]
    struct HardwareSensor {
        name: String,
        #[serde(default)]
        identifier: String,
        sensor_type: String,
        value: f32,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "PascalCase")]
    #[allow(dead_code)]
    struct ThermalZonePerf {
        #[serde(default)]
        high_precision_temperature: Option<f32>,
        #[serde(default)]
        temperature: Option<f32>,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "PascalCase")]
    #[allow(dead_code)]
    struct AcpiThermalZone {
        #[serde(default)]
        current_temperature: Option<f32>,
    }

    fn plausible_temp(c: f32) -> bool {
        c.is_finite() && (5.0..=125.0).contains(&c)
    }

    fn tenths_kelvin_to_c(v: f32) -> f32 {
        v / 10.0 - 273.15
    }

    pub fn get_windows_cpu_temp() -> (Option<u8>, Option<u8>) {
        let com_con = match COMLibrary::new() {
            Ok(c) => c,
            Err(_) => match COMLibrary::without_security() {
                Ok(c) => c,
                Err(_) => return (None, None),
            },
        };

        // Source 1: LibreHardwareMonitor / OpenHardwareMonitor WMI (CPU Package, Core Max, hottest CPU sensor)
        let lhm_namespaces = ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"];
        for ns in lhm_namespaces {
            if let Ok(wmi_con) = WMIConnection::with_namespace_path(ns, com_con) {
                let query = "SELECT Name, Identifier, SensorType, Value FROM Sensor WHERE SensorType = 'Temperature'";
                if let Ok(sensors) = wmi_con.raw_query::<HardwareSensor>(query) {
                    if !sensors.is_empty() {
                        let temps: Vec<&HardwareSensor> = sensors
                            .iter()
                            .filter(|s| s.sensor_type.eq_ignore_ascii_case("Temperature") && plausible_temp(s.value))
                            .collect();

                        let is_cpu = |s: &HardwareSensor| {
                            s.identifier.to_ascii_lowercase().contains("cpu")
                                || s.name.to_ascii_lowercase().contains("cpu")
                        };

                        // 1. "CPU Package", else 2. "Core Max", else 3. hottest CPU sensor
                        let cpu_sensor = temps
                            .iter()
                            .find(|s| s.name.eq_ignore_ascii_case("CPU Package") && is_cpu(s))
                            .or_else(|| temps.iter().find(|s| s.name.eq_ignore_ascii_case("Core Max") && is_cpu(s)))
                            .or_else(|| {
                                temps
                                    .iter()
                                    .filter(|s| is_cpu(s))
                                    .max_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal))
                            });

                        let cpu_temp = cpu_sensor.map(|s| s.value.round() as u8);

                        // GPU temperature from LHM if available
                        let is_gpu = |s: &HardwareSensor| {
                            s.identifier.to_ascii_lowercase().contains("gpu")
                                || s.name.to_ascii_lowercase().contains("gpu")
                        };
                        let gpu_sensor = temps
                            .iter()
                            .find(|s| s.name.eq_ignore_ascii_case("GPU Core") && is_gpu(s))
                            .or_else(|| temps.iter().find(|s| is_gpu(s)));
                        let gpu_temp = gpu_sensor.map(|s| s.value.round() as u8);

                        if cpu_temp.is_some() || gpu_temp.is_some() {
                            return (cpu_temp, gpu_temp);
                        }
                    }
                }
            }
        }

        // Source 2: Win32_PerfFormattedData_Counters_ThermalZoneInformation (ROOT\CIMV2)
        if let Ok(wmi_con) = WMIConnection::with_namespace_path("ROOT\\CIMV2", com_con) {
            let query = "SELECT HighPrecisionTemperature, Temperature FROM Win32_PerfFormattedData_Counters_ThermalZoneInformation";
            if let Ok(rows) = wmi_con.raw_query::<ThermalZonePerf>(query) {
                let best = rows
                    .iter()
                    .filter_map(|r| {
                        r.high_precision_temperature
                            .filter(|&v| v > 0.0)
                            .or_else(|| r.temperature.map(|k| k * 10.0))
                    })
                    .map(tenths_kelvin_to_c)
                    .filter(|&c| plausible_temp(c))
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                if let Some(c) = best {
                    return (Some(c.round() as u8), None);
                }
            }
        }

        // Source 3: MSAcpi_ThermalZoneTemperature (ROOT\WMI)
        if let Ok(wmi_con) = WMIConnection::with_namespace_path("ROOT\\WMI", com_con) {
            let query = "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
            if let Ok(rows) = wmi_con.raw_query::<AcpiThermalZone>(query) {
                let best = rows
                    .iter()
                    .filter_map(|r| r.current_temperature)
                    .map(tenths_kelvin_to_c)
                    .filter(|&c| plausible_temp(c))
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                if let Some(c) = best {
                    return (Some(c.round() as u8), None);
                }
            }
        }

        (None, None)
    }
}
