use crate::host::{key_from_name, AudioStretcher, EmuThread, GbaMachine, BIOS_SIZE, GBA_FPS, OPEN_SOURCE_BIOS};
use godot::classes::{INode, Image, ImageTexture};
use godot::prelude::*;

/// Maximum GBA frames emulated in a single Godot frame.
/// Avoids the "spiral of death" when the browser gets slow: instead of trying
/// to catch up on all the lost time, the game slows down.
const MAX_FRAMES_PER_TICK: u32 = 4;

/// Real time (seconds) one tick may spend emulating when catching up. Enough for
/// one skipped + one drawn frame on a slow phone (~17 + 23 ms).
const CATCH_UP_BUDGET: f64 = 0.045;

/// Where the console runs.
enum Backend {
    /// On Godot's main thread, inside `process` (Web exports without Thread Support).
    Sync(GbaMachine),
    /// On its own thread, paced by the real clock (everything else).
    Threaded(EmuThread),
}

impl Backend {
    /// Direct access to the machine (the emulation thread, if any, waits meanwhile).
    fn with_machine<R>(&mut self, f: impl FnOnce(&mut GbaMachine) -> R) -> R {
        match self {
            Backend::Sync(m) => f(m),
            Backend::Threaded(t) => t.with_machine(f),
        }
    }
}

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
    machine: Option<Backend>,
    /// Serial number of the last picture taken from the emulation thread.
    video_serial: u64,
    /// BIOS used by the next load_rom (defaults to the embedded Cult-of-GBA one).
    bios: Vec<u8>,
    /// Copy of the ROM, for reset.
    rom: Vec<u8>,
    texture: Option<Gd<ImageTexture>>,
    rgb: Vec<u8>,
    audio: Vec<Vector2>,
    /// Keeps the sound continuous when the device is too slow for full speed.
    stretcher: AudioStretcher,
    last_audio_call: Option<std::time::Instant>,
    /// GBA frames emulated since boot (for the stats overlay).
    frame_count: u64,
    /// Time spent emulating since the last get_emulation_ms() call (stats overlay).
    emu_time: f64,
    emu_time_frames: u32,
    /// Real time (seconds) the last drawn / skipped frame took, to plan the next tick.
    draw_frame_time: f64,
    skip_frame_time: f64,
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
            video_serial: 0,
            bios: OPEN_SOURCE_BIOS.to_vec(),
            rom: Vec::new(),
            texture: None,
            rgb: Vec::new(),
            audio: Vec::new(),
            stretcher: AudioStretcher::new(),
            last_audio_call: None,
            frame_count: 0,
            emu_time: 0.0,
            emu_time_frames: 0,
            draw_frame_time: 0.0,
            skip_frame_time: 0.0,
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
        let machine = match self.machine.as_mut() {
            None => return,
            Some(Backend::Threaded(thread)) => {
                // The console runs on its own thread: just show its latest picture.
                thread.set_speed(self.speed.max(1.0) as u32);
                if thread.take_video(&mut self.rgb, &mut self.video_serial) {
                    self.upload_texture();
                }
                return;
            }
            Some(Backend::Sync(machine)) => machine,
        };

        let frame_time = 1.0 / GBA_FPS;
        self.time_debt += delta * self.speed.max(1.0);

        let max_frames = MAX_FRAMES_PER_TICK * self.speed.max(1.0) as u32;
        let mut frames = 0;
        let started = std::time::Instant::now();
        // Several frames in one tick (catching up on a slow device, or fast forward):
        // only the last one is drawn, the others are emulated without video (frame
        // skip), which is ~30% cheaper. The game and its sound are not affected.
        // The tick also has a time budget: on a device that cannot emulate in real
        // time, running many frames per tick only makes every tick longer
        // (4 frames x 23 ms = 9 fps on screen).
        let budget = CATCH_UP_BUDGET * self.speed.max(1.0);
        while self.time_debt >= frame_time && frames < max_frames {
            let frame_started = started.elapsed().as_secs_f64();
            // Is there time for one more skipped frame AND the drawn one after it?
            let skip_time = if self.skip_frame_time > 0.0 {
                self.skip_frame_time
            } else {
                self.draw_frame_time * 0.7
            };
            let out_of_time = frames + 1 == max_frames
                || frame_started + skip_time + self.draw_frame_time > budget;
            let last = out_of_time || self.time_debt - frame_time < frame_time;
            if last {
                machine.run_frame();
                self.draw_frame_time = started.elapsed().as_secs_f64() - frame_started;
            } else {
                machine.run_frame_no_video();
                self.skip_frame_time = started.elapsed().as_secs_f64() - frame_started;
            }
            self.time_debt -= frame_time;
            frames += 1;
            self.frame_count += 1;
            if out_of_time {
                // We can't keep up with real time: forget the backlog.
                self.time_debt = self.time_debt.min(frame_time);
                break;
            }
        }
        self.emu_time += started.elapsed().as_secs_f64();
        self.emu_time_frames += frames;

        if frames > 0 {
            machine.frame_rgb(&mut self.rgb);
            self.upload_texture();
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
            self.release_all_buttons();
        }
        if let Some(Backend::Threaded(t)) = self.machine.as_ref() {
            t.set_paused(value);
        }
    }

    /// True when the console runs on its own thread (false: inside Godot's main loop).
    #[func]
    pub fn is_threaded(&self) -> bool {
        matches!(self.machine, Some(Backend::Threaded(_)))
    }

    #[func]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Presses or releases a button: "A", "B", "L", "R", "START", "SELECT", "UP", "DOWN", "LEFT", "RIGHT".
    #[func]
    pub fn send_button(&mut self, name: GString, pressed: bool) {
        let Some(key) = key_from_name(&name.to_string()) else {
            godot_warn!("[GBA] Unknown button: {}", name);
            return;
        };
        match self.machine.as_mut() {
            Some(Backend::Sync(m)) => m.set_key(key, pressed),
            Some(Backend::Threaded(t)) => t.set_key(key, pressed),
            None => {}
        }
    }

    #[func]
    pub fn release_all_buttons(&mut self) {
        match self.machine.as_mut() {
            Some(Backend::Sync(m)) => m.release_all_keys(),
            Some(Backend::Threaded(t)) => t.release_all_keys(),
            None => {}
        }
    }

    /// Texture with the current screen (240x160). Always the same texture, updated every frame.
    #[func]
    pub fn get_texture(&self) -> Option<Gd<ImageTexture>> {
        self.texture.clone()
    }

    /// Audio accumulated since the last call, as stereo frames (x = left, y = right)
    /// at 44100 Hz, already filtered (DC blocker) and clamped. Push straight into
    /// an AudioStreamGeneratorPlayback.
    #[func]
    pub fn get_audio_frames(&mut self) -> PackedVector2Array {
        self.audio.clear();
        let audio = &mut self.audio;
        let stretcher = &mut self.stretcher;
        let mut produced = 0;
        let mut out = |l: f32, r: f32| audio.push(Vector2::new(l, r));
        let mut push = |l: f32, r: f32| {
            produced += 1;
            stretcher.push(l, r, &mut out);
        };
        match self.machine.as_mut() {
            Some(Backend::Sync(m)) => m.drain_audio_frames(&mut push),
            Some(Backend::Threaded(t)) => t.take_audio(&mut push),
            None => {}
        }

        // Measure the real emulation speed from the sound actually produced.
        let now = std::time::Instant::now();
        let elapsed = self.last_audio_call.map(|t| (now - t).as_secs_f64()).unwrap_or(0.0);
        self.last_audio_call = Some(now);
        if self.paused || self.speed > 1.0 || elapsed > 0.5 {
            self.stretcher.reset();
        } else {
            self.stretcher.measure(elapsed, produced);
        }
        PackedVector2Array::from(self.audio.as_slice())
    }

    /// Speed the sound is being played at (1.0 = normal). Below 1.0 the device is
    /// too slow for full speed and the sound is stretched to stay continuous.
    #[func]
    pub fn get_audio_speed(&self) -> f64 {
        self.stretcher.ratio() as f64
    }

    /// GBA frames emulated so far (to measure the real emulation speed).
    #[func]
    pub fn get_frame_count(&self) -> i64 {
        match self.machine.as_ref() {
            Some(Backend::Threaded(t)) => t.frame_count() as i64,
            _ => self.frame_count as i64,
        }
    }

    /// Average real time (ms) taken to emulate one GBA frame since the last call.
    /// Must stay well under 16.7 ms for full speed.
    #[func]
    pub fn get_emulation_ms(&mut self) -> f64 {
        if let Some(Backend::Threaded(t)) = self.machine.as_ref() {
            return t.take_emulation_ms();
        }
        let ms = if self.emu_time_frames > 0 {
            self.emu_time * 1000.0 / self.emu_time_frames as f64
        } else {
            0.0
        };
        self.emu_time = 0.0;
        self.emu_time_frames = 0;
        ms
    }

    /// Saves the full console state (includes the in-game save). Empty on failure.
    #[func]
    pub fn save_state(&mut self) -> PackedByteArray {
        if let Some(backend) = self.machine.as_mut() {
            match backend.with_machine(|m| m.gba.save_state()) {
                Ok(bytes) => return PackedByteArray::from(bytes.as_slice()),
                Err(e) => godot_error!("[GBA] Failed to save the state: {:?}", e),
            }
        }
        PackedByteArray::new()
    }

    /// Restores a state saved with `save_state` (from the same game).
    #[func]
    pub fn load_state(&mut self, data: PackedByteArray) -> bool {
        let Some(backend) = self.machine.as_mut() else { return false };
        match backend.with_machine(|m| m.gba.restore_state(data.as_slice())) {
            Ok(()) => {
                match backend {
                    Backend::Sync(m) => m.clear_audio(),
                    Backend::Threaded(t) => t.clear_audio(),
                }
                self.time_debt = 0.0;
                self.show_current_frame();
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
    pub fn get_game_title(&mut self) -> GString {
        let title = self
            .machine
            .as_mut()
            .map(|b| b.with_machine(|m| m.gba.get_game_title()))
            .map(|t| t.trim_end_matches('\0').trim().to_string())
            .unwrap_or_default();
        GString::from(title.as_str())
    }

    /// 4-letter cartridge code (e.g. "AXVE").
    #[func]
    pub fn get_game_code(&mut self) -> GString {
        let code = self
            .machine
            .as_mut()
            .map(|b| b.with_machine(|m| m.gba.get_game_code()))
            .unwrap_or_default();
        GString::from(code.as_str())
    }
}

impl GBAEmulator {
    fn boot(&mut self) -> bool {
        // Stop the previous console (and its thread) first.
        self.machine = None;
        match GbaMachine::new(&self.bios, &self.rom, self.skip_bios) {
            Ok(machine) => {
                // Own thread whenever threads exist; the Web export without Thread
                // Support ("nothreads" build) runs inside `process` instead.
                let backend = if cfg!(feature = "nothreads") {
                    Backend::Sync(machine)
                } else {
                    match EmuThread::spawn(machine) {
                        Ok(thread) => {
                            thread.set_paused(self.paused);
                            Backend::Threaded(thread)
                        }
                        Err(machine) => {
                            godot_warn!("[GBA] No threads available: emulating on the main thread");
                            Backend::Sync(machine)
                        }
                    }
                };
                self.machine = Some(backend);
                self.time_debt = 0.0;
                self.frame_count = 0;
                self.show_current_frame();
                true
            }
            Err(e) => {
                godot_error!("[GBA] {}", e);
                false
            }
        }
    }

    /// Reads the machine's current picture and shows it (boot, load state).
    fn show_current_frame(&mut self) {
        let Some(backend) = self.machine.as_mut() else { return };
        let rgb = &mut self.rgb;
        backend.with_machine(|m| m.frame_rgb(rgb));
        self.upload_texture();
    }

    /// Sends `self.rgb` (240x160 RGB8) to the texture.
    fn upload_texture(&mut self) {
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
