extends "res://flow_graph_node.gd"

signal value_changed


func _on_button_button_down():
	value_changed.emit(wisp_node_idx, 1.0)


func _on_button_button_up():
	value_changed.emit(wisp_node_idx, 0.0)
