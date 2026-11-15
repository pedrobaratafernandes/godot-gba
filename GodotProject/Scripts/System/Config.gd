extends Node

# ==============================================================================
# Central configuration
# ------------------------------------------------------------------------------
# Holds the BIOS and games paths, and the game chosen in the Launcher
# (shared between scenes, because this script is an Autoload).
# ==============================================================================

# --- BIOS ---

## Official GBA BIOS (16 KB). Optional: if it doesn't exist, the emulator uses
## the free Cult-of-GBA BIOS embedded in the library.
static var BIOS_PATH: String = "res://roms/gba_bios.bin"

## Size of a GBA BIOS.
const BIOS_SIZE := 16384

## Returns the official BIOS if it exists and is valid; otherwise an empty
## array (= use the embedded BIOS).
func get_bios_data() -> PackedByteArray:
	if BIOS_PATH != "" and FileAccess.file_exists(BIOS_PATH):
		var data := FileAccess.get_file_as_bytes(BIOS_PATH)
		if data.size() == BIOS_SIZE:
			return data
		push_error("[Config] Invalid BIOS at %s: %d bytes (expected %d). Using the embedded BIOS." % [BIOS_PATH, data.size(), BIOS_SIZE])
	return PackedByteArray()


# --- GAMES ---

## Folder containing the games (.gba or .zip).
static var GAMES_DIR: String = "res://games/"

## Accepted game extensions.
const GAME_EXTENSIONS := ["gba", "zip"]

## Full path of the game chosen in the Launcher (used by the emulator scene).
static var selected_game_path: String = ""
