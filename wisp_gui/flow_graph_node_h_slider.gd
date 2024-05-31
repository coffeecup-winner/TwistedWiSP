extends "res://flow_graph_node.gd"


func _on_h_slider_value_changed(value):
	flow_node.set_data_value(value)
