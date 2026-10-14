use std::io::Write;
use std::time::Duration;
use serialport::SerialPortType;
use sysinfo::System;

mod helpers;
mod system_info;

fn main() {
    // std::env::set_var("RUST_BACKTRACE", "full");

    println!("Starting Flipper PC Monitor (USB Backend)");

    let mut system_info = System::new_all();
    
    // Auto-detect loop
    loop {
        println!("Scanning for Flipper Zero...");
        
        let ports = serialport::available_ports().expect("No ports found!");
        let mut flipper_port_name: Option<String> = None;

        for p in ports {
            if let SerialPortType::UsbPort(info) = p.port_type {
                // Flipper Zero VID/PID: 0483:5740
                if info.vid == 0x0483 && info.pid == 0x5740 {
                    println!("Found Flipper Zero on {}", p.port_name);
                    flipper_port_name = Some(p.port_name);
                    break;
                }
            }
        }

        if let Some(port_name) = flipper_port_name {
            match serialport::new(port_name.clone(), 115200)
                .timeout(Duration::from_millis(1000))
                .flow_control(serialport::FlowControl::None)
                .open() 
            {
                Ok(mut port) => {
                    println!("Connected to {}!", port_name);
                    
                    port.write_data_terminal_ready(true).ok();
                    port.write_request_to_send(true).ok();
                    
                    loop {
                        let info = system_info::SystemInfo::get_system_info(&mut system_info);
                        // Manual packing to match DataStruct layout
                        // struct {
                        //     uint8_t cpu_usage;
                        //     uint16_t ram_max;
                        //     uint8_t ram_usage;
                        //     char ram_unit[4];
                        //     uint8_t gpu_usage;
                        //     uint16_t vram_max;
                        //     uint8_t vram_usage;
                        //     char vram_unit[4];
                        //     uint8_t cpu_temp;
                        //     uint8_t gpu_temp;
                        // } DataStruct;
                        
                        // We use bincode or manual packing. SystemInfo struct in system_info.rs is:
                        /*
                        pub struct SystemInfo {
                            pub cpu_usage: u8,
                            pub ram_max: u16,
                            pub ram_usage: u8,
                            pub ram_unit: [u8; 4],
                            pub gpu_usage: u8,
                            pub vram_max: u16,
                            pub vram_usage: u8,
                            pub vram_unit: [u8; 4],
                            pub cpu_temp: u8,
                            pub gpu_temp: u8,
                        }
                        */
                        // Since SystemInfo uses u8 for fields, and [u8;4] for arrays, AND it derives Serialize.
                        // However, default bincode serialization might add length prefix for arrays if they were Vec, but they are fixed arrays.
                        // Also endianness. bincode default is little endian.
                        // Let's rely on bincode::serialize for now as it was used before.
                        
                        let data = bincode::serialize(&info).unwrap();
                        
                        match port.write_all(&data) {
                            Ok(_) => {
                                // println!("Sent update");
                            },
                            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                                // Ignore timeouts, just skip this frame vs reconnecting
                                println!("Timeout writing to serial port, skipping frame...");
                            },
                            Err(e) => {
                                println!("Failed to write to serial port: {}", e);
                                break; // Break inner loop to reconnect
                            }
                        }

                        std::thread::sleep(Duration::from_secs(1));
                    }
                },
                Err(e) => {
                    let err_str = e.to_string();
                    // "Device or resource busy" on Linux, "Access is denied" on Windows
                    let is_busy = err_str.to_lowercase().contains("busy")
                        || err_str.to_lowercase().contains("access is denied");

                    if is_busy {
                        println!(
                            "Port {} is busy — another process has it open.",
                            port_name
                        );
                        #[cfg(target_os = "linux")]
                        println!(
                            "  -> Run:  sudo fuser -k {}  (kills whichever process holds it)",
                            port_name
                        );
                        #[cfg(target_os = "windows")]
                        println!(
                            "  -> Close qFlipper, any serial terminal, or Device Manager view\
                             that may have the port open."
                        );
                        println!("  -> Then restart this backend.");
                        // No point hammering the port if it is busy; exit so the user can fix it.
                        std::process::exit(1);
                    } else {
                        println!("Failed to open serial port: {}", e);
                    }
                }
            }
        } else {
            println!("Flipper Zero not found. Retrying in 2s...");
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}
