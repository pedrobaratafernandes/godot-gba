extends Control

# Touch D-pad.
# The wiiu_dpad_*.png images are the whole cross with one direction in red.
# - "Base" shows the neutral cross (the shader paints the red part white).
# - "Highlight*" show only the red part of the pressed direction, so diagonals
#   (e.g. up + right) look right.
# - Touch areas are invisible TouchScreenButtons, one per arm of the cross;
#   the corners overlap to allow diagonals.

const DIRECTIONS := {
	"Up": &"gba_up",
	"Down": &"gba_down",
	"Left": &"gba_left",
	"Right": &"gba_right",
}

@onready var _highlights := {
	"Up": $Dpad/HighlightUp,
	"Down": $Dpad/HighlightDown,
	"Left": $Dpad/HighlightLeft,
	"Right": $Dpad/HighlightRight,
}

func _process(_delta):
	if not is_visible_in_tree(): return
	for dir in DIRECTIONS:
		_highlights[dir].visible = Input.is_action_pressed(DIRECTIONS[dir])
