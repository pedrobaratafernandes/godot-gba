use godot::prelude::*;

// 'host' connects the rustboyadvance core to Godot (BIOS, audio, screen, buttons)
// 'emulator_node' contains the Godot Node used in scenes (GBAEmulator)
mod emulator_node;
mod host;

#[cfg(test)]
mod tests;

/// Main structure of the GDExtension library.
/// It is the "anchor" Godot looks for when loading the library.
struct GodotGBA;

/// The #[gdextension] macro generates the entry point (gdext_rust_init)
/// that registers every #[derive(GodotClass)] class in the modules.
#[gdextension]
unsafe impl ExtensionLibrary for GodotGBA {}
