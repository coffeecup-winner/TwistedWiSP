extends FlowGraphNode


func _ready():
	super._ready()
	$HSlider.value = flow_node.get_data_value()


func _on_h_slider_value_changed(value):
	flow_node.set_data_value(value)


func _process(_delta):
	var values = flow_node.get_watch_updates()
	if len(values) > 0:
		$HSlider.value = values[-1]
