use crate::host::{key_from_name, GbaMachine, BIOS_SIZE, GBA_FPS, OPEN_SOURCE_BIOS};
use godot::classes::{INode, Image, ImageTexture};
use godot::prelude::*;

/// Maximum GBA frames emulated in a single Godot frame.
/// Avoids the "spiral of death" when the browser gets slow: instead of trying
/// to catch up on all the lost time, the game slows down.
const MAX_FRAMES_PER_TICK: u32 = 4;

/// Godot Node representing a Game Boy Advance.
///
/// Typical GDScript usage:
/// ```gdscript
/// emulator.load_bios(FileAccess.get_file_as_bytes("res://roms/gba_bios.bin")) # optional
/// emulator.load_rom(FileAccess.get_file_as_bytes("res://games/game.gba"))
/// display.texture = emulator.get_texture()
/// emulator.send_button("A", true)
/// ```
#[derive(GodotClass)]
#[class(base=Node)]
pub struct GBAEmulator {
    machine: Option<GbaMachine>,
    /// BIOS used by the next load_rom (defaults to the embedded Cult-of-GBA one).
    bios: Vec<u8>,
    /// Copy of the ROM, for reset.
    rom: Vec<u8>,
    texture: Option<Gd<ImageTexture>>,
    rgb: Vec<u8>,
    audio: Vec<f32>,
    /// Accumulated real time that still has to be emulated (seconds).
    time_debt: f64,
    paused: bool,
    /// Skips the BIOS boot animation and starts the game right away.
    #[export]
    skip_bios: bool,
    /// Speed multiplier (1 = normal, 2 = double...). Used for fast forward.
    #[export(range = (1.0, 4.0, 1.0))]
    speed: f64,
    base: Base<Node>,
}

#[godot_api]
impl INode for GBAEmulator {
    fn init(base: Base<Node>) -> Self {
        Self {
            machine: None,
            bios: OPEN_SOURCE_BIOS.to_vec(),
            rom: Vec::new(),
            texture: None,
            rgb: Vec::new(),
            audio: Vec::new(),
            time_debt: 0.0,
            paused: false,
            skip_bios: true,
            speed: 1.0,
            base,
        }
    }

    fn process(&mut self, delta: f64) {
        if self.paused {
            return;
        }
        let Some(machine) = self.machine.as_mut() else { return };

        let frame_time = 1.0 / GBA_FPS;
        self.time_debt += delta * self.speed.max(1.0);

        let max_frames = MAX_FRAMES_PER_TICK * self.speed.max(1.0) as u32;
        let mut frames = 0;
        while self.time_debt >= frame_time && frames < max_frames {
            machine.run_frame();
            self.time_debt -= frame_time;
            frames += 1;
        }
        if frames == max_frames {
            // We can't keep up with real time: forget the backlog.
            self.time_debt = 0.0;
        }

        if frames > 0 {
            self.update_texture();
        }
    }
}

#[godot_api]
impl GBAEmulator {
    /// Sets the BIOS (16 KB) used by the next `load_rom`, e.g. the official gba_bios.bin.
    /// Returns false if the size is wrong (and keeps the previous BIOS).
    #[func]
    pub fn load_bios(&mut self, data: PackedByteArray) -> bool {
        if data.len() != BIOS_SIZE {
            godot_error!("[GBA] Invalid BIOS: {} bytes (expected {})", data.len(), BIOS_SIZE);
            return false;
        }
        self.bios = data.to_vec();
        true
    }

    /// Goes back to the embedded free BIOS (Cult-of-GBA).
    #[func]
    pub fn use_open_source_bios(&mut self) {
        self.bios = OPEN_SOURCE_BIOS.to_vec();
    }

    /// Loads a ROM (.gba, or a .zip containing a .gba) and powers the console on.
    #[func]
    pub fn load_rom(&mut self, data: PackedByteArray) -> bool {
        self.rom = data.to_vec();
        self.boot()
    }

    /// Powers the console off and on again with the same ROM.
    #[func]
    pub fn reset(&mut self) -> bool {
        if self.rom.is_empty() {
            return false;
        }
        self.boot()
    }

    #[func]
    pub fn is_running(&self) -> bool {
        self.machine.is_some()
    }

    #[func]
    pub fn set_paused(&mut self, value: bool) {
        self.paused = value;
        self.time_debt = 0.0;
        if value {
            if let Some(m) = self.machine.as_mut() {
                m.release_all_keys();
            }
        }
    }

    #[func]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Presses or releases a button: "A", "B", "L", "R", "START", "SELECT", "UP", "DOWN", "LEFT", "RIGHT".
    #[func]
    pub fn send_button(&mut self, name: GString, pressed: bool) {
        let Some(machine) = self.machine.as_mut() else { return };
        match key_from_name(&name.to_string()) {
            Some(key) => machine.set_key(key, pressed),
            None => godot_warn!("[GBA] Unknown button: {}", name),
        }
    }

    #[func]
    pub fn release_all_buttons(&mut self) {
        if let Some(m) = self.machine.as_mut() {
            m.release_all_keys();
        }
    }

    /// Texture with the current screen (240x160). Always the same texture, updated every frame.
    #[func]
    pub fn get_texture(&self) -> Option<Gd<ImageTexture>> {
        self.texture.clone()
    }

    /// Audio accumulated since the last call: interleaved stereo (left, right, ...) at 44100 Hz.
    #[func]
    pub fn get_audio_samples(&mut self) -> PackedFloat32Array {
        self.audio.clear();
        if let Some(m) = self.machine.as_mut() {
            m.drain_audio(&mut self.audio);
        }
        PackedFloat32Array::from(self.audio.as_slice())
    }

    /// Saves the full console state (includes the in-game save). Empty on failure.
    #[func]
    pub fn save_state(&mut self) -> PackedByteArray {
        if let Some(m) = self.machine.as_ref() {
            match m.gba.save_state() {
                Ok(bytes) => return PackedByteArray::from(bytes.as_slice()),
                Err(e) => godot_error!("[GBA] Failed to save the state: {:?}", e),
            }
        }
        PackedByteArray::new()
    }

    /// Restores a state saved with `save_state` (from the same game).
    #[func]
    pub fn load_state(&mut self, data: PackedByteArray) -> bool {
        let Some(m) = self.machine.as_mut() else { return false };
        match m.gba.restore_state(data.as_slice()) {
            Ok(()) => {
                m.clear_audio();
                self.time_debt = 0.0;
                self.update_texture();
                true
            }
            Err(e) => {
                godot_error!("[GBA] Failed to load the state: {:?}", e);
                false
            }
        }
    }

    /// Internal cartridge title (from the ROM header).
    #[func]
    pub fn get_game_title(&self) -> GString {
        let title = self
            .machine
            .as_ref()
            .map(|m| m.gba.get_game_title().trim_end_matches('\0').trim().to_string())
            .unwrap_or_default();
        GString::from(title.as_str())
    }

    /// 4-letter cartridge code (e.g. "AXVE").
    #[func]
    pub fn get_game_code(&self) -> GString {
        let code = self.machine.as_ref().map(|m| m.gba.get_game_code()).unwrap_or_default();
        GString::from(code.as_str())
    }
}

impl GBAEmulator {
    fn boot(&mut self) -> bool {
        match GbaMachine::new(&self.bios, &self.rom, self.skip_bios) {
            Ok(machine) => {
                self.machine = Some(machine);
                self.time_debt = 0.0;
                self.update_texture();
                true
            }
            Err(e) => {
                godot_error!("[GBA] {}", e);
                self.machine = None;
                false
            }
        }
    }

    fn update_texture(&mut self) {
        let Some(machine) = self.machine.as_ref() else { return };
        machine.frame_rgb(&mut self.rgb);

        let Some(image) = Image::create_from_data(
            240,
            160,
            false,
            godot::classes::image::Format::RGB8,
            &PackedByteArray::from(self.rgb.as_slice()),
        ) else {
            return;
        };

        match self.texture.as_mut() {
            Some(tex) => tex.update(&image),
            None => self.texture = ImageTexture::create_from_image(&image),
        }
    }
}
