use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, OutputStream, Sink};
use serialport::SerialPortType;
use sysinfo::System;

mod helpers;
mod system_info;

/// Busca el archivo de sonido preferentemente en 'sounds/', luego en '<exe_dir>/sounds/', o en el directorio actual.
fn find_sound_file(file_name: &str) -> Option<PathBuf> {
    // 1. Relativo directo: "sounds/<file_name>"
    let p1 = Path::new("sounds").join(file_name);
    if p1.is_file() {
        return Some(p1);
    }

    // 2. Relativo al directorio del ejecutable: "<exe_dir>/sounds/<file_name>"
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let p2 = exe_dir.join("sounds").join(file_name);
            if p2.is_file() {
                return Some(p2);
            }
            let p3 = exe_dir.join(file_name);
            if p3.is_file() {
                return Some(p3);
            }
        }
    }

    // 3. Directorio actual: "<file_name>"
    let p4 = Path::new(file_name);
    if p4.is_file() {
        return Some(p4.to_path_buf());
    }

    None
}

/// Reproduce el sonido en el stream/handle de audio especificado
fn play_sound(stream_handle: &rodio::OutputStreamHandle, file_name: &str) {
    if let Some(path) = find_sound_file(file_name) {
        if let Ok(file) = File::open(&path) {
            let buf = std::io::BufReader::new(file);
            if let Ok(source) = Decoder::new(buf) {
                if let Ok(sink) = Sink::try_new(stream_handle) {
                    sink.append(source);
                    sink.detach();
                }
            }
        }
    } else {
        println!("Aviso: No se encontró el archivo de audio '{}' en 'sounds/'", file_name);
    }
}

fn main() {
    println!("=====================================================");
    println!("  Flipper PC Monitor (USB Backend + Soundboard v2.0) ");
    println!("=====================================================");

    // 1. INICIALIZAR AUDIO PRINCIPAL (Auriculares / Altavoces)
    let (_default_stream, default_handle) = match OutputStream::try_default() {
        Ok(res) => {
            println!("Audio local: Conectado (Tú escucharás los sonidos).");
            res
        }
        Err(e) => {
            eprintln!("No se pudo inicializar audio por defecto: {}", e);
            // Salir o continuar sin sonido si falla el driver
            std::process::exit(1);
        }
    };

    // 2. BUSCAR E INICIALIZAR CABLE VIRTUAL (Para Discord / Juegos / Micro)
    let host = rodio::cpal::default_host();
    let mut virtual_device = None;
    let mut _virtual_stream = None;
    let mut virtual_handle = None;

    if let Ok(devices) = host.output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if name.to_uppercase().contains("CABLE") {
                    println!("Cable Virtual detectado: {}", name);
                    virtual_device = Some(device);
                    break;
                }
            }
        }
    }

    if let Some(device) = virtual_device {
        if let Ok((stream, handle)) = OutputStream::try_from_device(&device) {
            _virtual_stream = Some(stream);
            virtual_handle = Some(handle);
            println!("Audio Virtual: Conectado (Tus amigos en Discord escucharán los sonidos).");
        }
    } else {
        println!("No se detectó Cable Virtual (VB-Cable). Los sonidos solo se escucharán localmente.");
    }

    let mut system_info = System::new_all();
    let mut components = sysinfo::Components::new_with_refreshed_list();
    let mut serial_buf: Vec<u8> = vec![0; 128];
    let mut text_buffer = String::new();

    // Auto-detect loop
    loop {
        println!("\nScanning for Flipper Zero...");

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
            // Timeout bajo (10ms) para leer pulsaciones en tiempo real sin bloquear el loop
            match serialport::new(port_name.clone(), 115200)
                .timeout(Duration::from_millis(10))
                .flow_control(serialport::FlowControl::None)
                .open()
            {
                Ok(mut port) => {
                    println!("Connected to {}!", port_name);

                    port.write_data_terminal_ready(true).ok();
                    port.write_request_to_send(true).ok();

                    let mut last_update_time = Instant::now();
                    let mut first_run = true;
                    text_buffer.clear();

                    loop {
                        // 1. MANDAR DATOS AL FLIPPER CADA 1 SEGUNDO (Telemetría de PC)
                        if first_run || last_update_time.elapsed() >= Duration::from_secs(1) {
                            first_run = false;
                            let info = system_info::SystemInfo::get_system_info(
                                &mut system_info,
                                &mut components,
                            );
                            let data = bincode::serialize(&info).unwrap();

                            match port.write_all(&data) {
                                Ok(_) => {
                                    let _ = port.flush();
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                                    // Ignorar timeouts de escritura
                                }
                                Err(e) => {
                                    println!("Fallo al escribir en puerto serie: {}", e);
                                    break; // Reconectar
                                }
                            }
                            last_update_time = Instant::now();
                        }

                        // 2. LEER EVENTOS DE BOTONES DEL FLIPPER (Soundboard)
                        match port.read(serial_buf.as_mut_slice()) {
                            Ok(bytes_read) if bytes_read > 0 => {
                                let received = String::from_utf8_lossy(&serial_buf[..bytes_read]);
                                text_buffer.push_str(&received);

                                while let Some(pos) = text_buffer.find('\n') {
                                    let line = text_buffer[..pos].trim().to_string();
                                    text_buffer = text_buffer[pos + 1..].to_string();

                                    if line.contains("Press") {
                                        let mut sound_to_play = None;

                                        if line.contains("Up") {
                                            println!("[Soundboard] Up");
                                            sound_to_play = Some("up.mp3");
                                        } else if line.contains("Down") {
                                            println!("[Soundboard] Down");
                                            sound_to_play = Some("down.mp3");
                                        } else if line.contains("Left") {
                                            println!("[Soundboard] Left");
                                            sound_to_play = Some("left.mp3");
                                        } else if line.contains("Right") {
                                            println!("[Soundboard] Right");
                                            sound_to_play = Some("right.mp3");
                                        } else if line.contains("Ok") {
                                            println!("[Soundboard] Ok");
                                            sound_to_play = Some("ok.mp3");
                                        } else if line.contains("Back") {
                                            println!("[Soundboard] Back");
                                            sound_to_play = Some("back.mp3");
                                        }

                                        if let Some(file) = sound_to_play {
                                            play_sound(&default_handle, file);
                                            if let Some(ref v_handle) = virtual_handle {
                                                play_sound(v_handle, file);
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                                // Timeout normal al no haber teclas presionadas
                            }
                            Err(e) => {
                                println!("Flipper desconectado o error de lectura: {}", e);
                                break;
                            }
                        }

                        // Pequeño descanso para no saturar CPU en el bucle rápido
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let is_busy = err_str.to_lowercase().contains("busy")
                        || err_str.to_lowercase().contains("access is denied");

                    if is_busy {
                        println!("Port {} is busy — another process has it open.", port_name);
                        #[cfg(target_os = "linux")]
                        println!("  -> Run:  sudo fuser -k {}  (kills whichever process holds it)", port_name);
                        #[cfg(target_os = "windows")]
                        println!("  -> Close qFlipper, any serial terminal, or Device Manager view that may have the port open.");
                        println!("  -> Then restart this backend.");
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
