extends FlowGraphNode


func _on_check_button_toggled(toggled_on):
	var value = 0.0
	if toggled_on:
		value = 1.0
	flow_node.set_data_value(value)
