extends "res://flow_graph_node.gd"

signal value_changed

func _on_h_slider_value_changed(value):
	value_changed.emit(wisp_node_idx, value)
