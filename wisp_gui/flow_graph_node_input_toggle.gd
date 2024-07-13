extends FlowGraphNode


func _ready():
	super._ready()
	$CheckButton.toggle_mode = flow_node.get_property_value("value") > 0.0


func _on_check_button_toggled(toggled_on):
	var value = 0.0
	if toggled_on:
		value = 1.0
	flow_node.set_property_value("value", value)
