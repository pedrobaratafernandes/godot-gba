//! Glue between the rustboyadvance-core emulator and Godot, with no Godot
//! dependency (so it can be tested with `cargo test`).

use rustboyadvance_core::keypad::{Keys, KEYINPUT_ALL_RELEASED};
use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::audio::SampleConsumer;
use rustboyadvance_utils::Consumer;

/// Free "Cult-of-GBA" BIOS (MIT license, see bios/LICENSE).
/// Embedded in the library so the emulator boots without Nintendo's official BIOS.
pub static OPEN_SOURCE_BIOS: &[u8] = include_bytes!("../bios/cult-of-gba-bios.bin");

/// Size of a GBA BIOS (16 KB).
pub const BIOS_SIZE: usize = 16 * 1024;

/// Sample rate of the generated audio (must match Godot's AudioStreamGenerator).
pub const SAMPLE_RATE: i32 = 44100;

/// Audio buffer capacity (i16 samples, left/right interleaved).
/// ~0.37 s of sound: enough for Godot never to run dry, even when frames are dropped.
const AUDIO_BUFFER_SAMPLES: usize = 32768;

/// Real GBA refresh rate: 16.78 MHz / 280896 cycles per frame = ~59.73 Hz.
pub const GBA_FPS: f64 = 16_777_216.0 / 280_896.0;

/// A running GBA console: the core plus its audio output.
pub struct GbaMachine {
    pub gba: GameBoyAdvance,
    audio: SampleConsumer,
    /// Button state in KEYINPUT register format (bit = 0 means pressed).
    keys: u16,
}

impl GbaMachine {
    /// Creates the machine with the given BIOS and ROM. The ROM may be .gba or .zip.
    pub fn new(bios: &[u8], rom: &[u8], skip_bios: bool) -> Result<Self, String> {
        if bios.len() != BIOS_SIZE {
            return Err(format!("Invalid BIOS: {} bytes (expected {})", bios.len(), BIOS_SIZE));
        }

        // No save file: the Web has no regular file system.
        // In-game saves (SRAM/Flash/EEPROM) are stored inside save states.
        let gamepak = GamepakBuilder::new()
            .buffer(rom)
            .without_backup_to_file()
            .build()
            .map_err(|e| format!("Failed to load the ROM: {:?}", e))?;

        let (audio_device, audio) =
            SimpleAudioInterface::create_channel(SAMPLE_RATE, Some(AUDIO_BUFFER_SAMPLES));

        let mut gba = GameBoyAdvance::new(bios.to_vec().into_boxed_slice(), gamepak, audio_device);
        if skip_bios {
            gba.skip_bios();
        }

        Ok(Self { gba, audio, keys: KEYINPUT_ALL_RELEASED })
    }

    /// Emulates one full frame (~16.7 ms of GBA time).
    pub fn run_frame(&mut self) {
        *self.gba.get_key_state_mut() = self.keys;
        self.gba.frame();
    }

    /// Presses (`pressed = true`) or releases a button.
    pub fn set_key(&mut self, key: Keys, pressed: bool) {
        let bit = 1u16 << (key as u16);
        if pressed {
            self.keys &= !bit;
        } else {
            self.keys |= bit;
        }
    }

    /// Releases every button (e.g. when the pause menu opens).
    pub fn release_all_keys(&mut self) {
        self.keys = KEYINPUT_ALL_RELEASED;
    }

    /// Current screen converted to RGB8 (3 bytes per pixel), ready for a Godot Image.
    pub fn frame_rgb(&self, out: &mut Vec<u8>) {
        let fb = self.gba.get_frame_buffer();
        out.resize(fb.len() * 3, 0);
        for (px, color) in out.chunks_exact_mut(3).zip(fb.iter()) {
            px[0] = (color >> 16) as u8;
            px[1] = (color >> 8) as u8;
            px[2] = *color as u8;
        }
    }

    /// Takes the accumulated audio, as interleaved f32 (left, right, ...).
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        while let Some(sample) = self.audio.try_pop() {
            out.push(sample as f32 / 32768.0);
        }
    }

    /// Discards the accumulated audio (e.g. after loading a state).
    pub fn clear_audio(&mut self) {
        while self.audio.try_pop().is_some() {}
    }
}

/// Converts a button name (coming from GDScript) to the GBA key.
pub fn key_from_name(name: &str) -> Option<Keys> {
    match name.to_uppercase().as_str() {
        "A" => Some(Keys::ButtonA),
        "B" => Some(Keys::ButtonB),
        "L" => Some(Keys::ButtonL),
        "R" => Some(Keys::ButtonR),
        "START" => Some(Keys::Start),
        "SELECT" => Some(Keys::Select),
        "UP" => Some(Keys::Up),
        "DOWN" => Some(Keys::Down),
        "LEFT" => Some(Keys::Left),
        "RIGHT" => Some(Keys::Right),
        _ => None,
    }
}
