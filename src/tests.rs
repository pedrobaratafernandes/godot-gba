//! Core tests (no Godot): `cargo test --release`
//!
//! They use a tiny ROM assembled here (hand-written ARM code) that switches the
//! screen to mode 3 (240x160 bitmap) and paints it red. They also test every
//! game found in GodotProject/games/.

use crate::host::*;
use rustboyadvance_core::keypad::{Keys, KEYINPUT_ALL_RELEASED};

/// Builds a test ROM with a valid header.
fn red_screen_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x200];
    let mut put = |addr: usize, word: u32| rom[addr..addr + 4].copy_from_slice(&word.to_le_bytes());

    // 0x000: b 0xC0 (jump over the header)
    put(0x00, 0xEA00002E);

    // 0xC0: code
    let code: [u32; 11] = [
        0xE3A00404, // mov  r0, #0x04000000   ; I/O registers
        0xE3A01B01, // mov  r1, #0x400        ; BG2 on
        0xE3811003, // orr  r1, r1, #3        ; mode 3 (16-bit bitmap)
        0xE1C010B0, // strh r1, [r0]          ; DISPCNT
        0xE3A00406, // mov  r0, #0x06000000   ; VRAM
        0xE3A0101F, // mov  r1, #0x1F         ; red (RGB15)
        0xE3A02C96, // mov  r2, #0x9600       ; 240*160 pixels
        0xE0C010B2, // loop: strh r1, [r0], #2
        0xE2522001, // subs r2, r2, #1
        0x1AFFFFFC, // bne  loop
        0xEAFFFFFE, // end: b end
    ];
    for (i, w) in code.iter().enumerate() {
        put(0xC0 + i * 4, *w);
    }

    // Header: title, game code, maker, fixed value and checksum.
    rom[0xA0..0xAC].copy_from_slice(b"GODOTGBATEST");
    rom[0xAC..0xB0].copy_from_slice(b"TEST");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    let checksum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, b| acc.wrapping_sub(*b))
        .wrapping_sub(0x19);
    rom[0xBD] = checksum;
    rom
}

fn run(machine: &mut GbaMachine, frames: usize) {
    for _ in 0..frames {
        machine.run_frame();
    }
}

fn center_pixel(machine: &GbaMachine) -> (u8, u8, u8) {
    let mut rgb = Vec::new();
    machine.frame_rgb(&mut rgb);
    assert_eq!(rgb.len(), 240 * 160 * 3);
    let i = (80 * 240 + 120) * 3;
    (rgb[i], rgb[i + 1], rgb[i + 2])
}

fn is_red((r, g, b): (u8, u8, u8)) -> bool {
    r > 200 && g < 16 && b < 16
}

#[test]
fn embedded_bios_is_16k() {
    assert_eq!(OPEN_SOURCE_BIOS.len(), BIOS_SIZE);
}

#[test]
fn rejects_bad_bios() {
    assert!(GbaMachine::new(&[0u8; 100], &red_screen_rom(), true).is_err());
}

#[test]
fn rejects_bad_rom() {
    assert!(GbaMachine::new(OPEN_SOURCE_BIOS, &[0u8; 16], true).is_err());
}

#[test]
fn skip_bios_runs_rom() {
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    run(&mut m, 10);
    assert!(is_red(center_pixel(&m)), "pixel: {:?}", center_pixel(&m));
    assert_eq!(m.gba.get_game_code(), "TEST");
}

#[test]
fn open_source_bios_boots_rom() {
    // Without skip_bios: the free BIOS plays its boot animation, then jumps to the ROM.
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), false).unwrap();
    run(&mut m, 60 * 10);
    assert!(is_red(center_pixel(&m)), "pixel: {:?}", center_pixel(&m));
}

#[test]
fn produces_audio_at_44100() {
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    let mut audio = Vec::new();
    for _ in 0..60 {
        m.run_frame();
        m.drain_audio(&mut audio);
    }
    // 1 second of stereo sound = ~2 * 44100 samples (2% tolerance).
    let expected = 2.0 * SAMPLE_RATE as f64 * 60.0 / GBA_FPS;
    let got = audio.len() as f64;
    assert!((got - expected).abs() / expected < 0.02, "samples: {got}, expected ~{expected}");
}

#[test]
fn filtered_audio_frames_are_stereo_and_clamped() {
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    let mut frames = Vec::new();
    for _ in 0..60 {
        m.run_frame();
        m.drain_audio_frames(|l, r| frames.push((l, r)));
    }
    // 1 second = ~44100 stereo frames (2% tolerance).
    let expected = SAMPLE_RATE as f64 * 60.0 / GBA_FPS;
    let got = frames.len() as f64;
    assert!((got - expected).abs() / expected < 0.02, "frames: {got}, expected ~{expected}");
    assert!(frames.iter().all(|&(l, r)| (-1.0..=1.0).contains(&l) && (-1.0..=1.0).contains(&r)));
}

#[test]
fn frame_skip_does_not_change_the_game() {
    // Skipping the drawing must not change the emulated machine: after the same
    // number of frames, a run with frame skip shows exactly the same picture and
    // produced exactly the same sound as a run without it.
    let Ok(rom) = std::fs::read("GodotProject/games/los-burros.gba") else { return };
    let mut a = GbaMachine::new(OPEN_SOURCE_BIOS, &rom, true).unwrap();
    let mut b = GbaMachine::new(OPEN_SOURCE_BIOS, &rom, true).unwrap();
    let (mut audio_a, mut audio_b) = (Vec::new(), Vec::new());
    for i in 0..600 {
        a.run_frame();
        if i % 3 == 2 { b.run_frame() } else { b.run_frame_no_video() }
        a.drain_audio_frames(|l, r| audio_a.push((l, r)));
        b.drain_audio_frames(|l, r| audio_b.push((l, r)));
    }
    let (mut rgb_a, mut rgb_b) = (Vec::new(), Vec::new());
    a.frame_rgb(&mut rgb_a);
    b.frame_rgb(&mut rgb_b);
    assert!(rgb_a == rgb_b, "picture differs with frame skip");
    assert!(audio_a == audio_b, "sound differs with frame skip");
}

#[test]
fn emulation_thread_runs_in_real_time() {
    let machine = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    let Ok(thread) = EmuThread::spawn(machine) else { panic!("threads not available") };
    std::thread::sleep(std::time::Duration::from_millis(1000));
    thread.set_paused(true);
    std::thread::sleep(std::time::Duration::from_millis(50));

    // ~1 second of real time = ~60 GBA frames and ~44100 stereo sound frames.
    let frames = thread.frame_count();
    assert!((50..=66).contains(&frames), "frames in 1 s: {frames}");
    let mut audio = 0;
    thread.take_audio(|_, _| audio += 1);
    assert!((18000..=22050).contains(&audio), "queued sound is capped at ~0.5 s: {audio}");

    // The picture reaches the Godot side, and only once per new frame.
    let (mut rgb, mut serial) = (Vec::new(), 0);
    assert!(thread.take_video(&mut rgb, &mut serial));
    assert_eq!(rgb.len(), 240 * 160 * 3);
    let center = (80 * 240 + 120) * 3;
    assert!(rgb[center] > 200 && rgb[center + 1] < 50 && rgb[center + 2] < 50, "not red");
    assert!(!thread.take_video(&mut rgb, &mut serial));

    // Buttons, direct access (save states) and pause.
    thread.set_key(Keys::ButtonA, true);
    thread.set_paused(false);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(thread.with_machine(|m| *m.gba.get_key_state()) & 1, 0, "A not pressed");
    let state = thread.with_machine(|m| m.gba.save_state().unwrap());
    assert!(thread.with_machine(|m| m.gba.restore_state(&state).is_ok()));
    assert!(thread.take_emulation_ms() > 0.0);
}

#[test]
fn audio_stretcher_follows_the_emulation_speed() {
    let count = |st: &mut AudioStretcher, n: usize| {
        let mut out = 0;
        for i in 0..n {
            st.push(i as f32 / n as f32, 0.0, &mut |l, _| {
                assert!((0.0..=1.0).contains(&l));
                out += 1;
            });
        }
        out
    };
    // Full speed (44100 frames per second): nothing changes.
    let mut st = AudioStretcher::new();
    for _ in 0..5 { st.measure(0.5, 22050); }
    assert_eq!(st.ratio(), 1.0);
    assert_eq!(count(&mut st, 1000), 1000);
    // 70% speed: after a few seconds the sound is stretched to ~1/0.7.
    for _ in 0..16 { st.measure(0.5, 15435); }
    assert!((st.ratio() - 0.7).abs() < 0.01, "ratio: {}", st.ratio());
    let out = count(&mut st, 7000);
    assert!((9900..=10100).contains(&out), "stretched frames: {out}");
    // Back to full speed: normal pitch again.
    for _ in 0..4 { st.measure(0.5, 22050); }
    assert_eq!(st.ratio(), 1.0);
}

#[test]
fn keys_map_to_keyinput_bits() {
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    m.set_key(Keys::ButtonA, true);
    m.set_key(Keys::Start, true);
    m.run_frame();
    assert_eq!(*m.gba.get_key_state(), KEYINPUT_ALL_RELEASED & !(1 | 1 << 3));
    m.set_key(Keys::ButtonA, false);
    m.run_frame();
    assert_eq!(*m.gba.get_key_state(), KEYINPUT_ALL_RELEASED & !(1 << 3));
    m.release_all_keys();
    m.run_frame();
    assert_eq!(*m.gba.get_key_state(), KEYINPUT_ALL_RELEASED);

    for name in ["a", "B", "l", "R", "start", "SELECT", "up", "down", "left", "right"] {
        assert!(key_from_name(name).is_some(), "{name}");
    }
    assert!(key_from_name("X").is_none());
}

#[test]
fn save_and_restore_state() {
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    run(&mut m, 10);
    let state = m.gba.save_state().unwrap();
    assert!(!state.is_empty());

    // A fresh machine (screen not drawn yet) restores the state and keeps running.
    let mut other = GbaMachine::new(OPEN_SOURCE_BIOS, &red_screen_rom(), true).unwrap();
    other.gba.restore_state(&state).unwrap();
    run(&mut other, 2);
    assert!(is_red(center_pixel(&other)));
}

/// Every game in GodotProject/games/ (.gba/.zip) must boot and show an image.
#[test]
fn all_games_in_folder_boot() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/GodotProject/games");
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext != "gba" && ext != "zip" {
            continue;
        }
        let rom = std::fs::read(&path).unwrap();
        let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &rom, true)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        run(&mut m, 60 * 10);
        let mut rgb = Vec::new();
        m.frame_rgb(&mut rgb);
        let first = &rgb[0..3];
        let has_image = rgb.chunks_exact(3).any(|px| px != first);
        assert!(has_image, "{}: blank screen after 10 s", path.display());
        println!("OK: {} ({})", path.display(), m.gba.get_game_title());
    }
}

#[test]
#[ignore]
fn bench_emulation_speed() {
    let rom = std::fs::read("GodotProject/games/los-burros.gba").unwrap();
    let mut m = GbaMachine::new(OPEN_SOURCE_BIOS, &rom, true).unwrap();
    for _ in 0..120 { m.run_frame(); m.drain_audio_frames(|_, _| {}); }
    let t = std::time::Instant::now();
    let n: u32 = std::env::var("BENCH_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1200);
    let mut rgb = Vec::new();
    let skip: u32 = std::env::var("BENCH_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    for i in 0..n {
        if i % (skip + 1) == skip { m.run_frame(); m.frame_rgb(&mut rgb); } else { m.run_frame_no_video(); }
        m.drain_audio_frames(|_, _| {});
    }
    let s = t.elapsed().as_secs_f64();
    println!("BENCH: {:.0} fps native ({:.2} ms/frame, {:.1}x real time)", n as f64 / s, s * 1000.0 / n as f64, n as f64 / s / GBA_FPS);
}
