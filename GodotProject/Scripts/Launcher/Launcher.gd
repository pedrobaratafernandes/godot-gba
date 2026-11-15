extends Control

# The Launcher is the first scene the user sees.
# It lists the games in 'res://games/' (.gba, .zip) and starts the emulator with the chosen one.

@onready var btn_play: Button = %BtnPlay
@onready var game_list: ItemList = %GameList
@onready var bios_label: Label = %BiosLabel

# Internal list with the file name for each row of the list.
var game_files: Array[String] = []

@export_category("Configuration")
## Official GBA BIOS (16 KB). Leave empty to use res://roms/gba_bios.bin
## if it exists, or the embedded free Cult-of-GBA BIOS.
@export_file("*.bin") var bios_path: String = ""

func _ready():
	if bios_path != "":
		Config.BIOS_PATH = bios_path

	# 1. Button and list
	btn_play.pressed.connect(_on_play_pressed)
	game_list.item_selected.connect(_on_game_selected)
	# Double click / Enter on the list starts the game right away
	game_list.item_activated.connect(func(_i): _on_play_pressed())

	# 2. Button animations (hover/focus)
	btn_play.mouse_entered.connect(_on_button_hover.bind(btn_play))
	btn_play.mouse_exited.connect(_on_button_unhover.bind(btn_play))
	btn_play.focus_entered.connect(_on_button_hover.bind(btn_play))
	btn_play.focus_exited.connect(_on_button_unhover.bind(btn_play))
	btn_play.pressed.connect(_on_button_pressed.bind(btn_play))

	# 3. Which BIOS will be used
	if Config.get_bios_data().size() > 0:
		bios_label.text = "BIOS: " + Config.BIOS_PATH.get_file()
	else:
		bios_label.text = "BIOS: Cult-of-GBA (open-source)"

	# 4. Look for games
	_scan_games()

	# 5. Initial focus for keyboard/gamepad
	if game_files.size() > 0:
		game_list.select(0)
		_on_game_selected(0)
		game_list.focus_next = btn_play.get_path()
		btn_play.focus_neighbor_left = game_list.get_path()
		game_list.grab_focus()
	else:
		btn_play.disabled = true

	# 6. Simple fade-in
	modulate.a = 0
	var tween = create_tween()
	tween.tween_property(self, "modulate:a", 1.0, 0.4)

func _scan_games():
	game_list.clear()
	game_files.clear()

	var dir = DirAccess.open(Config.GAMES_DIR)
	if dir:
		var names: Array[String] = []
		dir.list_dir_begin()
		var file_name = dir.get_next()
		while file_name != "":
			if not dir.current_is_dir():
				var ext = file_name.get_extension().to_lower()
				if ext in Config.GAME_EXTENSIONS:
					names.append(file_name)
			file_name = dir.get_next()
		dir.list_dir_end()
		names.sort()
		for n in names:
			game_files.append(n)
			game_list.add_item(n.get_basename())

	if game_files.is_empty():
		game_list.add_item("No games found in " + Config.GAMES_DIR)

func _on_game_selected(index):
	if index < game_files.size():
		var file_path = Config.GAMES_DIR + game_files[index]
		Config.selected_game_path = file_path
		print("Selected game: ", Config.selected_game_path)
		_update_preview(file_path)

## Shows an image with the same name as the game (e.g. game.png next to game.gba).
func _update_preview(file_path: String):
	var preview_node: TextureRect = %GamePreview
	if not preview_node: return

	var base_path = file_path.get_basename()
	for ext in ["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"]:
		var img_path = base_path + "." + ext
		if ResourceLoader.exists(img_path):
			# Image imported by Godot (works in exports)
			preview_node.texture = load(img_path)
			return
		if FileAccess.file_exists(img_path):
			var img = Image.load_from_file(img_path)
			if img:
				preview_node.texture = ImageTexture.create_from_image(img)
				return

	preview_node.texture = null

func _on_play_pressed():
	if Config.selected_game_path == "": return
	await get_tree().create_timer(0.3).timeout
	get_tree().change_scene_to_file("res://Scenes/Emulator/MainGBA.tscn")

# --- Button animations ---

func _on_button_pressed(btn: Button):
	_stop_pulse(btn)
	btn.pivot_offset = btn.size / 2

	var tween = create_tween()
	tween.tween_property(btn, "scale", Vector2(1.2, 0.8), 0.1).set_trans(Tween.TRANS_SINE)
	tween.tween_property(btn, "scale", Vector2(0.9, 1.1), 0.1).set_trans(Tween.TRANS_SINE)
	tween.tween_property(btn, "scale", Vector2(1.0, 1.0), 0.2).set_trans(Tween.TRANS_ELASTIC).set_ease(Tween.EASE_OUT)

	await tween.finished
	if btn.is_hovered() or btn.has_focus():
		_start_pulse(btn)

func _on_button_hover(btn: Button):
	btn.grab_focus()
	_start_pulse(btn)

func _on_button_unhover(btn: Button):
	_stop_pulse(btn)
	var tween = create_tween()
	tween.tween_property(btn, "scale", Vector2(1.0, 1.0), 0.2).set_trans(Tween.TRANS_SINE)

func _start_pulse(btn: Button):
	if btn.has_meta("pulse_tween"):
		return
	btn.pivot_offset = btn.size / 2

	var tween = create_tween().set_loops()
	tween.tween_property(btn, "scale", Vector2(1.05, 1.05), 0.6).set_trans(Tween.TRANS_SINE)
	tween.tween_property(btn, "scale", Vector2(1.0, 1.0), 0.6).set_trans(Tween.TRANS_SINE)
	btn.set_meta("pulse_tween", tween)

func _stop_pulse(btn: Button):
	if btn.has_meta("pulse_tween"):
		var old_tween = btn.get_meta("pulse_tween")
		if old_tween: old_tween.kill()
		btn.remove_meta("pulse_tween")
