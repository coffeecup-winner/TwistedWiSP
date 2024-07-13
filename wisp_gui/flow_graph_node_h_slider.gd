extends FlowGraphNode


func _ready():
	super._ready()
	$HSlider.value = flow_node.get_property_value("value")


func _on_h_slider_value_changed(value):
	flow_node.set_property_value("value", value)


func _on_property_value_changed(prop_name, new_value):
	super._on_property_value_changed(prop_name, new_value)
	if prop_name == "value":
		$HSlider.value = new_value


func _process(_delta):
	var values = flow_node.get_watch_updates()
	if len(values) > 0:
		$HSlider.value = values[-1]
