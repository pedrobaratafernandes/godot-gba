extends Control

# ==============================================================================
# Game Boy Advance emulator controller
# ------------------------------------------------------------------------------
# Connects the GBAEmulator node (Rust) to the screen, sound, controls and menu.
# ==============================================================================

@onready var emulator: GBAEmulator = $GBAEmulator
@onready var display: TextureRect = $Display
@onready var audio_player: AudioStreamPlayer = $AudioStreamPlayer
@onready var menu: CanvasLayer = $GameMenu

## Godot action -> GBA button (see Project Settings > Input Map).
const BUTTON_ACTIONS := {
	&"gba_a": "A",
	&"gba_b": "B",
	&"gba_l": "L",
	&"gba_r": "R",
	&"gba_start": "START",
	&"gba_select": "SELECT",
	&"gba_up": "UP",
	&"gba_down": "DOWN",
	&"gba_left": "LEFT",
	&"gba_right": "RIGHT",
}

@export_group("Game")
## Game used when this scene runs on its own (F6), without the Launcher.
## If empty, the first game found in res://games/ is used.
@export_file("*.gba", "*.zip") var default_game: String = ""

@export_group("Emulation")
## Skip the BIOS boot animation.
@export var skip_bios: bool = true
## Speed while the fast-forward key (Tab) is held.
@export_range(2, 4) var fast_forward_speed: int = 3

var audio_playback: AudioStreamGeneratorPlayback
var audio_buffer: PackedVector2Array = []

# Safety margin (in audio frames) kept queued in the AudioStreamGenerator to
# prevent audio stutter (underruns)
var min_buffer_frames = 1024

# True once the safety margin has been built up. Reset after a pause, reset or
# underrun, so the margin is rebuilt before the sound resumes.
var _audio_primed := false
# Underrun counter seen last frame (AudioStreamGeneratorPlayback.get_skips()).
var _audio_last_skips := 0
# Total size of the generator's buffer, in frames.
var _generator_capacity := 0

# Stats overlay (F3, or open the Web page with ?stats in the URL)
var _stats_label: Label
var _stats_time := 0.0
var _stats_frames := 0

func _ready():
	print("[GBA] Initializing emulator...")
	_setup_audio()
	_setup_stats()

	menu.launcher_requested.connect(_on_launcher_requested)
	menu.save_requested.connect(_save_state)
	menu.load_requested.connect(_load_state)
	menu.reset_requested.connect(_on_reset_requested)
	menu.resume_requested.connect(_toggle_pause)
	menu.quit_requested.connect(_on_quit_requested)
	menu.fullscreen_requested.connect(_toggle_fullscreen)

	if DisplayServer.is_touchscreen_available():
		if has_node("VirtualControls"): $VirtualControls.show()
		if has_node("GBAButtons"): $GBAButtons.show()
		if has_node("MenuButton"): $MenuButton.show()

	_load_game()

func _setup_audio():
	if not audio_player: return
	if OS.has_feature("mobile") or OS.has_feature("web_android") or OS.has_feature("web_ios"):
		min_buffer_frames = 4096  # ~93ms (safe on phones)
	else:
		min_buffer_frames = 1024  # ~23ms

	var gen = AudioStreamGenerator.new()
	gen.mix_rate = 44100.0  # Same as SAMPLE_RATE in Rust
	gen.buffer_length = 0.15 if min_buffer_frames <= 1024 else 0.5
	audio_player.stream = gen
	# The Web defaults to "Sample" playback, which an AudioStreamGenerator cannot use.
	audio_player.playback_type = AudioServer.PLAYBACK_TYPE_STREAM
	audio_player.play()
	audio_playback = audio_player.get_stream_playback()
	_generator_capacity = audio_playback.get_frames_available()

func _load_game():
	emulator.skip_bios = skip_bios

	var bios = Config.get_bios_data()
	if bios.size() > 0:
		emulator.load_bios(bios)
	else:
		emulator.use_open_source_bios()

	# Scene run on its own (F6): no game came from the Launcher
	if Config.selected_game_path == "":
		Config.selected_game_path = _find_fallback_game()

	var game_path = Config.selected_game_path
	var game_name = game_path.get_file() if game_path != "" else "None"

	if game_path == "" or not FileAccess.file_exists(game_path):
		push_error("[GBA] Game file not found: " + game_path)
		menu.set_status("Game not found: " + game_name)
		return

	if emulator.load_rom(FileAccess.get_file_as_bytes(game_path)):
		var title = emulator.get_game_title()
		menu.set_status("GBA | %s (%s)" % [game_name, title if title != "" else "?"])
	else:
		menu.set_status("Failed to load: " + game_name)

## Game from the command line (`godot ... -- --game=res://games/x.gba`, used by CI),
## from the Inspector (default_game), or the first one in res://games/ alphabetically.
func _find_fallback_game() -> String:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--game="):
			return arg.trim_prefix("--game=")
	if default_game != "":
		return default_game
	var names: Array[String] = []
	var dir = DirAccess.open(Config.GAMES_DIR)
	if dir:
		for file_name in dir.get_files():
			if file_name.get_extension().to_lower() in Config.GAME_EXTENSIONS:
				names.append(file_name)
	if names.is_empty():
		return ""
	names.sort()
	print("[GBA] No game from the Launcher, using: ", names[0])
	return Config.GAMES_DIR + names[0]

func _process(delta):
	_update_stats(delta)
	if emulator.is_paused(): return
	var tex = emulator.get_texture()
	if tex and display.texture != tex:
		display.texture = tex
	_update_audio()

func _update_audio():
	if not audio_playback: return

	# 1. Fetch ALL pending audio from the emulator (already filtered in Rust)
	audio_buffer.append_array(emulator.get_audio_frames())

	# 2. Underrun: the generator ran dry -> rebuild the safety margin first
	var skips = audio_playback.get_skips()
	if skips != _audio_last_skips:
		_audio_last_skips = skips
		_audio_primed = false

	# 3. Jitter buffer: wait for the margin ONCE, then keep the generator fed
	# every frame. (Waiting on every push left the generator almost empty.)
	if not _audio_primed:
		if audio_buffer.size() < min_buffer_frames: return
		_audio_primed = true

	# 4. Push samples to Godot, keeping at most 2x the margin queued
	var queued = _generator_capacity - audio_playback.get_frames_available()
	var room = max(min_buffer_frames * 2 - queued, 0)
	var to_push = min(audio_buffer.size(), room)
	if to_push > 0:
		audio_playback.push_buffer(audio_buffer.slice(0, to_push))
		audio_buffer = audio_buffer.slice(to_push)
	_audio_last_skips = audio_playback.get_skips()

	# 5. Latency control: drop the oldest audio if it piles up (fast forward,
	# or the emulator running slightly faster than the sound card)
	if audio_buffer.size() > min_buffer_frames:
		audio_buffer = audio_buffer.slice(audio_buffer.size() - min_buffer_frames)

func _reset_audio():
	audio_buffer.clear()
	_audio_primed = false

func _setup_stats():
	var layer = CanvasLayer.new()
	layer.layer = 100
	add_child(layer)
	_stats_label = Label.new()
	_stats_label.position = Vector2(8, 8)
	_stats_label.add_theme_color_override("font_color", Color.YELLOW)
	_stats_label.add_theme_color_override("font_outline_color", Color.BLACK)
	_stats_label.add_theme_constant_override("outline_size", 4)
	_stats_label.add_theme_font_size_override("font_size", 22)
	layer.add_child(_stats_label)
	# Shown by default in Web debug builds ("Run in Browser"), so it can be read
	# on a phone; in release builds only with ?stats in the URL (or F3).
	var show_stats := false
	if OS.has_feature("web"):
		show_stats = OS.is_debug_build() \
			or str(JavaScriptBridge.eval("window.location.search", true)).contains("stats")
	_stats_label.visible = show_stats

func _update_stats(delta: float):
	if not _stats_label or not _stats_label.visible: return
	_stats_time += delta
	if _stats_time < 0.5: return
	var frames = emulator.get_frame_count()
	var emu_fps = (frames - _stats_frames) / _stats_time
	_stats_frames = frames
	_stats_time = 0.0
	var audio_line: String
	if audio_playback:
		var queued = _generator_capacity - audio_playback.get_frames_available()
		audio_line = "Godot audio | queued %d ms | underruns %d | sound speed %d%%" % [
			int(queued / 44.1), audio_playback.get_skips(), roundi(emulator.get_audio_speed() * 100.0)]
	else:
		audio_line = "no audio"
	# Emulation time per GBA frame vs. the whole Godot frame: shows whether the
	# phone is slowed down by the emulator or by Godot (drawing, UI).
	_stats_label.text = "FPS %d | GBA %.1f fps (%d%%)\nemu %.1f ms/frame (%s) | Godot frame %.1f ms\n%s" % [
		Engine.get_frames_per_second(), emu_fps, roundi(emu_fps / 59.73 * 100.0),
		emulator.get_emulation_ms(), "own thread" if emulator.is_threaded() else "main thread",
		1000.0 / max(Engine.get_frames_per_second(), 1), audio_line]

func _toggle_pause():
	var new_state = not emulator.is_paused()
	emulator.set_paused(new_state)
	if new_state:
		menu.show()
	else:
		menu.hide()
		_reset_audio()

func _get_save_path() -> String:
	var game_path = Config.selected_game_path
	if game_path == "": return "user://saves/default.gbastate"
	var safe_name = game_path.replace("res://", "").replace("/", "_").replace(":", "_").replace(".", "_")
	return "user://saves/" + safe_name + ".gbastate"

func _save_state():
	if not DirAccess.dir_exists_absolute("user://saves"):
		DirAccess.make_dir_recursive_absolute("user://saves")
	var data = emulator.save_state()
	if data.size() > 0:
		var file = FileAccess.open(_get_save_path(), FileAccess.WRITE)
		if file:
			file.store_buffer(data)
			file.close()
			print("[GBA] State saved: ", _get_save_path())
	_toggle_pause()

func _load_state():
	var path = _get_save_path()
	if FileAccess.file_exists(path):
		var data = FileAccess.get_file_as_bytes(path)
		if data.size() > 0 and emulator.load_state(data):
			print("[GBA] State loaded: ", path)
	_toggle_pause()

func _toggle_fullscreen():
	var current_mode = DisplayServer.window_get_mode()
	if current_mode == DisplayServer.WINDOW_MODE_FULLSCREEN or current_mode == DisplayServer.WINDOW_MODE_EXCLUSIVE_FULLSCREEN:
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_WINDOWED)
	else:
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)

func _input(event: InputEvent):
	if event is InputEventKey and event.pressed and not event.echo and event.keycode == KEY_F3:
		_stats_label.visible = not _stats_label.visible
		_stats_frames = emulator.get_frame_count()
		_stats_time = 0.0
		return

	# Don't use ui_cancel: it includes the gamepad B button, which is the GBA A button here.
	if event.is_action_pressed("gba_menu"):
		_toggle_pause()
		get_viewport().set_input_as_handled()
		return

	if emulator.is_paused(): return

	# Fast forward while the key is held
	if event.is_action("gba_fast_forward"):
		emulator.speed = fast_forward_speed if event.is_pressed() else 1
		get_viewport().set_input_as_handled()
		return

	for action in BUTTON_ACTIONS:
		if event.is_action(action):
			emulator.send_button(BUTTON_ACTIONS[action], event.is_pressed())
			get_viewport().set_input_as_handled()
			return

func _on_launcher_requested():
	get_tree().change_scene_to_file("res://Scenes/Launcher/Launcher.tscn")

func _on_reset_requested():
	emulator.reset()
	_reset_audio()
	_toggle_pause()

func _on_quit_requested():
	get_tree().quit()
