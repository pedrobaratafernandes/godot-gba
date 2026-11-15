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
