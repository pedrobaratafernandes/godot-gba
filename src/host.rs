//! Glue between the rustboyadvance-core emulator and Godot, with no Godot
//! dependency (so it can be tested with `cargo test`).

use rustboyadvance_core::keypad::{Keys, KEYINPUT_ALL_RELEASED};
use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::audio::SampleConsumer;
use rustboyadvance_utils::Consumer;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
    /// DC blocker state (left, right): last raw sample and last filtered sample.
    dc_last_raw: [f32; 2],
    dc_filtered: [f32; 2],
}

/// DC blocker pole: removes the GBA's DC offset (prevents pops/clicks) without
/// touching audible frequencies (cut-off ~35 Hz at 44100 Hz).
const DC_BLOCKER_POLE: f32 = 0.995;

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

        Ok(Self {
            gba,
            audio,
            keys: KEYINPUT_ALL_RELEASED,
            dc_last_raw: [0.0; 2],
            dc_filtered: [0.0; 2],
        })
    }

    /// Emulates one full frame (~16.7 ms of GBA time).
    pub fn run_frame(&mut self) {
        *self.gba.get_key_state_mut() = self.keys;
        self.gba.frame();
    }

    /// Emulates one full frame without drawing it (frame skip). The game, its timing
    /// and its sound advance exactly as in `run_frame`; only the screen is not updated.
    pub fn run_frame_no_video(&mut self) {
        self.gba.set_skip_render(true);
        self.run_frame();
        self.gba.set_skip_render(false);
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

    /// Sets every button at once, in KEYINPUT format (bit = 0 means pressed).
    pub fn set_keys(&mut self, keys: u16) {
        self.keys = keys;
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
    #[cfg(test)]
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        while let Some(sample) = self.audio.try_pop() {
            out.push(sample as f32 / 32768.0);
        }
    }

    /// Takes the accumulated audio as stereo frames (left, right), already passed
    /// through the DC blocker and clamped to [-1, 1]. Ready for AudioStreamGenerator.
    pub fn drain_audio_frames(&mut self, mut push: impl FnMut(f32, f32)) {
        let mut frame = [0.0f32; 2];
        let mut channel = 0;
        while let Some(sample) = self.audio.try_pop() {
            frame[channel] = sample as f32 / 32768.0;
            channel += 1;
            if channel == 2 {
                channel = 0;
                let mut out = [0.0f32; 2];
                for c in 0..2 {
                    self.dc_filtered[c] =
                        frame[c] - self.dc_last_raw[c] + self.dc_filtered[c] * DC_BLOCKER_POLE;
                    self.dc_last_raw[c] = frame[c];
                    out[c] = self.dc_filtered[c].clamp(-1.0, 1.0);
                }
                push(out[0], out[1]);
            }
        }
    }

    /// Discards the accumulated audio (e.g. after loading a state).
    pub fn clear_audio(&mut self) {
        while self.audio.try_pop().is_some() {}
    }
}

/// Makes the sound follow the real emulation speed.
///
/// On a device too slow for full speed, the console produces less than one second
/// of sound per second, so the sound card runs dry and the sound breaks up. This
/// measures the real speed and stretches the sound to match: at 70% speed it plays
/// 30% slower (and lower), but continuously. At full speed it changes nothing.
pub struct AudioStretcher {
    /// Emulation speed the sound is currently stretched for (1.0 = full speed).
    ratio: f32,
    prev: (f32, f32),
    pos: f32,
    /// Measurement window: seconds elapsed and sound frames produced in it.
    window_time: f64,
    window_frames: u64,
}

impl AudioStretcher {
    /// Above this measured speed the sound is left untouched.
    const FULL_SPEED: f32 = 0.95;
    const WINDOW_SECONDS: f64 = 1.0;

    pub fn new() -> Self {
        Self { ratio: 1.0, prev: (0.0, 0.0), pos: 0.0, window_time: 0.0, window_frames: 0 }
    }

    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// Back to full speed (pause, reset, fast forward).
    pub fn reset(&mut self) {
        self.ratio = 1.0;
        self.window_time = 0.0;
        self.window_frames = 0;
    }

    /// Tells the stretcher that `frames` sound frames were produced in `seconds`
    /// of real time, and updates the speed estimate once per window.
    pub fn measure(&mut self, seconds: f64, frames: usize) {
        self.window_time += seconds;
        self.window_frames += frames as u64;
        if self.window_time < Self::WINDOW_SECONDS {
            return;
        }
        let speed = (self.window_frames as f64 / (self.window_time * SAMPLE_RATE as f64)) as f32;
        self.window_time = 0.0;
        self.window_frames = 0;
        let target = if speed >= Self::FULL_SPEED { 1.0 } else { speed.max(0.4) };
        // Follow the measured speed smoothly, but return to normal pitch at once.
        self.ratio = if target >= 1.0 { 1.0 } else { self.ratio + (target - self.ratio) * 0.6 };
    }

    /// Stretches one input frame into zero or more output frames (linear interpolation).
    #[inline]
    pub fn push(&mut self, l: f32, r: f32, out: &mut impl FnMut(f32, f32)) {
        if self.ratio >= 1.0 {
            self.prev = (l, r);
            self.pos = 0.0;
            out(l, r);
            return;
        }
        while self.pos < 1.0 {
            let t = self.pos;
            out(self.prev.0 + (l - self.prev.0) * t, self.prev.1 + (r - self.prev.1) * t);
            self.pos += self.ratio;
        }
        self.pos -= 1.0;
        self.prev = (l, r);
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

// ==============================================================================
// Emulation on its own thread
// ------------------------------------------------------------------------------
// The console runs on a dedicated thread, paced by the real clock, so it does not
// share its CPU time with Godot's drawing, UI and audio mixing (on a phone browser
// that sharing is what kept the game far below full speed). The Godot side only
// picks up the latest picture and the pending sound, and sets the buttons.
// ==============================================================================

/// Most frames in a row emulated without drawing when the thread is behind.
const MAX_FRAME_SKIP: u32 = 2;
/// Sound kept for the Godot side at most (stereo frames, ~0.5 s). Older sound is dropped.
const MAX_QUEUED_AUDIO: usize = 22050;

/// The machine holds `Rc`s and raw pointers, so it is not `Send` by itself. It is
/// only ever touched while holding the mutex around it, one thread at a time.
struct SendMachine(GbaMachine);
unsafe impl Send for SendMachine {}

struct Shared {
    machine: Mutex<SendMachine>,
    /// Button state in KEYINPUT format, read by the thread before every frame.
    keys: AtomicU16,
    speed: AtomicU32,
    paused: AtomicBool,
    quit: AtomicBool,
    frame_count: AtomicU64,
    emu_nanos: AtomicU64,
    emu_frames: AtomicU32,
    audio: Mutex<Vec<(f32, f32)>>,
    /// Latest drawn picture (RGB8) and its serial number.
    video: Mutex<(Vec<u8>, u64)>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A GBA console running on its own thread.
pub struct EmuThread {
    shared: Arc<Shared>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl EmuThread {
    /// Starts the thread. Gives the machine back if threads are not available
    /// (e.g. a Web export without Thread Support).
    pub fn spawn(machine: GbaMachine) -> Result<Self, GbaMachine> {
        let shared = Arc::new(Shared {
            machine: Mutex::new(SendMachine(machine)),
            keys: AtomicU16::new(KEYINPUT_ALL_RELEASED),
            speed: AtomicU32::new(1),
            paused: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            frame_count: AtomicU64::new(0),
            emu_nanos: AtomicU64::new(0),
            emu_frames: AtomicU32::new(0),
            audio: Mutex::new(Vec::new()),
            video: Mutex::new((Vec::new(), 0)),
        });
        let for_thread = shared.clone();
        match std::thread::Builder::new()
            .name("gba-emulator".into())
            .spawn(move || emulation_loop(&for_thread))
        {
            Ok(handle) => Ok(Self { shared, handle: Some(handle) }),
            Err(_) => match Arc::try_unwrap(shared) {
                Ok(shared) => {
                    Err(shared.machine.into_inner().unwrap_or_else(|e| e.into_inner()).0)
                }
                Err(_) => unreachable!("the closure (and its Arc) was dropped by the failed spawn"),
            },
        }
    }

    /// Runs `f` on the machine while the emulation thread waits (save states, title...).
    pub fn with_machine<R>(&self, f: impl FnOnce(&mut GbaMachine) -> R) -> R {
        f(&mut lock(&self.shared.machine).0)
    }

    pub fn set_key(&self, key: Keys, pressed: bool) {
        let bit = 1u16 << (key as u16);
        if pressed {
            self.shared.keys.fetch_and(!bit, Ordering::Relaxed);
        } else {
            self.shared.keys.fetch_or(bit, Ordering::Relaxed);
        }
    }

    pub fn release_all_keys(&self) {
        self.shared.keys.store(KEYINPUT_ALL_RELEASED, Ordering::Relaxed);
    }

    /// Speed multiplier (1 = normal). Used for fast forward.
    pub fn set_speed(&self, speed: u32) {
        self.shared.speed.store(speed.clamp(1, 8), Ordering::Relaxed);
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// Takes the sound produced since the last call (filtered stereo frames).
    pub fn take_audio(&self, mut push: impl FnMut(f32, f32)) {
        let mut audio = lock(&self.shared.audio);
        for &(l, r) in audio.iter() {
            push(l, r);
        }
        audio.clear();
    }

    /// Discards the pending sound (e.g. after loading a state).
    pub fn clear_audio(&self) {
        self.with_machine(|m| m.clear_audio());
        lock(&self.shared.audio).clear();
    }

    /// Copies the latest picture into `rgb` if there is a newer one than `serial`.
    pub fn take_video(&self, rgb: &mut Vec<u8>, serial: &mut u64) -> bool {
        let video = lock(&self.shared.video);
        if video.1 == *serial || video.0.is_empty() {
            return false;
        }
        rgb.clear();
        rgb.extend_from_slice(&video.0);
        *serial = video.1;
        true
    }

    pub fn frame_count(&self) -> u64 {
        self.shared.frame_count.load(Ordering::Relaxed)
    }

    /// Average real time (ms) per emulated frame since the last call.
    pub fn take_emulation_ms(&self) -> f64 {
        let nanos = self.shared.emu_nanos.swap(0, Ordering::Relaxed);
        let frames = self.shared.emu_frames.swap(0, Ordering::Relaxed);
        if frames == 0 { 0.0 } else { nanos as f64 / 1e6 / frames as f64 }
    }
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Relaxed);
        // On the Web the main thread must not block waiting for another thread:
        // the emulation thread stops by itself within one frame.
        if let Some(handle) = self.handle.take() {
            if cfg!(not(target_family = "wasm")) {
                let _ = handle.join();
            }
        }
    }
}

fn emulation_loop(shared: &Shared) {
    let mut next = Instant::now();
    let mut skipped = 0;
    let mut rgb = Vec::new();
    let mut audio = Vec::new();

    while !shared.quit.load(Ordering::Relaxed) {
        if shared.paused.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(4));
            next = Instant::now();
            continue;
        }
        let speed = shared.speed.load(Ordering::Relaxed).max(1);
        let step = Duration::from_secs_f64(1.0 / GBA_FPS / speed as f64);

        let now = Instant::now();
        if now < next {
            std::thread::sleep(next - now);
            continue;
        }
        // Behind schedule: emulate without drawing (~30% cheaper) to catch up,
        // but never more than MAX_FRAME_SKIP frames in a row.
        let behind = now - next;
        let draw = behind < step || skipped >= MAX_FRAME_SKIP;

        {
            let mut machine = lock(&shared.machine);
            let m = &mut machine.0;
            m.set_keys(shared.keys.load(Ordering::Relaxed));
            if draw {
                m.run_frame();
                m.frame_rgb(&mut rgb);
            } else {
                m.run_frame_no_video();
            }
            m.drain_audio_frames(|l, r| audio.push((l, r)));
        }
        skipped = if draw { 0 } else { skipped + 1 };
        shared.emu_nanos.fetch_add(now.elapsed().as_nanos() as u64, Ordering::Relaxed);
        shared.emu_frames.fetch_add(1, Ordering::Relaxed);
        shared.frame_count.fetch_add(1, Ordering::Relaxed);

        {
            let mut queue = lock(&shared.audio);
            queue.append(&mut audio);
            if queue.len() > MAX_QUEUED_AUDIO {
                let excess = queue.len() - MAX_QUEUED_AUDIO;
                queue.drain(..excess);
            }
        }
        if draw {
            let mut video = lock(&shared.video);
            std::mem::swap(&mut video.0, &mut rgb);
            video.1 += 1;
        }

        next += step;
        // Too slow for real time: forget the backlog instead of piling it up.
        let now = Instant::now();
        if now > next + step * 4 {
            next = now;
        }
    }
}
