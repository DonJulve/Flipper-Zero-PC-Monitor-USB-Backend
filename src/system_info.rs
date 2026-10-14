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
    use std::cell::RefCell;
    use std::collections::HashMap;
    use wmi::{COMLibrary, Variant, WMIConnection};

    type Row = HashMap<String, Variant>;

    fn variant_f64(v: &Variant) -> Option<f64> {
        Some(match v {
            Variant::I1(x) => f64::from(*x),
            Variant::I2(x) => f64::from(*x),
            Variant::I4(x) => f64::from(*x),
            Variant::I8(x) => *x as f64,
            Variant::UI1(x) => f64::from(*x),
            Variant::UI2(x) => f64::from(*x),
            Variant::UI4(x) => f64::from(*x),
            Variant::UI8(x) => *x as f64,
            Variant::R4(x) => f64::from(*x),
            Variant::R8(x) => *x,
            Variant::String(s) => s.trim().parse().ok()?,
            _ => return None,
        })
    }

    fn variant_string(v: &Variant) -> String {
        match v {
            Variant::String(s) => s.clone(),
            _ => String::new(),
        }
    }

    fn plausible_temp(c: f64) -> bool {
        c.is_finite() && (5.0..=125.0).contains(&c)
    }

    fn tenths_kelvin_to_c(v: f64) -> f64 {
        v / 10.0 - 273.15
    }

    struct SensorData {
        name: String,
        identifier: String,
        value: f64,
    }

    pub struct WindowsHardwareMonitor {
        com_lib: Option<COMLibrary>,
        lhm_conn: Option<WMIConnection>,
        ohm_conn: Option<WMIConnection>,
        cimv2_conn: Option<WMIConnection>,
        wmi_conn: Option<WMIConnection>,
        last_logged: Option<String>,
    }

    impl WindowsHardwareMonitor {
        fn new() -> Self {
            let com_lib = COMLibrary::new().ok().or_else(|| COMLibrary::without_security().ok());
            WindowsHardwareMonitor {
                com_lib,
                lhm_conn: None,
                ohm_conn: None,
                cimv2_conn: None,
                wmi_conn: None,
                last_logged: None,
            }
        }

        fn get_com(&mut self) -> Option<COMLibrary> {
            if self.com_lib.is_none() {
                self.com_lib = COMLibrary::new().ok().or_else(|| COMLibrary::without_security().ok());
            }
            self.com_lib
        }

        fn query_lhm_ns(&mut self, ns: &'static str, is_ohm: bool) -> Result<(Option<u8>, Option<u8>), String> {
            let com = self.get_com().ok_or_else(|| "COM library initialization failed".to_string())?;
            let conn = if is_ohm {
                if self.ohm_conn.is_none() {
                    self.ohm_conn = WMIConnection::with_namespace_path(ns, com).ok();
                }
                self.ohm_conn.as_ref()
            } else {
                if self.lhm_conn.is_none() {
                    self.lhm_conn = WMIConnection::with_namespace_path(ns, com).ok();
                }
                self.lhm_conn.as_ref()
            }.ok_or_else(|| format!("cannot connect to namespace {}", ns))?;

            let query = "SELECT Name, Identifier, SensorType, Value FROM Sensor WHERE SensorType = 'Temperature'";
            let rows: Vec<Row> = conn.raw_query(query).map_err(|e| {
                if is_ohm { self.ohm_conn = None; } else { self.lhm_conn = None; }
                format!("query failed: {e}")
            })?;

            if rows.is_empty() {
                return Err("empty sensor list".to_string());
            }

            let sensors: Vec<SensorData> = rows
                .iter()
                .filter_map(|r| {
                    let val = variant_f64(r.get("Value")?)?;
                    if !plausible_temp(val) {
                        return None;
                    }
                    Some(SensorData {
                        name: r.get("Name").map(variant_string).unwrap_or_default(),
                        identifier: r.get("Identifier").map(variant_string).unwrap_or_default(),
                        value: val,
                    })
                })
                .collect();

            if sensors.is_empty() {
                return Err("no plausible temperature sensors found".to_string());
            }

            let is_cpu = |s: &SensorData| {
                s.identifier.to_ascii_lowercase().contains("cpu")
                    || s.name.to_ascii_lowercase().contains("cpu")
            };

            // Order: "CPU Package" > "Core Max" > "Core (Tctl/Tdie)" > hottest CPU sensor
            let cpu_sensor = sensors
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case("CPU Package") && is_cpu(s))
                .or_else(|| sensors.iter().find(|s| s.name.eq_ignore_ascii_case("Core Max") && is_cpu(s)))
                .or_else(|| sensors.iter().find(|s| s.name.to_ascii_lowercase().contains("tctl") && is_cpu(s)))
                .or_else(|| {
                    sensors
                        .iter()
                        .filter(|s| is_cpu(s))
                        .max_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal))
                });

            let is_gpu = |s: &SensorData| {
                s.identifier.to_ascii_lowercase().contains("gpu")
                    || s.name.to_ascii_lowercase().contains("gpu")
            };
            let gpu_sensor = sensors
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case("GPU Core") && is_gpu(s))
                .or_else(|| sensors.iter().find(|s| is_gpu(s)));

            let cpu_temp = cpu_sensor.map(|s| s.value.round() as u8);
            let gpu_temp = gpu_sensor.map(|s| s.value.round() as u8);

            if cpu_temp.is_some() || gpu_temp.is_some() {
                Ok((cpu_temp, gpu_temp))
            } else {
                Err("no CPU or GPU temperature identified".to_string())
            }
        }

        fn query_thermal_zone_perf(&mut self) -> Result<Option<u8>, String> {
            let com = self.get_com().ok_or_else(|| "COM library initialization failed".to_string())?;
            if self.cimv2_conn.is_none() {
                self.cimv2_conn = WMIConnection::with_namespace_path("ROOT\\CIMV2", com).ok();
            }
            let conn = self.cimv2_conn.as_ref().ok_or_else(|| "cannot connect to ROOT\\CIMV2".to_string())?;

            let query = "SELECT HighPrecisionTemperature, Temperature FROM Win32_PerfFormattedData_Counters_ThermalZoneInformation";
            let rows: Vec<Row> = conn.raw_query(query).map_err(|e| {
                self.cimv2_conn = None;
                format!("query failed: {e}")
            })?;

            let best = rows
                .iter()
                .filter_map(|r| {
                    r.get("HighPrecisionTemperature")
                        .and_then(variant_f64)
                        .filter(|&v| v > 0.0)
                        .or_else(|| r.get("Temperature").and_then(variant_f64).map(|k| k * 10.0))
                })
                .map(tenths_kelvin_to_c)
                .filter(|&c| plausible_temp(c))
                .max_by(|a, b| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal));

            Ok(best.map(|c| c.round() as u8))
        }

        fn query_acpi_thermal_zone(&mut self) -> Result<Option<u8>, String> {
            let com = self.get_com().ok_or_else(|| "COM library initialization failed".to_string())?;
            if self.wmi_conn.is_none() {
                self.wmi_conn = WMIConnection::with_namespace_path("ROOT\\WMI", com).ok();
            }
            let conn = self.wmi_conn.as_ref().ok_or_else(|| "cannot connect to ROOT\\WMI".to_string())?;

            let query = "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
            let rows: Vec<Row> = conn.raw_query(query).map_err(|e| {
                self.wmi_conn = None;
                format!("query failed: {e}")
            })?;

            let best = rows
                .iter()
                .filter_map(|r| r.get("CurrentTemperature").and_then(variant_f64))
                .map(tenths_kelvin_to_c)
                .filter(|&c| plausible_temp(c))
                .max_by(|a, b| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal));

            Ok(best.map(|c| c.round() as u8))
        }

        pub fn read(&mut self) -> (Option<u8>, Option<u8>) {
            // Source 1: LibreHardwareMonitor
            match self.query_lhm_ns("ROOT\\LibreHardwareMonitor", false) {
                Ok((cpu, gpu)) => {
                    let msg = "LibreHardwareMonitor WMI";
                    if self.last_logged.as_deref() != Some(msg) {
                        println!("[Hardware Monitor] CPU Temp source: {} (CPU: {:?}°C, GPU: {:?}°C)", msg, cpu, gpu);
                        self.last_logged = Some(msg.to_string());
                    }
                    return (cpu, gpu);
                }
                Err(_) => {}
            }

            // Source 2: OpenHardwareMonitor
            match self.query_lhm_ns("ROOT\\OpenHardwareMonitor", true) {
                Ok((cpu, gpu)) => {
                    let msg = "OpenHardwareMonitor WMI";
                    if self.last_logged.as_deref() != Some(msg) {
                        println!("[Hardware Monitor] CPU Temp source: {} (CPU: {:?}°C, GPU: {:?}°C)", msg, cpu, gpu);
                        self.last_logged = Some(msg.to_string());
                    }
                    return (cpu, gpu);
                }
                Err(_) => {}
            }

            // Source 3: ThermalZoneInformation perf counter (ROOT\CIMV2)
            match self.query_thermal_zone_perf() {
                Ok(Some(cpu)) => {
                    let msg = "Win32_PerfFormattedData ThermalZone";
                    if self.last_logged.as_deref() != Some(msg) {
                        println!("[Hardware Monitor] CPU Temp source: {} (CPU: {}°C)", msg, cpu);
                        self.last_logged = Some(msg.to_string());
                    }
                    return (Some(cpu), None);
                }
                _ => {}
            }

            // Source 4: MSAcpi_ThermalZoneTemperature (ROOT\WMI)
            match self.query_acpi_thermal_zone() {
                Ok(Some(cpu)) => {
                    let msg = "MSAcpi_ThermalZoneTemperature";
                    if self.last_logged.as_deref() != Some(msg) {
                        println!("[Hardware Monitor] CPU Temp source: {} (CPU: {}°C)", msg, cpu);
                        self.last_logged = Some(msg.to_string());
                    }
                    return (Some(cpu), None);
                }
                _ => {}
            }

            if self.last_logged.as_deref() != Some("None") {
                println!("[Hardware Monitor] Aviso: No se detectó ninguna fuente de temperatura en Windows.");
                println!("  -> Si usas Libre Hardware Monitor, ábrelo y verifica que 'Options -> WMI' esté activado.");
                self.last_logged = Some("None".to_string());
            }

            (None, None)
        }
    }

    thread_local! {
        static MONITOR: RefCell<WindowsHardwareMonitor> = RefCell::new(WindowsHardwareMonitor::new());
    }

    pub fn get_windows_cpu_temp() -> (Option<u8>, Option<u8>) {
        MONITOR.with(|m| m.borrow_mut().read())
    }
}
