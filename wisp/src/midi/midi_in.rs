use log::info;
use midir::{MidiInput, MidiInputPort};

pub struct WispMidiIn {
    pub midi_in: MidiInput,
    pub port: MidiInputPort,
}

impl WispMidiIn {
    pub fn open(name: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let midi_in = MidiInput::new("wisp-midi-in")?;
        let mut selected_port = None;
        info!("MIDI Input Ports:");
        for port in midi_in.ports() {
            info!("  - {}", midi_in.port_name(&port)?);
            if selected_port.is_none()
                && midi_in
                    .port_name(&port)?
                    .to_lowercase()
                    .contains(&name.unwrap_or("").to_lowercase())
            {
                selected_port = Some(port);
            }
        }
        let port = match selected_port {
            Some(port) => port,
            None => {
                if let Some(port) = midi_in.ports().first() {
                    info!("No MIDI Input Port selected, using first available");
                    port.clone()
                } else {
                    return Err("No MIDI Input Ports found".into());
                }
            }
        };
        info!("Selected MIDI Input Port: {}", midi_in.port_name(&port)?);
        Ok(WispMidiIn { midi_in, port })
    }
}
