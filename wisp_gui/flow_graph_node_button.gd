extends FlowGraphNode


func _on_button_button_down():
	flow_node.set_property_value("value", 1.0)


func _on_button_button_up():
	flow_node.set_property_value("value", 0.0)
