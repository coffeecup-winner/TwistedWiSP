extends "res://flow_graph_node.gd"

signal value_changed


func _on_check_button_toggled(toggled_on):
	var value = 0.0
	if toggled_on:
		value = 1.0
	value_changed.emit(wisp_node_idx, value)
