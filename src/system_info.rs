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

        // CPU temperature via persistent sysinfo Components
        // Rejects invalid readings (<= 15°C or >= 125°C) to prevent false 0°C on Windows WMI
        components.refresh();
        let cpu_temp: u8 = components
            .iter()
            .find(|c| {
                let temp = c.temperature();
                if temp <= 15.0 || temp >= 125.0 {
                    return false;
                }
                let label = c.label().to_lowercase();
                label.contains("package")
                    || label.contains("cpu")
                    || label.contains("tctl")
                    || label.contains("tdie")
                    || label.contains("core")
                    || label.contains("k10temp")
            })
            .or_else(|| {
                components
                    .iter()
                    .find(|c| c.temperature() > 15.0 && c.temperature() < 125.0)
            })
            .map(|c| c.temperature().round() as u8)
            .unwrap_or(u8::MAX);

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
            gpu_temp: match &gpu_info {
                Some(gi) => gi.gpu_temp as u8,
                None => u8::MAX,
            },
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
